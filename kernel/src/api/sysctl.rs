//! System services: DosSysCtl, DosSystemService, DosQueryRASInfo,
//! DosQueryABIOSSupport, DosPerfSysCall (CPU utilisation), DosErrClass,
//! processor status, spin locks, DosShutdown, the high-resolution timer
//! (DosTmrQueryFreq/DosTmrQueryTime from the TSC) and rs2's kernel object
//! counters (Rs2QueryCounters).

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::{err, Ctx};
use crate::irq;
use crate::sync::SpinLock;

// ---- High-resolution timer ----

/// Frequency of DosTmrQueryTime (the 8254 input clock, as OS/2 reports).
pub const TMR_FREQ: u64 = 1_193_167;

/// TSC at the last timer tick and TSC counts per tick (running estimate).
static TICK_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_PER_TICK: AtomicU64 = AtomicU64::new(0);
static LAST_TMR: AtomicU64 = AtomicU64::new(0);

/// Called from the timer interrupt.
pub fn tick() {
    let now = crate::arch::rdtsc();
    let last = TICK_TSC.swap(now, Ordering::Relaxed);
    if last != 0 && now > last {
        let d = now - last;
        let est = TSC_PER_TICK.load(Ordering::Relaxed);
        // Ignore ticks that were delayed (interrupts off for a while).
        let new = if est == 0 { d } else if d < est * 2 { (est * 7 + d) / 8 } else { est };
        TSC_PER_TICK.store(new, Ordering::Relaxed);
    }
}

/// Time since boot in units of TMR_FREQ: timer ticks plus the TSC
/// interpolation within the current tick. Never goes backwards.
pub fn tmr_now() -> u64 {
    let per_tick = TMR_FREQ / irq::HZ;
    // A consistent snapshot: retry if a tick came in between.
    let (t, base, per) = loop {
        let t = irq::ticks();
        let base = TICK_TSC.load(Ordering::Relaxed);
        let per = TSC_PER_TICK.load(Ordering::Relaxed);
        if irq::ticks() == t {
            break (t, base, per);
        }
    };
    let mut v = t * TMR_FREQ / irq::HZ;
    if per != 0 && base != 0 {
        let d = crate::arch::rdtsc().saturating_sub(base);
        v += (d * per_tick / per).min(per_tick - 1);
    }
    // Monotonic across ticks whose interpolation ran ahead.
    let prev = LAST_TMR.fetch_max(v, Ordering::Relaxed);
    v.max(prev)
}

pub fn dos_tmr_query_freq(c: &mut Ctx) -> u32 {
    if c.write_u32(c.arg(0), TMR_FREQ as u32) { 0 } else { err::INVALID_ADDRESS }
}

pub fn dos_tmr_query_time(c: &mut Ctx) -> u32 {
    let v = tmr_now();
    let p = c.arg(0);
    if c.write_u32(p, v as u32) && c.write_u32(p + 4, (v >> 32) as u32) { 0 } else { err::INVALID_ADDRESS }
}

// ---- DosPerfSysCall ----

const CMD_KI_ENABLE: u32 = 0x60;
const CMD_KI_DISABLE: u32 = 0x61;
const CMD_KI_RDCNT: u32 = 0x63;
const CMD_SOFTTRACE_LOG: u32 = 0x14;

/// DosPerfSysCall(ulCommand, ulParm1, ulParm2, ulParm3). CMD_KI_RDCNT
/// fills one CPUUTIL per processor: total, idle, busy and interrupt time
/// (64-bit counts at DosTmrQueryFreq's frequency, from the processors'
/// performance counters, see perf.rs).
pub fn dos_perf_sys_call(c: &mut Ctx) -> u32 {
    let (cmd, p1) = (c.arg(0), c.arg(1));
    match cmd {
        CMD_KI_ENABLE | CMD_KI_DISABLE | CMD_SOFTTRACE_LOG => 0,
        CMD_KI_RDCNT => {
            let total = tmr_now();
            let mut b = alloc::vec::Vec::new();
            for cpu in 0..crate::smp::online_count() as usize {
                let busy = super::perf::busy_time(cpu).min(total);
                for v in [total, total - busy, busy, 0] {
                    b.extend_from_slice(&(v as u32).to_le_bytes());
                    b.extend_from_slice(&((v >> 32) as u32).to_le_bytes());
                }
            }
            if c.write(p1, &b) { 0 } else { err::INVALID_ADDRESS }
        }
        _ => err::INVALID_FUNCTION,
    }
}

