//! FORMAT: create a file system. FORMAT drive: [/FS:FAT|FAT32|HPFS]
//! [/V[:label]] [/Q] [/ONCE]
//!
//! The volume is locked and written through DASD track IOCtls (the same
//! FAT and HPFS code the kernel uses), then the system is told to
//! redetermine the medium so the new file system is mounted.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use os2::*;
use utils::disk::{fs_name, Disk};
use utils::*;
os2::entry!(main);

#[derive(Clone, Copy, PartialEq)]
enum Fs {
    Fat,
    Fat32,
    Hpfs,
}

fn usage_error() -> u32 {
    eprint("SYS1276: The FORMAT command has detected an incorrect parameter.\r\n");
    87
}

fn main(s: &os2::Startup) -> u32 {
    let mut drive = None;
    let mut fs = None;
    let mut label: Option<String> = None;
    let mut quick = false;
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        if let Some(v) = u.strip_prefix("/FS:") {
            fs = Some(match v {
                "FAT" => Fs::Fat,
                "FAT32" => Fs::Fat32,
                "HPFS" => Fs::Hpfs,
                _ => {
                    eprint(&alloc::format!("SYS1330: The system cannot accept the {u} parameter.\r\n"));
                    return 87;
                }
            });
        } else if u == "/V" {
            label = None;
        } else if u.starts_with("/V:") {
            label = Some(String::from(&a[3..]));
        } else if u == "/Q" {
            quick = true;
        } else if u == "/ONCE" || u == "/L" || u.starts_with("/BS:") {
            // Diskette and optical options: nothing to do on a hard disk.
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
        eprint(&alloc::format!("SYS1303: The system is currently unable to format drive {letter}:\r\n(it is the drive the system was started from).\r\n"));
        return 5;
    }
    let cur = fs_name(d);
    let Some(cur) = cur else {
        error(15);
        return 15;
    };
    if cur != "UNKNOWN" {
        outln(&alloc::format!("The type of file system for the disk is {cur}."));
    }
    let disk = match Disk::open(d, true) {
        Ok(x) => x,
        Err(rc) => {
            error(rc);
            return rc;
        }
    };
    let bytes = disk.sectors * disk.bps as u64;
    let fs = fs.unwrap_or(if cur == "HPFS" { Fs::Hpfs } else { Fs::Fat });
    // Hard disks: confirm with the current label, then the warning.
    if let Ok((cur_label, _)) = volume_label(d) {
        if !cur_label.is_empty() {
            let typed = ask(&alloc::format!("Enter the current volume label for drive {letter}: "));
            if !typed.eq_ignore_ascii_case(&cur_label) {
                eprint(&alloc::format!("SYS1319: An incorrect volume label was entered for drive {letter}:.\r\n"));
                return 1;
            }
        }
    }
    if !yes(&alloc::format!("\r\nWarning!  All data on hard disk {letter}: will be lost!\r\nProceed with FORMAT (Y/N)? ")) {
        return 0;
    }
    if disk.lock() != 0 {
        eprint(&alloc::format!("SYS1303: The system is currently unable to format drive {letter}:\r\n(files on it are in use).\r\n"));
        return 108;
    }
    if !quick && !verify(&disk) {
        disk.unlock();
        eprint("SYS1279: The format was unsuccessful.\r\n");
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
                eprint("SYS1295: The volume is larger than 64 gigabytes, the HPFS limit.\r\n");
                return 1;
            }
            let now = hpfs::time::from_dos(
                fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds).date,
                fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds).time,
            );
            let p = hpfs::FormatParams { label: lab, serial: ser, hidden_sectors: hidden, code_page: 850, now };
            hpfs::format(&&disk, &p).is_ok()
        }
    };
    // Mount the new file system (also after a failure: the old one is gone).
    disk.redetermine();
    if !ok {
        disk.unlock();
        eprint("SYS1296: FORMAT cannot write the file system to the target disk.\r\n");
        return 1;
    }
    outln("\r\n\r\nFormat completed successfully.\r\n");
    let name = match fs {
        Fs::Hpfs => "HPFS",
        _ => "FAT",
    };
    outln(&alloc::format!("The new type of file system is {name}.\r\n"));
    if label.is_none() {
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

/// Read every track (what a full format does on a hard disk), showing
/// progress. False if the disk cannot be read.
fn verify(disk: &Disk) -> bool {
    let tracks = disk.sectors.div_ceil(disk.track_sectors());
    let mut buf = alloc::vec::Vec::new();
    let mut last = u64::MAX;
    for t in 0..tracks {
        let pct = t * 100 / tracks;
        if pct != last {
            out(&alloc::format!("\r{pct} percent of disk formatted "));
            last = pct;
        }
        if disk.read_track(t, &mut buf) != 0 {
            outln("");
            return false;
        }
    }
    out("\r100 percent of disk formatted ");
    true
}
