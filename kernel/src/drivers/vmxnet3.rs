//! VMware vmxnet3: the paravirtual network adapter of VMware Workstation,
//! Fusion and ESXi (PCI 15ad:07b0), also emulated by QEMU (`-device
//! vmxnet3`).
//!
//! The device is driven through two register windows: BAR 1 holds the
//! version selection, the command register (commands are written to it,
//! their results read back from it), the MAC address and the address of
//! the "driver shared" area; BAR 0 holds the interrupt mask and the
//! queues' producer indexes. Everything else lives in guest memory the
//! driver describes in the shared area before the ACTIVATE_DEV command:
//! the configuration (MTU, receive filter, interrupt setup) and a queue
//! descriptor table with one transmit and one receive queue.
//!
//! Rings carry generation bits instead of head/tail registers: a
//! descriptor belongs to whoever's current generation it carries, and
//! each side flips its generation when it wraps. Transmit uses a ring of
//! [`TX_DESCS`] descriptors, each with its own 2 KB buffer the frame is
//! copied into, and a completion ring the device reports sent frames on.
//! Receive uses the first of the queue's two buffer rings ([`RX_DESCS`]
//! 2 KB "head" buffers, enough for any frame without jumbo frames); the
//! second ring, for packet bodies, is left empty, and its descriptors are
//! never handed to the device. Received frames are reported on the
//! receive completion ring.
//!
//! One interrupt (MSI-X, else MSI) covers both queues and the device's
//! events (link changes, queue errors); without either the device's
//! interrupts stay disabled and the timer tick polls the receive
//! completion ring. Events are read from the shared area and acknowledged
//! through the ECR register on the network thread.
//!
//! The device ABI is the one VMware publishes with its open-source
//! drivers (`vmxnet3_defs.h`); revision 1 of the device and UPT version 1
//! are selected, which every implementation supports.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, Ordering};

use super::pci;
use super::virtio::queue::Dma;
use crate::mm::mmio;
use crate::net::Nic;
use crate::sync::SpinLock;

// BAR 0 registers (8 bytes apart per queue / vector).
const IMR: usize = 0x000;
const TXPROD: usize = 0x600;
const RXPROD: usize = 0x800;
const RXPROD2: usize = 0xa00;

// BAR 1 registers.
const VRRS: usize = 0x00;
const UVRS: usize = 0x08;
const DSAL: usize = 0x10;
const DSAH: usize = 0x18;
const CMD: usize = 0x20;
const MACL: usize = 0x28;
const MACH: usize = 0x30;
const ECR: usize = 0x40;

// Commands written to CMD.
const CMD_ACTIVATE_DEV: u32 = 0xcafe_0000;
const CMD_QUIESCE_DEV: u32 = 0xcafe_0001;
const CMD_RESET_DEV: u32 = 0xcafe_0002;
const CMD_UPDATE_RX_MODE: u32 = 0xcafe_0003;
const CMD_UPDATE_VLAN_FILTERS: u32 = 0xcafe_0005;
const CMD_GET_QUEUE_STATUS: u32 = 0xf00d_0000;
const CMD_GET_LINK: u32 = 0xf00d_0002;
const CMD_GET_PERM_MAC_LO: u32 = 0xf00d_0003;
const CMD_GET_PERM_MAC_HI: u32 = 0xf00d_0004;

const MAGIC: u32 = 0xbabe_fee1;

// Driver shared area (Vmxnet3_DriverShared), offsets.
const DS_MAGIC: usize = 0;
const DS_MISC: usize = 8;
const DS_INTR: usize = 80;
const DS_RXFILTER: usize = 120;
const DS_ECR: usize = 696;
const DS_SIZE: usize = 720;
// Vmxnet3_MiscConf, relative to DS_MISC.
const MISC_VERSION: usize = 0;
const MISC_GOS: usize = 4;
const MISC_REV_SPT: usize = 8;
const MISC_UPT_SPT: usize = 12;
const MISC_QDESC_PA: usize = 32;
const MISC_QDESC_LEN: usize = 44;
const MISC_MTU: usize = 48;
const MISC_MAX_RX_SG: usize = 52;
const MISC_NUM_TXQ: usize = 54;
const MISC_NUM_RXQ: usize = 55;
// Vmxnet3_IntrConf, relative to DS_INTR.
const INTR_AUTOMASK: usize = 0;
const INTR_NUM: usize = 1;
const INTR_EVENT_IDX: usize = 2;
const INTR_MOD_LEVELS: usize = 3;
const INTR_CTRL: usize = 28;
const IC_DISABLE_ALL: u32 = 1;
// Vmxnet3_RxFilterConf, relative to DS_RXFILTER.
const RXF_MODE: usize = 0;
const RXF_VFTABLE: usize = 16;
const RXM_UCAST: u32 = 1;
const RXM_BCAST: u32 = 4;

