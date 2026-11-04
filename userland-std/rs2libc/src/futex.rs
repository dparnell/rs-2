//! Futexes (std's Mutex, Condvar, RwLock, Once and thread parking are
//! built on `syscall(SYS_futex)`), in user space: waiters queue in a hashed
//! table under a spin lock and sleep on their thread's event semaphore.

use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::spin::SpinLock;
use crate::sys::*;

struct Waiter {
    addr: usize,
    sem: u32,
    woken: bool,
    next: *mut Waiter,
}

struct Bucket {
    head: *mut Waiter,
}

unsafe impl Send for Bucket {}

const BUCKETS: usize = 64;

static TABLE: [SpinLock<Bucket>; BUCKETS] = [const { SpinLock::new(Bucket { head: null_mut() }) }; BUCKETS];

fn bucket(addr: usize) -> &'static SpinLock<Bucket> {
    &TABLE[(addr >> 2).wrapping_mul(0x9e37_79b9) >> 26 & (BUCKETS - 1)]
}

fn my_sem() -> u32 {
    let tcb = crate::tls::tcb();
    unsafe {
        if (*tcb).sem == 0 {
            let mut h = 0;
            if DosCreateEventSem(core::ptr::null(), &mut h, 0, 0) != 0 {
                crate::start::fatal("rs2libc: no event semaphore for a futex wait\r\n");
            }
            (*tcb).sem = h;
        }
        (*tcb).sem
    }
}

/// Wait while `*f == expected`, at most `ms` milliseconds. Returns false
/// on a time-out, true when woken (or the value differed).
pub fn wait(f: &AtomicU32, expected: u32, ms: Option<u32>) -> bool {
    let addr = f as *const AtomicU32 as usize;
    let sem = my_sem();
    let mut w = Waiter { addr, sem, woken: false, next: null_mut() };
    let wp = &mut w as *mut Waiter;
    {
        let mut b = bucket(addr).lock();
        if f.load(Ordering::SeqCst) != expected {
            return true;
        }
        unsafe { (*wp).next = b.head };
        b.head = wp;
    }
    let rc = unsafe { DosWaitEventSem(sem, ms.unwrap_or(SEM_INDEFINITE_WAIT)) };
    let woken = {
        let mut b = bucket(addr).lock();
        if unsafe { !(*wp).woken } {
            // Timed out (or interrupted): leave the queue.
            let mut p = &mut b.head as *mut *mut Waiter;
            unsafe {
                while !(*p).is_null() {
                    if *p == wp {
                        *p = (*wp).next;
                        break;
                    }
                    p = &mut (**p).next;
                }
            }
            false
        } else {
            true
        }
    };
    // Nobody can post the semaphore any more: clear a post we did not
    // wait for.
    let mut n = 0;
    unsafe { DosResetEventSem(sem, &mut n) };
    woken || rc == 0
}

/// Wake up to `n` waiters on `f`; returns how many were woken.
pub fn wake(f: &AtomicU32, n: u32) -> u32 {
    wake_addr(f as *const AtomicU32 as usize, n)
}

pub fn wake_addr(addr: usize, n: u32) -> u32 {
    let mut b = bucket(addr).lock();
    let mut count = 0;
    let mut p = &mut b.head as *mut *mut Waiter;
    unsafe {
        while !(*p).is_null() && count < n {
            let w = *p;
            if (*w).addr == addr {
                *p = (*w).next;
                (*w).woken = true;
                DosPostEventSem((*w).sem);
                count += 1;
            } else {
                p = &mut (*w).next;
            }
        }
    }
    count
}

const FUTEX_WAIT: i32 = 0;
const FUTEX_WAKE: i32 = 1;
const FUTEX_WAIT_BITSET: i32 = 9;
const FUTEX_WAKE_BITSET: i32 = 10;

/// `syscall(SYS_futex, uaddr, op, val, timeout, uaddr2, val3)`.
pub unsafe fn sys_futex(uaddr: usize, op: i32, val: u32, timeout: usize) -> i32 {
    let f = unsafe { &*(uaddr as *const AtomicU32) };
    match op & 0x7f {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            let ms = if timeout == 0 {
                None
            } else {
                let ts = unsafe { &*(timeout as *const libc::timespec) };
                let req = ts.tv_sec as i64 * 1000 + (ts.tv_nsec as i64 + 999_999) / 1_000_000;
                let ms = if op & 0x7f == FUTEX_WAIT_BITSET {
                    // Absolute CLOCK_MONOTONIC time.
                    req - crate::time::monotonic_ns() as i64 / 1_000_000
                } else {
                    req
                };
                if ms <= 0 {
                    return crate::errno::fail(libc::ETIMEDOUT);
                }
                Some(ms.min(0x7fff_ffff) as u32)
            };
            if f.load(Ordering::SeqCst) != val {
                return crate::errno::fail(libc::EAGAIN);
            }
            if wait(f, val, ms) { 0 } else { crate::errno::fail(libc::ETIMEDOUT) }
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => wake(f, val) as i32,
        _ => crate::errno::fail(libc::ENOSYS),
    }
}
