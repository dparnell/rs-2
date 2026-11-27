//! The built-in controls, drawn in Windows 3.1's style through WIN16's
//! GDI: BUTTON and STATIC here, EDIT in edit.rs, LISTBOX and COMBOBOX in
//! listbox.rs, SCROLLBAR in scroll.rs. Each is a window procedure in
//! Rust (`proc_`, by entry number) with its state in the window's
//! `data`; colours come from the parent's WM_CTLCOLOR, fonts from
//! WM_SETFONT, notifications go to the parent as WM_COMMAND.

use alloc::vec::Vec;

use crate::dialog::*;
use crate::gdi;
use crate::wnd::*;


pub const CTLCOLOR_EDIT: u16 = W_CTLCOLOREDIT;
pub const CTLCOLOR_LISTBOX: u16 = W_CTLCOLORLISTBOX;
pub const CTLCOLOR_BTN: u16 = W_CTLCOLORBTN;
pub const CTLCOLOR_STATIC: u16 = W_CTLCOLORSTATIC;

/// The parent's WM_CTLCOLORxxx (`kind`) for a control: the brush (the
/// DC gets the colours).
pub fn ctl_color(h: u16, hdc: u16, kind: u16) -> u16 {
    let parent = wnd(h).map_or(0, |w| w.parent);
    let target = if parent != 0 { parent } else { h };
    let (fg, bg) = if kind == CTLCOLOR_EDIT || kind == CTLCOLOR_LISTBOX { (gdi::COLOR_WINDOWTEXT, gdi::COLOR_WINDOW) } else { (gdi::COLOR_BTNTEXT, gdi::COLOR_BTNFACE) };
    gdi::set_text_color(hdc, gdi::sys_color(fg));
    gdi::set_bk_color(hdc, gdi::sys_color(bg));
    let br = send(target, kind, hdc as u32, h as u32) as u16;
    if br == 0 { gdi::sys_brush(bg) } else { br }
}

/// A control's text (WM_GETTEXT).
pub fn text_of(h: u16) -> Vec<u8> {
    if wnd(h).is_none() {
        return Vec::new();
    }
    let n = send(h, W_GETTEXTLENGTH, 0, 0) as usize;
    let mut b = alloc::vec![0u8; n + 2];
    let got = send(h, W_GETTEXT, (n + 1) as u32, b.as_mut_ptr() as u32) as usize;
    b.truncate(got.min(n + 1));
    b
}

/// Text without its "&" prefixes, and the position of the underlined
/// character.
pub fn strip_prefix(t: &[u8]) -> (Vec<u8>, Option<usize>) {
    let mut v = Vec::with_capacity(t.len());
    let mut u = None;
    let mut i = 0;
    while i < t.len() {
        if t[i] == b'&' && i + 1 < t.len() {
            if t[i + 1] != b'&' && u.is_none() {
                u = Some(v.len());
            }
            i += 1;
        }
        v.push(t[i]);
        i += 1;
    }
    (v, u)
}

/// Text with its mnemonic underlined, at (x, y); the width.
pub fn label(hdc: u16, x: i32, y: i32, t: &[u8], prefix: bool) -> i32 {
    let (s, u) = if prefix { strip_prefix(t) } else { (t.to_vec(), None) };
    gdi::text_at(hdc, x, y, &s);
    if let Some(i) = u {
        let ux = x + gdi::text_width(hdc, &s[..i]);
        let uw = gdi::text_width(hdc, &s[i..i + 1]);
        let lh = gdi::line_height(hdc);
        gdi::fill(hdc, [ux, y + lh - 2, ux + uw, y + lh - 1], gdi::text_color(hdc));
    }
    gdi::text_width(hdc, &s)
}

/// The width of a label's text.
pub fn label_width(hdc: u16, t: &[u8]) -> i32 {
    gdi::text_width(hdc, &strip_prefix(t).0)
}

