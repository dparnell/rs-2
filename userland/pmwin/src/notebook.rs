//! Notebooks (WC_NOTEBOOK): pages with major and minor tabs, back pages,
//! a status line with page buttons, page windows the notebook keeps as
//! its children and sizes to the page (BKA_AUTOPAGESIZE); BKM_*
//! messages and BKN_* notifications. The page list and the geometry are
//! `pmwin::notebook`.
//!
//! Everything the paint code uses (tab and status texts, copies of tab
//! bitmaps' pixels, colours) lives in the shared state, so the PM server
//! can paint a notebook of a program that stopped responding.

use alloc::vec::Vec;

use gpi::{Point, Rect};
use pmwin::notebook::*;

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc};
use crate::state::st;
use crate::win::{self, WM_BUTTON1DOWN, WM_CHAR, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;

const BKM_CALCPAGERECT: u32 = 0x0353;
const BKM_DELETEPAGE: u32 = 0x0354;
const BKM_INSERTPAGE: u32 = 0x0355;
const BKM_INVALIDATETABS: u32 = 0x0356;
const BKM_TURNTOPAGE: u32 = 0x0357;
const BKM_QUERYPAGECOUNT: u32 = 0x0358;
const BKM_QUERYPAGEID: u32 = 0x0359;
const BKM_QUERYPAGEDATA: u32 = 0x035a;
const BKM_QUERYPAGEWINDOWHWND: u32 = 0x035b;
const BKM_QUERYTABBITMAP: u32 = 0x035c;
const BKM_QUERYTABTEXT: u32 = 0x035d;
const BKM_SETDIMENSIONS: u32 = 0x035e;
const BKM_SETPAGEDATA: u32 = 0x035f;
const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
const BKM_SETTABBITMAP: u32 = 0x0362;
const BKM_SETTABTEXT: u32 = 0x0363;
const BKM_SETNOTEBOOKCOLORS: u32 = 0x0364;
const BKM_QUERYPAGESTYLE: u32 = 0x0365;
const BKM_QUERYSTATUSLINETEXT: u32 = 0x0366;
const BKM_SETPAGEINFO: u32 = 0x0367;
const BKM_QUERYPAGEINFO: u32 = 0x0368;
const BKM_SETTABCOLOR: u32 = 0x0374;
const BKM_SETNOTEBOOKBUTTONS: u32 = 0x0375;

const BKN_PAGESELECTED: u32 = 130;
const BKN_NEWPAGESIZE: u32 = 131;
const BKN_HELP: u32 = 132;
const BKN_PAGEDELETED: u32 = 133;
const BKN_PAGESELECTEDPENDING: u32 = 134;

const BKA_MAJORTAB: u32 = 0x0001;
const BKA_MINORTAB: u32 = 0x0002;
const BKA_PAGEBUTTON: u32 = 0x0100;
const BKA_AUTOCOLOR: u32 = 0xffff_ffff;
/// BKS_BUTTONAREA (with BKS_TABBEDDIALOG).
const BKS_BUTTONAREA: u32 = 0x0200;

const BFA_PAGEDATA: u32 = 0x0001;
const BFA_PAGEFROMHWND: u32 = 0x0002;
const BFA_PAGEFROMDLGTEMPLATE: u32 = 0x0004;
const BFA_PAGEFROMDLGRES: u32 = 0x0008;
const BFA_STATUSLINE: u32 = 0x0010;
const BFA_MAJORTABBITMAP: u32 = 0x0020;
const BFA_MINORTABBITMAP: u32 = 0x0040;
const BFA_MAJORTABTEXT: u32 = 0x0080;
const BFA_MINORTABTEXT: u32 = 0x0100;

const BOOKERR_INVALID_PARAMETERS: u32 = 0xffff_ffff;

const VK_F1: u32 = 0x20;
const SWP_SIZE: u32 = 0x0001;
const SWP_MOVE: u32 = 0x0002;
const EDI_FIRSTTABITEM: u32 = 0;

/// Height of the area for notebook buttons (BKM_SETNOTEBOOKBUTTONS).
const BUTTON_AREA: i32 = 34;
/// BKA_AUTOCOLOR tab colours.
const AUTO_COLORS: [u32; 6] = [0xffffc0, 0xc0ffc0, 0xc0e0ff, 0xffd0d0, 0xc0ffff, 0xffd0ff];

wndproc! {
    notebook_entry => notebook_proc;
}

#[derive(Default)]
struct Nb {
    book: Book,
    major: (i32, i32),
    minor: (i32, i32),
    button: (i32, i32),
    first_major: usize,
    first_minor: usize,
    page_bg: Option<u32>,
    major_bg: Option<u32>,
    minor_bg: Option<u32>,
    major_fg: Option<u32>,
    minor_fg: Option<u32>,
    /// Notebook buttons (BKM_SETNOTEBOOKBUTTONS).
    buttons: Vec<u32>,
    /// The page rectangle the page windows were last sized to.
    page_rect: Option<Rect>,
}

fn data(h: u32) -> Option<&'static mut Nb> {
    ctl::data::<Nb>(h)
}

/// Whether `h` is a notebook.
pub fn is_notebook(h: u32) -> bool {
    data(h).is_some()
}

fn tab_text_width(t: &[u8]) -> i32 {
    draw::text_width(&draw::strip_mnemonic(t).0)
}

