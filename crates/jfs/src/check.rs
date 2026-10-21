//! A read-only consistency check (CHKDSK without /F, and the tests):
//! walks the directory tree from the root, collects every block owned
//! by an inode or by the fixed metadata, and compares that with the block
//! map; compares reachable inodes with the inode map, and link counts.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Result, Volume};

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub files: u64,
    pub dirs: u64,
    /// Blocks in use by files' data, trees and EAs.
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub problems: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

struct Owners {
    bits: Vec<u8>,
    dup: u64,
}

impl Owners {
    fn claim(&mut self, start: u64, n: u64, r: &mut Report, what: &str) {
        for b in start..start + n {
            let (i, m) = ((b / 8) as usize, 1u8 << (b % 8));
            if i >= self.bits.len() {
                r.problems.push(format!("{what}: block {b} beyond the map"));
                return;
            }
            if self.bits[i] & m != 0 {
                self.dup += 1;
                if self.dup <= 10 {
                    r.problems.push(format!("{what}: block {b} used twice"));
                }
            }
            self.bits[i] |= m;
        }
    }
}

impl<D: BlockDevice> Volume<D> {
    /// Check the volume's metadata; changes nothing.
    pub fn check(&mut self) -> Result<Report> {
        let mut r = Report::default();
        let mapsize = self.bmap.mapsize;
        let mut own = Owners { bits: vec![0u8; mapsize.div_ceil(8) as usize], dup: 0 };
        let l2 = self.l2bsize;
        own.claim(0, AGGR_RSVD_BYTES >> l2, &mut r, "reserved area");
        own.claim(self.ait2, (INODE_EXTENT_SIZE >> l2) as u64, &mut r, "secondary inode table");
        let aim2 = Pxd::read(&self.sb, sb::AIM2);
        own.claim(aim2.addr, aim2.len as u64, &mut r, "secondary inode map");
        for x in self.bmap.file.clone() {
            own.claim(x.addr, x.len as u64, &mut r, "block map");
        }
        let fsi = self.fsi.clone();
        let (ix, ipages) = self.xt_load(&fsi)?;
        for x in &ix {
            own.claim(x.addr, x.len as u64, &mut r, "inode map");
        }
        for p in ipages {
            own.claim(p.addr, p.len as u64, &mut r, "inode map xtree");
        }
        // Inode extents.
        for n in 0..self.iag_count() {
            let p = self.iag_read(n)?;
            for e in 0..EXTSPERIAG as usize {
                let x = Pxd::read(&p, iag::INOEXT + 8 * e);
                if x.addr != 0 {
                    own.claim(x.addr, x.len as u64, &mut r, "inode extent");
                }
            }
        }
        // Walk the tree.
        let mut seen: BTreeMap<u32, u32> = BTreeMap::new(); // inode -> links found
        let mut subdirs: BTreeMap<u32, u32> = BTreeMap::new();
        let mut stack = vec![(ROOT_I, ROOT_I)];
        seen.insert(ROOT_I, 1);
        while let Some((ino, parent)) = stack.pop() {
            let i = match self.read_inode(ino) {
                Ok(i) => i,
                Err(_) => {
                    r.problems.push(format!("inode {ino}: unreadable"));
                    continue;
                }
            };
            if !self.inode_used(ino)? {
                r.problems.push(format!("inode {ino}: in a directory but free in the inode map"));
            }
            let ea = i.ea();
            if ea.flag & dxd::EXTENT != 0 {
                own.claim(ea.loc.addr, ea.loc.len as u64, &mut r, "EA");
            }
            if i.is_dir() {
                r.dirs += 1;
                if i.parent() != parent {
                    r.problems.push(format!("directory {ino}: parent {} should be {parent}", i.parent()));
                }
                match self.dt_pages(&i) {
                    Ok(pages) => {
                        for p in &pages {
                            own.claim(p.addr, p.len as u64, &mut r, "directory page");
                        }
                        let blocks: u64 = pages.iter().map(|p| p.len as u64).sum::<u64>()
                            + if ea.flag & dxd::EXTENT != 0 { ea.loc.len as u64 } else { 0 };
                        if i.nblocks() != blocks {
                            r.problems.push(format!("directory {ino}: {} blocks recorded, {blocks} used", i.nblocks()));
                        }
                    }
                    Err(e) => r.problems.push(format!("directory {ino}: bad tree ({e:?})")),
                }
                let list = match self.dt_list(&i) {
                    Ok(l) => l,
                    Err(e) => {
                        r.problems.push(format!("directory {ino}: unreadable ({e:?})"));
                        continue;
                    }
                };
                for w in list.windows(2) {
                    if crate::name::compare(&w[0].0, &w[1].0, self.ci) != core::cmp::Ordering::Less {
                        r.problems.push(format!("directory {ino}: names out of order"));
                        break;
                    }
                }
                for (_, child) in list {
                    let c = seen.entry(child).or_insert(0);
                    *c += 1;
                    if *c == 1 {
                        stack.push((child, ino));
                    }
                    if let Ok(ci) = self.read_inode(child) {
                        if ci.is_dir() {
                            *subdirs.entry(ino).or_insert(0) += 1;
                        }
                    }
                }
            } else {
                r.files += 1;
                match self.xt_load(&i) {
                    Ok((x, pages)) => {
                        let mut blocks = 0;
                        for e in &x {
                            own.claim(e.addr, e.len as u64, &mut r, "file data");
                            blocks += e.len as u64;
                        }
                        for p in &pages {
                            own.claim(p.addr, p.len as u64, &mut r, "file xtree");
                            blocks += p.len as u64;
                        }
                        if ea.flag & dxd::EXTENT != 0 {
                            blocks += ea.loc.len as u64;
                        }
                        if i.nblocks() != blocks {
                            r.problems.push(format!("file {ino}: {} blocks recorded, {blocks} used", i.nblocks()));
                        }
                        let need = i.size().div_ceil(self.bsize as u64);
                        if x.last().map_or(0, |e| e.end()) > need {
                            r.problems.push(format!("file {ino}: blocks past the end of file"));
                        }
                    }
                    Err(e) => r.problems.push(format!("file {ino}: bad xtree ({e:?})")),
                }
            }
        }
        // Link counts.
        for (&ino, &links) in &seen {
            let Ok(i) = self.read_inode(ino) else { continue };
            let want = if i.is_dir() { 2 + subdirs.get(&ino).copied().unwrap_or(0) } else { links };
            if i.nlink() != want {
                r.problems.push(format!("inode {ino}: link count {} should be {want}", i.nlink()));
            }
        }
        // Inode map: allocated inodes nobody refers to.
        let mut free_inodes = 0u32;
        let mut total_inodes = 0u32;
        for n in 0..self.iag_count() {
            let p = self.iag_read(n)?;
            let mut nfree = 0;
            for e in 0..EXTSPERIAG as usize {
                if Pxd::read(&p, iag::INOEXT + 8 * e).addr == 0 {
                    continue;
                }
                total_inodes += INOSPEREXT;
                let w = rd32(&p, iag::WMAP + 4 * e);
                nfree += w.count_zeros();
                for k in 0..32 {
                    let ino = n * INOSPERIAG + e as u32 * 32 + k;
                    if w & (0x8000_0000 >> k) != 0 && ino >= FIRST_FREE_I && !seen.contains_key(&ino) {
                        r.problems.push(format!("inode {ino}: allocated but in no directory"));
                    }
                }
            }
            if rd32(&p, iag::NFREEINOS) != nfree {
                r.problems.push(format!("IAG {n}: {} free inodes recorded, {nfree} counted", rd32(&p, iag::NFREEINOS)));
            }
            free_inodes += nfree;
        }
        if rd32(&self.imap.ctl, im::NUMFREE) != free_inodes || rd32(&self.imap.ctl, im::NUMINOS) != total_inodes {
            r.problems.push(String::from("inode map: counts wrong"));
        }
        // Block map against what is owned.
        let mut used = 0u64;
        let mut wrong = 0u64;
        for b in 0..mapsize {
            let owned = own.bits[(b / 8) as usize] & (1 << (b % 8)) != 0;
            let marked = self.block_used(b)?;
            if owned {
                used += 1;
            }
            if owned != marked {
                wrong += 1;
                if wrong <= 10 {
                    r.problems.push(format!("block {b}: {} in the map", if marked { "allocated but unused" } else { "used but free" }));
                }
            }
        }
        if wrong > 10 {
            r.problems.push(format!("{wrong} blocks wrong in the map"));
        }
        // Map summaries.
        let mut nfree = 0u64;
        for d in 0..self.dmap_count() {
            let mut p = self.dmap_page_data(d)?;
            let before = p.clone();
            let root = crate::bmap::dmap_rebuild(&mut p);
            if p != before {
                r.problems.push(format!("dmap {d}: tree or count wrong"));
            }
            nfree += rd32(&p, dm::NFREE) as u64;
            let c = self.ctl_page_data(0, d >> L2LPERCTL)?;
            if c[tree::STREE + CTL_LEAFIDX + (d as usize & (LPERCTL - 1))] as i8 != root {
                r.problems.push(format!("dmap {d}: summary wrong"));
            }
        }
        if nfree != self.bmap.nfree {
            r.problems.push(format!("block map: {} free recorded, {nfree} counted", self.bmap.nfree));
        }
        r.used_blocks = used;
        r.free_blocks = nfree;
        Ok(r)
    }
}
