//! USER: window classes, windows, messages and painting, on PM.
//!
//! A top-level Windows window is a PM frame window (title bar, system
//! menu, sizing border and buttons from the window style) with a client
//! window of the class `Win16Wnd`; a child window is a `Win16Wnd` window
//! in its parent's client. The Windows HWND is a handle of this table;
//! messages for it arrive on the PM client window.
//!
//! Messages PM sends to the client come to `pm_wndproc`, which
//! translates them (Windows coordinates have y growing down from the
//! client's top-left corner) and calls the window's 16-bit window
//! procedure. Messages PM posts (input, WM_PAINT, WM_TIMER) come out of
//! GetMessage translated the same way and go to the procedure through
//! DispatchMessage. While a translated message is being handled,
//! DefWindowProc for it passes the PM message on to WinDefWindowProc;
//! other messages get Windows' default handling here.

use alloc::vec::Vec;
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
pub const W_SETTEXT: u16 = 0x000c;
pub const W_GETTEXT: u16 = 0x000d;
pub const W_GETTEXTLENGTH: u16 = 0x000e;
pub const W_PAINT: u16 = 0x000f;
pub const W_CLOSE: u16 = 0x0010;
pub const W_QUERYENDSESSION: u16 = 0x0011;
pub const W_QUIT: u16 = 0x0012;
pub const W_ERASEBKGND: u16 = 0x0014;
pub const W_SHOWWINDOW: u16 = 0x0018;
pub const W_SETCURSOR: u16 = 0x0020;
pub const W_MOUSEACTIVATE: u16 = 0x0021;
pub const W_NCCREATE: u16 = 0x0081;
pub const W_NCDESTROY: u16 = 0x0082;
pub const W_KEYDOWN: u16 = 0x0100;
pub const W_KEYUP: u16 = 0x0101;
pub const W_CHAR: u16 = 0x0102;
pub const W_SYSKEYDOWN: u16 = 0x0104;
pub const W_SYSKEYUP: u16 = 0x0105;
pub const W_SYSCHAR: u16 = 0x0106;
pub const W_SYSCOMMAND: u16 = 0x0112;
pub const W_TIMER: u16 = 0x0113;
pub const W_MOUSEMOVE: u16 = 0x0200;

// Window styles.
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_DISABLED: u32 = 0x0800_0000;
const WS_BORDER: u32 = 0x0080_0000;
const WS_DLGFRAME: u32 = 0x0040_0000;
const WS_CAPTION: u32 = 0x00c0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_THICKFRAME: u32 = 0x0004_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_MAXIMIZEBOX: u32 = 0x0001_0000;

const CS_VREDRAW: u16 = 0x0001;
const CS_HREDRAW: u16 = 0x0002;
const CS_DBLCLKS: u16 = 0x0008;

const CW_USEDEFAULT: i32 = -0x8000;

/// PM message number of a Windows message posted with PostMessage.
const POSTED: u32 = 0x0001_0000;

#[allow(dead_code)]
pub struct Class {
    pub name: Vec<u8>,
    pub atom: u16,
    pub style: u16,
    pub proc_: u32,
    pub wnd_extra: u16,
    pub hinst: u16,
    pub icon: u16,
    pub cursor: u16,
    pub brush: u16,
}

pub struct Wnd {
    pub hwnd: u16,
    /// The PM frame (top-level windows) or the window itself (children).
    pub frame: u32,
    /// The PM window of the client area.
    pub client: u32,
    pub class: usize,
    pub proc_: u32,
    pub style: u32,
    pub exstyle: u32,
    pub parent: u16,
    pub id: u16,
    pub hinst: u16,
    pub extra: Vec<u8>,
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
    next_hwnd: u16,
    next_atom: u16,
    inflight: Vec<InFlight>,
    /// The PM message behind the last message GetMessage returned.
    last: Option<(u16, u16, QMsg)>,
    /// The character of the last WM_KEYDOWN (TranslateMessage posts it).
    last_char: Option<(u16, u16, u16, u32)>,
    timers: Vec<Timer>,
    cascade: i32,
}

