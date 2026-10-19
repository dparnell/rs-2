//! The frame window (WC_FRAME) and its controls: title bar (WC_TITLEBAR),
//! system menu, min/max buttons, menu bar and scroll bars, laid out
//! around the client. Moving and sizing with the mouse.

use gpi::{Point, Rect};
use pmwin::{ws, HWND_DESKTOP};

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{post, send, WinDefWindowProc};
use crate::state::{st, Drag, Frame};
use crate::win::{self, WinSetWindowPos, WinWindowFromID, WM_BUTTON1DOWN, WM_CHAR, WM_CLOSE, WM_PAINT, WM_SYSCOMMAND};
use crate::{accel, color, menu};

pub const FCF_TITLEBAR: u32 = 0x0001;
pub const FCF_SYSMENU: u32 = 0x0002;
pub const FCF_MENU: u32 = 0x0004;
pub const FCF_SIZEBORDER: u32 = 0x0008;
pub const FCF_MINBUTTON: u32 = 0x0010;
pub const FCF_MAXBUTTON: u32 = 0x0020;
pub const FCF_VERTSCROLL: u32 = 0x0040;
pub const FCF_HORZSCROLL: u32 = 0x0080;
pub const FCF_DLGBORDER: u32 = 0x0100;
pub const FCF_BORDER: u32 = 0x0200;
pub const FCF_SHELLPOSITION: u32 = 0x0400;
pub const FCF_ICON: u32 = 0x4000;
pub const FCF_ACCELTABLE: u32 = 0x8000;
pub const FCF_HIDEBUTTON: u32 = 0x0100_0000;

pub const FID_SYSMENU: u32 = 0x8002;
pub const FID_TITLEBAR: u32 = 0x8003;
pub const FID_MINMAX: u32 = 0x8004;
pub const FID_MENU: u32 = 0x8005;
pub const FID_VERTSCROLL: u32 = 0x8006;
pub const FID_HORZSCROLL: u32 = 0x8007;
pub const FID_CLIENT: u32 = 0x8008;

pub const SC_SIZE: u32 = 0x8000;
pub const SC_MOVE: u32 = 0x8001;
pub const SC_MINIMIZE: u32 = 0x8002;
pub const SC_MAXIMIZE: u32 = 0x8003;
pub const SC_CLOSE: u32 = 0x8004;
pub const SC_APPMENU: u32 = 0x8006;
pub const SC_SYSMENU: u32 = 0x8007;
pub const SC_RESTORE: u32 = 0x8008;
pub const SC_TASKMANAGER: u32 = 0x8011;
pub const SC_HIDE: u32 = 0x802a;

const WM_ACTIVATE: u32 = 0x000d;
pub const WM_MINMAXFRAME: u32 = 0x0046;
pub const WM_SETICON: u32 = 0x0047;
pub const WM_QUERYICON: u32 = 0x0048;
/// Posted to a minimized window viewer: mp1 = the frame, mp2 = 1 when
/// it was minimized, 0 when restored (rs2-private).
pub const WM_RS2_MINWINDOW: u32 = 0x0ff8;

pub const SWP_MINIMIZE: u32 = 0x0400;
pub const SWP_MAXIMIZE: u32 = 0x0800;
pub const SWP_RESTORE: u32 = 0x1000;
const TBM_SETHILITE: u32 = 0x01e3;
const TBM_QUERYHILITE: u32 = 0x01e4;

/// Height of a frame's title bar.
pub const TITLE_H: i32 = 22;
/// Height of the menu bar.
pub const MENU_H: i32 = 20;
/// Width of frame scroll bars.
pub const SCROLL_W: i32 = 16;

wndproc! {
    frame_entry => frame_proc;
    titlebar_entry => titlebar_proc;
}

pub fn fcf(h: u32) -> u32 {
    st().frames.get(&h).map_or(0, |f| f.fcf)
}

pub fn is_frame(h: u32) -> bool {
    st().frames.contains_key(&h)
}

/// Frame border width.
pub fn border(fcf: u32) -> i32 {
    if fcf & FCF_SIZEBORDER != 0 {
        4
    } else if fcf & FCF_DLGBORDER != 0 {
        3
    } else if fcf & FCF_BORDER != 0 {
        1
    } else {
        0
    }
}

