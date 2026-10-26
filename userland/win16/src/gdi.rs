//! GDI: device contexts on PM presentation spaces, objects, text and
//! drawing.
//!
//! An HDC is a handle of this table over the HPS of a window (from
//! BeginPaint or GetDC), in RGB mode, with the window's client height to
//! turn Windows' y-down coordinates into PM's. Colours are COLORREFs
//! (0x00BBGGRR); PM's RGB is 0x00RRGGBB.
//!
//! Fonts are PM logical fonts made when a font is selected (local ID 1 of
//! the PS): the stock fixed fonts are rs2's monospaced face, CreateFont
//! maps Windows' faces to rs2's outline faces (Helvetica, Courier, Times
//! New Roman) at the requested size. The built-in controls draw through
//! the functions here too (`fill`, `text_at`, `draw_text_bytes`, ...).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use thunk16::{flat, Pascal};

use crate::pm::*;
use crate::{far_str, read_rect, write_rect, Global};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DcKind {
    /// From BeginPaint: EndPaint releases it.
    Paint,
    /// From GetDC: ReleaseDC.
    Window,
    /// From CreateCompatibleDC (or CreateDC/CreateIC): a PM memory PS on
    /// a memory DC (bitmap.rs); DeleteDC.
    Memory,
}

/// A font: the face (0 system, 1 monospaced, 2 sans, 3 serif, 4 the
/// monospaced outline face), the cell height in pels (0: the face's own),
/// weight, italic, underline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Font {
    pub face: u8,
    pub height: i32,
    pub width: i32,
    pub weight: u16,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone)]
pub enum Obj {
    Brush { color: u32, null: bool },
    Pen { style: u16, width: u16, color: u32 },
    Font(Font),
    /// A PM bitmap: width, height, bits per pixel (1: monochrome).
    Bitmap { hbm: u32, w: i32, h: i32, bpp: u16 },
    /// The COLORREFs of a logical palette.
    Palette(Vec<u32>),
    /// A region: non-overlapping rectangles (region.rs).
    Region(Vec<[i32; 4]>),
}

#[derive(Clone, Copy)]
struct State {
    text: u32,
    bk: u32,
    bk_mode: u16,
    brush: u16,
    pen: u16,
    font: u16,
    pos: (i32, i32),
    rop2: u16,
    align: u16,
    palette: u16,
    stretch: u16,
    poly_fill: u16,
    /// The mapping mode, window and viewport origins and extents.
    map: u16,
    worg: (i32, i32),
    wext: (i32, i32),
    vorg: (i32, i32),
    vext: (i32, i32),
}

pub struct Dc {
    pub hdc: u16,
    pub hps: u32,
    pub height: i32,
    pub kind: DcKind,
    s: State,
    /// The PM window (0 for a memory DC).
    pub pmwnd: u32,
    /// A memory DC's PM device context, its selected bitmap (0: the
    /// default 1x1 monochrome one) and that default bitmap.
    pub mem: u32,
    pub bitmap: u16,
    pub def_bm: u32,
    /// The font made current in the PS (0: none yet).
    applied: u16,
    saved: Vec<(State, bool)>,
    pub clipped: bool,
}

struct Gdi {
    dcs: Vec<Dc>,
    objs: Vec<Option<Obj>>,
    next_dc: u16,
    /// Character widths of each font (by handle).
    widths: BTreeMap<u16, [u8; 256]>,
    /// Brushes of the system colours (GetSysColor index -> HBRUSH).
    sys_brushes: [u16; 21],
}

static GDI: Global<Gdi> = Global::new(Gdi { dcs: Vec::new(), objs: Vec::new(), next_dc: 0x0402, widths: BTreeMap::new(), sys_brushes: [0; 21] });

/// Object handles: 0x0100 + 2 * index; the stock objects come first
/// (GetStockObject(i) is index i).
const OBJ_BASE: u16 = 0x0100;
pub const OPAQUE: u16 = 2;
pub const TRANSPARENT: u16 = 1;
pub const SYSTEM_FONT: u16 = OBJ_BASE + 2 * 13;
pub const ANSI_FIXED_FONT: u16 = OBJ_BASE + 2 * 11;
const STOCK: usize = 17;

fn sys_font() -> Font {
    Font { face: 0, height: 0, width: 0, weight: 700, italic: false, underline: false }
}

fn g() -> &'static mut Gdi {
    let g = GDI.get();
    if g.objs.is_empty() {
        let b = |c, null| Some(Obj::Brush { color: c, null });
        let p = |c, style| Some(Obj::Pen { style, width: 1, color: c });
        let mono = Some(Obj::Font(Font { face: 1, ..sys_font() }));
        g.objs = alloc::vec![
            b(0xffffff, false), // WHITE_BRUSH
            b(0xc0c0c0, false), // LTGRAY_BRUSH
            b(0x808080, false), // GRAY_BRUSH
            b(0x404040, false), // DKGRAY_BRUSH
            b(0x000000, false), // BLACK_BRUSH
            b(0, true),         // NULL_BRUSH
            p(0xffffff, 0),     // WHITE_PEN
            p(0x000000, 0),     // BLACK_PEN
            p(0, 5),            // NULL_PEN
            None,
            mono.clone(),                      // OEM_FIXED_FONT
            mono.clone(),                      // ANSI_FIXED_FONT
            Some(Obj::Font(Font { weight: 400, ..sys_font() })), // ANSI_VAR_FONT
            Some(Obj::Font(sys_font())),       // SYSTEM_FONT
            Some(Obj::Font(sys_font())),       // DEVICE_DEFAULT_FONT
            Some(Obj::Palette(DEFAULT_PALETTE.to_vec())), // DEFAULT_PALETTE
            mono,                              // SYSTEM_FIXED_FONT
        ];
    }
    g
}

/// DeleteObject: not the stock objects, nor a bitmap a DC has selected.
pub fn delete_object(o: u16) -> bool {
    let Some(i) = o.checked_sub(OBJ_BASE).map(|i| i as usize / 2) else { return false };
    if i < STOCK || i >= g().objs.len() || g().sys_brushes.contains(&o) {
        return i < STOCK || g().sys_brushes.contains(&o);
    }
    if let Some(Obj::Bitmap { hbm, .. }) = obj(o) {
        // Not while a DC has it selected.
        if g().dcs.iter().any(|d| d.bitmap == o) {
            return false;
        }
        unsafe { GpiDeleteBitmap(hbm) };
    }
    g().objs[i] = None;
    g().widths.remove(&o);
    true
}

/// Windows' 20 static colours (the default palette).
pub const DEFAULT_PALETTE: [u32; 20] = [
    0x000000, 0x000080, 0x008000, 0x008080, 0x800000, 0x800080, 0x808000, 0xc0c0c0, 0xc0dcc0, 0xf0caa6,
    0xf0fbff, 0xa4a0a0, 0x808080, 0x0000ff, 0x00ff00, 0x00ffff, 0xff0000, 0xff00ff, 0xffff00, 0xffffff,
];

pub fn obj(h: u16) -> Option<Obj> {
    let i = h.checked_sub(OBJ_BASE)? as usize / 2;
    g().objs.get(i).cloned().flatten()
}

/// The object itself, to change it.
pub fn obj_mut(h: u16) -> Option<&'static mut Obj> {
    let i = h.checked_sub(OBJ_BASE)? as usize / 2;
    g().objs.get_mut(i).and_then(|o| o.as_mut())
}

pub fn new_obj(o: Obj) -> u16 {
    let g = g();
    let i = match g.objs.iter().skip(STOCK).position(|o| o.is_none()) {
        Some(i) => {
            g.objs[STOCK + i] = Some(o);
            STOCK + i
        }
        None => {
            g.objs.push(Some(o));
            g.objs.len() - 1
        }
    };
    let h = OBJ_BASE + 2 * i as u16;
    g.widths.remove(&h);
    h
}

pub fn dc(h: u16) -> Option<&'static mut Dc> {
    g().dcs.iter_mut().find(|d| d.hdc == h)
}

/// The brush of a system colour (cached).
pub fn sys_brush(i: u16) -> u16 {
    let i = (i as usize).min(20);
    let b = g().sys_brushes[i];
    if b != 0 {
        return b;
    }
    let b = new_obj(Obj::Brush { color: sys_color(i as u16), null: false });
    g().sys_brushes[i] = b;
    b
}

pub fn is_font(h: u16) -> bool {
    matches!(obj(h), Some(Obj::Font(_)))
}

