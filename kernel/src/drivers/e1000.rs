//! Intel 8254x and 82574 gigabit Ethernet ("e1000"/"e1000e"): the
//! default network adapter of QEMU (82540EM, 82574L on q35), VirtualBox
//! (82540EM, 82543GC, 82545EM) and VMware's e1000 (82545EM), and common
//! on real boards of the same era.
//!
//! The legacy descriptor format is used on all of them: a receive ring of
//! [`RX_DESCS`] 2 KB buffers kept owned by the hardware and a transmit
//! ring of [`TX_DESCS`] buffers frames are copied into. The MAC address
//! comes from the receive address registers the hardware loads from its
//! EEPROM at reset, or from the EEPROM itself. Adapters with an MSI
//! capability (the 82574L; QEMU's 82540EM/82545EM have none) interrupt
//! on received frames, receive overruns and link changes: the handler
//! acknowledges ICR and wakes the network thread. The others stay masked
//! and the timer tick polls the descriptors' DD bits (see `net`).

use alloc::string::String;
use alloc::sync::Arc;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::pci;
use super::virtio::queue::Dma;
use crate::mm::mmio;
use crate::net::Nic;
use crate::sync::SpinLock;

// Registers.
const CTRL: usize = 0x0000;
const STATUS: usize = 0x0008;
const EERD: usize = 0x0014;
const ICR: usize = 0x00c0;
const IMS: usize = 0x00d0;
const IMC: usize = 0x00d8;
const RCTL: usize = 0x0100;
const TCTL: usize = 0x0400;
const TIPG: usize = 0x0410;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH: usize = 0x2810;
const RDT: usize = 0x2818;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH: usize = 0x3810;
const TDT: usize = 0x3818;
const MTA: usize = 0x5200;
const RAL0: usize = 0x5400;
const RAH0: usize = 0x5404;

const CTRL_LRST: u32 = 1 << 3;
const CTRL_ASDE: u32 = 1 << 5;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_ILOS: u32 = 1 << 7;
const CTRL_RST: u32 = 1 << 26;
const CTRL_VME: u32 = 1 << 30;
const CTRL_PHY_RST: u32 = 1 << 31;
const STATUS_LU: u32 = 1 << 1;
const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const RAH_AV: u32 = 1 << 31;
/// Interrupt causes: link status change, receive descriptor minimum
/// threshold, receiver overrun, receive timer (frame received).
const INT_CAUSES: u32 = 1 << 2 | 1 << 4 | 1 << 6 | 1 << 7;

// Descriptor bits.
const TX_EOP: u8 = 1;
const TX_IFCS: u8 = 2;
const TX_RS: u8 = 8;
const DD: u8 = 1;
const RX_EOP: u8 = 2;
/// CRC, symbol, sequence, carrier extension and RX data errors.
const RX_FRAME_ERRORS: u8 = 0x97;

const RX_DESCS: usize = 128;
const TX_DESCS: usize = 64;
const BUF: usize = 2048;
const BAR_SIZE: u64 = 0x20000;

/// Supported devices: (device id, name, EEPROM read through EERD with
/// the newer layout (address << 2, done bit 1)).
const MODELS: &[(u16, &str, bool)] = &[
    (0x1000, "82542", false),
    (0x1001, "82543GC", false),
    (0x1004, "82543GC", false),
    (0x1008, "82544EI", false),
    (0x100c, "82544GC", false),
    (0x100e, "82540EM", false),
    (0x100f, "82545EM", false),
    (0x1010, "82546EB", false),
    (0x1011, "82545EM", false),
    (0x1015, "82540EM", false),
    (0x1016, "82540EP", false),
    (0x1017, "82540EP", false),
    (0x101e, "82540EP", false),
    (0x1026, "82545GM", false),
    (0x1027, "82545GM", false),
    (0x1028, "82545GM", false),
    (0x1075, "82547GI", true),
    (0x1076, "82541GI", true),
    (0x1078, "82541ER", true),
    (0x107c, "82541PI", true),
    (0x10d3, "82574L", true),
    (0x10f6, "82574LA", true),
];

pub fn model(d: &pci::Device) -> Option<(&'static str, bool)> {
    if d.vendor != 0x8086 {
        return None;
    }
    MODELS.iter().find(|m| m.0 == d.device).map(|m| (m.1, m.2))
}

struct Regs(*mut u8);

unsafe impl Send for Regs {}
unsafe impl Sync for Regs {}

