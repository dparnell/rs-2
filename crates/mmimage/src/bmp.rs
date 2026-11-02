//! OS/2 and Windows bitmap files.
//!
//! A file is a 14-byte file header ("BM", size, hotspot, offset of the
//! bits) and an info header whose size tells the flavour: 12 bytes for
//! OS/2 1.x (BITMAPCOREHEADER: 16-bit width and height, 3-byte palette
//! entries), 16..64 for OS/2 2.0 (BITMAPINFOHEADER2), 40/52/56/108/124 for
//! Windows (BITMAPINFOHEADER and its V2..V5 extensions). OS/2 2.0 and
//! Windows use 4-byte palette entries. Rows are stored bottom-up (Windows
//! allows top-down with a negative height), each padded to 4 bytes.
//!
//! OS/2 bitmap arrays ("BA") chain several bitmaps of different sizes;
//! the first one is decoded. Compression: RLE8 and RLE4 (both flavours),
//! OS/2's RLE24, and Windows bit fields for 16/32-bit pixels; OS/2's
//! modified Huffman (compression 3 in a BITMAPINFOHEADER2) is not
//! supported.

use alloc::vec::Vec;

use crate::{Error, Image, le16, le32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// OS/2 1.x: BITMAPCOREHEADER.
    Os13,
    /// OS/2 2.0: BITMAPINFOHEADER2 (64 bytes).
    Os20,
    /// Windows 3: BITMAPINFOHEADER.
    Win3,
}

