//! Lock-order checking (builds with debug assertions: `xtask run`'s
//! `checked` profile).
//!
//! Every SpinLock and Mutex has a class: the source location that created
//! it (`#[track_caller]` on the constructors, so a lock inside a struct
//! built by `Process::new` is "Process.space", one class for all
//! processes). Each CPU keeps the spin locks it holds, each thread the
//! Mutexes it holds. Taking lock B while holding A records the pair
//! A -> B; if B -> A was recorded before, the two orders can deadlock on
//! two CPUs, and the first time a pair is seen both ways it is reported on
//! the serial port with the two locks' creation sites and a backtrace.
//! Taking a spin lock this CPU already holds is reported before it spins
//! for ever. Nothing here takes a lock.

use core::panic::Location;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub type Class = &'static Location<'static>;

/// Locks one CPU (spin) or one thread (Mutex) can hold at once and still
/// be checked; deeper nesting is not recorded.
pub const DEPTH: usize = 12;

/// Held locks: (lock address, class).
pub struct Held {
    n: usize,
    locks: [(usize, usize); DEPTH],
}

impl Held {
    pub const fn new() -> Self {
        Held { n: 0, locks: [(0, 0); DEPTH] }
    }
}

const PAIRS: usize = 8192;
/// Recorded pairs (outer class, inner class), open addressing.
static OUTER: [AtomicUsize; PAIRS] = [const { AtomicUsize::new(0) }; PAIRS];
static INNER: [AtomicUsize; PAIRS] = [const { AtomicUsize::new(0) }; PAIRS];
/// Pairs already reported (by slot of the A -> B entry).
static REPORTED: [AtomicBool; PAIRS] = [const { AtomicBool::new(false) }; PAIRS];
static FULL: AtomicBool = AtomicBool::new(false);
/// Reports so far (shown by Ctrl+Alt+F12 dumps).
pub static REPORTS: AtomicUsize = AtomicUsize::new(0);

fn slot(a: usize, b: usize) -> usize {
    let h = (a ^ b.rotate_left(17)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    (h >> 40) % PAIRS
}

/// Find (a, b); with `insert`, add it if missing. Returns its slot.
fn find(a: usize, b: usize, insert: bool) -> Option<usize> {
    let mut i = slot(a, b);
    for _ in 0..PAIRS {
        let o = OUTER[i].load(Ordering::Acquire);
        if o == 0 {
            if !insert {
                return None;
            }
            match OUTER[i].compare_exchange(0, a, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    INNER[i].store(b, Ordering::Release);
                    return Some(i);
                }
                Err(o2) => {
                    if o2 == a {
                        // Someone else is filling this slot: wait for it.
                        while INNER[i].load(Ordering::Acquire) == 0 {
                            core::hint::spin_loop();
                        }
                        if INNER[i].load(Ordering::Acquire) == b {
                            return Some(i);
                        }
                    }
                }
            }
        } else if o == a {
            let mut inn = INNER[i].load(Ordering::Acquire);
            while inn == 0 {
                core::hint::spin_loop();
                inn = INNER[i].load(Ordering::Acquire);
            }
            if inn == b {
                return Some(i);
            }
        }
        i = (i + 1) % PAIRS;
    }
    if insert && !FULL.swap(true, Ordering::Relaxed) {
        crate::log::print_unlocked(format_args!("[lockdep: pair table full]\n"));
    }
    None
}

