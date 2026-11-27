//! GDI: a facade over W32USER's GDI32 (docs/WIN16.md, "One window
//! manager"). DCs, objects, text, drawing, bitmaps, DIBs, regions,
//! palettes and printing are W32USER's; the entries here take Win16's
//! arguments (16-bit coordinates and handles, 16:16 pointers, Win16's
//! LOGFONT, TEXTMETRIC, LOGPEN, LOGBRUSH and BITMAP) and give Win16's
//! results (positions and sizes packed in a DWORD where Win32 has the Ex
//! calls).

use alloc::vec::Vec;
use thunk16::{flat, Pascal, PascalCall};

use crate::msg16::sx;
use crate::user::str32;
use crate::w32::*;
use crate::{read_rect, write_rect};

fn rd16(p: *const u8, o: usize) -> u16 {
    unsafe { (p.add(o) as *const u16).read_unaligned() }
}
fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}
fn wr16(p: *mut u8, o: usize, v: u16) {
    unsafe { (p.add(o) as *mut u16).write_unaligned(v) }
}
fn wr32(p: *mut u8, o: usize, v: u32) {
    unsafe { (p.add(o) as *mut u32).write_unaligned(v) }
}

/// A POINT or SIZE W32USER fills, as a DWORD (x low, y high).
fn packed(f: impl FnOnce(u32) -> u32) -> u32 {
    let mut p = [0i32; 2];
    f(p.as_mut_ptr() as u32);
    (p[0] as u32 & 0xffff) | (p[1] as u32) << 16
}

/// A POINT or SIZE W32USER fills, written as a Win16 one (16 bits each).
fn out_point(far_: u32, f: impl FnOnce(u32) -> u32) -> u32 {
    let mut p = [0i32; 2];
    let r = f(p.as_mut_ptr() as u32);
    let q = flat(far_);
    if !q.is_null() {
        wr16(q, 0, p[0] as u16);
        wr16(q, 2, p[1] as u16);
    }
    r
}

/// Win16 POINTs as 32-bit ones.
fn points(far_: u32, n: usize) -> Vec<i32> {
    let p = flat(far_);
    if p.is_null() {
        return Vec::new();
    }
    (0..2 * n).map(|i| rd16(p, 2 * i) as i16 as i32).collect()
}

/// Win16 ints as 32-bit ones.
fn ints(far_: u32, n: usize) -> Vec<i32> {
    let p = flat(far_);
    if p.is_null() {
        return Vec::new();
    }
    (0..n).map(|i| rd16(p, 2 * i) as i16 as i32).collect()
}

fn ptr(v: &[i32]) -> u32 {
    if v.is_empty() { 0 } else { v.as_ptr() as u32 }
}

/// LOGFONT16 (50 bytes) as LOGFONTA (60).
pub fn logfont32(p: *const u8) -> [u8; 60] {
    let mut b = [0u8; 60];
    for i in 0..5 {
        b[4 * i..4 * i + 4].copy_from_slice(&sx(rd16(p, 2 * i)).to_le_bytes());
    }
    unsafe { core::ptr::copy_nonoverlapping(p.add(10), b.as_mut_ptr().add(20), 8) };
    unsafe { core::ptr::copy_nonoverlapping(p.add(18), b.as_mut_ptr().add(28), 32) };
    b[59] = 0;
    b
}

/// LOGFONTA as LOGFONT16.
pub fn logfont16(lf: *const u8, q: *mut u8) {
    for i in 0..5 {
        wr16(q, 2 * i, rd32(lf, 4 * i) as u16);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(lf.add(20), q.add(10), 8);
        core::ptr::copy_nonoverlapping(lf.add(28), q.add(18), 32);
    }
}

/// TEXTMETRICA (56 bytes) as TEXTMETRIC16 (31).
pub fn textmetric16(tm: *const u8, q: *mut u8) {
    // tmHeight .. tmWeight: eight ints.
    for i in 0..8 {
        wr16(q, 2 * i, rd32(tm, 4 * i) as u16);
    }
    unsafe {
        // tmItalic, tmUnderlined, tmStruckOut
        core::ptr::copy_nonoverlapping(tm.add(48), q.add(16), 3);
        // tmFirstChar, tmLastChar, tmDefaultChar, tmBreakChar
        core::ptr::copy_nonoverlapping(tm.add(44), q.add(19), 4);
        // tmPitchAndFamily, tmCharSet
        *q.add(23) = *tm.add(51);
        *q.add(24) = *tm.add(52);
    }
    // tmOverhang, tmDigitizedAspectX, tmDigitizedAspectY
    for i in 0..3 {
        wr16(q, 25 + 2 * i, rd32(tm, 32 + 4 * i) as u16);
    }
}

