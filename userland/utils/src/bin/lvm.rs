//! LVM: list and change the partitions and volumes of the hard disks
//! (crates/lvm: MBR with OS/2 LVM drive letter tables, and GPT).
//!
//! Commands as IBM's LVM takes them on its command line, several
//! separated by `;`, or typed at the `LVM>` prompt when it runs without
//! any (the leading `/` may be left out there):
//!
//! ```text
//! /QUERY[:ALL|VOLUMES|PRIMARY|LOGICAL|FREESPACE|COMPATIBILITY|LVM|BOOTABLE[,drive]]
//! /CREATE:PARTITION,name,drive,size,PRIMARY|LOGICAL[,FROMSTART|FROMEND]
//!         [,BESTFIT|FIRSTFIT|LASTFIT|FROMLARGEST|FROMSMALLEST|free space id]
//! /CREATE:VOLUME,COMPATIBILITY,BOOTOS2|BOOTDOS|NOBOOT,letter|*|NONE,volume,drive,partition
//! /DELETE:PARTITION,drive,name   /DELETE:VOLUME,name   /DELETE:ALL,PRIMARY|LOGICAL|VOLUMES,drive
//! /SETNAME:DRIVE,old,new   /SETNAME:PARTITION,drive,old,new   /SETNAME:VOLUME,old,new
//! /SETSTARTABLE:VOLUME,name   /SETSTARTABLE:PARTITION,drive,name
//! /HIDE:VOLUME,name           (takes the volume's drive letter away)
//! /SETLETTER:VOLUME,name,letter   (rs2: change a volume's drive letter)
//! /NEWMBR:drive               (an empty partition table: everything is lost)
//! ```
//!
//! Drives are numbers from 1 or their names (`[ D1 ]`), sizes megabytes
//! (0 or ALL: all of the free space). New partitions get type 06 (FAT;
//! FORMAT writes the file system); compatibility volumes on MBR disks get
//! their letter in the partition's DLA entry, which rs2's kernel honours
//! at the next start. GPT disks hold partitions but no letters.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use lvm::{Error, Free, Kind, Layout, Scheme};
use utils::phys::{self, PhysDisk};
use utils::*;
os2::entry!(main);

struct Ctx {
    seed: u32,
    changed: bool,
}

fn fail(msg: &str) -> u32 {
    eprint(&alloc::format!("LVM: {msg}\r\n"));
    1
}

fn lvm_err(e: Error) -> u32 {
    fail(match e {
        Error::Io(_) => "The disk could not be read or written.",
        Error::NoSpace => "There is not enough free space there.",
        Error::NotFound => "No such partition or volume.",
        Error::TableFull => "The partition table is full.",
        Error::InUse => "That name or drive letter is already in use.",
        Error::NotSupported => "That cannot be done on this disk or partition.",
        Error::Corrupt => "The partition table of the disk is damaged.",
    })
}

fn open(n: u32) -> Result<(PhysDisk, Layout), u32> {
    let d = PhysDisk::open(n).map_err(|_| fail("There is no such drive."))?;
    let mut l = Layout::read(&d).map_err(lvm_err)?;
    // IBM's LVM names disks by their number until they are renamed.
    if l.dla.disk_name.is_empty() {
        l.dla.disk_name = alloc::format!("[ D{n} ]");
    }
    Ok((d, l))
}

/// A drive argument: a number or a drive name.
fn drive_no(s: &str) -> Option<u32> {
    let s = s.trim().trim_matches('"');
    if let Ok(n) = s.parse::<u32>() {
        return (1..=phys::count()).contains(&n).then_some(n);
    }
    (1..=phys::count()).find(|&n| open(n).is_ok_and(|(_, l)| drive_name(n, &l).eq_ignore_ascii_case(s)))
}

fn drive_name(n: u32, l: &Layout) -> String {
    if l.dla.disk_name.is_empty() { alloc::format!("[ D{n} ]") } else { l.dla.disk_name.clone() }
}

fn letter_text(l: u8) -> String {
    match l {
        0 => String::from("--"),
        b'*' => String::from("*"),
        c => alloc::format!("{}:", c as char),
    }
}

