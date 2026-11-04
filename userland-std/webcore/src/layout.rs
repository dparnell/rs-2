//! Layout: the box tree is laid out by Taffy (block, flex and grid; tables
//! become grids), and inline content by this module (white space, line
//! breaking, alignment, atomic inlines). The result is a display list in
//! page coordinates (pels, y down) and the links' rectangles.
//!
//! Text is measured through the [`Fonts`] trait, so the same code serves
//! PM (GPI fonts) and tests on the host.

use std::collections::HashMap;
use std::sync::Arc;

use taffy::prelude::*;
use taffy::style_helpers;
use url::Url;

use crate::css::{Color, TRANSPARENT};
use crate::dom::{Dom, NodeData, NodeId as DomId};
use crate::style::{self, Align, Display as D, Family, Len, ListStyle, Position as P, Style, Styles, Track, WhiteSpace};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FontKey {
    pub family: Family,
    pub size: u16,
    pub bold: bool,
    pub italic: bool,
}

pub fn font_of(s: &Style) -> FontKey {
    FontKey { family: s.family, size: s.font_size.round().clamp(4.0, 200.0) as u16, bold: s.bold, italic: s.italic }
}

pub trait Fonts {
    /// Width of `text` in pels.
    fn width(&mut self, f: FontKey, text: &str) -> f32;
    /// Ascender and descender in pels.
    fn metrics(&mut self, f: FontKey) -> (f32, f32);
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Text,
    Password,
    Submit,
    Button,
    Checkbox,
    Radio,
    Select,
    TextArea,
    Image,
    /// A `<button>`: its content is laid out and painted like any box's;
    /// this only marks where it is.
    ButtonBox,
}

#[derive(Clone, Debug)]
pub enum Item {
    Fill { r: Rect, color: Color },
    Text { x: f32, y: f32, w: f32, text: String, font: FontKey, color: Color, decoration: u8 },
    Image { r: Rect, node: DomId },
    /// An inline `<svg>`; `bg` is the colour behind it.
    Svg { r: Rect, node: DomId, bg: Color },
    /// A box with rounded corners, or only its outline `ring` pels wide.
    RoundRect { r: Rect, color: Color, radii: [f32; 4], ring: f32 },
    /// What is outside rounded corners, over an image.
    Corners { r: Rect, color: Color, radii: [f32; 4] },
    Marker { r: Rect, kind: ListStyle, color: Color },
    /// A form control; its box has been painted. `pad` is the room left of
    /// its text.
    Control { r: Rect, node: DomId, kind: ControlKind, font: FontKey, color: Color, pad: f32 },
    /// Items up to the matching PopClip are clipped to `r`.
    PushClip { r: Rect },
    PopClip,
}

/// A rounded box as rectangles (x, y, w, h), row by row where the corners
/// need it; with `ring` only an outline that wide.
pub fn round_spans(r: Rect, radii: [f32; 4], ring: f32, mut out: impl FnMut(f32, f32, f32, f32)) {
    let (x, y, w, h) = (r.x.round(), r.y.round(), (r.x + r.w).round() - r.x.round(), (r.y + r.h).round() - r.y.round());
    let inset = |row: f32, top: f32, bottom: f32, h: f32| -> f32 {
        let c = |rad: f32, d: f32| if rad > 0.0 && d < rad { rad - (rad * rad - (rad - d) * (rad - d)).max(0.0).sqrt() } else { 0.0 };
        c(top, row + 0.5).max(c(bottom, h - row - 0.5))
    };
    let mut run: Option<(f32, [f32; 4], f32)> = None; // start row, spans, end row
    let flush = |run: &mut Option<(f32, [f32; 4], f32)>, out: &mut dyn FnMut(f32, f32, f32, f32)| {
        if let Some((start, s, end)) = run.take() {
            out(x + s[0], y + start, s[1] - s[0], end - start);
            if s[3] > s[2] {
                out(x + s[2], y + start, s[3] - s[2], end - start);
            }
        }
    };
    let (iw, ih) = (w - 2.0 * ring, h - 2.0 * ring);
    let ir = radii.map(|v| (v - ring).max(0.0));
    for row in 0..h as i32 {
        let row = row as f32;
        let l = inset(row, radii[0], radii[3], h).round();
        let rt = w - inset(row, radii[1], radii[2], h).round();
        let spans = if ring > 0.0 && row >= ring && row < h - ring && iw > 0.0 && ih > 0.0 {
            let il = ring + inset(row - ring, ir[0], ir[3], ih).round();
            let irt = ring + iw - inset(row - ring, ir[1], ir[2], ih).round();
            [l, il.max(l), irt.min(rt), rt]
        } else {
            [l, rt, 0.0, 0.0]
        };
        match &mut run {
            Some((_, s, end)) if *s == spans => *end = row + 1.0,
            _ => {
                flush(&mut run, &mut out);
                run = Some((row, spans, row + 1.0));
            }
        }
    }
    flush(&mut run, &mut out);
}

/// The parts of a box outside its rounded corners, as rectangles.
pub fn corner_spans(r: Rect, radii: [f32; 4], mut out: impl FnMut(f32, f32, f32, f32)) {
    let (x, y, w, h) = (r.x.round(), r.y.round(), (r.x + r.w).round() - r.x.round(), (r.y + r.h).round() - r.y.round());
    let cut = |rad: f32, d: f32| if rad > 0.0 && d < rad { (rad - (rad * rad - (rad - d) * (rad - d)).max(0.0).sqrt()).round() } else { 0.0 };
    for row in 0..h as i32 {
        let row = row as f32;
        let (top, bottom) = (row + 0.5, h - row - 0.5);
        let l = cut(radii[0], top).max(cut(radii[3], bottom));
        let rt = cut(radii[1], top).max(cut(radii[2], bottom));
        if l > 0.0 {
            out(x, y + row, l, 1.0);
        }
        if rt > 0.0 {
            out(x + w - rt, y + row, rt, 1.0);
        }
    }
}

/// What a page looks like at a width.
pub struct Laid {
    pub items: Vec<Item>,
    /// Link rectangles and link numbers (into `links`).
    pub link_rects: Vec<(Rect, usize)>,
    pub links: Vec<Url>,
    pub width: f32,
    pub height: f32,
    pub background: Color,
    /// Element positions (for #fragment scrolling): (id attribute, y).
    pub anchors: Vec<(String, f32)>,
}

/// Natural sizes of images (by element), once known.
pub type ImageSizes = HashMap<DomId, (u32, u32)>;

// ---- Inline content ----

#[derive(Clone, Debug)]
enum Atom {
    Image(DomId),
    Control(DomId, ControlKind),
    Svg(DomId),
    /// An inline-block: a separately laid out subtree.
    Subtree(NodeId),
}

#[derive(Clone, Debug)]
enum Kind {
    Word,
    /// An inline box's margin, border and padding (left or right).
    Gap,
    Space,
    Break,
    /// Width, height, baseline below its top, and a percentage of the
    /// line's width in place of the width (0 for none, negative for a
    /// maximum width).
    Atomic(Atom, f32, f32, f32, f32),
    Marker(ListStyle),
}

#[derive(Clone, Debug)]
struct Piece {
    kind: Kind,
    text: String,
    w: f32,
    style: Arc<Style>,
    font: FontKey,
    link: Option<usize>,
    bg: Color,
    wrap: bool,
}

struct Ifc {
    pieces: Vec<Piece>,
    strut: (f32, f32, f32),
    align: Align,
    indent: f32,
    cache: Vec<(f32, Lines)>,
    min_content: f32,
    max_content: f32,
    /// Floats beside this context: (top, bottom, left inset, right inset)
    /// relative to its content box (from a first layout pass).
    excl: Vec<(f32, f32, f32, f32)>,
}

#[derive(Clone, Debug, Default)]
struct Lines {
    frags: Vec<Frag>,
    height: f32,
    width: f32,
}

#[derive(Clone, Debug)]
struct Frag {
    x: f32,
    /// Top of the line box and baseline (relative to the IFC).
    top: f32,
    baseline: f32,
    line_h: f32,
    piece: usize,
    text: String,
    w: f32,
}

#[derive(Default)]
struct IfcBuilder {
    raw: Vec<Raw>,
    marker: Option<(ListStyle, usize, Arc<Style>)>,
}

enum Raw {
    Text(String, Arc<Style>, Option<usize>, Color),
    Break(Arc<Style>),
    Atomic(Atom, f32, f32, f32, f32, Arc<Style>, Option<usize>),
    /// Horizontal space of an inline box's edge; `true` on its left.
    Gap(f32, Arc<Style>, bool),
}

impl IfcBuilder {
    fn has_content(&self) -> bool {
        self.raw.iter().any(|r| match r {
            Raw::Text(t, s, ..) => !t.trim().is_empty() || matches!(s.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap) && !t.is_empty(),
            Raw::Break(_) => true,
            Raw::Atomic(..) => true,
            Raw::Gap(..) => false,
        })
    }
}

