//! The multiple document interface: the MDICLIENT class, DefFrameProc,
//! DefMDIChildProc, TranslateMDISysAccel and ArrangeIconicWindows.
//!
//! An MDI child is a PM frame window (title bar, system menu, sizing
//! border, minimise and maximise buttons from its style) inside the MDI
//! client's PM window, so PM moves and sizes it within the client.
//! Activation among the children is the client's: the active child is
//! on top with its title bar highlighted, gets the focus and
//! WM_MDIACTIVATE, and is checked in the Window menu, where the client
//! lists the children (IDs from idFirstChild on).
//!
//! A maximized child is Windows' too: its client area fills the MDI
//! client (its border and title bar lie outside), the frame's menu bar
//! gets the child's system menu at its start and a restore button at its
//! end (owner-drawn items WIN16 draws; their commands come to
//! DefFrameProc as WM_COMMAND and go to the child as WM_SYSCOMMAND), and
//! the frame's title becomes "Frame - [Child]". Activating another child
//! maximizes it instead; new children are made maximized. Minimized
//! children are icons along the bottom of the client (PM's frames inside
//! another window minimize there). The title bar buttons of a child send
//! it WM_SYSCOMMAND, which DefMDIChildProc carries out.

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::pm::*;
use crate::user::{self, wnd};

pub const W_MDICREATE: u16 = 0x0220;
const W_MDIDESTROY: u16 = 0x0221;
const W_MDIACTIVATE: u16 = 0x0222;
const W_MDIRESTORE: u16 = 0x0223;
const W_MDINEXT: u16 = 0x0224;
const W_MDIMAXIMIZE: u16 = 0x0225;
const W_MDITILE: u16 = 0x0226;
const W_MDICASCADE: u16 = 0x0227;
const W_MDIICONARRANGE: u16 = 0x0228;
const W_MDIGETACTIVE: u16 = 0x0229;
const W_MDISETMENU: u16 = 0x0230;
const W_CREATE: u16 = 0x0001;
const W_SIZE: u16 = 0x0005;
const W_SETFOCUS: u16 = 0x0007;
const W_SETTEXT: u16 = 0x000c;
const W_CLOSE: u16 = 0x0010;
const W_CHILDACTIVATE: u16 = 0x0022;
const W_COMMAND: u16 = 0x0111;
const W_SYSCOMMAND: u16 = 0x0112;
const W_MOUSEACTIVATE: u16 = 0x0021;
const W_KEYDOWN: u16 = 0x0100;
const W_SYSKEYDOWN: u16 = 0x0104;

const SC_SIZE: u16 = 0xf000;
const SC_MOVE: u16 = 0xf010;
const SC_CLOSE: u16 = 0xf060;
const SC_NEXTWINDOW: u16 = 0xf040;
const SC_MINIMIZE: u16 = 0xf020;
const SC_MAXIMIZE: u16 = 0xf030;
const SC_RESTORE: u16 = 0xf120;

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_DISABLED: u32 = 0x0800_0000;
const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
const WS_CAPTION: u32 = 0x00c0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_THICKFRAME: u32 = 0x0004_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
const WS_MINIMIZE: u32 = 0x2000_0000;
const WS_MAXIMIZE: u32 = 0x0100_0000;
const CW_USEDEFAULT: i32 = -32768;

const FID_TITLEBAR: u32 = 0x8003;
const FID_SYSMENU: u32 = 0x8002;
/// The ID of the separator before the children in the Window menu.
const SEPARATOR: u16 = 0xf7ff;
const TBM_SETHILITE: u32 = 0x01e3;
/// The menu bar items of a maximized child: its system menu (a pop-up,
/// an ID in the range of WIN16's pop-up items) and the restore button
/// (SC_RESTORE).
pub const ID_MDISYSMENU: u16 = 0xfff0;