fn geometry_for(h: u32, w: i32, hh: i32) -> Option<Geometry> {
    let d = data(h)?;
    let f = draw::font();
    let widest = |minor: bool| {
        d.book
            .pages
            .iter()
            .filter(|p| if minor { p.minor() } else { p.major() })
            .map(|p| if p.bits.is_empty() { tab_text_width(&p.tab) } else { p.bits_size.0 })
            .max()
            .unwrap_or(0)
    };
    let style = ctl::style(h);
    let area = if !d.buttons.is_empty() || (style & BKS_TABBEDDIALOG != 0 && style & BKS_BUTTONAREA != 0) { BUTTON_AREA } else { 0 };
    Some(Geometry {
        w,
        h: hh,
        style,
        major: d.major,
        minor: d.minor,
        button: d.button,
        char_w: f.width,
        char_h: f.height,
        has_minor: d.book.has_minor(),
        major_text: widest(false),
        minor_text: widest(true),
        button_area: area,
    })
}

fn layout(h: u32) -> Option<Layout> {
    let (w, hh) = ctl::size(h);
    Some(geometry_for(h, w, hh)?.layout())
}

/// The layout and the tabs shown: (page index, rectangle) of the major
/// and the minor tabs.
fn shown(h: u32) -> Option<(Layout, Vec<(usize, Rect)>, Vec<(usize, Rect)>)> {
    let l = layout(h)?;
    let d = data(h)?;
    let len = |p: &Page| if p.bits.is_empty() { tab_text_width(&p.tab) + 20 } else { p.bits_size.0 + 12 }.max(40);
    let cur = d.book.current_tab();
    let top = d.book.top_index();
    let run = |minor: bool, first: &mut usize| -> Vec<(usize, Rect)> {
        let list = if minor { d.book.minor_tabs() } else { d.book.major_tabs() };
        let lens: Vec<i32> = list.iter().map(|&i| len(&d.book.pages[i])).collect();
        let sel_page = if minor { cur.filter(|&c| d.book.pages[c].minor()) } else { top.and_then(|t| d.book.major_of(t)) };
        let sel = sel_page.and_then(|s| list.iter().position(|&i| i == s));
        l.tabs(minor, &lens, sel, first).into_iter().map(|(k, r)| (list[k], r)).collect()
    };
    let mut fm = d.first_major;
    let mut fmi = d.first_minor;
    let major = run(false, &mut fm);
    let minor = run(true, &mut fmi);
    d.first_major = fm;
    d.first_minor = fmi;
    Some((l, major, minor))
}

// ---- Page windows ----

/// Hide every page window but the top page's, which is shown (and sized
/// to the page with BKA_AUTOPAGESIZE).
fn place_pages(h: u32) {
    let Some(l) = layout(h) else { return };
    let Some(d) = data(h) else { return };
    let top = d.book.top;
    let pages: Vec<(u32, u32, u16)> = d.book.pages.iter().map(|p| (p.id, p.hwnd, p.style)).collect();
    for &(id, w, _) in &pages {
        if w != 0 && id != top && st().desk.exists(w) && ctl::parent(w) == h {
            win::show(w, false);
        }
    }
    if let Some(&(_, w, style)) = pages.iter().find(|p| p.0 == top) {
        if w != 0 && st().desk.exists(w) && ctl::parent(w) == h {
            if style & BKA_AUTOPAGESIZE != 0 {
                let r = l.page;
                win::WinSetWindowPos(w, 0, r.left, r.bottom, r.width(), r.height(), SWP_MOVE | SWP_SIZE);
            }
            if st().desk.exists(w) {
                win::show(w, true);
            }
        }
    }
}

/// After a change of size or layout: page windows follow, the owner hears
/// of a new page size.
fn relayout(h: u32) {
    let Some(l) = layout(h) else { return };
    layout_buttons(h, &l);
    let Some(d) = data(h) else { return };
    let old = d.page_rect.replace(l.page);
    ctl::invalidate(h);
    if old != Some(l.page) {
        place_pages(h);
        if old.is_some() {
            send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), BKN_NEWPAGESIZE), h);
        }
    }
}

fn set_page_window(h: u32, id: u32, w: u32) -> bool {
    let Some(d) = data(h) else { return false };
    let Some(p) = d.book.page_mut(id) else { return false };
    let old = core::mem::replace(&mut p.hwnd, w);
    if old != 0 && old != w && st().desk.exists(old) && ctl::parent(old) == h {
        win::show(old, false);
    }
    if w != 0 && st().desk.exists(w) {
        if ctl::parent(w) != h {
            win::WinSetParent(w, h, 0);
        }
        place_pages(h);
    }
    true
}

/// Load a page's dialog (BKM_SETPAGEINFO with a resource or template).
fn load_dialog(h: u32, id: u32) {
    let Some(d) = data(h) else { return };
    let Some(p) = d.book.page_mut(id) else { return };
    let Some(pd) = p.dlg.take() else { return };
    let w = if pd.template != 0 {
        crate::dlg::WinCreateDlg(h, h, pd.proc_, pd.template as *const u8, pd.params)
    } else {
        crate::dlg::WinLoadDlg(h, h, pd.proc_, pd.hmod, pd.id, pd.params)
    };
    if w != 0 {
        set_page_window(h, id, w);
    }
}

// ---- Turning pages ----

/// PAGESELECTNOTIFY: hwndBook, ulPageIdCur, ulPageIdNew.
#[repr(C)]
struct PageSelect {
    book: u32,
    cur: u32,
    new: u32,
}