fn transform(t: &str, mode: u8) -> String {
    match mode {
        1 => t.to_uppercase(),
        2 => t.to_lowercase(),
        3 => {
            let mut out = String::with_capacity(t.len());
            let mut start = true;
            for c in t.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            out
        }
        _ => t.to_string(),
    }
}

fn roman(mut n: usize, upper: bool) -> String {
    let t = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper { s.to_uppercase() } else { s }
}

pub fn marker_text(kind: ListStyle, n: usize) -> Option<String> {
    Some(match kind {
        ListStyle::Decimal => format!("{n}."),
        ListStyle::LowerAlpha => format!("{}.", (b'a' + ((n.max(1) - 1) % 26) as u8) as char),
        ListStyle::UpperAlpha => format!("{}.", (b'A' + ((n.max(1) - 1) % 26) as u8) as char),
        ListStyle::LowerRoman => format!("{}.", roman(n, false)),
        ListStyle::UpperRoman => format!("{}.", roman(n, true)),
        _ => return None,
    })
}

fn build_ifc(b: IfcBuilder, block: &Style, fonts: &mut dyn Fonts, cw: f32) -> Ifc {
    let mut pieces: Vec<Piece> = Vec::new();
    let bf = font_of(block);
    let (a, d) = fonts.metrics(bf);
    let lh = block.line_height_px();
    if let Some((kind, n, st)) = &b.marker {
        let f = font_of(st);
        let (text, w) = match marker_text(*kind, *n) {
            Some(t) => {
                let w = fonts.width(f, &t) + st.font_size * 0.4;
                (t, w)
            }
            None => (String::new(), st.font_size * 0.9),
        };
        if *kind != ListStyle::None {
            pieces.push(Piece { kind: Kind::Marker(*kind), text, w, style: st.clone(), font: f, link: None, bg: TRANSPARENT, wrap: false });
        }
    }
    let mut last_space = true;
    for r in b.raw {
        match r {
            Raw::Text(t, st, link, bg) => {
                let f = font_of(&st);
                let t = transform(&t, st.text_transform);
                let pre = matches!(st.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap);
                let keep_breaks = pre || st.white_space == WhiteSpace::PreLine;
                let wrap = !matches!(st.white_space, WhiteSpace::Pre | WhiteSpace::NoWrap);
                let mut word = String::new();
                let push_word = |word: &mut String, pieces: &mut Vec<Piece>, fonts: &mut dyn Fonts| {
                    if !word.is_empty() {
                        let w = fonts.width(f, word);
                        pieces.push(Piece { kind: Kind::Word, text: std::mem::take(word), w, style: st.clone(), font: f, link, bg, wrap });
                    }
                };
                for c in t.chars() {
                    match c {
                        '\n' if keep_breaks => {
                            push_word(&mut word, &mut pieces, fonts);
                            pieces.push(Piece { kind: Kind::Break, text: String::new(), w: 0.0, style: st.clone(), font: f, link, bg, wrap });
                            last_space = true;
                        }
                        ' ' | '\t' | '\n' | '\r' | '\u{c}' => {
                            if pre {
                                // Kept spaces are words that may break after.
                                let n = if c == '\t' { 8 } else { 1 };
                                push_word(&mut word, &mut pieces, fonts);
                                let sp = " ".repeat(n);
                                let w = fonts.width(f, &sp);
                                pieces.push(Piece { kind: if wrap { Kind::Space } else { Kind::Word }, text: sp, w, style: st.clone(), font: f, link, bg, wrap });
                            } else if !last_space {
                                push_word(&mut word, &mut pieces, fonts);
                                let w = fonts.width(f, " ");
                                pieces.push(Piece { kind: Kind::Space, text: " ".into(), w, style: st.clone(), font: f, link, bg, wrap });
                                last_space = true;
                            }
                        }
                        '\u{a0}' => {
                            word.push(' ');
                            last_space = false;
                        }
                        c => {
                            word.push(c);
                            last_space = false;
                        }
                    }
                }
                push_word(&mut word, &mut pieces, fonts);
            }
            Raw::Break(st) => {
                pieces.push(Piece { kind: Kind::Break, text: String::new(), w: 0.0, font: font_of(&st), style: st, link: None, bg: TRANSPARENT, wrap: true });
                last_space = true;
            }
            Raw::Gap(w, st, left) => {
                pieces.push(Piece { kind: Kind::Gap, text: String::new(), w, font: font_of(&st), style: st, link: None, bg: TRANSPARENT, wrap: left });
                if left {
                    last_space = false;
                }
            }
            Raw::Atomic(atom, w, h, base, pct, st, link) => {
                pieces.push(Piece { kind: Kind::Atomic(atom, w, h, base, pct), text: String::new(), w, font: font_of(&st), style: st, link, bg: TRANSPARENT, wrap: true });
                last_space = false;
            }
        }
    }
    // Spaces before a break or at the end go.
    while matches!(pieces.last().map(|p| &p.kind), Some(Kind::Space)) {
        pieces.pop();
    }
    let mut min_content: f32 = 0.0;
    let mut run = 0.0;
    for p in &pieces {
        match p.kind {
            Kind::Space if p.wrap => run = 0.0,
            Kind::Break => run = 0.0,
            Kind::Atomic(_, _, _, _, pct) => {
                min_content = if pct != 0.0 { min_content } else { min_content.max(p.w) };
                run = 0.0;
            }
            _ => {
                run += p.w;
                min_content = min_content.max(run);
            }
        }
    }
    let mut ifc = Ifc {
        pieces,
        strut: (lh, a, d),
        align: block.text_align,
        indent: block.text_indent.px(cw),
        cache: Vec::new(),
        min_content,
        max_content: 0.0,
        excl: Vec::new(),
    };
    ifc.max_content = lay_lines(&ifc, None, fonts).width;
    ifc
}

/// The left inset and width of a line starting at `y` (floats beside it).
fn band(ifc: &Ifc, width: Option<f32>, y: f32) -> (f32, Option<f32>) {
    let Some(w) = width else { return (0.0, None) };
    let h = ifc.strut.0.max(1.0);
    let (mut l, mut r) = (0.0f32, 0.0f32);
    for &(top, bottom, li, ri) in &ifc.excl {
        if y < bottom && y + h > top {
            l = l.max(li);
            r = r.max(ri);
        }
    }
    if l + r > w - 20.0 {
        // No room beside the floats: lines go below them (approximately).
        return (0.0, Some(w));
    }
    (l, Some(w - l - r))
}

fn lay_lines(ifc: &Ifc, width_all: Option<f32>, fonts: &mut dyn Fonts) -> Lines {
    let mut out = Lines::default();
    let mut line: Vec<(usize, f32, String, f32)> = Vec::new(); // piece, x, text, w
    let mut x = ifc.indent;
    let mut pending = 0.0f32;
    let mut y = 0.0f32;
    let (mut left, mut width) = band(ifc, width_all, 0.0);
    let pieces = &ifc.pieces;
    for (i, p) in pieces.iter().enumerate() {
        match &p.kind {
            Kind::Break => {
                finish(ifc, width, left, fonts, &mut line, x, &mut y, &mut out);
                (left, width) = band(ifc, width_all, y);
                x = 0.0;
                pending = 0.0;
            }
            Kind::Space => {
                if !line.is_empty() {
                    pending += p.w;
                }
            }
            Kind::Marker(_) => {
                // Outside the content box, before the first line.
                line.push((i, -p.w, p.text.clone(), p.w));
            }
            Kind::Atomic(_, aw, _, _, pct) if *pct != 0.0 && width.is_some() && (*pct > 0.0 || *aw > width.unwrap() * -pct / 100.0) => {
                // Sized by the line: a field of width 100%.
                let w = width.unwrap() * pct.abs() / 100.0;
                if !line.is_empty() && x + pending + w > width.unwrap() + 0.5 {
                    finish(ifc, width, left, fonts, &mut line, x, &mut y, &mut out);
                    (left, width) = band(ifc, width_all, y);
                    x = 0.0;
                    pending = 0.0;
                }
                x += pending;
                pending = 0.0;
                line.push((i, x, String::new(), w));
                x += w;
            }
            _ => {
                let can_break = p.wrap && !line.is_empty() && (pending > 0.0 || matches!(p.kind, Kind::Atomic(..)) || matches!(pieces[i - 1].kind, Kind::Atomic(..)));
                if let Some(w) = width {
                    if can_break && x + pending + p.w > w + 0.5 {
                        finish(ifc, width, left, fonts, &mut line, x, &mut y, &mut out);
                (left, width) = band(ifc, width_all, y);
                        x = 0.0;
                        pending = 0.0;
                    }
                    // A word wider than the line: break it between characters.
                    if matches!(p.kind, Kind::Word) && p.wrap && x + pending == 0.0 && p.w > w + 0.5 && w > 0.0 {
                        let mut part = String::new();
                        for c in p.text.chars() {
                            let mut t = part.clone();
                            t.push(c);
                            let tw = fonts.width(p.font, &t);
                            if tw > w + 0.5 && !part.is_empty() {
                                let pw = fonts.width(p.font, &part);
                                line.push((i, x, std::mem::take(&mut part), pw));
                                x += pw;
                                finish(ifc, width, left, fonts, &mut line, x, &mut y, &mut out);
                (left, width) = band(ifc, width_all, y);
                                x = 0.0;
                                part.push(c);
                            } else {
                                part = t;
                            }
                        }
                        if !part.is_empty() {
                            let pw = fonts.width(p.font, &part);
                            line.push((i, x, part, pw));
                            x += pw;
                        }
                        continue;
                    }
                }
                x += pending;
                pending = 0.0;
                // Merge with the previous word of the same look.
                if let Some(last) = line.last_mut() {
                    let lp = &pieces[last.0];
                    if matches!(lp.kind, Kind::Word) && matches!(p.kind, Kind::Word) && lp.font == p.font && lp.link == p.link
                        && lp.bg == p.bg && lp.style.color == p.style.color && lp.style.decoration == p.style.decoration
                        && lp.style.vertical_align == p.style.vertical_align
                    {
                        let gap = x - (last.1 + last.3);
                        if gap.abs() < 0.01 || gap > 0.0 {
                            if gap > 0.0 {
                                last.2.push(' ');
                            }
                            last.2.push_str(&p.text);
                            last.3 = x + p.w - last.1;
                            x += p.w;
                            continue;
                        }
                    }
                }
                line.push((i, x, p.text.clone(), p.w));
                x += p.w;
            }
        }
    }
    if !line.is_empty() {
        finish(ifc, width, left, fonts, &mut line, x, &mut y, &mut out);
    }
    out.height = y;
    out
}

