//! Circular sliders (WC_CIRCULARSLIDER, "#65"): a dial with tick marks
//! around it, an indicator for the value, the value as a number, − and +
//! buttons and the window text below; CSM_* messages and CSN_*
//! notifications. As IBM's PMCTLS dial (DialWndProc): the value range
//! is laid out over 180 degrees (the top half, left to right), 252 with
//! CSS_NOBUTTON (or CSS_NOTICKS | CSS_CIRCULARVALUE) or 360 with CSS_360
//! (from the bottom, clockwise), in steps of 6 degrees; a click on the
//! dial or its ticks moves the value towards the point (one scroll
//! increment every 50 ms; CSS_POINTSELECT jumps there and follows the
//! pointer), the buttons step it (1, then the scroll increment every
//! 100 ms), keys step it by the scroll increment (VK_LEFT down, other
//! virtual keys up). WinRegisterCircularSlider (PMCTLS) only says yes:
//! the class is built in, as IBM's PMCTLS registers it when it loads.

use gpi::{Point, Rect};

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc, WinStartTimer, WinStopTimer};
use crate::state::st;
use crate::win::{WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_MOUSEMOVE, WM_PAINT, WM_SETFOCUS, WM_SIZE, WM_TIMER};

const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_SETWINDOWPARAMS: u32 = 0x000a;
const WM_SYSCOLORCHANGE: u32 = 0x002b;
const WM_PRESPARAMCHANGED: u32 = 0x002f;
const WM_BUTTON2DOWN: u32 = 0x0074;
const WM_BUTTON2UP: u32 = 0x0075;
const WM_BUTTON2DBLCLK: u32 = 0x0076;

const CSM_QUERYRANGE: u32 = 0x053d;
const CSM_SETRANGE: u32 = 0x053e;
const CSM_QUERYVALUE: u32 = 0x053f;
const CSM_SETVALUE: u32 = 0x0540;
const CSM_QUERYRADIUS: u32 = 0x0541;
const CSM_SETINCREMENT: u32 = 0x0542;
const CSM_QUERYINCREMENT: u32 = 0x0543;
const CSM_SETBITMAPDATA: u32 = 0x0544;
const CSN_SETFOCUS: u32 = 0x0548;
const CSN_CHANGED: u32 = 0x0549;
const CSN_TRACKING: u32 = 0x054a;
const CSN_QUERYBACKGROUNDCOLOR: u32 = 0x054b;

const CSS_NOBUTTON: u32 = 0x0001;
const CSS_NOTEXT: u32 = 0x0002;
const CSS_NONUMBER: u32 = 0x0004;
const CSS_POINTSELECT: u32 = 0x0008;
const CSS_360: u32 = 0x0010;
const CSS_MIDPOINT: u32 = 0x0020;
const CSS_PROPORTIONALTICKS: u32 = 0x0040;
const CSS_NOTICKS: u32 = 0x0080;
const CSS_CIRCULARVALUE: u32 = 0x0100;

// State bits (IBM's).
const MINUS: u32 = 0x01;
const PLUS: u32 = 0x02;
const TRACK: u32 = 0x04;
const TIMER: u32 = 0x20;
const DRAG: u32 = 0x80;

const TID_DIAL: u32 = 1;

wndproc! {
    csl_entry => csl_proc;
}

struct Dial {
    min: i32,
    max: i32,
    value: i32,
    /// Where tracking moves the value to.
    target: i32,
    /// Scroll increment (buttons, keys, tracking) and tick increment.
    inc: i32,
    tick: i32,
    /// Steps of 6 degrees the range covers.
    span: i32,
    state: u32,
    /// Timer ticks so far and how many to wait before stepping.
    count: u32,
    wait: u32,
    /// CSM_SETBITMAPDATA: − up, − down, + up, + down.
    bitmaps: Option<[u32; 4]>,
    /// Which button went down (1 or 2).
    button: u32,
}

