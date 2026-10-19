//! A small lock for DLL state shared by a process's threads.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub struct Lock<T> {
    held: AtomicBool,
    v: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Lock<T> {}

impl<T> Lock<T> {
    pub const fn new(v: T) -> Lock<T> {
        Lock { held: AtomicBool::new(false), v: UnsafeCell::new(v) }
    }

    pub fn lock(&self) -> Guard<'_, T> {
        while self.held.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            unsafe { os2::DosSleep(0) };
        }
        Guard { l: self }
    }
}

pub struct Guard<'a, T> {
    l: &'a Lock<T>,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.l.v.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.l.v.get() }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.l.held.store(false, Ordering::Release);
    }
}
