//! Presentation spaces and the GPI entry points (exported at 4000 + the
//! PMGPI ordinal; PMGPI.DLL jumps here).

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::{Mix, Point, Ps, Rect};

use crate::color;
use crate::pointer;
use crate::state::{st, PsEntry, State};

const GPI_ERROR: i32 = 0;
const GPI_OK: i32 = 1;
const LCOLF_RGB: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RectL {
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
    pub top: i32,
}

impl RectL {
    pub fn rect(&self) -> Rect {
        Rect::new(self.left, self.bottom, self.right, self.top)
    }

    pub fn from(r: Rect) -> RectL {
        RectL { left: r.left, bottom: r.bottom, right: r.right, top: r.top }
    }
}

const HPS_BASE: u32 = 0x100;

/// Create a presentation space for a window, clipped to `clip` (device
/// rectangles).
pub fn create(s: &mut State, hwnd: u32, origin: Point, clip: Vec<Rect>) -> u32 {
    let mut ps = Ps::new(origin, clip);
    ps.color = 0x000000;
    ps.back_color = 0xffffff;
    let e = PsEntry { ps, hwnd, color_index: gpi::color::CLR_NEUTRAL, back_index: gpi::color::CLR_BACKGROUND, rgb_mode: false };
    match s.pss.iter().position(|p| p.is_none()) {
        Some(i) => {
            s.pss[i] = Some(e);
            HPS_BASE + i as u32
        }
        None => {
            s.pss.push(Some(e));
            HPS_BASE + s.pss.len() as u32 - 1
        }
    }
}

pub fn destroy(s: &mut State, hps: u32) -> bool {
    match s.pss.get_mut(hps.wrapping_sub(HPS_BASE) as usize) {
        Some(slot @ Some(_)) => {
            *slot = None;
            true
        }
        _ => false,
    }
}

pub fn entry(s: &mut State, hps: u32) -> Option<&mut PsEntry> {
    s.pss.get_mut(hps.wrapping_sub(HPS_BASE) as usize)?.as_mut()
}

fn rgb(e: &PsEntry, c: i32) -> u32 {
    if e.rgb_mode && c >= 0 { c as u32 & 0xff_ffff } else { color::resolve(c) }
}

/// Run a drawing operation on a PS with the pointer hidden.
fn draw<R>(hps: u32, f: impl FnOnce(&mut PsEntry, &mut gpi::Surface) -> R) -> Option<R> {
    let s = st();
    let mut surface = s.surface;
    pointer::hide(s);
    let r = entry(s, hps).map(|e| f(e, &mut surface));
    pointer::show(s);
    r
}

fn read<T: Copy>(p: *const T) -> Option<T> {
    (!p.is_null()).then(|| unsafe { p.read_unaligned() })
}

fn point(p: *const PointL) -> Option<Point> {
    read(p).map(|p| Point::new(p.x, p.y))
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetColor(hps: u32, c: i32) -> u32 {
    let s = st();
    let Some(e) = entry(s, hps) else { return 0 };
    e.color_index = c;
    e.ps.color = rgb(e, c);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryColor(hps: u32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.color_index)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetBackColor(hps: u32, c: i32) -> u32 {
    let s = st();
    let Some(e) = entry(s, hps) else { return 0 };
    e.back_index = c;
    e.ps.back_color = rgb(e, c);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBackColor(hps: u32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.back_index)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetMix(hps: u32, m: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    match Mix::from_fm(m) {
        Some(m) => {
            e.ps.mix = m;
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetBackMix(hps: u32, m: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    match Mix::from_bm(m) {
        Some(m) => {
            e.ps.back_mix = m;
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiMove(hps: u32, p: *const PointL) -> u32 {
    let (Some(e), Some(p)) = (entry(st(), hps), point(p)) else { return 0 };
    e.ps.move_to(p);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCurrentPosition(hps: u32, p: *const PointL) -> u32 {
    GpiMove(hps, p)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCurrentPosition(hps: u32, p: *mut PointL) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if p.is_null() {
        return 0;
    }
    unsafe { p.write_unaligned(PointL { x: e.ps.pos.x, y: e.ps.pos.y }) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiLine(hps: u32, p: *const PointL) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.line_to(s, p)).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiPolyLine(hps: u32, n: i32, pts: *const PointL) -> i32 {
    if pts.is_null() || n < 0 {
        return GPI_ERROR;
    }
    let pts: Vec<Point> = (0..n as usize).filter_map(|i| point(unsafe { pts.add(i) })).collect();
    draw(hps, |e, s| {
        for p in pts {
            e.ps.line_to(s, p);
        }
    })
    .map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiBox(hps, lControl, pptl, lHRound, lVRound): DRO_FILL 1,
/// DRO_OUTLINE 2, DRO_OUTLINEFILL 3. Rounded corners are drawn square.
#[unsafe(no_mangle)]
pub extern "C" fn GpiBox(hps: u32, ctl: i32, p: *const PointL, _h: i32, _v: i32) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.draw_box(s, p, ctl & 2 != 0, ctl & 1 != 0)).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiCharStringAt(hps: u32, p: *const PointL, n: i32, text: *const u8) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    if text.is_null() || n < 0 {
        return GPI_ERROR;
    }
    let t = unsafe { core::slice::from_raw_parts(text, n as usize) };
    draw(hps, |e, s| e.ps.char_string_at(s, p, t)).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiCharString(hps: u32, n: i32, text: *const u8) -> i32 {
    let Some(pos) = entry(st(), hps).map(|e| e.ps.pos) else { return GPI_ERROR };
    let p = PointL { x: pos.x, y: pos.y };
    GpiCharStringAt(hps, &p, n, text)
}

/// Clear the presentation space to the window colour.
#[unsafe(no_mangle)]
pub extern "C" fn GpiErase(hps: u32) -> u32 {
    let s = st();
    let Some(hwnd) = entry(s, hps).map(|e| e.hwnd) else { return 0 };
    let r = s.desk.get(hwnd).map_or(Rect::default(), |w| w.rect());
    draw(hps, |e, surf| e.ps.fill_rect(surf, r, color::resolve(color::SYSCLR_WINDOW))).map_or(0, |_| 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPel(hps: u32, p: *const PointL) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    draw(hps, |e, s| {
        let c = e.ps.color;
        e.ps.fill_rect(s, Rect::new(p.x, p.y, p.x + 1, p.y + 1), c)
    })
    .map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPel(hps: u32, p: *const PointL) -> i32 {
    let s = st();
    let Some(p) = point(p) else { return -1 };
    let surface = s.surface;
    let Some(e) = entry(s, hps) else { return -1 };
    let o = e.ps.origin;
    surface.get(p.x + o.x, p.y + o.y).map_or(-1, |c| c as i32)
}

/// GpiCreateLogColorTable(hps, flOptions, lFormat, lStart, lCount,
/// alTable): only switching to RGB mode (LCOLF_RGB) is supported.
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreateLogColorTable(hps: u32, _opts: u32, format: i32, _start: i32, _count: i32, _table: *const i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    e.rgb_mode = format == LCOLF_RGB;
    1
}

/// GpiCreatePS(hab, hdc, psizlPage, flOptions): a presentation space not
/// tied to a window draws nowhere until associated (memory bitmaps are not
/// supported yet).
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreatePS(_hab: u32, _hdc: u32, _size: *const u32, _opts: u32) -> u32 {
    create(st(), 0, Point::new(0, 0), Vec::new())
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDestroyPS(hps: u32) -> u32 {
    destroy(st(), hps) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiAssociate(hps: u32, _hdc: u32) -> u32 {
    entry(st(), hps).is_some() as u32
}