/// rs2's application icon (DrawIcon, SS_ICON): a window with a title
/// bar.
pub fn draw_icon(hdc: u16, x: i32, y: i32) {
    gdi::fill(hdc, [x + 2, y + 4, x + 30, y + 28], 0xffffff);
    gdi::frame(hdc, [x + 2, y + 4, x + 30, y + 28], 0);
    gdi::fill(hdc, [x + 3, y + 5, x + 29, y + 10], 0x800000);
    gdi::fill(hdc, [x + 6, y + 13, x + 26, y + 14], 0x808080);
    gdi::fill(hdc, [x + 6, y + 17, x + 22, y + 18], 0x808080);
    gdi::fill(hdc, [x + 6, y + 21, x + 24, y + 22], 0x808080);
}

/// WM_DRAWITEM with a DRAWITEMSTRUCT: CtlType, CtlID, itemID,
/// itemAction, itemState, hwndItem, hDC, rcItem, itemData.
pub fn draw_item(h: u16, kind: u16, item: u32, action: u16, state: u16, hdc: u16, r: [i32; 4], data: u32) {
    let Some((parent, id)) = wnd(h).map(|w| (w.parent, w.id)) else { return };
    let di: [u32; 12] = [kind as u32, id, item, action as u32, state as u32, h as u32, hdc as u32, r[0] as u32, r[1] as u32, r[2] as u32, r[3] as u32, data];
    send(parent, W_DRAWITEM, id, di.as_ptr() as u32);
}

// ---- BUTTON ----

const BS_PUSHBUTTON: u32 = 0;
const BS_DEFPUSHBUTTON: u32 = 1;
const BS_AUTOCHECKBOX: u32 = 3;
const BS_RADIOBUTTON: u32 = 4;
const BS_3STATE: u32 = 5;
const BS_AUTO3STATE: u32 = 6;
const BS_GROUPBOX: u32 = 7;
const BS_USERBUTTON: u32 = 8;
const BS_AUTORADIOBUTTON: u32 = 9;
const BS_OWNERDRAW: u32 = 0xb;
const BS_LEFTTEXT: u32 = 0x20;

const BM_GETCHECK: u16 = 0x00f0;
const BM_SETCHECK: u16 = 0x00f0 + 1;
const BM_GETSTATE: u16 = 0x00f0 + 2;
const BM_SETSTATE: u16 = 0x00f0 + 3;
const BM_SETSTYLE: u16 = 0x00f0 + 4;

const BN_CLICKED: u16 = 0;
const BN_DOUBLECLICKED: u16 = 5;

#[derive(Default)]
struct Button {
    check: u16,
    pushed: bool,
    focus: bool,
    captured: bool,
    font: u16,
}

fn btn(h: u16) -> Option<&'static mut Button> {
    data::<Button>(h)
}

fn kind(h: u16) -> u32 {
    wnd(h).map_or(0, |w| w.style & 0x0f)
}

fn style(h: u16) -> u32 {
    wnd(h).map_or(0, |w| w.style)
}

fn is_push(k: u32) -> bool {
    k == BS_PUSHBUTTON || k == BS_DEFPUSHBUTTON || k == BS_USERBUTTON
}

fn is_radio(k: u32) -> bool {
    k == BS_RADIOBUTTON || k == BS_AUTORADIOBUTTON
}

/// A click: the auto buttons change, the parent gets BN_CLICKED.
fn click(h: u16) {
    let k = kind(h);
    match k {
        BS_AUTOCHECKBOX => {
            let c = btn(h).map_or(0, |b| b.check);
            set_check(h, if c == 0 { 1 } else { 0 });
        }
        BS_AUTO3STATE => {
            let c = btn(h).map_or(0, |b| b.check);
            set_check(h, (c + 1) % 3);
        }
        BS_AUTORADIOBUTTON => check_radio(h),
        _ => {}
    }
    notify(h, BN_CLICKED);
}

