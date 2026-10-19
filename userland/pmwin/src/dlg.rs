//! Dialogs: windows built from dialog templates (WinLoadDlg,
//! WinCreateDlg), modal processing (WinProcessDlg, WinDlgBox,
//! WinDismissDlg), the default dialog procedure with keyboard navigation
//! (Tab, arrows within groups, Enter for the default button, Esc,
//! mnemonics) and the dialog item helpers.
//!
//! A dialog is a frame window whose window procedure is the dialog
//! procedure; WinDefDlgProc passes what it does not handle to the frame.
//! Template coordinates are dialog units: a quarter of the average
//! character width and an eighth of the character height (2 pixels each
//! with the 8x16 system font).

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::Rect;
use pmwin::{ws, HWND_DESKTOP};

use crate::ctl::{self, *};
use crate::frame::{self, FCF_BORDER, FCF_DLGBORDER, FCF_SIZEBORDER, SC_CLOSE};
use crate::lock::wndproc;
use crate::msg::{self, post, send};
use crate::res::{self, DlgItemT};
use crate::state::{st, QMsg};
use crate::win::{self, WM_CHAR, WM_CLOSE, WM_SYSCOMMAND};

pub const DID_OK: u32 = 1;
pub const DID_CANCEL: u32 = 2;
const DID_ERROR: u32 = 0xffff;

const FS_DLGBORDER: u32 = 0x0080;
const FS_BORDER: u32 = 0x0100;
const FS_SCREENALIGN: u32 = 0x0200;
const FS_MOUSEALIGN: u32 = 0x0400;
const FS_SIZEBORDER: u32 = 0x0800;

const EDI_FIRSTTABITEM: u32 = 0;
const EDI_LASTTABITEM: u32 = 1;
const EDI_NEXTTABITEM: u32 = 2;
const EDI_PREVTABITEM: u32 = 3;
const EDI_FIRSTGROUPITEM: u32 = 4;
const EDI_LASTGROUPITEM: u32 = 5;
const EDI_NEXTGROUPITEM: u32 = 6;
const EDI_PREVGROUPITEM: u32 = 7;

const EM_SETSEL: u32 = 0x0142;
const BM_CLICK: u32 = 0x0120;

/// Pixels per dialog unit.
const DU_X: i32 = 2;
const DU_Y: i32 = 2;

wndproc! {
    def_dlg_entry => WinDefDlgProc;
}

struct Dlg {
    result: u32,
    dismissed: bool,
    /// Owner disabled while the dialog runs modally.
    disabled_owner: u32,
}

fn data(h: u32) -> Option<&'static mut Dlg> {
    ctl::data::<Dlg>(h)
}

// ---- Creation ----

/// WinLoadDlg(hwndParent, hwndOwner, pfnDlgProc, hmod, idDlg,
/// pCreateParams)
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadDlg(parent: u32, owner: u32, proc_: u32, hmod: u32, id: u32, params: u32) -> u32 {
    let Some(b) = res::load(hmod, res::RT_DIALOG, id) else { return 0 };
    let Some(t) = res::parse_dialog(b) else { return 0 };
    create_from(parent, owner, proc_, &t, params)
}

/// WinCreateDlg(hwndParent, hwndOwner, pfnDlgProc, pdlgt, pCreateParams)
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateDlg(parent: u32, owner: u32, proc_: u32, pdlgt: *const u8, params: u32) -> u32 {
    if pdlgt.is_null() {
        return 0;
    }
    let cb = unsafe { (pdlgt as *const u16).read_unaligned() } as usize;
    let Some(t) = res::parse_dialog(unsafe { core::slice::from_raw_parts(pdlgt, cb) }) else { return 0 };
    create_from(parent, owner, proc_, &t, params)
}

