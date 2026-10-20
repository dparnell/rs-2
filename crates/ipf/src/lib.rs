//! OS/2 Information Presentation Facility files: `.INF` books (VIEW) and
//! `.HLP` help libraries (the help manager), as IBM's IPF compiler writes
//! them. No OS dependencies; HELPMGR.DLL and VIEW.EXE use it on real
//! files, and it is tested on the host against the files on the OS/2
//! media.
//!
//! The format was worked out from IBM's HELPMGR.DLL (its loaders
//! `IOLoad*`, the formatter `FormatLine`/`FormatCell` and the bitmap code,
//! named by the debug symbols on the media) and checked against every
//! INF/HLP file on the media. All numbers are little-endian; offsets are
//! from the start of the file.
//!
//! # Header (0x9B bytes)
//!
//! | Offset | Size | Contents |
//! |--------|------|----------|
//! | 0x00 | 3 | `HSP` |
//! | 0x03 | 1 | 0x01 book (.INF), 0x10 help library (.HLP) |
//! | 0x04 | 2 | header size (0x9B; not used) |
//! | 0x06 | 2 | version: major (at most 2), minor |
//! | 0x08 | 2 | number of TOC entries |
//! | 0x0A | 4 | offset of the TOC entries |
//! | 0x0E | 4 | their total size |
//! | 0x12 | 4 | offset of the TOC offset table (u32 per entry) |
//! | 0x16 | 2 | number of resource ids (`res=`) |
//! | 0x18 | 4 | resource id table: ids (sorted), then TOC indexes |
//! | 0x1C | 2 | number of panel names (`name=`) |
//! | 0x1E | 4 | name table: dictionary word numbers (sorted), then TOC indexes |
//! | 0x22 | 2 | number of index entries |
//! | 0x24 | 4 | offset of the index |
//! | 0x28 | 4 | its size |
//! | 0x2C | 10 | a second index (count, offset, size; unused) |
//! | 0x36 | 4 | full-text search table (bit 31: 16-bit record sizes) |
//! | 0x3A | 4 | its size |
//! | 0x3E | 2 | number of cells |
//! | 0x40 | 4 | cell offset table (u32 per cell) |
//! | 0x44 | 4 | size of the dictionary |
//! | 0x48 | 2 | number of dictionary words |
//! | 0x4A | 4 | offset of the dictionary |
//! | 0x4E | 4 | offset of the bitmaps |
//! | 0x52 | 1 | largest local dictionary (245) |
//! | 0x53 | 4 | offset of the country information |
//! | 0x57 | 4 | its size |
//! | 0x5B | 4 | offset of the extended header (0: none) |
//! | 0x5F | 12 | reserved |
//! | 0x6B | 48 | title |
//!
//! Extended header (0x40 bytes): font count and table (entries of 0x27
//! bytes: face name\[33\], point height, width, code page), external
//! database count, table and size (length-prefixed file names), global
//! name count and table, a strings section, the child page table
//! (`u8 len, u16 parent, u16 children[]` records), the number of global
//! index entries and the control data.
//!
//! The dictionary holds every word of the file, sorted, each `u8 len`
//! (counting itself) and the characters. A TOC entry is `u8 len, u8 flags
//! (level 0-15, 0x20 extended, 0x40 hidden, 0x80 has children), u8
//! cells`, an extended block when flagged (`u16 bits`, then: 0x01 origin,
//! 0x02 size (each `u8 units, u16, u16`), 0x08 style, 0x400 group, 0x40
//! control group (each u16)), the cell numbers (u16 each) and the title.
//! A cell is `u8 0, u32 local dictionary offset, u8 local words, u16 text
//! size`, the text, and the local dictionary (u16 dictionary word numbers):
//! text bytes below 0xFA are local word numbers; see [`text`].
//!
//! Index entries: `u8 text size, u8 flags (0x01 level 1, 0x02 level 2,
//! 0x40 global, 0x80 sort key first), u8 synonyms, u16 TOC index`, the
//! text and u32 synonym offsets.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod bitmap;
pub mod compile;
pub mod layout;
pub mod text;

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not an IPF file.
    BadMagic,
    /// A table or record reaches past the end of the file.
    Truncated,
    /// A value out of range (version, counts, indexes).
    Invalid(&'static str),
}

/// Little-endian reads that report truncation.
#[derive(Clone, Copy)]
pub(crate) struct Rd<'a>(pub &'a [u8]);

