//! rs2's own cards, drawn with GPI at any size: the faces (corner
//! indexes, pips, aces, court cards with a framed emblem) and nine
//! backs. They are drawn once per card size into an atlas (13 columns,
//! one row per suit and a row of backs) that the board copies from.

use alloc::vec::Vec;

use pmkit::gpi::{self, MemPs};
use pmkit::*;

use crate::game::{rank, red, suit};

pub const BACKS: usize = 9;

const WHITE: i32 = 0xffffff;
const FACE: i32 = 0xfdfcf6;
const EDGE: i32 = 0x303030;
const RED: i32 = 0xc8102e;
const BLACK: i32 = 0x161616;

/// The atlas of card pictures at one size.
pub struct Atlas {
    pub ps: MemPs,
    pub w: i32,
    pub h: i32,
}

impl Atlas {
    pub fn new(hab: u32, w: i32, h: i32, board: i32) -> Option<Atlas> {
        let ps = MemPs::new(hab, w * 13, h * 5)?;
        gpi::fill(ps.hps, RectL::new(0, 0, w * 13, h * 5), board);
        let a = Atlas { ps, w, h };
        for c in 0..52u8 {
            let (x, y) = a.slot_of(c as usize);
            draw_face(a.ps.hps, c, x, y, w, h);
        }
        for b in 0..BACKS {
            let (x, y) = a.slot_of(52 + b);
            draw_back(a.ps.hps, b, x, y, w, h);
        }
        Some(a)
    }

    /// Where picture `i` (a card 0..52, then the backs) is in the atlas.
    fn slot_of(&self, i: usize) -> (i32, i32) {
        let (col, row) = if i < 52 { (i % 13, i / 13) } else { (i - 52, 4) };
        (col as i32 * self.w, row as i32 * self.h)
    }

    /// Copy picture `i` to (x, y) of a PS.
    pub fn blit(&self, hps: u32, i: usize, x: i32, y: i32) {
        let (sx, sy) = self.slot_of(i);
        let p = [pt(x, y), pt(x + self.w, y + self.h), pt(sx, sy), pt(sx + self.w, sy + self.h)];
        unsafe { GpiBitBlt(hps, self.ps.hps, 4, p.as_ptr(), ROP_SRCCOPY, BBO_IGNORE) };
    }

    /// Copy the lower part of picture `i` (its rows from `y0` up) — for a
    /// card partly covered by the next.
    pub fn blit_part(&self, hps: u32, i: usize, x: i32, y: i32, visible_top: i32) {
        let (sx, sy) = self.slot_of(i);
        let vis = visible_top.clamp(0, self.h);
        let p = [pt(x, y + self.h - vis), pt(x + self.w, y + self.h), pt(sx, sy + self.h - vis), pt(sx + self.w, sy + self.h)];
        unsafe { GpiBitBlt(hps, self.ps.hps, 4, p.as_ptr(), ROP_SRCCOPY, BBO_IGNORE) };
    }
}

fn card_shape(hps: u32, x: i32, y: i32, w: i32, h: i32, fill_c: i32) {
    let r = (w / 10).max(2);
    gpi::round_box(hps, RectL::new(x, y, x + w, y + h), r, fill_c, EDGE);
}

fn suit_color(c: u8) -> i32 {
    if red(c) { RED } else { BLACK }
}

