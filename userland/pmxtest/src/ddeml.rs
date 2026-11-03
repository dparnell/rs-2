//! PMDDEML: string and data handles, then conversations with another
//! process: `PMXTEST DDEMLSERVER` registers the service "PMXDDEML"
//! (topic "Test"; items "Greeting", "Value", "Counter"; the command
//! "[advise]" updates "Counter"). This side connects, requests, pokes,
//! executes, runs an advise loop and an asynchronous request; a plain
//! PM DDE client (WinDdeInitiate) talks to the same server, and a DDEML
//! client to PMXTEST's plain DDE server.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::*;

use crate::pm::*;
use crate::{check, pump_for};

type Callback = extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMDDEML.116"]
    fn WinDdeInitialize(pid: *mut u32, pfn: Callback, af: u32, res: u32) -> u32;
    #[link_name = "PMDDEML.126"]
    fn WinDdeUninitialize(inst: u32) -> u32;
    #[link_name = "PMDDEML.105"]
    fn WinDdeConnect(inst: u32, svc: u32, topic: u32, cc: *const u32) -> u32;
    #[link_name = "PMDDEML.109"]
    fn WinDdeDisconnect(h: u32) -> u32;
    #[link_name = "PMDDEML.103"]
    fn WinDdeSubmitTransaction(p: *const u8, cb: u32, h: u32, item: u32, fmt: u32, typ: u32, timeout: u32, pres: *mut u32) -> u32;
    #[link_name = "PMDDEML.108"]
    fn WinDdeCreateStringHandle(psz: *const u8, cp: u32) -> u32;
    #[link_name = "PMDDEML.113"]
    fn WinDdeFreeStringHandle(h: u32) -> u32;
    #[link_name = "PMDDEML.117"]
    fn WinDdeKeepStringHandle(h: u32) -> u32;
    #[link_name = "PMDDEML.122"]
    fn WinDdeQueryString(h: u32, buf: *mut u8, cch: u32, cp: u32) -> u32;
    #[link_name = "PMDDEML.104"]
    fn WinDdeCompareStringHandles(a: u32, b: u32, case: u32) -> u32;
    #[link_name = "PMDDEML.107"]
    fn WinDdeCreateDataHandle(p: *const u8, cb: u32, off: u32, item: u32, fmt: u32, af: u32) -> u32;
    #[link_name = "PMDDEML.102"]
    fn WinDdeAddData(h: u32, p: *const u8, cb: u32, off: u32) -> u32;
    #[link_name = "PMDDEML.114"]
    fn WinDdeGetData(h: u32, p: *mut u8, cb: u32, off: u32) -> u32;
    #[link_name = "PMDDEML.101"]
    fn WinDdeAccessData(h: u32, pcb: *mut u32) -> *const u8;
    #[link_name = "PMDDEML.112"]
    fn WinDdeFreeDataHandle(h: u32) -> u32;
    #[link_name = "PMDDEML.118"]
    fn WinDdeNameService(inst: u32, svc: u32, res: u32, af: u32) -> u32;
    #[link_name = "PMDDEML.119"]
    fn WinDdePostAdvise(inst: u32, topic: u32, item: u32) -> u32;
    #[link_name = "PMDDEML.120"]
    fn WinDdeQueryConvInfo(h: u32, id: u32, ci: *mut u32) -> u32;
    #[link_name = "PMDDEML.106"]
    fn WinDdeConnectList(inst: u32, svc: u32, topic: u32, list: u32, cc: *const u32) -> u32;
    #[link_name = "PMDDEML.121"]
    fn WinDdeQueryNextServer(list: u32, prev: u32) -> u32;
    #[link_name = "PMDDEML.110"]
    fn WinDdeDisconnectList(list: u32) -> u32;
    #[link_name = "PMDDEML.123"]
    fn WinDdeReconnect(h: u32) -> u32;
    #[link_name = "PMDDEML.127"]
    fn WinDdeQueryModuleVersion(m: u32) -> u32;
    #[link_name = "PMWIN.753"]
    fn WinGetLastError(hab: u32) -> u32;
    #[link_name = "PMWIN.718"]
    fn WinDdeInitiate(client: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32;
    #[link_name = "PMWIN.719"]
    fn WinDdePostMsg(to: u32, from: u32, wm: u32, dde: *mut u8, fl: u32) -> u32;
}

