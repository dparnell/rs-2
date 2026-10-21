//! dtrees: directories as B+-trees of names.
//!
//! The root lives in the inode (8 slots of 32 bytes after a header);
//! further pages are 4 KB (127 slots, 4 of them the sorted index table
//! `stbl`). An entry takes a head slot plus continuation slots of 15
//! characters: a leaf entry holds the inode number and 13 characters
//! (11 and a directory index number on volumes with index tables), an
//! internal entry the child page's extent and 11 characters of the
//! router key: the shortest prefix of the right page's first name that
//! sorts after the left page's last one, upper-cased on OS/2 volumes.
//! The first entry of the root and of every leftmost internal page has
//! an empty key. Pages at one level are linked left to right.
//!
//! rs2 works on whole nodes: a node is parsed into its entries, changed
//! and packed again (slots in order, the rest on the free list). Pages
//! that become empty are freed; nothing is merged otherwise (as in JFS).

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::inode::Inode;
use crate::layout::*;
use crate::name::{compare, prefix_key};
use crate::{BlockDevice, Error, Result, Volume};

#[derive(Clone, Debug)]
pub(crate) struct DEnt {
    pub name: Vec<u16>,
    /// Inode (leaf) or child page address (internal).
    pub ino: u32,
    pub child: Pxd,
    /// Directory index (index-table volumes).
    pub index: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    /// The page; addr 0 for the root in the inode.
    pub pxd: Pxd,
    pub leaf: bool,
    pub next: u64,
    pub prev: u64,
    pub ents: Vec<DEnt>,
    /// Page geometry: slots and the index table's first slot.
    pub maxslot: usize,
    pub stblindex: usize,
}

impl Node {
    fn is_root(&self) -> bool {
        self.pxd.addr == 0
    }

    /// Slots available for entries.
    fn capacity(&self) -> usize {
        if self.is_root() { DTROOTMAXSLOT - 1 } else { self.maxslot - 1 - self.maxslot.div_ceil(DTSLOTSIZE) }
    }
}

const MAX_DEPTH: usize = 16;

impl<D: BlockDevice> Volume<D> {
    fn head_chars(&self, leaf: bool) -> usize {
        if !leaf {
            DTIHDRDATALEN
        } else if self.dir_index {
            DTLHDRDATALEN
        } else {
            DTLHDRDATALEN_LEGACY
        }
    }

    fn slots_for(&self, leaf: bool, len: usize) -> usize {
        1 + len.saturating_sub(self.head_chars(leaf)).div_ceil(DTSLOTDATALEN)
    }

    fn node_slots(&self, n: &Node) -> usize {
        n.ents.iter().map(|e| self.slots_for(n.leaf, e.name.len())).sum()
    }

    fn fits(&self, n: &Node) -> bool {
        self.node_slots(n) <= n.capacity()
    }

    /// An empty directory root.
    pub(crate) fn dt_init_root(&mut self, i: &mut Inode, parent: u32) {
        let r = di::ROOT;
        i.b[r..r + DTROOTMAXSLOT * DTSLOTSIZE].fill(0);
        i.b[r + 16] = bt::DXD_INDEX | bt::ROOT | bt::LEAF;
        i.b[r + 17] = 0;
        i.b[r + 18] = 8;
        i.b[r + 19] = 1;
        wr32(&mut i.b, r + 20, parent);
        for s in 1..DTROOTMAXSLOT {
            i.b[r + s * DTSLOTSIZE] = if s + 1 < DTROOTMAXSLOT { (s + 1) as u8 } else { 0xff };
            i.b[r + s * DTSLOTSIZE + 1] = 1;
        }
        i.set_size(IDATASIZE);
        if self.dir_index {
            wr32(&mut i.b, di::NEXT_INDEX, 2);
            i.set_size(1);
        }
    }

    pub(crate) fn dt_set_parent(&mut self, i: &mut Inode, parent: u32) -> Result<()> {
        wr32(&mut i.b, di::ROOT + 20, parent);
        Ok(())
    }

    // ---- Parsing and packing ----

