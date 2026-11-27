//! USER's window manager: classes, windows, messages, painting.
//!
//! WIN16's (userland/win16/src/user.rs) in 32-bit form. A top-level
//! window with a caption or a frame is a PM frame window (title bar,
//! system menu, sizing border and buttons from the window style, the
//! menu bar) with a client window of the class `Win32Wnd`. Other windows
//! (child windows, and pop-ups without a caption) are `Win32Wnd` windows
//! in their parent's client (or on the desktop); one with a border or
//! scroll bars is a `Win32Frame` window that draws the border and holds
//! the client and the scroll bars (Windows' non-client area). The HWND is
//! a handle of this table; messages for it arrive on the PM client.
//!
//! Messages PM sends to the client come to `pm_wndproc`, which
//! translates them (Windows' y grows down from the client's top-left
//! corner) and calls the window procedure. Messages PM posts (input,
//! WM_PAINT, WM_TIMER) come out of GetMessage translated the same way
//! and go to the procedure through DispatchMessage. While a translated
//! message is being handled, DefWindowProc for it passes the PM message
//! on to WinDefWindowProc; other messages get Windows' default here.
//!
//! Messages are handled in their A form (text in the process code page):
//! a window whose procedure is a W one (its class registered with
//! RegisterClassW, or subclassed with SetWindowLongW) gets the W form,
//! converted on the way (`deliver`). A window belongs to the thread that
//! made it; SendMessage to another thread's window goes through PM's
//! WinSendMsg, which waits for that thread to take it.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;

use crate::gdi;
use crate::pm::*;
use crate::res::Name;
use crate::{call4, far_str, queue, read_rect, tid, trace, write_rect, z, Global};

// Windows messages.
pub const W_NULL: u16 = 0x0000;
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
pub const W_SYSCOLORCHANGE: u16 = 0x0015;
pub const W_SHOWWINDOW: u16 = 0x0018;
pub const W_SETTINGCHANGE: u16 = 0x001a;
pub const W_ACTIVATEAPP: u16 = 0x001c;
pub const W_SETCURSOR: u16 = 0x0020;
pub const W_MOUSEACTIVATE: u16 = 0x0021;
pub const W_GETMINMAXINFO: u16 = 0x0024;
pub const W_NEXTDLGCTL: u16 = 0x0028;
pub const W_DRAWITEM: u16 = 0x002b;
pub const W_MEASUREITEM: u16 = 0x002c;
pub const W_DELETEITEM: u16 = 0x002d;
pub const W_VKEYTOITEM: u16 = 0x002e;
pub const W_CHARTOITEM: u16 = 0x002f;
pub const W_SETFONT: u16 = 0x0030;
pub const W_GETFONT: u16 = 0x0031;
pub const W_COMPAREITEM: u16 = 0x0039;
pub const W_WINDOWPOSCHANGING: u16 = 0x0046;
pub const W_WINDOWPOSCHANGED: u16 = 0x0047;
pub const W_NOTIFY: u16 = 0x004e;
pub const W_HELP: u16 = 0x0053;
pub const W_NOTIFYFORMAT: u16 = 0x0055;
pub const W_CONTEXTMENU: u16 = 0x007b;
pub const W_STYLECHANGING: u16 = 0x007c;
pub const W_STYLECHANGED: u16 = 0x007d;
pub const W_GETICON: u16 = 0x007f;
pub const W_SETICON: u16 = 0x0080;
pub const W_NCCREATE: u16 = 0x0081;
pub const W_NCDESTROY: u16 = 0x0082;
pub const W_NCCALCSIZE: u16 = 0x0083;
pub const W_NCHITTEST: u16 = 0x0084;
pub const W_NCPAINT: u16 = 0x0085;
pub const W_NCACTIVATE: u16 = 0x0086;
pub const W_GETDLGCODE: u16 = 0x0087;
pub const W_KEYDOWN: u16 = 0x0100;
pub const W_KEYUP: u16 = 0x0101;
pub const W_CHAR: u16 = 0x0102;
pub const W_DEADCHAR: u16 = 0x0103;
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
pub const W_MENUCHAR: u16 = 0x0120;
pub const W_ENTERIDLE: u16 = 0x0121;
pub const W_CTLCOLORMSGBOX: u16 = 0x0132;
pub const W_CTLCOLOREDIT: u16 = 0x0133;
pub const W_CTLCOLORLISTBOX: u16 = 0x0134;
pub const W_CTLCOLORBTN: u16 = 0x0135;
pub const W_CTLCOLORDLG: u16 = 0x0136;
pub const W_CTLCOLORSCROLLBAR: u16 = 0x0137;
pub const W_CTLCOLORSTATIC: u16 = 0x0138;
pub const W_MOUSEMOVE: u16 = 0x0200;
pub const W_LBUTTONDOWN: u16 = 0x0201;
pub const W_LBUTTONUP: u16 = 0x0202;
pub const W_LBUTTONDBLCLK: u16 = 0x0203;
pub const W_RBUTTONDOWN: u16 = 0x0204;
pub const W_RBUTTONUP: u16 = 0x0205;
pub const W_MBUTTONDOWN: u16 = 0x0207;
pub const W_MOUSEWHEEL: u16 = 0x020a;
pub const W_PARENTNOTIFY: u16 = 0x0210;
pub const W_ENTERMENULOOP: u16 = 0x0211;
pub const W_EXITMENULOOP: u16 = 0x0212;
pub const W_CAPTURECHANGED: u16 = 0x0215;
pub const W_MDICREATE: u16 = 0x0220;
pub const W_CUT: u16 = 0x0300;
pub const W_COPY: u16 = 0x0301;
pub const W_PASTE: u16 = 0x0302;
pub const W_CLEAR: u16 = 0x0303;
pub const W_UNDO: u16 = 0x0304;
pub const W_QUERYNEWPALETTE: u16 = 0x030f;
pub const W_PALETTECHANGED: u16 = 0x0311;
pub const W_PRINTCLIENT: u16 = 0x0318;
pub const W_USER: u16 = 0x0400;

// Window styles.
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_MINIMIZE: u32 = 0x2000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_DISABLED: u32 = 0x0800_0000;
pub const W_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_MAXIMIZE: u32 = 0x0100_0000;
pub const WS_CAPTION: u32 = 0x00c0_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_DLGFRAME: u32 = 0x0040_0000;
pub const WS_VSCROLL: u32 = 0x0020_0000;
pub const WS_HSCROLL: u32 = 0x0010_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_GROUP: u32 = 0x0002_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
pub const WS_EX_DLGMODALFRAME: u32 = 0x0000_0001;
pub const WS_EX_NOPARENTNOTIFY: u32 = 0x0000_0004;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;

pub const CS_VREDRAW: u32 = 0x0001;
pub const CS_HREDRAW: u32 = 0x0002;
pub const CS_DBLCLKS: u32 = 0x0008;
pub const CS_OWNDC: u32 = 0x0020;
pub const CS_CLASSDC: u32 = 0x0040;
pub const CS_PARENTDC: u32 = 0x0080;
pub const CS_GLOBALCLASS: u32 = 0x4000;

pub const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;

/// PM message numbers of Windows messages: posted with PostMessage, and
/// sent from another thread (A or W form).
pub const POSTED: u32 = 0x0001_0000;
pub const XSEND_A: u32 = 0x0002_0000;
pub const XSEND_W: u32 = 0x0003_0000;

pub struct Class {
    /// Upper case.
    pub name: Vec<u8>,
    pub atom: u16,
    pub style: u32,
    pub proc_: u32,
    /// The procedure takes the W forms of messages (RegisterClassW).
    pub unicode: bool,
    pub cls_extra: Vec<u8>,
    pub wnd_extra: u32,
    /// The module (0: the system's classes).
    pub hinst: u32,
    pub icon: u32,
    pub icon_sm: u32,
    pub cursor: u32,
    pub brush: u32,
    pub menu: Option<Name>,
}

pub struct Wnd {
    pub hwnd: u16,
    /// The PM frame (top-level windows), the `Win32Frame` (windows with
    /// a border or scroll bars) or the window itself.
    pub frame: u32,
    /// The PM window of the client area.
    pub client: u32,
    pub class: usize,
    pub proc_: u32,
    pub unicode: bool,
    pub style: u32,
    pub exstyle: u32,
    /// The parent (child windows) or the owner (top-level windows).
    pub parent: u16,
    pub id: u32,
    pub hinst: u32,
    pub user_data: u32,
    pub extra: Vec<u8>,
    /// The window text (windows without a title bar).
    pub text: Vec<u8>,
    /// The menu bar (HMENU) of a top-level window.
    pub menu: u32,
    /// A PM frame window (with a title bar or a frame).
    pub framed: bool,
    /// The non-client parts of a plain window: WS_BORDER, WS_DLGFRAME,
    /// WS_THICKFRAME, WS_VSCROLL, WS_HSCROLL.
    pub nc: u32,
    /// The state of a built-in control.
    pub data: Option<Box<dyn Any>>,
    pub vsb: Option<crate::scroll::Bar>,
    pub hsb: Option<crate::scroll::Bar>,
    /// SetProp's properties: (name, data); an atom's name is "#atom".
    pub props: Vec<(Vec<u8>, u32)>,
    /// WM_NCCREATE has been sent: PM's messages reach the procedure from
    /// then on.
    pub created: bool,
    /// An overlapped window's first ShowWindow sends WM_SIZE and WM_MOVE.
    pub need_size: bool,
    /// The client origin WM_MOVE last told the program.
    pub moved_to: Option<u32>,
    /// The thread that made it.
    pub tid: u32,
    /// The icons of WM_SETICON (big, small).
    pub icons: (u32, u32),
    /// DestroyWindow is under way: it sends WM_DESTROY itself.
    pub destroying: bool,
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
    id: u32,
    pm_id: u32,
    proc_: u32,
    tid: u32,
}

struct User {
    classes: Vec<Class>,
    wnds: Vec<Box<Wnd>>,
    /// Destroyed windows, freed when no built-in procedure is running.
    gone: Vec<Box<Wnd>>,
    next_hwnd: u16,
    next_atom: u16,
    inflight: Vec<InFlight>,
    /// The PM message behind the last message GetMessage returned.
    last: Option<(u16, u16, QMsg)>,
    /// The characters of the WM_KEYDOWNs translated but not yet given to
    /// TranslateMessage: (PM time, hwnd, wParam, lParam, WM_CHAR or
    /// WM_SYSCHAR, the character).
    chars: Vec<(u32, u16, u32, u32, u16, u32)>,
    timers: Vec<Timer>,
    next_timer: u32,
    cascade: i32,
    msg_time: u32,
    msg_pos: u32,
    /// Procedures replaced by subclassing, and whether each takes W
    /// messages (CallWindowProc calls them in their form).
    procs: Vec<(u32, bool)>,
    /// Depth of built-in procedures running (freeing waits).
    native_depth: u32,
    pm_registered: bool,
}

static USER: Global<User> = Global::new(User {
    classes: Vec::new(),
    wnds: Vec::new(),
    gone: Vec::new(),
    next_hwnd: 0x4004,
    next_atom: 0xc000,
    inflight: Vec::new(),
    last: None,
    chars: Vec::new(),
    timers: Vec::new(),
    next_timer: 0x7000,
    cascade: 0,
    msg_time: 0,
    msg_pos: 0,
    procs: Vec::new(),
    native_depth: 0,
    pm_registered: false,
});

fn u() -> &'static mut User {
    USER.get()
}

/// A window by handle (the low word: HWNDs are 32 bits to programs).
pub fn wnd(h: u16) -> Option<&'static mut Wnd> {
    if h == 0 {
        return None;
    }
    u().wnds.iter_mut().find(|w| w.hwnd == h).map(|w| &mut **w)
}

/// A window by a program's HWND.
pub fn hw(h: u32) -> u16 {
    if h > 0xffff { 0 } else { h as u16 }
}

/// The state of a built-in control.
pub fn data<T: 'static>(h: u16) -> Option<&'static mut T> {
    wnd(h)?.data.as_mut()?.downcast_mut::<T>()
}

fn by_client(pm: u32) -> Option<&'static mut Wnd> {
    u().wnds.iter_mut().find(|w| w.client == pm).map(|w| &mut **w)
}

