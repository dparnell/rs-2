//! Messages between Windows 3.x window procedures and W32USER's 32-bit
//! ones, both ways (as Wine's WINPROC_CallProc16To32A and back).
//!
//! W32USER keeps every message in its Win32 form. A 16-bit procedure
//! (the program's, kept in W32USER as a 0xFFFF0000+n handle) gets the
//! Win16 form: WPARAM is 16 bits, so WM_COMMAND, WM_HSCROLL,
//! WM_MENUSELECT, WM_ACTIVATE... carry the handle or code in LPARAM's
//! other half; the WM_CTLCOLORxxx messages are one WM_CTLCOLOR with a
//! type; structures (CREATESTRUCT, WINDOWPOS, DRAWITEMSTRUCT...) have
//! 16-bit fields and live in memory 16-bit code addresses; the controls'
//! messages are numbered from WM_USER (EM_, LB_, CB_, BM_, STM_) where
//! Win32 gives each a number of its own. A 16-bit control message means
//! what the window's built-in class says (W32USER's Rs2W32Builtin): a
//! WM_USER+1 is EM_SETSEL to an edit control, LB_ADDSTRING to a list box.

use alloc::vec;
use alloc::vec::Vec;
use thunk16::{far, flat, PascalCall};

use crate::mem16::Scratch;
use crate::w32::*;

pub const WM_USER: u32 = 0x0400;

// Built-in kinds (Rs2W32Builtin).
pub const K_NONE: u32 = 0;
pub const K_BUTTON: u32 = 1;
pub const K_STATIC: u32 = 2;
pub const K_EDIT: u32 = 3;
pub const K_LISTBOX: u32 = 4;
pub const K_COMBO: u32 = 5;
pub const K_SCROLLBAR: u32 = 6;
pub const K_MDICLIENT: u32 = 7;
pub const K_DIALOG: u32 = 8;

/// The built-in class of a window's class procedure.
pub fn kind(h: u16) -> u32 {
    unsafe { Rs2W32Builtin(h as u32) }
}

fn rd16(p: *const u8, o: usize) -> u16 {
    unsafe { (p.add(o) as *const u16).read_unaligned() }
}
fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}
fn wr16(p: *mut u8, o: usize, v: u16) {
    unsafe { (p.add(o) as *mut u16).write_unaligned(v) }
}
fn wr32(p: *mut u8, o: usize, v: u32) {
    unsafe { (p.add(o) as *mut u32).write_unaligned(v) }
}
/// A 16-bit signed value as 32 bits.
pub fn sx(v: u16) -> u32 {
    v as i16 as i32 as u32
}
/// A 16-bit index where 0xFFFF is -1.
fn ix(v: u16) -> u32 {
    if v == 0xffff { u32::MAX } else { v as u32 }
}

// ---- Rectangles and points ----

pub fn rect16to32(p: *const u8, r: &mut [i32; 4]) {
    for (i, v) in r.iter_mut().enumerate() {
        *v = rd16(p, 2 * i) as i16 as i32;
    }
}
pub fn rect32to16(r: &[i32; 4], p: *mut u8) {
    for (i, v) in r.iter().enumerate() {
        wr16(p, 2 * i, *v as u16);
    }
}
fn rect32_at(p: *const u8) -> [i32; 4] {
    core::array::from_fn(|i| rd32(p, 4 * i) as i32)
}
fn put_rect32(p: *mut u8, r: &[i32; 4]) {
    for (i, v) in r.iter().enumerate() {
        wr32(p, 4 * i, *v as u32);
    }
}

// ---- The control messages' numbers ----

/// A Win16 control message (WM_USER + n) to a window of `kind` as Win32's
/// number; None for other messages.
pub fn ctl16to32(kind: u32, m: u16) -> Option<u32> {
    let m = m as u32;
    if !(WM_USER..WM_USER + 0x40).contains(&m) {
        return None;
    }
    let n = m - WM_USER;
    match kind {
        K_EDIT if n <= 34 => Some(0xb0 + n),
        K_BUTTON if n <= 4 => Some(0xf0 + n),
        K_STATIC if n <= 1 => Some(0x170 + n),
        K_LISTBOX if (1..=35).contains(&n) => Some(0x17f + n),
        K_COMBO if n <= 24 => Some(0x140 + n),
        _ => None,
    }
}

/// A Win32 control message as Win16's (WM_USER + n); None for others.
pub fn ctl32to16(m: u32) -> Option<u16> {
    Some(match m {
        0xb0..=0xd2 => WM_USER + (m - 0xb0),
        0xf0..=0xf4 => WM_USER + (m - 0xf0),
        0x170..=0x171 => WM_USER + (m - 0x170),
        0x180..=0x1a2 => WM_USER + (m - 0x17f),
        0x140..=0x158 => WM_USER + (m - 0x140),
        _ => return None,
    } as u16)
}

