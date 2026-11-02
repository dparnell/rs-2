//! UNDELETE: bring back deleted files kept in the drive's DELDIR
//! directory. UNDELETE [drive:][path][filename] [/L | /S] [/A] [/F]
//!
//! On OS/2 the file system keeps deleted files in the directories that
//! `SET DELDIR=d:\path,maxKB;...` names. rs2's file systems do not keep
//! them yet: UNDELETE checks DELDIR for the drive (SYS3194 when it has
//! none, as IBM's UNDELETE) and otherwise finds nothing to undelete
//! (SYS3195). /L lists, /S includes subdirectories, /A undeletes without
//! asking, /F purges.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use os2::*;
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

fn env(name: &str) -> Option<String> {
    let c = os2::path::cstr(name);
    let mut p: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(c.as_ptr(), &mut p) } != 0 || p.is_null() {
        return None;
    }
    let mut s = String::new();
    unsafe {
        while *p != 0 {
            s.push(*p as char);
            p = p.add(1);
        }
    }
    Some(s)
}

fn main(s: &os2::Startup) -> u32 {
    let mut spec = None;
    for a in args(s) {
        match a.to_ascii_uppercase().as_str() {
            "/L" | "/S" | "/A" | "/F" => {}
            "/?" => {
                say(3193, &[], "UNDELETE brings back deleted files.\r\nSyntax:\r\n  UNDELETE [drive:][path][filename] [/L or /S][/A][/F]\r\n");
                return 0;
            }
            x if x.starts_with('/') => {
                err(3192, &[], "The path or parameter is not correct.\r\n");
                return 1;
            }
            _ => spec = Some(a),
        }
    }
    let target = utils::files::full_path(spec.as_deref().unwrap_or("*"));
    let drive = target.as_bytes().first().map_or(b'C', |b| b.to_ascii_uppercase());
    say(3205, &[&target], "Undeleting %1.\r\n");
    out("\r\n");
    // DELDIR=d:\path,size;d:\path,size
    let dir = env("DELDIR").and_then(|v| {
        v.split(';').map(|e| String::from(e.split(',').next().unwrap_or("").trim())).find(|p| p.as_bytes().first().is_some_and(|b| b.to_ascii_uppercase() == drive))
    });
    match dir {
        None => {
            err(3194, &[], "The DELDIR environment variable is missing or not correct\r\nfor the drive.\r\n");
            1
        }
        Some(_) => {
            say(3195, &[], "There are no deleted files to bring back on this drive.\r\n");
            1
        }
    }
}
