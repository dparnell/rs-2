//! TIFF files, Intel ("II") and Motorola ("MM") byte order.
//!
//! An 8-byte header (byte order, 42, offset of the first IFD), then image
//! file directories: a count and 12-byte entries (tag, type, count,
//! value or offset of the values). Only the first IFD is decoded. The
//! pixels are in strips (StripOffsets/StripByteCounts, RowsPerStrip) or
//! tiles (TileWidth/TileLength, TileOffsets/TileByteCounts), either
//! chunky (samples interleaved) or planar (one plane after another).
//!
//! Supported: compression 1 (none), 5 (LZW, with the horizontal
//! differencing predictor) and 32773 (PackBits); photometric 0/1 (white
//! or black is zero, 1/2/4/8/16 bits), 2 (RGB, 8 or 16 bits per sample,
//! extra samples ignored) and 3 (palette, 1/2/4/8 bits). CCITT
//! compressions, JPEG, CMYK and YCbCr are not supported.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Error, Image, be16, be32, le16, le32};

struct Ifd<'a> {
    d: &'a [u8],
    big: bool,
    at: usize,
    n: usize,
}

impl<'a> Ifd<'a> {
    fn u16(&self, at: usize) -> Option<u16> { if self.big { be16(self.d, at) } else { le16(self.d, at) } }
    fn u32(&self, at: usize) -> Option<u32> { if self.big { be32(self.d, at) } else { le32(self.d, at) } }