/// Check an auto radio button and uncheck the others of its group.
fn check_radio(h: u16) {
    let parent = wnd(h).map_or(0, |w| w.parent);
    let cs = children(parent);
    let Some(i) = cs.iter().position(|&c| c == h) else { return };
    let group = |c: u16| wnd(c).is_some_and(|w| w.style & WS_GROUP != 0);
    let mut s = i;
    while s > 0 && !group(cs[s]) {
        s -= 1;
    }
    let mut e = i + 1;
    while e < cs.len() && !group(cs[e]) {
        e += 1;
    }
    for &c in &cs[s..e] {
        if c != h && kind(c) == BS_AUTORADIOBUTTON && btn(c).is_some_and(|b| b.check != 0) {
            set_check(c, 0);
        }
    }
    set_check(h, 1);
}

fn set_check(h: u16, v: u16) {
    let k = kind(h);
    let Some(b) = btn(h) else { return };
    let v = if k == BS_3STATE || k == BS_AUTO3STATE { v.min(2) } else { v.min(1) };
    if b.check != v {
        b.check = v;
        invalidate(h, None);
    }
    // The checked auto radio button is the group's tab stop.
    if k == BS_AUTORADIOBUTTON {
        if let Some(w) = wnd(h) {
            if v != 0 {
                w.style |= WS_TABSTOP;
            } else {
                w.style &= !WS_TABSTOP;
            }
        }
    }
}

/// The dialog manager moved the focus to a radio button with an arrow.
pub fn radio_arrive(h: u16) {
    match kind(h) {
        BS_AUTORADIOBUTTON => {
            if btn(h).is_some_and(|b| b.check == 0) {
                click(h);
            }
        }
        BS_RADIOBUTTON => notify(h, BN_CLICKED),
        _ => {}
    }
}

/// A mnemonic of a dialog control: a button is clicked (a radio button
/// or check box gets the focus too), another control gets the focus.
pub fn mnemonic_hit(dlg_h: u16, c: u16) {
    let code = send(c, W_GETDLGCODE, 0, 0);
    if code & DLGC_BUTTON != 0 && code & DLGC_STATIC == 0 {
        if code & (DLGC_DEFPUSHBUTTON | DLGC_UNDEFPUSHBUTTON) == 0 {
            set_focus(c);
            check_default(dlg_h, c);
        }
        click(c);
    } else {
        focus_control(dlg_h, c);
    }
}

