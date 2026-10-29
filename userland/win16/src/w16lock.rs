//! The Win16Lock: WIN16's state belongs to one thread at a time.
//!
//! A task is a process with one thread, except while a debugger's
//! interrupt callback handles another task's stop (toolhelp.rs): that
//! callback runs on a thread of its own, standing for the stopped task,
//! as Windows runs it in that task. The two threads take turns as Windows
//! tasks do: the one running holds the lock, and hands it over only while
//! it waits for messages (GetMessage, WaitMessage, a PeekMessage that
//! finds none, Yield).

use core::sync::atomic::{AtomicU32, Ordering};

/// The mutex (0 until the task starts).
static LOCK: AtomicU32 = AtomicU32::new(0);
/// Threads besides the task's own running WIN16 code.
static OTHERS: AtomicU32 = AtomicU32::new(0);

/// InitTask: the task's thread takes the lock.
pub fn init() {
    if LOCK.load(Ordering::Relaxed) != 0 {
        return;
    }
    let mut h = 0u32;
    if unsafe { os2::DosCreateMutexSem(core::ptr::null(), &mut h, 0, 1) } == 0 {
        LOCK.store(h, Ordering::Relaxed);
    }
}

pub fn acquire() {
    let h = LOCK.load(Ordering::Relaxed);
    if h != 0 {
        unsafe { os2::DosRequestMutexSem(h, u32::MAX) };
    }
}

pub fn release() {
    let h = LOCK.load(Ordering::Relaxed);
    if h != 0 {
        unsafe { os2::DosReleaseMutexSem(h) };
    }
}

/// Is a second thread running WIN16 code?
pub fn shared() -> bool {
    OTHERS.load(Ordering::Relaxed) != 0
}

/// A thread of its own starts or ends running WIN16 code.
pub fn other_started() {
    OTHERS.fetch_add(1, Ordering::Relaxed);
}

pub fn other_ended() {
    OTHERS.fetch_sub(1, Ordering::Relaxed);
}

/// `f` waits (for messages): the other thread may run meanwhile.
pub fn waiting<T>(f: impl FnOnce() -> T) -> T {
    if !shared() {
        return f();
    }
    release();
    let r = f();
    acquire();
    r
}

/// Yield: the other thread runs if it can.
pub fn yield_now() {
    if shared() {
        release();
        unsafe { os2::DosSleep(1) };
        acquire();
    }
}
