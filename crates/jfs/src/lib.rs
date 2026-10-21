//! JFS, IBM's Journaled File System, in the layout OS/2 Warp Server for
//! e-business and Warp 4.5x write (JFS.IFS, FORMAT /FS:JFS).
//!
//! Shared by the kernel (as its JFS driver), FORMAT, the installer and
//! the build tool. Reads and writes files and directories (B+-tree
//! directories with case-preserving, case-insensitive names on OS/2
//! volumes), extended attributes (inline and in extents), and formats
//! volumes with an inline log.
//!
//! The journal: rs2 writes metadata in place, synchronously at the end of
//! each call, and writes no log records. A volume is marked mounted
//! (`s_state` 1) at the first change and clean again by
//! [`Volume::unmount`] / [`Volume::mark_clean`]. A volume whose log was
//! left in use (another system crashed with transactions in it), or that
//! is marked dirty, is opened read-only: its log is not replayed; run
//! CHKDSK on OS/2 or jfs_fsck on Linux first.
//!
//! Paths are relative to the volume root, components separated by `\` or
//! `/`. Name characters U+0000-U+00FF stand for code page bytes (see
//! [`CodePage`]). Times are seconds since 1970 (local time, as OS/2 keeps
//! them). The on-disk layout is described in `docs/formats/jfs.md`.

#![no_std]

extern crate alloc;

mod bmap;
mod check;
mod dirindex;
mod dtree;
mod ea;
mod file;
mod format;
mod imap;
mod inode;
#[allow(dead_code)] // documents the full on-disk layout
pub mod layout;
mod name;
mod upcase;
mod xtree;

#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub use check::Report;
pub use ea::{fea2_size, Ea};
pub use format::{format, FormatParams};
pub use name::{is_83, valid_name, CodePage, MAX_NAME};

use inode::Inode;
use layout::*;

/// Errors, mapped to OS/2 codes by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Io,
    /// The device does not hold a JFS volume (or one rs2 cannot read).
    NotJfs,
    NotFound,
    PathNotFound,
    Exists,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    InvalidName,
    DiskFull,
    Corrupt,
    ReadOnly,
    AccessDenied,
    /// The EAs of a file would exceed 64 KB.
    EaTooLong,
}

pub type Result<T> = core::result::Result<T, Error>;

/// A block device. Reads and writes are whole sectors and may span
/// several.
pub trait BlockDevice {
    fn sector_size(&self) -> usize;
    fn sector_count(&self) -> u64;
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<()>;
    fn write(&self, lba: u64, buf: &[u8]) -> Result<()>;
}

/// OS/2 attribute bits (stored in di_mode above bit 25).
pub mod attr {
    pub const READONLY: u8 = 0x01;
    pub const HIDDEN: u8 = 0x02;
    pub const SYSTEM: u8 = 0x04;
    pub const DIRECTORY: u8 = 0x10;
    pub const ARCHIVE: u8 = 0x20;
    /// The name does not fit the FAT 8.3 rules.
    pub const LONG_NAME: u8 = 0x40;
    /// Bits a caller may set.
    pub const CHANGEABLE: u8 = READONLY | HIDDEN | SYSTEM | ARCHIVE;
}

/// A file or directory.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Name as stored (case preserved); empty for the root.
    pub name: String,
    pub attr: u8,
    pub size: u64,
    /// Inode number; identifies the file.
    pub ino: u32,
    /// Inode of the containing directory (the root's own for the root).
    pub parent: u32,
    pub created: u32,
    pub accessed: u32,
    pub modified: u32,
    /// Size of the stored FEA list (with its 4-byte length), 0 if none.
    pub ea_size: u32,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.attr & attr::DIRECTORY != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Info {
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub block_size: u32,
    pub serial: u32,
}

/// A cached metadata block.
struct CBlock {
    data: Vec<u8>,
    dirty: bool,
}