/// Create a frame window with its controls (and no client).
#[allow(clippy::too_many_arguments)]
pub fn create(parent: u32, style: u32, fcf: u32, title: &[u8], pos: Rect, owner: u32, id: u32, hmod: u32) -> u32 {
    let style = style | ws::CLIPCHILDREN | ws::CLIPSIBLINGS;
    let frame = win::create(parent, b"#1", title, style & !ws::VISIBLE, pos, owner, id, 0);
    if frame == 0 {
        return 0;
    }
    st().frames.insert(frame, Frame { fcf, client: 0, accel: 0, focus_save: 0, restore: None, icon: 0, min: None, icon_pos: None });
    if fcf & FCF_ICON != 0 {
        let i = crate::pointer::WinLoadPointer(1, hmod, id);
        if let Some(f) = st().frames.get_mut(&frame) {
            f.icon = i;
        }
    }
    create_controls(frame, fcf, title, hmod, id);
    layout(frame);
    frame
}

fn create_controls(frame: u32, fcf: u32, title: &[u8], hmod: u32, id: u32) {
    let vis = ws::VISIBLE;
    if fcf & FCF_TITLEBAR != 0 {
        win::create(frame, b"#9", title, vis, Rect::default(), frame, FID_TITLEBAR, 0);
    }
    if fcf & FCF_SYSMENU != 0 {
        menu::create_sysmenu(frame, fcf);
    }
    if fcf & (FCF_MINBUTTON | FCF_MAXBUTTON | FCF_HIDEBUTTON) != 0 {
        menu::create_minmax(frame, fcf);
    }
    if fcf & FCF_MENU != 0 {
        let m = menu::WinLoadMenu(frame, hmod, id);
        if m != 0 {
            if let Some(w) = st().desk.get_mut(m) {
                w.id = FID_MENU;
            }
        }
    }
    if fcf & FCF_ACCELTABLE != 0 {
        let a = accel::WinLoadAccelTable(1, hmod, id);
        if let Some(f) = st().frames.get_mut(&frame) {
            f.accel = a;
        }
    }
    if fcf & FCF_VERTSCROLL != 0 {
        win::create(frame, b"#8", b"", vis | 1, Rect::default(), frame, FID_VERTSCROLL, 0);
    }
    if fcf & FCF_HORZSCROLL != 0 {
        win::create(frame, b"#8", b"", vis, Rect::default(), frame, FID_HORZSCROLL, 0);
    }
}

/// Place the frame controls and the client.
pub fn layout(frame: u32) {
    let s = st();
    let Some(f) = s.frames.get(&frame) else { return };
    let (fcf, client) = (f.fcf, f.client);
    if f.min.is_some() {
        return;
    }
    let (cx, cy) = s.desk.get(frame).map_or((0, 0), |w| (w.cx, w.cy));
    let b = border(fcf);
    let (mut l, mut bot, mut r, mut top) = (b, b, cx - b, cy - b);
    let sys = WinWindowFromID(frame, FID_SYSMENU);
    let title = WinWindowFromID(frame, FID_TITLEBAR);
    let minmax = WinWindowFromID(frame, FID_MINMAX);
    if sys != 0 || title != 0 || minmax != 0 {
        let y = top - TITLE_H;
        let mut tl = l;
        let mut tr = r;
        if sys != 0 {
            place(sys, Rect::new(l, y, l + TITLE_H, top));
            tl += TITLE_H;
        }
        if minmax != 0 {
            let n = menu::item_count(minmax).max(1);
            place(minmax, Rect::new(r - n * TITLE_H, y, r, top));
            tr -= n * TITLE_H;
        }
        if title != 0 {
            place(title, Rect::new(tl, y, tr, top));
        }
        top = y;
    }
    let m = WinWindowFromID(frame, FID_MENU);
    if m != 0 && st().desk.get(m).is_some_and(|w| w.visible_style()) {
        place(m, Rect::new(l, top - MENU_H, r, top));
        top -= MENU_H;
    }
    let vs = WinWindowFromID(frame, FID_VERTSCROLL);
    let hs = WinWindowFromID(frame, FID_HORZSCROLL);
    if hs != 0 {
        place(hs, Rect::new(l, bot, r - if vs != 0 { SCROLL_W } else { 0 }, bot + SCROLL_W));
        bot += SCROLL_W;
    }
    if vs != 0 {
        place(vs, Rect::new(r - SCROLL_W, bot, r, top));
        r -= SCROLL_W;
    }
    let _ = &mut l;
    if client != 0 {
        place(client, Rect::new(l, bot, r.max(l), top.max(bot)));
    }
}

