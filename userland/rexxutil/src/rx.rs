//! What every function needs: the arguments, the result string, the
//! variable pool, number parsing the way IBM's REXXUTIL does it.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::*;

use crate::api::RexxVariablePool;

/// REXX error 40, "Incorrect call to routine".
pub const INVALID: u32 = 40;

/// RXSHV_ function codes and flags.
pub const SHV_SET: u8 = 0;
pub const SHV_SYSET: u8 = 3;
pub const SHV_SYFET: u8 = 4;
pub const SHV_SYDRO: u8 = 5;
pub const SHV_NEXTV: u8 = 6;
pub const SHV_LVAR: u8 = 2;
pub const SHV_BADN: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RxString {
    pub len: u32,
    pub ptr: *mut u8,
}

#[repr(C)]
pub struct ShvBlock {
    pub next: *mut ShvBlock,
    pub name: RxString,
    pub value: RxString,
    pub namelen: u32,
    pub valuelen: u32,
    pub code: u8,
    pub ret: u8,
}

impl ShvBlock {
    pub fn new(code: u8) -> ShvBlock {
        let z = RxString { len: 0, ptr: null_mut() };
        ShvBlock { next: null_mut(), name: z, value: z, namelen: 0, valuelen: 0, code, ret: 0 }
    }
}

/// A C string's length.
pub fn c_len(p: *const u8) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

/// The arguments of a call. REXX passes each as a counted string that is
/// also zero-terminated; an omitted argument has a null pointer.
pub struct Args {
    argv: &'static [RxString],
    queue: *const u8,
}

impl Args {
    pub fn new(argc: u32, argv: *const RxString, queue: *const u8) -> Args {
        let argv = if argv.is_null() || argc == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(argv, argc as usize) } };
        Args { argv, queue }
    }

    /// The current queue's name.
    pub fn queue(&self) -> *const u8 {
        self.queue
    }

    pub fn n(&self) -> usize {
        self.argv.len()
    }

    /// The argument was given (RXNULLSTRING false).
    pub fn exists(&self, i: usize) -> bool {
        i < self.argv.len() && !self.argv[i].ptr.is_null()
    }

    /// Given and not empty (RXVALIDSTRING).
    pub fn valid(&self, i: usize) -> bool {
        self.exists(i) && self.argv[i].len != 0
    }

    /// The argument's length.
    pub fn len(&self, i: usize) -> usize {
        if i < self.argv.len() { self.argv[i].len as usize } else { 0 }
    }

    pub fn ptr(&self, i: usize) -> *const u8 {
        if i < self.argv.len() { self.argv[i].ptr } else { null() }
    }

    /// The argument as a C string (up to its first zero), as IBM's code
    /// reads most of them.
    pub fn cstr(&self, i: usize) -> &'static [u8] {
        let p = self.ptr(i);
        if p.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(p, c_len(p)) }
    }

    /// The argument's bytes (its counted length).
    pub fn bytes(&self, i: usize) -> &'static [u8] {
        let p = self.ptr(i);
        if p.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(p, self.argv[i].len as usize) }
    }

    /// The first character (0 for an omitted or empty argument).
    pub fn first(&self, i: usize) -> u8 {
        self.cstr(i).first().copied().unwrap_or(0)
    }
}

/// The result string.
pub struct Ret(*mut RxString);

impl Ret {
    pub fn new(r: *mut RxString) -> Ret {
        Ret(r)
    }

    /// The size of the buffer the interpreter passed.
    pub fn cap(&self) -> usize {
        unsafe { (*self.0).len as usize }
    }

    /// Store `b`, in new memory when the buffer is too small (as IBM's
    /// code does; the interpreter frees it). False if that failed.
    pub fn set(&mut self, b: &[u8]) -> bool {
        unsafe {
            let r = &mut *self.0;
            if b.len() > r.len as usize || r.ptr.is_null() {
                let mut p: *mut u8 = null_mut();
                if DosAllocMem(&mut p, b.len().max(1) as u32, PAG_COMMIT | PAG_WRITE | PAG_READ) != 0 {
                    r.len = 0;
                    return false;
                }
                r.ptr = p;
            }
            core::ptr::copy_nonoverlapping(b.as_ptr(), r.ptr, b.len());
            // Zero-terminated when there is room, as sprintf leaves it.
            if b.len() < r.len as usize {
                *r.ptr.add(b.len()) = 0;
            }
            r.len = b.len() as u32;
        }
        true
    }

    /// A number in decimal ("%d" / "%ld").
    pub fn num(&mut self, n: i32) {
        self.set(&dec(n as i64));
    }

    /// An unsigned number ("%lu").
    pub fn unum(&mut self, n: u32) {
        self.set(&dec(n as i64));
    }

    /// "0" if `ok`, otherwise "-1" (the stem functions' convention).
    pub fn ok_or_minus1(&mut self, ok: bool) {
        self.set(if ok { b"0" } else { b"-1" });
    }

    /// "1" if `ok`, otherwise "0" (the Workplace functions' convention).
    pub fn bool(&mut self, ok: bool) {
        self.set(if ok { b"1" } else { b"0" });
    }

    /// Point the result at memory the caller allocated (DosAllocMem).
    pub fn give(&mut self, p: *mut u8, len: usize) {
        unsafe {
            (*self.0).ptr = p;
            (*self.0).len = len as u32;
        }
    }
}

