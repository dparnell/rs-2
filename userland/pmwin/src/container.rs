//! The container control (WC_CONTAINER): records (RECORDCORE or
//! MINIRECORDCORE) shown in the icon, name, text, detail and tree views,
//! with selection, cursor, in-use and source emphasis, keyboard and
//! mouse, scrolling, sorting, direct editing and moving icons by
//! dragging them. The geometry and selection rules are host-tested in
//! `pmwin::container`.
//!
//! Records and FIELDINFOs are application memory: CM_ALLOCRECORD and
//! CM_ALLOCDETAILFIELDINFO allocate them from the application process's
//! own heap, and the application reads and writes them directly
//! (emphasis lives in each record's flRecordAttr, as on OS/2). The
//! control's own data, in the shared state, holds only their addresses
//! and order; it reads the records only on the thread that owns the
//! container, which is in the application's process. The PM server never
//! runs this paint code for a container whose program does not respond
//! (it paints a plain placeholder), since it cannot read the records.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use gpi::{Point, Rect};
use pmwin::container::{self as layout, Column, Dir, Item, Metrics, Pick, Placed, SelMode, View};
use pmwin::ws;

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::{self, wndproc};
use crate::msg::{send, WinDefWindowProc};
use crate::state::st;
use crate::win::{self, WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_MOUSEMOVE, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_BUTTON2DOWN: u32 = 0x0074;
const WM_BUTTON2UP: u32 = 0x0075;
const WM_CONTEXTMENU: u32 = 0x0424;

const CM_ALLOCDETAILFIELDINFO: u32 = 0x0330;
const CM_ALLOCRECORD: u32 = 0x0331;
const CM_ARRANGE: u32 = 0x0332;
const CM_ERASERECORD: u32 = 0x0333;
const CM_FILTER: u32 = 0x0334;
const CM_FREEDETAILFIELDINFO: u32 = 0x0335;
const CM_FREERECORD: u32 = 0x0336;
const CM_HORZSCROLLSPLITWINDOW: u32 = 0x0337;
const CM_INSERTDETAILFIELDINFO: u32 = 0x0338;
const CM_INSERTRECORD: u32 = 0x0339;
const CM_INVALIDATEDETAILFIELDINFO: u32 = 0x033a;
const CM_INVALIDATERECORD: u32 = 0x033b;
const CM_PAINTBACKGROUND: u32 = 0x033c;
const CM_QUERYCNRINFO: u32 = 0x033d;
const CM_QUERYDETAILFIELDINFO: u32 = 0x033e;
const CM_QUERYRECORD: u32 = 0x0340;
const CM_QUERYRECORDEMPHASIS: u32 = 0x0341;
const CM_QUERYRECORDFROMRECT: u32 = 0x0342;
const CM_QUERYRECORDRECT: u32 = 0x0343;
const CM_QUERYVIEWPORTRECT: u32 = 0x0344;
const CM_REMOVEDETAILFIELDINFO: u32 = 0x0345;
const CM_REMOVERECORD: u32 = 0x0346;
const CM_SCROLLWINDOW: u32 = 0x0347;
const CM_SEARCHSTRING: u32 = 0x0348;
const CM_SETCNRINFO: u32 = 0x0349;
const CM_SETRECORDEMPHASIS: u32 = 0x034a;
const CM_SORTRECORD: u32 = 0x034b;
const CM_OPENEDIT: u32 = 0x034c;
const CM_CLOSEEDIT: u32 = 0x034d;
const CM_COLLAPSETREE: u32 = 0x034e;
const CM_EXPANDTREE: u32 = 0x034f;
const CM_QUERYRECORDINFO: u32 = 0x0350;
const CM_INSERTRECORDARRAY: u32 = 0x0351;
const CM_MOVETREE: u32 = 0x0352;
const CM_SETTEXTVISIBILITY: u32 = 0x0353;

const CN_DRAGLEAVE: u32 = 102;
const CN_DRAGOVER: u32 = 103;
const CN_DROP: u32 = 104;
const CN_DROPHELP: u32 = 105;
const CN_ENTER: u32 = 106;
const CN_INITDRAG: u32 = 107;
const CN_EMPHASIS: u32 = 108;
const CN_KILLFOCUS: u32 = 109;
const CN_SCROLL: u32 = 110;
const CN_SETFOCUS: u32 = 112;
const CN_REALLOCPSZ: u32 = 113;
const CN_BEGINEDIT: u32 = 114;
const CN_ENDEDIT: u32 = 115;
const CN_COLLAPSETREE: u32 = 116;
const CN_EXPANDTREE: u32 = 117;
const CN_HELP: u32 = 118;
const CN_CONTEXTMENU: u32 = 119;

const CCS_AUTOPOSITION: u32 = 0x0008;
const CCS_READONLY: u32 = 0x0020;
const CCS_MINIRECORDCORE: u32 = 0x0040;

const CA_CONTAINERTITLE: u32 = 0x0200;
const CA_TITLESEPARATOR: u32 = 0x0400;
const CA_TITLERIGHT: u32 = 0x1000;
const CA_TITLECENTER: u32 = 0x2000;
const CA_DETAILSVIEWTITLES: u32 = 0x8000;
const CA_TITLEREADONLY: u32 = 0x0008_0000;
const CA_TREELINE: u32 = 0x0040_0000;
const CA_OWNERDRAW: u32 = 0x4000;

const WM_DRAWITEM: u32 = 0x0036;
const DM_DROP: u32 = 0x032f;
const DM_DRAGOVER: u32 = 0x032e;
const DM_DRAGLEAVE: u32 = 0x032d;
const DM_DROPHELP: u32 = 0x032c;

const CRA_SELECTED: u32 = layout::CRA_SELECTED;
const CRA_TARGET: u32 = layout::CRA_TARGET;
const CRA_CURSORED: u32 = layout::CRA_CURSORED;
const CRA_INUSE: u32 = layout::CRA_INUSE;
const CRA_FILTERED: u32 = layout::CRA_FILTERED;
const CRA_RECORDREADONLY: u32 = 0x0040;
const CRA_EXPANDED: u32 = 0x0080;
const CRA_COLLAPSED: u32 = 0x0100;
const CRA_DISABLED: u32 = layout::CRA_DISABLED;
const CRA_SOURCE: u32 = layout::CRA_SOURCE;

const CFA_RIGHT: u32 = 0x0002;
const CFA_CENTER: u32 = 0x0004;
const CFA_INVISIBLE: u32 = 0x0040;
const CFA_BITMAPORICON: u32 = 0x0100;
const CFA_SEPARATOR: u32 = 0x0200;
const CFA_HORZSEPARATOR: u32 = 0x0400;
const CFA_STRING: u32 = 0x0800;
const CFA_DATE: u32 = 0x2000;
const CFA_TIME: u32 = 0x4000;
const CFA_FIREADONLY: u32 = 0x8000;
const CFA_ULONG: u32 = 0x0002_0000;

const CMA_BOTTOM: u32 = 0x0002;
const CMA_FIRST: u32 = 0x0010;
const CMA_LAST: u32 = 0x0020;
const CMA_END: u32 = 0x0040;
const CMA_PREV: u32 = 0x0080;
const CMA_NEXT: u32 = 0x0100;
const CMA_PARENT: u32 = 0x0001;
const CMA_FIRSTCHILD: u32 = 0x0002;
const CMA_LASTCHILD: u32 = 0x0004;
const CMA_ZORDER: u32 = 0x0008;
const CMA_WINDOW: u32 = 0x0002;
const CMA_WORKSPACE: u32 = 0x0004;
const CMA_ICON: u32 = 0x0800;
const CMA_TEXT: u32 = 0x1000;
const CMA_PARTIAL: u32 = 0x2000;
const CMA_COMPLETE: u32 = 0x4000;
const CMA_HORIZONTAL: u32 = 0x0200;
const CMA_VERTICAL: u32 = 0x0400;
const CMA_FREE: u32 = 0x0001;
const CMA_INVALIDATE: u32 = 0x0002;
const CMA_REPOSITION: u32 = 0x0002;
const CMA_TEXTCHANGED: u32 = 0x0004;

// CNRINFO fields to set (CM_SETCNRINFO).
const CMA_CNRTITLE: u32 = 0x0001;
const CMA_DELTA: u32 = 0x0002;
const CMA_FLWINDOWATTR: u32 = 0x0004;
const CMA_LINESPACING: u32 = 0x0008;
const CMA_PFIELDINFOLAST: u32 = 0x0010;
const CMA_PSORTRECORD: u32 = 0x0020;
const CMA_PTLORIGIN: u32 = 0x0040;
const CMA_SLBITMAPORICON: u32 = 0x0080;
const CMA_XVERTSPLITBAR: u32 = 0x0100;
const CMA_PFIELDINFOOBJECT: u32 = 0x0200;
const CMA_TREEICON: u32 = 0x0400;
const CMA_CXTREEINDENT: u32 = 0x1000;
const CMA_CXTREELINE: u32 = 0x2000;
const CMA_SLTREEBITMAPORICON: u32 = 0x4000;

const SBM_SETSCROLLBAR: u32 = 0x01a0;
const SBM_SETTHUMBSIZE: u32 = 0x01a6;

/// Child window ids.
const CID_HSCROLL: u32 = 0x7ff3;
const CID_VSCROLL: u32 = 0x7ff9;
const CID_MLE: u32 = 0x7ffa;

// RECORDCORE offsets.
const R_CB: u32 = 0;
const R_ATTR: u32 = 4;
const R_X: u32 = 8;
const R_Y: u32 = 12;
const R_NEXT: u32 = 16;
const R_PSZICON: u32 = 20;
const R_HPTRICON: u32 = 24;
const R_HPTRMINI: u32 = 28;
const R_PSZTEXT: u32 = 44;
const R_PSZNAME: u32 = 48;
const R_PSZTREE: u32 = 52;
const RECORDCORE: u32 = 56;
const MINIRECORDCORE: u32 = 28;

// FIELDINFO offsets.
const F_DATA: u32 = 4;
const F_TITLEFL: u32 = 8;
const F_TITLE: u32 = 12;
const F_OFF: u32 = 16;
const F_NEXT: u32 = 24;
const F_WIDTH: u32 = 28;
const FIELDINFO: u32 = 32;

const SCROLL_W: i32 = 16;
const MAGIC_REC: u32 = 0x4345_5243; // "CREC"
const MAGIC_FLD: u32 = 0x444c_4643; // "CFLD"

wndproc! {
    container_entry => container_proc;
}

struct Cnr {
    attr: u32,
    title: u32,
    sort: u32,
    field_last: u32,
    field_object: u32,
    delta: u32,
    icon_size: (i32, i32),
    tree_icon: (i32, i32),
    line_spacing: i32,
    tree_indent: i32,
    tree_line: i32,
    split: i32,
    /// Icon view: workspace position of the item area's bottom-left.
    org: (i32, i32),
    /// Other views: scrolled distance (right, down) from the top-left.
    scroll: (i32, i32),
    /// Root records in item order; children of tree records.
    top: Vec<u32>,
    kids: BTreeMap<u32, Vec<u32>>,
    parent: BTreeMap<u32, u32>,
    /// Icon view drawing order (bottom first).
    z: Vec<u32>,
    fields: Vec<u32>,
    cursor: u32,
    anchor: u32,
    vs: u32,
    hs: u32,
    /// Button 2 pressed on a record (icon view move or context menu).
    b2: Option<(u32, Point, bool)>,
    /// Direct editing: the MLE, the record and field.
    edit: (u32, u32, u32),
    /// The record with target emphasis while something is dragged over.
    drop_on: u32,
}

fn data(h: u32) -> Option<&'static mut Cnr> {
    ctl::data::<Cnr>(h)
}

