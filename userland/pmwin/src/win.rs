//! Windows: classes, creation and destruction, positions, focus and
//! activation, text, window words and painting.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::{Point, Rect};
use pmwin::{ws, HWND_DESKTOP};

use crate::color::{self, resolve};
use crate::gpiapi::{self, PointL, RectL};
use crate::msg::{notify, send};
use crate::pointer;
use crate::frame::{self, FCF_SHELLPOSITION, FID_CLIENT};
use crate::state::{pid, st, Class};

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SHOW: u32 = 0x0005;
pub const WM_SIZE: u32 = 0x0007;
pub const WM_ACTIVATE: u32 = 0x000d;
pub const WM_SETFOCUS: u32 = 0x000f;
pub const WM_SYSCOMMAND: u32 = 0x0021;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_TIMER: u32 = 0x0024;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_QUIT: u32 = 0x002a;
pub const WM_MOUSEMOVE: u32 = 0x0070;
pub const WM_BUTTON1DOWN: u32 = 0x0071;
pub const WM_BUTTON1UP: u32 = 0x0072;
pub const WM_CHAR: u32 = 0x007a;

const SWP_SIZE: u32 = 0x0001;
const SWP_MOVE: u32 = 0x0002;
const SWP_ZORDER: u32 = 0x0004;
const SWP_SHOW: u32 = 0x0008;
const SWP_HIDE: u32 = 0x0010;
const SWP_ACTIVATE: u32 = 0x0080;
const HWND_TOP: u32 = 3;


const QW_NEXT: i32 = 0;
const QW_PREV: i32 = 1;
const QW_TOP: i32 = 2;
const QW_BOTTOM: i32 = 3;
const QW_OWNER: i32 = 4;
const QW_PARENT: i32 = 5;

pub use frame::TITLE_H;

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let mut v = Vec::new();
    let mut i = 0;
    loop {
        let b = unsafe { *p.add(i) };
        if b == 0 {
            break;
        }
        v.push(b);
        i += 1;
    }
    v
}

/// Class name argument: a string, or WC_* (0xffff00nn).
fn class_name(p: *const u8) -> Vec<u8> {
    let v = p as u32;
    if v >> 16 == 0xffff {
        return alloc::format!("#{}", v & 0xffff).into_bytes();
    }
    cstr(p)
}

// ---- Classes ----

#[unsafe(no_mangle)]
pub extern "C" fn WinRegisterClass(_hab: u32, name: *const u8, proc_: u32, style: u32, words: u32) -> u32 {
    let s = st();
    let name = class_name(name);
    if name.is_empty() {
        return 0;
    }
    // Classes belong to the process that registers them.
    let me = pid();
    match s.classes.iter_mut().find(|c| c.pid == me && c.name.eq_ignore_ascii_case(&name)) {
        Some(c) => {
            c.proc_ = proc_;
            c.style = style;
            c.words = words;
        }
        None => s.classes.push(Class { name, proc_, style, words, pid: me }),
    }
    1
}

/// The window procedure of a window's class (as registered, before any
/// subclassing).
pub fn class_proc(h: u32) -> u32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return 0 };
    s.classes.get(w.class as usize).map_or(0, |c| c.proc_)
}

/// Built-in classes registered on initialisation.
pub fn register_builtin() {
    crate::ctl::register_builtin();
}

// ---- Creation and destruction ----

#[allow(clippy::too_many_arguments)]
pub fn create(parent: u32, class: &[u8], text: &[u8], style: u32, pos: Rect, owner: u32, id: u32, ctl: u32) -> u32 {
    let s = st();
    let Some(c) = s.class(class) else { return 0 };
    let (proc_, cstyle, words) = (c.proc_, c.style, c.words);
    let class_id = s.classes.iter().position(|x| core::ptr::eq(x, c)).unwrap_or(0) as u32;
    let queue = s.my_queue();
    // Class styles CS_CLIPCHILDREN etc. are window styles too.
    let style = style | (cstyle & 0x3e00_0000);
    let parent = if parent == 0 { HWND_DESKTOP } else { parent };
    let Some(h) = s.desk.create(parent, owner, class_id, style, pos, id, queue, proc_, words.div_ceil(4) as usize + 1) else {
        return 0;
    };
    if let Some(w) = s.desk.get_mut(h) {
        w.text = text.to_vec();
    }
    // WM_CREATE: mp1 = control data, mp2 = CREATESTRUCT (pCtlData,
    // pPresParams, id, hwndInsertBehind, hwndOwner, cy, cx, y, x,
    // flStyle, pszText, pszClass, hwndParent).
    let mut tz = text.to_vec();
    tz.push(0);
    let mut cz = class.to_vec();
    cz.push(0);
    let cs: [u32; 13] = [
        ctl, 0, id, 3, owner, pos.height() as u32, pos.width() as u32, pos.bottom as u32, pos.left as u32, style, tz.as_ptr() as u32, cz.as_ptr() as u32, parent,
    ];
    if send(h, WM_CREATE, ctl, cs.as_ptr() as u32) != 0 {
        // A non-zero result from WM_CREATE fails the creation.
        destroy(h);
        return 0;
    }
    if style & ws::VISIBLE != 0 {
        let r = st().desk.screen_rect(h);
        st().desk.invalidate_screen(r);
    }
    h
}