/// The state of an MDI client window.
pub struct Client {
    /// The Window menu (its children listed at the end) and the first
    /// child's ID.
    window_menu: u16,
    first_id: u16,
    /// The children, oldest first, and the active one.
    children: Vec<u16>,
    active: u16,
    /// Where the next child goes (cascading).
    next: i32,
    /// The children listed in the Window menu.
    listed: u32,
    /// The maximized child, the frame's own title meanwhile, the menu
    /// bar that has the child's items and the child's system menu there.
    maximized: u16,
    base_title: Option<Vec<u8>>,
    bar: u16,
    sys_popup: u16,
}

fn client(h: u16) -> Option<&'static mut Client> {
    user::data::<Client>(h)
}

/// Is `h` an MDI client window?
pub fn is_client(h: u16) -> bool {
    client(h).is_some()
}

/// The MDI client a child belongs to.
fn client_of(child: u16) -> u16 {
    wnd(child).map_or(0, |w| w.parent)
}

/// The frame window an MDI client is in.
fn frame_of(h: u16) -> u16 {
    wnd(h).map_or(0, |w| w.parent)
}

/// The Window menu: the children, numbered, after a separator; the
/// active one checked.
fn update_menu(h: u16) {
    let Some(c) = client(h) else { return };
    let (menu, first, kids, active) = (c.window_menu, c.first_id, c.children.clone(), c.active);
    if menu == 0 {
        return;
    }
    // Take the old entries (and their separator) away.
    for i in 0..c.listed {
        crate::menu::remove(menu, first + i as u16, 0, true);
    }
    crate::menu::remove(menu, SEPARATOR, 0, true);
    c.listed = 0;
    if kids.is_empty() {
        return;
    }
    crate::menu::insert(menu, -1, 0x0800, SEPARATOR, b"");
    c.listed = kids.len().min(9) as u32;
    for (i, &k) in kids.iter().enumerate().take(9) {
        let mut t = alloc::format!("&{} ", i + 1).into_bytes();
        t.extend_from_slice(&user::window_text(k));
        let fl = if k == active { 0x0008 } else { 0 };
        crate::menu::insert(menu, -1, fl, first + i as u16, &t);
    }
}

/// Highlight a child's title bar (or not).
fn hilite(child: u16, on: bool) {
    let Some(f) = wnd(child).map(|w| w.frame) else { return };
    unsafe {
        let tb = WinWindowFromID(f, FID_TITLEBAR);
        if tb != 0 {
            WinSendMsg(tb, TBM_SETHILITE, on as u32, 0);
        }
    }
}

/// Make a child the active one (maximized if the active one was).
pub fn activate(h: u16, child: u16) {
    let Some(c) = client(h) else { return };
    let old = c.active;
    if !c.children.contains(&child) {
        return;
    }
    c.active = child;
    let max = c.maximized;
    if max != 0 && max != child && !user::is_iconic(child) {
        maximize(h, child);
    }
    if old != child {
        if old != 0 && wnd(old).is_some() {
            hilite(old, false);
            user::send(old, W_MDIACTIVATE, 0, (child as u32) | (old as u32) << 16);
        }
        hilite(child, true);
        user::send(child, W_MDIACTIVATE, 1, (child as u32) | (old as u32) << 16);
    }
    if let Some(f) = wnd(child).map(|w| w.frame) {
        unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER) };
    }
    if !user::is_child(child, user::focus()) {
        user::set_focus(child);
    }
    update_menu(h);
}

/// The next (or previous) child after `from`.
fn next(h: u16, from: u16, back: bool) -> u16 {
    let Some(c) = client(h) else { return 0 };
    let n = c.children.len();
    if n == 0 {
        return 0;
    }
    let i = c.children.iter().position(|&k| k == from).unwrap_or(0);
    let j = if back { (i + n - 1) % n } else { (i + 1) % n };
    c.children[j]
}

