//! The fonts PM draws with, kept in the shared state so every PM process
//! uses the same ones (their glyphs are rendered once, in the shared
//! heap):
//!
//! - the system font (controls, menus, title bars): Liberation Sans at
//!   `SYSTEM_EM` pels, anti-aliased;
//! - the monospaced font (MLEs, "System Monospaced"): Liberation Mono;
//! - the outline faces GPI offers (Helvetica, Times New Roman, Courier,
//!   their bold and italic, and the image faces Helv, Tms Rmn, System
//!   Proportional and System Monospaced): each from a file in the boot
//!   drive's PSFONTS directory, read the first time it is asked for.
//!
//! Without the files, everything is unscii (gpi's built-in font).

use alloc::boxed::Box;
use alloc::vec::Vec;

use gpi::font::{Font, Source};
use gpi::ttf::Ttf;

use crate::state::st;

/// The system font's size: pels to the em.
pub const SYSTEM_EM: i32 = 13;

/// The faces: (face name, file in PSFONTS).
const FACES: &[(&[u8], &[u8])] = &[
    (b"Helvetica", b"LIBSANS.TTF"),
    (b"Helvetica Bold", b"LIBSANSB.TTF"),
    (b"Helvetica Italic", b"LIBSANSI.TTF"),
    (b"Helvetica Bold Italic", b"LIBSANBI.TTF"),
    (b"Times New Roman", b"LIBSERIF.TTF"),
    (b"Times New Roman Bold", b"LIBSERFB.TTF"),
    (b"Times New Roman Italic", b"LIBSERFI.TTF"),
    (b"Times New Roman Bold Italic", b"LIBSRFBI.TTF"),
    (b"Courier", b"LIBMONO.TTF"),
    (b"Courier Bold", b"LIBMONOB.TTF"),
    (b"Courier Italic", b"LIBMONOI.TTF"),
    (b"Courier Bold Italic", b"LIBMONBI.TTF"),
    (b"Helv", b"LIBSANS.TTF"),
    (b"Tms Rmn", b"LIBSERIF.TTF"),
    (b"System Proportional", b"LIBSANS.TTF"),
    (b"WarpSans", b"LIBSANS.TTF"),
    (b"System Monospaced", b"LIBMONO.TTF"),
];

/// What the shared state keeps.
#[derive(Default)]
pub struct Fonts {
    started: bool,
    /// Faces made so far: (FACES index, its source).
    sources: Vec<(usize, &'static Source)>,
    /// Files read: (name, the font in it).
    files: Vec<(&'static [u8], &'static Ttf)>,
    pub system: Option<&'static Font>,
    pub mono: Option<&'static Font>,
}

fn boot_drive() -> u8 {
    let mut v = [0u32; 1];
    // QSV_BOOT_DRIVE
    unsafe { os2::DosQuerySysInfo(5, 5, v.as_mut_ptr(), 4) };
    if (1..=26).contains(&v[0]) { b'A' + v[0] as u8 - 1 } else { b'C' }
}

/// A font file in PSFONTS, read once.
fn file(name: &'static [u8]) -> Option<&'static Ttf> {
    let f = &mut st().fonts;
    if let Some(&(_, t)) = f.files.iter().find(|(n, _)| *n == name) {
        return Some(t);
    }
    let mut path = alloc::vec![boot_drive()];
    path.extend_from_slice(b":\\PSFONTS\\");
    path.extend_from_slice(name);
    let data: &'static [u8] = Box::leak(crate::gpiseg::read_file(&path)?.into_boxed_slice());
    let t: &'static Ttf = Box::leak(Box::new(Ttf::parse(data)?));
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
    let (face, name) = *FACES.get(i)?;
    let t = file(name)?;
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

/// When a process joins PM: the first makes the system fonts; every
/// process then draws with them.
pub fn init() {
    if !st().fonts.started {
        st().fonts.started = true;
        st().fonts.system = face_index(b"System Proportional").and_then(|i| face(i, SYSTEM_EM));
        st().fonts.mono = face_index(b"System Monospaced").and_then(|i| face(i, SYSTEM_EM));
    }
    if let Some(f) = st().fonts.system {
        gpi::font::set_system(f);
    }
}
