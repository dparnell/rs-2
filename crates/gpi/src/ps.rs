//! Presentation space: drawing attributes, transforms, areas, paths,
//! clipping and the primitives.
//!
//! Coordinates: the application draws in world coordinates, which the
//! model transform, default viewing transform and page units map to
//! device coordinates (relative to the window or bitmap, y up). Device
//! coordinates plus `origin` are surface coordinates. Pixel output is
//! clipped to `clip` (surface rectangles: the visible region intersected
//! with the clip region and clip path).
//!
//! The simple methods (`fill_rect`, `line_to`, `draw_box`,
//! `char_string_at`) take page = device coordinates and the `color`,
//! `back_color`, `mix` and `back_mix` fields; the window manager uses them
//! for its own drawing. The GPI entry points use the `gpi_*` methods,
//! which honour transforms and the per-primitive attribute bundles.

use alloc::vec;
use alloc::vec::Vec;

use crate::bitmap::{self, Bitmap};
use crate::color::{ColorTable, Rgb, CLR_BACKGROUND, CLR_DEFAULT};
use crate::font::{self, Font};
use crate::math::{abs, fx, round, sqrt, PI};
use crate::pattern::{self, Pattern};
use crate::raster::{self, ipt, Pf};
use crate::rect::{Point, Rect};
use crate::region::{Op, Region};
use crate::surface::Surface;
use crate::xform::{self, Matrix};

/// Foreground/background mix modes (FM_* / BM_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mix {
    Overpaint,
    LeaveAlone,
    Xor,
    Or,
    And,
    Invert,
    /// Any other boolean combination: truth table indexed by
    /// (source << 1) | destination.
    Bool(u8),
}

impl Mix {
    fn from_code(v: i32) -> Option<Mix> {
        Some(match v {
            1 => Mix::Or,
            2 => Mix::Overpaint,
            4 => Mix::Xor,
            5 => Mix::LeaveAlone,
            6 => Mix::And,
            7 => Mix::Bool(0x2),  // SUBTRACT
            8 => Mix::Bool(0x4),  // MASKSRCNOT
            9 => Mix::Bool(0x0),  // ZERO
            10 => Mix::Bool(0x1), // NOTMERGESRC
            11 => Mix::Bool(0x9), // NOTXORSRC
            12 => Mix::Invert,
            13 => Mix::Bool(0xd), // MERGESRCNOT
            14 => Mix::Bool(0x3), // NOTCOPYSRC
            15 => Mix::Bool(0xb), // MERGENOTSRC
            16 => Mix::Bool(0x7), // NOTMASKSRC
            17 => Mix::Bool(0xf), // ONE
            _ => return None,
        })
    }

    /// FM_* code (0 = default = overpaint).
    pub fn from_fm(v: i32) -> Option<Mix> {
        if v == 0 { Some(Mix::Overpaint) } else { Mix::from_code(v) }
    }

    /// BM_* code (0 = default = leave alone; the transparency modes are
    /// treated as leave alone).
    pub fn from_bm(v: i32) -> Option<Mix> {
        match v {
            0 | 18 | 19 => Some(Mix::LeaveAlone),
            v => Mix::from_code(v),
        }
    }

    fn table(self) -> u8 {
        match self {
            Mix::Overpaint => 0xc,
            Mix::LeaveAlone => 0xa,
            Mix::Xor => 0x6,
            Mix::Or => 0xe,
            Mix::And => 0x8,
            Mix::Invert => 0x5,
            Mix::Bool(t) => t & 0xf,
        }
    }

    pub fn apply(self, dst: Rgb, src: Rgb) -> Rgb {
        match self {
            Mix::Overpaint => src,
            Mix::LeaveAlone => dst,
            m => {
                let t = m.table();
                let mut r = 0;
                for i in 0..4 {
                    if t & (1 << i) != 0 {
                        let s = if i & 2 != 0 { src } else { !src };
                        let d = if i & 1 != 0 { dst } else { !dst };
                        r |= s & d;
                    }
                }
                r & 0xff_ffff
            }
        }
    }
}

/// Colours and mixes of one primitive type. Colours are as the
/// application set them (indices or RGB, resolved through the colour
/// table); mixes are FM_*/BM_* codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bundle {
    pub color: i32,
    pub back: i32,
    pub mix: i32,
    pub back_mix: i32,
}

impl Default for Bundle {
    fn default() -> Self {
        Bundle { color: CLR_DEFAULT, back: CLR_BACKGROUND, mix: 0, back_mix: 0 }
    }
}

/// Primitive types (PRIM_*) for GpiSetAttrs.
pub const PRIM_LINE: i32 = 1;
pub const PRIM_CHAR: i32 = 2;
pub const PRIM_MARKER: i32 = 3;
pub const PRIM_AREA: i32 = 4;
pub const PRIM_IMAGE: i32 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attrs {
    pub line: Bundle,
    pub chr: Bundle,
    pub marker: Bundle,
    pub area: Bundle,
    pub image: Bundle,
    pub line_type: i32,
    /// Cosmetic width, FIXED (0 = default = 1 pel).
    pub line_width: i32,
    /// Geometric width (world units) for GpiStrokePath.
    pub line_width_geom: i32,
    pub line_end: i32,
    pub line_join: i32,
    pub pattern: i32,
    pub pattern_set: i32,
    pub pattern_ref: Point,
    pub marker_sym: i32,
    pub marker_set: i32,
    /// FIXED world units.
    pub marker_box: (i32, i32),
    /// FIXED world units.
    pub char_box: (i32, i32),
    pub char_angle: (i32, i32),
    pub char_shear: (i32, i32),
    pub char_set: i32,
    pub char_mode: i32,
    pub char_direction: i32,
    pub char_extra: i32,
    pub char_break_extra: i32,
    pub text_align: (i32, i32),
    /// Arc parameters P, Q, R, S.
    pub arc: [i32; 4],
}

impl Default for Attrs {
    fn default() -> Self {
        let f = font::system();
        Attrs {
            line: Bundle::default(),
            chr: Bundle::default(),
            marker: Bundle::default(),
            area: Bundle::default(),
            image: Bundle::default(),
            line_type: 0,
            line_width: 0,
            line_width_geom: 0,
            line_end: 0,
            line_join: 0,
            pattern: 0,
            pattern_set: 0,
            pattern_ref: Point::default(),
            marker_sym: 0,
            marker_set: 0,
            marker_box: (9 << 16, 9 << 16),
            char_box: (f.width << 16, f.height << 16),
            char_angle: (1, 0),
            char_shear: (0, 1),
            char_set: 0,
            char_mode: 0,
            char_direction: 0,
            char_extra: 0,
            char_break_extra: 0,
            text_align: (0, 0),
            arc: [1, 1, 0, 0],
        }
    }
}

/// What primitives are being collected into, instead of drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Build {
    None,
    Area(u32),
    Path,
}

/// A figure of an area or path, in device coordinates.
#[derive(Clone, Debug, Default)]
pub struct Figure {
    pub pts: Vec<Pf>,
    pub closed: bool,
}

/// Area fill source: a bit pattern (foreground/background colours) or a
/// colour pattern (from a bitmap pattern set).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fill {
    Bits(Pattern),
    Colors(Vec<Rgb>),
}

// Area/path options.
pub const BA_BOUNDARY: u32 = 1;
pub const BA_WINDING: u32 = 2;
pub const BA_EXCL: u32 = 8;
pub const DRO_FILL: i32 = 1;
pub const DRO_OUTLINE: i32 = 2;

/// Coordinate spaces of GpiConvert (CVTC_*).
pub const CVTC_WORLD: i32 = 1;
pub const CVTC_MODEL: i32 = 2;
pub const CVTC_DEFAULTPAGE: i32 = 3;
pub const CVTC_PAGE: i32 = 4;
pub const CVTC_DEVICE: i32 = 5;

/// Logical fonts: whether the selected font is scaled to the character
/// box (outline fonts) or drawn at its pixel size (image fonts), and the
/// simulated styles of its FATTRS fsSelection (italic slanted, bold one
/// pel wider, underscored).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogFont {
    pub outline: bool,
    pub italic: bool,
    pub bold: bool,
    pub underscore: bool,
}

/// The slant of simulated italics (x per y).
const ITALIC_SLANT: f64 = 0.25;

