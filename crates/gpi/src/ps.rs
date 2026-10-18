//! Presentation space: drawing attributes and primitives, in presentation
//! page coordinates offset by the window origin and clipped to the
//! visible region.

use alloc::vec::Vec;

use crate::color::Rgb;
use crate::font::{self, Font};
use crate::rect::{Point, Rect};
use crate::surface::Surface;

/// Foreground/background mix modes (FM_* / BM_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mix {
    Overpaint,
    LeaveAlone,
    Xor,
    Or,
    And,
    Invert,
}

impl Mix {
    /// FM_* code (0 = default = overpaint).
    pub fn from_fm(v: i32) -> Option<Mix> {
        Some(match v {
            0 | 2 => Mix::Overpaint,
            1 => Mix::Or,
            4 => Mix::Xor,
            5 => Mix::LeaveAlone,
            6 => Mix::And,
            9 => Mix::Invert,
            _ => return None,
        })
    }

    /// BM_* code (0 = default = leave alone).
    pub fn from_bm(v: i32) -> Option<Mix> {
        Some(match v {
            0 | 5 => Mix::LeaveAlone,
            2 => Mix::Overpaint,
            4 => Mix::Xor,
            _ => return None,
        })
    }

    fn apply(self, dst: Rgb, src: Rgb) -> Rgb {
        match self {
            Mix::Overpaint => src,
            Mix::LeaveAlone => dst,
            Mix::Xor => dst ^ src,
            Mix::Or => dst | src,
            Mix::And => dst & src,
            Mix::Invert => !dst & 0xff_ffff,
        }
    }
}

pub struct Ps {
    pub color: Rgb,
    pub back_color: Rgb,
    pub mix: Mix,
    pub back_mix: Mix,
    /// Current position (page coordinates).
    pub pos: Point,
    /// Device position of the page origin (the window's bottom-left).
    pub origin: Point,
    /// Visible region in device coordinates.
    pub clip: Vec<Rect>,
    pub font: &'static Font,
}

impl Ps {
    pub fn new(origin: Point, clip: Vec<Rect>) -> Ps {
        Ps {
            color: 0x000000,
            back_color: 0xffffff,
            mix: Mix::Overpaint,
            back_mix: Mix::LeaveAlone,
            pos: Point::default(),
            origin,
            clip,
            font: font::system(),
        }
    }

    fn device(&self, r: Rect) -> Rect {
        r.offset(self.origin.x, self.origin.y)
    }

    /// Fill a page rectangle with a colour (overpaint).
    pub fn fill_rect(&self, s: &mut Surface, r: Rect, c: Rgb) {
        let d = self.device(r);
        for cr in &self.clip {
            s.fill(d.intersect(cr), c);
        }
    }

    /// Plot one page point with the foreground mix.
    fn plot(&self, s: &mut Surface, x: i32, y: i32, c: Rgb, mix: Mix) {
        let (dx, dy) = (x + self.origin.x, y + self.origin.y);
        let p = Point::new(dx, dy);
        if !self.clip.iter().any(|r| r.contains(p)) {
            return;
        }
        match mix {
            Mix::Overpaint => s.set(dx, dy, c),
            Mix::LeaveAlone => {}
            m => {
                if let Some(old) = s.get(dx, dy) {
                    s.set(dx, dy, m.apply(old, c));
                }
            }
        }
    }

    pub fn move_to(&mut self, p: Point) {
        self.pos = p;
    }

