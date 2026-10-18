//! HPFS, the High Performance File System of OS/2.
//!
//! Shared by the kernel (as its HPFS driver) and the build tool (to make
//! test disks). Supports reading and writing files and directories, long
//! case-preserving names with case-insensitive lookup through the volume's
//! code page case map, extended attributes, and formatting.
//!
//! Paths are relative to the volume root, components separated by `\` or
//! `/`. Name characters U+0000-U+00FF stand for code page bytes. Times are
//! seconds since 1970 (see [`time`]). The on-disk layout is described in
//! `docs/formats/hpfs.md`.

#![no_std]

extern crate alloc;

mod bitmap;
mod btree;
mod dir;
mod ea;
mod file;
mod format;
#[allow(dead_code)] // documents the full on-disk layout
mod layout;
mod name;
pub mod time;

#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub use ea::{fea2_size, Ea};
pub use format::{format, FormatParams};
pub use name::{is_83, valid_name, CaseMap, MAX_NAME};

use dir::Dirent;
use layout::*;

/// Errors, mapped to OS/2 codes by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Io,
    /// The device does not hold an HPFS volume.
    NotHpfs,
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

/// A device of 512-byte sectors. Reads and writes may span several
/// sectors.
pub trait BlockDevice {
    fn sector_size(&self) -> usize;
    fn sector_count(&self) -> u64;
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<()>;
    fn write(&self, lba: u64, buf: &[u8]) -> Result<()>;
}

/// DIRENT attribute bits (the DOS attributes plus the long-name flag).
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
    pub size: u32,
    /// Sector of the F-node; identifies the file.
    pub fnode: u32,
    /// F-node of the containing directory (the root's own for the root).
    pub parent: u32,
    pub created: u32,
    pub accessed: u32,
    pub modified: u32,
    /// Size of the extended attributes on disk (FEA list), 0 if none.
    pub ea_size: u32,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.attr & attr::DIRECTORY != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Info {
    pub total_sectors: u32,
    pub free_sectors: u32,
    pub serial: u32,
    pub sector_size: u32,
}

/// A run of sectors of a file: file sectors `file..file+len` live at disk
/// sectors `disk..disk+len`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Extent {
    pub file: u32,
    pub len: u32,
    pub disk: u32,
}

/// The directory band: a region reserved for DNODEs with its own bitmap.
pub(crate) struct DirBand {
    pub start: u32,
    /// Number of DNODEs in the band.
    pub count: u32,
    pub bitmap: u32,
    pub bits: Vec<u8>,
}

pub struct Volume<D: BlockDevice> {
    pub dev: D,
    pub(crate) total: u32,
    pub(crate) root: u32,
    /// Bitmap location for each band.
    pub(crate) bitmap_secs: Vec<u32>,
    pub(crate) bitmaps: Vec<Vec<u8>>,
    pub(crate) free: u32,
    pub(crate) band: DirBand,
    /// Hot-fixed sectors: (bad sector, replacement).
    pub(crate) hotfix: Vec<(u32, u32)>,
    /// Case map of the volume's code page.
    pub case: CaseMap,
    pub serial: u32,
    pub label: [u8; 11],
    readonly: bool,
    /// Extents of the most recently used file: (F-node, extents, ALSECs).
    pub(crate) cache: Option<(u32, Vec<Extent>, Vec<u32>)>,
}

/// Name components of a path ("." skipped, ".." kept).
fn components(path: &str) -> Result<Vec<Vec<u8>>> {
    path.split(['\\', '/'])
        .filter(|s| !s.is_empty() && *s != ".")
        .map(|s| name::to_bytes(s).ok_or(Error::InvalidName))
        .collect()
}

/// The `..` entry's name.
pub(crate) const DOTDOT: &[u8] = &[1, 1];

