//! List boxes (WC_LISTBOX): a scrolling list of text items with single
//! or multiple selection, LM_* messages and LN_* notifications. The
//! vertical scroll bar is a WC_SCROLLBAR child.

use alloc::vec::Vec;

use gpi::Rect;

use crate::ctl::{self, *};
use crate::draw;
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc};
use crate::state::st;
use crate::win::{self, WM_BUTTON1DOWN, WM_CHAR, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;

const LS_MULTIPLESEL: u32 = 0x0001;
const LS_OWNERDRAW: u32 = 0x0002;
const LS_EXTENDEDSEL: u32 = 0x0010;

const LN_SELECT: u32 = 1;
const LN_SETFOCUS: u32 = 2;
const LN_KILLFOCUS: u32 = 3;
const LN_SCROLL: u32 = 4;
const LN_ENTER: u32 = 5;

const LM_QUERYITEMCOUNT: u32 = 0x0160;
const LM_INSERTITEM: u32 = 0x0161;
const LM_SETTOPINDEX: u32 = 0x0162;
const LM_DELETEITEM: u32 = 0x0163;
const LM_SELECTITEM: u32 = 0x0164;
const LM_QUERYSELECTION: u32 = 0x0165;
const LM_SETITEMTEXT: u32 = 0x0166;
const LM_QUERYITEMTEXTLENGTH: u32 = 0x0167;
const LM_QUERYITEMTEXT: u32 = 0x0168;
const LM_SETITEMHANDLE: u32 = 0x0169;
const LM_QUERYITEMHANDLE: u32 = 0x016a;
const LM_SEARCHSTRING: u32 = 0x016b;
const LM_SETITEMHEIGHT: u32 = 0x016c;
const LM_QUERYTOPINDEX: u32 = 0x016d;
const LM_DELETEALL: u32 = 0x016e;
const LM_INSERTMULTITEMS: u32 = 0x016f;

const LIT_NONE: u32 = 0xffff_ffff;
const LIT_END: i32 = -1;
const LIT_SORTASCENDING: i32 = -2;
const LIT_SORTDESCENDING: i32 = -3;

const LSS_SUBSTRING: u32 = 0x0001;
const LSS_PREFIX: u32 = 0x0002;
const LSS_CASESENSITIVE: u32 = 0x0004;

const SBM_SETSCROLLBAR: u32 = 0x01a0;
const SBM_SETTHUMBSIZE: u32 = 0x01a6;

const SCROLL_W: i32 = 16;

wndproc! {
    listbox_entry => listbox_proc;
}

struct ListItem {
    text: Vec<u8>,
    handle: u32,
    selected: bool,
}

struct List {
    items: Vec<ListItem>,
    top: usize,
    cursor: usize,
    item_h: i32,
    scroll: u32,
}

fn data(h: u32) -> Option<&'static mut List> {
    ctl::data::<List>(h)
}

fn multi(h: u32) -> bool {
    ctl::style(h) & (LS_MULTIPLESEL | LS_EXTENDEDSEL) != 0
}

/// Item area (inside the border, left of the scroll bar).
fn area(h: u32) -> Rect {
    let (cx, cy) = ctl::size(h);
    Rect::new(2, 2, cx - 2 - SCROLL_W, cy - 2)
}

fn rows(h: u32) -> usize {
    let ih = data(h).map_or(18, |l| l.item_h);
    (area(h).height() / ih).max(1) as usize
}

/// Update the scroll bar's range and slider.
fn sync_scroll(h: u32) {
    let rows = rows(h);
    let Some(l) = data(h) else { return };
    let n = l.items.len();
    let max_top = n.saturating_sub(rows);
    l.top = l.top.min(max_top);
    let (sb, top) = (l.scroll, l.top);
    send(sb, SBM_SETSCROLLBAR, top as u32, mp(0, max_top as u32));
    send(sb, SBM_SETTHUMBSIZE, mp(rows as u32, n as u32), 0);
}

fn place_scroll(h: u32) {
    let (cx, cy) = ctl::size(h);
    let sb = data(h).map_or(0, |l| l.scroll);
    if sb != 0 {
        st().desk.set_pos(sb, cx - 2 - SCROLL_W, 2, SCROLL_W, (cy - 4).max(0));
    }
    sync_scroll(h);
}

fn set_top(h: u32, top: usize) {
    if let Some(l) = data(h) {
        if l.top != top {
            l.top = top;
            ctl::invalidate(h);
        }
    }
    sync_scroll(h);
    ctl::notify(h, LN_SCROLL);
}

