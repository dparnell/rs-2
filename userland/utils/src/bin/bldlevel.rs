//! BLDLEVEL: show the build level signature of a program, DLL or driver.
//! BLDLEVEL [drive:][path]filename
//!
//! The signature is the module description (the first entry of an LX or
//! NE module's non-resident name table), in the form
//! `@#vendor:revision#@description`, optionally extended as
//! `@#vendor:revision#@##1## date time machine:asd:language:country:build:fixpak@@description`.
//! Other files are searched for an `@#...#@` string.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use utils::files::read_file;
use utils::*;
os2::entry!(main);


fn u32at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}

/// The module description of an LX or NE file.
fn description(d: &[u8]) -> Option<Vec<u8>> {
    let hdr = if d.get(0..2)? == b"MZ" { u32at(d, 0x3c)? as usize } else { 0 };
    let sig = d.get(hdr..hdr + 2)?;
    let at = match sig {
        b"LX" | b"LE" => u32at(d, hdr + 0x88)? as usize,
        b"NE" => u32at(d, hdr + 0x2c)? as usize,
        _ => return None,
    };
    let n = *d.get(at)? as usize;
    d.get(at + 1..at + 1 + n).map(|s| s.to_vec())
}

/// An `@#...` signature anywhere in the data.
fn search(d: &[u8]) -> Option<Vec<u8>> {
    let i = d.windows(2).position(|w| w == b"@#")?;
    let end = d[i..].iter().position(|&b| b == 0 || b == b'\r' || b == b'\n').map_or(d.len(), |e| i + e);
    let s = &d[i..end];
    s.windows(2).any(|w| w == b"#@").then(|| s.to_vec())
}

fn line(label: &str, v: &str) {
    if !v.is_empty() {
        outln(&alloc::format!("{label:<17}{v}"));
    }
}

fn show(sig: &str) -> bool {
    let Some(rest) = sig.strip_prefix("@#") else { return false };
    let Some(end) = rest.find("#@") else { return false };
    let (vendor, revision) = rest[..end].split_once(':').unwrap_or((&rest[..end], ""));
    let mut desc = &rest[end + 2..];
    line("Signature:", sig);
    line("Vendor:", vendor);
    line("Revision:", revision);
    let mut ext: Vec<&str> = Vec::new();
    let mut when = "";
    if let Some(x) = desc.strip_prefix("##1##") {
        if let Some(e) = x.find("@@") {
            let body = &x[..e];
            desc = &x[e + 2..];
            // Date and time take the first 24 columns, then the machine
            // and the colon-separated fields.
            let (dt, more) = if body.len() > 25 { body.split_at(25) } else { (body, "") };
            when = dt.trim();
            ext = more.split(':').collect();
        }
    }
    line("Date/Time:", when);
    line("Build Machine:", ext.first().map_or("", |s| s.trim()));
    line("ASD Feature ID:", ext.get(1).map_or("", |s| s.trim()));
    line("Language Code:", ext.get(2).map_or("", |s| s.trim()));
    line("Country Code:", ext.get(3).map_or("", |s| s.trim()));
    line("Build:", ext.get(4).map_or("", |s| s.trim()));
    line("FixPak Version:", ext.get(5).map_or("", |s| s.trim()));
    line("File Version:", revision);
    line("Description:", desc.trim());
    true
}

fn usage() {
    outln("BLDLEVEL shows the build level (version) signature of a file.\r\n");
    outln("Syntax:  BLDLEVEL [drive:][path]filename\r\n");
    outln("A signature has the form @#vendor:revision#@description; programs,");
    outln("DLLs and drivers carry it as their module description.");
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let Some(name) = a.first().filter(|n| *n != "/?") else {
        usage();
        return 0;
    };
    let data = match read_file(name) {
        Ok(d) => d,
        Err(rc) => {
            error(rc);
            return rc;
        }
    };
    let sig = description(&data).filter(|d| d.starts_with(b"@#")).or_else(|| search(&data));
    let text: Option<String> = sig.map(|v| v.iter().map(|&b| b as char).collect());
    if !text.as_deref().is_some_and(show) {
        outln(&alloc::format!("{name} has no build level signature."));
        return 1;
    }
    0
}
