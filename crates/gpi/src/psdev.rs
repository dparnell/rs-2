//! A vector device for presentation spaces: [`Sink`] receives what the
//! GPI draws as device-level shapes (filled figures, polylines,
//! rectangles, images, glyph runs) instead of pels, and [`PsWriter`]
//! turns them into a DSC-conforming PostScript document (rs2's
//! PostScript printer driver, docs/PRINTING.md).
//!
//! Coordinates are device pels (y up, origin bottom left), as the GPI
//! has them; the page's CTM scales them to points (72 / dpi) and turns
//! landscape pages. Text uses Type 3 fonts made from the glyphs the GPI
//! draws with: outlines (font units) for outline fonts, imagemasks for
//! image fonts; each glyph is defined once, the first time it is shown,
//! and runs are shown with `xshow` at the GPI's own advances. Mixes are
//! applied against white paper (FM_XOR of black prints white...);
//! leave-alone draws nothing.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::color::Rgb;
use crate::font::{Font, Outlines};
use crate::raster::Pf;
use crate::ttf::Op;
use crate::Rect;

/// How an area is painted.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Rgb),
    /// An 8x8 bit pattern (rows top first, bit 7 leftmost): ones in `fg`,
    /// zeros in `bg` (None: left alone); `org` is the pattern's origin
    /// (device pels).
    Pattern { bits: [u8; 8], fg: Rgb, bg: Option<Rgb>, org: (i32, i32) },
    /// An 8x8 colour pattern (rows top first).
    Colors { cells: Vec<Rgb>, org: (i32, i32) },
}

/// The glyphs of a run: an outline face (its glyphs in font units) or an
/// image font (pels).
#[derive(Clone, Copy)]
pub enum Face {
    Outline { outlines: &'static dyn Outlines, bold: f64 },
    Image(&'static Font),
}

impl Face {
    /// What identifies the face's Type 3 font.
    fn key(&self) -> usize {
        match self {
            Face::Outline { outlines, bold } => (*outlines as *const dyn Outlines as *const u8 as usize) ^ ((*bold * 1000.0) as usize).rotate_left(20),
            Face::Image(f) => *f as *const Font as usize,
        }
    }
}

/// A run of characters on one baseline.
pub struct GlyphRun<'a> {
    pub face: Face,
    /// Code page 850 characters.
    pub codes: &'a [u8],
    /// Pen position of each character (device x).
    pub xs: &'a [f64],
    /// Baseline (device y).
    pub y: f64,
    /// Glyph space (font units or pels) to device: x' = a x + c y,
    /// y' = b x + d y.
    pub matrix: [f64; 4],
    pub color: Rgb,
}

/// Where a presentation space's drawing goes when it is not drawn into
/// pels. `clip` is the clip in device rectangles (empty: draw nothing).
pub trait Sink {
    fn fill(&mut self, clip: &[Rect], figs: &[Vec<Pf>], winding: bool, paint: &Paint);
    /// A polyline `width` pels wide; `dash` (pels, on/off) when styled.
    fn stroke(&mut self, clip: &[Rect], pts: &[Pf], closed: bool, width: f64, color: Rgb, dash: &[f64]);
    fn rects(&mut self, clip: &[Rect], rects: &[Rect], paint: &Paint);
    /// An image of `w` x `h` samples (rows bottom first; None is
    /// transparent) filling `dst`.
    fn image(&mut self, clip: &[Rect], dst: Rect, w: i32, h: i32, px: &[Option<Rgb>]);
    fn glyphs(&mut self, clip: &[Rect], run: &GlyphRun);
    /// Downcast support for the device context that owns the sink.
    fn as_any(&mut self) -> &mut dyn core::any::Any;
}

/// Forms (paper sizes) in points.
pub const FORMS: &[(&str, i32, i32)] = &[("Letter", 612, 792), ("A4", 595, 842), ("Legal", 612, 1008), ("A3", 842, 1191), ("A5", 420, 595), ("Executive", 522, 756)];

/// A number as PostScript text: integers plainly, else two decimals.
fn num(s: &mut String, x: f64) {
    let r = crate::math::round(x * 100.0);
    if r % 100 == 0 {
        let _ = write!(s, "{}", r / 100);
    } else {
        let neg = r < 0;
        let a = r.unsigned_abs();
        let frac = a % 100;
        if neg {
            s.push('-');
        }
        if frac % 10 == 0 {
            let _ = write!(s, "{}.{}", a / 100, frac / 10);
        } else {
            let _ = write!(s, "{}.{:02}", a / 100, frac);
        }
    }
}

fn nums(s: &mut String, xs: &[f64]) {
    for (i, &x) in xs.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        num(s, x);
    }
}

