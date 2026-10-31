//! Threads.

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use super::process::Process;
use crate::arch::idt::TrapFrame;

/// Kernel stack per thread. The loader recurses through DLL dependencies
/// with large frames (unoptimised builds especially): 32 KB overflowed into
/// the neighbouring heap block when a program pulled in a chain of DLLs.
pub const KSTACK_SIZE: usize = 128 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum State {
    Ready = 0,
    Running = 1,
    Blocked = 2,
    Suspended = 3,
    Dead = 4,
}

#[repr(C, align(16))]
struct KStack([u8; KSTACK_SIZE]);

/// FXSAVE area: x87, MMX and SSE state.
#[repr(C, align(16))]
pub struct FpuState(pub [u8; 512]);

impl FpuState {
    /// State after FNINIT with the default MXCSR.
    pub fn initial() -> alloc::boxed::Box<FpuState> {
        let mut f = alloc::boxed::Box::new(FpuState([0; 512]));
        f.0[0..2].copy_from_slice(&0x037fu16.to_le_bytes()); // FCW
        f.0[24..28].copy_from_slice(&0x1f80u32.to_le_bytes()); // MXCSR
        f.0[28..32].copy_from_slice(&0xffffu32.to_le_bytes()); // MXCSR_MASK
        f
    }
}

pub struct Thread {
    /// System-wide unique id.
    pub id: u32,
    /// OS/2 thread ordinal within its process (1 = main thread).
    pub tid: u32,
    process: Option<Weak<Process>>,
    kstack: Box<KStack>,
    rsp: UnsafeCell<u64>,
    state: AtomicU8,
    /// Priority: class << 8 | level.
    priority: AtomicU32,
    wake_at: AtomicU64,
    timed_out: AtomicBool,
    /// Linear address of the thread information block (TIB).
    tib: AtomicU32,
    /// Bottom and top of the user stack.
    pub stack_base: AtomicU32,
    pub stack_top: AtomicU32,
    idle: bool,
    /// Pending suspend count (DosSuspendThread).
    pub suspend: AtomicU32,
    /// Set when the thread must terminate at its next return to user mode
    /// (or when its current wait is interrupted).
    exit_requested: AtomicBool,
    /// A signal is pending: blocking waits return early.
    interrupted: AtomicBool,
    /// Exceptions being dispatched to this thread's handlers (innermost
    /// last).
    pub xcpt: crate::sync::SpinLock<alloc::vec::Vec<crate::api::exceptions::Dispatch>>,
    /// DLL init/term routines running on this thread.
    pub init_run: crate::sync::SpinLock<Option<crate::api::module::InitRun>>,
    /// Saved FPU/SSE state while the thread is switched out.
    pub fpu: UnsafeCell<Box<FpuState>>,
    /// Kernel threads: the function to run (taken when the thread starts)
    /// and a name for messages.
    pub kentry: crate::sync::SpinLock<Option<Box<dyn FnOnce() + Send>>>,
    pub kname: &'static str,
    /// Running on a CPU, or still on its stack while being switched away
    /// from: no other CPU may resume it.
    pub on_cpu: AtomicBool,
    /// Holds the big kernel lock (smp.rs).
    pub holds_bkl: AtomicBool,
    /// Tick at which the thread last became ready (starvation boost).
    pub ready_since: AtomicU64,
    /// Temporarily raised for having waited MAXWAIT seconds to run.
    pub boosted: AtomicBool,
    /// Base of the user stack the kernel allocated for the thread
    /// (DosCreateThread), released when the thread ends; 0 = none.
    pub owned_stack: AtomicU32,
    /// The thread's own page for thread-local memory (mm::tlm), 0 until
    /// first needed.
    pub tlm_page: AtomicU64,
    /// DosEnterMustComplete nesting: signals wait while it is non-zero.
    pub must_complete: AtomicU32,
    /// Watchpoints (debug register numbers, bits 0-3) the thread hit and
    /// has not reported to its debugger yet (api/debug.rs).
    pub watch_hits: AtomicU8,
    /// The upper halves of EAX..EBP and ESP of each interrupt or fault
    /// reflected to the Win16 handler (api/win16.rs), innermost last: its
    /// registers are 16-bit and 386 code keeps 32-bit values in them.
    pub win16_hi: crate::sync::SpinLock<alloc::vec::Vec<[u16; 8]>>,
    /// Termination unwind (XCPT_[ASYNC_]PROCESS_TERMINATE to the exception
    /// handlers before the thread dies): 0 not started, 1 running, 2 done.
    pub exit_unwind: AtomicU8,
    /// The thread ends itself (DosExit, returning from its function): its
    /// handlers see XCPT_PROCESS_TERMINATE rather than the asynchronous one.
    pub sync_exit: AtomicBool,
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Threads alive (created and not yet freed), for the leak checks.
pub static LIVE: AtomicU32 = AtomicU32::new(0);

impl Drop for Thread {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
        let p = self.tlm_page.load(Ordering::Relaxed);
        if p != 0 {
            crate::mm::phys::free(p);
        }
    }
}

