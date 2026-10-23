//! 8.3 names.

use alloc::string::String;

fn valid_char(c: u8) -> bool {
    c >= 0x80
        || c.is_ascii_alphanumeric()
        || b"!#$%&'()-@^_`{}~".contains(&c)
}

/// True if `name` is already a valid 8.3 name (case-insensitively).
pub fn valid_83(name: &str) -> bool {
    to_83(name).is_some()
}

/// Normalise a name to upper-case `NAME.EXT`, or None if it does not fit
/// 8.3 or contains characters FAT does not allow.
pub fn to_83(name: &str) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let (base, ext) = match name.rfind('.') {
        Some(0) => return None,
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    };
    // A trailing dot means "no extension" (OS/2 strips it).
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return None;
    }
    // OS/2 takes spaces inside FAT names (`EA DATA. SF`, `WP ROOT. SF`),
    // not at the start, nor at the end of a part (they would not come
    // back from the padded directory form).
    let ok = |p: &str| p.bytes().all(|c| valid_char(c) || c == b' ') && !p.ends_with(' ');
    if !ok(base) || !ok(ext) || base.starts_with(' ') {
        return None;
    }
    let mut s = String::with_capacity(12);
    s.push_str(&base.to_ascii_uppercase());
    if !ext.is_empty() {
        s.push('.');
        s.push_str(&ext.to_ascii_uppercase());
    }
    Some(s)
}

/// Pack `NAME.EXT` into the 11-byte directory form.
pub fn pack_83(name: &str) -> Option<[u8; 11]> {
    let mut out = [b' '; 11];
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    };
    if base.len() > 8 || ext.len() > 3 {
        return None;
    }
    for (d, s) in out.iter_mut().zip(base.bytes()) {
        *d = s.to_ascii_uppercase();
    }
    for (d, s) in out[8..].iter_mut().zip(ext.bytes()) {
        *d = s.to_ascii_uppercase();
    }
    if out[0] == 0xe5 {
        out[0] = 0x05;
    }
    Some(out)
}

/// Unpack an 11-byte directory name to `NAME.EXT`.
pub fn from_83(raw: &[u8]) -> String {
    let mut b = [0u8; 11];
    b.copy_from_slice(&raw[..11]);
    if b[0] == 0x05 {
        b[0] = 0xe5;
    }
    let base = trim(&b[..8]);
    let ext = trim(&b[8..11]);
    let mut s: String = base.iter().map(|&c| c as char).collect();
    if !ext.is_empty() {
        s.push('.');
        s.extend(ext.iter().map(|&c| c as char));
    }
    s
}

fn trim(b: &[u8]) -> &[u8] {
    let n = b.iter().rposition(|&c| c != b' ').map_or(0, |i| i + 1);
    &b[..n]
}
