//! Retained segments, metafile device contexts and metafiles.
//!
//! GPI calls are recorded as GOCA orders (crates/gpi's `goca`): into the
//! open segment of a presentation space in DM_RETAIN or DM_DRAWANDRETAIN
//! mode, and onto the metafile when the PS is associated with a metafile
//! DC (DevOpenDC OD_METAFILE). Calls that are not drawing orders
//! (GpiCreateLogFont, GpiErase, GpiSetCp, ...) go to a metafile as IBM's
//! escape orders. DevCloseDC of a metafile DC builds the metafile
//! (crates/gpi's `metafile`) and returns its handle.
//!
//! Playing (GpiDrawChain, GpiPutData, GpiPlayMetaFile) turns the orders
//! back into the GPI calls, so drawing, recording into another segment or
//! metafile and the boundary data all work as for the calls themselves.
//!
//! A metafile handle stands for the bytes of a .MET file (what
//! GpiQueryMetaFileBits and GpiSetMetaFileBits see). Metafiles are in the
//! shared PM state: one put on the clipboard (CF_METAFILE) belongs to the
//! system and every process can play it.

#![allow(non_snake_case)]

use alloc::vec;
use alloc::vec::Vec;

use gpi::goca::{self, Attr, Order};
use gpi::metafile::{self, Metafile};
use gpi::{Matrix, Point};

use crate::gpiapi::{self, entry, read, write, PointL, RectL, GPI_ERROR, GPI_OK};
use crate::state::st;

pub const DM_DRAW: i32 = 1;
pub const DM_DRAWANDRETAIN: i32 = 3;

const ATTR_DETECTABLE: i32 = 1;
const ATTR_VISIBLE: i32 = 2;
const ATTR_CHAINED: i32 = 6;
const ATTR_DYNAMIC: i32 = 8;
const ATTR_FASTCHAIN: i32 = 9;
const ATTR_PROP_DETECTABLE: i32 = 10;
const ATTR_PROP_VISIBLE: i32 = 11;
const ATTR_ERROR: i32 = -1;

const PMERR_ALREADY_IN_SEG: u32 = 0x2004;
const PMERR_INV_DRAWING_MODE: u32 = 0x2065;
const PMERR_INV_HMF: u32 = 0x207e;
const PMERR_INV_METAFILE: u32 = 0x209d;
const PMERR_INV_METAFILE_LENGTH: u32 = 0x209e;
const PMERR_INV_METAFILE_OFFSET: u32 = 0x209f;
const PMERR_INV_PLAY_METAFILE_OPTION: u32 = 0x20b8;
const PMERR_INV_SEG_ATTR: u32 = 0x20c5;
const PMERR_INV_SEG_ATTR_VALUE: u32 = 0x20c6;
const PMERR_INV_SEG_NAME: u32 = 0x20c8;
const PMERR_NOT_IN_SEG: u32 = 0x20e3;
const PMERR_SEG_NOT_FOUND: u32 = 0x2106;

fn err(e: u32) {
    crate::misc::set_error(e);
}

// ---- Segment state of a presentation space ----

pub struct Seg {
    pub id: i32,
    /// Attributes: bit n for ATTR_* n.
    pub attrs: u32,
    pub data: Vec<u8>,
    /// Segment transform (GpiSetSegmentTransformMatrix).
    pub xform: Option<Matrix>,
}

pub struct SegState {
    pub mode: i32,
    pub segs: Vec<Seg>,
    /// The open segment: its name, retained or not.
    pub open: Option<(i32, bool)>,
    /// Attributes new segments get.
    pub initial: u32,
    /// Drawing retained orders (GpiDrawChain): not retained again.
    pub replaying: u32,
    /// The metafile DC the PS is associated with (0: none).
    pub meta_dc: u32,
    pub view_limits: RectL,
}

impl Default for SegState {
    fn default() -> Self {
        SegState {
            mode: DM_DRAW,
            segs: Vec::new(),
            open: None,
            initial: 1 << ATTR_VISIBLE | 1 << ATTR_CHAINED | 1 << ATTR_FASTCHAIN | 1 << ATTR_PROP_DETECTABLE | 1 << ATTR_PROP_VISIBLE,
            replaying: 0,
            meta_dc: 0,
            view_limits: RectL { left: i32::MIN / 2, bottom: i32::MIN / 2, right: i32::MAX / 2, top: i32::MAX / 2 },
        }
    }
}

/// What a metafile DC has recorded.
pub struct MetaRec {
    /// Segments (name, orders); drawing goes to the last.
    pub segs: Vec<(i32, Vec<u8>)>,
    pub fonts: Vec<metafile::Font>,
    pub units: u32,
    pub page: (i32, i32),
}

impl MetaRec {
    pub fn new() -> MetaRec {
        MetaRec { segs: vec![(0, Vec::new())], fonts: Vec::new(), units: gpi::xform::PU_PELS, page: (0, 0) }
    }

    fn append(&mut self, b: &[u8]) {
        self.segs.last_mut().unwrap().1.extend_from_slice(b);
    }

    /// Start a segment (0: back to unnamed drawing).
    fn begin(&mut self, id: i32) {
        if self.segs.last().is_some_and(|s| s.1.is_empty()) {
            self.segs.pop();
        }
        self.segs.push((id, Vec::new()));
    }

    /// The metafile.
    pub fn build(&self) -> Vec<u8> {
        let mut data = Vec::new();
        for (id, d) in &self.segs {
            if d.is_empty() && *id == 0 {
                continue;
            }
            goca::encode(&Order::BeginSegment { id: *id as u32, flags: 0x7000, len: d.len() as u32 }, &mut data);
            data.extend_from_slice(d);
        }
        metafile::build(&Metafile { units: self.units, page: self.page, fonts: self.fonts.clone(), data, ..Metafile::default() })
    }
}

fn seg_state(hps: u32) -> Option<&'static mut SegState> {
    entry(st(), hps).map(|e| &mut e.gpi.seg)
}

fn enc(o: &Order) -> Vec<u8> {
    let mut v = Vec::new();
    goca::encode(o, &mut v);
    v
}

/// Record the order of a GPI call on `hps`: into the open retained
/// segment and onto the PS's metafile. False when the call must not draw
/// (DM_RETAIN in a segment).
pub fn rec(hps: u32, o: impl FnOnce() -> Order) -> bool {
    let Some(g) = seg_state(hps) else { return true };
    let retain = g.replaying == 0 && g.mode != DM_DRAW && matches!(g.open, Some((_, true)));
    if !retain && g.meta_dc == 0 {
        return true;
    }
    let bytes = enc(&o());
    let draw = !retain || g.mode == DM_DRAWANDRETAIN;
    if retain {
        let id = g.open.unwrap().0;
        if let Some(s) = g.segs.iter_mut().rev().find(|s| s.id == id) {
            s.data.extend_from_slice(&bytes);
        }
    }
    let dc = g.meta_dc;
    if draw && dc != 0 {
        if let Some(m) = crate::gpiobj::meta_rec(dc) {
            m.append(&bytes);
        }
    }
    draw
}