/// The dialog's position on its parent: template coordinates are
/// relative to the owner (or the screen, or the pointer), kept on the
/// screen. The template size is the area inside the frame controls.
fn place(parent: u32, owner: u32, t: &DlgItemT, fcf: u32) -> Rect {
    let b = frame::border(fcf);
    let title = if fcf & (frame::FCF_TITLEBAR | frame::FCF_SYSMENU) != 0 { frame::TITLE_H } else { 0 };
    let s = st();
    let (cx, cy) = (t.cx * DU_X + 2 * b, t.cy * DU_Y + 2 * b + title);
    let (mut x, mut y) = (t.x * DU_X, t.y * DU_Y);
    if t.style & FS_MOUSEALIGN != 0 {
        x += s.pointer.x;
        y += s.pointer.y;
    } else if t.style & FS_SCREENALIGN == 0 {
        let base = if owner != 0 && owner != HWND_DESKTOP && s.desk.exists(owner) { owner } else { parent };
        let o = s.desk.origin(base);
        x += o.x;
        y += o.y;
    }
    let (sw, sh) = (s.surface.width, s.surface.height);
    x = x.clamp(0, (sw - cx).max(0));
    y = y.clamp(0, (sh - cy).max(0));
    let po = s.desk.origin(parent);
    Rect::new(x - po.x, y - po.y, x - po.x + cx, y - po.y + cy)
}

pub fn create_from(parent: u32, owner: u32, proc_: u32, t: &DlgItemT, params: u32) -> u32 {
    let parent = if parent == 0 { HWND_DESKTOP } else { parent };
    // The actual owner is the ancestor of hwndOwner that is a child of
    // the dialog's parent (a client window's frame, for a dialog on the
    // desktop).
    let mut owner = owner;
    while owner != 0 && st().desk.get(owner).is_some_and(|w| w.parent != parent && w.parent != 0) {
        owner = ctl::parent(owner);
    }
    if !st().desk.exists(owner) {
        owner = 0;
    }
    let mut fcf = if t.ctl.is_null() { 0 } else { unsafe { (t.ctl as *const u32).read_unaligned() } };
    for (fs, f) in [(FS_DLGBORDER, FCF_DLGBORDER), (FS_BORDER, FCF_BORDER), (FS_SIZEBORDER, FCF_SIZEBORDER)] {
        if t.style & fs != 0 {
            fcf |= f;
        }
    }
    let pos = place(parent, owner, t, fcf);
    let h = frame::create(parent, t.style & !ws::VISIBLE, fcf, &t.text, pos, owner, t.id, 0);
    if h == 0 {
        return 0;
    }
    ctl::set_data(h, Dlg { result: 0, dismissed: false, disabled_owner: 0 });
    let b = frame::border(fcf);
    for c in &t.children {
        create_item(h, h, c, b);
    }
    let proc_ = if proc_ == 0 { def_dlg_entry as *const () as u32 } else { proc_ };
    if let Some(w) = st().desk.get_mut(h) {
        w.proc_ = proc_;
    }
    let first = WinEnumDlgItem(h, 0, EDI_FIRSTTABITEM);
    if let Some(f) = st().frames.get_mut(&h) {
        f.focus_save = first;
    }
    if send(h, WM_INITDLG, first, params) == 0 && first != 0 && st().desk.exists(first) {
        if let Some(f) = st().frames.get_mut(&h) {
            f.focus_save = first;
        }
        select_all(first);
    }
    if t.style & ws::VISIBLE != 0 && st().desk.exists(h) {
        win::show(h, true);
        win::activate(h);
    }
    h
}

/// Create a dialog item and its children, `off` pixels in from the
/// parent's origin.
fn create_item(parent: u32, dlg: u32, t: &DlgItemT, off: i32) {
    let mut class = t.class.clone();
    let mut style = t.style;
    if st().class(&class).is_none() {
        // A control class rs2 does not have yet: a placeholder frame.
        class = b"#5".to_vec();
        style = (style & 0xffff_0000) | 0x09;
    }
    let pos = Rect::new(t.x * DU_X, t.y * DU_Y, (t.x + t.cx) * DU_X, (t.y + t.cy) * DU_Y).offset(off, off);
    let h = win::create(parent, &class, &t.text, style, pos, dlg, t.id, t.ctl as u32);
    for c in &t.children {
        if h != 0 {
            create_item(h, dlg, c, 0);
        }
    }
}

// ---- Modal processing ----