/// WM_MDICREATE: MDICREATESTRUCT szClass, szTitle, hOwner, x, y, cx, cy,
/// style, lParam.
fn create(h: u16, p: u32) -> u16 {
    let q = thunk16::flat(p);
    if q.is_null() {
        return 0;
    }
    let l = |o: usize| unsafe { (q.add(o) as *const u32).read_unaligned() };
    let w = |o: usize| unsafe { (q.add(o) as *const i16).read_unaligned() } as i32;
    let (class, title, owner) = (l(0), l(4), w(8) as u16);
    let (mut x, mut y, mut cx, mut cy) = (w(10), w(12), w(14), w(16));
    let style = l(18);
    let Some(c) = client(h) else { return 0 };
    let (cw, ch) = user::client_size(h);
    let step = cascade_step();
    if x == CW_USEDEFAULT {
        let k = c.next % 8;
        c.next += 1;
        x = k * step;
        y = k * step;
    }
    if cx == CW_USEDEFAULT || cx == 0 {
        cx = (cw * 3 / 4).max(100);
        cy = (ch * 3 / 4).max(60);
    }
    let id = c.first_id + c.children.len() as u16;
    let style = style | WS_CHILD | WS_CLIPSIBLINGS | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
    let hwnd = user::create_window(0, class, title, style & !(WS_MINIMIZE | WS_MAXIMIZE | WS_VISIBLE), [x, y, cx, cy], h, id, owner, p);
    if hwnd == 0 {
        return 0;
    }
    let max = client(h).map_or(0, |c| {
        c.children.push(hwnd);
        c.maximized
    });
    if style & WS_MINIMIZE != 0 {
        user::show_window(hwnd, 7);
    } else {
        user::show_window(hwnd, 5);
        if style & WS_MAXIMIZE != 0 || max != 0 {
            maximize(h, hwnd);
        }
    }
    activate(h, hwnd);
    hwnd
}

/// How far each cascaded child is from the one before: a title bar.
fn cascade_step() -> i32 {
    let v = unsafe { WinQuerySysValue(HWND_DESKTOP, 30) };
    if (10..40).contains(&v) { v + 2 } else { 20 }
}

/// A child is going: the next one becomes active (maximized if this one
/// was).
fn destroy(h: u16, child: u16) {
    let Some(c) = client(h) else { return };
    let Some(i) = c.children.iter().position(|&k| k == child) else { return };
    let was_active = c.active == child;
    let was_max = c.maximized == child;
    c.children.remove(i);
    // The others keep their IDs in order.
    let (first, kids) = (c.first_id, c.children.clone());
    for (k, &w) in kids.iter().enumerate() {
        if let Some(x) = wnd(w) {
            x.id = first + k as u16;
        }
    }
    if was_active {
        c.active = 0;
    }
    if was_max {
        unmaximized(h);
    }
    user::destroy_window(child);
    if was_active {
        if let Some(&n) = client(h).and_then(|c| c.children.last()) {
            if was_max && !user::is_iconic(n) {
                maximize(h, n);
            }
            activate(h, n);
        }
    }
    update_menu(h);
}

/// Put a maximized child's client area over the MDI client: its border
/// and title bar go outside.
fn place_max(h: u16, child: u16) {
    let Some(f) = wnd(child).map(|w| w.frame) else { return };
    let (pw, ph) = user::client_size(h);
    let mut fs = Swp::default();
    let mut ts = Swp::default();
    let mut ss = Swp::default();
    unsafe {
        WinQueryWindowPos(f, &mut fs);
        let tb = WinWindowFromID(f, FID_TITLEBAR);
        if tb == 0 {
            return;
        }
        WinQueryWindowPos(tb, &mut ts);
        // The border: where the system menu button (or the title bar)
        // starts.
        let sm = WinWindowFromID(f, FID_SYSMENU);
        if sm != 0 {
            WinQueryWindowPos(sm, &mut ss);
        } else {
            ss = ts;
        }
    }
    let b = ss.x.max(0);
    let top = (fs.cy - ts.y).max(b);
    unsafe { WinSetWindowPos(f, 0, -b, -b, pw + 2 * b, ph + b + top, SWP_MOVE | SWP_SIZE) };
}

