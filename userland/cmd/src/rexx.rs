//! REXX procedures: a .CMD file whose first line starts with `/*` runs in
//! the REXX interpreter (REXX.DLL, when installed) instead of as a batch
//! file. Commands in the procedure come back to this shell through the
//! "CMD" subcommand environment.
//!
//! REXX changes environment variables (VALUE(..., 'OS2ENVIRONMENT'),
//! SETLOCAL) in the process's environment block, as OS/2's REXX does; the
//! shell keeps its variables in a list of its own, so it writes them to
//! the block before the procedure and around each command it runs for it,
//! and reads them back afterwards.

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

fn pib() -> *mut Pib {
    let mut tib: *mut Tib = null_mut();
    let mut pib: *mut Pib = null_mut();
    unsafe { DosGetInfoBlocks(&mut tib, &mut pib) };
    pib
}

/// Length of the variables part of an environment block (up to and with
/// its final zero).
fn block_len(e: *const u8) -> usize {
    let mut n = 0;
    unsafe {
        while !(*e.add(n) == 0 && (n == 0 || *e.add(n - 1) == 0)) {
            n += 1;
        }
    }
    n + 1
}

/// Read the shell's variables back from the process's environment block.
fn env_from_block(sh: &mut Shell) {
    let p = pib();
    if p.is_null() || unsafe { (*p).pchenv }.is_null() {
        return;
    }
    let e = unsafe { (*p).pchenv };
    let n = block_len(e);
    let b = unsafe { core::slice::from_raw_parts(e, n) };
    sh.env.clear();
    for v in b.split(|&c| c == 0).filter(|v| !v.is_empty()) {
        let s: String = v.iter().map(|&c| c as char).collect();
        if let Some((k, val)) = s.split_once('=') {
            sh.env.push((k.to_ascii_uppercase(), val.into()));
        }
    }
}

/// Write the shell's variables into the process's environment block (in
/// place when they fit, else in a new block the PIB points to); the
/// program name and arguments after the variables are kept.
fn env_to_block(sh: &Shell) {
    let p = pib();
    if p.is_null() || unsafe { (*p).pchenv }.is_null() {
        return;
    }
    let e = unsafe { (*p).pchenv } as *mut u8;
    let old = block_len(e);
    // the tail: program path, then the program name and arguments
    let mut tail = 0;
    let mut zeros = 0;
    while zeros < 4 && tail < 4096 {
        if unsafe { *e.add(old + tail) } == 0 {
            zeros += 1;
        }
        tail += 1;
    }
    let nb = sh.env_block();
    let (mut size, mut flags) = (0u32, 0u32);
    unsafe { DosQueryMem(e, &mut size, &mut flags) };
    let t: Vec<u8> = unsafe { core::slice::from_raw_parts(e.add(old), tail).to_vec() };
    if (nb.len() + tail) as u32 <= size {
        unsafe {
            core::ptr::copy_nonoverlapping(nb.as_ptr(), e, nb.len());
            core::ptr::copy_nonoverlapping(t.as_ptr(), e.add(nb.len()), t.len());
        }
    } else {
        let mut np: *mut u8 = null_mut();
        if unsafe { DosAllocMem(&mut np, (nb.len() + tail + 16384) as u32, PAG_COMMIT | PAG_READ | PAG_WRITE) } != 0 {
            return;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(nb.as_ptr(), np, nb.len());
            core::ptr::copy_nonoverlapping(t.as_ptr(), np.add(nb.len()), t.len());
            (*p).pchenv = np;
        }
    }
}

/// The shell running the current procedure (for the subcommand handler).
static mut SHELL: *mut Shell = null_mut();

/// Subcommand handler: run one command line in the shell; the return code
/// goes back to REXX as RC.
extern "C" fn subcom(cmd: *const RxString, flags: *mut u16, ret: *mut RxString) -> u32 {
    unsafe {
        let line: String = core::slice::from_raw_parts((*cmd).ptr, (*cmd).len as usize).iter().map(|&b| b as char).collect();
        let rc = match SHELL.as_mut() {
            Some(sh) => {
                env_from_block(sh);
                sh.run_line(&line);
                env_to_block(sh);
                sh.rc
            }
            None => 1,
        };
        // RXSUBCOM_ERROR when the command failed, as IBM's CMD reports it
        *flags = (rc != 0) as u16;
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
    env_to_block(sh);
    let r = unsafe { start(1, &mut arg, name.as_ptr(), null_mut(), b"CMD\0".as_ptr(), RXCOMMAND, null_mut(), &mut rc, &mut result) };
    unsafe { SHELL = prev };
    env_from_block(sh);
    if !result.ptr.is_null() {
        unsafe { DosFreeMem(result.ptr) };
    }
    // an error ends the procedure with its negative number (IBM's CMD:
    // 65501 for -35)
    sh.rc = if r < 0 { r as i16 as u16 as u32 } else { rc as u16 as u32 };
    true
}