static USER: Global<User> = Global::new(User {
    classes: Vec::new(),
    wnds: Vec::new(),
    next_hwnd: 0x2004,
    next_atom: 0xc000,
    inflight: Vec::new(),
    last: None,
    last_char: None,
    timers: Vec::new(),
    cascade: 0,
});

fn u() -> &'static mut User {
    USER.get()
}

pub fn wnd(h: u16) -> Option<&'static mut Wnd> {
    u().wnds.iter_mut().find(|w| w.hwnd == h)
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

/// The client height of a window (for y-down coordinates).
pub fn client_height(pm: u32) -> i32 {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(pm, &mut r) };
    r.t - r.b
}

fn screen_size() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) }
}

/// A NUL-terminated copy of a string argument.
fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// Call a 16-bit window procedure.
pub fn call_proc(proc_: u32, hwnd: u16, msg: u16, wp: u16, lp: u32) -> u32 {
    if proc_ == 0 {
        return 0;
    }
    PascalCall::new().w(hwnd).w(msg).w(wp).l(lp).call(proc_)
}

/// Send a message to a window: its procedure, now.
pub fn send(hwnd: u16, msg: u16, wp: u16, lp: u32) -> u32 {
    let Some(p) = wnd(hwnd).map(|w| w.proc_) else { return 0 };
    call_proc(p, hwnd, msg, wp, lp)
}