/// WinProcessDlg(hwndDlg): run the dialog modally until dismissed.
#[unsafe(no_mangle)]
pub extern "C" fn WinProcessDlg(h: u32) -> u32 {
    if data(h).is_none() {
        return DID_ERROR;
    }
    let owner = win::top_level(ctl::owner(h));
    if owner != 0 && owner != HWND_DESKTOP && owner != h && ctl::enabled(owner) {
        win::WinEnableWindow(owner, 0);
        if let Some(d) = data(h) {
            d.disabled_owner = owner;
        }
    }
    if let Some(d) = data(h) {
        d.dismissed = false;
    }
    win::show(h, true);
    win::activate(h);
    let mut m = QMsg::default();
    while data(h).is_some_and(|d| !d.dismissed) {
        if msg::WinGetMsg(1, &mut m, 0, 0, 0) == 0 {
            // WM_QUIT ends the dialog; the application's loop sees it too.
            msg::WinPostQueueMsg(st().my_queue(), win::WM_QUIT, 0, 0);
            if let Some(d) = data(h) {
                d.result = DID_ERROR;
            }
            break;
        }
        msg::WinDispatchMsg(1, &m);
    }
    restore_owner(h);
    data(h).map_or(DID_ERROR, |d| d.result)
}

fn restore_owner(h: u32) {
    let Some(d) = data(h) else { return };
    let owner = core::mem::take(&mut d.disabled_owner);
    if owner != 0 && st().desk.exists(owner) {
        win::WinEnableWindow(owner, 1);
        win::activate(owner);
    }
}

/// WinDismissDlg(hwndDlg, usResult): hide the dialog and end its modal
/// loop with the result.
#[unsafe(no_mangle)]
pub extern "C" fn WinDismissDlg(h: u32, result: u32) -> u32 {
    let Some(d) = data(h) else { return 0 };
    d.result = result & 0xffff;
    d.dismissed = true;
    win::show(h, false);
    restore_owner(h);
    1
}

/// WinDlgBox(hwndParent, hwndOwner, pfnDlgProc, hmod, idDlg,
/// pCreateParams): load, run and destroy a dialog.
#[unsafe(no_mangle)]
pub extern "C" fn WinDlgBox(parent: u32, owner: u32, proc_: u32, hmod: u32, id: u32, params: u32) -> u32 {
    let h = WinLoadDlg(parent, owner, proc_, hmod, id, params);
    if h == 0 {
        return DID_ERROR;
    }
    let r = WinProcessDlg(h);
    win::WinDestroyWindow(h);
    r
}

// ---- The default dialog procedure ----

#[unsafe(no_mangle)]
pub extern "C" fn WinDefDlgProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    // Not a dialog (a control subclassed with a procedure that passes
    // what it does not handle to WinDefDlgProc, as the System Editor
    // does with its MLE): the window's class procedure.
    if ctl::data::<Dlg>(h).is_none() {
        let p = win::class_proc(h);
        if p != 0 && p != def_dlg_entry as *const () as u32 {
            return crate::msg::call(p, h, msg, mp1, mp2);
        }
        return crate::msg::WinDefWindowProc(h, msg, mp1, mp2);
    }
    match msg {
        WM_INITDLG | WM_CONTROL => 0,
        WM_COMMAND => {
            WinDismissDlg(h, lo(mp1));
            0
        }
        WM_SYSCOMMAND if lo(mp1) == SC_CLOSE => {
            post(h, WM_COMMAND, DID_CANCEL, mp(CMDSRC_OTHER, 0));
            0
        }
        WM_CLOSE => {
            post(h, WM_COMMAND, DID_CANCEL, mp(CMDSRC_OTHER, 0));
            0
        }
        WM_CHAR => {
            if key(h, mp1, mp2) || crate::notebook::page_key(h, mp1, mp2) {
                return 1;
            }
            frame::frame_proc(h, msg, mp1, mp2)
        }
        _ => frame::frame_proc(h, msg, mp1, mp2),
    }
}

fn dlgcode(h: u32) -> u32 {
    send(h, WM_QUERYDLGCODE, 0, 0)
}

fn usable(h: u32) -> bool {
    st().desk.get(h).is_some_and(|w| w.style & ws::VISIBLE != 0 && w.style & ws::DISABLED == 0)
}

/// Give a dialog item the focus (entry fields select their text).
fn focus(h: u32) {
    win::WinSetFocus(1, h);
    select_all(h);
}

fn select_all(h: u32) {
    if dlgcode(h) & DLGC_ENTRYFIELD != 0 {
        send(h, EM_SETSEL, mp(0, 0x7fff), 0);
    }
}

/// The dialog item (child of the dialog) containing `h`.
fn item_of(dlg: u32, h: u32) -> u32 {
    let mut cur = h;
    while cur != 0 {
        let p = ctl::parent(cur);
        if p == dlg {
            return cur;
        }
        cur = p;
    }
    0
}

