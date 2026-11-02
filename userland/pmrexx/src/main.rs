//! PMREXX: runs a REXX procedure in PMREXXIO's Presentation Manager
//! window, as IBM's.
//!
//! `PMREXX [/T] [/Q] procedure [arguments]`: /T starts the procedure in
//! interactive trace, /Q closes the window when it ends (else a message
//! box says it has ended and the window stays, for Restart, Save and the
//! rest). The procedure (`.CMD` added when there is no such file; found
//! on the PATH when the name has no drive or directory) must start with
//! a `/*` comment. It runs through RexxStart on a thread of its own with
//! PMREXXIO's I/O exit and an initialisation exit (for /T); its return
//! code is PMREXX's.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::SeqCst};

os2::entry!(main);

type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

#[repr(C)]
#[derive(Default)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

#[repr(C)]
struct RxString {
    len: u32,
    ptr: *mut u8,
}

/// RXSYSEXIT
#[repr(C)]
struct SysExit {
    name: *const u8,
    code: i32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.857"]
    fn WinSetCp(hmq: u32, cp: u32) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.929"]
    fn WinSubclassWindow(h: u32, p: WndProc) -> usize;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.877"]
    fn WinSetWindowText(h: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.860"]
    fn WinSetFocus(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.701"]
    fn WinAlarm(desk: u32, kind: u32) -> u32;
    #[link_name = "PMWIN.789"]
    fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.713"]
    fn WinCreateAccelTable(hab: u32, tbl: *const u8) -> u32;
    #[link_name = "PMWIN.850"]
    fn WinSetAccelTable(hab: u32, haccel: u32, frame: u32) -> u32;
    #[link_name = "PMWIN.892"]
    fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMWIN.838"]
    fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.736"]
    fn WinEnableWindowUpdate(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.765"]
    fn WinInvalidateRect(h: u32, r: *const u8, children: u32) -> u32;
    #[link_name = "PMSHAPI.125"]
    fn WinQuerySwitchHandle(h: u32, pid: u32) -> u32;
    #[link_name = "PMSHAPI.124"]
    fn WinQuerySwitchEntry(hsw: u32, swc: *mut u8) -> u32;
    #[link_name = "PMSHAPI.123"]
    fn WinChangeSwitchEntry(hsw: u32, swc: *const u8) -> u32;
    #[link_name = "PMCTLS.4"]
    fn WinFileDlg(parent: u32, owner: u32, fild: *mut u8) -> u32;
    #[link_name = "DOSCALLS.228"]
    fn DosSearchPath(flags: u32, path: *const u8, name: *const u8, buf: *mut u8, cb: u32) -> u32;

    #[link_name = "PMREXXIO.2"]
    fn RexxCreateIOWindow(owner: u32, client: *mut u32) -> u32;
    #[link_name = "PMREXXIO.1"]
    fn RexxDestroyIOWindow(frame: u32);
    #[link_name = "REXX.1"]
    fn RexxStart(argc: i32, argv: *const RxString, name: *const u8, instore: *const RxString, env: *const u8, kind: i32, exits: *const SysExit, rc: *mut i16, result: *mut RxString) -> i32;
    #[link_name = "REXX.4"]
    fn RexxSetTrace(pid: u32, tid: u32) -> u32;
    #[link_name = "REXXAPI.12"]
    fn RexxRegisterExitExe(name: *const u8, entry: usize, user: *const u8) -> u32;
    #[link_name = "REXXAPI.13"]
    fn RexxDeregisterExit(name: *const u8, dll: *const u8) -> u32;
}

const HWND_DESKTOP: u32 = 1;
const FID_MENU: u32 = 0x8005;
const FID_TITLEBAR: u32 = 0x8003;
const WM_COMMAND: u32 = 0x20;
const WM_CLOSE: u32 = 0x29;
const WM_QUIT: u32 = 0x2a;
const WM_SAVEAPPLICATION: u32 = 0x3e;
const WM_CHAR: u32 = 0x7a;
const MM_SETITEMATTR: u32 = 0x192;
const MIA_CHECKED: u32 = 0x2000;
const MIA_DISABLED: u32 = 0x4000;
const KC_VIRTUALKEY: u32 = 2;
const KC_KEYUP: u32 = 0x40;
const VK_BREAK: u32 = 4;
const MB_OK: u32 = 0;
const MB_YESNO: u32 = 4;
const MB_QUERY: u32 = 0x10;
const MB_INFORMATION: u32 = 0x30;
const MB_CUAWARNING: u32 = 0x20;
const MB_ERROR: u32 = 0x40;
const MB_MOVEABLE: u32 = 0x4000;
const MBID_YES: u32 = 6;
const MLM_SETCHANGED: u32 = 0x1bb;
const MLM_QUERYTEXTLENGTH: u32 = 0x1c0;
const MLM_SETIMPORTEXPORT: u32 = 0x1c2;
const MLM_EXPORT: u32 = 0x1c4;
const MLM_DELETE: u32 = 0x1c6;
const MLM_QUERYFORMATTEXTLENGTH: u32 = 0x1c8;
const MLM_RESETUNDO: u32 = 0x1cf;
const RXSIO: i32 = 5;
const RXINI: i32 = 9;

// The I/O window's menu items PMREXX handles, and its messages.
const RXIOA_HALT: u32 = 0x110;
const RXIOA_SAVE: u32 = 0x202;
const RXIOA_SAVEAS: u32 = 0x203;
const RXIOA_EXIT: u32 = 0x204;
const RXIOA_ABOUT: u32 = 0x205;
const RXIOA_RESTART: u32 = 0x206;
const MLE_WNDW: u32 = 0x127;
const REXX_STARTPROC: u32 = 0x1001;
const REXX_ENDPROC: u32 = 0x1002;
const PMRXA_FLAGCHANGE: u32 = 0x1005;
const PMRXA_RESULTCHANGE: u32 = 0x1006;
/// PMREXX's own messages: the procedure was not found, it has ended.
const PMRX_NOTFOUND: u32 = 0x30c;
const PMRX_ENDED: u32 = 0x30d;

const TITLE: &[u8] = b"PMREXX\0";
const ERROR_TITLE: &[u8] = b"PMREXX Error\0";

static HAB: AtomicU32 = AtomicU32::new(0);
static FRAME: AtomicU32 = AtomicU32::new(0);
static CLIENT: AtomicU32 = AtomicU32::new(0);
static SUPER: AtomicUsize = AtomicUsize::new(0);
static PID: AtomicU32 = AtomicU32::new(0);
static REXX_TID: AtomicU32 = AtomicU32::new(0);
static TRACE: AtomicBool = AtomicBool::new(false);
static QUIT: AtomicBool = AtomicBool::new(false);
static RESULTS: AtomicBool = AtomicBool::new(true);
static PROC_RC: AtomicU32 = AtomicU32::new(0);

/// Data the REXX thread and the window share: the procedure, its
/// argument and the file Save writes.
struct Shared {
    procname: Vec<u8>,
    arg: Option<Vec<u8>>,
    file: Vec<u8>,
}

static mut SHARED: Shared = Shared { procname: Vec::new(), arg: None, file: Vec::new() };

fn shared() -> &'static mut Shared {
    unsafe { &mut *core::ptr::addr_of_mut!(SHARED) }
}

fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

fn post_hard(h: u32, m: u32, a: u32, b: u32) {
    while unsafe { WinPostMsg(h, m, a, b) } == 0 {
        unsafe { os2::DosSleep(0) };
    }
}

fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}

fn message(owner: u32, text: &[u8], title: &[u8], style: u32) -> u32 {
    let (t, c) = (z(text), z(title));
    unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), 1, style) }
}

