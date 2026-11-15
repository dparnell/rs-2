//! The `|TOPIC` file: its blocks decompressed into one stream, the
//! TOPICLINK records walked in order, phrase decompression of their
//! text, and the decoding of displayable text records into [`Ev`]ents.

use crate::hfs::{lz77, u16_at, u32_at, Cur};
use alloc::vec;
use alloc::vec::Vec;

/// Record types.
pub const TL_TOPICHDR: u8 = 0x02;
pub const TL_DISPLAY30: u8 = 0x01;
pub const TL_DISPLAY: u8 = 0x20;
pub const TL_TABLE: u8 = 0x23;

/// Phrase tables: old style (`|Phrases`) or Hall's (`|PhrIndex` and
/// `|PhrImage`).
pub struct Phrases {
    /// Phrase i is `text[offs[i]..offs[i + 1]]`.
    offs: Vec<u32>,
    text: Vec<u8>,
    hall: bool,
}

impl Phrases {
    /// `|Phrases`: u16 count, u16 0x0100, (3.1: u32 size of the
    /// decompressed phrases,) u16 offsets\[count + 1\] from the start of
    /// the offsets, the phrases (3.1: LZ77).
    pub fn old(d: &[u8], v30: bool, compressed: bool) -> Option<Self> {
        let n = u16_at(d, 0)? as usize;
        let (offs_at, size) = if v30 { (4, 0) } else { (8, u32_at(d, 4)? as usize) };
        let mut offs = Vec::with_capacity(n + 1);
        for i in 0..=n {
            offs.push(u16_at(d, offs_at + 2 * i)? as u32);
        }
        let base = *offs.first()?;
        for o in &mut offs {
            *o = o.checked_sub(base)?;
        }
        let data = d.get(offs_at + 2 * (n + 1)..)?;
        let text = if compressed && !v30 {
            let mut t = Vec::with_capacity(size);
            lz77(data, &mut t, size.max(*offs.last()? as usize));
            t
        } else {
            data.to_vec()
        };
        Some(Phrases { offs, text, hall: false })
    }

    /// Hall compression: `|PhrIndex` (a 28 byte header: u32 magic, u32
    /// count, u32 compressed size, u32 size of `|PhrImage` decompressed,
    /// u32 its compressed size, u32 0, u16 bit count in its low 4 bits,
    /// u16), then the phrase lengths as a bit stream (bit 0 of each u32
    /// first): 1 + unary multiples of 2^bits, then the low bits (at
    /// least one); `|PhrImage` the phrases (LZ77 when its compressed
    /// size is less than its size). No file at hand uses it: untested.
    pub fn hall(index: &[u8], image: &[u8]) -> Option<Self> {
        let n = u32_at(index, 4)? as usize;
        let size = u32_at(index, 12)? as usize;
        let csize = u32_at(index, 16)? as usize;
        let bits = (u16_at(index, 24)? & 0x0f) as u32;
        let stream = index.get(28..)?;
        let mut bitpos = 0usize;
        let mut bit = || -> Option<bool> {
            let w = u32_at(stream, (bitpos / 32) * 4)?;
            let b = w >> (bitpos % 32) & 1 != 0;
            bitpos += 1;
            Some(b)
        };
        let mut offs = Vec::with_capacity(n + 1);
        let mut o = 0u32;
        offs.push(0);
        for _ in 0..n {
            let mut len = 1u32;
            while bit()? {
                len += 1 << bits;
            }
            if bit()? {
                len += 1;
            }
            for k in 1..bits {
                if bit()? {
                    len += 1 << k;
                }
            }
            o += len;
            offs.push(o);
        }
        let text = if csize < size {
            let mut t = Vec::with_capacity(size);
            lz77(image, &mut t, size);
            t
        } else {
            image.to_vec()
        };
        Some(Phrases { offs, text, hall: true })
    }

    fn phrase(&self, i: usize) -> &[u8] {
        match (self.offs.get(i), self.offs.get(i + 1)) {
            (Some(&s), Some(&e)) => self.text.get(s as usize..e as usize).unwrap_or(&[]),
            _ => &[],
        }
    }