/// Move/size a frame control (WM_SIZE when the size changes).
fn place(h: u32, r: Rect) {
    let Some(w) = st().desk.get(h) else { return };
    if (w.x, w.y, w.cx, w.cy) == (r.left, r.bottom, r.width(), r.height()) {
        return;
    }
    let old = (w.cx, w.cy);
    st().desk.set_pos(h, r.left, r.bottom, r.width(), r.height());
    if old != (r.width(), r.height()) {
        send(h, win::WM_SIZE, mp(old.0 as u32, old.1 as u32), mp(r.width() as u32, r.height() as u32));
    }
}

/// Client rectangle of a frame, in frame coordinates.
#[allow(dead_code)]
pub fn client_rect(frame: u32) -> Rect {
    let client = st().frames.get(&frame).map_or(0, |f| f.client);
    match st().desk.get(client) {
        Some(w) => Rect::new(w.x, w.y, w.x + w.cx, w.y + w.cy),
        None => {
            let (cx, cy) = ctl::size(frame);
            let b = border(fcf(frame));
            Rect::new(b, b, cx - b, cy - b)
        }
    }
}

fn paint(h: u32) {
    let f = fcf(h);
    let active = st().active == h;
    if is_minimized(h) {
        let (icon, title) = (icon(h), title_of(h));
        draw::paint(h, |c: &mut Canvas| draw_minimized(c, icon, &title, active));
        return;
    }
    draw::paint(h, |c: &mut Canvas| draw_frame(c, f, active));
}

/// The frame's icon (0: none).
pub fn icon(h: u32) -> u32 {
    st().frames.get(&h).map_or(0, |f| f.icon)
}

/// The text a frame shows as its title.
pub fn title_of(h: u32) -> alloc::vec::Vec<u8> {
    let tb = WinWindowFromID(h, FID_TITLEBAR);
    ctl::text(if tb != 0 { tb } else { h })
}

pub fn is_minimized(h: u32) -> bool {
    ctl::style(h) & ws::MINIMIZED != 0
}

/// A minimized frame on the desktop: its icon (the application icon if
/// it has none) with the title below, highlighted while active.
pub fn draw_minimized(c: &mut Canvas, icon: u32, title: &[u8], active: bool) {
    let r = c.rect();
    c.fill(r, color::sys(color::SYSCLR_BACKGROUND));
    let n = crate::pointer::ICON;
    let ir = Rect::new(r.left, r.top - n - 2, r.right, r.top - 2);
    let icon = if icon != 0 && crate::pointer::get(icon).is_some() { icon } else { crate::pointer::system(pmwin::sysptr::SPTR_APPICON) };
    crate::pointer::draw_in(c, ir, icon, false, false);
    let f = draw::font();
    let cols = (r.width() / f.width).max(1) as usize;
    let mut lines = draw::wrap(title, cols);
    lines.truncate(2);
    let tw = lines.iter().map(|l| draw::text_width(l)).max().unwrap_or(0);
    let th = lines.len() as i32 * f.height;
    let top = ir.bottom - 2;
    let tr = Rect::new(r.left + (r.width() - tw) / 2 - 2, top - th, r.left + (r.width() + tw) / 2 + 2, top);
    if active {
        c.fill(tr, draw::HILITE);
    }
    let fg = if active { draw::HILITE_TEXT } else { 0xffffff };
    c.text_in(Rect::new(r.left, top - th, r.right, top), &lines.join(&b'\n'), fg, draw::DT_CENTER | draw::DT_WORDBREAK);
}