/// The frame's title: "Frame - [Child]" while a child is maximized, its
/// own again after.
fn set_title(h: u16) {
    let Some(c) = client(h) else { return };
    let (max, base) = (c.maximized, c.base_title.clone());
    let frame = frame_of(h);
    let Some(base) = base else { return };
    if max == 0 {
        user::set_text_raw(frame, &base);
        if let Some(c) = client(h) {
            c.base_title = None;
        }
        return;
    }
    let mut t = base;
    t.extend_from_slice(b" - [");
    t.extend_from_slice(&user::window_text(max));
    t.push(b']');
    user::set_text_raw(frame, &t);
}

/// The maximized child's items in the frame's menu bar.
fn add_items(h: u16) {
    let bar = wnd(frame_of(h)).map_or(0, |w| w.menu);
    let Some(c) = client(h) else { return };
    if bar == 0 || c.bar != 0 {
        return;
    }
    if c.sys_popup == 0 {
        c.sys_popup = sys_popup();
    }
    c.bar = bar;
    let sys = c.sys_popup;
    crate::menu::insert_owner(bar, 0, ID_MDISYSMENU, sys);
    crate::menu::insert_owner(bar, -1, SC_RESTORE, 0);
}

fn remove_items(h: u16) {
    let Some(c) = client(h) else { return };
    let bar = core::mem::replace(&mut c.bar, 0);
    if bar == 0 {
        return;
    }
    crate::menu::remove_keep(bar, ID_MDISYSMENU);
    crate::menu::remove_keep(bar, SC_RESTORE);
}

/// A maximized child's system menu: Windows' window commands.
fn sys_popup() -> u16 {
    let m = crate::menu::create_popup();
    let items: [(u16, u16, &[u8]); 9] = [
        (SC_RESTORE, 0, b"&Restore"),
        (SC_MOVE, 1, b"&Move"),
        (SC_SIZE, 1, b"&Size"),
        (SC_MINIMIZE, 0, b"Mi&nimize"),
        (SC_MAXIMIZE, 1, b"Ma&ximize"),
        (0, 0x0800, b""),
        (SC_CLOSE, 0, b"&Close\tCtrl+F4"),
        (0, 0x0800, b""),
        (SC_NEXTWINDOW, 0, b"Nex&t\tCtrl+F6"),
    ];
    for (id, fl, t) in items {
        crate::menu::insert(m, -1, fl, id, t);
    }
    m
}

/// Maximize a child (a maximized one is restored first).
pub fn maximize(h: u16, child: u16) {
    let Some(c) = client(h) else { return };
    if !c.children.contains(&child) {
        return;
    }
    let old = c.maximized;
    if old == child {
        place_max(h, child);
        return;
    }
    if c.base_title.is_none() {
        c.base_title = Some(user::window_text(frame_of(h)));
    }
    c.maximized = child;
    if old != 0 && wnd(old).is_some() {
        user::show_window(old, 4);
        unsafe { WinSetWindowPos(wnd(old).map_or(0, |w| w.frame), 0, 0, 0, 0, 0, SWP_RESTORE) };
    }
    if user::is_iconic(child) {
        user::show_window(child, 9);
    }
    user::show_window(child, 3);
    place_max(h, child);
    add_items(h);
    set_title(h);
    crate::menu::draw_bar(frame_of(h));
}

/// No child is maximized any more: the menu bar and the title go back.
fn unmaximized(h: u16) {
    let Some(c) = client(h) else { return };
    if c.maximized == 0 {
        return;
    }
    c.maximized = 0;
    remove_items(h);
    set_title(h);
    crate::menu::draw_bar(frame_of(h));
}

/// Restore a child (from maximized or minimized).
fn restore_child(h: u16, child: u16) {
    if client(h).is_some_and(|c| c.maximized == child) {
        unmaximized(h);
    }
    user::show_window(child, 9);
}

/// Minimize a child.
fn minimize_child(h: u16, child: u16) {
    if client(h).is_some_and(|c| c.maximized == child) {
        unmaximized(h);
    }
    user::show_window(child, 6);
}

