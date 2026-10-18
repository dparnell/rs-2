//! The FAT file system driver (on the shared `fat` crate).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use super::block::{BlockDev, FatDev};
use super::{attr, FileInfo, FileSystem, FsFile, SetInfo, Stamp, VolumeInfo};
use crate::api::err;
use crate::sync::{Mutex, SpinLock};

type Vol = fat::Volume<FatDev>;

pub struct FatFs {
    inner: Arc<Inner>,
}

struct Inner {
    vol: Mutex<Vol>,
    /// Open files by directory slot so all handles share one entry.
    open: SpinLock<BTreeMap<(u32, u32), Weak<FatFile>>>,
}

fn map(e: fat::Error) -> u32 {
    match e {
        fat::Error::Io => err::NOT_READY,
        fat::Error::NotFound => err::FILE_NOT_FOUND,
        fat::Error::PathNotFound => err::PATH_NOT_FOUND,
        fat::Error::Exists => err::ACCESS_DENIED,
        fat::Error::NotADirectory => err::PATH_NOT_FOUND,
        fat::Error::IsADirectory => err::ACCESS_DENIED,
        fat::Error::DirectoryNotEmpty => err::ACCESS_DENIED,
        fat::Error::InvalidName => err::FILENAME_EXCED_RANGE,
        fat::Error::DiskFull => err::DISK_FULL,
        fat::Error::Corrupt => err::NOT_READY,
        fat::Error::ReadOnly => err::WRITE_PROTECT,
        fat::Error::AccessDenied => err::ACCESS_DENIED,
    }
}

fn ts(s: Stamp) -> fat::Timestamp {
    fat::Timestamp { date: s.date, time: s.time }
}

fn now() -> fat::Timestamp {
    ts(super::now())
}

fn info_of(e: &fat::Entry, cluster_size: u32) -> FileInfo {
    FileInfo {
        name: e.name.clone(),
        attr: e.attr as u32 & 0x37,
        size: e.size as u64,
        alloc: (e.size as u64).next_multiple_of(cluster_size as u64),
        created: Stamp { date: e.created.date, time: e.created.time },
        accessed: Stamp { date: e.accessed, time: 0 },
        modified: Stamp { date: e.modified.date, time: e.modified.time },
        ea_size: 0,
    }
}

fn slot_key(e: &fat::Entry) -> Option<(u32, u32)> {
    let s = e.slot?;
    let d = match s.dir {
        fat::Dir::FixedRoot => 0,
        fat::Dir::Cluster(c) => c,
    };
    Some((d, s.index))
}

impl FatFs {
    pub fn mount(dev: Arc<dyn BlockDev>) -> Result<Arc<dyn FileSystem>, u32> {
        let vol = fat::Volume::mount(FatDev(dev)).map_err(map)?;
        Ok(Arc::new(FatFs { inner: Arc::new(Inner { vol: Mutex::new(vol), open: SpinLock::new(BTreeMap::new()) }) }))
    }

    fn file_for(&self, e: fat::Entry) -> Arc<FatFile> {
        let key = slot_key(&e);
        if let Some(k) = key {
            if let Some(f) = self.inner.open.lock().get(&k).and_then(|w| w.upgrade()) {
                return f;
            }
        }
        let f = Arc::new(FatFile { fs: self.inner.clone(), entry: Mutex::new(e) });
        if let Some(k) = key {
            let mut o = self.inner.open.lock();
            o.retain(|_, w| w.strong_count() > 0);
            o.insert(k, Arc::downgrade(&f));
        }
        f
    }
}

impl FileSystem for FatFs {
    fn name(&self) -> &'static str {
        "FAT"
    }

    fn max_component(&self) -> usize {
        12
    }

    fn stat(&self, path: &str) -> Result<FileInfo, u32> {
        let v = self.inner.vol.lock();
        let e = v.lookup(path).map_err(map)?;
        let mut i = info_of(&e, v.cluster_size());
        if path.is_empty() {
            i.attr = attr::DIRECTORY;
        }
        Ok(i)
    }

    fn list(&self, dir: &str) -> Result<Vec<FileInfo>, u32> {
        let v = self.inner.vol.lock();
        let e = v.lookup(dir).map_err(map)?;
        if !e.is_dir() {
            return Err(err::PATH_NOT_FOUND);
        }
        let d = if e.cluster == 0 { v.root_dir() } else { fat::Dir::Cluster(e.cluster) };
        let cs = v.cluster_size();
        Ok(v.read_dir(d).map_err(map)?.iter().map(|e| info_of(e, cs)).collect())
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
            if e == fat::Error::Exists { err::FILE_EXISTS } else { map(e) }
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
        self.inner.vol.lock().rename(from, to).map_err(map)
    }

    fn set_info(&self, path: &str, info: &SetInfo) -> Result<(), u32> {
        let mut v = self.inner.vol.lock();
        let mut e = v.lookup(path).map_err(map)?;
        apply(&mut e, info);
        v.update_entry(&e).map_err(map)
    }

    fn volume(&self) -> Result<VolumeInfo, u32> {
        let mut v = self.inner.vol.lock();
        let i = v.info().map_err(map)?;
        let label: String = v.label.iter().map(|&c| c as char).collect::<String>().trim_end().into();
        Ok(VolumeInfo {
            serial: i.serial,
            label: if label == "NO NAME" { String::new() } else { label },
            sectors_per_unit: i.bytes_per_cluster / i.sector_size,
            units: i.total_clusters,
            free_units: i.free_clusters,
            bytes_per_sector: i.sector_size,
        })
    }

    fn set_label(&self, label: &str) -> Result<(), u32> {
        self.inner.vol.lock().set_label(label).map_err(map)
    }

    fn device(&self) -> Option<Arc<dyn BlockDev>> {
        Some(self.inner.vol.lock().dev.0.clone())
    }
}

fn apply(e: &mut fat::Entry, info: &SetInfo) {
    if let Some(a) = info.attr {
        e.attr = (e.attr & fat::attr::DIRECTORY) | (a as u8 & 0x27);
    }
    if let Some(s) = info.created {
        if s.date != 0 {
            e.created = ts(s);
        }
    }
    if let Some(s) = info.accessed {
        if s.date != 0 {
            e.accessed = s.date;
        }
    }
    if let Some(s) = info.modified {
        if s.date != 0 {
            e.modified = ts(s);
        }
    }
}

pub struct FatFile {
    fs: Arc<Inner>,
    entry: Mutex<fat::Entry>,
}

impl FsFile for FatFile {
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
        info_of(&e, self.fs.vol.lock().cluster_size())
    }

    fn set_info(&self, info: &SetInfo) -> Result<(), u32> {
        let mut e = self.entry.lock();
        apply(&mut e, info);
        self.fs.vol.lock().update_entry(&e).map_err(map)
    }
}