// Win32 control messages with pointers or packed parameters.
const EM_GETSEL: u32 = 0xb0;
const EM_SETSEL: u32 = 0xb1;
const EM_GETRECT: u32 = 0xb2;
const EM_SETRECT: u32 = 0xb3;
const EM_SETRECTNP: u32 = 0xb4;
const EM_LINESCROLL: u32 = 0xb6;
const EM_REPLACESEL: u32 = 0xc2;
const EM_GETLINE: u32 = 0xc4;
const EM_SETTABSTOPS: u32 = 0xcb;
const EM_SETHANDLE: u32 = 0xbc;
const EM_SETWORDBREAKPROC: u32 = 0xd0;
const EM_GETWORDBREAKPROC: u32 = 0xd1;
const LB_ADDSTRING: u32 = 0x180;
const LB_INSERTSTRING: u32 = 0x181;
const LB_SETSEL: u32 = 0x185;
const LB_GETTEXT: u32 = 0x189;
const LB_SELECTSTRING: u32 = 0x18c;
const LB_DIR: u32 = 0x18d;
const LB_FINDSTRING: u32 = 0x18f;
const LB_GETSELITEMS: u32 = 0x191;
const LB_SETTABSTOPS: u32 = 0x192;
const LB_GETITEMRECT: u32 = 0x198;
const LB_FINDSTRINGEXACT: u32 = 0x1a2;
const CB_GETEDITSEL: u32 = 0x140;
const CB_ADDSTRING: u32 = 0x143;
const CB_DIR: u32 = 0x145;
const CB_GETLBTEXT: u32 = 0x148;
const CB_INSERTSTRING: u32 = 0x14a;
const CB_FINDSTRING: u32 = 0x14c;
const CB_SELECTSTRING: u32 = 0x14d;
const CB_GETDROPPEDCONTROLRECT: u32 = 0x152;
const CB_FINDSTRINGEXACT: u32 = 0x158;

/// Whether a list box (combo box) message's LPARAM is a string: unless
/// the control is owner-drawn without LBS_HASSTRINGS (CBS_HASSTRINGS),
/// when it is the item's data.
fn lb_strings(h: u16, combo: bool) -> bool {
    let style = unsafe { GetWindowLongA(h as u32, (-16i32) as u32) };
    let (owner, has) = if combo { (style & 0x30 != 0, style & 0x200 != 0) } else { (style & 0x30 != 0, style & 0x40 != 0) };
    !owner || has
}

/// Index arguments of list box and combo box messages (-1 meaning none
/// or all): their WPARAM sign-extended.
fn index_wp(m: u32) -> bool {
    matches!(m, 0x182 | 0x186 | 0x189 | 0x18a | 0x18c | 0x18f | 0x197 | 0x198 | 0x199 | 0x19a | 0x19c | 0x19e | 0x1a0 | 0x1a1 | 0x1a2 | 0x181 | 0x144 | 0x148 | 0x149 | 0x14a | 0x14c | 0x14d | 0x14e | 0x150 | 0x151 | 0x153 | 0x154 | 0x158)
}

// ---- 16 to 32 ----

