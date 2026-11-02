//! GIF87a and GIF89a files.
//!
//! A header, the logical screen descriptor (size, global colour table
//! flag and size, background index), the global colour table, then
//! blocks: extensions (0x21, label, data sub-blocks; the graphic control
//! extension 0xF9 gives a transparent index), images (0x2C: position,
//! size, local colour table, interlace flag, then the LZW minimum code
//! size and the compressed data in sub-blocks of up to 255 bytes) and
//! the trailer (0x3B). Only the first image is decoded, drawn over the
//! logical screen filled with the background colour.
//!
//! LZW codes are packed least significant bit first; they start one bit
//! wider than the minimum code size, with a clear code (1 << min) and an
//! end code (clear + 1), and grow to at most 12 bits.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Error, Image, le16, palettize};

fn table(d: &[u8], at: usize, n: usize) -> Result<Vec<[u8; 3]>, Error> {
    let t = d.get(at..at + n * 3).ok_or(Error::Truncated)?;
    Ok(t.chunks(3).map(|c| [c[0], c[1], c[2]]).collect())
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) { return Err(Error::NotThisFormat); }
    let sw = le16(data, 6).ok_or(Error::Truncated)? as u32;
    let sh = le16(data, 8).ok_or(Error::Truncated)? as u32;
    let flags = *data.get(10).ok_or(Error::Truncated)?;
    let bg = *data.get(11).ok_or(Error::Truncated)?;
    let mut at = 13;
    let global = if flags & 0x80 != 0 {
        let n = 2usize << (flags & 7);
        at += n * 3;
        Some(table(data, 13, n)?)
    } else {
        None
    };
    let mut transparent = None;
    loop {
        match *data.get(at).ok_or(Error::Truncated)? {
            0x21 => {
                let label = *data.get(at + 1).ok_or(Error::Truncated)?;
                at += 2;
                if label == 0xF9 && data.get(at) == Some(&4) {
                    let p = data.get(at + 1..at + 5).ok_or(Error::Truncated)?;
                    transparent = if p[0] & 1 != 0 { Some(p[3]) } else { None };
                }
                at = skip_blocks(data, at)?;
            }
            0x2C => break,
            0x3B => return Err(Error::Corrupt),
            _ => return Err(Error::Corrupt),
        }
    }
    let d = data.get(at..at + 10).ok_or(Error::Truncated)?;
    let (left, top, w, h, f) = (le16(d, 1).unwrap_or(0) as u32, le16(d, 3).unwrap_or(0) as u32, le16(d, 5).unwrap_or(0) as u32, le16(d, 7).unwrap_or(0) as u32, d[9]);
    at += 10;
    let mut pal = if f & 0x80 != 0 {
        let n = 2usize << (f & 7);
        at += n * 3;
        table(data, at - n * 3, n)?
    } else {
        global.clone().unwrap_or_else(|| (0..=255u8).map(|v| [v; 3]).collect())
    };
    let min = *data.get(at).ok_or(Error::Truncated)?;
    if !(1..=11).contains(&min) { return Err(Error::Corrupt); }
    at += 1;
    // Gather the sub-blocks.
    let mut lzw = Vec::new();
    loop {
        let Some(&n) = data.get(at) else { break };
        if n == 0 { break; }
        let Some(b) = data.get(at + 1..at + 1 + n as usize) else {
            lzw.extend_from_slice(&data[at + 1..]);
            break;
        };
        lzw.extend_from_slice(b);
        at += 1 + n as usize;
    }
    let npix = w as usize * h as usize;
    let pixels = lzw_decode(&lzw, min, npix)?;
    let (cw, ch) = (sw.max(left + w), sh.max(top + h));
    let mut img = Image::alloc(cw, ch, 8)?;
    // The background: the global table's entry, found in (or added to)
    // the image's palette.
    let bgc = global.as_ref().and_then(|g| g.get(bg as usize).copied()).unwrap_or([0; 3]);
    let covers = left == 0 && top == 0 && w == cw && h == ch && transparent.is_none();
    let bgi = if covers { 0 } else {
        match pal.iter().position(|&c| c == bgc) {
            Some(i) if Some(i as u8) != transparent => i as u8,
            _ if pal.len() < 256 => { pal.push(bgc); (pal.len() - 1) as u8 }
            _ => 0,
        }
    };
    img.data.fill(bgi);
    img.palette = pal;
    // Interlaced rows come in four passes: every 8th from 0, every 8th
    // from 4, every 4th from 2, every 2nd from 1.
    let mut order = Vec::with_capacity(h as usize);
    if f & 0x40 != 0 {
        for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
            order.extend((start..h).step_by(step));
        }
    } else {
        order.extend(0..h);
    }
    for (n, &y) in order.iter().enumerate() {
        for x in 0..w {
            let Some(&v) = pixels.get(n * w as usize + x as usize) else { break };
            if Some(v) != transparent { img.set_index(left + x, top + y, v); }
        }
    }
    Ok(img)
}