pub fn is_container(h: u32) -> bool {
    data(h).is_some()
}

// ---- Application memory ----

fn rd(p: u32, off: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    unsafe { ((p + off) as *const u32).read_unaligned() }
}

fn wr(p: u32, off: u32, v: u32) {
    if p != 0 {
        unsafe { ((p + off) as *mut u32).write_unaligned(v) };
    }
}

fn attr(r: u32) -> u32 {
    rd(r, R_ATTR)
}

fn set_attr(r: u32, v: u32) {
    wr(r, R_ATTR, v);
}

/// Allocate `n` zeroed blocks of `size` bytes in the application's heap,
/// each behind a 16-byte header (magic, size).
fn alloc_blocks(n: u32, size: u32, magic: u32) -> Vec<u32> {
    let mut v = Vec::new();
    for _ in 0..n {
        let total = (size + 16) as usize;
        let p = unsafe { os2::heap::alloc_private(total) };
        if p.is_null() {
            break;
        }
        unsafe {
            core::ptr::write_bytes(p, 0, total);
            (p as *mut u32).write(magic);
            (p as *mut u32).add(1).write(total as u32);
        }
        v.push(p as u32 + 16);
    }
    v
}

fn free_block(p: u32, magic: u32) -> bool {
    if p < 16 {
        return false;
    }
    let base = (p - 16) as *mut u32;
    unsafe {
        if base.read() != magic {
            return false;
        }
        let size = base.add(1).read() as usize;
        base.write(0);
        os2::heap::free_private(base as *mut u8, size);
    }
    true
}

fn text_at(p: u32) -> Vec<u8> {
    ctl::cstr(p as *const u8)
}

// ---- The records ----

/// Every record in the container (roots and children).
fn all(c: &Cnr) -> Vec<u32> {
    let mut v = Vec::new();
    let mut stack: Vec<u32> = c.top.iter().rev().copied().collect();
    while let Some(r) = stack.pop() {
        v.push(r);
        if let Some(k) = c.kids.get(&r) {
            stack.extend(k.iter().rev().copied());
        }
    }
    v
}

fn contains(c: &Cnr, r: u32) -> bool {
    r != 0 && (c.top.contains(&r) || c.parent.contains_key(&r))
}

fn view(c: &Cnr) -> View {
    View::from_attr(c.attr)
}

fn mini(c: &Cnr) -> bool {
    c.attr & layout::CV_MINI != 0
}

fn full(h: u32, r: u32) -> bool {
    ctl::style(h) & CCS_MINIRECORDCORE == 0 && rd(r, R_CB) >= RECORDCORE
}

fn record_text(h: u32, c: &Cnr, r: u32) -> Vec<u8> {
    let off = if !full(h, r) {
        R_PSZICON
    } else {
        match view(c) {
            View::Icon => R_PSZICON,
            View::Name => R_PSZNAME,
            View::Text => R_PSZTEXT,
            View::Tree => {
                if c.attr & layout::CV_TEXT != 0 {
                    R_PSZTEXT
                } else if c.attr & layout::CV_NAME != 0 {
                    R_PSZNAME
                } else {
                    R_PSZTREE
                }
            }
            View::Detail => R_PSZICON,
        }
    };
    text_at(rd(r, off))
}

fn record_icon(h: u32, c: &Cnr, r: u32) -> u32 {
    if mini(c) && full(h, r) && rd(r, R_HPTRMINI) != 0 {
        rd(r, R_HPTRMINI)
    } else {
        rd(r, R_HPTRICON)
    }
}

/// The records shown, in display order (tree: depth-first), with their
/// depth.
fn shown(c: &Cnr) -> Vec<(u32, u32)> {
    let visible = |r: u32| attr(r) & CRA_FILTERED == 0;
    if view(c) != View::Tree {
        let order: &Vec<u32> = if view(c) == View::Icon { &c.z } else { &c.top };
        return order.iter().copied().filter(|&r| visible(r)).map(|r| (r, 0)).collect();
    }
    let mut v = Vec::new();
    let mut stack: Vec<(u32, u32)> = c.top.iter().rev().map(|&r| (r, 0)).collect();
    while let Some((r, d)) = stack.pop() {
        if !visible(r) {
            continue;
        }
        v.push((r, d));
        if let Some(k) = c.kids.get(&r) {
            stack.extend(k.iter().rev().map(|&x| (x, d + 1)));
        }
    }
    v
}

/// Item order of the shown records (icon view: item order, not z-order).
fn item_order(c: &Cnr) -> Vec<u32> {
    if view(c) == View::Icon {
        c.top.iter().copied().filter(|&r| attr(r) & CRA_FILTERED == 0).collect()
    } else {
        shown(c).into_iter().map(|x| x.0).collect()
    }
}

// ---- Geometry ----

/// The parts of the window.
struct Parts {
    /// Container title.
    title: Rect,
    /// Detail view column headings.
    heads: Rect,
    /// Where the records are.
    area: Rect,
}

fn title_h(c: &Cnr) -> i32 {
    if c.attr & CA_CONTAINERTITLE != 0 && c.title != 0 {
        let n = text_at(c.title).split(|&b| b == b'\n').count().max(1) as i32;
        n * draw::font().height + 4 + if c.attr & CA_TITLESEPARATOR != 0 { 2 } else { 0 }
    } else {
        0
    }
}

fn parts(h: u32) -> Parts {
    let (cx, cy) = ctl::size(h);
    let Some(c) = data(h) else { return Parts { title: Rect::default(), heads: Rect::default(), area: Rect::default() } };
    let vs = c.vs != 0 && st().desk.get(c.vs).is_some_and(|w| w.visible_style());
    let hs = c.hs != 0 && st().desk.get(c.hs).is_some_and(|w| w.visible_style());
    let (l, b, r, t) = (1, 1 + if hs { SCROLL_W } else { 0 }, cx - 1 - if vs { SCROLL_W } else { 0 }, cy - 1);
    let th = title_h(c);
    let title = Rect::new(l, t - th, r, t);
    let hh = if view(c) == View::Detail && c.attr & CA_DETAILSVIEWTITLES != 0 { draw::font().height + 6 } else { 0 };
    let heads = Rect::new(l, t - th - hh, r, t - th);
    let area = Rect::new(l, b, r.max(l), (t - th - hh).max(b));
    Parts { title, heads, area }
}

fn metrics(h: u32, c: &Cnr, area: Rect) -> Metrics {
    let f = draw::font();
    let v = view(c);
    let icon = if v == View::Text || (v == View::Tree && c.attr & layout::CV_TEXT != 0) {
        0
    } else if v == View::Detail {
        if mini(c) { crate::pointer::MINI } else { c.icon_size.1.max(0).min(64) }
    } else if mini(c) || (v == View::Tree && c.attr & layout::CV_ICON == 0 && c.attr & layout::CV_NAME == 0) {
        crate::pointer::MINI
    } else if c.icon_size.1 > 0 {
        c.icon_size.1.min(64)
    } else {
        crate::pointer::ICON
    };
    let _ = h;
    Metrics {
        cw: f.width,
        ch: f.height,
        icon,
        spacing: c.line_spacing.max(0),
        indent: if c.tree_indent > 0 { c.tree_indent } else { 16 },
        area_w: area.width(),
        area_h: area.height(),
        flow: c.attr & layout::CV_FLOW != 0,
    }
}

/// The detail view's columns (FIELDINFOs that are not invisible).
fn columns(c: &Cnr) -> Vec<u32> {
    c.fields.iter().copied().filter(|&f| rd(f, F_DATA) & CFA_INVISIBLE == 0).collect()
}

/// A detail view cell's text.
fn cell_text(r: u32, f: u32) -> Vec<u8> {
    let fl = rd(f, F_DATA);
    let at = r + rd(f, F_OFF);
    if fl & CFA_BITMAPORICON != 0 {
        return Vec::new();
    }
    if fl & CFA_ULONG != 0 {
        return alloc::format!("{}", rd(at, 0)).into_bytes();
    }
    if fl & CFA_DATE != 0 {
        let v = rd(at, 0);
        return alloc::format!("{:02}-{:02}-{:04}", (v >> 8) & 0xff, v & 0xff, v >> 16).into_bytes();
    }
    if fl & CFA_TIME != 0 {
        let v = rd(at, 0);
        return alloc::format!("{:02}:{:02}:{:02}", v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff).into_bytes();
    }
    if fl & CFA_STRING != 0 || fl & 0x0003_f900 == 0 {
        return text_at(rd(at, 0));
    }
    Vec::new()
}

/// The current layout: the records in the order placed, and where.
struct Lay {
    view: View,
    recs: Vec<u32>,
    placed: Vec<Placed>,
    bounds: Rect,
    m: Metrics,
    parts: Parts,
    cols: Vec<u32>,
    widths: Vec<i32>,
}

fn compute(h: u32) -> Option<Lay> {
    let p = parts(h);
    let c = data(h)?;
    let v = view(c);
    let m = metrics(h, c, p.area);
    let list = shown(c);
    let mut items = Vec::with_capacity(list.len());
    let cols = if v == View::Detail { columns(c) } else { Vec::new() };
    for &(r, d) in &list {
        let mut it = Item { text: record_text(h, c, r), pos: (rd(r, R_X) as i32, rd(r, R_Y) as i32), depth: d, ..Default::default() };
        if v == View::Tree {
            it.has_children = c.kids.get(&r).is_some_and(|k| !k.is_empty());
            it.expanded = attr(r) & CRA_EXPANDED != 0;
        }
        if v == View::Detail {
            it.cells = cols.iter().map(|&f| cell_text(r, f)).collect();
        }
        items.push(it);
    }
    let colspec: Vec<Column> = cols.iter().map(|&f| Column { width: rd(f, F_WIDTH) as i32, icon: rd(f, F_DATA) & CFA_BITMAPORICON != 0, title: if rd(f, F_TITLEFL) & CFA_BITMAPORICON != 0 { Vec::new() } else { text_at(rd(f, F_TITLE)) } }).collect();
    let (placed, bounds) = layout::layout(v, &items, &m, &colspec);
    let widths = layout::column_widths(&items, &m, &colspec);
    let recs = list.into_iter().map(|x| x.0).collect();
    Some(Lay { view: v, recs, placed, bounds, m, parts: p, cols, widths })
}