/// Send a translated PM message: DefWindowProc for it goes to PM.
fn send_inflight(hwnd: u16, msg: u16, wp: u16, lp: u32, q: QMsg) -> u32 {
    u().inflight.push(InFlight { hwnd, msg, q });
    let r = send(hwnd, msg, wp, lp);
    u().inflight.pop();
    r
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
        // GetTickCount
        13 => unsafe { WinGetCurrentTime(TASK.get().hab) },
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
        // SetFocus(hwnd): the previous focus window.
        22 => {
            let h = a.w();
            let old = hwnd_of(unsafe { WinQueryFocus(HWND_DESKTOP) });
            if let Some(c) = wnd(h).map(|w| w.client) {
                unsafe { WinSetFocus(HWND_DESKTOP, c) };
            }
            old as u32
        }
        // GetFocus
        23 => hwnd_of(unsafe { WinQueryFocus(HWND_DESKTOP) }) as u32,
        // ClientToScreen(hwnd, lpPoint), ScreenToClient(hwnd, lpPoint)
        28 | 29 => {
            let (h, p) = (a.w(), a.p() as *mut i16);
            map_point(h, p, ord == 28);
            0
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
            if let Some(c) = wnd(h).map(|w| w.client) {
                let mut rc = RectL::default();
                unsafe { WinQueryWindowRect(c, &mut rc) };
                write_rect(r, [0, 0, rc.r - rc.l, rc.t - rc.b]);
            }
            0
        }
        // EnableWindow(hwnd, enable): was it disabled?
        34 => {
            let (h, on) = (a.w(), a.w());
            let Some(w) = wnd(h) else { return 0 };
            let was = w.style & WS_DISABLED != 0;
            if on != 0 { w.style &= !WS_DISABLED } else { w.style |= WS_DISABLED }
            let f = w.frame;
            unsafe { WinEnableWindow(f, on as u32) };
            send(h, W_ENABLE, on, 0);
            was as u32
        }
        // GetWindowText(hwnd, buf, max)
        36 => {
            let (h, buf, max) = (a.w(), a.p(), a.i());
            let Some(f) = wnd(h).map(|w| w.frame) else { return 0 };
            if buf.is_null() || max <= 0 {
                return 0;
            }
            unsafe { WinQueryWindowText(f, max, buf) as u32 }
        }
        // SetWindowText(hwnd, text)
        37 => {
            let (h, t) = (a.w(), a.l());
            let Some(f) = wnd(h).map(|w| w.frame) else { return 0 };
            let s = z(far_str(t).unwrap_or(b""));
            unsafe { WinSetWindowText(f, s.as_ptr()) };
            0
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
        // GetParent(hwnd)
        46 => {
            let h = a.w();
            wnd(h).map_or(0, |w| w.parent as u32)
        }
        // IsWindow(hwnd)
        47 => wnd(a.w()).is_some() as u32,
        // IsWindowVisible(hwnd)
        49 => {
            let h = a.w();
            wnd(h).is_some_and(|w| unsafe { WinIsWindowVisible(w.frame) } != 0) as u32
        }
        // DestroyWindow(hwnd)
        53 => destroy_window(a.w()) as u32,
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
        // GetActiveWindow
        60 => hwnd_of(unsafe { WinQueryActiveWindow(HWND_DESKTOP) }) as u32,
        // GetDC(hwnd)
        66 => {
            let h = a.w();
            let pm = if h == 0 { HWND_DESKTOP } else { wnd(h).map_or(0, |w| w.client) };
            if pm == 0 {
                return 0;
            }
            let hps = unsafe { WinGetPS(pm) };
            gdi::new_dc(hps, pm, client_height(pm), gdi::DcKind::Window) as u32
        }
        // ReleaseDC(hwnd, hdc)
        68 => {
            let (_h, hdc) = (a.w(), a.w());
            gdi::release_dc(hdc) as u32
        }
        // SetCursor(hcursor): the previous one.
        69 => set_cursor(a.w()) as u32,
        // FillRect(hdc, lpRect, hbrush)
        81 => {
            let (hdc, r, br) = (a.w(), a.l(), a.w());
            if let Some(r) = read_rect(r) {
                gdi::fill_rect(hdc, r, br);
            }
            0
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
            let Some(c) = wnd(h).map(|w| w.client) else { return 0 };
            let ch = client_height(c);
            let rc = read_rect(r).map(|r| RectL { l: r[0], b: ch - r[3], r: r[2], t: ch - r[1] });
            let pr = rc.as_ref().map_or(core::ptr::null(), |r| r as *const RectL);
            unsafe {
                if ord == 125 { WinInvalidateRect(c, pr, 0) } else { WinValidateRect(c, pr, 0) };
            }
            0
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
        // ones (IDC_*, IDI_*); the program's own are stage 2.
        173 | 174 => {
            let (hi, name) = (a.w(), a.l());
            if name >> 16 == 0 {
                let id = name as u16;
                if hi == 0 || (32512..=32651).contains(&id) {
                    return id as u32;
                }
            }
            trace(format_args!("WIN16: {} from the program's resources not supported", if ord == 173 { "LoadCursor" } else { "LoadIcon" }));
            // IDC_ARROW, IDI_APPLICATION
            32512
        }
        // GetSystemMetrics(index)
        179 => system_metric(a.i()) as u32,
        // GetSysColor(index)
        180 => gdi::sys_color(a.w()),
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
            if fl & 0x14 == 0 {
                f |= SWP_ACTIVATE | SWP_ZORDER;
            }
            move_window(h, x, y, cx, cy, f) as u32
        }
        _ => 0,
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
    unsafe { WinRegisterClass(t.hab, b"Win16Wnd\0".as_ptr(), pm_wndproc, 0, 4) };
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
    if s.classes.iter().any(|c| c.name == name) {
        return 0;
    }
    let atom = s.next_atom;
    s.next_atom += 1;
    s.classes.push(Class {
        name,
        atom,
        style: w(0),
        proc_: l(2),
        wnd_extra: w(8),
        hinst: w(10),
        icon: w(12),
        cursor: w(14),
        brush: w(16),
    });
    atom
}

fn find_class(name: u32) -> Option<usize> {
    let s = u();
    match far_str(name) {
        Some(n) => s.classes.iter().position(|c| c.name.eq_ignore_ascii_case(n)),
        None => s.classes.iter().position(|c| c.atom == name as u16),
    }
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

#[allow(clippy::too_many_arguments)]
fn create_window(exstyle: u32, class: u32, title: u32, style: u32, pos: [i32; 4], parent: u16, menu: u16, hinst: u16, param: u32) -> u16 {
    let Some(ci) = find_class(class) else {
        trace(format_args!(
            "WIN16: CreateWindow: class {} not found (the built-in controls are stage 2)",
            far_str(class).map(|n| n.iter().map(|&c| c as char).collect::<alloc::string::String>()).unwrap_or_default()
        ));
        return 0;
    };
    let (cstyle, cproc, cextra) = {
        let c = &u().classes[ci];
        (c.style, c.proc_, c.wnd_extra)
    };
    let text = z(far_str(title).unwrap_or(b""));
    let [mut x, mut y, mut cx, mut cy] = pos;
    let child = style & WS_CHILD != 0;
    let (frame, client) = if child {
        let Some(pc) = wnd(parent).map(|w| w.client) else { return 0 };
        let ph = client_height(pc);
        let (x0, y0) = (if x == CW_USEDEFAULT { 0 } else { x }, if y == CW_USEDEFAULT { 0 } else { y });
        let h = unsafe {
            WinCreateWindow(pc, b"Win16Wnd\0".as_ptr(), text.as_ptr(), WS_CLIPSIBLINGS, x0, ph - y0 - cy, cx, cy, pc, HWND_TOP, menu as u32, core::ptr::null(), core::ptr::null())
        };
        (h, h)
    } else {
        let mut fcf = frame_flags(style, parent != 0);
        let mut client = 0;
        let f = unsafe {
            WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"Win16Wnd\0".as_ptr(), text.as_ptr(), 0, 0, 0, &mut client)
        };
        let (sx, sy) = screen_size();
        let s = u();
        if x == CW_USEDEFAULT {
            let k = s.cascade % 8;
            s.cascade += 1;
            x = 24 + 24 * k;
            y = 24 + 24 * k;
        }
        if cx == CW_USEDEFAULT {
            cx = sx * 3 / 4;
            cy = sy * 3 / 4;
        }
        unsafe { WinSetWindowPos(f, HWND_TOP, x, sy - y - cy, cx, cy, SWP_MOVE | SWP_SIZE) };
        (f, client)
    };
    if frame == 0 || client == 0 {
        return 0;
    }
    let s = u();
    let hwnd = s.next_hwnd;
    s.next_hwnd = s.next_hwnd.wrapping_add(4);
    s.wnds.push(Wnd {
        hwnd,
        frame,
        client,
        class: ci,
        proc_: cproc,
        style,
        exstyle,
        parent,
        id: menu,
        hinst,
        extra: alloc::vec![0; cextra as usize],
    });
    // WM_NCCREATE and WM_CREATE get a CREATESTRUCT (on the 16-bit
    // stack): lpCreateParams, hInstance, hMenu, hwndParent, cy, cx, y,
    // x, style, lpszName, lpszClass, dwExStyle.
    let saved = thunk16::sp16();
    let cs = thunk16::alloc16(34);
    let p = flat(cs);
    let mut b = Vec::with_capacity(34);
    b.extend_from_slice(&param.to_le_bytes());
    for v in [hinst, menu, parent, cy as u16, cx as u16, y as u16, x as u16] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&style.to_le_bytes());
    b.extend_from_slice(&title.to_le_bytes());
    b.extend_from_slice(&class.to_le_bytes());
    b.extend_from_slice(&exstyle.to_le_bytes());
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
    let ok = send(hwnd, W_NCCREATE, 0, cs) != 0 && send(hwnd, W_CREATE, 0, cs) as i32 != -1;
    thunk16::set_sp16(saved);
    if !ok {
        destroy_window(hwnd);
        return 0;
    }
    let _ = cstyle;
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(client, &mut r) };
    send(hwnd, W_SIZE, 0, ((r.t - r.b) as u32) << 16 | (r.r - r.l) as u32 & 0xffff);
    send(hwnd, W_MOVE, 0, client_origin(client));
    if style & WS_VISIBLE != 0 {
        show_window(hwnd, 5);
    }
    if crate::tracing() {
        trace(format_args!("W16 CreateWindow -> {hwnd:#x} (PM {frame:#x}/{client:#x})"));
    }
    hwnd
}

