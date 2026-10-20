//! The PMWIN, PMCTLS, HELPMGR, PMSHAPI and DOSCALLS entry points the
//! editor uses, their constants, and small helpers (strings, menu and
//! dialog templates made in memory).

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
#[derive(Default, Clone, Copy)]
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
    #[link_name = "PMWIN.899"]
    pub fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.837"]
    pub fn WinQueryWindowPos(h: u32, swp: *mut Swp) -> u32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.860"]
    pub fn WinSetFocus(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.829"]
    pub fn WinQuerySysValue(desktop: u32, i: i32) -> i32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.735"]
    pub fn WinEnableWindow(h: u32, on: u32) -> u32;
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
    #[link_name = "PMWIN.929"]
    pub fn WinSubclassWindow(h: u32, p: WndProc) -> u32;
    #[link_name = "PMWIN.913"]
    pub fn WinDrawText(hps: u32, cch: i32, text: *const u8, r: *mut RectL, fg: i32, bg: i32, fl: u32) -> i32;
    #[link_name = "PMWIN.701"]
    pub fn WinAlarm(desk: u32, kind: u32) -> u32;
    #[link_name = "PMWIN.713"]
    pub fn WinCreateAccelTable(hab: u32, tbl: *const u8) -> u32;
    #[link_name = "PMWIN.850"]
    pub fn WinSetAccelTable(hab: u32, haccel: u32, frame: u32) -> u32;
    #[link_name = "PMWIN.807"]
    pub fn WinQueryClipbrdFmtInfo(hab: u32, fmt: u32, info: *mut u32) -> u32;
    #[link_name = "PMWIN.799"]
    pub fn WinQueryActiveWindow(desk: u32) -> u32;
    #[link_name = "PMCTLS.4"]
    pub fn WinFileDlg(parent: u32, owner: u32, fild: *mut u8) -> u32;
    #[link_name = "PMCTLS.2"]
    pub fn WinFontDlg(parent: u32, owner: u32, fntd: *mut u8) -> u32;
    #[link_name = "HELPMGR.51"]
    pub fn WinCreateHelpInstance(hab: u32, init: *mut HelpInit) -> u32;
    #[link_name = "HELPMGR.52"]
    pub fn WinDestroyHelpInstance(h: u32) -> u32;
    #[link_name = "HELPMGR.54"]
    pub fn WinAssociateHelpInstance(help: u32, frame: u32) -> u32;
    #[link_name = "PMWIN.705"]
    pub fn WinCancelShutdown(hmq: u32, always: u32) -> u32;
    #[link_name = "PMWIN.841"]
    pub fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.842"]
    pub fn WinQueryWindowTextLength(h: u32) -> i32;
    #[link_name = "PMCTLS.5"]
    pub fn WinDefFileDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMCTLS.3"]
    pub fn WinDefFontDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMSHAPI.114"]
    pub fn PrfQueryProfileInt(hini: u32, app: *const u8, key: *const u8, default: i32) -> i32;
    #[link_name = "PMSHAPI.116"]
    pub fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
    #[link_name = "PMSHAPI.127"]
    pub fn WinQueryTaskSizePos(hab: u32, pid: u32, swp: *mut Swp) -> u32;
    #[link_name = "DOSCALLS.228"]
    pub fn DosSearchPath(flags: u32, path: *const u8, name: *const u8, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "DOSCALLS.286"]
    pub fn DosBeep(freq: u32, ms: u32) -> u32;
    #[link_name = "PMSHAPI.117"]
    pub fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    pub fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
}

/// HELPINIT
#[repr(C)]
pub struct HelpInit {
    pub cb: u32,
    pub ret: u32,
    pub tutorial: *const u8,
    pub table: *const u8,
    pub table_module: u32,
    pub accel_module: u32,
    pub accel_id: u32,
    pub action_bar_id: u32,
    pub title: *const u8,
    pub show_panel_id: u32,
    pub library: *const u8,
}

pub const HWND_DESKTOP: u32 = 1;
pub const HINI_USERPROFILE: u32 = 0xffff_ffff;

// Window messages.
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0007;
pub const WM_SETFOCUS: u32 = 0x000f;
pub const WM_ACTIVATE: u32 = 0x000d;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_HELP: u32 = 0x0022;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_TIMER: u32 = 0x0024;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_QUIT: u32 = 0x002a;
pub const WM_CONTROL: u32 = 0x0030;
pub const WM_INITMENU: u32 = 0x0033;
pub const WM_INITDLG: u32 = 0x003b;
pub const WM_SAVEAPPLICATION: u32 = 0x003e;
pub const WM_UPDATEFRAME: u32 = 0x0042;
pub const WM_USER: u32 = 0x1000;

