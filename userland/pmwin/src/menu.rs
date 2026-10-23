//! Menus (WC_MENU): menu bars, title-bar buttons (system menu, min/max)
//! and pull-down/pop-up menus, built from menu templates or MM_*
//! messages.
//!
//! Each submenu is its own hidden menu window, a child of the desktop,
//! shown below or beside its item while the menu is in use. Menu mode
//! (tracking) is driven by the menu messages: the root menu holds the
//! mouse capture and gets the queue's keys (the focus window keeps the
//! focus, as in PM), sends WM_INITMENU and WM_MENUSELECT to its
//! owner and posts WM_COMMAND (WM_SYSCOMMAND for MIS_SYSCOMMAND items,
//! WM_HELP for MIS_HELP) when an item is chosen.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::{Point, Rect};
use pmwin::{ws, HWND_DESKTOP};

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::frame::{self, *};
use crate::lock::wndproc;
use crate::msg::{post, send, WinDefWindowProc};
use crate::res::{self, MenuItemT};
use crate::state::st;
use crate::win::{self, WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_MOUSEMOVE, WM_PAINT, WM_SYSCOMMAND};

const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;

pub const MS_ACTIONBAR: u32 = 0x0001;
pub const MS_TITLEBUTTON: u32 = 0x0002;

const MIS_TEXT: u16 = 0x0001;
const MIS_BITMAP: u16 = 0x0002;
const MIS_SEPARATOR: u16 = 0x0004;
const MIS_SUBMENU: u16 = 0x0010;
const MIS_SYSCOMMAND: u16 = 0x0040;
const MIS_HELP: u16 = 0x0080;
const MIS_STATIC: u16 = 0x0100;
const MIA_CHECKED: u16 = 0x2000;
const MIA_DISABLED: u16 = 0x4000;
const MIA_HILITED: u16 = 0x8000;

const MM_INSERTITEM: u32 = 0x0180;
const MM_DELETEITEM: u32 = 0x0181;
const MM_QUERYITEM: u32 = 0x0182;
const MM_SETITEM: u32 = 0x0183;
const MM_QUERYITEMCOUNT: u32 = 0x0184;
const MM_STARTMENUMODE: u32 = 0x0185;
const MM_ENDMENUMODE: u32 = 0x0186;
const MM_REMOVEITEM: u32 = 0x0188;
const MM_SELECTITEM: u32 = 0x0189;
const MM_QUERYSELITEMID: u32 = 0x018a;
const MM_QUERYITEMTEXT: u32 = 0x018b;
const MM_QUERYITEMTEXTLENGTH: u32 = 0x018c;
const MM_SETITEMHANDLE: u32 = 0x018d;
const MM_SETITEMTEXT: u32 = 0x018e;
const MM_ITEMPOSITIONFROMID: u32 = 0x018f;
const MM_ITEMIDFROMPOSITION: u32 = 0x0190;
const MM_QUERYITEMATTR: u32 = 0x0191;
const MM_SETITEMATTR: u32 = 0x0192;
const MM_ISITEMVALID: u32 = 0x0193;
const MM_QUERYITEMRECT: u32 = 0x0194;
const MM_QUERYDEFAULTITEMID: u32 = 0x0431;
const MM_SETDEFAULTITEMID: u32 = 0x0432;

const MIT_END: i16 = -1;
const MIT_NONE: u32 = 0xffff;

const ITEM_H: i32 = 20;
const SEP_H: i32 = 8;

wndproc! {
    menu_entry => menu_proc;
}

pub struct Item {
    pub style: u16,
    pub attr: u16,
    pub id: u16,
    pub text: Vec<u8>,
    pub sub: u32,
    pub handle: u32,
}

#[derive(Default)]
pub struct Menu {
    pub items: Vec<Item>,
    /// Highlighted item.
    pub sel: Option<usize>,
    /// Submenu shown for the highlighted item.
    pub open: u32,
    pub default_id: u32,
}

/// Menu tracking: the root menu.
pub struct Mode {
    pub root: u32,
}

/// MENUITEM: iPosition, afStyle, afAttribute, id, hwndSubMenu, hItem.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    handle: u32,
}

fn data(h: u32) -> Option<&'static mut Menu> {
    ctl::data::<Menu>(h)
}

pub fn is_menu(h: u32) -> bool {
    data(h).is_some()
}

fn kind(h: u32) -> u32 {
    ctl::style(h) & (MS_ACTIONBAR | MS_TITLEBUTTON)
}

fn horizontal(h: u32) -> bool {
    kind(h) != 0
}

pub fn item_count(h: u32) -> i32 {
    data(h).map_or(0, |m| m.items.len() as i32)
}

// ---- Creation ----

/// Create a menu window with items from a template; submenus become
/// hidden menu windows owned by it.
fn create(parent: u32, owner: u32, style: u32, id: u32, items: &[MenuItemT]) -> u32 {
    let h = win::create(parent, b"#4", b"", style, Rect::default(), owner, id, 0);
    if h != 0 {
        fill(h, items);
    }
    h
}

