//! The interface repository file's encoding, and SOMIR's exported helpers
//! for it (getIntFromBuf, putTypeCodeInBuf, ...): big-endian 32-bit
//! integers; strings as their length (-1 for none) and their bytes with a
//! NUL; TypeCodes as kind (-1 for none), alignment, two reserved
//! integers, then the kind's parameters (see docs/formats/som-ir.md).

use alloc::vec::Vec;

use somsup::aligned;

use crate::tc;

pub fn get_int(p: *const u8) -> i32 {
    unsafe { i32::from_be_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]) }
}

pub fn put_int(p: *mut u8, v: i32) -> *mut u8 {
    unsafe {
        core::ptr::copy_nonoverlapping(v.to_be_bytes().as_ptr(), p, 4);
        p.add(4)
    }
}

/// A string in a buffer: its address (null for none) and what follows.
pub fn get_str(p: *const u8) -> (*const u8, *const u8) {
    let n = get_int(p);
    unsafe {
        if n == -1 { (core::ptr::null(), p.add(5)) } else { (p.add(4), p.add(4 + n as usize + 1)) }
    }
}

pub fn str_size(s: *const u8) -> usize {
    if s.is_null() { 5 } else { (unsafe { somsup::strlen(s) }) + 5 }
}

pub fn put_str(p: *mut u8, s: *const u8) -> *mut u8 {
    unsafe {
        if s.is_null() {
            let q = put_int(p, -1);
            *q = 0;
            return q.add(1);
        }
        let n = somsup::strlen(s);
        let q = put_int(p, n as i32);
        core::ptr::copy_nonoverlapping(s, q, n + 1);
        q.add(n + 1)
    }
}

/// A TypeCode from a buffer (made with tcNew; null for none).
pub fn get_tc(p: *const u8) -> (usize, *const u8) {
    let k = get_int(p);
    if k == -1 {
        return (0, unsafe { p.add(4) });
    }
    let align = get_int(unsafe { p.add(4) });
    let mut p = unsafe { p.add(16) };
    let mut args: Vec<usize> = Vec::new();
    let mut owned: Vec<usize> = Vec::new();
    let k = k as u32;
    match k {
        tc::TK_OBJREF | tc::TK_SELF => {
            let (s, q) = get_str(p);
            args.push(s as usize);
            p = q;
        }
        tc::TK_STRUCT | tc::TK_ENUM => {
            let (name, q) = get_str(p);
            args.push(name as usize);
            let n = get_int(q);
            p = unsafe { q.add(4) };
            for _ in 0..n.max(0) {
                let (m, q) = get_str(p);
                args.push(m as usize);
                p = q;
                if k == tc::TK_STRUCT {
                    let (t, q) = get_tc(p);
                    args.push(t);
                    owned.push(t);
                    p = q;
                }
            }
            args.push(0);
        }
        tc::TK_UNION => {
            let (name, q) = get_str(p);
            args.push(name as usize);
            let (sw, q) = get_tc(q);
            args.push(sw);
            let n = get_int(q);
            p = unsafe { q.add(4) };
            for _ in 0..n.max(0) {
                let flag = get_int(p);
                p = unsafe { p.add(4) };
                let label = if flag == 1 {
                    let l = get_int(p);
                    p = unsafe { p.add(4) };
                    l
                } else {
                    0
                };
                args.push(flag as usize);
                args.push(label as usize);
                let (m, q) = get_str(p);
                args.push(m as usize);
                let (t, q) = get_tc(q);
                args.push(t);
                p = q;
            }
            args.push(0);
        }
        tc::TK_STRING => {
            args.push(get_int(p) as usize);
            p = unsafe { p.add(4) };
        }
        tc::TK_SEQUENCE | tc::TK_ARRAY => {
            let (t, q) = get_tc(p);
            args.push(t);
            args.push(get_int(q) as usize);
            p = unsafe { q.add(4) };
        }
        tc::TK_POINTER => {
            let (t, q) = get_tc(p);
            args.push(t);
            p = q;
        }
        tc::TK_FOREIGN => {
            let (a, q) = get_str(p);
            let (b, q) = get_str(q);
            args.push(a as usize);
            args.push(b as usize);
            args.push(get_int(q) as usize);
            p = unsafe { q.add(4) };
        }
        _ => {}
    }
    args.push(0);
    let t = unsafe { tc::tcNewVL(k, args.as_ptr()) };
    if t != 0 && align != 0 {
        unsafe { tc::tcSetAlignment(t, core::ptr::null_mut(), align as i16 as i32) };
    }
    let _ = owned;
    (t, p)
}