/// Clean blocks kept after a flush.
const CACHE_KEEP: usize = 4096;

pub struct Volume<D: BlockDevice> {
    pub dev: D,
    /// Device sector size.
    pub(crate) ss: u32,
    pub(crate) bsize: u32,
    pub(crate) l2bsize: u32,
    /// Blocks per 4 KB page.
    pub(crate) nbperpage: u32,
    /// The superblock (first 4 KB of it).
    pub(crate) sb: Vec<u8>,
    pub(crate) flags: u32,
    /// Case-insensitive names (OS/2 volume).
    pub(crate) ci: bool,
    /// Directories keep index tables (Linux).
    pub(crate) dir_index: bool,
    cache: BTreeMap<u64, CBlock>,
    pub(crate) bmap: bmap::Bmap,
    pub(crate) imap: imap::Imap,
    /// The fileset inode map's inode (FILESYSTEM_I).
    pub(crate) fsi: Inode,
    pub(crate) fsi_dirty: bool,
    pub(crate) inostamp: u32,
    /// First block of the secondary aggregate inode table.
    pub(crate) ait2: u64,
    /// Code page of names.
    pub cp: CodePage,
    pub serial: u32,
    /// Volume label (up to 11 characters, code page bytes).
    pub label: String,
    readonly: bool,
    /// Superblock marked mounted by us (cleared by mark_clean).
    marked: bool,
}

/// Name components of a path ("." skipped, ".." kept).
fn components(path: &str) -> Vec<&str> {
    path.split(['\\', '/']).filter(|s| !s.is_empty() && *s != ".").collect()
}

impl<D: BlockDevice> Volume<D> {
    pub fn mount(dev: D) -> Result<Self> {
        let ss = dev.sector_size() as u32;
        if !ss.is_power_of_two() || !(512..=4096).contains(&ss) {
            return Err(Error::NotJfs);
        }
        let read_sb = |off: u64| -> Result<Vec<u8>> {
            let mut b = vec![0u8; PSIZE];
            dev.read(off / ss as u64, &mut b)?;
            Ok(b)
        };
        let ok = |b: &[u8]| {
            let bs = rd32(b, sb::BSIZE);
            &b[0..4] == MAGIC
                && (1..=2).contains(&rd32(b, sb::VERSION))
                && bs.is_power_of_two()
                && (512..=4096).contains(&bs)
                && 1 << rd16(b, sb::L2BSIZE) == bs
                && rd32(b, sb::PBSIZE) == ss
                && rd32(b, sb::AGSIZE) != 0
        };
        let mut s = read_sb(SUPER1_OFF)?;
        if !ok(&s) {
            s = read_sb(SUPER2_OFF)?;
            if !ok(&s) {
                return Err(Error::NotJfs);
            }
        }
        let bsize = rd32(&s, sb::BSIZE);
        let flags = rd32(&s, sb::FLAG);
        let mut v = Volume {
            dev,
            ss,
            bsize,
            l2bsize: log2(bsize as u64),
            nbperpage: PSIZE as u32 / bsize,
            flags,
            ci: flags & flag::OS2 != 0,
            dir_index: flags & flag::DIR_INDEX != 0,
            cache: BTreeMap::new(),
            bmap: bmap::Bmap::default(),
            imap: imap::Imap::default(),
            fsi: Inode::empty(),
            fsi_dirty: false,
            inostamp: 0,
            ait2: Pxd::read(&s, sb::AIT2).addr,
            sb: s,
            cp: CodePage::builtin(850),
            serial: 0,
            label: String::new(),
            readonly: false,
            marked: false,
        };
        v.mount_maps()?;
        v.readonly = !v.can_write()?;
        v.read_label()?;
        let root = v.read_inode(ROOT_I)?;
        if !root.is_dir() {
            return Err(Error::Corrupt);
        }
        Ok(v)
    }