fn skip_blocks(d: &[u8], mut at: usize) -> Result<usize, Error> {
    loop {
        let n = *d.get(at).ok_or(Error::Truncated)? as usize;
        at += 1 + n;
        if n == 0 { return Ok(at); }
    }
}

/// Decodes GIF LZW data to at most `max` indexes; a stream that stops
/// early gives what it has (zeros after it).
fn lzw_decode(src: &[u8], min: u8, max: usize) -> Result<Vec<u8>, Error> {
    let clear = 1u16 << min;
    let mut out = Vec::with_capacity(max);
    let mut prefix = [0u16; 4096];
    let mut suffix = [0u8; 4096];
    let mut first = [0u8; 4096];
    for i in 0..clear { suffix[i as usize] = i as u8; first[i as usize] = i as u8; }
    let mut size = min as u32 + 1;
    let mut next = clear + 2;
    let mut old: Option<u16> = None;
    let (mut acc, mut nbits, mut i) = (0u32, 0u32, 0usize);
    let mut stack = Vec::with_capacity(4096);
    while out.len() < max {
        while nbits < size {
            let Some(&b) = src.get(i) else { break };
            acc |= (b as u32) << nbits;
            nbits += 8;
            i += 1;
        }
        if nbits < size { break; }
        let code = (acc & ((1 << size) - 1)) as u16;
        acc >>= size;
        nbits -= size;
        if code == clear {
            size = min as u32 + 1;
            next = clear + 2;
            old = None;
            continue;
        }
        if code == clear + 1 { break; }
        let Some(o) = old else {
            if code >= clear { return Err(Error::Corrupt); }
            out.push(code as u8);
            old = Some(code);
            continue;
        };
        // The string for `code`; the one not yet in the table is the
        // previous string plus its own first byte.
        let (mut c, extra) = if code < next { (code, None) } else if code == next { (o, Some(first[o as usize])) } else { return Err(Error::Corrupt) };
        stack.clear();
        if let Some(e) = extra { stack.push(e); }
        loop {
            stack.push(suffix[c as usize]);
            if c < clear { break; }
            c = prefix[c as usize];
        }
        out.extend(stack.iter().rev().take(max - out.len()));
        if next < 4096 {
            prefix[next as usize] = o;
            suffix[next as usize] = *stack.last().unwrap_or(&0);
            first[next as usize] = first[o as usize];
            next += 1;
            if next == 1 << size && size < 12 { size += 1; }
        }
        old = Some(code);
    }
    if out.is_empty() && max > 0 { return Err(Error::Truncated); }
    out.resize(max, 0);
    Ok(out)
}

