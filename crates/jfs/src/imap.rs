//! The fileset inode map (FILESYSTEM_I): a control page, then one 4 KB
//! IAG per 4096 inodes. An IAG holds the addresses of up to 128 inode
//! extents (32 inodes each), a bit per inode (working and persistent
//! map) and summary bits per extent. IAGs with free inodes, and with
//! free extent slots, are chained per allocation group from the control
//! page; IAGs with no extents at all on a free IAG list.
//!
//! rs2 allocates inode extents as needed and does not give them back
//! (an extent with all its inodes free is valid).

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Error, Result, Volume};

#[derive(Default)]
pub(crate) struct Imap {
    /// The control page.
    pub ctl: Vec<u8>,
    /// Extents of the map file.
    pub file: Vec<Xad>,
    pub dirty: bool,
}

const NIL: u32 = u32::MAX;

impl<D: BlockDevice> Volume<D> {
    pub(crate) fn imap_mount(&mut self, file: Vec<Xad>) -> Result<()> {
        self.imap.file = file;
        self.imap.ctl = self.imap_read_page(0)?;
        let next = rd32(&self.imap.ctl, im::NEXTIAG);
        if next == 0 || next > 1 << 20 {
            return Err(corrupt!());
        }
        Ok(())
    }

    fn imap_block(&self, p: u64) -> Result<u64> {
        let lb = p << log2(self.nbperpage as u64);
        let f = &self.imap.file;
        let k = f.partition_point(|x| x.end() <= lb);
        match f.get(k) {
            Some(x) if x.off <= lb => Ok(x.addr + (lb - x.off)),
            _ => Err(corrupt!()),
        }
    }

    fn imap_read_page(&mut self, p: u64) -> Result<Vec<u8>> {
        let b = self.imap_block(p)?;
        self.read_blocks(b, self.nbperpage)
    }

    pub(crate) fn iag_read(&mut self, n: u32) -> Result<Vec<u8>> {
        if n >= self.iag_count() {
            return Err(corrupt!());
        }
        self.imap_read_page(n as u64 + 1)
    }

    fn iag_write(&mut self, n: u32, p: &[u8]) -> Result<()> {
        let b = self.imap_block(n as u64 + 1)?;
        self.write_blocks(b, p)
    }

    pub(crate) fn iag_count(&self) -> u32 {
        rd32(&self.imap.ctl, im::NEXTIAG)
    }

    /// Device byte offset of fileset inode `ino`.
    pub(crate) fn inode_off(&mut self, ino: u32) -> Result<u64> {
        let p = self.iag_read(ino >> L2INOSPERIAG)?;
        let ext = ((ino >> 5) & (EXTSPERIAG - 1)) as usize;
        let x = Pxd::read(&p, iag::INOEXT + 8 * ext);
        if x.addr == 0 {
            return Err(corrupt!());
        }
        Ok((x.addr << self.l2bsize) + (ino & (INOSPEREXT - 1)) as u64 * DISIZE as u64)
    }

    /// Is fileset inode `ino` allocated?
    pub(crate) fn inode_used(&mut self, ino: u32) -> Result<bool> {
        let n = ino >> L2INOSPERIAG;
        if n >= self.iag_count() {
            return Ok(false);
        }
        let p = self.iag_read(n)?;
        let ext = (ino >> 5) & (EXTSPERIAG - 1);
        Ok(rd32(&p, iag::WMAP + 4 * ext as usize) & (0x8000_0000 >> (ino & 31)) != 0)
    }

    fn ag_of(&self, blk: u64) -> usize {
        ((blk >> self.bmap.agl2size) as usize).min(MAXAG - 1)
    }

    fn agctl(&self, ag: usize, field: usize) -> u32 {
        rd32(&self.imap.ctl, im::AGCTL + 16 * ag + field)
    }

    fn set_agctl(&mut self, ag: usize, field: usize, v: u32) {
        wr32(&mut self.imap.ctl, im::AGCTL + 16 * ag + field, v);
        self.imap.dirty = true;
    }

    fn add_ctl(&mut self, field: usize, delta: i32) {
        let v = rd32(&self.imap.ctl, field) as i32 + delta;
        wr32(&mut self.imap.ctl, field, v as u32);
        self.imap.dirty = true;
    }

