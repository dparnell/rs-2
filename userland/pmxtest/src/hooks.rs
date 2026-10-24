//! Hooks: queue hooks in this program for every kind PMWIN calls, and
//! system hooks in PMXHOOK.DLL that PMWIN loads into a second process
//! (`PMXTEST CHILD`) and into the PM server (journal playback).

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::*;

use crate::pm::*;
use crate::{check, pump, pump_for};

const HK_SENDMSG: u32 = 0;
const HK_INPUT: u32 = 1;
const HK_MSGFILTER: u32 = 2;
const HK_JOURNALRECORD: u32 = 3;
const HK_JOURNALPLAYBACK: u32 = 4;
const HK_LOADER: u32 = 6;
const HK_REGISTERUSERMSG: u32 = 7;
const HK_FINDWORD: u32 = 11;
const HK_CODEPAGECHANGED: u32 = 12;
const HK_WINDOWDC: u32 = 15;
const HK_DESTROYWINDOW: u32 = 16;
const HK_CHECKMSGFILTER: u32 = 20;
const HK_MSGINPUT: u32 = 21;
const HMQ_CURRENT: u32 = 1;

const MSGF_MESSAGEBOX: u32 = 2;

/// What the hooks and the client saw.
struct Seen {
    input: Vec<(u32, u32)>,
    sent: Vec<(u32, u32, u32)>,
    filters: Vec<u32>,
    destroyed: Vec<u32>,
    dcs: Vec<(u32, u32)>,
    codepages: Vec<(u32, u32, u32)>,
    user5_mp1: u32,
    chars: Vec<u8>,
    msginput_left: u32,
}

static mut SEEN: Seen = Seen {
    input: Vec::new(),
    sent: Vec::new(),
    filters: Vec::new(),
    destroyed: Vec::new(),
    dcs: Vec::new(),
    codepages: Vec::new(),
    user5_mp1: 0,
    chars: Vec::new(),
    msginput_left: 0,
};

fn seen() -> &'static mut Seen {
    unsafe { &mut *core::ptr::addr_of_mut!(SEEN) }
}

/// The client's part (None: default processing).
pub fn client_msg(_h: u32, msg: u32, mp1: u32, mp2: u32) -> Option<u32> {
    match msg {
        m if m == WM_USER + 5 => {
            seen().user5_mp1 = mp1;
            Some(0)
        }
        WM_CHAR => {
            // KC_CHAR, key down.
            if mp1 & 1 != 0 && mp1 & 0x40 == 0 {
                seen().chars.push(mp2 as u8);
            }
            Some(1)
        }
        _ => None,
    }
}

extern "C" fn input_hook(_hab: u32, pq: *mut QMsg, fs: u32) -> u32 {
    let m = unsafe { *pq };
    if m.msg >= WM_USER && m.msg < WM_USER + 100 {
        seen().input.push((m.msg, fs));
    }
    // It takes WM_USER + 2.
    (m.msg == WM_USER + 2) as u32
}

extern "C" fn send_hook(_hab: u32, psmh: *mut u32, inter: u32) -> u32 {
    // SMHSTRUCT: mp2, mp1, msg, hwnd, model.
    unsafe {
        let msg = *psmh.add(2);
        if msg >= WM_USER && msg < WM_USER + 100 {
            seen().sent.push((*psmh.add(3), msg, inter | *psmh.add(4) << 8));
        }
        if msg == WM_USER + 5 {
            *psmh.add(1) = 7;
        }
    }
    0
}

extern "C" fn filter_hook(_hab: u32, pq: *mut QMsg, msgf: u32) -> u32 {
    seen().filters.push(msgf);
    if msgf == MSGF_MESSAGEBOX && unsafe { (*pq).msg } == WM_TIMER {
        // Ends the message box that runs the loop.
        unsafe { WinDismissDlg(WinQueryActiveWindow(HWND_DESKTOP), 1) };
        return 1;
    }
    (msgf == 77) as u32
}

extern "C" fn destroy_hook(_hab: u32, h: u32, _r: u32) -> u32 {
    seen().destroyed.push(h);
    0
}

extern "C" fn dc_hook(_hab: u32, _hdc: u32, h: u32, assoc: u32) -> u32 {
    seen().dcs.push((h, assoc));
    0
}

