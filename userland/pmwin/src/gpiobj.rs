//! GPI objects: regions and clipping, device contexts, bitmaps and
//! bit-block transfers, logical colour tables and palettes, and the
//! PMWIN calls built on them (WinOpenWindowDC, WinDrawBitmap,
//! WinQueryUpdateRegion, WinValidateRegion, WinRealizePalette).

#![allow(non_snake_case)]

use alloc::vec;
use alloc::vec::Vec;

use gpi::bitmap::{self, Bitmap, Info};
use gpi::region::{Op, Region};
use gpi::{Point, Rect, Surface};

use crate::gpiapi::{self, draw, entry, point, points, read, write, PointL, RectL, Target, GPI_ERROR, GPI_OK};
use crate::pointer;
use crate::state::st;

// ---- Object tables ----

enum DcKind {
    Window(u32),
    Memory,
    Info,
    Metafile(alloc::boxed::Box<crate::gpiseg::MetaRec>),
}

struct Bm {
    bm: Bitmap,
    /// Selected into a presentation space.
    selected: bool,
}

#[derive(Default)]
pub struct Objects {
    regions: Vec<Option<Region>>,
    bitmaps: Vec<Option<Bm>>,
    dcs: Vec<Option<DcKind>>,
    palettes: Vec<Option<Vec<u32>>>,
    /// The presentation spaces that stand for DCs used as `hdc | 1`
    /// (hdc, hps).
    dc_pss: Vec<(u32, u32)>,
}

/// The calling process's GPI objects.
fn objects() -> &'static mut Objects {
    st().objects.entry(crate::state::pid()).or_default()
}

const HRGN_BASE: u32 = 0x1000_0001;
const HBM_BASE: u32 = 0x2000_0001;
/// DC handles are even: OS/2's GPI takes `hdc | 1` for a presentation
/// space that draws on the DC itself (OS/2's Icon Editor uses that).
const HDC_BASE: u32 = 0x3000_0002;

fn dc_index(h: u32) -> Option<usize> {
    (h & 1 == 0 && h >= HDC_BASE).then(|| ((h - HDC_BASE) / 2) as usize)
}

fn dc_handle(i: usize) -> u32 {
    HDC_BASE + 2 * i as u32
}

fn dc_get(h: u32) -> Option<&'static mut DcKind> {
    objects().dcs.get_mut(dc_index(h)?)?.as_mut()
}

fn dc_add(k: DcKind) -> u32 {
    let v = &mut objects().dcs;
    match v.iter().position(|p| p.is_none()) {
        Some(i) => {
            v[i] = Some(k);
            dc_handle(i)
        }
        None => {
            v.push(Some(k));
            dc_handle(v.len() - 1)
        }
    }
}

/// What a metafile DC has recorded.
pub fn meta_rec(hdc: u32) -> Option<&'static mut crate::gpiseg::MetaRec> {
    match dc_get(hdc)? {
        DcKind::Metafile(m) => Some(m),
        _ => None,
    }
}

/// The presentation space for `hdc | 1`: one per DC, associated with it,
/// made when first used. 0 if `hdc` is not a DC.
pub fn dc_ps(hdc: u32) -> u32 {
    if dc_get(hdc).is_none() {
        return 0;
    }
    if let Some(&(_, hps)) = objects().dc_pss.iter().find(|(d, _)| *d == hdc) {
        return hps;
    }
    // GPIA_ASSOC | PU_PELS.
    let hps = crate::gpiapi::GpiCreatePS(1, hdc, core::ptr::null(), 0x4008);
    if hps != 0 {
        objects().dc_pss.push((hdc, hps));
    }
    hps
}
/// The screen device of windows' cached presentation spaces
/// (GpiQueryDevice of a WinGetPS PS): DevQueryCaps answers for it.
pub const SCREEN_DC: u32 = 0x3000_0000;
const HPAL_BASE: u32 = 0x4000_0001;

fn add<T>(v: &mut Vec<Option<T>>, x: T, base: u32) -> u32 {
    match v.iter().position(|p| p.is_none()) {
        Some(i) => {
            v[i] = Some(x);
            base + i as u32
        }
        None => {
            v.push(Some(x));
            base + v.len() as u32 - 1
        }
    }
}

fn get<T>(v: &mut [Option<T>], h: u32, base: u32) -> Option<&mut T> {
    v.get_mut(h.wrapping_sub(base) as usize)?.as_mut()
}

fn remove<T>(v: &mut [Option<T>], h: u32, base: u32) -> Option<T> {
    v.get_mut(h.wrapping_sub(base) as usize)?.take()
}

fn region(h: u32) -> Option<&'static mut Region> {
    get(&mut objects().regions, h, HRGN_BASE)
}

fn bm(h: u32) -> Option<&'static mut Bm> {
    get(&mut objects().bitmaps, h, HBM_BASE)
}

pub fn bitmap_surface(h: u32) -> Option<Surface> {
    bm(h).map(|b| b.bm.surface())
}

pub fn bitmap_size(h: u32) -> Option<(i32, i32)> {
    bm(h).map(|b| (b.bm.width, b.bm.height))
}

pub fn with_bitmap<R>(h: u32, f: impl FnOnce(&Bitmap) -> R) -> Option<R> {
    bm(h).map(|b| f(&b.bm))
}

pub fn bitmap_selected(h: u32, on: bool) {
    if let Some(b) = bm(h) {
        b.selected = on;
    }
}

pub fn new_bitmap(b: Bitmap) -> u32 {
    add(&mut objects().bitmaps, Bm { bm: b, selected: false }, HBM_BASE)
}

/// Take a bitmap of the calling process out of its objects (it goes to
/// the clipboard); a bitmap selected into a PS stays.
pub fn take_bitmap(h: u32) -> Option<Bitmap> {
    if bm(h)?.selected {
        return None;
    }
    remove(&mut objects().bitmaps, h, HBM_BASE).map(|b| b.bm)
}

/// Delete a bitmap of process `p` (unless selected).
pub fn delete_bitmap_of(p: u32, h: u32) {
    if let Some(o) = st().objects.get_mut(&p) {
        if get(&mut o.bitmaps, h, HBM_BASE).is_some_and(|b| !b.selected) {
            remove(&mut o.bitmaps, h, HBM_BASE);
        }
    }
}

/// Associate a PS with a device context.
pub fn associate(hps: u32, hdc: u32) -> bool {
    let kind = match dc_get(hdc) {
        Some(DcKind::Window(h)) => DcKind::Window(*h),
        Some(DcKind::Memory) => DcKind::Memory,
        Some(DcKind::Info) => DcKind::Info,
        Some(DcKind::Metafile(_)) => {
            let Some(e) = entry(st(), hps) else { return false };
            e.gpi.hdc = hdc;
            e.hwnd = 0;
            e.gpi.memory = false;
            e.ps.set_vis(Point::new(0, 0), Vec::new());
            crate::gpiseg::set_meta_dc(hps, hdc);
            return true;
        }
        None => return false,
    };
    let Some(e) = entry(st(), hps) else { return false };
    e.gpi.hdc = hdc;
    e.gpi.seg.meta_dc = 0;
    match kind {
        DcKind::Window(h) => {
            e.hwnd = h;
            e.gpi.memory = false;
            // The visible region is filled in before each drawing call.
            e.ps.set_vis(Point::new(0, 0), Vec::new());
        }
        DcKind::Memory => {
            e.hwnd = 0;
            e.gpi.memory = true;
            e.ps.set_vis(Point::new(0, 0), Vec::new());
        }
        DcKind::Info | DcKind::Metafile(_) => {
            e.hwnd = 0;
            e.gpi.memory = false;
        }
    }
    true
}