/// PostScript string literal of bytes.
fn ps_string(s: &mut String, b: &[u8]) {
    s.push('(');
    for &c in b {
        match c {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(c as char);
            }
            0x20..=0x7e => s.push(c as char),
            _ => {
                let _ = write!(s, "\\{:03o}", c);
            }
        }
    }
    s.push(')');
}

/// RunLengthDecode's format.
pub fn run_length(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len() / 2 + 16);
    let mut i = 0;
    while i < d.len() {
        // A run of equal bytes?
        let mut j = i + 1;
        while j < d.len() && d[j] == d[i] && j - i < 128 {
            j += 1;
        }
        if j - i >= 3 {
            out.push((257 - (j - i)) as u8);
            out.push(d[i]);
            i = j;
            continue;
        }
        // Literal bytes up to the next run of three.
        let start = i;
        while i < d.len() && i - start < 128 {
            if i + 2 < d.len() && d[i] == d[i + 1] && d[i] == d[i + 2] {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&d[start..i]);
    }
    out.push(128);
    out
}

/// ASCII85, with its `~>` end, in lines of at most 75 characters.
pub fn ascii85(d: &[u8], out: &mut String) {
    let mut col = 0;
    let mut put = |c: char, out: &mut String| {
        out.push(c);
        col += 1;
        if col == 75 {
            out.push('\n');
            col = 0;
        }
    };
    for chunk in d.chunks(4) {
        let mut b = [0u8; 4];
        b[..chunk.len()].copy_from_slice(chunk);
        let mut v = u32::from_be_bytes(b);
        if chunk.len() == 4 && v == 0 {
            put('z', out);
            continue;
        }
        let mut c = [0u8; 5];
        for k in (0..5).rev() {
            c[k] = (v % 85) as u8 + b'!';
            v /= 85;
        }
        for &x in &c[..chunk.len() + 1] {
            put(x as char, out);
        }
    }
    out.push_str("~>\n");
}

struct FontState {
    key: usize,
    id: u32,
    defined: [bool; 256],
}

/// A PostScript document being written: [`PsWriter::take`] hands over
/// what is ready (a whole page at a time).
pub struct PsWriter {
    out: String,
    pub dpi: i32,
    /// Paper size in points.
    pub paper: (i32, i32),
    pub landscape: bool,
    pub copies: u32,
    pub title: Vec<u8>,
    pages: u32,
    doc: bool,
    page_open: bool,
    /// What the graphics state has (None: unknown after a grestore).
    clip: Option<Vec<Rect>>,
    color: Option<Rgb>,
    width: Option<f64>,
    dash: Option<Vec<f64>>,
    fonts: Vec<FontState>,
    patterns: Vec<(Paint, u32)>,
    level3: bool,
}

impl PsWriter {
    pub fn new(dpi: i32, paper: (i32, i32), landscape: bool) -> PsWriter {
        PsWriter { out: String::new(), dpi, paper, landscape, copies: 1, title: Vec::new(), pages: 0, doc: false, page_open: false, clip: None, color: None, width: None, dash: None, fonts: Vec::new(), patterns: Vec::new(), level3: false }
    }

