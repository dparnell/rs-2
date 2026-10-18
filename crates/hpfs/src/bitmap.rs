//! Free space: the band bitmaps (one bit per sector, 1 = free) and the
//! directory band bitmap (one bit per DNODE, 1 = free).

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Error, Result, Volume};

impl<D: BlockDevice> Volume<D> {
    pub(crate) fn load_bitmaps(&mut self, list: u32) -> Result<()> {
        let bands = self.total.div_ceil(BAND) as usize;
        let mut l = vec![0u8; (bands * 4).div_ceil(SECTOR) * SECTOR];
        self.read_secs(list, &mut l)?;
        for b in 0..bands {
            let s = rd32(&l, b * 4);
            let mut bm = vec![0u8; BITMAP_BYTES];
            self.read_secs(s, &mut bm)?;
            self.bitmap_secs.push(s);
            self.bitmaps.push(bm);
        }
        self.free = (0..self.total).filter(|&s| self.is_free(s)).count() as u32;
        if self.band.count > 0 {
            let mut bits = vec![0u8; BITMAP_BYTES];
            self.read_secs(self.band.bitmap, &mut bits)?;
            self.band.count = self.band.count.min(BITMAP_BYTES as u32 * 8);
            self.band.bits = bits;
        }
        Ok(())
    }

    pub(crate) fn is_free(&self, s: u32) -> bool {
        if s >= self.total {
            return false;
        }
        let bit = (s % BAND) as usize;
        self.bitmaps[(s / BAND) as usize][bit / 8] & (1 << (bit % 8)) != 0
    }

    /// Mark `n` sectors from `start` free or used and write the changed
    /// bitmap sectors.
    fn set_bits(&mut self, start: u32, n: u32, free: bool) -> Result<()> {
        if start as u64 + n as u64 > self.total as u64 {
            return Err(Error::Corrupt);
        }
        let mut dirty: Vec<(usize, usize)> = Vec::new();
        for s in start..start + n {
            let band = (s / BAND) as usize;
            let bit = (s % BAND) as usize;
            let byte = &mut self.bitmaps[band][bit / 8];
            let mask = 1u8 << (bit % 8);
            let was_free = *byte & mask != 0;
            if was_free == free {
                continue;
            }
            if free {
                *byte |= mask;
                self.free += 1;
            } else {
                *byte &= !mask;
                self.free -= 1;
            }
            let d = (band, bit / 8 / SECTOR);
            if dirty.last() != Some(&d) {
                dirty.push(d);
            }
        }
        dirty.dedup();
        for (band, sec) in dirty {
            let data = &self.bitmaps[band][sec * SECTOR..(sec + 1) * SECTOR];
            self.write_secs(self.bitmap_secs[band] + sec as u32, data)?;
        }
        Ok(())
    }

    pub(crate) fn free_run(&mut self, start: u32, n: u32) -> Result<()> {
        self.set_bits(start, n, true)
    }

    /// First free sector at or after `from`.
    fn find_free(&self, from: u32) -> Option<u32> {
        let mut s = from;
        while s < self.total {
            let bit = (s % BAND) as usize;
            let byte = self.bitmaps[(s / BAND) as usize][bit / 8];
            if byte == 0 && bit.is_multiple_of(8) {
                s += 8;
                continue;
            }
            if byte & (1 << (bit % 8)) != 0 {
                return Some(s);
            }
            s += 1;
        }
        None
    }

    /// Take up to `max` free sectors starting exactly at `s`; returns how
    /// many were taken (0 if `s` is in use).
    pub(crate) fn extend_at(&mut self, s: u32, max: u32) -> Result<u32> {
        let mut n = 0;
        while n < max && self.is_free(s + n) {
            n += 1;
        }
        if n > 0 {
            self.set_bits(s, n, false)?;
        }
        Ok(n)
    }

    /// Allocate a run of 1..=`max` sectors, the first free one at or after
    /// `hint` (wrapping around).
    pub(crate) fn alloc_run(&mut self, hint: u32, max: u32) -> Result<(u32, u32)> {
        let s = self.find_free(hint).or_else(|| self.find_free(0)).ok_or(Error::DiskFull)?;
        let n = self.extend_at(s, max)?;
        Ok((s, n))
    }

    pub(crate) fn alloc_sector(&mut self, hint: u32) -> Result<u32> {
        Ok(self.alloc_run(hint, 1)?.0)
    }

    /// Allocate a DNODE (4 sectors): from the directory band if possible,
    /// otherwise 4 aligned sectors of normal space.
    pub(crate) fn alloc_dnode(&mut self, hint: u32) -> Result<u32> {
        if let Some(k) = (0..self.band.count as usize).find(|&k| self.band.bits[k / 8] & (1 << (k % 8)) != 0) {
            self.band.bits[k / 8] &= !(1 << (k % 8));
            self.write_band_bitmap(k)?;
            return Ok(self.band.start + k as u32 * DNODE_SECTORS);
        }
        for from in [hint & !3, 0] {
            let mut s = from;
            while let Some(f) = self.find_free(s) {
                let a = (f + 3) & !3;
                if (a..a + DNODE_SECTORS).all(|x| self.is_free(x)) {
                    self.set_bits(a, DNODE_SECTORS, false)?;
                    return Ok(a);
                }
                s = a + DNODE_SECTORS;
            }
        }
        Err(Error::DiskFull)
    }

    pub(crate) fn free_dnode(&mut self, s: u32) -> Result<()> {
        let b = &self.band;
        if s >= b.start && s < b.start + b.count * DNODE_SECTORS {
            let k = ((s - b.start) / DNODE_SECTORS) as usize;
            self.band.bits[k / 8] |= 1 << (k % 8);
            return self.write_band_bitmap(k);
        }
        self.free_run(s, DNODE_SECTORS)
    }

    fn write_band_bitmap(&mut self, k: usize) -> Result<()> {
        let sec = k / 8 / SECTOR;
        self.write_secs(self.band.bitmap + sec as u32, &self.band.bits[sec * SECTOR..(sec + 1) * SECTOR])
    }

    /// Free DNODEs left in the directory band.
    pub fn free_dir_band_dnodes(&self) -> u32 {
        (0..self.band.count as usize).filter(|&k| self.band.bits[k / 8] & (1 << (k % 8)) != 0).count() as u32
    }
}
