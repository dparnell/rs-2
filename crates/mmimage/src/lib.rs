//! Still image formats for rs2's multimedia I/O procedures (the OS/2,
//! JPEG, GIF, PCX, TIFF and Targa IOProcs), with no OS dependencies
//! (host-tested):
//!
//! - [`bmp`]: OS/2 1.x and 2.0 bitmaps, Windows DIBs, bitmap arrays;
//! - [`jpeg`]: baseline and progressive JPEG decoding, baseline encoding;
//! - [`gif`]: GIF87a/89a with LZW;
//! - [`pcx`]: ZSoft PCX;
//! - [`tiff`]: TIFF in both byte orders (none, PackBits, LZW);
//! - [`targa`]: Truevision Targa, plain and run-length encoded.
//!
//! Every decoder produces an [`Image`]: a device-independent bitmap with
//! rows top-down and padded to 4 bytes, either palettized (1, 4 or 8
//! bits) or 24-bit B,G,R. Decoders never panic on bad input; they return
//! an [`Error`].

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

pub mod bmp;
pub mod gif;
pub mod jpeg;
pub mod pcx;
pub mod targa;
pub mod tiff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The data is not in the format asked for (or in no known format).
    NotThisFormat,
    /// The data ends before the image does.
    Truncated,
    /// A valid file using a feature that is not implemented.
    Unsupported,
    /// Inconsistent or invalid data.
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Os13Bmp,
    Os20Bmp,
    WinBmp,
    Jpeg,
    Gif,
    Pcx,
    TiffIntel { compressed: bool },
    TiffMotorola { compressed: bool },
    Targa { compressed: bool },
}

/// A decoded image: rows top-down, each [`Image::row_bytes`] long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// 1, 4, 8 (with a palette) or 24 (B,G,R bytes per pixel).
    pub bits: u8,
    /// R,G,B entries (empty for 24-bit).
    pub palette: Vec<[u8; 3]>,
    pub data: Vec<u8>,
}

impl Image {
    /// A black image (all indexes 0), with an empty palette.
    pub fn new(width: u32, height: u32, bits: u8) -> Image {
        let mut i = Image { width, height, bits, palette: Vec::new(), data: Vec::new() };
        i.data = vec![0; i.row_bytes() * height as usize];
        i
    }

    /// Bytes per row, padded to 4 bytes as DIB rows are.
    pub fn row_bytes(&self) -> usize {
        (self.width as usize * self.bits as usize).div_ceil(32) * 4
    }

    /// Row `y` (0 = top); empty if out of range.
    pub fn row(&self, y: u32) -> &[u8] {
        let rb = self.row_bytes();
        let at = y as usize * rb;
        self.data.get(at..at + rb).unwrap_or(&[])
    }

    pub fn row_mut(&mut self, y: u32) -> &mut [u8] {
        let rb = self.row_bytes();
        let at = y as usize * rb;
        self.data.get_mut(at..at + rb).unwrap_or(&mut [])
    }

    /// The palette index of pixel (x, y) of a 1/4/8-bit image.
    pub fn index(&self, x: u32, y: u32) -> u8 {
        let r = self.row(y);
        let x = x as usize;
        match self.bits {
            1 => r.get(x / 8).map_or(0, |b| (b >> (7 - x % 8)) & 1),
            4 => r.get(x / 2).map_or(0, |b| if x % 2 == 0 { b >> 4 } else { b & 15 }),
            8 => r.get(x).copied().unwrap_or(0),
            _ => 0,
        }
    }

    /// Sets the palette index of pixel (x, y) of a 1/4/8-bit image.
    pub fn set_index(&mut self, x: u32, y: u32, v: u8) {
        let bits = self.bits;
        let x = x as usize;
        let r = self.row_mut(y);
        match bits {
            1 => if let Some(b) = r.get_mut(x / 8) { let m = 0x80 >> (x % 8); if v & 1 != 0 { *b |= m } else { *b &= !m } },
            4 => if let Some(b) = r.get_mut(x / 2) { if x % 2 == 0 { *b = (*b & 0x0F) | (v << 4) } else { *b = (*b & 0xF0) | (v & 15) } },
            8 => if let Some(b) = r.get_mut(x) { *b = v },
            _ => {}
        }
    }