/// Call `f` with a 16-bit message (to window `h`, whose built-in kind is
/// `k`) in its 32-bit form, and give its result in the 16-bit form. The
/// structures it points at are copied to 32-bit ones and back.
pub fn to32(h: u16, m: u16, wp: u16, lp: u32, k: u32, f: &mut dyn FnMut(u32, u32, u32) -> u32) -> u32 {
    let p = flat(lp);
    // The controls' messages.
    if let Some(m32) = ctl16to32(k, m) {
        return ctl_to32(h, m32, wp, lp, f);
    }
    let w = wp as u32;
    // The font dialog's WM_CHOOSEFONT_GETLOGFONT and _SETLOGFONT: a
    // LOGFONT.
    if (m == 0x0401 || m == 0x0465) && k == K_DIALOG && !p.is_null() && is_commdlg(h) {
        let mut lf = crate::gdi::logfont32(p);
        let r = f(m as u32, w, lf.as_mut_ptr() as u32);
        if m == 0x0401 {
            crate::gdi::logfont16(lf.as_ptr(), p);
        }
        return r;
    }
    match m {
        // WM_ACTIVATE(state, MAKELONG(hwnd, minimized))
        0x0006 => f(0x0006, w | (lp >> 16) << 16, lp & 0xffff),
        // WM_CHARTOITEM, WM_VKEYTOITEM(char, MAKELONG(hwnd, caret))
        0x002e | 0x002f => {
            let r = f(m as u32, w | (lp >> 16) << 16, lp & 0xffff);
            sx(r as u16)
        }
        // WM_COMMAND(id, MAKELONG(hwnd, code))
        0x0111 => f(0x0111, w | (lp >> 16) << 16, lp & 0xffff),
        // WM_CTLCOLOR(hdc, MAKELONG(hwnd, type))
        0x0019 => f(0x0132 + (lp >> 16).min(6), w, lp & 0xffff) & 0xffff,
        // WM_HSCROLL, WM_VSCROLL(code, MAKELONG(pos, hwnd))
        0x0114 | 0x0115 => f(m as u32, w | (lp & 0xffff) << 16, lp >> 16),
        // WM_MENUSELECT(id or popup, MAKELONG(flags, hmenu))
        0x011f => {
            let (fl, hm) = (lp & 0xffff, lp >> 16);
            let id = if fl & 0x10 != 0 && fl != 0xffff {
                // A pop-up: Win32 gives its position in the menu.
                let n = unsafe { GetMenuItemCount(hm) } as i32;
                (0..n.max(0)).find(|&i| unsafe { GetSubMenu(hm, i as u32) } == w).map_or(w, |i| i as u32)
            } else {
                w
            };
            f(0x011f, id | fl << 16, hm)
        }
        // WM_MENUCHAR(char, MAKELONG(flags, hmenu))
        0x0120 => f(0x0120, w | (lp & 0xffff) << 16, lp >> 16),
        // WM_PARENTNOTIFY(event, MAKELONG(hwnd, id) or the point)
        0x0210 => {
            if wp == 1 || wp == 2 {
                f(0x0210, w | (lp >> 16) << 16, lp & 0xffff)
            } else {
                f(0x0210, w, lp)
            }
        }
        // WM_MDIACTIVATE: to the client, the child; to a child, (activate,
        // MAKELONG(activated, deactivated)).
        0x0222 => {
            if k == K_MDICLIENT {
                f(0x0222, w, 0)
            } else {
                f(0x0222, lp >> 16, lp & 0xffff)
            }
        }
        // WM_MDIGETACTIVE: MAKELONG(child, maximized).
        0x0229 => {
            let mut max = 0u32;
            let r = f(0x0229, 0, &mut max as *mut u32 as u32);
            (r & 0xffff) | (max != 0) as u32 * 0x1_0000
        }
        // WM_MDISETMENU(refresh, MAKELONG(frame menu, window menu))
        0x0230 => f(0x0230, lp & 0xffff, lp >> 16),
        // WM_MDINEXT(child, previous)
        0x0224 => f(0x0224, w, lp),
        // WM_MDICREATE: MDICREATESTRUCT.
        0x0220 if !p.is_null() => {
            let mcs = mdicreate16to32(p);
            f(0x0220, 0, mcs.as_ptr() as u32)
        }
        // WM_NCCREATE, WM_CREATE: CREATESTRUCT (an MDI child's
        // lpCreateParams is an MDICREATESTRUCT).
        0x0081 | 0x0001 if !p.is_null() => {
            let mut cs = [0u32; 12];
            cs[0] = rd32(p, 0);
            cs[1] = rd16(p, 4) as u32;
            cs[2] = rd16(p, 6) as u32;
            cs[3] = rd16(p, 8) as u32;
            for i in 0..4 {
                cs[4 + i] = sx(rd16(p, 10 + 2 * i));
            }
            cs[8] = rd32(p, 18);
            cs[9] = flat_or_id(rd32(p, 22));
            cs[10] = flat_or_id(rd32(p, 26));
            cs[11] = rd32(p, 30);
            let mut mcs = Vec::new();
            if cs[3] != 0 && kind(cs[3] as u16) == K_MDICLIENT && cs[0] != 0 {
                mcs = mdicreate16to32(flat(cs[0])).to_vec();
                cs[0] = mcs.as_ptr() as u32;
            }
            let r = f(m as u32, w, cs.as_ptr() as u32);
            drop(mcs);
            r
        }
        // WM_GETMINMAXINFO: five POINTs.
        0x0024 if !p.is_null() => {
            let mut mm = [0i32; 10];
            for (i, v) in mm.iter_mut().enumerate() {
                *v = rd16(p, 2 * i) as i16 as i32;
            }
            let r = f(0x0024, w, mm.as_mut_ptr() as u32);
            for (i, v) in mm.iter().enumerate() {
                wr16(p, 2 * i, *v as u16);
            }
            r
        }
        // WM_WINDOWPOSCHANGING, WM_WINDOWPOSCHANGED: WINDOWPOS.
        0x0046 | 0x0047 if !p.is_null() => {
            let mut wp32 = windowpos16to32(p);
            let r = f(m as u32, w, wp32.as_mut_ptr() as u32);
            windowpos32to16(&wp32, p);
            r
        }
        // WM_NCCALCSIZE: a RECT, or NCCALCSIZE_PARAMS.
        0x0083 if !p.is_null() => {
            let mut b = [0i32; 13];
            let n = if wp != 0 { 3 } else { 1 };
            for i in 0..n {
                let mut r = [0; 4];
                rect16to32(unsafe { p.add(8 * i) }, &mut r);
                b[4 * i..4 * i + 4].copy_from_slice(&r);
            }
            let mut wpos = [0u32; 7];
            if wp != 0 {
                let pp = flat(rd32(p, 24));
                if !pp.is_null() {
                    wpos = windowpos16to32(pp);
                }
                b[12] = wpos.as_mut_ptr() as i32;
            }
            let r = f(0x0083, w, b.as_mut_ptr() as u32);
            for i in 0..n {
                let r: [i32; 4] = b[4 * i..4 * i + 4].try_into().unwrap();
                rect32to16(&r, unsafe { p.add(8 * i) });
            }
            r
        }
        // WM_MEASUREITEM: MEASUREITEMSTRUCT.
        0x002c if !p.is_null() => {
            let mut mi = [rd16(p, 0) as u32, rd16(p, 2) as u32, ix(rd16(p, 4)), rd16(p, 6) as u32, rd16(p, 8) as u32, rd32(p, 10)];
            let r = f(0x002c, w, mi.as_mut_ptr() as u32);
            wr16(p as *mut u8, 6, mi[3] as u16);
            wr16(p as *mut u8, 8, mi[4] as u16);
            r
        }
        // WM_DRAWITEM: DRAWITEMSTRUCT.
        0x002b if !p.is_null() => {
            let mut di = [0u32; 12];
            for i in 0..7 {
                di[i] = rd16(p, 2 * i) as u32;
            }
            di[2] = ix(rd16(p, 4));
            let mut r = [0; 4];
            rect16to32(unsafe { p.add(14) }, &mut r);
            for i in 0..4 {
                di[7 + i] = r[i] as u32;
            }
            di[11] = rd32(p, 22);
            f(0x002b, w, di.as_mut_ptr() as u32)
        }
        // WM_COMPAREITEM: COMPAREITEMSTRUCT.
        0x0039 if !p.is_null() => {
            let ci = [rd16(p, 0) as u32, rd16(p, 2) as u32, rd16(p, 4) as u32, ix(rd16(p, 6)), rd32(p, 8), ix(rd16(p, 12)), rd32(p, 14), 0];
            sx(f(0x0039, w, ci.as_ptr() as u32) as u16)
        }
        // WM_DELETEITEM: DELETEITEMSTRUCT.
        0x002d if !p.is_null() => {
            let di = [rd16(p, 0) as u32, rd16(p, 2) as u32, ix(rd16(p, 4)), rd16(p, 6) as u32, rd32(p, 8)];
            f(0x002d, w, di.as_ptr() as u32)
        }
        // Strings: WM_SETTEXT, WM_WININICHANGE, WM_DEVMODECHANGE; buffers:
        // WM_GETTEXT, WM_ASKCBFORMATNAME (flat, as the 16:16 addresses
        // are).
        0x000c | 0x001a | 0x001b | 0x000d | 0x030c => f(m as u32, w, flat(lp) as u32),
        // WM_GETDLGCODE, WM_QUERYDRAGICON...: a MSG.
        0x0087 if !p.is_null() => {
            let mut m32 = [0u32; 7];
            msg16to32(p, &mut m32);
            f(0x0087, w, m32.as_ptr() as u32)
        }
        // WM_TIMER: the procedure (a 16-bit one's handle).
        0x0113 => f(0x0113, w, if lp == 0 { 0 } else { proc16to32(lp) }),
        // WM_NEXTDLGCTL(control or direction, flag)
        0x0028 => f(0x0028, if lp & 0xffff != 0 { w } else { sx(wp) }, lp & 0xffff),
        // WM_SYSTIMER and the others: as they are (WPARAM zero-extended).
        _ => f(m as u32, w, lp),
    }
}

