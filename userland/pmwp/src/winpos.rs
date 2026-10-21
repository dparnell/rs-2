//! WinStoreWindowPos and WinRestoreWindowPos: a window's position, size
//! and state kept in OS2.INI under the application's name and key, in the
//! Workplace Shell's record (`pmwin::winpos`). Presentation parameters are
//! not stored (rs2's PMWIN keeps none to store); those of a record OS/2
//! wrote are skipped.

use alloc::vec;
use pmwin::winpos::WinPos;

unsafe extern "C" {
    #[link_name = "PMWIN.837"]
    fn WinQueryWindowPos(h: u32, swp: *mut u32) -> u32;
    #[link_name = "PMWIN.844"]
    fn WinQueryWindowUShort(h: u32, index: i32) -> u16;
    #[link_name = "PMWIN.843"]
    fn WinQueryWindowULong(h: u32, index: i32) -> u32;
    #[link_name = "PMWIN.879"]
    fn WinSetWindowUShort(h: u32, index: i32, v: u16) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.829"]
    fn WinQuerySysValue(desk: u32, i: i32) -> i32;
    #[link_name = "PMWIN.799"]
    fn WinQueryActiveWindow(desk: u32) -> u32;
    #[link_name = "PMWIN.838"]
    fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.874"]
    fn WinSetWindowBits(h: u32, index: i32, v: u32, mask: u32) -> u32;
    #[link_name = "PMWIN.883"]
    fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.772"]
    fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.805"]
    fn WinQueryClassName(h: u32, cb: i32, buf: *mut u8) -> i32;
    #[link_name = "PMSHAPI.101"]
    fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.117"]
    fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, buf: *const u8, len: u32) -> u32;
}

const HINI_USERPROFILE: u32 = 0xffff_ffff;
const HWND_DESKTOP: u32 = 1;
const HWND_TOP: u32 = 3;
const QWL_STYLE: i32 = -2;
const QWS_XRESTORE: i32 = 0x0c;
const QWS_XMINIMIZE: i32 = 0x14;
const FS_SHELLPOSITION: u32 = 0x4;
const FS_SIZEBORDER: u32 = 0x800;
const SWP_SHOW: u32 = 0x8;
const SWP_HIDE: u32 = 0x10;
const SWP_DEACTIVATE: u32 = 0x100;
const SWP_MINIMIZE: u32 = 0x400;
const SWP_MAXIMIZE: u32 = 0x800;
const SWP_RESTORE: u32 = 0x1000;
const WM_SETBORDERSIZE: u32 = 0x44;
const WM_QUERYBORDERSIZE: u32 = 0x4d;

fn named(p: *const u8) -> bool {
    !p.is_null() && unsafe { *p } != 0
}

fn screen() -> (u32, u32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, 20) as u32, WinQuerySysValue(HWND_DESKTOP, 21) as u32) }
}

fn is_frame(h: u32) -> bool {
    let mut b = [0u8; 8];
    let n = unsafe { WinQueryClassName(h, b.len() as i32, b.as_mut_ptr()) };
    n > 0 && &b[..3] == b"#1\0"
}

/// WinStoreWindowPos(pszAppName, pszKeyName, hwnd)
#[unsafe(no_mangle)]
pub extern "C" fn WinStoreWindowPos(app: *const u8, key: *const u8, h: u32) -> u32 {
    let mut swp = [0u32; 9];
    if unsafe { WinQueryWindowPos(h, swp.as_mut_ptr()) } == 0 || !named(app) || !named(key) {
        return 0;
    }
    let fl = swp[0];
    let pos = [swp[4] as i16, swp[3] as i16, swp[2] as i16, swp[1] as i16];
    let word = |i: i32| unsafe { WinQueryWindowUShort(h, i) } as i16;
    // Where restoring puts it: the window words while minimized or
    // maximized, else where it is.
    let restore = if fl & SWP_RESTORE == 0 {
        let r = [word(QWS_XRESTORE), word(QWS_XRESTORE + 2), word(QWS_XRESTORE + 4), word(QWS_XRESTORE + 6)];
        (r != [0; 4]).then_some(r)
    } else {
        Some(pos)
    };
    let minimize = if fl & SWP_MINIMIZE == 0 {
        let m = [word(QWS_XMINIMIZE), word(QWS_XMINIMIZE + 2)];
        (m != [0; 2]).then_some(m)
    } else {
        Some([pos[0], pos[1]])
    };
    // A sizing border other than the system's.
    let mut border = None;
    if unsafe { WinQueryWindowULong(h, QWL_STYLE) } & FS_SIZEBORDER != 0 {
        let mut b = [0i32; 2];
        let def = unsafe { (WinQuerySysValue(HWND_DESKTOP, 4), WinQuerySysValue(HWND_DESKTOP, 5)) };
        if unsafe { WinSendMsg(h, WM_QUERYBORDERSIZE, b.as_mut_ptr() as u32, 0) } != 0 && (b[0], b[1]) != def {
            border = Some((b[0], b[1]));
        }
    }
    let rec = WinPos { fl, pos, restore, minimize, screen: screen(), border, pres: 0 }.encode();
    unsafe { PrfWriteProfileData(HINI_USERPROFILE, app, key, rec.as_ptr(), rec.len() as u32) }
}