/// Line up the minimized children along the bottom of the client
/// (WM_MDIICONARRANGE, ArrangeIconicWindows): the icons' height.
fn arrange_icons(h: u16) -> u32 {
    let Some(c) = client(h) else { return 0 };
    let icons: Vec<u32> = c.children.iter().copied().filter(|&k| user::is_iconic(k)).filter_map(|k| wnd(k).map(|w| w.frame)).collect();
    let (cw, _) = user::client_size(h);
    let gap = 2;
    let (mut x, mut y, mut row_h) = (gap, gap, 0);
    for f in icons {
        let mut s = Swp::default();
        unsafe { WinQueryWindowPos(f, &mut s) };
        if x > gap && x + s.cx > cw {
            x = gap;
            y += row_h + gap;
            row_h = 0;
        }
        unsafe { WinSetWindowPos(f, 0, x, y, 0, 0, SWP_MOVE) };
        x += s.cx + gap;
        row_h = row_h.max(s.cy);
    }
    row_h as u32
}

/// The height the icons take at the bottom of the client.
fn icon_rows(h: u16) -> i32 {
    let Some(c) = client(h) else { return 0 };
    let mut hh = 0;
    for &k in &c.children {
        if user::is_iconic(k) {
            if let Some(f) = wnd(k).map(|w| w.frame) {
                let mut s = Swp::default();
                unsafe { WinQueryWindowPos(f, &mut s) };
                hh = hh.max(s.y + s.cy + 2);
            }
        }
    }
    hh
}

/// Tile (side by side, or MDITILE_HORIZONTAL one above another) or
/// cascade the children; a maximized one is restored first.
fn arrange(h: u16, tile: bool, how: u16) {
    if let Some(m) = client(h).map(|c| c.maximized).filter(|&m| m != 0) {
        restore_child(h, m);
    }
    let Some(c) = client(h) else { return };
    // MDITILE_SKIPDISABLED (2): disabled children stay where they are.
    let kids: Vec<u16> = c
        .children
        .iter()
        .copied()
        .filter(|&k| !user::is_iconic(k) && (how & 2 == 0 || wnd(k).is_some_and(|w| w.style & WS_DISABLED == 0)))
        .collect();
    let (cw, ch) = user::client_size(h);
    // The icons keep their row at the bottom.
    let ch = (ch - icon_rows(h)).max(40);
    if kids.is_empty() {
        return;
    }
    if tile {
        let n = kids.len() as i32;
        let k = (1..=n).find(|c| c * c >= n).unwrap_or(1);
        let other = (n + k - 1) / k;
        // Vertical tiling: columns; horizontal: rows.
        let (cols, rows) = if how & 1 != 0 { (other, k) } else { (k, other) };
        for (i, &kid) in kids.iter().enumerate() {
            let i = i as i32;
            let (col, row) = if how & 1 != 0 { (i / rows, i % rows) } else { (i % cols, i / cols) };
            user::show_window(kid, 9);
            user::move_to(kid, [col * cw / cols, row * ch / rows, (col + 1) * cw / cols, (row + 1) * ch / rows]);
        }
    } else {
        let step = cascade_step();
        let n = kids.len() as i32;
        let (w, hh) = ((cw - step * (n - 1)).max(cw * 2 / 3), (ch - step * (n - 1)).max(ch * 2 / 3));
        for (i, &k) in kids.iter().enumerate() {
            let o = i as i32 * step;
            user::show_window(k, 9);
            user::move_to(k, [o, o, o + w, o + hh]);
        }
        if let Some(c) = client(h) {
            c.next = kids.len() as i32;
        }
    }
    if let Some(&a) = kids.last() {
        activate(h, a);
    }
}

