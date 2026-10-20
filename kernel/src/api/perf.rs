//! Performance counters ("perfview"): DosConfigurePerf and
//! DosAliasPerfCtrs, undocumented calls that OS/2's Pulse (MPCPUMON.EXE)
//! reads its per-processor CPU load through.
//!
//! In OS/2 the kernel keeps a list of registered performance blocks
//! (RPDs, `_pPvwRPDHead`), each with a counter block (the live values)
//! and a text block describing it, and DosAliasPerfCtrs maps either into
//! the caller. Worked out from OS2KRNLB (`_VR32ConfigurePerf`,
//! `_VR32AliasPerfCtrs`, `_pvwFindRPDesc`, `_pvwInitPersist`,
//! `_PVWRegisterPerfCtrs`, `_pvwDupTextBlk`, the `_pvtbh_cpu` and
//! `_apvtbn_cpu` tables) and from MPCPUMON's use of them.
//!
//! rs2 registers only the per-processor blocks, which the kernel's
//! `_pvwInitPersist` registers first, so RPD n (n = 1, 2, ...) is
//! processor n. A counter block is 0x30 bytes:
//!
//! | offset | field                                                  |
//! |--------|--------------------------------------------------------|
//! | 0x00   | size (0x30)                                            |
//! | 0x04   | handle: instance (low word), RPD id (high word)        |
//! | 0x08   | flags (0x0b)                                           |
//! | 0x18   | tmNotIdle: time not idle, 64-bit, DosTmrQueryTime units |
//! | 0x20   | ctSched: dispatches                                    |
//! | 0x24   | ctInt: interrupts                                      |
//! | 0x28   | tmInt: time in interrupts (always 0 on rs2)            |
//!
//! A timer counter is kept the way the kernel's TMR32AddTime/SubTime
//! keep it: while the interval runs it holds accumulated time minus its
//! start time, which is negative; a reader adds the current
//! DosTmrQueryTime to a negative value. So the counter only changes when
//! a processor switches between its idle thread and anything else.
//!
//! A text block: a 0x30-byte header (0x02000000, handle, flags 0x50,
//! offsets of the block's name and three more strings at 0x10 and
//! 0x1c..0x24, the counter count at 0x28 and the offset of the counter
//! table at 0x2c), the table (12 bytes a counter: type 1 = timer or
//! 0 = count, size, name offset), then the strings. Offsets are from the
//! start of the block. Processor 0 is named "CPU", the others "CPU1"...

use alloc::collections::BTreeSet;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};

use super::{err, Ctx};
use crate::smp::MAX_CPUS;
use crate::sync::{Once, SpinLock};
use crate::vm::{self, PageState, SegKind, SharedMem, PAGE};

const ERROR_PVW_INVALID_ENTITY_TYPE: u32 = 538;
const ERROR_PVW_INVALID_SPEC: u32 = 539;
const ERROR_PVW_INVALID_RANGE_TYPE: u32 = 540;
const ERROR_PRF_NOT_INITIALIZED: u32 = 543;

/// Counter blocks first, text blocks after them, in one shared object.
const DATA_SIZE: usize = 0x30;
const TEXT_BASE: usize = DATA_SIZE * MAX_CPUS;
const TEXT_STRIDE: usize = 0x88;
const SIZE: u32 = 2 * PAGE;
const _: () = assert!(TEXT_BASE + TEXT_STRIDE * MAX_CPUS <= SIZE as usize);

const TM_NOT_IDLE: usize = 0x18;
const CT_SCHED: usize = 0x20;
const CT_INT: usize = 0x24;

/// (type, size, name) of the processor block's counters (`_apvtbn_cpu`).
const COUNTERS: [(u32, u32, &str); 4] = [(1, 8, "tmNotIdle"), (0, 4, "ctSched"), (0, 4, "ctInt"), (1, 8, "tmInt")];

static OBJ: Once<Arc<SharedMem>> = Once::new();
/// Kernel address of the object (null until `init`), for the scheduler.
static PAGE0: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
/// Text block sizes, by processor.
static TEXT_SIZE: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(0) }; MAX_CPUS];
/// DosConfigurePerf has configured the kernel blocks.
static CONFIGURED: AtomicBool = AtomicBool::new(false);
/// Processes the object is mapped into.
static MAPPED: SpinLock<BTreeSet<u32>> = SpinLock::new(BTreeSet::new());