/// An atomic piece's height and baseline at the width `w` its line gave
/// it: images keep their shape.
fn atomic_height(p: &Piece, w: f32) -> (f32, f32) {
    let Kind::Atomic(atom, aw, h, base, _) = &p.kind else { return (0.0, 0.0) };
    if matches!(atom, Atom::Image(_)) && *aw > 0.0 && p.style.height == Len::Auto {
        let s = w / aw;
        (h * s, base * s)
    } else {
        (*h, *base)
    }
}

#[allow(clippy::too_many_arguments)]
fn finish(ifc: &Ifc, width: Option<f32>, left: f32, fonts: &mut dyn Fonts, line: &mut Vec<(usize, f32, String, f32)>, x: f32, y: &mut f32, out: &mut Lines) {
    let pieces = &ifc.pieces;
    let (lh, sa, sd) = ifc.strut;
    let hl = (lh - (sa + sd)) / 2.0;
    let mut above = sa + hl;
    let mut below = sd + hl;
    for (i, _, _, fw) in line.iter() {
        let p = &pieces[*i];
        match &p.kind {
            Kind::Atomic(..) => match (p.style.vertical_align, atomic_height(p, *fw)) {
                (1, (h, _)) => {
                    above = above.max(h / 2.0 + sa * 0.3);
                    below = below.max(h / 2.0 - sa * 0.3);
                }
                (2, (h, _)) => below = below.max(h - sa),
                (3, (h, _)) => above = above.max(h - sd),
                (_, (h, base)) => {
                    above = above.max(base);
                    below = below.max(h - base);
                }
            },
            _ => {
                let (a, d) = fonts.metrics(p.font);
                let l = p.style.line_height_px();
                let hl = (l - (a + d)) / 2.0;
                above = above.max(a + hl);
                below = below.max(d + hl);
            }
        }
    }
    let h = (above + below).max(0.0);
    let used = x;
    let shift = match (width, ifc.align) {
        (Some(w), Align::Right) => (w - used).max(0.0),
        (Some(w), Align::Center) => ((w - used) / 2.0).max(0.0),
        _ => 0.0,
    };
    for (i, fx, text, w) in line.drain(..) {
        out.frags.push(Frag { x: fx + shift + left, top: *y, baseline: *y + above, line_h: h, piece: i, text, w });
    }
    out.width = out.width.max(used + left);
    *y += h;
}

fn ifc_layout<'a>(ifc: &'a mut Ifc, width: f32, fonts: &mut dyn Fonts) -> &'a Lines {
    let w = (width * 4.0).ceil() / 4.0;
    if let Some(i) = ifc.cache.iter().position(|(cw, _)| *cw == w) {
        return &ifc.cache[i].1;
    }
    let l = lay_lines(ifc, Some(w), fonts);
    if ifc.cache.len() >= 4 {
        ifc.cache.remove(0);
    }
    ifc.cache.push((w, l));
    &ifc.cache.last().unwrap().1
}

// ---- The box tree ----

#[derive(Clone, Debug)]
enum Leaf {
    Ifc(usize),
    /// A block-level replaced element, and its content size.
    Atom(Atom, f32, f32),
}

struct BoxInfo {
    node: Option<DomId>,
    style: Arc<Style>,
    /// A row's background drawn in its cells.
    bg_override: Color,
}

struct Builder<'a> {
    dom: &'a Dom,
    styles: &'a Styles,
    fonts: &'a mut dyn Fonts,
    tree: TaffyTree<Leaf>,
    info: HashMap<NodeId, BoxInfo>,
    ifcs: Vec<Ifc>,
    base: Url,
    links: Vec<Url>,
    images: &'a ImageSizes,
    viewport: f32,
    pending_marker: Option<(ListStyle, usize, Arc<Style>)>,
    depth: u32,
}

fn len_dim(l: Len) -> Dimension {
    match l {
        Len::Auto => Dimension::auto(),
        Len::Px(v) => Dimension::length(v),
        Len::Pct(p) => Dimension::percent(p / 100.0),
    }
}

fn len_lpa(l: Len) -> LengthPercentageAuto {
    match l {
        Len::Auto => LengthPercentageAuto::auto(),
        Len::Px(v) => LengthPercentageAuto::length(v),
        Len::Pct(p) => LengthPercentageAuto::percent(p / 100.0),
    }
}

fn len_lp(l: Len) -> LengthPercentage {
    match l {
        Len::Auto => LengthPercentage::length(0.0),
        Len::Px(v) => LengthPercentage::length(v),
        Len::Pct(p) => LengthPercentage::percent(p / 100.0),
    }
}

