//! The installer's work, shared by INSTALL.EXE (the command line) and
//! PMINSTAL.EXE (Presentation Manager).
//!
//! A new installation is one of two layouts on a GPT disk:
//!
//! - **FAT32**: one EFI system partition holding everything: the boot
//!   loader, the kernel and the system (drive C:).
//! - **HPFS** or **JFS**: a small FAT32 EFI system
//!   partition with the boot loader, the kernel and `\RS2\CMDLINE`
//!   (`root=GUID`), and the system on a partition of its own, which the
//!   kernel mounts as C:. HPFS volumes hold at most 64 GB (the rest of a
//!   larger disk stays free); JFS takes the whole disk.
//!
//! An existing installation (an EFI system partition holding
//! `\RS2\KERNEL`, and the partition its root= names) can be updated: the
//! system files are replaced, the user's (CONFIG.SYS, INI files,
//! `\MPTN\ETC`, `\RS2\CMDLINE`) kept, and CONFIG.SYS gets what the new
//! version adds.

pub mod disk;
pub mod vol;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use disk::*;
use os2::path::cstr;
use os2::*;
pub use vol::Fs;
use vol::Vol;

/// What the installer reports while it works.
pub trait Progress {
    /// A step begins ("Formatting...").
    fn step(&mut self, what: &str);
    /// Another file was copied.
    fn file(&mut self, count: u32, name: &str);
    /// Something to tell the user afterwards (CONFIG.SYS changes).
    fn note(&mut self, what: &str);
}

/// rs2 found on a disk.
pub struct Found {
    pub esp: Disk,
    /// The system partition when it is not the EFI system partition.
    pub system: Option<(Disk, Fs)>,
    pub version: String,
}

impl Found {
    pub fn layout(&self) -> Fs {
        self.system.map_or(Fs::Fat, |s| s.1)
    }
}

pub struct DiskInfo {
    /// 1-based, as DosPhysicalDisk numbers them.
    pub number: u32,
    pub disk: Disk,
    pub found: Option<Found>,
}

/// The size of the EFI system partition in the HPFS/JFS layouts.
pub const ESP_MB: u64 = 256;
/// The smallest disk the installer accepts.
pub const MIN_MB: u64 = 64;
/// HPFS's largest volume (2^27 sectors).
const HPFS_MAX_SECTORS: u64 = 1 << 27;

pub fn version_of(text: Option<Vec<u8>>) -> String {
    match text {
        Some(t) => t.iter().map(|&b| b as char).collect::<String>().trim().to_string(),
        None => String::from("an earlier rs2"),
    }
}

/// The version on the installation medium.
pub fn this_version() -> String {
    version_of(read_file("C:\\RS2\\VERSION"))
}

/// The file systems a new installation can use.
pub fn layouts() -> &'static [Fs] {
    &[Fs::Fat, Fs::Hpfs, Fs::Jfs]
}

/// The hard disks and what is on them.
pub fn disks() -> Vec<DiskInfo> {
    let mut v = Vec::new();
    for n in 1..=disk::count() {
        if let Some(d) = Disk::open(n) {
            let found = find_rs2(&d);
            v.push(DiskInfo { number: n, disk: d, found });
        }
    }
    v
}

/// The root= of a boot command line.
fn root_of(cmdline: &[u8]) -> Option<String> {
    let t: String = cmdline.iter().map(|&b| b as char).collect();
    t.split_whitespace().find_map(|w| w.strip_prefix("root=")).map(|g| g.to_ascii_lowercase())
}

