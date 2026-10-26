//! The LISTBOX and COMBOBOX controls.
//!
//! A list box keeps its items (text, item data, selection) here and
//! draws them (owner-drawn ones through WM_DRAWITEM); single, multiple
//! (LBS_MULTIPLESEL) and extended (LBS_EXTENDEDSEL) selection, sorting,
//! the window's vertical scroll bar. A combo box is an edit control
//! (CBS_SIMPLE, CBS_DROPDOWN) or a drawn selection field
//! (CBS_DROPDOWNLIST) with a list box: a child below it (CBS_SIMPLE) or
//! a pop-up (COMBOLBOX) shown under it while dropped, the mouse captured
//! by the combo box meanwhile as Windows does. The list's notifications
//! go to the combo box, which makes CBN_ ones of them.

use alloc::vec::Vec;

use crate::ctl::{ctl_color, draw_item, CTLCOLOR_EDIT, CTLCOLOR_LISTBOX};
use crate::dialog::*;
use crate::gdi;
use crate::user::*;

const LBS_NOTIFY: u32 = 0x0001;
const LBS_SORT: u32 = 0x0002;
const LBS_MULTIPLESEL: u32 = 0x0008;
const LBS_OWNERDRAWFIXED: u32 = 0x0010;
const LBS_OWNERDRAWVARIABLE: u32 = 0x0020;
const LBS_HASSTRINGS: u32 = 0x0040;
const LBS_USETABSTOPS: u32 = 0x0080;
const LBS_WANTKEYBOARDINPUT: u32 = 0x0400;
const LBS_EXTENDEDSEL: u32 = 0x0800;

const LB_ADDSTRING: u16 = W_USER + 1;
const LB_INSERTSTRING: u16 = W_USER + 2;
const LB_DELETESTRING: u16 = W_USER + 3;
const LB_RESETCONTENT: u16 = W_USER + 5;
const LB_SETSEL: u16 = W_USER + 6;
const LB_SETCURSEL: u16 = W_USER + 7;
const LB_GETSEL: u16 = W_USER + 8;
const LB_GETCURSEL: u16 = W_USER + 9;
const LB_GETTEXT: u16 = W_USER + 10;
const LB_GETTEXTLEN: u16 = W_USER + 11;
const LB_GETCOUNT: u16 = W_USER + 12;
const LB_SELECTSTRING: u16 = W_USER + 13;
const LB_DIR: u16 = W_USER + 14;
const LB_GETTOPINDEX: u16 = W_USER + 15;
const LB_FINDSTRING: u16 = W_USER + 16;
const LB_GETSELCOUNT: u16 = W_USER + 17;
const LB_GETSELITEMS: u16 = W_USER + 18;
const LB_SETTABSTOPS: u16 = W_USER + 19;
const LB_GETHORIZONTALEXTENT: u16 = W_USER + 20;
const LB_SETHORIZONTALEXTENT: u16 = W_USER + 21;
const LB_SETCOLUMNWIDTH: u16 = W_USER + 22;
const LB_SETTOPINDEX: u16 = W_USER + 24;
const LB_GETITEMRECT: u16 = W_USER + 25;
const LB_GETITEMDATA: u16 = W_USER + 26;
const LB_SETITEMDATA: u16 = W_USER + 27;
const LB_SELITEMRANGE: u16 = W_USER + 28;
const LB_SETCARETINDEX: u16 = W_USER + 31;
const LB_GETCARETINDEX: u16 = W_USER + 32;
const LB_SETITEMHEIGHT: u16 = W_USER + 33;
const LB_GETITEMHEIGHT: u16 = W_USER + 34;
const LB_FINDSTRINGEXACT: u16 = W_USER + 35;

const LBN_SELCHANGE: u16 = 1;
const LBN_DBLCLK: u16 = 2;
const LBN_SELCANCEL: u16 = 3;
const LBN_SETFOCUS: u16 = 4;
const LBN_KILLFOCUS: u16 = 5;

const LB_ERR: u32 = 0xffff_ffff;

struct Item {
    text: Vec<u8>,
    data: u32,
    sel: bool,
}

pub struct ListBox {
    items: Vec<Item>,
    /// The selected item (single selection) or -1.
    cur: i32,
    /// The focus item (the caret).
    caret: i32,
    anchor: i32,
    top: i32,
    font: u16,
    item_h: i32,
    focus: bool,
    captured: bool,
    tabs: Vec<i32>,
    /// The combo box this list belongs to (0: none).
    combo: u16,
}

fn lb(h: u16) -> Option<&'static mut ListBox> {
    data::<ListBox>(h)
}

fn style(h: u16) -> u32 {
    wnd(h).map_or(0, |w| w.style)
}

fn multi(h: u16) -> bool {
    style(h) & (LBS_MULTIPLESEL | LBS_EXTENDEDSEL) != 0
}

fn has_strings(h: u16) -> bool {
    let s = style(h);
    s & (LBS_OWNERDRAWFIXED | LBS_OWNERDRAWVARIABLE) == 0 || s & LBS_HASSTRINGS != 0
}

fn visible(h: u16) -> i32 {
    let (_, cy) = client_size(h);
    let ih = lb(h).map_or(16, |l| l.item_h).max(1);
    (cy / ih).max(1)
}

fn notify_lb(h: u16, code: u16) {
    if style(h) & LBS_NOTIFY != 0 || lb(h).is_some_and(|l| l.combo != 0) {
        let combo = lb(h).map_or(0, |l| l.combo);
        if combo != 0 {
            combo_list_notify(combo, code);
        } else {
            notify(h, code);
        }
    }
}

fn update_bar(h: u16) {
    if style(h) & WS_VSCROLL == 0 {
        return;
    }
    let vis = visible(h);
    let Some(l) = lb(h) else { return };
    let max = (l.items.len() as i32 - vis).max(0);
    l.top = l.top.clamp(0, max);
    let top = l.top;
    crate::scroll::set_range(h, 1, 0, max);
    crate::scroll::set_pos(h, 1, top);
}

fn show_item(h: u16, i: i32) {
    let vis = visible(h);
    let Some(l) = lb(h) else { return };
    if i < 0 {
        return;
    }
    if i < l.top {
        l.top = i;
    } else if i >= l.top + vis {
        l.top = i - vis + 1;
    }
    update_bar(h);
}