pub fn destroy(h: u32) {
    let s = st();
    if !s.desk.exists(h) || h == HWND_DESKTOP {
        return;
    }
    let mut all = Vec::new();
    collect(h, &mut all);
    for &w in &all {
        // Windows of other threads are told, not waited for.
        notify(w, WM_DESTROY, 0, 0);
    }
    remove(h);
}

/// Take a window and its descendants out of the tree, with everything
/// that refers to them (no messages: also for windows of a process that
/// has ended).
pub fn remove(h: u32) {
    let s = st();
    if !s.desk.exists(h) || h == HWND_DESKTOP {
        return;
    }
    let r = s.desk.screen_rect(h);
    let showing = s.desk.is_showing(h);
    let gone = s.desk.destroy(h);
    let was_active = gone.contains(&s.active);
    for g in &gone {
        s.frames.remove(g);
        s.ctl.remove(g);
        s.pres.remove(g);
        s.timers.retain(|t| t.hwnd != *g);
        if s.focus == *g {
            s.focus = 0;
        }
        if s.active == *g {
            s.active = 0;
        }
        if s.capture == *g {
            s.capture = 0;
        }
        if s.drag.as_ref().is_some_and(|d| d.hwnd == *g) {
            s.drag = None;
        }
    }
    for q in s.queues.values_mut() {
        q.msgs.retain(|m| !gone.contains(&m.hwnd));
        q.sent.retain(|m| !gone.contains(&m.hwnd) || m.reply);
    }
    if showing {
        s.desk.invalidate_screen(r);
    }
    crate::menu::forget(&gone);
    // The next window up becomes active when the active one went.
    if was_active {
        activate_next();
    }
}

/// Activate the topmost visible frame (after the active window went away).
pub fn activate_next() {
    let s = st();
    let kids = s.desk.get(HWND_DESKTOP).map_or(Vec::new(), |w| w.children.clone());
    let next = kids.iter().rev().copied().find(|&c| s.desk.get(c).is_some_and(|w| w.visible_style() && w.style & ws::DISABLED == 0) && s.frames.contains_key(&c));
    if let Some(n) = next {
        activate(n);
    }
}

/// Top-level windows (children of the desktop and of the object window)
/// of a message queue, and windows of the queue whose parent belongs to
/// another queue.
pub fn top_windows_of(q: u32) -> Vec<u32> {
    let s = st();
    s.desk
        .windows_of(q)
        .into_iter()
        .filter(|&h| s.desk.get(h).is_some_and(|w| w.parent == HWND_DESKTOP || w.parent == pmwin::HWND_OBJECT || s.queue_of(w.parent) != q))
        .collect()
}

fn collect(h: u32, out: &mut Vec<u32>) {
    out.push(h);
    let kids = st().desk.get(h).map_or(Vec::new(), |w| w.children.clone());
    for k in kids {
        collect(k, out);
    }
}

/// WinCreateWindow(hwndParent, pszClass, pszName, flStyle, x, y, cx, cy,
/// hwndOwner, hwndInsertBehind, id, pCtlData, pPresParams)
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateWindow(
    parent: u32,
    class: *const u8,
    name: *const u8,
    style: u32,
    x: i32,
    y: i32,
    cx: i32,
    cy: i32,
    owner: u32,
    _behind: u32,
    id: u32,
    ctl: u32,
    _pres: u32,
) -> u32 {
    let class = class_name(class);
    let pos = Rect::new(x, y, x + cx, y + cy);
    if class == b"#1" {
        // FRAMECDATA: cb, flCreateFlags, hmodResources, idResources.
        let (fcf, hmod) = if ctl == 0 {
            (0, 0)
        } else {
            let p = ctl as *const u8;
            unsafe { ((p.add(2) as *const u32).read_unaligned(), (p.add(6) as *const u16).read_unaligned() as u32) }
        };
        let h = frame::create(parent, style, fcf, &cstr(name), pos, owner, id, hmod);
        if h != 0 && style & ws::VISIBLE != 0 {
            show(h, true);
        }
        return h;
    }
    create(parent, &class, &cstr(name), style, pos, owner, id, ctl)
}