pub fn button(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    let k = kind(h);
    match m {
        W_CREATE => {
            if let Some(w) = wnd(h) {
                w.data = Some(alloc::boxed::Box::new(Button::default()));
            }
            if k == BS_GROUPBOX {
                see_through(h);
            }
            0
        }
        // Group boxes let the mouse through to their parent, as Windows'.
        W_NCHITTEST if k == BS_GROUPBOX => 0xffff_ffff,
        W_ERASEBKGND => 1,
        W_PAINT => {
            let hdc = begin_paint_rs(h);
            paint_button(h, hdc);
            gdi::release_dc(hdc);
            0
        }
        W_SETTEXT => {
            let r = def_window_proc(h, m, wp, lp);
            invalidate(h, None);
            r
        }
        W_ENABLE => {
            invalidate(h, None);
            0
        }
        W_SETFONT => {
            if let Some(b) = btn(h) {
                b.font = wp as u16;
            }
            if lp & 0xffff != 0 {
                invalidate(h, None);
            }
            0
        }
        W_GETFONT => btn(h).map_or(0, |b| b.font as u32),
        W_GETDLGCODE => match k {
            BS_DEFPUSHBUTTON => DLGC_BUTTON | DLGC_DEFPUSHBUTTON,
            BS_PUSHBUTTON | BS_USERBUTTON => DLGC_BUTTON | DLGC_UNDEFPUSHBUTTON,
            // An owner-drawn button is not made the default (it would be
            // restyled as a push button).
            BS_RADIOBUTTON | BS_AUTORADIOBUTTON => DLGC_BUTTON | DLGC_RADIOBUTTON,
            BS_GROUPBOX => DLGC_STATIC,
            _ => DLGC_BUTTON,
        },
        W_SETFOCUS => {
            if let Some(b) = btn(h) {
                b.focus = true;
            }
            invalidate(h, None);
            let parent = wnd(h).map_or(0, |w| w.parent);
            if is_dialog(parent) {
                check_default(parent, h);
            }
            0
        }
        W_KILLFOCUS => {
            if let Some(b) = btn(h) {
                b.focus = false;
                if b.captured || b.pushed {
                    b.pushed = false;
                    if b.captured {
                        b.captured = false;
                        unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, 0) };
                    }
                }
            }
            invalidate(h, None);
            let parent = wnd(h).map_or(0, |w| w.parent);
            if is_dialog(parent) && wp != 0 {
                check_default(parent, wp as u16);
            }
            0
        }
        W_LBUTTONDOWN | W_LBUTTONDBLCLK => {
            if k == BS_GROUPBOX {
                return 0;
            }
            if m == W_LBUTTONDBLCLK && (is_radio(k) || k == BS_USERBUTTON || k == BS_OWNERDRAW) {
                notify(h, BN_DOUBLECLICKED);
                return 0;
            }
            set_focus(h);
            if let Some(c) = wnd(h).map(|w| w.client) {
                unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, c) };
            }
            if let Some(b) = btn(h) {
                b.captured = true;
                b.pushed = true;
            }
            invalidate(h, None);
            0
        }
        W_MOUSEMOVE => {
            if btn(h).is_some_and(|b| b.captured) {
                let inside = inside(h, lp);
                if let Some(b) = btn(h) {
                    if b.pushed != inside {
                        b.pushed = inside;
                        invalidate(h, None);
                    }
                }
            }
            0
        }
        W_LBUTTONUP => {
            let Some(b) = btn(h) else { return 0 };
            if !b.captured {
                return 0;
            }
            b.captured = false;
            let was = b.pushed;
            b.pushed = false;
            unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, 0) };
            invalidate(h, None);
            if was && inside(h, lp) {
                click(h);
            }
            0
        }
        W_KEYDOWN => {
            if wp == 0x20 {
                if let Some(b) = btn(h) {
                    if !b.pushed {
                        b.pushed = true;
                        invalidate(h, None);
                    }
                }
            }
            0
        }
        W_KEYUP => {
            if wp == 0x20 && btn(h).is_some_and(|b| b.pushed) {
                if let Some(b) = btn(h) {
                    b.pushed = false;
                }
                invalidate(h, None);
                click(h);
            }
            0
        }
        W_CHAR => 0,
        BM_GETCHECK => btn(h).map_or(0, |b| b.check as u32),
        BM_SETCHECK => {
            set_check(h, wp as u16);
            0
        }
        BM_GETSTATE => btn(h).map_or(0, |b| b.check as u32 | if b.pushed { 4 } else { 0 } | if b.focus { 8 } else { 0 }),
        BM_SETSTATE => {
            if let Some(b) = btn(h) {
                if b.pushed != (wp != 0) {
                    b.pushed = wp != 0;
                    invalidate(h, None);
                }
            }
            0
        }
        // BM_CLICK
        0x00f5 => {
            click(h);
            0
        }
        BM_SETSTYLE => {
            if let Some(w) = wnd(h) {
                w.style = (w.style & !0xffff) | wp & 0xffff;
            }
            if lp & 0xffff != 0 {
                invalidate(h, None);
            }
            0
        }
        _ => def_window_proc(h, m, wp, lp),
    }
}

fn inside(h: u16, lp: u32) -> bool {
    let (x, y) = (lp as i16 as i32, (lp >> 16) as i16 as i32);
    let (cx, cy) = client_size(h);
    x >= 0 && y >= 0 && x < cx && y < cy
}

/// The pels of a radio button's circle (12 x 12) per row: (from, to).
const CIRCLE: [(i32, i32); 12] = [(4, 8), (2, 10), (1, 11), (1, 11), (0, 12), (0, 12), (0, 12), (0, 12), (1, 11), (1, 11), (2, 10), (4, 8)];
const DOT: [(i32, i32); 4] = [(5, 7), (4, 8), (4, 8), (5, 7)];