impl<D: BlockDevice> Volume<D> {
    pub fn mount(dev: D) -> Result<Self> {
        if dev.sector_size() != SECTOR {
            return Err(Error::NotHpfs);
        }
        let mut s = vec![0u8; SECTOR];
        dev.read(SUPER_LBA as u64, &mut s)?;
        if (rd32(&s, 0), rd32(&s, 4)) != SUPER_MAGIC {
            return Err(Error::NotHpfs);
        }
        let mut sp = vec![0u8; SECTOR];
        dev.read(SPARE_LBA as u64, &mut sp)?;
        if (rd32(&sp, 0), rd32(&sp, 4)) != SPARE_MAGIC {
            return Err(Error::NotHpfs);
        }
        let total = rd32(&s, sb::SECTORS).min(dev.sector_count().min(u32::MAX as u64) as u32);
        let mut v = Volume {
            dev,
            total,
            root: rd32(&s, sb::ROOT),
            bitmap_secs: Vec::new(),
            bitmaps: Vec::new(),
            free: 0,
            band: DirBand {
                start: rd32(&s, sb::DIR_BAND_START),
                count: rd32(&s, sb::DIR_BAND_SECTORS) / DNODE_SECTORS,
                bitmap: rd32(&s, sb::DIR_BAND_BITMAP),
                bits: Vec::new(),
            },
            hotfix: Vec::new(),
            case: CaseMap::for_code_page(850),
            serial: 0,
            label: [b' '; 11],
            readonly: false,
            cache: None,
        };
        v.load_hotfixes(&sp)?;
        v.load_bitmaps(rd32(&s, sb::BITMAPS))?;
        v.load_code_page(&sp)?;
        let mut bs = vec![0u8; SECTOR];
        v.read_secs(0, &mut bs)?;
        if bs[38] == 0x29 {
            v.serial = rd32(&bs, 39);
            v.label.copy_from_slice(&bs[43..54]);
        }
        let root = v.read_fnode(v.root)?;
        if root[fnode::FLAGS] & fnode::DIR == 0 {
            return Err(Error::Corrupt);
        }
        Ok(v)
    }

    fn load_hotfixes(&mut self, sp: &[u8]) -> Result<()> {
        let used = rd32(sp, sp::HOTFIX_USED) as usize;
        let count = rd32(sp, sp::HOTFIX_COUNT) as usize;
        let list = rd32(sp, sp::HOTFIX_LIST);
        if used == 0 || list == 0 {
            return Ok(());
        }
        if used > count || count * 12 > 4 * SECTOR {
            return Err(Error::Corrupt);
        }
        let mut b = vec![0u8; 4 * SECTOR];
        self.read_secs(list, &mut b)?;
        self.hotfix = (0..used).map(|i| (rd32(&b, i * 4), rd32(&b, (count + i) * 4))).collect();
        Ok(())
    }

    fn load_code_page(&mut self, sp: &[u8]) -> Result<()> {
        let dir = rd32(sp, sp::CODE_PAGE_DIR);
        if dir == 0 || rd32(sp, sp::CODE_PAGES) == 0 || dir >= self.total {
            return Ok(());
        }
        let d = self.read_sector(dir)?;
        if rd32(&d, 0) != CPDIR_MAGIC || rd32(&d, 4) == 0 {
            return Ok(());
        }
        let cp = rd16(&d, 16 + 2);
        let data = rd32(&d, 16 + 8);
        let index = rd16(&d, 16 + 12) as usize;
        if data == 0 || data >= self.total || index > 2 {
            return Ok(());
        }
        let c = self.read_sector(data)?;
        if rd32(&c, 0) != CPDATA_MAGIC {
            return Ok(());
        }
        let off = rd16(&c, 20 + index * 2) as usize + 6;
        if off + 128 <= SECTOR {
            self.case = CaseMap::from_high(cp, &c[off..off + 128]);
        }
        Ok(())
    }

    pub fn set_readonly(&mut self, ro: bool) {
        self.readonly = ro;
    }

    // ---- Sector I/O (with hot-fix remapping) ----

    fn map_sector(&self, s: u32) -> u32 {
        self.hotfix.iter().find(|h| h.0 == s).map_or(s, |h| h.1)
    }

