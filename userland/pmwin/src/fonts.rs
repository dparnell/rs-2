//! The fonts PM draws with, kept in the shared state so every PM process
//! uses the same ones (their glyphs are rendered once, in the shared
//! heap). IBM's fonts come first where they are installed (APPINST's
//! "OS/2 fonts" copies them from the OS/2 CD); rs2 ships the Liberation
//! fonts for the rest:
//!
//! - the system font (controls, menus, title bars): IBM's WarpSans (9
//!   points, from OS2\DLL\DSPRES.DLL), else Liberation Sans at
//!   `SYSTEM_EM` pels, anti-aliased;
//! - the monospaced font (MLEs): Liberation Mono;
//! - the outline faces GPI offers (Helvetica, Times New Roman, Courier,
//!   their bold and italic): IBM's Type 1 fonts (PSFONTS\*.PFB), else
//!   the Liberation fonts (PSFONTS\LIB*.TTF), each read the first time
//!   it is asked for; the image faces (Helv, Tms Rmn, System
//!   Proportional, System Monospaced) are drawn from them too;
//! - IBM's image fonts (DSPRES.DLL, HELV.FON, TIMES.FON, COURIER.FON,
//!   COURIERI.FON, SYSMONO.FON) are offered to every process as if it
//!   had loaded them with GpiLoadFonts, so an image face at a size they
//!   have is IBM's bitmap.
//!
//! Without the files, everything is unscii (gpi's built-in font).

use alloc::boxed::Box;
use alloc::vec::Vec;

use gpi::font::{Font, Os2FontInfo, Outlines, Source};
use gpi::ttf::Ttf;
use gpi::type1::Type1;

use crate::state::st;

/// The system font's size: pels to the em.
pub const SYSTEM_EM: i32 = 13;

/// The faces: (face name, files in PSFONTS, the first there wins).
const FACES: &[(&[u8], &[&[u8]])] = &[
    (b"Helvetica", &[b"HELV.PFB", b"LIBSANS.TTF"]),
    (b"Helvetica Bold", &[b"HELVB.PFB", b"LIBSANSB.TTF"]),
    (b"Helvetica Italic", &[b"HELVI.PFB", b"LIBSANSI.TTF"]),
    (b"Helvetica Bold Italic", &[b"HELVBI.PFB", b"LIBSANBI.TTF"]),
    (b"Times New Roman", &[b"TNR.PFB", b"LIBSERIF.TTF"]),
    (b"Times New Roman Bold", &[b"TNRB.PFB", b"LIBSERFB.TTF"]),
    (b"Times New Roman Italic", &[b"TNRI.PFB", b"LIBSERFI.TTF"]),
    (b"Times New Roman Bold Italic", &[b"TNRBI.PFB", b"LIBSRFBI.TTF"]),
    (b"Courier", &[b"COUR.PFB", b"LIBMONO.TTF"]),
    (b"Courier Bold", &[b"COURB.PFB", b"LIBMONOB.TTF"]),
    (b"Courier Italic", &[b"COURI.PFB", b"LIBMONOI.TTF"]),
    (b"Courier Bold Italic", &[b"COURBI.PFB", b"LIBMONBI.TTF"]),
    (b"Helv", &[b"HELV.PFB", b"LIBSANS.TTF"]),
    (b"Tms Rmn", &[b"TNR.PFB", b"LIBSERIF.TTF"]),
    (b"System Proportional", &[b"LIBSANS.TTF"]),
    (b"WarpSans", &[b"LIBSANS.TTF"]),
    (b"System Monospaced", &[b"LIBMONO.TTF"]),
];

/// IBM's image font files (OS2\DLL).
const IMAGE_FILES: &[&[u8]] = &[b"DSPRES.DLL", b"HELV.FON", b"TIMES.FON", b"COURIER.FON", b"COURIERI.FON", b"SYSMONO.FON"];

/// What the shared state keeps.
#[derive(Default)]
pub struct Fonts {
    started: bool,
    /// Faces made so far: (FACES index, its source).
    sources: Vec<(usize, &'static Source)>,
    /// Files read: (name, the font in it).
    files: Vec<(&'static [u8], &'static dyn Outlines)>,
    /// IBM's image fonts: (file, what the font says, the font).
    public: Vec<(&'static [u8], Os2FontInfo, &'static Font)>,
    pub system: Option<&'static Font>,
    pub mono: Option<&'static Font>,
}

fn boot_drive() -> u8 {
    let mut v = [0u32; 1];
    // QSV_BOOT_DRIVE
    unsafe { os2::DosQuerySysInfo(5, 5, v.as_mut_ptr(), 4) };
    if (1..=26).contains(&v[0]) { b'A' + v[0] as u8 - 1 } else { b'C' }
}

fn path(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = alloc::vec![boot_drive(), b':', b'\\'];
    p.extend_from_slice(dir);
    p.push(b'\\');
    p.extend_from_slice(name);
    p
}

/// A font file in PSFONTS (TrueType or Type 1), read once.
fn file(name: &'static [u8]) -> Option<&'static dyn Outlines> {
    if let Some(&(_, t)) = st().fonts.files.iter().find(|(n, _)| *n == name) {
        return Some(t);
    }
    let data = crate::gpiseg::read_file(&path(b"PSFONTS", name))?;
    let t: &'static dyn Outlines = if name.ends_with(b".PFB") {
        Box::leak(Box::new(Type1::parse(&data)?))
    } else {
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        Box::leak(Box::new(Ttf::parse(data)?))
    };
    st().fonts.files.push((name, t));
    Some(t)
}

/// The FACES index of a face name.
pub fn face_index(face: &[u8]) -> Option<usize> {
    FACES.iter().position(|(n, _)| n.eq_ignore_ascii_case(face))
}

/// The face's variant with the given style, if there is one
/// ("Helvetica" bold: "Helvetica Bold").
pub fn styled(i: usize, bold: bool, italic: bool) -> usize {
    let has = |w: &[u8]| FACES[i].0.windows(w.len()).any(|x| x == w);
    let (bold, italic) = (bold || has(b"Bold"), italic || has(b"Italic"));
    let base = FACES[i].0;
    let base = base.strip_suffix(b" Italic").unwrap_or(base);
    let base = base.strip_suffix(b" Bold").unwrap_or(base);
    // The image faces' styles come from their outline families.
    let styled_any = bold || italic;
    let base: &[u8] = match base {
        b"Helv" | b"System Proportional" | b"WarpSans" if styled_any => b"Helvetica",
        b"Tms Rmn" if styled_any => b"Times New Roman",
        b"System Monospaced" if styled_any => b"Courier",
        b => b,
    };
    let mut want = base.to_vec();
    if bold {
        want.extend_from_slice(b" Bold");
    }
    if italic {
        want.extend_from_slice(b" Italic");
    }
    face_index(&want).unwrap_or(i)
}

/// The name of face `i`.
pub fn face_name(i: usize) -> &'static [u8] {
    FACES.get(i).map_or(b"", |f| f.0)
}

