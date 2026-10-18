//! Kernel threads: threads without an OS/2 process, for work that must
//! block or poll in thread context (mounting hot-plugged storage, ...).
//!
//! A kernel thread runs on the kernel page tables with interrupts enabled,
//! is scheduled like any other thread (class regular) and is only switched
//! out when it blocks or yields, as the kernel is not preemptive. It ends
//! when its function returns.

use alloc::boxed::Box;
use alloc::sync::Arc;

use super::sched;
use super::thread::Thread;

/// Start a kernel thread running `f`.
pub fn spawn(name: &'static str, f: impl FnOnce() + Send + 'static) -> Arc<Thread> {
    let t = Thread::new_kernel(name, Box::new(f));
    sched::start(t.clone());
    t
}

/// First code a kernel thread runs (switch_stacks returns here).
pub extern "C" fn kthread_start() -> ! {
    // schedule() disabled interrupts before switching; a new thread does
    // not return through it.
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