impl Regs {
    fn r(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.0.add(off) as *const u32) }
    }

    fn w(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.0.add(off) as *mut u32, v) }
    }
}

struct Rings {
    rx: Dma,
    rxbuf: Dma,
    rx_next: usize,
    tx: Dma,
    txbuf: Dma,
    tx_next: usize,
}

impl Rings {
    fn rx_status(&self, i: usize) -> u8 {
        unsafe { read_volatile(self.rx.ptr(i * 16 + 12)) }
    }
}

pub struct E1000 {
    name: String,
    mac: [u8; 6],
    regs: Regs,
    rings: SpinLock<Rings>,
    /// Receive interrupts through MSI.
    irq: bool,
}

/// MSI handler: acknowledge the causes and wake the network thread. The
/// context is the register window.
fn interrupt(regs: usize) {
    let r = Regs(regs as *mut u8);
    let icr = r.r(ICR);
    r.w(ICR, icr);
    crate::net::kick();
}

fn delay_ms(ms: u64) {
    super::virtio::wait(ms, || {}, || false);
}

/// Read an EEPROM word through EERD.
fn eeprom(regs: &Regs, word: u32, new_layout: bool) -> Option<u16> {
    let (start, shift, done) = if new_layout { (1, 2, 1 << 1) } else { (1, 8, 1 << 4) };
    regs.w(EERD, start | word << shift);
    for _ in 0..100_000 {
        let v = regs.r(EERD);
        if v & done != 0 {
            return Some((v >> 16) as u16);
        }
        core::hint::spin_loop();
    }
    None
}

