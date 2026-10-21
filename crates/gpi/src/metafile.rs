//! OS/2 metafiles (.MET): MO:DCA structured fields around GOCA orders.
//!
//! A metafile is a list of structured fields, each `length (2, big
//! endian), id (3), flags, sequence (2), data`. OS/2's GPI writes
//!
//! ```text
//! Begin Document (description triplet), Begin Resource Group,
//!   Begin/End Colour Attribute Table, Begin Graphics Object,
//!     Begin Object Environment Group: Map Colour Attribute Table,
//!       Map Coded Font (one per logical font), End
//!     Graphics Data Descriptor (page units, page size, coordinate
//!       format, default viewing transform)
//!     Graphics Data (the segments' orders, split into fields)
//!   End Graphics Object, End Resource Group, End Document
//! ```
//!
//! [`parse`] reads that (and tolerates other fields); [`build`] writes it
//! the way IBM's PMGPI lays it out.

use alloc::vec::Vec;

use crate::xform;

pub const BDT: [u8; 3] = [0xd3, 0xa8, 0xa8];
pub const EDT: [u8; 3] = [0xd3, 0xa9, 0xa8];
pub const BRG: [u8; 3] = [0xd3, 0xa8, 0xc6];
pub const ERG: [u8; 3] = [0xd3, 0xa9, 0xc6];
pub const BCA: [u8; 3] = [0xd3, 0xa8, 0x77];
pub const ECA: [u8; 3] = [0xd3, 0xa9, 0x77];
pub const CAT: [u8; 3] = [0xd3, 0xb0, 0x77];
pub const BGR: [u8; 3] = [0xd3, 0xa8, 0xbb];
pub const EGR: [u8; 3] = [0xd3, 0xa9, 0xbb];
pub const BOG: [u8; 3] = [0xd3, 0xa8, 0xc7];
pub const EOG: [u8; 3] = [0xd3, 0xa9, 0xc7];
pub const MCA: [u8; 3] = [0xd3, 0xab, 0x77];
pub const MCF: [u8; 3] = [0xd3, 0xab, 0x8a];
pub const GDD: [u8; 3] = [0xd3, 0xa6, 0xbb];
pub const GAD: [u8; 3] = [0xd3, 0xee, 0xbb];

/// Largest Graphics Data field PMGPI writes.
const GAD_MAX: usize = 0x7df0;

/// A logical colour table: RGB mode, or (index, RGB) entries.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Lct {
    pub rgb: bool,
    pub entries: Vec<(u32, u32)>,
}

/// A logical font of the metafile (Map Coded Font): GpiCreateLogFont's
/// local id, name and FATTRS.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub lcid: u8,
    pub name: [u8; 8],
    pub fattrs: [u8; 56],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Metafile {
    /// PU_* page units and page size.
    pub units: u32,
    pub page: (i32, i32),
    /// Coordinates in the orders are 4 bytes (else 2).
    pub long: bool,
    /// Default viewing transform (m11 m12 m21 m22 FIXED, m41 m42).
    pub default_view: Option<[i32; 6]>,
    pub lct: Option<Lct>,
    pub fonts: Vec<Font>,
    /// The description GpiPlayMetaFile returns.
    pub desc: Vec<u8>,
    /// GOCA orders: the segments, each opened by a Begin Segment order.
    pub data: Vec<u8>,
}

impl Default for Metafile {
    fn default() -> Self {
        Metafile {
            units: xform::PU_PELS,
            page: (0, 0),
            long: true,
            default_view: None,
            lct: None,
            fonts: Vec::new(),
            desc: Vec::new(),
            data: Vec::new(),
        }
    }
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(o)?, *b.get(o + 1)?]))
}

