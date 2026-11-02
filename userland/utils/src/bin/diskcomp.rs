//! DISKCOMP: compare two whole diskettes track by track.
//! DISKCOMP [first [second]]
//!
//! Each may be a diskette drive or, in rs2, a diskette image file (see
//! DISKCOPY). Differing tracks are reported by side and track; at most
//! 30 of them. Return code 0 identical, 1 different, 2 error.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use utils::files::{press_enter, yes_no};
use utils::media::{geometry, Medium};
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

fn load(arg: &str, n: u32) -> Result<alloc::vec::Vec<u8>, u32> {
    let m = match Medium::open(arg, false) {
        Ok(m) => m,
        Err(1231) => {
            err(1231, &[], "The drive given cannot be used: it is a hard disk.\r\n");
            return Err(2);
        }
        Err(rc) => {
            error(rc);
            return Err(2);
        }
    };
    if m.is_drive() {
        if n == 1 {
            press_enter(1232, &[arg], "Insert the first diskette in drive %1\r\n\r\n");
        } else {
            press_enter(1233, &[arg], "Insert the second diskette in drive %1\r\n\r\n");
        }
    }
    m.read().map_err(|_| {
        if n == 1 {
            err(1261, &[], "The first diskette could not be read.\r\n");
        } else {
            err(1263, &[], "The second diskette could not be read.\r\n");
        }
        2
    })
}

fn one(a: &str, b: &str) -> u32 {
    let x = match load(a, 1) {
        Ok(d) => d,
        Err(rc) => return rc,
    };
    let (t, spt, sides) = geometry(&x);
    say(1239, &[&alloc::format!("{t}"), &alloc::format!("{spt}"), &alloc::format!("{sides}")], "Comparing %1 tracks\r\n%2 sectors per track, %3 side(s).\r\n");
    let y = match load(b, 2) {
        Ok(d) => d,
        Err(rc) => return rc,
    };
    if x.len() != y.len() {
        err(1240, &[], "The drive or diskette types do not match.\r\n");
        return 2;
    }
    let track = (spt * 512) as usize;
    let mut bad = 0;
    for (i, (p, q)) in x.chunks(track).zip(y.chunks(track)).enumerate() {
        if p != q {
            bad += 1;
            let (cyl, side) = (i as u32 / sides.max(1), i as u32 % sides.max(1));
            say(1242, &[&alloc::format!("{side}"), &alloc::format!("{cyl}")], "The diskettes differ on side %1 track %2.\r\n");
            if bad >= 30 {
                break;
            }
        }
    }
    if bad == 0 {
        say(1245, &[], "The diskettes are the same.\r\n");
        0
    } else {
        1
    }
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.iter().any(|x| x == "/?") || a.len() > 2 {
        say(3095, &[], "DISKCOMP compares two diskettes.\r\nSyntax:\r\n  DISKCOMP [first drive] [second drive]\r\n\r\nIn rs2 either may also be a diskette image file.\r\n");
        return if a.len() > 2 { 2 } else { 0 };
    }
    let cur = drive_arg(None).map_or(String::from("A:"), |d| alloc::format!("{}:", (b'A' + d) as char));
    let first = a.first().cloned().unwrap_or_else(|| String::from("A:"));
    let second = a.get(1).cloned().unwrap_or(cur);
    let mut rc;
    loop {
        rc = one(&first, &second);
        say(1244, &[], "The comparison has ended.\r\n");
        let both_files = !first.ends_with(':') && !second.ends_with(':');
        if both_files || !yes_no(1238, &[], "Compare other diskettes (Y/N)? ") {
            break;
        }
        out("\r\n");
    }
    rc
}
