//! Drawing helpers for the frame controls, menus and dialog controls: a
//! canvas over a window's presentation space with 3D borders and text
//! with mnemonics (`~X` underlines X).

use alloc::vec::Vec;

use gpi::font::Font;
use gpi::{Point, Ps, Rect, Surface};

use crate::color;
use crate::gpiapi;
use crate::state::st;
use crate::win::{WinBeginPaint, WinEndPaint};

// The controls' colours: the system colours (WinSetSysColors, the Scheme
// Palette).
pub fn text() -> u32 {
    color::sys(color::SYSCLR_WINDOWTEXT)
}
pub fn face() -> u32 {
    color::sys(color::SYSCLR_DIALOGBACKGROUND)
}
pub fn light() -> u32 {
    color::sys(color::SYSCLR_BUTTONLIGHT)
}
pub fn shadow() -> u32 {
    color::sys(color::SYSCLR_BUTTONDARK)
}
pub fn window() -> u32 {
    color::sys(color::SYSCLR_WINDOW)
}
pub fn hilite() -> u32 {
    color::sys(color::SYSCLR_HILITEBACKGROUND)
}
pub fn hilite_text() -> u32 {
    color::sys(color::SYSCLR_HILITEFOREGROUND)
}
pub const GRAY_TEXT: u32 = 0x808080;
pub const DARK: u32 = 0x404040;

pub const DT_CENTER: u32 = 0x0100;
pub const DT_RIGHT: u32 = 0x0200;
pub const DT_VCENTER: u32 = 0x0400;
pub const DT_BOTTOM: u32 = 0x0800;
pub const DT_HALFTONE: u32 = 0x1000;
pub const DT_MNEMONIC: u32 = 0x2000;
pub const DT_WORDBREAK: u32 = 0x4000;

pub fn font() -> &'static Font {
    gpi::font::system()
}

/// Dialog background colour.
pub fn dialog_bg() -> u32 {
    color::sys(color::SYSCLR_DIALOGBACKGROUND)
}

pub struct Canvas<'a> {
    pub ps: &'a mut Ps,
    surface: Surface,
    /// Window size.
    pub w: i32,
    pub h: i32,
}

/// Paint a window's update region (WM_PAINT processing).
pub fn paint(h: u32, f: impl FnOnce(&mut Canvas)) {
    let hps = WinBeginPaint(h, 0, core::ptr::null_mut());
    with(h, hps, f);
    WinEndPaint(hps);
}

fn with(h: u32, hps: u32, f: impl FnOnce(&mut Canvas)) {
    let s = st();
    let (w, hh) = s.desk.get(h).map_or((0, 0), |w| (w.cx, w.cy));
    gpiapi::refresh(s, hps);
    let surface = s.surface;
    // Built-in controls paint within one PMWIN call: the pointer is out
    // of the way meanwhile.
    crate::pointer::hide(s);
    if let Some(e) = gpiapi::entry(s, hps) {
        let mut c = Canvas { ps: &mut e.ps, surface, w, h: hh };
        f(&mut c);
    }
    crate::pointer::show(st());
}

/// Text without the mnemonic marks, and the index of the marked byte.
pub fn strip_mnemonic(t: &[u8]) -> (Vec<u8>, Option<usize>) {
    let mut out = Vec::with_capacity(t.len());
    let mut mark = None;
    let mut i = 0;
    while i < t.len() {
        if t[i] == b'~' && i + 1 < t.len() {
            if t[i + 1] == b'~' {
                out.push(b'~');
                i += 2;
                continue;
            }
            mark.get_or_insert(out.len());
            i += 1;
            continue;
        }
        out.push(t[i]);
        i += 1;
    }
    (out, mark)
}

/// The mnemonic character of a text (lower case), if any.
pub fn mnemonic(t: &[u8]) -> Option<u8> {
    let (s, m) = strip_mnemonic(t);
    m.and_then(|i| s.get(i)).map(|c| c.to_ascii_lowercase())
}

pub fn text_width(t: &[u8]) -> i32 {
    font().text_width(t)
}

