//! FORMAT: create a file system. FORMAT drive: [/FS:FAT|FAT32|HPFS|JFS]
//! [/V[:label]] [/Q] [/ONCE] [/F:size]
//!
//! A diskette (DSK_BLOCKREMOVABLE) is formatted track by track with
//! DSK_FORMATVERIFY in the drive's own format or the /F: one (360, 720,
//! 1200, 1440, 2880 KB, set with DSK_SETDEVICEPARAMS), and gets FAT12
//! with the standard layout of its size; FORMAT then offers to format
//! another unless /ONCE is given.
//!
//! The volume is locked and written through DASD track IOCtls (the same
//! FAT and HPFS code the kernel uses), then the system is told to
//! redetermine the medium so the new file system is mounted.
//!
//! /P reports to the Presentation Manager front end (PMFORMAT runs
//! `FORMAT.COM d: /P /V:"label" [/ONCE] [/FS:x]` with its stdin and
//! stdout on pipes), as IBM's FORMAT does: every message goes out as a
//! line `MSG_<number>,<parameter>` (OSO001.MSG numbers), the protocol
//! CHKDSK /P uses for PMCHKDSK. PMFORMAT reads the percentage from 1312
//! (or 538), 1294 (or 635) as 100 %, 1283 and 1286 (total and available
//! bytes; 1286 ends the dialog's wait), answers 1271 (the warning) and
//! 1309 with `Y` and 1288 (the label prompt) with Enter, and shows its
//! own text for the errors (1276, 1279, 1281, 1303, 1313, 1319, 19, 123,
//! 540, ...) before stopping FORMAT.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use os2::*;
use utils::disk::{fs_name, Disk};
use utils::*;
use core::sync::atomic::{AtomicBool, Ordering};
os2::entry!(main);

static PM: AtomicBool = AtomicBool::new(false);

fn pm() -> bool {
    PM.load(Ordering::Relaxed)
}

/// /P: one message line for PMFORMAT.
fn pm_msg(n: u32, param: &str) {
    if param.is_empty() {
        outln(&alloc::format!("MSG_{n}"));
    } else {
        outln(&alloc::format!("MSG_{n},{param}"));
    }
}

