//! RESTORE: bring files back from a backup set made by BACKUP (rs2's or
//! IBM's).
//! RESTORE drive1: [drive2:][path][filename] [/S] [/P] [/B:date] [/A:date]
//!         [/E:time] [/L:time] [/M] [/N] [/F] [/D]
//!
//! Reads `\BACKUP\CONTROL.001` and `BACKUP.001` on drive1 (format in
//! crates/os2-backup) and restores the matching files, with their
//! extended attributes, attributes and dates, to their directories on
//! drive2 (the current drive if none). /S includes subdirectories, /P asks
//! before replacing read-only files or files changed since the backup,
//! /B and /A restore only files changed on or before / on or after a date,
//! /E and /L on or before / on or after a time, /M only files changed or
//! deleted since the backup, /N only files that no longer exist, /D lists
//! the files without restoring them. Return codes: 0 done, 1 no files
//! found, 4 error.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::*;
use os2_backup::{matches, parse_fealist, Control};
use utils::files::*;
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

fn parse_date(s: &str) -> Option<u16> {
    let parts: Vec<u32> = s.split(['-', '/', '.']).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    if parts.len() != 3 {
        return None;
    }
    let mut ci = CountryInfo::default();
    let mut cb = 0;
    unsafe { DosQueryCtryInfo(core::mem::size_of::<CountryInfo>() as u32, &CountryCode::default(), &mut ci, &mut cb) };
    let (m, d, y) = match ci.date_fmt {
        1 => (parts[1], parts[0], parts[2]),
        2 => (parts[1], parts[2], parts[0]),
        _ => (parts[0], parts[1], parts[2]),
    };
    let y = if y < 80 { y + 2000 } else if y < 100 { y + 1900 } else { y };
    ((1..=12).contains(&m) && (1..=31).contains(&d) && y >= 1980).then(|| (((y - 1980) << 9) | (m << 5) | d) as u16)
}

fn parse_time(s: &str) -> Option<u16> {
    let p: Vec<u32> = s.split([':', '.']).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (h, m, sec) = (*p.first()?, *p.get(1).unwrap_or(&0), *p.get(2).unwrap_or(&0));
    (h < 24 && m < 60 && sec < 60).then(|| ((h << 11) | (m << 5) | (sec / 2)) as u16)
}

/// Read `len` bytes at `off` of an open file.
fn read_at(hf: u32, off: u64, len: u64) -> Result<Vec<u8>, u32> {
    let mut p = 0;
    let rc = unsafe { DosSetFilePtr(hf, off as i32, 0, &mut p) };
    if rc != 0 {
        return Err(rc);
    }
    let mut v = alloc::vec![0u8; len as usize];
    let mut done = 0usize;
    while done < v.len() {
        let mut n = 0;
        let rc = unsafe { DosRead(hf, v[done..].as_mut_ptr(), (v.len() - done) as u32, &mut n) };
        if rc != 0 {
            return Err(rc);
        }
        if n == 0 {
            return Err(38); // ERROR_HANDLE_EOF
        }
        done += n as usize;
    }
    Ok(v)
}

/// Copy `len` bytes at `off` of the data file into a new file.
fn extract(data: u32, off: u64, len: u64, to: &str) -> Result<(), u32> {
    let mut p = 0;
    let rc = unsafe { DosSetFilePtr(data, off as i32, 0, &mut p) };
    if rc != 0 {
        return Err(rc);
    }
    let hf = create(to, 0)?;
    let mut buf = alloc::vec![0u8; 32768];
    let mut left = len;
    let mut r = Ok(());
    while left > 0 {
        let want = (buf.len() as u64).min(left) as u32;
        let mut n = 0;
        let rc = unsafe { DosRead(data, buf.as_mut_ptr(), want, &mut n) };
        if rc != 0 || n == 0 {
            r = Err(if rc != 0 { rc } else { 38 });
            break;
        }
        if let Err(e) = write_all(hf, &buf[..n as usize]) {
            r = Err(e);
            break;
        }
        left -= n as u64;
    }
    unsafe { DosClose(hf) };
    r
}