/// Scroll so the cursor item is visible.
fn reveal(h: u32) {
    let rows = rows(h);
    let Some(l) = data(h) else { return };
    let (cur, top) = (l.cursor, l.top);
    if cur < top {
        set_top(h, cur);
    } else if cur >= top + rows {
        set_top(h, cur + 1 - rows);
    }
}

fn select(h: u32, i: usize, on: bool, notify: bool) -> bool {
    let multi = multi(h);
    let Some(l) = data(h) else { return false };
    if i >= l.items.len() {
        return false;
    }
    if !multi && on {
        for it in l.items.iter_mut() {
            it.selected = false;
        }
    }
    l.items[i].selected = on;
    if on {
        l.cursor = i;
    }
    ctl::invalidate(h);
    if notify {
        ctl::notify(h, LN_SELECT);
    }
    true
}

fn insert(h: u32, at: i32, text: Vec<u8>) -> u32 {
    let Some(l) = data(h) else { return LIT_NONE };
    let n = l.items.len();
    let i = match at {
        LIT_SORTASCENDING => l.items.iter().position(|it| it.text.to_ascii_lowercase() > text.to_ascii_lowercase()).unwrap_or(n),
        LIT_SORTDESCENDING => l.items.iter().position(|it| it.text.to_ascii_lowercase() < text.to_ascii_lowercase()).unwrap_or(n),
        LIT_END => n,
        i if i >= 0 => (i as usize).min(n),
        _ => n,
    };
    l.items.insert(i, ListItem { text, handle: 0, selected: false });
    ctl::invalidate(h);
    sync_scroll(h);
    i as u32
}

fn search(h: u32, cmd: u32, start: i32, key: &[u8]) -> u32 {
    let Some(l) = data(h) else { return LIT_NONE };
    let n = l.items.len();
    let cs = cmd & LSS_CASESENSITIVE != 0;
    let norm = |t: &[u8]| if cs { t.to_vec() } else { t.to_ascii_lowercase() };
    let k = norm(key);
    let first = if start < 0 { 0 } else { start as usize + 1 };
    for j in 0..n {
        let i = (first + j) % n;
        let t = norm(&l.items[i].text);
        let hit = if cmd & LSS_SUBSTRING != 0 {
            k.is_empty() || t.windows(k.len()).any(|w| w == &k[..])
        } else if cmd & LSS_PREFIX != 0 {
            t.starts_with(&k)
        } else {
            t == k
        };
        if hit {
            return i as u32;
        }
    }
    LIT_NONE
}

/// The items on view: (index, text, selected, cursor, handle, rectangle).
fn visible(h: u32) -> Vec<(usize, Vec<u8>, bool, bool, u32, Rect)> {
    let a = area(h);
    let n = rows(h) + 1;
    let Some(l) = data(h) else { return Vec::new() };
    let ih = l.item_h;
    l.items
        .iter()
        .enumerate()
        .skip(l.top)
        .take(n)
        .enumerate()
        .map(|(k, (i, it))| {
            let top = a.top - k as i32 * ih;
            (i, it.text.clone(), it.selected, i == l.cursor, it.handle, Rect::new(a.left, top - ih, a.right, top).intersect(&a))
        })
        .filter(|v| !v.5.is_empty())
        .collect()
}

fn draw_text_item(c: &mut draw::Canvas, h: u32, ir: Rect, text: &[u8], sel: bool) {
    let f = draw::font_of(h);
    let col = crate::pp::text_colors(h);
    let fg = if sel {
        c.fill(ir, col.hilite_bg);
        col.hilite_fg
    } else {
        col.fg
    };
    let y = ir.bottom + (ir.height() - f.height) / 2 + f.descender;
    c.text(ir.left + 3, y, text, fg, ir);
}