fn data(h: u32) -> Option<&'static mut Dial> {
    ctl::data::<Dial>(h)
}

/// 100 sin(k * 6 degrees) for k in 0..=15.
const SIN: [i32; 16] = [0, 10, 21, 31, 41, 50, 59, 67, 74, 81, 87, 91, 95, 98, 99, 100];

fn sin6(k: i32) -> i32 {
    let k = k.rem_euclid(60);
    match k {
        0..=15 => SIN[k as usize],
        16..=30 => SIN[(30 - k) as usize],
        31..=45 => -SIN[(k - 30) as usize],
        _ => -SIN[(60 - k) as usize],
    }
}

/// Direction of step `idx` (0 points down, 15 left, 30 up, 45 right),
/// times 100.
fn dir(idx: i32) -> (i32, i32) {
    (-sin6(idx), -sin6(idx + 15))
}

/// The layout: centre, radius, button size (and half of it for
/// proportional ticks), the font's line height.
struct Geo {
    c: Point,
    r: i32,
    bs: i32,
    bsh: i32,
    line: i32,
}

fn geo(h: u32) -> Geo {
    let (cx, cy) = ctl::size(h);
    let style = ctl::style(h);
    let line = draw::font_of(h).height;
    let avail = if style & CSS_NOTEXT == 0 { cy - line } else { cy };
    let d = cx.min(avail);
    let (bs, bsh) = if style & CSS_PROPORTIONALTICKS != 0 {
        let b = (d / 2 / 3).clamp(8, 20);
        (b, (b / 2).max(3))
    } else {
        (16, 0)
    };
    let r = (d / 2 - bs - 1).max(1);
    let mut y = bs + r;
    if style & CSS_NOTEXT == 0 {
        y += line;
    }
    Geo { c: Point::new(cx / 2, y), r, bs, bsh, line }
}

/// The step of a value (DialGetPoint).
fn index(d: &Dial, v: i32) -> i32 {
    let first = 30 - d.span / 2;
    if d.max == d.min {
        return first;
    }
    first + (v - d.min) * d.span / (d.max - d.min)
}

fn point_at(d: &Dial, c: Point, v: i32, dist: i32) -> Point {
    let (x, y) = dir(index(d, v));
    Point::new(c.x + dist * x / 100, c.y + dist * y / 100)
}

/// The − and + buttons (odd-sized squares beside the dial's foot).
fn buttons(h: u32, g: &Geo) -> Option<(Rect, Rect)> {
    if ctl::style(h) & CSS_NOBUTTON != 0 {
        return None;
    }
    let s = if (g.bs - 1) & 1 != 0 { g.bs - 1 } else { g.bs };
    let bottom = g.c.y - g.r - g.bs / 2;
    let left = g.c.x - g.r - g.bs;
    let right = g.c.x + g.bs + g.r - s + 1;
    Some((Rect::new(left, bottom, left + s, bottom + s), Rect::new(right, bottom, right + s, bottom + s)))
}

fn isqrt(v: i32) -> i32 {
    if v <= 0 {
        return 0;
    }
    // Bisection, as IBM's Dialsqrt.
    let (mut lo, mut hi) = (0i32, v.min(46340) + 1);
    while hi - lo > 1 {
        let m = (lo + hi) / 2;
        if m * m <= v { lo = m } else { hi = m }
    }
    lo
}

// ---- Drawing ----

fn disc(c: &mut Canvas, m: Point, r: i32, col: u32) {
    for dy in -r..=r {
        let dx = isqrt(r * r - dy * dy);
        c.hline(m.x - dx, m.x + dx + 1, m.y + dy, col);
    }
}