/// The queue descriptor table follows the shared area in the same page
/// (128-byte alignment): one transmit queue descriptor, then one receive
/// queue descriptor, 256 bytes each.
const QDESC: usize = 1024;
const QDESC_LEN: usize = 512;
// Vmxnet3_TxQueueDesc: ctrl (16), conf (64), status, stats.
const TXQ_NUM_DEFERRED: usize = 0;
const TXQ_RING: usize = 16;
const TXQ_DATA_RING: usize = 24;
const TXQ_COMP_RING: usize = 32;
const TXQ_RING_SIZE: usize = 56;
const TXQ_DATA_SIZE: usize = 60;
const TXQ_COMP_SIZE: usize = 64;
const TXQ_INTR_IDX: usize = 72;
const TXQ_STATUS: usize = 80;
// Vmxnet3_RxQueueDesc: ctrl (16), conf (64), status, stats.
const RXQ_RING: [usize; 2] = [16, 24];
const RXQ_COMP_RING: usize = 32;
const RXQ_RING_SIZE: [usize; 2] = [56, 60];
const RXQ_COMP_SIZE: usize = 64;
const RXQ_INTR_IDX: usize = 72;
const RXQ_STATUS: usize = 80;

// Event causes (shared area ecr, ECR register).
const ECR_RQERR: u32 = 1 << 0;
const ECR_TQERR: u32 = 1 << 1;
const ECR_LINK: u32 = 1 << 2;

// Descriptor bits (dword 2 and 3 of the 16-byte descriptors).
const TXD_GEN: u32 = 1 << 14;
const TXD_EOP: u32 = 1 << 12;
const TXD_CQ: u32 = 1 << 13;
const RXD_GEN: u32 = 1 << 31;
const COMP_GEN: u32 = 1 << 31;
const RCD_EOP: u32 = 1 << 14;
const RCD_SOP: u32 = 1 << 15;
const RCD_ERR: u32 = 1 << 14;

/// Ring sizes (multiples of 32).
const TX_DESCS: usize = 64;
const RX_DESCS: usize = 256;
const RX2_DESCS: usize = 32;
const RXC_DESCS: usize = RX_DESCS + RX2_DESCS;
/// Transmit data ring entries (header copies; unused, but provided).
const TX_DATA: usize = 128;
const BUF: usize = 2048;

pub fn is_vmxnet3(d: &pci::Device) -> bool {
    (d.vendor, d.device) == (0x15ad, 0x07b0)
}

#[derive(Clone, Copy)]
struct Win(*mut u8);

unsafe impl Send for Win {}
unsafe impl Sync for Win {}

impl Win {
    fn r(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.0.add(off) as *const u32) }
    }

    fn w(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.0.add(off) as *mut u32, v) }
    }
}

fn put32(m: &Dma, off: usize, v: u32) {
    unsafe { write_volatile(m.ptr(off) as *mut u32, v) }
}

fn put64(m: &Dma, off: usize, v: u64) {
    unsafe { write_volatile(m.ptr(off) as *mut u64, v) }
}

fn get32(m: &Dma, off: usize) -> u32 {
    unsafe { read_volatile(m.ptr(off) as *const u32) }
}

