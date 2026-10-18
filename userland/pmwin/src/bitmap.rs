//! Bitmap resources for static controls (SS_BITMAP): the OS/2 bitmap
//! file format (an optional "BA" array header, then "BM" with a 1.x or
//! 2.x info header), uncompressed 1, 4, 8 and 24 bits per pixel.

use alloc::vec::Vec;

use gpi::Rect;

use crate::ctl;
use crate::draw;
use crate::res::{self, u16_at};

pub struct Bmp {
    pub w: i32,
    pub h: i32,
    /// Rows bottom-up.
    pub pixels: Vec<u32>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u16_at(b, at) as u32 | (u16_at(b, at + 2) as u32) << 16
}

pub fn decode(b: &[u8]) -> Option<Bmp> {
    let mut base = 0;
    if b.get(0..2)? == b"BA" {
        base = 14;
    }
    if b.get(base..base + 2)? != b"BM" {
        return None;
    }
    let off_bits = u32_at(b, base + 10) as usize;
    let info = base + 14;
    let cb_fix = u32_at(b, info) as usize;
    let (w, h, bits, pal_entry) = if cb_fix == 12 {
        (u16_at(b, info + 4) as usize, u16_at(b, info + 6) as usize, u16_at(b, info + 10), 3)
    } else {
        if cb_fix >= 20 && u32_at(b, info + 16) != 0 {
            return None; // compressed
        }
        (u32_at(b, info + 4) as usize, u32_at(b, info + 8) as usize, u16_at(b, info + 14), 4)
    };
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let colors = if bits <= 8 {
        let used = if cb_fix >= 36 { u32_at(b, info + 32) as usize } else { 0 };
        if used != 0 { used } else { 1 << bits }
    } else {
        0
    };
    let pal_at = info + cb_fix;
    let pal: Vec<u32> = (0..colors)
        .map(|i| {
            let o = pal_at + i * pal_entry;
            b.get(o..o + 3).map_or(0, |c| (c[2] as u32) << 16 | (c[1] as u32) << 8 | c[0] as u32)
        })
        .collect();
    let stride = (w * bits as usize).div_ceil(32) * 4;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = b.get(off_bits + y * stride..off_bits + (y + 1) * stride)?;
        for x in 0..w {
            let c = match bits {
                1 => pal.get(((row[x / 8] >> (7 - x % 8)) & 1) as usize).copied().unwrap_or(0),
                4 => pal.get(((row[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 0xf) as usize).copied().unwrap_or(0),
                8 => pal.get(row[x] as usize).copied().unwrap_or(0),
                24 => (row[x * 3 + 2] as u32) << 16 | (row[x * 3 + 1] as u32) << 8 | row[x * 3] as u32,
                _ => return None,
            };
            pixels.push(c);
        }
    }
    Some(Bmp { w: w as i32, h: h as i32, pixels })
}

/// The bitmap named by a static control's text: 0xFF and the resource id,
/// or "#id".
pub fn for_static(h: u32) -> Option<Bmp> {
    let t = ctl::text(h);
    let id = match t.first() {
        Some(0xff) if t.len() >= 3 => t[1] as u32 | (t[2] as u32) << 8,
        Some(b'#') => core::str::from_utf8(&t[1..]).ok()?.parse().ok()?,
        _ => return None,
    };
    decode(res::load(0, res::RT_BITMAP, id)?)
}

/// Draw a bitmap stretched to `w` x `h` pixels at the window origin.
pub fn blit(c: &mut draw::Canvas, b: &Bmp, w: i32, h: i32) {
    for y in 0..h {
        let sy = (y as i64 * b.h as i64 / h.max(1) as i64) as i32;
        let row = &b.pixels[(sy * b.w) as usize..((sy + 1) * b.w) as usize];
        let at = |x: i32| row[(x as i64 * b.w as i64 / w.max(1) as i64) as usize];
        let mut x = 0;
        while x < w {
            // Runs of one colour as one fill.
            let col = at(x);
            let mut e = x + 1;
            while e < w && at(e) == col {
                e += 1;
            }
            c.fill(Rect::new(x, y, e, y + 1), col);
            x = e;
        }
    }
}
