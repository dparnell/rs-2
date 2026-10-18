//! Bitmaps: memory pixel buffers, the OS/2 1.x and 2.x bitmap header and
//! file formats (1/4/8/24 bpp, RLE4/RLE8), and bit-block transfers with
//! ternary raster operations.

use alloc::vec;
use alloc::vec::Vec;

use crate::color::{self, Rgb};
use crate::rect::Rect;
use crate::surface::Surface;

/// A memory bitmap. Pixels are always kept as 0x00RRGGBB (top row first,
/// like the framebuffer); `bitcount` is the format the application asked
/// for, used when bits are queried and for monochrome conversions.
pub struct Bitmap {
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bitcount: u16,
    pub pixels: Vec<u32>,
}

impl Bitmap {
    pub fn new(width: i32, height: i32, planes: u16, bitcount: u16) -> Bitmap {
        let (w, h) = (width.max(1), height.max(1));
        Bitmap { width: w, height: h, planes: planes.max(1), bitcount, pixels: vec![0; (w * h) as usize] }
    }

    pub fn surface(&mut self) -> Surface {
        unsafe { Surface::new(self.pixels.as_mut_ptr(), self.width, self.height, self.width, false) }
    }

    pub fn is_mono(&self) -> bool {
        self.bitcount == 1 && self.planes == 1
    }
}

fn u16le(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]) as u32)
}

fn u32le(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub const BCA_UNCOMP: u32 = 0;
pub const BCA_RLE8: u32 = 1;
pub const BCA_RLE4: u32 = 2;

/// A parsed BITMAPINFOHEADER / BITMAPINFOHEADER2 (and colour table).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub cbfix: u32,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bitcount: u16,
    pub compression: u32,
    pub colors: Vec<Rgb>,
}

impl Info {
    /// Parse the header at the start of `b`; `table` also reads the colour
    /// table that follows it.
    pub fn parse(b: &[u8], table: bool) -> Option<Info> {
        let cbfix = u32le(b, 0);
        let mut i = if cbfix == 12 {
            Info { cbfix, width: u16le(b, 4) as i32, height: u16le(b, 6) as i32, planes: u16le(b, 8) as u16, bitcount: u16le(b, 10) as u16, ..Info::default() }
        } else if (16..=64).contains(&cbfix) {
            Info {
                cbfix,
                width: u32le(b, 4) as i32,
                height: u32le(b, 8) as i32,
                planes: u16le(b, 12) as u16,
                bitcount: u16le(b, 14) as u16,
                compression: if cbfix > 16 { u32le(b, 16) } else { 0 },
                ..Info::default()
            }
        } else {
            return None;
        };
        if table && i.bitcount <= 8 {
            let used = if cbfix >= 36 { u32le(b, 32) } else { 0 };
            let n = if used != 0 && used < 256 { used } else { 1 << i.bitcount } as usize;
            let es = if cbfix == 12 { 3 } else { 4 };
            for k in 0..n {
                let o = cbfix as usize + k * es;
                let Some(e) = b.get(o..o + 3) else { break };
                i.colors.push((e[2] as u32) << 16 | (e[1] as u32) << 8 | e[0] as u32);
            }
        }
        Some(i)
    }

    /// Bytes in one scan line (padded to 32 bits).
    pub fn stride(&self) -> usize {
        (self.width as usize * self.bitcount as usize * self.planes.max(1) as usize).div_ceil(32) * 4
    }

    /// Size of the header plus colour table.
    pub fn header_len(&self) -> usize {
        let es = if self.cbfix == 12 { 3 } else { 4 };
        self.cbfix as usize + if self.bitcount <= 8 { es << self.bitcount } else { 0 }
    }
}

