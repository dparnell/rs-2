//! Inodes: 512-byte records in 16 KB extents, located through the inode
//! maps (the fileset's for files and directories; the aggregate inodes
//! sit in the fixed aggregate inode table at 44 KB and its copy).

use alloc::vec;
use alloc::vec::Vec;

use crate::layout::*;
use crate::{BlockDevice, Error, Result, Volume};

#[derive(Clone)]
pub(crate) struct Inode {
    pub ino: u32,
    /// Byte offset on the device.
    pub off: u64,
    pub b: Vec<u8>,
}

impl Inode {
    pub fn empty() -> Self {
        Inode { ino: 0, off: 0, b: vec![0u8; DISIZE] }
    }

    pub fn mode(&self) -> u32 {
        rd32(&self.b, di::MODE)
    }

    pub fn set_mode(&mut self, m: u32) {
        wr32(&mut self.b, di::MODE, m);
    }

    pub fn is_dir(&self) -> bool {
        self.mode() & mode::IFMT == mode::IFDIR
    }

    pub fn size(&self) -> u64 {
        rd64(&self.b, di::SIZE)
    }

    pub fn set_size(&mut self, s: u64) {
        wr64(&mut self.b, di::SIZE, s);
    }

    pub fn nblocks(&self) -> u64 {
        rd64(&self.b, di::NBLOCKS)
    }

    pub fn set_nblocks(&mut self, n: u64) {
        wr64(&mut self.b, di::NBLOCKS, n);
    }

    pub fn nlink(&self) -> u32 {
        rd32(&self.b, di::NLINK)
    }

    pub fn set_nlink(&mut self, n: u32) {
        wr32(&mut self.b, di::NLINK, n);
    }

    pub fn time(&self, at: usize) -> u32 {
        rd32(&self.b, at)
    }

    pub fn set_time(&mut self, at: usize, secs: u32) {
        wr32(&mut self.b, at, secs);
        wr32(&mut self.b, at + 4, 0);
    }

    pub fn ea(&self) -> Dxd {
        Dxd::read(&self.b, di::EA)
    }

    pub fn set_ea(&mut self, d: Dxd) {
        d.write(&mut self.b, di::EA);
    }

    pub fn acl(&self) -> Dxd {
        Dxd::read(&self.b, di::ACL)
    }

    /// Set the OS/2 attribute byte (the directory bit follows the type).
    pub fn set_attr(&mut self, a: u8) {
        let mut a = a as u32 & 0x6f;
        if self.is_dir() {
            a |= 0x10;
        }
        let m = self.mode() & ((1 << mode::ATTRSHIFT) - 1) | a << mode::ATTRSHIFT;
        self.set_mode(m);
    }

    /// The parent directory of a directory (dtree root's idotdot).
    pub fn parent(&self) -> u32 {
        rd32(&self.b, di::ROOT + 20)
    }

    pub fn ixpxd(&self) -> Pxd {
        Pxd::read(&self.b, di::IXPXD)
    }
}

impl<D: BlockDevice> Volume<D> {
    /// Aggregate inode `ino` from the primary or secondary table.
    pub(crate) fn read_ait(&mut self, ino: u32, secondary: bool) -> Result<Inode> {
        let base = if secondary { self.ait2 << self.l2bsize } else { AITBL_OFF };
        let off = base + ino as u64 * DISIZE as u64;
        let b = self.meta_read(off, DISIZE)?;
        Ok(Inode { ino, off, b })
    }

    /// Write an aggregate inode to both tables.
    pub(crate) fn write_ait(&mut self, i: &Inode) -> Result<()> {
        let off = AITBL_OFF + i.ino as u64 * DISIZE as u64;
        self.meta_write(off, &i.b)?;
        if self.ait2 != 0 {
            let mut b = i.b.clone();
            Pxd::new(self.ait2, (INODE_EXTENT_SIZE >> self.l2bsize) as u32).write(&mut b, di::IXPXD);
            self.meta_write((self.ait2 << self.l2bsize) + i.ino as u64 * DISIZE as u64, &b)?;
        }
        Ok(())
    }

    pub(crate) fn read_inode(&mut self, ino: u32) -> Result<Inode> {
        let off = self.inode_off(ino)?;
        let b = self.meta_read(off, DISIZE)?;
        if rd32(&b, di::NUMBER) != ino {
            return Err(Error::Corrupt);
        }
        Ok(Inode { ino, off, b })
    }

    pub(crate) fn write_inode(&mut self, i: &Inode) -> Result<()> {
        self.meta_write(i.off, &i.b)
    }

    /// Fill in a newly allocated inode.
    pub(crate) fn new_inode(&mut self, ino: u32, dir: bool, a: u8, parent: u32, now: u32) -> Result<Inode> {
        let off = self.inode_off(ino)?;
        let old = self.meta_read(off, DISIZE)?;
        let mut b = vec![0u8; DISIZE];
        wr32(&mut b, di::INOSTAMP, self.inostamp);
        wr32(&mut b, di::FILESET, FILESYSTEM_I);
        wr32(&mut b, di::NUMBER, ino);
        b[di::IXPXD..di::IXPXD + 8].copy_from_slice(&old[di::IXPXD..di::IXPXD + 8]);
        let g = rd32(&self.fsi.b, di::GENGEN);
        wr32(&mut b, di::GEN, g);
        wr32(&mut self.fsi.b, di::GENGEN, g.wrapping_add(1));
        self.fsi_dirty = true;
        let mut i = Inode { ino, off, b };
        i.set_nlink(if dir { 2 } else { 1 });
        i.set_mode(if dir { mode::IFDIR | mode::IFJOURNAL } else { mode::IFREG | mode::INLINEEA });
        i.set_attr(a);
        for t in [di::ATIME, di::CTIME, di::MTIME, di::OTIME] {
            i.set_time(t, now);
        }
        if dir {
            self.dt_init_root(&mut i, parent);
        } else {
            Self::xt_init_root(&mut i, di::ROOT, XTROOTINITSLOT);
        }
        Ok(i)
    }

    /// Free everything an inode owns (data, tree pages, EAs, ACL) and
    /// clear it.
    pub(crate) fn free_inode_storage(&mut self, i: &mut Inode) -> Result<()> {
        if i.is_dir() {
            for p in self.dt_pages(i)? {
                self.free_meta(p)?;
            }
            if self.dir_index && rd32(&i.b, di::NEXT_INDEX) > MAX_INLINE_DIRTABLE_ENTRY {
                let (x, pages) = self.xt_load_at(i, di::DIRTABLE)?;
                for e in x {
                    self.free_meta(Pxd::new(e.addr, e.len))?;
                }
                for p in pages {
                    self.free_meta(p)?;
                }
            }
        } else if i.mode() & mode::IFMT != mode::IFLNK || i.size() > 128 {
            let (x, pages) = self.xt_load(i)?;
            for e in x {
                self.free_blocks(Pxd::new(e.addr, e.len))?;
            }
            for p in pages {
                self.free_meta(p)?;
            }
        }
        for d in [i.ea(), i.acl()] {
            if d.flag & dxd::EXTENT != 0 && d.loc.len != 0 {
                self.free_meta(d.loc)?;
            }
        }
        i.b[di::SIZE..].fill(0);
        self.write_inode(i)
    }

    /// Free metadata blocks (and drop them from the cache).
    pub(crate) fn free_meta(&mut self, p: Pxd) -> Result<()> {
        self.forget(p.addr, p.len as u64);
        self.free_blocks(p)
    }
}
