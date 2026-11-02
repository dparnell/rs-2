//! SOMTC.DLL: SOM 2.1's TypeCodes as OS/2 Warp 4.52 has them (studied in
//! IBM's SOMTC.DLL with Ghidra), binary compatible with it: IBM's
//! ordinals, the TypeCode layout code compiled against the Toolkit's
//! somtc.h and IBM's interface repository (SOMIR) rely on, the TypeCode
//! constants (`TC__long`, the repository's descriptions), and IBM's
//! replaceable allocator variables (TCMalloc, ...).
//!
//! A TypeCode is `{ kind, alignment: i16, 1: u8, flags: u8, params... }`
//! (flags: 1 = allocated, tcFree frees it; 2 = alignment given). The
//! parameters by kind:
//!
//! | kind | parameters |
//! |------|------------|
//! | tk_objref | interface name |
//! | tk_struct | count, name, members `{ name, TypeCode }` |
//! | tk_union | count, name, switch TypeCode, members `{ flag, label, name, TypeCode }` |
//! | tk_enum | count, name, names |
//! | tk_string | maximum length |
//! | tk_sequence | element TypeCode, maximum |
//! | tk_array | element TypeCode, length |
//! | tk_pointer | TypeCode |
//! | tk_self | name |
//! | tk_foreign | name, implementation context, instance size |
//!
//! tcNew makes a TypeCode in one block (its strings after it); the
//! TypeCodes it is given become part of it (tcFree frees them too).
//! tcCopy copies a TypeCode and everything it refers to into one block,
//! of which only the outer TypeCode is marked to be freed.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, function_casts_as_integer)]

extern crate alloc;
extern crate os2;

pub mod data;

use data::*;
use somsup::{Environment, USER_EXCEPTION, aligned, cstr, strlen};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, _term: u32) -> u32 {
    1
}

// ---- The replaceable allocator (SOM's by default) ----

extern "C" fn tc_malloc(n: u32) -> *mut u8 {
    somsup::malloc(n as usize)
}
extern "C" fn tc_calloc(count: u32, size: u32) -> *mut u8 {
    unsafe { (somsup::SOMCalloc)(count, size) }
}
extern "C" fn tc_realloc(p: *mut u8, n: u32) -> *mut u8 {
    somsup::realloc(p, n as usize)
}
extern "C" fn tc_free(p: *mut u8) {
    somsup::free(p)
}

aligned! {
    __a_tc_malloc => tc_malloc, 1;
    __a_tc_calloc => tc_calloc, 2;
    __a_tc_realloc => tc_realloc, 2;
    __a_tc_free => tc_free, 1;
}

#[unsafe(no_mangle)]
pub static mut TCMalloc: extern "C" fn() = __a_tc_malloc;
#[unsafe(no_mangle)]
pub static mut TCCalloc: extern "C" fn() = __a_tc_calloc;
#[unsafe(no_mangle)]
pub static mut TCRealloc: extern "C" fn() = __a_tc_realloc;
#[unsafe(no_mangle)]
pub static mut TCFree: extern "C" fn() = __a_tc_free;

fn alloc(n: usize) -> *mut u32 {
    let f: extern "C" fn(u32) -> *mut u8 = unsafe { core::mem::transmute(TCMalloc) };
    f(n as u32) as *mut u32
}
fn calloc(count: usize, size: usize) -> *mut u8 {
    let f: extern "C" fn(u32, u32) -> *mut u8 = unsafe { core::mem::transmute(TCCalloc) };
    f(count as u32, size as u32)
}
fn dealloc(p: *mut u8) {
    let f: extern "C" fn(*mut u8) = unsafe { core::mem::transmute(TCFree) };
    f(p)
}

// ---- TypeCodes ----

pub type Tc = *mut u32;

pub const TK_NULL: u32 = 1;
pub const TK_VOID: u32 = 2;
pub const TK_SHORT: u32 = 3;
pub const TK_LONG: u32 = 4;
pub const TK_USHORT: u32 = 5;
pub const TK_ULONG: u32 = 6;
pub const TK_FLOAT: u32 = 7;
pub const TK_DOUBLE: u32 = 8;
pub const TK_BOOLEAN: u32 = 9;
pub const TK_CHAR: u32 = 10;
pub const TK_OCTET: u32 = 11;
pub const TK_ANY: u32 = 12;
pub const TK_TYPECODE: u32 = 13;
pub const TK_PRINCIPAL: u32 = 14;
pub const TK_OBJREF: u32 = 15;
pub const TK_STRUCT: u32 = 16;
pub const TK_UNION: u32 = 17;
pub const TK_ENUM: u32 = 18;
pub const TK_STRING: u32 = 19;
pub const TK_SEQUENCE: u32 = 20;
pub const TK_ARRAY: u32 = 21;
pub const TK_POINTER: u32 = 101;
pub const TK_SELF: u32 = 102;
pub const TK_FOREIGN: u32 = 103;

const DYNAMIC: u8 = 1;
const ALIGNED: u8 = 2;