    /// The page in device pels (width, height as the page is drawn).
    pub fn page_pels(&self) -> (i32, i32) {
        let (w, h) = (self.paper.0 * self.dpi / 72, self.paper.1 * self.dpi / 72);
        if self.landscape { (h, w) } else { (w, h) }
    }

    /// The finished output so far.
    pub fn take(&mut self) -> Vec<u8> {
        let s = core::mem::take(&mut self.out);
        // One char per byte (the strings written are ASCII and Latin-1
        // escapes only).
        s.chars().map(|c| c as u32 as u8).collect()
    }

    pub fn pages(&self) -> u32 {
        self.pages
    }

    pub fn in_doc(&self) -> bool {
        self.doc
    }

    /// Start the document (the prolog and setup).
    pub fn begin_doc(&mut self) {
        if self.doc {
            return;
        }
        self.doc = true;
        self.pages = 0;
        let (w, h) = self.paper;
        let mut s = String::new();
        s.push_str("%!PS-Adobe-3.0\n%%Creator: rs2 PostScript driver\n%%Title: ");
        for &c in self.title.iter().filter(|c| (0x20..0x7f).contains(*c)) {
            s.push(c as char);
        }
        let _ = write!(s, "\n%%Pages: (atend)\n%%BoundingBox: 0 0 {w} {h}\n%%DocumentData: Clean7Bit\n%%LanguageLevel: 2\n%%Orientation: {}\n%%EndComments\n", if self.landscape { "Landscape" } else { "Portrait" });
        s.push_str("%%BeginProlog\n%%BeginResource: procset rs2 1.0 0\n/rs2dict 200 dict def rs2dict begin\n");
        s.push_str("/m {moveto} bind def /l {lineto} bind def /c {curveto} bind def /h {closepath} bind def /n {newpath} bind def\n");
        s.push_str("/f {fill} bind def /ef {eofill} bind def /s {stroke} bind def /rg {setrgbcolor} bind def /w {setlinewidth} bind def\n");
        s.push_str("/rf {rectfill} bind def /rc {n rectclip} bind def /gs {gsave} bind def /gr {grestore} bind def /d {setdash} bind def\n");
        s.push_str("/enc256 [");
        for i in 0..256 {
            let _ = write!(s, "/g{i}{}", if i % 16 == 15 { "\n" } else { " " });
        }
        s.push_str("] def\n");
        s.push_str("/mkfont {/GP exch def 8 dict begin /FontType 3 def /FontMatrix [1 0 0 1 0 0] def /FontBBox [0 0 0 0] def /Encoding enc256 def /GP GP def\n");
        s.push_str(" /BuildGlyph {exch /GP get exch 2 copy known {get exec} {pop pop 0 0 setcharwidth} ifelse} bind def\n");
        s.push_str(" /BuildChar {1 index /Encoding get exch get 1 index /BuildGlyph get exec} bind def currentdict end definefont pop} bind def\n");
        s.push_str("end\n%%EndResource\n%%EndProlog\n%%BeginSetup\n");
        let _ = write!(s, "%%BeginFeature: *PageSize\n{{<< /PageSize [{w} {h}] >> setpagedevice}} stopped pop\n%%EndFeature\n");
        if self.copies > 1 {
            let _ = write!(s, "{{<< /NumCopies {} >> setpagedevice}} stopped pop\n", self.copies);
        }
        s.push_str("%%EndSetup\n");
        self.out.push_str(&s);
    }

    fn begin_page(&mut self) {
        self.begin_doc();
        self.pages += 1;
        let n = self.pages;
        let _ = write!(self.out, "%%Page: {n} {n}\n%%BeginPageSetup\nrs2dict begin gs\n");
        let k = 72.0 / self.dpi as f64;
        let mut s = String::new();
        if self.landscape {
            // Device x runs up the paper, device y to the left.
            let _ = write!(s, "{} 0 translate 90 rotate ", self.paper.0);
        }
        num(&mut s, k);
        s.push(' ');
        num(&mut s, k);
        s.push_str(" scale 1 setlinecap 1 setlinejoin\n%%EndPageSetup\ngs\n");
        self.out.push_str(&s);
        self.page_open = true;
        self.forget();
    }

