//! SYSLEVEL: list the installed components and their service levels from
//! their SYSLEVEL.xxx files. SYSLEVEL [directory] [/C] [/D]
//!
//! Without a directory every local hard disk is searched. A SYSLEVEL file
//! starts with `FF FF "SYSLEVEL"`, a version word at 0x0F and the offset of
//! its table at 0x21; the table holds the system id, edition, version
//! (major/minor nibbles), modification level, date, the current and
//! previous CSD levels (8 characters each), the product name (80), the
//! component id (9), the refresh level and the type (9).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::*;
use utils::files::*;
use utils::*;
os2::entry!(main);

fn field(d: &[u8], at: usize, n: usize) -> String {
    d.get(at..at + n).unwrap_or(&[]).iter().take_while(|&&b| b != 0).map(|&b| b as char).collect::<String>().trim_end_matches('_').trim().into()
}

fn show(path: &str) -> bool {
    let Ok(d) = read_file(path) else { return false };
    if d.len() < 0x25 || &d[0..10] != b"\xff\xffSYSLEVEL" {
        return false;
    }
    let t = u32::from_le_bytes([d[0x21], d[0x22], d[0x23], d[0x24]]) as usize;
    if t + 0x71 > d.len() {
        return false;
    }
    let ver = d[t + 3];
    let modify = d[t + 4];
    let version = alloc::format!("{}.{}{}", ver >> 4, ver & 15, modify);
    let csd = field(&d, t + 7, 8);
    let prev = field(&d, t + 15, 8);
    let name = field(&d, t + 23, 80);
    let comp = field(&d, t + 103, 9);
    let ty = field(&d, t + 113, 9);
    outln(&full_path(path));
    // The layout of IBM's SYSLEVEL report (its texts are in SYSLEVEL.MSG,
    // not in OSO001.MSG).
    out(&alloc::format!("{name}\r\nVersion {version}     "));
    outln(&alloc::format!("Component ID {comp}"));
    if !ty.is_empty() {
        outln(&alloc::format!("Type {ty}"));
    }
    outln(&alloc::format!("Current CSD level: {csd}\r\nPrior   CSD level: {prev}\r\n"));
    true
}

fn search(dir: &str, found: &mut usize) {
    for e in find_all(&join(dir, "SYSLEVEL.*"), FILE_HIDDEN | FILE_SYSTEM | FILE_READONLY | FILE_ARCHIVED) {
        if e.attr & FILE_DIRECTORY == 0 && show(&join(dir, &e.name)) {
            *found += 1;
        }
    }
    for e in find_all(&join(dir, "*"), FILE_DIRECTORY | FILE_HIDDEN | FILE_SYSTEM) {
        if e.attr & FILE_DIRECTORY != 0 {
            search(&join(dir, &e.name), found);
        }
    }
}

fn main(s: &os2::Startup) -> u32 {
    let mut dirs: Vec<String> = Vec::new();
    for a in args(s) {
        match a.to_ascii_uppercase().as_str() {
            "/C" | "/D" => {}
            "/?" => {
                outln("SYSLEVEL lists installed components and their service levels.\r\n");
                outln("Syntax:  SYSLEVEL [directory] [/C] [/D]\r\n");
                outln("Without a directory, every local hard disk is searched for SYSLEVEL files.");
                return 0;
            }
            _ if a.starts_with('/') => {
                outln("SYSLEVEL: incorrect parameter.");
                return 1;
            }
            _ => {
                if !is_dir(&a) {
                    outln("The specified directory does not exist.");
                    return 1;
                }
                dirs.push(a);
            }
        }
    }
    out("Please wait...\r\n");
    if dirs.is_empty() {
        let (mut cur, mut map) = (0u32, 0u32);
        unsafe { DosQueryCurrentDisk(&mut cur, &mut map) };
        for d in 2..26u8 {
            if map & (1 << d) == 0 {
                continue;
            }
            // Hard disks only: removable media and CDs are skipped.
            if disk::fs_name(d).is_none_or(|f| f == "CDFS" || f == "UDF" || f == "LAN" || f == "NFS") {
                continue;
            }
            dirs.push(alloc::format!("{}:\\", (b'A' + d) as char));
        }
    }
    let mut found = 0;
    for d in &dirs {
        search(d, &mut found);
    }
    if found == 0 {
        outln("No SYSLEVEL files were found.");
    }
    0
}
