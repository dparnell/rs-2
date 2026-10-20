//! Bitmaps (`:artwork.`), stored back to back from the header's bitmap
//! offset; text refers to them by offset from there.
//!
//! A record is an optional bitmap array header (`BA`, 14 bytes: type,
//! size, offset of the next entry (0: last), display width and height),
//! a bitmap file header (14 bytes: `bM` for the help compiler's compressed
//! bitmaps), an OS/2 1.x (12 bytes) or 2.x (64 bytes) info header with
//! its palette (3 or 4 bytes per colour; none at 16 bits or more), then
//! `u32` size of the rest, `u16` bytes per uncompressed block, and the
//! blocks: `u16` size and data whose first byte names the algorithm: 0
//! stored, 1 run lengths, 2 LZW. Rows are bottom-up, 32-bit aligned, as
//! GpiSetBitmapBits takes them.

use crate::{Error, Rd};
use alloc::vec;
use alloc::vec::Vec;

/// A decoded bitmap.
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub cx: u32,
    pub cy: u32,
    pub bpp: u16,
    /// Palette entries as 0xRRGGBB.
    pub palette: Vec<u32>,
    /// Rows bottom-up, each `row_bytes()` long.
    pub bits: Vec<u8>,
}

impl Bitmap {
    pub fn row_bytes(&self) -> usize {
        row_bytes(self.cx, self.bpp)
    }

    /// The colour of pixel (x, y), y counted from the bottom, as 0xRRGGBB.
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        let row = &self.bits[y as usize * self.row_bytes()..];
        let x = x as usize;
        let index = match self.bpp {
            1 => (row[x / 8] >> (7 - x % 8)) & 1,
            4 => (row[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 0xf,
            8 => row[x],
            24 => {
                let p = &row[x * 3..];
                return (p[2] as u32) << 16 | (p[1] as u32) << 8 | p[0] as u32;
            }
            _ => 0,
        };
        self.palette.get(index as usize).copied().unwrap_or(0)
    }
}

fn row_bytes(cx: u32, bpp: u16) -> usize {
    (bpp as usize * cx as usize).div_ceil(32) * 4
}

/// Size and colour depth of a stored bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    pub cx: u32,
    pub cy: u32,
    pub bpp: u16,
}

/// Where the bitmap file header of the record at `at` is: the first
/// entry of a bitmap array matching `display` (width, height), else the
/// first.
fn select(r: Rd<'_>, at: usize, display: (u16, u16)) -> Result<usize, Error> {
    if r.bytes(at, 2)? != b"BA" {
        return Ok(at);
    }
    let first = at + 14;
    let mut e = at;
    loop {
        let next = r.u32(e + 6)?;
        let (cx, cy) = (r.u16(e + 10)?, r.u16(e + 12)?);
        if cx == display.0 || cy == display.1 {
            return Ok(e + 14);
        }
        if next == 0 {
            return Ok(first);
        }
        // Entries follow each other: header, info, palette, data.
        let (_, data) = header(r, e + 14)?;
        e = data + 4 + r.u32(data)? as usize;
        if r.bytes(e, 2)? != b"BA" {
            return Ok(first);
        }
    }
}

/// The info at bitmap file header `fh` and the offset of its data size.
fn header(r: Rd<'_>, fh: usize) -> Result<(Info, usize), Error> {
    let cb = r.u32(fh + 14)?;
    let (info, entry, hdr) = if cb == 12 {
        (Info { cx: r.u16(fh + 18)? as u32, cy: r.u16(fh + 20)? as u32, bpp: r.u16(fh + 24)? }, 3, 0x1a)
    } else if cb >= 16 {
        (Info { cx: r.u32(fh + 18)?, cy: r.u32(fh + 22)?, bpp: r.u16(fh + 28)? }, 4, 14 + cb as usize)
    } else {
        return Err(Error::Invalid("bitmap header"));
    };
    if !matches!(info.bpp, 1 | 4 | 8 | 24) || info.cx == 0 || info.cy == 0 || info.cx > 8192 || info.cy > 8192 {
        return Err(Error::Invalid("bitmap size"));
    }
    let colours = if info.bpp >= 16 { 0 } else { 1usize << info.bpp };
    Ok((info, fh + hdr + colours * entry))
}

/// Size of the bitmap at `offset` (from the header's bitmap offset).
pub fn info(data: &[u8], image_start: u32, offset: u32) -> Result<Info, Error> {
    let r = Rd(data);
    let fh = select(r, image_start as usize + offset as usize, (0, 0))?;
    Ok(header(r, fh)?.0)
}

