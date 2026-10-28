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

    #[link_name = "PMGPI.588"]
    pub fn GpiSetAttrs(hps: u32, prim: i32, attrs: u32, defs: u32, bundle: *const u8) -> u32;
    #[link_name = "PMGPI.355"]
    pub fn GpiBitBlt(dst: u32, src: u32, n: i32, p: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.598"]
    pub fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, init: *const u8, bmi: *const u8) -> u32;
    #[link_name = "PMGPI.371"]
    pub fn GpiDeleteBitmap(hbm: u32) -> u32;
    #[link_name = "PMGPI.506"]
    pub fn GpiSetBitmap(hps: u32, hbm: u32) -> u32;
    #[link_name = "PMGPI.602"]
    pub fn GpiSetBitmapBits(hps: u32, start: i32, scans: i32, buf: *const u8, bmi: *const u8) -> i32;
    #[link_name = "PMGPI.599"]
    pub fn GpiQueryBitmapBits(hps: u32, start: i32, scans: i32, buf: *mut u8, bmi: *mut u8) -> i32;
    #[link_name = "PMGPI.369"]
    pub fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    pub fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.610"]
    pub fn DevOpenDC(hab: u32, ty: i32, token: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    pub fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.370"]
    pub fn GpiCreateRegion(hps: u32, n: i32, rects: *const RectL) -> u32;
    #[link_name = "PMGPI.611"]
    pub fn GpiDestroyRegion(hps: u32, h: u32) -> u32;
    #[link_name = "PMGPI.603"]
    pub fn GpiDrawBits(hps: u32, bits: *const u8, bmi: *const u8, n: i32, p: *const PointL, rop: i32, opts: u32) -> i32;
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

    #[link_name = "PMGPI.513"]
    pub fn GpiSetCharSet(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.378"]
    pub fn GpiDeleteSetId(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.368"]
    pub fn GpiCreateLogFont(hps: u32, name: *const u8, lcid: i32, fat: *const u8) -> i32;
    #[link_name = "PMGPI.510"]
    pub fn GpiSetCharBox(hps: u32, p: *const i32) -> u32;
    #[link_name = "PMGPI.396"]
    pub fn GpiIntersectClipRectangle(hps: u32, r: *const RectL) -> i32;
    #[link_name = "PMGPI.391"]
    pub fn GpiExcludeClipRectangle(hps: u32, r: *const RectL) -> i32;
    #[link_name = "PMGPI.516"]
    pub fn GpiSetClipRegion(hps: u32, h: u32, old: *mut u32) -> i32;
    #[link_name = "PMGPI.435"]
    pub fn GpiQueryClipBox(hps: u32, r: *mut RectL) -> i32;
    #[link_name = "PMGPI.502"]
    pub fn GpiSetArcParams(hps: u32, p: *const [i32; 4]) -> u32;
    #[link_name = "PMGPI.393"]
    pub fn GpiFullArc(hps: u32, ctl: i32, mult: i32) -> i32;
    #[link_name = "PMGPI.415"]
    pub fn GpiPolyLine(hps: u32, n: i32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.476"]
    pub fn GpiQueryPel(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.352"]
    pub fn GpiBeginArea(hps: u32, opts: u32) -> u32;
    #[link_name = "PMGPI.385"]
    pub fn GpiEndArea(hps: u32) -> i32;
    #[link_name = "PMWIN.767"]
    pub fn WinInvertRect(hps: u32, r: *const RectL) -> u32;
    #[link_name = "PMWIN.852"]
    pub fn WinSetCapture(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.804"]
    pub fn WinQueryCapture(desk: u32) -> u32;
    #[link_name = "PMWIN.849"]
    pub fn WinScrollWindow(h: u32, dx: i32, dy: i32, scroll: *const RectL, clip: *const RectL, rgn: u32, upd: *mut RectL, fl: u32) -> i32;
    #[link_name = "PMWIN.864"]
    pub fn WinSetOwner(h: u32, owner: u32) -> u32;
    #[link_name = "PMWIN.865"]
    pub fn WinSetParent(h: u32, parent: u32, redraw: u32) -> u32;
    #[link_name = "PMWIN.874"]
    pub fn WinSetWindowBits(h: u32, i: i32, v: u32, mask: u32) -> u32;
    #[link_name = "PMWIN.899"]
    pub fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.879"]
    pub fn WinSetWindowUShort(h: u32, i: i32, v: u16) -> u32;
    #[link_name = "PMWIN.715"]
    pub fn WinCreateCursor(h: u32, x: i32, y: i32, cx: i32, cy: i32, fs: u32, clip: *const RectL) -> u32;
    #[link_name = "PMWIN.880"]
    pub fn WinShowCursor(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.725"]
    pub fn WinDestroyCursor(h: u32) -> u32;
    #[link_name = "PMWIN.793"]
    pub fn WinOpenClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.707"]
    pub fn WinCloseClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.733"]
    pub fn WinEmptyClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.854"]
    pub fn WinSetClipbrdData(hab: u32, data: u32, fmt: u32, info: u32) -> u32;
    #[link_name = "PMWIN.806"]
    pub fn WinQueryClipbrdData(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.807"]
    pub fn WinQueryClipbrdFmtInfo(hab: u32, fmt: u32, info: *mut u32) -> u32;
    #[link_name = "PMWIN.752"]
    pub fn WinGetKeyState(desk: u32, vk: i32) -> i32;
    #[link_name = "PMWIN.831"]
    pub fn WinQueryUpdateRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.900"]
    pub fn WinWindowFromPoint(h: u32, p: *const PointL, children: u32) -> u32;
    #[link_name = "PMWIN.818"]
    pub fn WinQueryMsgPos(hab: u32, p: *mut PointL) -> u32;
    #[link_name = "PMWIN.819"]
    pub fn WinQueryMsgTime(hab: u32) -> u32;
    #[link_name = "PMWIN.773"]
    pub fn WinIsWindowEnabled(h: u32) -> u32;
    #[link_name = "PMWIN.745"]
    pub fn WinFlashWindow(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.867"]
    pub fn WinSetPointerPos(desk: u32, x: i32, y: i32) -> u32;

    #[link_name = "DOSCALLS.320"]
    pub fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> u32;
    #[link_name = "RS2API.4"]
    pub fn Rs2DebugTrace(psz: *const u8) -> u32;
    #[link_name = "RS2API.420"]
    pub fn Rs2Win16SetIntHandler(handler: u32) -> u32;
    #[link_name = "RS2API.421"]
    pub fn Rs2Win16Selector(op: u32, sel: u32, value: u32) -> u32;
    #[link_name = "RS2API.422"]
    pub fn Rs2Win16ReadTask(pid: u32, sel: u32, off: u32, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "RS2API.423"]
    pub fn Rs2Win16Debug(op: u32, pid: u32, a: u32, b: u32, c: u32) -> u32;
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
pub const WM_SYSCOMMAND: u32 = 0x0021;
pub const WM_CONTROL: u32 = 0x0030;
pub const WM_VSCROLL: u32 = 0x0031;
pub const WM_HSCROLL: u32 = 0x0032;
pub const WM_INITMENU: u32 = 0x0033;
pub const WM_MENUSELECT: u32 = 0x0034;
pub const WM_MENUEND: u32 = 0x0035;
pub const WM_UPDATEFRAME: u32 = 0x0042;
pub const WM_BUTTON1UP: u32 = 0x0072;
pub const FID_MENU: u32 = 0x8005;
pub const FID_VERTSCROLL: u32 = 0x8006;
pub const FID_HORZSCROLL: u32 = 0x8007;
pub const FID_CLIENT: u32 = 0x8008;
pub const HWND_OBJECT: u32 = 2;
pub const HWND_BOTTOM: u32 = 4;
pub const QWS_ID: i32 = -1;
pub const WS_CLIPCHILDREN: u32 = 0x2000_0000;
pub const WS_DISABLED: u32 = 0x4000_0000;

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
/// rs2's PM: not activated by a click (pmwin::ws::NOACTIVATE).
pub const NOACTIVATE: u32 = 0x0010_0000;
pub const QWL_STYLE: i32 = -2;

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
pub const SV_CXVSCROLL: i32 = 22;
pub const SV_CYHSCROLL: i32 = 23;
pub const SV_CYVSCROLLARROW: i32 = 24;
pub const SV_CXHSCROLLARROW: i32 = 25;
pub const SV_CXBORDER: i32 = 26;
pub const SV_CYBORDER: i32 = 27;
pub const SV_CXDLGFRAME: i32 = 28;
pub const SV_CYTITLEBAR: i32 = 30;
pub const SV_CYMENU: i32 = 35;
pub const SV_CXPOINTER: i32 = 40;
pub const SV_CYPOINTER: i32 = 41;
pub const SV_CXSIZEBORDER: i32 = 4;
pub const SV_CYSIZEBORDER: i32 = 5;

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