extern "C" fn cp_hook(hmq: u32, old: u32, new: u32) -> u32 {
    seen().codepages.push((hmq, old, new));
    0
}

extern "C" fn findword_hook(_cp: u32, _psz: *const u8, _cb: u32, _ich: u32, ps: *mut u32, pe: *mut u32, pn: *mut u32) -> u32 {
    unsafe {
        *ps = 3;
        *pe = 8;
        *pn = 9;
    }
    1
}

extern "C" fn loader_hook(_hab: u32, ctx: u32, _lib: *const u8, phlib: *mut u32, _proc: *const u8, _wp: u32, ok: *mut u32) -> u32 {
    // LHK_LOADLIB: a library of the hook's own.
    if ctx == 4 {
        unsafe {
            *phlib = 0x1234;
            *ok = 1;
        }
        return 1;
    }
    0
}

extern "C" fn reguser_hook(_hab: u32, ctx: u32, msg: u32, _t1: u32, _d1: u32, _t2: u32, _d2: u32, _tr: u32, _c: u32, _ar: u32, pf: *mut u32) -> u32 {
    if ctx == 2 && msg == WM_USER + 40 {
        unsafe { *pf = 0 };
        return 1;
    }
    0
}

extern "C" fn check_hook(_hab: u32, pq: *mut QMsg, _first: u32, _last: u32, _fl: u32) -> u32 {
    // Skips WM_USER + 10.
    (unsafe { (*pq).msg } == WM_USER + 10) as u32
}

extern "C" fn msginput_hook(_hab: u32, pq: *mut QMsg, skip: u32, _norec: *mut u32) -> u32 {
    let s = seen();
    if s.msginput_left == 0 {
        return 0;
    }
    if skip != 0 {
        s.msginput_left -= 1;
        return 1;
    }
    unsafe { *pq = QMsg { msg: WM_USER + 20, mp1: 99, ..Default::default() } };
    1
}

fn set(hab: u32, kind: u32, pfn: usize) -> bool {
    unsafe { WinSetHook(hab, HMQ_CURRENT, kind, pfn as u32, 0) != 0 }
}

fn release(hab: u32, kind: u32, pfn: usize) -> bool {
    unsafe { WinReleaseHook(hab, HMQ_CURRENT, kind, pfn as u32, 0) != 0 }
}

/// PMXHOOK.DLL's counts block (see userland/pmxhook).
#[repr(C)]
struct Counts {
    procs: [[u32; 3]; 16],
    records: u32,
    last_record: u32,
    played: u32,
    playback_calls: u32,
}

fn my_pid() -> u32 {
    let (mut t, mut p) = (null_mut(), null_mut());
    unsafe {
        DosGetInfoBlocks(&mut t, &mut p);
        (*p).pid
    }
}