/// Record an attribute order: "push and set" in AM_PRESERVE mode.
pub fn rec_attr(hps: u32, a: impl FnOnce() -> Attr) -> bool {
    let preserve = entry(st(), hps).is_some_and(|e| e.ps.attr_mode == 0);
    rec(hps, || if preserve { Order::Push(a()) } else { Order::Set(a()) })
}

/// Record a call that only a metafile keeps (an escape order).
pub fn rec_meta(hps: u32, ident: u8, data: impl FnOnce() -> Vec<u8>) {
    let Some(g) = seg_state(hps) else { return };
    if g.meta_dc == 0 {
        return;
    }
    let dc = g.meta_dc;
    let b = enc(&Order::Escape { ident, data: data() });
    if let Some(m) = crate::gpiobj::meta_rec(dc) {
        m.append(&b);
    }
}

/// GpiCreateLogFont on a metafile PS: the font goes into the metafile.
pub fn meta_font(hps: u32, name: [u8; 8], lcid: i32, fattrs: &[u8; 56]) {
    rec_meta(hps, goca::ESC_CREATELOGFONT, || {
        let mut d = name.to_vec();
        d.extend_from_slice(&lcid.to_le_bytes());
        d.extend_from_slice(fattrs);
        d
    });
    let Some(dc) = seg_state(hps).map(|g| g.meta_dc).filter(|&d| d != 0) else { return };
    if let Some(m) = crate::gpiobj::meta_rec(dc) {
        m.fonts.retain(|f| f.lcid as i32 != lcid);
        m.fonts.push(metafile::Font { lcid: lcid as u8, name, fattrs: *fattrs });
    }
}

/// A PS was associated with (or left, `dc` 0) a metafile DC.
pub fn set_meta_dc(hps: u32, dc: u32) {
    if let Some(e) = entry(st(), hps) {
        e.gpi.seg.meta_dc = dc;
        let (u, p) = (e.ps.page_units, e.ps.page_size);
        if dc != 0 {
            if let Some(m) = crate::gpiobj::meta_rec(dc) {
                m.units = u;
                m.page = p;
            }
        }
    }
}

/// Run a GPI call that has been recorded already.
pub fn quiet_draw<R>(hps: u32, f: impl FnOnce() -> R) -> R {
    quiet(hps, f)
}

/// Run `f` without recording anything on `hps`.
fn quiet<R>(hps: u32, f: impl FnOnce() -> R) -> R {
    let saved = seg_state(hps).map(|g| {
        g.replaying += 1;
        core::mem::replace(&mut g.meta_dc, 0)
    });
    let r = f();
    if let (Some(dc), Some(g)) = (saved, seg_state(hps)) {
        g.replaying -= 1;
        g.meta_dc = dc;
    }
    r
}

// ---- Recording helpers for the GPI calls ----

/// Model transform options (TRANSFORM_*) to GSTM flags and back.
pub fn gstm_flags(opt: i32) -> u8 {
    match opt {
        1 => 1,
        2 => 2,
        _ => 3,
    }
}

fn transform_opt(flags: u8) -> i32 {
    match flags {
        1 => 1,
        2 => 2,
        _ => 0,
    }
}

/// MATRIXLF elements to an order's (m11 m12 m21 m22 m41 m42).
pub fn goca_matrix(m: &Matrix) -> [i32; 6] {
    let lf = m.to_lf();
    [lf[0], lf[1], lf[3], lf[4], lf[6], lf[7]]
}

fn lf(m: &[i32; 6]) -> [i32; 9] {
    [m[0], m[1], 0, m[2], m[3], 0, m[4], m[5], 1]
}

/// GCCHSTE flags of CHS_* options.
pub fn chs_flags(opts: u32) -> u8 {
    let mut f = 0;
    for (o, b) in [(1, 0x80), (0x10, 0x40), (8, 0x10), (0x200, 8), (0x400, 4)] {
        if opts & o != 0 {
            f |= b;
        }
    }
    f
}

fn chs_options(f: u8) -> u32 {
    let mut o = 0;
    for (b, v) in [(0x80, 1), (0x40, 0x10), (0x10, 8), (8, 0x200), (4, 0x400)] {
        if f & b != 0 {
            o |= v;
        }
    }
    o
}

/// GpiSetAttrs: the bundle as orders (an element of the bundle type, as
/// PMGPI writes it).
pub fn rec_bundle(hps: u32, prim: i32, mask: u32) {
    let Some(e) = entry(st(), hps) else { return };
    if e.gpi.seg.meta_dc == 0 && !matches!(e.gpi.seg.open, Some((_, true))) {
        return;
    }
    let a = e.ps.attrs.clone();
    let b = match prim {
        gpi::PRIM_LINE => a.line,
        gpi::PRIM_CHAR => a.chr,
        gpi::PRIM_MARKER => a.marker,
        gpi::PRIM_AREA => a.area,
        gpi::PRIM_IMAGE => a.image,
        _ => return,
    };
    let p = prim as u8;
    let mut v = vec![Order::BeginElement { ty: 0xfd00 | prim, desc: Vec::new() }];
    for (bit, attr, value) in [(1, 1, b.color), (2, 2, b.back), (4, 3, b.mix), (8, 4, b.back_mix)] {
        if mask & bit != 0 {
            v.push(Order::Set(Attr::Individual { attr, prim: p, value }));
        }
    }
    let m = |bit: u32| mask & bit != 0;
    match prim {
        gpi::PRIM_LINE => {
            if m(0x10) {
                v.push(Order::Set(Attr::LineWidth(a.line_width)));
            }
            if m(0x20) {
                v.push(Order::Set(Attr::LineWidthGeom(Some(a.line_width_geom))));
            }
            if m(0x40) {
                v.push(Order::Set(Attr::LineType(a.line_type)));
            }
            if m(0x80) {
                v.push(Order::Set(Attr::LineEnd(a.line_end)));
            }
            if m(0x100) {
                v.push(Order::Set(Attr::LineJoin(a.line_join)));
            }
        }
        gpi::PRIM_CHAR => {
            if m(0x10) {
                v.push(Order::Set(Attr::CharSet(a.char_set)));
            }
            if m(0x20) {
                v.push(Order::Set(Attr::CharMode(a.char_mode)));
            }
            if m(0x40) {
                v.push(Order::Set(Attr::CharBox(a.char_box.0, a.char_box.1)));
            }
            if m(0x80) {
                v.push(Order::Set(Attr::CharAngle(Point::new(a.char_angle.0, a.char_angle.1))));
            }
            if m(0x100) {
                v.push(Order::Set(Attr::CharShear(Point::new(a.char_shear.0, a.char_shear.1))));
            }
            if m(0x200) {
                v.push(Order::Set(Attr::CharDirection(a.char_direction)));
            }
            if m(0x400) {
                v.push(Order::Set(Attr::TextAlign(a.text_align.0, a.text_align.1)));
            }
            if m(0x800) {
                v.push(Order::Set(Attr::CharExtra(a.char_extra)));
            }
            if m(0x1000) {
                v.push(Order::Set(Attr::CharBreakExtra(a.char_break_extra)));
            }
        }
        gpi::PRIM_MARKER => {
            if m(0x10) {
                v.push(Order::Set(Attr::MarkerSet(a.marker_set)));
            }
            if m(0x20) {
                v.push(Order::Set(Attr::Marker(a.marker_sym)));
            }
            if m(0x40) {
                v.push(Order::Set(Attr::MarkerBox(a.marker_box.0, a.marker_box.1)));
            }
        }
        gpi::PRIM_AREA => {
            if m(0x10) {
                v.push(Order::Set(Attr::PatternSet(a.pattern_set)));
            }
            if m(0x20) {
                v.push(Order::Set(Attr::Pattern(a.pattern)));
            }
            if m(0x40) {
                v.push(Order::Set(Attr::PatternRef(Some(a.pattern_ref))));
            }
        }
        _ => {}
    }
    v.push(Order::EndElement);
    for o in v {
        rec(hps, || o);
    }
}