/// GetObject(h, n, p) in Win16's layouts.
fn get_object(h: u16, n: i32, p: *mut u8) -> u32 {
    let ty = unsafe { GetObjectType(h as u32) };
    let mut b = [0u8; 128];
    let got = unsafe { GetObjectA(h as u32, b.len() as u32, b.as_mut_ptr() as u32) };
    if got == 0 {
        return 0;
    }
    let bp = b.as_ptr();
    let mut o = [0u8; 64];
    let size = match ty {
        // OBJ_PEN: LOGPEN16 (style, width POINT, colour).
        1 => {
            wr16(o.as_mut_ptr(), 0, rd32(bp, 0) as u16);
            wr16(o.as_mut_ptr(), 2, rd32(bp, 4) as u16);
            wr16(o.as_mut_ptr(), 4, rd32(bp, 8) as u16);
            wr32(o.as_mut_ptr(), 6, rd32(bp, 12));
            10
        }
        // OBJ_BRUSH: LOGBRUSH16 (style, colour, hatch).
        2 => {
            wr16(o.as_mut_ptr(), 0, rd32(bp, 0) as u16);
            wr32(o.as_mut_ptr(), 2, rd32(bp, 4));
            wr16(o.as_mut_ptr(), 6, rd32(bp, 8) as u16);
            8
        }
        // OBJ_FONT: LOGFONT16.
        6 => {
            logfont16(bp, o.as_mut_ptr());
            50
        }
        // OBJ_BITMAP: BITMAP16 (type, width, height, widthbytes, planes,
        // bitspixel, bits).
        7 => {
            for i in 0..4 {
                wr16(o.as_mut_ptr(), 2 * i, rd32(bp, 4 * i) as u16);
            }
            o[8] = rd16(bp, 16) as u8;
            o[9] = rd16(bp, 18) as u8;
            14
        }
        // OBJ_PAL: the number of entries.
        5 => {
            wr16(o.as_mut_ptr(), 0, rd16(bp, 0));
            2
        }
        _ => 0,
    };
    if p.is_null() {
        return size;
    }
    let k = size.min(n.max(0) as u32);
    unsafe { core::ptr::copy_nonoverlapping(o.as_ptr(), p, k as usize) };
    k
}