    fn parse(&self, b: &[u8], pxd: Pxd) -> Result<Node> {
        let root = pxd.addr == 0;
        let flag = b[16];
        let leaf = flag & bt::LEAF != 0;
        if flag & (bt::LEAF | bt::INTERNAL) == 0 {
            return Err(corrupt!());
        }
        let count = b[17] as usize;
        let (maxslot, stbl) = if root {
            (DTROOTMAXSLOT, 24)
        } else {
            let m = b[20] as usize;
            let si = b[21] as usize;
            if m == 0 || m * DTSLOTSIZE > b.len() || si == 0 || si >= m {
                return Err(corrupt!());
            }
            (m, si * DTSLOTSIZE)
        };
        if count > maxslot || stbl + count > b.len() {
            return Err(corrupt!());
        }
        let head = self.head_chars(leaf);
        let mut ents = Vec::with_capacity(count);
        for k in 0..count {
            let s = b[stbl + k] as usize;
            if s == 0 || s >= maxslot {
                return Err(corrupt!());
            }
            let o = s * DTSLOTSIZE;
            let (namlen, mut next, name_at) =
                if leaf { (b[o + 5] as usize, b[o + 4] as i8, o + 6) } else { (b[o + 9] as usize, b[o + 8] as i8, o + 10) };
            let mut name = Vec::with_capacity(namlen);
            for c in 0..namlen.min(head) {
                name.push(rd16(b, name_at + 2 * c));
            }
            let mut guard = 0;
            while name.len() < namlen {
                if next < 0 || next as usize >= maxslot || guard > maxslot {
                    return Err(corrupt!());
                }
                let t = next as usize * DTSLOTSIZE;
                // (The slot's count byte is a segment count, 1.)
                let cnt = DTSLOTDATALEN.min(namlen - name.len());
                for c in 0..cnt {
                    name.push(rd16(b, t + 2 + 2 * c));
                }
                next = b[t] as i8;
                guard += 1;
            }
            ents.push(DEnt {
                name,
                ino: if leaf { rd32(b, o) } else { 0 },
                child: if leaf { Pxd::default() } else { Pxd::read(b, o) },
                index: if leaf && self.dir_index { rd32(b, o + 28) } else { 0 },
            });
        }
        let (next, prev) = if root { (0, 0) } else { (rd64(b, 0), rd64(b, 8)) };
        Ok(Node { pxd, leaf, next, prev, ents, maxslot, stblindex: if root { 0 } else { stbl / DTSLOTSIZE } })
    }

    /// Pack a node into `b` (the 288-byte root area, or a page). Returns
    /// the head slot of each entry.
    fn pack(&self, n: &Node, b: &mut [u8]) -> Vec<usize> {
        let root = n.is_root();
        let head = self.head_chars(n.leaf);
        let idotdot = rd32(b, 20);
        if root {
            b[16..].fill(0);
            wr32(b, 20, idotdot);
        } else {
            b.fill(0);
        }
        // Usable slots in order.
        let stbl_slots = if root { 0 } else { n.maxslot.div_ceil(DTSLOTSIZE) };
        let usable: Vec<usize> =
            (1..n.maxslot).filter(|&s| root || s < n.stblindex || s >= n.stblindex + stbl_slots).collect();
        let mut u = 0;
        let mut heads = Vec::with_capacity(n.ents.len());
        let stbl = if root { 24 } else { n.stblindex * DTSLOTSIZE };
        for (k, e) in n.ents.iter().enumerate() {
            let s = usable[u];
            u += 1;
            heads.push(s);
            b[stbl + k] = s as u8;
            let o = s * DTSLOTSIZE;
            let first = e.name.len().min(head);
            let name_at = if n.leaf { o + 6 } else { o + 10 };
            for (c, &ch) in e.name[..first].iter().enumerate() {
                wr16(b, name_at + 2 * c, ch);
            }
            if n.leaf {
                wr32(b, o, e.ino);
                b[o + 5] = e.name.len() as u8;
                if self.dir_index {
                    wr32(b, o + 28, e.index);
                }
            } else {
                e.child.write(b, o);
                b[o + 9] = e.name.len() as u8;
            }
            let link = if n.leaf { o + 4 } else { o + 8 };
            let mut prev_link = link;
            let mut rest = &e.name[first..];
            while !rest.is_empty() {
                let t = usable[u];
                u += 1;
                b[prev_link] = t as u8;
                let to = t * DTSLOTSIZE;
                let cnt = rest.len().min(DTSLOTDATALEN);
                b[to + 1] = 1;
                for (c, &ch) in rest[..cnt].iter().enumerate() {
                    wr16(b, to + 2 + 2 * c, ch);
                }
                prev_link = to;
                rest = &rest[cnt..];
            }
            b[prev_link] = 0xff;
        }
        // The free list.
        let free = &usable[u..];
        for (k, &s) in free.iter().enumerate() {
            let o = s * DTSLOTSIZE;
            b[o] = if k + 1 < free.len() { free[k + 1] as u8 } else { 0xff };
            b[o + 1] = 1;
        }
        b[16] = if root { bt::DXD_INDEX | bt::ROOT } else { 0 } | if n.leaf { bt::LEAF } else { bt::INTERNAL };
        b[17] = n.ents.len() as u8;
        b[18] = free.len() as u8;
        b[19] = free.first().map_or(0xff, |&s| s as u8);
        if !root {
            wr64(b, 0, n.next);
            wr64(b, 8, n.prev);
            b[20] = n.maxslot as u8;
            b[21] = n.stblindex as u8;
            n.pxd.write(b, 24);
        }
        heads
    }