/// Is `h` one of COMDLG32's dialogs (its dialog procedure W32USER's)?
fn is_commdlg(h: u16) -> bool {
    let p = unsafe { GetWindowLongA(h as u32, 4) };
    p != 0 && p == Rs2CommDlgProc as *const () as usize as u32
}

fn flat_or_id(v: u32) -> u32 {
    if v >> 16 == 0 { v } else { flat(v) as u32 }
}

/// A 16-bit procedure address as W32USER keeps it: its built-in
/// procedure for WIN16's entries of W32USER's, else the 16-bit handle.
pub fn proc16to32(far_: u32) -> u32 {
    if far_ == 0 {
        return 0;
    }
    crate::user::native32_of(far_).unwrap_or_else(|| unsafe { Rs2W32Proc16(far_) })
}

/// A procedure address W32USER gives, as a 16-bit program sees it.
pub fn proc32to16(p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let f = unsafe { Rs2W32Far16(p) };
    if f != 0 {
        return f;
    }
    crate::user::native16_of(p).unwrap_or(p)
}

/// MDICREATESTRUCT 16 to 32: szClass, szTitle, hOwner, x, y, cx, cy,
/// style, lParam.
fn mdicreate16to32(p: *const u8) -> [u32; 9] {
    let cw = |v: u16| if v == 0x8000 { 0x8000_0000 } else { sx(v) };
    [flat_or_id(rd32(p, 0)), flat_or_id(rd32(p, 4)), rd16(p, 8) as u32, cw(rd16(p, 10)), cw(rd16(p, 12)), cw(rd16(p, 14)), cw(rd16(p, 16)), rd32(p, 18), rd32(p, 22)]
}

fn windowpos16to32(p: *const u8) -> [u32; 7] {
    let after = rd16(p, 2);
    // HWND_TOPMOST (-1), HWND_NOTOPMOST (-2) are signed.
    let after = if after >= 0xfffe { sx(after) } else { after as u32 };
    [rd16(p, 0) as u32, after, sx(rd16(p, 4)), sx(rd16(p, 6)), sx(rd16(p, 8)), sx(rd16(p, 10)), rd16(p, 12) as u32]
}

fn windowpos32to16(w: &[u32; 7], p: *mut u8) {
    for (i, v) in w.iter().enumerate() {
        wr16(p, 2 * i, *v as u16);
    }
}

/// MSG 16 (hwnd, message, wParam, lParam, time, pt) to 32, the message
/// converted as a posted one.
pub fn msg16to32(p: *const u8, m: &mut [u32; 7]) {
    let (h, msg, wp, lp) = (rd16(p, 0), rd16(p, 2), rd16(p, 4), rd32(p, 6));
    let (m32, wp32, lp32) = posted16to32(h, msg, wp, lp);
    *m = [h as u32, m32, wp32, lp32, rd32(p, 10), sx(rd16(p, 14)), sx(rd16(p, 16))];
}

/// A posted 16-bit message (no structures to copy) in its 32-bit form.
pub fn posted16to32(h: u16, m: u16, wp: u16, lp: u32) -> (u32, u32, u32) {
    let mut out = (m as u32, wp as u32, lp);
    let k = if h == 0 { K_NONE } else { kind(h) };
    let mut f = |a: u32, b: u32, c: u32| {
        out = (a, b, c);
        0
    };
    match m {
        // Messages whose conversion copies structures are posted as they
        // are (the pointers stay 16:16).
        0x0001 | 0x0081 | 0x0024 | 0x0046 | 0x0047 | 0x0083 | 0x002b | 0x002c | 0x002d | 0x0039 | 0x0220 | 0x0229 | 0x0087 => {}
        _ => {
            to32(h, m, wp, lp, k, &mut f);
        }
    }
    out
}

