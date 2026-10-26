//! Scroll bars: the window scroll bars (WS_VSCROLL, WS_HSCROLL) and the
//! SCROLLBAR control, drawn in Windows 3.1's style through our GDI.
//!
//! A window's scroll bar is a PM window of the class `Win16Sb` in the
//! window's frame: the PM frame (with the ID FID_VERTSCROLL or
//! FID_HORZSCROLL, so the frame places it) or the `Win16Frame` of a plain
//! window. A SCROLLBAR control is an ordinary window whose procedure
//! (`control_proc`) uses the same drawing and tracking. Tracking a click
//! is a loop on PM's queue while the mouse is captured, as Windows' is;
//! WM_VSCROLL/WM_HSCROLL go to the window (lParam: the position) or to
//! the control's parent (lParam: the position and the control).

use crate::gdi;
use crate::kernel::TASK;
use crate::pm::*;
use crate::user::{self, wnd, W_HSCROLL, W_VSCROLL};

pub struct Bar {
    pub pm: u32,
    pub min: i32,
    pub max: i32,
    pub pos: i32,
    /// ESB_DISABLE_* (1 up/left arrow, 2 down/right arrow).
    pub disabled: u16,
    pub shown: bool,
    pub vert: bool,
    /// The part held down (for drawing it pressed).
    pub pressed: Option<Part>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Part {
    LineUp,
    PageUp,
    Thumb,
    PageDown,
    LineDown,
}

const SB_HORZ: u16 = 0;
const SB_VERT: u16 = 1;
const SB_CTL: u16 = 2;
const SB_BOTH: u16 = 3;
const SBS_VERT: u32 = 1;

const SB_LINEUP: u16 = 0;
const SB_LINEDOWN: u16 = 1;
const SB_PAGEUP: u16 = 2;
const SB_PAGEDOWN: u16 = 3;
const SB_THUMBPOSITION: u16 = 4;
const SB_THUMBTRACK: u16 = 5;
const SB_TOP: u16 = 6;
const SB_BOTTOM: u16 = 7;
const SB_ENDSCROLL: u16 = 8;

/// The timer of a held arrow or page area.
const TID_REPEAT: u32 = 0x7f31;

pub fn register(hab: u32) {
    unsafe { WinRegisterClass(hab, b"Win16Sb\0".as_ptr(), sb_proc, 0, 4) };
}

fn new_bar(pm: u32, vert: bool, min: i32, max: i32) -> Bar {
    Bar { pm, min, max, pos: min, disabled: 0, shown: true, vert, pressed: None }
}

/// A window's scroll bar (made when the window is created).
pub fn create_bar(h: u16, vert: bool) {
    let Some(w) = wnd(h) else { return };
    let (f, framed) = (w.frame, w.framed);
    let id = if !framed { 0 } else if vert { FID_VERTSCROLL } else { FID_HORZSCROLL };
    let pm = unsafe { WinCreateWindow(f, b"Win16Sb\0".as_ptr(), b"\0".as_ptr(), WS_VISIBLE | WS_CLIPSIBLINGS, 0, 0, 0, 0, f, HWND_TOP, id, core::ptr::null(), core::ptr::null()) };
    let b = new_bar(pm, vert, 0, 100);
    if let Some(w) = wnd(h) {
        if vert {
            w.vsb = Some(b);
        } else {
            w.hsb = Some(b);
        }
    }
}

/// The bar of a window: SB_HORZ, SB_VERT or (a control) SB_CTL.
fn bar(h: u16, which: u16) -> Option<&'static mut Bar> {
    match which {
        SB_HORZ => wnd(h)?.hsb.as_mut(),
        SB_VERT => wnd(h)?.vsb.as_mut(),
        SB_CTL => user::data::<Bar>(h),
        _ => None,
    }
}

/// (min, max, pos)
pub fn get(h: u16, which: u16) -> Option<(i32, i32, i32)> {
    bar(h, which).map(|b| (b.min, b.max, b.pos))
}

fn redraw(b: &Bar) {
    unsafe { WinInvalidateRect(b.pm, core::ptr::null(), 0) };
}

