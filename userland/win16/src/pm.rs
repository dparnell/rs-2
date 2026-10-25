//! The PMWIN, PMGPI and DOSCALLS entry points the Win16 layer is built
//! on, and their constants.

#![allow(dead_code)]

pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

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
#[derive(Default, Clone, Copy, Debug)]
pub struct RectL {
    pub l: i32,
    pub b: i32,
    pub r: i32,
    pub t: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

/// SWP
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
    #[link_name = "PMWIN.763"]
    pub fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    pub fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.926"]
    pub fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    pub fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    pub fn WinCreateWindow(parent: u32, class: *const u8, text: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.915"]
    pub fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.918"]
    pub fn WinPeekMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.912"]
    pub fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.911"]
    pub fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    pub fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.902"]
    pub fn WinPostQueueMsg(hmq: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    pub fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    pub fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    pub fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.743"]
    pub fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.840"]
    pub fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.895"]
    pub fn WinValidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.892"]
    pub fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMWIN.877"]
    pub fn WinSetWindowText(h: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.841"]
    pub fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.837"]
    pub fn WinQueryWindowPos(h: u32, swp: *mut Swp) -> u32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.775"]
    pub fn WinIsWindowVisible(h: u32) -> u32;
    #[link_name = "PMWIN.772"]
    pub fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.860"]
    pub fn WinSetFocus(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.817"]
    pub fn WinQueryFocus(desktop: u32) -> u32;
    #[link_name = "PMWIN.799"]
    pub fn WinQueryActiveWindow(desk: u32) -> u32;
    #[link_name = "PMWIN.851"]
    pub fn WinSetActiveWindow(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.829"]
    pub fn WinQuerySysValue(desktop: u32, i: i32) -> i32;
    #[link_name = "PMWIN.826"]
    pub fn WinQuerySysColor(desktop: u32, i: i32, reserved: u32) -> i32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.735"]
    pub fn WinEnableWindow(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.828"]
    pub fn WinQuerySysPointer(desktop: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.866"]
    pub fn WinSetPointer(desktop: u32, hptr: u32) -> u32;
    #[link_name = "PMWIN.823"]
    pub fn WinQueryPointerPos(desktop: u32, p: *mut PointL) -> u32;
    #[link_name = "PMWIN.788"]
    pub fn WinMapWindowPoints(from: u32, to: u32, p: *mut PointL, n: i32) -> u32;
    #[link_name = "PMWIN.750"]
    pub fn WinGetCurrentTime(hab: u32) -> u32;
    #[link_name = "PMWIN.884"]
    pub fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    pub fn WinStopTimer(hab: u32, h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.834"]
    pub fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.701"]
    pub fn WinAlarm(desk: u32, kind: u32) -> u32;
    #[link_name = "PMWIN.913"]
    pub fn WinDrawText(hps: u32, cch: i32, text: *const u8, r: *mut RectL, fg: i32, bg: i32, fl: u32) -> i32;

    #[link_name = "PMGPI.592"]
    pub fn GpiCreateLogColorTable(hps: u32, opts: u32, fmt: i32, start: i32, count: i32, table: *const i32) -> u32;
    #[link_name = "PMGPI.517"]
    pub fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.504"]
    pub fn GpiSetBackColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.505"]
    pub fn GpiSetBackMix(hps: u32, m: i32) -> u32;
    #[link_name = "PMGPI.537"]
    pub fn GpiSetMix(hps: u32, m: i32) -> u32;
    #[link_name = "PMGPI.453"]
    pub fn GpiQueryFontMetrics(hps: u32, cb: i32, fm: *mut u8) -> u32;
    #[link_name = "PMGPI.359"]
    pub fn GpiCharStringAt(hps: u32, p: *const PointL, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.489"]
    pub fn GpiQueryTextBox(hps: u32, n: i32, s: *const u8, count: i32, pts: *mut PointL) -> u32;
    #[link_name = "PMGPI.519"]
    pub fn GpiSetCurrentPosition(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.404"]
    pub fn GpiMove(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.398"]
    pub fn GpiLine(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.356"]
    pub fn GpiBox(hps: u32, ctl: i32, p: *const PointL, h: i32, v: i32) -> i32;
    #[link_name = "PMGPI.530"]
    pub fn GpiSetLineType(hps: u32, t: i32) -> u32;
    #[link_name = "PMGPI.531"]
    pub fn GpiSetLineWidth(hps: u32, w: i32) -> u32;

    #[link_name = "DOSCALLS.320"]
    pub fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> u32;
    #[link_name = "RS2API.4"]
    pub fn Rs2DebugTrace(psz: *const u8) -> u32;
    #[link_name = "RS2API.420"]
    pub fn Rs2Win16SetIntHandler(handler: u32) -> u32;
}

pub const HWND_DESKTOP: u32 = 1;
pub const HWND_TOP: u32 = 3;

// PM messages.
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_ENABLE: u32 = 0x0004;
pub const WM_SHOW: u32 = 0x0005;
pub const WM_MOVE: u32 = 0x0006;
pub const WM_SIZE: u32 = 0x0007;
pub const WM_ACTIVATE: u32 = 0x000d;
pub const WM_SETFOCUS: u32 = 0x000f;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_TIMER: u32 = 0x0024;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_QUIT: u32 = 0x002a;
pub const WM_ERASEBACKGROUND: u32 = 0x004f;
pub const WM_MOUSEMOVE: u32 = 0x0070;
pub const WM_BUTTON1DOWN: u32 = 0x0071;
pub const WM_BUTTON3DBLCLK: u32 = 0x0079;
pub const WM_CHAR: u32 = 0x007a;

// Frame creation flags.
pub const FCF_TITLEBAR: u32 = 0x0001;
pub const FCF_SYSMENU: u32 = 0x0002;
pub const FCF_SIZEBORDER: u32 = 0x0008;
pub const FCF_MINBUTTON: u32 = 0x0010;
pub const FCF_MAXBUTTON: u32 = 0x0020;
pub const FCF_BORDER: u32 = 0x0100;
pub const FCF_DLGBORDER: u32 = 0x0200;
pub const FCF_TASKLIST: u32 = 0x0800;
pub const FCF_NOBYTEALIGN: u32 = 0x1000;

pub const WS_VISIBLE: u32 = 0x8000_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x1000_0000;

pub const SWP_SIZE: u32 = 0x0001;
pub const SWP_MOVE: u32 = 0x0002;
pub const SWP_ZORDER: u32 = 0x0004;
pub const SWP_SHOW: u32 = 0x0008;
pub const SWP_HIDE: u32 = 0x0010;
pub const SWP_ACTIVATE: u32 = 0x0080;
pub const SWP_MINIMIZE: u32 = 0x0400;
pub const SWP_MAXIMIZE: u32 = 0x0800;
pub const SWP_RESTORE: u32 = 0x1000;

pub const QW_PARENT: i32 = 5;
pub const SV_CXSCREEN: i32 = 20;
pub const SV_CYSCREEN: i32 = 21;
pub const SV_CXPOINTER: i32 = 25;
pub const SV_CYPOINTER: i32 = 26;
pub const SV_CYTITLEBAR: i32 = 30;
pub const SV_CXSIZEBORDER: i32 = 4;
pub const SV_CYSIZEBORDER: i32 = 5;
pub const SV_CXBORDER: i32 = 2;
pub const SV_CYBORDER: i32 = 3;

pub const LCOLF_RGB: i32 = 3;
pub const BM_LEAVEALONE: i32 = 5;
pub const BM_OVERPAINT: i32 = 2;
pub const FM_OVERPAINT: i32 = 2;
pub const DRO_OUTLINE: i32 = 2;
pub const DRO_FILL: i32 = 1;
pub const TXTBOX_CONCAT: usize = 4;
pub const TXTBOX_COUNT: i32 = 5;

pub const PM_REMOVE: u32 = 1;
pub const PM_NOREMOVE: u32 = 0;

/// A line on the serial port.
pub fn trace(args: core::fmt::Arguments) {
    let mut s = alloc::format!("{}", args).into_bytes();
    s.push(0);
    unsafe { Rs2DebugTrace(s.as_ptr()) };
}
