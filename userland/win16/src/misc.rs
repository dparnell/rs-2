//! The rest of USER: the calls of dialog.rs, menu.rs and caret.rs, the
//! string functions (lstrcmp, AnsiUpper, wsprintf...), LoadString.

use alloc::vec::Vec;
use thunk16::{flat, Pascal};

use crate::{cstr, far_str};

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    if let Some(r) = crate::dialog::call(ord, a) {
        return r;
    }
    if let Some(r) = crate::menu::call(ord, a) {
        return r;
    }
    if let Some(r) = crate::clip::call(ord, a) {
        return r;
    }
    if let Some(r) = crate::caret::call(ord, a) {
        return r;
    }
    match ord {
        // WinHelp(hwnd, file, cmd, data): no help engine for Windows
        // help files.
        171 => {
            a.w();
            a.l();
            a.w();
            a.l();
            0
        }
        // LoadString(hinst, id, buf, max)
        176 => crate::res::user_load_string(a),
        // wvsprintf(buf, format, args)
        421 => {
            let (buf, fmt, args) = (a.p(), a.l(), a.p());
            sprintf(buf, far_str(fmt).unwrap_or(b""), args) as u32
        }
        // lstrcmp(a, b), lstrcmpi(a, b)
        430 | 471 => {
            let (x, y) = (a.l(), a.l());
            let (s, t) = (far_str(x).unwrap_or(b""), far_str(y).unwrap_or(b""));
            let o = if ord == 471 { s.to_ascii_lowercase().cmp(&t.to_ascii_lowercase()) } else { s.cmp(t) };
            match o {
                core::cmp::Ordering::Less => 0xffff,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            }
        }
        // AnsiUpper(s), AnsiLower(s): a string, or a character (selector
        // 0).
        431 | 432 => {
            let s = a.l();
            let up = ord == 431;
            if s >> 16 == 0 {
                let c = s as u8;
                return if up { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() } as u32;
            }
            let p = flat(s);
            let n = unsafe { cstr(p) }.len();
            for i in 0..n {
                unsafe { *p.add(i) = if up { (*p.add(i)).to_ascii_uppercase() } else { (*p.add(i)).to_ascii_lowercase() } };
            }
            s
        }
        // IsCharAlpha, IsCharAlphaNumeric, IsCharUpper, IsCharLower (c)
        433 => (a.w() as u8).is_ascii_alphabetic() as u32,
        434 => (a.w() as u8).is_ascii_alphanumeric() as u32,
        435 => (a.w() as u8).is_ascii_uppercase() as u32,
        436 => (a.w() as u8).is_ascii_lowercase() as u32,
        // AnsiUpperBuff(s, n), AnsiLowerBuff(s, n)
        437 | 438 => {
            let (p, n) = (a.p(), a.w());
            let n = if n == 0 { 0x10000 } else { n as usize };
            for i in 0..n {
                unsafe { *p.add(i) = if ord == 437 { (*p.add(i)).to_ascii_uppercase() } else { (*p.add(i)).to_ascii_lowercase() } };
            }
            n as u32
        }
        // AnsiNext(s), AnsiPrev(start, s)
        472 => {
            let s = a.l();
            if unsafe { *flat(s) } == 0 { s } else { s + 1 }
        }
        473 => {
            let (start, s) = (a.l(), a.l());
            if s > start { s - 1 } else { start }
        }
        _ => 0,
    }
}

/// wsprintf's format: %[-][#][0][width][.precision][l]type (d, i, u, x,
/// X, s, c, %), arguments 16-bit (32-bit with l), strings far pointers.
pub fn sprintf(buf: *mut u8, fmt: &[u8], args: *const u8) -> usize {
    let mut out: Vec<u8> = Vec::new();
    let mut ap = args;
    let mut take16 = || {
        let v = unsafe { (ap as *const u16).read_unaligned() };
        ap = unsafe { ap.add(2) };
        v
    };
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        i += 1;
        if c != b'%' {
            out.push(c);
            continue;
        }
        let (mut left, mut zero, mut alt) = (false, false, false);
        while i < fmt.len() && matches!(fmt[i], b'-' | b'0' | b'#') {
            match fmt[i] {
                b'-' => left = true,
                b'0' => zero = true,
                _ => alt = true,
            }
            i += 1;
        }
        let mut width = 0usize;
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            width = width * 10 + (fmt[i] - b'0') as usize;
            i += 1;
        }
        let mut prec = None;
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            let mut p = 0;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                p = p * 10 + (fmt[i] - b'0') as usize;
                i += 1;
            }
            prec = Some(p);
        }
        let mut long = false;
        if i < fmt.len() && (fmt[i] == b'l' || fmt[i] == b'L') {
            long = true;
            i += 1;
        }
        let Some(&t) = fmt.get(i) else { break };
        i += 1;
        let mut take = |long: bool| -> u32 {
            if long {
                let lo = take16() as u32;
                let hi = take16() as u32;
                hi << 16 | lo
            } else {
                take16() as u32
            }
        };
        let s: Vec<u8> = match t {
            b'd' | b'i' => {
                let v = if long { take(true) as i32 } else { take(false) as u16 as i16 as i32 };
                alloc::format!("{v}").into_bytes()
            }
            b'u' => alloc::format!("{}", take(long)).into_bytes(),
            b'x' | b'X' => {
                let v = take(long);
                let mut s = if t == b'x' { alloc::format!("{v:x}") } else { alloc::format!("{v:X}") };
                if alt && v != 0 {
                    s.insert_str(0, if t == b'x' { "0x" } else { "0X" });
                }
                s.into_bytes()
            }
            b'c' => alloc::vec![take(false) as u8],
            b's' => {
                let p = take(true);
                let s = far_str(p).unwrap_or(b"(null)");
                let n = prec.map_or(s.len(), |p| p.min(s.len()));
                s[..n].to_vec()
            }
            b'%' => alloc::vec![b'%'],
            other => alloc::vec![b'%', other],
        };
        let s = if matches!(t, b'd' | b'i' | b'u' | b'x' | b'X') {
            match prec {
                Some(p) if s.len() < p => {
                    let mut z = alloc::vec![b'0'; p - s.len()];
                    z.extend_from_slice(&s);
                    z
                }
                _ => s,
            }
        } else {
            s
        };
        let pad = width.saturating_sub(s.len());
        if left {
            out.extend_from_slice(&s);
            out.extend(core::iter::repeat_n(b' ', pad));
        } else if zero && matches!(t, b'd' | b'i' | b'u' | b'x' | b'X') {
            let (sign, digits) = if s.first() == Some(&b'-') { (&s[..1], &s[1..]) } else { (&s[..0], &s[..]) };
            out.extend_from_slice(sign);
            out.extend(core::iter::repeat_n(b'0', pad));
            out.extend_from_slice(digits);
        } else {
            out.extend(core::iter::repeat_n(b' ', pad));
            out.extend_from_slice(&s);
        }
    }
    if !buf.is_null() {
        let n = out.len().min(1024);
        unsafe {
            core::ptr::copy_nonoverlapping(out.as_ptr(), buf, n);
            *buf.add(n) = 0;
        }
    }
    out.len().min(1024)
}
