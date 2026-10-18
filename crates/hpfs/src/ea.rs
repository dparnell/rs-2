//! Extended attributes and freeing of EA/ACL storage.
//!
//! An F-node's EAs are a packed FEA list: part inside the F-node (from
//! `fnode::EA_OFFSET`, `EA_INT_SIZE` bytes) and part outside (`EA_EXT_SIZE`
//! bytes at `EA_SECTOR`: a contiguous run, or an ALSEC tree when
//! `EA_IN_ANODE` is set). A large value may itself be stored outside the
//! list ("indirect": the value field holds its length and sector).
//!
//! rs2 writes the whole list in the F-node when it fits, otherwise all of
//! it outside: in one run of sectors if one is free, else in runs described
//! by an ALSEC tree. Values are always stored in the list.

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{btree, BlockDevice, Entry, Error, Extent, Result, Volume};

/// FEA flags: the value is stored elsewhere / in an ALSEC tree.
const EA_INDIRECT: u8 = 0x01;
const EA_IN_ANODE: u8 = 0x02;
/// Largest EA data accepted when reading (OS/2 limits a file's EAs to 64 KB).
const MAX_EA_DATA: u32 = 1 << 20;
/// Largest FEA list written (cbList of the OS/2 1.x FEALIST included).
pub const MAX_FEALIST: usize = 0xffff;
/// FEA flag: the EA is critical.
const FEA_NEEDEA: u8 = 0x80;

/// One extended attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ea {
    /// FEA flags (0x80 = NEEDEA).
    pub flags: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// Size of the FEA2LIST describing `eas` (as DosQueryPathInfo reports it).
pub fn fea2_size(eas: &[Ea]) -> u32 {
    let mut n = 4usize;
    for (i, e) in eas.iter().enumerate() {
        let one = 4 + 1 + 1 + 2 + e.name.len() + 1 + e.value.len();
        n += if i + 1 == eas.len() { one } else { align4(one) };
    }
    n as u32
}

/// An FEA list entry as stored: (flags, name, value field).
struct RawEa<'a> {
    flags: u8,
    name: &'a [u8],
    value: &'a [u8],
}

fn parse(list: &[u8]) -> Result<Vec<RawEa<'_>>> {
    let mut v = Vec::new();
    let mut i = 0;
    while i + 4 <= list.len() {
        let flags = list[i];
        let nl = list[i + 1] as usize;
        let vl = rd16(list, i + 2) as usize;
        let name = i + 4;
        let value = name + nl + 1;
        if value + vl > list.len() {
            return Err(Error::Corrupt);
        }
        v.push(RawEa { flags, name: &list[name..name + nl], value: &list[value..value + vl] });
        i = value + vl;
    }
    Ok(v)
}

impl<D: BlockDevice> Volume<D> {
    /// Read `len` bytes stored at `sec`: a contiguous run, or the data
    /// described by the ALSEC tree rooted at `sec`.
    fn read_stored(&self, sec: u32, anode: bool, len: u32) -> Result<Vec<u8>> {
        if len > MAX_EA_DATA {
            return Err(Error::Corrupt);
        }
        let sectors = len.div_ceil(SECTOR as u32);
        let mut buf = vec![0u8; sectors as usize * SECTOR];
        if anode {
            let a = self.read_sector(sec)?;
            if rd32(&a, 0) != ALSEC_MAGIC {
                return Err(Error::Corrupt);
            }
            let (ext, _) = self.tree_of(&a, alsec::BTREE)?;
            for x in ext {
                for k in 0..x.len {
                    let fs = x.file + k;
                    if fs < sectors {
                        let o = fs as usize * SECTOR;
                        self.read_secs(x.disk + k, &mut buf[o..o + SECTOR])?;
                    }
                }
            }
        } else {
            self.read_secs(sec, &mut buf)?;
        }
        buf.truncate(len as usize);
        Ok(buf)
    }

    /// Free storage written by [`Self::read_stored`]'s layout.
    fn free_stored(&mut self, sec: u32, anode: bool, len: u32) -> Result<()> {
        if anode {
            let a = self.read_sector(sec)?;
            if rd32(&a, 0) != ALSEC_MAGIC {
                return Err(Error::Corrupt);
            }
            let (ext, alsecs) = self.tree_of(&a, alsec::BTREE)?;
            for x in ext {
                self.free_run(x.disk, x.len)?;
            }
            for s in alsecs {
                self.free_run(s, 1)?;
            }
            self.free_run(sec, 1)
        } else {
            self.free_run(sec, len.div_ceil(SECTOR as u32))
        }
    }

