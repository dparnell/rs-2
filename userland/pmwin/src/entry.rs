//! Entry fields (WC_ENTRYFIELD): a single line of editable text (the
//! window text) with a cursor, a selection, a text limit, insert and
//! overwrite modes, horizontal scrolling and EM_* messages.

use alloc::vec::Vec;

use gpi::Rect;

use crate::ctl::{self, *};
use crate::draw;
use crate::lock::wndproc;
use crate::msg::WinDefWindowProc;
use crate::state::st;
use crate::win::{WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_MOUSEMOVE, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;

const ES_CENTER: u32 = 0x0001;
const ES_RIGHT: u32 = 0x0002;
const ES_MARGIN: u32 = 0x0008;
const ES_READONLY: u32 = 0x0020;
const ES_UNREADABLE: u32 = 0x0080;

const EM_QUERYCHANGED: u32 = 0x0140;
const EM_QUERYSEL: u32 = 0x0141;
const EM_SETSEL: u32 = 0x0142;
const EM_SETTEXTLIMIT: u32 = 0x0143;
const EM_CUT: u32 = 0x0144;
const EM_COPY: u32 = 0x0145;
const EM_CLEAR: u32 = 0x0146;
const EM_PASTE: u32 = 0x0147;
const EM_QUERYFIRSTCHAR: u32 = 0x0148;
const EM_SETFIRSTCHAR: u32 = 0x0149;
const EM_QUERYREADONLY: u32 = 0x014a;
const EM_SETREADONLY: u32 = 0x014b;
const EM_SETINSERTMODE: u32 = 0x014c;

const EN_SETFOCUS: u32 = 0x0001;
const EN_KILLFOCUS: u32 = 0x0002;
const EN_CHANGE: u32 = 0x0004;
const EN_SCROLL: u32 = 0x0008;
const EN_OVERFLOW: u32 = 0x0020;
const EN_INSERTMODETOGGLE: u32 = 0x0040;

/// Margin drawn around ES_MARGIN fields (the window grows by it).
const MARGIN: i32 = 3;

wndproc! {
    entry_entry => entry_proc;
}

struct Entry {
    limit: usize,
    cursor: usize,
    anchor: usize,
    first: usize,
    changed: bool,
    insert: bool,
    tracking: bool,
}

fn data(h: u32) -> Option<&'static mut Entry> {
    ctl::data::<Entry>(h)
}

fn sel(e: &Entry) -> (usize, usize) {
    (e.cursor.min(e.anchor), e.cursor.max(e.anchor))
}

fn inset(h: u32) -> i32 {
    if ctl::style(h) & ES_MARGIN != 0 { MARGIN } else { 1 }
}

/// The text as shown (asterisks in an ES_UNREADABLE field).
fn shown(h: u32) -> Vec<u8> {
    let t = ctl::text(h);
    if ctl::style(h) & ES_UNREADABLE != 0 { alloc::vec![b'*'; t.len()] } else { t }
}

/// The width text may take in the field.
fn room(h: u32) -> i32 {
    let (cx, _) = ctl::size(h);
    (cx - 2 * inset(h)).max(1)
}

/// Characters from `first` that fit in the field (at least one).
fn columns(h: u32, first: usize) -> usize {
    let t = shown(h);
    let (f, w) = (draw::font_of(h), room(h));
    let mut x = 0;
    let mut n = 0;
    for &b in t.iter().skip(first) {
        x += f.char_width(b);
        if x > w {
            break;
        }
        n += 1;
    }
    n.max(1)
}

/// Scroll so the cursor is visible.
fn reveal(h: u32) {
    let t = shown(h);
    let w = room(h);
    let Some(e) = data(h) else { return };
    let old = e.first;
    if e.cursor < e.first {
        e.first = e.cursor;
    } else {
        // The caret may sit just after the last character shown.
        let c = e.cursor.min(t.len());
        while e.first < c && draw::font_of(h).text_width(&t[e.first..c]) >= w {
            e.first += 1;
        }
    }
    if e.first != old {
        ctl::notify(h, EN_SCROLL);
    }
}

