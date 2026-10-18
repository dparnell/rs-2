//! Processes, threads and scheduling.

pub mod kthread;
pub mod process;
pub mod sched;
pub mod thread;

use alloc::collections::VecDeque;
use alloc::sync::Arc;

use crate::arch::gdt;
use crate::arch::idt::TrapFrame;
use crate::sync::SpinLock;
use crate::vm::{self, Fault};
use thread::{State, Thread};

pub use process::Process;

/// Threads waiting for an event.
pub struct WaitQueue {
    waiters: SpinLock<VecDeque<Arc<Thread>>>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self { waiters: SpinLock::new(VecDeque::new()) }
    }

    /// Block the current thread until `ready` holds, the deadline (in
    /// ticks) passes, or the thread is told to terminate or has a signal
    /// pending. Returns false in the latter cases. `ready` is re-checked with
    /// the queue locked to avoid lost wake-ups.
    pub fn wait_until(&self, deadline: Option<u64>, ready: impl FnMut() -> bool) -> bool {
        self.wait(deadline, true, ready)
    }

    /// Wait for `ready` regardless of termination requests and signals
    /// (kernel locks and work queues that must not be abandoned).
    pub fn wait_uninterruptible(&self, ready: impl FnMut() -> bool) {
        self.wait(None, false, ready);
    }

    fn wait(&self, deadline: Option<u64>, interruptible: bool, mut ready: impl FnMut() -> bool) -> bool {
        loop {
            let me = sched::current();
            {
                let mut w = self.waiters.lock();
                if ready() {
                    return true;
                }
                if deadline.is_some_and(|d| crate::irq::ticks() >= d) {
                    return false;
                }
                // Do not go to sleep with a kill or a signal pending: the
                // wake-up that announced it has already happened.
                if interruptible && (me.exit_requested() || me.interrupted()) {
                    return false;
                }
                w.push_back(me.clone());
                // Mark blocked while holding the queue lock so a wake between
                // here and block() is not lost.
                me.set_state(State::Blocked);
            }
            sched::block(deadline);
            self.waiters.lock().retain(|t| !Arc::ptr_eq(t, &me));
            if interruptible && (me.exit_requested() || me.interrupted()) {
                return ready();
            }
            if me.timed_out() && !ready() {
                return false;
            }
        }
    }

    /// Whether a thread matching `f` is waiting.
    pub fn has_waiter(&self, f: impl Fn(&Thread) -> bool) -> bool {
        self.waiters.lock().iter().any(|t| f(t))
    }

    pub fn wake_one(&self) {
        if let Some(t) = self.waiters.lock().pop_front() {
            sched::wake(&t);
        }
    }

    pub fn wake_all(&self) {
        let ts: alloc::vec::Vec<_> = self.waiters.lock().drain(..).collect();
        for t in ts {
            sched::wake(&t);
        }
    }
}

/// A user thread raised a CPU exception.
pub fn user_exception(f: &mut TrapFrame) {
    let t = sched::current();
    let Some(p) = t.process() else { return };
    if f.vector == 14 {
        let addr = crate::arch::read_cr2();
        let write = f.error & 2 != 0;
        if addr < vm::TILED_TOP as u64 * 8 {
            let r = p.space.lock().fault_in(addr as u32, write);
            match r {
                Fault::Resolved => return,
                Fault::Guard => {
                    // Default guard page handling: grow the stack down.
                    let mut sp = p.space.lock();
                    let page = (addr as u32) & !(vm::PAGE - 1);
                    if page >= vm::PAGE {
                        let _ = sp.set_mem(page - vm::PAGE, vm::PAGE, vm::pag::COMMIT | vm::pag::GUARD);
                    }
                    if sp.fault_in(addr as u32, write) == Fault::Resolved {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
    crate::api::exceptions::deliver(f, &p, &t);
}

/// Resolve a page fault taken by the kernel while touching user memory.
pub fn resolve_user_fault(addr: u64, error: u64) -> bool {
    let Some(t) = sched::current_opt() else { return false };
    let Some(p) = t.process() else { return false };
    let r = p.space.lock().fault_in(addr as u32, error & 2 != 0);
    matches!(r, Fault::Resolved | Fault::Guard)
}

pub fn timer_tick(_f: &mut TrapFrame) {
    sched::tick();
    crate::api::sysctl::tick(sched::current_opt().is_none_or(|t| t.is_idle()));
    crate::api::timer::tick();
    crate::drivers::xhci::tick();
    if crate::irq::ticks() % 250 == 0 {
        crate::infoseg::update();
    }
}

/// Last chance before `iretq` to user mode: preemption and sanity checks on
/// the selectors we are about to load. Returns true if the current thread
/// must terminate instead (the caller then calls `sched::exit_current`
/// without holding references).
pub fn before_user_return(f: &mut TrapFrame) -> bool {
    sched::maybe_preempt();
    let t = sched::current();
    if t.exit_requested() {
        // 32-bit exception handlers see the termination first (and may run
        // user code for it); then the thread ends.
        let p = t.process();
        return !p.is_some_and(|p| crate::api::exceptions::exit_unwind(f, &p, &t));
    }
    let Some(p) = t.process() else { return false };
    crate::api::proc::hold_if_needed(&t, &p);
    if crate::api::proc::exit_list_step(&t, &p, f) || t.exit_requested() {
        return true;
    }
    crate::api::signal::deliver(&t, &p, f);
    if t.exit_requested() {
        return true;
    }
    let space = p.space.lock();
    let ok_data = |sel: u64| -> bool {
        let sel = sel as u16;
        if sel < 4 {
            return true;
        }
        if sel & 3 != 3 {
            return false;
        }
        let d = space.descriptor(sel);
        // Present, code/data, DPL 3; code must be readable.
        let present = d & (1 << 47) != 0;
        let s = d & (1 << 44) != 0;
        let dpl = (d >> 45) & 3;
        let code = d & (1 << 43) != 0;
        let readable = d & (1 << 41) != 0;
        present && s && dpl == 3 && (!code || readable)
    };
    for seg in [&mut f.ds, &mut f.es, &mut f.fs, &mut f.gs] {
        if !ok_data(*seg) {
            *seg = 0;
        }
    }
    let cs = f.cs as u16;
    let ss = f.ss as u16;
    let dcs = space.descriptor(cs);
    let dss = space.descriptor(ss);
    let cs_ok = cs & 3 == 3 && dcs & (1 << 47) != 0 && dcs & (1 << 43) != 0 && (dcs >> 45) & 3 == 3 && dcs & (1 << 53) == 0;
    let ss_ok = ss & 3 == 3
        && dss & (1 << 47) != 0
        && dss & (1 << 43) == 0
        && dss & (1 << 41) != 0
        && (dss >> 45) & 3 == 3;
    drop(space);
    if !cs_ok || !ss_ok {
        crate::println!(
            "process {}: invalid CS {:#x} or SS {:#x} at return, terminating",
            p.pid,
            cs,
            ss
        );
        p.exit(2, 0xffff);
        return true;
    }
    let _ = gdt::USER_CS;
    false
}