pub fn hook_checks(hab: u32, hmq: u32, client: u32) {
    let s = seen();
    // HK_INPUT: every message retrieved, removed or not; one it takes is
    // gone.
    check("WinSetHook HK_INPUT", set(hab, HK_INPUT, input_hook as *const () as usize));
    pump(hab);
    s.input.clear();
    let mut m = QMsg::default();
    unsafe {
        WinPostMsg(client, WM_USER + 1, 0, 0);
        WinPeekMsg(hab, &mut m, 0, 0, 0, 0);
        WinGetMsg(hab, &mut m, 0, 0, 0);
    }
    check("input hook sees a peeked (PM_NOREMOVE) and a removed message", s.input == [(WM_USER + 1, 0), (WM_USER + 1, 1)] && m.msg == WM_USER + 1);
    unsafe {
        WinPostMsg(client, WM_USER + 2, 0, 0);
        WinPostMsg(client, WM_USER + 3, 0, 0);
        WinGetMsg(hab, &mut m, 0, 0, 0);
    }
    check("a message the input hook takes is not returned", m.msg == WM_USER + 3);
    check("WinReleaseHook HK_INPUT", release(hab, HK_INPUT, input_hook as *const () as usize));
    s.input.clear();
    unsafe {
        WinPostMsg(client, WM_USER + 2, 0, 0);
        WinGetMsg(hab, &mut m, 0, 0, 0);
    }
    check("released: the hook sees nothing", s.input.is_empty() && m.msg == WM_USER + 2);

    // HK_SENDMSG: in the sender, before the message arrives; it may
    // change it.
    check("WinSetHook HK_SENDMSG", set(hab, HK_SENDMSG, send_hook as *const () as usize));
    s.sent.clear();
    unsafe { WinSendMsg(client, WM_USER + 5, 1, 2) };
    // fInterTask 0, PM_MODEL_2X.
    check("send hook sees the message (same thread, PM_MODEL_2X)", s.sent == [(client, WM_USER + 5, 1 << 8)]);
    check("send hook changes the message", s.user5_mp1 == 7);
    release(hab, HK_SENDMSG, send_hook as *const () as usize);

    // HK_MSGFILTER: WinCallMsgFilter, and the message box's loop.
    check("WinSetHook HK_MSGFILTER", set(hab, HK_MSGFILTER, filter_hook as *const () as usize));
    s.filters.clear();
    let r1 = unsafe { WinCallMsgFilter(hab, &mut m, 77) };
    let r2 = unsafe { WinCallMsgFilter(hab, &mut m, 5) };
    check("WinCallMsgFilter returns what the hook does", r1 == 1 && r2 == 0 && s.filters == [77, 5]);
    unsafe { WinStartTimer(hab, client, 7, 100) };
    let r = unsafe { WinMessageBox(HWND_DESKTOP, client, b"Ends by itself\0".as_ptr(), b"PMXTEST\0".as_ptr(), 0, 0) };
    unsafe { WinStopTimer(hab, client, 7) };
    check("a message box's loop calls the filter hook (MSGF_MESSAGEBOX)", r == 1 && s.filters.contains(&MSGF_MESSAGEBOX));
    release(hab, HK_MSGFILTER, filter_hook as *const () as usize);
    pump(hab);

    // HK_DESTROYWINDOW, HK_WINDOWDC, HK_CODEPAGECHANGED.
    check("WinSetHook HK_DESTROYWINDOW", set(hab, HK_DESTROYWINDOW, destroy_hook as *const () as usize));
    let w = unsafe { WinCreateWindow(client, b"PmxTest\0".as_ptr(), null(), 0, 0, 0, 10, 10, client, 3, 50, null(), null()) };
    unsafe { WinDestroyWindow(w) };
    check("destroy window hook", w != 0 && s.destroyed == [w]);
    release(hab, HK_DESTROYWINDOW, destroy_hook as *const () as usize);
    check("WinSetHook HK_WINDOWDC", set(hab, HK_WINDOWDC, dc_hook as *const () as usize));
    let hps = unsafe { WinGetPS(client) };
    unsafe { WinReleasePS(hps) };
    check("window DC hook: WinGetPS and WinReleasePS", s.dcs == [(client, 1), (client, 0)]);
    release(hab, HK_WINDOWDC, dc_hook as *const () as usize);
    check("WinSetHook HK_CODEPAGECHANGED", set(hab, HK_CODEPAGECHANGED, cp_hook as *const () as usize));
    let ok = unsafe { WinSetCp(hmq, 437) } != 0 && unsafe { WinQueryCp(hmq) } == 437;
    unsafe { WinSetCp(hmq, 850) };
    check("WinSetCp: code page changed hook", ok && s.codepages == [(hmq, 850, 437), (hmq, 437, 850)]);
    release(hab, HK_CODEPAGECHANGED, cp_hook as *const () as usize);

    // HK_FINDWORD: where WinDrawText breaks a line.
    let hps = unsafe { WinGetPS(client) };
    let text = b"abcd efgh ijkl";
    let draw = || {
        let mut r = RectL { left: 0, bottom: 0, right: 45, top: 40 };
        // DT_QUERYEXTENT | DT_WORDBREAK.
        unsafe { WinDrawText(hps, text.len() as i32, text.as_ptr(), &mut r, 0, 0, 0x4002) }
    };
    let plain = draw();
    check("WinSetHook HK_FINDWORD", set(hab, HK_FINDWORD, findword_hook as *const () as usize));
    let hooked = draw();
    release(hab, HK_FINDWORD, findword_hook as *const () as usize);
    unsafe { WinReleasePS(hps) };
    check("find word hook decides WinDrawText's line break", plain == 5 && hooked == 3);

    // HK_LOADER, HK_REGISTERUSERMSG.
    let lib = unsafe { WinLoadLibrary(hab, b"PMXHOOK\0".as_ptr()) };
    let proc_ = unsafe { WinLoadProcedure(hab, lib, b"PmxCounts\0".as_ptr()) };
    check("WinLoadLibrary, WinLoadProcedure, WinDeleteLibrary", lib != 0 && proc_ != 0 && unsafe { WinDeleteLibrary(hab, lib) } != 0);
    check("WinSetHook HK_LOADER", set(hab, HK_LOADER, loader_hook as *const () as usize));
    let lib = unsafe { WinLoadLibrary(hab, b"PMXHOOK\0".as_ptr()) };
    check("loader hook answers WinLoadLibrary", lib == 0x1234);
    release(hab, HK_LOADER, loader_hook as *const () as usize);
    let plain = unsafe { WinRegisterUserMsg(hab, WM_USER + 40, 1, 1, 1, 1, 1) };
    check("WinSetHook HK_REGISTERUSERMSG", set(hab, HK_REGISTERUSERMSG, reguser_hook as *const () as usize));
    let hooked = unsafe { WinRegisterUserMsg(hab, WM_USER + 40, 1, 1, 1, 1, 1) };
    release(hab, HK_REGISTERUSERMSG, reguser_hook as *const () as usize);
    check("register user message hook answers WinRegisterUserMsg", plain == 1 && hooked == 0);

    // HK_CHECKMSGFILTER: which messages a filter takes.
    pump(hab);
    check("WinSetHook HK_CHECKMSGFILTER", set(hab, HK_CHECKMSGFILTER, check_hook as *const () as usize));
    unsafe {
        WinPostMsg(client, WM_USER + 10, 0, 0);
        WinPostMsg(client, WM_USER + 11, 0, 0);
    }
    let got = unsafe { WinPeekMsg(hab, &mut m, 0, 0, 0, 1) } != 0;
    release(hab, HK_CHECKMSGFILTER, check_hook as *const () as usize);
    let first = m.msg;
    let then = unsafe { WinPeekMsg(hab, &mut m, 0, 0, 0, 1) } != 0 && m.msg == WM_USER + 10;
    check("check message filter hook skips a message", got && first == WM_USER + 11 && then);

    // HK_MSGINPUT: a source of input before the queue.
    s.msginput_left = 1;
    check("WinSetHook HK_MSGINPUT", set(hab, HK_MSGINPUT, msginput_hook as *const () as usize));
    let got = unsafe { WinPeekMsg(hab, &mut m, 0, 0, 0, 1) } != 0 && m.msg == WM_USER + 20 && m.mp1 == 99;
    let after = unsafe { WinPeekMsg(hab, &mut m, 0, 0, 0, 1) };
    release(hab, HK_MSGINPUT, msginput_hook as *const () as usize);
    check("message input hook supplies a message, then none", got && after == 0 && s.msginput_left == 0);

    system_checks(hab, client);
}