unsafe fn w(t: Tc, i: usize) -> u32 {
    unsafe { *t.add(i) }
}
unsafe fn set(t: Tc, i: usize, v: u32) {
    unsafe { *t.add(i) = v }
}
unsafe fn tp(t: Tc, i: usize) -> Tc {
    unsafe { *t.add(i) as Tc }
}
unsafe fn flags(t: Tc) -> u8 {
    unsafe { *(t as *const u8).add(7) }
}
unsafe fn set_flags(t: Tc, f: u8) {
    unsafe { *(t as *mut u8).add(7) = f }
}
unsafe fn align_field(t: Tc) -> i16 {
    unsafe { *(t as *const i16).add(2) }
}
unsafe fn set_align_field(t: Tc, a: i16) {
    unsafe { *(t as *mut i16).add(2) = a }
}
/// Struct member i: (name, TypeCode).
unsafe fn smember(t: Tc, i: usize) -> *mut u32 {
    unsafe { (w(t, 4) as *mut u32).add(2 * i) }
}
/// Union member i: (flag, label, name, TypeCode).
unsafe fn umember(t: Tc, i: usize) -> *mut u32 {
    unsafe { (w(t, 5) as *mut u32).add(4 * i) }
}
unsafe fn sz(p: u32) -> usize {
    unsafe { strlen(p as *const u8) }
}

fn pad(n: i32, a: i16) -> i32 {
    if a < 2 {
        return 0;
    }
    let r = n % a as i32;
    if r == 0 { 0 } else { a as i32 - r }
}

pub fn kind(t: Tc) -> u32 {
    if t.is_null() { TK_NULL } else { unsafe { *t } }
}

/// tcKind: the kind (tk_null for no TypeCode).
pub extern "C" fn tcKind(t: Tc, _ev: *mut Environment) -> u32 {
    kind(t)
}

/// The alignment a TypeCode has without an explicit one (IBM's rules for
/// VisualAge C++'s default packing: double 4).
pub fn default_alignment(t: Tc) -> i16 {
    unsafe {
        match kind(t) {
            TK_SHORT | TK_USHORT => 2,
            TK_LONG | TK_ULONG | TK_FLOAT | TK_DOUBLE => 4,
            TK_BOOLEAN | TK_CHAR | TK_OCTET => 1,
            TK_ANY | TK_TYPECODE | TK_PRINCIPAL | TK_OBJREF => 4,
            TK_STRUCT => (0..w(t, 2) as usize).map(|i| default_alignment(*smember(t, i).add(1) as Tc)).max().unwrap_or(0).max(0),
            TK_UNION => {
                let a = default_alignment(tp(t, 4));
                (0..w(t, 2) as usize).map(|i| default_alignment(*umember(t, i).add(3) as Tc)).fold(a, i16::max)
            }
            TK_ENUM | TK_STRING | TK_SEQUENCE => 4,
            TK_ARRAY => default_alignment(tp(t, 2)),
            TK_POINTER | TK_SELF | TK_FOREIGN => 4,
            _ => 0,
        }
    }
}

pub fn alignment(t: Tc) -> i16 {
    if t.is_null() {
        return 0;
    }
    if unsafe { flags(t) } & ALIGNED == 0 { default_alignment(t) } else { unsafe { align_field(t) } }
}

/// tcAlignment: the explicit alignment, else the default one.
pub extern "C" fn tcAlignment(t: Tc, _ev: *mut Environment) -> i32 {
    alignment(t) as i32
}

/// The size of a value of the type (as IBM's: a union's is the switch,
/// padded to the members' alignment, and its largest member).
pub fn size(t: Tc) -> i32 {
    if t.is_null() {
        return 0;
    }
    unsafe {
        match kind(t) {
            0 | TK_TYPECODE | TK_PRINCIPAL | TK_OBJREF | TK_STRING => 4,
            TK_NULL | TK_VOID => 0,
            TK_SHORT | TK_USHORT => 2,
            TK_LONG | TK_ULONG | TK_FLOAT => 4,
            TK_DOUBLE | TK_ANY => 8,
            TK_BOOLEAN | TK_CHAR | TK_OCTET => 1,
            TK_STRUCT => {
                let n = w(t, 2) as usize;
                if n == 0 {
                    return 0;
                }
                let mut s = size(*smember(t, 0).add(1) as Tc);
                if n == 1 {
                    return s;
                }
                for i in 1..n {
                    let m = *smember(t, i).add(1) as Tc;
                    s += pad(s, alignment(m)) + size(m);
                }
                s + pad(s, alignment(t))
            }
            TK_UNION => {
                let sw = size(tp(t, 4));
                let (mut most, mut al) = (0, 0i16);
                for i in 0..w(t, 2) as usize {
                    let m = *umember(t, i).add(3) as Tc;
                    most = most.max(size(m));
                    al = al.max(alignment(m));
                }
                sw + pad(sw, al) + most
            }
            TK_ENUM => 4,
            TK_SEQUENCE => 12,
            TK_ARRAY => {
                let e = tp(t, 2);
                let s = size(e);
                (s + pad(s, alignment(e))) * w(t, 3) as i32
            }
            TK_FOREIGN => w(t, 4) as i32,
            _ => 4,
        }
    }
}

pub extern "C" fn tcSize(t: Tc, _ev: *mut Environment) -> i32 {
    size(t)
}

pub extern "C" fn tcParmCount(t: Tc, _ev: *mut Environment) -> i32 {
    if t.is_null() {
        return 0;
    }
    unsafe {
        match kind(t) {
            TK_OBJREF | TK_STRING | TK_POINTER | TK_SELF => 1,
            TK_STRUCT => w(t, 2) as i32 * 2 + 1,
            TK_UNION => w(t, 2) as i32 * 3 + 2,
            TK_ENUM => w(t, 2) as i32 + 1,
            TK_SEQUENCE | TK_ARRAY => 2,
            TK_FOREIGN => 3,
            _ => 0,
        }
    }
}