/// An error: the text on the console, its number for PMFORMAT.
fn fail(n: u32, param: &str, text: &str) {
    if pm() {
        pm_msg(n, param);
    } else {
        eprint(text);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Fs {
    Fat,
    Fat32,
    Hpfs,
    Jfs,
}

fn usage_error() -> u32 {
    fail(1276, "", "SYS1276: The FORMAT command has detected an incorrect parameter.\r\n");
    87
}

fn main(s: &os2::Startup) -> u32 {
    let mut drive = None;
    let mut fs = None;
    let mut label: Option<String> = None;
    let mut quick = false;
    let mut once = false;
    let mut size: Option<u32> = None;
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        if let Some(v) = u.strip_prefix("/FS:") {
            fs = Some(match v {
                "FAT" => Fs::Fat,
                "FAT32" => Fs::Fat32,
                "HPFS" => Fs::Hpfs,
                "JFS" => Fs::Jfs,
                _ => {
                    eprint(&alloc::format!("SYS1330: The system cannot accept the {u} parameter.\r\n"));
                    return 87;
                }
            });
        } else if u == "/V" {
            label = None;
        } else if u.starts_with("/V:") {
            label = Some(String::from(&a[3..]));
        } else if u == "/P" {
            PM.store(true, Ordering::Relaxed);
        } else if u == "/Q" {
            quick = true;
        } else if u == "/ONCE" {
            once = true;
        } else if let Some(v) = u.strip_prefix("/F:") {
            // Diskette size in KB (/F:1.44 and the like too).
            size = Some(match v.trim_end_matches("KB").trim_end_matches('K') {
                "360" => 360,
                "720" => 720,
                "1200" | "1.2" => 1200,
                "1440" | "1.44" => 1440,
                "2880" | "2.88" => 2880,
                _ => return usage_error(),
            });
        } else if u == "/L" || u.starts_with("/BS:") {
            // Optical options: nothing to do on a hard disk.
        } else if u.starts_with('/') {
            return usage_error();
        } else if drive.is_none() {
            drive = Some(a);
        } else {
            return usage_error();
        }
    }
    let Some(dname) = drive else {
        eprint("SYS1310: A drive letter must be specified.\r\n");
        return 87;
    };
    let Some(d) = drive_arg(Some(&dname)) else { return usage_error() };
    let letter = (b'A' + d) as char;
    let mut boot = 0u32;
    unsafe { DosQuerySysInfo(5, 5, &mut boot, 4) };
    if boot == d as u32 + 1 {
        fail(1303, &alloc::format!("{letter}:"), &alloc::format!("SYS1303: The system is currently unable to format drive {letter}:\r\n(it is the drive the system was started from).\r\n"));
        return 5;
    }
    let cur = fs_name(d);
    let Some(cur) = cur else {
        error(15);
        return 15;
    };
    if cur != "UNKNOWN" && !pm() {
        outln(&alloc::format!("The type of file system for the disk is {cur}."));
    }
    let disk = match Disk::open(d, true) {
        Ok(x) => x,
        Err(rc) => {
            if pm() {
                pm_msg(rc, "");
            } else {
                error(rc);
            }
            return rc;
        }
    };
    if disk.removable() {
        return format_diskettes(d, disk, size, label, quick, once);
    }
    let bytes = disk.sectors * disk.bps as u64;
    let fs = fs.unwrap_or(match cur.as_str() {
        "HPFS" => Fs::Hpfs,
        "JFS" => Fs::Jfs,
        _ => Fs::Fat,
    });
    // Hard disks: confirm with the current label, then the warning
    // (PMFORMAT asks its own questions: no label check with /P).
    if let Ok((cur_label, _)) = volume_label(d).map(|v| if pm() { (String::new(), v.1) } else { v }) {
        if !cur_label.is_empty() {
            let typed = ask(&alloc::format!("Enter the current volume label for drive {letter}: "));
            if !typed.eq_ignore_ascii_case(&cur_label) {
                eprint(&alloc::format!("SYS1319: An incorrect volume label was entered for drive {letter}:.\r\n"));
                return 1;
            }
        }
    }
    let go = if pm() {
        pm_msg(1271, &alloc::format!("{letter}:"));
        matches!(ask("").chars().next(), Some('Y' | 'y'))
    } else {
        yes(&alloc::format!("\r\nWarning!  All data on hard disk {letter}: will be lost!\r\nProceed with FORMAT (Y/N)? "))
    };
    if !go {
        return 0;
    }
    if disk.lock() != 0 {
        fail(1303, &alloc::format!("{letter}:"), &alloc::format!("SYS1303: The system is currently unable to format drive {letter}:\r\n(files on it are in use).\r\n"));
        return 108;
    }
    if !quick && !verify(&disk) {
        disk.unlock();
        fail(1279, "", "SYS1279: The format was unsuccessful.\r\n");
        return 1;
    }
    let lab = match &label {
        Some(l) => l.to_ascii_uppercase(),
        None => String::new(),
    };
    let lab = &lab[..lab.len().min(11)];
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    let ser = ((dt.day as u32) << 24 | (dt.month as u32) << 16 | (dt.seconds as u32) << 8 | dt.hundredths as u32)
        .wrapping_add((dt.year as u32) << 16 | (dt.hours as u32) << 8 | dt.minutes as u32);
    let hidden = disk.bpb.hidden_sectors;
    let ok = match fs {
        Fs::Fat | Fs::Fat32 => {
            let mb = bytes >> 20;
            let ty = match fs {
                Fs::Fat32 => fat::FatType::Fat32,
                _ if mb < 16 => fat::FatType::Fat12,
                _ if mb < 2048 => fat::FatType::Fat16,
                _ => fat::FatType::Fat32,
            };
            fat::format(&&disk, &fat::FormatParams { label: lab, serial: ser, fat_type: Some(ty), hidden_sectors: hidden }).is_ok()
        }
        Fs::Hpfs => {
            if bytes > 64 << 30 {
                disk.unlock();
                fail(1295, "", "SYS1295: The volume is larger than 64 gigabytes, the HPFS limit.\r\n");
                return 1;
            }
            let now = hpfs::time::from_dos(
                fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds).date,
                fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds).time,
            );
            let p = hpfs::FormatParams { label: lab, serial: ser, hidden_sectors: hidden, code_page: 850, now };
            hpfs::format(&&disk, &p).is_ok()
        }
        Fs::Jfs => {
            if bytes < jfs::MIN_BYTES {
                disk.unlock();
                fail(1279, "", "SYS1295: The volume is too small for JFS (16 MB at least).\r\n");
                return 1;
            }
            let now = hpfs::time::from_dos(
                fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds).date,
                fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds).time,
            );
            let p = jfs::FormatParams { label: lab, serial: ser, hidden_sectors: hidden, now, ..Default::default() };
            jfs::format(&&disk, &p).is_ok()
        }
    };
    // Mount the new file system (also after a failure: the old one is gone).
    disk.redetermine();
    if !ok {
        disk.unlock();
        fail(1279, "", "SYS1296: FORMAT cannot write the file system to the target disk.\r\n");
        return 1;
    }
    let name = match fs {
        Fs::Hpfs => "HPFS",
        Fs::Jfs => "JFS",
        _ => "FAT",
    };
    if pm() {
        pm_msg(1294, "");
    } else {
        outln("\r\n\r\nFormat completed successfully.\r\n");
        outln(&alloc::format!("The new type of file system is {name}.\r\n"));
    }
    if label.is_none() {
        let l = if pm() {
            pm_msg(1288, "");
            ask("")
        } else {
            ask("Enter up to 11 characters for the volume label\r\nor press Enter for no volume label. ")
        };
        if !l.is_empty() {
            let rc = set_volume_label(d, &l.to_ascii_uppercase());
            if rc != 0 {
                error(rc);
            }
        }
    }
    let mut a = FsAllocate::default();
    let rc = unsafe { DosQueryFSInfo(d as u32 + 1, FSIL_ALLOC, &mut a as *mut _ as *mut u8, core::mem::size_of::<FsAllocate>() as u32) };
    if rc == 0 {
        let unit = a.sectors_per_unit as u64 * a.bytes_per_sector as u64;
        if pm() {
            // 1286 last: PMFORMAT stops reading after it.
            pm_msg(1283, &alloc::format!("{}", a.units as u64 * unit));
            pm_msg(1286, &alloc::format!("{}", a.units_avail as u64 * unit));
            disk.unlock();
            return 0;
        }
        outln(&alloc::format!("\r\n{} bytes total disk space", num(a.units as u64 * unit, 12)));
        outln(&alloc::format!("{} bytes available on disk", num(a.units_avail as u64 * unit, 12)));
        outln(&alloc::format!("\r\n{} bytes in each allocation unit.", num(unit, 12)));
        outln(&alloc::format!("{} total allocation units.", num(a.units as u64, 12)));
        outln(&alloc::format!("{} available allocation units on disk.", num(a.units_avail as u64, 12)));
    }
    if let Ok((_, s)) = volume_label(d) {
        outln(&alloc::format!("\r\nThe Volume Serial Number is {}.", serial(s)));
    }
    disk.unlock();
    0
}