pub struct Ps {
    pub color: Rgb,
    pub back_color: Rgb,
    pub mix: Mix,
    pub back_mix: Mix,
    /// Current position (world coordinates).
    pub pos: Point,
    /// Surface position of the device origin (the window's bottom-left).
    pub origin: Point,
    /// Effective clip in surface coordinates.
    pub clip: Vec<Rect>,
    pub font: &'static Font,
    pub attrs: Attrs,
    pub colors: ColorTable,
    /// Attribute stack (GpiPop) and mode (AM_PRESERVE 0, AM_NOPRESERVE 1).
    pub attr_mode: i32,
    attr_stack: Vec<Attrs>,
    pub model: Matrix,
    /// Viewing transform of the segment being drawn
    /// (GpiSetViewingTransformMatrix), between model and default view.
    pub viewing: Matrix,
    pub view: Matrix,
    /// Default attribute values (GpiSetDefAttrs): what a reset restores.
    pub defaults: Attrs,
    /// Page to device (page units / page viewport).
    pub page: Matrix,
    pub page_units: u32,
    pub page_size: (i32, i32),
    xf: Matrix,
    /// Visible region in surface coordinates.
    vis: Vec<Rect>,
    /// Application clip region and clip path, in device coordinates.
    pub clip_region: Option<Region>,
    pub clip_path: Option<Region>,
    build: Build,
    figs: Vec<Figure>,
    cur: Vec<Pf>,
    path: Option<Vec<Figure>>,
    /// Pels drawn along the current line run (for line types).
    style_pos: u32,
    pub logfont: LogFont,
    /// Pattern from a bitmap set by GpiSetPatternSet.
    pub pattern_bitmap: Option<Fill>,
    /// Accumulated boundary of drawing (device), when enabled.
    pub bounds: Option<Rect>,
    pub collect_bounds: bool,
    /// DCTL_DISPLAY off: draw nothing (bounds are still collected).
    pub display: bool,
}

impl Ps {
    pub fn new(origin: Point, clip: Vec<Rect>) -> Ps {
        Ps {
            color: 0x000000,
            back_color: 0xffffff,
            mix: Mix::Overpaint,
            back_mix: Mix::LeaveAlone,
            pos: Point::default(),
            origin,
            vis: clip.clone(),
            clip,
            font: font::system(),
            attrs: Attrs::default(),
            colors: ColorTable::default(),
            attr_mode: 1,
            attr_stack: Vec::new(),
            model: Matrix::IDENTITY,
            viewing: Matrix::IDENTITY,
            view: Matrix::IDENTITY,
            defaults: Attrs::default(),
            page: Matrix::IDENTITY,
            page_units: xform::PU_PELS,
            page_size: (0, 0),
            xf: Matrix::IDENTITY,
            clip_region: None,
            clip_path: None,
            build: Build::None,
            figs: Vec::new(),
            cur: Vec::new(),
            path: None,
            style_pos: 0,
            logfont: LogFont::default(),
            pattern_bitmap: None,
            bounds: None,
            collect_bounds: false,
            display: true,
        }
    }

    // ---- Simple drawing for the window manager ----

    fn device(&self, r: Rect) -> Rect {
        r.offset(self.origin.x, self.origin.y)
    }

    /// Fill a page rectangle with a colour (overpaint).
    pub fn fill_rect(&self, s: &mut Surface, r: Rect, c: Rgb) {
        let d = self.device(r);
        for cr in &self.clip {
            s.fill(d.intersect(cr), c);
        }
    }

    /// Plot one device point with a mix.
    fn plot(&self, s: &mut Surface, x: i32, y: i32, c: Rgb, mix: Mix) {
        let (dx, dy) = (x + self.origin.x, y + self.origin.y);
        let p = Point::new(dx, dy);
        if !self.clip.iter().any(|r| r.contains(p)) {
            return;
        }
        match mix {
            Mix::Overpaint => s.set(dx, dy, c),
            Mix::LeaveAlone => {}
            m => {
                if let Some(old) = s.get(dx, dy) {
                    s.set(dx, dy, m.apply(old, c));
                }
            }
        }
    }

    pub fn move_to(&mut self, p: Point) {
        self.pos = p;
    }

    /// Line from the current position to `p` (both ends drawn), which
    /// becomes the current position.
    pub fn line_to(&mut self, s: &mut Surface, p: Point) {
        let (c, m) = (self.color, self.mix);
        if let Some(win) = self.stroke_window(1) {
            raster::line_clip((self.pos.x, self.pos.y), (p.x, p.y), win, |_, x, y| self.plot(s, x, y, c, m));
        }
        self.pos = p;
    }

    /// Rectangle from the current position to the opposite corner `p`
    /// (inclusive), outlined and/or filled; the current position stays.
    pub fn draw_box(&mut self, s: &mut Surface, p: Point, outline: bool, fill: bool) {
        let a = self.pos;
        let r = Rect::new(a.x.min(p.x), a.y.min(p.y), a.x.max(p.x) + 1, a.y.max(p.y) + 1);
        if fill {
            self.fill_rect(s, r, self.color);
        }
        if outline {
            let corners = [Point::new(r.right - 1, r.bottom), Point::new(r.right - 1, r.top - 1), Point::new(r.left, r.top - 1), Point::new(r.left, r.bottom)];
            self.pos = Point::new(r.left, r.bottom);
            for c in corners {
                self.line_to(s, c);
            }
        }
        self.pos = a;
    }

    /// Draw `text` with the baseline starting at `p` (device coordinates);
    /// the current position moves to the end of the string.
    pub fn char_string_at(&mut self, s: &mut Surface, p: Point, text: &[u8]) {
        self.glyphs_at(s, p, text, self.color, self.mix, self.back_color, self.back_mix);
        self.pos = Point::new(p.x + self.font.text_width(text), p.y);
    }

    #[allow(clippy::too_many_arguments)]
    fn glyphs_at(&self, s: &mut Surface, p: Point, text: &[u8], c: Rgb, mix: Mix, back: Rgb, back_mix: Mix) {
        let f = self.font;
        let bottom = p.y - f.descender;
        if back_mix != Mix::LeaveAlone {
            let r = Rect::new(p.x, bottom, p.x + f.text_width(text), bottom + f.height);
            if back_mix == Mix::Overpaint {
                self.fill_rect(s, r, back);
            } else {
                for y in r.bottom..r.top {
                    for x in r.left..r.right {
                        self.plot(s, x, y, back, back_mix);
                    }
                }
            }
        }
        let mut x0 = p.x;
        for &b in text {
            let w = f.char_width(b);
            for row in 0..f.height {
                let y = bottom + f.height - 1 - row;
                for col in 0..w {
                    if f.pixel(b, row, col) {
                        self.plot(s, x0 + col, y, c, mix);
                    }
                }
            }
            x0 += w;
        }
    }

    // ---- Visible region and clipping ----

    /// Set the window origin and visible region (surface rectangles).
    pub fn set_vis(&mut self, origin: Point, vis: Vec<Rect>) {
        self.origin = origin;
        self.vis = vis;
        self.refresh_clip();
    }

    pub fn vis(&self) -> &[Rect] {
        &self.vis
    }

    /// Recompute the effective clip after a change.
    pub fn refresh_clip(&mut self) {
        if self.clip_region.is_none() && self.clip_path.is_none() {
            self.clip = self.vis.clone();
            return;
        }
        let mut g = Region::from_rects(&self.vis);
        for c in [&self.clip_region, &self.clip_path].into_iter().flatten() {
            let mut c = c.clone();
            c.offset(self.origin.x, self.origin.y);
            g = g.combine(&c, Op::And);
        }
        self.clip = g.rects();
    }

    pub fn set_clip_region(&mut self, r: Option<Region>) {
        self.clip_region = r;
        self.refresh_clip();
    }

    /// The effective clip region in device coordinates.
    pub fn clip_device(&self) -> Region {
        let mut g = Region::from_rects(&self.clip);
        g.offset(-self.origin.x, -self.origin.y);
        g
    }

    /// GpiIntersectClipRectangle / GpiExcludeClipRectangle (world rect).
    pub fn clip_rect(&mut self, r: Rect, exclude: bool) {
        let d = self.world_rect_to_device(r);
        let base = self.clip_region.take().unwrap_or_else(|| {
            let mut g = Region::from_rects(&self.vis);
            g.offset(-self.origin.x, -self.origin.y);
            g
        });
        let r = Region::from_rect(d);
        self.clip_region = Some(base.combine(&r, if exclude { Op::Diff } else { Op::And }));
        self.refresh_clip();
    }

    // ---- Transforms ----

    fn update_xf(&mut self) {
        self.xf = self.model.then(&self.viewing).then(&self.view).then(&self.page);
    }

    pub fn world_to_device(&self) -> Matrix {
        self.xf
    }

    pub fn set_model(&mut self, m: Matrix) {
        self.model = m;
        self.update_xf();
    }

    pub fn set_view(&mut self, m: Matrix) {
        self.view = m;
        self.update_xf();
    }

    pub fn set_viewing(&mut self, m: Matrix) {
        self.viewing = m;
        self.update_xf();
    }

    /// Page units and size (GpiCreatePS / GpiSetPS).
    pub fn set_page(&mut self, units: u32, size: (i32, i32)) {
        self.page_units = units;
        self.page_size = size;
        let k = xform::page_unit_scale(units);
        self.page = Matrix::scale(k, k);
        self.update_xf();
    }

    /// GpiSetPageViewport: map the page to `r` (device).
    pub fn set_page_viewport(&mut self, r: Rect) -> bool {
        let (cx, cy) = self.page_size;
        if cx <= 0 || cy <= 0 || r.width() == 0 || r.height() == 0 {
            return false;
        }
        self.page = Matrix::scale(r.width() as f64 / cx as f64, r.height() as f64 / cy as f64).then(&Matrix::translate(r.left as f64, r.bottom as f64));
        self.update_xf();
        true
    }

