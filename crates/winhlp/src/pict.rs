//! Pictures: the `bmN` files and pictures embedded in topics (SHG and
//! MRB: segmented hypergraphics and multi-resolution bitmaps), decoded
//! into Windows `.BMP` files, with their hotspots.

use crate::hfs::{cstr, lz77, rle, u16_at, u32_at, Cur};
use alloc::vec;
use alloc::vec::Vec;

/// A hotspot of a picture: its action byte (0xE2 popup, 0xE3 jump,
/// 0xE6/0xE7 the same without highlight, 0xC8/0xCC macro), its
/// rectangle in pixels from the top left, and its context hash and
/// context string or macro.
#[derive(Debug, Clone)]
pub struct Hotspot {
    pub kind: u8,
    pub x: u16,
    pub y: u16,
    pub cx: u16,
    pub cy: u16,
    pub hash: u32,
    pub target: Vec<u8>,
}

/// A decoded picture.
pub struct Picture {
    /// A Windows bitmap file.
    pub bmp: Vec<u8>,
    pub cx: u32,
    pub cy: u32,
    pub hotspots: Vec<Hotspot>,
}

/// The default palettes of device dependent bitmaps.
const VGA: [u32; 16] = [
    0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0, 0x808080, 0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
];

/// The best picture of a picture file (`lp`/`lP` magic, u16 count, u32
/// offsets): the one with the most colours, then the largest.
pub fn decode(file: &[u8]) -> Option<Picture> {
    let magic = u16_at(file, 0)?;
    if magic != 0x506c && magic != 0x706c {
        return None;
    }
    let n = u16_at(file, 2)? as usize;
    let mut best: Option<Picture> = None;
    let mut best_key = (0u16, 0u64);
    for i in 0..n.min(16) {
        let o = u32_at(file, 4 + 4 * i)? as usize;
        let Some(p) = file.get(o..) else { continue };
        let Some((pic, bpp)) = picture(p) else { continue };
        let key = (bpp, pic.cx as u64 * pic.cy as u64);
        if best.is_none() || key > best_key {
            best_key = key;
            best = Some(pic);
        }
    }
    best
}

/// Unpack picture data: 0 stored, 1 run lengths, 2 LZ77, 3 both.
fn unpack(data: &[u8], method: u8, size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    match method {
        0 => out.extend_from_slice(data),
        1 => rle(data, &mut out, size),
        2 => lz77(data, &mut out, size),
        3 => {
            let mut t = Vec::new();
            lz77(data, &mut t, usize::MAX);
            rle(&t, &mut out, size);
        }
        _ => return None,
    }
    Some(out)
}

/// One picture: u8 type (5 device dependent bitmap, 6 device
/// independent, 8 metafile), u8 packing, then for bitmaps compressed
/// x and y resolution, planes, bit count, width, height, colours used,
/// colours important, data size, hotspot size, u32 data offset, u32
/// hotspot offset (from the picture), and for type 6 the palette.
/// Returns the picture and its bits per pixel.
fn picture(p: &[u8]) -> Option<(Picture, u16)> {
    let ty = *p.first()?;
    let method = *p.get(1)?;
    if ty != 5 && ty != 6 {
        return None;
    }
    let mut c = Cur::at(p, 2);
    c.cdword()?;
    c.cdword()?;
    let planes = c.cword()?;
    let bits = c.cword()?;
    let cx = c.cdword()?;
    let cy = c.cdword()?;
    let used = c.cdword()?;
    c.cdword()?;
    let csize = c.cdword()? as usize;
    let hsize = c.cdword()? as usize;
    let coff = c.u32()? as usize;
    let hoff = c.u32()? as usize;
    if cx == 0 || cy == 0 || cx > 8192 || cy > 8192 {
        return None;
    }
    let bpp = bits * planes.max(1);
    // The palette as 0xRRGGBB.
    let mut pal: Vec<u32> = Vec::new();
    if ty == 6 && bits <= 8 {
        let n = if used == 0 { 1usize << bits } else { (used as usize).min(256) };
        for _ in 0..n {
            let q = c.bytes(4)?;
            pal.push((q[2] as u32) << 16 | (q[1] as u32) << 8 | q[0] as u32);
        }
    } else if bpp == 1 {
        pal = vec![0x000000, 0xffffff];
    } else if bpp == 4 {
        pal = VGA.to_vec();
    } else if bpp == 8 {
        // A device dependent 8 bit bitmap: grey levels.
        pal = (0..256u32).map(|i| i << 16 | i << 8 | i).collect();
    }
    let data = p.get(coff..coff.checked_add(csize)?.min(p.len()))?;
    // Device independent rows are 32-bit aligned, device dependent ones
    // 16-bit aligned and top-down, planes after each other per row.
    let (dib_row, out_bpp) = (row_bytes(cx, bpp), if matches!(bpp, 1 | 4 | 8 | 24) { bpp } else { 24 });
    let ddb_row = ((cx as usize * bits as usize + 15) / 16) * 2;
    let need = if ty == 6 { dib_row * cy as usize } else { ddb_row * planes.max(1) as usize * cy as usize };
    let mut raw = unpack(data, method, need)?;
    raw.resize(need, 0);
    let out_row = row_bytes(cx, out_bpp);
    let mut bitsv = vec![0u8; out_row * cy as usize];
    if ty == 6 {
        if out_bpp == bpp {
            bitsv.copy_from_slice(&raw[..out_row * cy as usize]);
        } else {
            // 16 or 32 bits: to 24.
            for y in 0..cy as usize {
                for x in 0..cx as usize {
                    let rgb = match bpp {
                        16 => {
                            let v = u16_at(&raw, y * dib_row + 2 * x).unwrap_or(0) as u32;
                            ((v >> 10 & 31) << 19) | ((v >> 5 & 31) << 11) | ((v & 31) << 3)
                        }
                        32 => u32_at(&raw, y * dib_row + 4 * x).unwrap_or(0) & 0xffffff,
                        _ => 0,
                    };
                    let o = y * out_row + 3 * x;
                    bitsv[o] = rgb as u8;
                    bitsv[o + 1] = (rgb >> 8) as u8;
                    bitsv[o + 2] = (rgb >> 16) as u8;
                }
            }
        }
    } else {
        // Device dependent: rows top-down; several planes combine into
        // the pixel's bits (plane 0 the lowest).
        let np = planes.max(1) as usize;
        for y in 0..cy as usize {
            let dst = (cy as usize - 1 - y) * out_row;
            if np == 1 {
                let s = y * ddb_row;
                let n = out_row.min(ddb_row);
                bitsv[dst..dst + n].copy_from_slice(&raw[s..s + n]);
            } else if out_bpp == 4 && bits == 1 && np <= 4 {
                for x in 0..cx as usize {
                    let mut v = 0u8;
                    for pl in 0..np {
                        let b = raw[(y * np + pl) * ddb_row + x / 8];
                        if b & (0x80 >> (x % 8)) != 0 {
                            v |= 1 << pl;
                        }
                    }
                    let o = dst + x / 2;
                    bitsv[o] |= if x % 2 == 0 { v << 4 } else { v };
                }
            }
        }
    }
    let hotspots = if hsize > 0 { p.get(hoff..hoff + hsize).map(hotspots).unwrap_or_default() } else { Vec::new() };
    let bmp = bmp_file(cx, cy, out_bpp, &pal, &bitsv);
    Some((Picture { bmp, cx, cy, hotspots }, out_bpp))
}