fn measure(h: u16) {
    let font = lb(h).map_or(0, |l| l.font);
    let hdc = get_dc(h);
    gdi::select_font(hdc, font);
    let lh = gdi::line_height(hdc);
    gdi::release_dc(hdc);
    let st = style(h);
    let mut ih = lh;
    if st & (LBS_OWNERDRAWFIXED | LBS_OWNERDRAWVARIABLE) != 0 {
        // WM_MEASUREITEM: CtlType, CtlID, itemID, itemWidth, itemHeight,
        // itemData.
        let (parent, id) = wnd(h).map_or((0, 0), |w| (w.parent, w.id));
        let saved = thunk16::sp16();
        let far = thunk16::alloc16(14);
        let p = thunk16::flat(far) as *mut u16;
        let (cx, _) = client_size(h);
        for (i, v) in [2u16, id, 0, cx as u16, lh as u16, 0, 0].iter().enumerate() {
            unsafe { p.add(i).write_unaligned(*v) };
        }
        send(parent, W_MEASUREITEM, id, far);
        ih = unsafe { p.add(4).read_unaligned() } as i32;
        thunk16::set_sp16(saved);
    }
    if let Some(l) = lb(h) {
        l.item_h = ih.max(1);
    }
}

/// Insert an item (sorted when LBS_SORT): its index.
fn insert(h: u16, at: i32, text: Vec<u8>, data: u32) -> i32 {
    let sort = style(h) & LBS_SORT != 0 && has_strings(h);
    let Some(l) = lb(h) else { return -1 };
    let i = if sort {
        let key = text.to_ascii_lowercase();
        l.items.iter().position(|it| it.text.to_ascii_lowercase() > key).unwrap_or(l.items.len())
    } else if at < 0 || at as usize > l.items.len() {
        l.items.len()
    } else {
        at as usize
    };
    l.items.insert(i, Item { text, data, sel: false });
    let i = i as i32;
    if l.cur >= i {
        l.cur += 1;
    }
    if l.caret >= i && l.caret >= 0 && l.items.len() > 1 {
        l.caret += 1;
    }
    update_bar(h);
    invalidate(h, None);
    i
}

fn find(h: u16, start: i32, s: &[u8], exact: bool) -> i32 {
    let Some(l) = lb(h) else { return -1 };
    let n = l.items.len() as i32;
    if n == 0 {
        return -1;
    }
    let key = s.to_ascii_lowercase();
    for k in 0..n {
        let i = (start + 1 + k).rem_euclid(n);
        let t = l.items[i as usize].text.to_ascii_lowercase();
        if (exact && t == key) || (!exact && t.starts_with(&key)) {
            return i;
        }
    }
    -1
}

/// Select an item (single selection: the current one).
fn select(h: u16, i: i32, notify: bool) {
    let m = multi(h);
    let Some(l) = lb(h) else { return };
    let i = if i >= l.items.len() as i32 { -1 } else { i };
    let changed;
    if m {
        changed = i >= 0 && !l.items[i as usize].sel;
        if i >= 0 {
            l.items[i as usize].sel = true;
        }
    } else {
        changed = l.cur != i;
        l.cur = i;
    }
    if i >= 0 {
        l.caret = i;
    }
    show_item(h, i);
    invalidate(h, None);
    if changed && notify {
        notify_lb(h, LBN_SELCHANGE);
    }
}

fn item_at(h: u16, y: i32) -> i32 {
    let Some(l) = lb(h) else { return -1 };
    let i = l.top + y.div_euclid(l.item_h);
    if i < 0 { -1 } else { i.min(l.items.len() as i32 - 1) }
}

/// Text with tabs expanded (LBS_USETABSTOPS) drawn at (x, y).
fn draw_text(h: u16, hdc: u16, x: i32, y: i32, t: &[u8]) {
    if style(h) & LBS_USETABSTOPS == 0 || !t.contains(&b'\t') {
        gdi::text_at(hdc, x, y, t);
        return;
    }
    let tabs = lb(h).map(|l| l.tabs.clone()).unwrap_or_default();
    let step = gdi::ave_width(hdc) * 8;
    let mut cx = 0;
    for (k, part) in t.split(|&c| c == b'\t').enumerate() {
        if k > 0 {
            cx = tabs.iter().copied().find(|&t| t > cx).unwrap_or((cx / step + 1) * step);
        }
        gdi::text_at(hdc, x + cx, y, part);
        cx += gdi::text_width(hdc, part);
    }
}

fn paint(h: u16) {
    let hdc = begin_paint_rs(h);
    let (cx, cy) = client_size(h);
    let st = style(h);
    let font = lb(h).map_or(0, |l| l.font);
    gdi::select_font(hdc, font);
    let br = ctl_color(h, hdc, CTLCOLOR_LISTBOX);
    let fg = if st & WS_DISABLED != 0 { gdi::sys_color(gdi::COLOR_GRAYTEXT) } else { gdi::text_color(hdc) };
    let owner = st & (LBS_OWNERDRAWFIXED | LBS_OWNERDRAWVARIABLE) != 0;
    gdi::fill_rect(hdc, [0, 0, cx, cy], br);
    gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
    let m = multi(h);
    let Some(l) = lb(h) else {
        gdi::release_dc(hdc);
        return;
    };
    let (top, ih, focus, caret, cur) = (l.top, l.item_h, l.focus, l.caret, l.cur);
    let n = l.items.len() as i32;
    let mut i = top;
    while i < n && (i - top) * ih < cy {
        let y = (i - top) * ih;
        let r = [0, y, cx, y + ih];
        let Some(l) = lb(h) else { break };
        let it = &l.items[i as usize];
        let sel = if m { it.sel } else { i == cur };
        let (text, data) = (it.text.clone(), it.data);
        if owner {
            let state = if sel { 1 } else { 0 } | if focus && i == caret { 0x10 } else { 0 };
            draw_item(h, 2, i as u16, 1, state, hdc, r, data);
        } else {
            if sel {
                gdi::fill(hdc, r, gdi::sys_color(gdi::COLOR_HIGHLIGHT));
                gdi::set_text_color(hdc, gdi::sys_color(gdi::COLOR_HIGHLIGHTTEXT));
            } else {
                gdi::set_text_color(hdc, fg);
            }
            draw_text(h, hdc, 2, y, &text);
            if focus && i == caret {
                gdi::focus_rect(hdc, r);
            }
        }
        i += 1;
    }
    // An empty list with the focus shows it on the first line.
    if n == 0 && lb(h).is_some_and(|l| l.focus) && !owner {
        gdi::focus_rect(hdc, [0, 0, cx, ih]);
    }
    gdi::release_dc(hdc);
}

