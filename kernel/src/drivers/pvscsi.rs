//! VMware paravirtual SCSI (PVSCSI, PCI 15ad:07c0), the fast SCSI
//! controller of VMware VMs (also emulated by QEMU).
//!
//! The device takes commands through a register window: a command code
//! written to COMMAND, its descriptor dword by dword to COMMAND_DATA.
//! SCSI requests travel through rings in guest memory: a request ring of
//! 128-byte descriptors (CDB, target, LUN, data and sense buffers), a
//! completion ring of 32-byte descriptors (status, bytes transferred) and
//! a shared page with the producer/consumer indexes. A write to a kick
//! register makes the device look at new requests.
//!
//! One request is in flight at a time per controller; data goes through
//! a physically contiguous bounce buffer, and completion is polled
//! (yielding the CPU): rs2 does not route PCI interrupts yet. Targets
//! 0-63 on bus 0 are scanned, LUN 0 of each; the units are handled by
//! `scsi`.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::pci;
use super::scsi::{self, Dir, Error, Hba};
use crate::fs::block::BlockDev;
use crate::mm::{mmio, phys};
use crate::sync::Mutex;

const PAGE: usize = 4096;
/// Register window size.
const MEM_SPACE: u64 = 0x8000;
/// Bounce buffer; transfers are split into chunks of at most this.
const BOUNCE: usize = 64 * 1024;
const TIMEOUT_MS: u64 = 10_000;
/// Targets scanned on bus 0.
const MAX_TARGETS: u8 = 64;

// Registers (byte offsets).
const REG_COMMAND: usize = 0x0;
const REG_COMMAND_DATA: usize = 0x4;
const REG_COMMAND_STATUS: usize = 0x8;
const REG_INTR_MASK: usize = 0x2010;
const REG_KICK_NON_RW_IO: usize = 0x3014;
const REG_KICK_RW_IO: usize = 0x4018;

// Adapter commands.
const CMD_ADAPTER_RESET: u32 = 1;
const CMD_SETUP_RINGS: u32 = 3;
/// Most ring pages SETUP_RINGS can describe.
const SETUP_RINGS_MAX_PAGES: usize = 32;

// Request descriptor flags.
const FLAG_DIR_NONE: u32 = 1 << 2;
const FLAG_DIR_TOHOST: u32 = 1 << 3;
const FLAG_DIR_TODEVICE: u32 = 1 << 4;

const REQ_SIZE: usize = 128;
const CMP_SIZE: usize = 32;
const SIMPLE_QUEUE_TAG: u8 = 0x20;
const SENSE_LEN: usize = 96;

// Rings state page (byte offsets).
const RS_REQ_PROD: usize = 0;
const RS_REQ_NUM_LOG2: usize = 8;
const RS_CMP_PROD: usize = 12;
const RS_CMP_CONS: usize = 16;
const RS_CMP_NUM_LOG2: usize = 20;

// Host adapter status of a completion (BTSTAT_*).
const BT_SUCCESS: u16 = 0x00;
const BT_LINKED_COMMAND_COMPLETED: u16 = 0x0a;
const BT_LINKED_COMMAND_COMPLETED_WITH_FLAG: u16 = 0x0b;
const BT_DATA_UNDERRUN: u16 = 0x0c;
const BT_DATARUN: u16 = 0x12;

/// Controller state behind the lock.
pub struct State {
    regs: *mut u8,
    /// Rings state page, request ring, completion ring (one page each),
    /// sense buffer and bounce buffer: physical addresses.
    rings: u64,
    req: u64,
    cmp: u64,
    sense: u64,
    buf: u64,
    req_entries: u32,
    cmp_entries: u32,
    context: u64,
    /// A request timed out: the rings are out of step.
    failed: bool,
}

unsafe impl Send for State {}

fn rd32(regs: *mut u8, off: usize) -> u32 {
    unsafe { read_volatile(regs.add(off) as *const u32) }
}

fn wr32(regs: *mut u8, off: usize, v: u32) {
    unsafe { write_volatile(regs.add(off) as *mut u32, v) }
}

impl State {
    /// An adapter command with its descriptor; returns COMMAND_STATUS.
    fn adapter_command(&self, cmd: u32, data: &[u32]) -> u32 {
        wr32(self.regs, REG_COMMAND, cmd);
        for &d in data {
            wr32(self.regs, REG_COMMAND_DATA, d);
        }
        rd32(self.regs, REG_COMMAND_STATUS)
    }

