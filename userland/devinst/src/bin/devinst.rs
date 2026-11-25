//! INSTALL.EXE of the developer disk, text (docs/DEVDISK.md).
//!
//! ```text
//! INSTALL [/DIR:path | /DRIVE:d] [/SOURCE | /NOSOURCE] [/YES] [/LIST]
//! ```

#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};

use devinst::*;
use os2::Startup;
use utils::{args, ask, out, outln, yes};

os2::entry!(main);

struct Text {
    percent: u64,
}

impl Progress for Text {
    fn at(&mut self, _file: &str, done: u64, total: u64) -> bool {
        let p = done * 100 / total.max(1);
        if p / 5 != self.percent / 5 {
            out(&format!(" {}%", p / 5 * 5));
        }
        self.percent = p;
        true
    }
}

fn usage() -> u32 {
    outln("Installs rs2's compilers (Rust, C and C++) and sources from this disk.");
    outln("");
    outln("INSTALL [/DIR:path | /DRIVE:d] [/SOURCE | /NOSOURCE] [/YES] [/LIST]");
    outln("");
    outln("  /DIR:path    where they go (C:\\RS2DEV), on an HPFS or JFS drive");
    outln("  /DRIVE:d     the drive: \\RS2DEV there");
    outln("  /SOURCE      rs2's sources too, to build rs2 itself");
    outln("  /NOSOURCE    the compilers only");
    outln("  /YES         do not ask (the compilers only, unless /SOURCE)");
    outln("  /LIST        list the drives they can go on");
    87
}

fn run(s: &Startup) -> Result<u32, String> {
    let (mut dir, mut source, mut sure, mut list) = (None, None, false, false);
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        match u.split_once(':') {
            Some(("/DIR", _)) => dir = Some(a[5..].to_string()),
            Some(("/DRIVE", d)) if !d.is_empty() => dir = Some(format!("{}:\\{DIR}", &d[..1])),
            None if u == "/SOURCE" => source = Some(true),
            None if u == "/NOSOURCE" => source = Some(false),
            None if u == "/YES" || u == "/Y" => sure = true,
            None if u == "/LIST" => list = true,
            _ => return Ok(usage()),
        }
    }
    let disc = disc(s)?;
    if source == Some(true) && disc.source.is_none() {
        return Err(format!("There is no {SOURCE} on this disk."));
    }
    let drives = drives();
    if drives.is_empty() {
        return Err(NO_DRIVE.to_string());
    }
    outln(&format!("The compilers take {} MB.", disc.needs_mb(false)));
    if disc.source.is_some() {
        outln(&format!("rs2's sources (optional) take {} MB more, and {BUILD_MB} MB to build.", disc.needs_mb(true) - disc.needs_mb(false)));
    }
    outln("Drives they can go on:");
    for d in &drives {
        outln(&format!("  {}:  {:4}  {} MB free", d.letter, d.fs, d.free_mb));
    }
    if list {
        return Ok(0);
    }
    let source = match source {
        Some(s) => s,
        None if sure || disc.source.is_none() => false,
        None => yes("Install rs2's sources too (Y/N)? "),
    };
    let needs = disc.needs_mb(source) + if source { BUILD_MB } else { 0 };
    // The boot drive if it will do, else the one with the most room.
    let best = drives.iter().max_by_key(|d| (d.letter == boot_drive() && d.free_mb > needs, d.free_mb)).unwrap();
    let default = format!("{}:\\{DIR}", best.letter);
    let dir = match dir {
        Some(d) => d,
        None if sure => default,
        None => {
            let a = ask(&format!("Directory [{default}]: "));
            if a.is_empty() { default } else { a }
        }
    };
    let dir = check_dir(&dir)?;
    if !sure && !yes(&format!("Install into {dir} (Y/N)? ")) {
        return Ok(1);
    }
    out(&format!("Unpacking into {dir}:"));
    let notes = install(&disc, &dir, source, &mut Text { percent: 0 })?;
    outln(" 100%");
    for n in notes {
        outln(&n);
    }
    outln(&format!("The developer disk is installed. Read {dir}\\README.TXT."));
    Ok(0)
}

fn main(s: &Startup) -> u32 {
    match run(s) {
        Ok(rc) => rc,
        Err(e) => {
            outln("");
            outln(&format!("INSTALL: {e}"));
            1
        }
    }
}