fn le32(b: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// The structured fields of a metafile: (id, data).
pub fn fields(b: &[u8]) -> Option<Vec<([u8; 3], &[u8])>> {
    let mut v = Vec::new();
    let mut p = 0;
    while p + 8 <= b.len() {
        let n = be16(b, p)? as usize;
        if n < 8 || p + n > b.len() || b[p + 2] != 0xd3 {
            return None;
        }
        v.push(([b[p + 2], b[p + 3], b[p + 4]], &b[p + 8..p + n]));
        p += n;
    }
    Some(v)
}

/// Page units from a picture descriptor's flags, unit base and units per
/// unit base (IBM's MTMakePageUnits).
fn page_units(flags: u8, base: u8, res: i32) -> u32 {
    match (flags, base, res) {
        (0x40, 0, 14400) => xform::PU_TWIPS,
        (0x40, 0, 10000) => xform::PU_HIENGLISH,
        (0x40, 0, 1000) => xform::PU_LOENGLISH,
        (0x40, 1, 10000) => xform::PU_HIMETRIC,
        (0x40, 1, 1000) => xform::PU_LOMETRIC,
        (0x60, 0, 1) => xform::PU_PELS,
        _ => xform::PU_ARBITRARY,
    }
}

fn units_descriptor(units: u32) -> (u8, u8, i32) {
    match units {
        xform::PU_TWIPS => (0x40, 0, 14400),
        xform::PU_HIENGLISH => (0x40, 0, 10000),
        xform::PU_LOENGLISH => (0x40, 0, 1000),
        xform::PU_HIMETRIC => (0x40, 1, 10000),
        xform::PU_LOMETRIC => (0x40, 1, 1000),
        xform::PU_PELS => (0x60, 0, 1),
        _ => (0, 0, 1),
    }
}

fn parse_gdd(d: &[u8], m: &mut Metafile) {
    let mut p = 0;
    let mut long = false;
    let mut view = None;
    while p + 2 <= d.len() {
        let (id, n) = (d[p], d[p + 1] as usize);
        let t = &d[p..(p + 2 + n).min(d.len())];
        match id {
            0xf6 if t.len() >= 34 => {
                let (flags, base) = (t[2], t[3] | t[5]);
                if t[4] == 4 {
                    let res = be16(t, 6).unwrap_or(1) as i32;
                    m.units = page_units(flags, base, res);
                    m.page = (be16(t, 14).unwrap_or(0) as i16 as i32, be16(t, 18).unwrap_or(0) as i16 as i32);
                } else {
                    m.units = page_units(flags, base, le32(t, 6).unwrap_or(1));
                    m.page = (le32(t, 22).unwrap_or(0), le32(t, 30).unwrap_or(0));
                }
            }
            0x21 if t.len() >= 3 => match t[2] {
                0x08 if t.len() >= 8 => long = t[7] == 5,
                0x07 if t.len() >= 6 + 24 => {
                    let mut v = [0; 6];
                    for (i, e) in v.iter_mut().enumerate() {
                        *e = le32(t, 6 + 4 * i).unwrap_or(0);
                    }
                    view = Some(v);
                }
                _ => {}
            },
            _ => {}
        }
        p += 2 + n;
    }
    m.long = long;
    m.default_view = if long { view } else { None };
}

fn parse_cat(d: &[u8]) -> Option<Lct> {
    // Header (3 bytes), then entries: len, type (1 table, 2 RGB mode).
    let mut p = 3;
    let mut lct = Lct::default();
    while p + 2 <= d.len() {
        let n = d[p] as usize;
        if n < 2 || p + n > d.len() {
            break;
        }
        let e = &d[p..p + n];
        match e[1] {
            2 => lct.rgb = true,
            1 if n >= 15 => {
                let idx = (e[4] as u32) << 16 | (e[5] as u32) << 8 | e[6] as u32;
                lct.entries.push((idx, be32(e, 11).unwrap_or(0) & 0xff_ffff));
            }
            _ => {}
        }
        p += n;
    }
    (lct.rgb || !lct.entries.is_empty()).then_some(lct)
}

fn parse_mcf(d: &[u8]) -> Option<Font> {
    // One repeating group: length (2), triplets.
    let n = (be16(d, 0)? as usize).min(d.len());
    let mut f = Font { lcid: 0, name: [0; 8], fattrs: [0; 56] };
    f.fattrs[0] = 56;
    let mut have = false;
    let mut p = 2;
    while p + 2 <= n {
        let len = d[p] as usize;
        if len < 2 || p + len > n {
            break;
        }
        let t = &d[p..p + len];
        match t[1] {
            0x02 if len >= 12 && t[2] == 0x84 => f.name.copy_from_slice(&t[4..12]),
            0x02 if len >= 5 && t[2] == 0x08 => {
                let k = (len - 4).min(32);
                f.fattrs[8..8 + k].copy_from_slice(&t[4..4 + k]);
            }
            0x24 if len >= 4 => f.lcid = t[3],
            0x20 if len >= 6 => f.fattrs[0x2a..0x2c].copy_from_slice(&be16(t, 4)?.to_le_bytes()),
            0x1f if len >= 20 => {
                have = true;
                let mut sel = 0u16;
                for (bit, s) in [(0x80, 1), (0x40, 2), (0x10, 8), (0x08, 0x10)] {
                    if t[8] & bit != 0 {
                        sel |= s;
                    }
                }
                f.fattrs[2..4].copy_from_slice(&sel.to_le_bytes());
                f.fattrs[0x2c..0x30].copy_from_slice(&(be16(t, 4)? as i32).to_le_bytes());
                f.fattrs[0x30..0x34].copy_from_slice(&(be16(t, 6)? as i32).to_le_bytes());
                if t[8] & 2 != 0 {
                    f.fattrs[0x34] |= 4;
                }
                f.fattrs[0x36] = t[19];
            }
            _ => {}
        }
        p += len;
    }
    (have && f.lcid != 0).then_some(f)
}

/// Read a metafile. None if it does not start with Begin Document.
pub fn parse(b: &[u8]) -> Option<Metafile> {
    let fs = fields(b)?;
    if fs.first()?.0 != BDT {
        return None;
    }
    let mut m = Metafile { long: false, ..Metafile::default() };
    for (id, d) in fs {
        match id {
            BDT => {
                // Name (8), 2 bytes, triplets; 0x65 is the description.
                let mut p = 10;
                while p + 2 <= d.len() {
                    let n = d[p] as usize;
                    if n < 2 || p + n > d.len() {
                        break;
                    }
                    if d[p + 1] == 0x65 {
                        let t = &d[p + 2..p + n];
                        m.desc = t[..t.iter().position(|&c| c == 0).unwrap_or(t.len())].to_vec();
                    }
                    p += n;
                }
            }
            CAT => m.lct = parse_cat(d),
            MCF => m.fonts.extend(parse_mcf(d)),
            GDD => parse_gdd(d, &mut m),
            GAD => m.data.extend_from_slice(d),
            _ => {}
        }
    }
    Some(m)
}

fn field(out: &mut Vec<u8>, id: [u8; 3], data: &[u8]) {
    out.extend_from_slice(&((data.len() + 8) as u16).to_be_bytes());
    out.extend_from_slice(&id);
    out.extend_from_slice(&[0, 0, 0]);
    out.extend_from_slice(data);
}

fn name(n: u8) -> [u8; 8] {
    let mut v = *b"00000000";
    v[7] = b'0' + n;
    v
}

fn font_group(f: &Font) -> Vec<u8> {
    let mut g = Vec::new();
    g.extend_from_slice(&[0, 0, 0x0c, 0x02, 0x84, 0x00]);
    g.extend_from_slice(&f.name);
    g.extend_from_slice(&[0x04, 0x24, 0x05, f.lcid]);
    let fa = &f.fattrs;
    let mut t = [0u8; 20];
    t[0] = 0x14;
    t[1] = 0x1f;
    t[2] = 5;
    t[3] = 5;
    let base = i32::from_le_bytes(fa[0x2c..0x30].try_into().unwrap()).clamp(0, 0xffff) as u16;
    let ave = i32::from_le_bytes(fa[0x30..0x34].try_into().unwrap()).clamp(0, 0xffff) as u16;
    t[4..6].copy_from_slice(&base.to_be_bytes());
    t[6..8].copy_from_slice(&ave.to_be_bytes());
    let sel = u16::from_le_bytes([fa[2], fa[3]]);
    for (bit, s) in [(0x80u8, 1u16), (0x40, 2), (0x10, 8), (0x08, 0x10)] {
        if sel & s != 0 {
            t[8] |= bit;
        }
    }
    if fa[0x34] & 4 != 0 {
        t[8] |= 2;
    }
    t[19] = fa[0x36];
    g.extend_from_slice(&t);
    let cp = u16::from_le_bytes([fa[0x2a], fa[0x2b]]);
    g.extend_from_slice(&[0x06, 0x20, 0x03, 0xd4]);
    g.extend_from_slice(&cp.to_be_bytes());
    g.extend_from_slice(&[0x24, 0x02, 0x08, 0x00]);
    g.extend_from_slice(&fa[8..40]);
    let n = g.len() as u16;
    g[0..2].copy_from_slice(&n.to_be_bytes());
    g
}

/// Write a metafile.
pub fn build(m: &Metafile) -> Vec<u8> {
    let mut out = Vec::new();
    // Begin Document: name, MO:DCA interchange set, code page 850,
    // description.
    let mut d = Vec::new();
    d.extend_from_slice(&name(1));
    d.extend_from_slice(&[0, 0, 0x05, 0x18, 0x03, 0x0c, 0x00, 0x06, 0x01, 0x03, 0xd4, 0x03, 0x52]);
    let desc = &m.desc[..m.desc.len().min(250)];
    d.push(desc.len() as u8 + 3);
    d.push(0x65);
    d.extend_from_slice(desc);
    d.push(0);
    field(&mut out, BDT, &d);
    field(&mut out, BRG, &name(2));
    field(&mut out, BCA, &name(4));
    let mut cat = Vec::from([0x40u8, 0, 0]);
    if let Some(l) = &m.lct {
        if l.rgb {
            cat.extend_from_slice(&[0x0a, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0x08, 0x08]);
        }
        for &(i, rgb) in &l.entries {
            cat.extend_from_slice(&[0x0f, 0x01, 0x00, 0x01, (i >> 16) as u8, (i >> 8) as u8, i as u8, 0x08, 0x08, 0x08, 0x04]);
            cat.extend_from_slice(&(rgb & 0xff_ffff).to_be_bytes());
        }
    }
    field(&mut out, CAT, &cat);
    field(&mut out, ECA, &name(4));
    field(&mut out, BGR, &name(7));
    field(&mut out, BOG, &name(7));
    let mut mca = Vec::from([0x00u8, 0x0e, 0x0c, 0x02, 0x84, 0x00]);
    mca.extend_from_slice(&name(4));
    field(&mut out, MCA, &mca);
    // The default font, then the logical fonts.
    field(&mut out, MCF, &[0x00, 0x18, 0x0c, 0x02, 0x84, 0x00, 0xff, 0, 0, 0, 0, 0, 0, 0, 0x04, 0x24, 0x05, 0x00, 0x06, 0x20, 0x03, 0xd4, 0x03, 0x52]);
    for f in &m.fonts {
        field(&mut out, MCF, &font_group(f));
    }
    field(&mut out, EOG, &name(7));
    // Graphics Data Descriptor.
    let mut g = Vec::from([0xf7u8, 0x07, 0xb0, 0x00, 0x00, 0x23, 0x01, 0x01, 0x05]);
    let (flags, base, res) = units_descriptor(m.units);
    g.extend_from_slice(&[0xf6, 0x28, flags, 0x00, 0x05, base]);
    for v in [res, res, 0, 0, m.page.0, 0, m.page.1, 0, 0] {
        g.extend_from_slice(&v.to_le_bytes());
    }
    g.extend_from_slice(&[0x21, 0x07, 0x08, 0xe0, 0x00, 0x8f, 0x00, if m.long { 5 } else { 4 }, 0x05]);
    g.extend_from_slice(&[0x21, 0x1c, 0x07, 0xcc, 0x0c, 0x8f]);
    for v in m.default_view.unwrap_or(crate::goca::IDENTITY) {
        g.extend_from_slice(&v.to_le_bytes());
    }
    field(&mut out, GDD, &g);
    // Graphics Data, split at order boundaries.
    let mut rest: &[u8] = &m.data;
    while !rest.is_empty() {
        let n = split_point(rest, GAD_MAX);
        field(&mut out, GAD, &rest[..n]);
        rest = &rest[n..];
    }
    field(&mut out, EGR, &name(7));
    field(&mut out, ERG, &name(2));
    field(&mut out, EDT, &name(1));
    out
}

/// How much of an order stream fits in `max` bytes without splitting an
/// order (at least one order).
fn split_point(d: &[u8], max: usize) -> usize {
    let mut p = 0;
    while p < d.len() {
        let code = d[p];
        let len = match code {
            0x00 | 0xff => 1,
            0xfe if p + 4 <= d.len() => 4 + ((d[p + 2] as usize) << 8 | d[p + 3] as usize),
            c if crate::goca::short_order(c) => 2,
            _ if p + 2 <= d.len() => 2 + d[p + 1] as usize,
            _ => d.len() - p,
        };
        if p + len > max && p > 0 {
            break;
        }
        p += len;
    }
    p.min(d.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goca::{self, Attr, Order};
    use crate::Point;
    use alloc::vec;

    #[test]
    fn build_parse_round_trip() {
        let mut fa = [0u8; 56];
        fa[0] = 56;
        fa[2] = 1;
        fa[8..15].copy_from_slice(b"Courier");
        fa[0x2a..0x2c].copy_from_slice(&850u16.to_le_bytes());
        fa[0x2c] = 12;
        fa[0x30] = 7;
        fa[0x36] = 0x0c;
        let data = goca::encode_all(&[
            Order::BeginSegment { id: 0, flags: 0x7000, len: 0 },
            Order::Set(Attr::Color(2)),
            Order::Line { at: Some(Point::new(0, 0)), pts: vec![Point::new(100, 100)] },
        ]);
        let m = Metafile {
            units: xform::PU_LOENGLISH,
            page: (984, 736),
            long: true,
            default_view: Some([0x20000, 0, 0, 0x20000, 5, 6]),
            lct: Some(Lct { rgb: false, entries: vec![(1, 0x0000ff), (2, 0xff0000)] }),
            fonts: vec![Font { lcid: 3, name: *b"FONT0003", fattrs: fa }],
            desc: b"rs2 test".to_vec(),
            data,
        };
        let b = build(&m);
        assert_eq!(&b[2..5], &BDT);
        assert_eq!(parse(&b), Some(m));
    }

    #[test]
    fn units() {
        for u in [xform::PU_TWIPS, xform::PU_HIENGLISH, xform::PU_LOENGLISH, xform::PU_HIMETRIC, xform::PU_LOMETRIC, xform::PU_PELS, xform::PU_ARBITRARY] {
            let m = Metafile { units: u, page: (10, 20), ..Metafile::default() };
            let p = parse(&build(&m)).unwrap();
            assert_eq!((p.units, p.page), (u, (10, 20)));
        }
    }

    #[test]
    fn graphics_data_split() {
        let pts: Vec<Point> = (0..20000).map(|i| Point::new(i, -i)).collect();
        let data = goca::encode_all(&[Order::Line { at: None, pts }]);
        let m = Metafile { data: data.clone(), ..Metafile::default() };
        let b = build(&m);
        let gads: Vec<_> = fields(&b).unwrap().into_iter().filter(|(id, _)| *id == GAD).collect();
        assert!(gads.len() > 1);
        assert!(gads.iter().all(|(_, d)| d.len() <= GAD_MAX && d[0] == 0x81));
        assert_eq!(parse(&b).unwrap().data, data);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(b"not a metafile at all"), None);
        assert_eq!(parse(&[]), None);
    }

    /// OS/2's own metafiles, when the CD has been unpacked (work/).
    #[test]
    fn ibm_metafiles() {
        extern crate std;
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/");
        for (f, units, page, segs) in [
            ("media/boot/toolkit/os2tk45/samples/pm/graphics/graphic.met", xform::PU_LOENGLISH, (984, 736), 9),
            ("unpacked/disk_12/OS2/BITMAP/AAAAA.MET", xform::PU_HIENGLISH, (12725, 9722), 1),
        ] {
            let Ok(b) = std::fs::read(std::format!("{root}{f}")) else { continue };
            let m = parse(&b).unwrap();
            assert_eq!((m.units, m.page, m.long), (units, page, true), "{f}");
            let orders = goca::decode(&m.data, true);
            let n = orders.iter().filter(|o| matches!(o, Order::BeginSegment { .. })).count();
            assert_eq!(n, segs, "{f}");
            assert!(!orders.iter().any(|o| matches!(o, Order::Unknown(_))), "{f}: {:?}", orders.iter().find(|o| matches!(o, Order::Unknown(_))));
            // Rebuilding keeps everything.
            assert_eq!(parse(&build(&m)).unwrap(), m);
        }
    }
}