    fn mount_maps(&mut self) -> Result<()> {
        let bi = self.read_ait(BMAP_I, false)?;
        let bfile = self.xt_load(&bi)?.0;
        self.bmap_mount(bfile)?;
        self.fsi = self.read_ait(FILESYSTEM_I, false)?;
        self.inostamp = rd32(&self.fsi.b, di::INOSTAMP);
        let ifile = self.xt_load(&self.fsi.clone())?.0;
        self.imap_mount(ifile)
    }

    /// May the volume be changed? Only when it is clean or was left
    /// mounted with its log unused (rs2 itself writes no log records).
    fn can_write(&mut self) -> Result<bool> {
        match rd32(&self.sb, sb::STATE) {
            state::CLEAN => Ok(true),
            state::MOUNT if self.flags & flag::INLINELOG != 0 => Ok(self.log_state()? == Some(LOGREDONE)),
            _ => Ok(false),
        }
    }

    fn read_label(&mut self) -> Result<()> {
        let fp = &self.sb[sb::FPACK..sb::FPACK + 11];
        let n = fp.iter().position(|&c| c == 0).unwrap_or(11);
        self.label = fp[..n].iter().map(|&c| c as char).collect::<String>().trim_end().into();
        let mut bs = vec![0u8; self.ss as usize];
        self.dev.read(0, &mut bs)?;
        self.serial = if bs[38] == 0x29 && bs[510..512] == [0x55, 0xaa] {
            rd32(&bs, 39)
        } else {
            rd32(&self.sb, sb::UUID) ^ rd32(&self.sb, sb::UUID + 4)
        };
        Ok(())
    }

    pub fn set_readonly(&mut self, ro: bool) {
        self.readonly = ro;
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    /// Superblock flags (see [`layout::flag`]).
    pub fn flags(&self) -> u32 {
        self.flags
    }

    pub fn block_size(&self) -> u32 {
        self.bsize
    }

    // ---- Device and metadata block I/O ----

    pub(crate) fn dev_read(&self, off: u64, buf: &mut [u8]) -> Result<()> {
        self.dev.read(off / self.ss as u64, buf)
    }

    pub(crate) fn dev_write(&self, off: u64, buf: &[u8]) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        self.dev.write(off / self.ss as u64, buf)
    }

    fn cblock(&mut self, blk: u64) -> Result<&mut CBlock> {
        if !self.cache.contains_key(&blk) {
            let mut data = vec![0u8; self.bsize as usize];
            self.dev_read(blk << self.l2bsize, &mut data)?;
            self.cache.insert(blk, CBlock { data, dirty: false });
        }
        Ok(self.cache.get_mut(&blk).unwrap())
    }

    /// Read metadata bytes (through the block cache).
    pub(crate) fn meta_read(&mut self, off: u64, len: usize) -> Result<Vec<u8>> {
        let mut out = vec![0u8; len];
        let bs = self.bsize as u64;
        let mut done = 0;
        while done < len {
            let pos = off + done as u64;
            let (blk, o) = (pos / bs, (pos % bs) as usize);
            let n = (bs as usize - o).min(len - done);
            let b = self.cblock(blk)?;
            out[done..done + n].copy_from_slice(&b.data[o..o + n]);
            done += n;
        }
        Ok(out)
    }