fn paint_button(h: u16, hdc: u16) {
    let k = kind(h);
    let st = style(h);
    let (cx, cy) = client_size(h);
    let Some((check, pushed, focus, font)) = btn(h).map(|b| (b.check, b.pushed, b.focus, b.font)) else { return };
    gdi::select_font(hdc, font);
    let enabled = st & WS_DISABLED == 0;
    let text = window_text(h);
    if k == BS_OWNERDRAW {
        let state = if pushed { 1 } else { 0 } | if !enabled { 4 } else { 0 } | if focus { 0x10 } else { 0 };
        draw_item(h, 4, 0, 1, state, hdc, [0, 0, cx, cy], 0);
        return;
    }
    let lh = gdi::line_height(hdc);
    if is_push(k) {
        // Windows 3.1: a black outline with its corners cut, a thicker
        // one for the default button, a raised face (sunken when
        // pushed).
        ctl_color(h, hdc, CTLCOLOR_BTN);
        let def = k == BS_DEFPUSHBUTTON;
        let black = gdi::sys_color(gdi::COLOR_WINDOWFRAME);
        gdi::fill(hdc, [0, 0, cx, cy], gdi::sys_color(gdi::COLOR_BTNFACE));
        let o = if def { 2 } else { 1 };
        gdi::fill(hdc, [1, 0, cx - 1, o], black);
        gdi::fill(hdc, [1, cy - o, cx - 1, cy], black);
        gdi::fill(hdc, [0, 1, o, cy - 1], black);
        gdi::fill(hdc, [cx - o, 1, cx, cy - 1], black);
        // Background pels at the corners.
        let bk = gdi::sys_color(gdi::COLOR_WINDOW);
        for (x, y) in [(0, 0), (cx - 1, 0), (0, cy - 1), (cx - 1, cy - 1)] {
            gdi::fill(hdc, [x, y, x + 1, y + 1], bk);
        }
        let inner = [o, o, cx - o, cy - o];
        let shadow = gdi::sys_color(gdi::COLOR_BTNSHADOW);
        if pushed {
            gdi::fill(hdc, [inner[0], inner[1], inner[2], inner[1] + 1], shadow);
            gdi::fill(hdc, [inner[0], inner[1], inner[0] + 1, inner[3]], shadow);
        } else {
            gdi::fill(hdc, [inner[0], inner[1], inner[2] - 1, inner[1] + 1], 0xffffff);
            gdi::fill(hdc, [inner[0], inner[1], inner[0] + 1, inner[3] - 1], 0xffffff);
            gdi::fill(hdc, [inner[0] + 1, inner[1] + 1, inner[2] - 2, inner[1] + 2], 0xffffff);
            gdi::fill(hdc, [inner[0] + 1, inner[1] + 1, inner[0] + 2, inner[3] - 2], 0xffffff);
            gdi::fill(hdc, [inner[0], inner[3] - 2, inner[2], inner[3]], shadow);
            gdi::fill(hdc, [inner[2] - 2, inner[1], inner[2], inner[3]], shadow);
        }
        let tw = label_width(hdc, &text);
        let off = pushed as i32;
        let (tx, ty) = ((cx - tw) / 2 + off, (cy - lh) / 2 + off);
        gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
        gdi::set_text_color(hdc, gdi::sys_color(if enabled { gdi::COLOR_BTNTEXT } else { gdi::COLOR_GRAYTEXT }));
        label(hdc, tx, ty, &text, true);
        if focus {
            gdi::focus_rect(hdc, [tx - 2, ty - 1, tx + tw + 2, ty + lh + 1].map(|v| v.max(0)));
        }
        return;
    }
    let br = ctl_color(h, hdc, CTLCOLOR_BTN);
    // A group box is its frame and title only.
    if k != BS_GROUPBOX {
        gdi::fill_rect(hdc, [0, 0, cx, cy], br);
    }
    gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
    if !enabled {
        gdi::set_text_color(hdc, gdi::sys_color(gdi::COLOR_GRAYTEXT));
    }
    if k == BS_GROUPBOX {
        let tw = label_width(hdc, &text);
        let y = lh / 2;
        gdi::frame(hdc, [0, y, cx, cy], gdi::sys_color(gdi::COLOR_WINDOWFRAME));
        if !text.is_empty() {
            gdi::fill_rect(hdc, [6, 0, 6 + tw + 4, lh], br);
            label(hdc, 8, 0, &text, true);
        }
        return;
    }
    // Check boxes and radio buttons: the box (13 x 13) and the text.
    let bs = 13;
    let left_text = st & BS_LEFTTEXT != 0;
    let bx = if left_text { cx - bs } else { 0 };
    let by = (cy - bs) / 2;
    let fg = if enabled { 0 } else { gdi::sys_color(gdi::COLOR_GRAYTEXT) };
    let face = if pushed { gdi::sys_color(gdi::COLOR_BTNFACE) } else { 0xffffff };
    if is_radio(k) {
        let (x0, y0) = (bx, by);
        for (row, (a, b)) in CIRCLE.iter().enumerate() {
            let y = y0 + row as i32;
            gdi::fill(hdc, [x0 + a, y, x0 + b, y + 1], fg);
            if b - a > 2 && row > 0 && row < 11 {
                gdi::fill(hdc, [x0 + a + 1, y, x0 + b - 1, y + 1], face);
            }
        }
        if check != 0 {
            for (i, (a, b)) in DOT.iter().enumerate() {
                let y = y0 + 4 + i as i32;
                gdi::fill(hdc, [x0 + a, y, x0 + b, y + 1], fg);
            }
        }
    } else {
        let r = [bx, by, bx + bs, by + bs];
        gdi::fill(hdc, r, if check == 2 { gdi::sys_color(gdi::COLOR_BTNFACE) } else { face });
        gdi::frame(hdc, r, fg);
        if check == 1 {
            for i in 2..bs - 2 {
                gdi::fill(hdc, [bx + i, by + i, bx + i + 1, by + i + 1], fg);
                gdi::fill(hdc, [bx + bs - 1 - i, by + i, bx + bs - i, by + i + 1], fg);
            }
        }
    }
    let tx = if left_text { 0 } else { bs + 5 };
    let ty = (cy - lh) / 2;
    let tw = label(hdc, tx, ty, &text, true);
    if focus && !text.is_empty() {
        gdi::focus_rect(hdc, [tx - 1, ty, (tx + tw + 1).min(cx), (ty + lh).min(cy)]);
    }
}

