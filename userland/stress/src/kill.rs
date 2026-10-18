//! DosKillThread and DosKillProcess while the victim is blocked in the
//! kernel: event, mutex, muxwait, sleep, pipe read, queue read, thread
//! wait, critical section.

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, join, ms, start_child, thread, wait_child};

static H: AtomicU32 = AtomicU32::new(0);
static RC: AtomicU32 = AtomicU32::new(0xffff);

unsafe extern "C" fn block_event(_: u32) {
    let rc = unsafe { DosWaitEventSem(H.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT) };
    RC.store(rc, Ordering::Relaxed);
}

unsafe extern "C" fn block_mutex(_: u32) {
    let rc = unsafe { DosRequestMutexSem(H.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT) };
    RC.store(rc, Ordering::Relaxed);
}

unsafe extern "C" fn block_mux(_: u32) {
    let mut u = 0;
    let rc = unsafe { DosWaitMuxWaitSem(H.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT, &mut u) };
    RC.store(rc, Ordering::Relaxed);
}

unsafe extern "C" fn block_sleep(_: u32) {
    unsafe { DosSleep(10_000_000) };
    RC.store(0, Ordering::Relaxed);
}

unsafe extern "C" fn block_pipe(_: u32) {
    let mut buf = [0u8; 16];
    let mut n = 0;
    let rc = unsafe { DosRead(H.load(Ordering::Relaxed), buf.as_mut_ptr(), 16, &mut n) };
    RC.store(rc, Ordering::Relaxed);
}

unsafe extern "C" fn block_queue(_: u32) {
    let mut req = RequestData::default();
    let (mut cb, mut pb, mut prio) = (0, null(), 0u8);
    let rc = unsafe { DosReadQueue(H.load(Ordering::Relaxed), &mut req, &mut cb, &mut pb, 0, 0, &mut prio, 0) };
    RC.store(rc, Ordering::Relaxed);
}

unsafe extern "C" fn block_join(other: u32) {
    let rc = crate::join(other);
    RC.store(rc, Ordering::Relaxed);
}

unsafe extern "C" fn spin(_: u32) {
    loop {
        core::hint::spin_loop();
    }
}

unsafe extern "C" fn crit_then_spin(_: u32) {
    unsafe { DosEnterCritSec() };
    // Others are frozen until we leave or die.
    let t0 = ms();
    while ms().wrapping_sub(t0) < 100 {}
    unsafe { DosExitCritSec() };
    loop {
        unsafe { DosSleep(10) };
    }
}

/// Kill a thread blocked in `f` and check it goes away quickly without
/// having returned from its call normally.
fn kill_blocked(name: &str, f: ThreadFn) {
    RC.store(0xffff, Ordering::Relaxed);
    let t = thread(f, 0);
    unsafe { DosSleep(30) };
    let t0 = ms();
    let rc = unsafe { DosKillThread(t) };
    let jrc = join(t);
    let el = ms().wrapping_sub(t0);
    let r = RC.load(Ordering::Relaxed);
    if !(rc == 0 && jrc == 0 && el < 1000 && r == 0xffff) {
        println!("  kill {name}: rc {rc} wait {jrc} after {el} ms, call returned {r}");
    }
    check(&alloc::format!("kill thread blocked in {name}"), rc == 0 && jrc == 0 && el < 1000 && r == 0xffff);
}

pub fn run() {
    unsafe {
        let mut ev = 0;
        DosCreateEventSem(null(), &mut ev, 0, 0);
        H.store(ev, Ordering::Relaxed);
        kill_blocked("DosWaitEventSem", block_event);

        let mut mtx = 0;
        DosCreateMutexSem(null(), &mut mtx, 0, 1);
        H.store(mtx, Ordering::Relaxed);
        kill_blocked("DosRequestMutexSem", block_mutex);
        DosReleaseMutexSem(mtx);

        let rec = SemRecord { hsem: ev, user: 1 };
        let mut mux = 0;
        DosCreateMuxWaitSem(null(), &mut mux, 1, &rec, DCMW_WAIT_ANY);
        H.store(mux, Ordering::Relaxed);
        kill_blocked("DosWaitMuxWaitSem", block_mux);
        DosCloseMuxWaitSem(mux);

        kill_blocked("DosSleep", block_sleep);

        let (mut r, mut w) = (0, 0);
        DosCreatePipe(&mut r, &mut w, 512);
        H.store(r, Ordering::Relaxed);
        kill_blocked("DosRead of a pipe", block_pipe);
        // The pipe still works after the reader was killed.
        let mut n = 0;
        let mut buf = [0u8; 4];
        DosWrite(w, b"ok".as_ptr(), 2, &mut n);
        DosRead(r, buf.as_mut_ptr(), 4, &mut n);
        check("pipe usable after killed reader", n == 2 && &buf[..2] == b"ok");
        DosClose(r);
        DosClose(w);

        let mut q = 0;
        DosCreateQueue(&mut q, QUE_FIFO, b"\\QUEUES\\STRESS\\KILL\0".as_ptr());
        H.store(q, Ordering::Relaxed);
        kill_blocked("DosReadQueue", block_queue);
        DosCloseQueue(q);

        // A thread waiting for another thread.
        let s = thread(spin, 0);
        RC.store(0xffff, Ordering::Relaxed);
        let j = thread(block_join, s);
        DosSleep(30);
        check("kill thread blocked in DosWaitThread", DosKillThread(j) == 0 && join(j) == 0 && RC.load(Ordering::Relaxed) == 0xffff);
        // A thread running user code (preempted, not blocked).
        let t0 = ms();
        check("kill a spinning thread", DosKillThread(s) == 0 && join(s) == 0 && ms() - t0 < 1000);

        // Killing a thread that owns the critical section releases it.
        let c = thread(crit_then_spin, 0);
        DosSleep(200);
        check("kill thread after its critical section", DosKillThread(c) == 0 && join(c) == 0);

        // Bad requests.
        check("kill self fails", DosKillThread(1) == 309);
        check("kill unknown thread fails", DosKillThread(999) == 309);

        // Killing child processes blocked in the kernel.
        for mode in ["block event", "block pipe", "block sleep", "block queue", "block threads"] {
            let pid = start_child(mode);
            DosSleep(200);
            let t0 = ms();
            let rc = DosKillProcess(0, pid);
            let (wrc, cause, _code, got) = wait_child(pid);
            let el = ms() - t0;
            check(
                &alloc::format!("DosKillProcess child ({mode})"),
                pid != 0 && rc == 0 && wrc == 0 && got == pid && cause == 3 && el < 2000,
            );
        }
        // A whole tree (DKP_PROCESSTREE).
        let pid = start_child("tree-block 2");
        DosSleep(600);
        let before = crate::counters().processes;
        let rc = DosKillProcess(1, pid);
        let (wrc, cause, _, _) = wait_child(pid);
        DosSleep(300);
        let after = crate::counters().processes;
        check("DosKillProcess process tree", rc == 0 && wrc == 0 && cause == 3 && after + 7 == before);
        check("kill unknown process fails", DosKillProcess(0, 9999) == 303);
        let _ = null_mut::<u8>();
        DosCloseEventSem(ev);
        DosCloseMutexSem(mtx);
    }
}