/// Mouse selection: click (and drag) as the style says.
fn click(h: u16, i: i32, keys: u16, drag: bool) {
    let st = style(h);
    if i < 0 {
        return;
    }
    if st & LBS_MULTIPLESEL != 0 {
        if drag {
            return;
        }
        let Some(l) = lb(h) else { return };
        let it = &mut l.items[i as usize];
        it.sel = !it.sel;
        l.caret = i;
        invalidate(h, None);
        notify_lb(h, LBN_SELCHANGE);
    } else if st & LBS_EXTENDEDSEL != 0 {
        let Some(l) = lb(h) else { return };
        let shift = keys & 0x0004 != 0;
        let ctrl = keys & 0x0008 != 0;
        if !drag && !shift && !ctrl {
            l.anchor = i;
        }
        if !ctrl || drag || shift {
            let (a, b) = (l.anchor.min(i), l.anchor.max(i));
            for (k, it) in l.items.iter_mut().enumerate() {
                let k = k as i32;
                if !ctrl {
                    it.sel = k >= a && k <= b;
                } else if k >= a && k <= b {
                    it.sel = true;
                }
            }
        } else {
            l.items[i as usize].sel = !l.items[i as usize].sel;
            l.anchor = i;
        }
        l.caret = i;
        show_item(h, i);
        invalidate(h, None);
        notify_lb(h, LBN_SELCHANGE);
    } else {
        select(h, i, true);
    }
}

fn key(h: u16, vk: u16) {
    let vis = visible(h);
    let st = style(h);
    let Some(l) = lb(h) else { return };
    let n = l.items.len() as i32;
    if n == 0 {
        return;
    }
    let from = if multi(h) { l.caret } else { l.cur.max(0) };
    let to = match vk {
        0x26 | 0x25 => from - 1,
        0x28 | 0x27 => from + 1,
        0x21 => from - vis + 1,
        0x22 => from + vis - 1,
        0x24 => 0,
        0x23 => n - 1,
        0x20 if st & LBS_MULTIPLESEL != 0 => {
            click(h, l.caret.max(0), 0, false);
            return;
        }
        _ => return,
    }
    .clamp(0, n - 1);
    if st & LBS_MULTIPLESEL != 0 {
        l.caret = to;
        show_item(h, to);
        invalidate(h, None);
    } else if st & LBS_EXTENDEDSEL != 0 {
        let shift = key_down(0x10);
        click(h, to, if shift { 4 } else { 0 }, false);
    } else {
        select(h, to, true);
    }
}

