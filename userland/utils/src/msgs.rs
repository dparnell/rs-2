//! Numbered messages: the text comes from OSO001.MSG (DosGetMessage, which
//! puts `SYSnnnn: ` before error and warning messages and substitutes the
//! %1..%9 parameters); without the file, from the built-in English text
//! the caller passes, treated the same way.

use alloc::string::String;
use alloc::vec::Vec;

use os2::*;

/// Message classes, as in the message file.
#[derive(Clone, Copy, PartialEq)]
pub enum Class {
    /// Information: no prefix.
    Info,
    /// Error or warning: `SYSnnnn: ` prefix.
    Error,
}

fn subst(text: &str, subs: &[&str]) -> String {
    let mut out = String::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '%' {
            if let Some(&d) = it.peek() {
                if let Some(i) = d.to_digit(10).filter(|&i| i >= 1) {
                    it.next();
                    out.push_str(subs.get(i as usize - 1).copied().unwrap_or(""));
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// Message `n` with its parameters. `fallback` is used when OSO001.MSG
/// does not have the message; it carries its own line ends.
pub fn text(n: u32, subs: &[&str], class: Class, fallback: &str) -> String {
    let cs: Vec<Vec<u8>> = subs
        .iter()
        .map(|s| {
            let mut v: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
            v.push(0);
            v
        })
        .collect();
    let ptrs: Vec<*const u8> = cs.iter().map(|v| v.as_ptr()).collect();
    let mut buf = alloc::vec![0u8; 4096];
    let mut cb = 0;
    let file = b"OSO001.MSG\0";
    let rc = unsafe {
        DosGetMessage(ptrs.as_ptr(), ptrs.len() as u32, buf.as_mut_ptr(), buf.len() as u32, n, file.as_ptr(), &mut cb)
    };
    // 317/318: no such message or no file (DosGetMessage then returns its
    // own text about that, which is not what the program wants to say).
    if rc == 0 {
        return buf[..cb as usize].iter().map(|&b| b as char).collect();
    }
    let body = subst(fallback, subs);
    match class {
        Class::Info => body,
        Class::Error => alloc::format!("SYS{n:04}: {body}"),
    }
}

/// An information message on standard output.
pub fn say(n: u32, subs: &[&str], fallback: &str) {
    os2::print(&text(n, subs, Class::Info, fallback));
}

/// An error or warning message on standard error.
pub fn err(n: u32, subs: &[&str], fallback: &str) {
    os2::write_text(2, &text(n, subs, Class::Error, fallback));
}

/// An error or warning message on standard output (as IBM's utilities
/// show some of them).
pub fn warn(n: u32, subs: &[&str], fallback: &str) {
    os2::print(&text(n, subs, Class::Error, fallback));
}

/// A Y/N question (message `n`); true for yes. End of redirected input
/// answers no.
pub fn ask_yes(n: u32, subs: &[&str], fallback: &str) -> bool {
    let q = text(n, subs, Class::Info, fallback);
    crate::yes(&q)
}

/// A number with thousands separators, as the OS/2 utilities show sizes
/// (the separator from the country information).
pub fn grouped(v: u64) -> String {
    let sep = thousands_sep();
    let s = alloc::format!("{v}");
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

fn country() -> CountryInfo {
    let mut ci = CountryInfo::default();
    let cc = CountryCode::default();
    let mut cb = 0u32;
    unsafe { DosQueryCtryInfo(core::mem::size_of::<CountryInfo>() as u32, &cc, &mut ci, &mut cb) };
    ci
}

fn thousands_sep() -> char {
    let ci = country();
    if ci.thousands[0] != 0 { ci.thousands[0] as char } else { ',' }
}

/// A date in the country's format.
pub fn date(y: u32, m: u32, d: u32) -> String {
    let ci = country();
    let sep = if ci.date_sep[0] != 0 { ci.date_sep[0] as char } else { '-' };
    let y2 = y % 100;
    match ci.date_fmt {
        1 => alloc::format!("{d:02}{sep}{m:02}{sep}{y2:02}"),
        2 => alloc::format!("{y2:02}{sep}{m:02}{sep}{d:02}"),
        _ => alloc::format!("{m:02}{sep}{d:02}{sep}{y2:02}"),
    }
}

/// A time in the country's format (seconds optional).
pub fn time(h: u32, mi: u32, s: Option<u32>) -> String {
    let ci = country();
    let sep = if ci.time_sep[0] != 0 { ci.time_sep[0] as char } else { ':' };
    let (hh, ap) = if ci.time_fmt & 1 == 0 {
        (if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "a" } else { "p" })
    } else {
        (h, "")
    };
    match s {
        Some(s) => alloc::format!("{hh:2}{sep}{mi:02}{sep}{s:02}{ap}"),
        None => alloc::format!("{hh:2}{sep}{mi:02}{ap}"),
    }
}
