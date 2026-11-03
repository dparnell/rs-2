//! PMDDEML.DLL: the DDE management library (the Win32 DDEML's calls in
//! PM, `WinDde*`), as IBM's PMDDEML.DLL has them (studied with Ghidra and
//! pmddeml.sym: the `Win32Dde*` entry points and what they call). Open32
//! (PMWINX) builds the Win32 Dde* calls on it; the Toolkit has no header
//! for it, so the arguments are IBM's entry points' (see each function).
//!
//! It works with PM's own DDE (WM_DDE_* messages, WinDdeRespond,
//! WinDdePostMsg, DDESTRUCTs in giveable shared memory), so DDEML
//! clients talk to any PM DDE server and DDEML servers answer any PM DDE
//! client. The instance and conversations are windows (conv.rs), string
//! and data handles a table (handles.rs). Errors are the PM error ids
//! IBM's DdeSetErrorInfo sets (WinGetLastError); there is no
//! WinDdeGetLastError. Monitors (APPCLASS_MONITOR, XTYP_MONITOR) are in
//! mon.rs.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr::null;

mod conv;
mod exports;
mod handles;
mod mon;
mod pm;

use conv::*;
use handles::*;
use pm::*;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

pub type Callback = extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32;

// DMLERR_* (PM error ids, IBM's DdeSetErrorInfo).
pub const DMLERR_DLL_USAGE: u32 = 0x7002;
pub const DMLERR_INVALIDPARAMETER: u32 = 0x7003;
pub const DMLERR_NO_CONV_ESTABLISHED: u32 = 0x7005;
pub const DMLERR_DLL_NOT_INITIALIZED: u32 = 0x7007;
pub const DMLERR_INVALID_INSTANCE: u32 = 0x7008;
pub const DMLERR_NOTPROCESSED: u32 = 0x7009;
pub const DMLERR_REENTRANCY: u32 = 0x700b;
pub const DMLERR_SERVER_DIED: u32 = 0x700c;
pub const DMLERR_SYS_ERROR: u32 = 0x700d;
pub const DMLERR_ADVACKTIMEOUT: u32 = 0x700e;
pub const DMLERR_EXECACKTIMEOUT: u32 = 0x700f;
pub const DMLERR_POKEACKTIMEOUT: u32 = 0x7010;
pub const DMLERR_DATAACKTIMEOUT: u32 = 0x7011;
pub const DMLERR_UNADVACKTIMEOUT: u32 = 0x7012;
pub const DMLERR_UNFOUND_QUEUE_ID: u32 = 0x7013;
/// rs2's name for a failed WinDdePostMsg (IBM sets DMLERR_SYS_ERROR).
pub const DMLERR_POSTMSG_FAILED: u32 = DMLERR_SYS_ERROR;

// Transaction types (XTYP_*, the Win32 values).
pub const XTYPF_NOBLOCK: u32 = 0x0002;
pub const XTYPF_NODATA: u16 = 0x0004;
pub const XTYPF_ACKREQ: u16 = 0x0008;
pub const XTYP_ADVDATA: u32 = 0x4010;
pub const XTYP_ADVREQ: u32 = 0x2022;
pub const XTYP_ADVSTART: u16 = 0x1030;
pub const XTYP_ADVSTOP: u16 = 0x8040;
pub const XTYP_EXECUTE: u16 = 0x4050;
pub const XTYP_CONNECT: u32 = 0x1062;
pub const XTYP_CONNECT_CONFIRM: u32 = 0x8072;
pub const XTYP_XACT_COMPLETE: u32 = 0x8080;
pub const XTYP_POKE: u16 = 0x4090;
pub const XTYP_REGISTER: u32 = 0x80a2;
pub const XTYP_REQUEST: u16 = 0x20b0;
pub const XTYP_DISCONNECT: u32 = 0x80c2;
pub const XTYP_UNREGISTER: u32 = 0x80d2;
pub const XTYP_WILDCONNECT: u32 = 0x20e2;