/// The window text changed (WinSetWindowText): cursor to the start.
pub fn text_changed(h: u32) {
    let Some(e) = data(h) else { return };
    let limit = e.limit;
    if let Some(w) = st().desk.get_mut(h) {
        w.text.truncate(limit);
    }
    let e = data(h).expect("entry");
    (e.cursor, e.anchor, e.first, e.changed) = (0, 0, 0, true);
    ctl::notify(h, EN_CHANGE);
}

/// Replace the selection with `t`.
fn replace(h: u32, t: &[u8]) -> bool {
    if ctl::style(h) & ES_READONLY != 0 {
        return false;
    }
    let Some(e) = data(h) else { return false };
    let (a, b) = sel(e);
    let mut text = ctl::text(h);
    let mut b = b.min(text.len());
    if !e.insert && a == b && !t.is_empty() {
        b = (a + t.len()).min(text.len());
    }
    if text.len() - (b - a) + t.len() > e.limit {
        ctl::notify(h, EN_OVERFLOW);
        crate::misc::WinAlarm(1, 0);
        return false;
    }
    text.splice(a..b, t.iter().copied());
    (e.cursor, e.anchor, e.changed) = (a + t.len(), a + t.len(), true);
    if let Some(w) = st().desk.get_mut(h) {
        w.text = text;
    }
    reveal(h);
    ctl::invalidate(h);
    ctl::notify(h, EN_CHANGE);
    true
}

fn move_cursor(h: u32, to: usize, extend: bool) {
    let len = ctl::text(h).len();
    let Some(e) = data(h) else { return };
    e.cursor = to.min(len);
    if !extend {
        e.anchor = e.cursor;
    }
    reveal(h);
    ctl::invalidate(h);
}

/// Character index at a window x position.
fn index_at(h: u32, x: i32) -> usize {
    let first = data(h).map_or(0, |e| e.first);
    let t = shown(h);
    let f = draw::font_of(h);
    let mut at = inset(h);
    let mut i = first;
    while i < t.len() {
        let cw = f.char_width(t[i]);
        if x < at + cw / 2 {
            break;
        }
        at += cw;
        i += 1;
    }
    i.min(t.len())
}

/// Copy the selection to the clipboard (not from ES_UNREADABLE fields).
fn copy(h: u32) -> bool {
    let Some(e) = data(h) else { return false };
    let (a, b) = sel(e);
    let text = ctl::text(h);
    let b = b.min(text.len());
    if a >= b || ctl::style(h) & ES_UNREADABLE != 0 {
        return false;
    }
    crate::clip::set_text(&text[a..b])
}

/// Replace the selection with the clipboard's text (its first line).
fn paste(h: u32) -> bool {
    let Some(t) = crate::clip::text() else { return false };
    let line: alloc::vec::Vec<u8> = t.iter().copied().take_while(|&c| c != b'\r' && c != b'\n').collect();
    replace(h, &line)
}

