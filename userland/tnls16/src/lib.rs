//! TNLS16.DLL: the 16-bit national-language helper of IBM's TCP/IP
//! applications (TFTP, INETD, LPD, TELNETD, BOOTP and the telnet daemon's
//! terminal DLLs import it by ordinal). rs2's version replaces IBM's,
//! which needs PMWIN's 16-bit entry points: message catalogs are never
//! found (`catgets` returns the program's built-in text), `xpg_printf`
//! formats like printf (with XPG `%n$` positions) to standard output, the
//! converters copy, the DBCS tables are empty. Entries are `_cdecl far`
//! (thunk16).

#![no_std]
#![no_main]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;
use thunk16::{far, Args};

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub static mut tnls16_dbcs_cp: u16 = 0;
#[unsafe(no_mangle)]
pub static mut tnls16_dbcs_vector: [u8; 256] = [0; 256];
#[unsafe(no_mangle)]
pub static mut tnls16_primary_cp: u16 = 850;
#[unsafe(no_mangle)]
pub static mut tnls16_dbcs_enabling: u16 = 0;
#[unsafe(no_mangle)]
pub static mut tnls16_default_code: u16 = 0;
/// What converter handles point at.
static mut CONVERTER: [u8; 16] = [0; 16];

thunk16::entries16! {
    dispatch = dispatch;
    t16_catclose = 16;
    t16_catopen = 17;
    t16_wnlcv_close = 19;
    t16_wnlcv = 23;
    t16_xpg_printf = 24;
    t16_catgets = 27;
    t16_wnlcv_open = 30;
    t16_wnlinitglobal = 35;
    t16_xpg_fprintf = 40;
    t16_xpg_sprintf = 8;
}

extern "C" fn dispatch(ord: u32, args: *const u8) -> u32 {
    let a = Args(args);
    match ord {
        // catopen(name, oflag): no catalogs.
        17 => u32::MAX,
        16 | 19 | 35 => 0,
        // catgets(catd, set, num, default)
        27 => a.l(8),
        // wnlCv_open(from, to): a handle
        30 => far(core::ptr::addr_of!(CONVERTER) as *const u8),
        // wnlCv(handle, dst, src, len)
        23 => {
            let (dst, src, len) = (a.p(4), a.p(8), a.i(12));
            if dst.is_null() || src.is_null() || len <= 0 {
                return 0;
            }
            unsafe { core::ptr::copy(src, dst, len as usize) };
            len as u32
        }
        24 => {
            let out = format(a.p(0), Args(unsafe { args.add(4) }));
            write(1, &out);
            out.len() as u32
        }
        40 => {
            // fprintf(FILE far *, fmt, ...): the handle is the MSC 6
            // FILE's _file byte (offset 11, large model).
            let f = a.p(0);
            let h = if f.is_null() { 2 } else { (unsafe { *f.add(11) }) as u32 };
            let out = format(a.p(4), Args(unsafe { args.add(8) }));
            write(h, &out);
            out.len() as u32
        }
        8 => {
            // sprintf(buf, fmt, ...)
            let buf = a.p(0);
            let out = format(a.p(4), Args(unsafe { args.add(8) }));
            if !buf.is_null() {
                unsafe {
                    core::ptr::copy_nonoverlapping(out.as_ptr(), buf, out.len());
                    *buf.add(out.len()) = 0;
                }
            }
            out.len() as u32
        }
        _ => 0,
    }
}

fn write(h: u32, b: &[u8]) {
    let mut n = 0;
    unsafe { os2::DosWrite(h, b.as_ptr(), b.len() as u32, &mut n) };
}

/// One conversion of a format.
struct Spec {
    pos: Option<usize>,
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: Option<usize>,
    width_arg: Option<Option<usize>>,
    prec: Option<usize>,
    prec_arg: Option<Option<usize>>,
    long: bool,
    conv: u8,
}

/// Size of the 16-bit argument a conversion takes.
fn arg_size(s: &Spec) -> usize {
    match s.conv {
        b'd' | b'i' | b'o' | b'u' | b'x' | b'X' | b'c' => {
            if s.long {
                4
            } else {
                2
            }
        }
        b'e' | b'E' | b'f' | b'g' | b'G' => 8,
        b's' | b'p' | b'n' => 4,
        _ => 0,
    }
}

