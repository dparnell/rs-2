//! The block allocation map: the BMAP_I file of 4 KB pages. Page 0 is
//! the control page; then come the summary (dmapctl) pages of levels 2,
//! 1 and 0 and the dmaps, interleaved so each summary page precedes the
//! pages it covers (see [`dmap_page`], [`ctl_page`]).
//!
//! A dmap covers 8192 blocks: a bitmap (working and persistent copy, one
//! bit per block, the first block in the most significant bit of each
//! 32-bit word) and a buddy tree of maximum free run sizes (log2) over
//! its 256 words. A dmapctl page summarises 1024 lower pages the same
//! way. rs2 keeps both bitmaps equal and recomputes a page's tree after
//! each change.

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Error, Result, Volume};

#[derive(Default)]
pub(crate) struct Bmap {
    /// The control page.
    pub ctl: Vec<u8>,
    /// Extents of the map file.
    pub file: Vec<Xad>,
    pub mapsize: u64,
    pub nfree: u64,
    pub maxlevel: u32,
    pub agl2size: u32,
    pub numag: u32,
    pub agfree: Vec<u64>,
    pub dirty: bool,
}

/// Largest free buddy (log2 size) in a 32-bit map word.
fn maxbud(w: u32) -> i8 {
    if w == 0 {
        return 5;
    }
    if w & 0xffff == 0 || w >> 16 == 0 {
        return 4;
    }
    w.to_le_bytes().iter().map(|&b| budbyte(b)).max().unwrap()
}

fn budbyte(b: u8) -> i8 {
    if b == 0 {
        3
    } else if b & 0xf0 == 0 || b & 0x0f == 0 {
        2
    } else if (0..4).any(|k| (b >> (2 * k)) & 3 == 0) {
        1
    } else if b != 0xff {
        0
    } else {
        -1
    }
}

/// Build a buddy summary tree from its leaves: combine equal free buddies
/// level by level, then each node is the maximum of its four children.
/// Returns the root.
pub(crate) fn build_tree(stree: &mut [u8], leafidx: usize, l2nleafs: u32, budmin: i8) -> i8 {
    let nleafs = 1usize << l2nleafs;
    let l2max = l2nleafs as i32 + budmin as i32;
    let mut l2free = budmin as i32;
    let mut bsize = 1usize;
    while l2free < l2max {
        let next = bsize << 1;
        let mut i = 0;
        while i < nleafs {
            let (a, b) = (leafidx + i, leafidx + i + bsize);
            if stree[a] as i8 as i32 == l2free && stree[b] as i8 as i32 == l2free {
                stree[a] = (l2free + 1) as i8 as u8;
                stree[b] = NOFREE as u8;
            }
            i += next;
        }
        l2free += 1;
        bsize = next;
    }
    let mut child = leafidx;
    let mut nparent = nleafs >> 2;
    while nparent > 0 {
        let parent = (child - 1) >> 2;
        for k in 0..nparent {
            let c = child + 4 * k;
            stree[parent + k] = (0..4).map(|j| stree[c + j] as i8).max().unwrap() as u8;
        }
        child = parent;
        nparent >>= 2;
    }
    stree[0] as i8
}

/// Recompute a dmap page's free count and tree from its working map.
/// Returns the tree root.
pub(crate) fn dmap_rebuild(p: &mut [u8]) -> i8 {
    let nblocks = rd32(p, dm::NBLOCKS) as usize;
    let mut nfree = 0;
    for w in 0..LPERDMAP {
        let word = rd32(p, dm::WMAP + 4 * w);
        let valid = nblocks.saturating_sub(w * 32).min(32);
        let used = if valid == 0 { 0 } else { (word >> (32 - valid)).count_ones() as usize };
        nfree += valid - used;
        p[dm::TREE + tree::STREE + DMAP_LEAFIDX + w] = maxbud(word) as u8;
    }
    wr32(p, dm::NFREE, nfree as u32);
    let t = dm::TREE;
    wr32(p, t + tree::NLEAFS, LPERDMAP as u32);
    wr32(p, t + tree::L2NLEAFS, 8);
    wr32(p, t + tree::LEAFIDX, DMAP_LEAFIDX as u32);
    wr32(p, t + tree::HEIGHT, 4);
    p[t + tree::BUDMIN] = 5;
    build_tree(&mut p[t + tree::STREE..t + tree::STREE + DMAP_TREESIZE], DMAP_LEAFIDX, 8, 5)
}

