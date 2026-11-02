//! The PMWIN, PMGPI, HELPMGR, PMCTLS, PMSHAPI, REXX and REXXAPI entry
//! points PMREXXIO uses, with their constants and structures.

#![allow(dead_code, non_snake_case, clippy::too_many_arguments)]

pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

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

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

/// RXSTRING
#[repr(C)]
pub struct RxString {
    pub len: u32,
    pub ptr: *mut u8,
}

/// MLE_SEARCHDATA
#[repr(C)]
pub struct MleSearch {
    pub cb: u16,
    pub find: *const u8,
    pub replace: *const u8,
    pub cch_find: i16,
    pub cch_replace: i16,
    pub start: i32,
    pub stop: i32,
    pub cch_found: u16,
}

/// HELPINIT
#[repr(C)]
pub struct HelpInit {
    pub cb: u32,
    pub ret: u32,
    pub tutorial: *const u8,
    pub table: *const u16,
    pub table_module: u32,
    pub accel_module: u32,
    pub accel_id: u32,
    pub action_bar_id: u32,
    pub title: *const u8,
    pub show_panel_id: u32,
    pub library: *const u8,
}

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    pub fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.926"]
    pub fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    pub fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    pub fn WinCreateWindow(parent: u32, class: u32, text: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    pub fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.910"]
    pub fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    pub fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.903"]
    pub fn WinSendDlgItemMsg(h: u32, id: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.899"]
    pub fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.834"]
    pub fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.839"]
    pub fn WinQueryWindowPtr(h: u32, i: i32) -> usize;
    #[link_name = "PMWIN.876"]
    pub fn WinSetWindowPtr(h: u32, i: i32, p: usize) -> u32;
    #[link_name = "PMWIN.844"]
    pub fn WinQueryWindowUShort(h: u32, i: i32) -> u16;
    #[link_name = "PMWIN.879"]
    pub fn WinSetWindowUShort(h: u32, i: i32, v: u16) -> u32;
    #[link_name = "PMWIN.837"]
    pub fn WinQueryWindowPos(h: u32, swp: *mut Swp) -> u32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.877"]
    pub fn WinSetWindowText(h: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.841"]
    pub fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.736"]
    pub fn WinEnableWindowUpdate(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(h: u32, r: *const u8, children: u32) -> u32;
    #[link_name = "PMWIN.860"]
    pub fn WinSetFocus(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.929"]
    pub fn WinSubclassWindow(h: u32, p: WndProc) -> usize;
    #[link_name = "PMWIN.884"]
    pub fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    pub fn WinStopTimer(hab: u32, h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    pub fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.701"]
    pub fn WinAlarm(desk: u32, kind: u32) -> u32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.922"]
    pub fn WinCreateDlg(parent: u32, owner: u32, proc_: WndProc, tpl: *const u8, params: usize) -> u32;
    #[link_name = "PMWIN.796"]
    pub fn WinProcessDlg(h: u32) -> u32;
    #[link_name = "PMWIN.729"]
    pub fn WinDismissDlg(h: u32, result: u32) -> u32;
    #[link_name = "PMWIN.907"]
    pub fn WinCreateMenu(parent: u32, tpl: *const u8) -> u32;

    #[link_name = "PMGPI.453"]
    pub fn GpiQueryFontMetrics(hps: u32, cb: i32, fm: *mut u8) -> u32;
    #[link_name = "PMGPI.359"]
    pub fn GpiCharStringAt(hps: u32, pt: *const PointL, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.441"]
    pub fn GpiQueryCurrentPosition(hps: u32, pt: *mut PointL) -> u32;
    #[link_name = "PMGPI.586"]
    pub fn GpiQueryFonts(hps: u32, opts: u32, face: *const u8, n: *mut i32, cb: i32, fm: *mut u8) -> i32;
    #[link_name = "PMGPI.368"]
    pub fn GpiCreateLogFont(hps: u32, name: *const u8, lcid: i32, fat: *const u8) -> i32;
    #[link_name = "PMGPI.444"]
    pub fn GpiQueryDevice(hps: u32) -> u32;
    #[link_name = "PMGPI.606"]
    pub fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32;

    #[link_name = "PMGPI.598"]
    pub fn GpiCreateBitmap(hps: u32, hdr: *const u8, init: u32, bits: *const u8, info: *const u8) -> u32;
    #[link_name = "PMGPI.371"]
    pub fn GpiDeleteBitmap(hbm: u32) -> u32;
    #[link_name = "PMWIN.942"]
    pub fn WinCreatePointerIndirect(desk: u32, pi: *const u32) -> u32;

    #[link_name = "HELPMGR.51"]
    pub fn WinCreateHelpInstance(hab: u32, init: *mut HelpInit) -> u32;
    #[link_name = "HELPMGR.52"]
    pub fn WinDestroyHelpInstance(h: u32) -> u32;
    #[link_name = "HELPMGR.54"]
    pub fn WinAssociateHelpInstance(help: u32, frame: u32) -> u32;

    #[link_name = "PMCTLS.2"]
    pub fn WinFontDlg(parent: u32, owner: u32, fntd: *mut u8) -> u32;

    #[link_name = "PMSHAPI.117"]
    pub fn PrfQueryProfileData(ini: u32, app: *const u8, key: *const u8, buf: *mut u8, cb: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    pub fn PrfWriteProfileData(ini: u32, app: *const u8, key: *const u8, buf: *const u8, cb: u32) -> u32;

    #[link_name = "REXX.3"]
    pub fn RexxSetHalt(pid: u32, tid: u32) -> u32;
    #[link_name = "REXX.4"]
    pub fn RexxSetTrace(pid: u32, tid: u32) -> u32;
    #[link_name = "REXX.5"]
    pub fn RexxResetTrace(pid: u32, tid: u32) -> u32;
    #[link_name = "REXXAPI.12"]
    pub fn RexxRegisterExitExe(name: *const u8, entry: usize, user: *const u8) -> u32;
    #[link_name = "REXXAPI.13"]
    pub fn RexxDeregisterExit(name: *const u8, dll: *const u8) -> u32;
    #[link_name = "REXXAPI.14"]
    pub fn RexxQueryExit(name: *const u8, dll: *const u8, flag: *mut u16, user: *mut u8) -> u32;
}

pub const HWND_DESKTOP: u32 = 1;
pub const HWND_TOP: u32 = 3;
pub const QW_PARENT: i32 = 5;
pub const FID_MENU: u32 = 0x8005;
pub const FID_TITLEBAR: u32 = 0x8003;
pub const FID_CLIENT: u32 = 0x8008;
pub const HINI_USERPROFILE: u32 = 0xffff_ffff;

pub const WM_CREATE: u32 = 0x0001;
pub const WM_SIZE: u32 = 0x0007;
pub const WM_SETFOCUS: u32 = 0x000f;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_TIMER: u32 = 0x0024;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_INITDLG: u32 = 0x003b;
pub const WM_SAVEAPPLICATION: u32 = 0x003e;
pub const WM_UPDATEFRAME: u32 = 0x0042;
pub const WM_SETICON: u32 = 0x0047;
pub const WM_ERASEBACKGROUND: u32 = 0x004f;
pub const WM_BUTTON1DBLCLK: u32 = 0x0073;
pub const WM_BUTTON2DBLCLK: u32 = 0x0076;
pub const WM_CHAR: u32 = 0x007a;
pub const WM_USER: u32 = 0x1000;

pub const KC_VIRTUALKEY: u32 = 0x0002;
pub const KC_KEYUP: u32 = 0x0040;
pub const VK_BREAK: u32 = 0x04;
pub const VK_TAB: u32 = 0x06;
pub const VK_BACKTAB: u32 = 0x07;
pub const VK_NEWLINE: u32 = 0x08;
pub const VK_UP: u32 = 0x16;
pub const VK_DELETE: u32 = 0x1b;
pub const VK_ENTER: u32 = 0x1e;

pub const FCF_TITLEBAR: u32 = 0x0001;
pub const FCF_SYSMENU: u32 = 0x0002;
pub const FCF_MENU: u32 = 0x0004;
pub const FCF_SIZEBORDER: u32 = 0x0008;
pub const FCF_MINBUTTON: u32 = 0x0010;
pub const FCF_MAXBUTTON: u32 = 0x0020;
pub const FCF_SHELLPOSITION: u32 = 0x0400;
pub const FCF_TASKLIST: u32 = 0x0800;
pub const CS_SIZEREDRAW: u32 = 0x0004;
pub const WS_VISIBLE: u32 = 0x8000_0000;
pub const WS_GROUP: u32 = 0x0001_0000;
pub const WS_TABSTOP: u32 = 0x0002_0000;

pub const SWP_SIZE: u32 = 0x0001;
pub const SWP_MOVE: u32 = 0x0002;
pub const SWP_ZORDER: u32 = 0x0004;
pub const SWP_SHOW: u32 = 0x0008;
pub const SWP_MINIMIZE: u32 = 0x0400;
pub const SWP_MAXIMIZE: u32 = 0x0800;
pub const QWS_XRESTORE: i32 = 0x0c;
pub const QWS_YRESTORE: i32 = 0x0e;
pub const QWS_CXRESTORE: i32 = 0x10;
pub const QWS_CYRESTORE: i32 = 0x12;

pub const WC_FRAME: u32 = 0xffff_0001;
pub const WC_BUTTON: u32 = 0xffff_0003;
pub const WC_STATIC: u32 = 0xffff_0005;
pub const WC_ENTRYFIELD: u32 = 0xffff_0006;
pub const WC_MLE: u32 = 0xffff_000a;

pub const MB_OK: u32 = 0x0000;
pub const MB_CANCEL: u32 = 0x0006;
pub const MB_ENTER: u32 = 0x0007;
pub const MB_INFORMATION: u32 = 0x0030;
pub const MB_CUAWARNING: u32 = 0x0020;
pub const MB_CUACRITICAL: u32 = 0x0040;
pub const MB_MOVEABLE: u32 = 0x4000;
pub const WA_ERROR: u32 = 2;

pub const MM_SETITEMATTR: u32 = 0x0192;
pub const MIA_CHECKED: u32 = 0x2000;
pub const MIA_DISABLED: u32 = 0x4000;
pub const MIS_TEXT: u16 = 0x0001;
pub const MIS_SUBMENU: u16 = 0x0010;
pub const MIS_SEPARATOR: u16 = 0x0004;

pub const EM_SETSEL: u32 = 0x0142;
pub const EM_SETTEXTLIMIT: u32 = 0x0143;
pub const EM_CUT: u32 = 0x0144;
pub const EM_COPY: u32 = 0x0145;
pub const EM_CLEAR: u32 = 0x0146;
pub const EM_PASTE: u32 = 0x0147;
pub const EM_SETFIRSTCHAR: u32 = 0x0149;
pub const ES_AUTOSCROLL: u32 = 0x0004;
pub const ES_MARGIN: u32 = 0x0008;
pub const BS_AUTOCHECKBOX: u32 = 0x0002;
pub const BS_HELP: u32 = 0x0100;
pub const BS_DEFAULT: u32 = 0x0400;
pub const BS_NOPOINTERFOCUS: u32 = 0x0800;
pub const BM_QUERYCHECK: u32 = 0x0124;
pub const SS_TEXT: u32 = 0x0001;

pub const MLS_BORDER: u32 = 0x0002;
pub const MLS_VSCROLL: u32 = 0x0004;
pub const MLS_HSCROLL: u32 = 0x0008;
pub const MLM_SETREADONLY: u32 = 0x01b8;
pub const MLM_QUERYTEXTLENGTH: u32 = 0x01c0;
pub const MLM_FORMAT: u32 = 0x01c1;
pub const MLM_SETIMPORTEXPORT: u32 = 0x01c2;
pub const MLM_IMPORT: u32 = 0x01c3;
pub const MLM_EXPORT: u32 = 0x01c4;
pub const MLM_INSERT: u32 = 0x01c9;
pub const MLM_SETSEL: u32 = 0x01ca;
pub const MLM_QUERYSEL: u32 = 0x01cb;
pub const MLM_RESETUNDO: u32 = 0x01cf;
pub const MLM_SETFONT: u32 = 0x01d1;
pub const MLM_CUT: u32 = 0x01d8;
pub const MLM_COPY: u32 = 0x01d9;
pub const MLM_CLEAR: u32 = 0x01db;
pub const MLM_SEARCH: u32 = 0x01de;
pub const MLFIE_CFTEXT: u32 = 0;
pub const MLFIE_NOTRANS: u32 = 1;
pub const MLFSEARCH_CASESENSITIVE: u32 = 1;
pub const MLFSEARCH_SELECTMATCH: u32 = 2;

pub const HM_DISPLAY_HELP: u32 = 0x0222;
pub const HM_EXT_HELP: u32 = 0x0223;
pub const HM_HELP_INDEX: u32 = 0x022a;
pub const HM_KEYS_HELP: u32 = 0x022c;
pub const HM_QUERY_KEYS_HELP: u32 = 0x0230;
pub const CMIC_HIDE_PANEL_ID: u32 = 0;

pub const DID_OK: u32 = 1;
pub const FNTS_CENTER: u32 = 0x0001;
pub const FNTS_HELPBUTTON: u32 = 0x0008;
pub const FATTR_FONTUSE_OUTLINE: u16 = 0x0004;
pub const CAPS_HORIZONTAL_FONT_RES: i32 = 37;
pub const CAPS_VERTICAL_FONT_RES: i32 = 38;
pub const QF_PUBLIC: u32 = 1;

/// REXX exits: the I/O exit and its subfunctions.
pub const RXSIO: i32 = 5;
pub const RXSIOTRD: i32 = 3;
pub const RXSIODTR: i32 = 4;
pub const RXEXIT_HANDLED: i32 = 0;
pub const RXEXIT_NOT_HANDLED: i32 = 1;
pub const RXEXIT_RAISE_ERROR: i32 = -1;

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

pub fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

/// Post, retrying while the queue is full (as IBM's does).
pub fn post_hard(h: u32, m: u32, a: u32, b: u32, wait: u32) {
    while unsafe { WinPostMsg(h, m, a, b) } == 0 {
        unsafe { os2::DosSleep(wait) };
    }
}
