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
}

#[derive(Clone, Debug)]
pub enum Item {
    Fill { r: Rect, color: Color },
    Text { x: f32, y: f32, w: f32, text: String, font: FontKey, color: Color, decoration: u8 },
    Image { r: Rect, node: DomId },
    Marker { r: Rect, kind: ListStyle, color: Color },
    Control { r: Rect, node: DomId, kind: ControlKind, font: FontKey },
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
    Atomic(Atom, f32, f32),
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
    Atomic(Atom, f32, f32, Arc<Style>, Option<usize>),
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
            Raw::Atomic(atom, w, h, st, link) => {
                pieces.push(Piece { kind: Kind::Atomic(atom, w, h), text: String::new(), w, font: font_of(&st), style: st, link, bg: TRANSPARENT, wrap: true });
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
            Kind::Atomic(..) => {
                min_content = min_content.max(p.w);
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
    };
    ifc.max_content = lay_lines(&ifc, None, fonts).width;
    ifc
}

fn lay_lines(ifc: &Ifc, width: Option<f32>, fonts: &mut dyn Fonts) -> Lines {
    let mut out = Lines::default();
    let mut line: Vec<(usize, f32, String, f32)> = Vec::new(); // piece, x, text, w
    let mut x = ifc.indent;
    let mut pending = 0.0f32;
    let mut y = 0.0f32;
    let pieces = &ifc.pieces;
    for (i, p) in pieces.iter().enumerate() {
        match &p.kind {
            Kind::Break => {
                finish(ifc, width, fonts, &mut line, x, &mut y, &mut out);
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
            _ => {
                let can_break = p.wrap && !line.is_empty() && (pending > 0.0 || matches!(p.kind, Kind::Atomic(..)) || matches!(pieces[i - 1].kind, Kind::Atomic(..)));
                if let Some(w) = width {
                    if can_break && x + pending + p.w > w + 0.5 {
                        finish(ifc, width, fonts, &mut line, x, &mut y, &mut out);
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
                                finish(ifc, width, fonts, &mut line, x, &mut y, &mut out);
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
        finish(ifc, width, fonts, &mut line, x, &mut y, &mut out);
    }
    out.height = y;
    out
}

fn finish(ifc: &Ifc, width: Option<f32>, fonts: &mut dyn Fonts, line: &mut Vec<(usize, f32, String, f32)>, x: f32, y: &mut f32, out: &mut Lines) {
    let pieces = &ifc.pieces;
    let (lh, sa, sd) = ifc.strut;
    let hl = (lh - (sa + sd)) / 2.0;
    let mut above = sa + hl;
    let mut below = sd + hl;
    for (i, ..) in line.iter() {
        let p = &pieces[*i];
        match &p.kind {
            Kind::Atomic(_, _, h) => {
                if p.style.vertical_align == 1 {
                    above = above.max(h / 2.0 + sa * 0.3);
                    below = below.max(h / 2.0 - sa * 0.3);
                } else {
                    above = above.max(*h);
                }
            }
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
        out.frags.push(Frag { x: fx + shift, top: *y, baseline: *y + above, line_h: h, piece: i, text, w });
    }
    out.width = out.width.max(used);
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
    if s.grid_column_span > 1 {
        t.grid_column = Line { start: style_helpers::span(s.grid_column_span), end: GridPlacement::Auto };
    }
    t
}

fn is_block_level(d: D) -> bool {
    matches!(d, D::Block | D::ListItem | D::Flex | D::Grid | D::Table | D::TableRow | D::TableCell | D::TableRowGroup | D::TableCaption)
}

fn replaced(name: &str) -> bool {
    matches!(name, "img" | "input" | "button" | "select" | "textarea" | "video" | "iframe")
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
                    if is_block_level(st.display) && !replaced(name) {
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
                                b.raw.push(Raw::Atomic(Atom::Image(c), w, h, st, link));
                            } else if let Some(alt) = e.attr("alt").filter(|a| !a.trim().is_empty()) {
                                b.raw.push(Raw::Text(alt.to_string(), st, link, bg));
                            }
                        }
                        "input" | "button" | "select" | "textarea" => {
                            let kind = match name {
                                "button" => ControlKind::Button,
                                "select" => ControlKind::Select,
                                "textarea" => ControlKind::TextArea,
                                _ => match e.attr("type").unwrap_or("text").to_ascii_lowercase().as_str() {
                                    "hidden" => continue,
                                    "submit" | "reset" => ControlKind::Submit,
                                    "button" => ControlKind::Button,
                                    "checkbox" => ControlKind::Checkbox,
                                    "radio" => ControlKind::Radio,
                                    "password" => ControlKind::Password,
                                    "image" => ControlKind::Image,
                                    _ => ControlKind::Text,
                                },
                            };
                            let (w, h) = self.control_size(c, kind, &st);
                            b.raw.push(Raw::Atomic(Atom::Control(c, kind), w, h, st, link));
                        }
                        "video" | "iframe" => {}
                        _ if matches!(st.display, D::InlineBlock | D::InlineFlex | D::InlineGrid) => {
                            let saved = self.pending_marker.take();
                            let sub = self.block(c);
                            self.pending_marker = saved;
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
                            let h = h + m(st.margin[style::TOP]) + m(st.margin[style::BOTTOM]);
                            b.raw.push(Raw::Atomic(Atom::Subtree(sub), w, h, st, link));
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
    }

    fn image_size(&self, id: DomId, st: &Style) -> (f32, f32) {
        let nat = self.images.get(&id).map(|&(w, h)| (w as f32, h as f32));
        let px = |l: Len, of: f32| match l {
            Len::Px(v) => Some(v),
            Len::Pct(p) => Some(of * p / 100.0),
            Len::Auto => None,
        };
        let (w, h) = (px(st.width, self.viewport), px(st.height, 0.0).filter(|&h| h > 0.0));
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
            Len::Pct(p) => self.viewport * p / 100.0,
            Len::Auto => f32::MAX,
        };
        if w > maxw && w > 0.0 { (maxw, h * maxw / w) } else { (w, h) }
    }

    fn control_size(&mut self, id: DomId, kind: ControlKind, st: &Style) -> (f32, f32) {
        let e = self.dom.element(id).unwrap();
        let f = font_of(st);
        let (a, d) = self.fonts.metrics(f);
        let h = a + d + 8.0;
        let label = |s: &mut Self| -> String {
            match kind {
                ControlKind::Button => s.dom.text_content(id).trim().to_string(),
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
        let px = |l: Len| if let Len::Px(v) = l { Some(v) } else { None };
        let (w, h) = match kind {
            ControlKind::Checkbox | ControlKind::Radio => (13.0, 13.0),
            ControlKind::Text | ControlKind::Password => {
                let size: f32 = e.attr("size").and_then(|s| s.parse().ok()).unwrap_or(20.0);
                (self.fonts.width(f, "n") * size + 8.0, h)
            }
            ControlKind::TextArea => {
                let cols: f32 = e.attr("cols").and_then(|s| s.parse().ok()).unwrap_or(20.0);
                let rows: f32 = e.attr("rows").and_then(|s| s.parse().ok()).unwrap_or(2.0);
                (self.fonts.width(f, "n") * cols + 8.0, (a + d) * rows + 8.0)
            }
            ControlKind::Image => (32.0, 32.0),
            ControlKind::Select => {
                let t = label(self);
                (self.fonts.width(f, &t) + 28.0, h)
            }
            _ => {
                let t = label(self);
                (self.fonts.width(f, &t) + 16.0, h)
            }
        };
        (px(st.width).unwrap_or(w), px(st.height).unwrap_or(h))
    }

    /// A table as a grid: a track per column, cells placed by row.
    fn table(&mut self, id: DomId, st: &Arc<Style>, mut t: taffy::Style) -> NodeId {
        let dom = self.dom;
        let mut rows: Vec<DomId> = Vec::new();
        let mut caption = None;
        let mut collect = |n: DomId, rows: &mut Vec<DomId>| {
            for &c in &dom.nodes[n].children {
                if let Some(e) = dom.element(c) {
                    match &*e.name.local {
                        "tr" => rows.push(c),
                        "thead" | "tbody" | "tfoot" => {
                            for &r in &dom.nodes[c].children {
                                if dom.element(r).is_some_and(|e| e.is("tr")) {
                                    rows.push(r);
                                }
                            }
                        }
                        "caption" => caption = Some(c),
                        _ => {}
                    }
                }
            }
        };
        collect(id, &mut rows);
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
            let mut col = 1u16;
            for &c in &dom.nodes[r].children {
                let Some(e) = dom.element(c) else { continue };
                if !(e.is("td") || e.is("th")) || self.style(c).display == D::None {
                    continue;
                }
                let span: u16 = e.attr("colspan").and_then(|s| s.trim().parse().ok()).unwrap_or(1u16).clamp(1, 64);
                let n = self.block(c);
                if rst.background >> 24 != 0 {
                    if let Some(i) = self.info.get_mut(&n) {
                        i.bg_override = rst.background;
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
        t.gap = Size { width: LengthPercentage::length(2.0), height: LengthPercentage::length(2.0) };
        t.justify_content = JustifyContent::START;
        t.align_items = AlignItems::STRETCH;
        let _ = st;
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
    let mut p = Painter { b: &mut b, laid: &mut laid, skip_bg: [Some(html), if hs.background >> 24 != 0 { None } else { body }] };
    p.paint(root, 0.0, 0.0, 0);
    let rl = b.tree.layout(root).unwrap();
    laid.height = laid.height.max(rl.size.height);
    laid.links = b.links;
    laid
}

struct Painter<'a, 'b> {
    b: &'a mut Builder<'b>,
    laid: &'a mut Laid,
    skip_bg: [Option<DomId>; 2],
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
        if let Some((node, st, bgo)) = &info {
            if st.display == D::None {
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
                if let (Some(c), false) = (bg, skip) {
                    self.laid.items.push(Item::Fill { r: Rect { x, y, w, h }, color: c });
                }
                // Borders as four rectangles.
                let bw = [st.border(style::TOP), st.border(style::RIGHT), st.border(style::BOTTOM), st.border(style::LEFT)];
                if bw.iter().any(|&v| v > 0.0) {
                    let rects = [
                        Rect { x, y, w, h: bw[0] },
                        Rect { x: x + w - bw[1], y, w: bw[1], h },
                        Rect { x, y: y + h - bw[2], w, h: bw[2] },
                        Rect { x, y, w: bw[3], h },
                    ];
                    for (i, r) in rects.iter().enumerate() {
                        if bw[i] > 0.0 {
                            if let Some(c) = blend(st.border_color(i)) {
                                self.laid.items.push(Item::Fill { r: *r, color: c });
                            }
                        }
                    }
                }
            }
        }
        if let Some(Leaf::Ifc(i)) = self.b.tree.get_node_context(n).cloned() {
            let cx = x + l.padding.left + l.border.left;
            let cy = y + l.padding.top + l.border.top;
            let cw = (w - l.padding.left - l.padding.right - l.border.left - l.border.right).max(0.0);
            self.inline(i, cx, cy, cw, depth);
        }
        let kids: Vec<NodeId> = self.b.tree.children(n).unwrap_or_default();
        for k in kids {
            self.paint(k, x, y, depth + 1);
        }
    }

    fn inline(&mut self, i: usize, cx: f32, cy: f32, cw: f32, depth: u32) {
        let lines = ifc_layout(&mut self.b.ifcs[i], cw, self.b.fonts).clone();
        let pieces = self.b.ifcs[i].pieces.clone();
        for f in &lines.frags {
            let p = &pieces[f.piece];
            if p.style.hidden {
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
                Kind::Atomic(atom, w, h) => {
                    let top = if p.style.vertical_align == 1 { cy + f.top + (f.line_h - h) / 2.0 } else { cy + f.baseline - h };
                    let r = Rect { x: fx, y: top, w: *w, h: *h };
                    match atom {
                        Atom::Image(node) => self.laid.items.push(Item::Image { r, node: *node }),
                        Atom::Control(node, kind) => self.laid.items.push(Item::Control { r, node: *node, kind: *kind, font: p.font }),
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
