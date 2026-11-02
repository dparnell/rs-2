//! The PMWIN, PMGPI, PMSHAPI, PMCTLS, HELPMGR, PMSPL and PMWP entry
//! points rs2's PM applets use, with their structures.

#![allow(non_snake_case)]

pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
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
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectL {
    pub l: i32,
    pub b: i32,
    pub r: i32,
    pub t: i32,
}

impl RectL {
    pub const fn new(l: i32, b: i32, r: i32, t: i32) -> RectL {
        RectL { l, b, r, t }
    }
    pub fn width(&self) -> i32 {
        self.r - self.l
    }
    pub fn height(&self) -> i32 {
        self.t - self.b
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.l && x < self.r && y >= self.b && y < self.t
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

pub const fn pt(x: i32, y: i32) -> PointL {
    PointL { x, y }
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct SizeL {
    pub cx: i32,
    pub cy: i32,
}

/// SWP
#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
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

/// BITMAPINFOHEADER2 (the first 16 bytes are enough for most uses; the
/// rest is zero).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BitmapInfoHeader2 {
    pub cb: u32,
    pub cx: u32,
    pub cy: u32,
    pub planes: u16,
    pub bit_count: u16,
    pub compression: u32,
    pub cb_image: u32,
    pub x_res: u32,
    pub y_res: u32,
    pub clr_used: u32,
    pub clr_important: u32,
    pub units: u16,
    pub reserved: u16,
    pub recording: u16,
    pub rendering: u16,
    pub size1: u32,
    pub size2: u32,
    pub color_encoding: u32,
    pub identifier: u32,
}

impl BitmapInfoHeader2 {
    pub fn new(cx: u32, cy: u32, bits: u16) -> Self {
        BitmapInfoHeader2 {
            cb: 64,
            cx,
            cy,
            planes: 1,
            bit_count: bits,
            compression: 0,
            cb_image: 0,
            x_res: 0,
            y_res: 0,
            clr_used: 0,
            clr_important: 0,
            units: 0,
            reserved: 0,
            recording: 0,
            rendering: 0,
            size1: 0,
            size2: 0,
            color_encoding: 0,
            identifier: 0,
        }
    }
}

/// FATTRS
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FAttrs {
    pub cb: u16,
    pub selection: u16,
    pub match_: i32,
    pub facename: [u8; 32],
    pub registry: u16,
    pub codepage: u16,
    pub max_baseline_ext: i32,
    pub ave_char_width: i32,
    pub type_: u16,
    pub quality: u16,
}

impl FAttrs {
    /// An outline font of that face (FATTR_FONTUSE_OUTLINE), sized
    /// later with GpiSetCharBox.
    pub fn outline(face: &[u8]) -> FAttrs {
        let mut f = FAttrs { cb: 56, selection: 0, match_: 0, facename: [0; 32], registry: 0, codepage: 0, max_baseline_ext: 0, ave_char_width: 0, type_: 0, quality: 0 };
        let n = face.len().min(31);
        f.facename[..n].copy_from_slice(&face[..n]);
        f.type_ = 0;
        f.quality = 0;
        f
    }
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

/// POINTERINFO
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct PointerInfo {
    pub pointer: u32,
    pub x_hotspot: i32,
    pub y_hotspot: i32,
    pub hbm_pointer: u32,
    pub hbm_color: u32,
    pub hbm_mini_pointer: u32,
    pub hbm_mini_color: u32,
}

/// SWCNTRL (a window list entry).
#[repr(C)]
pub struct SwCntrl {
    pub hwnd: u32,
    pub icon: u32,
    pub program: u32,
    pub pid: u32,
    pub session: u32,
    pub visibility: u32,
    pub jump: u32,
    pub title: [u8; 60],
    pub prog_type: u32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    pub fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.888"]
    pub fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.716"]
    pub fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    pub fn WinDestroyMsgQueue(hmq: u32) -> u32;
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
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    pub fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.743"]
    pub fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.731"]
    pub fn WinDrawBorder(hps: u32, r: *const RectL, cx: i32, cy: i32, fg: i32, bg: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.840"]
    pub fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.892"]
    pub fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMWIN.877"]
    pub fn WinSetWindowText(h: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.841"]
    pub fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.899"]
    pub fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.834"]
    pub fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.837"]
    pub fn WinQueryWindowPos(h: u32, swp: *mut Swp) -> u32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.775"]
    pub fn WinIsWindowVisible(h: u32) -> u32;
    #[link_name = "PMWIN.860"]
    pub fn WinSetFocus(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.817"]
    pub fn WinQueryFocus(desktop: u32) -> u32;
    #[link_name = "PMWIN.829"]
    pub fn WinQuerySysValue(desktop: u32, i: i32) -> i32;
    #[link_name = "PMWIN.826"]
    pub fn WinQuerySysColor(desktop: u32, i: i32, reserved: i32) -> i32;
    #[link_name = "PMWIN.828"]
    pub fn WinQuerySysPointer(desktop: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.866"]
    pub fn WinSetPointer(desktop: u32, ptr: u32) -> u32;
    #[link_name = "PMWIN.823"]
    pub fn WinQueryPointerPos(desktop: u32, p: *mut PointL) -> u32;
    #[link_name = "PMWIN.942"]
    pub fn WinCreatePointerIndirect(desktop: u32, info: *const PointerInfo) -> u32;
    #[link_name = "PMWIN.717"]
    pub fn WinCreatePointer(desktop: u32, hbm: u32, pointer: u32, x: i32, y: i32) -> u32;
    #[link_name = "PMWIN.727"]
    pub fn WinDestroyPointer(ptr: u32) -> u32;
    #[link_name = "PMWIN.822"]
    pub fn WinQueryPointerInfo(ptr: u32, info: *mut PointerInfo) -> u32;
    #[link_name = "PMWIN.732"]
    pub fn WinDrawPointer(hps: u32, x: i32, y: i32, ptr: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.730"]
    pub fn WinDrawBitmap(hps: u32, hbm: u32, src: *const RectL, dst: *const PointL, fg: i32, bg: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.852"]
    pub fn WinSetCapture(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.804"]
    pub fn WinQueryCapture(desktop: u32) -> u32;
    #[link_name = "PMWIN.788"]
    pub fn WinMapWindowPoints(from: u32, to: u32, p: *mut PointL, n: i32) -> u32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.735"]
    pub fn WinEnableWindow(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.773"]
    pub fn WinIsWindowEnabled(h: u32) -> u32;
    #[link_name = "PMWIN.907"]
    pub fn WinCreateMenu(parent: u32, tpl: *const u8) -> u32;
    #[link_name = "PMWIN.937"]
    pub fn WinPopupMenu(parent: u32, owner: u32, menu: u32, x: i32, y: i32, id: i32, fl: u32) -> u32;
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
    #[link_name = "PMWIN.844"]
    pub fn WinQueryWindowUShort(h: u32, i: i32) -> u16;
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
    #[link_name = "PMWIN.799"]
    pub fn WinQueryActiveWindow(desk: u32) -> u32;
    #[link_name = "PMWIN.705"]
    pub fn WinCancelShutdown(hmq: u32, always: u32) -> u32;
    #[link_name = "PMWIN.884"]
    pub fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    pub fn WinStopTimer(hab: u32, h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.750"]
    pub fn WinGetCurrentTime(hab: u32) -> u32;
    #[link_name = "PMWIN.793"]
    pub fn WinOpenClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.707"]
    pub fn WinCloseClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.733"]
    pub fn WinEmptyClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.854"]
    pub fn WinSetClipbrdData(hab: u32, data: u32, fmt: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.806"]
    pub fn WinQueryClipbrdData(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.739"]
    pub fn WinEnumClipbrdFmts(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.807"]
    pub fn WinQueryClipbrdFmtInfo(hab: u32, fmt: u32, info: *mut u32) -> u32;
    #[link_name = "PMWIN.856"]
    pub fn WinSetClipbrdViewer(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.809"]
    pub fn WinQueryClipbrdViewer(hab: u32) -> u32;
    #[link_name = "PMWIN.855"]
    pub fn WinSetClipbrdOwner(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.808"]
    pub fn WinQueryClipbrdOwner(hab: u32) -> u32;
    #[link_name = "PMWIN.849"]
    pub fn WinScrollWindow(h: u32, dx: i32, dy: i32, scroll: *const RectL, clip: *const RectL, rgn: u32, upd: *mut RectL, fl: u32) -> i32;
    #[link_name = "PMWIN.938"]
    pub fn WinSetPresParam(h: u32, id: u32, cb: u32, v: *const u8) -> u32;
    #[link_name = "PMWIN.939"]
    pub fn WinQueryPresParam(h: u32, id1: u32, id2: u32, found: *mut u32, cb: u32, v: *mut u8, fl: u32) -> u32;
    #[link_name = "PMWIN.700"]
    pub fn WinAddAtom(table: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.830"]
    pub fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.802"]
    pub fn WinQueryAtomName(table: u32, atom: u32, buf: *mut u8, cch: u32) -> u32;
    #[link_name = "PMWIN.846"]
    pub fn WinRegisterUserMsg(hab: u32, msg: u32, t1: i32, t2: i32, tr: i32) -> u32;
    #[link_name = "PMWIN.746"]
    pub fn WinFocusChange(desktop: u32, h: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.768"]
    pub fn WinIsChild(h: u32, parent: u32) -> u32;
    #[link_name = "PMWIN.715"]
    pub fn WinCreateCursor(h: u32, x: i32, y: i32, cx: i32, cy: i32, fs: u32, clip: *const RectL) -> u32;
    #[link_name = "PMWIN.725"]
    pub fn WinDestroyCursor(h: u32) -> u32;
    #[link_name = "PMWIN.880"]
    pub fn WinShowCursor(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.784"]
    pub fn WinLockWindowUpdate(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.736"]
    pub fn WinEnableWindowUpdate(h: u32, on: u32) -> u32;
    #[link_name = "PMWIN.863"]
    pub fn WinSetMultWindowPos(hab: u32, swp: *const Swp, n: u32) -> u32;
    #[link_name = "PMWIN.704"]
    pub fn WinCalcFrameRect(frame: u32, r: *mut RectL, client: u32) -> u32;
    #[link_name = "PMWIN.9014"]
    pub fn WinQuerySwitchHandle(h: u32, pid: u32) -> u32;
    #[link_name = "PMWIN.9012"]
    pub fn WinQuerySwitchEntry(sw: u32, ctl: *mut SwCntrl) -> u32;
    #[link_name = "PMWIN.9011"]
    pub fn WinChangeSwitchEntry(sw: u32, ctl: *const SwCntrl) -> u32;
    #[link_name = "PMWIN.800"]
    pub fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.710"]
    pub fn WinCopyRect(hab: u32, d: *mut RectL, s: *const RectL) -> u32;
    #[link_name = "PMWIN.901"]
    pub fn WinBroadcastMsg(h: u32, m: u32, a: u32, b: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.780"]
    pub fn WinLoadPointer(desktop: u32, hmod: u32, id: u32) -> u32;
    #[link_name = "PMWIN.872"]
    pub fn WinSetSysModalWindow(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.813"]
    pub fn WinQueryDesktopWindow(hab: u32, hdc: u32) -> u32;

    #[link_name = "PMCTLS.4"]
    pub fn WinFileDlg(parent: u32, owner: u32, fild: *mut u8) -> u32;
    #[link_name = "PMCTLS.5"]
    pub fn WinDefFileDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMCTLS.2"]
    pub fn WinFontDlg(parent: u32, owner: u32, fntd: *mut u8) -> u32;

    #[link_name = "HELPMGR.51"]
    pub fn WinCreateHelpInstance(hab: u32, init: *mut HelpInit) -> u32;
    #[link_name = "HELPMGR.52"]
    pub fn WinDestroyHelpInstance(h: u32) -> u32;
    #[link_name = "HELPMGR.54"]
    pub fn WinAssociateHelpInstance(help: u32, frame: u32) -> u32;

    #[link_name = "PMSHAPI.114"]
    pub fn PrfQueryProfileInt(hini: u32, app: *const u8, key: *const u8, default: i32) -> i32;
    #[link_name = "PMSHAPI.115"]
    pub fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
    #[link_name = "PMSHAPI.116"]
    pub fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
    #[link_name = "PMSHAPI.117"]
    pub fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    pub fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
    #[link_name = "PMSHAPI.127"]
    pub fn WinQueryTaskSizePos(hab: u32, pid: u32, swp: *mut Swp) -> u32;
    #[link_name = "PMSHAPI.119"]
    pub fn WinStartApp(notify: u32, pd: *const u8, params: *const u8, res: u32, fl: u32) -> u32;

    #[link_name = "PMWP.281"]
    pub fn WinCreateObject(class: *const u8, title: *const u8, setup: *const u8, location: *const u8, fl: u32) -> u32;

    #[link_name = "PMGPI.369"]
    pub fn GpiCreatePS(hab: u32, hdc: u32, size: *const SizeL, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    pub fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    pub fn GpiAssociate(hps: u32, hdc: u32) -> u32;
    #[link_name = "PMGPI.444"]
    pub fn GpiQueryDevice(hps: u32) -> u32;
    #[link_name = "PMGPI.498"]
    pub fn GpiResetPS(hps: u32, fl: u32) -> u32;
    #[link_name = "PMGPI.389"]
    pub fn GpiErase(hps: u32) -> u32;
    #[link_name = "PMGPI.517"]
    pub fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.504"]
    pub fn GpiSetBackColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.537"]
    pub fn GpiSetMix(hps: u32, m: i32) -> u32;
    #[link_name = "PMGPI.505"]
    pub fn GpiSetBackMix(hps: u32, m: i32) -> u32;
    #[link_name = "PMGPI.541"]
    pub fn GpiSetPattern(hps: u32, p: i32) -> u32;
    #[link_name = "PMGPI.530"]
    pub fn GpiSetLineType(hps: u32, t: i32) -> u32;
    #[link_name = "PMGPI.531"]
    pub fn GpiSetLineWidth(hps: u32, w: i32) -> u32;
    #[link_name = "PMGPI.532"]
    pub fn GpiSetLineWidthGeom(hps: u32, w: i32) -> u32;
    #[link_name = "PMGPI.592"]
    pub fn GpiCreateLogColorTable(hps: u32, opts: u32, fmt: i32, start: i32, n: i32, table: *const i32) -> u32;
    #[link_name = "PMGPI.439"]
    pub fn GpiQueryColorIndex(hps: u32, opts: u32, rgb: i32) -> i32;
    #[link_name = "PMGPI.479"]
    pub fn GpiQueryRGBColor(hps: u32, opts: u32, c: i32) -> i32;
    #[link_name = "PMGPI.404"]
    pub fn GpiMove(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.398"]
    pub fn GpiLine(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.415"]
    pub fn GpiPolyLine(hps: u32, n: i32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.356"]
    pub fn GpiBox(hps: u32, ctl: i32, p: *const PointL, h: i32, v: i32) -> i32;
    #[link_name = "PMGPI.544"]
    pub fn GpiSetPel(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.476"]
    pub fn GpiQueryPel(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.502"]
    pub fn GpiSetArcParams(hps: u32, p: *const i32) -> u32;
    #[link_name = "PMGPI.393"]
    pub fn GpiFullArc(hps: u32, ctl: i32, mult: i32) -> i32;
    #[link_name = "PMGPI.612"]
    pub fn GpiPartialArc(hps: u32, center: *const PointL, mult: i32, start: i32, sweep: i32) -> i32;
    #[link_name = "PMGPI.352"]
    pub fn GpiBeginArea(hps: u32, fl: u32) -> u32;
    #[link_name = "PMGPI.385"]
    pub fn GpiEndArea(hps: u32) -> i32;
    #[link_name = "PMGPI.359"]
    pub fn GpiCharStringAt(hps: u32, p: *const PointL, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.358"]
    pub fn GpiCharString(hps: u32, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.489"]
    pub fn GpiQueryTextBox(hps: u32, n: i32, s: *const u8, n2: i32, out: *mut PointL) -> u32;
    #[link_name = "PMGPI.453"]
    pub fn GpiQueryFontMetrics(hps: u32, len: i32, out: *mut u8) -> u32;
    #[link_name = "PMGPI.368"]
    pub fn GpiCreateLogFont(hps: u32, name: *const u8, id: i32, attrs: *const FAttrs) -> i32;
    #[link_name = "PMGPI.513"]
    pub fn GpiSetCharSet(hps: u32, id: i32) -> u32;
    #[link_name = "PMGPI.378"]
    pub fn GpiDeleteSetId(hps: u32, id: i32) -> u32;
    #[link_name = "PMGPI.510"]
    pub fn GpiSetCharBox(hps: u32, size: *const SizeL) -> u32;
    #[link_name = "PMGPI.598"]
    pub fn GpiCreateBitmap(hps: u32, bmp: *const BitmapInfoHeader2, opts: u32, init: *const u8, info: *const u8) -> u32;
    #[link_name = "PMGPI.371"]
    pub fn GpiDeleteBitmap(hbm: u32) -> u32;
    #[link_name = "PMGPI.506"]
    pub fn GpiSetBitmap(hps: u32, hbm: u32) -> u32;
    #[link_name = "PMGPI.599"]
    pub fn GpiQueryBitmapBits(hps: u32, start: i32, n: i32, bits: *mut u8, info: *mut u8) -> i32;
    #[link_name = "PMGPI.602"]
    pub fn GpiSetBitmapBits(hps: u32, start: i32, n: i32, bits: *const u8, info: *const u8) -> i32;
    #[link_name = "PMGPI.601"]
    pub fn GpiQueryBitmapInfoHeader(hbm: u32, info: *mut BitmapInfoHeader2) -> u32;
    #[link_name = "PMGPI.355"]
    pub fn GpiBitBlt(hps: u32, src: u32, n: i32, pts: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.557"]
    pub fn GpiWCBitBlt(hps: u32, hbm: u32, n: i32, pts: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.603"]
    pub fn GpiDrawBits(hps: u32, bits: *const u8, bmi: *const u8, n: i32, p: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.401"]
    pub fn GpiLoadMetaFile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMGPI.500"]
    pub fn GpiSaveMetaFile(hmf: u32, name: *const u8) -> u32;
    #[link_name = "PMGPI.375"]
    pub fn GpiDeleteMetaFile(hmf: u32) -> u32;
    #[link_name = "PMGPI.365"]
    pub fn GpiCopyMetaFile(hmf: u32) -> u32;
    #[link_name = "PMGPI.411"]
    pub fn GpiPlayMetaFile(hps: u32, hmf: u32, n: i32, opts: *const i32, segs: *mut i32, cdesc: i32, desc: *mut u8) -> i32;
    #[link_name = "PMGPI.466"]
    pub fn GpiQueryMetaFileLength(hmf: u32) -> i32;
    #[link_name = "PMGPI.465"]
    pub fn GpiQueryMetaFileBits(hmf: u32, off: i32, n: i32, buf: *mut u8) -> u32;
    #[link_name = "PMGPI.536"]
    pub fn GpiSetMetaFileBits(hmf: u32, off: i32, n: i32, buf: *const u8) -> u32;
    #[link_name = "PMGPI.471"]
    pub fn GpiQueryPS(hps: u32, size: *mut SizeL) -> u32;
    #[link_name = "PMGPI.540"]
    pub fn GpiSetPageViewport(hps: u32, r: *const RectL) -> u32;
    #[link_name = "PMGPI.472"]
    pub fn GpiQueryPageViewport(hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMGPI.370"]
    pub fn GpiCreateRegion(hps: u32, n: i32, r: *const RectL) -> u32;
    #[link_name = "PMGPI.611"]
    pub fn GpiDestroyRegion(hps: u32, rgn: u32) -> u32;
    #[link_name = "PMGPI.516"]
    pub fn GpiSetClipRegion(hps: u32, rgn: u32, old: *mut u32) -> i32;
    #[link_name = "PMGPI.396"]
    pub fn GpiIntersectClipRectangle(hps: u32, r: *const RectL) -> i32;
    #[link_name = "PMGPI.650"]
    pub fn GpiPolygons(hps: u32, n: u32, polys: *const u8, opts: u32, model: u32) -> i32;
    #[link_name = "PMGPI.441"]
    pub fn GpiQueryCurrentPosition(hps: u32, p: *mut PointL) -> u32;
    #[link_name = "PMGPI.364"]
    pub fn GpiConvert(hps: u32, src: i32, dst: i32, n: i32, p: *mut PointL) -> u32;
    #[link_name = "PMGPI.558"]
    pub fn GpiPolyLineDisjoint(hps: u32, n: i32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.412"]
    pub fn GpiPointArc(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.520"]
    pub fn GpiSetDefaultViewMatrix(hps: u32, n: i32, m: *const i32, ty: i32) -> u32;
    #[link_name = "PMGPI.509"]
    pub fn GpiSetCharAngle(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.649"]
    pub fn GpiSetTextAlignment(hps: u32, h: i32, v: i32) -> u32;
    #[link_name = "PMGPI.610"]
    pub fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    pub fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.605"]
    pub fn DevEscape(hdc: u32, code: i32, nin: i32, pin: *const u8, nout: *mut i32, pout: *mut u8) -> i32;
    #[link_name = "PMGPI.606"]
    pub fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32;

    #[link_name = "PMSPL.397"]
    pub fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;

    #[link_name = "RS2API.4"]
    pub fn Rs2DebugTrace(psz: *const u8) -> u32;
}

unsafe extern "C" {
    #[link_name = "PMWIN.794"]
    pub fn WinOpenWindowDC(h: u32) -> u32;
    #[link_name = "PMWIN.835"]
    pub fn WinQueryWindowDC(h: u32) -> u32;
    #[link_name = "PMWIN.865"]
    pub fn WinSetParent(h: u32, parent: u32, redraw: u32) -> u32;
    #[link_name = "PMWIN.864"]
    pub fn WinSetOwner(h: u32, owner: u32) -> u32;
    #[link_name = "PMWIN.895"]
    pub fn WinValidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.831"]
    pub fn WinQueryUpdateRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.851"]
    pub fn WinSetActiveWindow(desktop: u32, h: u32) -> u32;
    #[link_name = "PMWIN.900"]
    pub fn WinWindowFromPoint(parent: u32, p: *const PointL, children: u32) -> u32;
    #[link_name = "PMWIN.818"]
    pub fn WinQueryMsgPos(hab: u32, p: *mut PointL) -> u32;
    #[link_name = "PMWIN.867"]
    pub fn WinSetPointerPos(desktop: u32, x: i32, y: i32) -> u32;
    #[link_name = "PMWIN.752"]
    pub fn WinGetKeyState(desktop: u32, vk: i32) -> i32;
    #[link_name = "PMWIN.838"]
    pub fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.902"]
    pub fn WinPostQueueMsg(hmq: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.805"]
    pub fn WinQueryClassName(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMGPI.573"]
    pub fn GpiQueryBitmapParameters(hbm: u32, info: *mut u8) -> u32;
    #[link_name = "PMGPI.539"]
    pub fn GpiSetPS(hps: u32, size: *const SizeL, opts: u32) -> u32;
    #[link_name = "PMGPI.430"]
    pub fn GpiQueryCharBox(hps: u32, size: *mut SizeL) -> u32;
    #[link_name = "PMGPI.400"]
    pub fn GpiLoadFonts(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMGPI.586"]
    pub fn GpiQueryFonts(hps: u32, opts: u32, face: *const u8, req: *mut i32, cb: i32, metrics: *mut u8) -> i32;
}
