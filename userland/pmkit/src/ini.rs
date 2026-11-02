//! Settings in OS2.INI (the user profile).

use alloc::vec::Vec;

use crate::consts::*;
use crate::sys::*;
use crate::z;

pub fn read(app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let (a, k) = (z(app), z(key));
    let mut n = 0u32;
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, a.as_ptr(), k.as_ptr(), core::ptr::null_mut(), &mut n) } == 0 {
        // rs2's PMSHAPI answers the size with a null buffer; IBM's too.
        n = 4096;
    }
    let mut buf = alloc::vec![0u8; n.max(1) as usize];
    let mut got = buf.len() as u32;
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, a.as_ptr(), k.as_ptr(), buf.as_mut_ptr(), &mut got) } == 0 {
        return None;
    }
    buf.truncate(got as usize);
    Some(buf)
}

pub fn write(app: &[u8], key: &[u8], data: &[u8]) {
    let (a, k) = (z(app), z(key));
    unsafe { PrfWriteProfileData(HINI_USERPROFILE, a.as_ptr(), k.as_ptr(), data.as_ptr(), data.len() as u32) };
}

pub fn delete(app: &[u8], key: &[u8]) {
    let (a, k) = (z(app), z(key));
    unsafe { PrfWriteProfileData(HINI_USERPROFILE, a.as_ptr(), k.as_ptr(), core::ptr::null(), 0) };
}

/// A string (without its NUL).
pub fn read_str(app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    read(app, key).map(|v| crate::until_nul(&v).to_vec())
}

pub fn write_str(app: &[u8], key: &[u8], s: &[u8]) {
    write(app, key, &z(s));
}

/// A number kept as text (PrfQueryProfileInt).
pub fn read_int(app: &[u8], key: &[u8], default: i32) -> i32 {
    let (a, k) = (z(app), z(key));
    unsafe { PrfQueryProfileInt(HINI_USERPROFILE, a.as_ptr(), k.as_ptr(), default) }
}

pub fn write_int(app: &[u8], key: &[u8], v: i32) {
    write_str(app, key, alloc::format!("{v}").as_bytes());
}

/// A u32 kept as 4 binary bytes.
pub fn read_u32(app: &[u8], key: &[u8]) -> Option<u32> {
    read(app, key).filter(|v| v.len() >= 4).map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

pub fn write_u32(app: &[u8], key: &[u8], v: u32) {
    write(app, key, &v.to_le_bytes());
}

/// A plain structure kept as its bytes; true when one of the right size
/// was there.
pub fn read_struct<T: Copy>(app: &[u8], key: &[u8], out: &mut T) -> bool {
    let size = core::mem::size_of::<T>();
    match read(app, key) {
        Some(v) if v.len() == size => {
            unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), out as *mut T as *mut u8, size) };
            true
        }
        _ => false,
    }
}

pub fn write_struct<T: Copy>(app: &[u8], key: &[u8], v: &T) {
    let b = unsafe { core::slice::from_raw_parts(v as *const T as *const u8, core::mem::size_of::<T>()) };
    write(app, key, b);
}