/// Bring page `id` to the top: BKN_PAGESELECTEDPENDING (the owner may
/// refuse by clearing ulPageIdNew), then BKN_PAGESELECTED.
fn turn(h: u32, id: u32) -> bool {
    let Some(d) = data(h) else { return false };
    let cur = d.book.top;
    if d.book.index(id).is_none() {
        return false;
    }
    if id == cur {
        return true;
    }
    let mut ps = PageSelect { book: h, cur, new: id };
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), BKN_PAGESELECTEDPENDING), &mut ps as *mut PageSelect as u32);
    if ps.new == 0 {
        return false;
    }
    if data(h).is_none_or(|d| d.book.index(id).is_none()) {
        return false;
    }
    if data(h).and_then(|d| d.book.page(id)).is_some_and(|p| p.dlg.is_some() && p.hwnd == 0) {
        load_dialog(h, id);
    }
    show_top(h, id);
    let mut ps = PageSelect { book: h, cur, new: id };
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), BKN_PAGESELECTED), &mut ps as *mut PageSelect as u32);
    true
}

/// Make `id` the top page and show its window (no notifications).
fn show_top(h: u32, id: u32) {
    let Some(d) = data(h) else { return };
    let old_w = d.book.page(d.book.top).map_or(0, |p| p.hwnd);
    d.book.top = id;
    let f = st().focus;
    // The focus was in the page going away: to the notebook.
    if old_w != 0 && f != 0 && st().desk.is_ancestor(old_w, f) {
        win::set_focus(h);
    }
    place_pages(h);
    ctl::invalidate(h);
}

fn delete(h: u32, id: u32, flag: u16) -> bool {
    let Some(d) = data(h) else { return false };
    let top = d.book.top;
    let Some(gone) = d.book.delete(id, flag) else { return false };
    let new_top = d.book.top;
    for p in &gone {
        if p.hwnd != 0 && st().desk.exists(p.hwnd) && ctl::parent(p.hwnd) == h {
            win::show(p.hwnd, false);
        }
    }
    if new_top != top {
        if let Some(d) = data(h) {
            // show_top compares with the old top's window.
            d.book.top = 0;
        }
        show_top(h, new_top);
    }
    relayout(h);
    ctl::invalidate(h);
    for p in &gone {
        // DELETENOTIFY: hwndBook, hwndPage, ulAppPageData, hbmTab.
        let dn: [u32; 4] = [h, p.hwnd, p.data, p.bitmap];
        send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), BKN_PAGEDELETED), dn.as_ptr() as u32);
        if !st().desk.exists(h) {
            return true;
        }
    }
    if new_top != top && new_top != 0 && data(h).is_some_and(|d| d.book.top == new_top) {
        if data(h).and_then(|d| d.book.page(new_top)).is_some_and(|p| p.dlg.is_some() && p.hwnd == 0) {
            load_dialog(h, new_top);
        }
        let mut ps = PageSelect { book: h, cur: top, new: new_top };
        send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), BKN_PAGESELECTED), &mut ps as *mut PageSelect as u32);
    }
    true
}

// ---- Notebook buttons ----

fn layout_buttons(h: u32, l: &Layout) {
    let Some(d) = data(h) else { return };
    let mut x = 8;
    for &b in &d.buttons.clone() {
        if !st().desk.exists(b) {
            continue;
        }
        let w = (tab_text_width(&ctl::text(b)) + 24).max(64);
        let y = l.button_area.bottom + (l.button_area.height() - 26) / 2;
        st().desk.set_pos(b, x, y, w, 26);
        x += w + 8;
    }
}

fn set_buttons(h: u32, n: u32, p: u32) -> bool {
    let Some(d) = data(h) else { return false };
    let old = core::mem::take(&mut d.buttons);
    for b in old {
        if st().desk.exists(b) {
            win::destroy(b);
        }
    }
    let mut made = Vec::new();
    for i in 0..n.min(32) {
        // NOTEBOOKBUTTON: pszText, idButton, hImage, flStyle.
        let at = p + i * 16;
        let (text, id, style) = (rd(at, 0), rd(at, 4).min(7999), rd(at, 12));
        let t = ctl::cstr(text as *const u8);
        let b = win::create(h, b"#3", &t, style | pmwin::ws::VISIBLE | WS_TABSTOP, Rect::new(0, 0, 64, 26), h, id, 0);
        if b != 0 {
            made.push(b);
        }
    }
    if let Some(d) = data(h) {
        d.buttons = made;
    }
    relayout(h);
    true
}

// ---- Painting ----

struct TabPaint {
    r: Rect,
    text: Vec<u8>,
    bits: Option<crate::bitmap::Bmp>,
    bg: u32,
    fg: u32,
    selected: bool,
    /// Shows the focus (the current tab: minor if shown, else major).
    focused: bool,
    minor: bool,
}

fn arrow(c: &mut Canvas, r: Rect, left: bool, enabled: bool) {
    c.fill(r, draw::face());
    c.raised(r);
    let col = if enabled { draw::text() } else { draw::shadow() };
    let (cx, cy) = (r.left + r.width() / 2, r.bottom + r.height() / 2);
    for i in 0..5 {
        let x = if left { cx - 2 + i } else { cx + 2 - i };
        c.fill(Rect::new(x, cy - i, x + 1, cy + i + 1), col);
    }
}

