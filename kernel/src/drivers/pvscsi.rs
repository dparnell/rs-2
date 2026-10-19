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
//! 0-63 on bus 0 are scanned, LUN 0 of each; disks become block devices,
//! CD/DVD drives optical drives.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::pci;
use crate::api::err;
use crate::fs::block::BlockDev;
use crate::mm::{mmio, phys};
use crate::sync::Mutex;

const PAGE: usize = 4096;
/// Register window size.
const MEM_SPACE: u64 = 0x8000;
/// Bounce buffer; transfers are split into chunks of at most this.
const BOUNCE: usize = 64 * 1024;
const TIMEOUT_MS: u64 = 10_000;
/// How long a unit may take to become ready at attach time.
const READY_MS: u64 = 5000;
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

// SCSI.
const TEST_UNIT_READY: u8 = 0x00;
const REQUEST_SENSE: u8 = 0x03;
const INQUIRY: u8 = 0x12;
const READ_CAPACITY_10: u8 = 0x25;
const SERVICE_ACTION_IN: u8 = 0x9e;
const READ_CAPACITY_16: u8 = 0x10;
const STATUS_GOOD: u16 = 0x00;
const STATUS_CHECK_CONDITION: u16 = 0x02;
const NOT_READY: u8 = 2;
const UNIT_ATTENTION: u8 = 6;
const DATA_PROTECT: u8 = 7;
const ASC_NO_MEDIUM: u8 = 0x3a;
const TYPE_DISK: u8 = 0x00;
const TYPE_CDROM: u8 = 0x05;
const TYPE_OPTICAL: u8 = 0x07;
const SENSE_LEN: usize = 96;

#[derive(Clone, Copy, Debug)]
enum Error {
    /// CHECK CONDITION: sense key, ASC, ASCQ.
    Sense(u8, u8, u8),
    /// Another SCSI status (busy, reservation conflict, ...).
    Status(u16),
    /// Host adapter status: no device (selection timeout), bus errors.
    Host(u16),
    Timeout,
}

impl Error {
    fn code(self) -> u32 {
        match self {
            Error::Sense(DATA_PROTECT, _, _) => err::WRITE_PROTECT,
            _ => err::NOT_READY,
        }
    }
}

#[derive(Clone, Copy)]
enum Dir {
    None,
    In(usize),
    Out(usize),
}

/// Controller state behind the lock.
struct State {
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

fn page_u32(p: u64, off: usize) -> *mut u32 {
    unsafe { phys::phys_to_virt(p).add(off) as *mut u32 }
}

/// Poll `cond` for up to `ms` milliseconds, yielding the CPU (time is
/// estimated from the number of polls before the timer runs).
fn wait(ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let t0 = crate::irq::ticks();
    let limit = ms * crate::irq::HZ / 1000;
    let mut polls = 0u64;
    loop {
        if cond() {
            return true;
        }
        polls += 1;
        let now = crate::irq::ticks();
        let expired = if now != t0 { now - t0 > limit } else { polls > ms * 2000 };
        if expired {
            return cond();
        }
        if polls > 100 {
            crate::task::sched::yield_now();
        }
        core::hint::spin_loop();
    }
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
        page_u32(self.rings, off)
    }

    /// One SCSI command, without retries. Ok(bytes transferred).
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
        if !wait(TIMEOUT_MS, || unsafe { read_volatile(prod) } != cons_idx) {
            crate::println!("pvscsi: command {:#x} to target {target} timed out", cdb[0]);
            self.failed = true;
            return Err(Error::Timeout);
        }
        fence(Ordering::SeqCst);
        let (rctx, got, host, scsi) = unsafe {
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
            h => return Err(Error::Host(h)),
        };
        match scsi {
            STATUS_GOOD => Ok(got),
            STATUS_CHECK_CONDITION => Err(self.sense_data(target, lun)),
            s => Err(Error::Status(s)),
        }
    }

    /// Sense data of a failed command: the automatic sense buffer, or
    /// else REQUEST SENSE.
    fn sense_data(&mut self, target: u8, lun: u8) -> Error {
        let s = unsafe { core::slice::from_raw_parts(phys::phys_to_virt(self.sense), 18) };
        let valid = |s: &[u8]| matches!(s[0] & 0x7f, 0x70 | 0x71);
        if valid(s) {
            let e = Error::Sense(s[2] & 0xf, s[12], s[13]);
            unsafe { core::ptr::write_bytes(phys::phys_to_virt(self.sense), 0, SENSE_LEN) };
            return e;
        }
        match self.exec(target, lun, &[REQUEST_SENSE, 0, 0, 0, 18, 0], Dir::In(18)) {
            Ok(_) => {
                let b = unsafe { core::slice::from_raw_parts(phys::phys_to_virt(self.buf), 18) };
                if valid(b) { Error::Sense(b[2] & 0xf, b[12], b[13]) } else { Error::Status(STATUS_CHECK_CONDITION) }
            }
            Err(e) => e,
        }
    }

    /// A command with retries after UNIT ATTENTION and while becoming
    /// ready.
    fn command(&mut self, target: u8, lun: u8, cdb: &[u8], dir: Dir) -> Result<usize, Error> {
        let mut tries = 0;
        loop {
            tries += 1;
            let e = match self.exec(target, lun, cdb, dir) {
                Ok(n) => return Ok(n),
                Err(e) => e,
            };
            let retry = match e {
                Error::Sense(UNIT_ATTENTION, _, _) => true,
                Error::Sense(NOT_READY, 0x04, 0x01) | Error::Status(0x08) => {
                    wait(100, || false);
                    true
                }
                _ => false,
            };
            if !retry || tries >= 4 {
                return Err(e);
            }
        }
    }

    fn bounce(&self) -> *mut u8 {
        phys::phys_to_virt(self.buf)
    }
}

