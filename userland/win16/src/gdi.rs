//! GDI: device contexts on PM presentation spaces, objects, text and
//! simple drawing.
//!
//! An HDC is a handle of this table over the HPS of a window (from
//! BeginPaint or GetDC), in RGB mode, with the window's client height to
//! turn Windows' y-down coordinates into PM's. Colours are COLORREFs
//! (0x00BBGGRR); PM's RGB is 0x00RRGGBB.

use alloc::vec::Vec;
use thunk16::{flat, Pascal};

use crate::pm::*;
use crate::{far_str, read_rect, write_rect, Global};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DcKind {
    /// From BeginPaint: EndPaint releases it.
    Paint,
    /// From GetDC: ReleaseDC.
    Window,
}

#[derive(Clone, Copy)]
enum Obj {
    Brush { color: u32, null: bool },
    Pen { style: u16, width: u16, color: u32 },
    Font,
    Palette,
}

struct Dc {
    hdc: u16,
    hps: u32,
    height: i32,
    kind: DcKind,
    text: u32,
    bk: u32,
    bk_mode: u16,
    brush: u16,
    pen: u16,
    font: u16,
    pos: (i32, i32),
}

struct Gdi {
    dcs: Vec<Dc>,
    objs: Vec<Option<Obj>>,
    next_dc: u16,
}

static GDI: Global<Gdi> = Global::new(Gdi { dcs: Vec::new(), objs: Vec::new(), next_dc: 0x0402 });

/// Object handles: 0x0100 + 2 * index; the stock objects come first
/// (GetStockObject(i) is index i).
const OBJ_BASE: u16 = 0x0100;
const OPAQUE: u16 = 2;
const TRANSPARENT: u16 = 1;

fn g() -> &'static mut Gdi {
    let g = GDI.get();
    if g.objs.is_empty() {
        let b = |c, null| Some(Obj::Brush { color: c, null });
        let p = |c, style| Some(Obj::Pen { style, width: 1, color: c });
        g.objs = alloc::vec![
            b(0xffffff, false), // WHITE_BRUSH
            b(0xc0c0c0, false), // LTGRAY_BRUSH
            b(0x808080, false), // GRAY_BRUSH
            b(0x404040, false), // DKGRAY_BRUSH
            b(0x000000, false), // BLACK_BRUSH
            b(0, true),         // NULL_BRUSH
            p(0xffffff, 0),     // WHITE_PEN
            p(0x000000, 0),     // BLACK_PEN
            p(0, 5),            // NULL_PEN
            None,
            Some(Obj::Font), // OEM_FIXED_FONT
            Some(Obj::Font), // ANSI_FIXED_FONT
            Some(Obj::Font), // ANSI_VAR_FONT
            Some(Obj::Font), // SYSTEM_FONT
            Some(Obj::Font), // DEVICE_DEFAULT_FONT
            Some(Obj::Palette),
            Some(Obj::Font), // SYSTEM_FIXED_FONT
        ];
    }
    g
}

fn obj(h: u16) -> Option<Obj> {
    let i = h.checked_sub(OBJ_BASE)? as usize / 2;
    g().objs.get(i).copied().flatten()
}

fn new_obj(o: Obj) -> u16 {
    let g = g();
    let i = match g.objs.iter().skip(17).position(|o| o.is_none()) {
        Some(i) => {
            g.objs[17 + i] = Some(o);
            17 + i
        }
        None => {
            g.objs.push(Some(o));
            g.objs.len() - 1
        }
    };
    OBJ_BASE + 2 * i as u16
}

fn dc(h: u16) -> Option<&'static mut Dc> {
    g().dcs.iter_mut().find(|d| d.hdc == h)
}

/// PM's RGB for a COLORREF.
pub fn rgb(c: u32) -> i32 {
    ((c & 0xff) << 16 | (c & 0xff00) | (c >> 16) & 0xff) as i32
}

pub fn new_dc(hps: u32, _pm: u32, height: i32, kind: DcKind) -> u16 {
    if hps == 0 {
        return 0;
    }
    unsafe { GpiCreateLogColorTable(hps, 0, LCOLF_RGB, 0, 0, core::ptr::null()) };
    let g = g();
    let hdc = g.next_dc;
    g.next_dc = g.next_dc.wrapping_add(2).max(0x0402);
    g.dcs.push(Dc {
        hdc,
        hps,
        height,
        kind,
        text: 0,
        bk: 0xffffff,
        bk_mode: OPAQUE,
        brush: OBJ_BASE,
        pen: OBJ_BASE + 2 * 7,
        font: OBJ_BASE + 2 * 13,
        pos: (0, 0),
    });
    hdc
}