enum Comp {
    None,
    Rle8,
    Rle4,
    Rle24,
    Fields([u32; 3]),
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    let mut base = 0;
    if data.starts_with(b"BA") {
        // Bitmap array: the first element's file header follows.
        base = 14;
        if data.get(14..16) != Some(b"BM") { return Err(if data.len() < 16 { Error::Truncated } else { Error::Unsupported }); }
    } else if !data.starts_with(b"BM") {
        return Err(Error::NotThisFormat);
    }
    let off_bits = le32(data, base + 10).ok_or(Error::Truncated)? as usize;
    let h = base + 14;
    let hsize = le32(data, h).ok_or(Error::Truncated)? as usize;
    let (width, height, bits, comp, pal_entry, pal_count, top_down, os2);
    if hsize == 12 {
        width = le16(data, h + 4).ok_or(Error::Truncated)? as i64;
        height = le16(data, h + 6).ok_or(Error::Truncated)? as i64;
        bits = le16(data, h + 10).ok_or(Error::Truncated)?;
        comp = Comp::None;
        pal_entry = 3;
        pal_count = if bits <= 8 { 1usize << bits } else { 0 };
        top_down = false;
        os2 = true;
    } else if (16..=124).contains(&hsize) {
        if data.len() < h + hsize { return Err(Error::Truncated); }
        let win = matches!(hsize, 40 | 52 | 56 | 108 | 124);
        if !win && hsize > 64 { return Err(Error::Unsupported); }
        os2 = !win;
        width = le32(data, h + 4).ok_or(Error::Truncated)? as i32 as i64;
        let hh = le32(data, h + 8).ok_or(Error::Truncated)? as i32 as i64;
        top_down = win && hh < 0;
        height = hh.abs();
        bits = le16(data, h + 14).ok_or(Error::Truncated)?;
        let c = if hsize >= 20 { le32(data, h + 16).unwrap_or(0) } else { 0 };
        let used = if hsize >= 36 { le32(data, h + 32).unwrap_or(0) as usize } else { 0 };
        comp = match (c, win) {
            (0, _) => Comp::None,
            (1, _) => Comp::Rle8,
            (2, _) => Comp::Rle4,
            (4, false) => Comp::Rle24,
            (3, true) => {
                // Masks follow a 40-byte header, or are part of a longer one.
                let m = h + 40;
                Comp::Fields([le32(data, m).ok_or(Error::Truncated)?, le32(data, m + 4).ok_or(Error::Truncated)?, le32(data, m + 8).ok_or(Error::Truncated)?])
            }
            _ => return Err(Error::Unsupported),
        };
        pal_entry = 4;
        pal_count = if bits <= 8 { if used != 0 && used <= 256 { used } else { 1usize << bits } } else { 0 };
    } else {
        return Err(Error::Corrupt);
    }
    let _ = os2;
    if width <= 0 || height <= 0 || width > 0xFFFF || height > 0xFFFF { return Err(Error::Corrupt); }
    let (width, height) = (width as u32, height as u32);
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) { return Err(Error::Unsupported); }
    match comp {
        Comp::Rle8 if bits != 8 => return Err(Error::Corrupt),
        Comp::Rle4 if bits != 4 => return Err(Error::Corrupt),
        Comp::Rle24 if bits != 24 => return Err(Error::Corrupt),
        Comp::Fields(_) if bits != 16 && bits != 32 => return Err(Error::Corrupt),
        _ => {}
    }
    // The palette follows the header (and the masks of a 40-byte header).
    let mut pal_at = h + hsize;
    if hsize == 40 && matches!(comp, Comp::Fields(_)) { pal_at += 12; }
    let mut palette = Vec::new();
    for i in 0..pal_count {
        let at = pal_at + i * pal_entry;
        match data.get(at..at + 3) {
            Some(p) => palette.push([p[2], p[1], p[0]]),
            None => break,
        }
    }
    if bits <= 8 && palette.is_empty() { return Err(Error::Truncated); }
    let bits_at = if off_bits >= pal_at && off_bits < data.len() { off_bits } else { pal_at + pal_count * pal_entry };
    let src = data.get(bits_at..).ok_or(Error::Truncated)?;
    let out_bits = if bits <= 8 { bits as u8 } else { 24 };
    let mut img = Image::alloc(width, height, out_bits)?;
    img.palette = palette;
    let row_of = |y: u32| if top_down { y } else { height - 1 - y };
    match comp {
        Comp::Rle8 | Comp::Rle4 | Comp::Rle24 => {
            let px = match comp { Comp::Rle8 => 1, Comp::Rle4 => 0, _ => 3 };
            rle_decode(src, &mut img, px, true)?;
        }
        Comp::None | Comp::Fields(_) => {
            let srb = (width as usize * bits as usize).div_ceil(32) * 4;
            let masks = match comp {
                Comp::Fields(m) => m,
                _ if bits == 16 => [0x7C00, 0x03E0, 0x001F],
                _ => [0xFF0000, 0xFF00, 0xFF],
            };
            for sy in 0..height {
                let at = sy as usize * srb;
                // A short last strip still decodes what is there.
                let Some(row) = src.get(at..at + srb) else {
                    if sy == 0 { return Err(Error::Truncated); }
                    break;
                };
                let y = row_of(sy);
                match bits {
                    1 | 4 | 8 => {
                        let n = img.row_bytes();
                        img.row_mut(y).copy_from_slice(&row[..n]);
                    }
                    24 => {
                        let n = width as usize * 3;
                        img.row_mut(y)[..n].copy_from_slice(&row[..n]);
                    }
                    _ => {
                        for x in 0..width {
                            let v = if bits == 16 { le16(row, x as usize * 2).unwrap_or(0) as u32 } else { le32(row, x as usize * 4).unwrap_or(0) };
                            img.set_rgb(x, y, [field(v, masks[0]), field(v, masks[1]), field(v, masks[2])]);
                        }
                    }
                }
            }
        }
    }
    Ok(img)
}

/// Scales the bits of `v` under `mask` to 0..=255.
fn field(v: u32, mask: u32) -> u8 {
    if mask == 0 { return 0; }
    let shift = mask.trailing_zeros();
    let m = mask >> shift;
    let x = (v & mask) >> shift;
    ((x as u64 * 255 + m as u64 / 2) / m as u64) as u8
}

