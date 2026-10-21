//! The volumes the installer writes: FAT (the EFI system partition, or
//! the whole system) and HPFS (a system partition of its own), behind one
//! interface.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::disk::Disk;
use os2::*;

pub enum Vol {
    Fat(fat::Volume<Disk>),
    Hpfs(hpfs::Volume<Disk>),
}

/// The system partition's file system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fs {
    Fat,
    Hpfs,
    Jfs,
}

impl Fs {
    #[allow(dead_code)]
    pub fn name(self) -> &'static str {
        match self {
            Fs::Fat => "FAT32",
            Fs::Hpfs => "HPFS",
            Fs::Jfs => "JFS",
        }
    }
}

pub fn fat_now() -> fat::Timestamp {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    fat::Timestamp::new(dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds)
}

/// Seconds since 1970 (HPFS time stamps).
pub fn unix_now() -> u32 {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    let (y, m, d) = (dt.year as i64, dt.month as i64, dt.day as i64);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days * 86400 + dt.hours as i64 * 3600 + dt.minutes as i64 * 60 + dt.seconds as i64) as u32
}

impl Vol {
    /// Mount whatever a partition holds (FAT, then HPFS).
    pub fn mount(part: Disk) -> Option<Vol> {
        if let Ok(v) = fat::Volume::mount(part) {
            return Some(Vol::Fat(v));
        }
        hpfs::Volume::mount(part).ok().map(Vol::Hpfs)
    }

    pub fn fs(&self) -> Fs {
        match self {
            Vol::Fat(_) => Fs::Fat,
            Vol::Hpfs(_) => Fs::Hpfs,
        }
    }

    pub fn exists(&mut self, path: &str) -> bool {
        match self {
            Vol::Fat(v) => v.lookup(path).is_ok(),
            Vol::Hpfs(v) => v.lookup(path).is_ok(),
        }
    }

    pub fn get(&mut self, path: &str) -> Option<Vec<u8>> {
        match self {
            Vol::Fat(v) => {
                let e = v.lookup(path).ok()?;
                let mut b = vec![0u8; e.size as usize];
                let n = v.read(&e, 0, &mut b).ok()?;
                b.truncate(n);
                Some(b)
            }
            Vol::Hpfs(v) => {
                let e = v.lookup(path).ok()?;
                let mut b = vec![0u8; e.size as usize];
                let n = v.read(&e, 0, &mut b).ok()?;
                b.truncate(n);
                Some(b)
            }
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), String> {
        if self.exists(path) {
            return Ok(());
        }
        let r = match self {
            Vol::Fat(v) => v.mkdir(path, fat_now()).map(|_| ()).map_err(|e| alloc::format!("{e:?}")),
            Vol::Hpfs(v) => v.mkdir(path, unix_now()).map(|_| ()).map_err(|e| alloc::format!("{e:?}")),
        };
        r.map_err(|e| alloc::format!("{path}: {e}"))
    }

    /// Write file `path` (replacing it), making its directories.
    pub fn put(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
        let parts: Vec<&str> = path.split('\\').filter(|s| !s.is_empty()).collect();
        let mut p = String::new();
        for d in &parts[..parts.len().saturating_sub(1)] {
            if !p.is_empty() {
                p.push('\\');
            }
            p.push_str(d);
            self.mkdir(&p)?;
        }
        let r = match self {
            Vol::Fat(v) => {
                let ts = fat_now();
                (|| {
                    if v.lookup(path).is_ok() {
                        v.delete(path)?;
                    }
                    let mut e = v.create_file(path, 0, ts)?;
                    v.write(&mut e, 0, data, ts)?;
                    Ok::<(), fat::Error>(())
                })()
                .map_err(|e| alloc::format!("{e:?}"))
            }
            Vol::Hpfs(v) => {
                let ts = unix_now();
                (|| {
                    if v.lookup(path).is_ok() {
                        v.delete(path)?;
                    }
                    let mut e = v.create_file(path, 0, ts)?;
                    v.write(&mut e, 0, data, ts)?;
                    Ok::<(), hpfs::Error>(())
                })()
                .map_err(|e| alloc::format!("{e:?}"))
            }
        };
        r.map_err(|e| alloc::format!("{path}: {e}"))
    }
}
