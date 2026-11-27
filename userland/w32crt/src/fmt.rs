//! The printf and scanf engines and strtod.
//!
//! printf works on code units: bytes for the narrow functions, UTF-16 for
//! the wide ones; floating point goes through Rust's exact conversions.
//! The old runtime writes three exponent digits (1.5e+003), the UCRT two
//! unless asked (its options word). A `va_list` is the address of the
//! arguments on the stack: words, 8 bytes for 64-bit integers and
//! doubles.

use alloc::string::String;
use alloc::vec::Vec;

use crate::stdio::{self, File};
use crate::{cstr, narrow, widen, wstr};

/// Reading arguments from a va_list.
pub struct Args(pub *const u32);

impl Args {
    pub fn u32(&mut self) -> u32 {
        let v = unsafe { self.0.read_unaligned() };
        self.0 = unsafe { self.0.add(1) };
        v
    }

    pub fn u64(&mut self) -> u64 {
        let lo = self.u32() as u64;
        let hi = self.u32() as u64;
        lo | hi << 32
    }

    pub fn f64(&mut self) -> f64 {
        f64::from_bits(self.u64())
    }
}

pub struct Opts {
    /// The wide functions (output and format are UTF-16).
    pub wide: bool,
    /// Three exponent digits (the old runtime).
    pub three: bool,
    /// In wide functions, %s is a wide string (Microsoft's legacy rule).
    pub legacy_wide: bool,
}

pub const MSVCRT: Opts = Opts { wide: false, three: true, legacy_wide: true };
pub const MSVCRT_W: Opts = Opts { wide: true, three: true, legacy_wide: true };

/// The UCRT's options word (_CRT_INTERNAL_PRINTF_*).
pub fn ucrt(options: u64, wide: bool) -> Opts {
    Opts { wide, three: options & 0x10 != 0, legacy_wide: options & 0x4 != 0 }
}

fn units_a(s: &[u8]) -> Vec<u16> {
    s.iter().map(|&b| b as u16).collect()
}

/// Exactly `prec` digits after the point of |x|.
fn fixed(x: f64, prec: usize) -> Vec<u8> {
    alloc::format!("{:.*}", prec, x.abs()).into_bytes()
}

/// |x| as d.ddd and a decimal exponent.
fn sci(x: f64, prec: usize) -> (Vec<u8>, i32) {
    let s = alloc::format!("{:.*e}", prec, x.abs());
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    (m.as_bytes().to_vec(), e.parse().unwrap_or(0))
}

fn exp_str(e: i32, upper: bool, three: bool) -> Vec<u8> {
    let mut v = alloc::vec![if upper { b'E' } else { b'e' }, if e < 0 { b'-' } else { b'+' }];
    let digits = if three { 3 } else { 2 };
    let d = alloc::format!("{:0width$}", e.unsigned_abs(), width = digits);
    v.extend_from_slice(d.as_bytes());
    v
}

fn strip_zeros(v: &mut Vec<u8>) {
    if v.contains(&b'.') {
        while v.last() == Some(&b'0') {
            v.pop();
        }
        if v.last() == Some(&b'.') {
            v.pop();
        }
    }
}

/// The body of a floating point conversion (no sign).
fn float_body(x: f64, conv: u8, prec: Option<usize>, alt: bool, three: bool) -> Vec<u8> {
    let upper = conv.is_ascii_uppercase();
    if x.is_nan() {
        return if upper { b"NAN".to_vec() } else { b"nan".to_vec() };
    }
    if x.is_infinite() {
        return if upper { b"INF".to_vec() } else { b"inf".to_vec() };
    }
    match conv | 0x20 {
        b'f' => {
            let mut v = fixed(x, prec.unwrap_or(6));
            if alt && !v.contains(&b'.') {
                v.push(b'.');
            }
            v
        }
        b'e' => {
            let (mut m, e) = sci(x, prec.unwrap_or(6));
            if alt && !m.contains(&b'.') {
                m.push(b'.');
            }
            m.extend(exp_str(e, upper, three));
            m
        }
        b'g' => {
            let p = match prec {
                Some(0) => 1,
                Some(p) => p,
                None => 6,
            };
            let (_, e) = sci(x, p - 1);
            let mut v = if x == 0.0 || (e >= -4 && e < p as i32) {
                fixed(x, (p as i32 - 1 - e).max(0) as usize)
            } else {
                let (mut m, e) = sci(x, p - 1);
                if !alt {
                    strip_zeros(&mut m);
                }
                m.extend(exp_str(e, upper, three));
                return m;
            };
            if !alt {
                strip_zeros(&mut v);
            }
            v
        }
        _ => {
            // %a: hexadecimal.
            let bits = x.abs().to_bits();
            let exp = ((bits >> 52) & 0x7ff) as i32;
            let mut mant = bits & ((1 << 52) - 1);
            let (lead, e) = if exp == 0 { (0, if mant == 0 { 0 } else { -1022 }) } else { (1, exp - 1023) };
            let mut digits = alloc::format!("{mant:013x}").into_bytes();
            match prec {
                Some(p) if p < 13 => {
                    // Round to p digits.
                    let shift = (13 - p) * 4;
                    let round = 1u64 << (shift - 1);
                    mant = (mant + round) >> shift;
                    digits = if p == 0 { Vec::new() } else { alloc::format!("{mant:0w$x}", w = p).into_bytes() };
                }
                Some(_) => {}
                None => {
                    while digits.last() == Some(&b'0') {
                        digits.pop();
                    }
                }
            }
            let mut v = alloc::format!("0x{lead}").into_bytes();
            if !digits.is_empty() || alt {
                v.push(b'.');
            }
            v.extend(digits);
            v.extend(alloc::format!("p{}{}", if e < 0 { '-' } else { '+' }, e.unsigned_abs()).into_bytes());
            if upper {
                v.make_ascii_uppercase();
            }
            v
        }
    }
}