    pub(crate) fn read_secs(&self, lba: u32, buf: &mut [u8]) -> Result<()> {
        let n = (buf.len() / SECTOR) as u64;
        if lba as u64 + n > self.total as u64 {
            return Err(Error::Corrupt);
        }
        if self.hotfix.is_empty() {
            return self.dev.read(lba as u64, buf);
        }
        for (i, chunk) in buf.chunks_mut(SECTOR).enumerate() {
            self.dev.read(self.map_sector(lba + i as u32) as u64, chunk)?;
        }
        Ok(())
    }

    pub(crate) fn write_secs(&self, lba: u32, buf: &[u8]) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        let n = (buf.len() / SECTOR) as u64;
        if lba as u64 + n > self.total as u64 {
            return Err(Error::Corrupt);
        }
        if self.hotfix.is_empty() {
            return self.dev.write(lba as u64, buf);
        }
        for (i, chunk) in buf.chunks(SECTOR).enumerate() {
            self.dev.write(self.map_sector(lba + i as u32) as u64, chunk)?;
        }
        Ok(())
    }

    pub(crate) fn read_sector(&self, lba: u32) -> Result<Vec<u8>> {
        let mut b = vec![0u8; SECTOR];
        self.read_secs(lba, &mut b)?;
        Ok(b)
    }

    pub(crate) fn read_fnode(&self, s: u32) -> Result<Vec<u8>> {
        let b = self.read_sector(s)?;
        if rd32(&b, 0) != FNODE_MAGIC {
            return Err(Error::Corrupt);
        }
        Ok(b)
    }

    pub(crate) fn check_writable(&self) -> Result<()> {
        if self.readonly { Err(Error::ReadOnly) } else { Ok(()) }
    }

    // ---- Lookup ----

    fn entry_of(&self, d: &Dirent, parent: u32) -> Entry {
        Entry {
            name: name::to_string(&d.name),
            attr: d.attr,
            size: d.size,
            fnode: d.fnode,
            parent,
            created: d.ctime,
            accessed: d.atime,
            modified: d.mtime,
            ea_size: d.ea_size,
        }
    }

    /// The root directory (its times live in the root's `..` entry).
    pub fn root_entry(&mut self) -> Result<Entry> {
        let dn = self.dir_root(self.root)?;
        let mut e = match self.find_dirent(dn, DOTDOT)? {
            Some((d, i)) => self.entry_of(&d.entries[i], self.root),
            None => Entry {
                name: String::new(),
                attr: 0,
                size: 0,
                fnode: self.root,
                parent: self.root,
                created: 0,
                accessed: 0,
                modified: 0,
                ea_size: 0,
            },
        };
        e.name.clear();
        e.attr = attr::DIRECTORY;
        e.fnode = self.root;
        e.size = 0;
        Ok(e)
    }

    /// Look up `name` in directory `dir`.
    pub fn find(&mut self, dir: &Entry, name: &str) -> Result<Option<Entry>> {
        if !dir.is_dir() {
            return Err(Error::NotADirectory);
        }
        let n = name::to_bytes(name).ok_or(Error::InvalidName)?;
        self.find_bytes(dir.fnode, &n)
    }

    fn find_bytes(&mut self, dir: u32, n: &[u8]) -> Result<Option<Entry>> {
        if !name::valid_name(n) {
            return Ok(None);
        }
        let dn = self.dir_root(dir)?;
        Ok(self.find_dirent(dn, n)?.map(|(d, i)| self.entry_of(&d.entries[i], dir)))
    }

    /// Look up a path; the empty path is the root directory.
    pub fn lookup(&mut self, path: &str) -> Result<Entry> {
        let parts = components(path)?;
        let mut stack = vec![self.root_entry()?];
        for (i, p) in parts.iter().enumerate() {
            let cur = stack.last().unwrap();
            if !cur.is_dir() {
                return Err(Error::PathNotFound);
            }
            if p == b".." {
                if stack.len() > 1 {
                    stack.pop();
                }
                continue;
            }
            let dir = cur.fnode;
            match self.find_bytes(dir, p)? {
                Some(e) => stack.push(e),
                None => return Err(if i + 1 == parts.len() { Error::NotFound } else { Error::PathNotFound }),
            }
        }
        Ok(stack.pop().unwrap())
    }

    /// Split a path into (parent directory, final component).
    fn parent(&mut self, path: &str) -> Result<(Entry, Vec<u8>)> {
        let parts = components(path)?;
        let Some((last, dirs)) = parts.split_last() else { return Err(Error::InvalidName) };
        if !name::valid_name(last) {
            return Err(Error::InvalidName);
        }
        let mut p = String::new();
        for d in dirs {
            p.push_str(&name::to_string(d));
            p.push('\\');
        }
        let parent = self.lookup(&p).map_err(|e| if e == Error::NotFound { Error::PathNotFound } else { e })?;
        if !parent.is_dir() {
            return Err(Error::PathNotFound);
        }
        Ok((parent, last.clone()))
    }

    /// All entries of a directory in collating order (without `..`).
    pub fn read_dir(&mut self, dir: &Entry) -> Result<Vec<Entry>> {
        if !dir.is_dir() {
            return Err(Error::NotADirectory);
        }
        let dn = self.dir_root(dir.fnode)?;
        let mut v = Vec::new();
        self.walk_dir(dn, &mut v, 0)?;
        Ok(v.iter().filter(|d| d.flags & de::SPECIAL == 0).map(|d| self.entry_of(d, dir.fnode)).collect())
    }

    // ---- Creating and removing ----

    fn new_dirent(&self, name: &[u8], attr_bits: u8, fnode: u32, now: u32) -> Dirent {
        let long = if name::is_83(name) { 0 } else { attr::LONG_NAME };
        Dirent {
            flags: 0,
            attr: attr_bits | long,
            fnode,
            mtime: now,
            size: 0,
            atime: now,
            ctime: now,
            ea_size: 0,
            acls: 0,
            code_page: 0,
            name: name.to_vec(),
            extra: Vec::new(),
            down: None,
        }
    }

    fn new_fnode(name: &[u8], up: u32, is_dir: bool) -> Vec<u8> {
        let mut f = vec![0u8; SECTOR];
        wr32(&mut f, 0, FNODE_MAGIC);
        set_fnode_name(&mut f, name);
        wr32(&mut f, fnode::UP, up);
        wr16(&mut f, fnode::EA_OFFSET, fnode::EA_AREA as u16);
        if is_dir {
            f[fnode::FLAGS] = fnode::DIR;
        }
        btree::init_header(&mut f, fnode::BTREE, bt::FNODE_LEAVES as u8, 0);
        f
    }

    /// Create an empty file (the archive attribute is added). Fails if it
    /// exists.
    pub fn create_file(&mut self, path: &str, attr_bits: u8, now: u32) -> Result<Entry> {
        self.check_writable()?;
        let (parent, name) = self.parent(path)?;
        if self.find_bytes(parent.fnode, &name)?.is_some() {
            return Err(Error::Exists);
        }
        let fno = self.alloc_sector(parent.fnode)?;
        let f = Self::new_fnode(&name, parent.fnode, false);
        let d = self.new_dirent(&name, (attr_bits & attr::CHANGEABLE) | attr::ARCHIVE, fno, now);
        if let Err(e) = self.write_secs(fno, &f).and_then(|_| self.insert_dirent(parent.fnode, d.clone())) {
            self.free_run(fno, 1)?;
            return Err(e);
        }
        Ok(self.entry_of(&d, parent.fnode))
    }

    pub fn mkdir(&mut self, path: &str, now: u32) -> Result<Entry> {
        self.check_writable()?;
        let (parent, name) = self.parent(path)?;
        if self.find_bytes(parent.fnode, &name)?.is_some() {
            return Err(Error::Exists);
        }
        let fno = self.alloc_sector(parent.fnode)?;
        let dno = match self.alloc_dnode(fno) {
            Ok(d) => d,
            Err(e) => {
                self.free_run(fno, 1)?;
                return Err(e);
            }
        };
        let mut f = Self::new_fnode(&name, parent.fnode, true);
        dir::set_dir_root_in(&mut f, dno);
        let mut dotdot = self.new_dirent(DOTDOT, attr::DIRECTORY, fno, now);
        dotdot.attr = attr::DIRECTORY;
        dotdot.flags = de::SPECIAL;
        let top = dir::Dnode::new_root(dno, fno, vec![dotdot]);
        let d = self.new_dirent(&name, attr::DIRECTORY, fno, now);
        let r = self
            .write_secs(fno, &f)
            .and_then(|_| self.write_dnode(&top))
            .and_then(|_| self.insert_dirent(parent.fnode, d.clone()));
        if let Err(e) = r {
            self.free_dnode(dno)?;
            self.free_run(fno, 1)?;
            return Err(e);
        }
        Ok(self.entry_of(&d, parent.fnode))
    }

    /// Release everything an F-node owns (data, ALSECs, EAs, ACLs) and the
    /// F-node itself.
    fn free_fnode(&mut self, fno: u32) -> Result<()> {
        let f = self.read_fnode(fno)?;
        if f[fnode::FLAGS] & fnode::DIR == 0 {
            let (ext, alsecs) = self.tree_of(&f, fnode::BTREE)?;
            for x in ext {
                self.free_run(x.disk, x.len)?;
            }
            for a in alsecs {
                self.free_run(a, 1)?;
            }
        }
        self.free_eas(&f)?;
        self.free_acls(&f)?;
        if self.cache.as_ref().is_some_and(|c| c.0 == fno) {
            self.cache = None;
        }
        self.free_run(fno, 1)
    }

    pub fn delete(&mut self, path: &str) -> Result<()> {
        self.check_writable()?;
        let e = self.lookup(path)?;
        if e.is_dir() {
            return Err(Error::IsADirectory);
        }
        if e.attr & attr::READONLY != 0 {
            return Err(Error::AccessDenied);
        }
        let name = name::to_bytes(&e.name).ok_or(Error::InvalidName)?;
        self.remove_dirent(e.parent, &name)?;
        self.free_fnode(e.fnode)
    }

    pub fn rmdir(&mut self, path: &str) -> Result<()> {
        self.check_writable()?;
        let e = self.lookup(path)?;
        if !e.is_dir() {
            return Err(Error::NotADirectory);
        }
        if e.fnode == self.root {
            return Err(Error::AccessDenied);
        }
        let dn = self.dir_root(e.fnode)?;
        let top = self.read_dnode(dn)?;
        let empty = top.leaf() && top.entries.iter().all(|d| d.is_end() || d.flags & de::SPECIAL != 0);
        if !empty {
            return Err(Error::DirectoryNotEmpty);
        }
        let name = name::to_bytes(&e.name).ok_or(Error::InvalidName)?;
        self.remove_dirent(e.parent, &name)?;
        self.free_dnode(dn)?;
        self.free_fnode(e.fnode)
    }

    /// Is directory F-node `dir` equal to or below `ancestor`?
    fn is_within(&self, mut dir: u32, ancestor: u32) -> Result<bool> {
        for _ in 0..1024 {
            if dir == ancestor {
                return Ok(true);
            }
            if dir == self.root {
                return Ok(false);
            }
            dir = rd32(&self.read_fnode(dir)?, fnode::UP);
        }
        Err(Error::Corrupt)
    }

    /// Rename or move within the volume.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.check_writable()?;
        let e = self.lookup(from)?;
        if e.fnode == self.root {
            return Err(Error::AccessDenied);
        }
        let old = name::to_bytes(&e.name).ok_or(Error::InvalidName)?;
        let (np, new) = self.parent(to)?;
        if e.is_dir() && self.is_within(np.fnode, e.fnode)? {
            return Err(Error::AccessDenied);
        }
        let old_dn = self.dir_root(e.parent)?;
        let (mut dn, i) = self.find_dirent(old_dn, &old)?.ok_or(Error::NotFound)?;
        if np.fnode == e.parent && self.case.eq(&old, &new) {
            // Only the case changes: same length, same position.
            dn.entries[i].name = new.clone();
            self.write_dnode(&dn)?;
        } else {
            if self.find_bytes(np.fnode, &new)?.is_some() {
                return Err(Error::Exists);
            }
            let mut d = dn.entries[i].clone();
            d.down = None;
            d.name = new.clone();
            d.attr = (d.attr & !attr::LONG_NAME) | if name::is_83(&new) { 0 } else { attr::LONG_NAME };
            self.insert_dirent(np.fnode, d)?;
            self.remove_dirent(e.parent, &old)?;
        }
        let mut f = self.read_fnode(e.fnode)?;
        set_fnode_name(&mut f, &new);
        wr32(&mut f, fnode::UP, np.fnode);
        self.write_secs(e.fnode, &f)
    }

    /// Write an entry's attributes, times and size back to its DIRENT
    /// (and the size to the F-node).
    pub fn update_entry(&mut self, e: &Entry) -> Result<()> {
        self.check_writable()?;
        let (mut dn, i) = self.locate(e)?;
        let d = &mut dn.entries[i];
        let keep = d.attr & (attr::DIRECTORY | attr::LONG_NAME);
        d.attr = keep | (e.attr & attr::CHANGEABLE);
        d.ctime = e.created;
        d.atime = e.accessed;
        d.mtime = e.modified;
        if !e.is_dir() {
            d.size = e.size;
        }
        self.write_dnode(&dn)?;
        if !e.is_dir() {
            let mut f = self.read_fnode(e.fnode)?;
            if rd32(&f, fnode::FILE_SIZE) != e.size {
                wr32(&mut f, fnode::FILE_SIZE, e.size);
                self.write_secs(e.fnode, &f)?;
            }
        }
        Ok(())
    }

    /// Find the DIRENT of `e`, even if it was renamed since `e` was read.
    pub(crate) fn locate(&mut self, e: &Entry) -> Result<(dir::Dnode, usize)> {
        if e.fnode == self.root {
            let dn = self.dir_root(self.root)?;
            return self.find_dirent(dn, DOTDOT)?.ok_or(Error::Corrupt);
        }
        let n = name::to_bytes(&e.name).ok_or(Error::InvalidName)?;
        let dn = self.dir_root(e.parent)?;
        if let Some((d, i)) = self.find_dirent(dn, &n)?
            && d.entries[i].fnode == e.fnode {
                return Ok((d, i));
            }
        let up = rd32(&self.read_fnode(e.fnode)?, fnode::UP);
        let dn = self.dir_root(up)?;
        self.find_by_fnode(dn, e.fnode, 0)?.ok_or(Error::NotFound)
    }

    // ---- Volume ----

    pub fn info(&self) -> Info {
        Info { total_sectors: self.total, free_sectors: self.free, serial: self.serial, sector_size: SECTOR as u32 }
    }

    /// Set the volume label (boot sector).
    pub fn set_label(&mut self, label: &str) -> Result<()> {
        self.check_writable()?;
        let mut l = [b' '; 11];
        for (d, s) in l.iter_mut().zip(label.bytes()) {
            *d = s.to_ascii_uppercase();
        }
        let mut bs = self.read_sector(0)?;
        bs[38] = 0x29;
        bs[43..54].copy_from_slice(&l);
        self.write_secs(0, &bs)?;
        self.label = l;
        Ok(())
    }
}

/// Store the (true length, first 15 bytes of the) name in an F-node.
fn set_fnode_name(f: &mut [u8], name: &[u8]) {
    f[fnode::NAME_LEN] = name.len() as u8;
    f[fnode::NAME..fnode::NAME + fnode::NAME_MAX].fill(0);
    let n = name.len().min(fnode::NAME_MAX);
    f[fnode::NAME..fnode::NAME + n].copy_from_slice(&name[..n]);
}
