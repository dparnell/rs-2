//! rs2 installer.
//!
//! Installs the running live system onto a hard disk: a GPT with one EFI
//! system partition (FAT32) that holds the boot loader, the kernel and the
//! system files. The partition is written directly through the physical
//! disk IOCtls with the same FAT code the build tools use.
//!
//! A disk that already has rs2 (an EFI system partition with
//! `RS2\KERNEL`) can be updated instead: the system files are replaced
//! in place, without partitioning or formatting, and the user's settings
//! are kept (CONFIG.SYS, the INI files, `MPTN\ETC`). CONFIG.SYS gets what
//! the new version adds: `SET` variables it does not have, and entries of
//! PATH, DPATH, LIBPATH, HELP and BOOKSHELF missing from its lines.
//! `RS2\VERSION` (written by the build) names both versions.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

os2::entry!(main);

fn out(s: &str) {
    os2::print(s);
}

fn outln(s: &str) {
    os2::print(s);
    os2::print("\r\n");
}

fn ask(prompt: &str) -> String {
    out(prompt);
    let mut buf = [0u8; 128];
    let mut n = 0;
    unsafe { DosRead(0, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    buf[..n as usize].iter().map(|&b| b as char).collect::<String>().trim().to_string()
}

// ---- Physical disk access (DosPhysicalDisk + DosDevIOCtl category 9) ----

unsafe extern "C" {
    #[link_name = "DOSCALLS.287"]
    fn DosPhysicalDisk(func: u32, buf: *mut u8, cb: u32, params: *const u8, cbp: u32) -> APIRET;
    #[link_name = "DOSCALLS.284"]
    fn DosDevIOCtl(
        h: u32,
        cat: u32,
        func: u32,
        params: *const u8,
        cbp: u32,
        pcbp: *mut u32,
        data: *mut u8,
        cbd: u32,
        pcbd: *mut u32,
    ) -> APIRET;
}

#[derive(Clone, Copy)]
struct Disk {
    handle: u32,
    sectors: u64,
    sector_size: u32,
    /// Offset added to every LBA (partition start).
    offset: u64,
    /// Sectors visible through this view.
    count: u64,
}

impl Disk {
    fn open(n: u32) -> Option<Disk> {
        let name = alloc::format!("{n}:\0");
        let mut h = 0u16;
        let rc = unsafe { DosPhysicalDisk(2, &mut h as *mut u16 as *mut u8, 2, name.as_ptr(), name.len() as u32) };
        if rc != 0 {
            return None;
        }
        let mut info = [0u8; 12];
        let (mut pl, mut dl) = (0, 0);
        let rc = unsafe { DosDevIOCtl(h as u32, 9, 0xa2, core::ptr::null(), 0, &mut pl, info.as_mut_ptr(), 12, &mut dl) };
        if rc != 0 {
            return None;
        }
        let sectors = u64::from_le_bytes(info[0..8].try_into().unwrap());
        let ss = u32::from_le_bytes(info[8..12].try_into().unwrap());
        Some(Disk { handle: h as u32, sectors, sector_size: ss, offset: 0, count: sectors })
    }

    fn io(&self, lba: u64, buf: *mut u8, bytes: usize, write: bool) -> Result<(), u32> {
        let count = (bytes / self.sector_size as usize) as u32;
        let abs = self.offset + lba;
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&(abs as u32).to_le_bytes());
        p[4..8].copy_from_slice(&((abs >> 32) as u32).to_le_bytes());
        p[8..12].copy_from_slice(&count.to_le_bytes());
        let (mut pl, mut dl) = (0, 0);
        let f = if write { 0xa1 } else { 0xa0 };
        let rc = unsafe { DosDevIOCtl(self.handle, 9, f, p.as_ptr(), 12, &mut pl, buf, bytes as u32, &mut dl) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    fn read_at(&self, lba: u64, bytes: usize) -> Result<Vec<u8>, u32> {
        let ss = self.sector_size as usize;
        let mut v = vec![0u8; bytes.div_ceil(ss) * ss];
        self.io(lba, v.as_mut_ptr(), v.len(), false)?;
        v.truncate(bytes);
        Ok(v)
    }

    fn write_at(&self, lba: u64, data: &[u8]) -> Result<(), u32> {
        let mut v = data.to_vec();
        self.io(lba, v.as_mut_ptr(), v.len(), true)
    }
}

impl fat::BlockDevice for Disk {
    fn sector_size(&self) -> usize {
        self.sector_size as usize
    }
    fn sector_count(&self) -> u64 {
        self.count
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> fat::Result<()> {
        self.io(lba, buf.as_mut_ptr(), buf.len(), false).map_err(|_| fat::Error::Io)
    }
    fn write(&self, lba: u64, buf: &[u8]) -> fat::Result<()> {
        self.write_at(lba, buf).map_err(|_| fat::Error::Io)
    }
}

// ---- GPT ----

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

const ESP_TYPE: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];

/// A partition GUID derived from the time of day (good enough for a
/// unique label on one machine).
fn guid(seed: u32) -> [u8; 16] {
    let mut g = [0u8; 16];
    let mut x = seed ^ 0x9e37_79b9;
    for b in g.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x as u8;
    }
    g[7] = (g[7] & 0x0f) | 0x40; // version 4
    g[8] = (g[8] & 0x3f) | 0x80;
    g
}

/// Write a GPT with one EFI system partition; returns (start, count).
fn partition(d: &Disk, seed: u32) -> Result<(u64, u64), u32> {
    let ss = d.sector_size as usize;
    let last = d.sectors - 1;
    let entries_lba = 2u64;
    let entry_sectors = (128 * 128) / ss as u64;
    let first_usable = 2048u64;
    let last_usable = last - entry_sectors - 1;
    let start = first_usable;
    let end = last_usable & !2047; // keep the end aligned
    let end = if end > start { end - 1 } else { last_usable };
    // Protective MBR.
    let mut mbr = vec![0u8; ss];
    mbr[446 + 1..446 + 4].copy_from_slice(&[0, 2, 0]);
    mbr[446 + 4] = 0xee;
    mbr[446 + 5..446 + 8].copy_from_slice(&[0xff, 0xff, 0xff]);
    mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    mbr[446 + 12..446 + 16].copy_from_slice(&((last.min(0xffff_ffff)) as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    d.write_at(0, &mbr)?;
    // Partition entries.
    let mut ents = vec![0u8; (entry_sectors as usize) * ss];
    ents[0..16].copy_from_slice(&ESP_TYPE);
    ents[16..32].copy_from_slice(&guid(seed));
    ents[32..40].copy_from_slice(&start.to_le_bytes());
    ents[40..48].copy_from_slice(&end.to_le_bytes());
    for (i, ch) in "rs2 system".encode_utf16().enumerate() {
        ents[56 + i * 2..58 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    let ents_crc = crc32(&ents[..128 * 128]);
    let header = |my: u64, alt: u64, ents_at: u64| -> Vec<u8> {
        let mut h = vec![0u8; ss];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&my.to_le_bytes());
        h[32..40].copy_from_slice(&alt.to_le_bytes());
        h[40..48].copy_from_slice(&first_usable.to_le_bytes());
        h[48..56].copy_from_slice(&last_usable.to_le_bytes());
        h[56..72].copy_from_slice(&guid(seed.wrapping_add(1)));
        h[72..80].copy_from_slice(&ents_at.to_le_bytes());
        h[80..84].copy_from_slice(&128u32.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&ents_crc.to_le_bytes());
        let c = crc32(&h[..92]);
        h[16..20].copy_from_slice(&c.to_le_bytes());
        h
    };
    d.write_at(entries_lba, &ents)?;
    d.write_at(1, &header(1, last, entries_lba))?;
    let backup_ents = last - entry_sectors;
    d.write_at(backup_ents, &ents)?;
    d.write_at(last, &header(last, 1, backup_ents))?;
    Ok((start, end - start + 1))
}

// ---- An existing installation ----

/// rs2 found on a disk: its partition and version.
struct Found {
    part: Disk,
    version: String,
}

fn read_vol(vol: &fat::Volume<Disk>, path: &str) -> Option<Vec<u8>> {
    let e = vol.lookup(path).ok()?;
    let mut v = vec![0u8; e.size as usize];
    let n = vol.read(&e, 0, &mut v).ok()?;
    v.truncate(n);
    Some(v)
}

fn version_of(text: Option<Vec<u8>>) -> String {
    match text {
        Some(t) => t.iter().map(|&b| b as char).collect::<String>().trim().to_string(),
        None => String::from("an earlier rs2"),
    }
}

/// The rs2 installation on disk `d`, if it has one: an EFI system
/// partition of its GPT holding RS2\KERNEL.
fn find_rs2(d: &Disk) -> Option<Found> {
    let h = d.read_at(1, d.sector_size as usize).ok()?;
    if &h[0..8] != b"EFI PART" {
        return None;
    }
    let u64_at = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let (lba, n, size) = (u64_at(&h, 72), u32_at(&h, 80).min(256) as usize, u32_at(&h, 84) as usize);
    if size < 128 {
        return None;
    }
    let ents = d.read_at(lba, n * size).ok()?;
    for e in ents.chunks_exact(size) {
        if e[0..16] != ESP_TYPE {
            continue;
        }
        let (start, end) = (u64_at(e, 32), u64_at(e, 40));
        if end < start || end >= d.sectors {
            continue;
        }
        let part = Disk { offset: start, count: end - start + 1, ..*d };
        let Ok(vol) = fat::Volume::mount(part) else { continue };
        if vol.lookup("RS2\\KERNEL").is_ok() {
            return Some(Found { part, version: version_of(read_vol(&vol, "RS2\\VERSION")) });
        }
    }
    None
}

/// Files of an installation that are the user's: kept when updating.
fn kept(dest: &str) -> bool {
    let u = dest.to_ascii_uppercase();
    u == "CONFIG.SYS" || u.ends_with(".INI") || u.starts_with("MPTN\\ETC\\")
}

/// CONFIG.SYS with what `new` adds to `old`: SET variables `old` does not
/// set, and the entries of its path lists that `old`'s lines lack.
/// Returns the text and what was added.
fn merge_config(old: &[u8], new: &[u8]) -> (Vec<u8>, Vec<String>) {
    let text = |b: &[u8]| -> Vec<String> { b.split(|&c| c == b'\n').map(|l| l.iter().map(|&c| c as char).collect::<String>().trim_end_matches('\r').to_string()).collect() };
    let key = |l: &str| -> Option<String> {
        let u = l.trim().to_ascii_uppercase();
        if u.starts_with("REM") {
            return None;
        }
        let k = u.split('=').next()?.trim().to_string();
        (u.contains('=') && (k.starts_with("SET ") || k == "LIBPATH")).then_some(k)
    };
    const LISTS: [&str; 5] = ["SET PATH", "SET DPATH", "LIBPATH", "SET HELP", "SET BOOKSHELF"];
    let mut lines = text(old);
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut notes = Vec::new();
    for nl in text(new) {
        let Some(k) = key(&nl) else { continue };
        let value = nl.split_once('=').map_or("", |x| x.1).trim().to_string();
        match lines.iter().position(|l| key(l).as_deref() == Some(k.as_str())) {
            None => {
                notes.push(alloc::format!("added {}", nl.trim()));
                lines.push(nl.trim().to_string());
            }
            Some(i) if LISTS.contains(&k.as_str()) => {
                let mut l = lines[i].trim_end().to_string();
                let have: Vec<String> = l.split_once('=').map_or("", |x| x.1).split(';').map(|e| e.trim().to_ascii_uppercase()).collect();
                for entry in value.split(';').map(str::trim).filter(|e| !e.is_empty()) {
                    if !have.contains(&entry.to_ascii_uppercase()) {
                        if !l.ends_with(';') {
                            l.push(';');
                        }
                        l.push_str(entry);
                        notes.push(alloc::format!("{k}: added {entry}"));
                    }
                }
                lines[i] = l;
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for l in &lines {
        out.extend(l.bytes());
        out.extend_from_slice(b"\r\n");
    }
    (out, notes)
}

// ---- Copying ----

fn now() -> fat::Timestamp {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds)
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let c = cstr(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut())
    };
    if rc != 0 {
        return None;
    }
    let mut v = Vec::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let mut n = 0;
        if unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        v.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(hf) };
    Some(v)
}

struct Entry {
    name: String,
    dir: bool,
}

fn list(dir: &str) -> Vec<Entry> {
    let mut v = Vec::new();
    let spec = cstr(&alloc::format!("{}\\*", dir.trim_end_matches('\\')));
    let mut hdir = HDIR_CREATE;
    let mut buf = vec![0u8; 8192];
    let mut count = 64;
    let mut rc = unsafe { DosFindFirst(spec.as_ptr(), &mut hdir, FILE_DIRECTORY | FILE_HIDDEN | FILE_SYSTEM, buf.as_mut_ptr(), buf.len() as u32, &mut count, FIL_STANDARD) };
    while rc == 0 {
        let mut off = 0usize;
        for _ in 0..count {
            let e = &buf[off..];
            let next = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let attr = u32::from_le_bytes([e[24], e[25], e[26], e[27]]);
            let n = e[28] as usize;
            let name: String = e[29..29 + n].iter().map(|&b| b as char).collect();
            if name != "." && name != ".." {
                v.push(Entry { name, dir: attr & FILE_DIRECTORY != 0 });
            }
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 64;
        rc = unsafe { DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    }
    unsafe { DosFindClose(hdir) };
    v
}

fn put(vol: &mut fat::Volume<Disk>, dest: &str, data: &[u8]) -> Result<(), fat::Error> {
    let ts = now();
    let parts: Vec<&str> = dest.split('\\').filter(|s| !s.is_empty()).collect();
    let mut p = String::new();
    for d in &parts[..parts.len() - 1] {
        if !p.is_empty() {
            p.push('\\');
        }
        p.push_str(d);
        if vol.lookup(&p).is_err() {
            vol.mkdir(&p, ts)?;
        }
    }
    if vol.lookup(dest).is_ok() {
        vol.delete(dest)?;
    }
    let mut e = vol.create_file(dest, 0, ts)?;
    vol.write(&mut e, 0, data, ts)?;
    Ok(())
}

/// Copy the tree `src` (e.g. "C:") into `dest` on the new volume; when
/// updating, the user's files there (`kept`) stay as they are.
fn copy_tree(vol: &mut fat::Volume<Disk>, src: &str, dest: &str, files: &mut u32, update: bool) -> Result<(), String> {
    for e in list(src) {
        let s = alloc::format!("{}\\{}", src.trim_end_matches('\\'), e.name);
        let d = if dest.is_empty() { e.name.clone() } else { alloc::format!("{dest}\\{}", e.name) };
        if e.dir {
            if vol.lookup(&d).is_err() {
                vol.mkdir(&d, now()).map_err(|x| alloc::format!("{d}: {x:?}"))?;
            }
            copy_tree(vol, &s, &d, files, update)?;
        } else {
            if update && kept(&d) && vol.lookup(&d).is_ok() {
                continue;
            }
            let data = read_file(&s).ok_or_else(|| alloc::format!("cannot read {s}"))?;
            put(vol, &d, &data).map_err(|x| alloc::format!("{d}: {x:?}"))?;
            *files += 1;
            if *files % 10 == 0 {
                out(".");
            }
        }
    }
    Ok(())
}

fn main(_s: &Startup) -> u32 {
    let rc = install();
    // Started from the desktop the installer has a session of its own,
    // which closes when it ends: keep any message on the screen.
    if rc != 0 {
        outln("");
        let _ = ask("Press Enter to close. ");
    }
    rc
}

fn install() -> u32 {
    outln("");
    outln("rs2 Installation");
    outln("================");
    outln("");
    let this = version_of(read_file("C:\\RS2\\VERSION"));
    outln(&alloc::format!("This disc: {this}"));
    outln("");
    let mut n = 0u16;
    unsafe { DosPhysicalDisk(1, &mut n as *mut u16 as *mut u8, 2, core::ptr::null(), 0) };
    if n == 0 {
        outln("No hard disks were found. Installation cannot continue.");
        return 1;
    }
    outln("Hard disks:");
    let mut disks = Vec::new();
    for i in 1..=n as u32 {
        if let Some(d) = Disk::open(i) {
            let found = find_rs2(&d);
            let what = match &found {
                Some(f) => alloc::format!(" - {} installed", f.version),
                None => String::new(),
            };
            outln(&alloc::format!("  {i}. {} MB{what}", d.sectors * d.sector_size as u64 >> 20));
            disks.push((i, d, found));
        }
    }
    let pick = ask("\r\nInstall on which disk (number, or Enter to cancel)? ");
    let Ok(choice) = pick.parse::<u32>() else {
        outln("Installation cancelled.");
        return 1;
    };
    let Some((_, disk, found)) = disks.into_iter().find(|(i, _, _)| *i == choice) else {
        outln("There is no such disk.");
        return 1;
    };
    let mut vol = match found {
        Some(f) => {
            outln("");
            outln(&alloc::format!("Disk {choice} has {}.", f.version));
            outln("  U  Update it: the system files are replaced; your settings, desktop,");
            outln("     programs and files are kept.");
            outln("  E  Erase the disk and install anew.");
            match ask("Choose U or E (or Enter to cancel): ").to_ascii_uppercase().as_str() {
                "U" => match update_volume(&f) {
                    Some(v) => v,
                    None => return 1,
                },
                "E" => match fresh(disk, choice) {
                    Some(v) => v,
                    None => return 1,
                },
                _ => {
                    outln("Installation cancelled.");
                    return 1;
                }
            }
        }
        None => match fresh(disk, choice) {
            Some(v) => v,
            None => return 1,
        },
    };
    if let Err(e) = boot_files(&mut vol) {
        outln(&e);
        return 1;
    }
    outln("");
    outln("Installation is complete. Remove the installation disc and press Enter");
    out("to restart the computer.");
    let _ = ask("");
    let c = cstr("DOS$");
    let (mut h, mut act) = (0, 0);
    unsafe {
        if DosOpen(c.as_ptr(), &mut h, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, core::ptr::null_mut()) == 0 {
            let (mut pl, mut dl) = (0, 0);
            DosDevIOCtl(h, 0xd5, 0xab, core::ptr::null(), 0, &mut pl, core::ptr::null_mut(), 0, &mut dl);
        }
    }
    0
}

/// Partition and format the disk and copy the system onto it.
fn fresh(disk: Disk, choice: u32) -> Option<fat::Volume<Disk>> {
    if disk.sectors * (disk.sector_size as u64) < (64 << 20) {
        outln("The disk is too small (64 MB or more is needed).");
        return None;
    }
    outln("");
    outln(&alloc::format!("ALL DATA ON DISK {choice} WILL BE ERASED."));
    if !ask("Type YES to continue: ").eq_ignore_ascii_case("YES") {
        outln("Installation cancelled.");
        return None;
    }
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    let seed = (dt.hours as u32) << 24 | (dt.minutes as u32) << 16 | (dt.seconds as u32) << 8 | dt.hundredths as u32;

    out("Partitioning... ");
    let (start, count) = match partition(&disk, seed) {
        Ok(v) => v,
        Err(e) => {
            outln(&alloc::format!("failed (error {e})"));
            return None;
        }
    };
    outln("done");

    out("Formatting... ");
    let part = Disk { offset: start, count, ..disk };
    let fat_type = if count * part.sector_size as u64 >= (256 << 20) { Some(fat::FatType::Fat32) } else { Some(fat::FatType::Fat16) };
    if let Err(e) = fat::format(&part, &fat::FormatParams { label: "RS2", serial: seed, fat_type, hidden_sectors: start as u32 }) {
        outln(&alloc::format!("failed ({e:?})"));
        return None;
    }
    let mut vol = match fat::Volume::mount(part) {
        Ok(v) => v,
        Err(e) => {
            outln(&alloc::format!("failed ({e:?})"));
            return None;
        }
    };
    outln("done");

    out("Copying the system");
    let mut files = 0;
    if let Err(e) = copy_tree(&mut vol, "C:", "", &mut files, false) {
        outln(&alloc::format!("\r\nCopying failed: {e}"));
        return None;
    }
    outln(&alloc::format!(" {files} files copied."));
    Some(vol)
}

/// Replace the system files of an installation, keeping the user's.
fn update_volume(f: &Found) -> Option<fat::Volume<Disk>> {
    let mut vol = match fat::Volume::mount(f.part) {
        Ok(v) => v,
        Err(e) => {
            outln(&alloc::format!("The installation cannot be read ({e:?})."));
            return None;
        }
    };
    out("Updating the system");
    let mut files = 0;
    if let Err(e) = copy_tree(&mut vol, "C:", "", &mut files, true) {
        outln(&alloc::format!("\r\nUpdating failed: {e}"));
        return None;
    }
    outln(&alloc::format!(" {files} files updated."));
    // CONFIG.SYS: the user's, with what the new version adds.
    if let (Some(old), Some(new)) = (read_vol(&vol, "CONFIG.SYS"), read_file("C:\\CONFIG.SYS")) {
        let (merged, notes) = merge_config(&old, &new);
        if !notes.is_empty() {
            if let Err(e) = put(&mut vol, "CONFIG.SYS", &merged) {
                outln(&alloc::format!("CONFIG.SYS cannot be updated ({e:?})."));
                return None;
            }
            outln("CONFIG.SYS:");
            for n in notes {
                outln(&alloc::format!("  {n}"));
            }
        }
    }
    Some(vol)
}

/// The boot loader and kernel where the firmware and the loader look.
fn boot_files(vol: &mut fat::Volume<Disk>) -> Result<(), String> {
    for (src, dest) in [("C:\\RS2\\BOOT\\BOOTX64.EFI", "EFI\\BOOT\\BOOTX64.EFI"), ("C:\\RS2\\BOOT\\KERNEL", "RS2\\KERNEL")] {
        let d = read_file(src).ok_or_else(|| alloc::format!("The boot file {src} is missing."))?;
        put(vol, dest, &d).map_err(|e| alloc::format!("Cannot write {dest}: {e:?}"))?;
    }
    Ok(())
}
