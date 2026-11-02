//! BACKUP: copy files and their extended attributes to a backup set.
//! BACKUP source drive: [/S] [/M] [/A] [/D:date] [/T:time] [/L[:logfile]] [/FS:size] [/F]
//!
//! The set goes to `\BACKUP` on the target drive in IBM's format
//! (crates/os2-backup): CONTROL.001 and BACKUP.001, so IBM's RESTORE reads
//! it and RESTORE reads IBM's. /S takes the subdirectories too, /M only
//! files whose archive bit is set, /D and /T only files changed on or
//! after that date (and time), /A adds to the set already on the target,
//! /L logs each file (default \BACKUP.LOG on the source drive). The
//! archive bit of each file backed up is cleared. Return codes: 0 done,
//! 1 nothing to back up, 2 some files could not be backed up, 4 error.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::path::cstr;
use os2::*;
use os2_backup::{Control, DirRecord, FileRecord, Header, FLAG_COMPLETE, FLAG_LAST_PART};
use utils::files::*;
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

struct Opts {
    sub: bool,
    modified: bool,
    append: bool,
    since: Option<u32>,
    log: Option<String>,
}

/// A date (and time) argument in the country's date order as a packed
/// date << 16 | packed time.
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
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1980 {
        return None;
    }
    Some((((y - 1980) << 9) | (m << 5) | d) as u16)
}

fn parse_time(s: &str) -> Option<u16> {
    let p: Vec<u32> = s.split([':', '.']).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (h, m, sec) = (*p.first()?, *p.get(1).unwrap_or(&0), *p.get(2).unwrap_or(&0));
    (h < 24 && m < 60 && sec < 60).then(|| ((h << 11) | (m << 5) | (sec / 2)) as u16)
}

fn copy_into(data: u32, path: &str) -> Result<u64, u32> {
    let hf = open_read(path)?;
    let mut buf = alloc::vec![0u8; 32768];
    let mut total = 0u64;
    let r = loop {
        let mut n = 0;
        let rc = unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 {
            break Err(rc);
        }
        if n == 0 {
            break Ok(total);
        }
        if let Err(rc) = write_all(data, &buf[..n as usize]) {
            break Err(rc);
        }
        total += n as u64;
    };
    unsafe { DosClose(hf) };
    r
}

fn open_data(path: &str, append: bool) -> Result<(u32, u64), u32> {
    let c = cstr(path);
    let (mut hf, mut act) = (0, 0);
    let open = if append { OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS } else { OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS };
    let rc = unsafe { DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, open, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE, core::ptr::null_mut()) };
    if rc != 0 {
        return Err(rc);
    }
    let mut end = 0u32;
    unsafe { DosSetFilePtr(hf, 0, 2, &mut end) };
    Ok((hf, end as u64))
}

