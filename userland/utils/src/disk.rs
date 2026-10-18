//! A logical drive opened for DASD access (CHKDSK, FORMAT): sectors are
//! read and written with the category 8 track IOCtls, like the OS/2
//! utilities do. Reads go through a track cache; writes go to the disk at
//! once and update the cache.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use os2::*;

/// Tracks kept in the cache before it is emptied.
const CACHE_TRACKS: usize = 256;

pub struct Disk {
    hf: u32,
    pub drive: u8,
    pub bpb: BiosParameterBlock,
    pub bps: usize,
    pub sectors: u64,
    spt: u64,
    heads: u64,
    cache: RefCell<BTreeMap<u64, Vec<u8>>>,
}

impl Disk {
    /// Open drive `drive` (0 = A:) for DASD access.
    pub fn open(drive: u8, write: bool) -> Result<Disk, u32> {
        let name = [b'A' + drive, b':', 0];
        let (mut hf, mut act) = (0, 0);
        let access = if write { OPEN_ACCESS_READWRITE } else { OPEN_ACCESS_READONLY };
        let rc = unsafe {
            DosOpen(
                name.as_ptr(),
                &mut hf,
                &mut act,
                0,
                0,
                OPEN_ACTION_OPEN_IF_EXISTS,
                OPEN_FLAGS_DASD | OPEN_FLAGS_FAIL_ON_ERROR | OPEN_SHARE_DENYNONE | access,
                core::ptr::null_mut(),
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        let mut d = Disk {
            hf,
            drive,
            bpb: BiosParameterBlock::default(),
            bps: 512,
            sectors: 0,
            spt: 63,
            heads: 255,
            cache: RefCell::new(BTreeMap::new()),
        };
        let mut cmd = [1u8, 0];
        let mut bpb = BiosParameterBlock::default();
        let rc = d.ioctl(DSK_GETDEVICEPARAMS, &mut cmd, unsafe {
            core::slice::from_raw_parts_mut(&mut bpb as *mut _ as *mut u8, core::mem::size_of::<BiosParameterBlock>())
        });
        if rc != 0 {
            return Err(rc);
        }
        d.bpb = bpb;
        d.bps = bpb.bytes_per_sector as usize;
        d.sectors = if bpb.sectors != 0 { bpb.sectors as u64 } else { bpb.large_sectors as u64 };
        d.spt = bpb.sectors_per_track.max(1) as u64;
        d.heads = bpb.heads.max(1) as u64;
        Ok(d)
    }

    fn ioctl(&self, function: u32, params: &mut [u8], data: &mut [u8]) -> u32 {
        let (mut pl, mut dl) = (params.len() as u32, data.len() as u32);
        unsafe {
            DosDevIOCtl(
                self.hf,
                IOCTL_DISK,
                function,
                params.as_mut_ptr(),
                params.len() as u32,
                &mut pl,
                data.as_mut_ptr(),
                data.len() as u32,
                &mut dl,
            )
        }
    }

    fn simple(&self, function: u32) -> u32 {
        let mut p = [0u8];
        let mut d = [0u8];
        self.ioctl(function, &mut p, &mut d)
    }

    pub fn lock(&self) -> u32 {
        self.simple(DSK_LOCKDRIVE)
    }

    pub fn unlock(&self) -> u32 {
        self.simple(DSK_UNLOCKDRIVE)
    }

    pub fn redetermine(&self) -> u32 {
        self.cache.borrow_mut().clear();
        self.simple(DSK_REDETERMINEMEDIA)
    }

    pub fn track_sectors(&self) -> u64 {
        self.spt
    }

    /// Read or write `n` sectors of track `track` from sector `first`.
    fn track_io(&self, track: u64, first: u64, n: u64, buf: &mut [u8], write: bool) -> u32 {
        let mut p = Vec::with_capacity(9 + 4 * (first + n) as usize);
        p.push(1u8);
        p.extend_from_slice(&((track % self.heads) as u16).to_le_bytes());
        p.extend_from_slice(&((track / self.heads) as u16).to_le_bytes());
        p.extend_from_slice(&(first as u16).to_le_bytes());
        p.extend_from_slice(&(n as u16).to_le_bytes());
        for s in 0..first + n {
            p.extend_from_slice(&(s as u16 + 1).to_le_bytes());
            p.extend_from_slice(&(self.bps as u16).to_le_bytes());
        }
        self.ioctl(if write { DSK_WRITETRACK } else { DSK_READTRACK }, &mut p, buf)
    }

    /// Read a whole track (shorter at the end of the volume) without the cache.
    pub fn read_track(&self, track: u64, buf: &mut Vec<u8>) -> u32 {
        let start = track * self.spt;
        let n = self.spt.min(self.sectors.saturating_sub(start));
        buf.resize(n as usize * self.bps, 0);
        if n == 0 {
            return 0;
        }
        self.track_io(track, 0, n, buf, false)
    }

    fn cached_track(&self, track: u64) -> Result<(), u32> {
        if self.cache.borrow().contains_key(&track) {
            return Ok(());
        }
        let mut buf = Vec::new();
        let rc = self.read_track(track, &mut buf);
        if rc != 0 {
            return Err(rc);
        }
        let mut c = self.cache.borrow_mut();
        if c.len() >= CACHE_TRACKS {
            c.clear();
        }
        c.insert(track, buf);
        Ok(())
    }

    pub fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        let n = (buf.len() / self.bps) as u64;
        if lba + n > self.sectors {
            return Err(27); // ERROR_SECTOR_NOT_FOUND
        }
        let mut done = 0u64;
        while done < n {
            let s = lba + done;
            let (t, i) = (s / self.spt, s % self.spt);
            self.cached_track(t)?;
            let k = (self.spt - i).min(n - done);
            let c = self.cache.borrow();
            let tb = &c[&t];
            let from = i as usize * self.bps;
            let len = k as usize * self.bps;
            buf[done as usize * self.bps..][..len].copy_from_slice(&tb[from..from + len]);
            done += k;
        }
        Ok(())
    }

    pub fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let n = (buf.len() / self.bps) as u64;
        if lba + n > self.sectors {
            return Err(27);
        }
        let mut done = 0u64;
        while done < n {
            let s = lba + done;
            let (t, i) = (s / self.spt, s % self.spt);
            let k = (self.spt - i).min(n - done);
            let len = k as usize * self.bps;
            let at = done as usize * self.bps;
            let mut piece = buf[at..at + len].to_vec();
            let rc = self.track_io(t, i, k, &mut piece, true);
            if rc != 0 {
                return Err(rc);
            }
            if let Some(tb) = self.cache.borrow_mut().get_mut(&t) {
                let from = i as usize * self.bps;
                tb[from..from + len].copy_from_slice(&buf[at..at + len]);
            }
            done += k;
        }
        Ok(())
    }