/// Paint the list. With LS_OWNERDRAW each item on view goes to the owner
/// first (IBM's LboxDrawItems): WM_DRAWITEM with an OWNERITEM (fsState
/// and fsStateOld both the selection, idItem the index, hItem the item
/// handle) and a presentation space in RGB mode with the list's colours;
/// an item the owner does not draw is drawn as text, one it draws is
/// inverted if it leaves either state set.
fn paint(h: u32) {
    let focus = ctl::has_focus(h);
    let owner = ctl::style(h) & LS_OWNERDRAW != 0;
    let items = visible(h);
    let hps = win::WinBeginPaint(h, 0, core::ptr::null_mut());
    let col = crate::pp::text_colors(h);
    draw::with(h, hps, |c| {
        let r = c.rect();
        c.fill(r, col.bg);
        c.sunken(r);
        c.bevel(Rect::new(r.left + 1, r.bottom + 1, r.right - 1, r.top - 1), draw::DARK, draw::face());
        if !owner {
            for (_, text, sel, cur, _, ir) in &items {
                draw_text_item(c, h, *ir, text, *sel);
                if focus && *cur {
                    c.focus_rect(*ir);
                }
            }
        }
    });
    if owner {
        let clip = area(h);
        for (i, text, sel, cur, handle, ir) in &items {
            let mut oi = ctl::OwnerItem {
                hwnd: h,
                hps,
                fs_state: *sel as u32,
                fs_state_old: *sel as u32,
                rcl: crate::gpiapi::RectL { left: ir.left, bottom: ir.bottom, right: ir.right, top: ir.top },
                id_item: *i as i32,
                h_item: *handle,
                ..Default::default()
            };
            let drawn = ctl::draw_item(h, &mut oi, Some(clip), col.fg, col.bg);
            draw::with(h, hps, |c| {
                let saved = core::mem::replace(&mut c.ps.clip, alloc::vec::Vec::new());
                c.ps.clip = saved.iter().map(|x| x.intersect(&clip.offset(c.ps.origin.x, c.ps.origin.y))).collect();
                if !drawn {
                    draw_text_item(c, h, *ir, text, *sel);
                } else if oi.fs_state != 0 || oi.fs_state_old != 0 {
                    c.invert(*ir);
                }
                if focus && *cur {
                    c.focus_rect(*ir);
                }
                c.ps.clip = saved;
            });
        }
    }
    win::WinEndPaint(hps);
}

fn key(h: u32, mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.up() {
        return false;
    }
    let rows = rows(h);
    let Some(l) = data(h) else { return false };
    let n = l.items.len();
    if n == 0 {
        return false;
    }
    let cur = l.cursor.min(n - 1);
    let to = match k.vk {
        VK_UP => cur.saturating_sub(1),
        VK_DOWN => (cur + 1).min(n - 1),
        VK_PAGEUP => cur.saturating_sub(rows),
        VK_PAGEDOWN => (cur + rows).min(n - 1),
        VK_HOME => 0,
        VK_END => n - 1,
        VK_SPACE if multi(h) => {
            let on = !l.items[cur].selected;
            select(h, cur, on, true);
            return true;
        }
        VK_NEWLINE | VK_ENTER => {
            ctl::notify(h, LN_ENTER);
            return true;
        }
        _ => {
            // A letter selects the next item starting with it.
            let c = k.ch;
            if c.is_ascii_graphic() && !k.ctrl() && !k.alt() {
                let i = search(h, LSS_PREFIX, cur as i32, &[c]);
                if i != LIT_NONE {
                    select(h, i as usize, true, true);
                    reveal(h);
                }
                return true;
            }
            return false;
        }
    };
    if multi(h) {
        l.cursor = to;
        ctl::invalidate(h);
    } else {
        select(h, to, true, true);
    }
    reveal(h);
    true
}