/// A ring between radii `inner` and `outer`, lit from the top left: the
/// upper left half in `tl`, the lower right in `br`.
fn ring(c: &mut Canvas, m: Point, inner: i32, outer: i32, tl: u32, br: u32) {
    for dy in -outer..=outer {
        let xo = isqrt(outer * outer - dy * dy);
        let xi = if dy.abs() < inner { isqrt(inner * inner - dy * dy) } else { -1 };
        for (a, b) in [(-xo, -xi - 1), (xi + 1, xo)] {
            let mut x = a;
            while x <= b {
                // Runs of one colour: split where dy - dx changes sign.
                let lit = dy - x > 0;
                let mut e = x;
                while e < b && (dy - (e + 1) > 0) == lit {
                    e += 1;
                }
                c.hline(m.x + x, m.x + e + 1, m.y + dy, if lit { tl } else { br });
                x = e + 1;
            }
        }
    }
}

fn line(c: &mut Canvas, a: Point, b: Point, col: u32) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let n = dx.abs().max(dy.abs()).max(1);
    for i in 0..=n {
        let x = a.x + dx * i / n;
        let y = a.y + dy * i / n;
        c.fill(Rect::new(x, y, x + 1, y + 1), col);
    }
}

fn triangle(c: &mut Canvas, p: [Point; 3], col: u32) {
    let lo = p.iter().map(|q| q.y).min().unwrap_or(0);
    let hi = p.iter().map(|q| q.y).max().unwrap_or(0);
    for y in lo..=hi {
        let mut xs = [i32::MAX, i32::MIN];
        for i in 0..3 {
            let (a, b) = (p[i], p[(i + 1) % 3]);
            if (a.y <= y && y <= b.y) || (b.y <= y && y <= a.y) {
                let x = if a.y == b.y { a.x } else { a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y) };
                let x2 = if a.y == b.y { b.x } else { x };
                xs[0] = xs[0].min(x.min(x2));
                xs[1] = xs[1].max(x.max(x2));
            }
        }
        if xs[0] <= xs[1] {
            c.hline(xs[0], xs[1] + 1, y, col);
        }
    }
}

fn number(v: i32) -> alloc::vec::Vec<u8> {
    alloc::format!("{v}").into_bytes()
}

