//! WinHelp topics to IPF source, compiled with `ipf::compile`.

use crate::help::{hash, Font, Help};
use crate::hfs::Hfs;
use crate::pict::{self, Picture};
use crate::topic::{decode, Ev, Hot, Para, Pic, TL_DISPLAY, TL_DISPLAY30, TL_TABLE, TL_TOPICHDR};
use crate::{cp, Converted, Error, Options};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

/// Paragraph units (1/144 inch, as WinHelp stores indents) per IPF
/// margin unit (an average character of the system font).
const UNITS_PER_CHAR: i32 = 12;

/// IPF fonts by face: the point sizes the help manager has.
const FACES: [(&[u8], &[u16]); 3] = [(b"Helv", &[8, 10, 12, 14, 18, 24]), (b"Tms Rmn", &[8, 10, 12, 14, 18, 24]), (b"Courier", &[8, 10, 12])];

/// How a WinHelp font shows: IPF face (index into FACES) and size,
/// IPF style (1 italic, 2 bold, 4 underline), colour index, and how its
/// characters map (0 text, 1 Symbol, 2 Wingdings).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct FontMap {
    face: u8,
    size: u16,
    style: u8,
    colour: u8,
    charset: u8,
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

fn map_font(f: &Font) -> FontMap {
    let face = &f.face;
    let charset = if contains(face, b"symbol") {
        1
    } else if contains(face, b"wingding") {
        2
    } else {
        0
    };
    let fi = if contains(face, b"courier") || contains(face, b"mono") || contains(face, b"fixed") || contains(face, b"terminal") || contains(face, b"console") {
        2
    } else if contains(face, b"sans") || contains(face, b"helv") || contains(face, b"arial") || contains(face, b"swiss") {
        0
    } else if contains(face, b"roman") || contains(face, b"times") || contains(face, b"tms") || contains(face, b"serif") || f.family == 2 {
        1
    } else if f.family == 1 {
        2
    } else {
        0
    };
    let pt = (f.half_points as u16 + 1) / 2;
    let sizes = FACES[fi].1;
    let size = *sizes.iter().min_by_key(|&&s| (s as i32 - pt as i32).abs()).unwrap_or(&10);
    let mut style = 0;
    if f.attr & 0x01 != 0 {
        style |= 2;
    }
    if f.attr & 0x02 != 0 {
        style |= 1;
    }
    if f.attr & 0x14 != 0 {
        style |= 4;
    }
    FontMap { face: fi as u8, size, style, colour: colour(f.fg), charset }
}

/// The IPF colour index nearest an RGB colour; 0 (the window's text
/// colour) for "automatic" (1, 1, 0), black and white.
fn colour((r, g, b): (u8, u8, u8)) -> u8 {
    if (r, g, b) == (1, 1, 0) || (r < 0x30 && g < 0x30 && b < 0x30) || (r > 0xe0 && g > 0xe0 && b > 0xe0) {
        return 0;
    }
    const RGB: [(u8, u32); 14] = [
        (1, 0x0000ff),
        (2, 0xff0000),
        (3, 0xff00ff),
        (4, 0x00ff00),
        (5, 0x00ffff),
        (6, 0xffff00),
        (7, 0x808000),
        (8, 0x808080),
        (9, 0x000080),
        (10, 0x800000),
        (11, 0x800080),
        (12, 0x008000),
        (13, 0x008080),
        (15, 0xcccccc),
    ];
    let d = |c: u32| {
        let (cr, cg, cb) = ((c >> 16) as i32, (c >> 8 & 0xff) as i32, (c & 0xff) as i32);
        (cr - r as i32).pow(2) + (cg - g as i32).pow(2) + (cb - b as i32).pow(2)
    };
    RGB.iter().min_by_key(|(_, c)| d(*c)).map(|x| x.0).unwrap_or(0)
}

const COLOUR_NAMES: [&str; 16] = [
    "default", "blue", "red", "pink", "green", "cyan", "yellow", "neutral", "darkgray", "darkblue", "darkred", "darkpink", "darkgreen", "darkcyan", "black", "palegray",
];

