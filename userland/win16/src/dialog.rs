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
use thunk16::{flat, Pascal, PascalCall};

use crate::res::{self, Name};
use crate::user::*;
use crate::{far_str, gdi, read_rect, write_rect};

pub const DLGWINDOWEXTRA: u16 = 30;
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

/// A dialog's state.
pub struct Dlg {
    pub proc_: u32,
    pub result: i16,
    pub ended: bool,
    pub font: u16,
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

/// A string or ordinal (0xFF and a word) of a template.
enum Text<'a> {
    Str(&'a [u8]),
    Ord(u16),
}

fn text_at<'a>(t: &'a [u8], p: &mut usize) -> Text<'a> {
    if t.get(*p) == Some(&0xff) {
        let v = res::w16(t, *p + 1);
        *p += 3;
        Text::Ord(v)
    } else {
        Text::Str(res::sz(t, p))
    }
}

/// Create a dialog from a template: its window (hidden until the dialog
/// procedure has had WM_INITDIALOG), its controls.
pub fn create_dialog(hinst: u16, t: &[u8], owner: u16, proc_: u32, param: u32) -> u16 {
    let style = res::l32(t, 0);
    let n = t.get(4).copied().unwrap_or(0) as usize;
    let dx = res::w16(t, 5) as i16 as i32;
    let dy = res::w16(t, 7) as i16 as i32;
    let dcx = res::w16(t, 9) as i16 as i32;
    let dcy = res::w16(t, 11) as i16 as i32;
    let mut p = 13;
    let menu = text_at(t, &mut p);
    let class = res::sz(t, &mut p).to_vec();
    let caption = res::sz(t, &mut p).to_vec();
    if style & DS_SETFONT != 0 {
        p += 2;
        res::sz(t, &mut p);
    }
    let (bx, by) = base_units();
    let px = |v: i32| v * bx / 4;
    let py = |v: i32| v * by / 8;
    let ci = if class.is_empty() { find_class_name(b"#32770") } else { find_class_name(&class) };
    let Some(ci) = ci else { return 0 };
    let mut wstyle = style & !(WS_VISIBLE | DS_SETFONT | DS_ABSALIGN | DS_MODALFRAME | 0x0f);
    if style & DS_MODALFRAME != 0 {
        wstyle |= WS_DLGFRAME;
    }
    if wstyle & WS_CHILD == 0 {
        wstyle |= WS_POPUP;
    }
    // The position: in the owner's client area (DS_ABSALIGN: the screen).
    let (mut x, mut y) = (px(dx), py(dy));
    let child = wstyle & WS_CHILD != 0;
    if !child && style & DS_ABSALIGN == 0 && owner != 0 {
        let o = top_level(owner);
        let (sx, sy) = to_screen(o, 0, 0);
        x += sx;
        y += sy;
    }
    let (l, tp, r, b) = frame_insets(wstyle, !matches!(menu, Text::Str(s) if s.is_empty()));
    let (cx, cy) = (px(dcx), py(dcy));
    let hmenu = match &menu {
        Text::Str(s) if !s.is_empty() => crate::menu::load(hinst, &Name::from_bytes(s)),
        Text::Ord(id) => crate::menu::load(hinst, &Name::Id(*id)),
        _ => 0,
    };
    // A top-level dialog stays on the screen.
    let (scx, scy) = screen_size();
    let (w, hh) = (cx + l + r, cy + tp + b);
    if !child {
        x = x.clamp(0, (scx - w).max(0));
        y = y.clamp(0, (scy - hh).max(0));
    }
    let h = create(&Create {
        exstyle: 0,
        class: ci,
        title: &caption,
        title_far: 0,
        class_far: 0,
        style: wstyle,
        pos: [x, y, w, hh],
        parent: owner,
        menu: hmenu,
        hinst,
        param,
    });
    if h == 0 {
        return 0;
    }
    // Make the client area the template's size exactly.
    let (ccx, ccy) = client_size(h);
    if (ccx, ccy) != (cx, cy) {
        let wr = window_rect(h);
        move_to(h, [wr[0], wr[1], wr[2] + cx - ccx, wr[3] + cy - ccy]);
    }
    if let Some(wd) = wnd(h) {
        wd.data = Some(alloc::boxed::Box::new(Dlg { proc_, result: 0, ended: false, font: 0, focus: 0, def_id: IDOK, base: (bx, by) }));
        let pv = proc_.to_le_bytes();
        if wd.extra.len() >= 8 {
            wd.extra[4..8].copy_from_slice(&pv);
        }
    }
    // The controls.
    let mut def_id = 0;
    for _ in 0..n {
        if p + 14 > t.len() {
            break;
        }
        let (ix, iy, icx, icy) = (res::w16(t, p) as i16 as i32, res::w16(t, p + 2) as i16 as i32, res::w16(t, p + 4) as i16 as i32, res::w16(t, p + 6) as i16 as i32);
        let id = res::w16(t, p + 8);
        let istyle = res::l32(t, p + 10);
        p += 14;
        let cls: Vec<u8> = match t.get(p).copied().unwrap_or(0) {
            c if c & 0x80 != 0 => {
                p += 1;
                match c {
                    0x80 => b"BUTTON".to_vec(),
                    0x81 => b"EDIT".to_vec(),
                    0x82 => b"STATIC".to_vec(),
                    0x83 => b"LISTBOX".to_vec(),
                    0x84 => b"SCROLLBAR".to_vec(),
                    0x85 => b"COMBOBOX".to_vec(),
                    _ => b"STATIC".to_vec(),
                }
            }
            _ => res::sz(t, &mut p).to_vec(),
        };
        let text = match text_at(t, &mut p) {
            Text::Str(s) => s.to_vec(),
            Text::Ord(_) => Vec::new(),
        };
        let extra = t.get(p).copied().unwrap_or(0) as usize;
        p += 1 + extra;
        let Some(cci) = find_class_name(&cls) else {
            crate::trace(format_args!("WIN16: dialog control class {} not found", cls.iter().map(|&c| c as char).collect::<alloc::string::String>()));
            continue;
        };
        if cls == b"BUTTON" && istyle & 0x0f == 1 && def_id == 0 {
            def_id = id;
        }
        create(&Create {
            exstyle: 0,
            class: cci,
            title: &text,
            title_far: 0,
            class_far: 0,
            style: istyle | WS_CHILD,
            pos: [px(ix), py(iy), px(icx), py(icy)],
            parent: h,
            menu: id,
            hinst,
            param: 0,
        });
    }
    if let Some(d) = dlg(h) {
        d.def_id = if def_id != 0 { def_id } else { IDOK };
    }
    // WM_INITDIALOG: the focus goes to the first tab stop if it says so.
    let first = next_tab(h, 0, false);
    if send(h, W_INITDIALOG, first, param) as u16 != 0 && first != 0 {
        focus_control(h, first);
    }
    if wnd(h).is_some() && style & WS_VISIBLE != 0 && !dlg(h).is_some_and(|d| d.ended) {
        show_window(h, 1);
    }
    h
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
        send(c, W_USER + 1, 0, 0x7fff_0000);
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
        let want = if focus_push { c == focus } else { id == def_id };
        let is = code & DLGC_DEFPUSHBUTTON != 0;
        if want != is {
            // BM_SETSTYLE
            send(c, W_USER + 4, if want { 1 } else { 0 }, 1);
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
        crate::user::call_beep();
        return;
    }
    send(h, W_COMMAND, id, c as u32);
}