fn rects(p: *const RectL, n: i32) -> Option<Vec<Rect>> {
    if n < 0 || (n > 0 && p.is_null()) {
        return None;
    }
    Some((0..n as usize).filter_map(|i| read(unsafe { p.add(i) })).map(|r| r.rect()).collect())
}

// ---- Regions ----

const RGN_ERROR: i32 = 0;

/// GpiCreateRegion(hps, lCount, arclRectangles): device coordinates.
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreateRegion(_hps: u32, n: i32, p: *const RectL) -> u32 {
    let Some(r) = rects(p, n) else { return 0 };
    add(&mut objects().regions, Region::from_rects(&r), HRGN_BASE)
}

/// GpiCreateEllipticRegion(hps, prclRect)
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreateEllipticRegion(_hps: u32, p: *const RectL) -> u32 {
    let Some(r) = read(p).map(|r| r.rect()) else { return 0 };
    let c = ((r.left + r.right) as f64 / 2.0, (r.bottom + r.top) as f64 / 2.0);
    let mut pts = Vec::new();
    gpi::raster::arc_points(c, (r.width() as f64 / 2.0, 0.0), (0.0, r.height() as f64 / 2.0), 0.0, 2.0 * gpi::math::PI, &mut pts);
    add(&mut objects().regions, gpi::raster::fill_region(&[pts], false, false), HRGN_BASE)
}

