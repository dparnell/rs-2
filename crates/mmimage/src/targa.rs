//! Truevision Targa (TGA) files.
//!
//! An 18-byte header (ID length, colour map type, image type, colour map
//! origin/length/entry size, image origin, width, height, pixel depth,
//! descriptor), the ID field, the colour map, then the pixels. Image
//! types: 1 colour-mapped, 2 true colour, 3 grey, and 9/10/11 the same
//! run-length encoded (a packet byte: high bit set = the next pixel
//! repeated (n & 0x7F) + 1 times, clear = n + 1 literal pixels). Pixels
//! are stored bottom-up unless descriptor bit 5 is set; bit 4 mirrors
//! them right to left. 15/16-bit pixels are little-endian x:R5:G5:B5,
//! 24/32-bit ones B,G,R(,A).
//!
//! The format has no magic number, so [`plausible`] checks a header
//! strictly before a file is taken for a Targa.

use alloc::vec::Vec;

use crate::{Error, Image, le16};

/// Whether `h` is a believable Targa header for a file of `file_len`
/// bytes.
pub fn plausible(h: &[u8], file_len: u64) -> bool {
    if h.len() < 18 { return false; }
    let (cmap_type, ty) = (h[1], h[2]);
    let cmap_len = le16(h, 5).unwrap_or(0) as u64;
    let cmap_bits = h[7];
    let (w, hh, bpp, desc) = (le16(h, 12).unwrap_or(0) as u64, le16(h, 14).unwrap_or(0) as u64, h[16], h[17]);
    if !matches!(ty, 1 | 2 | 3 | 9 | 10 | 11) || cmap_type > 1 || w == 0 || hh == 0 || desc & 0xC0 != 0 { return false; }
    match ty & 7 {
        1 => if cmap_type != 1 || !matches!(bpp, 8 | 16) || cmap_len == 0 { return false },
        2 => if !matches!(bpp, 15 | 16 | 24 | 32) { return false },
        _ => if !matches!(bpp, 8 | 16) { return false },
    }
    let cmap_bytes = if cmap_type == 1 {
        if !matches!(cmap_bits, 15 | 16 | 24 | 32) { return false; }
        cmap_len * (cmap_bits as u64).div_ceil(8)
    } else {
        if cmap_len != 0 { return false; }
        0
    };
    let pixel_bytes = w * hh * (bpp as u64).div_ceil(8);
    // RLE packets hold at most 128 pixels each.
    let min_pixels = if ty >= 9 { (w * hh).div_ceil(128) * (1 + (bpp as u64).div_ceil(8)) } else { pixel_bytes };
    18 + h[0] as u64 + cmap_bytes + min_pixels <= file_len
}

fn colour(p: &[u8], bits: u8) -> [u8; 3] {
    match bits {
        15 | 16 => {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let s = |x: u16| ((x & 31) * 255 / 31) as u8;
            [s(v >> 10), s(v >> 5), s(v)]
        }
        8 => [p[0]; 3],
        _ => [p[2], p[1], p[0]],
    }
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.len() < 18 { return Err(Error::Truncated); }
    if !plausible(data, u64::MAX) { return Err(Error::NotThisFormat); }
    let (id_len, cmap_type, ty) = (data[0] as usize, data[1], data[2]);
    let cmap_first = le16(data, 3).unwrap_or(0) as usize;
    let cmap_len = le16(data, 5).unwrap_or(0) as usize;
    let cmap_bits = data[7];
    let (w, h, bpp, desc) = (le16(data, 12).unwrap_or(0) as u32, le16(data, 14).unwrap_or(0) as u32, data[16], data[17]);
    let mut at = 18 + id_len;
    let mut cmap = Vec::new();
    if cmap_type == 1 {
        let eb = (cmap_bits as usize).div_ceil(8);
        let m = data.get(at..at + cmap_len * eb).ok_or(Error::Truncated)?;
        cmap = m.chunks(eb).map(|p| colour(p, cmap_bits)).collect();
        at += cmap_len * eb;
    }
    let pb = (bpp as usize).div_ceil(8);
    let mapped = ty & 7 == 1;
    let grey = ty & 7 == 3;
    // Colour-mapped images with up to 256 entries stay palettized.
    let pal8 = (mapped && cmap_first + cmap_len <= 256 && bpp == 8) || grey;
    let mut img = Image::alloc(w, h, if pal8 { 8 } else { 24 })?;
    if grey {
        img.palette = (0..=255u8).map(|v| [v; 3]).collect();
    } else if pal8 {
        img.palette = alloc::vec![[0; 3]; cmap_first + cmap_len];
        img.palette[cmap_first..].copy_from_slice(&cmap);
    }
    let src = &data[at.min(data.len())..];
    let total = w as usize * h as usize;
    let mut put = |n: usize, p: &[u8]| {
        let (mut x, mut y) = ((n % w as usize) as u32, (n / w as usize) as u32);
        if desc & 0x20 == 0 { y = h - 1 - y; }
        if desc & 0x10 != 0 { x = w - 1 - x; }
        if pal8 {
            img.set_index(x, y, p[0]);
        } else if mapped {
            let i = if pb == 2 { u16::from_le_bytes([p[0], p[1]]) as usize } else { p[0] as usize };
            img.set_rgb(x, y, i.checked_sub(cmap_first).and_then(|i| cmap.get(i)).copied().unwrap_or([0; 3]));
        } else {
            img.set_rgb(x, y, colour(p, bpp));
        }
    };
    if ty < 9 {
        if src.len() < total * pb {
            if src.len() < pb { return Err(Error::Truncated); }
        }
        for (n, p) in src.chunks_exact(pb).take(total).enumerate() {
            put(n, p);
        }
    } else {
        let (mut n, mut i) = (0, 0);
        while n < total {
            let Some(&c) = src.get(i) else { break };
            i += 1;
            let count = (c & 0x7F) as usize + 1;
            if c & 0x80 != 0 {
                let Some(p) = src.get(i..i + pb) else { break };
                i += pb;
                for _ in 0..count.min(total - n) {
                    put(n, p);
                    n += 1;
                }
            } else {
                for _ in 0..count {
                    let Some(p) = src.get(i..i + pb) else { break };
                    i += pb;
                    if n < total { put(n, p); n += 1; }
                }
            }
        }
        if n == 0 { return Err(Error::Truncated); }
    }
    Ok(img)
}