/// Expand RLE8/RLE4 data into uncompressed scan lines.
fn decompress(info: &Info, data: &[u8]) -> Vec<u8> {
    let stride = info.stride();
    let mut out = vec![0u8; stride * info.height.max(0) as usize];
    let four = info.compression == BCA_RLE4;
    let (mut x, mut y) = (0usize, 0usize);
    let put = |out: &mut Vec<u8>, x: usize, y: usize, v: u8| {
        if y >= info.height as usize || x >= info.width as usize {
            return;
        }
        let row = &mut out[y * stride..(y + 1) * stride];
        if four {
            let sh = if x % 2 == 0 { 4 } else { 0 };
            row[x / 2] = row[x / 2] & !(0xf << sh) | (v & 0xf) << sh;
        } else {
            row[x] = v;
        }
    };
    let mut i = 0;
    while i + 1 < data.len() {
        let (n, v) = (data[i] as usize, data[i + 1]);
        i += 2;
        if n > 0 {
            for k in 0..n {
                let px = if four { if k % 2 == 0 { v >> 4 } else { v & 0xf } } else { v };
                put(&mut out, x, y, px);
                x += 1;
            }
            continue;
        }
        match v {
            0 => {
                x = 0;
                y += 1;
            }
            1 => break,
            2 => {
                x += *data.get(i).unwrap_or(&0) as usize;
                y += *data.get(i + 1).unwrap_or(&0) as usize;
                i += 2;
            }
            n => {
                let n = n as usize;
                let bytes = if four { n.div_ceil(2) } else { n };
                for k in 0..n {
                    let b = *data.get(i + if four { k / 2 } else { k }).unwrap_or(&0);
                    let px = if four { if k % 2 == 0 { b >> 4 } else { b & 0xf } } else { b };
                    put(&mut out, x, y, px);
                    x += 1;
                }
                i += bytes.div_ceil(2) * 2;
            }
        }
    }
    out
}

/// Read pixel `x` of an uncompressed scan line.
fn pixel(info: &Info, row: &[u8], x: usize) -> Rgb {
    let lookup = |i: usize| info.colors.get(i).copied().unwrap_or(if i == 0 { 0 } else { 0xffffff });
    match info.bitcount {
        1 => lookup(((row.get(x / 8).unwrap_or(&0) >> (7 - x % 8)) & 1) as usize),
        4 => lookup(((row.get(x / 2).unwrap_or(&0) >> if x % 2 == 0 { 4 } else { 0 }) & 0xf) as usize),
        8 => lookup(*row.get(x).unwrap_or(&0) as usize),
        24 => row.get(x * 3..x * 3 + 3).map_or(0, |p| (p[2] as u32) << 16 | (p[1] as u32) << 8 | p[0] as u32),
        32 => row.get(x * 4..x * 4 + 3).map_or(0, |p| (p[2] as u32) << 16 | (p[1] as u32) << 8 | p[0] as u32),
        _ => 0,
    }
}

/// Decode `scans` scan lines (bottom-up, starting at scan `start` of the
/// bitmap) from `data` into `bm`. Returns the number of scans set.
pub fn set_bits(bm: &mut Bitmap, info: &Info, start: i32, scans: i32, data: &[u8]) -> i32 {
    let expanded;
    let data = if info.compression == BCA_RLE8 || info.compression == BCA_RLE4 {
        expanded = decompress(&Info { height: scans, ..info.clone() }, data);
        &expanded[..]
    } else {
        data
    };
    let stride = info.stride();
    let mut n = 0;
    for s in 0..scans.max(0) {
        let y = start + s;
        if y < 0 || y >= bm.height {
            continue;
        }
        let Some(row) = data.get(s as usize * stride..(s as usize + 1) * stride) else { break };
        let base = ((bm.height - 1 - y) * bm.width) as usize;
        for x in 0..bm.width.min(info.width) as usize {
            bm.pixels[base + x] = pixel(info, row, x);
        }
        n += 1;
    }
    n
}

/// A colour table for querying bits in an indexed format.
pub fn standard_table(bitcount: u16) -> Vec<Rgb> {
    match bitcount {
        1 => vec![0x000000, 0xffffff],
        4 => vec![
            0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xcccccc, 0x808080, 0xff0000, 0x00ff00,
            0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
        ],
        8 => {
            // 6x6x6 colour cube, then 40 greys.
            let mut t = Vec::with_capacity(256);
            for r in 0..6u32 {
                for g in 0..6u32 {
                    for b in 0..6u32 {
                        t.push((r * 51) << 16 | (g * 51) << 8 | b * 51);
                    }
                }
            }
            for i in 0..40u32 {
                let v = i * 255 / 39;
                t.push(v << 16 | v << 8 | v);
            }
            t
        }
        _ => Vec::new(),
    }
}

