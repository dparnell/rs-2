//! USER: window classes, windows, messages and painting, on PM.
//!
//! A top-level Windows window with a caption or a frame is a PM frame
//! window (title bar, system menu, sizing border and buttons from the
//! window style, the menu bar, scroll bars) with a client window of the
//! class `Win16Wnd`. Other windows (child windows, and pop-ups without a
//! caption such as a combo box's list) are `Win16Wnd` windows in their
//! parent's client (or on the desktop); one with a border or scroll bars
//! is a `Win16Frame` window that draws the border and holds the client
//! and the scroll bars (Windows' non-client area). The Windows HWND is a
//! handle of this table; messages for it arrive on the PM client window.
//!
//! Messages PM sends to the client come to `pm_wndproc`, which
//! translates them (Windows coordinates have y growing down from the
//! client's top-left corner) and calls the window's 16-bit window
//! procedure. Messages PM posts (input, WM_PAINT, WM_TIMER) come out of
//! GetMessage translated the same way and go to the procedure through
//! DispatchMessage. While a translated message is being handled,
//! DefWindowProc for it passes the PM message on to WinDefWindowProc;
//! other messages get Windows' default handling here.
//!
//! The built-in classes (BUTTON, EDIT, ... and the dialog class) have
//! window procedures in Rust (ctl.rs, dialog.rs) with 16:16 addresses of
//! their own (entries in lib.rs), so programs can subclass them and call
//! them with CallWindowProc; calls to them from here go straight to Rust.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;
use thunk16::{flat, Pascal, PascalCall};

use crate::gdi;
use crate::kernel::TASK;
use crate::pm::*;
use crate::{far_str, read_rect, trace, write_rect, Global};

// Windows messages.
pub const W_CREATE: u16 = 0x0001;
pub const W_DESTROY: u16 = 0x0002;
pub const W_MOVE: u16 = 0x0003;
pub const W_SIZE: u16 = 0x0005;
pub const W_ACTIVATE: u16 = 0x0006;
pub const W_SETFOCUS: u16 = 0x0007;
pub const W_KILLFOCUS: u16 = 0x0008;
pub const W_ENABLE: u16 = 0x000a;
pub const W_SETREDRAW: u16 = 0x000b;
pub const W_SETTEXT: u16 = 0x000c;
pub const W_GETTEXT: u16 = 0x000d;
pub const W_GETTEXTLENGTH: u16 = 0x000e;
pub const W_PAINT: u16 = 0x000f;
pub const W_CLOSE: u16 = 0x0010;
pub const W_QUERYENDSESSION: u16 = 0x0011;
pub const W_QUIT: u16 = 0x0012;
pub const W_ERASEBKGND: u16 = 0x0014;
pub const W_SHOWWINDOW: u16 = 0x0018;
pub const W_CTLCOLOR: u16 = 0x0019;
pub const W_CANCELMODE: u16 = 0x001f;
pub const W_SETCURSOR: u16 = 0x0020;
pub const W_MOUSEACTIVATE: u16 = 0x0021;
pub const W_NEXTDLGCTL: u16 = 0x0028;
pub const W_DRAWITEM: u16 = 0x002b;
pub const W_MEASUREITEM: u16 = 0x002c;
pub const W_DELETEITEM: u16 = 0x002d;
pub const W_VKEYTOITEM: u16 = 0x002e;
pub const W_CHARTOITEM: u16 = 0x002f;
pub const W_SETFONT: u16 = 0x0030;
pub const W_GETFONT: u16 = 0x0031;
pub const W_COMPAREITEM: u16 = 0x0039;
pub const W_NCCREATE: u16 = 0x0081;
pub const W_NCDESTROY: u16 = 0x0082;
pub const W_GETDLGCODE: u16 = 0x0087;
pub const W_KEYDOWN: u16 = 0x0100;
pub const W_KEYUP: u16 = 0x0101;
pub const W_CHAR: u16 = 0x0102;
pub const W_SYSKEYDOWN: u16 = 0x0104;
pub const W_SYSKEYUP: u16 = 0x0105;
pub const W_SYSCHAR: u16 = 0x0106;
pub const W_INITDIALOG: u16 = 0x0110;
pub const W_COMMAND: u16 = 0x0111;
pub const W_SYSCOMMAND: u16 = 0x0112;
pub const W_TIMER: u16 = 0x0113;
pub const W_HSCROLL: u16 = 0x0114;
pub const W_VSCROLL: u16 = 0x0115;
pub const W_INITMENU: u16 = 0x0116;
pub const W_INITMENUPOPUP: u16 = 0x0117;
pub const W_MENUSELECT: u16 = 0x011f;
pub const W_ENTERIDLE: u16 = 0x0121;
pub const W_MOUSEMOVE: u16 = 0x0200;
pub const W_LBUTTONDOWN: u16 = 0x0201;
pub const W_LBUTTONUP: u16 = 0x0202;
pub const W_LBUTTONDBLCLK: u16 = 0x0203;
pub const W_PARENTNOTIFY: u16 = 0x0210;
pub const W_CUT: u16 = 0x0300;
pub const W_COPY: u16 = 0x0301;
pub const W_PASTE: u16 = 0x0302;
pub const W_CLEAR: u16 = 0x0303;
pub const W_UNDO: u16 = 0x0304;
pub const W_USER: u16 = 0x0400;

// Window styles.
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_DISABLED: u32 = 0x0800_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_CAPTION: u32 = 0x00c0_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_DLGFRAME: u32 = 0x0040_0000;
pub const WS_VSCROLL: u32 = 0x0020_0000;
pub const WS_HSCROLL: u32 = 0x0010_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_GROUP: u32 = 0x0002_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_MAXIMIZEBOX: u32 = 0x0001_0000;

const CS_VREDRAW: u16 = 0x0001;
const CS_HREDRAW: u16 = 0x0002;
pub const CS_DBLCLKS: u16 = 0x0008;

const CW_USEDEFAULT: i32 = -0x8000;

/// PM message number of a Windows message posted with PostMessage.
const POSTED: u32 = 0x0001_0000;

#[allow(dead_code)]
pub struct Class {
    pub name: Vec<u8>,
    pub atom: u16,
    pub style: u16,
    pub proc_: u32,
    pub cls_extra: Vec<u8>,
    pub wnd_extra: u16,
    pub hinst: u16,
    pub icon: u16,
    pub cursor: u16,
    pub brush: u16,
    pub menu: Option<crate::res::Name>,
}

pub struct Wnd {
    pub hwnd: u16,
    /// The PM frame (top-level windows), the `Win16Frame` (windows with
    /// a border or scroll bars) or the window itself.
    pub frame: u32,
    /// The PM window of the client area.
    pub client: u32,
    pub class: usize,
    pub proc_: u32,
    pub style: u32,
    pub exstyle: u32,
    /// The parent (child windows) or the owner (top-level windows).
    pub parent: u16,
    pub id: u16,
    pub hinst: u16,
    pub extra: Vec<u8>,
    /// The window text (windows without a title bar).
    pub text: Vec<u8>,
    /// The menu bar (HMENU) of a top-level window.
    pub menu: u16,
    /// A PM frame window (with a title bar or a frame).
    pub framed: bool,
    /// The non-client parts of a plain window: WS_BORDER, WS_DLGFRAME,
    /// WS_THICKFRAME, WS_VSCROLL, WS_HSCROLL (a combo box's are its
    /// list's).
    pub nc: u32,
    /// The state of a built-in control.
    pub data: Option<Box<dyn Any>>,
    /// The window's scroll bars (WS_VSCROLL, WS_HSCROLL).
    pub vsb: Option<crate::scroll::Bar>,
    pub hsb: Option<crate::scroll::Bar>,
}

/// A PM message being handled as a Windows message.
#[derive(Clone, Copy)]
struct InFlight {
    hwnd: u16,
    msg: u16,
    q: QMsg,
}

struct Timer {
    hwnd: u16,
    id: u16,
    pm_id: u32,
    proc_: u32,
}

struct User {
    classes: Vec<Class>,
    wnds: Vec<Wnd>,
    /// Destroyed windows, freed when no built-in procedure is running.
    gone: Vec<Wnd>,
    next_hwnd: u16,
    next_atom: u16,
    inflight: Vec<InFlight>,
    /// The PM message behind the last message GetMessage returned.
    last: Option<(u16, u16, QMsg)>,
    /// The character of the last WM_KEYDOWN (TranslateMessage posts it).
    last_char: Option<(u16, u16, u16, u32)>,
    timers: Vec<Timer>,
    cascade: i32,
    /// Registered window messages.
    messages: Vec<Vec<u8>>,
    /// The last message's time and position (GetMessageTime, GetMessagePos).
    msg_time: u32,
    msg_pos: u32,
}

static USER: Global<User> = Global::new(User {
    classes: Vec::new(),
    wnds: Vec::new(),
    gone: Vec::new(),
    next_hwnd: 0x2004,
    next_atom: 0xc000,
    inflight: Vec::new(),
    last: None,
    last_char: None,
    timers: Vec::new(),
    cascade: 0,
    messages: Vec::new(),
    msg_time: 0,
    msg_pos: 0,
});

fn u() -> &'static mut User {
    USER.get()
}

pub fn wnd(h: u16) -> Option<&'static mut Wnd> {
    if h == 0 {
        return None;
    }
    u().wnds.iter_mut().find(|w| w.hwnd == h)
}

/// The state of a built-in control.
pub fn data<T: 'static>(h: u16) -> Option<&'static mut T> {
    wnd(h)?.data.as_mut()?.downcast_mut::<T>()
}

fn by_client(pm: u32) -> Option<&'static mut Wnd> {
    u().wnds.iter_mut().find(|w| w.client == pm)
}

fn by_frame(pm: u32) -> Option<&'static mut Wnd> {
    u().wnds.iter_mut().find(|w| w.frame == pm || w.client == pm)
}

/// The Windows window of a PM window (0 if none).
pub fn hwnd_of(pm: u32) -> u16 {
    by_frame(pm).map_or(0, |w| w.hwnd)
}

/// The Windows window of a PM window or of the nearest PM ancestor that
/// is one (a scroll bar's window).
pub fn hwnd_above(pm: u32) -> u16 {
    let mut p = pm;
    for _ in 0..8 {
        if p == 0 || p == HWND_DESKTOP {
            return 0;
        }
        let h = hwnd_of(p);
        if h != 0 {
            return h;
        }
        p = unsafe { WinQueryWindow(p, QW_PARENT) };
    }
    0
}

/// The windows of the task, in creation order.
pub fn all() -> Vec<u16> {
    u().wnds.iter().map(|w| w.hwnd).collect()
}

/// The child windows of a window, in Z order (the first created first).
pub fn children(h: u16) -> Vec<u16> {
    u().wnds.iter().filter(|w| w.parent == h && w.style & WS_CHILD != 0).map(|w| w.hwnd).collect()
}

pub fn is_child_style(h: u16) -> bool {
    wnd(h).is_some_and(|w| w.style & WS_CHILD != 0)
}

/// The top-level window a window is in.
pub fn top_level(h: u16) -> u16 {
    let mut h = h;
    for _ in 0..64 {
        match wnd(h) {
            Some(w) if w.style & WS_CHILD != 0 && w.parent != 0 => h = w.parent,
            _ => return h,
        }
    }
    h
}

/// The client height of a window (for y-down coordinates).
pub fn client_height(pm: u32) -> i32 {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(pm, &mut r) };
    r.t - r.b
}

/// The client area's size.
pub fn client_size(h: u16) -> (i32, i32) {
    let Some(c) = wnd(h).map(|w| w.client) else { return (0, 0) };
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(c, &mut r) };
    (r.r - r.l, r.t - r.b)
}

pub fn screen_size() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) }
}

/// A NUL-terminated copy of a string argument.
pub fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// The far addresses of the procedures implemented here (built-in
/// controls, DefWindowProc, DefDlgProc) and their entry numbers.
fn natives() -> &'static [(u32, u32); 9] {
    static N: Global<[(u32, u32); 9]> = Global::new([(0, 0); 9]);
    let n = N.get();
    if n[0].0 == 0 {
        unsafe extern "C" {
            fn p_button();
            fn p_static();
            fn p_edit();
            fn p_listbox();
            fn p_combo();
            fn p_scrollbar();
            fn p_combolbox();
            fn u_defwindowproc();
            fn u_defdlgproc();
        }
        let f = |p: unsafe extern "C" fn()| thunk16::far(p as *const u8);
        *n = [
            (f(p_button), 2),
            (f(p_static), 3),
            (f(p_edit), 4),
            (f(p_listbox), 5),
            (f(p_combo), 6),
            (f(p_scrollbar), 7),
            (f(p_combolbox), 8),
            (f(u_defwindowproc), 2107),
            (f(u_defdlgproc), 2308),
        ];
    }
    n
}

/// The 16:16 address of a native procedure.
pub fn native_far(idx: u32) -> u32 {
    natives().iter().find(|n| n.1 == idx).map_or(0, |n| n.0)
}

/// Call a window procedure (16-bit, or one of ours).
pub fn call_proc(proc_: u32, hwnd: u16, msg: u16, wp: u16, lp: u32) -> u32 {
    if proc_ == 0 {
        return 0;
    }
    if let Some(&(_, idx)) = natives().iter().find(|n| n.0 == proc_) {
        return crate::native(idx, hwnd, msg, wp, lp);
    }
    PascalCall::new().w(hwnd).w(msg).w(wp).l(lp).call(proc_)
}

