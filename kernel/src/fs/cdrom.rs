//! A CD drive's file system: the drive has its letter from boot, with or
//! without a disc; the disc's ISO 9660 volume is mounted when the drive is
//! first used, and again after a failed access (the disc was taken out or
//! changed), so a disc put in later, or a different one, is found.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::block::BlockDev;
use super::{Ea, FileInfo, FileSystem, FsFile, SetInfo, VolumeInfo};
use crate::api::err;
use crate::sync::SpinLock;

pub struct CdDrive {
    /// The drive, and the drive with hard-error pop-ups.
    raw: Arc<dyn BlockDev>,
    dev: Arc<dyn BlockDev>,
    volume: SpinLock<Option<Arc<dyn FileSystem>>>,
}

impl CdDrive {
    /// A disc there at boot is mounted at once (an empty drive is not
    /// touched until it is used).
    pub fn new(raw: Arc<dyn BlockDev>, has_disc: bool) -> Arc<dyn FileSystem> {
        let dev = crate::api::harderr::wrap(raw.clone());
        let volume = if has_disc { super::iso9660::Iso9660::mount(dev.clone()).ok() } else { None };
        Arc::new(CdDrive { raw, dev, volume: SpinLock::new(volume) })
    }

    /// The disc's volume, mounting it if needed. With `quiet`, an empty
    /// drive is reported as not ready without the hard-error pop-up (for
    /// volume queries; opening and listing files show it, as on OS/2).
    fn mounted(&self, quiet: bool) -> Result<Arc<dyn FileSystem>, u32> {
        if let Some(v) = self.volume.lock().clone() {
            return Ok(v);
        }
        // The first read after a disc change reports it (a unit
        // attention), which is no error to show: read the volume
        // descriptor quietly first.
        let ss = self.raw.sector_size().max(512);
        let mut s = alloc::vec![0u8; ss];
        let mut ok = false;
        for _ in 0..2 {
            if self.raw.read(16 * 2048 / ss as u64, &mut s).is_ok() {
                ok = true;
                break;
            }
        }
        if !ok && quiet {
            return Err(err::NOT_READY);
        }
        let v = super::iso9660::Iso9660::mount(self.dev.clone()).map_err(|_| err::NOT_READY)?;
        *self.volume.lock() = Some(v.clone());
        Ok(v)
    }

    fn fs(&self) -> Result<Arc<dyn FileSystem>, u32> {
        self.mounted(false)
    }

    /// Run `f` on the volume; when it fails with a device error, forget the
    /// volume and try once more on the disc now in the drive.
    fn with<R>(&self, f: impl Fn(&dyn FileSystem) -> Result<R, u32>) -> Result<R, u32> {
        let had = self.volume.lock().is_some();
        match f(&*self.fs()?) {
            Err(e) if is_device_error(e) => {
                *self.volume.lock() = None;
                if had { f(&*self.fs()?) } else { Err(e) }
            }
            r => r,
        }
    }
}

/// Errors that mean the medium, not the request, is at fault.
fn is_device_error(e: u32) -> bool {
    !matches!(
        e,
        err::FILE_NOT_FOUND
            | err::PATH_NOT_FOUND
            | err::ACCESS_DENIED
            | err::NO_MORE_FILES
            | err::WRITE_PROTECT
            | err::NOT_SUPPORTED
            | err::FILE_EXISTS
            | err::INVALID_PARAMETER
            | err::BUFFER_OVERFLOW
    ) && e < 0x100
}

impl FileSystem for CdDrive {
    fn name(&self) -> &'static str {
        "CDFS"
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
    fn create(&self, _path: &str, _attr: u32) -> Result<Arc<dyn FsFile>, u32> {
        Err(err::WRITE_PROTECT)
    }
    fn delete(&self, _path: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn mkdir(&self, _path: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn rmdir(&self, _path: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn rename(&self, _from: &str, _to: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn set_info(&self, _path: &str, _info: &SetInfo) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn volume(&self) -> Result<VolumeInfo, u32> {
        let v = self.mounted(true)?;
        match v.volume() {
            Err(e) if is_device_error(e) => {
                *self.volume.lock() = None;
                self.mounted(true)?.volume()
            }
            r => r,
        }
    }
    fn device(&self) -> Option<Arc<dyn BlockDev>> {
        Some(self.dev.clone())
    }
    fn read_eas(&self, path: &str) -> Result<Vec<Ea>, u32> {
        self.with(|v| v.read_eas(path))
    }
}