/// Enable the system menu's items for the frame's state.
fn sysmenu_state(h: u32) {
    let st_ = ctl::style(h);
    let (min, max) = (st_ & ws::MINIMIZED != 0, st_ & ws::MAXIMIZED != 0);
    let f = fcf(h);
    let sys = WinWindowFromID(h, FID_SYSMENU);
    menu::enable_item(sys, SC_RESTORE, min || max);
    menu::enable_item(sys, SC_MINIMIZE, !min && f & FCF_MINBUTTON != 0);
    menu::enable_item(sys, SC_MAXIMIZE, !max && f & FCF_MAXBUTTON != 0);
    menu::enable_item(sys, SC_MOVE, !max);
    menu::enable_item(sys, SC_SIZE, !min && !max && f & FCF_SIZEBORDER != 0);
}

/// WM_MINMAXFRAME to the frame (mp1 = the frame, mp2 = an SWP with the
/// new state); not from the PM server, which never waits for programs.
fn minmax_frame(h: u32, fl: u32, pos: (i32, i32, i32, i32)) {
    if st().is_server() {
        return;
    }
    let swp: [u32; 9] = [fl, pos.3 as u32, pos.2 as u32, pos.1 as u32, pos.0 as u32, 0, h, 0, 0];
    send(h, WM_MINMAXFRAME, h, swp.as_ptr() as u32);
}

/// Minimize a frame: its controls and client are hidden and it becomes
/// an icon with its title at the bottom of the desktop, or, when a
/// minimized window viewer is registered, it is hidden and the viewer
/// is told.
pub fn minimize(h: u32) {
    let s = st();
    if !s.frames.contains_key(&h) || is_minimized(h) {
        return;
    }
    let Some(w) = s.desk.get(h) else { return };
    let pos = (w.x, w.y, w.cx, w.cy);
    let was_max = w.style & ws::MAXIMIZED != 0;
    let kids: alloc::vec::Vec<u32> = w.children.iter().copied().filter(|&k| s.desk.get(k).is_some_and(|x| x.visible_style())).collect();
    let viewer = s.min_viewer != 0 && s.desk.exists(s.min_viewer);
    let (sw, sh) = (s.surface.width, s.surface.height);
    // Where it goes: where it was last minimized, else the first free
    // place along the bottom.
    let taken: alloc::vec::Vec<Rect> = s
        .frames
        .iter()
        .filter(|&(&f, x)| f != h && x.min.as_ref().is_some_and(|m| !m.viewer))
        .filter_map(|(&f, _)| s.desk.get(f).map(|x| Rect::new(x.x, x.y, x.x + x.cx, x.y + x.cy)))
        .collect();
    let icon_pos = s.frames.get(&h).and_then(|f| f.icon_pos);
    let slot = match icon_pos {
        Some((x, y)) => Rect::new(x, y, x + pmwin::minimize::ICON_W, y + pmwin::minimize::ICON_H),
        None => pmwin::minimize::free_slot(&taken, sw, sh),
    };
    minmax_frame(h, SWP_MINIMIZE, (slot.left, slot.bottom, slot.width(), slot.height()));
    let s = st();
    for &k in &kids {
        s.desk.set_visible(k, false);
    }
    if let Some(f) = s.frames.get_mut(&h) {
        f.min = Some(crate::state::Minimized { pos, was_max, hidden: kids, viewer });
    }
    if let Some(w) = s.desk.get_mut(h) {
        w.style = (w.style & !ws::MAXIMIZED) | ws::MINIMIZED;
    }
    sysmenu_state(h);
    if viewer {
        win::show(h, false);
        let v = st().min_viewer;
        post(v, WM_RS2_MINWINDOW, h, 1);
    } else {
        st().desk.set_pos(h, slot.left, slot.bottom, slot.width(), slot.height());
        st().desk.to_bottom(h);
        st().desk.invalidate(h, None, false);
        if st().active == h {
            st().active = 0;
            st().focus = 0;
            win::activate_next();
        }
    }
}