/// Encode scan lines of `bm` into `out` in `info`'s format (using
/// `info.colors` for indexed formats). Returns the number of scans.
pub fn get_bits(bm: &Bitmap, info: &Info, start: i32, scans: i32, out: &mut [u8]) -> i32 {
    let stride = info.stride();
    let mut n = 0;
    for s in 0..scans.max(0) {
        let y = start + s;
        if y < 0 || y >= bm.height {
            break;
        }
        let Some(row) = out.get_mut(s as usize * stride..(s as usize + 1) * stride) else { break };
        row.fill(0);
        let base = ((bm.height - 1 - y) * bm.width) as usize;
        for x in 0..bm.width.min(info.width).max(0) as usize {
            let c = bm.pixels[base + x];
            match info.bitcount {
                1 | 4 | 8 => {
                    let i = color::nearest(&info.colors, c) as u8;
                    match info.bitcount {
                        1 => row[x / 8] |= (i & 1) << (7 - x % 8),
                        4 => row[x / 2] |= (i & 0xf) << if x % 2 == 0 { 4 } else { 0 },
                        _ => row[x] = i,
                    }
                }
                24 => row[x * 3..x * 3 + 3].copy_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8]),
                32 => row[x * 4..x * 4 + 4].copy_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8, 0]),
                _ => {}
            }
        }
        n += 1;
    }
    n
}

/// A bitmap file or RT_BITMAP resource ('BM', or the first image of a
/// 'BA' array): header info and the offset of the bits.
pub fn parse_file(b: &[u8]) -> Option<(Info, usize)> {
    let ty = u16le(b, 0);
    let (hdr, info_at) = match ty {
        0x4142 => (14, 28),   // 'BA' -> BITMAPFILEHEADER at 14
        0x4d42 => (0, 14),    // 'BM'
        _ => return None,
    };
    if ty == 0x4142 && u16le(b, hdr) != 0x4d42 {
        return None;
    }
    let info = Info::parse(b.get(info_at..)?, true)?;
    let off = u32le(b, hdr + 10) as usize;
    Some((info, off))
}

/// Load a bitmap file/resource into a new bitmap.
pub fn load(b: &[u8]) -> Option<Bitmap> {
    let (info, off) = parse_file(b)?;
    if info.width <= 0 || info.height <= 0 {
        return None;
    }
    let mut bm = Bitmap::new(info.width, info.height, 1, info.bitcount);
    set_bits(&mut bm, &info, 0, info.height, b.get(off..)?);
    Some(bm)
}

/// Apply a ternary raster operation (P = pattern, S = source,
/// D = destination) bitwise to 24-bit colours.
pub fn rop3(rop: u8, p: Rgb, s: Rgb, d: Rgb) -> Rgb {
    let mut r = 0u32;
    for i in 0..8 {
        if rop & (1 << i) != 0 {
            let pick = |bit: u32, v: Rgb| if i & bit != 0 { v } else { !v };
            r |= pick(4, p) & pick(2, s) & pick(1, d);
        }
    }
    r & 0xff_ffff
}

/// Whether a ROP uses the source / the pattern.
pub fn rop_uses_source(rop: u8) -> bool {
    (rop >> 2) & 0x33 != rop & 0x33
}

pub fn rop_uses_pattern(rop: u8) -> bool {
    (rop >> 4) & 0x0f != rop & 0x0f
}