/// Format `fmt` (code units) with `args`; returns the output's units.
pub fn format(fmt: &[u16], mut args: Args, o: &Opts) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    let mut i = 0;
    let at = |i: usize| fmt.get(i).copied().unwrap_or(0);
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' as u16 {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let (mut left, mut plus, mut space, mut zero, mut alt) = (false, false, false, false, false);
        loop {
            match at(i) as u8 {
                b'-' => left = true,
                b'+' => plus = true,
                b' ' => space = true,
                b'0' => zero = true,
                b'#' => alt = true,
                _ => break,
            }
            i += 1;
        }
        let mut width: usize = 0;
        if at(i) == b'*' as u16 {
            let w = args.u32() as i32;
            if w < 0 {
                left = true;
            }
            width = w.unsigned_abs() as usize;
            i += 1;
        } else {
            while (b'0' as u16..=b'9' as u16).contains(&at(i)) {
                width = width * 10 + (at(i) - b'0' as u16) as usize;
                i += 1;
            }
        }
        let mut prec: Option<usize> = None;
        if at(i) == b'.' as u16 {
            i += 1;
            if at(i) == b'*' as u16 {
                let p = args.u32() as i32;
                prec = if p < 0 { None } else { Some(p as usize) };
                i += 1;
            } else {
                let mut p = 0;
                while (b'0' as u16..=b'9' as u16).contains(&at(i)) {
                    p = p * 10 + (at(i) - b'0' as u16) as usize;
                    i += 1;
                }
                prec = Some(p);
            }
        }
        // Size: 1 = short/char, 2 = int, 4 = 64-bit; wide: 0 default,
        // 1 narrow (h), 2 wide (l, w).
        let mut big = false;
        let mut short = 0; // 1 h, 2 hh
        let mut charw = 0;
        loop {
            match at(i) as u8 {
                b'h' => {
                    short += 1;
                    charw = 1;
                }
                b'l' => {
                    if at(i + 1) == b'l' as u16 {
                        big = true;
                        i += 1;
                    }
                    charw = 2;
                }
                b'w' => charw = 2,
                b'L' | b'q' | b'j' => big = true,
                b'z' | b't' => {}
                b'I' => {
                    if at(i + 1) == b'6' as u16 && at(i + 2) == b'4' as u16 {
                        big = true;
                        i += 2;
                    } else if at(i + 1) == b'3' as u16 && at(i + 2) == b'2' as u16 {
                        i += 2;
                    }
                }
                _ => break,
            }
            i += 1;
        }
        if i >= fmt.len() {
            break;
        }
        let conv = fmt[i] as u8;
        i += 1;
        let mut sign: Vec<u16> = Vec::new();
        let mut prefix: Vec<u16> = Vec::new();
        let body: Vec<u16>;
        let mut numeric = false;
        match conv {
            b'%' => body = alloc::vec![b'%' as u16],
            b'd' | b'i' => {
                let v: i64 = if big { args.u64() as i64 } else { args.u32() as i32 as i64 };
                let v = match short {
                    1 => v as i16 as i64,
                    s if s >= 2 => v as i8 as i64,
                    _ => v,
                };
                if v < 0 {
                    sign.push(b'-' as u16);
                } else if plus {
                    sign.push(b'+' as u16);
                } else if space {
                    sign.push(b' ' as u16);
                }
                let mut d = alloc::format!("{}", v.unsigned_abs()).into_bytes();
                if let Some(p) = prec {
                    if p == 0 && v == 0 {
                        d.clear();
                    }
                    while d.len() < p {
                        d.insert(0, b'0');
                    }
                }
                body = units_a(&d);
                numeric = prec.is_none();
            }
            b'u' | b'o' | b'x' | b'X' => {
                let v: u64 = if big { args.u64() } else { args.u32() as u64 };
                let v = match short {
                    1 => v as u16 as u64,
                    s if s >= 2 => v as u8 as u64,
                    _ => v,
                };
                let mut d = match conv {
                    b'u' => alloc::format!("{v}"),
                    b'o' => alloc::format!("{v:o}"),
                    b'x' => alloc::format!("{v:x}"),
                    _ => alloc::format!("{v:X}"),
                }
                .into_bytes();
                if let Some(p) = prec {
                    if p == 0 && v == 0 {
                        d.clear();
                    }
                    while d.len() < p {
                        d.insert(0, b'0');
                    }
                }
                if alt && v != 0 {
                    match conv {
                        b'o' if d.first() != Some(&b'0') => d.insert(0, b'0'),
                        b'x' => prefix = units_a(b"0x"),
                        b'X' => prefix = units_a(b"0X"),
                        _ => {}
                    }
                }
                body = units_a(&d);
                numeric = prec.is_none();
            }
            b'p' => {
                body = units_a(alloc::format!("{:08X}", args.u32()).as_bytes());
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
                let x = args.f64();
                if x.is_sign_negative() && !x.is_nan() {
                    sign.push(b'-' as u16);
                } else if plus {
                    sign.push(b'+' as u16);
                } else if space {
                    sign.push(b' ' as u16);
                }
                body = units_a(&float_body(x, conv, prec, alt, o.three));
                numeric = x.is_finite();
            }
            b'c' | b'C' => {
                let v = args.u32();
                // %c is the call's own width; %lc, %wc, %C (in narrow
                // calls) wide; %hc, %C (in wide calls) narrow.
                let wide_arg = match (conv, charw) {
                    (_, 2) => true,
                    (_, 1) => false,
                    (b'C', _) => !o.wide,
                    _ => o.wide,
                };
                body = if o.wide {
                    if wide_arg { alloc::vec![v as u16] } else { widen(&[v as u8]) }
                } else if wide_arg {
                    units_a(&narrow(&[v as u16]))
                } else {
                    alloc::vec![v as u8 as u16]
                };
            }
            b's' | b'S' | b'Z' => {
                let p = args.u32();
                let wide_arg = match (conv, charw) {
                    (_, 2) => true,
                    (_, 1) => false,
                    (b'S', _) => !o.wide,
                    _ => o.wide && o.legacy_wide,
                };
                let mut s: Vec<u16> = if p == 0 {
                    units_a(b"(null)")
                } else if wide_arg {
                    let w = unsafe { wstr(p as *const u16) };
                    if o.wide { w.to_vec() } else { units_a(&narrow(w)) }
                } else {
                    let b = unsafe { cstr(p as *const u8) };
                    if o.wide { widen(b) } else { units_a(b) }
                };
                if let Some(pr) = prec {
                    s.truncate(pr);
                }
                body = s;
            }
            b'n' => {
                let p = args.u32();
                if p != 0 {
                    unsafe { *(p as *mut i32) = out.len() as i32 };
                }
                continue;
            }
            other => {
                body = alloc::vec![b'%' as u16, other as u16];
            }
        }
        let len = sign.len() + prefix.len() + body.len();
        let pad = width.saturating_sub(len);
        if left {
            out.extend(&sign);
            out.extend(&prefix);
            out.extend(&body);
            out.extend(core::iter::repeat_n(b' ' as u16, pad));
        } else if zero && numeric {
            out.extend(&sign);
            out.extend(&prefix);
            out.extend(core::iter::repeat_n(b'0' as u16, pad));
            out.extend(&body);
        } else {
            out.extend(core::iter::repeat_n(b' ' as u16, pad));
            out.extend(&sign);
            out.extend(&prefix);
            out.extend(&body);
        }
    }
    out
}

