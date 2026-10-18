//! File data: reading, writing, growing and truncating.
//!
//! A file owns exactly the sectors that hold its data
//! (`ceil(size / 512)`); new space is zero-filled by writing, so stale disk
//! contents never become visible.

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{attr, BlockDevice, Entry, Error, Extent, Result, Volume};

/// Largest file size (the size fields are 32 bits).
const MAX_SIZE: u64 = u32::MAX as u64;

/// Disk sector of file sector `fs` and how many sectors follow
/// contiguously within the same extent.
fn map(ext: &[Extent], fs: u32) -> Result<(u32, u32)> {
    let i = ext.partition_point(|x| x.file + x.len <= fs);
    match ext.get(i) {
        Some(x) if x.file <= fs => Ok((x.disk + (fs - x.file), x.len - (fs - x.file))),
        _ => Err(Error::Corrupt),
    }
}

fn allocated(ext: &[Extent]) -> u32 {
    ext.last().map_or(0, |x| x.file + x.len)
}

impl<D: BlockDevice> Volume<D> {
    /// Read file data at `offset`; returns bytes read.
    pub fn read(&mut self, e: &Entry, offset: u64, buf: &mut [u8]) -> Result<usize> {
        if e.is_dir() {
            return Err(Error::IsADirectory);
        }
        if offset >= e.size as u64 {
            return Ok(0);
        }
        let len = buf.len().min((e.size as u64 - offset) as usize);
        let ext = self.extents(e.fnode)?;
        self.transfer(&ext, offset, &mut buf[..len])?;
        Ok(len)
    }

    /// Copy file bytes `offset..offset+buf.len()` into `buf`.
    fn transfer(&self, ext: &[Extent], offset: u64, buf: &mut [u8]) -> Result<()> {
        let mut done = 0;
        let mut sec = vec![0u8; SECTOR];
        while done < buf.len() {
            let pos = offset + done as u64;
            let (disk, run) = map(ext, (pos / SECTOR as u64) as u32)?;
            let within = (pos % SECTOR as u64) as usize;
            let left = buf.len() - done;
            if within == 0 && left >= SECTOR {
                let n = (run as usize).min(left / SECTOR).min(128);
                self.read_secs(disk, &mut buf[done..done + n * SECTOR])?;
                done += n * SECTOR;
            } else {
                self.read_secs(disk, &mut sec)?;
                let n = (SECTOR - within).min(left);
                buf[done..done + n].copy_from_slice(&sec[within..within + n]);
                done += n;
            }
        }
        Ok(())
    }

    /// Store `data` at file offset `offset` (space must be allocated).
    fn store_data(&self, ext: &[Extent], offset: u64, data: &[u8]) -> Result<()> {
        let mut done = 0;
        let mut sec = vec![0u8; SECTOR];
        while done < data.len() {
            let pos = offset + done as u64;
            let (disk, run) = map(ext, (pos / SECTOR as u64) as u32)?;
            let within = (pos % SECTOR as u64) as usize;
            let left = data.len() - done;
            if within == 0 && left >= SECTOR {
                let n = (run as usize).min(left / SECTOR).min(128);
                self.write_secs(disk, &data[done..done + n * SECTOR])?;
                done += n * SECTOR;
            } else {
                let n = (SECTOR - within).min(left);
                self.read_secs(disk, &mut sec)?;
                sec[within..within + n].copy_from_slice(&data[done..done + n]);
                self.write_secs(disk, &sec)?;
                done += n;
            }
        }
        Ok(())
    }

    /// Zero file bytes `from..to`.
    fn zero_range(&self, ext: &[Extent], from: u64, to: u64) -> Result<()> {
        let zeros = vec![0u8; 64 * 1024];
        let mut pos = from;
        while pos < to {
            let n = ((to - pos) as usize).min(zeros.len());
            self.store_data(ext, pos, &zeros[..n])?;
            pos += n as u64;
        }
        Ok(())
    }