    fn read_node(&mut self, dir: &Inode, pxd: Pxd) -> Result<Node> {
        if pxd.addr == 0 {
            let r = dir.b[di::ROOT..di::ROOT + DTROOTMAXSLOT * DTSLOTSIZE].to_vec();
            return self.parse(&r, pxd);
        }
        if pxd.len == 0 || pxd.len as usize > PSIZE >> self.l2bsize || pxd.end() > self.bmap.mapsize {
            return Err(corrupt!());
        }
        let b = self.read_blocks(pxd.addr, pxd.len)?;
        let n = self.parse(&b, pxd)?;
        if n.is_root() {
            return Err(corrupt!());
        }
        Ok(n)
    }

    /// Write a node (the root into the inode, which the caller writes).
    fn write_node(&mut self, dir: &mut Inode, n: &Node) -> Result<()> {
        let heads = if n.is_root() {
            let mut r = dir.b[di::ROOT..di::ROOT + DTROOTMAXSLOT * DTSLOTSIZE].to_vec();
            let h = self.pack(n, &mut r);
            dir.b[di::ROOT + 16..di::ROOT + DTROOTMAXSLOT * DTSLOTSIZE].copy_from_slice(&r[16..]);
            h
        } else {
            let mut b = vec![0u8; (n.pxd.len as usize) << self.l2bsize];
            let h = self.pack(n, &mut b);
            self.write_blocks(n.pxd.addr, &b)?;
            h
        };
        if self.dir_index && n.leaf {
            for (e, s) in n.ents.iter().zip(heads) {
                self.index_set(dir, e.index, n.pxd.addr, s)?;
            }
        }
        Ok(())
    }

    // ---- Search ----

    /// Descend to the leaf where `key` is or would go: the internal nodes
    /// passed (with the entry taken), the leaf and the position.
    fn descend(&mut self, dir: &Inode, key: &[u16]) -> Result<(Vec<(Node, usize)>, Node, core::result::Result<usize, usize>)> {
        let ci = self.ci;
        let mut path = Vec::new();
        let mut node = self.read_node(dir, Pxd::default())?;
        loop {
            if node.leaf {
                let pos = node.ents.binary_search_by(|e| compare(&e.name, key, ci));
                return Ok((path, node, pos));
            }
            if node.ents.is_empty() || path.len() >= MAX_DEPTH {
                return Err(corrupt!());
            }
            let k = node.ents.partition_point(|e| compare(&e.name, key, ci) != Ordering::Greater);
            let k = k.saturating_sub(1);
            let child = node.ents[k].child;
            path.push((node, k));
            node = self.read_node(dir, child)?;
        }
    }

    /// Find a name: the stored name and the inode.
    pub(crate) fn dt_lookup(&mut self, dir: &Inode, name: &[u16]) -> Result<Option<(Vec<u16>, u32)>> {
        let (_, leaf, pos) = self.descend(dir, name)?;
        if let Ok(k) = pos {
            return Ok(Some((leaf.ents[k].name.clone(), leaf.ents[k].ino)));
        }
        if !self.ci {
            // Case-sensitive volume: OS/2 still finds names in any case.
            for (n, ino) in self.dt_list(dir)? {
                if compare(&n, name, true) == Ordering::Equal {
                    return Ok(Some((n, ino)));
                }
            }
        }
        Ok(None)
    }

    /// All (name, inode) pairs in order.
    pub(crate) fn dt_list(&mut self, dir: &Inode) -> Result<Vec<(Vec<u16>, u32)>> {
        let mut out = Vec::new();
        let root = self.read_node(dir, Pxd::default())?;
        self.walk(dir, &root, 0, &mut |_, n| {
            if n.leaf {
                for e in &n.ents {
                    out.push((e.name.clone(), e.ino));
                }
            }
        })?;
        Ok(out)
    }

