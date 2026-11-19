//! The Feature Installer's text file edits (AsciiFile blocks) on lines of
//! CONFIG.SYS, WIN.INI and the packages' own files.
//!
//! Three kinds occur in the BonusPak:
//! - a list on a line (`LIBPATH=`, `SET HELP=`, with a delimiter): the
//!   entry is added unless the list has one containing the search text;
//!   a missing line is made;
//! - a line found by a pattern (`DEVICE=.*\\ecrdrv.sys`): replaced by the
//!   new line (or removed when there is none), or added when there is no
//!   such line: at the end, or after the AddAfterLine line;
//! - lines found by a pattern (EditLine) that lack a mark (`^REM`): the
//!   new line replaces them, `^` in it standing for the old line
//!   (`REM  ^` comments a line out).
//!
//! Patterns are the installer's: `.` any character, `*` repeats the one
//! before, `\` takes the next literally, `^` at the start anchors there;
//! no regard to case. Otherwise they match anywhere in the line.

use crate::fi::Edit;
use alloc::vec::Vec;

fn eq(a: u8, b: u8) -> bool {
    a.eq_ignore_ascii_case(&b)
}

/// One atom of a pattern at `p`: (matches byte c, atom length).
fn atom(p: &[u8], c: u8) -> (bool, usize) {
    match p[0] {
        b'.' => (true, 1),
        b'\\' if p.len() > 1 => (eq(p[1], c), 2),
        x => (eq(x, c), 1),
    }
}

fn here(p: &[u8], s: &[u8]) -> bool {
    if p.is_empty() {
        return true;
    }
    if p == b"$" {
        return s.is_empty();
    }
    let (_, n) = atom(p, 0);
    if p.len() > n && p[n] == b'*' {
        let rest = &p[n + 1..];
        let mut i = 0;
        loop {
            if here(rest, &s[i..]) {
                return true;
            }
            if i < s.len() && atom(p, s[i]).0 {
                i += 1;
            } else {
                return false;
            }
        }
    }
    !s.is_empty() && atom(p, s[0]).0 && here(&p[n..], &s[1..])
}

/// Whether `pattern` matches `line`.
pub fn matches(pattern: &[u8], line: &[u8]) -> bool {
    if let Some(p) = pattern.strip_prefix(b"^") {
        return here(p, line.trim_ascii_start());
    }
    (0..=line.len()).any(|i| here(pattern, &line[i..]))
}

fn starts_with_nocase(line: &[u8], start: &[u8]) -> bool {
    let l = line.trim_ascii_start();
    l.len() >= start.len() && l[..start.len()].eq_ignore_ascii_case(start)
}

fn contains_nocase(hay: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

fn is_pattern(s: &[u8]) -> bool {
    s.contains(&b'*') || s.starts_with(b"^") || s.contains(&b'\\')
}

/// Apply `e` to the file's lines. Returns whether they changed.
pub fn apply(e: &Edit, lines: &mut Vec<Vec<u8>>) -> bool {
    // A list on a line.
    if let (Some(d), false) = (e.delimiter, e.edit_line.is_empty() || is_pattern(&e.edit_line)) {
        let Some(i) = lines.iter().position(|l| starts_with_nocase(l, &e.edit_line)) else {
            if e.not_found == 1 || e.new_line.is_empty() {
                return false;
            }
            let mut l = e.edit_line.clone();
            if !l.ends_with(b"=") {
                l.push(b'=');
            }
            l.extend_from_slice(&e.new_line);
            l.push(d);
            lines.push(l);
            return true;
        };
        let line = &lines[i];
        let start = line.iter().position(|&c| c == b'=').map_or(e.edit_line.len(), |p| p + 1);
        if line[start..].split(|&c| c == d).any(|entry| contains_nocase(entry, &e.search)) {
            return false;
        }
        let mut l = line.clone();
        while l.last() == Some(&b' ') {
            l.pop();
        }
        if l.ends_with(&[d]) {
            l.extend_from_slice(&e.new_line);
            l.push(d);
        } else {
            if l.len() > start {
                l.push(d);
            }
            l.extend_from_slice(&e.new_line);
        }
        lines[i] = l;
        return true;
    }
    // Lines found by EditLine's pattern without the search mark.
    if !e.edit_line.is_empty() {
        let mut changed = false;
        for l in lines.iter_mut() {
            if matches(&e.edit_line, l) && !matches(&e.search, l) && e.not_found == 1 {
                let mut n = Vec::new();
                for &c in &e.new_line {
                    if c == b'^' {
                        n.extend_from_slice(l);
                    } else {
                        n.push(c);
                    }
                }
                *l = n;
                changed = true;
            }
        }
        return changed;
    }
    // A line found by a pattern.
    let found: Vec<usize> = (0..lines.len()).filter(|&i| matches(&e.search, &lines[i])).collect();
    if !found.is_empty() {
        if e.found != 1 {
            return false;
        }
        let mut changed = false;
        for &i in found.iter().rev() {
            if e.new_line.is_empty() {
                lines.remove(i);
                changed = true;
            } else if lines[i] != e.new_line {
                lines[i] = e.new_line.clone();
                changed = true;
            }
        }
        return changed;
    }
    if e.new_line.is_empty() || !(e.not_found == 2 || e.not_found == 4) {
        return false;
    }
    let after = (!e.add_after.is_empty()).then(|| lines.iter().position(|l| starts_with_nocase(l, &e.add_after))).flatten();
    let before = (!e.add_before.is_empty()).then(|| lines.iter().position(|l| starts_with_nocase(l, &e.add_before))).flatten();
    let at = match (after, before) {
        (Some(a), _) => a + 1,
        (None, Some(b)) => b,
        // At the end, before blank lines and a closing Ctrl+Z.
        _ => {
            let mut n = lines.len();
            while n > 0 && (lines[n - 1].is_empty() || lines[n - 1] == b"\x1a") {
                n -= 1;
            }
            n
        }
    };
    lines.insert(at, e.new_line.clone());
    true
}

/// A text file's lines (without their line ends).
pub fn split(text: &[u8]) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = text.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

pub fn join(lines: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for l in lines {
        out.extend_from_slice(l);
        if l.as_slice() != b"\x1a" {
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}