fn key(h: u32, mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.up() {
        return false;
    }
    match k.vk {
        VK_INSERT if k.shift() && !k.ctrl() && !k.alt() => {
            paste(h);
            return true;
        }
        VK_INSERT if k.ctrl() && !k.shift() && !k.alt() => {
            copy(h);
            return true;
        }
        VK_DELETE if k.shift() && !k.ctrl() && !k.alt() => {
            if copy(h) {
                replace(h, b"");
            }
            return true;
        }
        _ => {}
    }
    let len = ctl::text(h).len();
    let Some(e) = data(h) else { return false };
    let (cur, (a, b)) = (e.cursor, sel(e));
    let shift = k.shift();
    match k.vk {
        VK_LEFT if !k.alt() => move_cursor(h, cur.saturating_sub(1), shift),
        VK_RIGHT if !k.alt() => move_cursor(h, cur + 1, shift),
        VK_HOME => move_cursor(h, 0, shift),
        VK_END => move_cursor(h, len, shift),
        VK_BACKSPACE if !k.alt() => {
            if a == b && a > 0 {
                e.anchor = a - 1;
            }
            replace(h, b"");
        }
        VK_DELETE if !shift && !k.ctrl() => {
            if a == b && b < len {
                e.cursor = b + 1;
                e.anchor = a;
            }
            replace(h, b"");
        }
        VK_INSERT if !shift && !k.ctrl() => {
            e.insert = !e.insert;
            ctl::notify(h, EN_INSERTMODETOGGLE);
        }
        _ => {
            if k.ch >= 0x20 && !k.ctrl() && !k.alt() && k.vk != VK_TAB && k.vk != VK_NEWLINE && k.vk != VK_ENTER && k.vk != VK_ESC {
                replace(h, &[k.ch]);
                return true;
            }
            return false;
        }
    }
    true
}

fn paint(h: u32) {
    let style = ctl::style(h);
    let text = ctl::text(h);
    let focus = ctl::has_focus(h);
    let enabled = ctl::enabled(h);
    let Some(e) = data(h) else { return };
    let (first, cursor, (sa, sb)) = (e.first, e.cursor, sel(e));
    let m = inset(h);
    let cols = columns(h, first);
    let col = crate::pp::text_colors(h);
    draw::paint(h, |c| {
        let r = c.rect();
        c.fill(r, col.bg);
        if style & ES_MARGIN != 0 {
            c.sunken(r);
            c.bevel(Rect::new(r.left + 1, r.bottom + 1, r.right - 1, r.top - 1), draw::DARK, draw::face());
        }
        let area = Rect::new(r.left + m, r.bottom + m, r.right - m, r.top - m);
        let f = draw::font_of(h);
        let shown: Vec<u8> = if style & ES_UNREADABLE != 0 { alloc::vec![b'*'; text.len()] } else { text.clone() };
        let vis = &shown[first.min(shown.len())..(first + cols).min(shown.len())];
        let w = draw::font_of(h).text_width(vis);
        let x0 = if first == 0 && style & ES_CENTER != 0 {
            area.left + (area.width() - w) / 2
        } else if first == 0 && style & ES_RIGHT != 0 {
            area.right - w
        } else {
            area.left
        };
        let y = area.bottom + (area.height() - f.height) / 2 + f.descender;
        let _ = enabled;
        c.text(x0, y, vis, col.fg, area);
        // The selection, drawn inverted.
        if focus && sa < sb {
            let a = sa.max(first) - first;
            let b = sb.min(first + cols).max(first) - first;
            if a < b {
                let (xa, xb) = (x0 + f.text_width(&vis[..a.min(vis.len())]), x0 + f.text_width(&vis[..b.min(vis.len())]));
                let sr = Rect::new(xa, y - f.descender, xb, y - f.descender + f.height).intersect(&area);
                c.fill(sr, col.hilite_bg);
                c.text(xa, y, &vis[a.min(vis.len())..b.min(vis.len())], col.hilite_fg, area);
            }
        }
        if focus && cursor >= first {
            let x = (x0 + f.text_width(&shown[first.min(shown.len())..cursor.min(shown.len())])).min(area.right - 1);
            let cr = Rect::new(x, y - f.descender, x + 1, y - f.descender + f.height).intersect(&area);
            c.fill(cr, col.fg);
        }
    });
}

