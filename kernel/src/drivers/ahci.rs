//! AHCI (SATA) host controllers: ATA disks and ATAPI optical drives.
//!
//! One command at a time per port through slot 0; DMA goes through a
//! physically contiguous bounce buffer. With MSI the controller interrupts
//! on command completion and errors: the handler clears and records each
//! port's interrupt status and wakes the thread waiting on that port.
//! Without MSI (or with `nomsi`, and during boot) completion is polled,
//! yielding the CPU.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use super::pci;
use crate::api::err;
use crate::fs::block::BlockDev;
use crate::mm::{mmio, phys};
use crate::msi::{self, Event};
use crate::sync::Mutex;

const BOUNCE: usize = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ata,
    Atapi,
}

struct PortState {
    regs: *mut u32,
    /// Command list (1 KB), received FIS (256 B), command table (256 B)
    /// and bounce buffer, all physically contiguous.
    clb: u64,
    fb: u64,
    ctba: u64,
    buf: u64,
    /// Interrupt status and wake-up, with MSI.
    irq: Option<&'static PortIrq>,
}

/// What the interrupt handler shares with a port.
struct PortIrq {
    regs: *mut u32,
    /// PxIS bits the handler cleared since the command was issued.
    status: AtomicU32,
    event: Event,
}

/// The controller as the interrupt handler sees it.
struct HbaIrq {
    hba: *mut u32,
    /// `&'static PortIrq` by port, 0 if none.
    ports: [AtomicUsize; 32],
}

unsafe impl Sync for PortIrq {}
unsafe impl Sync for HbaIrq {}

/// Port interrupts handled: D2H register FIS, PIO setup, DMA setup, set
/// device bits, descriptor processed, and the error causes (interface
/// fatal and non-fatal, host bus data and fatal, task file error).
const PXIE_MASK: u32 = 0xf | 1 << 5 | 1 << 26 | 0xf << 27;
const TFES: u32 = 1 << 30;
/// Longest wait for a command (spin-up, ATAPI media).
const CMD_TIMEOUT_MS: u64 = 30_000;

/// MSI handler: record and clear the interrupt status of every port that
/// has some, wake its waiter, then acknowledge the controller.
fn interrupt(ctx: usize) {
    let h = unsafe { &*(ctx as *const HbaIrq) };
    let is = rd(h.hba, 0x08 / 4);
    for port in 0..32 {
        if is & (1 << port) == 0 {
            continue;
        }
        let p = h.ports[port].load(Ordering::Acquire);
        if p == 0 {
            continue;
        }
        let p = unsafe { &*(p as *const PortIrq) };
        let s = rd(p.regs, PXIS);
        wr(p.regs, PXIS, s);
        p.status.fetch_or(s, Ordering::AcqRel);
        p.event.signal();
    }
    wr(h.hba, 0x08 / 4, is);
}

unsafe impl Send for PortState {}

pub struct Port {
    pub kind: Kind,
    pub model: String,
    pub sector_size: usize,
    pub sectors: u64,
    state: Mutex<PortState>,
}

unsafe impl Send for Port {}
unsafe impl Sync for Port {}

// Port register indices (in u32 units from the port base).
const PXCLB: usize = 0x00 / 4;
const PXCLBU: usize = 0x04 / 4;
const PXFB: usize = 0x08 / 4;
const PXFBU: usize = 0x0c / 4;
const PXIS: usize = 0x10 / 4;
const PXIE: usize = 0x14 / 4;
const PXCMD: usize = 0x18 / 4;
const PXTFD: usize = 0x20 / 4;
const PXSIG: usize = 0x24 / 4;
const PXSSTS: usize = 0x28 / 4;
const PXSERR: usize = 0x30 / 4;
const PXCI: usize = 0x38 / 4;

fn rd(p: *mut u32, i: usize) -> u32 {
    unsafe { read_volatile(p.add(i)) }
}

fn wr(p: *mut u32, i: usize, v: u32) {
    unsafe { write_volatile(p.add(i), v) }
}

fn spin_until(mut cond: impl FnMut() -> bool, max_iter: u32) -> bool {
    for i in 0..max_iter {
        if cond() {
            return true;
        }
        if i > 1000 {
            crate::task::sched::yield_now();
        }
        core::hint::spin_loop();
    }
    cond()
}