fn taffy_style(s: &Style) -> taffy::Style {
    let mut t = taffy::Style {
        display: match s.display {
            D::Flex | D::InlineFlex => Display::Flex,
            D::Grid | D::InlineGrid | D::Table => Display::Grid,
            D::None => Display::None,
            _ => Display::Block,
        },
        box_sizing: if s.border_box { BoxSizing::BorderBox } else { BoxSizing::ContentBox },
        size: Size { width: len_dim(s.width), height: len_dim(s.height) },
        min_size: Size { width: len_lpa(s.min_width), height: len_lpa(s.min_height) },
        max_size: Size { width: len_lpa(s.max_width), height: len_lpa(s.max_height) },
        margin: taffy::Rect {
            left: len_lpa(s.margin[style::LEFT]),
            right: len_lpa(s.margin[style::RIGHT]),
            top: len_lpa(s.margin[style::TOP]),
            bottom: len_lpa(s.margin[style::BOTTOM]),
        },
        padding: taffy::Rect {
            left: len_lp(s.padding[style::LEFT]),
            right: len_lp(s.padding[style::RIGHT]),
            top: len_lp(s.padding[style::TOP]),
            bottom: len_lp(s.padding[style::BOTTOM]),
        },
        border: taffy::Rect {
            left: LengthPercentage::length(s.border(style::LEFT)),
            right: LengthPercentage::length(s.border(style::RIGHT)),
            top: LengthPercentage::length(s.border(style::TOP)),
            bottom: LengthPercentage::length(s.border(style::BOTTOM)),
        },
        position: match s.position {
            P::Absolute | P::Fixed => Position::Absolute,
            _ => Position::Relative,
        },
        inset: if matches!(s.position, P::Static) {
            taffy::Rect { left: LengthPercentageAuto::auto(), right: LengthPercentageAuto::auto(), top: LengthPercentageAuto::auto(), bottom: LengthPercentageAuto::auto() }
        } else {
            taffy::Rect {
                left: len_lpa(s.inset[style::LEFT]),
                right: len_lpa(s.inset[style::RIGHT]),
                top: len_lpa(s.inset[style::TOP]),
                bottom: len_lpa(s.inset[style::BOTTOM]),
            }
        },
        flex_direction: match (s.flex_row, s.flex_reverse) {
            (true, false) => FlexDirection::Row,
            (true, true) => FlexDirection::RowReverse,
            (false, false) => FlexDirection::Column,
            (false, true) => FlexDirection::ColumnReverse,
        },
        flex_wrap: if s.flex_wrap { FlexWrap::Wrap } else { FlexWrap::NoWrap },
        flex_grow: s.flex_grow,
        flex_shrink: s.flex_shrink,
        flex_basis: len_dim(s.flex_basis),
        aspect_ratio: s.aspect_ratio,
        gap: Size { width: len_lp(s.gap.1), height: len_lp(s.gap.0) },
        justify_content: match s.justify {
            1 => JustifyContent::FLEX_START,
            2 => JustifyContent::FLEX_END,
            3 => JustifyContent::CENTER,
            4 => JustifyContent::SPACE_BETWEEN,
            5 => JustifyContent::SPACE_AROUND,
            6 => JustifyContent::SPACE_EVENLY,
            _ => taffy::AlignContent::NORMAL,
        },
        align_items: match s.align_items {
            1 => AlignItems::FLEX_START,
            2 => AlignItems::FLEX_END,
            3 => AlignItems::CENTER,
            7 => AlignItems::BASELINE,
            _ => taffy::AlignItems::NORMAL,
        },
        align_content: match s.align_content {
            1 => AlignContent::FLEX_START,
            2 => AlignContent::FLEX_END,
            3 => AlignContent::CENTER,
            4 => AlignContent::SPACE_BETWEEN,
            5 => AlignContent::SPACE_AROUND,
            6 => AlignContent::SPACE_EVENLY,
            _ => AlignContent::NORMAL,
        },
        justify_self: match s.justify_self {
            1 => Some(AlignSelf::FLEX_START),
            2 => Some(AlignSelf::FLEX_END),
            3 => Some(AlignSelf::CENTER),
            _ => None,
        },
        align_self: match s.align_self {
            1 => Some(AlignSelf::FLEX_START),
            2 => Some(AlignSelf::FLEX_END),
            3 => Some(AlignSelf::CENTER),
            7 => Some(AlignSelf::BASELINE),
            _ => None,
        },
        float: match s.float {
            style::Float::Left => taffy::Float::Left,
            style::Float::Right => taffy::Float::Right,
            style::Float::None => taffy::Float::None,
        },
        clear: if s.clear { taffy::Clear::Both } else { taffy::Clear::None },
        ..Default::default()
    };
    if matches!(s.display, D::Grid | D::InlineGrid) && !s.grid_columns.is_empty() {
        t.grid_template_columns = s
            .grid_columns
            .iter()
            .map(|tr| {
                let f: TrackSizingFunction = match tr {
                    Track::Fr(f) => style_helpers::fr(*f),
                    Track::Share(f) => style_helpers::minmax(style_helpers::length(0.0), style_helpers::fr(*f)),
                    Track::Len(Len::Px(v)) => style_helpers::length(*v),
                    Track::Len(Len::Pct(p)) => style_helpers::percent(*p / 100.0),
                    Track::Len(Len::Auto) => style_helpers::auto(),
                    Track::MinContent => style_helpers::min_content(),
                    Track::MaxContent => style_helpers::max_content(),
                };
                GridTemplateComponent::Single(f)
            })
            .collect();
    }
    let tr = |tr: &Track| -> TrackSizingFunction {
        match tr {
            Track::Fr(f) => style_helpers::fr(*f),
            Track::Share(f) => style_helpers::minmax(style_helpers::length(0.0), style_helpers::fr(*f)),
            Track::Len(Len::Px(v)) => style_helpers::length(*v),
            Track::Len(Len::Pct(p)) => style_helpers::percent(*p / 100.0),
            Track::Len(Len::Auto) => style_helpers::auto(),
            Track::MinContent => style_helpers::min_content(),
            Track::MaxContent => style_helpers::max_content(),
        }
    };
    if matches!(s.display, D::Grid | D::InlineGrid) {
        if !s.grid_rows.is_empty() {
            t.grid_template_rows = s.grid_rows.iter().map(|x| GridTemplateComponent::Single(tr(x))).collect();
        }
        if !s.grid_areas.is_empty() {
            let mut areas: Vec<taffy::GridTemplateArea<String>> = Vec::new();
            for (r, row) in s.grid_areas.iter().enumerate() {
                for (c, name) in row.iter().enumerate() {
                    if name.starts_with('.') {
                        continue;
                    }
                    let (r, c) = (r as u16 + 1, c as u16 + 1);
                    match areas.iter_mut().find(|a| a.name == *name) {
                        Some(a) => {
                            a.row_start = a.row_start.min(r);
                            a.row_end = a.row_end.max(r + 1);
                            a.column_start = a.column_start.min(c);
                            a.column_end = a.column_end.max(c + 1);
                        }
                        None => areas.push(taffy::GridTemplateArea { name: name.clone(), row_start: r, row_end: r + 1, column_start: c, column_end: c + 1 }),
                    }
                }
            }
            let cols = s.grid_areas.iter().map(|r| r.len()).max().unwrap_or(0) as u16;
            t.grid_template_areas = Some(taffy::GridTemplateAreas { areas: areas.into_iter().collect(), row_count: s.grid_areas.len() as u16, column_count: cols });
        }
    }
    if let Some(a) = &s.grid_area {
        t.grid_row = Line { start: GridPlacement::NamedLine(a.clone(), 0), end: GridPlacement::NamedLine(a.clone(), 0) };
        t.grid_column = Line { start: GridPlacement::NamedLine(a.clone(), 0), end: GridPlacement::NamedLine(a.clone(), 0) };
    }
    let lines = |(a, b): (i16, i16), span: u16| -> Option<Line<GridPlacement>> {
        let spanned = || if span > 1 { style_helpers::span(span) } else { GridPlacement::Auto };
        match (a, b) {
            (0, 0) if span > 1 => Some(Line { start: style_helpers::span(span), end: GridPlacement::Auto }),
            (0, 0) => None,
            (0, b) => Some(Line { start: spanned(), end: style_helpers::line(b) }),
            (a, 0) => Some(Line { start: style_helpers::line(a), end: spanned() }),
            (a, b) => Some(Line { start: style_helpers::line(a), end: style_helpers::line(b) }),
        }
    };
    if s.grid_area.is_none() {
        if let Some(l) = lines(s.grid_row_lines, s.grid_row_span) {
            t.grid_row = l;
        }
        if let Some(l) = lines(s.grid_col_lines, s.grid_column_span) {
            t.grid_column = l;
        }
    }
    t
}

fn is_block_level(d: D) -> bool {
    matches!(d, D::Block | D::ListItem | D::Flex | D::Grid | D::Table | D::TableRow | D::TableCell | D::TableRowGroup | D::TableCaption)
}

fn replaced(name: &str) -> bool {
    matches!(name, "img" | "input" | "select" | "textarea" | "video" | "iframe" | "svg")
}