impl Thread {
    fn blank(id: u32, tid: u32, process: Option<Weak<Process>>, state: State, priority: u32, idle: bool) -> Thread {
        LIVE.fetch_add(1, Ordering::Relaxed);
        Thread {
            id,
            tid,
            process,
            kstack: unsafe { Box::new_zeroed().assume_init() },
            rsp: UnsafeCell::new(0),
            state: AtomicU8::new(state as u8),
            priority: AtomicU32::new(priority),
            wake_at: AtomicU64::new(u64::MAX),
            timed_out: AtomicBool::new(false),
            tib: AtomicU32::new(0),
            stack_base: AtomicU32::new(0),
            stack_top: AtomicU32::new(0),
            idle,
            suspend: AtomicU32::new(0),
            exit_requested: AtomicBool::new(false),
            interrupted: AtomicBool::new(false),
            xcpt: crate::sync::SpinLock::new(alloc::vec::Vec::new()),
            fpu: UnsafeCell::new(FpuState::initial()),
            init_run: crate::sync::SpinLock::new(None),
            kentry: crate::sync::SpinLock::new(None),
            kname: "",
            on_cpu: AtomicBool::new(idle),
            holds_bkl: AtomicBool::new(false),
            ready_since: AtomicU64::new(0),
            boosted: AtomicBool::new(false),
            owned_stack: AtomicU32::new(0),
            tlm_page: AtomicU64::new(0),
            must_complete: AtomicU32::new(0),
            watch_hits: AtomicU8::new(0),
            win16_hi: crate::sync::SpinLock::new(alloc::vec::Vec::new()),
            exit_unwind: AtomicU8::new(0),
            sync_exit: AtomicBool::new(false),
        }
    }

    pub fn new_idle() -> Arc<Thread> {
        Arc::new(Thread::blank(0, 0, None, State::Running, 1 << 8, true))
    }

    /// Create a kernel thread (no process) that runs `f` and then ends.
    pub fn new_kernel(name: &'static str, f: Box<dyn FnOnce() + Send>) -> Arc<Thread> {
        let mut t = Thread::blank(NEXT_ID.fetch_add(1, Ordering::Relaxed), 0, None, State::Blocked, 2 << 8, false);
        t.kname = name;
        *t.kentry.lock() = Some(f);
        let t = Arc::new(t);
        // switch_stacks pops six registers and returns into kthread_start,
        // which is entered with the stack aligned as after a call.
        let top = t.kstack_top() & !15;
        unsafe {
            let mut sp = top - 8;
            *(sp as *mut u64) = 0;
            sp -= 8;
            *(sp as *mut u64) = super::kthread::kthread_start as *const () as usize as u64;
            for _ in 0..6 {
                sp -= 8;
                *(sp as *mut u64) = 0;
            }
            *t.rsp.get() = sp;
        }
        t
    }

