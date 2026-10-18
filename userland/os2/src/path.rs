//! Helpers for OS/2 strings and paths.

use alloc::string::String;
use alloc::vec::Vec;

/// A NUL-terminated copy of `s` for passing to the API.
pub fn cstr(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = crate::text_bytes(s).into_owned();
    v.push(0);
    v
}

/// Read a NUL-terminated string from a pointer.
///
/// # Safety
/// `p` must point to a NUL-terminated string.
pub unsafe fn from_cstr(p: *const u8) -> String {
    let mut s = String::new();
    let mut i = 0;
    loop {
        let b = unsafe { *p.add(i) };
        if b == 0 {
            break;
        }
        s.push(b as char);
        i += 1;
    }
    s
}

/// Bytes up to the first NUL as a String.
pub fn from_bytes(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}
