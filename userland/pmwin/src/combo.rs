//! Combination boxes (WC_COMBOBOX): an entry field (CBID_EDIT) over a
//! list box (CBID_LIST). CBS_SIMPLE shows the list all the time;
//! CBS_DROPDOWN and CBS_DROPDOWNLIST (read-only entry) drop it down from
//! a button. The window covers only the entry while the list is up and
//! grows to its template height (brought to the top of its siblings)
//! when the list drops down. LM_* messages go to the list, EM_* to the
//! entry; the owner gets CBN_* notifications.

use gpi::Rect;

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc};
use crate::state::st;
use crate::win::{self, WM_BUTTON1DOWN, WM_CHAR, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;

const CBS_SIMPLE: u32 = 0x0001;
const CBS_DROPDOWN: u32 = 0x0002;
const CBS_DROPDOWNLIST: u32 = 0x0004;

const CBM_SHOWLIST: u32 = 0x0170;
const CBM_HILITE: u32 = 0x0171;
const CBM_ISLISTSHOWING: u32 = 0x0172;

const CBN_EFCHANGE: u32 = 1;
const CBN_EFSCROLL: u32 = 2;
const CBN_LBSELECT: u32 = 4;
const CBN_LBSCROLL: u32 = 5;
const CBN_SHOWLIST: u32 = 6;
const CBN_ENTER: u32 = 7;

pub const CBID_LIST: u32 = 0x029a;
pub const CBID_EDIT: u32 = 0x029b;

const EN_CHANGE: u32 = 0x0004;
const EN_SCROLL: u32 = 0x0008;
const LN_SELECT: u32 = 1;
const LN_SCROLL: u32 = 4;
const LN_ENTER: u32 = 5;

const LM_QUERYITEMCOUNT: u32 = 0x0160;
const LM_SELECTITEM: u32 = 0x0164;
const LM_QUERYSELECTION: u32 = 0x0165;
const LM_QUERYITEMTEXT: u32 = 0x0168;
const LM_QUERYITEMTEXTLENGTH: u32 = 0x0167;
const LIT_NONE: u32 = 0xffff_ffff;
const ES_READONLY: u32 = 0x0020;
const EM_SETSEL: u32 = 0x0142;
const ES_AUTOSCROLL: u32 = 0x0004;

wndproc! {
    combo_entry => combo_proc;
}

struct Combo {
    entry: u32,
    list: u32,
    /// Height of the whole control (entry and list) as created.
    full: i32,
    showing: bool,
    hilite: bool,
}

fn data(h: u32) -> Option<&'static mut Combo> {
    ctl::data::<Combo>(h)
}

pub fn is_combo(h: u32) -> bool {
    data(h).is_some()
}

fn simple(h: u32) -> bool {
    let s = ctl::style(h);
    s & CBS_SIMPLE != 0 || s & (CBS_DROPDOWN | CBS_DROPDOWNLIST) == 0
}

/// Height of the entry part.
fn entry_h(h: u32) -> i32 {
    draw::font_of(h).height + 8
}

fn button_w(h: u32) -> i32 {
    if simple(h) { 0 } else { entry_h(h) }
}

/// Place the children for the current window size.
fn layout(h: u32) {
    let (cx, cy) = ctl::size(h);
    let eh = entry_h(h);
    let bw = button_w(h);
    let Some(d) = data(h) else { return };
    let (e, l) = (d.entry, d.list);
    st().desk.set_pos(e, 3, cy - eh + 3, (cx - bw - 6).max(0), eh - 6);
    let lh = (cy - eh).max(0);
    st().desk.set_pos(l, 0, 0, cx, lh);
    win::show(l, simple(h) || data(h).is_some_and(|d| d.showing));
    send(l, WM_SIZE, 0, mp(cx as u32, lh as u32));
}

/// Drop the list down or pull it up.
fn show_list(h: u32, on: bool) {
    if simple(h) {
        return;
    }
    let Some(d) = data(h) else { return };
    if d.showing == on {
        return;
    }
    d.showing = on;
    let full = d.full;
    let Some(w) = st().desk.get(h) else { return };
    let (x, y, cx, cy) = (w.x, w.y, w.cx, w.cy);
    let eh = entry_h(h);
    let top = y + cy;
    let new_cy = if on { full.max(eh) } else { eh };
    st().desk.set_pos(h, x, top - new_cy, cx, new_cy);
    if on {
        st().desk.to_top(h);
    }
    layout(h);
    ctl::invalidate(h);
    if on {
        ctl::notify(h, CBN_SHOWLIST);
    }
}