    pub fn page_viewport(&self) -> Rect {
        let (x0, y0) = self.page.apply(0.0, 0.0);
        let (x1, y1) = self.page.apply(self.page_size.0 as f64, self.page_size.1 as f64);
        Rect::new(round(x0), round(y0), round(x1), round(y1))
    }

    /// Matrix from world coordinates to a CVTC_* space.
    fn to_space(&self, space: i32) -> Option<Matrix> {
        Some(match space {
            CVTC_WORLD => Matrix::IDENTITY,
            CVTC_MODEL => self.model,
            CVTC_DEFAULTPAGE | CVTC_PAGE => self.model.then(&self.viewing).then(&self.view),
            CVTC_DEVICE => self.xf,
            _ => return None,
        })
    }

    /// GpiConvert.
    pub fn convert(&self, from: i32, to: i32, pts: &mut [Point]) -> bool {
        let (Some(a), Some(b)) = (self.to_space(from), self.to_space(to)) else { return false };
        let Some(ai) = a.invert() else { return false };
        let m = ai.then(&b);
        for p in pts.iter_mut() {
            let (x, y) = m.apply(p.x as f64, p.y as f64);
            *p = Point::new(round(x), round(y));
        }
        true
    }

    fn dev(&self, p: Point) -> Pf {
        self.xf.apply(p.x as f64, p.y as f64)
    }

    /// A world vector (relative) in device units.
    fn dev_vec(&self, x: f64, y: f64) -> Pf {
        (x * self.xf.m11 + y * self.xf.m21, x * self.xf.m12 + y * self.xf.m22)
    }

    /// Device point back to world (rounded).
    fn world(&self, p: Pf) -> Point {
        match self.xf.invert() {
            Some(i) => {
                let (x, y) = i.apply(p.0, p.1);
                Point::new(round(x), round(y))
            }
            None => self.pos,
        }
    }

    /// World rectangle (inclusive corners) to a device rectangle.
    pub fn world_rect_to_device(&self, r: Rect) -> Rect {
        let pts = [self.dev(Point::new(r.left, r.bottom)), self.dev(Point::new(r.right, r.top))];
        let (x0, x1) = (round(pts[0].0.min(pts[1].0)), round(pts[0].0.max(pts[1].0)));
        let (y0, y1) = (round(pts[0].1.min(pts[1].1)), round(pts[0].1.max(pts[1].1)));
        Rect::new(x0, y0, x1, y1)
    }

    // ---- Attributes ----

    /// Save the attributes before a change (AM_PRESERVE).
    pub fn will_change_attrs(&mut self) {
        if self.attr_mode == 0 {
            self.attr_stack.push(self.attrs.clone());
        }
    }

    /// GpiPop.
    pub fn pop_attrs(&mut self, n: i32) -> bool {
        if n < 0 || n as usize > self.attr_stack.len() {
            return false;
        }
        for _ in 0..n {
            if let Some(a) = self.attr_stack.pop() {
                self.attrs = a;
            }
        }
        self.sync_simple();
        true
    }

    /// Keep the simple drawing fields in step with the line/char bundle.
    pub fn sync_simple(&mut self) {
        let b = self.attrs.chr;
        self.color = self.colors.rgb(b.color);
        self.back_color = self.colors.rgb(b.back);
        self.mix = Mix::from_fm(b.mix).unwrap_or(Mix::Overpaint);
        self.back_mix = Mix::from_bm(b.back_mix).unwrap_or(Mix::LeaveAlone);
    }

    pub fn bundle_mut(&mut self, prim: i32) -> Option<&mut Bundle> {
        Some(match prim {
            PRIM_LINE => &mut self.attrs.line,
            PRIM_CHAR => &mut self.attrs.chr,
            PRIM_MARKER => &mut self.attrs.marker,
            PRIM_AREA => &mut self.attrs.area,
            PRIM_IMAGE => &mut self.attrs.image,
            _ => return None,
        })
    }

    /// Set a field of every bundle (GpiSetColor etc.).
    pub fn set_all(&mut self, f: impl Fn(&mut Bundle)) {
        for b in [&mut self.attrs.line, &mut self.attrs.chr, &mut self.attrs.marker, &mut self.attrs.area, &mut self.attrs.image] {
            f(b);
        }
        self.sync_simple();
    }

    fn rgb(&self, c: i32) -> Rgb {
        self.colors.rgb(c)
    }

    /// Foreground colour and mix of a bundle; background colour and mix.
    fn resolve(&self, b: &Bundle) -> (Rgb, Mix, Rgb, Mix) {
        (self.rgb(b.color), Mix::from_fm(b.mix).unwrap_or(Mix::Overpaint), self.rgb(b.back), Mix::from_bm(b.back_mix).unwrap_or(Mix::LeaveAlone))
    }

    /// GpiResetPS: attributes, transforms, clipping, colours.
    pub fn reset(&mut self, all: bool) {
        self.attrs = self.defaults.clone();
        self.attr_stack.clear();
        self.pos = Point::default();
        if all {
            self.colors.reset();
            self.colors.palette = None;
            self.model = Matrix::IDENTITY;
            self.viewing = Matrix::IDENTITY;
            self.view = Matrix::IDENTITY;
            self.defaults = Attrs::default();
            self.attrs = Attrs::default();
            let (u, sz) = (self.page_units, self.page_size);
            self.set_page(u, sz);
            self.clip_region = None;
            self.clip_path = None;
            self.build = Build::None;
            self.figs.clear();
            self.cur.clear();
            self.path = None;
            self.pattern_bitmap = None;
            self.logfont = LogFont::default();
            self.font = font::system();
            self.refresh_clip();
        }
        self.sync_simple();
    }

    // ---- Pixel output ----

    fn note_bounds(&mut self, r: Rect) {
        if self.collect_bounds && !r.is_empty() {
            self.bounds = Some(match self.bounds {
                Some(b) => Rect::new(b.left.min(r.left), b.bottom.min(r.bottom), b.right.max(r.right), b.top.max(r.top)),
                None => r,
            });
        }
    }

    fn fill_source(&self) -> Fill {
        if let Some(f) = &self.pattern_bitmap {
            return f.clone();
        }
        Fill::Bits(pattern::standard(self.attrs.pattern).unwrap_or([0xff; 8]))
    }

    /// Paint a device region with a bundle's colours and the area pattern
    /// (`solid` ignores the pattern).
    pub fn paint(&mut self, s: &mut Surface, g: &Region, b: Bundle, solid: bool) {
        self.note_bounds(g.bounds());
        if !self.display {
            return;
        }
        let (fg, mix, bg, bmix) = self.resolve(&b);
        let fill = if solid { Fill::Bits([0xff; 8]) } else { self.fill_source() };
        let pref = self.attrs.pattern_ref;
        let org = (pref.x + self.origin.x, pref.y + self.origin.y);
        let mut dr = g.clone();
        dr.offset(self.origin.x, self.origin.y);
        for r in dr.rects() {
            for c in &self.clip {
                let r = r.intersect(c).intersect(&s.bounds());
                if r.is_empty() {
                    continue;
                }
                if fill == Fill::Bits([0xff; 8]) && mix == Mix::Overpaint {
                    s.fill(r, fg);
                    continue;
                }
                s.map(r, |x, y, d| match &fill {
                    Fill::Bits(p) => {
                        if pattern::bit(p, x, y, org) {
                            mix.apply(d, fg)
                        } else {
                            bmix.apply(d, bg)
                        }
                    }
                    Fill::Colors(c) => {
                        let (i, j) = ((x - org.0).rem_euclid(8) as usize, 7 - (y - org.1).rem_euclid(8) as usize);
                        mix.apply(d, c[j * 8 + i])
                    }
                });
            }
        }
    }

    /// Draw a polyline (device coordinates) with the line attributes.
    fn stroke(&mut self, s: &mut Surface, pts: &[Pf], closed: bool) {
        if pts.is_empty() {
            return;
        }
        let (fg, mix, bg, bmix) = self.resolve(&self.attrs.line);
        let lt = self.attrs.line_type;
        if lt == 8 {
            return;
        }
        let w = match self.attrs.line_width {
            w if w <= 0 => 1,
            w => round(fx(w)).max(1),
        };
        let n = pts.len();
        let segs = if closed && n > 2 { n } else { n.saturating_sub(1) };
        // Steps are only visited inside the clip (widened by the pen), so
        // far-off coordinates cost nothing; the dash position still
        // advances over the whole line and the bounds cover it.
        let win = if self.display { self.stroke_window(w) } else { None };
        let mut pos = self.style_pos as u64;
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        let put = |ps: &Ps, s: &mut Surface, pos: u64, x: i32, y: i32| {
            let on = pattern::dash_on(lt, pos as u32);
            let (c, m) = if on { (fg, mix) } else { (bg, bmix) };
            if m == Mix::LeaveAlone {
                return;
            }
            if w == 1 {
                ps.plot(s, x, y, c, m);
            } else {
                let o = (w - 1) / 2;
                for dy in 0..w {
                    for dx in 0..w {
                        ps.plot(s, x - o + dx, y - o + dy, c, m);
                    }
                }
            }
        };
        if segs == 0 {
            let (x, y) = ipt(pts[0]);
            (lo, hi) = ((x, y), (x, y));
            if let Some(win) = win {
                raster::line_clip((x, y), (x, y), win, |_, x, y| put(self, s, pos, x, y));
            }
            pos += 1;
        }
        for i in 0..segs {
            let (a, b) = (ipt(pts[i]), ipt(pts[(i + 1) % n]));
            for p in [a, b] {
                lo = (lo.0.min(p.0), lo.1.min(p.1));
                hi = (hi.0.max(p.0), hi.1.max(p.1));
            }
            // Later segments start on the previous one's last pixel.
            let skip = (i > 0) as u64;
            let steps = match win {
                Some(win) => raster::line_clip(a, b, win, |k, x, y| {
                    if k >= skip {
                        put(self, s, pos + k - skip, x, y);
                    }
                }),
                None => raster::line_clip(a, b, [0, 0, -1, -1], |_, _, _| {}),
            };
            pos += steps - skip;
        }
        self.style_pos = pos as u32;
        if lo.0 <= hi.0 {
            self.note_bounds(Rect::new(lo.0, lo.1, hi.0.saturating_add(1), hi.1.saturating_add(1)));
        }
    }