    fn add_agctl(&mut self, ag: usize, field: usize, delta: i32) {
        let v = self.agctl(ag, field) as i32 + delta;
        self.set_agctl(ag, field, v as u32);
    }

    // List links: (head field in agctl / control page, fwd, back).

    /// Put IAG `n` (page `p`, written by the caller) at the head of a
    /// list.
    fn list_insert(&mut self, n: u32, p: &mut [u8], ag: usize, head: usize, fwd: usize, back: usize) -> Result<()> {
        let h = self.agctl(ag, head);
        if h != NIL {
            let mut hp = self.iag_read(h)?;
            wr32(&mut hp, back, n);
            self.iag_write(h, &hp)?;
        }
        wr32(p, fwd, h);
        wr32(p, back, NIL);
        self.set_agctl(ag, head, n);
        Ok(())
    }

    fn list_remove(&mut self, _n: u32, p: &mut [u8], ag: usize, head: usize, fwd: usize, back: usize) -> Result<()> {
        let (f, b) = (rd32(p, fwd), rd32(p, back));
        if f != NIL {
            let mut fp = self.iag_read(f)?;
            wr32(&mut fp, back, b);
            self.iag_write(f, &fp)?;
        }
        if b != NIL {
            let mut bp = self.iag_read(b)?;
            wr32(&mut bp, fwd, f);
            self.iag_write(b, &bp)?;
        } else {
            self.set_agctl(ag, head, f);
        }
        wr32(p, fwd, NIL);
        wr32(p, back, NIL);
        Ok(())
    }

    /// Allocate an inode (near the parent directory's).
    pub(crate) fn ialloc(&mut self, near: u32, _dir: bool) -> Result<u32> {
        let count = self.iag_count();
        let first = (near >> L2INOSPERIAG).min(count - 1);
        // An IAG with a free inode, then one with room for an extent.
        for pass in 0..2 {
            for k in 0..count {
                let n = (first + k) % count;
                let p = self.iag_read(n)?;
                if pass == 0 && rd32(&p, iag::NFREEINOS) > 0 {
                    return self.ialloc_in(n, p);
                }
                if pass == 1 && rd32(&p, iag::NFREEEXTS) > 0 {
                    let p = self.new_extent(n, p)?;
                    return self.ialloc_in(n, p);
                }
            }
        }
        let n = self.new_iag()?;
        let p = self.iag_read(n)?;
        let p = self.new_extent(n, p)?;
        self.ialloc_in(n, p)
    }

    /// Take a free inode of IAG `n` (which has one).
    fn ialloc_in(&mut self, n: u32, mut p: Vec<u8>) -> Result<u32> {
        let ag = self.ag_of(rd64(&p, iag::AGSTART));
        for e in 0..EXTSPERIAG as usize {
            let w = rd32(&p, iag::WMAP + 4 * e);
            let alloc = Pxd::read(&p, iag::INOEXT + 8 * e).addr != 0;
            if !alloc || w == u32::MAX {
                continue;
            }
            let bit = w.leading_ones();
            let w = w | 0x8000_0000 >> bit;
            wr32(&mut p, iag::WMAP + 4 * e, w);
            wr32(&mut p, iag::PMAP + 4 * e, w);
            if w == u32::MAX {
                let s = iag::INOSMAP + 4 * (e / 32);
                { let v = rd32(&p, s); wr32(&mut p, s, v | 0x8000_0000 >> (e % 32)); }
            }
            let nf = rd32(&p, iag::NFREEINOS) - 1;
            wr32(&mut p, iag::NFREEINOS, nf);
            if nf == 0 {
                self.list_remove(n, &mut p, ag, 0, iag::INOFREEFWD, iag::INOFREEBACK)?;
            }
            self.iag_write(n, &p)?;
            self.add_ctl(im::NUMFREE, -1);
            self.add_agctl(ag, 12, -1);
            return Ok(n * INOSPERIAG + e as u32 * INOSPEREXT + bit);
        }
        Err(corrupt!())
    }