// Instance flags (afCmd).
pub const APPCLASS_MONITOR: u32 = 0x0001;
pub const APPCMD_CLIENTONLY: u32 = 0x0010;
pub const APPCMD_FILTERINITS: u32 = 0x0020;
pub const CBF_FAIL_SELFCONNECTIONS: u32 = 0x1000;
pub const CBF_FAIL_CONNECTIONS: u32 = 0x2000;
pub const CBF_FAIL_ADVISES: u32 = 0x4000;
pub const CBF_FAIL_EXECUTES: u32 = 0x8000;
pub const CBF_FAIL_POKES: u32 = 0x10000;
pub const CBF_FAIL_REQUESTS: u32 = 0x20000;
pub const CBF_SKIP_CONNECT_CONFIRMS: u32 = 0x40000;
pub const CBF_SKIP_REGISTRATIONS: u32 = 0x80000;
pub const CBF_SKIP_UNREGISTRATIONS: u32 = 0x100000;
pub const CBF_SKIP_DISCONNECTS: u32 = 0x200000;

pub const CBR_BLOCK: u32 = 0xffff_ffff;
pub const TIMEOUT_ASYNC: u32 = 0xffff_ffff;
pub const QID_SYNC: u32 = 0xffff_ffff;
pub const MSGF_DDEMGR: u32 = 0x8001;

pub const EC_ENABLEALL: u16 = 0;
pub const EC_ENABLEONE: u16 = 0x0080;
pub const EC_DISABLE: u16 = 0x0008;
pub const EC_QUERYWAITING: u32 = 0x0002;

pub const DNS_REGISTER: u32 = 1;
pub const DNS_UNREGISTER: u32 = 2;
pub const DNS_FILTERON: u32 = 4;
pub const DNS_FILTEROFF: u32 = 8;

/// An instance (the agent window's data): afCmd, the callback, the
/// frame it lives in and the services it registered.
pub struct Inst {
    pub flags: u32,
    pub pfn: Callback,
    pub frame: u32,
    pub names: Vec<u32>,
}

pub fn instance(h: u32) -> Option<&'static mut Inst> {
    if h == 0 || unsafe { WinIsWindow(0, h) } == 0 || !is_class(h, AGENT_CLASS) {
        return None;
    }
    let p = unsafe { WinQueryWindowPtr(h, 0) } as *mut Inst;
    if p.is_null() { None } else { Some(unsafe { &mut *p }) }
}

/// IBM's IsValidInstId.
fn valid_instance(h: u32) -> Option<&'static mut Inst> {
    let i = instance(h);
    if i.is_none() {
        set_error(DMLERR_INVALID_INSTANCE, 0);
    }
    i
}

/// IBM's DdeSetErrorInfo: the PM error id (severity by error) for
/// WinGetLastError.
pub fn set_error(e: u32, _conv: u32) {
    let sev = match e {
        0 => 0,
        DMLERR_DLL_USAGE | DMLERR_DLL_NOT_INITIALIZED | DMLERR_INVALID_INSTANCE | DMLERR_REENTRANCY => 8,
        DMLERR_SYS_ERROR | 0x7004 | 0x7006 => 0xc,
        _ => 4,
    };
    // Monitors first: what they are sent must not change the error.
    mon::error(e);
    unsafe { WinSetErrorInfo(sev << 16 | e, 0x6000) };
}

pub fn my_pid() -> u32 {
    let mut tib = core::ptr::null_mut();
    let mut pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    unsafe { (*pib).pid }
}

static mut FRAME_PROC: Option<WndProc> = None;
static mut REGISTERED: bool = false;

/// The instance's frame passes everything to the frame procedure but
/// WM_CLOSE (IBM's FrameWindowProc); rs2 also hands it the registration
/// messages for its client.
extern "C" fn frame_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CLOSE => 0,
        AM_REGISTER | AM_UNREGISTER | mon::AM_MONITOR => {
            let client = unsafe { WinWindowFromID(h, FID_CLIENT) };
            if is_class(client, AGENT_CLASS) {
                unsafe { WinSendMsg(client, msg, mp1, mp2) };
            }
            0
        }
        _ => match unsafe { FRAME_PROC } {
            Some(f) => f(h, msg, mp1, mp2),
            None => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
        },
    }
}

unsafe extern "C" {
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
}