/// Workspace -> window coordinates.
fn to_window(c: &Cnr, area: Rect, v: View, r: Rect) -> Rect {
    if v == View::Icon {
        r.offset(area.left - c.org.0, area.bottom - c.org.1)
    } else {
        r.offset(area.left - c.scroll.0, area.top + c.scroll.1)
    }
}

/// Window -> workspace coordinates.
fn to_work(c: &Cnr, area: Rect, v: View, p: Point) -> Point {
    if v == View::Icon {
        Point::new(p.x - area.left + c.org.0, p.y - area.bottom + c.org.1)
    } else {
        Point::new(p.x - area.left + c.scroll.0, p.y - area.top - c.scroll.1)
    }
}

/// The viewport in workspace coordinates.
fn viewport(c: &Cnr, area: Rect, v: View) -> Rect {
    if v == View::Icon {
        Rect::new(c.org.0, c.org.1, c.org.0 + area.width(), c.org.1 + area.height())
    } else {
        Rect::new(c.scroll.0, -c.scroll.1 - area.height(), c.scroll.0 + area.width(), -c.scroll.1)
    }
}

/// Show or hide the scroll bars as the contents need them and set their
/// ranges.
fn sync_scroll(h: u32) {
    let (cx, cy) = ctl::size(h);
    for _ in 0..2 {
        let Some(l) = compute(h) else { return };
        let c = data(h).unwrap();
        let a = l.parts.area;
        let vp = viewport(c, a, l.view);
        let ext = layout::union(&l.bounds, &vp);
        let need_v = l.bounds.height() > a.height() || ext.top > vp.top || ext.bottom < vp.bottom;
        let need_h = l.bounds.width() > a.width() || ext.left < vp.left || ext.right > vp.right;
        let (vs, hs) = (c.vs, c.hs);
        let mut changed = false;
        for (sb, need) in [(vs, need_v), (hs, need_h)] {
            let shown = st().desk.get(sb).is_some_and(|w| w.visible_style());
            if shown != need {
                st().desk.set_visible(sb, need);
                changed = true;
            }
        }
        let hs_shown = need_h;
        st().desk.set_pos(vs, cx - 1 - SCROLL_W, 1 + if hs_shown { SCROLL_W } else { 0 }, SCROLL_W, (cy - 2 - if hs_shown { SCROLL_W } else { 0 }).max(0));
        st().desk.set_pos(hs, 1, 1, (cx - 2 - if need_v { SCROLL_W } else { 0 }).max(0), SCROLL_W);
        if !changed {
            // Positions count from the top (vertical) and left.
            let (vpos, vmax) = (ext.top - vp.top, (ext.height() - a.height()).max(0));
            let (hpos, hmax) = (vp.left - ext.left, (ext.width() - a.width()).max(0));
            send(vs, SBM_SETSCROLLBAR, vpos as u32, mp(0, vmax as u32));
            send(vs, SBM_SETTHUMBSIZE, mp(a.height() as u32, ext.height() as u32), 0);
            send(hs, SBM_SETSCROLLBAR, hpos as u32, mp(0, hmax as u32));
            send(hs, SBM_SETTHUMBSIZE, mp(a.width() as u32, ext.width() as u32), 0);
            return;
        }
    }
}

/// Scroll by (dx right, dy down) pels.
fn scroll_by(h: u32, dx: i32, dy: i32) {
    let Some(l) = compute(h) else { return };
    let c = data(h).unwrap();
    let a = l.parts.area;
    let vp = viewport(c, a, l.view);
    let ext = layout::union(&l.bounds, &vp);
    // Keep the viewport within the extent.
    let dx = dx.clamp(ext.left - vp.left, (ext.right - vp.right).max(0));
    let dy = dy.clamp(-(ext.top - vp.top).max(0), (vp.bottom - ext.bottom).max(0));
    if dx == 0 && dy == 0 {
        return;
    }
    if l.view == View::Icon {
        c.org = (c.org.0 + dx, c.org.1 - dy);
    } else {
        c.scroll = ((c.scroll.0 + dx).max(0), (c.scroll.1 + dy).max(0));
    }
    ctl::invalidate(h);
    sync_scroll(h);
    let n: [u32; 3] = [h, if dy != 0 { dy as u32 } else { dx as u32 }, if dy != 0 { CMA_VERTICAL } else { CMA_HORIZONTAL }];
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CN_SCROLL), n.as_ptr() as u32);
}

/// Scroll so record `r` is visible.
fn reveal(h: u32, r: u32) {
    let Some(l) = compute(h) else { return };
    let Some(i) = l.recs.iter().position(|&x| x == r) else { return };
    let Some(p) = l.placed.iter().find(|p| p.index == i) else { return };
    let c = data(h).unwrap();
    let vp = viewport(c, l.parts.area, l.view);
    let rr = p.rect;
    let dx = if rr.left < vp.left { rr.left - vp.left } else if rr.right > vp.right { (rr.right - vp.right).min(rr.left - vp.left) } else { 0 };
    let dy = if rr.top > vp.top { vp.top - rr.top } else if rr.bottom < vp.bottom { (vp.bottom - rr.bottom).min(vp.top - rr.top) } else { 0 };
    if dx != 0 || dy != 0 {
        scroll_by(h, dx, -dy);
    }
}

// ---- Painting ----

fn hatch(cv: &mut Canvas, r: Rect, col: u32) {
    for k in (-r.height()..r.width()).step_by(4) {
        for y in 0..r.height() {
            let x = k + y;
            if x >= 0 && x < r.width() {
                cv.fill(Rect::new(r.left + x, r.bottom + y, r.left + x + 1, r.bottom + y + 1), col);
            }
        }
    }
}

fn dashed(cv: &mut Canvas, r: Rect) {
    for x in (r.left..r.right).step_by(3) {
        cv.fill(Rect::new(x, r.bottom, x + 2, r.bottom + 1), draw::TEXT);
        cv.fill(Rect::new(x, r.top - 1, x + 2, r.top), draw::TEXT);
    }
    for y in (r.bottom..r.top).step_by(3) {
        cv.fill(Rect::new(r.left, y, r.left + 1, y + 2), draw::TEXT);
        cv.fill(Rect::new(r.right - 1, y, r.right, y + 2), draw::TEXT);
    }
}