/// A suit symbol of height `s` centred at (cx, cy).
pub fn pip(hps: u32, s_: u8, cx: i32, cy: i32, s: i32, col: i32) {
    let f = |v: f32| (v * s as f32) as i32;
    match s_ {
        1 => {
            // Diamond.
            gpi::polygon(hps, &[pt(cx, cy + f(0.5)), pt(cx + f(0.37), cy), pt(cx, cy - f(0.5)), pt(cx - f(0.37), cy)], col);
        }
        2 => {
            // Heart: two lobes and a point.
            let r = f(0.26);
            gpi::ellipse(hps, RectL::new(cx - 2 * r, cy + f(0.24) - r, cx, cy + f(0.24) + r), col);
            gpi::ellipse(hps, RectL::new(cx, cy + f(0.24) - r, cx + 2 * r, cy + f(0.24) + r), col);
            gpi::polygon(hps, &[pt(cx - 2 * r + 1, cy + f(0.16)), pt(cx + 2 * r - 1, cy + f(0.16)), pt(cx, cy - f(0.5))], col);
        }
        3 => {
            // Spade: a heart upside down and a foot.
            let r = f(0.25);
            gpi::ellipse(hps, RectL::new(cx - 2 * r, cy - f(0.1) - r, cx, cy - f(0.1) + r), col);
            gpi::ellipse(hps, RectL::new(cx, cy - f(0.1) - r, cx + 2 * r, cy - f(0.1) + r), col);
            gpi::polygon(hps, &[pt(cx - 2 * r + 1, cy - f(0.02)), pt(cx + 2 * r - 1, cy - f(0.02)), pt(cx, cy + f(0.5))], col);
            gpi::polygon(hps, &[pt(cx, cy - f(0.12)), pt(cx + f(0.17), cy - f(0.5)), pt(cx - f(0.17), cy - f(0.5))], col);
        }
        _ => {
            // Club: three leaves and a foot.
            let r = f(0.21);
            gpi::ellipse(hps, RectL::new(cx - r, cy + f(0.29) - r, cx + r, cy + f(0.29) + r), col);
            gpi::ellipse(hps, RectL::new(cx - f(0.25) - r, cy - f(0.04) - r, cx - f(0.25) + r, cy - f(0.04) + r), col);
            gpi::ellipse(hps, RectL::new(cx + f(0.25) - r, cy - f(0.04) - r, cx + f(0.25) + r, cy - f(0.04) + r), col);
            gpi::ellipse(hps, RectL::new(cx - f(0.1), cy - f(0.1), cx + f(0.1), cy + f(0.12)), col);
            gpi::polygon(hps, &[pt(cx, cy + f(0.02)), pt(cx + f(0.16), cy - f(0.5)), pt(cx - f(0.16), cy - f(0.5))], col);
        }
    }
}

const RANKS: [&[u8]; 13] = [b"A", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"10", b"J", b"Q", b"K"];

pub fn rank_text(c: u8) -> &'static [u8] {
    RANKS[rank(c) as usize]
}

/// Pip places (fractions of the pip area's width and height, from the
/// top) for 2..10.
fn pips_for(n: u8) -> &'static [(f32, f32)] {
    const L: f32 = 0.0;
    const M: f32 = 0.5;
    const R: f32 = 1.0;
    match n {
        2 => &[(M, 0.0), (M, 1.0)],
        3 => &[(M, 0.0), (M, 0.5), (M, 1.0)],
        4 => &[(L, 0.0), (R, 0.0), (L, 1.0), (R, 1.0)],
        5 => &[(L, 0.0), (R, 0.0), (M, 0.5), (L, 1.0), (R, 1.0)],
        6 => &[(L, 0.0), (R, 0.0), (L, 0.5), (R, 0.5), (L, 1.0), (R, 1.0)],
        7 => &[(L, 0.0), (R, 0.0), (M, 0.25), (L, 0.5), (R, 0.5), (L, 1.0), (R, 1.0)],
        8 => &[(L, 0.0), (R, 0.0), (M, 0.25), (L, 0.5), (R, 0.5), (M, 0.75), (L, 1.0), (R, 1.0)],
        9 => &[(L, 0.0), (R, 0.0), (L, 1.0 / 3.0), (R, 1.0 / 3.0), (M, 0.5), (L, 2.0 / 3.0), (R, 2.0 / 3.0), (L, 1.0), (R, 1.0)],
        _ => &[(L, 0.0), (R, 0.0), (M, 1.0 / 6.0), (L, 1.0 / 3.0), (R, 1.0 / 3.0), (L, 2.0 / 3.0), (R, 2.0 / 3.0), (M, 5.0 / 6.0), (L, 1.0), (R, 1.0)],
    }
}

/// The font for the corner indexes and court letters (outline,
/// `height` pels, bold).
fn font(hps: u32, face: &[u8], height: i32, bold: bool) {
    // The faces' own bold versions ("Helvetica Bold").
    let name = if bold { [face, b" Bold"].concat() } else { face.to_vec() };
    let f = FAttrs::outline(&name);
    unsafe {
        GpiSetCharSet(hps, 0);
        GpiDeleteSetId(hps, 1);
        GpiCreateLogFont(hps, core::ptr::null(), 1, &f);
        GpiSetCharSet(hps, 1);
        GpiSetCharBox(hps, &SizeL { cx: height << 16, cy: height << 16 });
    }
}