pub fn proc_(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    match m {
        W_CREATE => {
            let combo = wnd(h).filter(|w| is_combo(w.parent)).map_or(0, |w| w.parent);
            if let Some(w) = wnd(h) {
                w.data = Some(alloc::boxed::Box::new(ListBox { items: Vec::new(), cur: -1, caret: 0, anchor: 0, top: 0, font: 0, item_h: 16, focus: false, captured: false, tabs: Vec::new(), combo }));
            }
            measure(h);
            update_bar(h);
            0
        }
        W_ERASEBKGND => 1,
        W_PAINT => {
            paint(h);
            0
        }
        W_SIZE => {
            update_bar(h);
            0
        }
        W_SETFONT => {
            if let Some(l) = lb(h) {
                l.font = wp;
            }
            measure(h);
            update_bar(h);
            if lp & 0xffff != 0 {
                invalidate(h, None);
            }
            0
        }
        W_GETFONT => lb(h).map_or(0, |l| l.font as u32),
        W_GETDLGCODE => DLGC_WANTARROWS | DLGC_WANTCHARS,
        W_SETFOCUS => {
            if let Some(l) = lb(h) {
                l.focus = true;
            }
            invalidate(h, None);
            notify_lb(h, LBN_SETFOCUS);
            0
        }
        W_KILLFOCUS => {
            if let Some(l) = lb(h) {
                l.focus = false;
            }
            invalidate(h, None);
            notify_lb(h, LBN_KILLFOCUS);
            0
        }
        W_ENABLE => {
            invalidate(h, None);
            0
        }
        W_LBUTTONDOWN => {
            let combo = lb(h).map_or(0, |l| l.combo);
            if combo == 0 && focus() != h {
                set_focus(h);
            }
            let i = item_at(h, (lp >> 16) as i16 as i32);
            if let Some(c) = wnd(h).map(|w| w.client) {
                unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, c) };
            }
            if let Some(l) = lb(h) {
                l.captured = true;
            }
            click(h, i, wp, false);
            0
        }
        W_MOUSEMOVE => {
            if lb(h).is_some_and(|l| l.captured) {
                let y = (lp >> 16) as i16 as i32;
                let vis = visible(h);
                let i = item_at(h, y);
                if y < 0 || y >= vis * lb(h).map_or(16, |l| l.item_h) {
                    show_item(h, i);
                }
                if !multi(h) || style(h) & LBS_EXTENDEDSEL != 0 {
                    click(h, i, wp, true);
                }
            }
            0
        }
        W_LBUTTONUP => {
            if lb(h).is_some_and(|l| l.captured) {
                if let Some(l) = lb(h) {
                    l.captured = false;
                }
                unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, 0) };
                let combo = lb(h).map_or(0, |l| l.combo);
                if combo != 0 {
                    combo_close(combo, true);
                }
            }
            0
        }
        W_LBUTTONDBLCLK => {
            notify_lb(h, LBN_DBLCLK);
            0
        }
        W_KEYDOWN => {
            if style(h) & LBS_WANTKEYBOARDINPUT != 0 {
                let parent = wnd(h).map_or(0, |w| w.parent);
                let caret = lb(h).map_or(0, |l| l.caret);
                let r = send(parent, W_VKEYTOITEM, wp, (caret as u32) << 16 | h as u32) as i16;
                if r == -2 {
                    return 0;
                }
            }
            key(h, wp);
            0
        }
        W_CHAR => {
            // A letter selects the next item that starts with it.
            if wp > 0x20 && has_strings(h) {
                let from = lb(h).map_or(-1, |l| if multi(h) { l.caret } else { l.cur });
                let i = find(h, from, &[wp as u8], false);
                if i >= 0 {
                    if multi(h) {
                        if let Some(l) = lb(h) {
                            l.caret = i;
                        }
                        show_item(h, i);
                        invalidate(h, None);
                    } else {
                        select(h, i, true);
                    }
                }
            }
            0
        }
        W_VSCROLL => {
            let vis = visible(h);
            let Some(l) = lb(h) else { return 0 };
            let max = (l.items.len() as i32 - vis).max(0);
            l.top = match wp {
                0 => l.top - 1,
                1 => l.top + 1,
                2 => l.top - vis,
                3 => l.top + vis,
                4 | 5 => lp as u16 as i16 as i32,
                6 => 0,
                7 => max,
                _ => return 0,
            }
            .clamp(0, max);
            update_bar(h);
            invalidate(h, None);
            0
        }
        LB_ADDSTRING | LB_INSERTSTRING => {
            let at = if m == LB_ADDSTRING { -1 } else { wp as i16 as i32 };
            let (text, data) = if has_strings(h) { (crate::far_str(lp).unwrap_or(b"").to_vec(), 0) } else { (Vec::new(), lp) };
            insert(h, at, text, data) as u32
        }
        LB_DELETESTRING => {
            let i = wp as usize;
            let Some(l) = lb(h) else { return LB_ERR };
            if i >= l.items.len() {
                return LB_ERR;
            }
            l.items.remove(i);
            let i = i as i32;
            if l.cur == i {
                l.cur = -1;
            } else if l.cur > i {
                l.cur -= 1;
            }
            if l.caret >= l.items.len() as i32 {
                l.caret = (l.items.len() as i32 - 1).max(0);
            }
            let n = l.items.len() as u32;
            update_bar(h);
            invalidate(h, None);
            n
        }
        LB_RESETCONTENT => {
            if let Some(l) = lb(h) {
                l.items.clear();
                l.cur = -1;
                l.caret = 0;
                l.top = 0;
            }
            update_bar(h);
            invalidate(h, None);
            0
        }
        LB_SETSEL => {
            if !multi(h) {
                return LB_ERR;
            }
            let i = lp as u16 as i16 as i32;
            let Some(l) = lb(h) else { return LB_ERR };
            if i == -1 {
                for it in &mut l.items {
                    it.sel = wp != 0;
                }
            } else if let Some(it) = l.items.get_mut(i as usize) {
                it.sel = wp != 0;
                l.caret = i;
            } else {
                return LB_ERR;
            }
            invalidate(h, None);
            0
        }
        LB_SETCURSEL => {
            if multi(h) {
                return LB_ERR;
            }
            let i = wp as i16 as i32;
            select(h, i, false);
            if i < 0 { LB_ERR } else { i as u32 }
        }
        LB_GETSEL => {
            let Some(l) = lb(h) else { return LB_ERR };
            let i = wp as i16 as i32;
            match l.items.get(i as usize) {
                Some(it) => (if multi(h) { it.sel } else { l.cur == i }) as u32,
                None => LB_ERR,
            }
        }
        LB_GETCURSEL => {
            let Some(l) = lb(h) else { return LB_ERR };
            let i = if multi(h) { l.caret } else { l.cur };
            if i < 0 || i as usize >= l.items.len() { LB_ERR } else { i as u32 }
        }
        LB_GETTEXT => {
            let Some(l) = lb(h) else { return LB_ERR };
            let Some(it) = l.items.get(wp as usize) else { return LB_ERR };
            let p = thunk16::flat(lp);
            if !has_strings(h) {
                if !p.is_null() {
                    unsafe { (p as *mut u32).write_unaligned(it.data) };
                }
                return 4;
            }
            let t = it.text.clone();
            if !p.is_null() {
                unsafe {
                    core::ptr::copy_nonoverlapping(t.as_ptr(), p, t.len());
                    *p.add(t.len()) = 0;
                }
            }
            t.len() as u32
        }
        LB_GETTEXTLEN => lb(h).and_then(|l| l.items.get(wp as usize)).map_or(LB_ERR, |it| if has_strings(h) { it.text.len() as u32 } else { 4 }),
        LB_GETCOUNT => lb(h).map_or(0, |l| l.items.len() as u32),
        LB_SELECTSTRING => {
            let s = crate::far_str(lp).unwrap_or(b"").to_vec();
            let i = find(h, wp as i16 as i32, &s, false);
            if i < 0 {
                return LB_ERR;
            }
            if multi(h) {
                if let Some(l) = lb(h) {
                    l.items[i as usize].sel = true;
                    l.caret = i;
                }
                show_item(h, i);
                invalidate(h, None);
            } else {
                select(h, i, false);
            }
            i as u32
        }
        LB_FINDSTRING | LB_FINDSTRINGEXACT => {
            let s = crate::far_str(lp).unwrap_or(b"").to_vec();
            let i = find(h, wp as i16 as i32, &s, m == LB_FINDSTRINGEXACT);
            if i < 0 { LB_ERR } else { i as u32 }
        }
        LB_DIR => LB_ERR,
        LB_GETTOPINDEX => lb(h).map_or(0, |l| l.top as u32),
        LB_SETTOPINDEX => {
            if let Some(l) = lb(h) {
                l.top = wp as i32;
            }
            update_bar(h);
            invalidate(h, None);
            0
        }
        LB_GETSELCOUNT => {
            if !multi(h) {
                return LB_ERR;
            }
            lb(h).map_or(0, |l| l.items.iter().filter(|i| i.sel).count() as u32)
        }
        LB_GETSELITEMS => {
            if !multi(h) {
                return LB_ERR;
            }
            let p = thunk16::flat(lp) as *mut u16;
            let Some(l) = lb(h) else { return 0 };
            let mut n = 0;
            for (i, it) in l.items.iter().enumerate() {
                if it.sel && n < wp as usize {
                    if !p.is_null() {
                        unsafe { p.add(n).write_unaligned(i as u16) };
                    }
                    n += 1;
                }
            }
            n as u32
        }
        LB_SETTABSTOPS => {
            let (bx, _) = base_units();
            let p = thunk16::flat(lp) as *const i16;
            let Some(l) = lb(h) else { return 0 };
            l.tabs.clear();
            if !p.is_null() {
                for i in 0..wp as usize {
                    l.tabs.push(unsafe { p.add(i).read_unaligned() } as i32 * bx / 4);
                }
            }
            1
        }
        LB_GETHORIZONTALEXTENT => 0,
        LB_SETHORIZONTALEXTENT | LB_SETCOLUMNWIDTH => 0,
        LB_GETITEMRECT => {
            let (cx, _) = client_size(h);
            let Some(l) = lb(h) else { return LB_ERR };
            let y = (wp as i32 - l.top) * l.item_h;
            crate::write_rect(lp, [0, y, cx, y + l.item_h]);
            1
        }
        LB_GETITEMDATA => lb(h).and_then(|l| l.items.get(wp as usize)).map_or(LB_ERR, |it| it.data),
        LB_SETITEMDATA => {
            match lb(h).and_then(|l| l.items.get_mut(wp as usize)) {
                Some(it) => {
                    it.data = lp;
                    1
                }
                None => LB_ERR,
            }
        }
        LB_SELITEMRANGE => {
            if !multi(h) {
                return LB_ERR;
            }
            let (a, b) = (lp as u16 as usize, (lp >> 16) as u16 as usize);
            if let Some(l) = lb(h) {
                for (k, it) in l.items.iter_mut().enumerate() {
                    if k >= a.min(b) && k <= a.max(b) {
                        it.sel = wp != 0;
                    }
                }
            }
            invalidate(h, None);
            0
        }
        LB_SETCARETINDEX => {
            if let Some(l) = lb(h) {
                l.caret = wp as i32;
            }
            show_item(h, wp as i32);
            invalidate(h, None);
            0
        }
        LB_GETCARETINDEX => lb(h).map_or(0, |l| l.caret as u32),
        LB_SETITEMHEIGHT => {
            if let Some(l) = lb(h) {
                l.item_h = (lp as u16 as i32).max(1);
            }
            update_bar(h);
            invalidate(h, None);
            0
        }
        LB_GETITEMHEIGHT => lb(h).map_or(16, |l| l.item_h as u32),
        _ => def_window_proc(h, m, wp, lp),
    }
}