#[repr(C)]
pub struct Any {
    pub tc: *const u8,
    pub value: *const u8,
}

fn bounds(ev: *mut Environment) {
    somsup::set_exception(ev, USER_EXCEPTION, b"Bounds\0", core::ptr::null_mut());
}

/// tcParameter: an `any` (returned through the hidden pointer `ret`) for
/// parameter `i`, pointing into the TypeCode; tk_null and a Bounds
/// exception when there is no such parameter.
pub extern "C" fn tcParameter(ret: *mut Any, t: Tc, ev: *mut Environment, i: i32) -> *mut Any {
    let string = &TC__string as *const _ as *const u8;
    let typecode = &TC__TypeCode as *const _ as *const u8;
    let long = &TC__long as *const _ as *const u8;
    let r = |tc: *const u8, v: *const u32| unsafe {
        (*ret).tc = tc;
        (*ret).value = v as *const u8;
        ret
    };
    unsafe {
        if !t.is_null() {
            match kind(t) {
                TK_OBJREF | TK_SELF if i < 1 => return r(string, t.add(2)),
                TK_STRING if i < 1 => return r(long, t.add(2)),
                TK_POINTER if i < 1 => return r(typecode, t.add(2)),
                TK_SEQUENCE | TK_ARRAY if i < 2 => return if i == 0 { r(typecode, t.add(2)) } else { r(long, t.add(3)) },
                TK_FOREIGN if i < 3 => return if i < 2 { r(string, t.add(2 + i as usize)) } else { r(long, t.add(4)) },
                TK_STRUCT if i < w(t, 2) as i32 * 2 + 1 => {
                    if i == 0 {
                        return r(string, t.add(3));
                    }
                    let (m, odd) = (((i - 1) / 2) as usize, (i - 1) % 2 == 1);
                    return if odd { r(typecode, smember(t, m).add(1)) } else { r(string, smember(t, m)) };
                }
                TK_UNION if i < w(t, 2) as i32 * 3 + 2 => {
                    if i == 0 {
                        return r(string, t.add(3));
                    }
                    if i == 1 {
                        return r(typecode, t.add(4));
                    }
                    let (m, k) = (((i - 2) / 3) as usize, (i - 2) % 3);
                    let mb = umember(t, m);
                    return match k {
                        0 if *mb == 2 => r(&TC__octet as *const _ as *const u8, &ZERO_OCTET),
                        0 => {
                            let lt: *const u8 = match kind(tp(t, 4)) {
                                TK_SHORT => &TC__short as *const _ as *const u8,
                                TK_USHORT => &TC__ushort as *const _ as *const u8,
                                TK_ULONG => &TC__ulong as *const _ as *const u8,
                                TK_BOOLEAN => &TC__boolean as *const _ as *const u8,
                                TK_CHAR => &TC__char as *const _ as *const u8,
                                _ => long,
                            };
                            r(lt, mb.add(1))
                        }
                        1 => r(string, mb.add(2)),
                        _ => r(typecode, mb.add(3)),
                    };
                }
                TK_ENUM if i <= w(t, 2) as i32 => {
                    return if i == 0 { r(string, t.add(3)) } else { r(string, (w(t, 4) as *const u32).add(i as usize - 1)) };
                }
                _ => {}
            }
        }
        bounds(ev);
        r(&TC__null as *const _ as *const u8, core::ptr::null())
    }
}

fn str_eq(a: u32, b: u32) -> bool {
    unsafe { cstr(a as *const u8) == cstr(b as *const u8) }
}

pub fn equal(mut x: Tc, mut y: Tc) -> bool {
    unsafe {
        loop {
            if x.is_null() && y.is_null() {
                return true;
            }
            if (x.is_null() && *y == TK_NULL) || (y.is_null() && *x == TK_NULL) {
                return true;
            }
            if x.is_null() || y.is_null() || *x != *y || alignment(x) != alignment(y) {
                return false;
            }
            match *x {
                TK_OBJREF | TK_SELF => return str_eq(w(x, 2), w(y, 2)),
                TK_STRUCT => {
                    if w(x, 2) != w(y, 2) || !str_eq(w(x, 3), w(y, 3)) {
                        return false;
                    }
                    return (0..w(x, 2) as usize).all(|i| {
                        let (a, b) = (smember(x, i), smember(y, i));
                        str_eq(*a, *b) && equal(*a.add(1) as Tc, *b.add(1) as Tc)
                    });
                }
                TK_UNION => {
                    if w(x, 2) != w(y, 2) || !str_eq(w(x, 3), w(y, 3)) || !equal(tp(x, 4), tp(y, 4)) {
                        return false;
                    }
                    return (0..w(x, 2) as usize).all(|i| {
                        let (a, b) = (umember(x, i), umember(y, i));
                        *a == *b && *a.add(1) == *b.add(1) && str_eq(*a.add(2), *b.add(2)) && equal(*a.add(3) as Tc, *b.add(3) as Tc)
                    });
                }
                TK_ENUM => {
                    if w(x, 2) != w(y, 2) || !str_eq(w(x, 3), w(y, 3)) {
                        return false;
                    }
                    return (0..w(x, 2) as usize).all(|i| str_eq(*(w(x, 4) as *const u32).add(i), *(w(y, 4) as *const u32).add(i)));
                }
                TK_STRING => return w(x, 2) == w(y, 2),
                TK_SEQUENCE | TK_ARRAY => return w(x, 3) == w(y, 3) && equal(tp(x, 2), tp(y, 2)),
                TK_POINTER => {
                    x = tp(x, 2);
                    y = tp(y, 2);
                }
                TK_FOREIGN => return w(x, 4) == w(y, 4) && str_eq(w(x, 2), w(y, 2)) && str_eq(w(x, 3), w(y, 3)),
                _ => return true,
            }
        }
    }
}

