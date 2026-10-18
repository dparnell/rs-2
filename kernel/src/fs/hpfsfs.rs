//! The HPFS file system driver (on the shared `hpfs` crate).
//!
//! Long names (up to 254 characters), case preserved; lookups are
//! case-insensitive through the volume's code page case map.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use super::block::BlockDev;
use super::{attr, FileInfo, FileSystem, FsFile, SetInfo, Stamp, VolumeInfo};
use crate::api::err;
use crate::sync::{Mutex, SpinLock};

/// Adapter so the shared HPFS crate can use kernel block devices.
pub struct HpfsDev(pub Arc<dyn BlockDev>);

impl hpfs::BlockDevice for HpfsDev {
    fn sector_size(&self) -> usize {
        self.0.sector_size()
    }

    fn sector_count(&self) -> u64 {
        self.0.sector_count()
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> hpfs::Result<()> {
        self.0.read(lba, buf).map_err(|_| hpfs::Error::Io)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> hpfs::Result<()> {
        self.0.write(lba, buf).map_err(|_| hpfs::Error::Io)
    }
}

type Vol = hpfs::Volume<HpfsDev>;

pub struct HpfsFs {
    inner: Arc<Inner>,
}

struct Inner {
    vol: Mutex<Vol>,
    /// Open files by F-node so all handles share one entry.
    open: SpinLock<BTreeMap<u32, Weak<HpfsFile>>>,
}

fn map(e: hpfs::Error) -> u32 {
    match e {
        hpfs::Error::Io => err::NOT_READY,
        hpfs::Error::NotHpfs => err::NOT_READY,
        hpfs::Error::NotFound => err::FILE_NOT_FOUND,
        hpfs::Error::PathNotFound => err::PATH_NOT_FOUND,
        hpfs::Error::Exists => err::ACCESS_DENIED,
        hpfs::Error::NotADirectory => err::PATH_NOT_FOUND,
        hpfs::Error::IsADirectory => err::ACCESS_DENIED,
        hpfs::Error::DirectoryNotEmpty => err::ACCESS_DENIED,
        hpfs::Error::InvalidName => err::FILENAME_EXCED_RANGE,
        hpfs::Error::DiskFull => err::DISK_FULL,
        hpfs::Error::Corrupt => err::NOT_READY,
        hpfs::Error::ReadOnly => err::WRITE_PROTECT,
        hpfs::Error::AccessDenied => err::ACCESS_DENIED,
    }
}

fn stamp(secs: u32) -> Stamp {
    let (date, time) = hpfs::time::to_dos(secs);
    Stamp { date, time }
}

fn secs(s: Stamp) -> u32 {
    hpfs::time::from_dos(s.date, s.time)
}

fn now() -> u32 {
    secs(super::now())
}

fn info_of(v: &mut Vol, e: &hpfs::Entry) -> FileInfo {
    let ea_size = if e.ea_size > 0 { v.read_eas(e).map(|l| hpfs::fea2_size(&l)).unwrap_or(0) } else { 0 };
    FileInfo {
        name: e.name.clone(),
        attr: e.attr as u32 & 0x37,
        size: e.size as u64,
        alloc: (e.size as u64).next_multiple_of(512),
        created: stamp(e.created),
        accessed: stamp(e.accessed),
        modified: stamp(e.modified),
        ea_size,
    }
}

/// Does the device hold an HPFS volume (superblock and spareblock)?
pub fn probe(dev: &Arc<dyn BlockDev>) -> bool {
    let mut b = alloc::vec![0u8; 512];
    let magic = |b: &[u8], a: u32, c: u32| b[0..4] == a.to_le_bytes() && b[4..8] == c.to_le_bytes();
    dev.sector_size() == 512
        && dev.read(16, &mut b).is_ok()
        && magic(&b, 0xf995_e849, 0xfa53_e9c5)
        && dev.read(17, &mut b).is_ok()
        && magic(&b, 0xf991_1849, 0xfa52_29c5)
}

impl HpfsFs {
    pub fn mount(dev: Arc<dyn BlockDev>) -> Result<Arc<dyn FileSystem>, u32> {
        let vol = hpfs::Volume::mount(HpfsDev(dev)).map_err(map)?;
        Ok(Arc::new(HpfsFs { inner: Arc::new(Inner { vol: Mutex::new(vol), open: SpinLock::new(BTreeMap::new()) }) }))
    }

    fn file_for(&self, e: hpfs::Entry) -> Arc<HpfsFile> {
        let key = e.fnode;
        if let Some(f) = self.inner.open.lock().get(&key).and_then(|w| w.upgrade()) {
            return f;
        }
        let f = Arc::new(HpfsFile { fs: self.inner.clone(), entry: Mutex::new(e) });
        let mut o = self.inner.open.lock();
        o.retain(|_, w| w.strong_count() > 0);
        o.insert(key, Arc::downgrade(&f));
        f
    }
}

impl FileSystem for HpfsFs {
    fn name(&self) -> &'static str {
        "HPFS"
    }

    fn max_component(&self) -> usize {
        hpfs::MAX_NAME
    }

    fn stat(&self, path: &str) -> Result<FileInfo, u32> {
        let mut v = self.inner.vol.lock();
        let e = v.lookup(path).map_err(map)?;
        let mut i = info_of(&mut v, &e);
        if path.is_empty() {
            i.attr = attr::DIRECTORY;
        }
        Ok(i)
    }

    fn list(&self, dir: &str) -> Result<Vec<FileInfo>, u32> {
        let mut v = self.inner.vol.lock();
        let e = v.lookup(dir).map_err(map)?;
        if !e.is_dir() {
            return Err(err::PATH_NOT_FOUND);
        }
        let entries = v.read_dir(&e).map_err(map)?;
        Ok(entries.iter().map(|e| info_of(&mut v, e)).collect())
    }

    fn open(&self, path: &str) -> Result<Arc<dyn FsFile>, u32> {
        let e = self.inner.vol.lock().lookup(path).map_err(map)?;
        if e.is_dir() {
            return Err(err::ACCESS_DENIED);
        }
        Ok(self.file_for(e))
    }

    fn create(&self, path: &str, attr: u32) -> Result<Arc<dyn FsFile>, u32> {
        let e = self.inner.vol.lock().create_file(path, attr as u8 & 0x27, now()).map_err(|e| {
            if e == hpfs::Error::Exists { err::FILE_EXISTS } else { map(e) }
        })?;
        Ok(self.file_for(e))
    }

    fn delete(&self, path: &str) -> Result<(), u32> {
        self.inner.vol.lock().delete(path).map_err(map)
    }

    fn mkdir(&self, path: &str) -> Result<(), u32> {
        self.inner.vol.lock().mkdir(path, now()).map(|_| ()).map_err(map)
    }

    fn rmdir(&self, path: &str) -> Result<(), u32> {
        self.inner.vol.lock().rmdir(path).map_err(map)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), u32> {
        let moved = {
            let mut v = self.inner.vol.lock();
            v.rename(from, to).map_err(map)?;
            v.lookup(to).ok()
        };
        // Keep open handles' entries pointing at the new name (the volume
        // lock is released first: file operations take entry, then volume).
        if let Some(e) = moved {
            let f = self.inner.open.lock().get(&e.fnode).and_then(|w| w.upgrade());
            if let Some(f) = f {
                let mut fe = f.entry.lock();
                fe.name = e.name;
                fe.parent = e.parent;
            }
        }
        Ok(())
    }

    fn set_info(&self, path: &str, info: &SetInfo) -> Result<(), u32> {
        let mut v = self.inner.vol.lock();
        let mut e = v.lookup(path).map_err(map)?;
        apply(&mut e, info);
        v.update_entry(&e).map_err(map)
    }

    fn volume(&self) -> Result<VolumeInfo, u32> {
        let v = self.inner.vol.lock();
        let i = v.info();
        let label: String = v.label.iter().map(|&c| c as char).collect::<String>().trim_end().into();
        Ok(VolumeInfo {
            serial: i.serial,
            label,
            sectors_per_unit: 1,
            units: i.total_sectors,
            free_units: i.free_sectors,
            bytes_per_sector: i.sector_size,
        })
    }

    fn set_label(&self, label: &str) -> Result<(), u32> {
        self.inner.vol.lock().set_label(label).map_err(map)
    }
}

fn apply(e: &mut hpfs::Entry, info: &SetInfo) {
    if let Some(a) = info.attr {
        e.attr = (e.attr & hpfs::attr::DIRECTORY) | (a as u8 & 0x27);
    }
    if let Some(s) = info.created.filter(|s| s.date != 0) {
        e.created = secs(s);
    }
    if let Some(s) = info.accessed.filter(|s| s.date != 0) {
        e.accessed = secs(s);
    }
    if let Some(s) = info.modified.filter(|s| s.date != 0) {
        e.modified = secs(s);
    }
}

pub struct HpfsFile {
    fs: Arc<Inner>,
    entry: Mutex<hpfs::Entry>,
}

impl FsFile for HpfsFile {
    fn read(&self, off: u64, buf: &mut [u8]) -> Result<usize, u32> {
        let e = self.entry.lock().clone();
        self.fs.vol.lock().read(&e, off, buf).map_err(map)
    }

    fn write(&self, off: u64, data: &[u8]) -> Result<usize, u32> {
        let mut e = self.entry.lock();
        self.fs.vol.lock().write(&mut e, off, data, now()).map_err(map)
    }

    fn size(&self) -> u64 {
        self.entry.lock().size as u64
    }

    fn set_size(&self, size: u64) -> Result<(), u32> {
        if size > u32::MAX as u64 {
            return Err(err::DISK_FULL);
        }
        let mut e = self.entry.lock();
        self.fs.vol.lock().set_size(&mut e, size as u32, now()).map_err(map)
    }

    fn info(&self) -> FileInfo {
        let e = self.entry.lock().clone();
        info_of(&mut self.fs.vol.lock(), &e)
    }

    fn set_info(&self, info: &SetInfo) -> Result<(), u32> {
        let mut e = self.entry.lock();
        apply(&mut e, info);
        self.fs.vol.lock().update_entry(&e).map_err(map)
    }
}
