//! AMD PCnet-PCI II / PCnet-FAST (Am79C970A, Am79C971/972/973/975; PCI
//! 1022:2000): VMware's "vlance" (the "Flexible" adapter, the default
//! for older guest types such as OS/2) and QEMU's `pcnet`.
//!
//! The chip is programmed through I/O ports: the address PROM (the MAC)
//! at the start of the window, and the control/status registers (CSRs)
//! and bus configuration registers (BCRs), each selected by writing its
//! number to RAP and then accessed through RDP or BDP. After a reset the
//! window is in 16-bit mode (RDP 0x10, RAP 0x12, RESET 0x14, BDP 0x16),
//! which is what IBM's PCNTND.OS2 uses; a chip left in 32-bit mode (a
//! firmware driver writing RDP with 32 bits) is driven in that mode
//! (RDP 0x10, RAP 0x14, RESET 0x18, BDP 0x1c), since on the chip only a
//! hardware reset leaves it (QEMU's software reset leaves it too).
//!
//! Software style 2 (BCR20) selects 32-bit descriptors and the 32-bit
//! initialisation block, which gives the MAC address, an empty multicast
//! filter and the addresses and sizes of a receive ring of [`RX_DESCS`]
//! and a transmit ring of [`TX_DESCS`] descriptors, each with a fixed 2 KB
//! buffer. A descriptor belongs to the chip while its OWN bit is set; the
//! chip strips nothing on receive (the length includes the FCS) and pads
//! short frames on transmit (CSR4 APAD_XMT).
//!
//! The chip has no MSI, and rs2 cannot route PCI INTx lines (no ACPI
//! _PRT), so its interrupt stays disabled (CSR0 IENA clear): the timer
//! tick looks at the next receive descriptor's OWN bit and wakes the
//! network thread, which also looks every 20 ms. Link state comes from
//! the link LED (BCR4 LEDOUT) once that LED has been seen lit; until then
//! the link counts as up, since emulations without a link indicator
//! (VMware's vlance reads BCR4 as 0xc0 throughout) would otherwise never
//! transmit.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, Ordering};

use super::pci;
use super::virtio::queue::Dma;
use crate::arch::{inb, inl, inw, outl, outw};
use crate::net::Nic;
use crate::sync::SpinLock;

// CSR0 bits.
const INIT: u32 = 1 << 0;
const STRT: u32 = 1 << 1;
const STOP: u32 = 1 << 2;
const TDMD: u32 = 1 << 3;
const IDON: u32 = 1 << 8;
/// Status bits cleared by writing ones: BABL CERR MISS MERR RINT TINT IDON.
const CSR0_W1C: u32 = 0x7f00;

// Descriptor status (upper half of the second dword).
const OWN: u32 = 1 << 31;
const ERR: u32 = 1 << 30;
const STP: u32 = 1 << 25;
const ENP: u32 = 1 << 24;
/// The upper 4 bits of a buffer byte count are ones.
const BCNT_ONES: u32 = 0xf000;

/// CSR4: pad short transmit frames to 64 bytes.
const CSR4_APAD_XMT: u32 = 1 << 11;
/// BCR4: LED0 programmed for link status, and its output.
const LED_LNKSTE: u32 = 1 << 6;
const LED_OUT: u32 = 1 << 15;
/// BCR20 software style 2: 32-bit descriptors and initialisation block.
const SWSTYLE_PCNET_PCI: u32 = 2;

/// Ring sizes (powers of two, up to 512).
const RX_LOG2: u32 = 7;
const TX_LOG2: u32 = 6;
const RX_DESCS: usize = 1 << RX_LOG2;
const TX_DESCS: usize = 1 << TX_LOG2;
const BUF: usize = 2048;
/// Largest frame handed to the chip (without FCS).
const MAX_FRAME: usize = 1518;

pub fn is_pcnet(d: &pci::Device) -> bool {
    (d.vendor, d.device) == (0x1022, 0x2000)
}

/// The register window, 16- or 32-bit.
struct Io {
    base: u16,
    dwio: bool,
}

impl Io {
    fn rap(&self, r: u32) {
        unsafe {
            if self.dwio {
                outl(self.base + 0x14, r);
            } else {
                outw(self.base + 0x12, r as u16);
            }
        }
    }

    fn csr(&self, r: u32) -> u32 {
        self.rap(r);
        unsafe {
            if self.dwio {
                inl(self.base + 0x10) & 0xffff
            } else {
                inw(self.base + 0x10) as u32
            }
        }
    }

    fn set_csr(&self, r: u32, v: u32) {
        self.rap(r);
        unsafe {
            if self.dwio {
                outl(self.base + 0x10, v);
            } else {
                outw(self.base + 0x10, v as u16);
            }
        }
    }

    fn bcr(&self, r: u32) -> u32 {
        self.rap(r);
        unsafe {
            if self.dwio {
                inl(self.base + 0x1c) & 0xffff
            } else {
                inw(self.base + 0x16) as u32
            }
        }
    }

