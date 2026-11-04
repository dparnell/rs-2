//! Computed styles and the cascade.
//!
//! Each element's declarations are gathered from the user-agent sheet, the
//! document's sheets and its `style` attribute, sorted by origin,
//! importance, specificity and order, and applied on top of what it
//! inherits from its parent. Custom properties (`--x`) are inherited and
//! substituted into `var()` references as text.

use std::collections::HashMap;
use std::sync::Arc;

use selectors::matching::{MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, SelectorCaches};
use selectors::context::QuirksMode;

use crate::css::{self, Color, Decl, Origin, Rules, Tok, BLACK, TRANSPARENT};
use crate::dom::{Dom, NodeData, NodeId};
use crate::select::El;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    Table,
    TableRow,
    TableCell,
    TableRowGroup,
    TableCaption,
    Contents,
}

impl Display {
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Len {
    Auto,
    Px(f32),
    Pct(f32),
}

impl Len {
    /// Resolved against a containing size (Auto is 0).
    pub fn px(self, of: f32) -> f32 {
        match self {
            Len::Auto => 0.0,
            Len::Px(v) => v,
            Len::Pct(p) => of * p / 100.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Family {
    Sans,
    Serif,
    Mono,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhiteSpace {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Float {
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LineHeight {
    Normal,
    Factor(f32),
    Px(f32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListStyle {
    None,
    Disc,
    Circle,
    Square,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

/// An SVG fill or stroke.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Paint {
    None,
    Color(Color),
    Current,
}

/// A grid track size.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Track {
    Len(Len),
    Fr(f32),
    MinContent,
    MaxContent,
}

pub const UNDERLINE: u8 = 1;
pub const LINE_THROUGH: u8 = 2;
pub const OVERLINE: u8 = 4;

#[derive(Clone, Debug)]
pub struct Style {
    pub display: Display,
    pub position: Position,
    pub float: Float,
    pub clear: bool,
    pub color: Color,
    pub background: Color,
    pub background_image: Option<String>,
    pub family: Family,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub line_height: LineHeight,
    pub text_align: Align,
    pub decoration: u8,
    pub text_transform: u8,
    pub white_space: WhiteSpace,
    pub text_indent: Len,
    pub margin: [Len; 4],
    pub padding: [Len; 4],
    pub border_width: [f32; 4],
    pub border_visible: [bool; 4],
    pub border_color: [Option<Color>; 4],
    pub width: Len,
    pub height: Len,
    pub min_width: Len,
    pub min_height: Len,
    pub max_width: Len,
    pub max_height: Len,
    pub border_box: bool,
    pub list_style: ListStyle,
    pub hidden: bool,
    pub flex_row: bool,
    pub flex_reverse: bool,
    pub flex_wrap: bool,
    pub justify: u8,
    pub align_items: u8,
    pub align_self: u8,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Len,
    pub gap: (Len, Len),
    pub order: i32,
    pub inset: [Len; 4],
    pub overflow_hidden: bool,
    /// Not drawn with what it holds (opacity 0, clipped away).
    pub invisible: bool,
    pub vertical_align: u8,
    pub grid_columns: Vec<Track>,
    pub grid_column_span: u16,
    pub grid_rows: Vec<Track>,
    /// grid-template-areas: the names of each row's cells.
    pub grid_areas: Vec<Vec<String>>,
    /// grid-area: name, or lines (row start, column start, row end, column end; 0 auto).
    pub grid_area: Option<String>,
    pub grid_row_lines: (i16, i16),
    pub grid_col_lines: (i16, i16),
    pub vars: Arc<Vars>,
    /// A child of a flex or grid container (they are blocks).
    pub in_flex: bool,
    /// Corners: top left, top right, bottom right, bottom left.
    pub border_radius: [Len; 4],
    pub border_spacing: f32,
    pub border_collapse: bool,
    pub align_content: u8,
    pub justify_self: u8,
    pub aspect_ratio: Option<f32>,
    pub fill: Paint,
    pub stroke: Paint,
    pub stroke_width: f32,
    pub fill_evenodd: bool,
}

/// Custom properties: what an element declares, then its ancestors'.
/// Values have no var() left in them.
#[derive(Default, Debug)]
pub struct Vars {
    map: HashMap<String, String>,
    parent: Option<Arc<Vars>>,
}

impl Vars {
    fn get(&self, name: &str) -> Option<&str> {
        let mut v = self;
        loop {
            if let Some(s) = v.map.get(name) {
                return Some(s);
            }
            v = v.parent.as_deref()?;
        }
    }
}

// Edges: top, right, bottom, left (CSS order).
pub const TOP: usize = 0;
pub const RIGHT: usize = 1;
pub const BOTTOM: usize = 2;
pub const LEFT: usize = 3;

impl Default for Style {
    fn default() -> Self {
        Style {
            display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: false,
            color: BLACK,
            background: TRANSPARENT,
            background_image: None,
            family: Family::Serif,
            font_size: 16.0,
            bold: false,
            italic: false,
            line_height: LineHeight::Normal,
            text_align: Align::Left,
            decoration: 0,
            text_transform: 0,
            white_space: WhiteSpace::Normal,
            text_indent: Len::Px(0.0),
            margin: [Len::Px(0.0); 4],
            padding: [Len::Px(0.0); 4],
            border_width: [3.0; 4],
            border_visible: [false; 4],
            border_color: [None; 4],
            width: Len::Auto,
            height: Len::Auto,
            min_width: Len::Auto,
            min_height: Len::Auto,
            max_width: Len::Auto,
            max_height: Len::Auto,
            border_box: false,
            list_style: ListStyle::Disc,
            hidden: false,
            flex_row: true,
            flex_reverse: false,
            flex_wrap: false,
            justify: 0,
            align_items: 0,
            align_self: 0,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Len::Auto,
            gap: (Len::Px(0.0), Len::Px(0.0)),
            order: 0,
            inset: [Len::Auto; 4],
            overflow_hidden: false,
            invisible: false,
            vertical_align: 0,
            grid_columns: Vec::new(),
            grid_column_span: 1,
            grid_rows: Vec::new(),
            grid_areas: Vec::new(),
            grid_area: None,
            grid_row_lines: (0, 0),
            grid_col_lines: (0, 0),
            vars: Arc::default(),
            in_flex: false,
            border_radius: [Len::Px(0.0); 4],
            border_spacing: 2.0,
            border_collapse: false,
            align_content: 0,
            justify_self: 0,
            aspect_ratio: None,
            fill: Paint::Color(BLACK),
            stroke: Paint::None,
            stroke_width: 1.0,
            fill_evenodd: false,
        }
    }
}

impl Style {
    /// What a child starts from: the inherited properties of `self`.
    pub fn inherit(&self) -> Style {
        Style {
            color: self.color,
            family: self.family,
            font_size: self.font_size,
            bold: self.bold,
            italic: self.italic,
            line_height: self.line_height,
            text_align: self.text_align,
            decoration: self.decoration,
            text_transform: self.text_transform,
            white_space: self.white_space,
            text_indent: self.text_indent,
            list_style: self.list_style,
            hidden: self.hidden,
            vars: self.vars.clone(),
            border_spacing: self.border_spacing,
            border_collapse: self.border_collapse,
            fill: self.fill,
            stroke: self.stroke,
            stroke_width: self.stroke_width,
            fill_evenodd: self.fill_evenodd,
            ..Style::default()
        }
    }

    pub fn border(&self, side: usize) -> f32 {
        if self.border_visible[side] { self.border_width[side] } else { 0.0 }
    }

    pub fn border_color(&self, side: usize) -> Color {
        self.border_color[side].unwrap_or(self.color)
    }

    /// The corners' radii for a box of this size, scaled down to fit.
    pub fn radii(&self, w: f32, h: f32) -> [f32; 4] {
        let mut r = [0.0; 4];
        for (i, c) in self.border_radius.iter().enumerate() {
            r[i] = c.px(w.min(h));
        }
        let over = [(r[0] + r[1]) / w, (r[2] + r[3]) / w, (r[0] + r[3]) / h, (r[1] + r[2]) / h].into_iter().fold(1.0f32, f32::max);
        r.map(|v| v / over)
    }

    pub fn line_height_px(&self) -> f32 {
        match self.line_height {
            LineHeight::Normal => (self.font_size * 1.2).round(),
            LineHeight::Factor(f) => (self.font_size * f).round(),
            LineHeight::Px(p) => p,
        }
    }
}

pub const UA_CSS: &str = r#"
html, address, blockquote, body, dd, div, dl, dt, fieldset, form, frame, frameset, h1, h2, h3, h4, h5, h6,
noframes, ol, p, ul, center, dir, hr, menu, pre, header, footer, nav, article, section, aside, main, figure,
figcaption, details, summary, legend, hgroup, search, optgroup, address, dialog, noscript { display: block }
li { display: list-item }
head, script, style, link, meta, title, template, datalist, area, base, param, [hidden], option, map, track,
source, audio:not([controls]), embed, object, iframe, canvas, noembed, colgroup, col { display: none }
dialog:not([open]), [popover], details:not([open]) > :not(summary) { display: none }
table { display: table; border-collapse: separate }
tr { display: table-row }
td, th { display: table-cell; padding: 1px; vertical-align: middle }
thead, tbody, tfoot { display: table-row-group }
caption { display: table-caption; text-align: center }
body { margin: 8px }
p, dl, ol, ul, pre { margin: 1em 0 }
blockquote, figure { margin: 1em 40px }
h1 { font-size: 2em; margin: .67em 0; font-weight: bold }
h2 { font-size: 1.5em; margin: .83em 0; font-weight: bold }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }
h4 { margin: 1.33em 0; font-weight: bold }
h5 { font-size: .83em; margin: 1.67em 0; font-weight: bold }
h6 { font-size: .67em; margin: 2.33em 0; font-weight: bold }
ul, ol, menu, dir { padding-left: 40px }
ul, menu, dir { list-style-type: disc }
ol { list-style-type: decimal }
ul ul, ol ul { list-style-type: circle; margin: 0 }
ul ol, ol ol { margin: 0 }
ul ul ul, ol ul ul { list-style-type: square }
dd { margin-left: 40px }
b, strong, th { font-weight: bold }
i, em, cite, var, dfn, address { font-style: italic }
pre, code, kbd, samp, tt, listing, xmp, plaintext { font-family: monospace }
pre, listing, xmp, plaintext { white-space: pre }
code, kbd, samp, tt { font-size: .9em }
a:link, a:visited { color: #0000ee; text-decoration: underline }
u, ins { text-decoration: underline }
s, strike, del { text-decoration: line-through }
small { font-size: smaller }
big { font-size: larger }
sub, sup { font-size: smaller }
sub { vertical-align: sub }
sup { vertical-align: super }
hr { border: 1px solid #a0a0a0; margin: .5em 0 }
center { text-align: center }
th { text-align: center }
mark { background-color: yellow }
img, input, button, select, textarea, video { display: inline-block }
input, button, select, textarea { font-family: sans-serif; font-size: 13px }
input, select, textarea { border: 1px solid #767676; background-color: #fff; padding: 2px 3px }
button, input[type=submit], input[type=button], input[type=reset] { border: 2px outset #a0a0a0; background-color: #dcdcdc; padding: 1px 6px; text-align: center }
input[type=checkbox], input[type=radio] { border: 0; padding: 0; background: none }
fieldset { margin: 0 2px; padding: .35em .75em .625em; border: 2px groove #c0c0c0 }
legend { padding: 0 2px }
nobr { white-space: nowrap }
"#;

/// The style of every node: element styles, and text nodes take their
/// parent's.
pub struct Styles {
    pub of: Vec<Option<Arc<Style>>>,
}

/// Compute the style of every element of the document.
pub fn compute(dom: &Dom, rules: &Rules, viewport: (f32, f32)) -> Styles {
    let mut of: Vec<Option<Arc<Style>>> = vec![None; dom.nodes.len()];
    let root = Arc::new(Style::default());
    let mut caches = SelectorCaches::default();
    let mut stack: Vec<(NodeId, Arc<Style>)> = dom.nodes[Dom::DOCUMENT].children.iter().rev().map(|&c| (c, root.clone())).collect();
    let root_font = std::cell::Cell::new(16.0f32);
    while let Some((id, parent)) = stack.pop() {
        let st = match &dom.nodes[id].data {
            NodeData::Element(_) => {
                let s = Arc::new(element_style(dom, rules, id, &parent, &mut caches, viewport, &root_font));
                if dom.nodes[id].parent == Some(Dom::DOCUMENT) {
                    root_font.set(s.font_size);
                }
                s
            }
            NodeData::Text(_) => parent.clone(),
            _ => continue,
        };
        for &c in dom.nodes[id].children.iter().rev() {
            stack.push((c, st.clone()));
        }
        of[id] = Some(st);
    }
    Styles { of }
}

fn element_style(
    dom: &Dom,
    rules: &Rules,
    id: NodeId,
    parent: &Style,
    caches: &mut SelectorCaches,
    viewport: (f32, f32),
    root_font: &std::cell::Cell<f32>,
) -> Style {
    let e = dom.element(id).unwrap();
    let mut cand: Vec<usize> = Vec::new();
    if let Some(i) = e.attr("id") {
        if let Some(v) = rules.by_id.get(i) {
            cand.extend(v);
        }
    }
    if let Some(cl) = e.attr("class") {
        for c in cl.split_ascii_whitespace() {
            if let Some(v) = rules.by_class.get(c) {
                cand.extend(v);
            }
        }
    }
    if let Some(v) = rules.by_tag.get(&*e.name.local) {
        cand.extend(v);
    }
    cand.extend(&rules.universal);
    cand.sort_unstable();
    cand.dedup();
    let el = El { dom, id };
    let mut ctx = MatchingContext::new(MatchingMode::Normal, None, caches, QuirksMode::NoQuirks, NeedsSelectorFlags::No, MatchingForInvalidation::No);
    let mut matched: Vec<&css::Rule> = cand
        .into_iter()
        .map(|i| &rules.rules[i])
        .filter(|r| selectors::matching::matches_selector(&r.selector, 0, None, el, &mut ctx))
        .collect();
    matched.sort_by_key(|r| (r.origin, r.layer, r.specificity, r.order));
    let attr_decls = e.attr("style").map(css::parse_style_attr).unwrap_or_default();
    // Presentational attributes, below any CSS.
    let mut pres: Vec<Decl> = Vec::new();
    let mut hint = |n: &str, v: String| pres.push(Decl { name: n.into(), value: v, important: false });
    let dim = |v: &str| -> String {
        let v = v.trim();
        if v.ends_with('%') { v.to_string() } else { format!("{}px", v.trim_end_matches("px")) }
    };
    match &*e.name.local {
        "img" | "table" | "td" | "th" | "iframe" | "video" | "canvas" | "hr" | "col" | "svg" => {
            if let Some(w) = e.attr("width") {
                hint("width", dim(w));
            }
            if let Some(h) = e.attr("height") {
                hint("height", dim(h));
            }
        }
        _ => {}
    }
    if &*e.name.ns == "http://www.w3.org/2000/svg" {
        for n in ["fill", "stroke", "stroke-width", "fill-rule", "display", "color"] {
            if let Some(v) = e.attr(n) {
                hint(n, v.to_string());
            }
        }
    }
    if let Some(c) = e.attr("bgcolor") {
        hint("background-color", c.to_string());
    }
    if let Some(a) = e.attr("align") {
        let a = a.to_ascii_lowercase();
        match (&*e.name.local, a.as_str()) {
            ("img" | "table", "left") => hint("float", "left".into()),
            ("img" | "table", "right") => hint("float", "right".into()),
            ("table", "center") => {
                hint("margin-left", "auto".into());
                hint("margin-right", "auto".into());
            }
            (_, "center" | "middle") => hint("text-align", "center".into()),
            (_, "right") => hint("text-align", "right".into()),
            (_, "left") => hint("text-align", "left".into()),
            _ => {}
        }
    }
    if e.is("font") {
        if let Some(c) = e.attr("color") {
            hint("color", c.to_string());
        }
        if let Some(f) = e.attr("face") {
            hint("font-family", f.to_string());
        }
        if let Some(s) = e.attr("size") {
            let n: i32 = s.trim().trim_start_matches('+').parse().unwrap_or(3);
            let n = if s.trim().starts_with(['+', '-']) { 3 + n } else { n };
            let px = [10, 13, 16, 18, 24, 32, 48][(n.clamp(1, 7) - 1) as usize];
            hint("font-size", format!("{px}px"));
        }
    }
    if e.is("body") {
        if let Some(c) = e.attr("text") {
            hint("color", c.to_string());
        }
    }
    if e.is("table") {
        if let Some(b) = e.attr("border") {
            let w = b.trim().parse::<f32>().unwrap_or(1.0);
            if w > 0.0 {
                hint("border", format!("{w}px outset #808080"));
            }
        }
        if let Some(c) = e.attr("cellspacing") {
            hint("border-spacing", dim(c));
        }
    }
    if matches!(&*e.name.local, "td" | "th") {
        // Tables with border="n" draw cell borders too.
        let table = dom.descendants(Dom::DOCUMENT).next().and(None::<NodeId>);
        let _ = table;
        let mut p = dom.nodes[id].parent;
        while let Some(pp) = p {
            if let Some(pe) = dom.element(pp) {
                if pe.is("table") {
                    if pe.attr("border").and_then(|b| b.trim().parse::<f32>().ok()).is_some_and(|w| w > 0.0) {
                        hint("border", "1px inset #808080".into());
                    }
                    if let Some(c) = pe.attr("cellpadding") {
                        hint("padding", dim(c));
                    }
                    break;
                }
            }
            p = dom.nodes[pp].parent;
        }
        if let Some(c) = e.attr("colspan") {
            hint("grid-column", format!("span {}", c.trim()));
        }
        if e.attr("nowrap").is_some() {
            hint("white-space", "nowrap".into());
        }
    }
    let mut order: Vec<&Decl> = Vec::new();
    let is_ua = |r: &&&css::Rule| r.origin == Origin::UserAgent;
    order.extend(matched.iter().filter(is_ua).flat_map(|r| r.decls.iter()).filter(|d| !d.important));
    order.extend(pres.iter());
    order.extend(matched.iter().filter(|r| r.origin == Origin::Author).flat_map(|r| r.decls.iter()).filter(|d| !d.important));
    order.extend(attr_decls.iter().filter(|d| !d.important));
    // !important runs the other way between layers: earlier ones win.
    let mut strong: Vec<&&css::Rule> = matched.iter().filter(|r| r.origin == Origin::Author).collect();
    strong.sort_by_key(|r| (std::cmp::Reverse(r.layer), r.specificity, r.order));
    order.extend(strong.iter().flat_map(|r| r.decls.iter()).filter(|d| d.important));
    order.extend(attr_decls.iter().filter(|d| d.important));
    order.extend(matched.iter().filter(|r| r.origin == Origin::UserAgent).flat_map(|r| r.decls.iter()).filter(|d| d.important));

    let mut s = parent.inherit();
    // Custom properties first.
    if order.iter().any(|d| d.name.starts_with("--")) {
        // All of them before any is resolved: one may refer to a later one.
        let mut vars = Vars { map: HashMap::new(), parent: Some(s.vars.clone()) };
        for d in order.iter().filter(|d| d.name.starts_with("--")) {
            vars.map.insert(d.name.clone(), d.value.clone());
        }
        let names: Vec<String> = vars.map.iter().filter(|(_, v)| v.contains("var(")).map(|(k, _)| k.clone()).collect();
        for n in names {
            let v = substitute(&vars.map[&n], &vars, 0);
            vars.map.insert(n, v);
        }
        s.vars = Arc::new(vars);
    }
    let mut cx = Cx { parent, root_font: root_font.get(), vw: viewport.0, vh: viewport.1, em: parent.font_size };
    // Then the font size (em units depend on it).
    for d in order.iter().filter(|d| d.name == "font-size" || d.name == "font") {
        let v = value_of(d, &s);
        let t = css::tokens(&v);
        if d.name == "font" {
            font_shorthand(&mut s, &t, &cx, true);
        } else if let Some(sz) = font_size(&t, &cx) {
            s.font_size = sz;
        }
    }
    if dom.nodes[id].parent == Some(Dom::DOCUMENT) {
        cx.root_font = s.font_size;
    }
    cx.em = s.font_size;
    for d in order {
        if d.name.starts_with("--") || d.name == "font-size" {
            continue;
        }
        let v = value_of(d, &s);
        let t = css::tokens(&v);
        apply(&mut s, &d.name, &t, &cx);
    }
    // Blockification: floated and absolutely positioned boxes are blocks,
    // and so are the children of flex and grid containers.
    s.in_flex = matches!(parent.display, Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid) || parent.display == Display::Contents && parent.in_flex;
    if s.in_flex || s.float != Float::None || matches!(s.position, Position::Absolute | Position::Fixed) {
        s.display = match s.display {
            Display::Inline | Display::InlineBlock | Display::TableCell | Display::TableRow => Display::Block,
            Display::InlineFlex => Display::Flex,
            Display::InlineGrid => Display::Grid,
            d => d,
        };
    }
    s
}

fn value_of(d: &Decl, s: &Style) -> String {
    if d.value.contains("var(") { substitute(&d.value, &s.vars, 0) } else { d.value.clone() }
}

/// Replace var(--name, fallback) references.
fn substitute(v: &str, vars: &Vars, depth: u32) -> String {
    if depth > 32 || !v.contains("var(") {
        return v.to_string();
    }
    let mut out = String::new();
    let mut rest = v;
    while let Some(i) = rest.find("var(") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 4..];
        // The matching parenthesis.
        let mut level = 1;
        let mut end = after.len();
        for (j, c) in after.char_indices() {
            match c {
                '(' => level += 1,
                ')' => {
                    level -= 1;
                    if level == 0 {
                        end = j;
                        break;
                    }
                }
                _ => {}
            }
        }
        let inner = &after[..end];
        let (name, fallback) = match inner.find(',') {
            Some(c) => (inner[..c].trim(), Some(inner[c + 1..].trim())),
            None => (inner.trim(), None),
        };
        let val = vars.get(name).or(fallback).unwrap_or("");
        out.push_str(&substitute(val, vars, depth + 1));
        rest = if end < after.len() { &after[end + 1..] } else { "" };
    }
    out.push_str(rest);
    out
}

struct Cx<'a> {
    parent: &'a Style,
    root_font: f32,
    vw: f32,
    vh: f32,
    em: f32,
}

fn calc(t: &[Tok], cx: &Cx) -> Option<(f32, f32)> {
    // (px, percent) of a sum of terms, or min/max/clamp of them.
    let one = |t: &Tok| -> Option<(f32, f32)> {
        match t {
            Tok::Pct(p) => Some((0.0, *p)),
            Tok::Func(f, inner) => fn_value(f, inner, cx),
            Tok::Num(n) => Some((*n, 0.0)),
            t => match length(t, cx)? {
                Len::Px(v) => Some((v, 0.0)),
                Len::Pct(p) => Some((0.0, p)),
                Len::Auto => None,
            },
        }
    };
    let mut acc = (0.0, 0.0);
    let mut sign = 1.0;
    let mut i = 0;
    while i < t.len() {
        match &t[i] {
            Tok::Delim('+') => sign = 1.0,
            Tok::Delim('-') => sign = -1.0,
            Tok::Delim('*') => {
                if let Some(Tok::Num(n)) = t.get(i + 1) {
                    acc = (acc.0 * n, acc.1 * n);
                    i += 1;
                }
            }
            Tok::Slash => {
                if let Some(Tok::Num(n)) = t.get(i + 1) {
                    if *n != 0.0 {
                        acc = (acc.0 / n, acc.1 / n);
                    }
                    i += 1;
                }
            }
            x => {
                let (px, pc) = one(x)?;
                // "2 * 3px" and "3px * 2".
                if let (Some(Tok::Delim('*')), Some(Tok::Num(n))) = (t.get(i + 1), t.get(i + 2)) {
                    acc = (acc.0 + sign * px * n, acc.1 + sign * pc * n);
                    i += 2;
                } else if let Tok::Num(n) = x {
                    if let (Some(Tok::Delim('*')), Some(next)) = (t.get(i + 1), t.get(i + 2)) {
                        let (p2, c2) = one(next)?;
                        acc = (acc.0 + sign * p2 * n, acc.1 + sign * c2 * n);
                        i += 2;
                    } else {
                        acc = (acc.0 + sign * px, acc.1 + sign * pc);
                    }
                } else {
                    acc = (acc.0 + sign * px, acc.1 + sign * pc);
                }
                sign = 1.0;
            }
        }
        i += 1;
    }
    Some(acc)
}

fn fn_value(f: &str, inner: &[Tok], cx: &Cx) -> Option<(f32, f32)> {
    let args: Vec<&[Tok]> = inner.split(|t| *t == Tok::Comma).collect();
    let vals: Option<Vec<(f32, f32)>> = args.iter().map(|a| calc(a, cx)).collect();
    let vals = vals?;
    // Compare at an assumed 1000 px containing block.
    let key = |v: &(f32, f32)| v.0 + v.1 * 10.0;
    match f {
        "calc" | "-webkit-calc" => vals.first().copied(),
        "min" => vals.into_iter().min_by(|a, b| key(a).total_cmp(&key(b))),
        "max" => vals.into_iter().max_by(|a, b| key(a).total_cmp(&key(b))),
        "clamp" if vals.len() == 3 => {
            let (lo, v, hi) = (vals[0], vals[1], vals[2]);
            Some(if key(&v) < key(&lo) { lo } else if key(&v) > key(&hi) { hi } else { v })
        }
        _ => None,
    }
}

fn length(t: &Tok, cx: &Cx) -> Option<Len> {
    Some(match t {
        Tok::Num(n) if *n == 0.0 => Len::Px(0.0),
        Tok::Num(n) => Len::Px(*n),
        Tok::Pct(p) => Len::Pct(*p),
        Tok::Ident(i) if i == "auto" => Len::Auto,
        Tok::Dim(v, u) => Len::Px(match u.as_str() {
            "px" => *v,
            "em" => v * cx.em,
            "rem" => v * cx.root_font,
            "ex" | "ch" => v * cx.em * 0.5,
            "pt" => v * 96.0 / 72.0,
            "pc" => v * 16.0,
            "in" => v * 96.0,
            "cm" => v * 96.0 / 2.54,
            "mm" => v * 96.0 / 25.4,
            "q" => v * 96.0 / 101.6,
            "vw" => v * cx.vw / 100.0,
            "vh" => v * cx.vh / 100.0,
            "dvw" | "svw" | "lvw" => v * cx.vw / 100.0,
            "dvh" | "svh" | "lvh" => v * cx.vh / 100.0,
            "vmin" => v * cx.vw.min(cx.vh) / 100.0,
            "vmax" => v * cx.vw.max(cx.vh) / 100.0,
            "lh" => v * cx.em * 1.2,
            _ => return None,
        }),
        Tok::Func(f, inner) => {
            let (px, pc) = fn_value(f, inner, cx)?;
            if pc == 0.0 {
                Len::Px(px)
            } else if px == 0.0 {
                Len::Pct(pc)
            } else {
                // Mixed: right for a containing block of the viewport's width.
                Len::Px(px + pc * cx.vw.min(1200.0) / 100.0)
            }
        }
        _ => return None,
    })
}

fn font_size(t: &[Tok], cx: &Cx) -> Option<f32> {
    let p = cx.parent.font_size;
    Some(match t.first()? {
        Tok::Ident(k) => match k.as_str() {
            "xx-small" => 9.0,
            "x-small" => 10.0,
            "small" => 13.0,
            "medium" => 16.0,
            "large" => 18.0,
            "x-large" => 24.0,
            "xx-large" => 32.0,
            "xxx-large" => 48.0,
            "smaller" => p / 1.2,
            "larger" => p * 1.2,
            "inherit" => p,
            "initial" => 16.0,
            _ => return None,
        },
        Tok::Pct(v) => p * v / 100.0,
        Tok::Dim(v, u) if u == "em" => p * v,
        t => match length(t, &Cx { em: p, ..*cx })? {
            Len::Px(v) => v,
            _ => return None,
        },
    }
    .clamp(1.0, 400.0))
}

fn family(t: &[Tok]) -> Option<Family> {
    for x in t {
        let name = match x {
            Tok::Ident(i) => i.as_str(),
            Tok::Str(s) => s.as_str(),
            _ => continue,
        };
        let n = name.to_ascii_lowercase();
        if n.contains("mono") || n.contains("courier") || n.contains("consol") || n.contains("menlo") || n == "monaco" || n.contains("code") {
            return Some(Family::Mono);
        }
        if n == "serif" || n.contains("times") || n.contains("georgia") || n.contains("garamond") || n.contains("palatino") || n.contains("book") || n.contains("cambria") || n == "linux libertine" {
            return Some(Family::Serif);
        }
        if n == "sans-serif" || n.contains("arial") || n.contains("helvetica") || n.contains("verdana") || n.contains("sans") || n.contains("tahoma") || n.contains("segoe") || n == "system-ui" || n == "-apple-system" || n.contains("roboto") || n.contains("inter") || n == "lato" || n.contains("trebuchet") {
            return Some(Family::Sans);
        }
    }
    None
}

fn font_shorthand(s: &mut Style, t: &[Tok], cx: &Cx, size_only: bool) {
    let mut i = 0;
    while i < t.len() {
        match &t[i] {
            Tok::Ident(k) if k == "italic" || k == "oblique" => {
                if !size_only {
                    s.italic = true
                }
            }
            Tok::Ident(k) if k == "bold" || k == "bolder" => {
                if !size_only {
                    s.bold = true
                }
            }
            Tok::Num(n) if *n >= 100.0 => {
                if !size_only {
                    s.bold = *n >= 600.0
                }
            }
            Tok::Ident(k) if matches!(k.as_str(), "normal" | "small-caps" | "lighter") => {}
            x => {
                if let Some(sz) = font_size(std::slice::from_ref(x), cx) {
                    if size_only {
                        s.font_size = sz;
                        return;
                    }
                    if let (Some(Tok::Slash), Some(lh)) = (t.get(i + 1), t.get(i + 2)) {
                        if let Some(l) = line_height(std::slice::from_ref(lh), cx) {
                            s.line_height = l;
                        }
                    }
                    if let Some(f) = family(&t[i..]) {
                        s.family = f;
                    }
                    return;
                }
            }
        }
        i += 1;
    }
}

fn line_height(t: &[Tok], cx: &Cx) -> Option<LineHeight> {
    Some(match t.first()? {
        Tok::Ident(k) if k == "normal" => LineHeight::Normal,
        Tok::Num(n) => LineHeight::Factor(*n),
        Tok::Pct(p) => LineHeight::Px(cx.em * p / 100.0),
        x => match length(x, cx)? {
            Len::Px(v) => LineHeight::Px(v),
            _ => return None,
        },
    })
}

/// 1 to 4 values for the four edges.
fn edges(t: &[Tok], cx: &Cx) -> Option<[Len; 4]> {
    let v: Option<Vec<Len>> = t.iter().filter(|x| !matches!(x, Tok::Comma)).map(|x| length(x, cx)).collect();
    let v = v?;
    Some(match v.len() {
        1 => [v[0]; 4],
        2 => [v[0], v[1], v[0], v[1]],
        3 => [v[0], v[1], v[2], v[1]],
        4 => [v[0], v[1], v[2], v[3]],
        _ => return None,
    })
}

fn border_width(t: &Tok, cx: &Cx) -> Option<f32> {
    match t {
        Tok::Ident(k) if k == "thin" => Some(1.0),
        Tok::Ident(k) if k == "medium" => Some(3.0),
        Tok::Ident(k) if k == "thick" => Some(5.0),
        t => match length(t, cx)? {
            Len::Px(v) => Some(v.max(0.0)),
            _ => None,
        },
    }
}

fn border_style(t: &Tok) -> Option<bool> {
    match t {
        Tok::Ident(k) => match k.as_str() {
            "none" | "hidden" => Some(false),
            "solid" | "dotted" | "dashed" | "double" | "groove" | "ridge" | "inset" | "outset" => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// border / border-top...: width, style and colour in any order.
fn border_shorthand(s: &mut Style, sides: &[usize], t: &[Tok], cx: &Cx) {
    let (mut w, mut vis, mut col) = (3.0, false, None);
    for x in t {
        if let Some(v) = border_style(x) {
            vis = v;
        } else if let Some(bw) = border_width(x, cx) {
            w = bw;
        } else if let Some(c) = css::parse_color(std::slice::from_ref(x), s.color) {
            col = Some(c);
        }
    }
    for &i in sides {
        s.border_width[i] = w;
        s.border_visible[i] = vis;
        s.border_color[i] = col;
    }
}

fn side_of(name: &str) -> Option<usize> {
    Some(match name {
        "top" | "block-start" => TOP,
        "right" | "inline-end" => RIGHT,
        "bottom" | "block-end" => BOTTOM,
        "left" | "inline-start" => LEFT,
        _ => return None,
    })
}

fn tracks(t: &[Tok], cx: &Cx, out: &mut Vec<Track>) {
    for x in t {
        match x {
            Tok::Dim(v, u) if u == "fr" => out.push(Track::Fr(*v)),
            Tok::Ident(k) if k == "auto" => out.push(Track::Len(Len::Auto)),
            Tok::Ident(k) if k == "min-content" => out.push(Track::MinContent),
            Tok::Ident(k) if k == "max-content" => out.push(Track::MaxContent),
            Tok::Func(f, inner) if f == "repeat" => {
                let mut parts = inner.splitn(2, |t| *t == Tok::Comma);
                let n = parts.next().and_then(|p| p.first().cloned());
                let body = parts.next().unwrap_or(&[]);
                let mut one = Vec::new();
                tracks(body, cx, &mut one);
                let times = match n {
                    Some(Tok::Num(n)) => n as usize,
                    // auto-fill/auto-fit: as many as fit a 1000 px width.
                    _ => {
                        let w: f32 = one.iter().map(|t| if let Track::Len(Len::Px(p)) = t { *p } else { 200.0 }).sum();
                        ((1000.0 / w.max(1.0)) as usize).max(1)
                    }
                };
                for _ in 0..times.min(64) {
                    out.extend(one.iter().copied());
                }
            }
            Tok::Func(f, inner) if f == "minmax" => {
                let max = inner.split(|t| *t == Tok::Comma).nth(1).unwrap_or(&[]);
                let mut one = Vec::new();
                tracks(max, cx, &mut one);
                out.push(one.first().copied().unwrap_or(Track::Len(Len::Auto)));
            }
            Tok::Func(f, inner) if f == "fit-content" => {
                let _ = inner;
                out.push(Track::Len(Len::Auto))
            }
            x => {
                if let Some(l) = length(x, cx) {
                    out.push(Track::Len(l));
                }
            }
        }
    }
}

fn apply(s: &mut Style, name: &str, t: &[Tok], cx: &Cx) {
    let Some(first) = t.first() else { return };
    let p = cx.parent;
    if let Tok::Ident(k) = first {
        if k == "inherit" {
            match name {
                "color" => s.color = p.color,
                "background-color" | "background" => s.background = p.background,
                "font-family" => s.family = p.family,
                "font-weight" => s.bold = p.bold,
                "font-style" => s.italic = p.italic,
                "line-height" => s.line_height = p.line_height,
                "text-align" => s.text_align = p.text_align,
                "white-space" => s.white_space = p.white_space,
                "display" => s.display = p.display,
                "width" => s.width = p.width,
                "height" => s.height = p.height,
                "margin" => s.margin = p.margin,
                "padding" => s.padding = p.padding,
                "visibility" => s.hidden = p.hidden,
                _ => {}
            }
            return;
        }
        if k == "initial" || k == "unset" || k == "revert" || k == "revert-layer" {
            let d = Style::default();
            match name {
                "color" => s.color = if k == "unset" { p.color } else { BLACK },
                "background-color" | "background" => s.background = TRANSPARENT,
                "display" => s.display = Display::Inline,
                "width" => s.width = Len::Auto,
                "height" => s.height = Len::Auto,
                "max-width" => s.max_width = Len::Auto,
                "margin" => s.margin = d.margin,
                "padding" => s.padding = d.padding,
                "border" => s.border_visible = [false; 4],
                "float" => s.float = Float::None,
                "position" => s.position = Position::Static,
                _ => {}
            }
            return;
        }
    }
    let len = |t: &Tok| length(t, cx);
    match name {
        "display" => {
            if let Tok::Ident(k) = first {
                s.display = match k.as_str() {
                    "none" => Display::None,
                    "block" | "flow-root" | "flow" | "run-in" => Display::Block,
                    "inline" => {
                        if t.len() > 1 { if let Some(Tok::Ident(k2)) = t.get(1) { if k2 == "flex" { Display::InlineFlex } else if k2 == "grid" { Display::InlineGrid } else if k2 == "flow-root" { Display::InlineBlock } else { Display::Inline } } else { Display::Inline } } else { Display::Inline }
                    }
                    "inline-block" => Display::InlineBlock,
                    "list-item" => Display::ListItem,
                    "flex" | "-webkit-box" | "-webkit-flex" | "-ms-flexbox" => Display::Flex,
                    "inline-flex" | "-webkit-inline-flex" | "-webkit-inline-box" => Display::InlineFlex,
                    "grid" | "-ms-grid" => Display::Grid,
                    "inline-grid" => Display::InlineGrid,
                    "table" | "inline-table" => Display::Table,
                    "table-row" => Display::TableRow,
                    "table-cell" => Display::TableCell,
                    "table-row-group" | "table-header-group" | "table-footer-group" => Display::TableRowGroup,
                    "table-caption" => Display::TableCaption,
                    "table-column" | "table-column-group" => Display::None,
                    "contents" => Display::Contents,
                    _ => return,
                };
            }
        }
        "position" => {
            if let Tok::Ident(k) = first {
                s.position = match k.as_str() {
                    "static" => Position::Static,
                    "relative" => Position::Relative,
                    "absolute" => Position::Absolute,
                    "fixed" => Position::Fixed,
                    "sticky" | "-webkit-sticky" => Position::Sticky,
                    _ => return,
                };
            }
        }
        "float" => {
            if let Tok::Ident(k) = first {
                s.float = match k.as_str() {
                    "left" | "inline-start" => Float::Left,
                    "right" | "inline-end" => Float::Right,
                    _ => Float::None,
                };
            }
        }
        "clear" => s.clear = !matches!(first, Tok::Ident(k) if k == "none"),
        "color" => {
            if let Some(c) = css::parse_color(t, p.color) {
                s.color = c;
            }
        }
        "background-color" => {
            if let Some(c) = css::parse_color(t, s.color) {
                s.background = c;
            }
        }
        "background" => {
            // The shorthand resets what it does not name. The colour is the last layer's.
            s.background = TRANSPARENT;
            s.background_image = None;
            let last = t.rsplit(|x| *x == Tok::Comma).next().unwrap_or(t);
            for x in last {
                if let Some(c) = css::parse_color(std::slice::from_ref(x), s.color) {
                    s.background = c;
                }
            }
            if let Some(u) = t.iter().find_map(|x| if let Tok::Url(u) = x { Some(u.clone()) } else { None }) {
                s.background_image = Some(u);
            } else if t.iter().any(|x| matches!(x, Tok::Ident(k) if k == "none")) {
                s.background_image = None;
            }
        }
        "background-image" => {
            s.background_image = t.iter().find_map(|x| if let Tok::Url(u) = x { Some(u.clone()) } else { None });
        }
        "font-family" => {
            if let Some(f) = family(t) {
                s.family = f;
            }
        }
        "font" => font_shorthand(s, t, cx, false),
        "font-weight" => match first {
            Tok::Ident(k) if k == "bold" || k == "bolder" => s.bold = true,
            Tok::Ident(k) if k == "normal" || k == "lighter" => s.bold = false,
            Tok::Num(n) => s.bold = *n >= 600.0,
            _ => {}
        },
        "font-style" => s.italic = matches!(first, Tok::Ident(k) if k == "italic" || k == "oblique"),
        "line-height" => {
            if let Some(l) = line_height(t, cx) {
                s.line_height = l;
            }
        }
        "text-align" => {
            if let Tok::Ident(k) = first {
                s.text_align = match k.as_str() {
                    "right" | "end" | "-webkit-right" => Align::Right,
                    "center" | "-webkit-center" | "-moz-center" => Align::Center,
                    _ => Align::Left,
                };
            }
        }
        "text-decoration" | "text-decoration-line" => {
            let mut d = 0;
            for x in t {
                match x {
                    Tok::Ident(k) if k == "underline" => d |= UNDERLINE,
                    Tok::Ident(k) if k == "line-through" => d |= LINE_THROUGH,
                    Tok::Ident(k) if k == "overline" => d |= OVERLINE,
                    _ => {}
                }
            }
            s.decoration = d;
        }
        "text-transform" => {
            s.text_transform = match first {
                Tok::Ident(k) if k == "uppercase" => 1,
                Tok::Ident(k) if k == "lowercase" => 2,
                Tok::Ident(k) if k == "capitalize" => 3,
                _ => 0,
            }
        }
        "white-space" | "white-space-collapse" | "text-wrap-mode" => {
            if let Tok::Ident(k) = first {
                s.white_space = match k.as_str() {
                    "nowrap" => WhiteSpace::NoWrap,
                    "pre" => WhiteSpace::Pre,
                    "pre-wrap" | "break-spaces" | "preserve" => WhiteSpace::PreWrap,
                    "pre-line" | "preserve-breaks" => WhiteSpace::PreLine,
                    "wrap" | "normal" | "collapse" => WhiteSpace::Normal,
                    _ => return,
                };
            }
        }
        "text-indent" => {
            if let Some(l) = len(first) {
                s.text_indent = l;
            }
        }
        "margin" | "padding" => {
            if let Some(e) = edges(t, cx) {
                if name == "margin" {
                    s.margin = e
                } else {
                    s.padding = e.map(|l| if l == Len::Auto { Len::Px(0.0) } else { l })
                }
            }
        }
        "margin-inline" | "padding-inline" | "margin-block" | "padding-block" => {
            let v: Vec<Len> = t.iter().filter_map(|x| len(x)).collect();
            if let Some(&a) = v.first() {
                let b = v.get(1).copied().unwrap_or(a);
                let (i, j) = if name.ends_with("inline") { (LEFT, RIGHT) } else { (TOP, BOTTOM) };
                let arr = if name.starts_with("margin") { &mut s.margin } else { &mut s.padding };
                arr[i] = a;
                arr[j] = b;
            }
        }
        "border" => border_shorthand(s, &[0, 1, 2, 3], t, cx),
        "border-width" => {
            let v: Vec<f32> = t.iter().filter_map(|x| border_width(x, cx)).collect();
            let e = match v.len() {
                1 => [v[0]; 4],
                2 => [v[0], v[1], v[0], v[1]],
                3 => [v[0], v[1], v[2], v[1]],
                4 => [v[0], v[1], v[2], v[3]],
                _ => return,
            };
            s.border_width = e;
        }
        "border-style" => {
            let v: Vec<bool> = t.iter().filter_map(border_style).collect();
            let e = match v.len() {
                1 => [v[0]; 4],
                2 => [v[0], v[1], v[0], v[1]],
                3 => [v[0], v[1], v[2], v[1]],
                4 => [v[0], v[1], v[2], v[3]],
                _ => return,
            };
            s.border_visible = e;
        }
        "border-color" => {
            let v: Vec<Color> = t.iter().filter_map(|x| css::parse_color(std::slice::from_ref(x), s.color)).collect();
            let e = match v.len() {
                1 => [v[0]; 4],
                2 => [v[0], v[1], v[0], v[1]],
                3 => [v[0], v[1], v[2], v[1]],
                4 => [v[0], v[1], v[2], v[3]],
                _ => return,
            };
            s.border_color = e.map(Some);
        }
        "border-inline" => border_shorthand(s, &[LEFT, RIGHT], t, cx),
        "border-block" => border_shorthand(s, &[TOP, BOTTOM], t, cx),
        "box-sizing" => s.border_box = matches!(first, Tok::Ident(k) if k == "border-box"),
        "width" => s.width = len(first).unwrap_or(s.width),
        "height" => s.height = len(first).unwrap_or(s.height),
        "min-width" => s.min_width = len(first).unwrap_or(s.min_width),
        "min-height" => s.min_height = len(first).unwrap_or(s.min_height),
        "max-width" => s.max_width = if matches!(first, Tok::Ident(k) if k == "none") { Len::Auto } else { len(first).unwrap_or(s.max_width) },
        "max-height" => s.max_height = if matches!(first, Tok::Ident(k) if k == "none") { Len::Auto } else { len(first).unwrap_or(s.max_height) },
        "inline-size" => s.width = len(first).unwrap_or(s.width),
        "block-size" => s.height = len(first).unwrap_or(s.height),
        "min-inline-size" => s.min_width = len(first).unwrap_or(s.min_width),
        "min-block-size" => s.min_height = len(first).unwrap_or(s.min_height),
        "max-inline-size" => s.max_width = if matches!(first, Tok::Ident(k) if k == "none") { Len::Auto } else { len(first).unwrap_or(s.max_width) },
        "max-block-size" => s.max_height = if matches!(first, Tok::Ident(k) if k == "none") { Len::Auto } else { len(first).unwrap_or(s.max_height) },
        "aspect-ratio" => {
            let v: Vec<f32> = t.iter().filter_map(|x| if let Tok::Num(n) = x { Some(*n) } else { None }).collect();
            s.aspect_ratio = match v[..] {
                [a] if a > 0.0 => Some(a),
                [a, b] if b > 0.0 => Some(a / b),
                _ => None,
            };
        }
        "border-radius" => {
            let first_half = t.split(|x| *x == Tok::Slash).next().unwrap_or(t);
            let v: Vec<Len> = first_half.iter().filter_map(|x| len(x)).collect();
            s.border_radius = match v[..] {
                [a] => [a; 4],
                [a, b] => [a, b, a, b],
                [a, b, c] => [a, b, c, b],
                [a, b, c, d] => [a, b, c, d],
                _ => return,
            };
        }
        "border-top-left-radius" | "border-start-start-radius" => s.border_radius[0] = len(first).unwrap_or(s.border_radius[0]),
        "border-top-right-radius" | "border-start-end-radius" => s.border_radius[1] = len(first).unwrap_or(s.border_radius[1]),
        "border-bottom-right-radius" | "border-end-end-radius" => s.border_radius[2] = len(first).unwrap_or(s.border_radius[2]),
        "border-bottom-left-radius" | "border-end-start-radius" => s.border_radius[3] = len(first).unwrap_or(s.border_radius[3]),
        "border-spacing" => {
            if let Some(Len::Px(v)) = len(first) {
                s.border_spacing = v;
            }
        }
        "border-collapse" => s.border_collapse = matches!(first, Tok::Ident(k) if k == "collapse"),
        "fill" | "stroke" => {
            let p = match first {
                Tok::Ident(k) if k == "none" => Paint::None,
                Tok::Ident(k) if k == "currentcolor" => Paint::Current,
                // Gradients and patterns are not drawn.
                Tok::Url(_) => Paint::Color(0xff80_8080),
                _ => match css::parse_color(t, s.color) {
                    Some(c) => Paint::Color(c),
                    None => return,
                },
            };
            if name == "fill" {
                s.fill = p;
            } else {
                s.stroke = p;
            }
        }
        "stroke-width" => {
            if let Some(Len::Px(v)) = len(first) {
                s.stroke_width = v;
            }
        }
        "fill-rule" => s.fill_evenodd = matches!(first, Tok::Ident(k) if k == "evenodd"),
        "list-style" | "list-style-type" => {
            for x in t {
                if let Tok::Ident(k) = x {
                    s.list_style = match k.as_str() {
                        "none" => ListStyle::None,
                        "disc" => ListStyle::Disc,
                        "circle" => ListStyle::Circle,
                        "square" => ListStyle::Square,
                        "decimal" | "decimal-leading-zero" => ListStyle::Decimal,
                        "lower-alpha" | "lower-latin" => ListStyle::LowerAlpha,
                        "upper-alpha" | "upper-latin" => ListStyle::UpperAlpha,
                        "lower-roman" => ListStyle::LowerRoman,
                        "upper-roman" => ListStyle::UpperRoman,
                        _ => continue,
                    };
                }
            }
        }
        "visibility" => s.hidden = matches!(first, Tok::Ident(k) if k == "hidden" || k == "collapse"),
        "flex-direction" => {
            if let Tok::Ident(k) = first {
                s.flex_row = k.starts_with("row");
                s.flex_reverse = k.ends_with("reverse");
            }
        }
        "flex-wrap" => s.flex_wrap = matches!(first, Tok::Ident(k) if k.starts_with("wrap")),
        "flex-flow" => {
            for x in t {
                if let Tok::Ident(k) = x {
                    match k.as_str() {
                        "row" | "row-reverse" | "column" | "column-reverse" => {
                            s.flex_row = k.starts_with("row");
                            s.flex_reverse = k.ends_with("reverse");
                        }
                        "wrap" | "wrap-reverse" => s.flex_wrap = true,
                        "nowrap" => s.flex_wrap = false,
                        _ => {}
                    }
                }
            }
        }
        "flex" => match t {
            [Tok::Ident(k)] if k == "none" => {
                s.flex_grow = 0.0;
                s.flex_shrink = 0.0;
                s.flex_basis = Len::Auto;
            }
            [Tok::Ident(k)] if k == "auto" => {
                s.flex_grow = 1.0;
                s.flex_shrink = 1.0;
                s.flex_basis = Len::Auto;
            }
            _ => {
                let mut nums = Vec::new();
                let mut basis = None;
                for x in t {
                    match x {
                        Tok::Num(n) if nums.len() < 2 => nums.push(*n),
                        x => basis = len(x).or(basis),
                    }
                }
                s.flex_grow = nums.first().copied().unwrap_or(1.0);
                s.flex_shrink = nums.get(1).copied().unwrap_or(1.0);
                s.flex_basis = basis.unwrap_or(if nums.is_empty() { Len::Auto } else { Len::Pct(0.0) });
            }
        },
        "flex-grow" => {
            if let Tok::Num(n) = first {
                s.flex_grow = *n
            }
        }
        "flex-shrink" => {
            if let Tok::Num(n) = first {
                s.flex_shrink = *n
            }
        }
        "flex-basis" => s.flex_basis = if matches!(first, Tok::Ident(k) if k == "content") { Len::Auto } else { len(first).unwrap_or(s.flex_basis) },
        "justify-content" => s.justify = align_kw(t),
        "align-items" => s.align_items = align_kw(t),
        "align-self" => s.align_self = align_kw(t),
        "align-content" => s.align_content = align_kw(t),
        "justify-self" => s.justify_self = align_kw(t),
        "place-items" => s.align_items = align_kw(t),
        "place-self" => {
            s.align_self = align_kw(t);
            s.justify_self = align_kw(&t[t.len() - 1..]);
        }
        "place-content" => {
            s.align_content = align_kw(t);
            s.justify = align_kw(&t[t.len() - 1..]);
        }
        "order" => {
            if let Tok::Num(n) = first {
                s.order = *n as i32
            }
        }
        "gap" | "grid-gap" => {
            let v: Vec<Len> = t.iter().filter_map(|x| len(x)).collect();
            if let Some(&a) = v.first() {
                s.gap = (a, v.get(1).copied().unwrap_or(a));
            }
        }
        "row-gap" | "grid-row-gap" => s.gap.0 = len(first).unwrap_or(s.gap.0),
        "column-gap" | "grid-column-gap" => s.gap.1 = len(first).unwrap_or(s.gap.1),
        "top" => s.inset[TOP] = len(first).unwrap_or(Len::Auto),
        "right" => s.inset[RIGHT] = len(first).unwrap_or(Len::Auto),
        "bottom" => s.inset[BOTTOM] = len(first).unwrap_or(Len::Auto),
        "left" => s.inset[LEFT] = len(first).unwrap_or(Len::Auto),
        "inset" => {
            if let Some(e) = edges(t, cx) {
                s.inset = e;
            }
        }
        "inset-inline-start" => s.inset[LEFT] = len(first).unwrap_or(Len::Auto),
        "inset-inline-end" => s.inset[RIGHT] = len(first).unwrap_or(Len::Auto),
        "inset-block-start" => s.inset[TOP] = len(first).unwrap_or(Len::Auto),
        "inset-block-end" => s.inset[BOTTOM] = len(first).unwrap_or(Len::Auto),
        "inset-inline" | "inset-block" => {
            let v: Vec<Len> = t.iter().filter_map(|x| len(x)).collect();
            if let Some(&a) = v.first() {
                let (i, j) = if name == "inset-inline" { (LEFT, RIGHT) } else { (TOP, BOTTOM) };
                s.inset[i] = a;
                s.inset[j] = v.get(1).copied().unwrap_or(a);
            }
        }
        "opacity" => {
            if let Tok::Num(n) = first {
                s.invisible = *n <= 0.01;
            } else if let Tok::Pct(p) = first {
                s.invisible = *p <= 1.0;
            }
        }
        "clip" => {
            // rect(0 0 0 0), rect(1px, 1px, 1px, 1px): the visually-hidden idiom.
            if let Tok::Func(f, args) = first {
                if f == "rect" {
                    let v: Vec<f32> = args.iter().filter_map(|a| match length(a, cx) { Some(Len::Px(p)) => Some(p), _ => None }).collect();
                    if v.len() == 4 && (v[2] - v[0]).abs() <= 1.0 {
                        s.invisible = true;
                    }
                }
            }
        }
        "clip-path" => {
            if let Tok::Func(f, args) = first {
                if f == "inset" && matches!(args.first(), Some(Tok::Pct(p)) if *p >= 50.0) {
                    s.invisible = true;
                }
            }
        }
        "overflow" | "overflow-x" | "overflow-y" => s.overflow_hidden = matches!(first, Tok::Ident(k) if k == "hidden" || k == "clip" || k == "auto" || k == "scroll"),
        "vertical-align" => {
            s.vertical_align = match first {
                Tok::Ident(k) if k == "middle" => 1,
                Tok::Ident(k) if k == "top" || k == "text-top" => 2,
                Tok::Ident(k) if k == "bottom" || k == "text-bottom" => 3,
                Tok::Ident(k) if k == "sub" => 4,
                Tok::Ident(k) if k == "super" => 5,
                _ => 0,
            }
        }
        "grid-template-columns" => {
            let mut v = Vec::new();
            tracks(t, cx, &mut v);
            s.grid_columns = v;
        }
        "grid-template-rows" => {
            let mut v = Vec::new();
            tracks(t, cx, &mut v);
            s.grid_rows = v;
        }
        "grid-template-areas" => {
            s.grid_areas = t.iter().filter_map(|x| if let Tok::Str(r) = x { Some(r.split_whitespace().map(|n| n.to_ascii_lowercase()).collect()) } else { None }).collect();
        }
        "grid-template" => {
            let mut parts = t.split(|x| *x == Tok::Slash);
            let rows = parts.next().unwrap_or(&[]);
            let areas: Vec<Vec<String>> = rows.iter().filter_map(|x| if let Tok::Str(r) = x { Some(r.split_whitespace().map(|n| n.to_ascii_lowercase()).collect()) } else { None }).collect();
            let mut rv = Vec::new();
            let sizes: Vec<Tok> = rows.iter().filter(|x| !matches!(x, Tok::Str(_))).cloned().collect();
            tracks(&sizes, cx, &mut rv);
            s.grid_rows = rv;
            s.grid_areas = areas;
            if let Some(cols) = parts.next() {
                let mut cv = Vec::new();
                tracks(cols, cx, &mut cv);
                s.grid_columns = cv;
            }
        }
        "grid-area" => {
            let parts: Vec<&[Tok]> = t.split(|x| *x == Tok::Slash).collect();
            match parts.first().and_then(|p| p.first()) {
                Some(Tok::Ident(name)) if name != "auto" => s.grid_area = Some(name.clone()),
                _ => {
                    let num = |i: usize| match parts.get(i).and_then(|p| p.first()) {
                        Some(Tok::Num(n)) => *n as i16,
                        _ => 0,
                    };
                    s.grid_row_lines = (num(0), num(2));
                    s.grid_col_lines = (num(1), num(3));
                }
            }
        }
        "grid-column-end" | "grid-row-end" => {
            let row = name.starts_with("grid-row");
            match t {
                [Tok::Num(n)] if row => s.grid_row_lines.1 = *n as i16,
                [Tok::Num(n)] => s.grid_col_lines.1 = *n as i16,
                [Tok::Ident(k), Tok::Num(n)] if k == "span" && !row => s.grid_column_span = (*n as u16).clamp(1, 64),
                _ => {}
            }
        }
        "grid-column" | "grid-column-start" | "grid-row" | "grid-row-start" => {
            let parts: Vec<&[Tok]> = t.split(|x| *x == Tok::Slash).collect();
            let row = name.starts_with("grid-row");
            let first = parts.first().copied().unwrap_or(&[]);
            match (first.first(), first.get(1)) {
                (Some(Tok::Ident(k)), Some(Tok::Num(n))) if k == "span" => {
                    if !row {
                        s.grid_column_span = (*n as u16).clamp(1, 64);
                    }
                }
                (Some(Tok::Ident(k)), _) if k != "auto" && !row => s.grid_area = Some(k.clone()),
                (Some(Tok::Num(a)), _) => {
                    let end = match parts.get(1).and_then(|p| p.first()) {
                        Some(Tok::Num(b)) => *b as i16,
                        _ => 0,
                    };
                    if let (Some(Tok::Ident(k)), Some(Tok::Num(n))) = (parts.get(1).and_then(|p| p.first()), parts.get(1).and_then(|p| p.get(1))) {
                        if k == "span" && !row {
                            s.grid_column_span = (*n as u16).clamp(1, 64);
                        }
                    }
                    if row {
                        s.grid_row_lines = (*a as i16, end);
                    } else {
                        s.grid_col_lines = (*a as i16, end);
                    }
                }
                _ => {}
            }
        }
        _ => {
            // margin-top, padding-left, border-top-width...
            if let Some(rest) = name.strip_prefix("margin-") {
                if let (Some(i), Some(l)) = (side_of(rest), len(first)) {
                    s.margin[i] = l;
                }
            } else if let Some(rest) = name.strip_prefix("padding-") {
                if let (Some(i), Some(l)) = (side_of(rest), len(first)) {
                    s.padding[i] = l;
                }
            } else if let Some(rest) = name.strip_prefix("border-") {
                if let Some(i) = side_of(rest) {
                    border_shorthand(s, &[i], t, cx);
                } else if let Some((side, what)) = rest.rsplit_once('-') {
                    let Some(i) = side_of(side) else { return };
                    match what {
                        "width" => {
                            if let Some(w) = border_width(first, cx) {
                                s.border_width[i] = w;
                            }
                        }
                        "style" => {
                            if let Some(v) = border_style(first) {
                                s.border_visible[i] = v;
                            }
                        }
                        "color" => {
                            if let Some(c) = css::parse_color(t, s.color) {
                                s.border_color[i] = Some(c);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// flex alignment keywords: 0 normal/stretch, 1 start, 2 end, 3 center,
/// 4 space-between, 5 space-around, 6 space-evenly, 7 baseline.
fn align_kw(t: &[Tok]) -> u8 {
    for x in t {
        if let Tok::Ident(k) = x {
            return match k.as_str() {
                "flex-start" | "start" | "left" | "self-start" => 1,
                "flex-end" | "end" | "right" | "self-end" => 2,
                "center" => 3,
                "space-between" => 4,
                "space-around" => 5,
                "space-evenly" => 6,
                "baseline" | "first" | "last" => 7,
                "stretch" | "normal" => 0,
                _ => continue,
            };
        }
    }
    0
}