/// Restore a minimized frame (or a maximized one).
pub fn restore(h: u32) {
    if !is_minimized(h) {
        maximize(h, false);
        return;
    }
    let Some(m) = st().frames.get_mut(&h).and_then(|f| f.min.take()) else { return };
    if !m.viewer {
        let cur = st().desk.get(h).map(|w| (w.x, w.y));
        if let Some(f) = st().frames.get_mut(&h) {
            f.icon_pos = cur;
        }
    }
    minmax_frame(h, SWP_RESTORE, m.pos);
    if let Some(w) = st().desk.get_mut(h) {
        w.style &= !ws::MINIMIZED;
        if m.was_max {
            w.style |= ws::MAXIMIZED;
        }
    }
    let s = st();
    for &k in &m.hidden {
        if s.desk.exists(k) {
            s.desk.set_visible(k, true);
        }
    }
    let (x, y, cx, cy) = m.pos;
    s.desk.set_pos(h, x, y, cx, cy);
    layout(h);
    sysmenu_state(h);
    win::show(h, true);
    st().desk.invalidate(h, None, true);
    win::activate(h);
    if m.viewer {
        let v = st().min_viewer;
        post(v, WM_RS2_MINWINDOW, h, 0);
    }
}

/// The frame's background and border.
pub fn draw_frame(c: &mut Canvas, f: u32, active: bool) {
    let r = c.rect();
    c.fill(r, draw::dialog_bg());
    let b = border(f);
    if b > 0 {
        c.frame(r, if active { draw::DARK } else { draw::SHADOW });
    }
    if b > 1 {
        let i = Rect::new(r.left + 1, r.bottom + 1, r.right - 1, r.top - 1);
        c.raised(i);
        let k = Rect::new(r.left + b - 1, r.bottom + b - 1, r.right - b + 1, r.top - b + 1);
        c.sunken(k);
    }
}

/// Maximize or restore a frame.
pub fn maximize(h: u32, on: bool) {
    if on && is_minimized(h) {
        restore(h);
    }
    let s = st();
    let Some(w) = s.desk.get(h) else { return };
    let cur = (w.x, w.y, w.cx, w.cy);
    let b = border(fcf(h));
    let (sw, sh) = (s.surface.width, s.surface.height);
    let Some(f) = s.frames.get_mut(&h) else { return };
    let target = if on {
        if f.restore.is_none() {
            f.restore = Some(cur);
        }
        (-b, -b, sw + 2 * b, sh + 2 * b)
    } else {
        match f.restore.take() {
            Some(r) => r,
            None => return,
        }
    };
    if let Some(w) = st().desk.get_mut(h) {
        if on {
            w.style |= ws::MAXIMIZED;
        } else {
            w.style &= !ws::MAXIMIZED;
        }
    }
    let mm = WinWindowFromID(h, FID_MINMAX);
    if on {
        menu::replace_id(mm, SC_MAXIMIZE, SC_RESTORE);
    } else {
        menu::replace_id(mm, SC_RESTORE, SC_MAXIMIZE);
    }
    sysmenu_state(h);
    WinSetWindowPos(h, 0, target.0, target.1, target.2, target.3, 0x0003);
}

/// Start moving (edges 0) or sizing a frame with the mouse, from the
/// position of the button press being processed. The PM server's input
/// thread does the tracking.
pub fn start_drag(h: u32, edges: u8) {
    let s = st();
    // Released already (the thread came late to the press).
    if s.buttons & 1 == 0 {
        return;
    }
    let ptr = s.pointer;
    let p = s.my_queue_mut().map_or(ptr, |q| q.msg_pos);
    start_drag_at(h, edges, p);
}

pub fn start_drag_at(h: u32, edges: u8, p: Point) {
    let s = st();
    let Some(w) = s.desk.get(h) else { return };
    if w.style & ws::MAXIMIZED != 0 {
        return;
    }
    s.drag = Some(Drag { hwnd: h, start: p, orig: (w.x, w.y, w.cx, w.cy), edges });
    s.capture = h;
    // The tracking catches up with where the pointer is now.
    let (now, buttons) = (s.pointer, s.buttons);
    if now != p {
        track(now, buttons);
    }
}

