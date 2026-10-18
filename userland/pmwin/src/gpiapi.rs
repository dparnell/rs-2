//! Presentation spaces and the GPI entry points for attributes,
//! primitives, areas, paths and transforms (exported at 4000 + the PMGPI
//! ordinal; PMGPI.DLL jumps here). Regions, bitmaps, device contexts and
//! colours are in gpiobj.rs, text and fonts in gpitext.rs.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::{Matrix, Mix, Point, Ps, Rect, Surface};

use crate::color;
use crate::gpiobj;
use crate::pointer;
use crate::state::{st, PsEntry, State};

pub const GPI_ERROR: i32 = 0;
pub const GPI_OK: i32 = 1;

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

/// A logical font or bitmap tagged with a local identifier.
#[derive(Clone)]
pub enum Lcid {
    Font(gpi::LogFont, [u8; 32]),
    Bitmap(u32),
}

/// GPI state of a presentation space beyond the engine's `Ps`.
#[derive(Default)]
pub struct PsExtra {
    /// Cached micro-PS from WinGetPS / WinBeginPaint (destroyed on
    /// release).
    pub cached: bool,
    /// Between WinBeginPaint and WinEndPaint (clip = update region).
    pub painting: bool,
    /// Window association before a paint borrowed this PS.
    pub hwnd_before_paint: u32,
    pub hdc: u32,
    /// Associated with a memory device context.
    pub memory: bool,
    pub bitmap: u32,
    pub clip_hrgn: u32,
    pub palette: u32,
    pub lcids: Vec<(i32, Lcid)>,
    /// GpiCreatePS options.
    pub options: u32,
}

const HPS_BASE: u32 = 0x100;

fn new_ps(origin: Point, clip: Vec<Rect>) -> Ps {
    let mut ps = Ps::new(origin, clip);
    ps.colors.sys = color::sys;
    ps.sync_simple();
    ps
}

