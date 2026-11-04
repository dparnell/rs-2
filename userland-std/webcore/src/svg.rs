//! Inline SVG drawn to an RGBA image: shapes (path, rect, circle, ellipse,
//! line, polyline, polygon) in groups with transforms, filled and stroked
//! with flat colours. Clip paths, masks, gradients, text and `<use>` are
//! not drawn.

use crate::css::Color;
use crate::dom::{Dom, NodeData, NodeId};
use crate::image::Image;
use crate::style::{Paint, Styles};

type Pt = (f32, f32);

#[derive(Clone, Copy)]
struct Matrix([f32; 6]);

impl Matrix {
    const ID: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    fn apply(&self, (x, y): Pt) -> Pt {
        let m = &self.0;
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }
    fn then(&self, o: &Matrix) -> Matrix {
        // `o` is applied first.
        let (a, b) = (&self.0, &o.0);
        Matrix([
            a[0] * b[0] + a[2] * b[1],
            a[1] * b[0] + a[3] * b[1],
            a[0] * b[2] + a[2] * b[3],
            a[1] * b[2] + a[3] * b[3],
            a[0] * b[4] + a[2] * b[5] + a[4],
            a[1] * b[4] + a[3] * b[5] + a[5],
        ])
    }
    fn scale(&self) -> f32 {
        (self.0[0] * self.0[3] - self.0[1] * self.0[2]).abs().sqrt()
    }
}

fn numbers(s: &str) -> Vec<f32> {
    let mut sc = Scan { b: s.as_bytes(), i: 0 };
    let mut v = Vec::new();
    while let Some(n) = sc.number() {
        v.push(n);
    }
    v
}

fn transform(s: &str) -> Matrix {
    let mut m = Matrix::ID;
    for part in s.split(')') {
        let Some((name, args)) = part.split_once('(') else { continue };
        let a = numbers(args);
        let g = |i: usize| a.get(i).copied().unwrap_or(0.0);
        let t = match name.trim().trim_start_matches(',').trim() {
            "matrix" if a.len() == 6 => Matrix([a[0], a[1], a[2], a[3], a[4], a[5]]),
            "translate" => Matrix([1.0, 0.0, 0.0, 1.0, g(0), g(1)]),
            "scale" => Matrix([g(0), 0.0, 0.0, if a.len() > 1 { g(1) } else { g(0) }, 0.0, 0.0]),
            "rotate" => {
                let (s, c) = g(0).to_radians().sin_cos();
                let r = Matrix([c, s, -s, c, 0.0, 0.0]);
                if a.len() == 3 { Matrix([1.0, 0.0, 0.0, 1.0, g(1), g(2)]).then(&r).then(&Matrix([1.0, 0.0, 0.0, 1.0, -g(1), -g(2)])) } else { r }
            }
            _ => continue,
        };
        m = m.then(&t);
    }
    m
}

struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}

impl Scan<'_> {
    fn skip(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b',' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn number(&mut self) -> Option<f32> {
        self.skip();
        let start = self.i;
        let mut j = start;
        if matches!(self.b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let (mut digits, mut dot) = (0, false);
        while let Some(&c) = self.b.get(j) {
            match c {
                b'0'..=b'9' => digits += 1,
                b'.' if !dot => dot = true,
                _ => break,
            }
            j += 1;
        }
        if digits == 0 {
            // transform() lists may hold names between numbers.
            return None;
        }
        if matches!(self.b.get(j), Some(b'e' | b'E')) {
            let mut k = j + 1;
            if matches!(self.b.get(k), Some(b'+' | b'-')) {
                k += 1;
            }
            if self.b.get(k).is_some_and(|c| c.is_ascii_digit()) {
                while self.b.get(k).is_some_and(|c| c.is_ascii_digit()) {
                    k += 1;
                }
                j = k;
            }
        }
        self.i = j;
        std::str::from_utf8(&self.b[start..j]).ok()?.parse().ok()
    }
    /// An arc's flag: one digit, which may run into what follows.
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let c = *self.b.get(self.i)?;
        self.i += 1;
        Some(c == b'1')
    }
}

