//! xtrees: B+-trees of extents (xads) mapping file blocks to aggregate
//! blocks. The root sits in the inode (8 extents, or 16 when the inline
//! EA area is given up; 16 in aggregate inodes; 4 for a directory's
//! index table); larger trees have 4 KB pages of 254 entries, internal
//! entries pointing at the page that maps from their offset on.
//!
//! rs2 keeps a file's extent list whole: [`Volume::xt_load`] reads the
//! tree, [`Volume::xt_store`] writes it back packed, reusing the old
//! pages and writing only those that changed.

use alloc::vec;
use alloc::vec::Vec;

use crate::inode::Inode;
use crate::layout::*;
use crate::{BlockDevice, Error, Result, Volume};

/// Entries in a 4 KB xtree page.
const PAGE_ENTRIES: usize = XTPAGEMAXSLOT as usize - XTENTRYSTART;

impl<D: BlockDevice> Volume<D> {
    /// An empty leaf root at `at` with `maxentry` slots.
    pub(crate) fn xt_init_root(i: &mut Inode, at: usize, maxentry: u16) {
        i.b[at..at + 32].fill(0);
        i.b[at + 16] = bt::DXD_INDEX | bt::ROOT | bt::LEAF;
        wr16(&mut i.b, at + 18, XTENTRYSTART as u16);
        wr16(&mut i.b, at + 20, maxentry);
    }

    /// The extents of the xtree rooted at di::ROOT, and its pages.
    pub(crate) fn xt_load(&mut self, i: &Inode) -> Result<(Vec<Xad>, Vec<Pxd>)> {
        self.xt_load_at(i, di::ROOT)
    }

    pub(crate) fn xt_load_at(&mut self, i: &Inode, at: usize) -> Result<(Vec<Xad>, Vec<Pxd>)> {
        let mut xads = Vec::new();
        let mut pages = Vec::new();
        let root = i.b[at..at + 32 + 16 * 16.min((DISIZE - at - 32) / 16)].to_vec();
        let max = (root.len() / 16) as u16;
        self.xt_walk(&root, max, 0, &mut xads, &mut pages)?;
        Ok((xads, pages))
    }

    fn xt_walk(&mut self, node: &[u8], max: u16, depth: u32, xads: &mut Vec<Xad>, pages: &mut Vec<Pxd>) -> Result<()> {
        let flag = node[16];
        let next = rd16(node, 18);
        let maxentry = rd16(node, 20);
        if depth > 8 || next < XTENTRYSTART as u16 || next > maxentry || maxentry > max {
            return Err(corrupt!());
        }
        for k in XTENTRYSTART..next as usize {
            let x = Xad::read(node, k * 16);
            if flag & bt::LEAF != 0 {
                if let Some(l) = xads.last() {
                    if x.off < l.end() {
                        return Err(corrupt!());
                    }
                }
                if x.len == 0 || x.addr + x.len as u64 > self.bmap.mapsize + (1 << 30) {
                    return Err(corrupt!());
                }
                xads.push(x);
            } else if flag & bt::INTERNAL != 0 {
                let p = Pxd::new(x.addr, x.len);
                if p.len as usize != PSIZE >> self.l2bsize {
                    return Err(corrupt!());
                }
                pages.push(p);
                let child = self.read_blocks(p.addr, p.len)?;
                self.xt_walk(&child, XTPAGEMAXSLOT, depth + 1, xads, pages)?;
            } else {
                return Err(corrupt!());
            }
        }
        Ok(())
    }

