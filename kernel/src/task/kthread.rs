//! Kernel threads: threads without an OS/2 process, for work that must
//! block or poll in thread context (mounting hot-plugged storage, ...).
//!
//! A kernel thread runs on the kernel page tables with interrupts enabled,
//! is scheduled like any other thread (class regular) and is only switched
//! out when it blocks or yields, as the kernel is not preemptive. It runs
//! concurrently with other kernel code under the same locks as API calls.
//! It ends when its function returns.

use alloc::boxed::Box;
use alloc::sync::Arc;

use super::sched;
use super::thread::Thread;

/// Kernel threads started so far (for the thread report; ended ones
/// drop out).
static ALL: crate::sync::SpinLock<alloc::vec::Vec<alloc::sync::Weak<Thread>>> = crate::sync::SpinLock::new(alloc::vec::Vec::new());

/// The kernel threads still alive (None while the list is busy).
pub fn try_all() -> Option<alloc::vec::Vec<Arc<Thread>>> {
    let mut l = ALL.try_lock()?;
    l.retain(|w| w.strong_count() > 0);
    Some(l.iter().filter_map(|w| w.upgrade()).collect())
}

/// Start a kernel thread running `f`.
pub fn spawn(name: &'static str, f: impl FnOnce() + Send + 'static) -> Arc<Thread> {
    let t = Thread::new_kernel(name, Box::new(f));
    {
        let mut l = ALL.lock();
        l.retain(|w| w.strong_count() > 0);
        l.push(Arc::downgrade(&t));
    }
    sched::start(t.clone());
    t
}

/// First code a kernel thread runs (switch_stacks returns here).
pub extern "C" fn kthread_start() -> ! {
    // A new thread does not return through schedule(): finish the switch,
    // then enable interrupts (schedule() disabled them before switching).
    sched::finish_switch();
    crate::arch::enable_interrupts();
    {
        let f = sched::current().kentry.lock().take();
        if let Some(f) = f {
            f();
        }
    }
    sched::exit_current();
}

/// A queue of work items for a kernel thread, filled from any context
/// (including interrupt handlers).
pub struct Inbox<T> {
    items: crate::sync::SpinLock<alloc::collections::VecDeque<T>>,
    wait: super::WaitQueue,
}

impl<T> Inbox<T> {
    #[track_caller]
    pub const fn new() -> Self {
        Self { items: crate::sync::SpinLock::new(alloc::collections::VecDeque::new()), wait: super::WaitQueue::new() }
    }

    pub fn post(&self, v: T) {
        self.items.lock().push_back(v);
        self.wait.wake_all();
    }

    /// Wait for the next item.
    pub fn take(&self) -> T {
        loop {
            if let Some(v) = self.items.lock().pop_front() {
                return v;
            }
            self.wait.wait_uninterruptible(|| !self.items.lock().is_empty());
        }
    }
}