pub fn set_pos(h: u16, which: u16, pos: i32) -> i32 {
    let Some(b) = bar(h, which) else { return 0 };
    let old = b.pos;
    let (lo, hi) = (b.min.min(b.max), b.min.max(b.max));
    b.pos = pos.clamp(lo, hi);
    if b.pos != old {
        redraw(b);
    }
    old
}

pub fn set_range(h: u16, which: u16, lo: i32, hi: i32) {
    let Some(b) = bar(h, which) else { return };
    b.min = lo;
    b.max = hi;
    b.pos = b.pos.clamp(lo.min(hi), lo.max(hi));
    redraw(b);
    // A window's scroll bar with an empty range is hidden.
    if which != SB_CTL {
        show(h, which, lo != hi);
    }
}

/// SetScrollRange that keeps the bar (an empty range disables it).
pub fn set_range_keep(h: u16, which: u16, lo: i32, hi: i32) {
    let Some(b) = bar(h, which) else { return };
    if (b.min, b.max) == (lo, hi) {
        return;
    }
    b.min = lo;
    b.max = hi;
    b.pos = b.pos.clamp(lo.min(hi), lo.max(hi));
    redraw(b);
}

pub fn show(h: u16, which: u16, on: bool) {
    if which == SB_BOTH {
        show(h, SB_HORZ, on);
        show(h, SB_VERT, on);
        return;
    }
    if which == SB_CTL {
        user::show_window(h, if on { 5 } else { 0 });
        return;
    }
    let Some(b) = bar(h, which) else { return };
    if b.shown == on {
        return;
    }
    b.shown = on;
    let (pm, vert) = (b.pm, b.vert);
    let Some((framed, f)) = wnd(h).map(|w| (w.framed, w.frame)) else { return };
    unsafe {
        WinShowWindow(pm, on as u32);
        if framed {
            // The frame places the scroll bars it finds by ID.
            let id = if !on { 0 } else if vert { FID_VERTSCROLL as u16 } else { FID_HORZSCROLL as u16 };
            WinSetWindowUShort(pm, QWS_ID, id);
            WinSendMsg(f, WM_UPDATEFRAME, 0, 0);
        }
    }
    if !framed {
        user::layout_frame(h);
    }
}

pub fn enable(h: u16, which: u16, fl: u16) -> bool {
    if which == SB_BOTH {
        return enable(h, SB_HORZ, fl) | enable(h, SB_VERT, fl);
    }
    let Some(b) = bar(h, which) else { return false };
    let changed = b.disabled != fl;
    b.disabled = fl;
    redraw(b);
    changed
}

/// Geometry along the bar: (length, thickness, arrow size, thumb start).
fn geometry(b: &Bar, cx: i32, cy: i32) -> (i32, i32, i32, Option<i32>) {
    let (len, th) = if b.vert { (cy, cx) } else { (cx, cy) };
    let arrow = th.min(len / 2);
    let track = len - 2 * arrow;
    let thumb = if b.max > b.min && track >= arrow + 2 && b.disabled != 3 {
        let span = track - arrow;
        Some(arrow + ((b.pos - b.min) as i64 * span as i64 / (b.max - b.min) as i64) as i32)
    } else {
        None
    };
    (len, th, arrow, thumb)
}

fn part_at(b: &Bar, cx: i32, cy: i32, x: i32, y: i32) -> Option<Part> {
    let (len, _, arrow, thumb) = geometry(b, cx, cy);
    let p = if b.vert { y } else { x };
    Some(if p < arrow {
        Part::LineUp
    } else if p >= len - arrow {
        Part::LineDown
    } else {
        match thumb {
            Some(t) if p < t => Part::PageUp,
            Some(t) if p < t + arrow => Part::Thumb,
            Some(_) => Part::PageDown,
            None => return None,
        }
    })
}