// ---- STATIC ----

const SS_CENTER: u32 = 1;
const SS_RIGHT: u32 = 2;
const SS_ICON: u32 = 3;
const SS_BLACKRECT: u32 = 4;
const SS_GRAYRECT: u32 = 5;
const SS_WHITERECT: u32 = 6;
const SS_BLACKFRAME: u32 = 7;
const SS_GRAYFRAME: u32 = 8;
const SS_WHITEFRAME: u32 = 9;
const SS_SIMPLE: u32 = 0xb;
const SS_LEFTNOWORDWRAP: u32 = 0xc;
const SS_NOPREFIX: u32 = 0x80;

const SS_BITMAP: u32 = 0xe;
const SS_ETCHEDHORZ: u32 = 0x10;
const SS_ETCHEDVERT: u32 = 0x11;
const SS_ETCHEDFRAME: u32 = 0x12;
const SS_NOTIFY: u32 = 0x100;
const SS_SUNKEN: u32 = 0x1000;

#[derive(Default)]
struct Static {
    font: u16,
    /// The icon (SS_ICON) or bitmap (SS_BITMAP).
    icon: u32,
}

/// A control that draws only a frame or text (a group box, a frame
/// static) shows the windows beneath it, as on Windows.
fn see_through(h: u16) {
    if let Some(f) = wnd(h).map(|w| w.frame) {
        unsafe { crate::pm::WinSetWindowBits(f, crate::pm::QWL_STYLE, crate::pm::TRANSPARENT, crate::pm::TRANSPARENT) };
    }
}