fn insert(s: &mut State, e: PsEntry) -> u32 {
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

/// Create a cached presentation space for a window, clipped to `clip`
/// (device rectangles).
pub fn create(s: &mut State, hwnd: u32, origin: Point, clip: Vec<Rect>) -> u32 {
    let e = PsEntry {
        ps: new_ps(origin, clip),
        hwnd,
        color_index: gpi::color::CLR_NEUTRAL,
        back_index: gpi::color::CLR_BACKGROUND,
        rgb_mode: false,
        gpi: PsExtra { cached: true, ..PsExtra::default() },
    };
    insert(s, e)
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

/// WinBeginPaint with an application PS: draw into `hwnd`, clipped to
/// the update region, until WinEndPaint.
pub fn begin_paint(s: &mut State, hps: u32, hwnd: u32, origin: Point, clip: Vec<Rect>) {
    if let Some(e) = entry(s, hps) {
        if !e.gpi.painting {
            e.gpi.hwnd_before_paint = e.hwnd;
        }
        e.gpi.painting = true;
        e.hwnd = hwnd;
        e.ps.set_vis(origin, clip);
    }
}

/// WinEndPaint: release a cached PS, or end painting with an
/// application PS.
pub fn end_paint(s: &mut State, hps: u32) {
    let cached = entry(s, hps).is_none_or(|e| e.gpi.cached);
    if cached {
        destroy(s, hps);
    } else if let Some(e) = entry(s, hps) {
        e.gpi.painting = false;
        e.hwnd = e.gpi.hwnd_before_paint;
        if e.hwnd == 0 {
            e.ps.set_vis(Point::new(0, 0), Vec::new());
        }
    }
}

/// RGB for a colour value in a PS (its colour table and system colours).
pub fn rgb(e: &PsEntry, c: i32) -> u32 {
    e.ps.colors.rgb(c)
}

/// Bring a window PS's visible region up to date (windows move).
pub fn refresh(s: &mut State, hps: u32) {
    let Some(e) = entry(s, hps) else { return };
    let h = e.hwnd;
    if h == 0 || e.gpi.cached || e.gpi.painting || e.gpi.memory {
        return;
    }
    let Some(w) = s.desk.get(h) else {
        // The window is gone: draw nowhere.
        if let Some(e) = entry(s, hps) {
            e.ps.set_vis(Point::new(0, 0), Vec::new());
        }
        return;
    };
    let clip_children = w.style & pmwin::ws::CLIPCHILDREN != 0;
    let vis = s.desk.visible_region(h, clip_children).rects;
    let o = s.desk.origin(h);
    let Some(e) = entry(s, hps) else { return };
    if e.ps.origin != o || e.ps.vis() != vis.as_slice() {
        e.ps.set_vis(o, vis);
    }
}

/// Where a PS draws.
pub enum Target {
    Screen,
    Bitmap(u32),
    Nowhere,
}

pub fn target(e: &PsEntry) -> Target {
    if e.gpi.memory {
        if e.gpi.bitmap != 0 { Target::Bitmap(e.gpi.bitmap) } else { Target::Nowhere }
    } else if e.hwnd != 0 {
        Target::Screen
    } else {
        Target::Nowhere
    }
}

/// A surface that draws nothing.
pub fn null_surface() -> Surface {
    unsafe { Surface::new(core::ptr::null_mut(), 0, 0, 0, false) }
}

/// Run a drawing operation on a PS with its target surface (the pointer
/// hidden when drawing on the screen).
pub fn draw<R>(hps: u32, f: impl FnOnce(&mut PsEntry, &mut Surface) -> R) -> Option<R> {
    let s = st();
    refresh(s, hps);
    let t = target(entry(s, hps)?);
    match t {
        Target::Screen => {
            let mut surface = s.surface;
            pointer::hide(s);
            let r = entry(s, hps).map(|e| f(e, &mut surface));
            pointer::show(s);
            r
        }
        Target::Bitmap(h) => {
            let mut surface = gpiobj::bitmap_surface(h).unwrap_or_else(null_surface);
            entry(s, hps).map(|e| f(e, &mut surface))
        }
        Target::Nowhere => {
            let mut surface = null_surface();
            entry(s, hps).map(|e| f(e, &mut surface))
        }
    }
}

/// Change a PS's attributes (saving them first in AM_PRESERVE mode).
fn attrs<R>(hps: u32, f: impl FnOnce(&mut PsEntry) -> R) -> Option<R> {
    let e = entry(st(), hps)?;
    e.ps.will_change_attrs();
    let r = f(e);
    e.ps.sync_simple();
    Some(r)
}

pub fn read<T: Copy>(p: *const T) -> Option<T> {
    (!p.is_null()).then(|| unsafe { p.read_unaligned() })
}

pub fn write<T>(p: *mut T, v: T) -> bool {
    if p.is_null() {
        return false;
    }
    unsafe { p.write_unaligned(v) };
    true
}

pub fn point(p: *const PointL) -> Option<Point> {
    read(p).map(|p| Point::new(p.x, p.y))
}

pub fn points(p: *const PointL, n: i32) -> Option<Vec<Point>> {
    if p.is_null() || n < 0 {
        return None;
    }
    Some((0..n as usize).filter_map(|i| point(unsafe { p.add(i) })).collect())
}

fn ok(b: bool) -> i32 {
    if b { GPI_OK } else { GPI_ERROR }
}

fn query<T>(hps: u32, f: impl FnOnce(&PsEntry) -> T, err: T) -> T {
    entry(st(), hps).map_or(err, |e| f(e))
}

// ---- Presentation spaces ----

const GPIA_ASSOC: u32 = 0x4000;

/// GpiCreatePS(hab, hdc, psizlPage, flOptions)
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreatePS(_hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32 {
    let s = st();
    let mut ps = new_ps(Point::new(0, 0), Vec::new());
    let units = match opts & 0x1c {
        0 => gpi::xform::PU_PELS,
        u => u,
    };
    let sz = if size.is_null() { (0, 0) } else { unsafe { (size.read_unaligned(), size.add(1).read_unaligned()) } };
    ps.set_page(units, sz);
    let e = PsEntry {
        ps,
        hwnd: 0,
        color_index: gpi::color::CLR_NEUTRAL,
        back_index: gpi::color::CLR_BACKGROUND,
        rgb_mode: false,
        gpi: PsExtra { options: opts, ..PsExtra::default() },
    };
    let hps = insert(s, e);
    if opts & GPIA_ASSOC != 0 && hdc != 0 && !gpiobj::associate(hps, hdc) {
        destroy(st(), hps);
        return 0;
    }
    hps
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDestroyPS(hps: u32) -> u32 {
    let s = st();
    if let Some(e) = entry(s, hps) {
        let bm = e.gpi.bitmap;
        if bm != 0 {
            gpiobj::bitmap_selected(bm, false);
        }
    }
    destroy(s, hps) as u32
}

/// GpiAssociate(hps, hdc): 0 disassociates.
#[unsafe(no_mangle)]
pub extern "C" fn GpiAssociate(hps: u32, hdc: u32) -> u32 {
    if entry(st(), hps).is_none() {
        return 0;
    }
    if hdc == 0 {
        let e = entry(st(), hps).unwrap();
        if e.gpi.bitmap != 0 {
            gpiobj::bitmap_selected(e.gpi.bitmap, false);
        }
        e.gpi.hdc = 0;
        e.gpi.memory = false;
        e.gpi.bitmap = 0;
        e.hwnd = 0;
        e.ps.set_vis(Point::new(0, 0), Vec::new());
        return 1;
    }
    gpiobj::associate(hps, hdc) as u32
}

/// GpiQueryDevice(hps): the associated device context.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDevice(hps: u32) -> u32 {
    query(hps, |e| e.gpi.hdc, 0)
}

/// GpiResetPS(hps, flOptions): GRES_ATTRS 1, GRES_SEGMENTS 2, GRES_ALL 4.
#[unsafe(no_mangle)]
pub extern "C" fn GpiResetPS(hps: u32, opts: u32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    e.ps.reset(opts & 4 != 0);
    if opts & 4 != 0 {
        e.gpi.clip_hrgn = 0;
        e.gpi.palette = 0;
        e.rgb_mode = false;
    }
    e.color_index = gpi::color::CLR_NEUTRAL;
    e.back_index = gpi::color::CLR_BACKGROUND;
    1
}

/// GpiSetPS(hps, psizlSize, flOptions): new page size and units.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPS(hps: u32, size: *const i32, opts: u32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let sz = if size.is_null() { (0, 0) } else { unsafe { (size.read_unaligned(), size.add(1).read_unaligned()) } };
    let units = if opts & 0x1c == 0 { gpi::xform::PU_PELS } else { opts & 0x1c };
    e.ps.reset(true);
    e.ps.set_page(units, sz);
    1
}

/// GpiQueryPS(hps, psizlSize): page size; returns the options.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPS(hps: u32, size: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if !size.is_null() {
        unsafe {
            size.write_unaligned(e.ps.page_size.0);
            size.add(1).write_unaligned(e.ps.page_size.1);
        }
    }
    e.gpi.options & !0x1c | e.ps.page_units
}

