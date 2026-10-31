//! The physical palette of a 256-colour display: the system's default
//! palettes (as IBM's PMMERGE defines them), palette realization, and the
//! nearest physical colour of an RGB value (what a pel drawn on such a
//! display shows).
//!
//! PM keeps the system's colours, the "default palette", at both ends of
//! the hardware palette: the first half of its entries at 0, the second
//! half at the top. It starts with 256 entries (all of the hardware
//! palette); when a program realizes a palette whose colours do not fit,
//! the default palette shrinks (256, 128, 64, then 16 entries) and the
//! entries in between go to the program's palette.

use alloc::vec;
use alloc::vec::Vec;

use crate::color::{distance, Rgb};

/// A cube of `rs` x `gs` x `bs` levels (red slowest), with `grays`
/// inserted at entry `at`.
fn cube(rs: &[u8], gs: &[u8], bs: &[u8], grays: &[u8], at: usize) -> Vec<Rgb> {
    let mut v = Vec::with_capacity(rs.len() * gs.len() * bs.len() + grays.len());
    for &r in rs {
        for &g in gs {
            for &b in bs {
                v.push((r as u32) << 16 | (g as u32) << 8 | b as u32);
            }
        }
    }
    let g: Vec<Rgb> = grays.iter().map(|&x| (x as u32) * 0x010101).collect();
    v.splice(at..at, g);
    v
}

/// The default palette of `n` entries (256, 128, 64 or 16): PMMERGE's
/// ulDefaultPalette256 etc.
pub fn default_palette(n: usize) -> Vec<Rgb> {
    match n {
        256 => {
            // 7 reds (one level shared with 32 greys) x 8 greens x 4 blues.
            let mut v = cube(
                &[0x00, 0x2b, 0x55, 0x80, 0xaa, 0xd5, 0xff],
                &[0x00, 0x24, 0x49, 0x6d, 0x92, 0xb6, 0xdb, 0xff],
                &[0x00, 0x55, 0xaa, 0xff],
                &[
                    0x08, 0x0f, 0x17, 0x1f, 0x27, 0x2e, 0x36, 0x3e, 0x46, 0x4d, 0x55, 0x5d, 0x64, 0x6c, 0x74, 0x7c, 0x83,
                    0x8b, 0x93, 0x9b, 0xa2, 0xaa, 0xb2, 0xb9, 0xc1, 0xcc, 0xd1, 0xd8, 0xe0, 0xe8, 0xf0, 0xf7,
                ],
                112,
            );
            // Red 0x80's greens 0x92.. come after the greys; the greys take
            // the place of the rest of red 0x80.
            v.truncate(256);
            // CLR_DARKPINK.
            v[98] = 0x800080;
            v
        }
        128 => {
            let mut v = cube(
                &[0x00, 0x40, 0x80, 0xc0, 0xff],
                &[0x00, 0x33, 0x66, 0x99, 0xcc, 0xff],
                &[0x00, 0x55, 0xaa, 0xff],
                &[0x1c, 0x38, 0x54, 0x70, 0x8c, 0xa8, 0xc4, 0xe0],
                60,
            );
            v.truncate(128);
            v
        }
        64 => {
            let mut v = cube(&[0x00, 0x55, 0xaa, 0xff], &[0x00, 0x40, 0x80, 0xc0, 0xff], &[0x00, 0x80, 0xff], &[0x55, 0x80, 0xa6, 0xcc], 30);
            v.truncate(64);
            v
        }
        _ => vec![
            0x000000, 0x000080, 0x008000, 0x008080, 0x800000, 0x800080, 0x808000, 0x808080, 0xcccccc, 0x0000ff, 0x00ff00,
            0x00ffff, 0xff0000, 0xff00ff, 0xffff00, 0xffffff,
        ],
    }
}

/// LUT key: 6 bits per channel.
fn key(c: Rgb) -> usize {
    ((c >> 18) & 0x3f) as usize * 4096 + ((c >> 10) & 0x3f) as usize * 64 + ((c >> 2) & 0x3f) as usize
}

const UNKNOWN: u16 = u16::MAX;

/// PC_* flags of a palette entry (its top byte).
pub const PC_RESERVED: u32 = 0x01;
pub const PC_EXPLICIT: u32 = 0x02;
pub const PC_NOCOLLAPSE: u32 = 0x04;

/// The hardware palette of a 256-colour display.
pub struct Physical {
    pub rgb: [Rgb; 256],
    /// Entries of the default palette now (256, 128, 64 or 16).
    pub default_size: usize,
    /// Entries a realized palette took (pels may map to them too).
    pub taken: [bool; 256],
    /// Nearest entry by 6-6-6 RGB, filled in on first use.
    lut: Vec<u16>,
}

impl Default for Physical {
    fn default() -> Self {
        Self::new()
    }
}

impl Physical {
    pub fn new() -> Physical {
        let mut p = Physical { rgb: [0; 256], default_size: 256, taken: [false; 256], lut: vec![UNKNOWN; 1 << 18] };
        p.set_default(256);
        p
    }

    /// Lay out the default palette of `n` entries (realized entries are
    /// given up).
    pub fn set_default(&mut self, n: usize) {
        let d = default_palette(n);
        let h = n / 2;
        for (i, &c) in d.iter().enumerate() {
            let slot = if i < h { i } else { 256 - n + i };
            self.rgb[slot] = c;
        }
        self.default_size = n;
        self.taken = [false; 256];
        self.lut.fill(UNKNOWN);
    }

