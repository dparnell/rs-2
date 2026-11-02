//! PATCH: apply fixes to a file. PATCH [drive:][path]filename [/A]
//!
//! Interactive: asks for hexadecimal offsets, shows the bytes there and
//! takes replacement bytes (hex, Enter keeps a byte, `.` ends the line);
//! at the end it asks whether to apply what was entered.
//!
//! /A: `filename` is a patch control file applied without questions. Its
//! lines (`;` starts a comment) are
//!
//! ```text
//! FILE name              the file the following lines patch
//! VER  offset hexbytes   the bytes there must be these
//! CHA  offset hexbytes   replace the bytes there
//! ```
//!
//! Offsets and bytes are hexadecimal; bytes may be separated by blanks.
//! Nothing is written to a file unless all its VER lines match.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use utils::files::*;
use utils::msgs::{err, say, text, Class};
use utils::*;
os2::entry!(main);

fn hex(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    u32::from_str_radix(s.trim_end_matches(['h', 'H']), 16).ok()
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let digits: String = s.chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
    if digits.is_empty() || digits.len() % 2 != 0 {
        return None;
    }
    (0..digits.len()).step_by(2).map(|i| u8::from_str_radix(&digits[i..i + 2], 16).ok()).collect()
}

struct Job {
    file: String,
    verify: Vec<(u32, Vec<u8>)>,
    change: Vec<(u32, Vec<u8>)>,
}

fn apply(j: &Job) -> u32 {
    if j.change.is_empty() {
        err(1600, &[&j.file], "No changes were given for %1.\r\n");
        return 1;
    }
    let mut data = match read_file(&j.file) {
        Ok(d) => d,
        Err(rc) => {
            error(rc);
            return rc;
        }
    };
    let end = alloc::format!("{:X}", data.len());
    for (off, bytes) in &j.verify {
        let o = *off as usize;
        if o + bytes.len() > data.len() {
            err(1594, &[&alloc::format!("{off:X}"), "VER", &end, &alloc::format!("{}", bytes.len())], "Offset %1 for %2 is too near or past the end\r\nof the file (offset %3) to check %4 bytes.\r\n");
            return 1;
        }
        if &data[o..o + bytes.len()] != bytes.as_slice() {
            err(1595, &[&j.file], "%1 does not hold the expected bytes; it was not changed.\r\n");
            return 1;
        }
    }
    for (off, bytes) in &j.change {
        let o = *off as usize;
        if o + bytes.len() > data.len() {
            err(1597, &[&alloc::format!("{off:X}"), "CHA", &end], "Offset %1 for %2 is past the end of the file\r\n(offset %3).\r\n");
            return 1;
        }
        data[o..o + bytes.len()].copy_from_slice(bytes);
    }
    if let Err(rc) = rewrite(&j.file, &data) {
        error(rc);
        return rc;
    }
    say(1584, &[&j.file], "%1 was patched.\r\n");
    0
}

/// Write the data back over the file, keeping its attributes and EAs.
fn rewrite(path: &str, data: &[u8]) -> Result<(), u32> {
    use os2::*;
    let c = os2::path::cstr(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYREADWRITE, core::ptr::null_mut())
    };
    if rc != 0 {
        return Err(rc);
    }
    let r = write_all(hf, data);
    unsafe { DosClose(hf) };
    r
}