fn fill(h: u32, items: &[MenuItemT]) {
    let mut v = Vec::new();
    for it in items {
        let sub = match &it.sub {
            Some(s) => create(HWND_DESKTOP, h, 0, it.id as u32, s),
            None => 0,
        };
        v.push(Item { style: it.style, attr: it.attr, id: it.id, text: it.text.clone(), sub, handle: 0 });
    }
    if let Some(m) = data(h) {
        m.items = v;
    }
}

fn text_item(style: u16, attr: u16, id: u32, text: &[u8]) -> MenuItemT {
    MenuItemT { style: MIS_TEXT | style, attr, id: id as u16, text: text.to_vec(), sub: None }
}

/// The system menu of a frame (FID_SYSMENU): one button whose submenu
/// has the window commands.
pub fn create_sysmenu(frame: u32, fcf: u32) {
    let dis = |on: bool| if on { 0 } else { MIA_DISABLED };
    let sep = || MenuItemT { style: MIS_SEPARATOR | MIS_SYSCOMMAND, attr: 0, id: 0xffff, text: Vec::new(), sub: None };
    let sc = MIS_SYSCOMMAND;
    let items = alloc::vec![
        text_item(sc, MIA_DISABLED, SC_RESTORE, b"~Restore\tAlt+F5"),
        text_item(sc, 0, SC_MOVE, b"~Move\tAlt+F7"),
        text_item(sc, dis(fcf & FCF_SIZEBORDER != 0), SC_SIZE, b"~Size\tAlt+F8"),
        text_item(sc, dis(fcf & FCF_MINBUTTON != 0), SC_MINIMIZE, b"Mi~nimize\tAlt+F9"),
        text_item(sc, dis(fcf & FCF_MAXBUTTON != 0), SC_MAXIMIZE, b"Ma~ximize\tAlt+F10"),
        text_item(sc, dis(fcf & FCF_HIDEBUTTON != 0), SC_HIDE, b"~Hide\tAlt+F11"),
        sep(),
        text_item(sc, 0, SC_CLOSE, b"~Close\tAlt+F4"),
        sep(),
        text_item(sc, 0, SC_TASKMANAGER, b"Window ~list\tCtrl+Esc"),
    ];
    let root = MenuItemT { style: MIS_BITMAP | MIS_SUBMENU | MIS_SYSCOMMAND, attr: 0, id: SC_SYSMENU as u16, text: Vec::new(), sub: Some(items) };
    create(frame, frame, ws::VISIBLE | MS_TITLEBUTTON, FID_SYSMENU, &[root]);
}

/// The min/max buttons of a frame (FID_MINMAX).
pub fn create_minmax(frame: u32, fcf: u32) {
    let mut items = Vec::new();
    for (flag, id) in [(FCF_HIDEBUTTON, SC_HIDE), (FCF_MINBUTTON, SC_MINIMIZE), (FCF_MAXBUTTON, SC_MAXIMIZE)] {
        if fcf & flag != 0 {
            items.push(MenuItemT { style: MIS_BITMAP | MIS_SYSCOMMAND, attr: 0, id: id as u16, text: Vec::new(), sub: None });
        }
    }
    create(frame, frame, ws::VISIBLE | MS_TITLEBUTTON, FID_MINMAX, &items);
}

/// WinLoadMenu(hwndFrame, hmod, idMenu): a menu bar when the parent is a
/// frame, otherwise a hidden pop-up menu.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadMenu(parent: u32, hmod: u32, id: u32) -> u32 {
    let Some(items) = res::load(hmod, res::RT_MENU, id).and_then(res::parse_menu) else { return 0 };
    let bar = frame::is_frame(parent);
    let style = if bar { ws::VISIBLE | MS_ACTIONBAR } else { 0 };
    let p = if parent == pmwin::HWND_OBJECT || parent == 0 { HWND_DESKTOP } else { parent };
    // The menu's ID is FID_MENU, whatever the resource's (as IBM's
    // PMWIN): a frame finds it so on WM_UPDATEFRAME.
    let h = create(p, parent, style, FID_MENU, &items);
    if bar {
        frame::layout(parent);
    }
    h
}

/// WinCreateMenu(hwndParent, pmt): a menu from a template in memory.
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateMenu(parent: u32, pmt: *const u8) -> u32 {
    if pmt.is_null() {
        return 0;
    }
    let cb = unsafe { (pmt as *const u16).read_unaligned() } as usize;
    let items = res::parse_menu(unsafe { core::slice::from_raw_parts(pmt, cb) }).unwrap_or_default();
    let bar = frame::is_frame(parent);
    let p = if parent == pmwin::HWND_OBJECT || parent == 0 { HWND_DESKTOP } else { parent };
    create(p, parent, if bar { ws::VISIBLE | MS_ACTIONBAR } else { 0 }, if bar { FID_MENU } else { 0 }, &items)
}