fn by_frame(pm: u32) -> Option<&'static mut Wnd> {
    u().wnds.iter_mut().find(|w| w.frame == pm || w.client == pm).map(|w| &mut **w)
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

/// The client area's update rectangle (Windows coordinates).
pub fn update_rect(h: u16) -> Option<[i32; 4]> {
    let c = wnd(h)?.client;
    let mut rc = RectL::default();
    let some = unsafe { WinQueryUpdateRect(c, &mut rc) } != 0 && rc.r > rc.l && rc.t > rc.b;
    let ch = client_height(c);
    some.then_some([rc.l, ch - rc.t, rc.r, ch - rc.b])
}

pub fn owned_popups(h: u16) -> Vec<u16> {
    u().wnds.iter().filter(|w| w.parent == h && w.style & WS_CHILD == 0).map(|w| w.hwnd).collect()
}

/// Is input or a message waiting in the thread's queue?
pub fn input_waiting() -> bool {
    let mut q = QMsg::default();
    unsafe { WinPeekMsg(queue().hab, &mut q, 0, 0, 0, PM_NOREMOVE) != 0 }
}

pub fn top_levels() -> Vec<u16> {
    u().wnds.iter().filter(|w| w.style & WS_CHILD == 0).map(|w| w.hwnd).collect()
}

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

/// The client height of a PM window (for y-down coordinates).
pub fn client_height(pm: u32) -> i32 {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(pm, &mut r) };
    r.t - r.b
}

pub fn client_size(h: u16) -> (i32, i32) {
    let Some(c) = wnd(h).map(|w| w.client) else { return (0, 0) };
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(c, &mut r) };
    (r.r - r.l, r.t - r.b)
}

pub fn screen_size() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) }
}

// ---- Window procedures ----

/// The built-in procedures: (their address for programs, the Rust
/// procedure). Their addresses are their exports' (stdcall thunks).
pub type Native = fn(u16, u16, u32, u32) -> u32;

static NATIVES: Global<Vec<(u32, Native)>> = Global::new(Vec::new());

pub fn add_native(addr: u32, f: Native) {
    NATIVES.get().push((addr, f));
}

fn native(p: u32) -> Option<Native> {
    NATIVES.get().iter().find(|n| n.0 == p).map(|n| n.1)
}

pub fn is_native(p: u32) -> bool {
    native(p).is_some()
}

pub fn native_depth() -> u32 {
    u().native_depth
}

/// Call a window procedure with a message in its own form.
fn raw_call(p: u32, h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    if let Some(f) = native(p) {
        u().native_depth += 1;
        let r = f(h, m, wp, lp);
        u().native_depth -= 1;
        return r;
    }
    if crate::tracing() {
        trace(format_args!("W32 msg {h:#x} {m:#x} {wp:#x} {lp:#x}"));
    }
    call4(p, h as u32, m as u32, wp, lp)
}

/// Call a procedure that takes the W (`proc_wide`) or A form with a
/// message in the W (`args_wide`) or A form, converting the text of the
/// messages that carry it.
pub fn call_conv(p: u32, proc_wide: bool, h: u16, m: u16, wp: u32, lp: u32, args_wide: bool) -> u32 {
    let proc_wide = proc_wide && !is_native(p);
    if proc_wide == args_wide {
        return raw_call(p, h, m, wp, lp);
    }
    match m {
        W_SETTEXT | W_SETTINGCHANGE if lp != 0 => {
            if args_wide {
                let s = z(&crate::narrow(unsafe { crate::wstr(lp as *const u16) }));
                raw_call(p, h, m, wp, s.as_ptr() as u32)
            } else {
                let s = crate::zw(&crate::to_wide(unsafe { crate::cstr(lp as *const u8) }));
                raw_call(p, h, m, wp, s.as_ptr() as u32)
            }
        }
        W_GETTEXT if lp != 0 && wp != 0 => {
            if args_wide {
                // The A procedure fills bytes; the caller wants UTF-16.
                let mut b = alloc::vec![0u8; wp as usize * 2 + 1];
                raw_call(p, h, m, b.len() as u32, b.as_mut_ptr() as u32);
                let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
                let w = crate::to_wide(&b[..n]);
                crate::copy_out_w(&w, lp as *mut u16, wp as i32) as u32
            } else {
                let mut b = alloc::vec![0u16; wp as usize + 1];
                raw_call(p, h, m, b.len() as u32, b.as_mut_ptr() as u32);
                let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
                let a = crate::narrow(&b[..n]);
                crate::copy_out(&a, lp as *mut u8, wp as i32) as u32
            }
        }
        W_NCCREATE | W_CREATE if lp != 0 => {
            // CREATESTRUCT: the name (offset 36) and class (40) converted.
            let mut cs = [0u32; 12];
            unsafe { core::ptr::copy_nonoverlapping(lp as *const u32, cs.as_mut_ptr(), 12) };
            let conv = |v: u32| -> Option<Vec<u8>> {
                if v >> 16 == 0 {
                    return None;
                }
                Some(if args_wide {
                    z(&crate::narrow(unsafe { crate::wstr(v as *const u16) }))
                } else {
                    let w = crate::zw(&crate::to_wide(unsafe { crate::cstr(v as *const u8) }));
                    w.iter().flat_map(|c| c.to_le_bytes()).collect()
                })
            };
            let (name, class) = (conv(cs[9]), conv(cs[10]));
            if let Some(n) = &name {
                cs[9] = n.as_ptr() as u32;
            }
            if let Some(c) = &class {
                cs[10] = c.as_ptr() as u32;
            }
            let r = raw_call(p, h, m, wp, cs.as_ptr() as u32);
            // (A procedure may change lpCreateParams' fields: none kept.)
            r
        }
        W_CHAR | W_SYSCHAR | W_DEADCHAR => {
            let c = if args_wide {
                let a = crate::narrow(&[wp as u16]);
                a.first().copied().unwrap_or(b'?') as u32
            } else {
                crate::to_wide(&[wp as u8]).first().copied().unwrap_or(0x3f) as u32
            };
            raw_call(p, h, m, c, lp)
        }
        _ => raw_call(p, h, m, wp, lp),
    }
}

/// Deliver a message to a window's procedure (A form unless `wide`).
pub fn deliver(hwnd: u16, msg: u16, wp: u32, lp: u32, wide: bool) -> u32 {
    let Some((p, uni, t)) = wnd(hwnd).map(|w| (w.proc_, w.unicode, w.tid)) else { return 0 };
    if t != tid() {
        // Another thread's window: its thread takes the message.
        let c = wnd(hwnd).map_or(0, |w| w.client);
        return unsafe { WinSendMsg(c, if wide { XSEND_W } else { XSEND_A } | msg as u32, wp, lp) };
    }
    call_conv(p, uni, hwnd, msg, wp, lp, wide)
}

/// Send a message (A form) to a window: its procedure, now.
pub fn send(hwnd: u16, msg: u16, wp: u32, lp: u32) -> u32 {
    if hwnd == 0xffff {
        for h in top_levels() {
            send(h, msg, wp, lp);
        }
        return 0;
    }
    deliver(hwnd, msg, wp, lp, false)
}

/// WM_COMMAND notification from a control to its parent.
pub fn notify(ctl: u16, code: u16) {
    let Some((parent, id)) = wnd(ctl).map(|w| (w.parent, w.id)) else { return };
    send(parent, W_COMMAND, (code as u32) << 16 | id & 0xffff, ctl as u32);
}

/// Send a translated PM message: DefWindowProc for it goes to PM.
fn send_inflight(hwnd: u16, msg: u16, wp: u32, lp: u32, q: QMsg) -> u32 {
    u().inflight.push(InFlight { hwnd, msg, q });
    let r = send(hwnd, msg, wp, lp);
    u().inflight.pop();
    r
}

/// Free the destroyed windows (when no built-in procedure runs).
pub fn collect() {
    if u().native_depth == 0 {
        u().gone.clear();
    }
}

/// Remember the form of a procedure a window had (subclassing).
pub fn note_proc(p: u32, wide: bool) {
    let v = &mut u().procs;
    if let Some(e) = v.iter_mut().find(|e| e.0 == p) {
        e.1 = wide;
    } else {
        v.push((p, wide));
    }
}

/// Whether a procedure takes W messages, if known.
pub fn proc_form(p: u32) -> Option<bool> {
    if is_native(p) {
        return Some(false);
    }
    u().procs.iter().find(|e| e.0 == p).map(|e| e.1).or_else(|| u().classes.iter().find(|c| c.proc_ == p).map(|c| c.unicode))
}

// ---- Classes ----

/// PM's classes and the built-in Windows classes, when the first queue
/// of the process is made.
pub fn queue_created(hab: u32) {
    unsafe {
        // CS_MOVENOTIFY: WM_MOVE when a window moves.
        WinRegisterClass(hab, b"Win32Wnd\0".as_ptr(), pm_wndproc, 0x0001, 4);
        WinRegisterClass(hab, b"Win32Frame\0".as_ptr(), frame_proc, 0, 4);
    }
    crate::scroll::register(hab);
    if core::mem::replace(&mut u().pm_registered, true) {
        return;
    }
    crate::user32::register_builtin();
}

/// What RegisterClass(Ex) is given.
pub struct NewClass {
    pub name: Vec<u8>,
    pub style: u32,
    pub proc_: u32,
    pub unicode: bool,
    pub cls_extra: u32,
    pub wnd_extra: u32,
    pub hinst: u32,
    pub icon: u32,
    pub icon_sm: u32,
    pub cursor: u32,
    pub brush: u32,
    pub menu: Option<Name>,
}

/// RegisterClass: the class atom (0: one of the name is there).
pub fn register_class(c: NewClass) -> u16 {
    let name = c.name.to_ascii_uppercase();
    if name.is_empty() {
        return 0;
    }
    let s = u();
    let global = c.style & CS_GLOBALCLASS != 0;
    if s.classes.iter().any(|k| k.name == name && k.hinst != 0 && (k.hinst == c.hinst || global && k.style & CS_GLOBALCLASS != 0)) {
        return crate::fail(1410, 0); // ERROR_CLASS_ALREADY_EXISTS
    }
    if crate::tracing() {
        trace(format_args!("W32 RegisterClass {} instance {:#x} style {:#x}", name.iter().map(|&c| c as char).collect::<alloc::string::String>(), c.hinst, c.style));
    }
    let atom = s.next_atom;
    s.next_atom += 1;
    s.classes.push(Class {
        name,
        atom,
        style: c.style,
        proc_: c.proc_,
        unicode: c.unicode,
        cls_extra: alloc::vec![0; c.cls_extra.min(4096) as usize],
        wnd_extra: c.wnd_extra.min(4096),
        hinst: c.hinst,
        icon: c.icon,
        icon_sm: c.icon_sm,
        cursor: c.cursor,
        brush: c.brush,
        menu: c.menu,
    });
    atom
}

/// A built-in class (no instance).
pub fn register_system(name: &[u8], atom: u16, style: u32, proc_: u32, extra: u32, cursor: u32, brush: u32) {
    u().classes.push(Class {
        name: name.to_vec(),
        atom,
        style,
        proc_,
        unicode: false,
        cls_extra: Vec::new(),
        wnd_extra: extra,
        hinst: 0,
        icon: 0,
        icon_sm: 0,
        cursor,
        brush,
        menu: None,
    });
}

/// A class name or atom as given (an A or W pointer).
pub enum ClassName {
    Atom(u16),
    Name(Vec<u8>),
}

impl ClassName {
    pub fn of(p: u32, wide: bool) -> ClassName {
        if p >> 16 == 0 {
            return ClassName::Atom(p as u16);
        }
        let n = if wide { crate::wide_str(p).unwrap_or_default() } else { far_str(p).unwrap_or(b"").to_vec() };
        ClassName::Name(n)
    }
}