/// Send a message to a window: its procedure, now.
pub fn send(hwnd: u16, msg: u16, wp: u16, lp: u32) -> u32 {
    let Some(p) = wnd(hwnd).map(|w| w.proc_) else { return 0 };
    call_proc(p, hwnd, msg, wp, lp)
}

/// WM_COMMAND notification from a control to its parent.
pub fn notify(ctl: u16, code: u16) {
    let Some((parent, id)) = wnd(ctl).map(|w| (w.parent, w.id)) else { return };
    send(parent, W_COMMAND, id, (code as u32) << 16 | ctl as u32);
}

/// Send a translated PM message: DefWindowProc for it goes to PM.
fn send_inflight(hwnd: u16, msg: u16, wp: u16, lp: u32, q: QMsg) -> u32 {
    u().inflight.push(InFlight { hwnd, msg, q });
    let r = send(hwnd, msg, wp, lp);
    u().inflight.pop();
    r
}

/// Free the destroyed windows (when no built-in procedure runs).
pub fn collect() {
    if crate::native_depth() == 0 {
        u().gone.clear();
    }
}

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    match ord {
        // MessageBox(hwnd, text, caption, type)
        1 => {
            let (h, text, cap, ty) = (a.w(), a.l(), a.l(), a.w());
            message_box(h, text, cap, ty)
        }
        // InitApp(hInstance)
        5 => {
            a.w();
            init_app()
        }
        // PostQuitMessage(code)
        6 => {
            let code = a.w();
            unsafe { WinPostQueueMsg(TASK.get().hmq, WM_QUIT, code as u32, 0) };
            0
        }
        // SetTimer(hwnd, id, ms, proc)
        10 => {
            let (h, id, ms, p) = (a.w(), a.w(), a.w(), a.l());
            set_timer(h, id, ms, p) as u32
        }
        // KillTimer(hwnd, id)
        12 => {
            let (h, id) = (a.w(), a.w());
            let Some(i) = u().timers.iter().position(|t| t.hwnd == h && t.id == id) else { return 0 };
            let t = u().timers.remove(i);
            let pm = wnd(h).map_or(0, |w| w.client);
            unsafe { WinStopTimer(TASK.get().hab, pm, t.pm_id) };
            1
        }
        // GetTickCount, GetCurrentTime
        13 | 15 => unsafe { WinGetCurrentTime(TASK.get().hab) },
        // GetCursorPos(lpPoint)
        17 => {
            let p = a.p() as *mut i16;
            let mut pt = PointL::default();
            unsafe { WinQueryPointerPos(HWND_DESKTOP, &mut pt) };
            let (_, sy) = screen_size();
            if !p.is_null() {
                unsafe {
                    p.write_unaligned(pt.x as i16);
                    p.add(1).write_unaligned((sy - 1 - pt.y) as i16);
                }
            }
            0
        }
        // SetCapture(hwnd): the previous capture window.
        18 => {
            let h = a.w();
            set_capture(h) as u32
        }
        // ReleaseCapture
        19 => {
            unsafe { WinSetCapture(HWND_DESKTOP, 0) };
            0
        }
        // SetDoubleClickTime(ms), GetDoubleClickTime
        20 => {
            a.w();
            0
        }
        21 => 500,
        // SetFocus(hwnd): the previous focus window.
        22 => set_focus(a.w()) as u32,
        // GetFocus
        23 => focus() as u32,
        // ClientToScreen(hwnd, lpPoint), ScreenToClient(hwnd, lpPoint)
        28 | 29 => {
            let (h, p) = (a.w(), a.p() as *mut i16);
            map_point(h, p, ord == 28);
            0
        }
        // WindowFromPoint(POINT)
        30 => {
            let pt = a.l();
            let (x, y) = (pt as i16 as i32, (pt >> 16) as i16 as i32);
            let sy = screen_size().1;
            let pm = unsafe { WinWindowFromPoint(HWND_DESKTOP, &PointL { x, y: sy - 1 - y }, 1) };
            hwnd_above(pm) as u32
        }
        // IsIconic(hwnd), IsZoomed(hwnd)
        31 | 272 => {
            let h = a.w();
            let Some(f) = wnd(h).map(|w| w.frame) else { return 0 };
            let mut swp = Swp::default();
            unsafe { WinQueryWindowPos(f, &mut swp) };
            (swp.fl & if ord == 31 { SWP_MINIMIZE } else { SWP_MAXIMIZE } != 0) as u32
        }
        // GetWindowRect(hwnd, lpRect): screen coordinates.
        32 => {
            let (h, r) = (a.w(), a.l());
            if let Some(f) = wnd(h).map(|w| w.frame) {
                write_rect(r, screen_rect(f));
            }
            0
        }
        // GetClientRect(hwnd, lpRect)
        33 => {
            let (h, r) = (a.w(), a.l());
            if wnd(h).is_some() {
                let (cx, cy) = client_size(h);
                write_rect(r, [0, 0, cx, cy]);
            }
            0
        }
        // EnableWindow(hwnd, enable): was it disabled?
        34 => {
            let (h, on) = (a.w(), a.w());
            enable_window(h, on != 0) as u32
        }
        // IsWindowEnabled(hwnd)
        35 => wnd(a.w()).is_some_and(|w| w.style & WS_DISABLED == 0) as u32,
        // GetWindowText(hwnd, buf, max)
        36 => {
            let (h, buf, max) = (a.w(), a.l(), a.i());
            if buf == 0 || max <= 0 {
                return 0;
            }
            send(h, W_GETTEXT, max as u16, buf)
        }
        // SetWindowText(hwnd, text)
        37 => {
            let (h, t) = (a.w(), a.l());
            send(h, W_SETTEXT, 0, t);
            0
        }
        // GetWindowTextLength(hwnd)
        38 => {
            let h = a.w();
            send(h, W_GETTEXTLENGTH, 0, 0)
        }
        // BeginPaint(hwnd, lpPaint)
        39 => {
            let (h, ps) = (a.w(), a.p());
            begin_paint(h, ps) as u32
        }
        // EndPaint(hwnd, lpPaint)
        40 => {
            let (_h, ps) = (a.w(), a.p());
            if !ps.is_null() {
                let hdc = unsafe { (ps as *const u16).read_unaligned() };
                gdi::release_dc(hdc);
            }
            0
        }
        // CreateWindow(class, title, style, x, y, cx, cy, parent, menu,
        // instance, param)
        41 => {
            let (class, title, style) = (a.l(), a.l(), a.l());
            let (x, y, cx, cy) = (a.i(), a.i(), a.i(), a.i());
            let (parent, menu, hinst, param) = (a.w(), a.w(), a.w(), a.l());
            create_window(0, class, title, style, [x, y, cx, cy], parent, menu, hinst, param) as u32
        }
        // ShowWindow(hwnd, cmd): was it visible?
        42 => {
            let (h, cmd) = (a.w(), a.w());
            show_window(h, cmd) as u32
        }
        // BringWindowToTop(hwnd)
        45 => {
            let h = a.w();
            if let Some(f) = wnd(h).map(|w| w.frame) {
                unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER) };
            }
            1
        }
        // GetParent(hwnd)
        46 => {
            let h = a.w();
            wnd(h).map_or(0, |w| w.parent as u32)
        }
        // IsWindow(hwnd)
        47 => wnd(a.w()).is_some() as u32,
        // IsChild(parent, hwnd)
        48 => {
            let (p, h) = (a.w(), a.w());
            is_child(p, h) as u32
        }
        // IsWindowVisible(hwnd)
        49 => {
            let h = a.w();
            wnd(h).is_some_and(|w| unsafe { WinIsWindowVisible(w.frame) } != 0) as u32
        }
        // FindWindow(class, title): the task's own windows.
        50 => {
            let (c, t) = (a.l(), a.l());
            find_window(c, t) as u32
        }
        // DestroyWindow(hwnd)
        53 => destroy_window(a.w()) as u32,
        // EnumWindows(proc, lParam), EnumChildWindows(hwnd, proc, lParam)
        54 | 55 => {
            let h = if ord == 55 { a.w() } else { 0 };
            let (p, lp) = (a.l(), a.l());
            let list: Vec<u16> = if ord == 54 {
                u().wnds.iter().filter(|w| w.style & WS_CHILD == 0).map(|w| w.hwnd).collect()
            } else {
                descendants(h)
            };
            for w in list {
                if wnd(w).is_some() && PascalCall::new().w(w).l(lp).call(p) as u16 == 0 {
                    break;
                }
            }
            1
        }
        // MoveWindow(hwnd, x, y, cx, cy, repaint)
        56 => {
            let (h, x, y, cx, cy, _rp) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.w());
            move_window(h, x, y, cx, cy, SWP_MOVE | SWP_SIZE) as u32
        }
        // RegisterClass(lpWndClass)
        57 => {
            let p = a.p();
            register_class(p) as u32
        }
        // GetClassName(hwnd, buf, max)
        58 => {
            let (h, buf, max) = (a.w(), a.p(), a.i());
            let Some(ci) = wnd(h).map(|w| w.class) else { return 0 };
            let name = u().classes[ci].name.clone();
            copy_out(&name, buf, max) as u32
        }
        // SetActiveWindow(hwnd): the previous one.
        59 => {
            let h = a.w();
            let old = hwnd_of(unsafe { WinQueryActiveWindow(HWND_DESKTOP) });
            let t = top_level(h);
            if let Some(f) = wnd(t).map(|w| w.frame) {
                unsafe { WinSetActiveWindow(HWND_DESKTOP, f) };
            }
            old as u32
        }
        // GetActiveWindow
        60 => hwnd_of(unsafe { WinQueryActiveWindow(HWND_DESKTOP) }) as u32,
        // ScrollWindow(hwnd, dx, dy, lpRect, lpClip)
        61 => {
            let (h, dx, dy, r, c) = (a.w(), a.i(), a.i(), a.l(), a.l());
            scroll_window(h, dx, dy, read_rect(r), read_rect(c));
            0
        }
        // SetScrollPos(hwnd, bar, pos, redraw)
        62 => {
            let (h, bar, pos, _rd) = (a.w(), a.w(), a.i(), a.w());
            crate::scroll::set_pos(h, bar, pos) as u32
        }
        // GetScrollPos(hwnd, bar)
        63 => {
            let (h, bar) = (a.w(), a.w());
            crate::scroll::get(h, bar).map_or(0, |b| b.2 as u32 & 0xffff)
        }
        // SetScrollRange(hwnd, bar, min, max, redraw)
        64 => {
            let (h, bar, lo, hi, _rd) = (a.w(), a.w(), a.i(), a.i(), a.w());
            crate::scroll::set_range(h, bar, lo, hi);
            0
        }
        // GetScrollRange(hwnd, bar, lpMin, lpMax)
        65 => {
            let (h, bar, pmin, pmax) = (a.w(), a.w(), a.p() as *mut i16, a.p() as *mut i16);
            let (lo, hi) = crate::scroll::get(h, bar).map_or((0, 0), |b| (b.0, b.1));
            unsafe {
                if !pmin.is_null() {
                    pmin.write_unaligned(lo as i16);
                }
                if !pmax.is_null() {
                    pmax.write_unaligned(hi as i16);
                }
            }
            0
        }
        // GetDC(hwnd), GetWindowDC(hwnd)
        66 | 67 => {
            let h = a.w();
            get_dc(h) as u32
        }
        // ReleaseDC(hwnd, hdc)
        68 => {
            let (_h, hdc) = (a.w(), a.w());
            gdi::release_dc(hdc) as u32
        }
        // SetCursor(hcursor): the previous one.
        69 => set_cursor(a.w()) as u32,
        // SetCursorPos(x, y)
        70 => {
            let (x, y) = (a.i(), a.i());
            let sy = screen_size().1;
            unsafe { WinSetPointerPos(HWND_DESKTOP, x, sy - 1 - y) };
            0
        }
        // ShowCursor(show): the display count.
        71 => {
            a.w();
            0
        }
        // SetRect(lpRect, left, top, right, bottom)
        72 => {
            let r = a.l();
            let v = [a.i(), a.i(), a.i(), a.i()];
            write_rect(r, v);
            0
        }
        // SetRectEmpty(lpRect)
        73 => {
            write_rect(a.l(), [0; 4]);
            0
        }
        // CopyRect(dst, src)
        74 => {
            let (d, s) = (a.l(), a.l());
            if let Some(r) = read_rect(s) {
                write_rect(d, r);
            }
            1
        }
        // IsRectEmpty(lpRect)
        75 => read_rect(a.l()).is_none_or(|r| r[2] <= r[0] || r[3] <= r[1]) as u32,
        // PtInRect(lpRect, POINT)
        76 => {
            let r = a.l();
            let pt = a.l();
            let (x, y) = (pt as i16 as i32, (pt >> 16) as i16 as i32);
            read_rect(r).is_some_and(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]) as u32
        }
        // OffsetRect(lpRect, dx, dy), InflateRect(lpRect, dx, dy)
        77 | 78 => {
            let (p, dx, dy) = (a.l(), a.i(), a.i());
            if let Some(r) = read_rect(p) {
                write_rect(p, if ord == 77 { [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy] } else { [r[0] - dx, r[1] - dy, r[2] + dx, r[3] + dy] });
            }
            0
        }
        // IntersectRect(dst, a, b), UnionRect(dst, a, b): not empty?
        79 | 80 => {
            let (d, s1, s2) = (a.l(), a.l(), a.l());
            let (Some(x), Some(y)) = (read_rect(s1), read_rect(s2)) else { return 0 };
            let empty = |r: [i32; 4]| r[2] <= r[0] || r[3] <= r[1];
            let r = if ord == 79 {
                [x[0].max(y[0]), x[1].max(y[1]), x[2].min(y[2]), x[3].min(y[3])]
            } else if empty(x) {
                y
            } else if empty(y) {
                x
            } else {
                [x[0].min(y[0]), x[1].min(y[1]), x[2].max(y[2]), x[3].max(y[3])]
            };
            let r = if empty(r) { [0; 4] } else { r };
            write_rect(d, r);
            !empty(r) as u32
        }
        // FillRect(hdc, lpRect, hbrush)
        81 => {
            let (hdc, r, br) = (a.w(), a.l(), a.w());
            if let Some(r) = read_rect(r) {
                gdi::fill_rect(hdc, r, br);
            }
            0
        }
        // InvertRect(hdc, lpRect)
        82 => {
            let (hdc, r) = (a.w(), a.l());
            if let Some(r) = read_rect(r) {
                gdi::invert(hdc, r);
            }
            0
        }
        // FrameRect(hdc, lpRect, hbrush)
        83 => {
            let (hdc, r, br) = (a.w(), a.l(), a.w());
            if let Some(r) = read_rect(r) {
                let [l, t, rr, b] = r;
                for e in [[l, t, rr, t + 1], [l, b - 1, rr, b], [l, t, l + 1, b], [rr - 1, t, rr, b]] {
                    gdi::fill_rect(hdc, e, br);
                }
            }
            1
        }
        // DrawIcon(hdc, x, y, hicon): rs2's application icon.
        84 => {
            let (hdc, x, y, _i) = (a.w(), a.i(), a.i(), a.w());
            crate::ctl::draw_icon(hdc, x, y);
            1
        }
        // DrawText(hdc, text, count, lpRect, format)
        85 => {
            let (hdc, s, n, r, fmt) = (a.w(), a.l(), a.i(), a.l(), a.w());
            gdi::draw_text(hdc, s, n, r, fmt) as u32
        }
        // MessageBeep(type)
        104 => {
            let t = a.w();
            unsafe { WinAlarm(HWND_DESKTOP, if t == 0x10 { 1 } else if t == 0x30 { 0 } else { 2 }) };
            0
        }
        // FlashWindow(hwnd, invert)
        105 => {
            let (h, on) = (a.w(), a.w());
            if let Some(f) = wnd(h).map(|w| w.frame) {
                unsafe { WinFlashWindow(f, on as u32) };
            }
            0
        }
        // GetKeyState(vk), GetAsyncKeyState(vk)
        106 | 249 => key_state(a.w()) as u32,
        // DefWindowProc(hwnd, msg, wParam, lParam)
        107 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            def_window_proc(h, m, wp, lp)
        }
        // GetMessage(lpMsg, hwnd, min, max)
        108 => {
            let (p, h, lo, hi) = (a.p(), a.w(), a.w(), a.w());
            get_message(p, h, lo, hi, None)
        }
        // PeekMessage(lpMsg, hwnd, min, max, flags)
        109 => {
            let (p, h, lo, hi, fl) = (a.p(), a.w(), a.w(), a.w(), a.w());
            get_message(p, h, lo, hi, Some(fl))
        }
        // PostMessage(hwnd, msg, wParam, lParam)
        110 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            post(h, m, wp, lp) as u32
        }
        // SendMessage(hwnd, msg, wParam, lParam)
        111 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            send(h, m, wp, lp)
        }
        // WaitMessage
        112 => {
            wait_message();
            0
        }
        // TranslateMessage(lpMsg): WM_CHAR for a key with a character.
        113 => {
            let p = a.p() as *const u16;
            translate(p) as u32
        }
        // DispatchMessage(lpMsg)
        114 => {
            let p = a.p();
            dispatch(p)
        }
        // PostAppMessage(task, msg, wParam, lParam)
        116 => {
            let (_t, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            post(0, m, wp, lp) as u32
        }
        // RegisterWindowMessage(name): 0xC000 and up.
        118 => {
            let s = far_str(a.l()).unwrap_or(b"").to_ascii_uppercase();
            let m = &mut u().messages;
            let i = m.iter().position(|x| *x == s).unwrap_or_else(|| {
                m.push(s);
                m.len() - 1
            });
            0xc100 + i as u32
        }
        // GetMessagePos, GetMessageTime
        119 => u().msg_pos,
        120 => u().msg_time,
        // CallWindowProc(proc, hwnd, msg, wParam, lParam)
        122 => {
            let (p, h, m, wp, lp) = (a.l(), a.w(), a.w(), a.w(), a.l());
            call_proc(p, h, m, wp, lp)
        }
        // UpdateWindow(hwnd)
        124 => {
            let h = a.w();
            if let Some(c) = wnd(h).map(|w| w.client) {
                unsafe { WinUpdateWindow(c) };
            }
            0
        }
        // InvalidateRect(hwnd, lpRect, erase), ValidateRect(hwnd, lpRect)
        125 | 127 => {
            let (h, r) = (a.w(), a.l());
            if ord == 125 {
                a.w();
            }
            if h == 0 {
                return 0;
            }
            if ord == 125 { invalidate(h, read_rect(r)) } else { validate(h, read_rect(r)) }
            0
        }
        // InvalidateRgn(hwnd, hrgn, erase), ValidateRgn(hwnd, hrgn): all.
        126 | 128 => {
            let (h, _r) = (a.w(), a.w());
            if ord == 126 {
                a.w();
                invalidate(h, None);
            } else {
                validate(h, None);
            }
            0
        }
        // GetClassWord(hwnd, i), GetClassLong(hwnd, i)
        129 | 131 => {
            let (h, i) = (a.w(), a.i());
            class_value(h, i, None, ord == 131)
        }
        // SetClassWord(hwnd, i, v)
        130 => {
            let (h, i, v) = (a.w(), a.i(), a.w());
            class_value(h, i, Some(v as u32), false)
        }
        // SetClassLong(hwnd, i, v)
        132 => {
            let (h, i, v) = (a.w(), a.i(), a.l());
            class_value(h, i, Some(v), true)
        }
        // GetWindowWord(hwnd, index), GetWindowLong(hwnd, index)
        133 | 135 => {
            let (h, i) = (a.w(), a.i());
            window_value(h, i, None, ord == 135)
        }
        // SetWindowWord(hwnd, index, value)
        134 => {
            let (h, i, v) = (a.w(), a.i(), a.w());
            window_value(h, i, Some(v as u32), false)
        }
        // SetWindowLong(hwnd, index, value)
        136 => {
            let (h, i, v) = (a.w(), a.i(), a.l());
            window_value(h, i, Some(v), true)
        }
        // LoadCursor(hinst, name), LoadIcon(hinst, name): the built-in
        // ones (IDC_*, IDI_*); the program's own are drawn as rs2's.
        173 | 174 => {
            let (hi, name) = (a.w(), a.l());
            if name >> 16 == 0 {
                let id = name as u16;
                if hi == 0 || (32512..=32651).contains(&id) {
                    return id as u32;
                }
            }
            // IDC_ARROW, IDI_APPLICATION
            32512
        }
        // GetSystemMetrics(index)
        179 => system_metric(a.i()) as u32,
        // GetSysColor(index)
        180 => gdi::sys_color(a.w()),
        // GetUpdateRect(hwnd, lpRect, erase)
        190 => {
            let (h, r, _e) = (a.w(), a.l(), a.w());
            let Some(c) = wnd(h).map(|w| w.client) else { return 0 };
            let mut rc = RectL::default();
            let some = unsafe { WinQueryUpdateRect(c, &mut rc) } != 0 && rc.r > rc.l && rc.t > rc.b;
            let ch = client_height(c);
            write_rect(r, if some { [rc.l, ch - rc.t, rc.r, ch - rc.b] } else { [0; 4] });
            some as u32
        }
        // ChildWindowFromPoint(hwnd, POINT)
        191 => {
            let (h, pt) = (a.w(), a.l());
            child_from_point(h, pt as i16 as i32, (pt >> 16) as i16 as i32) as u32
        }
        // InSendMessage
        192 => 0,
        // GetKeyboardState(lpKeys)
        222 => {
            let p = a.p();
            if !p.is_null() {
                for vk in 0..256u16 {
                    let s = key_state(vk);
                    unsafe { *p.add(vk as usize) = ((s >> 8) as u8 & 0x80) | (s as u8 & 1) };
                }
            }
            0
        }
        // SetKeyboardState(lpKeys)
        223 => {
            a.l();
            0
        }
        // GetWindowTask(hwnd)
        224 => {
            a.w();
            TASK.get().hinst as u32
        }
        // GetTopWindow(hwnd)
        229 => {
            let h = a.w();
            get_window(h, 5) as u32
        }
        // GetNextWindow(hwnd, cmd)
        230 => {
            let (h, c) = (a.w(), a.w());
            get_window(h, c) as u32
        }
        // SetWindowPos(hwnd, after, x, y, cx, cy, flags)
        232 => {
            let (h, _after, x, y, cx, cy, fl) = (a.w(), a.w(), a.i(), a.i(), a.i(), a.i(), a.w());
            let mut f = 0;
            if fl & 1 == 0 {
                f |= SWP_SIZE;
            }
            if fl & 2 == 0 {
                f |= SWP_MOVE;
            }
            if fl & 0x40 != 0 {
                f |= SWP_SHOW;
            }
            if fl & 0x80 != 0 {
                f |= SWP_HIDE;
            }
            if fl & 0x14 == 0 && !is_child_style(h) {
                f |= SWP_ACTIVATE | SWP_ZORDER;
            }
            if let Some(w) = wnd(h) {
                if fl & 0x40 != 0 {
                    w.style |= WS_VISIBLE;
                }
                if fl & 0x80 != 0 {
                    w.style &= !WS_VISIBLE;
                }
            }
            move_window(h, x, y, cx, cy, f) as u32
        }
        // SetParent(hwnd, parent): the old parent.
        233 => {
            let (h, p) = (a.w(), a.w());
            let Some((f, old)) = wnd(h).map(|w| (w.frame, w.parent)) else { return 0 };
            let pc = if p == 0 { HWND_DESKTOP } else { wnd(p).map_or(HWND_DESKTOP, |w| w.client) };
            unsafe { WinSetParent(f, pc, 1) };
            if let Some(w) = wnd(h) {
                w.parent = p;
            }
            old as u32
        }
        // GetCapture
        236 => hwnd_above(unsafe { WinQueryCapture(HWND_DESKTOP) }) as u32,
        // EqualRect(a, b)
        244 => {
            let (x, y) = (a.l(), a.l());
            (read_rect(x) == read_rect(y)) as u32
        }
        // GetCursor
        247 => 32512,
        // MapWindowPoints(from, to, points, n)
        258 => {
            let (from, to, p, n) = (a.w(), a.w(), a.p() as *mut i16, a.w());
            for i in 0..n as usize {
                map_point(from, unsafe { p.add(2 * i) }, true);
                map_point(to, unsafe { p.add(2 * i) }, false);
            }
            0
        }
        // GetWindow(hwnd, cmd)
        262 => {
            let (h, c) = (a.w(), a.w());
            get_window(h, c) as u32
        }
        // ShowScrollBar(hwnd, bar, show), EnableScrollBar(hwnd, bar, flags)
        267 => {
            let (h, bar, on) = (a.w(), a.w(), a.w());
            crate::scroll::show(h, bar, on != 0);
            0
        }
        482 => {
            let (h, bar, fl) = (a.w(), a.w(), a.w());
            crate::scroll::enable(h, bar, fl) as u32
        }
        // GetDesktopWindow, GetDesktopHwnd
        278 | 286 => 0,
        // GetLastActivePopup(hwnd)
        287 => a.w() as u32,
        // RedrawWindow(hwnd, lpRect, hrgn, flags)
        290 => {
            let (h, r, _rgn, fl) = (a.w(), a.l(), a.w(), a.w());
            if fl & 0x0001 != 0 {
                invalidate(h, read_rect(r));
            }
            if fl & 0x0008 != 0 {
                validate(h, read_rect(r));
            }
            if fl & 0x0300 != 0 {
                if let Some(c) = wnd(h).map(|w| w.client) {
                    unsafe { WinUpdateWindow(c) };
                }
            }
            1
        }
        // DefDlgProc(hwnd, msg, wParam, lParam)
        308 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            crate::dialog::def_dlg_proc(h, m, wp, lp)
        }
        // SubtractRect(dst, a, b)
        373 => {
            let (d, s1, s2) = (a.l(), a.l(), a.l());
            let (Some(x), Some(y)) = (read_rect(s1), read_rect(s2)) else { return 0 };
            let mut r = x;
            if y[1] <= x[1] && y[3] >= x[3] {
                if y[0] <= x[0] && y[2] > x[0] {
                    r[0] = y[2];
                }
                if y[2] >= x[2] && y[0] < x[2] {
                    r[2] = y[0];
                }
            }
            if y[0] <= x[0] && y[2] >= x[2] {
                if y[1] <= x[1] && y[3] > x[1] {
                    r[1] = y[3];
                }
                if y[3] >= x[3] && y[1] < x[3] {
                    r[3] = y[1];
                }
            }
            let empty = r[2] <= r[0] || r[3] <= r[1];
            write_rect(d, if empty { [0; 4] } else { r });
            !empty as u32
        }
        // UnregisterClass(name, hinst)
        403 => {
            let (n, _hi) = (a.l(), a.w());
            match find_class(n) {
                Some(i) if !u().wnds.iter().any(|w| w.class == i) && u().classes[i].atom >= 0xc000 => {
                    u().classes[i].name.clear();
                    u().classes[i].atom = 0;
                    1
                }
                _ => 0,
            }
        }
        // GetClassInfo(hinst, name, lpWndClass)
        404 => {
            let (_hi, n, p) = (a.w(), a.l(), a.p());
            let Some(i) = find_class(n) else { return 0 };
            let c = &u().classes[i];
            if !p.is_null() {
                let mut b = [0u8; 26];
                b[0..2].copy_from_slice(&c.style.to_le_bytes());
                b[2..6].copy_from_slice(&c.proc_.to_le_bytes());
                b[6..8].copy_from_slice(&(c.cls_extra.len() as u16).to_le_bytes());
                b[8..10].copy_from_slice(&c.wnd_extra.to_le_bytes());
                b[10..12].copy_from_slice(&c.hinst.to_le_bytes());
                b[12..14].copy_from_slice(&c.icon.to_le_bytes());
                b[14..16].copy_from_slice(&c.cursor.to_le_bytes());
                b[16..18].copy_from_slice(&c.brush.to_le_bytes());
                b[22..26].copy_from_slice(&n.to_le_bytes());
                unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 26) };
            }
            1
        }
        // CreateWindowEx(exstyle, class, title, style, x, y, cx, cy,
        // parent, menu, instance, param)
        452 => {
            let (ex, class, title, style) = (a.l(), a.l(), a.l(), a.l());
            let (x, y, cx, cy) = (a.i(), a.i(), a.i(), a.i());
            let (parent, menu, hinst, param) = (a.w(), a.w(), a.w(), a.l());
            create_window(ex, class, title, style, [x, y, cx, cy], parent, menu, hinst, param) as u32
        }
        // AdjustWindowRect(lpRect, style, menu), AdjustWindowRectEx(+ex)
        102 | 454 => {
            let (r, style, menu) = (a.l(), a.l(), a.w());
            if ord == 454 {
                a.l();
            }
            if let Some(rc) = read_rect(r) {
                let (l, t, rr, b) = frame_insets(style, menu != 0);
                write_rect(r, [rc[0] - l, rc[1] - t, rc[2] + rr, rc[3] + b]);
            }
            1
        }
        // DrawFocusRect(hdc, lpRect)
        466 => {
            let (hdc, r) = (a.w(), a.l());
            if let Some(r) = read_rect(r) {
                gdi::focus_rect(hdc, r);
            }
            0
        }
        _ => crate::misc::call(ord, a),
    }
}