/// Cut the outer corners of a tab (rounded and polygon tabs).
fn cut_corners(c: &mut Canvas, r: Rect, side: Side, n: i32, bg: u32) {
    for i in 0..n {
        let k = n - i;
        let (a, b) = match side {
            Side::Right => (Rect::new(r.right - k, r.top - 1 - i, r.right, r.top - i), Rect::new(r.right - k, r.bottom + i, r.right, r.bottom + i + 1)),
            Side::Left => (Rect::new(r.left, r.top - 1 - i, r.left + k, r.top - i), Rect::new(r.left, r.bottom + i, r.left + k, r.bottom + i + 1)),
            Side::Top => (Rect::new(r.left + i, r.top - k, r.left + i + 1, r.top), Rect::new(r.right - 1 - i, r.top - k, r.right - i, r.top)),
            Side::Bottom => (Rect::new(r.left + i, r.bottom, r.left + i + 1, r.bottom + k), Rect::new(r.right - 1 - i, r.bottom, r.right - i, r.bottom + k)),
        };
        c.fill(a, bg);
        c.fill(b, bg);
    }
}

fn paint(h: u32) {
    let style = ctl::style(h);
    let focus = ctl::has_focus(h);
    let Some((l, major, minor)) = shown(h) else { return };
    let Some(d) = data(h) else { return };
    let bg = draw::dialog_bg();
    let page_bg = d.page_bg.unwrap_or(bg);
    let top = d.book.top_index();
    let cur = d.book.current_tab();
    let sel_major = top.and_then(|t| d.book.major_of(t));
    let mut tabs = Vec::new();
    for (minor_tab, list) in [(false, &major), (true, &minor)] {
        for &(i, r) in list.iter() {
            let p = &d.book.pages[i];
            let selected = if minor_tab { cur == Some(i) } else { sel_major == Some(i) };
            let base = if minor_tab { d.minor_bg } else { d.major_bg };
            let tab_bg = p.tab_color.or(base).unwrap_or(if selected { page_bg } else { 0xb8b8b8 });
            let fg = if minor_tab { d.minor_fg } else { d.major_fg }.unwrap_or(draw::text());
            let bits = (!p.bits.is_empty()).then(|| crate::bitmap::Bmp { w: p.bits_size.0, h: p.bits_size.1, pixels: p.bits.clone() });
            let focused = if minor_tab { selected } else { selected && !minor.iter().any(|&(j, _)| Some(j) == cur) };
            tabs.push(TabPaint { r: if selected { l.selected(minor_tab, r) } else { r }, text: p.tab.clone(), bits, bg: tab_bg, fg, selected, focused, minor: minor_tab });
        }
    }
    // Unselected tabs first: the selected ones overlap the back pages.
    tabs.sort_by_key(|t| t.selected);
    let tp = top.map(|t| &d.book.pages[t]);
    let status = tp.filter(|p| p.style & BKA_STATUSTEXTON != 0).map_or(Vec::new(), |p| p.status.clone());
    let (has_prev, has_next) = (d.book.step(-1).is_some(), d.book.step(1).is_some());
    let enabled = ctl::enabled(h);
    draw::paint(h, |c: &mut Canvas| {
        let all = c.rect();
        c.fill(all, bg);
        for r in &l.backs {
            c.fill(*r, page_bg);
            c.frame(*r, draw::shadow());
        }
        // The top page.
        c.fill(l.book, page_bg);
        c.frame(l.book, draw::DARK);
        c.raised(Rect::new(l.book.left + 1, l.book.bottom + 1, l.book.right - 1, l.book.top - 1));
        if style & BKS_SPIRALBIND != 0 && !l.tabbed {
            // Rings along the side opposite the major tabs.
            let (vert, at) = match l.major_side {
                Side::Right => (true, l.book.left),
                Side::Left => (true, l.book.right),
                Side::Top => (false, l.book.bottom),
                Side::Bottom => (false, l.book.top),
            };
            if vert {
                let mut y = l.book.bottom + 8;
                while y + 4 < l.book.top {
                    c.fill(Rect::new(at - 2, y, at + 6, y + 3), draw::DARK);
                    y += 12;
                }
            } else {
                let mut x = l.book.left + 8;
                while x + 4 < l.book.right {
                    c.fill(Rect::new(x, at - 6, x + 3, at + 2), draw::DARK);
                    x += 12;
                }
            }
        }
        // Tabs.
        for t in &tabs {
            let side = if t.minor { l.minor_side } else { l.major_side };
            c.fill(t.r, t.bg);
            // Outline on the three sides away from the book.
            let r = t.r;
            match side {
                Side::Right => {
                    c.hline(r.left, r.right, r.top - 1, draw::DARK);
                    c.hline(r.left, r.right, r.bottom, draw::DARK);
                    c.vline(r.right - 1, r.bottom, r.top, draw::DARK);
                    if !t.selected {
                        c.vline(r.left, r.bottom, r.top, draw::DARK);
                    }
                }
                Side::Left => {
                    c.hline(r.left, r.right, r.top - 1, draw::DARK);
                    c.hline(r.left, r.right, r.bottom, draw::DARK);
                    c.vline(r.left, r.bottom, r.top, draw::DARK);
                    if !t.selected {
                        c.vline(r.right - 1, r.bottom, r.top, draw::DARK);
                    }
                }
                Side::Top => {
                    c.vline(r.left, r.bottom, r.top, draw::DARK);
                    c.vline(r.right - 1, r.bottom, r.top, draw::DARK);
                    c.hline(r.left, r.right, r.top - 1, draw::DARK);
                    if !t.selected {
                        c.hline(r.left, r.right, r.bottom, draw::DARK);
                    }
                }
                Side::Bottom => {
                    c.vline(r.left, r.bottom, r.top, draw::DARK);
                    c.vline(r.right - 1, r.bottom, r.top, draw::DARK);
                    c.hline(r.left, r.right, r.bottom, draw::DARK);
                    if !t.selected {
                        c.hline(r.left, r.right, r.top - 1, draw::DARK);
                    }
                }
            }
            if style & BKS_ROUNDEDTABS != 0 {
                cut_corners(c, r, side, 3, bg);
            } else if style & BKS_POLYGONTABS != 0 && !(l.tabbed && style & BKS_BUTTONAREA != 0) {
                cut_corners(c, r, side, 6, bg);
            }
            // The text (or bitmap) within the part outside the book.
            let inner = match side {
                Side::Right => Rect::new(r.right - (if t.minor { l.minor_tab.0 } else { l.major_tab.0 }) + 4, r.bottom + 2, r.right - 4, r.top - 2),
                Side::Left => Rect::new(r.left + 4, r.bottom + 2, r.left + (if t.minor { l.minor_tab.0 } else { l.major_tab.0 }) - 4, r.top - 2),
                Side::Top | Side::Bottom => Rect::new(r.left + 6, r.bottom + 2, r.right - 6, r.top - 2),
            };
            let inner = if matches!(side, Side::Top | Side::Bottom) && t.selected {
                let a = if t.minor { l.minor_tab.0 } else { l.major_tab.0 };
                if side == Side::Top { Rect::new(inner.left, r.top - a + 2, inner.right, inner.top) } else { Rect::new(inner.left, inner.bottom, inner.right, r.bottom + a - 2) }
            } else {
                inner
            };
            if let Some(b) = &t.bits {
                let o = c.ps.origin;
                let (w, hh) = (b.w.min(inner.width()), b.h.min(inner.height()));
                let (x, y) = (inner.left + (inner.width() - w) / 2, inner.bottom + (inner.height() - hh) / 2);
                c.ps.origin = Point::new(o.x + x, o.y + y);
                crate::bitmap::blit(c, b, w, hh);
                c.ps.origin = o;
            } else {
                let align = if style & BKS_TABTEXTCENTER != 0 {
                    draw::DT_CENTER
                } else if style & BKS_TABTEXTRIGHT != 0 {
                    draw::DT_RIGHT
                } else {
                    0
                };
                let fg = if enabled { t.fg } else { draw::GRAY_TEXT };
                c.text_in(inner, &t.text, fg, align | draw::DT_VCENTER | draw::DT_MNEMONIC);
            }
            if focus && t.focused {
                c.focus_rect(Rect::new(inner.left - 2, inner.bottom, inner.right + 2, inner.top));
            }
        }
        // The status line and the page buttons.
        let s = l.status;
        c.hline(s.left + 2, s.right - 2, s.top, draw::shadow());
        let tr = Rect::new(s.left + 4, s.bottom, l.prev.left - 4, s.top);
        let align = if style & BKS_STATUSTEXTCENTER != 0 {
            draw::DT_CENTER
        } else if style & BKS_STATUSTEXTRIGHT != 0 {
            draw::DT_RIGHT
        } else {
            0
        };
        if !status.is_empty() {
            c.text_in(tr, &status, if enabled { draw::text() } else { draw::GRAY_TEXT }, align | draw::DT_VCENTER);
        }
        arrow(c, l.prev, true, enabled && has_prev);
        arrow(c, l.next, false, enabled && has_next);
    });
}

