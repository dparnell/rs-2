//! Locking primitives.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch;

/// Spin locks held right now on this CPU. A thread must never block with
/// one held: the scheduler checks this (see [held_spin_locks]).
fn held() -> &'static core::sync::atomic::AtomicU32 {
    &crate::smp::this().held_locks
}

/// Number of spin locks currently held on this CPU.
pub fn held_spin_locks() -> u32 {
    held().load(Ordering::Relaxed)
}

/// A spin lock that also disables interrupts while held, so it can be
/// shared between thread context and interrupt handlers.
pub struct SpinLock<T> {
    locked: AtomicBool,
    /// Where it was made: its class for lock-order checking (lockdep.rs).
    class: crate::lockdep::Class,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    #[track_caller]
    pub const fn new(value: T) -> Self {
        Self { locked: AtomicBool::new(false), class: core::panic::Location::caller(), value: UnsafeCell::new(value) }
    }

    pub fn lock(&self) -> SpinGuard<'_, T> {
        let irq = arch::interrupts_enabled();
        arch::disable_interrupts();
        if cfg!(debug_assertions) {
            crate::lockdep::spin_acquire(self as *const _ as *const u8 as usize, self.class, true);
        }
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // Wait on plain reads (test and test-and-set). Interrupts are
            // off: answer TLB shootdowns meanwhile (the holder may be
            // waiting for this CPU's flush).
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
                crate::smp::service_tlb();
            }
        }
        held().fetch_add(1, Ordering::Relaxed);
        if cfg!(debug_assertions) {
            crate::lockdep::spin_acquired(self as *const _ as *const u8 as usize, self.class);
        }
        SpinGuard { lock: self, irq }
    }

    pub fn try_lock(&self) -> Option<SpinGuard<'_, T>> {
        let irq = arch::interrupts_enabled();
        arch::disable_interrupts();
        if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            held().fetch_add(1, Ordering::Relaxed);
            if cfg!(debug_assertions) {
                crate::lockdep::spin_acquire(self as *const _ as *const u8 as usize, self.class, false);
                crate::lockdep::spin_acquired(self as *const _ as *const u8 as usize, self.class);
            }
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
        if cfg!(debug_assertions) {
            crate::lockdep::spin_release(self.lock as *const _ as *const u8 as usize);
        }
        held().fetch_sub(1, Ordering::Relaxed);
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
    class: crate::lockdep::Class,
    waiters: crate::task::WaitQueue,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Mutex<T> {}
unsafe impl<T: Send> Send for Mutex<T> {}

impl<T> Mutex<T> {
    #[track_caller]
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            class: core::panic::Location::caller(),
            waiters: crate::task::WaitQueue::new(),
            value: UnsafeCell::new(value),
        }
    }

    fn addr(&self) -> usize {
        self as *const _ as *const u8 as usize
    }

    /// The lock, if nobody holds it.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        let ok = self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok();
        if ok && cfg!(debug_assertions) {
            crate::lockdep::mutex_acquire(self.addr(), self.class, false, true);
        }
        ok.then(|| MutexGuard { m: self })
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        if cfg!(debug_assertions) {
            crate::lockdep::mutex_acquire(self.addr(), self.class, true, false);
        }
        loop {
            if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                if cfg!(debug_assertions) {
                    crate::lockdep::mutex_acquire(self.addr(), self.class, true, true);
                }
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
        if cfg!(debug_assertions) {
            crate::lockdep::mutex_release(self.m.addr());
        }
        self.m.locked.store(false, Ordering::Release);
        self.m.waiters.wake_one();
    }
}

/// A sleeping lock without data that the thread holding it may take
/// again (the loader lock, the session lock): thread context only. Its
/// guard must be dropped by the thread that took it.
pub struct RecursiveLock {
    /// Thread id + 1 of the holder, 0 = free.
    owner: core::sync::atomic::AtomicU32,
    depth: core::sync::atomic::AtomicU32,
    waiters: crate::task::WaitQueue,
}

pub struct RecursiveGuard<'a> {
    l: &'a RecursiveLock,
    held: bool,
}

impl RecursiveLock {
    #[track_caller]
    pub const fn new() -> Self {
        Self {
            owner: core::sync::atomic::AtomicU32::new(0),
            depth: core::sync::atomic::AtomicU32::new(0),
            waiters: crate::task::WaitQueue::new(),
        }
    }

    fn me() -> Option<u32> {
        crate::task::sched::current_opt().map(|t| t.id + 1)
    }

    /// Whether the calling thread holds it.
    pub fn held(&self) -> bool {
        Self::me().is_some_and(|me| self.owner.load(Ordering::Acquire) == me)
    }

    pub fn lock(&self) -> RecursiveGuard<'_> {
        // Before the scheduler runs (boot) there is nobody to exclude.
        let Some(me) = Self::me() else { return RecursiveGuard { l: self, held: false } };
        // It sleeps: never under a spin lock or in an interrupt handler.
        debug_assert!(
            held_spin_locks() == 0 && crate::smp::this().irq_depth.load(Ordering::Relaxed) == 0,
            "sleeping lock taken with a spin lock held or in an interrupt"
        );
        if self.owner.load(Ordering::Acquire) == me {
            self.depth.fetch_add(1, Ordering::Relaxed);
            return RecursiveGuard { l: self, held: true };
        }
        loop {
            if self.owner.compare_exchange(0, me, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                self.depth.store(1, Ordering::Relaxed);
                return RecursiveGuard { l: self, held: true };
            }
            self.waiters.wait_uninterruptible(|| self.owner.load(Ordering::Acquire) == 0);
        }
    }
}

impl Drop for RecursiveGuard<'_> {
    fn drop(&mut self) {
        if self.held && self.l.depth.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.l.owner.store(0, Ordering::Release);
            self.l.waiters.wake_one();
        }
    }
}