impl<'a> Rd<'a> {
    pub fn bytes(&self, o: usize, n: usize) -> Result<&'a [u8], Error> {
        self.0.get(o..o.checked_add(n).ok_or(Error::Truncated)?).ok_or(Error::Truncated)
    }
    pub fn u8(&self, o: usize) -> Result<u8, Error> {
        self.0.get(o).copied().ok_or(Error::Truncated)
    }
    pub fn u16(&self, o: usize) -> Result<u16, Error> {
        let b = self.bytes(o, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn u32(&self, o: usize) -> Result<u32, Error> {
        let b = self.bytes(o, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Header fields.
#[derive(Debug, Clone, Default)]
pub struct Header {
    /// 0x01 book, 0x10 help library.
    pub kind: u8,
    pub version: (u8, u8),
    pub toc_count: u16,
    pub toc_start: u32,
    pub toc_size: u32,
    pub toc_offsets: u32,
    pub res_count: u16,
    pub res_start: u32,
    pub name_count: u16,
    pub name_start: u32,
    pub index_count: u16,
    pub index_start: u32,
    pub index_size: u32,
    pub search_start: u32,
    pub search_size: u32,
    pub cell_count: u16,
    pub cell_offsets: u32,
    pub dict_size: u32,
    pub dict_count: u16,
    pub dict_start: u32,
    pub image_start: u32,
    pub nls_start: u32,
    pub nls_size: u32,
    pub ext_start: u32,
    pub title: Vec<u8>,
}

pub const KIND_BOOK: u8 = 0x01;
pub const KIND_HELP: u8 = 0x10;

/// Extended header fields.
#[derive(Debug, Clone, Default)]
pub struct ExtHeader {
    pub font_count: u16,
    pub font_start: u32,
    pub db_count: u16,
    pub db_start: u32,
    pub db_size: u32,
    pub gname_count: u16,
    pub gname_start: u32,
    pub child_start: u32,
    pub child_size: u32,
    pub ctrl_start: u32,
    pub ctrl_size: u32,
}

/// A font of the font table (`:font.` changes select them by number;
/// entry 0 is the document default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub face: Vec<u8>,
    /// Point size (0: default).
    pub height: u16,
    pub width: u16,
    pub codepage: u16,
}

/// Units of a TOC entry's window origin or size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WinCoord {
    pub units: u8,
    pub x: u16,
    pub y: u16,
}

/// A table of contents entry: one panel.
#[derive(Debug, Clone, Default)]
pub struct TocEntry {
    pub flags: u8,
    /// Extended flags (0 if none).
    pub ext: u16,
    pub origin: Option<WinCoord>,
    pub size: Option<WinCoord>,
    pub style: Option<u16>,
    pub group: Option<u16>,
    pub ctrl_group: Option<u16>,
    /// Cell numbers making up the panel's text.
    pub cells: Vec<u16>,
    pub title: Vec<u8>,
}

impl TocEntry {
    pub fn level(&self) -> u8 {
        self.flags & 0x0f
    }
    pub fn has_children(&self) -> bool {
        self.flags & 0x80 != 0
    }
    /// Not shown in the contents (IBM: "has children" wins over hidden).
    pub fn hidden(&self) -> bool {
        !self.has_children() && (self.flags & 0x40 != 0 || self.flags == 0)
    }
}

/// A cell: a run of text with its local dictionary.
#[derive(Debug, Clone, Copy)]
pub struct Cell<'a> {
    pub text: &'a [u8],
    /// Local word n is dictionary word `dict[n]` (u16 each).
    pub dict: &'a [u8],
}

impl Cell<'_> {
    pub fn word(&self, n: u8) -> Option<u16> {
        let i = n as usize * 2;
        let b = self.dict.get(i..i + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
}

/// An index entry.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub flags: u8,
    pub toc: u16,
    /// Sort key (the text if there is none).
    pub key: Vec<u8>,
    pub text: Vec<u8>,
}

impl IndexEntry {
    pub fn level(&self) -> u8 {
        if self.flags & 0x02 != 0 { 2 } else { 1 }
    }
    pub fn global(&self) -> bool {
        self.flags & 0x40 != 0
    }
}

/// A parsed IPF file borrowing its contents.
pub struct Doc<'a> {
    data: &'a [u8],
    pub header: Header,
    pub ext: Option<ExtHeader>,
    /// Dictionary words (offset, length) into the file.
    words: Vec<(u32, u8)>,
}

