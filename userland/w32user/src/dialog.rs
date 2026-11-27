//! Dialog boxes: Windows dialog templates (RT_DIALOG, or in memory),
//! the dialog class (#32770) and DefDlgProc, modal (DialogBox) and
//! modeless (CreateDialog) dialogs, and the dialog manager's keyboard
//! interface (IsDialogMessage: Tab and Shift+Tab through WS_TABSTOP
//! controls, arrows within a WS_GROUP group, Enter for the default push
//! button, Esc for IDCANCEL, Alt+letter and letter mnemonics).
//!
//! A dialog is a top-level window of the dialog class (or the
//! template's class) whose controls are created from the template in
//! dialog units: a quarter of the dialog font's average character width
//! horizontally, an eighth of its height vertically. The dialog
//! procedure is called by DefDlgProc (DWL_DLGPROC in the window's extra
//! bytes); what it returns is the result for most messages, DWL_MSGRESULT
//! the value.

use alloc::vec::Vec;

use crate::res::{self, Name};
use crate::wnd::*;
use crate::{gdi, read_rect, write_rect};

pub const DLGWINDOWEXTRA: u32 = 30;
const DWL_MSGRESULT: i32 = 0;
const DWL_DLGPROC: i32 = 4;

const DS_ABSALIGN: u32 = 0x01;
const DS_SETFONT: u32 = 0x40;
const DS_MODALFRAME: u32 = 0x80;

pub const DM_GETDEFID: u16 = W_USER;
pub const DM_SETDEFID: u16 = W_USER + 1;
const DC_HASDEFID: u32 = 0x534b;

pub const DLGC_WANTARROWS: u32 = 0x0001;
pub const DLGC_WANTTAB: u32 = 0x0002;
pub const DLGC_WANTALLKEYS: u32 = 0x0004;
pub const DLGC_HASSETSEL: u32 = 0x0008;
pub const DLGC_DEFPUSHBUTTON: u32 = 0x0010;
pub const DLGC_UNDEFPUSHBUTTON: u32 = 0x0020;
pub const DLGC_RADIOBUTTON: u32 = 0x0040;
pub const DLGC_WANTCHARS: u32 = 0x0080;
pub const DLGC_STATIC: u32 = 0x0100;
pub const DLGC_BUTTON: u32 = 0x2000;

const IDOK: u16 = 1;
const IDCANCEL: u16 = 2;

/// A dialog's state (its procedure is DWL_DLGPROC, in the window's
/// extra bytes).
pub struct Dlg {
    /// The procedure takes W messages (DialogBoxParamW...).
    pub unicode: bool,
    pub result: i32,
    pub ended: bool,
    pub focus: u16,
    pub def_id: u16,
    /// Dialog base units: the font's average width and height.
    pub base: (i32, i32),
}

fn dlg(h: u16) -> Option<&'static mut Dlg> {
    data::<Dlg>(h)
}

pub fn is_dialog(h: u16) -> bool {
    dlg(h).is_some()
}

/// A dialog's base units (its font's).
pub fn base_units_of(h: u16) -> (i32, i32) {
    dlg(h).map_or_else(base_units, |d| d.base)
}

/// The system font's base units (GetDialogBaseUnits).
pub fn base_units() -> (i32, i32) {
    static B: crate::Global<(i32, i32)> = crate::Global::new((0, 0));
    let b = B.get();
    if b.0 == 0 {
        let hdc = get_dc(0);
        *b = (gdi::ave_width(hdc).max(4), gdi::line_height(hdc).max(8));
        gdi::release_dc(hdc);
    }
    *b
}


/// Focus a control of a dialog as the dialog manager does: an edit
/// control's text is selected.
pub fn focus_control(dlg_h: u16, c: u16) {
    if c == 0 {
        return;
    }
    let code = send(c, W_GETDLGCODE, 0, 0);
    set_focus(c);
    if code & DLGC_HASSETSEL != 0 {
        // EM_SETSEL(0, 0..32767)
        send(c, 0x00b1, 0, 0xffff_ffff);
    }
    check_default(dlg_h, c);
}

