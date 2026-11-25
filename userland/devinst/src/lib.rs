//! What the developer disk's two installers share (docs/DEVDISK.md): the
//! archives beside the program are unpacked into a directory on an HPFS or
//! JFS drive, and CONFIG.SYS gets the paths and settings.
//!
//! An archive's file NAME.rs2in is written as NAME with `@DIR@` replaced by
//! the directory (E:\RS2DEV), `@UDIR@` by it with slashes (E:/RS2DEV) and
//! `@DRIVE@` by its drive (E:).

#![no_std]

extern crate alloc;

pub mod config;
pub mod zip;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use os2::*;
use utils::files::{create, make_dirs, read_file, write_all, write_file};

/// The toolchain, and rs2's sources.
pub const SDK: &str = "SDK.ZIP";
pub const SOURCE: &str = "SOURCE.ZIP";
/// The directory's name unless another is chosen.
pub const DIR: &str = "RS2DEV";
/// Free space that has to be left (temporary files, first programs).
const SPARE_MB: u64 = 100;
/// What a build of rs2 takes besides.
pub const BUILD_MB: u64 = 2500;

/// A drive that can take it.
pub struct Drive {
    pub letter: char,
    pub fs: String,
    pub free_mb: u64,
}

/// The archives' files.
pub struct Disc {
    pub dir: String,
    pub sdk: zip::Archive,
    pub source: Option<zip::Archive>,
}

/// The running program's directory.
fn own_dir(s: &Startup) -> String {
    let mut b = [0u8; 260];
    unsafe { DosQueryModuleName(s.hmod, b.len() as u32, b.as_mut_ptr()) };
    let p = os2::path::from_bytes(&b[..b.iter().position(|&c| c == 0).unwrap_or(0)]);
    p[..p.rfind('\\').unwrap_or(0)].to_string()
}

pub fn disc(s: &Startup) -> Result<Disc, String> {
    let dir = own_dir(s);
    let sdk = zip::Archive::open(&format!("{dir}\\{SDK}")).map_err(|e| format!("{dir}\\{SDK}: {e}"))?;
    let src = format!("{dir}\\{SOURCE}");
    let source = if utils::files::exists(&src) { Some(zip::Archive::open(&src).map_err(|e| format!("{src}: {e}"))?) } else { None };
    Ok(Disc { dir, sdk, source })
}

impl Disc {
    /// Megabytes on the disk, with the sources or without.
    pub fn needs_mb(&self, source: bool) -> u64 {
        let mut n = self.sdk.disk_bytes();
        if let (true, Some(s)) = (source, &self.source) {
            n += s.disk_bytes();
        }
        (n >> 20) + 1
    }
}

pub fn free_mb(letter: char) -> u64 {
    let mut b = [0u8; 18];
    if unsafe { DosQueryFSInfo(letter as u32 - 'A' as u32 + 1, 1, b.as_mut_ptr(), b.len() as u32) } != 0 {
        return 0;
    }
    let w = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as u64;
    w(4) * w(12) * u16::from_le_bytes([b[16], b[17]]) as u64 >> 20
}

pub fn boot_drive() -> char {
    let mut d = 3u32;
    unsafe { DosQuerySysInfo(5, 5, &mut d, 4) };
    (b'A' + (d.clamp(1, 26) - 1) as u8) as char
}

/// The drives it can go on: the file names need HPFS or JFS.
pub fn drives() -> Vec<Drive> {
    let (mut cur, mut map) = (0, 0);
    unsafe {
        DosQueryCurrentDisk(&mut cur, &mut map);
        // No boxes about empty drives.
        DosError(2);
    }
    let mut v = Vec::new();
    for d in 2..26u8 {
        if map & 1 << d == 0 {
            continue;
        }
        let fs = utils::disk::fs_name(d).unwrap_or_default();
        if fs == "HPFS" || fs == "JFS" {
            let letter = (b'A' + d) as char;
            v.push(Drive { letter, fs, free_mb: free_mb(letter) });
        }
    }
    v
}

pub const NO_DRIVE: &str = "The compilers' file names need an HPFS or JFS drive, and there is none. \
Install rs2 with the system on HPFS or JFS, or make such a partition (LVM, FORMAT d: /FS:HPFS).";

/// `path` as E:\DIR, if the installation can go there.
pub fn check_dir(path: &str) -> Result<String, String> {
    let p = path.trim().replace('/', "\\");
    let p = p.trim_end_matches('\\');
    let b = p.as_bytes();
    if b.len() < 4 || !b[0].is_ascii_alphabetic() || &b[1..3] != b":\\" {
        return Err(format!("{path} is not a directory with its drive, such as C:\\{DIR}."));
    }
    // It goes into PATH, into cargo's files and C compilers' flags.
    if let Some(c) = p[3..].chars().find(|&c| !(c.is_ascii_alphanumeric() || "\\_-.".contains(c))) {
        return Err(format!("{p}: the directory's name cannot have \"{c}\" (programs take it apart there)."));
    }
    let letter = b[0].to_ascii_uppercase() as char;
    match drives().iter().find(|d| d.letter == letter) {
        Some(_) => Ok(format!("{letter}{}", &p[1..])),
        None if drives().is_empty() => Err(NO_DRIVE.to_string()),
        None => Err(format!("{letter}: is not an HPFS or JFS drive, which the compilers' file names need.")),
    }
}