/// GpiCreateRoundRectRegion(hps, pptlPt[2], lHround, lVround)
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreateRoundRectRegion(hps: u32, p: *const PointL, h: i32, v: i32) -> u32 {
    let Some(pts) = points(p, 2) else { return 0 };
    // Rasterise the box on a scratch PS with identity transforms.
    let mut ps = gpi::Ps::new(Point::new(0, 0), Vec::new());
    let _ = hps;
    ps.begin_path();
    ps.gpi_move(pts[0]);
    let mut null = gpiapi::null_surface();
    ps.gpi_box(&mut null, 2, Point::new(pts[1].x - 1, pts[1].y - 1), h, v);
    ps.end_path();
    let g = ps.path_region(false, true).unwrap_or_default();
    add(&mut objects().regions, g, HRGN_BASE)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDestroyRegion(_hps: u32, h: u32) -> u32 {
    remove(&mut objects().regions, h, HRGN_BASE).is_some() as u32
}

/// GpiSetRegion(hps, hrgn, lcount, arclRectangles)
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetRegion(_hps: u32, h: u32, n: i32, p: *const RectL) -> u32 {
    let (Some(g), Some(r)) = (region(h), rects(p, n)) else { return 0 };
    *g = Region::from_rects(&r);
    1
}

/// GpiCombineRegion(hps, hrgnDest, hrgnSrc1, hrgnSrc2, lMode): CRGN_OR 1,
/// COPY 2, XOR 4, AND 6, DIFF 7.
#[unsafe(no_mangle)]
pub extern "C" fn GpiCombineRegion(_hps: u32, dst: u32, a: u32, b: u32, mode: i32) -> i32 {
    let Some(ra) = region(a).cloned() else { return RGN_ERROR };
    let out = if mode == 2 {
        ra
    } else {
        let Some(rb) = region(b).cloned() else { return RGN_ERROR };
        let op = match mode {
            1 => Op::Or,
            4 => Op::Xor,
            6 => Op::And,
            7 => Op::Diff,
            _ => return RGN_ERROR,
        };
        ra.combine(&rb, op)
    };
    let Some(d) = region(dst) else { return RGN_ERROR };
    *d = out;
    d.complexity()
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiEqualRegion(_hps: u32, a: u32, b: u32) -> i32 {
    match (region(a).cloned(), region(b)) {
        (Some(x), Some(y)) => 1 + (x == *y) as i32,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiOffsetRegion(_hps: u32, h: u32, p: *const PointL) -> u32 {
    let (Some(g), Some(p)) = (region(h), point(p)) else { return 0 };
    g.offset(p.x, p.y);
    1
}

/// GpiPaintRegion(hps, hrgn): fill with the area attributes.
#[unsafe(no_mangle)]
pub extern "C" fn GpiPaintRegion(hps: u32, h: u32) -> i32 {
    let Some(g) = region(h).cloned() else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.paint_region(s, &g)).map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiFrameRegion(hps, hrgn, thickness)
#[unsafe(no_mangle)]
pub extern "C" fn GpiFrameRegion(hps: u32, h: u32, t: *const [i32; 2]) -> i32 {
    let (Some(g), Some(t)) = (region(h).cloned(), read(t)) else { return GPI_ERROR };
    draw(hps, |e, s| e.ps.frame_region(s, &g, (t[0], t[1]))).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryRegionBox(_hps: u32, h: u32, p: *mut RectL) -> i32 {
    let Some(g) = region(h) else { return RGN_ERROR };
    write(p, RectL::from(g.bounds()));
    g.complexity()
}

/// RGNRECT: ircStart (1-based), crc, crcReturned, ulDirection.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryRegionRects(_hps: u32, h: u32, bound: *const RectL, ctl: *mut u32, out: *mut RectL) -> u32 {
    let Some(g) = region(h) else { return 0 };
    if ctl.is_null() {
        return 0;
    }
    let mut v = g.rects();
    if let Some(b) = read(bound) {
        v = v.iter().map(|r| r.intersect(&b.rect())).filter(|r| !r.is_empty()).collect();
    }
    let (start, crc, dir) = unsafe { (ctl.read_unaligned(), ctl.add(1).read_unaligned(), ctl.add(3).read_unaligned()) };
    // Bands come bottom-up, left to right.
    if dir == 1 || dir == 2 {
        v.sort_by_key(|r| (-r.top, r.left));
    }
    if dir == 2 || dir == 4 {
        v.sort_by_key(|r| (if dir == 2 { -r.top } else { r.bottom }, -r.left));
    }
    let from = start.max(1) as usize - 1;
    let mut n = 0u32;
    if crc == 0 || out.is_null() {
        n = v.len().saturating_sub(from) as u32;
    } else {
        for (i, r) in v.iter().skip(from).take(crc as usize).enumerate() {
            write(unsafe { out.add(i) }, RectL::from(*r));
            n += 1;
        }
    }
    unsafe { ctl.add(2).write_unaligned(n) };
    1
}

/// GpiPtInRegion: PRGN_OUTSIDE 1, PRGN_INSIDE 2.
#[unsafe(no_mangle)]
pub extern "C" fn GpiPtInRegion(_hps: u32, h: u32, p: *const PointL) -> i32 {
    let (Some(g), Some(p)) = (region(h), point(p)) else { return 0 };
    if g.contains(p) { 2 } else { 1 }
}

/// GpiRectInRegion: RRGN_OUTSIDE 1, PARTIAL 2, INSIDE 3.
#[unsafe(no_mangle)]
pub extern "C" fn GpiRectInRegion(_hps: u32, h: u32, p: *const RectL) -> i32 {
    let (Some(g), Some(r)) = (region(h), read(p)) else { return 0 };
    1 + g.rect_inside(r.rect())
}

/// GpiSetClipRegion(hps, hrgn, phrgnOld): the region (device
/// coordinates) clips all drawing; 0 removes it.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetClipRegion(hps: u32, h: u32, old: *mut u32) -> i32 {
    let g = if h == 0 {
        None
    } else {
        match region(h) {
            Some(g) => Some(g.clone()),
            None => return RGN_ERROR,
        }
    };
    let Some(e) = entry(st(), hps) else { return RGN_ERROR };
    write(old, e.gpi.clip_hrgn);
    e.gpi.clip_hrgn = h;
    let c = g.as_ref().map_or(gpi::region::RGN_RECT, |g| g.complexity());
    e.ps.set_clip_region(g);
    c
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryClipRegion(hps: u32) -> u32 {
    entry(st(), hps).map_or(0, |e| e.gpi.clip_hrgn)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiOffsetClipRegion(hps: u32, p: *const PointL) -> i32 {
    let (Some(e), Some(p)) = (entry(st(), hps), point(p)) else { return RGN_ERROR };
    let Some(mut g) = e.ps.clip_region.clone() else { return gpi::region::RGN_RECT };
    g.offset(p.x, p.y);
    let c = g.complexity();
    e.ps.set_clip_region(Some(g));
    c
}

/// GpiQueryClipBox(hps, prclBound): bounds of the clip, world
/// coordinates.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryClipBox(hps: u32, p: *mut RectL) -> i32 {
    gpiapi::refresh(st(), hps);
    let Some(e) = entry(st(), hps) else { return RGN_ERROR };
    let g = if e.gpi.memory || e.hwnd != 0 || !e.ps.clip.is_empty() {
        e.ps.clip_device()
    } else {
        // Not associated: the clip region alone (or infinite).
        e.ps.clip_region.clone().unwrap_or_else(|| Region::from_rect(Rect::new(-0x7fff_fff, -0x7fff_fff, 0x7fff_fff, 0x7fff_fff)))
    };
    let b = g.bounds();
    let mut pts = [Point::new(b.left, b.bottom), Point::new(b.right - 1, b.top - 1)];
    e.ps.convert(gpi::CVTC_DEVICE, gpi::CVTC_WORLD, &mut pts);
    write(p, RectL { left: pts[0].x, bottom: pts[0].y, right: pts[1].x, top: pts[1].y });
    g.complexity()
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiIntersectClipRectangle(hps: u32, p: *const RectL) -> i32 {
    if let Some(r) = read(p) {
        crate::gpiseg::rec_meta(hps, gpi::goca::ESC_INTERSECTCLIPRECT, || [r.left, r.bottom, r.right, r.top].iter().flat_map(|v| v.to_le_bytes()).collect());
    }
    let (Some(e), Some(r)) = (entry(st(), hps), read(p)) else { return RGN_ERROR };
    e.ps.clip_rect(r.rect(), false);
    e.ps.clip_region.as_ref().map_or(gpi::region::RGN_NULL, |g| g.complexity())
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiExcludeClipRectangle(hps: u32, p: *const RectL) -> i32 {
    if let Some(r) = read(p) {
        crate::gpiseg::rec_meta(hps, gpi::goca::ESC_EXCLUDECLIPRECT, || [r.left, r.bottom, r.right, r.top].iter().flat_map(|v| v.to_le_bytes()).collect());
    }
    let (Some(e), Some(r)) = (entry(st(), hps), read(p)) else { return RGN_ERROR };
    e.ps.clip_rect(r.rect(), true);
    e.ps.clip_region.as_ref().map_or(gpi::region::RGN_NULL, |g| g.complexity())
}

/// GpiPtVisible: PVIS_INVISIBLE 1, PVIS_VISIBLE 2.
#[unsafe(no_mangle)]
pub extern "C" fn GpiPtVisible(hps: u32, p: *const PointL) -> i32 {
    let Some(p) = point(p) else { return 0 };
    draw(hps, |e, _| {
        let mut d = [p];
        e.ps.convert(gpi::CVTC_WORLD, gpi::CVTC_DEVICE, &mut d);
        if e.ps.clip_device().contains(d[0]) { 2 } else { 1 }
    })
    .unwrap_or(0)
}

/// GpiRectVisible: RVIS_INVISIBLE 1, PARTIAL 2, VISIBLE 3.
#[unsafe(no_mangle)]
pub extern "C" fn GpiRectVisible(hps: u32, p: *const RectL) -> i32 {
    let Some(r) = read(p) else { return 0 };
    draw(hps, |e, _| {
        let d = e.ps.world_rect_to_device(r.rect());
        1 + e.ps.clip_device().rect_inside(Rect::new(d.left, d.bottom, d.right + 1, d.top + 1))
    })
    .unwrap_or(0)
}

/// GpiPathToRegion(hps, lPath, lOptions): FPATH_WINDING 2.
#[unsafe(no_mangle)]
pub extern "C" fn GpiPathToRegion(hps: u32, id: i32, opts: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if id != 1 {
        return 0;
    }
    let Some(g) = e.ps.path_region(opts & 2 != 0, opts & 8 == 0) else { return 0 };
    add(&mut objects().regions, g, HRGN_BASE)
}

// ---- Window regions (PMWIN) ----

/// Invalidate a window by a region (window coordinates); 0 = all.
pub fn invalidate_region(h: u32, hrgn: u32, children: bool) -> bool {
    let s = st();
    let h = if h == 0 { pmwin::HWND_DESKTOP } else { h };
    if !s.desk.exists(h) {
        return false;
    }
    if hrgn == 0 {
        s.desk.invalidate(h, None, children);
        return true;
    }
    let Some(g) = region(hrgn) else { return false };
    for r in g.rects() {
        st().desk.invalidate(h, Some(r), children);
    }
    true
}

/// WinQueryUpdateRegion(hwnd, hrgn): the window's update region.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryUpdateRegion(h: u32, hrgn: u32) -> i32 {
    let Some(w) = st().desk.get(h) else { return RGN_ERROR };
    let g = Region::from_rects(&w.update.rects);
    let Some(d) = region(hrgn) else { return RGN_ERROR };
    *d = g;
    d.complexity()
}

/// WinValidateRegion(hwnd, hrgn, fIncludeClippedChildren)
#[unsafe(no_mangle)]
pub extern "C" fn WinValidateRegion(h: u32, hrgn: u32, _children: u32) -> u32 {
    let Some(g) = region(hrgn).cloned() else { return 0 };
    if !st().desk.exists(h) {
        return 0;
    }
    for r in g.rects() {
        st().desk.validate(h, Some(r));
    }
    1
}

/// WinExcludeUpdateRegion(hps, hwnd): clip the PS to exclude the
/// window's update region.
#[unsafe(no_mangle)]
pub extern "C" fn WinExcludeUpdateRegion(hps: u32, h: u32) -> i32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return RGN_ERROR };
    let upd = Region::from_rects(&w.update.rects);
    let Some(e) = entry(s, hps) else { return RGN_ERROR };
    let base = e.ps.clip_region.take().unwrap_or_else(|| e.ps.clip_device());
    let g = base.combine(&upd, Op::Diff);
    let c = g.complexity();
    e.ps.set_clip_region(Some(g));
    c
}

/// WinScrollWindow(hwnd, dx, dy, prclScroll, prclClip, hrgnUpdate,
/// prclUpdate, rgfsw): move the window's visible pels and its pending
/// update region; the uncovered area goes to hrgnUpdate/prclUpdate and,
/// with SW_INVALIDATERGN (2), is invalidated (SW_SCROLLCHILDREN 1 also
/// invalidates children).
#[unsafe(no_mangle)]
pub extern "C" fn WinScrollWindow(h: u32, dx: i32, dy: i32, scroll: *const RectL, clip: *const RectL, hrgn: u32, upd: *mut RectL, fl: u32) -> i32 {
    let s = st();
    let Some(w) = s.desk.get(h) else { return RGN_ERROR };
    let wr = w.rect();
    let clip_children = w.style & pmwin::ws::CLIPCHILDREN != 0;
    let sr = read(scroll).map_or(wr, |r| r.rect().intersect(&wr));
    let cr = read(clip).map_or(wr, |r| r.rect().intersect(&wr));
    let o = s.desk.origin(h);
    let mut vis = Region::from_rects(&s.desk.visible_region(h, clip_children).rects);
    vis.offset(-o.x, -o.y);
    let area = vis.intersect_rect(sr).intersect_rect(cr);
    let mut moved = vis.intersect_rect(sr);
    moved.offset(dx, dy);
    let copy = area.combine(&moved, Op::And);
    // Read every source pel first: the rectangles may overlap their sources.
    let mut surf = s.surface;
    pointer::hide(s);
    let mut saved: Vec<(Rect, Vec<u32>)> = Vec::new();
    for r in copy.rects() {
        let src = r.offset(o.x - dx, o.y - dy);
        let mut px = Vec::with_capacity((r.width() * r.height()) as usize);
        for y in src.bottom..src.top {
            for x in src.left..src.right {
                px.push(surf.get(x, y).unwrap_or(0));
            }
        }
        saved.push((r, px));
    }
    for (r, px) in saved {
        let d = r.offset(o.x, o.y);
        let mut i = 0;
        for y in d.bottom..d.top {
            for x in d.left..d.right {
                surf.set(x, y, px[i]);
                i += 1;
            }
        }
    }
    pointer::show(st());
    let exposed = area.combine(&copy, Op::Diff);
    // The pending update region moves with the contents.
    if let Some(w) = st().desk.get_mut(h) {
        let old = Region::from_rects(&w.update.rects);
        let mut inside = old.intersect_rect(sr);
        inside.offset(dx, dy);
        let new = old.combine(&Region::from_rect(sr), Op::Diff).combine(&inside.intersect_rect(sr), Op::Or);
        let mut u = pmwin::Region::new();
        for r in new.rects() {
            u.add(r);
        }
        w.update = u;
    }
    if fl & 2 != 0 {
        for r in exposed.rects() {
            st().desk.invalidate(h, Some(r), fl & 1 != 0);
        }
    }
    if let Some(g) = region(hrgn) {
        *g = exposed.clone();
    }
    write(upd, RectL::from(exposed.bounds()));
    exposed.complexity()
}

// ---- Device contexts ----

const OD_SCREEN: i32 = 0;
const OD_METAFILE: i32 = 3;
const OD_INFO: i32 = 6;
const OD_METAFILE_NOQUERY: i32 = 7;
const OD_MEMORY: i32 = 8;

/// DevOpenDC(hab, lType, pszToken, lCount, pdopData, hdcComp): memory,
/// information and metafile DCs; there are no printers yet.
#[unsafe(no_mangle)]
pub extern "C" fn DevOpenDC(_hab: u32, ty: i32, _token: *const u8, _n: i32, _dop: *const u32, _comp: u32) -> u32 {
    let k = match ty {
        OD_MEMORY => DcKind::Memory,
        OD_INFO | OD_SCREEN => DcKind::Info,
        OD_METAFILE | OD_METAFILE_NOQUERY => DcKind::Metafile(alloc::boxed::Box::new(crate::gpiseg::MetaRec::new())),
        _ => return 0,
    };
    dc_add(k)
}

/// DevCloseDC(hdc): DEV_OK 1; for a metafile DC the metafile's handle
/// (presentation spaces still associated with it lose it).
#[unsafe(no_mangle)]
pub extern "C" fn DevCloseDC(hdc: u32) -> u32 {
    let Some(i) = dc_index(hdc) else { return 0 };
    let o = objects();
    if let Some(at) = o.dc_pss.iter().position(|(d, _)| *d == hdc) {
        let (_, hps) = o.dc_pss.remove(at);
        crate::gpiapi::GpiDestroyPS(hps);
    }
    let me = crate::state::pid();
    for e in st().pss.iter_mut().flatten().filter(|e| e.pid == me && e.gpi.hdc == hdc) {
        e.gpi.hdc = 0;
        e.gpi.seg.meta_dc = 0;
    }
    match objects().dcs.get_mut(i).and_then(|d| d.take()) {
        Some(DcKind::Metafile(m)) => crate::gpiseg::new_mf(m.build()),
        Some(_) => 1,
        None => 0,
    }
}

/// WinOpenWindowDC(hwnd): the window's device context (one per window).
#[unsafe(no_mangle)]
pub extern "C" fn WinOpenWindowDC(h: u32) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    let o = objects();
    if let Some(i) = o.dcs.iter().position(|d| matches!(d, Some(DcKind::Window(w)) if *w == h)) {
        return dc_handle(i);
    }
    crate::avio::dc_opened();
    dc_add(DcKind::Window(h))
}

/// The window of a window DC of the caller.
pub fn window_of_dc(hdc: u32) -> Option<u32> {
    match objects().dcs.get(dc_index(hdc)?)? {
        Some(DcKind::Window(w)) => Some(*w),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryWindowDC(h: u32) -> u32 {
    let o = objects();
    o.dcs.iter().position(|d| matches!(d, Some(DcKind::Window(w)) if *w == h)).map_or(0, dc_handle)
}

/// Device capabilities (CAPS_*) of the display.
fn caps(i: i32) -> i32 {
    let s = st();
    let (w, h) = (s.surface.width, s.surface.height);
    match i {
        1 => 4,                                // IO_CAPS: supports I/O
        2 => 2,                                // TECHNOLOGY: raster display
        3 => 0x200,                            // DRIVER_VERSION
        4 => w,
        5 => h,
        6 => w / 8,
        7 => h / 16,
        8 | 9 => gpi::xform::PELS_PER_METRE,
        10 | 12 | 35 => 8,
        11 | 13 | 36 => 16,
        14 | 33 => 1 << 24,                    // COLORS, PHYS_COLORS
        15 => 1,                               // COLOR_PLANES
        16 => 24,                              // COLOR_BITCOUNT
        17 => 1 | 4,                           // COLOR_TABLE_SUPPORT: RGB_8, TRUE_MIX
        18 => 3,                               // MOUSE_BUTTONS
        19 => 1 | 2 | 8 | 16 | 32 | 64,        // FOREGROUND_MIX_SUPPORT
        20 => 1 | 2 | 8 | 16,                  // BACKGROUND_MIX_SUPPORT
        22 => 2,                               // WINDOW_BYTE_ALIGNMENT: not required
        23 => 4,                               // BITMAP_FORMATS
        24 => 1 | 4 | 16 | 32,                 // RASTER_CAPS: bitblt, scaling, set pel, fonts
        25 | 26 => 9,                          // MARKER_HEIGHT/WIDTH
        27 => 1,                               // DEVICE_FONTS
        29 => 0x200,                           // GRAPHICS_VERSION
        31 => 1,                               // DEVICE_WINDOWING
        32 => 256 | 512 | 4096,                // ADDITIONAL_GRAPHICS: palette manager, wide lines, clip fills
        34 => 255,                             // COLOR_INDEX
        37 | 38 => 96,                         // FONT_RES
        40 => 2,                               // LINEWIDTH_THICK
        _ => 0,
    }
}

/// DevQueryCaps(hdc, lStart, lCount, alArray). IBM's GRE also takes a
/// presentation space for the DC (OS/2's Icon Editor passes a WinGetPS
/// one).
#[unsafe(no_mangle)]
pub extern "C" fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32 {
    let known = hdc == SCREEN_DC || dc_get(hdc).is_some() || entry(st(), hdc).is_some();
    if !known || out.is_null() || start < 0 || n < 0 {
        return 0;
    }
    for i in 0..n {
        unsafe { out.add(i as usize).write_unaligned(caps(start + i)) };
    }
    1
}

/// DevEscape: DEVESC_NOTIMPLEMENTED.
#[unsafe(no_mangle)]
pub extern "C" fn DevEscape(_hdc: u32, _code: i32, _nin: i32, _pin: *const u8, _nout: *mut i32, _pout: *mut u8) -> i32 {
    0
}

/// DevPostDeviceModes: no printer drivers (DPDM_ERROR).
#[unsafe(no_mangle)]
pub extern "C" fn DevPostDeviceModes(_hab: u32, _drv: *mut u8, _name: *const u8, _dev: *const u8, _pname: *const u8, _fl: u32) -> i32 {
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn DevQueryDeviceNames(_hab: u32, _drv: *const u8, n: *mut i32, _names: *mut u8, _desc: *mut u8, nt: *mut i32, _types: *mut u8) -> u32 {
    write(n, 0);
    write(nt, 0);
    1
}

/// DevQueryHardcopyCaps: DQHC_ERROR.
#[unsafe(no_mangle)]
pub extern "C" fn DevQueryHardcopyCaps(_hdc: u32, _start: i32, _n: i32, _info: *mut u8) -> i32 {
    -1
}

// ---- Bitmaps ----

/// Read a BITMAPINFOHEADER(2) or BITMAPINFO(2) (with its colour table).
fn read_info(p: *const u8, table: bool) -> Option<Info> {
    if p.is_null() {
        return None;
    }
    let cbfix = unsafe { (p as *const u32).read_unaligned() } as usize;
    if !(12..=64).contains(&cbfix) {
        return None;
    }
    let hdr = unsafe { core::slice::from_raw_parts(p, cbfix) };
    let i = Info::parse(hdr, false)?;
    if !table || i.bitcount > 8 {
        return Some(i);
    }
    let used = if cbfix >= 36 { u32::from_le_bytes([hdr[32], hdr[33], hdr[34], hdr[35]]) } else { 0 };
    let n = if used != 0 && used < 256 { used as usize } else { 1 << i.bitcount };
    let es = if cbfix == 12 { 3 } else { 4 };
    let all = unsafe { core::slice::from_raw_parts(p, cbfix + n * es) };
    Info::parse(all, true)
}

/// Bytes of bit data for `scans` lines (compressed: cbImage).
fn data_len(p: *const u8, i: &Info, scans: i32) -> usize {
    if i.compression != 0 && i.cbfix >= 24 {
        return unsafe { (p.add(20) as *const u32).read_unaligned() } as usize;
    }
    i.stride() * scans.max(0) as usize
}

/// GpiCreateBitmap(hps, pbmpNew, flOptions, pbInitData, pbmiInfoTable):
/// CBM_INIT 4 initialises the bits.
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreateBitmap(_hps: u32, hdr: *const u8, opts: u32, init: *const u8, bmi: *const u8) -> u32 {
    let Some(i) = read_info(hdr, false) else { return 0 };
    if i.width <= 0 || i.height <= 0 {
        return 0;
    }
    let mut b = Bitmap::new(i.width, i.height, i.planes, i.bitcount);
    if opts & 4 != 0 && !init.is_null() {
        if let Some(t) = read_info(bmi, true) {
            let n = data_len(bmi, &t, i.height);
            let data = unsafe { core::slice::from_raw_parts(init, n) };
            bitmap::set_bits(&mut b, &t, 0, i.height, data);
        }
    }
    new_bitmap(b)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDeleteBitmap(h: u32) -> u32 {
    match bm(h) {
        Some(b) if !b.selected => remove(&mut objects().bitmaps, h, HBM_BASE).is_some() as u32,
        _ => 0,
    }
}

/// GpiSetBitmap(hps, hbm): select into a memory PS; returns the previous
/// bitmap (BMB_ERROR -1 on failure).
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetBitmap(hps: u32, h: u32) -> u32 {
    const BMB_ERROR: u32 = u32::MAX;
    let Some(e) = entry(st(), hps) else { return BMB_ERROR };
    if !e.gpi.memory {
        return BMB_ERROR;
    }
    let old = e.gpi.bitmap;
    if h == old {
        return old;
    }
    if h != 0 && !bm(h).is_some_and(|b| !b.selected) {
        return BMB_ERROR;
    }
    if old != 0 {
        bitmap_selected(old, false);
    }
    let e = entry(st(), hps).unwrap();
    e.gpi.bitmap = h;
    match bm(h) {
        Some(b) => {
            b.selected = true;
            e.ps.set_vis(Point::new(0, 0), vec![Rect::new(0, 0, b.bm.width, b.bm.height)]);
        }
        None => e.ps.set_vis(Point::new(0, 0), Vec::new()),
    }
    old
}

/// GpiSetBitmapId(hps, hbm, lLcid): tag a bitmap for use as a pattern.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetBitmapId(hps: u32, h: u32, lcid: i32) -> u32 {
    if bm(h).is_none() || !(1..=254).contains(&lcid) {
        return 0;
    }
    let Some(e) = entry(st(), hps) else { return 0 };
    if e.gpi.lcids.iter().any(|(l, _)| *l == lcid) {
        return 0;
    }
    e.gpi.lcids.push((lcid, gpiapi::Lcid::Bitmap(h)));
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBitmapHandle(hps: u32, lcid: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    e.gpi.lcids.iter().find_map(|(l, x)| match x {
        gpiapi::Lcid::Bitmap(h) if *l == lcid => Some(*h),
        _ => None,
    })
    .unwrap_or(0)
}

/// GpiQueryBitmapParameters(hbm, pbmpData): a BITMAPINFOHEADER.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBitmapParameters(h: u32, p: *mut u8) -> u32 {
    let Some(b) = bm(h) else { return 0 };
    if p.is_null() {
        return 0;
    }
    let mut v = [0u8; 12];
    v[0..4].copy_from_slice(&12u32.to_le_bytes());
    v[4..6].copy_from_slice(&(b.bm.width as u16).to_le_bytes());
    v[6..8].copy_from_slice(&(b.bm.height as u16).to_le_bytes());
    v[8..10].copy_from_slice(&b.bm.planes.to_le_bytes());
    v[10..12].copy_from_slice(&b.bm.bitcount.to_le_bytes());
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), p, 12) };
    1
}

/// GpiQueryBitmapInfoHeader(hbm, pbmpData): fills as much of a
/// BITMAPINFOHEADER2 as its cbFix asks for.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBitmapInfoHeader(h: u32, p: *mut u8) -> u32 {
    let Some(b) = bm(h) else { return 0 };
    if p.is_null() {
        return 0;
    }
    let cbfix = unsafe { (p as *const u32).read_unaligned() } as usize;
    if cbfix == 12 {
        return GpiQueryBitmapParameters(h, p);
    }
    if !(16..=64).contains(&cbfix) {
        return 0;
    }
    let mut v = [0u8; 64];
    v[0..4].copy_from_slice(&(cbfix as u32).to_le_bytes());
    v[4..8].copy_from_slice(&(b.bm.width as u32).to_le_bytes());
    v[8..12].copy_from_slice(&(b.bm.height as u32).to_le_bytes());
    v[12..14].copy_from_slice(&b.bm.planes.to_le_bytes());
    v[14..16].copy_from_slice(&b.bm.bitcount.to_le_bytes());
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), p, cbfix) };
    1
}