fn paint(h: u32) {
    let Some(l) = compute(h) else { return };
    let focus = ctl::has_focus(h);
    let enabled = ctl::enabled(h);
    let c = data(h).unwrap();
    let (title, tattr) = (if c.title != 0 { text_at(c.title) } else { Vec::new() }, c.attr);
    let field_last = c.field_last;
    // What to draw, read from the records now (application memory).
    struct Row {
        rect: Rect,
        icon_r: Rect,
        text_r: Rect,
        lines: Vec<Vec<u8>>,
        icon: u32,
        attr: u32,
        button: Rect,
        expanded: bool,
        cells: Vec<(Vec<u8>, u32, u32)>,
    }
    let mut rows = Vec::with_capacity(l.placed.len());
    // CA_OWNERDRAW: the owner may draw the records' text over the
    // container's (those wholly in the item area).
    let mut owner_rows: Vec<(u32, Rect, u32)> = Vec::new();
    for p in &l.placed {
        if c.attr & CA_OWNERDRAW != 0 && l.view != View::Detail {
            let t = to_window(c, l.parts.area, l.view, p.text);
            if !t.is_empty() && t.intersect(&l.parts.area) == t {
                let r = l.recs[p.index];
                owner_rows.push((r, t, attr(r)));
            }
        }
        let r = l.recs[p.index];
        let cells = if l.view == View::Detail {
            l.cols.iter().map(|&f| (cell_text(r, f), rd(f, F_DATA), if rd(f, F_DATA) & CFA_BITMAPORICON != 0 { rd(r + rd(f, F_OFF), 0) } else { 0 })).collect()
        } else {
            Vec::new()
        };
        rows.push(Row {
            rect: to_window(c, l.parts.area, l.view, p.rect),
            icon_r: to_window(c, l.parts.area, l.view, p.icon),
            text_r: to_window(c, l.parts.area, l.view, p.text),
            lines: p.lines.clone(),
            icon: if l.m.icon > 0 { record_icon(h, c, r) } else { 0 },
            attr: attr(r),
            button: to_window(c, l.parts.area, l.view, p.button),
            expanded: attr(r) & CRA_EXPANDED != 0,
            cells,
        });
    }
    let heads: Vec<(Vec<u8>, u32)> = l.cols.iter().map(|&f| (if rd(f, F_TITLEFL) & CFA_BITMAPORICON != 0 { Vec::new() } else { text_at(rd(f, F_TITLE)) }, rd(f, F_TITLEFL) | rd(f, F_DATA) & (CFA_SEPARATOR | CFA_HORZSEPARATOR))).collect();
    let split_after = l.cols.iter().position(|&f| f == field_last);
    let scroll_x = c.scroll.0;
    let (widths, parts, v, mini_icons) = (l.widths, l.parts, l.view, l.m.icon <= crate::pointer::MINI);
    let f = draw::font();
    // Presentation parameter colours (a desktop's background).
    let bg = crate::misc::pres_color(h, true);
    let text = crate::misc::pres_color(h, false).unwrap_or(draw::TEXT);
    draw::paint(h, |cv| {
        let all = cv.rect();
        cv.fill(all, bg.unwrap_or(draw::WINDOW));
        if bg.is_none() {
            cv.frame(all, draw::SHADOW);
        }
        // The title.
        if parts.title.height() > 0 {
            let fl = if tattr & CA_TITLECENTER != 0 {
                draw::DT_CENTER
            } else if tattr & CA_TITLERIGHT != 0 {
                draw::DT_RIGHT
            } else {
                0
            };
            let tr = Rect::new(parts.title.left + 4, parts.title.bottom + 2, parts.title.right - 4, parts.title.top - 2);
            cv.text_in(tr, &title, text, fl);
            if tattr & CA_TITLESEPARATOR != 0 {
                cv.hline(parts.title.left, parts.title.right, parts.title.bottom, draw::SHADOW);
            }
        }
        // Detail view headings.
        if parts.heads.height() > 0 {
            cv.fill(parts.heads, draw::FACE);
            let mut x = parts.heads.left - scroll_x;
            for (k, (t, fl)) in heads.iter().enumerate() {
                let w = widths.get(k).copied().unwrap_or(0);
                let r = Rect::new(x, parts.heads.bottom, x + w, parts.heads.top);
                let al = if fl & CFA_RIGHT != 0 { draw::DT_RIGHT } else if fl & CFA_CENTER != 0 { draw::DT_CENTER } else { 0 };
                cv.text_in(Rect::new(r.left + 4, r.bottom, r.right - 4, r.top).intersect(&parts.heads), t, draw::TEXT, al | draw::DT_VCENTER);
                cv.vline(r.right - 1, r.bottom, r.top, draw::SHADOW);
                x += w;
            }
            cv.hline(parts.heads.left, parts.heads.right, parts.heads.bottom, draw::SHADOW);
        }
        let area = parts.area;
        let saved = cv.ps.clip.clone();
        let o = cv.ps.origin;
        let dev = area.offset(o.x, o.y);
        cv.ps.clip = saved.iter().map(|r| r.intersect(&dev)).filter(|r| !r.is_empty()).collect();
        for row in &rows {
            if row.rect.intersect(&area).is_empty() {
                continue;
            }
            let sel = row.attr & CRA_SELECTED != 0;
            let disabled = row.attr & CRA_DISABLED != 0 || !enabled;
            let fg = if disabled { draw::GRAY_TEXT } else if sel { draw::HILITE_TEXT } else { text };
            if v == View::Detail {
                if sel {
                    cv.fill(row.rect, draw::HILITE);
                }
                if row.attr & CRA_INUSE != 0 {
                    hatch(cv, row.rect, draw::SHADOW);
                }
                let mut x = row.rect.left;
                for (k, (t, fl, hp)) in row.cells.iter().enumerate() {
                    let w = widths.get(k).copied().unwrap_or(0);
                    let r = Rect::new(x, row.rect.bottom, x + w, row.rect.top);
                    if fl & CFA_BITMAPORICON != 0 {
                        if *hp != 0 {
                            crate::pointer::draw_in(cv, r, *hp, mini_icons, disabled);
                        }
                    } else {
                        let al = if fl & CFA_RIGHT != 0 { draw::DT_RIGHT } else if fl & CFA_CENTER != 0 { draw::DT_CENTER } else { 0 };
                        cv.text_in(Rect::new(r.left + 4, r.bottom, r.right - 4, r.top), t, fg, al | draw::DT_VCENTER);
                    }
                    if fl & CFA_SEPARATOR != 0 {
                        cv.vline(r.right - 1, r.bottom, r.top, draw::SHADOW);
                    }
                    if fl & CFA_HORZSEPARATOR != 0 {
                        cv.hline(r.left, r.right, r.bottom, draw::SHADOW);
                    }
                    if split_after == Some(k) {
                        cv.fill(Rect::new(r.right - 2, r.bottom, r.right + 1, r.top), draw::DARK);
                    }
                    x += w;
                }
                if focus && row.attr & CRA_CURSORED != 0 {
                    cv.focus_rect(row.rect);
                }
                if row.attr & (CRA_SOURCE | CRA_TARGET) != 0 {
                    dashed(cv, row.rect);
                }
                continue;
            }
            // Tree lines and the expand/collapse button.
            if v == View::Tree && !row.button.is_empty() {
                let b = row.button;
                cv.fill(b, bg.unwrap_or(draw::WINDOW));
                cv.frame(b, draw::SHADOW);
                let (mx, my) = (b.left + b.width() / 2, b.bottom + b.height() / 2);
                cv.hline(b.left + 2, b.right - 2, my, draw::TEXT);
                if !row.expanded {
                    cv.vline(mx, b.bottom + 2, b.top - 2, draw::TEXT);
                }
            }
            if v == View::Tree && tattr & CA_TREELINE != 0 && row.button.is_empty() {
                let y = row.rect.bottom + row.rect.height() / 2;
                let x = row.icon_r.left.min(row.text_r.left) - 9;
                cv.hline(x, x + 7, y, draw::SHADOW);
            }
            if row.attr & CRA_INUSE != 0 {
                let r = if row.icon_r.is_empty() { row.text_r } else { row.icon_r };
                hatch(cv, r, draw::SHADOW);
            }
            if row.icon != 0 && !row.icon_r.is_empty() {
                crate::pointer::draw_in(cv, row.icon_r, row.icon, mini_icons, disabled);
            }
            if sel {
                cv.fill(row.text_r, draw::HILITE);
            }
            let mut y = row.text_r.top;
            for ln in &row.lines {
                let tw = ln.len() as i32 * f.width;
                let x = if v == View::Icon { row.text_r.left + (row.text_r.width() - tw) / 2 } else { row.text_r.left + 2 };
                cv.text(x, y - f.height + f.descender, ln, fg, row.text_r);
                y -= f.height;
            }
            if focus && row.attr & CRA_CURSORED != 0 {
                cv.focus_rect(row.text_r);
            }
            if row.attr & (CRA_SOURCE | CRA_TARGET) != 0 {
                let r = Rect::new(row.rect.left - 2, row.rect.bottom - 2, row.rect.right + 2, row.rect.top + 2);
                dashed(cv, r);
            }
        }
        cv.ps.clip = saved;
    });
    owner_draw(h, owner_rows);
}

/// WM_DRAWITEM to the owner for each record's text (OWNERITEM: hwnd,
/// hps, fsState, fsAttribute, fsStateOld, fsAttributeOld, rclItem,
/// idItem CMA_TEXT, hItem the CNRDRAWITEMINFO: pRecord, pFieldInfo).
/// The container drew the text already; an owner that draws returns
/// TRUE and paints over it.
fn owner_draw(h: u32, rows: Vec<(u32, Rect, u32)>) {
    if rows.is_empty() {
        return;
    }
    let (owner, id) = (ctl::owner(h), ctl::id(h));
    let hps = win::WinGetPS(h);
    for (r, t, a) in rows {
        let info: [u32; 2] = [r, 0];
        let oi: [u32; 12] = [h, hps, a, a, a, a, t.left as u32, t.bottom as u32, t.right as u32, t.top as u32, CMA_TEXT, info.as_ptr() as u32];
        send(owner, WM_DRAWITEM, id, oi.as_ptr() as u32);
    }
    win::WinReleasePS(hps);
}

// ---- Direct manipulation ----

/// What is dragged over the container goes to the owner, as IBM's
/// container does it: DM_DRAGOVER as CN_DRAGOVER (CNRDRAGINFO: the
/// DRAGINFO and the record under the pointer, or none, over empty
/// space; the owner's answer is the container's), DM_DRAGLEAVE as
/// CN_DRAGLEAVE, DM_DROP as CN_DROP (the record at the drop position),
/// DM_DROPHELP as CN_DROPHELP. The record under the pointer has target
/// emphasis while it is.
fn drag_msg(h: u32, msg: u32, pdinfo: u32, mp2: u32) -> u32 {
    // The DRAGINFO is shared memory the owner's process may not have yet
    // (what DrgAccessDraginfo does).
    if pdinfo > 0x2c {
        unsafe { DosGetSharedMem((pdinfo - 0x2c) as *mut u8, 3) };
    }
    let rec = if msg == DM_DRAGLEAVE || pdinfo == 0 {
        0
    } else {
        let pos = if msg == DM_DRAGOVER { mp2 } else { rd(pdinfo, 12) };
        let o = st().desk.origin(h);
        let p = Point { x: pos as i16 as i32 - o.x, y: (pos >> 16) as i16 as i32 - o.y };
        record_at(h, p).filter(|x| !x.1).map_or(0, |x| x.0)
    };
    let new = if msg == DM_DRAGOVER { rec } else { 0 };
    let old = data(h).map_or(0, |c| core::mem::replace(&mut c.drop_on, new));
    if old != new {
        if old != 0 && data(h).is_some_and(|c| contains(c, old)) {
            emphasize(h, old, CRA_TARGET, false, false);
        }
        if new != 0 {
            emphasize(h, new, CRA_TARGET, true, false);
        }
    }
    let code = match msg {
        DM_DRAGOVER => CN_DRAGOVER,
        DM_DRAGLEAVE => CN_DRAGLEAVE,
        DM_DROP => CN_DROP,
        _ => CN_DROPHELP,
    };
    let cdi: [u32; 2] = [pdinfo, rec];
    let r = send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), code), cdi.as_ptr() as u32);
    if msg == DM_DRAGOVER { r } else { 0 }
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.302"]
    fn DosGetSharedMem(pb: *mut u8, flags: u32) -> u32;
}

// ---- Notifications ----

fn notify_rec(h: u32, code: u32, r: u32) -> u32 {
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), code), r)
}

fn notify_emphasis(h: u32, r: u32, mask: u32) {
    let n: [u32; 3] = [h, r, mask];
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CN_EMPHASIS), n.as_ptr() as u32);
}

fn notify_enter(h: u32, r: u32, key: bool) {
    let n: [u32; 3] = [h, key as u32, r];
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CN_ENTER), n.as_ptr() as u32);
}

/// Change emphasis bits of a record; repaints, and tells the owner if
/// `user` (a change the user made).
fn emphasize(h: u32, r: u32, bits: u32, on: bool, user: bool) {
    let a = attr(r);
    let n = if on { a | bits } else { a & !bits };
    if n == a {
        return;
    }
    set_attr(r, n);
    ctl::invalidate(h);
    if user {
        notify_emphasis(h, r, (a ^ n) & (CRA_SELECTED | CRA_CURSORED | CRA_INUSE | CRA_SOURCE));
    }
}

/// Move the cursor to `r` (one record has it).
fn set_cursor(h: u32, r: u32, user: bool) {
    let Some(c) = data(h) else { return };
    let old = c.cursor;
    if old == r {
        return;
    }
    c.cursor = r;
    if contains(c, old) {
        emphasize(h, old, CRA_CURSORED, false, user);
    }
    if r != 0 {
        emphasize(h, r, CRA_CURSORED, true, user);
    }
}

/// The user picked record `r` (click, keyboard, space).
fn pick(h: u32, r: u32, how: Pick) {
    let Some(c) = data(h) else { return };
    let order = item_order(c);
    let Some(i) = order.iter().position(|&x| x == r) else { return };
    let mut sel: Vec<bool> = order.iter().map(|&x| attr(x) & CRA_SELECTED != 0).collect();
    let anchor = order.iter().position(|&x| x == c.anchor);
    let idx: Vec<usize> = (0..order.len()).collect();
    let mode = SelMode::from_style(ctl::style(h));
    let (changed, anchor) = layout::pick(&mut sel, &idx, i, anchor, mode, how);
    if let Some(c) = data(h) {
        c.anchor = anchor.map_or(0, |a| order[a]);
    }
    set_cursor(h, r, true);
    // Deselections first.
    for &k in changed.iter().filter(|&&k| !sel[k]) {
        emphasize(h, order[k], CRA_SELECTED, false, true);
    }
    for &k in changed.iter().filter(|&&k| sel[k]) {
        emphasize(h, order[k], CRA_SELECTED, true, true);
    }
    reveal(h, r);
}

fn deselect_all(h: u32, user: bool) {
    let Some(c) = data(h) else { return };
    for r in all(c) {
        if attr(r) & CRA_SELECTED != 0 {
            emphasize(h, r, CRA_SELECTED, false, user);
        }
    }
}

