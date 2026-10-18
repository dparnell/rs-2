//! Thread scheduling and context switching.
//!
//! Single processor, non-preemptive kernel: a thread is only switched out
//! when it blocks or when it is about to return to user mode with its time
//! slice used up.
//!
//! OS/2 priorities are a class (idle, regular, time-critical, foreground
//! server) plus a level 0-31; threads run strictly by priority, round robin
//! within one priority.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, Ordering};

use super::thread::{State, Thread};
use crate::arch::{self, gdt};
use crate::irq;
use crate::sync::SpinLock;

/// Ticks per time slice.
const SLICE: u64 = 32;

struct Sched {
    /// Ready threads by effective priority (index 0 = lowest).
    ready: [VecDeque<Arc<Thread>>; 4],
    current: Option<Arc<Thread>>,
    idle: Option<Arc<Thread>>,
    sleepers: Vec<Arc<Thread>>,
    /// Threads that exited; reaped once off their stack.
    dead: Vec<Arc<Thread>>,
    slice_end: u64,
}

static SCHED: SpinLock<Sched> = SpinLock::new(Sched {
    ready: [VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new()],
    current: None,
    idle: None,
    sleepers: Vec::new(),
    dead: Vec::new(),
    slice_end: 0,
});

static NEED_RESCHED: AtomicBool = AtomicBool::new(false);

global_asm!(
    r#"
.global switch_stacks
// switch_stacks(save: *mut u64 [rdi], next: u64 [rsi])
switch_stacks:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, rsi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

.global thread_trampoline
// First switch into a new thread lands here with rsp at its TrapFrame.
thread_trampoline:
    jmp trap_return
"#
);

unsafe extern "C" {
    fn switch_stacks(save: *mut u64, next: u64);
    pub fn thread_trampoline();
}

/// Turn the boot context into the idle thread.
pub fn init() {
    let idle = Thread::new_idle();
    let mut s = SCHED.lock();
    s.current = Some(idle.clone());
    s.idle = Some(idle);
}

pub fn current() -> Arc<Thread> {
    SCHED.lock().current.clone().expect("no current thread")
}

pub fn current_opt() -> Option<Arc<Thread>> {
    SCHED.lock().current.clone()
}

fn class_index(t: &Thread) -> usize {
    // Classes: 1 idle, 2 regular, 3 time critical, 4 foreground server.
    match t.priority_class() {
        1 => 0,
        3 => 3,
        4 => 2,
        _ => 1,
    }
}

/// Make a thread runnable.
pub fn wake(t: &Arc<Thread>) {
    let mut s = SCHED.lock();
    if t.state() == State::Ready || t.state() == State::Running || t.state() == State::Dead {
        return;
    }
    t.set_state(State::Ready);
    s.sleepers.retain(|x| !Arc::ptr_eq(x, t));
    let i = class_index(t);
    s.ready[i].push_back(t.clone());
    // A higher class than the running thread preempts it.
    if let Some(c) = &s.current {
        if i > class_index(c) || c.is_idle() {
            NEED_RESCHED.store(true, Ordering::Relaxed);
        }
    }
}

/// Add a new thread to the run queue.
pub fn start(t: Arc<Thread>) {
    t.set_state(State::Blocked);
    wake(&t);
}

/// Block the current thread until woken (or until `deadline` tick).
pub fn block(deadline: Option<u64>) {
    let t = current();
    {
        let mut s = SCHED.lock();
        t.set_state(State::Blocked);
        if let Some(d) = deadline {
            t.set_wake_at(d);
            s.sleepers.push(t.clone());
        } else {
            t.set_wake_at(u64::MAX);
        }
    }
    schedule();
}

/// Sleep for `ms` milliseconds.
pub fn sleep_ms(ms: u64) {
    if ms == 0 {
        yield_now();
        return;
    }
    block(Some(irq::ticks() + ms * irq::HZ / 1000));
}

pub fn yield_now() {
    let t = current();
    if !t.is_idle() {
        let mut s = SCHED.lock();
        t.set_state(State::Ready);
        let i = class_index(&t);
        s.ready[i].push_back(t.clone());
    }
    schedule();
}