/// Decimal digits of a number.
pub fn dec(n: i64) -> Vec<u8> {
    let mut v = Vec::new();
    let neg = n < 0;
    let mut u = n.unsigned_abs();
    loop {
        v.push(b'0' + (u % 10) as u8);
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if neg {
        v.push(b'-');
    }
    v.reverse();
    v
}

/// Decimal, right-aligned in `w` columns, optionally zero-padded.
pub fn dec_w(n: i64, w: usize, zero: bool) -> Vec<u8> {
    let d = dec(n);
    let mut v = Vec::new();
    if d.len() < w {
        if zero && n < 0 {
            v.push(b'-');
            v.resize(w - d.len() + 1, b'0');
            v.extend_from_slice(&d[1..]);
            return v;
        }
        v.resize(w - d.len(), if zero { b'0' } else { b' ' });
    }
    v.extend_from_slice(&d);
    v
}

/// Left-aligned in `w` columns.
pub fn left_w(s: &[u8], w: usize) -> Vec<u8> {
    let mut v = s.to_vec();
    if v.len() < w {
        v.resize(w, b' ');
    }
    v
}

/// IBM's string2long: an optional '-' and 1-9 digits, nothing else.
pub fn string2long(s: &[u8]) -> Option<i32> {
    let (neg, d) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    if d.is_empty() || d.len() > 9 || !d.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = d.iter().fold(0i32, |a, &c| a * 10 + (c - b'0') as i32);
    Some(if neg { -v } else { v })
}

/// IBM's string2ulong: 1-10 digits, nothing else.
pub fn string2ulong(s: &[u8]) -> Option<u32> {
    if s.is_empty() || s.len() > 10 || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0u32, |a, &c| a.wrapping_mul(10).wrapping_add((c - b'0') as u32)))
}

/// sscanf("%ld"): blanks, an optional sign, at least one digit; what
/// follows is ignored.
pub fn scan_long(s: &[u8]) -> Option<i32> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut v: i32 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((s[i] - b'0') as i32);
        i += 1;
    }
    if i == start {
        return None;
    }
    Some(if neg { v.wrapping_neg() } else { v })
}

/// IBM's case tables only map ASCII letters.
pub fn upper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}

pub fn lower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// rxstricmp: case-insensitive compare of C strings (signed characters).
pub fn stricmp(a: &[u8], b: &[u8]) -> i32 {
    let mut i = 0;
    while lower(at(a, i)) == lower(at(b, i)) && at(a, i) != 0 {
        i += 1;
    }
    lower(at(a, i)) as i8 as i32 - lower(at(b, i)) as i8 as i32
}

/// Equal ignoring case.
pub fn eqi(a: &[u8], b: &[u8]) -> bool {
    stricmp(a, b) == 0
}

/// rxstrnicmp as IBM wrote it: it stops after `n + 1` equal characters
/// and returns the difference of the next pair.
pub fn strnicmp(a: &[u8], b: &[u8], n: i32) -> i32 {
    let mut i: i32 = 0;
    loop {
        let (x, y) = (at(a, i as usize), at(b, i as usize));
        if lower(x) != lower(y) || x == 0 {
            break;
        }
        i += 1;
        if i > n {
            break;
        }
    }
    lower(at(a, i as usize)) as i8 as i32 - lower(at(b, i as usize)) as i8 as i32
}

/// A zero-terminated copy for passing to the API.
pub fn cz(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s);
    v.push(0);
    v
}

/// The name of a stem as the file functions build it: upper-cased, with
/// a period added.
pub fn stem_name(s: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = s.iter().map(|&c| upper(c)).collect();
    if v.last() != Some(&b'.') {
        v.push(b'.');
    }
    v
}

/// The stem name with a tail.
pub fn tail(stem: &[u8], t: &[u8]) -> Vec<u8> {
    let mut v = stem.to_vec();
    v.extend_from_slice(t);
    v
}

/// Run one request through the variable pool: (return code, shvret).
pub fn pool(code: u8, name: &[u8], value: *mut u8, vlen: u32, buflen: u32) -> (u32, ShvBlock) {
    let n = cz(name);
    let mut b = ShvBlock::new(code);
    b.name = RxString { len: name.len() as u32, ptr: n.as_ptr() as *mut u8 };
    b.namelen = name.len() as u32;
    b.value = RxString { len: vlen, ptr: value };
    b.valuelen = buflen;
    let rc = unsafe { RexxVariablePool(&mut b) };
    (rc, b)
}

/// Set a variable (RXSHV_SET or RXSHV_SYSET): the pool's return code.
pub fn set_var(code: u8, name: &[u8], value: &[u8]) -> u32 {
    pool(code, name, value.as_ptr() as *mut u8, value.len() as u32, value.len() as u32).0
}

/// Fetch a variable letting the interpreter allocate the value (which
/// is copied and freed): the return code and the value.
pub fn fetch_var(code: u8, name: &[u8]) -> (u32, Vec<u8>) {
    let (rc, b) = pool(code, name, null_mut(), 0, 0);
    let mut v = Vec::new();
    if rc == 0 && !b.value.ptr.is_null() {
        v.extend_from_slice(unsafe { core::slice::from_raw_parts(b.value.ptr, b.value.len as usize) });
    }
    if !b.value.ptr.is_null() {
        unsafe { DosFreeMem(b.value.ptr) };
    }
    (rc, v)
}

/// Fetch into a 255-byte buffer (how IBM reads `stem.0`).
pub fn fetch_small(code: u8, name: &[u8]) -> (u32, Vec<u8>) {
    let mut buf = [0u8; 256];
    let (rc, b) = pool(code, name, buf.as_mut_ptr(), 255, 255);
    let n = (b.value.len as usize).min(255);
    (rc, buf[..n].to_vec())
}