// ---- Processors and spin locks ----

fn valid_processor(id: u32) -> bool {
    (1..=crate::smp::online_count()).contains(&id)
}

/// DosGetProcessorStatus(procid, pulStatus): processors 1..n, all online.
pub fn dos_get_processor_status(c: &mut Ctx) -> u32 {
    if !valid_processor(c.arg(0)) {
        return err::INVALID_PARAMETER;
    }
    if c.write_u32(c.arg(1), 1) { 0 } else { err::INVALID_ADDRESS } // PROC_ONLINE
}

/// DosSetProcessorStatus(procid, status): processors stay online.
pub fn dos_set_processor_status(c: &mut Ctx) -> u32 {
    match (valid_processor(c.arg(0)), c.arg(1)) {
        (true, 1) => 0,
        (true, _) => err::INVALID_FUNCTION,
        _ => err::INVALID_PARAMETER,
    }
}

/// Spin locks (DosCreateSpinLock and friends): on one processor a spin
/// lock only has to keep other threads of the system out, so acquiring it
/// waits (yielding) while another thread holds it.
static SPINLOCKS: SpinLock<alloc::collections::BTreeMap<u32, (u32, u32)>> = SpinLock::new(alloc::collections::BTreeMap::new());
static NEXT_SPIN: AtomicU32 = AtomicU32::new(1);

pub fn dos_create_spin_lock(c: &mut Ctx) -> u32 {
    let h = NEXT_SPIN.fetch_add(1, Ordering::Relaxed);
    SPINLOCKS.lock().insert(h, (c.process.pid, 0));
    let p = if c.is16 { c.ptr(0) } else { c.arg(0) };
    if c.write_u32(p, h) { 0 } else { err::INVALID_ADDRESS }
}

pub fn dos_acquire_spin_lock(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    let me = c.thread.id;
    loop {
        {
            let mut l = SPINLOCKS.lock();
            let Some(e) = l.get_mut(&h) else { return err::INVALID_HANDLE };
            if e.1 == 0 || e.1 == me {
                e.1 = me;
                return 0;
            }
        }
        if c.thread.exit_requested() {
            return err::INTERRUPT;
        }
        crate::task::sched::yield_now();
    }
}

pub fn dos_release_spin_lock(c: &mut Ctx) -> u32 {
    if let Some(e) = SPINLOCKS.lock().get_mut(&c.arg(0)) {
        e.1 = 0;
    }
    0
}

pub fn dos_free_spin_lock(c: &mut Ctx) -> u32 {
    if SPINLOCKS.lock().remove(&c.arg(0)).is_some() { 0 } else { err::INVALID_HANDLE }
}

/// A process ended: its spin locks go away.
pub fn process_ended(pid: u32) {
    SPINLOCKS.lock().retain(|_, e| e.0 != pid);
}

/// DosTestPSD(pszPSDFileName): no platform-specific drivers are loaded
/// (the multiprocessor support they provide is the kernel's own).
pub fn dos_test_psd(c: &mut Ctx) -> u32 {
    if c.read_str(c.arg(0), 260).is_none() {
        return err::INVALID_ADDRESS;
    }
    err::FILE_NOT_FOUND
}

// ---- ABIOS, RAS ----

/// DosQueryABIOSSupport(reserved): HW_CFG_* flags as the return value.
/// A PC with PCI and a Pentium-class CPU, no ABIOS, no MCA/EISA.
pub fn dos_query_abios_support(_c: &mut Ctx) -> u32 {
    0x10 | 0x40 // HW_CFG_PCI | HW_CFG_PENTIUM_CPU
}