fn show(n: u32, l: &Layout, what: &str) {
    let scheme = match l.scheme {
        Scheme::Empty => "no partition table",
        Scheme::Mbr => "MBR",
        Scheme::Gpt => "GPT",
    };
    outln(&alloc::format!("\r\nDrive {n}: {}  {} MB, {scheme}", drive_name(n, l), l.sectors / 2048));
    if what == "FREESPACE" {
        for (i, f) in free_list(l).iter().enumerate() {
            outln(&alloc::format!("  Free space {i}: {:>8} MB at {:>8} MB ({})", f.0.count / 2048, f.0.start / 2048, if f.1 { "logical" } else { "primary" }));
        }
        return;
    }
    outln("  Volume                Letter  Partition             Type          Kind      Start MB   Size MB");
    for p in &l.parts {
        let keep = match what {
            "PRIMARY" => p.kind != Kind::Logical,
            "LOGICAL" => p.kind == Kind::Logical,
            "VOLUMES" | "COMPATIBILITY" => p.volume().is_some(),
            "LVM" => p.mbr_type == 0x35,
            "BOOTABLE" => p.active,
            _ => true,
        };
        if !keep {
            continue;
        }
        let (vname, letter) = match p.volume() {
            Some(v) => (v.volume_name.clone(), letter_text(v.letter)),
            None => (String::from("-"), String::from("--")),
        };
        let kind = match p.kind {
            Kind::Primary => "Primary",
            Kind::Logical => "Logical",
            Kind::Gpt => "GPT",
        };
        let pname = if p.name.is_empty() { String::from("-") } else { p.name.clone() };
        outln(&alloc::format!(
            "  {vname:<20}  {letter:<6}  {pname:<20}  {:<12}  {kind:<8}{} {:>8}  {:>8}",
            p.type_name(),
            if p.active { "*" } else { " " },
            p.start / 2048,
            p.count / 2048
        ));
    }
    if what == "ALL" {
        for (i, f) in free_list(l).iter().enumerate() {
            outln(&alloc::format!("  [ free space {i} ]{:>58} {:>8}  {:>8}", if f.1 { "logical " } else { "primary " }, f.0.start / 2048, f.0.count / 2048));
        }
    }
}

/// Free stretches of a disk, primary ones then logical ones that differ.
fn free_list(l: &Layout) -> Vec<(Free, bool)> {
    let mut v: Vec<(Free, bool)> = l.free(false).into_iter().map(|f| (f, false)).collect();
    if l.scheme != Scheme::Gpt {
        for f in l.free(true) {
            if !v.iter().any(|(g, _)| g.start <= f.start && f.start < g.start + g.count) {
                v.push((f, true));
            }
        }
    }
    v
}

fn save(c: &mut Ctx, d: &PhysDisk, l: &Layout) -> u32 {
    match l.write(d, &mut c.seed) {
        Ok(()) => {
            c.changed = true;
            0
        }
        Err(e) => lvm_err(e),
    }
}

fn args_of(s: &str) -> Vec<String> {
    s.split(',').map(|x| String::from(x.trim().trim_matches('"'))).collect()
}