/// A class as Windows finds it for a module: the module's own classes,
/// then the global ones (CS_GLOBALCLASS), then the system's.
pub fn find_class(name: &ClassName, hinst: u32) -> Option<usize> {
    let s = u();
    let hit = |c: &Class| match name {
        ClassName::Name(n) => !c.name.is_empty() && c.name.eq_ignore_ascii_case(n),
        ClassName::Atom(a) => c.atom == *a && *a != 0,
    };
    let exe = unsafe { crate::GetModuleHandleA(core::ptr::null()) };
    let hinst = if hinst == 0 { exe } else { hinst };
    s.classes
        .iter()
        .position(|c| hit(c) && c.hinst == hinst && c.hinst != 0)
        .or_else(|| s.classes.iter().position(|c| hit(c) && c.hinst != 0 && c.style & CS_GLOBALCLASS != 0))
        .or_else(|| s.classes.iter().position(|c| hit(c) && c.hinst == 0))
        .or_else(|| s.classes.iter().rposition(|c| hit(c)))
}

pub fn class(i: usize) -> Option<&'static mut Class> {
    u().classes.get_mut(i)
}

pub fn unregister_class(name: &ClassName, hinst: u32) -> bool {
    let Some(i) = find_class(name, hinst) else { return false };
    if u().classes[i].hinst == 0 || u().wnds.iter().any(|w| w.class == i) {
        return false;
    }
    // (Indexes stay: the class is emptied, not removed.)
    let c = &mut u().classes[i];
    c.name.clear();
    c.atom = 0;
    true
}

// ---- Creating windows ----

fn pm_clip_children(style: u32) -> u32 {
    if style & WS_CLIPCHILDREN != 0 { crate::pm::WS_CLIPCHILDREN } else { 0 }
}

fn frame_flags(style: u32, exstyle: u32, owned: bool) -> u32 {
    let mut f = FCF_NOBYTEALIGN;
    if style & WS_CAPTION == WS_CAPTION {
        f |= FCF_TITLEBAR;
        if !owned && exstyle & WS_EX_TOOLWINDOW == 0 {
            f |= FCF_TASKLIST;
        }
    }
    if style & WS_SYSMENU != 0 {
        f |= FCF_SYSMENU;
    }
    if style & WS_CAPTION == WS_CAPTION && style & WS_MINIMIZEBOX != 0 {
        f |= FCF_MINBUTTON;
    }
    if style & WS_CAPTION == WS_CAPTION && style & WS_MAXIMIZEBOX != 0 {
        f |= FCF_MAXBUTTON;
    }
    if style & WS_THICKFRAME != 0 {
        f |= FCF_SIZEBORDER;
    } else if style & WS_DLGFRAME != 0 || exstyle & WS_EX_DLGMODALFRAME != 0 {
        f |= FCF_DLGBORDER;
    } else if style & WS_BORDER != 0 {
        f |= FCF_BORDER;
    }
    f
}

/// The non-client insets (left, top, right, bottom) of a window style
/// (AdjustWindowRect).
pub fn frame_insets(style: u32, exstyle: u32, menu: bool) -> (i32, i32, i32, i32) {
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    let b = if style & WS_THICKFRAME != 0 {
        q(SV_CXSIZEBORDER)
    } else if style & WS_DLGFRAME != 0 || exstyle & WS_EX_DLGMODALFRAME != 0 {
        q(SV_CXDLGFRAME)
    } else if style & WS_BORDER != 0 {
        q(SV_CXBORDER)
    } else {
        0
    };
    let edge = if exstyle & WS_EX_CLIENTEDGE != 0 { 2 } else { 0 };
    let mut top = b + edge;
    if style & WS_CAPTION == WS_CAPTION {
        top += q(SV_CYTITLEBAR);
    }
    if menu {
        top += q(SV_CYMENU);
    }
    let r = b + edge + if style & WS_VSCROLL != 0 { q(SV_CXVSCROLL) } else { 0 };
    let bb = b + edge + if style & WS_HSCROLL != 0 { q(SV_CYHSCROLL) } else { 0 };
    (b + edge, top, r, bb)
}

fn caption_height(nc: u32) -> i32 {
    if nc & WS_CAPTION == WS_CAPTION { unsafe { WinQuerySysValue(HWND_DESKTOP, SV_CYTITLEBAR) } } else { 0 }
}

/// The border of a plain window (WS_THICKFRAME, WS_DLGFRAME, WS_BORDER,
/// WS_EX_CLIENTEDGE's sunken edge: bit 31 of `nc` here).
fn plain_border(nc: u32) -> i32 {
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    let edge = if nc & 0x8000_0000 != 0 { 2 } else { 0 };
    edge + if nc & WS_THICKFRAME != 0 {
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

/// What CreateWindowEx is given.
pub struct Create {
    pub exstyle: u32,
    pub class: usize,
    pub title: Vec<u8>,
    /// The title and class as the program gave them (for CREATESTRUCT).
    pub title_arg: u32,
    pub class_arg: u32,
    /// Those are W strings.
    pub wide: bool,
    pub style: u32,
    pub pos: [i32; 4],
    pub parent: u16,
    /// The menu (top-level windows) or the ID (child windows).
    pub menu: u32,
    pub hinst: u32,
    pub param: u32,
}

/// Create a window: the PM windows, the table entry, WM_NCCREATE and
/// WM_CREATE, WM_SIZE and WM_MOVE; shown if WS_VISIBLE.
pub fn create(c: &Create) -> u16 {
    let ci = c.class;
    let (cproc, cextra, cuni) = {
        let cl = &u().classes[ci];
        (cl.proc_, cl.wnd_extra, cl.unicode)
    };
    queue();
    let mut style = c.style;
    // An overlapped window has a caption and a border.
    if style & (WS_CHILD | WS_POPUP) == 0 {
        style |= WS_CAPTION;
    }
    let text = z(&c.title);
    let [mut x, mut y, mut cx, mut cy] = c.pos;
    let child = style & WS_CHILD != 0;
    let parent = c.parent;
    let framed = is_framed(style);
    let edge = if c.exstyle & WS_EX_CLIENTEDGE != 0 { 0x8000_0000 } else { 0 };
    let nc = style & (WS_BORDER | WS_DLGFRAME | WS_THICKFRAME | WS_VSCROLL | WS_HSCROLL | if child { WS_CAPTION } else { 0 }) | edge;
    if child && wnd(parent).is_none() {
        return crate::fail(1406, 0); // ERROR_TLW_WITH_WSCHILD
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
            cx = 0;
            cy = 0;
        }
    }
    let (frame, client) = if framed {
        let mut fcf = frame_flags(style, c.exstyle, parent != 0);
        let mut client = 0;
        let f = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"Win32Wnd\0".as_ptr(), text.as_ptr(), pm_clip_children(style), 0, 0, &mut client) };
        if f != 0 {
            let old = unsafe { WinSubclassWindow(f, std_frame_proc) };
            STD_FRAME_PROC.store(old, core::sync::atomic::Ordering::Relaxed);
        }
        (f, client)
    } else {
        let pm_parent = if child { wnd(parent).map_or(0, |w| w.client) } else { HWND_DESKTOP };
        let ph = if child { client_height(pm_parent) } else { sy };
        let pmy = ph - y - cy;
        let behind = if child { HWND_BOTTOM } else { HWND_TOP };
        let siblings = if !child || style & W_CLIPSIBLINGS != 0 { WS_CLIPSIBLINGS } else { 0 };
        let clip = pm_clip_children(style) | siblings;
        if nc != 0 {
            let f = unsafe { WinCreateWindow(pm_parent, b"Win32Frame\0".as_ptr(), text.as_ptr(), crate::pm::WS_CLIPCHILDREN | siblings, x, pmy, cx, cy, pm_parent, behind, c.menu, core::ptr::null(), core::ptr::null()) };
            let cl = unsafe { WinCreateWindow(f, b"Win32Wnd\0".as_ptr(), text.as_ptr(), clip | crate::pm::WS_VISIBLE, 0, 0, cx, cy, f, HWND_TOP, FID_CLIENT, core::ptr::null(), core::ptr::null()) };
            (f, cl)
        } else {
            let h = unsafe { WinCreateWindow(pm_parent, b"Win32Wnd\0".as_ptr(), text.as_ptr(), clip, x, pmy, cx, cy, pm_parent, behind, c.menu, core::ptr::null(), core::ptr::null()) };
            (h, h)
        }
    };
    if frame == 0 || client == 0 {
        return crate::fail(8, 0);
    }
    let s = u();
    let mut hwnd = s.next_hwnd;
    while s.wnds.iter().any(|w| w.hwnd == hwnd) {
        hwnd = hwnd.wrapping_add(4).max(0x4004);
    }
    s.next_hwnd = hwnd.wrapping_add(4).max(0x4004);
    s.wnds.push(Box::new(Wnd {
        hwnd,
        frame,
        client,
        class: ci,
        proc_: cproc,
        unicode: cuni,
        style: style & !WS_VISIBLE,
        exstyle: c.exstyle,
        parent,
        id: if child { c.menu } else { 0 },
        hinst: c.hinst,
        user_data: 0,
        extra: alloc::vec![0; cextra as usize],
        text: c.title.clone(),
        menu: 0,
        framed,
        nc,
        data: None,
        vsb: None,
        hsb: None,
        props: Vec::new(),
        created: false,
        need_size: style & (WS_CHILD | WS_POPUP) == 0,
        moved_to: None,
        tid: tid(),
        icons: (0, 0),
        destroying: false,
    }));
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
            name.map_or(0, |n| crate::menu::load(c.hinst, &n) as u32)
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
    if style & WS_DISABLED != 0 && !child {
        unsafe { WinEnableWindow(frame, 0) };
    }
    if child && c.exstyle & WS_EX_TRANSPARENT != 0 {
        unsafe { WinSetWindowBits(frame, QWL_STYLE, crate::pm::TRANSPARENT, crate::pm::TRANSPARENT) };
    }
    // WM_NCCREATE and WM_CREATE get a CREATESTRUCT: lpCreateParams,
    // hInstance, hMenu, hwndParent, cy, cx, y, x, style, lpszName,
    // lpszClass, dwExStyle (the strings in the form they were given).
    let atom = u().classes[ci].atom as u32;
    let cs: [u32; 12] = [
        c.param,
        c.hinst,
        c.menu,
        parent as u32,
        cy as u32,
        cx as u32,
        y as u32,
        x as u32,
        style,
        if c.title_arg != 0 { c.title_arg } else { text.as_ptr() as u32 },
        if c.class_arg != 0 { c.class_arg } else { atom },
        c.exstyle,
    ];
    let wide_args = c.wide && c.title_arg != 0;
    let csp = cs.as_ptr() as u32;
    if let Some(w) = wnd(hwnd) {
        w.created = true;
    }
    let ok = deliver(hwnd, W_NCCREATE, 0, csp, wide_args) != 0 && deliver(hwnd, W_CREATE, 0, csp, wide_args) as i32 != -1;
    if !ok || wnd(hwnd).is_none() {
        destroy_window(hwnd);
        return 0;
    }
    if !wnd(hwnd).is_some_and(|w| w.need_size) {
        send_size_move(hwnd);
    }
    if child && c.exstyle & WS_EX_NOPARENTNOTIFY == 0 {
        send(parent, W_PARENTNOTIFY, (c.menu & 0xffff) << 16 | W_CREATE as u32, hwnd as u32);
    }
    if style & WS_VISIBLE != 0 && wnd(hwnd).is_some() {
        let cmd = if style & WS_MAXIMIZE != 0 { 3 } else if style & WS_MINIMIZE != 0 { 6 } else { 5 };
        show_window(hwnd, cmd);
    }
    if crate::tracing() {
        let cname: alloc::string::String = u().classes[ci].name.iter().map(|&c| c as char).collect();
        trace(format_args!("W32 CreateWindow -> {hwnd:#x} (PM {frame:#x}/{client:#x}) class {cname} parent {parent:#x} style {style:#x} {x},{y} {cx}x{cy}"));
    }
    hwnd
}

/// Position the client and the scroll bars of a plain window with a
/// non-client area in its `Win32Frame`.
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
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    let (l, t, rr, b) = (bd, bd + caption_height(nc), bd, bd);
    let (sw, sh) = (q(SV_CXVSCROLL), q(SV_CYHSCROLL));
    let mut inner = RectL { l: r.l + l, b: r.b + b, r: r.r - rr, t: r.t - t };
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