    /// Give IAG `n` a new inode extent; returns the updated page.
    fn new_extent(&mut self, n: u32, mut p: Vec<u8>) -> Result<Vec<u8>> {
        let e = (0..EXTSPERIAG as usize).find(|&e| Pxd::read(&p, iag::INOEXT + 8 * e).addr == 0).ok_or_else(|| corrupt!())?;
        let nb = (INODE_EXTENT_SIZE >> self.l2bsize) as u32;
        let empty = rd32(&p, iag::NFREEEXTS) == EXTSPERIAG;
        let mut ag = self.ag_of(rd64(&p, iag::AGSTART));
        // Near the IAG's other extents, inside its allocation group.
        let goal = (0..EXTSPERIAG as usize)
            .map(|k| Pxd::read(&p, iag::INOEXT + 8 * k))
            .filter(|x| x.addr != 0)
            .map(|x| x.end())
            .max()
            .unwrap_or((ag as u64) << self.bmap.agl2size);
        let x = self.alloc_meta(goal, nb)?;
        if empty {
            // Leaves the free IAG list, joins its group's lists.
            self.free_iag_remove(n, &mut p)?;
            ag = self.ag_of(x.addr);
            wr64(&mut p, iag::AGSTART, (ag as u64) << self.bmap.agl2size);
        }
        // Initialise the 32 inodes.
        let base = n * INOSPERIAG + e as u32 * INOSPEREXT;
        let mut buf = vec![0u8; INODE_EXTENT_SIZE];
        for k in 0..INOSPEREXT as usize {
            let b = &mut buf[k * DISIZE..(k + 1) * DISIZE];
            wr32(b, di::INOSTAMP, self.inostamp);
            wr32(b, di::FILESET, FILESYSTEM_I);
            wr32(b, di::NUMBER, base + k as u32);
            x.write(b, di::IXPXD);
        }
        self.write_blocks(x.addr, &buf)?;
        x.write(&mut p, iag::INOEXT + 8 * e);
        wr32(&mut p, iag::WMAP + 4 * e, 0);
        wr32(&mut p, iag::PMAP + 4 * e, 0);
        let s = 4 * (e / 32);
        let bit = 0x8000_0000u32 >> (e % 32);
        { let v = rd32(&p, iag::EXTSMAP + s); wr32(&mut p, iag::EXTSMAP + s, v | bit); }
        { let v = rd32(&p, iag::INOSMAP + s); wr32(&mut p, iag::INOSMAP + s, v & !bit); }
        let had_free = rd32(&p, iag::NFREEINOS) > 0;
        { let v = rd32(&p, iag::NFREEINOS); wr32(&mut p, iag::NFREEINOS, v + INOSPEREXT); }
        let fe = rd32(&p, iag::NFREEEXTS) - 1;
        wr32(&mut p, iag::NFREEEXTS, fe);
        if !had_free {
            self.list_insert(n, &mut p, ag, 0, iag::INOFREEFWD, iag::INOFREEBACK)?;
        }
        if empty && fe > 0 {
            self.list_insert(n, &mut p, ag, 4, iag::EXTFREEFWD, iag::EXTFREEBACK)?;
        } else if !empty && fe == 0 {
            self.list_remove(n, &mut p, ag, 4, iag::EXTFREEFWD, iag::EXTFREEBACK)?;
        }
        self.iag_write(n, &p)?;
        self.add_ctl(im::NUMINOS, INOSPEREXT as i32);
        self.add_ctl(im::NUMFREE, INOSPEREXT as i32);
        self.add_agctl(ag, 8, INOSPEREXT as i32);
        self.add_agctl(ag, 12, INOSPEREXT as i32);
        Ok(p)
    }

    /// Take IAG `n` off the free IAG list.
    fn free_iag_remove(&mut self, n: u32, p: &mut [u8]) -> Result<()> {
        let mut prev = NIL;
        let mut cur = rd32(&self.imap.ctl, im::FREEIAG);
        let mut guard = 0;
        while cur != NIL && guard < 1 << 20 {
            if cur == n {
                let next = rd32(p, iag::IAGFREE);
                if prev == NIL {
                    wr32(&mut self.imap.ctl, im::FREEIAG, next);
                    self.imap.dirty = true;
                } else {
                    let mut pp = self.iag_read(prev)?;
                    wr32(&mut pp, iag::IAGFREE, next);
                    self.iag_write(prev, &pp)?;
                }
                wr32(p, iag::IAGFREE, NIL);
                return Ok(());
            }
            prev = cur;
            cur = rd32(&self.iag_read(cur)?, iag::IAGFREE);
            guard += 1;
        }
        Ok(())
    }