// ---- Input ----

fn into_page(h: u32) -> bool {
    let w = data(h).and_then(|d| d.book.page(d.book.top)).map_or(0, |p| p.hwnd);
    if w == 0 || !st().desk.get(w).is_some_and(|x| x.visible_style()) {
        return false;
    }
    let first = crate::dlg::WinEnumDlgItem(w, 0, EDI_FIRSTTABITEM);
    if first != 0 {
        win::WinSetFocus(1, first);
        return true;
    }
    if ctl::enabled(w) && send(w, WM_QUERYDLGCODE, 0, 0) & DLGC_STATIC == 0 {
        win::WinSetFocus(1, w);
        return true;
    }
    false
}

fn key(h: u32, mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.up() {
        return false;
    }
    let mine = ctl::has_focus(h);
    let Some(d) = data(h) else { return false };
    let target = match k.vk {
        VK_PAGEUP | VK_PAGEDOWN if mine || k.alt() => {
            let dir = if k.vk == VK_PAGEUP { -1 } else { 1 };
            let t = d.book.step(dir);
            if t.is_none() {
                return true;
            }
            t
        }
        VK_LEFT | VK_UP | VK_RIGHT | VK_DOWN if mine && !k.alt() => {
            let t = d.book.tab_step(if matches!(k.vk, VK_LEFT | VK_UP) { -1 } else { 1 });
            if t.is_none() {
                return true;
            }
            t
        }
        VK_HOME if mine => d.book.pages.first().map(|p| p.id),
        VK_END if mine => d.book.pages.last().map(|p| p.id),
        VK_TAB if mine && !k.shift() && !k.alt() && !k.ctrl() => return into_page(h),
        VK_TAB | VK_BACKTAB if !mine && (k.vk == VK_BACKTAB || k.shift()) => {
            // Shift+Tab out of the page: back to the tabs.
            win::WinSetFocus(1, h);
            return true;
        }
        VK_F1 if mine => {
            let top = d.book.top;
            send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), BKN_HELP), top);
            return true;
        }
        _ => {
            let c = k.letter(mp1);
            if c == 0 || k.ctrl() || (!mine && !k.alt()) {
                return false;
            }
            match d.book.mnemonic(c) {
                Some(id) => Some(id),
                None => return false,
            }
        }
    };
    if let Some(id) = target {
        turn(h, id);
    }
    true
}