/// PM's frame procedure, which the frames of Windows windows are
/// subclassed from.
static STD_FRAME_PROC: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// The frames of Windows windows: their activation is the window's
/// (WM_ACTIVATE); the program's own system menu items are WM_SYSCOMMAND.
extern "C" fn std_frame_proc(pmh: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let old = STD_FRAME_PROC.load(core::sync::atomic::Ordering::Relaxed);
    let old: WndProc = unsafe { core::mem::transmute(old) };
    if msg == WM_SYSCOMMAND && mp1 & 0xffff < 0x8000 {
        if let Some(h) = by_frame(pmh).filter(|w| w.created).map(|w| w.hwnd) {
            let q = QMsg { hwnd: pmh, msg, mp1, mp2, ..Default::default() };
            return send_inflight(h, W_SYSCOMMAND, mp1 & 0xffff, 0, q);
        }
    }
    // WM_QUERYTRACKINFO (0x4e): a window's minimum size (WM_GETMINMAXINFO).
    let r = old(pmh, msg, mp1, mp2);
    if msg == 0x004e {
        if let Some(h) = by_frame(pmh).filter(|w| w.created).map(|w| w.hwnd) {
            min_max_info(h, mp2);
        }
    }
    if msg == WM_ACTIVATE {
        if let Some(h) = by_frame(pmh).filter(|w| w.created && w.frame != w.client).map(|w| w.hwnd) {
            let q = QMsg { hwnd: pmh, msg, mp1, mp2, ..Default::default() };
            activate(h, mp1 & 0xffff != 0, mp2, q);
        }
    }
    r
}

/// WM_GETMINMAXINFO when PM asks how far a frame may be sized
/// (TRACKINFO: cxBorder, cyBorder, cxGrid, cyGrid, cxKeyboard,
/// cyKeyboard, rclTrack, rclBoundary, ptlMinTrackSize,
/// ptlMaxTrackSize, fs).
fn min_max_info(h: u16, ti: u32) {
    if ti == 0 {
        return;
    }
    let (sx, sy) = screen_size();
    // MINMAXINFO: ptReserved, ptMaxSize, ptMaxPosition, ptMinTrackSize,
    // ptMaxTrackSize.
    let mut mmi: [i32; 10] = [0, 0, sx, sy, 0, 0, 100, 26, sx, sy];
    unsafe {
        mmi[6] = (*((ti + 56) as *const i32)).max(1);
        mmi[7] = (*((ti + 60) as *const i32)).max(1);
    }
    send(h, W_GETMINMAXINFO, 0, mmi.as_mut_ptr() as u32);
    unsafe {
        *((ti + 56) as *mut i32) = mmi[6];
        *((ti + 60) as *mut i32) = mmi[7];
        *((ti + 64) as *mut i32) = mmi[8];
        *((ti + 68) as *mut i32) = mmi[9];
    }
}

/// WM_NCACTIVATE and WM_ACTIVATE (wParam WA_ACTIVE or WA_INACTIVE, the
/// window minimized in the high word; lParam the other window).
fn activate(h: u16, active: bool, other_pm: u32, q: QMsg) {
    let other = hwnd_of(other_pm) as u32;
    send(h, W_NCACTIVATE, active as u32, 0);
    let min = if is_iconic(h) { 0x1_0000 } else { 0 };
    send_inflight(h, W_ACTIVATE, active as u32 | min, other, q);
    if active && crate::bitmap::display_bits() == 8 && wnd(h).is_some_and(|w| w.style & WS_CHILD == 0) {
        send(h, W_QUERYNEWPALETTE, 0, 0);
    }
}

/// PM's WM_REALIZEPALETTE: WM_PALETTECHANGED to every top-level window.
fn palette_changed(frame: u32) {
    let from = by_frame(frame).map_or(0, |w| w.hwnd);
    for h in top_levels() {
        if wnd(h).is_some_and(|w| w.created) {
            send(h, W_PALETTECHANGED, from as u32, 0);
        }
    }
}

/// The PM procedure of `Win32Frame`: the border of a plain window and
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
            let nc = wnd(h).map_or(0, |w| w.nc);
            let l = plain_border(nc);
            let face = gdi::rgb(gdi::sys_color(gdi::COLOR_BTNFACE));
            let shadow = gdi::rgb(gdi::sys_color(gdi::COLOR_BTNSHADOW));
            let light = gdi::rgb(gdi::sys_color(gdi::COLOR_BTNHIGHLIGHT));
            unsafe {
                let mut o = r;
                // WS_EX_CLIENTEDGE: a sunken edge (shadow above and left,
                // highlight below and right; then black and face).
                if nc & 0x8000_0000 != 0 {
                    WinFillRect(hps, &o, light);
                    WinFillRect(hps, &RectL { l: o.l, b: o.b + 1, r: o.r - 1, t: o.t }, shadow);
                    o = RectL { l: o.l + 1, b: o.b + 1, r: o.r - 1, t: o.t - 1 };
                    WinFillRect(hps, &o, face);
                    WinFillRect(hps, &RectL { l: o.l, b: o.b + 1, r: o.r - 1, t: o.t }, 0);
                    o = RectL { l: o.l + 1, b: o.b + 1, r: o.r - 1, t: o.t - 1 };
                }
                let b = l - if nc & 0x8000_0000 != 0 { 2 } else { 0 };
                if b > 0 {
                    WinFillRect(hps, &o, 0);
                    if b > 1 {
                        WinFillRect(hps, &RectL { l: o.l + 1, b: o.b + 1, r: o.r - 1, t: o.t - 1 }, face);
                    }
                }
                let cap = caption_height(nc);
                if cap > 0 {
                    let active = h != 0 && (focus() == h || is_child(h, focus()));
                    let (bg, fg) = if active { (gdi::sys_color(2), gdi::sys_color(9)) } else { (gdi::sys_color(3), gdi::sys_color(19)) };
                    let mut cr = RectL { l: r.l + l, b: r.t - l - cap, r: r.r - l, t: r.t - l };
                    WinFillRect(hps, &cr, gdi::rgb(bg));
                    cr.b += 1;
                    let t = window_text(h);
                    WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut cr, gdi::rgb(fg), 0, 0x0100 | 0x0400);
                }
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

// ---- Geometry ----

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

/// A PM window's rectangle in y-down screen coordinates.
pub fn screen_rect(pm: u32) -> [i32; 4] {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(pm, &mut r) };
    let mut pts = [PointL { x: r.l, y: r.b }, PointL { x: r.r, y: r.t }];
    unsafe { WinMapWindowPoints(pm, HWND_DESKTOP, pts.as_mut_ptr(), 2) };
    let sy = screen_size().1;
    [pts[0].x, sy - pts[1].y, pts[1].x, sy - pts[0].y]
}

/// A window's rectangle on the screen (GetWindowRect).
pub fn window_screen_rect(h: u16) -> [i32; 4] {
    wnd(h).map_or([0; 4], |w| screen_rect(w.frame))
}

/// A window's rectangle in its parent's client coordinates (the screen's
/// for top-level windows).
pub fn window_rect(h: u16) -> [i32; 4] {
    let Some((f, child, parent)) = wnd(h).map(|w| (w.frame, w.style & WS_CHILD != 0, w.parent)) else { return [0; 4] };
    let r = screen_rect(f);
    if !child {
        return r;
    }
    let a = to_client(parent, r[0], r[1]);
    let b = to_client(parent, r[2], r[3]);
    [a.0, a.1, b.0, b.1]
}

/// A point in a window's client coordinates to the screen's.
pub fn to_screen(h: u16, x: i32, y: i32) -> (i32, i32) {
    let Some(c) = wnd(h).map(|w| w.client) else { return (x, y) };
    let ch = client_height(c);
    let sy = screen_size().1;
    let mut pt = PointL { x, y: ch - 1 - y };
    unsafe { WinMapWindowPoints(c, HWND_DESKTOP, &mut pt, 1) };
    (pt.x, sy - 1 - pt.y)
}

/// A screen point in a window's client coordinates.
pub fn to_client(h: u16, x: i32, y: i32) -> (i32, i32) {
    let Some(c) = wnd(h).map(|w| w.client) else { return (x, y) };
    let ch = client_height(c);
    let sy = screen_size().1;
    let mut pt = PointL { x, y: sy - 1 - y };
    unsafe { WinMapWindowPoints(HWND_DESKTOP, c, &mut pt, 1) };
    (pt.x, ch - 1 - pt.y)
}

/// The window a mouse message goes to: `h`, or the first parent of it
/// that does not answer WM_NCHITTEST with HTTRANSPARENT (-1); `lp` in
/// that window's client coordinates.
fn hit_target(h: u16, lp: u32) -> (u16, u32) {
    let (sx, sy) = to_screen(h, lp as i16 as i32, (lp >> 16) as i16 as i32);
    let mut t = enabled_ancestor(h).unwrap_or(h);
    for _ in 0..16 {
        let at = (sy as u16 as u32) << 16 | sx as u16 as u32;
        if send(t, W_NCHITTEST, 0, at) as i32 != -1 {
            break;
        }
        match wnd(t).filter(|w| w.style & WS_CHILD != 0).map(|w| w.parent) {
            Some(p) if p != 0 => t = p,
            _ => break,
        }
    }
    if t == h {
        return (h, lp);
    }
    let (x, y) = to_client(t, sx, sy);
    (t, (y as u16 as u32) << 16 | x as u16 as u32)
}

/// WM_PARENTNOTIFY for a button pressed in a child window: each parent up
/// the chain hears of it, unless the child has WS_EX_NOPARENTNOTIFY.
fn parent_notify_click(h: u16, m: u16, lp: u32) {
    let (sx, sy) = to_screen(h, lp as i16 as i32, (lp >> 16) as i16 as i32);
    let mut c = h;
    for _ in 0..16 {
        let Some((style, ex, parent)) = wnd(c).map(|w| (w.style, w.exstyle, w.parent)) else { break };
        if style & WS_CHILD == 0 || ex & WS_EX_NOPARENTNOTIFY != 0 || parent == 0 {
            break;
        }
        let (x, y) = to_client(parent, sx, sy);
        send(parent, W_PARENTNOTIFY, m as u32, (y as u16 as u32) << 16 | x as u16 as u32);
        c = parent;
    }
}

pub fn is_iconic(h: u16) -> bool {
    frame_state(h) & SWP_MINIMIZE != 0
}

pub fn is_zoomed(h: u16) -> bool {
    frame_state(h) & SWP_MAXIMIZE != 0
}

fn frame_state(h: u16) -> u32 {
    let Some(f) = wnd(h).map(|w| w.frame) else { return 0 };
    let mut swp = Swp::default();
    unsafe { WinQueryWindowPos(f, &mut swp) };
    swp.fl
}

/// WM_SIZE and WM_MOVE with the window's present size and place.
fn send_size_move(h: u16) {
    let Some(w) = wnd(h) else { return };
    w.need_size = false;
    let (cw, ch) = client_size(h);
    send(h, W_SIZE, 0, (ch as u32) << 16 | cw as u32 & 0xffff);
    let o = client_origin(h);
    if let Some(w) = wnd(h) {
        w.moved_to = Some(o);
    }
    send(h, W_MOVE, 0, o);
}