/// WinDdeInitialize(pidInst, pfnCallback, afCmd, ulRes): a new instance
/// when *pidInst is 0 (its id is the agent window), else the callback
/// filter flags of an existing one are changed. Returns a DMLERR_ value
/// (0: done).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeInitialize(pid: *mut u32, pfn: Option<Callback>, af: u32, res: u32) -> u32 {
    let mut qi = [0u32; 5];
    if unsafe { WinQueryQueueInfo(HMQ_CURRENT, qi.as_mut_ptr() as *mut u8, 20) } == 0 {
        set_error(DMLERR_DLL_NOT_INITIALIZED, 0);
        return DMLERR_DLL_NOT_INITIALIZED;
    }
    let (Some(pfn), 0, false) = (pfn, res, pid.is_null()) else {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return DMLERR_INVALIDPARAMETER;
    };
    let cur = unsafe { pid.read_unaligned() };
    if cur != 0 {
        let Some(i) = instance(cur) else {
            set_error(DMLERR_INVALIDPARAMETER, 0);
            return DMLERR_INVALIDPARAMETER;
        };
        i.flags = (af & 0x7fff_f000) | (i.flags & 0x8000_0fff);
        set_error(0, 0);
        return 0;
    }
    unsafe {
        if !REGISTERED {
            WinRegisterClass(0, AGENT_CLASS.as_ptr(), agent_proc, 0, 4);
            WinRegisterClass(0, CONV_CLASS.as_ptr(), conv_proc, 0, 4);
            WinRegisterClass(0, LIST_CLASS.as_ptr(), list_proc, 0, 4);
            REGISTERED = true;
        }
    }
    // FRAMECDATA: cb 10, no frame controls.
    let fcd: [u16; 5] = [10, 0, 0, 0, 0];
    let frame = unsafe { WinCreateWindow(HWND_DESKTOP, 0xffff_0001 as *const u8, null(), WS_DISABLED, 0, 0, 0, 0, 0, 3, 0, fcd.as_ptr() as *const u8, null()) };
    if frame == 0 {
        set_error(DMLERR_SYS_ERROR, 0);
        return DMLERR_SYS_ERROR;
    }
    let old = unsafe { WinSubclassWindow(frame, frame_proc) };
    unsafe {
        if (*core::ptr::addr_of!(FRAME_PROC)).is_none() {
            FRAME_PROC = old;
        }
    }
    let agent = unsafe { WinCreateWindow(frame, AGENT_CLASS.as_ptr(), null(), 0, 0, 0, 0, 0, frame, 3, FID_CLIENT, null(), null()) };
    if agent == 0 {
        unsafe { WinDestroyWindow(frame) };
        set_error(DMLERR_SYS_ERROR, 0);
        return DMLERR_SYS_ERROR;
    }
    // A monitor: the system hooks that report every process's DDE
    // messages (IBM's Win32DdeInitialize).
    if af & APPCLASS_MONITOR != 0 && !mon::start() {
        unsafe { WinDestroyWindow(frame) };
        set_error(DMLERR_SYS_ERROR, 0);
        return DMLERR_SYS_ERROR;
    }
    let i = Box::new(Inst { flags: af | APPCMD_FILTERINITS, pfn, frame, names: Vec::new() });
    unsafe {
        WinSetWindowPtr(agent, 0, Box::into_raw(i) as u32);
        pid.write_unaligned(agent);
    }
    set_error(0, 0);
    0
}

/// WinDdeUninitialize(idInst): every conversation is closed, the
/// instance's windows go.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeUninitialize(inst: u32) -> u32 {
    let Some(i) = valid_instance(inst) else { return 0 };
    let frame = i.frame;
    if i.flags & APPCLASS_MONITOR != 0 {
        i.flags &= !APPCLASS_MONITOR;
        mon::stop();
    }
    for n in core::mem::take(&mut i.names) {
        broadcast_name(inst, AM_UNREGISTER, n);
        free_string(n);
    }
    for k in children(inst) {
        unsafe { WinSendMsg(k, WM_CLOSE, 0, 0) };
    }
    unsafe { WinDestroyWindow(frame) };
    set_error(0, 0);
    1
}

/// Tell the other instances (every frame on the desktop) about a
/// registered service: mp1 the instance, mp2 a system atom of the name.
fn broadcast_name(inst: u32, msg: u32, hsz: u32) {
    let Some((mut s, _, _)) = string(hsz) else { return };
    s.push(0);
    let t = unsafe { WinQuerySystemAtomTable() };
    let atom = unsafe { WinAddAtom(t, s.as_ptr()) };
    if atom == 0 {
        return;
    }
    let frame = unsafe { WinQueryWindow(inst, QW_PARENT) };
    for k in children(HWND_DESKTOP) {
        if k != frame && is_class(k, b"#1\0") {
            unsafe { WinSendMsg(k, msg, inst, atom) };
        }
    }
    unsafe { WinDeleteAtom(t, atom) };
}

