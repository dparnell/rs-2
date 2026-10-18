//! Scroll bars (WC_SCROLLBAR): arrows, a shaft and a proportional slider;
//! they tell their owner with WM_VSCROLL/WM_HSCROLL, which sets the
//! position with SBM_SETPOS.

use gpi::{Point, Rect};

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc};
use crate::state::st;
use crate::win::{WM_BUTTON1DOWN, WM_BUTTON1UP, WM_MOUSEMOVE, WM_PAINT};

const WM_CREATE: u32 = 0x0001;
const SBS_VERT: u32 = 0x0001;

const SBM_SETSCROLLBAR: u32 = 0x01a0;
const SBM_SETPOS: u32 = 0x01a1;
const SBM_QUERYPOS: u32 = 0x01a2;
const SBM_QUERYRANGE: u32 = 0x01a3;
const SBM_SETTHUMBSIZE: u32 = 0x01a6;

const SB_LINEUP: u32 = 1;
const SB_LINEDOWN: u32 = 2;
const SB_PAGEUP: u32 = 3;
const SB_PAGEDOWN: u32 = 4;
const SB_SLIDERTRACK: u32 = 5;
const SB_SLIDERPOSITION: u32 = 6;
const SB_ENDSCROLL: u32 = 7;

wndproc! {
    scrollbar_entry => scrollbar_proc;
}

#[derive(Default)]
struct Scroll {
    pos: i32,
    first: i32,
    last: i32,
    visible: i32,
    total: i32,
    /// Dragging the slider: pointer offset into it.
    drag: Option<i32>,
    track_pos: i32,
}

fn data(h: u32) -> &'static mut Scroll {
    if ctl::data::<Scroll>(h).is_none() {
        ctl::set_data(h, Scroll::default());
    }
    ctl::data::<Scroll>(h).expect("scroll bar data")
}

fn vertical(h: u32) -> bool {
    ctl::style(h) & SBS_VERT != 0
}

/// Geometry along the bar: (length, arrow size), measured from the
/// top (vertical) or left (horizontal).
fn dims(h: u32) -> (i32, i32) {
    let (cx, cy) = ctl::size(h);
    if vertical(h) { (cy, cx.min(cy / 2)) } else { (cx, cy.min(cx / 2)) }
}

/// Slider start and size along the bar (from the top/left).
fn slider(h: u32) -> (i32, i32) {
    let (len, a) = dims(h);
    let s = data(h);
    let shaft = (len - 2 * a).max(0);
    let range = (s.last - s.first).max(0);
    let size = if s.visible > 0 && s.total > 0 {
        (shaft as i64 * s.visible as i64 / s.total.max(s.visible) as i64) as i32
    } else {
        a
    }
    .clamp(8.min(shaft), shaft);
    let pos = if s.drag.is_some() { s.track_pos } else { s.pos };
    let off = if range > 0 { ((pos - s.first) as i64 * (shaft - size) as i64 / range as i64) as i32 } else { 0 };
    (a + off, size)
}

/// Convert a rectangle given along/across the bar into window space.
fn to_rect(h: u32, start: i32, size: i32) -> Rect {
    let (cx, cy) = ctl::size(h);
    if vertical(h) { Rect::new(0, cy - start - size, cx, cy - start) } else { Rect::new(start, 0, start + size, cy) }
}

/// Distance along the bar from the top/left.
fn along(h: u32, p: Point) -> i32 {
    let (_, cy) = ctl::size(h);
    if vertical(h) { cy - 1 - p.y } else { p.x }
}

fn notify(h: u32, cmd: u32, pos: i32) {
    let msg = if vertical(h) { WM_VSCROLL } else { WM_HSCROLL };
    send(ctl::owner(h), msg, ctl::id(h), mp(pos as u32, cmd));
}