// ---- COMBOBOX ----

const CBS_SIMPLE: u32 = 1;
const CBS_DROPDOWN: u32 = 2;
const CBS_DROPDOWNLIST: u32 = 3;
const CBS_AUTOHSCROLL: u32 = 0x0040;
const CBS_SORT: u32 = 0x0100;
const CBS_HASSTRINGS: u32 = 0x0200;

const CB_GETEDITSEL: u16 = W_USER;
const CB_LIMITTEXT: u16 = W_USER + 1;
const CB_SETEDITSEL: u16 = W_USER + 2;
const CB_ADDSTRING: u16 = W_USER + 3;
const CB_DELETESTRING: u16 = W_USER + 4;
const CB_DIR: u16 = W_USER + 5;
const CB_GETCOUNT: u16 = W_USER + 6;
const CB_GETCURSEL: u16 = W_USER + 7;
const CB_GETLBTEXT: u16 = W_USER + 8;
const CB_GETLBTEXTLEN: u16 = W_USER + 9;
const CB_INSERTSTRING: u16 = W_USER + 10;
const CB_RESETCONTENT: u16 = W_USER + 11;
const CB_FINDSTRING: u16 = W_USER + 12;
const CB_SELECTSTRING: u16 = W_USER + 13;
const CB_SETCURSEL: u16 = W_USER + 14;
const CB_SHOWDROPDOWN: u16 = W_USER + 15;
const CB_GETITEMDATA: u16 = W_USER + 16;
const CB_SETITEMDATA: u16 = W_USER + 17;
const CB_GETDROPPEDCONTROLRECT: u16 = W_USER + 18;
const CB_SETITEMHEIGHT: u16 = W_USER + 19;
const CB_GETITEMHEIGHT: u16 = W_USER + 20;
const CB_SETEXTENDEDUI: u16 = W_USER + 21;
const CB_GETEXTENDEDUI: u16 = W_USER + 22;
const CB_GETDROPPEDSTATE: u16 = W_USER + 23;
const CB_FINDSTRINGEXACT: u16 = W_USER + 24;

const CBN_SELCHANGE: u16 = 1;
const CBN_DBLCLK: u16 = 2;
const CBN_SETFOCUS: u16 = 3;
const CBN_KILLFOCUS: u16 = 4;
const CBN_EDITCHANGE: u16 = 5;
const CBN_EDITUPDATE: u16 = 6;
const CBN_DROPDOWN: u16 = 7;
const CBN_CLOSEUP: u16 = 8;
const CBN_SELENDOK: u16 = 9;
const CBN_SELENDCANCEL: u16 = 10;

/// The edit control's ID in a combo box, and the list's.
const ID_EDIT: u16 = 1001;
const ID_LIST: u16 = 1000;

pub struct Combo {
    kind: u32,
    edit: u16,
    list: u16,
    dropped: bool,
    /// The button went down on the combo box and has not come up.
    tracking: bool,
    font: u16,
    /// The selection field's height.
    field_h: i32,
    /// The dropped list's height.
    list_h: i32,
    focus: bool,
    ext_ui: bool,
    /// The selection when the list dropped (for CBN_SELENDCANCEL).
    before: i32,
}

fn cb(h: u16) -> Option<&'static mut Combo> {
    data::<Combo>(h)
}

pub fn is_combo(h: u16) -> bool {
    cb(h).is_some()
}

fn button_w() -> i32 {
    unsafe { crate::pm::WinQuerySysValue(crate::pm::HWND_DESKTOP, crate::pm::SV_CXVSCROLL) }
}

/// The class of a built-in control by name.
fn class(n: &[u8]) -> usize {
    find_class_name(n).unwrap_or(0)
}