    fn forget(&mut self) {
        self.clip = None;
        self.color = None;
        self.width = None;
        self.dash = None;
    }

    /// DEVESC_NEWFRAME: finish the page (an empty one if nothing was
    /// drawn).
    pub fn new_frame(&mut self) {
        if !self.page_open {
            self.begin_page();
        }
        self.out.push_str("gr gr end showpage\n");
        self.page_open = false;
    }

    /// Finish the document; the page in progress is printed.
    pub fn end_doc(&mut self) {
        if self.page_open {
            self.new_frame();
        }
        if !self.doc {
            return;
        }
        let _ = write!(self.out, "%%Trailer\n%%Pages: {}\n%%EOF\n", self.pages);
        self.doc = false;
    }

    /// Raw printer data (DEVESC_RAWDATA): passed on as it is.
    pub fn raw(&mut self, d: &[u8]) {
        self.out.extend(d.iter().map(|&b| b as char));
    }

    /// Get ready to draw with `clip`; false when nothing would show.
    fn prepare(&mut self, clip: &[Rect]) -> bool {
        if clip.is_empty() {
            return false;
        }
        if !self.page_open {
            self.begin_page();
        }
        let (pw, ph) = self.page_pels();
        let whole = clip.iter().any(|r| r.left <= 0 && r.bottom <= 0 && r.right >= pw && r.top >= ph);
        let want: Vec<Rect> = if whole { Vec::new() } else { clip.to_vec() };
        if self.clip.as_ref() != Some(&want) {
            let mut s = String::from("gr gs");
            if !want.is_empty() {
                s.push_str(" [");
                for r in &want {
                    let _ = write!(s, "{} {} {} {} ", r.left, r.bottom, r.right - r.left, r.top - r.bottom);
                }
                s.push_str("] rc");
            }
            s.push('\n');
            self.out.push_str(&s);
            self.forget();
            self.clip = Some(want);
        }
        true
    }

    fn set_color(&mut self, c: Rgb) {
        if self.color == Some(c) {
            return;
        }
        let mut s = String::new();
        for sh in [16, 8, 0] {
            num(&mut s, ((c >> sh) & 0xff) as f64 / 255.0);
            s.push(' ');
        }
        s.push_str("rg\n");
        self.out.push_str(&s);
        self.color = Some(c);
    }

    fn path(&mut self, figs: &[Vec<Pf>], close: bool) {
        let mut s = String::from("n");
        for f in figs {
            for (i, &(x, y)) in f.iter().enumerate() {
                s.push(' ');
                num(&mut s, x);
                s.push(' ');
                num(&mut s, y);
                s.push_str(if i == 0 { " m" } else { " l" });
                if i % 8 == 7 {
                    s.push('\n');
                }
            }
            if close && f.len() > 1 {
                s.push_str(" h");
            }
        }
        s.push('\n');
        self.out.push_str(&s);
    }

    /// The pattern's name (defined on first use).
    fn pattern(&mut self, p: &Paint) -> u32 {
        if let Some((_, id)) = self.patterns.iter().find(|(q, _)| q == p) {
            return *id;
        }
        let id = self.patterns.len() as u32 + 1;
        let mut s = String::new();
        // In device space: the pattern cell is 8 x 8 device pels.
        match p {
            Paint::Pattern { bits, org, .. } => {
                let _ = write!(s, "/P{id} << /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8 /PaintProc {{pop 8 8 true [1 0 0 -1 0 8] <");
                for b in bits {
                    let _ = write!(s, "{:02x}", b);
                }
                let _ = write!(s, "> imagemask}} bind >> [1 0 0 1 {} {}] makepattern def\n", org.0, org.1);
            }
            Paint::Colors { cells, org } => {
                let _ = write!(s, "/P{id} << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8 /PaintProc {{pop 8 8 scale 8 8 8 [8 0 0 -8 0 8] <");
                for c in cells.iter().take(64) {
                    let _ = write!(s, "{:06x}", c & 0xff_ffff);
                }
                let _ = write!(s, "> false 3 colorimage}} bind >> [1 0 0 1 {} {}] makepattern def\n", org.0, org.1);
            }
            Paint::Solid(_) => {}
        }
        // Defined in rs2dict (not in the page's graphics state) so later
        // pages can use it.
        self.out.push_str("rs2dict begin ");
        self.out.push_str(&s);
        self.out.push_str("end\n");
        self.patterns.push((p.clone(), id));
        id
    }