    /// The device-coordinate window (inclusive) a pen `w` pels wide can
    /// touch, or None when the clip is empty.
    fn stroke_window(&self, w: i32) -> Option<[i64; 4]> {
        if self.clip.is_empty() {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for r in &self.clip {
            x0 = x0.min(r.left as i64);
            y0 = y0.min(r.bottom as i64);
            x1 = x1.max(r.right as i64 - 1);
            y1 = y1.max(r.top as i64 - 1);
        }
        let (ox, oy, w) = (self.origin.x as i64, self.origin.y as i64, w as i64);
        Some([x0 - ox - w, y0 - oy - w, x1 - ox + w, y1 - oy + w])
    }

    // ---- Figure collection (areas and paths) ----

    fn building(&self) -> bool {
        self.build != Build::None
    }

    fn flush_fig(&mut self) {
        if self.cur.len() > 1 {
            let closed = matches!(self.build, Build::Area(_));
            self.figs.push(Figure { pts: core::mem::take(&mut self.cur), closed });
        }
        self.cur.clear();
    }

    /// Continue the current figure (or draw) with device points from the
    /// current position.
    fn emit(&mut self, s: &mut Surface, pts: &[Pf]) {
        let start = self.dev(self.pos);
        if self.building() {
            if self.cur.is_empty() {
                self.cur.push(start);
            }
            self.cur.extend_from_slice(pts);
        } else {
            let mut v = Vec::with_capacity(pts.len() + 1);
            v.push(start);
            v.extend_from_slice(pts);
            self.stroke(s, &v, false);
        }
    }

    /// A closed shape: added as its own figure, or filled and outlined.
    fn shape(&mut self, s: &mut Surface, pts: Vec<Pf>, ctl: i32) {
        if self.building() {
            self.flush_fig();
            self.figs.push(Figure { pts, closed: true });
            return;
        }
        if ctl & DRO_FILL != 0 {
            let g = raster::fill_region(&[pts.clone()], false, true);
            let b = self.attrs.area;
            self.paint(s, &g, b, false);
        }
        if ctl & DRO_OUTLINE != 0 || ctl & 3 == 0 {
            self.style_pos = 0;
            self.stroke(s, &pts, true);
        }
    }

    // ---- GPI primitives ----

    /// GpiMove / GpiSetCurrentPosition.
    pub fn gpi_move(&mut self, p: Point) {
        if self.building() {
            self.flush_fig();
        }
        self.pos = p;
        self.style_pos = 0;
    }

    /// GpiLine.
    pub fn gpi_line(&mut self, s: &mut Surface, p: Point) {
        let d = self.dev(p);
        self.emit(s, &[d]);
        self.pos = p;
    }

    /// GpiPolyLine.
    pub fn gpi_polyline(&mut self, s: &mut Surface, pts: &[Point]) {
        let Some(&last) = pts.last() else { return };
        let d: Vec<Pf> = pts.iter().map(|&p| self.dev(p)).collect();
        self.emit(s, &d);
        self.pos = last;
    }

    /// GpiPolyLineDisjoint: separate lines between pairs of points.
    pub fn gpi_polyline_disjoint(&mut self, s: &mut Surface, pts: &[Point]) {
        for pair in pts.chunks(2) {
            if let [a, b] = pair {
                self.gpi_move(*a);
                self.gpi_line(s, *b);
            }
        }
    }

    /// GpiSetPel: one pel at a world point with the line colour and mix.
    pub fn gpi_set_pel(&mut self, s: &mut Surface, p: Point) {
        let (x, y) = ipt(self.dev(p));
        let (fg, mix, _, _) = self.resolve(&self.attrs.line);
        self.note_bounds(Rect::new(x, y, x + 1, y + 1));
        if self.display {
            self.plot(s, x, y, fg, mix);
        }
    }

    /// GpiBox: rectangle from the current position to `p`, with corners
    /// rounded by an ellipse of full axes `hr` x `vr`.
    pub fn gpi_box(&mut self, s: &mut Surface, ctl: i32, p: Point, hr: i32, vr: i32) {
        let a = self.pos;
        let (x0, x1) = (a.x.min(p.x) as f64, a.x.max(p.x) as f64);
        let (y0, y1) = (a.y.min(p.y) as f64, a.y.max(p.y) as f64);
        let rx = (abs(hr as f64) / 2.0).min((x1 - x0) / 2.0);
        let ry = (abs(vr as f64) / 2.0).min((y1 - y0) / 2.0);
        let mut w: Vec<Pf> = Vec::new();
        if rx > 0.0 && ry > 0.0 {
            let corners = [(x1 - rx, y0 + ry, -PI / 2.0), (x1 - rx, y1 - ry, 0.0), (x0 + rx, y1 - ry, PI / 2.0), (x0 + rx, y0 + ry, PI)];
            for (cx, cy, st) in corners {
                // Corner arcs in device space for a smooth curve.
                let c = self.xf.apply(cx, cy);
                let u = self.dev_vec(rx, 0.0);
                let v = self.dev_vec(0.0, ry);
                raster::arc_points(c, u, v, st, PI / 2.0, &mut w);
            }
        } else {
            w = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)].into_iter().map(|(x, y)| self.xf.apply(x, y)).collect();
        }
        self.shape(s, w, ctl);
        self.pos = a;
    }

    /// Arc parameter vectors scaled by a FIXED multiplier, in device units.
    fn arc_vectors(&self, mult: i32) -> (Pf, Pf) {
        let [p, q, r, sv] = self.attrs.arc;
        let m = fx(mult);
        (self.dev_vec(p as f64 * m, sv as f64 * m), self.dev_vec(r as f64 * m, q as f64 * m))
    }

    /// GpiFullArc around the current position.
    pub fn gpi_full_arc(&mut self, s: &mut Surface, ctl: i32, mult: i32) {
        let c = self.dev(self.pos);
        let (u, v) = self.arc_vectors(mult);
        let mut pts = Vec::new();
        raster::arc_points(c, u, v, 0.0, 2.0 * PI, &mut pts);
        pts.pop();
        let pos = self.pos;
        self.shape(s, pts, ctl);
        self.pos = pos;
    }

    /// GpiPartialArc: a line from the current position to the start of
    /// the arc, then the arc; angles in FIXED degrees.
    pub fn gpi_partial_arc(&mut self, s: &mut Surface, center: Point, mult: i32, start: i32, sweep: i32) {
        let c = self.dev(center);
        let (u, v) = self.arc_vectors(mult);
        let (st, sw) = (fx(start) * PI / 180.0, fx(sweep) * PI / 180.0);
        let mut pts = Vec::new();
        raster::arc_points(c, u, v, st, sw, &mut pts);
        let end = *pts.last().unwrap_or(&c);
        self.emit(s, &pts);
        self.pos = self.world(end);
    }

    /// GpiPointArc: from the current position through `p1` to `p2`.
    pub fn gpi_point_arc(&mut self, s: &mut Surface, p1: Point, p2: Point) {
        let [p, q, r, sv] = self.attrs.arc;
        // Arc-parameter space -> world: (x, y) -> (P x + R y, S x + Q y).
        let a = Matrix { m11: p as f64, m12: sv as f64, m21: r as f64, m22: q as f64, m31: 0.0, m32: 0.0 };
        let p0 = self.pos;
        let line = |ps: &mut Ps, s: &mut Surface| {
            let d = [ps.dev(p1), ps.dev(p2)];
            ps.emit(s, &d);
        };
        let Some(ai) = a.invert() else {
            line(self, s);
            self.pos = p2;
            return;
        };
        let to_u = |pt: Point| ai.apply(pt.x as f64, pt.y as f64);
        let (u0, u1, u2) = (to_u(p0), to_u(p1), to_u(p2));
        match raster::circle3(u0, u1, u2) {
            Some((o, rad)) => {
                let (st, sw) = raster::arc3_angles(o, u0, u1, u2);
                let mut up = Vec::new();
                raster::arc_points(o, (rad, 0.0), (0.0, rad), st, sw, &mut up);
                let pts: Vec<Pf> = up.iter().map(|&(x, y)| {
                    let (wx, wy) = a.apply(x, y);
                    self.xf.apply(wx, wy)
                }).collect();
                self.emit(s, &pts[1..]);
            }
            None => line(self, s),
        }
        self.pos = p2;
    }

    /// GpiPolySpline: cubic Béziers, three points each.
    pub fn gpi_poly_spline(&mut self, s: &mut Surface, pts: &[Point]) {
        let mut out = Vec::new();
        let mut p0 = self.dev(self.pos);
        for c in pts.chunks(3) {
            if let [a, b, e] = c {
                let (a, b, e) = (self.dev(*a), self.dev(*b), self.dev(*e));
                raster::bezier(p0, a, b, e, &mut out);
                p0 = e;
            }
        }
        self.emit(s, &out);
        let n = pts.len() / 3 * 3;
        if n > 0 {
            self.pos = pts[n - 1];
        }
    }

    /// GpiPolyFillet.
    pub fn gpi_poly_fillet(&mut self, s: &mut Surface, pts: &[Point]) {
        let Some(&last) = pts.last() else { return };
        let mut d = vec![self.dev(self.pos)];
        d.extend(pts.iter().map(|&p| self.dev(p)));
        let mut out = Vec::new();
        raster::fillet(&d, &mut out);
        self.emit(s, &out);
        self.pos = last;
    }

    /// GpiMarker / GpiPolyMarker.
    pub fn gpi_markers(&mut self, s: &mut Surface, pts: &[Point]) {
        let Some(m) = pattern::marker(self.attrs.marker_sym) else { return };
        let (fg, mix, _, _) = self.resolve(&self.attrs.marker);
        for &p in pts {
            let (cx, cy) = ipt(self.dev(p));
            self.note_bounds(Rect::new(cx - 4, cy - 4, cx + 5, cy + 5));
            if !self.display {
                continue;
            }
            for (j, row) in m.iter().enumerate() {
                for (i, &b) in row.iter().enumerate() {
                    if b == b'#' {
                        self.plot(s, cx - 4 + i as i32, cy + 4 - j as i32, fg, mix);
                    }
                }
            }
        }
        if let Some(&l) = pts.last() {
            self.pos = l;
        }
    }

    // ---- Areas ----

    /// GpiBeginArea.
    pub fn begin_area(&mut self, flags: u32) -> bool {
        if self.building() {
            return false;
        }
        self.build = Build::Area(flags);
        self.figs.clear();
        self.cur.clear();
        true
    }

    /// GpiEndArea: fill the collected figures.
    pub fn end_area(&mut self, s: &mut Surface) -> bool {
        let Build::Area(flags) = self.build else { return false };
        self.flush_fig();
        self.build = Build::None;
        let figs = core::mem::take(&mut self.figs);
        let polys: Vec<Vec<Pf>> = figs.iter().map(|f| f.pts.clone()).collect();
        let g = raster::fill_region(&polys, flags & BA_WINDING != 0, flags & BA_EXCL == 0);
        let b = self.attrs.area;
        self.paint(s, &g, b, false);
        if flags & BA_BOUNDARY != 0 {
            for f in &polys {
                self.style_pos = 0;
                self.stroke(s, f, true);
            }
        }
        true
    }

    // ---- Paths ----

    pub fn begin_path(&mut self) -> bool {
        if self.building() {
            return false;
        }
        self.build = Build::Path;
        self.figs.clear();
        self.cur.clear();
        true
    }

    /// GpiCloseFigure.
    pub fn close_figure(&mut self) -> bool {
        if self.build != Build::Path {
            return false;
        }
        if self.cur.len() > 1 {
            let pts = core::mem::take(&mut self.cur);
            if let Some(&first) = pts.first() {
                // The current position becomes the figure's start.
                self.pos = self.world(first);
            }
            self.figs.push(Figure { pts, closed: true });
        }
        true
    }

    pub fn end_path(&mut self) -> bool {
        if self.build != Build::Path {
            return false;
        }
        self.flush_fig();
        self.build = Build::None;
        self.path = Some(core::mem::take(&mut self.figs));
        true
    }

    /// The path as a region (device), consuming it.
    pub fn path_region(&mut self, winding: bool, incl: bool) -> Option<Region> {
        let p = self.path.take()?;
        let polys: Vec<Vec<Pf>> = p.into_iter().map(|f| f.pts).collect();
        Some(raster::fill_region(&polys, winding, incl))
    }

    /// GpiFillPath (FPATH_WINDING 2, FPATH_EXCL 8).
    pub fn fill_path(&mut self, s: &mut Surface, opts: i32) -> bool {
        let Some(g) = self.path_region(opts & 2 != 0, opts & 8 == 0) else { return false };
        let b = self.attrs.area;
        self.paint(s, &g, b, false);
        true
    }

    /// GpiOutlinePath: cosmetic lines along the path.
    pub fn outline_path(&mut self, s: &mut Surface) -> bool {
        let Some(p) = self.path.take() else { return false };
        for f in &p {
            self.style_pos = 0;
            self.stroke(s, &f.pts, f.closed);
        }
        true
    }

    /// GpiStrokePath: the path widened to the geometric line width.
    pub fn stroke_path(&mut self, s: &mut Surface) -> bool {
        let Some(p) = self.path.take() else { return false };
        let (dx, dy) = self.dev_vec(self.attrs.line_width_geom as f64, 0.0);
        let w = sqrt(dx * dx + dy * dy);
        if w <= 1.5 {
            for f in &p {
                self.style_pos = 0;
                self.stroke(s, &f.pts, f.closed);
            }
            return true;
        }
        let mut polys = Vec::new();
        for f in &p {
            raster::wide_stroke(&f.pts, f.closed, w, &mut polys);
        }
        let g = raster::fill_region(&polys, true, false);
        let b = self.attrs.line;
        self.paint(s, &g, b, true);
        true
    }

    /// GpiSetClipPath: 0 resets; otherwise the path becomes (or with
    /// SCP_AND is intersected with) the clip path.
    pub fn set_clip_path(&mut self, id: i32, opts: i32) -> bool {
        if id == 0 {
            self.clip_path = None;
            self.refresh_clip();
            return true;
        }
        let Some(g) = self.path_region(opts & 2 != 0, opts & 8 == 0) else { return false };
        self.clip_path = Some(match (opts & 4 != 0, self.clip_path.take()) {
            (true, Some(old)) => old.combine(&g, Op::And),
            _ => g,
        });
        self.refresh_clip();
        true
    }

    // ---- Text ----

    /// Glyph scale (device pels per font pel): 1 for image fonts, the
    /// character box for outline fonts.
    fn text_scale(&self) -> (f64, f64) {
        if !self.logfont.outline {
            return (1.0, 1.0);
        }
        let (bx, by) = self.dev_vec(fx(self.attrs.char_box.0), 0.0);
        let (cx, cy) = self.dev_vec(0.0, fx(self.attrs.char_box.1));
        (sqrt(bx * bx + by * by) / self.font.width as f64, sqrt(cx * cx + cy * cy) / self.font.height as f64)
    }

    /// Horizontal offset of glyph pels per pel above the baseline: the
    /// character shear (outline fonts; GpiSetCharShear) and italics.
    fn slant(&self) -> f64 {
        let mut k = if self.logfont.italic { ITALIC_SLANT } else { 0.0 };
        let (x, y) = self.attrs.char_shear;
        if self.logfont.outline && x != 0 && y != 0 {
            k += x as f64 / y as f64;
        }
        k
    }

    /// Advance of `n` characters plus extra spacing, in device pels.
    fn advance(&self, text: &[u8], dx: Option<&[i32]>) -> Vec<f64> {
        let (sx, _) = self.text_scale();
        let (ex, _) = self.dev_vec(fx(self.attrs.char_extra), 0.0);
        let (bx, _) = self.dev_vec(fx(self.attrs.char_break_extra), 0.0);
        text.iter()
            .enumerate()
            .map(|(i, &c)| match dx.and_then(|d| d.get(i)) {
                Some(&w) => self.dev_vec(w as f64, 0.0).0,
                None => self.font.char_width(c) as f64 * sx + ex + if c == b' ' { bx } else { 0.0 },
            })
            .collect()
    }

    /// GpiCharStringAt / GpiCharStringPos: text with its baseline at `p`
    /// (world); `dx` optionally gives per-character increments (world).
    /// `opaque` fills `rect` (world, or the text box) with the background
    /// first; `clip` clips to `rect`.
    #[allow(clippy::too_many_arguments)]
    pub fn gpi_text(&mut self, s: &mut Surface, p: Point, text: &[u8], dx: Option<&[i32]>, rect: Option<Rect>, opaque: bool, clip_to: bool) {
        let start = self.dev(p);
        let adv = self.advance(text, dx);
        let total: f64 = adv.iter().sum();
        let (sx, sy) = self.text_scale();
        let f = self.font;
        let saved_clip = if clip_to && let Some(r) = rect {
            let saved = self.clip.clone();
            let d = self.world_rect_to_device(r);
            let d = Rect::new(d.left, d.bottom, d.right + 1, d.top + 1).offset(self.origin.x, self.origin.y);
            self.clip = self.clip.iter().map(|c| c.intersect(&d)).filter(|c| !c.is_empty()).collect();
            Some(saved)
        } else {
            None
        };
        let (fg, mix, bg, bmix) = self.resolve(&self.attrs.chr);
        let bottom = start.1 - f.descender as f64 * sy;
        let text_rect = Rect::new(round(start.0), round(bottom), round(start.0 + total), round(bottom + f.height as f64 * sy));
        let slant = self.slant();
        let lf = self.logfont;
        if self.building() {
            self.flush_fig();
            let mut x = start.0;
            for (i, &b) in text.iter().enumerate() {
                for row in 0..f.height {
                    for col in 0..f.char_width(b) {
                        if f.pixel(b, row, col) {
                            let up = (f.height - 1 - row - f.descender) as f64 * sy;
                            let (x0, y0) = (x + col as f64 * sx + up * slant, bottom + (f.height - 1 - row) as f64 * sy);
                            self.figs.push(Figure { pts: vec![(x0, y0), (x0 + sx, y0), (x0 + sx, y0 + sy), (x0, y0 + sy)], closed: true });
                        }
                    }
                }
                x += adv[i];
            }
        } else {
            self.note_bounds(text_rect);
            if self.display {
                if opaque {
                    let r = rect.map_or(text_rect, |r| {
                        let d = self.world_rect_to_device(r);
                        Rect::new(d.left, d.bottom, d.right + 1, d.top + 1)
                    });
                    self.fill_rect(s, r, bg);
                }
                let plain = slant == 0.0 && !lf.bold && !lf.underscore;
                if plain && sx == 1.0 && sy == 1.0 && dx.is_none() && self.attrs.char_extra == 0 && self.attrs.char_break_extra == 0 {
                    let bm = if opaque { Mix::LeaveAlone } else { bmix };
                    self.glyphs_at(s, Point::new(round(start.0), round(start.1)), text, fg, mix, bg, bm);
                } else {
                    // The character cells' background (BM_OVERPAINT etc.),
                    // as for plain text.
                    if !opaque && bmix != Mix::LeaveAlone {
                        if bmix == Mix::Overpaint {
                            self.fill_rect(s, text_rect, bg);
                        } else {
                            for y in text_rect.bottom..text_rect.top {
                                for x in text_rect.left..text_rect.right {
                                    self.plot(s, x, y, bg, bmix);
                                }
                            }
                        }
                    }
                    let mut spans = crate::region::Spans::new();
                    let mut x = start.0;
                    let wide = lf.bold as i32;
                    for (i, &b) in text.iter().enumerate() {
                        for row in 0..f.height {
                            let y0 = round(bottom + (f.height - 1 - row) as f64 * sy);
                            let y1 = round(bottom + (f.height - row) as f64 * sy);
                            for col in 0..f.char_width(b) {
                                if f.pixel(b, row, col) {
                                    let (x0, x1) = (x + col as f64 * sx, x + (col + 1) as f64 * sx);
                                    for y in y0..y1.max(y0 + 1) {
                                        let off = (y as f64 + 0.5 - start.1) * slant;
                                        let (a, b) = (round(x0 + off), round(x1 + off));
                                        spans.add(y, a, b.max(a + 1) + wide);
                                    }
                                }
                            }
                        }
                        x += adv[i];
                    }
                    if lf.underscore {
                        let y = round(start.1) - 1 - (sy as i32 - 1).max(0) / 2;
                        for t in 0..round(sy).max(1) {
                            spans.add(y - t, round(start.0), round(start.0 + total));
                        }
                    }
                    let g = spans.into_region();
                    let b = Bundle { back_mix: 0, ..self.attrs.chr };
                    self.paint(s, &g, b, true);
                }
            }
        }
        if let Some(c) = saved_clip {
            self.clip = c;
        }
        self.pos = self.world((start.0 + total, start.1));
    }

    /// GpiQueryTextBox: corners relative to the start point (world):
    /// top-left, bottom-left, top-right, bottom-right, concatenation point.
    pub fn text_box(&self, text: &[u8]) -> [Point; 5] {
        let (_, sy) = self.text_scale();
        let total: f64 = self.advance(text, None).iter().sum();
        let f = self.font;
        let asc = (f.height - f.descender) as f64 * sy;
        let desc = f.descender as f64 * sy;
        let inv = self.xf.invert().unwrap_or(Matrix::IDENTITY);
        let w = |x: f64, y: f64| {
            let (a, b) = (x * inv.m11 + y * inv.m21, x * inv.m12 + y * inv.m22);
            Point::new(round(a), round(b))
        };
        [w(0.0, asc), w(0.0, -desc), w(total, asc), w(total, -desc), w(total, 0.0)]
    }

    /// Positions of each character (world) for GpiQueryCharStringPos:
    /// n + 1 points starting at `p`.
    pub fn char_positions(&self, p: Point, text: &[u8], dx: Option<&[i32]>) -> Vec<Point> {
        let start = self.dev(p);
        let mut x = start.0;
        let mut v = vec![p];
        for a in self.advance(text, dx) {
            x += a;
            v.push(self.world((x, start.1)));
        }
        v
    }

    /// World size of one device pel vector (for font metrics).
    pub fn device_to_world_len(&self, dx: f64, dy: f64) -> (i32, i32) {
        let inv = self.xf.invert().unwrap_or(Matrix::IDENTITY);
        (round(abs(dx * inv.m11 + dy * inv.m21)), round(abs(dx * inv.m12 + dy * inv.m22)))
    }

    /// Font metrics scale for the current font (device pels per font pel).
    pub fn font_scale(&self) -> (f64, f64) {
        self.text_scale()
    }

    // ---- Regions and erasing ----

    /// GpiPaintRegion: a device region with the area attributes.
    pub fn paint_region(&mut self, s: &mut Surface, g: &Region) {
        let b = self.attrs.area;
        self.paint(s, g, b, false);
    }

    /// GpiFrameRegion: the inside border of a region, `t` pels thick.
    pub fn frame_region(&mut self, s: &mut Surface, g: &Region, t: (i32, i32)) {
        let mut inner = g.clone();
        for (dx, dy) in [(t.0, 0), (-t.0, 0), (0, t.1), (0, -t.1)] {
            let mut o = g.clone();
            o.offset(dx, dy);
            inner = inner.combine(&o, Op::And);
        }
        let frame = g.combine(&inner, Op::Diff);
        let b = self.attrs.area;
        self.paint(s, &frame, b, false);
    }

    // ---- Bit-block transfers ----

    /// Transfer `src` samples (w x h, bottom row first; None = outside
    /// the source) to the device rectangle `t` with a ternary ROP. Mono
    /// conversion: `mono_src` maps a monochrome source (white = 1) to the
    /// image foreground/background colours; `mono_dst` makes the target
    /// monochrome (pels equal to `src_back` become 0).
    #[allow(clippy::too_many_arguments)]
    pub fn blit(&mut self, s: &mut Surface, t: Rect, src: Option<(&[Option<Rgb>], i32, i32)>, rop: u8, mono_src: bool, mono_dst: Option<Rgb>) {
        self.note_bounds(t);
        if !self.display || t.is_empty() {
            return;
        }
        let (ifg, _, ibg, _) = self.resolve(&self.attrs.image);
        let (pfg, _, pbg, _) = self.resolve(&self.attrs.area);
        let fill = self.fill_source();
        let pref = self.attrs.pattern_ref;
        let org = (pref.x + self.origin.x, pref.y + self.origin.y);
        let dt = t.offset(self.origin.x, self.origin.y);
        let uses_p = bitmap::rop_uses_pattern(rop);
        for c in &self.clip {
            let r = dt.intersect(c).intersect(&s.bounds());
            if r.is_empty() {
                continue;
            }
            s.map(r, |x, y, d| {
                let sv = match src {
                    Some((px, w, h)) => {
                        let (i, j) = (x - dt.left, y - dt.bottom);
                        let k = (j * w + i) as usize;
                        if i >= w || j >= h {
                            return d;
                        }
                        match px.get(k).copied().flatten() {
                            Some(v) => {
                                let v = if mono_src { if v & 0x808080 != 0 { ifg } else { ibg } } else { v };
                                match mono_dst {
                                    Some(back) => if v == back { 0 } else { 0xffffff },
                                    None => v,
                                }
                            }
                            None => return d,
                        }
                    }
                    None => 0,
                };
                let pv = if !uses_p {
                    0
                } else {
                    match &fill {
                        Fill::Bits(p) => if pattern::bit(p, x, y, org) { pfg } else { pbg },
                        Fill::Colors(c) => {
                            let (i, j) = ((x - org.0).rem_euclid(8) as usize, 7 - (y - org.1).rem_euclid(8) as usize);
                            c[j * 8 + i]
                        }
                    }
                };
                bitmap::rop3(rop, pv, sv, d)
            });
        }
    }

    /// GpiErase: clear the whole device rectangle `r` to the reset colour.
    pub fn erase(&mut self, s: &mut Surface, r: Rect) {
        let c = if self.colors.rgb_mode { 0xffffff } else { self.colors.rgb(CLR_BACKGROUND) };
        if self.display {
            self.fill_rect(s, r, c);
        }
    }

    /// Bitmap pattern (GpiSetPatternSet with a bitmap's lcid).
    pub fn set_pattern_bitmap(&mut self, bm: Option<&Bitmap>) {
        self.pattern_bitmap = bm.map(|b| {
            let get = |i: i32, j: i32| b.pixels.get(((j % b.height) * b.width + (i % b.width)) as usize).copied().unwrap_or(0);
            if b.is_mono() {
                let mut p = [0u8; 8];
                for (j, row) in p.iter_mut().enumerate() {
                    for i in 0..8 {
                        if get(i, j as i32) & 0x808080 != 0 {
                            *row |= 0x80 >> i;
                        }
                    }
                }
                Fill::Bits(p)
            } else {
                Fill::Colors((0..64).map(|k| get(k % 8, k / 8)).collect())
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn surface(buf: &mut Vec<u32>, w: i32, h: i32) -> Surface {
        unsafe { Surface::new(buf.as_mut_ptr(), w, h, w, false) }
    }

    fn count(s: &Surface, c: Rgb) -> usize {
        let mut n = 0;
        for y in 0..s.height {
            for x in 0..s.width {
                if s.get(x, y) == Some(c) {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn fill_is_clipped() {
        let mut buf = vec![0u32; 100];
        let mut s = surface(&mut buf, 10, 10);
        let ps = Ps::new(Point::new(2, 2), vec![Rect::new(0, 0, 5, 5)]);
        ps.fill_rect(&mut s, Rect::new(0, 0, 10, 10), 0xff0000);
        assert_eq!(s.get(2, 2), Some(0xff0000));
        assert_eq!(s.get(4, 4), Some(0xff0000));
        assert_eq!(s.get(5, 5), Some(0));
        assert_eq!(s.get(1, 1), Some(0));
    }

    #[test]
    fn device_y_is_bottom_up() {
        let mut buf = vec![0u32; 4];
        let mut s = surface(&mut buf, 2, 2);
        s.set(0, 0, 0x123456);
        // Bottom-left pixel is the first pixel of the last row in memory.
        assert_eq!(buf[2], 0x123456);
    }

    #[test]
    fn text_draws_glyph_pixels() {
        let mut buf = vec![0xffffffu32; 64 * 32];
        let mut s = surface(&mut buf, 64, 32);
        let mut ps = Ps::new(Point::new(0, 0), vec![s.bounds()]);
        ps.char_string_at(&mut s, Point::new(0, 8), b"H");
        let black = buf.iter().filter(|&&p| p == 0).count();
        assert!(black > 10, "glyph drawn");
        assert_eq!(ps.pos, Point::new(8, 8));
    }

    #[test]
    fn line_endpoints() {
        let mut buf = vec![0u32; 100];
        let mut s = surface(&mut buf, 10, 10);
        let mut ps = Ps::new(Point::new(0, 0), vec![s.bounds()]);
        ps.color = 0xffffff;
        ps.move_to(Point::new(1, 1));
        ps.line_to(&mut s, Point::new(8, 5));
        assert_eq!(s.get(1, 1), Some(0xffffff));
        assert_eq!(s.get(8, 5), Some(0xffffff));
    }

    fn setup(w: i32, h: i32) -> (Vec<u32>, Ps) {
        let buf = vec![0xffffffu32; (w * h) as usize];
        let ps = Ps::new(Point::new(0, 0), vec![Rect::new(0, 0, w, h)]);
        (buf, ps)
    }

    #[test]
    fn mixes() {
        assert_eq!(Mix::from_fm(9), Some(Mix::Bool(0)));
        assert_eq!(Mix::from_fm(12), Some(Mix::Invert));
        assert_eq!(Mix::Invert.apply(0x00ff00, 0x123456), 0xff00ff);
        assert_eq!(Mix::Bool(0x2).apply(0xffffff, 0x00ff00), 0xff00ff);
        assert_eq!(Mix::Xor.apply(0xff0000, 0xffff00), 0x00ff00);
        assert_eq!(Mix::from_bm(0), Some(Mix::LeaveAlone));
    }

    #[test]
    fn box_inclusive_and_rounded() {
        let (mut buf, mut ps) = setup(40, 40);
        let mut s = surface(&mut buf, 40, 40);
        ps.set_all(|b| b.color = crate::color::CLR_RED);
        ps.gpi_move(Point::new(5, 5));
        ps.gpi_box(&mut s, 1, Point::new(14, 14), 0, 0);
        assert_eq!(count(&s, 0xff0000), 100);
        assert_eq!(ps.pos, Point::new(5, 5));
        // Rounded: the corner pel is left out.
        let (mut buf, mut ps) = setup(40, 40);
        let mut s = surface(&mut buf, 40, 40);
        ps.gpi_move(Point::new(5, 5));
        ps.gpi_box(&mut s, 3, Point::new(30, 30), 10, 10);
        assert_eq!(s.get(5, 5), Some(0xffffff));
        assert_eq!(s.get(17, 17), Some(0));
        assert_eq!(s.get(5, 17), Some(0));
    }

    #[test]
    fn full_arc_and_params() {
        let (mut buf, mut ps) = setup(60, 60);
        let mut s = surface(&mut buf, 60, 60);
        ps.gpi_move(Point::new(30, 30));
        ps.attrs.arc = [2, 1, 0, 0];
        ps.gpi_full_arc(&mut s, DRO_OUTLINEFILL_TEST, 10 << 16);
        // Ellipse 20 x 10 radii: area ~ pi * 200.
        let n = count(&s, 0) as f64;
        assert!((n - PI * 200.0).abs() < 60.0, "{n}");
        assert_eq!(s.get(49, 30), Some(0));
        assert_eq!(s.get(30, 39), Some(0));
        assert_eq!(s.get(30, 43), Some(0xffffff));
    }

    const DRO_OUTLINEFILL_TEST: i32 = 3;

    #[test]
    fn area_alternate_and_winding() {
        for (flags, hole) in [(0u32, true), (BA_WINDING, false)] {
            let (mut buf, mut ps) = setup(30, 30);
            let mut s = surface(&mut buf, 30, 30);
            ps.begin_area(flags);
            ps.gpi_move(Point::new(2, 2));
            ps.gpi_polyline(&mut s, &[Point::new(27, 2), Point::new(27, 27), Point::new(2, 27)]);
            ps.gpi_move(Point::new(10, 10));
            ps.gpi_polyline(&mut s, &[Point::new(19, 10), Point::new(19, 19), Point::new(10, 19)]);
            // Nothing drawn until the area ends.
            assert_eq!(count(&s, 0), 0);
            assert!(ps.end_area(&mut s));
            assert_eq!(s.get(5, 5), Some(0));
            assert_eq!(s.get(15, 15) == Some(0xffffff), hole);
        }
    }

    #[test]
    fn patterns_fill() {
        let (mut buf, mut ps) = setup(16, 16);
        let mut s = surface(&mut buf, 16, 16);
        ps.attrs.pattern = 10; // HORIZ: one row in four
        ps.gpi_move(Point::new(0, 0));
        ps.gpi_box(&mut s, 1, Point::new(15, 15), 0, 0);
        assert_eq!(count(&s, 0), 64);
    }

    #[test]
    fn line_types_and_width() {
        let (mut buf, mut ps) = setup(40, 10);
        let mut s = surface(&mut buf, 40, 10);
        ps.attrs.line_type = 1; // DOT: 1 on 2 off
        ps.gpi_move(Point::new(0, 5));
        ps.gpi_line(&mut s, Point::new(29, 5));
        assert_eq!(count(&s, 0), 10);
        let (mut buf, mut ps) = setup(40, 10);
        let mut s = surface(&mut buf, 40, 10);
        ps.attrs.line_width = 2 << 16;
        ps.gpi_move(Point::new(0, 5));
        ps.gpi_line(&mut s, Point::new(9, 5));
        assert_eq!(count(&s, 0), 22);
    }

    /// A line from far-off garbage coordinates (Pulse's bad layout) is
    /// clipped before stepping: it finishes at once and draws what a short
    /// line across the surface would, dashes in step, bounds unclipped.
    #[test]
    fn huge_line_is_clipped() {
        let (a, b) = (Point::new(-2147482763, 5), Point::new(1879049159, 5));
        for (lt, width) in [(0, 0), (1, 0), (0, 3 << 16)] {
            let (mut buf, mut ps) = setup(40, 10);
            let mut s = surface(&mut buf, 40, 10);
            ps.attrs.line_type = lt;
            ps.attrs.line_width = width;
            ps.collect_bounds = true;
            ps.gpi_move(a);
            ps.gpi_polyline(&mut s, &[b, Point::new(b.x, -100000)]);
            assert_eq!(ps.bounds, Some(Rect::new(a.x, -100000, b.x + 1, 6)));
            // The same dash phase: -2147482763 = -2 (mod 3).
            let (mut buf2, mut ps2) = setup(40, 10);
            let mut s2 = surface(&mut buf2, 40, 10);
            ps2.attrs.line_type = lt;
            ps2.attrs.line_width = width;
            ps2.gpi_move(Point::new(-2, 5));
            ps2.gpi_line(&mut s2, Point::new(45, 5));
            assert_eq!(buf, buf2, "line type {lt}, width {width}");
            assert!(buf.iter().any(|&p| p == 0));
        }
        let (mut buf, mut ps) = setup(40, 10);
        let mut s = surface(&mut buf, 40, 10);
        ps.move_to(Point::new(i32::MIN, i32::MIN));
        ps.line_to(&mut s, Point::new(i32::MAX, i32::MAX));
        assert_eq!(s.get(7, 7), Some(0));
    }

    #[test]
    fn transforms_and_convert() {
        let (mut buf, mut ps) = setup(100, 100);
        let mut s = surface(&mut buf, 100, 100);
        ps.set_view(Matrix::scale(2.0, 2.0).then(&Matrix::translate(10.0, 0.0)));
        ps.gpi_move(Point::new(0, 0));
        ps.gpi_box(&mut s, 1, Point::new(9, 9), 0, 0);
        assert_eq!(s.get(10, 0), Some(0));
        assert_eq!(s.get(28, 18), Some(0));
        assert_eq!(s.get(9, 0), Some(0xffffff));
        let mut p = [Point::new(5, 5)];
        assert!(ps.convert(CVTC_WORLD, CVTC_DEVICE, &mut p));
        assert_eq!(p[0], Point::new(20, 10));
        assert!(ps.convert(CVTC_DEVICE, CVTC_WORLD, &mut p));
        assert_eq!(p[0], Point::new(5, 5));
        // Page units: LOMETRIC (0.1 mm) at 96 dpi.
        ps.set_view(Matrix::IDENTITY);
        ps.set_page(xform::PU_LOMETRIC, (1000, 1000));
        let mut p = [Point::new(1000, 0)];
        ps.convert(CVTC_WORLD, CVTC_DEVICE, &mut p);
        assert_eq!(p[0], Point::new(378, 0));
    }

    #[test]
    fn clip_path_from_path() {
        let (mut buf, mut ps) = setup(40, 40);
        let mut s = surface(&mut buf, 40, 40);
        ps.begin_path();
        ps.gpi_move(Point::new(10, 10));
        ps.gpi_box(&mut s, 2, Point::new(19, 19), 0, 0);
        ps.end_path();
        assert!(ps.set_clip_path(1, 4));
        ps.fill_rect(&mut s, Rect::new(0, 0, 40, 40), 0x0000ff);
        assert_eq!(count(&s, 0x0000ff), 100);
        assert!(ps.set_clip_path(0, 0));
        assert_eq!(ps.clip, vec![Rect::new(0, 0, 40, 40)]);
    }

    #[test]
    fn fill_and_stroke_path() {
        let (mut buf, mut ps) = setup(40, 40);
        let mut s = surface(&mut buf, 40, 40);
        ps.begin_path();
        ps.gpi_move(Point::new(5, 20));
        ps.gpi_line(&mut s, Point::new(35, 20));
        ps.end_path();
        ps.attrs.line_width_geom = 6;
        assert!(ps.stroke_path(&mut s));
        assert_eq!(s.get(20, 22), Some(0));
        assert_eq!(s.get(20, 17), Some(0));
        assert_eq!(s.get(20, 25), Some(0xffffff));
        assert!(!ps.fill_path(&mut s, 0), "path consumed");
    }

    #[test]
    fn splines_and_arcs_move_position() {
        let (mut buf, mut ps) = setup(60, 60);
        let mut s = surface(&mut buf, 60, 60);
        ps.gpi_move(Point::new(5, 5));
        ps.gpi_poly_spline(&mut s, &[Point::new(5, 50), Point::new(50, 50), Point::new(50, 5)]);
        assert_eq!(ps.pos, Point::new(50, 5));
        assert_eq!(s.get(50, 5), Some(0));
        ps.gpi_move(Point::new(10, 30));
        ps.gpi_point_arc(&mut s, Point::new(30, 50), Point::new(50, 30));
        assert_eq!(ps.pos, Point::new(50, 30));
        assert_eq!(s.get(30, 50), Some(0));
        ps.gpi_move(Point::new(0, 0));
        ps.gpi_partial_arc(&mut s, Point::new(30, 30), 10 << 16, 0, 90 << 16);
        assert_eq!(ps.pos, Point::new(30, 40));
        assert_eq!(s.get(40, 30), Some(0));
        ps.gpi_move(Point::new(5, 55));
        ps.gpi_poly_fillet(&mut s, &[Point::new(30, 55), Point::new(55, 55)]);
        assert_eq!(s.get(30, 55), Some(0));
    }

    #[test]
    fn markers() {
        let (mut buf, mut ps) = setup(20, 20);
        let mut s = surface(&mut buf, 20, 20);
        ps.attrs.marker_sym = 8; // solid square
        ps.gpi_markers(&mut s, &[Point::new(10, 10)]);
        assert_eq!(count(&s, 0), 81);
    }

    #[test]
    fn text_in_path_and_box() {
        let (mut buf, mut ps) = setup(64, 64);
        let mut s = surface(&mut buf, 64, 64);
        ps.logfont.outline = true;
        ps.attrs.char_box = (16 << 16, 32 << 16);
        let tb = ps.text_box(b"AB");
        assert_eq!(tb[4], Point::new(32, 0));
        assert_eq!(tb[2], Point::new(32, 24));
        ps.begin_path();
        ps.gpi_move(Point::new(0, 10));
        ps.gpi_text(&mut s, Point::new(0, 10), b"I", None, None, false, false);
        ps.end_path();
        assert!(ps.set_clip_path(1, 4));
        ps.fill_rect(&mut s, Rect::new(0, 0, 64, 64), 0x00ff00);
        let n = count(&s, 0x00ff00);
        assert!(n > 40, "{n}");
        assert_eq!(ps.pos, Point::new(16, 10));
    }

    #[test]
    fn text_with_increments_paints_cell_background() {
        // EPM's status line: GpiCharStringPosAt with CHS_VECTOR and
        // BM_OVERPAINT must erase what was drawn there before.
        let (mut buf, mut ps) = setup(64, 32);
        let mut s = surface(&mut buf, 64, 32);
        ps.fill_rect(&mut s, Rect::new(0, 0, 64, 32), 0x123456);
        ps.attrs.chr.back_mix = 2;
        ps.attrs.chr.back = -2;
        let dx = [8, 8, 8];
        ps.gpi_text(&mut s, Point::new(0, 10), b"   ", Some(&dx), None, false, false);
        assert_eq!(s.get(2, 10), Some(0xffffff));
        assert_eq!(s.get(23, 12), Some(0xffffff));
        assert_eq!(s.get(30, 12), Some(0x123456));
    }

    #[test]
    fn blit_rops() {
        let (mut buf, mut ps) = setup(8, 8);
        let mut s = surface(&mut buf, 8, 8);
        let src: Vec<Option<Rgb>> = (0..16).map(|i| Some(if i % 2 == 0 { 0xff0000 } else { 0x0000ff })).collect();
        ps.blit(&mut s, Rect::new(2, 2, 6, 6), Some((&src, 4, 4)), 0xcc, false, None);
        assert_eq!(s.get(2, 2), Some(0xff0000));
        assert_eq!(s.get(3, 2), Some(0x0000ff));
        assert_eq!(s.get(6, 6), Some(0xffffff));
        ps.blit(&mut s, Rect::new(0, 0, 8, 8), None, 0x55, false, None);
        assert_eq!(s.get(0, 0), Some(0));
        assert_eq!(s.get(2, 2), Some(0x00ffff));
        ps.blit(&mut s, Rect::new(0, 0, 8, 8), None, 0x00, false, None);
        assert_eq!(count(&s, 0), 64);
    }

    #[test]
    fn attr_stack() {
        let (_, mut ps) = setup(4, 4);
        ps.attr_mode = 0;
        ps.will_change_attrs();
        ps.set_all(|b| b.color = 2);
        ps.will_change_attrs();
        ps.attrs.area.color = 4;
        assert!(ps.pop_attrs(1));
        assert_eq!(ps.attrs.area.color, 2);
        assert!(ps.pop_attrs(1));
        assert_eq!(ps.attrs.area.color, CLR_DEFAULT);
        assert!(!ps.pop_attrs(1));
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn clip_region_with_origin() {
        let mut buf = vec![0xffffffu32; 200 * 200];
        let mut s = unsafe { Surface::new(buf.as_mut_ptr(), 200, 200, 200, false) };
        let mut ps = Ps::new(Point::new(50, 50), vec![Rect::new(50, 50, 150, 150), Rect::new(0, 0, 10, 10)]);
        ps.set_clip_region(Some(Region::from_rect(Rect::new(10, 10, 40, 40))));
        assert_eq!(ps.clip, vec![Rect::new(60, 60, 90, 90)]);
        ps.gpi_move(Point::new(0, 20));
        ps.gpi_line(&mut s, Point::new(99, 20));
        assert_eq!(s.get(70, 70), Some(0));
        assert_eq!(s.get(55, 70), Some(0xffffff));
    }
}
