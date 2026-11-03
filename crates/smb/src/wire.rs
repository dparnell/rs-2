//! Little-endian reading and writing, UTF-16 and code page conversion.

use alloc::string::String;
use alloc::vec::Vec;

/// A growable little-endian message buffer.
#[derive(Default, Clone)]
pub struct Buf(pub Vec<u8>);

impl Buf {
    pub fn new() -> Buf {
        Buf(Vec::new())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
    pub fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.resize(self.0.len() + n, 0);
        self
    }
    /// Pad with zeros to a multiple of `n`.
    pub fn align(&mut self, n: usize) -> &mut Self {
        while self.0.len() % n != 0 {
            self.0.push(0);
        }
        self
    }
    pub fn put_u16(&mut self, at: usize, v: u16) {
        self.0[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn put_u32(&mut self, at: usize, v: u32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
}

pub fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}

pub fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn u64_at(b: &[u8], at: usize) -> u64 {
    u32_at(b, at) as u64 | (u32_at(b, at + 4) as u64) << 32
}

/// `len` bytes at `off`, or an empty slice if out of range.
pub fn slice(b: &[u8], off: usize, len: usize) -> &[u8] {
    b.get(off..off.saturating_add(len)).unwrap_or(&[])
}

/// Code page 850 (OS/2's default multilingual code page): the Unicode
/// characters of bytes 0x80-0xFF.
const CP850: [u16; 128] = [
    0x00C7, 0x00FC, 0x00E9, 0x00E2, 0x00E4, 0x00E0, 0x00E5, 0x00E7, 0x00EA, 0x00EB, 0x00E8, 0x00EF, 0x00EE, 0x00EC, 0x00C4, 0x00C5, 0x00C9, 0x00E6,
    0x00C6, 0x00F4, 0x00F6, 0x00F2, 0x00FB, 0x00F9, 0x00FF, 0x00D6, 0x00DC, 0x00F8, 0x00A3, 0x00D8, 0x00D7, 0x0192, 0x00E1, 0x00ED, 0x00F3, 0x00FA,
    0x00F1, 0x00D1, 0x00AA, 0x00BA, 0x00BF, 0x00AE, 0x00AC, 0x00BD, 0x00BC, 0x00A1, 0x00AB, 0x00BB, 0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x00C1,
    0x00C2, 0x00C0, 0x00A9, 0x2563, 0x2551, 0x2557, 0x255D, 0x00A2, 0x00A5, 0x2510, 0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x00E3, 0x00C3,
    0x255A, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256C, 0x00A4, 0x00F0, 0x00D0, 0x00CA, 0x00CB, 0x00C8, 0x0131, 0x00CD, 0x00CE, 0x00CF, 0x2518,
    0x250C, 0x2588, 0x2584, 0x00A6, 0x00CC, 0x2580, 0x00D3, 0x00DF, 0x00D4, 0x00D2, 0x00F5, 0x00D5, 0x00B5, 0x00FE, 0x00DE, 0x00DA, 0x00DB, 0x00D9,
    0x00FD, 0x00DD, 0x00AF, 0x00B4, 0x00AD, 0x00B1, 0x2017, 0x00BE, 0x00B6, 0x00A7, 0x00F7, 0x00B8, 0x00B0, 0x00A8, 0x00B7, 0x00B9, 0x00B3, 0x00B2,
    0x25A0, 0x00A0,
];

/// One OS/2 byte (code page 850) as a UTF-16 unit.
pub fn cp_to_unicode(b: u8) -> u16 {
    if b < 0x80 { b as u16 } else { CP850[b as usize - 0x80] }
}

/// A UTF-16 unit as a code page 850 byte ('_' when it has none).
pub fn unicode_to_cp(u: u16) -> u8 {
    if u < 0x80 {
        return u as u8;
    }
    CP850.iter().position(|&c| c == u).map_or(b'_', |i| 0x80 + i as u8)
}

/// rs2 strings hold one OS/2 character per `char` (U+0000-U+00FF): to
/// UTF-16LE through the code page.
pub fn utf16(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() * 2);
    for c in s.chars() {
        let u = if (c as u32) < 0x100 { cp_to_unicode(c as u32 as u8) } else { c as u32 as u16 };
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}

/// Plain UTF-16LE of ASCII/Unicode text (NTLM names, no code page).
pub fn utf16_plain(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}

/// UTF-16LE to an rs2 string (one OS/2 character per char).
pub fn from_utf16(b: &[u8]) -> String {
    let mut s = String::new();
    for c in b.chunks_exact(2) {
        let u = u16::from_le_bytes([c[0], c[1]]);
        if u == 0 {
            break;
        }
        s.push(unicode_to_cp(u) as char);
    }
    s
}

/// An 8-bit (OEM) string to bytes (rs2 strings: one byte per char).
pub fn oem(s: &str) -> Vec<u8> {
    s.chars().map(|c| if (c as u32) < 0x100 { c as u32 as u8 } else { b'_' }).collect()
}

/// Bytes up to a NUL as an rs2 string.
pub fn from_oem(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}

/// Upper-case for names (ASCII and code page 850 letters).
pub fn upper(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_uppercase()).collect()
}
