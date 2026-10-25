//! PMSPL.DLL: the spooler (docs/PRINTING.md). As IBM's PMSPL, the DLL is
//! the spooler: every process that prints or looks at queues uses it, and
//! PMSPOOL.EXE runs its spooler thread (SplQmInitialize).
//!
//! - `ini`: the configuration (queues, printers, ports, drivers, queue
//!   processors) in OS2SYS.INI's PM_SPOOLER* applications;
//! - `jobs`: jobs as spool and shadow files in `\SPOOL\<queue>`, the lock
//!   every process takes, and the spooler's wake-up semaphore;
//! - `api`: SplEnum*/SplQuery*/SplSet*/SplCreate*/SplDelete* and the
//!   hold, release, purge and control calls;
//! - `qm`: SplQmOpen and the other queue manager calls (spooling a job);
//! - `spooler`: the spooler thread, giving jobs to queue processors;
//! - `port`: PrtOpen/PrtWrite/PrtClose and the ports (FILE, LPT, COM,
//!   LPR, IPP).

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod api;
mod exports;
mod ini;
mod jobs;
mod port;
mod qm;
mod spooler;
mod util;

pub const PMERR_INVALID_PARM: u32 = 0x1303;
pub const PMERR_SPL_QUEUE_NOT_FOUND: u32 = 0x4024;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// WinSetErrorInfo, when the process uses PMWIN (PMSPL does not import
/// it: text-mode programs use the spooler too).
pub fn set_error(e: u32) {
    let mut hmod = 0;
    if unsafe { os2::DosQueryModuleHandle(b"PMWIN\0".as_ptr(), &mut hmod) } != 0 {
        return;
    }
    let mut f = 0usize;
    if unsafe { os2::DosQueryProcAddr(hmod, 977, core::ptr::null(), &mut f) } == 0 && f != 0 {
        let set: unsafe extern "C" fn(u32, u32) -> u32 = unsafe { core::mem::transmute(f) };
        unsafe { set(e, 0xfffe) };
    }
}

/// SplMessageBox(pszLogAddr, fErrInfo, fErrData, pszText, pszCaption,
/// idWindow, fStyle): the spooler's message box (WinMessageBox on the
/// desktop when PM is there). Returns the button pressed (MBID_*), or
/// MBID_ERROR.
#[unsafe(no_mangle)]
pub extern "C" fn SplMessageBox(_addr: *const u8, _info: u32, _data: u32, text: *const u8, caption: *const u8, id: u32, style: u32) -> u32 {
    let mut hmod = 0;
    let mut fail = [0u8; 16];
    if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 16, b"PMWIN\0".as_ptr(), &mut hmod) } != 0 {
        return 0xffff;
    }
    let mut f = 0usize;
    // WinMessageBox
    if unsafe { os2::DosQueryProcAddr(hmod, 789, core::ptr::null(), &mut f) } != 0 || f == 0 {
        return 0xffff;
    }
    let mb: unsafe extern "C" fn(u32, u32, *const u8, *const u8, u32, u32) -> u32 = unsafe { core::mem::transmute(f) };
    unsafe { mb(1, 0, text, caption, id, style) }
}