/// The push buttons' default look follows the focus: a push button with
/// the focus is the default one, else the dialog's default button.
pub fn check_default(dlg_h: u16, focus: u16) {
    let Some(def_id) = dlg(dlg_h).map(|d| d.def_id) else { return };
    let focus_push = is_child(dlg_h, focus) && send(focus, W_GETDLGCODE, 0, 0) & (DLGC_DEFPUSHBUTTON | DLGC_UNDEFPUSHBUTTON) != 0;
    for c in children(dlg_h) {
        let code = send(c, W_GETDLGCODE, 0, 0);
        if code & (DLGC_DEFPUSHBUTTON | DLGC_UNDEFPUSHBUTTON) == 0 {
            continue;
        }
        let id = wnd(c).map_or(0, |w| w.id);
        let want = if focus_push { c == focus } else { id == def_id as u32 };
        let is = code & DLGC_DEFPUSHBUTTON != 0;
        if want != is {
            // BM_SETSTYLE
            send(c, 0x00f4, if want { 1 } else { 0 }, 1);
        }
    }
}

/// Can a control get the focus from the keyboard?
fn usable(c: u16) -> bool {
    wnd(c).is_some_and(|w| w.style & WS_VISIBLE != 0 && w.style & WS_DISABLED == 0)
}

/// The next (or previous) control with WS_TABSTOP after `from`.
pub fn next_tab(h: u16, from: u16, prev: bool) -> u16 {
    let mut cs = children(h);
    if prev {
        cs.reverse();
    }
    let n = cs.len();
    let start = cs.iter().position(|&c| c == from).map_or(0, |i| i + 1);
    for k in 0..n {
        let c = cs[(start + k) % n];
        if wnd(c).is_some_and(|w| w.style & WS_TABSTOP != 0) && usable(c) {
            return c;
        }
    }
    if from == 0 { cs.first().copied().filter(|&c| usable(c)).unwrap_or(0) } else { from }
}

/// The next (or previous) control in `from`'s group (WS_GROUP starts
/// one).
pub fn next_group(h: u16, from: u16, prev: bool) -> u16 {
    let cs = children(h);
    let Some(i) = cs.iter().position(|&c| c == from) else { return from };
    let group = |c: u16| wnd(c).is_some_and(|w| w.style & WS_GROUP != 0);
    let mut start = i;
    while start > 0 && !group(cs[start]) {
        start -= 1;
    }
    let mut end = i + 1;
    while end < cs.len() && !group(cs[end]) {
        end += 1;
    }
    let g = &cs[start..end];
    let n = g.len();
    let j = i - start;
    for k in 1..=n {
        let idx = if prev { (j + n * 2 - k) % n } else { (j + k) % n };
        if usable(g[idx]) {
            return g[idx];
        }
    }
    from
}

/// The control of a mnemonic character (a static's goes to the next
/// control).
fn mnemonic(h: u16, c: u8) -> Option<u16> {
    let c = c.to_ascii_lowercase();
    let cs = children(h);
    for (i, &w) in cs.iter().enumerate() {
        let t = window_text(w);
        let mut k = 0;
        let mut hit = false;
        while k + 1 < t.len() {
            if t[k] == b'&' {
                if t[k + 1] == b'&' {
                    k += 2;
                    continue;
                }
                hit = t[k + 1].to_ascii_lowercase() == c;
                break;
            }
            k += 1;
        }
        if !hit || !usable(w) {
            continue;
        }
        let code = send(w, W_GETDLGCODE, 0, 0);
        if code & DLGC_STATIC != 0 && code & DLGC_BUTTON == 0 {
            return cs[i + 1..].iter().copied().find(|&n| usable(n) && send(n, W_GETDLGCODE, 0, 0) & DLGC_STATIC == 0);
        }
        return Some(w);
    }
    None
}

/// The dialog manager's WM_COMMAND for a button (Enter, Esc, a
/// mnemonic).
fn command(h: u16, id: u16) {
    let c = get_item(h, id);
    if c != 0 && !usable(c) {
        unsafe { crate::pm::WinAlarm(crate::pm::HWND_DESKTOP, 2) };
        return;
    }
    send(h, W_COMMAND, id as u32, c as u32);
}

pub fn get_item(h: u16, id: u16) -> u16 {
    children(h).into_iter().find(|&c| wnd(c).is_some_and(|w| w.id & 0xffff == id as u32)).unwrap_or(0)
}

/// Is a key (Windows virtual key) down?
fn send_key_state(vk: u16) -> bool {
    crate::wnd::key_down(vk)
}

/// DefDlgProc: the dialog procedure first.
pub fn def_dlg_proc(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    def_dlg_proc_form(h, m, wp, lp, false)
}

