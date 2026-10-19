//! C strings in caller memory.

use alloc::vec::Vec;

/// Length of a NUL-terminated string (0 for NULL).
pub unsafe fn strlen(p: *const u8) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

/// The bytes of a C string (empty for NULL).
pub unsafe fn bytes<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(p, strlen(p)) }
}

/// A NUL-terminated copy.
pub fn z(b: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(b.len() + 1);
    v.extend_from_slice(b);
    v.push(0);
    v
}

/// Copy `s` to `dst` (room for `cap` bytes including the NUL),
/// truncating; returns false if it did not fit.
pub unsafe fn copy_out(s: &[u8], dst: *mut u8, cap: usize) -> bool {
    if dst.is_null() || cap == 0 {
        return false;
    }
    let n = s.len().min(cap - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, n);
        *dst.add(n) = 0;
    }
    n == s.len()
}

pub fn eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}
