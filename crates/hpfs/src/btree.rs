//! Allocation B+ trees: the extents of a file (or of external EA data).
//!
//! The root lives in the F-node (8 extents or 12 subtrees); further levels
//! are ALSEC sectors (40 extents or 60 subtrees each). Internal entries
//! hold the first file sector *after* their subtree (the last one -1) and
//! the subtree's ALSEC. The tree is rebuilt from the extent list whenever
//! the list changes, reusing the old ALSEC sectors.

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Error, Extent, Result, Volume};

/// Reset a B+ tree header to an empty leaf with room for `cap` extents.
pub(crate) fn init_header(b: &mut [u8], hdr: usize, cap: u8, flags: u8) {
    b[hdr..hdr + bt::ENTRIES].fill(0);
    b[hdr + bt::FLAGS] = flags;
    b[hdr + bt::FREE] = cap;
    wr16(b, hdr + bt::FIRST_FREE, bt::ENTRIES as u16);
}

pub(crate) fn write_leaves(b: &mut [u8], hdr: usize, ext: &[Extent], cap: usize, flags: u8) {
    init_header(b, hdr, (cap - ext.len()) as u8, flags);
    b[hdr + bt::USED] = ext.len() as u8;
    wr16(b, hdr + bt::FIRST_FREE, (bt::ENTRIES + ext.len() * bt::LEAF_SIZE) as u16);
    let area = hdr + bt::ENTRIES;
    b[area..area + cap * bt::LEAF_SIZE].fill(0);
    for (i, x) in ext.iter().enumerate() {
        let o = area + i * bt::LEAF_SIZE;
        wr32(b, o, x.file);
        wr32(b, o + 4, x.len);
        wr32(b, o + 8, x.disk);
    }
}

/// Internal entries for children `(first file sector, ALSEC)`.
pub(crate) fn write_nodes(b: &mut [u8], hdr: usize, kids: &[(u32, u32)], cap: usize, flags: u8) {
    init_header(b, hdr, (cap - kids.len()) as u8, flags | bt::INTERNAL);
    b[hdr + bt::USED] = kids.len() as u8;
    wr16(b, hdr + bt::FIRST_FREE, (bt::ENTRIES + kids.len() * bt::NODE_SIZE) as u16);
    let area = hdr + bt::ENTRIES;
    b[area..area + cap * bt::NODE_SIZE].fill(0);
    for (i, &(_, sec)) in kids.iter().enumerate() {
        let bound = kids.get(i + 1).map_or(u32::MAX, |k| k.0);
        let o = area + i * bt::NODE_SIZE;
        wr32(b, o, bound);
        wr32(b, o + 4, sec);
    }
}

pub(crate) fn new_alsec(sec: u32) -> Vec<u8> {
    let mut a = vec![0u8; SECTOR];
    wr32(&mut a, 0, ALSEC_MAGIC);
    wr32(&mut a, alsec::SELF, sec);
    a
}

/// Merge extents that continue each other on disk.
fn merge(ext: &[Extent]) -> Vec<Extent> {
    let mut v: Vec<Extent> = Vec::with_capacity(ext.len());
    for &x in ext.iter().filter(|x| x.len > 0) {
        match v.last_mut() {
            Some(l) if l.file + l.len == x.file && l.disk + l.len == x.disk => l.len += x.len,
            _ => v.push(x),
        }
    }
    v
}

impl<D: BlockDevice> Volume<D> {
    /// Extents and ALSEC sectors of the tree whose header is at `hdr` in
    /// `buf` (an F-node at `fnode::BTREE` or an ALSEC at `alsec::BTREE`).
    pub(crate) fn tree_of(&self, buf: &[u8], hdr: usize) -> Result<(Vec<Extent>, Vec<u32>)> {
        let mut ext = Vec::new();
        let mut alsecs = Vec::new();
        self.walk_tree(buf, hdr, hdr == fnode::BTREE, &mut ext, &mut alsecs, 0)?;
        Ok((ext, alsecs))
    }

