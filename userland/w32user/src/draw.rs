//! More of GDI's and USER's drawing: arcs, pies and chords, flood fill,
//! poly-polygons, LineDDA, tabbed and grayed text, ScrollDC, and the
//! smaller USER calls programs drawing their own windows use
//! (GetUpdateRgn, deferred window positions, SystemParametersInfo...).

use alloc::vec::Vec;

use crate::gdi;

/// The points of an elliptic arc in the rectangle (l, t, r, b), counter-
/// clockwise (on the screen) from the radial through (x1, y1) to the one
/// through (x2, y2), as Windows draws Arc, Pie and Chord.
pub fn arc_points(l: i32, t: i32, r: i32, b: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
    let (cx, cy) = ((l + r) as f32 / 2.0, (t + b) as f32 / 2.0);
    let (rx, ry) = (((r - l) as f32 / 2.0).abs(), ((b - t) as f32 / 2.0).abs());
    // Angles with y up (counter-clockwise on the screen).
    let a1 = atan2(cy - y1 as f32, x1 as f32 - cx);
    let mut a2 = atan2(cy - y2 as f32, x2 as f32 - cx);
    if a2 <= a1 {
        a2 += 2.0 * PI;
    }
    let steps = (((a2 - a1) / (2.0 * PI)) * (rx + ry).max(8.0) * 1.5) as i32 + 2;
    (0..=steps)
        .map(|i| {
            let a = a1 + (a2 - a1) * i as f32 / steps as f32;
            ((cx + rx * cos(a) + 0.5) as i32, (cy - ry * sin(a) + 0.5) as i32)
        })
        .collect()
}

const PI: f32 = core::f32::consts::PI;

fn sin(x: f32) -> f32 {
    // Range-reduced Taylor series: plenty for pels.
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    } else if x < -PI {
        x += 2.0 * PI;
    }
    let (x2, mut term, mut sum) = (x * x, x, x);
    for n in 1..9 {
        term *= -x2 / ((2 * n) as f32 * (2 * n + 1) as f32);
        sum += term;
    }
    sum
}

fn cos(x: f32) -> f32 {
    sin(x + PI / 2.0)
}

fn atan2(y: f32, x: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    // atan on [-1, 1] by its series after halving the angle twice.
    let atan = |v: f32| {
        let h = v / (1.0 + sqrt(1.0 + v * v));
        let h = h / (1.0 + sqrt(1.0 + h * h));
        let (h2, mut term, mut sum) = (h * h, h, h);
        for n in 1..12 {
            term *= -h2;
            sum += term / (2 * n + 1) as f32;
        }
        4.0 * sum
    };
    if x.abs() >= y.abs() {
        let a = atan(y / x);
        if x > 0.0 { a } else if y >= 0.0 { a + PI } else { a - PI }
    } else {
        let a = atan(x / y);
        if y > 0.0 { PI / 2.0 - a } else { -PI / 2.0 - a }
    }
}

fn sqrt(v: f32) -> f32 {
    let r: f32;
    unsafe { core::arch::asm!("sqrtss {0}, {1}", out(xmm_reg) r, in(xmm_reg) v, options(nostack, pure, nomem)) };
    r
}

/// FloodFill / ExtFloodFill: fill the area around (x, y) that is (mode
/// FLOODFILLSURFACE 1) or is not bounded by (FLOODFILLBORDER 0) the
/// colour, with the DC's brush.
pub fn flood(hdc: u16, x: i32, y: i32, color: u32, surface: bool) -> bool {
    let Some(d) = gdi::dc(hdc) else { return false };
    let (w, h) = gdi::dc_size(hdc);
    let (x, y) = gdi::to_dev(d, x, y);
    if w <= 0 || h <= 0 || x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    // The DC's pels, through a memory copy (24 bits, rows bottom-up).
    let Some(px) = crate::bitmap::read_pels(hdc, w, h) else { return false };
    let at = |x: i32, y: i32| px[((h - 1 - y) * w + x) as usize];
    let target = color & 0xffffff;
    let inside = |c: u32| if surface { c == target } else { c != target };
    if !inside(at(x, y)) {
        return false;
    }
    let mut done = alloc::vec![false; (w * h) as usize];
    let mut stack = alloc::vec![(x, y)];
    let mut runs: Vec<[i32; 4]> = Vec::new();
    while let Some((x, y)) = stack.pop() {
        if done[(y * w + x) as usize] || !inside(at(x, y)) {
            continue;
        }
        let (mut l, mut r) = (x, x);
        while l > 0 && !done[(y * w + l - 1) as usize] && inside(at(l - 1, y)) {
            l -= 1;
        }
        while r + 1 < w && !done[(y * w + r + 1) as usize] && inside(at(r + 1, y)) {
            r += 1;
        }
        for i in l..=r {
            done[(y * w + i) as usize] = true;
        }
        runs.push([l, y, r + 1, y + 1]);
        for ny in [y - 1, y + 1] {
            if ny < 0 || ny >= h {
                continue;
            }
            let mut i = l;
            while i <= r {
                if !done[(ny * w + i) as usize] && inside(at(i, ny)) {
                    stack.push((i, ny));
                    while i <= r && inside(at(i, ny)) {
                        i += 1;
                    }
                }
                i += 1;
            }
        }
    }
    let br = gdi::brush_of(hdc);
    for r in runs {
        gdi::fill_rect_dev(hdc, r, br);
    }
    true
}

/// TabbedTextOut and GetTabbedTextExtent: the text with tabs at the given
/// stops (none: every 8 average widths; one: every that many pels); the
/// extent (width | height << 16).
pub fn tabbed(hdc: u16, x: i32, y: i32, s: &[u8], tabs: &[i32], origin: i32, draw: bool) -> u32 {
    let ave = gdi::ave_width(hdc).max(1);
    let next_tab = |pos: i32| -> i32 {
        match tabs.len() {
            0 => (pos / (8 * ave) + 1) * 8 * ave,
            1 => {
                let t = tabs[0].max(1);
                (pos / t + 1) * t
            }
            _ => tabs.iter().copied().find(|&t| t > pos).unwrap_or(pos + ave),
        }
    };
    let mut pos = 0;
    for (i, part) in s.split(|&c| c == b'\t').enumerate() {
        if i > 0 {
            pos = next_tab(pos + x - origin) - x + origin;
        }
        if draw && !part.is_empty() {
            gdi::text_at(hdc, x + pos, y, part);
        }
        pos += gdi::text_width(hdc, part);
    }
    (gdi::line_height(hdc) as u32) << 16 | pos as u32 & 0xffff
}
