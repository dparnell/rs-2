//! GOCA drawing orders: what OS/2's GPI keeps in retained segments and
//! metafiles (GpiGetData/GpiPutData give them to applications).
//!
//! Four shapes of order (the Toolkit's pmord.h): one byte (no-op, end of
//! symbol), two bytes (code, value), long (code, length, up to 255 bytes)
//! and very long (0xFE, qualifier, big-endian length, data). Numbers are
//! Intel order; coordinates are 2 bytes (GPIF_SHORT) or 4 (GPIF_LONG).
//! [`decode`] reads both; [`encode`] writes the long format, laid out as
//! IBM's PMGPI builds its orders.

use alloc::vec::Vec;

use crate::Point;

/// An attribute order; each has a "set" and a "push and set" code.
#[derive(Clone, Debug, PartialEq)]
pub enum Attr {
    /// Colour as GpiSetColor takes it (CLR_* or an index/RGB value).
    Color(i32),
    BackColor(i32),
    Mix(i32),
    BackMix(i32),
    /// GSIA: one attribute (1 colour, 2 background colour, 3 mix, 4
    /// background mix) of one primitive type (1 line, 2 char, 3 marker,
    /// 4 area, 5 image); colours in API form.
    Individual { attr: u8, prim: u8, value: i32 },
    LineType(i32),
    /// Cosmetic line width (FIXED).
    LineWidth(i32),
    /// Geometric line width; None: the default.
    LineWidthGeom(Option<i32>),
    LineEnd(i32),
    LineJoin(i32),
    PatternSet(i32),
    Pattern(i32),
    /// None: the default reference point.
    PatternRef(Option<Point>),
    MarkerSet(i32),
    Marker(i32),
    MarkerBox(i32, i32),
    CharSet(i32),
    /// Character box (FIXED width, height).
    CharBox(i32, i32),
    CharAngle(Point),
    CharShear(Point),
    CharDirection(i32),
    CharMode(i32),
    /// FIXED.
    CharExtra(i32),
    CharBreakExtra(i32),
    /// TA_* horizontal, vertical.
    TextAlign(i32, i32),
    ArcParams([i32; 4]),
    Position(Point),
    /// GSTM_* flags (0 unity, 1 after, 2 before, 3 overwrite) and the
    /// matrix elements m11 m12 m21 m22 (FIXED) m41 m42.
    ModelTransform { flags: u8, m: [i32; 6] },
    PickId(i32),
}

/// A decoded order.
#[derive(Clone, Debug, PartialEq)]
pub enum Order {
    Nop,
    Comment(Vec<u8>),
    /// Start of a segment in a metafile or GpiGetData stream (`len`: the
    /// bytes of orders that follow, as written).
    BeginSegment { id: u32, flags: u16, len: u32 },
    EndPrologue,
    Set(Attr),
    Push(Attr),
    Pop,
    /// GSTV: the segment's viewing transform (flags 0 overwrite, 4 after).
    ViewTransform { flags: u8, m: [i32; 6] },
    /// Polyline; `at`: a starting position (else the current position).
    Line { at: Option<Point>, pts: Vec<Point> },
    /// Relative polyline (offsets from the previous point).
    RelLine { at: Option<Point>, d: Vec<(i8, i8)> },
    /// GBOX flags: 0x40 fill, 0x20 outline.
    Box { at: Option<Point>, flags: u8, corner: Point, h: i32, v: i32 },
    /// Multiplier FIXED.
    FullArc { at: Option<Point>, mult: i32 },
    PartialArc { at: Option<Point>, center: Point, mult: i32, start: i32, sweep: i32 },
    Arc { at: Option<Point>, inter: Point, end: Point },
    Bezier { at: Option<Point>, pts: Vec<Point> },
    Fillet { at: Option<Point>, pts: Vec<Point> },
    /// Pairs of points and a FIXED sharpness per pair.
    SharpFillet { at: Option<Point>, pts: Vec<Point>, sharp: Vec<i32> },
    Marker { at: Option<Point>, pts: Vec<Point> },
    CharString { at: Option<Point>, moving: bool, text: Vec<u8> },
    /// Character string extended (GpiCharStringPos/PosAt): GCCHSTE
    /// flags (0x80 opaque rectangle, 0x40 clip, 0x10 leave position,
    /// 0x08 underscore, 0x04 strikeout), rectangle, increments.
    CharStringExt { at: Option<Point>, flags: u8, rect: [Point; 2], text: Vec<u8>, dx: Vec<i32> },
    /// GBAR flags: 0x40 boundary, 0x20 winding.
    BeginArea(u8),
    EndArea,
    BeginPath(i32),
    EndPath,
    CloseFigure,
    /// GFPTH flags: 0x40 winding, 0x20 modify (stroke) first.
    FillPath { flags: u8, id: i32 },
    OutlinePath { flags: u8, id: i32 },
    ModifyPath { mode: u8, id: i32 },
    /// GSCPTH flags: 0x40 winding, 0x20 intersect (0: reset).
    SetClipPath { flags: u8, id: i32 },
    BeginImage { at: Option<Point>, format: u8, cx: u16, cy: u16 },
    ImageData(Vec<u8>),
    EndImage,
    BeginElement { ty: i32, desc: Vec<u8> },
    EndElement,
    Label(i32),
    CallSegment(u32),
    SetPel(Point),
    /// IBM's metafile escapes (0xFE 0xD5): GpiCreateLogFont, GpiErase
    /// and the other calls that are not drawing orders. `ident` is the
    /// escape (see the ESC_* constants), `data` what follows it.
    Escape { ident: u8, data: Vec<u8> },
    /// Anything else, kept as its raw bytes.
    Unknown(Vec<u8>),
}