/// Decodes RLE4 (`px` = 0), RLE8 (1) or OS/2 RLE24 (3) into `img`, rows
/// bottom-up when `bottom_up`. Pixels not written keep their value
/// (index 0 in a fresh image; the previous frame for AVI deltas).
pub fn rle_decode(src: &[u8], img: &mut Image, px: usize, bottom_up: bool) -> Result<(), Error> {
    let (w, h) = (img.width, img.height);
    let (mut x, mut y) = (0u32, 0u32);
    let mut i = 0;
    let put = |img: &mut Image, x: u32, y: u32, p: &[u8]| {
        if x >= w || y >= h { return; }
        let yy = if bottom_up { h - 1 - y } else { y };
        if px == 3 { img.set_rgb(x, yy, [p[2], p[1], p[0]]) } else { img.set_index(x, yy, p[0]) }
    };
    while y < h {
        let (Some(&n), Some(&c)) = (src.get(i), src.get(i + 1)) else { return Ok(()) };
        i += 2;
        if n > 0 {
            // A run: n pixels of one value (RLE4 alternates two nibbles).
            let p: [u8; 3] = if px == 3 {
                let Some(b) = src.get(i - 1..i + 2) else { return Ok(()) };
                i += 2;
                [b[0], b[1], b[2]]
            } else {
                [c, 0, 0]
            };
            for k in 0..n as u32 {
                let v = if px == 0 { if k % 2 == 0 { [c >> 4, 0, 0] } else { [c & 15, 0, 0] } } else { p };
                put(img, x + k, y, &v);
            }
            x += n as u32;
            continue;
        }
        match c {
            0 => { x = 0; y += 1; }
            1 => return Ok(()),
            2 => {
                let (Some(&dx), Some(&dy)) = (src.get(i), src.get(i + 1)) else { return Ok(()) };
                i += 2;
                x += dx as u32;
                y += dy as u32;
            }
            n => {
                // Absolute run of n pixels, padded to a 16-bit boundary.
                let n = n as usize;
                let len = match px { 0 => n.div_ceil(2), 1 => n, _ => n * 3 };
                let Some(run) = src.get(i..i + len) else { return Err(Error::Truncated) };
                for k in 0..n {
                    let v = match px {
                        0 => [if k % 2 == 0 { run[k / 2] >> 4 } else { run[k / 2] & 15 }, 0, 0],
                        1 => [run[k], 0, 0],
                        _ => [run[k * 3], run[k * 3 + 1], run[k * 3 + 2]],
                    };
                    put(img, x + k as u32, y, &v);
                }
                x += n as u32;
                i += len + (len & 1);
            }
        }
    }
    Ok(())
}