    fn open(d: &'a [u8]) -> Result<Ifd<'a>, Error> {
        let big = match d.get(..4) {
            Some(b"II*\0") => false,
            Some(b"MM\0*") => true,
            Some(_) => return Err(Error::NotThisFormat),
            None => return Err(Error::Truncated),
        };
        let mut i = Ifd { d, big, at: 0, n: 0 };
        i.at = i.u32(4).ok_or(Error::Truncated)? as usize;
        i.n = i.u16(i.at).ok_or(Error::Truncated)? as usize;
        if d.len() < i.at + 2 + i.n * 12 { return Err(Error::Truncated); }
        Ok(i)
    }

    /// The values of `tag` (integers of any width; rationals give their
    /// numerators).
    fn values(&self, tag: u16) -> Option<Vec<u32>> {
        for k in 0..self.n {
            let e = self.at + 2 + k * 12;
            if self.u16(e)? != tag { continue; }
            let (ty, count) = (self.u16(e + 2)?, self.u32(e + 4)? as usize);
            let size = match ty { 1 | 2 | 6 | 7 => 1, 3 | 8 => 2, 4 | 9 => 4, 5 | 10 => 8, _ => return None };
            if count > 1 << 24 { return None; }
            let at = if size * count <= 4 { e + 8 } else { self.u32(e + 8)? as usize };
            let mut v = Vec::with_capacity(count);
            for i in 0..count {
                let p = at + i * size;
                v.push(match size { 1 => *self.d.get(p)? as u32, 2 => self.u16(p)? as u32, _ => self.u32(p)? });
            }
            return Some(v);
        }
        None
    }

    fn get(&self, tag: u16) -> Option<u32> { self.values(tag)?.first().copied() }
}

/// The compression of the first IFD, when the header holds it.
pub fn compression(head: &[u8]) -> Option<u32> {
    let i = Ifd::open(head).ok()?;
    Some(i.get(259).unwrap_or(1))
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    let ifd = Ifd::open(data)?;
    let w = ifd.get(256).ok_or(Error::Corrupt)?;
    let h = ifd.get(257).ok_or(Error::Corrupt)?;
    let bps = ifd.values(258).unwrap_or(vec![1]);
    let bits = *bps.first().unwrap_or(&1);
    let comp = ifd.get(259).unwrap_or(1);
    let photo = ifd.get(262).ok_or(Error::Corrupt)?;
    let spp = ifd.get(277).unwrap_or(1).max(1);
    let planar = ifd.get(284).unwrap_or(1) == 2 && spp > 1;
    let predictor = ifd.get(317).unwrap_or(1);
    let fill_lsb = ifd.get(266) == Some(2);
    if !matches!(comp, 1 | 5 | 32773) { return Err(Error::Unsupported); }
    let ok = match photo {
        0 | 1 => matches!(bits, 1 | 2 | 4 | 8 | 16),
        2 => matches!(bits, 8 | 16) && spp >= 3,
        3 => matches!(bits, 1 | 2 | 4 | 8),
        _ => false,
    };
    if !ok || spp > 8 { return Err(Error::Unsupported); }
    if w == 0 || h == 0 || w > 0xFFFF || h > 0xFFFF { return Err(Error::Corrupt); }
    // Raw sample planes, rows top-down: one plane when chunky.
    let (nplanes, pspp) = if planar { (spp as usize, 1) } else { (1, spp as usize) };
    let bpr = (w as usize * pspp * bits as usize).div_ceil(8);
    let mut planes = vec![vec![0u8; bpr * h as usize]; nplanes];
    let tiled = ifd.get(322).is_some();
    let (tw, th, offs, counts) = if tiled {
        (ifd.get(322).unwrap_or(0), ifd.get(323).ok_or(Error::Corrupt)?, ifd.values(324).ok_or(Error::Corrupt)?, ifd.values(325).unwrap_or_default())
    } else {
        let rps = ifd.get(278).unwrap_or(h).clamp(1, h);
        (w, rps, ifd.values(273).ok_or(Error::Corrupt)?, ifd.values(279).unwrap_or_default())
    };
    if tw == 0 || th == 0 { return Err(Error::Corrupt); }
    let across = w.div_ceil(tw) as usize;
    let down = h.div_ceil(th) as usize;
    let tbpr = (tw as usize * pspp * bits as usize).div_ceil(8);
    let mut any = false;
    for p in 0..nplanes {
        for ty in 0..down {
            for tx in 0..across {
                let k = p * across * down + ty * across + tx;
                let Some(&off) = offs.get(k) else { continue };
                let off = off as usize;
                // A missing byte count: up to the next chunk or the end.
                let cnt = counts.get(k).map(|&c| c as usize).unwrap_or(data.len().saturating_sub(off));
                let Some(src) = data.get(off..off.saturating_add(cnt).min(data.len())) else { continue };
                if src.is_empty() { continue; }
                let rows = (th as usize).min(h as usize - ty * th as usize);
                let want = tbpr * if tiled { th as usize } else { rows };
                let mut buf = match comp {
                    1 => src[..want.min(src.len())].to_vec(),
                    5 => lzw_decode(src, want)?,
                    _ => packbits_decode(src, want),
                };
                buf.resize(want, 0);
                if fill_lsb { for b in &mut buf { *b = b.reverse_bits(); } }
                if predictor == 2 {
                    for r in buf.chunks_mut(tbpr) { undo_predictor(r, pspp, bits); }
                }
                // Copy into the plane, clipped at the right and bottom edges.
                let x0 = tx * (tw as usize * pspp * bits as usize) / 8;
                for r in 0..rows {
                    let y = ty * th as usize + r;
                    let n = tbpr.min(bpr - x0);
                    planes[p][y * bpr + x0..y * bpr + x0 + n].copy_from_slice(&buf[r * tbpr..r * tbpr + n]);
                }
                any = true;
            }
        }
    }
    if !any { return Err(Error::Truncated); }
    // Samples to pixels.
    let sample = |p: &[u8], i: usize| -> u32 {
        match bits {
            8 => p[i] as u32,
            16 => if ifd.big { p[2 * i] as u32 } else { p[2 * i + 1] as u32 },
            b => ((p[i * b as usize / 8] >> (8 - b as usize - (i * b as usize) % 8)) & ((1 << b) - 1)) as u32,
        }
    };
    let mut img;
    if photo == 2 {
        img = Image::alloc(w, h, 24)?;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let c = if planar {
                    [0, 1, 2].map(|s| sample(&planes[s][y * bpr..], x) as u8)
                } else {
                    [0, 1, 2].map(|s| sample(&planes[0][y * bpr..], x * pspp + s) as u8)
                };
                img.set_rgb(x as u32, y as u32, c);
            }
        }
    } else {
        let ob = if bits == 1 { 1 } else if bits <= 4 { 4 } else { 8 };
        img = Image::alloc(w, h, ob)?;
        if photo == 3 {
            let map = ifd.values(320).ok_or(Error::Corrupt)?;
            let n = 1usize << bits;
            if map.len() < 3 * n { return Err(Error::Corrupt); }
            img.palette = (0..n).map(|i| [(map[i] >> 8) as u8, (map[n + i] >> 8) as u8, (map[2 * n + i] >> 8) as u8]).collect();
        } else {
            let top = if bits == 16 { 255 } else { (1u32 << bits) - 1 };
            img.palette = (0..=top).map(|v| { let g = (v * 255 / top) as u8; [if photo == 0 { 255 - g } else { g }; 3] }).collect();
        }
        for y in 0..h as usize {
            for x in 0..w as usize {
                img.set_index(x as u32, y as u32, sample(&planes[0][y * bpr..], x * pspp) as u8);
            }
        }
    }
    Ok(img)
}