fn centered_text(hps: u32, cx: i32, baseline: i32, t: &[u8], col: i32) {
    let w = gpi::text_width(hps, t);
    gpi::text_at(hps, cx - w / 2, baseline, t, col);
}

pub fn draw_face(hps: u32, c: u8, x: i32, y: i32, w: i32, h: i32) {
    card_shape(hps, x, y, w, h, FACE);
    let col = suit_color(c);
    let s = suit(c);
    let r = rank(c);
    // Corner indexes: the rank and a small pip, top left and bottom right.
    let ih = (h * 15 / 100).max(8);
    font(hps, b"Helvetica", ih, true);
    let iw = (w * 16 / 100).max(6);
    let ix = x + 2 + iw / 2 + w / 40;
    centered_text(hps, ix, y + h - 3 - ih * 3 / 4, rank_text(c), col);
    pip(hps, s, ix, y + h - 4 - ih - ih * 3 / 8, ih * 3 / 4, col);
    let rx = x + w - 2 - iw / 2 - w / 40;
    pip(hps, s, rx, y + 4 + ih * 3 / 8, ih * 3 / 4, col);
    centered_text(hps, rx, y + 3 + ih * 3 / 4 + ih / 5, rank_text(c), col);
    // The middle.
    let (ml, mr) = (x + w * 30 / 100, x + w * 70 / 100);
    let (mt, mb) = (y + h * 80 / 100, y + h * 20 / 100);
    match r {
        0 => {
            let big = h * 36 / 100;
            if s == 3 {
                // The ace of spades gets a ring around its pip.
                gpi::ellipse_outline(hps, RectL::new(x + w / 2 - big * 3 / 4, y + h / 2 - big * 3 / 4, x + w / 2 + big * 3 / 4, y + h / 2 + big * 3 / 4), col);
                gpi::ellipse_outline(hps, RectL::new(x + w / 2 - big * 3 / 4 + 2, y + h / 2 - big * 3 / 4 + 2, x + w / 2 + big * 3 / 4 - 2, y + h / 2 + big * 3 / 4 - 2), col);
            }
            pip(hps, s, x + w / 2, y + h / 2, big, col);
        }
        1..=9 => {
            let ps = (h * 14 / 100).max(6);
            for &(fx, fy) in pips_for(r + 1) {
                let px = ml + ((mr - ml) as f32 * fx) as i32;
                let py = mt - ((mt - mb) as f32 * fy) as i32;
                pip(hps, s, px, py, ps, col);
            }
        }
        _ => court(hps, c, x, y, w, h),
    }
}