/// GpiSetBitmapBits(hps, lScanStart, lScans, pbBuffer, pbmiInfoTable):
/// into the bitmap selected in the PS.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetBitmapBits(hps: u32, start: i32, scans: i32, buf: *const u8, bmi: *const u8) -> i32 {
    let Some(h) = entry(st(), hps).map(|e| e.gpi.bitmap).filter(|&h| h != 0) else { return -1 };
    let (Some(b), Some(i)) = (bm(h), read_info(bmi, true)) else { return -1 };
    if buf.is_null() {
        return -1;
    }
    let data = unsafe { core::slice::from_raw_parts(buf, data_len(bmi, &i, scans)) };
    bitmap::set_bits(&mut b.bm, &i, start, scans, data)
}

/// GpiQueryBitmapBits(hps, lScanStart, lScans, pbBuffer, pbmiInfoTable):
/// in the format the header asks for; the colour table is filled in.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryBitmapBits(hps: u32, start: i32, scans: i32, buf: *mut u8, bmi: *mut u8) -> i32 {
    let Some(h) = entry(st(), hps).map(|e| e.gpi.bitmap).filter(|&h| h != 0) else { return -1 };
    let (Some(b), Some(mut i)) = (bm(h), read_info(bmi, false)) else { return -1 };
    i.width = b.bm.width;
    i.height = b.bm.height;
    i.colors = bitmap::standard_table(i.bitcount);
    let es = if i.cbfix == 12 { 3 } else { 4 };
    for (k, c) in i.colors.iter().enumerate() {
        let e = [*c as u8, (*c >> 8) as u8, (*c >> 16) as u8, 0];
        unsafe { core::ptr::copy_nonoverlapping(e.as_ptr(), bmi.add(i.cbfix as usize + k * es), es) };
    }
    if buf.is_null() {
        return scans.min(b.bm.height - start).max(0);
    }
    let out = unsafe { core::slice::from_raw_parts_mut(buf, i.stride() * scans.max(0) as usize) };
    bitmap::get_bits(&b.bm, &i, start, scans, out)
}