struct Rings {
    tx: Dma,
    txbuf: Dma,
    txcomp: Dma,
    /// Next transmit descriptor and the generation it gets.
    tx_next: usize,
    tx_gen: u32,
    /// Oldest descriptor not yet reported done, and how many are out.
    tx_oldest: usize,
    tx_busy: usize,
    txc_next: usize,
    txc_gen: u32,
    rx: Dma,
    rxbuf: Dma,
    rxcomp: Dma,
    /// Next ring-1 descriptor to hand back, and its generation.
    rx_fill: usize,
    rx_gen: u32,
    rxc_next: usize,
    rxc_gen: u32,
    /// Inside a frame spread over several buffers (dropped).
    rx_skip: bool,
}

impl Rings {
    fn rx_ready(&self) -> bool {
        get32(&self.rxcomp, self.rxc_next * 16 + 12) & COMP_GEN == self.rxc_gen
    }

    /// Take back the transmit descriptors the device reports done.
    fn reclaim_tx(&mut self) {
        loop {
            let off = self.txc_next * 16;
            if get32(&self.txcomp, off + 12) & COMP_GEN != self.txc_gen {
                break;
            }
            fence(Ordering::SeqCst);
            let idx = get32(&self.txcomp, off) as usize & 0xfff;
            // Everything up to and including idx is done.
            let n = (idx + TX_DESCS - self.tx_oldest) % TX_DESCS + 1;
            if n <= self.tx_busy {
                self.tx_busy -= n;
                self.tx_oldest = (idx + 1) % TX_DESCS;
            }
            self.txc_next += 1;
            if self.txc_next == TX_DESCS {
                self.txc_next = 0;
                self.txc_gen ^= COMP_GEN;
            }
        }
    }

    /// Hand the ring-1 buffer at `addr` back to the device.
    fn post_rx(&mut self, addr: u64) {
        let off = self.rx_fill * 16;
        put64(&self.rx, off, addr);
        put32(&self.rx, off + 12, 0);
        fence(Ordering::SeqCst);
        // Length, head buffer, generation last.
        put32(&self.rx, off + 8, BUF as u32 | self.rx_gen);
        self.rx_fill += 1;
        if self.rx_fill == RX_DESCS {
            self.rx_fill = 0;
            self.rx_gen ^= RXD_GEN;
        }
    }
}

pub struct Vmxnet3 {
    name: String,
    mac: [u8; 6],
    bar0: Win,
    bar1: Win,
    /// The shared area and queue descriptor table.
    shared: Dma,
    /// Command register access (write, then read the result).
    cmd: SpinLock<()>,
    link: AtomicBool,
    rings: SpinLock<Rings>,
    irq: bool,
}

/// Interrupt handler (MSI-X/MSI, not shared): wake the network thread,
/// which looks at both queues and the events.
fn interrupt(_ctx: usize) {
    crate::net::kick();
}

/// A command whose result is read back (ACTIVATE_DEV and the GET_s).
fn command(bar1: Win, c: u32) -> u32 {
    bar1.w(CMD, c);
    bar1.r(CMD)
}

/// A command without a result.
fn issue(bar1: Win, c: u32) {
    bar1.w(CMD, c);
}

/// Leave the device quiet: no DMA, no interrupts.
fn shut_down(d: &pci::Device, bar1: Win) {
    issue(bar1, CMD_QUIESCE_DEV);
    issue(bar1, CMD_RESET_DEV);
    d.write(0x04, (d.read(0x04) & 0xffff & !(1 << 2)) | 1 << 10);
}

fn read_mac(bar1: Win) -> [u8; 6] {
    let (lo, hi) = (bar1.r(MACL), bar1.r(MACH));
    let m = [lo as u8, (lo >> 8) as u8, (lo >> 16) as u8, (lo >> 24) as u8, hi as u8, (hi >> 8) as u8];
    if m != [0; 6] && m[0] & 1 == 0 {
        return m;
    }
    let (lo, hi) = (command(bar1, CMD_GET_PERM_MAC_LO), command(bar1, CMD_GET_PERM_MAC_HI));
    [lo as u8, (lo >> 8) as u8, (lo >> 16) as u8, (lo >> 24) as u8, hi as u8, (hi >> 8) as u8]
}