/// WinCreateStdWindow(hwndParent, flStyle, pflCreateFlags, pszClientClass,
/// pszTitle, flClientStyle, hmod, idResources, phwndClient)
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateStdWindow(
    parent: u32,
    style: u32,
    pfl: *mut u32,
    client_class: *const u8,
    title: *const u8,
    client_style: u32,
    hmod: u32,
    id: u32,
    phwnd_client: *mut u32,
) -> u32 {
    let fcf = if pfl.is_null() { 0 } else { unsafe { pfl.read_unaligned() } };
    let mut pos = Rect::default();
    if fcf & FCF_SHELLPOSITION != 0 {
        pos = shell_position();
    }
    let frame = frame::create(parent, style, fcf, &cstr(title), pos, 0, id, hmod);
    if frame == 0 {
        return 0;
    }
    let class = class_name(client_class);
    if !class.is_empty() {
        let client = create(frame, &class, b"", client_style | ws::VISIBLE, Rect::default(), frame, FID_CLIENT, 0);
        if client == 0 {
            destroy(frame);
            return 0;
        }
        if let Some(f) = st().frames.get_mut(&frame) {
            f.client = client;
        }
        if !phwnd_client.is_null() {
            unsafe { phwnd_client.write_unaligned(client) };
        }
        frame::layout(frame);
    }
    if style & ws::VISIBLE != 0 {
        show(frame, true);
        activate(frame);
    }
    frame
}

/// Where the shell puts the next program's window: a cascade.
fn shell_position() -> Rect {
    let s = st();
    let (w, h) = (s.surface.width, s.surface.height);
    let off = 32 + s.cascade * 24;
    s.cascade = (s.cascade + 1) % 8;
    Rect::new(off, h / 3 - off / 2, off + w * 3 / 5, h / 3 - off / 2 + h * 3 / 5)
}

/// Rs2SetMinimizeViewer(hwnd) (rs2-private): from now on minimized
/// frames are hidden instead of becoming desktop icons, and `hwnd` gets
/// WM_RS2_MINWINDOW (0x0ff8: mp1 = the frame, mp2 = 1 minimized, 0
/// restored) for each; 0 goes back to desktop icons. Returns the
/// previous viewer. For a desktop shell's minimized window viewer.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2SetMinimizeViewer(h: u32) -> u32 {
    let s = st();
    if h != 0 && !s.desk.exists(h) {
        return 0;
    }
    core::mem::replace(&mut s.min_viewer, h)
}

/// Rs2SetDesktopWindow(hwnd) (rs2-private): keep a top-level window of
/// a desktop shell below every other window (activating it does not
/// raise it; windows sent to the bottom, such as minimized ones, go just
/// above it); 0 ends that. Returns the previous one.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2SetDesktopWindow(h: u32) -> u32 {
    let s = st();
    if h != 0 && s.desk.get(h).is_none_or(|w| w.parent != HWND_DESKTOP) {
        return 0;
    }
    let r = s.desk.screen_rect(h);
    let old = s.desk.pin_bottom(h);
    if h != 0 {
        s.desk.invalidate_screen(r);
    }
    old
}

/// WinWindowFromPoint(hwndParent, pptl, fChildren): the topmost showing
/// child of the parent under the point (in the parent's coordinates);
/// with fChildren the deepest descendant. The parent itself if no
/// child is there, 0 if the point is outside the parent.
#[unsafe(no_mangle)]
pub extern "C" fn WinWindowFromPoint(parent: u32, pptl: *const PointL, children: u32) -> u32 {
    let s = st();
    let parent = if parent == 0 { HWND_DESKTOP } else { parent };
    if pptl.is_null() || !s.desk.exists(parent) {
        return 0;
    }
    let p = unsafe { pptl.read_unaligned() };
    let o = s.desk.origin(parent);
    let at = Point::new(o.x + p.x, o.y + p.y);
    if !s.desk.screen_rect(parent).contains(at) {
        return 0;
    }
    let mut cur = parent;
    'down: loop {
        let kids = s.desk.get(cur).map_or(Vec::new(), |w| w.children.clone());
        for &c in kids.iter().rev() {
            if s.desk.get(c).is_some_and(|x| x.visible_style()) && s.desk.screen_rect(c).contains(at) {
                if children == 0 {
                    return c;
                }
                cur = c;
                continue 'down;
            }
        }
        return cur;
    }
}