/// PM's RGB for a COLORREF; PALETTEINDEX(i) (high byte 1) is entry i of
/// the palette last selected.
pub fn rgb(c: u32) -> i32 {
    let c = if c >> 24 == 1 {
        let i = (c & 0xffff) as usize;
        match obj(*CUR_PALETTE.get()) {
            Some(Obj::Palette(p)) => p.get(i).copied().unwrap_or(0),
            _ => DEFAULT_PALETTE.get(i).copied().unwrap_or(0),
        }
    } else {
        c
    };
    ((c & 0xff) << 16 | (c & 0xff00) | (c >> 16) & 0xff) as i32
}

/// The palette last selected into a DC (for PALETTEINDEX colours).
pub static CUR_PALETTE: Global<u16> = Global::new(OBJ_BASE + 2 * 15);

pub fn new_dc(hps: u32, pm: u32, height: i32, kind: DcKind) -> u16 {
    if hps == 0 {
        return 0;
    }
    unsafe { GpiCreateLogColorTable(hps, 0, LCOLF_RGB, 0, 0, core::ptr::null()) };
    let g = g();
    let hdc = g.next_dc;
    g.next_dc = g.next_dc.wrapping_add(2).max(0x0402);
    g.dcs.push(Dc {
        hdc,
        hps,
        height,
        kind,
        s: State {
            text: 0,
            bk: 0xffffff,
            bk_mode: OPAQUE,
            brush: OBJ_BASE,
            pen: OBJ_BASE + 2 * 7,
            font: SYSTEM_FONT,
            pos: (0, 0),
            rop2: 13,
            align: 0,
            palette: OBJ_BASE + 2 * 15,
            stretch: 1,
            poly_fill: 1,
            map: MM_TEXT,
            worg: (0, 0),
            wext: (1, 1),
            vorg: (0, 0),
            vext: (1, 1),
        },
        pmwnd: pm,
        mem: 0,
        bitmap: 0,
        def_bm: 0,
        applied: 0,
        saved: Vec::new(),
        clipped: false,
    });
    hdc
}

/// SetPolyFillMode: ALTERNATE 1, WINDING 2.
pub fn set_poly_fill_mode(hdc: u16, m: u16) -> u16 {
    if m != 1 && m != 2 {
        return 0;
    }
    dc(hdc).map_or(0, |d| core::mem::replace(&mut d.s.poly_fill, m))
}

pub fn poly_fill_mode(hdc: u16) -> u16 {
    dc(hdc).map_or(0, |d| d.s.poly_fill)
}

/// The DC's size in pels: its window's client area, or its bitmap.
pub fn dc_size(hdc: u16) -> (i32, i32) {
    let Some(d) = dc(hdc) else { return (0, 0) };
    if d.kind == DcKind::Memory {
        return match obj(d.bitmap) {
            Some(Obj::Bitmap { w, h, .. }) => (w, h),
            _ => (1, 1),
        };
    }
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(d.pmwnd, &mut r) };
    (r.r - r.l, d.height)
}

/// Fill a device rectangle with a brush.
pub fn fill_rect_dev(hdc: u16, r: [i32; 4], br: u16) {
    let Some(c) = brush_color(br) else { return };
    let Some(d) = dc(hdc) else { return };
    if r[2] <= r[0] || r[3] <= r[1] {
        return;
    }
    let rc = pm_rect_dev(d, r);
    unsafe { WinFillRect(d.hps, &rc, rgb(c)) };
}

/// Polygons (logical points): the brush inside (the DC's fill mode),
/// the pen along each (closed).
pub fn poly_poly(hdc: u16, polys: &[Vec<(i32, i32)>]) -> bool {
    let Some(d) = dc(hdc) else { return false };
    let pm: Vec<Vec<PointL>> = polys.iter().filter(|p| p.len() >= 2).map(|p| p.iter().map(|&(x, y)| pm_pt(d, x, y)).collect()).collect();
    unsafe {
        if let Some(c) = brush_color(d.s.brush) {
            GpiSetColor(d.hps, rgb(c));
            // BA_BOUNDARY | BA_ALTERNATE or BA_WINDING
            GpiBeginArea(d.hps, if d.s.poly_fill == 2 { 2 } else { 0 });
            for p in &pm {
                GpiMove(d.hps, &p[0]);
                GpiPolyLine(d.hps, p.len() as i32 - 1, p[1..].as_ptr());
            }
            GpiEndArea(d.hps);
        }
        if let Some((c, w)) = pen_color(d) {
            GpiSetColor(d.hps, rgb(c));
            GpiSetLineWidth(d.hps, w << 16);
            for p in &pm {
                GpiMove(d.hps, &p[0]);
                GpiPolyLine(d.hps, p.len() as i32 - 1, p[1..].as_ptr());
                GpiLine(d.hps, &p[0]);
            }
        }
    }
    true
}

/// A polyline (the pen) or polygon (`fill`: the brush inside, the pen
/// around; `close` joins the last point to the first).
pub fn poly(hdc: u16, pts: &[(i32, i32)], fill: bool, close: bool) -> bool {
    if fill {
        return poly_poly(hdc, &[pts.to_vec()]);
    }
    let Some(d) = dc(hdc) else { return false };
    if pts.len() < 2 {
        return false;
    }
    let pm: Vec<PointL> = pts.iter().map(|&(x, y)| pm_pt(d, x, y)).collect();
    if let Some((c, w)) = pen_color(d) {
        unsafe {
            GpiSetColor(d.hps, rgb(c));
            GpiSetLineWidth(d.hps, w << 16);
            GpiMove(d.hps, &pm[0]);
            GpiPolyLine(d.hps, pm.len() as i32 - 1, pm[1..].as_ptr());
            if close {
                GpiLine(d.hps, &pm[0]);
            }
        }
    }
    true
}

/// The PS of the memory DC that has the bitmap `h` selected.
pub fn dc_with_bitmap(h: u16) -> Option<u32> {
    g().dcs.iter().find(|d| d.bitmap == h).map(|d| d.hps)
}

/// The DC's brush.
pub fn brush_of(hdc: u16) -> u16 {
    dc(hdc).map_or(0, |d| d.s.brush)
}

pub fn set_stretch_mode(hdc: u16, m: u16) -> u16 {
    dc(hdc).map_or(0, |d| core::mem::replace(&mut d.s.stretch, m))
}

pub fn stretch_mode(hdc: u16) -> u16 {
    dc(hdc).map_or(0, |d| d.s.stretch)
}

/// SelectPalette: the previous palette.
pub fn select_palette(hdc: u16, p: u16) -> u16 {
    if !matches!(obj(p), Some(Obj::Palette(_))) {
        return 0;
    }
    *CUR_PALETTE.get() = p;
    dc(hdc).map_or(0, |d| core::mem::replace(&mut d.s.palette, p))
}

/// The colours of the DC's palette.
pub fn palette_colors(hdc: u16) -> Vec<u32> {
    match dc(hdc).and_then(|d| obj(d.s.palette)) {
        Some(Obj::Palette(p)) => p,
        _ => DEFAULT_PALETTE.to_vec(),
    }
}

pub fn release_dc(hdc: u16) -> bool {
    let g = g();
    let Some(i) = g.dcs.iter().position(|d| d.hdc == hdc) else { return false };
    let d = g.dcs.remove(i);
    unsafe {
        if d.clipped {
            let mut old = 0;
            GpiSetClipRegion(d.hps, 0, &mut old);
        }
        if d.applied != 0 && d.applied != SYSTEM_FONT {
            GpiSetCharSet(d.hps, 0);
            GpiDeleteSetId(d.hps, 1);
        }
        match d.kind {
            DcKind::Paint => WinEndPaint(d.hps),
            DcKind::Window => WinReleasePS(d.hps),
            DcKind::Memory => {
                crate::bitmap::delete_memory(&d);
                1
            }
        }
    };
    true
}

pub const MM_TEXT: u16 = 1;
const MM_ISOTROPIC: u16 = 7;
const MM_ANISOTROPIC: u16 = 8;