/// WinPopupMenu(hwndParent, hwndOwner, hwndMenu, x, y, idItem, fs): show
/// a pop-up menu at (x, y) of the parent and track it.
#[unsafe(no_mangle)]
pub extern "C" fn WinPopupMenu(parent: u32, owner: u32, menu: u32, x: i32, y: i32, id: u32, fs: u32) -> u32 {
    const PU_SELECTITEM: u32 = 0x0020;
    if !is_menu(menu) {
        return 0;
    }
    end(None, false);
    let o = st().desk.origin(if parent == 0 { HWND_DESKTOP } else { parent });
    // A menu loaded for a frame (WinLoadMenu with a dialog as the
    // parent) became an action bar there: as a pop-up it is a vertical
    // menu on the desktop.
    if horizontal(menu) || ctl::parent(menu) != HWND_DESKTOP {
        if let Some(w) = st().desk.get_mut(menu) {
            w.style &= !(MS_ACTIONBAR | MS_TITLEBUTTON | ws::VISIBLE);
        }
        st().desk.set_parent(menu, HWND_DESKTOP);
    }
    if let Some(w) = st().desk.get_mut(menu) {
        w.owner = owner;
    }
    let (cw, chh) = popup_size(menu);
    let (sw, sh) = (st().surface.width, st().surface.height);
    let left = (o.x + x).clamp(0, (sw - cw).max(0));
    let bottom = (o.y + y - chh).clamp(0, (sh - chh).max(0));
    st().desk.set_pos(menu, left, bottom, cw, chh);
    st().desk.to_top(menu);
    win::show(menu, true);
    let sel = if fs & PU_SELECTITEM != 0 { find(menu, id as u16, false).map(|(_, i)| i) } else { None };
    start(menu, sel, false);
    1
}

// ---- Layout ----

fn label(t: &[u8]) -> (&[u8], &[u8]) {
    match t.iter().position(|&c| c == b'\t') {
        Some(i) => (&t[..i], &t[i + 1..]),
        None => (t, &[]),
    }
}

/// Size of a pop-up menu window for its items.
fn popup_size(h: u32) -> (i32, i32) {
    let Some(m) = data(h) else { return (0, 0) };
    let (mut wl, mut wr, mut hh) = (0, 0, 6);
    for it in &m.items {
        if it.style & MIS_SEPARATOR != 0 {
            hh += SEP_H;
            continue;
        }
        let (l, r) = label(&it.text);
        wl = wl.max(draw::font_of(h).text_width(&draw::strip_mnemonic(l).0));
        wr = wr.max(draw::font_of(h).text_width(r));
        hh += ITEM_H;
    }
    (6 + 22 + wl + if wr > 0 { 24 + wr } else { 0 } + 22, hh)
}

/// Item rectangles in window coordinates.
fn rects(h: u32) -> Vec<Rect> {
    let Some(m) = data(h) else { return Vec::new() };
    let (cx, cy) = ctl::size(h);
    let mut v = Vec::with_capacity(m.items.len());
    match kind(h) {
        MS_TITLEBUTTON => {
            for i in 0..m.items.len() as i32 {
                v.push(Rect::new(i * cy, 0, (i + 1) * cy, cy));
            }
        }
        MS_ACTIONBAR => {
            let mut x = 2;
            for it in &m.items {
                let w = if it.style & MIS_SEPARATOR != 0 { 8 } else { draw::font_of(h).text_width(&draw::strip_mnemonic(&it.text).0) + 16 };
                v.push(Rect::new(x, 0, x + w, cy));
                x += w;
            }
        }
        _ => {
            let mut top = cy - 3;
            for it in &m.items {
                let ih = if it.style & MIS_SEPARATOR != 0 { SEP_H } else { ITEM_H };
                v.push(Rect::new(3, top - ih, cx - 3, top));
                top -= ih;
            }
        }
    }
    v
}

/// The item at a window point.
fn item_at(h: u32, p: Point) -> Option<usize> {
    rects(h).iter().position(|r| r.contains(p))
}

/// The id of the item at `p` (window coordinates) of a menu.
pub fn item_id_at(h: u32, p: Point) -> Option<u32> {
    let i = item_at(h, p)?;
    data(h)?.items.get(i).map(|it| it.id as u32)
}

fn selectable(it: &Item) -> bool {
    it.style & (MIS_SEPARATOR | MIS_STATIC) == 0
}

// ---- Painting ----

fn paint(h: u32) {
    let k = kind(h);
    let rs = rects(h);
    let tracking = crate::state::menu_mode().is_some();
    let Some(m) = data(h) else { return };
    let sel = if tracking { m.sel } else { None };
    let items: Vec<(u16, u16, u16, Vec<u8>, bool)> = m.items.iter().map(|i| (i.style, i.attr, i.id, i.text.clone(), i.sub != 0)).collect();
    let maximized = ctl::style(ctl::parent(h)) & ws::MAXIMIZED != 0;
    let icon = if k == MS_TITLEBUTTON { frame::icon(ctl::parent(h)) } else { 0 };
    draw::paint(h, |c| {
        let full = c.rect();
        c.fill(full, draw::face());
        if k == 0 {
            c.frame(full, draw::DARK);
            c.raised(Rect::new(1, 1, full.right - 1, full.top - 1));
        }
        for (i, ((style, attr, id, text, has_sub), r)) in items.iter().zip(rs.iter()).enumerate() {
            let hilite = sel == Some(i) || attr & MIA_HILITED != 0;
            match k {
                MS_TITLEBUTTON => title_button(c, *r, *id as u32, hilite, maximized, icon),
                _ if style & MIS_SEPARATOR != 0 => {
                    if k == 0 {
                        let y = r.bottom + r.height() / 2;
                        c.hline(r.left + 2, r.right - 2, y, draw::shadow());
                        c.hline(r.left + 2, r.right - 2, y - 1, draw::light());
                    }
                }
                _ => item(c, *r, k, *attr, text, *has_sub, hilite),
            }
        }
    });
}

