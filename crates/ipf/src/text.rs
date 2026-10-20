//! Cell text: the bytes of a cell decoded into [`Item`]s.
//!
//! | Byte | Meaning |
//! |------|---------|
//! | below the header's local word limit (0xF5) | local dictionary word |
//! | 0xFA | end of paragraph (autospacing back on) |
//! | 0xFB | centre the rest of the line |
//! | 0xFC | autospacing on/off (on at the start of each cell) |
//! | 0xFD | line break |
//! | 0xFE | a space (two while autospacing) |
//! | 0xFF | escape: `FF len type data`, `len` counting itself and the type |
//!
//! Escapes (margins are in system font character widths):
//!
//! | Type | Data | Meaning |
//! |------|------|---------|
//! | 0x02 | n | left margin (`:lm.`, list indents) |
//! | 0x03 | n | right margin (0: default) |
//! | 0x04 | style | `:hp1.`-`:hp7.`: 1 italic, 2 bold, 4 underline, combined |
//! | 0x05 | u16 TOC, link data | link to a panel |
//! | 0x06 | u16 TOC | link to a panel |
//! | 0x07 | u16 TOC | footnote |
//! | 0x08 | | end of link |
//! | 0x0B / 0x0C | | `:xmp.` (monospaced, as it is) / end |
//! | 0x0D | colour | `:hp4.`, `:hp8.`, `:hp9.` colours (0: end) |
//! | 0x0E | flags, u32 offset | bitmap |
//! | 0x0F | subtype ... | bitmap with link areas (0) and its areas |
//! | 0x10 | m, "program params", link data at m | launch link |
//! | 0x11 | n | left margin on a new line |
//! | 0x12 | n | left margin, new line if past it |
//! | 0x13 / 0x14 | colour | foreground / background colour |
//! | 0x16 | u16 res, link data | inform link |
//! | 0x17 / 0x18 | key | `:hide.` text unless IPF_KEYS has the key / end |
//! | 0x19 | font | font table entry (also resets the style) |
//! | 0x1A / 0x1B | alignment | `:lines.` / end |
//! | 0x1C | | left margin at the current position |
//! | 0x1D, 0x1E | u16 res, link data | link to a panel by resource id |
//! | 0x1F | db, n, id\[n\], link data | link into an external database |
//! | 0x20 | u16 res | dynamic data (`:ddf.`) |
//! | 0x21 | ... | application-controlled viewport |
//!
//! Link data: `u16 flags`, then by flag 0x01 origin and 0x02 size (`u8
//! units, u16, u16` each), 0x08 a window id, 0x400 a group (u16 each);
//! flag 0x40: open automatically.

use crate::{Cell, Doc};
use alloc::vec::Vec;

/// Extra data of a link (`vpx=`, `group=`, `auto`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkData {
    pub flags: u16,
    pub origin: Option<(u8, u16, u16)>,
    pub size: Option<(u8, u16, u16)>,
    pub window: Option<u16>,
    pub group: Option<u16>,
}

impl LinkData {
    pub const AUTO: u16 = 0x40;

    fn parse(b: &[u8]) -> Option<Self> {
        let rd = |o: usize| -> Option<u16> { Some(u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?])) };
        let flags = rd(0)?;
        let mut d = LinkData { flags, ..Default::default() };
        let mut p = 2;
        if flags & 0x01 != 0 {
            d.origin = Some((*b.get(p)?, rd(p + 1)?, rd(p + 3)?));
            p += 5;
        }
        if flags & 0x02 != 0 {
            d.size = Some((*b.get(p)?, rd(p + 1)?, rd(p + 3)?));
            p += 5;
        }
        if flags & 0x08 != 0 {
            d.window = rd(p);
            p += 2;
        }
        if flags & 0x400 != 0 {
            d.group = rd(p);
        }
        Some(d)
    }

    pub fn auto(&self) -> bool {
        self.flags & Self::AUTO != 0
    }
}

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target<'a> {
    /// A panel of this file by TOC index.
    Panel(u16),
    /// A footnote panel.
    Footnote(u16),
    /// A panel by resource id.
    Res(u16),
    /// A message to the application (HM_INFORM) with this id.
    Inform(u16),
    /// A program to start: "program parameters".
    Launch(&'a [u8]),
    /// A panel of external database `db` (header's database list) by
    /// resource id or name `id`.
    External { db: u8, id: &'a [u8] },
}

/// A hypergraphic area of the last bitmap with link areas: `rect` (x, y
/// from the bitmap's bottom left, cx, cy) or the whole bitmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area<'a> {
    pub target: Target<'a>,
    pub rect: Option<(u16, u16, u16, u16)>,
    pub data: Option<LinkData>,
}