/// Where a hotspot leads in this file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Jump(usize),
    Popup(usize),
}

/// A macro call's name and arguments (strings without their quotes).
fn macro_calls(m: &[u8]) -> Vec<(Vec<u8>, Vec<Vec<u8>>)> {
    let mut v = Vec::new();
    let mut p = 0;
    while p < m.len() {
        while p < m.len() && (m[p] == b' ' || m[p] == b';' || m[p] == b':') {
            p += 1;
        }
        let s = p;
        while p < m.len() && (m[p].is_ascii_alphanumeric() || m[p] == b'_') {
            p += 1;
        }
        let name = m[s..p].to_ascii_lowercase();
        while p < m.len() && m[p] == b' ' {
            p += 1;
        }
        let mut args = Vec::new();
        if p < m.len() && m[p] == b'(' {
            p += 1;
            let mut cur = Vec::new();
            let mut depth = 0;
            while p < m.len() {
                let c = m[p];
                match c {
                    b'"' | b'`' => {
                        let close = if c == b'"' { b'"' } else { b'\'' };
                        p += 1;
                        while p < m.len() && m[p] != close {
                            cur.push(m[p]);
                            p += 1;
                        }
                    }
                    b'(' => {
                        depth += 1;
                        cur.push(c);
                    }
                    b')' if depth > 0 => {
                        depth -= 1;
                        cur.push(c);
                    }
                    b')' => {
                        p += 1;
                        break;
                    }
                    b',' if depth == 0 => args.push(core::mem::take(&mut cur).trim_ascii().to_vec()),
                    _ => cur.push(c),
                }
                p += 1;
            }
            args.push(cur.trim_ascii().to_vec());
        } else if name.is_empty() {
            p += 1;
            continue;
        }
        v.push((name, args));
    }
    v
}

fn parse_num(s: &[u8]) -> Option<u32> {
    let s = core::str::from_utf8(s).ok()?.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(h, 16).ok()
    } else {
        s.parse::<i64>().ok().map(|v| v as u32)
    }
}

struct Ctx<'h, 'a> {
    h: &'h Help<'a>,
    opts: &'h Options<'h>,
    /// Context hashes to topics, sorted.
    contexts: Vec<(u32, usize)>,
    map: Vec<(u32, usize)>,
    contents: usize,
    fonts: Vec<FontMap>,
    /// The topics that are emitted (with their res = index + 1).
    count: usize,
}

impl Ctx<'_, '_> {
    /// Whether a file named in a jump or macro is this one.
    fn own(&self, file: &[u8]) -> bool {
        let f = file.split(|&c| c == b'>').next().unwrap_or(b"");
        let f = f.trim_ascii();
        if f.is_empty() || f.eq_ignore_ascii_case(b"qchPath") {
            return true;
        }
        let base = f.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(f);
        self.opts.name.is_some_and(|n| {
            let nb = n.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(n);
            nb.eq_ignore_ascii_case(base)
        })
    }

    fn by_hash(&self, h: u32) -> Option<usize> {
        let i = self.contexts.binary_search_by_key(&h, |c| c.0).ok()?;
        Some(self.contexts[i].1)
    }

    fn by_map(&self, n: u32) -> Option<usize> {
        let i = self.map.binary_search_by_key(&n, |c| c.0).ok()?;
        Some(self.map[i].1)
    }

    fn target(&self, hot: &Hot<'_>) -> Option<Target> {
        let t = match *hot {
            Hot::Jump(v) => Target::Jump(self.h.jump_target(v)?),
            Hot::Popup(v) => Target::Popup(self.h.jump_target(v)?),
            Hot::JumpHash { hash, file, .. } => {
                if file.is_some_and(|f| !self.own(f)) {
                    return None;
                }
                Target::Jump(self.by_hash(hash)?)
            }
            Hot::PopupHash { hash, file } => {
                if file.is_some_and(|f| !self.own(f)) {
                    return None;
                }
                Target::Popup(self.by_hash(hash)?)
            }
            Hot::Macro(m) => return self.macro_target(m),
        };
        match t {
            Target::Jump(i) | Target::Popup(i) if i >= self.count => None,
            _ => Some(t),
        }
    }