// ---- Playing orders ----

#[derive(Default)]
struct Play {
    /// GpiImage being collected (format, width, height, rows).
    image: Option<(i32, i32, i32, Vec<u8>)>,
    /// Escapes that load objects are done (LC_LOADDISC, CTAB_REPLACE,
    /// LT_ORIGINALVIEW).
    fonts: bool,
    ctab: bool,
    view: bool,
    depth: u32,
}

fn pl(p: Point) -> PointL {
    PointL { x: p.x, y: p.y }
}

fn le32(d: &[u8], o: usize) -> i32 {
    d.get(o..o + 4).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()))
}

/// Set an attribute with AM_PRESERVE (push) or AM_NOPRESERVE.
fn set_attr(hps: u32, a: &Attr, push: bool) {
    let saved = entry(st(), hps).map(|e| core::mem::replace(&mut e.ps.attr_mode, if push { 0 } else { 1 }));
    use crate::gpiapi::*;
    use crate::gpitext::*;
    match *a {
        Attr::Color(c) => {
            GpiSetColor(hps, c);
        }
        Attr::BackColor(c) => {
            GpiSetBackColor(hps, c);
        }
        Attr::Mix(m) => {
            GpiSetMix(hps, m);
        }
        Attr::BackMix(m) => {
            GpiSetBackMix(hps, m);
        }
        Attr::Individual { attr, prim, value } => {
            let mut b = [0u8; 64];
            let (bit, off, w) = match attr {
                1 => (1, 0, 4),
                2 => (2, 4, 4),
                3 => (4, 8, 2),
                _ => (8, 10, 2),
            };
            b[off..off + w].copy_from_slice(&value.to_le_bytes()[..w]);
            GpiSetAttrs(hps, prim as i32, bit, 0, b.as_ptr());
        }
        Attr::LineType(v) => {
            GpiSetLineType(hps, v);
        }
        Attr::LineWidth(v) => {
            GpiSetLineWidth(hps, v);
        }
        Attr::LineWidthGeom(v) => {
            GpiSetLineWidthGeom(hps, v.unwrap_or(gpi::Attrs::default().line_width_geom));
        }
        Attr::LineEnd(v) => {
            GpiSetLineEnd(hps, v);
        }
        Attr::LineJoin(v) => {
            GpiSetLineJoin(hps, v);
        }
        Attr::PatternSet(v) => {
            GpiSetPatternSet(hps, v);
        }
        Attr::Pattern(v) => {
            GpiSetPattern(hps, v);
        }
        Attr::PatternRef(p) => {
            GpiSetPatternRefPoint(hps, &pl(p.unwrap_or_default()));
        }
        Attr::MarkerSet(v) => {
            GpiSetMarkerSet(hps, v);
        }
        Attr::Marker(v) => {
            GpiSetMarker(hps, v);
        }
        Attr::MarkerBox(x, y) => {
            GpiSetMarkerBox(hps, [x, y].as_ptr());
        }
        Attr::CharSet(v) => {
            GpiSetCharSet(hps, v);
        }
        Attr::CharBox(x, y) => {
            GpiSetCharBox(hps, [x, y].as_ptr());
        }
        Attr::CharAngle(p) => {
            GpiSetCharAngle(hps, [p.x, p.y].as_ptr());
        }
        Attr::CharShear(p) => {
            GpiSetCharShear(hps, [p.x, p.y].as_ptr());
        }
        Attr::CharDirection(v) => {
            GpiSetCharDirection(hps, v);
        }
        Attr::CharMode(v) => {
            GpiSetCharMode(hps, v);
        }
        Attr::CharExtra(v) => {
            GpiSetCharExtra(hps, v);
        }
        Attr::CharBreakExtra(v) => {
            GpiSetCharBreakExtra(hps, v);
        }
        Attr::TextAlign(h, v) => {
            GpiSetTextAlignment(hps, h, v);
        }
        Attr::ArcParams(p) => {
            GpiSetArcParams(hps, &p);
        }
        Attr::Position(p) => {
            GpiMove(hps, &pl(p));
        }
        Attr::ModelTransform { flags, m } => {
            GpiSetModelTransformMatrix(hps, 9, lf(&m).as_ptr(), transform_opt(flags));
        }
        Attr::PickId(_) => {}
    }
    if let (Some(m), Some(e)) = (saved, entry(st(), hps)) {
        e.ps.attr_mode = m;
    }
}