/// One unit of cell text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item<'a> {
    Word(&'a [u8]),
    Para,
    Center,
    ToggleSpacing,
    Break,
    Space,
    LeftMargin(u8),
    RightMargin(u8),
    MarginNewLine(u8),
    MarginIfPast(u8),
    MarginHere,
    Style(u8),
    SpecialColor(u8),
    Foreground(u8),
    Background(u8),
    Font(u8),
    XmpBegin,
    XmpEnd,
    LinesBegin(u8),
    LinesEnd,
    Link { target: Target<'a>, data: Option<LinkData> },
    EndLink,
    /// A bitmap: alignment flags (0x02 right, 0x04 centre, 0x08 fit, 0x10
    /// in the text) and its offset; `areas`: it has link areas.
    Bitmap { flags: u8, offset: u32, areas: bool },
    Area(Area<'a>),
    HideBegin(&'a [u8]),
    HideEnd,
    Ddf(u16),
    /// Another escape (type, data), ignored when formatting.
    Other(u8, &'a [u8]),
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?]))
}

/// Decode an escape: `e` is the whole escape from its FF.
fn escape(e: &[u8]) -> Option<Item<'_>> {
    let ty = *e.get(2)?;
    let len = e.len();
    let arg = || e.get(3).copied();
    let data_at = |o: usize| if len > o { LinkData::parse(&e[o..]) } else { None };
    Some(match ty {
        0x02 => Item::LeftMargin(arg()?),
        0x03 => Item::RightMargin(arg()?),
        0x04 => Item::Style(arg()?),
        0x05 => Item::Link { target: Target::Panel(u16_at(e, 3)?), data: data_at(5) },
        0x06 => Item::Link { target: Target::Panel(u16_at(e, 3)?), data: None },
        0x07 => Item::Link { target: Target::Footnote(u16_at(e, 3)?), data: None },
        0x08 => Item::EndLink,
        0x0b => Item::XmpBegin,
        0x0c => Item::XmpEnd,
        0x0d => Item::SpecialColor(arg()?),
        0x0e => Item::Bitmap { flags: arg()?, offset: u32_at(e, 4)?, areas: false },
        0x0f => return art(e),
        0x10 => {
            // m: where the link data starts (0: none).
            let m = arg()? as usize;
            let end = if m != 0 { m + 1 } else { len };
            let cmd = e.get(4..end.min(len))?;
            Item::Link { target: Target::Launch(cmd), data: if m != 0 && m + 1 < len { data_at(m + 1) } else { None } }
        }
        0x11 => Item::MarginNewLine(arg()?),
        0x12 => Item::MarginIfPast(arg()?),
        0x13 => Item::Foreground(arg()?),
        0x14 => Item::Background(arg()?),
        0x16 => Item::Link { target: Target::Inform(u16_at(e, 3)?), data: data_at(5) },
        0x17 => Item::HideBegin(&e[3..]),
        0x18 => Item::HideEnd,
        0x19 => Item::Font(arg()?),
        0x1a => Item::LinesBegin(arg()?),
        0x1b => Item::LinesEnd,
        0x1c => Item::MarginHere,
        0x1d => Item::Link { target: Target::Res(u16_at(e, 3)?), data: data_at(5) },
        0x1e => Item::Link { target: Target::Res(u16_at(e, 3)?), data: None },
        0x1f => {
            let db = arg()?;
            let n = *e.get(4)? as usize;
            let id = e.get(5..5 + n)?;
            Item::Link { target: Target::External { db, id }, data: data_at(5 + n) }
        }
        0x20 => Item::Ddf(u16_at(e, 3)?),
        _ => Item::Other(ty, &e[3..]),
    })
}