fn control_file(name: &str) -> u32 {
    let text = match read_file(name) {
        Ok(d) => lines(&d),
        Err(rc) => {
            error(rc);
            return rc;
        }
    };
    let mut jobs: Vec<Job> = Vec::new();
    for l in text {
        let l = l.split(';').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        let (cmd, rest) = l.split_once([' ', '\t']).unwrap_or((l, ""));
        let rest = rest.trim();
        match cmd.to_ascii_uppercase().as_str() {
            "FILE" => jobs.push(Job { file: String::from(rest), verify: Vec::new(), change: Vec::new() }),
            c @ ("VER" | "CHA") => {
                let Some(j) = jobs.last_mut() else {
                    err(1590, &[name], "The patch control file %1 does not\r\nname a file to patch.\r\n");
                    return 1;
                };
                let (o, b) = rest.split_once([' ', '\t']).unwrap_or((rest, ""));
                let Some(off) = hex(o) else {
                    err(1592, &[o, c], "%1 is not a correct offset for %2.\r\n");
                    return 1;
                };
                let Some(bytes) = hex_bytes(b) else {
                    err(1593, &[b.trim(), c], "%1 is not a correct hexadecimal string for %2.\r\n");
                    return 1;
                };
                if c == "VER" { j.verify.push((off, bytes)) } else { j.change.push((off, bytes)) }
            }
            _ => {
                err(1598, &[cmd], "The patch control file has an unknown\r\ncommand: %1\r\n");
                return 1;
            }
        }
    }
    if jobs.is_empty() {
        err(1590, &[name], "The patch control file %1 does not\r\nname a file to patch.\r\n");
        return 1;
    }
    let mut rc = 0;
    for j in &jobs {
        say(1579, &[&j.file], "Patching %1\r\n");
        let r = apply(j);
        if r != 0 {
            say(1585, &[&j.file], "%1 was not patched.\r\n");
            rc = r;
        }
    }
    rc
}

fn interactive(name: &str) -> u32 {
    let mut data = match read_file(name) {
        Ok(d) => d,
        Err(rc) => {
            error(rc);
            return rc;
        }
    };
    let orig = data.clone();
    say(1579, &[name], "Patching %1\r\n");
    say(1586, &[&alloc::format!("{:X}", data.len())], "The file ends at offset %1\r\n");
    loop {
        out(&text(1587, &[], Class::Info, "Offset of the patch (hexadecimal): "));
        let line = ask("");
        let Some(mut off) = hex(&line) else { break };
        if off as usize >= data.len() {
            err(1588, &[], "The offset is past the end of the file.\r\n");
            continue;
        }
        // One line of 16 bytes at a time: the old byte, then the new one.
        'bytes: while (off as usize) < data.len() {
            out(&alloc::format!("{off:08X}"));
            let n = (16 - (off as usize % 16)).min(data.len() - off as usize);
            for k in 0..n {
                let o = off as usize + k;
                out(&alloc::format!(" {:02X}.", data[o]));
                let v = ask("");
                let v = v.trim();
                if v == "." {
                    break 'bytes;
                }
                if let Some(b) = hex_bytes(&alloc::format!("{v:0>2}")).filter(|_| !v.is_empty()) {
                    data[o] = b[0];
                }
            }
            off += n as u32;
        }
        out("\r\n");
        if !yes_no(1580, &[name], "Patch more of %1 (Y/N)? ") {
            out("\r\n");
            break;
        }
        out("\r\n");
    }
    if data == orig {
        say(1581, &[], "No patches were applied.\r\n");
        return 0;
    }
    say(1582, &[name], "Changes entered for %1\r\n");
    for (i, (a, b)) in orig.iter().zip(&data).enumerate() {
        if a != b {
            outln(&alloc::format!("{i:08X}  {a:02X} -> {b:02X}"));
        }
    }
    if !yes_no(1583, &[name], "Apply these changes to %1 (Y/N)? ") {
        out("\r\n");
        say(1585, &[name], "%1 was not patched.\r\n");
        return 0;
    }
    out("\r\n");
    if let Err(rc) = rewrite(name, &data) {
        error(rc);
        return rc;
    }
    say(1584, &[name], "%1 was patched.\r\n");
    0
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let auto = a.iter().any(|x| x.eq_ignore_ascii_case("/A"));
    let files: Vec<&String> = a.iter().filter(|x| !x.starts_with('/')).collect();
    if files.len() != 1 || a.iter().any(|x| x.starts_with('/') && !x.eq_ignore_ascii_case("/A")) {
        err(1575, &[], "The PATCH parameters are not correct.\r\n");
        return 1;
    }
    if auto { control_file(files[0]) } else { interactive(files[0]) }
}
