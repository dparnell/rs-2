//! ZSoft PCX files.
//!
//! A 128-byte header (0x0A, version, encoding 1 = RLE, bits per pixel per
//! plane, window xmin/ymin/xmax/ymax, resolution, a 16-entry palette,
//! number of planes, bytes per line per plane), then the scan lines: each
//! is every plane's bytes in turn, run-length encoded as a stream (a
//! byte with the top two bits set is a count for the next byte). 8-bit
//! images keep their 256-entry palette at the end of the file after a
//! 0x0C byte. Supported: 1 bit × 1..4 planes (mono and EGA), 2/4 bits ×
//! 1 plane (packed), 8 bits × 1 plane (256 colours), 8 bits × 3/4 planes
//! (24-bit RGB, a fourth plane ignored).

use alloc::vec;
use alloc::vec::Vec;

use crate::{Error, Image, le16};

pub fn plausible(h: &[u8]) -> bool {
    h.len() >= 128 && h[0] == 0x0A && matches!(h[1], 0 | 2 | 3 | 4 | 5) && h[2] <= 1 && matches!(h[3], 1 | 2 | 4 | 8)
        && (1..=4).contains(&h[65]) && le16(h, 8) >= le16(h, 4) && le16(h, 10) >= le16(h, 6) && le16(h, 66).is_some_and(|b| b > 0)
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.len() < 128 { return Err(if data.first() == Some(&0x0A) { Error::Truncated } else { Error::NotThisFormat }); }
    if !plausible(data) { return Err(Error::NotThisFormat); }
    let (bpp, planes) = (data[3] as u32, data[65] as u32);
    let w = le16(data, 8).unwrap_or(0) as u32 - le16(data, 4).unwrap_or(0) as u32 + 1;
    let h = le16(data, 10).unwrap_or(0) as u32 - le16(data, 6).unwrap_or(0) as u32 + 1;
    let bpl = le16(data, 66).unwrap_or(0) as usize;
    let line = bpl * planes as usize;
    if (bpl * 8) < (w * bpp) as usize { return Err(Error::Corrupt); }
    let truecolour = bpp == 8 && planes >= 3;
    if !(truecolour || (bpp == 1 && planes <= 4) || planes == 1) { return Err(Error::Unsupported); }
    let mut img = Image::alloc(w, h, if truecolour { 24 } else if bpp == 8 { 8 } else if bpp == 1 && planes == 1 { 1 } else { 4 })?;
    let hp: Vec<[u8; 3]> = data[16..64].chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    if bpp == 8 && !truecolour {
        let n = data.len();
        img.palette = if n >= 128 + 769 && data[n - 769] == 0x0C {
            data[n - 768..].chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
        } else {
            (0..=255u8).map(|v| [v; 3]).collect()
        };
    } else if img.bits == 1 {
        img.palette = if hp[0] != hp[1] { hp[..2].to_vec() } else { vec![[0; 3], [255; 3]] };
    } else if img.bits == 4 {
        img.palette = hp;
    }
    // Decompress all scan lines.
    let src = &data[128..];
    let total = line * h as usize;
    let mut buf = Vec::with_capacity(total);
    let mut i = 0;
    if data[2] == 1 {
        while buf.len() < total {
            let Some(&b) = src.get(i) else { break };
            i += 1;
            if b & 0xC0 == 0xC0 {
                let Some(&v) = src.get(i) else { break };
                i += 1;
                let n = ((b & 0x3F) as usize).min(total - buf.len());
                buf.extend(core::iter::repeat_n(v, n));
            } else {
                buf.push(b);
            }
        }
    } else {
        buf.extend_from_slice(&src[..total.min(src.len())]);
    }
    if buf.is_empty() { return Err(Error::Truncated); }
    buf.resize(total, 0);
    for y in 0..h {
        let l = &buf[y as usize * line..(y as usize + 1) * line];
        for x in 0..w {
            let xi = x as usize;
            if truecolour {
                img.set_rgb(x, y, [l[xi], l[bpl + xi], l[2 * bpl + xi]]);
            } else if bpp == 1 {
                let mut v = 0;
                for p in 0..planes as usize {
                    v |= ((l[p * bpl + xi / 8] >> (7 - xi % 8)) & 1) << p;
                }
                img.set_index(x, y, v);
            } else {
                let per = 8 / bpp as usize;
                let b = l[xi / per];
                let v = (b >> (8 - bpp as usize * (xi % per + 1))) & ((1 << bpp) - 1) as u8;
                img.set_index(x, y, v);
            }
        }
    }
    Ok(img)
}