    /// The colour of pixel (x, y) as R,G,B (black outside the image or
    /// for indexes past the palette).
    pub fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        if x >= self.width || y >= self.height { return [0; 3]; }
        if self.bits == 24 {
            let r = self.row(y);
            let at = x as usize * 3;
            return match r.get(at..at + 3) { Some(p) => [p[2], p[1], p[0]], None => [0; 3] };
        }
        self.palette.get(self.index(x, y) as usize).copied().unwrap_or([0; 3])
    }

    /// Sets pixel (x, y) of a 24-bit image.
    pub fn set_rgb(&mut self, x: u32, y: u32, c: [u8; 3]) {
        let at = x as usize * 3;
        if let Some(p) = self.row_mut(y).get_mut(at..at + 3) {
            p[0] = c[2];
            p[1] = c[1];
            p[2] = c[0];
        }
    }

    /// A 24-bit copy.
    pub fn to_24(&self) -> Image {
        if self.bits == 24 { return self.clone(); }
        let mut o = Image::new(self.width, self.height, 24);
        for y in 0..self.height {
            for x in 0..self.width {
                o.set_rgb(x, y, self.rgb(x, y));
            }
        }
        o
    }

    /// An 8-bit copy of a 1- or 4-bit image (same palette).
    pub fn to_8(&self) -> Image {
        if self.bits == 8 || self.bits == 24 { return self.clone(); }
        let mut o = Image::new(self.width, self.height, 8);
        o.palette = self.palette.clone();
        for y in 0..self.height {
            for x in 0..self.width {
                o.set_index(x, y, self.index(x, y));
            }
        }
        o
    }

    /// Whether every palette entry is grey (R = G = B).
    pub fn is_grey(&self) -> bool {
        self.bits != 24 && self.palette.iter().all(|c| c[0] == c[1] && c[1] == c[2])
    }

    /// Allocates an image after checking that its size is sane: no more
    /// than 2^28 bytes of pixels, so that bogus headers fail cleanly.
    pub(crate) fn alloc(width: u32, height: u32, bits: u8) -> Result<Image, Error> {
        if width == 0 || height == 0 { return Err(Error::Corrupt); }
        let rb = (width as u64 * bits as u64).div_ceil(32) * 4;
        if rb * height as u64 > 1 << 28 { return Err(Error::Unsupported); }
        Ok(Image::new(width, height, bits))
    }
}

/// Identifies an image file from its first bytes (at least 64 when the
/// file has them) and its length.
pub fn identify(head: &[u8], file_len: u64) -> Option<Format> {
    if head.starts_with(b"BM") || head.starts_with(b"BA") {
        let at = if head.starts_with(b"BA") { 28 } else { 14 };
        if head.starts_with(b"BA") && head.get(14..16) != Some(b"BM") { return None; }
        return match le32(head, at)? {
            12 => Some(Format::Os13Bmp),
            40 | 52 | 56 | 108 | 124 => Some(Format::WinBmp),
            16..=64 => Some(Format::Os20Bmp),
            _ => None,
        };
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) { return Some(Format::Jpeg); }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") { return Some(Format::Gif); }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        let compressed = tiff::compression(head).is_some_and(|c| c != 1);
        return Some(if head[0] == b'I' { Format::TiffIntel { compressed } } else { Format::TiffMotorola { compressed } });
    }
    if pcx::plausible(head) { return Some(Format::Pcx); }
    if targa::plausible(head, file_len) { return Some(Format::Targa { compressed: head[2] >= 9 }); }
    None
}

/// Decodes an image in any supported format.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    match identify(data, data.len() as u64).ok_or(Error::NotThisFormat)? {
        Format::Os13Bmp | Format::Os20Bmp | Format::WinBmp => bmp::decode(data),
        Format::Jpeg => jpeg::decode(data),
        Format::Gif => gif::decode(data),
        Format::Pcx => pcx::decode(data),
        Format::TiffIntel { .. } | Format::TiffMotorola { .. } => tiff::decode(data),
        Format::Targa { .. } => targa::decode(data),
    }
}

/// Reads that fail instead of panicking past the end.
pub(crate) fn le16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

pub(crate) fn le32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

pub(crate) fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

