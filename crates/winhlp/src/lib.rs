//! Windows 3.0 and 3.1 help files (`.HLP`, WinHelp) read and converted
//! into OS/2 IPF help libraries, so rs2's help manager (HELPMGR.DLL, which
//! reads IPF through the `ipf` crate) and VIEW.EXE can show them, and
//! WIN16's WinHelp can look up contexts, map numbers and keywords. No OS
//! dependencies; tested on the host against help files of Windows 3.x
//! programs.
//!
//! The format as implemented here (all numbers little-endian):
//!
//! # File system
//!
//! A 16-byte header: magic 0x00035F3F, the offset of the directory, of
//! the first free block (-1), the file size. Every internal file starts
//! with a 9-byte header (u32 reserved space, u32 used space, u8 flags)
//! and its data. The directory is a B+tree of (zero-terminated name, u32
//! file offset); names start with `|` (`|SYSTEM`) except the pictures
//! (`bm0`, `bm1`, ...).
//!
//! A B+tree: a 38-byte header (u16 0x293B, u16 flags, u16 page size,
//! char structure\[16\], i16 0, i16 page splits, i16 root page, i16 -1,
//! i16 pages, i16 levels, i32 entries), then the pages. Index pages are
//! `u16 free, u16 entries, i16 first child` and (key, u16 child) entries;
//! leaf pages `u16 free, u16 entries, i16 previous, i16 next` and the
//! entries. The leaves are walked from the leftmost child down.
//!
//! # `|SYSTEM`
//!
//! u16 0x036C, u16 minor (15: Windows 3.0, 21: 3.1), u16 major, u32
//! date, u16 flags (3.1: 4 for LZ77-compressed topics; 8, WinHelp 4's 2
//! KB blocks, is refused). 3.0 has the title after it; 3.1 records `u16
//! type, u16 size, data`: 1 title, 2 copyright, 3 the contents topic
//! (TOPICOFFSET), 4 a startup macro, 6 a secondary window (u16 flags,
//! type\[10\], name\[9\], caption\[51\], x, y, width, height,
//! maximize, colours).
//!
//! # `|FONT`
//!
//! u16 face names, u16 descriptors, u16 offset of the names, u16 offset
//! of the descriptors; names 20 bytes each; descriptors `u8 attributes
//! (bold, italic, underline, strikeout, double underline, small caps),
//! u8 half points, u8 family, u16 face, u8 fg[3], u8 bg[3]` (1, 1, 0:
//! automatic colour).
//!
//! # `|TOPIC`
//!
//! Blocks of 4 KB (3.0: 2 KB), each a 12-byte header (TOPICPOS of the
//! last record, of the first record starting in it, of the last topic
//! header) and data, LZ77-compressed to at most 16 KB when the flags say
//! so. Data runs on from block to block (3.0: records do not cross
//! blocks, the rest of a block is padding). A TOPICPOS is the block
//! number × 16 KB (3.0: × 2 KB) plus the offset in the block counting
//! its header; a TOPICOFFSET (what 3.1's `|CONTEXT`, `|KWDATA`,
//! `|CTXOMAP`, `|TTLBTREE` and jumps use) is the block a record starts
//! in × 0x8000 plus the characters of the text records before it in that
//! block (their LinkData1 text lengths; a picture counts one). 3.0 uses
//! TOPICPOS there, and topic numbers in jumps (`|TOMAP`: the TOPICPOS of
//! each topic number; the first topic is number 16).
//!
//! LZ77: a flag byte per 8 items, bit 0 first; a set bit is a `u16` (12
//! bits distance - 1, 4 bits length - 3) back reference, a clear one a
//! literal.
//!
//! A record (TOPICLINK): i32 size, i32 LinkData2 size expanded, prev,
//! next (3.1 TOPICPOS; 3.0 the distance to the next), i32 size of the
//! header and LinkData1, u8 type (2 topic header, 0x20 text, 0x23
//! table, 0x01 3.0 text), LinkData1, LinkData2. LinkData2 shorter than
//! its size is phrase-compressed: `|Phrases` (u16 count, u16 0x100, 3.1:
//! u32 expanded size, u16 offsets, the phrases, LZ77 in 3.1); bytes 1-15
//! with the next byte are phrase n/2 (and a space if n is odd). Hall
//! compression (`|PhrIndex` with the phrase lengths as a bit stream,
//! `|PhrImage`) codes even bytes as phrases 0-127, `xx01` with the next
//! byte the others, `x011` literal runs, `x0111` spaces, `x1111` NULs.
//!
//! A topic header's LinkData1 is (3.1) i32 size, browse back and
//! forward, i32 topic number, non-scrolling and scrolling region and
//! next topic positions; its LinkData2 the title and the topic's macros,
//! zero-terminated. A text record's LinkData1 holds the paragraph
//! settings and the formatting commands, its LinkData2 the text whose
//! zero-terminated strings come between the commands: see
//! [`topic::decode`] (fonts 0x80, line break 0x81, paragraph end 0x82,
//! tab 0x83, pictures 0x86-0x88, hotspot end 0x89, non-breaking space
//! and hyphen 0x8B/0x8C, macros 0xC8/0xCC, jumps and popups 0xE0-0xE7,
//! to context hashes, maybe in other files or windows, 0xEA-0xEF, end
//! 0xFF).
//!
//! # Lookups
//!
//! `|CONTEXT`: B+tree of (context hash, TOPICOFFSET); the hash of a
//! context string is `hash * 43 + value(c)` over its characters, see
//! [`help::hash`]. `|CTXOMAP`: u16 count, (u32 map number, u32
//! TOPICOFFSET). `|KWBTREE`: (keyword, u16 count, u32 offset into
//! `|KWDATA`, which holds the TOPICOFFSETs). `|TTLBTREE`: (TOPICOFFSET,
//! title).
//!
//! # Pictures
//!
//! `bmN` files and pictures embedded in text: u16 `lP` (`lp` in 3.0),
//! u16 count, u32 offsets; each picture u8 type (5 device dependent
//! bitmap, 6 device independent, 8 metafile), u8 packing (0 none, 1 run
//! lengths, 2 LZ77, 3 both), compressed header fields, the palette, the
//! bits and a hotspot table (see [`pict`]). Metafiles are not converted.
//!
//! # Conversion
//!
//! Every topic becomes a panel with `res=` its ordinal + 1; the contents
//! topic is the first panel, the topics it jumps to (and theirs, while
//! fewer than [`Options::max_contents`]) its children in the table of
//! contents, all others hidden. Fonts become IPF fonts (Helv, Tms Rmn,
//! Courier at the help manager's sizes), with bold/italic/underline and
//! colours; indents, tab stops and tables become margins; jumps links,
//! popups footnote links; K keywords index entries; bitmaps artwork
//! with their hotspots as link areas. See [`convert()`].

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod convert;
pub mod cp;
pub mod help;
pub mod hfs;
pub mod pict;
pub mod topic;