/// Set up an adapter and register it with the network layer.
pub fn probe(d: &pci::Device) {
    let name = alloc::format!("vmxnet3 {}", d.bdf());
    // BAR 0 and BAR 1 are memory windows (BAR 1 follows a 64-bit BAR 0
    // in the next slot but one).
    let b0 = d.read(0x10);
    let idx1 = if b0 & 1 == 0 && (b0 >> 1) & 3 == 2 { 2 } else { 1 };
    if b0 & 1 != 0 || d.read(0x10 + idx1 * 4) & 1 != 0 {
        crate::println!("{name}: BARs are not memory");
        return;
    }
    d.enable();
    d.write(0x04, (d.read(0x04) & 0xffff) | 1 << 10);
    let (p0, p1) = (d.bar(0), d.bar(idx1));
    if p0 == 0 || p1 == 0 {
        return;
    }
    let bar0 = Win(mmio::map(p0, 0x1000));
    let bar1 = Win(mmio::map(p1, 0x1000));
    // Revision 1 of the device and of UPT.
    if bar1.r(VRRS) & 1 == 0 || bar1.r(UVRS) & 1 == 0 {
        crate::println!("{name}: device revision 1 not supported ({:#x}, {:#x})", bar1.r(VRRS), bar1.r(UVRS));
        shut_down(d, bar1);
        return;
    }
    bar1.w(VRRS, 1);
    bar1.w(UVRS, 1);
    issue(bar1, CMD_RESET_DEV);
    let mac = read_mac(bar1);
    if mac == [0; 6] || mac[0] & 1 != 0 {
        crate::println!("{name}: no MAC address");
        shut_down(d, bar1);
        return;
    }
    let mems = (
        Dma::new(4096),
        Dma::new(TX_DESCS * 16),
        Dma::new(TX_DESCS * BUF),
        Dma::new(TX_DESCS * 16),
        Dma::new(TX_DESCS * TX_DATA),
        Dma::new(RX_DESCS * 16),
        Dma::new(RX2_DESCS * 16),
        Dma::new(RX_DESCS * BUF),
        Dma::new(RXC_DESCS * 16),
    );
    let (Some(shared), Some(tx), Some(txbuf), Some(txcomp), Some(txdata), Some(rx), Some(rx2), Some(rxbuf), Some(rxcomp)) = mems else {
        crate::println!("{name}: out of memory");
        shut_down(d, bar1);
        return;
    };
    // Descriptors start out owned by no one (generation 0).
    for (m, len) in [(&shared, 4096), (&tx, TX_DESCS * 16), (&txcomp, TX_DESCS * 16), (&rx, RX_DESCS * 16), (&rx2, RX2_DESCS * 16), (&rxcomp, RXC_DESCS * 16)] {
        unsafe { core::ptr::write_bytes(m.ptr(0), 0, len) };
    }
    // One vector for everything; interrupts stay disabled without one.
    let irq = crate::msi::setup(d, true, interrupt, 0);

    let s = &shared;
    put32(s, DS_MAGIC, MAGIC);
    put32(s, DS_MISC + MISC_VERSION, 1);
    // 64-bit guest of the Linux type: there is no OS/2 type, and that
    // one gets the device's standard behaviour.
    put32(s, DS_MISC + MISC_GOS, 2 | 1 << 2);
    put32(s, DS_MISC + MISC_REV_SPT, 1);
    put32(s, DS_MISC + MISC_UPT_SPT, 1);
    put64(s, DS_MISC + MISC_QDESC_PA, s.addr(QDESC));
    put32(s, DS_MISC + MISC_QDESC_LEN, QDESC_LEN as u32);
    put32(s, DS_MISC + MISC_MTU, 1500);
    unsafe {
        write_volatile(s.ptr(DS_MISC + MISC_MAX_RX_SG) as *mut u16, 1);
        write_volatile(s.ptr(DS_MISC + MISC_NUM_TXQ), 1);
        write_volatile(s.ptr(DS_MISC + MISC_NUM_RXQ), 1);
        write_volatile(s.ptr(DS_INTR + INTR_AUTOMASK), 0);
        write_volatile(s.ptr(DS_INTR + INTR_NUM), 1);
        write_volatile(s.ptr(DS_INTR + INTR_EVENT_IDX), 0);
        // No moderation (level 0).
        write_volatile(s.ptr(DS_INTR + INTR_MOD_LEVELS), 0);
    }
    put32(s, DS_INTR + INTR_CTRL, if irq.is_some() { 0 } else { IC_DISABLE_ALL });
    put32(s, DS_RXFILTER + RXF_MODE, RXM_UCAST | RXM_BCAST);
    // VLAN 0: untagged frames.
    put32(s, DS_RXFILTER + RXF_VFTABLE, 1);

    let t = QDESC;
    put64(s, t + TXQ_RING, tx.base);
    put64(s, t + TXQ_DATA_RING, txdata.base);
    put64(s, t + TXQ_COMP_RING, txcomp.base);
    put32(s, t + TXQ_RING_SIZE, TX_DESCS as u32);
    put32(s, t + TXQ_DATA_SIZE, TX_DESCS as u32);
    put32(s, t + TXQ_COMP_SIZE, TX_DESCS as u32);
    unsafe { write_volatile(s.ptr(t + TXQ_INTR_IDX), 0) };
    let r = QDESC + 256;
    put64(s, r + RXQ_RING[0], rx.base);
    put64(s, r + RXQ_RING[1], rx2.base);
    put64(s, r + RXQ_COMP_RING, rxcomp.base);
    put32(s, r + RXQ_RING_SIZE[0], RX_DESCS as u32);
    put32(s, r + RXQ_RING_SIZE[1], RX2_DESCS as u32);
    put32(s, r + RXQ_COMP_SIZE, RXC_DESCS as u32);
    unsafe { write_volatile(s.ptr(r + RXQ_INTR_IDX), 0) };

    let mut rings = Rings {
        tx,
        txbuf,
        txcomp,
        tx_next: 0,
        tx_gen: TXD_GEN,
        tx_oldest: 0,
        tx_busy: 0,
        txc_next: 0,
        txc_gen: COMP_GEN,
        rx,
        rxbuf,
        rxcomp,
        rx_fill: 0,
        rx_gen: RXD_GEN,
        rxc_next: 0,
        rxc_gen: COMP_GEN,
        rx_skip: false,
    };
    // All but one ring-1 buffer (a full ring would look empty to a
    // device going by the producer index).
    for i in 0..RX_DESCS - 1 {
        let a = rings.rxbuf.addr(i * BUF);
        rings.post_rx(a);
    }
    fence(Ordering::SeqCst);
    bar1.w(DSAL, shared.base as u32);
    bar1.w(DSAH, (shared.base >> 32) as u32);
    let st = command(bar1, CMD_ACTIVATE_DEV);
    if st != 0 {
        crate::println!("{name}: activation failed ({st:#x})");
        shut_down(d, bar1);
        return;
    }
    bar0.w(RXPROD, rings.rx_fill as u32);
    bar0.w(RXPROD2, 0);
    issue(bar1, CMD_UPDATE_RX_MODE);
    issue(bar1, CMD_UPDATE_VLAN_FILTERS);
    match irq {
        Some((v, kind)) => {
            crate::println!("{name}: {kind} vector {v:#x}");
            bar0.w(IMR, 0);
        }
        None => bar0.w(IMR, 1),
    }
    let link = command(bar1, CMD_GET_LINK);
    let nic = Vmxnet3 {
        name,
        mac,
        bar0,
        bar1,
        shared,
        cmd: SpinLock::new(()),
        link: AtomicBool::new(link & 1 != 0),
        rings: SpinLock::new(rings),
        irq: irq.is_some(),
    };
    // The transmit data ring and the empty body ring stay allocated for
    // the device's lifetime.
    core::mem::forget(txdata);
    core::mem::forget(rx2);
    crate::net::register(Arc::new(nic));
}