fn init_app() -> u32 {
    let t = TASK.get();
    if t.hmq != 0 {
        return 1;
    }
    t.hab = unsafe { WinInitialize(0) };
    t.hmq = unsafe { WinCreateMsgQueue(t.hab, 0) };
    if t.hmq == 0 {
        trace(format_args!("WIN16: no PM message queue"));
        return 0;
    }
    unsafe {
        WinRegisterClass(t.hab, b"Win16Wnd\0".as_ptr(), pm_wndproc, 0, 4);
        WinRegisterClass(t.hab, b"Win16Frame\0".as_ptr(), frame_proc, 0, 4);
    }
    crate::scroll::register(t.hab);
    // The built-in classes: (name, procedure, style, extra bytes,
    // cursor, brush).
    let builtin: [(&[u8], u32, u16, u16, u16, u16); 8] = [
        (b"BUTTON", 2, CS_DBLCLKS | 0x80, 0, 32512, 0),
        (b"STATIC", 3, 0x80, 0, 32512, 0),
        (b"EDIT", 4, CS_DBLCLKS | 0x80, 0, 32513, 0),
        (b"LISTBOX", 5, CS_DBLCLKS | 0x80, 0, 32512, 0),
        (b"COMBOBOX", 6, CS_DBLCLKS | 0x80, 0, 32512, 0),
        (b"SCROLLBAR", 7, CS_DBLCLKS | 0x80, 0, 32512, 0),
        (b"COMBOLBOX", 8, CS_DBLCLKS | 0x80, 0, 32512, 0),
        (b"#32770", 2308, 0x0800, crate::dialog::DLGWINDOWEXTRA, 32512, 0),
    ];
    for (i, (name, idx, style, extra, cursor, brush)) in builtin.into_iter().enumerate() {
        u().classes.push(Class {
            name: name.to_vec(),
            atom: if idx == 2308 { 0x8002 } else { 0x8080 + i as u16 },
            style,
            proc_: native_far(idx),
            cls_extra: Vec::new(),
            wnd_extra: extra,
            hinst: 0,
            icon: 0,
            cursor,
            brush,
            menu: None,
        });
    }
    1
}