pub extern "C" fn tcEqual(x: Tc, _ev: *mut Environment, y: Tc) -> u8 {
    equal(x, y) as u8
}

// ---- Making TypeCodes ----

/// Copy a string to `*at` (advancing it); its new address.
unsafe fn put_str(s: u32, at: &mut usize) -> u32 {
    let p = *at;
    if s != 0 {
        let n = unsafe { sz(s) } + 1;
        unsafe { core::ptr::copy_nonoverlapping(s as *const u8, p as *mut u8, n) };
        *at += n;
    }
    p as u32
}

fn header(t: Tc, k: u32) {
    unsafe {
        set(t, 0, k);
        set_align_field(t, 0);
        *(t as *mut u8).add(6) = 1;
        set_flags(t, DYNAMIC);
    }
}

/// Fix a new aggregate's alignment when one of its parts has an
/// explicit one.
unsafe fn inherit_alignment(t: Tc, parts: &[Tc]) {
    if !parts.iter().any(|&p| !p.is_null() && unsafe { flags(p) } & ALIGNED != 0) {
        return;
    }
    unsafe {
        set_align_field(t, default_alignment(t));
        for &p in parts {
            let a = alignment(p);
            if align_field(t) < a {
                set_align_field(t, a);
                set_flags(t, flags(t) | ALIGNED);
            }
        }
        if flags(t) & ALIGNED == 0 {
            set_align_field(t, 0);
        }
    }
}

/// tcNewVL: a new TypeCode of kind `k` from the parameters at `ap`
/// (somtc.h's tcNew conventions); NULL for parameters that make none.
pub extern "C" fn tcNewVL(k: u32, ap: *const u32) -> Tc {
    unsafe {
        let a = |i: usize| *ap.add(i);
        match k {
            TK_OBJREF | TK_SELF => {
                let t = alloc(sz(a(0)) + 1 + 0xc);
                if t.is_null() {
                    return t;
                }
                header(t, k);
                let mut at = t.add(3) as usize;
                set(t, 2, put_str(a(0), &mut at));
                t
            }
            TK_STRUCT => {
                let mut n = 0;
                while a(1 + 2 * n) != 0 {
                    if *(a(1 + 2 * n) as *const u8) == 0 {
                        return core::ptr::null_mut();
                    }
                    n += 1;
                }
                if n == 0 {
                    return core::ptr::null_mut();
                }
                let strings: usize = sz(a(0)) + 1 + (0..n).map(|i| sz(a(1 + 2 * i)) + 1).sum::<usize>();
                let t = alloc(0x14 + n * 8 + strings);
                if t.is_null() {
                    return t;
                }
                header(t, k);
                set(t, 2, n as u32);
                let mut at = t.add(5 + 2 * n) as usize;
                set(t, 3, put_str(a(0), &mut at));
                set(t, 4, t.add(5) as u32);
                let mut parts = alloc::vec::Vec::new();
                for i in 0..n {
                    let m = smember(t, i);
                    *m = put_str(a(1 + 2 * i), &mut at);
                    *m.add(1) = a(2 + 2 * i);
                    parts.push(a(2 + 2 * i) as Tc);
                }
                inherit_alignment(t, &parts);
                t
            }
            TK_UNION => {
                let sw = a(1) as Tc;
                if !matches!(kind(sw), TK_BOOLEAN | TK_CHAR | TK_ENUM | TK_SHORT | TK_USHORT | TK_LONG | TK_ULONG) || sw.is_null() {
                    return core::ptr::null_mut();
                }
                let mut n = 0;
                while a(2 + 4 * n) != 0 {
                    if *(a(4 + 4 * n) as *const u8) == 0 {
                        return core::ptr::null_mut();
                    }
                    n += 1;
                }
                if n == 0 {
                    return core::ptr::null_mut();
                }
                let strings: usize = sz(a(0)) + 1 + (0..n).map(|i| sz(a(4 + 4 * i)) + 1).sum::<usize>();
                let t = alloc(0x18 + n * 16 + strings);
                if t.is_null() {
                    return t;
                }
                header(t, k);
                set(t, 2, n as u32);
                let mut at = t.add(6 + 4 * n) as usize;
                set(t, 3, put_str(a(0), &mut at));
                set(t, 4, sw as u32);
                set(t, 5, t.add(6) as u32);
                let mut parts = alloc::vec![sw];
                for i in 0..n {
                    let m = umember(t, i);
                    *m = a(2 + 4 * i);
                    *m.add(1) = 0;
                    let label = a(3 + 4 * i);
                    match *sw {
                        TK_SHORT | TK_USHORT => *(m.add(1) as *mut u16) = label as u16,
                        TK_BOOLEAN | TK_CHAR => *(m.add(1) as *mut u8) = label as u8,
                        _ => *m.add(1) = label,
                    }
                    *m.add(2) = put_str(a(4 + 4 * i), &mut at);
                    *m.add(3) = a(5 + 4 * i);
                    parts.push(a(5 + 4 * i) as Tc);
                }
                inherit_alignment(t, &parts);
                t
            }
            TK_ENUM => {
                let mut n = 0;
                while a(1 + n) != 0 {
                    if *(a(1 + n) as *const u8) == 0 {
                        return core::ptr::null_mut();
                    }
                    n += 1;
                }
                if n == 0 {
                    return core::ptr::null_mut();
                }
                let strings: usize = sz(a(0)) + 1 + (0..n).map(|i| sz(a(1 + i)) + 1).sum::<usize>();
                let t = alloc(0x14 + n * 4 + strings);
                if t.is_null() {
                    return t;
                }
                header(t, k);
                set(t, 2, n as u32);
                let mut at = t.add(5 + n) as usize;
                set(t, 3, put_str(a(0), &mut at));
                set(t, 4, t.add(5) as u32);
                for i in 0..n {
                    *t.add(5 + i) = put_str(a(1 + i), &mut at);
                }
                t
            }
            TK_STRING => {
                let t = alloc(0xc);
                if !t.is_null() {
                    header(t, k);
                    set(t, 2, a(0));
                }
                t
            }
            TK_SEQUENCE | TK_ARRAY => {
                let t = alloc(0x10);
                if !t.is_null() {
                    header(t, k);
                    set(t, 2, a(0));
                    set(t, 3, a(1));
                    if k == TK_ARRAY && !tp(t, 2).is_null() {
                        inherit_alignment(t, &[tp(t, 2)]);
                    }
                }
                t
            }
            TK_POINTER => {
                let t = alloc(0xc);
                if !t.is_null() {
                    header(t, k);
                    set(t, 2, a(0));
                }
                t
            }
            TK_FOREIGN => {
                let t = alloc(0x14 + sz(a(0)) + 1 + sz(a(1)) + 1);
                if t.is_null() {
                    return t;
                }
                header(t, k);
                let mut at = t.add(5) as usize;
                set(t, 2, put_str(a(0), &mut at));
                set(t, 3, put_str(a(1), &mut at));
                set(t, 4, a(2));
                t
            }
            _ => {
                let t = alloc(8);
                if !t.is_null() {
                    header(t, k);
                }
                t
            }
        }
    }
}