/// Rebuild a dmapctl page's tree (level `level`) from its leaves.
pub(crate) fn ctl_rebuild(p: &mut [u8], level: u32) -> i8 {
    wr32(p, tree::NLEAFS, LPERCTL as u32);
    wr32(p, tree::L2NLEAFS, L2LPERCTL);
    wr32(p, tree::LEAFIDX, CTL_LEAFIDX as u32);
    wr32(p, tree::HEIGHT, 5);
    let budmin = (L2BPERDMAP + level * L2LPERCTL) as i8;
    p[tree::BUDMIN] = budmin as u8;
    build_tree(&mut p[tree::STREE..tree::STREE + CTL_TREESIZE], CTL_LEAFIDX, L2LPERCTL, budmin)
}

/// A fresh dmap page for the `n` blocks from `start` (all free; the
/// words past the end marked allocated).
pub(crate) fn new_dmap(start: u64, n: u32) -> Vec<u8> {
    let mut p = vec![0u8; PSIZE];
    wr32(&mut p, dm::NBLOCKS, n);
    wr64(&mut p, dm::START, start);
    for w in 0..LPERDMAP {
        let valid = (n as usize).saturating_sub(w * 32).min(32);
        let word = if valid == 32 { 0 } else { u32::MAX >> valid };
        wr32(&mut p, dm::WMAP + 4 * w, word);
        wr32(&mut p, dm::PMAP + 4 * w, word);
    }
    dmap_rebuild(&mut p);
    p
}

/// The highest allocation group with blocks in use, and the preferred
/// group for allocations: the leftmost active one with at least the
/// average free space (as JFS keeps them; jfs_fsck checks both).
pub(crate) fn ag_hints(agfree: &[u64], numag: u32, agsize: u64, mapsize: u64, nfree: u64, pref: u32) -> (u32, u32) {
    let blocks = |a: u32| agsize.min(mapsize - a as u64 * agsize);
    let mut maxag = numag.saturating_sub(1);
    while maxag > 0 && agfree[maxag as usize] == blocks(maxag) {
        maxag -= 1;
    }
    let actags = maxag + 1;
    let inactfree: u64 = (actags..numag).map(blocks).sum();
    let avg = (nfree - inactfree.min(nfree)) / actags as u64;
    let mut agpref = pref;
    if agpref >= actags || agfree[agpref as usize] < avg {
        agpref = (0..actags).find(|&a| agfree[a as usize] >= avg).unwrap_or(0);
    }
    (maxag, agpref)
}

impl<D: BlockDevice> Volume<D> {
    pub(crate) fn bmap_mount(&mut self, file: Vec<Xad>) -> Result<()> {
        self.bmap.file = file;
        let ctl = self.bmap_read_page(0)?;
        let mapsize = rd64(&ctl, bm::MAPSIZE);
        let l2nbpp = rd32(&ctl, bm::L2NBPERPAGE);
        let numag = rd32(&ctl, bm::NUMAG);
        if mapsize == 0 || l2nbpp != log2(self.nbperpage as u64) || numag == 0 || numag as usize > MAXAG {
            return Err(Error::Corrupt);
        }
        self.bmap.mapsize = mapsize;
        self.bmap.nfree = rd64(&ctl, bm::NFREE);
        self.bmap.maxlevel = rd32(&ctl, bm::MAXLEVEL);
        self.bmap.agl2size = rd32(&ctl, bm::AGL2SIZE);
        self.bmap.numag = numag;
        self.bmap.agfree = (0..MAXAG).map(|a| rd64(&ctl, bm::AGFREE + 8 * a)).collect();
        if self.bmap.maxlevel > 2 || self.bmap.agl2size > 40 {
            return Err(Error::Corrupt);
        }
        self.bmap.ctl = ctl;
        Ok(())
    }

    /// Aggregate block of page `p` of the map file.
    fn bmap_block(&self, p: u64) -> Result<u64> {
        let lb = p << log2(self.nbperpage as u64);
        let f = &self.bmap.file;
        let k = f.partition_point(|x| x.end() <= lb);
        match f.get(k) {
            Some(x) if x.off <= lb => Ok(x.addr + (lb - x.off)),
            _ => Err(Error::Corrupt),
        }
    }

    fn bmap_read_page(&mut self, p: u64) -> Result<Vec<u8>> {
        let b = self.bmap_block(p)?;
        self.read_blocks(b, self.nbperpage)
    }