/// Encodes 1-bit images as mono, 4-bit as 4-plane EGA, 8-bit with the
/// 256-colour palette and 24-bit as three 8-bit planes, all RLE.
pub fn encode(img: &Image) -> Vec<u8> {
    let tmp;
    let img = if matches!(img.bits, 1 | 4 | 8 | 24) { img } else { tmp = img.to_24(); &tmp };
    let (bpp, planes) = match img.bits { 1 => (1u8, 1u8), 4 => (1, 4), 8 => (8, 1), _ => (8, 3) };
    let bpl = ((img.width as usize * bpp as usize).div_ceil(8) + 1) & !1;
    let mut o = vec![0u8; 128];
    o[..4].copy_from_slice(&[0x0A, 5, 1, bpp]);
    o[8..10].copy_from_slice(&((img.width - 1) as u16).to_le_bytes());
    o[10..12].copy_from_slice(&((img.height - 1) as u16).to_le_bytes());
    o[12..14].copy_from_slice(&72u16.to_le_bytes());
    o[14..16].copy_from_slice(&72u16.to_le_bytes());
    if img.bits <= 4 {
        for (i, c) in img.palette.iter().take(16).enumerate() {
            o[16 + i * 3..19 + i * 3].copy_from_slice(c);
        }
    }
    o[65] = planes;
    o[66..68].copy_from_slice(&(bpl as u16).to_le_bytes());
    o[68] = if img.is_grey() { 2 } else { 1 };
    let mut line = vec![0u8; bpl * planes as usize];
    for y in 0..img.height {
        line.fill(0);
        for x in 0..img.width {
            let xi = x as usize;
            match img.bits {
                24 => {
                    let c = img.rgb(x, y);
                    for p in 0..3 { line[p * bpl + xi] = c[p]; }
                }
                8 => line[xi] = img.index(x, y),
                _ => {
                    let v = img.index(x, y);
                    for p in 0..planes as usize {
                        if (v >> p) & 1 != 0 { line[p * bpl + xi / 8] |= 0x80 >> (xi % 8); }
                    }
                }
            }
        }
        // Runs do not cross scan lines (as ZSoft recommends).
        let mut i = 0;
        while i < line.len() {
            let v = line[i];
            let mut n = 1;
            while i + n < line.len() && n < 63 && line[i + n] == v { n += 1; }
            if n > 1 || v & 0xC0 == 0xC0 { o.push(0xC0 | n as u8); }
            o.push(v);
            i += n;
        }
    }
    if img.bits == 8 {
        o.push(0x0C);
        for i in 0..256 {
            o.extend_from_slice(&img.palette.get(i).copied().unwrap_or([0; 3]));
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
        for bits in [4, 8] {
            let i = pattern_pal(21, 9, bits);
            let e = encode(&i);
            assert_eq!(crate::identify(&e, e.len() as u64), Some(crate::Format::Pcx));
            let d = decode(&e).unwrap();
            assert_eq!(d.bits, bits);
            assert!(same_pixels(&i, &d), "{bits}");
        }
        let mut i = pattern_pal(21, 9, 1);
        i.palette = vec![[0; 3], [255; 3]];
        assert!(same_pixels(&i, &decode(&encode(&i)).unwrap()));
        let i = pattern24(33, 7);
        assert_eq!(decode(&encode(&i)).unwrap(), i);
    }

    #[test]
    fn truncated() {
        let e = encode(&pattern24(8, 8));
        for n in 0..e.len() { let _ = decode(&e[..n]); }
    }

    #[test]
    fn toolkit_files() {
        for p in work_files("media/boot/toolkit/os2tk45/samples/mm/beehive", "pcx") {
            let i = decode(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e:?}", p.display()));
            assert!(i.width > 0);
            // Re-encoding gives the same pixels.
            assert!(same_pixels(&i, &decode(&encode(&i)).unwrap()));
        }
    }
}