/// A raised box (button face, white top-left, shadow bottom-right).
pub fn raised(hdc: u16, r: [i32; 4], pressed: bool) {
    let [l, t, rr, b] = r;
    gdi::fill(hdc, r, gdi::sys_color(gdi::COLOR_BTNFACE));
    if pressed {
        gdi::fill(hdc, [l, t, rr, t + 1], gdi::sys_color(gdi::COLOR_BTNSHADOW));
        gdi::fill(hdc, [l, t, l + 1, b], gdi::sys_color(gdi::COLOR_BTNSHADOW));
        return;
    }
    gdi::fill(hdc, [l, t, rr - 1, t + 1], 0xffffff);
    gdi::fill(hdc, [l, t, l + 1, b - 1], 0xffffff);
    gdi::fill(hdc, [l, b - 1, rr, b], gdi::sys_color(gdi::COLOR_BTNSHADOW));
    gdi::fill(hdc, [rr - 1, t, rr, b], gdi::sys_color(gdi::COLOR_BTNSHADOW));
    if rr - l > 6 && b - t > 6 {
        gdi::fill(hdc, [l + 1, b - 2, rr - 1, b - 1], gdi::sys_color(gdi::COLOR_BTNSHADOW));
        gdi::fill(hdc, [rr - 2, t + 1, rr - 1, b - 1], gdi::sys_color(gdi::COLOR_BTNSHADOW));
    }
}

/// A solid triangle pointing up, down, left or right, centred in `r`.
pub fn triangle(hdc: u16, r: [i32; 4], dir: u8, c: u32) {
    let (cx, cy) = ((r[0] + r[2]) / 2, (r[1] + r[3]) / 2);
    let n = ((r[2] - r[0]).min(r[3] - r[1]) / 4).max(2);
    for i in 0..n {
        let k = match dir {
            b'u' | b'l' => i,
            _ => n - 1 - i,
        };
        match dir {
            b'u' | b'd' => {
                let y = cy - n / 2 + i;
                gdi::fill(hdc, [cx - k, y, cx + k + 1, y + 1], c);
            }
            _ => {
                let x = cx - n / 2 + i;
                gdi::fill(hdc, [x, cy - k, x + 1, cy + k + 1], c);
            }
        }
    }
}

/// Draw a scroll bar on a DC of its window (cx x cy).
pub fn paint(b: &Bar, hdc: u16, cx: i32, cy: i32, track_brush: Option<u16>) {
    let (len, th, arrow, thumb) = geometry(b, cx, cy);
    let r = |a: i32, z: i32| if b.vert { [0, a, th, z] } else { [a, 0, z, th] };
    let black = 0;
    // The track.
    match track_brush {
        Some(br) => gdi::fill_rect(hdc, r(arrow, len - arrow), br),
        None => gdi::fill(hdc, r(arrow, len - arrow), gdi::sys_color(gdi::COLOR_SCROLLBAR)),
    }
    if let (Some(Part::PageUp), Some(t)) = (b.pressed, thumb) {
        gdi::fill(hdc, r(arrow, t), 0);
    }
    if let (Some(Part::PageDown), Some(t)) = (b.pressed, thumb) {
        gdi::fill(hdc, r(t + arrow, len - arrow), 0);
    }
    gdi::frame(hdc, [0, 0, cx, cy], black);
    // The arrows.
    let dis = |bit: u16| if b.disabled & bit != 0 || b.max <= b.min { gdi::sys_color(gdi::COLOR_GRAYTEXT) } else { 0 };
    let up = r(0, arrow);
    let down = r(len - arrow, len);
    for (rc, dir, part, bit) in [(up, if b.vert { b'u' } else { b'l' }, Part::LineUp, 1), (down, if b.vert { b'd' } else { b'r' }, Part::LineDown, 2)] {
        gdi::frame(hdc, rc, black);
        let inner = [rc[0] + 1, rc[1] + 1, rc[2] - 1, rc[3] - 1];
        let pressed = b.pressed == Some(part);
        raised(hdc, inner, pressed);
        let off = pressed as i32;
        triangle(hdc, [inner[0] + off, inner[1] + off, inner[2] + off, inner[3] + off], dir, dis(bit));
    }
    // The thumb.
    if let Some(t) = thumb {
        let rc = r(t, t + arrow);
        gdi::frame(hdc, rc, black);
        raised(hdc, [rc[0] + 1, rc[1] + 1, rc[2] - 1, rc[3] - 1], false);
    }
}

/// The window and bar a `Win16Sb` PM window belongs to.
fn owner_of(pm: u32) -> Option<(u16, u16)> {
    let h = user::hwnd_above(unsafe { WinQueryWindow(pm, QW_PARENT) });
    let w = wnd(h)?;
    if w.vsb.as_ref().is_some_and(|b| b.pm == pm) {
        return Some((h, SB_VERT));
    }
    if w.hsb.as_ref().is_some_and(|b| b.pm == pm) {
        return Some((h, SB_HORZ));
    }
    None
}