    /// The first jump or popup of a macro string that leads into this
    /// file: JumpId, PopupId, JumpContext, PopupContext, JumpHash,
    /// PopupHash, JumpContents and Contents, and their short names.
    fn macro_target(&self, m: &[u8]) -> Option<Target> {
        for (name, args) in macro_calls(m) {
            let a = |i: usize| args.get(i).map(|v| v.as_slice()).unwrap_or(b"");
            let t = match name.as_slice() {
                b"jumpid" | b"ji" if self.own(a(0)) => self.by_hash(hash(a(1))).map(Target::Jump),
                b"popupid" | b"pi" if self.own(a(0)) => self.by_hash(hash(a(1))).map(Target::Popup),
                b"jumpcontext" | b"jc" if self.own(a(0)) => parse_num(a(1)).and_then(|n| self.by_map(n)).map(Target::Jump),
                b"popupcontext" | b"pc" if self.own(a(0)) => parse_num(a(1)).and_then(|n| self.by_map(n)).map(Target::Popup),
                b"jumphash" | b"jh" if self.own(a(0)) => parse_num(a(1)).and_then(|n| self.by_hash(n)).map(Target::Jump),
                b"popuphash" | b"ph" if self.own(a(0)) => parse_num(a(1)).and_then(|n| self.by_hash(n)).map(Target::Popup),
                b"jumpcontents" if self.own(a(0)) => Some(Target::Jump(self.contents)),
                b"contents" => Some(Target::Jump(self.contents)),
                _ => None,
            };
            if let Some(t) = t {
                if matches!(t, Target::Jump(i) | Target::Popup(i) if i < self.count) {
                    return Some(t);
                }
            }
        }
        None
    }

    /// Walk the records of topic `i` (after its header, up to the next).
    fn records(&self, i: usize, f: &mut dyn FnMut(u8, &[u8], &[u8])) {
        let t = &self.h.topics;
        let mut first = true;
        t.links_from(self.h.list[i].at, self.h.phrases.as_ref(), |l| {
            if l.kind == TL_TOPICHDR {
                if !first {
                    return false;
                }
                first = false;
                return true;
            }
            if matches!(l.kind, TL_DISPLAY | TL_TABLE | TL_DISPLAY30) {
                f(l.kind, t.data1(l), &l.data2);
            }
            true
        });
    }

    /// The topics topic `i` jumps to (not popups), in order.
    fn jumps(&self, i: usize) -> Vec<usize> {
        let mut v = Vec::new();
        self.records(i, &mut |kind, d1, d2| {
            let _ = decode(kind, d1, d2, &mut |e| {
                if let Ev::HotStart(h, _) = e {
                    if let Some(Target::Jump(t)) = self.target(&h) {
                        if !v.contains(&t) {
                            v.push(t);
                        }
                    }
                }
            });
        });
        v
    }
}

/// Pending vertical space before the next text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Gap {
    None,
    Break,
    Para,
}

/// A table being written: the margin of each column.
struct Table {
    margins: Vec<u8>,
    last: i16,
}

/// Writes one topic's IPF text.
struct Writer<'c, 'h, 'a> {
    c: &'c Ctx<'h, 'a>,
    out: Vec<u8>,
    /// The first words of the text (a title for untitled topics).
    plain: Vec<u8>,
    font: Option<FontMap>,
    /// The IPF font, style and colour in effect.
    ipf_font: Option<(u8, u16)>,
    style: u8,
    colour: u8,
    link: bool,
    para: Para,
    /// Left margin of the paragraph's first line and of the others.
    first_margin: u8,
    left_margin: u8,
    margin: u8,
    tabs: usize,
    gap: Gap,
    /// Anything written since the last paragraph.
    content: bool,
    table: Option<Table>,
    /// A table ended with the last record (the next may be its next row).
    table_ended: bool,
    pics: &'c RefCell<Pics>,
    centred_pic: bool,
}