fn paint(h: u32) {
    let style = ctl::style(h);
    let enabled = ctl::enabled(h);
    let focus = ctl::has_focus(h);
    let g = geo(h);
    let Some(d) = data(h) else { return };
    let (min, max, value, tick, state, bitmaps) = (d.min, d.max, d.value, d.tick.max(1), d.state, d.bitmaps);
    let f = draw::font_of(h);
    // The owner may choose the background (an RGB colour; 0 or 1: the
    // default).
    let asked = send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CSN_QUERYBACKGROUNDCOLOR), 0);
    let bg = if asked > 1 { asked & 0xff_ffff } else { crate::pp::or(h, crate::pp::PP_BACKGROUNDCOLOR, draw::dialog_bg()) };
    let face = draw::face();
    let ink = if enabled { draw::text() } else { draw::GRAY_TEXT };
    let (cx, cy) = ctl::size(h);
    let ticks: alloc::vec::Vec<(Point, Point)> = if style & CSS_NOTICKS == 0 {
        let (inner, outer) = if style & CSS_PROPORTIONALTICKS != 0 { (g.bs - g.bsh, g.bs) } else { (8, 16) };
        let mut v = min;
        let mut t = alloc::vec::Vec::new();
        while v <= max {
            let long = style & CSS_MIDPOINT != 0 && (v == min || v == max || v == (min + max) / 2);
            t.push((point_at(d, g.c, v, g.r + inner), point_at(d, g.c, v, g.r + outer + if long { 3 } else { 0 })));
            v += tick;
        }
        t
    } else {
        alloc::vec::Vec::new()
    };
    let (ux, uy) = dir(index(d, value));
    let at = |dist: i32, side: i32| Point::new(g.c.x + (dist * ux - side * uy) / 100, g.c.y + (dist * uy + side * ux) / 100);
    let indicator = [at(g.r - 6, 0), at(g.r / 2, -g.r / 6), at(g.r / 2, g.r / 6)];
    let knob = at(g.r * 3 / 4, 0);
    // The value: under the centre, or in it (CSS_CIRCULARVALUE).
    let num = (style & CSS_NONUMBER == 0).then(|| {
        let w = f.text_width(&number(min)).max(f.text_width(&number(max)));
        let t = number(value);
        if style & CSS_CIRCULARVALUE == 0 {
            let b = g.c.y + (g.r - g.line) / 2 - g.r;
            (g.r - 6 >= g.line).then(|| (Rect::new(g.c.x - w / 2, b, g.c.x - w / 2 + w, b + g.line), t))
        } else {
            let b = g.c.y - g.line / 2;
            (g.r * 3 / 4 - g.r / 8 - 4 >= w / 2).then(|| (Rect::new(g.c.x - w / 2, b, g.c.x - w / 2 + w, b + g.line), t))
        }
    }).flatten();
    // The window text, centred below.
    let text = ctl::text(h);
    let label = (style & CSS_NOTEXT == 0 && !text.is_empty()).then(|| {
        let (s, _) = draw::strip_mnemonic(&text);
        let w = f.text_width(&s);
        let foot = g.c.y - g.r;
        let b = if style & CSS_360 == 0 { ((foot - g.line) / 3).max(0) } else { 0 };
        Rect::new(g.c.x - w / 2, b, g.c.x - w / 2 + w, b + g.line)
    });
    let btns = buttons(h, &g);
    let hps = crate::win::WinBeginPaint(h, 0, core::ptr::null_mut());
    draw::with(h, hps, |c: &mut Canvas| {
        c.fill(Rect::new(0, 0, cx, cy), bg);
        // The dial: a lit face with a dark rim.
        disc(c, g.c, g.r + 1, face);
        ring(c, g.c, g.r - 1, g.r + 1, draw::light(), draw::shadow());
        ring(c, g.c, g.r + 1, g.r + 2, ink, ink);
        for (a, b) in &ticks {
            line(c, *a, *b, ink);
        }
        if style & CSS_CIRCULARVALUE == 0 {
            triangle(c, indicator, ink);
        } else {
            let kr = (g.r / 8).max(2);
            disc(c, knob, kr, face);
            ring(c, knob, kr - 1, kr, draw::shadow(), draw::light());
            ring(c, knob, kr, kr + 1, ink, ink);
        }
        if let Some((r, t)) = &num {
            c.fill(*r, face);
            let x = r.left + (r.width() - f.text_width(t)) / 2;
            c.text(x, r.bottom + f.descender, t, ink, *r);
        }
        if let Some(r) = label {
            let clip = c.rect();
            c.mnemonic_text(r.left, r.bottom + f.descender, &text, ink, clip);
        }
        if let Some((m, p)) = btns {
            for (r, pressed, plus) in [(m, state & MINUS != 0, false), (p, state & PLUS != 0, true)] {
                if bitmaps.is_some() {
                    continue;
                }
                c.fill(r, face);
                let w = if r.width() > 10 { 2 } else { 1 };
                for i in 0..w {
                    let rr = Rect::new(r.left + i, r.bottom + i, r.right - i, r.top - i);
                    if pressed { c.sunken(rr) } else { c.raised(rr) }
                }
                let o = if pressed { 1 } else { 0 };
                let (mx, my) = (r.left + r.width() / 2 + o, r.bottom + r.height() / 2 - o);
                let arm = (r.width() / 2 - w - 2).max(1);
                c.hline(mx - arm, mx + arm + 1, my, ink);
                if plus {
                    c.vline(mx, my - arm, my + arm + 1, ink);
                }
            }
        }
        if focus {
            let r = label.or(num.as_ref().map(|n| n.0)).unwrap_or(Rect::new(0, 0, cx, cy));
            c.focus_rect(Rect::new(r.left - 2, r.bottom - 1, r.right + 2, r.top + 1));
        }
    });
    // Bitmaps for the buttons (CSM_SETBITMAPDATA), stretched into them.
    if let (Some(b), Some((m, p))) = (bitmaps, btns) {
        for (r, hbm) in [(m, b[(state & MINUS != 0) as usize]), (p, b[2 + (state & PLUS != 0) as usize])] {
            let rl = crate::gpiapi::RectL { left: r.left, bottom: r.bottom, right: r.right, top: r.top };
            crate::gpiobj::WinDrawBitmap(hps, hbm, core::ptr::null(), &rl as *const _ as *const crate::gpiapi::PointL, 0, 0, 4);
        }
    }
    crate::win::WinEndPaint(hps);
}