/// A logical point in device units (Windows' y down): the window origin
/// and extent map to the viewport's.
pub fn to_dev(d: &Dc, x: i32, y: i32) -> (i32, i32) {
    let s = &d.s;
    if s.map == MM_TEXT {
        return (x - s.worg.0 + s.vorg.0, y - s.worg.1 + s.vorg.1);
    }
    let we = |e: i32| if e == 0 { 1 } else { e };
    (
        ((x - s.worg.0) as i64 * s.vext.0 as i64 / we(s.wext.0) as i64) as i32 + s.vorg.0,
        ((y - s.worg.1) as i64 * s.vext.1 as i64 / we(s.wext.1) as i64) as i32 + s.vorg.1,
    )
}

/// A device point in logical units.
pub fn to_log(d: &Dc, x: i32, y: i32) -> (i32, i32) {
    let s = &d.s;
    if s.map == MM_TEXT {
        return (x - s.vorg.0 + s.worg.0, y - s.vorg.1 + s.worg.1);
    }
    let ve = |e: i32| if e == 0 { 1 } else { e };
    (
        ((x - s.vorg.0) as i64 * s.wext.0 as i64 / ve(s.vext.0) as i64) as i32 + s.worg.0,
        ((y - s.vorg.1) as i64 * s.wext.1 as i64 / ve(s.vext.1) as i64) as i32 + s.worg.1,
    )
}

/// A logical rectangle in device units (normalised).
pub fn dev_rect(d: &Dc, r: [i32; 4]) -> [i32; 4] {
    let (a, b) = (to_dev(d, r[0], r[1]), to_dev(d, r[2], r[3]));
    [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)]
}

/// A logical point as a PM device point.
pub fn pm_pt(d: &Dc, x: i32, y: i32) -> PointL {
    let (x, y) = to_dev(d, x, y);
    PointL { x, y: d.height - 1 - y }
}

/// A Windows rectangle (logical, y down) as a PM one.
pub fn pm_rect(d: &Dc, r: [i32; 4]) -> RectL {
    pm_rect_dev(d, dev_rect(d, r))
}

/// A Windows device rectangle as a PM one.
pub fn pm_rect_dev(d: &Dc, r: [i32; 4]) -> RectL {
    RectL { l: r[0], b: d.height - r[3], r: r[2], t: d.height - r[1] }
}

/// The extents of the fixed mapping modes (window, viewport) at 96 pels
/// per inch: y goes up.
fn fixed_extents(map: u16) -> Option<((i32, i32), (i32, i32))> {
    let per_inch = match map {
        2 => 254,  // MM_LOMETRIC
        3 => 2540, // MM_HIMETRIC
        4 => 100,  // MM_LOENGLISH
        5 => 1000, // MM_HIENGLISH
        6 => 1440, // MM_TWIPS
        _ => return None,
    };
    Some(((per_inch, per_inch), (96, -96)))
}

/// The mapping calls: SetMapMode, the origins and extents (their old
/// values packed x | y << 16, or written to a POINT/SIZE for the Ex
/// forms), DPtoLP and LPtoDP.
fn mapping(ord: u32, a: &mut Pascal) -> Option<u32> {
    let pack = |v: (i32, i32)| (v.1 as u32 & 0xffff) << 16 | v.0 as u32 & 0xffff;
    let put = |p: *mut u8, v: (i32, i32)| {
        if !p.is_null() {
            unsafe {
                (p as *mut i16).write_unaligned(v.0 as i16);
                (p.add(2) as *mut i16).write_unaligned(v.1 as i16);
            }
        }
    };
    Some(match ord {
        // SetMapMode(hdc, mode)
        3 => {
            let (h, m) = (a.w(), a.w());
            let Some(d) = dc(h) else { return Some(0) };
            if !(1..=8).contains(&m) {
                return Some(0);
            }
            let old = core::mem::replace(&mut d.s.map, m);
            if let Some((w, v)) = fixed_extents(m) {
                d.s.wext = w;
                d.s.vext = v;
            } else if m == MM_TEXT || (old != MM_ISOTROPIC && old != MM_ANISOTROPIC) {
                d.s.wext = (1, 1);
                d.s.vext = (1, 1);
            }
            old as u32
        }
        // GetMapMode(hdc)
        81 => dc(a.w()).map_or(0, |d| d.s.map as u32),
        // SetWindowOrg, SetWindowExt, SetViewportOrg, SetViewportExt(hdc,
        // x, y) and their Ex forms (+ the old value's address).
        11..=14 | 479..=482 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            let p = if ord > 400 { a.p() } else { core::ptr::null_mut() };
            let Some(d) = dc(h) else { return Some(0) };
            let which = match ord {
                11 | 482 => 0,
                12 | 481 => 1,
                13 | 480 => 2,
                _ => 3,
            };
            let fixed_ext = which % 2 == 1 && d.s.map != MM_ISOTROPIC && d.s.map != MM_ANISOTROPIC;
            let slot = match which {
                0 => &mut d.s.worg,
                1 => &mut d.s.wext,
                2 => &mut d.s.vorg,
                _ => &mut d.s.vext,
            };
            let old = *slot;
            if !fixed_ext && !(which % 2 == 1 && (x == 0 || y == 0)) {
                *slot = (x, y);
            }
            if d.s.map == MM_ISOTROPIC && which % 2 == 1 {
                isotropic(d);
            }
            if ord > 400 {
                put(p, old);
                1
            } else {
                pack(old)
            }
        }
        // OffsetWindowOrg, OffsetViewportOrg(hdc, dx, dy) (+ Ex)
        15 | 17 | 476 | 477 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            let p = if ord > 400 { a.p() } else { core::ptr::null_mut() };
            let Some(d) = dc(h) else { return Some(0) };
            let slot = if ord == 15 || ord == 477 { &mut d.s.worg } else { &mut d.s.vorg };
            let old = *slot;
            *slot = (old.0 + x, old.1 + y);
            if ord > 400 {
                put(p, old);
                1
            } else {
                pack(old)
            }
        }
        // ScaleWindowExt, ScaleViewportExt(hdc, xnum, xdenom, ynum,
        // ydenom) (+ Ex)
        16 | 18 | 484 | 485 => {
            let (h, xn, xd, yn, yd) = (a.w(), a.i(), a.i(), a.i(), a.i());
            let p = if ord > 400 { a.p() } else { core::ptr::null_mut() };
            let Some(d) = dc(h) else { return Some(0) };
            let aniso = d.s.map == MM_ISOTROPIC || d.s.map == MM_ANISOTROPIC;
            let slot = if ord == 16 || ord == 485 { &mut d.s.wext } else { &mut d.s.vext };
            let old = *slot;
            if aniso && xd != 0 && yd != 0 {
                *slot = (old.0 * xn / xd, old.1 * yn / yd);
                if d.s.map == MM_ISOTROPIC {
                    isotropic(d);
                }
            }
            if ord > 400 {
                put(p, old);
                1
            } else {
                pack(old)
            }
        }
        // GetViewportExt, GetViewportOrg, GetWindowExt, GetWindowOrg(hdc)
        94..=97 => {
            let h = a.w();
            let Some(d) = dc(h) else { return Some(0) };
            pack(match ord {
                94 => d.s.vext,
                95 => d.s.vorg,
                96 => d.s.wext,
                _ => d.s.worg,
            })
        }
        // GetViewportExtEx, GetViewportOrgEx, GetWindowExtEx,
        // GetWindowOrgEx(hdc, lpPoint)
        472..=475 => {
            let (h, p) = (a.w(), a.p());
            let Some(d) = dc(h) else { return Some(0) };
            put(p, match ord {
                472 => d.s.vext,
                473 => d.s.vorg,
                474 => d.s.wext,
                _ => d.s.worg,
            });
            1
        }
        // DPtoLP(hdc, points, n), LPtoDP(hdc, points, n)
        67 | 99 => {
            let (h, p, n) = (a.w(), a.p(), a.i());
            let Some(d) = dc(h) else { return Some(0) };
            for i in 0..n.max(0) as usize {
                let q = unsafe { p.add(4 * i) };
                let (x, y) = unsafe { ((q as *const i16).read_unaligned() as i32, (q.add(2) as *const i16).read_unaligned() as i32) };
                put(q, if ord == 67 { to_log(d, x, y) } else { to_dev(d, x, y) });
            }
            1
        }
        // MoveToEx(hdc, x, y, lpOld), GetCurrentPositionEx(hdc, lpPoint)
        483 => {
            let (h, x, y, p) = (a.w(), a.i(), a.i(), a.p());
            let Some(d) = dc(h) else { return Some(0) };
            let old = core::mem::replace(&mut d.s.pos, (x, y));
            put(p, old);
            1
        }
        470 => {
            let (h, p) = (a.w(), a.p());
            let Some(d) = dc(h) else { return Some(0) };
            put(p, d.s.pos);
            1
        }
        // GetTextExtentPoint(hdc, text, count, lpSize)
        471 => {
            let (h, s, n, p) = (a.w(), a.l(), a.i(), a.p());
            if dc(h).is_none() {
                return Some(0);
            }
            let text = if n > 0 { unsafe { core::slice::from_raw_parts(flat(s), n as usize) } } else { &[] };
            put(p, (text_width(h, text), line_height(h)));
            1
        }
        // GetDCOrg(hdc): the DC's origin on the screen.
        79 => {
            let h = a.w();
            dc_origin(h).map_or(0, pack)
        }
        // GetBrushOrgEx(hdc, lpPoint), SetBrushOrg(hdc, x, y)
        469 => {
            let (_h, p) = (a.w(), a.p());
            put(p, (0, 0));
            1
        }
        148 => {
            let (_h, _x, _y) = (a.w(), a.i(), a.i());
            0
        }
        // GetAspectRatioFilterEx(hdc, lpSize)
        486 => {
            let (_h, p) = (a.w(), a.p());
            put(p, (0, 0));
            1
        }
        _ => return None,
    })
}

