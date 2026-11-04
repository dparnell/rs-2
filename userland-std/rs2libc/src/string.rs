//! Memory and string functions. The copies use `rep movsb`/`rep stosb`
//! (written in assembly: a Rust loop could be turned back into a call to
//! the very function being defined).

use core::arch::asm;
use core::ffi::{c_char, c_int, c_void};
use libc::size_t;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: size_t) -> *mut c_void {
    unsafe {
        asm!("xchg esi, {s}", "rep movsb", "mov esi, {s}",
             s = inout(reg) src => _, inout("ecx") n => _, inout("edi") dest => _,
             options(nostack, preserves_flags));
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: size_t) -> *mut c_void {
    if (dest as usize).wrapping_sub(src as usize) >= n {
        // No overlap a forward copy would trip on.
        return unsafe { memcpy(dest, src, n) };
    }
    unsafe {
        asm!("xchg esi, {s}", "std", "rep movsb", "cld", "mov esi, {s}",
             s = inout(reg) (src as *const u8).add(n).wrapping_sub(1) => _,
             inout("ecx") n => _,
             inout("edi") (dest as *mut u8).add(n).wrapping_sub(1) => _,
             options(nostack));
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut c_void, c: c_int, n: size_t) -> *mut c_void {
    unsafe {
        asm!("rep stosb", inout("ecx") n => _, inout("edi") dest => _, in("al") c as u8,
             options(nostack, preserves_flags));
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    let (a, b) = (a as *const u8, b as *const u8);
    let mut i = 0;
    while i < n {
        let (x, y) = unsafe { (core::ptr::read_volatile(a.add(i)), core::ptr::read_volatile(b.add(i))) };
        if x != y {
            return x as c_int - y as c_int;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    unsafe { memcmp(a, b, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    let s = s as *const u8;
    for i in 0..n {
        if unsafe { *s.add(i) } == c as u8 {
            return unsafe { s.add(i) as *mut c_void };
        }
    }
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const c_char) -> size_t {
    let mut n = 0;
    while unsafe { core::ptr::read_volatile(s.add(n)) } != 0 {
        n += 1;
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strnlen(s: *const c_char, max: size_t) -> size_t {
    let mut n = 0;
    while n < max && unsafe { core::ptr::read_volatile(s.add(n)) } != 0 {
        n += 1;
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcmp(a: *const c_char, b: *const c_char) -> c_int {
    let mut i = 0;
    loop {
        let (x, y) = unsafe { (*a.add(i) as u8, *b.add(i) as u8) };
        if x != y || x == 0 {
            return x as c_int - y as c_int;
        }
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncmp(a: *const c_char, b: *const c_char, n: size_t) -> c_int {
    for i in 0..n {
        let (x, y) = unsafe { (*a.add(i) as u8, *b.add(i) as u8) };
        if x != y || x == 0 {
            return x as c_int - y as c_int;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    let mut p = s;
    loop {
        let x = unsafe { *p };
        if x as u8 == c as u8 {
            return p as *mut c_char;
        }
        if x == 0 {
            return core::ptr::null_mut();
        }
        p = unsafe { p.add(1) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    let mut last = core::ptr::null_mut();
    let mut p = s;
    loop {
        let x = unsafe { *p };
        if x as u8 == c as u8 {
            last = p as *mut c_char;
        }
        if x == 0 {
            return last;
        }
        p = unsafe { p.add(1) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcpy(d: *mut c_char, s: *const c_char) -> *mut c_char {
    unsafe { memcpy(d as *mut c_void, s as *const c_void, strlen(s) + 1) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strdup(s: *const c_char) -> *mut c_char {
    unsafe {
        let n = strlen(s) + 1;
        let p = crate::malloc::malloc(n);
        if !p.is_null() {
            memcpy(p, s as *const c_void, n);
        }
        p as *mut c_char
    }
}

/// C strings from Rust: the bytes before the NUL.
pub unsafe fn cstr<'a>(s: *const c_char) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(s as *const u8, strlen(s)) }
}
