//! Presentation parameters: any id kept and queried, inheritance along
//! the owner chain, colour index forms, WM_PRESPARAMCHANGED to the
//! window and to those that take it from there, PRESPARAMS at creation,
//! and controls drawing in them.

use alloc::vec::Vec;
use core::ptr::null;

use crate::check;
use crate::pm::*;

unsafe extern "C" {
    #[link_name = "PMWIN.938"]
    fn WinSetPresParam(h: u32, id: u32, cb: u32, pv: *const u8) -> u32;
    #[link_name = "PMWIN.939"]
    fn WinQueryPresParam(h: u32, id1: u32, id2: u32, pid: *mut u32, cb: u32, pv: *mut u8, fs: u32) -> u32;
    #[link_name = "PMWIN.940"]
    fn WinRemovePresParam(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.892"]
    fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMGPI.476"]
    fn GpiQueryPel(hps: u32, p: *const i32) -> i32;
    #[link_name = "PMWIN.826"]
    fn WinQuerySysColor(desk: u32, clr: i32, reserved: i32) -> u32;
    #[link_name = "PMGPI.453"]
    fn GpiQueryFontMetrics(hps: u32, cb: i32, fm: *mut u8) -> u32;
}

/// FONTMETRICS of a window's cached PS: lEmHeight and the whole.
fn ps_font(h: u32) -> (i32, [u8; 208]) {
    let mut fm = [0u8; 208];
    let hps = unsafe { WinGetPS(h) };
    unsafe { GpiQueryFontMetrics(hps, fm.len() as i32, fm.as_mut_ptr()) };
    unsafe { WinReleasePS(hps) };
    (i32::from_le_bytes([fm[68], fm[69], fm[70], fm[71]]), fm)
}

const PP_FOREGROUNDCOLOR: u32 = 1;
const PP_FOREGROUNDCOLORINDEX: u32 = 2;
const PP_BACKGROUNDCOLOR: u32 = 3;
const PP_FONTNAMESIZE: u32 = 15;
const PP_USER: u32 = 0x8000;
const QPF_NOINHERIT: u32 = 1;
const QPF_ID2COLORINDEX: u32 = 4;
const WM_PRESPARAMCHANGED: u32 = 0x002f;
const CLR_BLUE: i32 = 1;
const CLR_RED: i32 = 2;
const CLR_GREEN: i32 = 4;

static mut CHANGED: Vec<(u32, u32)> = Vec::new();

fn changed() -> &'static mut Vec<(u32, u32)> {
    unsafe { &mut *core::ptr::addr_of_mut!(CHANGED) }
}

/// The client's part (it and its PmxTest children).
pub fn client_msg(h: u32, msg: u32, mp1: u32, _mp2: u32) -> Option<u32> {
    if msg == WM_PRESPARAMCHANGED {
        changed().push((h, mp1));
    }
    None
}

fn query(h: u32, id1: u32, id2: u32, fs: u32) -> (u32, u32, Vec<u8>) {
    let mut id = 0;
    let mut buf = [0u8; 32];
    let n = unsafe { WinQueryPresParam(h, id1, id2, &mut id, buf.len() as u32, buf.as_mut_ptr(), fs) };
    (n, id, buf[..n as usize].to_vec())
}

fn pel(h: u32, x: i32, y: i32) -> i32 {
    let hps = unsafe { WinGetPS(h) };
    let p = [x, y];
    let v = unsafe { GpiQueryPel(hps, p.as_ptr()) };
    unsafe { WinReleasePS(hps) };
    v
}

