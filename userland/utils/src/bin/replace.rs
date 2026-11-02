//! REPLACE: replace files on the target with the source's, or add the
//! ones the target lacks.
//! REPLACE [drive:][path]filename [drive:][path] [/A] [/P] [/R] [/S] [/U] [/W] [/F]
//!
//! /A adds the files missing from the target (not with /S or /U); without
//! it the files the target has are replaced: in the target directory, or
//! with /S wherever they are below it. /P asks for each file, /R replaces
//! read-only files too, /U only files older than the source's, /W waits
//! for Enter first, /F fails a file whose extended attributes the target
//! cannot keep. Copies go through DosCopy, so EAs come along.
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

const DCPY_EXISTING: u32 = 1;
const DCPY_FAILEAS: u32 = 4;

fn stamp(f: &FileStatus3) -> u32 {
    (f.fdate_write as u32) << 16 | f.ftime_write as u32
}

fn main(s: &os2::Startup) -> u32 {
    let (mut add, mut prompt, mut ro, mut sub, mut update, mut wait, mut faileas) = (false, false, false, false, false, false, false);
    let mut names: Vec<String> = Vec::new();
    for a in args(s) {
        if let Some(sw) = a.strip_prefix('/') {
            match sw.to_ascii_uppercase().as_str() {
                "A" => add = true,
                "P" => prompt = true,
                "R" => ro = true,
                "S" => sub = true,
                "U" => update = true,
                "W" => wait = true,
                "F" => faileas = true,
                _ => {
                    err(1002, &[&a], "%1 is not a correct parameter for this command.\r\n");
                    return 1;
                }
            }
        } else {
            names.push(a);
        }
    }
    if names.is_empty() {
        err(1137, &[], "No source was given.\r\n");
        if add { say(1148, &[], "No files were added.\r\n") } else { say(1150, &[], "No files were replaced.\r\n") }
        return 11;
    }
    if names.len() > 2 || (add && (sub || update)) {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 11;
    }
    let source = names[0].clone();
    let target = names.get(1).cloned().unwrap_or_else(|| String::from("."));
    if !is_dir(&target) && !target.ends_with(':') {
        err(3, &[], "The path cannot be found.\r\n");
        return 3;
    }
    let src_dir = if is_dir(&source) { join(&source, "") } else { dir_of(&source) };
    let pattern = if is_dir(&source) { join(&source, "*") } else { source.clone() };
    let files: Vec<Found> = find_all(&pattern, FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED).into_iter().filter(|f| f.attr & FILE_DIRECTORY == 0).collect();
    if files.is_empty() {
        err(1141, &[&source], "No files match %1.\r\n");
        if add { say(1148, &[], "No files were added.\r\n") } else { say(1150, &[], "No files were replaced.\r\n") }
        return 2;
    }
    if wait {
        if add {
            press_enter(1139, &[], "Press Enter to start adding files.\r\n");
        } else {
            press_enter(1140, &[], "Press Enter to start replacing files.\r\n");
        }
    }
    // Target directories to look in.
    let mut dirs = alloc::vec![target.clone()];
    if sub {
        let mut all = Vec::new();
        walk(&target, "*", true, true, FILE_HIDDEN | FILE_SYSTEM, &mut all);
        for (rel, f) in all {
            if f.attr & FILE_DIRECTORY != 0 {
                dirs.push(join(&target, &rel));
            }
        }
    }
    let src_full = full_path(&src_dir);
    let mut count = 0u32;
    let mut rc = 0;
    for f in &files {
        let from = alloc::format!("{src_dir}{}", f.name);
        let places: Vec<String> = if add {
            let t = join(&target, &f.name);
            if exists(&t) { Vec::new() } else { alloc::vec![t] }
        } else {
            dirs.iter().map(|d| join(d, &f.name)).filter(|t| exists(t)).collect()
        };
        for to in places {
            if full_path(&dir_of(&to)).eq_ignore_ascii_case(&src_full) && full_path(&to).eq_ignore_ascii_case(&full_path(&from)) {
                err(1078, &[], "A file cannot be copied onto itself.\r\n");
                rc = 1;
                continue;
            }
            let shown = full_path(&to);
            if !add {
                if let (Some(sf), Some(tf)) = (status(&from), status(&to)) {
                    if update && stamp(&tf) >= stamp(&sf) {
                        continue;
                    }
                    if tf.attr & FILE_READONLY != 0 {
                        if !ro {
                            err(1145, &[&shown], "Access to %1 was denied.\r\n");
                            rc = 5;
                            continue;
                        }
                    }
                }
            }
            if prompt {
                let ok = if add { yes_no(1152, &[&shown], "Add %1 (Y/N)? ") } else { yes_no(1153, &[&shown], "Replace %1 (Y/N)? ") };
                out("\r\n");
                if !ok {
                    continue;
                }
            }
            if add { say(1154, &[&shown], "Adding %1\r\n") } else { say(1155, &[&shown], "Replacing %1\r\n") }
            let ct = cstr(&to);
            if !add && ro {
                // Clear read-only so the copy can replace it.
                if let Some(mut st) = status(&to) {
                    st.attr &= !FILE_READONLY;
                    st.fdate_creation = 0;
                    st.ftime_creation = 0;
                    st.fdate_access = 0;
                    st.ftime_access = 0;
                    st.fdate_write = 0;
                    st.ftime_write = 0;
                    unsafe { DosSetPathInfo(ct.as_ptr(), FIL_STANDARD, &st as *const _ as *const u8, core::mem::size_of::<FileStatus3>() as u32, 0) };
                }
            }
            let cf = cstr(&from);
            let opts = DCPY_EXISTING | if faileas { DCPY_FAILEAS } else { 0 };
            let r = unsafe { DosCopy(cf.as_ptr(), ct.as_ptr(), opts) };
            if r != 0 {
                error(r);
                rc = r;
                continue;
            }
            count += 1;
        }
    }
    let n = alloc::format!("{count}");
    match (add, count) {
        (true, 0) => say(1148, &[], "No files were added.\r\n"),
        (true, _) => say(1149, &[&n], "%1 file(s) added.\r\n"),
        (false, 0) => say(1150, &[], "No files were replaced.\r\n"),
        (false, _) => say(1151, &[&n], "%1 file(s) replaced.\r\n"),
    }
    rc
}