/// GpiQueryDeviceBitmapFormats(hps, lCount, alArray): (planes, bitcount)
/// pairs, preferred first.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDeviceBitmapFormats(_hps: u32, n: i32, out: *mut i32) -> u32 {
    let f = [1, 24, 1, 1, 1, 4, 1, 8];
    if out.is_null() {
        return 0;
    }
    for (i, v) in f.iter().take(n.max(0) as usize).enumerate() {
        unsafe { out.add(i).write_unaligned(*v) };
    }
    1
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.352"]
    fn DosGetResource(hmod: u32, ty: u32, id: u32, ppb: *mut *const u8) -> u32;
    #[link_name = "DOSCALLS.353"]
    fn DosFreeResource(pb: *const u8) -> u32;
    #[link_name = "DOSCALLS.572"]
    fn DosQueryResourceSize(hmod: u32, ty: u32, id: u32, size: *mut u32) -> u32;
}

/// Nearest-neighbour copy of `b` at a new size.
fn stretch(b: &mut Bitmap, w: i32, h: i32) -> Bitmap {
    let mut out = Bitmap::new(w, h, b.planes, b.bitcount);
    let src = b.surface();
    let v = bitmap::sample(&src, Rect::new(0, 0, b.width, b.height), w, h);
    let mut s = out.surface();
    for j in 0..h {
        for i in 0..w {
            s.set(i, j, v[(j * w + i) as usize].unwrap_or(0));
        }
    }
    out
}