    /// Expand phrase-compressed text into `out`.
    pub fn expand(&self, src: &[u8], out: &mut Vec<u8>) {
        let mut p = 0;
        if self.hall {
            while p < src.len() {
                let c = src[p] as usize;
                p += 1;
                if c & 1 == 0 {
                    out.extend_from_slice(self.phrase(c / 2));
                } else if c & 3 == 1 {
                    let Some(&x) = src.get(p) else { break };
                    p += 1;
                    out.extend_from_slice(self.phrase(128 + (c / 4) * 256 + x as usize));
                } else if c & 7 == 3 {
                    let n = c / 8 + 1;
                    let e = (p + n).min(src.len());
                    out.extend_from_slice(&src[p..e]);
                    p = e;
                } else if c & 0xf == 7 {
                    for _ in 0..c / 16 + 1 {
                        out.push(b' ');
                    }
                } else {
                    for _ in 0..c / 16 + 1 {
                        out.push(0);
                    }
                }
            }
        } else {
            while p < src.len() {
                let c = src[p];
                p += 1;
                if (1..0x10).contains(&c) {
                    let Some(&x) = src.get(p) else { break };
                    p += 1;
                    let n = (c as usize - 1) * 256 + x as usize;
                    out.extend_from_slice(self.phrase(n / 2));
                    if n & 1 != 0 {
                        out.push(b' ');
                    }
                } else {
                    out.push(c);
                }
            }
        }
    }
}

/// A TOPICLINK record.
pub struct Link {
    /// Position in the stream.
    pub at: usize,
    pub kind: u8,
    /// Its LinkData1 and LinkData2 (expanded) as ranges.
    pub data1: (usize, usize),
    pub data2: Vec<u8>,
    /// TOPICOFFSET of the record's start (3.1), or its TOPICPOS (3.0).
    pub offset: u32,
}

/// The decompressed topic stream.
pub struct TopicFile {
    pub buf: Vec<u8>,
    /// Stream offset of each block's data.
    starts: Vec<usize>,
    /// Bytes a TOPICPOS counts per block.
    dsize: usize,
    v30: bool,
    /// The first record's position.
    first: usize,
}

impl TopicFile {
    /// Blocks of `block` bytes (2 KB for 3.0, 4 KB for 3.1), each a 12
    /// byte header (TOPICPOS of the last record, of the first record
    /// starting in it, of the last topic header) and data, LZ77
    /// compressed to at most 16 KB when `compressed`.
    pub fn new(t: &[u8], v30: bool, compressed: bool) -> Self {
        let block = if v30 { 2048 } else { 4096 };
        let dsize = if v30 { block } else { 0x4000 };
        let mut buf = Vec::with_capacity(if compressed { t.len() * 2 } else { t.len() });
        let mut starts = Vec::new();
        let mut first = 0;
        let mut o = 0;
        while o + 12 <= t.len() {
            let e = (o + block).min(t.len());
            starts.push(buf.len());
            if o == 0 {
                first = u32_at(t, 4).unwrap_or(12) as usize;
            }
            if compressed {
                lz77(&t[o + 12..e], &mut buf, dsize - 12);
            } else {
                buf.extend_from_slice(&t[o + 12..e]);
            }
            o += block;
        }
        let mut f = TopicFile { buf, starts, dsize, v30, first: 0 };
        f.first = f.pos(first as u32).unwrap_or(0);
        f
    }

    /// Stream offset of a TOPICPOS (3.1: block * block's decompressed
    /// size + offset in it, counting its header).
    pub fn pos(&self, tp: u32) -> Option<usize> {
        let (b, o) = (tp as usize / self.dsize, tp as usize % self.dsize);
        let s = *self.starts.get(b)?;
        Some(s + o.checked_sub(12)?)
    }

    /// The TOPICPOS of a stream offset.
    pub fn topicpos(&self, at: usize) -> u32 {
        let b = self.block_of(at);
        (b * self.dsize + at - self.starts[b] + 12) as u32
    }