/// WinQueryWindowProcess(hwnd, ppid, ptid): the process and thread that
/// own a window.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowProcess(h: u32, ppid: *mut u32, ptid: *mut u32) -> u32 {
    let s = st();
    let Some(q) = s.desk.get(h).and_then(|w| s.queues.get(&w.queue)) else { return 0 };
    unsafe {
        if !ppid.is_null() {
            ppid.write_unaligned(q.pid);
        }
        if !ptid.is_null() {
            ptid.write_unaligned(q.tid);
        }
    }
    1
}

/// Rs2ShellPosition(prcl) (rs2-private, for PMSHAPI's
/// WinQueryTaskSizePos): the next cascade position.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2ShellPosition(prcl: *mut RectL) -> u32 {
    if prcl.is_null() || crate::state::get().is_none() {
        return 0;
    }
    unsafe { prcl.write_unaligned(RectL::from(shell_position())) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyWindow(h: u32) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    destroy(h);
    1
}

// ---- Position, visibility, z-order, activation ----

pub fn show(h: u32, on: bool) {
    let was = st().desk.get(h).is_some_and(|w| w.visible_style());
    if was != on {
        st().desk.set_visible(h, on);
        notify(h, WM_SHOW, on as u32, 0);
        // A hidden active window gives up the activation.
        if !on && st().active == h {
            st().active = 0;
            if st().desk.is_ancestor(h, st().focus) {
                st().focus = 0;
            }
            activate_next();
        }
    }
}

/// The top-level window (child of the desktop) containing `h`.
pub fn top_level(h: u32) -> u32 {
    let s = st();
    let mut cur = h;
    while let Some(w) = s.desk.get(cur) {
        if w.parent == HWND_DESKTOP || w.parent == 0 {
            return cur;
        }
        cur = w.parent;
    }
    cur
}

/// Make a top-level window active: on top, focus on its client.
pub fn activate(h: u32) {
    let top = top_level(h);
    if top == HWND_DESKTOP || top == 0 {
        return;
    }
    let s = st();
    let old = s.active;
    s.desk.to_top(top);
    s.active = top;
    let saved = s.frames.get(&top).map_or(0, |f| f.focus_save);
    let client = s.frames.get(&top).map_or(top, |f| if f.client != 0 { f.client } else { top });
    let client = if saved != 0 && saved != top && s.desk.is_ancestor(top, saved) { saved } else { client };
    if old != top {
        if old != 0 && s.desk.exists(old) {
            notify(old, WM_ACTIVATE, 0, old);
            repaint_title(old);
        }
        // The border shows the activation too.
        st().desk.invalidate(top, None, false);
        if old != 0 {
            st().desk.invalidate(old, None, false);
        }
        notify(top, WM_ACTIVATE, 1, top);
        repaint_title(top);
    }
    let f = st().focus;
    if f != top && f != 0 && st().desk.is_ancestor(top, f) {
        // The focus is in the window already.
        return;
    }
    // A minimized frame keeps the focus itself (its client is hidden).
    if frame::is_minimized(top) {
        set_focus(top);
    } else if st().frames.contains_key(&top) && client != top {
        // A frame gets the focus and passes it on (to QWL_HWNDFOCUSSAVE
        // or its client), which so hears that it came from the frame.
        set_focus(top);
    } else {
        set_focus(client);
    }
}

/// A frame's title bar shows whether it is active (also while its
/// thread does not respond).
fn repaint_title(frame: u32) {
    let t = WinWindowFromID(frame, frame::FID_TITLEBAR);
    if t != 0 {
        st().desk.invalidate(t, None, false);
    }
}

pub fn set_focus(h: u32) {
    let old = st().focus;
    if old == h {
        return;
    }
    st().focus = h;
    let top = top_level(h);
    if let Some(f) = st().frames.get_mut(&top).filter(|_| top != h) {
        f.focus_save = h;
    }
    if old != 0 && st().desk.exists(old) {
        notify(old, WM_SETFOCUS, h, 0);
    }
    if h != 0 {
        notify(h, WM_SETFOCUS, old, 1);
    }
}

