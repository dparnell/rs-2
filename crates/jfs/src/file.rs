//! File data: reading, writing and resizing through the xtree.
//!
//! Blocks are allocated as they are written, next to the file's last
//! extent when possible; growing a file allocates and zeroes the new
//! blocks (volumes are not sparse). The bytes past the end of file in its
//! last block are kept zero.

use alloc::vec;
use alloc::vec::Vec;

use crate::inode::Inode;
use crate::layout::*;
use crate::xtree::insert_xad;
use crate::{BlockDevice, Entry, Error, Result, Volume};

/// Largest file (JFS allows 2^52 bytes; OS/2 programs see 64-bit sizes).
const MAX_FILE: u64 = 1 << 52;
/// Bytes per device transfer for file data.
const CHUNK: usize = 1 << 20;

/// Where file block `fb` is: (aggregate block, blocks left in the
/// extent, unwritten), or None in a hole.
fn map(x: &[Xad], fb: u64) -> Option<(u64, u64, bool)> {
    let k = x.partition_point(|e| e.end() <= fb);
    let e = x.get(k)?;
    if e.off > fb {
        return None;
    }
    Some((e.addr + (fb - e.off), e.end() - fb, e.flag & xad::NOTRECORDED != 0))
}

/// Blocks to the next mapped block after hole block `fb`.
fn hole_len(x: &[Xad], fb: u64) -> u64 {
    let k = x.partition_point(|e| e.end() <= fb);
    x.get(k).map_or(u64::MAX, |e| e.off - fb)
}

impl<D: BlockDevice> Volume<D> {
    /// Read from a file; returns the bytes read (short at end of file).
    pub fn read(&mut self, e: &Entry, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let i = self.read_inode(e.ino)?;
        if i.is_dir() {
            return Err(Error::IsADirectory);
        }
        let size = i.size();
        if offset >= size {
            return Ok(0);
        }
        let len = ((size - offset).min(buf.len() as u64)) as usize;
        let (x, _) = self.xt_load(&i)?;
        let bs = self.bsize as u64;
        let mut done = 0usize;
        let mut tmp = Vec::new();
        while done < len {
            let pos = offset + done as u64;
            let fb = pos >> self.l2bsize;
            let o = (pos % bs) as usize;
            match map(&x, fb) {
                Some((pb, run, false)) => {
                    if o == 0 && len - done >= bs as usize {
                        // Whole blocks straight into the buffer.
                        let n = ((len - done) / bs as usize).min(run as usize).min(CHUNK / bs as usize);
                        let bytes = n * bs as usize;
                        self.dev_read(pb << self.l2bsize, &mut buf[done..done + bytes])?;
                        done += bytes;
                    } else {
                        tmp.resize(bs as usize, 0);
                        self.dev_read(pb << self.l2bsize, &mut tmp)?;
                        let n = (bs as usize - o).min(len - done);
                        buf[done..done + n].copy_from_slice(&tmp[o..o + n]);
                        done += n;
                    }
                }
                other => {
                    let blocks = match other {
                        Some((_, run, true)) => run,
                        _ => hole_len(&x, fb),
                    };
                    let n = (blocks.saturating_mul(bs) - o as u64).min((len - done) as u64) as usize;
                    buf[done..done + n].fill(0);
                    done += n;
                }
            }
        }
        Ok(len)
    }