/// The system hooks of PMXHOOK.DLL.
fn system_checks(hab: u32, client: u32) {
    let mut hmod = 0;
    if unsafe { DosLoadModule(null_mut(), 0, b"PMXHOOK\0".as_ptr(), &mut hmod) } != 0 {
        check("PMXHOOK.DLL loads", false);
        return;
    }
    let proc_ = |name: &[u8]| {
        let mut p = 0usize;
        unsafe { DosQueryProcAddr(hmod, 0, name.as_ptr(), &mut p) };
        p as u32
    };
    let (input, sendmsg, rec, play) = (proc_(b"PmxInputHook\0"), proc_(b"PmxSendMsgHook\0"), proc_(b"PmxRecordHook\0"), proc_(b"PmxPlaybackHook\0"));
    let counts = proc_(b"PmxCounts\0");
    let counts: extern "C" fn() -> *mut Counts = unsafe { core::mem::transmute(counts as usize) };
    let Some(c) = (unsafe { counts().as_mut() }) else {
        check("PMXHOOK.DLL's counts", false);
        return;
    };
    check("a system hook needs its module", unsafe { WinSetHook(hab, 0, HK_INPUT, input, 0) } == 0);
    let ok = unsafe { WinSetHook(hab, 0, HK_INPUT, input, hmod) != 0 && WinSetHook(hab, 0, HK_SENDMSG, sendmsg, hmod) != 0 };
    check("WinSetHook system HK_INPUT and HK_SENDMSG", ok);
    // Another PM process: PMWIN loads PMXHOOK.DLL there for the hooks.
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, b"PMXTEST\0CHILD\0\0".as_ptr(), null(), &mut res, b"C:\\RS2\\PMXTEST.EXE\0".as_ptr()) };
    let me = my_pid();
    let other = c.procs.iter().find(|e| e[0] != 0 && e[0] != me).copied();
    check("system hooks run in another process", rc == 0 && other.is_some_and(|e| e[1] >= 5 && e[2] >= 3));
    if let Some(e) = other {
        println!("  child: {} input, {} sent", e[1], e[2]);
    }
    unsafe {
        WinReleaseHook(hab, 0, HK_INPUT, input, hmod);
        WinReleaseHook(hab, 0, HK_SENDMSG, sendmsg, hmod);
    }

    // Journal playback (the PM server plays the keys to the focus window)
    // and record (which does not see played-back input).
    unsafe {
        WinSetActiveWindow(HWND_DESKTOP, client);
        WinSetFocus(HWND_DESKTOP, client);
    }
    pump(hab);
    seen().chars.clear();
    c.records = 0;
    c.played = 0;
    let ok = unsafe { WinSetHook(hab, 0, HK_JOURNALRECORD, rec, hmod) != 0 && WinSetHook(hab, 0, HK_JOURNALPLAYBACK, play, hmod) != 0 };
    check("WinSetHook system HK_JOURNALRECORD and HK_JOURNALPLAYBACK", ok);
    for _ in 0..300 {
        pump_for(hab, 10);
        if seen().chars.len() >= 3 {
            break;
        }
    }
    unsafe { WinReleaseHook(hab, 0, HK_JOURNALPLAYBACK, play, hmod) };
    pump_for(hab, 100);
    check("journal playback: the keys reach the focus window", seen().chars == b"xyz" && c.played == 3);
    check("played-back input is not recorded", c.records == 0);
    unsafe { WinReleaseHook(hab, 0, HK_JOURNALRECORD, rec, hmod) };
    println!("  played {:?}, {} playback calls", core::str::from_utf8(&seen().chars), c.playback_calls);
}