fn main(s: &os2::Startup) -> u32 {
    let mut o = Opts { sub: false, modified: false, append: false, since: None, log: None };
    let (mut date, mut time) = (None, None);
    let mut names: Vec<String> = Vec::new();
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        if let Some(sw) = u.strip_prefix('/') {
            match sw {
                "S" => o.sub = true,
                "M" => o.modified = true,
                "A" => o.append = true,
                "F" => {}
                "L" => o.log = Some(String::new()),
                _ if sw.starts_with("L:") => o.log = Some(String::from(&a[3..])),
                _ if sw.starts_with("FS:") => {}
                _ if sw.starts_with("D:") => match parse_date(&sw[2..]) {
                    Some(d) => date = Some(d),
                    None => {
                        err(1036, &[], "The date is not correct.\r\n");
                        return 4;
                    }
                },
                _ if sw.starts_with("T:") => match parse_time(&sw[2..]) {
                    Some(t) => time = Some(t),
                    None => {
                        err(1044, &[], "The time is not correct.\r\n");
                        return 4;
                    }
                },
                _ => {
                    err(1002, &[&a], "%1 is not a correct parameter for this command.\r\n");
                    return 4;
                }
            }
        } else {
            names.push(a);
        }
    }
    if let Some(d) = date {
        o.since = Some((d as u32) << 16 | time.unwrap_or(0) as u32);
    }
    if names.first().is_some_and(|n| n.starts_with("\\\\")) || names.get(1).is_some_and(|n| n.starts_with("\\\\")) {
        err(1069, &[], "This command does not accept UNC names.\r\n");
        return 4;
    }
    let Some(src) = names.first() else {
        err(1667, &[], "No source drive was given.\r\n");
        return 4;
    };
    let Some(tgt) = names.get(1) else {
        err(1668, &[], "No target drive was given.\r\n");
        return 4;
    };
    let Some(tdrive) = drive_arg(Some(tgt.trim_end_matches('\\'))) else {
        err(1665, &[], "BACKUP needs the target drive letter.\r\n");
        return 4;
    };
    let src_full = full_path(src);
    let sdrive = src_full.as_bytes()[0].to_ascii_uppercase() - b'A';
    if sdrive == tdrive {
        err(1671, &[], "The source and target drives are the same.\r\n");
        return 4;
    }
    // Source: a directory means all of its files.
    let (dir, pattern) = if is_dir(&src_full) {
        (String::from(src_full.trim_end_matches('\\')), String::from("*"))
    } else {
        let d = dir_of(&src_full);
        (String::from(d.trim_end_matches('\\')), String::from(&src_full[d.len()..]))
    };
    let root = alloc::format!("{}:\\", (b'A' + sdrive) as char);
    let base = if dir.len() <= 2 { root.clone() } else { dir.clone() };
    let mut found = Vec::new();
    walk(&base, &pattern, o.sub, true, FILE_HIDDEN | FILE_SYSTEM | FILE_READONLY | FILE_ARCHIVED, &mut found);
    let tl = (b'A' + tdrive) as char;
    let bdir = alloc::format!("{tl}:\\BACKUP");
    let control_path = alloc::format!("{bdir}\\CONTROL.001");
    let data_path = alloc::format!("{bdir}\\BACKUP.001");
    let mut control = Control { header: Header { sequence: 1, last: true }, dirs: Vec::new() };
    if o.append {
        match read_file(&control_path).map(|d| Control::parse(&d)) {
            Ok(Ok(c)) => control = c,
            Ok(Err(_)) => {
                say(1669, &[], "Files cannot be added to the backup on the target: it was\r\nwritten by a different version of BACKUP.\r\n");
                return 4;
            }
            Err(_) => o.append = false,
        }
    }
    if !o.append && is_dir(&bdir) {
        let old = find_all(&alloc::format!("{bdir}\\*"), FILE_HIDDEN | FILE_SYSTEM | FILE_READONLY);
        if !old.is_empty() {
            press_enter(
                1682,
                &[&alloc::format!("{tl}")],
                "Warning!  The files in the \\BACKUP directory of\r\ntarget drive %1: will be deleted.\r\n\r\nPress Enter to continue or Ctrl+Break to stop.\r\n",
            );
            for f in old {
                if f.attr & FILE_DIRECTORY == 0 {
                    let p = cstr(&join(&bdir, &f.name));
                    unsafe { DosForceDelete(p.as_ptr()) };
                }
            }
        }
    }
    make_dirs(&bdir);
    let mut log = None;
    if let Some(l) = &o.log {
        let name = if l.is_empty() { alloc::format!("{root}BACKUP.LOG") } else { full_path(l) };
        let c = cstr(&name);
        let (mut hf, mut act) = (0, 0);
        let rc = unsafe {
            DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut())
        };
        if rc != 0 {
            err(1677, &[], "The log file cannot be opened.\r\n");
            return 4;
        }
        let mut end = 0;
        unsafe { DosSetFilePtr(hf, 0, 2, &mut end) };
        say(1680, &[&name], "BACKUP is writing its log to %1.\r\n");
        let dt = now();
        let line = alloc::format!("{:02}-{:02}-{:04}  {}:{:02}:{:02}\r\n", dt.month, dt.day, dt.year, dt.hours, dt.minutes, dt.seconds);
        let _ = write_all(hf, &os2::text_bytes(&line));
        log = Some(hf);
    }
    let (data, mut at) = match open_data(&data_path, o.append) {
        Ok(v) => v,
        Err(_) => {
            err(1690, &[], "The target cannot hold a backup.\r\n");
            return 4;
        }
    };
    say(1683, &[&alloc::format!("{tl}")], "The files are being backed up to drive %1.\r\n");
    let mut rc = 0;
    let mut count = 0;
    let mut full = false;
    // Group the files by directory, in the order the walk found them.
    let rel_dir = |rel: &str| -> String {
        let d = if dir.len() <= 2 { String::new() } else { String::from(&dir[3..]) };
        match rel.rfind('\\') {
            Some(i) if d.is_empty() => String::from(&rel[..i]),
            Some(i) => alloc::format!("{d}\\{}", &rel[..i]),
            None => d,
        }
    };
    let mut cur: Option<String> = None;
    for (rel, f) in found.iter().filter(|(_, f)| f.attr & FILE_DIRECTORY == 0) {
        if o.modified && f.attr & FILE_ARCHIVED == 0 {
            continue;
        }
        if let Some(since) = o.since {
            if ((f.written.0 as u32) << 16 | f.written.1 as u32) < since {
                continue;
            }
        }
        let path = join(&base, rel);
        let rd = rel_dir(rel);
        if cur.as_deref() != Some(rd.as_str()) {
            // A new directory record, with the directory's EAs.
            let dpath = alloc::format!("{root}{rd}");
            let eas = os2_backup::fealist(&read_eas(&dpath).unwrap_or_default());
            let mut rec = DirRecord { path: rd.clone(), ..Default::default() };
            if eas.len() >= 5 {
                if write_all(data, &eas).is_err() {
                    full = true;
                    break;
                }
                rec.ea_offset = at;
                rec.ea_len = eas.len() as u64;
                at += eas.len() as u64;
            }
            control.dirs.push((rec, Vec::new()));
            out("\r\n");
            cur = Some(rd.clone());
        }
        let shown = if rd.is_empty() { alloc::format!("\\{}", f.name) } else { alloc::format!("\\{rd}\\{}", f.name) };
        outln(&shown);
        let eas = os2_backup::fealist(&read_eas(&path).unwrap_or_default());
        let mut rec = FileRecord {
            name: f.name.clone(),
            flags: FLAG_LAST_PART | FLAG_COMPLETE,
            size: f.size as u64,
            part: 1,
            attr: f.attr as u16,
            time: f.written.1,
            date: f.written.0,
            ..Default::default()
        };
        let start = at;
        if eas.len() >= 5 {
            if write_all(data, &eas).is_err() {
                full = true;
                break;
            }
            rec.ea_offset = at;
            rec.ea_len = eas.len() as u64;
            at += eas.len() as u64;
        }
        rec.offset = at;
        match copy_into(data, &path) {
            Ok(n) => {
                rec.part_len = n;
                rec.size = n;
                at += n;
            }
            Err(112) => {
                full = true;
                break;
            }
            Err(_) => {
                // Not readable now (in use): skip it.
                say(1688, &[], "This file cannot be backed up now.\r\n");
                say(2093, &[], "The file shown last was not backed up.\r\n");
                unsafe { DosSetFileSize(data, start as u32) };
                let mut p = 0;
                unsafe { DosSetFilePtr(data, start as i32, 0, &mut p) };
                at = start;
                rc = 2;
                continue;
            }
        }
        control.dirs.last_mut().unwrap().1.push(rec);
        count += 1;
        if let Some(l) = log {
            let _ = write_all(l, &os2::text_bytes(&alloc::format!("001  {shown}\r\n")));
        }
        // Backed up: clear the archive bit.
        set_info(&path, f.attr & !FILE_ARCHIVED, 0, 0);
    }
    // Directory records left without files (a skipped file) are dropped.
    control.dirs.retain(|(_, files)| !files.is_empty());
    unsafe { DosClose(data) };
    if let Some(l) = log {
        unsafe { DosClose(l) };
    }
    if full {
        say(1691, &[], "The last file was not backed up: the target hard disk\r\nis full.\r\n");
        say(1692, &[&alloc::format!("{tl}")], "Target hard disk %1: is full.\r\n");
        rc = 4;
    }
    if count == 0 && !o.append {
        let c = cstr(&data_path);
        unsafe { DosDelete(c.as_ptr()) };
        if !full {
            say(1685, &[], "No files were found to back up.\r\n");
            return 1;
        }
        return rc;
    }
    control.header.last = true;
    if let Err(e) = write_file(&control_path, &control.write()) {
        error(e);
        return 4;
    }
    rc
}