// ---- Behaviour ----

/// Notify the owner: WM_CONTROL (id, code) with the value.
fn tell(h: u32, code: u32) {
    let v = data(h).map_or(0, |d| d.value);
    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), code), v as u16 as u32);
}

/// DialDrawValue: the value moves by `delta` (kept in the range); a move
/// is reported as CSN_TRACKING.
fn step(h: u32, delta: i32) {
    let Some(d) = data(h) else { return };
    d.value = (d.value + delta).clamp(d.min, d.max.max(d.min));
    if delta != 0 {
        tell(h, CSN_TRACKING);
    }
    ctl::invalidate(h);
}

/// The value nearest to a point on the dial (DialPointSelect).
fn nearest(h: u32, p: Point) -> i32 {
    let g = geo(h);
    let Some(d) = data(h) else { return 0 };
    let mut best = (i32::MAX, d.min);
    for v in d.min..=d.max {
        let q = point_at(d, g.c, v, g.r);
        let e = (p.x - q.x) * (p.x - q.x) + (p.y - q.y) * (p.y - q.y);
        if e < best.0 {
            best = (e, v);
        }
    }
    best.1
}

/// On the ring of tick marks (DialTickSelect).
fn on_ticks(h: u32, p: Point) -> bool {
    let g = geo(h);
    let Some(d) = data(h) else { return false };
    let dist = isqrt((g.c.x - p.x) * (g.c.x - p.x) + (g.c.y - p.y) * (g.c.y - p.y));
    if dist < g.r || dist > g.r + g.bs {
        return false;
    }
    if d.span == 30 && p.y < g.c.y {
        return false;
    }
    let (a, b) = (point_at(d, g.c, d.min, g.r), point_at(d, g.c, d.max, g.r));
    if d.span == 42 && p.y < g.c.y && a.x < p.x && p.x < b.x {
        return false;
    }
    !buttons(h, &g).is_some_and(|(m, pl)| m.contains(p) || pl.contains(p))
}

fn start_timer(h: u32, ms: u32) {
    WinStartTimer(0, h, TID_DIAL, ms);
    if let Some(d) = data(h) {
        d.state |= TIMER;
        d.count = 0;
    }
}

fn button_down(h: u32, msg: u32, p: Point) {
    let Some(d) = data(h) else { return };
    d.button = if matches!(msg, WM_BUTTON2DOWN | WM_BUTTON2DBLCLK) { 2 } else { 1 };
    d.state &= !(MINUS | PLUS | TRACK | DRAG);
    let g = geo(h);
    let point = ctl::style(h) & CSS_POINTSELECT != 0;
    let btns = buttons(h, &g);
    let in_dial = p.x >= g.c.x - g.r && p.x < g.c.x + g.r && p.y >= g.c.y - g.r && p.y < g.c.y + g.r;
    let ticks = on_ticks(h, p);
    if ticks || (!btns.is_some_and(|(m, pl)| m.contains(p) || pl.contains(p)) && in_dial) {
        if !point {
            st().capture = h;
            let t = nearest(h, p);
            if let Some(d) = data(h) {
                d.state |= TRACK;
                d.target = t;
                d.wait = 0;
            }
            start_timer(h, 50);
        } else {
            if !ticks {
                st().capture = h;
                if let Some(d) = data(h) {
                    d.state |= DRAG;
                }
            }
            let v = nearest(h, p);
            if let Some(d) = data(h) {
                d.value = v;
            }
            tell(h, CSN_TRACKING);
            ctl::invalidate(h);
        }
    } else if let Some((m, pl)) = btns.filter(|(m, pl)| m.contains(p) || pl.contains(p)) {
        st().capture = h;
        let minus = m.contains(p);
        let _ = pl;
        if let Some(d) = data(h) {
            d.state |= if minus { MINUS } else { PLUS };
            d.wait = 3;
        }
        step(h, if minus { -1 } else { 1 });
        start_timer(h, 100);
    }
    crate::win::WinSetFocus(1, h);
}

