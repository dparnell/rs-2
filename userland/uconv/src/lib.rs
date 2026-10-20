//! UCONV.DLL: conversion objects between a code page and UCS-2, on the
//! single-byte code page tables of rs2's NLS data (crates/uls). A code
//! set is named `IBM-nnn` (or by number); an empty name or one that is
//! only modifiers (`@path=no`, ...) means the process code page.
//! Characters a code page lacks become `?` (from UCS-2) or U+FFFD.

#![no_std]
#![no_main]
#![allow(non_snake_case, clippy::missing_safety_doc)]
// The exports take pointers from OS/2 callers, as the API defines them.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;
extern crate os2;

mod exports;

use alloc::boxed::Box;
use alloc::string::String;
use uls::{ULS_BADOBJECT, ULS_BUFFERFULL, ULS_INVALID, ULS_NOMEMORY, ULS_SUCCESS, ascii, from_ptr};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

const MAGIC: u32 = 0x5543_4f42;
const SUBCHAR: u8 = b'?';

struct Uconv {
    magic: u32,
    cp: u16,
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.291"]
    fn DosQueryCp(cb: u32, cps: *mut u32, actual: *mut u32) -> u32;
}

fn process_cp() -> u16 {
    let mut cps = [0u32; 4];
    let mut got = 0u32;
    if unsafe { DosQueryCp(16, cps.as_mut_ptr(), &mut got) } == 0 && cps[0] != 0 { cps[0] as u16 } else { 850 }
}

/// The code page a code set name stands for.
pub fn code_set(name: &str) -> Option<u16> {
    let name = name.split('@').next().unwrap_or("").trim();
    if name.is_empty() {
        return Some(process_cp());
    }
    let digits = ["IBM-", "IBM", "CP", "ibm-", "cp"].iter().find_map(|p| name.strip_prefix(p)).unwrap_or(name);
    let cp: u16 = digits.parse().ok()?;
    uls::codepage(cp).map(|_| cp)
}

fn object<'a>(u: u32) -> Option<&'a Uconv> {
    let o = u as *const Uconv;
    (!o.is_null() && unsafe { (*o).magic } == MAGIC).then(|| unsafe { &*o })
}