/// tcNew(kind, ...): tcNewVL with the arguments after the kind.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_tcNew() {
    core::arch::naked_asm!(
        "lea eax, [esp + 8]",
        "push eax",
        "push dword ptr [esp + 8]",
        "call {f}",
        "add esp, 8",
        "ret",
        f = sym __a_tcNewVL,
    );
}

/// The alignment applied to a TypeCode and its parts (copied first when
/// it is a constant); IBM's setAlignment.
unsafe fn set_alignment(t: Tc, a: i16) -> Tc {
    unsafe {
        if a == 0 && flags(t) & DYNAMIC == 0 {
            return t;
        }
        if a == alignment(t) {
            return t;
        }
        let c = if flags(t) & DYNAMIC == 0 { copy(t) } else { t };
        if c.is_null() {
            return t;
        }
        if a == 0 {
            set_align_field(c, 0);
            set_flags(c, flags(c) & !ALIGNED);
        } else {
            set_flags(c, flags(c) | ALIGNED);
            if align_field(c) < a && align_field(c) > 0 {
                set_align_field(c, a);
                return c;
            }
            set_align_field(c, a);
        }
        match *t {
            TK_STRUCT => {
                for i in 0..w(c, 2) as usize {
                    let m = smember(c, i).add(1);
                    *m = set_alignment(*m as Tc, a) as u32;
                }
            }
            TK_UNION => {
                set(c, 4, set_alignment(tp(c, 4), a) as u32);
                for i in 0..w(c, 2) as usize {
                    let m = umember(c, i).add(3);
                    *m = set_alignment(*m as Tc, a) as u32;
                }
            }
            TK_ARRAY => set(c, 2, set_alignment(tp(c, 2), a) as u32),
            _ => {}
        }
        c
    }
}

/// tcSetAlignment: only TypeCodes made at run time change.
pub extern "C" fn tcSetAlignment(t: Tc, _ev: *mut Environment, a: i16) {
    if !t.is_null() && unsafe { flags(t) } & DYNAMIC != 0 {
        unsafe { set_alignment(t, a) };
    }
}

pub fn free(t: Tc) {
    unsafe {
        if t.is_null() || flags(t) & DYNAMIC == 0 {
            return;
        }
        match *t {
            TK_STRUCT => (0..w(t, 2) as usize).for_each(|i| free(*smember(t, i).add(1) as Tc)),
            TK_UNION => {
                free(tp(t, 4));
                (0..w(t, 2) as usize).for_each(|i| free(*umember(t, i).add(3) as Tc));
            }
            TK_SEQUENCE | TK_ARRAY | TK_POINTER => free(tp(t, 2)),
            _ => {}
        }
        dealloc(t as *mut u8);
    }
}

pub extern "C" fn tcFree(t: Tc, _ev: *mut Environment) {
    free(t)
}

// ---- Copies in one block ----

fn round4(n: usize) -> usize {
    (n + 3) & !3
}