// ---- Inserting and removing ----

fn insert_one(h: u32, r: u32, order: u32, parent: u32, z: u32) {
    let Some(c) = data(h) else { return };
    if contains(c, r) {
        return;
    }
    let list: &mut Vec<u32> = if parent != 0 && contains(c, parent) {
        c.parent.insert(r, parent);
        c.kids.entry(parent).or_default()
    } else {
        &mut c.top
    };
    match order {
        CMA_FIRST => list.insert(0, r),
        CMA_END | CMA_LAST | 0 => list.push(r),
        after => match list.iter().position(|&x| x == after) {
            Some(i) => list.insert(i + 1, r),
            None => list.push(r),
        },
    }
    if parent == 0 || !c.parent.contains_key(&r) {
        if z == CMA_BOTTOM {
            c.z.insert(0, r);
        } else {
            c.z.push(r);
        }
    }
    // Tree records with children are collapsed until expanded.
    let a = attr(r);
    if a & (CRA_EXPANDED | CRA_COLLAPSED) == 0 {
        set_attr(r, a | CRA_COLLAPSED);
    }
}

/// RECORDINSERT: cb, pRecordOrder, pRecordParent, fInvalidateRecord,
/// zOrder, cRecordsInsert.
fn insert(h: u32, recs: &[u32], ri: *const u32) -> u32 {
    if ri.is_null() {
        return 0;
    }
    let v: [u32; 6] = core::array::from_fn(|i| unsafe { ri.add(i).read_unaligned() });
    let (mut order, parent, inval, z) = (v[1], v[2], v[3], v[4]);
    for &r in recs {
        insert_one(h, r, order, parent, z);
        // The next one goes after this one (unless they go to the end).
        if !matches!(order, CMA_END | CMA_LAST | 0) {
            order = r;
        }
    }
    if ctl::style(h) & CCS_AUTOPOSITION != 0 {
        arrange(h);
    }
    if inval != 0 {
        ctl::invalidate(h);
        sync_scroll(h);
    }
    data(h).map_or(0, |c| all(c).len() as u32)
}

fn remove_one(h: u32, r: u32, free: bool) {
    let Some(c) = data(h) else { return };
    // Children go with their parent.
    let kids = c.kids.remove(&r).unwrap_or_default();
    for k in kids {
        remove_one(h, k, free);
    }
    let c = data(h).unwrap();
    c.top.retain(|&x| x != r);
    c.z.retain(|&x| x != r);
    if let Some(p) = c.parent.remove(&r) {
        if let Some(v) = c.kids.get_mut(&p) {
            v.retain(|&x| x != r);
        }
    }
    if c.cursor == r {
        c.cursor = 0;
    }
    if c.anchor == r {
        c.anchor = 0;
    }
    if free {
        free_block(r, MAGIC_REC);
    }
}

// ---- Icon view ----

fn arrange(h: u32) {
    let p = parts(h);
    let Some(c) = data(h) else { return };
    let m = metrics(h, c, p.area);
    let recs: Vec<u32> = c.top.iter().copied().filter(|&r| attr(r) & CRA_FILTERED == 0).collect();
    let items: Vec<Item> = recs.iter().map(|&r| Item { text: text_at(rd(r, R_PSZICON)), ..Default::default() }).collect();
    let m = Metrics { icon: if m.icon == 0 { crate::pointer::ICON } else { m.icon }, ..m };
    for (r, (x, y)) in recs.iter().zip(layout::arrange(&items, &m)) {
        wr(*r, R_X, x as u32);
        wr(*r, R_Y, y as u32);
    }
    if let Some(c) = data(h) {
        c.org = (0, 0);
    }
    ctl::invalidate(h);
    sync_scroll(h);
}

// ---- Sorting ----

/// Sort the records (each level of the tree) with the application's
/// comparison function `pfn(p1, p2, pStorage)` (returns SHORT), called
/// without the PM lock.
fn sort(h: u32, pfn: u32, storage: u32) -> u32 {
    if pfn == 0 {
        return 0;
    }
    let f: extern "C" fn(u32, u32, u32) -> i32 = unsafe { core::mem::transmute(pfn as usize) };
    let cmp = |a: &u32, b: &u32| (f(*a, *b, storage) as i16).cmp(&0);
    let Some(c) = data(h) else { return 0 };
    let mut lists: Vec<(Option<u32>, Vec<u32>)> = alloc::vec![(None, c.top.clone())];
    for (&p, k) in c.kids.iter() {
        lists.push((Some(p), k.clone()));
    }
    lock::unlocked(|| {
        for (_, v) in lists.iter_mut() {
            v.sort_by(cmp);
        }
    });
    let Some(c) = data(h) else { return 0 };
    for (p, v) in lists {
        match p {
            None => {
                if v.len() == c.top.len() && v.iter().all(|r| c.top.contains(r)) {
                    c.top = v;
                }
            }
            Some(p) => {
                if let Some(k) = c.kids.get_mut(&p) {
                    if v.len() == k.len() {
                        *k = v;
                    }
                }
            }
        }
    }
    if ctl::style(h) & CCS_AUTOPOSITION != 0 {
        arrange(h);
    }
    ctl::invalidate(h);
    1
}

// ---- Direct editing ----

/// CNREDITDATA: cb, hwndCnr, pRecord, pFieldInfo, ppszText, cbText, id.
fn open_edit(h: u32, r: u32, field: u32) -> u32 {
    close_edit(h, false);
    if ctl::style(h) & CCS_READONLY != 0 || (r != 0 && attr(r) & CRA_RECORDREADONLY != 0) || (field != 0 && rd(field, F_DATA) & CFA_FIREADONLY != 0) {
        return 0;
    }
    let Some(l) = compute(h) else { return 0 };
    let c = data(h).unwrap();
    let (rect, text) = if r == 0 {
        if c.attr & CA_TITLEREADONLY != 0 || l.parts.title.height() == 0 {
            return 0;
        }
        (l.parts.title, text_at(c.title))
    } else {
        let Some(i) = l.recs.iter().position(|&x| x == r) else { return 0 };
        let Some(p) = l.placed.iter().find(|p| p.index == i) else { return 0 };
        let tr = to_window(c, l.parts.area, l.view, p.text);
        (Rect::new(tr.left, tr.bottom - 2, (tr.left + 160).max(tr.right), tr.top + 2), record_text(h, c, r))
    };
    let begin: [u32; 7] = [28, h, r, field, 0, 0, CID_MLE];
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CN_BEGINEDIT), begin.as_ptr() as u32);
    // An entry field (id CID_MLE, as OS/2's MLE) over the text: Enter
    // keeps the new text, Esc drops it.
    let e = win::create(h, b"#6", &text, ws::VISIBLE | 0x0008, Rect::new(rect.left, rect.bottom, rect.right, rect.top), h, CID_MLE, 0);
    if e == 0 {
        return 0;
    }
    // EM_SETTEXTLIMIT, EM_SETSEL (all).
    send(e, 0x0143, 1024, 0);
    win::set_text(e, &text);
    send(e, 0x0142, mp(0, text.len() as u32), 0);
    if let Some(c) = data(h) {
        c.edit = (e, r, field);
    }
    win::WinSetFocus(1, e);
    1
}

/// End direct editing: with `keep` the text goes into the record (the
/// owner reallocates the string with CN_REALLOCPSZ if it wants).
fn close_edit(h: u32, keep: bool) -> u32 {
    let Some(c) = data(h) else { return 0 };
    let (e, r, field) = c.edit;
    if e == 0 {
        return 0;
    }
    c.edit = (0, 0, 0);
    let text = ctl::text(e);
    win::destroy(e);
    win::WinSetFocus(1, h);
    if keep {
        let c = data(h).unwrap();
        // Where the text lives: the record's string for the view, a
        // detail field's string, or the container title.
        let pp: u32 = if r == 0 {
            0
        } else if field != 0 {
            r + rd(field, F_OFF)
        } else {
            let off = match (full(h, r), view(c)) {
                (false, _) | (true, View::Icon) => R_PSZICON,
                (true, View::Name) => R_PSZNAME,
                (true, View::Text) => R_PSZTEXT,
                (true, View::Tree) => R_PSZTREE,
                (true, View::Detail) => R_PSZICON,
            };
            r + off
        };
        let mut ppsz = pp;
        let mut title_ptr = c.title;
        if r == 0 {
            ppsz = &mut title_ptr as *mut u32 as u32;
        }
        let ed: [u32; 7] = [28, h, r, field, ppsz, text.len() as u32 + 1, CID_MLE];
        // A non-zero answer to CN_REALLOCPSZ lets the text be copied.
        if send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CN_REALLOCPSZ), ed.as_ptr() as u32) != 0 {
            let dst = rd(ppsz, 0);
            if dst != 0 {
                unsafe {
                    core::ptr::copy_nonoverlapping(text.as_ptr(), dst as *mut u8, text.len());
                    *((dst as usize + text.len()) as *mut u8) = 0;
                }
            }
        }
        let end: [u32; 7] = [28, h, r, field, ppsz, text.len() as u32 + 1, CID_MLE];
        send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CN_ENDEDIT), end.as_ptr() as u32);
    }
    ctl::invalidate(h);
    1
}

// ---- Mouse ----

/// The record (and whether its tree button) at window point `p`.
fn record_at(h: u32, p: Point) -> Option<(u32, bool, Rect)> {
    let l = compute(h)?;
    if !l.parts.area.contains(p) {
        return None;
    }
    let c = data(h)?;
    let w = to_work(c, l.parts.area, l.view, p);
    if l.view == View::Tree {
        if let Some(b) = l.placed.iter().find(|x| !x.button.is_empty() && x.button.contains(w)) {
            return Some((l.recs[b.index], true, to_window(c, l.parts.area, l.view, b.rect)));
        }
    }
    let i = layout::hit(&l.placed, w.x, w.y, l.view == View::Icon)?;
    Some((l.recs[l.placed[i].index], false, to_window(c, l.parts.area, l.view, l.placed[i].rect)))
}

fn toggle_tree(h: u32, r: u32) {
    let expanded = attr(r) & CRA_EXPANDED != 0;
    set_attr(r, attr(r) & !(CRA_EXPANDED | CRA_COLLAPSED) | if expanded { CRA_COLLAPSED } else { CRA_EXPANDED });
    ctl::invalidate(h);
    sync_scroll(h);
    notify_rec(h, if expanded { CN_COLLAPSETREE } else { CN_EXPANDTREE }, r);
}