fn number(f: &[u8], i: &mut usize) -> Option<usize> {
    let start = *i;
    let mut v = 0usize;
    while *i < f.len() && f[*i].is_ascii_digit() {
        v = v * 10 + (f[*i] - b'0') as usize;
        *i += 1;
    }
    (*i > start).then_some(v)
}

/// Parse "%..." at f[i] (after the '%'); i ends after the conversion.
fn parse(f: &[u8], i: &mut usize) -> Spec {
    let mut s = Spec { pos: None, left: false, plus: false, space: false, alt: false, zero: false, width: None, width_arg: None, prec: None, prec_arg: None, long: false, conv: 0 };
    // "n$"
    let save = *i;
    if let Some(n) = number(f, i) {
        if f.get(*i) == Some(&b'$') {
            s.pos = Some(n);
            *i += 1;
        } else {
            *i = save;
        }
    }
    while let Some(&c) = f.get(*i) {
        match c {
            b'-' => s.left = true,
            b'+' => s.plus = true,
            b' ' => s.space = true,
            b'#' => s.alt = true,
            b'0' => s.zero = true,
            _ => break,
        }
        *i += 1;
    }
    let star = |i: &mut usize| -> Option<usize> {
        let save = *i;
        if let Some(n) = number(f, i) {
            if f.get(*i) == Some(&b'$') {
                *i += 1;
                return Some(n);
            }
        }
        *i = save;
        None
    };
    if f.get(*i) == Some(&b'*') {
        *i += 1;
        s.width_arg = Some(star(i));
    } else {
        s.width = number(f, i);
    }
    if f.get(*i) == Some(&b'.') {
        *i += 1;
        if f.get(*i) == Some(&b'*') {
            *i += 1;
            s.prec_arg = Some(star(i));
        } else {
            s.prec = Some(number(f, i).unwrap_or(0));
        }
    }
    while let Some(&c) = f.get(*i) {
        match c {
            b'l' | b'L' => s.long = true,
            b'h' | b'F' | b'N' => {}
            _ => break,
        }
        *i += 1;
    }
    s.conv = f.get(*i).copied().unwrap_or(0);
    *i += 1;
    s
}

