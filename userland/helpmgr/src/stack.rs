//! A stack of the help manager's own for callers with little stack left.
//! IBM's programs may call (and dispatch messages to) the help manager on
//! small stacks: VIEWDOC.EXE's is 8 KB in its automatic data object.
//! Laying out and drawing panels takes more, so the help manager's window
//! procedures and entry points run on a 256 KB stack of the thread's when
//! less than 64 KB is left below the caller's stack pointer (the TIB's
//! stack base). Exception handlers stay registered on the caller's stack.

use alloc::boxed::Box;
use alloc::vec::Vec;

const NEED: u32 = 64 * 1024;
const SIZE: usize = 256 * 1024;

struct Stacks(core::cell::UnsafeCell<Vec<(u32, Box<[u8]>)>>);
unsafe impl Sync for Stacks {}
static STACKS: Stacks = Stacks(core::cell::UnsafeCell::new(Vec::new()));

fn esp() -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("mov {}, esp", out(reg) v) };
    v
}

/// TIB: tib_pstack (the stack's lowest address) at +4, tib_ptib2 at +0xC
/// (TIB2: the thread id first).
fn tib(off: u32) -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("mov {}, fs:[{}]", out(reg) v, in(reg) off) };
    v
}

/// Call `f(arg)` with the stack pointer at `top`.
#[unsafe(naked)]
extern "C" fn call_on(_top: u32, _f: extern "C" fn(*mut u8), _arg: *mut u8) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "mov eax, [ebp + 8]",
        "mov ecx, [ebp + 12]",
        "mov edx, [ebp + 16]",
        "mov esp, eax",
        "and esp, -16",
        "sub esp, 12",
        "push edx",
        "call ecx",
        "mov esp, ebp",
        "pop ebp",
        "ret",
    );
}

struct Call<F, R> {
    f: Option<F>,
    r: Option<R>,
}

extern "C" fn run<F: FnOnce() -> R, R>(p: *mut u8) {
    let c = unsafe { &mut *(p as *mut Call<F, R>) };
    if let Some(f) = c.f.take() {
        c.r = Some(f());
    }
}

/// Run `f` with enough stack.
pub fn deep<R>(f: impl FnOnce() -> R) -> R {
    let sp = esp();
    let base = tib(4);
    let stacks = unsafe { &mut *STACKS.0.get() };
    let ours = stacks.iter().any(|(_, s)| {
        let lo = s.as_ptr() as u32;
        sp > lo && sp <= lo + SIZE as u32
    });
    if ours || base == 0 || sp < base || sp - base >= NEED {
        return f();
    }
    let tid = unsafe { *(tib(0x0c) as *const u32) };
    let top = match stacks.iter().find(|(t, _)| *t == tid) {
        Some((_, s)) => s.as_ptr() as u32 + SIZE as u32,
        None => {
            let s = alloc::vec![0u8; SIZE].into_boxed_slice();
            let top = s.as_ptr() as u32 + SIZE as u32;
            stacks.push((tid, s));
            top
        }
    };
    let mut c = Call { f: Some(f), r: None };
    call_on(top, run_for(&c), &mut c as *mut _ as *mut u8);
    c.r.expect("helpmgr: deep call")
}

fn run_for<F: FnOnce() -> R, R>(_: &Call<F, R>) -> extern "C" fn(*mut u8) {
    run::<F, R>
}