fn exec(hps: u32, o: &Order, p: &mut Play) {
    use crate::gpiapi::*;
    use crate::gpitext::*;
    let mv = |at: &Option<Point>| {
        if let Some(a) = at {
            GpiMove(hps, &pl(*a));
        }
    };
    let pts = |v: &[Point]| -> Vec<PointL> { v.iter().map(|&q| pl(q)).collect() };
    match o {
        Order::Set(a) => set_attr(hps, a, false),
        Order::Push(a) => set_attr(hps, a, true),
        Order::Pop => {
            GpiPop(hps, 1);
        }
        Order::ViewTransform { flags, m } => {
            GpiSetViewingTransformMatrix(hps, 9, lf(m).as_ptr(), if *flags == 4 { 1 } else { 0 });
        }
        Order::Line { at, pts: v } => {
            mv(at);
            let v = pts(v);
            GpiPolyLine(hps, v.len() as i32, v.as_ptr());
        }
        Order::RelLine { at, d } => {
            mv(at);
            let mut c = PointL { x: 0, y: 0 };
            GpiQueryCurrentPosition(hps, &mut c);
            let v: Vec<PointL> = d
                .iter()
                .map(|&(dx, dy)| {
                    c = PointL { x: c.x + dx as i32, y: c.y + dy as i32 };
                    c
                })
                .collect();
            GpiPolyLine(hps, v.len() as i32, v.as_ptr());
        }
        Order::Box { at, flags, corner, h, v } => {
            mv(at);
            let ctl = match flags & 0x60 {
                0x60 => 3,
                0x40 => 1,
                _ => 2,
            };
            GpiBox(hps, ctl, &pl(*corner), *h, *v);
        }
        Order::FullArc { at, mult } => {
            mv(at);
            GpiFullArc(hps, 2, *mult);
        }
        Order::PartialArc { at, center, mult, start, sweep } => {
            mv(at);
            GpiPartialArc(hps, &pl(*center), *mult, *start, *sweep);
        }
        Order::Arc { at, inter, end } => {
            mv(at);
            let v = [pl(*inter), pl(*end)];
            GpiPointArc(hps, v.as_ptr());
        }
        Order::Bezier { at, pts: v } => {
            mv(at);
            let v = pts(v);
            GpiPolySpline(hps, v.len() as i32, v.as_ptr());
        }
        Order::Fillet { at, pts: v } => {
            mv(at);
            let v = pts(v);
            GpiPolyFillet(hps, v.len() as i32, v.as_ptr());
        }
        Order::SharpFillet { at, pts: v, sharp } => {
            mv(at);
            let v = pts(v);
            GpiPolyFilletSharp(hps, v.len() as i32, v.as_ptr(), sharp.as_ptr());
        }
        Order::Marker { at, pts: v } => {
            match at {
                Some(a) => {
                    GpiMarker(hps, &pl(*a));
                }
                None if v.is_empty() => {
                    let mut c = PointL { x: 0, y: 0 };
                    GpiQueryCurrentPosition(hps, &mut c);
                    GpiMarker(hps, &c);
                }
                None => {}
            }
            if !v.is_empty() {
                let v = pts(v);
                GpiPolyMarker(hps, v.len() as i32, v.as_ptr());
            }
        }
        Order::CharString { at, text, .. } => match at {
            Some(a) => {
                GpiCharStringAt(hps, &pl(*a), text.len() as i32, text.as_ptr());
            }
            None => {
                GpiCharString(hps, text.len() as i32, text.as_ptr());
            }
        },
        Order::CharStringExt { at, flags, rect, text, dx } => {
            let mut opts = chs_options(*flags);
            if !dx.is_empty() {
                opts |= 2;
            }
            let r = RectL { left: rect[0].x, bottom: rect[0].y, right: rect[1].x, top: rect[1].y };
            let pr: *const RectL = if flags & 0xc0 != 0 { &r } else { core::ptr::null() };
            let n = text.len() as i32;
            match at {
                Some(a) => {
                    GpiCharStringPosAt(hps, &pl(*a), pr, opts, n, text.as_ptr(), dx.as_ptr());
                }
                None => {
                    GpiCharStringPos(hps, pr, opts, n, text.as_ptr(), dx.as_ptr());
                }
            }
        }
        Order::BeginArea(f) => {
            GpiBeginArea(hps, (f & 0x40 != 0) as u32 | if f & 0x20 != 0 { 2 } else { 0 });
        }
        Order::EndArea => {
            GpiEndArea(hps);
        }
        Order::BeginPath(id) => {
            GpiBeginPath(hps, *id);
        }
        Order::EndPath => {
            GpiEndPath(hps);
        }
        Order::CloseFigure => {
            GpiCloseFigure(hps);
        }
        Order::FillPath { flags, id } => {
            if flags & 0x20 != 0 {
                GpiStrokePath(hps, *id, 0);
            } else {
                GpiFillPath(hps, *id, if flags & 0x40 != 0 { 2 } else { 0 });
            }
        }
        Order::OutlinePath { id, .. } => {
            GpiOutlinePath(hps, *id, 0);
        }
        Order::ModifyPath { id, .. } => {
            GpiModifyPath(hps, *id, 6);
        }
        Order::SetClipPath { flags, id } => {
            if flags & 0x20 != 0 {
                GpiSetClipPath(hps, *id, 4 | if flags & 0x40 != 0 { 2 } else { 0 });
            } else {
                GpiSetClipPath(hps, 0, 0);
            }
        }
        Order::BeginImage { at, format, cx, cy } => {
            mv(at);
            p.image = Some((*format as i32, *cx as i32, *cy as i32, Vec::new()));
        }
        Order::ImageData(d) => {
            if let Some(i) = &mut p.image {
                i.3.extend_from_slice(d);
            }
        }
        Order::EndImage => {
            if let Some((f, cx, cy, d)) = p.image.take() {
                let size = [cx, cy];
                GpiImage(hps, f, size.as_ptr(), d.len() as i32, d.as_ptr());
            }
        }
        Order::CallSegment(id) => {
            let idx = seg_state(hps).and_then(|g| g.segs.iter().position(|s| s.id == *id as i32));
            if let Some(i) = idx {
                if p.depth < 16 {
                    p.depth += 1;
                    draw_seg(hps, i, false);
                    p.depth -= 1;
                }
            }
        }
        Order::SetPel(q) => {
            GpiSetPel(hps, &pl(*q));
        }
        Order::Escape { ident, data } => escape(hps, *ident, data, p),
        Order::Nop
        | Order::Comment(_)
        | Order::BeginSegment { .. }
        | Order::EndPrologue
        | Order::BeginElement { .. }
        | Order::EndElement
        | Order::Label(_)
        | Order::Unknown(_) => {}
    }
}

/// IBM's metafile escapes.
fn escape(hps: u32, ident: u8, d: &[u8], p: &Play) {
    use crate::gpiapi::*;
    match ident {
        goca::ESC_CREATELOGCOLORTABLE if p.ctab && d.len() >= 16 => {
            let n = le32(d, 12).max(0) as usize;
            let t: Vec<i32> = (0..n).map(|i| le32(d, 16 + 4 * i)).collect();
            crate::gpiobj::GpiCreateLogColorTable(hps, le32(d, 0) as u32, le32(d, 4), le32(d, 8), n as i32, t.as_ptr());
        }
        goca::ESC_CREATELOGFONT if p.fonts && d.len() >= 12 + 56 => {
            crate::gpitext::GpiCreateLogFont(hps, d.as_ptr(), le32(d, 8), d[12..].as_ptr());
        }
        goca::ESC_DELETESETID if p.fonts => {
            crate::gpitext::GpiDeleteSetId(hps, le32(d, 0));
        }
        goca::ESC_RESETPS => {
            GpiResetPS(hps, 1);
        }
        goca::ESC_SETCP => {
            crate::gpitext::GpiSetCp(hps, le32(d, 0) as u32);
        }
        goca::ESC_SETDEFAULTVIEWMATRIX if p.view && d.len() >= 24 => {
            let m = [le32(d, 0), le32(d, 4), le32(d, 8), le32(d, 12), le32(d, 16), le32(d, 20)];
            GpiSetDefaultViewMatrix(hps, 9, lf(&m).as_ptr(), 0);
        }
        goca::ESC_ERASE => {
            GpiErase(hps);
        }
        goca::ESC_INTERSECTCLIPRECT if d.len() >= 16 => {
            let r = RectL { left: le32(d, 0), bottom: le32(d, 4), right: le32(d, 8), top: le32(d, 12) };
            crate::gpiobj::GpiIntersectClipRectangle(hps, &r);
        }
        goca::ESC_EXCLUDECLIPRECT if d.len() >= 16 => {
            let r = RectL { left: le32(d, 0), bottom: le32(d, 4), right: le32(d, 8), top: le32(d, 12) };
            crate::gpiobj::GpiExcludeClipRectangle(hps, &r);
        }
        goca::ESC_SAVEPS => {
            GpiSavePS(hps);
        }
        goca::ESC_RESTOREPS => {
            GpiRestorePS(hps, -1);
        }
        goca::ESC_SETDEFATTRS if d.len() >= 8 => {
            GpiSetDefAttrs(hps, le32(d, 0), le32(d, 4) as u32, d[8..].as_ptr());
        }
        goca::ESC_SETDEFVIEWINGLIMITS if d.len() >= 16 => {
            let r = RectL { left: le32(d, 0), bottom: le32(d, 4), right: le32(d, 8), top: le32(d, 12) };
            GpiSetDefViewingLimits(hps, &r);
        }
        goca::ESC_SETDEFARCPARAMS if d.len() >= 16 => {
            let a = [le32(d, 0), le32(d, 4), le32(d, 8), le32(d, 12)];
            GpiSetDefArcParams(hps, &a);
        }
        _ => {}
    }
}

