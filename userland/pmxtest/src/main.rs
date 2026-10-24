//! Self-checks of the Presentation Manager features beyond the basics:
//! hooks (hooks.rs), DDE with another process (dde.rs). Each check
//! prints PASS or FAIL; the last line is "pmxtest done: N failure(s)"
//! and the exit code N. `PMXTEST CHILD` is the second process the system
//! hooks are checked in, `PMXTEST DDESERVER` the DDE server; `PMXTEST
//! RECORD` waits for three keys or clicks (typed by `xtask run --type`)
//! and reports what the journal record hook saw.

#![no_std]
#![no_main]

extern crate alloc;

use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

mod dde;
mod hooks;
mod pm;

use pm::*;

os2::entry!(main);

static FAILS: AtomicU32 = AtomicU32::new(0);

pub fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

/// Dispatch what is waiting.
pub fn pump(hab: u32) {
    let mut m = QMsg::default();
    while unsafe { WinPeekMsg(hab, &mut m, 0, 0, 0, 1) } != 0 {
        unsafe { WinDispatchMsg(hab, &m) };
    }
}

/// Dispatch messages for about `ms` milliseconds.
pub fn pump_for(hab: u32, ms: u32) {
    for _ in 0..ms / 10 {
        pump(hab);
        unsafe { DosSleep(10) };
    }
    pump(hab);
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if let Some(r) = hooks::client_msg(h, msg, mp1, mp2) {
        return r;
    }
    if let Some(r) = dde::client_msg(h, msg, mp1, mp2) {
        return r;
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

fn main(s: &Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let args: alloc::vec::Vec<u8> = s.args().iter().copied().skip_while(|&c| c == b' ').collect();
    unsafe { WinRegisterClass(hab, b"PmxTest\0".as_ptr(), client_proc, 0, 0) };
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER, visible.
    let mut fcf = 1u32 | 2 | 8;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0x8000_0000, &mut fcf, b"PmxTest\0".as_ptr(), b"PM extras test\0".as_ptr(), 0, 0, 1, &mut client) };
    unsafe {
        WinSetWindowPos(frame, 3, 40, 40, 400, 300, 0x0001 | 0x0002 | 0x0008 | 0x0080);
        WinSetFocus(HWND_DESKTOP, client);
    }
    pump(hab);
    if args.starts_with(b"CHILD") {
        let r = hooks::child(hab, client);
        unsafe {
            WinDestroyWindow(frame);
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return r;
    }
    if args.starts_with(b"DDESERVER") {
        let r = dde::server(hab);
        unsafe {
            WinDestroyWindow(frame);
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return r;
    }
    if args.starts_with(b"RECORD") {
        hooks::record(hab, client);
    } else {
        check("WinInitialize/WinCreateMsgQueue and a window", hab != 0 && hmq != 0 && client != 0);
        hooks::hook_checks(hab, hmq, client);
        dde::dde_checks(hab, client);
    }
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    let f = FAILS.load(Ordering::Relaxed);
    println!("pmxtest done: {f} failure(s)");
    f
}