/// A key a page dialog (a dialog that is a notebook page) did not use:
/// the notebook gets it (Alt+PgUp/PgDn, tab mnemonics), and passes on
/// what it does not use. Called from WinDefDlgProc.
pub fn page_key(dlg: u32, mp1: u32, mp2: u32) -> bool {
    let p = ctl::parent(dlg);
    if !is_notebook(p) {
        return false;
    }
    send(p, WM_CHAR, mp1, mp2) != 0
}

fn click(h: u32, p: Point) {
    let Some((l, major, minor)) = shown(h) else { return };
    let Some(d) = data(h) else { return };
    let hit = l.hit(p, &major, &minor);
    let id = match hit {
        Hit::MajorTab(i) | Hit::MinorTab(i) => d.book.pages.get(i).map(|p| p.id),
        Hit::Prev => d.book.step(-1),
        Hit::Next => d.book.step(1),
        Hit::Page | Hit::Nothing => return,
    };
    if !ctl::has_focus(h) && matches!(hit, Hit::MajorTab(_) | Hit::MinorTab(_)) {
        win::WinSetFocus(1, h);
    }
    if let Some(id) = id {
        turn(h, id);
    }
}

// ---- Messages ----

fn page_text(p: &Page, status: bool) -> &Vec<u8> {
    if status { &p.status } else { &p.tab }
}

/// BKM_QUERYTABTEXT / BKM_QUERYSTATUSLINETEXT: BOOKTEXT {pString,
/// textLen}.
fn query_text(h: u32, id: u32, bt: u32, status: bool) -> u32 {
    let Some(d) = data(h) else { return 0 };
    let id = if id == 0 { d.book.top } else { id };
    let Some(p) = d.book.page(id) else { return BOOKERR_INVALID_PARAMETERS };
    let t = page_text(p, status);
    if bt == 0 {
        return t.len() as u32;
    }
    let (buf, len) = unsafe { ((bt as *const u32).read_unaligned(), ((bt + 4) as *const u32).read_unaligned()) };
    if buf == 0 || len == 0 {
        return t.len() as u32;
    }
    ctl::copy_out(t, len as usize, buf as *mut u8)
}

/// A copy of a bitmap's pixels (rows bottom up) for painting.
fn bitmap_pixels(hbm: u32) -> Option<((i32, i32), Vec<u32>)> {
    crate::gpiobj::with_bitmap(hbm, |b| {
        let (w, hh) = (b.width.min(256), b.height.min(256));
        let mut v = Vec::with_capacity((w * hh) as usize);
        for y in 0..hh {
            // Bitmap memory is top row first.
            let row = (b.height - 1 - y) * b.width;
            for x in 0..w {
                v.push(b.pixels[(row + x) as usize] & 0xff_ffff);
            }
        }
        ((w, hh), v)
    })
}

fn set_bitmap(h: u32, id: u32, hbm: u32) -> bool {
    let pix = if hbm != 0 { bitmap_pixels(hbm) } else { None };
    let Some(d) = data(h) else { return false };
    let Some(p) = d.book.page_mut(id) else { return false };
    p.bitmap = hbm;
    match pix {
        Some((s, v)) => {
            p.bits_size = s;
            p.bits = v;
        }
        None => {
            p.bits_size = (0, 0);
            p.bits = Vec::new();
        }
    }
    relayout(h);
    true
}

fn read_text(cb: u32, p: u32) -> Vec<u8> {
    let mut t = ctl::cstr(p as *const u8);
    if cb > 0 && (cb as usize) < t.len() {
        t.truncate(cb as usize);
    }
    t
}

/// BOOKPAGEINFO field offsets.
mod bpi {
    pub const FL: u32 = 4;
    pub const LOAD: u32 = 8;
    pub const DATA: u32 = 12;
    pub const HWND: u32 = 16;
    pub const PROC: u32 = 20;
    pub const ID: u32 = 24;
    pub const HMOD: u32 = 28;
    pub const PARAMS: u32 = 32;
    pub const TEMPLATE: u32 = 36;
    pub const CB_STATUS: u32 = 40;
    pub const STATUS: u32 = 44;
    pub const HBM_MAJOR: u32 = 48;
    pub const HBM_MINOR: u32 = 52;
    pub const CB_MAJOR: u32 = 56;
    pub const MAJOR: u32 = 60;
    pub const CB_MINOR: u32 = 64;
    pub const MINOR: u32 = 68;
}

fn rd(base: u32, off: u32) -> u32 {
    unsafe { ((base + off) as *const u32).read_unaligned() }
}

fn wr(base: u32, off: u32, v: u32) {
    unsafe { ((base + off) as *mut u32).write_unaligned(v) }
}