/// Encodes a 1/4/8/24-bit image, rows bottom-up, uncompressed.
pub fn encode(img: &Image, kind: Kind) -> Vec<u8> {
    let tmp;
    let img = if matches!(img.bits, 1 | 4 | 8 | 24) { img } else { tmp = img.to_24(); &tmp };
    let pal_count = if img.bits <= 8 { 1usize << img.bits } else { 0 };
    let (hsize, entry) = match kind { Kind::Os13 => (12, 3), Kind::Os20 => (64, 4), Kind::Win3 => (40, 4) };
    let rb = img.row_bytes();
    let off = 14 + hsize + pal_count * entry;
    let size = off + rb * img.height as usize;
    let mut o = Vec::with_capacity(size);
    o.extend_from_slice(b"BM");
    o.extend_from_slice(&(size as u32).to_le_bytes());
    o.extend_from_slice(&[0; 4]);
    o.extend_from_slice(&(off as u32).to_le_bytes());
    o.extend_from_slice(&(hsize as u32).to_le_bytes());
    if kind == Kind::Os13 {
        o.extend_from_slice(&(img.width as u16).to_le_bytes());
        o.extend_from_slice(&(img.height as u16).to_le_bytes());
        o.extend_from_slice(&1u16.to_le_bytes());
        o.extend_from_slice(&(img.bits as u16).to_le_bytes());
    } else {
        o.extend_from_slice(&img.width.to_le_bytes());
        o.extend_from_slice(&img.height.to_le_bytes());
        o.extend_from_slice(&1u16.to_le_bytes());
        o.extend_from_slice(&(img.bits as u16).to_le_bytes());
        o.extend_from_slice(&0u32.to_le_bytes());
        o.extend_from_slice(&((rb * img.height as usize) as u32).to_le_bytes());
        o.extend_from_slice(&2835u32.to_le_bytes());
        o.extend_from_slice(&2835u32.to_le_bytes());
        o.extend_from_slice(&(pal_count as u32).to_le_bytes());
        o.extend_from_slice(&0u32.to_le_bytes());
        // The rest of BITMAPINFOHEADER2 (units, recording, rendering,
        // halftoning sizes, colour encoding, identifier) is all zero.
        o.resize(14 + hsize, 0);
    }
    for i in 0..pal_count {
        let c = img.palette.get(i).copied().unwrap_or([0; 3]);
        o.extend_from_slice(&[c[2], c[1], c[0]]);
        if entry == 4 { o.push(0); }
    }
    for y in (0..img.height).rev() {
        o.extend_from_slice(img.row(y));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use alloc::vec;

    #[test]
    fn round_trips() {
        for kind in [Kind::Os13, Kind::Os20, Kind::Win3] {
            for bits in [1, 4, 8] {
                let i = pattern_pal(13, 7, bits);
                let d = decode(&encode(&i, kind)).unwrap();
                assert_eq!(d.bits, bits);
                assert!(same_pixels(&i, &d), "{kind:?} {bits}");
            }
            let i = pattern24(17, 9);
            let e = encode(&i, kind);
            assert_eq!(crate::identify(&e, e.len() as u64), Some(match kind { Kind::Os13 => crate::Format::Os13Bmp, Kind::Os20 => crate::Format::Os20Bmp, Kind::Win3 => crate::Format::WinBmp }));
            assert_eq!(decode(&e).unwrap(), i);
        }
    }

    fn win_header(w: i32, h: i32, bits: u16, comp: u32, extra: &[u8], pixels: &[u8]) -> Vec<u8> {
        let mut o = vec![];
        o.extend_from_slice(b"BM");
        let off = 14 + 40 + extra.len();
        o.extend_from_slice(&((off + pixels.len()) as u32).to_le_bytes());
        o.extend_from_slice(&[0; 4]);
        o.extend_from_slice(&(off as u32).to_le_bytes());
        o.extend_from_slice(&40u32.to_le_bytes());
        o.extend_from_slice(&w.to_le_bytes());
        o.extend_from_slice(&h.to_le_bytes());
        o.extend_from_slice(&1u16.to_le_bytes());
        o.extend_from_slice(&bits.to_le_bytes());
        o.extend_from_slice(&comp.to_le_bytes());
        o.extend_from_slice(&[0; 20]);
        o.extend_from_slice(extra);
        o.extend_from_slice(pixels);
        o
    }

    #[test]
    fn rle8_and_rle4() {
        let pal: Vec<u8> = (0..16u8).flat_map(|i| [i, i, i, 0]).collect();
        // 4x2, RLE8: bottom row "1 1 1 2", top row absolute "3 4 5" + delta.
        let rle = [3, 1, 1, 2, 0, 0, 0, 3, 3, 4, 5, 0, 0, 1];
        let d = decode(&win_header(4, 2, 8, 1, &pal, &rle)).unwrap();
        assert_eq!(d.row(1)[..4], [1, 1, 1, 2]);
        assert_eq!(d.row(0)[..4], [3, 4, 5, 0]);
        let rle4 = [4, 0x12, 0, 0, 0, 3, 0x34, 0x50, 0, 1];
        let d = decode(&win_header(4, 2, 4, 2, &pal, &rle4)).unwrap();
        assert_eq!((0..4).map(|x| d.index(x, 1)).collect::<Vec<_>>(), [1, 2, 1, 2]);
        assert_eq!((0..4).map(|x| d.index(x, 0)).collect::<Vec<_>>(), [3, 4, 5, 0]);
    }

    #[test]
    fn os2_rle24_and_short_header() {
        // OS/2 2.0, cbFix 64, RLE24 (compression 4): 3x2, bottom row a
        // run of 3 red, top row absolute green, blue, white.
        let mut e = encode(&Image::new(3, 2, 24), Kind::Os20);
        e[30..34].copy_from_slice(&4u32.to_le_bytes());
        e.truncate(14 + 64);
        e.extend_from_slice(&[3, 0, 0, 255, 0, 0, 0, 3, 0, 255, 0, 255, 0, 0, 255, 255, 255, 0, 0, 1]);
        let d = decode(&e).unwrap();
        assert!((0..3).all(|x| d.rgb(x, 1) == [255, 0, 0]));
        assert_eq!([d.rgb(0, 0), d.rgb(1, 0), d.rgb(2, 0)], [[0, 255, 0], [0, 0, 255], [255, 255, 255]]);
        // A 16-byte BITMAPINFOHEADER2 (no compression field).
        let i = pattern_pal(5, 3, 4);
        let full = encode(&i, Kind::Os20);
        let mut s = full[..14 + 16].to_vec();
        s[14..18].copy_from_slice(&16u32.to_le_bytes());
        s.extend_from_slice(&full[14 + 64..]);
        let off = le32(&s, 10).unwrap() - 48;
        s[10..14].copy_from_slice(&off.to_le_bytes());
        assert_eq!(crate::identify(&s, s.len() as u64), Some(crate::Format::Os20Bmp));
        assert!(same_pixels(&decode(&s).unwrap(), &i));
    }

    #[test]
    fn bitfields_and_top_down() {
        // 2x1, 16-bit 5:6:5, top-down.
        let masks = [0xF800u32, 0x07E0, 0x001F].iter().flat_map(|m| m.to_le_bytes()).collect::<Vec<_>>();
        let px = [0x00, 0xF8, 0x1F, 0x00];
        let d = decode(&win_header(2, -1, 16, 3, &masks, &px)).unwrap();
        assert_eq!(d.rgb(0, 0), [255, 0, 0]);
        assert_eq!(d.rgb(1, 0), [0, 0, 255]);
        // 32-bit default masks, two rows bottom-up.
        let px = [1, 2, 3, 0, 4, 5, 6, 0];
        let d = decode(&win_header(1, 2, 32, 0, &[], &px)).unwrap();
        assert_eq!(d.rgb(0, 1), [3, 2, 1]);
        assert_eq!(d.rgb(0, 0), [6, 5, 4]);
    }

    #[test]
    fn bitmap_array() {
        let inner = encode(&pattern_pal(3, 3, 4), Kind::Os20);
        let mut o = b"BA".to_vec();
        o.extend_from_slice(&[40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // Offsets inside the array are from the start of the whole file.
        let mut inner = inner;
        let off = le32(&inner, 10).unwrap() + 14;
        inner[10..14].copy_from_slice(&off.to_le_bytes());
        o.extend_from_slice(&inner);
        assert_eq!(crate::identify(&o, o.len() as u64), Some(crate::Format::Os20Bmp));
        assert!(same_pixels(&decode(&o).unwrap(), &pattern_pal(3, 3, 4)));
    }

    #[test]
    fn bad_input() {
        let e = encode(&pattern_pal(8, 8, 8), Kind::Win3);
        for n in 0..e.len() {
            let _ = decode(&e[..n]);
        }
        let mut bad = e.clone();
        bad[18] = 0;
        bad[22] = 0;
        assert!(decode(&bad).is_err());
    }

    #[test]
    fn real_files() {
        // Every BMP on the OS/2 media decodes (skipped when absent).
        let mut n = 0;
        for d in ["unpacked", "media"] {
            for p in work_files(d, "bmp") {
                let data = std::fs::read(&p).unwrap();
                match decode(&data) {
                    Ok(i) => { n += 1; assert!(i.width > 0 && i.height > 0); }
                    Err(Error::NotThisFormat) if !data.starts_with(b"B") => {}
                    Err(e) => panic!("{}: {e:?}", p.display()),
                }
            }
        }
        eprintln!("decoded {n} BMP files");
    }
}