/// GpiLoadBitmap(hps, hmod, idBitmap, lWidth, lHeight): from an RT_BITMAP
/// resource, stretched when a size is given.
#[unsafe(no_mangle)]
pub extern "C" fn GpiLoadBitmap(_hps: u32, hmod: u32, id: u32, w: i32, h: i32) -> u32 {
    let mut size = 0u32;
    let mut p: *const u8 = core::ptr::null();
    if unsafe { DosQueryResourceSize(hmod, 2, id, &mut size) } != 0 || unsafe { DosGetResource(hmod, 2, id, &mut p) } != 0 || p.is_null() {
        return 0;
    }
    let data = unsafe { core::slice::from_raw_parts(p, size as usize) };
    let b = bitmap::load(data);
    unsafe { DosFreeResource(p) };
    let Some(mut b) = b else { return 0 };
    if w > 0 && h > 0 && (w != b.width || h != b.height) {
        b = stretch(&mut b, w, h);
    }
    new_bitmap(b)
}

/// Source pels of a PS for a blit (device rect `r`, `w`x`h` samples),
/// whether the source is monochrome, and its image background colour.
fn source_pels(hps: u32, r: Rect, w: i32, h: i32) -> Option<(Vec<Option<u32>>, bool, u32)> {
    let s = st();
    gpiapi::refresh(s, hps);
    let e = entry(s, hps)?;
    let back = e.ps.colors.rgb(e.ps.attrs.image.back);
    let o = e.ps.origin;
    match gpiapi::target(e) {
        Target::Screen => {
            let surf = s.surface;
            pointer::hide(s);
            let v = bitmap::sample(&surf, r.offset(o.x, o.y), w, h);
            pointer::show(s);
            Some((v, false, back))
        }
        Target::Bitmap(b) => {
            let b = bm(b)?;
            let surf = b.bm.surface();
            Some((bitmap::sample(&surf, r, w, h), b.bm.is_mono(), back))
        }
        Target::Nowhere => None,
    }
}

fn target_is_mono(hps: u32) -> bool {
    entry(st(), hps).is_some_and(|e| e.gpi.memory && bm(e.gpi.bitmap).is_some_and(|b| b.bm.is_mono()))
}

/// Blit samples to a target device rectangle.
fn blit_to(hps: u32, t: Rect, src: Option<(Vec<Option<u32>>, bool, u32)>, rop: i32) -> i32 {
    let rop = rop as u8;
    let mono_dst = target_is_mono(hps);
    let (w, h) = (t.width(), t.height());
    draw(hps, |e, s| match &src {
        Some((v, mono, back)) => {
            let md = (mono_dst && !mono).then_some(*back);
            e.ps.blit(s, t, Some((v, w, h)), rop, *mono && !mono_dst, md)
        }
        None => e.ps.blit(s, t, None, rop, false, None),
    })
    .map_or(GPI_ERROR, |_| GPI_OK)
}

