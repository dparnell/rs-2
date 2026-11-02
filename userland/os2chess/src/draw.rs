//! Drawing the board, flat (2-D) or in perspective (3-D, turned by the
//! Rotate dialog), with rs2's own piece shapes, and finding the square
//! under the pointer.

use alloc::vec::Vec;

use pmkit::gpi;
use pmkit::*;

use crate::engine::*;

/// The colours of the board (0xRRGGBB), as the Settings dialog picks them.
#[derive(Clone, Copy)]
pub struct Colors {
    pub background: i32,
    pub light: i32,
    pub dark: i32,
    pub edge: i32,
    pub labels: i32,
    pub white: i32,
    pub black: i32,
    pub highlight: i32,
}

/// The colour schemes offered (name, colours).
pub const SCHEMES: [(&[u8], Colors); 4] = [
    (b"Walnut", Colors { background: 0x2f4f4f, light: 0xe8d0a0, dark: 0x9a6a3a, edge: 0x5a3818, labels: 0xf0e0b0, white: 0xf8f4e8, black: 0x282828, highlight: 0x30c030 }),
    (b"Marble", Colors { background: 0x404858, light: 0xe8e8e8, dark: 0x8090a0, edge: 0x303840, labels: 0xe0e0e0, white: 0xfffff0, black: 0x302018, highlight: 0xffc000 }),
    (b"Tournament", Colors { background: 0x204020, light: 0xf0f0d0, dark: 0x608850, edge: 0x183018, labels: 0xe0f0d0, white: 0xffffff, black: 0x202020, highlight: 0xe04040 }),
    (b"Classic blue", Colors { background: 0x103060, light: 0xd8e4f0, dark: 0x4870a8, edge: 0x102040, labels: 0xd0e0ff, white: 0xffffff, black: 0x181818, highlight: 0xffff40 }),
];

/// How the board is shown.
#[derive(Clone, Copy)]
pub struct View {
    pub three_d: bool,
    /// The board turned so black is at the bottom.
    pub flipped: bool,
    /// Degrees the 3-D board is turned (counter clockwise).
    pub angle: i32,
    pub labels: bool,
}

/// Where squares and pieces are on the window (made by `layout`).
pub struct Layout {
    /// The four corners of each square (lower left, lower right, upper
    /// right, upper left as seen from white), in window pels.
    pub corners: [[PointL; 4]; 64],
    /// Each square's centre on the board and the size of a piece there.
    pub base: [(PointL, i32); 64],
    /// Squares from far to near (drawing order).
    pub order: [u8; 64],
    pub board: [PointL; 4],
    pub area: RectL,
}

fn sin_deg(d: i32) -> f32 {
    // A sine from a short series after reducing to -90..90 degrees.
    let mut a = d.rem_euclid(360);
    let mut sign = 1.0;
    if a >= 180 {
        a -= 180;
        sign = -1.0;
    }
    if a > 90 {
        a = 180 - a;
    }
    let x = a as f32 * core::f32::consts::PI / 180.0;
    let x2 = x * x;
    sign * x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
}

fn cos_deg(d: i32) -> f32 {
    sin_deg(d + 90)
}