/// The pictures the topics use, by name: `bmN` files and embedded
/// ones (`E<n>`), decoded when the compiler loads them.
struct Pics {
    embedded: Vec<Vec<u8>>,
    /// Hotspots of the decoded pictures by name.
    decoded: BTreeMap<Vec<u8>, Option<(u32, u32, Vec<pict::Hotspot>)>>,
}

impl<'c, 'h, 'a> Writer<'c, 'h, 'a> {
    fn new(c: &'c Ctx<'h, 'a>, pics: &'c RefCell<Pics>) -> Self {
        Writer {
            c,
            out: Vec::new(),
            plain: Vec::new(),
            font: None,
            ipf_font: None,
            style: 0,
            colour: 0,
            link: false,
            para: Para::default(),
            first_margin: 1,
            left_margin: 1,
            margin: 1,
            tabs: 0,
            gap: Gap::None,
            content: false,
            table: None,
            table_ended: false,
            pics,
            centred_pic: false,
        }
    }

    fn put(&mut self, s: &[u8]) {
        self.out.extend_from_slice(s);
    }

    fn line(&mut self) {
        if !self.out.is_empty() && self.out.last() != Some(&b'\n') {
            self.out.push(b'\n');
        }
    }

    fn set_margin(&mut self, m: u8) {
        if m != self.margin {
            self.margin = m;
            self.put(format!(":lm margin={m}.").as_bytes());
        }
    }

    /// Write the pending paragraph gap before text.
    fn flush_gap(&mut self) {
        match self.gap {
            Gap::None => {}
            Gap::Break => {
                self.line();
                self.put(b".br\n");
            }
            Gap::Para => {
                self.line();
                self.put(b":p.");
            }
        }
        self.gap = Gap::None;
    }

    fn apply_font(&mut self) {
        let Some(f) = self.font else { return };
        let face = (f.face, f.size);
        if self.ipf_font != Some(face) {
            self.ipf_font = Some(face);
            self.put(format!(":font facename='{}' size={}x{}.", core::str::from_utf8(FACES[f.face as usize].0).unwrap_or("Helv"), f.size, f.size).as_bytes());
            // A font change ends the style.
            self.style = 0;
        }
        if self.style != f.style {
            if f.style == 0 {
                self.put(b":ehp1.");
            } else {
                let hp = match f.style {
                    1 => 1,
                    2 => 2,
                    3 => 3,
                    4 => 5,
                    5 => 6,
                    _ => 7,
                };
                self.put(format!(":hp{hp}.").as_bytes());
            }
            self.style = f.style;
        }
        if self.colour != f.colour {
            self.colour = f.colour;
            self.put(format!(":color fc={}.", COLOUR_NAMES[f.colour as usize]).as_bytes());
        }
    }

    /// Before text or a picture: the gap, the font.
    fn begin_content(&mut self) {
        self.flush_gap();
        self.apply_font();
        self.content = true;
    }