/// The list's selection into the entry field.
fn take_selection(h: u32) {
    let Some(d) = data(h) else { return };
    let (e, l) = (d.entry, d.list);
    let i = send(l, LM_QUERYSELECTION, LIT_NONE, 0);
    if i == LIT_NONE {
        return;
    }
    let n = send(l, LM_QUERYITEMTEXTLENGTH, i, 0);
    if n == LIT_NONE {
        return;
    }
    let mut buf = alloc::vec![0u8; n as usize + 1];
    send(l, LM_QUERYITEMTEXT, mp(i, n + 1), buf.as_mut_ptr() as u32);
    buf.truncate(n as usize);
    win::set_text(e, &buf);
    send(e, EM_SETSEL, mp(0, 0x7fff), 0);
}

/// Select the next or previous list item (arrow keys in the entry).
fn step(h: u32, delta: i32) {
    let Some(d) = data(h) else { return };
    let l = d.list;
    let n = send(l, LM_QUERYITEMCOUNT, 0, 0) as i32;
    if n == 0 {
        return;
    }
    let cur = send(l, LM_QUERYSELECTION, LIT_NONE, 0);
    let next = if cur == LIT_NONE { 0 } else { (cur as i32 + delta).clamp(0, n - 1) };
    send(l, LM_SELECTITEM, next as u32, 1);
}

fn paint(h: u32) {
    let (cx, cy) = ctl::size(h);
    let eh = entry_h(h);
    let bw = button_w(h);
    let showing = data(h).is_some_and(|d| d.showing);
    let hilite = data(h).is_some_and(|d| d.hilite);
    let enabled = ctl::enabled(h);
    draw::paint(h, |c: &mut Canvas| {
        let er = Rect::new(0, cy - eh, cx - bw, cy);
        c.fill(er, if hilite { draw::hilite() } else { draw::window() });
        c.sunken(er);
        c.bevel(Rect::new(er.left + 1, er.bottom + 1, er.right - 1, er.top - 1), draw::DARK, draw::face());
        if bw > 0 {
            let br = Rect::new(cx - bw, cy - eh, cx, cy);
            c.fill(br, draw::face());
            if showing { c.sunken(br) } else { c.raised(br) }
            // A down arrow.
            let col = if enabled { draw::text() } else { draw::shadow() };
            let (mx, my) = (br.left + br.width() / 2, br.bottom + br.height() / 2);
            for i in 0..4 {
                c.fill(Rect::new(mx - 3 + i, my + 1 - i, mx + 4 - i, my + 2 - i), col);
            }
            c.fill(Rect::new(mx - 4, my - 5, mx + 5, my - 4), col);
        }
    });
}

// ---- Window text: the entry field's ----

pub fn text(h: u32) -> Option<alloc::vec::Vec<u8>> {
    let e = data(h)?.entry;
    Some(ctl::text(e))
}

pub fn set_text(h: u32, t: &[u8]) {
    let Some(e) = data(h).map(|d| d.entry) else { return };
    win::set_text(e, t);
}

