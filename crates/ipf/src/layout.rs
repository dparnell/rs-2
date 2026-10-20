//! Panel layout, following IBM's formatter (HELPMGR's `FormatCell`): the
//! text of a panel's cells as one stream of words and bitmaps placed on a
//! page of a given width, with the fonts measured by the caller.
//!
//! Coordinates are pixels, `x` from the left, `y` downwards from the top
//! of the page; a word's `y` is its baseline, a bitmap's its bottom edge.
//! Margins count in the system font's character width `U`; the default
//! left margin is `2U` (one more `U` for files of version 2, which is
//! every file of the compiler shipped with Warp 4), the right edge `U`
//! from the window's.
//!
//! Spacing: while autospacing is on (at the start of every cell, after
//! paragraphs and breaks) a word is followed by a space; the compiler
//! glues tokens such as `OS/2*` by turning it off before the first and on
//! again before the last, and such a run wraps as one. A space (0xFE) is
//! two spaces while autospacing, which is how sentences end.

use crate::bitmap;
use crate::text::{decode, AcViewport, Area, Item, LinkData, Target};
use crate::Doc;
use alloc::vec::Vec;

/// Size of a font: line height, the width of a space (for the system
/// font: of an average character, the margin unit) and the descent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontMetrics {
    pub height: i32,
    pub space: i32,
    pub descent: i32,
}

/// Measures text for the layout.
pub trait Metrics {
    /// The system (default) font: its space is the margin unit.
    fn system(&mut self) -> FontMetrics;
    /// Font table entry `font` (0: the default) in `style` (0x04's: 1
    /// italic, 2 bold, 4 underline, combined).
    fn font(&mut self, font: u8, style: u8) -> FontMetrics;
    /// Width of `word` in that font.
    fn width(&mut self, font: u8, style: u8, word: &[u8]) -> i32;
    /// The font number (16 and up) for a font of dynamic data by face
    /// name and point size; 0: the default font.
    fn face(&mut self, _face: &[u8], _width: u16, _height: u16) -> u8 {
        0
    }
    /// Size of an application's bitmap (HBITMAP) of dynamic data.
    fn image(&mut self, _handle: u32) -> (i32, i32) {
        (0, 0)
    }
}

/// What a run shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunKind<'a> {
    Word(&'a [u8]),
    /// A bitmap; `fit`: stretched over the whole window.
    Bitmap { offset: u32, flags: u8, cx: i32, cy: i32, fit: bool },
    /// An application's bitmap (HBITMAP) or metafile (HMF) of dynamic
    /// data.
    Image { handle: u32, metafile: bool, cx: i32, cy: i32, fit: bool },
}

/// A placed word or bitmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<'a> {
    pub x: i32,
    /// Baseline (words) or bottom edge (bitmaps), downwards.
    pub y: i32,
    pub width: i32,
    /// Line height and descent of the font (bitmaps: their height, 0).
    pub height: i32,
    pub descent: i32,
    pub font: u8,
    pub style: u8,
    /// Colour indexes (0: the help window's colours).
    pub fg: u8,
    pub bg: u8,
    /// Index into [`Page::links`].
    pub link: Option<u16>,
    pub kind: RunKind<'a>,
}

/// A link of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link<'a> {
    pub target: Target<'a>,
    pub data: Option<LinkData>,
}

/// A laid-out panel.
#[derive(Debug, Clone, Default)]
pub struct Page<'a> {
    pub runs: Vec<Run<'a>>,
    pub links: Vec<Link<'a>>,
    /// Link areas of bitmaps: (run index, area).
    pub areas: Vec<(usize, Area<'a>)>,
    /// Links that open by themselves (`auto`), in order.
    pub auto: Vec<Link<'a>>,
    /// Application-controlled viewports (`:acviewport.`).
    pub acvps: Vec<AcViewport<'a>>,
    /// Height of the text and widest x reached.
    pub height: i32,
    pub width: i32,
}