fn read_mac(regs: &Regs, new_layout: bool) -> Option<[u8; 6]> {
    let (lo, hi) = (regs.r(RAL0), regs.r(RAH0));
    let mut mac = [lo as u8, (lo >> 8) as u8, (lo >> 16) as u8, (lo >> 24) as u8, hi as u8, (hi >> 8) as u8];
    if hi & RAH_AV == 0 || mac == [0; 6] || mac == [0xff; 6] {
        for w in 0..3 {
            let v = eeprom(regs, w as u32, new_layout)?;
            mac[w * 2] = v as u8;
            mac[w * 2 + 1] = (v >> 8) as u8;
        }
        regs.w(RAL0, u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
        regs.w(RAH0, u16::from_le_bytes([mac[4], mac[5]]) as u32 | RAH_AV);
    }
    (mac != [0; 6] && mac[0] & 1 == 0).then_some(mac)
}

/// Set up an adapter and register it with the network layer.
pub fn probe(d: &pci::Device) {
    let Some((model, new_layout)) = model(d) else { return };
    let name = alloc::format!("e1000 {model} {:02x}:{:02x}.{}", d.bus, d.dev, d.func);
    if d.read(0x10) & 1 != 0 {
        crate::println!("{name}: BAR 0 is not memory");
        return;
    }
    d.enable();
    // Interrupts masked at the PCI level too (INTx disable).
    d.write(0x04, (d.read(0x04) & 0xffff) | 1 << 10);
    let base = d.bar(0);
    if base == 0 {
        return;
    }
    let regs = Regs(mmio::map(base, BAR_SIZE));
    regs.w(IMC, u32::MAX);
    regs.w(CTRL, regs.r(CTRL) | CTRL_RST);
    delay_ms(2);
    for _ in 0..100 {
        if regs.r(CTRL) & CTRL_RST == 0 {
            break;
        }
        delay_ms(1);
    }
    // Let the EEPROM auto-load finish.
    delay_ms(10);
    regs.w(IMC, u32::MAX);
    let _ = regs.r(ICR);
    let ctrl = regs.r(CTRL) & !(CTRL_LRST | CTRL_PHY_RST | CTRL_VME | CTRL_ILOS);
    regs.w(CTRL, ctrl | CTRL_SLU | CTRL_ASDE);
    let Some(mac) = read_mac(&regs, new_layout) else {
        crate::println!("{name}: no MAC address");
        return;
    };
    for i in 0..128 {
        regs.w(MTA + i * 4, 0);
    }
    let (Some(rx), Some(rxbuf), Some(tx), Some(txbuf)) =
        (Dma::new(RX_DESCS * 16), Dma::new(RX_DESCS * BUF), Dma::new(TX_DESCS * 16), Dma::new(TX_DESCS * BUF))
    else {
        crate::println!("{name}: out of memory");
        return;
    };
    for i in 0..RX_DESCS {
        unsafe {
            write_volatile(rx.ptr(i * 16) as *mut u64, rxbuf.addr(i * BUF));
            write_volatile(rx.ptr(i * 16 + 8) as *mut u64, 0);
        }
    }
    for i in 0..TX_DESCS {
        unsafe {
            write_volatile(tx.ptr(i * 16) as *mut u64, txbuf.addr(i * BUF));
            write_volatile(tx.ptr(i * 16 + 8) as *mut u64, 0);
            // Free (done) until used.
            write_volatile(tx.ptr(i * 16 + 12), DD);
        }
    }
    fence(Ordering::SeqCst);
    regs.w(RDBAL, rx.base as u32);
    regs.w(RDBAH, (rx.base >> 32) as u32);
    regs.w(RDLEN, (RX_DESCS * 16) as u32);
    regs.w(RDH, 0);
    regs.w(RDT, (RX_DESCS - 1) as u32);
    regs.w(RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);
    regs.w(TDBAL, tx.base as u32);
    regs.w(TDBAH, (tx.base >> 32) as u32);
    regs.w(TDLEN, (TX_DESCS * 16) as u32);
    regs.w(TDH, 0);
    regs.w(TDT, 0);
    regs.w(TCTL, TCTL_EN | TCTL_PSP | 0x10 << 4 | 0x40 << 12);
    regs.w(TIPG, 10 | 8 << 10 | 6 << 20);
    let irq = match crate::msi::setup(d, false, interrupt, regs.0 as usize) {
        Some((v, kind)) => {
            crate::println!("{name}: {kind} vector {v:#x}");
            let _ = regs.r(ICR);
            regs.w(IMS, INT_CAUSES);
            true
        }
        None => false,
    };
    let nic = E1000 { name, mac, regs, irq, rings: SpinLock::new(Rings { rx, rxbuf, rx_next: 0, tx, txbuf, tx_next: 0 }) };
    crate::net::register(Arc::new(nic));
}

impl Nic for E1000 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn link_up(&self) -> bool {
        self.regs.r(STATUS) & STATUS_LU != 0
    }

    fn send(&self, frame: &[u8]) -> bool {
        if frame.len() > BUF || frame.is_empty() {
            return false;
        }
        let mut r = self.rings.lock();
        let i = r.tx_next;
        let d = r.tx.ptr(i * 16);
        if unsafe { read_volatile(d.add(12)) } & DD == 0 {
            return false;
        }
        r.txbuf.write(i * BUF, frame);
        unsafe {
            write_volatile(d.add(8) as *mut u16, frame.len() as u16);
            write_volatile(d.add(10), 0);
            write_volatile(d.add(12), 0);
            write_volatile(d.add(11), TX_EOP | TX_IFCS | TX_RS);
        }
        r.tx_next = (i + 1) % TX_DESCS;
        fence(Ordering::SeqCst);
        self.regs.w(TDT, r.tx_next as u32);
        true
    }

    fn receive(&self, f: &mut dyn FnMut(&[u8])) -> usize {
        // Copy out under the lock, deliver after it.
        let mut frames: alloc::vec::Vec<alloc::vec::Vec<u8>> = alloc::vec::Vec::new();
        {
            let mut r = self.rings.lock();
            while frames.len() < RX_DESCS / 2 {
                let i = r.rx_next;
                let status = r.rx_status(i);
                if status & DD == 0 {
                    break;
                }
                fence(Ordering::SeqCst);
                let d = r.rx.ptr(i * 16);
                let len = unsafe { read_volatile(d.add(8) as *const u16) } as usize;
                let errors = unsafe { read_volatile(d.add(13)) };
                // Frames spanning buffers do not occur with 2 KB buffers
                // and no jumbo frames; drop them if they do.
                // Checksum errors (IPE/TCPE) are the stack's business.
                if status & RX_EOP != 0 && errors & RX_FRAME_ERRORS == 0 && len <= BUF {
                    frames.push(r.rxbuf.bytes(i * BUF, len).to_vec());
                }
                unsafe {
                    write_volatile(d.add(12), 0);
                    write_volatile(d.add(13), 0);
                }
                fence(Ordering::SeqCst);
                self.regs.w(RDT, i as u32);
                r.rx_next = (i + 1) % RX_DESCS;
            }
        }
        for fr in &frames {
            f(fr);
        }
        frames.len()
    }

    fn rx_pending(&self) -> bool {
        self.rings.try_lock().is_some_and(|r| r.rx_status(r.rx_next) & DD != 0)
    }

    fn interrupt_driven(&self) -> bool {
        self.irq
    }
}
