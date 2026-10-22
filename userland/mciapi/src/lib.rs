//! MCIAPI.DLL: MMPM/2's REXX interface to the media control interface,
//! and the high-level calls built on it.
//!
//! REXX procedures (IBM's PLAY.CMD and RECORD.CMD among them) load it with
//! `RXFUNCADD('mciRxInit', 'MCIAPI', 'mciRxInit')`; `mciRxInit()` then
//! registers the other functions:
//!
//! - `mciRxSendString(command, 'retvar', hwnd, userparm)`: the command's
//!   result goes into the variable `retvar`, the return code (0 or an
//!   MCIERR number) is the function's value;
//! - `mciRxGetErrorString(rc, 'var')`, `mciRxGetDeviceID(name)`,
//!   `mciRxExit()`.
//!
//! The REXX API (REXX.DLL's RexxVariablePool, REXXAPI.DLL's
//! RexxRegisterFunctionDll) is found when first needed, so that programs
//! using only mciPlayFile load MCIAPI without REXX installed.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;

use alloc::string::String;
use alloc::vec::Vec;

use os2::*;

unsafe extern "C" {
    #[link_name = "MDM.2"]
    fn mciSendString(cmd: *const u8, ret: *mut u8, cb: u32, hwnd: u32, user: u32) -> u32;
    #[link_name = "MDM.3"]
    fn mciGetErrorString(err: u32, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "MDM.16"]
    fn mciGetDeviceID(name: *const u8) -> u32;
    #[link_name = "DOSCALLS.318"]
    fn DosLoadModule(fail: *mut u8, cb: u32, name: *const u8, h: *mut u32) -> u32;
    #[link_name = "DOSCALLS.321"]
    fn DosQueryProcAddr(h: u32, ord: u32, name: *const u8, pfn: *mut u32) -> u32;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// RXSTRING.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RxString {
    len: u32,
    ptr: *mut u8,
}

/// SHVBLOCK (RexxVariablePool's request).
#[repr(C)]
struct ShvBlock {
    next: *mut ShvBlock,
    name: RxString,
    value: RxString,
    namelen: u32,
    valuelen: u32,
    code: u8,
    ret: u8,
}

const RXSHV_SYSET: u8 = 0x03;
/// A REXX function's "incorrect call" (REXX error 40).
const INVALID_CALL: u32 = 40;

type VarPool = extern "C" fn(*mut ShvBlock) -> u32;
type RegisterDll = extern "C" fn(*const u8, *const u8, *const u8) -> u32;
type Deregister = extern "C" fn(*const u8) -> u32;

fn proc_addr(module: &[u8], ordinal: u32) -> u32 {
    let mut h = 0;
    let mut fail = [0u8; 16];
    if unsafe { DosLoadModule(fail.as_mut_ptr(), 16, module.as_ptr(), &mut h) } != 0 {
        return 0;
    }
    let mut p = 0;
    if unsafe { DosQueryProcAddr(h, ordinal, core::ptr::null(), &mut p) } != 0 {
        return 0;
    }
    p
}

fn arg(argv: *const RxString, argc: u32, i: u32) -> Option<String> {
    if i >= argc || argv.is_null() {
        return None;
    }
    let a = unsafe { *argv.add(i as usize) };
    if a.ptr.is_null() {
        return None;
    }
    let b = unsafe { core::slice::from_raw_parts(a.ptr, a.len as usize) };
    Some(b.iter().map(|&c| c as char).collect())
}

fn z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
    v.push(0);
    v
}

/// Set a REXX variable of the calling procedure.
fn set_var(name: &str, value: &str) -> bool {
    let p = proc_addr(b"REXX\0", 2);
    if p == 0 {
        return false;
    }
    let pool: VarPool = unsafe { core::mem::transmute(p as usize) };
    let mut n: Vec<u8> = name.chars().map(|c| c as u32 as u8).collect();
    let mut v: Vec<u8> = value.chars().map(|c| c as u32 as u8).collect();
    let mut b = ShvBlock {
        next: core::ptr::null_mut(),
        name: RxString { len: n.len() as u32, ptr: n.as_mut_ptr() },
        value: RxString { len: v.len() as u32, ptr: v.as_mut_ptr() },
        namelen: n.len() as u32,
        valuelen: v.len() as u32,
        code: RXSHV_SYSET,
        ret: 0,
    };
    pool(&mut b) < 2
}

/// The function's value: REXX's default return buffer is 256 bytes.
fn ret(r: *mut RxString, s: &str) -> u32 {
    if r.is_null() {
        return 0;
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
    unsafe {
        let rs = &mut *r;
        let n = bytes.len().min(if rs.ptr.is_null() { 0 } else { 255 });
        if n > 0 {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), rs.ptr, n);
        }
        rs.len = n as u32;
    }
    0
}

const FUNCS: [&[u8]; 4] = [b"mciRxSendString\0", b"mciRxGetErrorString\0", b"mciRxGetDeviceID\0", b"mciRxExit\0"];