fn item(c: &mut Canvas, r: Rect, k: u32, attr: u16, text: &[u8], has_sub: bool, hilite: bool) {
    let disabled = attr & MIA_DISABLED != 0;
    let fg = if disabled {
        draw::GRAY_TEXT
    } else if hilite {
        draw::hilite_text()
    } else {
        draw::text()
    };
    if hilite {
        c.fill(r, draw::hilite());
    }
    let f = c.font();
    let y = r.bottom + (r.height() - f.height) / 2 + f.descender;
    let (l, rt) = label(text);
    if k == MS_ACTIONBAR {
        c.mnemonic_text(r.left + 8, y, l, fg, r);
        return;
    }
    c.mnemonic_text(r.left + 20, y, l, fg, r);
    if !rt.is_empty() {
        c.text(r.right - 20 - f.text_width(rt), y, rt, fg, r);
    }
    if attr & MIA_CHECKED != 0 {
        // A check mark.
        let (x, b) = (r.left + 5, r.bottom + r.height() / 2 - 3);
        for i in 0..3 {
            c.fill(Rect::new(x + i, b + 2 - i + 2, x + i + 1, b + 5 - i + 2), fg);
        }
        for i in 0..6 {
            c.fill(Rect::new(x + 3 + i, b + i, x + 4 + i, b + i + 3), fg);
        }
    }
    if has_sub {
        // A small triangle pointing right.
        let (x, cy) = (r.right - 12, r.bottom + r.height() / 2);
        for i in 0..4 {
            c.fill(Rect::new(x + i, cy - 3 + i, x + i + 1, cy + 4 - i), fg);
        }
    }
}

/// A title-bar button with its symbol.
fn title_button(c: &mut Canvas, r: Rect, id: u32, down: bool, maximized: bool, icon: u32) {
    let r = Rect::new(r.left + 1, r.bottom + 1, r.right - 1, r.top - 1);
    c.fill(r, draw::face());
    if down {
        c.sunken(r);
    } else {
        c.raised(r);
    }
    let (cx, cy) = (r.left + r.width() / 2, r.bottom + r.height() / 2);
    let k = draw::text();
    match id {
        // The frame's mini icon, or a bar.
        SC_SYSMENU if icon != 0 && crate::pointer::draw_in(c, r, icon, true, false) => {}
        SC_SYSMENU => c.fill(Rect::new(r.left + 4, cy - 2, r.right - 4, cy + 2), k),
        SC_MINIMIZE => c.fill(Rect::new(cx - 2, cy - 2, cx + 2, cy + 2), k),
        SC_HIDE => c.fill(Rect::new(cx - 5, cy - 5, cx + 5, cy - 2), k),
        SC_MAXIMIZE | SC_RESTORE if maximized || id == SC_RESTORE => {
            c.frame(Rect::new(cx - 6, cy - 5, cx + 3, cy + 3), k);
            c.frame(Rect::new(cx - 3, cy - 2, cx + 6, cy + 6), k);
        }
        SC_MAXIMIZE => {
            c.frame(Rect::new(cx - 6, cy - 6, cx + 7, cy + 7), k);
            c.hline(cx - 6, cx + 7, cy + 5, k);
        }
        SC_CLOSE => {
            for i in -4..5 {
                c.fill(Rect::new(cx + i, cy + i, cx + i + 2, cy + i + 1), k);
                c.fill(Rect::new(cx + i, cy - i, cx + i + 2, cy - i + 1), k);
            }
        }
        _ => {}
    }
}

// ---- Items ----

/// Find an item by id (in submenus too when `deep`): (menu, index).
fn find(h: u32, id: u16, deep: bool) -> Option<(u32, usize)> {
    let m = data(h)?;
    if let Some(i) = m.items.iter().position(|it| it.id == id) {
        return Some((h, i));
    }
    if deep {
        let subs: Vec<u32> = m.items.iter().map(|it| it.sub).filter(|&s| s != 0).collect();
        for s in subs {
            if let Some(r) = find(s, id, true) {
                return Some(r);
            }
        }
    }
    None
}

fn item_ref(h: u32, i: usize) -> Option<&'static mut Item> {
    data(h)?.items.get_mut(i)
}

pub fn item_disabled(h: u32, id: u32) -> bool {
    find(h, id as u16, true).and_then(|(m, i)| item_ref(m, i)).is_some_and(|it| it.attr & MIA_DISABLED != 0)
}

/// Enable or disable an item (in submenus too).
pub fn enable_item(h: u32, id: u32, on: bool) {
    if let Some((m, i)) = find(h, id as u16, true) {
        if let Some(it) = item_ref(m, i) {
            it.attr = if on { it.attr & !MIA_DISABLED } else { it.attr | MIA_DISABLED };
        }
        ctl::invalidate(m);
    }
}

/// Change an item's id (the maximize button becomes restore).
pub fn replace_id(h: u32, old: u32, new: u32) {
    if let Some(it) = find(h, old as u16, false).and_then(|(m, i)| item_ref(m, i)) {
        it.id = new as u16;
        ctl::invalidate(h);
    }
}