/// WM_MEASUREITEM and WM_DRAWITEM (PM's, `oi` an OWNERITEM) for the
/// maximized child's items in a menu bar: None for other items.
pub fn owner_item(msg: u32, oi: *mut u8) -> Option<u32> {
    if oi.is_null() {
        return None;
    }
    let rd = |o: usize| unsafe { (oi.add(o) as *const i32).read_unaligned() };
    let id = rd(40) as u16;
    if (id != ID_MDISYSMENU && id != SC_RESTORE) || rd(8) as u16 & 0x0008 == 0 {
        return None;
    }
    if msg == 0x0037 {
        // WM_MEASUREITEM: rclItem's xRight and yTop.
        let (l, b) = (rd(24), rd(28));
        unsafe {
            (oi.add(32) as *mut i32).write_unaligned(l + 24);
            (oi.add(36) as *mut i32).write_unaligned(b + 18);
        }
        return Some(18);
    }
    let hps = rd(4) as u32;
    let r = RectL { l: rd(24), b: rd(28), r: rd(32), t: rd(36) };
    let face = crate::gdi::rgb(crate::gdi::sys_color(crate::gdi::COLOR_BTNFACE));
    let (black, white, dark) = (0, 0xffffff, 0x808080);
    let bx = |c: i32, l: i32, b: i32, rr: i32, t: i32, fill: bool| unsafe {
        GpiSetColor(hps, c);
        GpiMove(hps, &PointL { x: l, y: b });
        GpiBox(hps, if fill { DRO_FILL } else { DRO_OUTLINE }, &PointL { x: rr, y: t }, 0, 0);
    };
    unsafe { GpiCreateLogColorTable(hps, 0, LCOLF_RGB, 0, 0, core::ptr::null()) };
    let (cx, cy) = ((r.l + r.r) / 2, (r.b + r.t) / 2);
    // A button of 18 x 16 pels in the middle of the item.
    let (l, b, rr, t) = (cx - 9, cy - 8, cx + 8, cy + 7);
    bx(face, r.l, r.b, r.r - 1, r.t - 1, true);
    if id == ID_MDISYSMENU {
        // Windows' document system menu: a short bar in a box.
        bx(dark, l + 2, b + 2, rr - 2, t - 2, false);
        bx(white, l + 3, b + 3, rr - 3, t - 3, true);
        bx(black, l + 5, cy - 1, rr - 5, cy + 1, true);
    } else {
        // The restore button: a raised button with an arrow up and one
        // down.
        bx(black, l, b, rr, t, false);
        bx(white, l + 1, b + 2, rr - 2, t - 1, false);
        bx(dark, l + 1, b + 1, rr - 1, b + 1, true);
        bx(dark, rr - 1, b + 1, rr - 1, t - 1, true);
        for i in 0..4 {
            bx(black, cx - i, cy + 5 - i, cx + i, cy + 5 - i, true);
            bx(black, cx - i, cy - 5 + i, cx + i, cy - 5 + i, true);
        }
    }
    Some(1)
}

/// PM's window commands from an MDI child's title bar buttons (or PM's
/// keys): Windows' WM_SYSCOMMAND to the child, which DefMDIChildProc
/// carries out. False for other windows.
pub fn pm_syscommand(child: u16, pm_sc: u32) -> bool {
    if !is_client(client_of(child)) {
        return false;
    }
    let sc = match pm_sc {
        0x8002 => SC_MINIMIZE,
        0x8003 => SC_MAXIMIZE,
        0x8008 => SC_RESTORE,
        0x8004 => SC_CLOSE,
        _ => return false,
    };
    user::send(child, W_SYSCOMMAND, sc, 0);
    true
}