/// MM_ISOTROPIC: the viewport extent shrinks so that a logical unit is
/// as wide as it is high.
fn isotropic(d: &mut Dc) {
    let (wx, wy) = (d.s.wext.0.max(1) as i64, d.s.wext.1.abs().max(1) as i64);
    let (vx, vy) = (d.s.vext.0 as i64, d.s.vext.1 as i64);
    let (sx, sy) = (vx.abs() * 1000 / wx, vy.abs() * 1000 / wy);
    if sx > sy {
        d.s.vext.0 = (vx.signum() * wx * sy / 1000) as i32;
    } else {
        d.s.vext.1 = (vy.signum() * wy * sx / 1000) as i32;
    }
}

/// The screen position of a DC's top-left corner (window DCs).
fn dc_origin(h: u16) -> Option<(i32, i32)> {
    let d = dc(h)?;
    let hwnd = d.pmwnd;
    if hwnd == 0 {
        return Some((0, 0));
    }
    let mut p = PointL { x: 0, y: d.height };
    unsafe { WinMapWindowPoints(hwnd, HWND_DESKTOP, &mut p, 1) };
    let sh = unsafe { WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN) };
    Some((p.x, sh - p.y))
}

/// The brush's colour: an HBRUSH, or a system colour index + 1.
pub fn brush_color(br: u16) -> Option<u32> {
    if (1..=21).contains(&br) {
        return Some(sys_color(br - 1));
    }
    match obj(br) {
        Some(Obj::Brush { null: true, .. }) => None,
        Some(Obj::Brush { color, .. }) => Some(color),
        _ => None,
    }
}

pub fn fill_rect(hdc: u16, r: [i32; 4], br: u16) {
    let Some(c) = brush_color(br) else { return };
    fill(hdc, r, c);
}

/// Fill a rectangle with a colour.
pub fn fill(hdc: u16, r: [i32; 4], c: u32) {
    let Some(d) = dc(hdc) else { return };
    if r[2] <= r[0] || r[3] <= r[1] {
        return;
    }
    let rc = pm_rect(d, r);
    unsafe { WinFillRect(d.hps, &rc, rgb(c)) };
}

/// A one-pel outline.
pub fn frame(hdc: u16, r: [i32; 4], c: u32) {
    let [l, t, rr, b] = r;
    fill(hdc, [l, t, rr, t + 1], c);
    fill(hdc, [l, b - 1, rr, b], c);
    fill(hdc, [l, t, l + 1, b], c);
    fill(hdc, [rr - 1, t, rr, b], c);
}

/// Invert a rectangle.
pub fn invert(hdc: u16, r: [i32; 4]) {
    let Some(d) = dc(hdc) else { return };
    if r[2] <= r[0] || r[3] <= r[1] {
        return;
    }
    let rc = pm_rect(d, r);
    unsafe { WinInvertRect(d.hps, &rc) };
}

/// DrawFocusRect: a dotted outline, inverted (drawing it twice removes
/// it).
pub fn focus_rect(hdc: u16, r: [i32; 4]) {
    let [l, t, rr, b] = r;
    if rr <= l || b <= t {
        return;
    }
    let mut x = l;
    while x < rr {
        invert(hdc, [x, t, x + 1, t + 1]);
        if b - 1 > t {
            invert(hdc, [x, b - 1, x + 1, b]);
        }
        x += 2;
    }
    let mut y = t + 2;
    while y < b - 1 {
        invert(hdc, [l, y, l + 1, y + 1]);
        invert(hdc, [rr - 1, y, rr, y + 1]);
        y += 2;
    }
}

/// Clip the DC's drawing to a rectangle (until `unclip`).
pub fn clip(hdc: u16, r: [i32; 4]) {
    let Some(d) = dc(hdc) else { return };
    let rc = pm_rect(d, r);
    unsafe { GpiIntersectClipRectangle(d.hps, &rc) };
    d.clipped = true;
}

pub fn unclip(hdc: u16) {
    let Some(d) = dc(hdc) else { return };
    if d.clipped {
        let mut old = 0;
        unsafe { GpiSetClipRegion(d.hps, 0, &mut old) };
        d.clipped = false;
    }
}

/// Make the DC's font the PS's current one.
fn apply_font(d: &mut Dc) {
    if d.applied == d.s.font {
        return;
    }
    let f = match obj(d.s.font) {
        Some(Obj::Font(f)) => f,
        _ => sys_font(),
    };
    unsafe {
        GpiSetCharSet(d.hps, 0);
        if d.applied != 0 && d.applied != SYSTEM_FONT {
            GpiDeleteSetId(d.hps, 1);
        }
    }
    d.applied = d.s.font;
    if f.face == 0 && !f.italic && !f.underline {
        return;
    }
    let mut fat = [0u8; 56];
    fat[0..2].copy_from_slice(&56u16.to_le_bytes());
    let mut sel = 0u16;
    if f.italic {
        sel |= 1;
    }
    if f.underline {
        sel |= 2;
    }
    if f.weight >= 600 && f.face != 0 {
        sel |= 0x20;
    }
    fat[2..4].copy_from_slice(&sel.to_le_bytes());
    let face: &[u8] = match f.face {
        0 => b"System Proportional",
        1 => b"System Monospaced",
        3 => b"Times New Roman",
        4 => b"Courier",
        _ => b"Helvetica",
    };
    fat[8..8 + face.len()].copy_from_slice(face);
    let outline = f.face >= 2;
    if outline {
        // FATTR_FONTUSE_OUTLINE
        fat[54..56].copy_from_slice(&0x0010u16.to_le_bytes());
    } else if f.face == 1 {
        fat[44..48].copy_from_slice(&16i32.to_le_bytes());
    }
    unsafe {
        if GpiCreateLogFont(d.hps, core::ptr::null(), 1, fat.as_ptr()) == 0 {
            return;
        }
        GpiSetCharSet(d.hps, 1);
        if outline {
            // The em height: a cell height (positive) is about 1.2 em.
            let h = if f.height < 0 { -f.height } else if f.height > 0 { f.height * 5 / 6 } else { 13 };
            let w = if f.width > 0 { f.width * 2 } else { h };
            let bx = [w.max(1) << 16, h.max(1) << 16];
            GpiSetCharBox(d.hps, bx.as_ptr());
        }
    }
}

/// The font's ascender, descender, average width and widest character.
fn metrics(d: &mut Dc) -> (i32, i32, i32, i32) {
    apply_font(d);
    let mut fm = [0u8; 256];
    unsafe { GpiQueryFontMetrics(d.hps, 208, fm.as_mut_ptr()) };
    let l = |o: usize| i32::from_le_bytes(fm[o..o + 4].try_into().unwrap());
    // lMaxAscender, lMaxDescender, lAveCharWidth, lMaxCharInc.
    let (asc, desc) = (l(76), l(80));
    (asc.max(1), desc.max(0), l(100).max(1), l(104).max(1))
}

fn text_width_ps(hps: u32, s: &[u8]) -> i32 {
    if s.is_empty() {
        return 0;
    }
    let mut pts = [PointL::default(); 5];
    unsafe { GpiQueryTextBox(hps, s.len() as i32, s.as_ptr(), TXTBOX_COUNT, pts.as_mut_ptr()) };
    pts[TXTBOX_CONCAT].x
}