fn menu_item(m: u32, i: usize) -> MenuItem {
    let it = item_ref(m, i).expect("menu item");
    MenuItem { pos: i as i16, style: it.style, attr: it.attr, id: it.id, sub: it.sub, handle: it.handle }
}

fn remove(h: u32, id: u16, deep: bool, destroy_sub: bool) -> u32 {
    let Some((m, i)) = find(h, id, deep) else { return item_count(h) as u32 };
    let Some(md) = data(m) else { return 0 };
    let it = md.items.remove(i);
    if md.sel == Some(i) {
        md.sel = None;
    }
    if destroy_sub && it.sub != 0 {
        win::destroy(it.sub);
    }
    ctl::invalidate(m);
    if kind(m) == MS_TITLEBUTTON {
        frame::layout(ctl::parent(m));
    }
    item_count(m) as u32
}

/// MM_INSERTITEM: insert at MENUITEM.iPosition (MIT_END appends).
fn insert(h: u32, p: *const MenuItem, text: *const u8) -> u32 {
    if p.is_null() {
        return MIT_NONE;
    }
    let mi = unsafe { p.read_unaligned() };
    let t = if mi.style & (MIS_TEXT | MIS_BITMAP) != 0 && mi.style & MIS_BITMAP == 0 { ctl::cstr(text) } else { Vec::new() };
    let Some(m) = data(h) else { return MIT_NONE };
    let at = if mi.pos == MIT_END || mi.pos < 0 { m.items.len() } else { (mi.pos as usize).min(m.items.len()) };
    m.items.insert(at, Item { style: mi.style, attr: mi.attr, id: mi.id, text: t, sub: mi.sub, handle: mi.handle });
    if mi.sub != 0 {
        if let Some(w) = st().desk.get_mut(mi.sub) {
            w.owner = h;
        }
    }
    ctl::invalidate(h);
    at as u32
}

// ---- Menu mode ----

fn root() -> u32 {
    crate::state::menu_mode().as_ref().map_or(0, |m| m.root)
}

/// The owner that gets the root menu's notifications.
fn notify_target(root: u32) -> u32 {
    ctl::owner(root)
}

/// The chain of open menus from the root.
fn chain() -> Vec<u32> {
    let mut v = Vec::new();
    let mut cur = root();
    while cur != 0 && !v.contains(&cur) {
        v.push(cur);
        cur = data(cur).map_or(0, |m| m.open);
    }
    v
}

/// Enter menu mode on `h` with item `sel` highlighted (and its submenu
/// shown when `drop`).
fn start(h: u32, sel: Option<usize>, drop: bool) {
    if root() != 0 {
        end(None, false);
    }
    let s = st();
    *crate::state::menu_mode() = Some(Mode { root: h });
    s.capture = h;
    // As in PM, the focus window keeps the focus (it gets no
    // WM_SETFOCUS); the queue's keys go to the menu meanwhile.
    send(notify_target(h), WM_INITMENU, ctl::id(h), h);
    if let Some(i) = sel {
        select(h, i);
        if drop {
            open_sub(h, i, false);
        }
    }
    ctl::invalidate(h);
}

/// Start menu mode on a menu bar or title button from the keyboard
/// (F10, Alt+Space): a title button drops its menu at once.
pub fn start_bar(h: u32) {
    if h == 0 || !is_menu(h) {
        return;
    }
    if root() == h {
        end(None, false);
        return;
    }
    let first = data(h).and_then(|m| m.items.iter().position(selectable));
    let drop = kind(h) == MS_TITLEBUTTON;
    start(h, first, drop);
    if drop {
        if let Some(sub) = data(h).map(|m| m.open).filter(|&s| s != 0) {
            select_first(sub);
        }
    }
}

/// Alt+letter on a frame: open the menu bar item with that mnemonic.
pub fn open_mnemonic(bar: u32, c: u8) -> bool {
    let Some(i) = data(bar).and_then(|m| m.items.iter().position(|it| selectable(it) && draw::mnemonic(&it.text) == Some(c))) else {
        return false;
    };
    start(bar, Some(i), true);
    if let Some(sub) = data(bar).map(|m| m.open).filter(|&s| s != 0) {
        select_first(sub);
    }
    true
}

/// Highlight item `i` of menu `h` (closing another item's submenu).
fn select(h: u32, i: usize) {
    let Some(m) = data(h) else { return };
    if m.sel == Some(i) {
        return;
    }
    m.sel = Some(i);
    let open = m.open;
    if open != 0 && m.items.get(i).map_or(true, |it| it.sub != open) {
        close_from(h);
    }
    ctl::invalidate(h);
    let id = data(h).and_then(|m| m.items.get(i)).map_or(0, |it| it.id as u32);
    send(notify_target(root()), WM_MENUSELECT, mp(id, 0), h);
}

fn select_first(h: u32) {
    if let Some(i) = data(h).and_then(|m| m.items.iter().position(selectable)) {
        select(h, i);
    }
}