    /// Whether pels may be drawn with entry `i`: the default palette's and
    /// realized ones.
    pub fn in_use(&self, i: usize) -> bool {
        let h = self.default_size / 2;
        i < h || i >= 256 - h || self.taken[i]
    }

    /// The physical index nearest to `c`.
    pub fn index(&mut self, c: Rgb) -> u8 {
        let k = key(c);
        if self.lut[k] == UNKNOWN {
            let i = (0..256).filter(|&i| self.in_use(i)).min_by_key(|&i| distance(self.rgb[i], c)).unwrap_or(0);
            self.lut[k] = i as u16;
        }
        self.lut[k] as u8
    }

    /// The colour a pel of `c` shows.
    pub fn map(&mut self, c: Rgb) -> Rgb {
        let i = self.index(c) as usize;
        self.rgb[i]
    }

    /// Realize a palette (PC_* flags in the top byte of each entry) for
    /// the foreground program: each colour gets an entry of the same
    /// colour, else a free one between the halves of the default palette
    /// (shrinking it when none is left), else the nearest. Returns the
    /// physical index of each entry and how many hardware entries
    /// changed.
    pub fn realize(&mut self, pal: &[u32]) -> (Vec<u8>, u32) {
        // Start again from the largest default palette that leaves room.
        let full = default_palette(256);
        let need = pal
            .iter()
            .filter(|&&e| e >> 24 & PC_EXPLICIT == 0 && (e >> 24 & (PC_RESERVED | PC_NOCOLLAPSE) != 0 || !full.contains(&(e & 0xff_ffff))))
            .count();
        let old = self.rgb;
        let mut n = 256;
        while n > 16 && 256 - n < need.min(236) {
            n = match n {
                256 => 128,
                128 => 64,
                _ => 16,
            };
        }
        self.set_default(n);
        let (lo, hi) = if n == 16 { (10, 245) } else { (n / 2, 255 - n / 2) };
        let mut next = lo;
        let mut map = Vec::with_capacity(pal.len());
        for &e in pal {
            let (flags, c) = (e >> 24, e & 0xff_ffff);
            if flags & PC_EXPLICIT != 0 {
                map.push(e as u8);
                continue;
            }
            let exact = if flags & (PC_RESERVED | PC_NOCOLLAPSE) == 0 {
                (0..256).find(|&i| self.in_use(i) && self.rgb[i] == c)
            } else {
                None
            };
            let i = match exact {
                Some(i) => i,
                None if next <= hi => {
                    next += 1;
                    self.rgb[next - 1] = c;
                    self.taken[next - 1] = true;
                    next - 1
                }
                None => (0..256).filter(|&i| self.in_use(i)).min_by_key(|&i| distance(self.rgb[i], c)).unwrap_or(0),
            };
            map.push(i as u8);
        }
        self.lut.fill(UNKNOWN);
        let changed = (0..256).filter(|&i| self.rgb[i] != old[i]).count() as u32;
        (map, changed)
    }

    /// Give the realized entries back to the full default palette.
    /// Returns how many hardware entries changed.
    pub fn restore(&mut self) -> u32 {
        let old = self.rgb;
        self.set_default(256);
        (0..256).filter(|&i| self.rgb[i] != old[i]).count() as u32
    }

    /// The colours pels may now have (the entries in use), by index.
    pub fn entries(&self) -> impl Iterator<Item = (usize, Rgb)> + '_ {
        (0..256).filter(|&i| self.in_use(i)).map(|i| (i, self.rgb[i]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let p = default_palette(256);
        assert_eq!(p.len(), 256);
        assert_eq!((p[0], p[3], p[36], p[98], p[111], p[112], p[143], p[144], p[159], p[255]), (0, 0xff, 0x2b2400, 0x800080, 0x806dff, 0x080808, 0xf7f7f7, 0x809200, 0x80ffff, 0xffffff));
        let p = default_palette(128);
        assert_eq!((p.len(), p[59], p[60], p[67], p[68], p[127]), (128, 0x8066ff, 0x1c1c1c, 0xe0e0e0, 0x809900, 0xffffff));
        let p = default_palette(64);
        assert_eq!((p.len(), p[29], p[30], p[33], p[34], p[63]), (64, 0x55ffff, 0x555555, 0xcccccc, 0xaa0000, 0xffffff));
        assert_eq!(default_palette(16)[8], 0xcccccc);
    }

    #[test]
    fn nearest_and_realize() {
        let mut p = Physical::new();
        assert_eq!(p.map(0xffffff), 0xffffff);
        assert_eq!(p.map(0x808080), 0x838383);
        assert_eq!(p.map(0xcccccc), 0xcccccc);
        // 20 new colours: the default palette goes down to 128 entries.
        let pal: Vec<u32> = (0..20).map(|i| 0x123400 + i).collect();
        let (map, changed) = p.realize(&pal);
        assert_eq!(p.default_size, 128);
        assert_eq!(map[0], 64);
        assert_eq!(p.rgb[64], 0x123400);
        assert!(changed >= 20);
        assert_eq!(p.map(0x123405), 0x123405);
        // White is in the default palette: no new entry.
        let (map, _) = p.realize(&[0xffffff, 0x010203]);
        assert_eq!(map[0], 255);
        assert_eq!(map[1], 64);
        assert_eq!(p.default_size, 128);
        assert!(p.restore() > 0);
        assert_eq!(p.default_size, 256);
    }
}