    /// Make sure file blocks `from..to` are allocated; newly allocated
    /// blocks outside `keep` (byte range about to be written) are zeroed.
    fn alloc_range(&mut self, i: &Inode, x: &mut Vec<Xad>, from: u64, to: u64, keep: (u64, u64)) -> Result<()> {
        let mut fb = from;
        while fb < to {
            if let Some((_, run, unwritten)) = map(x, fb) {
                if unwritten {
                    // Turn unwritten blocks into zeroed ones.
                    let k = x.partition_point(|e| e.end() <= fb);
                    let e = x[k];
                    self.zero_blocks(e.addr, e.len as u64)?;
                    x[k].flag &= !xad::NOTRECORDED;
                }
                fb += run;
                continue;
            }
            let want = hole_len(x, fb).min(to - fb);
            let goal = {
                let k = x.partition_point(|e| e.end() <= fb);
                if k > 0 { x[k - 1].addr + x[k - 1].len as u64 + (fb - x[k - 1].end()) } else { i.ixpxd().end() }
            };
            let p = self.alloc_blocks(goal, want, 1)?;
            // Zero what the write will not cover.
            let bs = self.bsize as u64;
            for k in 0..p.len as u64 {
                let (s, e) = ((fb + k) * bs, (fb + k + 1) * bs);
                if s < keep.0 || e > keep.1 {
                    self.zero_blocks(p.addr + k, 1)?;
                }
            }
            insert_xad(x, Xad { flag: 0, off: fb, len: p.len, addr: p.addr });
            fb += p.len as u64;
        }
        Ok(())
    }

    fn zero_blocks(&mut self, blk: u64, n: u64) -> Result<()> {
        let per = (CHUNK >> self.l2bsize) as u64;
        let z = vec![0u8; (per.min(n) as usize) << self.l2bsize];
        let mut k = 0;
        while k < n {
            let m = per.min(n - k);
            self.dev_write((blk + k) << self.l2bsize, &z[..(m as usize) << self.l2bsize])?;
            k += m;
        }
        Ok(())
    }

    /// Store a changed extent list and the block count.
    fn store_extents(&mut self, i: &mut Inode, x: &[Xad], pages: &[Pxd]) -> Result<()> {
        let newpages = self.xt_store(i, x, pages)?;
        let data: u64 = x.iter().map(|e| e.len as u64).sum();
        let ea = i.ea();
        let ea_blocks = if ea.flag & dxd::EXTENT != 0 { ea.loc.len as u64 } else { 0 };
        let acl = i.acl();
        let acl_blocks = if acl.flag & dxd::EXTENT != 0 { acl.loc.len as u64 } else { 0 };
        i.set_nblocks(data + newpages.len() as u64 * self.nbperpage as u64 + ea_blocks + acl_blocks);
        Ok(())
    }

