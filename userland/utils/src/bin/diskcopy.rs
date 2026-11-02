//! DISKCOPY: copy a whole diskette. DISKCOPY [source [target]]
//!
//! Source and target are diskette drives (`A:`) or, in rs2, which has no
//! diskette drives, also diskette image files: `DISKCOPY A: DISK1.IMG`
//! makes an image, `DISKCOPY DISK1.IMG A:` writes one, and a drive here
//! is any volume of diskette size (an unpartitioned 1.44 MB disk). Hard
//! disks are refused, as by OS/2's DISKCOPY.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use utils::files::{press_enter, yes_no};
use utils::media::{geometry, Medium};
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

fn one(src: &str, dst: &str) -> u32 {
    let s = match Medium::open(src, false) {
        Ok(m) => m,
        Err(1231) => {
            err(1231, &[], "The drive given cannot be used: it is a hard disk.\r\n");
            return 1;
        }
        Err(rc) => {
            error(rc);
            return 1;
        }
    };
    if s.is_drive() {
        press_enter(1232, &[src], "Insert the source diskette in drive %1\r\n\r\n");
    }
    let data = match s.read() {
        Ok(d) => d,
        Err(_) => {
            err(1261, &[], "The source diskette could not be read.\r\n");
            return 1;
        }
    };
    let (t, spt, sides) = geometry(&data);
    say(1260, &[&alloc::format!("{t}"), &alloc::format!("{spt}"), &alloc::format!("{sides}")], "Copying %1 tracks %2 sectors per track, %3 side(s).\r\n");
    let d = match Medium::open(dst, true) {
        Ok(m) => m,
        Err(1231) => {
            err(1231, &[], "The drive given cannot be used: it is a hard disk.\r\n");
            return 1;
        }
        Err(rc) => {
            error(rc);
            return 1;
        }
    };
    if d.is_drive() && s.is_drive() && src.eq_ignore_ascii_case(dst) {
        press_enter(1233, &[dst], "Insert the target diskette in drive %1\r\n\r\n");
    }
    match d.write(&data) {
        Ok(()) => {
            say(1254, &["100"], "%1 percent of the disk has been copied.\r\n");
            0
        }
        Err(1240) => {
            err(1240, &[], "The drive or diskette types do not match.\r\n");
            1
        }
        Err(_) => {
            err(1262, &[], "The target diskette could not be written;\r\nits contents may be damaged.\r\n");
            say(1255, &[], "The target diskette may not be usable.\r\n");
            1
        }
    }
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.iter().any(|x| x == "/?") || a.len() > 2 {
        say(3096, &[], "DISKCOPY copies one diskette to another.\r\nSyntax:\r\n  DISKCOPY [source drive] [target drive]\r\n\r\nIn rs2 either may also be a diskette image file.\r\n");
        return if a.len() > 2 { 1 } else { 0 };
    }
    let cur = drive_arg(None).map_or(String::from("A:"), |d| alloc::format!("{}:", (b'A' + d) as char));
    let src = a.first().cloned().unwrap_or_else(|| String::from("A:"));
    let dst = a.get(1).cloned().unwrap_or(cur);
    let mut rc;
    loop {
        rc = one(&src, &dst);
        say(1266, &[], "Copy has ended.\r\n");
        let both_files = !src.ends_with(':') && !dst.ends_with(':');
        if both_files || !yes_no(1259, &[], "Copy another diskette (Y/N)? ") {
            break;
        }
        out("\r\n");
    }
    rc
}