/// The size of the block tcCopyToBuffer fills.
pub fn buffer_length(t: Tc) -> usize {
    let t = if t.is_null() { &TC__null as *const _ as Tc } else { t };
    unsafe {
        let n = match *t {
            TK_OBJREF | TK_SELF => sz(w(t, 2)) + 1 + 0xc,
            TK_STRUCT => {
                let mut n = round4(w(t, 2) as usize * 8 + 0x14 + sz(w(t, 3)) + 1);
                for i in 0..w(t, 2) as usize {
                    n = round4(n + sz(*smember(t, i)) + 1);
                    n += buffer_length(*smember(t, i).add(1) as Tc);
                }
                n
            }
            TK_UNION => {
                let mut n = round4(w(t, 2) as usize * 16 + 0x18 + sz(w(t, 3)) + 1);
                n += buffer_length(tp(t, 4));
                for i in 0..w(t, 2) as usize {
                    n = round4(n + sz(*umember(t, i).add(2)) + 1);
                    n += buffer_length(*umember(t, i).add(3) as Tc);
                }
                n
            }
            TK_ENUM => w(t, 2) as usize * 4 + 0x14 + sz(w(t, 3)) + 1 + (0..w(t, 2) as usize).map(|i| sz(*(w(t, 4) as *const u32).add(i)) + 1).sum::<usize>(),
            TK_STRING => 0xc,
            TK_SEQUENCE | TK_ARRAY => 0x10 + buffer_length(tp(t, 2)),
            TK_POINTER => 0xc + buffer_length(tp(t, 2)),
            TK_FOREIGN => sz(w(t, 2)) + 1 + sz(w(t, 3)) + 1 + 0x14,
            _ => 8,
        };
        round4(n)
    }
}

/// Copy a TypeCode and its parts to `buf`; the end of what was written.
/// Only the outer TypeCode gets the "allocated" flag (when `dynamic`).
pub unsafe fn copy_to_buffer(t: Tc, buf: Tc, dynamic: bool) -> usize {
    if buf.is_null() {
        return 0;
    }
    let t = if t.is_null() { &TC__null as *const _ as Tc } else { t };
    unsafe {
        set(buf, 0, *t);
        set_align_field(buf, align_field(t));
        *(buf as *mut u8).add(6) = *(t as *const u8).add(6);
        set_flags(buf, (flags(t) & !DYNAMIC) | dynamic as u8);
        let mut at;
        match *t {
            TK_OBJREF | TK_SELF => {
                at = buf.add(3) as usize;
                set(buf, 2, put_str(w(t, 2), &mut at));
            }
            TK_STRUCT => {
                let n = w(t, 2) as usize;
                at = buf.add(5 + 2 * n) as usize;
                set(buf, 2, n as u32);
                set(buf, 4, buf.add(5) as u32);
                set(buf, 3, put_str(w(t, 3), &mut at));
                at = round4(at);
                for i in 0..n {
                    let (s, d) = (smember(t, i), smember(buf, i));
                    *d = put_str(*s, &mut at);
                    at = round4(at);
                    *d.add(1) = at as u32;
                    at = copy_to_buffer(*s.add(1) as Tc, at as Tc, false);
                }
            }
            TK_UNION => {
                let n = w(t, 2) as usize;
                at = buf.add(6 + 4 * n) as usize;
                set(buf, 2, n as u32);
                set(buf, 3, put_str(w(t, 3), &mut at));
                at = round4(at);
                set(buf, 4, at as u32);
                at = copy_to_buffer(tp(t, 4), at as Tc, false);
                set(buf, 5, buf.add(6) as u32);
                for i in 0..n {
                    let (s, d) = (umember(t, i), umember(buf, i));
                    *d = *s;
                    *d.add(1) = *s.add(1);
                    *d.add(2) = put_str(*s.add(2), &mut at);
                    at = round4(at);
                    *d.add(3) = at as u32;
                    at = copy_to_buffer(*s.add(3) as Tc, at as Tc, false);
                }
            }
            TK_ENUM => {
                let n = w(t, 2) as usize;
                at = buf.add(5 + n) as usize;
                set(buf, 2, n as u32);
                set(buf, 3, put_str(w(t, 3), &mut at));
                set(buf, 4, buf.add(5) as u32);
                for i in 0..n {
                    *buf.add(5 + i) = put_str(*(w(t, 4) as *const u32).add(i), &mut at);
                }
            }
            TK_STRING => {
                at = buf.add(3) as usize;
                set(buf, 2, w(t, 2));
            }
            TK_SEQUENCE | TK_ARRAY => {
                at = buf.add(4) as usize;
                set(buf, 2, at as u32);
                set(buf, 3, w(t, 3));
                at = copy_to_buffer(tp(t, 2), at as Tc, false);
            }
            TK_POINTER => {
                at = buf.add(3) as usize;
                set(buf, 2, at as u32);
                at = copy_to_buffer(tp(t, 2), at as Tc, false);
            }
            TK_FOREIGN => {
                at = buf.add(5) as usize;
                set(buf, 4, w(t, 4));
                set(buf, 2, put_str(w(t, 2), &mut at));
                set(buf, 3, put_str(w(t, 3), &mut at));
            }
            _ => at = buf.add(2) as usize,
        }
        round4(at)
    }
}

pub fn copy(t: Tc) -> Tc {
    if t.is_null() {
        return t;
    }
    let b = alloc(buffer_length(t));
    if !b.is_null() {
        unsafe { copy_to_buffer(t, b, true) };
    }
    b
}