/// The width of each character of the DC's font.
pub fn char_widths(hdc: u16) -> [u8; 256] {
    let Some(d) = dc(hdc) else { return [8; 256] };
    if let Some(w) = g().widths.get(&d.s.font) {
        return *w;
    }
    apply_font(d);
    let mut w = [0u8; 256];
    for (c, v) in w.iter_mut().enumerate() {
        *v = text_width_ps(d.hps, &[c as u8]).clamp(0, 255) as u8;
    }
    w[0] = 0;
    let f = d.s.font;
    g().widths.insert(f, w);
    w
}

/// The width of a string in the DC's font.
pub fn text_width(hdc: u16, s: &[u8]) -> i32 {
    let w = char_widths(hdc);
    s.iter().map(|&c| w[c as usize] as i32).sum()
}

/// The height of a line of text (ascender + descender).
pub fn line_height(hdc: u16) -> i32 {
    let Some(d) = dc(hdc) else { return 16 };
    let (a, de, _, _) = metrics(d);
    a + de
}

/// The average character width of the DC's font.
pub fn ave_width(hdc: u16) -> i32 {
    let Some(d) = dc(hdc) else { return 8 };
    metrics(d).2
}

pub fn set_text_color(hdc: u16, c: u32) -> u32 {
    dc(hdc).map_or(0, |d| core::mem::replace(&mut d.s.text, c & 0xffffff))
}

pub fn set_bk_color(hdc: u16, c: u32) -> u32 {
    dc(hdc).map_or(0, |d| core::mem::replace(&mut d.s.bk, c & 0xffffff))
}

pub fn set_bk_mode(hdc: u16, m: u16) -> u16 {
    dc(hdc).map_or(0, |d| core::mem::replace(&mut d.s.bk_mode, m))
}

pub fn text_color(hdc: u16) -> u32 {
    dc(hdc).map_or(0, |d| d.s.text)
}

pub fn bk_color(hdc: u16) -> u32 {
    dc(hdc).map_or(0xffffff, |d| d.s.bk)
}

/// Select a font (0: the system font): the previous one.
pub fn select_font(hdc: u16, f: u16) -> u16 {
    let f = if f == 0 || !is_font(f) { SYSTEM_FONT } else { f };
    dc(hdc).map_or(0, |d| core::mem::replace(&mut d.s.font, f))
}

/// Text with its top-left corner at (x, y), in the DC's colours and
/// background mode.
pub fn text_at(hdc: u16, x: i32, y: i32, s: &[u8]) {
    let Some(d) = dc(hdc) else { return };
    if s.is_empty() {
        return;
    }
    let (asc, _, _, _) = metrics(d);
    set_text_attrs(d);
    let (x, y) = to_dev(d, x, y);
    let p = PointL { x, y: d.height - y - asc };
    unsafe { GpiCharStringAt(d.hps, &p, s.len() as i32, s.as_ptr()) };
}

/// Line from (x0, y0) to (x1, y1), the last pel excluded, in a colour.
pub fn line(hdc: u16, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
    if x0 == x1 {
        fill(hdc, [x0, y0.min(y1), x0 + 1, y0.max(y1)], c);
    } else if y0 == y1 {
        fill(hdc, [x0.min(x1), y0, x0.max(x1), y0 + 1], c);
    } else {
        let Some(d) = dc(hdc) else { return };
        unsafe {
            GpiSetColor(d.hps, rgb(c));
            GpiSetLineWidth(d.hps, 1 << 16);
            GpiMove(d.hps, &pm_pt(d, x0, y0));
            GpiLine(d.hps, &pm_pt(d, x1, y1));
        }
    }
}

fn set_text_attrs(d: &Dc) {
    unsafe {
        GpiSetColor(d.hps, rgb(d.s.text));
        GpiSetBackColor(d.hps, rgb(d.s.bk));
        GpiSetBackMix(d.hps, if d.s.bk_mode == OPAQUE { BM_OVERPAINT } else { BM_LEAVEALONE });
    }
}

/// Windows' & prefixes as PM's ~ mnemonics ("&&" is an ampersand).
fn mnemonics(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'&' if s.get(i + 1) == Some(&b'&') => {
                v.push(b'&');
                i += 1;
            }
            b'&' => v.push(b'~'),
            b'~' => v.extend_from_slice(b"~~"),
            c => v.push(c),
        }
        i += 1;
    }
    v
}

pub const DT_CENTER: u16 = 0x01;
pub const DT_RIGHT: u16 = 0x02;
pub const DT_VCENTER: u16 = 0x04;
pub const DT_BOTTOM: u16 = 0x08;
pub const DT_WORDBREAK: u16 = 0x10;
pub const DT_SINGLELINE: u16 = 0x20;
pub const DT_EXPANDTABS: u16 = 0x40;
pub const DT_NOCLIP: u16 = 0x100;
pub const DT_CALCRECT: u16 = 0x400;
pub const DT_NOPREFIX: u16 = 0x800;

/// DrawText(hdc, text, count, lpRect, format): the height of the text.
pub fn draw_text(hdc: u16, s: u32, n: i32, r: u32, fmt: u16) -> i32 {
    let Some(rect) = read_rect(r) else { return 0 };
    let all = far_str(s).unwrap_or(b"");
    let text = if n < 0 { all } else { unsafe { core::slice::from_raw_parts(flat(s), n as usize) } };
    let (h, out) = draw_text_bytes(hdc, text, rect, fmt);
    if fmt & DT_CALCRECT != 0 {
        write_rect(r, out);
    }
    h
}

/// DrawText on bytes: the height and (DT_CALCRECT) the rectangle.
pub fn draw_text_bytes(hdc: u16, text: &[u8], rect: [i32; 4], fmt: u16) -> (i32, [i32; 4]) {
    let Some(d) = dc(hdc) else { return (0, rect) };
    let text = if fmt & DT_NOPREFIX == 0 { mnemonics(text) } else { text.to_vec() };
    let (asc, desc, _, _) = metrics(d);
    let lh = asc + desc;
    let mut fl = 0u32;
    if fmt & DT_NOPREFIX == 0 {
        fl |= 0x2000; // DT_MNEMONIC
    }
    if fmt & DT_CENTER != 0 {
        fl |= 0x0100;
    } else if fmt & DT_RIGHT != 0 {
        fl |= 0x0200;
    }
    if fmt & DT_WORDBREAK != 0 {
        fl |= 0x4000;
    }
    // The lines: one, or each line of the text (broken at words).
    let mut lines: Vec<&[u8]> = Vec::new();
    if fmt & DT_SINGLELINE != 0 {
        lines.push(&text);
    } else {
        for l in text.split(|&c| c == b'\n') {
            lines.push(l.strip_suffix(b"\r").unwrap_or(l));
        }
    }
    let pr = pm_rect(d, rect);
    let (fg, bg, opaque) = (rgb(d.s.text), rgb(d.s.bk), d.s.bk_mode == OPAQUE);
    let hps = d.hps;
    let mut top = pr.t;
    let mut widest = 0;
    let mut count = 0;
    for line in lines {
        let mut rest: &[u8] = line;
        loop {
            let mut lr = RectL { l: pr.l, b: top - lh, r: pr.r, t: top };
            if fmt & DT_SINGLELINE != 0 {
                lr.b = pr.b;
                if fmt & DT_VCENTER != 0 {
                    fl |= 0x0400;
                } else if fmt & DT_BOTTOM != 0 {
                    fl |= 0x0800;
                }
            }
            if fmt & DT_CALCRECT != 0 && fmt & DT_WORDBREAK == 0 {
                lr.r = lr.l + 0x7fff;
            }
            // The extent: for DT_CALCRECT and the opaque background.
            let mut ext = lr;
            let used = if rest.is_empty() { 0 } else { unsafe { WinDrawText(hps, rest.len() as i32, rest.as_ptr(), &mut ext, fg, bg, fl | 0x0002) } };
            if rest.is_empty() {
                ext.r = ext.l;
            }
            widest = widest.max(ext.r - ext.l);
            if fmt & DT_CALCRECT == 0 && !rest.is_empty() {
                if opaque {
                    unsafe { WinFillRect(hps, &ext, bg) };
                }
                let mut dr = lr;
                unsafe { WinDrawText(hps, rest.len() as i32, rest.as_ptr(), &mut dr, fg, bg, fl) };
            }
            count += 1;
            top -= lh;
            let used = (used.max(1) as usize).min(rest.len());
            rest = &rest[used..];
            if fmt & DT_WORDBREAK != 0 {
                while rest.first() == Some(&b' ') {
                    rest = &rest[1..];
                }
            }
            if rest.is_empty() || fmt & DT_WORDBREAK == 0 {
                break;
            }
        }
    }
    let height = count * lh;
    (height, [rect[0], rect[1], rect[0] + widest, rect[1] + height])
}