    /// Visit every node depth first (pages after their parent).
    pub(crate) fn walk(&mut self, dir: &Inode, n: &Node, depth: usize, f: &mut dyn FnMut(usize, &Node)) -> Result<()> {
        f(depth, n);
        if n.leaf {
            return Ok(());
        }
        if depth >= MAX_DEPTH {
            return Err(corrupt!());
        }
        for e in &n.ents {
            let c = self.read_node(dir, e.child)?;
            self.walk(dir, &c, depth + 1, f)?;
        }
        Ok(())
    }

    /// The tree's pages.
    pub(crate) fn dt_pages(&mut self, dir: &Inode) -> Result<Vec<Pxd>> {
        let mut v = Vec::new();
        let root = self.read_node(dir, Pxd::default())?;
        self.walk(dir, &root, 0, &mut |_, n| {
            if !n.is_root() {
                v.push(n.pxd);
            }
        })?;
        Ok(v)
    }

    pub(crate) fn dt_root(&mut self, dir: &Inode) -> Result<Node> {
        self.read_node(dir, Pxd::default())
    }

    // ---- Insertion ----

    /// Add `name` -> `ino` to directory `dir` (written by the caller).
    pub(crate) fn dt_insert(&mut self, dir: &mut Inode, name: &[u16], ino: u32) -> Result<()> {
        let (path, mut leaf, pos) = self.descend(dir, name)?;
        let Err(k) = pos else { return Err(Error::Exists) };
        let index = if self.dir_index { self.index_add(dir)? } else { 0 };
        leaf.ents.insert(k, DEnt { name: name.to_vec(), ino, child: Pxd::default(), index });
        self.settle(dir, path, leaf, k)
    }

    /// Write `node` back, splitting it (and its parents) while it does
    /// not fit. `at` is where the newest entry went.
    fn settle(&mut self, dir: &mut Inode, mut path: Vec<(Node, usize)>, mut node: Node, mut at: usize) -> Result<()> {
        loop {
            if self.fits(&node) {
                return self.write_node(dir, &node);
            }
            if node.is_root() {
                return self.split_root(dir, node);
            }
            let (left, right, key) = self.split(dir, node, at)?;
            let Some((mut parent, k)) = path.pop() else { return Err(corrupt!()) };
            self.write_node(dir, &left)?;
            self.write_node(dir, &right)?;
            parent.ents.insert(k + 1, DEnt { name: key, ino: 0, child: right.pxd, index: 0 });
            at = k + 1;
            node = parent;
        }
    }

    fn new_page(&mut self, dir: &mut Inode) -> Result<Pxd> {
        let nb = (PSIZE >> self.l2bsize) as u32;
        let goal = dir.ixpxd().addr;
        let p = self.alloc_meta(goal, nb)?;
        dir.set_nblocks(dir.nblocks() + nb as u64);
        if !self.dir_index {
            let s = dir.size();
            dir.set_size(if s <= IDATASIZE { PSIZE as u64 } else { s + PSIZE as u64 });
        }
        Ok(p)
    }

    fn drop_page(&mut self, dir: &mut Inode, p: Pxd) -> Result<()> {
        self.free_meta(p)?;
        dir.set_nblocks(dir.nblocks().saturating_sub(p.len as u64));
        if !self.dir_index {
            let s = dir.size().saturating_sub(PSIZE as u64);
            dir.set_size(if s < PSIZE as u64 { IDATASIZE } else { s });
        }
        Ok(())
    }

    /// The root is full: move its entries to a new page under it.
    fn split_root(&mut self, dir: &mut Inode, root: Node) -> Result<()> {
        let p = self.new_page(dir)?;
        let mut child = Node {
            pxd: p,
            leaf: root.leaf,
            next: 0,
            prev: 0,
            ents: root.ents,
            maxslot: (p.len as usize) << self.l2bsize >> 5,
            stblindex: 1,
        };
        if !self.fits(&child) {
            // Only possible with a very long new name in a full root:
            // split the new page right away.
            let at = child.ents.len() - 1;
            let (l, r, key) = self.split(dir, child, at)?;
            self.write_node(dir, &l)?;
            self.write_node(dir, &r)?;
            let newroot = Node {
                pxd: Pxd::default(),
                leaf: false,
                next: 0,
                prev: 0,
                ents: vec![
                    DEnt { name: Vec::new(), ino: 0, child: l.pxd, index: 0 },
                    DEnt { name: key, ino: 0, child: r.pxd, index: 0 },
                ],
                maxslot: DTROOTMAXSLOT,
                stblindex: 0,
            };
            return self.write_node(dir, &newroot);
        }
        child.maxslot = child.maxslot.min(128);
        self.write_node(dir, &child)?;
        let newroot = Node {
            pxd: Pxd::default(),
            leaf: false,
            next: 0,
            prev: 0,
            ents: vec![DEnt { name: Vec::new(), ino: 0, child: p, index: 0 }],
            maxslot: DTROOTMAXSLOT,
            stblindex: 0,
        };
        self.write_node(dir, &newroot)
    }

