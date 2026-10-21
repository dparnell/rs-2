//! CHKDSK: check a disk and report its use. CHKDSK [drive:] [/F] [/V] [/C] [/P]
//!
//! FAT volumes are read through DASD track IOCtls and checked with
//! `fat::check`; /F repairs them (the drive is locked meanwhile, and the
//! system remounts it when CHKDSK unlocks it). HPFS volumes get a
//! read-only scan of the directory tree.
//!
//! /P reports to the Presentation Manager front end (IBM's PMCHKDSK, which
//! runs `CHKDSK.COM drive /P` with its stdin and stdout on pipes): results
//! and questions go out as `MSG_<number>,<param>,...` lines, the numbers
//! being the OSO001.MSG messages CHKDSK would display. PMCHKDSK reads the
//! lines, ignores every one without `MSG_`, fills its Results window and pie
//! chart from 1507 (file system), 1361-1368/1819 (bytes) or 568-571/632/633
//! (kilobytes), and answers prompts (1356) with a line starting Y or N.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use fat::check::{Checker, Finding};
use utils::disk::{fs_name, Disk};
use utils::*;
use core::sync::atomic::{AtomicBool, Ordering};
os2::entry!(main);

static PM: AtomicBool = AtomicBool::new(false);

fn pm() -> bool {
    PM.load(Ordering::Relaxed)
}

/// /P: one message for PMCHKDSK.
fn pm_msg(n: u32, params: &[u64]) {
    let mut l = alloc::format!("MSG_{n}");
    for p in params {
        l.push_str(&alloc::format!(",{p}"));
    }
    outln(&l);
}

/// Text for the console, or the message number and parameters for /P.
fn report(n: u32, params: &[u64], text: &str) {
    if pm() {
        pm_msg(n, params);
    } else {
        outln(text);
    }
}

fn main(s: &os2::Startup) -> u32 {
    let (mut fix, mut verbose) = (false, false);
    let mut drive = None;
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        match u.as_str() {
            "/F" => fix = true,
            "/V" => verbose = true,
            "/C" => {}
            "/P" => PM.store(true, Ordering::Relaxed),
            _ if u.starts_with("/F:") => fix = true,
            _ if u.starts_with('/') => {
                eprint(&alloc::format!("SYS1330: The system cannot accept the {u} parameter.\r\n"));
                return 87;
            }
            _ => drive = Some(a),
        }
    }
    let Some(d) = drive_arg(drive.as_deref()) else {
        eprint("SYS1333: CHKDSK cannot access the specified drive.\r\n");
        return 15;
    };
    let Some(fs) = fs_name(d) else {
        error(15);
        return 15;
    };
    if pm() {
        outln(&alloc::format!("MSG_1507,{fs}"));
    } else {
        outln(&alloc::format!("The type of file system for the disk is {fs}.\r\n"));
    }
    match fs.as_str() {
        "FAT" => check_fat(d, fix, verbose),
        "HPFS" => check_hpfs(d, verbose),
        _ => {
            outln(&alloc::format!("CHKDSK does not support the {fs} file system."));
            1
        }
    }
}

fn show_label(d: u8) {
    if pm() {
        return;
    }
    if let Ok((label, ser)) = volume_label(d) {
        if !label.is_empty() {
            outln(&alloc::format!("The volume label is {label}."));
        }
        outln(&alloc::format!("The Volume Serial Number is {}.", serial(ser)));
    }
}