/// The MDICLIENT window procedure (built-in entry 9).
pub fn client_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    match m {
        // CLIENTCREATESTRUCT (lpCreateParams): hWindowMenu, idFirstChild.
        W_CREATE => {
            let cs = thunk16::flat(lp);
            let params = if cs.is_null() { 0 } else { unsafe { (cs as *const u32).read_unaligned() } };
            let p = thunk16::flat(params);
            let (menu, first) = if p.is_null() {
                (0, 0)
            } else {
                unsafe { ((p as *const u16).read_unaligned(), (p.add(2) as *const u16).read_unaligned()) }
            };
            // Its scroll bars show only when children are out of view.
            crate::scroll::set_range(h, 0, 0, 0);
            crate::scroll::set_range(h, 1, 0, 0);
            if let Some(w) = wnd(h) {
                w.data = Some(alloc::boxed::Box::new(Client {
                    window_menu: menu,
                    first_id: first,
                    children: Vec::new(),
                    active: 0,
                    next: 0,
                    listed: 0,
                    maximized: 0,
                    base_title: None,
                    bar: 0,
                    sys_popup: 0,
                }));
            }
            0
        }
        W_MDICREATE => create(h, lp) as u32,
        W_MDIDESTROY => {
            destroy(h, wp);
            0
        }
        W_MDIACTIVATE => {
            activate(h, wp);
            0
        }
        W_MDIGETACTIVE => {
            let (a, max) = client(h).map_or((0, 0), |c| (c.active, c.maximized));
            (a as u32) | ((a != 0 && a == max) as u32) << 16
        }
        W_MDINEXT => {
            let from = if wp != 0 { wp } else { client(h).map_or(0, |c| c.active) };
            let n = next(h, from, lp != 0);
            if n != 0 {
                activate(h, n);
            }
            0
        }
        W_MDIMAXIMIZE => {
            maximize(h, wp);
            activate(h, wp);
            0
        }
        W_MDIRESTORE => {
            restore_child(h, wp);
            0
        }
        W_MDITILE | W_MDICASCADE => {
            arrange(h, m == W_MDITILE, wp);
            1
        }
        W_MDIICONARRANGE => {
            arrange_icons(h);
            0
        }
        W_SIZE => {
            if let Some(mx) = client(h).map(|c| c.maximized).filter(|&x| x != 0) {
                place_max(h, mx);
            }
            user::def_window_proc(h, m, wp, lp)
        }
        // WM_MDISETMENU(fRefresh, MAKELONG(hMenuFrame, hMenuWindow)): the
        // frame's old menu.
        W_MDISETMENU => {
            let (frame_menu, window_menu) = (lp as u16, (lp >> 16) as u16);
            let frame = frame_of(h);
            let old = wnd(frame).map_or(0, |w| w.menu);
            if window_menu != 0 {
                // The children leave the old Window menu.
                let kids = client(h).map(|c| core::mem::take(&mut c.children)).unwrap_or_default();
                update_menu(h);
                if let Some(c) = client(h) {
                    c.children = kids;
                    c.window_menu = window_menu;
                }
            }
            let max = client(h).map_or(0, |c| c.maximized);
            if frame_menu != 0 {
                if max != 0 {
                    remove_items(h);
                }
                crate::menu::set_menu(frame, frame_menu);
                if max != 0 {
                    add_items(h);
                }
            }
            update_menu(h);
            old as u32
        }
        W_SETFOCUS => {
            if let Some(a) = client(h).map(|c| c.active).filter(|&a| a != 0) {
                user::set_focus(a);
            }
            0
        }
        _ => user::def_window_proc(h, m, wp, lp),
    }
}

/// DefFrameProc(hwnd, hwndMDIClient, msg, wParam, lParam)
fn def_frame_proc(h: u16, mc: u16, m: u16, wp: u16, lp: u32) -> u32 {
    match m {
        // The Window menu's children; system commands of a maximized
        // child (its items in the menu bar).
        W_COMMAND => {
            if let Some(c) = client(mc) {
                let i = wp.wrapping_sub(c.first_id) as usize;
                if let Some(&k) = c.children.get(i) {
                    if user::is_iconic(k) {
                        restore_child(mc, k);
                    }
                    activate(mc, k);
                    return 0;
                }
                let a = c.active;
                if wp >= 0xf000 && a != 0 && c.maximized == a {
                    return user::send(a, W_SYSCOMMAND, wp, lp);
                }
            }
            user::def_window_proc(h, m, wp, lp)
        }
        // The client fills the frame's client area.
        W_SIZE => {
            if mc != 0 && wnd(mc).is_some() {
                let (w, hh) = (lp as u16 as i32, (lp >> 16) as u16 as i32);
                user::move_to(mc, [0, 0, w, hh]);
            }
            user::def_window_proc(h, m, wp, lp)
        }
        W_SETFOCUS => {
            if mc != 0 && wnd(mc).is_some() {
                client_proc(mc, W_SETFOCUS, 0, 0);
            }
            0
        }
        // While a child is maximized the title is "Frame - [Child]".
        W_SETTEXT if client(mc).is_some_and(|c| c.maximized != 0) => {
            let t = crate::far_str(lp).unwrap_or(b"").to_vec();
            if let Some(c) = client(mc) {
                c.base_title = Some(t);
            }
            set_title(mc);
            1
        }
        _ => user::def_window_proc(h, m, wp, lp),
    }
}

