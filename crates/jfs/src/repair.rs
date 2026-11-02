//! Repairs (CHKDSK /F): what [`Volume::check`] finds is set right from
//! the directory tree, which is taken as the truth.
//!
//! - inodes allocated but in no directory are linked into a new
//!   `FOUND.nnn` directory as `FILEnnnn.CHK` / `DIRnnnn.CHK` (their
//!   blocks kept);
//! - the block map is rewritten to the blocks something owns;
//! - block counts, directory parents and link counts are corrected;
//! - the inode map's free counts are recounted;
//! - the inline log is reset (marked redone: rs2 cannot replay it, the
//!   full check takes its place) and the volume marked clean, so a volume
//!   another system left with its log in use can be written again.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Error, Result, Volume};

/// What a repair did.
#[derive(Debug, Default, Clone)]
pub struct Repairs {
    pub map_blocks: u64,
    pub inodes: u32,
    pub recovered: Vec<String>,
    pub log_reset: bool,
}

impl<D: BlockDevice> Volume<D> {
    fn claim(own: &mut [u8], addr: u64, len: u64) {
        for b in addr..addr + len {
            if let Some(x) = own.get_mut((b / 8) as usize) {
                *x |= 1 << (b % 8);
            }
        }
    }

    /// Blocks of an inode that is in no directory (its data, xtree pages,
    /// directory pages and EAs).
    fn orphan_blocks(&mut self, ino: u32, own: &mut [u8]) -> Result<bool> {
        let i = self.read_inode(ino)?;
        let ea = i.ea();
        if ea.flag & dxd::EXTENT != 0 {
            Self::claim(own, ea.loc.addr, ea.loc.len as u64);
        }
        if i.is_dir() {
            for p in self.dt_pages(&i)? {
                Self::claim(own, p.addr, p.len as u64);
            }
        } else {
            let (x, pages) = self.xt_load(&i)?;
            for e in &x {
                Self::claim(own, e.addr, e.len as u64);
            }
            for p in &pages {
                Self::claim(own, p.addr, p.len as u64);
            }
        }
        Ok(i.is_dir())
    }