/// Move the highlight by `d` items, skipping separators.
fn step(h: u32, d: i32) {
    let Some(m) = data(h) else { return };
    let n = m.items.len() as i32;
    if n == 0 {
        return;
    }
    let mut i = m.sel.map_or(if d > 0 { -1 } else { n }, |s| s as i32);
    for _ in 0..n {
        i = (i + d).rem_euclid(n);
        if m.items.get(i as usize).is_some_and(selectable) {
            break;
        }
    }
    select(h, i as usize);
}

/// Show the submenu of item `i` of `h`.
fn open_sub(h: u32, i: usize, first: bool) {
    let Some(it) = data(h).and_then(|m| m.items.get(i)) else { return };
    let (sub, id) = (it.sub, it.id);
    if sub == 0 || it.attr & MIA_DISABLED != 0 {
        return;
    }
    if data(h).is_some_and(|m| m.open == sub) {
        return;
    }
    close_from(h);
    send(notify_target(root()), WM_INITMENU, id as u32, sub);
    let r = rects(h).get(i).copied().unwrap_or_default();
    let sr = st().desk.screen_rect(h);
    let (w, hh) = popup_size(sub);
    let (sw, sh) = (st().surface.width, st().surface.height);
    let (mut left, mut top) = if horizontal(h) { (sr.left + r.left, sr.bottom + r.bottom) } else { (sr.left + r.right - 3, sr.bottom + r.top + 3) };
    if top - hh < 0 {
        top = if horizontal(h) { (sr.bottom + r.top + hh).min(sh) } else { hh.min(sh) };
    }
    left = left.clamp(0, (sw - w).max(0));
    if let Some(m) = data(sub) {
        m.sel = None;
        m.open = 0;
    }
    st().desk.set_pos(sub, left, top - hh, w, hh);
    st().desk.to_top(sub);
    win::show(sub, true);
    if let Some(m) = data(h) {
        m.open = sub;
    }
    if first {
        select_first(sub);
    }
}

/// Hide the submenus open below `h`.
fn close_from(h: u32) {
    let Some(m) = data(h) else { return };
    let open = core::mem::take(&mut m.open);
    if open != 0 {
        close_from(open);
        if let Some(sm) = data(open) {
            sm.sel = None;
        }
        win::show(open, false);
    }
}

/// Leave menu mode; `chosen` is the item picked (menu, index).
fn end(chosen: Option<(u32, usize)>, pointer: bool) {
    let Some(mode) = crate::state::menu_mode().take() else { return };
    let r = mode.root;
    close_from(r);
    if let Some(m) = data(r) {
        m.sel = None;
    }
    ctl::invalidate(r);
    if kind(r) == 0 {
        win::show(r, false);
    }
    let s = st();
    if s.capture == r {
        s.capture = 0;
    }
    let target = notify_target(r);
    send(target, WM_MENUEND, ctl::id(r), r);
    let Some((m, i)) = chosen else { return };
    let Some(it) = item_ref(m, i) else { return };
    let (id, style) = (it.id as u32, it.style);
    if send(target, WM_MENUSELECT, mp(id, 1), m) == 0 {
        return;
    }
    let msg = if style & MIS_SYSCOMMAND != 0 {
        WM_SYSCOMMAND
    } else if style & MIS_HELP != 0 {
        WM_HELP
    } else {
        WM_COMMAND
    };
    post(target, msg, id, mp(CMDSRC_MENU, pointer as u32));
}

/// Choose an item: open its submenu, or end menu mode with it.
fn choose(h: u32, i: usize, pointer: bool) {
    let Some(it) = item_ref(h, i) else { return };
    if !selectable(it) || it.attr & MIA_DISABLED != 0 {
        return;
    }
    if it.sub != 0 {
        open_sub(h, i, !pointer);
        return;
    }
    end(Some((h, i)), pointer);
}

/// The open menu (deepest first) and item under a screen point.
fn hit(p: Point) -> Option<(u32, Option<usize>)> {
    for m in chain().into_iter().rev() {
        let r = st().desk.screen_rect(m);
        if r.contains(p) {
            let i = item_at(m, Point::new(p.x - r.left, p.y - r.bottom)).filter(|&i| item_ref(m, i).is_some_and(|it| selectable(it)));
            return Some((m, i));
        }
    }
    None
}

fn screen_point(h: u32, mp1: u32) -> Point {
    let o = st().desk.origin(h);
    let p = ctl::mouse_pos(mp1);
    Point::new(p.x + o.x, p.y + o.y)
}

fn button_down(h: u32, mp1: u32, dbl: bool) {
    let p = screen_point(h, mp1);
    if root() == 0 {
        let i = item_at(h, ctl::mouse_pos(mp1)).filter(|&i| item_ref(h, i).is_some_and(|it| selectable(it)));
        if dbl && kind(h) == MS_TITLEBUTTON && i.and_then(|i| item_ref(h, i)).is_some_and(|it| it.id as u32 == SC_SYSMENU) {
            post(ctl::owner(h), WM_SYSCOMMAND, SC_CLOSE, mp(CMDSRC_MENU, 1));
            return;
        }
        start(h, i, true);
        return;
    }
    let r = root();
    match hit(p) {
        None => end(None, true),
        Some((m, Some(i))) => {
            let it = item_ref(m, i).map(|it| (it.sub, it.id as u32));
            if dbl && m == r && kind(r) == MS_TITLEBUTTON && it.is_some_and(|x| x.1 == SC_SYSMENU) {
                end(None, true);
                post(ctl::owner(r), WM_SYSCOMMAND, SC_CLOSE, mp(CMDSRC_MENU, 1));
                return;
            }
            let was_open = data(m).is_some_and(|d| d.sel == Some(i) && d.open != 0);
            if m == r && horizontal(r) && was_open {
                close_from(r);
                ctl::invalidate(r);
                return;
            }
            select(m, i);
            open_sub(m, i, false);
        }
        Some((_, None)) => {}
    }
}

