//! The PMWIN, PMGPI and DOSCALLS entry points the help manager uses,
//! their constants and structures.

#![allow(dead_code)]

use alloc::vec::Vec;

pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct RectL {
    pub l: i32,
    pub b: i32,
    pub r: i32,
    pub t: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct QMsg {
    pub hwnd: u32,
    pub msg: u32,
    pub mp1: u32,
    pub mp2: u32,
    pub time: u32,
    pub x: i32,
    pub y: i32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Swp {
    pub fl: u32,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub behind: u32,
    pub hwnd: u32,
    pub reserved1: u32,
    pub reserved2: u32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.926"]
    pub fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    pub fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    pub fn WinCreateWindow(parent: u32, class: *const u8, text: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    pub fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    pub fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    pub fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    pub fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    pub fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.743"]
    pub fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.840"]
    pub fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.877"]
    pub fn WinSetWindowText(h: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.899"]
    pub fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.837"]
    pub fn WinQueryWindowPos(h: u32, swp: *mut Swp) -> u32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.851"]
    pub fn WinSetActiveWindow(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.860"]
    pub fn WinSetFocus(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.817"]
    pub fn WinQueryFocus(desktop: u32) -> u32;
    #[link_name = "PMWIN.799"]
    pub fn WinQueryActiveWindow(desktop: u32) -> u32;
    #[link_name = "PMWIN.843"]
    pub fn WinQueryWindowULong(h: u32, i: i32) -> u32;
    #[link_name = "PMWIN.878"]
    pub fn WinSetWindowULong(h: u32, i: i32, v: u32) -> u32;
    #[link_name = "PMWIN.844"]
    pub fn WinQueryWindowUShort(h: u32, i: i32) -> u16;
    #[link_name = "PMWIN.834"]
    pub fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.772"]
    pub fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.838"]
    pub fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.928"]
    pub fn WinSetHook(hab: u32, hmq: u32, kind: i32, pfn: u32, hmod: u32) -> u32;
    #[link_name = "PMWIN.927"]
    pub fn WinReleaseHook(hab: u32, hmq: u32, kind: i32, pfn: u32, hmod: u32) -> u32;
    #[link_name = "PMWIN.829"]
    pub fn WinQuerySysValue(desktop: u32, i: i32) -> i32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.903"]
    pub fn WinSendDlgItemMsg(h: u32, id: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.866"]
    pub fn WinSetPointer(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.828"]
    pub fn WinQuerySysPointer(desk: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.735"]
    pub fn WinEnableWindow(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.788"]
    pub fn WinMapWindowPoints(from: u32, to: u32, pts: *mut PointL, n: i32) -> u32;
    #[link_name = "PMWIN.841"]
    pub fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.800"]
    pub fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.763"]
    pub fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    pub fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.915"]
    pub fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    pub fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.726"]
    pub fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    pub fn WinTerminate(hab: u32) -> u32;

    #[link_name = "PMGPI.359"]
    pub fn GpiCharStringAt(hps: u32, p: *const PointL, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.489"]
    pub fn GpiQueryTextBox(hps: u32, n: i32, s: *const u8, n2: i32, out: *mut PointL) -> u32;
    #[link_name = "PMGPI.453"]
    pub fn GpiQueryFontMetrics(hps: u32, len: i32, out: *mut u8) -> u32;
    #[link_name = "PMGPI.517"]
    pub fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.504"]
    pub fn GpiSetBackColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.505"]
    pub fn GpiSetBackMix(hps: u32, m: i32) -> u32;
    #[link_name = "PMGPI.404"]
    pub fn GpiMove(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.398"]
    pub fn GpiLine(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.356"]
    pub fn GpiBox(hps: u32, ctl: i32, p: *const PointL, h: i32, v: i32) -> i32;
    #[link_name = "PMGPI.603"]
    pub fn GpiDrawBits(hps: u32, bits: *const u8, bmi: *const u8, n: i32, p: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.514"]
    pub fn GpiSetCharShear(hps: u32, p: *const PointL) -> u32;
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.352"]
    pub fn DosGetResource(hmod: u32, ty: u32, id: u32, pp: *mut *const u8) -> u32;
    #[link_name = "DOSCALLS.353"]
    pub fn DosFreeResource(p: *const u8) -> u32;
    #[link_name = "DOSCALLS.228"]
    pub fn DosSearchPath(control: u32, path: *const u8, name: *const u8, buf: *mut u8, cb: u32) -> u32;
}

pub const HWND_DESKTOP: u32 = 1;
pub const HWND_OBJECT: u32 = 2;
pub const HWND_TOP: u32 = 3;

pub const WS_VISIBLE: u32 = 0x8000_0000;
pub const WS_CLIPCHILDREN: u32 = 0x2000_0000;
pub const WS_TABSTOP: u32 = 0x0002_0000;
pub const WS_GROUP: u32 = 0x0001_0000;

pub const FCF_TITLEBAR: u32 = 0x0001;
pub const FCF_SYSMENU: u32 = 0x0002;
pub const FCF_SIZEBORDER: u32 = 0x0008;
pub const FCF_MINBUTTON: u32 = 0x0010;
pub const FCF_MAXBUTTON: u32 = 0x0020;
pub const FCF_SHELLPOSITION: u32 = 0x0000_0400;
pub const FCF_TASKLIST: u32 = 0x0000_0800;
pub const FCF_NOBYTEALIGN: u32 = 0x0000_1000;

pub const SWP_SIZE: u32 = 0x0001;
pub const SWP_MOVE: u32 = 0x0002;
pub const SWP_ZORDER: u32 = 0x0004;
pub const SWP_SHOW: u32 = 0x0008;
pub const SWP_ACTIVATE: u32 = 0x0080;
pub const SWP_RESTORE: u32 = 0x1000;

pub const QW_PARENT: i32 = 5;
pub const QW_OWNER: i32 = 4;
pub const QWS_ID: i32 = -1;
pub const QWL_USER: i32 = 0;

pub const SV_CXSCREEN: i32 = 20;
pub const SV_CYSCREEN: i32 = 21;
pub const SV_CXVSCROLL: i32 = 22;

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0007;
pub const WM_SETFOCUS: u32 = 0x000f;
pub const WM_ACTIVATE: u32 = 0x000d;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_SYSCOMMAND: u32 = 0x0021;
pub const WM_HELP: u32 = 0x0022;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_QUIT: u32 = 0x002a;
pub const WM_CONTROL: u32 = 0x0030;
pub const WM_VSCROLL: u32 = 0x0031;
pub const WM_HSCROLL: u32 = 0x0032;
pub const WM_MOUSEMOVE: u32 = 0x0070;
pub const WM_BUTTON1DOWN: u32 = 0x0071;
pub const WM_BUTTON1DBLCLK: u32 = 0x0073;
pub const WM_CHAR: u32 = 0x007a;

pub const SBM_SETSCROLLBAR: u32 = 0x01a0;
pub const SBM_SETPOS: u32 = 0x01a1;
pub const SBM_SETTHUMBSIZE: u32 = 0x01a6;
pub const SB_LINEUP: u32 = 1;
pub const SB_LINEDOWN: u32 = 2;
pub const SB_PAGEUP: u32 = 3;
pub const SB_PAGEDOWN: u32 = 4;
pub const SB_SLIDERTRACK: u32 = 5;
pub const SB_SLIDERPOSITION: u32 = 6;

pub const LM_INSERTITEM: u32 = 0x0161;
pub const LM_DELETEALL: u32 = 0x016e;
pub const LM_QUERYSELECTION: u32 = 0x0165;
pub const LM_SELECTITEM: u32 = 0x0164;
pub const LM_SETITEMHANDLE: u32 = 0x0169;
pub const LM_QUERYITEMHANDLE: u32 = 0x016a;
pub const LM_SETTOPINDEX: u32 = 0x0162;
pub const LM_QUERYITEMCOUNT: u32 = 0x0160;
pub const LIT_END: u32 = 0xffff_ffff;
pub const LIT_FIRST: u32 = 0xffff_ffff;
pub const LN_ENTER: u32 = 5;

pub const BS_PUSHBUTTON: u32 = 0;
pub const BS_NOPOINTERFOCUS: u32 = 0x0800;
pub const BN_CLICKED: u32 = 1;
pub const CMDSRC_PUSHBUTTON: u32 = 1;

pub const ES_AUTOSCROLL: u32 = 0x0004;
pub const ES_MARGIN: u32 = 0x0008;
pub const EM_SETTEXTLIMIT: u32 = 0x0143;

pub const KC_CHAR: u32 = 0x0001;
pub const KC_VIRTUALKEY: u32 = 0x0002;
pub const KC_KEYUP: u32 = 0x0040;
pub const KC_SHIFT: u32 = 0x0008;
pub const KC_CTRL: u32 = 0x0010;
pub const VK_BACKSPACE: u32 = 0x05;
pub const VK_TAB: u32 = 0x06;
pub const VK_BACKTAB: u32 = 0x07;
pub const VK_NEWLINE: u32 = 0x08;
pub const VK_ESC: u32 = 0x0f;
pub const VK_PAGEUP: u32 = 0x11;
pub const VK_PAGEDOWN: u32 = 0x12;
pub const VK_END: u32 = 0x13;
pub const VK_HOME: u32 = 0x14;
pub const VK_LEFT: u32 = 0x15;
pub const VK_UP: u32 = 0x16;
pub const VK_RIGHT: u32 = 0x17;
pub const VK_DOWN: u32 = 0x18;
pub const VK_ENTER: u32 = 0x1e;

pub const CLR_WHITE: i32 = -2;
pub const CLR_BLACK: i32 = -1;
pub const CLR_BACKGROUND: i32 = 0;
pub const CLR_DARKBLUE: i32 = 9;
pub const CLR_PALEGRAY: i32 = 15;
pub const SYSCLR_WINDOW: i32 = -20;
pub const SYSCLR_WINDOWTEXT: i32 = -17;
pub const SYSCLR_HELPBACKGROUND: i32 = -12;
pub const SYSCLR_HELPTEXT: i32 = -11;
pub const SYSCLR_HELPHILITE: i32 = -10;
pub const BM_LEAVEALONE: i32 = 5;
pub const BM_OVERPAINT: i32 = 2;
pub const DRO_OUTLINE: i32 = 2;
pub const DRO_FILL: i32 = 1;
pub const ROP_SRCCOPY: i32 = 0xcc;

pub const SPTR_ARROW: i32 = 1;
pub const SPTR_WAIT: i32 = 3;

pub const MB_OK: u32 = 0x0000;
pub const MB_ERROR: u32 = 0x0040;
pub const MB_MOVEABLE: u32 = 0x4000;

pub fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

pub fn post(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinPostMsg(h, m, a, b) }
}

pub fn lo(v: u32) -> u32 {
    v & 0xffff
}

pub fn hi(v: u32) -> u32 {
    v >> 16
}

/// A NUL-terminated copy of `t`.
pub fn z(t: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(t.len() + 1);
    v.extend_from_slice(t);
    v.push(0);
    v
}

/// The bytes of a C string.
pub unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

pub fn window_rect(h: u32) -> RectL {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(h, &mut r) };
    r
}

pub fn id(h: u32) -> u32 {
    unsafe { WinQueryWindowUShort(h, QWS_ID) as u32 }
}

pub fn sysval(i: i32) -> i32 {
    unsafe { WinQuerySysValue(HWND_DESKTOP, i) }
}

/// This process's id.
pub fn pid() -> u32 {
    let (mut tib, mut pib) = (core::ptr::null_mut(), core::ptr::null_mut());
    unsafe {
        os2::dos::DosGetInfoBlocks(&mut tib, &mut pib);
        (*pib).pid
    }
}