/// Pointer motion during a move or size; ends when button 1 is up.
pub fn track(p: Point, buttons: u8) {
    let s = st();
    let Some(d) = s.drag.as_ref() else { return };
    let (h, e) = (d.hwnd, d.edges);
    let (dx, dy) = (p.x - d.start.x, p.y - d.start.y);
    let (mut x, mut y, mut cx, mut cy) = d.orig;
    if buttons & 1 == 0 {
        s.drag = None;
        s.capture = 0;
    }
    if e == 0 {
        x += dx;
        y += dy;
    } else {
        let (minw, minh) = (120, TITLE_H + 2 * border(fcf(h)) + 8);
        if e & 1 != 0 {
            let ncx = (cx - dx).max(minw);
            x += cx - ncx;
            cx = ncx;
        }
        if e & 2 != 0 {
            cx = (cx + dx).max(minw);
        }
        if e & 4 != 0 {
            let ncy = (cy - dy).max(minh);
            y += cy - ncy;
            cy = ncy;
        }
        if e & 8 != 0 {
            cy = (cy + dy).max(minh);
        }
    }
    WinSetWindowPos(h, 0, x, y, cx, cy, 0x0003);
}

/// Which edges of a sizing border a point (frame coordinates) is on.
pub fn edges_at(h: u32, p: Point) -> u8 {
    let (cx, cy) = ctl::size(h);
    let k = border(fcf(h)) + 12;
    let mut e = 0;
    if p.x < k {
        e |= 1;
    }
    if p.x >= cx - k {
        e |= 2;
    }
    if p.y < k {
        e |= 4;
    }
    if p.y >= cy - k {
        e |= 8;
    }
    e
}

/// The frame forwards these from its controls to the client.
fn forwarded(msg: u32) -> bool {
    matches!(msg, WM_COMMAND | WM_HELP | WM_CONTROL | WM_VSCROLL | WM_HSCROLL | WM_INITMENU | WM_MENUSELECT | WM_MENUEND | WM_DRAWITEM | WM_MEASUREITEM)
}