fn button_up(h: u32, msg: u32) {
    let Some(d) = data(h) else { return };
    // The other button going up leaves a press of this one alone.
    if (msg == WM_BUTTON1UP && d.button == 2) || (msg == WM_BUTTON2UP && d.button == 1) {
        return;
    }
    d.state &= !(TIMER | MINUS | PLUS | TRACK | DRAG);
    WinStopTimer(0, h, TID_DIAL);
    if st().capture == h {
        st().capture = 0;
    }
    ctl::invalidate(h);
    tell(h, CSN_CHANGED);
}

fn timer(h: u32) {
    let Some(d) = data(h) else { return };
    if d.state & TIMER == 0 {
        return;
    }
    let n = d.count;
    d.count += 1;
    if n <= d.wait {
        return;
    }
    if d.state & TRACK != 0 {
        let gap = d.target - d.value;
        if gap != 0 {
            let s = gap.abs().min(d.inc);
            step(h, if gap > 0 { s } else { -s });
        }
    } else if d.state & MINUS != 0 {
        let i = d.inc;
        step(h, -i);
    } else if d.state & PLUS != 0 {
        let i = d.inc;
        step(h, i);
    }
}

pub extern "C" fn csl_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            // The default ranges: 0-30 over 180 degrees, 0-42 over 252,
            // 0-60 round (CSS_360, which implies no buttons and no
            // number).
            let style = ctl::style(h);
            let (max, span) = if style & CSS_360 != 0 {
                if let Some(w) = st().desk.get_mut(h) {
                    w.style |= CSS_NOBUTTON | CSS_NONUMBER;
                }
                (60, 60)
            } else if style & CSS_NOBUTTON != 0 || (style & CSS_NOTICKS != 0 && style & CSS_CIRCULARVALUE != 0) {
                (42, 42)
            } else {
                (30, 30)
            };
            ctl::set_data(h, Dial { min: 0, max, value: max / 2, target: 0, inc: 1, tick: 3, span, state: 0, count: 0, wait: 0, bitmaps: None, button: 0 });
            0
        }
        WM_DESTROY => {
            WinStopTimer(0, h, TID_DIAL);
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_SIZE | WM_SETWINDOWPARAMS | WM_PRESPARAMCHANGED | WM_SYSCOLORCHANGE | WM_ENABLE => {
            ctl::invalidate(h);
            (msg == WM_SETWINDOWPARAMS) as u32
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON2DOWN | WM_BUTTON1DBLCLK | WM_BUTTON2DBLCLK | WM_BUTTON1UP | WM_BUTTON2UP | WM_MOUSEMOVE | WM_TIMER => {
            if !ctl::enabled(h) && msg != WM_MOUSEMOVE && msg != WM_TIMER {
                crate::misc::WinAlarm(1, 0);
                return 0;
            }
            let p = ctl::mouse_pos(mp1);
            match msg {
                WM_BUTTON1UP | WM_BUTTON2UP => button_up(h, msg),
                WM_MOUSEMOVE => {
                    let state = data(h).map_or(0, |d| d.state);
                    if state & TRACK != 0 {
                        let t = nearest(h, p);
                        if let Some(d) = data(h) {
                            d.target = t;
                        }
                    } else if state & DRAG != 0 {
                        let v = nearest(h, p);
                        if data(h).is_some_and(|d| d.value != v) {
                            if let Some(d) = data(h) {
                                d.value = v;
                            }
                            tell(h, CSN_TRACKING);
                            ctl::invalidate(h);
                        }
                    }
                    return WinDefWindowProc(h, msg, mp1, mp2);
                }
                WM_TIMER => {
                    if mp1 & 0xffff != TID_DIAL {
                        return WinDefWindowProc(h, msg, mp1, mp2);
                    }
                    timer(h);
                }
                _ => button_down(h, msg, p),
            }
            0
        }
        WM_CHAR => {
            // A virtual key going down without Shift, Ctrl or Alt (and
            // without a character): VK_LEFT steps down, the others up.
            let fl = mp1 & 0xffff;
            if fl & (KC_CTRL | KC_SHIFT | KC_ALT | KC_KEYUP) == 0 && fl & KC_VIRTUALKEY != 0 && fl & KC_CHAR == 0 {
                let inc = data(h).map_or(1, |d| d.inc);
                step(h, if hi(mp2) == VK_LEFT { -inc } else { inc });
                tell(h, CSN_CHANGED);
                return 1;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_SETFOCUS => {
            ctl::invalidate(h);
            send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), CSN_SETFOCUS), mp2 & 0xffff);
            0
        }
        WM_QUERYDLGCODE => 0,
        WM_MATCHMNEMONIC => draw::mnemonic(&ctl::text(h)).is_some_and(|m| m == (mp1 as u8).to_ascii_lowercase()) as u32,
        CSM_QUERYRANGE => {
            let Some(d) = data(h) else { return 0 };
            unsafe {
                if mp1 != 0 {
                    (mp1 as *mut i16).write_unaligned(d.min as i16);
                }
                if mp2 != 0 {
                    (mp2 as *mut i16).write_unaligned(d.max as i16);
                }
            }
            1
        }
        CSM_SETRANGE => {
            let (lo, hi) = (mp1 as i16 as i32, mp2 as i16 as i32);
            if (lo == 0 && hi == 0) || lo >= hi {
                return 0;
            }
            let Some(d) = data(h) else { return 0 };
            d.min = lo;
            d.max = hi;
            ctl::invalidate(h);
            1
        }
        CSM_QUERYVALUE => {
            let Some(d) = data(h) else { return 0 };
            if mp1 != 0 {
                unsafe { (mp1 as *mut i16).write_unaligned(d.value as i16) };
            }
            1
        }
        CSM_SETVALUE => {
            let v = mp1 as i16 as i32;
            let Some(d) = data(h) else { return 0 };
            if v < d.min || v > d.max {
                return 0;
            }
            d.value = v;
            ctl::invalidate(h);
            tell(h, CSN_CHANGED);
            1
        }
        CSM_QUERYRADIUS => {
            if mp1 != 0 {
                unsafe { (mp1 as *mut u16).write_unaligned(geo(h).r as u16) };
            }
            1
        }
        CSM_SETINCREMENT => {
            let (scroll, tick) = (mp1 as i16 as i32, mp2 as i16 as i32);
            let Some(d) = data(h) else { return 0 };
            let range = d.max - d.min;
            if scroll < 1 || tick < 1 || scroll > range || tick > range {
                return 0;
            }
            d.inc = scroll;
            d.tick = tick;
            ctl::invalidate(h);
            1
        }
        CSM_QUERYINCREMENT => {
            let Some(d) = data(h) else { return 0 };
            unsafe {
                if mp1 != 0 {
                    (mp1 as *mut i16).write_unaligned(d.inc as i16);
                }
                if mp2 != 0 {
                    (mp2 as *mut i16).write_unaligned(d.tick as i16);
                }
            }
            1
        }
        CSM_SETBITMAPDATA => {
            if mp1 == 0 {
                return 0;
            }
            let b = unsafe { (mp1 as *const [u32; 4]).read_unaligned() };
            if let Some(d) = data(h) {
                d.bitmaps = Some(b);
            }
            ctl::invalidate(h);
            1
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
