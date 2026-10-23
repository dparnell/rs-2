//! Sliders (WC_SLIDER): a shaft with an arm that moves over a scale of
//! increments, tick marks with optional text, detents, optional arrow
//! buttons and a ribbon strip; SLM_* messages and SLN_* notifications.
//! The scale arithmetic is `pmwin::controls::Scale`.

use alloc::vec::Vec;

use gpi::{Point, Rect};
use pmwin::controls::Scale;

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::WinDefWindowProc;
use crate::state::st;
use crate::win::{WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_MOUSEMOVE, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;

const SLS_VERTICAL: u32 = 0x0001;
const SLS_BOTTOM: u32 = 0x0002;
const SLS_TOP: u32 = 0x0004;
const SLS_SNAPTOINCREMENT: u32 = 0x0008;
const SLS_BUTTONSBOTTOM: u32 = 0x0010;
const SLS_BUTTONSTOP: u32 = 0x0020;
const SLS_READONLY: u32 = 0x0080;
const SLS_RIBBONSTRIP: u32 = 0x0100;
const SLS_HOMETOP: u32 = 0x0200;

const SLM_ADDDETENT: u32 = 0x0369;
const SLM_QUERYDETENTPOS: u32 = 0x036a;
const SLM_QUERYSCALETEXT: u32 = 0x036b;
const SLM_QUERYSLIDERINFO: u32 = 0x036c;
const SLM_QUERYTICKPOS: u32 = 0x036d;
const SLM_QUERYTICKSIZE: u32 = 0x036e;
const SLM_REMOVEDETENT: u32 = 0x036f;
const SLM_SETSCALETEXT: u32 = 0x0370;
const SLM_SETSLIDERINFO: u32 = 0x0371;
const SLM_SETTICKSIZE: u32 = 0x0372;

const SLN_CHANGE: u32 = 1;
const SLN_SLIDERTRACK: u32 = 2;
const SLN_SETFOCUS: u32 = 3;
const SLN_KILLFOCUS: u32 = 4;

const SMA_SHAFTDIMENSIONS: u32 = 0;
const SMA_SHAFTPOSITION: u32 = 1;
const SMA_SLIDERARMDIMENSIONS: u32 = 2;
const SMA_SLIDERARMPOSITION: u32 = 3;
const SMA_INCREMENTVALUE: u32 = 1;
const SMA_SETALLTICKS: u32 = 0xffff;

/// Arm size along and across the shaft; shaft thickness; button size.
const ARM_ALONG: i32 = 10;
const ARM_ACROSS: i32 = 20;
const SHAFT: i32 = 6;
const BUTTON: i32 = 16;

wndproc! {
    slider_entry => slider_proc;
}

struct Slider {
    increments: u32,
    /// Increment the arm is at.
    pos: u32,
    ticks: Vec<u8>,
    texts: Vec<Vec<u8>>,
    /// Detents: (id, pel offset from home).
    detents: Vec<(u32, i32)>,
    next_detent: u32,
    /// Dragging the arm: pointer offset from the arm centre.
    drag: Option<i32>,
    /// Arm position while dragging (pels from home).
    track: i32,
}

fn data(h: u32) -> Option<&'static mut Slider> {
    ctl::data::<Slider>(h)
}

fn vertical(h: u32) -> bool {
    ctl::style(h) & SLS_VERTICAL != 0
}

fn buttons(h: u32) -> u32 {
    ctl::style(h) & (SLS_BUTTONSBOTTOM | SLS_BUTTONSTOP)
}

/// Window length along the shaft and across it.
fn dims(h: u32) -> (i32, i32) {
    let (cx, cy) = ctl::size(h);
    if vertical(h) { (cy, cx) } else { (cx, cy) }
}

