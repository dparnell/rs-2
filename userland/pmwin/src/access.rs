//! The window manager's share of the System Setup objects' settings, as
//! system values the Workplace Shell sets:
//!
//! - **Special Needs** (WPCONFIG's WPSpecialNeeds and the Keyboard's
//!   Special Needs page; rs2-private SV_RS2_SPNEEDS* values): with it on,
//!   a key counts only when it is held down for the acceptance delay (a
//!   shorter press is ignored; modifier keys always count), and Special
//!   Needs turns itself off after the time-out without keyboard input.
//!   IBM's keyboard driver does this (IMP32SetAIMProfile); rs2's PM input
//!   thread does it on the key events it gets.
//! - **Alt+Tab** (the System object's Input page, value 100 "Alt+Tab
//!   switching window"): Alt+Tab activates the next window in the
//!   z-order, or, with the switching window on, shows the window it
//!   would switch to (its icon and title) while Alt is held, each Tab
//!   (Shift+Tab: back) moving on, and switches when Alt goes up (the
//!   kernel passes PM the Alt key's release for this). Alt+Esc stays the
//!   kernel's (the next session).

use alloc::vec::Vec;

use crate::input::Input;
use crate::lock::wndproc;
use crate::state::st;
use crate::msg::{post_queue, qmsg};
use crate::state::now;
use crate::{ctl, frame, win};
use pmwin::HWND_DESKTOP;

/// rs2-private system values (WinSetSysValue/WinQuerySysValue).
pub const SV_RS2_SPNEEDS: i32 = 4000;
pub const SV_RS2_SPNEEDS_ACCEPT: i32 = 4001;
pub const SV_RS2_SPNEEDS_TIMEOUT: i32 = 4002;
/// The System object's Input page.
pub const SV_FULLWINDOWDRAG: i32 = 99;
pub const SV_ALTTABWINDOW: i32 = 100;
pub const SV_ASYNCFOCUSCHANGE: i32 = 102;
pub const SV_AFCSENSITIVITY: i32 = 103;

pub fn sysval(i: i32) -> i32 {
    crate::misc::WinQuerySysValue(1, i)
}

// ---- Special Needs: the acceptance delay ----

/// A key press held back until it has been down long enough.
struct Pending {
    ev: Input,
    since: u32,
    accepted: bool,
}

static mut PENDING: Option<Pending> = None;

fn pending() -> &'static mut Option<Pending> {
    unsafe { &mut *core::ptr::addr_of_mut!(PENDING) }
}

/// Shift, Ctrl, Alt and the locks: never delayed.
fn is_modifier(scan: u32) -> bool {
    matches!(scan, 0x2a | 0x36 | 0x1d | 0x38 | 0x3a | 0x45 | 0x46 | 0x5b | 0x5c)
}

/// The key events PM passes on now: none (held back), the event, or a
/// held-back press followed by this event.
pub fn special_needs(ev: &Input) -> Vec<Input> {
    let on = sysval(SV_RS2_SPNEEDS) != 0;
    if !on {
        if let Some(p) = pending().take() {
            if !p.accepted {
                return alloc::vec![p.ev, *ev];
            }
        }
        return alloc::vec![*ev];
    }
    let t = now();
    // The time-out: no keyboard input for that long turns it off.
    let timeout = sysval(SV_RS2_SPNEEDS_TIMEOUT);
    let last = st().last_input;
    if timeout > 0 && last != 0 && t.wrapping_sub(last) > timeout as u32 * 1000 {
        crate::misc::WinSetSysValue(1, SV_RS2_SPNEEDS, 0);
        *pending() = None;
        return alloc::vec![*ev];
    }
    let accept = sysval(SV_RS2_SPNEEDS_ACCEPT).max(0) as u32;
    let scan = (ev.a >> 8) & 0xff;
    let up = ev.a >> 24 & 1 != 0;
    if accept == 0 || is_modifier(scan) {
        return alloc::vec![*ev];
    }
    let same = pending().as_ref().is_some_and(|p| (p.ev.a >> 8) & 0xff == scan);
    if !up {
        if !same {
            // A new key: held back until it has been down long enough.
            *pending() = Some(Pending { ev: *ev, since: t, accepted: false });
            return Vec::new();
        }
        // The key repeats: accepted once held long enough.
        let p = pending().as_mut().unwrap();
        if p.accepted {
            return alloc::vec![*ev];
        }
        if t.wrapping_sub(p.since) >= accept {
            p.accepted = true;
            let mut first = p.ev;
            first.time = ev.time;
            return alloc::vec![first];
        }
        return Vec::new();
    }
    if !same {
        return alloc::vec![*ev];
    }
    let p = pending().take().unwrap();
    if p.accepted {
        return alloc::vec![*ev];
    }
    if t.wrapping_sub(p.since) >= accept {
        let mut first = p.ev;
        first.time = ev.time;
        return alloc::vec![first, *ev];
    }
    // Let go too soon: not a key press.
    Vec::new()
}