/// The rs2 installation on disk `d`, if it has one.
pub fn find_rs2(d: &Disk) -> Option<Found> {
    let parts = read_gpt(d);
    for &(t, _, start, count) in &parts {
        if t != ESP_TYPE {
            continue;
        }
        let esp = d.part(start, count);
        let Ok(v) = fat::Volume::mount(esp) else { continue };
        let mut v = Vol::Fat(v);
        if !v.exists("RS2\\KERNEL") {
            continue;
        }
        let root = v.get("RS2\\CMDLINE").and_then(|c| root_of(&c));
        let Some(g) = root else {
            return Some(Found { esp, system: None, version: version_of(v.get("RS2\\VERSION")) });
        };
        let sys = parts.iter().find(|p| guid_text(&p.1) == g).map(|p| d.part(p.2, p.3));
        let Some(sys) = sys else { continue };
        let Some(mut sv) = Vol::mount(sys) else { continue };
        let version = version_of(sv.get("RS2\\VERSION"));
        return Some(Found { esp, system: Some((sys, sv.fs())), version });
    }
    None
}

/// Files of an installation that are the user's: kept when updating.
fn kept(dest: &str) -> bool {
    let u = dest.to_ascii_uppercase();
    u == "CONFIG.SYS" || u.ends_with(".INI") || u.starts_with("MPTN\\ETC\\") || u == "RS2\\CMDLINE"
}