/// DefMDIChildProc(hwnd, msg, wParam, lParam)
fn def_child_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    let mc = client_of(h);
    match m {
        W_SETFOCUS | W_CHILDACTIVATE => {
            if client(mc).is_some_and(|c| c.active != h) {
                activate(mc, h);
            }
            user::def_window_proc(h, m, wp, lp)
        }
        W_MOUSEACTIVATE => {
            activate(mc, h);
            1
        }
        W_CLOSE => {
            user::send(mc, W_MDIDESTROY, h, 0);
            0
        }
        W_SYSCOMMAND => match wp & 0xfff0 {
            SC_NEXTWINDOW => {
                client_proc(mc, W_MDINEXT, h, 0);
                0
            }
            SC_CLOSE => {
                user::send(h, W_CLOSE, 0, 0);
                0
            }
            SC_MINIMIZE => {
                minimize_child(mc, h);
                0
            }
            SC_MAXIMIZE => {
                maximize(mc, h);
                activate(mc, h);
                0
            }
            SC_RESTORE => {
                restore_child(mc, h);
                activate(mc, h);
                0
            }
            _ => user::def_window_proc(h, m, wp, lp),
        },
        // A new title: in the Window menu, and the frame's while the
        // child is maximized.
        W_SETTEXT => {
            let r = user::def_window_proc(h, m, wp, lp);
            if client(mc).is_some_and(|c| c.maximized == h) {
                set_title(mc);
            }
            update_menu(mc);
            r
        }
        _ => user::def_window_proc(h, m, wp, lp),
    }
}

/// TranslateMDISysAccel(hwndClient, lpMsg): Ctrl+F4 closes the active
/// child, Ctrl+F6 (Ctrl+Tab) goes to the next one.
fn translate_accel(mc: u16, p: *const u8) -> bool {
    let Some((_, msg, wp, _)) = user::read_msg(p) else { return false };
    if msg != W_KEYDOWN && msg != W_SYSKEYDOWN || !user::key_down(0x11) {
        return false;
    }
    let Some(a) = client(mc).map(|c| c.active).filter(|&a| a != 0) else { return false };
    match wp {
        0x73 => {
            user::send(a, W_SYSCOMMAND, SC_CLOSE, 0);
            true
        }
        0x75 | 0x09 => {
            client_proc(mc, W_MDINEXT, a, user::key_down(0x10) as u32);
            true
        }
        _ => false,
    }
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // ArrangeIconicWindows(hwnd): the icons' height.
        2170 => {
            let h = a.w();
            if is_client(h) { arrange_icons(h) } else { 0 }
        }
        // DefFrameProc(hwnd, hwndMDIClient, msg, wParam, lParam)
        2445 => {
            let (h, mc, m, wp, lp) = (a.w(), a.w(), a.w(), a.w(), a.l());
            def_frame_proc(h, mc, m, wp, lp)
        }
        // DefMDIChildProc(hwnd, msg, wParam, lParam)
        2447 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            def_child_proc(h, m, wp, lp)
        }
        // TranslateMDISysAccel(hwndClient, lpMsg)
        2451 => {
            let (mc, p) = (a.w(), a.p());
            translate_accel(mc, p) as u32
        }
        _ => return None,
    })
}