fn button1(h: u32, mp1: u32, mp2: u32, dbl: bool) {
    let kc = mp2 >> 16;
    let (shift, ctrl, alt) = (kc & 0x08 != 0, kc & 0x10 != 0, kc & 0x20 != 0);
    if data(h).is_some_and(|c| c.edit.0 != 0) {
        close_edit(h, true);
    }
    if !ctl::has_focus(h) {
        win::WinSetFocus(1, h);
    }
    let p = ctl::mouse_pos(mp1);
    match record_at(h, p) {
        Some((r, true, _)) => {
            toggle_tree(h, r);
        }
        Some((r, false, _)) => {
            if attr(r) & CRA_DISABLED != 0 {
                return;
            }
            if alt && !dbl {
                let field = data(h).filter(|c| view(c) == View::Detail).map_or(0, |c| columns(c).first().copied().unwrap_or(0));
                open_edit(h, r, field);
                return;
            }
            if dbl {
                pick(h, r, Pick::Click { ctrl: false, shift: false });
                notify_enter(h, r, false);
            } else {
                pick(h, r, Pick::Click { ctrl, shift });
            }
        }
        None => {
            if parts(h).title.contains(p) && alt {
                open_edit(h, 0, 0);
                return;
            }
            if dbl {
                notify_enter(h, 0, false);
            } else if SelMode::from_style(ctl::style(h)) != SelMode::Multiple && !ctrl {
                deselect_all(h, true);
            }
        }
    }
}

/// Button 2 released: the end of moving icons. A click (no move) comes
/// back as WM_CONTEXTMENU, which PM makes of WM_BUTTON2CLICK.
fn button2_up(h: u32) {
    let Some((_, _, moved)) = data(h).and_then(|c| c.b2.take()) else { return };
    if st().capture == h {
        st().capture = 0;
    }
    if moved {
        sync_scroll(h);
    }
}

fn button2_down(h: u32, mp1: u32) {
    if !ctl::has_focus(h) {
        win::WinSetFocus(1, h);
    }
    let p = ctl::mouse_pos(mp1);
    let r = record_at(h, p).filter(|x| !x.1).map_or(0, |x| x.0);
    if let Some(c) = data(h) {
        c.b2 = Some((r, p, false));
    }
    st().capture = h;
}

/// Button 2 held and moved over a record: a drag. The owner hears
/// CN_INITDRAG; if it does not take the drag (returns 0) the icon view
/// moves the record (and the other selected ones) itself.
fn button2_move(h: u32, mp1: u32) {
    let Some((r, start, moved)) = data(h).and_then(|c| c.b2) else { return };
    let p = ctl::mouse_pos(mp1);
    let (dx, dy) = (p.x - start.x, p.y - start.y);
    if !moved && dx.abs() < 4 && dy.abs() < 4 {
        return;
    }
    if r == 0 {
        if let Some(c) = data(h) {
            c.b2 = Some((r, start, true));
        }
        return;
    }
    if !moved {
        let o = st().desk.origin(h);
        let init: [u32; 6] = [h, r, (start.x + o.x) as u32, (start.y + o.y) as u32, 0, 0];
        let taken = send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CN_INITDRAG), init.as_ptr() as u32);
        let Some(c) = data(h) else { return };
        if taken != 0 || view(c) != View::Icon {
            c.b2 = None;
            if st().capture == h {
                st().capture = 0;
            }
            return;
        }
    }
    let Some(c) = data(h) else { return };
    let group: Vec<u32> = if attr(r) & CRA_SELECTED != 0 { c.top.iter().copied().filter(|&x| attr(x) & CRA_SELECTED != 0).collect() } else { alloc::vec![r] };
    for g in group {
        wr(g, R_X, (rd(g, R_X) as i32 + dx) as u32);
        wr(g, R_Y, (rd(g, R_Y) as i32 + dy) as u32);
        // Moved icons come to the top.
        c.z.retain(|&x| x != g);
        c.z.push(g);
    }
    c.b2 = Some((r, p, true));
    ctl::invalidate(h);
}

// ---- Keyboard ----

fn key(h: u32, mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.up() {
        return false;
    }
    let Some(c) = data(h) else { return false };
    if c.edit.0 != 0 {
        match k.vk {
            VK_NEWLINE | VK_ENTER => {
                close_edit(h, true);
                return true;
            }
            VK_ESC => {
                close_edit(h, false);
                return true;
            }
            _ => return false,
        }
    }
    let cur = if contains(c, c.cursor) { c.cursor } else { 0 };
    let order = item_order(c);
    let first = order.first().copied().unwrap_or(0);
    let mode = SelMode::from_style(ctl::style(h));
    let mv = |h: u32, r: u32| {
        if r != 0 {
            if mode == SelMode::Multiple {
                set_cursor(h, r, true);
                reveal(h, r);
            } else {
                pick(h, r, Pick::Move { shift: k.shift(), ctrl: k.ctrl() });
            }
        }
    };
    match k.vk {
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
            if cur == 0 {
                mv(h, first);
                return true;
            }
            let Some(l) = compute(h) else { return true };
            // The tree: left and right collapse and expand.
            if l.view == View::Tree && matches!(k.vk, VK_LEFT | VK_RIGHT) {
                let has_kids = data(h).is_some_and(|c| c.kids.get(&cur).is_some_and(|v| !v.is_empty()));
                let expanded = attr(cur) & CRA_EXPANDED != 0;
                if k.vk == VK_RIGHT && has_kids && !expanded || k.vk == VK_LEFT && has_kids && expanded {
                    toggle_tree(h, cur);
                } else if k.vk == VK_LEFT {
                    let p = data(h).and_then(|c| c.parent.get(&cur).copied()).unwrap_or(0);
                    mv(h, p);
                }
                return true;
            }
            let Some(i) = l.placed.iter().position(|p| l.recs[p.index] == cur) else { return true };
            let d = match k.vk {
                VK_LEFT => Dir::Left,
                VK_RIGHT => Dir::Right,
                VK_UP => Dir::Up,
                _ => Dir::Down,
            };
            if let Some(n) = layout::neighbour(&l.placed, i, d) {
                mv(h, l.recs[l.placed[n].index]);
            }
            true
        }
        VK_HOME => {
            mv(h, first);
            true
        }
        VK_END => {
            mv(h, order.last().copied().unwrap_or(0));
            true
        }
        VK_PAGEUP | VK_PAGEDOWN => {
            let rows = parts(h).area.height() / layout::row_height(&metrics(h, data(h).unwrap(), parts(h).area)).max(1);
            let i = order.iter().position(|&x| x == cur).unwrap_or(0) as i32;
            let j = if k.vk == VK_PAGEUP { i - rows.max(1) } else { i + rows.max(1) };
            mv(h, order.get(j.clamp(0, order.len() as i32 - 1).max(0) as usize).copied().unwrap_or(0));
            true
        }
        VK_SPACE if cur != 0 => {
            pick(h, cur, Pick::Toggle);
            true
        }
        VK_NEWLINE | VK_ENTER => {
            notify_enter(h, cur, true);
            true
        }
        // Shift+F10: the context menu.
        0x29 if k.shift() => {
            notify_rec(h, CN_CONTEXTMENU, cur);
            true
        }
        // F1: help.
        0x20 => {
            notify_rec(h, CN_HELP, cur);
            true
        }
        _ => {
            // Ctrl+/ selects everything, Ctrl+\ nothing; + and - expand
            // and collapse a tree record.
            if k.ctrl() && k.ch == b'/' && mode != SelMode::Single {
                for r in order {
                    emphasize(h, r, CRA_SELECTED, true, true);
                }
                return true;
            }
            if k.ctrl() && k.ch == b'\\' {
                deselect_all(h, true);
                return true;
            }
            if (k.ch == b'+' || k.ch == b'-') && cur != 0 && data(h).is_some_and(|c| view(c) == View::Tree) {
                let expanded = attr(cur) & CRA_EXPANDED != 0;
                if expanded == (k.ch == b'-') {
                    toggle_tree(h, cur);
                }
                return true;
            }
            false
        }
    }
}

// ---- Queries ----

/// CM_QUERYRECORD: the record relative to `r` (CMA_FIRST, CMA_LAST:
/// relative to the container; CMA_NEXT, CMA_PREV, CMA_PARENT,
/// CMA_FIRSTCHILD, CMA_LASTCHILD: to `r`), in item or z-order.
fn query_record(h: u32, r: u32, cmd: u32, z: bool) -> u32 {
    let Some(c) = data(h) else { return 0 };
    let siblings = |r: u32| -> &Vec<u32> {
        match c.parent.get(&r) {
            Some(p) => c.kids.get(p).unwrap(),
            None => {
                if z {
                    &c.z
                } else {
                    &c.top
                }
            }
        }
    };
    let list = if z { &c.z } else { &c.top };
    match cmd {
        CMA_FIRST => list.first().copied().unwrap_or(0),
        CMA_LAST => list.last().copied().unwrap_or(0),
        CMA_NEXT | CMA_PREV if contains(c, r) => {
            let s = siblings(r);
            let i = s.iter().position(|&x| x == r).unwrap_or(0);
            if cmd == CMA_NEXT { s.get(i + 1).copied().unwrap_or(0) } else { i.checked_sub(1).and_then(|j| s.get(j).copied()).unwrap_or(0) }
        }
        CMA_PARENT => c.parent.get(&r).copied().unwrap_or(0),
        CMA_FIRSTCHILD => c.kids.get(&r).and_then(|v| v.first().copied()).unwrap_or(0),
        CMA_LASTCHILD => c.kids.get(&r).and_then(|v| v.last().copied()).unwrap_or(0),
        _ => 0,
    }
}

/// CM_QUERYRECORDRECT: QUERYRECORDRECT {cb, pRecord, fRightSplitWindow,
/// fsExtent}; the rectangle in container window coordinates.
fn record_rect(h: u32, prcl: *mut crate::gpiapi::RectL, q: *const u32) -> u32 {
    if prcl.is_null() || q.is_null() {
        return 0;
    }
    let (r, ext) = unsafe { (q.add(1).read_unaligned(), q.add(3).read_unaligned()) };
    let Some(l) = compute(h) else { return 0 };
    let Some(i) = l.recs.iter().position(|&x| x == r) else { return 0 };
    let Some(p) = l.placed.iter().find(|p| p.index == i) else { return 0 };
    let c = data(h).unwrap();
    let w = if ext & (CMA_ICON | CMA_TEXT) == CMA_ICON && !p.icon.is_empty() {
        p.icon
    } else if ext & (CMA_ICON | CMA_TEXT) == CMA_TEXT {
        p.text
    } else {
        p.rect
    };
    unsafe { prcl.write_unaligned(crate::gpiapi::RectL::from(to_window(c, l.parts.area, l.view, w))) };
    1
}