fn undo_predictor(row: &mut [u8], spp: usize, bits: u32) {
    match bits {
        8 => for i in spp..row.len() { row[i] = row[i].wrapping_add(row[i - spp]); },
        // 16-bit samples are big-endian in MM files and little in II;
        // only the high byte is used, so differencing per byte pair
        // would be wrong: leave them (rare).
        _ => {}
    }
}

fn packbits_decode(src: &[u8], max: usize) -> Vec<u8> {
    let mut o = Vec::with_capacity(max);
    let mut i = 0;
    while o.len() < max && i < src.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            let end = (i + len).min(src.len());
            o.extend_from_slice(&src[i..end]);
            i += len;
        } else if n != -128 {
            let Some(&v) = src.get(i) else { break };
            i += 1;
            o.extend(core::iter::repeat_n(v, (1 - n as isize) as usize));
        }
    }
    o.truncate(max);
    o
}

fn packbits_encode(src: &[u8], o: &mut Vec<u8>) {
    let mut i = 0;
    while i < src.len() {
        let mut run = 1;
        while i + run < src.len() && run < 128 && src[i + run] == src[i] { run += 1; }
        if run >= 2 {
            o.push((1i32 - run as i32) as u8);
            o.push(src[i]);
            i += run;
            continue;
        }
        let mut n = 1;
        while i + n < src.len() && n < 128 && !(i + n + 1 < src.len() && src[i + n] == src[i + n + 1]) { n += 1; }
        o.push((n - 1) as u8);
        o.extend_from_slice(&src[i..i + n]);
        i += n;
    }
}

/// TIFF LZW: codes most significant bit first, 256 = clear, 257 = end,
/// the width growing one code early ("early change").
fn lzw_decode(src: &[u8], max: usize) -> Result<Vec<u8>, Error> {
    // Old-style (pre-6.0, LSB-first) LZW starts without a clear code.
    if src.len() >= 2 && src[0] == 0 && src[1] & 1 != 0 { return Err(Error::Unsupported); }
    let mut out: Vec<u8> = Vec::with_capacity(max);
    // Each table entry is a (start, len) slice of the output: a new
    // entry is the string just output plus the next one's first byte,
    // which follows it in the output, so it is contiguous there too.
    let mut ent = vec![(0usize, 0usize); 4096];
    let mut size = 9u32;
    let mut next = 258usize;
    let mut prev: Option<(usize, usize)> = None;
    let (mut acc, mut nbits, mut i) = (0u32, 0u32, 0usize);
    while out.len() < max {
        while nbits < size {
            let Some(&b) = src.get(i) else { break };
            acc = acc << 8 | b as u32;
            nbits += 8;
            i += 1;
        }
        if nbits < size { break; }
        let code = ((acc >> (nbits - size)) & ((1 << size) - 1)) as usize;
        nbits -= size;
        acc &= (1 << nbits) - 1;
        if code == 256 {
            size = 9;
            next = 258;
            prev = None;
            continue;
        }
        if code == 257 { break; }
        let start = out.len();
        if code < 256 {
            out.push(code as u8);
        } else if code < next && prev.is_some() {
            let (s, l) = ent[code];
            out.extend_from_within(s..s + l);
        } else if code == next && let Some((ps, pl)) = prev {
            // The entry being defined: the previous string plus its own
            // first byte.
            out.extend_from_within(ps..ps + pl);
            out.push(out[ps]);
        } else {
            return Err(Error::Corrupt);
        }
        if let Some((ps, pl)) = prev && next < 4096 {
            ent[next] = (ps, pl + 1);
            next += 1;
            if next + 1 == 1 << size && size < 12 { size += 1; }
        }
        prev = Some((start, out.len() - start));
    }
    out.truncate(max);
    Ok(out)
}