fn combo_create(h: u16) {
    let Some(w) = wnd(h) else { return };
    let (st, hinst) = (w.style, w.hinst);
    let kind = st & 3;
    let kind = if kind == 0 { CBS_SIMPLE } else { kind };
    let r = window_rect(h);
    let (cx, total) = (r[2] - r[0], r[3] - r[1]);
    let hdc = get_dc(h);
    let lh = gdi::line_height(hdc);
    gdi::release_dc(hdc);
    let field_h = lh + 8;
    w.data = Some(alloc::boxed::Box::new(Combo { kind, edit: 0, list: 0, dropped: false, tracking: false, font: 0, field_h, list_h: (total - field_h).max(lh * 3 + 2), focus: false, ext_ui: false, before: -1 }));
    // A drop-down combo box is the field's height; the list hangs below.
    if kind != CBS_SIMPLE {
        move_to(h, [r[0], r[1], r[2], r[1] + field_h]);
    }
    let bw = button_w();
    if kind != CBS_DROPDOWNLIST {
        let ew = if kind == CBS_SIMPLE { cx } else { cx - bw };
        let es = WS_CHILD | WS_VISIBLE | WS_BORDER | if st & CBS_AUTOHSCROLL != 0 { 0x80 } else { 0 };
        let e = create(&Create { exstyle: 0, class: class(b"EDIT"), title: b"", title_far: 0, class_far: 0, style: es, pos: [0, 0, ew, field_h], parent: h, menu: ID_EDIT, hinst, param: 0 });
        if let Some(c) = cb(h) {
            c.edit = e;
        }
    }
    let lstyle = LBS_NOTIFY | WS_BORDER | WS_VSCROLL | if st & CBS_SORT != 0 { LBS_SORT } else { 0 } | (st & 0x30) | if st & CBS_HASSTRINGS != 0 { LBS_HASSTRINGS } else { 0 };
    let list = if kind == CBS_SIMPLE {
        create(&Create { exstyle: 0, class: class(b"LISTBOX"), title: b"", title_far: 0, class_far: 0, style: lstyle | WS_CHILD | WS_VISIBLE, pos: [0, field_h, cx, (total - field_h).max(lh)], parent: h, menu: ID_LIST, hinst, param: 0 })
    } else {
        create(&Create { exstyle: 0, class: class(b"COMBOLBOX"), title: b"", title_far: 0, class_far: 0, style: lstyle | WS_POPUP, pos: [0, 0, cx, 10], parent: h, menu: ID_LIST, hinst, param: 0 })
    };
    if let Some(c) = cb(h) {
        c.list = list;
    }
    if let Some(l) = lb(list) {
        l.combo = h;
    }
}

fn list_of(h: u16) -> u16 {
    cb(h).map_or(0, |c| c.list)
}

fn cur_sel(h: u16) -> i32 {
    send(list_of(h), LB_GETCURSEL, 0, 0) as i16 as i32
}

/// The selected item's text into the edit control (or the field).
fn sync_text(h: u16) {
    let list = list_of(h);
    let i = cur_sel(h);
    let t = if i >= 0 { lb(list).and_then(|l| l.items.get(i as usize)).map(|it| it.text.clone()).unwrap_or_default() } else { Vec::new() };
    let edit = cb(h).map_or(0, |c| c.edit);
    if edit != 0 {
        crate::mem16::with_str(&t, |far| send(edit, W_SETTEXT, 0, far));
        send(edit, W_USER + 1, 0, 0xffff_0000);
    }
    invalidate(h, None);
}

/// A notification from the combo box's list.
fn combo_list_notify(h: u16, code: u16) {
    match code {
        LBN_SELCHANGE => {
            sync_text(h);
            notify(h, CBN_SELCHANGE);
        }
        LBN_DBLCLK => notify(h, CBN_DBLCLK),
        _ => {}
    }
}

fn drop_down(h: u16) {
    let Some(c) = cb(h) else { return };
    if c.dropped || c.kind == CBS_SIMPLE {
        return;
    }
    notify(h, CBN_DROPDOWN);
    let Some(c) = cb(h) else { return };
    c.dropped = true;
    c.before = -1;
    let (list, list_h) = (c.list, c.list_h);
    let before = cur_sel(h);
    if let Some(c) = cb(h) {
        c.before = before;
    }
    let r = window_rect(h);
    let (sx, sy) = to_screen(h, 0, 0);
    let (cx, cy) = (r[2] - r[0], r[3] - r[1]);
    let ch = client_size(h).1;
    let _ = cy;
    // The list's height: its items, up to the height given.
    let ih = lb(list).map_or(16, |l| l.item_h);
    let n = lb(list).map_or(0, |l| l.items.len() as i32).max(1);
    let want = (n * ih + 2).min(list_h);
    let (_, scy) = screen_size();
    let top = if sy + ch + want > scy { sy - want } else { sy + ch };
    move_to(list, [sx, top, sx + cx, top + want]);
    if before >= 0 {
        show_item(list, before);
    }
    show_window(list, 8);
    invalidate(h, None);
    if let Some(pm) = wnd(h).map(|w| w.client) {
        unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, pm) };
    }
}

/// Close the dropped list: with the selection (ok) or as it was.
fn combo_close(h: u16, ok: bool) {
    let Some(c) = cb(h) else { return };
    if !c.dropped {
        return;
    }
    c.dropped = false;
    c.tracking = false;
    let (list, before) = (c.list, c.before);
    unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, 0) };
    show_window(list, 0);
    if ok {
        notify(h, CBN_SELENDOK);
    } else {
        if cur_sel(h) != before {
            send(list, LB_SETCURSEL, before as u16, 0);
            sync_text(h);
        }
        notify(h, CBN_SELENDCANCEL);
    }
    notify(h, CBN_CLOSEUP);
    invalidate(h, None);
}

fn paint_combo(h: u16) {
    let hdc = begin_paint_rs(h);
    let (cx, cy) = client_size(h);
    let Some((kind, font, focus, dropped, field_h)) = cb(h).map(|c| (c.kind, c.font, c.focus, c.dropped, c.field_h)) else {
        gdi::release_dc(hdc);
        return;
    };
    gdi::select_font(hdc, font);
    let parent_bk = ctl_color(h, hdc, CTLCOLOR_EDIT);
    if kind == CBS_SIMPLE {
        gdi::fill_rect(hdc, [0, 0, cx, cy], parent_bk);
        gdi::release_dc(hdc);
        return;
    }
    let bw = button_w();
    let fh = field_h.min(cy);
    if kind == CBS_DROPDOWNLIST {
        // The selection field: the item, highlighted with the focus.
        let r = [0, 0, cx - bw, fh];
        gdi::fill_rect(hdc, r, parent_bk);
        gdi::frame(hdc, r, gdi::sys_color(gdi::COLOR_WINDOWFRAME));
        let list = list_of(h);
        let i = cur_sel(h);
        let t = if i >= 0 { lb(list).and_then(|l| l.items.get(i as usize)).map(|it| it.text.clone()).unwrap_or_default() } else { Vec::new() };
        let lh = gdi::line_height(hdc);
        let inner = [3, 3, cx - bw - 3, fh - 3];
        gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
        if focus && !dropped {
            gdi::fill(hdc, inner, gdi::sys_color(gdi::COLOR_HIGHLIGHT));
            gdi::set_text_color(hdc, gdi::sys_color(gdi::COLOR_HIGHLIGHTTEXT));
        }
        gdi::clip(hdc, inner);
        gdi::text_at(hdc, inner[0] + 1, (fh - lh) / 2, &t);
        gdi::unclip(hdc);
        if focus && !dropped {
            gdi::focus_rect(hdc, inner);
        }
    }
    // The button with its arrow.
    let br = [cx - bw, 0, cx, fh];
    gdi::frame(hdc, br, 0);
    crate::scroll::raised(hdc, [br[0] + 1, br[1] + 1, br[2] - 1, br[3] - 1], dropped);
    let enabled = wnd(h).is_some_and(|w| w.style & WS_DISABLED == 0);
    crate::scroll::triangle(hdc, [br[0] + 1, br[1] + 1, br[2] - 1, br[3] - 1], b'd', if enabled { 0 } else { 0x808080 });
    gdi::release_dc(hdc);
}