    /// Paint the current path(s) built by `build` with `paint`.
    fn paint_with(&mut self, paint: &Paint, op: &str, build: &dyn Fn(&mut PsWriter)) {
        match paint {
            Paint::Solid(c) => {
                self.set_color(*c);
                build(self);
                self.out.push_str(op);
                self.out.push('\n');
            }
            Paint::Pattern { fg, bg, .. } => {
                if let Some(b) = bg {
                    self.set_color(*b);
                    build(self);
                    self.out.push_str(op);
                    self.out.push('\n');
                }
                let id = self.pattern(paint);
                let mut s = String::from("gs [/Pattern /DeviceRGB] setcolorspace ");
                for sh in [16, 8, 0] {
                    num(&mut s, ((fg >> sh) & 0xff) as f64 / 255.0);
                    s.push(' ');
                }
                let _ = write!(s, "P{id} setcolor\n");
                self.out.push_str(&s);
                build(self);
                self.out.push_str(op);
                self.out.push_str(" gr\n");
            }
            Paint::Colors { .. } => {
                let id = self.pattern(paint);
                let _ = write!(self.out, "gs P{id} setpattern\n");
                build(self);
                self.out.push_str(op);
                self.out.push_str(" gr\n");
            }
        }
    }

    /// The Type 3 font for a face (made on first use).
    fn font_for(&mut self, face: &Face) -> usize {
        let key = face.key();
        if let Some(i) = self.fonts.iter().position(|f| f.key == key) {
            return i;
        }
        let id = self.fonts.len() as u32 + 1;
        let _ = write!(self.out, "rs2dict begin /G{id} 256 dict def /F{id} G{id} mkfont end\n");
        self.fonts.push(FontState { key, id, defined: [false; 256] });
        self.fonts.len() - 1
    }