fn lzw_encode(src: &[u8], o: &mut Vec<u8>) {
    const HS: usize = 8191;
    let (mut acc, mut nbits) = (0u64, 0u32);
    let mut emit = |code: u32, size: u32, o: &mut Vec<u8>| {
        acc = acc << size | code as u64;
        nbits += size;
        while nbits >= 8 {
            o.push((acc >> (nbits - 8)) as u8);
            nbits -= 8;
        }
    };
    let mut keys = vec![0u32; HS];
    let mut codes = vec![0u16; HS];
    let mut size = 9u32;
    let mut next = 258u32;
    emit(256, size, o);
    let mut cur: Option<u32> = None;
    for &b in src {
        let Some(p) = cur else { cur = Some(b as u32); continue };
        let key = (p << 8 | b as u32) + 1;
        let mut h = (key as usize * 2654435761usize) % HS;
        let mut found = None;
        while keys[h] != 0 {
            if keys[h] == key { found = Some(codes[h]); break; }
            h = (h + 1) % HS;
        }
        if let Some(c) = found { cur = Some(c as u32); continue; }
        emit(p, size, o);
        keys[h] = key;
        codes[h] = next as u16;
        next += 1;
        if next + 1 > 1 << size && size < 12 { size += 1; }
        if next == 4094 {
            emit(256, size, o);
            keys.fill(0);
            size = 9;
            next = 258;
        }
        cur = Some(b as u32);
    }
    if let Some(p) = cur {
        emit(p, size, o);
        next += 1;
        if next + 1 > 1 << size && size < 12 { size += 1; }
    }
    emit(257, size, o);
    if nbits > 0 { o.push((acc << (8 - nbits)) as u8); }
}