impl Vmxnet3 {
    fn command(&self, c: u32) -> u32 {
        let _g = self.cmd.lock();
        command(self.bar1, c)
    }

    /// Handle the events the device posted: link changes, queue errors.
    fn events(&self) {
        let ecr = get32(&self.shared, DS_ECR);
        if ecr == 0 {
            return;
        }
        self.bar1.w(ECR, ecr);
        if ecr & ECR_LINK != 0 {
            let l = self.command(CMD_GET_LINK);
            self.link.store(l & 1 != 0, Ordering::Relaxed);
            crate::println!("{}: link {} ({} Mb/s)", self.name, if l & 1 != 0 { "up" } else { "down" }, l >> 16);
        }
        if ecr & (ECR_TQERR | ECR_RQERR) != 0 {
            self.command(CMD_GET_QUEUE_STATUS);
            let te = get32(&self.shared, QDESC + TXQ_STATUS + 4);
            let re = get32(&self.shared, QDESC + 256 + RXQ_STATUS + 4);
            crate::println!("{}: queue error (tx {te:#x}, rx {re:#x})", self.name);
        }
    }
}

impl Nic for Vmxnet3 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn link_up(&self) -> bool {
        self.link.load(Ordering::Relaxed)
    }

    fn send(&self, frame: &[u8]) -> bool {
        if frame.is_empty() || frame.len() > BUF {
            return false;
        }
        let mut r = self.rings.lock();
        r.reclaim_tx();
        if r.tx_busy >= TX_DESCS - 1 {
            return false;
        }
        let i = r.tx_next;
        r.txbuf.write(i * BUF, frame);
        let off = i * 16;
        put64(&r.tx, off, r.txbuf.addr(i * BUF));
        put32(&r.tx, off + 12, TXD_EOP | TXD_CQ);
        fence(Ordering::SeqCst);
        // Length (no offloads), generation last.
        put32(&r.tx, off + 8, frame.len() as u32 | r.tx_gen);
        r.tx_busy += 1;
        r.tx_next += 1;
        if r.tx_next == TX_DESCS {
            r.tx_next = 0;
            r.tx_gen ^= TXD_GEN;
        }
        fence(Ordering::SeqCst);
        put32(&self.shared, QDESC + TXQ_NUM_DEFERRED, 0);
        self.bar0.w(TXPROD, r.tx_next as u32);
        true
    }

    fn receive(&self, f: &mut dyn FnMut(&[u8])) -> usize {
        self.events();
        let mut frames: Vec<Vec<u8>> = Vec::new();
        {
            let mut r = self.rings.lock();
            r.reclaim_tx();
            let mut posted = false;
            while frames.len() < RX_DESCS / 2 && r.rx_ready() {
                fence(Ordering::SeqCst);
                let off = r.rxc_next * 16;
                let d0 = get32(&r.rxcomp, off);
                let d2 = get32(&r.rxcomp, off + 8);
                r.rxc_next += 1;
                if r.rxc_next == RXC_DESCS {
                    r.rxc_next = 0;
                    r.rxc_gen ^= COMP_GEN;
                }
                let idx = d0 as usize & 0xfff;
                let ring2 = (d0 >> 16) & 0x3ff != 0;
                if ring2 || idx >= RX_DESCS {
                    // Body buffers are never posted.
                    continue;
                }
                let addr = unsafe { read_volatile(r.rx.ptr(idx * 16) as *const u64) };
                let len = (d2 & 0x3fff) as usize;
                let (sop, eop) = (d0 & RCD_SOP != 0, d0 & RCD_EOP != 0);
                if sop && eop && d2 & RCD_ERR == 0 && len <= BUF && len > 0 && !r.rx_skip {
                    let boff = (addr - r.rxbuf.base) as usize;
                    frames.push(r.rxbuf.bytes(boff, len).to_vec());
                }
                // A frame over several buffers: skip to its end.
                if sop && !eop {
                    r.rx_skip = true;
                } else if eop {
                    r.rx_skip = false;
                }
                r.post_rx(addr);
                posted = true;
            }
            if posted {
                fence(Ordering::SeqCst);
                self.bar0.w(RXPROD, r.rx_fill as u32);
            }
        }
        for fr in &frames {
            f(fr);
        }
        frames.len()
    }

    fn rx_pending(&self) -> bool {
        get32(&self.shared, DS_ECR) != 0 || self.rings.try_lock().is_some_and(|r| r.rx_ready())
    }

    fn interrupt_driven(&self) -> bool {
        self.irq
    }
}
