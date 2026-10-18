//! Threads.

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use super::process::Process;
use crate::arch::idt::TrapFrame;

pub const KSTACK_SIZE: usize = 32 * 1024;

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
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

impl Thread {
    pub fn new_idle() -> Arc<Thread> {
        Arc::new(Thread {
            id: 0,
            tid: 0,
            process: None,
            kstack: unsafe { Box::new_zeroed().assume_init() },
            rsp: UnsafeCell::new(0),
            state: AtomicU8::new(State::Running as u8),
            priority: AtomicU32::new(1 << 8),
            wake_at: AtomicU64::new(u64::MAX),
            timed_out: AtomicBool::new(false),
            tib: AtomicU32::new(0),
            stack_base: AtomicU32::new(0),
            stack_top: AtomicU32::new(0),
            idle: true,
            suspend: AtomicU32::new(0),
            exit_requested: AtomicBool::new(false),
            interrupted: AtomicBool::new(false),
            xcpt: crate::sync::SpinLock::new(alloc::vec::Vec::new()),
            fpu: UnsafeCell::new(FpuState::initial()),
            init_run: crate::sync::SpinLock::new(None),
        })
    }

    /// Create a user thread that starts with register state `frame`.
    pub fn new_user(process: &Arc<Process>, tid: u32, frame: TrapFrame) -> Arc<Thread> {
        let t = Arc::new(Thread {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            tid,
            process: Some(Arc::downgrade(process)),
            kstack: unsafe { Box::new_zeroed().assume_init() },
            rsp: UnsafeCell::new(0),
            state: AtomicU8::new(State::Blocked as u8),
            priority: AtomicU32::new(2 << 8),
            wake_at: AtomicU64::new(u64::MAX),
            timed_out: AtomicBool::new(false),
            tib: AtomicU32::new(0),
            stack_base: AtomicU32::new(0),
            stack_top: AtomicU32::new(0),
            idle: false,
            suspend: AtomicU32::new(0),
            exit_requested: AtomicBool::new(false),
            interrupted: AtomicBool::new(false),
            xcpt: crate::sync::SpinLock::new(alloc::vec::Vec::new()),
            fpu: UnsafeCell::new(FpuState::initial()),
            init_run: crate::sync::SpinLock::new(None),
        });
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
