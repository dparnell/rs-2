//! The PM lock: one thread in the whole system at a time runs PMWIN code
//! on the shared state.
//!
//! It is a named mutex semaphore every PM process opens. Every export
//! takes it (the generated trampolines call `enter` and `leave`); it is
//! recursive per thread. It is never held while application code runs:
//! window procedures are called through `unlocked`, and a thread gives it
//! up completely while it waits for messages or semaphores. Holding it,
//! a thread is in a must-complete section (DosEnterMustComplete), so a
//! process being killed finishes its update of the shared state first;
//! if a holder dies anyway (an exception in PMWIN), the next requester
//! gets ERROR_SEM_OWNER_DIED and carries on.
//!
//! When a thread lets go of the lock it wakes the threads it gave work
//! to: queues it posted to, and the owners of windows it invalidated.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::shared::tid;

unsafe extern "C" {
    #[link_name = "DOSCALLS.331"]
    fn DosCreateMutexSem(name: *const u8, phmtx: *mut u32, attr: u32, owned: u32) -> u32;
    #[link_name = "DOSCALLS.332"]
    fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> u32;
    #[link_name = "DOSCALLS.334"]
    fn DosRequestMutexSem(hmtx: u32, timeout: u32) -> u32;
    #[link_name = "DOSCALLS.335"]
    fn DosReleaseMutexSem(hmtx: u32) -> u32;
    #[link_name = "DOSCALLS.229"]
    fn DosSleep(ms: u32) -> u32;
    #[link_name = "DOSCALLS.380"]
    fn DosEnterMustComplete(pul: *mut u32) -> u32;
    #[link_name = "DOSCALLS.381"]
    fn DosExitMustComplete(pul: *mut u32) -> u32;
}

const LOCK_NAME: &[u8] = b"\\SEM32\\PMWIN\\LOCK\0";
const CREATING: u32 = u32::MAX;
const ERROR_SEM_OWNER_DIED: u32 = 105;

static MUTEX: AtomicU32 = AtomicU32::new(0);
/// Thread of this process that holds the lock (0: none here).
static OWNER: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
/// Times the lock was taken over from a thread that died holding it.
pub static RECOVERED: AtomicU32 = AtomicU32::new(0);

/// The mutex semaphore: opened by name, created by the first PM process.
fn mutex() -> u32 {
    loop {
        match MUTEX.load(Ordering::Acquire) {
            0 => {
                if MUTEX.compare_exchange(0, CREATING, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    let mut h = 0;
                    loop {
                        if unsafe { DosOpenMutexSem(LOCK_NAME.as_ptr(), &mut h) } == 0 {
                            break;
                        }
                        if unsafe { DosCreateMutexSem(LOCK_NAME.as_ptr(), &mut h, 0, 0) } == 0 {
                            break;
                        }
                        // Created by another process meanwhile: open it.
                        unsafe { DosSleep(1) };
                    }
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
    let m = mutex();
    let mut n = 0;
    unsafe { DosEnterMustComplete(&mut n) };
    loop {
        match unsafe { DosRequestMutexSem(m, u32::MAX) } {
            0 => break,
            ERROR_SEM_OWNER_DIED => {
                RECOVERED.fetch_add(1, Ordering::Relaxed);
                break;
            }
            // Interrupted: the thread is being ended (it dies as it
            // leaves the must-complete section).
            _ => unsafe {
                DosExitMustComplete(&mut n);
                DosSleep(1);
                DosEnterMustComplete(&mut n);
            },
        }
    }
    OWNER.store(t, Ordering::Relaxed);
    DEPTH.store(depth, Ordering::Relaxed);
}

fn release() {
    OWNER.store(0, Ordering::Relaxed);
    let mut n = 0;
    unsafe {
        DosReleaseMutexSem(mutex());
        DosExitMustComplete(&mut n);
    }
}

/// Take the lock (recursively) without the wake-ups on release: for the
/// allocator.
pub fn enter_quiet() {
    let t = tid();
    if OWNER.load(Ordering::Relaxed) == t {
        DEPTH.fetch_add(1, Ordering::Relaxed);
        return;
    }
    acquire(t, 1);
}

pub fn leave_quiet() {
    if OWNER.load(Ordering::Relaxed) != tid() {
        return;
    }
    if DEPTH.fetch_sub(1, Ordering::Relaxed) == 1 {
        release();
    }
}

/// Entry from the export trampolines: ecx = the export's index in
/// `crate::exports::NAMES` (u32::MAX: none), ebp = the trampoline's frame
/// (the arguments are at ebp + 8).
#[unsafe(naked)]
pub extern "C" fn enter() {
    core::arch::naked_asm!(
        "sub esp, 4", "push ebp", "push ecx",
        "call {f}",
        "add esp, 12", "ret",
        f = sym enter_traced,
    );
}

extern "C" fn enter_traced(idx: u32, ebp: u32) {
    enter_quiet();
    if idx != u32::MAX && DEPTH.load(Ordering::Relaxed) == 1 && tracing() {
        trace_call(idx, ebp + 8);
    }
}

/// PMTRACE set in a process's environment: its PM API calls are
/// logged on the serial port (0: not checked yet, 1: off, 2: on).
static TRACE: AtomicU32 = AtomicU32::new(0);

pub fn tracing() -> bool {
    match TRACE.load(Ordering::Relaxed) {
        0 => {
            let mut v: *const u8 = core::ptr::null();
            let on = unsafe { os2::DosScanEnv(b"PMTRACE\0".as_ptr(), &mut v) } == 0;
            TRACE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        v => v == 2,
    }
}

fn trace_call(idx: u32, args: u32) {
    let Some(&(name, n)) = crate::exports::NAMES.get(idx as usize) else { return };
    let mut s = alloc::string::String::from(name);
    s.push('(');
    for i in 0..n as u32 {
        let v = unsafe { ((args + i * 4) as *const u32).read_unaligned() };
        if i > 0 {
            s.push_str(", ");
        }
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{v:x}"));
    }
    s.push(')');
    crate::trace(format_args!("{} {s}", tid()));
}

pub extern "C" fn leave() {
    if OWNER.load(Ordering::Relaxed) != tid() {
        return;
    }
    if DEPTH.load(Ordering::Relaxed) == 1 {
        crate::msg::wake_pending();
    }
    if DEPTH.fetch_sub(1, Ordering::Relaxed) == 1 {
        release();
    }
}

/// Run `f` (a wait, or application code) with the lock released.
pub fn unlocked<R>(f: impl FnOnce() -> R) -> R {
    let t = tid();
    if OWNER.load(Ordering::Relaxed) != t {
        return f();
    }
    crate::msg::wake_pending();
    let depth = DEPTH.swap(0, Ordering::Relaxed);
    release();
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
                "mov ecx, -1", "call {enter}",
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
