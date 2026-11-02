//! What the tools do to a form (no PM here): the pen, straight lines,
//! flood fill, rectangles of the selection (fill, flip, circle, cut and
//! paste) and the conversions between forms.

use alloc::vec::Vec;

use crate::format::Form;

/// A rectangle of pels, inclusive, from the top left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Area {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Area {
    /// The area between two corners, in any order.
    pub fn span(a: (i32, i32), b: (i32, i32)) -> Area {
        Area { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1) }
    }
    pub fn all(f: &Form) -> Area {
        Area { x0: 0, y0: 0, x1: f.cx as i32 - 1, y1: f.cy as i32 - 1 }
    }
    pub fn w(&self) -> i32 {
        self.x1 - self.x0 + 1
    }
    pub fn h(&self) -> i32 {
        self.y1 - self.y0 + 1
    }
    /// The part inside the form.
    pub fn clip(&self, f: &Form) -> Area {
        Area { x0: self.x0.max(0), y0: self.y0.max(0), x1: self.x1.min(f.cx as i32 - 1), y1: self.y1.min(f.cy as i32 - 1) }
    }
}

/// The pen: a w by h block around (x, y).
pub fn pen(f: &mut Form, x: i32, y: i32, w: i32, h: i32, v: u32) {
    let (x0, y0) = (x - (w - 1) / 2, y - (h - 1) / 2);
    for yy in y0..y0 + h {
        for xx in x0..x0 + w {
            f.set(xx, yy, v);
        }
    }
}