/// The control messages, 16 to 32.
fn ctl_to32(h: u16, m: u32, wp: u16, lp: u32, f: &mut dyn FnMut(u32, u32, u32) -> u32) -> u32 {
    let w = wp as u32;
    let p = flat(lp);
    match m {
        // EM_GETSEL: MAKELONG(start, end).
        EM_GETSEL => {
            let (mut a, mut b) = (0u32, 0u32);
            f(m, &mut a as *mut u32 as u32, &mut b as *mut u32 as u32);
            (a & 0xffff) | b << 16
        }
        // EM_SETSEL(_, MAKELONG(start, end)), -1 ends.
        EM_SETSEL => f(m, sx(lp as u16), sx((lp >> 16) as u16)),
        EM_GETRECT if !p.is_null() => {
            let mut r = [0i32; 4];
            let res = f(m, w, r.as_mut_ptr() as u32);
            rect32to16(&r, p);
            res
        }
        EM_SETRECT | EM_SETRECTNP if !p.is_null() => {
            let mut r = [0i32; 4];
            rect16to32(p, &mut r);
            f(m, w, r.as_ptr() as u32)
        }
        // EM_LINESCROLL(_, MAKELONG(lines, columns))
        EM_LINESCROLL => f(m, sx((lp >> 16) as u16), sx(lp as u16)),
        EM_REPLACESEL | EM_GETLINE => f(m, w, p as u32),
        EM_SETTABSTOPS => {
            let v: Vec<i32> = (0..w as usize).map(|i| rd16(p, 2 * i) as i16 as i32).collect();
            f(m, w, if p.is_null() { 0 } else { v.as_ptr() as u32 })
        }
        EM_SETWORDBREAKPROC => f(m, w, proc16to32(lp)),
        EM_GETWORDBREAKPROC => proc32to16(f(m, w, lp)),
        // EM_SETHANDLE(hlocal): a local block of the control's instance
        // segment (its text, for W32USER).
        EM_SETHANDLE if wp != 0 => {
            let seg = crate::user::raw_hinst(h);
            f(m, flat((seg as u32) << 16 | w) as u32, lp)
        }
        LB_ADDSTRING | LB_INSERTSTRING | LB_SELECTSTRING | LB_FINDSTRING | LB_FINDSTRINGEXACT => {
            let wi = if m == LB_ADDSTRING { w } else { ix(wp) };
            f(m, wi, if lb_strings(h, false) { p as u32 } else { lp })
        }
        LB_DIR | CB_DIR => f(m, w, p as u32),
        LB_GETTEXT => f(m, ix(wp), if lb_strings(h, false) { p as u32 } else { p as u32 }),
        CB_GETLBTEXT => f(m, ix(wp), p as u32),
        // LB_SETSEL(select, index)
        LB_SETSEL => f(m, w, sx(lp as u16)),
        LB_GETSELITEMS => {
            let mut v = vec![0i32; w as usize];
            let n = f(m, w, v.as_mut_ptr() as u32);
            for (i, x) in v.iter().take(n as usize).enumerate() {
                wr16(p, 2 * i, *x as u16);
            }
            n
        }
        LB_SETTABSTOPS => {
            let v: Vec<i32> = (0..w as usize).map(|i| rd16(p, 2 * i) as i16 as i32).collect();
            f(m, w, if p.is_null() { 0 } else { v.as_ptr() as u32 })
        }
        LB_GETITEMRECT | CB_GETDROPPEDCONTROLRECT if !p.is_null() => {
            let mut r = [0i32; 4];
            let res = f(m, ix(wp), r.as_mut_ptr() as u32);
            rect32to16(&r, p);
            res
        }
        CB_ADDSTRING | CB_INSERTSTRING | CB_FINDSTRING | CB_SELECTSTRING | CB_FINDSTRINGEXACT => {
            let wi = if m == CB_ADDSTRING { w } else { ix(wp) };
            f(m, wi, if lb_strings(h, true) { p as u32 } else { lp })
        }
        // CB_GETEDITSEL: MAKELONG(start, end).
        CB_GETEDITSEL => {
            let (mut a, mut b) = (0u32, 0u32);
            f(m, &mut a as *mut u32 as u32, &mut b as *mut u32 as u32);
            (a & 0xffff) | b << 16
        }
        _ if index_wp(m) => f(m, ix(wp), lp),
        _ => f(m, w, lp),
    }
}

// ---- 32 to 16 ----

