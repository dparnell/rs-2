//! CHKDSK for HPFS: walk the tree from the root F-node, collect every
//! sector a reachable structure owns (F-nodes, DNODEs, ALSECs, file data,
//! external EA and ACL runs), and compare that with the band bitmaps and
//! the directory band bitmap. With `fix`:
//!
//! - sectors in use but marked free are marked used (they would be handed
//!   out again: the dangerous kind);
//! - sectors marked used that nothing owns are freed, outside the volume's
//!   fixed structures (boot area, super and spare blocks, bitmaps and
//!   their list, the bad block list, hot-fix table and replacements, code
//!   page sectors, spare DNODEs, the directory band and its bitmap);
//! - DNODEs of the directory band are marked as owned or free;
//! - the spare block's dirty flag is cleared.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Result, Volume};

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub dirs: u64,
    pub files: u64,
    /// Sectors in files' data, in directories (DNODEs, F-nodes), in EAs.
    pub file_sectors: u64,
    pub dir_sectors: u64,
    pub ea_sectors: u64,
    pub problems: Vec<String>,
    /// Sectors marked used / freed / DNODEs corrected by the repair.
    pub marked_used: u32,
    pub freed: u32,
    pub dnodes: u32,
    /// The volume was marked dirty (not shut down cleanly).
    pub was_dirty: bool,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

struct Bits(Vec<u8>);

impl Bits {
    fn set(&mut self, s: u32, n: u32) {
        for x in s..s.saturating_add(n) {
            if let Some(b) = self.0.get_mut((x / 8) as usize) {
                *b |= 1 << (x % 8);
            }
        }
    }
    fn get(&self, s: u32) -> bool {
        self.0.get((s / 8) as usize).is_some_and(|b| b & (1 << (s % 8)) != 0)
    }
}

impl<D: BlockDevice> Volume<D> {
    /// Sectors of the fixed structures (never freed by a repair).
    fn fixed(&self, sp: &[u8], s: &[u8]) -> Result<Bits> {
        let mut f = Bits(vec![0u8; (self.total as usize).div_ceil(8)]);
        f.set(0, SPARE_LBA + 3);
        let bands = self.total.div_ceil(BAND);
        f.set(rd32(s, sb::BITMAPS), (bands * 4).div_ceil(SECTOR as u32).max(4));
        for &b in &self.bitmap_secs {
            f.set(b, (BITMAP_BYTES / SECTOR) as u32);
        }
        if rd32(s, sb::BAD_LIST) != 0 {
            f.set(rd32(s, sb::BAD_LIST), 4);
        }
        let list = rd32(sp, sp::HOTFIX_LIST);
        if list != 0 {
            f.set(list, 4);
            let count = (rd32(sp, sp::HOTFIX_COUNT) as usize).min(SECTOR * 4 / 12);
            let mut b = vec![0u8; 4 * SECTOR];
            self.read_secs(list, &mut b)?;
            for i in 0..count {
                f.set(rd32(&b, (count + i) * 4), 1);
            }
        }
        let cpd = rd32(sp, sp::CODE_PAGE_DIR);
        if cpd != 0 && cpd < self.total {
            f.set(cpd, 1);
            let d = self.read_sector(cpd)?;
            if rd32(&d, 0) == CPDIR_MAGIC {
                for i in 0..(rd32(&d, 4) as usize).min(31) {
                    f.set(rd32(&d, 16 + i * 16 + 8), 1);
                }
            }
        }
        for i in 0..(rd32(sp, sp::SPARE_DNODES) as usize).min(sp::MAX_SPARE_DNODES) {
            f.set(rd32(sp, sp::SPARE_DNODE_LIST + 4 * i), DNODE_SECTORS);
        }
        // The structures FORMAT puts at the start of the volume form one
        // area with alignment gaps: all of it up to the last of them.
        let mut end = 0;
        for x in 0..BAND.min(self.total) {
            if f.get(x) {
                end = x + 1;
            }
        }
        f.set(0, end);
        f.set(self.band.start, self.band.count * DNODE_SECTORS);
        if self.band.count > 0 {
            f.set(self.band.bitmap, (BITMAP_BYTES / SECTOR) as u32);
            // FORMAT puts the bitmap and the root F-node just before the
            // band, in one run.
            if self.band.bitmap < self.band.start && self.band.start - self.band.bitmap <= 16 {
                f.set(self.band.bitmap, self.band.start - self.band.bitmap);
            }
        }
        Ok(f)
    }

