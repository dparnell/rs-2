//! Extended attributes (read-only) and freeing of EA/ACL storage.
//!
//! An F-node's EAs are a packed FEA list: part inside the F-node (from
//! `fnode::EA_OFFSET`, `EA_INT_SIZE` bytes) and part outside (`EA_EXT_SIZE`
//! bytes at `EA_SECTOR`: a contiguous run, or an ALSEC tree when
//! `EA_IN_ANODE` is set). A large value may itself be stored outside the
//! list ("indirect": the value field holds its length and sector).

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Entry, Error, Result, Volume};

/// FEA flags: the value is stored elsewhere / in an ALSEC tree.
const EA_INDIRECT: u8 = 0x01;
const EA_IN_ANODE: u8 = 0x02;
/// Largest EA data accepted (OS/2 limits a file's EAs to 64 KB).
const MAX_EA_DATA: u32 = 1 << 20;

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
    pub fn read_eas(&mut self, e: &Entry) -> Result<Vec<Ea>> {
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