fn button_up(h: u32, mp1: u32) {
    if root() == 0 {
        return;
    }
    let p = screen_point(h, mp1);
    if let Some((m, Some(i))) = hit(p) {
        let has_sub = item_ref(m, i).is_some_and(|it| it.sub != 0);
        if !has_sub {
            choose(m, i, true);
        }
    }
}

fn mouse_move(h: u32, mp1: u32) {
    let r = root();
    if r == 0 {
        return;
    }
    let p = screen_point(h, mp1);
    let Some((m, Some(i))) = hit(p) else { return };
    let dropped = data(r).is_some_and(|d| d.open != 0);
    if m == r && horizontal(r) && !dropped && st().buttons & 1 == 0 {
        return;
    }
    select(m, i);
    if !horizontal(m) || dropped || st().buttons & 1 != 0 {
        open_sub(m, i, false);
    }
}

const VK_F1: u32 = 0x20;

fn key(mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.up() {
        return true;
    }
    let ch = chain();
    let Some(&deep) = ch.last() else { return false };
    let r = ch[0];
    let bar = horizontal(r);
    let sel = data(deep).and_then(|m| m.sel);
    match k.vk {
        VK_ESC => {
            if ch.len() > 1 {
                close_from(ch[ch.len() - 2]);
                if !bar && ch.len() == 2 {
                    end(None, false);
                }
            } else {
                end(None, false);
            }
        }
        VK_F10 => end(None, false),
        // F1: help on the selected item, after menu mode ends.
        VK_F1 => {
            let id = sel.and_then(|i| item_ref(deep, i)).map_or(0xffff, |it| it.id as u32);
            let mut rect = crate::help::screen_rect(deep);
            if let Some(r) = sel.and_then(|i| rects(deep).get(i).copied()) {
                let (x, y) = (rect.left, rect.bottom);
                rect = crate::gpiapi::RectL { left: x + r.left, bottom: y + r.bottom, right: x + r.right, top: y + r.top };
            }
            end(None, false);
            crate::help::menu_help(deep, id, rect);
        }
        VK_UP | VK_DOWN => {
            if horizontal(deep) {
                if let Some(i) = sel {
                    open_sub(deep, i, true);
                }
            } else {
                step(deep, if k.vk == VK_DOWN { 1 } else { -1 });
            }
        }
        VK_LEFT | VK_RIGHT => {
            let right = k.vk == VK_RIGHT;
            let sub_of_sel = sel.and_then(|i| item_ref(deep, i)).map_or(0, |it| it.sub);
            if !horizontal(deep) && right && sub_of_sel != 0 {
                open_sub(deep, sel.unwrap_or(0), true);
            } else if !horizontal(deep) && !right && ch.len() > 2 {
                close_from(ch[ch.len() - 2]);
            } else if bar {
                let dropped = ch.len() > 1;
                close_from(r);
                step(r, if right { 1 } else { -1 });
                if dropped {
                    if let Some(i) = data(r).and_then(|m| m.sel) {
                        open_sub(r, i, true);
                    }
                }
            }
        }
        VK_NEWLINE | VK_ENTER => {
            if let Some(i) = sel {
                choose(deep, i, false);
            }
        }
        _ => {
            let c = k.letter(mp1);
            if c == 0 {
                return false;
            }
            let found = data(deep).and_then(|m| m.items.iter().position(|it| selectable(it) && draw::mnemonic(&it.text) == Some(c)));
            if let Some(i) = found {
                select(deep, i);
                choose(deep, i, false);
            }
        }
    }
    true
}

/// Windows destroyed: leave menu mode if its root went.
pub fn forget(gone: &[u32]) {
    for q in st().queues.values_mut() {
        if q.menu_mode.as_ref().is_some_and(|m| gone.contains(&m.root)) {
            q.menu_mode = None;
        }
    }
}

// ---- The window procedure ----