/// Escape(hdc, escape, cbIn, lpIn, lpOut) with Win16's structures.
fn escape(hdc: u16, esc: i32, n: i32, pin: u32, pout: u32) -> u32 {
    let (i, o) = (str32(pin), str32(pout));
    let r = match esc {
        // NEXTBAND: a RECT out.
        3 => {
            let mut r = [0i32; 4];
            let v = unsafe { Escape(hdc as u32, 3, n as u32, i, r.as_mut_ptr() as u32) };
            write_rect(pout, r);
            v
        }
        // SETABORTPROC(proc)
        9 => unsafe { Escape(hdc as u32, 9, n as u32, crate::msg16::proc16to32(pin), o) },
        // GETPHYSPAGESIZE, GETPRINTINGOFFSET, GETSCALINGFACTOR: a POINT out.
        12..=14 => out_point(pout, |q| unsafe { Escape(hdc as u32, esc as u32, n as u32, i, q) }),
        _ => unsafe { Escape(hdc as u32, esc as u32, n as u32, i, o) },
    };
    r & 0xffff
}

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    let d = |h: u16| h as u32;
    let i32_ = |v: i32| v as u32;
    match ord {
        1 => {
            let (h, c) = (a.w(), a.l());
            unsafe { SetBkColor(d(h), c) }
        }
        2 => {
            let (h, m) = (a.w(), a.w());
            unsafe { SetBkMode(d(h), m as u32) }
        }
        3 => {
            let (h, m) = (a.w(), a.w());
            unsafe { SetMapMode(d(h), m as u32) }
        }
        4 => {
            let (h, m) = (a.w(), a.w());
            unsafe { SetROP2(d(h), m as u32) }
        }
        6 => {
            let (h, m) = (a.w(), a.w());
            unsafe { SetPolyFillMode(d(h), m as u32) }
        }
        7 => {
            let (h, m) = (a.w(), a.w());
            unsafe { SetStretchBltMode(d(h), m as u32) }
        }
        8 => {
            let (h, n) = (a.w(), a.i());
            unsafe { SetTextCharacterExtra(d(h), i32_(n)) & 0xffff }
        }
        9 => {
            let (h, c) = (a.w(), a.l());
            unsafe { SetTextColor(d(h), c) }
        }
        10 => {
            let (h, e, b) = (a.w(), a.i(), a.i());
            unsafe { SetTextJustification(d(h), i32_(e), i32_(b)) }
        }
        // SetWindowOrg, SetWindowExt, SetViewportOrg, SetViewportExt,
        // OffsetWindowOrg, OffsetViewportOrg(hdc, x, y): the old values.
        11 | 12 | 13 | 14 | 15 | 17 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            packed(|q| unsafe {
                match ord {
                    11 => SetWindowOrgEx(d(h), i32_(x), i32_(y), q),
                    12 => SetWindowExtEx(d(h), i32_(x), i32_(y), q),
                    13 => SetViewportOrgEx(d(h), i32_(x), i32_(y), q),
                    14 => SetViewportExtEx(d(h), i32_(x), i32_(y), q),
                    15 => OffsetWindowOrgEx(d(h), i32_(x), i32_(y), q),
                    _ => OffsetViewportOrgEx(d(h), i32_(x), i32_(y), q),
                }
            })
        }
        // ScaleWindowExt, ScaleViewportExt(hdc, xn, xd, yn, yd)
        16 | 18 => {
            let (h, xn, xd, yn, yd) = (a.w(), a.i(), a.i(), a.i(), a.i());
            packed(|q| unsafe {
                if ord == 16 { ScaleWindowExtEx(d(h), i32_(xn), i32_(xd), i32_(yn), i32_(yd), q) } else { ScaleViewportExtEx(d(h), i32_(xn), i32_(xd), i32_(yn), i32_(yd), q) }
            })
        }
        19 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            unsafe { LineTo(d(h), i32_(x), i32_(y)) }
        }
        // MoveTo(hdc, x, y): the old position.
        20 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            packed(|q| unsafe { MoveToEx(d(h), i32_(x), i32_(y), q) })
        }
        21 | 22 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            unsafe { if ord == 21 { ExcludeClipRect(d(h), i32_(l), i32_(t), i32_(r), i32_(b)) } else { IntersectClipRect(d(h), i32_(l), i32_(t), i32_(r), i32_(b)) } }
        }
        // Arc, Pie, Chord(hdc, l, t, r, b, x1, y1, x2, y2)
        23 | 26 | 348 => {
            let (h, l, t, r, b, x1, y1, x2, y2) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i());
            let v = [l, t, r, b, x1, y1, x2, y2].map(i32_);
            unsafe {
                match ord {
                    23 => Arc(d(h), v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]),
                    26 => Pie(d(h), v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]),
                    _ => Chord(d(h), v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]),
                }
            }
        }
        24 | 27 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            unsafe { if ord == 24 { Ellipse(d(h), i32_(l), i32_(t), i32_(r), i32_(b)) } else { Rectangle(d(h), i32_(l), i32_(t), i32_(r), i32_(b)) } }
        }
        25 => {
            let (h, x, y, c) = (a.w(), a.i(), a.i(), a.l());
            unsafe { FloodFill(d(h), i32_(x), i32_(y), c) }
        }
        28 => {
            let (h, l, t, r, b, w, hh) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i());
            unsafe { RoundRect(d(h), i32_(l), i32_(t), i32_(r), i32_(b), i32_(w), i32_(hh)) }
        }
        29 => {
            let (h, x, y, w, hh, rop) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.l());
            unsafe { PatBlt(d(h), i32_(x), i32_(y), i32_(w), i32_(hh), rop) }
        }
        30 => unsafe { SaveDC(d(a.w())) & 0xffff },
        31 => {
            let (h, x, y, c) = (a.w(), a.i(), a.i(), a.l());
            unsafe { SetPixel(d(h), i32_(x), i32_(y), c) }
        }
        32 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            unsafe { OffsetClipRgn(d(h), i32_(x), i32_(y)) }
        }
        // TextOut(hdc, x, y, text, n)
        33 => {
            let (h, x, y, s, n) = (a.w(), a.i(), a.i(), a.l(), a.i());
            unsafe { TextOutA(d(h), i32_(x), i32_(y), str32(s), i32_(n)) }
        }
        // BitBlt(hdc, x, y, w, h, src, sx, sy, rop)
        34 => {
            let (h, x, y, w, hh, s, sx_, sy_, rop) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.w(), a.i(), a.i(), a.l());
            unsafe { BitBlt(d(h), i32_(x), i32_(y), i32_(w), i32_(hh), s as u32, i32_(sx_), i32_(sy_), rop) }
        }
        // StretchBlt(hdc, x, y, w, h, src, sx, sy, sw, sh, rop)
        35 => {
            let (h, x, y, w, hh, s, sx_, sy_, sw, sh, rop) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.w(), a.i(), a.i(), a.i(), a.i(), a.l());
            unsafe { StretchBlt(d(h), i32_(x), i32_(y), i32_(w), i32_(hh), s as u32, i32_(sx_), i32_(sy_), i32_(sw), i32_(sh), rop) }
        }
        // Polygon, Polyline(hdc, points, n)
        36 | 37 => {
            let (h, p, n) = (a.w(), a.l(), a.i());
            let v = points(p, n.max(0) as usize);
            unsafe { if ord == 36 { Polygon(d(h), ptr(&v), i32_(n)) } else { Polyline(d(h), ptr(&v), i32_(n)) } }
        }
        38 => {
            let (h, e, n, i, o) = (a.w(), a.i(), a.i(), a.l(), a.l());
            escape(h, e, n, i, o)
        }
        39 => {
            let (h, n) = (a.w(), a.i());
            unsafe { RestoreDC(d(h), i32_(n)) }
        }
        40 => {
            let (h, r, b) = (a.w(), a.w(), a.w());
            unsafe { FillRgn(d(h), r as u32, b as u32) }
        }
        41 => {
            let (h, r, b, w, hh) = (a.w(), a.w(), a.w(), a.i(), a.i());
            unsafe { FrameRgn(d(h), r as u32, b as u32, i32_(w), i32_(hh)) }
        }
        42 | 43 => {
            let (h, r) = (a.w(), a.w());
            unsafe { if ord == 42 { InvertRgn(d(h), r as u32) } else { PaintRgn(d(h), r as u32) } }
        }
        44 => {
            let (h, r) = (a.w(), a.w());
            unsafe { SelectClipRgn(d(h), r as u32) }
        }
        45 => {
            let (h, o) = (a.w(), a.w());
            unsafe { SelectObject(d(h), o as u32) & 0xffff }
        }
        47 => {
            let (x, y, z, m) = (a.w(), a.w(), a.w(), a.w());
            unsafe { CombineRgn(x as u32, y as u32, z as u32, m as u32) }
        }
        // CreateBitmap(w, h, planes, bpp, bits)
        48 => {
            let (w, h, pl, bpp, bits) = (a.i(), a.i(), a.w(), a.w(), a.l());
            unsafe { CreateBitmap(i32_(w), i32_(h), pl as u32, bpp as u32, str32(bits)) & 0xffff }
        }
        // CreateBitmapIndirect(BITMAP16)
        49 => {
            let p = a.p();
            if p.is_null() {
                return 0;
            }
            let bm = [sx(rd16(p, 0)), sx(rd16(p, 2)), sx(rd16(p, 4)), sx(rd16(p, 6)), unsafe { *p.add(8) } as u32 | (unsafe { *p.add(9) } as u32) << 16, str32(rd32(p, 10))];
            unsafe { CreateBitmapIndirect(bm.as_ptr() as u32) & 0xffff }
        }
        // CreateBrushIndirect(LOGBRUSH16)
        50 => {
            let p = a.p();
            if p.is_null() {
                return 0;
            }
            let lb = [rd16(p, 0) as u32, rd32(p, 2), sx(rd16(p, 6))];
            unsafe { CreateBrushIndirect(lb.as_ptr() as u32) & 0xffff }
        }
        51 | 156 => {
            let (h, w, hh) = (a.w(), a.i(), a.i());
            unsafe { CreateCompatibleBitmap(d(h), i32_(w), i32_(hh)) & 0xffff }
        }
        52 => unsafe { CreateCompatibleDC(d(a.w())) & 0xffff },
        // CreateDC(driver, device, output, init), CreateIC
        53 | 153 => {
            let (drv, dev, out, init) = (a.l(), a.l(), a.l(), a.l());
            unsafe { (if ord == 53 { CreateDCA(str32(drv), str32(dev), str32(out), str32(init)) } else { CreateICA(str32(drv), str32(dev), str32(out), str32(init)) }) & 0xffff }
        }
        54 => {
            let (l, t, r, b) = (a.i(), a.i(), a.i(), a.i());
            unsafe { CreateEllipticRgn(i32_(l), i32_(t), i32_(r), i32_(b)) & 0xffff }
        }
        55 | 65 => {
            let Some(r) = read_rect(a.l()) else { return 0 };
            unsafe { (if ord == 55 { CreateEllipticRgnIndirect(r.as_ptr() as u32) } else { CreateRectRgnIndirect(r.as_ptr() as u32) }) & 0xffff }
        }
        // CreateFont(h, w, escapement, orientation, weight, italic,
        // underline, strikeout, charset, outprec, clipprec, quality, pitch,
        // face)
        56 => {
            let v: Vec<i32> = (0..5).map(|_| a.i()).collect();
            let b: Vec<u8> = (0..8).map(|_| a.w() as u8).collect();
            let face = a.l();
            let mut lf = [0u8; 60];
            for (i, x) in v.iter().enumerate() {
                lf[4 * i..4 * i + 4].copy_from_slice(&x.to_le_bytes());
            }
            lf[20..28].copy_from_slice(&b);
            let f = crate::far_str(face).unwrap_or(b"");
            let n = f.len().min(31);
            lf[28..28 + n].copy_from_slice(&f[..n]);
            unsafe { CreateFontIndirectA(lf.as_ptr() as u32) & 0xffff }
        }
        57 => {
            let p = a.p();
            if p.is_null() {
                return 0;
            }
            let lf = logfont32(p);
            unsafe { CreateFontIndirectA(lf.as_ptr() as u32) & 0xffff }
        }
        58 => {
            let (s, c) = (a.w(), a.l());
            unsafe { CreateHatchBrush(s as u32, c) & 0xffff }
        }
        60 => unsafe { CreatePatternBrush(a.w() as u32) & 0xffff },
        61 => {
            let (s, w, c) = (a.w(), a.i(), a.l());
            unsafe { CreatePen(s as u32, i32_(w), c) & 0xffff }
        }
        // CreatePenIndirect(LOGPEN16)
        62 => {
            let p = a.p();
            if p.is_null() {
                return 0;
            }
            let lp = [rd16(p, 0) as u32, sx(rd16(p, 2)), sx(rd16(p, 4)), rd32(p, 6)];
            unsafe { CreatePenIndirect(lp.as_ptr() as u32) & 0xffff }
        }
        // CreatePolygonRgn(points, n, mode)
        63 => {
            let (p, n, m) = (a.l(), a.i(), a.w());
            let v = points(p, n.max(0) as usize);
            unsafe { CreatePolygonRgn(ptr(&v), i32_(n), m as u32) & 0xffff }
        }
        64 => {
            let (l, t, r, b) = (a.i(), a.i(), a.i(), a.i());
            unsafe { CreateRectRgn(i32_(l), i32_(t), i32_(r), i32_(b)) & 0xffff }
        }
        66 => unsafe { CreateSolidBrush(a.l()) & 0xffff },
        // DPtoLP, LPtoDP(hdc, points, n)
        67 | 99 => {
            let (h, p, n) = (a.w(), a.l(), a.i());
            let mut v = points(p, n.max(0) as usize);
            let r = unsafe { if ord == 67 { DPtoLP(d(h), ptr(&v), i32_(n)) } else { LPtoDP(d(h), ptr(&v), i32_(n)) } };
            let q = flat(p);
            for (i, x) in v.iter_mut().enumerate() {
                wr16(q, 2 * i, *x as u16);
            }
            r
        }
        68 => unsafe { DeleteDC(d(a.w())) },
        69 => unsafe { DeleteObject(a.w() as u32) },
        // EnumFonts(hdc, face, proc, data), EnumFontFamilies
        70 | 330 => {
            let (h, face, p, data) = (a.w(), a.l(), a.l(), a.l());
            let f = crate::msg16::proc16to32(p);
            unsafe { if ord == 70 { EnumFontsA(d(h), str32(face), f, data) } else { EnumFontFamiliesA(d(h), str32(face), f, data) } }
        }
        72 => {
            let (x, y) = (a.w(), a.w());
            unsafe { EqualRgn(x as u32, y as u32) }
        }
        // GetBitmapBits(hbm, count, bits), SetBitmapBits
        74 | 106 => {
            let (h, n, bits) = (a.w(), a.l(), a.l());
            unsafe { if ord == 74 { GetBitmapBits(h as u32, n, str32(bits)) } else { SetBitmapBits(h as u32, n, str32(bits)) } }
        }
        75 => unsafe { GetBkColor(d(a.w())) },
        76 => unsafe { GetBkMode(d(a.w())) },
        77 => {
            let (h, r) = (a.w(), a.l());
            let mut rr = [0i32; 4];
            let v = unsafe { GetClipBox(d(h), rr.as_mut_ptr() as u32) };
            write_rect(r, rr);
            v
        }
        // GetCurrentPosition, GetDCOrg, GetViewportExt, GetViewportOrg,
        // GetWindowExt, GetWindowOrg(hdc): a DWORD.
        78 | 79 | 94 | 95 | 96 | 97 => {
            let h = a.w();
            packed(|q| unsafe {
                match ord {
                    78 => GetCurrentPositionEx(d(h), q),
                    79 => GetDCOrgEx(d(h), q),
                    94 => GetViewportExtEx(d(h), q),
                    95 => GetViewportOrgEx(d(h), q),
                    96 => GetWindowExtEx(d(h), q),
                    _ => GetWindowOrgEx(d(h), q),
                }
            })
        }
        80 => {
            let (h, i) = (a.w(), a.i());
            sx(unsafe { GetDeviceCaps(d(h), i32_(i)) } as u16)
        }
        81 => unsafe { GetMapMode(d(a.w())) },
        82 => {
            let (h, n, p) = (a.w(), a.i(), a.p());
            get_object(h, n, p)
        }
        83 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            unsafe { GetPixel(d(h), i32_(x), i32_(y)) }
        }
        84 => unsafe { GetPolyFillMode(d(a.w())) },
        85 => unsafe { GetROP2(d(a.w())) },
        87 => unsafe { GetStockObject(a.w() as u32) & 0xffff },
        88 => unsafe { GetStretchBltMode(d(a.w())) },
        89 => unsafe { GetTextCharacterExtra(d(a.w())) & 0xffff },
        90 => unsafe { GetTextColor(d(a.w())) },
        // GetTextExtent(hdc, text, n): a DWORD.
        91 => {
            let (h, s, n) = (a.w(), a.l(), a.i());
            packed(|q| unsafe { GetTextExtentPoint32A(d(h), str32(s), i32_(n), q) })
        }
        92 => {
            let (h, n, b) = (a.w(), a.i(), a.l());
            unsafe { GetTextFaceA(d(h), i32_(n), str32(b)) }
        }
        // GetTextMetrics(hdc, lpTM)
        93 => {
            let (h, p) = (a.w(), a.p());
            let mut tm = [0u8; 56];
            let r = unsafe { GetTextMetricsA(d(h), tm.as_mut_ptr() as u32) };
            if !p.is_null() {
                textmetric16(tm.as_ptr(), p);
            }
            r
        }
        // LineDDA(x1, y1, x2, y2, proc, data): the procedure gets each
        // point of the line but the last.
        100 => {
            let (x1, y1, x2, y2, p, data) = (a.i(), a.i(), a.i(), a.i(), a.l(), a.l());
            let (dx, dy) = ((x2 - x1).abs(), -(y2 - y1).abs());
            let (sx_, sy_) = (if x1 < x2 { 1 } else { -1 }, if y1 < y2 { 1 } else { -1 });
            let (mut x, mut y, mut err) = (x1, y1, dx + dy);
            while (x, y) != (x2, y2) {
                PascalCall::new().w(x as u16).w(y as u16).l(data).call(p);
                let e2 = 2 * err;
                if e2 >= dy {
                    err += dy;
                    x += sx_;
                }
                if e2 <= dx {
                    err += dx;
                    y += sy_;
                }
            }
            0
        }
        101 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            unsafe { OffsetRgn(h as u32, i32_(x), i32_(y)) }
        }
        103 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            unsafe { PtVisible(d(h), i32_(x), i32_(y)) }
        }
        104 => {
            let (h, r) = (a.w(), a.l());
            let Some(r) = read_rect(r) else { return 0 };
            unsafe { RectVisible(d(h), r.as_ptr() as u32) }
        }
        // MulDiv(a, b, c)
        128 => {
            let (x, y, z) = (a.i(), a.i(), a.i());
            if z == 0 {
                return 0x8001;
            }
            let r = (x as i64 * y as i64 + (z as i64 / 2) * if (x as i64 * y as i64) < 0 { -1 } else { 1 }) / z as i64;
            r.clamp(-32768, 32767) as i16 as u16 as u32
        }
        134 => {
            let (h, r) = (a.w(), a.l());
            let mut rr = [0i32; 4];
            let v = unsafe { GetRgnBox(h as u32, rr.as_mut_ptr() as u32) };
            write_rect(r, rr);
            v
        }
        // SetBrushOrg(hdc, x, y): the old origin.
        148 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            packed(|q| unsafe { SetBrushOrgEx(d(h), i32_(x), i32_(y), q) })
        }
        150 => unsafe { UnrealizeObject(a.w() as u32) },
        154 => {
            let (h, c) = (a.w(), a.l());
            unsafe { GetNearestColor(d(h), c) }
        }
        // QueryAbort(hdc, reserved)
        155 => {
            let (h, _) = (a.w(), a.w());
            unsafe { Rs2W32QueryAbort(d(h)) }
        }
        161 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            unsafe { PtInRegion(h as u32, i32_(x), i32_(y)) }
        }
        // GetBitmapDimension(hbm), SetBitmapDimension(hbm, x, y): DWORDs.
        162 => {
            let h = a.w();
            packed(|q| unsafe { GetBitmapDimensionEx(h as u32, q) })
        }
        163 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            packed(|q| unsafe { SetBitmapDimensionEx(h as u32, i32_(x), i32_(y), q) })
        }
        172 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            unsafe { SetRectRgn(h as u32, i32_(l), i32_(t), i32_(r), i32_(b)) }
        }
        181 => {
            let (h, r) = (a.w(), a.l());
            let Some(r) = read_rect(r) else { return 0 };
            unsafe { RectInRegion(h as u32, r.as_ptr() as u32) }
        }
        // GetCharABCWidths(hdc, first, last, lpabc): ABC16 (A, B, C: 6
        // bytes each).
        307 => {
            let (h, f, l, p) = (a.w(), a.w(), a.w(), a.p());
            let n = (l.saturating_sub(f) as usize) + 1;
            let mut v = alloc::vec![0i32; 3 * n];
            let r = unsafe { GetCharABCWidthsA(d(h), f as u32, l as u32, v.as_mut_ptr() as u32) };
            if r != 0 && !p.is_null() {
                for (i, x) in v.iter().enumerate() {
                    wr16(p, 2 * i, *x as u16);
                }
            }
            r
        }
        // GetRasterizerCaps(lpcaps, size): TrueType available and enabled.
        313 => {
            let (p, n) = (a.p(), a.i());
            if !p.is_null() && n >= 6 {
                wr16(p, 0, 6);
                wr16(p, 2, 3);
                wr16(p, 4, 0);
            }
            1
        }
        345 => unsafe { GetTextAlign(d(a.w())) },
        346 => {
            let (h, al) = (a.w(), a.w());
            unsafe { SetTextAlign(d(h), al as u32) }
        }
        // GetCharWidth(hdc, first, last, buf)
        350 => {
            let (h, f, l, p) = (a.w(), a.w(), a.w(), a.p());
            let n = (l.saturating_sub(f) as usize) + 1;
            let mut v = alloc::vec![0i32; n];
            let r = unsafe { GetCharWidthA(d(h), f as u32, l as u32, v.as_mut_ptr() as u32) };
            if r != 0 && !p.is_null() {
                for (i, x) in v.iter().enumerate() {
                    wr16(p, 2 * i, *x as u16);
                }
            }
            r
        }
        // ExtTextOut(hdc, x, y, flags, lpRect, text, n, lpDx)
        351 => {
            let (h, x, y, fl, r, s, n, dx) = (a.w(), a.i(), a.i(), a.w(), a.l(), a.l(), a.w(), a.l());
            let r = read_rect(r);
            let rp = r.as_ref().map_or(0, |v| v.as_ptr() as u32);
            let dxs = ints(dx, n as usize);
            unsafe { ExtTextOutA(d(h), i32_(x), i32_(y), fl as u32, rp, str32(s), n as u32, ptr(&dxs)) }
        }
        // CreatePalette(LOGPALETTE): the same layout.
        360 => unsafe { CreatePalette(str32(a.l())) & 0xffff },
        363 | 364 | 367 => {
            let (pal, s, n, p) = (a.w(), a.w(), a.w(), a.l());
            unsafe {
                match ord {
                    363 => GetPaletteEntries(pal as u32, s as u32, n as u32, str32(p)),
                    364 => SetPaletteEntries(pal as u32, s as u32, n as u32, str32(p)),
                    _ => AnimatePalette(pal as u32, s as u32, n as u32, str32(p)),
                }
            }
        }
        366 => unsafe { UpdateColors(d(a.w())) },
        368 => {
            let (pal, n) = (a.w(), a.w());
            unsafe { ResizePalette(pal as u32, n as u32) }
        }
        370 => {
            let (pal, c) = (a.w(), a.l());
            unsafe { GetNearestPaletteIndex(pal as u32, c) }
        }
        372 => {
            let (h, x, y, c, t) = (a.w(), a.i(), a.i(), a.l(), a.w());
            unsafe { ExtFloodFill(d(h), i32_(x), i32_(y), c, t as u32) }
        }
        373 => {
            let (h, u) = (a.w(), a.w());
            unsafe { SetSystemPaletteUse(d(h), u as u32) }
        }
        374 => unsafe { GetSystemPaletteUse(d(a.w())) },
        375 => {
            let (h, s, n, p) = (a.w(), a.w(), a.w(), a.l());
            unsafe { GetSystemPaletteEntries(d(h), s as u32, n as u32, str32(p)) }
        }
        // StretchDIBits(hdc, x, y, w, h, sx, sy, sw, sh, bits, bmi, usage,
        // rop)
        439 => {
            let (h, x, y, w, hh, sx_, sy_, sw, sh) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i());
            let (bits, bmi, u, rop) = (a.l(), a.l(), a.w(), a.l());
            unsafe { StretchDIBits(d(h), i32_(x), i32_(y), i32_(w), i32_(hh), i32_(sx_), i32_(sy_), i32_(sw), i32_(sh), str32(bits), str32(bmi), u as u32, rop) & 0xffff }
        }
        // SetDIBits, GetDIBits(hdc, hbm, start, scans, bits, bmi, usage)
        440 | 441 => {
            let (h, bm, s, n, bits, bmi, u) = (a.w(), a.w(), a.w(), a.w(), a.l(), a.l(), a.w());
            unsafe { if ord == 440 { SetDIBits(d(h), bm as u32, s as u32, n as u32, str32(bits), str32(bmi), u as u32) } else { GetDIBits(d(h), bm as u32, s as u32, n as u32, str32(bits), str32(bmi), u as u32) } }
        }
        // CreateDIBitmap(hdc, header, init, bits, bmi, usage)
        442 => {
            let (h, hdr, init, bits, bmi, u) = (a.w(), a.l(), a.l(), a.l(), a.l(), a.w());
            unsafe { CreateDIBitmap(d(h), str32(hdr), init, str32(bits), str32(bmi), u as u32) & 0xffff }
        }
        // SetDIBitsToDevice(hdc, x, y, w, h, sx, sy, start, scans, bits,
        // bmi, usage)
        443 => {
            let (h, x, y, w, hh, sx_, sy_, s, n) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i(), a.w(), a.w());
            let (bits, bmi, u) = (a.l(), a.l(), a.w());
            unsafe { SetDIBitsToDevice(d(h), i32_(x), i32_(y), i32_(w), i32_(hh), i32_(sx_), i32_(sy_), s as u32, n as u32, str32(bits), str32(bmi), u as u32) & 0xffff }
        }
        444 => {
            let (l, t, r, b, w, hh) = (a.i(), a.i(), a.i(), a.i(), a.i(), a.i());
            unsafe { CreateRoundRectRgn(i32_(l), i32_(t), i32_(r), i32_(b), i32_(w), i32_(hh)) & 0xffff }
        }
        // CreateDIBPatternBrush(hglobal, usage)
        445 => {
            let (g, u) = (a.w(), a.w());
            let p = flat(((g | 7) as u32) << 16);
            unsafe { CreateDIBPatternBrushPt(p as u32, u as u32) & 0xffff }
        }
        // PolyPolygon(hdc, points, counts, n)
        450 => {
            let (h, p, c, n) = (a.w(), a.l(), a.l(), a.i());
            let counts = ints(c, n.max(0) as usize);
            let total: i32 = counts.iter().sum();
            let v = points(p, total.max(0) as usize);
            unsafe { PolyPolygon(d(h), ptr(&v), ptr(&counts), i32_(n)) }
        }
        // CreatePolyPolygonRgn(points, counts, n, mode)
        451 => {
            let (p, c, n, m) = (a.l(), a.l(), a.i(), a.w());
            let counts = ints(c, n.max(0) as usize);
            let total: i32 = counts.iter().sum();
            let v = points(p, total.max(0) as usize);
            unsafe { CreatePolyPolygonRgn(ptr(&v), ptr(&counts), i32_(n), m as u32) & 0xffff }
        }
        462 => (unsafe { GetObjectType(a.w() as u32) } != 0) as u32,
        // The Ex calls with a POINT or SIZE out: GetBitmapDimensionEx,
        // GetBrushOrgEx, GetCurrentPositionEx, GetViewportExtEx,
        // GetViewportOrgEx, GetWindowExtEx, GetWindowOrgEx.
        468 | 469 | 470 | 472 | 473 | 474 | 475 => {
            let (h, p) = (a.w(), a.l());
            out_point(p, |q| unsafe {
                match ord {
                    468 => GetBitmapDimensionEx(h as u32, q),
                    469 => GetBrushOrgEx(d(h), q),
                    470 => GetCurrentPositionEx(d(h), q),
                    472 => GetViewportExtEx(d(h), q),
                    473 => GetViewportOrgEx(d(h), q),
                    474 => GetWindowExtEx(d(h), q),
                    _ => GetWindowOrgEx(d(h), q),
                }
            })
        }
        // GetTextExtentPoint(hdc, text, n, lpSize)
        471 => {
            let (h, s, n, p) = (a.w(), a.l(), a.i(), a.l());
            out_point(p, |q| unsafe { GetTextExtentPoint32A(d(h), str32(s), i32_(n), q) })
        }
        // OffsetViewportOrgEx, OffsetWindowOrgEx, SetBitmapDimensionEx,
        // SetViewportExtEx, SetViewportOrgEx, SetWindowExtEx,
        // SetWindowOrgEx, MoveToEx(hdc, x, y, lpPoint)
        476..=483 => {
            let (h, x, y, p) = (a.w(), a.i(), a.i(), a.l());
            let (x, y) = (i32_(x), i32_(y));
            if p == 0 {
                return unsafe {
                    match ord {
                        476 => OffsetViewportOrgEx(d(h), x, y, 0),
                        477 => OffsetWindowOrgEx(d(h), x, y, 0),
                        478 => SetBitmapDimensionEx(h as u32, x, y, 0),
                        479 => SetViewportExtEx(d(h), x, y, 0),
                        480 => SetViewportOrgEx(d(h), x, y, 0),
                        481 => SetWindowExtEx(d(h), x, y, 0),
                        482 => SetWindowOrgEx(d(h), x, y, 0),
                        _ => MoveToEx(d(h), x, y, 0),
                    }
                };
            }
            out_point(p, |q| unsafe {
                match ord {
                    476 => OffsetViewportOrgEx(d(h), x, y, q),
                    477 => OffsetWindowOrgEx(d(h), x, y, q),
                    478 => SetBitmapDimensionEx(h as u32, x, y, q),
                    479 => SetViewportExtEx(d(h), x, y, q),
                    480 => SetViewportOrgEx(d(h), x, y, q),
                    481 => SetWindowExtEx(d(h), x, y, q),
                    482 => SetWindowOrgEx(d(h), x, y, q),
                    _ => MoveToEx(d(h), x, y, q),
                }
            })
        }
        // ScaleViewportExtEx, ScaleWindowExtEx(hdc, xn, xd, yn, yd, lpSize)
        484 | 485 => {
            let (h, xn, xd, yn, yd, p) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.l());
            out_point(p, |q| unsafe {
                if ord == 484 { ScaleViewportExtEx(d(h), i32_(xn), i32_(xd), i32_(yn), i32_(yd), q) } else { ScaleWindowExtEx(d(h), i32_(xn), i32_(xd), i32_(yn), i32_(yd), q) }
            })
        }
        // GetAspectRatioFilterEx(hdc, lpSize): no filter.
        486 => {
            let (_h, p) = (a.w(), a.p());
            if !p.is_null() {
                wr32(p, 0, 0);
            }
            1
        }
        // ---- Printing ----
        // StartDoc(hdc, DOCINFO16: cbSize, lpszDocName, lpszOutput)
        377 => {
            let (h, di) = (a.w(), a.p());
            let mut d32 = [20u32, 0, 0, 0, 0];
            if !di.is_null() {
                d32[1] = str32(rd32(di, 2));
                d32[2] = str32(rd32(di, 6));
            }
            unsafe { StartDocA(d(h), d32.as_ptr() as u32) & 0xffff }
        }
        378 => unsafe { EndDoc(d(a.w())) & 0xffff },
        379 => unsafe { StartPage(d(a.w())) & 0xffff },
        380 => unsafe { EndPage(d(a.w())) & 0xffff },
        381 => {
            let (h, p) = (a.w(), a.l());
            unsafe { SetAbortProc(d(h), crate::msg16::proc16to32(p)) & 0xffff }
        }
        382 => unsafe { AbortDoc(d(a.w())) & 0xffff },
        _ => 0,
    }
}