/// Play an order stream on `hps`.
fn play(hps: u32, data: &[u8], long: bool, p: &mut Play) {
    for o in goca::decode(data, long) {
        exec(hps, &o, p);
    }
}

/// A segment starts: default attributes, current position (0,0),
/// identity model and viewing transforms.
fn seg_start(hps: u32, xform: Option<Matrix>) {
    if let Some(e) = entry(st(), hps) {
        e.ps.reset(false);
        e.ps.set_model(xform.unwrap_or(Matrix::IDENTITY));
        e.ps.set_viewing(Matrix::IDENTITY);
        e.color_index = e.ps.attrs.line.color;
        e.back_index = e.ps.attrs.line.back;
    }
}

/// Draw retained segment `i` (also onto a metafile, as a segment of its
/// own).
fn draw_seg(hps: u32, i: usize, start: bool) {
    let Some(g) = seg_state(hps) else { return };
    let Some(s) = g.segs.get(i) else { return };
    if s.attrs & (1 << ATTR_VISIBLE) == 0 {
        return;
    }
    let (id, data, xform) = (s.id, s.data.clone(), s.xform);
    g.replaying += 1;
    let dc = g.meta_dc;
    if dc != 0 {
        if let Some(m) = crate::gpiobj::meta_rec(dc) {
            m.begin(id);
        }
    }
    if start {
        seg_start(hps, xform);
    }
    play(hps, &data, true, &mut Play::default());
    if dc != 0 {
        if let Some(m) = crate::gpiobj::meta_rec(dc) {
            m.begin(0);
        }
    }
    if let Some(g) = seg_state(hps) {
        g.replaying -= 1;
    }
}