    /// Split an overfull page into itself and a new right sibling;
    /// returns both and the router key for the right one.
    fn split(&mut self, dir: &mut Inode, mut node: Node, at: usize) -> Result<(Node, Node, Vec<u16>)> {
        let n = node.ents.len();
        // Appending to the rightmost page: start the new page with just
        // the new entry (directories filled in order stay packed).
        let cut = if at == n - 1 && node.next == 0 && n > 1 {
            n - 1
        } else {
            let total = self.node_slots(&node);
            let mut acc = 0;
            let mut c = 0;
            while c < n - 1 {
                acc += self.slots_for(node.leaf, node.ents[c].name.len());
                c += 1;
                if acc * 2 >= total {
                    break;
                }
            }
            c.max(1)
        };
        let right_ents = node.ents.split_off(cut);
        let key = if node.leaf {
            prefix_key(&node.ents[cut - 1].name, &right_ents[0].name, self.ci)
        } else {
            right_ents[0].name.clone()
        };
        let p = self.new_page(dir)?;
        let right = Node {
            pxd: p,
            leaf: node.leaf,
            next: node.next,
            prev: node.pxd.addr,
            ents: right_ents,
            maxslot: ((p.len as usize) << self.l2bsize >> 5).min(128),
            stblindex: 1,
        };
        if node.next != 0 {
            self.relink(dir, node.next, None, Some(p.addr))?;
        }
        node.next = p.addr;
        if !self.fits(&node) || !self.fits(&right) {
            // A small page (from another system's small blocks) that one
            // split cannot settle: give up rather than loop.
            return Err(Error::DiskFull);
        }
        Ok((node, right, key))
    }

    /// Change the sibling links of the page at `addr`.
    fn relink(&mut self, dir: &mut Inode, addr: u64, next: Option<u64>, prev: Option<u64>) -> Result<()> {
        let nb = (PSIZE >> self.l2bsize) as u32;
        let mut b = self.read_blocks(addr, nb)?;
        let len = Pxd::read(&b, 24).len.clamp(1, nb);
        if len != nb {
            b = self.read_blocks(addr, len)?;
        }
        if let Some(n) = next {
            wr64(&mut b, 0, n);
        }
        if let Some(p) = prev {
            wr64(&mut b, 8, p);
        }
        let _ = dir;
        self.write_blocks(addr, &b)
    }

    // ---- Deletion ----

    /// Remove `name` from directory `dir` (written by the caller).
    pub(crate) fn dt_delete(&mut self, dir: &mut Inode, name: &[u16]) -> Result<()> {
        let (mut path, mut leaf, pos) = self.descend(dir, name)?;
        let k = match pos {
            Ok(k) => k,
            Err(_) => {
                // Case-sensitive volume, name given in another case.
                let Some((stored, _)) = self.dt_lookup(dir, name)? else { return Err(Error::NotFound) };
                let (p, l, pos) = self.descend(dir, &stored)?;
                path = p;
                leaf = l;
                pos.map_err(|_| Error::NotFound)?
            }
        };
        let e = leaf.ents.remove(k);
        if self.dir_index {
            self.index_free(dir, e.index)?;
        }
        let mut node = leaf;
        loop {
            if !node.ents.is_empty() || node.is_root() {
                if node.is_root() && node.ents.is_empty() && !node.leaf {
                    node.leaf = true;
                }
                return self.write_node(dir, &node);
            }
            // An empty page: unlink and free it, drop its router entry.
            if node.prev != 0 {
                self.relink(dir, node.prev, Some(node.next), None)?;
            }
            if node.next != 0 {
                self.relink(dir, node.next, None, Some(node.prev))?;
            }
            self.drop_page(dir, node.pxd)?;
            let Some((mut parent, k)) = path.pop() else { return Err(corrupt!()) };
            parent.ents.remove(k);
            if k == 0 && !parent.ents.is_empty() && (parent.is_root() || parent.prev == 0) {
                parent.ents[0].name.clear();
            }
            node = parent;
        }
    }
}