/// The outlines of face `i` (FACES index).
pub fn source(i: usize) -> Option<&'static Source> {
    if let Some(&(_, s)) = st().fonts.sources.iter().find(|(k, _)| *k == i) {
        return Some(s);
    }
    let (face, names) = *FACES.get(i)?;
    let t = names.iter().find_map(|n| file(n))?;
    let face_name: &'static str = core::str::from_utf8(face).unwrap_or("Helvetica");
    let s: &'static Source = Box::leak(Box::new(Source::new(face_name, t, 0.0)));
    st().fonts.sources.push((i, s));
    Some(s)
}

/// The font of face `i` at an em of `px` pels.
pub fn face(i: usize, px: i32) -> Option<&'static Font> {
    source(i)?.at(px)
}

/// The monospaced font (MLEs): Liberation Mono, else unscii.
pub fn mono() -> &'static Font {
    st().fonts.mono.unwrap_or_else(gpi::font::vio)
}

/// IBM's WarpSans at 9 points for a 96 dpi screen, if installed.
fn warpsans() -> Option<&'static Font> {
    st().fonts
        .public
        .iter()
        .filter(|(_, i, _)| i.face.eq_ignore_ascii_case(b"WarpSans") && !i.face.is_empty())
        // usNominalPointSize (decipoints), xDeviceRes.
        .min_by_key(|(_, i, _)| (i.fields[25] as i32 - 90).abs() * 4 + (i.fields[19] as i32 - 96).abs())
        .map(|&(_, _, f)| f)
}

/// When a process joins PM: the first loads IBM's image fonts and makes
/// the system fonts; every process then draws with them.
pub fn init() {
    if !st().fonts.started {
        st().fonts.started = true;
        for &name in IMAGE_FILES {
            let p = path(b"OS2\\DLL", name);
            if crate::gpiseg::read_size(&p).is_none() {
                continue;
            }
            for (font, info) in crate::gpitext::module_fonts(&p) {
                let f: &'static Font = Box::leak(Box::new(font));
                st().fonts.public.push((name, info, f));
            }
        }
        st().fonts.system = warpsans().or_else(|| face_index(b"System Proportional").and_then(|i| face(i, SYSTEM_EM)));
        st().fonts.mono = face_index(b"System Monospaced").and_then(|i| face(i, SYSTEM_EM));
    }
    let public: Vec<(&[u8], Os2FontInfo, &'static Font)> = st().fonts.public.iter().map(|(n, i, f)| (*n, i.clone(), *f)).collect();
    for (n, i, f) in public {
        crate::gpitext::add_loaded(&path(b"OS2\\DLL", n), i, f);
    }
    if let Some(f) = st().fonts.system {
        gpi::font::set_system(f);
    }
}

/// A font by its presentation parameter name, "points.face[.Bold]
/// [.Italic]" ("10.Helv", "12.Times New Roman.Bold"): IBM's bitmap at
/// that size for 96 dpi where installed, else the outline face at
/// points * 96 / 72 pels.
pub fn by_name(name: &[u8]) -> Option<&'static Font> {
    let dot = name.iter().position(|&c| c == b'.')?;
    let pt: i32 = core::str::from_utf8(&name[..dot]).ok()?.trim().parse().ok()?;
    if !(1..=144).contains(&pt) {
        return None;
    }
    let mut parts = name[dot + 1..].split(|&c| c == b'.');
    let fname = parts.next()?.trim_ascii();
    let (mut bold, mut italic) = (false, false);
    for a in parts {
        bold |= a.eq_ignore_ascii_case(b"Bold");
        italic |= a.eq_ignore_ascii_case(b"Italic");
    }
    if !bold && !italic
        && let Some(&(_, _, f)) = st().fonts.public.iter().find(|(_, i, _)| i.face.eq_ignore_ascii_case(fname) && i.fields[25] as i32 == pt * 10 && i.fields[19] == 96)
    {
        return Some(f);
    }
    let i = face_index(fname)?;
    face(styled(i, bold, italic), (pt * 96 + 36) / 72)
}