/// DefDlgProc with the message in the A or W (`wide`) form: the dialog
/// procedure (DWL_DLGPROC) first, in its own form.
pub fn def_dlg_proc_form(h: u16, m: u16, wp: u32, lp: u32, wide: bool) -> u32 {
    let proc_ = get_long(h, DWL_DLGPROC);
    if proc_ != 0 {
        set_long(h, DWL_MSGRESULT, 0);
        let uni = dlg(h).is_some_and(|d| d.unicode);
        let r = call_conv(proc_, uni, h, m, wp, lp, wide);
        if r != 0 {
            return match m {
                W_CTLCOLORMSGBOX..=W_CTLCOLORSTATIC | W_COMPAREITEM | W_VKEYTOITEM | W_CHARTOITEM | W_INITDIALOG | 0x0037 => r,
                _ => get_long(h, DWL_MSGRESULT),
            };
        }
        if wnd(h).is_none() {
            return 0;
        }
    }
    // The defaults take the A form.
    if wide {
        let a = crate::user32::thunk(crate::user32::def_dlg_a());
        return call_conv(a, false, h, m, wp, lp, true);
    }
    match m {
        W_INITDIALOG => 0,
        W_ERASEBKGND => {
            let hdc = wp as u16;
            let br = send(h, W_CTLCOLORDLG, hdc as u32, h as u32) as u16;
            let br = if br == 0 { gdi::sys_brush(gdi::COLOR_WINDOW) } else { br };
            let (cx, cy) = client_size(h);
            gdi::fill_rect(hdc, [0, 0, cx, cy], br);
            1
        }
        W_CLOSE => {
            let c = get_item(h, IDCANCEL);
            if c == 0 || usable(c) {
                send(h, W_COMMAND, IDCANCEL as u32, c as u32);
            }
            0
        }
        W_SETFOCUS => {
            restore_focus(h);
            0
        }
        W_ACTIVATE => {
            if wp & 0xffff != 0 {
                restore_focus(h);
            } else {
                let f = focus();
                if let Some(d) = dlg(h) {
                    if is_child(h, f) {
                        d.focus = f;
                    }
                }
            }
            0
        }
        DM_GETDEFID => dlg(h).map_or(0, |d| DC_HASDEFID << 16 | d.def_id as u32),
        DM_SETDEFID => {
            if let Some(d) = dlg(h) {
                d.def_id = wp as u16;
            }
            check_default(h, focus());
            1
        }
        W_NEXTDLGCTL => {
            let n = if lp & 0xffff != 0 { wp as u16 } else { next_tab(h, focus(), wp != 0) };
            focus_control(h, n);
            0
        }
        W_GETDLGCODE => 0,
        W_SHOWWINDOW => def_window_proc(h, m, wp, lp),
        _ => def_window_proc(h, m, wp, lp),
    }
}

fn restore_focus(h: u16) {
    let saved = dlg(h).map_or(0, |d| d.focus);
    let f = if saved != 0 && wnd(saved).is_some() && usable(saved) { saved } else { next_tab(h, 0, false) };
    if f != 0 && f != focus() {
        set_focus(f);
        check_default(h, f);
    }
}