pub extern "C" fn entry_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            // ENTRYFDATA: cb, cchEditLimit, ichMinSel, ichMaxSel.
            let limit = if mp1 != 0 { (unsafe { ((mp1 + 2) as *const u16).read_unaligned() }) as usize } else { 32 };
            let len = ctl::text(h).len();
            ctl::set_data(h, Entry { limit: limit.max(len), cursor: 0, anchor: 0, first: 0, changed: false, insert: true, tracking: false });
            if ctl::style(h) & ES_MARGIN != 0 {
                if let Some(w) = st().desk.get(h) {
                    let (x, y, cx, cy) = (w.x - MARGIN, w.y - MARGIN, w.cx + 2 * MARGIN, w.cy + 2 * MARGIN);
                    st().desk.set_pos(h, x, y, cx, cy);
                }
            }
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
            crate::win::WinSetFocus(1, h);
            let i = index_at(h, ctl::mouse_pos(mp1).x);
            if msg == WM_BUTTON1DBLCLK {
                let len = ctl::text(h).len();
                if let Some(e) = data(h) {
                    (e.anchor, e.cursor) = (0, len);
                }
                ctl::invalidate(h);
                return 1;
            }
            move_cursor(h, i, Key::from(0, 0).shift());
            if let Some(e) = data(h) {
                e.tracking = true;
            }
            st().capture = h;
            1
        }
        WM_MOUSEMOVE => {
            crate::pointer::set_system(pmwin::sysptr::SPTR_TEXT);
            if data(h).is_some_and(|e| e.tracking) {
                let i = index_at(h, ctl::mouse_pos(mp1).x);
                move_cursor(h, i, true);
            }
            1
        }
        WM_BUTTON1UP => {
            if let Some(e) = data(h) {
                e.tracking = false;
            }
            if st().capture == h {
                st().capture = 0;
            }
            1
        }
        WM_CHAR => {
            if key(h, mp1, mp2) {
                return 1;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_SETFOCUS => {
            ctl::invalidate(h);
            ctl::notify(h, if mp2 & 0xffff != 0 { EN_SETFOCUS } else { EN_KILLFOCUS });
            0
        }
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        WM_QUERYDLGCODE => DLGC_ENTRYFIELD,
        EM_QUERYCHANGED => data(h).map_or(0, |e| core::mem::take(&mut e.changed) as u32),
        EM_QUERYSEL => data(h).map_or(0, |e| {
            let (a, b) = sel(e);
            mp(a as u32, b as u32)
        }),
        WM_SIZE => {
            // Sized (a combo box sizes its field after the text was set):
            // the text from the start, as much as shows the cursor.
            if let Some(e) = data(h) {
                e.first = 0;
            }
            reveal(h);
            ctl::invalidate(h);
            0
        }
        EM_SETSEL => {
            let len = ctl::text(h).len();
            if let Some(e) = data(h) {
                e.anchor = (lo(mp1) as usize).min(len);
                e.cursor = (hi(mp1) as usize).min(len);
            }
            reveal(h);
            ctl::invalidate(h);
            1
        }
        EM_SETTEXTLIMIT => {
            if let Some(e) = data(h) {
                e.limit = lo(mp1) as usize;
            }
            1
        }
        EM_CLEAR => replace(h, b"") as u32,
        EM_CUT => (copy(h) && replace(h, b"")) as u32,
        EM_COPY => copy(h) as u32,
        EM_PASTE => paste(h) as u32,
        EM_QUERYFIRSTCHAR => data(h).map_or(0, |e| e.first as u32),
        EM_SETFIRSTCHAR => {
            if let Some(e) = data(h) {
                e.first = lo(mp1) as usize;
            }
            ctl::invalidate(h);
            1
        }
        EM_QUERYREADONLY => (ctl::style(h) & ES_READONLY != 0) as u32,
        EM_SETREADONLY => {
            if let Some(w) = st().desk.get_mut(h) {
                if mp1 & 0xffff != 0 {
                    w.style |= ES_READONLY;
                } else {
                    w.style &= !ES_READONLY;
                }
            }
            1
        }
        EM_SETINSERTMODE => data(h).map_or(0, |e| core::mem::replace(&mut e.insert, mp1 & 0xffff != 0) as u32),
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
