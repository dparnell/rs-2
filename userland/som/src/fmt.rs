//! printf for somPrintf, somVprintf and somLPrintf: C formats with the
//! arguments read from a va_list (32-bit words; doubles take two).

use alloc::vec::Vec;

use crate::sys::*;

fn num(out: &mut Vec<u8>, mut v: u64, base: u64, upper: bool) -> usize {
    let digits: &[u8] = if upper { b"0123456789ABCDEF" } else { b"0123456789abcdef" };
    let mut buf = [0u8; 24];
    let mut n = 0;
    loop {
        buf[n] = digits[(v % base) as usize];
        n += 1;
        v /= base;
        if v == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        out.push(buf[i]);
    }
    n
}

/// Format `fmt` with arguments from `ap`.
pub unsafe fn format(fmt: *const u8, mut ap: *const u32) -> Vec<u8> {
    let f = unsafe { cstr(fmt) };
    let mut out = Vec::new();
    let mut next = || {
        let v = unsafe { *ap };
        ap = unsafe { ap.add(1) };
        v
    };
    let mut i = 0;
    while i < f.len() {
        let c = f[i];
        i += 1;
        if c != b'%' {
            if c == b'\n' {
                out.push(b'\r');
            }
            out.push(c);
            continue;
        }
        let (mut left, mut zero, mut plus, mut space, mut alt) = (false, false, false, false, false);
        while i < f.len() {
            match f[i] {
                b'-' => left = true,
                b'0' => zero = true,
                b'+' => plus = true,
                b' ' => space = true,
                b'#' => alt = true,
                _ => break,
            }
            i += 1;
        }
        let mut width = 0usize;
        if i < f.len() && f[i] == b'*' {
            width = next() as i32 as usize;
            i += 1;
        }
        while i < f.len() && f[i].is_ascii_digit() {
            width = width * 10 + (f[i] - b'0') as usize;
            i += 1;
        }
        let mut prec: Option<usize> = None;
        if i < f.len() && f[i] == b'.' {
            i += 1;
            let mut p = 0;
            if i < f.len() && f[i] == b'*' {
                p = next() as usize;
                i += 1;
            }
            while i < f.len() && f[i].is_ascii_digit() {
                p = p * 10 + (f[i] - b'0') as usize;
                i += 1;
            }
            prec = Some(p);
        }
        while i < f.len() && matches!(f[i], b'l' | b'h' | b'L') {
            i += 1;
        }
        if i >= f.len() {
            break;
        }
        let conv = f[i];
        i += 1;
        let mut field = Vec::new();
        let mut numeric = true;
        match conv {
            b'd' | b'i' => {
                let v = next() as i32;
                if v < 0 {
                    field.push(b'-');
                } else if plus {
                    field.push(b'+');
                } else if space {
                    field.push(b' ');
                }
                num(&mut field, (v as i64).unsigned_abs(), 10, false);
            }
            b'u' => {
                num(&mut field, next() as u64, 10, false);
            }
            b'x' | b'X' => {
                if alt {
                    field.extend_from_slice(if conv == b'x' { b"0x" } else { b"0X" });
                }
                num(&mut field, next() as u64, 16, conv == b'X');
            }
            b'o' => {
                num(&mut field, next() as u64, 8, false);
            }
            b'p' => {
                let v = next();
                let mut tmp = Vec::new();
                num(&mut tmp, v as u64, 16, true);
                for _ in tmp.len()..8 {
                    field.push(b'0');
                }
                field.extend_from_slice(&tmp);
            }
            b'c' => {
                numeric = false;
                field.push(next() as u8);
            }
            b's' => {
                numeric = false;
                let p = next() as *const u8;
                let s: &[u8] = if p.is_null() { b"(null)" } else { unsafe { cstr(p) } };
                let s = match prec {
                    Some(p) => &s[..s.len().min(p)],
                    None => s,
                };
                field.extend_from_slice(s);
            }
            b'f' | b'e' | b'g' | b'E' | b'G' => {
                let lo = next() as u64;
                let hi = next() as u64;
                let v = f64::from_bits(hi << 32 | lo);
                if v < 0.0 {
                    field.push(b'-');
                }
                let a = if v < 0.0 { -v } else { v };
                let digits = prec.unwrap_or(6).min(9);
                let scale = 10u64.pow(digits as u32);
                let whole = a as u64;
                let frac = ((a - whole as f64) * scale as f64 + 0.5) as u64;
                num(&mut field, whole, 10, false);
                if digits > 0 {
                    field.push(b'.');
                    let mut t = Vec::new();
                    num(&mut t, frac.min(scale - 1), 10, false);
                    for _ in t.len()..digits {
                        field.push(b'0');
                    }
                    field.extend_from_slice(&t);
                }
            }
            b'%' => {
                out.push(b'%');
                continue;
            }
            _ => {
                out.push(b'%');
                out.push(conv);
                continue;
            }
        }
        if numeric && conv != b'f' {
            if let Some(p) = prec {
                let sign = matches!(field.first(), Some(b'-' | b'+' | b' ')) as usize;
                while field.len() - sign < p {
                    field.insert(sign, b'0');
                }
            }
        }
        if field.len() < width {
            let pad = width - field.len();
            if left {
                field.extend(core::iter::repeat_n(b' ', pad));
            } else if zero && numeric && prec.is_none() {
                let sign = matches!(field.first(), Some(b'-' | b'+' | b' ')) as usize;
                for _ in 0..pad {
                    field.insert(sign, b'0');
                }
            } else {
                for _ in 0..pad {
                    field.insert(0, b' ');
                }
            }
        }
        out.extend_from_slice(&field);
    }
    out
}