/// WNDCLASS: style, lpfnWndProc, cbClsExtra, cbWndExtra, hInstance,
/// hIcon, hCursor, hbrBackground, lpszMenuName, lpszClassName.
fn register_class(p: *mut u8) -> u16 {
    if p.is_null() {
        return 0;
    }
    let w = |o: usize| unsafe { (p.add(o) as *const u16).read_unaligned() };
    let l = |o: usize| unsafe { (p.add(o) as *const u32).read_unaligned() };
    let Some(name) = far_str(l(22)) else { return 0 };
    let name = name.to_ascii_uppercase();
    let s = u();
    if s.classes.iter().any(|c| c.name == name && c.hinst != 0) {
        return 0;
    }
    // A program's class of a built-in name replaces it for the program.
    if let Some(c) = s.classes.iter_mut().find(|c| c.name == name) {
        c.name.clear();
    }
    let atom = s.next_atom;
    s.next_atom += 1;
    s.classes.push(Class {
        name,
        atom,
        style: w(0),
        proc_: l(2),
        cls_extra: alloc::vec![0; w(6) as usize],
        wnd_extra: w(8),
        hinst: w(10).max(1),
        icon: w(12),
        cursor: w(14),
        brush: w(16),
        menu: crate::res::Name::of(l(18)),
    });
    atom
}

pub fn find_class(name: u32) -> Option<usize> {
    let s = u();
    match far_str(name) {
        Some(n) => s.classes.iter().position(|c| !c.name.is_empty() && c.name.eq_ignore_ascii_case(n)),
        None => s.classes.iter().position(|c| c.atom == name as u16 && c.atom != 0),
    }
}

pub fn find_class_name(n: &[u8]) -> Option<usize> {
    u().classes.iter().position(|c| !c.name.is_empty() && c.name.eq_ignore_ascii_case(n))
}

pub fn class_atom(ci: usize) -> u16 {
    u().classes.get(ci).map_or(0, |c| c.atom)
}

/// Frame creation flags for a window style.
fn frame_flags(style: u32, owned: bool) -> u32 {
    let mut f = FCF_NOBYTEALIGN;
    if style & WS_CAPTION == WS_CAPTION {
        f |= FCF_TITLEBAR;
        if !owned {
            f |= FCF_TASKLIST;
        }
    }
    if style & WS_SYSMENU != 0 {
        f |= FCF_SYSMENU;
    }
    if style & WS_MINIMIZEBOX != 0 {
        f |= FCF_MINBUTTON;
    }
    if style & WS_MAXIMIZEBOX != 0 {
        f |= FCF_MAXBUTTON;
    }
    if style & WS_THICKFRAME != 0 {
        f |= FCF_SIZEBORDER;
    } else if style & WS_DLGFRAME != 0 {
        f |= FCF_DLGBORDER;
    } else if style & WS_BORDER != 0 {
        f |= FCF_BORDER;
    }
    f
}

/// The non-client insets (left, top, right, bottom) of a window style
/// (AdjustWindowRect).
pub fn frame_insets(style: u32, menu: bool) -> (i32, i32, i32, i32) {
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    let b = if style & WS_THICKFRAME != 0 {
        q(SV_CXSIZEBORDER)
    } else if style & WS_DLGFRAME != 0 {
        q(SV_CXDLGFRAME)
    } else if style & WS_BORDER != 0 {
        q(SV_CXBORDER)
    } else {
        0
    };
    let mut top = b;
    if style & WS_CAPTION == WS_CAPTION {
        top += q(SV_CYTITLEBAR);
    }
    if menu {
        top += q(SV_CYMENU);
    }
    let r = b + if style & WS_VSCROLL != 0 { q(SV_CXVSCROLL) } else { 0 };
    let bb = b + if style & WS_HSCROLL != 0 { q(SV_CYHSCROLL) } else { 0 };
    (b, top, r, bb)
}

/// The border of a plain window (WS_THICKFRAME, WS_DLGFRAME, WS_BORDER).
fn plain_border(nc: u32) -> i32 {
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    if nc & WS_THICKFRAME != 0 {
        q(SV_CXSIZEBORDER)
    } else if nc & WS_DLGFRAME != 0 {
        q(SV_CXDLGFRAME)
    } else if nc & WS_BORDER != 0 {
        q(SV_CXBORDER).max(1)
    } else {
        0
    }
}

/// Is a window of this style a PM frame (rather than a plain window)?
fn is_framed(style: u32) -> bool {
    style & WS_CHILD == 0 && (style & WS_POPUP == 0 || style & WS_CAPTION == WS_CAPTION || style & (WS_THICKFRAME | WS_DLGFRAME) != 0)
}

/// Does a plain window of this style have a non-client area?
fn has_nonclient(style: u32) -> bool {
    style & (WS_BORDER | WS_DLGFRAME | WS_THICKFRAME | WS_VSCROLL | WS_HSCROLL) != 0
}

#[allow(clippy::too_many_arguments)]
fn create_window(exstyle: u32, class: u32, title: u32, style: u32, pos: [i32; 4], parent: u16, menu: u16, hinst: u16, param: u32) -> u16 {
    let Some(ci) = find_class(class) else {
        trace(format_args!(
            "WIN16: CreateWindow: class {} not found",
            far_str(class).map(|n| n.iter().map(|&c| c as char).collect::<alloc::string::String>()).unwrap_or_default()
        ));
        return 0;
    };
    let text = far_str(title).unwrap_or(b"").to_vec();
    create(&Create { exstyle, class: ci, title: &text, title_far: title, class_far: class, style, pos, parent, menu, hinst, param })
}

/// What CreateWindow is given.
pub struct Create<'a> {
    pub exstyle: u32,
    pub class: usize,
    pub title: &'a [u8],
    /// The title and class as far pointers (for CREATESTRUCT), or 0.
    pub title_far: u32,
    pub class_far: u32,
    pub style: u32,
    pub pos: [i32; 4],
    pub parent: u16,
    /// The menu (top-level windows) or the ID (child windows).
    pub menu: u16,
    pub hinst: u16,
    pub param: u32,
}