/// printf for 16-bit arguments.
fn format(fmt: *const u8, args: Args) -> Vec<u8> {
    let f = unsafe { cstr(fmt) };
    // First pass: the argument at each XPG position (sizes in order).
    let mut specs = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' && i + 1 < f.len() {
            i += 1;
            if f[i] == b'%' {
                i += 1;
                continue;
            }
            specs.push(parse(f, &mut i));
        } else {
            i += 1;
        }
    }
    let positional = specs.iter().any(|s| s.pos.is_some());
    // Positional: sizes of arguments 1..n.
    let mut sizes: Vec<usize> = Vec::new();
    let mut set = |n: usize, sz: usize| {
        if sizes.len() < n {
            sizes.resize(n, 2);
        }
        sizes[n - 1] = sz;
    };
    if positional {
        for s in &specs {
            if let Some(Some(n)) = s.width_arg {
                set(n, 2);
            }
            if let Some(Some(n)) = s.prec_arg {
                set(n, 2);
            }
            if let Some(n) = s.pos {
                set(n, arg_size(s));
            }
        }
    }
    let offset_of = |n: usize| -> usize { sizes.iter().take(n.saturating_sub(1)).sum() };
    let mut seq = 0usize;
    let mut out = Vec::new();
    let mut i = 0;
    while i < f.len() {
        let c = f[i];
        if c != b'%' || i + 1 >= f.len() {
            if c == b'\n' && out.last() != Some(&b'\r') {
                out.push(b'\r');
            }
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if f[i] == b'%' {
            out.push(b'%');
            i += 1;
            continue;
        }
        let s = parse(f, &mut i);
        let mut take = |pos: Option<usize>, size: usize| -> usize {
            let off = match pos {
                Some(n) if positional => offset_of(n),
                _ => {
                    let o = seq;
                    seq += size;
                    o
                }
            };
            off
        };
        let width = match s.width_arg {
            Some(p) => Some(args.i(take(p, 2)).unsigned_abs() as usize),
            None => s.width,
        };
        let prec = match s.prec_arg {
            Some(p) => Some(args.i(take(p, 2)).max(0) as usize),
            None => s.prec,
        };
        let size = arg_size(&s);
        let off = take(s.pos, size);
        let body: Vec<u8> = match s.conv {
            b'd' | b'i' => {
                let v: i64 = if s.long { args.l(off) as i32 as i64 } else { args.i(off) as i64 };
                let mut b = alloc::format!("{}", v.unsigned_abs()).into_bytes();
                if let Some(p) = prec {
                    while b.len() < p {
                        b.insert(0, b'0');
                    }
                }
                if v < 0 {
                    b.insert(0, b'-');
                } else if s.plus {
                    b.insert(0, b'+');
                } else if s.space {
                    b.insert(0, b' ');
                }
                b
            }
            b'u' | b'o' | b'x' | b'X' => {
                let v: u64 = if s.long { args.l(off) as u64 } else { args.w(off) as u64 };
                let mut b = match s.conv {
                    b'u' => alloc::format!("{v}"),
                    b'o' => alloc::format!("{v:o}"),
                    b'x' => alloc::format!("{v:x}"),
                    _ => alloc::format!("{v:X}"),
                }
                .into_bytes();
                if let Some(p) = prec {
                    while b.len() < p {
                        b.insert(0, b'0');
                    }
                }
                if s.alt && v != 0 && s.conv != b'u' {
                    let pre: &[u8] = match s.conv {
                        b'o' => b"0",
                        b'x' => b"0x",
                        _ => b"0X",
                    };
                    b.splice(0..0, pre.iter().copied());
                }
                b
            }
            b'c' => alloc::vec![args.w(off) as u8],
            b's' => {
                let p = args.p(off);
                let t = if p.is_null() { &b"(null)"[..] } else { unsafe { cstr(p) } };
                let n = prec.map_or(t.len(), |p| p.min(t.len()));
                t[..n].to_vec()
            }
            b'p' => {
                let v = args.l(off);
                alloc::format!("{:04X}:{:04X}", v >> 16, v & 0xffff).into_bytes()
            }
            b'n' => {
                let p = args.p(off);
                if !p.is_null() {
                    unsafe { (p as *mut i16).write_unaligned(out.len() as i16) };
                }
                Vec::new()
            }
            b'e' | b'E' | b'f' | b'g' | b'G' => {
                let v = f64::from_bits(args.l(off) as u64 | (args.l(off + 4) as u64) << 32);
                let p = prec.unwrap_or(6);
                // Fixed notation for all (enough for the programs' use).
                let mut b = alloc::vec::Vec::new();
                let neg = v < 0.0;
                let v = if neg { -v } else { v };
                let ip = v as u64;
                let mut frac = v - ip as f64;
                b.extend_from_slice(alloc::format!("{ip}").as_bytes());
                if p > 0 {
                    b.push(b'.');
                    for _ in 0..p.min(17) {
                        frac *= 10.0;
                        let d = frac as u8;
                        b.push(b'0' + d.min(9));
                        frac -= d as f64;
                    }
                }
                if neg {
                    b.insert(0, b'-');
                }
                b
            }
            _ => Vec::new(),
        };
        let w = width.unwrap_or(0);
        if body.len() < w {
            let pad = w - body.len();
            if s.left {
                out.extend_from_slice(&body);
                out.extend(core::iter::repeat_n(b' ', pad));
            } else if s.zero && !matches!(s.conv, b's' | b'c') {
                let sign = body.first().is_some_and(|c| matches!(c, b'-' | b'+' | b' '));
                if sign {
                    out.push(body[0]);
                }
                out.extend(core::iter::repeat_n(b'0', pad));
                out.extend_from_slice(&body[sign as usize..]);
            } else {
                out.extend(core::iter::repeat_n(b' ', pad));
                out.extend_from_slice(&body);
            }
        } else {
            out.extend_from_slice(&body);
        }
    }
    out
}

unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}
