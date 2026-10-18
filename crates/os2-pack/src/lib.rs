//! Reader for the packed files of the OS/2 installation media.
//!
//! The OS/2 installer ships most files compressed with IBM's "FTCOMP"
//! algorithm and expands them with `UNPACK`/`UNPACK2`. A packed file holds
//! one or more entries (multi-entry files are the `BUNDLE` archives); each
//! entry has a header with the target path, size, timestamp and attributes,
//! followed by the compressed data and optionally the compressed extended
//! attributes. See `docs/formats/ftcomp.md` for the format.
//!
//! The crate is `no_std` + `alloc` so the installer and host tools can share
//! it.

#![no_std]

extern crate alloc;

mod decode;
mod huffman;
mod tables;

use alloc::vec::Vec;

pub use decode::{BLOCK_MAGIC, Decompressor};

/// Signature at the start of every entry header.
pub const MAGIC: [u8; 4] = [0xa5, 0x96, 0xfd, 0xff];
/// Size of the fixed part of an entry header (the path follows).
pub const HEADER_SIZE: usize = 0x29;
/// The only algorithm name/version this crate (and UNPACK2) accepts.
pub const ALGORITHM: &[u8] = b"FTCOMP";
pub const ALGORITHM_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The data does not start with an entry header.
    BadMagic,
    /// A header or a stream ends early, or an offset points outside the file.
    Truncated,
    /// The entry uses an algorithm other than FTCOMP version 1.
    UnknownAlgorithm,
    /// The compressed data is inconsistent.
    Corrupt,
    /// The data decompressed to a different size than the header records.
    SizeMismatch { expected: u32, actual: usize },
}

/// One packed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    /// Offset of the header within the packed file.
    pub offset: usize,
    /// Target path as stored, without the terminating NUL (e.g.
    /// `\OS2\VIEW.EXE`). Usually relative to the target drive's root.
    pub path: &'a [u8],
    /// Last-write date and time in FAT/OS/2 packed form.
    pub date: u16,
    pub time: u16,
    /// File attributes (FILE_ARCHIVED etc.).
    pub attributes: u32,
    /// Uncompressed file size.
    pub size: u32,
    /// Uncompressed size of the extended attributes (a FEA2LIST whose
    /// `cbList` this equals; 4 means none).
    pub ea_size: u32,
    /// Algorithm name (NUL-terminated in the header) and version.
    pub algorithm: &'a [u8],
    pub version: u16,
    /// Compressed file data.
    pub data: &'a [u8],
    /// Compressed extended attributes, if stored.
    pub ea: Option<&'a [u8]>,
}

fn u16_at(d: &[u8], o: usize) -> Result<u16, Error> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Error::Truncated)
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, Error> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Error::Truncated)
}

/// Whether `data` looks like a packed file.
pub fn is_packed(data: &[u8]) -> bool {
    data.starts_with(&MAGIC)
}

/// Parse all entries of a packed file.
pub fn entries(file: &[u8]) -> Result<Vec<Entry<'_>>, Error> {
    if !is_packed(file) {
        return Err(Error::BadMagic);
    }
    // The file ends with a 32-bit entry count; streams stop before it.
    let end = file.len().checked_sub(4).ok_or(Error::Truncated)?;
    let mut list = Vec::new();
    let mut off = 0usize;
    loop {
        let h = file.get(off..).ok_or(Error::Truncated)?;
        if !h.starts_with(&MAGIC) {
            return Err(Error::BadMagic);
        }
        let date = u16_at(h, 0x04)?;
        let time = u16_at(h, 0x06)?;
        let attributes = u32_at(h, 0x08)?;
        let ea_off = u32_at(h, 0x0c)? as usize;
        let size = u32_at(h, 0x10)?;
        let next = u32_at(h, 0x14)? as usize;
        let name = h.get(0x18..0x21).ok_or(Error::Truncated)?;
        let algorithm = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
        let version = u16_at(h, 0x21)?;
        let ea_size = u32_at(h, 0x23)?;
        let path_len = u16_at(h, 0x27)? as usize;
        let path = h
            .get(HEADER_SIZE..HEADER_SIZE + path_len)
            .ok_or(Error::Truncated)?;
        let path = &path[..path.iter().position(|&c| c == 0).unwrap_or(path.len())];

        let data_start = off + HEADER_SIZE + path_len;
        let entry_end = if next != 0 { next } else { end };
        if next != 0 && next <= off {
            return Err(Error::Corrupt);
        }
        let (data_end, ea) = if ea_off != 0 {
            let ea = file.get(ea_off..entry_end).ok_or(Error::Truncated)?;
            (ea_off, Some(ea))
        } else {
            (entry_end, None)
        };
        let data = file.get(data_start..data_end).ok_or(Error::Truncated)?;
        list.push(Entry {
            offset: off,
            path,
            date,
            time,
            attributes,
            size,
            ea_size,
            algorithm,
            version,
            data,
            ea,
        });
        if next == 0 {
            break;
        }
        off = next;
    }
    Ok(list)
}