pub fn show_window(h: u16, cmd: u32) -> bool {
    let Some((f, child, framed)) = wnd(h).map(|w| (w.frame, w.style & WS_CHILD != 0, w.framed)) else { return false };
    let was = wnd(h).is_some_and(|w| w.style & WS_VISIBLE != 0);
    let mut fl = match cmd {
        0 => SWP_HIDE,
        2 | 6 | 7 | 11 => SWP_SHOW | SWP_MINIMIZE,
        3 => SWP_SHOW | SWP_MAXIMIZE | SWP_ACTIVATE | SWP_ZORDER,
        4 | 8 => SWP_SHOW,
        9 => SWP_SHOW | SWP_RESTORE | SWP_ACTIVATE | SWP_ZORDER,
        _ => SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER,
    };
    if !framed {
        fl &= !(SWP_ACTIVATE | SWP_MINIMIZE | SWP_MAXIMIZE | SWP_RESTORE);
        if child {
            fl &= !SWP_ZORDER;
        } else if fl & SWP_SHOW != 0 {
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
    if cmd != 0 && wnd(h).is_some_and(|w| w.need_size) {
        send_size_move(h);
    }
    if fl & SWP_ACTIVATE != 0 && !is_child(h, focus()) {
        if let Some(c) = wnd(h).map(|w| w.client) {
            unsafe { WinSetFocus(HWND_DESKTOP, c) };
        }
    }
    was
}

/// Move and size a window (Windows coordinates: the parent's client
/// area, or the screen).
pub fn move_window(h: u16, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> bool {
    let Some((f, parent)) = wnd(h).map(|w| (w.frame, w.parent)) else { return false };
    let child = is_child_style(h);
    let ph = wnd(parent).filter(|_| child).map_or(screen_size().1, |p| client_height(p.client));
    let (mut cx, mut cy) = (cx.max(0), cy.max(0));
    let mut swp = Swp::default();
    unsafe { WinQueryWindowPos(f, &mut swp) };
    if fl & SWP_SIZE == 0 {
        cx = swp.cx;
        cy = swp.cy;
    }
    // Sized without moving: Windows keeps the top-left corner where it
    // is, PM the bottom-left.
    let (fl, x, py) = if fl & (SWP_SIZE | SWP_MOVE) == SWP_SIZE && cy != swp.cy {
        (fl | SWP_MOVE, swp.x, swp.y + swp.cy - cy)
    } else {
        (fl, x, ph - y - cy)
    };
    let ok = unsafe { WinSetWindowPos(f, HWND_TOP, x, py, cx, cy, fl) != 0 };
    if fl & SWP_SIZE != 0 {
        layout_frame(h);
    }
    ok
}

/// SetWindowPos(hwnd, after, x, y, cx, cy, flags): SWP_NOSIZE 1,
/// SWP_NOMOVE 2, SWP_NOZORDER 4, SWP_NOACTIVATE 0x10, SWP_FRAMECHANGED
/// 0x20, SWP_SHOWWINDOW 0x40, SWP_HIDEWINDOW 0x80.
pub fn set_window_pos(h: u16, after: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> bool {
    let mut f = 0;
    if fl & 1 == 0 {
        f |= SWP_SIZE;
    }
    if fl & 2 == 0 {
        f |= SWP_MOVE;
    }
    if fl & 0x14 == 0 && !is_child_style(h) {
        f |= SWP_ACTIVATE;
    }
    let ok = if f & (SWP_SIZE | SWP_MOVE) != 0 { move_window(h, x, y, cx, cy, f & (SWP_SIZE | SWP_MOVE)) } else { wnd(h).is_some() };
    if fl & 0x40 != 0 {
        show_window(h, if fl & 0x10 != 0 { 8 } else { 5 });
    }
    if fl & 0x80 != 0 {
        show_window(h, 0);
    }
    // The Z order: HWND_TOP 0, HWND_BOTTOM 1, HWND_TOPMOST -1,
    // HWND_NOTOPMOST -2, or behind a window.
    if fl & 4 == 0 {
        if let Some(fr) = wnd(h).map(|w| w.frame) {
            let behind = match after {
                0 | 0xffff_ffff | 0xffff_fffe => HWND_TOP,
                1 => HWND_BOTTOM,
                a => wnd(hw(a)).map_or(HWND_TOP, |w| w.frame),
            };
            unsafe { WinSetWindowPos(fr, behind, 0, 0, 0, 0, SWP_ZORDER) };
        }
    }
    if f & SWP_ACTIVATE != 0 && fl & 0x80 == 0 {
        if let Some(fr) = wnd(h).filter(|w| w.style & WS_VISIBLE != 0).map(|w| w.frame) {
            unsafe { WinSetActiveWindow(HWND_DESKTOP, fr) };
        }
    }
    ok
}

/// Every window below a window.
pub fn descendants(h: u16) -> Vec<u16> {
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
    for o in owned_popups(h) {
        destroy_window(o);
    }
    if wnd(h).is_none() {
        return true;
    }
    crate::caret::window_gone(h);
    // The window hears WM_DESTROY before its children (PM's destruction
    // tells them; a frame's client hears nothing from PM), WM_NCDESTROY
    // after them.
    if let Some(w) = wnd(h) {
        w.destroying = true;
    }
    send(h, W_DESTROY, 0, 0);
    unsafe { WinDestroyWindow(f) };
    if wnd(h).is_some() {
        send(h, W_NCDESTROY, 0, 0);
    }
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
    if w.style & WS_CHILD == 0 {
        unsafe {
            WinEnableWindow(f, on as u32);
            if c != f {
                WinEnableWindow(c, on as u32);
            }
        }
    }
    if !on && (focus() == h || is_child(h, focus())) {
        unsafe { WinSetFocus(HWND_DESKTOP, 0) };
    }
    send(h, W_ENABLE, on as u32, 0);
    was
}

pub fn focus() -> u16 {
    hwnd_of(unsafe { WinQueryFocus(HWND_DESKTOP) })
}

/// SetFocus: the previous focus window.
pub fn set_focus(h: u16) -> u16 {
    let old = focus();
    let mut t = h;
    for _ in 0..64 {
        let Some((style, parent)) = wnd(t).map(|w| (w.style, w.parent)) else { break };
        if style & WS_DISABLED != 0 {
            return 0;
        }
        if style & WS_CHILD == 0 || parent == 0 {
            break;
        }
        t = parent;
    }
    if h == 0 {
        unsafe { WinSetFocus(HWND_DESKTOP, 0) };
    } else if old != h {
        if let Some(c) = wnd(h).map(|w| w.client) {
            unsafe { WinSetFocus(HWND_DESKTOP, c) };
        }
    }
    old
}

pub fn set_capture(h: u16) -> u16 {
    let old = hwnd_above(unsafe { WinQueryCapture(HWND_DESKTOP) });
    if let Some(c) = wnd(h).map(|w| w.client) {
        unsafe { WinSetCapture(HWND_DESKTOP, c) };
    } else if h == 0 {
        unsafe { WinSetCapture(HWND_DESKTOP, 0) };
    }
    old
}

/// Is `h` below `p` (a child, grandchild...)?
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

pub fn find_window(class: Option<usize>, class_given: bool, title: Option<&[u8]>, parent: Option<u16>, after: u16) -> u16 {
    if class_given && class.is_none() {
        return 0;
    }
    let list: Vec<u16> = match parent {
        Some(p) => children(p),
        None => top_levels(),
    };
    let start = if after != 0 { list.iter().position(|&x| x == after).map_or(list.len(), |i| i + 1) } else { 0 };
    for &h in &list[start.min(list.len())..] {
        let Some(w) = wnd(h) else { continue };
        if class.is_some_and(|ci| ci != w.class) {
            continue;
        }
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
        let mut b = [0u8; 512];
        let n = unsafe { WinQueryWindowText(w.frame, 512, b.as_mut_ptr()) };
        return b[..n.max(0) as usize].to_vec();
    }
    w.text.clone()
}

/// Set a window's text without telling the window.
pub fn set_text_raw(h: u16, t: &[u8]) {
    let Some(w) = wnd(h) else { return };
    if w.framed {
        let s = z(t);
        unsafe { WinSetWindowText(w.frame, s.as_ptr()) };
    }
    w.text = t.to_vec();
}

/// GetWindow(hwnd, GW_*).
pub fn get_window(h: u16, cmd: u32) -> u16 {
    let siblings = |h: u16| -> Vec<u16> {
        match wnd(h) {
            Some(w) if w.style & WS_CHILD != 0 => children(w.parent),
            _ => top_levels(),
        }
    };
    match cmd {
        0 => siblings(h).first().copied().unwrap_or(0),
        1 => siblings(h).last().copied().unwrap_or(0),
        2 | 3 => {
            let s = siblings(h);
            let Some(i) = s.iter().position(|&x| x == h) else { return 0 };
            if cmd == 2 { s.get(i + 1).copied().unwrap_or(0) } else if i > 0 { s[i - 1] } else { 0 }
        }
        4 => wnd(h).filter(|w| w.style & WS_CHILD == 0).map_or(0, |w| w.parent),
        5 => children(h).first().copied().unwrap_or(0),
        // GW_ENABLEDPOPUP
        6 => owned_popups(h).into_iter().find(|&p| wnd(p).is_some_and(|w| w.style & (WS_VISIBLE | WS_DISABLED) == WS_VISIBLE)).unwrap_or(0),
        _ => 0,
    }
}

/// ChildWindowFromPoint(Ex): the child at a client point (CWP_SKIPINVISIBLE
/// 1, CWP_SKIPDISABLED 2, CWP_SKIPTRANSPARENT 4), else the window.
pub fn child_from_point(h: u16, x: i32, y: i32, skip: u32) -> u16 {
    let (cx, cy) = client_size(h);
    if x < 0 || y < 0 || x >= cx || y >= cy {
        return 0;
    }
    for c in children(h) {
        let Some(w) = wnd(c) else { continue };
        if skip & 1 != 0 && w.style & WS_VISIBLE == 0 || skip & 2 != 0 && w.style & WS_DISABLED != 0 || skip & 4 != 0 && w.exstyle & WS_EX_TRANSPARENT != 0 {
            continue;
        }
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

/// ScrollWindow(Ex): the client area's bits move; what is uncovered is
/// invalidated (and the child windows move with SW_SCROLLCHILDREN or
/// when the whole area does).
pub fn scroll_window(h: u16, dx: i32, dy: i32, r: Option<[i32; 4]>, clip: Option<[i32; 4]>, children: bool) {
    let Some(c) = wnd(h).map(|w| w.client) else { return };
    let ch = client_height(c);
    let conv = |r: [i32; 4]| RectL { l: r[0], b: ch - r[3], r: r[2], t: ch - r[1] };
    let (rs, cs) = (r.map(conv), clip.map(conv));
    let fl = if r.is_none() || children { 1 | 2 } else { 2 };
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

/// GetDC: a DC on the window's client area (0: the screen).
pub fn get_dc(h: u16) -> u16 {
    let pm = if h == 0 { HWND_DESKTOP } else { wnd(h).map_or(0, |w| w.client) };
    if pm == 0 {
        return 0;
    }
    let hps = unsafe { WinGetPS(pm) };
    gdi::new_dc(hps, pm, client_height(pm), gdi::DcKind::Window)
}

/// SetCursor: the previous cursor.
pub fn set_cursor(c: u32) -> u32 {
    let prev = core::mem::replace(CURSOR.get(), c);
    if crate::icon::get(c).is_some() {
        let p = crate::icon::pm_pointer(c);
        if p != 0 {
            unsafe { WinSetPointer(HWND_DESKTOP, p) };
            return prev;
        }
    }
    let sptr = match c {
        0 => 0,
        _ => crate::icon::system_pointer(c),
    };
    unsafe { WinSetPointer(HWND_DESKTOP, if sptr != 0 { WinQuerySysPointer(HWND_DESKTOP, sptr as i32, 0) } else { 0 }) };
    prev
}

pub fn cursor() -> u32 {
    *CURSOR.get()
}

static CURSOR: Global<u32> = Global::new(32512);

// ---- Timers ----

/// SetTimer: the timer's ID (a window's: the ID given).
pub fn set_timer(h: u16, id: u32, ms: u32, proc_: u32) -> u32 {
    let pm = if h == 0 {
        0
    } else {
        match wnd(h) {
            Some(w) => w.client,
            None => return 0,
        }
    };
    let s = u();
    let t = tid();
    let id = if h == 0 {
        if id != 0 && s.timers.iter().any(|x| x.hwnd == 0 && x.id == id) {
            id
        } else {
            s.next_timer += 1;
            s.next_timer
        }
    } else {
        id
    };
    // PM's timer IDs: the window's, or one of the thread's.
    let pm_id = if h == 0 { id & 0x7fff } else { id & 0x7fff };
    if unsafe { WinStartTimer(queue().hab, pm, pm_id, ms.max(1)) } == 0 {
        return 0;
    }
    s.timers.retain(|x| !(x.hwnd == h && x.id == id));
    s.timers.push(Timer { hwnd: h, id, pm_id, proc_, tid: t });
    if h == 0 { id } else { id.max(1) }
}

pub fn kill_timer(h: u16, id: u32) -> bool {
    let Some(i) = u().timers.iter().position(|t| t.hwnd == h && t.id == id) else { return false };
    let t = u().timers.remove(i);
    let pm = wnd(h).map_or(0, |w| w.client);
    unsafe { WinStopTimer(queue().hab, pm, t.pm_id) };
    true
}

// ---- Posting and the message loop ----

/// PostMessage: to a window's queue (0: the thread's).
pub fn post(h: u16, m: u16, wp: u32, lp: u32) -> bool {
    if h == 0xffff {
        for t in top_levels() {
            post(t, m, wp, lp);
        }
        return true;
    }
    let pm = match wnd(h) {
        Some(w) => w.client,
        None if h == 0 => return unsafe { WinPostQueueMsg(queue().hmq, POSTED | m as u32, wp, lp) } != 0,
        None => return crate::fail(1400, false), // ERROR_INVALID_WINDOW_HANDLE
    };
    unsafe { WinPostMsg(pm, POSTED | m as u32, wp, lp) != 0 }
}

/// PostThreadMessage.
pub fn post_thread(t: u32, m: u16, wp: u32, lp: u32) -> bool {
    match crate::queue_of(t) {
        Some(q) if q.hmq != 0 => unsafe { WinPostQueueMsg(q.hmq, POSTED | m as u32, wp, lp) != 0 },
        _ => crate::fail(1444, false), // ERROR_INVALID_THREAD_ID
    }
}

/// Windows key state bits for a mouse message's wParam.
fn mouse_keys(msg: u32) -> u32 {
    let down = |vk| unsafe { WinGetKeyState(HWND_DESKTOP, vk) } & 0x8000 != 0;
    let mut k = 0;
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
        0x1d => 0x90,                         // NUMLOCK
        0x1c => 0x91,                         // SCRLLOCK
        0x20..=0x2b => 0x70 + (pm_vk - 0x20), // F1-F12
        _ => {
            let c = (ch as u8).to_ascii_uppercase();
            if c.is_ascii_alphanumeric() { c as u16 } else { 0 }
        }
    }
}

/// The Windows virtual key of a character key's set-1 scan code.
fn scan_vkey(scan: u8) -> Option<u16> {
    let c = scan_letter(scan);
    if c != 0 {
        return Some(c.to_ascii_uppercase() as u16);
    }
    Some(match scan {
        0x27 => 0xba,
        0x0d => 0xbb,
        0x33 => 0xbc,
        0x0c => 0xbd,
        0x34 => 0xbe,
        0x35 => 0xbf,
        0x29 => 0xc0,
        0x1a => 0xdb,
        0x2b => 0xdc,
        0x1b => 0xdd,
        0x28 => 0xde,
        0x37 => 0x6a,
        0x4a => 0x6d,
        0x4e => 0x6b,
        _ => return None,
    })
}

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
        0xa0 | 0xa1 => 0x09,
        0xa2 | 0xa3 => 0x0a,
        0xa4 | 0xa5 => 0x0b,
        _ => match VKEYS.iter().find(|v| v.1 == vk) {
            Some(&(p, _)) => p,
            None => return 0,
        },
    };
    let s = unsafe { WinGetKeyState(HWND_DESKTOP, pm as i32) };
    (if s & 0x8000 != 0 { 0x8000 } else { 0 }) | (s & 1) as u16
}

/// MessageBeep(0).
pub fn call_beep() {
    unsafe { WinAlarm(HWND_DESKTOP, 2) };
}

/// Move and size a window to a rectangle of its parent's (or the
/// screen's) coordinates.
pub fn move_to(h: u16, r: [i32; 4]) {
    move_window(h, r[0], r[1], r[2] - r[0], r[3] - r[1], SWP_MOVE | SWP_SIZE);
}

pub fn key_down(vk: u16) -> bool {
    key_state(vk) & 0x8000 != 0
}

/// Translate a PM message for a Windows window into the Windows one:
/// (msg, wParam, lParam).
fn translate_pm(w_hwnd: u16, q: &QMsg) -> Option<(u16, u32, u32)> {
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
            (W_TIMER, t.map_or(q.mp1 & 0xffff, |t| t.id), t.map_or(0, |t| t.proc_))
        }
        WM_MOUSEMOVE => (W_MOUSEMOVE, mouse_keys(q.msg), point(q.mp1)),
        0x71..=0x79 => {
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
            if (alt || ctrl) && fs & 0x0003 == 0 {
                let c = scan_letter(scan as u8);
                if c != 0 {
                    chr = c as u16;
                    fs |= 0x0001;
                }
            }
            let vk = if fs & 0x0002 != 0 { vkey(pvk, chr) } else { scan_vkey(scan as u8).unwrap_or_else(|| vkey(0, chr)) };
            if vk == 0 && fs & 0x0001 == 0 {
                return None;
            }
            let sys = alt || vk == 0x79;
            // Bit 24: an extended key (the cursor block).
            let ext = if fs & 0x0002 != 0 && (0x11..=0x1b).contains(&pvk) { 1 << 24 } else { 0 };
            let lp = repeat.max(1) | scan << 16 | ext | if alt { 1 << 29 } else { 0 } | if up { 0xc000_0000 } else { 0 };
            let m = match (up, sys) {
                (false, false) => W_KEYDOWN,
                (true, false) => W_KEYUP,
                (false, true) => W_SYSKEYDOWN,
                (true, true) => W_SYSKEYUP,
            };
            let wp = if vk != 0 { vk } else { chr & 0xff } as u32;
            if !up && fs & 0x0001 != 0 && chr != 0 {
                let c = chr & 0xff;
                let c = if ctrl && !alt && (c as u8).is_ascii_alphabetic() { c & 0x1f } else { c };
                let cs = &mut u().chars;
                if !cs.iter().any(|x| x.0 == q.time && x.1 == w_hwnd && x.3 == lp) {
                    if cs.len() >= 32 {
                        cs.remove(0);
                    }
                    cs.push((q.time, w_hwnd, wp, lp, if alt { W_SYSCHAR } else { W_CHAR }, c as u32));
                }
            }
            (m, wp, lp)
        }
        m if m & 0xffff_0000 == POSTED => ((m & 0xffff) as u16, q.mp1, q.mp2),
        _ => return None,
    })
}

/// The oldest PM queue message that is a Windows message of the range
/// (taken off the queue if `remove`).
fn peek_range(hab: u32, filter: u32, lo: u32, hi: u32, remove: bool) -> Option<QMsg> {
    let has = |a: u32, b: u32| lo <= b && a <= hi;
    let mut ranges: Vec<(u32, u32)> = alloc::vec![(POSTED | lo.min(0xffff), POSTED | hi.min(0xffff))];
    if has(0x100, 0x109) {
        ranges.push((WM_CHAR, WM_CHAR));
    }
    if has(0x200, 0x20d) {
        ranges.push((WM_MOUSEMOVE, 0x79));
    }
    if has(0x0f, 0x0f) {
        ranges.push((WM_PAINT, WM_PAINT));
    }
    if has(0x113, 0x113) {
        ranges.push((WM_TIMER, WM_TIMER));
    }
    let mut best: Option<(QMsg, (u32, u32))> = None;
    for r in ranges {
        let mut q = QMsg::default();
        if unsafe { WinPeekMsg(hab, &mut q, filter, r.0, r.1, PM_NOREMOVE) } == 0 {
            continue;
        }
        let Some((_, m, _, _)) = win_message(&q) else { continue };
        if (m as u32) < lo || m as u32 > hi {
            continue;
        }
        if best.as_ref().is_none_or(|b| q.time < b.0.time) {
            best = Some((q, r));
        }
    }
    let (q, r) = best?;
    if remove {
        let mut q2 = QMsg::default();
        unsafe { WinPeekMsg(hab, &mut q2, filter, r.0, r.1, PM_REMOVE) };
        return Some(q2);
    }
    Some(q)
}

/// A MSG: hwnd, message, wParam, lParam, time, pt.
#[derive(Clone, Copy, Default)]
pub struct Msg {
    pub hwnd: u32,
    pub msg: u32,
    pub wp: u32,
    pub lp: u32,
    pub time: u32,
    pub x: i32,
    pub y: i32,
}

impl Msg {
    pub fn write(&self, p: u32) {
        if p == 0 {
            return;
        }
        let v = [self.hwnd, self.msg, self.wp, self.lp, self.time, self.x as u32, self.y as u32];
        for (i, x) in v.iter().enumerate() {
            crate::wr32(p, 4 * i as u32, *x);
        }
    }

    pub fn read(p: u32) -> Option<Msg> {
        if p == 0 {
            return None;
        }
        let r = |i: u32| crate::rd32(p, 4 * i);
        Some(Msg { hwnd: r(0), msg: r(1), wp: r(2), lp: r(3), time: r(4), x: r(5) as i32, y: r(6) as i32 })
    }
}

/// GetMessage / PeekMessage (flags given): the next message for the
/// thread's windows (and the thread); PM's other messages are
/// dispatched on the way. The range applies to both (0, 0: all).
pub fn get_message(h: u16, lo: u32, hi: u32, peek: Option<u32>) -> Option<Msg> {
    collect();
    let hab = queue().hab;
    let filter = if h == 0 { 0 } else { wnd(h).map_or(0, |w| w.client) };
    let ranged = lo != 0 || hi != 0;
    loop {
        let mut q = QMsg::default();
        match peek {
            None if !ranged => {
                unsafe { WinGetMsg(hab, &mut q, filter, 0, 0) };
            }
            None => match peek_range(hab, filter, lo, hi.max(lo), true) {
                Some(m) => q = m,
                None => {
                    // Wait for anything, then look again.
                    unsafe { WinPeekMsg(hab, &mut q, 0, 0, 0, PM_NOREMOVE) };
                    let mut w = QMsg::default();
                    if unsafe { WinPeekMsg(hab, &mut w, 0, 0, 0, PM_NOREMOVE) } == 0 {
                        unsafe { os2::DosSleep(10) };
                    }
                    continue;
                }
            },
            Some(fl) if ranged => match peek_range(hab, filter, lo, hi.max(lo), fl & 1 != 0) {
                Some(m) => q = m,
                None => return None,
            },
            Some(fl) => {
                if unsafe { WinPeekMsg(hab, &mut q, filter, 0, 0, PM_NOREMOVE) } == 0 {
                    return None;
                }
                let ours = q.msg == WM_QUIT || win_message(&q).is_some();
                if !ours || fl & 1 != 0 {
                    unsafe { WinPeekMsg(hab, &mut q, filter, 0, 0, PM_REMOVE) };
                }
            }
        }
        if q.msg == WM_QUIT {
            let sy = screen_size().1;
            return Some(Msg { hwnd: 0, msg: W_QUIT as u32, wp: q.mp1, lp: 0, time: q.time, x: q.x, y: sy - 1 - q.y });
        }
        let Some((mut hwnd, msg, wp, mut lp)) = win_message(&q) else {
            if q.msg == WM_REALIZEPALETTE && q.hwnd == 0 {
                palette_changed(q.mp1);
                continue;
            }
            unsafe { WinDispatchMsg(hab, &q) };
            continue;
        };
        if (0x200..=0x20d).contains(&msg) {
            (hwnd, lp) = hit_target(hwnd, lp);
        }
        if msg == W_MOUSEMOVE || (0x201..=0x209).contains(&msg) {
            send(hwnd, W_SETCURSOR, hwnd as u32, (msg as u32) << 16 | 1);
        }
        if matches!(msg, W_LBUTTONDOWN | W_RBUTTONDOWN | W_MBUTTONDOWN) && peek.is_none_or(|fl| fl & 1 != 0) {
            parent_notify_click(hwnd, msg, lp);
        }
        let sy = screen_size().1;
        let m = Msg { hwnd: hwnd as u32, msg: msg as u32, wp, lp, time: q.time, x: q.x, y: sy - 1 - q.y };
        u().msg_time = q.time;
        u().msg_pos = (m.y as u32 & 0xffff) << 16 | m.x as u32 & 0xffff;
        u().last = Some((hwnd, msg, q));
        return Some(m);
    }
}

pub fn message_time() -> u32 {
    u().msg_time
}

pub fn message_pos() -> u32 {
    u().msg_pos
}

/// WaitMessage: until a message is in the queue.
pub fn wait_message() {
    let hab = queue().hab;
    let mut q = QMsg::default();
    while unsafe { WinPeekMsg(hab, &mut q, 0, 0, 0, PM_NOREMOVE) } == 0 {
        unsafe { os2::DosSleep(10) };
    }
}

/// The Windows message (hwnd, msg, wParam, lParam) of a PM queue
/// message, if it is one for the thread.
fn win_message(q: &QMsg) -> Option<(u16, u16, u32, u32)> {
    let h = by_client(q.hwnd).map_or(0, |w| w.hwnd);
    if h != 0 {
        return translate_pm(h, q).map(|(m, wp, lp)| (h, m, wp, lp));
    }
    if q.msg & 0xffff_0000 == POSTED && (q.hwnd == 0 || hwnd_of(q.hwnd) == 0) {
        return Some((0, q.msg as u16, q.mp1, q.mp2));
    }
    if q.msg == WM_TIMER && q.hwnd == 0 {
        let t = u().timers.iter().find(|t| t.hwnd == 0 && t.pm_id == q.mp1 & 0xffff)?;
        return Some((0, W_TIMER, t.id, t.proc_));
    }
    None
}

/// TranslateMessage: WM_CHAR (WM_SYSCHAR) for a key with a character.
pub fn translate(m: &Msg) -> bool {
    let (h, msg) = (hw(m.hwnd), m.msg as u16);
    if msg != W_KEYDOWN && msg != W_SYSKEYDOWN {
        return false;
    }
    let cs = &mut u().chars;
    let Some(i) = cs.iter().position(|x| x.1 == h && x.2 == m.wp && x.3 == m.lp) else { return false };
    let (_, _, _, _, cm, c) = cs.remove(i);
    let pm = wnd(h).map_or(0, |w| w.client);
    unsafe { WinPostMsg(pm, POSTED | cm as u32, c, m.lp) != 0 }
}

/// DispatchMessage(A or W).
pub fn dispatch(m: &Msg, wide: bool) -> u32 {
    let (h, msg) = (hw(m.hwnd), m.msg as u16);
    // A timer's procedure gets WM_TIMER itself.
    if msg == W_TIMER && m.lp != 0 {
        let t = unsafe { WinGetCurrentTime(queue().hab) };
        return call4(m.lp, h as u32, W_TIMER as u32, m.wp, t);
    }
    if wnd(h).is_none() {
        return 0;
    }
    let last = u().last.filter(|&(lh, lm, _)| lh == h && lm == msg);
    match last {
        Some((_, _, q)) => {
            u().inflight.push(InFlight { hwnd: h, msg, q });
            let r = deliver(h, msg, m.wp, m.lp, wide);
            u().inflight.pop();
            r
        }
        None => deliver(h, msg, m.wp, m.lp, wide),
    }
}

/// The window that gets a disabled child window's mouse input: the
/// nearest enabled one it is in; None when a top-level window is
/// disabled.
fn enabled_ancestor(h: u16) -> Option<u16> {
    let mut cur = h;
    let mut best = h;
    let mut chain_ok = true;
    for _ in 0..32 {
        let w = wnd(cur)?;
        if w.style & WS_DISABLED != 0 {
            chain_ok = false;
        } else if !chain_ok {
            best = cur;
            chain_ok = true;
        }
        if w.style & WS_CHILD == 0 || w.parent == 0 {
            break;
        }
        cur = w.parent;
    }
    chain_ok.then_some(best)
}

/// A client area that changed height: its children keep their distance
/// from its top, as on Windows (PM keeps them where they are from the
/// bottom).
fn keep_children_at_top(h: u16, old: i32, new: i32) {
    let dy = new - old;
    if dy == 0 {
        return;
    }
    for c in children(h) {
        let Some(f) = wnd(c).map(|w| w.frame) else { continue };
        let mut swp = Swp::default();
        unsafe {
            WinQueryWindowPos(f, &mut swp);
            WinSetWindowPos(f, HWND_TOP, swp.x, swp.y + dy, 0, 0, SWP_MOVE);
        }
    }
}

/// PM's window procedure of the Windows windows' client areas.
extern "C" fn pm_wndproc(pmh: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(h) = by_client(pmh).filter(|w| w.created).map(|w| w.hwnd) else {
        return unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) };
    };
    // A message another thread sent.
    if msg & 0xffff_0000 == XSEND_A || msg & 0xffff_0000 == XSEND_W {
        let Some((p, uni)) = wnd(h).map(|w| (w.proc_, w.unicode)) else { return 0 };
        return call_conv(p, uni, h, msg as u16, mp1, mp2, msg & 0xffff_0000 == XSEND_W);
    }
    let q = QMsg { hwnd: pmh, msg, mp1, mp2, ..Default::default() };
    if (0x70..=0x79).contains(&msg) {
        if let Some(to) = enabled_ancestor(h).filter(|&a| a != h) {
            let Some(pm_to) = wnd(to).map(|w| w.client) else { return 0 };
            let mut p = PointL { x: mp1 as i16 as i32, y: (mp1 >> 16) as i16 as i32 };
            unsafe {
                WinMapWindowPoints(pmh, pm_to, &mut p, 1);
                return WinSendMsg(pm_to, msg, (p.y as u32 & 0xffff) << 16 | p.x as u32 & 0xffff, mp2);
            }
        }
    }
    match msg {
        // PM's accelerators make F1 WM_HELP; Windows programs get the key.
        0x004b if mp1 != 0 => {
            let m = unsafe { core::slice::from_raw_parts(mp1 as *const u32, 4) };
            if m[1] == 0x7a && m[2] & 0x0002 != 0 && (m[3] >> 16) & 0xffff == 0x20 {
                return 0;
            }
            unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) }
        }
        WM_SIZE => {
            let (cx, cy) = (mp2 as u16, (mp2 >> 16) as u16);
            keep_children_at_top(h, (mp1 >> 16) as u16 as i32, cy as i32);
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
            let o = client_origin(h);
            if wnd(h).is_some_and(|w| w.moved_to != Some(o)) {
                if let Some(w) = wnd(h) {
                    w.moved_to = Some(o);
                }
                send_inflight(h, W_MOVE, 0, o, q);
            }
            0
        }
        WM_ACTIVATE => {
            activate(h, mp1 & 0xffff != 0, mp2, q);
            0
        }
        WM_SETFOCUS => {
            let other = hwnd_of(mp1) as u32;
            if mp2 & 0xffff != 0 {
                send_inflight(h, W_SETFOCUS, other, 0, q);
            } else {
                send_inflight(h, W_KILLFOCUS, other, 0, q);
            }
            0
        }
        WM_CLOSE => {
            send_inflight(h, W_CLOSE, 0, 0, q);
            0
        }
        // A child window of one being destroyed (DestroyWindow tells the
        // window itself).
        WM_DESTROY => {
            if !wnd(h).is_some_and(|w| w.destroying) {
                send_inflight(h, W_DESTROY, 0, 0, q);
                send(h, W_NCDESTROY, 0, 0);
                forget(h);
            }
            unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) }
        }
        WM_ENABLE => 0,
        // BeginPaint erases (WM_ERASEBKGND).
        WM_ERASEBACKGROUND => 0,
        WM_COMMAND | WM_INITMENU | WM_MENUSELECT | WM_MENUEND => crate::menu::pm_message(h, msg, mp1, mp2),
        0x0036 | 0x0037 => match crate::menu::owner_item(h, msg, mp2 as *mut u8) {
            Some(r) => r,
            None => unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) },
        },
        WM_VSCROLL | WM_HSCROLL | WM_CONTROL => 0,
        _ => match translate_pm(h, &q) {
            Some((m, wp, lp)) => {
                let r = send_inflight(h, m, wp, lp, q);
                if (0x70..=0x7a).contains(&msg) { 1 } else { r }
            }
            None => unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) },
        },
    }
}