/// Encodes one IFD with the image in strips of about 8 KB: palette
/// images as photometric 3 (or 1 for a grey ramp), 24-bit as RGB.
/// `compressed` uses LZW (with the predictor for RGB and grey).
pub fn encode(img: &Image, big_endian: bool, compressed: bool) -> Vec<u8> {
    let tmp;
    let img = if matches!(img.bits, 1 | 4 | 8 | 24) { img } else { tmp = img.to_24(); &tmp };
    let rgb = img.bits == 24;
    let n = 1usize << if rgb { 0 } else { img.bits };
    let ramp = !rgb && (0..n).all(|i| img.palette.get(i).copied() == Some([(i * 255 / (n - 1)) as u8; 3]));
    let photo = if rgb { 2 } else if ramp { 1 } else { 3 };
    let spp = if rgb { 3 } else { 1 };
    let bits = if rgb { 8 } else { img.bits as usize };
    let bpr = (img.width as usize * spp * bits).div_ceil(8);
    let rps = (8192 / bpr).clamp(1, img.height as usize);
    let predictor = compressed && (rgb || (ramp && bits == 8));
    let u16b = |v: u16| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let u32b = |v: u32| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let mut o = Vec::new();
    o.extend_from_slice(if big_endian { b"MM\0*" } else { b"II*\0" });
    o.extend_from_slice(&u32b(0));
    // Strips first.
    let mut offs = Vec::new();
    let mut counts = Vec::new();
    let mut row = vec![0u8; bpr];
    for y0 in (0..img.height as usize).step_by(rps) {
        let mut strip = Vec::new();
        for y in y0..(y0 + rps).min(img.height as usize) {
            let r = img.row(y as u32);
            if rgb {
                for x in 0..img.width as usize { row[x * 3] = r[x * 3 + 2]; row[x * 3 + 1] = r[x * 3 + 1]; row[x * 3 + 2] = r[x * 3]; }
            } else {
                row.copy_from_slice(&r[..bpr]);
            }
            if predictor {
                for i in (spp..bpr).rev() { row[i] = row[i].wrapping_sub(row[i - spp]); }
            }
            strip.extend_from_slice(&row);
        }
        offs.push(o.len() as u32);
        let start = o.len();
        if compressed { lzw_encode(&strip, &mut o) } else { o.extend_from_slice(&strip) }
        counts.push((o.len() - start) as u32);
        if o.len() % 2 == 1 { o.push(0); }
    }
    // Out-of-line values.
    let extra = |o: &mut Vec<u8>, bytes: Vec<u8>| -> u32 {
        if o.len() % 2 == 1 { o.push(0); }
        let at = o.len() as u32;
        o.extend_from_slice(&bytes);
        at
    };
    let arr32 = |v: &[u32]| v.iter().flat_map(|&x| u32b(x)).collect::<Vec<u8>>();
    let offs_at = if offs.len() > 1 { extra(&mut o, arr32(&offs)) } else { offs[0] };
    let counts_at = if counts.len() > 1 { extra(&mut o, arr32(&counts)) } else { counts[0] };
    let bps_at = extra(&mut o, [8u16; 3].iter().flat_map(|&x| u16b(x)).collect());
    let res_at = extra(&mut o, arr32(&[72, 1]));
    let map_at = if photo == 3 {
        let mut m = Vec::new();
        for k in 0..3 { for i in 0..n { m.extend_from_slice(&u16b(img.palette.get(i).map_or(0, |c| c[k] as u16 * 257))); } }
        extra(&mut o, m)
    } else { 0 };
    let ifd_at = extra(&mut o, Vec::new());
    o[4..8].copy_from_slice(&u32b(ifd_at));
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = Vec::new();
    let short = |v: u16| { let mut b = [0; 4]; b[..2].copy_from_slice(&u16b(v)); b };
    entries.push((256, 4, 1, u32b(img.width)));
    entries.push((257, 4, 1, u32b(img.height)));
    entries.push((258, 3, spp as u32, if rgb { u32b(bps_at) } else { short(bits as u16) }));
    entries.push((259, 3, 1, short(if compressed { 5 } else { 1 })));
    entries.push((262, 3, 1, short(photo)));
    entries.push((273, 4, offs.len() as u32, u32b(offs_at)));
    entries.push((277, 3, 1, short(spp as u16)));
    entries.push((278, 4, 1, u32b(rps as u32)));
    entries.push((279, 4, counts.len() as u32, u32b(counts_at)));
    entries.push((282, 5, 1, u32b(res_at)));
    entries.push((283, 5, 1, u32b(res_at)));
    entries.push((284, 3, 1, short(1)));
    entries.push((296, 3, 1, short(2)));
    if predictor { entries.push((317, 3, 1, short(2))); }
    if photo == 3 { entries.push((320, 3, 3 * n as u32, u32b(map_at))); }
    o.extend_from_slice(&u16b(entries.len() as u16));
    for (tag, ty, count, v) in entries {
        o.extend_from_slice(&u16b(tag));
        o.extend_from_slice(&u16b(ty));
        o.extend_from_slice(&u32b(count));
        o.extend_from_slice(&v);
    }
    o.extend_from_slice(&[0; 4]);
    let _ = packbits_encode;
    o
}

