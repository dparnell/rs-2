//! EXTTEXTOUT, STRBLT and GETCHARWIDTH with GDI's raster fonts.
//!
//! GDI hands the driver a FONTINFO that is the font file's header from
//! dfType on, at offset 42h of the file's image (dfVersion at offset 0):
//! the character table follows the header (3.0 fonts: width and a
//! 32-bit offset at 94h, 2.0 fonts: width and a 16-bit offset at 76h),
//! the offsets from the start of the image. A glyph is ceil(width / 8)
//! columns of one byte per row.

use alloc::vec;
use alloc::vec::Vec;

use super::blt::{pen_value, Brush};
use super::{Args, Dev, DrawMode, Ptr, Rect};
use crate::Machine;

pub(crate) struct Font {
    /// Linear address of the image (dfVersion).
    base: u32,
    v3: bool,
    height: i32,
    first: u8,
    last: u8,
    default: u8,
    break_char: u8,
    weight: u16,
}

impl Machine {
    fn font(&mut self, p: Ptr) -> Option<Font> {
        if p.null() {
            return None;
        }
        let fi = self.far(p);
        let base = fi.wrapping_sub(0x42);
        let version = self.hw.rd16(base);
        let v3 = match version {
            0x300 => true,
            0x200 => false,
            _ => return None,
        };
        Some(Font {
            base,
            v3,
            height: self.hw.rd16(fi + 22) as i32,
            first: self.hw.rd(fi + 29),
            last: self.hw.rd(fi + 30),
            default: self.hw.rd(fi + 31),
            break_char: self.hw.rd(fi + 32),
            weight: self.hw.rd16(fi + 17),
        })
    }

    /// (width, linear address of the bits) of a character.
    fn glyph(&mut self, f: &Font, c: u8) -> (i32, u32) {
        let i = if c < f.first || c > f.last { f.default } else { c - f.first } as u32;
        if f.v3 {
            let e = f.base + 0x94 + i * 6;
            (self.hw.rd16(e) as i32, f.base.wrapping_add(self.hw.rd32(e + 2)))
        } else {
            let e = f.base + 0x76 + i * 4;
            (self.hw.rd16(e) as i32, f.base.wrapping_add(self.hw.rd16(e + 2) as u32))
        }
    }

    /// The advance of each character (widths, character extra, breaks,
    /// or the caller's widths).
    fn advances(&mut self, f: &Font, s: &[u8], dm: &DrawMode, widths: Option<&[i32]>, bold: bool) -> Vec<i32> {
        let mut out = Vec::with_capacity(s.len());
        let mut err = dm.break_err;
        for (k, &c) in s.iter().enumerate() {
            if let Some(w) = widths {
                out.push(w.get(k).copied().unwrap_or(0));
                continue;
            }
            let (w, _) = self.glyph(f, c);
            let mut adv = w + dm.char_extra + bold as i32;
            if c == f.break_char.wrapping_add(f.first) && (dm.break_extra != 0 || dm.break_rem != 0) {
                adv += dm.break_extra;
                if dm.break_count > 0 {
                    err -= dm.break_rem;
                    if err <= 0 {
                        err += dm.break_count;
                        adv += 1;
                    }
                }
            }
            out.push(adv);
        }
        out
    }

    /// EXTTEXTOUT(lpDestDev, X, Y, lpClipRect, lpString, wCount,
    /// lpFontInfo, lpDrawMode, lpTextXForm, lpCharWidths, lpOpaqueRect,
    /// wOptions). A negative count asks for the extent only.
    pub(crate) fn disp_exttextout(&mut self, a: &mut Args) -> u32 {
        let pd = a.p();
        let x = a.i();
        let y = a.i();
        let pclip = a.p();
        let ps = a.p();
        let count = a.i();
        let pf = a.p();
        let pdm = a.p();
        let px = a.p();
        let pw = a.p();
        let po = a.p();
        let opts = a.w();
        self.text_out(pd, x, y, pclip, ps, count, pf, pdm, px, pw, po, opts)
    }

    /// STRBLT(lpDestDev, X, Y, lpClipRect, lpString, wCount, lpFontInfo,
    /// lpDrawMode, lpTextXForm).
    pub(crate) fn disp_strblt(&mut self, a: &mut Args) -> u32 {
        let pd = a.p();
        let x = a.i();
        let y = a.i();
        let pclip = a.p();
        let ps = a.p();
        let count = a.i();
        let pf = a.p();
        let pdm = a.p();
        let px = a.p();
        self.text_out(pd, x, y, pclip, ps, count, pf, pdm, px, Ptr::default(), Ptr::default(), 0)
    }