/// Enable or disable a menu item of the I/O window.
fn set_options(id: u32, on: bool) {
    let menu = unsafe { WinWindowFromID(FRAME.load(SeqCst), FID_MENU) };
    post_hard(menu, MM_SETITEMATTR, mp(id, 1), mp(MIA_DISABLED, if on { 0 } else { MIA_DISABLED }));
}

fn set_check(id: u32, on: bool) {
    let menu = unsafe { WinWindowFromID(FRAME.load(SeqCst), FID_MENU) };
    post_hard(menu, MM_SETITEMATTR, mp(id, 1), mp(MIA_CHECKED, if on { MIA_CHECKED } else { 0 }));
}

/// The command line after the program name, split at blanks (quotes
/// group), each word with its end in the raw text.
fn words(raw: &[u8]) -> Vec<(Vec<u8>, usize)> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        while i < raw.len() && (raw[i] == b' ' || raw[i] == b'\t') {
            i += 1;
        }
        if i >= raw.len() {
            break;
        }
        let mut w = Vec::new();
        let mut q = false;
        while i < raw.len() && (q || (raw[i] != b' ' && raw[i] != b'\t')) {
            if raw[i] == b'"' {
                q = !q;
            } else {
                w.push(raw[i]);
            }
            i += 1;
        }
        v.push((w, i));
    }
    v
}

