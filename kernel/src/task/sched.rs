//! Thread scheduling and context switching.
//!
//! Non-preemptive kernel: a thread is only switched out when it blocks or
//! when it is about to return to user mode with its time slice used up.
//! Every CPU runs the same scheduler over shared run queues; each has its
//! own current and idle thread (smp.rs). A thread is marked `on_cpu` from
//! the moment a CPU picks it until that CPU has finished switching away
//! from it, so it is never resumed on two CPUs, or on one while another is
//! still on its stack.
//!
//! OS/2 priorities are a class (idle, regular, time-critical, foreground
//! server) plus a level 0-31; threads run strictly by priority, round robin
//! within one priority. With PRIORITY=DYNAMIC (the default) a regular-class
//! thread that has been ready for MAXWAIT seconds without running is
//! boosted for one turn to time-critical level 0, above the regular and
//! foreground-server classes, so it cannot starve forever.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::thread::{State, Thread};
use crate::arch::{self, gdt};
use crate::irq;
use crate::smp;
use crate::sync::SpinLock;

/// Ticks per time slice (CONFIG.SYS TIMESLICE=).
static SLICE: AtomicU64 = AtomicU64::new(32);
/// Starvation boost enabled (PRIORITY=DYNAMIC) and its delay in ticks
/// (MAXWAIT=, seconds).
static DYNAMIC: AtomicBool = AtomicBool::new(true);
static MAXWAIT: AtomicU64 = AtomicU64::new(3 * irq::HZ);

/// Run queues: 4 classes x 32 levels, index 0 = lowest.
const QUEUES: usize = 128;
/// Where a starvation-boosted thread runs: time-critical, level 0.
const BOOST_QUEUE: usize = 3 * 32;

struct Sched {
    /// Ready threads by effective priority (index 0 = lowest).
    ready: [VecDeque<Arc<Thread>>; QUEUES],
    sleepers: Vec<Arc<Thread>>,
    /// Threads that exited; freed once no CPU is on their stack.
    dead: Vec<Arc<Thread>>,
}

static SCHED: SpinLock<Sched> = SpinLock::new(Sched {
    ready: [const { VecDeque::new() }; QUEUES],
    sleepers: Vec::new(),
    dead: Vec::new(),
});

static LOCK_WARNED: AtomicBool = AtomicBool::new(false);
/// Bumped whenever a thread is queued (or left queued for another try):
/// an idle CPU looks at the run queues only when it changed.
static READY_GEN: AtomicU64 = AtomicU64::new(0);

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
    call {finish}
    jmp trap_return
"#,
    finish = sym finish_switch,
);

unsafe extern "C" {
    fn switch_stacks(save: *mut u64, next: u64);
    pub fn thread_trampoline();
}

/// Turn the boot context into CPU 0's idle thread.
pub fn init() {
    let idle = Thread::new_idle();
    let c = smp::this();
    *c.idle() = Some(idle.clone());
    *c.current() = Some(idle);
}

/// The thread running on this CPU.
pub fn current() -> Arc<Thread> {
    current_opt().expect("no current thread")
}