/// LZW-compresses indexes (each < 1 << `min`).
fn lzw_encode(px: &[u8], min: u8) -> Vec<u8> {
    const HS: usize = 8191;
    let clear = 1u32 << min;
    let mut out = Vec::new();
    let (mut acc, mut nbits) = (0u32, 0u32);
    let mut emit = |code: u32, size: u32, out: &mut Vec<u8>| {
        acc |= code << nbits;
        nbits += size;
        while nbits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            nbits -= 8;
        }
    };
    // Open-addressed table: key (prefix << 8 | byte) + 1 -> code.
    let mut keys = vec![0u32; HS];
    let mut codes = vec![0u16; HS];
    let mut size = min as u32 + 1;
    let mut next = clear + 2;
    emit(clear, size, &mut out);
    let mut cur: Option<u32> = None;
    for &b in px {
        let Some(p) = cur else { cur = Some(b as u32); continue };
        let key = (p << 8 | b as u32) + 1;
        let mut h = (key as usize * 2654435761usize) % HS;
        let mut found = None;
        while keys[h] != 0 {
            if keys[h] == key { found = Some(codes[h]); break; }
            h = (h + 1) % HS;
        }
        if let Some(c) = found {
            cur = Some(c as u32);
            continue;
        }
        emit(p, size, &mut out);
        if next == 4096 {
            emit(clear, size, &mut out);
            keys.fill(0);
            size = min as u32 + 1;
            next = clear + 2;
        } else {
            keys[h] = key;
            codes[h] = next as u16;
            next += 1;
            if next > 1 << size && size < 12 { size += 1; }
        }
        cur = Some(b as u32);
    }
    if let Some(p) = cur {
        emit(p, size, &mut out);
        // The decoder adds an entry after this code too, and may widen.
        if next == 1 << size && size < 12 { size += 1; }
    }
    emit(clear + 1, size, &mut out);
    if nbits > 0 { out.push(acc as u8); }
    out
}

/// Maps a true-colour image onto a 6×7×6 colour cube (252 colours).
fn quantize(img: &Image) -> Image {
    let mut o = Image::new(img.width, img.height, 8);
    for r in 0..6u32 {
        for g in 0..7u32 {
            for b in 0..6u32 {
                o.palette.push([(r * 255 / 5) as u8, (g * 255 / 6) as u8, (b * 255 / 5) as u8]);
            }
        }
    }
    for y in 0..img.height {
        for x in 0..img.width {
            let c = img.rgb(x, y);
            let (r, g, b) = ((c[0] as u32 * 5 + 127) / 255, (c[1] as u32 * 6 + 127) / 255, (c[2] as u32 * 5 + 127) / 255);
            o.set_index(x, y, (r * 42 + g * 6 + b) as u8);
        }
    }
    o
}