/// The procedure's file: the name as given, else on the PATH (when it
/// has no drive or directory), else with `.CMD` added; it must start
/// with `/*`. The name to give RexxStart.
fn check_rexx_file(name: &[u8]) -> Option<Vec<u8>> {
    let mut n = name.to_vec();
    for _ in 0..2 {
        let zn = z(&n);
        let mut path = zn.clone();
        let mut found = exists(&zn);
        if !found && !n.contains(&b'\\') && !n.contains(&b':') {
            let mut buf = [0u8; 260];
            if unsafe { DosSearchPath(7, b"PATH\0".as_ptr(), zn.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
                let l = buf.iter().position(|&c| c == 0).unwrap_or(0);
                path = z(&buf[..l]);
                found = exists(&path);
            }
        }
        if found {
            return starts_with_comment(&path).then_some(n);
        }
        n.extend_from_slice(b".CMD");
    }
    None
}

fn exists(path: &[u8]) -> bool {
    let mut h = 0;
    let mut action = 0;
    let rc = unsafe { os2::DosOpen(path.as_ptr(), &mut h, &mut action, 0, 0, 1, 0x40, core::ptr::null_mut()) };
    if rc == 0 {
        unsafe { os2::DosClose(h) };
    }
    rc == 0
}

fn starts_with_comment(path: &[u8]) -> bool {
    let mut h = 0;
    let mut action = 0;
    if unsafe { os2::DosOpen(path.as_ptr(), &mut h, &mut action, 0, 0, 1, 0x40, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut b = [0u8; 2];
    let mut n = 0;
    unsafe {
        os2::DosRead(h, b.as_mut_ptr(), 2, &mut n);
        os2::DosClose(h);
    }
    n == 2 && b == *b"/*"
}

fn main(_s: &os2::Startup) -> u32 {
    let mut tib: *mut os2::Tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    PID.store(unsafe { (*pib).pid }, SeqCst);
    // The raw arguments: after the program name's zero.
    let raw = unsafe {
        let mut p = (*pib).pchcmd;
        while *p != 0 {
            p = p.add(1);
        }
        p = p.add(1);
        let start = p;
        while *p != 0 {
            p = p.add(1);
        }
        core::slice::from_raw_parts(start, p.offset_from(start) as usize)
    };
    let hab = unsafe { WinInitialize(0) };
    HAB.store(hab, SeqCst);
    let hmq = unsafe { WinCreateMsgQueue(hab, 50) };
    let mut cp = 0u32;
    let mut n = 0;
    unsafe {
        os2::DosQueryCp(4, &mut cp, &mut n);
        WinSetCp(hmq, cp);
    }
    let mut client = 0;
    let frame = unsafe { RexxCreateIOWindow(HWND_DESKTOP, &mut client) };
    FRAME.store(frame, SeqCst);
    CLIENT.store(client, SeqCst);
    SUPER.store(unsafe { WinSubclassWindow(client, sub_proc) }, SeqCst);
    // Accelerators: F3 Exit, Ctrl+S Save, Delete.
    let mut accel = Vec::new();
    for v in [4u16, 437, 2, 0x22, 0x204, 0x11, b'S' as u16, 0x202, 0x11, b's' as u16, 0x202, 2, 0x1b, 0x109] {
        accel.extend_from_slice(&v.to_le_bytes());
    }
    unsafe { WinSetAccelTable(hab, WinCreateAccelTable(hab, accel.as_ptr()), frame) };

    let w = words(raw);
    let mut i = 0;
    while i < w.len() && i < 2 {
        let o = &w[i].0;
        if o.eq_ignore_ascii_case(b"/T") {
            TRACE.store(true, SeqCst);
        } else if o.eq_ignore_ascii_case(b"/Q") {
            QUIT.store(true, SeqCst);
        } else {
            break;
        }
        i += 1;
    }
    let mut title = TITLE[..TITLE.len() - 1].to_vec();
    if i < w.len() {
        let name = &w[i].0;
        title.extend_from_slice(b": ");
        title.extend_from_slice(name);
        let sh = shared();
        match check_rexx_file(name) {
            Some(p) => sh.procname = z(&p),
            None => {
                message(0, b"The procedure file was not found.", ERROR_TITLE, MB_OK | MB_ERROR | MB_MOVEABLE);
                unsafe {
                    WinSetFocus(HWND_DESKTOP, frame);
                    WinPostMsg(frame, WM_QUIT, 0, 0);
                }
                PROC_RC.store(-3i32 as u32, SeqCst);
            }
        }
        // The argument string: the rest of the command line after the
        // procedure's name and one blank.
        let end = w[i].1;
        sh.arg = if i + 1 < w.len() { Some(raw[(end + 1).min(raw.len())..].to_vec()) } else { None };
        let t = z(&title);
        unsafe { WinSetWindowText(WinWindowFromID(frame, FID_TITLEBAR), t.as_ptr()) };
        if !sh.procname.is_empty() {
            unsafe { RexxRegisterExitExe(b"PMREXXINIT\0".as_ptr(), init_exit as *const () as usize, core::ptr::null()) };
            start_rexx();
        }
    } else {
        unsafe { WinAlarm(HWND_DESKTOP, 2) };
        message(0, b"Name the REXX procedure to run: PMREXX [/T] [/Q] procedure [arguments]", ERROR_TITLE, MB_OK | MB_ERROR | MB_MOVEABLE);
        unsafe {
            WinSetFocus(HWND_DESKTOP, frame);
            WinPostMsg(frame, WM_QUIT, 0, 0);
        }
        PROC_RC.store(2, SeqCst);
    }
    unsafe { WinUpdateWindow(frame) };
    // The window list shows the title too.
    let (mut pid, mut tid) = (0, 0);
    unsafe { WinQueryWindowProcess(frame, &mut pid, &mut tid) };
    let hsw = unsafe { WinQuerySwitchHandle(frame, pid) };
    let mut swc = [0u8; 96];
    if hsw != 0 && unsafe { WinQuerySwitchEntry(hsw, swc.as_mut_ptr()) } == 0 {
        let n = title.len().min(63);
        swc[28..28 + n].copy_from_slice(&title[..n]);
        swc[28 + n] = 0;
        unsafe { WinChangeSwitchEntry(hsw, swc.as_ptr()) };
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    let old: WndProc = unsafe { core::mem::transmute(SUPER.load(SeqCst)) };
    unsafe {
        WinSubclassWindow(client, old);
        RexxDestroyIOWindow(frame);
        RexxDeregisterExit(b"PMREXXINIT\0".as_ptr(), core::ptr::null());
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
        os2::DosExit(1, PROC_RC.load(SeqCst));
    }
    0
}

fn start_rexx() {
    let mut tid = 0;
    unsafe { os2::DosCreateThread(&mut tid, rexx_thread, 0, 0, 0x10000) };
    REXX_TID.store(tid, SeqCst);
}

/// RXINI: /T starts the procedure in interactive trace.
extern "C" fn init_exit(_exitno: i32, _sub: i32, _parm: *mut u8) -> i32 {
    if TRACE.load(SeqCst) {
        unsafe { RexxSetTrace(PID.load(SeqCst), REXX_TID.load(SeqCst)) };
    }
    0
}

/// The procedure, run on its own thread.
unsafe extern "C" fn rexx_thread(_p: u32) {
    let client = CLIENT.load(SeqCst);
    set_options(RXIOA_RESTART, false);
    set_options(RXIOA_HALT, true);
    post_hard(client, REXX_STARTPROC, REXX_TID.load(SeqCst), 0);
    unsafe { os2::DosSleep(0) };
    let sh = shared();
    let arg = sh.arg.as_ref().map(|a| RxString { len: a.len() as u32, ptr: a.as_ptr() as *mut u8 });
    let (argc, argv) = match &arg {
        Some(a) => (1, a as *const RxString),
        None => (0, core::ptr::null()),
    };
    let exits = [
        SysExit { name: b"PMREXXIO\0".as_ptr(), code: RXSIO },
        SysExit { name: b"PMREXXINIT\0".as_ptr(), code: RXINI },
        SysExit { name: core::ptr::null(), code: 0 },
    ];
    let mut rc = 0i16;
    let mut result = RxString { len: 0, ptr: core::ptr::null_mut() };
    let r = unsafe { RexxStart(argc, argv, sh.procname.as_ptr(), core::ptr::null(), core::ptr::null(), 0, exits.as_ptr(), &mut rc, &mut result) };
    if r == -3 {
        post_hard(client, PMRX_NOTFOUND, 0, 0);
        PROC_RC.store(-3i32 as u32, SeqCst);
    } else {
        unsafe { os2::DosSleep(100) };
        PROC_RC.store(rc as i32 as u32, SeqCst);
    }
    if !result.ptr.is_null() {
        unsafe { os2::DosFreeMem(result.ptr) };
    }
    post_hard(client, REXX_ENDPROC, 0, 0);
    set_options(RXIOA_RESTART, true);
    set_check(RXIOA_HALT, false);
    set_options(RXIOA_HALT, false);
    post_hard(client, PMRX_ENDED, 0, 0);
}

fn call_super(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let f: WndProc = unsafe { core::mem::transmute(SUPER.load(SeqCst)) };
    f(h, m, mp1, mp2)
}

/// The I/O window's client, subclassed: PMREXX's menu items, the end of
/// the procedure, closing.
extern "C" fn sub_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_COMMAND => return command(h, mp1, mp2),
        WM_CHAR if mp1 & KC_KEYUP == 0 && mp1 & KC_VIRTUALKEY != 0 && mp2 >> 16 == VK_BREAK => {
            unsafe { WinSendMsg(h, WM_COMMAND, RXIOA_HALT, 0) };
        }
        PMRX_NOTFOUND => {
            unsafe { WinAlarm(HWND_DESKTOP, 2) };
            message(CLIENT.load(SeqCst), b"The procedure file was not found.", ERROR_TITLE, MB_OK | MB_ERROR | MB_MOVEABLE);
            unsafe { WinPostMsg(CLIENT.load(SeqCst), WM_CLOSE, 0, 0) };
            return 0;
        }
        PMRX_ENDED => {
            if QUIT.load(SeqCst) {
                unsafe {
                    WinSetFocus(HWND_DESKTOP, FRAME.load(SeqCst));
                    WinPostMsg(FRAME.load(SeqCst), WM_QUIT, 0, 0);
                }
            } else {
                message(h, b"The REXX procedure has finished.", TITLE, MB_OK | MB_INFORMATION | MB_MOVEABLE);
            }
            return 0;
        }
        WM_CLOSE => {
            unsafe {
                WinSendMsg(CLIENT.load(SeqCst), WM_SAVEAPPLICATION, 0, 0);
                WinPostMsg(FRAME.load(SeqCst), WM_QUIT, 0, 0);
            }
            return 0;
        }
        _ => {}
    }
    call_super(h, m, mp1, mp2)
}

fn mle(h: u32) -> u32 {
    unsafe { WinWindowFromID(h, MLE_WNDW) }
}

fn command(h: u32, mp1: u32, mp2: u32) -> u32 {
    match mp1 & 0xffff {
        PMRXA_FLAGCHANGE => {
            TRACE.store(mp2 != 0, SeqCst);
            0
        }
        PMRXA_RESULTCHANGE => {
            RESULTS.store(!RESULTS.load(SeqCst), SeqCst);
            0
        }
        RXIOA_RESTART => {
            // Without "Append results" the output of the last run goes.
            if !RESULTS.load(SeqCst) {
                let m = mle(h);
                unsafe { WinEnableWindowUpdate(h, 0) };
                let n = unsafe { WinSendMsg(m, MLM_QUERYTEXTLENGTH, 0, 0) };
                unsafe {
                    WinSendMsg(m, MLM_DELETE, 0, n);
                    WinInvalidateRect(h, core::ptr::null(), 1);
                    WinEnableWindowUpdate(h, 1);
                    WinSendMsg(m, MLM_RESETUNDO, 0, 0);
                }
            }
            start_rexx();
            0
        }
        RXIOA_EXIT => {
            unsafe { WinPostMsg(h, WM_CLOSE, 0, 0) };
            0
        }
        RXIOA_SAVE => {
            unsafe { WinSendMsg(mle(h), MLM_RESETUNDO, 0, 0) };
            let file = shared().file.clone();
            if file.is_empty() {
                return unsafe { WinSendMsg(h, WM_COMMAND, RXIOA_SAVEAS, 0) };
            }
            if write_file(h, &file, false) {
                unsafe { WinSendMsg(mle(h), MLM_SETCHANGED, 0, 0) };
                return 1;
            }
            0
        }
        RXIOA_SAVEAS => {
            let r = match save_as_dialog() {
                Some(name) => {
                    if write_file(h, &name, true) {
                        unsafe { WinSendMsg(mle(h), MLM_SETCHANGED, 0, 0) };
                        shared().file = name;
                    }
                    1
                }
                None => 2,
            };
            unsafe { WinSendMsg(mle(h), MLM_RESETUNDO, 0, 0) };
            r
        }
        RXIOA_ABOUT => {
            message(h, b"PMREXX - REXX procedures in a Presentation Manager window.\nPart of rs2, an OS/2 compatible operating system.", b"Product Information", MB_OK | MB_INFORMATION | MB_MOVEABLE);
            0
        }
        _ => call_super(h, WM_COMMAND, mp1, mp2),
    }
}

/// The standard file dialog for Save as.
fn save_as_dialog() -> Option<Vec<u8>> {
    let mut fd = [0u8; 328];
    let wr = |fd: &mut [u8; 328], o: usize, v: u32| fd[o..o + 4].copy_from_slice(&v.to_le_bytes());
    wr(&mut fd, 0, 328);
    // FDS_CENTER | FDS_HELPBUTTON | FDS_SAVEAS_DIALOG | FDS_ENABLEFILELB
    wr(&mut fd, 4, 0xa09);
    wr(&mut fd, 20, b"Save PMREXX Output As\0".as_ptr() as u32);
    fd[52..55].copy_from_slice(b"*.*");
    let ok = unsafe { WinFileDlg(HWND_DESKTOP, CLIENT.load(SeqCst), fd.as_mut_ptr()) } != 0;
    let ret = u32::from_le_bytes([fd[12], fd[13], fd[14], fd[15]]);
    if !ok || ret != 1 {
        return None;
    }
    let l = fd[52..312].iter().position(|&c| c == 0).unwrap_or(260);
    (l > 0).then(|| fd[52..52 + l].to_vec())
}

/// Write the output window's text to a file (as IBM's, ended by a
/// Ctrl+Z). `ask`: confirm replacing an existing file.
fn write_file(h: u32, name: &[u8], ask: bool) -> bool {
    let m = mle(h);
    let path = z(name);
    let mut hf = 0;
    let mut action = 0;
    // OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE | OPEN_FLAGS_FAIL_ON_ERROR
    let mode = 0x4021;
    let mut rc = unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, 0, 0, if ask { 0x10 } else { 0x12 }, mode, core::ptr::null_mut()) };
    if ask && rc == 110 {
        let mut text = name.to_vec();
        text.extend_from_slice(b" exists. Replace it?");
        if message(h, &text, b"PMREXX Warning", MB_YESNO | MB_QUERY | MB_MOVEABLE) != MBID_YES {
            return false;
        }
        rc = unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, 0, 0, 0x12, mode, core::ptr::null_mut()) };
    }
    if rc != 0 {
        let mut text = b"The file ".to_vec();
        text.extend_from_slice(name);
        text.extend_from_slice(b" could not be written.");
        message(h, &text, ERROR_TITLE, MB_OK | MB_CUAWARNING | MB_MOVEABLE);
        return false;
    }
    let len = unsafe { WinSendMsg(m, MLM_QUERYTEXTLENGTH, 0, 0) };
    let size = unsafe { WinSendMsg(m, MLM_QUERYFORMATTEXTLENGTH, 0, len) }.min(0xfffe);
    let mut buf = alloc::vec![0u8; size as usize + 1];
    unsafe { WinSendMsg(m, MLM_SETIMPORTEXPORT, buf.as_mut_ptr() as u32, size) };
    // MLM_EXPORT moves the insertion point on and counts the rest down.
    let mut ipt = 0u32;
    let mut left = len;
    let mut ok = true;
    while left > 0 {
        let got = unsafe { WinSendMsg(m, MLM_EXPORT, &mut ipt as *mut u32 as u32, &mut left as *mut u32 as u32) };
        if got == 0 {
            break;
        }
        let mut n = 0;
        if unsafe { os2::DosWrite(hf, buf.as_ptr(), got, &mut n) } != 0 {
            ok = false;
            break;
        }
    }
    let mut n = 0;
    unsafe {
        os2::DosWrite(hf, b"\x1a".as_ptr(), 1, &mut n);
        os2::DosClose(hf);
    }
    ok
}