/// Escape 0x0F: a bitmap with link areas (subtype 0) and the areas.
fn art(e: &[u8]) -> Option<Item<'_>> {
    let sub = *e.get(3)?;
    let len = e.len();
    let rect = |o: usize| Some((u16_at(e, o)?, u16_at(e, o + 2)?, u16_at(e, o + 4)?, u16_at(e, o + 6)?));
    let data_at = |o: usize| if len > o { LinkData::parse(&e[o..]) } else { None };
    let area = |target, rect, data| Some(Item::Area(Area { target, rect, data }));
    match sub {
        0x00 => Some(Item::Bitmap { flags: *e.get(4)?, offset: u32_at(e, 5)?, areas: true }),
        0x01..=0x03 => {
            let t = u16_at(e, 4)?;
            let target = if sub == 0x03 { Target::Footnote(t) } else { Target::Panel(t) };
            area(target, rect(6), if sub == 0x01 { data_at(14) } else { None })
        }
        0x04..=0x06 => {
            let t = u16_at(e, 4)?;
            let target = if sub == 0x06 { Target::Footnote(t) } else { Target::Panel(t) };
            area(target, None, if sub == 0x04 { data_at(6) } else { None })
        }
        0x07 => area(Target::Launch(e.get(5..)?), None, None),
        0x08 => area(Target::Launch(e.get(13..)?), rect(5), None),
        0x09 => area(Target::Inform(u16_at(e, 4)?), None, data_at(6)),
        0x0a => area(Target::Inform(u16_at(e, 4)?), rect(6), data_at(14)),
        0x0c | 0x0d => area(Target::Res(u16_at(e, 4)?), rect(6), if sub == 0x0c { data_at(14) } else { None }),
        0x0e | 0x0f => area(Target::Res(u16_at(e, 4)?), None, if sub == 0x0e { data_at(6) } else { None }),
        0x10 => {
            let (db, n) = (*e.get(4)?, *e.get(5)? as usize);
            area(Target::External { db, id: e.get(6..6 + n)? }, None, None)
        }
        0x11 => {
            let (db, n) = (*e.get(4)?, *e.get(5)? as usize);
            area(Target::External { db, id: e.get(14..14 + n)? }, rect(6), None)
        }
        _ => Some(Item::Other(0x0f, &e[3..])),
    }
}

/// The items of a cell. Bytes that are not words (and words outside the
/// local dictionary) are left out.
pub fn decode<'a>(doc: &Doc<'a>, cell: &Cell<'a>) -> Vec<Item<'a>> {
    let limit = doc.data().get(0x52).copied().unwrap_or(0xf5).min(0xfa);
    let t = cell.text;
    let mut v = Vec::with_capacity(t.len());
    let mut p = 0;
    while p < t.len() {
        let b = t[p];
        match b {
            0xfa => v.push(Item::Para),
            0xfb => v.push(Item::Center),
            0xfc => v.push(Item::ToggleSpacing),
            0xfd => v.push(Item::Break),
            0xfe => v.push(Item::Space),
            0xff => {
                let n = t.get(p + 1).copied().unwrap_or(0) as usize;
                let end = (p + 1 + n).min(t.len());
                if n >= 2 {
                    if let Some(i) = escape(&t[p..end]) {
                        v.push(i);
                    }
                }
                p = end.max(p + 1);
                continue;
            }
            _ if b < limit => {
                if let Some(w) = cell.word(b) {
                    v.push(Item::Word(doc.word(w)));
                }
            }
            _ => {}
        }
        p += 1;
    }
    v
}

/// The items of panel `toc`: its cells' items, each cell starting with
/// autospacing on (a `None` between cells marks the start of a cell).
pub fn panel<'a>(doc: &Doc<'a>, toc: u16) -> Vec<Option<Item<'a>>> {
    let Ok(e) = doc.toc(toc) else { return Vec::new() };
    let mut v = Vec::new();
    for c in e.cells {
        let Ok(cell) = doc.cell(c) else { continue };
        v.push(None);
        v.extend(decode(doc, &cell).into_iter().map(Some));
    }
    v
}

/// Plain text of a panel (for searching and the clipboard): words with
/// the spacing they get, line ends at breaks and paragraphs.
pub fn plain(doc: &Doc<'_>, toc: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let mut spacing = true;
    let mut xmp = false;
    for i in panel(doc, toc) {
        match i {
            None => spacing = true,
            Some(Item::Word(w)) => {
                out.extend_from_slice(w);
                if spacing && !xmp {
                    out.push(b' ');
                }
            }
            Some(Item::ToggleSpacing) => spacing = !spacing,
            Some(Item::Space) => out.extend_from_slice(if spacing { b"  " } else { b" " }),
            Some(Item::Para) => {
                out.extend_from_slice(b"\r\n\r\n");
                spacing = true;
            }
            Some(Item::Break | Item::MarginNewLine(_) | Item::Center) => out.extend_from_slice(b"\r\n"),
            Some(Item::XmpBegin) => {
                xmp = true;
                out.extend_from_slice(b"\r\n");
            }
            Some(Item::XmpEnd) => xmp = false,
            _ => {}
        }
    }
    out
}