fn norm(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

/// GpiBitBlt(hpsTarget, hpsSource, lCount, aptlPoints, lRop, flOptions):
/// target rectangle and source corner(s) in device coordinates,
/// non-inclusive at the top right; four points stretch.
#[unsafe(no_mangle)]
pub extern "C" fn GpiBitBlt(dst: u32, src: u32, n: i32, p: *const PointL, rop: i32, _opts: u32) -> i32 {
    let Some(pts) = points(p, n) else { return GPI_ERROR };
    if pts.len() < 2 {
        return GPI_ERROR;
    }
    let t = norm(pts[0], pts[1]);
    let uses_src = bitmap::rop_uses_source(rop as u8);
    let s = if uses_src {
        if pts.len() < 3 {
            return GPI_ERROR;
        }
        let sr = if pts.len() >= 4 { norm(pts[2], pts[3]) } else { Rect::new(pts[2].x, pts[2].y, pts[2].x + t.width(), pts[2].y + t.height()) };
        match source_pels(src, sr, t.width(), t.height()) {
            Some(v) => Some(v),
            None => return GPI_ERROR,
        }
    } else {
        None
    };
    blit_to(dst, t, s, rop)
}

/// World target rectangle (inclusive corners) to device.
fn world_target(hps: u32, a: Point, b: Point) -> Option<Rect> {
    let s = st();
    gpiapi::refresh(s, hps);
    let e = entry(s, hps)?;
    let d = e.ps.world_rect_to_device(norm(a, b));
    Some(Rect::new(d.left, d.bottom, d.right + 1, d.top + 1))
}

/// GpiWCBitBlt(hpsTarget, hbmSource, lCount, aptlPoints, lRop,
/// flOptions): target in world coordinates (inclusive), source in bitmap
/// pels.
#[unsafe(no_mangle)]
pub extern "C" fn GpiWCBitBlt(dst: u32, hbm: u32, n: i32, p: *const PointL, rop: i32, _opts: u32) -> i32 {
    let Some(pts) = points(p, n) else { return GPI_ERROR };
    if pts.len() < 2 {
        return GPI_ERROR;
    }
    let Some(t) = world_target(dst, pts[0], pts[1]) else { return GPI_ERROR };
    let src = if bitmap::rop_uses_source(rop as u8) {
        let Some(b) = bm(hbm) else { return GPI_ERROR };
        let sr = match pts.len() {
            n if n >= 4 => norm(pts[2], pts[3]),
            3 => Rect::new(pts[2].x, pts[2].y, pts[2].x + t.width(), pts[2].y + t.height()),
            _ => Rect::new(0, 0, b.bm.width, b.bm.height),
        };
        let surf = b.bm.surface();
        Some((bitmap::sample(&surf, sr, t.width(), t.height()), b.bm.is_mono(), 0xffffff))
    } else {
        None
    };
    blit_to(dst, t, src, rop)
}

/// GpiDrawBits(hps, pBits, pbmiInfoTable, lCount, aptlPoints, lRop,
/// flOptions): bits in application memory to a world rectangle.
#[unsafe(no_mangle)]
pub extern "C" fn GpiDrawBits(dst: u32, bits: *const u8, bmi: *const u8, n: i32, p: *const PointL, rop: i32, _opts: u32) -> i32 {
    let Some(pts) = points(p, n) else { return GPI_ERROR };
    let Some(i) = read_info(bmi, true) else { return GPI_ERROR };
    if pts.len() < 2 || bits.is_null() || i.width <= 0 || i.height <= 0 {
        return GPI_ERROR;
    }
    let Some(t) = world_target(dst, pts[0], pts[1]) else { return GPI_ERROR };
    let mut b = Bitmap::new(i.width, i.height, 1, 24);
    let data = unsafe { core::slice::from_raw_parts(bits, data_len(bmi, &i, i.height)) };
    bitmap::set_bits(&mut b, &i, 0, i.height, data);
    let sr = match pts.len() {
        n if n >= 4 => norm(pts[2], pts[3]),
        3 => Rect::new(pts[2].x, pts[2].y, pts[2].x + t.width(), pts[2].y + t.height()),
        _ => Rect::new(0, 0, i.width, i.height),
    };
    let surf = b.surface();
    let v = bitmap::sample(&surf, sr, t.width(), t.height());
    blit_to(dst, t, Some((v, false, 0xffffff)), rop)
}

/// WinDrawBitmap(hpsDst, hbm, pwrcSrc, pptlDst, clrFore, clrBack, fl):
/// DBM_INVERT 1, DBM_STRETCH 4 (pptlDst is a RECTL), DBM_IMAGEATTRS 8
/// (use the PS's image colours for monochrome bitmaps).
#[unsafe(no_mangle)]
pub extern "C" fn WinDrawBitmap(hps: u32, hbm: u32, src: *const RectL, dst: *const PointL, fore: i32, back: i32, fl: u32) -> u32 {
    let Some(b) = bm(hbm) else { return 0 };
    let sr = read(src).map_or(Rect::new(0, 0, b.bm.width, b.bm.height), |r| r.rect());
    let t = if fl & 4 != 0 {
        match read(dst as *const RectL) {
            Some(r) => r.rect(),
            None => return 0,
        }
    } else {
        let Some(p) = point(dst) else { return 0 };
        Rect::new(p.x, p.y, p.x + sr.width(), p.y + sr.height())
    };
    let surf = b.bm.surface();
    let v = bitmap::sample(&surf, sr, t.width(), t.height());
    let mono = b.bm.is_mono();
    let rop = if fl & 1 != 0 { 0x33 } else { 0xcc };
    let (w, h) = (t.width(), t.height());
    draw(hps, |e, s| {
        let saved = e.ps.attrs.image;
        if fl & 8 == 0 {
            e.ps.attrs.image.color = fore;
            e.ps.attrs.image.back = back;
        }
        e.ps.blit(s, t, Some((&v, w, h)), rop, mono, None);
        e.ps.attrs.image = saved;
    })
    .is_some() as u32
}

// ---- Colour tables and palettes ----

/// GpiCreateLogColorTable(hps, flOptions, lFormat, lStart, lCount,
/// alTable)
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreateLogColorTable(hps: u32, opts: u32, format: i32, start: i32, n: i32, table: *const i32) -> u32 {
    let data: Vec<i32> = if table.is_null() || n <= 0 { Vec::new() } else { (0..n as usize).map(|i| unsafe { table.add(i).read_unaligned() }).collect() };
    crate::gpiseg::rec_meta(hps, gpi::goca::ESC_CREATELOGCOLORTABLE, || [opts as i32, format, start, data.len() as i32].iter().chain(data.iter()).flat_map(|v| v.to_le_bytes()).collect());
    let Some(e) = entry(st(), hps) else { return 0 };
    if !e.ps.colors.create(opts, format, start, &data) {
        return 0;
    }
    e.rgb_mode = e.ps.colors.rgb_mode;
    e.ps.sync_simple();
    1
}

/// GpiQueryLogColorTable(hps, flOptions, lStart, lCount, alArray):
/// LCOLOPT_INDEX 2 returns (index, RGB) pairs; QLCT_RGB in RGB mode.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryLogColorTable(hps: u32, opts: u32, start: i32, n: i32, out: *mut i32) -> i32 {
    let Some(e) = entry(st(), hps) else { return -1 };
    if e.ps.colors.rgb_mode {
        return -2;
    }
    if out.is_null() || start < 0 {
        return -1;
    }
    let t = e.ps.colors.table();
    let mut k = 0usize;
    for i in start as usize..t.len() {
        let pair = opts & 2 != 0;
        if k + if pair { 2 } else { 1 } > n.max(0) as usize {
            break;
        }
        unsafe {
            if pair {
                out.add(k).write_unaligned(i as i32);
                k += 1;
            }
            out.add(k).write_unaligned(t[i] as i32);
        }
        k += 1;
    }
    k as i32
}