const DDEFMT_TEXT: u32 = 1;
const XTYP_ADVDATA: u32 = 0x4010;
const XTYP_ADVREQ: u32 = 0x2022;
const XTYP_ADVSTART: u32 = 0x1030;
const XTYP_ADVSTOP: u32 = 0x8040;
const XTYP_EXECUTE: u32 = 0x4050;
const XTYP_CONNECT: u32 = 0x1062;
const XTYP_CONNECT_CONFIRM: u32 = 0x8072;
const XTYP_XACT_COMPLETE: u32 = 0x8080;
const XTYP_POKE: u32 = 0x4090;
const XTYP_REGISTER: u32 = 0x80a2;
const XTYP_REQUEST: u32 = 0x20b0;
const XTYP_DISCONNECT: u32 = 0x80c2;
const XTYP_WILDCONNECT: u32 = 0x20e2;
const DNS_REGISTER: u32 = 1;
const TIMEOUT_ASYNC: u32 = 0xffff_ffff;
const QID_SYNC: u32 = 0xffff_ffff;
const DDE_FACK: u32 = 1;
const ST_CONNECTED: u32 = 1;
const ST_CLIENT: u32 = 0x10;
const ST_ISLOCAL: u32 = 4;

fn hsz(s: &str) -> u32 {
    let mut v: Vec<u8> = s.as_bytes().to_vec();
    v.push(0);
    unsafe { WinDdeCreateStringHandle(v.as_ptr(), 0) }
}

fn query(h: u32) -> Vec<u8> {
    let mut b = [0u8; 64];
    let n = unsafe { WinDdeQueryString(h, b.as_mut_ptr(), 64, 0) } as usize;
    b[..n].to_vec()
}

fn data_of(h: u32) -> Vec<u8> {
    let n = unsafe { WinDdeGetData(h, null_mut(), 0, 0) };
    let mut v = alloc::vec![0u8; n as usize];
    unsafe { WinDdeGetData(h, v.as_mut_ptr(), n, 0) };
    v
}

fn is(h: u32, s: &str) -> bool {
    h != 0 && query(h) == s.as_bytes()
}

// ---- A monitor (APPCLASS_MONITOR) ----

const XTYP_MONITOR: u32 = 0x80f2;
const APPCLASS_MONITOR: u32 = 1;
const MF_HSZ_INFO: u32 = 0x0100_0000;
const MF_SENDMSGS: u32 = 0x0200_0000;
const MF_POSTMSGS: u32 = 0x0400_0000;
const MF_CALLBACKS: u32 = 0x0800_0000;
const MF_ERRORS: u32 = 0x1000_0000;
const MF_LINKS: u32 = 0x2000_0000;
const MF_CONV: u32 = 0x4000_0000;

/// What the monitor saw: (MF_* kind, the MON*STRUCT).
static mut MON: Vec<(u32, Vec<u8>)> = Vec::new();

fn mon() -> &'static mut Vec<(u32, Vec<u8>)> {
    unsafe { &mut *core::ptr::addr_of_mut!(MON) }
}

extern "C" fn monitor_cb(_inst: u32, typ: u32, _fmt: u32, _hconv: u32, _hsz1: u32, _hsz2: u32, hdata: u32, _d1: u32, kind: u32) -> u32 {
    if typ == XTYP_MONITOR {
        let mut cb = 0u32;
        let p = unsafe { WinDdeAccessData(hdata, &mut cb) };
        if !p.is_null() {
            mon().push((kind, unsafe { core::slice::from_raw_parts(p, cb as usize) }.to_vec()));
        }
    }
    0
}

