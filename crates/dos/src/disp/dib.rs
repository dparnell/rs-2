//! DEVICEBITMAPBITS and DIBSCREENBLT (SetDIBitsToDevice): device
//! independent bitmaps (1, 4, 8 and 24 bits, bottom row first) to and
//! from the device's formats.

use alloc::vec;
use alloc::vec::Vec;

use super::{cref_of, mono_of, nearest, Args, Dev, Rect, PALETTE};
use crate::Machine;

/// The 16 colours of a 4-bit DIB the driver writes.
const VGA16: [u32; 16] = [0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0, 0x808080, 0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff];

struct DibInfo {
    w: i32,
    h: i32,
    bpp: u16,
    compression: u32,
    /// Where the colour table is, its entry size (4, or 3 for the core
    /// header) and count.
    colours: u32,
    entry: u32,
    ncolours: usize,
}

impl DibInfo {
    fn stride(&self) -> usize {
        ((self.w as usize * self.bpp as usize + 31) / 32) * 4
    }
}

impl Machine {
    fn dib_info(&mut self, l: u32) -> Option<DibInfo> {
        let size = self.hw.rd32(l);
        let (w, h, bpp, compression, entry, used) = if size == 12 {
            (self.hw.rd16(l + 4) as i32, self.hw.rd16(l + 6) as i32, self.hw.rd16(l + 10), 0, 3, 0)
        } else {
            (self.hw.rd32(l + 4) as i32, self.hw.rd32(l + 8) as i32, self.hw.rd16(l + 14), self.hw.rd32(l + 16), 4, self.hw.rd32(l + 32) as usize)
        };
        if w <= 0 || h == 0 || !matches!(bpp, 1 | 4 | 8 | 24) {
            return None;
        }
        let ncolours = if bpp <= 8 { if used != 0 { used.min(1 << bpp) } else { 1 << bpp } } else { 0 };
        Some(DibInfo { w, h: h.abs(), bpp, compression, colours: l + size, entry, ncolours })
    }

    /// The DIB's colour table as COLORREFs.
    fn dib_colours(&mut self, d: &DibInfo) -> Vec<u32> {
        (0..d.ncolours)
            .map(|i| {
                let e = d.colours + i as u32 * d.entry;
                let (b, g, r) = (self.hw.rd(e) as u32, self.hw.rd(e + 1) as u32, self.hw.rd(e + 2) as u32);
                r | g << 8 | b << 16
            })
            .collect()
    }

    /// One DIB row as COLORREFs.
    fn dib_row(raw: &[u8], d: &DibInfo, table: &[u32], out: &mut [u32]) {
        for (x, o) in out.iter_mut().enumerate() {
            *o = match d.bpp {
                1 => table.get((raw[x / 8] >> (7 - x % 8) & 1) as usize).copied().unwrap_or(0),
                4 => table.get((raw[x / 2] >> if x % 2 == 0 { 4 } else { 0 } & 15) as usize).copied().unwrap_or(0),
                8 => table.get(raw[x] as usize).copied().unwrap_or(0),
                _ => raw[x * 3 + 2] as u32 | (raw[x * 3 + 1] as u32) << 8 | (raw[x * 3] as u32) << 16,
            };
        }
    }

    /// COLORREFs to device pixel values, with a cache of the last colour.
    fn to_device(row: &[u32], mono: bool, out: &mut [u8]) {
        let mut last = (u32::MAX, 0u8);
        for (o, &c) in out.iter_mut().zip(row) {
            if c != last.0 {
                last = (c, if mono { mono_of(c) } else { nearest(c) });
            }
            *o = last.1;
        }
    }

