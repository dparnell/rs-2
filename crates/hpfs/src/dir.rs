//! Directories: B-trees of DNODEs holding DIRENTs.
//!
//! A DNODE (2 KB) holds entries sorted by name in collating order, ending
//! with an END entry. In an internal DNODE every entry, END included,
//! carries a down pointer to the subtree of names that sort before it; leaf
//! DNODEs carry none. All leaves are at the same depth and every DNODE but
//! the top one holds at least one real entry. Each DNODE records its parent
//! (`up`): the parent DNODE, or the directory's F-node for the top one.

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::layout::*;
use crate::{BlockDevice, Error, Result, Volume};

/// Deepest directory tree accepted (guards against loops).
const MAX_DEPTH: u32 = 32;

#[derive(Debug, Clone)]
pub(crate) struct Dirent {
    /// DIRENT flags without `de::DOWN` (derived from `down`).
    pub flags: u8,
    pub attr: u8,
    pub fnode: u32,
    pub mtime: u32,
    pub size: u32,
    pub atime: u32,
    pub ctime: u32,
    pub ea_size: u32,
    pub acls: u8,
    pub code_page: u8,
    pub name: Vec<u8>,
    /// Bytes stored after the name (other than alignment), kept as is.
    pub extra: Vec<u8>,
    pub down: Option<u32>,
}

impl Dirent {
    pub fn end(down: Option<u32>) -> Dirent {
        Dirent {
            flags: de::END,
            attr: 0,
            fnode: 0,
            mtime: 0,
            size: 0,
            atime: 0,
            ctime: 0,
            ea_size: 0,
            acls: 0,
            code_page: 0,
            name: vec![0xff],
            extra: Vec::new(),
            down,
        }
    }

    pub fn is_end(&self) -> bool {
        self.flags & de::END != 0
    }

    fn body_len(&self) -> usize {
        align4(align4(de::NAME + self.name.len()) + self.extra.len())
    }

    pub fn len(&self) -> usize {
        self.body_len() + if self.down.is_some() { 4 } else { 0 }
    }

    /// Parse the entry at the start of `b`; returns it and its length.
    fn parse(b: &[u8]) -> Result<(Dirent, usize)> {
        if b.len() < de::NAME + 1 {
            return Err(Error::Corrupt);
        }
        let len = rd16(b, de::LEN) as usize;
        let flags = b[de::FLAGS];
        let nlen = b[de::NAME_LEN] as usize;
        let down_bytes = if flags & de::DOWN != 0 { 4 } else { 0 };
        let name_end = de::NAME + nlen;
        if len < align4(name_end) + down_bytes || len > b.len() || !len.is_multiple_of(4) || nlen == 0 {
            return Err(Error::Corrupt);
        }
        let down = (down_bytes != 0).then(|| rd32(b, len - 4));
        let d = Dirent {
            flags: flags & !de::DOWN,
            attr: b[de::ATTR],
            fnode: rd32(b, de::FNODE),
            mtime: rd32(b, de::MTIME),
            size: rd32(b, de::SIZE),
            atime: rd32(b, de::ATIME),
            ctime: rd32(b, de::CTIME),
            ea_size: rd32(b, de::EA_SIZE),
            acls: b[de::ACLS],
            code_page: b[de::CODE_PAGE],
            name: b[de::NAME..name_end].to_vec(),
            extra: b[align4(name_end)..len - down_bytes].to_vec(),
            down,
        };
        Ok((d, len))
    }