impl Builder<'_> {
    fn style(&self, id: DomId) -> Arc<Style> {
        self.styles.of[id].clone().unwrap_or_default()
    }

    fn add_link(&mut self, href: &str) -> Option<usize> {
        let href = href.trim();
        if href.is_empty() || href.starts_with("javascript:") {
            return None;
        }
        let u = self.base.join(href).ok()?;
        Some(match self.links.iter().position(|l| *l == u) {
            Some(i) => i,
            None => {
                self.links.push(u);
                self.links.len() - 1
            }
        })
    }

    /// A box for a block-level element (or an inline-block's root).
    fn block(&mut self, id: DomId) -> NodeId {
        let st = self.style(id);
        self.depth += 1;
        let tstyle = taffy_style(&st);
        let e = self.dom.element(id).unwrap();
        if st.display == D::ListItem {
            let parent = self.dom.parent_element(id).and_then(|p| self.dom.element(p));
            let start: usize = parent.and_then(|p| p.attr("start")).and_then(|s| s.trim().parse().ok()).unwrap_or(1);
            let n = match e.attr("value").and_then(|v| v.trim().parse().ok()) {
                Some(v) => v,
                None => {
                    let p = self.dom.nodes[id].parent.unwrap_or(0);
                    start + self.dom.nodes[p].children.iter().take_while(|&&c| c != id).filter(|&&c| self.dom.element(c).is_some_and(|e| e.is("li"))).count()
                }
            };
            self.pending_marker = Some((st.list_style, n, st.clone()));
        }
        let node = if self.depth > 120 {
            self.tree.new_leaf(tstyle).unwrap()
        } else if st.display == D::Table {
            self.table(id, &st, tstyle)
        } else {
            let (kids, ifc) = self.children(id, &st);
            match (kids.is_empty(), ifc) {
                (true, Some(i)) if !matches!(st.display, D::Flex | D::Grid) => self.tree.new_leaf_with_context(tstyle, Leaf::Ifc(i)).unwrap(),
                (_, Some(i)) => {
                    let leaf = self.anon_ifc(i, &st);
                    let mut all = kids;
                    all.push(leaf);
                    self.tree.new_with_children(tstyle, &all).unwrap()
                }
                (_, None) => self.tree.new_with_children(tstyle, &kids).unwrap(),
            }
        };
        self.depth -= 1;
        self.info.insert(node, BoxInfo { node: Some(id), style: st, bg_override: TRANSPARENT });
        node
    }

    fn anon_ifc(&mut self, i: usize, parent: &Arc<Style>) -> NodeId {
        let n = self.tree.new_leaf_with_context(taffy::Style { display: Display::Block, ..Default::default() }, Leaf::Ifc(i)).unwrap();
        let mut s = parent.inherit();
        s.display = D::Block;
        self.info.insert(n, BoxInfo { node: None, style: Arc::new(s), bg_override: TRANSPARENT });
        n
    }

    /// The child boxes of a block container; inline content in between
    /// becomes anonymous blocks. Returns the boxes and a trailing inline
    /// formatting context (not yet a box).
    fn children(&mut self, id: DomId, st: &Arc<Style>) -> (Vec<NodeId>, Option<usize>) {
        let mut kids = Vec::new();
        let mut b = IfcBuilder { raw: Vec::new(), marker: self.pending_marker.take() };
        self.walk(id, st, &mut kids, &mut b, None, TRANSPARENT);
        let ifc = if b.has_content() || b.marker.is_some() { Some(self.make_ifc(b, st)) } else { None };
        (kids, ifc)
    }

    fn make_ifc(&mut self, b: IfcBuilder, st: &Style) -> usize {
        let ifc = build_ifc(b, st, self.fonts, self.viewport);
        self.ifcs.push(ifc);
        self.ifcs.len() - 1
    }

    fn flush(&mut self, st: &Arc<Style>, kids: &mut Vec<NodeId>, b: &mut IfcBuilder) {
        if b.has_content() {
            let raw = std::mem::take(&mut b.raw);
            let marker = b.marker.take();
            let i = self.make_ifc(IfcBuilder { raw, marker }, st);
            let leaf = self.anon_ifc(i, st);
            kids.push(leaf);
        } else {
            b.raw.clear();
        }
    }

    fn walk(&mut self, id: DomId, block: &Arc<Style>, kids: &mut Vec<NodeId>, b: &mut IfcBuilder, link: Option<usize>, bg: Color) {
        let dom = self.dom;
        self.generated(id, 0, block, kids, b, link, bg);
        for &c in &dom.nodes[id].children {
            match &dom.nodes[c].data {
                NodeData::Text(t) => {
                    let st = self.style(c);
                    if !st.hidden {
                        b.raw.push(Raw::Text(t.clone(), st, link, bg));
                    }
                }
                NodeData::Element(e) => {
                    let st = self.style(c);
                    match st.display {
                        D::None => continue,
                        D::Contents => {
                            self.walk(c, block, kids, b, link, bg);
                            continue;
                        }
                        _ => {}
                    }
                    let name: &str = &e.name.local;
                    if is_block_level(st.display) && replaced(name) {
                        if let Some((atom, w, h)) = self.atom(c, name, &st) {
                            self.flush(block, kids, b);
                            let mut t = taffy_style(&st);
                            if matches!(atom, Atom::Image(_) | Atom::Svg(_)) && t.aspect_ratio.is_none() && h > 0.0 {
                                t.aspect_ratio = Some(w / h);
                            }
                            let n = self.tree.new_leaf_with_context(t, Leaf::Atom(atom, w, h)).unwrap();
                            self.info.insert(n, BoxInfo { node: Some(c), style: st.clone(), bg_override: TRANSPARENT });
                            kids.push(n);
                        }
                        continue;
                    }
                    if matches!(st.position, P::Absolute | P::Fixed) && !replaced(name) {
                        // Out of the flow: lines go on around it.
                        let saved = self.pending_marker.take();
                        let n = self.block(c);
                        self.pending_marker = saved;
                        kids.push(n);
                        continue;
                    }
                    // An inline-block as wide as its line might as well be a block
                    // (a percentage needs the line's width, unknown here).
                    let full = st.display == D::InlineBlock && matches!(st.width, Len::Pct(p) if p >= 99.0);
                    if (is_block_level(st.display) || full) && !replaced(name) {
                        // A marker waiting for content goes into the block.
                        if b.marker.is_some() && !b.has_content() {
                            self.pending_marker = b.marker.take();
                        }
                        self.flush(block, kids, b);
                        let n = self.block(c);
                        kids.push(n);
                        continue;
                    }
                    let link = if e.is("a") { e.attr("href").and_then(|h| self.add_link(h)).or(link) } else { link };
                    match name {
                        "br" => b.raw.push(Raw::Break(st)),
                        "img" => {
                            let (w, h) = self.image_size(c, &st);
                            if w > 0.0 && h > 0.0 {
                                let pct = match (st.width, st.max_width) {
                                    (Len::Pct(p), _) => p,
                                    (_, Len::Pct(p)) => -p,
                                    _ => 0.0,
                                };
                                b.raw.push(Raw::Atomic(Atom::Image(c), w, h, h, pct, st, link));
                            } else if let Some(alt) = e.attr("alt").filter(|a| !a.trim().is_empty()) {
                                b.raw.push(Raw::Text(alt.to_string(), st, link, bg));
                            }
                        }
                        "svg" => {
                            let (w, h) = self.svg_size(c, &st);
                            let edge = |i: usize| st.margin[i].px(0.0);
                            let wm = w + edge(style::LEFT) + edge(style::RIGHT);
                            b.raw.push(Raw::Atomic(Atom::Svg(c), wm, h, h, 0.0, st, link));
                        }
                        "input" | "select" | "textarea" => {
                            let Some((atom, cw, ch)) = self.atom(c, name, &st) else { continue };
                            let edge = |i: usize| st.padding[i].px(0.0) + st.border(i);
                            let (pw, ph) = (edge(style::LEFT) + edge(style::RIGHT), edge(style::TOP) + edge(style::BOTTOM));
                            let size = |l: Len, content: f32, pad: f32| match l {
                                Len::Px(v) if st.border_box => v,
                                Len::Px(v) => v + pad,
                                _ => content + pad,
                            };
                            let (w, h) = (size(st.width, cw, pw), size(st.height, ch, ph));
                            let pct = if let Len::Pct(p) = st.width { p } else { 0.0 };
                            let base = h - edge(style::BOTTOM) - 4.0;
                            b.raw.push(Raw::Atomic(atom, w, h, base, pct, st, link));
                        }
                        "video" | "iframe" => {}
                        _ if matches!(st.display, D::InlineBlock | D::InlineFlex | D::InlineGrid) => {
                            let saved = self.pending_marker.take();
                            let sub = self.block(c);
                            self.pending_marker = saved;
                            self.inline_block(sub, st, link, b);
                        }
                        _ => {
                            let bg = if st.background >> 24 != 0 { st.background } else { bg };
                            let edge = |i: usize| st.margin[i].px(0.0) + st.padding[i].px(0.0) + st.border(i);
                            let (l, r) = (edge(style::LEFT), edge(style::RIGHT));
                            if l != 0.0 {
                                b.raw.push(Raw::Gap(l, st.clone(), true));
                            }
                            self.walk(c, block, kids, b, link, bg);
                            if r != 0.0 {
                                b.raw.push(Raw::Gap(r, st.clone(), false));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.generated(id, 1, block, kids, b, link, bg);
    }

    /// An element's ::before (0) or ::after (1).
    #[allow(clippy::too_many_arguments)]
    fn generated(&mut self, id: DomId, which: usize, block: &Arc<Style>, kids: &mut Vec<NodeId>, b: &mut IfcBuilder, link: Option<usize>, bg: Color) {
        let Some((st, text)) = self.styles.generated.get(&id).and_then(|g| g[which].clone()) else { return };
        if st.hidden {
            return;
        }
        let inline = st.display == D::Inline;
        if inline {
            let bg = if st.background >> 24 != 0 { st.background } else { bg };
            b.raw.push(Raw::Text(text, st, link, bg));
            return;
        }
        // A box with the text in it.
        let mut ib = IfcBuilder::default();
        ib.raw.push(Raw::Text(text, st.clone(), None, TRANSPARENT));
        let t = taffy_style(&st);
        let n = if ib.has_content() {
            let i = self.make_ifc(ib, &st);
            self.tree.new_leaf_with_context(t, Leaf::Ifc(i)).unwrap()
        } else {
            self.tree.new_leaf(t).unwrap()
        };
        self.info.insert(n, BoxInfo { node: None, style: st.clone(), bg_override: TRANSPARENT });
        if matches!(st.position, P::Absolute | P::Fixed) {
            kids.push(n);
        } else if is_block_level(st.display) {
            self.flush(block, kids, b);
            kids.push(n);
        } else {
            self.inline_block(n, st, link, b);
        }
    }

    /// Size a separately laid out box and add it to the line.
    fn inline_block(&mut self, sub: NodeId, st: Arc<Style>, link: Option<usize>, b: &mut IfcBuilder) {
        self.tree
            .compute_layout_with_measure(sub, Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent }, measure(&mut self.ifcs, self.fonts))
            .ok();
        let l = *self.tree.layout(sub).unwrap();
        let (mut w, h) = (l.size.width, l.size.height);
        if w > self.viewport {
            // Too wide at max-content: lay it out at the viewport's width.
            self.tree
                .compute_layout_with_measure(sub, Size { width: AvailableSpace::Definite(self.viewport), height: AvailableSpace::MaxContent }, measure(&mut self.ifcs, self.fonts))
                .ok();
            w = self.tree.layout(sub).unwrap().size.width;
        }
        let h = self.tree.layout(sub).map(|l| l.size.height).unwrap_or(h);
        let m = |l: Len| l.px(0.0);
        let w = w + m(st.margin[style::LEFT]) + m(st.margin[style::RIGHT]);
        let base = self.first_baseline(sub).map_or(h, |v| v) + m(st.margin[style::TOP]);
        let h = h + m(st.margin[style::TOP]) + m(st.margin[style::BOTTOM]);
        let base = base.min(h);
        b.raw.push(Raw::Atomic(Atom::Subtree(sub), w, h, base, 0.0, st, link));
    }

    /// The first line's baseline, below the top of `n`.
    fn first_baseline(&mut self, n: NodeId) -> Option<f32> {
        let l = *self.tree.layout(n).ok()?;
        if let Some(Leaf::Ifc(i)) = self.tree.get_node_context(n).cloned() {
            let cw = (l.size.width - l.padding.left - l.padding.right - l.border.left - l.border.right).max(0.0);
            let lines = ifc_layout(&mut self.ifcs[i], cw, self.fonts);
            return lines.frags.first().map(|f| f.baseline + l.padding.top + l.border.top);
        }
        for k in self.tree.children(n).ok()? {
            if self.tree.style(k).is_ok_and(|s| s.position == Position::Absolute) {
                continue;
            }
            if let Some(b) = self.first_baseline(k) {
                return Some(b + self.tree.layout(k).ok()?.location.y);
            }
        }
        None
    }

    /// A replaced element and its content size; None for what shows nothing.
    fn atom(&mut self, id: DomId, name: &str, st: &Style) -> Option<(Atom, f32, f32)> {
        let e = self.dom.element(id).unwrap();
        match name {
            "img" => {
                let (w, h) = self.image_size(id, st);
                (w > 0.0 && h > 0.0).then_some((Atom::Image(id), w, h))
            }
            "svg" => {
                let (w, h) = crate::svg::natural_size(self.dom, id);
                Some((Atom::Svg(id), w, h))
            }
            "input" | "select" | "textarea" => {
                let kind = match name {
                    "select" => ControlKind::Select,
                    "textarea" => ControlKind::TextArea,
                    _ => match e.attr("type").unwrap_or("text").to_ascii_lowercase().as_str() {
                        "hidden" => return None,
                        "submit" | "reset" => ControlKind::Submit,
                        "button" => ControlKind::Button,
                        "checkbox" => ControlKind::Checkbox,
                        "radio" => ControlKind::Radio,
                        "password" => ControlKind::Password,
                        "image" => ControlKind::Image,
                        _ => ControlKind::Text,
                    },
                };
                let (w, h) = self.control_size(id, kind, st);
                Some((Atom::Control(id, kind), w, h))
            }
            _ => None,
        }
    }

    fn svg_size(&self, id: DomId, st: &Style) -> (f32, f32) {
        let (nw, nh) = crate::svg::natural_size(self.dom, id);
        let ratio = if nh > 0.0 { nw / nh } else { 1.0 };
        match (st.width, st.height) {
            (Len::Px(w), Len::Px(h)) => (w, h),
            (Len::Px(w), _) => (w, w / ratio),
            (_, Len::Px(h)) => (h * ratio, h),
            (Len::Pct(p), _) => {
                let w = self.viewport.min(nw) * p / 100.0;
                (w, w / ratio)
            }
            _ => (nw, nh),
        }
    }

    fn image_size(&self, id: DomId, st: &Style) -> (f32, f32) {
        let nat = self.images.get(&id).map(|&(w, h)| (w as f32, h as f32));
        let px = |l: Len, of: f32| match l {
            Len::Px(v) => Some(v),
            Len::Pct(p) => Some(of * p / 100.0),
            Len::Auto => None,
        };
        // A percentage is of the line (see lay_lines).
        let width = if let Len::Pct(_) = st.width { Len::Auto } else { st.width };
        let (w, h) = (px(width, self.viewport), px(st.height, 0.0).filter(|&h| h > 0.0));
        let (w, h) = match (w, h, nat) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some((nw, nh))) if nw > 0.0 => (w, w * nh / nw),
            (None, Some(h), Some((nw, nh))) if nh > 0.0 => (h * nw / nh, h),
            (None, None, Some(n)) => n,
            (Some(w), None, None) => (w, w.min(32.0)),
            (None, Some(h), None) => (h.min(32.0), h),
            _ => (0.0, 0.0),
        };
        // max-width: 100% and friends.
        let maxw = match st.max_width {
            Len::Px(v) => v,
            Len::Pct(_) | Len::Auto => f32::MAX,
        };
        if w > maxw && w > 0.0 { (maxw, h * maxw / w) } else { (w, h) }
    }

    fn control_size(&mut self, id: DomId, kind: ControlKind, st: &Style) -> (f32, f32) {
        let e = self.dom.element(id).unwrap();
        let f = font_of(st);
        let (a, d) = self.fonts.metrics(f);
        let h = a + d + 2.0;
        let label = |s: &mut Self| -> String {
            match kind {
                ControlKind::Select => {
                    let opts: Vec<DomId> = s.dom.descendants(id).filter(|&n| s.dom.element(n).is_some_and(|e| e.is("option"))).collect();
                    let sel = opts.iter().copied().find(|&o| s.dom.element(o).unwrap().attr("selected").is_some()).or(opts.first().copied());
                    // The widest option sets the width.
                    opts.iter().map(|&o| s.dom.text_content(o).trim().to_string()).max_by_key(|t| t.len()).or(sel.map(|o| s.dom.text_content(o)))
                        .unwrap_or_default()
                }
                _ => e.attr("value").map(String::from).unwrap_or_else(|| if kind == ControlKind::Submit { "Submit".into() } else { String::new() }),
            }
        };
        match kind {
            ControlKind::Checkbox | ControlKind::Radio => (13.0, 13.0),
            ControlKind::Text | ControlKind::Password => {
                let size: f32 = e.attr("size").and_then(|s| s.parse().ok()).unwrap_or(20.0);
                (self.fonts.width(f, "n") * size, h)
            }
            ControlKind::TextArea => {
                let cols: f32 = e.attr("cols").and_then(|s| s.parse().ok()).unwrap_or(20.0);
                let rows: f32 = e.attr("rows").and_then(|s| s.parse().ok()).unwrap_or(2.0);
                (self.fonts.width(f, "n") * cols, (a + d) * rows)
            }
            ControlKind::Image => (32.0, 32.0),
            ControlKind::Select => {
                let t = label(self);
                (self.fonts.width(f, &t) + 28.0, h)
            }
            _ => {
                let t = label(self);
                (self.fonts.width(f, &t), h)
            }
        }
    }

    /// A table as a grid: a track per column, cells placed by row.
    fn table(&mut self, id: DomId, st: &Arc<Style>, mut t: taffy::Style) -> NodeId {
        let dom = self.dom;
        let mut rows: Vec<DomId> = Vec::new();
        let mut caption = None;
        let mut loose = false;
        for &c in &dom.nodes[id].children {
            if dom.element(c).is_none() {
                continue;
            }
            match self.style(c).display {
                D::TableRow => rows.push(c),
                D::TableRowGroup => rows.extend(dom.nodes[c].children.iter().copied().filter(|&r| self.style(r).display == D::TableRow && dom.element(r).is_some())),
                D::TableCaption => caption = Some(c),
                D::TableCell => loose = true,
                _ => {}
            }
        }
        // Cells straight in the table are a row of their own.
        if loose {
            rows.insert(0, id);
        }
        let mut kids = Vec::new();
        let mut ncols = 1u16;
        let mut row_index = 1i16;
        if let Some(c) = caption {
            let n = self.block(c);
            kids.push((n, 0i16, 0u16, 0u16));
        }
        for &r in &rows {
            let rst = self.style(r);
            if rst.display == D::None {
                continue;
            }
            let row_bg = if r == id { TRANSPARENT } else { rst.background };
            let mut col = 1u16;
            for &c in &dom.nodes[r].children {
                let Some(e) = dom.element(c) else { continue };
                if self.style(c).display != D::TableCell {
                    continue;
                }
                let span: u16 = e.attr("colspan").and_then(|s| s.trim().parse().ok()).unwrap_or(1u16).clamp(1, 64);
                let n = self.block(c);
                if row_bg >> 24 != 0 {
                    if let Some(i) = self.info.get_mut(&n) {
                        i.bg_override = row_bg;
                    }
                }
                kids.push((n, row_index, col, span));
                col += span;
            }
            ncols = ncols.max(col - 1);
            row_index += 1;
        }
        let mut nodes = Vec::new();
        for (n, row, col, span) in kids {
            let mut s = self.tree.style(n).unwrap().clone();
            if row == 0 {
                s.grid_row = Line { start: style_helpers::line(1), end: style_helpers::span(1) };
                s.grid_column = Line { start: style_helpers::line(1), end: GridPlacement::Line((ncols as i16 + 1).into()) };
            } else {
                let r = row + if caption.is_some() { 1 } else { 0 };
                s.grid_row = Line { start: style_helpers::line(r), end: style_helpers::span(1) };
                s.grid_column = Line { start: style_helpers::line(col as i16), end: style_helpers::span(span) };
            }
            self.tree.set_style(n, s).ok();
            nodes.push(n);
        }
        t.display = Display::Grid;
        t.grid_template_columns = (0..ncols).map(|_| GridTemplateComponent::Single(style_helpers::auto())).collect();
        let gap = if st.border_collapse { 0.0 } else { st.border_spacing };
        t.gap = Size { width: LengthPercentage::length(gap), height: LengthPercentage::length(gap) };
        t.justify_content = if st.width == Len::Auto { JustifyContent::START } else { JustifyContent::STRETCH };
        t.align_items = AlignItems::STRETCH;
        self.tree.new_with_children(t, &nodes).unwrap()
    }
}

fn measure<'a>(
    ifcs: &'a mut Vec<Ifc>,
    fonts: &'a mut dyn Fonts,
) -> impl FnMut(taffy::LayoutInput, NodeId, Option<&mut Leaf>, &taffy::Style) -> taffy::LayoutOutput + 'a {
    move |inputs, _node, ctx, style| {
        taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, avail| match ctx.as_deref() {
            Some(Leaf::Ifc(i)) => {
                let ifc = &mut ifcs[*i];
                let w = match known.width {
                    Some(w) => w,
                    None => match avail.width {
                        AvailableSpace::MinContent => ifc.min_content,
                        AvailableSpace::MaxContent => ifc.max_content,
                        AvailableSpace::Definite(w) => ifc.max_content.min(w).max(ifc.min_content.min(w)),
                    },
                };
                let h = ifc_layout(ifc, w, fonts).height;
                if std::env::var("WEBCORE_MEASURE").is_ok() {
                    eprintln!("measure ifc {i} known {known:?} avail {avail:?} -> {w} x {h} ({} pieces, strut {:?})", ifc.pieces.len(), ifc.strut);
                }
                Size { width: w, height: known.height.unwrap_or(h) }
            }
            Some(Leaf::Atom(_, w, h)) => Size { width: known.width.unwrap_or(*w), height: known.height.unwrap_or(*h) },
            None => Size { width: known.width.unwrap_or(0.0), height: known.height.unwrap_or(0.0) },
        })
    }
}