    fn walk_tree(
        &self,
        buf: &[u8],
        hdr: usize,
        in_fnode: bool,
        ext: &mut Vec<Extent>,
        alsecs: &mut Vec<u32>,
        depth: u32,
    ) -> Result<()> {
        let (leaves, nodes) =
            if in_fnode { (bt::FNODE_LEAVES, bt::FNODE_NODES) } else { (bt::ALSEC_LEAVES, bt::ALSEC_NODES) };
        let n = buf[hdr + bt::USED] as usize;
        let area = hdr + bt::ENTRIES;
        if depth > 8 {
            return Err(Error::Corrupt);
        }
        if buf[hdr + bt::FLAGS] & bt::INTERNAL != 0 {
            if n > nodes {
                return Err(Error::Corrupt);
            }
            for i in 0..n {
                let down = rd32(buf, area + i * bt::NODE_SIZE + 4);
                let a = self.read_sector(down)?;
                if rd32(&a, 0) != ALSEC_MAGIC {
                    return Err(Error::Corrupt);
                }
                alsecs.push(down);
                self.walk_tree(&a, alsec::BTREE, false, ext, alsecs, depth + 1)?;
            }
        } else {
            if n > leaves {
                return Err(Error::Corrupt);
            }
            for i in 0..n {
                let o = area + i * bt::LEAF_SIZE;
                ext.push(Extent { file: rd32(buf, o), len: rd32(buf, o + 4), disk: rd32(buf, o + 8) });
            }
        }
        Ok(())
    }

    /// The extents of file F-node `fno` (cached for the last file used).
    pub(crate) fn extents(&mut self, fno: u32) -> Result<Vec<Extent>> {
        if let Some((f, e, _)) = &self.cache
            && *f == fno {
                return Ok(e.clone());
            }
        let f = self.read_fnode(fno)?;
        let (e, a) = self.tree_of(&f, fnode::BTREE)?;
        self.cache = Some((fno, e.clone(), a));
        Ok(e)
    }

    /// Replace the allocation tree of F-node `fno` (held in `f`, which the
    /// caller writes) with one describing `ext`.
    pub(crate) fn store_tree(&mut self, fno: u32, f: &mut [u8], ext: &[Extent]) -> Result<()> {
        let mut pool = match &self.cache {
            Some((c, _, a)) if *c == fno => a.clone(),
            _ => self.tree_of(f, fnode::BTREE)?.1,
        };
        pool.reverse();
        let ext = merge(ext);
        let hdr = fnode::BTREE;
        let mut written = Vec::new();
        if ext.len() <= bt::FNODE_LEAVES {
            write_leaves(f, hdr, &ext, bt::FNODE_LEAVES, 0);
        } else {
            let mut done: Vec<(u32, Vec<u8>)> = Vec::new();
            let mut level: Vec<(u32, u32, Vec<u8>)> = Vec::new();
            for chunk in ext.chunks(bt::ALSEC_LEAVES) {
                let sec = self.take_alsec(&mut pool, fno)?;
                let mut a = new_alsec(sec);
                write_leaves(&mut a, alsec::BTREE, chunk, bt::ALSEC_LEAVES, 0);
                level.push((chunk[0].file, sec, a));
            }
            while level.len() > bt::FNODE_NODES {
                let mut next = Vec::new();
                let mut rest = level;
                while !rest.is_empty() {
                    let tail = rest.split_off(rest.len().min(bt::ALSEC_NODES));
                    let group = core::mem::replace(&mut rest, tail);
                    let sec = self.take_alsec(&mut pool, fno)?;
                    let mut a = new_alsec(sec);
                    let kids: Vec<(u32, u32)> = group.iter().map(|g| (g.0, g.1)).collect();
                    write_nodes(&mut a, alsec::BTREE, &kids, bt::ALSEC_NODES, 0);
                    for (_, s, mut buf) in group {
                        wr32(&mut buf, alsec::UP, sec);
                        done.push((s, buf));
                    }
                    next.push((kids[0].0, sec, a));
                }
                level = next;
            }
            let kids: Vec<(u32, u32)> = level.iter().map(|g| (g.0, g.1)).collect();
            write_nodes(f, hdr, &kids, bt::FNODE_NODES, 0);
            for (_, s, mut buf) in level {
                wr32(&mut buf, alsec::UP, fno);
                buf[alsec::BTREE + bt::FLAGS] |= bt::FNODE_PARENT;
                done.push((s, buf));
            }
            for (s, buf) in &done {
                self.write_secs(*s, buf)?;
                written.push(*s);
            }
        }
        for s in pool {
            self.free_run(s, 1)?;
        }
        self.cache = Some((fno, ext, written));
        Ok(())
    }

    fn take_alsec(&mut self, pool: &mut Vec<u32>, hint: u32) -> Result<u32> {
        match pool.pop() {
            Some(s) => Ok(s),
            None => self.alloc_sector(hint),
        }
    }
}
