//! GPI text and fonts: character strings, text boxes and positions, the
//! character attributes, logical fonts and font metrics.
//!
//! The public fonts are the system font (8x16 pels) under several faces.
//! A logical font created with FATTR_FONTUSE_OUTLINE behaves like an
//! outline font: it is scaled to the character box, and character
//! strings in paths become figures. GpiLoadFonts loads the image fonts
//! of a font file (.FON: RT_FONT resources) for the process; those draw
//! with their own glyphs. A logical font's italic, bold and underscore
//! (fsSelection) are simulated: slanted, a pel wider, underlined;
//! GpiSetCharShear slants outline fonts.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::{LogFont, Point};

use crate::gpiapi::{draw, entry, point, read, write, Lcid, PointL, RectL, GPI_ERROR, GPI_OK};
use crate::state::st;

fn text<'a>(p: *const u8, n: i32) -> Option<&'a [u8]> {
    if n < 0 || (n > 0 && p.is_null()) {
        return None;
    }
    Some(if n == 0 { &[] } else { unsafe { core::slice::from_raw_parts(p, n as usize) } })
}

fn incs(p: *const i32, n: i32) -> Option<Vec<i32>> {
    (!p.is_null()).then(|| (0..n.max(0) as usize).map(|i| unsafe { p.add(i).read_unaligned() }).collect())
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiCharStringAt(hps: u32, p: *const PointL, n: i32, s: *const u8) -> i32 {
    let (Some(p), Some(t)) = (point(p), text(s, n)) else { return GPI_ERROR };
    draw(hps, |e, surf| e.ps.gpi_text(surf, p, t, None, None, false, false)).map_or(GPI_ERROR, |_| GPI_OK)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiCharString(hps: u32, n: i32, s: *const u8) -> i32 {
    let Some(pos) = entry(st(), hps).map(|e| e.ps.pos) else { return GPI_ERROR };
    let p = PointL { x: pos.x, y: pos.y };
    GpiCharStringAt(hps, &p, n, s)
}

const CHS_OPAQUE: u32 = 1;
const CHS_VECTOR: u32 = 2;
const CHS_LEAVEPOS: u32 = 8;
const CHS_CLIP: u32 = 0x10;

fn string_pos(hps: u32, at: Option<Point>, prcl: *const RectL, opts: u32, n: i32, s: *const u8, adx: *const i32) -> i32 {
    let Some(t) = text(s, n) else { return GPI_ERROR };
    let dx = if opts & CHS_VECTOR != 0 { incs(adx, n) } else { None };
    let rect = read(prcl).map(|r| r.rect());
    draw(hps, |e, surf| {
        let start = at.unwrap_or(e.ps.pos);
        let before = e.ps.pos;
        e.ps.gpi_text(surf, start, t, dx.as_deref(), rect, opts & CHS_OPAQUE != 0 && rect.is_some(), opts & CHS_CLIP != 0);
        if opts & CHS_LEAVEPOS != 0 {
            e.ps.pos = before;
        }
    })
    .map_or(GPI_ERROR, |_| GPI_OK)
}

/// GpiCharStringPos(hps, prclRect, flOptions, lCount, pchString, alAdx)
#[unsafe(no_mangle)]
pub extern "C" fn GpiCharStringPos(hps: u32, prcl: *const RectL, opts: u32, n: i32, s: *const u8, adx: *const i32) -> i32 {
    string_pos(hps, None, prcl, opts, n, s, adx)
}

/// GpiCharStringPosAt(hps, pptlStart, prclRect, flOptions, lCount,
/// pchString, alAdx)
#[unsafe(no_mangle)]
pub extern "C" fn GpiCharStringPosAt(hps: u32, p: *const PointL, prcl: *const RectL, opts: u32, n: i32, s: *const u8, adx: *const i32) -> i32 {
    let Some(p) = point(p) else { return GPI_ERROR };
    string_pos(hps, Some(p), prcl, opts, n, s, adx)
}

fn query_pos(hps: u32, at: Option<Point>, opts: u32, n: i32, s: *const u8, adx: *const i32, out: *mut PointL) -> u32 {
    let (Some(e), Some(t)) = (entry(st(), hps), text(s, n)) else { return 0 };
    if out.is_null() {
        return 0;
    }
    let dx = if opts & CHS_VECTOR != 0 { incs(adx, n) } else { None };
    let v = e.ps.char_positions(at.unwrap_or(e.ps.pos), t, dx.as_deref());
    for (i, p) in v.iter().enumerate() {
        write(unsafe { out.add(i) }, PointL { x: p.x, y: p.y });
    }
    1
}

/// GpiQueryCharStringPos(hps, flOptions, lCount, pchString, alXincrements,
/// aptlPositions): lCount + 1 positions.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharStringPos(hps: u32, opts: u32, n: i32, s: *const u8, adx: *const i32, out: *mut PointL) -> u32 {
    query_pos(hps, None, opts, n, s, adx, out)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharStringPosAt(hps: u32, p: *const PointL, opts: u32, n: i32, s: *const u8, adx: *const i32, out: *mut PointL) -> u32 {
    let Some(p) = point(p) else { return 0 };
    query_pos(hps, Some(p), opts, n, s, adx, out)
}

/// GpiQueryTextBox(hps, lCount1, pchString, lCount2, aptlPoints):
/// TXTBOX_TOPLEFT, BOTTOMLEFT, TOPRIGHT, BOTTOMRIGHT, CONCAT, relative to
/// the start point.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryTextBox(hps: u32, n: i32, s: *const u8, n2: i32, out: *mut PointL) -> u32 {
    let (Some(e), Some(t)) = (entry(st(), hps), text(s, n)) else { return 0 };
    if out.is_null() || n2 < 0 {
        return 0;
    }
    let b = e.ps.text_box(t);
    for (i, p) in b.iter().take(n2 as usize).enumerate() {
        write(unsafe { out.add(i) }, PointL { x: p.x, y: p.y });
    }
    1
}

/// GpiQueryWidthTable(hps, lFirstChar, lCount, alData): advances in
/// world units.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryWidthTable(hps: u32, first: i32, n: i32, out: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if out.is_null() || first < 0 || n < 0 {
        return 0;
    }
    let w = e.ps.text_box(b"x")[4].x;
    for i in 0..n as usize {
        write(unsafe { out.add(i) }, w);
    }
    1
}