    fn bmap_write_page(&mut self, p: u64, data: &[u8]) -> Result<()> {
        let b = self.bmap_block(p)?;
        self.write_blocks(b, data)
    }

    fn ndmaps(&self) -> u64 {
        self.bmap.mapsize.div_ceil(BPERDMAP)
    }

    /// Free-run summary of dmap `d` (its tree root) from the level 0 page.
    fn dmap_summary(&mut self, d: u64) -> Result<i8> {
        let p = self.bmap_read_page(ctl_page(0, d >> L2LPERCTL))?;
        Ok(p[tree::STREE + CTL_LEAFIDX + (d as usize & (LPERCTL - 1))] as i8)
    }

    /// Store a changed summary value for child `n` at `level`, upwards.
    fn ctl_update(&mut self, level: u32, n: u64, value: i8) -> Result<()> {
        let pg = ctl_page(level, n >> L2LPERCTL);
        let mut p = self.bmap_read_page(pg)?;
        let leaf = tree::STREE + CTL_LEAFIDX + (n as usize & (LPERCTL - 1));
        if p[leaf] as i8 == value && rd32(&p, tree::NLEAFS) != 0 {
            return Ok(());
        }
        p[leaf] = value as u8;
        let root = ctl_rebuild(&mut p, level);
        self.bmap_write_page(pg, &p)?;
        if level < self.bmap.maxlevel {
            self.ctl_update(level + 1, n >> L2LPERCTL, root)
        } else {
            self.bmap.ctl[bm::MAXFREEBUD] = root as u8;
            self.bmap.dirty = true;
            Ok(())
        }
    }

    /// Mark `n` blocks from `start` allocated (`used`) or free.
    pub(crate) fn mark_blocks(&mut self, start: u64, n: u64, used: bool) -> Result<()> {
        if n == 0 {
            return Ok(());
        }
        if start + n > self.bmap.mapsize {
            return Err(Error::Corrupt);
        }
        let mut b = start;
        let end = start + n;
        while b < end {
            let d = b >> L2BPERDMAP;
            let dend = ((d + 1) << L2BPERDMAP).min(end);
            let pg = dmap_page(d);
            let mut p = self.bmap_read_page(pg)?;
            for blk in b..dend {
                let o = (blk & (BPERDMAP - 1)) as usize;
                let (w, bit) = (o / 32, 0x8000_0000u32 >> (o % 32));
                let off = dm::WMAP + 4 * w;
                let word = rd32(&p, off);
                if (word & bit != 0) == used {
                    return Err(Error::Corrupt);
                }
                let word = if used { word | bit } else { word & !bit };
                wr32(&mut p, off, word);
                wr32(&mut p, dm::PMAP + 4 * w, word);
            }
            let root = dmap_rebuild(&mut p);
            self.bmap_write_page(pg, &p)?;
            self.ctl_update(0, d, root)?;
            // Free counts, per allocation group.
            let mut x = b;
            while x < dend {
                let ag = (x >> self.bmap.agl2size) as usize;
                let agend = (((ag as u64) + 1) << self.bmap.agl2size).min(dend);
                let k = agend - x;
                if ag < MAXAG {
                    let f = &mut self.bmap.agfree[ag];
                    *f = if used { f.saturating_sub(k) } else { *f + k };
                }
                x = agend;
            }
            let k = dend - b;
            self.bmap.nfree = if used { self.bmap.nfree.saturating_sub(k) } else { self.bmap.nfree + k };
            b = dend;
        }
        self.bmap.dirty = true;
        Ok(())
    }

    pub(crate) fn free_blocks(&mut self, p: Pxd) -> Result<()> {
        self.mark_blocks(p.addr, p.len as u64, false)
    }