/// A straight line drawn with the pen (Bresenham).
pub fn line(f: &mut Form, a: (i32, i32), b: (i32, i32), w: i32, h: i32, v: u32) {
    let (mut x, mut y) = a;
    let dx = (b.0 - x).abs();
    let dy = -(b.1 - y).abs();
    let sx = if x < b.0 { 1 } else { -1 };
    let sy = if y < b.1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        pen(f, x, y, w, h, v);
        if (x, y) == b {
            break;
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
}

/// Fill the pels of (x, y)'s value that touch it (sideways, not
/// diagonally) with `v`; how many changed.
pub fn flood(f: &mut Form, x: i32, y: i32, v: u32) -> usize {
    if x < 0 || y < 0 || x >= f.cx as i32 || y >= f.cy as i32 {
        return 0;
    }
    let old = f.get(x, y);
    if old == v {
        return 0;
    }
    let mut stack = alloc::vec![(x, y)];
    let mut n = 0;
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= f.cx as i32 || y >= f.cy as i32 || f.get(x, y) != old {
            continue;
        }
        f.set(x, y, v);
        n += 1;
        stack.extend_from_slice(&[(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
    n
}

pub fn fill(f: &mut Form, a: Area, v: u32) {
    let a = a.clip(f);
    for y in a.y0..=a.y1 {
        for x in a.x0..=a.x1 {
            f.set(x, y, v);
        }
    }
}

pub fn flip_h(f: &mut Form, a: Area) {
    let a = a.clip(f);
    for y in a.y0..=a.y1 {
        for i in 0..a.w() / 2 {
            let (l, r) = (f.get(a.x0 + i, y), f.get(a.x1 - i, y));
            f.set(a.x0 + i, y, r);
            f.set(a.x1 - i, y, l);
        }
    }
}

pub fn flip_v(f: &mut Form, a: Area) {
    let a = a.clip(f);
    for x in a.x0..=a.x1 {
        for i in 0..a.h() / 2 {
            let (t, b) = (f.get(x, a.y0 + i), f.get(x, a.y1 - i));
            f.set(x, a.y0 + i, b);
            f.set(x, a.y1 - i, t);
        }
    }
}

/// An ellipse's outline inside the area, drawn with the pen.
pub fn circle(f: &mut Form, a: Area, w: i32, h: i32, v: u32) {
    // Centre and radii in half pels, so even sizes centre between pels.
    let (cx2, cy2) = (a.x0 + a.x1, a.y0 + a.y1);
    let (rx2, ry2) = ((a.x1 - a.x0).max(0), (a.y1 - a.y0).max(0));
    // Step round the ellipse finely enough to leave no gaps.
    let steps = ((rx2 + ry2) * 4).max(8);
    let mut last: Option<(i32, i32)> = None;
    for i in 0..=steps {
        let t = i as f32 * core::f32::consts::TAU / steps as f32;
        let x = (cx2 as f32 + rx2 as f32 * cos(t)) / 2.0;
        let y = (cy2 as f32 + ry2 as f32 * sin(t)) / 2.0;
        let p = (round(x), round(y));
        if last != Some(p) {
            match last {
                Some(l) => line(f, l, p, w, h, v),
                None => pen(f, p.0, p.1, w, h, v),
            }
            last = Some(p);
        }
    }
}

fn round(v: f32) -> i32 {
    if v >= 0.0 { (v + 0.5) as i32 } else { (v - 0.5) as i32 }
}

/// sin and cos without libm (Taylor series after reducing the angle).
fn sin(x: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let mut x = x % (2.0 * pi);
    if x > pi {
        x -= 2.0 * pi;
    }
    if x < -pi {
        x += 2.0 * pi;
    }
    // Fold into -pi/2..pi/2.
    if x > pi / 2.0 {
        x = pi - x;
    } else if x < -pi / 2.0 {
        x = -pi - x;
    }
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
}

fn cos(x: f32) -> f32 {
    sin(x + core::f32::consts::FRAC_PI_2)
}

/// The pels of an area (row by row from the top).
pub fn extract(f: &Form, a: Area) -> (u32, u32, Vec<u32>) {
    let mut v = Vec::with_capacity((a.w() * a.h()).max(0) as usize);
    for y in a.y0..=a.y1 {
        for x in a.x0..=a.x1 {
            v.push(f.get(x, y));
        }
    }
    (a.w() as u32, a.h() as u32, v)
}

/// Put pels at (x, y) (what falls outside is lost). Colours not in an
/// indexed form's palette become the nearest entry.
pub fn paste(f: &mut Form, x: i32, y: i32, w: u32, h: u32, pels: &[u32]) {
    for yy in 0..h as i32 {
        for xx in 0..w as i32 {
            let p = pels[(yy as u32 * w + xx as u32) as usize];
            let p = fit(f, p);
            f.set(x + xx, y + yy, p);
        }
    }
}

/// Put pels stretched (or shrunk) to fill an area.
pub fn stretch_paste(f: &mut Form, a: Area, w: u32, h: u32, pels: &[u32]) {
    if w == 0 || h == 0 {
        return;
    }
    for yy in 0..a.h() {
        for xx in 0..a.w() {
            let sx = (xx as u32 * w / a.w() as u32).min(w - 1);
            let sy = (yy as u32 * h / a.h() as u32).min(h - 1);
            let p = fit(f, pels[(sy * w + sx) as usize]);
            f.set(a.x0 + xx, a.y0 + yy, p);
        }
    }
}

/// A pel value as the form can hold it.
pub fn fit(f: &Form, p: u32) -> u32 {
    if p & (crate::format::SCREEN | crate::format::INVERSE) != 0 || f.bpp > 8 {
        p
    } else {
        f.palette[f.nearest(p)]
    }
}

/// A new form of another size and colour depth showing the same figure
/// (OS/2's editor "renders" a form added to the device list so).
pub fn render(from: &Form, cx: u32, cy: u32, bpp: u16, display: (u16, u16)) -> Form {
    let mut f = Form::new(cx, cy, bpp, 0);
    f.display = display;
    f.pels = from.scaled(cx, cy);
    for i in 0..f.pels.len() {
        f.pels[i] = fit(&f, f.pels[i]);
    }
    f.hot = ((from.hot.0 * cx as i32) / from.cx.max(1) as i32, (from.hot.1 * cy as i32) / from.cy.max(1) as i32);
    f
}

/// Change a form's palette entry `i` to `rgb`, changing the pels of the
/// old colour with it.
pub fn set_palette(f: &mut Form, i: usize, rgb: u32) {
    let Some(&old) = f.palette.get(i) else { return };
    f.palette[i] = rgb;
    if f.bpp <= 8 && old != rgb && !f.palette.iter().enumerate().any(|(j, &c)| j != i && c == old) {
        for p in f.pels.iter_mut() {
            if *p == old {
                *p = rgb;
            }
        }
    }
}

/// Swap two palette entries: with `preserve` the picture keeps its
/// colours (only the entries' places change), without it the pels of the
/// two colours trade colours.
pub fn swap_colors(f: &mut Form, a: usize, b: usize, preserve: bool) {
    if a >= f.palette.len() || b >= f.palette.len() || a == b {
        return;
    }
    let (ca, cb) = (f.palette[a], f.palette[b]);
    f.palette.swap(a, b);
    if !preserve {
        for p in f.pels.iter_mut() {
            if *p == ca {
                *p = cb;
            } else if *p == cb {
                *p = ca;
            }
        }
    }
}

/// Self-checks of the tools.
pub fn selftest(mut report: impl FnMut(&str, bool)) -> u32 {
    let mut fails = 0;
    let mut check = |name: &str, ok: bool| {
        if !ok {
            fails += 1;
        }
        report(name, ok);
    };
    let mut f = Form::new(16, 16, 4, 0xffffff);
    line(&mut f, (0, 0), (15, 15), 1, 1, 0);
    check("line: diagonal", (0..16).all(|i| f.get(i, i) == 0) && f.get(1, 0) == 0xffffff);
    let n = flood(&mut f, 15, 0, 0xff0000);
    check("flood: one side of the line", n == 120 && f.get(0, 15) == 0xffffff);
    let all = Area::all(&f);
    flip_h(&mut f, all);
    check("flip horizontal", f.get(0, 0) == 0xff0000 && f.get(15, 0) == 0);
    flip_v(&mut f, all);
    check("flip vertical", f.get(15, 15) == 0 && f.get(0, 15) == 0xff0000);
    let mut c = Form::new(11, 11, 4, 0xffffff);
    let all = Area::all(&c);
    circle(&mut c, all, 1, 1, 0);
    check("circle: ends of the axes", c.get(0, 5) == 0 && c.get(10, 5) == 0 && c.get(5, 0) == 0 && c.get(5, 10) == 0 && c.get(5, 5) == 0xffffff);
    let (w, h, v) = extract(&c, Area { x0: 0, y0: 4, x1: 2, y1: 6 });
    let mut d = Form::new(4, 4, 4, 0xffffff);
    paste(&mut d, 1, 1, w, h, &v);
    check("cut and paste", d.get(1, 2) == 0 && d.get(0, 0) == 0xffffff);
    let all = Area::all(&d);
    stretch_paste(&mut d, all, 1, 1, &[0x0000ff]);
    check("stretch paste", d.pels.iter().all(|&p| p == 0x0000ff));
    let r = render(&c, 22, 22, 1, (0, 0));
    check("render to black and white", r.get(0, 10) == 0 && r.get(10, 10) == 0xffffff);
    let mut s = Form::new(2, 1, 4, 0x000080);
    s.set(1, 0, 0x008000);
    swap_colors(&mut s, 1, 2, false);
    check("swap colors", s.get(0, 0) == 0x008000 && s.get(1, 0) == 0x000080);
    set_palette(&mut s, 1, 0x123456);
    check("edit color", s.get(0, 0) == 0x123456 && s.get(1, 0) == 0x000080);
    fails
}