impl Entry<'_> {
    fn check_algorithm(&self) -> Result<(), Error> {
        if !self.algorithm.eq_ignore_ascii_case(ALGORITHM) || self.version != ALGORITHM_VERSION {
            return Err(Error::UnknownAlgorithm);
        }
        Ok(())
    }

    /// Decompress the file data, checking it against the recorded size.
    pub fn decompress(&self, dec: &mut Decompressor) -> Result<Vec<u8>, Error> {
        self.check_algorithm()?;
        let out = dec.stream(self.data, self.size as usize)?;
        if out.len() != self.size as usize {
            return Err(Error::SizeMismatch {
                expected: self.size,
                actual: out.len(),
            });
        }
        Ok(out)
    }

    /// Decompress the extended attributes (a FEA2LIST), if stored.
    pub fn decompress_ea(&self, dec: &mut Decompressor) -> Result<Option<Vec<u8>>, Error> {
        self.check_algorithm()?;
        let Some(ea) = self.ea else { return Ok(None) };
        let out = dec.stream(ea, self.ea_size as usize)?;
        if out.len() != self.ea_size as usize {
            return Err(Error::SizeMismatch {
                expected: self.ea_size,
                actual: out.len(),
            });
        }
        Ok(Some(out))
    }

    /// The path split into components (separators `\` and `/`), skipping a
    /// drive prefix and empty components.
    pub fn components(&self) -> impl Iterator<Item = &[u8]> {
        let p = match self.path {
            [_, b':', rest @ ..] => rest,
            p => p,
        };
        p.split(|&c| c == b'\\' || c == b'/')
            .filter(|c| !c.is_empty())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::vec;

    /// A synthetic single-entry file with a stored (uncompressed) block.
    fn sample() -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&MAGIC);
        f.extend_from_slice(&0x2b50u16.to_le_bytes());
        f.extend_from_slice(&0x6ee4u16.to_le_bytes());
        f.extend_from_slice(&0x20u32.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes()); // no EAs
        f.extend_from_slice(&3u32.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes()); // last entry
        f.extend_from_slice(b"FTCOMP\0\0\0");
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&4u32.to_le_bytes());
        let path = b"\\OS2\\A.TXT\0";
        f.extend_from_slice(&(path.len() as u16).to_le_bytes());
        f.extend_from_slice(path);
        f.extend_from_slice(&[0x80, 0x60, 0, 0]);
        f.extend_from_slice(&BLOCK_MAGIC);
        f.extend_from_slice(&[0xff, 0xff, 6, 0, 4, 0, 1, b'a', b'b', b'c']);
        f.extend_from_slice(&1u32.to_le_bytes());
        f
    }

    #[test]
    fn parse_and_extract() {
        let f = sample();
        let e = entries(&f).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].path, b"\\OS2\\A.TXT");
        assert_eq!(e[0].attributes, 0x20);
        assert_eq!((e[0].date, e[0].time), (0x2b50, 0x6ee4));
        let parts: Vec<&[u8]> = e[0].components().collect();
        assert_eq!(parts, vec![&b"OS2"[..], &b"A.TXT"[..]]);
        let mut d = Decompressor::new();
        assert_eq!(e[0].decompress(&mut d).unwrap(), b"abc");
        assert_eq!(e[0].decompress_ea(&mut d).unwrap(), None);
    }

    #[test]
    fn size_is_checked() {
        let mut f = sample();
        f[0x10] = 4;
        let e = entries(&f).unwrap();
        assert_eq!(
            e[0].decompress(&mut Decompressor::new()),
            Err(Error::SizeMismatch {
                expected: 4,
                actual: 3
            })
        );
    }

    #[test]
    fn rejects_other_data() {
        assert_eq!(entries(b"MZ\0\0\0\0\0\0"), Err(Error::BadMagic));
        assert_eq!(entries(&MAGIC), Err(Error::Truncated));
    }
}