/// `PMXTEST CHILD`: posts itself five messages and sends three (the
/// parent's system hooks count them).
pub fn child(hab: u32, client: u32) -> u32 {
    for i in 0..5 {
        unsafe { WinPostMsg(client, WM_USER + 30, i, 0) };
    }
    pump(hab);
    for _ in 0..3 {
        unsafe { WinSendMsg(client, WM_USER + 31, 0, 0) };
    }
    0
}

/// `PMXTEST RECORD`: a system journal record hook while keys are typed
/// into the window (by `xtask run --type`), for up to 20 seconds.
pub fn record(hab: u32, _client: u32) {
    let mut hmod = 0;
    unsafe { DosLoadModule(null_mut(), 0, b"PMXHOOK\0".as_ptr(), &mut hmod) };
    let mut p = 0usize;
    let mut cp = 0usize;
    unsafe {
        DosQueryProcAddr(hmod, 0, b"PmxRecordHook\0".as_ptr(), &mut p);
        DosQueryProcAddr(hmod, 0, b"PmxCounts\0".as_ptr(), &mut cp);
    }
    let counts: extern "C" fn() -> *mut Counts = unsafe { core::mem::transmute(cp) };
    let Some(c) = (unsafe { counts().as_mut() }) else { return };
    c.records = 0;
    check("WinSetHook system HK_JOURNALRECORD", unsafe { WinSetHook(hab, 0, HK_JOURNALRECORD, p as u32, hmod) } != 0);
    println!("type now");
    for _ in 0..200 {
        pump_for(hab, 100);
        if c.records >= 3 {
            break;
        }
    }
    unsafe { WinReleaseHook(hab, 0, HK_JOURNALRECORD, p as u32, hmod) };
    println!("  {} records, last message {:#x}", c.records, c.last_record);
    check("journal record hook sees typed input", c.records >= 3);
}