pub fn static_(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    match m {
        W_CREATE => {
            if let Some(w) = wnd(h) {
                w.data = Some(alloc::boxed::Box::new(Static::default()));
                let k = w.style & 0x1f;
                if k == SS_ICON || k == SS_BITMAP {
                    // An icon or bitmap static shows the image its text
                    // names (0xFF and the resource's number from a dialog
                    // template); an icon static is the icon's size.
                    let name_p = crate::rd32(lp, 36);
                    let t: Vec<u8> = if name_p >> 16 == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(name_p as *const u8, 3) }.to_vec() };
                    let t = if t.first() == Some(&0xff) { t } else { w.text.clone() };
                    let hinst = w.hinst;
                    let name = match t.first() {
                        Some(0xff) => Some(crate::res::Name::Id(u16::from_le_bytes([t.get(1).copied().unwrap_or(0), t.get(2).copied().unwrap_or(0)]))),
                        Some(_) => Some(crate::res::Name::Str(crate::to_wide(&t))),
                        None => None,
                    };
                    let img = match (name, k) {
                        (Some(n), SS_ICON) => crate::icon::load(hinst, &n, false) as u32,
                        (Some(n), _) => crate::res::find(hinst, crate::res::RT_BITMAP, &n).map_or(0, |d| crate::bitmap::from_dib(d) as u32),
                        _ => 0,
                    };
                    if let Some(s) = data::<Static>(h) {
                        s.icon = img;
                    }
                    if k == SS_ICON {
                        let r = window_rect(h);
                        move_window(h, r[0], r[1], 32, 32, crate::pm::SWP_MOVE | crate::pm::SWP_SIZE);
                    }
                }
            }
            if matches!(style(h) & 0x0f, SS_BLACKFRAME | SS_GRAYFRAME | SS_WHITEFRAME) {
                see_through(h);
            }
            0
        }
        W_ERASEBKGND => 1,
        // Statics let the mouse through to their parent (HTTRANSPARENT),
        // as Windows' do, unless they notify (SS_NOTIFY).
        W_NCHITTEST if style(h) & SS_NOTIFY == 0 => 0xffff_ffff,
        W_NCHITTEST => 1,
        W_LBUTTONDOWN => {
            notify(h, 0); // STN_CLICKED
            0
        }
        W_LBUTTONDBLCLK => {
            notify(h, 1); // STN_DBLCLK
            0
        }
        W_PAINT => {
            let hdc = begin_paint_rs(h);
            paint_static(h, hdc);
            gdi::release_dc(hdc);
            0
        }
        W_SETTEXT => {
            let r = def_window_proc(h, m, wp, lp);
            invalidate(h, None);
            r
        }
        W_ENABLE => {
            invalidate(h, None);
            0
        }
        W_SETFONT => {
            if let Some(s) = data::<Static>(h) {
                s.font = wp as u16;
            }
            if lp & 0xffff != 0 {
                invalidate(h, None);
            }
            0
        }
        W_GETFONT => data::<Static>(h).map_or(0, |s| s.font as u32),
        W_GETDLGCODE => DLGC_STATIC,
        // STM_SETICON, STM_GETICON, STM_SETIMAGE (type, image),
        // STM_GETIMAGE
        0x0170 | 0x0172 => {
            let v = if m == 0x0170 { wp } else { lp };
            let old = data::<Static>(h).map_or(0, |s| core::mem::replace(&mut s.icon, v));
            invalidate(h, None);
            old
        }
        0x0171 | 0x0173 => data::<Static>(h).map_or(0, |s| s.icon),
        _ => def_window_proc(h, m, wp, lp),
    }
}