    fn define_glyph(&mut self, fi: usize, face: &Face, b: u8) {
        if self.fonts[fi].defined[b as usize] {
            return;
        }
        self.fonts[fi].defined[b as usize] = true;
        let id = self.fonts[fi].id;
        let mut s = String::new();
        let _ = write!(s, "G{id} /g{b} {{0 0 setcharwidth");
        match face {
            Face::Outline { outlines, bold } => {
                let (ops, _) = outlines.outline(crate::font::cp850(b));
                let mut cur = (0.0, 0.0);
                let mut open = false;
                for (i, o) in ops.iter().enumerate() {
                    if i % 6 == 5 {
                        s.push('\n');
                    }
                    s.push(' ');
                    match *o {
                        Op::M(p) => {
                            if open {
                                s.push_str("h ");
                            }
                            nums(&mut s, &[p.0, p.1]);
                            s.push_str(" m");
                            cur = p;
                            open = true;
                        }
                        Op::L(p) => {
                            nums(&mut s, &[p.0, p.1]);
                            s.push_str(" l");
                            cur = p;
                        }
                        Op::Q(c1, p) => {
                            // Quadratic to cubic.
                            let a = (cur.0 + 2.0 / 3.0 * (c1.0 - cur.0), cur.1 + 2.0 / 3.0 * (c1.1 - cur.1));
                            let bq = (p.0 + 2.0 / 3.0 * (c1.0 - p.0), p.1 + 2.0 / 3.0 * (c1.1 - p.1));
                            nums(&mut s, &[a.0, a.1, bq.0, bq.1, p.0, p.1]);
                            s.push_str(" c");
                            cur = p;
                        }
                        Op::C(c1, c2, p) => {
                            nums(&mut s, &[c1.0, c1.1, c2.0, c2.1, p.0, p.1]);
                            s.push_str(" c");
                            cur = p;
                        }
                    }
                }
                if open {
                    s.push_str(" h");
                }
                if *bold > 0.0 {
                    s.push_str(" gs f gr ");
                    num(&mut s, *bold);
                    s.push_str(" w s");
                } else {
                    s.push_str(" f");
                }
            }
            Face::Image(f) => {
                let (l, n) = f.ink(b);
                if n > 0 && f.height > 0 {
                    let rowb = (n as usize).div_ceil(8);
                    let _ = write!(s, " gs {} {} translate {} {} scale {} {} true [{} 0 0 -{} 0 {}] <", l, -f.descender, n, f.height, n, f.height, n, f.height, f.height);
                    for row in 0..f.height {
                        let mut bytes = alloc::vec![0u8; rowb];
                        for col in 0..n {
                            if f.pixel(b, row, l + col) {
                                bytes[col as usize / 8] |= 0x80 >> (col % 8);
                            }
                        }
                        for x in bytes {
                            let _ = write!(s, "{:02x}", x);
                        }
                    }
                    s.push_str("> imagemask gr");
                }
            }
        }
        s.push_str("} put\n");
        self.out.push_str(&s);
    }
}

impl Sink for PsWriter {
    fn fill(&mut self, clip: &[Rect], figs: &[Vec<Pf>], winding: bool, paint: &Paint) {
        if figs.iter().all(|f| f.len() < 3) || !self.prepare(clip) {
            return;
        }
        let figs = figs.to_vec();
        self.paint_with(paint, if winding { "f" } else { "ef" }, &|w: &mut PsWriter| w.path(&figs, true));
    }

    fn stroke(&mut self, clip: &[Rect], pts: &[Pf], closed: bool, width: f64, color: Rgb, dash: &[f64]) {
        if pts.is_empty() || !self.prepare(clip) {
            return;
        }
        self.set_color(color);
        let width = width.max(1.0);
        if self.width != Some(width) {
            let mut s = String::new();
            num(&mut s, width);
            s.push_str(" w\n");
            self.out.push_str(&s);
            self.width = Some(width);
        }
        if self.dash.as_deref() != Some(dash) {
            let mut s = String::from("[");
            nums(&mut s, dash);
            s.push_str("] 0 d\n");
            self.out.push_str(&s);
            self.dash = Some(dash.to_vec());
        }
        let mut v = pts.to_vec();
        if v.len() == 1 {
            // A single pel: a dot (round caps).
            v.push((v[0].0 + 0.01, v[0].1));
        }
        self.path(&[v], closed);
        self.out.push_str("s\n");
    }

    fn rects(&mut self, clip: &[Rect], rects: &[Rect], paint: &Paint) {
        let rects: Vec<Rect> = rects.iter().copied().filter(|r| !r.is_empty()).collect();
        if rects.is_empty() || !self.prepare(clip) {
            return;
        }
        let mut list = String::from("[");
        for r in &rects {
            let _ = write!(list, "{} {} {} {} ", r.left, r.bottom, r.right - r.left, r.top - r.bottom);
        }
        list.push(']');
        match paint {
            Paint::Solid(c) => {
                self.set_color(*c);
                self.out.push_str(&list);
                self.out.push_str(" rf\n");
            }
            _ => {
                let figs: Vec<Vec<Pf>> = rects.iter().map(|r| alloc::vec![(r.left as f64, r.bottom as f64), (r.right as f64, r.bottom as f64), (r.right as f64, r.top as f64), (r.left as f64, r.top as f64)]).collect();
                self.paint_with(paint, "f", &|w: &mut PsWriter| w.path(&figs, true));
            }
        }
    }