// ---- Alt+Tab, Alt+Esc ----

/// Private message to the server's window thread: show the switching
/// window for mp1 (0: take it away).
pub const WM_RS2_ALTTAB: u32 = 0x0ff6;

struct Switching {
    list: Vec<u32>,
    at: usize,
}

static mut SWITCHING: Option<Switching> = None;

fn switching() -> &'static mut Option<Switching> {
    unsafe { &mut *core::ptr::addr_of_mut!(SWITCHING) }
}

/// The frames Alt+Tab goes through: top-level frames of programs with a
/// title bar, visible or minimized, top of the z-order first.
fn candidates() -> Vec<u32> {
    let s = st();
    let server = s.server_pid;
    let kids = s.desk.get(1).map_or(Vec::new(), |w| w.children.clone());
    kids.into_iter()
        .rev()
        .filter(|&f| s.frames.contains_key(&f))
        .filter(|&f| s.desk.get(f).is_some_and(|w| w.visible_style() && w.style & pmwin::ws::DISABLED == 0))
        .filter(|&f| win::WinWindowFromID(f, crate::frame::FID_TITLEBAR) != 0)
        .filter(|&f| s.queues.get(&s.queue_of(f)).is_none_or(|q| q.pid != server))
        .filter(|&f| s.switch.of_window(f).map(crate::switch::current).is_none_or(|c| c.visibility & crate::switch::SWL_INVISIBLE == 0))
        .collect()
}

fn post_server(m: u32, mp1: u32) {
    let q = st().server_queue;
    if q != 0 {
        post_queue(q, qmsg(1, m, mp1, 0));
    }
}

/// A key while Alt+Tab switching may be going on: true when it was used.
pub fn switch_key(ev: &Input) -> bool {
    let scan = (ev.a >> 8) & 0xff;
    let up = ev.a >> 24 & 1 != 0;
    let shift = ev.flags as u32;
    let alt = shift & 0x8 != 0;
    let ctrl = shift & 0x4 != 0;
    // Alt going up (the kernel passes it for this) is not a key for
    // programs.
    let alt_up = scan == 0x38 && up;
    if switching().is_some() {
        // Alt went up: switch.
        if alt_up || (!alt && !up) {
            let target = switching().take().and_then(|s| s.list.get(s.at).copied());
            post_server(WM_RS2_ALTTAB, 0);
            if let Some(f) = target {
                crate::server::switch_to_window(f);
            }
            return alt_up || scan == 0x0f;
        }
        if up {
            return scan == 0x0f;
        }
        match scan {
            0x0f => {
                let back = shift & 0x3 != 0;
                let s = switching().as_mut().unwrap();
                let n = s.list.len();
                s.at = if back { (s.at + n - 1) % n } else { (s.at + 1) % n };
                post_server(WM_RS2_ALTTAB, s.list[s.at]);
            }
            0x01 => {
                *switching() = None;
                post_server(WM_RS2_ALTTAB, 0);
            }
            _ => {}
        }
        return true;
    }
    if alt_up {
        return true;
    }
    if !alt || ctrl || up {
        return false;
    }
    match scan {
        // Alt+Tab.
        0x0f => {
            let list = candidates();
            if list.is_empty() {
                return true;
            }
            let active = st().active;
            let cur = list.iter().position(|&f| f == active);
            let back = shift & 0x3 != 0;
            let n = list.len();
            let at = match (cur, back) {
                (Some(i), false) => (i + 1) % n,
                (Some(i), true) => (i + n - 1) % n,
                (None, false) => 0,
                (None, true) => n - 1,
            };
            if sysval(SV_ALTTABWINDOW) == 0 || n == 1 {
                crate::server::switch_to_window(list[at]);
                return true;
            }
            post_server(WM_RS2_ALTTAB, list[at]);
            *switching() = Some(Switching { list, at });
            true
        }
        _ => false,
    }
}

// ---- The switching window (the server's window thread) ----

static mut BOX: u32 = 0;
static mut SHOWING: u32 = 0;
static mut REGISTERED: bool = false;
const BOX_W: i32 = 320;
const BOX_H: i32 = 72;

wndproc! {
    alttab_entry => alttab_proc;
}

fn title_of(f: u32) -> Vec<u8> {
    let entry = st().switch.of_window(f).map(crate::switch::current);
    if let Some(c) = &entry {
        let t = crate::switch::title(c);
        if !t.is_empty() {
            return t.to_vec();
        }
    }
    let tb = win::WinWindowFromID(f, crate::frame::FID_TITLEBAR);
    let t = ctl::text(if tb != 0 { tb } else { f });
    if t.is_empty() { b"(untitled)".to_vec() } else { t }
}

