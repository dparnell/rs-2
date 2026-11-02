//! rs2's own tile set, drawn with GPI: circles, bamboo, characters,
//! winds, dragons, flowers and seasons. Faces are drawn on a design
//! grid of 30 by 40 units (y up) scaled to the tile.

use pmkit::gpi;
use pmkit::*;

/// A face being drawn: its rectangle on the PS.
struct F {
    hps: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl F {
    fn px(&self, u: i32) -> i32 {
        self.x + u * self.w / 30
    }
    fn py(&self, v: i32) -> i32 {
        self.y + v * self.h / 40
    }
    fn r(&self, l: i32, b: i32, r: i32, t: i32) -> RectL {
        RectL::new(self.px(l), self.py(b), self.px(r).max(self.px(l) + 1), self.py(t).max(self.py(b) + 1))
    }
    fn disc(&self, cx: i32, cy: i32, rad: i32, c: i32) {
        let rr = (rad * self.w / 30).max(1);
        let (x, y) = (self.px(cx), self.py(cy));
        gpi::ellipse(self.hps, RectL::new(x - rr, y - rr, x + rr, y + rr), c);
    }
    fn line(&self, a: (i32, i32), b: (i32, i32), c: i32) {
        gpi::color(self.hps, c);
        gpi::line(self.hps, self.px(a.0), self.py(a.1), self.px(b.0), self.py(b.1));
    }
    fn poly(&self, pts: &[(i32, i32)], c: i32) {
        let v: alloc::vec::Vec<PointL> = pts.iter().map(|&(u, v)| pt(self.px(u), self.py(v))).collect();
        gpi::polygon(self.hps, &v, c);
    }
    fn fill(&self, l: i32, b: i32, r: i32, t: i32, c: i32) {
        gpi::fill(self.hps, self.r(l, b, r, t), c);
    }
    /// Text centred at (cx, baseline), `size` design units high.
    fn text(&self, cx: i32, base: i32, size: i32, s: &[u8], c: i32) {
        // Logical font 1 is made once per drawing (begin()).
        let hpx = (size * self.h / 40).max(6);
        unsafe {
            GpiSetCharSet(self.hps, 1);
            GpiSetCharBox(self.hps, &SizeL { cx: hpx << 16, cy: hpx << 16 });
        }
        let w = gpi::text_width(self.hps, s);
        gpi::text_at(self.hps, self.px(cx) - w / 2, self.py(base), s, c);
    }
}

const BLUE: i32 = 0x1d4fa8;
const GREEN: i32 = 0x16803a;
const RED: i32 = 0xc4231b;
const BLACK: i32 = 0x202020;
const GOLD: i32 = 0xd9a520;

/// Where the circles of 1-9 lie (design units).
fn circle_spots(n: usize) -> &'static [(i32, i32)] {
    match n {
        1 => &[(15, 20)],
        2 => &[(15, 30), (15, 10)],
        3 => &[(8, 32), (15, 20), (22, 8)],
        4 => &[(9, 29), (21, 29), (9, 11), (21, 11)],
        5 => &[(8, 31), (22, 31), (15, 20), (8, 9), (22, 9)],
        6 => &[(9, 32), (21, 32), (9, 20), (21, 20), (9, 8), (21, 8)],
        7 => &[(6, 34), (15, 30), (24, 26), (9, 16), (21, 16), (9, 6), (21, 6)],
        8 => &[(9, 34), (21, 34), (9, 25), (21, 25), (9, 15), (21, 15), (9, 6), (21, 6)],
        _ => &[(6, 33), (15, 33), (24, 33), (6, 20), (15, 20), (24, 20), (6, 7), (15, 7), (24, 7)],
    }
}