    fn set_bcr(&self, r: u32, v: u32) {
        self.rap(r);
        unsafe {
            if self.dwio {
                outl(self.base + 0x1c, v);
            } else {
                outw(self.base + 0x16, v as u16);
            }
        }
    }

    /// Software reset: the chip stops (CSR0 = STOP).
    fn reset(&self) {
        unsafe {
            if self.dwio {
                let _ = inl(self.base + 0x18);
            } else {
                let _ = inw(self.base + 0x14);
            }
        }
    }

    /// The chip answers in this mode: reset, CSR0 reads STOP, and RAP
    /// keeps what is written to it.
    fn check(&self) -> bool {
        self.reset();
        delay_ms(1);
        if self.csr(0) != STOP {
            return false;
        }
        self.rap(88);
        let ok = unsafe {
            if self.dwio {
                inl(self.base + 0x14) & 0xffff == 88
            } else {
                inw(self.base + 0x12) == 88
            }
        };
        ok
    }
}

fn delay_ms(ms: u64) {
    super::virtio::wait(ms, || {}, || false);
}

fn get32(m: &Dma, off: usize) -> u32 {
    unsafe { read_volatile(m.ptr(off) as *const u32) }
}

fn put32(m: &Dma, off: usize, v: u32) {
    unsafe { write_volatile(m.ptr(off) as *mut u32, v) }
}

/// Byte count field: the two's complement of the length, upper bits set.
fn bcnt(len: usize) -> u32 {
    (0u32.wrapping_sub(len as u32) & 0x0fff) | BCNT_ONES
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
    fn rx_ready(&self) -> bool {
        get32(&self.rx, self.rx_next * 16 + 4) & OWN == 0
    }

    fn give_rx(&self, i: usize) {
        put32(&self.rx, i * 16 + 8, 0);
        fence(Ordering::SeqCst);
        put32(&self.rx, i * 16 + 4, OWN | bcnt(BUF));
    }
}

pub struct Pcnet {
    name: String,
    mac: [u8; 6],
    io: SpinLock<Io>,
    /// LED0 shows link status (BCR4 LNKSTE).
    link_led: bool,
    /// LED0 has been seen lit: from then on it tells the link state.
    led_seen: AtomicBool,
    rings: SpinLock<Rings>,
    /// Initialisation block (kept for the chip's lifetime).
    _init: Dma,
}

/// Leave the chip quiet: stopped, no DMA, no interrupts.
fn shut_down(d: &pci::Device, io: &Io) {
    io.set_csr(0, STOP);
    d.write(0x04, (d.read(0x04) & 0xffff & !(1 << 2)) | 1 << 10);
}

fn chip_name(id: u32) -> &'static str {
    match id >> 12 & 0xffff {
        0x2420 => "Am79C970",
        0x2621 => "Am79C970A",
        0x2623 => "Am79C971",
        0x2624 => "Am79C972",
        0x2625 => "Am79C973",
        0x2626 => "Am79C978",
        0x2627 => "Am79C975",
        0x2628 => "Am79C976",
        _ => "PCnet",
    }
}