pub extern "C" fn combo_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            let style = ctl::style(h);
            let (cx, cy) = ctl::size(h);
            let full = cy;
            let ro = if style & CBS_DROPDOWNLIST != 0 { ES_READONLY } else { 0 };
            let text = ctl::text(h);
            let entry = win::create(h, b"#6", &text, pmwin::ws::VISIBLE | ES_AUTOSCROLL | ro, Rect::default(), h, CBID_EDIT, 0);
            let list = win::create(h, b"#7", b"", 0, Rect::new(0, 0, cx, 0), h, CBID_LIST, 0);
            ctl::set_data(h, Combo { entry, list, full, showing: false, hilite: false });
            // The entry field keeps its own text limit (no ENTRYFDATA).
            send(entry, 0x0143, 256, 0);
            if !simple(h) {
                // Collapsed to the entry: the list drops down over what is
                // below it.
                if let Some(w) = st().desk.get(h) {
                    let (x, y, cx, cy) = (w.x, w.y, w.cx, w.cy);
                    let eh = entry_h(h);
                    st().desk.set_pos(h, x, y + cy - eh, cx, eh);
                }
            }
            layout(h);
            0
        }
        WM_SIZE => {
            // The application sized the control: that is its full size
            // unless the list is up and down.
            if let Some(d) = data(h) {
                let (_, cy) = ctl::size(h);
                if simple(h) || d.showing {
                    d.full = cy;
                } else if cy > entry_h(h) {
                    d.full = cy;
                    if let Some(w) = st().desk.get(h) {
                        let (x, y, cx, cy) = (w.x, w.y, w.cx, w.cy);
                        let eh = entry_h(h);
                        st().desk.set_pos(h, x, y + cy - eh, cx, eh);
                    }
                }
            }
            layout(h);
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON1DBLCLK => {
            if !ctl::enabled(h) {
                return 1;
            }
            let p = ctl::mouse_pos(mp1);
            let (cx, cy) = ctl::size(h);
            let eh = entry_h(h);
            let entry = data(h).map_or(0, |d| d.entry);
            if p.y >= cy - eh && (p.x >= cx - button_w(h) || ctl::style(h) & CBS_DROPDOWNLIST != 0) {
                let on = !data(h).is_some_and(|d| d.showing);
                show_list(h, on);
            }
            crate::win::WinSetFocus(1, entry);
            1
        }
        WM_SETFOCUS => {
            if mp2 & 0xffff != 0 {
                if let Some(e) = data(h).map(|d| d.entry) {
                    crate::win::WinSetFocus(1, e);
                }
            }
            0
        }
        WM_CHAR => {
            let k = Key::from(mp1, mp2);
            if k.up() {
                return WinDefWindowProc(h, msg, mp1, mp2);
            }
            let showing = data(h).is_some_and(|d| d.showing);
            match k.vk {
                VK_DOWN if k.alt() => show_list(h, !showing),
                0x23 if !k.alt() => show_list(h, !showing), // VK_F4
                VK_DOWN => step(h, 1),
                VK_UP if !k.alt() => step(h, -1),
                VK_ENTER | VK_NEWLINE if showing => {
                    show_list(h, false);
                    ctl::notify(h, CBN_ENTER);
                }
                VK_ESC if showing => show_list(h, false),
                _ => return WinDefWindowProc(h, msg, mp1, mp2),
            }
            1
        }
        WM_CONTROL => {
            let (id, code) = (lo(mp1), hi(mp1));
            match (id, code) {
                (CBID_EDIT, EN_CHANGE) => {
                    ctl::notify(h, CBN_EFCHANGE);
                }
                (CBID_EDIT, EN_SCROLL) => {
                    ctl::notify(h, CBN_EFSCROLL);
                }
                (CBID_EDIT, 0x0002) => {
                    // EN_KILLFOCUS: focus left the control.
                    let f = st().focus;
                    if f != h && !st().desk.is_ancestor(h, f) {
                        show_list(h, false);
                    }
                }
                (CBID_LIST, LN_SELECT) => {
                    take_selection(h);
                    ctl::notify(h, CBN_LBSELECT);
                    // A click in the dropped list chooses the item.
                    if st().buttons & 1 != 0 && data(h).is_some_and(|d| d.showing) {
                        show_list(h, false);
                        if let Some(e) = data(h).map(|d| d.entry) {
                            crate::win::WinSetFocus(1, e);
                        }
                    }
                }
                (CBID_LIST, LN_SCROLL) => {
                    ctl::notify(h, CBN_LBSCROLL);
                }
                (CBID_LIST, LN_ENTER) => {
                    take_selection(h);
                    show_list(h, false);
                    ctl::notify(h, CBN_ENTER);
                }
                (CBID_LIST, 0x0003) => {
                    // LN_KILLFOCUS
                    let f = st().focus;
                    if f != h && !st().desk.is_ancestor(h, f) {
                        show_list(h, false);
                    }
                }
                _ => {}
            }
            0
        }
        // Its entry field takes typing (the dialog selects its text when
        // it gets the focus).
        WM_QUERYDLGCODE => DLGC_ENTRYFIELD,
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        CBM_SHOWLIST => {
            show_list(h, mp1 & 0xffff != 0);
            1
        }
        CBM_ISLISTSHOWING => (simple(h) || data(h).is_some_and(|d| d.showing)) as u32,
        CBM_HILITE => {
            let Some(d) = data(h) else { return 0 };
            let old = d.hilite;
            d.hilite = mp1 & 0xffff != 0;
            ctl::invalidate(h);
            old as u32
        }
        // List box messages go to the list, entry field messages to the
        // entry.
        0x0160..=0x016f => {
            let l = data(h).map_or(0, |d| d.list);
            send(l, msg, mp1, mp2)
        }
        0x0140..=0x014f => {
            let e = data(h).map_or(0, |d| d.entry);
            send(e, msg, mp1, mp2)
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