/// What is being done, for who watches; `false` stops the installation.
pub trait Progress {
    fn at(&mut self, file: &str, done: u64, total: u64) -> bool;
}

/// Install into `dir` (of `check_dir`). The lines to show at the end.
pub fn install(disc: &Disc, dir: &str, source: bool, p: &mut dyn Progress) -> Result<Vec<String>, String> {
    let source = disc.source.as_ref().filter(|_| source);
    let needs = disc.needs_mb(source.is_some());
    let letter = dir.chars().next().unwrap_or('C');
    let free = free_mb(letter);
    // (Over an earlier installation most of it is there already.)
    let again = utils::files::exists(&format!("{dir}\\sdk\\bin\\RUSTC.EXE"));
    if !again && free < needs + SPARE_MB {
        return Err(format!("{letter}: has {free} MB free; {} MB are needed.", needs + SPARE_MB));
    }
    let total = disc.sdk.bytes() + source.map_or(0, |s| s.bytes());
    let mut done = 0;
    for a in [Some(&disc.sdk), source].into_iter().flatten() {
        unpack(a, dir, &mut done, total, p)?;
    }
    for d in ["tmp", "cargo"] {
        make_dirs(&format!("{dir}\\{d}"));
    }
    let boot = boot_drive();
    let mut notes = Vec::new();
    let file = format!("{boot}:\\CONFIG.SYS");
    let text = read_file(&file).map_err(|e| format!("{file} cannot be read (error {e})."))?;
    let new = config::edit(&text, dir);
    if new != text {
        let old = format!("{boot}:\\CONFIG.DEV");
        write_file(&old, &text).map_err(|e| format!("{old} cannot be written (error {e})."))?;
        write_file(&file, &new).map_err(|e| format!("{file} cannot be written (error {e})."))?;
        notes.push(format!("{file} has the new paths (the old one is {old})."));
        notes.push("Restart the computer for them to take effect everywhere.".to_string());
    }
    notes.push(format!("Without restarting, {dir}\\DEVENV.CMD sets up a command prompt."));
    Ok(notes)
}

fn template(data: &[u8], dir: &str) -> Vec<u8> {
    let unix = dir.replace('\\', "/");
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    'next: while i < data.len() {
        if data[i] == b'@' {
            for (k, v) in [("@DIR@", dir), ("@UDIR@", &unix[..]), ("@DRIVE@", &dir[..2])] {
                if data[i..].starts_with(k.as_bytes()) {
                    out.extend_from_slice(v.as_bytes());
                    i += k.len();
                    continue 'next;
                }
            }
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

fn unpack(a: &zip::Archive, dir: &str, done: &mut u64, total: u64, p: &mut dyn Progress) -> Result<(), String> {
    let mut made = String::new();
    for e in &a.entries {
        if !p.at(&e.name, *done, total) {
            return Err("The installation was stopped. What was unpacked is still there.".to_string());
        }
        let (name, is_template) = match e.name.strip_suffix(".rs2in") {
            Some(n) => (n, true),
            None => (&e.name[..], false),
        };
        let path = format!("{dir}\\{}", name.replace('/', "\\"));
        if name.ends_with('/') {
            make_dirs(path.trim_end_matches('\\'));
            continue;
        }
        let parent = &path[..path.rfind('\\').unwrap_or(0)];
        if parent != made {
            make_dirs(parent);
            made = parent.to_string();
        }
        let fail = |e: u32| format!("{path} cannot be written (error {e}).");
        let hf = create(&path, 0).map_err(fail)?;
        let mut text = Vec::new();
        let start = *done;
        let mut n = 0u64;
        let r = a.read(e, &mut |d| {
            if is_template {
                text.extend_from_slice(d);
                return Ok(());
            }
            write_all(hf, d).map_err(fail)?;
            n += d.len() as u64;
            // (Large files.)
            if n > 4 << 20 && !p.at(&e.name, start + n, total) {
                return Err("The installation was stopped. What was unpacked is still there.".to_string());
            }
            Ok(())
        });
        let r = r.map_err(|m| format!("{}: {m}", e.name));
        let r = r.and_then(|()| if is_template { write_all(hf, &template(&text, dir)).map_err(fail) } else { Ok(()) });
        unsafe { DosClose(hf) };
        r?;
        *done += e.size as u64;
        // cargo rebuilds what is newer than its result: the archive's times.
        utils::files::set_info(&path, 0, e.date, e.time);
    }
    Ok(())
}