/// A court card: a tinted frame with an emblem (crown, coronet or cap),
/// the letter and the suit.
fn court(hps: u32, c: u8, x: i32, y: i32, w: i32, h: i32) {
    let col = suit_color(c);
    let r = rank(c);
    let fr = RectL::new(x + w * 20 / 100, y + h * 13 / 100, x + w * 80 / 100, y + h * 87 / 100);
    let tint = match r {
        12 => 0xf6e3a1,
        11 => 0xf5d2dc,
        _ => 0xd3e4f5,
    };
    gpi::fill(hps, fr, tint);
    gpi::frame(hps, fr, col);
    gpi::frame(hps, RectL::new(fr.l + 2, fr.b + 2, fr.r - 2, fr.t - 2), col);
    let cx = (fr.l + fr.r) / 2;
    let fw = fr.r - fr.l;
    let fh = fr.t - fr.b;
    // The emblem in the upper part.
    let top = fr.t - fh / 10;
    let base = fr.t - fh * 36 / 100;
    let gold = 0xd4a017;
    let half = fw * 36 / 100;
    match r {
        12 => {
            // A crown with three points and jewels.
            let pts = [pt(cx - half, base), pt(cx - half, top - fh / 10), pt(cx - half / 2, base + fh / 12), pt(cx, top), pt(cx + half / 2, base + fh / 12), pt(cx + half, top - fh / 10), pt(cx + half, base)];
            gpi::polygon(hps, &pts, gold);
            gpi::color(hps, col);
            gpi::move_to(hps, pts[0].x, pts[0].y);
            unsafe { GpiPolyLine(hps, 6, pts[1..].as_ptr()) };
            gpi::line_to(hps, pts[0].x, pts[0].y);
            let j = (fw / 14).max(2);
            for jx in [cx - half / 2 - 1, cx, cx + half / 2 + 1] {
                gpi::ellipse(hps, RectL::new(jx - j, base + j, jx + j, base + 3 * j), col);
            }
        }
        11 => {
            // A coronet: a band with five small balls.
            let band = fh / 12;
            gpi::fill(hps, RectL::new(cx - half, base, cx + half, base + band), gold);
            gpi::frame(hps, RectL::new(cx - half, base, cx + half, base + band), col);
            let b = (fw / 12).max(2);
            for i in 0..5 {
                let bx = cx - half + b + (2 * half - 2 * b) * i / 4;
                let by = base + band + b + if i == 2 { b } else { 0 };
                gpi::line(hps, bx, base + band, bx, by);
                gpi::ellipse(hps, RectL::new(bx - b, by - b / 2, bx + b, by + b + b / 2), col);
            }
        }
        _ => {
            // A cap with a feather.
            gpi::ellipse(hps, RectL::new(cx - half, base, cx + half, base + fh / 6), col);
            gpi::polygon(hps, &[pt(cx - half / 2, base + fh / 10), pt(cx + half / 2, base + fh / 10), pt(cx + half / 3, base + fh / 4), pt(cx - half / 3, base + fh / 4)], col);
            let fc = 0x2e8b57;
            gpi::polygon(hps, &[pt(cx + half / 4, base + fh / 5), pt(cx + half, top + fh / 20), pt(cx + half / 2, base + fh / 7)], fc);
        }
    }
    // The letter and the suit below the emblem.
    let lh = fh * 30 / 100;
    font(hps, b"Times New Roman", lh, true);
    centered_text(hps, cx, base - lh * 85 / 100, rank_text(c), col);
    pip(hps, suit(c), cx, fr.b + fh * 14 / 100, fh * 18 / 100, col);
}