/// WinDdeConnect(idInst, hszService, hszTopic, pConvContext): a
/// conversation with the first server (frame on the desktop) that takes
/// it; a service handle naming an instance goes to that instance only.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeConnect(inst: u32, svc: u32, topic: u32, cc: *const u32) -> u32 {
    if valid_instance(inst).is_none() {
        return 0;
    }
    if (svc != 0 && !is_string(svc)) || (topic != 0 && !is_string(topic)) || (!cc.is_null() && unsafe { cc.read_unaligned() } != 24) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    let h = connect(inst, inst, svc, topic, context(cc), None);
    if h == 0 {
        set_error(DMLERR_NO_CONV_ESTABLISHED, 0);
        return 0;
    }
    set_error(0, 0);
    h
}

/// The conversation context given, or the process's (IBM's: country and
/// code page from DosQueryCtryInfo).
fn context(cc: *const u32) -> [u32; 6] {
    if cc.is_null() {
        let mut ci = os2::CountryInfo::default();
        let code = os2::CountryCode { country: 0, codepage: 0 };
        let mut n = 0;
        unsafe { os2::DosQueryCtryInfo(core::mem::size_of::<os2::CountryInfo>() as u32, &code, &mut ci, &mut n) };
        return [24, 0, ci.country, if ci.codepage != 0 { ci.codepage } else { process_cp() }, 0, 0];
    }
    let mut a = [0u32; 6];
    for (i, v) in a.iter_mut().enumerate() {
        *v = unsafe { cc.add(i).read_unaligned() };
    }
    a
}

/// IBM's DdeConnect: a conversation window under `parent` (the agent or
/// a list) initiating with each frame in turn until one answers (`only`:
/// that frame only).
fn connect(parent: u32, inst: u32, svc: u32, topic: u32, cc: [u32; 6], only: Option<u32>) -> u32 {
    let svc_s = string(svc);
    let topic_s = string(topic).map(|t| t.0).unwrap_or_default();
    let target = match (only, &svc_s) {
        (Some(f), _) => Some(f),
        // An instance's service: its frame.
        (None, Some((_, _, hw))) if *hw != 0 => Some(unsafe { WinQueryWindow(*hw, QW_PARENT) }),
        _ => None,
    };
    let svc_name = svc_s.map(|s| s.0).unwrap_or_default();
    // The conversation asks for the plain service name.
    let svc_plain = if svc != 0 && svc & H_INST != 0 { string_handle(&svc_name, 0, 0) } else { svc };
    let h = create_conv(parent, inst, svc_plain, topic, Some(cc), CONV_CLASS);
    if svc_plain != svc {
        free_string(svc_plain);
    }
    if h == 0 {
        return 0;
    }
    let frames: Vec<u32> = match target {
        Some(f) => alloc::vec![f],
        None => {
            let me = unsafe { WinQueryWindow(inst, QW_PARENT) };
            let mut v: Vec<u32> = children(HWND_DESKTOP).into_iter().filter(|&k| is_class(k, b"#1\0")).collect();
            // Its own instance last (it may serve itself).
            if let Some(p) = v.iter().position(|&k| k == me) {
                let f = v.remove(p);
                v.push(f);
            }
            v
        }
    };
    for f in frames {
        connect_request(h, f, &svc_name, &topic_s, &cc);
        if conv(h).is_some_and(|c| c.status & ST_CONNECTED != 0) {
            return h;
        }
    }
    unsafe { WinDestroyWindow(h) };
    0
}