    /// Write metadata bytes into the cache (written out by `flush`).
    pub(crate) fn meta_write(&mut self, off: u64, data: &[u8]) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        let bs = self.bsize as u64;
        let mut done = 0;
        while done < data.len() {
            let pos = off + done as u64;
            let (blk, o) = (pos / bs, (pos % bs) as usize);
            let n = (bs as usize - o).min(data.len() - done);
            if o == 0 && n == bs as usize {
                self.cache.insert(blk, CBlock { data: data[done..done + n].to_vec(), dirty: true });
            } else {
                let b = self.cblock(blk)?;
                b.data[o..o + n].copy_from_slice(&data[done..done + n]);
                b.dirty = true;
            }
            done += n;
        }
        Ok(())
    }

    /// Read `n` metadata blocks from `blk`.
    pub(crate) fn read_blocks(&mut self, blk: u64, n: u32) -> Result<Vec<u8>> {
        self.meta_read(blk << self.l2bsize, (n as usize) << self.l2bsize)
    }

    pub(crate) fn write_blocks(&mut self, blk: u64, data: &[u8]) -> Result<()> {
        self.meta_write(blk << self.l2bsize, data)
    }

    /// Drop blocks that stopped being metadata (freed, or now file data).
    pub(crate) fn forget(&mut self, blk: u64, n: u64) {
        if n > 64 {
            let keys: Vec<u64> = self.cache.range(blk..blk + n).map(|(k, _)| *k).collect();
            for k in keys {
                self.cache.remove(&k);
            }
        } else {
            for b in blk..blk + n {
                self.cache.remove(&b);
            }
        }
    }

    /// Write out the maps and every dirty metadata block.
    pub fn flush(&mut self) -> Result<()> {
        if self.readonly {
            return Ok(());
        }
        self.bmap_flush()?;
        self.imap_flush()?;
        if self.fsi_dirty {
            let f = self.fsi.clone();
            self.write_ait(&f)?;
            self.fsi_dirty = false;
        }
        let dirty: Vec<u64> = self.cache.iter().filter(|(_, b)| b.dirty).map(|(k, _)| *k).collect();
        let bs = self.bsize as usize;
        let mut i = 0;
        while i < dirty.len() {
            let mut j = i + 1;
            while j < dirty.len() && dirty[j] == dirty[j - 1] + 1 && j - i < 64 {
                j += 1;
            }
            let mut buf = Vec::with_capacity((j - i) * bs);
            for k in &dirty[i..j] {
                buf.extend_from_slice(&self.cache[k].data);
            }
            self.dev_write(dirty[i] << self.l2bsize, &buf)?;
            for k in &dirty[i..j] {
                self.cache.get_mut(k).unwrap().dirty = false;
            }
            i = j;
        }
        if self.cache.len() > CACHE_KEEP {
            self.cache.clear();
        }
        Ok(())
    }

    /// Before a change: refuse on a read-only volume, mark it mounted.
    pub(crate) fn begin(&mut self) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        if !self.marked {
            self.write_state(state::MOUNT)?;
            self.marked = true;
        }
        Ok(())
    }

    /// After a change: write everything out (also when it failed half
    /// way, so the maps match what was written).
    pub(crate) fn end<T>(&mut self, r: Result<T>) -> Result<T> {
        let f = self.flush();
        let v = r?;
        f?;
        Ok(v)
    }

    fn write_state(&mut self, st: u32) -> Result<()> {
        wr32(&mut self.sb, sb::STATE, st);
        self.write_super()
    }

    /// Write both superblock copies.
    pub(crate) fn write_super(&mut self) -> Result<()> {
        let n = (self.ss as usize).max(512).min(PSIZE);
        let s = self.sb[..n].to_vec();
        self.dev_write(SUPER1_OFF, &s)?;
        self.dev_write(SUPER2_OFF, &s)
    }

    /// Flush and mark the volume clean (until the next change).
    pub fn mark_clean(&mut self) -> Result<()> {
        if self.readonly {
            return Ok(());
        }
        self.flush()?;
        if self.marked {
            self.write_state(state::CLEAN)?;
            self.marked = false;
        }
        Ok(())
    }

    /// Flush, mark clean and give the device back.
    pub fn unmount(mut self) -> Result<D> {
        self.mark_clean()?;
        Ok(self.dev)
    }

    pub fn info(&self) -> Info {
        Info { total_blocks: self.bmap.mapsize, free_blocks: self.bmap.nfree, block_size: self.bsize, serial: self.serial }
    }

    /// Set the volume label (superblock and boot sector).
    pub fn set_label(&mut self, label: &str) -> Result<()> {
        self.begin()?;
        let l: Vec<u8> = label.chars().take(11).map(|c| u8::try_from(c as u32).unwrap_or(b'_')).collect();
        self.sb[sb::FPACK..sb::FPACK + 11].fill(0);
        self.sb[sb::FPACK..sb::FPACK + l.len()].copy_from_slice(&l);
        self.sb[sb::LABEL..sb::LABEL + 16].fill(0);
        self.sb[sb::LABEL..sb::LABEL + l.len()].copy_from_slice(&l);
        self.write_super()?;
        let mut bs = vec![0u8; self.ss as usize];
        self.dev.read(0, &mut bs)?;
        if bs[38] == 0x29 && bs[510..512] == [0x55, 0xaa] {
            bs[43..54].fill(b' ');
            bs[43..43 + l.len()].copy_from_slice(&l);
            self.dev_write(0, &bs)?;
        }
        self.label = l.iter().map(|&c| c as char).collect();
        Ok(())
    }

    /// The inline log's superblock state, if the volume has one.
    fn log_state(&mut self) -> Result<Option<u32>> {
        let lp = Pxd::read(&self.sb, sb::LOGPXD);
        if self.flags & flag::INLINELOG == 0 || lp.len == 0 {
            return Ok(None);
        }
        let mut b = vec![0u8; LOGPSIZE];
        self.dev_read((lp.addr << self.l2bsize) + LOGPSIZE as u64, &mut b)?;
        if rd32(&b, logsup::MAGIC) != LOGMAGIC {
            return Ok(None);
        }
        Ok(Some(rd32(&b, logsup::STATE)))
    }

    // ---- Entries and paths ----

    pub(crate) fn entry_of(&self, ino: &Inode, name: String, parent: u32) -> Entry {
        let m = ino.mode();
        let mut a = (m >> mode::ATTRSHIFT) as u8 & (attr::CHANGEABLE | attr::LONG_NAME);
        if ino.is_dir() {
            a |= attr::DIRECTORY;
        }
        Entry {
            name,
            attr: a,
            size: if ino.is_dir() { 0 } else { ino.size() },
            ino: ino.ino,
            parent,
            created: ino.time(di::OTIME),
            accessed: ino.time(di::ATIME),
            modified: ino.time(di::MTIME),
            ea_size: ino.ea().size,
        }
    }

    /// The root directory.
    pub fn root_entry(&mut self) -> Result<Entry> {
        let r = self.read_inode(ROOT_I)?;
        Ok(self.entry_of(&r, String::new(), ROOT_I))
    }

    fn encode(&self, name: &str) -> Result<Vec<u16>> {
        let u = self.cp.encode(name).ok_or(Error::InvalidName)?;
        if !valid_name(&u) {
            return Err(Error::InvalidName);
        }
        Ok(u)
    }

    /// Look up `name` in directory `dir`.
    pub fn find(&mut self, dir: &Entry, name: &str) -> Result<Option<Entry>> {
        if !dir.is_dir() {
            return Err(Error::NotADirectory);
        }
        let Ok(u) = self.encode(name) else { return Ok(None) };
        let d = self.read_inode(dir.ino)?;
        match self.dt_lookup(&d, &u)? {
            Some((stored, ino)) => {
                let i = self.read_inode(ino)?;
                let n = self.cp.decode(&stored);
                Ok(Some(self.entry_of(&i, n, dir.ino)))
            }
            None => Ok(None),
        }
    }

    /// Look up a path; the empty path is the root directory.
    pub fn lookup(&mut self, path: &str) -> Result<Entry> {
        let parts = components(path);
        let mut stack = vec![self.root_entry()?];
        for (i, p) in parts.iter().enumerate() {
            let cur = stack.last().unwrap().clone();
            if !cur.is_dir() {
                return Err(Error::PathNotFound);
            }
            if *p == ".." {
                if stack.len() > 1 {
                    stack.pop();
                }
                continue;
            }
            match self.find(&cur, p)? {
                Some(e) => stack.push(e),
                None => return Err(if i + 1 == parts.len() { Error::NotFound } else { Error::PathNotFound }),
            }
        }
        Ok(stack.pop().unwrap())
    }

    /// Split a path into (parent directory, final component in UCS-2).
    fn parent(&mut self, path: &str) -> Result<(Entry, Vec<u16>)> {
        let parts = components(path);
        let Some((last, dirs)) = parts.split_last() else { return Err(Error::InvalidName) };
        let u = self.encode(last)?;
        let mut p = String::new();
        for d in dirs {
            p.push_str(d);
            p.push('\\');
        }
        let parent = self.lookup(&p).map_err(|e| if e == Error::NotFound { Error::PathNotFound } else { e })?;
        if !parent.is_dir() {
            return Err(Error::PathNotFound);
        }
        Ok((parent, u))
    }

    /// The entries of a directory, in directory order.
    pub fn read_dir(&mut self, dir: &Entry) -> Result<Vec<Entry>> {
        if !dir.is_dir() {
            return Err(Error::NotADirectory);
        }
        let d = self.read_inode(dir.ino)?;
        let list = self.dt_list(&d)?;
        let mut out = Vec::with_capacity(list.len());
        for (name, ino) in list {
            let i = self.read_inode(ino)?;
            let n = self.cp.decode(&name);
            out.push(self.entry_of(&i, n, dir.ino));
        }
        Ok(out)
    }

    /// OS/2 attributes for a new object named `n`.
    fn new_attr(n: &[u16], a: u8) -> u8 {
        (a & attr::CHANGEABLE) | if is_83(n) { 0 } else { attr::LONG_NAME }
    }

    pub fn create_file(&mut self, path: &str, attr_bits: u8, now: u32) -> Result<Entry> {
        let (parent, n) = self.parent(path)?;
        self.begin()?;
        let r = self.create_in(&parent, &n, Self::new_attr(&n, attr_bits), false, now);
        self.end(r)
    }

    pub fn mkdir(&mut self, path: &str, now: u32) -> Result<Entry> {
        let (parent, n) = self.parent(path)?;
        self.begin()?;
        let r = self.create_in(&parent, &n, Self::new_attr(&n, 0), true, now);
        self.end(r)
    }

    fn create_in(&mut self, parent: &Entry, n: &[u16], a: u8, dir: bool, now: u32) -> Result<Entry> {
        let mut pd = self.read_inode(parent.ino)?;
        if self.dt_lookup(&pd, n)?.is_some() {
            return Err(Error::Exists);
        }
        let ino = self.ialloc(parent.ino, dir)?;
        let mut i = self.new_inode(ino, dir, a, parent.ino, now)?;
        if let Err(e) = self.dt_insert(&mut pd, n, ino) {
            self.ifree(ino)?;
            return Err(e);
        }
        self.write_inode(&i)?;
        if dir {
            pd.set_nlink(pd.nlink() + 1);
        }
        pd.set_time(di::MTIME, now);
        pd.set_time(di::CTIME, now);
        self.write_inode(&pd)?;
        i = self.read_inode(ino)?;
        Ok(self.entry_of(&i, self.cp.decode(n), parent.ino))
    }

    pub fn delete(&mut self, path: &str) -> Result<()> {
        let e = self.lookup(path)?;
        if e.is_dir() {
            return Err(Error::IsADirectory);
        }
        if e.attr & attr::READONLY != 0 {
            return Err(Error::AccessDenied);
        }
        self.begin()?;
        let r = self.remove(&e, false);
        self.end(r)
    }

    pub fn rmdir(&mut self, path: &str) -> Result<()> {
        let e = self.lookup(path)?;
        if !e.is_dir() {
            return Err(Error::NotADirectory);
        }
        if e.ino == ROOT_I {
            return Err(Error::AccessDenied);
        }
        let d = self.read_inode(e.ino)?;
        if !self.dt_list(&d)?.is_empty() {
            return Err(Error::DirectoryNotEmpty);
        }
        self.begin()?;
        let r = self.remove(&e, true);
        self.end(r)
    }

    /// Remove entry `e` from its directory and free its inode.
    fn remove(&mut self, e: &Entry, dir: bool) -> Result<()> {
        let mut pd = self.read_inode(e.parent)?;
        let n = self.cp.encode(&e.name).ok_or(Error::InvalidName)?;
        self.dt_delete(&mut pd, &n)?;
        if dir {
            pd.set_nlink(pd.nlink().saturating_sub(1).max(2));
        }
        self.write_inode(&pd)?;
        let mut i = self.read_inode(e.ino)?;
        self.free_inode_storage(&mut i)?;
        self.ifree(e.ino)
    }

    /// Rename or move a file or directory. The target must not exist
    /// (unless it is the same object: a change of case).
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        let e = self.lookup(from)?;
        if e.ino == ROOT_I {
            return Err(Error::AccessDenied);
        }
        let (np, nn) = self.parent(to)?;
        let npd = self.read_inode(np.ino)?;
        if let Some((_, ino)) = self.dt_lookup(&npd, &nn)? {
            if ino != e.ino {
                return Err(Error::Exists);
            }
        }
        if e.is_dir() {
            // Not into itself or below.
            let mut p = np.ino;
            let mut guard = 0;
            while p != ROOT_I && guard < 4096 {
                if p == e.ino {
                    return Err(Error::AccessDenied);
                }
                p = self.read_inode(p)?.parent();
                guard += 1;
            }
            if np.ino == e.ino {
                return Err(Error::AccessDenied);
            }
        }
        self.begin()?;
        let r = self.rename_in(&e, &np, &nn);
        self.end(r)
    }

    fn rename_in(&mut self, e: &Entry, np: &Entry, nn: &[u16]) -> Result<()> {
        let old = self.cp.encode(&e.name).ok_or(Error::InvalidName)?;
        let mut opd = self.read_inode(e.parent)?;
        self.dt_delete(&mut opd, &old)?;
        if e.parent == np.ino {
            self.dt_insert(&mut opd, nn, e.ino)?;
            self.write_inode(&opd)?;
        } else {
            let mut npd = self.read_inode(np.ino)?;
            self.dt_insert(&mut npd, nn, e.ino)?;
            if e.is_dir() {
                opd.set_nlink(opd.nlink().saturating_sub(1).max(2));
                npd.set_nlink(npd.nlink() + 1);
                let mut d = self.read_inode(e.ino)?;
                self.dt_set_parent(&mut d, np.ino)?;
                self.write_inode(&d)?;
            }
            self.write_inode(&opd)?;
            self.write_inode(&npd)?;
        }
        // The long-name attribute follows the new name.
        let mut i = self.read_inode(e.ino)?;
        let a = Self::new_attr(nn, (i.mode() >> mode::ATTRSHIFT) as u8);
        i.set_attr(a);
        self.write_inode(&i)
    }

    /// Store an entry's attributes and times.
    pub fn update_entry(&mut self, e: &Entry) -> Result<()> {
        self.begin()?;
        let r = (|| {
            let mut i = self.read_inode(e.ino)?;
            let keep = (i.mode() >> mode::ATTRSHIFT) as u8 & attr::LONG_NAME;
            i.set_attr((e.attr & attr::CHANGEABLE) | keep);
            i.set_time(di::OTIME, e.created);
            i.set_time(di::ATIME, e.accessed);
            i.set_time(di::MTIME, e.modified);
            self.write_inode(&i)
        })();
        self.end(r)
    }
}
