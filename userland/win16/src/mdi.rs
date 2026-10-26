//! The multiple document interface: the MDICLIENT class, DefFrameProc,
//! DefMDIChildProc and TranslateMDISysAccel.
//!
//! An MDI child is a PM frame window (title bar, system menu, sizing
//! border, minimise and maximise buttons from its style) inside the MDI
//! client's PM window, so PM moves, sizes, minimises and maximises it
//! within the client. Activation among the children is the client's:
//! the active child is on top with its title bar highlighted, gets the
//! focus and WM_MDIACTIVATE, and is checked in the Window menu, where the
//! client lists the children (IDs from idFirstChild on).

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
const W_CLOSE: u16 = 0x0010;
const W_CHILDACTIVATE: u16 = 0x0022;
const W_COMMAND: u16 = 0x0111;
const W_SYSCOMMAND: u16 = 0x0112;
const W_MOUSEACTIVATE: u16 = 0x0021;
const W_KEYDOWN: u16 = 0x0100;
const W_SYSKEYDOWN: u16 = 0x0104;

const SC_CLOSE: u16 = 0xf060;
const SC_NEXTWINDOW: u16 = 0xf040;
const SC_MINIMIZE: u16 = 0xf020;
const SC_MAXIMIZE: u16 = 0xf030;
const SC_RESTORE: u16 = 0xf120;

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
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
/// The ID of the separator before the children in the Window menu.
const SEPARATOR: u16 = 0xf7ff;
const TBM_SETHILITE: u32 = 0x01e3;

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

/// Make a child the active one.
pub fn activate(h: u16, child: u16) {
    let Some(c) = client(h) else { return };
    let old = c.active;
    if !c.children.contains(&child) {
        return;
    }
    c.active = child;
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
    let step = 20;
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
    if let Some(c) = client(h) {
        c.children.push(hwnd);
    }
    user::show_window(hwnd, if style & WS_MAXIMIZE != 0 { 3 } else if style & WS_MINIMIZE != 0 { 7 } else { 5 });
    activate(h, hwnd);
    hwnd
}

/// A child is going: the next one becomes active.
fn destroy(h: u16, child: u16) {
    let Some(c) = client(h) else { return };
    let Some(i) = c.children.iter().position(|&k| k == child) else { return };
    let was_active = c.active == child;
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
    user::destroy_window(child);
    if was_active {
        if let Some(&n) = client(h).and_then(|c| c.children.last()) {
            activate(h, n);
        }
    }
    update_menu(h);
}

/// Tile (vertical: side by side) or cascade the children.
fn arrange(h: u16, tile: bool) {
    let Some(c) = client(h) else { return };
    let kids: Vec<u16> = c.children.iter().copied().filter(|&k| !user::is_iconic(k)).collect();
    let (cw, ch) = user::client_size(h);
    if kids.is_empty() {
        return;
    }
    if tile {
        let n = kids.len() as i32;
        let cols = (1..=n).find(|c| c * c >= n).unwrap_or(1);
        let rows = (n + cols - 1) / cols;
        for (i, &k) in kids.iter().enumerate() {
            let (col, row) = (i as i32 % cols, i as i32 / cols);
            user::show_window(k, 9);
            user::move_to(k, [col * cw / cols, row * ch / rows, (col + 1) * cw / cols, (row + 1) * ch / rows]);
        }
    } else {
        for (i, &k) in kids.iter().enumerate() {
            let o = i as i32 * 20;
            user::show_window(k, 9);
            user::move_to(k, [o, o, o + cw * 3 / 4, o + ch * 3 / 4]);
        }
        if let Some(c) = client(h) {
            c.next = kids.len() as i32;
        }
    }
    if let Some(&a) = kids.last() {
        activate(h, a);
    }
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
                w.data = Some(alloc::boxed::Box::new(Client { window_menu: menu, first_id: first, children: Vec::new(), active: 0, next: 0, listed: 0 }));
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
            let a = client(h).map_or(0, |c| c.active);
            (a as u32) | ((a != 0 && user::is_zoomed(a)) as u32) << 16
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
            user::show_window(wp, 3);
            activate(h, wp);
            0
        }
        W_MDIRESTORE => {
            user::show_window(wp, 9);
            0
        }
        W_MDITILE | W_MDICASCADE => {
            arrange(h, m == W_MDITILE);
            1
        }
        W_MDIICONARRANGE => 0,
        // WM_MDISETMENU(fRefresh, MAKELONG(hMenuFrame, hMenuWindow)): the
        // frame's old menu.
        W_MDISETMENU => {
            let (frame_menu, window_menu) = (lp as u16, (lp >> 16) as u16);
            let frame = wnd(h).map_or(0, |w| w.parent);
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
            if frame_menu != 0 {
                crate::menu::set_menu(frame, frame_menu);
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
        // The Window menu's children; system commands of a maximised
        // child.
        W_COMMAND => {
            if let Some(c) = client(mc) {
                let i = wp.wrapping_sub(c.first_id) as usize;
                if let Some(&k) = c.children.get(i) {
                    if user::is_iconic(k) {
                        user::show_window(k, 9);
                    }
                    activate(mc, k);
                    return 0;
                }
                let a = c.active;
                if wp >= 0xf000 && a != 0 && user::is_zoomed(a) {
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
                user::show_window(h, 6);
                0
            }
            SC_MAXIMIZE => {
                user::show_window(h, 3);
                0
            }
            SC_RESTORE => {
                user::show_window(h, 9);
                0
            }
            _ => user::def_window_proc(h, m, wp, lp),
        },
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