/// WinDdeConnectList(idInst, hszService, hszTopic, hConvList,
/// pConvContext): conversations with every server that takes them, in a
/// list window (IBM returns an existing list as it is).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeConnectList(inst: u32, svc: u32, topic: u32, list: u32, cc: *const u32) -> u32 {
    if valid_instance(inst).is_none() {
        return 0;
    }
    if list != 0 {
        if is_class(list, LIST_CLASS) {
            set_error(0, 0);
            return list;
        }
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    if (svc != 0 && !is_string(svc)) || (topic != 0 && !is_string(topic)) || (!cc.is_null() && unsafe { cc.read_unaligned() } != 24) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    let cc = context(cc);
    let l = create_conv(inst, inst, svc, topic, Some(cc), LIST_CLASS);
    if l == 0 {
        set_error(DMLERR_SYS_ERROR, 0);
        return 0;
    }
    let svc_s = string(svc).map(|s| s.0).unwrap_or_default();
    let topic_s = string(topic).map(|t| t.0).unwrap_or_default();
    let me = unsafe { WinQueryWindow(inst, QW_PARENT) };
    let mut frames: Vec<u32> = children(HWND_DESKTOP).into_iter().filter(|&k| is_class(k, b"#1\0")).collect();
    if let Some(p) = frames.iter().position(|&k| k == me) {
        let f = frames.remove(p);
        frames.push(f);
    }
    // Every answer is terminated at once and connected to again, one
    // conversation per service and topic.
    for f in frames {
        connect_request(l, f, &svc_s, &topic_s, &cc);
    }
    let answers = conv(l).map(|c| core::mem::take(&mut c.answers)).unwrap_or_default();
    let mut n = 0;
    for (s, t, f) in answers {
        let hs = string_handle(&s, cc[3], 0);
        let ht = string_handle(&t, cc[3], 0);
        if connect(l, inst, hs, ht, cc, Some(f)) != 0 {
            n += 1;
        }
        free_string(hs);
        free_string(ht);
    }
    if n == 0 {
        unsafe { WinDestroyWindow(l) };
        set_error(DMLERR_NO_CONV_ESTABLISHED, 0);
        return 0;
    }
    set_error(0, 0);
    l
}

/// WinDdeQueryNextServer(hConvList, hConvPrev): the next connected
/// conversation of the list.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeQueryNextServer(list: u32, prev: u32) -> u32 {
    if !is_class(list, LIST_CLASS) || (prev != 0 && !is_class(prev, CONV_CLASS)) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    let ks = children(list);
    let start = if prev == 0 {
        0
    } else {
        match ks.iter().position(|&k| k == prev) {
            Some(p) => p + 1,
            None => ks.len(),
        }
    };
    set_error(0, 0);
    ks[start.min(ks.len())..].iter().copied().find(|&k| conv(k).is_some_and(|c| c.status & ST_CONNECTED != 0)).unwrap_or(0)
}

/// WinDdeDisconnect(hConv).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeDisconnect(h: u32) -> u32 {
    if h == 0 || !is_class(h, CONV_CLASS) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    set_error(0, 0);
    unsafe { WinSendMsg(h, WM_CLOSE, 0, 0) };
    1
}

/// WinDdeDisconnectList(hConvList).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeDisconnectList(list: u32) -> u32 {
    if list == 0 || !is_class(list, LIST_CLASS) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    set_error(0, 0);
    unsafe {
        WinSendMsg(list, WM_CLOSE, 0, 0);
        WinDestroyWindow(list);
    }
    1
}

/// WinDdeReconnect(hConv): IBM's never reconnects (its DdeReconnect
/// returns no conversation): DMLERR_NO_CONV_ESTABLISHED.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeReconnect(h: u32) -> u32 {
    if !is_class(h, CONV_CLASS) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    set_error(DMLERR_NO_CONV_ESTABLISHED, h);
    0
}

