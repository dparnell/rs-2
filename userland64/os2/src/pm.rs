//! Presentation Manager for 64-bit programs (docs/API64.md): the 64-bit
//! PMWIN (userland64/pmwin), which calls rs2's 32-bit PMWIN. Window and
//! dialog procedures are `extern "C" fn(HWND, ULONG, MPARAM, MPARAM) ->
//! MRESULT` with 64-bit MPARAM and MRESULT; QMSG is the 64-bit layout.
//! Only part of the API is declared here: PMWIN's 64-bit DLL exports all
//! of the 32-bit one's functions by name (`PMWIN.Name`).

#![allow(non_snake_case, clippy::missing_safety_doc)]

pub type HAB = u32;
pub type HMQ = u32;
pub type HWND = u32;
pub type HPS = u32;
pub type MPARAM = u64;
pub type MRESULT = u64;
pub type BOOL = u32;
pub type PFNWP = unsafe extern "C" fn(HWND, u32, MPARAM, MPARAM) -> MRESULT;

pub const HWND_DESKTOP: HWND = 1;
pub const HWND_TOP: HWND = 3;

pub const WS_VISIBLE: u32 = 0x8000_0000;
pub const WS_CLIPCHILDREN: u32 = 0x2000_0000;
pub const CS_SIZEREDRAW: u32 = 0x0004;

pub const FCF_TITLEBAR: u32 = 0x0001;
pub const FCF_SYSMENU: u32 = 0x0002;
pub const FCF_MENU: u32 = 0x0004;
pub const FCF_SIZEBORDER: u32 = 0x0008;
pub const FCF_MINMAX: u32 = 0x0030;
pub const FCF_SHELLPOSITION: u32 = 0x0400;
pub const FCF_TASKLIST: u32 = 0x0800;

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0007;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_TIMER: u32 = 0x0024;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_QUIT: u32 = 0x002a;
pub const WM_BUTTON1DOWN: u32 = 0x0071;
pub const WM_CHAR: u32 = 0x007a;
pub const WM_INITDLG: u32 = 0x003b;
pub const WM_USER: u32 = 0x1000;

pub const FID_MENU: u32 = 0x8005;
pub const DID_OK: u32 = 1;

pub const QWP_PFNWP: i32 = -3;
pub const QWL_USER: i32 = 0;

pub const CLR_WHITE: i32 = -2;
pub const CLR_BLACK: i32 = -1;
pub const CLR_BLUE: i32 = 1;
pub const CLR_RED: i32 = 2;
pub const CLR_GREEN: i32 = 4;
pub const CLR_CYAN: i32 = 5;
pub const CLR_YELLOW: i32 = 6;
pub const CLR_DARKBLUE: i32 = 9;
pub const CLR_PALEGRAY: i32 = 15;

pub const DRO_FILL: u32 = 1;
pub const DRO_OUTLINE: u32 = 2;
pub const DRO_OUTLINEFILL: u32 = 3;

pub const MB_OK: u32 = 0x0000;
pub const MB_INFORMATION: u32 = 0x0030;
pub const MB_MOVEABLE: u32 = 0x4000;

pub const SWP_SIZE: u32 = 0x0001;
pub const SWP_MOVE: u32 = 0x0002;
pub const SWP_ZORDER: u32 = 0x0004;
pub const SWP_SHOW: u32 = 0x0008;
pub const SWP_ACTIVATE: u32 = 0x0080;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct POINTL {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct RECTL {
    pub xLeft: i32,
    pub yBottom: i32,
    pub xRight: i32,
    pub yTop: i32,
}

/// QMSG, 64-bit layout (40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct QMSG {
    pub hwnd: HWND,
    pub msg: u32,
    pub mp1: MPARAM,
    pub mp2: MPARAM,
    pub time: u32,
    pub ptl: POINTL,
    pub reserved: u32,
}

/// CREATESTRUCT (WM_CREATE's mp2), 64-bit layout.
#[repr(C)]
#[derive(Debug)]
pub struct CREATESTRUCT {
    pub pPresParams: *mut u8,
    pub pCtlData: *mut u8,
    pub id: u32,
    pub hwndInsertBehind: HWND,
    pub hwndOwner: HWND,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub flStyle: u32,
    pub pszText: *const u8,
    pub pszClass: *const u8,
    pub hwndParent: HWND,
}