pub fn init() {
    let base = vm::reserve_shared(SIZE, None).expect("perf counter address");
    let obj = SharedMem::new(base, SIZE, vm::pag::READ, None);
    for i in 0..(SIZE / PAGE) as usize {
        obj.frame(i).expect("perf counter memory");
        obj.pages.lock()[i] = PageState::Committed;
    }
    let pages: [u64; 2] = {
        let f = obj.frames.lock();
        [f[0], f[1]]
    };
    // The frames need not be contiguous: build the blocks in a buffer and
    // copy each page.
    let mut b = alloc::vec![0u8; SIZE as usize];
    for cpu in 0..MAX_CPUS {
        let handle = ((cpu as u32 + 1) << 16).to_le_bytes();
        let d = &mut b[cpu * DATA_SIZE..][..DATA_SIZE];
        d[0..4].copy_from_slice(&(DATA_SIZE as u32).to_le_bytes());
        d[4..8].copy_from_slice(&handle);
        d[8] = 0x0b;
        let t = text_block(cpu, handle);
        TEXT_SIZE[cpu].store(t.len() as u32, Ordering::Relaxed);
        b[TEXT_BASE + cpu * TEXT_STRIDE..][..t.len()].copy_from_slice(&t);
    }
    for (i, f) in pages.iter().enumerate() {
        let p = crate::mm::phys::phys_to_virt(*f);
        unsafe { core::ptr::copy_nonoverlapping(b[i * PAGE as usize..].as_ptr(), p, PAGE as usize) };
    }
    // The counter blocks all sit in the first page.
    PAGE0.store(crate::mm::phys::phys_to_virt(pages[0]), Ordering::Release);
    OBJ.set(obj);
}

fn text_block(cpu: usize, handle: [u8; 4]) -> alloc::vec::Vec<u8> {
    let name = if cpu == 0 { alloc::string::String::from("CPU") } else { alloc::format!("CPU{cpu}") };
    let table = 0x30;
    let mut strings = table + COUNTERS.len() * 12;
    let mut b = alloc::vec![0u8; strings];
    let put = |b: &mut alloc::vec::Vec<u8>, at: usize, s: &str, strings: &mut usize| {
        b[at..at + 4].copy_from_slice(&(*strings as u32).to_le_bytes());
        b.extend_from_slice(s.as_bytes());
        b.push(0);
        *strings += s.len() + 1;
    };
    b[0..4].copy_from_slice(&0x0200_0000u32.to_le_bytes());
    b[4..8].copy_from_slice(&handle);
    b[8] = 0x50;
    put(&mut b, 0x10, &name, &mut strings);
    for at in [0x1c, 0x20, 0x24] {
        put(&mut b, at, "", &mut strings);
    }
    b[0x28..0x2c].copy_from_slice(&(COUNTERS.len() as u32).to_le_bytes());
    b[0x2c..0x30].copy_from_slice(&(table as u32).to_le_bytes());
    for (i, (kind, size, name)) in COUNTERS.iter().enumerate() {
        let e = table + i * 12;
        b[e..e + 4].copy_from_slice(&kind.to_le_bytes());
        b[e + 4..e + 8].copy_from_slice(&size.to_le_bytes());
        put(&mut b, e + 8, name, &mut strings);
    }
    b
}

fn counter(cpu: usize, off: usize) -> Option<&'static AtomicU64> {
    let p = PAGE0.load(Ordering::Acquire);
    if p.is_null() || cpu >= MAX_CPUS {
        return None;
    }
    // Aligned: a reader never sees half of an update.
    Some(unsafe { &*(p.add(cpu * DATA_SIZE + off) as *const AtomicU64) })
}

fn counter32(cpu: usize, off: usize) -> Option<&'static AtomicU32> {
    counter(cpu, off & !7).map(|c| unsafe { &*((c as *const AtomicU64 as *const AtomicU32).add((off & 7) / 4)) })
}

/// The scheduler switches this processor between threads (interrupts
/// off): count the dispatch and, between the idle thread and another
/// thread, start or stop the not-idle interval.
pub fn switched(cpu: usize, from_idle: bool, to_idle: bool) {
    if let Some(c) = counter32(cpu, CT_SCHED) {
        c.fetch_add(1, Ordering::Relaxed);
    }
    if from_idle == to_idle {
        return;
    }
    if let Some(c) = counter(cpu, TM_NOT_IDLE) {
        let now = super::sysctl::tmr_now();
        if from_idle {
            c.fetch_sub(now, Ordering::Relaxed);
        } else {
            c.fetch_add(now, Ordering::Relaxed);
        }
    }
}