fn circles(f: &F, n: usize) {
    let spots = circle_spots(n);
    let rad = match n {
        1 => 11,
        2..=4 => 6,
        5 | 6 => 5,
        _ => 4,
    };
    for (i, &(x, y)) in spots.iter().enumerate() {
        let c = match (n, i) {
            (1, _) => RED,
            (_, i) if i % 3 == 0 => BLUE,
            (_, i) if i % 3 == 1 => GREEN,
            _ => RED,
        };
        f.disc(x, y, rad, c);
        f.disc(x, y, rad * 2 / 3, 0xf4f0e0);
        f.disc(x, y, (rad / 3).max(1), c);
    }
}

/// One stick of bamboo, from (x, y0) to (x, y1).
fn stick(f: &F, x: i32, y0: i32, y1: i32, c: i32) {
    f.fill(x - 2, y0, x + 2, y1, c);
    let mid = (y0 + y1) / 2;
    f.fill(x - 3, mid - 1, x + 3, mid + 1, 0x0b4d22);
    f.fill(x - 3, y0, x + 3, y0 + 1, 0x0b4d22);
    f.fill(x - 3, y1 - 1, x + 3, y1, 0x0b4d22);
}

fn bamboo(f: &F, n: usize) {
    if n == 1 {
        // One tall stalk with leaves.
        stick(f, 15, 4, 36, GREEN);
        f.poly(&[(15, 26), (4, 32), (8, 27)], GREEN);
        f.poly(&[(15, 18), (26, 24), (22, 19)], GREEN);
        f.poly(&[(15, 12), (5, 14), (9, 10)], 0x0b4d22);
        return;
    }
    // Rows of sticks: (count per row) top to bottom.
    let rows: &[usize] = match n {
        2 => &[1, 1],
        3 => &[1, 2],
        4 => &[2, 2],
        5 => &[2, 1, 2],
        6 => &[3, 3],
        7 => &[1, 3, 3],
        8 => &[4, 4],
        _ => &[3, 3, 3],
    };
    let nr = rows.len() as i32;
    let rh = 36 / nr;
    for (r, &cnt) in rows.iter().enumerate() {
        let top = 38 - r as i32 * rh;
        let bot = top - rh + 2;
        for k in 0..cnt {
            let x = 30 * (2 * k as i32 + 1) / (2 * cnt as i32);
            let c = if (r + k) % 3 == 2 { RED } else { GREEN };
            stick(f, x, bot, top, c);
        }
    }
}

fn characters(f: &F, n: usize) {
    let digit = [b'0' + n as u8];
    f.text(15, 22, 17, &digit, BLUE);
    // A red seal below the number.
    f.fill(7, 3, 23, 17, RED);
    f.fill(9, 5, 21, 15, 0xf4f0e0);
    f.fill(11, 9, 19, 11, RED);
    f.fill(14, 6, 16, 14, RED);
}

