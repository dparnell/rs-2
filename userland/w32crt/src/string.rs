//! Strings, wide strings, character classes, number conversions,
//! sorting, random numbers. (memcpy, memset, memmove, memcmp and strlen
//! are the os2 crate's.)

use alloc::vec::Vec;

use crate::{c_alloc_str, cstr, wstr};

// ---- Character classes: the C locale's table (_pctype) ----

const UPPER: u16 = 0x1;
const LOWER: u16 = 0x2;
const DIGIT: u16 = 0x4;
const SPACE: u16 = 0x8;
const PUNCT: u16 = 0x10;
const CONTROL: u16 = 0x20;
const BLANK: u16 = 0x40;
const HEX: u16 = 0x80;
const ALPHA: u16 = 0x100 | UPPER | LOWER;

const fn class(c: u8) -> u16 {
    let mut t = 0;
    if c.is_ascii_uppercase() {
        t |= UPPER | 0x100;
    }
    if c.is_ascii_lowercase() {
        t |= LOWER | 0x100;
    }
    if c.is_ascii_digit() {
        t |= DIGIT;
    }
    if matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        t |= SPACE;
    }
    if c.is_ascii_punctuation() {
        t |= PUNCT;
    }
    if c < 0x20 || c == 0x7f {
        t |= CONTROL;
    }
    if c == b' ' {
        t |= BLANK;
    }
    if c.is_ascii_hexdigit() {
        t |= HEX;
    }
    t
}

/// 257 entries: index -1 (EOF) is 0; _pctype points at entry 0.
static CTYPE: [u16; 257] = {
    let mut t = [0u16; 257];
    let mut i = 0;
    while i < 256 {
        t[i + 1] = if i < 128 { class(i as u8) } else { 0 };
        i += 1;
    }
    t
};

#[unsafe(no_mangle)]
pub static mut _pctype: *const u16 = unsafe { CTYPE.as_ptr().add(1) };
#[unsafe(no_mangle)]
pub static mut _pwctype: *const u16 = unsafe { CTYPE.as_ptr().add(1) };