/// The client area's top-left corner in its parent's (or the screen's)
/// y-down coordinates, as an lParam.
fn client_origin(client: u32) -> u32 {
    let parent = unsafe { WinQueryWindow(client, QW_PARENT) };
    let w = by_client(client);
    let top_level = w.as_ref().is_none_or(|w| w.frame != w.client);
    let (to, h) = if top_level { (HWND_DESKTOP, screen_size().1) } else { (parent, client_height(parent)) };
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(client, &mut r) };
    let mut pt = PointL { x: 0, y: r.t };
    unsafe { WinMapWindowPoints(client, to, &mut pt, 1) };
    ((h - pt.y) as u32) << 16 | pt.x as u32 & 0xffff
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

fn map_point(h: u16, p: *mut i16, to_screen: bool) {
    let Some(c) = wnd(h).map(|w| w.client) else { return };
    if p.is_null() {
        return;
    }
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

fn show_window(h: u16, cmd: u16) -> bool {
    let Some(f) = wnd(h).map(|w| w.frame) else { return false };
    let was = unsafe { WinIsWindowVisible(f) } != 0;
    let fl = match cmd {
        0 => SWP_HIDE,
        2 | 6 | 7 => SWP_SHOW | SWP_MINIMIZE,
        3 => SWP_SHOW | SWP_MAXIMIZE | SWP_ACTIVATE | SWP_ZORDER,
        4 | 8 => SWP_SHOW,
        9 => SWP_SHOW | SWP_RESTORE | SWP_ACTIVATE | SWP_ZORDER,
        _ => SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER,
    };
    if cmd != 0 && !was {
        send(h, W_SHOWWINDOW, 1, 0);
    }
    unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, fl) };
    if fl & SWP_ACTIVATE != 0 {
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
    let child = wnd(h).is_some_and(|w| w.style & WS_CHILD != 0);
    let ph = if child { wnd(parent).map_or(0, |p| client_height(p.client)) } else { screen_size().1 };
    let (mut cx, mut cy) = (cx, cy);
    if fl & SWP_SIZE == 0 {
        let mut swp = Swp::default();
        unsafe { WinQueryWindowPos(f, &mut swp) };
        cx = swp.cx;
        cy = swp.cy;
    }
    unsafe { WinSetWindowPos(f, HWND_TOP, x, ph - y - cy, cx, cy, fl) != 0 }
}