fn check_fat(d: u8, fix: bool, verbose: bool) -> u32 {
    let disk = match Disk::open(d, fix) {
        Ok(x) => x,
        Err(rc) => {
            error(rc);
            return rc;
        }
    };
    if fix && disk.lock() != 0 {
        if pm() {
            // SYS0108: the disk is in use or locked by another process.
            pm_msg(108, &[]);
            return 108;
        }
        outln("CHKDSK cannot lock the drive because files on it are in use.\r\nThe /F parameter cannot be used on this drive now.");
        return 108;
    }
    show_label(d);
    let mut vol = match fat::Volume::mount(&disk) {
        Ok(v) => v,
        Err(_) => {
            eprint(&alloc::format!("SYS1374: The File Allocation Table (FAT) on drive {}: is bad.\r\n", (b'A' + d) as char));
            return 1;
        }
    };
    let letter = (b'A' + d) as char;
    if verbose {
        list(&vol, letter, &vol.root_entry(), &mut String::new());
    }
    let mut c = match Checker::new(&mut vol, fix) {
        Ok(c) => c,
        Err(_) => {
            eprint(&alloc::format!("SYS1336: Disk error reading the File Allocation Table of drive {letter}:.\r\n"));
            return 1;
        }
    };
    if c.scan().is_err() {
        eprint("SYS1313: An error occurred while reading or writing the disk.\r\n");
        return 1;
    }
    if !c.findings.is_empty() {
        outln("");
    }
    for f in &c.findings {
        let msg = match f {
            Finding::BadFirstCluster(p) => alloc::format!("The first cluster number of {letter}:{p} is incorrect.\r\nThe entry has been truncated."),
            Finding::BadChain(p) => alloc::format!("{letter}:{p} has an incorrect cluster chain; it has been truncated."),
            Finding::SizeAdjusted(p) => alloc::format!("{letter}:{p} allocation error, size adjusted."),
            Finding::CrossLinked { path, cluster } => alloc::format!("{letter}:{path} is cross-linked on cluster {cluster}."),
            Finding::BadSubdirectory(p) => alloc::format!("{letter}:{p} is an incorrect subdirectory."),
            Finding::UnreadableDirectory(p) => alloc::format!("CHKDSK cannot read the directory {letter}:{p}."),
            Finding::EaMissing(p) => alloc::format!("The extended attributes of {letter}:{p} are missing or damaged; the reference has been removed."),
            Finding::EaShared(p) => alloc::format!("{letter}:{p} shares its extended attributes with another file; the reference has been removed."),
            Finding::EaOrphaned(p) => alloc::format!("Extended attributes without an owner ({p}) have been freed."),
            Finding::EaFileCorrupt => alloc::format!("The extended attribute file on drive {letter}: is damaged."),
        };
        outln(&msg);
    }
    let lost = c.totals.lost_clusters;
    let mut errors = !c.findings.is_empty() || lost > 0;
    let cs = c.cluster_size() as u64;
    let mut convert = false;
    if lost > 0 {
        let chains = c.totals.lost_chains;
        if fix && pm() {
            pm_msg(1356, &[lost as u64, chains as u64]);
            convert = matches!(ask("").chars().next(), Some('Y' | 'y'));
        } else if fix {
            convert = yes(&alloc::format!(
                "\r\n{lost} lost clusters found in {chains} chains.\r\nConvert the lost chains to files (Y/N)? "
            ));
        } else {
            if !pm() {
                outln(&alloc::format!("\r\n{lost} lost clusters found in {chains} chains."));
            }
            convert = true;
        }
    }
    let now = {
        let mut dt = os2::DateTime::default();
        unsafe { os2::DosGetDateTime(&mut dt) };
        fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds)
    };
    match c.recover(convert, now) {
        Ok(names) => {
            for n in names {
                outln(&alloc::format!("CHKDSK placed recovered data in file {letter}:\\{n}."));
            }
        }
        Err(_) => {
            outln("No recovered files can be added to the root directory.");
            errors = true;
        }
    }
    if fix && !convert && lost > 0 {
        outln(&alloc::format!("{} bytes disk space freed.", lost as u64 * cs));
    }
    if errors && !fix {
        outln("\r\nErrors were found.  The /F parameter was not specified, so the\r\ncorrections were not written to the disk.");
    }
    let t = c.totals.clone();
    if pm() {
        pm_fat_totals(&t, cs, c.total_clusters(), c.free_clusters());
        drop(c);
        drop(vol);
        if fix {
            disk.unlock();
        }
        return if errors && !fix { 1 } else { 0 };
    }
    let b = |n: u32| num(n as u64 * cs, 12);
    outln(&alloc::format!("\r\n{} bytes total disk space.", b(c.total_clusters())));
    if t.bad_clusters > 0 {
        outln(&alloc::format!("{} bytes in bad sectors.", b(t.bad_clusters)));
    }
    if t.hidden > 0 {
        outln(&alloc::format!("{} bytes in {} hidden files.", b(t.hidden_clusters), t.hidden));
    }
    outln(&alloc::format!("{} bytes in {} directories.", b(t.dir_clusters), t.dirs));
    if t.ea_clusters > 0 {
        outln(&alloc::format!("{} bytes in extended attributes.", b(t.ea_clusters)));
    }
    outln(&alloc::format!("{} bytes in {} user files.", b(t.file_clusters), t.files));
    if t.recovered > 0 {
        let would = if fix { "" } else { "would be " };
        outln(&alloc::format!("{} bytes {would}in {} recovered files.", b(t.recovered_clusters), t.recovered));
    }
    outln(&alloc::format!("{} bytes available on disk.", b(c.free_clusters())));
    outln(&alloc::format!("\r\n{} bytes in each allocation unit.", num(cs, 12)));
    outln(&alloc::format!("{} total allocation units.", num(c.total_clusters() as u64, 12)));
    outln(&alloc::format!("{} available allocation units on disk.", num(c.free_clusters() as u64, 12)));
    drop(c);
    drop(vol);
    if fix {
        disk.unlock();
    }
    if errors && !fix { 1 } else { 0 }
}