/// DefWindowProc (A form): PM's default for a translated PM message,
/// Windows' for the rest.
pub fn def_window_proc(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    let inflight = u().inflight.iter().rev().find(|f| f.hwnd == h && f.msg == m).copied();
    match m {
        W_NCHITTEST => return 1,
        W_NCCREATE => {
            // The window text, from CREATESTRUCT's lpszName.
            let name = crate::rd32(lp, 36);
            if let (Some(w), Some(t)) = (wnd(h), far_str(name)) {
                if !t.starts_with(&[0xff]) {
                    w.text = t.to_vec();
                }
            }
            return 1;
        }
        W_NCCALCSIZE | W_NCPAINT => return 0,
        W_NCACTIVATE => return 1,
        W_CLOSE => {
            destroy_window(h);
            return 0;
        }
        W_PAINT => {
            let hdc = begin_paint(h, 0);
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
            gdi::fill_rect(wp as u16, [0, 0, cx, cy], br as u16);
            return 1;
        }
        W_PRINTCLIENT => return 0,
        W_CTLCOLORMSGBOX..=W_CTLCOLORSTATIC => {
            let hdc = wp as u16;
            let (bk, br) = match m {
                W_CTLCOLORSCROLLBAR => (gdi::COLOR_SCROLLBAR, gdi::COLOR_SCROLLBAR),
                W_CTLCOLOREDIT | W_CTLCOLORLISTBOX => (gdi::COLOR_WINDOW, gdi::COLOR_WINDOW),
                _ => (gdi::COLOR_BTNFACE, gdi::COLOR_BTNFACE),
            };
            gdi::set_text_color(hdc, gdi::sys_color(if m == W_CTLCOLOREDIT || m == W_CTLCOLORLISTBOX { gdi::COLOR_WINDOWTEXT } else { gdi::COLOR_BTNTEXT }));
            gdi::set_bk_color(hdc, gdi::sys_color(bk));
            return gdi::sys_brush(br) as u32;
        }
        W_SETCURSOR => {
            if lp & 0xffff == 1 {
                let cur = wnd(h).and_then(|w| u().classes.get(w.class)).map_or(32512, |c| c.cursor);
                if cur != 0 {
                    set_cursor(cur);
                    return 1;
                }
            }
            // A child window: the parent may set it.
            if let Some(p) = wnd(h).filter(|w| w.style & WS_CHILD != 0).map(|w| w.parent) {
                return send(p, W_SETCURSOR, wp, lp);
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
            if w.nc & WS_CAPTION == WS_CAPTION && !w.framed {
                unsafe { WinInvalidateRect(w.frame, core::ptr::null(), 0) };
            }
            return 1;
        }
        W_GETTEXT => {
            let buf = lp as *mut u8;
            if buf.is_null() || wp == 0 {
                return 0;
            }
            let t = window_text(h);
            return crate::copy_out(&t, buf, wp as i32) as u32;
        }
        W_GETTEXTLENGTH => return window_text(h).len() as u32,
        W_SETICON => {
            let Some(w) = wnd(h) else { return 0 };
            return if wp == 0 { core::mem::replace(&mut w.icons.1, lp) } else { core::mem::replace(&mut w.icons.0, lp) };
        }
        W_GETICON => return wnd(h).map_or(0, |w| if wp == 0 { w.icons.1 } else { w.icons.0 }),
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
                _ => {
                    if let Some(fl) = inflight.filter(|f| f.q.msg == WM_SYSCOMMAND) {
                        return unsafe { WinDefWindowProc(fl.q.hwnd, fl.q.msg, fl.q.mp1, fl.q.mp2) };
                    }
                }
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
            if !wnd(h).is_some_and(|w| w.framed) {
                return 0;
            }
        }
        W_CONTEXTMENU => {
            if let Some(p) = wnd(h).filter(|w| w.style & WS_CHILD != 0).map(|w| w.parent) {
                return send(p, m, wp, lp);
            }
            return 0;
        }
        W_GETFONT | W_GETDLGCODE | W_NOTIFYFORMAT => return 0,
        W_WINDOWPOSCHANGING | W_WINDOWPOSCHANGED | W_GETMINMAXINFO | W_ACTIVATEAPP => return 0,
        _ => {}
    }
    match inflight {
        Some(f) => unsafe { WinDefWindowProc(f.q.hwnd, f.q.msg, f.q.mp1, f.q.mp2) },
        None => 0,
    }
}

