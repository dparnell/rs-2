//! W32USER.DLL: USER32 of rs2's Win32 layer (docs/WIN32.md). Stage 1 has
//! what console programs import: the character calls, wsprintf and
//! MessageBox (on the console's standard error). Windows and messages
//! come in stage 2, on a core shared with WIN16.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;
mod thunks;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

unsafe extern "C" {
    #[link_name = "W32KERN.MultiByteToWideChar"]
    fn MultiByteToWideChar(cp: u32, flags: u32, s: *const u8, n: i32, d: *mut u16, size: i32) -> i32;
    #[link_name = "W32KERN.WideCharToMultiByte"]
    fn WideCharToMultiByte(cp: u32, flags: u32, s: *const u16, n: i32, d: *mut u8, size: i32, dflt: *const u8, used: *mut i32) -> i32;
}

/// A code page byte's character and back (the process code page).
fn to_char(b: u8) -> char {
    if b.is_ascii() {
        return b as char;
    }
    let mut w = 0u16;
    unsafe { MultiByteToWideChar(0, 0, &b, 1, &mut w, 1) };
    char::from_u32(w as u32).unwrap_or('?')
}

fn to_byte(c: char) -> Option<u8> {
    if c.is_ascii() {
        return Some(c as u8);
    }
    let w = c as u32;
    if w > 0xffff {
        return None;
    }
    let w = w as u16;
    let mut b = 0u8;
    let mut used = 0;
    unsafe { WideCharToMultiByte(0, 0, &w, 1, &mut b, 1, core::ptr::null(), &mut used) };
    (used == 0).then_some(b)
}

fn upper(b: u8) -> u8 {
    let mut u = to_char(b).to_uppercase();
    match (u.next(), u.next()) {
        (Some(c), None) => to_byte(c).unwrap_or(b),
        _ => b,
    }
}

fn lower(b: u8) -> u8 {
    let mut l = to_char(b).to_lowercase();
    match (l.next(), l.next()) {
        (Some(c), None) => to_byte(c).unwrap_or(b),
        _ => b,
    }
}

fn wcase(c: u16, up: bool) -> u16 {
    match char::from_u32(c as u32) {
        Some(ch) => {
            let mut it: alloc::vec::IntoIter<char> = if up { ch.to_uppercase().collect::<Vec<_>>() } else { ch.to_lowercase().collect::<Vec<_>>() }.into_iter();
            match (it.next(), it.next()) {
                (Some(x), None) if (x as u32) < 0x10000 => x as u16,
                _ => c,
            }
        }
        None => c,
    }
}

