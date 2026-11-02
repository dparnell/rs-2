//! RECOVER: save what can still be read of a file on a disk with bad
//! sectors. RECOVER [drive:][path]filename
//!
//! The file is read in 512-byte pieces; pieces that cannot be read are
//! left as zeros. The readable data goes to a temporary file in the same
//! directory, which then replaces the original (keeping its attributes,
//! date and extended attributes). Recovering a whole drive (`RECOVER d:`,
//! which rebuilt FAT directories on DOS) is CHKDSK /F's job on OS/2 and
//! is refused.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec::Vec;
use os2::path::cstr;
use os2::*;
use utils::files::*;
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

fn one(path: &str) -> u32 {
    let Some(st) = status(path) else {
        err(594, &[], "RECOVER cannot find the file.\r\n");
        return 2;
    };
    let eas = read_eas(path).unwrap_or_default();
    let hf = match open_read(path) {
        Ok(h) => h,
        Err(rc) => {
            err(595, &[&alloc::format!("{rc}")], "Error %1 occurred; the file cannot be recovered.\r\n");
            return rc;
        }
    };
    let size = st.cb_file as usize;
    let mut data: Vec<u8> = alloc::vec![0u8; size];
    let (mut good, mut bad) = (0u64, 0u64);
    let mut at = 0usize;
    while at < size {
        let n = (size - at).min(512);
        let mut p = 0;
        let mut got = 0;
        let ok = unsafe { DosSetFilePtr(hf, at as i32, 0, &mut p) } == 0
            && unsafe { DosRead(hf, data[at..].as_mut_ptr(), n as u32, &mut got) } == 0
            && got as usize == n;
        if ok {
            good += n as u64;
        } else {
            data[at..at + n].fill(0);
            bad += n as u64;
        }
        at += n;
    }
    unsafe { DosClose(hf) };
    let tmp = alloc::format!("{}RECOVER.$$$", dir_of(path));
    if write_file(&tmp, &data).is_err() {
        err(597, &[], "RECOVER cannot write the recovered data to the disk.\r\n");
        return 1;
    }
    if !eas.is_empty() && write_eas(&tmp, &eas) != 0 {
        say(599, &[], "Warning!  RECOVER could not keep the extended attributes.\r\n");
    }
    set_info(path, 0, 0, 0);
    let (c, t) = (cstr(path), cstr(&tmp));
    if unsafe { DosDelete(c.as_ptr()) } != 0 {
        err(604, &[path, &tmp], "RECOVER cannot delete %1.\r\nIts recovered data is in %2.\r\n");
        return 1;
    }
    if unsafe { DosMove(t.as_ptr(), c.as_ptr()) } != 0 {
        err(605, &[path, &tmp], "RECOVER cannot create %1.\r\nIts recovered data is in %2.\r\n");
        return 1;
    }
    set_info(path, st.attr, st.fdate_write, st.ftime_write);
    say(600, &[&alloc::format!("{good}")], "%1 bytes of the file were recovered.\r\n");
    if bad > 0 {
        say(602, &[&alloc::format!("{bad}")], "%1 bytes of the file could not be recovered.\r\n");
    }
    0
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.len() != 1 || a[0].starts_with('/') {
        match a.iter().find(|x| x.starts_with('/')) {
            Some(x) => err(593, &[x], "%1 is not a correct parameter for RECOVER.\r\n"),
            None => err(1003, &[], "The command syntax is not correct.\r\n"),
        }
        return 1;
    }
    let spec = &a[0];
    if spec.len() == 2 && spec.ends_with(':') {
        eprint(&alloc::format!("RECOVER cannot rebuild a whole drive; CHKDSK {spec} /F repairs it.\r\n"));
        return 1;
    }
    let dir = dir_of(spec);
    let found: Vec<Found> = find_all(spec, FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED).into_iter().filter(|f| f.attr & FILE_DIRECTORY == 0).collect();
    if found.is_empty() {
        err(594, &[], "RECOVER cannot find the file.\r\n");
        return 2;
    }
    if stdin_is_console() {
        press_enter(631, &[], "Press Enter to start recovering the file.\r\n");
    }
    let mut rc = 0;
    for f in found {
        let r = one(&alloc::format!("{dir}{}", f.name));
        if r != 0 {
            rc = r;
        }
    }
    rc
}