/// A TypeCode's encoding (read from its layout, as SOMTC keeps it).
pub fn put_tc(out: &mut Vec<u8>, t: usize) {
    let w = |i: usize| unsafe { *(t as *const u32).add(i) };
    let s = |out: &mut Vec<u8>, p: u32| {
        let p = p as *const u8;
        if p.is_null() {
            out.extend_from_slice(&(-1i32).to_be_bytes());
            out.push(0);
        } else {
            let b = unsafe { somsup::cstr(p) };
            out.extend_from_slice(&(b.len() as i32).to_be_bytes());
            out.extend_from_slice(b);
            out.push(0);
        }
    };
    let i = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_be_bytes());
    if t == 0 {
        i(out, u32::MAX);
        return;
    }
    let k = w(0);
    let explicit = unsafe { *(t as *const u8).add(7) } & 2 != 0;
    i(out, k);
    i(out, if explicit { (unsafe { *(t as *const i16).add(2) }) as i32 as u32 } else { 0 });
    i(out, 0);
    i(out, 0);
    match k {
        tc::TK_OBJREF | tc::TK_SELF => s(out, w(2)),
        tc::TK_STRUCT => {
            s(out, w(3));
            i(out, w(2));
            for m in 0..w(2) as usize {
                let e = unsafe { (w(4) as *const u32).add(2 * m) };
                s(out, unsafe { *e });
                put_tc(out, unsafe { *e.add(1) } as usize);
            }
        }
        tc::TK_ENUM => {
            s(out, w(3));
            i(out, w(2));
            for m in 0..w(2) as usize {
                s(out, unsafe { *(w(4) as *const u32).add(m) });
            }
        }
        tc::TK_UNION => {
            s(out, w(3));
            put_tc(out, w(4) as usize);
            i(out, w(2));
            for m in 0..w(2) as usize {
                let e = unsafe { (w(5) as *const u32).add(4 * m) };
                let flag = unsafe { *e };
                i(out, flag);
                if flag == 1 {
                    i(out, unsafe { *e.add(1) });
                }
                s(out, unsafe { *e.add(2) });
                put_tc(out, unsafe { *e.add(3) } as usize);
            }
        }
        tc::TK_STRING => i(out, w(2)),
        tc::TK_SEQUENCE | tc::TK_ARRAY => {
            put_tc(out, w(2) as usize);
            i(out, w(3));
        }
        tc::TK_POINTER => put_tc(out, w(2) as usize),
        tc::TK_FOREIGN => {
            s(out, w(2));
            s(out, w(3));
            i(out, w(4));
        }
        _ => {}
    }
}

pub fn skip_tc(p: *const u8) -> *const u8 {
    let (t, q) = get_tc(p);
    if t != 0 {
        unsafe { tc::tcFree(t, core::ptr::null_mut()) };
    }
    q
}

// ---- The exported helpers (_System) ----

extern "C" fn get_int_from_buf(p: *const u8, out: *mut i32) -> *const u8 {
    unsafe { *out = get_int(p) };
    unsafe { p.add(4) }
}
extern "C" fn put_int_in_buf(p: *mut u8, v: i32) -> *mut u8 {
    put_int(p, v)
}
extern "C" fn skip_int_in_buf(p: *const u8) -> *const u8 {
    unsafe { p.add(4) }
}
extern "C" fn get_string_from_buf(p: *const u8, out: *mut *const u8) -> *const u8 {
    let (s, q) = get_str(p);
    unsafe { *out = s };
    q
}
extern "C" fn skip_string_in_buf(p: *const u8) -> *const u8 {
    get_str(p).1
}
extern "C" fn get_string_length_from_buf(p: *const u8) -> u32 {
    let n = get_int(p);
    if n == -1 { 0 } else { n as u32 }
}
/// copyStringFromBuf(buf, &str, &cursor): the string copied to *cursor.
extern "C" fn copy_string_from_buf(p: *const u8, out: *mut *mut u8, cursor: *mut *mut u8) -> *const u8 {
    let (s, q) = get_str(p);
    unsafe {
        if s.is_null() {
            *out = core::ptr::null_mut();
        } else {
            let n = somsup::strlen(s) + 1;
            core::ptr::copy_nonoverlapping(s, *cursor, n);
            *out = *cursor;
            *cursor = (*cursor).add(n);
        }
    }
    q
}
extern "C" fn put_string_in_buf(p: *mut u8, s: *const u8) -> *mut u8 {
    put_str(p, s)
}
extern "C" fn get_string_buf_size(s: *const u8) -> u32 {
    str_size(s) as u32
}
extern "C" fn get_type_code_from_buf(p: *const u8, out: *mut usize) -> *const u8 {
    let (t, q) = get_tc(p);
    unsafe { *out = t };
    q
}
extern "C" fn skip_type_code_in_buf(p: *const u8) -> *const u8 {
    skip_tc(p)
}
extern "C" fn put_type_code_in_buf(p: *mut u8, t: usize) -> *mut u8 {
    let mut v = Vec::new();
    put_tc(&mut v, t);
    unsafe {
        core::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len());
        p.add(v.len())
    }
}
extern "C" fn get_type_code_buf_size(t: usize) -> u32 {
    let mut v = Vec::new();
    put_tc(&mut v, t);
    v.len() as u32
}

aligned! {
    __a_getIntFromBuf => get_int_from_buf, 2;
    __a_putIntInBuf => put_int_in_buf, 2;
    __a_skipIntInBuf => skip_int_in_buf, 1;
    __a_getStringFromBuf => get_string_from_buf, 2;
    __a_skipStringInBuf => skip_string_in_buf, 1;
    __a_getStringLengthFromBuf => get_string_length_from_buf, 1;
    __a_copyStringFromBuf => copy_string_from_buf, 3;
    __a_putStringInBuf => put_string_in_buf, 2;
    __a_getStringBufSize => get_string_buf_size, 1;
    __a_getTypeCodeFromBuf => get_type_code_from_buf, 2;
    __a_skipTypeCodeInBuf => skip_type_code_in_buf, 1;
    __a_putTypeCodeInBuf => put_type_code_in_buf, 2;
    __a_getTypeCodeBufSize => get_type_code_buf_size, 1;
}
