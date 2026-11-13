//! Several CPUs in the kernel at once: threads (and processes) racing on
//! the same kernel objects, so that a missing lock shows. Run with more
//! than one processor (`xtask run --smp 4`); on one it still checks the
//! results. Each check prints how long it took; `smp bench` (alone) is a
//! kernel-call load for timing.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, join, ms, spawn, thread};

const THREADS: u32 = 4;

/// Released when all workers may start (they wait on it).
static GO: AtomicU32 = AtomicU32::new(0);
static ROUND: AtomicU32 = AtomicU32::new(0);
static WINS: AtomicU32 = AtomicU32::new(0);
static DUPS: AtomicU32 = AtomicU32::new(0);
static ODD: AtomicU32 = AtomicU32::new(0);
static MTX: AtomicU32 = AtomicU32::new(0);
/// A plain counter that only the mutex protects (load and store apart).
static COUNTER: AtomicU32 = AtomicU32::new(0);
static BAD: AtomicU32 = AtomicU32::new(0);
static PIPE_R: AtomicU32 = AtomicU32::new(0);
static READ_TOTAL: AtomicU32 = AtomicU32::new(0);
/// Thread IDs alive now (a TID handed out twice shows here).
static LIVE: [AtomicU8; 1024] = [const { AtomicU8::new(0) }; 1024];

fn start_gate() -> u32 {
    let mut h = 0;
    unsafe { DosCreateEventSem(null(), &mut h, 0, 0) };
    GO.store(h, Ordering::Relaxed);
    h
}

fn wait_gate() {
    unsafe { DosWaitEventSem(GO.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT) };
}

fn timed(name: &str, t0: u32) {
    println!("  {name}: {} ms", ms().wrapping_sub(t0));
}

fn race_name(round: u32) -> Vec<u8> {
    let mut v = alloc::format!("\\SEM32\\STRESS\\RACE{round}").into_bytes();
    v.push(0);
    v
}