pub const HM_MSG_BASE: u32 = 0x0220;
pub const HM_DISPLAY_HELP: u32 = HM_MSG_BASE + 0x02;
pub const HM_EXT_HELP: u32 = HM_MSG_BASE + 0x03;
pub const HM_HELP_INDEX: u32 = HM_MSG_BASE + 0x0a;
pub const HM_KEYS_HELP: u32 = HM_MSG_BASE + 0x0c;
pub const HM_ERROR: u32 = HM_MSG_BASE + 0x0e;
pub const HM_QUERY_KEYS_HELP: u32 = HM_MSG_BASE + 0x10;
pub const HM_RESOURCEID: u32 = 0;

// Frame creation flags and styles.
pub const FCF_TITLEBAR: u32 = 0x0001;
pub const FCF_SYSMENU: u32 = 0x0002;
pub const FCF_MENU: u32 = 0x0004;
pub const FCF_SIZEBORDER: u32 = 0x0008;
pub const FCF_MINMAX: u32 = 0x0030;
pub const FCF_DLGBORDER: u32 = 0x0100;
pub const FCF_TASKLIST: u32 = 0x0800;
pub const FCF_SHELLPOSITION: u32 = 0x0400;
pub const FCF_NOBYTEALIGN: u32 = 0x1000;
pub const FID_CLIENT: u32 = 0x8008;
pub const FID_MENU: u32 = 0x8005;

pub const WS_VISIBLE: u32 = 0x8000_0000;
pub const WS_DISABLED: u32 = 0x4000_0000;
pub const WS_CLIPCHILDREN: u32 = 0x2000_0000;
pub const WS_GROUP: u32 = 0x0001_0000;
pub const WS_TABSTOP: u32 = 0x0002_0000;

pub const SWP_SIZE: u32 = 0x0001;
pub const SWP_MOVE: u32 = 0x0002;
pub const SWP_ZORDER: u32 = 0x0004;
pub const SWP_SHOW: u32 = 0x0008;
pub const SWP_ACTIVATE: u32 = 0x0080;
pub const HWND_TOP: u32 = 3;

pub const SV_CXSCREEN: i32 = 20;
pub const SV_CYSCREEN: i32 = 21;

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
pub const MBID_RETRY: u32 = 4;
pub const MB_RETRYCANCEL: u32 = 0x0002;

// Menus.
pub const MIS_TEXT: u16 = 0x0001;
pub const MIS_SUBMENU: u16 = 0x0010;
pub const MIS_SEPARATOR: u16 = 0x0004;
pub const MIA_CHECKED: u16 = 0x2000;
pub const MIA_DISABLED: u16 = 0x4000;
pub const MM_SETITEMATTR: u32 = 0x0192;
pub const MM_SETITEMTEXT: u32 = 0x018e;

// Accelerators.
pub const AF_CHAR: u16 = 0x0001;
pub const AF_VIRTUALKEY: u16 = 0x0002;
pub const AF_CONTROL: u16 = 0x0010;
pub const AF_SHIFT: u16 = 0x0008;
pub const VK_F3: u16 = 0x22;

// Dialogs and controls.
pub const DID_OK: u32 = 1;
pub const DID_CANCEL: u32 = 2;
pub const BM_QUERYCHECK: u32 = 0x0124;
pub const BM_SETCHECK: u32 = 0x0125;
pub const BN_CLICKED: u32 = 1;
pub const EM_SETTEXTLIMIT: u32 = 0x0143;
pub const EM_SETSEL: u32 = 0x0142;
pub const SPBN_CHANGE: u32 = 0x020d;
pub const SPBM_SETTEXTLIMIT: u32 = 0x0202;
pub const EN_CHANGE: u32 = 0x0004;
pub const LM_INSERTITEM: u32 = 0x0161;
pub const LM_SELECTITEM: u32 = 0x0164;
pub const LM_QUERYSELECTION: u32 = 0x0165;
pub const LIT_END: u32 = 0xffff;
pub const LIT_FIRST: u32 = 0xffff_fffd;
pub const LN_SELECT: u32 = 1;
pub const LN_ENTER: u32 = 5;
pub const SPBM_SETLIMITS: u32 = 0x0207;
pub const SPBM_SETCURRENTVALUE: u32 = 0x0208;
pub const SPBM_QUERYVALUE: u32 = 0x0205;
pub const SPBQ_ALWAYSUPDATE: u32 = 1;
pub const SPBS_NUMERICONLY: u32 = 0x0001;
pub const SPBS_MASTER: u32 = 0x0010;
pub const SPBS_JUSTRIGHT: u32 = 0x0004;