/// DosQueryRASInfo(Index, ppAddr): SIS_MMIOADDR (0), SIS_MEC_TABLE (1),
/// SIS_SYS_LOG (2), SIS_PERF_MEC_TABLE (3). The tracing tables are a
/// zeroed (all tracing off) read-only page in the process; there is no
/// memory-mapped I/O area or system log.
pub fn dos_query_ras_info(c: &mut Ctx) -> u32 {
    let (index, paddr) = (c.arg(0), c.arg(1));
    let addr = match index {
        0 => 0,
        1..=3 => {
            let r = c.space().alloc(4096, crate::vm::pag::READ | crate::vm::pag::COMMIT, crate::vm::SegKind::Data, None);
            match r {
                Ok(a) => a,
                Err(e) => return e.code(),
            }
        }
        _ => return err::INVALID_PARAMETER,
    };
    if c.write_u32(paddr, addr) { 0 } else { err::INVALID_ADDRESS }
}

// ---- DosSysCtl, DosSystemService ----

static SYSCTL_SEEN: AtomicU32 = AtomicU32::new(0);

/// DosSysCtl(ulFunction, pParms): the kernel's private control calls
/// (swapper start-up, configuration queries, ...). OS/2 dispatches 31
/// functions (11..20 privileged); rs2 has none of their subsystems, so it
/// gives the documented answers: ERROR_ACCESS_DENIED for the privileged
/// range, ERROR_INVALID_FUNCTION otherwise. Each function is logged once.
pub fn dos_sys_ctl(c: &mut Ctx) -> u32 {
    let f = c.arg(0);
    if f < 32 && SYSCTL_SEEN.fetch_or(1 << f, Ordering::Relaxed) & (1 << f) == 0 {
        crate::println!("[DosSysCtl function {} from {} not supported]", f, c.process.exe.name);
    }
    if (11..=20).contains(&f) { err::ACCESS_DENIED } else { err::INVALID_FUNCTION }
}

/// DosSystemService(ulService, pParms...): private services of the
/// session manager and hard error daemon. None exist on rs2.
pub fn dos_system_service(_c: &mut Ctx) -> u32 {
    err::INVALID_FUNCTION
}

// ---- DosShutdown ----

static SHUT_DOWN: AtomicBool = AtomicBool::new(false);

pub fn is_shut_down() -> bool {
    SHUT_DOWN.load(Ordering::Relaxed)
}

/// DosShutdown(ulReserved): flush every open file and stop the file
/// systems; 1 = only flush. Afterwards it is safe to turn the machine off.
pub fn dos_shutdown(c: &mut Ctx) -> u32 {
    let r = c.arg(0);
    if r > 1 {
        return err::INVALID_PARAMETER;
    }
    let mut n = 0;
    for p in crate::task::process::all() {
        let files: alloc::vec::Vec<_> = p
            .handles
            .lock()
            .iter()
            .filter_map(|h| match h {
                Some(crate::fs::Handle::File(f)) => Some(f.clone()),
                _ => None,
            })
            .collect();
        for f in files {
            let _ = f.file.flush();
            n += 1;
        }
    }
    if r == 0 {
        // Mark journaled volumes clean.
        for d in 0..26 {
            if let Some(fs) = crate::fs::drive(d) {
                fs.shutdown();
            }
        }
    }
    crate::drivers::flush_disks();
    if r == 0 {
        SHUT_DOWN.store(true, Ordering::Relaxed);
        crate::println!("[DosShutdown: {} open files flushed, file systems stopped]", n);
    }
    0
}

// ---- DosErrClass ----