/// Lay the board out in `r` (the client without the status line).
pub fn layout(r: RectL, v: &View) -> Layout {
    let mut l = Layout { corners: [[PointL::default(); 4]; 64], base: [(PointL::default(), 0); 64], order: [0; 64], board: [PointL::default(); 4], area: r };
    let (w, h) = (r.width().max(40), r.height().max(40));
    if !v.three_d {
        let margin = if v.labels { 3 } else { 1 };
        let s = (w.min(h) * 2 / (16 + margin * 2)).max(4);
        let x0 = r.l + (w - 8 * s) / 2;
        let y0 = r.b + (h - 8 * s) / 2;
        for sq in 0..64u8 {
            let (mut f, mut rk) = (file(sq), rank(sq));
            if v.flipped {
                f = 7 - f;
                rk = 7 - rk;
            }
            let (x, y) = (x0 + f * s, y0 + rk * s);
            l.corners[sq as usize] = [pt(x, y), pt(x + s, y), pt(x + s, y + s), pt(x, y + s)];
            l.base[sq as usize] = (pt(x + s / 2, y + s * 2 / 25), s * 21 / 25);
            l.order[sq as usize] = sq;
        }
        l.board = [pt(x0, y0), pt(x0 + 8 * s, y0), pt(x0 + 8 * s, y0 + 8 * s), pt(x0, y0 + 8 * s)];
        return l;
    }
    // Perspective: the board in units of a square, centred, turned by
    // the angle, seen from the front at about 37 degrees.
    let ang = v.angle + if v.flipped { 180 } else { 0 };
    let (sa, ca) = (sin_deg(ang), cos_deg(ang));
    let (se, ce) = (0.6f32, 0.8f32);
    let dist = 15.0f32;
    let proj = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
        let u = x * ca - y * sa;
        let vv = x * sa + y * ca;
        let depth = dist + vv * ce - z * se;
        (u / depth, (vv * se + z * ce) / depth, depth)
    };
    // Fit the board's edge and the tallest pieces into the window.
    let (mut minx, mut maxx, mut miny, mut maxy) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (x, y) in [(-4.4f32, -4.4f32), (4.4, -4.4), (4.4, 4.4), (-4.4, 4.4)] {
        for z in [0.0f32, 1.1] {
            let (px, py, _) = proj(x, y, z);
            minx = minx.min(px);
            maxx = maxx.max(px);
            miny = miny.min(py);
            maxy = maxy.max(py);
        }
    }
    let scale = ((w as f32 - 8.0) / (maxx - minx)).min((h as f32 - 8.0) / (maxy - miny));
    let ox = r.l as f32 + w as f32 / 2.0 - (minx + maxx) / 2.0 * scale;
    let oy = r.b as f32 + h as f32 / 2.0 - (miny + maxy) / 2.0 * scale;
    let to = |x: f32, y: f32, z: f32| -> (PointL, f32) {
        let (px, py, d) = proj(x, y, z);
        (pt((ox + px * scale) as i32, (oy + py * scale) as i32), d)
    };
    let mut depth = [0f32; 64];
    for sq in 0..64u8 {
        let (x, y) = (file(sq) as f32 - 4.0, rank(sq) as f32 - 4.0);
        l.corners[sq as usize] = [to(x, y, 0.0).0, to(x + 1.0, y, 0.0).0, to(x + 1.0, y + 1.0, 0.0).0, to(x, y + 1.0, 0.0).0];
        let (c, d) = to(x + 0.5, y + 0.5, 0.0);
        depth[sq as usize] = d;
        let size = (scale / d * 0.95) as i32;
        l.base[sq as usize] = (pt(c.x, c.y - size / 8), size);
    }
    l.board = [to(-4.4, -4.4, 0.0).0, to(4.4, -4.4, 0.0).0, to(4.4, 4.4, 0.0).0, to(-4.4, 4.4, 0.0).0];
    let mut ord: Vec<u8> = (0..64).collect();
    ord.sort_by(|&a, &b| depth[b as usize].partial_cmp(&depth[a as usize]).unwrap_or(core::cmp::Ordering::Equal));
    l.order.copy_from_slice(&ord);
    l
}

/// The square at a point, if any.
pub fn square_at(l: &Layout, x: i32, y: i32) -> Option<u8> {
    (0..64u8).find(|&s| inside(&l.corners[s as usize], x, y))
}

fn inside(q: &[PointL; 4], x: i32, y: i32) -> bool {
    // The point is on the same side of every edge (the corners go round
    // one way or the other).
    let mut pos = false;
    let mut neg = false;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let c = (b.x - a.x) as i64 * (y - a.y) as i64 - (b.y - a.y) as i64 * (x - a.x) as i64;
        if c > 0 {
            pos = true;
        }
        if c < 0 {
            neg = true;
        }
    }
    !(pos && neg)
}

fn poly(hps: u32, p: &[PointL], fill: i32, edge: i32) {
    gpi::polygon(hps, p, fill);
    gpi::color(hps, edge);
    unsafe {
        GpiMove(hps, &p[p.len() - 1]);
        GpiPolyLine(hps, p.len() as i32, p.as_ptr());
    }
}