/// /P: the summary. PMCHKDSK keeps the values in 32-bit longs, so a volume
/// of 2 GB or more is reported in kilobytes, as IBM's HPFS CHKDSK does.
fn pm_fat_totals(t: &fat::check::Totals, cs: u64, total: u32, free: u32) {
    let total_b = total as u64 * cs;
    if total_b < 1 << 31 {
        let b = |n: u32| n as u64 * cs;
        pm_msg(1361, &[total_b]);
        if t.bad_clusters > 0 {
            pm_msg(1362, &[b(t.bad_clusters)]);
        }
        if t.hidden > 0 {
            pm_msg(1363, &[b(t.hidden_clusters), t.hidden as u64]);
        }
        pm_msg(1364, &[b(t.dir_clusters), t.dirs as u64]);
        if t.ea_clusters > 0 {
            pm_msg(1819, &[b(t.ea_clusters)]);
        }
        pm_msg(1365, &[b(t.file_clusters), t.files as u64]);
        // PMCHKDSK cuts this one's parameter at the next comma (and
        // faults without one): send the allocation unit size after it.
        pm_msg(1368, &[b(free), cs]);
    } else {
        let k = |n: u32| n as u64 * cs / 1024;
        pm_msg(568, &[total_b / 1024]);
        pm_msg(569, &[k(t.dir_clusters), t.dirs as u64]);
        pm_msg(570, &[k(t.file_clusters), t.files as u64]);
        if t.ea_clusters > 0 {
            pm_msg(633, &[k(t.ea_clusters)]);
        }
        let other = k(t.bad_clusters) + k(t.hidden_clusters);
        if other > 0 {
            pm_msg(632, &[other]);
        }
        pm_msg(571, &[k(free)]);
    }
}

/// /V: list every file.
fn list<D: fat::BlockDevice>(vol: &fat::Volume<D>, letter: char, dir: &fat::Entry, path: &mut String) {
    let Ok(entries) = vol.read_dir(if dir.cluster == 0 { vol.root_dir() } else { fat::Dir::Cluster(dir.cluster) }) else { return };
    outln(&alloc::format!("Directory {letter}:{}\\", path));
    let mut subdirs: Vec<fat::Entry> = Vec::new();
    for e in entries {
        if e.is_dir() {
            subdirs.push(e);
        } else {
            outln(&alloc::format!("{letter}:{path}\\{}", e.name));
        }
    }
    for e in subdirs {
        let len = path.len();
        path.push('\\');
        path.push_str(&e.name);
        list(vol, letter, &e, path);
        path.truncate(len);
    }
}

fn check_hpfs(d: u8, verbose: bool) -> u32 {
    let disk = match Disk::open(d, false) {
        Ok(x) => x,
        Err(rc) => {
            error(rc);
            return rc;
        }
    };
    show_label(d);
    let mut vol = match hpfs::Volume::mount(&disk) {
        Ok(v) => v,
        Err(_) => {
            report(1373, &[], "CHKDSK cannot read the HPFS structures of this volume.");
            return 1;
        }
    };
    if !pm() {
        outln("\r\nCHKDSK is checking the directory tree (read only: repairing HPFS\r\nvolumes is not available yet).");
    }
    let letter = (b'A' + d) as char;
    let (mut dirs, mut files, mut file_kb, mut bad) = (0u64, 0u64, 0u64, 0u32);
    let Ok(root) = vol.root_entry() else {
        outln("CHKDSK cannot read the root directory.");
        return 1;
    };
    let mut stack: Vec<(hpfs::Entry, String)> = alloc::vec![(root, String::new())];
    while let Some((dir, path)) = stack.pop() {
        if verbose {
            outln(&alloc::format!("Directory {letter}:{path}\\"));
        }
        let entries = match vol.read_dir(&dir) {
            Ok(e) => e,
            Err(_) => {
                outln(&alloc::format!("CHKDSK cannot read the directory {letter}:{path}\\."));
                bad += 1;
                continue;
            }
        };
        for e in entries {
            let p = alloc::format!("{path}\\{}", e.name);
            if e.is_dir() {
                dirs += 1;
                stack.push((e, p));
            } else {
                files += 1;
                file_kb += (e.size as u64).div_ceil(512) / 2;
                if verbose {
                    outln(&alloc::format!("{letter}:{p}"));
                }
            }
        }
    }
    let i = vol.info();
    if pm() {
        let k = |s: u32| s as u64 * i.sector_size as u64 / 1024;
        let (total, free) = (k(i.total_sectors), k(i.free_sectors));
        pm_msg(568, &[total]);
        pm_msg(569, &[dirs * 2, dirs]);
        pm_msg(570, &[file_kb, files]);
        let used = dirs * 2 + file_kb + free;
        if total > used {
            pm_msg(632, &[total - used]);
        }
        pm_msg(571, &[free]);
        return if bad > 0 { 1 } else { 0 };
    }
    let kb = |s: u32| num(s as u64 * i.sector_size as u64 / 1024, 12);
    outln(&alloc::format!("\r\n{} kilobytes total disk space.", kb(i.total_sectors)));
    outln(&alloc::format!("{} kilobytes are in {dirs} directories.", num(dirs * 2, 12)));
    outln(&alloc::format!("{} kilobytes are in {files} user files.", num(file_kb, 12)));
    outln(&alloc::format!("{} kilobytes are available for use.", kb(i.free_sectors)));
    if bad > 0 { 1 } else { 0 }
}