/// UniCreateUconvObject(code set name, &object).
#[unsafe(no_mangle)]
pub extern "C" fn UniCreateUconvObject(name: *const u16, out: *mut u32) -> i32 {
    if out.is_null() {
        return ULS_INVALID;
    }
    let name: String = if name.is_null() { String::new() } else { ascii(unsafe { from_ptr(name) }).unwrap_or_default() };
    let Some(cp) = code_set(&name) else { return ULS_INVALID };
    let o = Box::new(Uconv { magic: MAGIC, cp });
    unsafe { out.write_unaligned(Box::into_raw(o) as u32) };
    ULS_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn UniFreeUconvObject(u: u32) -> i32 {
    if object(u).is_none() {
        return ULS_BADOBJECT;
    }
    let mut o = unsafe { Box::from_raw(u as *mut Uconv) };
    o.magic = 0;
    ULS_SUCCESS
}

/// UniUconvToUcs(object, &in, &in bytes, &out, &out chars, &substitutions).
#[unsafe(no_mangle)]
pub extern "C" fn UniUconvToUcs(u: u32, inp: *mut *const u8, inb: *mut u32, out: *mut *mut u16, outc: *mut u32, subst: *mut u32) -> i32 {
    let Some(o) = object(u) else { return ULS_BADOBJECT };
    unsafe {
        let (mut i, mut n, mut d, mut m) = (*inp, *inb, *out, *outc);
        let mut rc = ULS_SUCCESS;
        while n > 0 {
            if m == 0 {
                rc = ULS_BUFFERFULL;
                break;
            }
            let c = uls::cp_to_ucs(o.cp, *i);
            if c == 0xfffd && !subst.is_null() {
                *subst += 1;
            }
            d.write(c);
            (i, n, d, m) = (i.add(1), n - 1, d.add(1), m - 1);
        }
        (*inp, *inb, *out, *outc) = (i, n, d, m);
        rc
    }
}

/// UniUconvFromUcs(object, &in, &in chars, &out, &out bytes, &substitutions).
#[unsafe(no_mangle)]
pub extern "C" fn UniUconvFromUcs(u: u32, inp: *mut *const u16, inc: *mut u32, out: *mut *mut u8, outb: *mut u32, subst: *mut u32) -> i32 {
    let Some(o) = object(u) else { return ULS_BADOBJECT };
    unsafe {
        let (mut i, mut n, mut d, mut m) = (*inp, *inc, *out, *outb);
        let mut rc = ULS_SUCCESS;
        while n > 0 {
            if m == 0 {
                rc = ULS_BUFFERFULL;
                break;
            }
            let b = uls::ucs_to_cp(o.cp, *i).unwrap_or_else(|| {
                if !subst.is_null() {
                    *subst += 1;
                }
                SUBCHAR
            });
            d.write(b);
            (i, n, d, m) = (i.add(1), n - 1, d.add(1), m - 1);
        }
        (*inp, *inc, *out, *outb) = (i, n, d, m);
        rc
    }
}

/// UniStrFromUcs(object, target, source, target bytes).
#[unsafe(no_mangle)]
pub extern "C" fn UniStrFromUcs(u: u32, target: *mut u8, source: *const u16, len: i32) -> i32 {
    let Some(o) = object(u) else { return ULS_BADOBJECT };
    if target.is_null() || source.is_null() || len < 1 {
        return ULS_INVALID;
    }
    let s = unsafe { from_ptr(source) };
    let n = s.len().min(len as usize - 1);
    for (k, &c) in s[..n].iter().enumerate() {
        unsafe { target.add(k).write(uls::ucs_to_cp(o.cp, c).unwrap_or(SUBCHAR)) };
    }
    unsafe { target.add(n).write(0) };
    if n < s.len() { ULS_BUFFERFULL } else { ULS_SUCCESS }
}

/// UniStrToUcs(object, target, source, target characters).
#[unsafe(no_mangle)]
pub extern "C" fn UniStrToUcs(u: u32, target: *mut u16, source: *const u8, len: i32) -> i32 {
    let Some(o) = object(u) else { return ULS_BADOBJECT };
    if target.is_null() || source.is_null() || len < 1 {
        return ULS_INVALID;
    }
    let mut k = 0usize;
    unsafe {
        while *source.add(k) != 0 {
            if k + 1 >= len as usize {
                target.add(k).write(0);
                return ULS_BUFFERFULL;
            }
            target.add(k).write(uls::cp_to_ucs(o.cp, *source.add(k)));
            k += 1;
        }
        target.add(k).write(0);
    }
    ULS_SUCCESS
}

/// UniMapCpToUcsCp(code page, buffer, characters): `IBM-nnn` (0: the
/// process code page).
#[unsafe(no_mangle)]
pub extern "C" fn UniMapCpToUcsCp(cp: u32, buf: *mut u16, n: u32) -> i32 {
    if buf.is_null() {
        return ULS_INVALID;
    }
    let cp = if cp == 0 { process_cp() as u32 } else { cp };
    let name = alloc::format!("IBM-{cp}");
    if name.len() + 1 > n as usize {
        return ULS_BUFFERFULL;
    }
    for (k, b) in name.bytes().enumerate() {
        unsafe { buf.add(k).write(b as u16) };
    }
    unsafe { buf.add(name.len()).write(0) };
    ULS_SUCCESS
}

/// uconv_attribute_t (uconv.h), the fields rs2 fills in.
#[repr(C)]
struct Attr {
    version: u32,
    mb_min_len: u8,
    mb_max_len: u8,
    usc_min_len: u8,
    usc_max_len: u8,
    esid: u16,
    options: u8,
    state: u8,
    endian: [u16; 2],
    displaymask: u32,
    converttype: u32,
    subchar_len: u16,
    subuni_len: u16,
    subchar: [u8; 16],
    subuni: [u16; 8],
}

/// UniQueryUconvObject(object, &attributes, size, first[256], other[256],
/// udcrange[32]): a single-byte code page.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryUconvObject(u: u32, attr: *mut u8, size: u32, first: *mut u8, other: *mut u8, udc: *mut u8) -> i32 {
    if object(u).is_none() {
        return ULS_BADOBJECT;
    }
    if !attr.is_null() {
        let mut a = Attr {
            version: 0,
            mb_min_len: 1,
            mb_max_len: 1,
            usc_min_len: 2,
            usc_max_len: 2,
            esid: 0x2100,
            options: 0,
            state: 0,
            endian: [0, 0],
            displaymask: 0xffff_ffff,
            converttype: 0,
            subchar_len: 1,
            subuni_len: 1,
            subchar: [0; 16],
            subuni: [0; 8],
        };
        a.subchar[0] = SUBCHAR;
        a.subuni[0] = 0xfffd;
        let n = (size as usize).min(core::mem::size_of::<Attr>());
        unsafe { core::ptr::copy_nonoverlapping(&a as *const Attr as *const u8, attr, n) };
    }
    unsafe {
        if !first.is_null() {
            core::ptr::write_bytes(first, 1, 256);
        }
        if !other.is_null() {
            core::ptr::write_bytes(other, 0, 256);
        }
        if !udc.is_null() {
            core::ptr::write_bytes(udc, 0, 32 * 4);
        }
    }
    ULS_SUCCESS
}

/// UniSetUconvObject(object, &attributes): accepted, nothing changes.
#[unsafe(no_mangle)]
pub extern "C" fn UniSetUconvObject(u: u32, _attr: *const u8) -> i32 {
    if object(u).is_none() { ULS_BADOBJECT } else { ULS_SUCCESS }
}

/// _UniMalloc(size): memory for the ULS DLLs (freed with _UniFree).
#[unsafe(no_mangle)]
pub extern "C" fn _UniMalloc(cb: u32) -> *mut u8 {
    let layout = core::alloc::Layout::from_size_align(cb as usize + 8, 8).unwrap();
    let p = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if p.is_null() {
        return p;
    }
    unsafe {
        (p as *mut usize).write(cb as usize);
        p.add(8)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _UniFree(p: *mut u8) -> i32 {
    if p.is_null() {
        return ULS_NOMEMORY;
    }
    unsafe {
        let base = p.sub(8);
        let cb = (base as *const usize).read();
        alloc::alloc::dealloc(base, core::alloc::Layout::from_size_align(cb + 8, 8).unwrap());
    }
    ULS_SUCCESS
}
