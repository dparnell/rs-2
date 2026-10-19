//! Spin buttons (WC_SPINBUTTON): an entry field with up and down arrows
//! that steps through a numeric range or an array of strings. A master
//! (SPBS_MASTER) has the arrows; servants linked to it with
//! SPBM_SETMASTER have none, and the master's arrows spin whichever of
//! them last had the focus. The value logic is `pmwin::controls::Spin`.

use alloc::vec::Vec;

use gpi::Rect;
use pmwin::controls::Spin;

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc};
use crate::state::st;
use crate::win::{self, WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_PAINT, WM_SETFOCUS, WM_SIZE, WM_TIMER};

const WM_CREATE: u32 = 0x0001;

const SPBS_NUMERICONLY: u32 = 0x0001;
const SPBS_READONLY: u32 = 0x0002;
const SPBS_JUSTRIGHT: u32 = 0x0004;
const SPBS_JUSTLEFT: u32 = 0x0008;
const SPBS_MASTER: u32 = 0x0010;
const SPBS_NOBORDER: u32 = 0x0020;
const SPBS_PADWITHZEROS: u32 = 0x0080;

const SPBN_UPARROW: u32 = 0x20a;
const SPBN_DOWNARROW: u32 = 0x20b;
const SPBN_ENDSPIN: u32 = 0x20c;
const SPBN_CHANGE: u32 = 0x20d;
const SPBN_SETFOCUS: u32 = 0x20e;
const SPBN_KILLFOCUS: u32 = 0x20f;

const SPBM_OVERRIDESETLIMITS: u32 = 0x200;
const SPBM_QUERYLIMITS: u32 = 0x201;
const SPBM_SETTEXTLIMIT: u32 = 0x202;
const SPBM_SPINUP: u32 = 0x203;
const SPBM_SPINDOWN: u32 = 0x204;
const SPBM_QUERYVALUE: u32 = 0x205;
const SPBM_SETARRAY: u32 = 0x206;
const SPBM_SETLIMITS: u32 = 0x207;
const SPBM_SETCURRENTVALUE: u32 = 0x208;
const SPBM_SETMASTER: u32 = 0x209;

const SPBQ_ALWAYSUPDATE: u32 = 1;
const SPBQ_DONOTUPDATE: u32 = 3;

const EN_SETFOCUS: u32 = 0x0001;
const EN_KILLFOCUS: u32 = 0x0002;
const EN_CHANGE: u32 = 0x0004;
const EM_SETTEXTLIMIT: u32 = 0x0143;
const EM_SETSEL: u32 = 0x0142;
const ES_RIGHT: u32 = 0x0002;
const ES_CENTER: u32 = 0x0001;
const ES_READONLY: u32 = 0x0020;

/// Arrow repeat while a button is held.
const TID_SPIN: u32 = 0x7f30;

wndproc! {
    spin_entry => spin_proc;
}

struct SpinWin {
    s: Spin,
    entry: u32,
    master: u32,
    /// For a master: the servant (or itself) that last had the focus.
    target: u32,
    /// Arrow held: +1 up, -1 down.
    held: i32,
    /// Setting the entry text ourselves (no SPBN_CHANGE echo).
    updating: bool,
}

fn data(h: u32) -> Option<&'static mut SpinWin> {
    ctl::data::<SpinWin>(h)
}

pub fn is_spin(h: u32) -> bool {
    data(h).is_some()
}

/// The control draws arrows: a master, or a servant without a master.
fn has_arrows(h: u32) -> bool {
    ctl::style(h) & SPBS_MASTER != 0 || data(h).is_some_and(|d| d.master == 0)
}

fn arrow_w(h: u32) -> i32 {
    if has_arrows(h) { (ctl::size(h).1).clamp(10, 18) } else { 0 }
}

fn border(h: u32) -> i32 {
    if ctl::style(h) & SPBS_NOBORDER != 0 { 0 } else { 2 }
}

fn layout(h: u32) {
    let (cx, cy) = ctl::size(h);
    let b = border(h);
    let aw = arrow_w(h);
    let Some(e) = data(h).map(|d| d.entry) else { return };
    st().desk.set_pos(e, b + 1, b, (cx - aw - 2 * b - 1).max(0), (cy - 2 * b).max(0));
}

/// Show the current value in the entry field.
fn show_value(h: u32) {
    let pad = ctl::style(h) & SPBS_PADWITHZEROS != 0;
    let Some(d) = data(h) else { return };
    let t = d.s.text(pad);
    let e = d.entry;
    d.updating = true;
    win::set_text(e, &t);
    if let Some(d) = data(h) {
        d.updating = false;
    }
}

/// Take what was typed into the entry field (if it is a valid value).
fn take_typed(h: u32) -> bool {
    let Some(d) = data(h) else { return false };
    let t = ctl::text(d.entry);
    d.s.set_from_text(&t)
}