    fn image(&mut self, clip: &[Rect], dst: Rect, w: i32, h: i32, px: &[Option<Rgb>]) {
        if w <= 0 || h <= 0 || dst.is_empty() || px.len() < (w * h) as usize || !self.prepare(clip) {
            return;
        }
        let transparent = px.iter().any(Option::is_none);
        if transparent && px.iter().all(Option::is_none) {
            return;
        }
        // A key colour no opaque sample has.
        let key = if transparent {
            let mut k = 0x01_02_03u32;
            while px.iter().any(|p| *p == Some(k)) {
                k = k.wrapping_add(0x01_01_07) & 0xff_ffff;
            }
            Some(k)
        } else {
            None
        };
        let mut data = Vec::with_capacity(px.len() * 3);
        for p in px.iter().take((w * h) as usize) {
            let c = p.or(key).unwrap_or(0xffffff);
            data.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        }
        let mut s = String::new();
        let _ = write!(s, "gs {} {} translate {} {} scale\n", dst.left, dst.bottom, dst.right - dst.left, dst.top - dst.bottom);
        match key {
            None => {
                let _ = write!(s, "{w} {h} 8 [{w} 0 0 {h} 0 0] currentfile /ASCII85Decode filter /RunLengthDecode filter false 3 colorimage\n");
            }
            Some(k) => {
                self.level3 = true;
                let _ = write!(s, "/DeviceRGB setcolorspace << /ImageType 4 /Width {w} /Height {h} /BitsPerComponent 8 /Decode [0 1 0 1 0 1] /ImageMatrix [{w} 0 0 {h} 0 0] /MaskColor [{} {} {}] /DataSource currentfile /ASCII85Decode filter /RunLengthDecode filter >> image\n", k >> 16, (k >> 8) & 0xff, k & 0xff);
            }
        }
        ascii85(&run_length(&data), &mut s);
        s.push_str("gr\n");
        self.out.push_str(&s);
        self.forget_color_only();
    }

    fn glyphs(&mut self, clip: &[Rect], run: &GlyphRun) {
        if run.codes.is_empty() || !self.prepare(clip) {
            return;
        }
        let fi = self.font_for(&run.face);
        for &b in run.codes {
            self.define_glyph(fi, &run.face, b);
        }
        self.set_color(run.color);
        let id = self.fonts[fi].id;
        let mut s = String::new();
        let _ = write!(s, "F{id} [");
        nums(&mut s, &[run.matrix[0], run.matrix[1], run.matrix[2], run.matrix[3]]);
        s.push_str(" 0 0] makefont setfont ");
        nums(&mut s, &[run.xs[0], run.y]);
        s.push_str(" m ");
        ps_string(&mut s, run.codes);
        s.push_str(" [");
        let n = run.codes.len();
        for i in 0..n {
            let next = if i + 1 < n { run.xs[i + 1] } else { run.xs[i] };
            num(&mut s, next - run.xs[i]);
            s.push(' ');
        }
        s.push_str("] xshow\n");
        self.out.push_str(&s);
    }