/// Polylines of a path; each sub-path says whether it was closed.
#[derive(Default)]
struct Shape {
    subs: Vec<(Vec<Pt>, bool)>,
}

impl Shape {
    fn start(&mut self, p: Pt) {
        self.subs.push((vec![p], false));
    }
    fn line(&mut self, p: Pt) {
        if self.subs.is_empty() {
            self.start(p);
        }
        self.subs.last_mut().unwrap().0.push(p);
    }
    fn cubic(&mut self, a: Pt, b: Pt, c: Pt, d: Pt, tol: f32) {
        let span = (b.0 - a.0).abs() + (b.1 - a.1).abs() + (c.0 - b.0).abs() + (c.1 - b.1).abs() + (d.0 - c.0).abs() + (d.1 - c.1).abs();
        let n = ((span * tol).sqrt().ceil() as usize + 1).clamp(2, 48);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.line((w0 * a.0 + w1 * b.0 + w2 * c.0 + w3 * d.0, w0 * a.1 + w1 * b.1 + w2 * c.1 + w3 * d.1));
        }
    }
    fn arc(&mut self, from: Pt, rx: f32, ry: f32, rot: f32, large: bool, sweep: bool, to: Pt, tol: f32) {
        let (mut rx, mut ry) = (rx.abs(), ry.abs());
        if rx < 1e-6 || ry < 1e-6 || from == to {
            self.line(to);
            return;
        }
        // SVG's endpoint form to centre form (implementation notes F.6.5).
        let (s, c) = rot.to_radians().sin_cos();
        let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
        let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
        let l = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
        if l > 1.0 {
            rx *= l.sqrt();
            ry *= l.sqrt();
        }
        let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let k = if den > 0.0 { (num / den).sqrt() } else { 0.0 } * if large == sweep { -1.0 } else { 1.0 };
        let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
        let (cx, cy) = (c * cx1 - s * cy1 + (from.0 + to.0) / 2.0, s * cx1 + c * cy1 + (from.1 + to.1) / 2.0);
        let a0 = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
        let a1 = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx);
        let mut da = a1 - a0;
        let tau = std::f32::consts::TAU;
        if sweep && da < 0.0 {
            da += tau;
        } else if !sweep && da > 0.0 {
            da -= tau;
        }
        let n = (((rx.max(ry) * tol).sqrt() * da.abs()).ceil() as usize + 1).clamp(2, 64);
        for i in 1..=n {
            let a = a0 + da * i as f32 / n as f32;
            let (sa, ca) = a.sin_cos();
            let (x, y) = (rx * ca, ry * sa);
            self.line(if i == n { to } else { (c * x - s * y + cx, s * x + c * y + cy) });
        }
    }
    fn close(&mut self) {
        if let Some(s) = self.subs.last_mut() {
            s.1 = true;
        }
    }
}