    /// The first free run of at least `min` blocks (at most `want`) at or
    /// after `from`, searching up to `to`.
    fn find_run(&mut self, from: u64, to: u64, want: u64, min: u64) -> Result<Option<(u64, u64)>> {
        let mut run = (0u64, 0u64);
        let mut b = from;
        while b < to {
            let d = b >> L2BPERDMAP;
            let dstart = d << L2BPERDMAP;
            let dend = (dstart + BPERDMAP).min(to);
            if self.dmap_summary(d)? == NOFREE {
                if run.1 >= min {
                    return Ok(Some(run));
                }
                run = (0, 0);
                b = dend;
                continue;
            }
            let p = self.bmap_read_page(dmap_page(d))?;
            while b < dend {
                let o = (b - dstart) as usize;
                let word = rd32(&p, dm::WMAP + 4 * (o / 32));
                if o % 32 == 0 && dend - b >= 32 && (word == 0 || word == u32::MAX) {
                    if word == 0 {
                        if run.1 == 0 {
                            run.0 = b;
                        }
                        run.1 += 32;
                        if run.1 >= want {
                            return Ok(Some((run.0, want)));
                        }
                    } else {
                        if run.1 >= min {
                            return Ok(Some(run));
                        }
                        run = (0, 0);
                    }
                    b += 32;
                    continue;
                }
                if word & (0x8000_0000 >> (o % 32)) == 0 {
                    if run.1 == 0 {
                        run.0 = b;
                    }
                    run.1 += 1;
                    if run.1 >= want {
                        return Ok(Some((run.0, want)));
                    }
                } else {
                    if run.1 >= min {
                        return Ok(Some(run));
                    }
                    run = (0, 0);
                }
                b += 1;
            }
        }
        Ok(if run.1 >= min { Some(run) } else { None })
    }

    /// Allocate up to `want` contiguous blocks (at least `min`), near
    /// `goal`.
    pub(crate) fn alloc_blocks(&mut self, goal: u64, want: u64, min: u64) -> Result<Pxd> {
        let want = want.min(0xff_ffff);
        if self.bmap.nfree < min {
            return Err(Error::DiskFull);
        }
        let goal = if goal >= self.bmap.mapsize { 0 } else { goal };
        let size = self.bmap.mapsize;
        // A long run near the goal first, then anything big enough.
        let long = want.min(256).max(min);
        let found = match self.find_run(goal, size, want, long)? {
            Some(r) => Some(r),
            None => match self.find_run(0, goal.min(size), want, long)? {
                Some(r) => Some(r),
                None if long > min => match self.find_run(goal, size, want, min)? {
                    Some(r) => Some(r),
                    None => self.find_run(0, goal, want, min)?,
                },
                None => None,
            },
        };
        let Some((start, len)) = found else { return Err(Error::DiskFull) };
        let len = len.min(want);
        self.mark_blocks(start, len, true)?;
        Ok(Pxd::new(start, len as u32))
    }

    /// Allocate `n` contiguous blocks for metadata.
    pub(crate) fn alloc_meta(&mut self, goal: u64, n: u32) -> Result<Pxd> {
        let p = self.alloc_blocks(goal, n as u64, n as u64)?;
        self.forget(p.addr, p.len as u64);
        Ok(p)
    }

    /// Write the control page if it changed.
    pub(crate) fn bmap_flush(&mut self) -> Result<()> {
        if !self.bmap.dirty {
            return Ok(());
        }
        let mut c = core::mem::take(&mut self.bmap.ctl);
        wr64(&mut c, bm::NFREE, self.bmap.nfree);
        for a in 0..MAXAG {
            wr64(&mut c, bm::AGFREE + 8 * a, self.bmap.agfree[a]);
        }
        let (maxag, agpref) = ag_hints(&self.bmap.agfree, self.bmap.numag, rd64(&c, bm::AGSIZE), self.bmap.mapsize, self.bmap.nfree, rd32(&c, bm::AGPREF));
        wr32(&mut c, bm::MAXAG, maxag);
        wr32(&mut c, bm::AGPREF, agpref);
        let r = self.bmap_write_page(0, &c);
        self.bmap.ctl = c;
        r?;
        self.bmap.dirty = false;
        Ok(())
    }

    /// Is block `b` allocated? (for checks)
    pub(crate) fn block_used(&mut self, b: u64) -> Result<bool> {
        let p = self.bmap_read_page(dmap_page(b >> L2BPERDMAP))?;
        let o = (b & (BPERDMAP - 1)) as usize;
        Ok(rd32(&p, dm::WMAP + 4 * (o / 32)) & (0x8000_0000 >> (o % 32)) != 0)
    }

    pub(crate) fn dmap_count(&self) -> u64 {
        self.ndmaps()
    }

    pub(crate) fn dmap_page_data(&mut self, d: u64) -> Result<Vec<u8>> {
        self.bmap_read_page(dmap_page(d))
    }

    pub(crate) fn ctl_page_data(&mut self, level: u32, n: u64) -> Result<Vec<u8>> {
        self.bmap_read_page(ctl_page(level, n))
    }
}
