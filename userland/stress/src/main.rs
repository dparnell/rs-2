//! Kernel stress test. `STRESS` runs every section and prints PASS/FAIL
//! per check; the exit code is the number of failures. `STRESS section...`
//! runs only the named sections; `STRESS ctrlc` waits for Ctrl+C in a
//! blocking pipe read. The program also starts copies of itself as child
//! processes (`STRESS -child MODE ...`).

#![no_std]
#![no_main]

extern crate alloc;

mod api;
mod files;
mod ipc;
mod kill;
mod mem;
mod misc;
mod more;
mod sched;
mod procs;
mod sems;
mod signals;
mod threads;
mod xcpt;

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

static FAILS: AtomicU32 = AtomicU32::new(0);
/// Progress for the watchdog: bumped by every check.
static PROGRESS: AtomicU32 = AtomicU32::new(0);
static SECTION: AtomicU32 = AtomicU32::new(0);

pub const SELF: &[u8] = b"C:\\RS2\\STRESS.EXE\0";

pub fn check(name: &str, ok: bool) {
    PROGRESS.fetch_add(1, Ordering::Relaxed);
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

/// Milliseconds since boot.
pub fn ms() -> u32 {
    let mut v = 0u32;
    unsafe { DosQuerySysInfo(14, 14, &mut v, 4) };
    v
}

/// Free physical memory in bytes (QSV_TOTAVAILMEM).
pub fn avail_mem() -> u32 {
    let mut v = 0u32;
    unsafe { DosQuerySysInfo(19, 19, &mut v, 4) };
    v
}

/// A small deterministic random number generator.
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        self.0 >> 8
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// Start a thread.
pub fn thread(f: ThreadFn, param: u32) -> u32 {
    let mut tid = 0;
    let rc = unsafe { DosCreateThread(&mut tid, f, param, 0, 32768) };
    if rc != 0 {
        println!("  DosCreateThread rc {rc}");
        return 0;
    }
    tid
}

pub fn join(tid: u32) -> u32 {
    let mut t = tid;
    unsafe { DosWaitThread(&mut t, DCWW_WAIT) }
}

/// Start a child copy of this program: `STRESS -child <args>`.
pub fn spawn(args: &str, flags: u32) -> Result<(u32, ResultCodes), u32> {
    let mut block: Vec<u8> = Vec::new();
    block.extend_from_slice(b"STRESS\0-child ");
    block.extend_from_slice(args.as_bytes());
    block.extend_from_slice(b"\0\0");
    let mut fail = [0u8; 64];
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), 64, flags, block.as_ptr(), null(), &mut res, SELF.as_ptr()) };
    if rc != 0 {
        println!("  DosExecPgm {args}: rc {rc}");
        return Err(rc);
    }
    Ok((res.code_terminate, res))
}

/// Run a child synchronously; its (termination cause, exit code).
pub fn run_child(args: &str) -> (u32, u32) {
    match spawn(args, EXEC_SYNC) {
        Ok((_, r)) => (r.code_terminate, r.code_result),
        Err(e) => (0xffff, e),
    }
}

/// Start a child asynchronously (result kept); its PID.
pub fn start_child(args: &str) -> u32 {
    match spawn(args, EXEC_ASYNCRESULT) {
        Ok((pid, _)) => pid,
        Err(_) => 0,
    }
}

/// Wait for a child; (rc, cause, code, pid).
pub fn wait_child(pid: u32) -> (u32, u32, u32, u32) {
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut got = 0;
    let rc = unsafe { DosWaitChild(DCWA_PROCESS, DCWW_WAIT, &mut res, &mut got, pid) };
    (rc, res.code_terminate, res.code_result, got)
}

/// Kernel object counters (RS2API Rs2QueryCounters).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counters {
    pub free_pages: u32,
    pub threads: u32,
    pub processes: u32,
    pub sems32: u32,
    pub sems16: u32,
    pub shared_mem: u32,
    pub open_files: u32,
    pub queues: u32,
    pub timers: u32,
    pub heap_used: u32,
    pub process_objects: u32,
    pub file_locks: u32,
}

