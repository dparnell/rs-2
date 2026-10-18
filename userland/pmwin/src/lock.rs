//! The PMWIN lock: one thread of a process at a time runs PMWIN code.
//!
//! Every export takes it (the generated trampolines call `enter` and
//! `leave`); it is recursive, so window procedures called from inside
//! PMWIN may call back in. A thread gives it up completely while it waits
//! for messages or semaphores (`unlocked`).

use core::sync::atomic::{AtomicU32, Ordering};

use crate::state::tid;

unsafe extern "C" {
    #[link_name = "DOSCALLS.331"]
    fn DosCreateMutexSem(name: *const u8, phmtx: *mut u32, attr: u32, owned: u32) -> u32;
    #[link_name = "DOSCALLS.334"]
    fn DosRequestMutexSem(hmtx: u32, timeout: u32) -> u32;
    #[link_name = "DOSCALLS.335"]
    fn DosReleaseMutexSem(hmtx: u32) -> u32;
    #[link_name = "DOSCALLS.229"]
    fn DosSleep(ms: u32) -> u32;
}

const CREATING: u32 = u32::MAX;

static MUTEX: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);

/// The mutex semaphore, created by the first caller.
fn mutex() -> u32 {
    loop {
        match MUTEX.load(Ordering::Acquire) {
            0 => {
                if MUTEX.compare_exchange(0, CREATING, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    let mut h = 0;
                    unsafe { DosCreateMutexSem(core::ptr::null(), &mut h, 0, 0) };
                    MUTEX.store(h, Ordering::Release);
                    return h;
                }
            }
            CREATING => unsafe {
                DosSleep(0);
            },
            h => return h,
        }
    }
}

fn acquire(t: u32, depth: u32) {
    unsafe { DosRequestMutexSem(mutex(), u32::MAX) };
    OWNER.store(t, Ordering::Relaxed);
    DEPTH.store(depth, Ordering::Relaxed);
}

pub extern "C" fn enter() {
    let t = tid();
    if OWNER.load(Ordering::Relaxed) == t {
        DEPTH.fetch_add(1, Ordering::Relaxed);
        return;
    }
    acquire(t, 1);
}

pub extern "C" fn leave() {
    if OWNER.load(Ordering::Relaxed) != tid() {
        return;
    }
    if DEPTH.fetch_sub(1, Ordering::Relaxed) == 1 {
        OWNER.store(0, Ordering::Relaxed);
        unsafe { DosReleaseMutexSem(mutex()) };
    }
}

/// Run `f` (a wait) with the lock released.
pub fn unlocked<R>(f: impl FnOnce() -> R) -> R {
    let t = tid();
    if OWNER.load(Ordering::Relaxed) != t {
        return f();
    }
    let depth = DEPTH.swap(0, Ordering::Relaxed);
    OWNER.store(0, Ordering::Relaxed);
    unsafe { DosReleaseMutexSem(mutex()) };
    let r = f();
    acquire(t, depth);
    r
}

/// Sleep without the lock.
pub fn sleep(ms: u32) {
    unlocked(|| unsafe { DosSleep(ms) });
}

/// An entry point for a built-in window procedure `$f`: applications may
/// call class procedures directly (from WinQueryClassInfo or after
/// subclassing) with a 4-byte aligned stack and without the lock.
macro_rules! wndproc {
    ($($entry:ident => $f:path;)*) => {$(
        #[unsafe(naked)]
        pub extern "C" fn $entry() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "call {enter}",
                "sub esp, 16",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 8]", "lea edi, [esp + 12]", "mov ecx, 4", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "add esp, 16",
                "sub esp, 16", "mov [esp], eax",
                "call {leave}",
                "mov eax, [esp]",
                "mov esp, ebp", "pop ebp", "ret",
                f = sym $f, enter = sym crate::lock::enter, leave = sym crate::lock::leave,
            );
        }
    )*};
}

pub(crate) use wndproc;