fn main(s: &os2::Startup) -> u32 {
    let (mut sub, mut prompt, mut changed, mut missing, mut list) = (false, false, false, false, false);
    let (mut before, mut after, mut earlier, mut later) = (None, None, None, None);
    let mut names: Vec<String> = Vec::new();
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        let Some(sw) = u.strip_prefix('/') else {
            names.push(a);
            continue;
        };
        let bad = || {
            err(1003, &[], "The command syntax is not correct.\r\n");
            4
        };
        match sw {
            "S" => sub = true,
            "P" => prompt = true,
            "M" => changed = true,
            "N" => missing = true,
            "D" => list = true,
            "F" => {}
            _ if sw.starts_with("B:") => match parse_date(&sw[2..]) {
                Some(d) => before = Some(d),
                None => return bad(),
            },
            _ if sw.starts_with("A:") => match parse_date(&sw[2..]) {
                Some(d) => after = Some(d),
                None => return bad(),
            },
            _ if sw.starts_with("E:") => match parse_time(&sw[2..]) {
                Some(t) => earlier = Some(t),
                None => return bad(),
            },
            _ if sw.starts_with("L:") => match parse_time(&sw[2..]) {
                Some(t) => later = Some(t),
                None => return bad(),
            },
            _ => return bad(),
        }
    }
    if names.iter().any(|n| n.starts_with("\\\\")) {
        err(1069, &[], "This command does not accept UNC names.\r\n");
        return 4;
    }
    if names.is_empty() || names.len() > 2 {
        err(1643, &[], "\r\nThe number of parameters is not correct.\r\n");
        return 4;
    }
    let Some(sdrive) = drive_arg(Some(names[0].trim_end_matches('\\'))) else {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 4;
    };
    // The target: drive, directory and name pattern.
    let spec = names.get(1).cloned().unwrap_or_default();
    let tdrive = if spec.len() >= 2 && spec.as_bytes()[1] == b':' { spec.as_bytes()[0].to_ascii_uppercase() - b'A' } else { drive_arg(None).unwrap_or(2) };
    if tdrive == sdrive {
        err(1642, &[], "\r\nThe source and target drives are the same.\r\n");
        return 4;
    }
    let tl = (b'A' + tdrive) as char;
    let rest = if spec.len() >= 2 && spec.as_bytes()[1] == b':' { &spec[2..] } else { &spec[..] };
    let full = if rest.starts_with('\\') {
        alloc::format!("{tl}:{rest}")
    } else {
        // Relative to the target drive's current directory.
        let mut buf = [0u8; 260];
        let mut cb = buf.len() as u32;
        unsafe { DosQueryCurrentDir(tdrive as u32 + 1, buf.as_mut_ptr(), &mut cb) };
        let cur: String = buf.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
        let base = if cur.is_empty() { alloc::format!("{tl}:\\") } else { alloc::format!("{tl}:\\{cur}\\") };
        alloc::format!("{base}{rest}")
    };
    let (sdir, sname) = if rest.is_empty() || full.ends_with('\\') || is_dir(&full) {
        (String::from(full[2..].trim_matches('\\')), String::from("*.*"))
    } else {
        let d = dir_of(&full);
        (String::from(d[2..].trim_matches('\\')), String::from(&full[d.len()..]))
    };
    let sl = (b'A' + sdrive) as char;
    let control_path = alloc::format!("{sl}:\\BACKUP\\CONTROL.001");
    let ctl = match read_file(&control_path).map(|d| Control::parse(&d)) {
        Ok(Ok(c)) => c,
        _ => {
            say(1649, &[], "\r\nThe source does not hold backup files.\r\n");
            return 4;
        }
    };
    let data = match open_read(&alloc::format!("{sl}:\\BACKUP\\BACKUP.001")) {
        Ok(h) => h,
        Err(_) => {
            say(1649, &[], "\r\nThe source does not hold backup files.\r\n");
            return 4;
        }
    };
    // The backup date: the control file's.
    if let Some(st) = status(&control_path) {
        let d = st.fdate_write;
        say(1640, &[&alloc::format!("{:02}-{:02}-{:04}", (d >> 5) & 15, d & 31, (d >> 9) + 1980)], "\r\nThe files were backed up on %1.\r\n");
    }
    if list {
        outln(&alloc::format!("Listing files on drive {sl}:"));
    } else {
        say(1634, &[&alloc::format!("{sl}")], "\r\nFiles are restored from drive %1:\r\n");
    }
    let mut count = 0;
    let mut rc = 0;
    for (dir, files) in &ctl.dirs {
        for f in files {
            if !matches(&dir.path, &f.name, &sdir, &sname, sub) {
                continue;
            }
            if before.is_some_and(|b| f.date > b) || after.is_some_and(|a| f.date < a) {
                continue;
            }
            if earlier.is_some_and(|e| f.time > e) || later.is_some_and(|l| f.time < l) {
                continue;
            }
            let shown = if dir.path.is_empty() { alloc::format!("\\{}", f.name) } else { alloc::format!("\\{}\\{}", dir.path, f.name) };
            let to = alloc::format!("{tl}:{shown}");
            let now_there = status(&to);
            if missing && now_there.is_some() {
                continue;
            }
            if changed && now_there.as_ref().is_some_and(|t| t.attr & FILE_ARCHIVED == 0 && ((t.fdate_write as u32) << 16 | t.ftime_write as u32) <= ((f.date as u32) << 16 | f.time as u32)) {
                continue;
            }
            count += 1;
            outln(&shown);
            if list {
                continue;
            }
            if let Some(t) = &now_there {
                if prompt {
                    let newer = ((t.fdate_write as u32) << 16 | t.ftime_write as u32) > ((f.date as u32) << 16 | f.time as u32);
                    let ask = if t.attr & FILE_READONLY != 0 {
                        Some(yes_no(1637, &[&to], "\r\nWarning!  %1 is a read-only file.\r\nReplace it (Y/N)?\r\n"))
                    } else if newer {
                        Some(yes_no(1638, &[&to], "Warning!  %1 was changed after the backup.\r\nReplace it (Y/N)?\r\n"))
                    } else {
                        None
                    };
                    if ask == Some(false) {
                        continue;
                    }
                }
                if t.attr & (FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM) != 0 {
                    set_info(&to, 0, 0, 0);
                }
            }
            // The directories, with their EAs, as backed up.
            let tdir = alloc::format!("{tl}:\\{}", dir.path);
            if !dir.path.is_empty() && !is_dir(&tdir) {
                make_dirs(&tdir);
                if dir.ea_len >= 5 {
                    if let Ok(e) = read_at(data, dir.ea_offset, dir.ea_len) {
                        write_eas(&tdir, &parse_fealist(&e).unwrap_or_default());
                    }
                }
            }
            let mut r = extract(data, f.offset, f.part_len, &to);
            if r.is_ok() && f.ea_len >= 5 {
                r = read_at(data, f.ea_offset, f.ea_len).and_then(|e| {
                    let rc = write_eas(&to, &parse_fealist(&e).unwrap_or_default());
                    // A target without EA support keeps the data.
                    if rc == 0 || rc == 282 { Ok(()) } else { Err(rc) }
                });
            }
            if let Err(e) = r {
                error(e);
                say(1648, &[], "\r\nThe file listed last was not restored.\r\n");
                rc = 4;
                continue;
            }
            set_info(&to, f.attr as u32, f.date, f.time);
            if f.attr & 4 != 0 && (f.name.eq_ignore_ascii_case("OS2KRNL") || f.name.eq_ignore_ascii_case("OS2LDR")) {
                say(1639, &[], "\r\nSystem files were restored.  The target disk may not start.\r\n");
            }
        }
    }
    unsafe { DosClose(data) };
    if count == 0 {
        err(1645, &[], "\r\nNo files were found to restore.\r\n");
        return 1;
    }
    out("\r\n");
    rc
}