/// CONFIG.SYS with what `new` adds to `old`: SET variables `old` does not
/// set, and the entries of its path lists that `old`'s lines lack.
/// Returns the text and what was added.
pub fn merge_config(old: &[u8], new: &[u8]) -> (Vec<u8>, Vec<String>) {
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

// ---- Copying from the running system ----

pub fn read_file(path: &str) -> Option<Vec<u8>> {
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

/// The extended attributes of `path` (flags, name, value).
fn read_eas(path: &str) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    let c = cstr(path);
    let mut out = vec![0u8; 0x10000 + 16];
    let n = out.len() as u32;
    out[0..4].copy_from_slice(&n.to_le_bytes());
    let mut op = EaOp2 { gea2: core::ptr::null_mut(), fea2: out.as_mut_ptr(), error: 0 };
    if unsafe { DosQueryPathInfo(c.as_ptr(), FIL_QUERYALLEAS, &mut op as *mut _ as *mut u8, 12) } != 0 {
        return Vec::new();
    }
    // FEA2LIST: cbList, then FEA2s (oNextEntryOffset, fEA, cbName,
    // cbValue, the name and its NUL, the value).
    let end = (u32::from_le_bytes([out[0], out[1], out[2], out[3]]) as usize).min(out.len());
    let mut v = Vec::new();
    let mut off = 4;
    while off + 8 <= end {
        let next = u32::from_le_bytes([out[off], out[off + 1], out[off + 2], out[off + 3]]) as usize;
        let (flags, cn, cv) = (out[off + 4], out[off + 5] as usize, u16::from_le_bytes([out[off + 6], out[off + 7]]) as usize);
        let name = off + 8;
        if name + cn + 1 + cv > end {
            break;
        }
        v.push((flags, out[name..name + cn].to_vec(), out[name + cn + 1..name + cn + 1 + cv].to_vec()));
        if next == 0 {
            break;
        }
        off += next;
    }
    v
}

/// The installer itself belongs to the live media: an installed system
/// gets no copy (an update removes an earlier one), and so no
/// "Install rs2" object (the shells drop it where its program is not).
const LIVE_ONLY: [&str; 2] = ["OS2\\PMINSTAL.EXE", "OS2\\INSTALL.EXE"];

fn live_only(dest: &str) -> bool {
    LIVE_ONLY.iter().any(|f| dest.eq_ignore_ascii_case(f))
}

/// The file that holds a FAT volume's own data (its extended
/// attributes): it is not copied as a file. (`WP ROOT. SF`, where the
/// Workplace Shell keeps the root folder's EAs, is copied with them.)
fn system_file(name: &str) -> bool {
    name.eq_ignore_ascii_case("EA DATA. SF")
}

/// Copy `src`'s extended attributes (the Workplace Shell's .CLASSINFO,
/// .LONGNAME and others) to `dest`.
fn copy_eas(vol: &mut Vol, src: &str, dest: &str) -> Result<(), String> {
    let eas = read_eas(src);
    if eas.is_empty() { Ok(()) } else { vol.set_eas(dest, &eas) }
}

/// Copy the tree `src` (e.g. "C:") into `dest` on `vol`; when updating,
/// the user's files there (`kept`) stay as they are.
fn copy_tree(vol: &mut Vol, src: &str, dest: &str, files: &mut u32, update: bool, p: &mut dyn Progress) -> Result<(), String> {
    for e in list(src) {
        if dest.is_empty() && system_file(&e.name) {
            continue;
        }
        let s = alloc::format!("{}\\{}", src.trim_end_matches('\\'), e.name);
        let d = if dest.is_empty() { e.name.clone() } else { alloc::format!("{dest}\\{}", e.name) };
        if e.dir {
            vol.mkdir(&d)?;
            copy_eas(vol, &s, &d)?;
            copy_tree(vol, &s, &d, files, update, p)?;
        } else {
            if live_only(&d) {
                if update && vol.exists(&d) {
                    vol.delete(&d)?;
                }
                continue;
            }
            if update && kept(&d) && vol.exists(&d) {
                continue;
            }
            let data = read_file(&s).ok_or_else(|| alloc::format!("cannot read {s}"))?;
            vol.put(&d, &data)?;
            copy_eas(vol, &s, &d)?;
            *files += 1;
            p.file(*files, &d);
        }
    }
    Ok(())
}

/// The boot loader and kernel where the firmware and the loader look.
fn boot_files(esp: &mut Vol) -> Result<(), String> {
    for (src, dest) in [("C:\\RS2\\BOOT\\BOOTX64.EFI", "EFI\\BOOT\\BOOTX64.EFI"), ("C:\\RS2\\BOOT\\KERNEL", "RS2\\KERNEL")] {
        let d = read_file(src).ok_or_else(|| alloc::format!("The boot file {src} is missing."))?;
        esp.put(dest, &d)?;
    }
    Ok(())
}

fn seed() -> u32 {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    (dt.hours as u32) << 24 | (dt.minutes as u32) << 16 | (dt.seconds as u32) << 8 | dt.hundredths as u32
}

fn format_fat(part: Disk, label: &str, serial: u32) -> Result<Vol, String> {
    let fat_type = if part.count * part.sector_size as u64 >= (256 << 20) { Some(fat::FatType::Fat32) } else { Some(fat::FatType::Fat16) };
    fat::format(&part, &fat::FormatParams { label, serial, fat_type, hidden_sectors: part.offset as u32 }).map_err(|e| alloc::format!("Formatting failed ({e:?})."))?;
    fat::Volume::mount(part).map(Vol::Fat).map_err(|e| alloc::format!("The new volume cannot be read ({e:?})."))
}

fn format_jfs(part: Disk, serial: u32) -> Result<Vol, String> {
    jfs::format(&part, &jfs::FormatParams { label: "RS2", serial, hidden_sectors: part.offset as u32, now: vol::unix_now(), ..Default::default() })
        .map_err(|e| alloc::format!("Formatting JFS failed ({e:?})."))?;
    jfs::Volume::mount(part).map(Vol::Jfs).map_err(|e| alloc::format!("The new JFS volume cannot be read ({e:?})."))
}

fn format_hpfs(part: Disk, serial: u32) -> Result<Vol, String> {
    hpfs::format(&part, &hpfs::FormatParams { label: "RS2", serial, hidden_sectors: part.offset as u32, code_page: 850, now: vol::unix_now() })
        .map_err(|e| alloc::format!("Formatting HPFS failed ({e:?})."))?;
    hpfs::Volume::mount(part).map(Vol::Hpfs).map_err(|e| alloc::format!("The new HPFS volume cannot be read ({e:?})."))
}

/// Erase the disk and install rs2 with the layout `fs`.
pub fn install_fresh(d: &Disk, fs: Fs, p: &mut dyn Progress) -> Result<(), String> {
    if d.mb() < MIN_MB {
        return Err(alloc::format!("The disk is too small ({MIN_MB} MB or more is needed)."));
    }
    let s = seed();
    let (first, last) = usable(d);
    let mb = |n: u64| n * (1 << 20) / d.sector_size as u64;
    let align = |n: u64| n & !(mb(1) - 1);
    p.step("Partitioning");
    let esp_part;
    let mut sys_part = None;
    if fs == Fs::Fat {
        let end = align(last + 1);
        esp_part = NewPart { kind: ESP_TYPE, guid: guid(s), start: first, count: end - first, name: "rs2 system" };
        write_gpt(d, core::slice::from_ref(&esp_part), s).map_err(|e| alloc::format!("Partitioning failed (error {e})."))?;
    } else {
        if d.mb() < ESP_MB + 128 {
            return Err(alloc::format!("The disk is too small for a separate system partition ({} MB or more is needed).", ESP_MB + 128));
        }
        esp_part = NewPart { kind: ESP_TYPE, guid: guid(s), start: first, count: mb(ESP_MB), name: "rs2 boot" };
        let sys_start = first + esp_part.count;
        let cap = if fs == Fs::Hpfs { HPFS_MAX_SECTORS } else { u64::MAX / 2 };
        let sys_end = align(last + 1).min(sys_start + cap);
        let sp = NewPart { kind: DATA_TYPE, guid: guid(s.wrapping_add(7)), start: sys_start, count: sys_end - sys_start, name: "rs2 system" };
        write_gpt(d, &[esp_part, sp], s).map_err(|e| alloc::format!("Partitioning failed (error {e})."))?;
        if align(last + 1) > sys_end {
            p.note(&alloc::format!("HPFS volumes hold at most 64 GB: {} MB of the disk are left free.", (align(last + 1) - sys_end) * d.sector_size as u64 >> 20));
        }
        sys_part = Some(sp);
    }
    p.step("Formatting");
    let mut esp = format_fat(d.part(esp_part.start, esp_part.count), if sys_part.is_some() { "RS2BOOT" } else { "RS2" }, s)?;
    let mut sys = match &sys_part {
        Some(sp) if fs == Fs::Jfs => Some(format_jfs(d.part(sp.start, sp.count), s.wrapping_add(1))?),
        Some(sp) => Some(format_hpfs(d.part(sp.start, sp.count), s.wrapping_add(1))?),
        None => None,
    };
    p.step("Copying the system");
    let mut files = 0;
    copy_tree(sys.as_mut().unwrap_or(&mut esp), "C:", "", &mut files, false, p)?;
    p.step("Writing the boot files");
    boot_files(&mut esp)?;
    if let Some(sp) = &sys_part {
        esp.put("RS2\\CMDLINE", alloc::format!("root={}\r\n", guid_text(&sp.guid)).as_bytes())?;
        if let Some(v) = read_file("C:\\RS2\\VERSION") {
            esp.put("RS2\\VERSION", &v)?;
        }
    }
    if let Some(v) = sys {
        v.close()?;
    }
    esp.close()
}

/// Update an installation in place.
pub fn update(f: &Found, p: &mut dyn Progress) -> Result<(), String> {
    let mut esp = Vol::mount(f.esp).ok_or("The EFI system partition cannot be read.")?;
    let mut sys = match f.system {
        Some((part, _)) => Some(Vol::mount(part).ok_or("The system partition cannot be read.")?),
        None => None,
    };
    p.step("Updating the system");
    let mut files = 0;
    let target = sys.as_mut().unwrap_or(&mut esp);
    copy_tree(target, "C:", "", &mut files, true, p)?;
    // CONFIG.SYS: the user's, with what the new version adds.
    if let (Some(old), Some(new)) = (target.get("CONFIG.SYS"), read_file("C:\\CONFIG.SYS")) {
        let (merged, notes) = merge_config(&old, &new);
        if !notes.is_empty() {
            target.put("CONFIG.SYS", &merged)?;
            for n in notes {
                p.note(&alloc::format!("CONFIG.SYS: {n}"));
            }
        }
    }
    p.step("Writing the boot files");
    boot_files(&mut esp)?;
    if sys.is_some() {
        if let Some(v) = read_file("C:\\RS2\\VERSION") {
            esp.put("RS2\\VERSION", &v)?;
        }
    }
    if let Some(v) = sys {
        v.close()?;
    }
    esp.close()
}