    fn as_any(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

impl PsWriter {
    fn forget_color_only(&mut self) {
        self.color = None;
    }

    /// Whether an image needed LanguageLevel 3 (masked images).
    pub fn uses_level3(&self) -> bool {
        self.level3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(w: &mut PsWriter) -> String {
        String::from_utf8(w.take()).unwrap()
    }

    #[test]
    fn numbers() {
        let mut s = String::new();
        nums(&mut s, &[1.0, -2.5, 0.125, 3.999, -0.05]);
        assert_eq!(s, "1 -2.5 0.13 4 -0.05");
    }

    #[test]
    fn run_length_round_trip() {
        let d = [1u8, 1, 1, 1, 2, 3, 4, 4, 4, 4, 4, 5];
        let e = run_length(&d);
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let n = e[i];
            i += 1;
            if n == 128 {
                break;
            }
            if n < 128 {
                out.extend_from_slice(&e[i..i + n as usize + 1]);
                i += n as usize + 1;
            } else {
                for _ in 0..257 - n as usize {
                    out.push(e[i]);
                }
                i += 1;
            }
        }
        assert_eq!(out, d);
    }

    #[test]
    fn ascii85_known() {
        let mut s = String::new();
        ascii85(b"Man ", &mut s);
        assert_eq!(s, "9jqo^~>\n");
        let mut s = String::new();
        ascii85(&[0, 0, 0, 0, 1], &mut s);
        assert_eq!(s, "z!<~>\n");
    }

    #[test]
    fn document_structure() {
        let mut w = PsWriter::new(300, (612, 792), false);
        w.title = b"Test".to_vec();
        w.begin_doc();
        let page = [Rect::new(0, 0, 2550, 3300)];
        w.fill(&page, &[alloc::vec![(100.0, 100.0), (200.0, 100.0), (150.0, 200.0)]], false, &Paint::Solid(0xff0000));
        w.stroke(&page, &[(0.0, 0.0), (300.0, 300.0)], false, 1.0, 0x000000, &[]);
        w.new_frame();
        w.rects(&[Rect::new(10, 10, 20, 20)], &[Rect::new(0, 0, 100, 100)], &Paint::Solid(0x0000ff));
        w.end_doc();
        let t = text(&mut w);
        assert!(t.starts_with("%!PS-Adobe-3.0\n"));
        assert!(t.contains("%%Title: Test\n"));
        assert!(t.contains("%%Page: 1 1\n"));
        assert!(t.contains("%%Page: 2 2\n"));
        assert!(t.contains("1 0 0 rg\nn 100 100 m 200 100 l 150 200 l h\nef\n"));
        assert!(t.contains("n 0 0 m 300 300 l\ns\n"));
        // The second page's clip, then its rectangle.
        assert!(t.contains("gr gs [10 10 10 10 ] rc\n0 0 1 rg\n[0 0 100 100 ] rf\n"));
        assert!(t.contains("0.24 0.24 scale"));
        assert!(t.ends_with("%%Trailer\n%%Pages: 2\n%%EOF\n"));
    }

    #[test]
    fn image_and_mask() {
        let mut w = PsWriter::new(300, (612, 792), false);
        let page = [Rect::new(0, 0, 2550, 3300)];
        w.image(&page, Rect::new(0, 0, 20, 20), 2, 2, &[Some(0), Some(0xffffff), Some(0xffffff), Some(0)]);
        w.image(&page, Rect::new(0, 0, 20, 20), 2, 1, &[Some(0xff), None]);
        let t = text(&mut w);
        assert!(t.contains("2 2 8 [2 0 0 2 0 0] currentfile /ASCII85Decode filter /RunLengthDecode filter false 3 colorimage"));
        assert!(t.contains("/ImageType 4"));
        assert!(w.uses_level3());
    }

    #[test]
    fn patterns_are_defined_once() {
        let mut w = PsWriter::new(300, (612, 792), false);
        let page = [Rect::new(0, 0, 2550, 3300)];
        let p = Paint::Pattern { bits: [0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55], fg: 0, bg: Some(0xffffff), org: (0, 0) };
        w.rects(&page, &[Rect::new(0, 0, 50, 50)], &p);
        w.rects(&page, &[Rect::new(60, 0, 90, 50)], &p);
        let t = text(&mut w);
        assert_eq!(t.matches("makepattern").count(), 1);
        assert_eq!(t.matches("P1 setcolor").count(), 2);
    }

    #[test]
    fn landscape_page() {
        let mut w = PsWriter::new(300, (612, 792), true);
        assert_eq!(w.page_pels(), (3300, 2550));
        w.new_frame();
        let t = text(&mut w);
        assert!(t.contains("612 0 translate 90 rotate"));
    }
}