/// Lay out a document at `width` pels.
pub fn layout(dom: &Dom, styles: &Styles, base: &Url, width: f32, fonts: &mut dyn Fonts, images: &ImageSizes) -> Laid {
    let mut b = Builder {
        dom,
        styles,
        fonts,
        tree: { let mut t = TaffyTree::new(); t.disable_rounding(); t },
        info: HashMap::new(),
        ifcs: Vec::new(),
        base: base.clone(),
        links: Vec::new(),
        images,
        viewport: width,
        pending_marker: None,
        depth: 0,
    };
    if let Some(h) = dom.find("base").and_then(|n| dom.element(n)).and_then(|e| e.attr("href")) {
        if let Ok(u) = base.join(h) {
            b.base = u;
        }
    }
    let html = dom.nodes[Dom::DOCUMENT].children.iter().copied().find(|&c| dom.element(c).is_some());
    let mut laid = Laid { items: Vec::new(), link_rects: Vec::new(), links: Vec::new(), width, height: 0.0, background: 0xffff_ffff, anchors: Vec::new() };
    let Some(html) = html else { return laid };
    let root = b.block(html);
    let mut rs = b.tree.style(root).unwrap().clone();
    rs.size.width = Dimension::length(width);
    b.tree.set_style(root, rs).ok();
    b.tree
        .compute_layout_with_measure(root, Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent }, measure(&mut b.ifcs, b.fonts))
        .ok();
    // Floats beside inline content: lay out again with the lines beside
    // them shortened.
    let mut floats = Vec::new();
    let mut leaves = Vec::new();
    let mut order = 0;
    gather(&b, root, 0.0, 0.0, &mut order, &mut floats, &mut leaves);
    if !floats.is_empty() {
        let mut changed = false;
        for (i, node, c, o) in &leaves {
            let mut e = Vec::new();
            for (f, left, fo, fend) in &floats {
                if fo >= o || (o > fo && o <= fend) {
                    continue;
                }
                if f.x < c.x + c.w && f.x + f.w > c.x && f.y < c.y + c.h + 2000.0 && f.y + f.h > c.y {
                    let li = if *left { (f.x + f.w - c.x).max(0.0) } else { 0.0 };
                    let ri = if *left { 0.0 } else { (c.x + c.w - f.x).max(0.0) };
                    e.push((f.y - c.y, f.y + f.h - c.y, li, ri));
                }
            }
            if !e.is_empty() {
                b.ifcs[*i].excl = e;
                b.ifcs[*i].cache.clear();
                b.tree.mark_dirty(*node).ok();
                changed = true;
            }
        }
        if changed {
            b.tree
                .compute_layout_with_measure(root, Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent }, measure(&mut b.ifcs, b.fonts))
                .ok();
        }
    }
    if std::env::var("WEBCORE_TREE").is_ok() {
        b.tree.print_tree(root);
    }
    // The canvas: the root's background, else the body's.
    let hs = b.style(html);
    let body = dom.find("body");
    let bodys = body.map(|n| b.style(n));
    laid.background = if hs.background >> 24 != 0 {
        hs.background
    } else {
        bodys.as_ref().map(|s| s.background).filter(|c| c >> 24 != 0).unwrap_or(0xffff_ffff)
    };
    let page_bg = laid.background;
    let mut p = Painter { bg: page_bg, b: &mut b, laid: &mut laid, skip_bg: [Some(html), if hs.background >> 24 != 0 { None } else { body }] };
    p.paint(root, 0.0, 0.0, 0);
    let rl = b.tree.layout(root).unwrap();
    laid.height = laid.height.max(rl.size.height);
    laid.links = b.links;
    laid
}