fn set_page_info(h: u32, id: u32, pi: u32) -> bool {
    if pi == 0 || data(h).is_none_or(|d| d.book.index(id).is_none()) {
        return false;
    }
    let fl = rd(pi, bpi::FL);
    let page = || data(h).and_then(|d| d.book.page_mut(id));
    if fl & BFA_PAGEDATA != 0 {
        if let Some(p) = page() {
            p.data = rd(pi, bpi::DATA);
        }
    }
    if fl & BFA_STATUSLINE != 0 {
        let t = read_text(rd(pi, bpi::CB_STATUS), rd(pi, bpi::STATUS));
        if let Some(p) = page() {
            p.status = t;
        }
    }
    for (flag, cb, s) in [(BFA_MAJORTABTEXT, bpi::CB_MAJOR, bpi::MAJOR), (BFA_MINORTABTEXT, bpi::CB_MINOR, bpi::MINOR)] {
        if fl & flag != 0 {
            let t = read_text(rd(pi, cb), rd(pi, s));
            if let Some(p) = page() {
                p.tab = t;
            }
        }
    }
    for (flag, off) in [(BFA_MAJORTABBITMAP, bpi::HBM_MAJOR), (BFA_MINORTABBITMAP, bpi::HBM_MINOR)] {
        if fl & flag != 0 {
            set_bitmap(h, id, rd(pi, off));
        }
    }
    if fl & (BFA_PAGEFROMDLGTEMPLATE | BFA_PAGEFROMDLGRES) != 0 {
        let pd = PageDlg {
            proc_: rd(pi, bpi::PROC),
            id: rd(pi, bpi::ID),
            hmod: rd(pi, bpi::HMOD),
            params: rd(pi, bpi::PARAMS),
            template: if fl & BFA_PAGEFROMDLGTEMPLATE != 0 { rd(pi, bpi::TEMPLATE) } else { 0 },
        };
        if let Some(p) = page() {
            p.dlg = Some(pd);
        }
        let now = rd(pi, bpi::LOAD) != 0 || data(h).is_some_and(|d| d.book.top == id);
        if now {
            load_dialog(h, id);
        }
    } else if fl & BFA_PAGEFROMHWND != 0 {
        set_page_window(h, id, rd(pi, bpi::HWND));
    }
    relayout(h);
    true
}

fn query_page_info(h: u32, id: u32, pi: u32) -> bool {
    let Some(d) = data(h) else { return false };
    let Some(p) = d.book.page(id) else { return false };
    if pi == 0 {
        return false;
    }
    let fl = rd(pi, bpi::FL);
    if fl & BFA_PAGEDATA != 0 {
        wr(pi, bpi::DATA, p.data);
    }
    if fl & BFA_PAGEFROMHWND != 0 {
        wr(pi, bpi::HWND, p.hwnd);
    }
    if fl & (BFA_PAGEFROMDLGTEMPLATE | BFA_PAGEFROMDLGRES) != 0 {
        if let Some(pd) = p.dlg {
            wr(pi, bpi::PROC, pd.proc_);
            wr(pi, bpi::ID, pd.id);
            wr(pi, bpi::HMOD, pd.hmod);
            wr(pi, bpi::PARAMS, pd.params);
            wr(pi, bpi::TEMPLATE, pd.template);
        }
    }
    let put = |cb_off: u32, s_off: u32, t: &[u8]| {
        let (cb, s) = (rd(pi, cb_off), rd(pi, s_off));
        if s != 0 && cb > 0 {
            ctl::copy_out(t, cb as usize, s as *mut u8);
        }
        wr(pi, cb_off, t.len() as u32 + 1);
    };
    if fl & BFA_STATUSLINE != 0 {
        put(bpi::CB_STATUS, bpi::STATUS, &p.status);
    }
    if fl & BFA_MAJORTABTEXT != 0 {
        put(bpi::CB_MAJOR, bpi::MAJOR, if p.major() { &p.tab } else { &[] });
    }
    if fl & BFA_MINORTABTEXT != 0 {
        put(bpi::CB_MINOR, bpi::MINOR, if p.minor() { &p.tab } else { &[] });
    }
    if fl & BFA_MAJORTABBITMAP != 0 {
        wr(pi, bpi::HBM_MAJOR, if p.major() { p.bitmap } else { 0 });
    }
    if fl & BFA_MINORTABBITMAP != 0 {
        wr(pi, bpi::HBM_MINOR, if p.minor() { p.bitmap } else { 0 });
    }
    true
}

fn calc_rect(h: u32, prcl: u32, page: bool) -> bool {
    if prcl == 0 {
        return false;
    }
    let p = prcl as *mut i32;
    let (l, b, r, t) = unsafe { (p.read_unaligned(), p.add(1).read_unaligned(), p.add(2).read_unaligned(), p.add(3).read_unaligned()) };
    let out = if page {
        let Some(g) = geometry_for(h, r - l, t - b) else { return false };
        g.layout().page.offset(l, b)
    } else {
        let Some(lay) = layout(h) else { return false };
        let (w, hh) = ctl::size(h);
        let (ml, mb, mr, mt) = lay.margins(w, hh);
        Rect::new(l - ml, b - mb, r + mr, t + mt)
    };
    unsafe {
        p.write_unaligned(out.left);
        p.add(1).write_unaligned(out.bottom);
        p.add(2).write_unaligned(out.right);
        p.add(3).write_unaligned(out.top);
    }
    true
}

fn color(v: u32, index: bool) -> u32 {
    if index { crate::color::resolve(v as i32) } else { v & 0xff_ffff }
}

