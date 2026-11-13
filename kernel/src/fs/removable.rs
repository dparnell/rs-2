//! A diskette drive's file system: the drive has its letter from boot,
//! with or without a diskette. The diskette's volume (FAT, or whatever
//! `probe_volume` finds) is mounted when the drive is first used, and
//! again whenever the drive's change line says the diskette was changed;
//! a DSK_REDETERMINEMEDIA or the end of DASD writes (FORMAT) forgets it
//! too (`fs::remount`).

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::block::BlockDev;
use super::{Ea, FileInfo, FileSystem, FsFile, SetInfo, VolumeInfo};
use crate::api::err;
use crate::drivers::floppy;
use crate::sync::SpinLock;

pub struct Removable {
    drive: Arc<floppy::Drive>,
    /// The drive with hard-error pop-ups.
    dev: Arc<dyn BlockDev>,
    /// The mounted volume and the diskette generation it belongs to.
    volume: SpinLock<Option<(Arc<dyn FileSystem>, u32)>>,
}

impl Removable {
    pub fn new(drive: Arc<floppy::Drive>) -> Arc<dyn FileSystem> {
        let raw: Arc<dyn BlockDev> = drive.clone();
        let dev = crate::api::harderr::wrap(raw);
        Arc::new(Removable { drive, dev, volume: SpinLock::new(None) })
    }

    /// The diskette's volume, mounting it if needed. With `quiet`, an
    /// empty drive is reported as not ready without the hard-error
    /// pop-up (volume queries); opening and listing files show it.
    fn mounted(&self, quiet: bool) -> Result<Arc<dyn FileSystem>, u32> {
        self.drive.check_change();
        let generation = self.drive.generation();
        if let Some((v, g)) = self.volume.lock().clone() {
            if g == generation {
                return Ok(v);
            }
        }
        if self.drive.media().is_err() {
            if quiet {
                return Err(err::NOT_READY);
            }
            // Through the pop-up (Retry finds a diskette put in meanwhile).
            let mut s = alloc::vec![0u8; 512];
            self.dev.read(0, &mut s)?;
        }
        let v = super::probe_volume(self.dev.clone());
        *self.volume.lock() = Some((v.clone(), self.drive.generation()));
        Ok(v)
    }

    fn with<R>(&self, f: impl Fn(&dyn FileSystem) -> Result<R, u32>) -> Result<R, u32> {
        f(&*self.mounted(false)?)
    }

    /// Forget the mounted volume (it is mounted again when next used).
    pub fn forget(&self) {
        let old = self.volume.lock().take();
        drop(old);
    }
}

impl FileSystem for Removable {
    fn name(&self) -> &'static str {
        match self.volume.lock().as_ref() {
            Some((v, _)) if v.name() != "UNKNOWN" => v.name(),
            _ => "FAT",
        }
    }
    fn stat(&self, path: &str) -> Result<FileInfo, u32> {
        self.with(|v| v.stat(path))
    }
    fn list(&self, dir: &str) -> Result<Vec<FileInfo>, u32> {
        self.with(|v| v.list(dir))
    }
    fn open(&self, path: &str) -> Result<Arc<dyn FsFile>, u32> {
        self.with(|v| v.open(path))
    }
    fn create(&self, path: &str, attr: u32) -> Result<Arc<dyn FsFile>, u32> {
        self.with(|v| v.create(path, attr))
    }
    fn delete(&self, path: &str) -> Result<(), u32> {
        self.with(|v| v.delete(path))
    }
    fn mkdir(&self, path: &str) -> Result<(), u32> {
        self.with(|v| v.mkdir(path))
    }
    fn rmdir(&self, path: &str) -> Result<(), u32> {
        self.with(|v| v.rmdir(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), u32> {
        self.with(|v| v.rename(from, to))
    }
    fn set_info(&self, path: &str, info: &SetInfo) -> Result<(), u32> {
        self.with(|v| v.set_info(path, info))
    }
    fn volume(&self) -> Result<VolumeInfo, u32> {
        self.mounted(true)?.volume()
    }
    fn set_label(&self, label: &str) -> Result<(), u32> {
        self.with(|v| v.set_label(label))
    }
    fn max_component(&self) -> usize {
        self.volume.lock().as_ref().map_or(12, |(v, _)| v.max_component())
    }
    fn shutdown(&self) {
        if let Some((v, _)) = self.volume.lock().clone() {
            v.shutdown();
        }
    }
    fn device(&self) -> Option<Arc<dyn BlockDev>> {
        Some(self.dev.clone())
    }
    fn read_eas(&self, path: &str) -> Result<Vec<Ea>, u32> {
        self.with(|v| v.read_eas(path))
    }
    fn write_eas(&self, path: &str, eas: &[Ea]) -> Result<(), u32> {
        self.with(|v| v.write_eas(path, eas))
    }
    fn removable(&self) -> bool {
        true
    }
    fn forget_medium(&self) {
        self.forget();
    }
}