impl PortState {
    fn stop(&self) {
        let p = self.regs;
        wr(p, PXCMD, rd(p, PXCMD) & !1);
        spin_until(|| rd(p, PXCMD) & (1 << 15) == 0, 500_000);
        wr(p, PXCMD, rd(p, PXCMD) & !(1 << 4));
        spin_until(|| rd(p, PXCMD) & (1 << 14) == 0, 500_000);
    }

    fn start(&self) {
        let p = self.regs;
        spin_until(|| rd(p, PXTFD) & 0x88 == 0, 1_000_000);
        wr(p, PXCMD, rd(p, PXCMD) | (1 << 4));
        wr(p, PXCMD, rd(p, PXCMD) | 1);
    }

    /// Issue a command in slot 0. `fis` is the 20-byte H2D register FIS,
    /// `atapi` an optional 12-byte packet. Data moves through the bounce
    /// buffer (`len` bytes, written to the device if `write`).
    fn command(&self, fis: &[u8; 20], atapi: Option<&[u8; 12]>, len: usize, write: bool) -> Result<(), u32> {
        let p = self.regs;
        unsafe {
            let hdr = phys::phys_to_virt(self.clb) as *mut u32;
            let mut dw0 = 5u32; // CFL: 5 dwords
            if atapi.is_some() {
                dw0 |= 1 << 5;
            }
            if write {
                dw0 |= 1 << 6;
            }
            let prdtl = if len > 0 { 1 } else { 0 };
            dw0 |= prdtl << 16;
            write_volatile(hdr, dw0);
            write_volatile(hdr.add(1), 0);
            write_volatile(hdr.add(2), self.ctba as u32);
            write_volatile(hdr.add(3), (self.ctba >> 32) as u32);
            let tbl = phys::phys_to_virt(self.ctba);
            core::ptr::write_bytes(tbl, 0, 0x90);
            core::ptr::copy_nonoverlapping(fis.as_ptr(), tbl, 20);
            if let Some(pk) = atapi {
                core::ptr::copy_nonoverlapping(pk.as_ptr(), tbl.add(0x40), 12);
            }
            if len > 0 {
                let prd = tbl.add(0x80) as *mut u32;
                write_volatile(prd, self.buf as u32);
                write_volatile(prd.add(1), (self.buf >> 32) as u32);
                write_volatile(prd.add(2), 0);
                write_volatile(prd.add(3), (len as u32 - 1) & 0x3f_ffff);
            }
        }
        wr(p, PXIS, 0xffff_ffff);
        if let Some(i) = self.irq {
            i.status.store(0, Ordering::Release);
        }
        if !spin_until(|| rd(p, PXTFD) & 0x88 == 0, 1_000_000) {
            return Err(err::NOT_READY);
        }
        wr(p, PXCI, 1);
        let done = match self.irq {
            Some(i) => i.event.wait(CMD_TIMEOUT_MS, || {}, || rd(p, PXCI) & 1 == 0 || self.is() & TFES != 0),
            None => spin_until(|| rd(p, PXCI) & 1 == 0 || rd(p, PXIS) & TFES != 0, 50_000_000),
        };
        if !done || self.is() & TFES != 0 || rd(p, PXTFD) & 1 != 0 {
            // Error: restart the port to clear it.
            self.stop();
            wr(p, PXSERR, 0xffff_ffff);
            wr(p, PXIS, 0xffff_ffff);
            self.start();
            return Err(if done { err::NOT_READY } else { err::NOT_READY });
        }
        Ok(())
    }

    fn bounce(&self) -> *mut u8 {
        phys::phys_to_virt(self.buf)
    }

    /// Interrupt status: the register plus what the handler took.
    fn is(&self) -> u32 {
        rd(self.regs, PXIS) | self.irq.map_or(0, |i| i.status.load(Ordering::Acquire))
    }
}