    /// Create a user thread that starts with register state `frame`.
    pub fn new_user(process: &Arc<Process>, tid: u32, frame: TrapFrame) -> Arc<Thread> {
        let t = Arc::new(Thread::blank(
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
            tid,
            Some(Arc::downgrade(process)),
            State::Blocked,
            2 << 8,
            false,
        ));
        // Initial kernel stack: TrapFrame at the top, below it a
        // switch_stacks frame returning into thread_trampoline.
        let top = t.kstack_top();
        let frame_at = top - core::mem::size_of::<TrapFrame>() as u64;
        unsafe {
            (frame_at as *mut TrapFrame).write(frame);
            let mut sp = frame_at;
            sp -= 8;
            *(sp as *mut u64) = super::sched::thread_trampoline as *const () as usize as u64;
            for _ in 0..6 {
                sp -= 8;
                *(sp as *mut u64) = 0;
            }
            *t.rsp.get() = sp;
        }
        t
    }

    /// The lowest word of the kernel stack is still zero (it is allocated
    /// zeroed and only an almost-overflowing stack reaches it).
    pub fn kstack_intact(&self) -> bool {
        self.kstack.0[..8].iter().all(|&b| b == 0)
    }

    pub fn kstack_top(&self) -> u64 {
        self.kstack.0.as_ptr() as u64 + KSTACK_SIZE as u64
    }

    /// The user-mode register state saved at the top of the kernel stack.
    pub fn user_frame(&self) -> &mut TrapFrame {
        unsafe { &mut *((self.kstack_top() - core::mem::size_of::<TrapFrame>() as u64) as *mut TrapFrame) }
    }

    pub fn rsp_slot(&self) -> *mut u64 {
        self.rsp.get()
    }

    pub fn process(&self) -> Option<Arc<Process>> {
        self.process.as_ref()?.upgrade()
    }

    pub fn state(&self) -> State {
        match self.state.load(Ordering::Acquire) {
            0 => State::Ready,
            1 => State::Running,
            2 => State::Blocked,
            3 => State::Suspended,
            _ => State::Dead,
        }
    }

    pub fn set_state(&self, s: State) {
        self.state.store(s as u8, Ordering::Release);
    }

    pub fn is_idle(&self) -> bool {
        self.idle
    }

    pub fn priority(&self) -> u32 {
        self.priority.load(Ordering::Relaxed)
    }

    pub fn set_priority(&self, p: u32) {
        self.priority.store(p, Ordering::Relaxed);
    }

    pub fn priority_class(&self) -> u32 {
        self.priority() >> 8
    }

    pub fn wake_at(&self) -> u64 {
        self.wake_at.load(Ordering::Relaxed)
    }

    pub fn set_wake_at(&self, t: u64) {
        self.wake_at.store(t, Ordering::Relaxed);
        self.timed_out.store(false, Ordering::Relaxed);
    }

    pub fn set_timed_out(&self, v: bool) {
        self.timed_out.store(v, Ordering::Relaxed);
    }

    pub fn timed_out(&self) -> bool {
        self.timed_out.load(Ordering::Relaxed)
    }

    pub fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::Release);
    }

    pub fn interrupt(&self) {
        self.interrupted.store(true, Ordering::Release);
    }

    pub fn clear_interrupt(&self) {
        self.interrupted.store(false, Ordering::Release);
    }

    pub fn interrupted(&self) -> bool {
        self.interrupted.load(Ordering::Acquire)
    }

    pub fn exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::Acquire)
    }

    pub fn tib(&self) -> u32 {
        self.tib.load(Ordering::Relaxed)
    }

    pub fn set_tib(&self, a: u32) {
        self.tib.store(a, Ordering::Relaxed);
    }
}