/// mciRxInit(): register the other REXX functions.
#[unsafe(no_mangle)]
pub extern "C" fn mciRxInit(_name: *const u8, _argc: u32, _argv: *const RxString, _q: *const u8, r: *mut RxString) -> u32 {
    let p = proc_addr(b"REXXAPI\0", 17);
    if p == 0 {
        return ret(r, "1");
    }
    let reg: RegisterDll = unsafe { core::mem::transmute(p as usize) };
    for f in FUNCS {
        reg(f.as_ptr(), b"MCIAPI\0".as_ptr(), f.as_ptr());
    }
    ret(r, "0")
}

/// mciRxExit(): deregister the functions.
#[unsafe(no_mangle)]
pub extern "C" fn mciRxExit(_name: *const u8, _argc: u32, _argv: *const RxString, _q: *const u8, r: *mut RxString) -> u32 {
    let p = proc_addr(b"REXXAPI\0", 19);
    if p != 0 {
        let dereg: Deregister = unsafe { core::mem::transmute(p as usize) };
        for f in FUNCS {
            dereg(f.as_ptr());
        }
        dereg(b"mciRxInit\0".as_ptr());
    }
    ret(r, "0")
}

/// mciRxSendString(command, retvar, hwndCallback, usUserParm).
#[unsafe(no_mangle)]
pub extern "C" fn mciRxSendString(_name: *const u8, argc: u32, argv: *const RxString, _q: *const u8, r: *mut RxString) -> u32 {
    let (Some(cmd), Some(var)) = (arg(argv, argc, 0), arg(argv, argc, 1)) else { return INVALID_CALL };
    let hwnd = arg(argv, argc, 2).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    let user = arg(argv, argc, 3).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    let c = z(&cmd);
    let mut buf = [0u8; 512];
    let rc = unsafe { mciSendString(c.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, hwnd, user) };
    let n = buf.iter().position(|&b| b == 0).unwrap_or(0);
    let out: String = buf[..n].iter().map(|&b| b as char).collect();
    set_var(&var, &out);
    ret(r, &alloc::format!("{}", rc & 0xffff))
}

/// mciRxGetErrorString(rc, var).
#[unsafe(no_mangle)]
pub extern "C" fn mciRxGetErrorString(_name: *const u8, argc: u32, argv: *const RxString, _q: *const u8, r: *mut RxString) -> u32 {
    let (Some(code), Some(var)) = (arg(argv, argc, 0), arg(argv, argc, 1)) else { return INVALID_CALL };
    let Ok(code) = code.trim().parse::<u32>() else { return INVALID_CALL };
    let mut buf = [0u8; 256];
    let rc = unsafe { mciGetErrorString(code, buf.as_mut_ptr(), buf.len() as u32) };
    let n = buf.iter().position(|&b| b == 0).unwrap_or(0);
    let text: String = buf[..n].iter().map(|&b| b as char).collect();
    set_var(&var, &text);
    ret(r, &alloc::format!("{}", rc & 0xffff))
}

/// mciRxGetDeviceID(name): the device ID, 0 if none.
#[unsafe(no_mangle)]
pub extern "C" fn mciRxGetDeviceID(_name: *const u8, argc: u32, argv: *const RxString, _q: *const u8, r: *mut RxString) -> u32 {
    let Some(name) = arg(argv, argc, 0) else { return INVALID_CALL };
    let n = z(&name);
    let id = unsafe { mciGetDeviceID(n.as_ptr()) };
    ret(r, &alloc::format!("{id}"))
}

fn cstr(p: *const u8) -> String {
    let mut s = String::new();
    if p.is_null() {
        return s;
    }
    let mut i = 0;
    loop {
        let b = unsafe { *p.add(i) };
        if b == 0 {
            return s;
        }
        s.push(b as char);
        i += 1;
    }
}

/// mciPlayFile(hwndOwner, pszFile, ulFlags, pszTitle, hwndViewport): play
/// a media file to the end on its device (by extension). MCI_ASYNC (1)
/// returns at once.
#[unsafe(no_mangle)]
pub extern "C" fn mciPlayFile(owner: u32, file: *const u8, flags: u32, _title: *const u8, _viewport: u32) -> u32 {
    let f = cstr(file);
    if f.is_empty() {
        return 5017; // MCIERR_MISSING_PARAMETER
    }
    let cmd = z(&alloc::format!("play \"{f}\" {}", if flags & 1 != 0 { "" } else { "wait" }));
    let mut buf = [0u8; 16];
    unsafe { mciSendString(cmd.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, owner, 0) }
}

/// mciPlayResource: sounds and movies stored as resources (not supported).
#[unsafe(no_mangle)]
pub extern "C" fn mciPlayResource(_owner: u32, _hmod: u32, _ty: u32, _id: u32, _flags: u32, _title: *const u8, _viewport: u32) -> u32 {
    5018 // MCIERR_UNSUPPORTED_FUNCTION
}

/// mciRecordAudioFile: recording (not supported yet).
#[unsafe(no_mangle)]
pub extern "C" fn mciRecordAudioFile(_owner: u32, _file: *const u8, _title: *const u8, _flags: u32) -> u32 {
    5018
}
