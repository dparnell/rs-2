//! The PM calls PMXTEST uses.

#![allow(non_snake_case, dead_code)]

#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq, Debug)]
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
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
    pub top: i32,
}

pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

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
    pub fn WinCreateWindow(parent: u32, class: *const u8, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    pub fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    pub fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.902"]
    pub fn WinPostQueueMsg(hmq: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.918"]
    pub fn WinPeekMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.915"]
    pub fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    pub fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.928"]
    pub fn WinSetHook(hab: u32, hmq: u32, kind: u32, pfn: u32, hmod: u32) -> u32;
    #[link_name = "PMWIN.927"]
    pub fn WinReleaseHook(hab: u32, hmq: u32, kind: u32, pfn: u32, hmod: u32) -> u32;
    #[link_name = "PMWIN.905"]
    pub fn WinCallMsgFilter(hab: u32, q: *mut QMsg, msgf: u32) -> u32;
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    pub fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.857"]
    pub fn WinSetCp(hmq: u32, cp: u32) -> u32;
    #[link_name = "PMWIN.810"]
    pub fn WinQueryCp(hmq: u32) -> u32;
    #[link_name = "PMWIN.913"]
    pub fn WinDrawText(hps: u32, n: i32, s: *const u8, r: *mut RectL, fg: i32, bg: i32, fl: u32) -> i32;
    #[link_name = "PMWIN.777"]
    pub fn WinLoadLibrary(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.722"]
    pub fn WinDeleteLibrary(hab: u32, hlib: u32) -> u32;
    #[link_name = "PMWIN.986"]
    pub fn WinLoadProcedure(hab: u32, hlib: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.846"]
    pub fn WinRegisterUserMsg(hab: u32, msg: u32, t1: u32, d1: u32, t2: u32, d2: u32, tr: u32) -> u32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.884"]
    pub fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    pub fn WinStopTimer(hab: u32, h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.799"]
    pub fn WinQueryActiveWindow(desk: u32) -> u32;
    #[link_name = "PMWIN.729"]
    pub fn WinDismissDlg(h: u32, r: u32) -> u32;
    #[link_name = "PMWIN.860"]
    pub fn WinSetFocus(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.817"]
    pub fn WinQueryFocus(desk: u32) -> u32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.851"]
    pub fn WinSetActiveWindow(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
}

pub const HWND_DESKTOP: u32 = 1;
pub const WM_USER: u32 = 0x1000;
pub const WM_TIMER: u32 = 0x0024;
pub const WM_CHAR: u32 = 0x007a;
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