    /// The raw FEA list of F-node buffer `f`.
    fn ea_list(&self, f: &[u8]) -> Result<Vec<u8>> {
        let mut list = Vec::new();
        let int = rd16(f, fnode::EA_INT_SIZE) as usize;
        if int > 0 {
            let off = rd16(f, fnode::EA_OFFSET) as usize;
            if off < fnode::EA_AREA || off + int > SECTOR {
                return Err(Error::Corrupt);
            }
            list.extend_from_slice(&f[off..off + int]);
        }
        let ext = rd32(f, fnode::EA_EXT_SIZE);
        if ext > 0 {
            list.extend(self.read_stored(rd32(f, fnode::EA_SECTOR), f[fnode::EA_IN_ANODE] & 1 != 0, ext)?);
        }
        Ok(list)
    }

    /// All extended attributes of a file or directory.
    pub fn read_eas(&self, e: &Entry) -> Result<Vec<Ea>> {
        let f = self.read_fnode(e.fnode)?;
        let list = self.ea_list(&f)?;
        let mut out = Vec::new();
        for r in parse(&list)? {
            let value = if r.flags & EA_INDIRECT != 0 {
                if r.value.len() < 8 {
                    return Err(Error::Corrupt);
                }
                self.read_stored(rd32(r.value, 4), r.flags & EA_IN_ANODE != 0, rd32(r.value, 0))?
            } else {
                r.value.to_vec()
            };
            out.push(Ea { flags: r.flags & !(EA_INDIRECT | EA_IN_ANODE), name: r.name.to_vec(), value });
        }
        Ok(out)
    }

    /// Replace all EAs of a file or directory with `eas` (empty: remove
    /// them), updating the F-node, the DIRENT's EA size and flags.
    pub fn write_eas(&mut self, e: &Entry, eas: &[Ea]) -> Result<()> {
        self.check_writable()?;
        let mut list = Vec::new();
        for x in eas {
            if x.name.len() > 255 || x.value.len() > 0xffff {
                return Err(Error::EaTooLong);
            }
            list.push(x.flags & !(EA_INDIRECT | EA_IN_ANODE));
            list.push(x.name.len() as u8);
            list.extend_from_slice(&(x.value.len() as u16).to_le_bytes());
            list.extend_from_slice(&x.name);
            list.push(0);
            list.extend_from_slice(&x.value);
        }
        if list.len() + 4 > MAX_FEALIST {
            return Err(Error::EaTooLong);
        }
        let fno = e.fnode;
        let mut f = self.read_fnode(fno)?;
        self.free_eas(&f)?;
        let mut off = rd16(&f, fnode::EA_OFFSET) as usize;
        if !(fnode::EA_AREA..=SECTOR).contains(&off) {
            off = fnode::EA_AREA;
        }
        // Internal ACLs follow the internal EAs; keep them.
        let old_int = rd16(&f, fnode::EA_INT_SIZE) as usize;
        let acl_int = rd16(&f, fnode::ACL_INT_SIZE) as usize;
        let acls = f.get(off + old_int..off + old_int + acl_int).map(|a| a.to_vec()).unwrap_or_default();
        f[off..].fill(0);
        wr16(&mut f, fnode::EA_OFFSET, off as u16);
        let (int, ext) = if off + list.len() + acls.len() <= SECTOR {
            f[off..off + list.len()].copy_from_slice(&list);
            (list.len(), 0)
        } else {
            (0, list.len())
        };
        f[off + int..off + int + acls.len()].copy_from_slice(&acls);
        wr16(&mut f, fnode::EA_INT_SIZE, int as u16);
        wr32(&mut f, fnode::EA_EXT_SIZE, ext as u32);
        wr32(&mut f, fnode::EA_SECTOR, 0);
        f[fnode::EA_IN_ANODE] = 0;
        if ext > 0 {
            let (sec, anode) = self.store_ext(fno, &list)?;
            wr32(&mut f, fnode::EA_SECTOR, sec);
            f[fnode::EA_IN_ANODE] = anode as u8;
        }
        let need = eas.iter().filter(|x| x.flags & FEA_NEEDEA != 0).count() as u32;
        wr32(&mut f, fnode::NEED_EAS, need);
        self.write_secs(fno, &f)?;
        let (mut dn, i) = self.locate(e)?;
        let d = &mut dn.entries[i];
        d.ea_size = list.len() as u32;
        d.flags &= !(de::HAS_EA | de::NEED_EA);
        if !list.is_empty() {
            d.flags |= de::HAS_EA;
        }
        if need > 0 {
            d.flags |= de::NEED_EA;
        }
        self.write_dnode(&dn)
    }