fn wind(f: &F, w: usize) {
    let letter = [b"E", b"S", b"W", b"N"][w];
    f.text(15, 14, 20, letter, BLACK);
    // An arrow pointing the wind's way.
    let (a, b): ((i32, i32), (i32, i32)) = match w {
        0 => ((5, 6), (25, 6)),
        1 => ((15, 38), (15, 32)),
        2 => ((25, 6), (5, 6)),
        _ => ((15, 32), (15, 38)),
    };
    f.line(a, b, RED);
    let (dx, dy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
    let tip = b;
    f.poly(&[tip, (tip.0 - 3 * dx - 2 * dy, tip.1 - 3 * dy - 2 * dx), (tip.0 - 3 * dx + 2 * dy, tip.1 - 3 * dy + 2 * dx)], RED);
}

fn dragon(f: &F, d: usize) {
    match d {
        0 => {
            // Red: a flame.
            f.poly(&[(15, 36), (24, 20), (21, 8), (15, 4), (9, 8), (6, 20)], RED);
            f.poly(&[(15, 26), (19, 16), (15, 8), (11, 16)], GOLD);
        }
        1 => {
            // Green: a pine.
            f.poly(&[(15, 37), (25, 22), (19, 22), (26, 11), (4, 11), (11, 22), (5, 22)], GREEN);
            f.fill(13, 4, 17, 11, 0x6b3e1a);
        }
        _ => {
            // White: a blue double frame.
            f.fill(4, 4, 26, 36, BLUE);
            f.fill(6, 6, 24, 34, 0xf4f0e0);
            f.fill(8, 8, 22, 32, BLUE);
            f.fill(10, 10, 20, 30, 0xf4f0e0);
        }
    }
}

fn corner_number(f: &F, n: usize, c: i32) {
    f.text(25, 31, 8, &[b'0' + n as u8], c);
}

fn flower(f: &F, k: usize) {
    let petal = [0xe0609a, 0x8a4fc4, 0xe8b818, 0xd8483c][k];
    for &(x, y) in &[(15, 27), (21, 22), (19, 15), (11, 15), (9, 22)] {
        f.disc(x, y, 5, petal);
    }
    f.disc(15, 20, 3, GOLD);
    f.line((15, 14), (15, 3), GREEN);
    f.poly(&[(15, 7), (22, 10), (18, 6)], GREEN);
    corner_number(f, k + 1, petal);
}

fn season(f: &F, k: usize) {
    match k {
        0 => {
            // Spring: a sprout.
            f.line((15, 4), (15, 22), GREEN);
            f.poly(&[(15, 18), (5, 26), (6, 20)], GREEN);
            f.poly(&[(15, 22), (25, 30), (24, 24)], GREEN);
            f.disc(15, 24, 3, 0xe0609a);
        }
        1 => {
            // Summer: the sun.
            for (dx, dy) in [(0, 12), (0, -12), (12, 0), (-12, 0), (8, 8), (-8, 8), (8, -8), (-8, -8)] {
                f.line((15, 20), (15 + dx, 20 + dy), 0xe07a10);
            }
            f.disc(15, 20, 7, 0xf2c419);
        }
        2 => {
            // Autumn: a leaf.
            f.poly(&[(15, 34), (24, 24), (21, 12), (15, 6), (9, 12), (6, 24)], 0xd2691e);
            f.line((15, 3), (15, 30), 0x7a3b10);
        }
        _ => {
            // Winter: a snowflake.
            for (a, b) in [((15, 6), (15, 34)), ((4, 13), (26, 27)), ((4, 27), (26, 13))] {
                f.line(a, b, 0x3a8fd8);
            }
            f.disc(15, 20, 2, 0x3a8fd8);
        }
    }
    corner_number(f, k + 1, BLACK);
}

/// Make the faces' font (logical font 1) on a PS before drawing them.
pub fn begin(hps: u32) {
    gpi::select_font(hps, 1, b"Helvetica", 12);
}

/// Draw face `face` in the rectangle (its background too).
pub fn draw_face(hps: u32, face: u8, x: i32, y: i32, w: i32, h: i32, bg: i32) {
    gpi::fill(hps, RectL::new(x, y, x + w, y + h), bg);
    let f = F { hps, x, y, w, h };
    let n = face as usize;
    match n {
        0..=8 => circles(&f, n + 1),
        9..=17 => bamboo(&f, n - 8),
        18..=26 => characters(&f, n - 17),
        27..=30 => wind(&f, n - 27),
        31..=33 => dragon(&f, n - 31),
        34..=37 => flower(&f, n - 34),
        _ => season(&f, n - 38),
    }
}

/// The name of a face (for hints and the self-check).
pub fn face_name(face: u8) -> alloc::string::String {
    let n = face as usize;
    match n {
        0..=8 => alloc::format!("{} of circles", n + 1),
        9..=17 => alloc::format!("{} of bamboo", n - 8),
        18..=26 => alloc::format!("{} of characters", n - 17),
        27..=30 => alloc::format!("{} wind", ["east", "south", "west", "north"][n - 27]),
        31..=33 => alloc::format!("{} dragon", ["red", "green", "white"][n - 31]),
        34..=37 => alloc::format!("flower {}", n - 33),
        _ => alloc::format!("season {}", n - 37),
    }
}