fn u16_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]) as u32)
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn str_at(b: &[u8], o: usize) -> &[u8] {
    let s = b.get(o..).unwrap_or(&[]);
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

/// The monitor's records of a kind that satisfy `f`.
fn mon_saw(kind: u32, f: impl Fn(&[u8]) -> bool) -> bool {
    mon().iter().any(|(k, d)| *k == kind && f(d))
}

fn monitor_checks() {
    // MONHSZSTRUCT: the string after cb, fsAction, dwTime, hsz, hTask.
    check("monitor MF_HSZ_INFO: a string handle made (MH_CREATE)", mon_saw(MF_HSZ_INFO, |d| u16_at(d, 4) == 1 && str_at(d, 22) == b"Greeting"));
    // MONMSGSTRUCT: wMsg at +16; the DDEINIT's application and topic.
    check("monitor MF_SENDMSGS: WM_DDE_INITIATE with its service and topic", mon_saw(MF_SENDMSGS, |d| u32_at(d, 16) == 0xa0 && str_at(d, u16_at(d, 32) as usize) == b"PMXDDEML" && str_at(d, u16_at(d, 34) as usize) == b"Test"));
    check("monitor MF_POSTMSGS: WM_DDE_REQUEST of Greeting", mon_saw(MF_POSTMSGS, |d| u32_at(d, 16) == 0xa1 && str_at(d, u16_at(d, 32) as usize) == b"Greeting"));
    check("monitor MF_POSTMSGS: WM_DDE_DATA with the server's data", mon_saw(MF_POSTMSGS, |d| u32_at(d, 16) == 0xa3 && d.get(40..56) == Some(&b"hello from ddeml"[..])));
    // MONCBSTRUCT: wType at +16 (the server's callbacks, another process).
    check("monitor MF_CALLBACKS: the server's XTYP_REQUEST and its data", mon_saw(MF_CALLBACKS, |d| u16_at(d, 16) == XTYP_REQUEST && u32_at(d, 68) == 16 && d.get(72..88) == Some(&b"hello from ddeml"[..])));
    check("monitor MF_CONV: the conversation made", mon_saw(MF_CONV, |d| u32_at(d, 4) == 1 && u32_at(d, 24) != 0 && u32_at(d, 28) != 0));
    check("monitor MF_LINKS: the advise link started and stopped", mon_saw(MF_LINKS, |d| u32_at(d, 32) == 1) && mon_saw(MF_LINKS, |d| u32_at(d, 32) == 0));
    check("monitor MF_ERRORS: DMLERR_NOTPROCESSED", mon_saw(MF_ERRORS, |d| u32_at(d, 4) == 0x7009));
}

// ---- The server (another process) ----

static mut SERVER_INST: u32 = 0;
static mut COUNTER: u32 = 0;
static mut VALUE: Vec<u8> = Vec::new();
static mut BYE: bool = false;

extern "C" fn server_cb(inst: u32, typ: u32, fmt: u32, _hconv: u32, hsz1: u32, hsz2: u32, hdata: u32, _d1: u32, _d2: u32) -> u32 {
    match typ {
        XTYP_CONNECT => (is(hsz1, "Test") && is(hsz2, "PMXDDEML")) as u32,
        XTYP_REQUEST | XTYP_ADVREQ if fmt == DDEFMT_TEXT => {
            let text = if is(hsz2, "Greeting") {
                b"hello from ddeml".to_vec()
            } else if is(hsz2, "Value") {
                unsafe { (*core::ptr::addr_of!(VALUE)).clone() }
            } else if is(hsz2, "Counter") {
                alloc::format!("{}", unsafe { COUNTER }).into_bytes()
            } else {
                return 0;
            };
            unsafe { WinDdeCreateDataHandle(text.as_ptr(), text.len() as u32, 0, hsz2, DDEFMT_TEXT, 0) }
        }
        XTYP_POKE => {
            if !is(hsz2, "Value") {
                return 0;
            }
            unsafe { *core::ptr::addr_of_mut!(VALUE) = data_of(hdata) };
            DDE_FACK
        }
        XTYP_EXECUTE => {
            let cmd = data_of(hdata);
            if cmd.starts_with(b"[advise]") {
                unsafe {
                    COUNTER += 1;
                    let item = hsz("Counter");
                    WinDdePostAdvise(inst, 0, item);
                    WinDdeFreeStringHandle(item);
                }
                DDE_FACK
            } else if cmd.starts_with(b"[bye]") {
                unsafe { BYE = true };
                DDE_FACK
            } else {
                0
            }
        }
        XTYP_WILDCONNECT => {
            // Every topic of the service: [("PMXDDEML", "Test"), (0, 0)].
            if hsz1 != 0 && !is(hsz1, "Test") {
                return 0;
            }
            let pairs = [hsz("PMXDDEML"), hsz("Test"), 0, 0];
            let h = unsafe { WinDdeCreateDataHandle(pairs.as_ptr() as *const u8, 16, 0, 0, 0, 0) };
            unsafe {
                WinDdeFreeStringHandle(pairs[0]);
                WinDdeFreeStringHandle(pairs[1]);
            }
            h
        }
        XTYP_ADVSTART => is(hsz2, "Counter") as u32,
        XTYP_DISCONNECT => {
            if unsafe { BYE } {
                unsafe { WinPostMsg(0, 0x2a, 0, 0) };
            }
            0
        }
        _ => 0,
    }
}

/// `PMXTEST DDEMLSERVER`: serves until a DDEML client disconnects (or
/// 30 s); the plain DDE client before it only terminates.
pub fn server(hab: u32) -> u32 {
    let mut inst = 0;
    if unsafe { WinDdeInitialize(&mut inst, server_cb, 0, 0) } != 0 {
        return 2;
    }
    unsafe { SERVER_INST = inst };
    let svc = hsz("PMXDDEML");
    unsafe { WinDdeNameService(inst, svc, 0, DNS_REGISTER) };
    let mut m = QMsg::default();
    unsafe { WinStartTimer(hab, 0, 1, 30000) };
    let mut r = 0;
    while unsafe { WinGetMsg(hab, &mut m, 0, 0, 0) } != 0 {
        if m.msg == WM_TIMER && m.hwnd == 0 {
            r = 1;
            break;
        }
        unsafe { WinDispatchMsg(hab, &m) };
    }
    unsafe {
        WinDdeFreeStringHandle(svc);
        WinDdeUninitialize(inst);
    }
    r
}

// ---- The client ----

/// What the client's callback saw: (type, fmt, hconv, hsz1 string, hsz2
/// string, data, ul1, ul2).
static mut SEEN: Vec<(u32, u32, u32, Vec<u8>, Vec<u8>, Vec<u8>, u32, u32)> = Vec::new();

fn seen() -> &'static mut Vec<(u32, u32, u32, Vec<u8>, Vec<u8>, Vec<u8>, u32, u32)> {
    unsafe { &mut *core::ptr::addr_of_mut!(SEEN) }
}

