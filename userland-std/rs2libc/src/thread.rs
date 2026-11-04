//! POSIX threads over DosCreateThread.
//!
//! A `pthread_t` points at a [`Thread`]. Each thread starts in
//! [`thread_entry`], which gives it TLS (tls.rs), runs the start routine,
//! then the thread's TLS destructors and key destructors, and posts the
//! thread's event semaphore for `pthread_join` (OS/2 reuses thread ids, so
//! joins do not wait on the id).

use alloc::boxed::Box;
use core::ffi::{c_int, c_void};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};

use libc::{pthread_attr_t, pthread_key_t, pthread_t, size_t};

use crate::spin::SpinLock;
use crate::sys::*;
use crate::tls::{self, Dtor, KEYS};

const DETACHED: u32 = 1;
const FINISHED: u32 = 2;

pub struct Thread {
    pub tid: u32,
    start: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
    arg: *mut c_void,
    result: *mut c_void,
    done: u32,
    state: AtomicU32,
    /// Lowest and highest address of the thread's stack.
    pub stack_lo: u32,
    pub stack_hi: u32,
}

static mut MAIN: Thread = Thread {
    tid: 1,
    start: None,
    arg: null_mut(),
    result: null_mut(),
    done: 0,
    state: AtomicU32::new(DETACHED),
    stack_lo: 0,
    stack_hi: 0,
};

fn record_stack(t: *mut Thread) {
    let (tib, _) = info_blocks();
    unsafe {
        (*t).stack_lo = tib.pstack;
        (*t).stack_hi = tib.pstacklimit;
        (*t).tid = (*tib.ptib2).tid;
    }
}

/// Thread 1's TLS and Thread (called first thing by `_start`).
pub unsafe fn init_main() {
    let t = &raw mut MAIN;
    unsafe { tls::setup(t) };
    record_stack(t);
}

/// The attribute object's fields, kept in the opaque `pthread_attr_t`.
#[repr(C)]
struct Attr {
    stack_size: usize,
    guard_size: usize,
    stack_addr: usize,
    detached: usize,
}

fn attr(a: *const pthread_attr_t) -> *mut Attr {
    a as *mut Attr
}