/// CM_QUERYRECORDFROMRECT: QUERYRECFROMRECT {cb, rect, fsSearch}; the
/// first record after `after` in the rectangle (window coordinates).
fn record_from_rect(h: u32, after: u32, q: *const u32) -> u32 {
    if q.is_null() {
        return 0;
    }
    let v: [i32; 6] = core::array::from_fn(|i| unsafe { q.add(i).read_unaligned() } as i32);
    let rect = Rect::new(v[1], v[2], v[3], v[4]);
    let fs = v[5] as u32;
    let Some(l) = compute(h) else { return 0 };
    let c = data(h).unwrap();
    let order = item_order(c);
    let mut start = if after == CMA_FIRST || after == 0 { 0 } else { order.iter().position(|&x| x == after).map_or(order.len(), |i| i + 1) };
    while start < order.len() {
        let r = order[start];
        if let Some(i) = l.recs.iter().position(|&x| x == r) {
            if let Some(p) = l.placed.iter().find(|p| p.index == i) {
                let w = to_window(c, l.parts.area, l.view, p.rect);
                let hit = if fs & CMA_COMPLETE != 0 { w.intersect(&rect) == w } else { !w.intersect(&rect).is_empty() };
                if hit {
                    return r;
                }
            }
        }
        start += 1;
    }
    let _ = CMA_PARTIAL;
    0
}

/// CNRINFO (92 bytes): cb, pSortRecord, pFieldInfoLast,
/// pFieldInfoObject, pszCnrTitle, flWindowAttr, ptlOrigin, cDelta,
/// cRecords, slBitmapOrIcon, slTreeBitmapOrIcon, hbmExpanded,
/// hbmCollapsed, hptrExpanded, hptrCollapsed, cyLineSpacing,
/// cxTreeIndent, cxTreeLine, cFields, xVertSplitbar.
fn query_info(h: u32, p: *mut u32, cb: u32) -> u32 {
    let Some(c) = data(h) else { return 0 };
    let v: [u32; 23] = [
        92,
        c.sort,
        c.field_last,
        c.field_object,
        c.title,
        c.attr,
        c.org.0 as u32,
        c.org.1 as u32,
        c.delta,
        all(c).len() as u32,
        c.icon_size.0 as u32,
        c.icon_size.1 as u32,
        c.tree_icon.0 as u32,
        c.tree_icon.1 as u32,
        0,
        0,
        0,
        0,
        c.line_spacing as u32,
        c.tree_indent as u32,
        c.tree_line as u32,
        c.fields.len() as u32,
        c.split as u32,
    ];
    if p.is_null() {
        return 0;
    }
    let n = (cb as usize / 4).min(23);
    for (i, x) in v.iter().take(n).enumerate() {
        unsafe { p.add(i).write_unaligned(*x) };
    }
    (n * 4) as u32
}

fn set_info(h: u32, p: *const u32, fl: u32) -> u32 {
    if p.is_null() {
        return 0;
    }
    let v: [u32; 23] = core::array::from_fn(|i| unsafe { p.add(i).read_unaligned() });
    let Some(c) = data(h) else { return 0 };
    let old_view = view(c);
    if fl & CMA_PSORTRECORD != 0 {
        c.sort = v[1];
    }
    if fl & CMA_PFIELDINFOLAST != 0 {
        c.field_last = v[2];
    }
    if fl & CMA_PFIELDINFOOBJECT != 0 {
        c.field_object = v[3];
    }
    if fl & CMA_CNRTITLE != 0 {
        c.title = v[4];
    }
    if fl & CMA_FLWINDOWATTR != 0 {
        c.attr = v[5];
    }
    if fl & CMA_PTLORIGIN != 0 {
        c.org = (v[6] as i32, v[7] as i32);
    }
    if fl & CMA_DELTA != 0 {
        c.delta = v[8];
    }
    if fl & CMA_SLBITMAPORICON != 0 {
        c.icon_size = (v[10] as i32, v[11] as i32);
    }
    if fl & CMA_SLTREEBITMAPORICON != 0 {
        c.tree_icon = (v[12] as i32, v[13] as i32);
    }
    let _ = CMA_TREEICON;
    if fl & CMA_LINESPACING != 0 {
        c.line_spacing = v[18] as i32;
    }
    if fl & CMA_CXTREEINDENT != 0 {
        c.tree_indent = v[19] as i32;
    }
    if fl & CMA_CXTREELINE != 0 {
        c.tree_line = v[20] as i32;
    }
    if fl & CMA_XVERTSPLITBAR != 0 {
        c.split = v[22] as i32;
    }
    if view(c) != old_view {
        c.scroll = (0, 0);
    }
    let sorted = fl & CMA_PSORTRECORD != 0 && c.sort != 0;
    let rearrange = ctl::style(h) & CCS_AUTOPOSITION != 0 && fl & (CMA_FLWINDOWATTR | CMA_CNRTITLE | CMA_SLBITMAPORICON) != 0;
    let sort_fn = c.sort;
    if sorted {
        sort(h, sort_fn, 0);
    } else if rearrange {
        arrange(h);
    }
    ctl::invalidate(h);
    sync_scroll(h);
    1
}

fn search(h: u32, ps: *const u32, after: u32) -> u32 {
    if ps.is_null() {
        return 0;
    }
    // SEARCHSTRING: cb, pszSearch, fsPrefix, fsCaseSensitive, usView.
    let v: [u32; 5] = core::array::from_fn(|i| unsafe { ps.add(i).read_unaligned() });
    let pat = text_at(v[1]);
    let (prefix, case) = (v[2] != 0, v[3] != 0);
    let Some(c) = data(h) else { return 0 };
    let saved = c.attr;
    c.attr = (c.attr & !0x7f) | (v[4] & 0x7f);
    let order: Vec<u32> = all(c);
    let start = if after == CMA_FIRST || after == 0 { 0 } else { order.iter().position(|&x| x == after).map_or(order.len(), |i| i + 1) };
    let fold = |b: &[u8]| -> Vec<u8> { if case { b.to_vec() } else { b.iter().map(|c| c.to_ascii_uppercase()).collect() } };
    let p = fold(&pat);
    let mut found = 0;
    for &r in &order[start.min(order.len())..] {
        let t = fold(&record_text(h, data(h).unwrap(), r));
        let hit = if prefix { t.starts_with(&p) } else { t.windows(p.len().max(1)).any(|w| w == &p[..]) || p.is_empty() };
        if hit {
            found = r;
            break;
        }
    }
    if let Some(c) = data(h) {
        c.attr = saved;
    }
    found
}

fn record_array(p: u32, n: usize) -> Vec<u32> {
    (0..n).map(|i| rd(p, i as u32 * 4)).filter(|&r| r != 0).collect()
}