fn destroy_window(h: u16) -> bool {
    let Some(f) = wnd(h).map(|w| w.frame) else { return false };
    // PM sends WM_DESTROY to the client (pm_wndproc passes it on, and
    // the window leaves the table there).
    unsafe { WinDestroyWindow(f) };
    if let Some(i) = u().wnds.iter().position(|w| w.hwnd == h) {
        u().wnds.remove(i);
    }
    true
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
    let owner = wnd(h).map_or(0, |w| w.frame);
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

fn post(h: u16, m: u16, wp: u16, lp: u32) -> bool {
    let pm = match wnd(h) {
        Some(w) => w.client,
        None if h == 0 => {
            return unsafe { WinPostQueueMsg(TASK.get().hmq, POSTED | m as u32, wp as u32, lp) } != 0;
        }
        None => return false,
    };
    unsafe { WinPostMsg(pm, POSTED | m as u32, wp as u32, lp) != 0 }
}

/// Windows key state bits for a mouse message's wParam.
fn mouse_keys(msg: u32) -> u16 {
    unsafe extern "C" {
        #[link_name = "PMWIN.752"]
        fn WinGetKeyState(desktop: u32, vk: i32) -> i32;
    }
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
fn vkey(pm_vk: u16, ch: u16) -> u16 {
    match pm_vk {
        0x05 => 0x08,  // BACKSPACE
        0x06 => 0x09,  // TAB
        0x08 | 0x1e => 0x0d, // NEWLINE, ENTER
        0x09 => 0x10,  // SHIFT
        0x0a => 0x11,  // CTRL
        0x0b => 0x12,  // ALT
        0x0d => 0x13,  // PAUSE
        0x0e => 0x14,  // CAPSLOCK
        0x0f => 0x1b,  // ESC
        0x10 => 0x20,  // SPACE
        0x11 => 0x21,  // PAGEUP
        0x12 => 0x22,  // PAGEDOWN
        0x13 => 0x23,  // END
        0x14 => 0x24,  // HOME
        0x15 => 0x25,  // LEFT
        0x16 => 0x26,  // UP
        0x17 => 0x27,  // RIGHT
        0x18 => 0x28,  // DOWN
        0x19 => 0x2c,  // PRINTSCRN
        0x1a => 0x2d,  // INSERT
        0x1b => 0x2e,  // DELETE
        0x1d => 0x90,  // NUMLOCK
        0x20..=0x2b => 0x70 + (pm_vk - 0x20), // F1-F12
        _ => {
            let c = (ch as u8).to_ascii_uppercase();
            if c.is_ascii_alphanumeric() { c as u16 } else { 0 }
        }
    }
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
            let (chr, pvk) = (q.mp2 as u16, (q.mp2 >> 16) as u16);
            let vk = if fs & 0x0002 != 0 { vkey(pvk, chr) } else { vkey(0, chr) };
            let up = fs & 0x0040 != 0;
            let alt = fs & 0x0020 != 0;
            if vk == 0 && fs & 0x0001 == 0 {
                return None;
            }
            let lp = repeat.max(1) | scan << 16 | if alt { 1 << 29 } else { 0 } | if up { 0xc000_0000 } else { 0 };
            let m = match (up, alt) {
                (false, false) => W_KEYDOWN,
                (true, false) => W_KEYUP,
                (false, true) => W_SYSKEYDOWN,
                (true, true) => W_SYSKEYUP,
            };
            if !up && fs & 0x0001 != 0 && chr != 0 {
                u().last_char = Some((w_hwnd, if alt { W_SYSCHAR } else { W_CHAR }, chr & 0xff, lp));
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
fn get_message(p: *mut u8, h: u16, _lo: u16, _hi: u16, peek: Option<u16>) -> u32 {
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
            continue;
        };
        // WM_SETCURSOR before the mouse messages, as Windows' input
        // processing sends it.
        if msg == W_MOUSEMOVE || (0x201..=0x209).contains(&msg) {
            send(hwnd, W_SETCURSOR, hwnd, (msg as u32) << 16 | 1);
        }
        write_msg(p, hwnd, msg, wp, lp, q.time, (q.x, q.y));
        u().last = Some((hwnd, msg, q));
        return 1;
    }
}

/// The Windows message (hwnd, msg, wParam, lParam) of a PM queue
/// message, if it is one for the task.
fn win_message(q: &QMsg) -> Option<(u16, u16, u16, u32)> {
    let h = by_client(q.hwnd).map_or(0, |w| w.hwnd);
    if h != 0 {
        return translate_pm(h, q).map(|(m, wp, lp)| (h, m, wp, lp));
    }
    if q.msg & POSTED != 0 {
        return Some((0, q.msg as u16, q.mp1 as u16, q.mp2));
    }
    if q.msg == WM_TIMER && q.hwnd == 0 {
        // A timer of no window: the id and the procedure.
        let t = u().timers.iter().find(|t| t.hwnd == 0 && t.pm_id == q.mp1 & 0xffff)?;
        return Some((0, W_TIMER, t.id, t.proc_));
    }
    None
}

fn write_msg(p: *mut u8, hwnd: u16, msg: u16, wp: u16, lp: u32, time: u32, pt: (i32, i32)) {
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

fn read_msg(p: *const u8) -> Option<(u16, u16, u16, u32)> {
    if p.is_null() {
        return None;
    }
    let w = |o: usize| unsafe { (p.add(o) as *const u16).read_unaligned() };
    let l = unsafe { (p.add(6) as *const u32).read_unaligned() };
    Some((w(0), w(2), w(4), l))
}

fn translate(p: *const u16) -> bool {
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

fn dispatch(p: *mut u8) -> u32 {
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
            send_inflight(h, W_MOVE, 0, client_origin(pmh), q);
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
            if let Some(i) = u().wnds.iter().position(|w| w.hwnd == h) {
                u().wnds.remove(i);
            }
            unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) }
        }
        // BeginPaint erases (WM_ERASEBKGND).
        WM_ERASEBACKGROUND => 0,
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
fn def_window_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    let inflight = u().inflight.iter().rev().find(|f| f.hwnd == h && f.msg == m).copied();
    match m {
        W_NCCREATE => return 1,
        W_CLOSE => {
            destroy_window(h);
            return 0;
        }
        W_PAINT => {
            // Validate: an empty BeginPaint/EndPaint.
            if let Some(c) = wnd(h).map(|w| w.client) {
                let hps = unsafe { WinBeginPaint(c, 0, core::ptr::null_mut()) };
                unsafe { WinEndPaint(hps) };
            }
            return 0;
        }
        W_ERASEBKGND => {
            let Some((c, cls)) = wnd(h).map(|w| (w.client, w.class)) else { return 0 };
            let br = u().classes.get(cls).map_or(0, |c| c.brush);
            if br == 0 {
                return 0;
            }
            let mut r = RectL::default();
            unsafe { WinQueryWindowRect(c, &mut r) };
            gdi::fill_rect(wp, [0, 0, r.r, r.t - r.b], br);
            return 1;
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
            if let Some(f) = wnd(h).map(|w| w.frame) {
                let s = z(far_str(lp).unwrap_or(b""));
                unsafe { WinSetWindowText(f, s.as_ptr()) };
            }
            return 1;
        }
        W_GETTEXT => {
            let Some(f) = wnd(h).map(|w| w.frame) else { return 0 };
            let buf = flat(lp);
            if buf.is_null() || wp == 0 {
                return 0;
            }
            return unsafe { WinQueryWindowText(f, wp as i32, buf) } as u32;
        }
        W_GETTEXTLENGTH => {
            let Some(f) = wnd(h).map(|w| w.frame) else { return 0 };
            let mut b = [0u8; 256];
            return unsafe { WinQueryWindowText(f, 256, b.as_mut_ptr()) } as u32;
        }
        W_SYSCOMMAND => {
            let f = wnd(h).map_or(0, |w| w.frame);
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
        _ => {}
    }
    match inflight {
        Some(f) => unsafe { WinDefWindowProc(f.q.hwnd, f.q.msg, f.q.mp1, f.q.mp2) },
        None => 0,
    }
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

fn system_metric(i: i32) -> i32 {
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    match i {
        0 => q(SV_CXSCREEN),
        1 => q(SV_CYSCREEN),
        2 | 3 => 17,
        4 => q(SV_CYTITLEBAR),
        5 => q(SV_CXBORDER),
        6 => q(SV_CYBORDER),
        7 | 8 => 4,
        9 | 10 => 17,
        11 | 12 => 32,
        13 => q(SV_CXPOINTER),
        14 => q(SV_CYPOINTER),
        15 => 18,
        16 => q(SV_CXSCREEN),
        17 => q(SV_CYSCREEN) - q(SV_CYTITLEBAR),
        19 => 1,  // SM_MOUSEPRESENT
        28 => 100, // SM_CXMIN
        29 => 26,
        32 => q(SV_CXSIZEBORDER),
        33 => q(SV_CYSIZEBORDER),
        34 | 35 => 32,
        43 => 3, // SM_CMOUSEBUTTONS
        _ => 0,
    }
}
