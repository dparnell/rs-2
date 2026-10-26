//! More of GDI's and USER's drawing: arcs, pies and chords, flood fill,
//! poly-polygons, LineDDA, tabbed and grayed text, ScrollDC, and the
//! smaller USER calls programs drawing their own windows use
//! (GetUpdateRgn, deferred window positions, SystemParametersInfo...).

use alloc::vec::Vec;
use thunk16::{flat, Pascal, PascalCall};

use crate::gdi;
use crate::pm::*;

/// The points of an elliptic arc in the rectangle (l, t, r, b), counter-
/// clockwise (on the screen) from the radial through (x1, y1) to the one
/// through (x2, y2), as Windows draws Arc, Pie and Chord.
fn arc_points(l: i32, t: i32, r: i32, b: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
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
fn flood(hdc: u16, x: i32, y: i32, color: u32, surface: bool) -> bool {
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
fn tabbed(hdc: u16, x: i32, y: i32, s: &[u8], tabs: &[i32], origin: i32, draw: bool) -> u32 {
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

static DEFER: crate::Global<u16> = crate::Global::new(0x7a00);

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // SetPolyFillMode(hdc, mode), GetPolyFillMode(hdc)
        3006 => {
            let (h, m) = (a.w(), a.w());
            gdi::set_poly_fill_mode(h, m) as u32
        }
        3084 => gdi::poly_fill_mode(a.w()) as u32,
        // SetTextCharacterExtra(hdc, n), GetTextCharacterExtra(hdc),
        // SetTextJustification(hdc, extra, breaks): no extra spacing.
        3008 => {
            let (_h, _n) = (a.w(), a.i());
            0
        }
        3089 => {
            a.w();
            0
        }
        3010 => {
            let (_h, _e, _b) = (a.w(), a.i(), a.i());
            1
        }
        // Arc, Pie, Chord(hdc, l, t, r, b, x1, y1, x2, y2)
        3023 | 3026 | 3348 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            let (x1, y1, x2, y2) = (a.i(), a.i(), a.i(), a.i());
            let mut pts = arc_points(l, t, r, b, x1, y1, x2, y2);
            (match ord {
                3023 => gdi::poly(h, &pts, false, false),
                3026 => {
                    pts.insert(0, ((l + r) / 2, (t + b) / 2));
                    gdi::poly(h, &pts, true, true)
                }
                _ => gdi::poly(h, &pts, true, true),
            }) as u32
        }
        // FloodFill(hdc, x, y, border), ExtFloodFill(hdc, x, y, color, type)
        3025 => {
            let (h, x, y, c) = (a.w(), a.i(), a.i(), a.l());
            flood(h, x, y, c, false) as u32
        }
        3372 => {
            let (h, x, y, c, t) = (a.w(), a.i(), a.i(), a.l(), a.w());
            flood(h, x, y, c, t == 1) as u32
        }
        // PolyPolygon(hdc, points, counts, n)
        3450 => {
            let (h, p, c, n) = (a.w(), a.p(), a.p(), a.i());
            if p.is_null() || c.is_null() {
                return Some(0);
            }
            let mut at = 0usize;
            let mut polys = Vec::new();
            for i in 0..n.max(0) as usize {
                let k = unsafe { (c.add(2 * i) as *const i16).read_unaligned() } as usize;
                let pts: Vec<(i32, i32)> = (0..k)
                    .map(|j| unsafe { ((p.add(4 * (at + j)) as *const i16).read_unaligned() as i32, (p.add(4 * (at + j) + 2) as *const i16).read_unaligned() as i32) })
                    .collect();
                at += k;
                polys.push(pts);
            }
            gdi::poly_poly(h, &polys) as u32
        }
        // LineDDA(x1, y1, x2, y2, proc, data): the procedure gets each
        // point of the line but the last.
        3100 => {
            let (x1, y1, x2, y2, p, data) = (a.i(), a.i(), a.i(), a.i(), a.l(), a.l());
            let (dx, dy) = ((x2 - x1).abs(), -(y2 - y1).abs());
            let (sx, sy) = (if x1 < x2 { 1 } else { -1 }, if y1 < y2 { 1 } else { -1 });
            let (mut x, mut y, mut err) = (x1, y1, dx + dy);
            while (x, y) != (x2, y2) {
                PascalCall::new().w(x as u16).w(y as u16).l(data).call(p);
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
            0
        }
        // OffsetClipRgn(hdc, dx, dy)
        3032 => {
            let (_h, _x, _y) = (a.w(), a.i(), a.i());
            2
        }
        // IsGDIObject(h)
        3462 => gdi::obj(a.w()).is_some() as u32,
        // GetBitmapDimension(hbm), SetBitmapDimension(hbm, x, y) and the Ex
        // forms: none kept.
        3162 => {
            a.w();
            0
        }
        3163 => {
            let (_h, _x, _y) = (a.w(), a.i(), a.i());
            0
        }
        3468 | 3478 => {
            let h = a.w();
            if ord == 3478 {
                a.i();
                a.i();
            }
            let p = a.p();
            if !p.is_null() {
                unsafe { (p as *mut u32).write_unaligned(0) };
            }
            (h != 0) as u32
        }
        // GetCharABCWidths(hdc, first, last, lpabc): TrueType fonts only.
        3307 => {
            let (_h, _f, _l, _p) = (a.w(), a.w(), a.w(), a.l());
            0
        }
        // GetRasterizerCaps(lpcaps, size): TrueType available and enabled.
        3313 => {
            let (p, n) = (a.p(), a.i());
            if !p.is_null() && n >= 6 {
                unsafe {
                    (p as *mut u16).write_unaligned(6);
                    (p.add(2) as *mut u16).write_unaligned(3);
                    (p.add(4) as *mut u16).write_unaligned(0);
                }
            }
            1
        }
        // GrayString(hdc, hbr, proc, data, n, x, y, cx, cy): the text (or
        // what the procedure draws) in the gray text colour.
        2185 => {
            let (h, _br, p, data, n, x, y, _cx, _cy) = (a.w(), a.w(), a.l(), a.l(), a.i(), a.i(), a.i(), a.i(), a.i());
            let old = gdi::set_text_color(h, gdi::sys_color(gdi::COLOR_GRAYTEXT));
            let r = if p != 0 {
                PascalCall::new().w(h).l(data).w(n as u16).call(p) as u16 != 0
            } else {
                let s = crate::far_str(data).unwrap_or(b"");
                let s = if n > 0 { &s[..(n as usize).min(s.len())] } else { s };
                gdi::text_at(h, x, y, s);
                true
            };
            gdi::set_text_color(h, old);
            r as u32
        }
        // TabbedTextOut(hdc, x, y, text, n, ntabs, tabs, origin),
        // GetTabbedTextExtent(hdc, text, n, ntabs, tabs)
        2196 | 2197 => {
            let h = a.w();
            let (x, y) = if ord == 2196 { (a.i(), a.i()) } else { (0, 0) };
            let (s, n, nt, tp) = (a.l(), a.i(), a.i(), a.p());
            let origin = if ord == 2196 { a.i() } else { 0 };
            let text = if n > 0 { unsafe { core::slice::from_raw_parts(flat(s), n as usize) } } else { &[] };
            let tabs: Vec<i32> = if tp.is_null() { Vec::new() } else { (0..nt.max(0) as usize).map(|i| unsafe { (tp.add(2 * i) as *const i16).read_unaligned() } as i32).collect() };
            tabbed(h, x, y, text, &tabs, origin, ord == 2196)
        }
        // ScrollDC(hdc, dx, dy, scroll, clip, hrgnUpdate, lprcUpdate)
        2221 => {
            let (h, dx, dy, sr, cr, rgn, up) = (a.w(), a.i(), a.i(), a.l(), a.l(), a.w(), a.l());
            let Some(r) = crate::read_rect(sr).or_else(|| { let (w, hh) = gdi::dc_size(h); Some([0, 0, w, hh]) }) else { return Some(0) };
            let clip = crate::read_rect(cr).unwrap_or(r);
            let src = [r[0].max(clip[0] - dx), r[1].max(clip[1] - dy), r[2].min(clip[2] - dx), r[3].min(clip[3] - dy)];
            if src[2] > src[0] && src[3] > src[1] {
                crate::bitmap::blit(h, src[0] + dx, src[1] + dy, src[2] - src[0], src[3] - src[1], h, src[0], src[1], 0x00cc_0020);
            }
            // What was uncovered.
            let moved = [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy];
            let mut upd = crate::region::combine(&alloc::vec![r], &alloc::vec![moved], 4);
            upd = crate::region::combine(&upd, &alloc::vec![clip], 1);
            if rgn != 0 {
                crate::region::set(rgn, upd.clone());
            }
            let b = upd.iter().fold([0; 4], |b, q| if b == [0; 4] { *q } else { [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[2]), b[3].max(q[3])] });
            crate::write_rect(up, b);
            1
        }
        // GetUpdateRgn(hwnd, hrgn, erase): the update rectangle.
        2237 => {
            let (hw, rgn, _e) = (a.w(), a.w(), a.w());
            let r = crate::user::update_rect(hw);
            crate::region::set(rgn, r.map_or(Vec::new(), |r| alloc::vec![r]))
        }
        // ExcludeUpdateRgn(hdc, hwnd)
        2238 => {
            let (h, hw) = (a.w(), a.w());
            if let Some(r) = crate::user::update_rect(hw) {
                if let Some(d) = gdi::dc(h) {
                    let rc = gdi::pm_rect_dev(d, r);
                    unsafe { GpiExcludeClipRectangle(d.hps, &rc) };
                    d.clipped = true;
                }
            }
            2
        }
        // LockWindowUpdate(hwnd)
        2294 => {
            a.w();
            1
        }
        // GetDCEx(hwnd, hrgnClip, flags): the client area's DC.
        2359 => {
            let (hw, _r, fl) = (a.w(), a.w(), a.l());
            crate::user::get_dc(hw) as u32
        }
        // BeginDeferWindowPos(n), DeferWindowPos(hdwp, hwnd, after, x, y,
        // cx, cy, flags), EndDeferWindowPos(hdwp): the positions are set at
        // once.
        2259 => {
            a.i();
            let d = DEFER.get();
            *d = d.wrapping_add(2);
            *d as u32
        }
        2260 => {
            let (hd, h, after, x, y, cx, cy, fl) = (a.w(), a.w(), a.w(), a.i(), a.i(), a.i(), a.i(), a.w());
            crate::user::set_window_pos(h, after, x, y, cx, cy, fl);
            hd as u32
        }
        2261 => {
            a.w();
            1
        }
        // ShowOwnedPopups(hwnd, show)
        2265 => {
            let (h, show) = (a.w(), a.w());
            for w in crate::user::owned_popups(h) {
                crate::user::show_window(w, if show != 0 { 5 } else { 0 });
            }
            1
        }
        // AnyPopup()
        2052 => crate::user::top_levels().iter().any(|&w| crate::user::wnd(w).is_some_and(|x| x.style & 0x9000_0000 == 0x9000_0000)) as u32,
        // CloseWindow(hwnd): minimised; OpenIcon(hwnd): restored.
        2043 => {
            let h = a.w();
            crate::user::show_window(h, 6);
            1
        }
        2044 => {
            let h = a.w();
            crate::user::show_window(h, 9);
            1
        }
        // ExitWindows(code, reserved): Windows programs do not end the
        // system.
        2007 => {
            let (_c, _r) = (a.l(), a.w());
            0
        }
        // EnumProps(hwnd, proc): the procedure gets each property's
        // atom (as MAKEINTATOM) and data.
        2027 => {
            let (h, p) = (a.w(), a.l());
            let props = crate::user::wnd(h).map_or(Vec::new(), |w| w.props.clone());
            let mut r = 0xffff;
            for (atom, data) in props {
                r = PascalCall::new().w(h).l(atom as u32).w(data).call(p);
                if r as u16 == 0 {
                    break;
                }
            }
            r as u16 as u32
        }
        // SystemParametersInfo(action, param, pv, winini)
        2483 => {
            let (act, _param, pv, _ini) = (a.w(), a.w(), a.p(), a.w());
            let put16 = |v: u16| {
                if !pv.is_null() {
                    unsafe { (pv as *mut u16).write_unaligned(v) };
                }
            };
            match act {
                1 => put16(1),                // SPI_GETBEEP
                5 => put16(3),                // SPI_GETBORDER
                10 => put16(31),              // SPI_GETKEYBOARDSPEED
                13 | 24 => put16(75),         // SPI_ICONHORIZONTALSPACING, VERTICAL
                14 => put16(0),               // SPI_GETSCREENSAVETIMEOUT
                16 => put16(0),               // SPI_GETSCREENSAVEACTIVE
                18 => put16(0),               // SPI_GETGRIDGRANULARITY
                22 => put16(1),               // SPI_GETKEYBOARDDELAY
                25 => put16(1),               // SPI_GETICONTITLEWRAP
                27 => put16(0),               // SPI_GETMENUDROPALIGNMENT
                31 => {
                    // SPI_GETICONTITLELOGFONT: 8 point MS Sans Serif.
                    if !pv.is_null() {
                        unsafe {
                            core::ptr::write_bytes(pv, 0, 50);
                            (pv as *mut i16).write_unaligned(-11);
                            (pv.add(8) as *mut i16).write_unaligned(400);
                            core::ptr::copy_nonoverlapping(b"MS Sans Serif".as_ptr(), pv.add(18), 13);
                        }
                    }
                }
                35 => put16(1), // SPI_GETFASTTASKSWITCH
                _ => {}
            }
            1
        }
        // GetMessageExtraInfo, GetTimerResolution (microseconds),
        // GetSysModalWindow, QuerySendMessage, GetOpenClipboardWindow
        2288 | 2189 | 2184 | 2248 => 0,
        2014 => 1000,
        // ReplyMessage(result)
        2115 => {
            a.l();
            0
        }
        // SetSysColors(n, indexes, colours), SwapMouseButton(swap),
        // SetSysModalWindow(hwnd)
        2181 => {
            let (_n, _i, _c) = (a.i(), a.l(), a.l());
            0
        }
        2186 | 2188 => {
            a.w();
            0
        }
        // GetQueueStatus(flags), GetInputState(): what is waiting.
        2334 | 2335 => {
            if ord == 2334 {
                a.w();
            }
            crate::user::input_waiting() as u32
        }
        _ => return None,
    })
}