    fn text(&mut self, s: &[u8]) {
        self.begin_content();
        let charset = self.font.map_or(0, |f| f.charset);
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            if c == b' ' {
                let mut n = 1;
                while i + n < s.len() && s[i + n] == b' ' {
                    n += 1;
                }
                let at_start = self.out.last().is_none_or(|&l| l == b'\n' || l == b'.');
                if n == 1 && !at_start {
                    self.out.push(b' ');
                } else {
                    for _ in 0..n {
                        self.put(b"&rbl.");
                    }
                }
                if self.plain.len() < 80 && self.plain.last().is_some_and(|&l| l != b' ') {
                    self.plain.push(b' ');
                }
                i += n;
                continue;
            }
            i += 1;
            if c < 0x20 {
                continue;
            }
            if c == 0xa0 && charset == 0 {
                self.put(b"&rbl.");
                continue;
            }
            let mut buf = Vec::new();
            match charset {
                1 => buf.push(cp::symbol(c)),
                2 => buf.push(cp::wingdings(c)),
                _ => cp::push(&mut buf, c),
            }
            for &b in &buf {
                if self.plain.len() < 80 {
                    self.plain.push(b);
                }
                match b {
                    b':' => self.put(b"&colon."),
                    b'&' => self.put(b"&amp."),
                    b'.' if self.out.last().is_none_or(|&l| l == b'\n') => self.put(b"&per."),
                    _ => self.out.push(b),
                }
            }
        }
    }

    fn end_link(&mut self) {
        if self.link {
            self.link = false;
            self.put(b":elink.");
        }
    }

    /// The margin of tab stop `n` of the paragraph (in characters).
    fn tab_margin(&self, n: usize) -> u8 {
        let p = &self.para;
        let first = (p.left as i32 + p.first as i32).max(0);
        let mut stops: Vec<i32> = p.tabs.iter().map(|t| t.0 as i32).filter(|&t| t > first).collect();
        // A hanging indent is a tab stop.
        if p.first < 0 && !stops.contains(&(p.left as i32)) {
            stops.push(p.left as i32);
        }
        stops.sort_unstable();
        let pos = match stops.get(n) {
            Some(&s) => s,
            None => {
                // Default stops every half inch past the last.
                let last = stops.last().copied().unwrap_or(first);
                let k = (n - stops.len()) as i32 + 1;
                (last / 72 + k) * 72
            }
        };
        chars(pos)
    }

    fn event(&mut self, e: Ev<'_>) {
        match e {
            Ev::Table(cols) => {
                self.end_link();
                // Rows are records of their own: a row after a row is a
                // line on.
                if self.table_ended {
                    self.gap = self.gap.max(Gap::Break);
                } else if self.content {
                    self.gap = self.gap.max(Gap::Para);
                }
                self.table_ended = false;
                let mut margins: Vec<u8> = cols.iter().map(|&x| chars(x)).collect();
                // Columns at least a few characters apart.
                for i in 1..margins.len() {
                    if margins[i] < margins[i - 1].saturating_add(4) {
                        margins[i] = margins[i - 1].saturating_add(4);
                    }
                }
                self.table = Some(Table { margins, last: i16::MAX - 1 });
            }
            Ev::Cell(col) => {
                self.end_link();
                let Some(t) = self.table.as_mut() else { return };
                // A column again: another paragraph of the cell.
                let same = col == t.last;
                let new_row = col < t.last;
                t.last = col;
                let m = t.margins.get(col.max(0) as usize).copied().unwrap_or(1);
                if same {
                    if self.content {
                        self.gap = self.gap.max(Gap::Break);
                    }
                } else if new_row {
                    if self.content {
                        self.gap = self.gap.max(Gap::Break);
                    }
                    // The margin before the break: the row starts at it.
                    self.set_margin(m);
                    self.flush_gap();
                    self.content = false;
                } else {
                    self.gap = Gap::None;
                    self.margin = m;
                    self.put(format!(":tab margin={m}.").as_bytes());
                    self.content = false;
                }
            }
            Ev::TableEnd => {
                self.end_link();
                self.table = None;
                self.table_ended = true;
                self.content = true;
            }
            Ev::Para(p) => {
                self.tabs = 0;
                self.centred_pic = p.align == 2;
                if self.table.is_some() {
                    self.para = p;
                    return;
                }
                if self.table_ended {
                    self.table_ended = false;
                    self.gap = self.gap.max(Gap::Para);
                }
                let first = chars(p.left as i32 + p.first as i32);
                let left = chars(p.left as i32);
                self.first_margin = first;
                self.left_margin = left;
                if self.content || self.gap != Gap::None {
                    let space = self.para.below.max(0) + p.above.max(0);
                    self.gap = self.gap.max(if space >= 4 { Gap::Para } else { Gap::Break });
                }
                self.para = p;
                // The margin goes before the pending break so that the
                // new line starts at it.
                self.set_margin(first);
            }
            Ev::Text(s) => self.text(s),
            Ev::Font(n) => {
                self.font = self.c.fonts.get(n as usize).copied().or(self.font);
            }
            Ev::Break => {
                self.end_link_if_table();
                if self.table.is_none() {
                    self.set_margin(self.left_margin);
                }
                self.gap = self.gap.max(Gap::Break);
                self.tabs = 0;
            }
            Ev::ParaEnd => {
                self.end_link();
                if self.table.is_some() {
                    // Several paragraphs in a cell: line breaks.
                    if self.content {
                        self.gap = Gap::Break;
                    }
                } else {
                    // The record's next paragraph has the same settings.
                    self.set_margin(self.first_margin);
                    let space = self.para.below.max(0) + self.para.above.max(0);
                    if self.content {
                        self.gap = self.gap.max(if space >= 4 { Gap::Para } else { Gap::Break });
                    }
                    self.content = false;
                }
                self.tabs = 0;
            }
            Ev::Tab => {
                if self.table.is_some() {
                    self.text(b"  ");
                    return;
                }
                let m = self.tab_margin(self.tabs);
                self.tabs += 1;
                self.flush_gap();
                self.margin = m;
                self.put(format!(":tab margin={m}.").as_bytes());
                self.content = true;
            }
            Ev::NbSpace => {
                self.begin_content();
                self.put(b"&rbl.");
            }
            Ev::NbHyphen => self.text(b"-"),
            Ev::Pict(align, pic) => self.picture(align, pic),
            Ev::HotStart(hot, _) => {
                self.end_link();
                if let Some(t) = self.c.target(&hot) {
                    self.begin_content();
                    match t {
                        Target::Jump(i) => self.put(format!(":link reftype=hd res={}.", i + 1).as_bytes()),
                        Target::Popup(i) => self.put(format!(":link reftype=fn res={}.", i + 1).as_bytes()),
                    }
                    self.link = true;
                }
            }
            Ev::HotEnd => self.end_link(),
        }
    }

    fn end_link_if_table(&mut self) {
        if self.table.is_some() {
            self.end_link();
        }
    }

    fn picture(&mut self, align: u8, pic: Pic<'_>) {
        let name: Vec<u8> = match pic {
            Pic::Num(n) => format!("bm{n}").into_bytes(),
            Pic::Data(d) => {
                let mut p = self.pics.borrow_mut();
                p.embedded.push(d.to_vec());
                format!("E{}", p.embedded.len() - 1).into_bytes()
            }
        };
        // Decode it now to know whether it is there and its hotspots.
        let info = {
            let mut p = self.pics.borrow_mut();
            if !p.decoded.contains_key(&name) {
                let d = load_pic(&self.c.h.hfs, &p, &name).map(|pic| (pic.cx, pic.cy, pic.hotspots));
                p.decoded.insert(name.clone(), d);
            }
            p.decoded[&name].clone()
        };
        let Some((cx, cy, hotspots)) = info else {
            // A metafile or a missing picture.
            return;
        };
        // In the text (bmc), except alone in a centred paragraph; small
        // pictures at the left (bml, text beside them) too.
        let inline = (align == 0 && !(self.centred_pic && !self.content)) || (align == 1 && cx <= 160);
        self.begin_content();
        let a = match align {
            _ if inline => "",
            1 => " align=left",
            2 => " align=right",
            _ if self.centred_pic => " align=center",
            _ => "",
        };
        let areas: Vec<(Target, &pict::Hotspot)> = hotspots
            .iter()
            .filter_map(|hs| {
                let hot = match hs.kind {
                    0xe2 | 0xe6 => Hot::PopupHash { hash: hs.hash, file: None },
                    0xe3 | 0xe7 => Hot::JumpHash { hash: hs.hash, file: None, window: None },
                    0xc8 | 0xcc => Hot::Macro(&hs.target),
                    _ => return None,
                };
                Some((self.c.target(&hot)?, hs))
            })
            .collect();
        // A link around a picture with areas would hide them; areas need
        // a picture on its own line.
        let in_link = self.link;
        let runin = if inline && (areas.is_empty() || in_link) { " runin" } else { "" };
        self.put(format!(":artwork name='{}'{a}{runin}.", core::str::from_utf8(&name).unwrap_or("")).as_bytes());
        if !areas.is_empty() && !in_link {
            self.line();
            self.put(b":artlink.\n");
            for (t, hs) in areas {
                let y = (cy as i32 - hs.y as i32 - hs.cy as i32).max(0);
                let (kind, i) = match t {
                    Target::Jump(i) => ("hd", i),
                    Target::Popup(i) => ("fn", i),
                };
                self.put(format!(":link reftype={kind} res={} x={} y={y} cx={} cy={}.\n", i + 1, hs.x, hs.cx, hs.cy).as_bytes());
            }
            self.put(b":eartlink.\n");
        }
    }
}