    fn write(&self, b: &mut [u8]) {
        let len = self.len();
        b[..len].fill(0);
        wr16(b, de::LEN, len as u16);
        b[de::FLAGS] = self.flags | if self.down.is_some() { de::DOWN } else { 0 };
        b[de::ATTR] = self.attr;
        wr32(b, de::FNODE, self.fnode);
        wr32(b, de::MTIME, self.mtime);
        wr32(b, de::SIZE, self.size);
        wr32(b, de::ATIME, self.atime);
        wr32(b, de::CTIME, self.ctime);
        wr32(b, de::EA_SIZE, self.ea_size);
        b[de::ACLS] = self.acls;
        b[de::CODE_PAGE] = self.code_page;
        b[de::NAME_LEN] = self.name.len() as u8;
        b[de::NAME..de::NAME + self.name.len()].copy_from_slice(&self.name);
        let x = align4(de::NAME + self.name.len());
        b[x..x + self.extra.len()].copy_from_slice(&self.extra);
        if let Some(d) = self.down {
            wr32(b, len - 4, d);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Dnode {
    pub sec: u32,
    pub up: u32,
    /// The flag bytes (root bit and activity counters), kept as read.
    pub flags: [u8; 4],
    pub entries: Vec<Dirent>,
}

impl Dnode {
    /// A new top DNODE of directory F-node `fno` holding `entries` (an END
    /// entry is appended).
    pub fn new_root(sec: u32, fno: u32, mut entries: Vec<Dirent>) -> Dnode {
        entries.push(Dirent::end(None));
        Dnode { sec, up: fno, flags: [dn::ROOT, 0, 0, 0], entries }
    }

    pub fn is_root(&self) -> bool {
        self.flags[0] & dn::ROOT != 0
    }

    fn set_root(&mut self, r: bool) {
        if r {
            self.flags[0] |= dn::ROOT;
        } else {
            self.flags[0] &= !dn::ROOT;
        }
    }

    pub fn leaf(&self) -> bool {
        self.entries.last().is_some_and(|e| e.down.is_none())
    }

    /// Number of real entries (without END).
    pub fn keys(&self) -> usize {
        self.entries.len() - 1
    }

    fn used(&self) -> usize {
        dn::ENTRIES + self.entries.iter().map(Dirent::len).sum::<usize>()
    }

    fn fits(&self) -> bool {
        self.used() <= DNODE_BYTES
    }

    fn end_mut(&mut self) -> &mut Dirent {
        self.entries.last_mut().unwrap()
    }

    fn parse(sec: u32, b: &[u8]) -> Result<Dnode> {
        if rd32(b, 0) != DNODE_MAGIC || rd32(b, dn::SELF) != sec {
            return Err(Error::Corrupt);
        }
        let first_free = (rd32(b, dn::FIRST_FREE) as usize).min(DNODE_BYTES);
        let mut entries = Vec::new();
        let mut o = dn::ENTRIES;
        loop {
            if o >= first_free {
                return Err(Error::Corrupt);
            }
            let (d, len) = Dirent::parse(&b[o..first_free])?;
            o += len;
            let end = d.is_end();
            entries.push(d);
            if end {
                break;
            }
        }
        let n = entries.iter().filter(|e| e.down.is_some()).count();
        if n != 0 && n != entries.len() {
            return Err(Error::Corrupt);
        }
        let mut flags = [0u8; 4];
        flags.copy_from_slice(&b[dn::FLAGS..dn::FLAGS + 4]);
        Ok(Dnode { sec, up: rd32(b, dn::UP), flags, entries })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut b = vec![0u8; DNODE_BYTES];
        wr32(&mut b, 0, DNODE_MAGIC);
        b[dn::FLAGS..dn::FLAGS + 4].copy_from_slice(&self.flags);
        wr32(&mut b, dn::UP, self.up);
        wr32(&mut b, dn::SELF, self.sec);
        let mut o = dn::ENTRIES;
        for e in &self.entries {
            e.write(&mut b[o..]);
            o += e.len();
        }
        wr32(&mut b, dn::FIRST_FREE, o as u32);
        b
    }

    /// Index of the first entry not sorting before `name` (END sorts last).
    fn position(&self, case: &crate::CaseMap, name: &[u8]) -> (usize, bool) {
        for (i, e) in self.entries.iter().enumerate() {
            if e.is_end() {
                return (i, false);
            }
            match case.cmp(name, &e.name) {
                Ordering::Less => return (i, false),
                Ordering::Equal => return (i, true),
                Ordering::Greater => {}
            }
        }
        (self.entries.len() - 1, false)
    }
}

/// Point a directory F-node's allocation entry at its top DNODE.
pub(crate) fn set_dir_root_in(f: &mut [u8], dnode: u32) {
    let h = fnode::BTREE;
    crate::btree::init_header(f, h, bt::FNODE_LEAVES as u8 - 1, 0);
    f[h + bt::USED] = 1;
    wr16(f, h + bt::FIRST_FREE, (bt::ENTRIES + bt::LEAF_SIZE) as u16);
    let o = h + bt::ENTRIES;
    wr32(f, o, u32::MAX);
    wr32(f, o + 4, 0);
    wr32(f, o + 8, dnode);
}

impl<D: BlockDevice> Volume<D> {
    pub(crate) fn read_dnode(&self, sec: u32) -> Result<Dnode> {
        let mut b = vec![0u8; DNODE_BYTES];
        self.read_secs(sec, &mut b)?;
        Dnode::parse(sec, &b)
    }

    pub(crate) fn write_dnode(&self, d: &Dnode) -> Result<()> {
        if !d.fits() {
            return Err(Error::Corrupt);
        }
        self.write_secs(d.sec, &d.serialize())
    }

    /// Top DNODE of directory F-node `dir`.
    pub(crate) fn dir_root(&self, dir: u32) -> Result<u32> {
        let f = self.read_fnode(dir)?;
        if f[fnode::FLAGS] & fnode::DIR == 0 {
            return Err(Error::NotADirectory);
        }
        if f[fnode::BTREE + bt::USED] == 0 {
            return Err(Error::Corrupt);
        }
        Ok(rd32(&f, fnode::BTREE + bt::ENTRIES + 8))
    }

    fn set_dir_root(&mut self, dir: u32, dnode: u32) -> Result<()> {
        let mut f = self.read_fnode(dir)?;
        set_dir_root_in(&mut f, dnode);
        self.write_secs(dir, &f)
    }

    /// Find `name`: the DNODE holding it and the entry's index.
    pub(crate) fn find_dirent(&self, top: u32, name: &[u8]) -> Result<Option<(Dnode, usize)>> {
        let mut sec = top;
        for _ in 0..MAX_DEPTH {
            let d = self.read_dnode(sec)?;
            let (i, found) = d.position(&self.case, name);
            if found {
                return Ok(Some((d, i)));
            }
            match d.entries[i].down {
                Some(c) => sec = c,
                None => return Ok(None),
            }
        }
        Err(Error::Corrupt)
    }

    /// Find the entry for F-node `fno` by scanning the whole directory.
    pub(crate) fn find_by_fnode(&self, sec: u32, fno: u32, depth: u32) -> Result<Option<(Dnode, usize)>> {
        if depth > MAX_DEPTH {
            return Err(Error::Corrupt);
        }
        let d = self.read_dnode(sec)?;
        if let Some(i) = d.entries.iter().position(|e| !e.is_end() && e.fnode == fno && e.flags & de::SPECIAL == 0) {
            return Ok(Some((d, i)));
        }
        for e in &d.entries {
            if let Some(c) = e.down
                && let Some(r) = self.find_by_fnode(c, fno, depth + 1)? {
                    return Ok(Some(r));
                }
        }
        Ok(None)
    }

    /// All entries below DNODE `sec`, in order (down pointers cleared).
    pub(crate) fn walk_dir(&self, sec: u32, out: &mut Vec<Dirent>, depth: u32) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::Corrupt);
        }
        for mut e in self.read_dnode(sec)?.entries {
            if let Some(c) = e.down.take() {
                self.walk_dir(c, out, depth + 1)?;
            }
            if !e.is_end() {
                out.push(e);
            }
        }
        Ok(())
    }