/// Spin by `n` steps (negative: down) and tell the owner.
fn spin(h: u32, n: i32) {
    take_typed(h);
    let Some(d) = data(h) else { return };
    d.s.spin(n);
    show_value(h);
    ctl::notify(h, if n >= 0 { SPBN_UPARROW } else { SPBN_DOWNARROW });
}

/// The control the arrows act on: for a master, the servant that last
/// had the focus.
fn arrow_target(h: u32) -> u32 {
    let t = data(h).map_or(h, |d| d.target);
    if t != 0 && t != h && is_spin(t) { t } else { h }
}

fn paint(h: u32) {
    let (cx, cy) = ctl::size(h);
    let aw = arrow_w(h);
    let b = border(h);
    let enabled = ctl::enabled(h);
    let held = data(h).map_or(0, |d| d.held);
    draw::paint(h, |c: &mut Canvas| {
        let r = c.rect();
        c.fill(r, draw::WINDOW);
        if b > 0 {
            let er = Rect::new(0, 0, cx - aw, cy);
            c.sunken(er);
            c.bevel(Rect::new(1, 1, cx - aw - 1, cy - 1), draw::DARK, draw::FACE);
        }
        if aw > 0 {
            let col = if enabled { draw::TEXT } else { draw::SHADOW };
            let up = Rect::new(cx - aw, cy / 2, cx, cy);
            let down = Rect::new(cx - aw, 0, cx - 0, cy / 2);
            for (rr, dir) in [(up, 1), (down, -1)] {
                c.fill(rr, draw::FACE);
                if held == dir { c.sunken(rr) } else { c.raised(rr) }
                let (mx, my) = (rr.left + rr.width() / 2, rr.bottom + rr.height() / 2);
                for i in 0..3 {
                    let y = if dir > 0 { my + 1 - i } else { my - 1 + i };
                    c.fill(Rect::new(mx - i, y, mx + i + 1, y + 1), col);
                }
            }
        }
    });
}

pub fn text(h: u32) -> Option<Vec<u8>> {
    let e = data(h)?.entry;
    Some(ctl::text(e))
}

pub fn set_text(h: u32, t: &[u8]) {
    let Some(e) = data(h).map(|d| d.entry) else { return };
    win::set_text(e, t);
}