pub const ESC_CREATELOGCOLORTABLE: u8 = 0x01;
pub const ESC_CREATELOGFONT: u8 = 0x02;
pub const ESC_DELETESETID: u8 = 0x03;
pub const ESC_RESETPS: u8 = 0x05;
pub const ESC_SETBITMAPID: u8 = 0x07;
pub const ESC_SETCP: u8 = 0x08;
pub const ESC_SETDEFAULTVIEWMATRIX: u8 = 0x09;
pub const ESC_ERASE: u8 = 0x0c;
pub const ESC_INTERSECTCLIPRECT: u8 = 0x0d;
pub const ESC_OFFSETCLIPREGION: u8 = 0x0e;
pub const ESC_EXCLUDECLIPRECT: u8 = 0x0f;
pub const ESC_SAVEPS: u8 = 0x14;
pub const ESC_RESTOREPS: u8 = 0x15;
pub const ESC_SETDEFATTRS: u8 = 0x16;
pub const ESC_SETDEFVIEWINGLIMITS: u8 = 0x18;
pub const ESC_SETDEFARCPARAMS: u8 = 0x19;

/// Identity in the order matrix layout (m11 m12 m21 m22 m41 m42).
pub const IDENTITY: [i32; 6] = [0x10000, 0, 0, 0x10000, 0, 0];

/// (attribute, set code, push code). The first code of an attribute is
/// the one [`encode`] writes.
const ATTR_CODES: &[(u8, u8, u8)] = &[
    (0, 0xa6, 0xe6), // colour (also 0x0a, 0x26 decoded)
    (1, 0xa7, 0xe7), // background colour (also 0x25)
    (2, 0x0c, 0x4c),
    (3, 0x0d, 0x4d),
    (4, 0x14, 0x54),
    (5, 0x18, 0x58),
    (6, 0x11, 0x51), // fractional line width (also 0x19)
    (7, 0x15, 0x55),
    (8, 0x1a, 0x5a),
    (9, 0x1b, 0x5b),
    (10, 0x08, 0x48),
    (11, 0x28, 0x09),
    (12, 0xa0, 0xe0),
    (13, 0x3c, 0x7c),
    (14, 0x29, 0x69),
    (15, 0x37, 0x77),
    (16, 0x38, 0x78),
    (17, 0x33, 0x03),
    (18, 0x34, 0x74),
    (19, 0x35, 0x75),
    (20, 0x3a, 0x7a),
    (21, 0x39, 0x79),
    (22, 0x17, 0x57),
    (23, 0x05, 0x45),
    (24, 0x10, 0x50),
    (25, 0x22, 0x62),
    (26, 0x21, 0x61),
    (27, 0x24, 0x64),
    (28, 0x43, 0x23),
    (0, 0x0a, 0x4a),
    (0, 0x26, 0x66),
    (1, 0x25, 0x65),
    (6, 0x19, 0x59),
];

fn attr_kind(a: &Attr) -> u8 {
    match a {
        Attr::Color(_) => 0,
        Attr::BackColor(_) => 1,
        Attr::Mix(_) => 2,
        Attr::BackMix(_) => 3,
        Attr::Individual { .. } => 4,
        Attr::LineType(_) => 5,
        Attr::LineWidth(_) => 6,
        Attr::LineWidthGeom(_) => 7,
        Attr::LineEnd(_) => 8,
        Attr::LineJoin(_) => 9,
        Attr::PatternSet(_) => 10,
        Attr::Pattern(_) => 11,
        Attr::PatternRef(_) => 12,
        Attr::MarkerSet(_) => 13,
        Attr::Marker(_) => 14,
        Attr::MarkerBox(..) => 15,
        Attr::CharSet(_) => 16,
        Attr::CharBox(..) => 17,
        Attr::CharAngle(_) => 18,
        Attr::CharShear(_) => 19,
        Attr::CharDirection(_) => 20,
        Attr::CharMode(_) => 21,
        Attr::CharExtra(_) => 22,
        Attr::CharBreakExtra(_) => 23,
        Attr::TextAlign(..) => 24,
        Attr::ArcParams(_) => 25,
        Attr::Position(_) => 26,
        Attr::ModelTransform { .. } => 27,
        Attr::PickId(_) => 28,
    }
}

fn attr_of_code(code: u8) -> Option<(u8, bool)> {
    ATTR_CODES.iter().find_map(|&(k, s, p)| {
        if s == code {
            Some((k, false))
        } else if p == code {
            Some((k, true))
        } else {
            None
        }
    })
}

/// A two-byte order (code, one value byte).
pub fn short_order(code: u8) -> bool {
    (code ^ 0x80) & 0x88 == 0x88
}

// ---- Colours ----

/// API colour from an order's flags and 3-byte value (GSICOL, GSIA):
/// special values are the negative CLR_* ones, the default flag
/// CLR_DEFAULT.
fn api_color(flags: u8, v: [u8; 3]) -> i32 {
    let n = v[0] as i32 | (v[1] as i32) << 8 | (v[2] as i32) << 16;
    if flags & 0x80 != 0 {
        -3
    } else if flags & 0x40 != 0 {
        -n
    } else {
        n
    }
}

fn order_color(c: i32) -> (u8, [u8; 3]) {
    let (f, n) = if c == -3 {
        (0x80, 0)
    } else if c < 0 {
        (0x40, -c)
    } else {
        (0, c)
    };
    (f, [n as u8, (n >> 8) as u8, (n >> 16) as u8])
}

/// API colour of a 2-byte extended colour (GSECOL, GSBCOL) or a 1-byte
/// GSCOL value, as IBM's PMGPI maps them.
fn ext_color(v: u16) -> i32 {
    match v {
        0 | 0xff00 => -3,
        7 => -2,
        8 => -1,
        0xff01..=0xff07 => (v & 0xff) as i32,
        0xff08 => 0,
        0xff09 => -4,
        0xff0a => -5,
        v => v as i32,
    }
}

// ---- Reading ----

struct Rd<'a> {
    d: &'a [u8],
    p: usize,
    long: bool,
}

