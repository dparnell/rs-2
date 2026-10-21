//! Extended attributes: an OS/2 FEA list (`cbList`, then per EA its
//! flags, name length, value length, name, NUL and value, unpadded),
//! described by the inode's EA dxd: inline in the inode's last 128 bytes
//! (files whose xtree root leaves that area free) or in one extent of
//! blocks. This is how OS/2's JFS stores them.

use alloc::vec;
use alloc::vec::Vec;

use crate::inode::Inode;
use crate::layout::*;
use crate::{BlockDevice, Entry, Error, Result, Volume};

/// Largest FEA list (OS/2 limits a file's EAs to 64 KB).
pub const MAX_FEALIST: usize = 0xffff;
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
        n += if i + 1 == eas.len() { one } else { (one + 3) & !3 };
    }
    n as u32
}

/// Parse a stored FEA list (with its cbList).
pub(crate) fn parse(list: &[u8]) -> Result<Vec<Ea>> {
    if list.len() < 4 {
        return Err(Error::Corrupt);
    }
    let total = (rd32(list, 0) as usize).min(list.len());
    let mut v = Vec::new();
    let mut i = 4;
    while i + 4 <= total {
        let flags = list[i];
        let nl = list[i + 1] as usize;
        let vl = rd16(list, i + 2) as usize;
        let name = i + 4;
        let value = name + nl + 1;
        if value + vl > total {
            return Err(Error::Corrupt);
        }
        v.push(Ea { flags, name: list[name..name + nl].to_vec(), value: list[value..value + vl].to_vec() });
        i = value + vl;
    }
    Ok(v)
}

/// Build the stored FEA list for `eas` (empty: no list).
pub(crate) fn build(eas: &[Ea]) -> Result<Vec<u8>> {
    if eas.is_empty() {
        return Ok(Vec::new());
    }
    let mut b = vec![0u8; 4];
    for e in eas {
        if e.name.is_empty() || e.name.len() > 255 || e.value.len() > 0xffff {
            return Err(Error::EaTooLong);
        }
        b.push(e.flags);
        b.push(e.name.len() as u8);
        b.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
        b.extend_from_slice(&e.name);
        b.push(0);
        b.extend_from_slice(&e.value);
    }
    if b.len() > MAX_FEALIST {
        return Err(Error::EaTooLong);
    }
    let n = b.len() as u32;
    wr32(&mut b, 0, n);
    Ok(b)
}

impl<D: BlockDevice> Volume<D> {
    fn ea_list(&mut self, i: &Inode) -> Result<Vec<u8>> {
        let d = i.ea();
        if d.size == 0 {
            return Ok(Vec::new());
        }
        if d.flag & dxd::INLINE != 0 {
            if d.size as usize > di::INLINEEA_SIZE || i.is_dir() {
                return Err(Error::Corrupt);
            }
            return Ok(i.b[di::INLINEEA..di::INLINEEA + d.size as usize].to_vec());
        }
        if d.flag & dxd::EXTENT != 0 {
            if d.size > MAX_EA_DATA || (d.loc.len as u64) << self.l2bsize < d.size as u64 {
                return Err(Error::Corrupt);
            }
            let mut b = self.read_blocks(d.loc.addr, d.loc.len)?;
            b.truncate(d.size as usize);
            return Ok(b);
        }
        Ok(Vec::new())
    }

    /// All extended attributes of a file or directory.
    pub fn read_eas(&mut self, e: &Entry) -> Result<Vec<Ea>> {
        let i = self.read_inode(e.ino)?;
        let l = self.ea_list(&i)?;
        if l.is_empty() { Ok(Vec::new()) } else { parse(&l) }
    }

    /// Replace the extended attributes of a file or directory.
    pub fn write_eas(&mut self, e: &Entry, eas: &[Ea]) -> Result<()> {
        let list = build(eas)?;
        self.begin()?;
        let r = self.write_eas_in(e.ino, &list);
        self.end(r)
    }

    fn write_eas_in(&mut self, ino: u32, list: &[u8]) -> Result<()> {
        let mut i = self.read_inode(ino)?;
        let old = i.ea();
        let was_inline = old.flag & dxd::INLINE != 0;
        // Inline when it fits and the area is free (or already ours).
        let area = !i.is_dir() && (i.mode() & mode::INLINEEA != 0 || was_inline);
        let mut new = Dxd::default();
        let mut nb = 0u64;
        if !list.is_empty() {
            if list.len() <= di::INLINEEA_SIZE && area {
                i.b[di::INLINEEA..].fill(0);
                i.b[di::INLINEEA..di::INLINEEA + list.len()].copy_from_slice(list);
                new = Dxd { flag: dxd::INLINE, size: list.len() as u32, loc: Pxd::default() };
                i.set_mode(i.mode() & !mode::INLINEEA);
            } else {
                let n = list.len().div_ceil(self.bsize as usize) as u32;
                let p = self.alloc_meta(i.ixpxd().addr, n)?;
                let mut b = list.to_vec();
                b.resize((n as usize) << self.l2bsize, 0);
                self.write_blocks(p.addr, &b)?;
                new = Dxd { flag: dxd::EXTENT, size: list.len() as u32, loc: p };
                nb = n as u64;
            }
        }
        if old.flag & dxd::EXTENT != 0 && old.loc.len != 0 {
            self.free_meta(old.loc)?;
            i.set_nblocks(i.nblocks().saturating_sub(old.loc.len as u64));
        }
        if was_inline && new.flag & dxd::INLINE == 0 {
            // The inline area is free again (if the xtree left it).
            i.b[di::INLINEEA..].fill(0);
            if !i.is_dir() && rd16(&i.b, di::ROOT + 20) == XTROOTINITSLOT {
                i.set_mode(i.mode() | mode::INLINEEA);
            }
        }
        i.set_ea(new);
        i.set_nblocks(i.nblocks() + nb);
        self.write_inode(&i)
    }

    /// Size of an inode's stored FEA list (0 if none).
    pub fn ea_info(&mut self, ino: u32) -> Result<u32> {
        Ok(self.read_inode(ino)?.ea().size)
    }
}
