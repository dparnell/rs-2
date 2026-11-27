//! Code pages and strings. A Win32 program's ANSI and OEM code page is
//! its OS/2 process code page (437 or 850), so its byte strings are OS/2's
//! as they are; UTF-16 converts through that page's table (or 1252's for
//! CP 1252).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::cptab::{CP1252, CP437, CP850};
use crate::{cstr, err, fail, wstr, FALSE, TRUE};

static PROCESS_CP: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    #[link_name = "DOSCALLS.291"]
    fn DosQueryCp(cb: u32, cps: *mut u32, pcb: *mut u32) -> u32;
}

/// The process's code page (437 or 850; others are treated as 850).
pub fn cp() -> u32 {
    let c = PROCESS_CP.load(Ordering::Relaxed);
    if c != 0 {
        return c;
    }
    let mut cps = [0u32; 4];
    let mut n = 0;
    let c = if unsafe { DosQueryCp(16, cps.as_mut_ptr(), &mut n) } == 0 && cps[0] == 437 { 437 } else { 850 };
    PROCESS_CP.store(c, Ordering::Relaxed);
    c
}

fn table(page: u32) -> &'static [u16; 128] {
    let page = match page {
        0 | 1 | 2 | 3 => cp(), // CP_ACP, CP_OEMCP, CP_MACCP, CP_THREAD_ACP
        p => p,
    };
    match page {
        437 => &CP437,
        1252 => &CP1252,
        _ => &CP850,
    }
}

pub const CP_UTF8: u32 = 65001;

pub fn byte_to_wide(page: u32, b: u8) -> u16 {
    if b < 0x80 { b as u16 } else { table(page)[(b - 0x80) as usize] }
}

pub fn wide_to_byte(page: u32, c: u16) -> Option<u8> {
    if c < 0x80 {
        return Some(c as u8);
    }
    table(page).iter().position(|&x| x == c).map(|i| i as u8 + 0x80)
}

/// Bytes in the process code page to UTF-16.
pub fn to_wide(b: &[u8]) -> Vec<u16> {
    b.iter().map(|&c| byte_to_wide(0, c)).collect()
}

/// UTF-16 to bytes in the process code page ('?' for what it lacks).
pub fn from_wide(w: &[u16]) -> Vec<u8> {
    w.iter().map(|&c| wide_to_byte(0, c).unwrap_or(b'?')).collect()
}

fn utf8_decode(b: &[u8]) -> Vec<u16> {
    let s = alloc::string::String::from_utf8_lossy(b);
    s.encode_utf16().collect()
}