/// Mark the current thread dead and switch away for good. The caller must
/// not hold references (they would never be dropped).
pub fn exit_current() -> ! {
    let t = current();
    let process = t.process();
    {
        let mut s = SCHED.lock();
        t.set_state(State::Dead);
        s.dead.push(t);
    }
    if let Some(p) = process {
        p.thread_exited();
    }
    schedule();
    unreachable!("dead thread rescheduled");
}

/// Make a thread terminate: it unwinds out of any wait and dies at its
/// next return to user mode.
pub fn kill(t: &Arc<Thread>) {
    t.request_exit();
    if t.state() == State::Blocked || t.state() == State::Suspended {
        wake(t);
    }
}

/// Called from the timer interrupt.
pub fn tick() {
    let now = irq::ticks();
    let mut woke = Vec::new();
    {
        let mut s = SCHED.lock();
        s.sleepers.retain(|t| {
            if t.wake_at() <= now {
                woke.push(t.clone());
                false
            } else {
                true
            }
        });
        if now >= s.slice_end {
            NEED_RESCHED.store(true, Ordering::Relaxed);
        }
    }
    for t in woke {
        t.set_timed_out(true);
        wake(&t);
    }
}

pub fn need_resched() -> bool {
    NEED_RESCHED.load(Ordering::Relaxed)
}

/// Preempt if the time slice ran out and someone else can run.
pub fn maybe_preempt() {
    if NEED_RESCHED.swap(false, Ordering::Relaxed) {
        let has_other = {
            let s = SCHED.lock();
            s.ready.iter().any(|q| !q.is_empty())
        };
        if has_other {
            yield_now();
        } else {
            SCHED.lock().slice_end = irq::ticks() + SLICE;
        }
    }
}

/// Pick the next thread and switch to it.
pub fn schedule() {
    let was = arch::interrupts_enabled();
    arch::disable_interrupts();
    let (prev, next) = {
        let mut s = SCHED.lock();
        let next = (0..4)
            .rev()
            .find_map(|i| s.ready[i].pop_front())
            .unwrap_or_else(|| s.idle.clone().unwrap());
        let prev = s.current.replace(next.clone()).unwrap();
        s.slice_end = irq::ticks() + SLICE;
        // Free threads that died earlier (we are not on their stacks).
        s.dead.retain(|d| Arc::ptr_eq(d, &prev));
        (prev, next)
    };
    NEED_RESCHED.store(false, Ordering::Relaxed);
    if !Arc::ptr_eq(&prev, &next) {
        next.set_state(State::Running);
        prepare_switch(&prev, &next);
        // Drop our references before switching: a dead thread never comes
        // back to release them. Both threads stay alive through their
        // owners (SCHED.current / dead list, a process thread list, idle).
        let save = prev.rsp_slot();
        let target = unsafe { *next.rsp_slot() };
        // Eager FPU/SSE switch (the kernel itself never uses the FPU).
        unsafe {
            let p = (*prev.fpu.get()).0.as_mut_ptr();
            let n = (*next.fpu.get()).0.as_ptr();
            core::arch::asm!("fxsave64 [{}]", in(reg) p, options(nostack));
            core::arch::asm!("fxrstor64 [{}]", in(reg) n, options(nostack));
        }
        drop(prev);
        drop(next);
        unsafe { switch_stacks(save, target) };
    } else if prev.state() != State::Dead {
        prev.set_state(State::Running);
    }
    if was {
        arch::enable_interrupts();
    }
}

fn prepare_switch(prev: &Arc<Thread>, next: &Arc<Thread>) {
    gdt::set_kernel_stack(next.kstack_top());
    match next.process() {
        Some(p) => {
            let same = prev.process().is_some_and(|q| Arc::ptr_eq(&p, &q));
            if !same || prev.state() == State::Dead {
                p.space.lock().activate();
            }
            gdt::set_tib_base(next.tib(), 0xfff);
        }
        None => {
            // Kernel threads run on the kernel page tables so a process's
            // tables can be freed while idle runs.
            let k = crate::mm::paging::kernel_root();
            if crate::mm::paging::current_root() != k {
                unsafe { crate::arch::write_cr3(k) };
            }
        }
    }
}