    /// Write to a file, extending it as needed. Returns the bytes written.
    pub fn write(&mut self, e: &mut Entry, offset: u64, data: &[u8], now: u32) -> Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset.checked_add(data.len() as u64).filter(|&x| x <= MAX_FILE).ok_or(Error::DiskFull)?;
        self.begin()?;
        let r = self.write_in(e, offset, data, end, now);
        self.end(r)
    }

    fn write_in(&mut self, e: &mut Entry, offset: u64, data: &[u8], end: u64, now: u32) -> Result<usize> {
        let mut i = self.read_inode(e.ino)?;
        if i.is_dir() {
            return Err(Error::IsADirectory);
        }
        let (mut x, pages) = self.xt_load(&i)?;
        let before = x.clone();
        let bs = self.bsize as u64;
        let size = i.size();
        // Allocate from the old end (zero-filling a gap) to the new.
        let from = size.min(offset) / bs;
        let to = end.div_ceil(bs);
        let alloc = self.alloc_range(&i, &mut x, from, to, (offset, end));
        if let Err(err) = alloc {
            // Keep what was allocated: store it so the map stays right.
            if x != before {
                self.store_extents(&mut i, &x, &pages)?;
                self.write_inode(&i)?;
            }
            return Err(err);
        }
        let mut done = 0usize;
        let mut tmp = vec![0u8; bs as usize];
        while done < data.len() {
            let pos = offset + done as u64;
            let fb = pos / bs;
            let o = (pos % bs) as usize;
            let (pb, run, _) = map(&x, fb).ok_or(Error::Corrupt)?;
            if o == 0 && data.len() - done >= bs as usize {
                let n = ((data.len() - done) / bs as usize).min(run as usize).min(CHUNK / bs as usize);
                let bytes = n * bs as usize;
                self.dev_write(pb << self.l2bsize, &data[done..done + bytes])?;
                done += bytes;
            } else {
                let n = (bs as usize - o).min(data.len() - done);
                // A block holding old data, or zeroed just now.
                self.dev_read(pb << self.l2bsize, &mut tmp)?;
                tmp[o..o + n].copy_from_slice(&data[done..done + n]);
                if pos + n as u64 >= size.max(end) {
                    // Keep the bytes past the end of file zero.
                    let tail = (end - fb * bs) as usize;
                    if tail < bs as usize {
                        tmp[tail..].fill(0);
                    }
                }
                self.dev_write(pb << self.l2bsize, &tmp)?;
                done += n;
            }
        }
        if x != before {
            self.store_extents(&mut i, &x, &pages)?;
        }
        if end > size {
            i.set_size(end);
            e.size = end;
        }
        i.set_time(di::MTIME, now);
        i.set_time(di::CTIME, now);
        // Writing sets the archive attribute.
        let m = i.mode() | (crate::attr::ARCHIVE as u32) << mode::ATTRSHIFT;
        i.set_mode(m);
        e.attr |= crate::attr::ARCHIVE;
        e.modified = now;
        self.write_inode(&i)?;
        Ok(data.len())
    }

    /// Change a file's size: shrinking frees the blocks past the end,
    /// growing allocates zeroed blocks.
    pub fn set_size(&mut self, e: &mut Entry, len: u64, now: u32) -> Result<()> {
        if len > MAX_FILE {
            return Err(Error::DiskFull);
        }
        self.begin()?;
        let r = self.set_size_in(e, len, now);
        self.end(r)
    }

    fn set_size_in(&mut self, e: &mut Entry, len: u64, now: u32) -> Result<()> {
        let mut i = self.read_inode(e.ino)?;
        if i.is_dir() {
            return Err(Error::IsADirectory);
        }
        let size = i.size();
        let (mut x, pages) = self.xt_load(&i)?;
        let bs = self.bsize as u64;
        let keep = len.div_ceil(bs);
        if len < size {
            // Free whole blocks from `keep` on.
            let mut out = Vec::with_capacity(x.len());
            for mut ext in x.drain(..) {
                if ext.off >= keep {
                    self.free_blocks(Pxd::new(ext.addr, ext.len))?;
                    continue;
                }
                if ext.end() > keep {
                    let cut = (keep - ext.off) as u32;
                    self.free_blocks(Pxd::new(ext.addr + cut as u64, ext.len - cut))?;
                    ext.len = cut;
                }
                out.push(ext);
            }
            x = out;
            // Zero the rest of the new last block.
            if len % bs != 0 {
                if let Some((pb, _, false)) = map(&x, len / bs) {
                    let mut tmp = vec![0u8; bs as usize];
                    self.dev_read(pb << self.l2bsize, &mut tmp)?;
                    tmp[(len % bs) as usize..].fill(0);
                    self.dev_write(pb << self.l2bsize, &tmp)?;
                }
            }
        } else if len > size {
            let before = x.clone();
            let r = self.alloc_range(&i, &mut x, size / bs, keep, (0, 0));
            if r.is_err() {
                if x != before {
                    self.store_extents(&mut i, &x, &pages)?;
                    self.write_inode(&i)?;
                }
                return r;
            }
        }
        self.store_extents(&mut i, &x, &pages)?;
        i.set_size(len);
        i.set_time(di::MTIME, now);
        i.set_time(di::CTIME, now);
        self.write_inode(&i)?;
        e.size = len;
        e.modified = now;
        Ok(())
    }

    /// Bytes allocated to a file's data.
    pub fn allocated(&mut self, e: &Entry) -> Result<u64> {
        let i = self.read_inode(e.ino)?;
        if i.is_dir() {
            return Ok(0);
        }
        let (x, _) = self.xt_load(&i)?;
        Ok(x.iter().map(|e| e.len as u64).sum::<u64>() << self.l2bsize)
    }
}