pub extern "C" fn tcCopy(t: Tc, _ev: *mut Environment) -> Tc {
    copy(t)
}

/// `_Optlink` entries: tcBufferLength(EAX tc), tcCopyToBuffer(EAX tc,
/// EDX buffer, CL allocated), tcDefaultAlignment(EAX tc).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __o_tcBufferLength() {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 16",
        "mov [esp], eax", "call {f}", "mov esp, ebp", "pop ebp", "ret",
        f = sym ol_buffer_length,
    );
}
extern "C" fn ol_buffer_length(t: Tc) -> usize {
    buffer_length(t)
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __o_tcCopyToBuffer() {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 16",
        "mov [esp], eax", "mov [esp + 4], edx", "movzx ecx, cl", "mov [esp + 8], ecx",
        "call {f}", "mov esp, ebp", "pop ebp", "ret",
        f = sym ol_copy_to_buffer,
    );
}
extern "C" fn ol_copy_to_buffer(t: Tc, buf: Tc, dynamic: u32) -> usize {
    unsafe { copy_to_buffer(t, buf, dynamic != 0) }
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __o_tcDefaultAlignment() {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 16",
        "mov [esp], eax", "call {f}", "mov esp, ebp", "pop ebp", "ret",
        f = sym ol_default_alignment,
    );
}
extern "C" fn ol_default_alignment(t: Tc) -> i32 {
    default_alignment(t) as i32
}

// ---- Sequences ----

#[repr(C)]
pub struct Seq {
    pub max: u32,
    pub len: u32,
    pub buf: *mut u8,
}

/// tcSequenceNew: a sequence for `max` elements of the type.
pub extern "C" fn tcSequenceNew(t: Tc, max: u32) -> *mut Seq {
    let s = size(t) as usize;
    let q = alloc(12) as *mut Seq;
    if q.is_null() {
        return q;
    }
    unsafe {
        (*q).buf = calloc(1, s * max as usize);
        (*q).max = max;
        (*q).len = 0;
    }
    q
}

/// The next item separator of a list from `p` (IBM's: C++ declarations
/// and maps keep the commas inside their parentheses).
unsafe fn next_comma(p: *mut u8) -> *mut u8 {
    unsafe {
        let rest = cstr(p);
        if rest.starts_with(b"cxxdecl") || rest.starts_with(b"cxxmap") {
            if rest.is_empty() || rest[0] == b',' {
                return if rest.is_empty() { core::ptr::null_mut() } else { p };
            }
            let mut depth = 0i32;
            for (i, &c) in rest.iter().enumerate() {
                if c == b'(' {
                    depth += 1;
                } else if c == b')' {
                    depth -= 1;
                } else if c == b',' && depth == 0 && i > 0 {
                    return p.add(i);
                }
            }
            return core::ptr::null_mut();
        }
        match rest.iter().position(|&c| c == b',') {
            Some(i) => p.add(i),
            None => core::ptr::null_mut(),
        }
    }
}

/// tcSeqFromListString: a comma-separated list as a sequence of strings
/// (one block: the pointers, then a copy of the list).
pub extern "C" fn tcSeqFromListString(ret: *mut Seq, s: *const u8) -> *mut Seq {
    unsafe {
        (*ret).max = 0;
        (*ret).len = 0;
        (*ret).buf = core::ptr::null_mut();
        if s.is_null() {
            return ret;
        }
        let mut count = 1;
        let mut p = next_comma(s as *mut u8);
        while !p.is_null() {
            count += 1;
            p = next_comma(p.add(1));
        }
        let len = strlen(s) + 1;
        let b = alloc(len + count * 4);
        (*ret).max = count as u32;
        if b.is_null() {
            return ret;
        }
        let text = (b as *mut u8).add(count * 4);
        core::ptr::copy_nonoverlapping(s, text, len);
        *b = text as u32;
        let mut at = text;
        for i in 1..count {
            let c = next_comma(at);
            *c = 0;
            *b.add(i) = c.add(1) as u32;
            at = c.add(1);
        }
        (*ret).len = count as u32;
        (*ret).buf = b as *mut u8;
        ret
    }
}

// ---- Printing ----

fn out(fmt: &[u8], args: &[u32]) {
    somsup::printf(fmt, args);
}