const DEFAULT_STACK: usize = 2 << 20;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_init(a: *mut pthread_attr_t) -> c_int {
    unsafe { *attr(a) = Attr { stack_size: DEFAULT_STACK, guard_size: 0, stack_addr: 0, detached: 0 } };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_destroy(_a: *mut pthread_attr_t) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_setstacksize(a: *mut pthread_attr_t, size: size_t) -> c_int {
    if size < 16384 {
        return libc::EINVAL;
    }
    unsafe { (*attr(a)).stack_size = size };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_getstacksize(a: *const pthread_attr_t, size: *mut size_t) -> c_int {
    unsafe { *size = (*attr(a)).stack_size };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_getguardsize(a: *const pthread_attr_t, size: *mut size_t) -> c_int {
    unsafe { *size = (*attr(a)).guard_size };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_getstack(a: *const pthread_attr_t, addr: *mut *mut c_void, size: *mut size_t) -> c_int {
    unsafe {
        *addr = (*attr(a)).stack_addr as *mut c_void;
        *size = (*attr(a)).stack_size;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_setdetachstate(a: *mut pthread_attr_t, state: c_int) -> c_int {
    unsafe { (*attr(a)).detached = state as usize };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_getattr_np(t: pthread_t, a: *mut pthread_attr_t) -> c_int {
    let t = t as *mut Thread;
    unsafe {
        pthread_attr_init(a);
        if (*t).stack_hi > (*t).stack_lo {
            (*attr(a)).stack_addr = (*t).stack_lo as usize;
            (*attr(a)).stack_size = ((*t).stack_hi - (*t).stack_lo) as usize;
        }
    }
    0
}

/// DosCreateThread's entry: align the stack for Rust code, then start.
#[unsafe(naked)]
unsafe extern "C" fn thread_entry(_t: u32) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "and esp, -16",
        "sub esp, 12",
        "push dword ptr [ebp + 8]",
        "call {start}",
        "mov esp, ebp",
        "pop ebp",
        "ret",
        start = sym thread_start,
    );
}

unsafe extern "C" fn thread_start(t: *mut Thread) {
    unsafe {
        tls::setup(t);
        record_stack(t);
        let r = ((*t).start.unwrap())((*t).arg);
        finish(t, r);
    }
}

/// End the calling thread with result `r`.
unsafe fn finish(t: *mut Thread, r: *mut c_void) -> ! {
    unsafe {
        run_tls_dtors();
        (*t).result = r;
        tls::teardown();
        let done = (*t).done;
        if (*t).state.fetch_or(FINISHED, Ordering::AcqRel) & DETACHED != 0 {
            DosCloseEventSem(done);
            if t != &raw mut MAIN {
                drop(Box::from_raw(t));
            }
        } else {
            DosPostEventSem(done);
        }
        DosExit(0, 0);
    }
    loop {}
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_create(
    out: *mut pthread_t,
    a: *const pthread_attr_t,
    start: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    arg: *mut c_void,
) -> c_int {
    let (stack, detached) = if a.is_null() {
        (DEFAULT_STACK, false)
    } else {
        unsafe { ((*attr(a)).stack_size, (*attr(a)).detached != 0) }
    };
    let mut done = 0;
    if unsafe { DosCreateEventSem(core::ptr::null(), &mut done, 0, 0) } != 0 {
        return libc::EAGAIN;
    }
    let t = Box::into_raw(Box::new(Thread {
        tid: 0,
        start: Some(start),
        arg,
        result: null_mut(),
        done,
        state: AtomicU32::new(if detached { DETACHED } else { 0 }),
        stack_lo: 0,
        stack_hi: 0,
    }));
    unsafe { *out = t as pthread_t };
    let mut tid = 0;
    // Round the stack to whole 64 KB (OS/2 objects come in tiles anyway).
    let stack = ((stack + 0xffff) & !0xffff) as u32;
    let rc = unsafe { DosCreateThread(&mut tid, thread_entry, t as u32, 0, stack) };
    if rc != 0 {
        unsafe {
            DosCloseEventSem(done);
            drop(Box::from_raw(t));
        }
        return libc::EAGAIN;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_join(t: pthread_t, ret: *mut *mut c_void) -> c_int {
    let t = t as *mut Thread;
    unsafe {
        while DosWaitEventSem((*t).done, SEM_INDEFINITE_WAIT) == ERROR_INTERRUPT {}
        if !ret.is_null() {
            *ret = (*t).result;
        }
        DosCloseEventSem((*t).done);
        drop(Box::from_raw(t));
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_detach(t: pthread_t) -> c_int {
    let t = t as *mut Thread;
    unsafe {
        if (*t).state.fetch_or(DETACHED, Ordering::AcqRel) & FINISHED != 0 {
            DosCloseEventSem((*t).done);
            drop(Box::from_raw(t));
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_self() -> pthread_t {
    let tcb = tls::tcb();
    unsafe {
        if (*tcb).thread.is_null() {
            // A thread started by someone else (tls::tcb adopted it).
            let t = Box::into_raw(Box::new(Thread {
                tid: 0,
                start: None,
                arg: null_mut(),
                result: null_mut(),
                done: 0,
                state: AtomicU32::new(DETACHED),
                stack_lo: 0,
                stack_hi: 0,
            }));
            record_stack(t);
            (*tcb).thread = t;
        }
        (*tcb).thread as pthread_t
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_equal(a: pthread_t, b: pthread_t) -> c_int {
    (a == b) as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_exit(r: *mut c_void) -> ! {
    unsafe { finish(pthread_self() as *mut Thread, r) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setname_np(_t: pthread_t, _name: *const core::ffi::c_char) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_yield() -> c_int {
    unsafe { DosSleep(0) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_getaffinity(_pid: c_int, _size: size_t, _set: *mut c_void) -> c_int {
    crate::errno::fail(libc::ENOSYS)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gettid() -> c_int {
    let (tib, _) = info_blocks();
    unsafe { (*tib.ptib2).tid as c_int }
}

// ---- Keys ----

struct Keys {
    used: [bool; KEYS],
    dtors: [Option<unsafe extern "C" fn(*mut c_void)>; KEYS],
}

static KEYTAB: SpinLock<Keys> = SpinLock::new(Keys { used: [false; KEYS], dtors: [None; KEYS] });

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_key_create(key: *mut pthread_key_t, dtor: Option<unsafe extern "C" fn(*mut c_void)>) -> c_int {
    let mut k = KEYTAB.lock();
    // Key 0 is never handed out (std treats 0 as "not yet created").
    for i in 1..KEYS {
        if !k.used[i] {
            k.used[i] = true;
            k.dtors[i] = dtor;
            unsafe { *key = i as pthread_key_t };
            return 0;
        }
    }
    libc::EAGAIN
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_key_delete(key: pthread_key_t) -> c_int {
    let mut k = KEYTAB.lock();
    if (key as usize) < KEYS {
        k.used[key as usize] = false;
        k.dtors[key as usize] = None;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_getspecific(key: pthread_key_t) -> *mut c_void {
    if key as usize >= KEYS {
        return null_mut();
    }
    unsafe { (*tls::tcb()).keys[key as usize] }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setspecific(key: pthread_key_t, v: *const c_void) -> c_int {
    if key as usize >= KEYS {
        return libc::EINVAL;
    }
    unsafe { (*tls::tcb()).keys[key as usize] = v as *mut c_void };
    0
}

/// glibc's hook for `thread_local!` destructors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __cxa_thread_atexit_impl(
    f: unsafe extern "C" fn(*mut c_void),
    arg: *mut c_void,
    _dso: *mut c_void,
) -> c_int {
    let tcb = tls::tcb();
    unsafe {
        let d = Box::into_raw(Box::new(Dtor { f, arg, next: (*tcb).dtors }));
        (*tcb).dtors = d;
    }
    0
}

/// Run the calling thread's TLS destructors, then its key destructors.
pub fn run_tls_dtors() {
    let tcb = tls::tcb();
    unsafe {
        while !(*tcb).dtors.is_null() {
            let d = Box::from_raw((*tcb).dtors);
            (*tcb).dtors = d.next;
            (d.f)(d.arg);
        }
        for _round in 0..4 {
            let mut any = false;
            for i in 1..KEYS {
                let v = (*tcb).keys[i];
                if v.is_null() {
                    continue;
                }
                let dtor = KEYTAB.lock().dtors[i];
                (*tcb).keys[i] = null_mut();
                if let Some(d) = dtor {
                    any = true;
                    d(v);
                }
            }
            if !any {
                break;
            }
        }
    }
}

// pthread mutexes and condition variables for C code (std uses futexes):
// a mutex is a futex word (0 free, 1 locked, 2 contended).

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_init(m: *mut libc::pthread_mutex_t, _a: *const c_void) -> c_int {
    unsafe { core::ptr::write_bytes(m, 0, 1) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_destroy(_m: *mut libc::pthread_mutex_t) -> c_int {
    0
}

fn word(m: *mut libc::pthread_mutex_t) -> &'static AtomicU32 {
    unsafe { &*(m as *const AtomicU32) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_lock(m: *mut libc::pthread_mutex_t) -> c_int {
    let w = word(m);
    if w.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        return 0;
    }
    while w.swap(2, Ordering::Acquire) != 0 {
        crate::futex::wait(w, 2, None);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_trylock(m: *mut libc::pthread_mutex_t) -> c_int {
    if word(m).compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() { 0 } else { libc::EBUSY }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_unlock(m: *mut libc::pthread_mutex_t) -> c_int {
    let w = word(m);
    if w.swap(0, Ordering::Release) == 2 {
        crate::futex::wake(w, 1);
    }
    0
}