extern "C" fn client_cb(_inst: u32, typ: u32, fmt: u32, hconv: u32, hsz1: u32, hsz2: u32, hdata: u32, d1: u32, d2: u32) -> u32 {
    let d = if hdata != 0 { data_of(hdata) } else { Vec::new() };
    let s1 = if hsz1 != 0 { query(hsz1) } else { Vec::new() };
    let s2 = if hsz2 != 0 { query(hsz2) } else { Vec::new() };
    seen().push((typ, fmt, hconv, s1, s2, d, d1, d2));
    if typ == XTYP_ADVDATA { DDE_FACK } else { 0 }
}

fn wait_seen(hab: u32, typ: u32) -> Option<(u32, u32, u32, Vec<u8>, Vec<u8>, Vec<u8>, u32, u32)> {
    for _ in 0..100 {
        if let Some(i) = seen().iter().position(|s| s.0 == typ) {
            return Some(seen().remove(i));
        }
        pump_for(hab, 30);
    }
    None
}

fn handle_checks() {
    let a = hsz("Topic");
    let b = hsz("topic");
    let a2 = hsz("Topic");
    check("WinDdeCreateStringHandle: one handle for one string", a != 0 && a == a2 && a != b);
    let mut buf = [0u8; 4];
    let n = unsafe { WinDdeQueryString(a, buf.as_mut_ptr(), 4, 0) };
    let len = unsafe { WinDdeQueryString(a, null_mut(), 0, 0) };
    check("WinDdeQueryString: the string (cut to the buffer) and its length", query(a) == b"Topic" && n == 3 && &buf == b"Top\0" && len == 5);
    let eq_nocase = unsafe { WinDdeCompareStringHandles(a, b, 0) };
    let eq_case = unsafe { WinDdeCompareStringHandles(a, b, 1) };
    let lt = unsafe { WinDdeCompareStringHandles(0, a, 0) };
    check("WinDdeCompareStringHandles: WCS_EQ without case, WCS_LT with it, none first", eq_nocase == 1 && eq_case == 2 && lt == 2);
    unsafe {
        WinDdeKeepStringHandle(a);
        WinDdeFreeStringHandle(a);
        WinDdeFreeStringHandle(a);
    }
    let still = query(a) == b"Topic";
    unsafe { WinDdeFreeStringHandle(a) };
    let gone = unsafe { WinDdeQueryString(a, buf.as_mut_ptr(), 4, 0) } == 0;
    check("WinDdeKeepStringHandle/WinDdeFreeStringHandle count references", still && gone && unsafe { WinGetLastError(0) } & 0xffff == 0x7003);
    unsafe { WinDdeFreeStringHandle(b) };
    check("WinDdeCreateStringHandle: none for an empty string", hsz("") == 0);

    let item = hsz("Item");
    let h = unsafe { WinDdeCreateDataHandle(b"abcdef".as_ptr(), 6, 0, item, DDEFMT_TEXT, 0) };
    let mut b2 = [0u8; 4];
    let got = unsafe { WinDdeGetData(h, b2.as_mut_ptr(), 4, 2) };
    check("WinDdeCreateDataHandle/WinDdeGetData (size, offset)", h != 0 && data_of(h) == b"abcdef" && got == 4 && &b2 == b"cdef");
    let h2 = unsafe { WinDdeAddData(h, b"XYZ".as_ptr(), 3, 5) };
    let mut cb = 0;
    let p = unsafe { WinDdeAccessData(h, &mut cb) };
    let acc = !p.is_null() && cb == 8 && unsafe { core::slice::from_raw_parts(p, 8) } == b"abcdeXYZ";
    check("WinDdeAddData grows the data, WinDdeAccessData", h2 == h && acc);
    check("WinDdeFreeDataHandle", unsafe { WinDdeFreeDataHandle(h) } == 1 && unsafe { WinDdeFreeDataHandle(h) } == 0);
    unsafe { WinDdeFreeStringHandle(item) };
    check("WinDdeQueryModuleVersion", unsafe { WinDdeQueryModuleVersion(0) } == 0x10101);
}