#[unsafe(no_mangle)]
pub extern "C" fn __pctype_func() -> *const u16 {
    unsafe { CTYPE.as_ptr().add(1) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __pwctype_func() -> *const u16 {
    unsafe { CTYPE.as_ptr().add(1) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __p__pctype() -> *mut *const u16 {
    unsafe { &raw mut _pctype }
}

fn ct(c: i32) -> u16 {
    if (-1..256).contains(&c) { CTYPE[(c + 1) as usize] } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn _isctype(c: i32, mask: i32) -> i32 {
    (ct(c) & mask as u16) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn _isctype_l(c: i32, mask: i32, _l: u32) -> i32 {
    _isctype(c, mask)
}

macro_rules! isfn {
    ($name:ident, $mask:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(c: i32) -> i32 {
            (ct(c) & $mask) as i32
        }
    };
}

isfn!(isalpha, ALPHA);
isfn!(isupper, UPPER);
isfn!(islower, LOWER);
isfn!(isdigit, DIGIT);
isfn!(isxdigit, HEX);
isfn!(isspace, SPACE);
isfn!(ispunct, PUNCT);
isfn!(isalnum, ALPHA | DIGIT);
isfn!(isprint, ALPHA | DIGIT | PUNCT | BLANK);
isfn!(isgraph, ALPHA | DIGIT | PUNCT);
isfn!(iscntrl, CONTROL);

#[unsafe(no_mangle)]
pub extern "C" fn isblank(c: i32) -> i32 {
    (c == b' ' as i32 || c == b'\t' as i32) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn __isascii(c: i32) -> i32 {
    ((c as u32) < 0x80) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn __iscsym(c: i32) -> i32 {
    (isalnum(c) != 0 || c == b'_' as i32) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn __iscsymf(c: i32) -> i32 {
    (isalpha(c) != 0 || c == b'_' as i32) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn toupper(c: i32) -> i32 {
    if (b'a' as i32..=b'z' as i32).contains(&c) { c - 32 } else { c }
}

#[unsafe(no_mangle)]
pub extern "C" fn tolower(c: i32) -> i32 {
    if (b'A' as i32..=b'Z' as i32).contains(&c) { c + 32 } else { c }
}

#[unsafe(no_mangle)]
pub extern "C" fn _toupper(c: i32) -> i32 {
    c - 32
}

#[unsafe(no_mangle)]
pub extern "C" fn _tolower(c: i32) -> i32 {
    c + 32
}

fn wclass(c: u32) -> u16 {
    if c < 128 {
        return ct(c as i32);
    }
    let Some(ch) = char::from_u32(c) else { return 0 };
    let mut t = 0;
    if ch.is_uppercase() {
        t |= UPPER | 0x100;
    }
    if ch.is_lowercase() {
        t |= LOWER | 0x100;
    }
    if ch.is_alphabetic() {
        t |= 0x100;
    }
    if ch.is_whitespace() {
        t |= SPACE;
    }
    if ch.is_control() {
        t |= CONTROL;
    }
    if !ch.is_alphanumeric() && !ch.is_whitespace() && !ch.is_control() {
        t |= PUNCT;
    }
    t
}

#[unsafe(no_mangle)]
pub extern "C" fn iswctype(c: u32, mask: u32) -> i32 {
    (wclass(c & 0xffff) & mask as u16) as i32
}

macro_rules! iswfn {
    ($name:ident, $mask:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(c: u32) -> i32 {
            (wclass(c & 0xffff) & $mask) as i32
        }
    };
}

iswfn!(iswalpha, ALPHA);
iswfn!(iswupper, UPPER);
iswfn!(iswlower, LOWER);
iswfn!(iswdigit, DIGIT);
iswfn!(iswxdigit, HEX);
iswfn!(iswspace, SPACE);
iswfn!(iswpunct, PUNCT);
iswfn!(iswalnum, ALPHA | DIGIT);
iswfn!(iswprint, ALPHA | DIGIT | PUNCT | BLANK);
iswfn!(iswgraph, ALPHA | DIGIT | PUNCT);
iswfn!(iswcntrl, CONTROL);

#[unsafe(no_mangle)]
pub extern "C" fn towupper(c: u32) -> u32 {
    match char::from_u32(c & 0xffff) {
        Some(ch) => {
            let mut u = ch.to_uppercase();
            match (u.next(), u.next()) {
                (Some(x), None) if (x as u32) < 0x10000 => x as u32,
                _ => c,
            }
        }
        None => c,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn towlower(c: u32) -> u32 {
    match char::from_u32(c & 0xffff) {
        Some(ch) => {
            let mut l = ch.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) if (x as u32) < 0x10000 => x as u32,
                _ => c,
            }
        }
        None => c,
    }
}

// ---- Narrow strings ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcpy(d: *mut u8, s: *const u8) -> *mut u8 {
    let n = unsafe { cstr(s) }.len();
    unsafe { core::ptr::copy(s, d, n + 1) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    let src = unsafe { cstr(s) };
    let k = src.len().min(n);
    unsafe {
        core::ptr::copy(s, d, k);
        core::ptr::write_bytes(d.add(k), 0, n - k);
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcat(d: *mut u8, s: *const u8) -> *mut u8 {
    let n = unsafe { cstr(d) }.len();
    unsafe { strcpy(d.add(n), s) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncat(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    let dl = unsafe { cstr(d) }.len();
    let src = unsafe { cstr(s) };
    let k = src.len().min(n);
    unsafe {
        core::ptr::copy(s, d.add(dl), k);
        *d.add(dl + k) = 0;
    }
    d
}

fn cmp_bytes(a: &[u8], b: &[u8]) -> i32 {
    for (x, y) in a.iter().zip(b.iter()) {
        if x != y {
            return *x as i32 - *y as i32;
        }
    }
    a.len() as i32 - b.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcmp(a: *const u8, b: *const u8) -> i32 {
    cmp_bytes(unsafe { cstr(a) }, unsafe { cstr(b) }).signum()
}

/// The first `n` bytes of a string (or all of it).
unsafe fn prefix<'a>(s: *const u8, n: usize) -> &'a [u8] {
    let mut k = 0;
    while k < n && unsafe { *s.add(k) } != 0 {
        k += 1;
    }
    unsafe { core::slice::from_raw_parts(s, k) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    cmp_bytes(unsafe { prefix(a, n) }, unsafe { prefix(b, n) }).signum()
}

fn lower_all(s: &[u8]) -> Vec<u8> {
    s.iter().map(|c| c.to_ascii_lowercase()).collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _stricmp(a: *const u8, b: *const u8) -> i32 {
    cmp_bytes(&lower_all(unsafe { cstr(a) }), &lower_all(unsafe { cstr(b) })).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strnicmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    cmp_bytes(&lower_all(unsafe { prefix(a, n) }), &lower_all(unsafe { prefix(b, n) })).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strcmpi(a: *const u8, b: *const u8) -> i32 {
    unsafe { _stricmp(a, b) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _memicmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let (x, y) = unsafe { (core::slice::from_raw_parts(a, n), core::slice::from_raw_parts(b, n)) };
    cmp_bytes(&lower_all(x), &lower_all(y)).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcoll(a: *const u8, b: *const u8) -> i32 {
    unsafe { strcmp(a, b) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _stricoll(a: *const u8, b: *const u8) -> i32 {
    unsafe { _stricmp(a, b) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strxfrm(d: *mut u8, s: *const u8, n: usize) -> usize {
    let src = unsafe { cstr(s) };
    if src.len() < n {
        unsafe { strcpy(d, s) };
    }
    src.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strnlen(s: *const u8, n: usize) -> usize {
    unsafe { prefix(s, n) }.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strchr(s: *const u8, c: i32) -> *const u8 {
    let b = unsafe { cstr(s) };
    let c = c as u8;
    if c == 0 {
        return unsafe { s.add(b.len()) };
    }
    match b.iter().position(|&x| x == c) {
        Some(i) => unsafe { s.add(i) },
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strrchr(s: *const u8, c: i32) -> *const u8 {
    let b = unsafe { cstr(s) };
    let c = c as u8;
    if c == 0 {
        return unsafe { s.add(b.len()) };
    }
    match b.iter().rposition(|&x| x == c) {
        Some(i) => unsafe { s.add(i) },
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memchr(s: *const u8, c: i32, n: usize) -> *const u8 {
    let b = unsafe { core::slice::from_raw_parts(s, n) };
    match b.iter().position(|&x| x == c as u8) {
        Some(i) => unsafe { s.add(i) },
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strstr(h: *const u8, n: *const u8) -> *const u8 {
    let (hb, nb) = unsafe { (cstr(h), cstr(n)) };
    if nb.is_empty() {
        return h;
    }
    match hb.windows(nb.len()).position(|w| w == nb) {
        Some(i) => unsafe { h.add(i) },
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strspn(s: *const u8, set: *const u8) -> usize {
    let (b, st) = unsafe { (cstr(s), cstr(set)) };
    b.iter().position(|c| !st.contains(c)).unwrap_or(b.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcspn(s: *const u8, set: *const u8) -> usize {
    let (b, st) = unsafe { (cstr(s), cstr(set)) };
    b.iter().position(|c| st.contains(c)).unwrap_or(b.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strpbrk(s: *const u8, set: *const u8) -> *const u8 {
    let n = unsafe { strcspn(s, set) };
    if unsafe { *s.add(n) } == 0 { core::ptr::null() } else { unsafe { s.add(n) } }
}

static mut STRTOK: *mut u8 = core::ptr::null_mut();

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtok_s(s: *mut u8, delim: *const u8, ctx: *mut *mut u8) -> *mut u8 {
    unsafe {
        let mut p = if s.is_null() { *ctx } else { s };
        if p.is_null() {
            return core::ptr::null_mut();
        }
        p = p.add(strspn(p, delim));
        if *p == 0 {
            *ctx = p;
            return core::ptr::null_mut();
        }
        let start = p;
        p = p.add(strcspn(p, delim));
        if *p != 0 {
            *p = 0;
            p = p.add(1);
        }
        *ctx = p;
        start
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtok(s: *mut u8, delim: *const u8) -> *mut u8 {
    unsafe { strtok_s(s, delim, &raw mut STRTOK) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strdup(s: *const u8) -> *mut u8 {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    c_alloc_str(unsafe { cstr(s) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strlwr(s: *mut u8) -> *mut u8 {
    let n = unsafe { cstr(s) }.len();
    for i in 0..n {
        unsafe { *s.add(i) = (*s.add(i)).to_ascii_lowercase() };
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strupr(s: *mut u8) -> *mut u8 {
    let n = unsafe { cstr(s) }.len();
    for i in 0..n {
        unsafe { *s.add(i) = (*s.add(i)).to_ascii_uppercase() };
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strrev(s: *mut u8) -> *mut u8 {
    let n = unsafe { cstr(s) }.len();
    unsafe { core::slice::from_raw_parts_mut(s, n) }.reverse();
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strset(s: *mut u8, c: i32) -> *mut u8 {
    let n = unsafe { cstr(s) }.len();
    unsafe { core::ptr::write_bytes(s, c as u8, n) };
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strnset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let k = unsafe { prefix(s, n) }.len();
    unsafe { core::ptr::write_bytes(s, c as u8, k) };
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _memccpy(d: *mut u8, s: *const u8, c: i32, n: usize) -> *mut u8 {
    for i in 0..n {
        let b = unsafe { *s.add(i) };
        unsafe { *d.add(i) = b };
        if b == c as u8 {
            return unsafe { d.add(i + 1) };
        }
    }
    core::ptr::null_mut()
}

const MESSAGES: [&str; 43] = [
    "No error", "Operation not permitted", "No such file or directory", "No such process", "Interrupted function call",
    "Input/output error", "No such device or address", "Arg list too long", "Exec format error", "Bad file descriptor",
    "No child processes", "Resource temporarily unavailable", "Not enough space", "Permission denied", "Bad address",
    "Unknown error", "Resource device", "File exists", "Improper link", "No such device", "Not a directory",
    "Is a directory", "Invalid argument", "Too many open files in system", "Too many open files",
    "Inappropriate I/O control operation", "Unknown error", "File too large", "No space left on device", "Invalid seek",
    "Read-only file system", "Too many links", "Broken pipe", "Domain error", "Result too large", "Unknown error",
    "Resource deadlock avoided", "Unknown error", "Filename too long", "No locks available", "Function not implemented",
    "Directory not empty", "Illegal byte sequence",
];

pub fn message(e: i32) -> &'static str {
    MESSAGES.get(e as usize).copied().unwrap_or("Unknown error")
}

static mut STRERROR: [u8; 64] = [0; 64];

#[unsafe(no_mangle)]
pub extern "C" fn strerror(e: i32) -> *const u8 {
    let m = message(e).as_bytes();
    unsafe {
        let b = &mut *(&raw mut STRERROR);
        b[..m.len()].copy_from_slice(m);
        b[m.len()] = 0;
        b.as_ptr()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strerror_s(buf: *mut u8, n: usize, e: i32) -> i32 {
    let m = message(e).as_bytes();
    if n == 0 {
        return crate::EINVAL;
    }
    let k = m.len().min(n - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(m.as_ptr(), buf, k);
        *buf.add(k) = 0;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _strerror(_s: *const u8) -> *const u8 {
    strerror(crate::errno())
}

// ---- Wide strings ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcslen(s: *const u16) -> usize {
    unsafe { wstr(s) }.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsnlen(s: *const u16, n: usize) -> usize {
    let mut k = 0;
    while k < n && unsafe { *s.add(k) } != 0 {
        k += 1;
    }
    k
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscpy(d: *mut u16, s: *const u16) -> *mut u16 {
    let n = unsafe { wstr(s) }.len();
    unsafe { core::ptr::copy(s, d, n + 1) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsncpy(d: *mut u16, s: *const u16, n: usize) -> *mut u16 {
    let k = unsafe { wcsnlen(s, n) };
    unsafe {
        core::ptr::copy(s, d, k);
        for i in k..n {
            *d.add(i) = 0;
        }
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscat(d: *mut u16, s: *const u16) -> *mut u16 {
    let n = unsafe { wstr(d) }.len();
    unsafe { wcscpy(d.add(n), s) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsncat(d: *mut u16, s: *const u16, n: usize) -> *mut u16 {
    let dl = unsafe { wstr(d) }.len();
    let k = unsafe { wcsnlen(s, n) };
    unsafe {
        core::ptr::copy(s, d.add(dl), k);
        *d.add(dl + k) = 0;
    }
    d
}

fn cmp_w(a: &[u16], b: &[u16]) -> i32 {
    for (x, y) in a.iter().zip(b.iter()) {
        if x != y {
            return *x as i32 - *y as i32;
        }
    }
    a.len() as i32 - b.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscmp(a: *const u16, b: *const u16) -> i32 {
    cmp_w(unsafe { wstr(a) }, unsafe { wstr(b) }).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsncmp(a: *const u16, b: *const u16, n: usize) -> i32 {
    let (x, y) = unsafe { (core::slice::from_raw_parts(a, wcsnlen(a, n)), core::slice::from_raw_parts(b, wcsnlen(b, n))) };
    cmp_w(x, y).signum()
}

fn wlow(s: &[u16]) -> Vec<u16> {
    s.iter().map(|&c| towlower(c as u32) as u16).collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wcsicmp(a: *const u16, b: *const u16) -> i32 {
    cmp_w(&wlow(unsafe { wstr(a) }), &wlow(unsafe { wstr(b) })).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wcsnicmp(a: *const u16, b: *const u16, n: usize) -> i32 {
    let (x, y) = unsafe { (core::slice::from_raw_parts(a, wcsnlen(a, n)), core::slice::from_raw_parts(b, wcsnlen(b, n))) };
    cmp_w(&wlow(x), &wlow(y)).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscoll(a: *const u16, b: *const u16) -> i32 {
    unsafe { wcscmp(a, b) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcschr(s: *const u16, c: u32) -> *const u16 {
    let b = unsafe { wstr(s) };
    if c == 0 {
        return unsafe { s.add(b.len()) };
    }
    match b.iter().position(|&x| x as u32 == c) {
        Some(i) => unsafe { s.add(i) },
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsrchr(s: *const u16, c: u32) -> *const u16 {
    let b = unsafe { wstr(s) };
    if c == 0 {
        return unsafe { s.add(b.len()) };
    }
    match b.iter().rposition(|&x| x as u32 == c) {
        Some(i) => unsafe { s.add(i) },
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsstr(h: *const u16, n: *const u16) -> *const u16 {
    let (hb, nb) = unsafe { (wstr(h), wstr(n)) };
    if nb.is_empty() {
        return h;
    }
    match hb.windows(nb.len()).position(|w| w == nb) {
        Some(i) => unsafe { h.add(i) },
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsspn(s: *const u16, set: *const u16) -> usize {
    let (b, st) = unsafe { (wstr(s), wstr(set)) };
    b.iter().position(|c| !st.contains(c)).unwrap_or(b.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscspn(s: *const u16, set: *const u16) -> usize {
    let (b, st) = unsafe { (wstr(s), wstr(set)) };
    b.iter().position(|c| st.contains(c)).unwrap_or(b.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcspbrk(s: *const u16, set: *const u16) -> *const u16 {
    let n = unsafe { wcscspn(s, set) };
    if unsafe { *s.add(n) } == 0 { core::ptr::null() } else { unsafe { s.add(n) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wcsdup(s: *const u16) -> *mut u16 {
    let b = unsafe { wstr(s) };
    let p = crate::mem::heap_alloc((b.len() + 1) * 2) as *mut u16;
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
            *p.add(b.len()) = 0;
        }
    }
    p
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wcslwr(s: *mut u16) -> *mut u16 {
    let n = unsafe { wstr(s) }.len();
    for i in 0..n {
        unsafe { *s.add(i) = towlower(*s.add(i) as u32) as u16 };
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wcsupr(s: *mut u16) -> *mut u16 {
    let n = unsafe { wstr(s) }.len();
    for i in 0..n {
        unsafe { *s.add(i) = towupper(*s.add(i) as u32) as u16 };
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemcpy(d: *mut u16, s: *const u16, n: usize) -> *mut u16 {
    unsafe { core::ptr::copy_nonoverlapping(s, d, n) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemmove(d: *mut u16, s: *const u16, n: usize) -> *mut u16 {
    unsafe { core::ptr::copy(s, d, n) };
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemset(d: *mut u16, c: u32, n: usize) -> *mut u16 {
    for i in 0..n {
        unsafe { *d.add(i) = c as u16 };
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemcmp(a: *const u16, b: *const u16, n: usize) -> i32 {
    let (x, y) = unsafe { (core::slice::from_raw_parts(a, n), core::slice::from_raw_parts(b, n)) };
    cmp_w(x, y).signum()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemchr(s: *const u16, c: u32, n: usize) -> *const u16 {
    let b = unsafe { core::slice::from_raw_parts(s, n) };
    match b.iter().position(|&x| x as u32 == c) {
        Some(i) => unsafe { s.add(i) },
        None => core::ptr::null(),
    }
}

static mut WCSTOK: *mut u16 = core::ptr::null_mut();

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcstok_s(s: *mut u16, delim: *const u16, ctx: *mut *mut u16) -> *mut u16 {
    unsafe {
        let mut p = if s.is_null() { *ctx } else { s };
        if p.is_null() {
            return core::ptr::null_mut();
        }
        p = p.add(wcsspn(p, delim));
        if *p == 0 {
            *ctx = p;
            return core::ptr::null_mut();
        }
        let start = p;
        p = p.add(wcscspn(p, delim));
        if *p != 0 {
            *p = 0;
            p = p.add(1);
        }
        *ctx = p;
        start
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcstok(s: *mut u16, delim: *const u16) -> *mut u16 {
    unsafe { wcstok_s(s, delim, &raw mut WCSTOK) }
}

// ---- Multibyte (single-byte code pages) ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbtowc(w: *mut u16, s: *const u8, n: usize) -> i32 {
    if s.is_null() {
        return 0;
    }
    if n == 0 {
        return -1;
    }
    let b = unsafe { *s };
    if !w.is_null() {
        let v = crate::widen(&[b]);
        unsafe { *w = v.first().copied().unwrap_or(b as u16) };
    }
    (b != 0) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbrtowc(w: *mut u16, s: *const u8, n: usize, _st: *mut u32) -> usize {
    if s.is_null() {
        return 0;
    }
    let r = unsafe { mbtowc(w, s, n) };
    if r < 0 { usize::MAX - 1 } else { r as usize }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mblen(s: *const u8, n: usize) -> i32 {
    if s.is_null() || n == 0 {
        return 0;
    }
    (unsafe { *s } != 0) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbrlen(s: *const u8, n: usize, _st: *mut u32) -> usize {
    unsafe { mblen(s, n) as usize }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wctomb(s: *mut u8, w: u16) -> i32 {
    if s.is_null() {
        return 0;
    }
    let b = crate::narrow(&[w]);
    unsafe { *s = b.first().copied().unwrap_or(b'?') };
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcrtomb(s: *mut u8, w: u16, _st: *mut u32) -> usize {
    unsafe { wctomb(s, w) as usize }
}

#[unsafe(no_mangle)]
pub extern "C" fn btowc(c: i32) -> u32 {
    if c < 0 { 0xffff } else { crate::widen(&[c as u8]).first().copied().unwrap_or(c as u16) as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn wctob(w: u32) -> i32 {
    let b = crate::narrow(&[w as u16]);
    b.first().map_or(-1, |&c| c as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn mbsinit(_st: *const u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbstowcs(w: *mut u16, s: *const u8, n: usize) -> usize {
    let v = crate::widen(unsafe { cstr(s) });
    if w.is_null() {
        return v.len();
    }
    let k = v.len().min(n);
    unsafe {
        core::ptr::copy_nonoverlapping(v.as_ptr(), w, k);
        if k < n {
            *w.add(k) = 0;
        }
    }
    k
}

/// mbsrtowcs: as mbstowcs, moving *src past what was converted (NULL at
/// the end).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbsrtowcs(w: *mut u16, src: *mut *const u8, n: usize, _st: *mut u32) -> usize {
    let s = unsafe { *src };
    let len = unsafe { cstr(s) }.len();
    let r = unsafe { mbstowcs(w, s, n) };
    if !w.is_null() {
        unsafe { *src = if r >= len { core::ptr::null() } else { s.add(r) } };
    }
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsrtombs(d: *mut u8, src: *mut *const u16, n: usize, _st: *mut u32) -> usize {
    let s = unsafe { *src };
    let len = unsafe { wstr(s) }.len();
    let r = unsafe { wcstombs(d, s, n) };
    if !d.is_null() {
        unsafe { *src = if r >= len { core::ptr::null() } else { s.add(r) } };
    }
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcstombs(s: *mut u8, w: *const u16, n: usize) -> usize {
    let v = crate::narrow(unsafe { wstr(w) });
    if s.is_null() {
        return v.len();
    }
    let k = v.len().min(n);
    unsafe {
        core::ptr::copy_nonoverlapping(v.as_ptr(), s, k);
        if k < n {
            *s.add(k) = 0;
        }
    }
    k
}

// ---- Number conversions ----

/// strtol's work: (value, bytes used, overflow), for `base` 0 or 2-36.
fn parse_int(s: &[u8], base: u32, signed: bool) -> (u64, bool, usize, bool) {
    let mut i = 0;
    while i < s.len() && ct(s[i] as i32) & SPACE != 0 {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = base;
    if (base == 0 || base == 16) && i + 1 < s.len() && s[i] == b'0' && (s[i + 1] | 0x20) == b'x' && i + 2 < s.len() && (s[i + 2] as char).is_ascii_hexdigit() {
        i += 2;
        base = 16;
    } else if base == 0 {
        base = if i < s.len() && s[i] == b'0' { 8 } else { 10 };
    }
    let start = i;
    let mut v: u64 = 0;
    let mut over = false;
    while i < s.len() {
        let d = match (s[i] as char).to_digit(36) {
            Some(d) if d < base => d,
            _ => break,
        };
        match v.checked_mul(base as u64).and_then(|x| x.checked_add(d as u64)) {
            Some(x) => v = x,
            None => over = true,
        }
        i += 1;
    }
    if i == start {
        return (0, false, 0, false);
    }
    let _ = signed;
    (v, neg, i, over)
}

unsafe fn str_end(s: *const u8, used: usize, end: *mut *const u8) {
    if !end.is_null() {
        unsafe { *end = s.add(used) };
    }
}

pub unsafe fn strtol_impl(s: *const u8, end: *mut *const u8, base: i32, min: i64, max: i64) -> i64 {
    let (v, neg, used, over) = parse_int(unsafe { cstr(s) }, base as u32, true);
    unsafe { str_end(s, used, end) };
    let (r, range) = if neg {
        if over || v > (-(min as i128)) as u64 { (min, true) } else { ((v as i128).wrapping_neg() as i64, false) }
    } else if over || v > max as u64 {
        (max, true)
    } else {
        (v as i64, false)
    };
    if range {
        crate::set_errno(crate::ERANGE);
    }
    r
}

pub unsafe fn strtoul_impl(s: *const u8, end: *mut *const u8, base: i32, max: u64) -> u64 {
    let (v, neg, used, over) = parse_int(unsafe { cstr(s) }, base as u32, false);
    unsafe { str_end(s, used, end) };
    if over || v > max {
        crate::set_errno(crate::ERANGE);
        return max;
    }
    if neg { v.wrapping_neg() & max } else { v }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtol(s: *const u8, end: *mut *const u8, base: i32) -> i32 {
    unsafe { strtol_impl(s, end, base, i32::MIN as i64, i32::MAX as i64) as i32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoul(s: *const u8, end: *mut *const u8, base: i32) -> u32 {
    unsafe { strtoul_impl(s, end, base, u32::MAX as u64) as u32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strtoi64(s: *const u8, end: *mut *const u8, base: i32) -> i64 {
    unsafe { strtol_impl(s, end, base, i64::MIN, i64::MAX) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strtoui64(s: *const u8, end: *mut *const u8, base: i32) -> u64 {
    unsafe { strtoul_impl(s, end, base, u64::MAX) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoll(s: *const u8, end: *mut *const u8, base: i32) -> i64 {
    unsafe { _strtoi64(s, end, base) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoull(s: *const u8, end: *mut *const u8, base: i32) -> u64 {
    unsafe { _strtoui64(s, end, base) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoimax(s: *const u8, end: *mut *const u8, base: i32) -> i64 {
    unsafe { _strtoi64(s, end, base) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoumax(s: *const u8, end: *mut *const u8, base: i32) -> u64 {
    unsafe { _strtoui64(s, end, base) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atoi(s: *const u8) -> i32 {
    let (v, neg, _, _) = parse_int(unsafe { cstr(s) }, 10, true);
    let v = v as i64 as i32;
    if neg { v.wrapping_neg() } else { v }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atol(s: *const u8) -> i32 {
    unsafe { atoi(s) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _atoi64(s: *const u8) -> i64 {
    let (v, neg, _, _) = parse_int(unsafe { cstr(s) }, 10, true);
    if neg { (v as i64).wrapping_neg() } else { v as i64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atoll(s: *const u8) -> i64 {
    unsafe { _atoi64(s) }
}

/// Wide forms through narrow copies (digits are ASCII).
unsafe fn wide_end<T>(w: *const u16, used_narrow: usize, end: *mut *const u16) {
    if !end.is_null() {
        unsafe { *end = w.add(used_narrow) };
    }
}

fn ascii_copy(w: &[u16]) -> Vec<u8> {
    let mut v: Vec<u8> = w.iter().map(|&c| if c < 128 { c as u8 } else { 0xff }).collect();
    v.push(0);
    v
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcstol(w: *const u16, end: *mut *const u16, base: i32) -> i32 {
    let a = ascii_copy(unsafe { wstr(w) });
    let mut e: *const u8 = core::ptr::null();
    let r = unsafe { strtol(a.as_ptr(), &mut e, base) };
    unsafe { wide_end::<()>(w, e as usize - a.as_ptr() as usize, end) };
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcstoul(w: *const u16, end: *mut *const u16, base: i32) -> u32 {
    let a = ascii_copy(unsafe { wstr(w) });
    let mut e: *const u8 = core::ptr::null();
    let r = unsafe { strtoul(a.as_ptr(), &mut e, base) };
    unsafe { wide_end::<()>(w, e as usize - a.as_ptr() as usize, end) };
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wtoi(w: *const u16) -> i32 {
    let a = ascii_copy(unsafe { wstr(w) });
    unsafe { atoi(a.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wtol(w: *const u16) -> i32 {
    unsafe { _wtoi(w) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcstod(w: *const u16, end: *mut *const u16) -> f64 {
    let a = ascii_copy(unsafe { wstr(w) });
    let mut e: *const u8 = core::ptr::null();
    let r = unsafe { crate::fmt::strtod_impl(a.as_ptr(), &mut e) };
    unsafe { wide_end::<()>(w, e as usize - a.as_ptr() as usize, end) };
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wtof(w: *const u16) -> f64 {
    unsafe { wcstod(w, core::ptr::null_mut()) }
}

fn to_radix(mut v: u64, neg: bool, radix: u32) -> Vec<u8> {
    let mut d = Vec::new();
    loop {
        d.push(core::char::from_digit((v % radix as u64) as u32, radix).unwrap_or('0') as u8);
        v /= radix as u64;
        if v == 0 {
            break;
        }
    }
    if neg {
        d.push(b'-');
    }
    d.reverse();
    d
}

unsafe fn put_digits(d: &[u8], buf: *mut u8) -> *mut u8 {
    unsafe {
        core::ptr::copy_nonoverlapping(d.as_ptr(), buf, d.len());
        *buf.add(d.len()) = 0;
    }
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _itoa(v: i32, buf: *mut u8, radix: i32) -> *mut u8 {
    let neg = radix == 10 && v < 0;
    let u = if neg { (v as i64).unsigned_abs() } else { v as u32 as u64 };
    unsafe { put_digits(&to_radix(u, neg, radix as u32), buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ltoa(v: i32, buf: *mut u8, radix: i32) -> *mut u8 {
    unsafe { _itoa(v, buf, radix) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ultoa(v: u32, buf: *mut u8, radix: i32) -> *mut u8 {
    unsafe { put_digits(&to_radix(v as u64, false, radix as u32), buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _i64toa(v: i64, buf: *mut u8, radix: i32) -> *mut u8 {
    let neg = radix == 10 && v < 0;
    let u = if neg { v.unsigned_abs() } else { v as u64 };
    unsafe { put_digits(&to_radix(u, neg, radix as u32), buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ui64toa(v: u64, buf: *mut u8, radix: i32) -> *mut u8 {
    unsafe { put_digits(&to_radix(v, false, radix as u32), buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _itow(v: i32, buf: *mut u16, radix: i32) -> *mut u16 {
    let neg = radix == 10 && v < 0;
    let u = if neg { (v as i64).unsigned_abs() } else { v as u32 as u64 };
    let d = to_radix(u, neg, radix as u32);
    for (i, &c) in d.iter().enumerate() {
        unsafe { *buf.add(i) = c as u16 };
    }
    unsafe { *buf.add(d.len()) = 0 };
    buf
}

#[unsafe(no_mangle)]
pub extern "C" fn abs(v: i32) -> i32 {
    v.wrapping_abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn labs(v: i32) -> i32 {
    v.wrapping_abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn llabs(v: i64) -> i64 {
    v.wrapping_abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn _abs64(v: i64) -> i64 {
    v.wrapping_abs()
}

/// div and ldiv return an 8-byte struct, in EDX:EAX on Windows.
#[unsafe(no_mangle)]
pub extern "C" fn div(a: i32, b: i32) -> u64 {
    let q = a.wrapping_div(b);
    let r = a.wrapping_rem(b);
    q as u32 as u64 | (r as u32 as u64) << 32
}

#[unsafe(no_mangle)]
pub extern "C" fn ldiv(a: i32, b: i32) -> u64 {
    div(a, b)
}

// ---- Sorting ----

unsafe fn swap(a: *mut u8, b: *mut u8, n: usize) {
    for i in 0..n {
        unsafe { core::ptr::swap(a.add(i), b.add(i)) };
    }
}

/// qsort: a heap sort (no recursion, no extra memory); the comparison
/// is the program's cdecl function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qsort(base: *mut u8, n: usize, size: usize, cmp: u32) {
    if n < 2 || size == 0 {
        return;
    }
    let at = |i: usize| unsafe { base.add(i * size) };
    let less = |a: usize, b: usize| (crate::call(cmp, at(a) as u32, at(b) as u32, 0, 0) as i32) < 0;
    let sift = |mut root: usize, end: usize| {
        loop {
            let mut child = 2 * root + 1;
            if child >= end {
                break;
            }
            if child + 1 < end && less(child, child + 1) {
                child += 1;
            }
            if less(root, child) {
                unsafe { swap(at(root), at(child), size) };
                root = child;
            } else {
                break;
            }
        }
    };
    for i in (0..n / 2).rev() {
        sift(i, n);
    }
    for end in (1..n).rev() {
        unsafe { swap(at(0), at(end), size) };
        sift(0, end);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qsort_s(base: *mut u8, n: usize, size: usize, cmp: u32, ctx: u32) {
    if n < 2 || size == 0 {
        return;
    }
    let at = |i: usize| unsafe { base.add(i * size) };
    // Insertion sort: qsort_s is rare.
    for i in 1..n {
        let mut j = i;
        while j > 0 && (crate::call(cmp, ctx, at(j - 1) as u32, at(j) as u32, 0) as i32) > 0 {
            unsafe { swap(at(j - 1), at(j), size) };
            j -= 1;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bsearch(key: *const u8, base: *const u8, n: usize, size: usize, cmp: u32) -> *const u8 {
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let p = unsafe { base.add(mid * size) };
        let r = crate::call(cmp, key as u32, p as u32, 0, 0) as i32;
        if r == 0 {
            return p;
        }
        if r < 0 {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _lfind(key: *const u8, base: *const u8, n: *const u32, size: u32, cmp: u32) -> *const u8 {
    for i in 0..unsafe { *n } {
        let p = unsafe { base.add((i * size) as usize) };
        if crate::call(cmp, key as u32, p as u32, 0, 0) == 0 {
            return p;
        }
    }
    core::ptr::null()
}

// ---- Random numbers (Microsoft's LCG, per process) ----

static SEED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

#[unsafe(no_mangle)]
pub extern "C" fn srand(s: u32) {
    SEED.store(s, core::sync::atomic::Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn rand() -> i32 {
    let s = SEED.load(core::sync::atomic::Ordering::Relaxed).wrapping_mul(214013).wrapping_add(2531011);
    SEED.store(s, core::sync::atomic::Ordering::Relaxed);
    ((s >> 16) & 0x7fff) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rand_s(out: *mut u32) -> i32 {
    let mut t = 0u64;
    unsafe { core::arch::asm!("rdtsc", out("eax") _, out("edx") _, options(nomem, nostack)) };
    let mut ft = 0u64;
    unsafe { crate::k::GetSystemTimeAsFileTime(&mut ft) };
    t ^= ft.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let r = (t >> 32) as u32 ^ rand() as u32 ^ (rand() as u32) << 15 ^ (rand() as u32) << 30;
    unsafe { *out = r };
    0
}