/// The shaft: start and length along, centre across.
fn shaft(h: u32) -> (i32, i32, i32) {
    let (len, across) = dims(h);
    let style = ctl::style(h);
    let b = if buttons(h) != 0 { 2 * BUTTON + 2 } else { 0 };
    let start = 4 + if style & SLS_BUTTONSBOTTOM != 0 { b } else { 0 };
    let l = (len - 8 - b).max(ARM_ALONG);
    // Text above the shaft (horizontal) leaves room for it.
    let centre = if style & SLS_BOTTOM != 0 {
        ARM_ACROSS / 2 + 2
    } else if style & SLS_TOP != 0 {
        across - ARM_ACROSS / 2 - 2
    } else {
        across / 2
    };
    (start, l, centre)
}

fn scale(h: u32) -> Scale {
    let (_, l, _) = shaft(h);
    Scale { increments: data(h).map_or(1, |d| d.increments.max(1)), len: l, arm: ARM_ALONG }
}

/// Home is at the bottom/left unless SLS_HOMETOP/SLS_HOMERIGHT.
fn home_far(h: u32) -> bool {
    ctl::style(h) & SLS_HOMETOP != 0
}

/// Window point for `along` pels from home and `across`.
fn point(h: u32, along: i32, across: i32) -> Point {
    let (start, l, _) = shaft(h);
    let a = if home_far(h) { start + l - along } else { start + along };
    if vertical(h) { Point::new(across, a) } else { Point::new(a, across) }
}

/// Pels from home of a window point.
fn along_of(h: u32, p: Point) -> i32 {
    let (start, l, _) = shaft(h);
    let a = if vertical(h) { p.y } else { p.x } - start;
    if home_far(h) { l - a } else { a }
}

fn rect_at(h: u32, along: i32, across: i32, w_along: i32, w_across: i32) -> Rect {
    let c = point(h, along, across);
    if vertical(h) {
        Rect::new(c.x - w_across / 2, c.y - w_along / 2, c.x - w_across / 2 + w_across, c.y - w_along / 2 + w_along)
    } else {
        Rect::new(c.x - w_along / 2, c.y - w_across / 2, c.x - w_along / 2 + w_along, c.y - w_across / 2 + w_across)
    }
}

/// Arm centre (pels from home).
fn arm_at(h: u32) -> i32 {
    let sc = scale(h);
    match data(h) {
        Some(d) if d.drag.is_some() => d.track,
        Some(d) => sc.pos(d.pos),
        None => 0,
    }
}

fn set_pos(h: u32, i: u32, notify: bool) {
    let Some(d) = data(h) else { return };
    let i = i.min(d.increments.saturating_sub(1));
    if d.pos == i {
        return;
    }
    d.pos = i;
    ctl::invalidate(h);
    if notify {
        let x = scale(h).pos(i) as u32;
        crate::msg::send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), SLN_CHANGE), x);
    }
}

/// The two arrow buttons: (decrement, increment) rectangles.
fn button_rects(h: u32) -> Option<(Rect, Rect)> {
    if buttons(h) == 0 {
        return None;
    }
    let (len, _) = dims(h);
    let (_, _, centre) = shaft(h);
    let first = if ctl::style(h) & SLS_BUTTONSBOTTOM != 0 { 2 } else { len - 2 - 2 * BUTTON };
    let mk = |a: i32| {
        if vertical(h) {
            Rect::new(centre - BUTTON / 2, a, centre - BUTTON / 2 + BUTTON, a + BUTTON)
        } else {
            Rect::new(a, centre - BUTTON / 2, a + BUTTON, centre - BUTTON / 2 + BUTTON)
        }
    };
    let (r1, r2) = (mk(first), mk(first + BUTTON));
    // Decrement towards home.
    Some(if home_far(h) { (r2, r1) } else { (r1, r2) })
}