fn start(args: &[u8]) -> u32 {
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, EXEC_ASYNCRESULT, args.as_ptr(), null(), &mut res, b"C:\\RS2\\PMXTEST.EXE\0".as_ptr()) };
    if rc != 0 { 0 } else { res.code_terminate }
}

/// Wait for the child to end, dispatching (it may send to this
/// process's windows on its way out).
fn wait_child(hab: u32, pid: u32) -> bool {
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut p = 0;
    for _ in 0..300 {
        // DCWW_NOWAIT
        let rc = unsafe { DosWaitChild(0, 1, &mut res, &mut p, pid) };
        if rc == 0 {
            return res.code_result == 0;
        }
        pump_for(hab, 50);
    }
    false
}

pub fn ddeml_checks(hab: u32, client: u32) {
    handle_checks();
    let mut inst = 0;
    let rc = unsafe { WinDdeInitialize(&mut inst, client_cb, 0, 0) };
    check("WinDdeInitialize", rc == 0 && inst != 0);
    if inst == 0 {
        return;
    }
    // A monitor of everything the conversations below do.
    let mut monitor = 0;
    let all = APPCLASS_MONITOR | MF_HSZ_INFO | MF_SENDMSGS | MF_POSTMSGS | MF_CALLBACKS | MF_ERRORS | MF_LINKS | MF_CONV;
    check("WinDdeInitialize: a monitor (APPCLASS_MONITOR)", unsafe { WinDdeInitialize(&mut monitor, monitor_cb, all, 0) } == 0 && monitor != 0);
    let child = start(b"PMXTEST\0DDEMLSERVER\0\0");
    check("DDEML server process started", child != 0);
    let reg = wait_seen(hab, XTYP_REGISTER);
    check("XTYP_REGISTER: the server's service, and its instance", reg.as_ref().is_some_and(|r| r.3 == b"PMXDDEML" && r.4.starts_with(b"PMXDDEML:(")));
    let svc = hsz("PMXDDEML");
    let topic = hsz("Test");
    let hconv = unsafe { WinDdeConnect(inst, svc, topic, null()) };
    check("WinDdeConnect to another process's DDEML server", hconv != 0);
    if hconv == 0 {
        return;
    }
    let mut ci = [0u32; 19];
    ci[0] = 0x4c;
    let n = unsafe { WinDdeQueryConvInfo(hconv, QID_SYNC, ci.as_mut_ptr()) };
    let st = ci[8] & 0xffff;
    check("WinDdeQueryConvInfo: connected client, service and topic, partner", n == 0x4c && st & (ST_CONNECTED | ST_CLIENT | ST_ISLOCAL) == ST_CONNECTED | ST_CLIENT | ST_ISLOCAL && is(ci[3], "PMXDDEML") && is(ci[5], "Test") && ci[2] != 0 && ci[17] == hconv);

    let greeting = hsz("Greeting");
    let hd = unsafe { WinDdeSubmitTransaction(null(), 0, hconv, greeting, DDEFMT_TEXT, XTYP_REQUEST, 5000, null_mut()) };
    check("XTYP_REQUEST (synchronous): the data", hd != 0 && data_of(hd) == b"hello from ddeml");
    unsafe { WinDdeFreeDataHandle(hd) };

    let value = hsz("Value");
    let mut res = 0;
    let ok = unsafe { WinDdeSubmitTransaction(b"12345".as_ptr(), 5, hconv, value, DDEFMT_TEXT, XTYP_POKE, 5000, &mut res) };
    let hd = unsafe { WinDdeSubmitTransaction(null(), 0, hconv, value, DDEFMT_TEXT, XTYP_REQUEST, 5000, null_mut()) };
    check("XTYP_POKE acknowledged, the value requested back", ok == 1 && res & 0xffff == DDE_FACK && hd != 0 && data_of(hd) == b"12345");
    unsafe { WinDdeFreeDataHandle(hd) };

    let counter = hsz("Counter");
    let ok = unsafe { WinDdeSubmitTransaction(null(), 0, hconv, counter, DDEFMT_TEXT, XTYP_ADVSTART, 5000, null_mut()) };
    check("XTYP_ADVSTART", ok == 1);
    let ok = unsafe { WinDdeSubmitTransaction(b"[advise]".as_ptr(), 9, hconv, 0, DDEFMT_TEXT, XTYP_EXECUTE, 5000, &mut res) };
    check("XTYP_EXECUTE acknowledged", ok == 1 && res & 0xffff == DDE_FACK);
    let adv = wait_seen(hab, XTYP_ADVDATA);
    check("XTYP_ADVDATA: the advise loop's data (WinDdePostAdvise in the server)", adv.as_ref().is_some_and(|a| a.2 == hconv && a.3 == b"Test" && a.4 == b"Counter" && a.5 == b"1"));
    let bad = unsafe { WinDdeSubmitTransaction(null(), 0, hconv, greeting, DDEFMT_TEXT + 7, XTYP_REQUEST, 5000, null_mut()) };
    check("XTYP_REQUEST refused: no data, DMLERR_NOTPROCESSED", bad == 0 && unsafe { WinGetLastError(0) } & 0xffff == 0x7009);

    let mut id = 0;
    let ok = unsafe { WinDdeSubmitTransaction(null(), 0, hconv, counter, DDEFMT_TEXT, XTYP_REQUEST, TIMEOUT_ASYNC, &mut id) };
    let done = wait_seen(hab, XTYP_XACT_COMPLETE);
    check("asynchronous XTYP_REQUEST: XTYP_XACT_COMPLETE with its id and data", ok == 1 && id != 0 && done.as_ref().is_some_and(|d| d.6 == id && d.4 == b"Counter" && d.5 == b"1"));
    let ok = unsafe { WinDdeSubmitTransaction(null(), 0, hconv, counter, DDEFMT_TEXT, XTYP_ADVSTOP, 5000, null_mut()) };
    check("XTYP_ADVSTOP", ok == 1);
    check("WinDdeReconnect (IBM's never reconnects)", unsafe { WinDdeReconnect(hconv) } == 0);
    pump_for(hab, 20);
    monitor_checks();
    unsafe { WinDdeUninitialize(monitor) };
    let n = mon().len();
    unsafe { WinDdeFreeStringHandle(hsz("After")) };
    check("WinDdeUninitialize: the monitor hears no more", mon().len() == n);

    // A plain PM DDE client talks to the DDEML server.
    seen().clear();
    crate::dde::got().clear();
    unsafe { WinDdeInitiate(client, b"PMXDDEML\0".as_ptr(), b"Test\0".as_ptr(), null()) };
    pump_for(hab, 100);
    let ack = crate::dde::got().iter().position(|g| g.0 == 0xa9).map(|i| crate::dde::got().remove(i));
    check("WinDdeInitiate: the DDEML server answers a plain DDE client", ack.as_ref().is_some_and(|a| a.3 == b"PMXDDEML" && a.4 == b"Test"));
    if let Some(a) = ack {
        let req = crate::dde::ddestruct(0, b"Greeting", b"");
        unsafe { WinDdePostMsg(a.1, client, 0xa1, req, 0) };
        let d = crate::dde::wait_for(hab, 0xa3);
        check("WM_DDE_REQUEST to the DDEML server: WM_DDE_DATA", d.is_some_and(|d| d.2 & 0x10 != 0 && d.3 == b"Greeting" && d.4 == b"hello from ddeml"));
        unsafe { WinDdePostMsg(a.1, client, 0xa8, null_mut(), 0) };
        let t = crate::dde::wait_for(hab, 0xa8);
        check("WM_DDE_TERMINATE answered by the DDEML server", t.is_some());
    }

    // Every server of every topic (XTYP_WILDCONNECT).
    let list = unsafe { WinDdeConnectList(inst, 0, 0, 0, null()) };
    let first = unsafe { WinDdeQueryNextServer(list, 0) };
    let mut ci = [0u32; 19];
    ci[0] = 0x4c;
    unsafe { WinDdeQueryConvInfo(first, QID_SYNC, ci.as_mut_ptr()) };
    let hd = unsafe { WinDdeSubmitTransaction(null(), 0, first, greeting, DDEFMT_TEXT, XTYP_REQUEST, 5000, null_mut()) };
    check("WinDdeConnectList (XTYP_WILDCONNECT): one server, a request on it", list != 0 && first != 0 && unsafe { WinDdeQueryNextServer(list, first) } == 0 && ci[10] == list && is(ci[3], "PMXDDEML") && hd != 0 && data_of(hd) == b"hello from ddeml");
    unsafe {
        WinDdeFreeDataHandle(hd);
        WinDdeDisconnectList(list);
    }
    unsafe { WinDdeSubmitTransaction(b"[bye]".as_ptr(), 6, hconv, 0, DDEFMT_TEXT, XTYP_EXECUTE, 5000, null_mut()) };
    unsafe { WinDdeDisconnect(hconv) };
    check("WinDdeDisconnect: the server sees XTYP_DISCONNECT and ends", wait_child(hab, child));
    for h in [svc, topic, greeting, value, counter] {
        unsafe { WinDdeFreeStringHandle(h) };
    }

    // A DDEML client and PMXTEST's plain DDE server.
    let child = start(b"PMXTEST\0DDESERVER\0\0");
    let svc = hsz("PMXTEST");
    let topic = hsz("System");
    let mut hconv = 0;
    for _ in 0..30 {
        hconv = unsafe { WinDdeConnect(inst, svc, topic, null()) };
        if hconv != 0 {
            break;
        }
        pump_for(hab, 50);
    }
    check("WinDdeConnect to a plain DDE server", hconv != 0);
    let greeting = hsz("Greeting");
    let hd = if hconv != 0 { unsafe { WinDdeSubmitTransaction(null(), 0, hconv, greeting, DDEFMT_TEXT, XTYP_REQUEST, 5000, null_mut()) } } else { 0 };
    check("XTYP_REQUEST to a plain DDE server", hd != 0 && data_of(hd) == b"hello from the server");
    unsafe {
        WinDdeFreeDataHandle(hd);
        WinDdeDisconnect(hconv);
    }
    check("the plain DDE server ends after WinDdeDisconnect", wait_child(hab, child));
    for h in [svc, topic, greeting] {
        unsafe { WinDdeFreeStringHandle(h) };
    }
    check("WinDdeUninitialize", unsafe { WinDdeUninitialize(inst) } == 1 && unsafe { WinDdeUninitialize(inst) } == 0);
    let _ = (XTYP_CONNECT_CONFIRM, XTYP_DISCONNECT);
}
