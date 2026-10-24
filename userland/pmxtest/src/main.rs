//! Self-checks of the Presentation Manager features beyond the basics:
//! hooks (hooks.rs), DDE with another process (dde.rs), the registry
//! through Open32's calls (reg.rs), owner drawing (owner.rs),
//! presentation parameters (pres.rs). Each check
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
mod owner;
mod pm;
mod pres;
mod reg;

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
    if let Some(r) = owner::client_msg(h, msg, mp1, mp2) {
        return r;
    }
    if let Some(r) = pres::client_msg(h, msg, mp1, mp2) {
        return r;
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

/// `PMXTEST WINS`: the window tree (class, text, position, style), for
/// looking at what another program made.
fn list_windows(parent: u32, depth: usize) {
    let e = unsafe { WinBeginEnumWindows(parent) };
    loop {
        let h = unsafe { WinGetNextWindow(e) };
        if h == 0 {
            break;
        }
        let mut class = [0u8; 32];
        let mut text = [0u8; 48];
        let mut swp = [0i32; 9];
        let n = unsafe { WinQueryClassName(h, 32, class.as_mut_ptr()) } as usize;
        let t = unsafe { WinQueryWindowText(h, 48, text.as_mut_ptr()) } as usize;
        unsafe { WinQueryWindowPos(h, swp.as_mut_ptr()) };
        let style = unsafe { WinQueryWindowULong(h, -2) };
        let id = unsafe { WinQueryWindowUShort(h, -1) };
        let line = alloc::format!(
            "{:w$}{h:#x} {} id {id:#x} '{}' ({}, {}) {}x{} style {style:#x}\0",
            "",
            core::str::from_utf8(&class[..n]).unwrap_or("?"),
            core::str::from_utf8(&text[..t]).unwrap_or("?"),
            swp[4],
            swp[3],
            swp[2],
            swp[1],
            w = depth * 2
        );
        // On the serial port too (PMXTEST may run from a command prompt).
        unsafe extern "C" {
            #[link_name = "RS2API.4"]
            fn Rs2DebugTrace(psz: *const u8) -> u32;
        }
        unsafe { Rs2DebugTrace(line.as_ptr()) };
        if depth < 3 {
            list_windows(h, depth + 1);
        }
    }
    unsafe { WinEndEnumWindows(e) };
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
    if args.starts_with(b"WINS") {
        // `PMXTEST WINS PROGRAM`: start it first.
        let prog: alloc::vec::Vec<u8> = args[4..].iter().copied().skip_while(|&c| c == b' ').collect();
        if !prog.is_empty() {
            let mut name = prog.clone();
            name.push(0);
            let mut a = prog.clone();
            a.extend_from_slice(b"\0\0");
            let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
            let rc = unsafe { DosExecPgm(core::ptr::null_mut(), 0, EXEC_ASYNC, a.as_ptr(), core::ptr::null(), &mut res, name.as_ptr()) };
            println!("started {}: {rc}", core::str::from_utf8(&prog).unwrap_or("?"));
        }
        unsafe { DosSleep(15000) };
        list_windows(HWND_DESKTOP, 0);
        return 0;
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
        reg::reg_checks();
        owner::owner_checks(hab, client);
        pres::pres_checks(client);
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