pub extern "C" fn notebook_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            ctl::set_data(h, Nb::default());
            if let Some(l) = layout(h) {
                if let Some(d) = data(h) {
                    d.page_rect = Some(l.page);
                }
            }
            0
        }
        WM_SIZE => {
            relayout(h);
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON1DBLCLK => {
            if ctl::enabled(h) {
                click(h, ctl::mouse_pos(mp1));
            }
            1
        }
        WM_CHAR => {
            if key(h, mp1, mp2) {
                return 1;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_SETFOCUS | WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        WM_QUERYDLGCODE => 0,
        WM_MATCHMNEMONIC => {
            let c = (mp1 as u8).to_ascii_lowercase();
            match data(h).and_then(|d| d.book.mnemonic(c)) {
                Some(id) => {
                    turn(h, id);
                    1
                }
                None => 0,
            }
        }
        WM_HELP => {
            let top = data(h).map_or(0, |d| d.book.top);
            send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), BKN_HELP), top);
            0
        }
        WM_COMMAND | WM_CONTROL => {
            // From the notebook buttons: to the top page's window.
            let w = data(h).and_then(|d| d.book.page(d.book.top)).map_or(0, |p| p.hwnd);
            let to = if w != 0 && st().desk.exists(w) { w } else { ctl::owner(h) };
            send(to, msg, mp1, mp2)
        }
        BKM_INSERTPAGE => {
            let Some(d) = data(h) else { return 0 };
            let was_empty = d.book.pages.is_empty();
            let had_minor = d.book.has_minor();
            let Some(id) = d.book.insert(mp1, lo(mp2) as u16, hi(mp2) as u16) else { return 0 };
            if was_empty || had_minor != d.book.has_minor() || lo(mp2) as u16 & (BKA_MAJOR | BKA_MINOR) != 0 {
                relayout(h);
            }
            ctl::invalidate(h);
            id
        }
        BKM_DELETEPAGE => delete(h, mp1, lo(mp2) as u16) as u32,
        BKM_TURNTOPAGE => turn(h, mp1) as u32,
        BKM_QUERYPAGECOUNT => data(h).map_or(0, |d| d.book.count(mp1, lo(mp2) as u16).unwrap_or(BOOKERR_INVALID_PARAMETERS)),
        BKM_QUERYPAGEID => data(h).map_or(0, |d| d.book.query_id(mp1, lo(mp2) as u16, hi(mp2) as u16).unwrap_or(BOOKERR_INVALID_PARAMETERS)),
        BKM_QUERYPAGEDATA => data(h).and_then(|d| d.book.page(mp1)).map_or(BOOKERR_INVALID_PARAMETERS, |p| p.data),
        BKM_SETPAGEDATA => match data(h).and_then(|d| d.book.page_mut(mp1)) {
            Some(p) => {
                p.data = mp2;
                1
            }
            None => 0,
        },
        BKM_QUERYPAGEWINDOWHWND => data(h).and_then(|d| d.book.page(mp1)).map_or(BOOKERR_INVALID_PARAMETERS, |p| p.hwnd),
        BKM_SETPAGEWINDOWHWND => set_page_window(h, mp1, mp2) as u32,
        BKM_QUERYPAGESTYLE => data(h).and_then(|d| d.book.page(mp1)).map_or(BOOKERR_INVALID_PARAMETERS, |p| p.style as u32),
        BKM_QUERYTABBITMAP => data(h).and_then(|d| d.book.page(mp1)).map_or(BOOKERR_INVALID_PARAMETERS, |p| p.bitmap),
        BKM_SETTABBITMAP => set_bitmap(h, mp1, mp2) as u32,
        BKM_SETTABTEXT | BKM_SETSTATUSLINETEXT => {
            let t = ctl::cstr(mp2 as *const u8);
            let Some(d) = data(h) else { return 0 };
            let id = if mp1 == 0 && msg == BKM_SETSTATUSLINETEXT { d.book.top } else { mp1 };
            let Some(p) = d.book.page_mut(id) else { return 0 };
            if msg == BKM_SETTABTEXT {
                p.tab = t;
                relayout(h);
            } else {
                p.status = t;
                ctl::invalidate(h);
            }
            1
        }
        BKM_QUERYTABTEXT => query_text(h, mp1, mp2, false),
        BKM_QUERYSTATUSLINETEXT => query_text(h, mp1, mp2, true),
        BKM_SETDIMENSIONS => {
            let Some(d) = data(h) else { return 0 };
            let dims = (lo(mp1) as i32, hi(mp1) as i32);
            match lo(mp2) {
                BKA_MAJORTAB => d.major = dims,
                BKA_MINORTAB => d.minor = dims,
                BKA_PAGEBUTTON => d.button = dims,
                _ => return 0,
            }
            relayout(h);
            1
        }
        BKM_CALCPAGERECT => calc_rect(h, mp1, mp2 != 0) as u32,
        BKM_INVALIDATETABS => {
            ctl::invalidate(h);
            1
        }
        BKM_SETNOTEBOOKCOLORS => {
            let Some(d) = data(h) else { return 0 };
            let index = mp2 % 2 == 1;
            let c = Some(color(mp1, index));
            match mp2 {
                1 | 2 => d.page_bg = c,
                3 | 4 => d.major_bg = c,
                5 | 6 => d.minor_bg = c,
                7 | 8 => d.major_fg = c,
                9 | 10 => d.minor_fg = c,
                _ => return 0,
            }
            ctl::invalidate(h);
            1
        }
        BKM_SETTABCOLOR => {
            let Some(d) = data(h) else { return 0 };
            let Some(i) = d.book.index(mp1) else { return 0 };
            d.book.pages[i].tab_color = Some(if mp2 == BKA_AUTOCOLOR { AUTO_COLORS[i % AUTO_COLORS.len()] } else { mp2 & 0xff_ffff });
            ctl::invalidate(h);
            1
        }
        BKM_SETNOTEBOOKBUTTONS => set_buttons(h, mp1, mp2) as u32,
        BKM_SETPAGEINFO => set_page_info(h, mp1, mp2) as u32,
        BKM_QUERYPAGEINFO => query_page_info(h, mp1, mp2) as u32,
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