fn h2d(cmd: u8, lba: u64, count: u16, features: u8) -> [u8; 20] {
    let mut f = [0u8; 20];
    f[0] = 0x27;
    f[1] = 0x80;
    f[2] = cmd;
    f[3] = features;
    f[4] = lba as u8;
    f[5] = (lba >> 8) as u8;
    f[6] = (lba >> 16) as u8;
    f[7] = 0x40; // LBA mode
    f[8] = (lba >> 24) as u8;
    f[9] = (lba >> 32) as u8;
    f[10] = (lba >> 40) as u8;
    f[12] = count as u8;
    f[13] = (count >> 8) as u8;
    f
}

fn ata_string(words: &[u16]) -> String {
    let mut s = String::new();
    for w in words {
        for b in [(w >> 8) as u8, *w as u8] {
            if b != 0 {
                s.push(b as char);
            }
        }
    }
    String::from(s.trim())
}

impl Port {
    fn identify(st: &PortState, kind: Kind) -> Option<(String, usize, u64)> {
        match kind {
            Kind::Ata => {
                st.command(&h2d(0xec, 0, 0, 0), None, 512, false).ok()?;
                let w: Vec<u16> = (0..256)
                    .map(|i| unsafe { (st.bounce().add(i * 2) as *const u16).read_unaligned() })
                    .collect();
                let model = ata_string(&w[27..47]);
                let lba48 = w[83] & (1 << 10) != 0;
                let sectors = if lba48 {
                    w[100] as u64 | (w[101] as u64) << 16 | (w[102] as u64) << 32 | (w[103] as u64) << 48
                } else {
                    w[60] as u64 | (w[61] as u64) << 16
                };
                Some((model, 512, sectors))
            }
            Kind::Atapi => {
                let model = if st.command(&h2d(0xa1, 0, 0, 0), None, 512, false).is_ok() {
                    let w: Vec<u16> = (0..256)
                        .map(|i| unsafe { (st.bounce().add(i * 2) as *const u16).read_unaligned() })
                        .collect();
                    ata_string(&w[27..47])
                } else {
                    String::from("ATAPI device")
                };
                // READ CAPACITY(10); no medium leaves the size at 0.
                let mut pk = [0u8; 12];
                pk[0] = 0x25;
                let mut fis = h2d(0xa0, 0, 0, 1);
                fis[5] = 8; // byte count limit
                let sectors = if st.command(&fis, Some(&pk), 8, false).is_ok() {
                    let b = unsafe { core::slice::from_raw_parts(st.bounce(), 8) };
                    u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64 + 1
                } else {
                    0
                };
                Some((model, 2048, sectors))
            }
        }
    }

    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let ss = self.sector_size;
        if buf.len() % ss != 0 {
            return Err(err::INVALID_PARAMETER);
        }
        let st = self.state.lock();
        let per = BOUNCE / ss;
        let mut done = 0usize;
        let total = buf.len() / ss;
        while done < total {
            let n = (total - done).min(per);
            let bytes = n * ss;
            let at = lba + done as u64;
            let chunk = &mut buf[done * ss..done * ss + bytes];
            if write {
                unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), st.bounce(), bytes) };
            }
            match self.kind {
                Kind::Ata => {
                    let cmd = if write { 0x35 } else { 0x25 }; // WRITE/READ DMA EXT
                    st.command(&h2d(cmd, at, n as u16, 0), None, bytes, write)?;
                }
                Kind::Atapi => {
                    if write {
                        return Err(err::WRITE_PROTECT);
                    }
                    let mut pk = [0u8; 12];
                    pk[0] = 0x28; // READ(10)
                    pk[2..6].copy_from_slice(&(at as u32).to_be_bytes());
                    pk[7..9].copy_from_slice(&(n as u16).to_be_bytes());
                    let mut fis = h2d(0xa0, 0, 0, 1);
                    fis[5] = (bytes & 0xff) as u8;
                    fis[6] = (bytes >> 8) as u8;
                    if let Err(e) = st.command(&fis, Some(&pk), bytes, false) {
                        // REQUEST SENSE clears a unit attention (medium
                        // changed), so that a retry can succeed.
                        let mut rs = [0u8; 12];
                        rs[0] = 0x03;
                        rs[4] = 18;
                        let mut f2 = h2d(0xa0, 0, 0, 1);
                        f2[5] = 18;
                        let _ = st.command(&f2, Some(&rs), 18, false);
                        return Err(e);
                    }
                }
            }
            if !write {
                unsafe { core::ptr::copy_nonoverlapping(st.bounce(), chunk.as_mut_ptr(), bytes) };
            }
            done += n;
        }
        Ok(())
    }
}