/// Call `f` with a 32-bit message (to window `h`) in its 16-bit form,
/// and give its result in the 32-bit form. Structures are copied into
/// memory 16-bit code addresses and back.
pub fn to16(h: u16, m: u32, wp: u32, lp: u32, f: &mut dyn FnMut(u16, u16, u32) -> u32) -> u32 {
    let s = Scratch::new();
    if let Some(m16) = ctl32to16(m) {
        return ctl_to16(h, m, m16, wp, lp, &s, f);
    }
    let p = lp as *mut u8;
    let lo = |v: u32| v & 0xffff;
    let hi = |v: u32| v >> 16;
    let r = match m {
        0x0006 => f(0x0006, wp as u16, lo(lp) | hi(wp) << 16),
        0x002e | 0x002f => f(m as u16, wp as u16, lo(lp) | hi(wp) << 16),
        0x0111 => f(0x0111, wp as u16, lo(lp) | hi(wp) << 16),
        0x0132..=0x0138 => f(0x0019, wp as u16, lo(lp) | (m - 0x0132) << 16),
        0x0114 | 0x0115 => f(m as u16, wp as u16, hi(wp) | lo(lp) << 16),
        0x011f => {
            let (id, fl) = (lo(wp), hi(wp));
            let w16 = if fl & 0x10 != 0 && fl != 0xffff { unsafe { GetSubMenu(lp, id) } } else { id };
            f(0x011f, w16 as u16, fl | lo(lp) << 16)
        }
        0x0120 => f(0x0120, wp as u16, hi(wp) | lo(lp) << 16),
        0x0210 => {
            let ev = lo(wp);
            if ev == 1 || ev == 2 { f(0x0210, ev as u16, lo(lp) | hi(wp) << 16) } else { f(0x0210, ev as u16, lp) }
        }
        0x0222 => {
            if kind(h) == K_MDICLIENT {
                f(0x0222, wp as u16, 0)
            } else {
                f(0x0222, (lp == h as u32) as u16, lo(lp) | lo(wp) << 16)
            }
        }
        0x0229 => {
            let r = f(0x0229, 0, 0);
            if lp != 0 {
                unsafe { (lp as *mut u32).write_unaligned(hi(r) & 1) };
            }
            lo(r)
        }
        0x0230 => f(0x0230, 1, lo(wp) | lo(lp) << 16),
        0x0220 if lp != 0 => {
            let (far16, q) = s.get(26);
            mdicreate32to16(p, q, &s);
            f(0x0220, wp as u16, far16)
        }
        0x0081 | 0x0001 if lp != 0 => {
            let (far16, q) = s.get(34);
            let cs: [u32; 12] = core::array::from_fn(|i| rd32(p, 4 * i));
            let mut params = cs[0];
            if cs[3] != 0 && kind(cs[3] as u16) == K_MDICLIENT && params != 0 {
                let (mf, mq) = s.get(26);
                mdicreate32to16(params as *const u8, mq, &s);
                params = mf;
            }
            wr32(q, 0, params);
            wr16(q, 4, cs[1] as u16);
            wr16(q, 6, cs[2] as u16);
            wr16(q, 8, cs[3] as u16);
            for i in 0..4 {
                wr16(q, 10 + 2 * i, cs[4 + i] as u16);
            }
            wr32(q, 18, cs[8]);
            wr32(q, 22, str16(cs[9], &s));
            wr32(q, 26, str16(cs[10], &s));
            wr32(q, 30, cs[11]);
            f(m as u16, wp as u16, far16)
        }
        0x0024 if lp != 0 => {
            let (far16, q) = s.get(20);
            for i in 0..10 {
                wr16(q, 2 * i, rd32(p, 4 * i) as u16);
            }
            let r = f(0x0024, wp as u16, far16);
            for i in 0..10 {
                wr32(p, 4 * i, rd16(q, 2 * i) as i16 as i32 as u32);
            }
            r
        }
        0x0046 | 0x0047 if lp != 0 => {
            let (far16, q) = s.get(14);
            let w32: [u32; 7] = core::array::from_fn(|i| rd32(p, 4 * i));
            windowpos32to16(&w32, q);
            let r = f(m as u16, wp as u16, far16);
            let back = windowpos16to32(q);
            for (i, v) in back.iter().enumerate() {
                wr32(p, 4 * i, *v);
            }
            r
        }
        0x0083 if lp != 0 => {
            let (far16, q) = s.get(28);
            let n = if wp != 0 { 3 } else { 1 };
            for i in 0..n {
                rect32to16(&rect32_at(unsafe { p.add(16 * i) }), unsafe { q.add(8 * i) });
            }
            if wp != 0 {
                let wpp = rd32(p, 48) as *mut u8;
                if !wpp.is_null() {
                    let (wf, wq) = s.get(14);
                    let w32: [u32; 7] = core::array::from_fn(|i| rd32(wpp, 4 * i));
                    windowpos32to16(&w32, wq);
                    wr32(q, 24, wf);
                }
            }
            let r = f(0x0083, wp as u16, far16);
            for i in 0..n {
                let mut rr = [0; 4];
                rect16to32(unsafe { q.add(8 * i) }, &mut rr);
                put_rect32(unsafe { p.add(16 * i) }, &rr);
            }
            r
        }
        0x002c if lp != 0 => {
            let (far16, q) = s.get(14);
            for i in 0..5 {
                wr16(q, 2 * i, rd32(p, 4 * i) as u16);
            }
            wr32(q, 10, rd32(p, 20));
            let r = f(0x002c, wp as u16, far16);
            wr32(p, 12, rd16(q, 6) as u32);
            wr32(p, 16, rd16(q, 8) as u32);
            r
        }
        0x002b if lp != 0 => {
            let (far16, q) = s.get(26);
            for i in 0..7 {
                wr16(q, 2 * i, rd32(p, 4 * i) as u16);
            }
            rect32to16(&rect32_at(unsafe { p.add(28) }), unsafe { q.add(14) });
            wr32(q, 22, rd32(p, 44));
            f(0x002b, wp as u16, far16)
        }
        0x0039 if lp != 0 => {
            let (far16, q) = s.get(18);
            for i in 0..4 {
                wr16(q, 2 * i, rd32(p, 4 * i) as u16);
            }
            wr32(q, 8, rd32(p, 16));
            wr16(q, 12, rd32(p, 20) as u16);
            wr32(q, 14, rd32(p, 24));
            sx(f(0x0039, wp as u16, far16) as u16)
        }
        0x002d if lp != 0 => {
            let (far16, q) = s.get(12);
            for i in 0..4 {
                wr16(q, 2 * i, rd32(p, 4 * i) as u16);
            }
            wr32(q, 8, rd32(p, 16));
            f(0x002d, wp as u16, far16)
        }
        0x000c | 0x001a | 0x001b if lp != 0 => f(m as u16, wp as u16, str16(lp, &s)),
        // WM_GETTEXT, WM_ASKCBFORMATNAME: a buffer of wParam bytes.
        0x000d | 0x030c if lp != 0 => {
            let n = (wp as usize).min(s.room().saturating_sub(64)).min(0xffff);
            let (far16, q) = s.get(n.max(1));
            let r = f(m as u16, n as u16, far16);
            if n > 0 {
                let len = (0..n).position(|i| unsafe { *q.add(i) } == 0).unwrap_or(n - 1);
                unsafe {
                    core::ptr::copy_nonoverlapping(q, p, len);
                    *p.add(len) = 0;
                }
            }
            r
        }
        0x0087 if lp != 0 => {
            let (far16, q) = s.get(18);
            msg32to16(p, q);
            f(0x0087, wp as u16, far16)
        }
        0x0113 => f(0x0113, wp as u16, proc32to16(lp)),
        // A common dialog's structure (WM_INITDIALOG to its hook, its
        // registered messages): the program's own.
        0x0110 | 0xc000..=0xffff => f(m as u16, wp as u16, crate::commdlg::map_lp(lp).unwrap_or(lp)),
        _ => f(m as u16, wp as u16, lp),
    };
    drop(s);
    r
}