fn arrow(c: &mut Canvas, r: Rect, dir: u8, enabled: bool) {
    c.fill(r, draw::FACE);
    c.raised(r);
    let col = if enabled { draw::TEXT } else { draw::SHADOW };
    let (cx, cy) = (r.left + r.width() / 2, r.bottom + r.height() / 2);
    for i in 0..4 {
        let rr = match dir {
            0 => Rect::new(cx - i, cy + 2 - i, cx + i + 1, cy + 3 - i),
            1 => Rect::new(cx - i, cy - 2 + i, cx + i + 1, cy - 1 + i),
            2 => Rect::new(cx - 2 + i, cy - i, cx - 1 + i, cy + i + 1),
            _ => Rect::new(cx + 2 - i, cy - i, cx + 3 - i, cy + i + 1),
        };
        c.fill(rr, col);
    }
}

fn paint(h: u32) {
    let (len, a) = dims(h);
    let (ss, sz) = slider(h);
    let v = vertical(h);
    let enabled = ctl::enabled(h) && data(h).last > data(h).first;
    let r1 = to_rect(h, 0, a);
    let r2 = to_rect(h, len - a, a);
    let rs = to_rect(h, ss, sz);
    draw::paint(h, |c| {
        c.fill(c.rect(), 0xe0e0e0);
        arrow(c, r1, if v { 0 } else { 2 }, enabled);
        arrow(c, r2, if v { 1 } else { 3 }, enabled);
        if enabled {
            c.fill(rs, draw::FACE);
            c.raised(rs);
        }
    });
}

fn set_pos(h: u32, pos: i32) {
    let s = data(h);
    let p = pos.clamp(s.first.min(s.last), s.last.max(s.first));
    if p != s.pos {
        s.pos = p;
        ctl::invalidate(h);
    }
}

pub extern "C" fn scrollbar_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            ctl::set_data(h, Scroll::default());
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON1DBLCLK => {
            if !ctl::enabled(h) || data(h).last <= data(h).first {
                return 1;
            }
            let (len, a) = dims(h);
            let d = along(h, ctl::mouse_pos(mp1));
            let (ss, sz) = slider(h);
            let pos = data(h).pos;
            if d < a {
                notify(h, SB_LINEUP, pos);
            } else if d >= len - a {
                notify(h, SB_LINEDOWN, pos);
            } else if d < ss {
                notify(h, SB_PAGEUP, pos);
            } else if d >= ss + sz {
                notify(h, SB_PAGEDOWN, pos);
            } else {
                let s = data(h);
                s.drag = Some(d - ss);
                s.track_pos = s.pos;
                st().capture = h;
                return 1;
            }
            notify(h, SB_ENDSCROLL, data(h).pos);
            1
        }
        WM_MOUSEMOVE => {
            let Some(off) = data(h).drag else { return 1 };
            let (len, a) = dims(h);
            let (_, sz) = slider(h);
            let shaft = (len - 2 * a - sz).max(1);
            let d = along(h, ctl::mouse_pos(mp1)) - off - a;
            let s = data(h);
            let p = s.first + ((d.clamp(0, shaft) as i64 * (s.last - s.first) as i64 + shaft as i64 / 2) / shaft as i64) as i32;
            if p != s.track_pos {
                s.track_pos = p;
                ctl::invalidate(h);
                notify(h, SB_SLIDERTRACK, p);
            }
            1
        }
        WM_BUTTON1UP => {
            let s = data(h);
            if s.drag.take().is_some() {
                let p = s.track_pos;
                if st().capture == h {
                    st().capture = 0;
                }
                notify(h, SB_SLIDERPOSITION, p);
                notify(h, SB_ENDSCROLL, p);
                ctl::invalidate(h);
            }
            1
        }
        WM_QUERYDLGCODE => DLGC_SCROLLBAR,
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        SBM_SETSCROLLBAR => {
            let s = data(h);
            s.first = lo(mp2) as i16 as i32;
            s.last = hi(mp2) as i16 as i32;
            s.pos = -1;
            set_pos(h, mp1 as i16 as i32);
            ctl::invalidate(h);
            1
        }
        SBM_SETPOS => {
            set_pos(h, mp1 as i16 as i32);
            1
        }
        SBM_QUERYPOS => data(h).pos as u32 & 0xffff,
        SBM_QUERYRANGE => mp(data(h).first as u32, data(h).last as u32),
        SBM_SETTHUMBSIZE => {
            let s = data(h);
            s.visible = lo(mp1) as i32;
            s.total = hi(mp1) as i32;
            ctl::invalidate(h);
            1
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
