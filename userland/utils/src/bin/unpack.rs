//! UNPACK / UNPACK2: expand files packed by PACK/PACK2 (IBM's or rs2's).
//! UNPACK source [drive:][path] [/SHOW] [/N:name] [/V] [/F]
//!
//! Each entry of the packed file (crates/os2-pack) is written with its
//! date, attributes and extended attributes. A drive alone as the
//! destination keeps the path stored in the entry, on that drive; a path
//! replaces it; without a destination the files go to the current
//! directory. /SHOW lists the entries' stored paths, /N takes one file
//! out of a multi-file pack, /F skips files whose EAs the destination
//! cannot keep. Unpacked (plain) source files are copied.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::path::cstr;
use os2::*;
use utils::files::*;
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

fn text(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn main(s: &os2::Startup) -> u32 {
    let (mut show, mut fail_eas) = (false, false);
    let mut only: Option<String> = None;
    let mut names: Vec<String> = Vec::new();
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        if let Some(sw) = u.strip_prefix('/') {
            match sw {
                "SHOW" => show = true,
                "V" => {}
                "F" => fail_eas = true,
                _ if sw.starts_with("N:") => only = Some(String::from(&a[3..])),
                _ => {
                    err(1003, &[], "The command syntax is not correct.\r\n");
                    return 1;
                }
            }
        } else {
            names.push(a);
        }
    }
    if names.is_empty() || names.len() > 2 {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 1;
    }
    let src = &names[0];
    let dest = names.get(1).cloned();
    let dir = dir_of(src);
    let found: Vec<Found> = find_all(src, FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED).into_iter().filter(|f| f.attr & FILE_DIRECTORY == 0).collect();
    if found.is_empty() {
        err(2, &[], "The file cannot be found.\r\n");
        return 2;
    }
    let mut unpacked = 0u32;
    let mut copied = 0u32;
    let mut rc = 0;
    let mut dec = os2_pack::Decompressor::new();
    for f in &found {
        let path = alloc::format!("{dir}{}", f.name);
        let data = match read_file(&path) {
            Ok(d) => d,
            Err(e) => {
                error(e);
                rc = e;
                continue;
            }
        };
        if !os2_pack::is_packed(&data) {
            if show {
                continue;
            }
            // A plain file is copied as is.
            let to = match &dest {
                Some(d) if d.len() == 2 && d.ends_with(':') => alloc::format!("{d}{}", f.name),
                Some(d) => join(d, &f.name),
                None => f.name.clone(),
            };
            if full_path(&to).eq_ignore_ascii_case(&full_path(&path)) {
                err(1078, &[], "A file cannot be copied onto itself.\r\n");
                rc = 1;
                continue;
            }
            let (cf, ct) = (cstr(&path), cstr(&to));
            let r = unsafe { DosCopy(cf.as_ptr(), ct.as_ptr(), 1) };
            if r != 0 {
                error(r);
                rc = r;
            } else {
                copied += 1;
            }
            continue;
        }
        let entries = match os2_pack::entries(&data) {
            Ok(e) => e,
            Err(_) => {
                err(1217, &[], "This file cannot be expanded by this version of UNPACK.\r\n");
                rc = 1;
                continue;
            }
        };
        for e in &entries {
            let stored = text(e.path);
            let name = String::from(base_name(&stored));
            if let Some(o) = &only {
                if !o.eq_ignore_ascii_case(&name) && !o.eq_ignore_ascii_case(&stored) {
                    continue;
                }
            }
            if show {
                outln(&path);
                outln(&alloc::format!("->{stored}"));
                continue;
            }
            let stored_dir: String = String::from(stored[..stored.len() - name.len()].trim_end_matches('\\'));
            let stored_dir = match stored_dir.as_bytes() {
                [_, b':', ..] => String::from(&stored_dir[2..]),
                _ => stored_dir,
            };
            let to = match &dest {
                // A drive: the stored path on that drive.
                Some(d) if d.len() == 2 && d.ends_with(':') => {
                    let p = if stored_dir.starts_with('\\') || stored_dir.is_empty() { stored_dir.clone() } else { alloc::format!("\\{stored_dir}") };
                    alloc::format!("{d}{p}\\{name}")
                }
                Some(d) => join(d, &name),
                None => name.clone(),
            };
            let target_dir = dir_of(&to);
            if !target_dir.is_empty() {
                make_dirs(target_dir.trim_end_matches('\\'));
            }
            let body = match e.decompress(&mut dec) {
                Ok(b) => b,
                Err(_) => {
                    err(1217, &[], "This file cannot be expanded by this version of UNPACK.\r\n");
                    rc = 1;
                    continue;
                }
            };
            let eas = e.decompress_ea(&mut dec).ok().flatten();
            if let Some(st) = status(&to) {
                if st.attr & (FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM) != 0 {
                    set_info(&to, 0, 0, 0);
                }
            }
            if let Err(r) = write_file(&to, &body) {
                error(r);
                rc = r;
                continue;
            }
            if let Some(l) = eas {
                let list = os2_backup::parse_fea2list(&l);
                let r = write_eas(&to, &list);
                if r == 282 && fail_eas {
                    let c = cstr(&to);
                    unsafe { DosDelete(c.as_ptr()) };
                    error(r);
                    rc = r;
                    continue;
                }
            }
            set_info(&to, e.attributes, e.date, e.time);
            outln(&full_path(&to));
            unpacked += 1;
        }
    }
    if !show {
        if unpacked > 0 || copied == 0 {
            say(1216, &[&alloc::format!("{unpacked:>8}")], "%1 file(s) unpacked.\r\n");
        }
        if copied > 0 {
            say(1049, &[&alloc::format!("{copied:>8}")], "%1 file(s) copied.\r\n");
        }
    }
    rc
}