    /// Append an IAG to the map (growing the map file by a page).
    fn new_iag(&mut self) -> Result<u32> {
        let n = self.iag_count();
        if n >= 1 << 19 {
            return Err(Error::DiskFull);
        }
        let nb = self.nbperpage;
        let lb = (n as u64 + 1) << log2(nb as u64);
        let goal = self.imap.file.last().map_or(0, |x| x.addr + x.len as u64);
        let page = self.alloc_meta(goal, nb)?;
        let mut xads = self.imap.file.clone();
        crate::xtree::push_xad(&mut xads, Xad { flag: 0, off: lb, len: nb, addr: page.addr });
        let mut fsi = self.fsi.clone();
        let (_, pages) = self.xt_load(&fsi)?;
        let newpages = self.xt_store(&mut fsi, &xads, &pages)?;
        fsi.set_size((n as u64 + 2) * PSIZE as u64);
        let data: u64 = xads.iter().map(|x| x.len as u64).sum();
        fsi.set_nblocks(data + newpages.len() as u64 * nb as u64);
        self.fsi = fsi;
        self.fsi_dirty = true;
        self.imap.file = xads;
        let mut p = vec![0u8; PSIZE];
        wr32(&mut p, iag::IAGNUM, n);
        for f in [iag::INOFREEFWD, iag::INOFREEBACK, iag::EXTFREEFWD, iag::EXTFREEBACK, iag::IAGFREE] {
            wr32(&mut p, f, NIL);
        }
        for k in 0..4 {
            wr32(&mut p, iag::INOSMAP + 4 * k, u32::MAX);
        }
        wr32(&mut p, iag::NFREEEXTS, EXTSPERIAG);
        // On the free IAG list until it gets an extent.
        wr32(&mut p, iag::IAGFREE, rd32(&self.imap.ctl, im::FREEIAG));
        wr32(&mut self.imap.ctl, im::FREEIAG, n);
        wr32(&mut self.imap.ctl, im::NEXTIAG, n + 1);
        self.imap.dirty = true;
        self.write_blocks(page.addr, &p)?;
        Ok(n)
    }

    /// Free inode `ino`.
    pub(crate) fn ifree(&mut self, ino: u32) -> Result<()> {
        let n = ino >> L2INOSPERIAG;
        let mut p = self.iag_read(n)?;
        let ag = self.ag_of(rd64(&p, iag::AGSTART));
        let e = ((ino >> 5) & (EXTSPERIAG - 1)) as usize;
        let bit = 0x8000_0000u32 >> (ino & 31);
        let w = rd32(&p, iag::WMAP + 4 * e);
        if w & bit == 0 {
            return Err(corrupt!());
        }
        wr32(&mut p, iag::WMAP + 4 * e, w & !bit);
        wr32(&mut p, iag::PMAP + 4 * e, w & !bit);
        let s = iag::INOSMAP + 4 * (e / 32);
        { let v = rd32(&p, s); wr32(&mut p, s, v & !(0x8000_0000 >> (e % 32))); }
        let nf = rd32(&p, iag::NFREEINOS);
        wr32(&mut p, iag::NFREEINOS, nf + 1);
        if nf == 0 {
            self.list_insert(n, &mut p, ag, 0, iag::INOFREEFWD, iag::INOFREEBACK)?;
        }
        self.iag_write(n, &p)?;
        self.add_ctl(im::NUMFREE, 1);
        self.add_agctl(ag, 12, 1);
        Ok(())
    }

    pub(crate) fn imap_flush(&mut self) -> Result<()> {
        if !self.imap.dirty {
            return Ok(());
        }
        let c = core::mem::take(&mut self.imap.ctl);
        let b = self.imap_block(0);
        let r = b.and_then(|b| self.write_blocks(b, &c));
        self.imap.ctl = c;
        r?;
        self.imap.dirty = false;
        Ok(())
    }
}