/// CharUpperA: a string in place, or (HIWORD 0) one character.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharUpperA(s: *mut u8) -> *mut u8 {
    if (s as u32) < 0x10000 {
        return upper(s as u32 as u8) as u32 as *mut u8;
    }
    let mut p = s;
    unsafe {
        while *p != 0 {
            *p = upper(*p);
            p = p.add(1);
        }
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharLowerA(s: *mut u8) -> *mut u8 {
    if (s as u32) < 0x10000 {
        return lower(s as u32 as u8) as u32 as *mut u8;
    }
    let mut p = s;
    unsafe {
        while *p != 0 {
            *p = lower(*p);
            p = p.add(1);
        }
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharUpperW(s: *mut u16) -> *mut u16 {
    if (s as u32) < 0x10000 {
        return wcase(s as u32 as u16, true) as u32 as *mut u16;
    }
    let mut p = s;
    unsafe {
        while *p != 0 {
            *p = wcase(*p, true);
            p = p.add(1);
        }
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharLowerW(s: *mut u16) -> *mut u16 {
    if (s as u32) < 0x10000 {
        return wcase(s as u32 as u16, false) as u32 as *mut u16;
    }
    let mut p = s;
    unsafe {
        while *p != 0 {
            *p = wcase(*p, false);
            p = p.add(1);
        }
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharUpperBuffA(s: *mut u8, n: u32) -> u32 {
    for i in 0..n as usize {
        unsafe { *s.add(i) = upper(*s.add(i)) };
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharLowerBuffA(s: *mut u8, n: u32) -> u32 {
    for i in 0..n as usize {
        unsafe { *s.add(i) = lower(*s.add(i)) };
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharNextA(s: *const u8) -> *const u8 {
    if unsafe { *s } == 0 { s } else { unsafe { s.add(1) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharPrevA(start: *const u8, s: *const u8) -> *const u8 {
    if s > start { unsafe { s.sub(1) } } else { start }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharNextW(s: *const u16) -> *const u16 {
    if unsafe { *s } == 0 { s } else { unsafe { s.add(1) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharPrevW(start: *const u16, s: *const u16) -> *const u16 {
    if s > start { unsafe { s.sub(1) } } else { start }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CharToOemA(s: *const u8, d: *mut u8) -> i32 {
    let mut i = 0;
    unsafe {
        loop {
            let c = *s.add(i);
            *d.add(i) = c;
            if c == 0 {
                break;
            }
            i += 1;
        }
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn OemToCharA(s: *const u8, d: *mut u8) -> i32 {
    unsafe { CharToOemA(s, d) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsCharAlphaA(c: u8) -> i32 {
    to_char(c).is_alphabetic() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsCharAlphaNumericA(c: u8) -> i32 {
    to_char(c).is_alphanumeric() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsCharUpperA(c: u8) -> i32 {
    to_char(c).is_uppercase() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsCharLowerA(c: u8) -> i32 {
    to_char(c).is_lowercase() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsCharAlphaW(c: u16) -> i32 {
    char::from_u32(c as u32).is_some_and(|c| c.is_alphabetic()) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsCharAlphaNumericW(c: u16) -> i32 {
    char::from_u32(c as u32).is_some_and(|c| c.is_alphanumeric()) as i32
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

unsafe fn wstr<'a>(p: *const u16) -> &'a [u16] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

fn narrow(w: &[u16]) -> Vec<u8> {
    w.iter().map(|&c| char::from_u32(c as u32).and_then(to_byte).unwrap_or(b'?')).collect()
}

/// MessageBoxA: the caption and text on standard error; the first button
/// is the answer (IDOK, or IDYES for MB_YESNO and MB_YESNOCANCEL).
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn MessageBoxA(_hwnd: u32, text: *const u8, caption: *const u8, ty: u32) -> i32 {
    let mut v = Vec::new();
    v.extend_from_slice(b"\r\n[");
    v.extend_from_slice(unsafe { cstr(caption) });
    v.extend_from_slice(b"] ");
    v.extend_from_slice(unsafe { cstr(text) });
    v.extend_from_slice(b"\r\n");
    let mut n = 0;
    unsafe { os2::DosWrite(2, v.as_ptr(), v.len() as u32, &mut n) };
    match ty & 0xf {
        3 | 4 => 6, // IDYES
        2 => 3,     // MB_ABORTRETRYIGNORE: IDABORT
        5 => 4,     // MB_RETRYCANCEL: IDRETRY
        _ => 1,     // IDOK
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn MessageBoxW(hwnd: u32, text: *const u16, caption: *const u16, ty: u32) -> i32 {
    let mut t = narrow(unsafe { wstr(text) });
    t.push(0);
    let mut c = narrow(unsafe { wstr(caption) });
    c.push(0);
    unsafe { MessageBoxA(hwnd, t.as_ptr(), c.as_ptr(), ty) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn MessageBoxExA(hwnd: u32, text: *const u8, caption: *const u8, ty: u32, _lang: u16) -> i32 {
    unsafe { MessageBoxA(hwnd, text, caption, ty) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBeep(_ty: u32) -> i32 {
    1
}

// ---- wsprintf ----

/// wvsprintfA(buf, fmt, args): %[-][0][width][.prec][l|h]{s,c,d,i,u,x,X,%}
/// (Windows' wsprintf has no floating point); at most 1024 characters.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn wvsprintfA(buf: *mut u8, fmt: *const u8, args: *const u32) -> i32 {
    let f = unsafe { cstr(fmt) };
    let out = unsafe { format(f, args, false) };
    let n = out.len().min(1024);
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn wvsprintfW(buf: *mut u16, fmt: *const u16, args: *const u32) -> i32 {
    let f: Vec<u8> = narrow(unsafe { wstr(fmt) });
    let out = unsafe { format(&f, args, true) };
    let n = out.len().min(1024);
    for (i, &b) in out[..n].iter().enumerate() {
        let mut w = b as u16;
        if !b.is_ascii() {
            unsafe { MultiByteToWideChar(0, 0, &b, 1, &mut w, 1) };
        }
        unsafe { *buf.add(i) = w };
    }
    unsafe { *buf.add(n) = 0 };
    n as i32
}

unsafe fn format(f: &[u8], mut args: *const u32, wide: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut next = || {
        let v = unsafe { *args };
        args = unsafe { args.add(1) };
        v
    };
    while i < f.len() {
        if f[i] != b'%' {
            out.push(f[i]);
            i += 1;
            continue;
        }
        i += 1;
        let (mut left, mut zero) = (false, false);
        while i < f.len() && (f[i] == b'-' || f[i] == b'0' || f[i] == b'#') {
            if f[i] == b'-' {
                left = true;
            } else if f[i] == b'0' {
                zero = true;
            }
            i += 1;
        }
        let mut width = 0usize;
        while i < f.len() && f[i].is_ascii_digit() {
            width = width * 10 + (f[i] - b'0') as usize;
            i += 1;
        }
        let mut prec: Option<usize> = None;
        if i < f.len() && f[i] == b'.' {
            i += 1;
            let mut p = 0;
            while i < f.len() && f[i].is_ascii_digit() {
                p = p * 10 + (f[i] - b'0') as usize;
                i += 1;
            }
            prec = Some(p);
        }
        let mut long_wide = false;
        while i < f.len() && matches!(f[i], b'l' | b'h' | b'L') {
            long_wide = f[i] == b'l';
            i += 1;
        }
        if i >= f.len() {
            break;
        }
        let c = f[i];
        i += 1;
        let s: Vec<u8> = match c {
            b'%' => alloc::vec![b'%'],
            b'c' | b'C' => alloc::vec![next() as u8],
            b's' | b'S' => {
                let p = next();
                // %s is the call's own width; %ls / %S the other one.
                let w = (wide && c == b's') || (!wide && (c == b'S' || long_wide));
                let mut v = if w { narrow(unsafe { wstr(p as *const u16) }) } else { unsafe { cstr(p as *const u8) }.to_vec() };
                if p == 0 {
                    v = b"(null)".to_vec();
                }
                if let Some(pr) = prec {
                    v.truncate(pr);
                }
                v
            }
            b'd' | b'i' => alloc::format!("{}", next() as i32).into_bytes(),
            b'u' => alloc::format!("{}", next()).into_bytes(),
            b'x' => alloc::format!("{:x}", next()).into_bytes(),
            b'X' => alloc::format!("{:X}", next()).into_bytes(),
            b'p' => alloc::format!("{:08X}", next()).into_bytes(),
            _ => alloc::vec![c],
        };
        let pad = width.saturating_sub(s.len());
        if left {
            out.extend_from_slice(&s);
            out.extend(core::iter::repeat_n(b' ', pad));
        } else if zero && matches!(c, b'd' | b'i' | b'u' | b'x' | b'X') {
            let (sign, digits) = if s.first() == Some(&b'-') { (&s[..1], &s[1..]) } else { (&s[..0], &s[..]) };
            out.extend_from_slice(sign);
            out.extend(core::iter::repeat_n(b'0', pad));
            out.extend_from_slice(digits);
        } else {
            out.extend(core::iter::repeat_n(b' ', pad));
            out.extend_from_slice(&s);
        }
    }
    out
}

// wsprintfA(buf, fmt, ...) and wsprintfW are cdecl with variable
// arguments: they pass the address of the arguments to wvsprintf.
core::arch::global_asm!(
    ".globl wsprintfA",
    "wsprintfA:",
    "lea eax, [esp+12]",
    "push eax",
    "push dword ptr [esp+12]",
    "push dword ptr [esp+12]",
    "call __w32t_wvsprintfA",
    "ret",
    ".globl wsprintfW",
    "wsprintfW:",
    "lea eax, [esp+12]",
    "push eax",
    "push dword ptr [esp+12]",
    "push dword ptr [esp+12]",
    "call __w32t_wvsprintfW",
    "ret",
);