/// An interrupt arrived on this processor.
pub fn interrupt(cpu: usize) {
    if let Some(c) = counter32(cpu, CT_INT) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// Time processor `cpu` was not idle, in DosTmrQueryTime units.
pub fn busy_time(cpu: usize) -> u64 {
    let Some(c) = counter(cpu, TM_NOT_IDLE) else { return 0 };
    let v = c.load(Ordering::Relaxed);
    if (v as i64) < 0 { v.wrapping_add(super::sysctl::tmr_now()) } else { v }
}

/// DosConfigurePerf(flFlags, ulCommand, ...): 0 configures (flag 0x20:
/// the kernel's blocks), 3 calibrates the timers, 4 deconfigures. The
/// kernel's blocks are kept up to date from boot, so configuring only
/// makes them visible.
pub fn dos_configure_perf(c: &mut Ctx) -> u32 {
    let (flags, cmd) = (c.arg(0), c.arg(1));
    match cmd {
        0 => {
            if flags & 0x20 != 0 {
                CONFIGURED.store(true, Ordering::Relaxed);
            }
            0
        }
        3 if flags & 0xff == 0 => 0,
        4 if flags & 0xff == 0 => {
            CONFIGURED.store(false, Ordering::Relaxed);
            0
        }
        4 => err::CALL_NOT_IMPLEMENTED,
        _ => ERROR_PVW_INVALID_ENTITY_TYPE,
    }
}

/// DosAliasPerfCtrs(flFlags, ulHandle, ppBlock, pulSize). With flag 1 it
/// works on the registered blocks: the handle names one (RPD id in the
/// high word, instance in the low word; 0 is the list head), flag 2 moves
/// to the next block (instance 0), flag 4 to the next instance (there are
/// none). Then flag 8 maps the block's text block, flag 0x10 its counter
/// block, and the address and size go to *ppBlock and *pulSize.
/// (Flag 0x20 reads perfview's message queue, which rs2 does not have.)
pub fn dos_alias_perf_ctrs(c: &mut Ctx) -> u32 {
    let (flags, handle, pp, psize) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if flags & 1 == 0 {
        return if flags & 0x20 != 0 { ERROR_PRF_NOT_INITIALIZED } else { ERROR_PVW_INVALID_RANGE_TYPE };
    }
    if !CONFIGURED.load(Ordering::Relaxed) {
        return err::NO_MORE_ITEMS;
    }
    let ncpu = crate::smp::online_count();
    let (mut id, mut inst) = (handle >> 16, handle & 0xffff);
    if flags & 2 != 0 {
        inst = 0;
    }
    // Find the block (0 = the list head).
    if (id == 0 && inst != 0) || id > ncpu || (id != 0 && inst != 0) {
        return ERROR_PVW_INVALID_SPEC;
    }
    if flags & 2 != 0 {
        id += 1;
        if id > ncpu {
            return err::NO_MORE_ITEMS;
        }
    } else if flags & 4 != 0 {
        return err::NO_MORE_ITEMS;
    }
    if id == 0 {
        // The list head has no blocks.
        return ERROR_PVW_INVALID_RANGE_TYPE;
    }
    let cpu = id as usize - 1;
    let (off, size) = if flags & 8 != 0 {
        (TEXT_BASE + cpu * TEXT_STRIDE, TEXT_SIZE[cpu].load(Ordering::Relaxed))
    } else if flags & 0x10 != 0 {
        (cpu * DATA_SIZE, DATA_SIZE as u32)
    } else {
        return ERROR_PVW_INVALID_RANGE_TYPE;
    };
    let Some(obj) = OBJ.get() else { return ERROR_PVW_INVALID_RANGE_TYPE };
    // OS/2 maps each block into one process once (ERROR_ALREADY_EXISTS
    // after that); here every block is in one read-only object mapped
    // once into each process that asks.
    if MAPPED.lock().insert(c.process.pid) {
        let obj = obj.clone();
        c.space().map_shared(obj, vm::pag::READ, SegKind::None);
    }
    let addr = obj.base + off as u32;
    if !c.write_u32(pp, addr) {
        return err::INVALID_ADDRESS;
    }
    if psize != 0 && !c.write_u32(psize, size) {
        return err::INVALID_ADDRESS;
    }
    0
}

/// A process ended: its mapping went with its address space.
pub fn process_ended(pid: u32) {
    MAPPED.lock().remove(&pid);
}