pub fn get_item(h: u16, id: u16) -> u16 {
    children(h).into_iter().find(|&c| wnd(c).is_some_and(|w| w.id == id)).unwrap_or(0)
}

/// IsDialogMessage on a MSG (flat): handled?
pub fn is_dialog_message(h: u16, p: *mut u8) -> bool {
    let Some((mh, m, wp, _lp)) = read_msg(p) else { return false };
    if wnd(h).is_none() || (mh != h && !is_child(h, mh)) {
        return false;
    }
    let keyish = matches!(m, W_KEYDOWN | W_CHAR | W_SYSCHAR);
    // The dialog's control the message is for (a combo box's edit
    // control is the combo box's).
    let mut ctl = mh;
    while ctl != h && wnd(ctl).is_some_and(|w| w.parent != h && w.style & WS_CHILD != 0) {
        ctl = wnd(ctl).map_or(h, |w| w.parent);
    }
    if keyish {
        let code = send(mh, W_GETDLGCODE, wp, thunk16::far(p));
        if code & DLGC_WANTALLKEYS == 0 {
            match m {
                W_KEYDOWN => match wp {
                    // VK_TAB
                    0x09 if code & DLGC_WANTTAB == 0 => {
                        let prev = send_key_state(0x10);
                        let n = next_tab(h, ctl, prev);
                        focus_control(h, n);
                        return true;
                    }
                    // VK_LEFT, VK_UP, VK_RIGHT, VK_DOWN
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
                    // VK_RETURN
                    0x0d => {
                        let id = if code & (DLGC_DEFPUSHBUTTON | DLGC_UNDEFPUSHBUTTON) != 0 && mh != h {
                            wnd(mh).map_or(IDOK, |w| w.id)
                        } else {
                            let r = send(h, DM_GETDEFID, 0, 0);
                            if r >> 16 == DC_HASDEFID { r as u16 } else { IDOK }
                        };
                        command(h, id);
                        return true;
                    }
                    // VK_ESCAPE
                    0x1b => {
                        command(h, IDCANCEL);
                        return true;
                    }
                    _ => {}
                },
                W_CHAR if code & DLGC_WANTCHARS == 0 && wp > 0x20 => {
                    if let Some(c) = mnemonic(h, wp as u8) {
                        crate::ctl::mnemonic_hit(h, c);
                        return true;
                    }
                }
                W_SYSCHAR => {
                    if let Some(c) = mnemonic(h, wp as u8) {
                        crate::ctl::mnemonic_hit(h, c);
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    translate(p as *const u16);
    dispatch(p);
    true
}

/// Is a key (Windows virtual key) down?
fn send_key_state(vk: u16) -> bool {
    crate::user::key_down(vk)
}

/// DefDlgProc: the dialog procedure first.
pub fn def_dlg_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    let proc_ = dlg(h).map_or(0, |d| d.proc_);
    if proc_ != 0 {
        set_long(h, DWL_MSGRESULT, 0);
        let r = PascalCall::new().w(h).w(m).w(wp).l(lp).call(proc_) as u16;
        if r != 0 {
            return match m {
                W_CTLCOLOR | W_COMPAREITEM | W_VKEYTOITEM | W_CHARTOITEM | W_INITDIALOG | 0x0037 => r as u32,
                _ => get_long(h, DWL_MSGRESULT),
            };
        }
        if wnd(h).is_none() {
            return 0;
        }
    }
    match m {
        W_INITDIALOG => 0,
        W_ERASEBKGND => {
            let hdc = wp;
            let br = send(h, W_CTLCOLOR, hdc, 4 << 16 | h as u32) as u16;
            let br = if br == 0 { gdi::sys_brush(gdi::COLOR_WINDOW) } else { br };
            let (cx, cy) = client_size(h);
            gdi::fill_rect(hdc, [0, 0, cx, cy], br);
            1
        }
        W_CLOSE => {
            let c = get_item(h, IDCANCEL);
            if c == 0 || usable(c) {
                send(h, W_COMMAND, IDCANCEL, c as u32);
            }
            0
        }
        W_SETFOCUS => {
            restore_focus(h);
            0
        }
        W_ACTIVATE => {
            if wp != 0 {
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
                d.def_id = wp;
            }
            check_default(h, focus());
            1
        }
        W_NEXTDLGCTL => {
            let n = if lp & 0xffff != 0 { wp } else { next_tab(h, focus(), wp != 0) };
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

/// A modal dialog: runs until EndDialog; the result.
pub fn dialog_box(hinst: u16, t: &[u8], owner: u16, proc_: u32, param: u32) -> i32 {
    let ot = if owner != 0 { top_level(owner) } else { 0 };
    let h = create_dialog(hinst, t, ot, proc_, param);
    if h == 0 {
        return -1;
    }
    let reenable = ot != 0 && wnd(ot).is_some_and(|w| w.style & WS_DISABLED == 0);
    if reenable {
        enable_window(ot, false);
    }
    if !dlg(h).is_some_and(|d| d.ended) {
        show_window(h, 1);
    }
    let mut msg = [0u8; 18];
    while dlg(h).is_some_and(|d| !d.ended) {
        if get_message(msg.as_mut_ptr(), 0, 0, 0, None) == 0 {
            // WM_QUIT: for the program's loop.
            post(0, W_QUIT, crate::user::read_msg(msg.as_ptr()).map_or(0, |m| m.2), 0);
            break;
        }
        if !is_dialog_message(h, msg.as_mut_ptr()) {
            translate(msg.as_ptr() as *const u16);
            dispatch(msg.as_mut_ptr());
        }
    }
    let r = dlg(h).map_or(-1, |d| d.result as i32);
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

/// A template's bytes at a far pointer (to the end of its segment).
fn template_at(far: u32) -> Option<&'static [u8]> {
    let p = flat(far);
    if p.is_null() {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(p, 0x10000 - (far & 0xffff) as usize) })
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

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // DialogBox(hinst, template, owner, proc), DialogBoxParam(+param)
        87 | 239 => {
            let (hi, name, owner, p) = (a.w(), a.l(), a.w(), a.l());
            let param = if ord == 239 { a.l() } else { 0 };
            let Some(t) = Name::of(name).and_then(|n| res::find(hi, res::RT_DIALOG, &n)) else { return Some(0xffff) };
            dialog_box(hi, t, owner, p, param) as u16 as u32
        }
        // DialogBoxIndirect(hinst, hglobal, owner, proc), +Param
        218 | 240 => {
            let (hi, hg, owner, p) = (a.w(), a.w(), a.w(), a.l());
            let param = if ord == 240 { a.l() } else { 0 };
            let Some(t) = template_at(((hg | 7) as u32) << 16) else { return Some(0xffff) };
            dialog_box(hi, t, owner, p, param) as u16 as u32
        }
        // CreateDialog(hinst, template, owner, proc), +Param
        89 | 241 => {
            let (hi, name, owner, p) = (a.w(), a.l(), a.w(), a.l());
            let param = if ord == 241 { a.l() } else { 0 };
            let Some(t) = Name::of(name).and_then(|n| res::find(hi, res::RT_DIALOG, &n)) else { return Some(0) };
            create_dialog(hi, t, owner, p, param) as u32
        }
        // CreateDialogIndirect(hinst, lpTemplate, owner, proc), +Param
        219 | 242 => {
            let (hi, tp, owner, p) = (a.w(), a.l(), a.w(), a.l());
            let param = if ord == 242 { a.l() } else { 0 };
            let Some(t) = template_at(tp) else { return Some(0) };
            create_dialog(hi, t, owner, p, param) as u32
        }
        // EndDialog(hwnd, result)
        88 => {
            let (h, r) = (a.w(), a.w());
            if let Some(d) = dlg(h) {
                d.ended = true;
                d.result = r as i16;
            }
            show_window(h, 0);
            1
        }
        // IsDialogMessage(hwnd, lpMsg)
        90 => {
            let (h, p) = (a.w(), a.p());
            is_dialog_message(h, p) as u32
        }
        // GetDlgItem(hwnd, id)
        91 => {
            let (h, id) = (a.w(), a.w());
            get_item(h, id) as u32
        }
        // SetDlgItemText(hwnd, id, text)
        92 => {
            let (h, id, s) = (a.w(), a.w(), a.l());
            send(get_item(h, id), W_SETTEXT, 0, s);
            0
        }
        // GetDlgItemText(hwnd, id, buf, max)
        93 => {
            let (h, id, buf, max) = (a.w(), a.w(), a.l(), a.i());
            let c = get_item(h, id);
            if c == 0 || max <= 0 {
                if let Some(p) = Some(flat(buf)).filter(|p| !p.is_null() && max > 0) {
                    unsafe { *p = 0 };
                }
                return Some(0);
            }
            send(c, W_GETTEXT, max as u16, buf)
        }
        // SetDlgItemInt(hwnd, id, value, signed)
        94 => {
            let (h, id, v, signed) = (a.w(), a.w(), a.w(), a.w());
            let s = if signed != 0 { alloc::format!("{}", v as i16) } else { alloc::format!("{}", v) };
            let c = get_item(h, id);
            crate::mem16::with_str(s.as_bytes(), |far| send(c, W_SETTEXT, 0, far));
            0
        }
        // GetDlgItemInt(hwnd, id, lpTranslated, signed)
        95 => {
            let (h, id, ok, signed) = (a.w(), a.w(), a.p() as *mut u16, a.w());
            let c = get_item(h, id);
            let t = crate::ctl::text_of(c);
            let r = parse_int(&t, signed != 0);
            if !ok.is_null() {
                unsafe { ok.write_unaligned(r.is_some() as u16) };
            }
            r.map_or(0, |v| v as u16 as u32)
        }
        // CheckRadioButton(hwnd, first, last, check)
        96 => {
            let (h, first, last, check) = (a.w(), a.w(), a.w(), a.w());
            for c in children(h) {
                let id = wnd(c).map_or(0, |w| w.id);
                if (first..=last).contains(&id) {
                    send(c, W_USER + 1, (id == check) as u16, 0);
                }
            }
            0
        }
        // CheckDlgButton(hwnd, id, check)
        97 => {
            let (h, id, check) = (a.w(), a.w(), a.w());
            send(get_item(h, id), W_USER + 1, check, 0);
            0
        }
        // IsDlgButtonChecked(hwnd, id)
        98 => {
            let (h, id) = (a.w(), a.w());
            send(get_item(h, id), W_USER, 0, 0)
        }
        // DlgDirSelect, DlgDirList: no directory lists in dialogs yet.
        99 => {
            a.w();
            a.l();
            a.w();
            0
        }
        100 => {
            a.w();
            a.l();
            a.w();
            a.w();
            a.w();
            0
        }
        // SendDlgItemMessage(hwnd, id, msg, wParam, lParam)
        101 => {
            let (h, id, m, wp, lp) = (a.w(), a.w(), a.w(), a.w(), a.l());
            send(get_item(h, id), m, wp, lp)
        }
        // MapDialogRect(hwnd, lpRect)
        103 => {
            let (h, r) = (a.w(), a.l());
            let (bx, by) = dlg(h).map_or(base_units(), |d| d.base);
            if let Some(rc) = read_rect(r) {
                write_rect(r, [rc[0] * bx / 4, rc[1] * by / 8, rc[2] * bx / 4, rc[3] * by / 8]);
            }
            0
        }
        // GetNextDlgGroupItem(hwnd, ctl, prev), GetNextDlgTabItem
        227 | 228 => {
            let (h, c, prev) = (a.w(), a.w(), a.w());
            (if ord == 227 { next_group(h, c, prev != 0) } else { next_tab(h, c, prev != 0) }) as u32
        }
        // GetDialogBaseUnits
        243 => {
            let (bx, by) = base_units();
            (by as u32) << 16 | bx as u32
        }
        // GetDlgCtrlID(hwnd)
        277 => wnd(a.w()).map_or(0, |w| w.id as u32),
        _ => {
            let _ = far_str;
            return None;
        }
    })
}
