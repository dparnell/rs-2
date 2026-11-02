//! The interpreter's own stack. REXX procedures recurse (each internal
//! routine call is a few Rust frames deeper), and callers' stacks may be
//! small, so RexxStart runs the interpreter on a 16 MB stack of the
//! thread's (committed on first touch). A nested RexxStart on the same
//! thread (an external function calling REXX) stays on it.

use core::ptr::null_mut;

use os2::*;

const SIZE: u32 = 16 * 1024 * 1024;
/// Thread id -> stack base; threads ids are small.
static mut STACKS: [*mut u8; 1024] = [null_mut(); 1024];

fn esp() -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("mov {}, esp", out(reg) v) };
    v
}

pub fn tid() -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("mov {0}, fs:[0x0c]", "mov {0}, [{0}]", out(reg) v) };
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

fn run_for<F: FnOnce() -> R, R>(_: &Call<F, R>) -> extern "C" fn(*mut u8) {
    run::<F, R>
}

/// Run `f` on the thread's interpreter stack.
pub fn deep<R>(f: impl FnOnce() -> R) -> R {
    let t = (tid() as usize) % 1024;
    let base = unsafe { STACKS[t] };
    let sp = esp();
    if !base.is_null() && sp > base as u32 && sp <= base as u32 + SIZE {
        return f();
    }
    let base = if base.is_null() {
        let mut p: *mut u8 = null_mut();
        if unsafe { DosAllocMem(&mut p, SIZE, PAG_COMMIT | PAG_READ | PAG_WRITE) } != 0 {
            return f();
        }
        unsafe { STACKS[t] = p };
        p
    } else {
        base
    };
    let mut c = Call { f: Some(f), r: None };
    call_on(base as u32 + SIZE, run_for(&c), &mut c as *mut _ as *mut u8);
    c.r.expect("rexx: deep call")
}
