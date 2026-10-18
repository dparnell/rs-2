//! Semaphores: timeouts, owner death (with and without a waiter), muxwait
//! ANY/ALL, many posters, named semaphores across processes, close while
//! in use, 16-bit-style races between posts and timed waits.

use core::ptr::null;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, join, ms, run_child, thread};

static MTX: AtomicU32 = AtomicU32::new(0);
static EV: AtomicU32 = AtomicU32::new(0);
static EV2: AtomicU32 = AtomicU32::new(0);
static RESULT: AtomicU32 = AtomicU32::new(0);
static STAMP: AtomicU32 = AtomicU32::new(0);

/// Take the mutex and end without releasing it.
unsafe extern "C" fn take_and_die(hold_ms: u32) {
    unsafe {
        DosRequestMutexSem(MTX.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT);
        EV.load(Ordering::Relaxed);
        DosPostEventSem(EV2.load(Ordering::Relaxed));
        DosSleep(hold_ms);
    }
}

/// Take the mutex, then block forever.
unsafe extern "C" fn take_and_block(_: u32) {
    unsafe {
        DosRequestMutexSem(MTX.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT);
        DosPostEventSem(EV2.load(Ordering::Relaxed));
        DosWaitEventSem(EV.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT);
    }
}

/// Wait for the mutex; store the result.
unsafe extern "C" fn mutex_waiter(_: u32) {
    let rc = unsafe { DosRequestMutexSem(MTX.load(Ordering::Relaxed), SEM_INDEFINITE_WAIT) };
    RESULT.store(rc, Ordering::Relaxed);
    STAMP.store(ms(), Ordering::Relaxed);
    if rc == 0 || rc == ERROR_SEM_OWNER_DIED {
        unsafe { DosReleaseMutexSem(MTX.load(Ordering::Relaxed)) };
    }
}

unsafe extern "C" fn poster(n: u32) {
    for _ in 0..n {
        unsafe { DosPostEventSem(EV.load(Ordering::Relaxed)) };
    }
}

unsafe extern "C" fn post_after(delay: u32) {
    unsafe {
        DosSleep(delay);
        DosPostEventSem(EV.load(Ordering::Relaxed));
    }
}

fn new_event() -> u32 {
    let mut h = 0;
    unsafe { DosCreateEventSem(null(), &mut h, 0, 0) };
    h
}

fn new_mutex() -> u32 {
    let mut h = 0;
    unsafe { DosCreateMutexSem(null(), &mut h, 0, 0) };
    h
}