/// WinDdeQueryConvInfo(hConv, idTransaction, pConvInfo): CONVINFO (cb
/// 0x4C: hUser, hConvPartner, hszSvcPartner, hszServiceReq, hszTopic,
/// hszItem, usFmt, usType, usStatus, usConvst, ulLastError, hConvList,
/// ConvCtxt, hwnd, hwndPartner); returns its size.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeQueryConvInfo(h: u32, id: u32, ci: *mut u32) -> u32 {
    let Some(c) = (if is_class(h, CONV_CLASS) { conv(h) } else { None }) else {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    };
    if ci.is_null() || unsafe { ci.read_unaligned() } != 0x4c {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    let x = c.xacts.iter().find(|x| x.id == id);
    if x.is_none() && id != QID_SYNC {
        set_error(DMLERR_UNFOUND_QUEUE_ID, h);
        return 0;
    }
    let mut v = [0u32; 19];
    v[0] = 0x4c;
    v[1] = c.user;
    v[2] = c.partner;
    let client = c.status & ST_CLIENT != 0;
    v[3] = if client { c.svc } else { 0 };
    v[4] = c.svc_req;
    v[5] = if client { c.topic } else { c.topic_req };
    if let Some(x) = x {
        v[1] = x.user;
        v[6] = x.item;
        v[7] = x.fmt as u32 | (x.typ as u32) << 16;
        v[8] = c.status as u32 | (x.state as u32) << 16;
    } else {
        v[7] = 0;
        v[8] = c.status as u32;
    }
    v[9] = c.last_err;
    v[10] = c.list;
    v[11..17].copy_from_slice(&c.cc);
    v[17] = h;
    v[18] = c.partner;
    for (i, w) in v.iter().enumerate() {
        unsafe { ci.add(i).write_unaligned(*w) };
    }
    set_error(0, h);
    0x4c
}

/// WinDdeSetUserHandle(hConv, idTransaction, hUser): the conversation's
/// (QID_SYNC) or a transaction's user handle.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeSetUserHandle(h: u32, id: u32, user: u32) -> u32 {
    let Some(c) = (if is_class(h, CONV_CLASS) { conv(h) } else { None }) else {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    };
    if id == QID_SYNC {
        c.user = user;
    } else if let Some(x) = c.xacts.iter_mut().find(|x| x.id == id) {
        x.user = user;
    } else {
        set_error(DMLERR_INVALIDPARAMETER, h);
        return 0;
    }
    set_error(0, h);
    1
}

/// WinDdeEnableCallback(idInst, hConv, usCmd): EC_ENABLEALL,
/// EC_ENABLEONE, EC_DISABLE for one conversation or all of the
/// instance's; EC_QUERYWAITING says whether one has messages held.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeEnableCallback(inst: u32, h: u32, cmd: u32) -> u32 {
    if valid_instance(inst).is_none() {
        return 0;
    }
    if h != 0 && !is_class(h, CONV_CLASS) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    if cmd & EC_QUERYWAITING != 0 {
        if h == 0 {
            set_error(DMLERR_INVALIDPARAMETER, 0);
            return 0;
        }
        return conv(h).is_some_and(|c| !c.queue.is_empty()) as u32;
    }
    if cmd & 0xff77 != 0 {
        set_error(DMLERR_INVALIDPARAMETER, h);
        return 0;
    }
    if h == 0 {
        for k in convs_of(inst) {
            unsafe { WinSendMsg(k, UM_ENABLE, cmd, 0) };
        }
    } else {
        unsafe { WinSendMsg(h, UM_ENABLE, cmd, 0) };
    }
    set_error(0, h);
    1
}

/// WinDdeNameService(idInst, hszService, hszReserved, afCmd):
/// DNS_REGISTER/DNS_UNREGISTER a service (all for none), DNS_FILTERON/
/// DNS_FILTEROFF answering only registered services.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeNameService(inst: u32, svc: u32, res: u32, af: u32) -> u32 {
    let Some(i) = instance(inst) else {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    };
    if i.flags & APPCMD_CLIENTONLY != 0 {
        set_error(DMLERR_DLL_USAGE, 0);
        return 0;
    }
    if af & !0xf != 0 || res != 0 {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    if af & DNS_FILTERON != 0 {
        i.flags |= APPCMD_FILTERINITS;
    }
    if af & DNS_FILTEROFF != 0 {
        i.flags &= !APPCMD_FILTERINITS;
    }
    if af & DNS_REGISTER != 0 {
        if !is_string(svc) {
            set_error(DMLERR_INVALIDPARAMETER, 0);
            return 0;
        }
        if !i.names.iter().any(|&n| equal(n, svc, false)) {
            keep_string(svc);
            i.names.push(svc);
            broadcast_name(inst, AM_REGISTER, svc);
        }
    }
    if af & DNS_UNREGISTER != 0 {
        let gone: Vec<u32> = if svc == 0 {
            core::mem::take(&mut i.names)
        } else {
            if !is_string(svc) {
                set_error(DMLERR_INVALIDPARAMETER, 0);
                return 0;
            }
            let (g, keep): (Vec<u32>, Vec<u32>) = core::mem::take(&mut i.names).into_iter().partition(|&n| equal(n, svc, false));
            i.names = keep;
            g
        };
        for n in gone {
            broadcast_name(inst, AM_UNREGISTER, n);
            free_string(n);
        }
    }
    set_error(0, 0);
    1
}