fn command(c: &mut Ctx, cmd: &str) -> u32 {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return 0;
    }
    let body = cmd.strip_prefix('/').unwrap_or(cmd);
    let (verb, rest) = body.split_once(':').unwrap_or((body, ""));
    let a = args_of(rest);
    let up = |i: usize| a.get(i).map(|s| s.to_ascii_uppercase()).unwrap_or_default();
    match verb.to_ascii_uppercase().as_str() {
        "QUERY" => {
            let what = if rest.is_empty() { String::from("ALL") } else { up(0) };
            let drives: Vec<u32> = match a.get(1) {
                Some(d) => match drive_no(d) {
                    Some(n) => alloc::vec![n],
                    None => return fail("There is no such drive."),
                },
                None => (1..=phys::count()).collect(),
            };
            if drives.is_empty() {
                outln("There are no hard disks.");
            }
            for n in drives {
                match open(n) {
                    Ok((_, l)) => show(n, &l, &what),
                    Err(e) => return e,
                }
            }
            0
        }
        "CREATE" if up(0) == "PARTITION" => {
            let (name, size) = (a.get(1).cloned().unwrap_or_default(), up(3));
            let Some(n) = a.get(2).and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
            let logical = match up(4).as_str() {
                "LOGICAL" => true,
                "PRIMARY" => false,
                _ => return fail("Expected LOGICAL or PRIMARY."),
            };
            let mb = if size == "ALL" { 0 } else {
                match size.parse::<u64>() {
                    Ok(v) => v,
                    Err(_) => return fail("Expected a size in megabytes."),
                }
            };
            let from_end = up(5) == "FROMEND";
            let alg = if up(5) == "FROMEND" || up(5) == "FROMSTART" { up(6) } else { up(5) };
            let Ok((d, mut l)) = open(n) else { return 1 };
            if l.partition_index(&name).is_some() {
                return lvm_err(Error::InUse);
            }
            let need = mb * 2048;
            let mut fr: Vec<Free> = l.free(logical).into_iter().filter(|f| mb == 0 || f.count + lvm::CYLINDER >= need).collect();
            let pick = match alg.as_str() {
                "" | "FIRSTFIT" => fr.first().copied(),
                "LASTFIT" => fr.last().copied(),
                "BESTFIT" => {
                    fr.sort_by_key(|f| f.count);
                    fr.first().copied()
                }
                "FROMSMALLEST" => fr.iter().min_by_key(|f| f.count).copied(),
                "FROMLARGEST" => fr.iter().max_by_key(|f| f.count).copied(),
                id => match id.parse::<usize>() {
                    Ok(i) => free_list(&l).get(i).filter(|f| f.1 == logical || !f.1).map(|f| f.0),
                    Err(_) => return fail("Expected an allocation method or a free space number."),
                },
            };
            let Some(f) = pick else { return lvm_err(Error::NoSpace) };
            match l.create(f, mb, logical, from_end, 0x06, &name, &mut c.seed) {
                Ok(_) => save(c, &d, &l),
                Err(e) => lvm_err(e),
            }
        }
        "CREATE" if up(0) == "VOLUME" => {
            if up(1) == "LVM" {
                return fail("LVM volumes (several partitions) are not supported; use COMPATIBILITY.");
            }
            if up(1) != "COMPATIBILITY" {
                return fail("Expected COMPATIBILITY or LVM.");
            }
            let letter = match up(3).trim_end_matches(':') {
                "*" => b'*',
                "NONE" | "" => 0,
                l if l.len() == 1 && l.as_bytes()[0].is_ascii_alphabetic() && l.as_bytes()[0] >= b'C' => l.as_bytes()[0],
                _ => return fail("Expected a drive letter (e.g. E:)."),
            };
            let vname = a.get(4).cloned().unwrap_or_default();
            let Some(n) = a.get(5).and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
            let Ok((d, mut l)) = open(n) else { return 1 };
            let Some(i) = a.get(6).and_then(|p| l.partition_index(p)) else { return lvm_err(Error::NotFound) };
            if let Err(e) = l.set_volume(i, &vname, letter, &mut c.seed) {
                return lvm_err(e);
            }
            if up(2) == "BOOTOS2" || up(2) == "BOOTDOS" {
                if let Some(v) = l.parts[i].dla.as_mut() {
                    v.installable = up(2) == "BOOTOS2";
                }
                let _ = l.set_active(i);
            }
            save(c, &d, &l)
        }
        "DELETE" => match up(0).as_str() {
            "PARTITION" => {
                let Some(n) = a.get(1).and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
                let Ok((d, mut l)) = open(n) else { return 1 };
                let Some(i) = a.get(2).and_then(|p| l.partition_index(p)) else { return lvm_err(Error::NotFound) };
                let _ = l.delete(i);
                save(c, &d, &l)
            }
            "VOLUME" => with_volume(c, a.get(1).map(String::as_str).unwrap_or(""), |l, i, _| l.delete_volume(i)),
            "ALL" => {
                let Some(n) = a.get(2).and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
                let Ok((d, mut l)) = open(n) else { return 1 };
                match up(1).as_str() {
                    "PRIMARY" => l.parts.retain(|p| p.kind == Kind::Logical),
                    "LOGICAL" => l.parts.retain(|p| p.kind != Kind::Logical),
                    "VOLUMES" | "COMPATIBILITY" => {
                        for i in 0..l.parts.len() {
                            let _ = l.delete_volume(i);
                        }
                    }
                    _ => return fail("Expected PRIMARY, LOGICAL or VOLUMES."),
                }
                save(c, &d, &l)
            }
            _ => fail("Expected ALL, PARTITION or VOLUME."),
        },
        "SETNAME" => match up(0).as_str() {
            "VOLUME" => {
                let new = a.get(2).cloned().unwrap_or_default();
                with_volume(c, a.get(1).map(String::as_str).unwrap_or(""), |l, i, seed| {
                    let letter = l.parts[i].volume().map_or(0, |v| v.letter);
                    l.set_volume(i, &new, letter, seed)
                })
            }
            "PARTITION" => {
                let Some(n) = a.get(1).and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
                let Ok((d, mut l)) = open(n) else { return 1 };
                let Some(i) = a.get(2).and_then(|p| l.partition_index(p)) else { return lvm_err(Error::NotFound) };
                if let Err(e) = l.rename_partition(i, a.get(3).map(String::as_str).unwrap_or("")) {
                    return lvm_err(e);
                }
                save(c, &d, &l)
            }
            "DRIVE" => {
                let Some(n) = a.get(1).and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
                let Ok((d, mut l)) = open(n) else { return 1 };
                if l.scheme != Scheme::Mbr {
                    return lvm_err(Error::NotSupported);
                }
                l.dla.disk_name = a.get(2).cloned().unwrap_or_default();
                save(c, &d, &l)
            }
            _ => fail("Expected VOLUME, PARTITION or DRIVE."),
        },
        "SETSTARTABLE" => match up(0).as_str() {
            "VOLUME" => with_volume(c, a.get(1).map(String::as_str).unwrap_or(""), |l, i, _| l.set_active(i)),
            "PARTITION" => {
                let Some(n) = a.get(1).and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
                let Ok((d, mut l)) = open(n) else { return 1 };
                let Some(i) = a.get(2).and_then(|p| l.partition_index(p)) else { return lvm_err(Error::NotFound) };
                if let Err(e) = l.set_active(i) {
                    return lvm_err(e);
                }
                save(c, &d, &l)
            }
            _ => fail("Expected VOLUME or PARTITION."),
        },
        "HIDE" => with_volume(c, a.get(1).map(String::as_str).unwrap_or(""), |l, i, seed| {
            let name = l.parts[i].volume().map(|v| v.volume_name.clone()).unwrap_or_default();
            l.set_volume(i, &name, 0, seed)
        }),
        "SETLETTER" => {
            let letter = match up(2).trim_end_matches(':') {
                "*" => b'*',
                "NONE" => 0,
                x if x.len() == 1 && x.as_bytes()[0].is_ascii_alphabetic() && x.as_bytes()[0] >= b'C' => x.as_bytes()[0],
                _ => return fail("Expected a drive letter (e.g. E:)."),
            };
            with_volume(c, a.get(1).map(String::as_str).unwrap_or(""), |l, i, seed| {
                let name = l.parts[i].volume().map(|v| v.volume_name.clone()).unwrap_or_default();
                l.set_volume(i, &name, letter, seed)
            })
        }
        "NEWMBR" => {
            let Some(n) = a.first().and_then(|d| drive_no(d)) else { return fail("There is no such drive.") };
            let Ok(d) = PhysDisk::open(n) else { return fail("There is no such drive.") };
            match Layout::new_mbr(&d) {
                Ok(()) => {
                    c.changed = true;
                    0
                }
                Err(e) => lvm_err(e),
            }
        }
        "BOOTMGR" => fail("rs2 starts from its own UEFI loader; Boot Manager cannot be installed."),
        "STARTLOG" | "FILE" | "SI" => fail("That command is not supported by this LVM."),
        _ => fail("Expected an LVM command: /QUERY, /CREATE, /DELETE, /SETNAME, /SETSTARTABLE, /HIDE, /SETLETTER or /NEWMBR."),
    }
}