/// BeginPaint: the DC (erased with WM_ERASEBKGND) and PAINTSTRUCT (hdc,
/// fErase, rcPaint, ...).
pub fn begin_paint(h: u16, ps: u32) -> u16 {
    let Some(c) = wnd(h).map(|w| w.client) else { return 0 };
    let mut rc = RectL::default();
    let hps = unsafe { WinBeginPaint(c, 0, &mut rc) };
    if hps == 0 {
        return 0;
    }
    // A window without WS_CLIPCHILDREN paints over its children: they
    // paint again after it.
    if wnd(h).is_some_and(|w| w.style & WS_CLIPCHILDREN == 0) {
        for k in children(h) {
            let Some(f) = wnd(k).filter(|w| w.style & WS_VISIBLE != 0).map(|w| w.frame) else { continue };
            let mut swp = Swp::default();
            unsafe { WinQueryWindowPos(f, &mut swp) };
            let r = RectL { l: rc.l.max(swp.x) - swp.x, b: rc.b.max(swp.y) - swp.y, r: rc.r.min(swp.x + swp.cx) - swp.x, t: rc.t.min(swp.y + swp.cy) - swp.y };
            if r.l < r.r && r.b < r.t {
                unsafe { WinInvalidateRect(f, &r, 1) };
            }
        }
    }
    let ch = client_height(c);
    let hdc = gdi::new_dc(hps, c, ch, gdi::DcKind::Paint);
    crate::caret::hide_for_paint(h);
    let erased = send(h, W_ERASEBKGND, hdc as u32, 0) != 0;
    let mut more = RectL::default();
    if unsafe { WinQueryUpdateRect(c, &mut more) } != 0 && more.r > more.l && more.t > more.b {
        unsafe { WinValidateRect(c, core::ptr::null(), 0) };
        rc = RectL { l: rc.l.min(more.l), b: rc.b.min(more.b), r: rc.r.max(more.r), t: rc.t.max(more.t) };
    }
    if ps != 0 {
        unsafe { core::ptr::write_bytes(ps as *mut u8, 0, 64) };
        crate::wr32(ps, 0, hdc as u32);
        crate::wr32(ps, 4, !erased as u32);
        write_rect(ps + 8, [rc.l, ch - rc.t, rc.r, ch - rc.b]);
    }
    hdc
}