/// Paragraph units to an IPF margin (at least 1).
fn chars(units: i32) -> u8 {
    (1 + (units.max(0) + UNITS_PER_CHAR / 2) / UNITS_PER_CHAR).min(200) as u8
}

fn load_pic(hfs: &Hfs<'_>, p: &Pics, name: &[u8]) -> Option<Picture> {
    let data: &[u8] = if let Some(n) = name.strip_prefix(b"E") {
        let i: usize = core::str::from_utf8(n).ok()?.parse().ok()?;
        p.embedded.get(i)?.as_slice()
    } else {
        let mut pipe = b"|".to_vec();
        pipe.extend_from_slice(name);
        hfs.file(name).or_else(|| hfs.file(&pipe))?
    };
    pict::decode(data)
}

/// Text for a heading or index line: code page 437, no line ends.
fn line_text(s: &[u8], max: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for &c in s {
        if c < 0x20 {
            v.push(b' ');
        } else {
            cp::push(&mut v, c);
        }
    }
    let t = v.trim_ascii();
    t[..t.len().min(max)].to_vec()
}

pub fn convert(data: &[u8], opts: &Options<'_>) -> Result<Converted, Error> {
    let mut h = Help::parse(data)?;
    let mut warnings: Vec<String> = Vec::new();
    let n = h.list.len();
    if n == 0 {
        return Err(Error::Invalid("no topics"));
    }
    // res = topic index + 1, so at most 65534 topics.
    let count = n.min(0xfffe);
    if count < n {
        warnings.push(format!("{} topics past the 65534th left out", n - count));
    }
    let mut contexts: Vec<(u32, usize)> = h.contexts().into_iter().filter_map(|(hv, o)| Some((hv, h.topic_at(o)?))).collect();
    contexts.sort_unstable();
    let mut map: Vec<(u32, usize)> = h.map().into_iter().filter_map(|(id, o)| Some((id, h.topic_at(o)?))).collect();
    map.sort_unstable();
    let contents = h.contents.and_then(|c| h.topic_at(c)).filter(|&c| c < count).unwrap_or(0);
    let fonts: Vec<FontMap> = h.fonts.iter().map(map_font).collect();
    let ctx = Ctx { h: &h, opts, contexts, map, contents, fonts, count };

    // Keywords: per topic for the index, and the lookup table.
    let mut kw_topics: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut keywords: Vec<(Vec<u8>, Vec<u16>)> = Vec::new();
    let mut index_entries = 0usize;
    for (k, offs) in h.keywords() {
        let mut res: Vec<u16> = Vec::new();
        for o in offs {
            if let Some(t) = h.topic_at(o).filter(|&t| t < count) {
                if !res.contains(&(t as u16 + 1)) {
                    res.push(t as u16 + 1);
                }
            }
        }
        if res.is_empty() {
            continue;
        }
        let ki = keywords.len();
        for &r in &res {
            if index_entries < 0xffff {
                kw_topics[r as usize - 1].push(ki);
                index_entries += 1;
            }
        }
        keywords.push((k, res));
    }
    if index_entries >= 0xffff {
        warnings.push(String::from("index entries past the 65535th left out"));
    }

    // The contents tree: the contents topic, the topics it jumps to and
    // theirs, while that stays small.
    let mut tree: Vec<(usize, u8)> = vec![(contents, 1)];
    let mut placed: BTreeSet<usize> = BTreeSet::new();
    placed.insert(contents);
    let level2: Vec<usize> = ctx.jumps(contents).into_iter().filter(|t| placed.insert(*t)).collect();
    let mut level3: Vec<Vec<usize>> = Vec::new();
    let mut total = 1 + level2.len();
    for &t in &level2 {
        let kids: Vec<usize> = if total < opts.max_contents { ctx.jumps(t).into_iter().filter(|k| !placed.contains(k)).collect() } else { Vec::new() };
        let kids: Vec<usize> = if total + kids.len() <= opts.max_contents { kids } else { Vec::new() };
        for &k in &kids {
            placed.insert(k);
        }
        total += kids.len();
        level3.push(kids);
    }
    for (i, &t) in level2.iter().enumerate() {
        tree.push((t, 2));
        for &k in &level3[i] {
            tree.push((k, 3));
        }
    }

    // The source, compiled a topic at a time.
    let pics = RefCell::new(Pics { embedded: Vec::new(), decoded: BTreeMap::new() });
    let hfs = &h.hfs;
    let load = |name: &[u8]| -> Option<Vec<u8>> {
        let p = pics.borrow();
        load_pic(hfs, &p, name).map(|p| p.bmp)
    };
    let mut comp = ipf::compile::Compiler::new(true, &load);
    let mut kept: Vec<u8> = Vec::new();
    let mut src: Vec<u8> = Vec::new();
    let mut feed = |src: &mut Vec<u8>| -> Result<(), ipf::compile::CompileError> {
        if opts.keep_source {
            kept.extend_from_slice(src);
        }
        let r = comp.feed(src);
        src.clear();
        r
    };
    let failed = |e: ipf::compile::CompileError, kept: Vec<u8>| Error::Compile { line: e.line, msg: e.msg, source: kept };
    src.extend_from_slice(b":userdoc.\n:title.");
    let title = line_text(&h.title, 47);
    src.extend_from_slice(&title);
    src.push(b'\n');
    let order = tree.iter().copied().chain((0..count).filter(|i| !placed.contains(i)).map(|i| (i, 0)));
    let mut result = Ok(());
    for (i, level) in order {
        let mut w = Writer::new(&ctx, &pics);
        ctx.records(i, &mut |kind, d1, d2| {
            let _ = decode(kind, d1, d2, &mut |e| w.event(e));
        });
        w.end_link();
        let mut t = line_text(&h.list[i].title, 120);
        if t.is_empty() {
            t = line_text(&w.plain, 60);
        }
        if t.is_empty() {
            t = title.clone();
        }
        let hide = if level == 0 { " hide" } else { "" };
        src.extend_from_slice(format!(":h{} res={}{hide}.", level.max(1), i + 1).as_bytes());
        src.extend_from_slice(&t);
        src.push(b'\n');
        for &k in &kw_topics[i] {
            src.extend_from_slice(b":i1.");
            src.extend_from_slice(&line_text(&keywords[k].0, 120));
            src.push(b'\n');
        }
        src.extend_from_slice(b":p.");
        src.extend_from_slice(&w.out);
        src.push(b'\n');
        result = feed(&mut src);
        if result.is_err() {
            break;
        }
    }
    if result.is_ok() {
        src.extend_from_slice(b":euserdoc.\n");
        result = feed(&mut src);
    }
    let Ctx { contexts, map, .. } = ctx;
    // The topics' text is not needed any more (the pictures are in the
    // file's data).
    h.topics.buf = Vec::new();
    let ipf = match result.and_then(|_| comp.finish()) {
        Ok(f) => f,
        Err(e) => return Err(failed(e, kept)),
    };

    keywords.sort_by(|a, b| a.0.to_ascii_uppercase().cmp(&b.0.to_ascii_uppercase()));
    let contexts = contexts.iter().filter(|c| c.1 < count).map(|&(hv, t)| (hv, t as u16 + 1)).collect();
    let map = map.iter().filter(|c| c.1 < count).map(|&(id, t)| (id, t as u16 + 1)).collect();
    Ok(Converted { ipf, title: h.title.clone(), contents_res: contents as u16 + 1, contexts, map, keywords, warnings, source: kept })
}