    fn rs(&self, off: usize) -> *mut u32 {
        unsafe { phys::phys_to_virt(self.rings).add(off) as *mut u32 }
    }
}

impl Hba for State {
    fn name(&self) -> &'static str {
        "pvscsi"
    }

    fn bounce(&self) -> *mut u8 {
        phys::phys_to_virt(self.buf)
    }

    fn bounce_size(&self) -> usize {
        BOUNCE
    }

    fn exec(&mut self, target: u8, lun: u8, cdb: &[u8], dir: Dir) -> Result<usize, Error> {
        if self.failed {
            return Err(Error::Timeout);
        }
        let (len, flags) = match dir {
            Dir::None => (0, FLAG_DIR_NONE),
            Dir::In(n) => (n, FLAG_DIR_TOHOST),
            Dir::Out(n) => (n, FLAG_DIR_TODEVICE),
        };
        self.context = self.context.wrapping_add(1);
        let ctx = self.context;
        unsafe {
            core::ptr::write_bytes(phys::phys_to_virt(self.sense), 0, SENSE_LEN);
            let prod = read_volatile(self.rs(RS_REQ_PROD));
            let d = phys::phys_to_virt(self.req).add((prod % self.req_entries) as usize * REQ_SIZE);
            core::ptr::write_bytes(d, 0, REQ_SIZE);
            (d as *mut u64).write_volatile(ctx);
            (d.add(8) as *mut u64).write_volatile(if len > 0 { self.buf } else { 0 });
            (d.add(16) as *mut u64).write_volatile(len as u64);
            (d.add(24) as *mut u64).write_volatile(self.sense);
            (d.add(32) as *mut u32).write_volatile(SENSE_LEN as u32);
            (d.add(36) as *mut u32).write_volatile(flags);
            core::ptr::copy_nonoverlapping(cdb.as_ptr(), d.add(40), cdb.len().min(16));
            *d.add(56) = cdb.len() as u8;
            // Single-level SAM LUN.
            *d.add(57 + 1) = lun;
            *d.add(65) = SIMPLE_QUEUE_TAG;
            *d.add(66) = 0; // bus
            *d.add(67) = target;
            fence(Ordering::SeqCst);
            write_volatile(self.rs(RS_REQ_PROD), prod.wrapping_add(1));
        }
        fence(Ordering::SeqCst);
        let rw = matches!(cdb[0], 0x08 | 0x0a | 0x28 | 0x2a | 0x88 | 0x8a | 0xa8 | 0xaa);
        wr32(self.regs, if rw { REG_KICK_RW_IO } else { REG_KICK_NON_RW_IO }, 0);
        let (prod, cons) = (self.rs(RS_CMP_PROD), self.rs(RS_CMP_CONS));
        let cons_idx = unsafe { read_volatile(cons) };
        if !scsi::wait(TIMEOUT_MS, || unsafe { read_volatile(prod) } != cons_idx) {
            crate::println!("pvscsi: command {:#x} to target {target} timed out", cdb[0]);
            self.failed = true;
            return Err(Error::Timeout);
        }
        fence(Ordering::SeqCst);
        let (rctx, got, host, status) = unsafe {
            let c = phys::phys_to_virt(self.cmp).add((cons_idx % self.cmp_entries) as usize * CMP_SIZE);
            let r = (
                (c as *const u64).read_volatile(),
                (c.add(8) as *const u64).read_volatile(),
                (c.add(20) as *const u16).read_volatile(),
                (c.add(22) as *const u16).read_volatile(),
            );
            fence(Ordering::SeqCst);
            write_volatile(cons, cons_idx.wrapping_add(1));
            r
        };
        if rctx != ctx {
            crate::println!("pvscsi: completion for request {rctx}, expected {ctx}");
            self.failed = true;
            return Err(Error::Timeout);
        }
        // The completion's byte count is only meaningful for a short
        // transfer (QEMU reports those as DATARUN).
        let got = match host {
            BT_SUCCESS | BT_LINKED_COMMAND_COMPLETED | BT_LINKED_COMMAND_COMPLETED_WITH_FLAG => len,
            BT_DATA_UNDERRUN | BT_DATARUN if (got as usize) < len => got as usize,
            h => return Err(Error::Host(h as u32)),
        };
        match status as u8 {
            scsi::STATUS_GOOD => Ok(got),
            scsi::STATUS_CHECK_CONDITION => {
                let s = unsafe { core::slice::from_raw_parts(phys::phys_to_virt(self.sense), 18) };
                Err(scsi::parse_sense(s).unwrap_or(Error::Check))
            }
            s => Err(Error::Status(s)),
        }
    }
}

