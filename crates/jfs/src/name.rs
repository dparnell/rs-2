//! File names: code page text to and from UCS-2, validity, collation.
//!
//! JFS stores names in UCS-2. OS/2 programs use code page text, so the
//! crate's `&str` names carry one code page byte per character
//! (U+0000-U+00FF, as in the HPFS crate) and [`CodePage`] translates.
//! Directories of OS/2 volumes are sorted case-insensitively (each
//! character upper-cased by [`crate::upcase::upper`], then by length);
//! other volumes (Linux) compare characters exactly.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::upcase::upper;

/// Longest name component (characters), as OS/2's JFS allows.
pub const MAX_NAME: usize = 254;

static CP437: [u16; 128] = [
    0x00c7, 0x00fc, 0x00e9, 0x00e2, 0x00e4, 0x00e0, 0x00e5, 0x00e7,
    0x00ea, 0x00eb, 0x00e8, 0x00ef, 0x00ee, 0x00ec, 0x00c4, 0x00c5,
    0x00c9, 0x00e6, 0x00c6, 0x00f4, 0x00f6, 0x00f2, 0x00fb, 0x00f9,
    0x00ff, 0x00d6, 0x00dc, 0x00a2, 0x00a3, 0x00a5, 0x20a7, 0x0192,
    0x00e1, 0x00ed, 0x00f3, 0x00fa, 0x00f1, 0x00d1, 0x00aa, 0x00ba,
    0x00bf, 0x2310, 0x00ac, 0x00bd, 0x00bc, 0x00a1, 0x00ab, 0x00bb,
    0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x2561, 0x2562, 0x2556,
    0x2555, 0x2563, 0x2551, 0x2557, 0x255d, 0x255c, 0x255b, 0x2510,
    0x2514, 0x2534, 0x252c, 0x251c, 0x2500, 0x253c, 0x255e, 0x255f,
    0x255a, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256c, 0x2567,
    0x2568, 0x2564, 0x2565, 0x2559, 0x2558, 0x2552, 0x2553, 0x256b,
    0x256a, 0x2518, 0x250c, 0x2588, 0x2584, 0x258c, 0x2590, 0x2580,
    0x03b1, 0x00df, 0x0393, 0x03c0, 0x03a3, 0x03c3, 0x00b5, 0x03c4,
    0x03a6, 0x0398, 0x03a9, 0x03b4, 0x221e, 0x03c6, 0x03b5, 0x2229,
    0x2261, 0x00b1, 0x2265, 0x2264, 0x2320, 0x2321, 0x00f7, 0x2248,
    0x00b0, 0x2219, 0x00b7, 0x221a, 0x207f, 0x00b2, 0x25a0, 0x00a0,
];
static CP850: [u16; 128] = [
    0x00c7, 0x00fc, 0x00e9, 0x00e2, 0x00e4, 0x00e0, 0x00e5, 0x00e7,
    0x00ea, 0x00eb, 0x00e8, 0x00ef, 0x00ee, 0x00ec, 0x00c4, 0x00c5,
    0x00c9, 0x00e6, 0x00c6, 0x00f4, 0x00f6, 0x00f2, 0x00fb, 0x00f9,
    0x00ff, 0x00d6, 0x00dc, 0x00f8, 0x00a3, 0x00d8, 0x00d7, 0x0192,
    0x00e1, 0x00ed, 0x00f3, 0x00fa, 0x00f1, 0x00d1, 0x00aa, 0x00ba,
    0x00bf, 0x00ae, 0x00ac, 0x00bd, 0x00bc, 0x00a1, 0x00ab, 0x00bb,
    0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x00c1, 0x00c2, 0x00c0,
    0x00a9, 0x2563, 0x2551, 0x2557, 0x255d, 0x00a2, 0x00a5, 0x2510,
    0x2514, 0x2534, 0x252c, 0x251c, 0x2500, 0x253c, 0x00e3, 0x00c3,
    0x255a, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256c, 0x00a4,
    0x00f0, 0x00d0, 0x00ca, 0x00cb, 0x00c8, 0x0131, 0x00cd, 0x00ce,
    0x00cf, 0x2518, 0x250c, 0x2588, 0x2584, 0x00a6, 0x00cc, 0x2580,
    0x00d3, 0x00df, 0x00d4, 0x00d2, 0x00f5, 0x00d5, 0x00b5, 0x00fe,
    0x00de, 0x00da, 0x00db, 0x00d9, 0x00fd, 0x00dd, 0x00af, 0x00b4,
    0x00ad, 0x00b1, 0x2017, 0x00be, 0x00b6, 0x00a7, 0x00f7, 0x00b8,
    0x00b0, 0x00a8, 0x00b7, 0x00b9, 0x00b3, 0x00b2, 0x25a0, 0x00a0,
];