pub fn release_dc(hdc: u16) -> bool {
    let g = g();
    let Some(i) = g.dcs.iter().position(|d| d.hdc == hdc) else { return false };
    let d = g.dcs.remove(i);
    unsafe {
        match d.kind {
            DcKind::Paint => WinEndPaint(d.hps),
            DcKind::Window => WinReleasePS(d.hps),
        }
    };
    true
}

/// A Windows rectangle (y down) as a PM one.
fn pm_rect(d: &Dc, r: [i32; 4]) -> RectL {
    RectL { l: r[0], b: d.height - r[3], r: r[2], t: d.height - r[1] }
}

/// The brush's colour: an HBRUSH, or a system colour index + 1.
fn brush_color(br: u16) -> Option<u32> {
    if (1..=21).contains(&br) {
        return Some(sys_color(br - 1));
    }
    match obj(br) {
        Some(Obj::Brush { null: true, .. }) => None,
        Some(Obj::Brush { color, .. }) => Some(color),
        _ => None,
    }
}

pub fn fill_rect(hdc: u16, r: [i32; 4], br: u16) {
    let Some(d) = dc(hdc) else { return };
    let Some(c) = brush_color(br) else { return };
    let rc = pm_rect(d, r);
    unsafe { WinFillRect(d.hps, &rc, rgb(c)) };
}

/// The font's ascender, descender and cell height.
fn metrics(hps: u32) -> (i32, i32, i32, i32) {
    let mut fm = [0u8; 256];
    unsafe { GpiQueryFontMetrics(hps, 208, fm.as_mut_ptr()) };
    let l = |o: usize| i32::from_le_bytes(fm[o..o + 4].try_into().unwrap());
    // lMaxAscender, lMaxDescender, lAveCharWidth, lMaxCharInc.
    let (asc, desc) = (l(76), l(80));
    (asc.max(1), desc.max(0), l(100).max(1), l(104).max(1))
}

fn text_width(hps: u32, s: &[u8]) -> i32 {
    if s.is_empty() {
        return 0;
    }
    let mut pts = [PointL::default(); 5];
    unsafe { GpiQueryTextBox(hps, s.len() as i32, s.as_ptr(), TXTBOX_COUNT, pts.as_mut_ptr()) };
    pts[TXTBOX_CONCAT].x
}

fn set_text_attrs(d: &Dc) {
    unsafe {
        GpiSetColor(d.hps, rgb(d.text));
        GpiSetBackColor(d.hps, rgb(d.bk));
        GpiSetBackMix(d.hps, if d.bk_mode == OPAQUE { BM_OVERPAINT } else { BM_LEAVEALONE });
    }
}

/// Windows' & prefixes as PM's ~ mnemonics ("&&" is an ampersand).
fn mnemonics(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'&' if s.get(i + 1) == Some(&b'&') => {
                v.push(b'&');
                i += 1;
            }
            b'&' => v.push(b'~'),
            c => v.push(c),
        }
        i += 1;
    }
    v
}