pub extern "C" fn menu_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            ctl::set_data(h, Menu::default());
            if mp1 != 0 {
                let p = mp1 as *const u8;
                let cb = unsafe { (p as *const u16).read_unaligned() } as usize;
                if let Some(items) = res::parse_menu(unsafe { core::slice::from_raw_parts(p, cb) }) {
                    fill(h, &items);
                }
            }
            0
        }
        WM_DESTROY => {
            if root() == h {
                end(None, false);
            }
            let subs: Vec<u32> = data(h).map_or(Vec::new(), |m| m.items.iter().map(|i| i.sub).filter(|&s| s != 0).collect());
            for s in subs {
                win::destroy(s);
            }
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON1DBLCLK => {
            button_down(h, mp1, msg == WM_BUTTON1DBLCLK);
            1
        }
        WM_BUTTON1UP => {
            button_up(h, mp1);
            1
        }
        WM_MOUSEMOVE => {
            mouse_move(h, mp1);
            1
        }
        WM_CHAR => {
            if root() != 0 {
                return key(mp1, mp2) as u32;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_QUERYDLGCODE => DLGC_MENU,
        MM_INSERTITEM => insert(h, mp1 as *const MenuItem, mp2 as *const u8),
        MM_DELETEITEM => remove(h, lo(mp1) as u16, hi(mp1) != 0, true),
        MM_REMOVEITEM => remove(h, lo(mp1) as u16, hi(mp1) != 0, false),
        MM_QUERYITEM => {
            let Some((m, i)) = find(h, lo(mp1) as u16, hi(mp1) != 0) else { return 0 };
            if mp2 != 0 {
                unsafe { (mp2 as *mut MenuItem).write_unaligned(menu_item(m, i)) };
            }
            1
        }
        MM_SETITEM => {
            if mp2 == 0 {
                return 0;
            }
            let mi = unsafe { (mp2 as *const MenuItem).read_unaligned() };
            let Some((m, i)) = find(h, mi.id, hi(mp1) != 0) else { return 0 };
            if let Some(it) = item_ref(m, i) {
                (it.style, it.attr, it.sub, it.handle) = (mi.style, mi.attr, mi.sub, mi.handle);
            }
            ctl::invalidate(m);
            1
        }
        MM_QUERYITEMCOUNT => item_count(h) as u32,
        MM_ITEMIDFROMPOSITION => data(h).and_then(|m| m.items.get(mp1 as i16 as usize)).map_or(MIT_NONE, |it| it.id as u32),
        MM_ITEMPOSITIONFROMID => find(h, lo(mp1) as u16, hi(mp1) != 0).map_or(MIT_NONE, |(_, i)| i as u32),
        MM_QUERYITEMATTR => find(h, lo(mp1) as u16, hi(mp1) != 0).and_then(|(m, i)| item_ref(m, i)).map_or(0, |it| (it.attr as u32) & mp2),
        MM_SETITEMATTR => {
            let Some((m, i)) = find(h, lo(mp1) as u16, hi(mp1) != 0) else { return 0 };
            let (mask, val) = (lo(mp2) as u16, hi(mp2) as u16);
            if let Some(it) = item_ref(m, i) {
                it.attr = (it.attr & !mask) | (val & mask);
            }
            ctl::invalidate(m);
            1
        }
        MM_ISITEMVALID => find(h, lo(mp1) as u16, hi(mp1) != 0).and_then(|(m, i)| item_ref(m, i)).is_some_and(|it| it.attr & MIA_DISABLED == 0) as u32,
        MM_QUERYITEMTEXT => {
            let Some(it) = find(h, lo(mp1) as u16, true).and_then(|(m, i)| item_ref(m, i)) else { return 0 };
            ctl::copy_out(&it.text, hi(mp1) as usize, mp2 as *mut u8)
        }
        MM_QUERYITEMTEXTLENGTH => find(h, lo(mp1) as u16, true).and_then(|(m, i)| item_ref(m, i)).map_or(0, |it| it.text.len() as u32),
        MM_SETITEMTEXT => {
            let Some((m, i)) = find(h, lo(mp1) as u16, true) else { return 0 };
            if let Some(it) = item_ref(m, i) {
                it.text = ctl::cstr(mp2 as *const u8);
            }
            ctl::invalidate(m);
            1
        }
        MM_SETITEMHANDLE => {
            let Some(it) = find(h, lo(mp1) as u16, true).and_then(|(m, i)| item_ref(m, i)) else { return 0 };
            it.handle = mp2;
            1
        }
        MM_QUERYITEMRECT => {
            let Some((m, i)) = find(h, lo(mp1) as u16, hi(mp1) != 0) else { return 0 };
            let r = rects(m).get(i).copied().unwrap_or_default();
            if mp2 != 0 {
                unsafe { (mp2 as *mut crate::gpiapi::RectL).write_unaligned(crate::gpiapi::RectL::from(r)) };
            }
            1
        }
        MM_SELECTITEM => {
            let id = lo(mp1) as i16;
            if id == -1 {
                if let Some(m) = data(h) {
                    m.sel = None;
                }
                ctl::invalidate(h);
                return 1;
            }
            let Some((m, i)) = find(h, id as u16, hi(mp1) != 0) else { return 0 };
            if root() == 0 {
                start(h, None, false);
            }
            select(m, i);
            1
        }
        MM_QUERYSELITEMID => data(h).and_then(|m| m.sel.and_then(|i| m.items.get(i))).map_or(MIT_NONE, |it| it.id as u32),
        MM_STARTMENUMODE => {
            let first = data(h).and_then(|m| m.items.iter().position(selectable));
            start(h, first, lo(mp1) != 0);
            1
        }
        MM_ENDMENUMODE => {
            end(None, false);
            1
        }
        MM_QUERYDEFAULTITEMID => data(h).map_or(0, |m| m.default_id),
        MM_SETDEFAULTITEMID => {
            if let Some(m) = data(h) {
                m.default_id = mp1;
            }
            1
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