/// Colour index (0x0D, 0x13, 0x14) as an RGB value, or None for the help
/// window's own text or background colour (index 0).
pub fn colour(index: u8) -> Option<u32> {
    Some(match index & 0x0f {
        0 => return None,
        1 => 0x0000ff,
        2 => 0xff0000,
        3 => 0xff00ff,
        4 => 0x00ff00,
        5 => 0x00ffff,
        6 => 0xffff00,
        7 => 0x808000,
        8 => 0x808080,
        9 => 0x000080,
        10 => 0x800000,
        11 => 0x800080,
        12 => 0x008000,
        13 => 0x008080,
        14 => 0x000000,
        _ => 0xcccccc,
    })
}

impl Page<'_> {
    /// The link at (x, y) (page coordinates): words of a link, or a
    /// bitmap's area. Returns the link, or for areas the area.
    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        for (i, r) in self.runs.iter().enumerate() {
            match r.kind {
                RunKind::Word(_) => {
                    let Some(l) = r.link else { continue };
                    if x >= r.x && x < r.x + r.width && y > r.y - (r.height - r.descent) && y <= r.y + r.descent {
                        return Some(Hit::Link(l));
                    }
                }
                RunKind::Bitmap { cx, cy, fit, .. } if !fit => {
                    if x < r.x || x >= r.x + cx || y <= r.y - cy || y > r.y {
                        continue;
                    }
                    for (k, (run, a)) in self.areas.iter().enumerate() {
                        if *run != i {
                            continue;
                        }
                        let inside = match a.rect {
                            None => true,
                            Some((ax, ay, acx, acy)) => {
                                let (bx, by) = (x - r.x, r.y - y);
                                bx >= ax as i32 && bx <= ax as i32 + acx as i32 && by >= ay as i32 && by <= ay as i32 + acy as i32
                            }
                        };
                        if inside {
                            return Some(Hit::Area(k));
                        }
                    }
                    if let Some(l) = r.link {
                        return Some(Hit::Link(l));
                    }
                }
                RunKind::Image { cx, cy, fit: false, .. } => {
                    if let Some(l) = r.link {
                        if x >= r.x && x < r.x + cx && y > r.y - cy && y <= r.y {
                            return Some(Hit::Link(l));
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Bounding box (x0, y0, x1, y1, page coordinates) of each line piece
    /// of link `l`.
    pub fn link_boxes(&self, l: u16) -> Vec<(i32, i32, i32, i32)> {
        let mut v: Vec<(i32, i32, i32, i32)> = Vec::new();
        for r in self.runs.iter().filter(|r| r.link == Some(l)) {
            let (top, bottom) = match r.kind {
                RunKind::Word(_) => (r.y - (r.height - r.descent), r.y + r.descent),
                RunKind::Bitmap { cy, .. } | RunKind::Image { cy, .. } => (r.y - cy, r.y),
            };
            match v.last_mut() {
                Some(b) if b.3 == bottom && r.x >= b.0 => b.2 = b.2.max(r.x + r.width),
                _ => v.push((r.x, top, r.x + r.width, bottom)),
            }
        }
        v
    }
}

/// What [`Page::hit`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Link(u16),
    Area(usize),
}

/// Layout options.
#[derive(Debug, Clone, Default)]
pub struct Options<'k> {
    /// Window width in pixels.
    pub width: i32,
    /// The keys `:hide.` text needs (IPF_KEYS, `+`-separated).
    pub keys: &'k [u8],
}

struct State {
    x: i32,
    y: i32,
    left: i32,
    right: i32,
    width: i32,
    mono: bool,
    auto_mono: bool,
    spacing: bool,
    wrap: bool,
    align: u8,
    realign: bool,
    font: u8,
    style: u8,
    fg: u8,
    bg: u8,
    link: Option<u16>,
    art: Option<usize>,
    fm: FontMetrics,
    sys: FontMetrics,
    unit: i32,
    extra: i32,
    /// Baseline of the previous line, and the first run of this line.
    prev: Option<i32>,
    line_start: usize,
    /// Words of a glued run that must not wrap by themselves.
    nowrap: usize,
    max_x: i32,
    hiding: bool,
    /// In dynamic data (no autospacing), and the spacing before it.
    ddf: bool,
    ddf_spacing: bool,
}

fn key_listed(keys: &[u8], key: &[u8]) -> bool {
    keys.split(|&c| c == b'+' || c == b';').any(|k| k.eq_ignore_ascii_case(key))
}

/// Lay out panel `toc` of `doc`.
pub fn layout<'a>(doc: &Doc<'a>, toc: u16, opts: &Options<'_>, m: &mut dyn Metrics) -> Page<'a> {
    let Ok(entry) = doc.toc(toc) else { return Page::default() };
    let cells: Vec<Vec<Item<'a>>> =
        entry.cells.iter().filter_map(|&c| doc.cell(c).ok()).map(|c| decode(doc, &c)).collect();
    layout_items(doc, &cells, opts, m)
}

/// Lay out the decoded cells of a panel.
pub fn layout_items<'a>(doc: &Doc<'a>, cells: &[Vec<Item<'a>>], opts: &Options<'_>, m: &mut dyn Metrics) -> Page<'a> {
    let sys = m.system();
    let unit = sys.space.max(1);
    let extra = if doc.header.version.0 > 1 { unit } else { 0 };
    let fm = m.font(0, 0);
    let mut s = State {
        x: unit + extra,
        y: sys.height - sys.descent,
        left: unit + extra,
        right: opts.width - unit,
        width: opts.width,
        mono: false,
        auto_mono: false,
        spacing: true,
        wrap: true,
        align: 1,
        realign: false,
        font: 0,
        style: 0,
        fg: 0,
        bg: 0,
        link: None,
        art: None,
        fm,
        sys,
        unit,
        extra,
        prev: None,
        line_start: 0,
        nowrap: 0,
        max_x: 0,
        hiding: false,
        ddf: false,
        ddf_spacing: true,
    };
    let mut page = Page::default();
    for items in cells {
        s.spacing = true;
        let mut i = 0;
        while i < items.len() {
            let item = &items[i];
            if s.hiding {
                if *item == Item::HideEnd {
                    s.hiding = false;
                }
                i += 1;
                continue;
            }
            step(doc, &mut s, &mut page, items, i, opts, m);
            i += 1;
        }
    }
    let bottom = page
        .runs
        .iter()
        .map(|r| match r.kind {
            RunKind::Word(_) => r.y + r.descent,
            RunKind::Bitmap { fit: true, .. } | RunKind::Image { fit: true, .. } => 0,
            RunKind::Bitmap { .. } | RunKind::Image { .. } => r.y,
        })
        .max()
        .unwrap_or(0);
    page.height = bottom.max(s.y + s.sys.descent);
    page.width = s.max_x;
    page
}

fn new_line(s: &mut State, page: &Page<'_>, lines: i32) {
    s.prev = Some(s.y + (lines - 1) * s.fm.height);
    s.y += lines * s.fm.height;
    s.x = s.left;
    s.line_start = page.runs.len();
}

fn margin(s: &State, n: u8) -> i32 {
    s.unit * n as i32 + s.extra
}

/// Width of the rest of the line from item `i` (up to a paragraph or
/// break), for alignment: IBM's PnlCELength.
fn line_len(s: &State, items: &[Item<'_>], i: usize, m: &mut dyn Metrics) -> i32 {
    let (mut font, mut style, mut spacing) = (s.font, s.style, s.spacing);
    let mut sw = s.fm.space;
    let mut len = 0;
    for it in &items[i..] {
        match it {
            Item::Word(w) => {
                len += m.width(font, style, w);
                if spacing {
                    len += sw;
                }
            }
            Item::Space => len += if spacing { 2 * sw } else { sw },
            Item::ToggleSpacing => spacing = !spacing,
            Item::Style(st) => {
                style = *st;
                sw = m.font(font, style).space;
            }
            Item::Font(f) => {
                font = *f;
                style = 0;
                sw = m.font(font, style).space;
            }
            Item::Para | Item::Break => break,
            _ => {}
        }
    }
    len
}

/// Width of a glued run starting with word `i` (autospacing off): the
/// words up to a space, paragraph, centre, break or margin; past a toggle
/// only the next word. Returns the width and the number of words.
fn glued(s: &State, items: &[Item<'_>], i: usize, m: &mut dyn Metrics) -> (i32, usize) {
    let mut total = 0;
    let mut words = 0;
    let mut last = false;
    for it in &items[i..] {
        match it {
            Item::Word(w) => {
                total += m.width(s.font, s.style, w);
                words += 1;
                if last {
                    break;
                }
            }
            Item::ToggleSpacing => last = true,
            Item::Space | Item::Para | Item::Center | Item::Break | Item::LeftMargin(_) => break,
            _ => {}
        }
    }
    (total, words.max(1))
}

fn set_font(s: &mut State, page: &mut Page<'_>, m: &mut dyn Metrics) {
    let old = s.fm.height;
    s.fm = m.font(s.font, s.style);
    // A taller font moves the line down (IBM's MscFormatWord).
    if s.fm.height > old && page.runs.len() > s.line_start {
        let want = match s.prev {
            Some(p) => p + s.fm.height,
            None => s.fm.height - s.fm.descent,
        };
        if want > s.y {
            for r in &mut page.runs[s.line_start..] {
                if r.y == s.y {
                    r.y = want;
                }
            }
            s.y = want;
        }
    }
}

fn step<'a>(
    doc: &Doc<'a>,
    s: &mut State,
    page: &mut Page<'a>,
    items: &[Item<'a>],
    i: usize,
    opts: &Options<'_>,
    m: &mut dyn Metrics,
) {
    match &items[i] {
        Item::Word(w) => {
            let wid = m.width(s.font, s.style, w);
            if s.realign {
                s.realign = false;
                if s.align & 1 == 0 && !s.mono {
                    let len = line_len(s, items, i, m);
                    s.x = if len < s.right {
                        if s.align & 2 != 0 { s.right - len } else { (s.right - len) / 2 }
                    } else {
                        s.fm.space
                    };
                }
            }
            if !s.mono && s.wrap {
                if s.nowrap > 0 {
                    s.nowrap -= 1;
                } else if s.spacing {
                    if s.x + wid > s.right && s.x > s.left {
                        new_line(s, page, 1);
                    }
                } else {
                    let (total, words) = glued(s, items, i, m);
                    if s.x + total > s.right && s.x > s.left && s.x - s.left > s.fm.space {
                        new_line(s, page, 1);
                    }
                    s.nowrap = words - 1;
                }
            }
            page.runs.push(Run {
                x: s.x,
                y: s.y,
                width: wid,
                height: s.fm.height,
                descent: s.fm.descent,
                font: s.font,
                style: s.style,
                fg: s.fg,
                bg: s.bg,
                link: s.link,
                kind: RunKind::Word(w),
            });
            s.x += wid;
            s.max_x = s.max_x.max(s.x);
            if s.spacing && !s.mono {
                s.x += s.fm.space;
            }
        }
        Item::Para => {
            let n = if s.x > s.left { 2 } else { 1 };
            new_line(s, page, n);
            s.spacing = !s.ddf;
            s.realign = true;
            s.nowrap = 0;
        }
        Item::Break => {
            new_line(s, page, 1);
            s.spacing = !s.ddf;
            s.realign = true;
            s.nowrap = 0;
        }
        Item::Space => {
            s.x += if s.spacing { 2 * s.fm.space } else { s.fm.space };
        }
        Item::ToggleSpacing => s.spacing = !s.spacing,
        Item::Center => {
            new_line(s, page, 1);
            let len = line_len(s, items, i + 1, m);
            s.x = if len <= s.right { (s.right - len) / 2 } else { s.fm.space };
        }
        Item::LeftMargin(n) => {
            s.left = margin(s, *n);
            if s.x < s.left {
                s.x = s.left;
            }
        }
        Item::RightMargin(n) => {
            let n = *n as i32 * s.unit;
            s.right = if n == 0 {
                s.width - s.unit
            } else if s.width < n + s.left + s.unit {
                if s.unit + s.left < s.width { s.unit + s.left } else { 0 }
            } else {
                s.width - n
            };
        }
        Item::MarginNewLine(n) => {
            s.left = margin(s, *n);
            new_line(s, page, 1);
        }
        Item::MarginIfPast(n) => {
            s.left = margin(s, *n);
            if s.x > s.left {
                new_line(s, page, 1);
            }
            s.x = s.left;
        }
        Item::MarginHere => s.left = s.x,
        Item::MarginOrSpace(n) => {
            s.left = margin(s, *n);
            s.x = if s.left <= s.x + s.unit { s.x + s.unit } else { s.left };
        }
        Item::LinesBegin(a) => {
            s.align = *a;
            let n = if s.x > s.left { 2 } else { 1 };
            new_line(s, page, n);
            if s.spacing {
                s.x += s.fm.space;
            }
            s.wrap = false;
            s.realign = true;
        }
        Item::LinesEnd => {
            s.wrap = true;
            s.align = 1;
        }
        Item::XmpBegin => {
            let n = if s.x > s.left { 2 } else { 1 };
            new_line(s, page, n);
            if s.font == 0 {
                s.font = 1;
                s.auto_mono = true;
                set_font(s, page, m);
            }
            s.mono = true;
        }
        Item::XmpEnd => {
            if s.auto_mono && s.font == 1 {
                s.font = 0;
                s.auto_mono = false;
                s.fm = m.font(s.font, s.style);
            }
            s.mono = false;
            let r = (s.y + s.sys.descent) % s.fm.height.max(1);
            if r != 0 {
                s.y += s.fm.height - r;
            }
        }
        Item::Style(st) => {
            s.style = *st & 7;
            set_font(s, page, m);
        }
        Item::SpecialColor(c) | Item::Foreground(c) => s.fg = *c,
        Item::Background(c) => s.bg = *c,
        Item::Font(f) => {
            s.font = *f;
            s.style = 0;
            s.auto_mono = false;
            set_font(s, page, m);
        }
        Item::Link { target, data } => {
            let link = Link { target: target.clone(), data: *data };
            let none = matches!(target, Target::Res(r) | Target::Inform(r) if *r >= 0xfffe);
            if data.is_some_and(|d| d.auto()) {
                page.auto.push(link);
            } else if !none {
                page.links.push(link);
                s.link = Some(page.links.len() as u16 - 1);
            }
        }
        Item::EndLink => s.link = None,
        Item::Bitmap { flags, offset, areas } => {
            let Ok(info) = bitmap::info(doc.data(), doc.header.image_start, *offset) else { return };
            let (w, h) = (info.cx as i32, info.cy as i32);
            let (x, y, fit) = place_bitmap(s, page, w, h, *flags);
            page.runs.push(Run {
                x,
                y,
                width: w,
                height: h,
                descent: 0,
                font: s.font,
                style: s.style,
                fg: s.fg,
                bg: s.bg,
                link: if *areas { None } else { s.link },
                kind: RunKind::Bitmap { offset: *offset, flags: *flags, cx: w, cy: h, fit },
            });
            s.art = if *areas { Some(page.runs.len() - 1) } else { None };
        }
        Item::Image { handle, flags } => {
            let (w, h) = m.image(*handle);
            let (x, y, fit) = place_bitmap(s, page, w, h, *flags);
            page.runs.push(Run {
                x,
                y,
                width: w,
                height: h,
                descent: 0,
                font: s.font,
                style: s.style,
                fg: s.fg,
                bg: s.bg,
                link: s.link,
                kind: RunKind::Image { handle: *handle, metafile: false, cx: w, cy: h, fit },
            });
            s.art = None;
        }
        Item::Metafile { handle, rect } => {
            // An empty rectangle: the whole window (IBM's DdfFormatCell).
            let empty = rect[2] == 0 || rect[3] == 0;
            let (w, h) = if empty { (s.width, 0) } else { (rect[2] - rect[0], rect[3] - rect[1]) };
            let (x, y, fit) = place_bitmap(s, page, w, h, if empty { 0x08 } else { 0x10 });
            page.runs.push(Run {
                x,
                y,
                width: w,
                height: h,
                descent: 0,
                font: s.font,
                style: s.style,
                fg: s.fg,
                bg: s.bg,
                link: s.link,
                kind: RunKind::Image { handle: *handle, metafile: true, cx: w, cy: h, fit },
            });
            s.art = None;
        }
        Item::Area(a) => {
            if let Some(run) = s.art {
                if a.data.is_some_and(|d| d.auto()) {
                    page.auto.push(Link { target: a.target.clone(), data: a.data });
                } else {
                    page.areas.push((run, a.clone()));
                }
            }
        }
        Item::AcViewport(a) => page.acvps.push(a.clone()),
        Item::HideBegin(key) => {
            if !key_listed(opts.keys, key) {
                s.hiding = true;
            }
        }
        Item::DdfBegin => {
            s.ddf = true;
            s.ddf_spacing = s.spacing;
            s.spacing = false;
        }
        Item::DdfEnd => {
            s.ddf = false;
            s.spacing = s.ddf_spacing;
        }
        Item::FaceFont { face, width, height } => {
            s.font = m.face(face, *width, *height);
            s.style = 0;
            s.auto_mono = false;
            set_font(s, page, m);
        }
        Item::HideEnd | Item::Ddf(_) | Item::Other(..) => {}
    }
}

/// Place a bitmap of `w` by `h` with alignment `flags` (IBM's
/// ArtFormatBitmap; see the crate's notes): its x, bottom y and whether
/// it is stretched over the window.
fn place_bitmap(s: &mut State, page: &mut Page<'_>, w: i32, h: i32, flags: u8) -> (i32, i32, bool) {
    let (slh, sdesc) = (s.sys.height.max(1), s.sys.descent);
    let hh = h + sdesc;
    let fit = flags & 0x08 != 0 && flags & 0x06 == 0;
    let (x, y);
    if fit {
        (x, y) = (-1, -1);
    } else if flags & 0x10 != 0 {
        if s.x + w < s.right {
            x = s.x;
            let gap = s.y - s.prev.unwrap_or(-sdesc);
            if gap < hh {
                let ny = s.y + (hh / slh) * slh;
                for r in &mut page.runs[s.line_start..] {
                    if r.y == s.y {
                        r.y = ny;
                    }
                }
                y = ny;
            } else {
                y = s.y;
            }
            s.x += w + s.fm.space;
        } else {
            x = s.left;
            y = s.y + (hh / slh + 1) * slh;
            s.x = s.left + w + s.fm.space;
            s.line_start = page.runs.len();
        }
        s.y = y;
    } else {
        x = if flags & 0x02 != 0 {
            s.right - w
        } else if flags & 0x04 != 0 {
            if s.right - s.left > w { (s.right - s.left - w) >> 1 } else { s.left }
        } else {
            s.left
        };
        y = s.y + (hh / slh + 1) * slh;
        s.prev = Some(y);
        s.y = y + slh;
        s.x = s.left;
        s.line_start = page.runs.len() + 1;
    }
    if !fit {
        s.max_x = s.max_x.max(x + w);
    }
    (x, y, fit)
}