/// The register window: the first memory BAR (VMware puts an I/O BAR
/// first, QEMU only has the memory BAR).
pub(super) fn mem_bar(dev: &pci::Device) -> Option<u64> {
    let mut n = 0;
    while n < 6 {
        let lo = dev.read(0x10 + n * 4);
        if lo & 1 == 0 && dev.bar(n) != 0 {
            return Some(dev.bar(n));
        }
        // A 64-bit memory BAR takes two slots.
        n += if lo & 1 == 0 && (lo >> 1) & 3 == 2 { 2 } else { 1 };
    }
    None
}

/// Bring up a PVSCSI controller and return its units, each with whether
/// it is an optical drive.
pub fn probe(dev: &pci::Device) -> Vec<(Arc<dyn BlockDev>, bool)> {
    let mut out: Vec<(Arc<dyn BlockDev>, bool)> = Vec::new();
    dev.enable();
    // Polled: mask legacy INTx.
    dev.write(0x04, (dev.read(0x04) & 0xffff) | 1 << 10);
    let Some(bar) = mem_bar(dev) else {
        crate::println!("pvscsi: no register window");
        return out;
    };
    let regs = mmio::map(bar, MEM_SPACE);
    // Rings state, request ring, completion ring, sense, bounce buffer.
    let pages = 4 + BOUNCE / PAGE;
    let Some(mem) = phys::alloc_contig(pages as u64, u64::MAX) else { return out };
    let page = |i: usize| mem + (i * PAGE) as u64;
    let mut st = State {
        regs,
        rings: page(0),
        req: page(1),
        cmp: page(2),
        sense: page(3),
        buf: page(4),
        req_entries: (PAGE / REQ_SIZE) as u32,
        cmp_entries: (PAGE / CMP_SIZE) as u32,
        context: 0,
        failed: false,
    };
    st.adapter_command(CMD_ADAPTER_RESET, &[]);
    wr32(regs, REG_INTR_MASK, 0);
    // SETUP_RINGS: page counts, rings state PPN, request and completion
    // ring PPNs (32 slots each).
    let mut d = Vec::with_capacity(4 + 4 * SETUP_RINGS_MAX_PAGES);
    let ppn = |p: u64| [(p >> 12) as u32, (p >> 44) as u32];
    d.extend_from_slice(&[1, 1]);
    d.extend_from_slice(&ppn(st.rings));
    for i in 0..SETUP_RINGS_MAX_PAGES {
        d.extend_from_slice(&if i == 0 { ppn(st.req) } else { [0, 0] });
    }
    for i in 0..SETUP_RINGS_MAX_PAGES {
        d.extend_from_slice(&if i == 0 { ppn(st.cmp) } else { [0, 0] });
    }
    let status = st.adapter_command(CMD_SETUP_RINGS, &d);
    if status != 0 {
        crate::println!("pvscsi: SETUP_RINGS failed ({status:#x})");
        return out;
    }
    // The device reports the ring sizes it derived from the page counts.
    let (rl, cl) = unsafe { (read_volatile(st.rs(RS_REQ_NUM_LOG2)), read_volatile(st.rs(RS_CMP_NUM_LOG2))) };
    if (1..=5).contains(&rl) {
        st.req_entries = 1 << rl;
    }
    if (1..=7).contains(&cl) {
        st.cmp_entries = 1 << cl;
    }
    let hba = Arc::new(Mutex::new(st));
    for target in 0..MAX_TARGETS {
        if let Some((unit, optical)) = scsi::setup_unit(&hba, target, 0) {
            out.push((Arc::new(unit), optical));
        }
        if hba.lock().failed {
            break;
        }
    }
    crate::println!("pvscsi: {:02x}:{:02x}.{}: {} unit(s)", dev.bus, dev.dev, dev.func, out.len());
    out
}
