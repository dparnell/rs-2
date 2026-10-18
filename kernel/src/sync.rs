//! Locking primitives.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch;

/// Spin locks held right now (one CPU). A thread must never block with
/// one held: the scheduler checks this (see [held_spin_locks]).
static HELD: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Number of spin locks currently held.
pub fn held_spin_locks() -> u32 {
    HELD.load(Ordering::Relaxed)
}

/// A spin lock that also disables interrupts while held, so it can be
/// shared between thread context and interrupt handlers.
pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self { locked: AtomicBool::new(false), value: UnsafeCell::new(value) }
    }

    pub fn lock(&self) -> SpinGuard<'_, T> {
        let irq = arch::interrupts_enabled();
        arch::disable_interrupts();
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        HELD.fetch_add(1, Ordering::Relaxed);
        SpinGuard { lock: self, irq }
    }

    pub fn try_lock(&self) -> Option<SpinGuard<'_, T>> {
        let irq = arch::interrupts_enabled();
        arch::disable_interrupts();
        if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            HELD.fetch_add(1, Ordering::Relaxed);
            Some(SpinGuard { lock: self, irq })
        } else {
            if irq {
                arch::enable_interrupts();
            }
            None
        }
    }

    /// Break the lock unconditionally. Only for the panic path.
    pub unsafe fn force_unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

pub struct SpinGuard<'a, T> {
    lock: &'a SpinLock<T>,
    irq: bool,
}

impl<T> Deref for SpinGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        HELD.fetch_sub(1, Ordering::Relaxed);
        self.lock.locked.store(false, Ordering::Release);
        if self.irq {
            arch::enable_interrupts();
        }
    }
}

/// A value initialised once during boot and read-only afterwards.
pub struct Once<T> {
    done: AtomicBool,
    value: UnsafeCell<Option<T>>,
}

unsafe impl<T: Send + Sync> Sync for Once<T> {}

impl<T> Once<T> {
    pub const fn new() -> Self {
        Self { done: AtomicBool::new(false), value: UnsafeCell::new(None) }
    }

    pub fn set(&self, v: T) {
        assert!(!self.done.load(Ordering::Acquire), "Once set twice");
        unsafe { *self.value.get() = Some(v) };
        self.done.store(true, Ordering::Release);
    }

    pub fn get(&self) -> Option<&T> {
        if self.done.load(Ordering::Acquire) { unsafe { (*self.value.get()).as_ref() } } else { None }
    }
}

/// A sleeping lock for thread context (may be held across blocking I/O).
pub struct Mutex<T> {
    locked: AtomicBool,
    waiters: crate::task::WaitQueue,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Mutex<T> {}
unsafe impl<T: Send> Send for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self { locked: AtomicBool::new(false), waiters: crate::task::WaitQueue::new(), value: UnsafeCell::new(value) }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        loop {
            if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                return MutexGuard { m: self };
            }
            // Before the scheduler runs (early boot) just spin.
            if crate::task::sched::current_opt().is_none() {
                core::hint::spin_loop();
                continue;
            }
            self.waiters.wait_uninterruptible(|| !self.locked.load(Ordering::Acquire));
        }
    }
}

pub struct MutexGuard<'a, T> {
    m: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.m.value.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.m.value.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.m.locked.store(false, Ordering::Release);
        self.m.waiters.wake_one();
    }
}