    /// Grow the allocation of `ext` to `want` sectors, extending the last
    /// extent in place where possible. On failure nothing stays allocated.
    fn grow(&mut self, fno: u32, ext: &mut Vec<Extent>, want: u32) -> Result<()> {
        let start = allocated(ext);
        let before = ext.clone();
        let mut have = start;
        while have < want {
            let need = want - have;
            if let Some(last) = ext.last_mut() {
                let n = self.extend_at(last.disk + last.len, need)?;
                if n > 0 {
                    last.len += n;
                    have += n;
                    continue;
                }
            }
            let hint = ext.last().map_or(fno + 1, |x| x.disk + x.len);
            match self.alloc_run(hint, need) {
                Ok((s, n)) => {
                    ext.push(Extent { file: have, len: n, disk: s });
                    have += n;
                }
                Err(e) => {
                    self.release_beyond(ext, start)?;
                    *ext = before;
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Free the sectors of `ext` from file sector `keep` on and cut the
    /// list there.
    fn release_beyond(&mut self, ext: &mut Vec<Extent>, keep: u32) -> Result<()> {
        let mut out = Vec::new();
        for x in ext.iter() {
            if x.file >= keep {
                self.free_run(x.disk, x.len)?;
            } else if x.file + x.len > keep {
                let k = keep - x.file;
                self.free_run(x.disk + k, x.len - k)?;
                out.push(Extent { len: k, ..*x });
            } else {
                out.push(*x);
            }
        }
        *ext = out;
        Ok(())
    }

    /// Make the file's allocation exactly `sectors` long; returns the new
    /// extents. Writes the F-node (with `size`).
    fn reallocate(&mut self, fno: u32, sectors: u32, size: u32) -> Result<Vec<Extent>> {
        let mut ext = self.extents(fno)?;
        let old = allocated(&ext);
        if sectors > old {
            self.grow(fno, &mut ext, sectors)?;
        }
        let mut f = self.read_fnode(fno)?;
        if sectors < old {
            self.release_beyond(&mut ext, sectors)?;
        }
        if sectors != old
            && let Err(e) = self.store_tree(fno, &mut f, &ext) {
                if sectors > old {
                    self.release_beyond(&mut ext, old)?;
                }
                self.cache = None;
                return Err(e);
            }
        wr32(&mut f, fnode::FILE_SIZE, size);
        self.write_secs(fno, &f)?;
        Ok(ext)
    }

    /// Write file data at `offset`, extending the file (a gap after the old
    /// end reads as zeros). Updates `e` and its DIRENT.
    pub fn write(&mut self, e: &mut Entry, offset: u64, data: &[u8], now: u32) -> Result<usize> {
        if e.is_dir() {
            return Err(Error::IsADirectory);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset + data.len() as u64;
        if end > MAX_SIZE {
            return Err(Error::DiskFull);
        }
        let old = e.size as u64;
        let ext = if end > old {
            self.reallocate(e.fnode, end.div_ceil(SECTOR as u64) as u32, e.size)?
        } else {
            self.extents(e.fnode)?
        };
        if offset > old {
            self.zero_range(&ext, old, offset)?;
        }
        self.store_data(&ext, offset, data)?;
        e.size = e.size.max(end as u32);
        e.modified = now;
        e.attr |= attr::ARCHIVE;
        self.update_entry(e)?;
        Ok(data.len())
    }

    /// Change the file size (truncate, or extend with zeros).
    pub fn set_size(&mut self, e: &mut Entry, len: u32, now: u32) -> Result<()> {
        if e.is_dir() {
            return Err(Error::IsADirectory);
        }
        let old = e.size;
        let ext = self.reallocate(e.fnode, (len as u64).div_ceil(SECTOR as u64) as u32, len.min(old))?;
        if len > old {
            self.zero_range(&ext, old as u64, len as u64)?;
        }
        e.size = len;
        e.modified = now;
        e.attr |= attr::ARCHIVE;
        self.update_entry(e)
    }

    /// Bytes allocated to a file.
    pub fn allocated(&mut self, e: &Entry) -> Result<u64> {
        if e.is_dir() {
            return Ok(0);
        }
        Ok(allocated(&self.extents(e.fnode)?) as u64 * SECTOR as u64)
    }
}