impl<'a> Canvas<'a> {
    /// A canvas drawing with `ps` on `surface` for a window of `w` x `h`.
    pub fn new(ps: &'a mut Ps, surface: Surface, w: i32, h: i32) -> Canvas<'a> {
        Canvas { ps, surface, w, h }
    }
}

impl Canvas<'_> {
    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    pub fn fill(&mut self, r: Rect, c: u32) {
        self.ps.fill_rect(&mut self.surface, r, c);
    }

    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, c: u32) {
        self.fill(Rect::new(x0, y, x1, y + 1), c);
    }

    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, c: u32) {
        self.fill(Rect::new(x, y0, x + 1, y1), c);
    }

    /// One-pixel outline inside `r`.
    pub fn frame(&mut self, r: Rect, c: u32) {
        self.hline(r.left, r.right, r.bottom, c);
        self.hline(r.left, r.right, r.top - 1, c);
        self.vline(r.left, r.bottom, r.top, c);
        self.vline(r.right - 1, r.bottom, r.top, c);
    }

    /// Two-colour border: `tl` on the top and left edges, `br` on the
    /// bottom and right.
    pub fn bevel(&mut self, r: Rect, tl: u32, br: u32) {
        self.hline(r.left, r.right, r.top - 1, tl);
        self.vline(r.left, r.bottom, r.top, tl);
        self.hline(r.left, r.right, r.bottom, br);
        self.vline(r.right - 1, r.bottom, r.top, br);
    }

    pub fn raised(&mut self, r: Rect) {
        self.bevel(r, light(), shadow());
    }

    pub fn sunken(&mut self, r: Rect) {
        self.bevel(r, shadow(), light());
    }

    /// Text with its baseline at (x, y), clipped to `clip`.
    pub fn text(&mut self, x: i32, y: i32, t: &[u8], c: u32, clip: Rect) {
        let saved = self.ps.clip.clone();
        let o = self.ps.origin;
        let dev = clip.offset(o.x, o.y);
        self.ps.clip = saved.iter().map(|r| r.intersect(&dev)).filter(|r| !r.is_empty()).collect();
        let (oc, om, obm) = (self.ps.color, self.ps.mix, self.ps.back_mix);
        self.ps.color = c;
        self.ps.mix = gpi::Mix::Overpaint;
        self.ps.back_mix = gpi::Mix::LeaveAlone;
        self.ps.char_string_at(&mut self.surface, Point::new(x, y), t);
        (self.ps.color, self.ps.mix, self.ps.back_mix) = (oc, om, obm);
        self.ps.clip = saved;
    }

    /// Text with mnemonic marks at baseline (x, y).
    pub fn mnemonic_text(&mut self, x: i32, y: i32, t: &[u8], c: u32, clip: Rect) {
        let (s, m) = strip_mnemonic(t);
        self.text(x, y, &s, c, clip);
        if let Some(i) = m {
            let f = font();
            let ux = x + i as i32 * f.width;
            let r = Rect::new(ux, y - 2, ux + f.width, y - 1).intersect(&clip);
            self.fill(r, c);
        }
    }

    /// Text laid out in `r` with DT_* alignment flags; DT_WORDBREAK wraps
    /// at spaces. Returns the number of bytes drawn.
    pub fn text_in(&mut self, r: Rect, t: &[u8], c: u32, fl: u32) -> usize {
        let f = font();
        let lines: Vec<&[u8]> = if fl & DT_WORDBREAK != 0 { wrap(t, (r.width() / f.width).max(1) as usize) } else { alloc::vec![t] };
        let total = lines.len() as i32 * f.height;
        let mut top = if fl & DT_VCENTER != 0 {
            r.top - (r.height() - total) / 2
        } else if fl & DT_BOTTOM != 0 {
            r.bottom + total
        } else {
            r.top
        };
        let mut drawn = 0;
        for l in lines {
            let shown = if fl & DT_MNEMONIC != 0 { strip_mnemonic(l).0.len() } else { l.len() };
            let w = shown as i32 * f.width;
            let x = if fl & DT_CENTER != 0 {
                r.left + (r.width() - w) / 2
            } else if fl & DT_RIGHT != 0 {
                r.right - w
            } else {
                r.left
            };
            let y = top - f.height + f.descender;
            if fl & DT_MNEMONIC != 0 {
                self.mnemonic_text(x, y, l, c, r);
            } else {
                self.text(x, y, l, c, r);
            }
            drawn += l.len();
            top -= f.height;
        }
        drawn
    }

    /// An icon image with its bottom-left corner at (x, y) (window
    /// coordinates).
    pub fn icon(&mut self, x: i32, y: i32, img: &gpi::icon::Image, halftone: bool) {
        let o = self.ps.origin;
        gpi::icon::draw(&mut self.surface, &self.ps.clip, x + o.x, y + o.y, img, halftone);
    }

    /// Dotted focus rectangle.
    pub fn focus_rect(&mut self, r: Rect) {
        for x in (r.left..r.right).step_by(2) {
            self.fill(Rect::new(x, r.bottom, x + 1, r.bottom + 1), text());
            self.fill(Rect::new(x, r.top - 1, x + 1, r.top), text());
        }
        for y in (r.bottom..r.top).step_by(2) {
            self.fill(Rect::new(r.left, y, r.left + 1, y + 1), text());
            self.fill(Rect::new(r.right - 1, y, r.right, y + 1), text());
        }
    }
}

/// Split text into lines of at most `cols` characters at spaces.
pub fn wrap(t: &[u8], cols: usize) -> Vec<&[u8]> {
    let mut out = Vec::new();
    for para in t.split(|&c| c == b'\n') {
        let mut rest = para;
        loop {
            if rest.len() <= cols {
                out.push(rest);
                break;
            }
            let cut = rest[..=cols].iter().rposition(|&c| c == b' ').unwrap_or(cols);
            out.push(&rest[..cut]);
            rest = &rest[cut..];
            while rest.first() == Some(&b' ') {
                rest = &rest[1..];
            }
        }
    }
    out
}