    fn in_band(&self, s: u32) -> Option<usize> {
        let b = &self.band;
        (s >= b.start && s < b.start + b.count * DNODE_SECTORS).then(|| ((s - b.start) / DNODE_SECTORS) as usize)
    }

    /// Check the volume (and with `fix` repair it, see the module docs).
    pub fn check(&mut self, fix: bool) -> Result<Report> {
        let mut r = Report::default();
        let s = self.read_sector(SUPER_LBA)?;
        let mut sp = self.read_sector(SPARE_LBA)?;
        r.was_dirty = sp[sp::FLAGS] & 1 != 0;
        let fixed = self.fixed(&sp, &s)?;
        let mut own = Bits(vec![0u8; (self.total as usize).div_ceil(8)]);
        let mut band_own = vec![false; self.band.count as usize];
        let claim = |v: &Self, own: &mut Bits, r: &mut Report, start: u32, n: u32, what: &str| {
            if start as u64 + n as u64 > v.total as u64 {
                r.problems.push(format!("{what}: sectors {start}+{n} beyond the volume"));
                return;
            }
            for x in start..start + n {
                if own.get(x) {
                    r.problems.push(format!("{what}: sector {x} used twice"));
                    break;
                }
            }
            own.set(start, n);
        };
        let mut stack: Vec<(u32, String)> = vec![(self.root, String::new())];
        let mut seen = 0u32;
        while let Some((fno, path)) = stack.pop() {
            seen += 1;
            if seen > self.total {
                break;
            }
            claim(self, &mut own, &mut r, fno, 1, &path);
            let f = match self.read_fnode(fno) {
                Ok(f) => f,
                Err(_) => {
                    r.problems.push(format!("{path}: F-node {fno} unreadable"));
                    continue;
                }
            };
            for (size, sec, anode) in [(fnode::EA_EXT_SIZE, fnode::EA_SECTOR, fnode::EA_IN_ANODE), (fnode::ACL_EXT_SIZE, fnode::ACL_SECTOR, fnode::ACL_IN_ANODE)] {
                let n = rd32(&f, size);
                if n != 0 {
                    let at = rd32(&f, sec);
                    if f[anode] != 0 {
                        // Kept in an allocation tree: its ALSEC and runs.
                        claim(self, &mut own, &mut r, at, 1, &path);
                        if let Ok(a) = self.read_sector(at) {
                            if let Ok((ext, als)) = self.tree_of(&a, alsec::BTREE) {
                                for e in ext {
                                    claim(self, &mut own, &mut r, e.disk, e.len, &path);
                                    r.ea_sectors += e.len as u64;
                                }
                                for x in als {
                                    claim(self, &mut own, &mut r, x, 1, &path);
                                }
                            }
                        }
                    } else {
                        let k = n.div_ceil(SECTOR as u32);
                        claim(self, &mut own, &mut r, at, k, &path);
                        r.ea_sectors += k as u64;
                    }
                }
            }
            if f[fnode::FLAGS] & fnode::DIR != 0 {
                r.dirs += 1;
                r.dir_sectors += 1;
                let top = match self.dir_root(fno) {
                    Ok(t) => t,
                    Err(_) => {
                        r.problems.push(format!("{path}\\: directory tree damaged"));
                        continue;
                    }
                };
                // DNODEs, depth first.
                let mut dn = vec![(top, 0u32)];
                while let Some((d, depth)) = dn.pop() {
                    if depth > 32 {
                        r.problems.push(format!("{path}\\: directory tree too deep"));
                        break;
                    }
                    match self.in_band(d) {
                        Some(k) => band_own[k] = true,
                        None => claim(self, &mut own, &mut r, d, DNODE_SECTORS, &path),
                    }
                    r.dir_sectors += DNODE_SECTORS as u64;
                    let node = match self.read_dnode(d) {
                        Ok(n) => n,
                        Err(_) => {
                            r.problems.push(format!("{path}\\: DNODE {d} unreadable"));
                            continue;
                        }
                    };
                    for e in node.entries {
                        if let Some(c) = e.down {
                            dn.push((c, depth + 1));
                        }
                        if e.is_end() || e.name == crate::DOTDOT {
                            continue;
                        }
                        let name = crate::name::to_string(&e.name);
                        if e.attr & 0x10 == 0 {
                            r.files += 1;
                            // The file's F-node, ALSECs and data.
                            claim(self, &mut own, &mut r, e.fnode, 1, &name);
                            match self.read_fnode(e.fnode).and_then(|ff| self.tree_of(&ff, fnode::BTREE)) {
                                Ok((ext, als)) => {
                                    for x in ext {
                                        claim(self, &mut own, &mut r, x.disk, x.len, &format!("{path}\\{name}"));
                                        r.file_sectors += x.len as u64;
                                    }
                                    for a in als {
                                        claim(self, &mut own, &mut r, a, 1, &name);
                                    }
                                }
                                Err(_) => r.problems.push(format!("{path}\\{name}: allocation damaged")),
                            }
                            if let Ok(ff) = self.read_fnode(e.fnode) {
                                let n = rd32(&ff, fnode::EA_EXT_SIZE);
                                if n != 0 && ff[fnode::EA_IN_ANODE] == 0 {
                                    let k = n.div_ceil(SECTOR as u32);
                                    claim(self, &mut own, &mut r, rd32(&ff, fnode::EA_SECTOR), k, &name);
                                    r.ea_sectors += k as u64;
                                }
                            }
                        } else {
                            stack.push((e.fnode, format!("{path}\\{name}")));
                        }
                    }
                }
            }
        }
        // Compare with the bitmaps.
        let mut used_free = Vec::new();
        let mut leaked = Vec::new();
        for x in 0..self.total {
            let owned = own.get(x);
            if owned && self.is_free(x) {
                used_free.push(x);
            } else if !owned && !self.is_free(x) && !fixed.get(x) {
                leaked.push(x);
            }
        }
        if !used_free.is_empty() {
            r.problems.push(format!("{} sectors in use are marked free", used_free.len()));
        }
        if !leaked.is_empty() {
            r.problems.push(format!("{} sectors are marked in use but belong to nothing", leaked.len()));
        }
        let mut band_wrong = Vec::new();
        for (k, &o) in band_own.iter().enumerate() {
            let free = self.band.bits[k / 8] & (1 << (k % 8)) != 0;
            if o == free {
                band_wrong.push(k);
            }
        }
        if !band_wrong.is_empty() {
            r.problems.push(format!("{} DNODEs of the directory band are marked wrongly", band_wrong.len()));
        }
        if fix {
            for x in used_free {
                self.set_bits_pub(x, false)?;
                r.marked_used += 1;
            }
            for x in leaked {
                self.free_run(x, 1)?;
                r.freed += 1;
            }
            for k in band_wrong {
                self.band.bits[k / 8] ^= 1 << (k % 8);
                let sec = k / 8 / SECTOR;
                self.write_secs(self.band.bitmap + sec as u32, &self.band.bits[sec * SECTOR..(sec + 1) * SECTOR])?;
                r.dnodes += 1;
            }
            if r.was_dirty {
                sp[sp::FLAGS] &= !1;
                self.write_secs(SPARE_LBA, &sp)?;
            }
            self.cache = None;
        }
        Ok(r)
    }

    fn set_bits_pub(&mut self, s: u32, free: bool) -> Result<()> {
        if free {
            self.free_run(s, 1)
        } else {
            // Taking exactly one known sector.
            let got = self.extend_at(s, 1)?;
            if got == 1 { Ok(()) } else { Err(crate::Error::Corrupt) }
        }
    }
}
