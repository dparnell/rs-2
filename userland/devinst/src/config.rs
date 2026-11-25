//! CONFIG.SYS: the paths and settings of the toolchain.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The setting a line makes: `SET NAME=` or `NAME=`, upper case.
fn key(line: &[u8]) -> Option<String> {
    let eq = line.iter().position(|&c| c == b'=')?;
    let k = String::from_utf8_lossy(&line[..eq]).to_ascii_uppercase();
    let mut words = k.split_whitespace();
    Some(match (words.next()?, words.next(), words.next()) {
        ("SET", Some(n), None) => format!("SET {n}"),
        (n, None, None) => n.to_string(),
        _ => return None,
    })
}

fn value(line: &[u8]) -> &[u8] {
    let at = line.iter().position(|&c| c == b'=').map_or(line.len(), |i| i + 1);
    line[at..].trim_ascii()
}

/// The text with the settings for a toolchain in `dir` (and those for an
/// earlier one elsewhere gone). Bytes: other lines stay as they are, in
/// whatever code page.
pub fn edit(text: &[u8], dir: &str) -> Vec<u8> {
    let bin = format!("{dir}\\sdk\\bin");
    let mut lines: Vec<Vec<u8>> =
        text.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let eof = lines.last().is_some_and(|l| l == b"\x1a");
    if eof {
        lines.pop();
    }
    let find = |lines: &[Vec<u8>], k: &str| lines.iter().position(|l| key(l).as_deref() == Some(k));
    // Where an earlier installation was: CARGO_HOME is its DIR\cargo.
    let old_bin = find(&lines, "SET CARGO_HOME").and_then(|i| {
        let v = String::from_utf8_lossy(value(&lines[i])).to_string();
        Some(format!("{}\\sdk\\bin", v.strip_suffix("\\cargo")?))
    });
    for (k, what) in [("LIBPATH", "LIBPATH="), ("SET PATH", "SET PATH=")] {
        match find(&lines, k) {
            Some(i) => {
                let old = &lines[i];
                let head = old.len() - old.iter().position(|&c| c == b'=').map_or(0, |p| old.len() - p - 1);
                let is = |e: &[u8], b: &str| e.trim_ascii().eq_ignore_ascii_case(b.as_bytes());
                if value(old).split(|&c| c == b';').any(|e| is(e, &bin)) {
                    continue;
                }
                let mut l = old[..head].to_vec();
                for e in value(old).split(|&c| c == b';') {
                    if !e.is_empty() && !old_bin.as_ref().is_some_and(|b| is(e, b)) {
                        l.extend_from_slice(e);
                        l.push(b';');
                    }
                }
                l.extend_from_slice(bin.as_bytes());
                lines[i] = l;
            }
            None => lines.push(format!("{what}{bin}").into_bytes()),
        }
    }
    // cargo's own directory, and where the compilers' temporary files go,
    // unless that is set already.
    for (k, v, ours) in [("SET CARGO_HOME", format!("{dir}\\cargo"), true), ("SET TMPDIR", format!("{dir}\\tmp"), false)] {
        let line = format!("{k}={v}").into_bytes();
        match find(&lines, k) {
            Some(i) => {
                let old = String::from_utf8_lossy(value(&lines[i])).to_string();
                let earlier = old_bin.as_ref().is_some_and(|b| old.eq_ignore_ascii_case(&b.replace("\\sdk\\bin", "\\tmp")));
                if (ours || earlier) && !old.eq_ignore_ascii_case(&v) {
                    lines[i] = line;
                }
            }
            None => lines.push(line),
        }
    }
    let mut out = Vec::with_capacity(text.len() + 256);
    for l in &lines {
        out.extend_from_slice(l);
        out.extend_from_slice(b"\r\n");
    }
    if eof {
        out.push(0x1a);
    }
    out
}
