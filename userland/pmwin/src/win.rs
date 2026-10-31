//! Windows: classes, creation and destruction, positions, focus and
//! activation, text, window words and painting.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::{Point, Rect};
use pmwin::{ws, HWND_DESKTOP};

use crate::color;
use crate::gpiapi::{self, PointL, RectL};
use crate::msg::{notify, send};
use crate::pointer;
use crate::frame::{self, FCF_SHELLPOSITION, FID_CLIENT};
use crate::state::{pid, st, Class};

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SHOW: u32 = 0x0005;
pub const WM_MOVE: u32 = 0x0006;
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

/// CS_MOVENOTIFY: the class's windows hear (WM_MOVE) when an ancestor
/// moves them on the screen.
pub const CS_MOVENOTIFY: u32 = 0x0001;

/// A window moved relative to its parent: WM_MOVE to it and to the
/// windows inside it whose class asks for it (CS_MOVENOTIFY), as
/// WinSetWindowPos does. Programs keep their window positions this way
/// (OS/2's Solitaire saves where its frame is on WM_MOVE).
pub fn moved(h: u32) {
    notify(h, WM_MOVE, 0, 0);
    move_notify(h);
}

fn move_notify(h: u32) {
    let kids = st().desk.get(h).map(|w| w.children.clone()).unwrap_or_default();
    for c in kids {
        let s = st();
        let Some(w) = s.desk.get(c) else { continue };
        if s.classes.get(w.class as usize).is_some_and(|k| k.style & CS_MOVENOTIFY != 0) {
            notify(c, WM_MOVE, 0, 0);
        }
        move_notify(c);
    }
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
    crate::pp::take_pending(h);
    // A frame's client is its child FID_CLIENT, however it was made
    // (EPM creates its frames' clients with WinCreateWindow).
    if id == frame::FID_CLIENT {
        if let Some(f) = s.frames.get_mut(&parent).filter(|f| f.client == 0) {
            f.client = h;
        }
    }
    // WM_CREATE: mp1 = control data, mp2 = CREATESTRUCT (pPresParams,
    // pCtlData, id, hwndInsertBehind, hwndOwner, cy, cx, y, x,
    // flStyle, pszText, pszClass, hwndParent). The Open Watcom GUI
    // library finds its window from pCtlData there.
    let mut tz = text.to_vec();
    tz.push(0);
    let mut cz = class.to_vec();
    cz.push(0);
    let cs: [u32; 13] = [
        0, ctl, id, 3, owner, pos.height() as u32, pos.width() as u32, pos.bottom as u32, pos.left as u32, style, tz.as_ptr() as u32, cz.as_ptr() as u32, parent,
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
        crate::hooks::destroy_window(w);
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
    crate::cursor::windows_gone(s, &gone);
    for f in s.frames.values_mut() {
        if gone.contains(&f.client) {
            f.client = 0;
        }
    }
    let was_active = gone.contains(&s.active);
    for g in &gone {
        s.frames.remove(g);
        s.switch.window_gone(*g);
        s.ctl.remove(g);
        crate::pp::forget(*g);
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
    behind: u32,
    id: u32,
    ctl: u32,
    pres: u32,
) -> u32 {
    // IBM makes a window without a parent for NULLHANDLE: it is never
    // shown (the Open Watcom installer's progress window). rs2 keeps it
    // under the object window.
    let parent = if parent == 0 { pmwin::HWND_OBJECT } else { parent };
    let h = create_window(parent, class, name, style, x, y, cx, cy, owner, id, ctl, pres);
    // hwndInsertBehind: new windows go on top of their siblings; HWND_BOTTOM
    // or a sibling places them lower (WIN16's dialog controls, the first
    // of which is the top of the Z order as on Windows).
    if h != 0 && behind != HWND_TOP {
        let s = st();
        match behind {
            4 => s.desk.to_bottom(h),
            b if s.desk.exists(b) => s.desk.place_behind(h, b),
            _ => {}
        }
    }
    h
}

#[allow(clippy::too_many_arguments)]
fn create_window(parent: u32, class: *const u8, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, id: u32, ctl: u32, pres: u32) -> u32 {
    let class = class_name(class);
    // The window gets its PRESPARAMS before WM_CREATE.
    crate::pp::set_pending(pres);
    let pos = Rect::new(x, y, x + cx, y + cy);
    if frame::is_frame_class(&class) {
        // FRAMECDATA: cb, flCreateFlags, hmodResources, idResources.
        let (fcf, hmod) = if ctl == 0 {
            (0, 0)
        } else {
            let p = ctl as *const u8;
            unsafe { ((p.add(2) as *const u32).read_unaligned(), (p.add(6) as *const u16).read_unaligned() as u32) }
        };
        let h = frame::create_class(&class, parent, style, fcf, &cstr(name), pos, owner, id, hmod, ctl);
        // FCF_SHELLPOSITION in the frame data, as from WinCreateStdWindow:
        // a frame made without a size gets the shell's position when shown.
        if h != 0 && fcf & FCF_SHELLPOSITION != 0 && cx == 0 && cy == 0 {
            if let Some(f) = st().frames.get_mut(&h) {
                f.shell_pos = true;
            }
        }
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
    // As IBM's: the parent must be a window, HWND_DESKTOP or HWND_OBJECT
    // (NULLHANDLE fails).
    if parent != HWND_DESKTOP && parent != pmwin::HWND_OBJECT && !st().desk.exists(parent) {
        return 0;
    }
    let fcf = if pfl.is_null() { 0 } else { unsafe { pfl.read_unaligned() } };
    // FCF_SHELLPOSITION: as on OS/2, the frame stays without a size (its
    // client gets no WM_SIZE) until it is first shown.
    let frame = frame::create(parent, style, fcf, &cstr(title), Rect::default(), 0, id, hmod);
    if frame == 0 {
        return 0;
    }
    if fcf & FCF_SHELLPOSITION != 0 {
        if let Some(f) = st().frames.get_mut(&frame) {
            f.shell_pos = true;
        }
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
/// A frame created with FCF_SHELLPOSITION is shown (or minimized or
/// maximized) for the first time: if it still has no size, it gets the
/// shell's position (IBM's PMWIN asks the shell when such a frame is
/// first shown).
pub fn apply_shell_position(h: u32) {
    let Some(f) = st().frames.get_mut(&h) else { return };
    if !core::mem::replace(&mut f.shell_pos, false) {
        return;
    }
    if st().desk.get(h).is_some_and(|w| w.cx == 0 && w.cy == 0) {
        let r = shell_position();
        st().desk.set_pos(h, r.left, r.bottom, r.width(), r.height());
        frame::layout(h);
        moved(h);
    }
}

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
    // The object window (and windows no queue owns) belong to PM: the
    // PM server.
    let queue = s.desk.get(h).map(|w| if w.queue == 0 { s.server_queue } else { w.queue });
    let Some(q) = queue.and_then(|q| s.queues.get(&q)) else { return 0 };
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
    // Only the window's own thread may destroy it, as on OS/2 (Borland's
    // installer tries from its worker thread; the window stays and gets
    // the worker's "done" message).
    if st().queue_of(h) != st().my_queue() {
        crate::misc::set_error(0x1009); // PMERR_CALL_FROM_WRONG_THREAD
        return 0;
    }
    destroy(h);
    1
}

// ---- Position, visibility, z-order, activation ----

pub fn show(h: u32, on: bool) {
    if on {
        apply_shell_position(h);
    }
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
    if fl & (SWP_SIZE | SWP_SHOW | frame::SWP_MINIMIZE | frame::SWP_MAXIMIZE) != 0 && !(fl & SWP_SIZE != 0 && cx > 0 && cy > 0) {
        apply_shell_position(h);
    }
    // Minimize, maximize and restore (frames): they size the window.
    let fl = if st().frames.contains_key(&h) && fl & (frame::SWP_MINIMIZE | frame::SWP_MAXIMIZE | frame::SWP_RESTORE) != 0 {
        let normal = st().desk.get(h).is_some_and(|w| w.style & (ws::MINIMIZED | ws::MAXIMIZED) == 0);
        if fl & frame::SWP_MINIMIZE != 0 {
            frame::minimize(h);
        } else if fl & frame::SWP_MAXIMIZE != 0 {
            frame::maximize(h, true);
        } else if normal {
            // Restoring a frame that is neither minimized nor maximized
            // leaves it where SWP_MOVE and SWP_SIZE put it
            // (WinRestoreWindowPos does this).
            return WinSetWindowPos(h, behind, x, y, cx, cy, fl & !frame::SWP_RESTORE);
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
        if (nx, ny) != (ox, oy) {
            moved(h);
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
    // Below a sibling.
    if fl & SWP_ZORDER != 0 && behind != HWND_TOP && behind != 4 && st().desk.exists(behind) {
        st().desk.place_behind(h, behind);
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
/// As IBM's: fl has SWP_SIZE, SWP_MOVE, SWP_ZORDER and SWP_SHOW or
/// SWP_HIDE (and a frame's SWP_MINIMIZE, SWP_MAXIMIZE or SWP_RESTORE), and
/// hwndInsertBehind is the sibling above it (HWND_TOP for the topmost).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowPos(h: u32, pswp: *mut u32) -> u32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return 0 };
    if pswp.is_null() {
        return 0;
    }
    let mut fl = SWP_SIZE | SWP_MOVE | SWP_ZORDER | if w.style & ws::VISIBLE != 0 { SWP_SHOW } else { SWP_HIDE };
    if s.frames.contains_key(&h) {
        fl |= if w.style & ws::MINIMIZED != 0 {
            frame::SWP_MINIMIZE
        } else if w.style & ws::MAXIMIZED != 0 {
            frame::SWP_MAXIMIZE
        } else {
            frame::SWP_RESTORE
        };
    }
    let above = s.desk.get(w.parent).and_then(|p| p.children.iter().position(|&c| c == h).and_then(|i| p.children.get(i + 1).copied()));
    let vals = [fl, w.cy as u32, w.cx as u32, w.y as u32, w.x as u32, above.unwrap_or(HWND_TOP), h, 0, 0];
    for (i, v) in vals.into_iter().enumerate() {
        unsafe { pswp.add(i).write_unaligned(v) };
    }
    1
}

/// WinSetMultWindowPos(hab, pswp, cswp): each SWP (fl, cy, cx, y, x,
/// hwndInsertBehind, hwnd, ...) applied in turn.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetMultWindowPos(_hab: u32, pswp: *const u32, n: u32) -> u32 {
    if pswp.is_null() {
        return 0;
    }
    let mut ok = 1;
    for i in 0..n as usize {
        let v: [u32; 7] = core::array::from_fn(|k| unsafe { pswp.add(i * 9 + k).read_unaligned() });
        if WinSetWindowPos(v[6], v[5], v[4] as i32, v[3] as i32, v[2] as i32, v[1] as i32, v[0]) == 0 {
            ok = 0;
        }
    }
    ok
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

/// Enumerations of child windows (WinBeginEnumWindows) of this process:
/// the children as they were, and the next index.
static mut ENUMS: Vec<Option<(Vec<u32>, usize)>> = Vec::new();
const HENUM_BASE: u32 = 0x4e00_0001;

fn enums() -> &'static mut Vec<Option<(Vec<u32>, usize)>> {
    unsafe { &mut *core::ptr::addr_of_mut!(ENUMS) }
}

/// WinBeginEnumWindows(hwnd): an enumeration of the window's children as
/// they are now, top of the z-order first (HWND_DESKTOP and
/// HWND_OBJECT, or 0 for the desktop, enumerate the top-level windows).
#[unsafe(no_mangle)]
pub extern "C" fn WinBeginEnumWindows(h: u32) -> u32 {
    let s = st();
    let h = if h == 0 { HWND_DESKTOP } else { h };
    let Some(w) = s.desk.get(h) else { return 0 };
    let list: Vec<u32> = w.children.iter().rev().copied().collect();
    let v = enums();
    let i = match v.iter().position(|e| e.is_none()) {
        Some(i) => {
            v[i] = Some((list, 0));
            i
        }
        None => {
            v.push(Some((list, 0)));
            v.len() - 1
        }
    };
    HENUM_BASE + i as u32
}

/// WinGetNextWindow(henum): the next child that still exists; 0 at the
/// end.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetNextWindow(henum: u32) -> u32 {
    let s = st();
    let Some(Some((list, next))) = enums().get_mut(henum.wrapping_sub(HENUM_BASE) as usize) else { return 0 };
    while let Some(&h) = list.get(*next) {
        *next += 1;
        if s.desk.exists(h) {
            return h;
        }
    }
    0
}

/// WinEndEnumWindows(henum)
#[unsafe(no_mangle)]
pub extern "C" fn WinEndEnumWindows(henum: u32) -> u32 {
    match enums().get_mut(henum.wrapping_sub(HENUM_BASE) as usize) {
        Some(e @ Some(_)) => {
            *e = None;
            1
        }
        _ => 0,
    }
}

/// WinQueryClassName(hwnd, cchMax, pch): the window's class name
/// ("#3" for WC_BUTTON and so on), truncated to cchMax - 1 characters;
/// returns the length copied.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryClassName(h: u32, cch: i32, buf: *mut u8) -> i32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return 0 };
    if buf.is_null() || cch <= 0 {
        return 0;
    }
    let name = s.classes.get(w.class as usize).map_or(&[][..], |c| &c.name[..]);
    let n = name.len().min(cch as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as i32
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
/// QWL_STYLE (-2), QWS_ID (-1), QWL_HMQ (-4).
fn word_get(h: u32, index: i32) -> Option<u32> {
    if let Some(v) = frame_word(h, index) {
        return Some(v);
    }
    let w = st().desk.get(h)?;
    match index {
        -4 => Some(w.queue),
        // QWP_PFNWP: the window procedure
        -3 => Some(w.proc_),
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

/// A frame's QWS_XRESTORE, QWS_YRESTORE, QWS_CXRESTORE, QWS_CYRESTORE
/// (where restoring puts it) and QWS_XMINIMIZE, QWS_YMINIMIZE (where it
/// goes when minimized).
const QWS_XRESTORE: i32 = 0x0c;
const QWS_CYRESTORE: i32 = 0x12;
const QWS_XMINIMIZE: i32 = 0x14;
const QWS_YMINIMIZE: i32 = 0x16;

/// The position restoring a frame gives it: the position before it was
/// minimized or maximized, its own while it is neither.
fn restore_pos(h: u32) -> (i32, i32, i32, i32) {
    let s = st();
    let cur = s.desk.get(h).map_or((0, 0, 0, 0), |w| (w.x, w.y, w.cx, w.cy));
    let Some(f) = s.frames.get(&h) else { return cur };
    match &f.min {
        Some(m) if !m.was_max => m.pos,
        _ => f.restore.unwrap_or(cur),
    }
}

fn frame_word(h: u32, index: i32) -> Option<u32> {
    let f = st().frames.get(&h)?;
    match index {
        QWL_HWNDFOCUSSAVE => Some(f.focus_save),
        QWS_XRESTORE..=QWS_CYRESTORE if index % 2 == 0 => {
            let p = restore_pos(h);
            Some([p.0, p.1, p.2, p.3][(index - QWS_XRESTORE) as usize / 2] as u16 as u32)
        }
        QWS_XMINIMIZE | QWS_YMINIMIZE => {
            let p = f.icon_pos.unwrap_or((0, 0));
            Some(if index == QWS_XMINIMIZE { p.0 } else { p.1 } as u16 as u32)
        }
        _ => None,
    }
}

/// Set a frame's restore or minimize position word.
fn set_frame_word(h: u32, index: i32, v: u32) -> Option<bool> {
    if !(QWS_XRESTORE..=QWS_YMINIMIZE).contains(&index) || index % 2 != 0 || !st().frames.contains_key(&h) {
        return None;
    }
    let v = v as u16 as i16 as i32;
    if index >= QWS_XMINIMIZE {
        let f = st().frames.get_mut(&h)?;
        let mut p = f.icon_pos.unwrap_or((0, 0));
        if index == QWS_XMINIMIZE { p.0 = v } else { p.1 = v }
        f.icon_pos = Some(p);
        return Some(true);
    }
    let mut p = restore_pos(h);
    match (index - QWS_XRESTORE) / 2 {
        0 => p.0 = v,
        1 => p.1 = v,
        2 => p.2 = v,
        _ => p.3 = v,
    }
    let maximized = st().desk.get(h).is_some_and(|w| w.style & ws::MAXIMIZED != 0);
    let f = st().frames.get_mut(&h)?;
    match &mut f.min {
        Some(m) if !m.was_max => m.pos = p,
        Some(_) => f.restore = Some(p),
        None if maximized => f.restore = Some(p),
        // A frame in its normal state is where restoring puts it.
        None => {}
    }
    Some(true)
}

fn word_set(h: u32, index: i32, v: u32, bytes: u32) -> bool {
    if bytes == 2 {
        if let Some(r) = set_frame_word(h, index, v) {
            return r;
        }
    }
    if index == QWL_HWNDFOCUSSAVE && bytes == 4 {
        if let Some(f) = st().frames.get_mut(&h) {
            f.focus_save = v;
            return true;
        }
    }
    let Some(w) = st().desk.get_mut(h) else { return false };
    if index == -3 && bytes == 4 {
        w.proc_ = v;
        return true;
    }
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

/// WinSetWindowBits(hwnd, index, data, mask): set the masked bits of a
/// window word to those of `data`.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetWindowBits(h: u32, index: i32, data: u32, mask: u32) -> u32 {
    let Some(old) = word_get(h, index) else { return 0 };
    word_set(h, index, (old & !mask) | (data & mask), 4) as u32
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
        s.desk.take_update(h)
    };
    let Some(w) = s.desk.get(h) else { return 0 };
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
    // The cursor is hidden while its window paints.
    crate::cursor::paint(h, false);
    let s = st();
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
    let h = gpiapi::entry(s, hps).map_or(0, |e| e.hwnd);
    gpiapi::end_paint(s, hps);
    crate::avio::poll();
    crate::cursor::paint(h, true);
    1
}

/// WinSetSysModalWindow(hwndDesktop, hwnd): recorded for
/// WinQuerySysModalWindow (input is not limited to it).
#[unsafe(no_mangle)]
pub extern "C" fn WinSetSysModalWindow(_desk: u32, h: u32) -> u32 {
    let s = st();
    if h != 0 && !s.desk.exists(h) {
        return 0;
    }
    s.sysmodal = h;
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysModalWindow(_desk: u32) -> u32 {
    let s = st();
    if s.desk.exists(s.sysmodal) { s.sysmodal } else { 0 }
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
    crate::hooks::window_dc(hps, h, true);
    hps
}

/// WinGetClipPS(hwnd, hwndClip, fl): a cached PS clipped as `fl` asks:
/// PSF_CLIPCHILDREN (0x10) leaves out the children, PSF_PARENTCLIP (0x20)
/// gives the parent's visible area. Siblings above are left out as for
/// WinGetPS (PSF_CLIPSIBLINGS); hwndClip with PSF_CLIPUPWARDS/DOWNWARDS
/// is taken as that.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetClipPS(h: u32, _clip: u32, fl: u32) -> u32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return 0 };
    let parent = w.parent;
    let vis = if fl & 0x20 != 0 && s.desk.exists(parent) {
        s.desk.visible_region(parent, fl & 0x10 != 0)
    } else {
        s.desk.visible_region(h, fl & 0x10 != 0)
    };
    let o = s.desk.origin(h);
    let hps = gpiapi::create(s, h, o, vis.rects);
    gpiapi::track_window(st(), hps, None);
    crate::hooks::window_dc(hps, h, true);
    hps
}

#[unsafe(no_mangle)]
pub extern "C" fn WinReleasePS(hps: u32) -> u32 {
    if crate::hooks::any(crate::hooks::HK_WINDOWDC) {
        let h = gpiapi::entry(st(), hps).map_or(0, |e| e.hwnd);
        crate::hooks::window_dc(hps, h, false);
    }
    gpiapi::destroy(st(), hps) as u32
}

/// WinFillRect(hps, prcl, lColor)
#[unsafe(no_mangle)]
pub extern "C" fn WinFillRect(hps: u32, prcl: *const RectL, c: i32) -> u32 {
    if prcl.is_null() {
        return 0;
    }
    let r = unsafe { prcl.read_unaligned() }.rect();
    // On the PS's target: the screen, or a memory PS's bitmap.
    gpiapi::draw(hps, |e, surface| {
        let rgb = gpiapi::rgb(e, c);
        e.ps.fill_rect(surface, r, rgb);
    })
    .is_some() as u32
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
    if !st().desk.exists(h) {
        return 0;
    }
    let r = (!prcl.is_null()).then(|| unsafe { prcl.read_unaligned() }.rect());
    st().desk.validate(h, r);
    1
}

/// WinUpdateWindow(hwnd): the window and its children with update
/// regions get WM_PAINT now (posted instead for another thread's). As in
/// PM, the window's update region is first taken out of a parent that
/// does not clip its children, so the parent's later painting does not
/// cover what the window draws now (OS/2's Solitaire shows each card this
/// way while the table's first WM_PAINT is still pending).
#[unsafe(no_mangle)]
pub extern "C" fn WinUpdateWindow(h: u32) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    let me = st().my_queue();
    let mut stack = alloc::vec![h];
    while let Some(w) = stack.pop() {
        let s = st();
        let Some(win) = s.desk.get(w) else { continue };
        if !win.visible_style() {
            continue;
        }
        stack.extend(win.children.iter().rev().copied());
        if win.update.is_empty() {
            continue;
        }
        let (parent, x, y) = (win.parent, win.x, win.y);
        let mut mine = win.update.clone();
        mine.offset(x, y);
        if let Some(p) = s.desk.get_mut(parent) {
            if p.style & ws::CLIPCHILDREN == 0 && !p.update.is_empty() && parent != HWND_DESKTOP {
                for r in &mine.rects {
                    p.update.subtract(r);
                }
            }
        }
        if s.queue_of(w) != me {
            continue; // painted by its own thread
        }
        let mut guard = 0;
        while st().desk.get(w).is_some_and(|x| !x.update.is_empty()) && guard < 4 {
            send(w, WM_PAINT, 0, 0);
            guard += 1;
        }
    }
    1
}

/// WinDrawText(hps, cchText, lpchText, prcl, clrFore, clrBack, flCmd):
/// one line of text in the rectangle, aligned by DT_LEFT/CENTER/RIGHT
/// and DT_TOP/VCENTER/BOTTOM, on the PS's target. DT_WORDBREAK draws
/// the words that fit on the line (callers loop over the rest);
/// DT_QUERYEXTENT only measures (the rectangle becomes the text's);
/// DT_MNEMONIC underlines the character after `~`; DT_HALFTONE greys
/// the text; DT_ERASERECT fills the rectangle first; DT_TEXTATTRS uses
/// the PS's colours. Returns the number of characters processed.
#[unsafe(no_mangle)]
pub extern "C" fn WinDrawText(hps: u32, cch: i32, text: *const u8, prcl: *mut RectL, fore: i32, back: i32, fl: u32) -> i32 {
    const DT_QUERYEXTENT: u32 = 0x0002;
    const DT_TEXTATTRS: u32 = 0x0040;
    const DT_CENTER: u32 = 0x0100;
    const DT_RIGHT: u32 = 0x0200;
    const DT_VCENTER: u32 = 0x0400;
    const DT_BOTTOM: u32 = 0x0800;
    const DT_HALFTONE: u32 = 0x1000;
    const DT_MNEMONIC: u32 = 0x2000;
    const DT_WORDBREAK: u32 = 0x4000;
    const DT_ERASERECT: u32 = 0x8000;
    if text.is_null() || prcl.is_null() {
        return 0;
    }
    let all = if cch < 0 { cstr(text) } else { unsafe { core::slice::from_raw_parts(text, cch as usize) }.to_vec() };
    let r = unsafe { prcl.read_unaligned() }.rect();
    let Some(e) = gpiapi::entry(st(), hps) else { return 0 };
    // An outline font at the character box's size; others as they are.
    let font = match e.ps.text_font() {
        (f, sx, sy) if sx == 1.0 && sy == 1.0 => f,
        _ => e.ps.font,
    };
    // The line: up to a line end; with DT_WORDBREAK, the words that fit.
    let line_end = all.iter().position(|&c| c == b'\r' || c == b'\n').unwrap_or(all.len());
    let mut n = line_end;
    let width = |t: &[u8]| {
        let t = if fl & DT_MNEMONIC != 0 { crate::draw::strip_mnemonic(t).0 } else { t.to_vec() };
        font.text_width(&t)
    };
    if fl & DT_WORDBREAK != 0 && width(&all[..n]) > r.width() {
        let mut fit = 0;
        let mut i = 0;
        while i <= line_end {
            if i == line_end || all[i] == b' ' {
                if width(&all[..i]) > r.width() {
                    break;
                }
                fit = i;
            }
            i += 1;
        }
        // A word longer than the line is drawn cut off.
        n = if fit == 0 { all[..line_end].iter().position(|&c| c == b' ').unwrap_or(line_end) } else { fit };
        // A find word hook says where words begin and end (IBM's
        // CchLineBreak): the line ends before the word that does not fit.
        if crate::hooks::any(crate::hooks::HK_FINDWORD) {
            let over = (1..=line_end).find(|&i| width(&all[..i]) > r.width()).map_or(line_end, |i| i - 1);
            let cp = crate::misc::WinQueryCp(1);
            if let Some((start, end, _)) = crate::hooks::find_word(cp, &all[..line_end], over as u32) {
                let (start, end) = (start as usize, end as usize);
                n = if start > 0 && start <= line_end { start } else { end.clamp(1, line_end) };
            }
        }
    }
    // (A hook ran without the lock: the PS may be gone.)
    if gpiapi::entry(st(), hps).is_none() {
        return 0;
    }
    let shown = &all[..n];
    let (t, mark) = if fl & DT_MNEMONIC != 0 { crate::draw::strip_mnemonic(shown) } else { (shown.to_vec(), None) };
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
    // What was processed: the line, the blanks after it, a line end.
    let mut used = n;
    while used < all.len() && all[used] == b' ' {
        used += 1;
    }
    if used < all.len() && all[used] == b'\r' {
        used += 1;
    }
    if used < all.len() && all[used] == b'\n' {
        used += 1;
    }
    if fl & DT_QUERYEXTENT != 0 {
        let base = y - font.descender;
        let ext = RectL { left: x, bottom: base, right: x + w, top: base + font.height };
        unsafe { prcl.write_unaligned(ext) };
        return used as i32;
    }
    gpiapi::draw(hps, |e, surface| {
        if fl & DT_ERASERECT != 0 {
            let bg = if fl & DT_TEXTATTRS != 0 { e.ps.back_color } else { gpiapi::rgb(e, back) };
            e.ps.fill_rect(surface, r, bg);
        }
        let saved = (e.ps.color, e.ps.back_mix);
        if fl & DT_TEXTATTRS == 0 {
            e.ps.color = gpiapi::rgb(e, fore);
        }
        if fl & DT_HALFTONE != 0 {
            e.ps.color = 0x808080;
        }
        e.ps.back_mix = gpi::Mix::LeaveAlone;
        let clip = e.ps.clip.clone();
        e.ps.clip = clip.iter().map(|c| c.intersect(&r.offset(e.ps.origin.x, e.ps.origin.y))).collect();
        let base = core::mem::replace(&mut e.ps.font, font);
        e.ps.char_string_at(surface, Point::new(x, y), &t);
        e.ps.font = base;
        if let Some(i) = mark {
            let ux = x + font.text_width(&t[..i]);
            let cw = font.text_width(&t[i..i + 1]);
            let c = e.ps.color;
            e.ps.fill_rect(surface, Rect::new(ux, y - 2, ux + cw, y - 1), c);
        }
        e.ps.clip = clip;
        (e.ps.color, e.ps.back_mix) = saved;
    });
    used as i32
}

// ---- The desktop and the frame window ----

/// Paint the desktop's update region.
pub fn paint_desktop() {
    let s = st();
    let Some(w) = s.desk.get_mut(HWND_DESKTOP) else { return };
    if w.update.is_empty() {
        return;
    }
    let update = s.desk.take_update(HWND_DESKTOP);
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