/// Set up an adapter and register it with the network layer.
pub fn probe(d: &pci::Device) {
    let b0 = d.read(0x10);
    if b0 & 1 == 0 {
        crate::println!("pcnet {}: BAR 0 is not I/O", d.bdf());
        return;
    }
    d.enable();
    d.write(0x04, (d.read(0x04) & 0xffff) | 1 << 10);
    let base = (b0 & 0xfffc) as u16;
    // 16-bit, else 32-bit, else 16-bit again: a 32-bit reset leaves
    // 32-bit mode on some implementations (QEMU's), not on the chip.
    let mut io = Io { base, dwio: false };
    if ![false, true, false].into_iter().any(|dwio| {
        io.dwio = dwio;
        io.check()
    }) {
        crate::println!("pcnet {}: no answer at I/O {base:#x}", d.bdf());
        shut_down(d, &io);
        return;
    }
    let id = io.csr(88) | io.csr(89) << 16;
    let chip = chip_name(id);
    let name = alloc::format!("pcnet {chip} {}", d.bdf());
    // MAC address: the address PROM, else what the chip loaded (CSR12-14).
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = unsafe { inb(base + i as u16) };
    }
    if mac == [0; 6] || mac == [0xff; 6] || mac[0] & 1 != 0 {
        for i in 0..3 {
            let v = io.csr(12 + i as u32);
            mac[i * 2] = v as u8;
            mac[i * 2 + 1] = (v >> 8) as u8;
        }
    }
    if mac == [0; 6] || mac[0] & 1 != 0 {
        crate::println!("{name}: no MAC address");
        shut_down(d, &io);
        return;
    }
    let (Some(init), Some(rx), Some(rxbuf), Some(tx), Some(txbuf)) =
        (Dma::new(64), Dma::new(RX_DESCS * 16), Dma::new(RX_DESCS * BUF), Dma::new(TX_DESCS * 16), Dma::new(TX_DESCS * BUF))
    else {
        crate::println!("{name}: out of memory");
        shut_down(d, &io);
        return;
    };
    io.set_bcr(20, SWSTYLE_PCNET_PCI);
    io.set_csr(4, io.csr(4) | CSR4_APAD_XMT);
    let rings = Rings { rx, rxbuf, rx_next: 0, tx, txbuf, tx_next: 0 };
    for i in 0..RX_DESCS {
        put32(&rings.rx, i * 16, rings.rxbuf.addr(i * BUF) as u32);
        put32(&rings.rx, i * 16 + 12, 0);
        rings.give_rx(i);
    }
    for i in 0..TX_DESCS {
        put32(&rings.tx, i * 16, rings.txbuf.addr(i * BUF) as u32);
        put32(&rings.tx, i * 16 + 4, 0);
        put32(&rings.tx, i * 16 + 8, 0);
        put32(&rings.tx, i * 16 + 12, 0);
    }
    // Initialisation block, software style 2: mode, ring lengths, MAC,
    // logical address filter (no multicast), ring addresses.
    put32(&init, 0, (RX_LOG2 << 20) | (TX_LOG2 << 28));
    put32(&init, 4, u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
    put32(&init, 8, u16::from_le_bytes([mac[4], mac[5]]) as u32);
    put32(&init, 12, 0);
    put32(&init, 16, 0);
    put32(&init, 20, rings.rx.base as u32);
    put32(&init, 24, rings.tx.base as u32);
    fence(Ordering::SeqCst);
    io.set_csr(1, init.base as u32 & 0xffff);
    io.set_csr(2, (init.base >> 16) as u32 & 0xffff);
    io.set_csr(0, INIT);
    let mut done = false;
    for _ in 0..100 {
        if io.csr(0) & IDON != 0 {
            done = true;
            break;
        }
        delay_ms(1);
    }
    if !done {
        crate::println!("{name}: initialisation did not complete (CSR0 {:#x})", io.csr(0));
        shut_down(d, &io);
        return;
    }
    // Start, interrupts left disabled (no IENA): polled.
    io.set_csr(0, IDON | STRT);
    let bcr4 = io.bcr(4);
    let link_led = bcr4 & LED_LNKSTE != 0;
    crate::println!("{name}: {} I/O {base:#x}, chip id {id:#010x}, CSR0 {:#06x}, BCR4 {bcr4:#06x}", if io.dwio { "32-bit" } else { "16-bit" }, io.csr(0));
    let nic = Pcnet {
        name,
        mac,
        io: SpinLock::new(io),
        link_led,
        led_seen: AtomicBool::new(link_led && bcr4 & LED_OUT != 0),
        rings: SpinLock::new(rings),
        _init: init,
    };
    crate::net::register(Arc::new(nic));
}

impl Nic for Pcnet {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn link_up(&self) -> bool {
        if !self.link_led {
            return true;
        }
        let lit = self.io.lock().bcr(4) & LED_OUT != 0;
        if lit {
            self.led_seen.store(true, Ordering::Relaxed);
        }
        lit || !self.led_seen.load(Ordering::Relaxed)
    }

    fn send(&self, frame: &[u8]) -> bool {
        if frame.is_empty() || frame.len() > MAX_FRAME {
            return false;
        }
        let mut r = self.rings.lock();
        let i = r.tx_next;
        if get32(&r.tx, i * 16 + 4) & OWN != 0 {
            return false;
        }
        r.txbuf.write(i * BUF, frame);
        put32(&r.tx, i * 16 + 8, 0);
        fence(Ordering::SeqCst);
        put32(&r.tx, i * 16 + 4, OWN | STP | ENP | bcnt(frame.len()));
        r.tx_next = (i + 1) % TX_DESCS;
        fence(Ordering::SeqCst);
        // Transmit now rather than at the next ring poll.
        self.io.lock().set_csr(0, TDMD);
        true
    }

    fn receive(&self, f: &mut dyn FnMut(&[u8])) -> usize {
        let mut frames: Vec<Vec<u8>> = Vec::new();
        {
            let mut r = self.rings.lock();
            while frames.len() < RX_DESCS / 2 && r.rx_ready() {
                fence(Ordering::SeqCst);
                let i = r.rx_next;
                let status = get32(&r.rx, i * 16 + 4);
                // Message length including the FCS.
                let len = (get32(&r.rx, i * 16 + 8) & 0xfff) as usize;
                if status & (STP | ENP) == STP | ENP && status & ERR == 0 && len > 4 && len <= BUF {
                    frames.push(r.rxbuf.bytes(i * BUF, len - 4).to_vec());
                }
                r.give_rx(i);
                r.rx_next = (i + 1) % RX_DESCS;
            }
        }
        if !frames.is_empty() {
            // Clear the status bits (RINT, MISS...) the chip collected.
            let io = self.io.lock();
            let csr0 = io.csr(0);
            if csr0 & CSR0_W1C != 0 {
                io.set_csr(0, csr0 & CSR0_W1C);
            }
        }
        for fr in &frames {
            f(fr);
        }
        frames.len()
    }

    fn rx_pending(&self) -> bool {
        self.rings.try_lock().is_some_and(|r| r.rx_ready())
    }
}