    /// Replace the xtree rooted at `at` with `xads` (sorted, disjoint).
    /// `old` are the tree's current pages, reused first; returns the new
    /// page list. The inode itself is not written.
    pub(crate) fn xt_store_at(&mut self, i: &mut Inode, at: usize, xads: &[Xad], old: &[Pxd]) -> Result<Vec<Pxd>> {
        let mut maxentry = rd16(&i.b, at + 20);
        // A file's root may grow into the inline EA area while it is free.
        if at == di::ROOT
            && !i.is_dir()
            && xads.len() > (maxentry as usize - XTENTRYSTART)
            && maxentry == XTROOTINITSLOT
            && i.mode() & mode::INLINEEA != 0
        {
            maxentry = XTROOTMAXSLOT;
            i.set_mode(i.mode() & !mode::INLINEEA);
            i.b[di::INLINEEA..].fill(0);
        }
        let cap = maxentry as usize - XTENTRYSTART;
        let nb = (PSIZE >> self.l2bsize) as u32;
        // Build the levels bottom up: levels[0] holds the leaf pages.
        let mut levels: Vec<Vec<Vec<Xad>>> = Vec::new();
        let mut level: Vec<Xad> = xads.to_vec();
        while level.len() > cap {
            let pages: Vec<Vec<Xad>> = level.chunks(PAGE_ENTRIES).map(|c| c.to_vec()).collect();
            level = pages.iter().map(|p| Xad { flag: 0, off: p[0].off, len: nb, addr: 0 }).collect();
            levels.push(pages);
        }
        // Assign blocks: reuse old pages, then allocate.
        let total: usize = levels.iter().map(|l| l.len()).sum();
        let mut addrs: Vec<u64> = old.iter().take(total).map(|p| p.addr).collect();
        let goal = i.ixpxd().addr;
        while addrs.len() < total {
            addrs.push(self.alloc_meta(goal, nb)?.addr);
        }
        for p in old.iter().skip(total) {
            self.free_meta(*p)?;
        }
        // Point each level's entries (and the root's) at the pages below.
        let mut base = 0;
        for lv in 0..levels.len() {
            let n = levels[lv].len();
            let mut k = 0;
            if lv + 1 < levels.len() {
                for page in levels[lv + 1].iter_mut() {
                    for x in page.iter_mut() {
                        x.addr = addrs[base + k];
                        k += 1;
                    }
                }
            } else {
                for x in level.iter_mut() {
                    x.addr = addrs[base + k];
                    k += 1;
                }
            }
            base += n;
        }
        // Write the pages (only those that changed).
        let mut base = 0;
        for (lv, pages) in levels.iter().enumerate() {
            let n = pages.len();
            for (k, page) in pages.iter().enumerate() {
                let a = addrs[base + k];
                let mut b = vec![0u8; PSIZE];
                wr64(&mut b, 0, if k + 1 < n { addrs[base + k + 1] } else { 0 });
                wr64(&mut b, 8, if k > 0 { addrs[base + k - 1] } else { 0 });
                b[16] = if lv == 0 { bt::LEAF } else { bt::INTERNAL };
                wr16(&mut b, 18, (XTENTRYSTART + page.len()) as u16);
                wr16(&mut b, 20, XTPAGEMAXSLOT);
                Pxd::new(a, nb).write(&mut b, 24);
                for (j, x) in page.iter().enumerate() {
                    x.write(&mut b, (XTENTRYSTART + j) * 16);
                }
                if self.read_blocks(a, nb)? != b {
                    self.write_blocks(a, &b)?;
                }
            }
            base += n;
        }
        // The root.
        let n = level.len();
        let flag = bt::DXD_INDEX | bt::ROOT | if levels.is_empty() { bt::LEAF } else { bt::INTERNAL };
        i.b[at..at + 16].fill(0);
        i.b[at + 16] = flag;
        i.b[at + 17] = 0;
        wr16(&mut i.b, at + 18, (XTENTRYSTART + n) as u16);
        wr16(&mut i.b, at + 20, maxentry);
        i.b[at + 22..at + 32].fill(0);
        let end = at + maxentry as usize * 16;
        i.b[at + 32..end].fill(0);
        for (j, x) in level.iter().enumerate() {
            x.write(&mut i.b, at + (XTENTRYSTART + j) * 16);
        }
        Ok(addrs.iter().map(|&a| Pxd::new(a, nb)).collect())
    }

    pub(crate) fn xt_store(&mut self, i: &mut Inode, xads: &[Xad], old: &[Pxd]) -> Result<Vec<Pxd>> {
        self.xt_store_at(i, di::ROOT, xads, old)
    }
}

/// Merge `x` into a sorted extent list (appending, or extending the last
/// extent when contiguous on disk).
pub(crate) fn push_xad(v: &mut Vec<Xad>, x: Xad) {
    if let Some(l) = v.last_mut() {
        if l.end() == x.off && l.addr + l.len as u64 == x.addr && l.flag == x.flag && l.len as u64 + x.len as u64 <= 0xff_ffff {
            l.len += x.len;
            return;
        }
    }
    v.push(x);
}

/// Insert `x` (not overlapping anything) keeping the list sorted and
/// merging with neighbours.
pub(crate) fn insert_xad(v: &mut Vec<Xad>, x: Xad) {
    let pos = v.partition_point(|e| e.off < x.off);
    let mut tail = v.split_off(pos);
    push_xad(v, x);
    if let Some(first) = tail.first().copied() {
        let l = v.last_mut().unwrap();
        if l.end() == first.off && l.addr + l.len as u64 == first.addr && l.flag == first.flag && l.len as u64 + first.len as u64 <= 0xff_ffff {
            l.len += first.len;
            tail.remove(0);
        }
    }
    v.append(&mut tail);
}