fn oval(hps: u32, r: RectL, fill: i32, edge: i32) {
    gpi::ellipse(hps, r, fill);
    gpi::ellipse_outline(hps, r, edge);
}

/// A piece of `kind` drawn with its base centred at `base`, `size` pels
/// tall (rs2's own shapes, on a 100 by 100 grid).
pub fn piece(hps: u32, kind: i8, white: bool, base: PointL, size: i32, c: &Colors) {
    let (fill, edge) = if white { (c.white, 0x202020) } else { (c.black, 0xa0a0a0) };
    let m = |x: i32, y: i32| pt(base.x + (x - 50) * size / 100, base.y + y * size / 100);
    let shape = |pts: &[(i32, i32)]| -> Vec<PointL> { pts.iter().map(|&(x, y)| m(x, y)).collect() };
    let circle = |x: i32, y: i32, r: i32| RectL::new(m(x - r, y - r).x, m(x - r, y - r).y, m(x + r, y + r).x, m(x + r, y + r).y);
    let foot = shape(&[(16, 0), (84, 0), (78, 10), (22, 10)]);
    match kind.abs() {
        PAWN => {
            poly(hps, &shape(&[(34, 10), (66, 10), (57, 40), (43, 40)]), fill, edge);
            oval(hps, circle(50, 41, 11).with_height(m(0, 4).y - m(0, 0).y), fill, edge);
            oval(hps, circle(50, 56, 13), fill, edge);
        }
        ROOK => {
            poly(hps, &shape(&[(28, 10), (72, 10), (67, 62), (33, 62)]), fill, edge);
            poly(hps, &shape(&[(24, 62), (76, 62), (76, 84), (66, 84), (66, 75), (57, 75), (57, 84), (43, 84), (43, 75), (34, 75), (34, 84), (24, 84)]), fill, edge);
        }
        KNIGHT => {
            poly(hps, &shape(&[(24, 10), (76, 10), (72, 30), (68, 56), (62, 72), (54, 84), (48, 76), (34, 66), (22, 52), (26, 42), (38, 46), (48, 42), (36, 26)]), fill, edge);
            oval(hps, circle(52, 64, 3), edge, edge);
        }
        BISHOP => {
            poly(hps, &shape(&[(36, 10), (64, 10), (57, 30), (43, 30)]), fill, edge);
            oval(hps, RectL::new(m(35, 28).x, m(35, 28).y, m(65, 72).x, m(65, 72).y), fill, edge);
            oval(hps, circle(50, 78, 6), fill, edge);
            gpi::color(hps, edge);
            gpi::line(hps, m(56, 58).x, m(56, 58).y, m(46, 44).x, m(46, 44).y);
        }
        QUEEN => {
            poly(hps, &shape(&[(30, 10), (70, 10), (62, 48), (38, 48)]), fill, edge);
            poly(hps, &shape(&[(30, 48), (70, 48), (80, 78), (64, 60), (58, 84), (50, 62), (42, 84), (36, 60), (20, 78)]), fill, edge);
            for (x, y) in [(20, 80), (42, 86), (58, 86), (80, 80)] {
                oval(hps, circle(x, y, 4), fill, edge);
            }
        }
        _ => {
            poly(hps, &shape(&[(30, 10), (70, 10), (62, 50), (38, 50)]), fill, edge);
            poly(hps, &shape(&[(32, 50), (68, 50), (72, 68), (28, 68)]), fill, edge);
            poly(hps, &shape(&[(46, 68), (54, 68), (54, 77), (63, 77), (63, 85), (54, 85), (54, 96), (46, 96), (46, 85), (37, 85), (37, 77), (46, 77)]), fill, edge);
        }
    }
    poly(hps, &foot, fill, edge);
}

trait WithHeight {
    fn with_height(self, h: i32) -> RectL;
}

impl WithHeight for RectL {
    /// The same centre, `h` pels tall (a flat collar).
    fn with_height(self, h: i32) -> RectL {
        let c = (self.b + self.t) / 2;
        RectL::new(self.l, c - h.max(1), self.r, c + h.max(1))
    }
}