/// Source pixels for a blit: `w`x`h` samples of `src` (device rect
/// `sr`), scaled nearest-neighbour; `None` outside the source.
pub fn sample(src: &Surface, sr: Rect, w: i32, h: i32) -> Vec<Option<Rgb>> {
    let mut v = Vec::with_capacity((w.max(0) * h.max(0)) as usize);
    for j in 0..h {
        let sy = sr.bottom + (j as i64 * sr.height() as i64 / h.max(1) as i64) as i32;
        for i in 0..w {
            let sx = sr.left + (i as i64 * sr.width() as i64 / w.max(1) as i64) as i32;
            v.push(src.get(sx, sy));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp_file(bitcount: u16, colors: &[Rgb], w: u16, h: u16, bits: &[u8]) -> Vec<u8> {
        let mut f = Vec::new();
        let ncol = if bitcount <= 8 { 1usize << bitcount } else { 0 };
        let off = 14 + 12 + ncol * 3;
        f.extend_from_slice(&0x4d42u16.to_le_bytes());
        f.extend_from_slice(&((off + bits.len()) as u32).to_le_bytes());
        f.extend_from_slice(&[0, 0, 0, 0]);
        f.extend_from_slice(&(off as u32).to_le_bytes());
        f.extend_from_slice(&12u32.to_le_bytes());
        f.extend_from_slice(&w.to_le_bytes());
        f.extend_from_slice(&h.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&bitcount.to_le_bytes());
        for i in 0..ncol {
            let c = colors.get(i).copied().unwrap_or(0);
            f.extend_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8]);
        }
        f.extend_from_slice(bits);
        f
    }

    #[test]
    fn load_1bpp_and_4bpp() {
        // 2x2, bottom row first: bottom = (1, 0), top = (0, 1).
        let f = bmp_file(1, &[0x000000, 0xff0000], 2, 2, &[0x80, 0, 0, 0, 0x40, 0, 0, 0]);
        let bm = load(&f).unwrap();
        // pixels are top row first.
        assert_eq!(bm.pixels, vec![0x000000, 0xff0000, 0xff0000, 0x000000]);
        let f = bmp_file(4, &[0, 0x00ff00, 0x0000ff], 3, 1, &[0x12, 0x00, 0, 0]);
        let bm = load(&f).unwrap();
        assert_eq!(bm.pixels, vec![0x00ff00, 0x0000ff, 0]);
    }

    #[test]
    fn bits_round_trip_24() {
        let mut bm = Bitmap::new(3, 2, 1, 24);
        let info = Info { cbfix: 16, width: 3, height: 2, planes: 1, bitcount: 24, ..Info::default() };
        let data: Vec<u8> = (0..info.stride() as u8 * 2).collect();
        assert_eq!(set_bits(&mut bm, &info, 0, 2, &data), 2);
        let mut out = vec![0u8; data.len()];
        assert_eq!(get_bits(&bm, &info, 0, 2, &mut out), 2);
        // Padding bytes come back as zero.
        let mut want = data.clone();
        want[9..12].fill(0);
        want[21..24].fill(0);
        assert_eq!(out, want);
    }

    #[test]
    fn rle8() {
        let info = Info { cbfix: 40, width: 4, height: 2, planes: 1, bitcount: 8, compression: BCA_RLE8, colors: vec![0, 0x111111, 0x222222, 0x333333] };
        // Row 0: 3 x index 1, then absolute run [2, 3, 1] would overflow; use: run 3 of 1, eol; row 1: absolute 3 pixels 2,3,2 (padded), eob.
        let data = [3, 1, 0, 0, 0, 3, 2, 3, 2, 0, 0, 1];
        let mut bm = Bitmap::new(4, 2, 1, 8);
        set_bits(&mut bm, &info, 0, 2, &data);
        assert_eq!(&bm.pixels[4..8], &[0x111111, 0x111111, 0x111111, 0]);
        assert_eq!(&bm.pixels[0..3], &[0x222222, 0x333333, 0x222222]);
    }

    /// The Toolkit's sample bitmaps, when the media is extracted.
    #[test]
    fn toolkit_bitmaps() {
        extern crate std;
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/boot/toolkit/os2tk45/samples/pm/");
        for f in ["bmpsamp/yosemite.bmp", "clipbrd/clipbrd.bmp"] {
            let Ok(data) = std::fs::read(std::format!("{dir}{f}")) else { continue };
            let (info, _) = parse_file(&data).unwrap();
            let bm = load(&data).unwrap();
            assert_eq!((bm.width, bm.height), (info.width, info.height), "{f}");
            // Not a blank image.
            let first = bm.pixels[0];
            assert!(bm.pixels.iter().any(|&p| p != first), "{f}");
        }
    }

    #[test]
    fn rops() {
        assert_eq!(rop3(0xcc, 0, 0x123456, 0xffffff), 0x123456);
        assert_eq!(rop3(0x88, 0, 0x00ff00, 0x0f0f0f), 0x000f00);
        assert_eq!(rop3(0x33, 0, 0x00ff00, 0), 0xff00ff);
        assert_eq!(rop3(0x00, 0x1, 0x2, 0x3), 0);
        assert_eq!(rop3(0xff, 0x1, 0x2, 0x3), 0xffffff);
        assert_eq!(rop3(0xf0, 0xabcdef, 0, 0), 0xabcdef);
        assert_eq!(rop3(0x55, 0, 0, 0x0000ff), 0xffff00);
        assert!(rop_uses_source(0xcc) && !rop_uses_source(0xf0) && !rop_uses_source(0x55));
        assert!(rop_uses_pattern(0xf0) && !rop_uses_pattern(0xcc));
    }
}
