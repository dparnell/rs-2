//! REXX procedures: a .CMD file whose first line starts with `/*` runs in
//! the REXX interpreter (REXX.DLL, when installed) instead of as a batch
//! file. Commands in the procedure come back to this shell through the
//! "CMD" subcommand environment.

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::null_mut;

use os2::path::cstr;
use os2::*;

use crate::shell::Shell;

#[repr(C)]
struct RxString {
    len: u32,
    ptr: *mut u8,
}

type RexxStart = unsafe extern "C" fn(
    argc: i32,
    argv: *mut RxString,
    name: *const u8,
    instore: *mut RxString,
    env: *const u8,
    calltype: i32,
    exits: *mut u8,
    rc: *mut i16,
    result: *mut RxString,
) -> i32;
type RegisterSubcomExe = unsafe extern "C" fn(name: *const u8, handler: usize, user: *mut u8) -> u32;

const RXCOMMAND: i32 = 0;

/// The shell running the current procedure (for the subcommand handler).
static mut SHELL: *mut Shell = null_mut();

/// Subcommand handler: run one command line in the shell; the return code
/// goes back to REXX as RC.
extern "C" fn subcom(cmd: *const RxString, flags: *mut u16, ret: *mut RxString) -> u32 {
    unsafe {
        let line: String = core::slice::from_raw_parts((*cmd).ptr, (*cmd).len as usize).iter().map(|&b| b as char).collect();
        let rc = match SHELL.as_mut() {
            Some(sh) => {
                sh.run_line(&line);
                sh.rc
            }
            None => 1,
        };
        *flags = 0; // RXSUBCOM_OK
        let text = alloc::format!("{rc}");
        // REXX supplies a 256-byte buffer.
        if !(*ret).ptr.is_null() {
            core::ptr::copy_nonoverlapping(text.as_ptr(), (*ret).ptr, text.len());
            (*ret).len = text.len() as u32;
        }
    }
    0
}

fn is_rexx(path: &str) -> bool {
    let c = cstr(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, null_mut())
    };
    if rc != 0 {
        return false;
    }
    let mut buf = [0u8; 2];
    let mut n = 0;
    unsafe {
        DosRead(hf, buf.as_mut_ptr(), 2, &mut n);
        DosClose(hf);
    }
    n == 2 && &buf == b"/*"
}

fn proc_addr(module: &[u8], name: &[u8]) -> Option<usize> {
    let mut fail = [0u8; 32];
    let mut h = 0u32;
    if unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, module.as_ptr(), &mut h) } != 0 {
        return None;
    }
    let mut pfn = 0usize;
    (unsafe { DosQueryProcAddr(h, 0, name.as_ptr(), &mut pfn) } == 0).then_some(pfn)
}

/// Run `path` as a REXX procedure if it is one and REXX is installed.
/// Returns false to let the caller treat it as a batch file.
pub fn run(sh: &mut Shell, path: &str, args: &str) -> bool {
    if !is_rexx(path) {
        return false;
    }
    let (Some(start), Some(reg)) = (proc_addr(b"REXX\0", b"RexxStart\0"), proc_addr(b"REXXAPI\0", b"RexxRegisterSubcomExe\0"))
    else {
        return false;
    };
    let start: RexxStart = unsafe { core::mem::transmute(start) };
    let reg: RegisterSubcomExe = unsafe { core::mem::transmute(reg) };
    unsafe { reg(b"CMD\0".as_ptr(), subcom as usize, null_mut()) };
    let mut a: Vec<u8> = args.trim().bytes().collect();
    let mut arg = RxString { len: a.len() as u32, ptr: a.as_mut_ptr() };
    let name = cstr(path);
    let mut rc: i16 = 0;
    let mut result = RxString { len: 0, ptr: null_mut() };
    let prev = unsafe { SHELL };
    unsafe { SHELL = sh as *mut Shell };
    let r = unsafe { start(1, &mut arg, name.as_ptr(), null_mut(), b"CMD\0".as_ptr(), RXCOMMAND, null_mut(), &mut rc, &mut result) };
    unsafe { SHELL = prev };
    if !result.ptr.is_null() {
        unsafe { DosFreeMem(result.ptr) };
    }
    sh.rc = if r < 0 { (-r) as u32 } else { rc as u16 as u32 };
    true
}