/// The TypeCode as the C call that makes it (IBM's format).
pub fn print(t: Tc, newline: bool) {
    unsafe {
        if t.is_null() {
            out(b"NULL\0", &[]);
            if newline {
                out(b"\n\0", &[]);
            }
            return;
        }
        out(b"TypeCodeNew (\0", &[]);
        if flags(t) & DYNAMIC == 0 {
            out(b"/*constant*/ \0", &[]);
        }
        if flags(t) & ALIGNED != 0 {
            out(b"/* align=%hd */ \0", &[align_field(t) as i32 as u32]);
        }
        match *t {
            TK_NULL => out(b"tk_null\0", &[]),
            TK_VOID => out(b"tk_void\0", &[]),
            TK_SHORT => out(b"tk_short\0", &[]),
            TK_LONG => out(b"tk_long\0", &[]),
            TK_USHORT => out(b"tk_ushort\0", &[]),
            TK_ULONG => out(b"tk_ulong\0", &[]),
            TK_FLOAT => out(b"tk_float\0", &[]),
            TK_DOUBLE => out(b"tk_double\0", &[]),
            TK_BOOLEAN => out(b"tk_boolean\0", &[]),
            TK_CHAR => out(b"tk_char\0", &[]),
            TK_OCTET => out(b"tk_octet\0", &[]),
            TK_ANY => out(b"tk_any\0", &[]),
            TK_TYPECODE => out(b"tk_TypeCode\0", &[]),
            TK_PRINCIPAL => out(b"tk_Principal\0", &[]),
            TK_OBJREF => out(b"tk_objref, \"%s\"\0", &[w(t, 2)]),
            TK_STRUCT => {
                out(b"tk_struct, \"%s\", \0", &[w(t, 3)]);
                for i in 0..w(t, 2) as usize {
                    out(b"\"%s\", \0", &[*smember(t, i)]);
                    print(*smember(t, i).add(1) as Tc, false);
                    out(b", \0", &[]);
                }
                out(b"NULL\0", &[]);
            }
            TK_UNION => {
                out(b"tk_union, \"%s\", \0", &[w(t, 3)]);
                print(tp(t, 4), false);
                out(b", \0", &[]);
                let sw = tp(t, 4);
                for i in 0..w(t, 2) as usize {
                    let m = umember(t, i);
                    if *m == 2 {
                        out(b"TCDEFAULT_CASE, 0L, \0", &[]);
                    } else {
                        out(b"TCREGULAR_CASE, \0", &[]);
                        let l = m.add(1);
                        match *sw {
                            TK_SHORT => out(b"%dL, \0", &[*(l as *const i16) as i32 as u32]),
                            TK_LONG => out(b"%ldL, \0", &[*l]),
                            TK_USHORT => out(b"%uL, \0", &[*(l as *const u16) as u32]),
                            TK_BOOLEAN => out(if *(l as *const u8) == 0 { b"(long)FALSE, \0" } else { b"(long)TRUE, \0" }, &[]),
                            TK_CHAR => out(b"(long)'%c', \0", &[*(l as *const u8) as u32]),
                            TK_ENUM => {
                                let v = *l;
                                if v == 0 || w(sw, 2) < v {
                                    out(b"(long)%ld, \0", &[v]);
                                } else {
                                    out(b"(long)%s, \0", &[*(w(sw, 4) as *const u32).add(v as usize - 1)]);
                                }
                            }
                            _ => out(b"%luL, \0", &[*l]),
                        }
                    }
                    out(b"\"%s\", \0", &[*m.add(2)]);
                    print(*m.add(3) as Tc, false);
                    out(b", \0", &[]);
                }
                out(b"NULL\0", &[]);
            }
            TK_ENUM => {
                out(b"tk_enum, \"%s\", \0", &[w(t, 3)]);
                for i in 0..w(t, 2) as usize {
                    out(b"\"%s\", \0", &[*(w(t, 4) as *const u32).add(i)]);
                }
                out(b"NULL\0", &[]);
            }
            TK_STRING => out(b"tk_string, %ld\0", &[w(t, 2)]),
            TK_SEQUENCE => {
                out(b"tk_sequence, \0", &[]);
                print(tp(t, 2), false);
                out(b", %ld\0", &[w(t, 3)]);
            }
            TK_ARRAY => {
                out(b"tk_array, \0", &[]);
                print(tp(t, 2), false);
                out(b", %ld\0", &[w(t, 3)]);
            }
            TK_POINTER => {
                out(b"tk_pointer, \0", &[]);
                print(tp(t, 2), false);
            }
            TK_SELF => out(b"tk_self, \"%s\"\0", &[w(t, 2)]),
            TK_FOREIGN => out(b"tk_foreign, \"%s\", \"%s\", %ld\0", &[w(t, 2), w(t, 3), w(t, 4)]),
            k => out(b"<Error - unknown tag: %d>\0", &[k]),
        }
        out(b")\0", &[]);
        if newline {
            out(b"\n\0", &[]);
        }
    }
}

pub extern "C" fn tcPrint(t: Tc, _ev: *mut Environment) {
    print(t, true)
}

/// tciPrintf(fmt, ...): somVprintf.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_tciPrintf() {
    core::arch::naked_asm!(
        "lea eax, [esp + 8]",
        "push eax",
        "push dword ptr [esp + 8]",
        "call {f}",
        "add esp, 8",
        "ret",
        f = sym somsup::somVprintf,
    );
}

pub extern "C" fn tciSetException(ev: *mut Environment, major: u32, name: *const u8, params: *mut u8) {
    if !ev.is_null() {
        unsafe { somsup::somSetException(ev, major, name, params) };
    }
}

aligned! {
    __a_tcAlignment => tcAlignment, 2;
    __a_tcParameter => tcParameter, 4;
    __a_tcParmCount => tcParmCount, 2;
    __a_tcSequenceNew => tcSequenceNew, 2;
    __a_tcSetAlignment => tcSetAlignment, 3;
    __a_tcFree => tcFree, 2;
    __a_tcKind => tcKind, 2;
    __a_tciSetException => tciSetException, 4;
    __a_tcCopy => tcCopy, 2;
    __a_tcSize => tcSize, 2;
    __a_tcEqual => tcEqual, 3;
    __a_tcNewVL => tcNewVL, 2;
    __a_tcPrint => tcPrint, 2;
    __a_tcSeqFromListString => tcSeqFromListString, 2;
}