/// Apply `f` to the disk holding volume `name` and save it.
fn with_volume(c: &mut Ctx, name: &str, f: impl Fn(&mut Layout, usize, &mut u32) -> lvm::Result<()>) -> u32 {
    for n in 1..=phys::count() {
        let Ok((d, mut l)) = open(n) else { continue };
        if let Some(i) = l.volume_index(name) {
            if let Err(e) = f(&mut l, i, &mut c.seed) {
                return lvm_err(e);
            }
            return save(c, &d, &l);
        }
    }
    lvm_err(Error::NotFound)
}

fn main(s: &os2::Startup) -> u32 {
    let line: String = s.args().iter().map(|&b| b as char).collect();
    let dt = files::now();
    let mut c = Ctx { seed: (dt.hundredths as u32) << 24 ^ (dt.seconds as u32) << 16 ^ (dt.minutes as u32) << 8 ^ dt.hours as u32 ^ 0x5eed, changed: false };
    let mut rc = 0;
    if line.trim().is_empty() {
        // Interactive: show the disks, then take commands.
        command(&mut c, "/QUERY");
        loop {
            let l = ask("\r\nLVM> ");
            let u = l.to_ascii_uppercase();
            if u.is_empty() && !stdin_is_console() || u == "EXIT" || u == "QUIT" {
                break;
            }
            if u == "HELP" || u == "?" {
                outln("Commands: QUERY[:what], CREATE:PARTITION,..., CREATE:VOLUME,..., DELETE:..., SETNAME:...,");
                outln("SETSTARTABLE:..., HIDE:VOLUME,name, SETLETTER:VOLUME,name,letter, NEWMBR:drive, EXIT");
                continue;
            }
            for cmd in l.split(';') {
                rc = command(&mut c, cmd);
            }
        }
    } else {
        for cmd in line.split(';') {
            rc = command(&mut c, cmd);
            if rc != 0 {
                break;
            }
        }
    }
    if c.changed {
        outln("The partition tables were changed; restart the system for the changes to take effect.");
    }
    rc
}