/// Floats (margin boxes, left?, tree order, end of their subtree's order)
/// and inline formatting contexts (index, node, content box, order), in
/// page coordinates.
#[allow(clippy::type_complexity)]
fn gather(b: &Builder, n: NodeId, ox: f32, oy: f32, order: &mut usize, floats: &mut Vec<(Rect, bool, usize, usize)>, leaves: &mut Vec<(usize, NodeId, Rect, usize)>) {
    let Ok(l) = b.tree.layout(n) else { return };
    let (x, y) = (ox + l.location.x, oy + l.location.y);
    *order += 1;
    let me = *order;
    let float = b.info.get(&n).map(|i| i.style.float).unwrap_or(style::Float::None);
    if let Some(Leaf::Ifc(i)) = b.tree.get_node_context(n) {
        let c = Rect {
            x: x + l.padding.left + l.border.left,
            y: y + l.padding.top + l.border.top,
            w: (l.size.width - l.padding.left - l.padding.right - l.border.left - l.border.right).max(0.0),
            h: (l.size.height - l.padding.top - l.padding.bottom - l.border.top - l.border.bottom).max(0.0),
        };
        leaves.push((*i, n, c, me));
    }
    for k in b.tree.children(n).unwrap_or_default() {
        gather(b, k, x, y, order, floats, leaves);
    }
    if float != style::Float::None {
        let r = Rect { x: x - l.margin.left, y: y - l.margin.top, w: l.size.width + l.margin.left + l.margin.right, h: l.size.height + l.margin.top + l.margin.bottom };
        floats.push((r, float == style::Float::Left, me, *order));
    }
}

struct Painter<'a, 'b> {
    b: &'a mut Builder<'b>,
    laid: &'a mut Laid,
    skip_bg: [Option<DomId>; 2],
    /// The colour behind what is being painted.
    bg: Color,
}

fn blend(c: Color) -> Option<Color> {
    if c >> 24 == 0 { None } else { Some(c) }
}