    /// Store `data` outside the F-node `fno`: (sector, is an ALSEC tree).
    fn store_ext(&mut self, fno: u32, data: &[u8]) -> Result<(u32, bool)> {
        let sectors = data.len().div_ceil(SECTOR) as u32;
        let mut runs: Vec<Extent> = Vec::new();
        let mut got = 0;
        let mut hint = fno;
        while got < sectors {
            let (s, n) = match self.alloc_run(hint, sectors - got) {
                Ok(r) => r,
                Err(x) => {
                    for r in &runs {
                        self.free_run(r.disk, r.len)?;
                    }
                    return Err(x);
                }
            };
            runs.push(Extent { file: got, len: n, disk: s });
            got += n;
            hint = s + n;
        }
        let mut buf = vec![0u8; sectors as usize * SECTOR];
        buf[..data.len()].copy_from_slice(data);
        for r in &runs {
            let o = r.file as usize * SECTOR;
            self.write_secs(r.disk, &buf[o..o + r.len as usize * SECTOR])?;
        }
        if runs.len() == 1 {
            return Ok((runs[0].disk, false));
        }
        // An ALSEC tree: one leaf ALSEC, or a root over leaf ALSECs.
        let root = self.alloc_sector(fno)?;
        let mut a = btree::new_alsec(root);
        wr32(&mut a, alsec::UP, fno);
        if runs.len() <= bt::ALSEC_LEAVES {
            btree::write_leaves(&mut a, alsec::BTREE, &runs, bt::ALSEC_LEAVES, bt::FNODE_PARENT);
        } else {
            let mut kids = Vec::new();
            for chunk in runs.chunks(bt::ALSEC_LEAVES) {
                let s = self.alloc_sector(root)?;
                let mut l = btree::new_alsec(s);
                wr32(&mut l, alsec::UP, root);
                btree::write_leaves(&mut l, alsec::BTREE, chunk, bt::ALSEC_LEAVES, 0);
                self.write_secs(s, &l)?;
                kids.push((chunk[0].file, s));
            }
            btree::write_nodes(&mut a, alsec::BTREE, &kids, bt::ALSEC_NODES, bt::FNODE_PARENT);
        }
        self.write_secs(root, &a)?;
        Ok((root, true))
    }

    /// Size and NEEDEA count of the EAs of F-node `fno` as stored.
    pub fn ea_info(&self, fno: u32) -> Result<(u32, u32)> {
        let f = self.read_fnode(fno)?;
        Ok((rd16(&f, fnode::EA_INT_SIZE) as u32 + rd32(&f, fnode::EA_EXT_SIZE), rd32(&f, fnode::NEED_EAS)))
    }

    /// Free the EA storage outside F-node buffer `f`.
    pub(crate) fn free_eas(&mut self, f: &[u8]) -> Result<()> {
        let list = self.ea_list(f)?;
        let indirect: Vec<(u32, bool, u32)> = parse(&list)?
            .iter()
            .filter(|r| r.flags & EA_INDIRECT != 0 && r.value.len() >= 8)
            .map(|r| (rd32(r.value, 4), r.flags & EA_IN_ANODE != 0, rd32(r.value, 0)))
            .collect();
        for (sec, anode, len) in indirect {
            self.free_stored(sec, anode, len)?;
        }
        let ext = rd32(f, fnode::EA_EXT_SIZE);
        if ext > 0 {
            self.free_stored(rd32(f, fnode::EA_SECTOR), f[fnode::EA_IN_ANODE] & 1 != 0, ext)?;
        }
        Ok(())
    }

    /// Free an F-node's external access control list.
    pub(crate) fn free_acls(&mut self, f: &[u8]) -> Result<()> {
        let ext = rd32(f, fnode::ACL_EXT_SIZE);
        if ext > 0 {
            self.free_stored(rd32(f, fnode::ACL_SECTOR), f[fnode::ACL_IN_ANODE] != 0, ext)?;
        }
        Ok(())
    }
}