/// DrawText(hdc, text, count, lpRect, format): the height of the text.
pub fn draw_text(hdc: u16, s: u32, n: i32, r: u32, fmt: u16) -> i32 {
    const DT_CENTER: u16 = 0x01;
    const DT_RIGHT: u16 = 0x02;
    const DT_VCENTER: u16 = 0x04;
    const DT_BOTTOM: u16 = 0x08;
    const DT_WORDBREAK: u16 = 0x10;
    const DT_SINGLELINE: u16 = 0x20;
    const DT_CALCRECT: u16 = 0x400;
    const DT_NOPREFIX: u16 = 0x800;
    let Some(d) = dc(hdc) else { return 0 };
    let Some(rect) = read_rect(r) else { return 0 };
    let all = far_str(s).unwrap_or(b"");
    let text = if n < 0 { all } else { unsafe { core::slice::from_raw_parts(flat(s), n as usize) } };
    let text = if fmt & DT_NOPREFIX == 0 { mnemonics(text) } else { text.to_vec() };
    let (asc, desc, _, _) = metrics(d.hps);
    let lh = asc + desc;
    let mut fl = 0u32;
    if fmt & DT_NOPREFIX == 0 {
        fl |= 0x2000; // DT_MNEMONIC
    }
    if fmt & DT_CENTER != 0 {
        fl |= 0x0100;
    } else if fmt & DT_RIGHT != 0 {
        fl |= 0x0200;
    }
    if fmt & DT_WORDBREAK != 0 {
        fl |= 0x4000;
    }
    // The lines: one, or each line of the text (broken at words).
    let mut lines: Vec<&[u8]> = Vec::new();
    if fmt & DT_SINGLELINE != 0 {
        lines.push(&text);
    } else {
        for l in text.split(|&c| c == b'\n') {
            lines.push(l.strip_suffix(b"\r").unwrap_or(l));
        }
    }
    let pr = pm_rect(d, rect);
    let (fg, bg, opaque) = (rgb(d.text), rgb(d.bk), d.bk_mode == OPAQUE);
    let hps = d.hps;
    let mut top = pr.t;
    let mut widest = 0;
    let mut count = 0;
    for line in lines {
        let mut rest: &[u8] = line;
        loop {
            let mut lr = RectL { l: pr.l, b: top - lh, r: pr.r, t: top };
            if fmt & DT_SINGLELINE != 0 {
                lr.b = pr.b;
                if fmt & DT_VCENTER != 0 {
                    fl |= 0x0400;
                } else if fmt & DT_BOTTOM != 0 {
                    fl |= 0x0800;
                }
            }
            // The extent: for DT_CALCRECT and the opaque background.
            let mut ext = lr;
            let used = unsafe { WinDrawText(hps, rest.len() as i32, rest.as_ptr(), &mut ext, fg, bg, fl | 0x0002) };
            widest = widest.max(ext.r - ext.l);
            if fmt & DT_CALCRECT == 0 {
                if opaque {
                    unsafe { WinFillRect(hps, &ext, bg) };
                }
                let mut dr = lr;
                unsafe { WinDrawText(hps, rest.len() as i32, rest.as_ptr(), &mut dr, fg, bg, fl) };
            }
            count += 1;
            top -= lh;
            let used = (used.max(1) as usize).min(rest.len());
            rest = &rest[used..];
            if rest.is_empty() || fmt & DT_WORDBREAK == 0 {
                break;
            }
        }
    }
    let height = count * lh;
    if fmt & DT_CALCRECT != 0 {
        write_rect(r, [rect[0], rect[1], rect[0] + widest, rect[1] + height]);
    }
    height
}

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    match ord {
        // SetBkColor(hdc, color), SetTextColor(hdc, color): the old one.
        1 | 9 => {
            let (h, c) = (a.w(), a.l());
            let Some(d) = dc(h) else { return 0x8000_0000 };
            let f = if ord == 1 { &mut d.bk } else { &mut d.text };
            core::mem::replace(f, c & 0x00ff_ffff)
        }
        // SetBkMode(hdc, mode)
        2 => {
            let (h, m) = (a.w(), a.w());
            let Some(d) = dc(h) else { return 0 };
            if m != OPAQUE && m != TRANSPARENT {
                return 0;
            }
            core::mem::replace(&mut d.bk_mode, m) as u32
        }
        // LineTo(hdc, x, y)
        19 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let Some(Obj::Pen { style, color, width }) = obj(d.pen) else { return 0 };
            let (x0, y0) = d.pos;
            d.pos = (x, y);
            if style == 5 {
                return 1;
            }
            let from = PointL { x: x0, y: d.height - 1 - y0 };
            let to = PointL { x, y: d.height - 1 - y };
            unsafe {
                GpiSetColor(d.hps, rgb(color));
                GpiSetLineWidth(d.hps, (width.max(1) as i32) << 16);
                GpiMove(d.hps, &from);
                GpiLine(d.hps, &to);
            }
            1
        }
        // MoveTo(hdc, x, y): the old position (x in AX, y in DX).
        20 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let (ox, oy) = core::mem::replace(&mut d.pos, (x, y));
            (oy as u32 & 0xffff) << 16 | ox as u32 & 0xffff
        }
        // Rectangle(hdc, left, top, right, bottom): the brush inside, the
        // pen around.
        27 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let rc = pm_rect(d, [l, t, r, b]);
            if let Some(c) = brush_color(d.brush) {
                unsafe { WinFillRect(d.hps, &rc, rgb(c)) };
            }
            if let Some(Obj::Pen { style, color, .. }) = obj(d.pen) {
                if style != 5 {
                    unsafe {
                        GpiSetColor(d.hps, rgb(color));
                        GpiMove(d.hps, &PointL { x: rc.l, y: rc.b });
                        GpiBox(d.hps, DRO_OUTLINE, &PointL { x: rc.r - 1, y: rc.t - 1 }, 0, 0);
                    }
                }
            }
            1
        }
        // TextOut(hdc, x, y, text, count)
        33 => {
            let (h, x, y, s, n) = (a.w(), a.i(), a.i(), a.l(), a.i());
            let Some(d) = dc(h) else { return 0 };
            if n <= 0 {
                return 1;
            }
            let text = unsafe { core::slice::from_raw_parts(flat(s), n as usize) };
            let (asc, _, _, _) = metrics(d.hps);
            set_text_attrs(d);
            let p = PointL { x, y: d.height - y - asc };
            unsafe { GpiCharStringAt(d.hps, &p, text.len() as i32, text.as_ptr()) };
            1
        }
        // SelectObject(hdc, obj): the previous object of its kind.
        45 => {
            let (h, o) = (a.w(), a.w());
            let Some(d) = dc(h) else { return 0 };
            match obj(o) {
                Some(Obj::Brush { .. }) => core::mem::replace(&mut d.brush, o) as u32,
                Some(Obj::Pen { .. }) => core::mem::replace(&mut d.pen, o) as u32,
                Some(Obj::Font) => core::mem::replace(&mut d.font, o) as u32,
                _ => 0,
            }
        }
        // CreatePen(style, width, color)
        61 => {
            let (style, width, c) = (a.w(), a.w(), a.l());
            new_obj(Obj::Pen { style, width, color: c & 0xffffff }) as u32
        }
        // CreateSolidBrush(color)
        66 => {
            let c = a.l();
            new_obj(Obj::Brush { color: c & 0xffffff, null: false }) as u32
        }
        // DeleteObject(obj): not the stock ones.
        69 => {
            let o = a.w();
            let Some(i) = o.checked_sub(OBJ_BASE).map(|i| i as usize / 2) else { return 0 };
            if i < 17 || i >= g().objs.len() {
                return (i < 17) as u32;
            }
            g().objs[i] = None;
            1
        }
        // GetDeviceCaps(hdc, index)
        80 => {
            let (_h, i) = (a.w(), a.w());
            let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
            match i {
                0 => 0x300,           // DRIVERVERSION
                2 => 1,               // TECHNOLOGY: DT_RASDISPLAY
                4 => 240,             // HORZSIZE (mm)
                6 => 180,             // VERTSIZE
                8 => q(SV_CXSCREEN) as u32,
                10 => q(SV_CYSCREEN) as u32,
                12 => 24,             // BITSPIXEL
                14 => 1,              // PLANES
                24 => u32::MAX >> 16, // NUMCOLORS: many
                88 | 90 => 96,        // LOGPIXELSX/Y
                _ => 0,
            }
        }
        // GetStockObject(i)
        87 => {
            let i = a.w();
            if i <= 16 && i != 9 {
                g();
                (OBJ_BASE + 2 * i) as u32
            } else {
                0
            }
        }
        // GetTextExtent(hdc, text, count): width in AX, height in DX.
        91 => {
            let (h, s, n) = (a.w(), a.l(), a.i());
            let Some(d) = dc(h) else { return 0 };
            let text = if n > 0 { unsafe { core::slice::from_raw_parts(flat(s), n as usize) } } else { &[] };
            let (asc, desc, _, _) = metrics(d.hps);
            ((asc + desc) as u32) << 16 | text_width(d.hps, text) as u32 & 0xffff
        }
        // GetTextMetrics(hdc, lpTextMetric)
        93 => {
            let (h, p) = (a.w(), a.p());
            let Some(d) = dc(h) else { return 0 };
            if p.is_null() {
                return 0;
            }
            let (asc, desc, ave, max) = metrics(d.hps);
            let mut b = [0u8; 31];
            for (i, v) in [asc + desc, asc, desc, 0, 0, ave, max, 400].iter().enumerate() {
                b[i * 2..i * 2 + 2].copy_from_slice(&(*v as i16).to_le_bytes());
            }
            // tmItalic.. tmCharSet: first char 0x20, last 0xff, default
            // '?', break ' ', variable pitch swiss family, ANSI charset.
            b[16..25].copy_from_slice(&[0, 0, 0, 0x20, 0xff, b'?', b' ', 0x21, 0]);
            for (i, v) in [0i16, 96, 96].iter().enumerate() {
                b[25 + i * 2..27 + i * 2].copy_from_slice(&v.to_le_bytes());
            }
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 31) };
            1
        }
        _ => 0,
    }
}

/// Windows 3.1's default system colours (COLORREF).
pub fn sys_color(i: u16) -> u32 {
    const COLORS: [u32; 21] = [
        0xc0c0c0, // SCROLLBAR
        0xc0c0c0, // BACKGROUND
        0x800000, // ACTIVECAPTION
        0xffffff, // INACTIVECAPTION
        0xffffff, // MENU
        0xffffff, // WINDOW
        0x000000, // WINDOWFRAME
        0x000000, // MENUTEXT
        0x000000, // WINDOWTEXT
        0xffffff, // CAPTIONTEXT
        0xc0c0c0, // ACTIVEBORDER
        0xc0c0c0, // INACTIVEBORDER
        0xffffff, // APPWORKSPACE
        0x800000, // HIGHLIGHT
        0xffffff, // HIGHLIGHTTEXT
        0xc0c0c0, // BTNFACE
        0x808080, // BTNSHADOW
        0x808080, // GRAYTEXT
        0x000000, // BTNTEXT
        0x000000, // INACTIVECAPTIONTEXT
        0xffffff, // BTNHIGHLIGHT
    ];
    COLORS.get(i as usize).copied().unwrap_or(0)
}