/// Decode the bitmap at `offset` (from the header's bitmap offset); of a
/// bitmap array, the entry for a `display` of that size.
pub fn decode(data: &[u8], image_start: u32, offset: u32, display: (u16, u16)) -> Result<Bitmap, Error> {
    let r = Rd(data);
    let fh = select(r, image_start as usize + offset as usize, display)?;
    let kind = r.bytes(fh, 2)?;
    let (info, size_at) = header(r, fh)?;
    let cb = r.u32(fh + 14)?;
    let pal_at = fh + 14 + cb as usize;
    let mut palette = Vec::new();
    if info.bpp < 16 {
        let entry = if cb == 12 { 3 } else { 4 };
        for i in 0..1usize << info.bpp {
            let p = r.bytes(pal_at + i * entry, 3)?;
            palette.push((p[2] as u32) << 16 | (p[1] as u32) << 8 | p[0] as u32);
        }
    }
    let rb = row_bytes(info.cx, info.bpp);
    let total = rb * info.cy as usize;
    let mut bits = vec![0u8; total];
    let size = r.u32(size_at)? as usize;
    let mut p = size_at + 4;
    match kind {
        b"bM" => {
            let block = r.u16(p)? as usize;
            p += 2;
            if block == 0 {
                return Err(Error::Invalid("bitmap block size"));
            }
            let mut done = 0;
            while done < total {
                let chunk = block.min(total - done);
                let n = r.u16(p)? as usize;
                p += 2;
                let out = &mut bits[done..done + chunk];
                if n == 0 {
                    out.copy_from_slice(r.bytes(p, chunk)?);
                    p += chunk;
                } else {
                    let src = r.bytes(p, n)?;
                    p += n;
                    match src[0] {
                        0 => {
                            let m = (n - 1).min(chunk);
                            out[..m].copy_from_slice(&src[1..1 + m]);
                        }
                        1 => {
                            rle(&src[1..], out);
                        }
                        2 => {
                            lzw(&src[1..], out);
                        }
                        _ => return Err(Error::Invalid("bitmap compression")),
                    }
                }
                done += chunk;
            }
        }
        // Old-style bitmaps are stored as they are (the compressed forms
        // of the old compiler are not on the media).
        b"BM" if size == total => bits.copy_from_slice(r.bytes(p, total)?),
        _ => return Err(Error::Invalid("bitmap type")),
    }
    Ok(Bitmap { cx: info.cx, cy: info.cy, bpp: info.bpp, palette, bits })
}

/// Algorithm 1 (run lengths): `c < 0x80`: the next byte c+1 times; `0x81
/// ..`: (c & 0x7F) + 1 bytes as they are; `0x80 0|1 u16 n`: 0x00 or 0xFF
/// n+1 times; `0x80 v a b`: the pair a, b v+1 times. Returns the bytes
/// written (at most `out.len()`).
pub fn rle(src: &[u8], out: &mut [u8]) -> usize {
    let (mut p, mut o) = (0, 0);
    let mut put = |o: &mut usize, b: u8| {
        if *o < out.len() {
            out[*o] = b;
            *o += 1;
        }
    };
    while p + 1 < src.len() || (p < src.len() && src[p] > 0x80) {
        let c = src[p];
        if c < 0x80 {
            for _ in 0..=c {
                put(&mut o, src[p + 1]);
            }
            p += 2;
        } else if c == 0x80 {
            let v = src[p + 1];
            let (a, b) = (src.get(p + 2).copied().unwrap_or(0), src.get(p + 3).copied().unwrap_or(0));
            if v <= 1 {
                let fill = if v == 1 { 0xff } else { 0 };
                for _ in 0..=u16::from_le_bytes([a, b]) {
                    put(&mut o, fill);
                }
            } else {
                for _ in 0..=v {
                    put(&mut o, a);
                    put(&mut o, b);
                }
            }
            p += 4;
        } else {
            let n = (c & 0x7f) as usize + 1;
            for k in 0..n {
                put(&mut o, src.get(p + 1 + k).copied().unwrap_or(0));
            }
            p += 1 + n;
        }
    }
    o
}

/// Algorithm 2: LZW, codes MSB first, 9 to 12 bits, 256 clear, 257 end,
/// the width growing as the next code reaches 511, 1023 and 2047. The
/// dictionary holds (offset, length) of earlier output. Returns the bytes
/// written.
pub fn lzw(src: &[u8], out: &mut [u8]) -> usize {
    const CLEAR: u16 = 256;
    const END: u16 = 257;
    let mut pos = [0u32; 4096];
    let mut len = [0u16; 4096];
    let (mut acc, mut nbits, mut p) = (0u32, 0u32, 0usize);
    let mut width = 9u32;
    let mut next = 258u16;
    let mut first = true;
    let mut prev = 0u16;
    let mut o = 0usize;
    loop {
        while nbits < 25 {
            acc |= (src.get(p).copied().unwrap_or(0) as u32) << (24 - nbits);
            p += 1;
            nbits += 8;
        }
        if p > src.len() + 4 {
            break;
        }
        let code = (acc >> (32 - width)) as u16;
        acc <<= width;
        nbits -= width;
        if code == END {
            break;
        }
        if code == CLEAR {
            first = true;
            width = 9;
            next = 258;
            continue;
        }
        let start = o;
        if code < 256 {
            if o >= out.len() {
                break;
            }
            out[o] = code as u8;
            o += 1;
        } else if code < next && !first {
            let (s, n) = (pos[code as usize] as usize, len[code as usize] as usize);
            if o + n > out.len() || s + n > start {
                break;
            }
            out.copy_within(s..s + n, o);
            o += n;
        } else if !first && code == next {
            // The previous string and its own first byte.
            let (s, n) = (pos[prev as usize] as usize, len[prev as usize] as usize);
            if o + n + 1 > out.len() {
                break;
            }
            out.copy_within(s..s + n, o);
            out[o + n] = out[s];
            o += n + 1;
        } else {
            break;
        }
        if code < 256 {
            pos[code as usize] = start as u32;
            len[code as usize] = 1;
        }
        if first {
            first = false;
        } else if (next as usize) < 4096 {
            // The new entry: the previous string and this one's first byte,
            // which follow each other in the output.
            pos[next as usize] = pos[prev as usize];
            len[next as usize] = len[prev as usize] + 1;
            next += 1;
            if next as u32 == (1 << width) - 1 && width < 12 {
                width += 1;
            }
        }
        // The previous string as it was just written (a literal or a copy).
        let n = o - start;
        if code >= 256 {
            pos[code as usize] = start as u32;
            len[code as usize] = n as u16;
        }
        prev = code;
    }
    o
}
