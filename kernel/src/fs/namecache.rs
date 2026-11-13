//! Path lookups remembered above the local file systems (`pathcache`): a
//! volume mounted is wrapped in [`CachedFs`], which answers `stat` for
//! directories and for names that do not exist, and forgets on every path
//! operation. Network drives change behind our back and are left alone.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use pathcache::PathCache;

use super::block::BlockDev;
use super::{attr, Ea, FileInfo, FileSystem, FsFile, SetInfo, VolumeInfo};
use crate::api::err;
use crate::sync::SpinLock;

/// Bumped when a disk changed behind the file systems (DosPhysicalDisk).
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// Everything anyone remembers may be wrong now.
pub fn clear_all() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
}

struct Kept {
    paths: PathCache<FileInfo>,
    epoch: u64,
}

pub struct CachedFs {
    inner: Arc<dyn FileSystem>,
    kept: SpinLock<Kept>,
}

/// `fs` with a cache in front of it, if it is a local volume that can have
/// one.
pub fn wrap(fs: Arc<dyn FileSystem>) -> Arc<dyn FileSystem> {
    if fs.remote_name().is_some() || fs.removable() || !matches!(fs.name(), "FAT" | "HPFS" | "JFS") {
        return fs;
    }
    let short = fs.max_component() <= 12;
    Arc::new(CachedFs { inner: fs, kept: SpinLock::new(Kept { paths: PathCache::new(short), epoch: EPOCH.load(Ordering::Relaxed) }) })
}

impl CachedFs {
    fn changed(&self, path: &str) {
        self.kept.lock().paths.changed(path);
    }
}

impl FileSystem for CachedFs {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn stat(&self, path: &str) -> Result<FileInfo, u32> {
        let (key, generation) = {
            let mut k = self.kept.lock();
            let epoch = EPOCH.load(Ordering::Relaxed);
            if k.epoch != epoch {
                k.epoch = epoch;
                k.paths.clear();
            }
            let Some(key) = k.paths.key(path) else {
                drop(k);
                return self.inner.stat(path);
            };
            if let Some(r) = k.paths.get(&key) {
                return r;
            }
            let g = k.paths.generation();
            (key, g)
        };
        let r = self.inner.stat(path);
        let keep = match &r {
            Ok(i) => i.attr & attr::DIRECTORY != 0,
            Err(e) => *e == err::FILE_NOT_FOUND || *e == err::PATH_NOT_FOUND,
        };
        if keep {
            self.kept.lock().paths.put(key, generation, r.clone());
        }
        r
    }

    fn list(&self, dir: &str) -> Result<Vec<FileInfo>, u32> {
        self.inner.list(dir)
    }

    fn open(&self, path: &str) -> Result<Arc<dyn FsFile>, u32> {
        self.inner.open(path)
    }

    fn create(&self, path: &str, attr: u32) -> Result<Arc<dyn FsFile>, u32> {
        let r = self.inner.create(path, attr);
        self.changed(path);
        r
    }

    fn delete(&self, path: &str) -> Result<(), u32> {
        let r = self.inner.delete(path);
        self.changed(path);
        r
    }

    fn mkdir(&self, path: &str) -> Result<(), u32> {
        let r = self.inner.mkdir(path);
        self.changed(path);
        r
    }

    fn rmdir(&self, path: &str) -> Result<(), u32> {
        let r = self.inner.rmdir(path);
        self.changed(path);
        r
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), u32> {
        let r = self.inner.rename(from, to);
        self.changed(from);
        self.changed(to);
        r
    }

    fn set_info(&self, path: &str, info: &SetInfo) -> Result<(), u32> {
        let r = self.inner.set_info(path, info);
        self.changed(path);
        r
    }

    fn volume(&self) -> Result<VolumeInfo, u32> {
        self.inner.volume()
    }

    fn set_label(&self, label: &str) -> Result<(), u32> {
        self.inner.set_label(label)
    }

    fn max_component(&self) -> usize {
        self.inner.max_component()
    }

    fn needs_check(&self) -> bool {
        self.inner.needs_check()
    }

    fn shutdown(&self) {
        self.inner.shutdown()
    }

    fn device(&self) -> Option<Arc<dyn BlockDev>> {
        self.inner.device()
    }

    fn read_eas(&self, path: &str) -> Result<Vec<Ea>, u32> {
        self.inner.read_eas(path)
    }

    fn write_eas(&self, path: &str, eas: &[Ea]) -> Result<(), u32> {
        let r = self.inner.write_eas(path, eas);
        self.changed(path);
        r
    }

    fn remote_name(&self) -> Option<String> {
        self.inner.remote_name()
    }
}
