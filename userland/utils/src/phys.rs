//! Whole hard disks through DosPhysicalDisk and the category 9 IOCtls
//! (rs2's LBA functions 0xA0-0xA2), for LVM and SETBOOT.

use alloc::vec::Vec;

use os2::*;

unsafe extern "C" {
    #[link_name = "DOSCALLS.287"]
    fn DosPhysicalDisk(func: u32, buf: *mut u8, cb: u32, params: *const u8, cbp: u32) -> APIRET;
}

/// The number of partitionable disks.
pub fn count() -> u32 {
    let mut n = 0u16;
    unsafe { DosPhysicalDisk(1, &mut n as *mut u16 as *mut u8, 2, core::ptr::null(), 0) };
    n as u32
}

pub struct PhysDisk {
    handle: u16,
    pub sectors: u64,
    pub sector_size: u32,
}

impl PhysDisk {
    /// Disk `n` (from 1).
    pub fn open(n: u32) -> Result<PhysDisk, u32> {
        let name = alloc::format!("{n}:\0");
        let mut h = 0u16;
        let rc = unsafe { DosPhysicalDisk(2, &mut h as *mut u16 as *mut u8, 2, name.as_ptr(), name.len() as u32) };
        if rc != 0 {
            return Err(rc);
        }
        let mut info = [0u8; 12];
        let (mut pl, mut dl) = (0, 0);
        let rc = unsafe { DosDevIOCtl(h as u32, 9, 0xa2, core::ptr::null_mut(), 0, &mut pl, info.as_mut_ptr(), 12, &mut dl) };
        if rc != 0 {
            return Err(rc);
        }
        let sectors = u64::from_le_bytes(info[0..8].try_into().unwrap());
        let sector_size = u32::from_le_bytes(info[8..12].try_into().unwrap());
        Ok(PhysDisk { handle: h, sectors, sector_size })
    }

    fn io(&self, lba: u64, buf: *mut u8, bytes: usize, write: bool) -> Result<(), u32> {
        let count = (bytes / self.sector_size as usize) as u32;
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&(lba as u32).to_le_bytes());
        p[4..8].copy_from_slice(&((lba >> 32) as u32).to_le_bytes());
        p[8..12].copy_from_slice(&count.to_le_bytes());
        let (mut pl, mut dl) = (0, 0);
        let rc = unsafe { DosDevIOCtl(self.handle as u32, 9, if write { 0xa1 } else { 0xa0 }, p.as_mut_ptr(), 12, &mut pl, buf, bytes as u32, &mut dl) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    pub fn read_at(&self, lba: u64, n: usize) -> Result<Vec<u8>, u32> {
        let mut v = alloc::vec![0u8; n * self.sector_size as usize];
        self.io(lba, v.as_mut_ptr(), v.len(), false)?;
        Ok(v)
    }
}

impl Drop for PhysDisk {
    fn drop(&mut self) {
        let h = self.handle;
        unsafe { DosPhysicalDisk(3, core::ptr::null_mut(), 0, &h as *const u16 as *const u8, 2) };
    }
}

impl lvm::Disk for PhysDisk {
    fn sectors(&self) -> u64 {
        self.sectors
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.io(lba, buf.as_mut_ptr(), buf.len(), false)
    }
    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut v = buf.to_vec();
        self.io(lba, v.as_mut_ptr(), v.len(), true)
    }
}