fn paint_static(h: u16, hdc: u16) {
    let st = style(h);
    let (cx, cy) = client_size(h);
    let font = data::<Static>(h).map_or(0, |s| s.font);
    gdi::select_font(hdc, font);
    let r = [0, 0, cx, cy];
    let frame = gdi::sys_color(gdi::COLOR_WINDOWFRAME);
    let gray = 0x808080;
    let white = 0xffffff;
    if st & SS_SUNKEN != 0 {
        let sh = gdi::sys_color(gdi::COLOR_BTNSHADOW);
        gdi::fill(hdc, [0, 0, cx, 1], sh);
        gdi::fill(hdc, [0, 0, 1, cy], sh);
        gdi::fill(hdc, [0, cy - 1, cx, cy], 0xffffff);
        gdi::fill(hdc, [cx - 1, 0, cx, cy], 0xffffff);
    }
    match st & 0x1f {
        SS_ETCHEDHORZ | SS_ETCHEDVERT | SS_ETCHEDFRAME => {
            let sh = gdi::sys_color(gdi::COLOR_BTNSHADOW);
            if st & 0x1f != SS_ETCHEDVERT {
                gdi::fill(hdc, [0, 0, cx, 1], sh);
                gdi::fill(hdc, [0, 1, cx, 2], white);
            }
            if st & 0x1f != SS_ETCHEDHORZ {
                gdi::fill(hdc, [0, 0, 1, cy], sh);
                gdi::fill(hdc, [1, 0, 2, cy], white);
            }
            if st & 0x1f == SS_ETCHEDFRAME {
                gdi::fill(hdc, [0, cy - 2, cx, cy - 1], sh);
                gdi::fill(hdc, [0, cy - 1, cx, cy], white);
                gdi::fill(hdc, [cx - 2, 0, cx - 1, cy], sh);
                gdi::fill(hdc, [cx - 1, 0, cx, cy], white);
            }
        }
        SS_BITMAP => {
            let br = ctl_color(h, hdc, CTLCOLOR_STATIC);
            gdi::fill_rect(hdc, r, br);
            let b = data::<Static>(h).map_or(0, |s| s.icon) as u16;
            if let Some((_, bw, bh, _)) = crate::bitmap::bitmap_of(b) {
                let mem = crate::bitmap::create_memory_dc();
                crate::bitmap::select(mem, b);
                crate::bitmap::blit(hdc, 0, 0, bw, bh, mem, 0, 0, 0x00cc_0020);
                crate::bitmap::deselect(mem);
                gdi::release_dc(mem);
            }
        }
        SS_BLACKRECT => gdi::fill(hdc, r, frame),
        SS_GRAYRECT => gdi::fill(hdc, r, gray),
        SS_WHITERECT => gdi::fill(hdc, r, white),
        // A frame is only its border (the controls inside show through).
        SS_BLACKFRAME | SS_GRAYFRAME | SS_WHITEFRAME => {
            let c = match st & 0x0f {
                SS_BLACKFRAME => frame,
                SS_GRAYFRAME => gray,
                _ => white,
            };
            gdi::frame(hdc, r, c);
        }
        SS_ICON => {
            let br = ctl_color(h, hdc, CTLCOLOR_STATIC);
            gdi::fill_rect(hdc, r, br);
            let icon = data::<Static>(h).map_or(0, |s| s.icon);
            if icon == 0 || !crate::icon::draw(hdc, 0, 0, icon) {
                draw_icon(hdc, 0, 0);
            }
        }
        k => {
            let br = ctl_color(h, hdc, CTLCOLOR_STATIC);
            gdi::fill_rect(hdc, r, br);
            if st & WS_DISABLED != 0 {
                gdi::set_text_color(hdc, gdi::sys_color(gdi::COLOR_GRAYTEXT));
            }
            gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
            let mut fmt = match k {
                SS_CENTER => gdi::DT_CENTER | gdi::DT_WORDBREAK,
                SS_RIGHT => gdi::DT_RIGHT | gdi::DT_WORDBREAK,
                SS_SIMPLE | SS_LEFTNOWORDWRAP => gdi::DT_SINGLELINE,
                _ => gdi::DT_WORDBREAK,
            } | gdi::DT_EXPANDTABS;
            if st & SS_NOPREFIX != 0 {
                fmt |= gdi::DT_NOPREFIX;
            }
            let text = window_text(h);
            gdi::clip(hdc, r);
            if fmt & gdi::DT_NOPREFIX == 0 && text.contains(&b'&') && fmt & gdi::DT_SINGLELINE != 0 {
                label(hdc, 0, 0, &text, true);
            } else {
                gdi::draw_text_bytes(hdc, &text, r, fmt);
            }
            gdi::unclip(hdc);
        }
    }
}