/// Create a window: the PM windows, the table entry, WM_NCCREATE and
/// WM_CREATE, WM_SIZE and WM_MOVE; shown if WS_VISIBLE.
pub fn create(c: &Create) -> u16 {
    let ci = c.class;
    let (cproc, cextra) = {
        let cl = &u().classes[ci];
        (cl.proc_, cl.wnd_extra)
    };
    let style = c.style;
    let text = z(c.title);
    let [mut x, mut y, mut cx, mut cy] = c.pos;
    let child = style & WS_CHILD != 0;
    let framed = is_framed(style);
    let parent = c.parent;
    let combo = u().classes[ci].name == b"COMBOBOX";
    let nc = style & if combo { WS_DLGFRAME | WS_THICKFRAME } else { WS_BORDER | WS_DLGFRAME | WS_THICKFRAME | WS_VSCROLL | WS_HSCROLL };
    if child && wnd(parent).is_none() {
        return 0;
    }
    let (sx, sy) = screen_size();
    if !child {
        if x == CW_USEDEFAULT {
            let s = u();
            let k = s.cascade % 8;
            s.cascade += 1;
            x = 24 + 24 * k;
            y = 24 + 24 * k;
        }
        if cx == CW_USEDEFAULT {
            cx = sx * 3 / 4;
            cy = sy * 3 / 4;
        }
    } else {
        if x == CW_USEDEFAULT {
            x = 0;
            y = 0;
        }
        if cx == CW_USEDEFAULT {
            cx = 100;
            cy = 100;
        }
    }
    // A drop-down combo box is the height of its selection field; the
    // height given is for the dropped list (combo_create gets it from
    // CREATESTRUCT).
    let full_cy = cy;
    if combo && style & 3 > 1 {
        cy = crate::listbox::field_height();
    }
    let (frame, client) = if framed {
        let mut fcf = frame_flags(style, parent != 0);
        let mut client = 0;
        let f = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"Win16Wnd\0".as_ptr(), text.as_ptr(), crate::pm::WS_CLIPCHILDREN, 0, 0, &mut client) };
        (f, client)
    } else {
        let pm_parent = if child { wnd(parent).map_or(0, |w| w.client) } else { HWND_DESKTOP };
        let ph = if child { client_height(pm_parent) } else { sy };
        let pmy = ph - y - cy;
        // A child goes below its siblings (the first one created is the
        // top of the Z order, as GetWindow(GW_CHILD) has it).
        let behind = HWND_TOP;
        let clip = crate::pm::WS_CLIPCHILDREN | WS_CLIPSIBLINGS;
        if has_nonclient(nc) {
            let f = unsafe { WinCreateWindow(pm_parent, b"Win16Frame\0".as_ptr(), text.as_ptr(), clip, x, pmy, cx, cy, pm_parent, behind, c.menu as u32, core::ptr::null(), core::ptr::null()) };
            let cl = unsafe { WinCreateWindow(f, b"Win16Wnd\0".as_ptr(), text.as_ptr(), clip | crate::pm::WS_VISIBLE, 0, 0, cx, cy, f, HWND_TOP, FID_CLIENT, core::ptr::null(), core::ptr::null()) };
            (f, cl)
        } else {
            let h = unsafe { WinCreateWindow(pm_parent, b"Win16Wnd\0".as_ptr(), text.as_ptr(), clip, x, pmy, cx, cy, pm_parent, behind, c.menu as u32, core::ptr::null(), core::ptr::null()) };
            (h, h)
        }
    };
    if frame == 0 || client == 0 {
        return 0;
    }
    let s = u();
    let hwnd = s.next_hwnd;
    s.next_hwnd = s.next_hwnd.wrapping_add(4).max(0x2004);
    s.wnds.push(Wnd {
        hwnd,
        frame,
        client,
        class: ci,
        proc_: cproc,
        style: style & !WS_VISIBLE,
        exstyle: c.exstyle,
        parent,
        id: if child { c.menu } else { 0 },
        hinst: c.hinst,
        extra: alloc::vec![0; cextra as usize],
        text: c.title.to_vec(),
        menu: 0,
        framed,
        nc,
        data: None,
        vsb: None,
        hsb: None,
    });
    if !child && parent != 0 {
        let of = wnd(top_level(parent)).map_or(0, |w| w.frame);
        unsafe { WinSetOwner(frame, of) };
    }
    let bars = if framed { style } else { nc };
    if bars & WS_VSCROLL != 0 {
        crate::scroll::create_bar(hwnd, true);
    }
    if bars & WS_HSCROLL != 0 {
        crate::scroll::create_bar(hwnd, false);
    }
    if framed {
        // The menu: the one given, or the class's.
        let m = if c.menu != 0 {
            c.menu
        } else {
            let name = u().classes[ci].menu.clone();
            name.map_or(0, |n| crate::menu::load(c.hinst, &n))
        };
        if m != 0 {
            crate::menu::set_menu(hwnd, m);
        }
        unsafe {
            WinSendMsg(frame, WM_UPDATEFRAME, 0, 0);
            WinSetWindowPos(frame, HWND_TOP, x, sy - y - cy, cx, cy, SWP_MOVE | SWP_SIZE);
        }
    } else {
        layout_frame(hwnd);
    }
    if style & WS_DISABLED != 0 {
        unsafe { WinEnableWindow(frame, 0) };
    }
    // WM_NCCREATE and WM_CREATE get a CREATESTRUCT (on the 16-bit
    // stack): lpCreateParams, hInstance, hMenu, hwndParent, cy, cx, y,
    // x, style, lpszName, lpszClass, dwExStyle.
    let saved = thunk16::sp16();
    let title_far = if c.title_far != 0 { c.title_far } else { crate::mem16::push_str(c.title) };
    let class_far = if c.class_far != 0 { c.class_far } else { class_atom(ci) as u32 };
    let cs = thunk16::alloc16(34);
    let p = flat(cs);
    let mut b = Vec::with_capacity(34);
    b.extend_from_slice(&c.param.to_le_bytes());
    for v in [c.hinst, c.menu, parent, full_cy as u16, cx as u16, y as u16, x as u16] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&style.to_le_bytes());
    b.extend_from_slice(&title_far.to_le_bytes());
    b.extend_from_slice(&class_far.to_le_bytes());
    b.extend_from_slice(&c.exstyle.to_le_bytes());
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
    let ok = send(hwnd, W_NCCREATE, 0, cs) != 0 && send(hwnd, W_CREATE, 0, cs) as i32 != -1;
    thunk16::set_sp16(saved);
    if !ok || wnd(hwnd).is_none() {
        destroy_window(hwnd);
        return 0;
    }
    let (w, h) = client_size(hwnd);
    send(hwnd, W_SIZE, 0, (h as u32) << 16 | w as u32 & 0xffff);
    send(hwnd, W_MOVE, 0, client_origin(hwnd));
    if child {
        let id = c.menu;
        send(parent, W_PARENTNOTIFY, W_CREATE, (id as u32) << 16 | hwnd as u32);
    }
    if style & WS_VISIBLE != 0 && wnd(hwnd).is_some() {
        show_window(hwnd, 5);
    }
    if crate::tracing() {
        trace(format_args!("W16 CreateWindow -> {hwnd:#x} (PM {frame:#x}/{client:#x})"));
    }
    hwnd
}

/// Position the client and the scroll bars of a plain window with a
/// non-client area in its `Win16Frame`.
pub fn layout_frame(h: u16) {
    let Some(w) = wnd(h) else { return };
    if w.framed || w.frame == w.client {
        return;
    }
    let (f, c, nc) = (w.frame, w.client, w.nc);
    let vs = w.vsb.as_ref().filter(|b| b.shown).map(|b| b.pm);
    let hs = w.hsb.as_ref().filter(|b| b.shown).map(|b| b.pm);
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(f, &mut r) };
    let bd = plain_border(nc);
    let (l, t, rr, b) = (bd, bd, bd, bd);
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    let (sw, sh) = (q(SV_CXVSCROLL), q(SV_CYHSCROLL));
    let mut inner = RectL { l: r.l + l, b: r.b + b, r: r.r - rr, t: r.t - t };
    // The scroll bars overlap the border, as Windows draws them.
    let ov = (l > 0) as i32;
    if let Some(v) = vs {
        let bottom = if hs.is_some() { inner.b + sh - ov } else { inner.b - ov };
        unsafe { WinSetWindowPos(v, HWND_TOP, inner.r - sw + ov, bottom, sw, inner.t + ov - bottom, SWP_MOVE | SWP_SIZE | SWP_SHOW) };
        inner.r -= sw - ov;
    }
    if let Some(hb) = hs {
        unsafe { WinSetWindowPos(hb, HWND_TOP, inner.l - ov, inner.b - ov, inner.r - inner.l + ov * 2 - ov * vs.is_some() as i32, sh, SWP_MOVE | SWP_SIZE | SWP_SHOW) };
        inner.b += sh - ov;
    }
    unsafe { WinSetWindowPos(c, HWND_TOP, inner.l, inner.b, (inner.r - inner.l).max(0), (inner.t - inner.b).max(0), SWP_MOVE | SWP_SIZE) };
}

/// The PM procedure of `Win16Frame`: the border of a plain window and
/// the corner between its scroll bars.
extern "C" fn frame_proc(pmh: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_SIZE => {
            let h = hwnd_of(pmh);
            if h != 0 {
                layout_frame(h);
            }
            0
        }
        WM_PAINT => {
            let h = hwnd_of(pmh);
            let hps = unsafe { WinBeginPaint(pmh, 0, core::ptr::null_mut()) };
            unsafe { GpiCreateLogColorTable(hps, 0, LCOLF_RGB, 0, 0, core::ptr::null()) };
            let mut r = RectL::default();
            unsafe { WinQueryWindowRect(pmh, &mut r) };
            let l = plain_border(wnd(h).map_or(0, |w| w.nc));
            let black = 0;
            let face = gdi::rgb(gdi::sys_color(gdi::COLOR_BTNFACE));
            unsafe {
                // The border: black, then the frame colour inside (thick
                // and dialog frames).
                if l > 0 {
                    WinFillRect(hps, &r, black);
                    let inner = RectL { l: r.l + 1, b: r.b + 1, r: r.r - 1, t: r.t - 1 };
                    if l > 1 {
                        WinFillRect(hps, &inner, face);
                    }
                }
                // The size box between the scroll bars.
                let both = wnd(h).is_some_and(|w| w.vsb.as_ref().is_some_and(|b| b.shown) && w.hsb.as_ref().is_some_and(|b| b.shown));
                if both {
                    let q = |v| WinQuerySysValue(HWND_DESKTOP, v);
                    let ov = (l > 0) as i32;
                    let sq = RectL { l: r.r - l - q(SV_CXVSCROLL) + ov, b: r.b + l, r: r.r - l, t: r.b + l + q(SV_CYHSCROLL) - ov };
                    WinFillRect(hps, &sq, face);
                }
                WinEndPaint(hps);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) },
    }
}

/// The client area's top-left corner in its parent's (or the screen's)
/// y-down coordinates, as an lParam.
fn client_origin(h: u16) -> u32 {
    let Some((client, child, parent)) = wnd(h).map(|w| (w.client, w.style & WS_CHILD != 0, w.parent)) else { return 0 };
    let (to, ph) = match wnd(parent).filter(|_| child) {
        Some(p) => (p.client, client_height(p.client)),
        None => (HWND_DESKTOP, screen_size().1),
    };
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(client, &mut r) };
    let mut pt = PointL { x: 0, y: r.t };
    unsafe { WinMapWindowPoints(client, to, &mut pt, 1) };
    ((ph - pt.y) as u32) << 16 | pt.x as u32 & 0xffff
}

/// A window's rectangle in y-down screen coordinates.
fn screen_rect(pm: u32) -> [i32; 4] {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(pm, &mut r) };
    let mut pts = [PointL { x: r.l, y: r.b }, PointL { x: r.r, y: r.t }];
    unsafe { WinMapWindowPoints(pm, HWND_DESKTOP, pts.as_mut_ptr(), 2) };
    let sy = screen_size().1;
    [pts[0].x, sy - pts[1].y, pts[1].x, sy - pts[0].y]
}

/// A window's rectangle in its parent's client coordinates (the screen's
/// for top-level windows).
pub fn window_rect(h: u16) -> [i32; 4] {
    let Some((f, child, parent)) = wnd(h).map(|w| (w.frame, w.style & WS_CHILD != 0, w.parent)) else { return [0; 4] };
    let r = screen_rect(f);
    if !child {
        return r;
    }
    let mut a = [r[0] as i16, r[1] as i16];
    let mut b = [r[2] as i16, r[3] as i16];
    map_point(parent, a.as_mut_ptr(), false);
    map_point(parent, b.as_mut_ptr(), false);
    [a[0] as i32, a[1] as i32, b[0] as i32, b[1] as i32]
}

fn map_point(h: u16, p: *mut i16, to_screen: bool) {
    if p.is_null() {
        return;
    }
    let Some(c) = wnd(h).map(|w| w.client) else { return };
    let (x, y) = unsafe { (p.read_unaligned() as i32, p.add(1).read_unaligned() as i32) };
    let ch = client_height(c);
    let sy = screen_size().1;
    let mut pt = if to_screen { PointL { x, y: ch - 1 - y } } else { PointL { x, y: sy - 1 - y } };
    let (from, to) = if to_screen { (c, HWND_DESKTOP) } else { (HWND_DESKTOP, c) };
    unsafe { WinMapWindowPoints(from, to, &mut pt, 1) };
    let ny = if to_screen { sy - 1 - pt.y } else { ch - 1 - pt.y };
    unsafe {
        p.write_unaligned(pt.x as i16);
        p.add(1).write_unaligned(ny as i16);
    }
}

/// A point in a window's client coordinates to the screen's.
pub fn to_screen(h: u16, x: i32, y: i32) -> (i32, i32) {
    let mut p = [x as i16, y as i16];
    map_point(h, p.as_mut_ptr(), true);
    (p[0] as i32, p[1] as i32)
}

pub fn show_window(h: u16, cmd: u16) -> bool {
    let Some((f, child, framed)) = wnd(h).map(|w| (w.frame, w.style & WS_CHILD != 0, w.framed)) else { return false };
    let was = unsafe { WinIsWindowVisible(f) } != 0;
    let mut fl = match cmd {
        0 => SWP_HIDE,
        2 | 6 | 7 => SWP_SHOW | SWP_MINIMIZE,
        3 => SWP_SHOW | SWP_MAXIMIZE | SWP_ACTIVATE | SWP_ZORDER,
        4 | 8 => SWP_SHOW,
        9 => SWP_SHOW | SWP_RESTORE | SWP_ACTIVATE | SWP_ZORDER,
        _ => SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER,
    };
    if child || !framed {
        fl &= !(SWP_ACTIVATE | SWP_MINIMIZE | SWP_MAXIMIZE | SWP_RESTORE);
        if !child && fl & SWP_SHOW != 0 {
            fl |= SWP_ZORDER;
        }
    }
    if cmd != 0 && !was {
        send(h, W_SHOWWINDOW, 1, 0);
    } else if cmd == 0 && was {
        send(h, W_SHOWWINDOW, 0, 0);
    }
    if let Some(w) = wnd(h) {
        if cmd == 0 {
            w.style &= !WS_VISIBLE;
        } else {
            w.style |= WS_VISIBLE;
        }
    }
    unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, fl) };
    if fl & SWP_ACTIVATE != 0 && !is_child(h, focus()) {
        if let Some(c) = wnd(h).map(|w| w.client) {
            unsafe { WinSetFocus(HWND_DESKTOP, c) };
        }
    }
    was
}