/// Encodes a GIF87a (or 89a: never needed here) with a global colour
/// table; true-colour images with more than 256 colours are quantized.
pub fn encode(img: &Image) -> Vec<u8> {
    let p = match palettize(img) { Some(p) => p, None => quantize(img) };
    let p = p.to_8();
    let used = p.palette.len().max(2);
    let mut bits = 1u8;
    while (1usize << bits) < used { bits += 1; }
    let mut o = b"GIF87a".to_vec();
    o.extend_from_slice(&(p.width as u16).to_le_bytes());
    o.extend_from_slice(&(p.height as u16).to_le_bytes());
    o.extend_from_slice(&[0x80 | 0x70 | (bits - 1), 0, 0]);
    for i in 0..1usize << bits {
        o.extend_from_slice(&p.palette.get(i).copied().unwrap_or([0; 3]));
    }
    o.push(0x2C);
    o.extend_from_slice(&[0, 0, 0, 0]);
    o.extend_from_slice(&(p.width as u16).to_le_bytes());
    o.extend_from_slice(&(p.height as u16).to_le_bytes());
    o.push(0);
    let min = bits.max(2);
    o.push(min);
    let mut px = Vec::with_capacity((p.width * p.height) as usize);
    for y in 0..p.height {
        px.extend_from_slice(&p.row(y)[..p.width as usize]);
    }
    for c in lzw_encode(&px, min).chunks(255) {
        o.push(c.len() as u8);
        o.extend_from_slice(c);
    }
    o.extend_from_slice(&[0, 0x3B]);
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    #[test]
    fn round_trips() {
        for bits in [1, 4, 8] {
            let i = pattern_pal(37, 13, bits);
            let e = encode(&i);
            assert_eq!(crate::identify(&e, e.len() as u64), Some(crate::Format::Gif));
            assert!(same_pixels(&i, &decode(&e).unwrap()), "{bits}");
        }
        // Big enough to fill the table and clear it.
        let mut i = Image::new(400, 300, 8);
        i.palette = (0..=255u8).map(|v| [v, v ^ 0x55, 255 - v]).collect();
        let mut s = 1u32;
        for y in 0..300 {
            for x in 0..400 {
                s = s.wrapping_mul(1103515245).wrapping_add(12345);
                i.set_index(x, y, if x % 7 == 0 { (s >> 16) as u8 } else { (x / 3 + y) as u8 });
            }
        }
        assert!(same_pixels(&i, &decode(&encode(&i)).unwrap()));
        // True colour: a few colours stay exact, many are quantized.
        let mut t = Image::new(5, 5, 24);
        t.set_rgb(2, 2, [1, 2, 3]);
        assert!(same_pixels(&t, &decode(&encode(&t)).unwrap()));
        let t = pattern24(64, 64);
        let (m, _) = max_diff(&t, &decode(&encode(&t)).unwrap());
        assert!(m <= 26, "{m}");
    }

    #[test]
    fn interlaced_and_offset() {
        // 1x5 image interlaced, at (1,1) on a 3x7 screen with background 2.
        let px = [0u8, 1, 1, 0, 1];
        let mut lzw_in = Vec::new();
        for y in [0usize, 4, 2, 1, 3] { lzw_in.push(px[y]); }
        let mut f = b"GIF89a".to_vec();
        f.extend_from_slice(&[3, 0, 7, 0, 0x81, 2, 0]);
        f.extend_from_slice(&[0, 0, 0, 255, 255, 255, 9, 9, 9, 0, 0, 0]);
        f.extend_from_slice(&[0x2C, 1, 0, 1, 0, 1, 0, 5, 0, 0x40, 2]);
        let l = lzw_encode(&lzw_in, 2);
        f.push(l.len() as u8);
        f.extend_from_slice(&l);
        f.extend_from_slice(&[0, 0x3B]);
        let d = decode(&f).unwrap();
        assert_eq!((d.width, d.height), (3, 7));
        assert_eq!(d.rgb(0, 0), [9, 9, 9]);
        for y in 0..5 { assert_eq!(d.rgb(1, 1 + y), if px[y as usize] == 1 { [255; 3] } else { [0; 3] }); }
    }

    #[test]
    fn truncated() {
        let e = encode(&pattern_pal(40, 40, 8));
        for n in 0..e.len() { let _ = decode(&e[..n]); }
        let mut g = e.clone();
        for k in (20..g.len()).step_by(7) { g[k] ^= 0x5A; let _ = decode(&g); }
    }

    #[test]
    fn sips_files() {
        // sips's GIF of one of IBM's JPEGs against sips's BMP of that GIF,
        // and our GIF read back by sips.
        let dir = scratch();
        for p in work_files("unpacked/disk_14/MMTEMP", "jpg").iter().take(2) {
            let g = dir.join("sips.gif");
            let b = dir.join("sips-gif.bmp");
            if !sips(p, "gif", &g) || !sips(&g, "bmp", &b) { return; }
            let d = decode(&std::fs::read(&g).unwrap()).unwrap();
            let want = crate::bmp::decode(&std::fs::read(&b).unwrap()).unwrap();
            assert!(same_pixels(&want, &d), "{}", p.display());
            let ours = dir.join("ours.gif");
            std::fs::write(&ours, encode(&d)).unwrap();
            let b2 = dir.join("ours-gif.bmp");
            assert!(sips(&ours, "bmp", &b2));
            assert!(same_pixels(&d, &crate::bmp::decode(&std::fs::read(&b2).unwrap()).unwrap()));
        }
    }

    #[test]
    fn real_files() {
        for p in work_files("media/boot/java118", "gif") {
            decode(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e:?}", p.display()));
        }
    }
}