impl<'a> Doc<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let r = Rd(data);
        if data.len() < 0x9b || &data[..3] != b"HSP" {
            return Err(Error::BadMagic);
        }
        let mut h = Header {
            kind: r.u8(3)?,
            version: (r.u8(6)?, r.u8(7)?),
            toc_count: r.u16(0x08)?,
            toc_start: r.u32(0x0a)?,
            toc_size: r.u32(0x0e)?,
            toc_offsets: r.u32(0x12)?,
            res_count: r.u16(0x16)? & 0x7fff,
            res_start: r.u32(0x18)?,
            name_count: r.u16(0x1c)? & 0x7fff,
            name_start: r.u32(0x1e)?,
            index_count: r.u16(0x22)?,
            index_start: r.u32(0x24)?,
            index_size: r.u32(0x28)?,
            search_start: r.u32(0x36)?,
            search_size: r.u32(0x3a)?,
            cell_count: r.u16(0x3e)?,
            cell_offsets: r.u32(0x40)?,
            dict_size: r.u32(0x44)?,
            dict_count: r.u16(0x48)?,
            dict_start: r.u32(0x4a)?,
            image_start: r.u32(0x4e)?,
            nls_start: r.u32(0x53)?,
            nls_size: r.u32(0x57)?,
            ext_start: r.u32(0x5b)?,
            title: Vec::new(),
        };
        let t = &data[0x6b..0x9b];
        h.title = t[..t.iter().position(|&c| c == 0).unwrap_or(t.len())].to_vec();
        if h.version.0 > 2 {
            return Err(Error::Invalid("version"));
        }
        if h.toc_count == 0 || h.cell_count == 0 {
            return Err(Error::Invalid("empty"));
        }
        let ext = if h.ext_start != 0 {
            let e = h.ext_start as usize;
            Some(ExtHeader {
                font_count: r.u16(e)?,
                font_start: r.u32(e + 2)?,
                db_count: r.u16(e + 6)?,
                db_start: r.u32(e + 8)?,
                db_size: r.u32(e + 0x0c)?,
                gname_count: r.u16(e + 0x10)? & 0x7fff,
                gname_start: r.u32(e + 0x12)?,
                child_start: r.u32(e + 0x1c)?,
                child_size: r.u32(e + 0x20)?,
                ctrl_start: r.u32(e + 0x28)?,
                ctrl_size: r.u32(e + 0x2c)?,
            })
        } else {
            None
        };
        let mut words = Vec::with_capacity(h.dict_count as usize);
        let mut o = h.dict_start as usize;
        for _ in 0..h.dict_count {
            let n = r.u8(o)?;
            if n == 0 {
                return Err(Error::Invalid("dictionary"));
            }
            r.bytes(o + 1, n as usize - 1)?;
            words.push((o as u32 + 1, n - 1));
            o += n as usize;
        }
        Ok(Self { data, header: h, ext, words })
    }

    fn r(&self) -> Rd<'a> {
        Rd(self.data)
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn is_help(&self) -> bool {
        self.header.kind == KIND_HELP
    }

    pub fn title(&self) -> &[u8] {
        &self.header.title
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// Dictionary word `n`.
    pub fn word(&self, n: u16) -> &'a [u8] {
        match self.words.get(n as usize) {
            Some(&(o, l)) => &self.data[o as usize..o as usize + l as usize],
            None => b"",
        }
    }

    /// TOC entry `i`.
    pub fn toc(&self, i: u16) -> Result<TocEntry, Error> {
        if i >= self.header.toc_count {
            return Err(Error::Invalid("TOC index"));
        }
        let r = self.r();
        let o = r.u32(self.header.toc_offsets as usize + i as usize * 4)? as usize;
        let len = r.u8(o)? as usize;
        let end = o + len;
        r.bytes(o, len)?;
        let flags = r.u8(o + 1)?;
        let n = r.u8(o + 2)? as usize;
        let mut e = TocEntry { flags, ..Default::default() };
        let mut p = o + 3;
        if flags & 0x20 != 0 {
            e.ext = r.u16(p)?;
            p += 2;
            let coord = |p: usize| -> Result<WinCoord, Error> {
                Ok(WinCoord { units: r.u8(p)?, x: r.u16(p + 1)?, y: r.u16(p + 3)? })
            };
            if e.ext & 0x01 != 0 {
                e.origin = Some(coord(p)?);
                p += 5;
            }
            if e.ext & 0x02 != 0 {
                e.size = Some(coord(p)?);
                p += 5;
            }
            if e.ext & 0x08 != 0 {
                e.style = Some(r.u16(p)?);
                p += 2;
            }
            if e.ext & 0x400 != 0 {
                e.group = Some(r.u16(p)?);
                p += 2;
            }
            if e.ext & 0x40 != 0 {
                e.ctrl_group = Some(r.u16(p)?);
                p += 2;
            }
        }
        for k in 0..n {
            e.cells.push(r.u16(p + k * 2)?);
        }
        p += n * 2;
        if p > end {
            return Err(Error::Invalid("TOC entry"));
        }
        if e.level() != 0 {
            let t = &self.data[p..end];
            e.title = t[..t.iter().position(|&c| c == 0).unwrap_or(t.len())].to_vec();
        }
        Ok(e)
    }

    /// Cell `i`.
    pub fn cell(&self, i: u16) -> Result<Cell<'a>, Error> {
        if i >= self.header.cell_count {
            return Err(Error::Invalid("cell number"));
        }
        let r = self.r();
        let o = r.u32(self.header.cell_offsets as usize + i as usize * 4)? as usize;
        let dict = r.u32(o + 1)? as usize;
        let nlocal = r.u8(o + 5)? as usize;
        let size = r.u16(o + 6)? as usize;
        Ok(Cell { text: r.bytes(o + 8, size)?, dict: r.bytes(dict, nlocal * 2)? })
    }

    /// Look up `key` in a table of `n` sorted u16 keys followed by `n`
    /// u16 TOC indexes.
    fn table_lookup(&self, start: u32, n: u16, key: u16) -> Option<u16> {
        let r = self.r();
        let (mut lo, mut hi) = (0usize, n as usize);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let k = r.u16(start as usize + mid * 2).ok()?;
            match k.cmp(&key) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return r.u16(start as usize + (n as usize + mid) * 2).ok(),
            }
        }
        None
    }

    /// The panel with resource id `res` (HM_RESOURCEID).
    pub fn panel_by_res(&self, res: u16) -> Option<u16> {
        self.table_lookup(self.header.res_start, self.header.res_count, res)
    }

    /// The resource id of panel `toc`, if it has one.
    pub fn res_of_panel(&self, toc: u16) -> Option<u16> {
        let r = self.r();
        let (s, n) = (self.header.res_start as usize, self.header.res_count as usize);
        (0..n).find(|&i| r.u16(s + (n + i) * 2).ok() == Some(toc)).and_then(|i| r.u16(s + i * 2).ok())
    }

    /// The dictionary word equal to `w` ignoring case (IBM compares
    /// upper-cased names).
    pub fn find_word(&self, w: &[u8]) -> Option<u16> {
        let up = |c: u8| c.to_ascii_uppercase();
        let cmp = |a: &[u8]| a.iter().map(|&c| up(c)).cmp(w.iter().map(|&c| up(c)));
        let (mut lo, mut hi) = (0usize, self.words.len());
        // The dictionary is sorted case-insensitively by IBM's NLS
        // collation: binary search, then a linear check nearby.
        while lo < hi {
            let mid = (lo + hi) / 2;
            match cmp(self.word(mid as u16)) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(mid as u16),
            }
        }
        (0..self.words.len()).find(|&i| cmp(self.word(i as u16)).is_eq()).map(|i| i as u16)
    }

    /// The panel named `name` (HM_PANELNAME): the name table, then the
    /// global names.
    pub fn panel_by_name(&self, name: &[u8]) -> Option<u16> {
        let w = self.find_word(name)?;
        self.table_lookup(self.header.name_start, self.header.name_count, w).or_else(|| {
            let e = self.ext.as_ref()?;
            self.table_lookup(e.gname_start, e.gname_count, w)
        })
    }

    /// The font table (empty without an extended header).
    pub fn fonts(&self) -> Vec<Font> {
        let Some(e) = &self.ext else { return Vec::new() };
        let r = self.r();
        (0..e.font_count as usize)
            .filter_map(|i| {
                let o = e.font_start as usize + i * 0x27;
                let face = r.bytes(o, 33).ok()?;
                Some(Font {
                    face: face[..face.iter().position(|&c| c == 0).unwrap_or(33)].to_vec(),
                    height: r.u16(o + 0x21).ok()?,
                    width: r.u16(o + 0x23).ok()?,
                    codepage: r.u16(o + 0x25).ok()?,
                })
            })
            .collect()
    }

    /// External databases (`:link database=`), as stored.
    pub fn databases(&self) -> Vec<Vec<u8>> {
        let Some(e) = &self.ext else { return Vec::new() };
        let r = self.r();
        let mut v = Vec::new();
        let (mut o, end) = (e.db_start as usize, e.db_start as usize + e.db_size as usize);
        while v.len() < e.db_count as usize && o < end {
            let Ok(n) = r.u8(o) else { break };
            if n == 0 {
                break;
            }
            if let Ok(b) = r.bytes(o + 1, n as usize - 1) {
                v.push(b.to_vec());
            }
            o += n as usize;
        }
        v
    }

    /// The child page table: (parent, children).
    pub fn child_pages(&self) -> Vec<(u16, Vec<u16>)> {
        let Some(e) = &self.ext else { return Vec::new() };
        let r = self.r();
        let mut v = Vec::new();
        let (mut o, end) = (e.child_start as usize, e.child_start as usize + e.child_size as usize);
        while o + 3 <= end {
            let Ok(n) = r.u8(o) else { break };
            if n < 3 {
                break;
            }
            let parent = r.u16(o + 1).unwrap_or(0);
            let kids = (0..(n as usize - 3) / 2).filter_map(|k| r.u16(o + 3 + k * 2).ok()).collect();
            v.push((parent, kids));
            o += n as usize;
        }
        v
    }

    /// The index, in file order (sorted by the compiler).
    pub fn index(&self) -> Vec<IndexEntry> {
        let r = self.r();
        let mut v = Vec::with_capacity(self.header.index_count as usize);
        let mut o = self.header.index_start as usize;
        let end = o + self.header.index_size as usize;
        while o < end && v.len() < self.header.index_count as usize {
            let (Ok(n), Ok(flags), Ok(syn), Ok(toc)) = (r.u8(o), r.u8(o + 1), r.u8(o + 2), r.u16(o + 3)) else {
                break;
            };
            let Ok(raw) = r.bytes(o + 5, n as usize) else { break };
            let (key, text) = if flags & 0x80 != 0 && !raw.is_empty() {
                let k = (raw[0] as usize).min(raw.len() - 1);
                (raw[1..1 + k].to_vec(), raw[1 + k..].to_vec())
            } else {
                (raw.to_vec(), raw.to_vec())
            };
            v.push(IndexEntry { flags, toc, key, text });
            o += 5 + n as usize + 4 * syn as usize;
        }
        v
    }

    /// Country and code page of the text (from the country information).
    pub fn country(&self) -> (u16, u16) {
        let r = self.r();
        let o = self.header.nls_start as usize;
        if self.header.nls_size < 10 || o == 0 {
            return (1, 437);
        }
        (r.u16(o + 6).unwrap_or(1), r.u16(o + 8).unwrap_or(437))
    }

    /// The panels containing dictionary word `w`, from the full-text
    /// search table (None: not known).
    pub fn panels_with_word(&self, w: u16) -> Option<Vec<u16>> {
        let r = self.r();
        let wide = self.header.search_start & 0x8000_0000 != 0;
        let start = (self.header.search_start & 0x7fff_ffff) as usize;
        if start == 0 {
            return None;
        }
        let end = start + self.header.search_size as usize;
        let mut o = start;
        for _ in 0..w {
            let n = if wide { r.u16(o).ok()? as usize } else { r.u8(o).ok()? as usize };
            if n == 0 {
                return None;
            }
            o += n;
            if o >= end {
                return None;
            }
        }
        let (n, hl) = if wide { (r.u16(o).ok()? as usize, 2) } else { (r.u8(o).ok()? as usize, 1) };
        let ty = r.u8(o + hl).ok()?;
        let data = r.bytes(o + hl + 1, n.checked_sub(hl + 1)?).ok()?;
        let nt = self.header.toc_count;
        let list = |d: &[u8]| -> Vec<u16> { d.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() };
        let bits = |d: &[u8], base: usize| -> Vec<u16> {
            (0..d.len() * 8).filter(|i| d[i >> 3] & (0x80 >> (i & 7)) != 0).map(|i| (base + i) as u16).collect()
        };
        Some(match ty {
            0 => Vec::new(),
            1 => (0..nt).collect(),
            2 => {
                let mut out = Vec::new();
                let mut packed = alloc::vec![0u8; data.len()];
                packed.copy_from_slice(data);
                let mut buf = alloc::vec![0u8; (nt as usize).div_ceil(8) + 8];
                let n = bitmap::rle(&packed, &mut buf);
                out.extend(bits(&buf[..n], 0));
                out
            }
            3 => list(data),
            4 => {
                let ex = list(data);
                (0..nt).filter(|t| !ex.contains(t)).collect()
            }
            5 => bits(data, 0),
            6 => bits(data.get(2..)?, u16::from_le_bytes([data[0], data[1]]) as usize * 8),
            _ => return None,
        }
        .into_iter()
        .filter(|&t| t < nt)
        .collect())
    }
}

/// Text in the file's code page as a `String` of Latin-1 characters (one
/// char per byte, as rs2 keeps OS/2 text).
pub fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

#[cfg(test)]
mod tests;