/// Encodes with PackBits instead of LZW (both are "compressed").
pub fn encode_packbits(img: &Image, big_endian: bool) -> Vec<u8> {
    // Re-encode the uncompressed strips with PackBits and patch the IFD.
    let plain = encode(img, big_endian, false);
    let Ok(ifd) = Ifd::open(&plain) else { return plain };
    let (Some(offs), Some(counts)) = (ifd.values(273), ifd.values(279)) else { return plain };
    let mut strips = Vec::new();
    for (&o, &c) in offs.iter().zip(&counts) {
        let mut s = Vec::new();
        packbits_encode(&plain[o as usize..(o + c) as usize], &mut s);
        strips.push(s);
    }
    // Lay the file out again: header, strips, then the old tail (values
    // and IFD) moved after them with its offsets shifted.
    let first = offs[0] as usize;
    let tail_at = offs.iter().zip(&counts).map(|(&o, &c)| (o + c) as usize).max().unwrap_or(first);
    let tail_at = tail_at + (tail_at & 1);
    let mut o = plain[..first].to_vec();
    let mut new_offs = Vec::new();
    for s in &strips {
        new_offs.push(o.len() as u32);
        o.extend_from_slice(s);
        if o.len() % 2 == 1 { o.push(0); }
    }
    let shift = o.len() as i64 - tail_at as i64;
    let base = o.len();
    o.extend_from_slice(&plain[tail_at..]);
    let w32 = |o: &mut Vec<u8>, at: usize, v: u32| o[at..at + 4].copy_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
    let r32 = |o: &[u8], at: usize| if big_endian { be32(o, at).unwrap_or(0) } else { le32(o, at).unwrap_or(0) };
    let r16 = |o: &[u8], at: usize| if big_endian { be16(o, at).unwrap_or(0) } else { le16(o, at).unwrap_or(0) };
    let ifd_at = (r32(&o, 4) as i64 + shift) as usize;
    w32(&mut o, 4, ifd_at as u32);
    let n = r16(&o, ifd_at) as usize;
    let _ = base;
    for k in 0..n {
        let e = ifd_at + 2 + k * 12;
        let (tag, ty, count) = (r16(&o, e), r16(&o, e + 2), r32(&o, e + 4));
        let size = match ty { 3 => 2, 4 => 4, 5 => 8, _ => 1 } * count as usize;
        if tag == 259 { let v = if big_endian { 32773u16.to_be_bytes() } else { 32773u16.to_le_bytes() }; o[e + 8..e + 10].copy_from_slice(&v); continue; }
        if size > 4 { let v = (r32(&o, e + 8) as i64 + shift) as u32; w32(&mut o, e + 8, v); }
        if tag == 273 || tag == 279 {
            let vals: Vec<u32> = if tag == 273 { new_offs.clone() } else { strips.iter().map(|s| s.len() as u32).collect() };
            if vals.len() == 1 { w32(&mut o, e + 8, vals[0]); } else {
                let at = r32(&o, e + 8) as usize;
                for (i, v) in vals.iter().enumerate() { w32(&mut o, at + i * 4, *v); }
            }
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
        for big in [false, true] {
            for c in [false, true] {
                let i = pattern24(45, 70);
                let e = encode(&i, big, c);
                let want = if big { crate::Format::TiffMotorola { compressed: c } } else { crate::Format::TiffIntel { compressed: c } };
                assert_eq!(crate::identify(&e[..64.min(e.len())], e.len() as u64).map(|_| ()), Some(()));
                assert_eq!(crate::identify(&e, e.len() as u64), Some(want));
                assert_eq!(decode(&e).unwrap(), i, "{big} {c}");
                for bits in [1, 4, 8] {
                    let i = pattern_pal(29, 300, bits);
                    assert!(same_pixels(&i, &decode(&encode(&i, big, c)).unwrap()), "{big} {c} {bits}");
                }
                let mut g = Image::new(300, 40, 8);
                g.palette = (0..=255u8).map(|v| [v; 3]).collect();
                for y in 0..40 { for x in 0..300 { g.set_index(x, y, (x ^ y) as u8); } }
                assert_eq!(decode(&encode(&g, big, c)).unwrap(), g);
            }
            let i = pattern24(45, 70);
            assert_eq!(decode(&encode_packbits(&i, big)).unwrap(), i);
            let i = pattern_pal(29, 300, 4);
            assert!(same_pixels(&i, &decode(&encode_packbits(&i, big)).unwrap()));
        }
    }

    #[test]
    fn lzw_long() {
        // Enough distinct data to clear the table several times.
        let mut s = 7u32;
        let data: Vec<u8> = (0..200_000).map(|i| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); if i % 3 == 0 { (s >> 24) as u8 } else { (i / 50) as u8 } }).collect();
        let mut e = Vec::new();
        lzw_encode(&data, &mut e);
        assert_eq!(lzw_decode(&e, data.len()).unwrap(), data);
        let mut p = Vec::new();
        packbits_encode(&data, &mut p);
        assert_eq!(packbits_decode(&p, data.len()), data);
    }

    #[test]
    fn truncated() {
        let e = encode(&pattern24(20, 20), false, true);
        for n in 0..e.len() { let _ = decode(&e[..n]); }
        let mut g = e.clone();
        for k in (8..g.len()).step_by(5) { g[k] ^= 0xA5; let _ = decode(&g); }
    }
}