    /// Line from the current position to `p` (both ends drawn), which
    /// becomes the current position.
    pub fn line_to(&mut self, s: &mut Surface, p: Point) {
        let (mut x0, mut y0) = (self.pos.x, self.pos.y);
        let (dx, dy) = ((p.x - x0).abs(), -(p.y - y0).abs());
        let (sx, sy) = (if x0 < p.x { 1 } else { -1 }, if y0 < p.y { 1 } else { -1 });
        let mut err = dx + dy;
        loop {
            self.plot(s, x0, y0, self.color, self.mix);
            if x0 == p.x && y0 == p.y {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
        self.pos = p;
    }

    /// Rectangle from the current position to the opposite corner `p`
    /// (inclusive), outlined and/or filled; the current position stays.
    pub fn draw_box(&mut self, s: &mut Surface, p: Point, outline: bool, fill: bool) {
        let a = self.pos;
        let r = Rect::new(a.x.min(p.x), a.y.min(p.y), a.x.max(p.x) + 1, a.y.max(p.y) + 1);
        if fill {
            self.fill_rect(s, r, self.color);
        }
        if outline {
            let corners = [Point::new(r.right - 1, r.bottom), Point::new(r.right - 1, r.top - 1), Point::new(r.left, r.top - 1), Point::new(r.left, r.bottom)];
            self.pos = Point::new(r.left, r.bottom);
            for c in corners {
                self.line_to(s, c);
            }
        }
        self.pos = a;
    }

    /// Draw `text` with the baseline starting at `p`; the current position
    /// moves to the end of the string.
    pub fn char_string_at(&mut self, s: &mut Surface, p: Point, text: &[u8]) {
        let f = self.font;
        let bottom = p.y - f.descender;
        if self.back_mix != Mix::LeaveAlone {
            let r = Rect::new(p.x, bottom, p.x + f.text_width(text), bottom + f.height);
            self.fill_rect(s, r, self.back_color);
        }
        for (i, &b) in text.iter().enumerate() {
            let x0 = p.x + i as i32 * f.width;
            for row in 0..f.height {
                let bits = f.row(b, row as usize);
                if bits == 0 {
                    continue;
                }
                let y = bottom + f.height - 1 - row;
                for col in 0..f.width.min(8) {
                    if bits & (0x80 >> col) != 0 {
                        self.plot(s, x0 + col, y, self.color, self.mix);
                    }
                }
            }
        }
        self.pos = Point::new(p.x + f.text_width(text), p.y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn surface(buf: &mut Vec<u32>, w: i32, h: i32) -> Surface {
        unsafe { Surface::new(buf.as_mut_ptr(), w, h, w, false) }
    }

    #[test]
    fn fill_is_clipped() {
        let mut buf = vec![0u32; 100];
        let mut s = surface(&mut buf, 10, 10);
        let ps = Ps::new(Point::new(2, 2), vec![Rect::new(0, 0, 5, 5)]);
        ps.fill_rect(&mut s, Rect::new(0, 0, 10, 10), 0xff0000);
        assert_eq!(s.get(2, 2), Some(0xff0000));
        assert_eq!(s.get(4, 4), Some(0xff0000));
        assert_eq!(s.get(5, 5), Some(0));
        assert_eq!(s.get(1, 1), Some(0));
    }

    #[test]
    fn device_y_is_bottom_up() {
        let mut buf = vec![0u32; 4];
        let mut s = surface(&mut buf, 2, 2);
        s.set(0, 0, 0x123456);
        // Bottom-left pixel is the first pixel of the last row in memory.
        assert_eq!(buf[2], 0x123456);
    }

    #[test]
    fn text_draws_glyph_pixels() {
        let mut buf = vec![0xffffffu32; 64 * 32];
        let mut s = surface(&mut buf, 64, 32);
        let mut ps = Ps::new(Point::new(0, 0), vec![s.bounds()]);
        ps.char_string_at(&mut s, Point::new(0, 8), b"H");
        let black = buf.iter().filter(|&&p| p == 0).count();
        assert!(black > 10, "glyph drawn");
        assert_eq!(ps.pos, Point::new(8, 8));
    }

    #[test]
    fn line_endpoints() {
        let mut buf = vec![0u32; 100];
        let mut s = surface(&mut buf, 10, 10);
        let mut ps = Ps::new(Point::new(0, 0), vec![s.bounds()]);
        ps.color = 0xffffff;
        ps.move_to(Point::new(1, 1));
        ps.line_to(&mut s, Point::new(8, 5));
        assert_eq!(s.get(1, 1), Some(0xffffff));
        assert_eq!(s.get(8, 5), Some(0xffffff));
    }
}