    fn block_of(&self, at: usize) -> usize {
        match self.starts.binary_search(&at) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// Walk the records in order. 3.1's next field is a TOPICPOS, 3.0's
    /// the distance to the next record's TOPICPOS (records do not cross
    /// 3.0's blocks: the rest of a block is padding). TOPICOFFSETs (3.1)
    /// are the block a record starts in times 0x8000 plus the characters
    /// before it in that block (the text lengths of LinkData1).
    pub fn links(&self, phrases: Option<&Phrases>, f: impl FnMut(&Link) -> bool) {
        self.links_from(self.first, phrases, f)
    }

    /// Walk the records from stream offset `at` (a record's start; the
    /// TOPICOFFSETs are only right from the first record).
    pub fn links_from(&self, mut at: usize, phrases: Option<&Phrases>, mut f: impl FnMut(&Link) -> bool) {
        let mut count = 0u32;
        let mut block = usize::MAX;
        let mut guard = 0usize;
        while at + 21 <= self.buf.len() {
            let b = &self.buf;
            let size = u32_at(b, at).unwrap_or(0) as usize;
            let len2 = u32_at(b, at + 4).unwrap_or(0) as usize;
            let next = u32_at(b, at + 12).unwrap_or(0);
            let len1 = u32_at(b, at + 16).unwrap_or(0) as usize;
            let kind = b[at + 20];
            if size < 21 || len1 < 21 || len1 > size || at + size > b.len() {
                break;
            }
            let bl = self.block_of(at);
            if bl != block {
                block = bl;
                count = 0;
            }
            let raw2 = &b[at + len1..at + size];
            let mut data2 = Vec::new();
            match phrases {
                Some(p) if len2 > raw2.len() => {
                    data2.reserve(len2);
                    p.expand(raw2, &mut data2);
                }
                _ => data2.extend_from_slice(raw2),
            }
            let offset = if self.v30 { self.topicpos(at) } else { (block as u32) << 15 | count };
            let link = Link { at, kind, data1: (at + 21, at + len1), data2, offset };
            if kind == TL_DISPLAY || kind == TL_TABLE {
                // The text length LinkData1 gives (pictures count one).
                let mut c = Cur::new(&b[at + 21..at + len1]);
                c.clong();
                count += c.cword().unwrap_or(0) as u32;
            }
            if !f(&link) {
                return;
            }
            let next = if self.v30 { self.topicpos(at).wrapping_add(next) } else { next };
            if next == 0xffff_ffff {
                return;
            }
            let Some(n) = self.pos(next) else { return };
            if n <= at {
                return;
            }
            at = n;
            guard += 1;
            if guard > 10_000_000 {
                return;
            }
        }
    }

    pub fn data1(&self, l: &Link) -> &[u8] {
        &self.buf[l.data1.0..l.data1.1]
    }
}

/// Topic header fields.
pub struct Header {
    /// 3.1: the topic number; 3.0: none.
    pub num: Option<u32>,
}

pub fn header(d1: &[u8], v30: bool) -> Header {
    if v30 {
        Header { num: None }
    } else {
        Header { num: u32_at(d1, 12) }
    }
}

/// A hotspot's action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hot<'a> {
    /// To a topic by number (3.0).
    Jump(u32),
    Popup(u32),
    /// To a context hash, maybe in another file or window.
    JumpHash { hash: u32, file: Option<&'a [u8]>, window: Option<&'a [u8]> },
    PopupHash { hash: u32, file: Option<&'a [u8]> },
    Macro(&'a [u8]),
}

/// Paragraph settings (units: as stored, about 1/144 inch... see the
/// crate notes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Para {
    pub above: i16,
    pub below: i16,
    pub lines: i16,
    pub left: i16,
    pub right: i16,
    pub first: i16,
    /// 0 left, 1 right, 2 centre.
    pub align: u8,
    pub tabs: Vec<(u16, u8)>,
    pub border: u8,
}

/// Events of a displayable text record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ev<'a> {
    /// A table starts: where its columns start (paragraph units).
    Table(Vec<i32>),
    /// A paragraph of a table cell in column n starts (its settings
    /// next); a cell may have several.
    Cell(i16),
    /// A paragraph's settings (start of the record or of a cell).
    Para(Para),
    Text(&'a [u8]),
    Font(u16),
    Break,
    ParaEnd,
    Tab,
    NbSpace,
    NbHyphen,
    /// A picture: 0 inline, 1 left, 2 right; `|bmN` number or the
    /// embedded picture's bytes.
    Pict(u8, Pic<'a>),
    HotStart(Hot<'a>, bool),
    HotEnd,
    /// End of the table.
    TableEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pic<'a> {
    Num(u16),
    Data(&'a [u8]),
}

/// Paragraph settings of a 3.1 record: u8, u8, u16 id, u16 bits, then
/// by bit 0x01 a compressed long, 0x02-0x40 compressed shorts (space
/// above, below, between lines, left, right and first line indents),
/// 0x100 a border (u8 flags, u16 width), 0x200 tab stops; 0x400 right,
/// 0x800 centred.
fn para(c: &mut Cur<'_>) -> Option<Para> {
    c.u8()?;
    c.u8()?;
    c.u16()?;
    let bits = c.u16()?;
    let mut p = Para::default();
    if bits & 0x0001 != 0 {
        c.clong()?;
    }
    if bits & 0x0002 != 0 {
        p.above = c.cshort()?;
    }
    if bits & 0x0004 != 0 {
        p.below = c.cshort()?;
    }
    if bits & 0x0008 != 0 {
        p.lines = c.cshort()?;
    }
    if bits & 0x0010 != 0 {
        p.left = c.cshort()?;
    }
    if bits & 0x0020 != 0 {
        p.right = c.cshort()?;
    }
    if bits & 0x0040 != 0 {
        p.first = c.cshort()?;
    }
    if bits & 0x0100 != 0 {
        p.border = c.u8()?;
        c.u16()?;
    }
    if bits & 0x0200 != 0 {
        let n = c.cshort()?;
        for _ in 0..n.max(0) {
            let t = c.cword()?;
            let ty = if t & 0x4000 != 0 { c.cword()? as u8 } else { 0 };
            p.tabs.push((t & 0x3fff, ty));
        }
    }
    p.align = if bits & 0x0400 != 0 {
        1
    } else if bits & 0x0800 != 0 {
        2
    } else {
        0
    };
    Some(p)
}

/// Decode a displayable text record (types 0x01, 0x20, 0x23): LinkData1
/// is a compressed long (the record's size), for 0x20 and 0x23 a
/// compressed word (its text length), for tables the columns (u8 count,
/// u8 type, for types 0 and 2 an i16 table width that the column
/// widths are 1/32767 of, then i16 width and gap per column) and then
/// per paragraph (tables: each first has i16 column (-1 ends the
/// table), i16, u8) the paragraph settings and formatting commands up
/// to 0xFF. LinkData2 is the text: its strings
/// end where the next command applies.
pub fn decode<'a>(kind: u8, d1: &'a [u8], d2: &'a [u8], f: &mut dyn FnMut(Ev<'a>)) -> Option<()> {
    let mut c = Cur::new(d1);
    c.clong()?;
    if kind != TL_DISPLAY30 {
        c.cword()?;
    }
    let table = kind == TL_TABLE;
    if table {
        let cols = c.u8()?;
        let ty = c.u8()?;
        // Types 0 and 2: widths in 1/32767 of the table's width.
        let scale = if ty == 0 || ty == 2 { c.i16()? as i32 } else { 32767 };
        let mut wg = Vec::new();
        for _ in 0..cols {
            let w = c.i16()? as i32 * scale / 32767;
            let g = c.i16()? as i32 * scale / 32767;
            wg.push((w, g));
        }
        // Where each column starts: the first cell ends half the second
        // gap past its width and gap, the others a width and gap on.
        let mut starts = vec![0];
        let mut x = 0;
        for (i, &(w, g)) in wg.iter().enumerate() {
            x += w + g + if i == 0 { wg.get(1).map_or(0, |n| n.1 / 2) } else { 0 };
            starts.push(x);
        }
        starts.truncate(cols as usize);
        f(Ev::Table(starts));
    }
    let mut t = 0usize; // position in the text
    let mut text = |f: &mut dyn FnMut(Ev<'a>)| {
        // The text up to the next NUL.
        let s = d2.get(t..).unwrap_or(&[]);
        let n = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        if n > 0 {
            f(Ev::Text(&s[..n]));
        }
        t += n + 1;
    };
    loop {
        if table {
            let col = c.i16()?;
            if col == -1 {
                f(Ev::TableEnd);
                break;
            }
            c.i16()?;
            c.u8()?;
            f(Ev::Cell(col));
        }
        f(Ev::Para(para(&mut c)?));
        text(f);
        loop {
            let cmd = c.u8()?;
            match cmd {
                0xff => break,
                0x20 => {
                    c.u32()?;
                }
                0x21 => {
                    c.u16()?;
                }
                0x80 => f(Ev::Font(c.u16()?)),
                0x81 => f(Ev::Break),
                0x82 => f(Ev::ParaEnd),
                0x83 => f(Ev::Tab),
                0x86..=0x88 => {
                    let ty = c.u8()?;
                    let size = c.clong()? as usize;
                    let _hot = if ty == 0x22 { c.cword()? } else { 0 };
                    let data = c.bytes(size)?;
                    let align = cmd - 0x86;
                    match u16_at(data, 0) {
                        Some(0) => f(Ev::Pict(align, Pic::Num(u16_at(data, 2)?))),
                        Some(1) => f(Ev::Pict(align, Pic::Data(&data[2..]))),
                        _ => {}
                    }
                }
                0x89 => f(Ev::HotEnd),
                0x8b => f(Ev::NbSpace),
                0x8c => f(Ev::NbHyphen),
                0xc8 | 0xcc => {
                    let n = c.u16()? as usize;
                    let m = c.bytes(n)?;
                    let m = &m[..m.iter().position(|&b| b == 0).unwrap_or(m.len())];
                    f(Ev::HotStart(Hot::Macro(m), cmd == 0xc8));
                }
                // 3.0: topic numbers.
                0xe0 => f(Ev::HotStart(Hot::Popup(c.u32()?), true)),
                0xe1 => f(Ev::HotStart(Hot::Jump(c.u32()?), true)),
                // 3.1: context hashes.
                0xe2 | 0xe6 => f(Ev::HotStart(Hot::PopupHash { hash: c.u32()?, file: None }, cmd == 0xe2)),
                0xe3 | 0xe7 => f(Ev::HotStart(Hot::JumpHash { hash: c.u32()?, file: None, window: None }, cmd == 0xe3)),
                0xea | 0xeb | 0xee | 0xef => {
                    let n = c.u16()? as usize;
                    let d = c.bytes(n)?;
                    let ty = *d.first()?;
                    let hash = u32_at(d, 1)?;
                    let rest = &d[5..];
                    let (mut file, mut window) = (None, None);
                    match ty {
                        1 => {}
                        4 => file = Some(crate::hfs::cstr(rest, 0).0),
                        6 => {
                            let (w, e) = crate::hfs::cstr(rest, 0);
                            window = Some(w);
                            file = Some(crate::hfs::cstr(rest, e).0);
                        }
                        _ => {}
                    }
                    let file = file.filter(|f| !f.is_empty());
                    let visible = cmd == 0xea || cmd == 0xeb;
                    if cmd & 1 == 0 {
                        f(Ev::HotStart(Hot::PopupHash { hash, file }, visible));
                    } else {
                        f(Ev::HotStart(Hot::JumpHash { hash, file, window }, visible));
                    }
                }
                _ => return None,
            }
            // Every command but the end has its string of text after it.
            text(f);
        }
        if !table {
            break;
        }
    }
    Some(())
}