/// A point of the combo box's client area in its list's.
fn in_list(h: u16, x: i32, y: i32) -> Option<(i32, i32)> {
    let list = list_of(h);
    let (sx, sy) = to_screen(h, x, y);
    let lr = window_rect(list);
    if sx < lr[0] || sx >= lr[2] || sy < lr[1] || sy >= lr[3] {
        return None;
    }
    let (lx, ly) = to_screen(list, 0, 0);
    Some((sx - lx, sy - ly))
}

fn combo_mouse(h: u16, m: u16, wp: u16, lp: u32) {
    let (x, y) = (lp as i16 as i32, (lp >> 16) as i16 as i32);
    let Some(c) = cb(h) else { return };
    let (dropped, kind, list) = (c.dropped, c.kind, c.list);
    let (cx, cy) = client_size(h);
    let on_combo = x >= 0 && y >= 0 && x < cx && y < cy;
    match m {
        W_LBUTTONDOWN | W_LBUTTONDBLCLK => {
            if !dropped {
                let bw = button_w();
                if kind == CBS_DROPDOWNLIST || x >= cx - bw {
                    let f = if kind == CBS_DROPDOWNLIST { h } else { cb(h).map_or(h, |c| c.edit) };
                    if focus() != f {
                        set_focus(f);
                    }
                    drop_down(h);
                    if let Some(c) = cb(h) {
                        c.tracking = true;
                    }
                } else if kind == CBS_DROPDOWN {
                    set_focus(cb(h).map_or(h, |c| c.edit));
                }
                return;
            }
            match in_list(h, x, y) {
                Some((lx, ly)) => {
                    let (lcx, lcy) = client_size(list);
                    if lx < lcx && ly < lcy {
                        let i = item_at(list, ly);
                        if i >= 0 {
                            select(list, i, true);
                        }
                        if let Some(c) = cb(h) {
                            c.tracking = true;
                        }
                    } else if let Some(pm) = wnd(list).and_then(|w| w.vsb.as_ref().map(|b| b.pm)) {
                        // The list's scroll bar.
                        let (sx, sy) = to_screen(h, x, y);
                        let mut p = crate::pm::PointL { x: sx, y: screen_size().1 - 1 - sy };
                        unsafe { crate::pm::WinMapWindowPoints(crate::pm::HWND_DESKTOP, pm, &mut p, 1) };
                        crate::scroll::track(list, 1, p.x, p.y);
                        if let Some(c) = wnd(h).map(|w| w.client) {
                            unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, c) };
                        }
                    }
                }
                None => {
                    // Outside: the list closes (the button toggles it).
                    combo_close(h, false);
                    let _ = on_combo;
                }
            }
        }
        W_MOUSEMOVE => {
            if dropped && wp & 1 != 0 {
                if let Some((_, ly)) = in_list(h, x, y) {
                    let i = item_at(list, ly);
                    if i >= 0 && i != cur_sel(h) {
                        select(list, i, true);
                    }
                }
            }
        }
        W_LBUTTONUP => {
            let tracking = cb(h).is_some_and(|c| c.tracking);
            if let Some(c) = cb(h) {
                c.tracking = false;
            }
            if dropped && tracking {
                if in_list(h, x, y).is_some() {
                    combo_close(h, true);
                }
            }
        }
        _ => {}
    }
}