/// Send a scroll code to the bar's owner.
fn sb_notify(h: u16, which: u16, code: u16, pos: i32) {
    let Some(vert) = bar(h, which).map(|b| b.vert) else { return };
    let msg = if vert { W_VSCROLL } else { W_HSCROLL };
    if which == SB_CTL {
        let parent = wnd(h).map_or(0, |w| w.parent);
        user::send(parent, msg, code, (h as u32) << 16 | pos as u16 as u32);
    } else {
        user::send(h, msg, code, pos as u16 as u32);
    }
}

fn code_of(p: Part) -> u16 {
    match p {
        Part::LineUp => SB_LINEUP,
        Part::LineDown => SB_LINEDOWN,
        Part::PageUp => SB_PAGEUP,
        Part::PageDown => SB_PAGEDOWN,
        Part::Thumb => SB_THUMBTRACK,
    }
}

/// Track a click at (x, y) (PM coordinates of the bar's window, y up)
/// until the button goes up.
pub fn track(h: u16, which: u16, x: i32, y: i32) {
    let Some(b) = bar(h, which) else { return };
    let pm = b.pm;
    if b.max <= b.min || b.disabled == 3 {
        return;
    }
    let mut rc = RectL::default();
    unsafe { WinQueryWindowRect(pm, &mut rc) };
    let (cx, cy) = (rc.r, rc.t);
    // Windows' y grows down.
    let wy = |y: i32| cy - 1 - y;
    let Some(part) = part_at(b, cx, cy, x, wy(y)) else { return };
    if (part == Part::LineUp && b.disabled & 1 != 0) || (part == Part::LineDown && b.disabled & 2 != 0) {
        return;
    }
    let hab = TASK.get().hab;
    unsafe { WinSetCapture(HWND_DESKTOP, pm) };
    let (_, _, arrow, thumb) = geometry(b, cx, cy);
    let grab = if b.vert { wy(y) } else { x } - thumb.unwrap_or(0);
    b.pressed = Some(part);
    redraw(b);
    let mut tracked = b.pos;
    if part != Part::Thumb {
        sb_notify(h, which, code_of(part), 0);
        unsafe { WinStartTimer(hab, pm, TID_REPEAT, 350) };
    }
    let mut inside = true;
    loop {
        let mut q = QMsg::default();
        if unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } == 0 {
            // WM_QUIT: back in the queue for the program.
            unsafe { WinPostQueueMsg(TASK.get().hmq, WM_QUIT, q.mp1, q.mp2) };
            break;
        }
        if q.hwnd != pm {
            if q.msg != WM_MOUSEMOVE && !(0x71..=0x79).contains(&q.msg) && q.msg != WM_CHAR {
                unsafe { WinDispatchMsg(hab, &q) };
            }
            continue;
        }
        let (mx, my) = (q.mp1 as i16 as i32, (q.mp1 >> 16) as i16 as i32);
        match q.msg {
            WM_MOUSEMOVE => {
                let Some(b) = bar(h, which) else { break };
                if part == Part::Thumb {
                    let (len, _, _, _) = geometry(b, cx, cy);
                    let p = if b.vert { wy(my) } else { mx } - grab;
                    let span = (len - 3 * arrow).max(1);
                    let pos = b.min + ((p - arrow).clamp(0, span) as i64 * (b.max - b.min) as i64 / span as i64) as i32;
                    if pos != tracked {
                        tracked = pos;
                        sb_notify(h, which, SB_THUMBTRACK, pos);
                    }
                } else {
                    let now = part_at(b, cx, cy, mx, wy(my)) == Some(part);
                    if now != inside {
                        inside = now;
                        b.pressed = now.then_some(part);
                        redraw(b);
                    }
                }
            }
            WM_TIMER if q.mp1 & 0xffff == TID_REPEAT => {
                unsafe { WinStartTimer(hab, pm, TID_REPEAT, 50) };
                let Some(b) = bar(h, which) else { break };
                // A page area repeats until the thumb reaches the pointer.
                let still = part_at(b, cx, cy, mx_last(pm).0, wy(mx_last(pm).1)) == Some(part);
                if inside && still {
                    sb_notify(h, which, code_of(part), 0);
                }
            }
            WM_BUTTON1UP => break,
            _ => {}
        }
    }
    unsafe {
        WinStopTimer(hab, pm, TID_REPEAT);
        WinSetCapture(HWND_DESKTOP, 0);
    }
    if part == Part::Thumb {
        sb_notify(h, which, SB_THUMBPOSITION, tracked);
    }
    if let Some(b) = bar(h, which) {
        b.pressed = None;
        redraw(b);
    }
    sb_notify(h, which, SB_ENDSCROLL, 0);
}