pub fn current_opt() -> Option<Arc<Thread>> {
    let was = arch::interrupts_enabled();
    arch::disable_interrupts();
    let t = smp::this().current().clone();
    if was {
        arch::enable_interrupts();
    }
    t
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

/// Run queue of a thread: class and level, or the boost queue.
fn queue_index(t: &Thread) -> usize {
    if t.boosted.load(Ordering::Relaxed) {
        return BOOST_QUEUE;
    }
    class_index(t) * 32 + (t.priority() & 0xff).min(31) as usize
}

/// Scheduler settings from CONFIG.SYS: PRIORITY=, MAXWAIT=, TIMESLICE=.
pub fn configure(dynamic: bool, maxwait_s: u32, slice_ms: u32) {
    DYNAMIC.store(dynamic, Ordering::Relaxed);
    MAXWAIT.store(maxwait_s.clamp(1, 255) as u64 * irq::HZ, Ordering::Relaxed);
    SLICE.store((slice_ms.max(1) as u64 * irq::HZ / 1000).max(1), Ordering::Relaxed);
}

fn slice() -> u64 {
    SLICE.load(Ordering::Relaxed)
}

/// Put a ready thread on its queue.
fn enqueue(s: &mut Sched, t: &Arc<Thread>) -> usize {
    let i = queue_index(t);
    t.ready_since.store(irq::ticks(), Ordering::Relaxed);
    s.ready[i].push_back(t.clone());
    READY_GEN.fetch_add(1, Ordering::Release);
    i
}

/// Make another CPU (or this one) run a thread that became ready on queue
/// `i`: an idle CPU takes it, else one running something lower.
fn preempt_for(i: usize) {
    let me = smp::this();
    if me.cur_class.load(Ordering::Relaxed) == smp::IDLE_CLASS {
        me.need_resched.store(true, Ordering::Relaxed);
        return;
    }
    if let Some(c) = smp::online().find(|c| c.cur_class.load(Ordering::Relaxed) == smp::IDLE_CLASS) {
        // Also ask it to reschedule: it may have just picked up a thread
        // and not updated its class yet.
        c.need_resched.store(true, Ordering::Relaxed);
        smp::kick(c);
        return;
    }
    // The CPU running the lowest-priority thread, if lower than `i`.
    let lowest = smp::online().min_by_key(|c| c.cur_class.load(Ordering::Relaxed));
    if let Some(c) = lowest {
        if (c.cur_class.load(Ordering::Relaxed) as usize) < i {
            c.need_resched.store(true, Ordering::Relaxed);
            smp::kick(c);
        }
    }
}

/// A thread's priority changed (DosSetPriority): move it if it is queued.
pub fn reprioritize(t: &Arc<Thread>) {
    let i = {
        let mut s = SCHED.lock();
        if t.state() != State::Ready {
            return;
        }
        let mut found = false;
        for q in s.ready.iter_mut() {
            let n = q.len();
            q.retain(|x| !Arc::ptr_eq(x, t));
            found |= q.len() != n;
        }
        if !found {
            return;
        }
        let since = t.ready_since.load(Ordering::Relaxed);
        let i = enqueue(&mut s, t);
        t.ready_since.store(since, Ordering::Relaxed);
        i
    };
    preempt_for(i);
}

/// Make a thread runnable.
pub fn wake(t: &Arc<Thread>) {
    let i = {
        let mut s = SCHED.lock();
        if t.state() == State::Ready || t.state() == State::Running || t.state() == State::Dead {
            return;
        }
        t.set_state(State::Ready);
        s.sleepers.retain(|x| !Arc::ptr_eq(x, t));
        enqueue(&mut s, t)
    };
    preempt_for(i);
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
        // Woken between deciding to block and getting here (callers mark
        // themselves blocked under their wait queue's lock): it is already
        // on the run queue. Take it off again and carry on, so it is never
        // queued twice.
        if t.state() == State::Ready {
            for q in s.ready.iter_mut() {
                q.retain(|x| !Arc::ptr_eq(x, &t));
            }
            t.set_state(State::Running);
            return;
        }
        t.boosted.store(false, Ordering::Relaxed);
        t.set_state(State::Blocked);
        if let Some(d) = deadline {
            t.set_wake_at(d);
            if !s.sleepers.iter().any(|x| Arc::ptr_eq(x, &t)) {
                s.sleepers.push(t.clone());
            }
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
    // Other wake-ups (a stray wake of a wait queue the thread left) do not
    // end the sleep early; termination requests and signals do.
    let deadline = irq::ticks().saturating_add(ms.saturating_mul(irq::HZ) / 1000);
    let t = current();
    while irq::ticks() < deadline && !t.exit_requested() && !t.interrupted() {
        block(Some(deadline));
    }
}

pub fn yield_now() {
    let t = current();
    if !t.is_idle() {
        let mut s = SCHED.lock();
        t.set_state(State::Ready);
        enqueue(&mut s, &t);
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

/// Called from the timer interrupt on every CPU.
pub fn tick() {
    let now = irq::ticks();
    let c = smp::this();
    if now >= c.slice_end.load(Ordering::Relaxed) {
        c.need_resched.store(true, Ordering::Relaxed);
        // A starvation boost lasts until the time slice ends (or the
        // thread blocks).
        if let Some(t) = c.current().as_ref() {
            t.boosted.store(false, Ordering::Relaxed);
        }
    }
    // Sleepers are woken by the boot processor.
    if c.index() != 0 {
        return;
    }
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
    }
    for t in woke {
        t.set_timed_out(true);
        wake(&t);
    }
    if now % 100 == 0 && DYNAMIC.load(Ordering::Relaxed) {
        boost_starved(now);
    }
}

/// MAXWAIT: regular-class threads ready for that long without running are
/// boosted for one turn.
fn boost_starved(now: u64) {
    let wait = MAXWAIT.load(Ordering::Relaxed);
    let boosted = {
        let mut s = SCHED.lock();
        let mut moved = Vec::new();
        for q in s.ready[32..64].iter_mut() {
            q.retain(|t| {
                let starved = now.saturating_sub(t.ready_since.load(Ordering::Relaxed)) >= wait;
                if starved && t.state() == State::Ready {
                    moved.push(t.clone());
                }
                !starved
            });
        }
        for t in &moved {
            t.boosted.store(true, Ordering::Relaxed);
            s.ready[BOOST_QUEUE].push_back(t.clone());
        }
        !moved.is_empty()
    };
    if boosted {
        preempt_for(BOOST_QUEUE);
    }
}

pub fn need_resched() -> bool {
    smp::this().need_resched.load(Ordering::Relaxed)
}

fn has_ready() -> bool {
    SCHED.lock().ready.iter().any(|q| q.iter().any(|t| !t.on_cpu.load(Ordering::Acquire)))
}

/// Preempt if the time slice ran out and someone else can run.
pub fn maybe_preempt() {
    if smp::this().need_resched.swap(false, Ordering::Relaxed) {
        if has_ready() {
            yield_now();
        } else {
            smp::this().slice_end.store(irq::ticks() + slice(), Ordering::Relaxed);
        }
    }
}

/// The idle loop of every CPU. After an interrupt it only goes through
/// the scheduler if it was asked to (a wake-up for this CPU, the end of
/// its time slice) or a thread was queued since it last looked, so idle
/// CPUs do not take the run queue lock on every timer tick.
pub fn idle_loop() -> ! {
    loop {
        let seen = READY_GEN.load(Ordering::Acquire);
        schedule();
        loop {
            // Halt unless something became ready meanwhile; `sti; hlt`
            // lets no interrupt slip in between.
            arch::disable_interrupts();
            if need_resched() || READY_GEN.load(Ordering::Acquire) != seen {
                arch::enable_interrupts();
                break;
            }
            unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
        }
    }
}

/// Release the thread this CPU switched away from (it is now off our
/// stack). Runs first thing in every thread a switch lands in.
pub extern "C" fn finish_switch() {
    let c = smp::this();
    if let Some(prev) = c.switched_from().take() {
        prev.on_cpu.store(false, Ordering::Release);
    }
}

/// Pick the next thread and switch to it.
pub fn schedule() {
    // Switching away while holding a spin lock would leave it locked for
    // the next thread, which then spins forever with interrupts off.
    let held = crate::sync::held_spin_locks();
    if held != 0 && !LOCK_WARNED.swap(true, Ordering::Relaxed) {
        crate::println!("[scheduler: switching with {held} spin lock(s) held]");
        let rbp: u64;
        unsafe { core::arch::asm!("mov {}, rbp", out(reg) rbp) };
        crate::backtrace(rbp);
    }
    let was = arch::interrupts_enabled();
    arch::disable_interrupts();
    let c = smp::this();
    let me = c.current().clone().expect("no current thread");
    // Give up the kernel lock while others run; take it back on return.
    let had_bkl = smp::bkl_release_for(&me);
    let next = {
        let mut s = SCHED.lock();
        // Only threads still marked ready (a thread that died or blocked
        // again must not run from a stale queue entry), and not one that
        // another CPU is still switching away from.
        let mut skipped = Vec::new();
        let next = loop {
            match (0..QUEUES).rev().find_map(|i| s.ready[i].pop_front()) {
                Some(t) if t.state() != State::Ready => continue,
                Some(t) if t.on_cpu.load(Ordering::Acquire) && !Arc::ptr_eq(&t, &me) => skipped.push(t),
                Some(t) => break Some(t),
                None => break None,
            }
        };
        if !skipped.is_empty() {
            // Still another CPU's: idle CPUs try again later.
            READY_GEN.fetch_add(1, Ordering::Release);
        }
        for t in skipped.into_iter().rev() {
            let i = queue_index(&t);
            s.ready[i].push_front(t);
        }
        let next = match next {
            Some(t) => t,
            // Nothing else: keep running a thread that can still run, else
            // idle.
            None if me.state() == State::Running && !me.is_idle() => me.clone(),
            None => c.idle().clone().unwrap(),
        };
        next.on_cpu.store(true, Ordering::Release);
        // Free threads that died earlier and are off every stack.
        s.dead.retain(|d| d.on_cpu.load(Ordering::Acquire) || Arc::ptr_eq(d, &me));
        next
    };
    c.slice_end.store(irq::ticks() + slice(), Ordering::Relaxed);
    c.need_resched.store(false, Ordering::Relaxed);
    c.cur_class.store(if next.is_idle() { smp::IDLE_CLASS } else { queue_index(&next) as u8 }, Ordering::Relaxed);
    if !me.kstack_intact() {
        panic!("kernel stack overflow in thread {} (tid {})", me.id, me.tid);
    }
    if !Arc::ptr_eq(&me, &next) {
        crate::api::perf::switched(c.index(), me.is_idle(), next.is_idle());
        next.set_state(State::Running);
        prepare_switch(&me, &next);
        let save = me.rsp_slot();
        let target = unsafe { *next.rsp_slot() };
        // Eager FPU/SSE switch (the kernel itself never uses the FPU).
        unsafe {
            let p = (*me.fpu.get()).0.as_mut_ptr();
            let n = (*next.fpu.get()).0.as_ptr();
            core::arch::asm!("fxsave64 [{}]", in(reg) p, options(nostack));
            core::arch::asm!("fxrstor64 [{}]", in(reg) n, options(nostack));
        }
        *c.current() = Some(next);
        // The new thread clears `me.on_cpu` once we are off this stack. A
        // dead thread never comes back, so no references may stay here.
        *c.switched_from() = Some(me);
        unsafe { switch_stacks(save, target) };
        // Back on this thread, possibly on another CPU.
        finish_switch();
    } else if me.state() != State::Dead {
        me.set_state(State::Running);
        drop(me);
    }
    if had_bkl {
        smp::bkl_acquire();
    }
    if was {
        arch::enable_interrupts();
    }
}

fn prepare_switch(prev: &Arc<Thread>, next: &Arc<Thread>) {
    gdt::set_kernel_stack(next.kstack_top());
    let (pp, np) = (prev.process(), next.process());
    let same = matches!((&pp, &np), (Some(a), Some(b)) if Arc::ptr_eq(a, b));
    if let Some(pp) = &pp {
        if !same {
            // This CPU leaves the previous address space (TLB shootdowns).
            pp.cpus.fetch_and(!smp::this().bit(), Ordering::SeqCst);
        }
    }
    match np {
        Some(p) => {
            // With thread-local memory this CPU runs on its own copies of
            // the top page tables, mapping the next thread's page.
            match crate::mm::tlm::switch_in(&p, next) {
                Some(cr3) => crate::vm::activate(p.cr3, cr3, p.ldt_base, &p.cpus),
                None if !same || prev.state() == State::Dead => crate::vm::activate(p.cr3, p.cr3, p.ldt_base, &p.cpus),
                None => {}
            }
            gdt::set_tib_base(next.tib(), 0xfff);
        }
        None => {
            // Kernel threads run on the kernel page tables so a process's
            // tables can be freed while idle runs.
            let k = crate::mm::paging::kernel_root();
            smp::this().space_root.store(0, Ordering::Relaxed);
            if crate::arch::read_cr3() & crate::mm::paging::ADDR_MASK != k {
                unsafe { crate::arch::write_cr3(k) };
            }
        }
    }
}
