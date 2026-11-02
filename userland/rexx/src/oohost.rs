//! Object REXX's part of REXX.DLL (built as OREXX.DLL): error reports in
//! Object REXX's form, ::REQUIRES files, REXXC's tokenized programs and
//! RexxTranslateProgram, and the bridges to other object systems:
//! REXXSOM.DLL (SOM classes, `.SOM`) and REXXWPS.DLL (the Workplace
//! Shell, `.WPS` and the WP classes), loaded when first needed.
//!
//! A bridge DLL exports (by name, rs2's own interface):
//! `rs2_ext_lookup(name, *handle, *is_class, id, cbid) -> 1 if known`,
//! `rs2_ext_class(spec, *handle, id, cbid) -> 1 if known`,
//! `rs2_ext_send(handle, msg, argc, *ExtVal, *ExtVal) -> 0 done, 1 no
//! such method, 2 error (result: number in `handle`, text in the string)`
//! and `rs2_ext_free(p)` for the strings it returns.

use alloc::vec::Vec;
use core::ptr::null_mut;

use os2::*;
use rexx::{ExtArg, ExtErr};

use crate::host::message_text;

/// The marker of a program REXXC made: the first line is a comment so
/// that CMD runs it as REXX; Ctrl+Z ends the text for anything that
/// shows it.
pub const TOKEN_MARK: &[u8] = b"/**/\x1aRS2 OBJECT REXX 1\x00";

/// REXXC's form of a program: the source, scrambled.
pub fn tokenize(src: &[u8]) -> Vec<u8> {
    let mut v = TOKEN_MARK.to_vec();
    let mut k: u8 = 0x5a;
    for &c in src {
        v.push(c ^ k);
        k = k.wrapping_mul(13).wrapping_add(7);
    }
    v
}

/// The source of a program REXXC made, if it is one.
pub fn untokenize(data: &[u8]) -> Option<Vec<u8>> {
    let body = data.strip_prefix(TOKEN_MARK)?;
    let mut v = Vec::with_capacity(body.len());
    let mut k: u8 = 0x5a;
    for &c in body {
        v.push(c ^ k);
        k = k.wrapping_mul(13).wrapping_add(7);
    }
    Some(v)
}

fn word(num: u32, def: &str) -> Vec<u8> {
    message_text(num).unwrap_or_else(|| def.as_bytes().to_vec())
}

/// `REX0042E: Error 42 running C:\X.CMD line 8:  text`
pub fn error_report(code: u32, file: &[u8], line: u32) -> Vec<u8> {
    let mut v = alloc::format!("REX{code:04}E: ").into_bytes();
    v.extend_from_slice(&word(524, "Error"));
    v.extend_from_slice(alloc::format!(" {code} ").as_bytes());
    v.extend_from_slice(&word(525, "running"));
    v.push(b' ');
    v.extend_from_slice(file);
    v.push(b' ');
    v.extend_from_slice(&word(526, "line"));
    v.extend_from_slice(alloc::format!(" {line}:  ").as_bytes());
    v.extend_from_slice(&message_text(code).unwrap_or_default());
    v.extend_from_slice(b"\r\n");
    v
}

/// `REX0413E: Error 42.3:  text`
pub fn detail_report(major: u32, minor: u32, text: &[u8]) -> Vec<u8> {
    let num = rexx::msgs::number(major * 1000 + minor).unwrap_or(major);
    let mut v = alloc::format!("REX{num:04}E: ").into_bytes();
    v.extend_from_slice(&word(524, "Error"));
    v.extend_from_slice(alloc::format!(" {major}.{minor}:  ").as_bytes());
    v.extend_from_slice(text);
    v.extend_from_slice(b"\r\n");
    v
}

// ---- RexxTranslateProgram (REXXC) ----

fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return b"";
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// RexxTranslateProgram(input, output, exits): check a program's syntax
/// and, with an output file, write its tokenized form. Returns 0, or the
/// error number (reported as an error of the program is).
pub extern "C" fn translate_program(input: *const u8, output: *const u8, _exits: *const u8) -> i32 {
    let inp = cstr(input).to_vec();
    let Some(path) = crate::host::full_name(&inp) else {
        let t = error_report(3, &inp, 0);
        unsafe { os2::msg::DosPutMessage(2, t.len() as u32, t.as_ptr()) };
        return 3;
    };
    let Some(src) = crate::host::read_file(&path) else { return 3 };
    let src = untokenize(&src).unwrap_or(src);
    if let Err((e, line)) = rexx::oo::load_package(&src, &path) {
        let code = match e {
            rexx::Err::Syntax(c) => c,
            _ => 49,
        };
        let major = if code >= 1000 { code / 1000 } else { code };
        let t = error_report(major, &path, line);
        unsafe { os2::msg::DosPutMessage(2, t.len() as u32, t.as_ptr()) };
        return major as i32;
    }
    if output.is_null() || cstr(output).is_empty() {
        return 0;
    }
    let out = tokenize(&src);
    let z = { let mut v = cstr(output).to_vec(); v.push(0); v };
    let (mut hf, mut act) = (0u32, 0u32);
    let rc = unsafe { DosOpen(z.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE, null_mut()) };
    if rc != 0 {
        return 3;
    }
    let mut n = 0u32;
    unsafe {
        DosWrite(hf, out.as_ptr(), out.len() as u32, &mut n);
        DosClose(hf);
    }
    0
}

// ---- the bridges ----

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExtVal {
    /// 0 none, 1 a string, 2 an object, 3 a class, 4 a REXX object (its
    /// number in `handle`).
    pub kind: u32,
    pub len: u32,
    pub ptr: *mut u8,
    pub handle: u32,
}

type LookupFn = extern "C" fn(*const u8, *mut u32, *mut u32, *mut u8, u32) -> u32;
type ClassFn = extern "C" fn(*const u8, *mut u32, *mut u8, u32) -> u32;
type SendFn = extern "C" fn(u32, *const u8, u32, *const ExtVal, *mut ExtVal) -> u32;
type FreeFn = extern "C" fn(*mut u8);

#[derive(Clone, Copy)]
struct Bridge {
    lookup: LookupFn,
    class: ClassFn,
    send: SendFn,
    free: FreeFn,
}

/// 0 not tried, 1 loaded, 2 not available.
static mut STATE: [u8; 3] = [0; 3];
static mut BRIDGES: [Option<Bridge>; 3] = [None; 3];

/// The bridge of kind 1 (SOM) or 2 (the Workplace Shell).
fn bridge(kind: u32) -> Option<Bridge> {
    let k = kind as usize;
    if k == 0 || k > 2 {
        return None;
    }
    unsafe {
        if STATE[k] == 0 {
            STATE[k] = 2;
            let names: &[&[u8]] = if k == 1 { &[b"REXXSOM\0", b"OREXXSOM\0"] } else { &[b"REXXWPS\0", b"OREXXWPS\0"] };
            for n in names {
                let mut fail = [0u8; 64];
                let mut hmod = 0u32;
                if DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, n.as_ptr(), &mut hmod) != 0 {
                    continue;
                }
                let get = |name: &[u8]| -> usize {
                    let mut p = 0usize;
                    if DosQueryProcAddr(hmod, 0, name.as_ptr(), &mut p) != 0 { 0 } else { p }
                };
                let (l, c, s, f) = (get(b"rs2_ext_lookup\0"), get(b"rs2_ext_class\0"), get(b"rs2_ext_send\0"), get(b"rs2_ext_free\0"));
                if l == 0 || c == 0 || s == 0 || f == 0 {
                    continue;
                }
                BRIDGES[k] = Some(Bridge {
                    lookup: core::mem::transmute::<usize, LookupFn>(l),
                    class: core::mem::transmute::<usize, ClassFn>(c),
                    send: core::mem::transmute::<usize, SendFn>(s),
                    free: core::mem::transmute::<usize, FreeFn>(f),
                });
                STATE[k] = 1;
                break;
            }
        }
        BRIDGES[k]
    }
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// An environment symbol of another object system: `.SOM` is SOM's;
/// `.WPS`, `.WPDESKTOP` and the Workplace Shell's classes the shell's.
pub fn ext_lookup(name: &[u8]) -> Option<(u32, usize, bool, Vec<u8>)> {
    let kinds: &[u32] = if name == b"SOM" {
        &[1]
    } else if name.starts_with(b"WP") {
        &[2]
    } else {
        &[]
    };
    for &k in kinds {
        let Some(b) = bridge(k) else { continue };
        let (mut h, mut c) = (0u32, 0u32);
        let mut id = [0u8; 256];
        let nz = z(name);
        if (b.lookup)(nz.as_ptr(), &mut h, &mut c, id.as_mut_ptr(), id.len() as u32) != 0 {
            return Some((k, h as usize, c != 0, cstr(id.as_ptr()).to_vec()));
        }
    }
    None
}