#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec::Vec;

pub const MAGIC: u32 = 0x0003_5f3f;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Not a WinHelp file.
    BadMagic,
    Truncated,
    /// A structure out of range, or a format not handled (WinHelp 4).
    Invalid(&'static str),
    /// The IPF compiler refused the generated source (`source`: the
    /// source when [`Options::keep_source`]).
    Compile { line: usize, msg: String, source: Vec<u8> },
}

/// Whether `data` is a WinHelp file.
pub fn is_winhelp(data: &[u8]) -> bool {
    hfs::u32_at(data, 0) == Some(MAGIC)
}

/// Conversion options.
#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// The help file's own name: jumps and macros naming it stay in the
    /// file (those to other files are left as plain text).
    pub name: Option<&'a [u8]>,
    /// The most panels the table of contents shows (the contents topic
    /// and the topics it jumps to always).
    pub max_contents: usize,
    /// Keep the IPF source in [`Converted::source`].
    pub keep_source: bool,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { name: None, max_contents: 400, keep_source: false }
    }
}

/// A converted help file.
#[derive(Debug, Clone)]
pub struct Converted {
    /// The IPF help library.
    pub ipf: Vec<u8>,
    /// The help file's title (code page 1252).
    pub title: Vec<u8>,
    /// The panel of the contents topic.
    pub contents_res: u16,
    /// Context hashes and their panels (`res`), sorted by hash.
    pub contexts: Vec<(u32, u16)>,
    /// Map numbers (`[MAP]`, `|CTXOMAP`) and their panels, sorted.
    pub map: Vec<(u32, u16)>,
    /// K keywords (code page 1252) and their panels, sorted ignoring
    /// case.
    pub keywords: Vec<(Vec<u8>, Vec<u16>)>,
    /// Limits that were hit and what was left out.
    pub warnings: Vec<String>,
    /// The IPF source, when [`Options::keep_source`].
    pub source: Vec<u8>,
}

impl Converted {
    /// The panel of a context string (JumpId's, `HELP_KEY` with a
    /// context, HELP_CONTEXT strings).
    pub fn context(&self, name: &[u8]) -> Option<u16> {
        self.context_hash(help::hash(name))
    }

    pub fn context_hash(&self, h: u32) -> Option<u16> {
        let i = self.contexts.binary_search_by_key(&h, |c| c.0).ok()?;
        Some(self.contexts[i].1)
    }

    /// The panel of a map number (`HELP_CONTEXT`).
    pub fn map_number(&self, n: u32) -> Option<u16> {
        let i = self.map.binary_search_by_key(&n, |c| c.0).ok()?;
        Some(self.map[i].1)
    }

    /// The panels of a keyword (`HELP_KEY`), ignoring case.
    pub fn keyword(&self, k: &[u8]) -> Option<&[u16]> {
        let key = k.to_ascii_uppercase();
        let i = self.keywords.binary_search_by(|e| e.0.to_ascii_uppercase().cmp(&key)).ok()?;
        Some(&self.keywords[i].1)
    }

    /// The keywords starting with `prefix`, ignoring case
    /// (`HELP_PARTIALKEY`).
    pub fn keywords_with_prefix(&self, prefix: &[u8]) -> &[(Vec<u8>, Vec<u16>)] {
        let p = prefix.to_ascii_uppercase();
        let up = |e: &(Vec<u8>, Vec<u16>)| e.0.to_ascii_uppercase();
        let s = self.keywords.partition_point(|e| up(e) < p);
        let n = self.keywords[s..].iter().take_while(|e| up(e).starts_with(&p)).count();
        &self.keywords[s..s + n]
    }
}

/// Convert a WinHelp file into an IPF help library.
pub fn convert(hlp: &[u8]) -> Result<Converted, Error> {
    convert::convert(hlp, &Options::default())
}

/// [`convert()`] with options.
pub fn convert_with(hlp: &[u8], opts: &Options<'_>) -> Result<Converted, Error> {
    convert::convert(hlp, opts)
}
