//! Block devices.

use alloc::sync::Arc;

use crate::api::err;

pub trait BlockDev: Send + Sync {
    fn sector_size(&self) -> usize;
    fn sector_count(&self) -> u64;
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32>;
    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32>;
}

/// A disk image in memory (handed over by the boot loader).
pub struct RamDisk {
    base: *mut u8,
    len: usize,
}

unsafe impl Send for RamDisk {}
unsafe impl Sync for RamDisk {}

impl RamDisk {
    pub fn from_module(m: &bootinfo::Module) -> Arc<dyn BlockDev> {
        Arc::new(RamDisk { base: crate::mm::phys::phys_to_virt(m.base), len: m.len as usize })
    }
}

impl BlockDev for RamDisk {
    fn sector_size(&self) -> usize {
        512
    }

    fn sector_count(&self) -> u64 {
        (self.len / 512) as u64
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        let o = lba as usize * 512;
        if o + buf.len() > self.len {
            return Err(err::INVALID_PARAMETER);
        }
        unsafe { core::ptr::copy_nonoverlapping(self.base.add(o), buf.as_mut_ptr(), buf.len()) };
        Ok(())
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let o = lba as usize * 512;
        if o + buf.len() > self.len {
            return Err(err::INVALID_PARAMETER);
        }
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), self.base.add(o), buf.len()) };
        Ok(())
    }
}

/// Adapter so the shared FAT crate can use kernel block devices.
pub struct FatDev(pub Arc<dyn BlockDev>);

impl fat::BlockDevice for FatDev {
    fn sector_size(&self) -> usize {
        self.0.sector_size()
    }

    fn sector_count(&self) -> u64 {
        self.0.sector_count()
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> fat::Result<()> {
        self.0.read(lba, buf).map_err(|_| fat::Error::Io)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> fat::Result<()> {
        self.0.write(lba, buf).map_err(|_| fat::Error::Io)
    }
}