pub(crate) fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// Picks, for an image with up to 256 distinct colours, the smallest
/// palette depth (1, 4 or 8) and converts it; `None` for more colours.
pub(crate) fn palettize(img: &Image) -> Option<Image> {
    if img.bits != 24 { return Some(img.clone()); }
    let mut pal: Vec<[u8; 3]> = Vec::new();
    let mut o = Image::new(img.width, img.height, 8);
    let mut last = None;
    for y in 0..img.height {
        for x in 0..img.width {
            let c = img.rgb(x, y);
            let i = match last {
                Some((lc, li)) if lc == c => li,
                _ => match pal.iter().position(|&p| p == c) {
                    Some(i) => i,
                    None => {
                        if pal.len() == 256 { return None; }
                        pal.push(c);
                        pal.len() - 1
                    }
                },
            };
            last = Some((c, i));
            o.set_index(x, y, i as u8);
        }
    }
    o.palette = pal;
    Some(o)
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// A 24-bit test pattern: gradients plus a few hard edges.
    pub fn pattern24(w: u32, h: u32) -> Image {
        let mut i = Image::new(w, h, 24);
        for y in 0..h {
            for x in 0..w {
                let c = [(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, if (x / 4 + y / 4) % 2 == 0 { 40 } else { 200 }];
                i.set_rgb(x, y, c);
            }
        }
        i
    }

    /// A palettized test pattern with `bits` bits.
    pub fn pattern_pal(w: u32, h: u32, bits: u8) -> Image {
        let mut i = Image::new(w, h, bits);
        let n = 1u32 << bits;
        i.palette = (0..n).map(|k| [(k * 37) as u8, (k * 91 + 5) as u8, (255u32.wrapping_sub(k * 13)) as u8]).collect();
        for y in 0..h {
            for x in 0..w {
                i.set_index(x, y, ((x + 3 * y) % n) as u8);
            }
        }
        i
    }

    pub fn same_pixels(a: &Image, b: &Image) -> bool {
        a.width == b.width && a.height == b.height && (0..a.height).all(|y| (0..a.width).all(|x| a.rgb(x, y) == b.rgb(x, y)))
    }

    pub fn max_diff(a: &Image, b: &Image) -> (u32, f64) {
        let (mut m, mut sum) = (0u32, 0f64);
        for y in 0..a.height {
            for x in 0..a.width {
                let (p, q) = (a.rgb(x, y), b.rgb(x, y));
                for k in 0..3 {
                    let d = (p[k] as i32 - q[k] as i32).unsigned_abs();
                    m = m.max(d);
                    sum += d as f64;
                }
            }
        }
        (m, sum / (a.width * a.height * 3) as f64)
    }

    /// rs2's work directory (IBM's media), when present.
    pub fn work_dir() -> std::path::PathBuf {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../../work");
        if root.exists() { root } else { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work") }
    }

    /// Files with extension `ext` anywhere under `dir` in the work
    /// directory (tests skip silently without them).
    pub fn work_files(dir: &str, ext: &str) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut todo = vec![work_dir().join(dir)];
        while let Some(d) = todo.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { todo.push(p); } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) { out.push(p); }
            }
        }
        out.sort();
        out
    }

    /// A scratch directory under target/ for generated fixtures.
    pub fn scratch() -> std::path::PathBuf {
        let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mmimage-test");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// Runs macOS sips to convert `src` to `fmt`; false when unavailable.
    pub fn sips(src: &std::path::Path, fmt: &str, out: &std::path::Path) -> bool {
        std::process::Command::new("sips").args(["-s", "format", fmt]).arg(src).arg("--out").arg(out)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .status().is_ok_and(|s| s.success()) && out.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_basics() {
        let mut i = Image::new(5, 2, 4);
        assert_eq!(i.row_bytes(), 4);
        i.palette = vec![[0, 0, 0], [1, 2, 3]];
        i.set_index(3, 1, 1);
        assert_eq!(i.rgb(3, 1), [1, 2, 3]);
        assert_eq!(i.rgb(2, 1), [0, 0, 0]);
        assert_eq!(i.to_24().rgb(3, 1), [1, 2, 3]);
        assert_eq!(Image::new(3, 1, 24).row_bytes(), 12);
        assert_eq!(Image::new(33, 1, 1).row_bytes(), 8);
    }

    #[test]
    fn identify_garbage() {
        assert_eq!(identify(&[], 0), None);
        assert_eq!(identify(&[0u8; 64], 64), None);
        assert!(decode(&[1, 2, 3]).is_err());
    }
}