pub fn counters() -> Counters {
    let mut v = [0u32; 12];
    let rc = unsafe { api::Rs2QueryCounters(v.as_mut_ptr(), 48) };
    if rc != 0 {
        return Counters::default();
    }
    Counters {
        free_pages: v[0],
        threads: v[1],
        processes: v[2],
        sems32: v[3],
        sems16: v[4],
        shared_mem: v[5],
        open_files: v[6],
        queues: v[7],
        timers: v[8],
        heap_used: v[9],
        process_objects: v[10],
        file_locks: v[11],
    }
}

/// Wait until no other process of ours is alive (children reaped).
pub fn settle() {
    unsafe { DosSleep(100) };
}

unsafe extern "C" fn watchdog(_: u32) {
    let mut last = PROGRESS.load(Ordering::Relaxed);
    let mut still = 0;
    loop {
        unsafe { DosSleep(1000) };
        let now = PROGRESS.load(Ordering::Relaxed);
        if now == last {
            still += 1;
            if still == 30 {
                println!("WATCHDOG: no progress for 30 s in section {} after check {}", SECTION.load(Ordering::Relaxed), now);
            }
        } else {
            still = 0;
            last = now;
        }
    }
}

const SECTIONS: &[(&str, fn())] = &[
    ("threads", threads::run),
    ("sems", sems::run),
    ("kill", kill::run),
    ("signals", signals::run),
    ("xcpt", xcpt::run),
    ("procs", procs::run),
    ("mem", mem::run),
    ("files", files::run),
    ("ipc", ipc::run),
    ("misc", misc::run),
    ("more", more::run),
    ("sched", sched::run),
];

fn words(s: &[u8]) -> Vec<String> {
    core::str::from_utf8(s).unwrap_or("").split_ascii_whitespace().map(String::from).collect()
}

fn main(s: &Startup) -> u32 {
    let args = words(s.args());
    if args.first().map(String::as_str) == Some("-child") {
        return procs::child(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("ctrlc") {
        return signals::ctrlc();
    }
    println!("rs2 stress test");
    thread(watchdog, 0);
    let before = counters();
    let mem0 = avail_mem();
    for (i, (name, f)) in SECTIONS.iter().enumerate() {
        if !args.is_empty() && !args.iter().any(|a| a.eq_ignore_ascii_case(name)) {
            continue;
        }
        SECTION.store(i as u32, Ordering::Relaxed);
        println!("== {name}");
        f();
        settle();
        let k = counters();
        println!(
            "   after {name}: {} free pages, heap {} KB, {} threads, {} processes ({} objects)",
            k.free_pages, k.heap_used, k.threads, k.processes, k.process_objects
        );
    }
    settle();
    let after = counters();
    println!("counters before {:?}", before);
    println!("counters after  {:?}", after);
    if args.is_empty() {
        check("no leaked processes", after.processes == before.processes && after.process_objects == before.process_objects);
        check("no leaked file locks", after.file_locks == before.file_locks);
        // The watchdog thread is still running.
        check("no leaked threads", after.threads == before.threads);
        check("no leaked semaphores", after.sems32 == before.sems32 && after.sems16 == before.sems16);
        check("no leaked shared memory", after.shared_mem == before.shared_mem);
        check("no leaked open files", after.open_files == before.open_files);
        check("no leaked queues or timers", after.queues == before.queues && after.timers == before.timers);
    }
    let mem1 = avail_mem();
    println!("free memory {} KB before, {} KB after", mem0 / 1024, mem1 / 1024);
    let _ = (null_mut::<u8>(), mem1);
    let f = FAILS.load(Ordering::Relaxed);
    println!("stress done: {f} failure(s)");
    f
}
