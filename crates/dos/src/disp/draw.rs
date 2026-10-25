//! OUTPUT (polylines and scan lines), PIXEL, SCANLR and FASTBORDER.

use alloc::vec;
use alloc::vec::Vec;

use super::blt::{pen_value, rop2, Brush};
use super::{Args, Dev, DrawMode, Rect};
use crate::Machine;

const OS_SCANLINES: u16 = 4;
const OS_POLYLINE: u16 = 18;
const OS_BEGINNSCAN: u16 = 80;
const OS_ENDNSCAN: u16 = 81;

/// Line styles (PS_DASH .. PS_DASHDOTDOT): on/off runs in pixels.
const STYLES: [&[u8]; 4] = [&[18, 6], &[3, 3], &[9, 6, 3, 6], &[9, 3, 3, 3, 3, 3]];

impl Machine {
    fn clip_rect(&mut self, p: super::Ptr, dev: &Dev) -> Rect {
        if p.null() {
            return dev.bounds();
        }
        let l = self.far(p);
        let r = Rect::new(self.hw.rd16(l) as i16 as i32, self.hw.rd16(l + 2) as i16 as i32, self.hw.rd16(l + 4) as i16 as i32, self.hw.rd16(l + 6) as i16 as i32);
        r.intersect(&dev.bounds())
    }

    /// OUTPUT(lpDestDev, wStyle, wCount, lpPoints, lpPPen, lpPBrush,
    /// lpDrawMode, lpClipRect).
    pub(crate) fn disp_output(&mut self, a: &mut Args) -> u32 {
        let pd = a.p();
        let style = a.w();
        let count = a.w() as usize;
        let pp = a.p();
        let ppen = a.p();
        let pbr = a.p();
        let pdm = a.p();
        let pclip = a.p();
        if matches!(style, OS_BEGINNSCAN | OS_ENDNSCAN) {
            return 1;
        }
        let Some(dev) = self.disp_dev(pd) else { return 0 };
        let dm = self.disp_drawmode(pdm);
        let clip = self.clip_rect(pclip, &dev);
        let lp = self.far(pp);
        let mut pts: Vec<(i32, i32)> = Vec::with_capacity(count);
        for i in 0..count.min(16384) {
            let at = lp + i as u32 * 4;
            pts.push((self.hw.rd16(at) as i16 as i32, self.hw.rd16(at + 2) as i16 as i32));
        }
        match style {
            OS_POLYLINE => {
                if let Some(pen) = self.disp_pen(ppen) {
                    let mut phase = 0usize;
                    for s in pts.windows(2) {
                        self.line(&dev, s[0], s[1], pen.phys, pen.style, &mut phase, &dm, &clip);
                    }
                }
                1
            }
            OS_SCANLINES => {
                // The first point's y is the scan line, each further point
                // one segment: x from its x to its y (exclusive).
                let Some(&(_, y)) = pts.first() else { return 1 };
                let brush = match self.disp_brush(pbr) {
                    Some(b) => Some(b),
                    None => self.disp_pen(ppen).map(|p| Brush::solid(p.phys)),
                };
                let Some(b) = brush else { return 1 };
                for &(x0, x1) in &pts[1..] {
                    let r = Rect::new(x0.min(x1), y, x0.max(x1), y + 1).intersect(&clip);
                    self.fill_rop2(&dev, r, &b, &dm);
                }
                1
            }
            _ => 0xffff_ffff,
        }
    }

    /// One pixel of a line under the ROP2.
    fn plot(&mut self, dev: &Dev, x: i32, y: i32, v: u8, rop: u8) {
        if dev.screen {
            let o = y as usize * self.disp.w + x as usize;
            self.disp.pix[o] = rop2(rop, v, self.disp.pix[o]);
            return;
        }
        let mut px = [0u8; 1];
        if rop != 13 {
            self.dev_read(dev, y, x, &mut px);
        }
        px[0] = if dev.mono { rop2(rop, if v & 1 != 0 { 0xff } else { 0 }, if px[0] & 1 != 0 { 0xff } else { 0 }) >> 7 & 1 } else { rop2(rop, v, px[0]) };
        self.dev_write(dev, y, x, &px);
    }