impl Painter<'_, '_> {
    fn paint(&mut self, n: NodeId, ox: f32, oy: f32, depth: u32) {
        if depth > 400 {
            return;
        }
        let l = *self.b.tree.layout(n).unwrap();
        let x = ox + l.location.x;
        let y = oy + l.location.y;
        let (w, h) = (l.size.width, l.size.height);
        self.laid.height = self.laid.height.max(y + h);
        let info = self.b.info.get(&n).map(|i| (i.node, i.style.clone(), i.bg_override));
        let mut clip = false;
        let outer_bg = self.bg;
        if let Some((node, st, bgo)) = &info {
            if st.display == D::None || st.invisible {
                return;
            }
            if st.overflow_hidden && (w <= 1.5 || h <= 1.5) {
                return;
            }
            if let Some(id) = node {
                if let Some(a) = self.b.dom.element(*id).and_then(|e| e.attr("id").or_else(|| if e.is("a") { e.attr("name") } else { None })) {
                    self.laid.anchors.push((a.to_string(), y));
                }
            }
            let visible = !st.hidden;
            if visible {
                let skip = node.is_some() && self.skip_bg.contains(node);
                let bg = blend(st.background).or(blend(*bgo));
                let rect = Rect { x, y, w, h };
                let radii = st.radii(w, h);
                let round = radii.iter().any(|&v| v >= 1.0);
                if let (Some(c), false) = (bg, skip) {
                    if c >> 24 >= 200 {
                        self.bg = c;
                    }
                    self.laid.items.push(if round { Item::RoundRect { r: rect, color: c, radii, ring: 0.0 } } else { Item::Fill { r: rect, color: c } });
                }
                // Borders as four rectangles, or one ring if alike.
                let bw = [st.border(style::TOP), st.border(style::RIGHT), st.border(style::BOTTOM), st.border(style::LEFT)];
                if bw.iter().any(|&v| v > 0.0) {
                    let colors = [st.border_color(0), st.border_color(1), st.border_color(2), st.border_color(3)];
                    if round && bw.iter().all(|&v| v == bw[0]) && colors.iter().all(|&c| c == colors[0]) {
                        if let Some(c) = blend(colors[0]) {
                            self.laid.items.push(Item::RoundRect { r: rect, color: c, radii, ring: bw[0] });
                        }
                    } else {
                        let rects = [
                            Rect { x, y, w, h: bw[0] },
                            Rect { x: x + w - bw[1], y, w: bw[1], h },
                            Rect { x, y: y + h - bw[2], w, h: bw[2] },
                            Rect { x, y, w: bw[3], h },
                        ];
                        for (i, r) in rects.iter().enumerate() {
                            if bw[i] > 0.0 {
                                if let Some(c) = blend(colors[i]) {
                                    self.laid.items.push(Item::Fill { r: *r, color: c });
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some((_, st, _)) = &info {
            if st.overflow_hidden && depth > 0 {
                clip = true;
                let r = Rect { x: x + l.border.left, y: y + l.border.top, w: (w - l.border.left - l.border.right).max(0.0), h: (h - l.border.top - l.border.bottom).max(0.0) };
                self.laid.items.push(Item::PushClip { r });
            }
        }
        if let Some(Leaf::Ifc(i)) = self.b.tree.get_node_context(n).cloned() {
            let cx = x + l.padding.left + l.border.left;
            let cy = y + l.padding.top + l.border.top;
            let cw = (w - l.padding.left - l.padding.right - l.border.left - l.border.right).max(0.0);
            self.inline(i, cx, cy, cw, depth);
        }
        if let Some(Leaf::Atom(atom, ..)) = self.b.tree.get_node_context(n).cloned() {
            if let Some((_, st, _)) = &info {
                let inner = Rect {
                    x: x + l.padding.left + l.border.left,
                    y: y + l.padding.top + l.border.top,
                    w: (w - l.padding.left - l.padding.right - l.border.left - l.border.right).max(0.0),
                    h: (h - l.padding.top - l.padding.bottom - l.border.top - l.border.bottom).max(0.0),
                };
                if !st.hidden {
                    let boxr = Rect { x, y, w, h };
                    self.atom(&atom, if matches!(atom, Atom::Control(..)) { boxr } else { inner }, st, l.padding.left + l.border.left);
                }
            }
        }
        if let Some((Some(id), st, _)) = &info {
            if self.b.dom.element(*id).is_some_and(|e| e.is("button")) && !st.hidden {
                self.laid.items.push(Item::Control { r: Rect { x, y, w, h }, node: *id, kind: ControlKind::ButtonBox, font: font_of(st), color: st.color, pad: 0.0 });
            }
        }
        let kids: Vec<NodeId> = self.b.tree.children(n).unwrap_or_default();
        for k in kids {
            self.paint(k, x, y, depth + 1);
        }
        if clip {
            self.laid.items.push(Item::PopClip);
        }
        self.bg = outer_bg;
    }

    /// Paint a replaced element in `r`; `pad` is a control's left padding.
    fn atom(&mut self, atom: &Atom, r: Rect, st: &Style, pad: f32) {
        match atom {
            Atom::Image(node) => {
                self.laid.items.push(Item::Image { r, node: *node });
                let radii = st.radii(r.w, r.h);
                if radii.iter().any(|&v| v >= 1.0) {
                    self.laid.items.push(Item::Corners { r, color: self.bg, radii });
                }
            }
            Atom::Svg(node) => self.laid.items.push(Item::Svg { r, node: *node, bg: self.bg }),
            Atom::Control(node, kind) => self.laid.items.push(Item::Control { r, node: *node, kind: *kind, font: font_of(st), color: st.color, pad }),
            Atom::Subtree(_) => {}
        }
    }

    /// An inline control's box (block-level ones are boxes of their own).
    fn control_box(&mut self, r: Rect, st: &Style) {
        let radii = st.radii(r.w, r.h);
        let round = radii.iter().any(|&v| v >= 1.0);
        if let Some(c) = blend(st.background) {
            self.laid.items.push(if round { Item::RoundRect { r, color: c, radii, ring: 0.0 } } else { Item::Fill { r, color: c } });
        }
        let bw = st.border(style::TOP).max(st.border(style::LEFT));
        if let (true, Some(c)) = (bw > 0.0, blend(st.border_color(style::TOP))) {
            self.laid.items.push(Item::RoundRect { r, color: c, radii, ring: bw });
        }
    }

    fn inline(&mut self, i: usize, cx: f32, cy: f32, cw: f32, depth: u32) {
        let lines = ifc_layout(&mut self.b.ifcs[i], cw, self.b.fonts).clone();
        let pieces = self.b.ifcs[i].pieces.clone();
        let strut_d = self.b.ifcs[i].strut.2;
        for f in &lines.frags {
            let p = &pieces[f.piece];
            // font-size: 0 hides text.
            if p.style.hidden || (p.style.font_size < 1.5 && matches!(p.kind, Kind::Word | Kind::Space)) {
                continue;
            }
            let fx = cx + f.x;
            match &p.kind {
                Kind::Word | Kind::Space => {
                    let (a, d) = self.b.fonts.metrics(p.font);
                    let shift = match p.style.vertical_align {
                        4 => p.style.font_size * 0.3,
                        5 => -p.style.font_size * 0.4,
                        _ => 0.0,
                    };
                    let base = cy + f.baseline + shift;
                    if let Some(c) = blend(p.bg) {
                        self.laid.items.push(Item::Fill { r: Rect { x: fx, y: base - a, w: f.w, h: a + d }, color: c });
                    }
                    self.laid.items.push(Item::Text { x: fx, y: base, w: f.w, text: f.text.clone(), font: p.font, color: p.style.color, decoration: p.style.decoration });
                    if let Some(li) = p.link {
                        self.laid.link_rects.push((Rect { x: fx, y: cy + f.top, w: f.w, h: f.line_h }, li));
                    }
                }
                Kind::Marker(kind) => {
                    if f.text.is_empty() {
                        let s = p.style.font_size * 0.36;
                        let r = Rect { x: fx + p.w * 0.25, y: cy + f.baseline - p.style.font_size * 0.32 - s / 2.0, w: s, h: s };
                        self.laid.items.push(Item::Marker { r, kind: *kind, color: p.style.color });
                    } else {
                        self.laid.items.push(Item::Text { x: fx, y: cy + f.baseline, w: f.w, text: f.text.clone(), font: p.font, color: p.style.color, decoration: 0 });
                    }
                }
                Kind::Atomic(atom, w, _, _, pct) => {
                    if p.style.invisible {
                        continue;
                    }
                    let w = if *pct != 0.0 { &f.w } else { w };
                    let (h, base) = atomic_height(p, *w);
                    let (h, base) = (&h, &base);
                    let top = match p.style.vertical_align {
                        1 => cy + f.top + (f.line_h - h) / 2.0,
                        2 => cy + f.top,
                        3 => cy + f.baseline + strut_d - h,
                        _ => cy + f.baseline - base,
                    };
                    let r = Rect { x: fx, y: top, w: *w, h: *h };
                    let st = p.style.clone();
                    match atom {
                        Atom::Control(..) => {
                            self.control_box(r, &st);
                            self.atom(atom, r, &st, st.padding[style::LEFT].px(0.0) + st.border(style::LEFT));
                        }
                        Atom::Svg(_) => {
                            let ml = st.margin[style::LEFT].px(0.0);
                            let mr = st.margin[style::RIGHT].px(0.0);
                            self.atom(atom, Rect { x: r.x + ml, w: r.w - ml - mr, ..r }, &st, 0.0);
                        }
                        Atom::Image(_) => self.atom(atom, r, &st, 0.0),
                        Atom::Subtree(sub) => {
                            let ml = p.style.margin[style::LEFT].px(0.0);
                            let mt = p.style.margin[style::TOP].px(0.0);
                            let l = *self.b.tree.layout(*sub).unwrap();
                            self.paint(*sub, fx + ml - l.location.x, top + mt - l.location.y, depth + 1);
                        }
                    }
                    if let Some(li) = p.link {
                        self.laid.link_rects.push((r, li));
                    }
                }
                Kind::Break | Kind::Gap => {}
            }
        }
    }
}