    /// DEVICEBITMAPBITS(lpBitmap, fGet, iStart, cScans, lpDIBits,
    /// lpBitmapInfo, lpDrawMode, lpTranslate).
    pub(crate) fn disp_devicebitmapbits(&mut self, a: &mut Args) -> u32 {
        let pbm = a.p();
        let get = a.w() != 0;
        let start = a.w() as i32;
        let scans = a.w() as i32;
        let pbits = a.p();
        let pinfo = a.p();
        let Some(dev) = self.disp_dev(pbm) else { return 0 };
        let il = self.far(pinfo);
        let Some(d) = self.dib_info(il) else { return 0 };
        if d.compression != 0 || pbits.null() {
            return 0;
        }
        let bl = self.far(pbits);
        let stride = d.stride();
        let w = d.w.min(dev.w) as usize;
        let scans = scans.min(dev.h - start).max(0);
        let mut raw = vec![0u8; stride];
        let mut px = vec![0u8; w];
        if !get {
            let table = self.dib_colours(&d);
            let mut row = vec![0u32; w];
            for k in 0..scans {
                let y = dev.h - 1 - (start + k);
                if y < 0 {
                    break;
                }
                self.hw.read_bytes(bl + (k as usize * stride) as u32, &mut raw);
                Self::dib_row(&raw, &d, &table, &mut row);
                Self::to_device(&row, dev.mono, &mut px);
                self.dev_write(&dev, y, 0, &px);
            }
            if dev.screen {
                self.disp.touch(Rect::new(0, dev.h - start - scans, w as i32, dev.h - start));
            }
            return scans as u32;
        }
        // Get: the colour table first.
        let table: Vec<u32> = match d.bpp {
            1 => vec![0x000000, 0xffffff],
            4 => VGA16.iter().map(|&c| (c >> 16) & 0xff | (c & 0xff00) | (c & 0xff) << 16).collect(),
            8 => (0..=255).map(cref_of).collect(),
            _ => Vec::new(),
        };
        for (i, &c) in table.iter().enumerate() {
            let e = d.colours + i as u32 * d.entry;
            self.hw.write_bytes(e, &[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
            if d.entry == 4 {
                self.hw.wr(e + 3, 0);
            }
        }
        for k in 0..scans {
            let y = dev.h - 1 - (start + k);
            if y < 0 {
                break;
            }
            self.dev_read(&dev, y, 0, &mut px);
            raw.fill(0);
            for (x, &v) in px.iter().enumerate() {
                let c = if dev.mono { if v != 0 { 0xffffff } else { 0 } } else { cref_of(v) };
                match d.bpp {
                    1 => raw[x / 8] |= mono_of(c) << (7 - x % 8),
                    4 => {
                        let i = if dev.mono { if v != 0 { 15 } else { 0 } } else { nearest16(PALETTE[v as usize]) };
                        raw[x / 2] |= i << if x % 2 == 0 { 4 } else { 0 };
                    }
                    8 => raw[x] = if dev.mono { if v != 0 { 255 } else { 0 } } else { v },
                    _ => {
                        raw[x * 3] = (c >> 16) as u8;
                        raw[x * 3 + 1] = (c >> 8) as u8;
                        raw[x * 3 + 2] = c as u8;
                    }
                }
            }
            self.hw.write_bytes(bl + (k as usize * stride) as u32, &raw);
        }
        scans as u32
    }

    /// DIBSCREENBLT (SetDIBitsToDevice)(lpDestDev, X, Y, iScan, cScans,
    /// lpClipRect, lpDrawMode, lpDIBits, lpBitmapInfo, lpTranslate): the
    /// DIB's top left at (X, Y).
    pub(crate) fn disp_setdibits(&mut self, a: &mut Args) -> u32 {
        let pd = a.p();
        let x = a.i();
        let y = a.i();
        let start = a.w() as i32;
        let scans = a.w() as i32;
        let pclip = a.p();
        let _pdm = a.p();
        let pbits = a.p();
        let pinfo = a.p();
        let Some(dev) = self.disp_dev(pd) else { return 0 };
        let il = self.far(pinfo);
        let Some(d) = self.dib_info(il) else { return 0 };
        if d.compression != 0 || pbits.null() {
            return 0;
        }
        let clip = if pclip.null() {
            dev.bounds()
        } else {
            let l = self.far(pclip);
            Rect::new(self.hw.rd16(l) as i16 as i32, self.hw.rd16(l + 2) as i16 as i32, self.hw.rd16(l + 4) as i16 as i32, self.hw.rd16(l + 6) as i16 as i32).intersect(&dev.bounds())
        };
        let bl = self.far(pbits);
        let table = self.dib_colours(&d);
        let stride = d.stride();
        let mut raw = vec![0u8; stride];
        let mut row = vec![0u32; d.w as usize];
        let mut px = vec![0u8; d.w as usize];
        for k in 0..scans {
            let dy = y + d.h - 1 - (start + k);
            if dy < clip.y0 || dy >= clip.y1 {
                continue;
            }
            self.hw.read_bytes(bl + (k as usize * stride) as u32, &mut raw);
            Self::dib_row(&raw, &d, &table, &mut row);
            Self::to_device(&row, dev.mono, &mut px);
            let x0 = x.max(clip.x0);
            let x1 = (x + d.w).min(clip.x1);
            if x0 < x1 {
                self.dev_write(&dev, dy, x0, &px[(x0 - x) as usize..(x1 - x) as usize]);
            }
        }
        if dev.screen {
            self.disp.touch(Rect::new(x, y, x + d.w, y + d.h).intersect(&clip));
        }
        scans as u32
    }
}

fn nearest16(rgb: u32) -> u8 {
    let (r, g, b) = ((rgb >> 16) as i32 & 0xff, (rgb >> 8) as i32 & 0xff, rgb as i32 & 0xff);
    let mut best = (i32::MAX, 0u8);
    for (i, &c) in VGA16.iter().enumerate() {
        let (pr, pg, pb) = ((c >> 16) as i32 & 0xff, (c >> 8) as i32 & 0xff, c as i32 & 0xff);
        let d = (pr - r).pow(2) + (pg - g).pow(2) + (pb - b).pow(2);
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

#[allow(dead_code)]
fn unused(_: &Dev) {}