pub extern "C" fn spin_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            let style = ctl::style(h);
            let mut s = Spin::new();
            let mut limit = 0;
            if mp1 != 0 {
                // SPBCDATA: cbSize, ulTextLimit, lLowerLimit, lUpperLimit,
                // idMasterSpb.
                let rd = |o: u32| unsafe { ((mp1 + o) as *const u32).read_unaligned() };
                limit = rd(4);
                s.set_limits(rd(8) as i32, rd(12) as i32);
            }
            let just = if style & SPBS_JUSTLEFT != 0 && style & SPBS_JUSTRIGHT != 0 {
                ES_CENTER
            } else if style & SPBS_JUSTRIGHT != 0 {
                ES_RIGHT
            } else {
                0
            };
            let ro = if style & SPBS_READONLY != 0 { ES_READONLY } else { 0 };
            let entry = win::create(h, b"#6", b"", pmwin::ws::VISIBLE | just | ro, Rect::default(), h, 0, 0);
            send(entry, EM_SETTEXTLIMIT, if limit > 0 { limit } else { 32 }, 0);
            ctl::set_data(h, SpinWin { s, entry, master: 0, target: h, held: 0, updating: false });
            layout(h);
            show_value(h);
            0
        }
        WM_SIZE => {
            layout(h);
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON1DBLCLK => {
            if !ctl::enabled(h) {
                return 1;
            }
            let p = ctl::mouse_pos(mp1);
            let (cx, cy) = ctl::size(h);
            let aw = arrow_w(h);
            if aw > 0 && p.x >= cx - aw {
                let dir = if p.y >= cy / 2 { 1 } else { -1 };
                let t = arrow_target(h);
                if let Some(d) = data(h) {
                    d.held = dir;
                }
                spin(t, dir);
                st().capture = h;
                crate::msg::WinStartTimer(1, h, TID_SPIN, 300);
                ctl::invalidate(h);
            } else if let Some(e) = data(h).map(|d| d.entry) {
                crate::win::WinSetFocus(1, e);
            }
            1
        }
        WM_TIMER if mp1 == TID_SPIN => {
            let dir = data(h).map_or(0, |d| d.held);
            if dir != 0 {
                spin(arrow_target(h), dir);
                crate::msg::WinStartTimer(1, h, TID_SPIN, 60);
            } else {
                crate::msg::WinStopTimer(1, h, TID_SPIN);
            }
            0
        }
        WM_BUTTON1UP => {
            if let Some(d) = data(h) {
                if d.held != 0 {
                    d.held = 0;
                    crate::msg::WinStopTimer(1, h, TID_SPIN);
                    if st().capture == h {
                        st().capture = 0;
                    }
                    ctl::invalidate(h);
                    let t = arrow_target(h);
                    ctl::notify(t, SPBN_ENDSPIN);
                }
            }
            1
        }
        WM_SETFOCUS => {
            if mp2 & 0xffff != 0 {
                if let Some(e) = data(h).map(|d| d.entry) {
                    crate::win::WinSetFocus(1, e);
                }
            }
            0
        }
        WM_CHAR => {
            let k = Key::from(mp1, mp2);
            if k.up() {
                return WinDefWindowProc(h, msg, mp1, mp2);
            }
            match k.vk {
                VK_UP if !k.alt() => spin(h, 1),
                VK_DOWN if !k.alt() => spin(h, -1),
                VK_PAGEUP => spin(h, 10),
                VK_PAGEDOWN => spin(h, -10),
                _ => {
                    // SPBS_NUMERICONLY: digits (and a minus sign) only.
                    if ctl::style(h) & SPBS_NUMERICONLY != 0 && k.fs & KC_CHAR != 0 && !k.ctrl() && !k.alt() && k.ch >= 0x20 {
                        crate::misc::WinAlarm(1, 0);
                        return 1;
                    }
                    return WinDefWindowProc(h, msg, mp1, mp2);
                }
            }
            1
        }
        WM_CONTROL => {
            let code = hi(mp1);
            match code {
                EN_CHANGE => {
                    if !data(h).is_some_and(|d| d.updating) {
                        ctl::notify(h, SPBN_CHANGE);
                    }
                }
                EN_SETFOCUS => {
                    // A servant tells its master it is the one to spin.
                    let m = data(h).map_or(0, |d| d.master);
                    if let Some(md) = data(m) {
                        md.target = h;
                    } else if let Some(d) = data(h) {
                        d.target = h;
                    }
                    ctl::notify(h, SPBN_SETFOCUS);
                }
                EN_KILLFOCUS => {
                    take_typed(h);
                    ctl::notify(h, SPBN_KILLFOCUS);
                }
                _ => {}
            }
            0
        }
        WM_QUERYDLGCODE => 0,
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        SPBM_SETLIMITS | SPBM_OVERRIDESETLIMITS => {
            let Some(d) = data(h) else { return 0 };
            // mp1 = upper, mp2 = lower.
            d.s.set_limits(mp2 as i32, mp1 as i32);
            show_value(h);
            1
        }
        SPBM_QUERYLIMITS => {
            let Some(d) = data(h) else { return 0 };
            let (lo, hi) = d.s.limits();
            unsafe {
                if mp1 != 0 {
                    (mp1 as *mut i32).write_unaligned(hi);
                }
                if mp2 != 0 {
                    (mp2 as *mut i32).write_unaligned(lo);
                }
            }
            1
        }
        SPBM_SETTEXTLIMIT => {
            let e = data(h).map_or(0, |d| d.entry);
            send(e, EM_SETTEXTLIMIT, mp1 & 0xffff, 0);
            1
        }
        SPBM_SPINUP => {
            spin(h, (mp1 as i32).max(1));
            1
        }
        SPBM_SPINDOWN => {
            spin(h, -(mp1 as i32).max(1));
            1
        }
        SPBM_SETARRAY => {
            // (papszItems, cItems)
            let n = mp2 as usize;
            let mut items = Vec::with_capacity(n);
            for i in 0..n {
                let p = unsafe { ((mp1 as usize + i * 4) as *const u32).read_unaligned() };
                items.push(ctl::cstr(p as *const u8));
            }
            if let Some(d) = data(h) {
                d.s.set_array(items);
            }
            show_value(h);
            1
        }
        SPBM_SETCURRENTVALUE => {
            let Some(d) = data(h) else { return 0 };
            let ok = d.s.set_value(mp1 as i32);
            show_value(h);
            ok as u32
        }
        SPBM_QUERYVALUE => {
            // (pStorage, MPFROM2SHORT(cchBuffer, usFlags)): with a buffer
            // the text, else the value (array index); TRUE when valid.
            let (cch, fl) = (lo(mp2) as usize, hi(mp2));
            let typed = data(h).map_or(Vec::new(), |d| ctl::text(d.entry));
            let Some(d) = data(h) else { return 0 };
            let mut probe = d.s.clone();
            let valid = probe.set_from_text(&typed);
            if fl != SPBQ_DONOTUPDATE && (valid || fl == SPBQ_ALWAYSUPDATE) {
                if valid {
                    d.s = probe.clone();
                }
                show_value(h);
            }
            let Some(d) = data(h) else { return 0 };
            if mp1 != 0 {
                if cch > 0 {
                    let t = if valid { d.s.text(false) } else { typed };
                    ctl::copy_out(&t, cch, mp1 as *mut u8);
                } else {
                    let v = if valid { probe.value() } else { d.s.value() };
                    unsafe { (mp1 as *mut i32).write_unaligned(v) };
                }
            }
            valid as u32
        }
        SPBM_SETMASTER => {
            let Some(d) = data(h) else { return 0 };
            d.master = mp1;
            layout(h);
            ctl::invalidate(h);
            1
        }
        EM_SETSEL => {
            let e = data(h).map_or(0, |d| d.entry);
            send(e, msg, mp1, mp2)
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