// ---- Drawing mode and segments ----

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetDrawingMode(hps: u32, mode: i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    if !(DM_DRAW..=DM_DRAWANDRETAIN).contains(&mode) /* DM_RETAIN 2 */ {
        err(PMERR_INV_DRAWING_MODE);
        return 0;
    }
    g.mode = mode;
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDrawingMode(hps: u32) -> i32 {
    seg_state(hps).map_or(0, |g| g.mode)
}

/// GpiOpenSegment(hps, idSegment): retained in DM_RETAIN and
/// DM_DRAWANDRETAIN (an existing segment of that name is replaced).
#[unsafe(no_mangle)]
pub extern "C" fn GpiOpenSegment(hps: u32, id: i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    if g.open.is_some() {
        err(PMERR_ALREADY_IN_SEG);
        return 0;
    }
    if id < 0 {
        err(PMERR_INV_SEG_NAME);
        return 0;
    }
    let retain = g.mode != DM_DRAW && g.replaying == 0;
    if retain {
        if id != 0 {
            g.segs.retain(|s| s.id != id);
        }
        let attrs = g.initial;
        g.segs.push(Seg { id, attrs, data: Vec::new(), xform: None });
    }
    g.open = Some((id, retain));
    let dc = g.meta_dc;
    if dc != 0 && !retain {
        if let Some(m) = crate::gpiobj::meta_rec(dc) {
            m.begin(id);
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiCloseSegment(hps: u32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    let Some((_, retain)) = g.open.take() else {
        err(PMERR_NOT_IN_SEG);
        return 0;
    };
    let dc = g.meta_dc;
    if dc != 0 && !retain {
        if let Some(m) = crate::gpiobj::meta_rec(dc) {
            m.begin(0);
        }
    }
    1
}

/// Close any open segment and delete all (GpiResetPS GRES_SEGMENTS).
pub fn reset_segments(hps: u32) {
    if let Some(g) = seg_state(hps) {
        g.open = None;
        g.segs.clear();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDeleteSegment(hps: u32, id: i32) -> u32 {
    GpiDeleteSegments(hps, id, id)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDeleteSegments(hps: u32, first: i32, last: i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    let open = g.open.map(|o| o.0);
    g.segs.retain(|s| !(first..=last).contains(&s.id) || Some(s.id) == open);
    1
}

/// GpiDrawChain(hps): the chained segments, in order.
#[unsafe(no_mangle)]
pub extern "C" fn GpiDrawChain(hps: u32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    let n = g.segs.len();
    for i in 0..n {
        if seg_state(hps).and_then(|g| g.segs.get(i)).is_some_and(|s| s.attrs & (1 << ATTR_CHAINED) != 0) {
            draw_seg(hps, i, true);
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDrawSegment(hps: u32, id: i32) -> u32 {
    let Some(i) = seg_state(hps).and_then(|g| g.segs.iter().position(|s| s.id == id)) else {
        err(PMERR_SEG_NOT_FOUND);
        return 0;
    };
    draw_seg(hps, i, true);
    1
}

/// GpiDrawFrom(hps, idFirst, idLast): the chain from one segment to
/// another.
#[unsafe(no_mangle)]
pub extern "C" fn GpiDrawFrom(hps: u32, first: i32, last: i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    let Some(a) = g.segs.iter().position(|s| s.id == first) else {
        err(PMERR_SEG_NOT_FOUND);
        return 0;
    };
    let b = g.segs.iter().position(|s| s.id == last).unwrap_or(g.segs.len() - 1);
    for i in a..=b.max(a) {
        if seg_state(hps).and_then(|g| g.segs.get(i)).is_some_and(|s| s.attrs & (1 << ATTR_CHAINED) != 0) {
            draw_seg(hps, i, true);
        }
    }
    1
}

/// GpiQuerySegmentNames(hps, idFirst, idLast, lMax, alSegid): the names
/// of the retained segments in a range.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQuerySegmentNames(hps: u32, first: i32, last: i32, max: i32, out: *mut i32) -> i32 {
    let Some(g) = seg_state(hps) else { return GPI_ERROR };
    let mut n = 0;
    for s in g.segs.iter().filter(|s| s.id != 0 && (first..=last).contains(&s.id)) {
        if n >= max {
            break;
        }
        write(unsafe { out.add(n as usize) }, s.id);
        n += 1;
    }
    n
}

fn attr_ok(attr: i32, val: i32) -> bool {
    let known = matches!(attr, ATTR_DETECTABLE | ATTR_VISIBLE | ATTR_CHAINED | ATTR_DYNAMIC | ATTR_FASTCHAIN | ATTR_PROP_DETECTABLE | ATTR_PROP_VISIBLE);
    if !known {
        err(PMERR_INV_SEG_ATTR);
    } else if !(0..=1).contains(&val) {
        err(PMERR_INV_SEG_ATTR_VALUE);
    }
    known && (0..=1).contains(&val)
}

fn set_bit(v: &mut u32, attr: i32, on: bool) {
    if on {
        *v |= 1 << attr;
    } else {
        *v &= !(1 << attr);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetInitialSegmentAttrs(hps: u32, attr: i32, val: i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    if !attr_ok(attr, val) {
        return 0;
    }
    set_bit(&mut g.initial, attr, val == 1);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryInitialSegmentAttrs(hps: u32, attr: i32) -> i32 {
    let Some(g) = seg_state(hps) else { return ATTR_ERROR };
    if !attr_ok(attr, 0) {
        return ATTR_ERROR;
    }
    (g.initial >> attr & 1) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetSegmentAttrs(hps: u32, id: i32, attr: i32, val: i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    if !attr_ok(attr, val) {
        return 0;
    }
    let Some(s) = g.segs.iter_mut().find(|s| s.id == id) else {
        err(PMERR_SEG_NOT_FOUND);
        return 0;
    };
    set_bit(&mut s.attrs, attr, val == 1);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQuerySegmentAttrs(hps: u32, id: i32, attr: i32) -> i32 {
    let Some(g) = seg_state(hps) else { return ATTR_ERROR };
    if !attr_ok(attr, 0) {
        return ATTR_ERROR;
    }
    match g.segs.iter().find(|s| s.id == id) {
        Some(s) => (s.attrs >> attr & 1) as i32,
        None => {
            err(PMERR_SEG_NOT_FOUND);
            ATTR_ERROR
        }
    }
}

/// GpiSetSegmentTransformMatrix(hps, idSegment, lCount, pmatlfArray,
/// lOptions)
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetSegmentTransformMatrix(hps: u32, id: i32, n: i32, m: *const i32, opt: i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    let Some(s) = g.segs.iter_mut().find(|s| s.id == id) else {
        err(PMERR_SEG_NOT_FOUND);
        return 0;
    };
    let new = if n == 0 || m.is_null() { Matrix::IDENTITY } else { Matrix::from_lf(&(0..n.min(9) as usize).map(|i| unsafe { m.add(i).read_unaligned() }).collect::<Vec<_>>()) };
    let Some(x) = Matrix::combine(&s.xform.unwrap_or(Matrix::IDENTITY), &new, opt) else { return 0 };
    s.xform = Some(x);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQuerySegmentTransformMatrix(hps: u32, id: i32, n: i32, m: *mut i32) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    let Some(s) = g.segs.iter().find(|s| s.id == id) else {
        err(PMERR_SEG_NOT_FOUND);
        return 0;
    };
    let lf = s.xform.unwrap_or(Matrix::IDENTITY).to_lf();
    for (i, v) in lf.iter().take(n.clamp(0, 9) as usize).enumerate() {
        write(unsafe { m.add(i) }, *v);
    }
    1
}

/// GpiGetData(hps, idSegment, plOffset, lFormat, lLength, pbData): the
/// orders of a retained segment from an offset (whole orders, in the long
/// format); the offset moves on. Returns the bytes given.
#[unsafe(no_mangle)]
pub extern "C" fn GpiGetData(hps: u32, id: i32, poff: *mut i32, _format: i32, len: i32, out: *mut u8) -> i32 {
    let Some(g) = seg_state(hps) else { return -1 };
    let Some(s) = g.segs.iter().rev().find(|s| s.id == id) else {
        err(PMERR_SEG_NOT_FOUND);
        return -1;
    };
    let Some(off) = read(poff) else { return -1 };
    if off < 0 || off as usize > s.data.len() || len < 0 || (len > 0 && out.is_null()) {
        return -1;
    }
    let rest = &s.data[off as usize..];
    let n = goca::whole_orders(rest, len as usize, false);
    unsafe { core::ptr::copy_nonoverlapping(rest.as_ptr(), out, n) };
    write(poff, off + n as i32);
    n as i32
}

/// GpiPutData(hps, lFormat, plCount, pbData): orders, as if they were
/// GPI calls (drawn, retained, recorded). DFORM_PCSHORT (2) has 2-byte
/// coordinates; the others are taken as the long format.
#[unsafe(no_mangle)]
pub extern "C" fn GpiPutData(hps: u32, format: i32, pcount: *mut i32, data: *const u8) -> i32 {
    if entry(st(), hps).is_none() {
        return GPI_ERROR;
    }
    let Some(n) = read(pcount) else { return GPI_ERROR };
    if n < 0 || (n > 0 && data.is_null()) {
        return GPI_ERROR;
    }
    let d = if n == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(data, n as usize) } };
    let used = goca::whole_orders(d, d.len(), false);
    play(hps, &d[..used], format != 2, &mut Play { fonts: true, ctab: true, view: true, ..Play::default() });
    write(pcount, used as i32);
    GPI_OK
}

/// GpiSetViewingTransformMatrix(hps, lCount, pmatlfArray, lOptions)
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetViewingTransformMatrix(hps: u32, n: i32, m: *const i32, opt: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let new = if n == 0 || m.is_null() { Matrix::IDENTITY } else { Matrix::from_lf(&(0..n.min(9) as usize).map(|i| unsafe { m.add(i).read_unaligned() }).collect::<Vec<_>>()) };
    let cur = e.ps.viewing;
    let Some(x) = Matrix::combine(&cur, &new, opt) else { return 0 };
    rec(hps, || Order::ViewTransform { flags: if opt == 1 { 4 } else { 0 }, m: goca_matrix(&new) });
    if let Some(e) = entry(st(), hps) {
        e.ps.set_viewing(x);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryViewingTransformMatrix(hps: u32, n: i32, m: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if m.is_null() || !(0..=9).contains(&n) {
        return 0;
    }
    for (i, v) in e.ps.viewing.to_lf().iter().take(n as usize).enumerate() {
        write(unsafe { m.add(i) }, *v);
    }
    1
}

/// GpiSetDefViewingLimits(hps, prclLimits): kept (drawing is not
/// limited by them).
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetDefViewingLimits(hps: u32, r: *const RectL) -> u32 {
    let Some(v) = read(r) else { return 0 };
    let Some(g) = seg_state(hps) else { return 0 };
    g.view_limits = v;
    rec_meta(hps, goca::ESC_SETDEFVIEWINGLIMITS, || [v.left, v.bottom, v.right, v.top].iter().flat_map(|x| x.to_le_bytes()).collect());
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDefViewingLimits(hps: u32, r: *mut RectL) -> u32 {
    let Some(g) = seg_state(hps) else { return 0 };
    write(r, g.view_limits) as u32
}

/// Bundle sizes for the GpiSetDefAttrs escape.
fn bundle_size(prim: i32) -> usize {
    match prim {
        gpi::PRIM_LINE => 28,
        gpi::PRIM_CHAR => 52,
        gpi::PRIM_MARKER => 24,
        gpi::PRIM_AREA => 24,
        _ => 12,
    }
}

/// GpiSetDefAttrs(hps, lPrimType, flAttrMask, ppbunAttrs): the default
/// values (what a reset gives) and the current ones.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetDefAttrs(hps: u32, prim: i32, mask: u32, b: *const u8) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if b.is_null() || !(1..=5).contains(&prim) {
        return 0;
    }
    let cur = core::mem::replace(&mut e.ps.attrs, e.ps.defaults.clone());
    let ok = quiet(hps, || crate::gpiapi::GpiSetAttrs(hps, prim, mask, 0, b));
    let Some(e) = entry(st(), hps) else { return 0 };
    e.ps.defaults = core::mem::replace(&mut e.ps.attrs, cur);
    if ok == 0 {
        return 0;
    }
    quiet(hps, || crate::gpiapi::GpiSetAttrs(hps, prim, mask, 0, b));
    rec_meta(hps, goca::ESC_SETDEFATTRS, || {
        let mut d = prim.to_le_bytes().to_vec();
        d.extend_from_slice(&mask.to_le_bytes());
        d.extend_from_slice(unsafe { core::slice::from_raw_parts(b, bundle_size(prim)) });
        d
    });
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDefAttrs(hps: u32, prim: i32, mask: u32, b: *mut u8) -> i32 {
    let Some(e) = entry(st(), hps) else { return -1 };
    let cur = core::mem::replace(&mut e.ps.attrs, e.ps.defaults.clone());
    let r = crate::gpiapi::GpiQueryAttrs(hps, prim, mask, b);
    if let Some(e) = entry(st(), hps) {
        e.ps.attrs = cur;
    }
    r
}

/// GpiImage(hps, lFormat, psizlImageSize, lLength, pbData): a 1 bpp image
/// (rows from the top, MSB first, padded to bytes) with its top left at
/// the current position, in the image colours and mixes.
#[unsafe(no_mangle)]
pub extern "C" fn GpiImage(hps: u32, format: i32, size: *const i32, len: i32, data: *const u8) -> i32 {
    let Some(sz) = read(size as *const [i32; 2]) else { return GPI_ERROR };
    let (w, h) = (sz[0], sz[1]);
    if format != 0 || w < 0 || h < 0 || len < 0 || (len > 0 && data.is_null()) {
        return GPI_ERROR;
    }
    let d = if len == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(data, len as usize) } };
    let stride = (w as usize).div_ceil(8);
    let draw = rec(hps, || Order::BeginImage { at: None, format: 0, cx: w as u16, cy: h as u16 });
    for r in d.chunks(stride.max(1)).take(h as usize) {
        rec(hps, || Order::ImageData(r.to_vec()));
    }
    rec(hps, || Order::EndImage);
    if !draw {
        return GPI_OK;
    }
    gpiapi::draw(hps, |e, s| {
        let mut p = [e.ps.pos];
        e.ps.convert(gpi::CVTC_WORLD, gpi::CVTC_DEVICE, &mut p);
        let (x0, ytop) = (p[0].x, p[0].y);
        // Bottom-up pels: 1 bits foreground (white source), 0 background.
        let mut px = vec![None; (w * h) as usize];
        let leave_back = e.ps.attrs.image.back_mix == 5 || e.ps.attrs.image.back_mix == 0;
        for row in 0..h {
            for col in 0..w {
                let byte = d.get(row as usize * stride + col as usize / 8).copied().unwrap_or(0);
                let on = byte & (0x80 >> (col % 8)) != 0;
                let k = ((h - 1 - row) * w + col) as usize;
                px[k] = if on { Some(0xffffff) } else if leave_back { None } else { Some(0) };
            }
        }
        let t = gpi::Rect::new(x0, ytop - h + 1, x0 + w, ytop + 1);
        e.ps.blit(s, t, Some((&px, w, h)), 0xcc, true, None);
    })
    .map_or(GPI_ERROR, |_| GPI_OK)
}

// ---- Metafile handles ----

pub struct Mf {
    pub bits: Vec<u8>,
    /// Owner (0: the system, e.g. on the clipboard).
    pub pid: u32,
}

const HMF_BASE: u32 = 0x5000_0001;

fn mf(h: u32) -> Option<&'static mut Mf> {
    let me = crate::state::pid();
    st().metafiles.get_mut(h.wrapping_sub(HMF_BASE) as usize)?.as_mut().filter(|m| m.pid == me || m.pid == 0)
}

pub fn new_mf(bits: Vec<u8>) -> u32 {
    let v = &mut st().metafiles;
    let m = Some(Mf { bits, pid: crate::state::pid() });
    match v.iter().position(|x| x.is_none()) {
        Some(i) => {
            v[i] = m;
            HMF_BASE + i as u32
        }
        None => {
            v.push(m);
            HMF_BASE + v.len() as u32 - 1
        }
    }
}

/// The clipboard takes a metafile: it belongs to the system from now on.
pub fn give_to_system(h: u32) -> bool {
    match mf(h) {
        Some(m) => {
            m.pid = 0;
            true
        }
        None => false,
    }
}

/// Free a metafile whatever its owner (the clipboard empties).
pub fn free(h: u32) {
    if let Some(s) = st().metafiles.get_mut(h.wrapping_sub(HMF_BASE) as usize) {
        *s = None;
    }
}

/// A process ended: its metafiles go.
pub fn process_gone(p: u32) {
    for s in st().metafiles.iter_mut() {
        if s.as_ref().is_some_and(|m| m.pid == p) {
            *s = None;
        }
    }
}

fn cstr(p: *const u8) -> Option<Vec<u8>> {
    if p.is_null() {
        return None;
    }
    let mut v = Vec::new();
    unsafe {
        while *p.add(v.len()) != 0 && v.len() < 260 {
            v.push(*p.add(v.len()));
        }
    }
    Some(v)
}

/// The size of a file, if it is there.
pub(crate) fn read_size(name: &[u8]) -> Option<u32> {
    let mut z = name.to_vec();
    z.push(0);
    // FILESTATUS3: cbFile at 12.
    let mut fs = [0u8; 24];
    (unsafe { os2::DosQueryPathInfo(z.as_ptr(), 1, fs.as_mut_ptr(), 24) } == 0).then(|| u32::from_le_bytes([fs[12], fs[13], fs[14], fs[15]]))
}

pub(crate) fn read_file(name: &[u8]) -> Option<Vec<u8>> {
    let mut z = name.to_vec();
    z.push(0);
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS; OPEN_SHARE_DENYWRITE | READONLY.
    if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut size = 0u32;
    unsafe { os2::DosSetFilePtr(hf, 0, 2, &mut size) };
    let mut pos = 0u32;
    unsafe { os2::DosSetFilePtr(hf, 0, 0, &mut pos) };
    let mut v = vec![0u8; size as usize];
    let mut got = 0u32;
    let ok = size <= 64 << 20 && unsafe { os2::DosRead(hf, v.as_mut_ptr(), size, &mut got) } == 0 && got == size;
    unsafe { os2::DosClose(hf) };
    ok.then_some(v)
}

pub fn write_file(name: &[u8], d: &[u8]) -> bool {
    let mut z = name.to_vec();
    z.push(0);
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_REPLACE_IF_EXISTS | CREATE_IF_NEW; DENYWRITE | WRITEONLY.
    if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, d.len() as u32, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut n = 0u32;
    let ok = unsafe { os2::DosWrite(hf, d.as_ptr(), d.len() as u32, &mut n) } == 0 && n as usize == d.len();
    unsafe { os2::DosClose(hf) };
    ok
}

/// GpiLoadMetaFile(hab, pszFilename)
#[unsafe(no_mangle)]
pub extern "C" fn GpiLoadMetaFile(_hab: u32, name: *const u8) -> u32 {
    let Some(b) = cstr(name).and_then(|n| read_file(&n)) else {
        err(PMERR_INV_METAFILE);
        return 0;
    };
    if metafile::parse(&b).is_none() {
        err(PMERR_INV_METAFILE);
        return 0;
    }
    new_mf(b)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiDeleteMetaFile(hmf: u32) -> u32 {
    if mf(hmf).is_none() {
        err(PMERR_INV_HMF);
        return 0;
    }
    free(hmf);
    1
}

/// GpiCopyMetaFile(hmf)
#[unsafe(no_mangle)]
pub extern "C" fn GpiCopyMetaFile(hmf: u32) -> u32 {
    let Some(b) = mf(hmf).map(|m| m.bits.clone()) else {
        err(PMERR_INV_HMF);
        return 0;
    };
    new_mf(b)
}

/// GpiQueryMetaFileLength(hmf)
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryMetaFileLength(hmf: u32) -> i32 {
    match mf(hmf) {
        Some(m) => m.bits.len() as i32,
        None => {
            err(PMERR_INV_HMF);
            -1
        }
    }
}

/// GpiQueryMetaFileBits(hmf, lOffset, lLength, pbData)
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryMetaFileBits(hmf: u32, off: i32, n: i32, p: *mut u8) -> u32 {
    let Some(m) = mf(hmf) else {
        err(PMERR_INV_HMF);
        return 0;
    };
    if off < 0 || off as usize > m.bits.len() {
        err(PMERR_INV_METAFILE_OFFSET);
        return 0;
    }
    if n < 0 || off as usize + n as usize > m.bits.len() || (n > 0 && p.is_null()) {
        err(PMERR_INV_METAFILE_LENGTH);
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(m.bits.as_ptr().add(off as usize), p, n as usize) };
    1
}

/// GpiSetMetaFileBits(hmf, lOffset, lLength, pbBuffer): writes at an
/// offset up to the current length (the metafile grows).
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetMetaFileBits(hmf: u32, off: i32, n: i32, p: *const u8) -> u32 {
    let Some(m) = mf(hmf) else {
        err(PMERR_INV_HMF);
        return 0;
    };
    if off < 0 || off as usize > m.bits.len() {
        err(PMERR_INV_METAFILE_OFFSET);
        return 0;
    }
    if n < 0 || (n > 0 && p.is_null()) {
        err(PMERR_INV_METAFILE_LENGTH);
        return 0;
    }
    let (o, n) = (off as usize, n as usize);
    if m.bits.len() < o + n {
        m.bits.resize(o + n, 0);
    }
    unsafe { core::ptr::copy_nonoverlapping(p, m.bits.as_mut_ptr().add(o), n) };
    1
}

/// GpiSaveMetaFile(hmf, pszFilename): writes the file; the metafile is
/// deleted, as by GpiDeleteMetaFile.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSaveMetaFile(hmf: u32, name: *const u8) -> u32 {
    let Some(b) = mf(hmf).map(|m| m.bits.clone()) else {
        err(PMERR_INV_HMF);
        return 0;
    };
    let Some(n) = cstr(name) else { return 0 };
    if !write_file(&n, &b) {
        err(PMERR_INV_METAFILE);
        return 0;
    }
    free(hmf);
    1
}

// Indices of GpiPlayMetaFile's options.
const PMF_SEGBASE: usize = 0;
const PMF_LOADTYPE: usize = 1;
const PMF_LCIDS: usize = 3;
const PMF_RESET: usize = 4;
const PMF_SUPPRESS: usize = 5;
const PMF_COLORTABLES: usize = 6;

/// GpiPlayMetaFile(hps, hmf, lCount1, alOptarray, plSegCount, lCount2,
/// pszDesc): draws the metafile (in DM_RETAIN and DM_DRAWANDRETAIN its
/// segments are retained). Options as IBM's PMGPI: LT_ORIGINALVIEW sets
/// the metafile's default viewing transform, LC_LOADDISC creates its
/// logical fonts, RES_RESET resets the PS to the metafile's page units
/// and size, SUP_SUPPRESS draws nothing, CTAB_REPLACE takes its colour
/// table.
#[unsafe(no_mangle)]
pub extern "C" fn GpiPlayMetaFile(hps: u32, hmf: u32, n1: i32, opts: *const i32, segs: *mut i32, n2: i32, desc: *mut u8) -> i32 {
    if entry(st(), hps).is_none() {
        return GPI_ERROR;
    }
    let Some(bits) = mf(hmf).map(|m| m.bits.clone()) else {
        err(PMERR_INV_HMF);
        return GPI_ERROR;
    };
    if !(0..=13).contains(&n1) || (n1 > 0 && opts.is_null()) || n2 < 0 {
        err(PMERR_INV_PLAY_METAFILE_OPTION);
        return GPI_ERROR;
    }
    let mut o = [0i32; 14];
    for (i, v) in o.iter_mut().enumerate().take(n1 as usize) {
        *v = unsafe { opts.add(i).read_unaligned() };
    }
    write(segs, 0);
    let Some(m) = metafile::parse(&bits) else {
        err(PMERR_INV_METAFILE);
        return GPI_ERROR;
    };
    if n2 > 0 && !desc.is_null() {
        let k = m.desc.len().min(n2 as usize - 1);
        unsafe {
            core::ptr::copy_nonoverlapping(m.desc.as_ptr(), desc, k);
            *desc.add(k) = 0;
        }
    }
    if o[PMF_RESET] == 2 {
        crate::gpiapi::GpiResetPS(hps, 4);
        let size = [m.page.0, m.page.1];
        if let Some(e) = entry(st(), hps) {
            e.ps.set_page(m.units, (size[0], size[1]));
            if let Some(v) = m.default_view {
                e.ps.set_view(Matrix::from_lf(&lf(&v)));
            }
        }
    }
    if o[PMF_SUPPRESS] == 2 {
        return GPI_OK;
    }
    let mut p = Play { fonts: o[PMF_LCIDS] == 3, ctab: o[PMF_COLORTABLES] == 3 || o[PMF_COLORTABLES] == 4, view: o[PMF_LOADTYPE] == 4, ..Play::default() };
    if p.ctab {
        if let Some(l) = &m.lct {
            if l.rgb {
                crate::gpiobj::GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
            } else {
                let t: Vec<i32> = l.entries.iter().flat_map(|&(i, c)| [i as i32, c as i32]).collect();
                crate::gpiobj::GpiCreateLogColorTable(hps, 0, 1, 0, t.len() as i32, t.as_ptr());
            }
        }
    }
    if p.fonts {
        for f in &m.fonts {
            crate::gpitext::GpiCreateLogFont(hps, f.name.as_ptr(), f.lcid as i32, f.fattrs.as_ptr());
        }
    }
    // The segments, each starting from the default attributes.
    let orders = goca::decode(&m.data, m.long);
    let retain = seg_state(hps).is_some_and(|g| g.mode != DM_DRAW);
    let base = o[PMF_SEGBASE];
    let mut open = false;
    for ord in &orders {
        if let Order::BeginSegment { id, .. } = ord {
            if open {
                GpiCloseSegment(hps);
                open = false;
            }
            if retain {
                open = GpiOpenSegment(hps, if *id == 0 { 0 } else { *id as i32 + base }) != 0;
            } else {
                seg_start(hps, None);
            }
            continue;
        }
        exec(hps, ord, &mut p);
    }
    if open {
        GpiCloseSegment(hps);
    }
    GPI_OK
}