unsafe extern "C" fn creator(_: u32) {
    wait_gate();
    let name = race_name(ROUND.load(Ordering::Relaxed));
    let mut h = 0;
    match unsafe { DosCreateEventSem(name.as_ptr(), &mut h, 0, 0) } {
        0 => {
            WINS.fetch_add(1, Ordering::Relaxed);
            // Keep it until all have tried, then close.
            unsafe { DosSleep(20) };
            unsafe { DosCloseEventSem(h) };
        }
        285 => {
            DUPS.fetch_add(1, Ordering::Relaxed);
        }
        _ => {
            ODD.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Named semaphore created by several threads at once: one wins.
fn named_create_race() {
    let t0 = ms();
    let mut ok = true;
    for round in 0..20 {
        ROUND.store(round, Ordering::Relaxed);
        WINS.store(0, Ordering::Relaxed);
        DUPS.store(0, Ordering::Relaxed);
        ODD.store(0, Ordering::Relaxed);
        let gate = start_gate();
        let ts: Vec<u32> = (0..6).map(|_| thread(creator, 0)).collect();
        unsafe { DosSleep(5) };
        unsafe { DosPostEventSem(gate) };
        for t in ts {
            join(t);
        }
        unsafe { DosCloseEventSem(gate) };
        let (w, d, o) = (WINS.load(Ordering::Relaxed), DUPS.load(Ordering::Relaxed), ODD.load(Ordering::Relaxed));
        if w != 1 || d != 5 || o != 0 {
            println!("  round {round}: {w} created, {d} duplicates, {o} other");
            ok = false;
        }
    }
    check("smp: a name is created once by racing threads", ok);
    timed("named create race", t0);
}

/// Busy work between reading and writing the counter, so a second owner
/// would overlap.
fn dawdle(n: u32) {
    let mut x = n;
    for _ in 0..50 {
        x = core::hint::black_box(x.wrapping_mul(3).wrapping_add(1));
    }
    core::hint::black_box(x);
}

unsafe extern "C" fn mutex_worker(n: u32) {
    wait_gate();
    let m = MTX.load(Ordering::Relaxed);
    for i in 0..n {
        if unsafe { DosRequestMutexSem(m, SEM_INDEFINITE_WAIT) } != 0 {
            BAD.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let v = COUNTER.load(Ordering::Relaxed);
        dawdle(i);
        COUNTER.store(v + 1, Ordering::Relaxed);
        unsafe { DosReleaseMutexSem(m) };
    }
}

/// Child: add `n` to the counter in named shared memory under the named
/// mutex.
pub fn child_mutex(n: u32) -> u32 {
    unsafe {
        let mut p: *mut u8 = null_mut();
        if DosGetNamedSharedMem(&mut p, b"\\SHAREMEM\\STRESS\\SMPCNT\0".as_ptr(), PAG_READ | PAG_WRITE) != 0 {
            return 1;
        }
        let mut m = 0;
        if DosOpenMutexSem(b"\\SEM32\\STRESS\\SMPMTX\0".as_ptr(), &mut m) != 0 {
            return 2;
        }
        let c = p as *mut u32;
        for i in 0..n {
            DosRequestMutexSem(m, SEM_INDEFINITE_WAIT);
            let v = c.read_volatile();
            dawdle(i);
            c.write_volatile(v + 1);
            DosReleaseMutexSem(m);
        }
        DosCloseMutexSem(m);
        DosFreeMem(p);
    }
    0
}

/// Mutual exclusion holds with the mutex contended on all CPUs, by
/// threads of one process and by several processes.
fn mutex_exclusion() {
    let t0 = ms();
    let n = 3000;
    let mut m = 0;
    unsafe { DosCreateMutexSem(null(), &mut m, 0, 0) };
    MTX.store(m, Ordering::Relaxed);
    COUNTER.store(0, Ordering::Relaxed);
    BAD.store(0, Ordering::Relaxed);
    let gate = start_gate();
    let ts: Vec<u32> = (0..THREADS).map(|_| thread(mutex_worker, n)).collect();
    unsafe { DosPostEventSem(gate) };
    for t in ts {
        join(t);
    }
    unsafe {
        DosCloseEventSem(gate);
        DosCloseMutexSem(m);
    }
    let got = COUNTER.load(Ordering::Relaxed);
    check("smp: mutex excludes threads of a process", got == THREADS * n && BAD.load(Ordering::Relaxed) == 0);
    if got != THREADS * n {
        println!("  counter {got}, expected {}", THREADS * n);
    }
    timed("mutex, threads", t0);

    let t0 = ms();
    let n = 2000;
    unsafe {
        let mut p: *mut u8 = null_mut();
        let rc = DosAllocSharedMem(&mut p, b"\\SHAREMEM\\STRESS\\SMPCNT\0".as_ptr(), 4096, PAG_READ | PAG_WRITE | PAG_COMMIT);
        let mut m = 0;
        let rc2 = DosCreateMutexSem(b"\\SEM32\\STRESS\\SMPMTX\0".as_ptr(), &mut m, 0, 0);
        if rc != 0 || rc2 != 0 {
            check("smp: mutex excludes processes", false);
            return;
        }
        (p as *mut u32).write_volatile(0);
        let arg = alloc::format!("smpmutex {n}");
        let pids: Vec<u32> = (0..THREADS).map(|_| crate::start_child(&arg)).collect();
        let mut ok = true;
        for pid in pids {
            let (rc, cause, code, _) = crate::wait_child(pid);
            ok &= rc == 0 && cause == 0 && code == 0;
        }
        let got = (p as *mut u32).read_volatile();
        check("smp: mutex excludes processes", ok && got == THREADS * n);
        if got != THREADS * n {
            println!("  counter {got}, expected {}", THREADS * n);
        }
        DosCloseMutexSem(m);
        DosFreeMem(p);
    }
    timed("mutex, processes", t0);
}

fn my_tid() -> u32 {
    let (mut tib, mut pib) = (null_mut(), null_mut());
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        (*(*tib).ptib2).tid
    }
}

unsafe extern "C" fn leaf(_: u32) {
    let tid = my_tid() as usize;
    if LIVE[tid % 1024].swap(1, Ordering::AcqRel) != 0 {
        BAD.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { DosSleep(0) };
    LIVE[tid % 1024].store(0, Ordering::Release);
}

unsafe extern "C" fn spawner(n: u32) {
    wait_gate();
    for _ in 0..n {
        let t = thread(leaf, 0);
        if t == 0 {
            BAD.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        join(t);
    }
}

/// Threads created by several threads at once get distinct IDs.
fn thread_ids() {
    let t0 = ms();
    BAD.store(0, Ordering::Relaxed);
    let gate = start_gate();
    let ts: Vec<u32> = (0..THREADS).map(|_| thread(spawner, 150)).collect();
    unsafe { DosPostEventSem(gate) };
    for t in ts {
        join(t);
    }
    unsafe { DosCloseEventSem(gate) };
    check("smp: concurrent DosCreateThread gives distinct IDs", BAD.load(Ordering::Relaxed) == 0);
    timed("thread create storm", t0);
}

unsafe extern "C" fn pipe_reader(_: u32) {
    let r = PIPE_R.load(Ordering::Relaxed);
    let mut buf = [0u8; 700];
    loop {
        let mut n = 0;
        let rc = unsafe { DosRead(r, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 {
            BAD.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if n == 0 {
            return;
        }
        READ_TOTAL.fetch_add(n, Ordering::Relaxed);
    }
}

/// Several readers of one pipe: none sees end of file before the writer
/// closes, and together they get every byte.
fn pipe_readers() {
    let t0 = ms();
    let (mut r, mut w) = (0, 0);
    unsafe { DosCreatePipe(&mut r, &mut w, 4096) };
    PIPE_R.store(r, Ordering::Relaxed);
    READ_TOTAL.store(0, Ordering::Relaxed);
    BAD.store(0, Ordering::Relaxed);
    let ts: Vec<u32> = (0..3).map(|_| thread(pipe_reader, 0)).collect();
    let chunk = [0x5au8; 1000];
    let total = 300 * chunk.len() as u32;
    let mut ok = true;
    for _ in 0..300 {
        let mut n = 0;
        ok &= unsafe { DosWrite(w, chunk.as_ptr(), chunk.len() as u32, &mut n) } == 0 && n == chunk.len() as u32;
    }
    unsafe { DosClose(w) };
    for t in ts {
        join(t);
    }
    unsafe { DosClose(r) };
    let got = READ_TOTAL.load(Ordering::Relaxed);
    check("smp: three readers of one pipe get every byte", ok && got == total && BAD.load(Ordering::Relaxed) == 0);
    if got != total {
        println!("  read {got} of {total}");
    }
    timed("pipe readers", t0);
}

/// A kernel-call mix: memory, semaphores, queries. Returns false on an
/// unexpected result.
fn call_mix(n: u32) -> bool {
    let mut ok = true;
    unsafe {
        let mut ev = 0;
        ok &= DosCreateEventSem(null(), &mut ev, 0, 0) == 0;
        for i in 0..n {
            let mut p: *mut u8 = null_mut();
            if DosAllocMem(&mut p, 0x10000, PAG_READ | PAG_WRITE | PAG_COMMIT) != 0 {
                ok = false;
                continue;
            }
            for k in 0..4 {
                p.add(k * 4096).write_volatile(i as u8);
            }
            ok &= DosFreeMem(p) == 0;
            ok &= DosPostEventSem(ev) == 0;
            let mut c = 0;
            ok &= DosResetEventSem(ev, &mut c) == 0;
            let mut v = 0u32;
            ok &= DosQuerySysInfo(14, 14, &mut v, 4) == 0;
        }
        DosCloseEventSem(ev);
    }
    ok
}

unsafe extern "C" fn mix_worker(n: u32) {
    wait_gate();
    if !call_mix(n) {
        BAD.fetch_add(1, Ordering::Relaxed);
    }
}

/// Child: the kernel-call mix in a process of its own.
pub fn child_mix(n: u32) -> u32 {
    if call_mix(n) { 0 } else { 1 }
}

unsafe extern "C" fn spin_worker(n: u32) {
    wait_gate();
    dawdle_long(n);
}

fn dawdle_long(n: u32) {
    let mut x = n;
    for _ in 0..n {
        x = core::hint::black_box(x.wrapping_mul(1_103_515_245).wrapping_add(12345));
    }
    core::hint::black_box(x);
}

/// User-mode work only: how many CPUs really run at once (QEMU runs
/// several only with multi-threaded TCG or a hypervisor).
fn compute() {
    let n = 20_000_000;
    let t0 = ms();
    dawdle_long(n);
    let one = ms().wrapping_sub(t0);
    let gate = start_gate();
    let ts: Vec<u32> = (0..THREADS).map(|_| thread(spin_worker, n)).collect();
    let t0 = ms();
    unsafe { DosPostEventSem(gate) };
    for t in ts {
        join(t);
    }
    let all = ms().wrapping_sub(t0);
    unsafe { DosCloseEventSem(gate) };
    println!("  compute: 1 thread {one} ms, {THREADS} threads {all} ms");
}

/// Kernel calls from all CPUs at once (also a timing: `STRESS smp`).
fn kernel_calls() {
    let n = 1500;
    BAD.store(0, Ordering::Relaxed);
    let t0 = ms();
    call_mix(n);
    let one = ms().wrapping_sub(t0);
    let gate = start_gate();
    let ts: Vec<u32> = (0..THREADS).map(|_| thread(mix_worker, n)).collect();
    let t0 = ms();
    unsafe { DosPostEventSem(gate) };
    for t in ts {
        join(t);
    }
    let all = ms().wrapping_sub(t0);
    unsafe { DosCloseEventSem(gate) };
    check("smp: kernel-call mix on all CPUs", BAD.load(Ordering::Relaxed) == 0);
    println!("  kernel-call mix: 1 thread {one} ms, {THREADS} threads {all} ms ({n} rounds each)");
    // The same in processes of their own (no shared address space).
    let arg = alloc::format!("smpmix {n}");
    let t0 = ms();
    let pids: Vec<u32> = (0..THREADS).map(|_| crate::start_child(&arg)).collect();
    let mut ok = true;
    for pid in pids {
        let (rc, cause, code, _) = crate::wait_child(pid);
        ok &= rc == 0 && cause == 0 && code == 0;
    }
    let procs = ms().wrapping_sub(t0);
    check("smp: kernel-call mix in several processes", ok);
    println!("  kernel-call mix: {THREADS} processes {procs} ms");
}

const SHARED_FILE: &[u8] = b"C:\\STRESSMP.DAT\0";
const SLICE: u32 = 8192;

unsafe extern "C" fn file_writer(k: u32) {
    wait_gate();
    let (mut h, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(SHARED_FILE.as_ptr(), &mut h, &mut act, 0, FILE_NORMAL, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, null_mut())
    };
    if rc != 0 {
        BAD.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let data = [b'A' + k as u8; 512];
    for round in 0..4 {
        for i in 0..SLICE / 512 {
            let mut pos = 0;
            let mut n = 0;
            unsafe {
                DosSetFilePtr(h, (k * SLICE + i * 512) as i32, FILE_BEGIN, &mut pos);
                if DosWrite(h, data.as_ptr(), 512, &mut n) != 0 || n != 512 {
                    BAD.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        let _ = round;
    }
    unsafe { DosClose(h) };
}

/// Threads with their own handles to one file write their own parts of it
/// at once (the file grows under them): everything lands.
fn shared_file() {
    let t0 = ms();
    BAD.store(0, Ordering::Relaxed);
    unsafe {
        let (mut h, mut act) = (0, 0);
        DosOpen(SHARED_FILE.as_ptr(), &mut h, &mut act, 0, FILE_NORMAL, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, null_mut());
        DosClose(h);
    }
    let gate = start_gate();
    let ts: Vec<u32> = (0..THREADS).map(|k| thread(file_writer, k)).collect();
    unsafe { DosPostEventSem(gate) };
    for t in ts {
        join(t);
    }
    unsafe { DosCloseEventSem(gate) };
    let mut ok = BAD.load(Ordering::Relaxed) == 0;
    unsafe {
        let (mut h, mut act) = (0, 0);
        ok &= DosOpen(SHARED_FILE.as_ptr(), &mut h, &mut act, 0, FILE_NORMAL, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, null_mut()) == 0;
        let mut buf = alloc::vec![0u8; (THREADS * SLICE) as usize + 16];
        let mut n = 0;
        DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n);
        DosClose(h);
        ok &= n == THREADS * SLICE;
        for k in 0..THREADS {
            let part = &buf[(k * SLICE) as usize..((k + 1) * SLICE).min(n) as usize];
            if part.len() != SLICE as usize || part.iter().any(|&b| b != b'A' + k as u8) {
                ok = false;
                println!("  part {k} wrong (file {n} bytes)");
            }
        }
        DosDelete(SHARED_FILE.as_ptr());
    }
    check("smp: one file written by threads with their own handles", ok);
    timed("shared file", t0);
}

unsafe extern "C" fn exec_worker(n: u32) {
    wait_gate();
    for _ in 0..n {
        match spawn("exit 7", EXEC_SYNC) {
            Ok((_, r)) if r.code_terminate == 0 && r.code_result == 7 => {}
            _ => {
                BAD.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Programs started from several threads at once (the loader, process
/// creation and end).
fn exec_storm() {
    let t0 = ms();
    BAD.store(0, Ordering::Relaxed);
    let gate = start_gate();
    let ts: Vec<u32> = (0..THREADS).map(|_| thread(exec_worker, 8)).collect();
    unsafe { DosPostEventSem(gate) };
    for t in ts {
        join(t);
    }
    unsafe { DosCloseEventSem(gate) };
    check("smp: programs started from several threads at once", BAD.load(Ordering::Relaxed) == 0);
    timed("exec storm", t0);
}

/// Child: wait for the start, load TESTDLLN (global initialisation),
/// report how many global initialisations it has seen, hold it until told.
pub fn child_dll() -> u32 {
    unsafe {
        let (mut go, mut hold) = (0, 0);
        if DosOpenEventSem(b"\\SEM32\\STRESS\\DLLGO\0".as_ptr(), &mut go) != 0
            || DosOpenEventSem(b"\\SEM32\\STRESS\\DLLHOLD\0".as_ptr(), &mut hold) != 0
        {
            return 100;
        }
        DosWaitEventSem(go, SEM_INDEFINITE_WAIT);
        let mut h = 0;
        if DosLoadModule(null_mut(), 0, b"TESTDLLN\0".as_ptr(), &mut h) != 0 {
            return 101;
        }
        let mut pf: usize = 0;
        if DosQueryProcAddr(h, 5, null(), &mut pf) != 0 {
            return 102;
        }
        let shared: extern "C" fn() -> u32 = core::mem::transmute(pf);
        let n = shared();
        DosWaitEventSem(hold, SEM_INDEFINITE_WAIT);
        n
    }
}

/// A DLL with a global initialisation loaded by several processes at
/// once: it runs once.
fn dll_init_once() {
    let t0 = ms();
    let mut ok = true;
    for round in 0..3 {
        let (mut go, mut hold) = (0, 0);
        unsafe {
            DosCreateEventSem(b"\\SEM32\\STRESS\\DLLGO\0".as_ptr(), &mut go, 0, 0);
            DosCreateEventSem(b"\\SEM32\\STRESS\\DLLHOLD\0".as_ptr(), &mut hold, 0, 0);
        }
        let pids: Vec<u32> = (0..THREADS).map(|_| crate::start_child("smpdll")).collect();
        unsafe {
            DosSleep(300);
            DosPostEventSem(go);
            DosSleep(1500);
            DosPostEventSem(hold);
        }
        for pid in pids {
            let (rc, cause, code, _) = crate::wait_child(pid);
            if rc != 0 || cause != 0 || code != 1 {
                println!("  round {round}: child rc {rc} cause {cause} saw {code} global inits");
                ok = false;
            }
        }
        unsafe {
            DosCloseEventSem(go);
            DosCloseEventSem(hold);
        }
    }
    check("smp: a DLL's global init runs once for processes loading it at once", ok);
    timed("DLL init race", t0);
}

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    // The global information segment's tiled selector (a constant
    // import: the "address" is the value).
    #[link_name = "DOSCALLS.217"]
    static DosGlobalInfo: u8;
}

/// The global information segment's time fields' sequence count (the
/// kernel's infoseg::SEQ_OFFSET).
const GIS_SEQ: usize = 0x7c;
static TIME_STOP: AtomicU32 = AtomicU32::new(0);
static TIME_READS: AtomicU32 = AtomicU32::new(0);
static TIME_TORN: AtomicU32 = AtomicU32::new(0);
static TIME_BACK: AtomicU32 = AtomicU32::new(0);
static TIME_RETRY: AtomicU32 = AtomicU32::new(0);

fn gis() -> *const u8 {
    (((&raw const DosGlobalInfo) as u32 & 0xfff8) << 13) as *const u8
}

/// Days since 1970-01-01 to (year, month, day).
fn civil(days: u32) -> (u32, u32, u32) {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y as u32, m as u32, d as u32)
}

unsafe extern "C" fn time_reader(_: u32) {
    wait_gate();
    let g = gis();
    let rd = |o: usize| unsafe { (g.add(o) as *const u32).read_volatile() };
    let mut last = 0u32;
    while TIME_STOP.load(Ordering::Relaxed) == 0 {
        // As IBM's DosGetDateTime reads it: the time-of-day dword alone,
        // no lock. It never goes back by half a second or more (but at
        // midnight; setting the clock to itself takes it back by less).
        // Stored byte by byte it did: a new second with the old
        // hundredths (.97), then the new hundredths.
        let tod = rd(8).to_le_bytes();
        let v = tod[0] as u32 * 360_000 + tod[1] as u32 * 6000 + tod[2] as u32 * 100 + tod[3] as u32;
        if v + 50 < last && !(last > 8_500_000 && v < 100_000) {
            TIME_BACK.fetch_add(1, Ordering::Relaxed);
        }
        last = v;
        // All fields between two equal, even sequence counts.
        let s0 = rd(GIS_SEQ);
        core::sync::atomic::fence(Ordering::Acquire);
        let (secs, tod, date, wd) = (rd(0), rd(8).to_le_bytes(), rd(16).to_le_bytes(), unsafe { g.add(20).read_volatile() });
        core::sync::atomic::fence(Ordering::Acquire);
        if s0 & 1 != 0 || rd(GIS_SEQ) != s0 {
            TIME_RETRY.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        TIME_READS.fetch_add(1, Ordering::Relaxed);
        let days = secs / 86_400;
        let (y, m, d) = civil(days);
        let ok = secs % 86_400 == tod[0] as u32 * 3600 + tod[1] as u32 * 60 + tod[2] as u32
            && (y, m, d) == (u16::from_le_bytes([date[2], date[3]]) as u32, date[1] as u32, date[0] as u32)
            && (days + 4) % 7 == wd as u32;
        if !ok {
            TIME_TORN.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Programs read the global information segment's time fields while the
/// timer (and DosSetDateTime on another CPU) rewrites them: a time of
/// day read as one dword never goes back, and a read bracketed by the
/// sequence count sees one consistent time and date.
fn infoseg_time() {
    let t0 = ms();
    for c in [&TIME_STOP, &TIME_READS, &TIME_TORN, &TIME_BACK, &TIME_RETRY] {
        c.store(0, Ordering::Relaxed);
    }
    let gate = start_gate();
    let ts: Vec<u32> = (0..THREADS - 1).map(|_| thread(time_reader, 0)).collect();
    unsafe { DosPostEventSem(gate) };
    // Set the clock to itself now and then (a second writer).
    let mut set_ok = true;
    for _ in 0..12 {
        unsafe { DosSleep(170) };
        let mut dt = DateTime::default();
        unsafe {
            DosGetDateTime(&mut dt);
            set_ok &= DosSetDateTime(&dt) == 0;
        }
    }
    TIME_STOP.store(1, Ordering::Relaxed);
    for t in ts {
        join(t);
    }
    unsafe { DosCloseEventSem(gate) };
    let (reads, torn, back) = (TIME_READS.load(Ordering::Relaxed), TIME_TORN.load(Ordering::Relaxed), TIME_BACK.load(Ordering::Relaxed));
    println!("  infoseg time: {reads} consistent reads ({} retried during an update), {torn} torn, {back} times the time of day went back", TIME_RETRY.load(Ordering::Relaxed));
    check("smp: DosSetDateTime while the infoseg is read", set_ok);
    check("smp: the infoseg's time fields are never torn", reads > 0 && torn == 0 && back == 0);
    timed("infoseg time", t0);
}

pub fn run() {
    let t0 = ms();
    dll_init_once();
    named_create_race();
    mutex_exclusion();
    thread_ids();
    pipe_readers();
    compute();
    kernel_calls();
    shared_file();
    exec_storm();
    infoseg_time();
    println!("  smp section: {} ms", ms().wrapping_sub(t0));
}