/// Move and size a window (Windows coordinates: the parent's client
/// area, or the screen).
fn move_window(h: u16, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> bool {
    let Some((f, parent)) = wnd(h).map(|w| (w.frame, w.parent)) else { return false };
    let child = is_child_style(h);
    let ph = if child { wnd(parent).map_or(0, |p| client_height(p.client)) } else { screen_size().1 };
    let (mut cx, mut cy) = (cx, cy);
    if fl & SWP_SIZE == 0 {
        let mut swp = Swp::default();
        unsafe { WinQueryWindowPos(f, &mut swp) };
        cx = swp.cx;
        cy = swp.cy;
    }
    let ok = unsafe { WinSetWindowPos(f, HWND_TOP, x, ph - y - cy, cx, cy, fl) != 0 };
    if fl & SWP_SIZE != 0 {
        layout_frame(h);
    }
    ok
}

pub fn move_to(h: u16, r: [i32; 4]) {
    move_window(h, r[0], r[1], r[2] - r[0], r[3] - r[1], SWP_MOVE | SWP_SIZE);
}

/// Every window below a window.
fn descendants(h: u16) -> Vec<u16> {
    let mut out = Vec::new();
    for c in children(h) {
        out.push(c);
        out.extend(descendants(c));
    }
    out
}

pub fn destroy_window(h: u16) -> bool {
    let Some(f) = wnd(h).map(|w| w.frame) else { return false };
    // Owned pop-ups go first.
    let owned: Vec<u16> = u().wnds.iter().filter(|w| w.parent == h && w.style & WS_CHILD == 0).map(|w| w.hwnd).collect();
    for o in owned {
        destroy_window(o);
    }
    if wnd(h).is_none() {
        return true;
    }
    crate::caret::window_gone(h);
    // PM sends WM_DESTROY to the client (pm_wndproc passes it on, and
    // the window leaves the table there).
    unsafe { WinDestroyWindow(f) };
    forget(h);
    true
}

/// Remove a window from the table (its data is freed later).
fn forget(h: u16) {
    if let Some(i) = u().wnds.iter().position(|w| w.hwnd == h) {
        let w = u().wnds.remove(i);
        u().timers.retain(|t| t.hwnd != h);
        u().gone.push(w);
    }
}

pub fn enable_window(h: u16, on: bool) -> bool {
    let Some(w) = wnd(h) else { return false };
    let was = w.style & WS_DISABLED != 0;
    if was == !on {
        return was;
    }
    if on { w.style &= !WS_DISABLED } else { w.style |= WS_DISABLED }
    let (f, c) = (w.frame, w.client);
    unsafe {
        WinEnableWindow(f, on as u32);
        if c != f {
            WinEnableWindow(c, on as u32);
        }
    }
    if !on && is_child(h, focus()) {
        unsafe { WinSetFocus(HWND_DESKTOP, 0) };
    }
    send(h, W_ENABLE, on as u16, 0);
    was
}

pub fn focus() -> u16 {
    hwnd_of(unsafe { WinQueryFocus(HWND_DESKTOP) })
}

/// SetFocus: the previous focus window.
pub fn set_focus(h: u16) -> u16 {
    let old = focus();
    if h != 0 && old != h {
        if let Some(c) = wnd(h).map(|w| w.client) {
            unsafe { WinSetFocus(HWND_DESKTOP, c) };
        }
    }
    old
}

fn set_capture(h: u16) -> u16 {
    let old = hwnd_above(unsafe { WinQueryCapture(HWND_DESKTOP) });
    if let Some(c) = wnd(h).map(|w| w.client) {
        unsafe { WinSetCapture(HWND_DESKTOP, c) };
    }
    old
}

/// Is `h` the window `p` or below it?
pub fn is_child(p: u16, h: u16) -> bool {
    let mut h = h;
    for _ in 0..64 {
        if h == 0 {
            return false;
        }
        let Some(w) = wnd(h) else { return false };
        if w.style & WS_CHILD == 0 {
            return false;
        }
        h = w.parent;
        if h == p {
            return true;
        }
    }
    false
}

fn find_window(c: u32, t: u32) -> u16 {
    let ci = if c != 0 { find_class(c) } else { None };
    if c != 0 && ci.is_none() {
        return 0;
    }
    let title = far_str(t);
    for i in 0..u().wnds.len() {
        let w = &u().wnds[i];
        if w.style & WS_CHILD != 0 || ci.is_some_and(|ci| ci != w.class) {
            continue;
        }
        let h = w.hwnd;
        if let Some(tt) = title {
            if !window_text(h).eq_ignore_ascii_case(tt) {
                continue;
            }
        }
        return h;
    }
    0
}

/// A window's text.
pub fn window_text(h: u16) -> Vec<u8> {
    let Some(w) = wnd(h) else { return Vec::new() };
    if w.framed {
        let mut b = [0u8; 256];
        let n = unsafe { WinQueryWindowText(w.frame, 256, b.as_mut_ptr()) };
        return b[..n.max(0) as usize].to_vec();
    }
    w.text.clone()
}

/// GetWindow(hwnd, GW_*).
fn get_window(h: u16, cmd: u16) -> u16 {
    let siblings = |h: u16| -> Vec<u16> {
        match wnd(h) {
            Some(w) if w.style & WS_CHILD != 0 => children(w.parent),
            _ => u().wnds.iter().filter(|w| w.style & WS_CHILD == 0).map(|w| w.hwnd).collect(),
        }
    };
    match cmd {
        // GW_HWNDFIRST, GW_HWNDLAST
        0 => siblings(h).first().copied().unwrap_or(0),
        1 => siblings(h).last().copied().unwrap_or(0),
        // GW_HWNDNEXT, GW_HWNDPREV
        2 | 3 => {
            let s = siblings(h);
            let Some(i) = s.iter().position(|&x| x == h) else { return 0 };
            if cmd == 2 { s.get(i + 1).copied().unwrap_or(0) } else if i > 0 { s[i - 1] } else { 0 }
        }
        // GW_OWNER
        4 => wnd(h).filter(|w| w.style & WS_CHILD == 0).map_or(0, |w| w.parent),
        // GW_CHILD
        5 => children(h).first().copied().unwrap_or(0),
        _ => 0,
    }
}

fn child_from_point(h: u16, x: i32, y: i32) -> u16 {
    let (cx, cy) = client_size(h);
    if x < 0 || y < 0 || x >= cx || y >= cy {
        return 0;
    }
    for c in children(h) {
        let r = window_rect(c);
        if x >= r[0] && x < r[2] && y >= r[1] && y < r[3] {
            return c;
        }
    }
    h
}

/// InvalidateRect: children too unless the window clips them.
pub fn invalidate(h: u16, r: Option<[i32; 4]>) {
    let Some((c, style)) = wnd(h).map(|w| (w.client, w.style)) else { return };
    let ch = client_height(c);
    let rc = r.map(|r| RectL { l: r[0], b: ch - r[3], r: r[2], t: ch - r[1] });
    let pr = rc.as_ref().map_or(core::ptr::null(), |r| r as *const RectL);
    unsafe { WinInvalidateRect(c, pr, (style & WS_CLIPCHILDREN == 0) as u32) };
}

pub fn validate(h: u16, r: Option<[i32; 4]>) {
    let Some(c) = wnd(h).map(|w| w.client) else { return };
    let ch = client_height(c);
    let rc = r.map(|r| RectL { l: r[0], b: ch - r[3], r: r[2], t: ch - r[1] });
    let pr = rc.as_ref().map_or(core::ptr::null(), |r| r as *const RectL);
    unsafe { WinValidateRect(c, pr, 0) };
}

/// ScrollWindow: the client area's bits move; what is uncovered is
/// invalidated (and the child windows move when the whole area does).
pub fn scroll_window(h: u16, dx: i32, dy: i32, r: Option<[i32; 4]>, clip: Option<[i32; 4]>) {
    let Some(c) = wnd(h).map(|w| w.client) else { return };
    let ch = client_height(c);
    let conv = |r: [i32; 4]| RectL { l: r[0], b: ch - r[3], r: r[2], t: ch - r[1] };
    let (rs, cs) = (r.map(conv), clip.map(conv));
    let fl = if r.is_none() { 1 | 2 } else { 2 };
    unsafe {
        WinScrollWindow(
            c,
            dx,
            -dy,
            rs.as_ref().map_or(core::ptr::null(), |r| r as *const RectL),
            cs.as_ref().map_or(core::ptr::null(), |r| r as *const RectL),
            0,
            core::ptr::null_mut(),
            fl,
        )
    };
}

pub fn get_dc(h: u16) -> u16 {
    let pm = if h == 0 { HWND_DESKTOP } else { wnd(h).map_or(0, |w| w.client) };
    if pm == 0 {
        return 0;
    }
    let hps = unsafe { WinGetPS(pm) };
    gdi::new_dc(hps, pm, client_height(pm), gdi::DcKind::Window)
}

fn set_cursor(c: u16) -> u16 {
    // The built-in cursors are PM's system pointers.
    let sptr = match c {
        32513 => 2,          // IDC_IBEAM: SPTR_TEXT
        32514 => 3,          // IDC_WAIT
        32640 | 32646 => 5,  // IDC_SIZE, IDC_SIZEALL: SPTR_MOVE
        32642 => 6,          // IDC_SIZENWSE
        32643 => 7,          // IDC_SIZENESW
        32644 => 8,          // IDC_SIZEWE
        32645 => 9,          // IDC_SIZENS
        0 => 0,
        _ => 1,              // IDC_ARROW and the rest
    };
    if sptr != 0 {
        unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, sptr, 0)) };
    } else {
        unsafe { WinSetPointer(HWND_DESKTOP, 0) };
    }
    32512
}

fn message_box(h: u16, text: u32, cap: u32, ty: u16) -> u32 {
    let t = z(far_str(text).unwrap_or(b""));
    let c = z(far_str(cap).unwrap_or(b"Error"));
    let buttons = match ty & 0x0f {
        1 => 1, // OKCANCEL
        2 => 3, // ABORTRETRYIGNORE
        3 => 5, // YESNOCANCEL
        4 => 4, // YESNO
        5 => 2, // RETRYCANCEL
        _ => 0,
    };
    let icon = match ty & 0xf0 {
        0x10 => 0x0040, // HAND
        0x20 => 0x0010, // QUESTION
        0x30 => 0x0020, // EXCLAMATION
        0x40 => 0x0030, // ASTERISK
        _ => 0,
    };
    let fl = buttons | icon | (ty as u32 & 0x0300) | if ty & 0x1000 != 0 { 0x1000 } else { 0 } | 0x4000;
    let owner = wnd(top_level(h)).map_or(0, |w| w.frame);
    let r = unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), 0, fl) };
    // MBID_* are IDOK..IDNO; MBID_ERROR is a failure.
    if r == 0xffff { 0 } else { r }
}

fn set_timer(h: u16, id: u16, ms: u16, proc_: u32) -> u16 {
    let pm = if h == 0 { 0 } else {
        match wnd(h) {
            Some(w) => w.client,
            None => return 0,
        }
    };
    let s = u();
    // A timer of no window gets an ID of its own.
    let id = if h == 0 { 0x100 + s.timers.len() as u16 } else { id };
    let pm_id = id as u32;
    if unsafe { WinStartTimer(TASK.get().hab, pm, pm_id, ms as u32) } == 0 {
        return 0;
    }
    s.timers.retain(|t| !(t.hwnd == h && t.id == id));
    s.timers.push(Timer { hwnd: h, id, pm_id, proc_ });
    if h == 0 { id } else { 1 }
}

pub fn post(h: u16, m: u16, wp: u16, lp: u32) -> bool {
    let pm = match wnd(h) {
        Some(w) => w.client,
        None if h == 0 || h == 0xffff => {
            return unsafe { WinPostQueueMsg(TASK.get().hmq, POSTED | m as u32, wp as u32, lp) } != 0;
        }
        None => return false,
    };
    unsafe { WinPostMsg(pm, POSTED | m as u32, wp as u32, lp) != 0 }
}

/// Windows key state bits for a mouse message's wParam.
fn mouse_keys(msg: u32) -> u16 {
    let down = |vk| unsafe { WinGetKeyState(HWND_DESKTOP, vk) } & 0x8000 != 0;
    let mut k = 0;
    // VK_BUTTON1..3, VK_SHIFT, VK_CTRL.
    if down(1) || msg == 0x71 || msg == 0x73 {
        k |= 0x0001;
    }
    if down(2) || msg == 0x74 || msg == 0x76 {
        k |= 0x0002;
    }
    if down(3) || msg == 0x77 || msg == 0x79 {
        k |= 0x0010;
    }
    if down(9) {
        k |= 0x0004;
    }
    if down(10) {
        k |= 0x0008;
    }
    if matches!(msg, 0x72 | 0x75 | 0x78) {
        k &= !match msg {
            0x72 => 0x0001,
            0x75 => 0x0002,
            _ => 0x0010,
        };
    }
    k
}

