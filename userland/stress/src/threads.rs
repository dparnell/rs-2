//! Many threads: creation storms (thread IDs must be reused), contention
//! on a mutex, DosWaitThread for any thread, suspend/resume, priorities.

use core::ptr::null;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, join, thread};

static DONE: AtomicU32 = AtomicU32::new(0);
static COUNTER: AtomicU32 = AtomicU32::new(0);
static MTX: AtomicU32 = AtomicU32::new(0);
static SPIN: AtomicU32 = AtomicU32::new(0);
static STOP: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn quick(n: u32) {
    DONE.fetch_add(n & 1, Ordering::Relaxed);
}

unsafe extern "C" fn contend(rounds: u32) {
    let h = MTX.load(Ordering::Relaxed);
    for i in 0..rounds {
        unsafe {
            if DosRequestMutexSem(h, SEM_INDEFINITE_WAIT) != 0 {
                return;
            }
            // Non-atomic read-modify-write under the mutex.
            let v = COUNTER.load(Ordering::Relaxed);
            if i % 7 == 0 {
                DosSleep(0);
            }
            COUNTER.store(v + 1, Ordering::Relaxed);
            DosReleaseMutexSem(h);
        }
    }
}

unsafe extern "C" fn spinner(_: u32) {
    while STOP.load(Ordering::Relaxed) == 0 {
        SPIN.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe extern "C" fn sleeper(ms: u32) {
    unsafe { DosSleep(ms) };
}

pub fn run() {
    // Sequential creation: more threads in total than a process has thread
    // slots, so IDs must be reused.
    DONE.store(0, Ordering::Relaxed);
    let mut ok = true;
    let mut max_tid = 0;
    for i in 0..1500 {
        let t = thread(quick, i);
        if t == 0 || join(t) != 0 {
            println!("  thread {i}: create/wait failed (tid {t})");
            ok = false;
            break;
        }
        max_tid = max_tid.max(t);
    }
    check("1500 threads created and waited for", ok && DONE.load(Ordering::Relaxed) == 750);
    check("thread IDs reused", max_tid < 64);

    // Contention: 48 threads on one mutex.
    unsafe {
        let mut h = 0;
        DosCreateMutexSem(null(), &mut h, 0, 0);
        MTX.store(h, Ordering::Relaxed);
        COUNTER.store(0, Ordering::Relaxed);
        let mut tids = [0u32; 48];
        for t in tids.iter_mut() {
            *t = thread(contend, 200);
        }
        let all = tids.iter().all(|&t| t != 0);
        let waited = tids.iter().all(|&t| join(t) == 0);
        check("48 threads contending on a mutex", all && waited && COUNTER.load(Ordering::Relaxed) == 48 * 200);
        DosCloseMutexSem(h);
    }

    // DosWaitThread(0): any thread.
    unsafe {
        let a = thread(sleeper, 30);
        let b = thread(sleeper, 80);
        let mut any = 0;
        let rc1 = DosWaitThread(&mut any, DCWW_WAIT);
        let first = any;
        any = 0;
        let rc2 = DosWaitThread(&mut any, DCWW_WAIT);
        check("DosWaitThread any", rc1 == 0 && rc2 == 0 && first == a && any == b);
        let mut none = 0;
        check("DosWaitThread any, none ended", DosWaitThread(&mut none, DCWW_NOWAIT) == 294);
        let mut me = 1;
        check("DosWaitThread self fails", DosWaitThread(&mut me, DCWW_WAIT) == 309);
    }

    // Suspend and resume a busy thread.
    unsafe {
        STOP.store(0, Ordering::Relaxed);
        let t = thread(spinner, 0);
        DosSleep(50);
        let rc = DosSuspendThread(t);
        DosSleep(20);
        let a = SPIN.load(Ordering::Relaxed);
        DosSleep(100);
        let b = SPIN.load(Ordering::Relaxed);
        check("suspended thread does not run", rc == 0 && a == b);
        check("DosResumeThread", DosResumeThread(t) == 0);
        DosSleep(50);
        check("resumed thread runs", SPIN.load(Ordering::Relaxed) != b);
        check("resume a running thread fails", DosResumeThread(t) == 90);
        STOP.store(1, Ordering::Relaxed);
        check("spinner ended", join(t) == 0);
        check("suspend a dead thread fails", DosSuspendThread(t) == 309);
    }

    // A time-critical thread runs before a regular one.
    unsafe {
        STOP.store(0, Ordering::Relaxed);
        let t = thread(spinner, 0);
        let rc = DosSetPriority(2, 1, 0, t); // PRTYS_THREAD, idle class
        DosSleep(30);
        STOP.store(1, Ordering::Relaxed);
        check("DosSetPriority idle class", rc == 0 && join(t) == 0);
    }
}
