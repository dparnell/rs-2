//! File names: validity, case mapping and collation.
//!
//! HPFS names are byte strings in the volume's code page. Directories are
//! sorted by the upper-cased name (code page case map), so lookups are
//! case-insensitive while the stored name keeps its case.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// Longest file name component.
pub const MAX_NAME: usize = 254;

/// Upper-case pairs shared by code pages 437 and 850.
const COMMON: &[(u8, u8)] = &[
    (0x81, 0x9a),
    (0x82, 0x90),
    (0x84, 0x8e),
    (0x86, 0x8f),
    (0x87, 0x80),
    (0x91, 0x92),
    (0x94, 0x99),
    (0xa4, 0xa5),
];

/// Additional pairs of code page 850.
const CP850: &[(u8, u8)] = &[
    (0xa0, 0xb5),
    (0x83, 0xb6),
    (0x85, 0xb7),
    (0xc6, 0xc7),
    (0x88, 0xd2),
    (0x89, 0xd3),
    (0x8a, 0xd4),
    (0xa1, 0xd6),
    (0x8c, 0xd7),
    (0x8b, 0xd8),
    (0x8d, 0xde),
    (0xa2, 0xe0),
    (0x93, 0xe2),
    (0x95, 0xe3),
    (0xe4, 0xe5),
    (0xa3, 0xe9),
    (0x96, 0xea),
    (0x97, 0xeb),
    (0xec, 0xed),
    (0x9b, 0x9d),
    (0xd0, 0xd1),
    (0xe7, 0xe8),
];

/// A code page's upper-case table.
#[derive(Clone)]
pub struct CaseMap {
    up: [u8; 256],
    /// The code page number recorded on the volume (0 if unknown).
    pub code_page: u16,
}

impl CaseMap {
    /// ASCII-only case mapping (bytes 0x80-0xFF map to themselves).
    pub fn ascii() -> Self {
        let mut up = [0u8; 256];
        for (i, u) in up.iter_mut().enumerate() {
            *u = (i as u8).to_ascii_uppercase();
        }
        CaseMap { up, code_page: 0 }
    }

    /// The built-in table for code page 437 or 850 (others: ASCII only).
    pub fn for_code_page(cp: u16) -> Self {
        let mut m = Self::ascii();
        m.code_page = cp;
        if cp == 437 || cp == 850 {
            for &(l, u) in COMMON {
                m.up[l as usize] = u;
            }
        }
        if cp == 850 {
            for &(l, u) in CP850 {
                m.up[l as usize] = u;
            }
        }
        m
    }

    /// A table from the volume: the upper case of bytes 0x80-0xFF.
    pub fn from_high(cp: u16, high: &[u8]) -> Self {
        let mut m = Self::ascii();
        m.code_page = cp;
        for (i, &u) in high.iter().take(128).enumerate() {
            m.up[0x80 + i] = u;
        }
        m
    }

    /// The upper case of bytes 0x80-0xFF (as stored on the volume).
    pub fn high(&self) -> [u8; 128] {
        let mut h = [0u8; 128];
        h.copy_from_slice(&self.up[0x80..]);
        h
    }

    pub fn upper(&self, b: u8) -> u8 {
        self.up[b as usize]
    }

    /// Directory collation: upper-cased bytes, then length.
    pub fn cmp(&self, a: &[u8], b: &[u8]) -> Ordering {
        for (&x, &y) in a.iter().zip(b) {
            match self.upper(x).cmp(&self.upper(y)) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        a.len().cmp(&b.len())
    }

    pub fn eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.cmp(a, b) == Ordering::Equal
    }
}

/// Is `n` a valid HPFS name component?
pub fn valid_name(n: &[u8]) -> bool {
    if n.is_empty() || n.len() > MAX_NAME || n == b"." || n == b".." {
        return false;
    }
    n.iter().all(|&c| c >= 0x20 && !b"\\/:*?\"<>|".contains(&c))
}

/// Does `n` also fit the FAT 8.3 rules? (Otherwise the DIRENT gets the
/// long-name attribute.)
pub fn is_83(n: &[u8]) -> bool {
    let (base, ext) = match n.iter().position(|&c| c == b'.') {
        Some(i) => (&n[..i], &n[i + 1..]),
        None => (n, &[][..]),
    };
    let ok = |s: &[u8]| s.iter().all(|&c| c > 0x20 && !b".\"/\\[]:;=,+*?<>|".contains(&c));
    !base.is_empty() && base.len() <= 8 && ext.len() <= 3 && ok(base) && ok(ext) && !(n.last() == Some(&b'.'))
}

/// Path text to name bytes: characters U+0000-U+00FF stand for the code
/// page bytes of the same value.
pub fn to_bytes(s: &str) -> Option<Vec<u8>> {
    s.chars().map(|c| u8::try_from(c as u32).ok()).collect()
}

/// Name bytes to text (the inverse of [`to_bytes`]).
pub fn to_string(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}
