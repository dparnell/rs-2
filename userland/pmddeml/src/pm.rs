//! The PMWIN and DOSCALLS calls PMDDEML uses, and PM's DDE constants.

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
    pub fs: u32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.926"]
    pub fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.909"]
    pub fn WinCreateWindow(parent: u32, class: *const u8, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    pub fn WinDefWindowProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.929"]
    pub fn WinSubclassWindow(h: u32, proc_: WndProc) -> Option<WndProc>;
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.919"]
    pub fn WinPostMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.702"]
    pub fn WinBeginEnumWindows(h: u32) -> u32;
    #[link_name = "PMWIN.756"]
    pub fn WinGetNextWindow(e: u32) -> u32;
    #[link_name = "PMWIN.737"]
    pub fn WinEndEnumWindows(e: u32) -> u32;
    #[link_name = "PMWIN.838"]
    pub fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.839"]
    pub fn WinQueryWindowPtr(h: u32, i: i32) -> u32;
    #[link_name = "PMWIN.876"]
    pub fn WinSetWindowPtr(h: u32, i: i32, p: u32) -> u32;
    #[link_name = "PMWIN.805"]
    pub fn WinQueryClassName(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.772"]
    pub fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.918"]
    pub fn WinPeekMsg(hab: u32, m: *mut QMsg, h: u32, first: u32, last: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.912"]
    pub fn WinDispatchMsg(hab: u32, m: *const QMsg) -> u32;
    #[link_name = "PMWIN.905"]
    pub fn WinCallMsgFilter(hab: u32, m: *const QMsg, f: u32) -> u32;
    #[link_name = "PMWIN.750"]
    pub fn WinGetCurrentTime(hab: u32) -> u32;
    #[link_name = "PMWIN.977"]
    pub fn WinSetErrorInfo(e: u32, fs: u32) -> u32;
    #[link_name = "PMWIN.824"]
    pub fn WinQueryQueueInfo(hmq: u32, p: *mut u8, cb: u32) -> u32;
    #[link_name = "PMWIN.810"]
    pub fn WinQueryCp(hmq: u32) -> u32;
    #[link_name = "PMWIN.893"]
    pub fn WinUpper(hab: u32, cp: u32, cc: u32, psz: *mut u8) -> u32;
    #[link_name = "PMWIN.712"]
    pub fn WinCpTranslateString(hab: u32, from: u32, src: *const u8, to: u32, cb: u32, dst: *mut u8) -> u32;
    #[link_name = "PMWIN.719"]
    pub fn WinDdePostMsg(to: u32, from: u32, wm: u32, dde: *mut u8, fl: u32) -> u32;
    #[link_name = "PMWIN.720"]
    pub fn WinDdeRespond(client: u32, server: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32;
    #[link_name = "PMWIN.830"]
    pub fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.700"]
    pub fn WinAddAtom(t: u32, psz: *const u8) -> u32;
    #[link_name = "PMWIN.721"]
    pub fn WinDeleteAtom(t: u32, a: u32) -> u32;
    #[link_name = "PMWIN.802"]
    pub fn WinQueryAtomName(t: u32, a: u32, buf: *mut u8, cch: u32) -> u32;
    #[link_name = "PMWIN.834"]
    pub fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "DOSCALLS.303"]
    pub fn DosGiveSharedMem(pb: *mut u8, pid: u32, flags: u32) -> u32;
}

pub const HWND_DESKTOP: u32 = 1;
pub const HMQ_CURRENT: u32 = 1;
pub const QW_PARENT: i32 = 5;
pub const WS_DISABLED: u32 = 0x4000_0000;
pub const FID_CLIENT: u32 = 0x8008;

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_QUIT: u32 = 0x002a;
pub const WM_DDE_INITIATE: u32 = 0xa0;
pub const WM_DDE_REQUEST: u32 = 0xa1;
pub const WM_DDE_ACK: u32 = 0xa2;
pub const WM_DDE_DATA: u32 = 0xa3;
pub const WM_DDE_ADVISE: u32 = 0xa4;
pub const WM_DDE_UNADVISE: u32 = 0xa5;
pub const WM_DDE_POKE: u32 = 0xa6;
pub const WM_DDE_EXECUTE: u32 = 0xa7;
pub const WM_DDE_TERMINATE: u32 = 0xa8;
pub const WM_DDE_INITIATEACK: u32 = 0xa9;

pub const DDE_FACK: u16 = 0x0001;
pub const DDE_FBUSY: u16 = 0x0002;
pub const DDE_FNODATA: u16 = 0x0004;
pub const DDE_FACKREQ: u16 = 0x0008;
pub const DDE_FRESPONSE: u16 = 0x0010;
pub const DDE_NOTPROCESSED: u16 = 0x0020;

pub const DDEPM_RETRY: u32 = 1;
pub const DDEPM_NOFREE: u32 = 2;

/// PAG_COMMIT | PAG_WRITE | PAG_READ | OBJ_GIVEABLE.
pub const GIVEABLE: u32 = 0x213;