pub extern "C" fn listbox_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            let f = draw::font_of(h);
            // LS_OWNERDRAW: the owner says how high the items are
            // (WM_MEASUREITEM, mp1 the id, mp2 0: the height in the low
            // word), as IBM's LboxCreate asks.
            let item_h = if ctl::style(h) & LS_OWNERDRAW != 0 {
                (send(ctl::owner(h), ctl::WM_MEASUREITEM, ctl::id(h), 0) & 0xffff).max(1) as i32
            } else {
                f.height + 2
            };
            ctl::set_data(h, List { items: Vec::new(), top: 0, cursor: 0, item_h, scroll: 0 });
            let sb = win::create(h, b"#8", b"", pmwin::ws::VISIBLE | 1, Rect::default(), h, 0, 0);
            if let Some(l) = data(h) {
                l.scroll = sb;
            }
            place_scroll(h);
            0
        }
        WM_SIZE => {
            place_scroll(h);
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
            let p = ctl::mouse_pos(mp1);
            let a = area(h);
            let Some(l) = data(h) else { return 1 };
            if !a.contains(p) {
                return 1;
            }
            let i = l.top + ((a.top - 1 - p.y) / l.item_h) as usize;
            if i >= l.items.len() {
                return 1;
            }
            if msg == WM_BUTTON1DBLCLK {
                select(h, i, true, true);
                ctl::notify(h, LN_ENTER);
            } else if multi(h) {
                let on = !l.items[i].selected;
                select(h, i, on, true);
            } else {
                select(h, i, true, true);
            }
            1
        }
        WM_VSCROLL => {
            let rows = rows(h);
            let top = data(h).map_or(0, |l| l.top);
            let pos = lo(mp2) as i16 as i32;
            let new = match hi(mp2) {
                1 => top.saturating_sub(1),
                2 => top + 1,
                3 => top.saturating_sub(rows),
                4 => top + rows,
                5 | 6 => pos.max(0) as usize,
                _ => top,
            };
            let n = data(h).map_or(0, |l| l.items.len());
            set_top(h, new.min(n.saturating_sub(rows)));
            0
        }
        WM_CHAR => {
            if key(h, mp1, mp2) {
                return 1;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_SETFOCUS => {
            ctl::invalidate(h);
            ctl::notify(h, if mp2 & 0xffff != 0 { LN_SETFOCUS } else { LN_KILLFOCUS });
            0
        }
        WM_QUERYDLGCODE => 0,
        LM_QUERYITEMCOUNT => data(h).map_or(0, |l| l.items.len() as u32),
        LM_INSERTITEM => insert(h, mp1 as i16 as i32, ctl::cstr(mp2 as *const u8)),
        LM_INSERTMULTITEMS => {
            if mp1 == 0 || mp2 == 0 {
                return 0;
            }
            // LBOXINFO: lItemIndex, ulItemCount, reserved, reserved2.
            let (at, n) = unsafe { ((mp1 as *const i32).read_unaligned(), ((mp1 + 4) as *const u32).read_unaligned()) };
            for i in 0..n {
                let p = unsafe { ((mp2 + i * 4) as *const u32).read_unaligned() } as *const u8;
                let pos = if at < 0 { at } else { at + i as i32 };
                insert(h, pos, ctl::cstr(p));
            }
            n
        }
        LM_SETTOPINDEX => {
            set_top(h, mp1 as usize);
            1
        }
        LM_QUERYTOPINDEX => data(h).map_or(LIT_NONE, |l| if l.items.is_empty() { LIT_NONE } else { l.top as u32 }),
        LM_DELETEITEM => {
            let Some(l) = data(h) else { return 0 };
            if (mp1 as usize) < l.items.len() {
                l.items.remove(mp1 as usize);
                if l.cursor >= l.items.len() {
                    l.cursor = l.items.len().saturating_sub(1);
                }
            }
            let n = l.items.len() as u32;
            ctl::invalidate(h);
            sync_scroll(h);
            n
        }
        LM_DELETEALL => {
            if let Some(l) = data(h) {
                l.items.clear();
                (l.top, l.cursor) = (0, 0);
            }
            ctl::invalidate(h);
            sync_scroll(h);
            1
        }
        LM_SELECTITEM => {
            let i = mp1 as i16 as i32;
            if i < 0 {
                if let Some(l) = data(h) {
                    for it in l.items.iter_mut() {
                        it.selected = false;
                    }
                }
                ctl::invalidate(h);
                return 1;
            }
            let ok = select(h, i as usize, mp2 != 0, true);
            reveal(h);
            ok as u32
        }
        LM_QUERYSELECTION => {
            let Some(l) = data(h) else { return LIT_NONE };
            let start = mp1 as i16 as i32;
            let from = if start < 0 { 0 } else { start as usize + 1 };
            l.items.iter().enumerate().skip(from).find(|(_, it)| it.selected).map_or(LIT_NONE, |(i, _)| i as u32)
        }
        LM_SETITEMTEXT => {
            let t = ctl::cstr(mp2 as *const u8);
            let Some(it) = data(h).and_then(|l| l.items.get_mut(mp1 as usize)) else { return 0 };
            it.text = t;
            ctl::invalidate(h);
            1
        }
        LM_QUERYITEMTEXTLENGTH => data(h).and_then(|l| l.items.get(mp1 as usize)).map_or(LIT_NONE, |it| it.text.len() as u32),
        LM_QUERYITEMTEXT => {
            let Some(it) = data(h).and_then(|l| l.items.get(lo(mp1) as usize)) else { return 0 };
            ctl::copy_out(&it.text, hi(mp1) as usize, mp2 as *mut u8)
        }
        LM_SETITEMHANDLE => {
            let Some(it) = data(h).and_then(|l| l.items.get_mut(mp1 as usize)) else { return 0 };
            it.handle = mp2;
            1
        }
        LM_QUERYITEMHANDLE => data(h).and_then(|l| l.items.get(mp1 as usize)).map_or(0, |it| it.handle),
        LM_SEARCHSTRING => search(h, lo(mp1), hi(mp1) as i16 as i32, &ctl::cstr(mp2 as *const u8)),
        LM_SETITEMHEIGHT => {
            if let Some(l) = data(h) {
                l.item_h = (mp1 as i32).max(4);
            }
            ctl::invalidate(h);
            sync_scroll(h);
            1
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
