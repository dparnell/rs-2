//! Build FAT disk images with the shared `fat` crate.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub struct MemDisk(pub RefCell<Vec<u8>>);

impl fat::BlockDevice for MemDisk {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.0.borrow().len() as u64 / 512
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> fat::Result<()> {
        let o = lba as usize * 512;
        buf.copy_from_slice(&self.0.borrow()[o..o + buf.len()]);
        Ok(())
    }
    fn write(&self, lba: u64, buf: &[u8]) -> fat::Result<()> {
        let o = lba as usize * 512;
        self.0.borrow_mut()[o..o + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

/// (source file on the host, destination path in the image)
pub type FileList = Vec<(PathBuf, String)>;

fn now() -> fat::Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let rem = secs % 86400;
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = (if m <= 2 { y + 1 } else { y }) as u16;
    fat::Timestamp::new(y, m, d, (rem / 3600) as u8, (rem % 3600 / 60) as u8, (rem % 60) as u8)
}

/// Create a FAT image containing `files`. `min_mb` sets a lower size bound.
pub fn build(files: &FileList, label: &str, min_mb: u64, fat_type: Option<fat::FatType>) -> Vec<u8> {
    let total: u64 = files.iter().map(|(s, _)| std::fs::metadata(s).map(|m| m.len()).unwrap_or(0)).sum();
    let mb = ((total * 11 / 10) >> 20) + 2;
    let size = (mb.max(min_mb) << 20) as usize;
    let disk = MemDisk(RefCell::new(vec![0u8; size]));
    fat::format(&disk, &fat::FormatParams { label, serial: 0x1995_0904, fat_type, hidden_sectors: 0 })
        .expect("format");
    let mut vol = fat::Volume::mount(disk).expect("mount");
    let ts = now();
    for (src, dest) in files {
        let data = std::fs::read(src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
        let dest = dest.replace('/', "\\").to_ascii_uppercase();
        // Create parent directories.
        let parts: Vec<&str> = dest.split('\\').filter(|s| !s.is_empty()).collect();
        let mut p = String::new();
        for d in &parts[..parts.len() - 1] {
            if !p.is_empty() {
                p.push('\\');
            }
            p.push_str(d);
            if vol.lookup(&p).is_err() {
                vol.mkdir(&p, ts).unwrap_or_else(|e| panic!("mkdir {p}: {e:?}"));
            }
        }
        if vol.lookup(&dest).is_ok() {
            vol.delete(&dest).ok();
        }
        let mut e = vol.create_file(&dest, 0, ts).unwrap_or_else(|e| panic!("create {dest}: {e:?}"));
        vol.write(&mut e, 0, &data, ts).unwrap_or_else(|e| panic!("write {dest}: {e:?}"));
    }
    vol.dev.0.into_inner()
}

/// Parse a manifest: `src` or `src -> DEST\PATH` per line.
pub fn manifest(root: &Path, text: &str) -> FileList {
    let mut v = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (src, dest) = match line.split_once("->") {
            Some((s, d)) => (s.trim(), d.trim().to_string()),
            None => (line, Path::new(line).file_name().unwrap().to_string_lossy().into_owned()),
        };
        v.push((root.join(src), dest));
    }
    v
}

/// An MBR-partitioned disk image with one FAT partition (starting at
/// sector 2048) holding `files`.
pub fn disk(files: &FileList, label: &str, size_mb: u64, fat_type: Option<fat::FatType>) -> Vec<u8> {
    let total = (size_mb << 20) as usize;
    let start = 2048usize;
    let part_bytes = total - start * 512;
    // Build the partition contents with the hidden-sector count set.
    let part = MemDisk(RefCell::new(vec![0u8; part_bytes]));
    fat::format(&part, &fat::FormatParams { label, serial: 0x2026_1018, fat_type, hidden_sectors: start as u32 })
        .expect("format");
    let mut vol = fat::Volume::mount(part).expect("mount");
    let ts = now();
    for (src, dest) in files {
        let data = std::fs::read(src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
        let dest = dest.replace('/', "\\").to_ascii_uppercase();
        let parts: Vec<&str> = dest.split('\\').filter(|s| !s.is_empty()).collect();
        let mut p = String::new();
        for d in &parts[..parts.len() - 1] {
            if !p.is_empty() {
                p.push('\\');
            }
            p.push_str(d);
            if vol.lookup(&p).is_err() {
                vol.mkdir(&p, ts).unwrap();
            }
        }
        let mut e = vol.create_file(&dest, 0, ts).unwrap();
        vol.write(&mut e, 0, &data, ts).unwrap();
    }
    let fat_type = vol.fat_type;
    let part = vol.dev.0.into_inner();
    let mut img = vec![0u8; total];
    img[start * 512..].copy_from_slice(&part);
    let ty: u8 = match fat_type {
        fat::FatType::Fat12 => 0x01,
        fat::FatType::Fat16 => 0x06,
        fat::FatType::Fat32 => 0x0c,
    };
    let e = 446;
    img[e] = 0x80; // active
    img[e + 1..e + 4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    img[e + 4] = ty;
    img[e + 5..e + 8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    img[e + 8..e + 12].copy_from_slice(&(start as u32).to_le_bytes());
    img[e + 12..e + 16].copy_from_slice(&((part_bytes / 512) as u32).to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xaa;
    img
}