impl Rd<'_> {
    fn left(&self) -> usize {
        self.d.len().saturating_sub(self.p)
    }

    fn u8(&mut self) -> Option<u8> {
        let v = *self.d.get(self.p)?;
        self.p += 1;
        Some(v)
    }

    fn i8(&mut self) -> Option<i8> {
        self.u8().map(|v| v as i8)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(self.u8()? as u16 | (self.u8()? as u16) << 8)
    }

    fn u16be(&mut self) -> Option<u16> {
        Some((self.u8()? as u16) << 8 | self.u8()? as u16)
    }

    fn i32(&mut self) -> Option<i32> {
        Some(self.u16()? as i32 | (self.u16()? as i32) << 16)
    }

    fn u32be(&mut self) -> Option<u32> {
        Some((self.u16be()? as u32) << 16 | self.u16be()? as u32)
    }

    /// A coordinate (2 or 4 bytes).
    fn c(&mut self) -> Option<i32> {
        if self.long { self.i32() } else { self.u16().map(|v| v as i16 as i32) }
    }

    fn pt(&mut self) -> Option<Point> {
        Some(Point::new(self.c()?, self.c()?))
    }

    fn pts(&mut self) -> Vec<Point> {
        let w = if self.long { 8 } else { 4 };
        let mut v = Vec::new();
        while self.left() >= w {
            v.push(self.pt().unwrap());
        }
        v
    }

    /// A multiplier: FIXED (long) or FIXED88 (short).
    fn mult(&mut self) -> Option<i32> {
        if self.long {
            self.i32()
        } else {
            Some((self.u8()? as i32) << 8 | (self.u8()? as i32) << 16)
        }
    }

    fn rest(&mut self) -> Vec<u8> {
        let v = self.d[self.p.min(self.d.len())..].to_vec();
        self.p = self.d.len();
        v
    }
}

fn at(given: bool, r: &mut Rd) -> Option<Option<Point>> {
    if given { r.pt().map(Some) } else { Some(None) }
}

fn matrix(r: &mut Rd, mask: u16) -> Option<[i32; 6]> {
    // Elements in mask order (M11 M12 M13 M14 M21 ... M44); the ones an
    // order can carry are M11 M12 M21 M22 M41 M42.
    let mut m = IDENTITY;
    let long = r.long;
    for bit in 0..16 {
        if mask & (0x8000 >> bit) == 0 {
            continue;
        }
        let v = r.c()?;
        // Short scale elements are FIXED 2.14.
        let scale = |v: i32| if long { v } else { v << 2 };
        match bit {
            0 => m[0] = scale(v),
            1 => m[1] = scale(v),
            4 => m[2] = scale(v),
            5 => m[3] = scale(v),
            12 => m[4] = v,
            13 => m[5] = v,
            _ => {}
        }
    }
    Some(m)
}

fn decode_attr(kind: u8, code: u8, r: &mut Rd) -> Option<Attr> {
    Some(match kind {
        0 => match code {
            0x0a | 0x4a => Attr::Color(ext_color(r.u8()? as u16)),
            0x26 | 0x66 => Attr::Color(ext_color(r.u16be()?)),
            _ => {
                let f = r.u8()?;
                Attr::Color(api_color(f, [r.u8()?, r.u8()?, r.u8()?]))
            }
        },
        1 => match code {
            0x25 | 0x65 => Attr::BackColor(ext_color(r.u16be()?)),
            _ => {
                let f = r.u8()?;
                Attr::BackColor(api_color(f, [r.u8()?, r.u8()?, r.u8()?]))
            }
        },
        2 => Attr::Mix(r.u8()? as i32),
        3 => Attr::BackMix(r.u8()? as i32),
        4 => {
            let (attr, prim, f) = (r.u8()?, r.u8()?, r.u8()?);
            let v = [r.u8()?, r.u8().unwrap_or(0), r.u8().unwrap_or(0)];
            let value = if attr <= 2 { api_color(f, v) } else if f & 0x80 != 0 { 0 } else { v[0] as i32 };
            Attr::Individual { attr, prim, value }
        }
        5 => Attr::LineType(r.u8()? as i32),
        6 => match code {
            0x19 | 0x59 => Attr::LineWidth((r.u8()? as i32) << 16),
            _ => {
                let (frac, int) = (r.u8()?, r.u8()?);
                Attr::LineWidth((int as i32) << 16 | (frac as i32) << 8)
            }
        },
        7 => {
            let f = r.u8()?;
            r.u8()?;
            let w = r.c()?;
            Attr::LineWidthGeom(if f & 0x80 != 0 { None } else { Some(w) })
        }
        8 => Attr::LineEnd(r.u8()? as i32),
        9 => Attr::LineJoin(r.u8()? as i32),
        10 => Attr::PatternSet(r.u8()? as i32),
        11 => Attr::Pattern(r.u8()? as i32),
        12 => {
            let f = r.u8()?;
            r.u8()?;
            if f & 0x80 != 0 { Attr::PatternRef(None) } else { Attr::PatternRef(Some(r.pt()?)) }
        }
        13 => Attr::MarkerSet(r.u8()? as i32),
        14 => Attr::Marker(r.u8()? as i32),
        15 => Attr::MarkerBox(r.c()? << 16, r.c()? << 16),
        16 => Attr::CharSet(r.u8()? as i32),
        17 => {
            let (cx, cy) = (r.c()?, r.c()?);
            let (fx, fy) = if r.left() >= 4 { (r.u16()? as i32, r.u16()? as i32) } else { (0, 0) };
            Attr::CharBox(cx << 16 | fx, cy << 16 | fy)
        }
        18 => Attr::CharAngle(r.pt()?),
        19 => Attr::CharShear(r.pt()?),
        20 => Attr::CharDirection(r.u8()? as i32),
        21 => Attr::CharMode(r.u8()? as i32),
        22 | 23 => {
            let f = r.u8()?;
            r.u8()?;
            let v = if f != 0 { 0 } else { r.i32()? };
            if kind == 22 { Attr::CharExtra(v) } else { Attr::CharBreakExtra(v) }
        }
        24 => {
            let (h, v) = (r.u8()?, r.u8()?);
            let h = if h == 0xff { 5 } else { h as i32 };
            let v = match v {
                4 => 3,
                5 => 4,
                6 => 5,
                0xff => 6,
                v => v as i32,
            };
            Attr::TextAlign(h, v)
        }
        25 => Attr::ArcParams([r.c()?, r.c()?, r.c()?, r.c()?]),
        26 => Attr::Position(r.pt()?),
        27 => {
            r.u8()?;
            let flags = r.u8()?;
            let mask = r.u16be()?;
            Attr::ModelTransform { flags, m: matrix(r, mask)? }
        }
        28 => Attr::PickId(r.i32().or_else(|| r.u8().map(|v| v as i32))?),
        _ => return None,
    })
}