/// A logical unit as a block device.
pub struct Unit {
    st: Arc<Mutex<State>>,
    target: u8,
    lun: u8,
    block: usize,
    blocks: u64,
    optical: bool,
}

impl Unit {
    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let bs = self.block;
        if buf.len() % bs != 0 || lba + (buf.len() / bs) as u64 > self.blocks {
            return Err(err::INVALID_PARAMETER);
        }
        if write && self.optical {
            return Err(err::WRITE_PROTECT);
        }
        let mut st = self.st.lock();
        let per = BOUNCE / bs;
        let total = buf.len() / bs;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(per);
            let bytes = n * bs;
            let at = lba + done as u64;
            let chunk = &mut buf[done * bs..done * bs + bytes];
            if write {
                unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), st.bounce(), bytes) };
            }
            let cdb = super::storage::rw_cdb(write, at, n as u32);
            let dir = if write { Dir::Out(bytes) } else { Dir::In(bytes) };
            match st.command(self.target, self.lun, &cdb, dir) {
                Ok(got) if got == bytes => {}
                r => {
                    let op = if write { "write" } else { "read" };
                    crate::println!("pvscsi: target {} {op} at {at} failed: {r:?}", self.target);
                    return Err(r.err().map_or(err::NOT_READY, Error::code));
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

impl BlockDev for Unit {
    fn sector_size(&self) -> usize {
        self.block
    }

    fn sector_count(&self) -> u64 {
        self.blocks
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.transfer(lba, buf, false)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut tmp = buf.to_vec();
        self.transfer(lba, &mut tmp, true)
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn ascii(b: &[u8]) -> String {
    let s: String = b.iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { ' ' }).collect();
    String::from(s.trim())
}

/// INQUIRY, wait until ready, READ CAPACITY: (block size, blocks, optical).
/// Optical drives without a medium have 0 blocks.
fn setup_unit(st: &mut State, target: u8, lun: u8) -> Option<(usize, u64, bool)> {
    let inq = match st.command(target, lun, &[INQUIRY, 0, 0, 0, 36, 0], Dir::In(36)) {
        Ok(n) if n >= 5 => unsafe { core::slice::from_raw_parts(st.bounce(), 36) }.to_vec(),
        // No device at this target.
        _ => return None,
    };
    let kind = inq[0] & 0x1f;
    let (vendor, product) = (ascii(&inq[8..16]), ascii(&inq[16..32]));
    if inq[0] >> 5 != 0 {
        return None; // no unit at this LUN
    }
    let optical = matches!(kind, TYPE_CDROM | TYPE_OPTICAL);
    if kind != TYPE_DISK && !optical {
        crate::println!("pvscsi: target {target}: {vendor} {product}, device type {kind:#x} not supported");
        return None;
    }
    let mut waited = 0;
    loop {
        match st.command(target, lun, &[TEST_UNIT_READY, 0, 0, 0, 0, 0], Dir::None) {
            Ok(_) => break,
            Err(Error::Sense(NOT_READY, ASC_NO_MEDIUM, _)) if optical => {
                crate::println!("pvscsi: target {target}: optical {vendor} {product}, no medium");
                return Some((2048, 0, true));
            }
            Err(e) if waited >= READY_MS => {
                crate::println!("pvscsi: target {target}: {vendor} {product}, not ready ({e:?})");
                return optical.then_some((2048, 0, true));
            }
            Err(_) => {
                wait(100, || false);
                waited += 100;
            }
        }
    }
    st.command(target, lun, &[READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0], Dir::In(8)).ok()?;
    let cap = unsafe { core::slice::from_raw_parts(st.bounce(), 32) };
    let (mut last, mut block) = (be32(cap) as u64, be32(&cap[4..]) as usize);
    if last == u32::MAX as u64 {
        let mut cdb = [0u8; 16];
        cdb[0] = SERVICE_ACTION_IN;
        cdb[1] = READ_CAPACITY_16;
        cdb[13] = 32;
        st.command(target, lun, &cdb, Dir::In(32)).ok()?;
        let cap = unsafe { core::slice::from_raw_parts(st.bounce(), 32) };
        last = (be32(cap) as u64) << 32 | be32(&cap[4..]) as u64;
        block = be32(&cap[8..]) as usize;
    }
    if block == 0 || block > BOUNCE || !block.is_power_of_two() {
        crate::println!("pvscsi: target {target}: unsupported block size {block}");
        return None;
    }
    let blocks = last + 1;
    crate::println!(
        "pvscsi: target {target}: {} {vendor} {product}, {} MB",
        if optical { "optical" } else { "disk" },
        (blocks * block as u64) >> 20
    );
    Some((block, blocks, optical))
}

/// The register window: the first memory BAR (VMware puts an I/O BAR
/// first, QEMU only has the memory BAR).
fn mem_bar(dev: &pci::Device) -> Option<u64> {
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
    let mut found = Vec::new();
    for target in 0..MAX_TARGETS {
        if let Some(u) = setup_unit(&mut st, target, 0) {
            found.push((target, u));
        }
        if st.failed {
            break;
        }
    }
    crate::println!("pvscsi: {:02x}:{:02x}.{}: {} unit(s)", dev.bus, dev.dev, dev.func, found.len());
    let st = Arc::new(Mutex::new(st));
    for (target, (block, blocks, optical)) in found {
        out.push((Arc::new(Unit { st: st.clone(), target, lun: 0, block, blocks, optical }), optical));
    }
    out
}
