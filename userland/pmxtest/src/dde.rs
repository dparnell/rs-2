//! DDE between two processes: `PMXTEST DDESERVER` is a server for the
//! application "PMXTEST", topic "System"; this side initiates the
//! conversation, requests an item, pokes one, executes a command and
//! terminates. Every DDEINIT and DDESTRUCT is giveable shared memory that
//! goes to the receiving process (one is even sent back).

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::*;

use crate::pm::*;
use crate::{check, pump_for};

const WM_DDE_INITIATE: u32 = 0xa0;
const WM_DDE_REQUEST: u32 = 0xa1;
const WM_DDE_ACK: u32 = 0xa2;
const WM_DDE_DATA: u32 = 0xa3;
const WM_DDE_POKE: u32 = 0xa6;
const WM_DDE_EXECUTE: u32 = 0xa7;
const WM_DDE_TERMINATE: u32 = 0xa8;
const WM_DDE_INITIATEACK: u32 = 0xa9;
const WM_QUIT: u32 = 0x002a;

const DDE_FACK: u16 = 0x0001;
const DDE_FRESPONSE: u16 = 0x0010;
const DDEFMT_TEXT: u16 = 1;

unsafe extern "C" {
    #[link_name = "PMWIN.718"]
    fn WinDdeInitiate(client: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32;
    #[link_name = "PMWIN.719"]
    fn WinDdePostMsg(to: u32, from: u32, wm: u32, dde: *mut u8, fl: u32) -> u32;
    #[link_name = "PMWIN.720"]
    fn WinDdeRespond(client: u32, server: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32;
}

fn cstr(p: *const u8) -> Vec<u8> {
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// A DDESTRUCT (cbData, fsStatus, usFormat, offszItemName, offabData)
/// with the item name and data after it, in giveable shared memory.
fn ddestruct(status: u16, item: &[u8], data: &[u8]) -> *mut u8 {
    let size = 12 + item.len() + 1 + data.len();
    let mut p: *mut u8 = null_mut();
    if unsafe { DosAllocSharedMem(&mut p, null(), size as u32, PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE) } != 0 {
        return null_mut();
    }
    unsafe {
        (p as *mut u32).write_unaligned(data.len() as u32);
        (p.add(4) as *mut u16).write_unaligned(status);
        (p.add(6) as *mut u16).write_unaligned(DDEFMT_TEXT);
        (p.add(8) as *mut u16).write_unaligned(12);
        (p.add(10) as *mut u16).write_unaligned((12 + item.len() + 1) as u16);
        core::ptr::copy_nonoverlapping(item.as_ptr(), p.add(12), item.len());
        *p.add(12 + item.len()) = 0;
        core::ptr::copy_nonoverlapping(data.as_ptr(), p.add(13 + item.len()), data.len());
    }
    p
}

/// A DDESTRUCT's status, item and data.
fn parts(p: *const u8) -> (u16, Vec<u8>, Vec<u8>) {
    unsafe {
        let cb = (p as *const u32).read_unaligned() as usize;
        let status = (p.add(4) as *const u16).read_unaligned();
        let oi = (p.add(8) as *const u16).read_unaligned() as usize;
        let od = (p.add(10) as *const u16).read_unaligned() as usize;
        (status, cstr(p.add(oi)), core::slice::from_raw_parts(p.add(od), cb).to_vec())
    }
}

/// What this side's client got: (message, from, status, item, data).
static mut GOT: Vec<(u32, u32, u16, Vec<u8>, Vec<u8>)> = Vec::new();

fn got() -> &'static mut Vec<(u32, u32, u16, Vec<u8>, Vec<u8>)> {
    unsafe { &mut *core::ptr::addr_of_mut!(GOT) }
}

/// The client's DDE messages (both sides use the same window class).
pub fn client_msg(h: u32, msg: u32, mp1: u32, mp2: u32) -> Option<u32> {
    if SERVER.load(core::sync::atomic::Ordering::Relaxed) {
        return server_msg(h, msg, mp1, mp2);
    }
    match msg {
        WM_DDE_INITIATEACK => {
            // DDEINIT: cb, pszAppName, pszTopic, offConvContext.
            let p = mp2 as *const u32;
            let (app, topic) = unsafe { (cstr(*p.add(1) as *const u8), cstr(*p.add(2) as *const u8)) };
            got().push((msg, mp1, 0, app, topic));
            unsafe { DosFreeMem(mp2 as *mut u8) };
            Some(1)
        }
        WM_DDE_DATA | WM_DDE_ACK => {
            let (st, item, data) = parts(mp2 as *const u8);
            got().push((msg, mp1, st, item, data));
            unsafe { DosFreeMem(mp2 as *mut u8) };
            Some(0)
        }
        WM_DDE_TERMINATE => {
            got().push((msg, mp1, 0, Vec::new(), Vec::new()));
            Some(0)
        }
        _ => None,
    }
}

static SERVER: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn server_msg(h: u32, msg: u32, mp1: u32, mp2: u32) -> Option<u32> {
    match msg {
        WM_DDE_INITIATE => {
            let p = mp2 as *const u32;
            let (app, topic) = unsafe { (cstr(*p.add(1) as *const u8), cstr(*p.add(2) as *const u8)) };
            unsafe { DosFreeMem(mp2 as *mut u8) };
            if app == b"PMXTEST" && topic == b"System" {
                unsafe { WinDdeRespond(mp1, h, b"PMXTEST\0".as_ptr(), b"System\0".as_ptr(), null()) };
            }
            Some(1)
        }
        WM_DDE_REQUEST => {
            let (_, item, _) = parts(mp2 as *const u8);
            unsafe { DosFreeMem(mp2 as *mut u8) };
            let data: &[u8] = if item == b"Greeting" { b"hello from the server" } else { b"" };
            let d = ddestruct(DDE_FRESPONSE, &item, data);
            unsafe { WinDdePostMsg(mp1, h, WM_DDE_DATA, d, 0) };
            Some(0)
        }
        WM_DDE_POKE | WM_DDE_EXECUTE => {
            // Acknowledged in the same block, sent back: the length of
            // what came in the application status byte.
            let (_, item, data) = parts(mp2 as *const u8);
            let ok = if msg == WM_DDE_EXECUTE { data.starts_with(b"[quit]") } else { item == b"Value" };
            unsafe { (mp2 as *mut u16).add(2).write_unaligned(if ok { DDE_FACK | (data.len() as u16) << 8 } else { 0 }) };
            unsafe { WinDdePostMsg(mp1, h, WM_DDE_ACK, mp2 as *mut u8, 0) };
            Some(0)
        }
        WM_DDE_TERMINATE => {
            unsafe {
                WinDdePostMsg(mp1, h, WM_DDE_TERMINATE, null_mut(), 0);
                WinPostMsg(h, WM_QUIT, 0, 0);
            }
            Some(0)
        }
        _ => None,
    }
}

/// `PMXTEST DDESERVER`: answers until the conversation ends (or 20 s).
pub fn server(hab: u32) -> u32 {
    SERVER.store(true, core::sync::atomic::Ordering::Relaxed);
    let mut m = QMsg::default();
    unsafe { WinStartTimer(hab, 0, 1, 20000) };
    while unsafe { WinGetMsg(hab, &mut m, 0, 0, 0) } != 0 {
        if m.msg == WM_TIMER && m.hwnd == 0 {
            return 1;
        }
        unsafe { WinDispatchMsg(hab, &m) };
    }
    0
}

/// Wait (dispatching) until the client got message `msg`.
fn wait_for(hab: u32, msg: u32) -> Option<(u32, u32, u16, Vec<u8>, Vec<u8>)> {
    for _ in 0..100 {
        if let Some(i) = got().iter().position(|g| g.0 == msg) {
            return Some(got().remove(i));
        }
        pump_for(hab, 30);
    }
    None
}

pub fn dde_checks(hab: u32, client: u32) {
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, EXEC_ASYNCRESULT, b"PMXTEST\0DDESERVER\0\0".as_ptr(), null(), &mut res, b"C:\\RS2\\PMXTEST.EXE\0".as_ptr()) };
    let child = res.code_terminate;
    check("DDE server process started", rc == 0);
    // Until the server's window is up.
    let mut ack = None;
    for _ in 0..30 {
        unsafe { WinDdeInitiate(client, b"PMXTEST\0".as_ptr(), b"System\0".as_ptr(), null()) };
        pump_for(hab, 50);
        if let Some(a) = got().iter().position(|g| g.0 == WM_DDE_INITIATEACK) {
            ack = Some(got().remove(a));
            break;
        }
    }
    let server = ack.as_ref().map_or(0, |a| a.1);
    check("WinDdeInitiate: another process's server answers (WinDdeRespond, its DDEINIT)", ack.as_ref().is_some_and(|a| a.3 == b"PMXTEST" && a.4 == b"System"));
    if server == 0 {
        return;
    }
    let req = ddestruct(0, b"Greeting", b"");
    unsafe { WinDdePostMsg(server, client, WM_DDE_REQUEST, req, 0) };
    let data = wait_for(hab, WM_DDE_DATA);
    check("WM_DDE_REQUEST answered by WM_DDE_DATA", data.is_some_and(|d| d.1 == server && d.2 == DDE_FRESPONSE && d.3 == b"Greeting" && d.4 == b"hello from the server"));
    let poke = ddestruct(0, b"Value", b"12345");
    unsafe { WinDdePostMsg(server, client, WM_DDE_POKE, poke, 0) };
    let a = wait_for(hab, WM_DDE_ACK);
    check("WM_DDE_POKE acknowledged in its own DDESTRUCT", a.is_some_and(|a| a.2 == DDE_FACK | 5 << 8 && a.3 == b"Value"));
    // DDEPM_NOFREE: this process keeps its access to the block.
    let exec = ddestruct(0, b"", b"[quit]");
    unsafe { WinDdePostMsg(server, client, WM_DDE_EXECUTE, exec, 2) };
    let still = unsafe { *exec.add(12) } == 0;
    unsafe { DosFreeMem(exec) };
    let a = wait_for(hab, WM_DDE_ACK);
    check("WM_DDE_EXECUTE acknowledged (DDEPM_NOFREE keeps the block)", still && a.is_some_and(|a| a.2 == DDE_FACK | 6 << 8));
    unsafe { WinDdePostMsg(server, client, WM_DDE_TERMINATE, null_mut(), 0) };
    let t = wait_for(hab, WM_DDE_TERMINATE);
    check("WM_DDE_TERMINATE answered", t.is_some_and(|t| t.1 == server));
    let mut pid = 0;
    let rc = unsafe { DosWaitChild(0, 0, &mut res, &mut pid, child) };
    check("the DDE server ends", rc == 0 && res.code_result == 0);
}
