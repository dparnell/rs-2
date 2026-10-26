//! Memory 16-bit code can address, for data WIN16 passes to window
//! procedures (strings with WM_SETTEXT, buffers for WM_GETTEXT, the
//! structures of WM_DRAWITEM...): small things on the 16-bit stack, big
//! ones in a tiled scratch area used as a stack.

use crate::Global;

/// The scratch area: base and top (in use below the top).
static SCRATCH: Global<(usize, usize)> = Global::new((0, 0));
const SIZE: usize = 0x10000;

/// A NUL-terminated string on the 16-bit stack (below `thunk16::sp16()`,
/// which the caller saves and puts back).
pub fn push_str(s: &[u8]) -> u32 {
    let far = thunk16::alloc16(s.len() + 1);
    let p = thunk16::flat(far);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
    }
    far
}

/// Run `f` with `n` bytes of 16-bit addressable memory (zeroed): its
/// 16:16 address and flat pointer.
pub fn with<R>(n: usize, f: impl FnOnce(u32, *mut u8) -> R) -> R {
    let s = SCRATCH.get();
    if s.0 == 0 {
        let mut p: *mut u8 = core::ptr::null_mut();
        if unsafe { os2::DosAllocMem(&mut p, SIZE as u32, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | os2::OBJ_TILE) } != 0 {
            return f(0, core::ptr::null_mut());
        }
        *s = (p as usize, 0);
    }
    let (base, top) = *s;
    let n = (n + 3) & !3;
    if top + n > SIZE {
        return f(0, core::ptr::null_mut());
    }
    let p = (base + top) as *mut u8;
    unsafe { core::ptr::write_bytes(p, 0, n) };
    SCRATCH.get().1 = top + n;
    let r = f(thunk16::far(p), p);
    SCRATCH.get().1 = top;
    r
}

/// Run `f` with a 16:16 copy of a string (NUL-terminated).
pub fn with_str<R>(s: &[u8], f: impl FnOnce(u32) -> R) -> R {
    with(s.len() + 1, |far, p| {
        if !p.is_null() {
            unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len()) };
        }
        f(far)
    })
}
