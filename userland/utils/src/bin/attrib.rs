//! ATTRIB: show or change file attributes. ATTRIB [+R|-R] [+A|-A] [+S|-S] [+H|-H] files
#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec::Vec;
use os2::*;
use utils::*;
os2::entry!(main);
fn main(s: &os2::Startup) -> u32 {
    let (mut set, mut clear) = (0u32, 0u32);
    let mut specs = Vec::new();
    for a in args(s) {
        let bit = |c: char| match c.to_ascii_uppercase() {
            'R' => FILE_READONLY,
            'A' => FILE_ARCHIVED,
            'S' => FILE_SYSTEM,
            'H' => FILE_HIDDEN,
            _ => 0,
        };
        if let Some(r) = a.strip_prefix('+') {
            set |= r.chars().map(bit).fold(0, |x, y| x | y);
        } else if let Some(r) = a.strip_prefix('-') {
            clear |= r.chars().map(bit).fold(0, |x, y| x | y);
        } else if !a.starts_with('/') {
            specs.push(a);
        }
    }
    if specs.is_empty() {
        specs.push(alloc::string::String::from("*"));
    }
    let mut rc = 0;
    for spec in specs {
        let dir = dir_of(&spec);
        let found = find(&spec, FILE_HIDDEN | FILE_SYSTEM);
        if found.is_empty() {
            eprint(&alloc::format!("SYS0002: File not found - {spec}\r\n"));
            rc = 2;
        }
        for e in found {
            let path = alloc::format!("{dir}{}", e.name);
            if set | clear != 0 {
                let mut fs = FileStatus3::default();
                let c = os2::path::cstr(&path);
                unsafe {
                    DosQueryPathInfo(c.as_ptr(), FIL_STANDARD, &mut fs as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32);
                    fs.attr = (fs.attr | set) & !clear & 0x27;
                    // Leave dates unchanged.
                    fs.fdate_creation = 0;
                    fs.ftime_creation = 0;
                    fs.fdate_access = 0;
                    fs.ftime_access = 0;
                    fs.fdate_write = 0;
                    fs.ftime_write = 0;
                    let r = DosSetPathInfo(c.as_ptr(), FIL_STANDARD, &fs as *const _ as *const u8, core::mem::size_of::<FileStatus3>() as u32, 0);
                    if r != 0 {
                        rc = r;
                    }
                }
            } else {
                let f = |b: u32, c: char| if e.attr & b != 0 { c } else { ' ' };
                outln(&alloc::format!(
                    "{}  {}{}{}   {}",
                    f(FILE_ARCHIVED, 'A'),
                    f(FILE_SYSTEM, 'S'),
                    f(FILE_HIDDEN, 'H'),
                    f(FILE_READONLY, 'R'),
                    path
                ));
            }
        }
    }
    rc
}