fn paint(h: u32) {
    let focus = ctl::has_focus(h);
    let enabled = ctl::enabled(h);
    let style = ctl::style(h);
    let (start, l, centre) = shaft(h);
    let sc = scale(h);
    let arm = arm_at(h);
    let Some(d) = data(h) else { return };
    let ticks = d.ticks.clone();
    let texts = d.texts.clone();
    let detents = d.detents.clone();
    let shaft_r = if vertical(h) {
        Rect::new(centre - SHAFT / 2, start, centre + SHAFT / 2, start + l)
    } else {
        Rect::new(start, centre - SHAFT / 2, start + l, centre + SHAFT / 2)
    };
    let ribbon = if style & SLS_RIBBONSTRIP != 0 {
        let a = point(h, 0, centre);
        let b = point(h, arm, centre);
        Some(if vertical(h) {
            Rect::new(centre - SHAFT / 2 + 1, a.y.min(b.y), centre + SHAFT / 2 - 1, a.y.max(b.y))
        } else {
            Rect::new(a.x.min(b.x), centre - SHAFT / 2 + 1, a.x.max(b.x), centre + SHAFT / 2 - 1)
        })
    } else {
        None
    };
    let mut tick_rects = Vec::new();
    let mut labels = Vec::new();
    let f = draw::font_of(h);
    for i in 0..sc.increments {
        let t = ticks.get(i as usize).copied().unwrap_or(0) as i32;
        let along = sc.pos(i);
        if t > 0 {
            let r = rect_at(h, along, centre - ARM_ACROSS / 2 - 2 - t / 2, 1, t);
            tick_rects.push(r);
        }
        if let Some(txt) = texts.get(i as usize).filter(|t| !t.is_empty()) {
            let c = point(h, along, centre + ARM_ACROSS / 2 + 2);
            let w = draw::font_of(h).text_width(txt);
            let (x, y) = if vertical(h) { (c.x + 2, c.y - f.height / 2) } else { (c.x - w / 2, c.y) };
            labels.push((x, y, txt.clone()));
        }
    }
    let detent_rects: Vec<Rect> = detents.iter().map(|&(_, a)| rect_at(h, a, centre + ARM_ACROSS / 2 + 1, 3, 3)).collect();
    let arm_r = rect_at(h, arm, centre, ARM_ALONG, ARM_ACROSS);
    let btns = button_rects(h);
    draw::paint(h, |c: &mut Canvas| {
        let r = c.rect();
        c.fill(r, draw::dialog_bg());
        c.fill(shaft_r, if enabled { 0xa0a0a0 } else { draw::face() });
        c.sunken(shaft_r);
        if let Some(rr) = ribbon {
            c.fill(rr, draw::hilite());
        }
        for t in &tick_rects {
            c.fill(*t, draw::text());
        }
        for d in &detent_rects {
            c.fill(*d, draw::text());
        }
        let clip = c.rect();
        for (x, y, t) in &labels {
            c.text(*x, *y + f.descender, t, draw::text(), clip);
        }
        c.fill(arm_r, draw::face());
        c.raised(arm_r);
        if let Some((a, b)) = btns {
            for (br, dec) in [(a, true), (b, false)] {
                c.fill(br, draw::face());
                c.raised(br);
                let (mx, my) = (br.left + br.width() / 2, br.bottom + br.height() / 2);
                let col = if enabled { draw::text() } else { draw::shadow() };
                let toward_low = dec != home_far(h);
                for i in 0..4 {
                    let rr = if vertical(h) {
                        let y = if toward_low { my - 2 + i } else { my + 2 - i };
                        Rect::new(mx - i, y, mx + i + 1, y + 1)
                    } else {
                        let x = if toward_low { mx - 2 + i } else { mx + 2 - i };
                        Rect::new(x, my - i, x + 1, my + i + 1)
                    };
                    c.fill(rr, col);
                }
            }
        }
        if focus {
            c.focus_rect(Rect::new(r.left, r.bottom, r.right, r.top));
        }
    });
}