fn path(d: &str, tol: f32) -> Shape {
    let mut sh = Shape::default();
    let mut sc = Scan { b: d.as_bytes(), i: 0 };
    let (mut cur, mut start, mut last_ctrl): (Pt, Pt, Pt) = ((0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let mut cmd = 0u8;
    let mut prev = 0u8;
    loop {
        sc.skip();
        match sc.b.get(sc.i) {
            None => break,
            Some(&c) if c.is_ascii_alphabetic() => {
                cmd = c;
                sc.i += 1;
            }
            Some(_) if cmd == 0 => break,
            Some(_) => {}
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { cur } else { (0.0, 0.0) };
        macro_rules! num {
            () => {
                match sc.number() {
                    Some(n) => n,
                    None => return sh,
                }
            };
        }
        macro_rules! pt {
            () => {{
                let x = num!();
                let y = num!();
                (x + ox, y + oy)
            }};
        }
        match cmd.to_ascii_uppercase() {
            b'M' => {
                cur = pt!();
                start = cur;
                sh.start(cur);
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                cur = pt!();
                sh.line(cur);
            }
            b'H' => {
                cur.0 = num!() + ox;
                sh.line(cur);
            }
            b'V' => {
                cur.1 = num!() + oy;
                sh.line(cur);
            }
            b'C' => {
                let (a, b, c) = (pt!(), pt!(), pt!());
                sh.cubic(cur, a, b, c, tol);
                last_ctrl = b;
                cur = c;
            }
            b'S' => {
                let a = if matches!(prev.to_ascii_uppercase(), b'C' | b'S') { (2.0 * cur.0 - last_ctrl.0, 2.0 * cur.1 - last_ctrl.1) } else { cur };
                let (b, c) = (pt!(), pt!());
                sh.cubic(cur, a, b, c, tol);
                last_ctrl = b;
                cur = c;
            }
            b'Q' | b'T' => {
                let q = if cmd.eq_ignore_ascii_case(&b'Q') {
                    pt!()
                } else if matches!(prev.to_ascii_uppercase(), b'Q' | b'T') {
                    (2.0 * cur.0 - last_ctrl.0, 2.0 * cur.1 - last_ctrl.1)
                } else {
                    cur
                };
                let e = pt!();
                let third = |a: Pt, b: Pt| (a.0 + 2.0 / 3.0 * (b.0 - a.0), a.1 + 2.0 / 3.0 * (b.1 - a.1));
                sh.cubic(cur, third(cur, q), third(e, q), e, tol);
                last_ctrl = q;
                cur = e;
            }
            b'A' => {
                let (rx, ry, rot) = (num!(), num!(), num!());
                let (Some(large), Some(sweep)) = (sc.flag(), sc.flag()) else { return sh };
                let e = pt!();
                sh.arc(cur, rx, ry, rot, large, sweep, e, tol);
                cur = e;
            }
            b'Z' => {
                sh.close();
                cur = start;
                prev = cmd;
                cmd = 0;
                // Further drawing starts a new sub-path at the same point.
                sc.skip();
                if sc.b.get(sc.i).is_some_and(|c| !matches!(c, b'M' | b'm')) {
                    sh.start(cur);
                }
                continue;
            }
            _ => return sh,
        }
        prev = cmd;
    }
    sh
}

fn ellipse(sh: &mut Shape, cx: f32, cy: f32, rx: f32, ry: f32, tol: f32) {
    sh.start((cx + rx, cy));
    sh.arc((cx + rx, cy), rx, ry, 0.0, false, true, (cx - rx, cy), tol);
    sh.arc((cx - rx, cy), rx, ry, 0.0, false, true, (cx + rx, cy), tol);
    sh.close();
}

/// A shape element's outline, or None if it is no shape.
fn outline(name: &str, a: &dyn Fn(&str) -> f32, attr: &dyn Fn(&str) -> Option<String>, tol: f32) -> Option<Shape> {
    let mut sh = Shape::default();
    match name {
        "path" => return Some(path(&attr("d")?, tol)),
        "circle" => ellipse(&mut sh, a("cx"), a("cy"), a("r"), a("r"), tol),
        "ellipse" => ellipse(&mut sh, a("cx"), a("cy"), a("rx"), a("ry"), tol),
        "line" => {
            sh.start((a("x1"), a("y1")));
            sh.line((a("x2"), a("y2")));
        }
        "polyline" | "polygon" => {
            let v = numbers(&attr("points")?);
            for (i, p) in v.chunks_exact(2).enumerate() {
                if i == 0 { sh.start((p[0], p[1])) } else { sh.line((p[0], p[1])) }
            }
            if name == "polygon" {
                sh.close();
            }
        }
        "rect" => {
            let (x, y, w, h) = (a("x"), a("y"), a("width"), a("height"));
            let (rx, ry) = (attr("rx").map(|_| a("rx")), attr("ry").map(|_| a("ry")));
            let (rx, ry) = (rx.or(ry).unwrap_or(0.0).min(w / 2.0), ry.or(rx).unwrap_or(0.0).min(h / 2.0));
            if rx > 0.0 && ry > 0.0 {
                sh.start((x + rx, y));
                sh.line((x + w - rx, y));
                sh.arc((x + w - rx, y), rx, ry, 0.0, false, true, (x + w, y + ry), tol);
                sh.line((x + w, y + h - ry));
                sh.arc((x + w, y + h - ry), rx, ry, 0.0, false, true, (x + w - rx, y + h), tol);
                sh.line((x + rx, y + h));
                sh.arc((x + rx, y + h), rx, ry, 0.0, false, true, (x, y + h - ry), tol);
                sh.line((x, y + ry));
                sh.arc((x, y + ry), rx, ry, 0.0, false, true, (x + rx, y), tol);
            } else {
                sh.start((x, y));
                sh.line((x + w, y));
                sh.line((x + w, y + h));
                sh.line((x, y + h));
            }
            sh.close();
        }
        _ => return None,
    }
    Some(sh)
}

const SUB: usize = 4;

/// Fill polygons into `img` with `color`: four scanlines per row, exact
/// coverage along each.
fn fill(img: &mut Image, polys: &[Vec<Pt>], color: Color, evenodd: bool) {
    let (w, h) = (img.w as usize, img.h as usize);
    let mut edges: Vec<(Pt, Pt, i32)> = Vec::new();
    for p in polys {
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if a.1 < b.1 {
                edges.push((a, b, 1));
            } else if a.1 > b.1 {
                edges.push((b, a, -1));
            }
        }
    }
    let mut cover = vec![0f32; w];
    let mut xs: Vec<(f32, i32)> = Vec::new();
    for y in 0..h {
        let mut any = false;
        for s in 0..SUB {
            let sy = y as f32 + (s as f32 + 0.5) / SUB as f32;
            xs.clear();
            for (a, b, wind) in &edges {
                if a.1 <= sy && sy < b.1 {
                    xs.push((a.0 + (sy - a.1) * (b.0 - a.0) / (b.1 - a.1), *wind));
                }
            }
            xs.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut n = 0;
            for i in 0..xs.len().saturating_sub(1) {
                n += xs[i].1;
                let inside = if evenodd { (i + 1) % 2 == 1 } else { n != 0 };
                if !inside {
                    continue;
                }
                let (x0, x1) = (xs[i].0.clamp(0.0, w as f32), xs[i + 1].0.clamp(0.0, w as f32));
                if x1 <= x0 {
                    continue;
                }
                any = true;
                let (i0, i1) = (x0 as usize, (x1.ceil() as usize).min(w));
                for (x, c) in cover.iter_mut().enumerate().take(i1).skip(i0) {
                    *c += ((x as f32 + 1.0).min(x1) - (x as f32).max(x0)) / SUB as f32;
                }
            }
        }
        if !any {
            continue;
        }
        let ca = (color >> 24) as f32 / 255.0;
        for (x, c) in cover.iter_mut().enumerate() {
            if *c > 0.0 {
                let a = (*c).min(1.0) * ca;
                let px = &mut img.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                let da = px[3] as f32 / 255.0;
                let oa = a + da * (1.0 - a);
                let mix = |s: f32, d: f32| ((s * a + d * da * (1.0 - a)) / oa).round() as u8;
                px[0] = mix((color >> 16 & 255) as f32, px[0] as f32);
                px[1] = mix((color >> 8 & 255) as f32, px[1] as f32);
                px[2] = mix((color & 255) as f32, px[2] as f32);
                px[3] = (oa * 255.0).round() as u8;
                *c = 0.0;
            }
        }
    }
}

fn stroke_polys(sh: &Shape, width: f32) -> Vec<Vec<Pt>> {
    let r = width / 2.0;
    let mut out = Vec::new();
    for (p, closed) in &sh.subs {
        let n = p.len();
        for i in 0..if *closed { n } else { n.saturating_sub(1) } {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = (dx * dx + dy * dy).sqrt();
            if l < 1e-4 {
                continue;
            }
            let (nx, ny) = (-dy / l * r, dx / l * r);
            // Wound the same way as the discs at the joins.
            out.push(vec![(a.0 + nx, a.1 + ny), (a.0 - nx, a.1 - ny), (b.0 - nx, b.1 - ny), (b.0 + nx, b.1 + ny)]);
            if r > 0.6 {
                let joint: Vec<Pt> = (0..8)
                    .map(|k| {
                        let (s, c) = (k as f32 * std::f32::consts::TAU / 8.0).sin_cos();
                        (b.0 + c * r, b.1 + s * r)
                    })
                    .collect();
                out.push(joint);
            }
        }
    }
    out
}

struct Ctx<'a> {
    dom: &'a Dom,
    styles: &'a Styles,
    tol: f32,
}

fn draw(c: &Ctx, n: NodeId, m: Matrix, img: &mut Image) {
    for &k in &c.dom.nodes[n].children {
        let NodeData::Element(e) = &c.dom.nodes[k].data else { continue };
        let Some(st) = c.styles.of[k].as_ref() else { continue };
        if st.display == crate::style::Display::None || st.invisible {
            continue;
        }
        let name: &str = &e.name.local;
        let m = e.attr("transform").map_or(m, |t| m.then(&transform(t)));
        match name {
            "defs" | "clipPath" | "mask" | "symbol" | "linearGradient" | "radialGradient" | "pattern" | "title" | "desc" | "text" | "use" | "image" | "foreignObject" | "filter" | "style" => continue,
            "g" | "a" | "switch" | "svg" => {
                draw(c, k, m, img);
                continue;
            }
            _ => {}
        }
        let a = |n: &str| e.attr(n).and_then(|v| numbers(v).first().copied()).unwrap_or(0.0);
        let attr = |n: &str| e.attr(n).map(String::from);
        let Some(mut sh) = outline(name, &a, &attr, c.tol * m.scale().max(0.1)) else { continue };
        for (p, _) in &mut sh.subs {
            for q in p.iter_mut() {
                *q = m.apply(*q);
            }
        }
        let paint = |p: Paint| match p {
            Paint::None => None,
            Paint::Color(c) => Some(c),
            Paint::Current => Some(st.color),
        };
        if let Some(col) = paint(st.fill) {
            let polys: Vec<Vec<Pt>> = sh.subs.iter().map(|(p, _)| p.clone()).collect();
            fill(img, &polys, col, st.fill_evenodd);
        }
        if let Some(col) = paint(st.stroke) {
            fill(img, &stroke_polys(&sh, st.stroke_width * m.scale()), col, false);
        }
    }
}

/// The natural size of an `<svg>`: from width and height, else its viewBox.
pub fn natural_size(dom: &Dom, n: NodeId) -> (f32, f32) {
    let e = dom.element(n).unwrap();
    let vb = e.attr("viewBox").map(numbers).filter(|v| v.len() == 4);
    let attr = |a: &str| e.attr(a).filter(|v| !v.contains('%')).and_then(|v| numbers(v).first().copied());
    match (attr("width"), attr("height"), &vb) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(v)) if v[2] > 0.0 => (w, w * v[3] / v[2]),
        (None, Some(h), Some(v)) if v[3] > 0.0 => (h * v[2] / v[3], h),
        (None, None, Some(v)) => (v[2], v[3]),
        _ => (300.0, 150.0),
    }
}

/// Draw the `<svg>` element `n` at `w` x `h` pels.
pub fn render(dom: &Dom, styles: &Styles, n: NodeId, w: u32, h: u32) -> Image {
    let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
    let mut img = Image { w, h, rgba: vec![0; (w * h * 4) as usize] };
    let e = dom.element(n).unwrap();
    let (nw, nh) = natural_size(dom, n);
    let vb = e.attr("viewBox").map(numbers).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).unwrap_or(vec![0.0, 0.0, nw, nh]);
    // preserveAspectRatio's default: fit and centre.
    let s = (w as f32 / vb[2]).min(h as f32 / vb[3]);
    let (tx, ty) = ((w as f32 - vb[2] * s) / 2.0 - vb[0] * s, (h as f32 - vb[3] * s) / 2.0 - vb[1] * s);
    let m = Matrix([s, 0.0, 0.0, s, tx, ty]);
    draw(&Ctx { dom, styles, tol: 2.0 }, n, m, &mut img);
    img
}