/// WinDdePostAdvise(idInst, hszTopic, hszItem): the advise loops of the
/// topic and item (all for none) get new data (XTYP_ADVREQ).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdePostAdvise(inst: u32, topic: u32, item: u32) -> u32 {
    let Some(i) = instance(inst) else {
        set_error(DMLERR_INVALID_INSTANCE, 0);
        return 0;
    };
    if i.flags & APPCMD_CLIENTONLY != 0 {
        set_error(DMLERR_DLL_USAGE, 0);
        return 0;
    }
    if (topic != 0 && !is_string(topic)) || (item != 0 && !is_string(item)) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    post_advise(inst, topic, item);
    set_error(0, 0);
    1
}

/// WinDdeSubmitTransaction(pData, cbData, hConv, hszItem, usFmt, usType,
/// ulTimeout, pulResult): a client transaction (XTYP_REQUEST, POKE,
/// EXECUTE, ADVSTART, ADVSTOP); cbData -1 makes pData a data handle.
/// Synchronous: the data handle or TRUE; TIMEOUT_ASYNC: TRUE and the
/// transaction id in *pulResult, XTYP_XACT_COMPLETE later.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeSubmitTransaction(pdata: u32, cb: u32, h: u32, item: u32, fmt: u32, typ: u32, timeout: u32, presult: *mut u32) -> u32 {
    let (fmt, typ) = (fmt as u16, typ as u16);
    if typ & 0xf0fe == 0 {
        set_error(DMLERR_INVALIDPARAMETER, h);
        return 0;
    }
    if typ == XTYP_EXECUTE || typ == XTYP_POKE {
        if item == 0 && (pdata == 0 || cb == 0) {
            set_error(DMLERR_INVALIDPARAMETER, h);
            return 0;
        }
        if cb == 0xffff_ffff && !is_data(pdata) {
            set_error(DMLERR_INVALIDPARAMETER, h);
            return 0;
        }
    }
    if typ != XTYP_EXECUTE && !is_string(item) {
        set_error(DMLERR_INVALIDPARAMETER, h);
        return 0;
    }
    if !is_class(h, CONV_CLASS) {
        set_error(DMLERR_INVALIDPARAMETER, h);
        return 0;
    }
    set_error(0, h);
    submit(pdata, cb, h, item, fmt, typ, timeout, presult)
}

/// WinDdeAbandonTransaction(idInst, hConv, idTransaction): forget an
/// asynchronous transaction (all of the conversation's for 0, every
/// conversation's for no conversation).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeAbandonTransaction(inst: u32, h: u32, id: u32) -> u32 {
    if instance(inst).is_none() {
        set_error(DMLERR_INVALID_INSTANCE, h);
        return 0;
    }
    if h == 0 {
        for k in convs_of(inst) {
            unsafe { WinSendMsg(k, UM_ABANDON, 0, 0) };
        }
    } else {
        if !conv(h).is_some_and(|c| c.status & ST_CLIENT != 0) {
            set_error(DMLERR_INVALIDPARAMETER, h);
            return 0;
        }
        if unsafe { WinSendMsg(h, UM_ABANDON, id, 0) } == 0 {
            set_error(DMLERR_UNFOUND_QUEUE_ID, h);
            return 0;
        }
    }
    set_error(0, h);
    1
}

// ---- String handles ----

/// WinDdeCreateStringHandle(psz, ulCodepage): 0 for an empty string.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeCreateStringHandle(psz: *const u8, cp: u32) -> u32 {
    set_error(0, 0);
    if psz.is_null() {
        return 0;
    }
    let s: Vec<u8> = (0..).map(|i| unsafe { *psz.add(i) }).take_while(|&b| b != 0).collect();
    let h = string_handle(&s, cp, 0);
    if h != 0 {
        mon::hsz(h, mon::MH_CREATE);
    }
    h
}