    fn set_up(&self, child: u32, parent: u32) -> Result<()> {
        let mut c = self.read_dnode(child)?;
        if c.up != parent {
            c.up = parent;
            self.write_dnode(&c)?;
        }
        Ok(())
    }

    /// The parent DNODE of non-top `d` and the index of the entry pointing
    /// down to `d`.
    fn parent_of(&self, d: &Dnode) -> Result<(Dnode, usize)> {
        let p = self.read_dnode(d.up)?;
        let j = p.entries.iter().position(|e| e.down == Some(d.sec)).ok_or(Error::Corrupt)?;
        Ok((p, j))
    }

    /// Insert a new entry into directory F-node `dir`.
    pub(crate) fn insert_dirent(&mut self, dir: u32, e: Dirent) -> Result<()> {
        let mut sec = self.dir_root(dir)?;
        for _ in 0..MAX_DEPTH {
            let mut d = self.read_dnode(sec)?;
            let (i, found) = d.position(&self.case, &e.name);
            if found {
                return Err(Error::Exists);
            }
            match d.entries[i].down {
                Some(c) => sec = c,
                None => {
                    d.entries.insert(i, e);
                    return self.store(d);
                }
            }
        }
        Err(Error::Corrupt)
    }

    /// Write a DNODE, splitting it (and its ancestors) while it overflows.
    fn store(&mut self, mut node: Dnode) -> Result<()> {
        loop {
            if node.fits() {
                return self.write_dnode(&node);
            }
            let m = split_point(&node);
            let right = node.entries.split_off(m + 1);
            let mut mid = node.entries.pop().unwrap();
            let mut left_entries = core::mem::replace(&mut node.entries, right);
            let left_sec = self.alloc_dnode(node.sec)?;
            left_entries.push(Dirent::end(mid.down));
            let mut left = Dnode { sec: left_sec, up: 0, flags: [0; 4], entries: left_entries };
            mid.down = Some(left_sec);
            for c in left.entries.iter().filter_map(|e| e.down) {
                self.set_up(c, left_sec)?;
            }
            if node.is_root() {
                let fno = node.up;
                let top = self.alloc_dnode(node.sec)?;
                node.set_root(false);
                node.up = top;
                left.up = top;
                let root = Dnode {
                    sec: top,
                    up: fno,
                    flags: [dn::ROOT, 0, 0, 0],
                    entries: vec![mid, Dirent::end(Some(node.sec))],
                };
                self.write_dnode(&left)?;
                self.write_dnode(&node)?;
                self.write_dnode(&root)?;
                return self.set_dir_root(fno, top);
            }
            let (mut parent, j) = self.parent_of(&node)?;
            left.up = parent.sec;
            self.write_dnode(&left)?;
            self.write_dnode(&node)?;
            parent.entries.insert(j, mid);
            node = parent;
        }
    }

