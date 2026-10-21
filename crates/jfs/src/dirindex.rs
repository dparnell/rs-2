//! Directory index tables (volumes with `flag::DIR_INDEX`, made by
//! Linux): every leaf entry carries an index number, and the directory
//! keeps, per number, where the entry is (its page, 0 for the root, and
//! its position in the page's sorted table) so readdir can resume. The
//! slots for indices 2-13 live in the inode (DIRTABLE); with the 14th
//! the table moves to pages (8 bytes per index, index i at (i - 2) * 8) mapped by
//! a 4-entry xtree in the same place. The directory's size is
//! 8 bytes per index while inline (1 when it never had one), else the
//! table pages' bytes.
//!
//! OS/2 volumes have none of this.

use alloc::vec;

use crate::inode::Inode;
use crate::layout::*;
use crate::{BlockDevice, Result, Volume};

/// Index numbers kept in the inode: 2..=MAX_INLINE_DIRTABLE_ENTRY.
fn inline(next_index: u32) -> bool {
    next_index <= MAX_INLINE_DIRTABLE_ENTRY + 1
}

const SLOT_VALID: u8 = 1;

impl<D: BlockDevice> Volume<D> {
    /// The slot of `index` (for checks): (valid, position, page).
    pub(crate) fn index_get(&mut self, dir: &Inode, index: u32) -> Result<(bool, u8, u64)> {
        let s = match self.index_slot(dir, index)? {
            Some(off) => self.meta_read(off, 8)?,
            None if (2..=MAX_INLINE_DIRTABLE_ENTRY).contains(&index) => {
                let o = di::DIRTABLE + 8 * (index as usize - 2);
                dir.b[o..o + 8].to_vec()
            }
            None => return Ok((false, 0, 0)),
        };
        Ok((s[1] == SLOT_VALID, s[2], (s[3] as u64) << 32 | rd32(&s, 4) as u64))
    }

    /// Device byte offset (external table) or inode offset of `index`'s slot.
    fn index_slot(&mut self, dir: &Inode, index: u32) -> Result<Option<u64>> {
        if inline(rd32(&dir.b, di::NEXT_INDEX)) {
            return Ok(None);
        }
        let (x, _) = self.xt_load_at(dir, di::DIRTABLE)?;
        let off = (index as u64 - 2) * 8;
        let fb = off >> self.l2bsize;
        let e = x.iter().find(|e| e.off <= fb && fb < e.end()).ok_or_else(|| corrupt!())?;
        Ok(Some(((e.addr + fb - e.off) << self.l2bsize) + (off & (self.bsize as u64 - 1))))
    }

    fn index_write(&mut self, dir: &mut Inode, index: u32, slot: [u8; 8]) -> Result<()> {
        match self.index_slot(dir, index)? {
            Some(off) => self.meta_write(off, &slot),
            None => {
                if (2..=MAX_INLINE_DIRTABLE_ENTRY).contains(&index) {
                    let o = di::DIRTABLE + 8 * (index as usize - 2);
                    dir.b[o..o + 8].copy_from_slice(&slot);
                }
                Ok(())
            }
        }
    }

    /// A new index number for an entry about to be inserted into `dir`.
    pub(crate) fn index_add(&mut self, dir: &mut Inode) -> Result<u32> {
        let index = rd32(&dir.b, di::NEXT_INDEX).max(2);
        let nb = (PSIZE >> self.l2bsize) as u32;
        if index == MAX_INLINE_DIRTABLE_ENTRY + 1 {
            // Move the inline slots to a table page.
            let p = self.alloc_meta(dir.ixpxd().addr, nb)?;
            let mut page = vec![0u8; PSIZE];
            page[..96].copy_from_slice(&dir.b[di::DIRTABLE..di::DIRTABLE + 96]);
            self.write_blocks(p.addr, &page)?;
            Self::xt_init_root(dir, di::DIRTABLE, XTROOTINITSLOT_DIR);
            self.xt_store_at(dir, di::DIRTABLE, &[Xad { flag: 0, off: 0, len: nb, addr: p.addr }], &[])?;
            dir.set_nblocks(dir.nblocks() + nb as u64);
        } else if !inline(index) && ((index as u64 - 2) * 8) % PSIZE as u64 == 0 {
            // The table grows by a page.
            let (mut x, pages) = self.xt_load_at(dir, di::DIRTABLE)?;
            let goal = x.last().map_or(0, |e| e.addr + e.len as u64);
            let p = self.alloc_meta(goal, nb)?;
            self.write_blocks(p.addr, &vec![0u8; PSIZE])?;
            crate::xtree::push_xad(&mut x, Xad { flag: 0, off: ((index as u64 - 2) * 8) >> self.l2bsize, len: nb, addr: p.addr });
            let newpages = self.xt_store_at(dir, di::DIRTABLE, &x, &pages)?;
            let tree = newpages.len() as i64 - pages.len() as i64;
            dir.set_nblocks((dir.nblocks() as i64 + nb as i64 + tree * nb as i64) as u64);
        }
        wr32(&mut dir.b, di::NEXT_INDEX, index + 1);
        let size = if inline(index + 1) {
            (index as u64 - 1) * 8
        } else {
            let (x, _) = self.xt_load_at(dir, di::DIRTABLE)?;
            x.iter().map(|e| e.len as u64).sum::<u64>() << self.l2bsize
        };
        dir.set_size(size);
        Ok(index)
    }

    /// Record where entry `index` now is.
    pub(crate) fn index_set(&mut self, dir: &mut Inode, index: u32, page: u64, pos: usize) -> Result<()> {
        if index < 2 {
            return Ok(());
        }
        let mut s = [0u8; 8];
        s[1] = SLOT_VALID;
        s[2] = pos as u8;
        s[3] = (page >> 32) as u8;
        s[4..8].copy_from_slice(&(page as u32).to_le_bytes());
        self.index_write(dir, index, s)
    }

    /// Free `index`; `next` is the index readdir continues with (the
    /// following entry's, or u32::MAX at the end).
    pub(crate) fn index_free(&mut self, dir: &mut Inode, index: u32, next: u32) -> Result<()> {
        if index < 2 {
            return Ok(());
        }
        let mut s = [0u8; 8];
        s[4..8].copy_from_slice(&next.to_le_bytes());
        self.index_write(dir, index, s)
    }
}
