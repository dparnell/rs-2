//! OS/2 path handling: canonicalisation and wildcards.

use alloc::string::String;
use alloc::vec::Vec;

pub const MAX_PATH: usize = 260;

/// A fully qualified path: drive (0 = A:) and components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FullPath {
    pub drive: u8,
    pub parts: Vec<String>,
}

impl FullPath {
    /// The path below the drive root, `\`-separated without a leading `\`.
    pub fn rel(&self) -> String {
        self.parts.join("\\")
    }

    pub fn display(&self) -> String {
        let mut s = String::new();
        s.push((b'A' + self.drive) as char);
        s.push_str(":\\");
        s.push_str(&self.rel());
        s
    }

    pub fn file_name(&self) -> &str {
        self.parts.last().map_or("", |s| s.as_str())
    }

    pub fn parent(&self) -> FullPath {
        let mut p = self.clone();
        p.parts.pop();
        p
    }
}

/// Resolve `path` against the current drive and directories.
///
/// `cwd` holds the current directory for each drive (relative to the root,
/// no leading backslash). Applies OS/2 rules: `/` equals `\`, `.` and `..`
/// components, and trailing dots and blanks of a component are dropped.
pub fn canonical(path: &str, cur_drive: u8, cwd: &[String; 26]) -> Result<FullPath, u32> {
    let b = path.as_bytes();
    let (drive, rest) = if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        (b[0].to_ascii_uppercase() - b'A', &path[2..])
    } else {
        (cur_drive, path)
    };
    if drive >= 26 {
        return Err(15); // ERROR_INVALID_DRIVE
    }
    let absolute = rest.starts_with(['\\', '/']);
    let mut all: Vec<&str> = Vec::new();
    if !absolute {
        all.extend(cwd[drive as usize].split('\\').filter(|s| !s.is_empty()));
    }
    all.extend(rest.split(['\\', '/']));
    let mut out: Vec<String> = Vec::new();
    for comp in all {
        let t = comp.trim_end_matches(' ');
        if t.is_empty() || t == "." {
            continue;
        }
        if t.bytes().all(|c| c == b'.') {
            // "..", "...": each dot after the first goes up one level.
            for _ in 1..t.len() {
                if out.pop().is_none() {
                    break;
                }
            }
            continue;
        }
        if t.chars().any(|c| matches!(c, '<' | '>' | '|' | '"' | ':') || (c as u32) < 0x20) {
            return Err(123); // ERROR_INVALID_NAME
        }
        out.push(String::from(t.trim_end_matches(['.', ' '])));
    }
    let fp = FullPath { drive, parts: out };
    if fp.display().len() >= MAX_PATH {
        return Err(206); // ERROR_FILENAME_EXCED_RANGE
    }
    Ok(fp)
}

pub fn has_wildcards(s: &str) -> bool {
    s.contains(['*', '?'])
}

/// OS/2 wildcard match (case-insensitive). `*` matches any run of
/// characters, `?` one character (or none at the end of a name part), and
/// `*.*` matches names without an extension as well.
pub fn matches(pattern: &str, name: &str) -> bool {
    if pattern == "*" || pattern == "*.*" {
        return true;
    }
    let p: Vec<u8> = pattern.bytes().map(|c| c.to_ascii_uppercase()).collect();
    let n: Vec<u8> = name.bytes().map(|c| c.to_ascii_uppercase()).collect();
    if glob(&p, &n) {
        return true;
    }
    // "NAME.*" also matches "NAME" (no extension); "NAME." matches "NAME".
    if !n.contains(&b'.') {
        if let Some(stripped) = p.strip_suffix(b".*") {
            return glob(stripped, &n);
        }
        if let Some(stripped) = p.strip_suffix(b".") {
            return glob(stripped, &n);
        }
    }
    false
}

fn glob(p: &[u8], n: &[u8]) -> bool {
    match (p.first(), n.first()) {
        (None, None) => true,
        (Some(b'*'), _) => glob(&p[1..], n) || (!n.is_empty() && glob(p, &n[1..])),
        (Some(b'?'), Some(_)) => glob(&p[1..], &n[1..]) || glob(&p[1..], n),
        (Some(b'?'), None) => glob(&p[1..], n),
        (Some(a), Some(b)) => a == b && glob(&p[1..], &n[1..]),
        _ => false,
    }
}