    /// Remove `name` from directory F-node `dir`; returns its entry.
    pub(crate) fn remove_dirent(&mut self, dir: u32, name: &[u8]) -> Result<Dirent> {
        let top = self.dir_root(dir)?;
        let (mut n, i) = self.find_dirent(top, name)?.ok_or(Error::NotFound)?;
        let mut removed = n.entries[i].clone();
        match removed.down {
            None => {
                n.entries.remove(i);
                self.write_dnode(&n)?;
                self.fix_underflow(n.sec)?;
            }
            Some(down) => {
                // Replace with the predecessor from the rightmost leaf of
                // the left subtree.
                let mut leaf = self.read_dnode(down)?;
                for _ in 0..MAX_DEPTH {
                    if leaf.leaf() {
                        break;
                    }
                    leaf = self.read_dnode(leaf.entries.last().unwrap().down.unwrap())?;
                }
                if !leaf.leaf() || leaf.keys() == 0 {
                    return Err(Error::Corrupt);
                }
                let mut pred = leaf.entries.remove(leaf.keys() - 1);
                self.write_dnode(&leaf)?;
                pred.down = Some(down);
                n.entries[i] = pred;
                self.store(n)?;
                self.fix_underflow(leaf.sec)?;
            }
        }
        removed.down = None;
        Ok(removed)
    }

    /// Restore the invariants after DNODE `sec` lost an entry: a DNODE
    /// left without real entries is merged into a neighbour, and a top
    /// DNODE with only a down pointer is replaced by its child.
    fn fix_underflow(&mut self, mut sec: u32) -> Result<()> {
        for _ in 0..MAX_DEPTH {
            let d = self.read_dnode(sec)?;
            if d.keys() > 0 {
                return Ok(());
            }
            let child = d.entries[0].down;
            if d.is_root() {
                if let Some(c) = child {
                    let mut c = self.read_dnode(c)?;
                    c.set_root(true);
                    c.up = d.up;
                    self.write_dnode(&c)?;
                    self.set_dir_root(d.up, c.sec)?;
                    self.free_dnode(d.sec)?;
                }
                return Ok(());
            }
            let (mut p, j) = self.parent_of(&d)?;
            if j + 1 < p.entries.len() {
                // Move the separator down into the right sibling.
                let mut x = p.entries.remove(j);
                let r_sec = p.entries[j].down.ok_or(Error::Corrupt)?;
                x.down = child;
                let mut r = self.read_dnode(r_sec)?;
                r.entries.insert(0, x);
                if let Some(c) = child {
                    self.set_up(c, r_sec)?;
                }
                self.free_dnode(d.sec)?;
                self.write_dnode(&p)?;
                self.store(r)?;
            } else {
                // The END entry pointed here: append the last separator
                // to the left sibling instead.
                if j == 0 {
                    return Err(Error::Corrupt);
                }
                let mut w = p.entries.remove(j - 1);
                let l_sec = w.down.ok_or(Error::Corrupt)?;
                let mut l = self.read_dnode(l_sec)?;
                w.down = l.end_mut().down;
                let at = l.entries.len() - 1;
                l.entries.insert(at, w);
                l.end_mut().down = child;
                if let Some(c) = child {
                    self.set_up(c, l_sec)?;
                }
                p.entries[j - 1].down = Some(l_sec);
                self.free_dnode(d.sec)?;
                self.write_dnode(&p)?;
                self.store(l)?;
            }
            sec = p.sec;
        }
        Err(Error::Corrupt)
    }
}

/// Where to split an overflowing DNODE: the index of the entry that moves
/// up, chosen to balance the bytes on both sides while leaving each side
/// at least one real entry.
fn split_point(d: &Dnode) -> usize {
    let keys = d.keys();
    let total: usize = d.entries.iter().map(Dirent::len).sum();
    let mut acc = 0;
    let mut m = 0;
    for (i, e) in d.entries.iter().enumerate().take(keys) {
        acc += e.len();
        if acc >= total / 2 {
            m = i;
            break;
        }
    }
    m.clamp(1, keys.saturating_sub(2).max(1))
}