/// CLASSINFO, 64-bit layout.
#[repr(C)]
pub struct CLASSINFO {
    pub flClassStyle: u32,
    pub pfnWindowProc: Option<PFNWP>,
    pub cbWindowData: u32,
}

pub fn mpfrom2short(a: u16, b: u16) -> MPARAM {
    (a as u32 | (b as u32) << 16) as MPARAM
}

pub fn short1frommp(mp: MPARAM) -> u16 {
    mp as u16
}

pub fn short2frommp(mp: MPARAM) -> u16 {
    (mp >> 16) as u16
}

unsafe extern "C" {
    #[link_name = "PMWIN.WinInitialize"]
    pub fn WinInitialize(fl: u32) -> HAB;
    #[link_name = "PMWIN.WinTerminate"]
    pub fn WinTerminate(hab: HAB) -> BOOL;
    #[link_name = "PMWIN.WinCreateMsgQueue"]
    pub fn WinCreateMsgQueue(hab: HAB, cmsg: i32) -> HMQ;
    #[link_name = "PMWIN.WinDestroyMsgQueue"]
    pub fn WinDestroyMsgQueue(hmq: HMQ) -> BOOL;
    #[link_name = "PMWIN.WinRegisterClass"]
    pub fn WinRegisterClass(hab: HAB, class: *const u8, pfn: PFNWP, style: u32, cb: u32) -> BOOL;
    #[link_name = "PMWIN.WinQueryClassInfo"]
    pub fn WinQueryClassInfo(hab: HAB, class: *const u8, pci: *mut CLASSINFO) -> BOOL;
    #[link_name = "PMWIN.WinCreateStdWindow"]
    pub fn WinCreateStdWindow(
        parent: HWND,
        style: u32,
        pfl: *mut u32,
        class: *const u8,
        title: *const u8,
        client_style: u32,
        hmod: u32,
        id: u32,
        phwnd_client: *mut HWND,
    ) -> HWND;
    #[link_name = "PMWIN.WinCreateWindow"]
    pub fn WinCreateWindow(
        parent: HWND,
        class: *const u8,
        name: *const u8,
        style: u32,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        owner: HWND,
        behind: HWND,
        id: u32,
        ctl: *mut u8,
        pres: *mut u8,
    ) -> HWND;
    #[link_name = "PMWIN.WinDestroyWindow"]
    pub fn WinDestroyWindow(hwnd: HWND) -> BOOL;
    #[link_name = "PMWIN.WinGetMsg"]
    pub fn WinGetMsg(hab: HAB, pq: *mut QMSG, hwnd: HWND, first: u32, last: u32) -> BOOL;
    #[link_name = "PMWIN.WinPeekMsg"]
    pub fn WinPeekMsg(hab: HAB, pq: *mut QMSG, hwnd: HWND, first: u32, last: u32, fl: u32) -> BOOL;
    #[link_name = "PMWIN.WinDispatchMsg"]
    pub fn WinDispatchMsg(hab: HAB, pq: *const QMSG) -> MRESULT;
    #[link_name = "PMWIN.WinPostMsg"]
    pub fn WinPostMsg(hwnd: HWND, msg: u32, mp1: MPARAM, mp2: MPARAM) -> BOOL;
    #[link_name = "PMWIN.WinSendMsg"]
    pub fn WinSendMsg(hwnd: HWND, msg: u32, mp1: MPARAM, mp2: MPARAM) -> MRESULT;
    #[link_name = "PMWIN.WinDefWindowProc"]
    pub fn WinDefWindowProc(hwnd: HWND, msg: u32, mp1: MPARAM, mp2: MPARAM) -> MRESULT;
    #[link_name = "PMWIN.WinSubclassWindow"]
    pub fn WinSubclassWindow(hwnd: HWND, pfn: PFNWP) -> Option<PFNWP>;
    #[link_name = "PMWIN.WinQueryWindowPtr"]
    pub fn WinQueryWindowPtr(hwnd: HWND, index: i32) -> *mut u8;
    #[link_name = "PMWIN.WinSetWindowPtr"]
    pub fn WinSetWindowPtr(hwnd: HWND, index: i32, p: *mut u8) -> BOOL;
    #[link_name = "PMWIN.WinQueryWindow"]
    pub fn WinQueryWindow(hwnd: HWND, cmd: i32) -> HWND;
    #[link_name = "PMWIN.WinQueryWindowRect"]
    pub fn WinQueryWindowRect(hwnd: HWND, prcl: *mut RECTL) -> BOOL;
    #[link_name = "PMWIN.WinQueryWindowText"]
    pub fn WinQueryWindowText(hwnd: HWND, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.WinSetWindowText"]
    pub fn WinSetWindowText(hwnd: HWND, text: *const u8) -> BOOL;
    #[link_name = "PMWIN.WinSetWindowPos"]
    pub fn WinSetWindowPos(hwnd: HWND, behind: HWND, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> BOOL;
    #[link_name = "PMWIN.WinInvalidateRect"]
    pub fn WinInvalidateRect(hwnd: HWND, prcl: *const RECTL, children: BOOL) -> BOOL;
    #[link_name = "PMWIN.WinBeginPaint"]
    pub fn WinBeginPaint(hwnd: HWND, hps: HPS, prcl: *mut RECTL) -> HPS;
    #[link_name = "PMWIN.WinEndPaint"]
    pub fn WinEndPaint(hps: HPS) -> BOOL;
    #[link_name = "PMWIN.WinFillRect"]
    pub fn WinFillRect(hps: HPS, prcl: *const RECTL, color: i32) -> BOOL;
    #[link_name = "PMWIN.WinStartTimer"]
    pub fn WinStartTimer(hab: HAB, hwnd: HWND, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.WinStopTimer"]
    pub fn WinStopTimer(hab: HAB, hwnd: HWND, id: u32) -> BOOL;
    #[link_name = "PMWIN.WinMessageBox"]
    pub fn WinMessageBox(parent: HWND, owner: HWND, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.WinWindowFromID"]
    pub fn WinWindowFromID(parent: HWND, id: u32) -> HWND;
    #[link_name = "PMWIN.WinLoadString"]
    pub fn WinLoadString(hab: HAB, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.WinLoadDlg"]
    pub fn WinLoadDlg(parent: HWND, owner: HWND, pfn: PFNWP, hmod: u32, id: u32, params: *mut u8) -> HWND;
    #[link_name = "PMWIN.WinDlgBox"]
    pub fn WinDlgBox(parent: HWND, owner: HWND, pfn: PFNWP, hmod: u32, id: u32, params: *mut u8) -> u32;
    #[link_name = "PMWIN.WinDefDlgProc"]
    pub fn WinDefDlgProc(hwnd: HWND, msg: u32, mp1: MPARAM, mp2: MPARAM) -> MRESULT;
    #[link_name = "PMWIN.WinDismissDlg"]
    pub fn WinDismissDlg(hwnd: HWND, result: u32) -> BOOL;
    #[link_name = "PMWIN.WinQuerySysValue"]
    pub fn WinQuerySysValue(hwnd: HWND, sv: i32) -> i32;

    #[link_name = "PMWIN.GpiSetColor"]
    pub fn GpiSetColor(hps: HPS, color: i32) -> BOOL;
    #[link_name = "PMWIN.GpiMove"]
    pub fn GpiMove(hps: HPS, pptl: *const POINTL) -> BOOL;
    #[link_name = "PMWIN.GpiLine"]
    pub fn GpiLine(hps: HPS, pptl: *const POINTL) -> i32;
    #[link_name = "PMWIN.GpiBox"]
    pub fn GpiBox(hps: HPS, ctl: u32, pptl: *const POINTL, hround: i32, vround: i32) -> i32;
    #[link_name = "PMWIN.GpiCharStringAt"]
    pub fn GpiCharStringAt(hps: HPS, pptl: *const POINTL, cch: i32, text: *const u8) -> i32;
    #[link_name = "PMWIN.GpiFullArc"]
    pub fn GpiFullArc(hps: HPS, ctl: u32, mult: i32) -> i32;
}
