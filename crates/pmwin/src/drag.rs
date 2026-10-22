//! Direct manipulation strings: the rendering mechanism and format
//! (RMF) strings and type lists of DRAGITEMs, as PMDRAG's
//! `DrgVerifyRMF`, `DrgQueryNativeRMF`, `DrgVerifyType` and friends read
//! them.
//!
//! An RMF string lists mechanism/format pairs, separated by commas:
//! `<DRM_OS2FILE,DRF_TEXT>` is one pair, `(DRM_OS2FILE,DRM_PRINT)x(DRF_TEXT,DRF_UNKNOWN)`
//! the cross product of two lists; blanks around the names do not count.
//! The first pair is the native one. A type list is names separated by
//! commas (`Plain Text,C Code`); the first is the true type.

use alloc::vec::Vec;

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&c| c != b' ' && c != b'\t').unwrap_or(s.len());
    let b = s.iter().rposition(|&c| c != b' ' && c != b'\t').map_or(a, |i| i + 1);
    &s[a..b.max(a)]
}

fn list(s: &[u8]) -> Vec<&[u8]> {
    s.split(|&c| c == b',').map(trim).filter(|x| !x.is_empty()).collect()
}

/// The (mechanism, format) pairs of an RMF string, in order.
pub fn rmf_pairs(s: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'<' => {
                let Some(end) = s[i..].iter().position(|&c| c == b'>') else { break };
                let inner = &s[i + 1..i + end];
                if let Some(k) = inner.iter().position(|&c| c == b',') {
                    let (m, f) = (trim(&inner[..k]), trim(&inner[k + 1..]));
                    if !m.is_empty() && !f.is_empty() {
                        out.push((m, f));
                    }
                }
                i += end + 1;
            }
            b'(' => {
                let Some(e1) = s[i..].iter().position(|&c| c == b')') else { break };
                let mechs = list(&s[i + 1..i + e1]);
                let mut j = i + e1 + 1;
                while j < s.len() && matches!(s[j], b' ' | b'\t') {
                    j += 1;
                }
                if j >= s.len() || !matches!(s[j], b'x' | b'X') {
                    i = j;
                    continue;
                }
                j += 1;
                while j < s.len() && matches!(s[j], b' ' | b'\t') {
                    j += 1;
                }
                if j >= s.len() || s[j] != b'(' {
                    i = j;
                    continue;
                }
                let Some(e2) = s[j..].iter().position(|&c| c == b')') else { break };
                let fmts = list(&s[j + 1..j + e2]);
                for &m in &mechs {
                    for &f in &fmts {
                        out.push((m, f));
                    }
                }
                i = j + e2 + 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Whether an RMF string has a mechanism and format (`None` matches
/// any).
pub fn rmf_has(s: &[u8], mech: Option<&[u8]>, fmt: Option<&[u8]>) -> bool {
    rmf_pairs(s).iter().any(|&(m, f)| mech.is_none_or(|x| trim(x) == m) && fmt.is_none_or(|x| trim(x) == f))
}

/// The native RMF of a string, as `<mechanism,format>`.
pub fn native_rmf(s: &[u8]) -> Option<Vec<u8>> {
    let (m, f) = *rmf_pairs(s).first()?;
    let mut v = alloc::vec![b'<'];
    v.extend_from_slice(m);
    v.push(b',');
    v.extend_from_slice(f);
    v.push(b'>');
    Some(v)
}

/// Whether the native pair of `s` is the (first) pair of `rmf`.
pub fn native_is(s: &[u8], rmf: &[u8]) -> bool {
    match (rmf_pairs(s).first(), rmf_pairs(rmf).first()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// The types of a type list.
pub fn types(s: &[u8]) -> Vec<&[u8]> {
    list(s)
}

/// The true type: the first of the list.
pub fn true_type(s: &[u8]) -> Option<&[u8]> {
    types(s).first().copied()
}

/// Whether a type list has one of the types of another list.
pub fn has_type(s: &[u8], wanted: &[u8]) -> bool {
    first_match(s, wanted).is_some()
}

/// The first type of `wanted` (a list) that `s` has.
pub fn first_match<'a>(s: &[u8], wanted: &'a [u8]) -> Option<&'a [u8]> {
    let have = types(s);
    types(wanted).into_iter().find(|w| have.contains(w))
}

// ---- Operations ----

pub const DO_DEFAULT: u16 = 0xbffe;
pub const DO_UNKNOWN: u16 = 0xbfff;
pub const DO_COPY: u16 = 0x0010;
pub const DO_MOVE: u16 = 0x0020;
pub const DO_LINK: u16 = 0x0018;
pub const DO_CREATE: u16 = 0x0040;

pub const KC_CTRL: u16 = 0x0010;
pub const KC_SHIFT: u16 = 0x0008;

/// The operation the keys held down ask for (PMDRAG's rule): Ctrl copy,
/// Shift move, both link (a shadow), none the default; any other key
/// held is an operation nobody knows.
pub fn operation(ctrl: bool, shift: bool, other: bool) -> u16 {
    match (ctrl, shift, other) {
        (_, _, true) => DO_UNKNOWN,
        (true, true, _) => DO_LINK,
        (true, false, _) => DO_COPY,
        (false, true, _) => DO_MOVE,
        _ => DO_DEFAULT,
    }
}

/// The Workplace Shell's default for files: a move on the same drive, a
/// copy to another one.
pub fn file_default(src_drive: u8, dst_drive: u8) -> u16 {
    if src_drive.eq_ignore_ascii_case(&dst_drive) { DO_MOVE } else { DO_COPY }
}

// ---- Names ----

/// A name for a copy that does not collide with existing ones (as the
/// Workplace Shell offers in its name collision dialog): `NAME:1`-style
/// suffixes are not valid file names, so `NAME (2).EXT` on long-name file
/// systems, and the 8.3 form `NAME~2.EXT` otherwise.
pub fn unique_name(name: &[u8], long: bool, exists: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let (stem, ext) = match name.iter().rposition(|&c| c == b'.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, &b""[..]),
    };
    for n in 2..10000u32 {
        let num = alloc::format!("{n}").into_bytes();
        let mut c = Vec::new();
        if long {
            c.extend_from_slice(stem);
            c.extend_from_slice(b" (");
            c.extend_from_slice(&num);
            c.push(b')');
        } else {
            let keep = 8usize.saturating_sub(num.len() + 1).min(stem.len());
            c.extend_from_slice(&stem[..keep]);
            c.push(b'~');
            c.extend_from_slice(&num);
        }
        c.extend_from_slice(ext);
        if !exists(&c) {
            return c;
        }
    }
    name.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs() {
        assert_eq!(rmf_pairs(b"<DRM_OS2FILE,DRF_TEXT>"), [(&b"DRM_OS2FILE"[..], &b"DRF_TEXT"[..])]);
        let p = rmf_pairs(b" < DRM_OS2FILE , DRF_TEXT > , <DRM_PRINT,DRF_UNKNOWN>");
        assert_eq!(p.len(), 2);
        assert_eq!(p[1], (&b"DRM_PRINT"[..], &b"DRF_UNKNOWN"[..]));
        let x = rmf_pairs(b"(DRM_OS2FILE,DRM_PRINT) x (DRF_TEXT,DRF_UNKNOWN),<DRM_OBJECT,DRF_OBJECT>");
        assert_eq!(x.len(), 5);
        assert_eq!(x[0], (&b"DRM_OS2FILE"[..], &b"DRF_TEXT"[..]));
        assert_eq!(x[3], (&b"DRM_PRINT"[..], &b"DRF_UNKNOWN"[..]));
        assert_eq!(x[4], (&b"DRM_OBJECT"[..], &b"DRF_OBJECT"[..]));
        assert!(rmf_pairs(b"garbage").is_empty());
        assert!(rmf_pairs(b"<DRM_X").is_empty());
    }

    #[test]
    fn verify() {
        let s = b"(DRM_OS2FILE,DRM_DISCARD)x(DRF_UNKNOWN)";
        assert!(rmf_has(s, Some(b"DRM_OS2FILE"), None));
        assert!(rmf_has(s, Some(b"DRM_DISCARD"), Some(b"DRF_UNKNOWN")));
        assert!(!rmf_has(s, Some(b"DRM_PRINT"), None));
        assert!(rmf_has(s, None, Some(b"DRF_UNKNOWN")));
        assert_eq!(native_rmf(s).unwrap(), b"<DRM_OS2FILE,DRF_UNKNOWN>");
        assert!(native_is(s, b"<DRM_OS2FILE, DRF_UNKNOWN>"));
        assert!(!native_is(s, b"<DRM_DISCARD,DRF_UNKNOWN>"));
    }

    #[test]
    fn type_lists() {
        assert_eq!(true_type(b" Plain Text , C Code"), Some(&b"Plain Text"[..]));
        assert!(has_type(b"Plain Text,C Code", b"C Code"));
        assert_eq!(first_match(b"Plain Text,C Code", b"Bitmap,C Code,Plain Text"), Some(&b"C Code"[..]));
        assert!(!has_type(b"Plain Text", b"Bitmap"));
        assert_eq!(true_type(b""), None);
    }

    #[test]
    fn ops() {
        assert_eq!(operation(false, false, false), DO_DEFAULT);
        assert_eq!(operation(true, false, false), DO_COPY);
        assert_eq!(operation(false, true, false), DO_MOVE);
        assert_eq!(operation(true, true, false), DO_LINK);
        assert_eq!(operation(false, false, true), DO_UNKNOWN);
        assert_eq!(file_default(b'C', b'c'), DO_MOVE);
        assert_eq!(file_default(b'C', b'D'), DO_COPY);
    }

    #[test]
    fn names() {
        let taken: [&[u8]; 2] = [b"README (2).TXT", b"README~2.TXT"];
        assert_eq!(unique_name(b"README.TXT", true, |n| taken.contains(&n)), b"README (3).TXT");
        assert_eq!(unique_name(b"README.TXT", false, |n| taken.contains(&n)), b"README~3.TXT");
        assert_eq!(unique_name(b"LONGNAMES.TXT", false, |_| false), b"LONGNA~2.TXT");
        assert_eq!(unique_name(b"Folder", true, |_| false), b"Folder (2)");
        assert_eq!(unique_name(b".profile", true, |_| false), b".profile (2)");
    }
}