fn name(c: usize) -> Class {
    unsafe { &*(c as *const Location<'static>) }
}

fn rbp() -> u64 {
    let r: u64;
    unsafe { core::arch::asm!("mov {}, rbp", out(reg) r) };
    r
}

/// Record that `class` (lock at `addr`) is being taken while the locks in
/// `held` are held. `blocking`: a waiting acquisition (try_lock is not).
fn check(held: &Held, addr: usize, class: usize, blocking: bool) {
    for &(ha, hc) in &held.locks[..held.n.min(DEPTH)] {
        if ha == addr {
            if blocking {
                REPORTS.fetch_add(1, Ordering::Relaxed);
                let c = name(class);
                crate::log::print_unlocked(format_args!(
                    "\n[lockdep: lock created at {}:{} taken again by its holder: deadlock]\n",
                    c.file(),
                    c.line()
                ));
                crate::backtrace(rbp());
            }
            continue;
        }
        if !blocking || hc == class {
            // try_lock cannot wait; same-class nesting (two processes'
            // locks) is ordered by the callers.
            continue;
        }
        let Some(i) = find(hc, class, true) else { continue };
        if REPORTED[i].load(Ordering::Relaxed) {
            continue;
        }
        if let Some(j) = find(class, hc, false) {
            if !REPORTED[i].swap(true, Ordering::Relaxed) {
                REPORTED[j].store(true, Ordering::Relaxed);
                REPORTS.fetch_add(1, Ordering::Relaxed);
                let (a, b) = (name(hc), name(class));
                crate::log::print_unlocked(format_args!(
                    "\n[lockdep: lock order inversion: {}:{}:{} -> {}:{}:{} here, the other order seen before]\n",
                    a.file(),
                    a.line(),
                    a.column(),
                    b.file(),
                    b.line(),
                    b.column()
                ));
                crate::backtrace(rbp());
            }
        }
    }
}

fn push(held: &mut Held, addr: usize, class: usize) {
    if held.n < DEPTH {
        held.locks[held.n] = (addr, class);
    }
    held.n += 1;
}

fn pop(held: &mut Held, addr: usize) {
    if held.n == 0 {
        return;
    }
    let n = held.n.min(DEPTH);
    if let Some(i) = held.locks[..n].iter().rposition(|&(a, _)| a == addr) {
        held.locks.copy_within(i + 1..n, i);
    }
    held.n -= 1;
}

/// Before taking a spin lock (interrupts are off).
pub fn spin_acquire(addr: usize, class: Class, blocking: bool) {
    let c = crate::smp::this();
    let class = class as *const _ as usize;
    if c.lockdep_busy.swap(true, Ordering::Relaxed) {
        return;
    }
    // Mutexes the running thread holds (not in an interrupt handler: the
    // interrupted thread's locks are not this code's).
    if c.irq_depth.load(Ordering::Relaxed) == 0 {
        if let Some(t) = c.current().as_ref() {
            check(unsafe { &*t.mutexes.get() }, addr, class, blocking);
        }
    }
    check(unsafe { &*c.spins.get() }, addr, class, blocking);
    c.lockdep_busy.store(false, Ordering::Relaxed);
}

/// A spin lock was taken.
pub fn spin_acquired(addr: usize, class: Class) {
    let c = crate::smp::this();
    push(unsafe { &mut *c.spins.get() }, addr, class as *const _ as usize);
}

/// A spin lock is being released (interrupts still off).
pub fn spin_release(addr: usize) {
    let c = crate::smp::this();
    pop(unsafe { &mut *c.spins.get() }, addr);
}

/// Before (`acquired` false) and after taking a Mutex in thread context.
pub fn mutex_acquire(addr: usize, class: Class, blocking: bool, acquired: bool) {
    let was = crate::arch::interrupts_enabled();
    crate::arch::disable_interrupts();
    let c = crate::smp::this();
    if let Some(t) = c.current().as_ref() {
        let class = class as *const _ as usize;
        if acquired {
            push(unsafe { &mut *t.mutexes.get() }, addr, class);
        } else if !c.lockdep_busy.swap(true, Ordering::Relaxed) {
            check(unsafe { &*t.mutexes.get() }, addr, class, blocking);
            c.lockdep_busy.store(false, Ordering::Relaxed);
        }
    }
    if was {
        crate::arch::enable_interrupts();
    }
}

pub fn mutex_release(addr: usize) {
    let was = crate::arch::interrupts_enabled();
    crate::arch::disable_interrupts();
    let c = crate::smp::this();
    if let Some(t) = c.current().as_ref() {
        pop(unsafe { &mut *t.mutexes.get() }, addr);
    }
    if was {
        crate::arch::enable_interrupts();
    }
}