pub const WC_FRAME: u16 = 1;
pub const WC_BUTTON: u16 = 3;
pub const WC_STATIC: u16 = 5;
pub const WC_ENTRYFIELD: u16 = 6;
pub const WC_LISTBOX: u16 = 7;
pub const WC_MLE: u32 = 0xffff_000a;
pub const WC_SPINBUTTON: u16 = 0x20;

pub const BS_PUSHBUTTON: u32 = 0;
pub const BS_CHECKBOX: u32 = 1;
pub const BS_AUTOCHECKBOX: u32 = 2;
pub const BS_HELP: u32 = 0x0100;
pub const BS_DEFAULT: u32 = 0x0400;
pub const SS_TEXT: u32 = 0x0001;
pub const SS_GROUPBOX: u32 = 0x0002;
pub const SS_SYSICON: u32 = 0x000b;
pub const DT_LEFT: u32 = 0x0000;
pub const DT_CENTER: u32 = 0x0100;
pub const DT_VCENTER: u32 = 0x0400;
pub const DT_WORDBREAK: u32 = 0x4000;
pub const DT_MNEMONIC: u32 = 0x2000;
pub const ES_MARGIN: u32 = 0x0008;
pub const ES_AUTOSCROLL: u32 = 0x0004;

// The MLE.
pub const MLS_WORDWRAP: u32 = 0x0001;
pub const MLS_BORDER: u32 = 0x0002;
pub const MLS_VSCROLL: u32 = 0x0004;
pub const MLS_HSCROLL: u32 = 0x0008;
pub const MLM_SETTEXTLIMIT: u32 = 0x01b0;
pub const MLM_SETWRAP: u32 = 0x01b4;
pub const MLM_QUERYCHANGED: u32 = 0x01ba;
pub const MLM_SETCHANGED: u32 = 0x01bb;
pub const MLM_QUERYTEXTLENGTH: u32 = 0x01c0;
pub const MLM_FORMAT: u32 = 0x01c1;
pub const MLM_SETIMPORTEXPORT: u32 = 0x01c2;
pub const MLM_IMPORT: u32 = 0x01c3;
pub const MLM_EXPORT: u32 = 0x01c4;
pub const MLM_DELETE: u32 = 0x01c6;
pub const MLM_QUERYFORMATTEXTLENGTH: u32 = 0x01c8;
pub const MLM_INSERT: u32 = 0x01c9;
pub const MLM_SETSEL: u32 = 0x01ca;
pub const MLM_QUERYSEL: u32 = 0x01cb;
pub const MLM_QUERYSELTEXT: u32 = 0x01cc;
pub const MLM_QUERYUNDO: u32 = 0x01cd;
pub const MLM_UNDO: u32 = 0x01ce;
pub const MLM_RESETUNDO: u32 = 0x01cf;
pub const MLM_QUERYFONT: u32 = 0x01d0;
pub const MLM_SETFONT: u32 = 0x01d1;
pub const MLM_SETTEXTCOLOR: u32 = 0x01d2;
pub const MLM_SETBACKCOLOR: u32 = 0x01d4;
pub const MLM_CUT: u32 = 0x01d8;
pub const MLM_COPY: u32 = 0x01d9;
pub const MLM_PASTE: u32 = 0x01da;
pub const MLM_CLEAR: u32 = 0x01db;
pub const MLM_ENABLEREFRESH: u32 = 0x01dc;
pub const MLM_DISABLEREFRESH: u32 = 0x01dd;
pub const MLM_SEARCH: u32 = 0x01de;
pub const MLN_CHANGE: u32 = 0x0007;
pub const MLN_OVERFLOW: u32 = 0x0001;
pub const MLM_SETTABSTOP: u32 = 0x01b6;
pub const MLFIE_CFTEXT: u32 = 0;
pub const MLFSEARCH_CASESENSITIVE: u32 = 0x0001;
pub const MLFSEARCH_SELECTMATCH: u32 = 0x0002;
pub const MLFSEARCH_CHANGEALL: u32 = 0x0004;
pub const MLFQS_MINSEL: u32 = 1;
pub const MLFQS_MAXSEL: u32 = 2;
pub const MLFQS_CURSORSEL: u32 = 4;

pub const CF_TEXT: u32 = 1;

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

pub fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

pub fn post(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinPostMsg(h, m, a, b) }
}

/// A NUL-terminated copy.
pub fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}

/// Bytes up to a NUL.
pub fn until_nul(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

pub fn window_text(h: u32) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 300];
    let n = unsafe { WinQueryWindowText(h, buf.len() as i32, buf.as_mut_ptr()) }.max(0) as usize;
    buf.truncate(n.min(300));
    buf
}