pub extern "C" fn slider_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            // SLDCDATA: cbSize, usScale1Increments, usScale1Spacing, ...
            let inc = if mp1 != 0 { (unsafe { ((mp1 + 4) as *const u16).read_unaligned() }) as u32 } else { 2 };
            let n = inc.max(1);
            ctl::set_data(
                h,
                Slider { increments: n, pos: 0, ticks: alloc::vec![0; n as usize], texts: alloc::vec![Vec::new(); n as usize], detents: Vec::new(), next_detent: 1, drag: None, track: 0 },
            );
            0
        }
        WM_SIZE => {
            ctl::invalidate(h);
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
            crate::win::WinSetFocus(1, h);
            if ctl::style(h) & SLS_READONLY != 0 {
                return 1;
            }
            let p = ctl::mouse_pos(mp1);
            let pos = data(h).map_or(0, |d| d.pos);
            if let Some((dec, inc)) = button_rects(h) {
                if dec.contains(p) {
                    set_pos(h, pos.saturating_sub(1), true);
                    return 1;
                }
                if inc.contains(p) {
                    set_pos(h, pos + 1, true);
                    return 1;
                }
            }
            let a = along_of(h, p);
            let arm = arm_at(h);
            if (a - arm).abs() <= ARM_ALONG / 2 + 1 {
                if let Some(d) = data(h) {
                    d.drag = Some(a - arm);
                    d.track = arm;
                }
                st().capture = h;
            } else if a < arm {
                set_pos(h, pos.saturating_sub(1), true);
            } else {
                set_pos(h, pos + 1, true);
            }
            1
        }
        WM_MOUSEMOVE => {
            let Some(off) = data(h).and_then(|d| d.drag) else { return WinDefWindowProc(h, msg, mp1, mp2) };
            let sc = scale(h);
            let a = (along_of(h, ctl::mouse_pos(mp1)) - off).clamp(sc.arm / 2, sc.len - sc.arm / 2);
            let snap = ctl::style(h) & SLS_SNAPTOINCREMENT != 0;
            let a = if snap { sc.pos(sc.index_at(a)) } else { a };
            if let Some(d) = data(h) {
                if d.track != a {
                    d.track = a;
                    ctl::invalidate(h);
                    crate::msg::send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), SLN_SLIDERTRACK), a as u32);
                }
            }
            1
        }
        WM_BUTTON1UP => {
            let Some(d) = data(h) else { return 1 };
            if d.drag.take().is_some() {
                let i = scale(h).index_at(d.track);
                if st().capture == h {
                    st().capture = 0;
                }
                ctl::invalidate(h);
                let old = data(h).map_or(0, |d| d.pos);
                if old != i {
                    set_pos(h, i, true);
                }
            }
            1
        }
        WM_CHAR => {
            let k = Key::from(mp1, mp2);
            if k.up() || ctl::style(h) & SLS_READONLY != 0 {
                return WinDefWindowProc(h, msg, mp1, mp2);
            }
            let (pos, n) = data(h).map_or((0, 1), |d| (d.pos, d.increments));
            let v = vertical(h);
            let far = home_far(h);
            let (up, down) = if v { (VK_UP, VK_DOWN) } else { (VK_RIGHT, VK_LEFT) };
            match k.vk {
                x if x == up => set_pos(h, if far { pos.saturating_sub(1) } else { pos + 1 }, true),
                x if x == down => set_pos(h, if far { pos + 1 } else { pos.saturating_sub(1) }, true),
                VK_HOME => set_pos(h, 0, true),
                VK_END => set_pos(h, n.saturating_sub(1), true),
                _ => return WinDefWindowProc(h, msg, mp1, mp2),
            }
            1
        }
        WM_SETFOCUS => {
            ctl::invalidate(h);
            ctl::notify(h, if mp2 & 0xffff != 0 { SLN_SETFOCUS } else { SLN_KILLFOCUS });
            0
        }
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        WM_QUERYDLGCODE => 0,
        SLM_SETSLIDERINFO => {
            let (attr, ty) = (lo(mp1), hi(mp1));
            match attr {
                SMA_SLIDERARMPOSITION => {
                    let i = if ty == SMA_INCREMENTVALUE { mp2 } else { scale(h).index_at(mp2 as i32) };
                    set_pos(h, i, true);
                    1
                }
                SMA_SHAFTDIMENSIONS | SMA_SHAFTPOSITION | SMA_SLIDERARMDIMENSIONS => 1,
                _ => 0,
            }
        }
        SLM_QUERYSLIDERINFO => {
            let (attr, ty) = (lo(mp1), hi(mp1));
            let (start, l, centre) = shaft(h);
            match attr {
                SMA_SLIDERARMPOSITION => {
                    if ty == SMA_INCREMENTVALUE {
                        data(h).map_or(0, |d| d.pos)
                    } else {
                        // SMA_RANGEVALUE: pels from home and the range.
                        mp(arm_at(h) as u32, l as u32)
                    }
                }
                SMA_SHAFTDIMENSIONS => mp(l as u32, SHAFT as u32),
                SMA_SHAFTPOSITION => {
                    let p = if vertical(h) { Point::new(centre - SHAFT / 2, start) } else { Point::new(start, centre - SHAFT / 2) };
                    mp(p.x as u32, p.y as u32)
                }
                SMA_SLIDERARMDIMENSIONS => {
                    if vertical(h) { mp(ARM_ACROSS as u32, ARM_ALONG as u32) } else { mp(ARM_ALONG as u32, ARM_ACROSS as u32) }
                }
                _ => 0,
            }
        }
        SLM_SETTICKSIZE => {
            let (i, size) = (lo(mp1), hi(mp1));
            let Some(d) = data(h) else { return 0 };
            if i == SMA_SETALLTICKS {
                d.ticks.iter_mut().for_each(|t| *t = size as u8);
            } else if let Some(t) = d.ticks.get_mut(i as usize) {
                *t = size as u8;
            } else {
                return 0;
            }
            ctl::invalidate(h);
            1
        }
        SLM_QUERYTICKSIZE => data(h).and_then(|d| d.ticks.get(lo(mp1) as usize).copied()).map_or(0, |t| t as u32),
        SLM_QUERYTICKPOS => {
            let i = lo(mp1);
            if data(h).is_none_or(|d| i >= d.increments) {
                return 0;
            }
            let (_, _, centre) = shaft(h);
            let p = point(h, scale(h).pos(i), centre);
            mp(p.x as u32, p.y as u32)
        }
        SLM_SETSCALETEXT => {
            let t = ctl::cstr(mp2 as *const u8);
            let Some(d) = data(h) else { return 0 };
            match d.texts.get_mut(lo(mp1) as usize) {
                Some(x) => *x = t,
                None => return 0,
            }
            ctl::invalidate(h);
            1
        }
        SLM_QUERYSCALETEXT => {
            let Some(t) = data(h).and_then(|d| d.texts.get(lo(mp1) as usize).cloned()) else { return 0 };
            if mp2 != 0 && hi(mp1) > 0 {
                ctl::copy_out(&t, hi(mp1) as usize, mp2 as *mut u8);
            }
            t.len() as u32
        }
        SLM_ADDDETENT => {
            let Some(d) = data(h) else { return 0 };
            let id = d.next_detent;
            d.next_detent += 1;
            d.detents.push((id, mp1 as i32));
            ctl::invalidate(h);
            id
        }
        SLM_REMOVEDETENT => {
            let Some(d) = data(h) else { return 0 };
            let n = d.detents.len();
            d.detents.retain(|&(id, _)| id != mp1);
            ctl::invalidate(h);
            (d.detents.len() != n) as u32
        }
        SLM_QUERYDETENTPOS => data(h).and_then(|d| d.detents.iter().find(|x| x.0 == mp1).map(|x| x.1 as u32)).unwrap_or(0),
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