    #[allow(clippy::too_many_arguments)]
    fn text_out(&mut self, pd: Ptr, x: i32, y: i32, pclip: Ptr, ps: Ptr, count: i32, pf: Ptr, pdm: Ptr, px: Ptr, pw: Ptr, po: Ptr, opts: u16) -> u32 {
        let Some(f) = self.font(pf) else { return 0 };
        let dm = self.disp_drawmode(pdm);
        let n = count.unsigned_abs() as usize;
        let mut s = vec![0u8; n.min(4096)];
        if !ps.null() {
            let l = self.far(ps);
            self.hw.read_bytes(l, &mut s);
        }
        let widths: Option<Vec<i32>> = (!pw.null()).then(|| {
            let l = self.far(pw);
            (0..s.len()).map(|i| self.hw.rd16(l + i as u32 * 2) as i16 as i32).collect()
        });
        // Emboldening by doubling (TC_EA_DOUBLE).
        let bold = !px.null() && {
            let l = self.far(px);
            self.hw.rd16(l + 8) >= 600 && f.weight < 600
        };
        let adv = self.advances(&f, &s, &dm, widths.as_deref(), bold);
        let total: i32 = adv.iter().sum();
        let extent = (total as u32 & 0xffff) | (f.height as u32) << 16;
        if count < 0 {
            return extent;
        }
        let Some(dev) = self.disp_dev(pd) else { return 0 };
        let clip = if pclip.null() {
            dev.bounds()
        } else {
            let l = self.far(pclip);
            Rect::new(self.hw.rd16(l) as i16 as i32, self.hw.rd16(l + 2) as i16 as i32, self.hw.rd16(l + 4) as i16 as i32, self.hw.rd16(l + 6) as i16 as i32).intersect(&dev.bounds())
        };
        let bk = Brush::solid(dm.bk);
        let fill = DrawMode { rop2: 13, ..dm };
        if opts & 2 != 0 && !po.null() {
            let l = self.far(po);
            let r = Rect::new(self.hw.rd16(l) as i16 as i32, self.hw.rd16(l + 2) as i16 as i32, self.hw.rd16(l + 4) as i16 as i32, self.hw.rd16(l + 6) as i16 as i32);
            self.fill_rop2(&dev, r.intersect(&clip), &bk, &fill);
        } else if dm.bk_mode == 2 && !s.is_empty() {
            self.fill_rop2(&dev, Rect::new(x, y, x + total, y + f.height).intersect(&clip), &bk, &fill);
        }
        let fg = pen_value(dm.text, dev.mono);
        let mut cx = x;
        for (k, &c) in s.iter().enumerate() {
            let (w, bits) = self.glyph(&f, c);
            self.draw_glyph(&dev, cx, y, w, f.height, bits, fg, &clip, bold);
            cx += adv[k];
        }
        if dev.screen {
            self.disp.touch(Rect::new(x, y, cx.max(x + total) + 1, y + f.height).intersect(&clip));
        }
        extent
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_glyph(&mut self, dev: &Dev, x: i32, y: i32, w: i32, h: i32, bits: u32, fg: u8, clip: &Rect, bold: bool) {
        let cols = (w as usize).div_ceil(8);
        let mut g = vec![0u8; cols * h as usize];
        self.hw.read_bytes(bits, &mut g);
        let gw = w + bold as i32;
        let r = Rect::new(x, y, x + gw, y + h).intersect(clip);
        if r.is_empty() {
            return;
        }
        let n = (r.x1 - r.x0) as usize;
        let mut row = vec![0u8; n];
        for py in r.y0..r.y1 {
            let gy = (py - y) as usize;
            let bit = |gx: i32| -> bool { gx >= 0 && gx < w && g[(gx as usize / 8) * h as usize + gy] & (0x80 >> (gx % 8)) != 0 };
            if !(0..n).any(|i| {
                let gx = r.x0 + i as i32 - x;
                bit(gx) || (bold && bit(gx - 1))
            }) {
                continue;
            }
            self.dev_read(dev, py, r.x0, &mut row);
            for (i, v) in row.iter_mut().enumerate() {
                let gx = r.x0 + i as i32 - x;
                if bit(gx) || (bold && bit(gx - 1)) {
                    *v = fg;
                }
            }
            self.dev_write(dev, py, r.x0, &row);
        }
    }

    /// GETCHARWIDTH(lpDestDev, lpBuffer, wFirstChar, wLastChar,
    /// lpFontInfo, lpDrawMode, lpFontTrans).
    pub(crate) fn disp_getcharwidth(&mut self, a: &mut Args) -> u32 {
        let _pd = a.p();
        let pb = a.p();
        let first = a.w();
        let last = a.w();
        let pf = a.p();
        let _pdm = a.p();
        let _px = a.p();
        let Some(f) = self.font(pf) else { return 0 };
        let l = self.far(pb);
        for (k, c) in (first..=last.min(255)).enumerate() {
            let (w, _) = self.glyph(&f, c as u8);
            self.hw.wr16(l + k as u32 * 2, w as u16);
        }
        1
    }
}