pub fn item_text(dlg: u32, id: u32) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 1024];
    let n = unsafe { WinQueryDlgItemText(dlg, id, buf.len() as i32, buf.as_mut_ptr()) } as usize;
    buf.truncate(n.min(1024));
    buf
}

pub fn set_item_text(dlg: u32, id: u32, t: &[u8]) {
    let s = z(t);
    unsafe { WinSetDlgItemText(dlg, id, s.as_ptr()) };
}

pub fn checked(dlg: u32, id: u32) -> bool {
    unsafe { WinSendDlgItemMsg(dlg, id, BM_QUERYCHECK, 0, 0) != 0 }
}

pub fn set_check(dlg: u32, id: u32, on: bool) {
    unsafe { WinSendDlgItemMsg(dlg, id, BM_SETCHECK, on as u32, 0) };
}

pub fn enable(dlg: u32, id: u32, on: bool) {
    unsafe { WinEnableWindow(WinWindowFromID(dlg, id), on as u32) };
}

/// A message box; returns the MBID_* of the button pressed.
pub fn message(owner: u32, text: &[u8], title: &[u8], style: u32) -> u32 {
    let (t, c) = (z(text), z(title));
    unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), 0, style | MB_MOVEABLE) }
}

pub fn screen() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) }
}

pub fn window_pos(h: u32) -> Swp {
    let mut s = Swp::default();
    unsafe { WinQueryWindowPos(h, &mut s) };
    s
}

/// Put a dialog over its owner (centred on it, kept on the screen).
pub fn place_over(dlg: u32, owner: u32) {
    let (d, o) = (window_pos(dlg), window_pos(owner));
    let (sw, sh) = screen();
    let x = (o.x + (o.cx - d.cx) / 2).clamp(0, (sw - d.cx).max(0));
    let y = (o.y + (o.cy - d.cy) * 2 / 3).clamp(0, (sh - d.cy).max(0));
    unsafe { WinSetWindowPos(dlg, 0, x, y, 0, 0, SWP_MOVE) };
}

// ---- Menus ----

/// One item of a menu built in memory.
pub enum Menu<'a> {
    Item(u16, &'a [u8]),
    Check(u16, &'a [u8], bool),
    Sep,
    Sub(u16, &'a [u8], Vec<Menu<'a>>),
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
            Menu::Item(id, t) => (MIS_TEXT, 0, *id, t, None),
            Menu::Check(id, t, on) => (MIS_TEXT, if *on { MIA_CHECKED } else { 0 }, *id, t, None),
            Menu::Sep => (MIS_SEPARATOR, 0, 0xffff, b"", None),
            Menu::Sub(id, t, s) => (MIS_TEXT | MIS_SUBMENU, 0, *id, t, Some(s)),
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

/// Set or clear a menu item attribute (searching submenus).
pub fn menu_attr(menu: u32, id: u16, attr: u16, on: bool) {
    send(menu, MM_SETITEMATTR, mp(id as u32, 1), mp(attr as u32, if on { attr as u32 } else { 0 }));
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

/// A push button (WS_TABSTOP; the first of a group has WS_GROUP).
pub fn button(text: &[u8], (x, y, cx): (i16, i16, i16), id: u32, extra: u32) -> Item {
    item(WC_BUTTON, text, WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON | extra, (x, y, cx, 14), id as u16)
}

/// Static text.
pub fn label(text: &[u8], r: (i16, i16, i16, i16), extra: u32) -> Item {
    item(WC_STATIC, text, WS_VISIBLE | SS_TEXT | extra, r, 0xffff)
}

/// A dialog frame with a title bar and system menu; `id` is the dialog's
/// id (its help subtable).
pub fn dialog_frame(title: &[u8], id: u16, cx: i16, cy: i16) -> Item {
    // WS_CLIPSIBLINGS | WS_SAVEBITS | FS_DLGBORDER | FS_NOBYTEALIGN.
    let mut f = item(WC_FRAME, title, 0x1400_0000 | 0x0000_0080 | 0x0000_0010, (20, 20, cx, cy), id);
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

/// Run a modal dialog made from a template; returns its result.
pub fn run_dialog(owner: u32, proc_: WndProc, tpl: &[u8], param: u32) -> u32 {
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, proc_ as *const () as u32, tpl.as_ptr(), param) };
    if dlg == 0 {
        return DID_CANCEL;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    r
}

unsafe extern "C" {
    #[link_name = "RS2API.4"]
    fn Rs2DebugTrace(psz: *const u8) -> u32;
}

/// A line on the serial port (the self-check reads them).
pub fn trace(s: &[u8]) {
    let mut t = b"[e] ".to_vec();
    t.extend_from_slice(s);
    t.push(0);
    unsafe { Rs2DebugTrace(t.as_ptr()) };
}