pub extern "C" fn frame_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN if is_minimized(h) => {
            // The icon moves.
            win::activate(h);
            start_drag(h, 0);
            1
        }
        WM_BUTTON1DBLCLK if is_minimized(h) => {
            restore(h);
            1
        }
        WM_SETICON => {
            if let Some(f) = st().frames.get_mut(&h) {
                f.icon = mp1;
            }
            let sys = WinWindowFromID(h, FID_SYSMENU);
            if sys != 0 {
                ctl::invalidate(sys);
            }
            if is_minimized(h) {
                ctl::invalidate(h);
            }
            1
        }
        WM_QUERYICON => icon(h),
        WM_MINMAXFRAME => 0,
        WM_CHAR if is_minimized(h) => {
            let k = Key::from(mp1, mp2);
            if !k.up() && matches!(k.vk, VK_NEWLINE | VK_ENTER) {
                restore(h);
                return 1;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_BUTTON1DOWN => {
            if fcf(h) & FCF_SIZEBORDER != 0 {
                let e = edges_at(h, ctl::mouse_pos(mp1));
                if e != 0 {
                    start_drag(h, e);
                }
            }
            1
        }
        crate::win::WM_MOUSEMOVE => {
            use pmwin::sysptr::*;
            let e = if fcf(h) & FCF_SIZEBORDER != 0 && ctl::style(h) & (ws::MAXIMIZED | ws::MINIMIZED) == 0 { edges_at(h, ctl::mouse_pos(mp1)) } else { 0 };
            crate::pointer::set_system(match e {
                0 => SPTR_ARROW,
                1 | 2 => SPTR_SIZEWE,
                4 | 8 => SPTR_SIZENS,
                5 | 10 => SPTR_SIZENESW,
                _ => SPTR_SIZENWSE,
            });
            1
        }
        WM_ACTIVATE => {
            let t = WinWindowFromID(h, FID_TITLEBAR);
            if t != 0 {
                ctl::invalidate(t);
            }
            0
        }
        WM_UPDATEFRAME => {
            layout(h);
            0
        }
        // A frame given the focus passes it on to the window that had it
        // last (QWL_HWNDFOCUSSAVE), else to its client.
        crate::win::WM_SETFOCUS if mp2 & 0xffff != 0 => {
            let (save, client) = st().frames.get(&h).map_or((0, 0), |f| (f.focus_save, f.client));
            let to = if save != 0 && save != h && st().desk.is_ancestor(h, save) { save } else { client };
            if st().focus == h && to != 0 && st().desk.exists(to) {
                crate::win::set_focus(to);
            }
            0
        }
        WM_SYSCOMMAND => {
            match lo(mp1) {
                SC_CLOSE => {
                    let client = st().frames.get(&h).map_or(0, |f| f.client);
                    post(if client != 0 { client } else { h }, WM_CLOSE, 0, 0);
                }
                SC_MAXIMIZE => maximize(h, true),
                SC_RESTORE => restore(h),
                SC_MINIMIZE => minimize(h),
                SC_HIDE => win::show(h, false),
                SC_APPMENU => menu::start_bar(WinWindowFromID(h, FID_MENU)),
                SC_SYSMENU => menu::start_bar(WinWindowFromID(h, FID_SYSMENU)),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            let client = st().frames.get(&h).map_or(0, |f| f.client);
            if client != 0 {
                post(client, WM_CLOSE, 0, 0);
            } else {
                WinDefWindowProc(h, msg, mp1, mp2);
            }
            0
        }
        WM_CHAR => {
            let k = Key::from(mp1, mp2);
            if !k.up() && k.alt() && !k.ctrl() {
                let c = k.letter(mp1);
                if c != 0 && menu::open_mnemonic(WinWindowFromID(h, FID_MENU), c) {
                    return 1;
                }
            }
            0
        }
        m if forwarded(m) => {
            let client = st().frames.get(&h).map_or(0, |f| f.client);
            if client != 0 {
                send(client, msg, mp1, mp2)
            } else {
                // No client (a dialog): the default (WM_MENUSELECT lets
                // the command through).
                WinDefWindowProc(h, msg, mp1, mp2)
            }
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}

// ---- The title bar ----

fn titlebar_paint(h: u32) {
    let active = title_active(h);
    let title = ctl::text(h);
    let hung = st().is_hung(h);
    draw::paint(h, |c| draw_title(c, &title, active, hung));
}

/// The title bar shows its frame as active.
pub fn title_active(h: u32) -> bool {
    st().active == win::top_level(h) || ctl::data::<bool>(h).is_some_and(|b| *b)
}

/// A title bar; one of a frame whose thread does not respond is grey
/// and says so.
pub fn draw_title(c: &mut Canvas, title: &[u8], active: bool, hung: bool) {
    let r = c.rect();
    let (bg, fg) = if hung {
        (0xa0a0a0, 0xf0f0f0)
    } else if active {
        (color::sys(color::SYSCLR_ACTIVETITLE), color::sys(color::SYSCLR_ACTIVETITLETEXT))
    } else {
        (color::sys(color::SYSCLR_INACTIVETITLE), color::sys(color::SYSCLR_INACTIVETITLETEXT))
    };
    c.fill(r, bg);
    let mut t = title.to_vec();
    if hung {
        t.extend_from_slice(b" (not responding)");
    }
    c.text_in(Rect::new(r.left + 2, r.bottom, r.right - 2, r.top), &t, fg, draw::DT_CENTER | draw::DT_VCENTER);
}

pub extern "C" fn titlebar_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            titlebar_paint(h);
            0
        }
        WM_BUTTON1DOWN => {
            let frame = ctl::parent(h);
            if frame != HWND_DESKTOP {
                start_drag(frame, 0);
            }
            1
        }
        WM_BUTTON1DBLCLK => {
            let frame = ctl::parent(h);
            if fcf(frame) & FCF_MAXBUTTON != 0 {
                let max = ctl::style(frame) & ws::MAXIMIZED != 0;
                post(frame, WM_SYSCOMMAND, if max { SC_RESTORE } else { SC_MAXIMIZE }, 0);
            }
            1
        }
        TBM_SETHILITE => {
            ctl::set_data::<bool>(h, mp1 != 0);
            ctl::invalidate(h);
            1
        }
        TBM_QUERYHILITE => ctl::data::<bool>(h).map_or(0, |b| *b as u32),
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