extern "C" fn alttab_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    const WM_PAINT: u32 = 0x0023;
    match m {
        WM_PAINT => {
            let hps = win::WinBeginPaint(h, 0, core::ptr::null_mut());
            let r = crate::gpiapi::RectL { left: 0, bottom: 0, right: BOX_W, top: BOX_H };
            // SYSCLR_DIALOGBACKGROUND, SYSCLR_WINDOWFRAME, SYSCLR_WINDOWTEXT.
            win::WinFillRect(hps, &r, -34);
            crate::sysdraw::WinDrawBorder(hps, &r, 2, 2, -19, -34, 0x0400);
            let f = unsafe { SHOWING };
            if f != 0 && st().desk.exists(f) {
                let icon = frame::icon(f);
                if icon != 0 {
                    crate::pointer::WinDrawPointer(hps, 16, (BOX_H - 32) / 2, icon, 0);
                }
                let mut t = title_of(f);
                t.truncate(80);
                let mut tr = crate::gpiapi::RectL { left: 60, bottom: 4, right: BOX_W - 8, top: BOX_H - 4 };
                // DT_LEFT | DT_VCENTER
                win::WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut tr, -17, -34, 0x0400);
            }
            win::WinEndPaint(hps);
            0
        }
        _ => crate::msg::WinDefWindowProc(h, m, mp1, mp2),
    }
}

/// WM_RS2_ALTTAB at the server's desktop window.
pub fn server_alttab(f: u32) {
    unsafe {
        if !REGISTERED {
            win::WinRegisterClass(1, b"RS2AltTab\0".as_ptr(), alttab_entry as *const () as u32, 0, 0);
            REGISTERED = true;
        }
        SHOWING = f;
        if f == 0 {
            if BOX != 0 {
                win::WinDestroyWindow(BOX);
                BOX = 0;
            }
            return;
        }
        let s = st();
        let (sw, sh) = (s.surface.width, s.surface.height);
        if BOX == 0 || !st().desk.exists(BOX) {
            // WS_SAVEBITS; on top of everything.
            BOX = win::WinCreateWindow(1, b"RS2AltTab\0".as_ptr(), b"\0".as_ptr(), 0x0400_0000, (sw - BOX_W) / 2, (sh - BOX_H) / 2, BOX_W, BOX_H, 0, 3, 0, 0, 0);
        }
        if BOX != 0 {
            // HWND_TOP; SWP_SHOW | SWP_ZORDER.
            win::WinSetWindowPos(BOX, 3, 0, 0, 0, 0, 0x0008 | 0x0004);
            win::WinInvalidateRect(BOX, core::ptr::null(), 0);
        }
    }
}

// ---- Moving and sizing without full window drag ----

/// The outline on the screen while a frame is moved or sized with the
/// System object's "Full window drag" off (IBM's default): the window
/// follows only when the button goes up.
static mut OUTLINE: Option<(i32, i32, i32, i32)> = None;

/// Whether frames move and size as the pointer goes (else an outline).
pub fn full_drag() -> bool {
    sysval(SV_FULLWINDOWDRAG) != 0
}

fn invert_outline(r: (i32, i32, i32, i32)) {
    use crate::gpiapi::RectL;
    let (x, y, cx, cy) = r;
    let (l, b, rr, t) = (x, y, x + cx, y + cy);
    let k = 3;
    if rr - l <= 2 * k || t - b <= 2 * k {
        return;
    }
    let hps = crate::misc::WinGetScreenPS(HWND_DESKTOP);
    if hps == 0 {
        return;
    }
    for e in [
        RectL { left: l, bottom: b, right: rr, top: b + k },
        RectL { left: l, bottom: t - k, right: rr, top: t },
        RectL { left: l, bottom: b + k, right: l + k, top: t - k },
        RectL { left: rr - k, bottom: b + k, right: rr, top: t - k },
    ] {
        crate::misc::WinInvertRect(hps, &e);
    }
    win::WinReleasePS(hps);
}

/// Show the outline at `r` (screen coordinates; None: take it away).
pub fn outline(r: Option<(i32, i32, i32, i32)>) {
    let cur = unsafe { &mut *core::ptr::addr_of_mut!(OUTLINE) };
    if *cur == r {
        return;
    }
    if let Some(old) = cur.take() {
        invert_outline(old);
    }
    if let Some(new) = r {
        invert_outline(new);
    }
    *cur = r;
}

// ---- Asynchronous focus change ----

/// How long a thread may take to come for its input before the server
/// treats it as not responding: PM_HANGTIME, or less with the System
/// object's asynchronous focus change on (its delay in tenths).
pub fn hang_limit(hang_ms: u32) -> u32 {
    if sysval(SV_ASYNCFOCUSCHANGE) == 0 {
        return hang_ms;
    }
    let afc = (sysval(SV_AFCSENSITIVITY).clamp(0, 99) as u32 * 100).max(250);
    afc.min(hang_ms)
}