pub fn run() {
    unsafe {
        // Timeouts.
        let ev = new_event();
        let t0 = ms();
        let rc = DosWaitEventSem(ev, 100);
        let el = ms() - t0;
        check("event wait times out", rc == ERROR_TIMEOUT && (90..400).contains(&el));
        let mtx = new_mutex();
        MTX.store(mtx, Ordering::Relaxed);
        EV.store(ev, Ordering::Relaxed);
        let ev2 = new_event();
        EV2.store(ev2, Ordering::Relaxed);
        let t = thread(take_and_block, 0);
        DosWaitEventSem(ev2, 2000);
        let t0 = ms();
        let rc = DosRequestMutexSem(mtx, 60);
        let el = ms() - t0;
        check("mutex request times out", rc == ERROR_TIMEOUT && (50..400).contains(&el));
        check("mutex immediate request fails", DosRequestMutexSem(mtx, 0) == ERROR_TIMEOUT);
        let (mut pid, mut tid, mut cnt) = (0, 0, 0);
        DosQueryMutexSem(mtx, &mut pid, &mut tid, &mut cnt);
        check("DosQueryMutexSem owner", tid == t && cnt == 1);
        check("close a mutex another thread owns: busy", DosCloseMutexSem(mtx) == ERROR_SEM_BUSY);

        // Owner killed while another thread waits: the waiter gets
        // ERROR_SEM_OWNER_DIED promptly.
        RESULT.store(0xffff, Ordering::Relaxed);
        let w = thread(mutex_waiter, 0);
        DosSleep(50);
        check("waiter blocked", RESULT.load(Ordering::Relaxed) == 0xffff);
        let k0 = ms();
        check("DosKillThread owner", DosKillThread(t) == 0);
        join(t);
        let waited = join(w);
        let r = RESULT.load(Ordering::Relaxed);
        let late = STAMP.load(Ordering::Relaxed).wrapping_sub(k0);
        check("waiter woken with ERROR_SEM_OWNER_DIED", waited == 0 && r == ERROR_SEM_OWNER_DIED && late < 500);
        DosCloseMutexSem(mtx);

        // Owner exits normally holding the mutex, nobody waiting.
        let mtx = new_mutex();
        MTX.store(mtx, Ordering::Relaxed);
        DosResetEventSem(ev2, &mut cnt);
        let t = thread(take_and_die, 0);
        join(t);
        check("request after owner ended: OWNER_DIED", DosRequestMutexSem(mtx, 1000) == ERROR_SEM_OWNER_DIED);
        check("now owned by us", DosReleaseMutexSem(mtx) == 0);
        check("then a plain request works", DosRequestMutexSem(mtx, 0) == 0 && DosReleaseMutexSem(mtx) == 0);

        // Owner exits while a waiter is blocked (no kill).
        DosResetEventSem(ev2, &mut cnt);
        let t = thread(take_and_die, 150);
        DosWaitEventSem(ev2, 2000);
        RESULT.store(0xffff, Ordering::Relaxed);
        let w = thread(mutex_waiter, 0);
        join(t);
        join(w);
        check("waiter woken when owner exits", RESULT.load(Ordering::Relaxed) == ERROR_SEM_OWNER_DIED);

        // Recursive requests and releases.
        let mut ok = true;
        for _ in 0..100 {
            ok &= DosRequestMutexSem(mtx, 0) == 0;
        }
        for _ in 0..100 {
            ok &= DosReleaseMutexSem(mtx) == 0;
        }
        check("100 nested requests", ok && DosReleaseMutexSem(mtx) == 288);
        DosCloseMutexSem(mtx);

        // Many posters.
        DosResetEventSem(ev, &mut cnt);
        let mut ts = [0u32; 16];
        for t in ts.iter_mut() {
            *t = thread(poster, 100);
        }
        for t in ts {
            join(t);
        }
        let mut posts = 0;
        DosQueryEventSem(ev, &mut posts);
        check("16 threads x 100 posts counted", posts == 1600);
        DosResetEventSem(ev, &mut cnt);

        // Race posts against timed waits.
        let mut ok = true;
        for i in 0..100 {
            let t = thread(post_after, i % 3);
            let rc = DosWaitEventSem(ev, 2000);
            ok &= rc == 0;
            DosResetEventSem(ev, &mut cnt);
            join(t);
        }
        check("100 post/wait races", ok);

        // Muxwait ANY and ALL.
        let evs = [new_event(), new_event(), new_event()];
        let recs: [SemRecord; 3] = core::array::from_fn(|i| SemRecord { hsem: evs[i], user: 10 + i as u32 });
        let mut any = 0;
        DosCreateMuxWaitSem(null(), &mut any, 3, recs.as_ptr(), DCMW_WAIT_ANY);
        let mut user = 0;
        check("muxwait ANY times out", DosWaitMuxWaitSem(any, 50, &mut user) == ERROR_TIMEOUT);
        EV.store(evs[1], Ordering::Relaxed);
        let t = thread(post_after, 30);
        check("muxwait ANY woken by member", DosWaitMuxWaitSem(any, 2000, &mut user) == 0 && user == 11);
        join(t);
        let mut all = 0;
        DosCreateMuxWaitSem(null(), &mut all, 3, recs.as_ptr(), DCMW_WAIT_ALL);
        check("muxwait ALL times out with one posted", DosWaitMuxWaitSem(all, 50, &mut user) == ERROR_TIMEOUT);
        DosPostEventSem(evs[0]);
        DosPostEventSem(evs[2]);
        check("muxwait ALL with all posted", DosWaitMuxWaitSem(all, 1000, &mut user) == 0);
        let m1 = new_mutex();
        let mrec = SemRecord { hsem: m1, user: 99 };
        let mut mmux = 0;
        DosCreateMuxWaitSem(null(), &mut mmux, 1, &mrec, DCMW_WAIT_ALL);
        check("muxwait on a mutex takes it", DosWaitMuxWaitSem(mmux, 100, &mut user) == 0);
        let (mut p, mut tt, mut n) = (0, 0, 0);
        DosQueryMutexSem(m1, &mut p, &mut tt, &mut n);
        check("mutex owned after muxwait", tt == 1 && n == 1);
        DosReleaseMutexSem(m1);
        check("adding a duplicate fails", DosAddMuxWaitSem(any, &recs[0]) == 284);
        check("delete from muxwait", DosDeleteMuxWaitSem(any, evs[2]) == 0);
        check("close muxwaits", DosCloseMuxWaitSem(any) == 0 && DosCloseMuxWaitSem(all) == 0 && DosCloseMuxWaitSem(mmux) == 0);
        for e in evs {
            DosCloseEventSem(e);
        }
        DosCloseMutexSem(m1);

        // Closing an event another thread waits on.
        EV.store(ev, Ordering::Relaxed);
        DosResetEventSem(ev, &mut cnt);
        let t = thread(post_after, 200);
        DosSleep(0);
        let w = thread(wait_ev, 0);
        DosSleep(50);
        check("close event with a waiter: ERROR_SEM_BUSY", DosCloseEventSem(ev) == ERROR_SEM_BUSY);
        join(t);
        join(w);
        check("close event", DosCloseEventSem(ev) == 0);
        check("close twice fails", DosCloseEventSem(ev) == ERROR_INVALID_HANDLE);
        DosCloseEventSem(ev2);

        // Named semaphores across processes.
        let mut h = 0;
        let rc = DosCreateEventSem(b"\\SEM32\\STRESS\\EV\0".as_ptr(), &mut h, 0, 0);
        check("named event", rc == 0);
        let mut h2 = 0;
        check("duplicate name fails", DosCreateEventSem(b"\\SEM32\\STRESS\\EV\0".as_ptr(), &mut h2, 0, 0) == 285);
        check("open missing name fails", DosOpenEventSem(b"\\SEM32\\STRESS\\NONE\0".as_ptr(), &mut h2) == 187);
        check("bad name fails", DosCreateEventSem(b"\\SEMX\\X\0".as_ptr(), &mut h2, 0, 0) == 123);
        let (cause, code) = run_child("postsem");
        let mut posts = 0;
        DosQueryEventSem(h, &mut posts);
        check("child opened and posted the named event", cause == 0 && code == 0 && posts == 3);
        check("open by handle", { h2 = h; DosOpenEventSem(null(), &mut h2) == 0 && h2 == h });
        DosCloseEventSem(h2);
        DosCloseEventSem(h);
        check("name released after close", DosOpenEventSem(b"\\SEM32\\STRESS\\EV\0".as_ptr(), &mut h2) == 187);

        // A child that owns a shared mutex and dies.
        let mut hm = 0;
        DosCreateMutexSem(b"\\SEM32\\STRESS\\MTX\0".as_ptr(), &mut hm, 0, 0);
        let (_, code) = run_child("holdmutex");
        check("child took the named mutex", code == 0);
        check("owner process died: OWNER_DIED", DosRequestMutexSem(hm, 1000) == ERROR_SEM_OWNER_DIED);
        DosReleaseMutexSem(hm);
        DosCloseMutexSem(hm);
    }
}

unsafe extern "C" fn wait_ev(_: u32) {
    unsafe { DosWaitEventSem(EV.load(Ordering::Relaxed), 5000) };
}

/// Child: open the named event and post it three times.
pub fn child_postsem() -> u32 {
    unsafe {
        let mut h = 0;
        if DosOpenEventSem(b"\\SEM32\\STRESS\\EV\0".as_ptr(), &mut h) != 0 {
            return 1;
        }
        DosPostEventSem(h);
        DosPostEventSem(h);
        let rc = DosPostEventSem(h);
        DosCloseEventSem(h);
        if rc == 299 { 0 } else { 2 }
    }
}

/// Child: take the named mutex and exit holding it.
pub fn child_holdmutex() -> u32 {
    unsafe {
        let mut h = 0;
        if DosOpenMutexSem(b"\\SEM32\\STRESS\\MTX\0".as_ptr(), &mut h) != 0 {
            return 1;
        }
        if DosRequestMutexSem(h, 1000) != 0 {
            return 2;
        }
        0
    }
}
