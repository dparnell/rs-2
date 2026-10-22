//! PMSHELL.EXE: the Workplace Shell's process, as IBM's PMSHELL is: PM
//! (WinInitialize, a message queue), PMWP.DLL's ShlStartWorkplace (the
//! classes, the first start's objects, the desktop, the Startup folder;
//! see userland/pmwp) and the message loop. `PROTSHELL=C:\OS2\PMSHELL.EXE`
//! in CONFIG.SYS makes it the desktop.
//!
//! Without PM (no graphics display) it runs the command prompt
//! (%OS2_SHELL%, else C:\OS2\CMD.EXE); with a Workplace Shell already
//! running it ends.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use os2::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fs: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut [u32; 8], h: u32, f: u32, l: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const [u32; 8]) -> u32;
    #[link_name = "PMWP.211"]
    fn ShlStartWorkplace(hab: u32, hmq: u32) -> u32;
}

fn cstr(p: *const u8) -> Vec<u8> {
    let mut v = Vec::new();
    if p.is_null() {
        return v;
    }
    let mut i = 0;
    while unsafe { *p.add(i) } != 0 {
        v.push(unsafe { *p.add(i) });
        i += 1;
    }
    v
}

/// Run %OS2_SHELL% (else C:\OS2\CMD.EXE) and wait for it.
fn text_shell() -> u32 {
    let mut p: *const u8 = core::ptr::null();
    let name = if unsafe { DosScanEnv(b"OS2_SHELL\0".as_ptr(), &mut p) } == 0 && !p.is_null() { cstr(p) } else { b"C:\\OS2\\CMD.EXE".to_vec() };
    let mut prog = name.clone();
    prog.push(0);
    let mut args = name;
    args.extend_from_slice(b"\0\0\0");
    let mut fail = [0u8; 64];
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, args.as_ptr(), core::ptr::null(), &mut res, prog.as_ptr()) };
    res.code_result
}

fn main(_s: &Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = if hab != 0 { unsafe { WinCreateMsgQueue(hab, 0) } } else { 0 };
    if hmq == 0 {
        println!("PMSHELL: Presentation Manager is not available; starting the command prompt");
        return text_shell();
    }
    if unsafe { ShlStartWorkplace(hab, hmq) } == 0 {
        println!("PMSHELL: the Workplace Shell is already running (or did not start)");
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return 1;
    }
    let mut q = [0u32; 8];
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