/// (class, action, locus) for an error code, as OS/2's table classifies
/// them. Classes: 1 out of resource, 2 temporary, 3 authorisation,
/// 4 internal, 5 hardware, 6 system, 7 application, 8 not found, 9 bad
/// format, 10 locked, 11 media, 12 already, 13 unknown, 14 can't, 15 time.
/// Actions: 1 retry, 2 delay retry, 3 user, 4 abort, 5 panic, 6 ignore,
/// 7 retry after intervention. Loci: 1 unknown, 2 disk, 3 network,
/// 4 serial device, 5 memory.
pub fn classify(code: u32) -> (u32, u32, u32) {
    match code {
        0 => (0, 0, 0),
        2 | 3 | 15 | 18 | 110 | 123 | 206 => (8, 3, 2),
        126 | 127 | 182 | 191 | 193 => (8, 3, 1),
        4 | 84 | 113 | 290 => (1, 4, 1),
        8 | 14 | 311 => (1, 4, 5),
        5 | 12 | 65 => (3, 3, 1),
        6 | 114 | 187 => (7, 4, 1),
        1 | 87 | 120 | 124 | 186 | 209 => (7, 4, 1),
        11 | 13 => (9, 3, 1),
        19 | 21 | 27 | 28 | 31 => (11, 7, 2),
        23 | 25 | 26 | 29 | 30 => (5, 7, 2),
        32 | 33 | 36 | 108 => (10, 2, 2),
        80 | 183 | 285 => (12, 3, 2),
        112 => (1, 7, 2),
        121 | 640 => (15, 2, 1),
        95 => (2, 1, 1),
        102 | 105 | 298..=301 => (12, 6, 1),
        109 | 232 | 233 => (6, 4, 4),
        _ => (13, 4, 1),
    }
}

/// DosErrClass(code, pClass, pAction, pLocus)
pub fn dos_err_class(c: &mut Ctx) -> u32 {
    let (cl, ac, lo) = classify(c.arg(0));
    if c.is16 {
        c.write_u16(c.ptr(1), cl as u16);
        c.write_u16(c.ptr(2), ac as u16);
        c.write_u16(c.ptr(3), lo as u16);
    } else {
        c.write_u32(c.arg(1), cl);
        c.write_u32(c.arg(2), ac);
        c.write_u32(c.arg(3), lo);
    }
    0
}

// ---- 16-bit DosQSysInfo ----

/// DosQSysInfo(index, pBuf, cbBuf): 0 = maximum path length (USHORT).
pub fn dos16_qsys_info(c: &mut Ctx) -> u32 {
    let (i, p, cb) = (c.arg(0), c.ptr(1), c.arg(2));
    if i != 0 {
        return err::INVALID_PARAMETER;
    }
    if cb < 2 {
        return err::BUFFER_OVERFLOW;
    }
    c.write_u16(p, 260);
    0
}

// ---- Counters ----

/// Rs2QueryCounters(pulBuf, cb): free physical pages, live threads,
/// processes, 32-bit and 16-bit semaphores, shared memory objects, open
/// files, queues, timers, kernel heap use (KB), process objects not yet
/// freed and file locks: for leak checks.
pub fn rs2_query_counters(c: &mut Ctx) -> u32 {
    let procs = crate::task::process::all();
    let (s32, s16) = super::sem::counts();
    let files: u32 = procs
        .iter()
        .map(|p| p.handles.lock().iter().filter(|h| matches!(h, Some(crate::fs::Handle::File(_)))).count() as u32)
        .sum();
    let v = [
        crate::mm::phys::free_count() as u32,
        crate::task::thread::LIVE.load(Ordering::Relaxed),
        procs.len() as u32,
        s32,
        s16,
        super::shmem::count(),
        files,
        super::queue::count(),
        super::timer::count(),
        (crate::mm::heap::used() / 1024) as u32,
        crate::task::process::LIVE.load(Ordering::Relaxed),
        super::filelock::count(),
    ];
    let n = (c.arg(1) as usize / 4).min(v.len());
    let mut b = alloc::vec::Vec::new();
    for x in &v[..n] {
        b.extend_from_slice(&x.to_le_bytes());
    }
    if c.write(c.arg(0), &b) { 0 } else { err::INVALID_ADDRESS }
}