/// A pen's colour, unless it is the null pen.
fn pen_color(d: &Dc) -> Option<(u32, i32)> {
    match obj(d.s.pen) {
        Some(Obj::Pen { style, color, width }) if style != 5 => Some((color, width.max(1) as i32)),
        _ => None,
    }
}

/// An ellipse in the rectangle: the brush inside, the pen around.
fn ellipse(d: &Dc, l: i32, t: i32, r: i32, b: i32) {
    let rc = pm_rect(d, [l, t, r, b]);
    let (rx, ry) = ((rc.r - rc.l) / 2, (rc.t - rc.b) / 2);
    let c = PointL { x: rc.l + rx, y: rc.b + ry };
    unsafe {
        GpiSetArcParams(d.hps, &[rx.max(1), ry.max(1), 0, 0]);
        GpiMove(d.hps, &c);
        if let Some(col) = brush_color(d.s.brush) {
            GpiSetColor(d.hps, rgb(col));
            GpiFullArc(d.hps, DRO_FILL, 1 << 16);
        }
        if let Some((col, _)) = pen_color(d) {
            GpiSetColor(d.hps, rgb(col));
            GpiFullArc(d.hps, DRO_OUTLINE, 1 << 16);
        }
    }
}

fn points(p: *const i16, n: usize, d: &Dc) -> Vec<PointL> {
    (0..n).map(|i| unsafe { pm_pt(d, p.add(2 * i).read_unaligned() as i32, p.add(2 * i + 1).read_unaligned() as i32) }).collect()
}