/// GpiQueryColorData(hps, lCount, alArray): format, lowest and highest
/// index.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryColorData(hps: u32, n: i32, out: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let c = &e.ps.colors;
    let v = [if c.rgb_mode { 3 } else { 1 }, 0, if c.rgb_mode { 0xff_ffff } else { c.table().len() as i32 - 1 }];
    for (i, x) in v.iter().take(n.max(0) as usize).enumerate() {
        write(unsafe { out.add(i) }, *x);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryRGBColor(hps: u32, _opts: u32, c: i32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.ps.colors.rgb(c) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryColorIndex(hps: u32, _opts: u32, rgb: i32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.ps.colors.index_of(rgb as u32))
}

/// True colour: every RGB value is available as is.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryNearestColor(hps: u32, _opts: u32, rgb: i32) -> i32 {
    entry(st(), hps).map_or(-1, |_| rgb & 0xff_ffff)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryRealColors(hps: u32, _opts: u32, start: i32, n: i32, out: *mut i32) -> i32 {
    let Some(e) = entry(st(), hps) else { return -1 };
    let t = e.ps.colors.table().to_vec();
    let mut k = 0;
    for c in t.iter().skip(start.max(0) as usize).take(n.max(0) as usize) {
        write(unsafe { out.add(k) }, *c as i32);
        k += 1;
    }
    k as i32
}

fn table(p: *const u32, n: u32) -> Vec<u32> {
    if p.is_null() {
        return Vec::new();
    }
    (0..n as usize).map(|i| unsafe { p.add(i).read_unaligned() }).collect()
}

/// GpiCreatePalette(hab, flOptions, ulFormat, ulCount, aulTable):
/// LCOLF_CONSECRGB entries (flags in the top byte).
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreatePalette(_hab: u32, _opts: u32, fmt: u32, n: u32, t: *const u32) -> u32 {
    if fmt != 2 || n == 0 || n > 4096 {
        return 0;
    }
    add(&mut objects().palettes, table(t, n), HPAL_BASE)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDeletePalette(h: u32) -> u32 {
    remove(&mut objects().palettes, h, HPAL_BASE).is_some() as u32
}

fn rgbs(p: &[u32]) -> Vec<u32> {
    p.iter().map(|c| c & 0xff_ffff).collect()
}

/// GpiSelectPalette(hps, hpal): returns the previous palette, PAL_ERROR
/// (-1) on failure; 0 deselects.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSelectPalette(hps: u32, h: u32) -> u32 {
    let pal = if h == 0 {
        None
    } else {
        match get(&mut objects().palettes, h, HPAL_BASE) {
            Some(p) => Some(rgbs(p)),
            None => return u32::MAX,
        }
    };
    let Some(e) = entry(st(), hps) else { return u32::MAX };
    let old = e.gpi.palette;
    e.gpi.palette = h;
    e.ps.colors.palette = pal;
    e.ps.sync_simple();
    old
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPalette(hps: u32) -> u32 {
    entry(st(), hps).map_or(u32::MAX, |e| e.gpi.palette)
}

/// Recolour what was drawn with a palette whose entries changed: on a
/// true-colour display, pels of each old entry colour become the new one.
fn recolour(hpal: u32, map: &[(u32, u32)]) {
    if map.is_empty() {
        return;
    }
    let s = st();
    let mut done: Vec<(u32, u32)> = Vec::new();
    for i in 0..s.pss.len() {
        let hps = 0x100 + i as u32;
        let Some(e) = s.pss[i].as_mut() else { continue };
        if e.gpi.palette != hpal {
            continue;
        }
        let new = get(&mut objects().palettes, hpal, HPAL_BASE).map(|p| rgbs(p));
        e.ps.colors.palette = new;
        let key = if e.gpi.memory { (1, e.gpi.bitmap) } else { (0, e.hwnd) };
        if done.contains(&key) {
            continue;
        }
        done.push(key);
        let f = |_: i32, _: i32, c: u32| match map.binary_search_by_key(&c, |m| m.0) {
            Ok(k) => map[k].1,
            Err(_) => c,
        };
        let _ = draw(hps, |e, surf| {
            let o = e.ps.origin;
            let vis: Vec<Rect> = if e.gpi.memory { vec![surf.bounds()] } else { e.ps.vis().to_vec() };
            let _ = o;
            for r in vis {
                surf.map(r, f);
            }
        });
    }
}

fn update_palette(h: u32, start: u32, t: &[u32], animate: bool) -> Option<i32> {
    let p = get(&mut objects().palettes, h, HPAL_BASE)?;
    let mut map: Vec<(u32, u32)> = Vec::new();
    let mut n = 0;
    for (k, &v) in t.iter().enumerate() {
        let Some(slot) = p.get_mut(start as usize + k) else { break };
        let (old, new) = (*slot & 0xff_ffff, v & 0xff_ffff);
        if old != new {
            map.push((old, new));
        }
        *slot = v;
        n += 1;
    }
    map.sort_unstable();
    map.dedup_by_key(|m| m.0);
    if animate {
        recolour(h, &map);
    } else {
        for e in st().pss.iter_mut().flatten() {
            if e.gpi.palette == h {
                e.ps.colors.palette = Some(rgbs(p));
            }
        }
    }
    Some(n)
}

/// GpiAnimatePalette(hpal, ulFormat, ulStart, ulCount, aulTable)
#[unsafe(no_mangle)]
pub extern "C" fn GpiAnimatePalette(h: u32, fmt: u32, start: u32, n: u32, t: *const u32) -> i32 {
    if fmt != 2 {
        return -1;
    }
    update_palette(h, start, &table(t, n), true).unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetPaletteEntries(h: u32, fmt: u32, start: u32, n: u32, t: *const u32) -> u32 {
    (fmt == 2 && update_palette(h, start, &table(t, n), false).is_some()) as u32
}

/// GpiQueryPaletteInfo(hpal, hps, flOptions, ulStart, ulCount, aulArray)
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryPaletteInfo(h: u32, _hps: u32, _opts: u32, start: u32, n: u32, out: *mut u32) -> i32 {
    let Some(p) = get(&mut objects().palettes, h, HPAL_BASE) else { return -1 };
    if n == 0 || out.is_null() {
        return p.len() as i32;
    }
    let mut k = 0;
    for v in p.iter().skip(start as usize).take(n as usize) {
        write(unsafe { out.add(k) }, *v);
        k += 1;
    }
    k as i32
}

/// WinRealizePalette(hwnd, hps, pcclr): a true-colour display needs no
/// realisation; no physical entries change.
#[unsafe(no_mangle)]
pub extern "C" fn WinRealizePalette(_h: u32, hps: u32, cclr: *mut u32) -> i32 {
    if entry(st(), hps).is_none() {
        return -1;
    }
    write(cclr, 0);
    0
}