/// WinSetWindowPos(hwnd, hwndInsertBehind, x, y, cx, cy, fl)
#[unsafe(no_mangle)]
pub extern "C" fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32 {
    // Minimize, maximize and restore (frames): they size the window.
    let fl = if st().frames.contains_key(&h) && fl & (frame::SWP_MINIMIZE | frame::SWP_MAXIMIZE | frame::SWP_RESTORE) != 0 {
        if fl & frame::SWP_MINIMIZE != 0 {
            frame::minimize(h);
        } else if fl & frame::SWP_MAXIMIZE != 0 {
            frame::maximize(h, true);
        } else {
            frame::restore(h);
        }
        fl & !(SWP_MOVE | SWP_SIZE)
    } else {
        fl
    };
    let s = st();
    let Some(w) = s.desk.get(h) else { return 0 };
    let (ox, oy, ocx, ocy) = (w.x, w.y, w.cx, w.cy);
    let (nx, ny) = if fl & SWP_MOVE != 0 { (x, y) } else { (ox, oy) };
    let (ncx, ncy) = if fl & SWP_SIZE != 0 { (cx, cy) } else { (ocx, ocy) };
    if (nx, ny, ncx, ncy) != (ox, oy, ocx, ocy) {
        s.desk.set_pos(h, nx, ny, ncx, ncy);
        if s.frames.contains_key(&h) {
            frame::layout(h);
        } else if (ncx, ncy) != (ocx, ocy) {
            let pack = |a: i32, b: i32| (a as u32 & 0xffff) | (b as u32) << 16;
            send(h, WM_SIZE, pack(ocx, ocy), pack(ncx, ncy));
        }
    }
    if fl & SWP_SHOW != 0 {
        show(h, true);
    }
    if fl & SWP_HIDE != 0 {
        show(h, false);
    }
    if fl & SWP_ZORDER != 0 && behind == HWND_TOP {
        st().desk.to_top(h);
    }
    // HWND_BOTTOM: below its siblings (above a pinned desktop window).
    if fl & SWP_ZORDER != 0 && behind == 4 {
        st().desk.to_bottom(h);
    }
    if fl & SWP_ACTIVATE != 0 {
        activate(h);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinShowWindow(h: u32, on: u32) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    show(h, on != 0);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetActiveWindow(_desk: u32, h: u32) -> u32 {
    activate(h);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryActiveWindow(_desk: u32) -> u32 {
    st().active
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetFocus(_desk: u32, h: u32) -> u32 {
    let top = top_level(h);
    if top != st().active {
        activate(top);
    }
    set_focus(h);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinFocusChange(_desk: u32, h: u32, _fl: u32) -> u32 {
    WinSetFocus(0, h)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryFocus(_desk: u32) -> u32 {
    st().focus
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowRect(h: u32, prcl: *mut RectL) -> u32 {
    let Some(w) = st().desk.get(h) else { return 0 };
    if prcl.is_null() {
        return 0;
    }
    unsafe { prcl.write_unaligned(RectL::from(w.rect())) };
    1
}

/// SWP: fl, cy, cx, y, x, hwndInsertBehind, hwnd, ulReserved1, ulReserved2.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowPos(h: u32, pswp: *mut u32) -> u32 {
    let Some(w) = st().desk.get(h) else { return 0 };
    if pswp.is_null() {
        return 0;
    }
    // fl reports the minimized/maximized state (SWP_MINIMIZE, SWP_MAXIMIZE).
    let fl = if w.style & ws::MINIMIZED != 0 {
        frame::SWP_MINIMIZE
    } else if w.style & ws::MAXIMIZED != 0 {
        frame::SWP_MAXIMIZE
    } else {
        0
    };
    let vals = [fl, w.cy as u32, w.cx as u32, w.y as u32, w.x as u32, 0, h, 0, 0];
    for (i, v) in vals.into_iter().enumerate() {
        unsafe { pswp.add(i).write_unaligned(v) };
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinIsWindow(_hab: u32, h: u32) -> u32 {
    st().desk.exists(h) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinIsWindowVisible(h: u32) -> u32 {
    st().desk.is_showing(h) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinEnableWindow(h: u32, on: u32) -> u32 {
    let Some(w) = st().desk.get_mut(h) else { return 0 };
    let old = w.style;
    if on != 0 {
        w.style &= !ws::DISABLED;
    } else {
        w.style |= ws::DISABLED;
    }
    // Controls draw themselves enabled or greyed: repaint on a change.
    if w.style != old {
        st().desk.invalidate(h, None, true);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinIsWindowEnabled(h: u32) -> u32 {
    st().desk.get(h).is_some_and(|w| w.style & ws::DISABLED == 0) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinIsChild(h: u32, parent: u32) -> u32 {
    (h != parent && st().desk.is_ancestor(parent, h)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinWindowFromID(parent: u32, id: u32) -> u32 {
    let s = st();
    let Some(p) = s.desk.get(parent) else { return 0 };
    p.children.iter().copied().find(|&c| s.desk.get(c).is_some_and(|w| w.id == id)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindow(h: u32, cmd: i32) -> u32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return 0 };
    let siblings = s.desk.get(w.parent).map_or(Vec::new(), |p| p.children.clone());
    let at = siblings.iter().position(|&c| c == h);
    match cmd {
        // QW_NEXT is the window below in z-order, QW_PREV the one above.
        QW_NEXT => at.and_then(|i| i.checked_sub(1)).map_or(0, |i| siblings[i]),
        QW_PREV => at.and_then(|i| siblings.get(i + 1).copied()).unwrap_or(0),
        QW_TOP => w.children.last().copied().unwrap_or(0),
        QW_BOTTOM => w.children.first().copied().unwrap_or(0),
        QW_OWNER => w.owner,
        QW_PARENT => w.parent,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryDesktopWindow(_hab: u32, _hdc: u32) -> u32 {
    HWND_DESKTOP
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetParent(h: u32, parent: u32, _redraw: u32) -> u32 {
    let parent = if parent == 0 { HWND_DESKTOP } else { parent };
    st().desk.set_parent(h, parent) as u32
}

/// WinMapWindowPoints(hwndFrom, hwndTo, aptl, cwpt)
#[unsafe(no_mangle)]
pub extern "C" fn WinMapWindowPoints(from: u32, to: u32, pts: *mut PointL, n: i32) -> u32 {
    let s = st();
    let a = if from == 0 { Point::new(0, 0) } else { s.desk.origin(from) };
    let b = if to == 0 { Point::new(0, 0) } else { s.desk.origin(to) };
    if pts.is_null() {
        return 0;
    }
    for i in 0..n.max(0) as usize {
        unsafe {
            let p = pts.add(i);
            let mut v = p.read_unaligned();
            v.x += a.x - b.x;
            v.y += a.y - b.y;
            p.write_unaligned(v);
        }
    }
    1
}

// ---- Window text and words ----

#[unsafe(no_mangle)]
pub extern "C" fn WinSetWindowText(h: u32, text: *const u8) -> u32 {
    set_text(h, &cstr(text))
}

/// Set a window's text; a frame and its title bar share theirs.
pub fn set_text(h: u32, t: &[u8]) -> u32 {
    if crate::ctl::set_text_of(h, t) {
        return 1;
    }
    let s = st();
    let Some(w) = s.desk.get_mut(h) else { return 0 };
    w.text = t.to_vec();
    let (id, parent) = (w.id, w.parent);
    s.desk.invalidate(h, None, false);
    let other = if s.frames.contains_key(&h) {
        WinWindowFromID(h, frame::FID_TITLEBAR)
    } else if id == frame::FID_TITLEBAR && s.frames.contains_key(&parent) {
        parent
    } else {
        0
    };
    if let Some(w) = st().desk.get_mut(other) {
        w.text = t.to_vec();
        st().desk.invalidate(other, None, false);
    }
    crate::entry::text_changed(h);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32 {
    if let Some(t) = crate::ctl::text_of(h) {
        return if cch <= 0 { 0 } else { crate::ctl::copy_out(&t, cch as usize, buf) as i32 };
    }
    let Some(w) = st().desk.get(h) else { return 0 };
    if buf.is_null() || cch <= 0 {
        return 0;
    }
    let n = w.text.len().min(cch as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(w.text.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowTextLength(h: u32) -> i32 {
    if let Some(t) = crate::ctl::text_of(h) {
        return t.len() as i32;
    }
    st().desk.get(h).map_or(0, |w| w.text.len() as i32)
}

/// Window words: QWL_USER (0) and the class's extra bytes at byte offsets;
/// QWL_STYLE (-2), QWS_ID (-1).
fn word_get(h: u32, index: i32) -> Option<u32> {
    if let Some(v) = frame_word(h, index) {
        return Some(v);
    }
    let w = st().desk.get(h)?;
    match index {
        -2 => Some(w.style),
        -1 => Some(w.id),
        i if i >= 0 => {
            let (wi, sh) = (i as usize / 4, (i % 4) * 8);
            let lo = *w.words.get(wi)? as u64;
            let hi = w.words.get(wi + 1).copied().unwrap_or(0) as u64;
            Some(((lo | hi << 32) >> sh) as u32)
        }
        _ => None,
    }
}

/// QWL_HWNDFOCUSSAVE of a frame: the window that gets the focus when
/// the frame is activated.
const QWL_HWNDFOCUSSAVE: i32 = 0x18;

fn frame_word(h: u32, index: i32) -> Option<u32> {
    let f = st().frames.get(&h)?;
    match index {
        QWL_HWNDFOCUSSAVE => Some(f.focus_save),
        _ => None,
    }
}

fn word_set(h: u32, index: i32, v: u32, bytes: u32) -> bool {
    if index == QWL_HWNDFOCUSSAVE && bytes == 4 {
        if let Some(f) = st().frames.get_mut(&h) {
            f.focus_save = v;
            return true;
        }
    }
    let Some(w) = st().desk.get_mut(h) else { return false };
    match index {
        -2 => w.style = v,
        -1 => w.id = v,
        i if i >= 0 => {
            let (wi, sh) = (i as usize / 4, (i % 4) as u32 * 8);
            if wi + 1 >= w.words.len() + 1 && wi >= w.words.len() {
                return false;
            }
            let lo = w.words[wi] as u64;
            let hi = w.words.get(wi + 1).copied().unwrap_or(0) as u64;
            let mask = if bytes == 4 { 0xffff_ffffu64 } else { 0xffff } << sh;
            let val = ((lo | hi << 32) & !mask) | ((v as u64) << sh) & mask;
            w.words[wi] = val as u32;
            if let Some(h2) = w.words.get_mut(wi + 1) {
                *h2 = (val >> 32) as u32;
            }
        }
        _ => return false,
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowULong(h: u32, index: i32) -> u32 {
    word_get(h, index).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetWindowULong(h: u32, index: i32, v: u32) -> u32 {
    word_set(h, index, v, 4) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowPtr(h: u32, index: i32) -> u32 {
    word_get(h, index).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetWindowPtr(h: u32, index: i32, v: u32) -> u32 {
    word_set(h, index, v, 4) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowUShort(h: u32, index: i32) -> u16 {
    word_get(h, index).unwrap_or(0) as u16
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetWindowUShort(h: u32, index: i32, v: u16) -> u32 {
    word_set(h, index, v as u32, 2) as u32
}

// ---- Painting ----

/// WinBeginPaint(hwnd, hps, prclPaint): a presentation space clipped to
/// the window's update region, which becomes valid.
#[unsafe(no_mangle)]
pub extern "C" fn WinBeginPaint(h: u32, hps: u32, prcl: *mut RectL) -> u32 {
    let s = st();
    let Some(w) = s.desk.get_mut(h) else { return 0 };
    // The PM server painting a placeholder for a thread that does not
    // respond: the update region stays for the thread, only what has not
    // been painted over yet is drawn.
    let ghost = crate::server::ghost_painting();
    let update = if ghost {
        let mut u = w.update.clone();
        for g in &w.ghost.rects {
            u.subtract(g);
        }
        w.ghost = w.update.clone();
        u
    } else {
        core::mem::take(&mut w.update)
    };
    let clip_children = w.style & ws::CLIPCHILDREN != 0;
    let bounds = update.bounds();
    let o = s.desk.origin(h);
    let mut vis = s.desk.visible_region(h, clip_children);
    let mut dev = update.clone();
    dev.offset(o.x, o.y);
    vis.intersect(&dev.bounds());
    let clip: Vec<Rect> = vis.rects.iter().flat_map(|v| dev.rects.iter().map(move |u| v.intersect(u))).filter(|r| !r.is_empty()).collect();
    if !prcl.is_null() {
        unsafe { prcl.write_unaligned(RectL::from(bounds)) };
    }
    if !ghost {
        if let Some(w) = s.desk.get_mut(h) {
            w.ghost = pmwin::Region::new();
        }
    }
    let hps = if hps != 0 {
        gpiapi::begin_paint(s, hps, h, o, clip);
        hps
    } else {
        gpiapi::create(s, h, o, clip)
    };
    gpiapi::track_window(st(), hps, Some(update));
    hps
}

#[unsafe(no_mangle)]
pub extern "C" fn WinEndPaint(hps: u32) -> u32 {
    let s = st();
    gpiapi::end_paint(s, hps);
    1
}

/// WinGetPS(hwnd): a presentation space clipped to the visible region.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetPS(h: u32) -> u32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return 0 };
    let clip_children = w.style & ws::CLIPCHILDREN != 0;
    let vis = s.desk.visible_region(h, clip_children);
    let o = s.desk.origin(h);
    let hps = gpiapi::create(s, h, o, vis.rects);
    gpiapi::track_window(st(), hps, None);
    hps
}

#[unsafe(no_mangle)]
pub extern "C" fn WinReleasePS(hps: u32) -> u32 {
    gpiapi::destroy(st(), hps) as u32
}

/// WinFillRect(hps, prcl, lColor)
#[unsafe(no_mangle)]
pub extern "C" fn WinFillRect(hps: u32, prcl: *const RectL, c: i32) -> u32 {
    if prcl.is_null() {
        return 0;
    }
    let r = unsafe { prcl.read_unaligned() }.rect();
    let s = st();
    let mut surface = s.surface;
    pointer::hide(s);
    let ok = gpiapi::entry(s, hps).map(|e| {
        let rgb = gpiapi::rgb(e, c);
        e.ps.fill_rect(&mut surface, r, rgb);
    });
    pointer::show(s);
    ok.is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinInvalidateRect(h: u32, prcl: *const RectL, children: u32) -> u32 {
    let s = st();
    let h = if h == 0 { HWND_DESKTOP } else { h };
    if !s.desk.exists(h) {
        return 0;
    }
    let r = (!prcl.is_null()).then(|| unsafe { prcl.read_unaligned() }.rect());
    s.desk.invalidate(h, r, children != 0);
    1
}

/// WinInvalidateRegion(hwnd, hrgn, fIncludeChildren): 0 = the whole
/// window.
#[unsafe(no_mangle)]
pub extern "C" fn WinInvalidateRegion(h: u32, hrgn: u32, children: u32) -> u32 {
    crate::gpiobj::invalidate_region(h, hrgn, children != 0) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinValidateRect(h: u32, prcl: *const RectL, _children: u32) -> u32 {
    let Some(w) = st().desk.get_mut(h) else { return 0 };
    match (!prcl.is_null()).then(|| unsafe { prcl.read_unaligned() }.rect()) {
        Some(r) => w.update.subtract(&r),
        None => w.update = pmwin::Region::new(),
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinUpdateWindow(h: u32) -> u32 {
    let mut guard = 0;
    while st().desk.get(h).is_some_and(|w| !w.update.is_empty()) && guard < 4 {
        send(h, WM_PAINT, 0, 0);
        guard += 1;
    }
    1
}

/// WinDrawText(hps, cchText, lpchText, prcl, clrFore, clrBack, flCmd)
#[unsafe(no_mangle)]
pub extern "C" fn WinDrawText(hps: u32, cch: i32, text: *const u8, prcl: *mut RectL, fore: i32, back: i32, fl: u32) -> i32 {
    const DT_CENTER: u32 = 0x100;
    const DT_RIGHT: u32 = 0x200;
    const DT_VCENTER: u32 = 0x400;
    const DT_BOTTOM: u32 = 0x800;
    const DT_ERASERECT: u32 = 0x8000;
    const DT_TEXTATTRS: u32 = 0x40;
    if text.is_null() || prcl.is_null() {
        return 0;
    }
    let t = if cch < 0 { cstr(text) } else { unsafe { core::slice::from_raw_parts(text, cch as usize) }.to_vec() };
    let r = unsafe { prcl.read_unaligned() }.rect();
    let s = st();
    let mut surface = s.surface;
    let font = gpi::font::system();
    let w = font.text_width(&t);
    let x = if fl & DT_CENTER != 0 {
        r.left + (r.width() - w) / 2
    } else if fl & DT_RIGHT != 0 {
        r.right - w
    } else {
        r.left
    };
    let y = if fl & DT_VCENTER != 0 {
        r.bottom + (r.height() - font.height) / 2 + font.descender
    } else if fl & DT_BOTTOM != 0 {
        r.bottom + font.descender
    } else {
        r.top - font.height + font.descender
    };
    pointer::hide(s);
    if let Some(e) = gpiapi::entry(s, hps) {
        if fl & DT_ERASERECT != 0 {
            let bg = if fl & DT_TEXTATTRS != 0 { e.ps.back_color } else { resolve(back) };
            e.ps.fill_rect(&mut surface, r, bg);
        }
        let saved = (e.ps.color, e.ps.back_mix);
        if fl & DT_TEXTATTRS == 0 {
            e.ps.color = resolve(fore);
        }
        e.ps.back_mix = gpi::Mix::LeaveAlone;
        let clip = e.ps.clip.clone();
        e.ps.clip = clip.iter().map(|c| c.intersect(&r.offset(e.ps.origin.x, e.ps.origin.y))).collect();
        e.ps.char_string_at(&mut surface, Point::new(x, y), &t);
        e.ps.clip = clip;
        (e.ps.color, e.ps.back_mix) = saved;
    }
    pointer::show(s);
    t.len() as i32
}

// ---- The desktop and the frame window ----

/// Paint the desktop's update region.
pub fn paint_desktop() {
    let s = st();
    let Some(w) = s.desk.get_mut(HWND_DESKTOP) else { return };
    if w.update.is_empty() {
        return;
    }
    let update = core::mem::take(&mut w.update);
    let vis = s.desk.visible_region(HWND_DESKTOP, true);
    pointer::hide(s);
    let c = color::sys(color::SYSCLR_BACKGROUND);
    for v in &vis.rects {
        for u in &update.rects {
            s.surface.fill(v.intersect(u), c);
        }
    }
    pointer::show(s);
}