/// LOGFONT -> Font: lfHeight, lfWidth, lfEscapement, lfOrientation,
/// lfWeight, lfItalic, lfUnderline, ..., lfPitchAndFamily, lfFaceName.
fn font_of(height: i32, width: i32, weight: u16, italic: bool, underline: bool, family: u8, face: &[u8]) -> Font {
    let f = face.to_ascii_lowercase();
    let has = |s: &[u8]| f.windows(s.len()).any(|w| w == s);
    let face = if has(b"courier") || has(b"fixed") || has(b"terminal") || (f.is_empty() && family & 3 == 1) {
        4
    } else if has(b"times") || has(b"tms") || has(b"roman") || has(b"serif") && !has(b"sans") || (f.is_empty() && family & 0xf0 == 0x10) {
        3
    } else if has(b"system") && height == 0 {
        0
    } else {
        2
    };
    Font { face, height, width, weight, italic, underline }
}

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    if let Some(r) = mapping(ord, a) {
        return r;
    }
    match ord {
        // SetBkColor(hdc, color), SetTextColor(hdc, color): the old one.
        1 | 9 => {
            let (h, c) = (a.w(), a.l());
            if dc(h).is_none() {
                return 0x8000_0000;
            }
            if ord == 1 { set_bk_color(h, c) } else { set_text_color(h, c) }
        }
        // SetBkMode(hdc, mode)
        2 => {
            let (h, m) = (a.w(), a.w());
            if m != OPAQUE && m != TRANSPARENT {
                return 0;
            }
            set_bk_mode(h, m) as u32
        }
        // SetROP2(hdc, rop)
        4 => {
            let (h, r) = (a.w(), a.w());
            dc(h).map_or(0, |d| core::mem::replace(&mut d.s.rop2, r) as u32)
        }
        // ExcludeClipRect, IntersectClipRect(hdc, l, t, r, b)
        21 | 22 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let rc = pm_rect(d, [l, t, r, b]);
            d.clipped = true;
            (unsafe { if ord == 21 { GpiExcludeClipRectangle(d.hps, &rc) } else { GpiIntersectClipRectangle(d.hps, &rc) } }) as u32
        }
        // Ellipse(hdc, l, t, r, b)
        24 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            ellipse(d, l, t, r, b);
            1
        }
        // RoundRect(hdc, l, t, r, b, w, h): a rectangle.
        28 => {
            let (h, l, t, r, b, _, _) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i());
            rectangle(h, l, t, r, b)
        }
        // PatBlt(hdc, x, y, w, h, rop)
        29 => {
            let (h, x, y, w, hh, rop) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.l());
            let r = [x.min(x + w), y.min(y + hh), x.max(x + w), y.max(y + hh)];
            match rop {
                0x0055_0009 | 0x005a_0049 => invert(h, r), // DSTINVERT, PATINVERT
                0x0000_0042 => fill(h, r, 0),              // BLACKNESS
                0x00ff_0062 => fill(h, r, 0xffffff),       // WHITENESS
                _ => {
                    if let Some(b) = dc(h).map(|d| d.s.brush) {
                        fill_rect(h, r, b);
                    }
                }
            }
            1
        }
        // SaveDC(hdc): the level.
        30 => {
            let h = a.w();
            let Some(d) = dc(h) else { return 0 };
            d.saved.push((d.s, d.clipped));
            d.saved.len() as u32
        }
        // SetPixel(hdc, x, y, color)
        31 => {
            let (h, x, y, c) = (a.w(), a.i(), a.i(), a.l());
            fill(h, [x, y, x + 1, y + 1], c & 0xffffff);
            c & 0xffffff
        }
        // Rectangle(hdc, left, top, right, bottom): the brush inside, the
        // pen around.
        27 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            rectangle(h, l, t, r, b)
        }
        // LineTo(hdc, x, y)
        19 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let (x0, y0) = d.s.pos;
            d.s.pos = (x, y);
            let Some((color, width)) = pen_color(d) else { return 1 };
            let color = if d.s.rop2 == 6 || d.s.rop2 == 7 { 0xffffff ^ color } else { color };
            let from = pm_pt(d, x0, y0);
            let to = pm_pt(d, x, y);
            unsafe {
                GpiSetColor(d.hps, rgb(color));
                GpiSetLineWidth(d.hps, width << 16);
                GpiMove(d.hps, &from);
                GpiLine(d.hps, &to);
            }
            1
        }
        // MoveTo(hdc, x, y): the old position (x in AX, y in DX).
        20 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let (ox, oy) = core::mem::replace(&mut d.s.pos, (x, y));
            (oy as u32 & 0xffff) << 16 | ox as u32 & 0xffff
        }
        // TextOut(hdc, x, y, text, count)
        33 => {
            let (h, x, y, s, n) = (a.w(), a.i(), a.i(), a.l(), a.i());
            if dc(h).is_none() {
                return 0;
            }
            if n <= 0 {
                return 1;
            }
            let text = unsafe { core::slice::from_raw_parts(flat(s), n as usize) };
            text_out(h, x, y, text);
            1
        }
        // Polygon(hdc, points, n), Polyline(hdc, points, n)
        36 | 37 => {
            let (h, p, n) = (a.w(), a.p() as *const i16, a.i());
            let Some(d) = dc(h) else { return 0 };
            if p.is_null() || n < 2 {
                return 0;
            }
            let pts = points(p, n as usize, d);
            unsafe {
                if ord == 36 {
                    if let Some(c) = brush_color(d.s.brush) {
                        GpiSetColor(d.hps, rgb(c));
                        GpiBeginArea(d.hps, 0);
                        GpiMove(d.hps, &pts[0]);
                        GpiPolyLine(d.hps, n - 1, pts[1..].as_ptr());
                        GpiEndArea(d.hps);
                    }
                }
                if let Some((c, w)) = pen_color(d) {
                    GpiSetColor(d.hps, rgb(c));
                    GpiSetLineWidth(d.hps, w << 16);
                    GpiMove(d.hps, &pts[0]);
                    GpiPolyLine(d.hps, n - 1, pts[1..].as_ptr());
                    if ord == 36 {
                        GpiLine(d.hps, &pts[0]);
                    }
                }
            }
            1
        }
        // RestoreDC(hdc, level)
        39 => {
            let (h, lvl) = (a.w(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let n = d.saved.len() as i32;
            let keep = if lvl < 0 { n + lvl } else { lvl - 1 };
            if keep < 0 || keep >= n {
                return 0;
            }
            let (s, clipped) = d.saved[keep as usize];
            d.saved.truncate(keep as usize);
            d.s = s;
            if d.clipped && !clipped {
                let mut old = 0;
                unsafe { GpiSetClipRegion(d.hps, 0, &mut old) };
                d.clipped = false;
            }
            1
        }
        // SelectObject(hdc, obj): the previous object of its kind.
        45 => {
            let (h, o) = (a.w(), a.w());
            let Some(d) = dc(h) else { return 0 };
            match obj(o) {
                Some(Obj::Brush { .. }) => core::mem::replace(&mut d.s.brush, o) as u32,
                Some(Obj::Pen { .. }) => core::mem::replace(&mut d.s.pen, o) as u32,
                Some(Obj::Font(_)) => core::mem::replace(&mut d.s.font, o) as u32,
                Some(Obj::Bitmap { .. }) => crate::bitmap::select(h, o) as u32,
                // A region: SelectClipRgn's result (the region type).
                Some(Obj::Region(_)) => crate::region::select_clip(h, o) as u32,
                _ => 0,
            }
        }
        // CreateBrushIndirect(LOGBRUSH: style, color, hatch)
        50 => {
            let p = a.p();
            if p.is_null() {
                return 0;
            }
            let style = unsafe { (p as *const u16).read_unaligned() };
            let c = unsafe { (p.add(2) as *const u32).read_unaligned() };
            new_obj(Obj::Brush { color: c & 0xffffff, null: style == 1 }) as u32
        }
        // CreateFont(height, width, esc, orient, weight, italic, underline,
        // strikeout, charset, outprec, clipprec, quality, pitch, face)
        56 => {
            let (hh, w, _, _, weight) = (a.i(), a.i(), a.i(), a.i(), a.w());
            let (it, ul, _, _, _, _, _, pitch) = (a.w(), a.w(), a.w(), a.w(), a.w(), a.w(), a.w(), a.w());
            let face = far_str(a.l()).unwrap_or(b"");
            new_obj(Obj::Font(font_of(hh, w, weight, it != 0, ul != 0, pitch as u8, face))) as u32
        }
        // CreateFontIndirect(LOGFONT)
        57 => {
            let p = a.p();
            if p.is_null() {
                return 0;
            }
            let w = |o: usize| unsafe { (p.add(o) as *const i16).read_unaligned() } as i32;
            let face = unsafe { crate::cstr(p.add(18)) };
            let face = &face[..face.len().min(32)];
            let b = |o: usize| unsafe { *p.add(o) };
            new_obj(Obj::Font(font_of(w(0), w(2), w(8) as u16, b(10) != 0, b(11) != 0, b(17), face))) as u32
        }
        // CreatePen(style, width, color)
        61 => {
            let (style, width, c) = (a.w(), a.w(), a.l());
            new_obj(Obj::Pen { style, width, color: c & 0xffffff }) as u32
        }
        // CreatePenIndirect(LOGPEN: style, POINT width, color)
        62 => {
            let p = a.p();
            if p.is_null() {
                return 0;
            }
            let style = unsafe { (p as *const u16).read_unaligned() };
            let width = unsafe { (p.add(2) as *const u16).read_unaligned() };
            let c = unsafe { (p.add(6) as *const u32).read_unaligned() };
            new_obj(Obj::Pen { style, width, color: c & 0xffffff }) as u32
        }
        // CreateSolidBrush(color), CreateHatchBrush(style, color): solid.
        58 | 66 => {
            if ord == 58 {
                a.w();
            }
            let c = a.l();
            new_obj(Obj::Brush { color: c & 0xffffff, null: false }) as u32
        }
        // DeleteObject(obj): not the stock ones.
        69 => {
            let o = a.w();
            delete_object(o) as u32
        }
        // GetBkColor(hdc), GetBkMode(hdc), GetTextColor(hdc)
        75 => bk_color(a.w()),
        76 => dc(a.w()).map_or(0, |d| d.s.bk_mode as u32),
        90 => text_color(a.w()),
        // GetClipBox(hdc, lpRect): the window's client area.
        77 => {
            let (h, r) = (a.w(), a.l());
            let Some(d) = dc(h) else { return 0 };
            let mut rc = RectL::default();
            unsafe { GpiQueryClipBox(d.hps, &mut rc) };
            let (l, t) = to_log(d, rc.l, d.height - rc.t - 1);
            let (rr, b) = to_log(d, rc.r + 1, d.height - rc.b);
            write_rect(r, [l.min(rr), t.min(b), l.max(rr), t.max(b)]);
            2
        }
        // GetCurrentPosition(hdc): x in AX, y in DX.
        78 => {
            let h = a.w();
            dc(h).map_or(0, |d| (d.s.pos.1 as u32 & 0xffff) << 16 | d.s.pos.0 as u32 & 0xffff)
        }
        // GetObject(obj, cb, buf)
        82 => {
            let (o, cb, p) = (a.w(), a.i(), a.p());
            get_object(o, cb, p) as u32
        }
        // GetPixel(hdc, x, y)
        83 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let c = unsafe { GpiQueryPel(d.hps, &pm_pt(d, x, y)) } as u32;
            // PM's RGB back to a COLORREF.
            (c & 0xff) << 16 | (c & 0xff00) | (c >> 16) & 0xff
        }
        // GetROP2(hdc)
        85 => dc(a.w()).map_or(0, |d| d.s.rop2 as u32),
        // GetDeviceCaps(hdc, index)
        80 => {
            let (_h, i) = (a.w(), a.w());
            let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
            match i {
                0 => 0x300,           // DRIVERVERSION
                2 => 1,               // TECHNOLOGY: DT_RASDISPLAY
                4 => 240,             // HORZSIZE (mm)
                6 => 180,             // VERTSIZE
                8 => q(SV_CXSCREEN) as u32,
                10 => q(SV_CYSCREEN) as u32,
                12 => 24,             // BITSPIXEL
                14 => 1,              // PLANES
                24 => u32::MAX >> 16, // NUMCOLORS: many
                26 => 0x7fff,         // NUMPENS...
                38 => 0x0001 | 0x0008 | 0x0080 | 0x0200, // RASTERCAPS: BITBLT, BITMAP64, DI_BITMAP, STRETCHBLT
                40 => 36,             // ASPECTX
                42 => 36,             // ASPECTY
                44 => 51,             // ASPECTXY
                88 | 90 => 96,        // LOGPIXELSX/Y
                _ => 0,
            }
        }
        // GetStockObject(i)
        87 => {
            let i = a.w();
            if i <= 16 && i != 9 {
                g();
                (OBJ_BASE + 2 * i) as u32
            } else {
                0
            }
        }
        // GetTextExtent(hdc, text, count): width in AX, height in DX.
        91 => {
            let (h, s, n) = (a.w(), a.l(), a.i());
            if dc(h).is_none() {
                return 0;
            }
            let text = if n > 0 { unsafe { core::slice::from_raw_parts(flat(s), n as usize) } } else { &[] };
            (line_height(h) as u32) << 16 | text_width(h, text) as u32 & 0xffff
        }
        // GetTextFace(hdc, cb, buf)
        92 => {
            let (h, cb, buf) = (a.w(), a.i(), a.p());
            let face: &[u8] = match dc(h).and_then(|d| obj(d.s.font)) {
                Some(Obj::Font(f)) => match f.face {
                    0 => b"System",
                    1 => b"Fixedsys",
                    3 => b"Times New Roman",
                    4 => b"Courier New",
                    _ => b"Arial",
                },
                _ => b"System",
            };
            if buf.is_null() || cb <= 0 {
                return 0;
            }
            let n = face.len().min(cb as usize - 1);
            unsafe {
                core::ptr::copy_nonoverlapping(face.as_ptr(), buf, n);
                *buf.add(n) = 0;
            }
            n as u32
        }
        // GetTextMetrics(hdc, lpTextMetric)
        93 => {
            let (h, p) = (a.w(), a.p());
            let Some(d) = dc(h) else { return 0 };
            if p.is_null() {
                return 0;
            }
            let (asc, desc, ave, max) = metrics(d);
            let (weight, italic, ul, fixed) = match obj(d.s.font) {
                Some(Obj::Font(f)) => (f.weight.max(400), f.italic, f.underline, f.face == 1 || f.face == 4),
                _ => (700, false, false, false),
            };
            let mut b = [0u8; 31];
            for (i, v) in [asc + desc, asc, desc, 0, 0, ave, max, weight as i32].iter().enumerate() {
                b[i * 2..i * 2 + 2].copy_from_slice(&(*v as i16).to_le_bytes());
            }
            // tmItalic.. tmCharSet: first char 0x20, last 0xff, default
            // '?', break ' ', the pitch (bit 0: variable) and family,
            // ANSI charset.
            let pitch = if fixed { 0x30 } else { 0x21 };
            b[16..25].copy_from_slice(&[italic as u8, ul as u8, 0, 0x20, 0xff, b'?', b' ', pitch, 0]);
            for (i, v) in [0i16, 96, 96].iter().enumerate() {
                b[25 + i * 2..27 + i * 2].copy_from_slice(&v.to_le_bytes());
            }
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 31) };
            1
        }
        // MulDiv(a, b, c)
        128 => {
            let (x, y, z) = (a.i(), a.i(), a.i());
            if z == 0 {
                return 0xffff_8001 & 0xffff;
            }
            let r = (x as i64 * y as i64 + (z as i64 / 2) * if (x as i64 * y as i64) < 0 { -1 } else { 1 }) / z as i64;
            r.clamp(-32768, 32767) as i16 as u16 as u32
        }
        // GetTextAlign(hdc), SetTextAlign(hdc, align)
        345 => dc(a.w()).map_or(0, |d| d.s.align as u32),
        346 => {
            let (h, al) = (a.w(), a.w());
            dc(h).map_or(0, |d| core::mem::replace(&mut d.s.align, al) as u32)
        }
        // GetCharWidth(hdc, first, last, buf)
        350 => {
            let (h, first, last, p) = (a.w(), a.w(), a.w(), a.p() as *mut i16);
            if p.is_null() || dc(h).is_none() {
                return 0;
            }
            let w = char_widths(h);
            for (i, c) in (first..=last.min(255)).enumerate() {
                unsafe { p.add(i).write_unaligned(w[c as usize] as i16) };
            }
            1
        }
        // ExtTextOut(hdc, x, y, options, lpRect, text, count, lpDx)
        351 => {
            let (h, x, y, opts, r, s, n, _dx) = (a.w(), a.i(), a.i(), a.w(), a.l(), a.l(), a.w(), a.l());
            if dc(h).is_none() {
                return 0;
            }
            let rect = read_rect(r);
            if let Some(rc) = rect {
                if opts & 2 != 0 {
                    // ETO_OPAQUE
                    fill(h, rc, bk_color(h));
                }
                if opts & 4 != 0 {
                    // ETO_CLIPPED
                    clip(h, rc);
                }
            }
            if n > 0 {
                let text = unsafe { core::slice::from_raw_parts(flat(s), n as usize) };
                text_out(h, x, y, text);
            }
            if rect.is_some() && opts & 4 != 0 {
                unclip(h);
            }
            1
        }
        _ => 0,
    }
}