/// A string for 16-bit code: a copy in the scratch area (IDs as they
/// are).
fn str16(v: u32, s: &Scratch) -> u32 {
    if v >> 16 == 0 {
        return v;
    }
    s.str(unsafe { crate::cstr(v as *const u8) })
}

fn mdicreate32to16(p: *const u8, q: *mut u8, s: &Scratch) {
    let cw = |v: u32| if v == 0x8000_0000 { 0x8000u16 } else { v as u16 };
    wr32(q, 0, str16(rd32(p, 0), s));
    wr32(q, 4, str16(rd32(p, 4), s));
    wr16(q, 8, rd32(p, 8) as u16);
    for i in 0..4 {
        wr16(q, 10 + 2 * i, cw(rd32(p, 12 + 4 * i)));
    }
    wr32(q, 18, rd32(p, 28));
    wr32(q, 22, rd32(p, 32));
}

/// MSG 32 to 16 (the message converted as a posted one).
pub fn msg32to16(p: *const u8, q: *mut u8) {
    let h = rd32(p, 0) as u16;
    let (m, wp, lp) = posted32to16(h, rd32(p, 4), rd32(p, 8), rd32(p, 12));
    wr16(q, 0, h);
    wr16(q, 2, m);
    wr16(q, 4, wp);
    wr32(q, 6, lp);
    wr32(q, 10, rd32(p, 16));
    wr16(q, 14, rd32(p, 20) as u16);
    wr16(q, 16, rd32(p, 24) as u16);
}

/// A posted 32-bit message in its 16-bit form (pointers that came from
/// 16-bit code become 16:16 again).
pub fn posted32to16(_h: u16, m: u32, wp: u32, lp: u32) -> (u16, u16, u32) {
    let lo = |v: u32| v & 0xffff;
    let hi = |v: u32| v >> 16;
    if let Some(m16) = ctl32to16(m) {
        return (m16, wp as u16, lp);
    }
    match m {
        0x0006 | 0x002e | 0x002f | 0x0111 => (m as u16, wp as u16, lo(lp) | hi(wp) << 16),
        0x0132..=0x0138 => (0x0019, wp as u16, lo(lp) | (m - 0x0132) << 16),
        0x0114 | 0x0115 => (m as u16, wp as u16, hi(wp) | lo(lp) << 16),
        0x0120 => (0x0120, wp as u16, hi(wp) | lo(lp) << 16),
        0x0210 => {
            let ev = lo(wp);
            (0x0210, ev as u16, if ev == 1 || ev == 2 { lo(lp) | hi(wp) << 16 } else { lp })
        }
        0x0113 => (0x0113, wp as u16, proc32to16(lp)),
        0x000c | 0x001a | 0x001b | 0x000d => (m as u16, wp as u16, if lp == 0 { 0 } else { far(lp as *const u8) }),
        _ => (m as u16, wp as u16, lp),
    }
}

/// The control messages, 32 to 16.
fn ctl_to16(h: u16, m: u32, m16: u16, wp: u32, lp: u32, s: &Scratch, f: &mut dyn FnMut(u16, u16, u32) -> u32) -> u32 {
    let p = lp as *mut u8;
    match m {
        EM_GETSEL | CB_GETEDITSEL => {
            let r = f(m16, 0, 0);
            if wp != 0 {
                unsafe { (wp as *mut u32).write_unaligned(r & 0xffff) };
            }
            if lp != 0 {
                unsafe { (lp as *mut u32).write_unaligned(r >> 16) };
            }
            r
        }
        EM_SETSEL => f(m16, 0, (wp & 0xffff) | (lp & 0xffff) << 16),
        EM_GETRECT if lp != 0 => {
            let (far16, q) = s.get(8);
            let r = f(m16, wp as u16, far16);
            let mut rr = [0; 4];
            rect16to32(q, &mut rr);
            put_rect32(p, &rr);
            r
        }
        EM_SETRECT | EM_SETRECTNP if lp != 0 => {
            let (far16, q) = s.get(8);
            rect32to16(&rect32_at(p), q);
            f(m16, wp as u16, far16)
        }
        EM_LINESCROLL => f(m16, 0, (lp & 0xffff) | (wp & 0xffff) << 16),
        EM_REPLACESEL if lp != 0 => f(m16, wp as u16, str16(lp, s)),
        EM_GETLINE if lp != 0 => {
            let n = (rd16(p, 0) as usize).max(2);
            let (far16, q) = s.get(n);
            wr16(q, 0, n as u16);
            let r = f(m16, wp as u16, far16);
            unsafe { core::ptr::copy_nonoverlapping(q, p, (r as usize).min(n)) };
            r
        }
        EM_SETTABSTOPS | LB_SETTABSTOPS if lp != 0 => {
            let (far16, q) = s.get(2 * wp as usize);
            for i in 0..wp as usize {
                wr16(q, 2 * i, rd32(p, 4 * i) as u16);
            }
            f(m16, wp as u16, far16)
        }
        EM_SETWORDBREAKPROC => f(m16, wp as u16, proc32to16(lp)),
        EM_GETWORDBREAKPROC => proc16to32(f(m16, wp as u16, lp)),
        LB_ADDSTRING | LB_INSERTSTRING | LB_SELECTSTRING | LB_FINDSTRING | LB_FINDSTRINGEXACT | LB_DIR if lp != 0 && lb_strings(h, false) => f(m16, wp as u16, str16(lp, s)),
        CB_ADDSTRING | CB_INSERTSTRING | CB_SELECTSTRING | CB_FINDSTRING | CB_FINDSTRINGEXACT | CB_DIR if lp != 0 && lb_strings(h, true) => f(m16, wp as u16, str16(lp, s)),
        LB_GETTEXT | CB_GETLBTEXT if lp != 0 => {
            // The text's length first, for the buffer.
            let n = (f(if m == LB_GETTEXT { m16 + 1 } else { m16 + 1 }, wp as u16, 0) as usize).min(0x7fff).max(4) + 1;
            let (far16, q) = s.get(n);
            let r = f(m16, wp as u16, far16);
            unsafe { core::ptr::copy_nonoverlapping(q, p, n.min((r as usize).saturating_add(1)).max(4).min(n)) };
            r
        }
        LB_SETSEL => f(m16, wp as u16, lp & 0xffff),
        LB_GETSELITEMS if lp != 0 => {
            let (far16, q) = s.get(2 * wp as usize);
            let r = f(m16, wp as u16, far16);
            for i in 0..(r as usize).min(wp as usize) {
                wr32(p, 4 * i, rd16(q, 2 * i) as i16 as i32 as u32);
            }
            r
        }
        LB_GETITEMRECT | CB_GETDROPPEDCONTROLRECT if lp != 0 => {
            let (far16, q) = s.get(8);
            let r = f(m16, wp as u16, far16);
            let mut rr = [0; 4];
            rect16to32(q, &mut rr);
            put_rect32(p, &rr);
            r
        }
        _ => f(m16, wp as u16, lp),
    }
}