pub fn pres_checks(client: u32) {
    // Every system colour has Warp 4's default: VisualAge Smalltalk draws
    // its labels in SYSCLR_WINDOWSTATICTEXT on SYSCLR_DIALOGBACKGROUND.
    let c = |i: i32| unsafe { WinQuerySysColor(1, i, 0) };
    check(
        "WinQuerySysColor: Warp 4's text colours",
        c(-26) == 0x000080 && c(-27) == 0 && c(-18) == 0 && c(-46) == 0xa0a0a0 && c(-34) == 0xcccccc,
    );

    // An application's own id.
    changed().clear();
    let ok = unsafe { WinSetPresParam(client, PP_USER + 1, 5, b"hello".as_ptr()) } != 0;
    let (n, id, b) = query(client, PP_USER + 1, 0, 0);
    check("WinSetPresParam/WinQueryPresParam: an application's id", ok && n == 5 && id == PP_USER + 1 && b == b"hello");
    check("WM_PRESPARAMCHANGED to the window", changed().contains(&(client, PP_USER + 1)));

    // A window's cached PS has its font parameter's font (VisualAge
    // Smalltalk draws its labels with WinDrawText in it).
    let plain = ps_font(client);
    unsafe { WinSetPresParam(client, PP_FONTNAMESIZE, 11, b"18.Courier\0".as_ptr()) };
    let (em, fm) = ps_font(client);
    unsafe { WinRemovePresParam(client, PP_FONTNAMESIZE) };
    check("WinGetPS: the window's PP_FONTNAMESIZE font", em >= 20 && fm != plain.1 && ps_font(client).1 == plain.1);

    // Owned windows take it, unless they have it themselves.
    let kid = unsafe { WinCreateWindow(client, b"PmxTest\0".as_ptr(), null(), 0x8000_0000, 300, 200, 40, 40, client, 3, 70, null(), null()) };
    let (n, _, b) = query(kid, PP_USER + 1, 0, 0);
    let (n2, _, _) = query(kid, PP_USER + 1, 0, QPF_NOINHERIT);
    check("an owned window inherits (not with QPF_NOINHERIT)", n == 5 && b == b"hello" && n2 == 0);
    changed().clear();
    unsafe { WinSetPresParam(client, PP_USER + 1, 3, b"bye".as_ptr()) };
    check("a change reaches the windows that inherit it", changed().contains(&(client, PP_USER + 1)) && changed().contains(&(kid, PP_USER + 1)));
    unsafe { WinSetPresParam(kid, PP_USER + 1, 2, b"me".as_ptr()) };
    changed().clear();
    unsafe { WinSetPresParam(client, PP_USER + 1, 3, b"new".as_ptr()) };
    check("... but not those with their own", !changed().iter().any(|c| c.0 == kid) && query(kid, PP_USER + 1, 0, 0).2 == b"me");

    // A colour index, turned into RGB on request.
    let red = (CLR_RED as u32).to_le_bytes();
    unsafe { WinSetPresParam(client, PP_FOREGROUNDCOLORINDEX, 4, red.as_ptr()) };
    let (n, id, b) = query(kid, PP_FOREGROUNDCOLOR, PP_FOREGROUNDCOLORINDEX, QPF_ID2COLORINDEX);
    check("a colour index as RGB (QPF_ID2COLORINDEX)", n == 4 && id == PP_FOREGROUNDCOLORINDEX && b == 0xff0000u32.to_le_bytes());
    let blue = 0x0000ffu32.to_le_bytes();
    unsafe { WinSetPresParam(client, PP_FOREGROUNDCOLOR, 4, blue.as_ptr()) };
    let (_, id, _) = query(client, PP_FOREGROUNDCOLOR, PP_FOREGROUNDCOLORINDEX, 0);
    check("an RGB colour replaces its index form", id == PP_FOREGROUNDCOLOR);
    unsafe {
        WinRemovePresParam(client, PP_FOREGROUNDCOLOR);
        WinRemovePresParam(client, PP_USER + 1);
    }
    check("WinRemovePresParam", query(client, PP_USER + 1, 0, 0).0 == 0 && query(kid, PP_FOREGROUNDCOLOR, 0, 0).0 == 0);
    unsafe { WinDestroyWindow(kid) };

    // PRESPARAMS when a window is made: a static in green.
    let mut pp = Vec::new();
    pp.extend_from_slice(&12u32.to_le_bytes());
    pp.extend_from_slice(&PP_BACKGROUNDCOLOR.to_le_bytes());
    pp.extend_from_slice(&4u32.to_le_bytes());
    pp.extend_from_slice(&0x00ff00u32.to_le_bytes());
    let st = unsafe { WinCreateWindow(client, 0xffff_0005 as *const u8, b"x\0".as_ptr(), 0x8000_0001, 300, 150, 40, 30, client, 3, 71, null(), pp.as_ptr()) };
    unsafe { WinUpdateWindow(st) };
    let (n, _, b) = query(st, PP_BACKGROUNDCOLOR, 0, QPF_NOINHERIT);
    check("WinCreateWindow's PRESPARAMS", n == 4 && b == 0x00ff00u32.to_le_bytes());
    check("a static text draws in its PP_BACKGROUNDCOLOR", pel(st, 35, 2) == CLR_GREEN);
    unsafe { WinDestroyWindow(st) };

    // A list box and an entry field in theirs.
    let lb = unsafe { WinCreateWindow(client, 0xffff_0007 as *const u8, null(), 0x8000_0000, 250, 10, 100, 60, client, 3, 72, null(), null()) };
    let bl = 0x0000ffu32.to_le_bytes();
    unsafe {
        WinSetPresParam(lb, PP_BACKGROUNDCOLOR, 4, bl.as_ptr());
        WinUpdateWindow(lb);
    }
    check("a list box draws in its PP_BACKGROUNDCOLOR", pel(lb, 20, 20) == CLR_BLUE);
    unsafe { WinDestroyWindow(lb) };
    let ef = unsafe { WinCreateWindow(client, 0xffff_0006 as *const u8, b"\0".as_ptr(), 0x8000_0000, 250, 80, 100, 20, client, 3, 73, null(), null()) };
    let rd = 0xff0000u32.to_le_bytes();
    unsafe {
        WinSetPresParam(ef, PP_BACKGROUNDCOLOR, 4, rd.as_ptr());
        WinUpdateWindow(ef);
    }
    check("an entry field draws in its PP_BACKGROUNDCOLOR", pel(ef, 50, 10) == CLR_RED);
    unsafe { WinDestroyWindow(ef) };
}