/// Encodes a palettized image as type 1/9 (8-bit indexes, 24-bit colour
/// map) and a 24-bit one as type 2/10, rows bottom-up.
pub fn encode(img: &Image, compressed: bool) -> Vec<u8> {
    let tmp;
    let img = if img.bits == 8 || img.bits == 24 { img } else { tmp = img.to_8(); &tmp };
    let mapped = img.bits == 8;
    let mut o = Vec::new();
    let ty = if mapped { 1 } else { 2 } + if compressed { 8 } else { 0 };
    let cmap_len = if mapped { img.palette.len().clamp(1, 256) } else { 0 };
    o.extend_from_slice(&[0, mapped as u8, ty, 0, 0]);
    o.extend_from_slice(&(cmap_len as u16).to_le_bytes());
    o.push(if mapped { 24 } else { 0 });
    o.extend_from_slice(&[0, 0, 0, 0]);
    o.extend_from_slice(&(img.width as u16).to_le_bytes());
    o.extend_from_slice(&(img.height as u16).to_le_bytes());
    o.extend_from_slice(&[if mapped { 8 } else { 24 }, 0]);
    for i in 0..cmap_len {
        let c = img.palette.get(i).copied().unwrap_or([0; 3]);
        o.extend_from_slice(&[c[2], c[1], c[0]]);
    }
    let pb = if mapped { 1 } else { 3 };
    for y in (0..img.height).rev() {
        let row = &img.row(y)[..img.width as usize * pb];
        if !compressed {
            o.extend_from_slice(row);
            continue;
        }
        let px: Vec<&[u8]> = row.chunks(pb).collect();
        let mut i = 0;
        while i < px.len() {
            let mut run = 1;
            while i + run < px.len() && run < 128 && px[i + run] == px[i] { run += 1; }
            if run >= 2 {
                o.push(0x80 | (run - 1) as u8);
                o.extend_from_slice(px[i]);
                i += run;
                continue;
            }
            // Literal pixels up to the next run of two.
            let mut n = 1;
            while i + n < px.len() && n < 128 && !(i + n + 1 < px.len() && px[i + n] == px[i + n + 1]) { n += 1; }
            o.push((n - 1) as u8);
            for p in &px[i..i + n] { o.extend_from_slice(p); }
            i += n;
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    #[test]
    fn round_trips() {
        for c in [false, true] {
            let i = pattern24(19, 11);
            let e = encode(&i, c);
            assert_eq!(crate::identify(&e, e.len() as u64), Some(crate::Format::Targa { compressed: c }));
            assert_eq!(decode(&e).unwrap(), i);
            for bits in [1, 4, 8] {
                let i = pattern_pal(23, 5, bits);
                let d = decode(&encode(&i, c)).unwrap();
                assert!(same_pixels(&i, &d));
            }
            // Long runs.
            let mut i = Image::new(300, 2, 24);
            i.set_rgb(299, 1, [1, 2, 3]);
            assert_eq!(decode(&encode(&i, c)).unwrap(), i);
        }
    }

    #[test]
    fn top_left_16bit() {
        // 2x1 type 2, 16 bpp, top-left origin: red, blue.
        let mut f = alloc::vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 16, 0x20];
        f.extend_from_slice(&[0x00, 0x7C, 0x1F, 0x00]);
        let d = decode(&f).unwrap();
        assert_eq!(d.rgb(0, 0), [255, 0, 0]);
        assert_eq!(d.rgb(1, 0), [0, 0, 255]);
        assert!(!plausible(&f, 20));
    }

    #[test]
    fn truncated() {
        let e = encode(&pattern24(8, 8), true);
        for n in 0..e.len() { let _ = decode(&e[..n]); }
    }
}