fn get_long(h: u16, i: i32) -> u32 {
    let Some(w) = wnd(h) else { return 0 };
    let i = i as usize;
    w.extra.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn set_long(h: u16, i: i32, v: u32) {
    let Some(w) = wnd(h) else { return };
    let i = i as usize;
    if let Some(b) = w.extra.get_mut(i..i + 4) {
        b.copy_from_slice(&v.to_le_bytes());
    }
}

/// The integer of a control's text (GetDlgItemInt).
fn parse_int(t: &[u8], signed: bool) -> Option<i32> {
    let t = t.iter().copied().skip_while(|c| *c == b' ').collect::<Vec<u8>>();
    let (neg, digits) = match t.first() {
        Some(b'-') if signed => (true, &t[1..]),
        _ => (false, &t[..]),
    };
    let n = digits.iter().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 || digits[n..].iter().any(|&c| c != b' ') {
        return None;
    }
    let mut v: i64 = 0;
    for &c in &digits[..n] {
        v = v * 10 + (c - b'0') as i64;
        if v > 65535 {
            return None;
        }
    }
    let v = if neg { -v } else { v };
    if signed && !(-32768..=32767).contains(&v) {
        return None;
    }
    Some(v as i32)
}

// ---- Win32 dialog templates ----

const DS_SHELLFONT: u32 = 0x48;
const DS_CENTER: u32 = 0x0800;
const DS_CENTERMOUSE: u32 = 0x1000;
const DS_CONTROL: u32 = 0x0400;

/// The dialog font of a template (DS_SETFONT: point size and face) and
/// its base units: the average width of the letters, the height.
fn dialog_font(points: i32, weight: u16, italic: bool, face: &[u8]) -> (u16, (i32, i32)) {
    let f = gdi::new_obj(gdi::Obj::Font(gdi::font_of(-(points * 96 / 72).max(1), 0, if weight == 0 { 400 } else { weight }, italic, false, 0x22, face)));
    let hdc = crate::bitmap::create_memory_dc();
    gdi::select_font(hdc, f);
    let w = gdi::text_width(hdc, b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz");
    let base = (((w + 26) / 52).max(4), gdi::line_height(hdc).max(8));
    gdi::release_dc(hdc);
    (f, base)
}

/// A control class of a template: an atom (0x80 BUTTON ... 0x85
/// COMBOBOX) or a name.
fn control_class(n: &Name) -> Vec<u8> {
    match n {
        Name::Id(0x80) => b"BUTTON".to_vec(),
        Name::Id(0x81) => b"EDIT".to_vec(),
        Name::Id(0x82) => b"STATIC".to_vec(),
        Name::Id(0x83) => b"LISTBOX".to_vec(),
        Name::Id(0x84) => b"SCROLLBAR".to_vec(),
        Name::Id(0x85) => b"COMBOBOX".to_vec(),
        Name::Id(_) => b"STATIC".to_vec(),
        Name::Str(s) => crate::narrow(s),
    }
}

/// Create a dialog from a template (DLGTEMPLATE, or DLGTEMPLATEEX:
/// version 1, signature 0xFFFF): its window (hidden until the dialog
/// procedure has had WM_INITDIALOG), its controls. `unicode`: the
/// procedure takes W messages.
pub fn create_dialog(hinst: u32, t: &[u8], owner: u16, proc_: u32, param: u32, unicode: bool) -> u16 {
    let ex = res::w16(t, 0) == 1 && res::w16(t, 2) == 0xffff;
    let (style, exstyle, n, dx, dy, dcx, dcy, mut p) = if ex {
        (res::l32(t, 12), res::l32(t, 8), res::w16(t, 16) as usize, res::w16(t, 18) as i16 as i32, res::w16(t, 20) as i16 as i32, res::w16(t, 22) as i16 as i32, res::w16(t, 24) as i16 as i32, 26)
    } else {
        (res::l32(t, 0), res::l32(t, 4), res::w16(t, 8) as usize, res::w16(t, 10) as i16 as i32, res::w16(t, 12) as i16 as i32, res::w16(t, 14) as i16 as i32, res::w16(t, 16) as i16 as i32, 18)
    };
    let menu = res::sz_or_ord(t, &mut p);
    let class = res::sz_or_ord(t, &mut p);
    let caption = crate::narrow(&res::wsz(t, &mut p));
    let (font, (bx, by)) = if style & DS_SETFONT != 0 {
        let pts = res::w16(t, p) as i32;
        p += 2;
        let (weight, italic) = if ex {
            let w = res::w16(t, p);
            let it = t.get(p + 2).copied().unwrap_or(0) != 0;
            p += 4;
            (w, it)
        } else {
            (400, false)
        };
        let face = crate::narrow(&res::wsz(t, &mut p));
        // MS Shell Dlg is MS Sans Serif.
        let face = if face.eq_ignore_ascii_case(b"MS Shell Dlg") || face.eq_ignore_ascii_case(b"MS Shell Dlg 2") { b"MS Sans Serif".to_vec() } else { face };
        dialog_font(pts, weight, italic, &face)
    } else {
        (0, base_units())
    };
    let px = |v: i32| (v * bx + 2) / 4;
    let py = |v: i32| (v * by + 4) / 8;
    let ci = match &class {
        Name::Str(s) if s.is_empty() => find_class(&ClassName::Name(b"#32770".to_vec()), 0),
        Name::Id(a) => find_class(&ClassName::Atom(*a), hinst),
        Name::Str(s) => find_class(&ClassName::Name(crate::narrow(s)), hinst),
    };
    let Some(ci) = ci else { return crate::fail(1407, 0) };
    let mut wstyle = style & !(WS_VISIBLE | DS_SETFONT | DS_ABSALIGN | DS_MODALFRAME | DS_CENTER | DS_CENTERMOUSE | DS_CONTROL | 0x0f);
    let mut wex = exstyle;
    if style & DS_MODALFRAME != 0 {
        wex |= WS_EX_DLGMODALFRAME;
    }
    if wstyle & WS_CHILD == 0 {
        wstyle |= WS_POPUP;
    }
    let child = wstyle & WS_CHILD != 0;
    let has_menu = !matches!(&menu, Name::Str(s) if s.is_empty());
    let (l, tp, r, b) = frame_insets(wstyle, wex, has_menu);
    let (cx, cy) = (px(dcx), py(dcy));
    let (w, hh) = (cx + l + r, cy + tp + b);
    // The position: in the owner's client area (DS_ABSALIGN: the
    // screen); DS_CENTER: the middle of the screen.
    let (scx, scy) = screen_size();
    let (mut x, mut y) = (px(dx), py(dy));
    if style & (DS_CENTER | DS_CENTERMOUSE) != 0 && !child {
        x = (scx - w) / 2;
        y = (scy - hh) / 2;
    } else if !child && style & DS_ABSALIGN == 0 && owner != 0 {
        let o = top_level(owner);
        let (sx, sy) = to_screen(o, 0, 0);
        x += sx;
        y += sy;
    }
    if !child {
        x = x.clamp(0, (scx - w).max(0));
        y = y.clamp(0, (scy - hh).max(0));
    }
    let hmenu = if has_menu { crate::menu::load(hinst, &menu) as u32 } else { 0 };
    let h = create(&Create { exstyle: wex, class: ci, title: caption, title_arg: 0, class_arg: 0, wide: false, style: wstyle, pos: [x, y, w, hh], parent: owner, menu: hmenu, hinst, param });
    if h == 0 {
        return 0;
    }
    let (ccx, ccy) = client_size(h);
    if (ccx, ccy) != (cx, cy) {
        let wr = window_rect(h);
        move_window(h, wr[0], wr[1], wr[2] - wr[0] + cx - ccx, wr[3] - wr[1] + cy - ccy, crate::pm::SWP_MOVE | crate::pm::SWP_SIZE);
    }
    if let Some(wd) = wnd(h) {
        wd.data = Some(alloc::boxed::Box::new(Dlg { unicode, result: 0, ended: false, focus: 0, def_id: IDOK, base: (bx, by) }));
        if wd.extra.len() >= 8 {
            wd.extra[4..8].copy_from_slice(&proc_.to_le_bytes());
        }
    }
    if font != 0 {
        send(h, W_SETFONT, font as u32, 0);
    }
    // The controls, DWORD-aligned.
    let mut def_id = 0;
    for _ in 0..n {
        p = (p + 3) & !3;
        let (istyle, iex, ix, iy, icx, icy, id) = if ex {
            if p + 24 > t.len() {
                break;
            }
            let v = (res::l32(t, p + 8), res::l32(t, p + 4), res::w16(t, p + 12) as i16 as i32, res::w16(t, p + 14) as i16 as i32, res::w16(t, p + 16) as i16 as i32, res::w16(t, p + 18) as i16 as i32, res::l32(t, p + 20));
            p += 24;
            v
        } else {
            if p + 18 > t.len() {
                break;
            }
            let v = (res::l32(t, p), res::l32(t, p + 4), res::w16(t, p + 8) as i16 as i32, res::w16(t, p + 10) as i16 as i32, res::w16(t, p + 12) as i16 as i32, res::w16(t, p + 14) as i16 as i32, res::w16(t, p + 16) as u32);
            p += 18;
            v
        };
        let cls = control_class(&res::sz_or_ord(t, &mut p));
        // An ordinal text (an icon static's icon) reaches the control as
        // 0xFF and the number.
        let text = match res::sz_or_ord(t, &mut p) {
            Name::Str(s) => crate::narrow(&s),
            Name::Id(v) => [&[0xff][..], &v.to_le_bytes()].concat(),
        };
        let extra = res::w16(t, p) as usize;
        p += 2 + extra;
        let Some(cci) = find_class(&ClassName::Name(cls.clone()), hinst) else {
            crate::trace(format_args!("W32 dialog: control class {} not found", alloc::string::String::from_utf8_lossy(&cls)));
            continue;
        };
        if cls.eq_ignore_ascii_case(b"BUTTON") && istyle & 0x0f == 1 && def_id == 0 {
            def_id = id as u16;
        }
        let c = create(&Create { exstyle: iex, class: cci, title: text, title_arg: 0, class_arg: 0, wide: false, style: istyle | WS_CHILD, pos: [px(ix), py(iy), px(icx), py(icy)], parent: h, menu: id, hinst, param: 0 });
        if c != 0 && font != 0 {
            send(c, W_SETFONT, font as u32, 0);
        }
    }
    if let Some(d) = dlg(h) {
        d.def_id = if def_id != 0 { def_id } else { IDOK };
    }
    // WM_INITDIALOG: the focus goes to the first tab stop if it says so.
    let first = next_tab(h, 0, false);
    if send(h, W_INITDIALOG, first as u32, param) != 0 && first != 0 {
        focus_control(h, first);
    }
    if wnd(h).is_some() && style & WS_VISIBLE != 0 && !dlg(h).is_some_and(|d| d.ended) {
        show_window(h, 5);
    }
    h
}

/// IsDialogMessage: a dialog's keys (Tab, the arrows, Enter, Esc, the
/// mnemonics); other messages for the dialog are dispatched. Handled?
pub fn is_dialog_message(h: u16, msg: &Msg, wide: bool) -> bool {
    let (mh, m, wp) = (hw(msg.hwnd), msg.msg as u16, msg.wp);
    if wnd(h).is_none() || (mh != h && !is_child(h, mh)) {
        return false;
    }
    let keyish = matches!(m, W_KEYDOWN | W_CHAR | W_SYSCHAR);
    let mut ctl = mh;
    while ctl != h && wnd(ctl).is_some_and(|w| w.parent != h && w.style & WS_CHILD != 0) {
        ctl = wnd(ctl).map_or(h, |w| w.parent);
    }
    if keyish {
        let mut mm = *msg;
        let code = send(mh, W_GETDLGCODE, wp, &mut mm as *mut Msg as u32);
        if code & DLGC_WANTALLKEYS == 0 {
            match m {
                W_KEYDOWN => match wp {
                    0x09 if code & DLGC_WANTTAB == 0 => {
                        let prev = send_key_state(0x10);
                        let n = next_tab(h, ctl, prev);
                        focus_control(h, n);
                        return true;
                    }
                    0x25..=0x28 if code & DLGC_WANTARROWS == 0 => {
                        let n = next_group(h, ctl, wp == 0x25 || wp == 0x26);
                        if n != ctl {
                            set_focus(n);
                            check_default(h, n);
                            if send(n, W_GETDLGCODE, 0, 0) & DLGC_RADIOBUTTON != 0 {
                                crate::ctl::radio_arrive(n);
                            }
                        }
                        return true;
                    }
                    0x0d => {
                        let id = if code & (DLGC_DEFPUSHBUTTON | DLGC_UNDEFPUSHBUTTON) != 0 && mh != h {
                            wnd(mh).map_or(IDOK, |w| w.id as u16)
                        } else {
                            let r = send(h, DM_GETDEFID, 0, 0);
                            if r >> 16 == DC_HASDEFID { r as u16 } else { IDOK }
                        };
                        command(h, id);
                        return true;
                    }
                    0x1b => {
                        command(h, IDCANCEL);
                        return true;
                    }
                    _ => {}
                },
                W_CHAR if code & DLGC_WANTCHARS == 0 && wp > 0x20 => {
                    let c = if wide { crate::narrow(&[wp as u16]).first().copied().unwrap_or(0) } else { wp as u8 };
                    if let Some(c) = mnemonic(h, c) {
                        crate::ctl::mnemonic_hit(h, c);
                        return true;
                    }
                }
                W_SYSCHAR => {
                    let c = if wide { crate::narrow(&[wp as u16]).first().copied().unwrap_or(0) } else { wp as u8 };
                    if let Some(c) = mnemonic(h, c) {
                        crate::ctl::mnemonic_hit(h, c);
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    translate(msg);
    dispatch(msg, wide);
    true
}

/// A modal dialog: runs until EndDialog; the result.
pub fn dialog_box(hinst: u32, t: &[u8], owner: u16, proc_: u32, param: u32, unicode: bool) -> i32 {
    let ot = if owner != 0 { top_level(owner) } else { 0 };
    let h = create_dialog(hinst, t, ot, proc_, param, unicode);
    if h == 0 {
        return -1;
    }
    let reenable = ot != 0 && wnd(ot).is_some_and(|w| w.style & WS_DISABLED == 0);
    if reenable {
        enable_window(ot, false);
    }
    if !dlg(h).is_some_and(|d| d.ended) {
        show_window(h, 5);
    }
    while dlg(h).is_some_and(|d| !d.ended) {
        let Some(m) = get_message(0, 0, 0, None) else { break };
        if m.msg == W_QUIT as u32 {
            // WM_QUIT: for the program's loop.
            post(0, W_QUIT, m.wp, 0);
            break;
        }
        // (The loop's messages are in the A form, whatever the dialog.)
        if !is_dialog_message(h, &m, false) {
            translate(&m);
            dispatch(&m, false);
        }
    }
    let r = dlg(h).map_or(-1, |d| d.result);
    if reenable {
        enable_window(ot, true);
    }
    destroy_window(h);
    if ot != 0 {
        if let Some(f) = wnd(ot).map(|w| w.frame) {
            unsafe { crate::pm::WinSetActiveWindow(crate::pm::HWND_DESKTOP, f) };
        }
    }
    r
}

/// A template in memory: as far as it can be (its length is not given).
fn template_at(p: u32) -> Option<&'static [u8]> {
    if p == 0 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(p as *const u8, 0x10000) })
}

fn template(hi: u32, name: u32, wide: bool) -> Option<&'static [u8]> {
    unsafe { Name::of(name, wide) }.and_then(|n| res::find(hi, res::RT_DIALOG, &n))
}

// ---- The entry points ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn DialogBoxParamA(hi: u32, n: u32, owner: u32, p: u32, lp: u32) -> i32 {
    template(hi, n, false).map_or(-1, |t| dialog_box(hi, t, hw(owner), p, lp, false))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DialogBoxParamW(hi: u32, n: u32, owner: u32, p: u32, lp: u32) -> i32 {
    template(hi, n, true).map_or(-1, |t| dialog_box(hi, t, hw(owner), p, lp, true))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DialogBoxIndirectParamA(hi: u32, t: u32, owner: u32, p: u32, lp: u32) -> i32 {
    template_at(t).map_or(-1, |t| dialog_box(hi, t, hw(owner), p, lp, false))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DialogBoxIndirectParamW(hi: u32, t: u32, owner: u32, p: u32, lp: u32) -> i32 {
    template_at(t).map_or(-1, |t| dialog_box(hi, t, hw(owner), p, lp, true))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDialogParamA(hi: u32, n: u32, owner: u32, p: u32, lp: u32) -> u32 {
    template(hi, n, false).map_or(0, |t| create_dialog(hi, t, hw(owner), p, lp, false) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDialogParamW(hi: u32, n: u32, owner: u32, p: u32, lp: u32) -> u32 {
    template(hi, n, true).map_or(0, |t| create_dialog(hi, t, hw(owner), p, lp, true) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDialogIndirectParamA(hi: u32, t: u32, owner: u32, p: u32, lp: u32) -> u32 {
    template_at(t).map_or(0, |t| create_dialog(hi, t, hw(owner), p, lp, false) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDialogIndirectParamW(hi: u32, t: u32, owner: u32, p: u32, lp: u32) -> u32 {
    template_at(t).map_or(0, |t| create_dialog(hi, t, hw(owner), p, lp, true) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EndDialog(h: u32, r: i32) -> i32 {
    let h = hw(h);
    let Some(d) = dlg(h) else { return 0 };
    d.ended = true;
    d.result = r;
    show_window(h, 0);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsDialogMessageA(h: u32, p: u32) -> i32 {
    Msg::read(p).is_some_and(|m| is_dialog_message(hw(h), &m, false)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsDialogMessageW(h: u32, p: u32) -> i32 {
    Msg::read(p).is_some_and(|m| is_dialog_message(hw(h), &m, true)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DefDlgProcA(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    def_dlg_proc_form(hw(h), m as u16, wp, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DefDlgProcW(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    def_dlg_proc_form(hw(h), m as u16, wp, lp, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDlgItemTextA(h: u32, id: i32, s: u32) -> i32 {
    deliver(get_item(hw(h), id as u16), W_SETTEXT, 0, s, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDlgItemTextW(h: u32, id: i32, s: u32) -> i32 {
    deliver(get_item(hw(h), id as u16), W_SETTEXT, 0, s, true) as i32
}

fn get_text(h: u32, id: i32, buf: u32, max: i32, wide: bool) -> u32 {
    let c = get_item(hw(h), id as u16);
    if c == 0 || max <= 0 {
        if buf != 0 && max > 0 {
            if wide { crate::copy_out_w(&[], buf as *mut u16, max) } else { crate::copy_out(&[], buf as *mut u8, max) };
        }
        return 0;
    }
    deliver(c, W_GETTEXT, max as u32, buf, wide)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDlgItemTextA(h: u32, id: i32, b: u32, m: i32) -> u32 {
    get_text(h, id, b, m, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDlgItemTextW(h: u32, id: i32, b: u32, m: i32) -> u32 {
    get_text(h, id, b, m, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDlgItemInt(h: u32, id: i32, v: u32, signed: i32) -> i32 {
    let s = if signed != 0 { alloc::format!("{}", v as i32) } else { alloc::format!("{}", v) };
    let z = crate::z(s.as_bytes());
    send(get_item(hw(h), id as u16), W_SETTEXT, 0, z.as_ptr() as u32);
    1
}

/// GetDlgItemInt: the text's number (32-bit in Win32).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDlgItemInt(h: u32, id: i32, ok: u32, signed: i32) -> u32 {
    let t = crate::ctl::text_of(get_item(hw(h), id as u16));
    let t: Vec<u8> = t.iter().copied().skip_while(|c| *c == b' ').collect();
    let (neg, digits) = match t.first() {
        Some(b'-') if signed != 0 => (true, &t[1..]),
        _ => (false, &t[..]),
    };
    let n = digits.iter().take_while(|c| c.is_ascii_digit()).count();
    let mut v: i64 = 0;
    let mut good = n > 0 && digits[n..].iter().all(|&c| c == b' ');
    for &c in &digits[..n] {
        v = v * 10 + (c - b'0') as i64;
        if v > u32::MAX as i64 {
            good = false;
            break;
        }
    }
    let v = if neg { -v } else { v };
    if signed != 0 && !(i32::MIN as i64..=i32::MAX as i64).contains(&v) {
        good = false;
    }
    if ok != 0 {
        crate::wr32(ok, 0, good as u32);
    }
    if good { v as u32 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CheckDlgButton(h: u32, id: i32, check: u32) -> i32 {
    send(get_item(hw(h), id as u16), 0x00f1, check, 0);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsDlgButtonChecked(h: u32, id: i32) -> u32 {
    send(get_item(hw(h), id as u16), 0x00f0, 0, 0)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CheckRadioButton(h: u32, first: i32, last: i32, check: i32) -> i32 {
    for c in children(hw(h)) {
        let id = wnd(c).map_or(0, |w| w.id as i32);
        if (first..=last).contains(&id) {
            send(c, 0x00f1, (id == check) as u32, 0);
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendDlgItemMessageA(h: u32, id: i32, m: u32, wp: u32, lp: u32) -> u32 {
    deliver(get_item(hw(h), id as u16), m as u16, wp, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendDlgItemMessageW(h: u32, id: i32, m: u32, wp: u32, lp: u32) -> u32 {
    deliver(get_item(hw(h), id as u16), m as u16, wp, lp, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MapDialogRect(h: u32, r: u32) -> i32 {
    let (bx, by) = base_units_of(hw(h));
    if let Some(rc) = read_rect(r) {
        write_rect(r, [rc[0] * bx / 4, rc[1] * by / 8, rc[2] * bx / 4, rc[3] * by / 8]);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetNextDlgGroupItem(h: u32, c: u32, prev: i32) -> u32 {
    next_group(hw(h), hw(c), prev != 0) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetNextDlgTabItem(h: u32, c: u32, prev: i32) -> u32 {
    next_tab(hw(h), hw(c), prev != 0) as u32
}

/// GetDialogBaseUnits: the system font's (MS Sans Serif 8 here).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDialogBaseUnits() -> u32 {
    let (bx, by) = base_units();
    (by as u32) << 16 | bx as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirListA(_h: u32, _p: u32, _id: i32, _s: i32, _t: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirListW(_h: u32, _p: u32, _id: i32, _s: i32, _t: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirSelectExA(_h: u32, _b: u32, _n: i32, _id: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirSelectExW(_h: u32, _b: u32, _n: i32, _id: i32) -> i32 {
    0
}