/// What to mark on the board.
pub struct Marks<'a> {
    pub selected: Option<u8>,
    pub targets: &'a [u8],
    pub last: Option<(u8, u8)>,
}

/// Draw the board and the pieces of `p`.
pub fn board(hps: u32, l: &Layout, p: &Position, v: &View, c: &Colors, marks: &Marks) {
    gpi::fill(hps, l.area, c.background);
    // The board's edge.
    poly(hps, &l.board, c.edge, c.edge);
    for sq in 0..64u8 {
        let light = (file(sq) + rank(sq)) & 1 == 1;
        let mut col = if light { c.light } else { c.dark };
        if marks.last.is_some_and(|(a, b)| a == sq || b == sq) {
            col = mix(col, c.highlight, 3);
        }
        poly(hps, &l.corners[sq as usize], col, col);
    }
    for &sq in marks.targets {
        outline_square(hps, l, sq, c.highlight);
    }
    if let Some(s) = marks.selected {
        outline_square(hps, l, s, c.highlight);
    }
    if v.labels {
        labels(hps, l, v, c);
    }
    for &sq in l.order.iter() {
        let pc = p.sq[sq as usize];
        if pc != 0 {
            let (b, size) = l.base[sq as usize];
            piece(hps, pc, pc > 0, b, size, c);
        }
    }
}

fn outline_square(hps: u32, l: &Layout, sq: u8, col: i32) {
    let q = &l.corners[sq as usize];
    gpi::color(hps, col);
    unsafe { GpiSetLineWidth(hps, 0x20000) };
    // Two pels wide: the square's edge and one inside it.
    for inset in 0..3 {
        let cx = (q[0].x + q[1].x + q[2].x + q[3].x) / 4;
        let cy = (q[0].y + q[1].y + q[2].y + q[3].y) / 4;
        let pts: Vec<PointL> = q.iter().map(|p| pt(p.x + (cx - p.x).signum() * inset, p.y + (cy - p.y).signum() * inset)).collect();
        unsafe {
            GpiMove(hps, &pts[3]);
            GpiPolyLine(hps, 4, pts.as_ptr());
        }
    }
}

fn labels(hps: u32, l: &Layout, v: &View, c: &Colors) {
    let _ = v;
    let s = (l.corners[0][1].x - l.corners[0][0].x).abs().max((l.corners[0][3].y - l.corners[0][0].y).abs());
    gpi::select_font(hps, 1, b"Helvetica", (s * 2 / 5).clamp(8, 24));
    let (fh, _) = gpi::font_height(hps);
    for i in 0..8u8 {
        // Files below rank 1 and ranks left of file a.
        let q = &l.corners[i as usize];
        let (mx, my) = ((q[0].x + q[1].x) / 2, (q[0].y + q[1].y) / 2);
        let (cx, cy) = ((q[2].x + q[3].x) / 2, (q[2].y + q[3].y) / 2);
        let t = [b'a' + i];
        let w = gpi::text_width(hps, &t);
        gpi::text_at(hps, mx + (mx - cx) * 2 / 3 - w / 2, my + (my - cy) * 2 / 3 - fh / 3, &t, c.labels);
        let q = &l.corners[(i * 8) as usize];
        let (mx, my) = ((q[0].x + q[3].x) / 2, (q[0].y + q[3].y) / 2);
        let (cx, cy) = ((q[1].x + q[2].x) / 2, (q[1].y + q[2].y) / 2);
        let t = [b'1' + i];
        let w = gpi::text_width(hps, &t);
        gpi::text_at(hps, mx + (mx - cx) * 2 / 3 - w / 2, my + (my - cy) * 2 / 3 - fh / 3, &t, c.labels);
    }
    unsafe { GpiSetCharSet(hps, 0) };
}

/// `a` with a part (in quarters) of `b`.
fn mix(a: i32, b: i32, quarters: i32) -> i32 {
    let ch = |s: i32| -> i32 { (((a >> s) & 0xff) * (4 - quarters) + ((b >> s) & 0xff) * quarters) / 4 };
    ch(16) << 16 | ch(8) << 8 | ch(0)
}