// ---- Calling 16-bit procedures ----

/// Call a 16-bit window procedure with a 16-bit message: WIN16's own
/// procedures (DDEML's, the Program Manager stand-in's) in Rust.
pub fn raw16(p: u32, h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    if m == crate::dde::WM_DDE_ACK {
        crate::dde::note_ack();
    }
    if let Some(r) = crate::user::win16_native(p, h, m, wp, lp) {
        return r;
    }
    if crate::tracing() {
        crate::trace(format_args!("W16 msg {h:#x} {m:#x} {wp:#x} {lp:#x}"));
    }
    PascalCall::new().w(h).w(m).w(wp).l(lp).call(p)
}

/// The host's call of a 16-bit procedure (kern.rs in W32USER): `kind` 0
/// a window, dialog or timer procedure, 1 a hook, 2 an abort procedure.
pub unsafe extern "C" fn host_call16(kind: u32, p: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    match kind {
        0 => {
            let h = a as u16;
            to16(h, b, c, d, &mut |m, wp, lp| raw16(p, h, m, wp, lp))
        }
        1 => hook16(p, a as i32, b as i32, c, d),
        2 => PascalCall::new().w(a as u16).w(b as u16).call(p) & 0xffff,
        3 => enum_font16(p, a, b, c, d),
        _ => 0,
    }
}

/// A 16-bit font enumeration callback (LOGFONT, TEXTMETRIC, type, data)
/// with ENUMLOGFONTA and NEWTEXTMETRICA as ENUMLOGFONT16 (146 bytes) and
/// NEWTEXTMETRIC16 (41).
fn enum_font16(p: u32, lf: u32, tm: u32, ty: u32, data: u32) -> u32 {
    let s = Scratch::new();
    let (lf16, q) = s.get(146);
    let (tm16, t) = s.get(44);
    if q.is_null() || t.is_null() {
        return 0;
    }
    let (lf, tm) = (lf as *const u8, tm as *const u8);
    crate::gdi::logfont16(lf, q);
    unsafe { core::ptr::copy_nonoverlapping(lf.add(60), q.add(50), 96) };
    crate::gdi::textmetric16(tm, t);
    wr32(t, 31, rd32(tm, 56));
    wr16(t, 35, rd32(tm, 60) as u16);
    wr16(t, 37, rd32(tm, 64) as u16);
    wr16(t, 39, rd32(tm, 68) as u16);
    PascalCall::new().l(lf16).l(tm16).w(ty as u16).l(data).call(p) & 0xffff
}

/// A 16-bit hook procedure (code, wParam, lParam) with its 32-bit
/// structures as 16-bit ones.
fn hook16(p: u32, kind: i32, code: i32, wp: u32, lp: u32) -> u32 {
    let s = Scratch::new();
    let call = |lp16: u32| PascalCall::new().w(code as u16).w(wp as u16).l(lp16).call(p);
    match kind {
        // WH_CALLWNDPROC: CWPSTRUCT (lParam, wParam, message, hwnd).
        4 if lp != 0 => {
            let q32 = lp as *const u8;
            let (h, m, w, l) = (rd32(q32, 12) as u16, rd32(q32, 8), rd32(q32, 4), rd32(q32, 0));
            let (m16, w16, l16) = posted32to16(h, m, w, l);
            let (far16, q) = s.get(10);
            wr32(q, 0, l16);
            wr16(q, 4, w16);
            wr16(q, 6, m16);
            wr16(q, 8, h);
            call(far16)
        }
        // WH_GETMESSAGE, WH_MSGFILTER: a MSG (a hook may change it).
        3 | -1 if lp != 0 => {
            let (far16, q) = s.get(18);
            msg32to16(lp as *const u8, q);
            let r = call(far16);
            let mut m = [0u32; 7];
            msg16to32(q, &mut m);
            for (i, v) in m.iter().enumerate() {
                wr32(lp as *mut u8, 4 * i, *v);
            }
            r
        }
        _ => call(lp),
    }
}