/// The diskette formats FORMAT /F: knows: size in KB, sectors, sectors
/// per cluster, root entries, media byte, sectors per FAT, sectors per
/// track.
const FORMATS: [(u32, u16, u8, u16, u8, u16, u16); 5] = [
    (360, 720, 2, 112, 0xfd, 2, 9),
    (720, 1440, 2, 112, 0xf9, 3, 9),
    (1200, 2400, 1, 224, 0xf9, 7, 15),
    (1440, 2880, 1, 224, 0xf0, 9, 18),
    (2880, 5760, 2, 240, 0xf0, 9, 36),
];

/// The standard BPB of a diskette of `sectors` sectors.
fn diskette_bpb(sectors: u16) -> Option<BiosParameterBlock> {
    let f = FORMATS.iter().find(|f| f.1 == sectors)?;
    Some(BiosParameterBlock {
        bytes_per_sector: 512,
        sectors_per_cluster: f.2,
        reserved_sectors: 1,
        fats: 2,
        root_entries: f.3,
        sectors: f.1,
        media: f.4,
        sectors_per_fat: f.5,
        sectors_per_track: f.6,
        heads: 2,
        ..Default::default()
    })
}

/// Write an empty FAT12 file system in the diskette format `b`.
fn write_fat12(disk: &Disk, b: &BiosParameterBlock, label: &str, serial: u32) -> Result<(), u32> {
    let mut bs = alloc::vec![0u8; 512];
    bs[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
    bs[3..11].copy_from_slice(b"RS2 4.52");
    bs[11..13].copy_from_slice(&512u16.to_le_bytes());
    bs[13] = b.sectors_per_cluster;
    bs[14..16].copy_from_slice(&1u16.to_le_bytes());
    bs[16] = 2;
    bs[17..19].copy_from_slice(&{ b.root_entries }.to_le_bytes());
    bs[19..21].copy_from_slice(&{ b.sectors }.to_le_bytes());
    bs[21] = b.media;
    bs[22..24].copy_from_slice(&{ b.sectors_per_fat }.to_le_bytes());
    bs[24..26].copy_from_slice(&{ b.sectors_per_track }.to_le_bytes());
    bs[26..28].copy_from_slice(&{ b.heads }.to_le_bytes());
    bs[38] = 0x29;
    bs[39..43].copy_from_slice(&serial.to_le_bytes());
    let mut lab = *b"NO NAME    ";
    if !label.is_empty() {
        lab = [b' '; 11];
        for (d, s) in lab.iter_mut().zip(label.bytes()) {
            *d = s;
        }
    }
    bs[43..54].copy_from_slice(&lab);
    bs[54..62].copy_from_slice(b"FAT12   ");
    bs[510] = 0x55;
    bs[511] = 0xaa;
    disk.write(0, &bs)?;
    let spf = b.sectors_per_fat as u64;
    let root = (b.root_entries as u64 * 32).div_ceil(512);
    disk.zero(1, 2 * spf + root)?;
    let mut fat = alloc::vec![0u8; 512];
    fat[0..3].copy_from_slice(&[b.media, 0xff, 0xff]);
    disk.write(1, &fat)?;
    disk.write(1 + spf, &fat)?;
    if !label.is_empty() {
        let mut sec = alloc::vec![0u8; 512];
        sec[0..11].copy_from_slice(&lab);
        sec[11] = 0x08;
        disk.write(1 + 2 * spf, &sec)?;
    }
    Ok(())
}

/// FORMAT on a diskette drive: each diskette is formatted track by
/// track (DSK_FORMATVERIFY) in the drive's own format or the /F: one,
/// then gets an empty FAT file system; then the next, unless /ONCE.
fn format_diskettes(d: u8, mut disk: Disk, size: Option<u32>, label: Option<String>, quick: bool, once: bool) -> u32 {
    let letter = (b'A' + d) as char;
    loop {
        if !pm() {
            ask(&alloc::format!("Insert a new diskette in drive {letter}:\r\nand press Enter when ready..."));
        }
        let native = match disk.device_params(0) {
            Ok(b) => b,
            Err(rc) => {
                error(rc);
                return rc;
            }
        };
        let sectors = match size {
            Some(kb) => match FORMATS.iter().find(|f| f.0 == kb) {
                Some(f) if f.1 <= native.sectors => f.1,
                _ => {
                    fail(1284, "", &alloc::format!("SYS1284: Drive {letter}: cannot format a diskette of {} KB.\r\n", kb));
                    return 1;
                }
            },
            None => native.sectors,
        };
        let Some(bpb) = diskette_bpb(sectors) else {
            fail(1279, "", "SYS1279: The format was unsuccessful.\r\n");
            return 1;
        };
        if disk.lock() != 0 {
            fail(1303, &alloc::format!("{letter}:"), &alloc::format!("SYS1303: The system is currently unable to format drive {letter}:\r\n(files on it are in use).\r\n"));
            return 108;
        }
        let rc = disk.set_medium(bpb);
        if rc != 0 {
            disk.unlock();
            error(rc);
            return rc;
        }
        let tracks = disk.tracks();
        let mut last = u64::MAX;
        for t in 0..if quick { 0 } else { tracks } {
            let pct = t * 100 / tracks;
            if pct != last {
                if pm() {
                    pm_msg(1312, &alloc::format!("{pct}"));
                } else {
                    out(&alloc::format!("\r{pct} percent of disk formatted "));
                }
                last = pct;
            }
            let rc = disk.format_track(t);
            if rc != 0 {
                disk.unlock();
                if !pm() {
                    outln("");
                }
                if rc == 19 {
                    error(rc);
                } else {
                    fail(1279, "", "SYS1279: The format was unsuccessful.\r\n");
                }
                return rc;
            }
        }
        if !pm() && !quick {
            out("\r100 percent of disk formatted ");
        }
        let lab = label.as_deref().map(|l| l.to_ascii_uppercase()).unwrap_or_default();
        let lab = &lab[..lab.len().min(11)];
        let mut dt = DateTime::default();
        unsafe { DosGetDateTime(&mut dt) };
        let ser = ((dt.day as u32) << 24 | (dt.month as u32) << 16 | (dt.seconds as u32) << 8 | dt.hundredths as u32)
            .wrapping_add((dt.year as u32) << 16 | (dt.hours as u32) << 8 | dt.minutes as u32);
        let written = write_fat12(&disk, &bpb, lab, ser);
        disk.redetermine();
        disk.unlock();
        if written.is_err() {
            fail(1279, "", "SYS1296: FORMAT cannot write the file system to the target disk.\r\n");
            return 1;
        }
        if pm() {
            pm_msg(1294, "");
        } else {
            outln("\r\n\r\nFormat completed successfully.\r\n");
        }
        if label.is_none() && !pm() {
            let l = ask("Enter up to 11 characters for the volume label\r\nor press Enter for no volume label. ");
            if !l.is_empty() {
                let rc = set_volume_label(d, &l.to_ascii_uppercase());
                if rc != 0 {
                    error(rc);
                }
            }
        }
        let mut a = FsAllocate::default();
        let rc = unsafe { DosQueryFSInfo(d as u32 + 1, FSIL_ALLOC, &mut a as *mut _ as *mut u8, core::mem::size_of::<FsAllocate>() as u32) };
        if rc == 0 {
            let unit = a.sectors_per_unit as u64 * a.bytes_per_sector as u64;
            if pm() {
                pm_msg(1283, &alloc::format!("{}", a.units as u64 * unit));
                pm_msg(1286, &alloc::format!("{}", a.units_avail as u64 * unit));
                return 0;
            }
            outln(&alloc::format!("\r\n{} bytes total disk space", num(a.units as u64 * unit, 12)));
            outln(&alloc::format!("{} bytes available on disk", num(a.units_avail as u64 * unit, 12)));
            outln(&alloc::format!("\r\n{} bytes in each allocation unit.", num(unit, 12)));
            outln(&alloc::format!("{} total allocation units.", num(a.units as u64, 12)));
            outln(&alloc::format!("{} available allocation units on disk.", num(a.units_avail as u64, 12)));
        }
        if let Ok((_, s)) = volume_label(d) {
            outln(&alloc::format!("\r\nThe Volume Serial Number is {}.", serial(s)));
        }
        if once || pm() || !yes("\r\nFormat another diskette (Y/N)? ") {
            return 0;
        }
    }
}

/// Read every track (what a full format does on a hard disk), showing
/// progress. False if the disk cannot be read.
fn verify(disk: &Disk) -> bool {
    let tracks = disk.sectors.div_ceil(disk.track_sectors());
    let mut buf = alloc::vec::Vec::new();
    let mut last = u64::MAX;
    for t in 0..tracks {
        let pct = t * 100 / tracks;
        if pct != last {
            if pm() {
                pm_msg(1312, &alloc::format!("{pct}"));
            } else {
                out(&alloc::format!("\r{pct} percent of disk formatted "));
            }
            last = pct;
        }
        if disk.read_track(t, &mut buf) != 0 {
            if !pm() {
                outln("");
            }
            return false;
        }
    }
    if pm() {
        pm_msg(1312, "100");
    } else {
        out("\r100 percent of disk formatted ");
    }
    true
}