    /// A line from `a` to `b`, `b` not drawn, styled and clipped.
    #[allow(clippy::too_many_arguments)]
    fn line(&mut self, dev: &Dev, a: (i32, i32), b: (i32, i32), phys: u32, style: u8, phase: &mut usize, dm: &DrawMode, clip: &Rect) {
        let v = pen_value(phys, dev.mono);
        let bg = pen_value(dm.bk, dev.mono);
        let pattern = (1..=4).contains(&style).then(|| STYLES[style as usize - 1]);
        let period: usize = pattern.map_or(1, |p| p.iter().map(|&x| x as usize).sum());
        let (mut x, mut y) = a;
        let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
        let (sx, sy) = (if a.0 < b.0 { 1 } else { -1 }, if a.1 < b.1 { 1 } else { -1 });
        let mut err = dx + dy;
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        while (x, y) != b {
            let on = match pattern {
                None => Some(true),
                Some(p) => {
                    let mut t = *phase % period;
                    let mut on = true;
                    for &run in p {
                        if t < run as usize {
                            break;
                        }
                        t -= run as usize;
                        on = !on;
                    }
                    if on { Some(true) } else if dm.bk_mode == 2 { Some(false) } else { None }
                }
            };
            *phase += 1;
            if let Some(on) = on {
                if x >= clip.x0 && x < clip.x1 && y >= clip.y0 && y < clip.y1 {
                    self.plot(dev, x, y, if on { v } else { bg }, dm.rop2);
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        if dev.screen && x0 <= x1 {
            self.disp.touch(Rect::new(x0, y0, x1 + 1, y1 + 1));
        }
    }

    /// PIXEL(lpDestDev, X, Y, dwPhysColor, lpDrawMode): set a pixel, or
    /// with lpDrawMode NULL read it.
    pub(crate) fn disp_pixel(&mut self, a: &mut Args) -> u32 {
        let pd = a.p();
        let x = a.i();
        let y = a.i();
        let c = a.d();
        let pdm = a.p();
        let Some(dev) = self.disp_dev(pd) else { return 0x8000_0000 };
        if x < 0 || y < 0 || x >= dev.w || y >= dev.h {
            return 0x8000_0000;
        }
        let mut px = [0u8; 1];
        if pdm.null() {
            self.dev_read(&dev, y, x, &mut px);
            return if dev.mono { (px[0] as u32) << 8 | if px[0] != 0 { 255 } else { 0 } } else { px[0] as u32 | (super::mono_of(super::cref_of(px[0])) as u32) << 8 };
        }
        let dm = self.disp_drawmode(pdm);
        self.plot(&dev, x, y, pen_value(c, dev.mono), dm.rop2);
        if dev.screen {
            self.disp.touch(Rect::new(x, y, x + 1, y + 1));
        }
        1
    }

    /// SCANLR(lpDestDev, X, Y, dwPhysColor, wStyle): from (X, Y) right
    /// (or left, style bit 1) to the first pixel of the colour (style bit
    /// 0) or not of it; -1 if none, 8000h when (X, Y) is off the device.
    pub(crate) fn disp_scanlr(&mut self, a: &mut Args) -> u32 {
        let pd = a.p();
        let x = a.i();
        let y = a.i();
        let c = a.d();
        let style = a.w();
        let Some(dev) = self.disp_dev(pd) else { return 0x8000 };
        if x < 0 || y < 0 || x >= dev.w || y >= dev.h {
            return 0x8000;
        }
        let want = pen_value(c, dev.mono);
        let mut row = vec![0u8; dev.w as usize];
        self.dev_read(&dev, y, 0, &mut row);
        let hit = |v: u8| (v == want) == (style & 1 != 0);
        let found = if style & 2 != 0 { (0..=x).rev().find(|&i| hit(row[i as usize])) } else { (x..dev.w).find(|&i| hit(row[i as usize])) };
        found.map_or(0xffff, |i| i as u32)
    }

    /// FASTBORDER(lpRect, wHorizBorderThick, wVertBorderThick, dwRasterOp,
    /// lpDestDev, lpPBrush, lpDrawMode, lpClipRect): a frame inside the
    /// rectangle with the brush.
    pub(crate) fn disp_fastborder(&mut self, a: &mut Args) -> u32 {
        let pr = a.p();
        let hb = a.i();
        let vb = a.i();
        let rop = (a.d() >> 16) as u8;
        let pd = a.p();
        let pbr = a.p();
        let pdm = a.p();
        let pclip = a.p();
        let Some(dev) = self.disp_dev(pd) else { return 0 };
        let dm = self.disp_drawmode(pdm);
        let clip = self.clip_rect(pclip, &dev);
        let brush = self.disp_brush(pbr);
        let l = self.far(pr);
        let r = Rect::new(self.hw.rd16(l) as i16 as i32, self.hw.rd16(l + 2) as i16 as i32, self.hw.rd16(l + 4) as i16 as i32, self.hw.rd16(l + 6) as i16 as i32);
        let parts = [
            Rect::new(r.x0, r.y0, r.x1, r.y0 + vb),
            Rect::new(r.x0, r.y1 - vb, r.x1, r.y1),
            Rect::new(r.x0, r.y0 + vb, r.x0 + hb, r.y1 - vb),
            Rect::new(r.x1 - hb, r.y0 + vb, r.x1, r.y1 - vb),
        ];
        for p in parts {
            let q = p.intersect(&clip);
            if !q.is_empty() {
                self.blt(&dev, q.x0, q.y0, None, 0, 0, q.x1 - q.x0, q.y1 - q.y0, rop, brush.as_ref(), &dm);
            }
        }
        1
    }
}