fn row_bytes(cx: u32, bpp: u16) -> usize {
    ((cx as usize * bpp as usize + 31) / 32) * 4
}

/// A Windows bitmap file: BITMAPFILEHEADER, BITMAPINFOHEADER, palette,
/// bottom-up rows.
pub fn bmp_file(cx: u32, cy: u32, bpp: u16, pal: &[u32], bits: &[u8]) -> Vec<u8> {
    let colours = if bpp <= 8 { 1usize << bpp } else { 0 };
    let off = 14 + 40 + colours * 4;
    let mut v = Vec::with_capacity(off + bits.len());
    v.extend_from_slice(b"BM");
    v.extend_from_slice(&((off + bits.len()) as u32).to_le_bytes());
    v.extend_from_slice(&[0; 4]);
    v.extend_from_slice(&(off as u32).to_le_bytes());
    v.extend_from_slice(&40u32.to_le_bytes());
    v.extend_from_slice(&cx.to_le_bytes());
    v.extend_from_slice(&cy.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&bpp.to_le_bytes());
    v.extend_from_slice(&[0; 4]);
    v.extend_from_slice(&(bits.len() as u32).to_le_bytes());
    v.extend_from_slice(&[0; 8]);
    v.extend_from_slice(&(colours as u32).to_le_bytes());
    v.extend_from_slice(&[0; 4]);
    for i in 0..colours {
        let c = pal.get(i).copied().unwrap_or(0);
        v.extend_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8, 0]);
    }
    v.extend_from_slice(bits);
    v
}

/// Hotspot data: u8 1, u16 count, u32 size of the macro data, the
/// hotspots (u8 kind, u8, u8, u16 x, y, cx, cy, u32 hash or macro
/// offset), the macro data, then each hotspot's name and context string
/// (or macro), zero-terminated.
fn hotspots(d: &[u8]) -> Vec<Hotspot> {
    let mut v = Vec::new();
    let (Some(n), Some(msize)) = (u16_at(d, 1), u32_at(d, 3)) else { return v };
    let n = n as usize;
    let mut strs = 7 + 15 * n + msize as usize;
    for i in 0..n {
        let o = 7 + 15 * i;
        let Some(r) = d.get(o..o + 15) else { break };
        let (_, e) = cstr(d, strs);
        let (target, e) = cstr(d, e);
        strs = e;
        v.push(Hotspot {
            kind: r[0],
            x: u16_at(r, 3).unwrap_or(0),
            y: u16_at(r, 5).unwrap_or(0),
            cx: u16_at(r, 7).unwrap_or(0),
            cy: u16_at(r, 9).unwrap_or(0),
            hash: u32_at(r, 11).unwrap_or(0),
            target: target.to_vec(),
        });
    }
    v
}