pub extern "C" fn container_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            let vs = win::create(h, b"#8", b"", 1, Rect::default(), h, CID_VSCROLL, 0);
            let hs = win::create(h, b"#8", b"", 0, Rect::default(), h, CID_HSCROLL, 0);
            ctl::set_data(h, Cnr {
                attr: layout::CV_ICON,
                title: 0,
                sort: 0,
                field_last: 0,
                field_object: 0,
                delta: 0,
                icon_size: (0, 0),
                tree_icon: (0, 0),
                line_spacing: 0,
                tree_indent: 0,
                tree_line: 0,
                split: -1,
                org: (0, 0),
                scroll: (0, 0),
                top: Vec::new(),
                kids: BTreeMap::new(),
                parent: BTreeMap::new(),
                z: Vec::new(),
                fields: Vec::new(),
                cursor: 0,
                anchor: 0,
                vs,
                hs,
                b2: None,
                edit: (0, 0, 0),
                drop_on: 0,
            });
            sync_scroll(h);
            0
        }
        WM_DESTROY => 0,
        WM_SIZE => {
            if let Some(c) = data(h) {
                let e = c.edit.0;
                if e != 0 {
                    close_edit(h, false);
                }
            }
            if ctl::style(h) & CCS_AUTOPOSITION != 0 {
                arrange(h);
            }
            sync_scroll(h);
            ctl::invalidate(h);
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN => {
            if ctl::enabled(h) {
                button1(h, mp1, mp2, false);
            }
            1
        }
        WM_BUTTON1DBLCLK => {
            if ctl::enabled(h) {
                button1(h, mp1, mp2, true);
            }
            1
        }
        WM_BUTTON1UP => 1,
        WM_BUTTON2DOWN => {
            if ctl::enabled(h) {
                button2_down(h, mp1);
            }
            1
        }
        WM_BUTTON2UP => {
            button2_up(h);
            1
        }
        WM_MOUSEMOVE => {
            if data(h).is_some_and(|c| c.b2.is_some()) {
                button2_move(h, mp1);
            }
            crate::pointer::set_system(pmwin::sysptr::SPTR_ARROW);
            1
        }
        DM_DRAGOVER | DM_DRAGLEAVE | DM_DROP | DM_DROPHELP => drag_msg(h, msg, mp1, mp2),
        // From the pointer (button 2 clicked): the record under it, or the
        // container itself over empty space.
        WM_CONTEXTMENU => {
            let r = record_at(h, ctl::mouse_pos(mp1)).filter(|x| !x.1).map_or(0, |x| x.0);
            notify_rec(h, CN_CONTEXTMENU, r);
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
            if data(h).is_some_and(|c| c.cursor == 0) {
                let first = data(h).map(|c| item_order(c).first().copied().unwrap_or(0)).unwrap_or(0);
                if first != 0 {
                    set_cursor(h, first, false);
                }
            }
            send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), if mp2 & 0xffff != 0 { CN_SETFOCUS } else { CN_KILLFOCUS }), h);
            0
        }
        WM_VSCROLL | WM_HSCROLL => {
            let Some(l) = compute(h) else { return 0 };
            let c = data(h).unwrap();
            let a = l.parts.area;
            let vp = viewport(c, a, l.view);
            let ext = layout::union(&l.bounds, &vp);
            let line = layout::row_height(&l.m).max(8);
            let vertical = msg == WM_VSCROLL;
            let (page, cur) = if vertical { (a.height(), ext.top - vp.top) } else { (a.width(), vp.left - ext.left) };
            let pos = lo(mp2) as i16 as i32;
            let d = match hi(mp2) {
                1 => -line,
                2 => line,
                3 => -page,
                4 => page,
                5 | 6 => pos - cur,
                _ => 0,
            };
            if vertical { scroll_by(h, 0, d) } else { scroll_by(h, d, 0) };
            0
        }
        WM_CONTROL if lo(mp1) == CID_MLE => 0,
        WM_QUERYDLGCODE => 0,
        CM_ALLOCRECORD => {
            let core = if ctl::style(h) & CCS_MINIRECORDCORE != 0 { MINIRECORDCORE } else { RECORDCORE };
            let n = (mp2 & 0xffff).max(1);
            let v = alloc_blocks(n, core + mp1, MAGIC_REC);
            if v.len() != n as usize {
                for r in v {
                    free_block(r, MAGIC_REC);
                }
                return 0;
            }
            for (i, &r) in v.iter().enumerate() {
                wr(r, R_CB, core);
                wr(r, R_NEXT, v.get(i + 1).copied().unwrap_or(0));
            }
            v[0]
        }
        CM_FREERECORD => {
            let recs = record_array(mp1, (mp2 & 0xffff) as usize);
            let c = data(h);
            let inserted = c.is_some_and(|c| recs.iter().any(|&r| contains(c, r)));
            if inserted {
                return 0;
            }
            recs.iter().all(|&r| free_block(r, MAGIC_REC)) as u32
        }
        CM_INSERTRECORD => {
            // A chain linked by preccNextRecord, cRecordsInsert long.
            let n = if mp2 != 0 { rd(mp2, 20) } else { 0 };
            let mut recs = Vec::new();
            let mut r = mp1;
            while r != 0 && (recs.len() as u32) < n.max(1) {
                recs.push(r);
                r = rd(r, R_NEXT);
            }
            insert(h, &recs, mp2 as *const u32)
        }
        CM_INSERTRECORDARRAY => {
            let n = if mp2 != 0 { rd(mp2, 20) } else { 0 };
            let recs = record_array(mp1, n as usize);
            insert(h, &recs, mp2 as *const u32)
        }
        CM_REMOVERECORD => {
            let (n, fl) = (lo(mp2) as usize, hi(mp2));
            let recs = if n == 0 { data(h).map_or(Vec::new(), |c| c.top.clone()) } else { record_array(mp1, n) };
            if data(h).is_some_and(|c| recs.iter().any(|&r| !contains(c, r))) {
                return u32::MAX;
            }
            if data(h).is_some_and(|c| c.edit.1 != 0 && recs.contains(&c.edit.1)) {
                close_edit(h, false);
            }
            for r in recs {
                remove_one(h, r, fl & CMA_FREE != 0);
            }
            if fl & CMA_INVALIDATE != 0 {
                ctl::invalidate(h);
                sync_scroll(h);
            }
            data(h).map_or(0, |c| all(c).len() as u32)
        }
        CM_QUERYRECORD => query_record(h, mp1, lo(mp2), hi(mp2) & CMA_ZORDER != 0),
        CM_QUERYRECORDEMPHASIS => {
            let Some(c) = data(h) else { return 0 };
            let order = all(c);
            let start = if mp1 == CMA_FIRST || mp1 == 0 { 0 } else { order.iter().position(|&x| x == mp1).map_or(order.len(), |i| i + 1) };
            let want = mp2 & 0xffff;
            order[start.min(order.len())..].iter().copied().find(|&r| attr(r) & want == want && want != 0).unwrap_or(0)
        }
        CM_SETRECORDEMPHASIS => {
            let (on, bits) = (lo(mp2) != 0, hi(mp2));
            let Some(c) = data(h) else { return 0 };
            if mp1 != 0 && !contains(c, mp1) {
                return 0;
            }
            if mp1 == 0 {
                // The container itself (source emphasis on the background).
                ctl::invalidate(h);
                return 1;
            }
            if bits & CRA_CURSORED != 0 && on {
                set_cursor(h, mp1, false);
            } else if bits & CRA_CURSORED != 0 {
                emphasize(h, mp1, CRA_CURSORED, false, false);
                if let Some(c) = data(h) {
                    if c.cursor == mp1 {
                        c.cursor = 0;
                    }
                }
            }
            let rest = bits & !CRA_CURSORED;
            if rest & CRA_SELECTED != 0 && on && SelMode::from_style(ctl::style(h)) == SelMode::Single {
                deselect_all(h, false);
            }
            if rest != 0 {
                emphasize(h, mp1, rest, on, false);
            }
            1
        }
        CM_SETCNRINFO => set_info(h, mp1 as *const u32, mp2),
        CM_QUERYCNRINFO => query_info(h, mp1 as *mut u32, mp2 & 0xffff),
        CM_ARRANGE => {
            arrange(h);
            1
        }
        CM_INVALIDATERECORD => {
            let (n, fl) = (lo(mp2) as usize, hi(mp2));
            let _ = (n, fl & (CMA_REPOSITION | CMA_TEXTCHANGED));
            ctl::invalidate(h);
            sync_scroll(h);
            1
        }
        CM_ERASERECORD | CM_INVALIDATEDETAILFIELDINFO | CM_QUERYRECORDINFO => {
            ctl::invalidate(h);
            1
        }
        CM_PAINTBACKGROUND => 0,
        CM_ALLOCDETAILFIELDINFO => {
            let n = (mp1 & 0xffff).max(1);
            let v = alloc_blocks(n, FIELDINFO, MAGIC_FLD);
            if v.len() != n as usize {
                for f in v {
                    free_block(f, MAGIC_FLD);
                }
                return 0;
            }
            for (i, &f) in v.iter().enumerate() {
                wr(f, 0, FIELDINFO);
                wr(f, F_NEXT, v.get(i + 1).copied().unwrap_or(0));
            }
            v[0]
        }
        CM_INSERTDETAILFIELDINFO => {
            // FIELDINFOINSERT: cb, pFieldInfoOrder, fInvalidateFieldInfo,
            // cFieldInfoInsert.
            if mp2 == 0 {
                return 0;
            }
            let (order, inval, n) = (rd(mp2, 4), rd(mp2, 8), rd(mp2, 12));
            let mut v = Vec::new();
            let mut f = mp1;
            while f != 0 && (v.len() as u32) < n.max(1) {
                v.push(f);
                f = rd(f, F_NEXT);
            }
            let Some(c) = data(h) else { return 0 };
            let at = match order {
                CMA_FIRST => 0,
                CMA_END | CMA_LAST | 0 => c.fields.len(),
                after => c.fields.iter().position(|&x| x == after).map_or(c.fields.len(), |i| i + 1),
            };
            for (k, f) in v.into_iter().enumerate() {
                if !c.fields.contains(&f) {
                    c.fields.insert(at + k, f);
                }
            }
            // The list stays linked in order.
            for i in 0..c.fields.len() {
                wr(c.fields[i], F_NEXT, c.fields.get(i + 1).copied().unwrap_or(0));
            }
            let n = c.fields.len() as u32;
            if inval != 0 {
                ctl::invalidate(h);
                sync_scroll(h);
            }
            n
        }
        CM_QUERYDETAILFIELDINFO => {
            let Some(c) = data(h) else { return 0 };
            match mp2 & 0xffff {
                CMA_FIRST => c.fields.first().copied().unwrap_or(0),
                CMA_LAST => c.fields.last().copied().unwrap_or(0),
                CMA_NEXT | CMA_PREV => {
                    let Some(i) = c.fields.iter().position(|&x| x == mp1) else { return 0 };
                    if mp2 & 0xffff == CMA_NEXT { c.fields.get(i + 1).copied().unwrap_or(0) } else { i.checked_sub(1).map_or(0, |j| c.fields[j]) }
                }
                _ => 0,
            }
        }
        CM_REMOVEDETAILFIELDINFO => {
            let (n, fl) = (lo(mp2) as usize, hi(mp2));
            let Some(c) = data(h) else { return u32::MAX };
            let v = if n == 0 { c.fields.clone() } else { record_array(mp1, n) };
            c.fields.retain(|f| !v.contains(f));
            for i in 0..c.fields.len() {
                wr(c.fields[i], F_NEXT, c.fields.get(i + 1).copied().unwrap_or(0));
            }
            let left = c.fields.len() as u32;
            if fl & CMA_FREE != 0 {
                for f in v {
                    free_block(f, MAGIC_FLD);
                }
            }
            if fl & CMA_INVALIDATE != 0 {
                ctl::invalidate(h);
                sync_scroll(h);
            }
            left
        }
        CM_FREEDETAILFIELDINFO => {
            let v = record_array(mp1, (mp2 & 0xffff) as usize);
            if data(h).is_some_and(|c| v.iter().any(|f| c.fields.contains(f))) {
                return 0;
            }
            v.iter().all(|&f| free_block(f, MAGIC_FLD)) as u32
        }
        CM_SORTRECORD => sort(h, mp1, mp2),
        CM_QUERYRECORDRECT => record_rect(h, mp1 as *mut crate::gpiapi::RectL, mp2 as *const u32),
        CM_QUERYRECORDFROMRECT => record_from_rect(h, mp1, mp2 as *const u32),
        CM_QUERYVIEWPORTRECT => {
            let Some(l) = compute(h) else { return 0 };
            let c = data(h).unwrap();
            let r = if lo(mp2) == CMA_WINDOW { l.parts.area } else { viewport(c, l.parts.area, l.view) };
            let _ = CMA_WORKSPACE;
            if mp1 == 0 {
                return 0;
            }
            unsafe { (mp1 as *mut crate::gpiapi::RectL).write_unaligned(crate::gpiapi::RectL::from(r)) };
            1
        }
        CM_SCROLLWINDOW => {
            let n = mp2 as i32;
            if mp1 & CMA_VERTICAL != 0 {
                scroll_by(h, 0, n);
            } else {
                scroll_by(h, n, 0);
            }
            1
        }
        CM_SEARCHSTRING => search(h, mp1 as *const u32, mp2),
        CM_OPENEDIT => {
            if mp1 == 0 {
                return 0;
            }
            open_edit(h, rd(mp1, 8), rd(mp1, 12))
        }
        CM_CLOSEEDIT => close_edit(h, true),
        CM_EXPANDTREE | CM_COLLAPSETREE => {
            let expand = msg == CM_EXPANDTREE;
            let Some(c) = data(h) else { return 0 };
            let targets: Vec<u32> = if mp1 == 0 { all(c) } else { alloc::vec![mp1] };
            for r in targets {
                set_attr(r, attr(r) & !(CRA_EXPANDED | CRA_COLLAPSED) | if expand { CRA_EXPANDED } else { CRA_COLLAPSED });
                notify_rec(h, if expand { CN_EXPANDTREE } else { CN_COLLAPSETREE }, r);
            }
            ctl::invalidate(h);
            sync_scroll(h);
            1
        }
        CM_FILTER => {
            // pfnFilter(pRecord, pStorage) -> BOOL: records it rejects are
            // filtered out (CRA_FILTERED); NULL shows them all again.
            let Some(c) = data(h) else { return 0 };
            let recs = all(c);
            if mp1 == 0 {
                for r in recs {
                    set_attr(r, attr(r) & !CRA_FILTERED);
                }
            } else {
                let f: extern "C" fn(u32, u32) -> u32 = unsafe { core::mem::transmute(mp1 as usize) };
                let keep: Vec<bool> = lock::unlocked(|| recs.iter().map(|&r| f(r, mp2) != 0).collect());
                for (r, k) in recs.into_iter().zip(keep) {
                    set_attr(r, if k { attr(r) & !CRA_FILTERED } else { attr(r) | CRA_FILTERED });
                }
            }
            ctl::invalidate(h);
            sync_scroll(h);
            1
        }
        CM_HORZSCROLLSPLITWINDOW => {
            scroll_by(h, mp2 as i32, 0);
            1
        }
        CM_MOVETREE | CM_SETTEXTVISIBILITY => 0,
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
