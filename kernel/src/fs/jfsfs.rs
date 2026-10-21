//! The JFS file system driver (on the shared `jfs` crate).
//!
//! Long names (up to 254 characters) in UCS-2, case preserved; OS/2
//! volumes are case-insensitive. Names are translated with code page 850.
//! The volume is marked mounted at its first change and clean again at
//! DosShutdown ([`FileSystem::shutdown`]).

use alloc::collections::BTreeMap;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use super::block::BlockDev;
use super::{attr, Ea, FileInfo, FileSystem, FsFile, SetInfo, Stamp, VolumeInfo};
use crate::api::err;
use crate::sync::{Mutex, SpinLock};

/// Adapter so the shared JFS crate can use kernel block devices.
pub struct JfsDev(pub Arc<dyn BlockDev>);

impl jfs::BlockDevice for JfsDev {
    fn sector_size(&self) -> usize {
        self.0.sector_size()
    }

    fn sector_count(&self) -> u64 {
        self.0.sector_count()
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> jfs::Result<()> {
        self.0.read(lba, buf).map_err(|_| jfs::Error::Io)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> jfs::Result<()> {
        self.0.write(lba, buf).map_err(|_| jfs::Error::Io)
    }
}

type Vol = jfs::Volume<JfsDev>;

pub struct JfsFs {
    inner: Arc<Inner>,
}

struct Inner {
    vol: Mutex<Vol>,
    /// Open files by F-node so all handles share one entry.
    open: SpinLock<BTreeMap<u32, Weak<JfsFile>>>,
}

fn map(e: jfs::Error) -> u32 {
    match e {
        jfs::Error::Io => err::NOT_READY,
        jfs::Error::NotJfs => err::NOT_READY,
        jfs::Error::NotFound => err::FILE_NOT_FOUND,
        jfs::Error::PathNotFound => err::PATH_NOT_FOUND,
        jfs::Error::Exists => err::ACCESS_DENIED,
        jfs::Error::NotADirectory => err::PATH_NOT_FOUND,
        jfs::Error::IsADirectory => err::ACCESS_DENIED,
        jfs::Error::DirectoryNotEmpty => err::ACCESS_DENIED,
        jfs::Error::InvalidName => err::FILENAME_EXCED_RANGE,
        jfs::Error::DiskFull => err::DISK_FULL,
        jfs::Error::Corrupt => err::NOT_READY,
        jfs::Error::ReadOnly => err::WRITE_PROTECT,
        jfs::Error::AccessDenied => err::ACCESS_DENIED,
        jfs::Error::EaTooLong => err::EA_LIST_TOO_LONG,
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

fn to_hpfs(eas: &[Ea]) -> Vec<jfs::Ea> {
    eas.iter().map(|e| jfs::Ea { flags: e.flags, name: e.name.clone(), value: e.value.clone() }).collect()
}

fn from_hpfs(eas: Vec<jfs::Ea>) -> Vec<Ea> {
    eas.into_iter().map(|e| Ea { flags: e.flags, name: e.name, value: e.value }).collect()
}

fn info_of(_v: &mut Vol, e: &jfs::Entry) -> FileInfo {
    let ea_size = e.ea_size;
    FileInfo {
        name: e.name.clone(),
        attr: e.attr as u32 & 0x37,
        size: e.size,
        alloc: e.size.next_multiple_of(4096),
        created: stamp(e.created),
        accessed: stamp(e.accessed),
        modified: stamp(e.modified),
        ea_size,
    }
}

/// Does the device hold a JFS volume (primary or secondary superblock)?
pub fn probe(dev: &Arc<dyn BlockDev>) -> bool {
    let ss = dev.sector_size() as u64;
    let mut b = alloc::vec![0u8; ss as usize];
    [0x8000u64, 0xf000].iter().any(|&off| dev.read(off / ss, &mut b).is_ok() && &b[0..4] == b"JFS1")
}

impl JfsFs {
    pub fn mount(dev: Arc<dyn BlockDev>) -> Result<Arc<dyn FileSystem>, u32> {
        let vol = jfs::Volume::mount(JfsDev(dev)).map_err(map)?;
        Ok(Arc::new(JfsFs { inner: Arc::new(Inner { vol: Mutex::new(vol), open: SpinLock::new(BTreeMap::new()) }) }))
    }

    fn file_for(&self, e: jfs::Entry) -> Arc<JfsFile> {
        let key = e.ino;
        if let Some(f) = self.inner.open.lock().get(&key).and_then(|w| w.upgrade()) {
            return f;
        }
        let f = Arc::new(JfsFile { fs: self.inner.clone(), entry: Mutex::new(e) });
        let mut o = self.inner.open.lock();
        o.retain(|_, w| w.strong_count() > 0);
        o.insert(key, Arc::downgrade(&f));
        f
    }
}

impl FileSystem for JfsFs {
    fn name(&self) -> &'static str {
        "JFS"
    }

    fn max_component(&self) -> usize {
        jfs::MAX_NAME
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
            if e == jfs::Error::Exists { err::FILE_EXISTS } else { map(e) }
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
            let f = self.inner.open.lock().get(&e.ino).and_then(|w| w.upgrade());
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
        let label = v.label.clone();
        Ok(VolumeInfo {
            serial: i.serial,
            label,
            sectors_per_unit: i.block_size / 512,
            units: i.total_blocks.min(u32::MAX as u64) as u32,
            free_units: i.free_blocks.min(u32::MAX as u64) as u32,
            bytes_per_sector: 512,
        })
    }

    fn set_label(&self, label: &str) -> Result<(), u32> {
        self.inner.vol.lock().set_label(label).map_err(map)
    }

    fn shutdown(&self) {
        let _ = self.inner.vol.lock().mark_clean();
    }

    fn device(&self) -> Option<Arc<dyn BlockDev>> {
        Some(self.inner.vol.lock().dev.0.clone())
    }

    fn read_eas(&self, path: &str) -> Result<Vec<Ea>, u32> {
        let mut v = self.inner.vol.lock();
        let e = v.lookup(path).map_err(map)?;
        v.read_eas(&e).map(from_hpfs).map_err(map)
    }

    fn write_eas(&self, path: &str, eas: &[Ea]) -> Result<(), u32> {
        let mut v = self.inner.vol.lock();
        let e = v.lookup(path).map_err(map)?;
        v.write_eas(&e, &to_hpfs(eas)).map_err(map)
    }
}

fn apply(e: &mut jfs::Entry, info: &SetInfo) {
    if let Some(a) = info.attr {
        e.attr = (e.attr & jfs::attr::DIRECTORY) | (a as u8 & 0x27);
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

pub struct JfsFile {
    fs: Arc<Inner>,
    entry: Mutex<jfs::Entry>,
}

impl FsFile for JfsFile {
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
        let mut e = self.entry.lock();
        self.fs.vol.lock().set_size(&mut e, size, now()).map_err(map)
    }

    fn info(&self) -> FileInfo {
        let e = self.entry.lock().clone();
        let mut v = self.fs.vol.lock();
        let mut i = info_of(&mut v, &e);
        // The entry may predate an EA change: ask the F-node.
        i.ea_size = v.ea_info(e.ino).unwrap_or(0);
        i
    }

    fn read_eas(&self) -> Result<Vec<Ea>, u32> {
        let e = self.entry.lock().clone();
        self.fs.vol.lock().read_eas(&e).map(from_hpfs).map_err(map)
    }

    fn write_eas(&self, eas: &[Ea]) -> Result<(), u32> {
        let e = self.entry.lock().clone();
        self.fs.vol.lock().write_eas(&e, &to_hpfs(eas)).map_err(map)
    }

    fn set_info(&self, info: &SetInfo) -> Result<(), u32> {
        let mut e = self.entry.lock();
        apply(&mut e, info);
        self.fs.vol.lock().update_entry(&e).map_err(map)
    }
}
