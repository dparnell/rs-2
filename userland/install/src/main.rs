//! rs2 installer.
//!
//! Installs the running live system onto a hard disk: a GPT with one EFI
//! system partition (FAT32) that holds the boot loader, the kernel and the
//! system files. The partition is written directly through the physical
//! disk IOCtls with the same FAT code the build tools use.

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

/// Copy the tree `src` (e.g. "C:") into `dest` on the new volume.
fn copy_tree(vol: &mut fat::Volume<Disk>, src: &str, dest: &str, files: &mut u32) -> Result<(), String> {
    for e in list(src) {
        let s = alloc::format!("{}\\{}", src.trim_end_matches('\\'), e.name);
        let d = if dest.is_empty() { e.name.clone() } else { alloc::format!("{dest}\\{}", e.name) };
        if e.dir {
            if vol.lookup(&d).is_err() {
                vol.mkdir(&d, now()).map_err(|x| alloc::format!("{d}: {x:?}"))?;
            }
            copy_tree(vol, &s, &d, files)?;
        } else {
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
            outln(&alloc::format!("  {i}. {} MB", d.sectors * d.sector_size as u64 >> 20));
            disks.push((i, d));
        }
    }
    let pick = ask("\r\nInstall on which disk (number, or Enter to cancel)? ");
    let Ok(choice) = pick.parse::<u32>() else {
        outln("Installation cancelled.");
        return 1;
    };
    let Some((_, disk)) = disks.into_iter().find(|(i, _)| *i == choice) else {
        outln("There is no such disk.");
        return 1;
    };
    if disk.sectors * (disk.sector_size as u64) < (64 << 20) {
        outln("The disk is too small (64 MB or more is needed).");
        return 1;
    }
    outln("");
    outln(&alloc::format!("ALL DATA ON DISK {choice} WILL BE ERASED."));
    if !ask("Type YES to continue: ").eq_ignore_ascii_case("YES") {
        outln("Installation cancelled.");
        return 1;
    }
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    let seed = (dt.hours as u32) << 24 | (dt.minutes as u32) << 16 | (dt.seconds as u32) << 8 | dt.hundredths as u32;

    out("Partitioning... ");
    let (start, count) = match partition(&disk, seed) {
        Ok(v) => v,
        Err(e) => {
            outln(&alloc::format!("failed (error {e})"));
            return 1;
        }
    };
    outln("done");

    out("Formatting... ");
    let part = Disk { offset: start, count, ..disk };
    let fat_type = if count * part.sector_size as u64 >= (256 << 20) { Some(fat::FatType::Fat32) } else { Some(fat::FatType::Fat16) };
    if let Err(e) = fat::format(&part, &fat::FormatParams { label: "RS2", serial: seed, fat_type, hidden_sectors: start as u32 }) {
        outln(&alloc::format!("failed ({e:?})"));
        return 1;
    }
    let mut vol = match fat::Volume::mount(part) {
        Ok(v) => v,
        Err(e) => {
            outln(&alloc::format!("failed ({e:?})"));
            return 1;
        }
    };
    outln("done");

    out("Copying the system");
    let mut files = 0;
    if let Err(e) = copy_tree(&mut vol, "C:", "", &mut files) {
        outln(&alloc::format!("\r\nCopying failed: {e}"));
        return 1;
    }
    // Boot loader and kernel at the places the firmware and loader look.
    for (src, dest) in [("C:\\RS2\\BOOT\\BOOTX64.EFI", "EFI\\BOOT\\BOOTX64.EFI"), ("C:\\RS2\\BOOT\\KERNEL", "RS2\\KERNEL")] {
        match read_file(src) {
            Some(d) => {
                if let Err(e) = put(&mut vol, dest, &d) {
                    outln(&alloc::format!("\r\nCannot write {dest}: {e:?}"));
                    return 1;
                }
            }
            None => {
                outln(&alloc::format!("\r\nThe boot file {src} is missing."));
                return 1;
            }
        }
    }
    outln(&alloc::format!(" {files} files copied."));
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
