//! USB Ethernet adapters of the communications class: the Ethernet
//! Control Model (CDC ECM, one frame per bulk transfer) and the Network
//! Control Model (CDC NCM, frames packed into NTB16 transfer blocks).
//!
//! `xhci` finds the function (communications interface with its union
//! and Ethernet functional descriptors, data interface alternate setting
//! with the bulk endpoints), reads the MAC address string, sets NCM's
//! receive block size, selects the data interface and the packet filter,
//! and runs the transfers. This module frames what the network stack
//! sends and unpacks what arrives. Link state comes from the
//! NETWORK_CONNECTION notifications; until the first, the link is up.
//!
//! References: USB CDC 1.2 (ECM 1.2, NCM 1.0 specifications) and, for
//! what real devices expect, Linux's cdc_ether and cdc_ncm.

use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU16, Ordering};

use super::usb::NtbParams;
use super::xhci::{self, Handle, NetState};
use crate::net::Nic;

const NTH16_SIG: u32 = u32::from_le_bytes(*b"NCMH");
const NDP16_SIG: u32 = u32::from_le_bytes(*b"NCM0");
/// NDP16 with CRC-32 appended to each datagram (the CRC is not checked).
const NDP16_CRC_SIG: u32 = u32::from_le_bytes(*b"NCM1");
const NTH16_LEN: usize = 12;
/// NDP16 header and room for one datagram and the terminating entry.
const NDP16_LEN: usize = 8 + 2 * 4;

pub struct UsbNic {
    h: Handle,
    state: Arc<NetState>,
    mac: [u8; 6],
    mtu: usize,
    /// NCM: the transfer block parameters (None: ECM).
    ntb: Option<NtbParams>,
    seq: AtomicU16,
    name: String,
}

/// A configured adapter: hand it to the network stack.
/// Called with the controller lock held: the NIC's state is read from
/// `state`, never through the controller.
pub fn attach(h: Handle, state: Arc<NetState>, mac: [u8; 6], ntb: Option<NtbParams>, mtu: usize, name: String) {
    crate::net::register(Arc::new(UsbNic { h, state, mac, mtu, ntb, seq: AtomicU16::new(0), name }));
}

/// Build an NTB16 holding `frame` in `buf`; returns its length (0 if it
/// does not fit).
fn ntb16(buf: &mut [u8], frame: &[u8], p: &NtbParams, seq: u16, mps: usize) -> usize {
    let align = (p.out_align as usize).clamp(4, 64).next_power_of_two();
    let div = (p.out_divisor as usize).clamp(1, 512);
    let rem = p.out_remainder as usize % div;
    let ndp = NTH16_LEN.next_multiple_of(align);
    let mut dg = ndp + NDP16_LEN;
    // The datagram where (offset % divisor) == remainder.
    dg += (rem + div - dg % div) % div;
    let mut block = dg + frame.len();
    // A block of whole packets would need a zero-length packet to end.
    if mps > 0 && block % mps == 0 {
        block += 1;
    }
    let max = if p.out_max >= 2048 { p.out_max as usize } else { 2048 };
    if block > buf.len() || block > max {
        return 0;
    }
    buf[..dg].fill(0);
    buf[dg..dg + frame.len()].copy_from_slice(frame);
    buf[dg + frame.len()..block].fill(0);
    let w16 = |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    buf[0..4].copy_from_slice(&NTH16_SIG.to_le_bytes());
    w16(buf, 4, NTH16_LEN as u16);
    w16(buf, 6, seq);
    w16(buf, 8, block as u16);
    w16(buf, 10, ndp as u16);
    buf[ndp..ndp + 4].copy_from_slice(&NDP16_SIG.to_le_bytes());
    w16(buf, ndp + 4, NDP16_LEN as u16);
    w16(buf, ndp + 6, 0);
    w16(buf, ndp + 8, dg as u16);
    w16(buf, ndp + 10, frame.len() as u16);
    block
}

/// Call `f` with each datagram of an NTB16; returns how many there were.
fn parse_ntb16(b: &[u8], f: &mut dyn FnMut(&[u8])) -> usize {
    let r16 = |o: usize| b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as usize);
    let r32 = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    if r32(0) != Some(NTH16_SIG) || r16(4) != Some(NTH16_LEN) {
        return 0;
    }
    let block = r16(8).unwrap_or(0).min(b.len());
    let b = &b[..block];
    let mut ndp = r16(10).unwrap_or(0);
    let mut count = 0;
    // NDPs chain; a bounded walk guards against loops.
    for _ in 0..16 {
        if ndp < NTH16_LEN || ndp % 4 != 0 {
            break;
        }
        let sig = r32(ndp);
        let crc = sig == Some(NDP16_CRC_SIG);
        if sig != Some(NDP16_SIG) && !crc {
            break;
        }
        let Some(len) = r16(ndp + 4) else { break };
        let end = (ndp + len).min(b.len());
        let mut e = ndp + 8;
        while e + 4 <= end {
            let (Some(off), Some(n)) = (r16(e), r16(e + 2)) else { break };
            if off == 0 || n == 0 {
                break;
            }
            let n = if crc { n.saturating_sub(4) } else { n };
            if let Some(dg) = b.get(off..off + n) {
                f(dg);
                count += 1;
            }
            e += 4;
        }
        ndp = r16(ndp + 6).unwrap_or(0);
        if ndp == 0 {
            break;
        }
    }
    count
}

impl Nic for UsbNic {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn link_up(&self) -> bool {
        self.state.alive.load(Ordering::Acquire) && self.state.link.load(Ordering::Acquire)
    }

    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send(&self, frame: &[u8]) -> bool {
        match &self.ntb {
            None => xhci::net_send(self.h, |buf, mps| {
                let mut len = frame.len().min(buf.len() - 1);
                buf[..len].copy_from_slice(&frame[..len]);
                // As Linux: a padding byte rather than a zero-length
                // packet ends a transfer of whole packets.
                if mps > 0 && len % mps == 0 {
                    buf[len] = 0;
                    len += 1;
                }
                len
            }),
            Some(p) => {
                let seq = self.seq.fetch_add(1, Ordering::Relaxed);
                xhci::net_send(self.h, |buf, mps| ntb16(buf, frame, p, seq, mps))
            }
        }
    }

    fn receive(&self, f: &mut dyn FnMut(&[u8])) -> usize {
        let mut count = 0;
        for t in xhci::net_take(self.h) {
            if self.ntb.is_some() {
                count += parse_ntb16(&t, f);
            } else if t.len() >= 14 {
                f(&t);
                count += 1;
            }
        }
        count
    }

    fn rx_pending(&self) -> bool {
        self.state.rx.load(Ordering::Acquire)
    }

    fn interrupt_driven(&self) -> bool {
        // Completions call net::kick wherever the event ring is read.
        true
    }

    fn gone(&self) -> bool {
        !self.state.alive.load(Ordering::Acquire)
    }
}