    /// Write zeros over `count` sectors from `lba`.
    pub fn zero(&self, lba: u64, count: u64) -> Result<(), u32> {
        let chunk = self.spt;
        let z = vec![0u8; chunk as usize * self.bps];
        let mut s = lba;
        while s < lba + count {
            let k = chunk.min(lba + count - s);
            self.write(s, &z[..k as usize * self.bps])?;
            s += k;
        }
        Ok(())
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        unsafe { DosClose(self.hf) };
    }
}

impl fat::BlockDevice for &Disk {
    fn sector_size(&self) -> usize {
        self.bps
    }
    fn sector_count(&self) -> u64 {
        self.sectors
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> fat::Result<()> {
        Disk::read(self, lba, buf).map_err(|_| fat::Error::Io)
    }
    fn write(&self, lba: u64, buf: &[u8]) -> fat::Result<()> {
        Disk::write(self, lba, buf).map_err(|_| fat::Error::Io)
    }
}

impl hpfs::BlockDevice for &Disk {
    fn sector_size(&self) -> usize {
        self.bps
    }
    fn sector_count(&self) -> u64 {
        self.sectors
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> hpfs::Result<()> {
        Disk::read(self, lba, buf).map_err(|_| hpfs::Error::Io)
    }
    fn write(&self, lba: u64, buf: &[u8]) -> hpfs::Result<()> {
        Disk::write(self, lba, buf).map_err(|_| hpfs::Error::Io)
    }
}

/// The file system name of a drive (DosQueryFSAttach), e.g. "FAT".
pub fn fs_name(drive: u8) -> Option<alloc::string::String> {
    let name = [b'A' + drive, b':', 0];
    let mut buf = [0u8; 128];
    let mut cb = buf.len() as u32;
    if unsafe { DosQueryFSAttach(name.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } != 0 {
        return None;
    }
    let cb_name = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    let cb_fsd = u16::from_le_bytes([buf[4], buf[5]]) as usize;
    let at = 8 + cb_name + 1;
    Some(buf[at..at + cb_fsd].iter().map(|&b| b as char).collect())
}