    /// Check and repair the volume; returns the problems found and what
    /// was done. A volume opened read-only because of its log is written
    /// all the same.
    pub fn repair(&mut self, now: u32) -> Result<(crate::check::Report, Repairs)> {
        self.readonly = false;
        let (report, mut fx) = self.scan()?;
        let mut done = Repairs::default();
        if report.ok() && self.can_write()? && rd32(&self.sb, sb::STATE) == state::CLEAN {
            return Ok((report, done));
        }
        self.begin()?;
        // Orphans keep their blocks. Those inside an orphaned directory
        // stay there; the others are linked into FOUND.nnn below.
        let mut inner: Vec<u32> = Vec::new();
        let mut dirs: Vec<u32> = Vec::new();
        for &o in &fx.orphans.clone() {
            match self.orphan_blocks(o, &mut fx.owned) {
                Ok(true) => {
                    dirs.push(o);
                    if let Ok(i) = self.read_inode(o) {
                        if let Ok(list) = self.dt_list(&i) {
                            inner.extend(list.iter().map(|e| e.1));
                        }
                    }
                }
                Ok(false) => {}
                // Unreadable: it is freed below.
                Err(_) => {}
            }
        }
        // The block map from what is owned.
        let mapsize = self.bmap.mapsize;
        let mut b = 0u64;
        while b < mapsize {
            let owned = fx.owned[(b / 8) as usize] & (1 << (b % 8)) != 0;
            if owned == self.block_used(b)? {
                b += 1;
                continue;
            }
            let start = b;
            while b < mapsize && (fx.owned[(b / 8) as usize] & (1 << (b % 8)) != 0) == owned && self.block_used(b)? != owned {
                b += 1;
            }
            self.mark_blocks(start, b - start, owned)?;
            done.map_blocks += b - start;
        }
        self.bmap_flush()?;
        // Inode fields.
        for &(ino, n) in &fx.nblocks {
            let mut i = self.read_inode(ino)?;
            i.set_nblocks(n);
            self.write_inode(&i)?;
            done.inodes += 1;
        }
        for &(ino, p) in &fx.parents {
            let mut i = self.read_inode(ino)?;
            wr32(&mut i.b, di::ROOT + 20, p);
            self.write_inode(&i)?;
            done.inodes += 1;
        }
        for &(ino, n) in &fx.links {
            let mut i = self.read_inode(ino)?;
            i.set_nlink(n);
            self.write_inode(&i)?;
            done.inodes += 1;
        }
        // Orphans into FOUND.nnn.
        let top: Vec<u32> = fx.orphans.iter().copied().filter(|o| !inner.contains(o)).collect();
        if !top.is_empty() {
            let mut k = 0;
            let found = loop {
                let name = format!("FOUND.{k:03}");
                match self.mkdir(&name, now) {
                    Ok(e) => break e,
                    Err(Error::Exists) if k < 999 => k += 1,
                    Err(e) => return Err(e),
                }
            };
            self.begin()?;
            let (mut nf, mut nd) = (0, 0);
            for o in top {
                let Ok(mut i) = self.read_inode(o) else { continue };
                let name = if i.is_dir() {
                    nd += 1;
                    format!("DIR{nd:04}.CHK")
                } else {
                    nf += 1;
                    format!("FILE{nf:04}.CHK")
                };
                let units: Vec<u16> = name.encode_utf16().collect();
                let mut pd = self.read_inode(found.ino)?;
                self.dt_insert(&mut pd, &units, o)?;
                if i.is_dir() {
                    pd.set_nlink(pd.nlink() + 1);
                    wr32(&mut i.b, di::ROOT + 20, found.ino);
                } else {
                    i.set_nlink(1);
                }
                self.write_inode(&pd)?;
                self.write_inode(&i)?;
                done.recovered.push(format!("{}\\{name}", found.name));
            }
        }
        self.flush()?;
        // A second pass: link counts and block counts the first changed
        // (FOUND.nnn's directory pages, inner orphans' links).
        let (_, fx2) = self.scan()?;
        for &(ino, n) in &fx2.nblocks {
            let mut i = self.read_inode(ino)?;
            i.set_nblocks(n);
            self.write_inode(&i)?;
        }
        for &(ino, n) in &fx2.links {
            let mut i = self.read_inode(ino)?;
            i.set_nlink(n);
            self.write_inode(&i)?;
        }
        // Inode map counts.
        for &(n, nfree) in &fx2.iag_nfree {
            let mut p = self.iag_read(n)?;
            wr32(&mut p, iag::NFREEINOS, nfree);
            self.iag_write(n, &p)?;
        }
        if let Some((total, free)) = fx2.imap_counts {
            wr32(&mut self.imap.ctl, im::NUMINOS, total);
            wr32(&mut self.imap.ctl, im::NUMFREE, free);
            self.imap.dirty = true;
        }
        self.flush()?;
        // The log: nothing in it is replayed; mark it redone.
        let lp = Pxd::read(&self.sb, sb::LOGPXD);
        if self.flags & flag::INLINELOG != 0 && lp.len != 0 {
            let off = (lp.addr << self.l2bsize) + LOGPSIZE as u64;
            let mut lb = alloc::vec![0u8; LOGPSIZE];
            self.dev_read(off, &mut lb)?;
            if rd32(&lb, logsup::MAGIC) == LOGMAGIC && rd32(&lb, logsup::STATE) != LOGREDONE {
                wr32(&mut lb, logsup::STATE, LOGREDONE);
                self.dev_write(off, &lb)?;
                done.log_reset = true;
            }
        }
        self.marked = true;
        self.mark_clean()?;
        Ok((report, done))
    }
}