pub fn combo_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    match m {
        W_CREATE => {
            combo_create(h);
            0
        }
        W_DESTROY => {
            if let Some(l) = cb(h).map(|c| c.list) {
                destroy_window(l);
            }
            0
        }
        W_ERASEBKGND => 1,
        W_PAINT => {
            paint_combo(h);
            0
        }
        W_SETFONT => {
            let Some(c) = cb(h) else { return 0 };
            c.font = wp;
            let (e, l) = (c.edit, c.list);
            if e != 0 {
                send(e, W_SETFONT, wp, lp);
            }
            send(l, W_SETFONT, wp, lp);
            invalidate(h, None);
            0
        }
        W_GETFONT => cb(h).map_or(0, |c| c.font as u32),
        W_GETDLGCODE => DLGC_WANTARROWS | DLGC_WANTCHARS,
        W_SETFOCUS => {
            let Some(c) = cb(h) else { return 0 };
            if c.edit != 0 {
                let e = c.edit;
                set_focus(e);
                send(e, W_USER + 1, 0, 0xffff_0000);
                return 0;
            }
            c.focus = true;
            invalidate(h, None);
            notify(h, CBN_SETFOCUS);
            0
        }
        W_KILLFOCUS => {
            let Some(c) = cb(h) else { return 0 };
            if c.edit == 0 {
                c.focus = false;
                combo_close(h, true);
                invalidate(h, None);
                notify(h, CBN_KILLFOCUS);
            }
            0
        }
        W_COMMAND => {
            // From the edit control.
            let code = (lp >> 16) as u16;
            match code {
                0x0100 => {
                    if let Some(c) = cb(h) {
                        c.focus = true;
                    }
                    notify(h, CBN_SETFOCUS);
                }
                0x0200 => {
                    if let Some(c) = cb(h) {
                        c.focus = false;
                    }
                    combo_close(h, true);
                    notify(h, CBN_KILLFOCUS);
                }
                0x0300 => notify(h, CBN_EDITCHANGE),
                0x0400 => notify(h, CBN_EDITUPDATE),
                _ => {}
            }
            0
        }
        W_LBUTTONDOWN | W_LBUTTONDBLCLK | W_MOUSEMOVE | W_LBUTTONUP => {
            combo_mouse(h, m, wp, lp);
            0
        }
        W_KEYDOWN => {
            let alt = key_down(0x12);
            let list = list_of(h);
            let dropped = cb(h).is_some_and(|c| c.dropped);
            match wp {
                // F4, Alt+Down: drop (or close) the list.
                0x73 => {
                    if dropped {
                        combo_close(h, true);
                    } else {
                        drop_down(h);
                    }
                }
                0x28 if alt => {
                    if dropped {
                        combo_close(h, true);
                    } else {
                        drop_down(h);
                    }
                }
                0x0d if dropped => combo_close(h, true),
                0x1b if dropped => combo_close(h, false),
                0x26 | 0x28 | 0x21 | 0x22 | 0x24 | 0x23 => {
                    let n = send(list, LB_GETCOUNT, 0, 0) as i32;
                    let i = cur_sel(h);
                    let to = match wp {
                        0x26 => i - 1,
                        0x28 => i + 1,
                        0x21 => i - 8,
                        0x22 => i + 8,
                        0x24 => 0,
                        _ => n - 1,
                    }
                    .clamp(0, (n - 1).max(0));
                    if n > 0 && to != i {
                        select(list, to, true);
                    }
                }
                _ => {}
            }
            0
        }
        W_CHAR => {
            if cb(h).is_some_and(|c| c.kind == CBS_DROPDOWNLIST) && wp > 0x20 {
                let list = list_of(h);
                let i = find(list, cur_sel(h), &[wp as u8], false);
                if i >= 0 {
                    select(list, i, true);
                }
            }
            0
        }
        W_ENABLE => {
            let Some(c) = cb(h) else { return 0 };
            let (e, l) = (c.edit, c.list);
            if e != 0 {
                enable_window(e, wp != 0);
            }
            enable_window(l, wp != 0);
            invalidate(h, None);
            0
        }
        W_SETTEXT => {
            let e = cb(h).map_or(0, |c| c.edit);
            if e != 0 {
                return send(e, W_SETTEXT, wp, lp);
            }
            let s = crate::far_str(lp).unwrap_or(b"").to_vec();
            let i = find(list_of(h), -1, &s, true);
            send(list_of(h), LB_SETCURSEL, i as u16, 0);
            invalidate(h, None);
            (i >= 0) as u32
        }
        W_GETTEXT | W_GETTEXTLENGTH => {
            let e = cb(h).map_or(0, |c| c.edit);
            if e != 0 {
                return send(e, m, wp, lp);
            }
            let list = list_of(h);
            let i = cur_sel(h);
            let t = if i >= 0 { lb(list).and_then(|l| l.items.get(i as usize)).map(|it| it.text.clone()).unwrap_or_default() } else { Vec::new() };
            if m == W_GETTEXTLENGTH {
                return t.len() as u32;
            }
            copy_out(&t, thunk16::flat(lp), wp as i32) as u32
        }
        CB_GETEDITSEL => {
            let e = cb(h).map_or(0, |c| c.edit);
            if e == 0 { LB_ERR } else { send(e, W_USER, 0, 0) }
        }
        CB_SETEDITSEL => {
            let e = cb(h).map_or(0, |c| c.edit);
            if e == 0 { LB_ERR } else { send(e, W_USER + 1, 0, lp) }
        }
        CB_LIMITTEXT => {
            let e = cb(h).map_or(0, |c| c.edit);
            if e == 0 { LB_ERR } else { send(e, W_USER + 21, wp, 0) | 1 }
        }
        CB_ADDSTRING => send(list_of(h), LB_ADDSTRING, wp, lp),
        CB_INSERTSTRING => send(list_of(h), LB_INSERTSTRING, wp, lp),
        CB_DELETESTRING => send(list_of(h), LB_DELETESTRING, wp, lp),
        CB_RESETCONTENT => {
            send(list_of(h), LB_RESETCONTENT, 0, 0);
            let e = cb(h).map_or(0, |c| c.edit);
            if e != 0 {
                crate::mem16::with_str(b"", |far| send(e, W_SETTEXT, 0, far));
            }
            invalidate(h, None);
            0
        }
        CB_GETCOUNT => send(list_of(h), LB_GETCOUNT, 0, 0),
        CB_GETCURSEL => send(list_of(h), LB_GETCURSEL, 0, 0),
        CB_SETCURSEL => {
            let r = send(list_of(h), LB_SETCURSEL, wp, 0);
            sync_text(h);
            r
        }
        CB_GETLBTEXT => send(list_of(h), LB_GETTEXT, wp, lp),
        CB_GETLBTEXTLEN => send(list_of(h), LB_GETTEXTLEN, wp, lp),
        CB_FINDSTRING => send(list_of(h), LB_FINDSTRING, wp, lp),
        CB_FINDSTRINGEXACT => send(list_of(h), LB_FINDSTRINGEXACT, wp, lp),
        CB_SELECTSTRING => {
            let r = send(list_of(h), LB_SELECTSTRING, wp, lp);
            sync_text(h);
            r
        }
        CB_SHOWDROPDOWN => {
            if wp != 0 {
                drop_down(h);
            } else {
                combo_close(h, true);
            }
            1
        }
        CB_GETITEMDATA => send(list_of(h), LB_GETITEMDATA, wp, lp),
        CB_SETITEMDATA => send(list_of(h), LB_SETITEMDATA, wp, lp),
        CB_GETDROPPEDCONTROLRECT => {
            let r = window_rect(h);
            let lh = cb(h).map_or(0, |c| c.list_h);
            let (sx, sy) = to_screen(h, 0, 0);
            let _ = sy;
            crate::write_rect(lp, [sx, r[1], sx + (r[2] - r[0]), r[3] + lh]);
            1
        }
        CB_SETITEMHEIGHT => send(list_of(h), LB_SETITEMHEIGHT, 0, lp),
        CB_GETITEMHEIGHT => send(list_of(h), LB_GETITEMHEIGHT, 0, 0),
        CB_SETEXTENDEDUI => {
            if let Some(c) = cb(h) {
                c.ext_ui = wp != 0;
            }
            0
        }
        CB_GETEXTENDEDUI => cb(h).is_some_and(|c| c.ext_ui) as u32,
        CB_GETDROPPEDSTATE => cb(h).is_some_and(|c| c.dropped) as u32,
        CB_DIR => LB_ERR,
        _ => def_window_proc(h, m, wp, lp),
    }
}

#[allow(dead_code)]
const UNUSED: [u16; 3] = [LBN_SELCANCEL, CTLCOLOR_LISTBOX, CTLCOLOR_EDIT];