/// PM virtual keys -> Windows virtual keys.
const VKEYS: [(u16, u16); 26] = [
    (0x01, 0x01), // BUTTON1: LBUTTON
    (0x02, 0x02), // BUTTON2: RBUTTON
    (0x03, 0x04), // BUTTON3: MBUTTON
    (0x05, 0x08), // BACKSPACE
    (0x06, 0x09), // TAB
    (0x07, 0x09), // BACKTAB
    (0x08, 0x0d), // NEWLINE
    (0x1e, 0x0d), // ENTER
    (0x09, 0x10), // SHIFT
    (0x0a, 0x11), // CTRL
    (0x0b, 0x12), // ALT
    (0x0d, 0x13), // PAUSE
    (0x0e, 0x14), // CAPSLOCK
    (0x0f, 0x1b), // ESC
    (0x10, 0x20), // SPACE
    (0x11, 0x21), // PAGEUP
    (0x12, 0x22), // PAGEDOWN
    (0x13, 0x23), // END
    (0x14, 0x24), // HOME
    (0x15, 0x25), // LEFT
    (0x16, 0x26), // UP
    (0x17, 0x27), // RIGHT
    (0x18, 0x28), // DOWN
    (0x19, 0x2c), // PRINTSCRN
    (0x1a, 0x2d), // INSERT
    (0x1b, 0x2e), // DELETE
];

fn vkey(pm_vk: u16, ch: u16) -> u16 {
    if let Some(&(_, w)) = VKEYS.iter().find(|v| v.0 == pm_vk) {
        return w;
    }
    match pm_vk {
        0x1d => 0x90,  // NUMLOCK
        0x1c => 0x91,  // SCRLLOCK
        0x20..=0x2b => 0x70 + (pm_vk - 0x20), // F1-F12
        _ => {
            let c = (ch as u8).to_ascii_uppercase();
            if c.is_ascii_alphanumeric() { c as u16 } else { 0 }
        }
    }
}

/// Is a key (Windows virtual key) down?
pub fn key_down(vk: u16) -> bool {
    key_state(vk) & 0x8000 != 0
}

/// MessageBeep(0).
pub fn call_beep() {
    unsafe { WinAlarm(HWND_DESKTOP, 2) };
}

/// The letter or digit of a set-1 scan code.
fn scan_letter(scan: u8) -> u8 {
    const ROWS: [(u8, &[u8]); 4] = [(0x02, b"1234567890"), (0x10, b"qwertyuiop"), (0x1e, b"asdfghjkl"), (0x2c, b"zxcvbnm")];
    for (base, keys) in ROWS {
        if scan >= base && ((scan - base) as usize) < keys.len() {
            return keys[(scan - base) as usize];
        }
    }
    0
}

/// GetKeyState: bit 15 down, bit 0 toggled.
pub fn key_state(vk: u16) -> u16 {
    let pm = match vk {
        0x90 => 0x1d,
        0x91 => 0x1c,
        0x70..=0x7b => 0x20 + (vk - 0x70),
        _ => match VKEYS.iter().find(|v| v.1 == vk) {
            Some(&(p, _)) => p,
            None => return 0,
        },
    };
    let s = unsafe { WinGetKeyState(HWND_DESKTOP, pm as i32) };
    (if s & 0x8000 != 0 { 0x8000 } else { 0 }) | (s & 1) as u16
}

/// Translate a PM message for a Windows window into the Windows one:
/// (msg, wParam, lParam).
fn translate_pm(w_hwnd: u16, q: &QMsg) -> Option<(u16, u16, u32)> {
    let w = wnd(w_hwnd)?;
    let ch = client_height(w.client);
    let cls_style = u().classes.get(w.class).map_or(0, |c| c.style);
    let point = |mp: u32| {
        let x = mp as i16 as i32;
        let y = (mp >> 16) as i16 as i32;
        ((ch - 1 - y) as u32 & 0xffff) << 16 | x as u32 & 0xffff
    };
    Some(match q.msg {
        WM_PAINT => (W_PAINT, 0, 0),
        WM_TIMER => {
            let t = u().timers.iter().find(|t| t.pm_id == q.mp1 & 0xffff && t.hwnd == w_hwnd);
            (W_TIMER, t.map_or(q.mp1 as u16, |t| t.id), t.map_or(0, |t| t.proc_))
        }
        WM_MOUSEMOVE => (W_MOUSEMOVE, mouse_keys(q.msg), point(q.mp1)),
        0x71..=0x79 => {
            // WM_BUTTONnDOWN/UP/DBLCLK -> WM_xBUTTONDOWN/UP/DBLCLK.
            let i = q.msg - 0x71;
            let (button, kind) = (i / 3, i % 3);
            let base = [0x201u16, 0x204, 0x207][button as usize];
            let kind = if kind == 2 && cls_style & CS_DBLCLKS == 0 { 0 } else { kind };
            (base + kind as u16, mouse_keys(q.msg), point(q.mp1))
        }
        WM_CHAR => {
            let fs = q.mp1 as u16;
            let (repeat, scan) = ((q.mp1 >> 16) as u8 as u32, (q.mp1 >> 24) as u32);
            let (mut chr, pvk) = (q.mp2 as u16, (q.mp2 >> 16) as u16);
            let up = fs & 0x0040 != 0;
            let alt = fs & 0x0020 != 0;
            let ctrl = fs & 0x0010 != 0;
            let mut fs = fs;
            // Alt and Ctrl with a letter or digit carry no character in
            // PM: the scan code's.
            if (alt || ctrl) && fs & 0x0003 == 0 {
                let c = scan_letter(scan as u8);
                if c != 0 {
                    chr = c as u16;
                    fs |= 0x0001;
                }
            }
            let vk = if fs & 0x0002 != 0 { vkey(pvk, chr) } else { vkey(0, chr) };
            if vk == 0 && fs & 0x0001 == 0 {
                return None;
            }
            // F10 is a system key, as in Windows.
            let sys = alt || vk == 0x79;
            let lp = repeat.max(1) | scan << 16 | if alt { 1 << 29 } else { 0 } | if up { 0xc000_0000 } else { 0 };
            let m = match (up, sys) {
                (false, false) => W_KEYDOWN,
                (true, false) => W_KEYUP,
                (false, true) => W_SYSKEYDOWN,
                (true, true) => W_SYSKEYUP,
            };
            if !up && fs & 0x0001 != 0 && chr != 0 {
                // Ctrl+letter is a control character.
                let c = chr & 0xff;
                let c = if ctrl && !alt && (c as u8).is_ascii_alphabetic() { c & 0x1f } else { c };
                u().last_char = Some((w_hwnd, if alt { W_SYSCHAR } else { W_CHAR }, c, lp));
            } else if !up {
                u().last_char = None;
            }
            (m, if vk != 0 { vk } else { chr & 0xff }, lp)
        }
        m if m & POSTED != 0 => ((m & 0xffff) as u16, q.mp1 as u16, q.mp2),
        _ => return None,
    })
}

/// GetMessage / PeekMessage (flags given): the next message for the
/// task's Windows windows into MSG (hwnd, message, wParam, lParam,
/// time, pt); PM's other messages are dispatched on the way. (The
/// message range filter is not applied.)
pub fn get_message(p: *mut u8, h: u16, _lo: u16, _hi: u16, peek: Option<u16>) -> u32 {
    collect();
    let hab = TASK.get().hab;
    let filter = if h == 0 { 0 } else { wnd(h).map_or(0, |w| w.client) };
    loop {
        let mut q = QMsg::default();
        match peek {
            None => {
                unsafe { WinGetMsg(hab, &mut q, filter, 0, 0) };
            }
            Some(fl) => {
                if unsafe { WinPeekMsg(hab, &mut q, filter, 0, 0, PM_NOREMOVE) } == 0 {
                    return 0;
                }
                let ours = q.msg == WM_QUIT || win_message(&q).is_some();
                if !ours || fl & 1 != 0 {
                    unsafe { WinPeekMsg(hab, &mut q, filter, 0, 0, PM_REMOVE) };
                }
            }
        }
        if q.msg == WM_QUIT {
            write_msg(p, 0, W_QUIT, q.mp1 as u16, 0, q.time, (q.x, q.y));
            return peek.is_some() as u32;
        }
        let Some((hwnd, msg, wp, lp)) = win_message(&q) else {
            unsafe { WinDispatchMsg(hab, &q) };
            if peek.is_some() {
                // Only Windows messages count for PeekMessage.
                continue;
            }
            continue;
        };
        // WM_SETCURSOR before the mouse messages, as Windows' input
        // processing sends it.
        if msg == W_MOUSEMOVE || (0x201..=0x209).contains(&msg) {
            send(hwnd, W_SETCURSOR, hwnd, (msg as u32) << 16 | 1);
        }
        if crate::tracing() && (0x100..=0x109).contains(&msg) {
            trace(format_args!("W16 key {hwnd:#x} {msg:#x} {wp:#x} {lp:#x} (PM {:#x} {:#x})", q.mp1, q.mp2));
        }
        write_msg(p, hwnd, msg, wp, lp, q.time, (q.x, q.y));
        let sy = screen_size().1;
        u().msg_time = q.time;
        u().msg_pos = ((sy - 1 - q.y) as u32 & 0xffff) << 16 | q.x as u32 & 0xffff;
        u().last = Some((hwnd, msg, q));
        return 1;
    }
}

/// WaitMessage: until a message is in the queue.
fn wait_message() {
    let hab = TASK.get().hab;
    let mut q = QMsg::default();
    while unsafe { WinPeekMsg(hab, &mut q, 0, 0, 0, PM_NOREMOVE) } == 0 {
        unsafe { os2::DosSleep(10) };
    }
}

/// The Windows message (hwnd, msg, wParam, lParam) of a PM queue
/// message, if it is one for the task.
fn win_message(q: &QMsg) -> Option<(u16, u16, u16, u32)> {
    let h = by_client(q.hwnd).map_or(0, |w| w.hwnd);
    if h != 0 {
        return translate_pm(h, q).map(|(m, wp, lp)| (h, m, wp, lp));
    }
    if q.msg & POSTED != 0 && (q.hwnd == 0 || hwnd_of(q.hwnd) == 0) {
        return Some((0, q.msg as u16, q.mp1 as u16, q.mp2));
    }
    if q.msg == WM_TIMER && q.hwnd == 0 {
        // A timer of no window: the id and the procedure.
        let t = u().timers.iter().find(|t| t.hwnd == 0 && t.pm_id == q.mp1 & 0xffff)?;
        return Some((0, W_TIMER, t.id, t.proc_));
    }
    None
}

pub fn write_msg(p: *mut u8, hwnd: u16, msg: u16, wp: u16, lp: u32, time: u32, pt: (i32, i32)) {
    if p.is_null() {
        return;
    }
    let sy = screen_size().1;
    let mut b = [0u8; 18];
    b[0..2].copy_from_slice(&hwnd.to_le_bytes());
    b[2..4].copy_from_slice(&msg.to_le_bytes());
    b[4..6].copy_from_slice(&wp.to_le_bytes());
    b[6..10].copy_from_slice(&lp.to_le_bytes());
    b[10..14].copy_from_slice(&time.to_le_bytes());
    b[14..16].copy_from_slice(&(pt.0 as i16).to_le_bytes());
    b[16..18].copy_from_slice(&((sy - 1 - pt.1) as i16).to_le_bytes());
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 18) };
}

pub fn read_msg(p: *const u8) -> Option<(u16, u16, u16, u32)> {
    if p.is_null() {
        return None;
    }
    let w = |o: usize| unsafe { (p.add(o) as *const u16).read_unaligned() };
    let l = unsafe { (p.add(6) as *const u32).read_unaligned() };
    Some((w(0), w(2), w(4), l))
}

pub fn translate(p: *const u16) -> bool {
    let Some((h, m, _, _)) = read_msg(p as *const u8) else { return false };
    if m != W_KEYDOWN && m != W_SYSKEYDOWN {
        return false;
    }
    let Some((ch, cm, c, lp)) = u().last_char.take() else { return false };
    if ch != h {
        return false;
    }
    let pm = wnd(h).map_or(0, |w| w.client);
    unsafe { WinPostMsg(pm, POSTED | cm as u32, c as u32, lp) != 0 }
}

pub fn dispatch(p: *mut u8) -> u32 {
    let Some((h, m, wp, lp)) = read_msg(p) else { return 0 };
    // A timer's procedure gets WM_TIMER itself.
    if m == W_TIMER && lp != 0 {
        let t = unsafe { WinGetCurrentTime(TASK.get().hab) };
        return PascalCall::new().w(h).w(m).w(wp).l(t).call(lp);
    }
    if wnd(h).is_none() {
        return 0;
    }
    let last = u().last.filter(|&(lh, lm, _)| lh == h && lm == m);
    match last {
        Some((_, _, q)) => send_inflight(h, m, wp, lp, q),
        None => send(h, m, wp, lp),
    }
}

