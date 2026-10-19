//! virtio-net: the paravirtual network adapter.
//!
//! Queue 0 receives, queue 1 transmits. Every packet is preceded by a
//! `virtio_net_hdr` (12 bytes for virtio 1.x devices, 10 for legacy ones
//! without MRG_RXBUF); no offloads are negotiated, so it is all zeros on
//! transmit and ignored on receive. Buffers are two-descriptor chains
//! (header, frame), the layout legacy devices without ANY_LAYOUT need.
//! The receive queue is kept full of [`SLOT`]-byte buffers; transmit
//! slots are recycled as the device returns them. Features: MAC (the
//! address from the configuration space) and STATUS (link state);
//! without MAC a locally administered address is made up.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::net::Nic;
use crate::sync::SpinLock;

const F_MAC: u64 = 1 << 5;
const F_STATUS: u64 = 1 << 16;

const CFG_MAC: usize = 0;
const CFG_STATUS: usize = 6;
const S_LINK_UP: u16 = 1;

const RX: u16 = 0;
const TX: u16 = 1;
const QUEUE: u16 = 256;
/// Per-buffer memory: header at 0, frame at 16.
const SLOT: usize = 2048;
const DATA: usize = 16;
const MAX_FRAME: usize = SLOT - DATA;

struct Queues {
    rx: Virtqueue,
    rxmem: Dma,
    tx: Virtqueue,
    txmem: Dma,
    /// Transmit slots not in use.
    tx_free: Vec<usize>,
}

pub struct VirtioNet {
    name: String,
    mac: [u8; 6],
    hdr_len: usize,
    has_status: bool,
    q: SpinLock<Queues>,
    dev: Device,
}

unsafe impl Send for VirtioNet {}
unsafe impl Sync for VirtioNet {}

impl Queues {
    fn post_rx(&mut self, slot: usize, hdr_len: usize) {
        let _ = self.rx.add(&[Buf::into(self.rxmem.addr(slot * SLOT), hdr_len), Buf::into(self.rxmem.addr(slot * SLOT + DATA), MAX_FRAME)]);
    }

    /// Take back the transmit buffers the device is done with.
    fn reclaim_tx(&mut self) {
        while let Some(u) = self.tx.pop() {
            let slot = ((u.addr - self.txmem.addr(0)) as usize) / SLOT;
            self.tx_free.push(slot);
        }
    }
}

/// Set up a virtio-net device.
pub fn attach(mut dev: Device) {
    if dev.negotiate(F_MAC | F_STATUS).is_none() {
        return;
    }
    let hdr_len = if dev.is_legacy() { 10 } else { 12 };
    let mac = if dev.has(F_MAC) {
        let mut m = [0u8; 6];
        dev.cfg_read(CFG_MAC, &mut m);
        m
    } else {
        // Locally administered, from the TSC.
        let t = crate::arch::rdtsc().to_le_bytes();
        [0x02, 0x52, t[0], t[1], t[2], t[3]]
    };
    let (Some(rx), Some(tx)) = (dev.queue(RX, QUEUE), dev.queue(TX, QUEUE)) else {
        crate::println!("virtio-net {}: cannot set up the queues", dev.name());
        dev.fail();
        return;
    };
    let rx_slots = rx.size() as usize / 2;
    let tx_slots = tx.size() as usize / 2;
    let (Some(rxmem), Some(txmem)) = (Dma::new(rx_slots * SLOT), Dma::new(tx_slots * SLOT)) else {
        dev.fail();
        return;
    };
    let mut q = Queues { rx, rxmem, tx, txmem, tx_free: (0..tx_slots).rev().collect() };
    for s in 0..rx_slots {
        q.post_rx(s, hdr_len);
    }
    dev.ready();
    q.rx.kick();
    let name = alloc::format!("virtio-net {}", dev.name());
    let has_status = dev.has(F_STATUS);
    let nic = VirtioNet { name, mac, hdr_len, has_status, q: SpinLock::new(q), dev };
    crate::net::register(Arc::new(nic));
}

impl Nic for VirtioNet {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn link_up(&self) -> bool {
        !self.has_status || self.dev.cfg16(CFG_STATUS) & S_LINK_UP != 0
    }

    fn send(&self, frame: &[u8]) -> bool {
        if frame.is_empty() || frame.len() > MAX_FRAME {
            return false;
        }
        let mut q = self.q.lock();
        q.reclaim_tx();
        let Some(slot) = q.tx_free.pop() else { return false };
        let off = slot * SLOT;
        q.txmem.write(off, &[0u8; DATA]);
        q.txmem.write(off + DATA, frame);
        let bufs = [Buf::out(q.txmem.addr(off), self.hdr_len), Buf::out(q.txmem.addr(off + DATA), frame.len())];
        if q.tx.add(&bufs).is_none() {
            q.tx_free.push(slot);
            return false;
        }
        q.tx.kick();
        true
    }

    fn receive(&self, f: &mut dyn FnMut(&[u8])) -> usize {
        let mut frames: Vec<Vec<u8>> = Vec::new();
        {
            let mut q = self.q.lock();
            while frames.len() < 64 {
                let Some(u) = q.rx.pop() else { break };
                let slot = ((u.addr - q.rxmem.addr(0)) as usize) / SLOT;
                let len = (u.len as usize).saturating_sub(self.hdr_len).min(MAX_FRAME);
                if len > 0 {
                    frames.push(q.rxmem.bytes(slot * SLOT + DATA, len).to_vec());
                }
                q.post_rx(slot, self.hdr_len);
            }
            if !frames.is_empty() {
                q.rx.kick();
            }
        }
        for fr in &frames {
            f(fr);
        }
        frames.len()
    }

    fn rx_pending(&self) -> bool {
        self.q.try_lock().is_some_and(|q| q.rx.has_used())
    }
}