/// The pointer in a window's coordinates (PM's, y up).
fn mx_last(pm: u32) -> (i32, i32) {
    let mut p = PointL::default();
    unsafe {
        WinQueryPointerPos(HWND_DESKTOP, &mut p);
        WinMapWindowPoints(HWND_DESKTOP, pm, &mut p, 1);
    }
    (p.x, p.y)
}

/// The PM procedure of the window scroll bars.
extern "C" fn sb_proc(pmh: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let hps = unsafe { WinBeginPaint(pmh, 0, core::ptr::null_mut()) };
            let mut r = RectL::default();
            unsafe { WinQueryWindowRect(pmh, &mut r) };
            let hdc = gdi::new_dc(hps, pmh, r.t, gdi::DcKind::Paint);
            match owner_of(pmh).and_then(|(h, w)| bar(h, w)) {
                Some(b) => paint(b, hdc, r.r, r.t, None),
                None => gdi::fill(hdc, [0, 0, r.r, r.t], gdi::sys_color(gdi::COLOR_SCROLLBAR)),
            }
            gdi::release_dc(hdc);
            0
        }
        0x71 | 0x73 => {
            if let Some((h, w)) = owner_of(pmh) {
                track(h, w, mp1 as i16 as i32, (mp1 >> 16) as i16 as i32);
            }
            1
        }
        _ => unsafe { WinDefWindowProc(pmh, msg, mp1, mp2) },
    }
}

/// The SCROLLBAR control's window procedure.
pub fn control_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    use crate::user::*;
    match m {
        W_CREATE => {
            let style = wnd(h).map_or(0, |w| w.style);
            let pm = wnd(h).map_or(0, |w| w.client);
            if let Some(w) = wnd(h) {
                w.data = Some(alloc::boxed::Box::new(new_bar(pm, style & SBS_VERT != 0, 0, 0)));
            }
            0
        }
        W_ERASEBKGND => 1,
        W_PAINT => {
            let hdc = begin_paint_rs(h);
            let (cx, cy) = client_size(h);
            let parent = wnd(h).map_or(0, |w| w.parent);
            let br = send(parent, W_CTLCOLOR, hdc, 5 << 16 | h as u32) as u16;
            if let Some(b) = bar(h, SB_CTL) {
                paint(b, hdc, cx, cy, (br != 0).then_some(br));
            }
            gdi::release_dc(hdc);
            0
        }
        W_LBUTTONDOWN | W_LBUTTONDBLCLK => {
            let (cx, cy) = client_size(h);
            let _ = cx;
            let (x, y) = (lp as i16 as i32, (lp >> 16) as i16 as i32);
            if wnd(h).is_some_and(|w| w.style & WS_TABSTOP != 0) {
                set_focus(h);
            }
            track(h, SB_CTL, x, cy - 1 - y);
            0
        }
        W_KEYDOWN => {
            let vert = bar(h, SB_CTL).is_some_and(|b| b.vert);
            let code = match (wp, vert) {
                (0x26, true) | (0x25, false) => SB_LINEUP,
                (0x28, true) | (0x27, false) => SB_LINEDOWN,
                (0x21, _) => SB_PAGEUP,
                (0x22, _) => SB_PAGEDOWN,
                (0x24, _) => SB_TOP,
                (0x23, _) => SB_BOTTOM,
                _ => return 0,
            };
            sb_notify(h, SB_CTL, code, 0);
            sb_notify(h, SB_CTL, SB_ENDSCROLL, 0);
            0
        }
        W_KEYUP => 0,
        W_GETDLGCODE => 0,
        W_ENABLE => {
            invalidate(h, None);
            0
        }
        _ => def_window_proc(h, m, wp, lp),
    }
}