/// The nine backs: a white rim around a coloured pattern.
pub fn draw_back(hps: u32, b: usize, x: i32, y: i32, w: i32, h: i32) {
    card_shape(hps, x, y, w, h, WHITE);
    let m = (w / 14).max(3);
    let r = RectL::new(x + m, y + m, x + w - m, y + h - m);
    let (base, pattern) = match b {
        0 => (0x1d4e9e, 0x7fa8e8),
        1 => (0xa31621, 0xe8828a),
        2 => (0x1e6b3a, 0x8fd19e),
        3 => (0x1b1f4b, 0xd8c35a),
        4 => (0x6d1a36, 0xe9b44c),
        5 => (0x0f6e70, 0xbfe9e7),
        6 => (0x4b2a7b, 0xcdb4f0),
        7 => (0xc4561b, 0xffd9a8),
        _ => (0x2b2b2b, 0xc0c0c0),
    };
    gpi::fill(hps, r, base);
    unsafe { GpiIntersectClipRectangle(hps, &RectL::new(r.l, r.b, r.r - 1, r.t - 1)) };
    let (rw, rh) = (r.r - r.l, r.t - r.b);
    let step = (w / 8).max(4);
    gpi::color(hps, pattern);
    match b {
        0 | 1 => {
            // Lattice.
            let mut k = -rh;
            while k < rw + rh {
                gpi::line(hps, r.l + k, r.b, r.l + k + rh, r.t);
                gpi::line(hps, r.l + k, r.t, r.l + k + rh, r.b);
                k += step;
            }
        }
        2 => {
            // Checks.
            let mut row = 0;
            let mut yy = r.b;
            while yy < r.t {
                let mut xx = r.l + if row % 2 == 1 { step } else { 0 };
                while xx < r.r {
                    gpi::fill(hps, RectL::new(xx, yy, xx + step, yy + step), pattern);
                    xx += 2 * step;
                }
                yy += step;
                row += 1;
            }
        }
        3 => {
            // Stars (small diamonds) on a dark ground.
            let mut yy = r.b + step / 2;
            let mut row = 0;
            while yy < r.t {
                let mut xx = r.l + step / 2 + if row % 2 == 1 { step / 2 } else { 0 };
                while xx < r.r {
                    pip(hps, 1, xx, yy, step / 2, pattern);
                    xx += step;
                }
                yy += step;
                row += 1;
            }
        }
        4 => {
            // Stripes with a border.
            let mut xx = r.l;
            while xx < r.r {
                gpi::fill(hps, RectL::new(xx, r.b, xx + step / 3, r.t), pattern);
                xx += step;
            }
            gpi::frame(hps, RectL::new(r.l + 3, r.b + 3, r.r - 3, r.t - 3), pattern);
        }
        5 => {
            // Nested diamonds.
            let (cx, cy) = ((r.l + r.r) / 2, (r.b + r.t) / 2);
            let mut k = rw.max(rh);
            while k > 0 {
                gpi::color(hps, pattern);
                gpi::move_to(hps, cx, cy + k);
                gpi::line_to(hps, cx + k * rw / rh.max(1), cy);
                gpi::line_to(hps, cx, cy - k);
                gpi::line_to(hps, cx - k * rw / rh.max(1), cy);
                gpi::line_to(hps, cx, cy + k);
                k -= step / 2;
            }
        }
        6 => {
            // Circles.
            let mut yy = r.b;
            while yy < r.t + step {
                let mut xx = r.l;
                while xx < r.r + step {
                    gpi::ellipse_outline(hps, RectL::new(xx - step / 2, yy - step / 2, xx + step / 2, yy + step / 2), pattern);
                    xx += step;
                }
                yy += step;
            }
        }
        7 => {
            // Zigzags.
            let mut yy = r.b;
            gpi::color(hps, pattern);
            while yy < r.t {
                gpi::move_to(hps, r.l, yy);
                let mut xx = r.l;
                let mut up = true;
                while xx < r.r {
                    xx += step / 2;
                    gpi::line_to(hps, xx, yy + if up { step / 2 } else { 0 });
                    up = !up;
                }
                yy += step;
            }
        }
        _ => {
            // A sun: rays around a disc.
            let (cx, cy) = ((r.l + r.r) / 2, (r.b + r.t) / 2);
            let rr = rw.min(rh) / 4;
            for i in 0..16 {
                let a = i as f32 * core::f32::consts::PI / 8.0;
                let (sx, sy) = (sin(a), sin(a + core::f32::consts::FRAC_PI_2));
                gpi::line(hps, cx, cy, cx + (sx * rh as f32) as i32, cy + (sy * rh as f32) as i32);
            }
            gpi::ellipse(hps, RectL::new(cx - rr, cy - rr, cx + rr, cy + rr), 0xe8c547);
        }
    }
    unsafe { GpiSetClipRegion(hps, 0, core::ptr::null_mut()) };
    gpi::frame(hps, r, pattern);
}

/// sin(x) for |x| up to a few pi (Taylor series after reduction).
fn sin(mut x: f32) -> f32 {
    let pi = core::f32::consts::PI;
    while x > pi {
        x -= 2.0 * pi;
    }
    while x < -pi {
        x += 2.0 * pi;
    }
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
}

/// The empty place of a pile: a rounded outline (and a letter or mark).
pub fn draw_place(hps: u32, x: i32, y: i32, w: i32, h: i32, board: i32, mark: &[u8]) {
    let r = (w / 10).max(2);
    let line = lighter(board);
    gpi::color(hps, line);
    gpi::move_to(hps, x + 1, y + 1);
    unsafe { GpiBox(hps, DRO_OUTLINE, &pt(x + w - 2, y + h - 2), r * 2, r * 2) };
    if !mark.is_empty() {
        font(hps, b"Helvetica", h / 4, true);
        centered_text(hps, x + w / 2, y + h / 2 - h / 12, mark, line);
    }
}

/// A colour a little lighter than `c` (for marks on the board).
pub fn lighter(c: i32) -> i32 {
    let f = |v: i32| (v + (255 - v) * 2 / 5).min(255);
    (f((c >> 16) & 255) << 16) | (f((c >> 8) & 255) << 8) | f(c & 255)
}

/// All the pictures' indexes (for the self-test).
pub fn all() -> Vec<usize> {
    (0..52 + BACKS).collect()
}