fn utf8_encode(w: &[u16]) -> Vec<u8> {
    let s: alloc::string::String = char::decode_utf16(w.iter().copied()).map(|r| r.unwrap_or('\u{fffd}')).collect();
    s.into_bytes()
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn MultiByteToWideChar(page: u32, _flags: u32, src: *const u8, n: i32, dst: *mut u16, size: i32) -> i32 {
    if src.is_null() {
        return fail(err::INVALID_PARAMETER, 0);
    }
    let s = if n < 0 {
        let s = unsafe { cstr(src) };
        unsafe { core::slice::from_raw_parts(src, s.len() + 1) }
    } else {
        unsafe { core::slice::from_raw_parts(src, n as usize) }
    };
    let w: Vec<u16> = if page == CP_UTF8 { utf8_decode(s) } else { s.iter().map(|&c| byte_to_wide(page, c)).collect() };
    if size == 0 {
        return w.len() as i32;
    }
    if (size as usize) < w.len() {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    unsafe { core::ptr::copy_nonoverlapping(w.as_ptr(), dst, w.len()) };
    w.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WideCharToMultiByte(page: u32, _flags: u32, src: *const u16, n: i32, dst: *mut u8, size: i32, default: *const u8, used: *mut i32) -> i32 {
    if src.is_null() {
        return fail(err::INVALID_PARAMETER, 0);
    }
    let s = if n < 0 {
        let s = unsafe { wstr(src) };
        unsafe { core::slice::from_raw_parts(src, s.len() + 1) }
    } else {
        unsafe { core::slice::from_raw_parts(src, n as usize) }
    };
    let dflt = if default.is_null() { b'?' } else { unsafe { *default } };
    let mut any_default = false;
    let b: Vec<u8> = if page == CP_UTF8 {
        utf8_encode(s)
    } else {
        s.iter()
            .map(|&c| {
                wide_to_byte(page, c).unwrap_or_else(|| {
                    any_default = true;
                    dflt
                })
            })
            .collect()
    };
    if !used.is_null() {
        unsafe { *used = any_default as i32 };
    }
    if size == 0 {
        return b.len() as i32;
    }
    if (size as usize) < b.len() {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), dst, b.len()) };
    b.len() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetACP() -> u32 {
    cp()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetOEMCP() -> u32 {
    cp()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetConsoleCP() -> u32 {
    cp()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetConsoleOutputCP() -> u32 {
    cp()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleCP(_cp: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleOutputCP(_cp: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsValidCodePage(cp: u32) -> i32 {
    matches!(cp, 437 | 850 | 1252 | CP_UTF8) as i32
}

/// CPINFO: MaxCharSize, DefaultChar[2], LeadByte[12].
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetCPInfo(cp: u32, info: *mut u8) -> i32 {
    if info.is_null() {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    unsafe {
        core::ptr::write_bytes(info, 0, 20);
        (info as *mut u32).write(if cp == CP_UTF8 { 4 } else { 1 });
        *info.add(4) = b'?';
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsDBCSLeadByte(_b: u8) -> i32 {
    FALSE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsDBCSLeadByteEx(_cp: u32, _b: u8) -> i32 {
    FALSE
}

// ---- Case ----

/// Upper case of a code page byte (through its character).
pub fn upper(b: u8) -> u8 {
    if b.is_ascii() {
        return b.to_ascii_uppercase();
    }
    let c = byte_to_wide(0, b);
    let u = wupper(c);
    wide_to_byte(0, u).unwrap_or(b)
}

pub fn lower(b: u8) -> u8 {
    if b.is_ascii() {
        return b.to_ascii_lowercase();
    }
    let c = byte_to_wide(0, b);
    let l = wlower(c);
    wide_to_byte(0, l).unwrap_or(b)
}

pub fn wupper(c: u16) -> u16 {
    match char::from_u32(c as u32) {
        Some(ch) => {
            let mut u = ch.to_uppercase();
            match (u.next(), u.next()) {
                (Some(x), None) if (x as u32) < 0x10000 => x as u16,
                _ => c,
            }
        }
        None => c,
    }
}

pub fn wlower(c: u16) -> u16 {
    match char::from_u32(c as u32) {
        Some(ch) => {
            let mut l = ch.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) if (x as u32) < 0x10000 => x as u16,
                _ => c,
            }
        }
        None => c,
    }
}

// ---- lstr ----

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrlenA(s: *const u8) -> i32 {
    unsafe { cstr(s) }.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrlenW(s: *const u16) -> i32 {
    unsafe { wstr(s) }.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcpyA(d: *mut u8, s: *const u8) -> *mut u8 {
    let src = unsafe { cstr(s) };
    unsafe {
        core::ptr::copy(src.as_ptr(), d, src.len());
        *d.add(src.len()) = 0;
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcpyW(d: *mut u16, s: *const u16) -> *mut u16 {
    let src = unsafe { wstr(s) };
    unsafe {
        core::ptr::copy(src.as_ptr(), d, src.len());
        *d.add(src.len()) = 0;
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcpynA(d: *mut u8, s: *const u8, n: i32) -> *mut u8 {
    if n <= 0 {
        return d;
    }
    let src = unsafe { cstr(s) };
    let k = src.len().min(n as usize - 1);
    unsafe {
        core::ptr::copy(src.as_ptr(), d, k);
        *d.add(k) = 0;
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcpynW(d: *mut u16, s: *const u16, n: i32) -> *mut u16 {
    if n <= 0 {
        return d;
    }
    let src = unsafe { wstr(s) };
    let k = src.len().min(n as usize - 1);
    unsafe {
        core::ptr::copy(src.as_ptr(), d, k);
        *d.add(k) = 0;
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcatA(d: *mut u8, s: *const u8) -> *mut u8 {
    let n = unsafe { cstr(d) }.len();
    unsafe { lstrcpyA(d.add(n), s) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcatW(d: *mut u16, s: *const u16) -> *mut u16 {
    let n = unsafe { wstr(d) }.len();
    unsafe { lstrcpyW(d.add(n), s) };
    d
}

/// Windows' comparison: by character, case-insensitive if `nocase`.
fn compare(a: &[u16], b: &[u16], nocase: bool) -> i32 {
    let f = |c: u16| if nocase { wlower(c) } else { c };
    for (x, y) in a.iter().map(|&c| f(c)).zip(b.iter().map(|&c| f(c))) {
        if x != y {
            return if x < y { -1 } else { 1 };
        }
    }
    (a.len() as i32 - b.len() as i32).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcmpA(a: *const u8, b: *const u8) -> i32 {
    compare(&to_wide(unsafe { cstr(a) }), &to_wide(unsafe { cstr(b) }), false)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcmpiA(a: *const u8, b: *const u8) -> i32 {
    compare(&to_wide(unsafe { cstr(a) }), &to_wide(unsafe { cstr(b) }), true)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcmpW(a: *const u16, b: *const u16) -> i32 {
    compare(unsafe { wstr(a) }, unsafe { wstr(b) }, false)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn lstrcmpiW(a: *const u16, b: *const u16) -> i32 {
    compare(unsafe { wstr(a) }, unsafe { wstr(b) }, true)
}

const NORM_IGNORECASE: u32 = 1;
const CSTR_LESS_THAN: i32 = 1;

unsafe fn sized_a<'a>(s: *const u8, n: i32) -> &'a [u8] {
    if n < 0 { unsafe { cstr(s) } } else { unsafe { core::slice::from_raw_parts(s, n as usize) } }
}

unsafe fn sized_w<'a>(s: *const u16, n: i32) -> &'a [u16] {
    if n < 0 { unsafe { wstr(s) } } else { unsafe { core::slice::from_raw_parts(s, n as usize) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CompareStringA(_lcid: u32, flags: u32, a: *const u8, na: i32, b: *const u8, nb: i32) -> i32 {
    let (a, b) = unsafe { (to_wide(sized_a(a, na)), to_wide(sized_a(b, nb))) };
    compare(&a, &b, flags & NORM_IGNORECASE != 0) + 1 + CSTR_LESS_THAN
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CompareStringW(_lcid: u32, flags: u32, a: *const u16, na: i32, b: *const u16, nb: i32) -> i32 {
    let (a, b) = unsafe { (sized_w(a, na), sized_w(b, nb)) };
    compare(a, b, flags & NORM_IGNORECASE != 0) + 1 + CSTR_LESS_THAN
}

const LCMAP_LOWERCASE: u32 = 0x100;
const LCMAP_UPPERCASE: u32 = 0x200;

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LCMapStringA(_lcid: u32, flags: u32, s: *const u8, n: i32, d: *mut u8, size: i32) -> i32 {
    let mut v: Vec<u8> = unsafe { sized_a(s, n) }.to_vec();
    if n < 0 {
        v.push(0);
    }
    for c in v.iter_mut() {
        if flags & LCMAP_UPPERCASE != 0 {
            *c = upper(*c);
        } else if flags & LCMAP_LOWERCASE != 0 {
            *c = lower(*c);
        }
    }
    if size == 0 {
        return v.len() as i32;
    }
    if (size as usize) < v.len() {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), d, v.len()) };
    v.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LCMapStringW(_lcid: u32, flags: u32, s: *const u16, n: i32, d: *mut u16, size: i32) -> i32 {
    let mut v: Vec<u16> = unsafe { sized_w(s, n) }.to_vec();
    if n < 0 {
        v.push(0);
    }
    for c in v.iter_mut() {
        if flags & LCMAP_UPPERCASE != 0 {
            *c = wupper(*c);
        } else if flags & LCMAP_LOWERCASE != 0 {
            *c = wlower(*c);
        }
    }
    if size == 0 {
        return v.len() as i32;
    }
    if (size as usize) < v.len() {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), d, v.len()) };
    v.len() as i32
}

/// GetStringTypeW, CT_CTYPE1 only: C1_UPPER 1, LOWER 2, DIGIT 4, SPACE 8,
/// PUNCT 0x10, CNTRL 0x20, BLANK 0x40, XDIGIT 0x80, ALPHA 0x100.
fn ctype1(c: u16) -> u16 {
    let Some(ch) = char::from_u32(c as u32) else { return 0 };
    let mut t = 0;
    if ch.is_uppercase() {
        t |= 1;
    }
    if ch.is_lowercase() {
        t |= 2;
    }
    if ch.is_ascii_digit() {
        t |= 4;
    }
    if ch.is_whitespace() {
        t |= 8;
    }
    if ch.is_ascii_punctuation() || (!ch.is_alphanumeric() && !ch.is_whitespace() && !ch.is_control() && c >= 0x80) {
        t |= 0x10;
    }
    if ch.is_control() {
        t |= 0x20;
    }
    if ch == ' ' || ch == '\t' {
        t |= 0x40;
    }
    if ch.is_ascii_hexdigit() {
        t |= 0x80;
    }
    if ch.is_alphabetic() {
        t |= 0x100;
    }
    t
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetStringTypeW(ty: u32, s: *const u16, n: i32, out: *mut u16) -> i32 {
    let s = unsafe { sized_w(s, n) };
    for (i, &c) in s.iter().enumerate() {
        unsafe { *out.add(i) = if ty == 1 { ctype1(c) } else { 0 } };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetStringTypeA(_lcid: u32, ty: u32, s: *const u8, n: i32, out: *mut u16) -> i32 {
    let s = unsafe { sized_a(s, n) };
    for (i, &c) in s.iter().enumerate() {
        unsafe { *out.add(i) = if ty == 1 { ctype1(byte_to_wide(0, c)) } else { 0 } };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetStringTypeExA(lcid: u32, ty: u32, s: *const u8, n: i32, out: *mut u16) -> i32 {
    unsafe { GetStringTypeA(lcid, ty, s, n, out) }
}

// ---- Locale (US English, the process's code page) ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetUserDefaultLCID() -> u32 {
    0x0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSystemDefaultLCID() -> u32 {
    0x0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetThreadLocale() -> u32 {
    0x0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetUserDefaultLangID() -> u16 {
    0x0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSystemDefaultLangID() -> u16 {
    0x0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsValidLocale(_lcid: u32, _flags: u32) -> i32 {
    TRUE
}

fn locale_info(ty: u32) -> Option<Vec<u8>> {
    let s: &[u8] = match ty & 0xffff {
        0x01 => b"0409",                // LOCALE_ILANGUAGE
        0x02 => b"English (United States)",
        0x03 => b"ENU",                 // LOCALE_SABBREVLANGNAME
        0x04 => b"English",             // LOCALE_SNATIVELANGNAME
        0x05 => b"1",                   // LOCALE_ICOUNTRY
        0x06 => b"United States",       // LOCALE_SCOUNTRY
        0x07 => b"USA",                 // LOCALE_SABBREVCTRYNAME
        0x0b => return Some(alloc::format!("{}", cp()).into_bytes()), // LOCALE_IDEFAULTCODEPAGE
        0x1004 => return Some(alloc::format!("{}", cp()).into_bytes()), // LOCALE_IDEFAULTANSICODEPAGE
        0x0c => b",",                   // LOCALE_SLIST
        0x0e => b".",                   // LOCALE_SDECIMAL
        0x0f => b",",                   // LOCALE_STHOUSAND
        0x10 => b"3;0",                 // LOCALE_SGROUPING
        0x11 => b"2",                   // LOCALE_IDIGITS
        0x14 => b"$",                   // LOCALE_SCURRENCY
        0x1d => b"/",                   // LOCALE_SDATE
        0x1e => b":",                   // LOCALE_STIME
        0x1f => b"M/d/yyyy",            // LOCALE_SSHORTDATE
        0x20 => b"dddd, MMMM dd, yyyy", // LOCALE_SLONGDATE
        0x21 => b"0",                   // LOCALE_IDATE
        0x23 => b"0",                   // LOCALE_ITIME
        0x28 => b"AM",                  // LOCALE_S1159
        0x29 => b"PM",                  // LOCALE_S2359
        0x59 => b"en",                  // LOCALE_SISO639LANGNAME
        0x5a => b"US",                  // LOCALE_SISO3166CTRYNAME
        _ => return None,
    };
    Some(s.to_vec())
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetLocaleInfoA(_lcid: u32, ty: u32, buf: *mut u8, size: i32) -> i32 {
    let Some(s) = locale_info(ty) else { return fail(err::INVALID_PARAMETER, 0) };
    if size == 0 {
        return s.len() as i32 + 1;
    }
    if (size as usize) < s.len() + 1 {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    unsafe { crate::put_str(&s, buf, size as u32) as i32 + 1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetLocaleInfoW(_lcid: u32, ty: u32, buf: *mut u16, size: i32) -> i32 {
    let Some(s) = locale_info(ty) else { return fail(err::INVALID_PARAMETER, 0) };
    let w = to_wide(&s);
    if size == 0 {
        return w.len() as i32 + 1;
    }
    if (size as usize) < w.len() + 1 {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    unsafe { crate::put_wstr(&w, buf, size as u32) as i32 + 1 }
}