fn key(dlg: u32, mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.up() {
        return false;
    }
    let cur = item_of(dlg, st().focus);
    match k.vk {
        VK_TAB | VK_BACKTAB if !k.alt() && !k.ctrl() => {
            let back = k.vk == VK_BACKTAB || k.shift();
            let next = WinEnumDlgItem(dlg, cur, if back { EDI_PREVTABITEM } else { EDI_NEXTTABITEM });
            if next != 0 {
                focus(next);
            }
            true
        }
        VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT if !k.alt() => {
            if cur == 0 {
                return false;
            }
            let back = matches!(k.vk, VK_UP | VK_LEFT);
            let next = WinEnumDlgItem(dlg, cur, if back { EDI_PREVGROUPITEM } else { EDI_NEXTGROUPITEM });
            if next != 0 && next != cur {
                focus(next);
                // Moving onto an automatic radio button checks it.
                if dlgcode(next) & DLGC_RADIOBUTTON != 0 && crate::button::kind(next) == crate::button::BS_AUTORADIOBUTTON {
                    crate::button::click(next, false);
                }
            }
            true
        }
        VK_NEWLINE | VK_ENTER => {
            let code = if cur != 0 { dlgcode(cur) } else { 0 };
            if code & (DLGC_PUSHBUTTON | DLGC_DEFAULT) != 0 {
                send(cur, BM_CLICK, 0, 0);
            } else if let Some(d) = children(dlg).into_iter().find(|&c| usable(c) && dlgcode(c) & DLGC_DEFAULT != 0) {
                send(d, BM_CLICK, 0, 0);
            } else {
                post(dlg, WM_COMMAND, DID_OK, mp(CMDSRC_OTHER, 0));
            }
            true
        }
        VK_ESC if !k.shift() => {
            post(dlg, WM_COMMAND, DID_CANCEL, mp(CMDSRC_OTHER, 0));
            true
        }
        _ => {
            let c = k.letter(mp1);
            let entry = cur != 0 && dlgcode(cur) & DLGC_ENTRYFIELD != 0;
            if c == 0 || k.ctrl() || (!k.alt() && entry) {
                return false;
            }
            mnemonic(dlg, c)
        }
    }
}

/// Activate the item whose text has mnemonic `c`.
fn mnemonic(dlg: u32, c: u8) -> bool {
    let kids = children(dlg);
    let Some(i) = kids.iter().position(|&h| usable(h) && send(h, WM_MATCHMNEMONIC, c as u32, 0) != 0) else { return false };
    let h = kids[i];
    let code = dlgcode(h);
    if code & DLGC_STATIC != 0 {
        // A label: the next item that takes the focus.
        if let Some(&n) = kids[i + 1..].iter().find(|&&n| usable(n) && dlgcode(n) & DLGC_STATIC == 0) {
            focus(n);
        }
    } else if code & DLGC_BUTTON != 0 {
        focus(h);
        send(h, BM_CLICK, 0, 0);
    } else {
        focus(h);
    }
    true
}

// ---- Items and groups ----

fn children(h: u32) -> Vec<u32> {
    st().desk.get(h).map_or(Vec::new(), |w| w.children.clone())
}

/// The items of the group containing `h` (WS_GROUP starts a group).
pub fn group(h: u32) -> Vec<u32> {
    let sibs = children(ctl::parent(h));
    let Some(at) = sibs.iter().position(|&c| c == h) else { return Vec::new() };
    let start = (0..=at).rev().find(|&i| ctl::style(sibs[i]) & WS_GROUP != 0).unwrap_or(0);
    let end = (at + 1..sibs.len()).find(|&i| ctl::style(sibs[i]) & WS_GROUP != 0).unwrap_or(sibs.len());
    sibs[start..end].to_vec()
}

fn is_frame_control(h: u32) -> bool {
    (0x8002..=0x8008).contains(&ctl::id(h))
}

