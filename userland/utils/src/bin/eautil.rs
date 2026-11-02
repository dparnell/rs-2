//! EAUTIL: split a file's extended attributes off into a hold file, or
//! join them back. EAUTIL datafile [holdfile] [/S | /J] [/R] [/O] [/M] [/P]
//!
//! The hold file holds the EAs as a 16-bit FEALIST (`u32 cbList`, then
//! `fEA, cbName, u16 cbValue, name, 0, value` per EA), as IBM's EAUTIL
//! writes it; without a hold file name it is EAS\name in the data file's
//! directory. /S (the default) moves the EAs into the hold file (/R
//! replaces an existing hold file, /P keeps them on the data file too);
//! /J attaches the hold file's EAs to the data file (/O replaces the data
//! file's EAs, /M merges them, the hold file's winning; /P keeps the hold
//! file). Wildcards in the data file name split or join each match.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::path::cstr;
use os2::*;
use os2_backup::{fealist, parse_fealist, Ea};
use utils::files::*;
use utils::msgs::err;
use utils::*;
os2::entry!(main);

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Split,
    Join,
}

/// Remove every EA of a file: set each name with an empty value.
fn clear_eas(path: &str, eas: &[Ea]) -> u32 {
    let empty: Vec<Ea> = eas.iter().map(|e| Ea { flags: 0, name: e.name.clone(), value: Vec::new() }).collect();
    write_eas(path, &empty)
}

fn one(data: &str, hold: &str, mode: Mode, replace: bool, overwrite: bool, merge: bool, preserve: bool) -> u32 {
    match mode {
        Mode::Split => {
            let eas = match read_eas(data) {
                Ok(e) => e,
                Err(rc) => {
                    error(rc);
                    return rc;
                }
            };
            if eas.is_empty() {
                return 0;
            }
            if exists(hold) && !replace {
                err(80, &[], "The file exists.\r\n");
                return 80;
            }
            let d = dir_of(hold);
            if !d.is_empty() {
                make_dirs(d.trim_end_matches('\\'));
            }
            if let Err(rc) = write_file(hold, &fealist(&eas)) {
                error(rc);
                return rc;
            }
            if !preserve {
                let rc = clear_eas(data, &eas);
                if rc != 0 {
                    error(rc);
                    return rc;
                }
            }
            0
        }
        Mode::Join => {
            let held = match read_file(hold) {
                Ok(d) => d,
                Err(rc) => {
                    error(rc);
                    return rc;
                }
            };
            let Ok(new) = parse_fealist(&held) else {
                err(276, &[], "The extended attributes are damaged.\r\n");
                return 276;
            };
            let old = read_eas(data).unwrap_or_default();
            if !old.is_empty() && !overwrite && !merge {
                err(1003, &[], "The command syntax is not correct.\r\n");
                return 1003;
            }
            let rc = if overwrite {
                // Only the hold file's EAs remain.
                let gone: Vec<Ea> = old.iter().filter(|o| !new.iter().any(|n| n.name.eq_ignore_ascii_case(&o.name))).cloned().collect();
                let rc = clear_eas(data, &gone);
                if rc != 0 { rc } else { write_eas(data, &new) }
            } else {
                write_eas(data, &new)
            };
            if rc != 0 {
                error(rc);
                return rc;
            }
            if !preserve {
                let c = cstr(hold);
                unsafe { DosDelete(c.as_ptr()) };
            }
            0
        }
    }
}

fn main(s: &os2::Startup) -> u32 {
    let mut mode = None;
    let (mut replace, mut overwrite, mut merge, mut preserve) = (false, false, false, false);
    let mut names: Vec<String> = Vec::new();
    for a in args(s) {
        match a.to_ascii_uppercase().as_str() {
            "/S" if mode != Some(Mode::Join) => mode = Some(Mode::Split),
            "/J" if mode != Some(Mode::Split) => mode = Some(Mode::Join),
            "/R" => replace = true,
            "/O" => overwrite = true,
            "/M" => merge = true,
            "/P" => preserve = true,
            x if x.starts_with('/') => {
                err(1003, &[], "The command syntax is not correct.\r\n");
                return 1003;
            }
            _ => names.push(a),
        }
    }
    let mode = mode.unwrap_or(Mode::Split);
    if names.is_empty() || names.len() > 2 || (overwrite && merge) || (mode == Mode::Split && (overwrite || merge)) || (mode == Mode::Join && replace) {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 1003;
    }
    let dir = dir_of(&names[0]);
    let matches: Vec<String> = if names[0].contains(['*', '?']) {
        find_all(&names[0], FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED | FILE_DIRECTORY).into_iter().map(|f| alloc::format!("{dir}{}", f.name)).collect()
    } else if exists(&names[0]) {
        alloc::vec![names[0].clone()]
    } else {
        Vec::new()
    };
    if matches.is_empty() {
        error(2);
        return 2;
    }
    let mut rc = 0;
    for m in &matches {
        let name = base_name(m);
        let hold = match names.get(1) {
            Some(h) if matches.len() > 1 || is_dir(h) => join(h, name),
            Some(h) => h.clone(),
            None => alloc::format!("{}EAS\\{name}", dir_of(m)),
        };
        let r = one(m, &hold, mode, replace, overwrite, merge, preserve);
        if r != 0 {
            rc = r;
        }
    }
    rc
}