/// WinRestoreWindowPos(pszAppName, pszKeyName, hwnd)
#[unsafe(no_mangle)]
pub extern "C" fn WinRestoreWindowPos(app: *const u8, key: *const u8, h: u32) -> u32 {
    if unsafe { WinIsWindow(0, h) } == 0 || !named(app) || !named(key) {
        return 0;
    }
    let mut n = 0u32;
    if unsafe { PrfQueryProfileSize(HINI_USERPROFILE, app, key, &mut n) } == 0 || n == 0 {
        return 0;
    }
    let mut d = vec![0u8; n as usize];
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, app, key, d.as_mut_ptr(), &mut n) } == 0 {
        return 0;
    }
    let Some(mut w) = WinPos::decode(&d[..n as usize]) else { return 0 };
    w.scale_to(screen());
    if w.fl & SWP_MINIMIZE != 0 {
        if let Some(m) = w.minimize {
            unsafe {
                WinSetWindowUShort(h, QWS_XMINIMIZE, m[0] as u16);
                WinSetWindowUShort(h, QWS_XMINIMIZE + 2, m[1] as u16);
            }
        }
    }
    // On top, unless another window of the program is active: then just
    // behind it.
    let active = unsafe { WinQueryActiveWindow(HWND_DESKTOP) };
    let behind = if active == 0 {
        HWND_TOP
    } else {
        let (mut p1, mut p2, mut t) = (0, 0, 0);
        unsafe {
            WinQueryWindowProcess(active, &mut p1, &mut t);
            WinQueryWindowProcess(h, &mut p2, &mut t);
        }
        if p1 == p2 { HWND_TOP } else { active }
    };
    let show = w.fl & SWP_SHOW != 0;
    let mut fl = w.fl & (SWP_HIDE | SWP_MINIMIZE | SWP_MAXIMIZE | SWP_RESTORE) | 7;
    if w.fl & SWP_HIDE != 0 {
        fl |= SWP_DEACTIVATE;
    }
    if is_frame(h) {
        unsafe { WinSetWindowBits(h, QWL_STYLE, 0, FS_SHELLPOSITION) };
    }
    let p = w.pos;
    unsafe { WinSetWindowPos(h, behind, p[0] as i32, p[1] as i32, p[2] as i32, p[3] as i32, fl) };
    if let Some((bx, by)) = w.border {
        unsafe { WinSendMsg(h, WM_SETBORDERSIZE, bx as u32 & 0xffff, by as u32 & 0xffff) };
    }
    if w.fl & (SWP_MINIMIZE | SWP_MAXIMIZE) != 0 {
        if let Some(r) = w.restore {
            for (i, v) in r.iter().enumerate() {
                unsafe { WinSetWindowUShort(h, QWS_XRESTORE + 2 * i as i32, *v as u16) };
            }
        }
    }
    if w.fl & (SWP_MAXIMIZE | SWP_RESTORE) != 0 {
        if let Some(m) = w.minimize {
            unsafe {
                WinSetWindowUShort(h, QWS_XMINIMIZE, m[0] as u16);
                WinSetWindowUShort(h, QWS_XMINIMIZE + 2, m[1] as u16);
            }
        }
    }
    if show {
        unsafe { WinShowWindow(h, 1) };
    }
    1
}