/// WinEnumDlgItem(hwndDlg, hwnd, code)
#[unsafe(no_mangle)]
pub extern "C" fn WinEnumDlgItem(dlg: u32, h: u32, code: u32) -> u32 {
    let items: Vec<u32> = children(dlg).into_iter().filter(|&c| !is_frame_control(c)).collect();
    let tab: Vec<u32> = items.iter().copied().filter(|&c| usable(c) && ctl::style(c) & WS_TABSTOP != 0).collect();
    let cycle = |list: &[u32], d: i32| -> u32 {
        if list.is_empty() {
            return 0;
        }
        let n = list.len() as i32;
        match list.iter().position(|&c| c == h) {
            Some(i) => list[(i as i32 + d).rem_euclid(n) as usize],
            None => {
                // From an item that is not a tab stop: the next tab stop
                // after it in item order.
                let at = items.iter().position(|&c| c == h);
                let after = at.and_then(|a| items[a + 1..].iter().chain(items[..a].iter()).copied().find(|c| list.contains(c)));
                if d > 0 { after.unwrap_or(list[0]) } else { list[n as usize - 1] }
            }
        }
    };
    match code {
        EDI_FIRSTTABITEM => tab.first().copied().unwrap_or(0),
        EDI_LASTTABITEM => tab.last().copied().unwrap_or(0),
        EDI_NEXTTABITEM => cycle(&tab, 1),
        EDI_PREVTABITEM => cycle(&tab, -1),
        EDI_FIRSTGROUPITEM | EDI_LASTGROUPITEM | EDI_NEXTGROUPITEM | EDI_PREVGROUPITEM => {
            let g: Vec<u32> = group(h).into_iter().filter(|&c| usable(c) && dlgcode(c) & DLGC_STATIC == 0).collect();
            match code {
                EDI_FIRSTGROUPITEM => g.first().copied().unwrap_or(0),
                EDI_LASTGROUPITEM => g.last().copied().unwrap_or(0),
                EDI_NEXTGROUPITEM => cycle(&g, 1),
                _ => cycle(&g, -1),
            }
        }
        _ => 0,
    }
}

// ---- Dialog item helpers ----

#[unsafe(no_mangle)]
pub extern "C" fn WinSendDlgItemMsg(dlg: u32, id: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    send(win::WinWindowFromID(dlg, id), msg, mp1, mp2)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryDlgItemText(dlg: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    win::WinQueryWindowText(win::WinWindowFromID(dlg, id), cch, buf)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryDlgItemTextLength(dlg: u32, id: u32) -> i32 {
    win::WinQueryWindowTextLength(win::WinWindowFromID(dlg, id))
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetDlgItemText(dlg: u32, id: u32, text: *const u8) -> u32 {
    win::WinSetWindowText(win::WinWindowFromID(dlg, id), text)
}

/// WinQueryDlgItemShort(hwndDlg, idItem, pResult, fSigned): the item's
/// text as a number.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryDlgItemShort(dlg: u32, id: u32, result: *mut i16, signed: u32) -> u32 {
    let h = win::WinWindowFromID(dlg, id);
    let t = ctl::text(h);
    let t = t.trim_ascii();
    let (neg, digits) = match t.first() {
        Some(b'-') if signed != 0 => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return 0;
    }
    let v = digits.iter().fold(0i64, |a, &d| (a * 10 + (d - b'0') as i64).min(1 << 20));
    let v = if neg { -v } else { v };
    let ok = if signed != 0 { (-32768..=32767).contains(&v) } else { (0..=65535).contains(&v) };
    if !ok {
        return 0;
    }
    if !result.is_null() {
        unsafe { result.write_unaligned(v as i16) };
    }
    1
}

/// WinSetDlgItemShort(hwndDlg, idItem, usValue, fSigned)
#[unsafe(no_mangle)]
pub extern "C" fn WinSetDlgItemShort(dlg: u32, id: u32, v: u32, signed: u32) -> u32 {
    let n = if signed != 0 { v as u16 as i16 as i32 } else { v as u16 as i32 };
    let t = alloc::format!("{n}");
    win::set_text(win::WinWindowFromID(dlg, id), t.as_bytes())
}

/// WinMapDlgPoints(hwndDlg, prgwptl, cwpt, fCalcWindowCoords): dialog
/// units to pixels (or back).
#[unsafe(no_mangle)]
pub extern "C" fn WinMapDlgPoints(_dlg: u32, pts: *mut i32, n: u32, to_window: u32) -> u32 {
    if pts.is_null() {
        return 0;
    }
    for i in 0..n as usize {
        unsafe {
            let (x, y) = (pts.add(2 * i).read_unaligned(), pts.add(2 * i + 1).read_unaligned());
            let (x, y) = if to_window != 0 { (x * DU_X, y * DU_Y) } else { (x / DU_X, y / DU_Y) };
            pts.add(2 * i).write_unaligned(x);
            pts.add(2 * i + 1).write_unaligned(y);
        }
    }
    1
}