/// Decode one order's body (`code`, data after the length).
fn decode_long(code: u8, data: &[u8], long: bool) -> Option<Order> {
    let r = &mut Rd { d: data, p: 0, long };
    if let Some((kind, push)) = attr_of_code(code) {
        let a = decode_attr(kind, code, r)?;
        return Some(if push { Order::Push(a) } else { Order::Set(a) });
    }
    let given = code & 0x40 != 0;
    Some(match code {
        0x01 => Order::Comment(r.rest()),
        0x07 => {
            r.u16()?;
            Order::CallSegment(r.i32()? as u32)
        }
        0x31 => {
            r.u8()?;
            let flags = r.u8()?;
            let mask = r.u16be()?;
            Order::ViewTransform { flags, m: matrix(r, mask)? }
        }
        0x60 => Order::EndArea,
        0x70 => {
            let id = r.u32be()?;
            let flags = r.u16be()?;
            let lo = r.u16be()? as u32;
            r.u32be();
            let hi = r.u16be().unwrap_or(0) as u32;
            Order::BeginSegment { id, flags, len: hi << 16 | lo }
        }
        0x81 | 0xc1 => Order::Line { at: at(given, r)?, pts: r.pts() },
        0xa1 | 0xe1 => {
            let a = at(given, r)?;
            let mut d = Vec::new();
            while r.left() >= 2 {
                d.push((r.i8()?, r.i8()?));
            }
            Order::RelLine { at: a, d }
        }
        0x80 | 0xc0 => {
            let flags = r.u8()?;
            r.u8()?;
            let a = at(given, r)?;
            let corner = r.pt()?;
            let (h, v) = if r.left() > 0 { (r.c()?, r.c()?) } else { (0, 0) };
            Order::Box { at: a, flags, corner, h, v }
        }
        0x87 | 0xc7 => Order::FullArc { at: at(given, r)?, mult: r.mult()? },
        0xa3 | 0xe3 => {
            let a = at(given, r)?;
            let center = r.pt()?;
            let mult = r.mult()?;
            Order::PartialArc { at: a, center, mult, start: r.i32()?, sweep: r.i32()? }
        }
        0x86 | 0xc6 => Order::Arc { at: at(given, r)?, inter: r.pt()?, end: r.pt()? },
        0xa5 | 0xe5 => Order::Bezier { at: at(given, r)?, pts: r.pts() },
        0x85 | 0xc5 => Order::Fillet { at: at(given, r)?, pts: r.pts() },
        0xa4 | 0xe4 => {
            let a = at(given, r)?;
            // n pairs of points (2 coords each) then n FIXED sharpnesses.
            let w = if long { 16 } else { 8 };
            let n = r.left() / (w + 4);
            let pts = (0..2 * n).filter_map(|_| r.pt()).collect();
            let sharp = (0..n).filter_map(|_| r.i32()).collect();
            Order::SharpFillet { at: a, pts, sharp }
        }
        0x82 | 0xc2 => {
            let mut pts = r.pts();
            let a = if given && !pts.is_empty() { Some(pts.remove(0)) } else { None };
            Order::Marker { at: a, pts }
        }
        0x83 | 0xc3 | 0xb1 | 0xf1 => Order::CharString { at: at(given, r)?, moving: code & 0x30 == 0x30, text: r.rest() },
        0x91 | 0xd1 => {
            let a = at(given, r)?;
            let format = r.u8()?;
            r.u8()?;
            Order::BeginImage { at: a, format, cx: r.u16be()?, cy: r.u16be()? }
        }
        0x92 => Order::ImageData(r.rest()),
        0x93 => Order::EndImage,
        0xb4 => {
            let flags = r.u8()?;
            r.u8()?;
            Order::SetClipPath { flags, id: r.i32()? }
        }
        0xd0 => {
            r.u16()?;
            Order::BeginPath(r.i32()?)
        }
        0xd2 => Order::BeginElement { ty: r.i32()?, desc: r.rest() },
        0xd3 => Order::Label(r.i32()?),
        0xd4 | 0xd7 | 0xd8 => {
            let flags = r.u8()?;
            r.u8()?;
            let id = r.i32()?;
            match code {
                0xd4 => Order::OutlinePath { flags, id },
                0xd7 => Order::FillPath { flags, id },
                _ => Order::ModifyPath { mode: flags, id },
            }
        }
        0xd5 => {
            let (ty, ident) = (r.u8()?, r.u8()?);
            if ty == 0x80 && ident == 1 {
                Order::SetPel(r.pt()?)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// Decode an order stream. A truncated last order ends it.
pub fn decode(data: &[u8], long: bool) -> Vec<Order> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < data.len() {
        let code = data[p];
        let (len, body) = match code {
            0x00 | 0xff => (1, 1),
            0xfe => {
                if p + 4 > data.len() {
                    break;
                }
                let n = (data[p + 2] as usize) << 8 | data[p + 3] as usize;
                (4 + n, 4)
            }
            c if short_order(c) => (2, 1),
            _ => match data.get(p + 1) {
                Some(&n) => (2 + n as usize, 2),
                None => break,
            },
        };
        if p + len > data.len() {
            break;
        }
        let raw = &data[p..p + len];
        let o = match code {
            0x00 | 0xff => Some(Order::Nop),
            0x3e => Some(Order::EndPrologue),
            0x3f => Some(Order::Pop),
            0x68 => Some(Order::BeginArea(raw[1])),
            0x7d => Some(Order::CloseFigure),
            0x7f => Some(Order::EndPath),
            0x49 => Some(Order::EndElement),
            0xfe => decode_vlong(raw[1], &raw[4..], long),
            c if short_order(c) => attr_of_code(c).and_then(|(k, push)| {
                let a = decode_attr(k, c, &mut Rd { d: &raw[1..], p: 0, long })?;
                Some(if push { Order::Push(a) } else { Order::Set(a) })
            }),
            c => decode_long(c, &raw[body..], long),
        };
        out.push(o.unwrap_or_else(|| Order::Unknown(raw.to_vec())));
        p += len;
    }
    out
}

fn decode_vlong(q: u8, data: &[u8], long: bool) -> Option<Order> {
    let r = &mut Rd { d: data, p: 0, long };
    match q {
        0xd5 => {
            r.u8()?;
            let ident = r.u8()?;
            Some(Order::Escape { ident, data: r.rest() })
        }
        0xb0 | 0xf0 => {
            let a = at(q == 0xf0, r)?;
            let flags = r.u8()?;
            r.u8()?;
            let rect = [r.pt()?, r.pt()?];
            let n = r.u16be()? as usize;
            if r.left() < n {
                return None;
            }
            let text = data[r.p..r.p + n].to_vec();
            r.p += n + (n & 1);
            let mut dx = Vec::new();
            while r.left() >= if long { 4 } else { 2 } && dx.len() < n {
                dx.push(r.c()?);
            }
            Some(Order::CharStringExt { at: a, flags, rect, text, dx })
        }
        _ => None,
    }
}

// ---- Writing (long format) ----

struct Wr<'a>(&'a mut Vec<u8>);

impl Wr<'_> {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u16be(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn pt(&mut self, p: Point) {
        self.i32(p.x);
        self.i32(p.y);
    }
}

/// Append a long order (code, length, body written by `f`).
fn long(out: &mut Vec<u8>, code: u8, f: impl FnOnce(&mut Wr)) {
    let at = out.len();
    out.push(code);
    out.push(0);
    f(&mut Wr(out));
    out[at + 1] = (out.len() - at - 2).min(255) as u8;
}

/// Append a very long order (0xFE, qualifier, big-endian length).
fn vlong(out: &mut Vec<u8>, q: u8, f: impl FnOnce(&mut Wr)) {
    let at = out.len();
    out.extend_from_slice(&[0xfe, q, 0, 0]);
    f(&mut Wr(out));
    let n = (out.len() - at - 4) as u16;
    out[at + 2..at + 4].copy_from_slice(&n.to_be_bytes());
}

fn encode_attr(a: &Attr, push: bool, out: &mut Vec<u8>) {
    let k = attr_kind(a);
    let &(_, s, p) = ATTR_CODES.iter().find(|c| c.0 == k).unwrap();
    let code = if push { p } else { s };
    let two = |out: &mut Vec<u8>, v: i32| out.extend_from_slice(&[code, v as u8]);
    match *a {
        Attr::Color(c) | Attr::BackColor(c) => long(out, code, |w| {
            let (f, v) = order_color(c);
            w.u8(f);
            v.iter().for_each(|&b| w.u8(b));
        }),
        Attr::Mix(v) | Attr::BackMix(v) | Attr::LineType(v) | Attr::LineEnd(v) | Attr::LineJoin(v) | Attr::PatternSet(v) | Attr::Pattern(v) | Attr::MarkerSet(v) | Attr::Marker(v) | Attr::CharSet(v) | Attr::CharDirection(v) | Attr::CharMode(v) => two(out, v),
        Attr::Individual { attr, prim, value } => long(out, code, |w| {
            w.u8(attr);
            w.u8(prim);
            if attr <= 2 {
                let (f, v) = order_color(value);
                w.u8(f);
                v.iter().for_each(|&b| w.u8(b));
            } else {
                w.u8(0);
                w.u8(value as u8);
                w.u8(0);
                w.u8(0);
            }
        }),
        Attr::LineWidth(fx) => {
            let int = fx >> 16;
            let (frac, int) = if int > 255 { (0xff, 0xff) } else { ((fx >> 8) as u8, int as u8) };
            out.extend_from_slice(&[code, 2, frac, int]);
        }
        Attr::LineWidthGeom(w) => long(out, code, |o| {
            o.u8(if w.is_none() { 0x80 } else { 0 });
            o.u8(0);
            o.i32(w.unwrap_or(0));
        }),
        Attr::PatternRef(p) => long(out, code, |w| {
            w.u8(if p.is_none() { 0x80 } else { 0 });
            w.u8(0);
            w.pt(p.unwrap_or_default());
        }),
        Attr::MarkerBox(cx, cy) => long(out, code, |w| {
            w.i32(cx >> 16);
            w.i32(cy >> 16);
            w.u8(0);
            w.u8(0);
        }),
        Attr::CharBox(cx, cy) => long(out, code, |w| {
            w.i32((cx >> 16) & 0xffff);
            w.i32((cy >> 16) & 0xffff);
            w.u16(cx as u16);
            w.u16(cy as u16);
            w.u8(0x80);
            w.u8(0);
        }),
        Attr::CharAngle(p) | Attr::CharShear(p) | Attr::Position(p) => long(out, code, |w| w.pt(p)),
        Attr::CharExtra(v) | Attr::CharBreakExtra(v) => long(out, code, |w| {
            w.u8(0);
            w.u8(0);
            w.i32(v);
        }),
        Attr::TextAlign(h, v) => {
            let h = if h == 5 { 0xff } else { h as u8 };
            let v = match v {
                3 => 4,
                4 => 5,
                5 => 6,
                6 => 0xff,
                v => v as u8,
            };
            out.extend_from_slice(&[code, 2, h, v]);
        }
        Attr::ArcParams(p) => long(out, code, |w| p.iter().for_each(|&v| w.i32(v))),
        Attr::ModelTransform { flags, m } => long(out, code, |w| {
            w.u8(0);
            w.u8(flags);
            w.u16be(0xcc0c);
            m.iter().for_each(|&v| w.i32(v));
        }),
        Attr::PickId(v) => long(out, code, |w| w.i32(v)),
    }
}

/// Points per long polyline-type order (8 bytes each, 255 max).
const PTS_PER_ORDER: usize = 30;

fn poly(out: &mut Vec<u8>, cur: u8, given: u8, at: Option<Point>, pts: &[Point]) {
    let mut first = true;
    let mut rest = pts;
    loop {
        let n = rest.len().min(if first && at.is_some() { PTS_PER_ORDER - 1 } else { PTS_PER_ORDER });
        let (chunk, tail) = rest.split_at(n);
        match (first, at) {
            (true, Some(p)) => long(out, given, |w| {
                w.pt(p);
                chunk.iter().for_each(|&q| w.pt(q));
            }),
            _ => long(out, cur, |w| chunk.iter().for_each(|&q| w.pt(q))),
        }
        first = false;
        rest = tail;
        if rest.is_empty() {
            break;
        }
    }
}

/// Append an order in the long (GPIF_LONG) format. Orders too long for
/// one order (polylines, strings) are split as PMGPI splits them.
pub fn encode(o: &Order, out: &mut Vec<u8>) {
    match o {
        Order::Nop => out.push(0),
        Order::Comment(d) => long(out, 0x01, |w| d.iter().take(255).for_each(|&b| w.u8(b))),
        Order::BeginSegment { id, flags, len } => long(out, 0x70, |w| {
            w.u16be((id >> 16) as u16);
            w.u16be(*id as u16);
            w.u16be(*flags);
            w.u16be(*len as u16);
            w.i32(0);
            w.u16be((len >> 16) as u16);
        }),
        Order::EndPrologue => out.extend_from_slice(&[0x3e, 0]),
        Order::Set(a) => encode_attr(a, false, out),
        Order::Push(a) => encode_attr(a, true, out),
        Order::Pop => out.extend_from_slice(&[0x3f, 0]),
        Order::ViewTransform { flags, m } => long(out, 0x31, |w| {
            w.u8(0);
            w.u8(*flags);
            w.u16be(0xcc0c);
            m.iter().for_each(|&v| w.i32(v));
        }),
        Order::Line { at, pts } => poly(out, 0x81, 0xc1, *at, pts),
        Order::RelLine { at, d } => long(out, if at.is_some() { 0xe1 } else { 0xa1 }, |w| {
            if let Some(p) = at {
                w.pt(*p);
            }
            for &(x, y) in d.iter().take(120) {
                w.u8(x as u8);
                w.u8(y as u8);
            }
        }),
        Order::Box { at, flags, corner, h, v } => long(out, if at.is_some() { 0xc0 } else { 0x80 }, |w| {
            w.u8(*flags);
            w.u8(0);
            if let Some(p) = at {
                w.pt(*p);
            }
            w.pt(*corner);
            w.i32(*h);
            w.i32(*v);
        }),
        Order::FullArc { at, mult } => long(out, if at.is_some() { 0xc7 } else { 0x87 }, |w| {
            if let Some(p) = at {
                w.pt(*p);
            }
            w.i32(*mult);
        }),
        Order::PartialArc { at, center, mult, start, sweep } => long(out, if at.is_some() { 0xe3 } else { 0xa3 }, |w| {
            if let Some(p) = at {
                w.pt(*p);
            }
            w.pt(*center);
            w.i32(*mult);
            w.i32(*start);
            w.i32(*sweep);
        }),
        Order::Arc { at, inter, end } => long(out, if at.is_some() { 0xc6 } else { 0x86 }, |w| {
            if let Some(p) = at {
                w.pt(*p);
            }
            w.pt(*inter);
            w.pt(*end);
        }),
        Order::Bezier { at, pts } => poly(out, 0xa5, 0xe5, *at, pts),
        Order::Fillet { at, pts } => poly(out, 0x85, 0xc5, *at, pts),
        Order::SharpFillet { at, pts, sharp } => {
            // 12 pairs per order, as PMGPI.
            let mut i = 0;
            let n = sharp.len().min(pts.len() / 2);
            let mut first = true;
            while first || i < n {
                let k = (n - i).min(12);
                let given = first && at.is_some();
                long(out, if given { 0xe4 } else { 0xa4 }, |w| {
                    if given {
                        w.pt(at.unwrap());
                    }
                    pts[2 * i..2 * (i + k)].iter().for_each(|&p| w.pt(p));
                    sharp[i..i + k].iter().for_each(|&s| w.i32(s));
                });
                i += k;
                first = false;
            }
        }
        Order::Marker { at, pts } => {
            let mut all = Vec::new();
            all.extend(at.iter().copied());
            all.extend_from_slice(pts);
            for c in all.chunks(PTS_PER_ORDER) {
                long(out, 0xc2, |w| c.iter().for_each(|&p| w.pt(p)));
            }
        }
        Order::CharString { at, moving, text } => {
            let mut first = true;
            let mut rest: &[u8] = text;
            loop {
                let given = first && at.is_some();
                let max = if given { 247 } else { 255 };
                let (chunk, tail) = rest.split_at(rest.len().min(max));
                let code = match (given, moving) {
                    (true, false) => 0xc3,
                    (false, false) => 0x83,
                    (true, true) => 0xf1,
                    (false, true) => 0xb1,
                };
                long(out, code, |w| {
                    if given {
                        w.pt(at.unwrap());
                    }
                    chunk.iter().for_each(|&b| w.u8(b));
                });
                first = false;
                rest = tail;
                if rest.is_empty() {
                    break;
                }
            }
        }
        Order::CharStringExt { at, flags, rect, text, dx } => vlong(out, if at.is_some() { 0xf0 } else { 0xb0 }, |w| {
            if let Some(p) = at {
                w.pt(*p);
            }
            w.u8(*flags);
            w.u8(0);
            w.pt(rect[0]);
            w.pt(rect[1]);
            w.u16be(text.len() as u16);
            text.iter().for_each(|&b| w.u8(b));
            if text.len() & 1 != 0 {
                w.u8(0);
            }
            dx.iter().for_each(|&d| w.i32(d));
        }),
        Order::BeginArea(f) => out.extend_from_slice(&[0x68, *f]),
        Order::EndArea => out.extend_from_slice(&[0x60, 0]),
        Order::BeginPath(id) => long(out, 0xd0, |w| {
            w.u16(0);
            w.i32(*id);
        }),
        Order::EndPath => out.extend_from_slice(&[0x7f, 0]),
        Order::CloseFigure => out.extend_from_slice(&[0x7d, 0]),
        Order::FillPath { flags, id } | Order::OutlinePath { flags, id } | Order::ModifyPath { mode: flags, id } | Order::SetClipPath { flags, id } => {
            let code = match o {
                Order::FillPath { .. } => 0xd7,
                Order::OutlinePath { .. } => 0xd4,
                Order::ModifyPath { .. } => 0xd8,
                _ => 0xb4,
            };
            long(out, code, |w| {
                w.u8(*flags);
                w.u8(0);
                w.i32(*id);
            })
        }
        Order::BeginImage { at, format, cx, cy } => long(out, if at.is_some() { 0xd1 } else { 0x91 }, |w| {
            if let Some(p) = at {
                w.pt(*p);
            }
            w.u8(*format);
            w.u8(0);
            w.u16be(*cx);
            w.u16be(*cy);
        }),
        Order::ImageData(d) => long(out, 0x92, |w| d.iter().take(255).for_each(|&b| w.u8(b))),
        Order::EndImage => out.extend_from_slice(&[0x93, 0]),
        Order::BeginElement { ty, desc } => long(out, 0xd2, |w| {
            w.i32(*ty);
            desc.iter().take(251).for_each(|&b| w.u8(b));
        }),
        Order::EndElement => out.extend_from_slice(&[0x49, 0]),
        Order::Label(l) => long(out, 0xd3, |w| w.i32(*l)),
        Order::CallSegment(id) => long(out, 0x07, |w| {
            w.u16(0);
            w.i32(*id as i32);
        }),
        Order::SetPel(p) => long(out, 0xd5, |w| {
            w.u8(0x80);
            w.u8(1);
            w.pt(*p);
        }),
        Order::Escape { ident, data } => vlong(out, 0xd5, |w| {
            w.u8(0);
            w.u8(*ident);
            data.iter().for_each(|&b| w.u8(b));
        }),
        Order::Unknown(raw) => out.extend_from_slice(raw),
    }
}

/// Encode a list of orders.
pub fn encode_all(orders: &[Order]) -> Vec<u8> {
    let mut v = Vec::new();
    for o in orders {
        encode(o, &mut v);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn p(x: i32, y: i32) -> Point {
        Point::new(x, y)
    }

    fn round_trip(o: Order) {
        let mut v = Vec::new();
        encode(&o, &mut v);
        let d = decode(&v, true);
        assert_eq!(d, vec![o], "bytes {:02x?}", v);
    }

    #[test]
    fn attributes_round_trip() {
        for a in [
            Attr::Color(2),
            Attr::Color(-1),
            Attr::Color(-3),
            Attr::Color(0xff_8040),
            Attr::BackColor(-2),
            Attr::Mix(5),
            Attr::BackMix(2),
            Attr::Individual { attr: 1, prim: 4, value: 7 },
            Attr::Individual { attr: 3, prim: 1, value: 4 },
            Attr::LineType(3),
            Attr::LineWidth(0x20000),
            Attr::LineWidthGeom(Some(40)),
            Attr::LineWidthGeom(None),
            Attr::LineEnd(2),
            Attr::LineJoin(3),
            Attr::PatternSet(0),
            Attr::Pattern(16),
            Attr::PatternRef(Some(p(3, -4))),
            Attr::PatternRef(None),
            Attr::MarkerSet(0),
            Attr::Marker(4),
            Attr::MarkerBox(10 << 16, 12 << 16),
            Attr::CharSet(3),
            Attr::CharBox(0x14_8000, 0x1e_0000),
            Attr::CharAngle(p(1, 1)),
            Attr::CharShear(p(0, 1)),
            Attr::CharDirection(2),
            Attr::CharMode(3),
            Attr::CharExtra(0x18000),
            Attr::CharBreakExtra(0x10000),
            Attr::TextAlign(3, 4),
            Attr::TextAlign(5, 6),
            Attr::ArcParams([2, 1, 0, 0]),
            Attr::Position(p(-100, 2000)),
            Attr::ModelTransform { flags: 3, m: [0x20000, 0, 0, 0x8000, 10, -20] },
        ] {
            round_trip(Order::Set(a.clone()));
            round_trip(Order::Push(a));
        }
    }

    #[test]
    fn primitives_round_trip() {
        round_trip(Order::Line { at: None, pts: vec![p(1, 2), p(3, 4)] });
        round_trip(Order::Line { at: Some(p(0, 0)), pts: vec![p(10, 20)] });
        round_trip(Order::RelLine { at: Some(p(5, 5)), d: vec![(1, -1), (-128, 127)] });
        round_trip(Order::Box { at: None, flags: 0x60, corner: p(100, 50), h: 10, v: 5 });
        round_trip(Order::FullArc { at: None, mult: 0x28000 });
        round_trip(Order::PartialArc { at: None, center: p(50, 50), mult: 0x10000, start: 0, sweep: 90 << 16 });
        round_trip(Order::Arc { at: None, inter: p(1, 5), end: p(9, 9) });
        round_trip(Order::Bezier { at: None, pts: vec![p(1, 1), p(2, 2), p(3, 1)] });
        round_trip(Order::Fillet { at: Some(p(0, 0)), pts: vec![p(1, 1), p(2, 0)] });
        round_trip(Order::SharpFillet { at: None, pts: vec![p(1, 1), p(2, 0), p(3, 3), p(4, 0)], sharp: vec![0x10000, 0x8000] });
        round_trip(Order::Marker { at: Some(p(1, 1)), pts: vec![p(5, 5)] });
        round_trip(Order::CharString { at: Some(p(10, 10)), moving: false, text: b"Hello".to_vec() });
        round_trip(Order::CharString { at: None, moving: true, text: b"x".to_vec() });
        round_trip(Order::CharStringExt { at: Some(p(1, 2)), flags: 0x80, rect: [p(0, 0), p(50, 20)], text: b"abc".to_vec(), dx: vec![8, 8, 8] });
        round_trip(Order::CharStringExt { at: None, flags: 0, rect: [p(0, 0), p(0, 0)], text: b"ab".to_vec(), dx: vec![] });
        round_trip(Order::BeginArea(0xc0));
        round_trip(Order::EndArea);
        round_trip(Order::BeginPath(1));
        round_trip(Order::EndPath);
        round_trip(Order::CloseFigure);
        round_trip(Order::FillPath { flags: 0x40, id: 1 });
        round_trip(Order::OutlinePath { flags: 0, id: 1 });
        round_trip(Order::ModifyPath { mode: 6, id: 1 });
        round_trip(Order::SetClipPath { flags: 0x20, id: 1 });
        round_trip(Order::BeginImage { at: None, format: 0, cx: 16, cy: 2 });
        round_trip(Order::ImageData(vec![0xff, 0x00]));
        round_trip(Order::EndImage);
        round_trip(Order::BeginElement { ty: 7, desc: b"el".to_vec() });
        round_trip(Order::EndElement);
        round_trip(Order::Label(9));
        round_trip(Order::CallSegment(4));
        round_trip(Order::SetPel(p(3, 3)));
        round_trip(Order::Escape { ident: ESC_ERASE, data: vec![] });
        round_trip(Order::Escape { ident: ESC_SETCP, data: vec![0x52, 3, 0, 0] });
        round_trip(Order::BeginSegment { id: 0x5816_4791, flags: 0x7000, len: 0x1_2345 });
        round_trip(Order::ViewTransform { flags: 0, m: IDENTITY });
        round_trip(Order::Comment(b"note".to_vec()));
        round_trip(Order::EndPrologue);
        round_trip(Order::Pop);
    }

    #[test]
    fn long_polylines_split() {
        let pts: Vec<Point> = (0..70).map(|i| p(i, i * 2)).collect();
        let v = encode_all(&[Order::Line { at: None, pts: pts.clone() }]);
        let d = decode(&v, true);
        assert_eq!(d.len(), 3);
        let all: Vec<Point> = d.iter().flat_map(|o| if let Order::Line { pts, .. } = o { pts.clone() } else { vec![] }).collect();
        assert_eq!(all, pts);
        let text = vec![b'a'; 600];
        let v = encode_all(&[Order::CharString { at: Some(p(0, 0)), moving: false, text: text.clone() }]);
        let d = decode(&v, true);
        let all: Vec<u8> = d.iter().flat_map(|o| if let Order::CharString { text, .. } = o { text.clone() } else { vec![] }).collect();
        assert_eq!(all, text);
        assert!(matches!(d[0], Order::CharString { at: Some(_), .. }));
        assert!(matches!(d[1], Order::CharString { at: None, .. }));
    }

    #[test]
    fn ibm_orders() {
        // From an OS/2 metafile: a bundle element, a GSIA area colour,
        // line type, mix, a path with a filled polyline.
        let d = [
            0xd2, 0x04, 0x04, 0xfd, 0x00, 0x00, // begin element (area bundle)
            0x14, 0x06, 0x01, 0x04, 0x40, 0x04, 0x00, 0x00, // GSIA colour area special 4
            0x49, 0x00, // end element
            0x18, 0x07, // line type 7
            0x0c, 0x02, // mix
            0xd0, 0x06, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // begin path 1
            0x21, 0x08, 0x8d, 0xe9, 0xff, 0xff, 0x8f, 0xed, 0xff, 0xff, // GSCP
            0x81, 0x08, 0x8d, 0xe9, 0xff, 0xff, 0xdf, 0xec, 0xff, 0xff, // line
            0x7f, 0x00, // end path
            0xd7, 0x06, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // fill path
            0xa6, 0x04, 0x40, 0x01, 0x00, 0x00, // colour CLR_BLACK
            0x0a, 0x07, // GSCOL 7: CLR_WHITE
        ];
        let o = decode(&d, true);
        assert_eq!(
            o,
            vec![
                Order::BeginElement { ty: 0xfd04, desc: vec![] },
                Order::Set(Attr::Individual { attr: 1, prim: 4, value: -4 }),
                Order::EndElement,
                Order::Set(Attr::LineType(7)),
                Order::Set(Attr::Mix(2)),
                Order::BeginPath(1),
                Order::Set(Attr::Position(p(-5747, -4721))),
                Order::Line { at: None, pts: vec![p(-5747, -4897)] },
                Order::EndPath,
                Order::FillPath { flags: 0, id: 1 },
                Order::Set(Attr::Color(-1)),
                Order::Set(Attr::Color(-2)),
            ]
        );
    }

    #[test]
    fn short_coordinates() {
        // GCLINE with two short points; GSCC short form; GFARC FIXED88.
        let d = [0x81, 0x08, 0x01, 0x00, 0x02, 0x00, 0xff, 0xff, 0xfe, 0xff, 0x33, 0x04, 0x0a, 0x00, 0x0c, 0x00, 0x87, 0x02, 0x80, 0x01];
        assert_eq!(
            decode(&d, false),
            vec![
                Order::Line { at: None, pts: vec![p(1, 2), p(-1, -2)] },
                Order::Set(Attr::CharBox(10 << 16, 12 << 16)),
                Order::FullArc { at: None, mult: 0x18000 },
            ]
        );
    }

    #[test]
    fn unknown_and_truncated() {
        let d = [0x9f, 0x01, 0x00, 0xc1, 0x10, 0x00];
        let o = decode(&d, true);
        assert_eq!(o, vec![Order::Unknown(vec![0x9f, 0x01, 0x00])]);
    }
}