// ---- Character attributes ----

fn set_attr(hps: u32, f: impl FnOnce(&mut gpi::Attrs)) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    e.ps.will_change_attrs();
    f(&mut e.ps.attrs);
    1
}

fn pair(p: *const i32) -> Option<(i32, i32)> {
    read(p as *const [i32; 2]).map(|v| (v[0], v[1]))
}

fn put_pair(hps: u32, p: *mut i32, f: impl FnOnce(&gpi::Attrs) -> (i32, i32)) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let v = f(&e.ps.attrs);
    write(p as *mut [i32; 2], [v.0, v.1]) as u32
}

/// GpiSetCharBox(hps, psizfxBox): FIXED width and height.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharBox(hps: u32, p: *const i32) -> u32 {
    let Some(v) = pair(p) else { return 0 };
    set_attr(hps, |a| a.char_box = v)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharBox(hps: u32, p: *mut i32) -> u32 {
    put_pair(hps, p, |a| a.char_box)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryDefCharBox(hps: u32, p: *mut i32) -> u32 {
    put_pair(hps, p, |_| gpi::Attrs::default().char_box)
}

/// GpiSetCharAngle(hps, pgradlAngle): accepted; text is drawn
/// horizontally.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharAngle(hps: u32, p: *const i32) -> u32 {
    let Some(v) = pair(p) else { return 0 };
    set_attr(hps, |a| a.char_angle = v)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharAngle(hps: u32, p: *mut i32) -> u32 {
    put_pair(hps, p, |a| a.char_angle)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharShear(hps: u32, p: *const i32) -> u32 {
    let Some(v) = pair(p) else { return 0 };
    set_attr(hps, |a| a.char_shear = v)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharShear(hps: u32, p: *mut i32) -> u32 {
    put_pair(hps, p, |a| a.char_shear)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharDirection(hps: u32, v: i32) -> u32 {
    set_attr(hps, |a| a.char_direction = v)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharDirection(hps: u32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.ps.attrs.char_direction)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharMode(hps: u32, v: i32) -> u32 {
    set_attr(hps, |a| a.char_mode = v)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharMode(hps: u32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.ps.attrs.char_mode)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharExtra(hps: u32, v: i32) -> u32 {
    set_attr(hps, |a| a.char_extra = v)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharExtra(hps: u32, p: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    write(p, e.ps.attrs.char_extra) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharBreakExtra(hps: u32, v: i32) -> u32 {
    set_attr(hps, |a| a.char_break_extra = v)
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharBreakExtra(hps: u32, p: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    write(p, e.ps.attrs.char_break_extra) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetTextAlignment(hps: u32, h: i32, v: i32) -> u32 {
    set_attr(hps, |a| a.text_align = (h, v))
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryTextAlignment(hps: u32, h: *mut i32, v: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let a = e.ps.attrs.text_align;
    (write(h, a.0) && write(v, a.1)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCp(hps: u32, _cp: u32) -> u32 {
    entry(st(), hps).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCp(hps: u32) -> u32 {
    entry(st(), hps).map_or(0, |_| 850)
}

// ---- Fonts ----

/// The public fonts: the system font (an image font), the standard
/// outline faces, which are drawn by scaling the system font's glyphs to
/// the character box, and OS/2's other image faces (Helv, Tms Rmn and
/// the system fonts, which programs ask for by name), drawn with the
/// system font's glyphs.
const FONTS: &[(&[u8], bool)] = &[
    (b"System VIO", false),
    (b"Helvetica", true),
    (b"Helvetica Bold", true),
    (b"Times New Roman", true),
    (b"Times New Roman Bold", true),
    (b"Courier", true),
    (b"Courier Bold", true),
    (b"Helv", false),
    (b"Tms Rmn", false),
    (b"System Proportional", false),
    (b"System Monospaced", false),
];
const FATTR_FONTUSE_OUTLINE: u16 = 4;
const FATTR_SEL_OUTLINE: u16 = 8;

fn trim(face: &[u8]) -> &[u8] {
    &face[..face.iter().position(|&b| b == 0).unwrap_or(face.len())]
}

/// Index of a public font by face name or lMatch (index + 1).
fn find_font(face: &[u8], lmatch: i32) -> Option<usize> {
    FONTS.iter().position(|(n, _)| n.eq_ignore_ascii_case(trim(face))).or_else(|| (1..=FONTS.len() as i32).contains(&lmatch).then(|| lmatch as usize - 1))
}

/// GpiCreateLogFont(hps, pName, lLcid, pfatAttrs): FONT_MATCH 2,
/// FONT_DEFAULT 1.
#[unsafe(no_mangle)]
pub extern "C" fn GpiCreateLogFont(hps: u32, _name: *const u8, lcid: i32, fat: *const u8) -> i32 {
    if fat.is_null() || !(1..=254).contains(&lcid) {
        return GPI_ERROR;
    }
    let Some(e) = entry(st(), hps) else { return GPI_ERROR };
    let sel = unsafe { (fat.add(2) as *const u16).read_unaligned() };
    let usef = unsafe { (fat.add(54) as *const u16).read_unaligned() };
    let mut face = [0u8; 32];
    unsafe { core::ptr::copy_nonoverlapping(fat.add(8), face.as_mut_ptr(), 32) };
    let lmatch = unsafe { (fat.add(4) as *const i32).read_unaligned() };
    // A font loaded with GpiLoadFonts: by lMatch, else the face at the
    // nearest size (lMaxBaselineExt, lAveCharWidth).
    let maxbe = unsafe { (fat.add(44) as *const i32).read_unaligned() };
    let avew = unsafe { (fat.add(48) as *const i32).read_unaligned() };
    if let Some(i) = find_loaded(&face, lmatch, maxbe, avew) {
        e.gpi.lcids.retain(|(l, _)| *l != lcid);
        e.gpi.lcids.push((lcid, Lcid::Font(LogFont { outline: false, italic: sel & 1 != 0, underscore: sel & 2 != 0, bold: sel & 0x20 != 0 }, face, i as u32 + 1)));
        return 2;
    }
    let found = find_font(&face, lmatch);
    let outline = usef & FATTR_FONTUSE_OUTLINE != 0 || sel & FATTR_SEL_OUTLINE != 0 || found.is_some_and(|i| FONTS[i].1);
    // fsSelection: FATTR_SEL_ITALIC 1, FATTR_SEL_UNDERSCORE 2,
    // FATTR_SEL_BOLD 0x20, simulated when the text is drawn.
    let lf = LogFont { outline, italic: sel & 1 != 0, underscore: sel & 2 != 0, bold: sel & 0x20 != 0 };
    if let Some(i) = found {
        face = [0; 32];
        face[..FONTS[i].0.len()].copy_from_slice(FONTS[i].0);
    }
    e.gpi.lcids.retain(|(l, _)| *l != lcid);
    e.gpi.lcids.push((lcid, Lcid::Font(lf, face, 0)));
    if found.is_some() || outline { 2 } else { 1 }
}

/// GpiSetCharSet(hps, llcid): 0 = the default font.
#[unsafe(no_mangle)]
pub extern "C" fn GpiSetCharSet(hps: u32, lcid: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let (lf, loaded) = if lcid == 0 {
        (LogFont::default(), 0)
    } else {
        match e.gpi.lcids.iter().find(|(l, _)| *l == lcid) {
            Some((_, Lcid::Font(lf, _, n))) => (*lf, *n),
            _ => return 0,
        }
    };
    e.ps.will_change_attrs();
    e.ps.attrs.char_set = lcid;
    e.ps.logfont = lf;
    e.ps.font = loaded_font(loaded).unwrap_or_else(gpi::font::system);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryCharSet(hps: u32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.ps.attrs.char_set)
}

/// GpiDeleteSetId(hps, lLcid): a logical font or bitmap tag; LCID_ALL
/// (-1) deletes all.
#[unsafe(no_mangle)]
pub extern "C" fn GpiDeleteSetId(hps: u32, lcid: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let before = e.gpi.lcids.len();
    e.gpi.lcids.retain(|(l, _)| lcid != -1 && *l != lcid);
    if e.ps.attrs.char_set == lcid || lcid == -1 {
        e.ps.attrs.char_set = 0;
        e.ps.logfont = LogFont::default();
        e.ps.font = gpi::font::system();
    }
    (e.gpi.lcids.len() != before || lcid == -1) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryNumberSetIds(hps: u32) -> i32 {
    entry(st(), hps).map_or(-1, |e| e.gpi.lcids.len() as i32)
}

/// GpiQuerySetIds(hps, lCount, alTypes, aNames, allcids): LCIDT_FONT 6,
/// LCIDT_BITMAP 7.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQuerySetIds(hps: u32, n: i32, types: *mut i32, names: *mut [u8; 8], lcids: *mut i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    for (i, (l, x)) in e.gpi.lcids.iter().take(n.max(0) as usize).enumerate() {
        unsafe {
            write(types.add(i), if matches!(x, Lcid::Font(..)) { 6 } else { 7 });
            write(names.add(i), [0; 8]);
            write(lcids.add(i), *l);
        }
    }
    1
}

/// GpiQueryLogicalFont(hps, lLcid, pName, pfatAttrs, lLength)
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryLogicalFont(hps: u32, lcid: i32, name: *mut [u8; 8], fat: *mut u8, len: i32) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    let Some((_, Lcid::Font(lf, face, _))) = e.gpi.lcids.iter().find(|(l, _)| *l == lcid) else { return 0 };
    write(name, [0; 8]);
    let mut v = [0u8; 56];
    v[0..2].copy_from_slice(&56u16.to_le_bytes());
    let sel = lf.italic as u16 | (lf.underscore as u16) << 1 | (lf.bold as u16) << 5;
    v[2..4].copy_from_slice(&sel.to_le_bytes());
    v[8..40].copy_from_slice(face);
    v[42..44].copy_from_slice(&850u16.to_le_bytes());
    if lf.outline {
        v[54..56].copy_from_slice(&FATTR_FONTUSE_OUTLINE.to_le_bytes());
    }
    if !fat.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), fat, (len.max(0) as usize).min(56)) };
    }
    1
}

/// FONTMETRICS of the system font scaled by `scale` (device pels per
/// font pel), converted to world units by `w`.
fn metrics(font: usize, scale: (f64, f64), w: &dyn Fn(f64, f64) -> (i32, i32)) -> [u8; 228] {
    let f = gpi::font::system();
    let (face, outline) = FONTS[font];
    let mut m = [0u8; 228];
    let n = face.len();
    m[32..32 + n].copy_from_slice(face);
    let family = face.strip_suffix(b" Bold").unwrap_or(face);
    m[0..family.len()].copy_from_slice(family);
    let p16 = |m: &mut [u8; 228], o: usize, v: i32| m[o..o + 2].copy_from_slice(&(v as i16).to_le_bytes());
    let p32 = |m: &mut [u8; 228], o: usize, v: i32| m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let x = |v: i32| w(v as f64 * scale.0, 0.0).0;
    let y = |v: i32| w(0.0, v as f64 * scale.1).1;
    let (h, d) = (f.height, f.descender);
    p16(&mut m, 66, 850); // usCodePage
    p32(&mut m, 68, y(h)); // lEmHeight
    p32(&mut m, 72, y(h / 2)); // lXHeight
    p32(&mut m, 76, y(h - d)); // lMaxAscender
    p32(&mut m, 80, y(d)); // lMaxDescender
    p32(&mut m, 84, y(h - d - 2)); // lLowerCaseAscent
    p32(&mut m, 88, y(d)); // lLowerCaseDescent
    p32(&mut m, 100, x(f.width)); // lAveCharWidth
    p32(&mut m, 104, x(f.width)); // lMaxCharInc
    p32(&mut m, 108, x(f.width)); // lEmInc
    p32(&mut m, 112, y(h)); // lMaxBaselineExt
    p16(&mut m, 122, if face.ends_with(b"Bold") { 7 } else { 5 }); // usWeightClass
    p16(&mut m, 124, 5); // usWidthClass
    p16(&mut m, 126, 96); // sXDeviceRes
    p16(&mut m, 128, 96);
    p16(&mut m, 130, 1); // sFirstChar
    p16(&mut m, 132, 254); // sLastChar (relative)
    p16(&mut m, 134, b'?' as i32 - 1); // sDefaultChar
    p16(&mut m, 136, b' ' as i32 - 1); // sBreakChar
    p16(&mut m, 138, 120); // sNominalPointSize (decipoints)
    p16(&mut m, 140, if outline { 10 } else { 120 });
    p16(&mut m, 142, if outline { 7200 } else { 120 });
    p16(&mut m, 144, 1); // fsType: FM_TYPE_FIXED
    p16(&mut m, 146, outline as i32); // fsDefn: FM_DEFN_OUTLINE
    p32(&mut m, 152, x(f.width / 2)); // subscript x size
    p32(&mut m, 156, y(h / 2));
    p32(&mut m, 164, y(d / 2));
    p32(&mut m, 168, x(f.width / 2)); // superscript
    p32(&mut m, 172, y(h / 2));
    p32(&mut m, 180, y(h / 2));
    p32(&mut m, 184, y(1).max(1)); // lUnderscoreSize
    p32(&mut m, 188, y(2)); // lUnderscorePosition
    p32(&mut m, 192, y(1).max(1)); // lStrikeoutSize
    p32(&mut m, 196, y(h / 3)); // lStrikeoutPosition
    p32(&mut m, 204, font as i32 + 1); // lMatch
    m
}

/// GpiQueryFontMetrics(hps, lMetricsLength, pfmMetrics)
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryFontMetrics(hps: u32, len: i32, out: *mut u8) -> u32 {
    let Some(e) = entry(st(), hps) else { return 0 };
    if out.is_null() || len < 0 {
        return 0;
    }
    let ps = &e.ps;
    let set = ps.attrs.char_set;
    let loaded = e.gpi.lcids.iter().find_map(|(l, x)| match x {
        Lcid::Font(_, _, n) if *l == set && *n != 0 => Some(*n as usize - 1),
        _ => None,
    });
    if let Some(m) = loaded.and_then(|i| loaded_metrics(i, &|x, y| ps.device_to_world_len(x, y))) {
        unsafe { core::ptr::copy_nonoverlapping(m.as_ptr(), out, (len as usize).min(m.len())) };
        return 1;
    }
    let font = e.gpi.lcids.iter().find_map(|(l, x)| match x {
        Lcid::Font(lf, face, _) if *l == set => Some(find_font(face, 0).unwrap_or(lf.outline as usize)),
        _ => None,
    });
    let m = metrics(font.unwrap_or(0), ps.font_scale(), &|x, y| ps.device_to_world_len(x, y));
    unsafe { core::ptr::copy_nonoverlapping(m.as_ptr(), out, (len as usize).min(m.len())) };
    1
}

/// GpiQueryFonts(hps, flOptions, pszFacename, plReqFonts, lMetricsLength,
/// afmMetrics): the public fonts (QF_PUBLIC 1), then the process's
/// loaded ones (QF_PRIVATE 2); returns how many matching fonts were not
/// returned.
#[unsafe(no_mangle)]
pub extern "C" fn GpiQueryFonts(hps: u32, opts: u32, face: *const u8, req: *mut i32, len: i32, out: *mut u8) -> i32 {
    if entry(st(), hps).is_none() || req.is_null() {
        return -1;
    }
    let name: Vec<u8> = if face.is_null() { Vec::new() } else { (0..).map(|i| unsafe { *face.add(i) }).take_while(|&b| b != 0).collect() };
    let round = |x: f64, y: f64| (gpi::math::round(x), gpi::math::round(y));
    let mut list: Vec<[u8; 228]> = Vec::new();
    if opts & QF_PUBLIC != 0 || opts & (QF_PUBLIC | QF_PRIVATE) == 0 {
        list.extend((0..FONTS.len()).filter(|&i| name.is_empty() || FONTS[i].0.eq_ignore_ascii_case(&name)).map(|i| metrics(i, (1.0, 1.0), &round)));
    }
    if opts & QF_PRIVATE != 0 {
        for (i, l) in loaded().iter().enumerate() {
            if l.as_ref().is_some_and(|l| name.is_empty() || l.info.face.eq_ignore_ascii_case(&name)) {
                list.extend(loaded_metrics(i, &round));
            }
        }
    }
    let total = list.len() as i32;
    let n = unsafe { req.read_unaligned() }.clamp(0, total);
    if !out.is_null() && len > 0 {
        for (k, m) in list.iter().take(n as usize).enumerate() {
            unsafe { core::ptr::copy_nonoverlapping(m.as_ptr(), out.add(k * len as usize), (len as usize).min(m.len())) };
        }
    }
    unsafe { req.write_unaligned(n) };
    total - n
}

const QF_PUBLIC: u32 = 1;
const QF_PRIVATE: u32 = 2;
const RT_FONTDIR: u32 = 6;
const RT_FONT: u32 = 7;

/// A font loaded with GpiLoadFonts: the file it came from (upper case),
/// what it says about itself, its glyphs.
struct Loaded {
    file: Vec<u8>,
    info: gpi::font::Os2FontInfo,
    font: &'static gpi::font::Font,
}

/// The process's loaded fonts (lMatch 0x1000 + index).
static mut LOADED: Vec<Option<Loaded>> = Vec::new();
const LOADED_MATCH: i32 = 0x1000;

fn loaded() -> &'static mut Vec<Option<Loaded>> {
    unsafe { &mut *core::ptr::addr_of_mut!(LOADED) }
}

fn loaded_font(n: u32) -> Option<&'static gpi::font::Font> {
    loaded().get((n as usize).checked_sub(1)?)?.as_ref().map(|l| l.font)
}

/// The loaded font for a FATTRS: by lMatch, else by face at the nearest
/// size.
fn find_loaded(face: &[u8], lmatch: i32, maxbe: i32, avew: i32) -> Option<usize> {
    let v = loaded();
    if let Some(i) = lmatch.checked_sub(LOADED_MATCH).filter(|&i| i >= 0).map(|i| i as usize) {
        if v.get(i).is_some_and(|l| l.is_some()) {
            return Some(i);
        }
    }
    let face = trim(face);
    v.iter()
        .enumerate()
        .filter_map(|(i, l)| l.as_ref().filter(|l| l.info.face.eq_ignore_ascii_case(face)).map(|l| (i, l)))
        .min_by_key(|(_, l)| {
            let f = &l.info.fields;
            (if maxbe > 0 { (f[13] as i32 - maxbe).abs() } else { 0 }) * 4 + if avew > 0 { (f[10] as i32 - avew).abs() } else { 0 }
        })
        .map(|(i, _)| i)
}

/// FONTMETRICS of a loaded font (from its FOCAMETRICS), converted to
/// world units by `w`.
fn loaded_metrics(i: usize, w: &dyn Fn(f64, f64) -> (i32, i32)) -> Option<[u8; 228]> {
    let l = loaded().get(i)?.as_ref()?;
    let f = &l.info.fields;
    let mut m = [0u8; 228];
    m[..l.info.family.len().min(31)].copy_from_slice(&l.info.family[..l.info.family.len().min(31)]);
    m[32..32 + l.info.face.len().min(31)].copy_from_slice(&l.info.face[..l.info.face.len().min(31)]);
    let p16 = |m: &mut [u8; 228], o: usize, v: i32| m[o..o + 2].copy_from_slice(&(v as i16).to_le_bytes());
    let p32 = |m: &mut [u8; 228], o: usize, v: i32| m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    p16(&mut m, 64, f[0] as i32);
    p16(&mut m, 66, f[1] as i32);
    // lEmHeight .. lMaxBaselineExt: x widths at 10..12, the rest heights.
    for k in 0..12 {
        let v = f[2 + k] as f64;
        let x = if (8..11).contains(&k) { w(v, 0.0).0 } else { w(0.0, v).1 };
        p32(&mut m, 68 + 4 * k, x);
    }
    for (k, o) in (14..32).zip((116..152).step_by(2)) {
        p16(&mut m, o, f[k] as i32);
    }
    for k in 0..12 {
        let v = f[32 + k] as f64;
        let x = if k < 8 && k % 2 == 0 { w(v, 0.0).0 } else { w(0.0, v).1 };
        p32(&mut m, 152 + 4 * k, x);
    }
    p16(&mut m, 200, f[44] as i32);
    p16(&mut m, 202, f[45] as i32);
    p32(&mut m, 204, LOADED_MATCH + i as i32);
    Some(m)
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.318"]
    fn DosLoadModule(fail: *mut u8, cb: u32, name: *const u8, phmod: *mut u32) -> u32;
    #[link_name = "DOSCALLS.322"]
    fn DosFreeModule(hmod: u32) -> u32;
}

/// GpiLoadFonts(hab, pszFilename): the image fonts of a font file (a
/// resource DLL: RT_FONTDIR lists the RT_FONT resources) for this
/// process. TRUE when the file has any.
#[unsafe(no_mangle)]
pub extern "C" fn GpiLoadFonts(_hab: u32, file: *const u8) -> u32 {
    if file.is_null() {
        return 0;
    }
    let name: Vec<u8> = (0..).map(|i| unsafe { *file.add(i) }).take_while(|&b| b != 0).collect();
    let key = name.to_ascii_uppercase();
    if loaded().iter().flatten().any(|l| l.file == key) {
        return 1;
    }
    let mut z = name.clone();
    z.push(0);
    let mut fail = [0u8; 16];
    let mut hmod = 0;
    let rc = crate::lock::unlocked(|| unsafe { DosLoadModule(fail.as_mut_ptr(), 16, z.as_ptr(), &mut hmod) });
    if rc != 0 {
        return 0;
    }
    let Some(dir) = crate::res::load(hmod, RT_FONTDIR, 1).map(|d| d.to_vec()) else {
        unsafe { DosFreeModule(hmod) };
        return 0;
    };
    // FONTDIRECTORY: cbHeader, cFonts, cbEntry, then entries that start
    // with the resource id.
    let u16_at = crate::res::u16_at;
    let (hdr, n, size) = (u16_at(&dir, 0) as usize, u16_at(&dir, 2) as usize, u16_at(&dir, 4) as usize);
    let mut any = false;
    for k in 0..n {
        let id = u16_at(&dir, hdr + k * size) as u32;
        let Some(data) = crate::res::load(hmod, RT_FONT, id) else { continue };
        if let Some((font, info)) = gpi::font::Font::from_os2(data) {
            let font: &'static gpi::font::Font = alloc::boxed::Box::leak(alloc::boxed::Box::new(font));
            loaded().push(Some(Loaded { file: key.clone(), info, font }));
            any = true;
        }
    }
    unsafe { DosFreeModule(hmod) };
    any as u32
}

/// GpiUnloadFonts(hab, pszFilename): the file's fonts are no longer
/// offered (logical fonts made from them keep drawing).
#[unsafe(no_mangle)]
pub extern "C" fn GpiUnloadFonts(_hab: u32, file: *const u8) -> u32 {
    if file.is_null() {
        return 0;
    }
    let key: Vec<u8> = (0..).map(|i| unsafe { *file.add(i) }).take_while(|&b| b != 0).map(|b: u8| b.to_ascii_uppercase()).collect();
    let mut any = false;
    for l in loaded().iter_mut() {
        if l.as_ref().is_some_and(|l| l.file == key) {
            *l = None;
            any = true;
        }
    }
    any as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiLoadPublicFonts(_hab: u32, _file: *const u8) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn GpiUnloadPublicFonts(_hab: u32, _file: *const u8) -> u32 {
    0
}