impl BlockDev for Port {
    fn sector_size(&self) -> usize {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.transfer(lba, buf, false)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut tmp = buf.to_vec();
        self.transfer(lba, &mut tmp, true)
    }
}

/// Bring up an AHCI controller and return its devices.
pub fn probe(dev: &pci::Device) -> Vec<Arc<Port>> {
    let mut out = Vec::new();
    dev.enable();
    let abar = dev.bar(5);
    if abar == 0 {
        return out;
    }
    let hba = mmio::map(abar, 0x1100) as *mut u32;
    // BIOS/OS handoff.
    let cap2 = rd(hba, 0x24 / 4);
    if cap2 & 1 != 0 {
        wr(hba, 0x28 / 4, rd(hba, 0x28 / 4) | 2);
        spin_until(|| rd(hba, 0x28 / 4) & 1 == 0, 1_000_000);
    }
    wr(hba, 0x04 / 4, rd(hba, 0x04 / 4) | (1 << 31)); // GHC.AE
    let pi = rd(hba, 0x0c / 4);
    // One MSI vector for all ports (INTx stays masked).
    dev.set_intx_disabled(true);
    let shared: &'static HbaIrq = alloc::boxed::Box::leak(alloc::boxed::Box::new(HbaIrq {
        hba,
        ports: [const { AtomicUsize::new(0) }; 32],
    }));
    let irq = msi::setup(dev, false, interrupt, shared as *const HbaIrq as usize);
    if let Some((v, kind)) = irq {
        crate::println!("ahci {}: {kind} vector {v:#x}", dev.bdf());
    }
    for port in 0..32 {
        if pi & (1 << port) == 0 {
            continue;
        }
        let regs = unsafe { hba.add((0x100 + port * 0x80) / 4) };
        let ssts = rd(regs, PXSSTS);
        if ssts & 0xf != 3 {
            continue;
        }
        let kind = match rd(regs, PXSIG) {
            0xeb14_0101 => Kind::Atapi,
            0x0000_0101 => Kind::Ata,
            _ => continue,
        };
        // 1 KB list + 256 B FIS + 256 B table, then the bounce buffer.
        let pages = (4096 + BOUNCE) / 4096;
        let Some(mem) = phys::alloc_contig(pages as u64, 1 << 32) else { continue };
        let pirq = irq.map(|_| {
            let p: &'static PortIrq = alloc::boxed::Box::leak(alloc::boxed::Box::new(PortIrq {
                regs,
                status: AtomicU32::new(0),
                event: Event::new(),
            }));
            p.event.arm();
            shared.ports[port].store(p as *const PortIrq as usize, Ordering::Release);
            p
        });
        let st = PortState { regs, clb: mem, fb: mem + 1024, ctba: mem + 2048, buf: mem + 4096, irq: pirq };
        st.stop();
        wr(regs, PXCLB, st.clb as u32);
        wr(regs, PXCLBU, (st.clb >> 32) as u32);
        wr(regs, PXFB, st.fb as u32);
        wr(regs, PXFBU, (st.fb >> 32) as u32);
        wr(regs, PXSERR, 0xffff_ffff);
        wr(regs, PXIS, 0xffff_ffff);
        wr(regs, PXIE, if st.irq.is_some() { PXIE_MASK } else { 0 });
        st.start();
        let Some((model, sector_size, sectors)) = Port::identify(&st, kind) else { continue };
        crate::println!(
            "ahci: port {port}: {} {model}, {} MB",
            if kind == Kind::Ata { "disk" } else { "optical" },
            sectors * sector_size as u64 >> 20
        );
        out.push(Arc::new(Port { kind, model, sector_size, sectors, state: Mutex::new(st) }));
    }
    if irq.is_some() {
        wr(hba, 0x08 / 4, 0xffff_ffff);
        wr(hba, 0x04 / 4, rd(hba, 0x04 / 4) | 2); // GHC.IE
    }
    out
}