pub fn format_a(fmt: *const u8, args: *const u32, o: &Opts) -> Vec<u8> {
    let f = units_a(unsafe { cstr(fmt) });
    format(&f, Args(args), o).into_iter().map(|u| u as u8).collect()
}

pub fn format_w(fmt: *const u16, args: *const u32, o: &Opts) -> Vec<u16> {
    format(unsafe { wstr(fmt) }, Args(args), o)
}

// ---- The narrow printf functions ----

/// vsnprintf with C99 semantics: as much as fits with a NUL; the full
/// length.
unsafe fn put_c99(out: &[u8], buf: *mut u8, n: usize) -> i32 {
    if n > 0 && !buf.is_null() {
        let k = out.len().min(n - 1);
        unsafe {
            core::ptr::copy_nonoverlapping(out.as_ptr(), buf, k);
            *buf.add(k) = 0;
        }
    }
    out.len() as i32
}

/// _vsnprintf: -1 if it does not fit (then n bytes, no NUL).
unsafe fn put_ms(out: &[u8], buf: *mut u8, n: usize) -> i32 {
    if out.len() > n {
        unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), buf, n) };
        return -1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), buf, out.len());
        if out.len() < n {
            *buf.add(out.len()) = 0;
        }
    }
    out.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vsprintf(buf: *mut u8, fmt: *const u8, va: *const u32) -> i32 {
    let out = format_a(fmt, va, &MSVCRT);
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), buf, out.len());
        *buf.add(out.len()) = 0;
    }
    out.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _vsnprintf(buf: *mut u8, n: usize, fmt: *const u8, va: *const u32) -> i32 {
    let out = format_a(fmt, va, &MSVCRT);
    unsafe { put_ms(&out, buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vsnprintf(buf: *mut u8, n: usize, fmt: *const u8, va: *const u32) -> i32 {
    let out = format_a(fmt, va, &MSVCRT);
    unsafe { put_c99(&out, buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _vscprintf(fmt: *const u8, va: *const u32) -> i32 {
    format_a(fmt, va, &MSVCRT).len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vfprintf(f: *mut File, fmt: *const u8, va: *const u32) -> i32 {
    let out = format_a(fmt, va, &MSVCRT);
    stdio::write_all(f, &out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vprintf(fmt: *const u8, va: *const u32) -> i32 {
    unsafe { vfprintf(stdio::stream(1), fmt, va) }
}

// ---- The wide printf functions ----

unsafe fn put_wc99(out: &[u16], buf: *mut u16, n: usize) -> i32 {
    if n > 0 && !buf.is_null() {
        let k = out.len().min(n - 1);
        unsafe {
            core::ptr::copy_nonoverlapping(out.as_ptr(), buf, k);
            *buf.add(k) = 0;
        }
    }
    out.len() as i32
}

unsafe fn put_wms(out: &[u16], buf: *mut u16, n: usize) -> i32 {
    if out.len() > n {
        unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), buf, n) };
        return -1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), buf, out.len());
        if out.len() < n {
            *buf.add(out.len()) = 0;
        }
    }
    out.len() as i32
}

/// The old runtime's vswprintf has no count (a C99 one is _vswprintf_c).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vswprintf(buf: *mut u16, fmt: *const u16, va: *const u32) -> i32 {
    let out = format_w(fmt, va, &MSVCRT_W);
    unsafe { put_wms(&out, buf, usize::MAX) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _vsnwprintf(buf: *mut u16, n: usize, fmt: *const u16, va: *const u32) -> i32 {
    let out = format_w(fmt, va, &MSVCRT_W);
    unsafe { put_wms(&out, buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _vswprintf_c(buf: *mut u16, n: usize, fmt: *const u16, va: *const u32) -> i32 {
    let out = format_w(fmt, va, &MSVCRT_W);
    unsafe { put_wc99(&out, buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _vscwprintf(fmt: *const u16, va: *const u32) -> i32 {
    format_w(fmt, va, &MSVCRT_W).len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vfwprintf(f: *mut File, fmt: *const u16, va: *const u32) -> i32 {
    let out = format_w(fmt, va, &MSVCRT_W);
    let b = narrow(&out);
    stdio::write_all(f, &b);
    out.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vwprintf(fmt: *const u16, va: *const u32) -> i32 {
    unsafe { vfwprintf(stdio::stream(1), fmt, va) }
}

// ---- The UCRT's printf entry points ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vfprintf(options: u64, f: *mut File, fmt: *const u8, _locale: u32, va: *const u32) -> i32 {
    let out = format_a(fmt, va, &ucrt(options, false));
    stdio::write_all(f, &out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vfprintf_s(options: u64, f: *mut File, fmt: *const u8, locale: u32, va: *const u32) -> i32 {
    unsafe { __stdio_common_vfprintf(options, f, fmt, locale, va) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vfprintf_p(options: u64, f: *mut File, fmt: *const u8, locale: u32, va: *const u32) -> i32 {
    unsafe { __stdio_common_vfprintf(options, f, fmt, locale, va) }
}

/// __stdio_common_vsprintf: with _CRT_INTERNAL_PRINTF_STANDARD_SNPRINTF_
/// BEHAVIOR (2), snprintf's rules; else _vsnprintf's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vsprintf(options: u64, buf: *mut u8, n: usize, fmt: *const u8, _locale: u32, va: *const u32) -> i32 {
    let out = format_a(fmt, va, &ucrt(options, false));
    if buf.is_null() {
        return out.len() as i32;
    }
    if options & 2 != 0 { unsafe { put_c99(&out, buf, n) } } else { unsafe { put_ms(&out, buf, n) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vsprintf_s(options: u64, buf: *mut u8, n: usize, fmt: *const u8, _locale: u32, va: *const u32) -> i32 {
    let out = format_a(fmt, va, &ucrt(options, false));
    if out.len() >= n {
        if n > 0 {
            unsafe { *buf = 0 };
        }
        crate::set_errno(crate::ERANGE);
        return -1;
    }
    unsafe { put_c99(&out, buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vsnprintf_s(options: u64, buf: *mut u8, n: usize, max: usize, fmt: *const u8, _locale: u32, va: *const u32) -> i32 {
    let mut out = format_a(fmt, va, &ucrt(options, false));
    let truncated = out.len() > max || out.len() >= n;
    out.truncate(max.min(n.saturating_sub(1)));
    unsafe { put_c99(&out, buf, n) };
    if truncated { -1 } else { out.len() as i32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vsprintf_p(options: u64, buf: *mut u8, n: usize, fmt: *const u8, locale: u32, va: *const u32) -> i32 {
    unsafe { __stdio_common_vsprintf(options | 2, buf, n, fmt, locale, va) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vfwprintf(options: u64, f: *mut File, fmt: *const u16, _locale: u32, va: *const u32) -> i32 {
    let out = format_w(fmt, va, &ucrt(options, true));
    stdio::write_all(f, &narrow(&out));
    out.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vfwprintf_s(options: u64, f: *mut File, fmt: *const u16, locale: u32, va: *const u32) -> i32 {
    unsafe { __stdio_common_vfwprintf(options, f, fmt, locale, va) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vswprintf(options: u64, buf: *mut u16, n: usize, fmt: *const u16, _locale: u32, va: *const u32) -> i32 {
    let out = format_w(fmt, va, &ucrt(options, true));
    if buf.is_null() {
        return out.len() as i32;
    }
    if options & 2 != 0 { unsafe { put_wc99(&out, buf, n) } } else { unsafe { put_wms(&out, buf, n) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vswprintf_s(options: u64, buf: *mut u16, n: usize, fmt: *const u16, _locale: u32, va: *const u32) -> i32 {
    let out = format_w(fmt, va, &ucrt(options, true));
    if out.len() >= n {
        if n > 0 {
            unsafe { *buf = 0 };
        }
        return -1;
    }
    unsafe { put_wc99(&out, buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vsnwprintf_s(options: u64, buf: *mut u16, n: usize, max: usize, fmt: *const u16, _locale: u32, va: *const u32) -> i32 {
    let mut out = format_w(fmt, va, &ucrt(options, true));
    let truncated = out.len() > max || out.len() >= n;
    out.truncate(max.min(n.saturating_sub(1)));
    unsafe { put_wc99(&out, buf, n) };
    if truncated { -1 } else { out.len() as i32 }
}

// ---- Numbers from text ----

/// The longest floating point number at the start of `s` (after white
/// space): its value and length, or None.
pub fn parse_float(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let start = i;
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let rest = &s[i..];
    let lower = |n: usize| rest.iter().take(n).map(|c| c.to_ascii_lowercase()).collect::<Vec<u8>>();
    if lower(8) == b"infinity" {
        return Some((if neg { f64::NEG_INFINITY } else { f64::INFINITY }, i + 8));
    }
    if lower(3) == b"inf" {
        return Some((if neg { f64::NEG_INFINITY } else { f64::INFINITY }, i + 3));
    }
    if lower(3) == b"nan" {
        let mut j = i + 3;
        if s.get(j) == Some(&b'(') {
            if let Some(k) = s[j..].iter().position(|&c| c == b')') {
                j += k + 1;
            }
        }
        return Some((f64::NAN, j));
    }
    // Hexadecimal.
    if rest.len() > 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'x' {
        let mut j = i + 2;
        let mut mant: u64 = 0;
        let mut exp: i32 = 0;
        let mut any = false;
        let mut seen_point = false;
        while j < s.len() {
            let c = s[j];
            if c == b'.' && !seen_point {
                seen_point = true;
                j += 1;
                continue;
            }
            let Some(d) = (c as char).to_digit(16) else { break };
            any = true;
            if mant < (1 << 59) {
                mant = mant * 16 + d as u64;
                if seen_point {
                    exp -= 4;
                }
            } else if !seen_point {
                exp += 4;
            }
            j += 1;
        }
        if !any {
            // Just the "0".
            return Some((if neg { -0.0 } else { 0.0 }, i + 1));
        }
        if j < s.len() && (s[j] | 0x20) == b'p' {
            let mut k = j + 1;
            let mut eneg = false;
            if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
                eneg = s[k] == b'-';
                k += 1;
            }
            let ds = k;
            let mut e: i32 = 0;
            while k < s.len() && s[k].is_ascii_digit() {
                e = e.saturating_mul(10).saturating_add((s[k] - b'0') as i32);
                k += 1;
            }
            if k > ds {
                exp = exp.saturating_add(if eneg { -e } else { e });
                j = k;
            }
        }
        let mut v = mant as f64;
        // Scale by 2^exp in steps that stay exact.
        let mut e = exp;
        while e > 0 {
            let step = e.min(1000);
            v *= f64::from_bits(((1023 + step) as u64) << 52);
            e -= step;
        }
        while e < 0 {
            let step = (-e).min(1000);
            v /= f64::from_bits(((1023 + step) as u64) << 52);
            e += step;
        }
        return Some((if neg { -v } else { v }, j));
    }
    let mut j = i;
    let mut digits = 0;
    while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    if j < s.len() && s[j] == b'.' {
        j += 1;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if j < s.len() && (s[j] | 0x20) == b'e' {
        let mut k = j + 1;
        if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
            k += 1;
        }
        if k < s.len() && s[k].is_ascii_digit() {
            while k < s.len() && s[k].is_ascii_digit() {
                k += 1;
            }
            j = k;
        }
    }
    let text: String = s[start..j].iter().map(|&c| c as char).collect();
    let text = text.strip_prefix('+').unwrap_or(&text);
    // Rust wants a digit before the point.
    let fixed: String = if text.starts_with('.') || text.starts_with("-.") {
        text.replacen('.', "0.", 1)
    } else {
        String::from(text)
    };
    let v: f64 = fixed.parse().ok()?;
    Some((v, j))
}

pub unsafe fn strtod_impl(s: *const u8, end: *mut *const u8) -> f64 {
    let b = unsafe { cstr(s) };
    match parse_float(b) {
        Some((v, n)) => {
            if !end.is_null() {
                unsafe { *end = s.add(n) };
            }
            if v.is_infinite() && !b.iter().any(|c| c.to_ascii_lowercase() == b'i') {
                crate::set_errno(crate::ERANGE);
            }
            v
        }
        None => {
            if !end.is_null() {
                unsafe { *end = s };
            }
            0.0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtod(s: *const u8, end: *mut *const u8) -> f64 {
    unsafe { strtod_impl(s, end) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtof(s: *const u8, end: *mut *const u8) -> f32 {
    unsafe { strtod_impl(s, end) as f32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtold(s: *const u8, end: *mut *const u8) -> f64 {
    unsafe { strtod_impl(s, end) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atof(s: *const u8) -> f64 {
    unsafe { strtod_impl(s, core::ptr::null_mut()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _atof_l(s: *const u8, _l: u32) -> f64 {
    unsafe { strtod_impl(s, core::ptr::null_mut()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strtod_l(s: *const u8, end: *mut *const u8, _l: u32) -> f64 {
    unsafe { strtod_impl(s, end) }
}

// ---- scanf ----

/// Where scanf reads from.
pub trait Input {
    fn get(&mut self) -> Option<u16>;
    fn unget(&mut self, c: u16);
}

pub struct StrInput<'a> {
    pub s: &'a [u16],
    pub i: usize,
}

impl Input for StrInput<'_> {
    fn get(&mut self) -> Option<u16> {
        let c = self.s.get(self.i).copied();
        if c.is_some() {
            self.i += 1;
        }
        c
    }

    fn unget(&mut self, _c: u16) {
        self.i -= 1;
    }
}

pub struct FileInput {
    pub f: *mut File,
}

impl Input for FileInput {
    fn get(&mut self) -> Option<u16> {
        let c = stdio::getc_impl(self.f);
        if c < 0 { None } else { Some(c as u16) }
    }

    fn unget(&mut self, c: u16) {
        stdio::ungetc_impl(c as i32, self.f);
    }
}

fn is_space(c: u16) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0a | 0x0b | 0x0c | 0x0d)
}

/// scanf: the number of assignments, or -1 if the input ended before the
/// first conversion.
pub fn scan(input: &mut dyn Input, fmt: &[u16], mut args: Args, wide: bool) -> i32 {
    let mut count = 0;
    let mut consumed = 0usize;
    let mut i = 0;
    let mut eof = false;
    let at = |i: usize| fmt.get(i).copied().unwrap_or(0);
    while i < fmt.len() {
        let c = fmt[i];
        if is_space(c) {
            while let Some(x) = input.get() {
                if !is_space(x) {
                    input.unget(x);
                    break;
                }
                consumed += 1;
            }
            i += 1;
            continue;
        }
        if c != b'%' as u16 || at(i + 1) == b'%' as u16 {
            if c == b'%' as u16 {
                i += 1;
            }
            match input.get() {
                Some(x) if x == fmt[i] => {
                    consumed += 1;
                    i += 1;
                    continue;
                }
                Some(x) => {
                    input.unget(x);
                    break;
                }
                None => {
                    eof = true;
                    break;
                }
            }
        }
        i += 1;
        let mut suppress = false;
        if at(i) == b'*' as u16 {
            suppress = true;
            i += 1;
        }
        let mut width = 0usize;
        while (b'0' as u16..=b'9' as u16).contains(&at(i)) {
            width = width * 10 + (at(i) - b'0' as u16) as usize;
            i += 1;
        }
        let (mut size, mut charw) = (0i32, 0); // size: -2 hh, -1 h, 1 l, 2 ll; charw 1 narrow 2 wide
        loop {
            match at(i) as u8 {
                b'h' => {
                    size = if size == -1 { -2 } else { -1 };
                    charw = 1;
                }
                b'l' => {
                    size += 1;
                    charw = 2;
                }
                b'L' | b'q' | b'j' => size = 2,
                b'w' => charw = 2,
                b'z' | b't' => {}
                b'I' => {
                    if at(i + 1) == b'6' as u16 && at(i + 2) == b'4' as u16 {
                        size = 2;
                        i += 2;
                    } else if at(i + 1) == b'3' as u16 && at(i + 2) == b'2' as u16 {
                        i += 2;
                    }
                }
                _ => break,
            }
            i += 1;
        }
        let conv = at(i) as u8;
        i += 1;
        if conv == b'n' {
            if !suppress {
                let p = args.u32();
                unsafe { *(p as *mut i32) = consumed as i32 };
            }
            continue;
        }
        // Leading white space, except for %c and %[.
        if conv != b'c' && conv != b'[' && conv != b'C' {
            while let Some(x) = input.get() {
                if !is_space(x) {
                    input.unget(x);
                    break;
                }
                consumed += 1;
            }
        }
        let maxw = if width == 0 { usize::MAX } else { width };
        match conv {
            b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'p' => {
                let base = match conv {
                    b'd' | b'u' => 10,
                    b'o' => 8,
                    b'x' | b'X' | b'p' => 16,
                    _ => 0,
                };
                let mut text: Vec<u8> = Vec::new();
                while text.len() < maxw {
                    let Some(x) = input.get() else {
                        eof = text.is_empty();
                        break;
                    };
                    let ch = if x < 128 { x as u8 } else { 0 };
                    let ok = match ch {
                        b'+' | b'-' => text.is_empty(),
                        b'x' | b'X' => (base == 16 || base == 0) && text.len() <= 2 && text.last() == Some(&b'0'),
                        _ => (ch as char).to_digit(if base == 0 { 16 } else { base }).is_some(),
                    };
                    if !ok {
                        input.unget(x);
                        break;
                    }
                    text.push(ch);
                }
                consumed += text.len();
                let digits = text.iter().any(|c| c.is_ascii_hexdigit());
                if !digits {
                    break;
                }
                text.push(0);
                let mut end: *const u8 = core::ptr::null();
                let v = unsafe { crate::string::strtoul_impl(text.as_ptr(), &mut end, base as i32, u64::MAX) };
                if !suppress {
                    let p = args.u32();
                    unsafe {
                        match size {
                            -2 => *(p as *mut u8) = v as u8,
                            -1 => *(p as *mut u16) = v as u16,
                            2 => (p as *mut u64).write_unaligned(v),
                            _ => *(p as *mut u32) = v as u32,
                        }
                    }
                    count += 1;
                }
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
                let mut text: Vec<u8> = Vec::new();
                while text.len() < maxw {
                    let Some(x) = input.get() else { break };
                    let ch = if x < 128 { x as u8 } else { 0 };
                    text.push(ch);
                    // Keep reading while what we have can still start a
                    // number.
                    let ok = ch != 0 && (b"+-.eEpPxXiInNfFaAtTyY".contains(&ch) || ch.is_ascii_hexdigit());
                    if !ok {
                        text.pop();
                        input.unget(x);
                        break;
                    }
                }
                let Some((v, n)) = parse_float(&text) else {
                    eof = text.is_empty();
                    break;
                };
                for &c in text[n..].iter().rev() {
                    input.unget(c as u16);
                }
                consumed += n;
                if !suppress {
                    let p = args.u32();
                    unsafe {
                        if size >= 1 {
                            (p as *mut f64).write_unaligned(v);
                        } else {
                            *(p as *mut f32) = v as f32;
                        }
                    }
                    count += 1;
                }
            }
            b's' | b'S' | b'c' | b'C' | b'[' => {
                let wide_dst = match (conv, charw) {
                    (_, 2) => true,
                    (_, 1) => false,
                    (b'S' | b'C', _) => !wide,
                    _ => wide,
                };
                let mut set: Vec<u16> = Vec::new();
                let mut negate = false;
                if conv == b'[' {
                    if at(i) == b'^' as u16 {
                        negate = true;
                        i += 1;
                    }
                    if at(i) == b']' as u16 {
                        set.push(b']' as u16);
                        i += 1;
                    }
                    while i < fmt.len() && fmt[i] != b']' as u16 {
                        if fmt[i] == b'-' as u16 && !set.is_empty() && at(i + 1) != b']' as u16 {
                            let (a, b) = (*set.last().unwrap(), at(i + 1));
                            for x in a..=b {
                                set.push(x);
                            }
                            i += 2;
                            continue;
                        }
                        set.push(fmt[i]);
                        i += 1;
                    }
                    i += 1;
                }
                let maxw = if (conv == b'c' || conv == b'C') && width == 0 { 1 } else { maxw };
                let mut got: Vec<u16> = Vec::new();
                while got.len() < maxw {
                    let Some(x) = input.get() else { break };
                    let ok = match conv {
                        b's' | b'S' => !is_space(x),
                        b'[' => set.contains(&x) != negate,
                        _ => true,
                    };
                    if !ok {
                        input.unget(x);
                        break;
                    }
                    got.push(x);
                }
                if got.is_empty() {
                    eof = true;
                    break;
                }
                consumed += got.len();
                if !suppress {
                    let p = args.u32();
                    let term = conv != b'c' && conv != b'C';
                    unsafe {
                        if wide_dst {
                            let w: Vec<u16> = if wide { got.clone() } else { widen(&got.iter().map(|&u| u as u8).collect::<Vec<u8>>()) };
                            core::ptr::copy_nonoverlapping(w.as_ptr(), p as *mut u16, w.len());
                            if term {
                                *(p as *mut u16).add(w.len()) = 0;
                            }
                        } else {
                            let b: Vec<u8> = if wide { narrow(&got) } else { got.iter().map(|&u| u as u8).collect() };
                            core::ptr::copy_nonoverlapping(b.as_ptr(), p as *mut u8, b.len());
                            if term {
                                *(p as *mut u8).add(b.len()) = 0;
                            }
                        }
                    }
                    count += 1;
                }
            }
            _ => break,
        }
    }
    if count == 0 && eof { -1 } else { count }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vsscanf(s: *const u8, fmt: *const u8, va: *const u32) -> i32 {
    let src = units_a(unsafe { cstr(s) });
    let f = units_a(unsafe { cstr(fmt) });
    scan(&mut StrInput { s: &src, i: 0 }, &f, Args(va), false)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vfscanf(file: *mut File, fmt: *const u8, va: *const u32) -> i32 {
    let f = units_a(unsafe { cstr(fmt) });
    scan(&mut FileInput { f: file }, &f, Args(va), false)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vscanf(fmt: *const u8, va: *const u32) -> i32 {
    unsafe { vfscanf(stdio::stream(0), fmt, va) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vswscanf(s: *const u16, fmt: *const u16, va: *const u32) -> i32 {
    let src = unsafe { wstr(s) };
    scan(&mut StrInput { s: src, i: 0 }, unsafe { wstr(fmt) }, Args(va), true)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vfwscanf(file: *mut File, fmt: *const u16, va: *const u32) -> i32 {
    scan(&mut FileInput { f: file }, unsafe { wstr(fmt) }, Args(va), true)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vsscanf(_options: u64, s: *const u8, n: usize, fmt: *const u8, _locale: u32, va: *const u32) -> i32 {
    let b = unsafe { cstr(s) };
    let b = &b[..b.len().min(n)];
    let src = units_a(b);
    let f = units_a(unsafe { cstr(fmt) });
    scan(&mut StrInput { s: &src, i: 0 }, &f, Args(va), false)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vfscanf(_options: u64, file: *mut File, fmt: *const u8, _locale: u32, va: *const u32) -> i32 {
    unsafe { vfscanf(file, fmt, va) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vswscanf(_options: u64, s: *const u16, n: usize, fmt: *const u16, _locale: u32, va: *const u32) -> i32 {
    let w = unsafe { wstr(s) };
    let w = &w[..w.len().min(n)];
    scan(&mut StrInput { s: w, i: 0 }, unsafe { wstr(fmt) }, Args(va), true)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stdio_common_vfwscanf(_options: u64, file: *mut File, fmt: *const u16, _locale: u32, va: *const u32) -> i32 {
    unsafe { vfwscanf(file, fmt, va) }
}
