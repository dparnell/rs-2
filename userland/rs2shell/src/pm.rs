//! The PMWIN, PMGPI, PMSHAPI, PMWP, DOSCALLS and RS2API entry points the
//! shell uses, their constants and structures, and small helpers (strings,
//! menus, dialog templates, icons made from pictures).

#![allow(dead_code)]

use alloc::vec::Vec;

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
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct RectL {
    pub l: i32,
    pub b: i32,
    pub r: i32,
    pub t: i32,
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

/// MENUITEM
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct MenuItem {
    pub pos: i16,
    pub style: u16,
    pub attr: u16,
    pub id: u16,
    pub sub: u32,
    pub handle: u32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    pub fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    pub fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    pub fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    pub fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    pub fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    pub fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    pub fn WinCreateWindow(parent: u32, class: u32, text: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
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
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    pub fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    pub fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.743"]
    pub fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.840"]
    pub fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.877"]
    pub fn WinSetWindowText(h: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.841"]
    pub fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.899"]
    pub fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.884"]
    pub fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    pub fn WinStopTimer(hab: u32, h: u32, id: u32) -> u32;
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
    #[link_name = "PMWIN.852"]
    pub fn WinSetCapture(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.829"]
    pub fn WinQuerySysValue(desktop: u32, i: i32) -> i32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.735"]
    pub fn WinEnableWindow(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.830"]
    pub fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.700"]
    pub fn WinAddAtom(table: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.828"]
    pub fn WinQuerySysPointer(desk: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.942"]
    pub fn WinCreatePointerIndirect(desk: u32, pi: *const u32) -> u32;
    #[link_name = "PMWIN.727"]
    pub fn WinDestroyPointer(h: u32) -> u32;
    #[link_name = "PMWIN.866"]
    pub fn WinSetPointer(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.823"]
    pub fn WinQueryPointerPos(desk: u32, p: *mut PointL) -> u32;
    #[link_name = "PMWIN.937"]
    pub fn WinPopupMenu(parent: u32, owner: u32, menu: u32, x: i32, y: i32, id: u32, fs: u32) -> u32;
    #[link_name = "PMWIN.907"]
    pub fn WinCreateMenu(parent: u32, tpl: *const u8) -> u32;
    #[link_name = "PMWIN.922"]
    pub fn WinCreateDlg(parent: u32, owner: u32, proc_: u32, tpl: *const u8, params: u32) -> u32;
    #[link_name = "PMWIN.796"]
    pub fn WinProcessDlg(h: u32) -> u32;
    #[link_name = "PMWIN.729"]
    pub fn WinDismissDlg(h: u32, result: u32) -> u32;
    #[link_name = "PMWIN.910"]
    pub fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.903"]
    pub fn WinSendDlgItemMsg(h: u32, id: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.815"]
    pub fn WinQueryDlgItemText(h: u32, id: u32, cch: i32, buf: *mut u8) -> u32;
    #[link_name = "PMWIN.859"]
    pub fn WinSetDlgItemText(h: u32, id: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.878"]
    pub fn WinSetWindowULong(h: u32, i: i32, v: u32) -> u32;
    #[link_name = "PMWIN.843"]
    pub fn WinQueryWindowULong(h: u32, i: i32) -> u32;
    #[link_name = "PMWIN.834"]
    pub fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.788"]
    pub fn WinMapWindowPoints(from: u32, to: u32, pts: *mut PointL, n: i32) -> u32;
    #[link_name = "PMWIN.900"]
    pub fn WinWindowFromPoint(parent: u32, p: *const PointL, children: u32) -> u32;
    #[link_name = "PMWIN.838"]
    pub fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.772"]
    pub fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.775"]
    pub fn WinIsWindowVisible(h: u32) -> u32;
    #[link_name = "PMWIN.929"]
    pub fn WinSubclassWindow(h: u32, p: WndProc) -> u32;
    #[link_name = "PMWIN.913"]
    pub fn WinDrawText(hps: u32, cch: i32, text: *const u8, r: *mut RectL, fg: i32, bg: i32, fl: u32) -> i32;
    #[link_name = "PMWIN.939"]
    pub fn WinQueryPresParam(h: u32, id1: u32, id2: u32, pid: *mut u32, cb: u32, pv: *mut u32, fs: u32) -> u32;
    #[link_name = "PMWIN.938"]
    pub fn WinSetPresParam(h: u32, id: u32, cb: u32, pv: *const u32) -> u32;
    #[link_name = "PMWIN.701"]
    pub fn WinAlarm(desk: u32, kind: u32) -> u32;
    #[link_name = "PMWIN.744"]
    pub fn WinFindAtom(table: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.721"]
    pub fn WinDeleteAtom(table: u32, atom: u32) -> u32;
    #[link_name = "PMWIN.752"]
    pub fn WinGetKeyState(desk: u32, vk: i32) -> i32;
    #[link_name = "PMWIN.867"]
    pub fn WinSetPointerPos(desk: u32, x: i32, y: i32) -> u32;
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    pub fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.9003"]
    pub fn Rs2SetMinimizeViewer(h: u32) -> u32;
    #[link_name = "PMWIN.9007"]
    pub fn Rs2SetDesktopWindow(h: u32) -> u32;
    #[link_name = "PMGPI.598"]
    pub fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, bits: *const u8, info: *const u8) -> u32;
    #[link_name = "PMGPI.592"]
    pub fn GpiCreateLogColorTable(hps: u32, opts: u32, fmt: u32, start: i32, count: i32, table: *const u32) -> u32;
    #[link_name = "PMGPI.371"]
    pub fn GpiDeleteBitmap(h: u32) -> u32;
    #[link_name = "PMWP.209"]
    pub fn WinLoadFileIcon(name: *const u8, private: u32) -> u32;
    #[link_name = "PMWP.216"]
    pub fn WinFreeFileIcon(h: u32) -> u32;
    #[link_name = "PMSHAPI.117"]
    pub fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    pub fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
    #[link_name = "PMSHAPI.101"]
    pub fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.109"]
    pub fn PrfAddProgram(hini: u32, details: *const u8, group: u32) -> u32;
    #[link_name = "PMSHAPI.110"]
    pub fn PrfChangeProgram(hini: u32, hprog: u32, details: *const u8) -> u32;
    #[link_name = "PMSHAPI.111"]
    pub fn PrfQueryDefinition(hini: u32, hprog: u32, details: *mut u8, cb: u32) -> u32;
    #[link_name = "PMSHAPI.104"]
    pub fn PrfRemoveProgram(hini: u32, hprog: u32) -> u32;
    #[link_name = "PMSHAPI.113"]
    pub fn PrfQueryProgramTitles(hini: u32, group: u32, titles: *mut u8, cb: u32, count: *mut u32) -> u32;
    #[link_name = "PMSHAPI.9000"]
    pub fn PrfCreateGroup(hini: u32, title: *const u8, visibility: u32) -> u32;
    #[link_name = "PMSHAPI.106"]
    pub fn PrfDestroyGroup(hini: u32, group: u32) -> u32;
    #[link_name = "DOSCALLS.415"]
    pub fn DosShutdown(reserved: u32) -> u32;
    #[link_name = "DOSCALLS.323"]
    pub fn DosQueryAppType(name: *const u8, flags: *mut u32) -> u32;
    #[link_name = "RS2API.7"]
    pub fn Rs2PowerOff(action: u32) -> u32;
    #[link_name = "RS2API.11"]
    pub fn Rs2VioQuerySessions(p: *mut VioSession, max: u32, found: *mut u32, all: u32) -> u32;
    #[link_name = "RS2API.16"]
    pub fn Rs2VioControl(sid: u32, cmd: u32, arg: u32) -> u32;
}

// Direct manipulation (PMDRAG).
unsafe extern "C" {
    #[link_name = "PMDRAG.33"]
    pub fn DrgAddStrHandle(psz: *const u8) -> u32;
    #[link_name = "PMDRAG.37"]
    pub fn DrgDeleteStrHandle(h: u32) -> u32;
    #[link_name = "PMDRAG.49"]
    pub fn DrgQueryStrName(h: u32, cb: u32, buf: *mut u8) -> u32;
    #[link_name = "PMDRAG.50"]
    pub fn DrgQueryStrNameLen(h: u32) -> u32;
    #[link_name = "PMDRAG.32"]
    pub fn DrgAccessDraginfo(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.34"]
    pub fn DrgAllocDraginfo(n: u32) -> u32;
    #[link_name = "PMDRAG.39"]
    pub fn DrgFreeDraginfo(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.36"]
    pub fn DrgDeleteDraginfoStrHandles(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.38"]
    pub fn DrgDrag(src: u32, pdinfo: u32, pdimg: *const u8, n: u32, vk: u32, rsvd: u32) -> u32;
    #[link_name = "PMDRAG.57"]
    pub fn DrgSetDragitem(pdinfo: u32, item: *const u8, cb: u32, i: u32) -> u32;
    #[link_name = "PMDRAG.46"]
    pub fn DrgQueryDragitemPtr(pdinfo: u32, i: u32) -> u32;
    #[link_name = "PMDRAG.45"]
    pub fn DrgQueryDragitemCount(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.59"]
    pub fn DrgVerifyRMF(pditem: u32, mech: *const u8, fmt: *const u8) -> u32;
    #[link_name = "PMDRAG.61"]
    pub fn DrgVerifyType(pditem: u32, ty: *const u8) -> u32;
    #[link_name = "PMDRAG.35"]
    pub fn DrgAllocDragtransfer(n: u32) -> u32;
    #[link_name = "PMDRAG.40"]
    pub fn DrgFreeDragtransfer(p: u32) -> u32;
    #[link_name = "PMDRAG.54"]
    pub fn DrgSendTransferMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMDRAG.42"]
    pub fn DrgPostTransferMsg(h: u32, msg: u32, pdxfer: u32, fl: u32, rsvd: u32, retry: u32) -> u32;
    #[link_name = "PMDRAG.43"]
    pub fn DrgPushDraginfo(pdinfo: u32, dest: u32) -> u32;
}

pub const DM_DROP: u32 = 0x032f;
pub const DM_DRAGOVER: u32 = 0x032e;
pub const DM_DRAGLEAVE: u32 = 0x032d;
pub const DM_DROPHELP: u32 = 0x032c;
pub const DM_ENDCONVERSATION: u32 = 0x032b;
pub const DM_RENDER: u32 = 0x0329;
pub const DM_RENDERCOMPLETE: u32 = 0x0328;
pub const DM_RENDERPREPARE: u32 = 0x0327;
pub const DM_DRAGOVERNOTIFY: u32 = 0x0321;
pub const DOR_NODROP: u32 = 0;
pub const DOR_DROP: u32 = 1;
pub const DOR_NODROPOP: u32 = 2;
pub const DOR_NEVERDROP: u32 = 3;
pub const DO_COPYABLE: u16 = 1;
pub const DO_MOVEABLE: u16 = 2;
pub const DO_LINKABLE: u16 = 4;
pub const DC_CONTAINER: u16 = 8;
pub const DO_DEFAULT: u32 = 0xbffe;
pub const DO_UNKNOWN: u32 = 0xbfff;
pub const DO_COPY: u32 = 0x10;
pub const DO_MOVE: u32 = 0x20;
pub const DO_LINK: u32 = 0x18;
pub const DO_CREATEPROGRAMOBJECT: u32 = 0x80;
pub const DMFL_TARGETSUCCESSFUL: u32 = 1;
pub const DMFL_TARGETFAIL: u32 = 2;
pub const DMFL_RENDEROK: u32 = 0x10;
pub const DMFL_RENDERFAIL: u32 = 0x20;
pub const DRG_ICON: u32 = 1;
pub const VK_ENDDRAG: u32 = 0x38;
pub const CN_DRAGLEAVE: u32 = 102;
pub const CN_DRAGOVER: u32 = 103;
pub const CN_DROP: u32 = 104;
pub const CN_DROPHELP: u32 = 105;
pub const CA_OWNERDRAW: u32 = 0x4000;
pub const WM_DRAWITEM: u32 = 0x0036;
pub const CRA_TARGET: u32 = 0x0002;
pub const CRA_DISABLED: u32 = 0x1000;
pub const CRA_EXPANDED: u32 = 0x0080;
pub const CRA_COLLAPSED: u32 = 0x0100;
pub const CMA_TEXT: u32 = 0x1000;

// DRAGITEM offsets.
pub const DI_HWND: usize = 0;
pub const DI_ID: usize = 4;
pub const DI_TYPE: usize = 8;
pub const DI_RMF: usize = 12;
pub const DI_CONTAINER: usize = 16;
pub const DI_SOURCE: usize = 20;
pub const DI_TARGET: usize = 24;
pub const DI_CONTROL: usize = 32;
pub const DI_OPS: usize = 34;
pub const DRAGITEM_SIZE: u32 = 36;

/// A string handle's text.
pub fn str_handle(h: u32) -> Vec<u8> {
    if h == 0 {
        return Vec::new();
    }
    let n = unsafe { DrgQueryStrNameLen(h) } as usize;
    let mut buf = alloc::vec![0u8; n + 1];
    let got = unsafe { DrgQueryStrName(h, buf.len() as u32, buf.as_mut_ptr()) } as usize;
    buf.truncate(got);
    buf
}

pub fn add_str(s: &[u8]) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let c = z(s);
    unsafe { DrgAddStrHandle(c.as_ptr()) }
}

/// RS2VIOSESSION
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VioSession {
    pub sid: u32,
    pub flags: u32,
    pub changes: u32,
    pub leader: u32,
    pub title: [u8; 64],
}

pub const VS_PM: u32 = 0x08;

pub const HWND_DESKTOP: u32 = 1;
pub const HWND_TOP: u32 = 3;
pub const HWND_BOTTOM: u32 = 4;
pub const HINI_USERPROFILE: u32 = 0xffff_ffff;
pub const SGH_ROOT: u32 = 0xffff_ffff;

// Window messages.
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_ENABLE: u32 = 0x0004;
pub const WM_SHOW: u32 = 0x0005;
pub const WM_MOVE: u32 = 0x0006;
pub const WM_SIZE: u32 = 0x0007;
pub const WM_ADJUSTWINDOWPOS: u32 = 0x0008;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_SYSCOMMAND: u32 = 0x0021;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_TIMER: u32 = 0x0024;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_QUIT: u32 = 0x002a;
pub const WM_CONTROL: u32 = 0x0030;
pub const WM_INITDLG: u32 = 0x003b;
pub const WM_SAVEAPPLICATION: u32 = 0x003e;
pub const WM_SETFOCUS: u32 = 0x000f;
pub const WM_ACTIVATE: u32 = 0x000d;
pub const WM_QUERYICON: u32 = 0x0048;
pub const WM_UPDATEFRAME: u32 = 0x0042;
pub const WM_MOUSEMOVE: u32 = 0x0070;
pub const WM_BUTTON1DOWN: u32 = 0x0071;
pub const WM_BUTTON1UP: u32 = 0x0072;
pub const WM_BUTTON2DOWN: u32 = 0x0074;
pub const WM_BUTTON2UP: u32 = 0x0075;
pub const WM_CHAR: u32 = 0x007a;
pub const WM_CONTEXTMENU: u32 = 0x0424;
pub const WM_USER: u32 = 0x1000;
pub const WM_RS2_WINLIST: u32 = 0x0ff1;
pub const WM_RS2_MINWINDOW: u32 = 0x0ff8;

// Frame creation flags and styles.
pub const FCF_TITLEBAR: u32 = 0x0001;
pub const FCF_SYSMENU: u32 = 0x0002;
pub const FCF_MENU: u32 = 0x0004;
pub const FCF_SIZEBORDER: u32 = 0x0008;
pub const FCF_MINBUTTON: u32 = 0x0010;
pub const FCF_MAXBUTTON: u32 = 0x0020;
pub const FCF_MINMAX: u32 = 0x0030;
pub const FCF_DLGBORDER: u32 = 0x0100;
pub const FCF_NOBYTEALIGN: u32 = 0x1000;
pub const FCF_TASKLIST: u32 = 0x0800;
pub const FCF_SHELLPOSITION: u32 = 0x0400;
pub const FID_CLIENT: u32 = 0x8008;
pub const FID_MENU: u32 = 0x8005;
pub const FID_TITLEBAR: u32 = 0x8002;

pub const WS_VISIBLE: u32 = 0x8000_0000;
pub const WS_DISABLED: u32 = 0x4000_0000;
pub const WS_CLIPCHILDREN: u32 = 0x2000_0000;
pub const WS_MINIMIZED: u32 = 0x0100_0000;
pub const WS_GROUP: u32 = 0x0001_0000;
pub const WS_TABSTOP: u32 = 0x0002_0000;

pub const SWP_SIZE: u32 = 0x0001;
pub const SWP_MOVE: u32 = 0x0002;
pub const SWP_ZORDER: u32 = 0x0004;
pub const SWP_SHOW: u32 = 0x0008;
pub const SWP_HIDE: u32 = 0x0010;
pub const SWP_ACTIVATE: u32 = 0x0080;
pub const SWP_MINIMIZE: u32 = 0x0400;
pub const SWP_MAXIMIZE: u32 = 0x0800;
pub const SWP_RESTORE: u32 = 0x1000;

pub const QWL_STYLE: i32 = -2;
pub const QWL_USER: i32 = 0;
pub const QW_NEXT: i32 = 0;
pub const QW_PREV: i32 = 1;
pub const QW_TOP: i32 = 2;
pub const QW_BOTTOM: i32 = 3;

pub const SV_CXSCREEN: i32 = 20;
pub const SV_CYSCREEN: i32 = 21;

pub const SPTR_ARROW: i32 = 1;
pub const SPTR_WAIT: i32 = 3;
pub const SPTR_APPICON: i32 = 10;
pub const SPTR_ILLEGAL: i32 = 18;
pub const SPTR_FILE: i32 = 19;
pub const SPTR_FOLDER: i32 = 20;
pub const SPTR_MULTFILE: i32 = 21;
pub const SPTR_PROGRAM: i32 = 22;

// Message boxes.
pub const MB_OK: u32 = 0x0000;
pub const MB_OKCANCEL: u32 = 0x0001;
pub const MB_YESNO: u32 = 0x0004;
pub const MB_ICONQUESTION: u32 = 0x0010;
pub const MB_WARNING: u32 = 0x0020;
pub const MB_INFORMATION: u32 = 0x0030;
pub const MB_ERROR: u32 = 0x0040;
pub const MB_MOVEABLE: u32 = 0x4000;
pub const MBID_OK: u32 = 1;
pub const MBID_YES: u32 = 6;

// Menus.
pub const MIS_TEXT: u16 = 0x0001;
pub const MIS_SUBMENU: u16 = 0x0010;
pub const MIS_SEPARATOR: u16 = 0x0004;
pub const MIA_CHECKED: u16 = 0x2000;
pub const MIA_DISABLED: u16 = 0x4000;
pub const MM_INSERTITEM: u32 = 0x0180;
pub const MM_SETITEMATTR: u32 = 0x0192;
pub const MIT_END: i16 = -1;
pub const PU_KEYBOARD: u32 = 0x0200;
pub const PU_MOUSEBUTTON1: u32 = 0x0040;
pub const PU_MOUSEBUTTON2: u32 = 0x0080;
pub const PU_HCONSTRAIN: u32 = 0x2000;
pub const PU_VCONSTRAIN: u32 = 0x4000;

// Dialogs and controls.
pub const DID_OK: u32 = 1;
pub const DID_CANCEL: u32 = 2;
pub const BM_QUERYCHECK: u32 = 0x0124;
pub const BM_SETCHECK: u32 = 0x0125;
pub const EM_SETTEXTLIMIT: u32 = 0x0143;
pub const BN_CLICKED: u32 = 1;

// Container.
pub const WC_CONTAINER: u32 = 0xffff_0025;
pub const WC_NOTEBOOK: u32 = 0xffff_0028;
pub const CCS_EXTENDSEL: u32 = 0x0001;
pub const CCS_SINGLESEL: u32 = 0x0004;
pub const CCS_AUTOPOSITION: u32 = 0x0008;
pub const CM_ALLOCDETAILFIELDINFO: u32 = 0x0330;
pub const CM_ALLOCRECORD: u32 = 0x0331;
pub const CM_ARRANGE: u32 = 0x0332;
pub const CM_FREERECORD: u32 = 0x0336;
pub const CM_INSERTDETAILFIELDINFO: u32 = 0x0338;
pub const CM_INSERTRECORD: u32 = 0x0339;
pub const CM_INVALIDATERECORD: u32 = 0x033b;
pub const CM_QUERYCNRINFO: u32 = 0x033d;
pub const CM_QUERYRECORD: u32 = 0x0340;
pub const CM_QUERYRECORDEMPHASIS: u32 = 0x0341;
pub const CM_QUERYRECORDRECT: u32 = 0x0343;
pub const CM_REMOVERECORD: u32 = 0x0346;
pub const CM_SETCNRINFO: u32 = 0x0349;
pub const CM_SETRECORDEMPHASIS: u32 = 0x034a;
pub const CM_SORTRECORD: u32 = 0x034b;
pub const CM_OPENEDIT: u32 = 0x034c;
pub const CM_CLOSEEDIT: u32 = 0x034d;
pub const CM_EXPANDTREE: u32 = 0x034f;
pub const CN_ENTER: u32 = 106;
pub const CN_INITDRAG: u32 = 107;
pub const CN_EMPHASIS: u32 = 108;
pub const CN_BEGINEDIT: u32 = 114;
pub const CN_ENDEDIT: u32 = 115;
pub const CN_REALLOCPSZ: u32 = 113;
pub const CN_CONTEXTMENU: u32 = 119;
pub const CN_EXPANDTREE: u32 = 117;
pub const CRA_SELECTED: u32 = 0x0001;
pub const CRA_CURSORED: u32 = 0x0004;
pub const CRA_INUSE: u32 = 0x0008;
pub const CRA_SOURCE: u32 = 0x4000;
pub const CMA_FIRST: u32 = 0x0010;
pub const CMA_END: u32 = 0x0040;
pub const CMA_NEXT: u32 = 0x0100;
pub const CMA_FREE: u32 = 0x0001;
pub const CMA_INVALIDATE: u32 = 0x0002;
pub const CMA_FLWINDOWATTR: u32 = 0x0004;
pub const CMA_CNRTITLE: u32 = 0x0001;
pub const CMA_TEXTCHANGED: u32 = 0x4000;
pub const CMA_REPOSITION: u32 = 0x0001;
pub const CMA_NOREPOSITION: u32 = 0x0002;
pub const CMA_ERASE: u32 = 0x0010;
pub const CV_TEXT: u32 = 0x0001;
pub const CV_NAME: u32 = 0x0002;
pub const CV_ICON: u32 = 0x0004;
pub const CV_DETAIL: u32 = 0x0008;
pub const CV_FLOW: u32 = 0x0010;
pub const CV_MINI: u32 = 0x0020;
pub const CV_TREE: u32 = 0x0040;
pub const CA_DETAILSVIEWTITLES: u32 = 0x8000;
pub const CA_TREELINE: u32 = 0x0040_0000;
pub const CFA_LEFT: u32 = 0x0001;
pub const CFA_RIGHT: u32 = 0x0002;
pub const CFA_VCENTER: u32 = 0x0010;
pub const CFA_BITMAPORICON: u32 = 0x0100;
pub const CFA_SEPARATOR: u32 = 0x0200;
pub const CFA_HORZSEPARATOR: u32 = 0x0400;
pub const CFA_STRING: u32 = 0x0800;
pub const CFA_DATE: u32 = 0x2000;
pub const CFA_TIME: u32 = 0x4000;
pub const CFA_FIREADONLY: u32 = 0x8000;
pub const CFA_ULONG: u32 = 0x0002_0000;

// RECORDCORE offsets, then the shell's own fields.
pub const R_CB: usize = 0;
pub const R_ATTR: usize = 4;
pub const R_X: usize = 8;
pub const R_Y: usize = 12;
pub const R_NEXT: usize = 16;
pub const R_PSZICON: usize = 20;
pub const R_HPTR: usize = 24;
pub const R_HPTRMINI: usize = 28;
pub const R_PSZTEXT: usize = 44;
pub const R_PSZNAME: usize = 48;
pub const R_PSZTREE: usize = 52;
/// The shell's item key.
pub const R_KEY: usize = 56;
pub const R_SIZE: usize = 60;
pub const R_DATE: usize = 64;
pub const R_TIME: usize = 68;
/// Bytes after the RECORDCORE.
pub const R_EXTRA: u32 = 16;

// Notebook.
pub const BKS_TABBEDDIALOG: u32 = 0x0800;
pub const BKM_INSERTPAGE: u32 = 0x0355;
pub const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
pub const BKM_SETTABTEXT: u32 = 0x0363;
pub const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
pub const BKA_MAJOR: u32 = 0x0040;
pub const BKA_LAST: u32 = 0x0002;
pub const BKA_AUTOPAGESIZE: u32 = 0x0100;
pub const BKA_STATUSTEXTON: u32 = 0x0001;

// Program list.
pub const PROG_DEFAULT: u32 = 0;
pub const PROG_FULLSCREEN: u32 = 1;
pub const PROG_WINDOWABLEVIO: u32 = 2;
pub const PROG_PM: u32 = 3;
pub const PROG_GROUP: u32 = 5;
pub const SHE_VISIBLE: u32 = 0;
pub const SHE_INVISIBLE: u32 = 1;

// Keys.
pub const KC_CHAR: u32 = 0x0001;
pub const KC_VIRTUALKEY: u32 = 0x0002;
pub const KC_KEYUP: u32 = 0x0040;
pub const KC_ALT: u32 = 0x0020;
pub const KC_CTRL: u32 = 0x0010;
pub const VK_DELETE: u32 = 0x1b;
pub const VK_F2: u32 = 0x21;
pub const VK_F5: u32 = 0x24;
pub const VK_NEWLINE: u32 = 0x08;
pub const VK_SHIFT: i32 = 9;
pub const VK_CTRL: i32 = 10;

// List boxes.
pub const WC_LISTBOX: u16 = 7;
pub const LM_INSERTITEM: u32 = 0x0161;
pub const LM_SELECTITEM: u32 = 0x0164;
pub const LM_QUERYSELECTION: u32 = 0x0165;
pub const LM_QUERYITEMTEXT: u32 = 0x0168;
pub const LM_DELETEALL: u32 = 0x016e;
pub const LM_QUERYITEMCOUNT: u32 = 0x0160;
pub const LN_SELECT: u32 = 1;
pub const LN_ENTER: u32 = 4;
pub const LIT_END: u32 = 0xffff;
pub const LIT_NONE: u32 = 0xffff;

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

pub fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

pub fn post(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinPostMsg(h, m, a, b) }
}

pub fn rd(p: u32, off: usize) -> u32 {
    unsafe { ((p as usize + off) as *const u32).read_unaligned() }
}

pub fn wr(p: u32, off: usize, v: u32) {
    unsafe { ((p as usize + off) as *mut u32).write_unaligned(v) };
}

/// A NUL-terminated copy.
pub fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}

/// The bytes of a NUL-terminated string.
pub fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// Bytes up to a NUL.
pub fn until_nul(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

pub fn window_text(h: u32) -> Vec<u8> {
    let mut buf = [0u8; 260];
    let n = unsafe { WinQueryWindowText(h, buf.len() as i32, buf.as_mut_ptr()) }.max(0) as usize;
    buf[..n.min(buf.len())].to_vec()
}

/// A frame's title (its text, else its title bar's).
pub fn frame_title(f: u32) -> Vec<u8> {
    let t = window_text(f);
    if !t.is_empty() {
        return t;
    }
    let tb = unsafe { WinWindowFromID(f, FID_TITLEBAR) };
    if tb != 0 { window_text(tb) } else { t }
}

pub fn item_text(dlg: u32, id: u32) -> Vec<u8> {
    let mut buf = [0u8; 300];
    let n = unsafe { WinQueryDlgItemText(dlg, id, buf.len() as i32, buf.as_mut_ptr()) } as usize;
    buf[..n.min(buf.len())].to_vec()
}

pub fn set_item_text(dlg: u32, id: u32, t: &[u8]) {
    let s = z(t);
    unsafe { WinSetDlgItemText(dlg, id, s.as_ptr()) };
}

/// A message box; returns the MBID_* of the button pressed.
pub fn message(owner: u32, text: &[u8], title: &[u8], style: u32) -> u32 {
    let (t, c) = (z(text), z(title));
    unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), 0, style | MB_MOVEABLE) }
}

pub fn screen() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) }
}

pub fn pointer_pos() -> PointL {
    let mut p = PointL::default();
    unsafe { WinQueryPointerPos(HWND_DESKTOP, &mut p) };
    p
}

pub fn window_pos(h: u32) -> Swp {
    let mut s = Swp::default();
    unsafe { WinQueryWindowPos(h, &mut s) };
    s
}

/// The process that owns a window (0 if it is gone).
pub fn window_pid(h: u32) -> u32 {
    let (mut pid, mut tid) = (0, 0);
    unsafe { WinQueryWindowProcess(h, &mut pid, &mut tid) };
    pid
}

// ---- Menus ----

/// One item of a menu built in memory.
pub enum Menu<'a> {
    Item(u16, &'a [u8], bool),
    Check(u16, &'a [u8], bool),
    Sep,
    Sub(&'a [u8], Vec<Menu<'a>>),
}

/// A pop-up menu window (on the desktop, hidden) from items.
pub fn popup_menu(items: &[Menu]) -> u32 {
    let tpl = menu_template(items);
    unsafe { WinCreateMenu(HWND_DESKTOP, tpl.as_ptr()) }
}

/// A menu template (the RT_MENU layout): cb, version, codepage, offset of
/// the items, count; each item style, attribute, id, text and, for a
/// submenu, its own template.
pub fn menu_template(items: &[Menu]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&850u16.to_le_bytes());
    out.extend_from_slice(&10u16.to_le_bytes());
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    for it in items {
        let (style, attr, id, text, sub): (u16, u16, u16, &[u8], Option<&Vec<Menu>>) = match it {
            Menu::Item(id, t, on) => (MIS_TEXT, if *on { 0 } else { MIA_DISABLED }, *id, t, None),
            Menu::Check(id, t, on) => (MIS_TEXT, if *on { MIA_CHECKED } else { 0 }, *id, t, None),
            Menu::Sep => (MIS_SEPARATOR, 0, 0xffff, b"", None),
            Menu::Sub(t, s) => (MIS_TEXT | MIS_SUBMENU, 0, 0xfff0, t, Some(s)),
        };
        out.extend_from_slice(&style.to_le_bytes());
        out.extend_from_slice(&attr.to_le_bytes());
        out.extend_from_slice(&id.to_le_bytes());
        if style & MIS_TEXT != 0 {
            out.extend_from_slice(text);
            out.push(0);
        }
        if let Some(s) = sub {
            out.extend_from_slice(&menu_template(s));
        }
    }
    let n = out.len() as u16;
    out[0..2].copy_from_slice(&n.to_le_bytes());
    out
}

// ---- Dialog templates ----

pub struct Item {
    pub class: u16,
    pub text: Vec<u8>,
    pub style: u32,
    pub x: i16,
    pub y: i16,
    pub cx: i16,
    pub cy: i16,
    pub id: u16,
    pub ctl: Vec<u8>,
}

pub fn item(class: u16, text: &[u8], style: u32, (x, y, cx, cy): (i16, i16, i16, i16), id: u16) -> Item {
    Item { class, text: text.to_vec(), style, x, y, cx, cy, id, ctl: Vec::new() }
}

pub const WC_FRAME: u16 = 1;
pub const WC_BUTTON: u16 = 3;
pub const WC_STATIC: u16 = 5;
pub const WC_ENTRYFIELD: u16 = 6;
pub const WC_NOTEBOOK_ID: u16 = 0x28;

/// A dialog frame with a title bar and system menu.
pub fn dialog_frame(title: &[u8], cx: i16, cy: i16) -> Item {
    // WS_CLIPSIBLINGS | WS_SAVEBITS | FS_DLGBORDER | FS_NOBYTEALIGN.
    let mut f = item(WC_FRAME, title, 0x1400_0000 | 0x0000_0080 | 0x0000_0010, (20, 20, cx, cy), 256);
    f.ctl = (FCF_TITLEBAR | FCF_SYSMENU).to_le_bytes().to_vec();
    f
}

/// Build a template: header (cbTemplate, type, codepage, offadlgti,
/// fsTemplateStatus, iItemFocus, coffPresParams), 30-byte items, then
/// the strings and control data.
pub fn template(frame: Item, children: Vec<Item>) -> Vec<u8> {
    let items: Vec<Item> = core::iter::once(frame).chain(children).collect();
    let n = items.len();
    let head = 14;
    let mut out = alloc::vec![0u8; head + 30 * n];
    let mut extra: Vec<u8> = Vec::new();
    let base = head + 30 * n;
    let put16 = |o: &mut Vec<u8>, at: usize, v: u16| o[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put16(&mut out, 2, 0);
    put16(&mut out, 4, 850);
    put16(&mut out, 6, head as u16);
    put16(&mut out, 8, 1);
    put16(&mut out, 10, 0xffff);
    put16(&mut out, 12, 0);
    for (i, it) in items.iter().enumerate() {
        let at = head + 30 * i;
        let children = if i == 0 { (n - 1) as u16 } else { 0 };
        let toff = base + extra.len();
        extra.extend_from_slice(&it.text);
        extra.push(0);
        let coff = if it.ctl.is_empty() {
            0xffff
        } else {
            while (base + extra.len()) % 4 != 0 {
                extra.push(0);
            }
            let c = base + extra.len();
            extra.extend_from_slice(&it.ctl);
            c as u16
        };
        put16(&mut out, at, 0);
        put16(&mut out, at + 2, children);
        put16(&mut out, at + 4, 0);
        put16(&mut out, at + 6, it.class);
        put16(&mut out, at + 8, it.text.len() as u16 + 1);
        put16(&mut out, at + 10, toff as u16);
        out[at + 12..at + 16].copy_from_slice(&it.style.to_le_bytes());
        put16(&mut out, at + 16, it.x as u16);
        put16(&mut out, at + 18, it.y as u16);
        put16(&mut out, at + 20, it.cx as u16);
        put16(&mut out, at + 22, it.cy as u16);
        put16(&mut out, at + 24, it.id);
        put16(&mut out, at + 26, 0xffff);
        put16(&mut out, at + 28, coff as u16);
    }
    out.extend_from_slice(&extra);
    let len = out.len() as u16;
    put16(&mut out, 0, len);
    out
}

/// Put a window in the middle of the screen.
pub fn center(h: u32) {
    let s = window_pos(h);
    let (sw, sh) = screen();
    unsafe { WinSetWindowPos(h, 0, ((sw - s.cx) / 2).max(0), ((sh - s.cy) / 2).max(0), 0, 0, SWP_MOVE) };
}

// ---- Icons from pictures ----

/// A PM icon (HPOINTER) from a 32 x 32 picture and its 16 x 16 mini
/// version: mask and colour bitmaps (GpiCreateBitmap) handed to
/// WinCreatePointerIndirect.
pub fn make_icon(big: &gpi::icon::Image, mini: &gpi::icon::Image) -> u32 {
    let (m1, c1) = bitmaps(big);
    let (m2, c2) = bitmaps(mini);
    // POINTERINFO: fPointer, xHotspot, yHotspot, hbmPointer, hbmColor,
    // hbmMiniPointer, hbmMiniColor.
    let pi: [u32; 7] = [0, 16, 16, m1, c1, m2, c2];
    let h = unsafe { WinCreatePointerIndirect(HWND_DESKTOP, pi.as_ptr()) };
    unsafe {
        for b in [m1, c1, m2, c2] {
            GpiDeleteBitmap(b);
        }
    }
    h
}

/// (mask bitmap, colour bitmap) of a picture.
fn bitmaps(img: &gpi::icon::Image) -> (u32, u32) {
    let (mask, colour) = img.to_bitmaps(true);
    let colour = colour.unwrap();
    (bitmap(&mask, 1), bitmap(&colour, 24))
}

/// A GPI bitmap from a gpi::Bitmap (top row first) at 1 or 24 bpp.
fn bitmap(b: &gpi::bitmap::Bitmap, bpp: u16) -> u32 {
    let (w, h) = (b.width, b.height);
    let row = ((w as usize * bpp as usize + 31) / 32) * 4;
    let mut bits = alloc::vec![0u8; row * h as usize];
    // Scan lines bottom-up.
    for y in 0..h {
        let src = &b.pixels[((h - 1 - y) * w) as usize..((h - y) * w) as usize];
        let dst = &mut bits[y as usize * row..(y as usize + 1) * row];
        for (x, &p) in src.iter().enumerate() {
            if bpp == 1 {
                if p & 0xff_ffff != 0 {
                    dst[x / 8] |= 0x80 >> (x % 8);
                }
            } else {
                dst[x * 3] = p as u8;
                dst[x * 3 + 1] = (p >> 8) as u8;
                dst[x * 3 + 2] = (p >> 16) as u8;
            }
        }
    }
    // BITMAPINFOHEADER2 (16 bytes used) for the bitmap; BITMAPINFO2 with
    // the colour table for 1 bpp (black, white).
    let mut hdr = [0u8; 16];
    hdr[0..4].copy_from_slice(&16u32.to_le_bytes());
    hdr[4..8].copy_from_slice(&(w as u32).to_le_bytes());
    hdr[8..12].copy_from_slice(&(h as u32).to_le_bytes());
    hdr[12..14].copy_from_slice(&1u16.to_le_bytes());
    hdr[14..16].copy_from_slice(&bpp.to_le_bytes());
    let mut info = hdr.to_vec();
    if bpp == 1 {
        info.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0]);
    }
    // CBM_INIT
    unsafe { GpiCreateBitmap(0, hdr.as_ptr(), 4, bits.as_ptr(), info.as_ptr()) }
}

unsafe extern "C" {
    #[link_name = "RS2API.4"]
    fn Rs2DebugTrace(psz: *const u8) -> u32;
}

/// A line on the serial port (debugging).
pub fn trace(s: &str) {
    let mut t = b"[shell] ".to_vec();
    t.extend_from_slice(s.as_bytes());
    t.push(0);
    unsafe { Rs2DebugTrace(t.as_ptr()) };
}