/// GpiSavePS / GpiRestorePS: a single level is enough for most programs;
/// state saving is not supported, so these only succeed.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSavePS(hps: u32) -> i32 {
    if entry(st(), hps).is_some() { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiRestorePS(hps: u32, _id: i32) -> u32 {
    entry(st(), hps).is_some() as u32
}

// ---- Drawing control, segments and metafiles (not supported) ----

/// GpiSetDrawControl(hps, lControl, lValue): DCTL_DISPLAY 2 and
/// DCTL_BOUNDARY 3 are honoured.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetDrawControl(hps: u32, ctl: i32, val: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    match ctl {
        2 => e.ps.display = val != 0,
        3 => e.ps.collect_bounds = val != 0,
        _ => {}
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDrawControl(hps: u32, ctl: i32) -> i32 {
    query(
        hps,
        |e| match ctl {
            1 => 1,
            2 => e.ps.display as i32,
            3 => e.ps.collect_bounds as i32,
            _ => 0,
        },
        -1,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiResetBoundaryData(hps: u32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    e.ps.bounds = None;
    1
}

/// GpiQueryBoundaryData(hps, prclBoundary): in model space (here device
/// coordinates converted back through the transforms).
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBoundaryData(hps: u32, prcl: *mut RectL) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let b = e.ps.bounds.unwrap_or_default();
    let mut pts = [Point::new(b.left, b.bottom), Point::new(b.right - 1, b.top - 1)];
    e.ps.convert(gpi::CVTC_DEVICE, gpi::CVTC_MODEL, &mut pts);
    write(prcl, RectL { left: pts[0].x, bottom: pts[0].y, right: pts[1].x, top: pts[1].y });
    e.ps.bounds.is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetStopDraw(hps: u32, _v: i32) -> u32 {
    entry(st(), hps).is_some() as u32
}

/// SDW_OFF: drawing is never stopped.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryStopDraw(hps: u32) -> i32 {
    query(hps, |_| 0, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetDrawingMode(hps: u32, _mode: i32) -> u32 {
    entry(st(), hps).is_some() as u32
}

/// DM_DRAW: segments are not retained.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDrawingMode(hps: u32) -> i32 {
    query(hps, |_| 1, 0)
}

/// Metafiles are not supported: loading fails.
#[unsafe(no_mangle)]
pub extern "C" fn GpiLoadMetaFile(_hab: u32, _name: *const u8) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDeleteMetaFile(_hmf: u32) -> u32 {
    0
}

/// GpiPlayMetaFile(hps, hmf, lCount1, alOptarray, plSegCount, lCount2,
/// pszDesc)
#[unsafe(no_mangle)]
pub extern "C" fn GpiPlayMetaFile(_hps: u32, _hmf: u32, _n1: i32, _opts: *const i32, _segs: *mut i32, _n2: i32, _desc: *mut u8) -> i32 {
    GPI_ERROR
}

// ---- Attributes ----

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetColor(hps: u32, c: i32) -> u32 {
    attrs(hps, |e| {
        e.color_index = c;
        e.ps.set_all(|b| b.color = c);
    })
    .is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryColor(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line.color, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetBackColor(hps: u32, c: i32) -> u32 {
    attrs(hps, |e| {
        e.back_index = c;
        e.ps.set_all(|b| b.back = c);
    })
    .is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBackColor(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line.back, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetMix(hps: u32, m: i32) -> u32 {
    if Mix::from_fm(m).is_none() {
        return 0;
    }
    attrs(hps, |e| e.ps.set_all(|b| b.mix = m)).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryMix(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line.mix, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetBackMix(hps: u32, m: i32) -> u32 {
    if Mix::from_bm(m).is_none() {
        return 0;
    }
    attrs(hps, |e| e.ps.set_all(|b| b.back_mix = m)).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBackMix(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line.back_mix, -1)
}

/// GpiSetAttrMode(hps, lMode): AM_PRESERVE 0, AM_NOPRESERVE 1.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetAttrMode(hps: u32, mode: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if !(0..=1).contains(&mode) {
        return 0;
    }
    e.ps.attr_mode = mode;
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryAttrMode(hps: u32) -> i32 {
    query(hps, |e| e.ps.attr_mode, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiPop(hps: u32, n: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let r = e.ps.pop_attrs(n);
    e.color_index = e.ps.attrs.line.color;
    e.back_index = e.ps.attrs.line.back;
    r as u32
}

// Bundle masks: color 1, back color 2, mix 4, back mix 8; area set 0x10,
// symbol 0x20, reference point 0x40; line width 0x10, geom width 0x20,
// type 0x40, end 0x80, join 0x100; char set 0x10, mode 0x20, box 0x40,
// angle 0x80, shear 0x100, direction 0x200, align 0x400, extra 0x800,
// break extra 0x1000; marker set 0x10, symbol 0x20, box 0x40.
fn rd16(p: *const u8, o: usize) -> i32 {
    unsafe { (p.add(o) as *const u16).read_unaligned() as i32 }
}

fn rd32(p: *const u8, o: usize) -> i32 {
    unsafe { (p.add(o) as *const i32).read_unaligned() }
}

fn wr16(p: *mut u8, o: usize, v: i32) {
    unsafe { (p.add(o) as *mut u16).write_unaligned(v as u16) }
}

fn wr32(p: *mut u8, o: usize, v: i32) {
    unsafe { (p.add(o) as *mut i32).write_unaligned(v) }
}

/// GpiSetAttrs(hps, lPrimType, flAttrMask, flDefMask, ppbunAttrs)
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetAttrs(hps: u32, prim: i32, mask: u32, defmask: u32, b: *const u8) -> u32 {
    if b.is_null() && mask & !defmask != 0 {
        return 0;
    }
    let ok = attrs(hps, |e| {
        let def = gpi::Attrs::default();
        let a = &mut e.ps.attrs;
        let pick = |bit: u32, cur: &mut i32, v: &dyn Fn() -> i32, d: i32| {
            if defmask & bit != 0 {
                *cur = d;
            } else if mask & bit != 0 {
                *cur = v();
            }
        };
        let dbund = gpi::Bundle::default();
        let bundle = match prim {
            gpi::PRIM_LINE => &mut a.line,
            gpi::PRIM_CHAR => &mut a.chr,
            gpi::PRIM_MARKER => &mut a.marker,
            gpi::PRIM_AREA => &mut a.area,
            gpi::PRIM_IMAGE => &mut a.image,
            _ => return false,
        };
        pick(1, &mut bundle.color, &|| rd32(b, 0), dbund.color);
        pick(2, &mut bundle.back, &|| rd32(b, 4), dbund.back);
        pick(4, &mut bundle.mix, &|| rd16(b, 8), dbund.mix);
        pick(8, &mut bundle.back_mix, &|| rd16(b, 10), dbund.back_mix);
        match prim {
            gpi::PRIM_LINE => {
                pick(0x10, &mut a.line_width, &|| rd32(b, 12), def.line_width);
                pick(0x20, &mut a.line_width_geom, &|| rd32(b, 16), def.line_width_geom);
                pick(0x40, &mut a.line_type, &|| rd16(b, 20), def.line_type);
                pick(0x80, &mut a.line_end, &|| rd16(b, 22), def.line_end);
                pick(0x100, &mut a.line_join, &|| rd16(b, 24), def.line_join);
            }
            gpi::PRIM_CHAR => {
                pick(0x10, &mut a.char_set, &|| rd16(b, 12), def.char_set);
                pick(0x20, &mut a.char_mode, &|| rd16(b, 14), def.char_mode);
                if defmask & 0x40 != 0 {
                    a.char_box = def.char_box;
                } else if mask & 0x40 != 0 {
                    a.char_box = (rd32(b, 16), rd32(b, 20));
                }
                if defmask & 0x80 != 0 {
                    a.char_angle = def.char_angle;
                } else if mask & 0x80 != 0 {
                    a.char_angle = (rd32(b, 24), rd32(b, 28));
                }
                pick(0x200, &mut a.char_direction, &|| rd16(b, 40), def.char_direction);
                pick(0x800, &mut a.char_extra, &|| rd32(b, 44), def.char_extra);
                pick(0x1000, &mut a.char_break_extra, &|| rd32(b, 48), def.char_break_extra);
            }
            gpi::PRIM_MARKER => {
                pick(0x10, &mut a.marker_set, &|| rd16(b, 12), def.marker_set);
                pick(0x20, &mut a.marker_sym, &|| rd16(b, 14), def.marker_sym);
                if defmask & 0x40 != 0 {
                    a.marker_box = def.marker_box;
                } else if mask & 0x40 != 0 {
                    a.marker_box = (rd32(b, 16), rd32(b, 20));
                }
            }
            gpi::PRIM_AREA => {
                pick(0x10, &mut a.pattern_set, &|| rd16(b, 12), def.pattern_set);
                pick(0x20, &mut a.pattern, &|| rd16(b, 14), def.pattern);
                if defmask & 0x40 != 0 {
                    a.pattern_ref = def.pattern_ref;
                } else if mask & 0x40 != 0 {
                    a.pattern_ref = Point::new(rd32(b, 16), rd32(b, 20));
                }
            }
            _ => {}
        }
        true
    });
    ok.unwrap_or(false) as u32
}

/// GpiQueryAttrs(hps, lPrimType, flAttrMask, ppbunAttrs): returns the
/// mask of attributes that are not defaults (here: all requested).
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryAttrs(hps: u32, prim: i32, mask: u32, b: *mut u8) -> i32 {
    let Some(e) = entry(st(), hps) else { return -1 };
    if b.is_null() {
        return -1;
    }
    let a = &e.ps.attrs;
    let bundle = match prim {
        gpi::PRIM_LINE => a.line,
        gpi::PRIM_CHAR => a.chr,
        gpi::PRIM_MARKER => a.marker,
        gpi::PRIM_AREA => a.area,
        gpi::PRIM_IMAGE => a.image,
        _ => return -1,
    };
    let put32 = |bit: u32, o: usize, v: i32| {
        if mask & bit != 0 {
            wr32(b, o, v)
        }
    };
    let put16 = |bit: u32, o: usize, v: i32| {
        if mask & bit != 0 {
            wr16(b, o, v)
        }
    };
    put32(1, 0, bundle.color);
    put32(2, 4, bundle.back);
    put16(4, 8, bundle.mix);
    put16(8, 10, bundle.back_mix);
    match prim {
        gpi::PRIM_LINE => {
            put32(0x10, 12, a.line_width);
            put32(0x20, 16, a.line_width_geom);
            put16(0x40, 20, a.line_type);
            put16(0x80, 22, a.line_end);
            put16(0x100, 24, a.line_join);
        }
        gpi::PRIM_CHAR => {
            put16(0x10, 12, a.char_set);
            put16(0x20, 14, a.char_mode);
            put32(0x40, 16, a.char_box.0);
            put32(0x40, 20, a.char_box.1);
            put32(0x80, 24, a.char_angle.0);
            put32(0x80, 28, a.char_angle.1);
            put32(0x100, 32, a.char_shear.0);
            put32(0x100, 36, a.char_shear.1);
            put16(0x200, 40, a.char_direction);
            put32(0x800, 44, a.char_extra);
            put32(0x1000, 48, a.char_break_extra);
        }
        gpi::PRIM_MARKER => {
            put16(0x10, 12, a.marker_set);
            put16(0x20, 14, a.marker_sym);
            put32(0x40, 16, a.marker_box.0);
            put32(0x40, 20, a.marker_box.1);
        }
        gpi::PRIM_AREA => {
            put16(0x10, 12, a.pattern_set);
            put16(0x20, 14, a.pattern);
            put32(0x40, 16, a.pattern_ref.x);
            put32(0x40, 20, a.pattern_ref.y);
        }
        _ => {}
    }
    mask as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetLineType(hps: u32, t: i32) -> u32 {
    if !(0..=9).contains(&t) {
        return 0;
    }
    attrs(hps, |e| e.ps.attrs.line_type = t).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryLineType(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line_type, -1)
}

/// GpiSetLineWidth(hps, fxLineWidth): cosmetic width (FIXED multiple of
/// the normal width).
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetLineWidth(hps: u32, w: i32) -> u32 {
    if w < 0 {
        return 0;
    }
    attrs(hps, |e| e.ps.attrs.line_width = w).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryLineWidth(hps: u32) -> i32 {
    query(hps, |e| if e.ps.attrs.line_width == 0 { 0x10000 } else { e.ps.attrs.line_width }, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetLineWidthGeom(hps: u32, w: i32) -> u32 {
    if w < 0 {
        return 0;
    }
    attrs(hps, |e| e.ps.attrs.line_width_geom = w).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryLineWidthGeom(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line_width_geom, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetLineEnd(hps: u32, v: i32) -> u32 {
    attrs(hps, |e| e.ps.attrs.line_end = v).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryLineEnd(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line_end, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetLineJoin(hps: u32, v: i32) -> u32 {
    attrs(hps, |e| e.ps.attrs.line_join = v).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryLineJoin(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.line_join, -1)
}

/// GpiSetPattern(hps, lPatternSymbol): PATSYM_*.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPattern(hps: u32, sym: i32) -> u32 {
    if gpi::pattern::standard(sym).is_none() {
        return 0;
    }
    attrs(hps, |e| e.ps.attrs.pattern = sym).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPattern(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.pattern, -1)
}

/// GpiSetPatternSet(hps, lSet): 0 = standard patterns; an lcid tagged
/// with GpiSetBitmapId uses that bitmap as the pattern.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPatternSet(hps: u32, set: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if set == 0 {
        e.ps.will_change_attrs();
        e.ps.attrs.pattern_set = 0;
        e.ps.set_pattern_bitmap(None);
        return 1;
    }
    let bm = e.gpi.lcids.iter().find(|(l, _)| *l == set).and_then(|(_, x)| if let Lcid::Bitmap(h) = x { Some(*h) } else { None });
    let Some(bm) = bm else { return 0 };
    e.ps.will_change_attrs();
    e.ps.attrs.pattern_set = set;
    gpiobj::with_bitmap(bm, |b| e.ps.set_pattern_bitmap(Some(b)));
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPatternSet(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.pattern_set, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPatternRefPoint(hps: u32, p: *const PointL) -> u32 {
    let Some(p) = point(p) else { return 0 };
    attrs(hps, |e| e.ps.attrs.pattern_ref = p).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPatternRefPoint(hps: u32, p: *mut PointL) -> u32 {
    let Some(r) = entry(st(), hps).map(|e| e.ps.attrs.pattern_ref) else { return 0 };
    write(p, PointL { x: r.x, y: r.y }) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetMarker(hps: u32, sym: i32) -> u32 {
    if gpi::pattern::marker(sym).is_none() {
        return 0;
    }
    attrs(hps, |e| e.ps.attrs.marker_sym = sym).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryMarker(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.marker_sym, -1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetMarkerBox(hps: u32, p: *const i32) -> u32 {
    if p.is_null() {
        return 0;
    }
    let v = unsafe { (p.read_unaligned(), p.add(1).read_unaligned()) };
    attrs(hps, |e| e.ps.attrs.marker_box = v).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryMarkerBox(hps: u32, p: *mut i32) -> u32 {
    let Some(v) = entry(st(), hps).map(|e| e.ps.attrs.marker_box) else { return 0 };
    (write(p, v.0) && write(unsafe { p.add(1) }, v.1)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetMarkerSet(hps: u32, set: i32) -> u32 {
    attrs(hps, |e| e.ps.attrs.marker_set = set).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryMarkerSet(hps: u32) -> i32 {
    query(hps, |e| e.ps.attrs.marker_set, -1)
}

/// ARCPARAMS: lP, lQ, lR, lS.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetArcParams(hps: u32, p: *const [i32; 4]) -> u32 {
    let Some(v) = read(p) else { return 0 };
    attrs(hps, |e| e.ps.attrs.arc = v).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryArcParams(hps: u32, p: *mut [i32; 4]) -> u32 {
    let Some(v) = entry(st(), hps).map(|e| e.ps.attrs.arc) else { return 0 };
    write(p, v) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetDefArcParams(hps: u32, p: *const [i32; 4]) -> u32 {
    GpiSetArcParams(hps, p)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDefArcParams(hps: u32, p: *mut [i32; 4]) -> u32 {
    if entry(st(), hps).is_none() {
        return 0;
    }
    write(p, [1, 1, 0, 0]) as u32
}

// ---- Current position and primitives ----

#[unsafe(no_mangle)]
pub extern "C" fn GpiMove(hps: u32, p: *const PointL) -> u32 {
    let (Some(e), Some(p)) = (entry(st(), hps), point(p)) else { return 0 };
    e.ps.gpi_move(p);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCurrentPosition(hps: u32, p: *const PointL) -> u32 {
    GpiMove(hps, p)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCurrentPosition(hps: u32, p: *mut PointL) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    write(p, PointL { x: e.ps.pos.x, y: e.ps.pos.y }) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiLine(hps: u32, p: *const PointL) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_line(s, p)).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiPolyLine(hps: u32, n: i32, pts: *const PointL) -> i32 {
    let Some(pts) = points(pts, n) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_polyline(s, &pts)).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiPolyLineDisjoint(hps: u32, n: i32, pts: *const PointL) -> i32 {
    let Some(pts) = points(pts, n) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_polyline_disjoint(s, &pts)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiBox(hps, lControl, pptl, lHRound, lVRound): DRO_FILL 1,
/// DRO_OUTLINE 2, DRO_OUTLINEFILL 3.
#[unsafe(no_mangle)]
pub extern "C" fn GpiBox(hps: u32, ctl: i32, p: *const PointL, h: i32, v: i32) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_box(s, ctl & 3, p, h, v)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiFullArc(hps, lControl, fxMultiplier)
#[unsafe(no_mangle)]
pub extern "C" fn GpiFullArc(hps: u32, ctl: i32, mult: i32) -> i32 {
    draw(hps, |e, s| e.ps.gpi_full_arc(s, ctl & 3, mult)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiPartialArc(hps, pptlCenter, fxMultiplier, fxStartAngle,
/// fxSweepAngle)
#[unsafe(no_mangle)]
pub extern "C" fn GpiPartialArc(hps: u32, c: *const PointL, mult: i32, start: i32, sweep: i32) -> i32 {
    let Some(c) = point(c) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_partial_arc(s, c, mult, start, sweep)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiPointArc(hps, pptl2): through pptl2[0] to pptl2[1].
#[unsafe(no_mangle)]
pub extern "C" fn GpiPointArc(hps: u32, p: *const PointL) -> i32 {
    let Some(pts) = points(p, 2) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_point_arc(s, pts[0], pts[1])).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiPolySpline(hps: u32, n: i32, pts: *const PointL) -> i32 {
    let Some(pts) = points(pts, n) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_poly_spline(s, &pts)).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiPolyFillet(hps: u32, n: i32, pts: *const PointL) -> i32 {
    let Some(pts) = points(pts, n) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_poly_fillet(s, &pts)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiPolyFilletSharp(hps, lCount, aptlPoints, afxPoints): the sharpness
/// values are ignored (drawn as ordinary fillets).
#[unsafe(no_mangle)]
pub extern "C" fn GpiPolyFilletSharp(hps: u32, n: i32, pts: *const PointL, _sharp: *const i32) -> i32 {
    GpiPolyFillet(hps, n, pts)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiMarker(hps: u32, p: *const PointL) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_markers(s, &[p])).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiPolyMarker(hps: u32, n: i32, pts: *const PointL) -> i32 {
    let Some(pts) = points(pts, n) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_markers(s, &pts)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// Clear the presentation space (window or bitmap) to the reset colour.
#[unsafe(no_mangle)]
pub extern "C" fn GpiErase(hps: u32) -> u32 {
    let s = st();
    refresh(s, hps);
    let Some(e) = entry(s, hps) else { return 0 };
    let r = match target(e) {
        Target::Screen => {
            let h = e.hwnd;
            s.desk.get(h).map_or(Rect::default(), |w| w.rect())
        }
        Target::Bitmap(b) => gpiobj::bitmap_size(b).map_or(Rect::default(), |(w, h)| Rect::new(0, 0, w, h)),
        Target::Nowhere => Rect::default(),
    };
    draw(hps, |e, surf| e.ps.erase(surf, r)).map_or(0, |_| 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPel(hps: u32, p: *const PointL) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.gpi_set_pel(s, p)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiQueryPel(hps, pptl): the colour (index, or RGB in RGB mode) at a
/// world point.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPel(hps: u32, p: *const PointL) -> i32 {
    let Some(p) = point(p) else { return -1 };
    draw(hps, |e, s| {
        let mut d = [p];
        e.ps.convert(gpi::CVTC_WORLD, gpi::CVTC_DEVICE, &mut d);
        let o = e.ps.origin;
        s.get(d[0].x + o.x, d[0].y + o.y).map_or(-1, |c| e.ps.colors.index_of(c))
    })
    .unwrap_or(-1)
}

// ---- Areas and paths ----

/// GpiBeginArea(hps, flOptions): BA_BOUNDARY 1, BA_WINDING 2, BA_EXCL 8.
#[unsafe(no_mangle)]
pub extern "C" fn GpiBeginArea(hps: u32, opts: u32) -> u32 {
    entry(st(), hps).is_some_and(|e| e.ps.begin_area(opts)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiEndArea(hps: u32) -> i32 {
    draw(hps, |e, s| ok(e.ps.end_area(s))).unwrap_or(GPI_ERROR)
}

/// GpiBeginPath(hps, lPath): only path 1 exists.
#[unsafe(no_mangle)]
pub extern "C" fn GpiBeginPath(hps: u32, id: i32) -> u32 {
    if id != 1 {
        return 0;
    }
    entry(st(), hps).is_some_and(|e| e.ps.begin_path()) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiEndPath(hps: u32) -> u32 {
    entry(st(), hps).is_some_and(|e| e.ps.end_path()) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiCloseFigure(hps: u32) -> u32 {
    entry(st(), hps).is_some_and(|e| e.ps.close_figure()) as u32
}

/// GpiAbortPath: discard the path being defined.
#[unsafe(no_mangle)]
pub extern "C" fn GpiAbortPath(hps: u32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    e.ps.end_path();
    e.ps.path_region(false, true);
    1
}

/// GpiFillPath(hps, lPath, lOptions): FPATH_WINDING 2, FPATH_EXCL 8.
#[unsafe(no_mangle)]
pub extern "C" fn GpiFillPath(hps: u32, id: i32, opts: i32) -> i32 {
    if id != 1 {
        return GPI_ERROR;
    }
    draw(hps, |e, s| ok(e.ps.fill_path(s, opts))).unwrap_or(GPI_ERROR)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiStrokePath(hps: u32, id: i32, _opts: u32) -> i32 {
    if id != 1 {
        return GPI_ERROR;
    }
    draw(hps, |e, s| ok(e.ps.stroke_path(s))).unwrap_or(GPI_ERROR)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiOutlinePath(hps: u32, id: i32, _opts: i32) -> i32 {
    if id != 1 {
        return GPI_ERROR;
    }
    draw(hps, |e, s| ok(e.ps.outline_path(s))).unwrap_or(GPI_ERROR)
}

/// GpiModifyPath(hps, lPath, lMode): MPATH_STROKE widens the path to the
/// geometric line width; it is kept as is here.
#[unsafe(no_mangle)]
pub extern "C" fn GpiModifyPath(hps: u32, id: i32, _mode: i32) -> u32 {
    (id == 1 && entry(st(), hps).is_some()) as u32
}

/// GpiSetClipPath(hps, lPath, lOptions): SCP_WINDING 2, SCP_AND 4,
/// SCP_EXCL 8; path 0 resets.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetClipPath(hps: u32, id: i32, opts: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    e.ps.set_clip_path(id, opts) as u32
}

// ---- Transforms ----

fn read_matrix(p: *const i32, n: i32) -> Option<Matrix> {
    if p.is_null() || !(0..=9).contains(&n) {
        return None;
    }
    let v: Vec<i32> = (0..n as usize).map(|i| unsafe { p.add(i).read_unaligned() }).collect();
    Some(Matrix::from_lf(&v))
}

fn write_matrix(p: *mut i32, n: i32, m: &Matrix) -> bool {
    if p.is_null() || !(0..=9).contains(&n) {
        return false;
    }
    let lf = m.to_lf();
    for (i, v) in lf.iter().take(n as usize).enumerate() {
        unsafe { p.add(i).write_unaligned(*v) };
    }
    true
}

/// GpiSetModelTransformMatrix(hps, lCount, pmatlfArray, lOptions)
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetModelTransformMatrix(hps: u32, n: i32, m: *const i32, opt: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let new = if n == 0 { Some(Matrix::IDENTITY) } else { read_matrix(m, n) };
    let Some(new) = new.and_then(|m| Matrix::combine(&e.ps.model, &m, opt)) else { return 0 };
    e.ps.set_model(new);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryModelTransformMatrix(hps: u32, n: i32, m: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    write_matrix(m, n, &e.ps.model) as u32
}

/// GpiSetDefaultViewMatrix(hps, lCount, pmatlfArray, lOptions)
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetDefaultViewMatrix(hps: u32, n: i32, m: *const i32, opt: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let new = if n == 0 { Some(Matrix::IDENTITY) } else { read_matrix(m, n) };
    let Some(new) = new.and_then(|m| Matrix::combine(&e.ps.view, &m, opt)) else { return 0 };
    e.ps.set_view(new);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDefaultViewMatrix(hps: u32, n: i32, m: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    write_matrix(m, n, &e.ps.view) as u32
}

/// The segment and viewing transforms are identity (no retained
/// segments); setting them succeeds without effect.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetSegmentTransformMatrix(hps: u32, _seg: i32, _n: i32, _m: *const i32, _opt: i32) -> u32 {
    entry(st(), hps).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryViewingTransformMatrix(hps: u32, n: i32, m: *mut i32) -> u32 {
    if entry(st(), hps).is_none() {
        return 0;
    }
    write_matrix(m, n, &Matrix::IDENTITY) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPageViewport(hps: u32, r: *const RectL) -> u32 {
    let (Some(e), Some(r)) = (entry(st(), hps), read(r)) else { return 0 };
    e.ps.set_page_viewport(r.rect()) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPageViewport(hps: u32, r: *mut RectL) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    write(r, RectL::from(e.ps.page_viewport())) as u32
}

/// GpiConvert(hps, lSrc, lTarg, lCount, aptlPoints)
#[unsafe(no_mangle)]
pub extern "C" fn GpiConvert(hps: u32, from: i32, to: i32, n: i32, pts: *mut PointL) -> u32 {
    let s = st();
    refresh(s, hps);
    let Some(e) = entry(s, hps) else { return 0 };
    let Some(mut v) = points(pts, n) else { return 0 };
    if !e.ps.convert(from, to, &mut v) {
        return 0;
    }
    for (i, p) in v.iter().enumerate() {
        write(unsafe { pts.add(i) }, PointL { x: p.x, y: p.y });
    }
    1
}

/// GpiConvertWithMatrix(hps, lCountp, aptlPoints, lCount, pmatlfArray)
#[unsafe(no_mangle)]
pub extern "C" fn GpiConvertWithMatrix(hps: u32, np: i32, pts: *mut PointL, n: i32, m: *const i32) -> u32 {
    if entry(st(), hps).is_none() {
        return 0;
    }
    let (Some(v), Some(m)) = (points(pts, np), read_matrix(m, n)) else { return 0 };
    for (i, p) in v.iter().enumerate() {
        let (x, y) = m.apply(p.x as f64, p.y as f64);
        write(unsafe { pts.add(i) }, PointL { x: gpi::math::round(x), y: gpi::math::round(y) });
    }
    1
}

fn modify(m: *mut i32, opt: i32, t: Matrix) -> u32 {
    let Some(cur) = read_matrix(m, 9) else { return 0 };
    let Some(new) = Matrix::combine(&cur, &t, opt) else { return 0 };
    write_matrix(m, 9, &new) as u32
}

/// GpiTranslate(hps, pmatlf, lOptions, pptlTranslation)
#[unsafe(no_mangle)]
pub extern "C" fn GpiTranslate(hps: u32, m: *mut i32, opt: i32, p: *const PointL) -> u32 {
    let (Some(_), Some(p)) = (entry(st(), hps), point(p)) else { return 0 };
    modify(m, opt, Matrix::translate(p.x as f64, p.y as f64))
}

/// GpiScale(hps, pmatlf, lOptions, afxScale[2], pptlCenter)
#[unsafe(no_mangle)]
pub extern "C" fn GpiScale(hps: u32, m: *mut i32, opt: i32, sc: *const i32, c: *const PointL) -> u32 {
    let (Some(_), Some(c)) = (entry(st(), hps), point(c)) else { return 0 };
    let Some(sc) = read(sc as *const [i32; 2]) else { return 0 };
    let (cx, cy) = (c.x as f64, c.y as f64);
    let t = Matrix::translate(-cx, -cy).then(&Matrix::scale(gpi::math::fx(sc[0]), gpi::math::fx(sc[1]))).then(&Matrix::translate(cx, cy));
    modify(m, opt, t)
}

/// GpiRotate(hps, pmatlf, lOptions, fxAngle, pptlCenter)
#[unsafe(no_mangle)]
pub extern "C" fn GpiRotate(hps: u32, m: *mut i32, opt: i32, a: i32, c: *const PointL) -> u32 {
    let (Some(_), Some(c)) = (entry(st(), hps), point(c)) else { return 0 };
    let (cx, cy) = (c.x as f64, c.y as f64);
    let t = Matrix::translate(-cx, -cy).then(&Matrix::rotate(gpi::math::fx(a))).then(&Matrix::translate(cx, cy));
    modify(m, opt, t)
}