/// WinDdeQueryString(hsz, pszBuf, cchMax, ulCodepage): the string's
/// length (without its NUL).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeQueryString(hsz: u32, buf: *mut u8, cch: u32, cp: u32) -> u32 {
    if !is_string(hsz) || (!buf.is_null() && cch == 0) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    let n = query_string(hsz, buf, cch, cp);
    set_error(0, 0);
    n
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDdeFreeStringHandle(hsz: u32) -> u32 {
    if !is_string(hsz) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    mon::hsz(hsz, mon::MH_DELETE);
    free_string(hsz);
    set_error(0, 0);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDdeKeepStringHandle(hsz: u32) -> u32 {
    if !is_string(hsz) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    keep_string(hsz);
    mon::hsz(hsz, mon::MH_KEEP);
    set_error(0, 0);
    1
}

/// WinDdeCompareStringHandles(hsz1, hsz2, fCaseSensitive): WCS_EQ,
/// WCS_LT or WCS_GT (no handle sorts first).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeCompareStringHandles(h1: u32, h2: u32, case: u32) -> u32 {
    if h1 == 0 {
        return if h2 == 0 { 1 } else { 2 };
    }
    if h2 == 0 {
        return 3;
    }
    if !is_string(h1) || !is_string(h2) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    set_error(0, 0);
    compare(h1, h2, case & 1 != 0)
}

// ---- Data handles ----

/// WinDdeCreateDataHandle(pSrc, cb, cbOff, hszItem, usFmt, afCmd): a
/// data handle with a copy of the data (IBM's ignores cbOff);
/// HDATA_APPOWNED (2) keeps it for the application to free.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeCreateDataHandle(src: *const u8, cb: u32, _off: u32, item: u32, fmt: u32, af: u32) -> u32 {
    if (item != 0 && !is_string(item)) || af & !HDATA_APPOWNED != 0 {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    let p = make_struct(item, src, cb, cb, fmt as u16, 0);
    set_error(0, 0);
    data_handle(p, af)
}

/// WinDdeAddData(hData, pSrc, cb, cbOff): the data at cbOff (the handle's
/// data grows).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeAddData(hd: u32, src: *const u8, cb: u32, off: u32) -> u32 {
    let Some((p, _)) = (if src.is_null() { None } else { data(hd) }) else {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    };
    let mut p = p;
    let d = parse(p);
    if off + cb > d.cb {
        let item = string_handle(&d.item, 0, 0);
        let np = make_struct(item, d.data, d.cb, off + cb, d.fmt, d.status);
        free_string(item);
        if np.is_null() {
            set_error(DMLERR_SYS_ERROR, 0);
            return 0;
        }
        unsafe { os2::DosFreeMem(p) };
        set_data(hd, np);
        p = np;
    }
    let d = parse(p);
    unsafe { core::ptr::copy_nonoverlapping(src, d.data.add(off as usize), cb as usize) };
    set_error(0, 0);
    hd
}

/// WinDdeGetData(hData, pDst, cbMax, cbOff): the data from cbOff (the
/// size of the data when pDst is null).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeGetData(hd: u32, dst: *mut u8, cbmax: u32, off: u32) -> u32 {
    let Some((p, _)) = data(hd) else {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    };
    let d = parse(p);
    set_error(0, 0);
    if dst.is_null() {
        return d.cb;
    }
    let n = cbmax.min(d.cb);
    if n < off || off >= d.cb {
        return 0;
    }
    let n = n.min(d.cb - off);
    unsafe { core::ptr::copy_nonoverlapping(d.data.add(off as usize), dst, n as usize) };
    n
}

/// WinDdeAccessData(hData, pcbData): the data itself.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeAccessData(hd: u32, pcb: *mut u32) -> *mut u8 {
    let Some((p, _)) = data(hd) else {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return core::ptr::null_mut();
    };
    let d = parse(p);
    if !pcb.is_null() {
        unsafe { pcb.write_unaligned(d.cb) };
    }
    set_error(0, 0);
    d.data
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDdeFreeDataHandle(hd: u32) -> u32 {
    if !is_data(hd) {
        set_error(DMLERR_INVALIDPARAMETER, 0);
        return 0;
    }
    free_data(hd);
    set_error(0, 0);
    1
}

/// WinDdeQueryModuleVersion(pszModule): 0x00010101 (IBM's answer for
/// the module itself, a null name).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeQueryModuleVersion(module: u32) -> u32 {
    if module == 0 { 0x0001_0101 } else { 0 }
}