/// A single-byte code page: bytes 0x80-0xFF to Unicode (0x00-0x7F are
/// ASCII).
#[derive(Clone)]
pub struct CodePage {
    pub number: u16,
    high: [u16; 128],
}

impl CodePage {
    /// Code page 437 or 850 (anything else: 850).
    pub fn builtin(number: u16) -> Self {
        if number == 437 {
            CodePage { number, high: CP437 }
        } else {
            CodePage { number: 850, high: CP850 }
        }
    }

    /// From a full 256-entry code page table (only 0x80-0xFF is used).
    pub fn from_table(number: u16, table: &[u16; 256]) -> Self {
        let mut high = [0u16; 128];
        high.copy_from_slice(&table[128..]);
        CodePage { number, high }
    }

    pub fn to_ucs(&self, b: u8) -> u16 {
        if b < 0x80 { b as u16 } else { self.high[b as usize - 0x80] }
    }

    pub fn from_ucs(&self, c: u16) -> Option<u8> {
        if c < 0x80 {
            return Some(c as u8);
        }
        self.high.iter().position(|&h| h == c).map(|i| i as u8 + 0x80)
    }

    /// Name text (code page bytes as characters) to UCS-2.
    pub fn encode(&self, s: &str) -> Option<Vec<u16>> {
        s.chars().map(|c| u8::try_from(c as u32).ok().map(|b| self.to_ucs(b))).collect()
    }

    /// UCS-2 to name text; characters outside the code page become `_`.
    pub fn decode(&self, u: &[u16]) -> String {
        u.iter().map(|&c| self.from_ucs(c).unwrap_or(b'_') as char).collect()
    }
}

/// Is `n` a valid name component (UCS-2)?
pub fn valid_name(n: &[u16]) -> bool {
    let dot = '.' as u16;
    if n.is_empty() || n.len() > MAX_NAME || n == [dot] || n == [dot, dot] {
        return false;
    }
    n.iter().all(|&c| c >= 0x20 && !"\\/:*?\"<>|".encode_utf16().any(|x| x == c))
}

/// Does `n` fit the FAT 8.3 rules? (Otherwise OS/2 marks the inode with
/// the long-name attribute.)
pub fn is_83(n: &[u16]) -> bool {
    let dot = '.' as u16;
    let (base, ext) = match n.iter().position(|&c| c == dot) {
        Some(i) => (&n[..i], &n[i + 1..]),
        None => (n, &[][..]),
    };
    let ok = |s: &[u16]| s.iter().all(|&c| c > 0x20 && c < 0x100 && !".\"/\\[]:;=,+*?<>|".encode_utf16().any(|x| x == c));
    !base.is_empty() && base.len() <= 8 && ext.len() <= 3 && ok(base) && ok(ext) && n.last() != Some(&dot)
}

pub fn to_upper(n: &[u16]) -> Vec<u16> {
    n.iter().map(|&c| upper(c)).collect()
}

/// Directory order: case-insensitive (`ci`) or exact, then by length.
pub fn compare(a: &[u16], b: &[u16], ci: bool) -> Ordering {
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = if ci { (upper(x), upper(y)) } else { (x, y) };
        match x.cmp(&y) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    a.len().cmp(&b.len())
}

/// The shortest key that sorts after `left` and not after `right` (the
/// router key for a split between them); upper-cased when `ci`.
pub fn prefix_key(left: &[u16], right: &[u16], ci: bool) -> Vec<u16> {
    let (l, r) = if ci { (to_upper(left), to_upper(right)) } else { (left.to_vec(), right.to_vec()) };
    for i in 0..r.len() {
        if i >= l.len() || l[i] != r[i] {
            return r[..=i].to_vec();
        }
    }
    r
}