/// `::CLASS x EXTERNAL 'SOM name'` (or 'WPS name').
pub fn ext_class(spec: &[u8]) -> Option<(u32, usize, Vec<u8>)> {
    let up = spec.to_ascii_uppercase();
    let k = if up.starts_with(b"SOM ") {
        1
    } else if up.starts_with(b"WPS ") {
        2
    } else {
        return None;
    };
    let b = bridge(k)?;
    let rest = spec[4..].iter().skip_while(|&&c| c == b' ').copied().collect::<Vec<u8>>();
    let (mut h, mut id) = (0u32, [0u8; 256]);
    let nz = z(&rest);
    if (b.class)(nz.as_ptr(), &mut h, id.as_mut_ptr(), id.len() as u32) != 0 {
        return Some((k, h as usize, cstr(id.as_ptr()).to_vec()));
    }
    None
}

/// A message to an object of another system.
pub fn ext_send(kind: u32, handle: usize, msg: &[u8], args: &[ExtArg]) -> Result<ExtArg, ExtErr> {
    let Some(b) = bridge(kind) else { return Err(ExtErr::NoMethod) };
    // keep the argument strings alive for the call
    let mut keep: Vec<Vec<u8>> = Vec::new();
    let mut xs: Vec<ExtVal> = Vec::new();
    for a in args {
        let v = match a {
            ExtArg::None | ExtArg::Nil => ExtVal { kind: 0, len: 0, ptr: null_mut(), handle: 0 },
            ExtArg::Str(s) => {
                keep.push(z(s));
                let p = keep.last_mut().unwrap();
                ExtVal { kind: 1, len: s.len() as u32, ptr: p.as_mut_ptr(), handle: 0 }
            }
            ExtArg::Obj(k, h, c, id) => {
                if *k != kind {
                    // an object of another system: its string
                    keep.push(z(id));
                    let p = keep.last_mut().unwrap();
                    ExtVal { kind: 1, len: id.len() as u32, ptr: p.as_mut_ptr(), handle: 0 }
                } else {
                    keep.push(z(id));
                    let p = keep.last_mut().unwrap();
                    ExtVal { kind: if *c { 3 } else { 2 }, len: id.len() as u32, ptr: p.as_mut_ptr(), handle: *h as u32 }
                }
            }
            ExtArg::Rexx(i) => ExtVal { kind: 4, len: 0, ptr: null_mut(), handle: *i as u32 },
        };
        xs.push(v);
    }
    let mz = z(msg);
    let mut r = ExtVal { kind: 0, len: 0, ptr: null_mut(), handle: 0 };
    let rc = (b.send)(handle as u32, mz.as_ptr(), xs.len() as u32, xs.as_ptr(), &mut r);
    let s = if r.ptr.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(r.ptr, r.len as usize).to_vec() } };
    if !r.ptr.is_null() {
        (b.free)(r.ptr);
    }
    match rc {
        0 => Ok(match r.kind {
            1 => ExtArg::Str(s),
            2 => ExtArg::Obj(kind, r.handle as usize, false, s),
            3 => ExtArg::Obj(kind, r.handle as usize, true, s),
            4 => ExtArg::Rexx(r.handle as usize),
            5 => ExtArg::Nil,
            _ => ExtArg::None,
        }),
        1 => Err(ExtErr::NoMethod),
        _ => Err(ExtErr::Error(r.handle, s)),
    }
}
