//! RIFF (Resource Interchange File Format) chunks.
//!
//! A RIFF file is a tree of chunks: an 8-byte header (a four-character
//! code and a little-endian data size) followed by the data, padded to an
//! even length. `RIFF` and `LIST` chunks hold a form type (another
//! four-character code) and then sub-chunks.

use crate::le32;

/// A four-character code as MMPM/2 keeps it: the first character in the
/// low byte (`mmioFOURCC`).
pub const fn fourcc(c: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*c)
}

pub const RIFF: u32 = fourcc(b"RIFF");
pub const LIST: u32 = fourcc(b"LIST");
pub const WAVE: u32 = fourcc(b"WAVE");
pub const FMT: u32 = fourcc(b"fmt ");
pub const DATA: u32 = fourcc(b"data");

/// A four-character code from a string, as `mmioStringToFOURCC` makes it:
/// at most the first four characters (stopping at a NUL), padded with
/// blanks; `upper` converts ASCII letters to upper case.
pub fn fourcc_from_str(s: &[u8], upper: bool) -> u32 {
    let mut c = [b' '; 4];
    for (d, &b) in c.iter_mut().zip(s.iter().take_while(|&&b| b != 0)) {
        *d = if upper { b.to_ascii_uppercase() } else { b };
    }
    fourcc(&c)
}

/// The size a chunk's data takes in the file (data padded to even).
pub const fn padded(size: u32) -> u32 {
    size.saturating_add(size & 1)
}

/// A chunk header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub id: u32,
    /// The data size (without the padding byte).
    pub size: u32,
    /// The form type of a `RIFF` or `LIST` chunk (it starts the data).
    pub form: Option<u32>,
}

impl Header {
    /// Parse a header from the start of `d` (8 bytes, 12 for `RIFF` and
    /// `LIST`).
    pub fn parse(d: &[u8]) -> Option<Header> {
        let id = le32(d, 0)?;
        let size = le32(d, 4)?;
        let form = if id == RIFF || id == LIST { Some(le32(d, 8)?) } else { None };
        Some(Header { id, size, form })
    }

    /// Bytes of header before the sub-chunks (or data): 12 for forms, 8 else.
    pub fn len(&self) -> usize {
        if self.form.is_some() { 12 } else { 8 }
    }
}

/// A chunk found in a byte slice.
#[derive(Clone, Copy, Debug)]
pub struct Chunk<'a> {
    pub header: Header,
    /// Offset of the chunk's data (after the form type for forms) in the
    /// slice the chunks were walked in.
    pub offset: usize,
    /// The data (after the form type for forms), cut short when the file
    /// is shorter than the header says.
    pub data: &'a [u8],
}

impl Chunk<'_> {
    /// The sub-chunks of a `RIFF` or `LIST` chunk.
    pub fn children(&self) -> Chunks<'_> {
        Chunks { d: self.data, pos: 0, base: self.offset }
    }
}

/// Iterator over the chunks that follow one another in a slice.
pub struct Chunks<'a> {
    d: &'a [u8],
    pos: usize,
    base: usize,
}

/// The chunks of `d` (a whole RIFF file gives its one `RIFF` chunk).
pub fn chunks(d: &[u8]) -> Chunks<'_> {
    Chunks { d, pos: 0, base: 0 }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Chunk<'a>> {
        let header = Header::parse(self.d.get(self.pos..)?)?;
        let start = self.pos + header.len();
        let size = (header.size as usize).saturating_sub(header.len() - 8);
        let end = start.saturating_add(size).min(self.d.len());
        let data = self.d.get(start..end)?;
        self.pos = (self.pos + 8).saturating_add(padded(header.size) as usize);
        Some(Chunk { header, offset: self.base + start, data })
    }
}

/// The form (`RIFF` chunk) a file starts with: its form type and chunk.
pub fn form(d: &[u8]) -> Option<(u32, Chunk<'_>)> {
    let c = chunks(d).next()?;
    (c.header.id == RIFF).then(|| (c.header.form.unwrap(), c))
}