/// BeginPaint for the built-in controls: the DC (erased by the control
/// itself).
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

// ---- Window and class values ----

/// Get/SetWindowLong: GWL_WNDPROC (-4), GWL_HINSTANCE (-6),
/// GWL_HWNDPARENT (-8), GWL_ID (-12), GWL_STYLE (-16), GWL_EXSTYLE
/// (-20), GWL_USERDATA (-21), or the window's extra bytes (DWLP_* of a
/// dialog are extra bytes too). Setting gives the old value.
pub fn window_value(h: u16, i: i32, set: Option<u32>, n: usize, wide: bool) -> u32 {
    let Some(w) = wnd(h) else { return crate::fail(1400, 0) };
    match i {
        -4 => {
            let old = w.proc_;
            if let Some(v) = set {
                note_proc(old, w.unicode);
                w.proc_ = v;
                w.unicode = wide && !is_native(v);
            }
            return old;
        }
        -6 => {
            let old = w.hinst;
            if let Some(v) = set {
                w.hinst = v;
            }
            return old;
        }
        -8 => return w.parent as u32,
        -12 => {
            let old = w.id;
            if let Some(v) = set {
                w.id = v;
            }
            return old;
        }
        -21 => {
            let old = w.user_data;
            if let Some(v) = set {
                w.user_data = v;
            }
            return old;
        }
        -16 | -20 => {
            let old = if i == -16 { w.style } else { w.exstyle };
            let Some(v) = set else { return old };
            if i == -20 {
                w.exstyle = v;
                return old;
            }
            w.style = v;
            if (old ^ v) & WS_CLIPCHILDREN != 0 {
                let c = w.client;
                unsafe { WinSetWindowBits(c, QWL_STYLE, pm_clip_children(v), crate::pm::WS_CLIPCHILDREN) };
            }
            if (old ^ v) & W_CLIPSIBLINGS != 0 && v & WS_CHILD != 0 {
                let f = w.frame;
                unsafe { WinSetWindowBits(f, QWL_STYLE, if v & W_CLIPSIBLINGS != 0 { WS_CLIPSIBLINGS } else { 0 }, WS_CLIPSIBLINGS) };
            }
            if (old ^ v) & WS_VISIBLE != 0 {
                let f = w.frame;
                unsafe { WinShowWindow(f, (v & WS_VISIBLE != 0) as u32) };
            }
            return old;
        }
        _ => {}
    }
    if i < 0 {
        return crate::fail(1413, 0); // ERROR_INVALID_INDEX
    }
    let i = i as usize;
    if i + n > w.extra.len() {
        return crate::fail(1413, 0);
    }
    let mut old = [0u8; 4];
    old[..n].copy_from_slice(&w.extra[i..i + n]);
    if let Some(v) = set {
        w.extra[i..i + n].copy_from_slice(&v.to_le_bytes()[..n]);
    }
    u32::from_le_bytes(old)
}

/// Get/SetClassLong: GCL_MENUNAME (-8), GCL_HBRBACKGROUND (-10),
/// GCL_HCURSOR (-12), GCL_HICON (-14), GCL_HMODULE (-16),
/// GCL_CBWNDEXTRA (-18), GCL_CBCLSEXTRA (-20), GCL_WNDPROC (-24),
/// GCL_STYLE (-26), GCW_ATOM (-32), GCL_HICONSM (-34), or the class's
/// extra bytes.
pub fn class_value(h: u16, i: i32, set: Option<u32>, n: usize) -> u32 {
    let Some(ci) = wnd(h).map(|w| w.class) else { return crate::fail(1400, 0) };
    let c = &mut u().classes[ci];
    let field: Option<&mut u32> = match i {
        -10 => Some(&mut c.brush),
        -12 => Some(&mut c.cursor),
        -14 => Some(&mut c.icon),
        -16 => Some(&mut c.hinst),
        -18 => Some(&mut c.wnd_extra),
        -24 => Some(&mut c.proc_),
        -26 => Some(&mut c.style),
        -34 => Some(&mut c.icon_sm),
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
        -20 => return c.cls_extra.len() as u32,
        -32 => return c.atom as u32,
        -8 => return 0,
        _ => {}
    }
    if i < 0 {
        return crate::fail(1413, 0);
    }
    let i = i as usize;
    if i + n > c.cls_extra.len() {
        return crate::fail(1413, 0);
    }
    let mut old = [0u8; 4];
    old[..n].copy_from_slice(&c.cls_extra[i..i + n]);
    if let Some(v) = set {
        c.cls_extra[i..i + n].copy_from_slice(&v.to_le_bytes()[..n]);
    }
    u32::from_le_bytes(old)
}

/// GetSystemMetrics.
pub fn system_metric(i: i32) -> i32 {
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
        19 => 1,
        20 => q(SV_CYVSCROLLARROW),
        21 => q(SV_CXHSCROLLARROW),
        28 => 112,
        29 => 27,
        30 | 31 => q(SV_CYTITLEBAR),
        32 => q(SV_CXSIZEBORDER),
        33 => q(SV_CYSIZEBORDER),
        34 => 112,
        35 => 27,
        36 | 37 => 4,
        38 | 39 => 75,
        40 => 0,
        43 => 3,
        // SM_CXEDGE, SM_CYEDGE
        45 | 46 => 2,
        // SM_CXSMICON, SM_CYSMICON
        49 | 50 => 16,
        // SM_CYSMCAPTION, SM_CXSMSIZE, SM_CYSMSIZE
        51 => q(SV_CYTITLEBAR) - 4,
        52 | 53 => q(SV_CYTITLEBAR) - 6,
        // SM_CXMENUSIZE, SM_CYMENUSIZE
        54 | 55 => q(SV_CYMENU),
        // SM_CXMINIMIZED, SM_CYMINIMIZED
        57 => 160,
        58 => 24,
        // SM_CXMAXTRACK, SM_CYMAXTRACK, SM_CXMAXIMIZED, SM_CYMAXIMIZED
        59 | 61 => q(SV_CXSCREEN),
        60 | 62 => q(SV_CYSCREEN),
        // SM_MOUSEWHEELPRESENT
        75 => 0,
        // SM_XVIRTUALSCREEN.. SM_CYVIRTUALSCREEN, SM_CMONITORS
        76 | 77 => 0,
        78 => q(SV_CXSCREEN),
        79 => q(SV_CYSCREEN),
        80 => 1,
        // SM_SAMEDISPLAYFORMAT
        81 => 1,
        _ => 0,
    }
}

// ---- Properties ----

pub fn set_prop(h: u16, name: Vec<u8>, v: u32) -> bool {
    let Some(w) = wnd(h) else { return false };
    let name = name.to_ascii_uppercase();
    match w.props.iter_mut().find(|p| p.0 == name) {
        Some(p) => p.1 = v,
        None => w.props.push((name, v)),
    }
    true
}

pub fn get_prop(h: u16, name: &[u8]) -> u32 {
    let name = name.to_ascii_uppercase();
    wnd(h).and_then(|w| w.props.iter().find(|p| p.0 == name).map(|p| p.1)).unwrap_or(0)
}

pub fn remove_prop(h: u16, name: &[u8]) -> u32 {
    let name = name.to_ascii_uppercase();
    let Some(w) = wnd(h) else { return 0 };
    match w.props.iter().position(|p| p.0 == name) {
        Some(i) => w.props.remove(i).1,
        None => 0,
    }
}

/// A property name argument: a string, or an atom (as "#atom").
pub fn prop_name(p: u32, wide: bool) -> Vec<u8> {
    if p >> 16 == 0 {
        return alloc::format!("#{}", p & 0xffff).into_bytes();
    }
    if wide { crate::wide_str(p).unwrap_or_default() } else { far_str(p).unwrap_or(b"").to_vec() }
}

// ---- Message boxes ----

/// Break text into lines of at most `max` characters at blanks, as
/// Windows' message box wraps long lines (PM's keeps them).
fn wrap(s: &[u8], max: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 8);
    for (i, line) in s.split(|&c| c == b'\n').enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        let mut col = 0;
        for (j, word) in line.split(|&c| c == b' ').enumerate() {
            if j > 0 {
                if col + 1 + word.len() > max && col > 0 {
                    out.push(b'\n');
                    col = 0;
                } else {
                    out.push(b' ');
                    col += 1;
                }
            }
            out.extend_from_slice(word);
            col += word.len();
        }
    }
    out
}

/// MessageBox: PM's message box (on the console's standard error when
/// the program has no PM queue: a console program).
pub fn message_box(h: u16, text: &[u8], cap: &[u8], ty: u32) -> u32 {
    if queue().hmq == 0 {
        let mut v = Vec::new();
        v.extend_from_slice(b"\r\n[");
        v.extend_from_slice(cap);
        v.extend_from_slice(b"] ");
        v.extend_from_slice(text);
        v.extend_from_slice(b"\r\n");
        let mut n = 0;
        unsafe { os2::DosWrite(2, v.as_ptr(), v.len() as u32, &mut n) };
        return match ty & 0xf {
            3 | 4 => 6,
            2 => 3,
            5 => 4,
            _ => 1,
        };
    }
    let t = z(&wrap(text, 64));
    let c = z(cap);
    let buttons = match ty & 0x0f {
        1 => 1,
        2 => 3,
        3 => 5,
        4 => 4,
        5 => 2,
        _ => 0,
    };
    let icon = match ty & 0xf0 {
        0x10 => 0x0040,
        0x20 => 0x0010,
        0x30 => 0x0020,
        0x40 => 0x0030,
        _ => 0,
    };
    let fl = buttons | icon | (ty & 0x0300) | if ty & 0x1000 != 0 { 0x1000 } else { 0 } | 0x4000;
    let owner = wnd(top_level(h)).map_or(0, |w| w.frame);
    let r = unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), 0, fl) };
    if r == 0xffff { 0 } else { r }
}

/// The handles of the class brush (an HBRUSH, or a COLOR_* index + 1).
pub fn read_rect_arg(p: u32) -> Option<[i32; 4]> {
    read_rect(p)
}