/// PM's window procedure of the Windows windows' client areas.
extern "C" fn pm_wndproc(pmh: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(h) = by_client(pmh).map(|w| w.hwnd) else {
        return unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) };
    };
    let q = QMsg { hwnd: pmh, msg, mp1, mp2, ..Default::default() };
    match msg {
        WM_SIZE => {
            let (cx, cy) = (mp2 as u16, (mp2 >> 16) as u16);
            let cls = wnd(h).and_then(|w| u().classes.get(w.class)).map_or(0, |c| c.style);
            if cls & (CS_HREDRAW | CS_VREDRAW) != 0 {
                unsafe { WinInvalidateRect(pmh, core::ptr::null(), 0) };
            }
            let f = wnd(h).map_or(0, |w| w.frame);
            let mut swp = Swp::default();
            unsafe { WinQueryWindowPos(f, &mut swp) };
            let kind = if swp.fl & SWP_MINIMIZE != 0 { 1 } else if swp.fl & SWP_MAXIMIZE != 0 { 2 } else { 0 };
            send_inflight(h, W_SIZE, kind, (cy as u32) << 16 | cx as u32, q);
            0
        }
        WM_MOVE => {
            send_inflight(h, W_MOVE, 0, client_origin(h), q);
            0
        }
        WM_ACTIVATE => {
            send_inflight(h, W_ACTIVATE, (mp1 & 0xffff != 0) as u16, 0, q);
            0
        }
        WM_SETFOCUS => {
            let other = hwnd_of(mp1);
            send_inflight(h, if mp2 & 0xffff != 0 { W_SETFOCUS } else { W_KILLFOCUS }, other, 0, q);
            0
        }
        WM_CLOSE => {
            // Windows' default closes the window (DestroyWindow); PM's
            // would post WM_QUIT.
            send_inflight(h, W_CLOSE, 0, 0, q);
            0
        }
        WM_DESTROY => {
            send_inflight(h, W_DESTROY, 0, 0, q);
            send(h, W_NCDESTROY, 0, 0);
            forget(h);
            unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) }
        }
        WM_ENABLE => 0,
        // BeginPaint erases (WM_ERASEBKGND).
        WM_ERASEBACKGROUND => 0,
        WM_COMMAND | WM_INITMENU | WM_MENUSELECT | WM_MENUEND => crate::menu::pm_message(h, msg, mp1, mp2),
        WM_VSCROLL | WM_HSCROLL | WM_CONTROL => 0,
        _ => match translate_pm(h, &q) {
            Some((m, wp, lp)) => {
                let r = send_inflight(h, m, wp, lp, q);
                // Mouse and keys: processed.
                if (0x70..=0x7a).contains(&msg) { 1 } else { r }
            }
            None => unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) },
        },
    }
}

/// DefWindowProc: PM's default for a translated PM message, Windows'
/// for the rest.
pub fn def_window_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    let inflight = u().inflight.iter().rev().find(|f| f.hwnd == h && f.msg == m).copied();
    match m {
        W_NCCREATE => {
            // The window text, from CREATESTRUCT's lpszName.
            let name = unsafe { (flat(lp).add(22) as *const u32).read_unaligned() };
            if let (Some(w), Some(t)) = (wnd(h), far_str(name)) {
                if !t.starts_with(&[0xff]) {
                    w.text = t.to_vec();
                }
            }
            return 1;
        }
        W_CLOSE => {
            destroy_window(h);
            return 0;
        }
        W_PAINT => {
            // BeginPaint (erasing) and EndPaint.
            let hdc = begin_paint(h, core::ptr::null_mut());
            gdi::release_dc(hdc);
            return 0;
        }
        W_ERASEBKGND => {
            let Some(cls) = wnd(h).map(|w| w.class) else { return 0 };
            let br = u().classes.get(cls).map_or(0, |c| c.brush);
            if br == 0 {
                return 0;
            }
            let (cx, cy) = client_size(h);
            gdi::fill_rect(wp, [0, 0, cx, cy], br);
            return 1;
        }
        W_CTLCOLOR => {
            // CTLCOLOR_SCROLLBAR: the scroll bar colour; the rest the
            // window's colours.
            let kind = (lp >> 16) as u16;
            gdi::set_text_color(wp, gdi::sys_color(gdi::COLOR_WINDOWTEXT));
            gdi::set_bk_color(wp, gdi::sys_color(gdi::COLOR_WINDOW));
            return if kind == 5 { gdi::sys_brush(gdi::COLOR_SCROLLBAR) } else { gdi::sys_brush(gdi::COLOR_WINDOW) } as u32;
        }
        W_SETCURSOR => {
            if lp & 0xffff == 1 {
                let cur = wnd(h).and_then(|w| u().classes.get(w.class)).map_or(32512, |c| c.cursor);
                if cur != 0 {
                    set_cursor(cur);
                    return 1;
                }
            }
            return 0;
        }
        W_MOUSEACTIVATE => return 1,
        W_QUERYENDSESSION => return 1,
        W_SETTEXT => {
            let t = far_str(lp).unwrap_or(b"").to_vec();
            let Some(w) = wnd(h) else { return 0 };
            if w.framed {
                let s = z(&t);
                unsafe { WinSetWindowText(w.frame, s.as_ptr()) };
            }
            w.text = t;
            return 1;
        }
        W_GETTEXT => {
            let buf = flat(lp);
            if buf.is_null() || wp == 0 {
                return 0;
            }
            let t = window_text(h);
            return copy_out(&t, buf, wp as i32) as u32;
        }
        W_GETTEXTLENGTH => return window_text(h).len() as u32,
        W_SYSCOMMAND => {
            let f = wnd(top_level(h)).map_or(0, |w| w.frame);
            match wp & 0xfff0 {
                0xf060 => {
                    send(h, W_CLOSE, 0, 0);
                }
                0xf020 => unsafe {
                    WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_MINIMIZE);
                },
                0xf030 => unsafe {
                    WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_MAXIMIZE);
                },
                0xf120 => unsafe {
                    WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_RESTORE);
                },
                _ => {}
            }
            return 0;
        }
        // The system keys go to the top-level frame (Alt+letter and F10
        // open its menu, Alt+F4 closes it...).
        W_SYSKEYDOWN | W_SYSKEYUP => {
            if let Some(f) = inflight.filter(|f| f.q.msg == WM_CHAR) {
                let tf = wnd(top_level(h)).filter(|w| w.framed).map_or(0, |w| w.frame);
                if tf != 0 {
                    return unsafe { WinSendMsg(tf, WM_CHAR, f.q.mp1, f.q.mp2) };
                }
            }
            return 0;
        }
        W_KEYDOWN | W_KEYUP | W_CHAR | W_SYSCHAR => {
            // A top-level window's keys go on to its frame (PM's
            // default); a child window's go nowhere.
            if !wnd(h).is_some_and(|w| w.framed) {
                return 0;
            }
        }
        W_GETFONT => return 0,
        _ => {}
    }
    match inflight {
        Some(f) => unsafe { WinDefWindowProc(f.q.hwnd, f.q.msg, f.q.mp1, f.q.mp2) },
        None => 0,
    }
}

/// Copy text to a buffer of `max` bytes (NUL-terminated): the length.
pub fn copy_out(t: &[u8], buf: *mut u8, max: i32) -> usize {
    if buf.is_null() || max <= 0 {
        return 0;
    }
    let n = t.len().min(max as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n
}

fn begin_paint(h: u16, ps: *mut u8) -> u16 {
    let Some(c) = wnd(h).map(|w| w.client) else { return 0 };
    let mut rc = RectL::default();
    let hps = unsafe { WinBeginPaint(c, 0, &mut rc) };
    if hps == 0 {
        return 0;
    }
    let ch = client_height(c);
    let hdc = gdi::new_dc(hps, c, ch, gdi::DcKind::Paint);
    crate::caret::hide_for_paint(h);
    // WM_ERASEBKGND (the class brush, by default).
    let erased = send(h, W_ERASEBKGND, hdc, 0) != 0;
    if !ps.is_null() {
        let mut b = [0u8; 32];
        b[0..2].copy_from_slice(&hdc.to_le_bytes());
        b[2..4].copy_from_slice(&(!erased as u16).to_le_bytes());
        for (i, v) in [rc.l, ch - rc.t, rc.r, ch - rc.b].iter().enumerate() {
            b[4 + i * 2..6 + i * 2].copy_from_slice(&(*v as i16).to_le_bytes());
        }
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), ps, 32) };
    }
    hdc
}

/// BeginPaint for the built-in controls: the DC (erased by the control
/// itself) and the update rectangle.
pub fn begin_paint_rs(h: u16) -> u16 {
    let Some(c) = wnd(h).map(|w| w.client) else { return 0 };
    let mut rc = RectL::default();
    let hps = unsafe { WinBeginPaint(c, 0, &mut rc) };
    if hps == 0 {
        return 0;
    }
    crate::caret::hide_for_paint(h);
    gdi::new_dc(hps, c, client_height(c), gdi::DcKind::Paint)
}

/// Get/SetWindowWord/Long: GWL_WNDPROC (-4), GWW_HINSTANCE (-6),
/// GWW_HWNDPARENT (-8), GWW_ID (-12), GWL_STYLE (-16), GWL_EXSTYLE
/// (-20), or the window's extra bytes. Setting returns the old value.
fn window_value(h: u16, i: i32, set: Option<u32>, long: bool) -> u32 {
    let Some(w) = wnd(h) else { return 0 };
    let field: Option<&mut u32> = match i {
        -4 => Some(&mut w.proc_),
        -16 => Some(&mut w.style),
        -20 => Some(&mut w.exstyle),
        _ => None,
    };
    if let Some(f) = field {
        let old = *f;
        if let Some(v) = set {
            *f = v;
        }
        return old;
    }
    match i {
        -6 => return w.hinst as u32,
        -8 => return w.parent as u32,
        -12 => {
            let old = w.id as u32;
            if let Some(v) = set {
                w.id = v as u16;
            }
            return old;
        }
        _ => {}
    }
    let n = if long { 4 } else { 2 };
    let i = i as usize;
    if i + n > w.extra.len() {
        return 0;
    }
    let mut old = [0u8; 4];
    old[..n].copy_from_slice(&w.extra[i..i + n]);
    if let Some(v) = set {
        w.extra[i..i + n].copy_from_slice(&v.to_le_bytes()[..n]);
    }
    u32::from_le_bytes(old)
}

/// Get/SetClassWord/Long: GCW_HBRBACKGROUND (-10), GCW_HCURSOR (-12),
/// GCW_HICON (-14), GCW_HMODULE (-16), GCW_CBWNDEXTRA (-18),
/// GCW_CBCLSEXTRA (-20), GCL_WNDPROC (-24), GCW_STYLE (-26), or the
/// class's extra bytes.
fn class_value(h: u16, i: i32, set: Option<u32>, long: bool) -> u32 {
    let Some(ci) = wnd(h).map(|w| w.class) else { return 0 };
    let c = &mut u().classes[ci];
    let w16: Option<&mut u16> = match i {
        -10 => Some(&mut c.brush),
        -12 => Some(&mut c.cursor),
        -14 => Some(&mut c.icon),
        -16 => Some(&mut c.hinst),
        -18 => Some(&mut c.wnd_extra),
        -26 => Some(&mut c.style),
        _ => None,
    };
    if let Some(f) = w16 {
        let old = *f;
        if let Some(v) = set {
            *f = v as u16;
        }
        return old as u32;
    }
    match i {
        -20 => return c.cls_extra.len() as u32,
        -24 => {
            let old = c.proc_;
            if let Some(v) = set {
                c.proc_ = v;
            }
            return old;
        }
        _ => {}
    }
    let n = if long { 4 } else { 2 };
    let i = i as usize;
    if i + n > c.cls_extra.len() {
        return 0;
    }
    let mut old = [0u8; 4];
    old[..n].copy_from_slice(&c.cls_extra[i..i + n]);
    if let Some(v) = set {
        c.cls_extra[i..i + n].copy_from_slice(&v.to_le_bytes()[..n]);
    }
    u32::from_le_bytes(old)
}

fn system_metric(i: i32) -> i32 {
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    match i {
        0 => q(SV_CXSCREEN),
        1 => q(SV_CYSCREEN),
        2 => q(SV_CXVSCROLL),
        3 => q(SV_CYHSCROLL),
        4 => q(SV_CYTITLEBAR),
        5 => q(SV_CXBORDER),
        6 => q(SV_CYBORDER),
        7 | 8 => q(SV_CXDLGFRAME),
        9 => q(SV_CYVSCROLLARROW),
        10 => q(SV_CXHSCROLLARROW),
        11 | 12 => 32,
        13 => q(SV_CXPOINTER),
        14 => q(SV_CYPOINTER),
        15 => q(SV_CYMENU),
        16 => q(SV_CXSCREEN),
        17 => q(SV_CYSCREEN) - q(SV_CYTITLEBAR),
        19 => 1,  // SM_MOUSEPRESENT
        20 => q(SV_CYVSCROLLARROW),
        21 => q(SV_CXHSCROLLARROW),
        28 => 100, // SM_CXMIN
        29 => 26,
        30 | 31 => q(SV_CYTITLEBAR),
        32 => q(SV_CXSIZEBORDER),
        33 => q(SV_CYSIZEBORDER),
        34 | 35 => 32,
        36 | 37 => 4,  // SM_CXDOUBLECLK
        38 | 39 => 64, // SM_CXICONSPACING
        40 => 0,       // SM_MENUDROPALIGNMENT
        43 => 3, // SM_CMOUSEBUTTONS
        _ => 0,
    }
}