/// TextOut: the alignment (TA_*) of the DC; TA_UPDATECP moves the
/// current position.
fn text_out(h: u16, x: i32, y: i32, text: &[u8]) {
    let Some(d) = dc(h) else { return };
    let (asc, desc, _, _) = metrics(d);
    let align = d.s.align;
    let (x, y) = if align & 1 != 0 { d.s.pos } else { (x, y) };
    let w = text_width(h, text);
    let x0 = match align & 6 {
        2 => x - w,
        6 => x - w / 2,
        _ => x,
    };
    let y0 = match align & 24 {
        8 => y - asc - desc,
        24 => y - asc,
        _ => y,
    };
    text_at(h, x0, y0, text);
    if align & 1 != 0 {
        if let Some(d) = dc(h) {
            d.s.pos = (x + w, y);
        }
    }
}

fn rectangle(h: u16, l: i32, t: i32, r: i32, b: i32) -> u32 {
    let Some(d) = dc(h) else { return 0 };
    let rc = pm_rect(d, [l, t, r, b]);
    if let Some(c) = brush_color(d.s.brush) {
        unsafe { WinFillRect(d.hps, &rc, rgb(c)) };
    }
    if let Some((color, _)) = pen_color(d) {
        unsafe {
            GpiSetColor(d.hps, rgb(color));
            GpiMove(d.hps, &PointL { x: rc.l, y: rc.b });
            GpiBox(d.hps, DRO_OUTLINE, &PointL { x: rc.r - 1, y: rc.t - 1 }, 0, 0);
        }
    }
    1
}

/// GetObject: LOGBRUSH, LOGPEN or LOGFONT.
fn get_object(o: u16, cb: i32, p: *mut u8) -> i32 {
    let mut b = Vec::new();
    match obj(o) {
        Some(Obj::Brush { color, null }) => {
            b.extend_from_slice(&(if null { 1u16 } else { 0 }).to_le_bytes());
            b.extend_from_slice(&color.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes());
        }
        Some(Obj::Pen { style, width, color }) => {
            b.extend_from_slice(&style.to_le_bytes());
            b.extend_from_slice(&width.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes());
            b.extend_from_slice(&color.to_le_bytes());
        }
        // BITMAP: type, width, height, bytes per row (words), planes,
        // bits per pixel, bits (none).
        Some(Obj::Bitmap { w, h, bpp, .. }) => {
            for v in [0, w, h, ((w * bpp as i32 + 15) / 16) * 2] {
                b.extend_from_slice(&(v as i16).to_le_bytes());
            }
            b.extend_from_slice(&[1, bpp as u8, 0, 0, 0, 0]);
        }
        // A palette: its number of entries.
        Some(Obj::Palette(p)) => b.extend_from_slice(&(p.len() as u16).to_le_bytes()),
        Some(Obj::Font(f)) => {
            let h = if f.height == 0 { 16 } else { f.height };
            for v in [h, f.width, 0, 0, f.weight as i32] {
                b.extend_from_slice(&(v as i16).to_le_bytes());
            }
            b.extend_from_slice(&[f.italic as u8, f.underline as u8, 0, 0, 0, 0, 0, if f.face == 1 || f.face == 4 { 1 } else { 2 }]);
            let face: &[u8] = match f.face {
                0 => b"System",
                1 => b"Fixedsys",
                3 => b"Times New Roman",
                4 => b"Courier New",
                _ => b"Arial",
            };
            let mut name = [0u8; 32];
            name[..face.len()].copy_from_slice(face);
            b.extend_from_slice(&name);
        }
        _ => return 0,
    }
    if p.is_null() {
        return b.len() as i32;
    }
    let n = b.len().min(cb.max(0) as usize);
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, n) };
    n as i32
}

/// Windows 3.1's default system colours (COLORREF).
pub fn sys_color(i: u16) -> u32 {
    const COLORS: [u32; 21] = [
        0xc0c0c0, // SCROLLBAR
        0xc0c0c0, // BACKGROUND
        0x800000, // ACTIVECAPTION
        0xffffff, // INACTIVECAPTION
        0xffffff, // MENU
        0xffffff, // WINDOW
        0x000000, // WINDOWFRAME
        0x000000, // MENUTEXT
        0x000000, // WINDOWTEXT
        0xffffff, // CAPTIONTEXT
        0xc0c0c0, // ACTIVEBORDER
        0xc0c0c0, // INACTIVEBORDER
        0xffffff, // APPWORKSPACE
        0x800000, // HIGHLIGHT
        0xffffff, // HIGHLIGHTTEXT
        0xc0c0c0, // BTNFACE
        0x808080, // BTNSHADOW
        0x808080, // GRAYTEXT
        0x000000, // BTNTEXT
        0x000000, // INACTIVECAPTIONTEXT
        0xffffff, // BTNHIGHLIGHT
    ];
    COLORS.get(i as usize).copied().unwrap_or(0)
}

pub const COLOR_SCROLLBAR: u16 = 0;
pub const COLOR_WINDOW: u16 = 5;
pub const COLOR_WINDOWFRAME: u16 = 6;
pub const COLOR_WINDOWTEXT: u16 = 8;
pub const COLOR_HIGHLIGHT: u16 = 13;
pub const COLOR_HIGHLIGHTTEXT: u16 = 14;
pub const COLOR_BTNFACE: u16 = 15;
pub const COLOR_BTNSHADOW: u16 = 16;
pub const COLOR_GRAYTEXT: u16 = 17;
pub const COLOR_BTNTEXT: u16 = 18;
pub const COLOR_BTNHIGHLIGHT: u16 = 20;
