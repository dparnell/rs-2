//! OS/2 extended attributes on FAT (see docs/formats/fat-ea.md).
//!
//! FAT12/16 use OS/2's own scheme: every EA set lives in the hidden system
//! file `EA DATA. SF` in the root directory, and a file's directory entry
//! holds a 16-bit handle into it (offset 0x14). The file starts with a
//! 512-byte header (signature `ED`, a table of 240 cluster bases, one per
//! group of 128 handles), followed by the handle tables (one 16-bit
//! cluster offset per handle, relative to its group's base), padded to a
//! cluster boundary. The EA sets follow, each starting on a cluster, kept
//! in handle-group order so a group's sets are contiguous: inserting or
//! removing a set moves later data by whole clusters, which is done by
//! relinking the cluster chain rather than by copying.
//!
//! FAT32 has no room for the handle, so it follows the Netlabs FAT32.IFS:
//! the EAs of `NAME` are a plain FEA list in the file `NAME EA. SF` (a long
//! name) in the same directory, and byte 0x0C of the entry says that the
//! file has EAs (0x40) or critical EAs (0x80).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::{attr, name, BlockDevice, Dir, Entry, Error, FatType, Result, Slot, Timestamp, Volume};

/// Name of the EA file of FAT12/16 volumes.
pub const EA_FILE_NAME: &str = "EA DATA. SF";
const EA_FILE_83: [u8; 11] = *b"EA DATA  SF";
/// Attributes OS/2 gives `EA DATA. SF`.
const EA_FILE_ATTR: u8 = attr::READONLY | attr::HIDDEN | attr::SYSTEM | attr::ARCHIVE;
/// FAT32: suffix of the per-file EA file.
pub const EA_SUFFIX: &str = " EA. SF";
/// FAT32 byte 0x0C flags.
pub const FAT32_HAS_EAS: u8 = 0x40;
pub const FAT32_CRITICAL_EAS: u8 = 0x80;
pub const FAT32_EA_MASK: u8 = 0xc0;

/// FEA flag: the EA is critical (NEEDEA).
pub const FEA_NEEDEA: u8 = 0x80;
/// Largest FEA list (cbList) of one file.
pub const MAX_FEALIST: usize = 0xffff;

const FILE_SIGNATURE: &[u8; 2] = b"ED";
const SET_SIGNATURE: &[u8; 2] = b"EA";
/// Size of the EA file header (signature, log, base table).
const HEADER: usize = 512;
const BASE_TABLE: usize = 32;
pub const GROUPS: usize = 240;
pub const PER_GROUP: usize = 128;
/// Handle table entry of an unused handle.
pub const UNUSED: u16 = 0xffff;
/// Offset mask of a handle table entry (the top bit is not an offset).
const OFFSET_MASK: u16 = 0x7fff;
/// Bytes before the FEA list in an EA set: signature, owner handle,
/// NEEDEA count, owner name (14), reserved (4).
pub const SET_HEADER: usize = 26;

/// One extended attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ea {
    /// FEA flags (0x80 = NEEDEA).
    pub flags: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// Size of the FEA list (cbList included) holding `eas`.
pub fn fealist_size(eas: &[Ea]) -> usize {
    4 + eas.iter().map(|e| 5 + e.name.len() + e.value.len()).sum::<usize>()
}

/// Encode an FEA list: cbList, then per EA flags, name length, value
/// length, name, NUL, value.
pub fn pack_fealist(eas: &[Ea]) -> Vec<u8> {
    let mut b = Vec::with_capacity(fealist_size(eas));
    b.extend_from_slice(&(fealist_size(eas) as u32).to_le_bytes());
    for e in eas {
        b.push(e.flags);
        b.push(e.name.len() as u8);
        b.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
        b.extend_from_slice(&e.name);
        b.push(0);
        b.extend_from_slice(&e.value);
    }
    b
}

/// Decode an FEA list (starting with cbList).
pub fn parse_fealist(b: &[u8]) -> Result<Vec<Ea>> {
    if b.len() < 4 {
        return Err(Error::EaCorrupt);
    }
    let cb = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    if cb < 4 || cb > b.len() {
        return Err(Error::EaCorrupt);
    }
    let mut v = Vec::new();
    let mut i = 4;
    while i < cb {
        if i + 4 > cb {
            return Err(Error::EaCorrupt);
        }
        let nl = b[i + 1] as usize;
        let vl = u16::from_le_bytes([b[i + 2], b[i + 3]]) as usize;
        let n = i + 4;
        let val = n + nl + 1;
        if val + vl > cb {
            return Err(Error::EaCorrupt);
        }
        v.push(Ea { flags: b[i], name: b[n..n + nl].to_vec(), value: b[val..val + vl].to_vec() });
        i = val + vl;
    }
    Ok(v)
}

/// True if `name` is a FAT32 EA file (`... EA. SF`).
pub fn is_ea_companion(name: &str) -> bool {
    name.len() > EA_SUFFIX.len()
        && name.is_char_boundary(name.len() - EA_SUFFIX.len())
        && name[name.len() - EA_SUFFIX.len()..].eq_ignore_ascii_case(EA_SUFFIX)
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

/// The header part of `EA DATA. SF` held in memory.
pub(crate) struct EaFile {
    pub entry: Entry,
    /// First 32 bytes (signature and recovery log fields).
    head: [u8; BASE_TABLE],
    /// Cluster base of each group of 128 handles.
    pub base: [u16; GROUPS],
    /// Cluster offset (relative to the group base) of each handle.
    pub offsets: Vec<u16>,
}

impl EaFile {
    /// Clusters before the first EA set (header and handle tables).
    fn header_clusters(&self) -> u32 {
        self.base[0] as u32
    }

    /// Cluster of handle `h`'s set, None if unused.
    pub fn location(&self, h: u16) -> Option<u32> {
        let off = *self.offsets.get(h as usize)?;
        if off == UNUSED {
            return None;
        }
        Some(self.base[h as usize / PER_GROUP] as u32 + (off & OFFSET_MASK) as u32)
    }
}

impl<D: BlockDevice> Volume<D> {
    /// Is `e` the volume's `EA DATA. SF`?
    pub(crate) fn is_ea_file(&self, e: &Entry) -> bool {
        self.fat_type != FatType::Fat32
            && e.slot.is_some_and(|s| s.dir == Dir::FixedRoot)
            && e.short.eq_ignore_ascii_case(EA_FILE_NAME)
    }

    /// The raw directory entry at `slot`.
    pub(crate) fn raw_at(&self, slot: Slot) -> Result<[u8; 32]> {
        let (lba, off) = self.entry_pos(slot.dir, slot.index)?.ok_or(Error::Corrupt)?;
        let mut sec = vec![0u8; self.bps as usize];
        self.read_sector(lba, &mut sec)?;
        let mut d = [0u8; 32];
        d.copy_from_slice(&sec[off..off + 32]);
        if d[0] == 0 || d[0] == 0xe5 || d[11] == attr::LFN {
            return Err(Error::NotFound);
        }
        Ok(d)
    }

    /// Patch the EA reference of the entry at `slot`.
    pub(crate) fn set_ea_ref(&mut self, slot: Slot, f: impl FnOnce(&mut [u8])) -> Result<()> {
        let (lba, off) = self.entry_pos(slot.dir, slot.index)?.ok_or(Error::Corrupt)?;
        let mut sec = vec![0u8; self.bps as usize];
        self.read_sector(lba, &mut sec)?;
        f(&mut sec[off..off + 32]);
        self.write_sector(lba, &sec)
    }

    /// The current state of `e` (which may be an older copy) as on disk.
    fn fresh(&self, e: &Entry) -> Result<Entry> {
        let slot = e.slot.ok_or(Error::AccessDenied)?;
        self.read_dir(slot.dir)?.into_iter().find(|x| x.slot == Some(slot)).ok_or(Error::NotFound)
    }

    /// EA handle of `e` (FAT12/16) as on disk.
    pub fn ea_handle_of(&self, e: &Entry) -> Result<u16> {
        match e.slot {
            None => Ok(0),
            Some(s) => Ok(rd16(&self.raw_at(s)?, 20)),
        }
    }

    /// Does `e` have EAs (and critical ones)? Answers from the directory
    /// entry alone where possible.
    pub fn has_eas(&self, e: &Entry) -> Result<bool> {
        let Some(slot) = e.slot else { return Ok(false) };
        let d = self.raw_at(slot)?;
        Ok(if self.fat_type == FatType::Fat32 { d[12] & FAT32_EA_MASK != 0 } else { rd16(&d, 20) != 0 })
    }

    /// All extended attributes of a file or directory (none for the root).
    pub fn read_eas(&self, e: &Entry) -> Result<Vec<Ea>> {
        let Some(slot) = e.slot else { return Ok(Vec::new()) };
        let d = self.raw_at(slot)?;
        if self.fat_type == FatType::Fat32 {
            if d[12] & FAT32_EA_MASK == 0 {
                return Ok(Vec::new());
            }
            let cur = self.fresh(e)?;
            let Some(c) = self.find_in(slot.dir, &companion_name(&cur.name))? else { return Ok(Vec::new()) };
            let mut buf = vec![0u8; c.size as usize];
            self.read(&c, 0, &mut buf)?;
            return parse_fealist(&buf);
        }
        let h = rd16(&d, 20);
        if h == 0 {
            return Ok(Vec::new());
        }
        let f = self.load_ea_file()?.ok_or(Error::EaCorrupt)?;
        let set = self.read_set(&f, h)?;
        parse_fealist(&set[SET_HEADER..])
    }

    /// Replace all EAs of `e` with `eas` (empty: remove them). The root
    /// directory has no directory entry and cannot hold EAs.
    pub fn write_eas(&mut self, e: &Entry, eas: &[Ea], now: Timestamp) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        let slot = e.slot.ok_or(Error::AccessDenied)?;
        if self.is_ea_file(e) {
            return Err(Error::AccessDenied);
        }
        if fealist_size(eas) > MAX_FEALIST {
            return Err(Error::EaTooLong);
        }
        let critical = eas.iter().any(|x| x.flags & FEA_NEEDEA != 0);
        if self.fat_type == FatType::Fat32 {
            let cur = self.fresh(e)?;
            let cname = companion_name(&cur.name);
            let existing = self.find_in(slot.dir, &cname)?;
            if eas.is_empty() {
                if let Some(c) = existing {
                    self.remove_plain(&c)?;
                }
                return self.set_ea_ref(slot, |d| d[12] &= !FAT32_EA_MASK);
            }
            let mut c = match existing {
                Some(c) => c,
                None => self.create_long(slot.dir, &cname, attr::HIDDEN | attr::SYSTEM | attr::ARCHIVE, now)?,
            };
            let data = pack_fealist(eas);
            self.set_size(&mut c, 0, now)?;
            self.write(&mut c, 0, &data, now)?;
            let flag = if critical { FAT32_CRITICAL_EAS } else { FAT32_HAS_EAS };
            return self.set_ea_ref(slot, |d| d[12] = (d[12] & !FAT32_EA_MASK) | flag);
        }
        let old = rd16(&self.raw_at(slot)?, 20);
        if eas.is_empty() {
            if old != 0 {
                if let Some(mut f) = self.load_ea_file()? {
                    if f.location(old).is_some() {
                        self.remove_set(&mut f, old, true, now)?;
                    }
                }
                self.set_ea_ref(slot, |d| d[20..22].fill(0))?;
            }
            return Ok(());
        }
        let mut f = match self.load_ea_file()? {
            Some(f) => f,
            None => self.create_ea_file(now)?,
        };
        let short = self.fresh(e)?.short;
        let h = match old {
            0 => self.alloc_handle(&mut f, now)?,
            h if (h as usize) < f.offsets.len() => h,
            // A handle beyond the tables: take a new one.
            _ => self.alloc_handle(&mut f, now)?,
        };
        let mut set = vec![0u8; SET_HEADER];
        set[0..2].copy_from_slice(SET_SIGNATURE);
        set[2..4].copy_from_slice(&h.to_le_bytes());
        let need = eas.iter().filter(|x| x.flags & FEA_NEEDEA != 0).count() as u32;
        set[4..8].copy_from_slice(&need.to_le_bytes());
        for (d, s) in set[8..21].iter_mut().zip(short.bytes()) {
            *d = s;
        }
        set.extend_from_slice(&pack_fealist(eas));
        self.store_set(&mut f, h, &set, now)?;
        if old != h {
            self.set_ea_ref(slot, |d| d[20..22].copy_from_slice(&h.to_le_bytes()))?;
        }
        Ok(())
    }

    /// Release the EAs of an entry about to be deleted.
    pub(crate) fn drop_eas(&mut self, e: &Entry) -> Result<()> {
        if !self.has_eas(e).unwrap_or(false) {
            return Ok(());
        }
        // A damaged EA file must not prevent deleting the file.
        match self.write_eas(e, &[], Timestamp::default()) {
            Ok(()) | Err(Error::EaCorrupt) => Ok(()),
            Err(x) => Err(x),
        }
    }

    /// Follow a rename of `old` to `new` (which may be in another
    /// directory): FAT32 moves the EA file, FAT12/16 updates the owner
    /// name in the EA set.
    pub(crate) fn eas_renamed(&mut self, old: &Entry, new: &Entry) -> Result<()> {
        let (Some(os), Some(ns)) = (old.slot, new.slot) else { return Ok(()) };
        if self.fat_type == FatType::Fat32 {
            if old.nt_flags & FAT32_EA_MASK == 0 {
                return Ok(());
            }
            let from = companion_name(&old.name);
            let to = companion_name(&new.name);
            if os.dir == ns.dir && from.eq_ignore_ascii_case(&to) {
                return Ok(());
            }
            let Some(c) = self.find_in(os.dir, &from)? else { return Ok(()) };
            let mut data = vec![0u8; c.size as usize];
            self.read(&c, 0, &mut data)?;
            if let Some(stale) = self.find_in(ns.dir, &to)? {
                self.remove_plain(&stale)?;
            }
            let mut nc = self.create_long(ns.dir, &to, c.attr, c.modified)?;
            self.write(&mut nc, 0, &data, c.modified)?;
            return self.remove_plain(&c);
        }
        let h = old.ea_handle;
        if h == 0 {
            return Ok(());
        }
        let Some(f) = self.load_ea_file()? else { return Ok(()) };
        let Some(loc) = f.location(h) else { return Ok(()) };
        let mut owner = [0u8; 14];
        for (d, s) in owner[..13].iter_mut().zip(new.short.bytes()) {
            *d = s;
        }
        let mut ef = f.entry.clone();
        let at = loc as u64 * self.cluster_size() as u64 + 8;
        self.write(&mut ef, at, &owner, f.entry.modified).map(|_| ())
    }

    /// Delete a plain file entry and its clusters (no EA handling).
    pub(crate) fn remove_plain(&mut self, e: &Entry) -> Result<()> {
        if e.cluster != 0 {
            self.free_chain(e.cluster)?;
        }
        self.mark_deleted(e.slot.ok_or(Error::Corrupt)?)
    }

    // ---- EA DATA. SF ----

    fn ea_file_entry(&self) -> Result<Option<Entry>> {
        Ok(self.read_dir(Dir::FixedRoot)?.into_iter().find(|e| e.short.eq_ignore_ascii_case(EA_FILE_NAME)))
    }

    /// Read the header and handle tables of `EA DATA. SF`, if present.
    pub(crate) fn load_ea_file(&self) -> Result<Option<EaFile>> {
        if self.fat_type == FatType::Fat32 {
            return Ok(None);
        }
        let Some(entry) = self.ea_file_entry()? else { return Ok(None) };
        let cs = self.cluster_size() as usize;
        if entry.size < HEADER as u32 || entry.size as usize % cs != 0 {
            return Err(Error::EaCorrupt);
        }
        let mut hdr = vec![0u8; HEADER];
        self.read(&entry, 0, &mut hdr)?;
        if &hdr[0..2] != FILE_SIGNATURE {
            return Err(Error::EaCorrupt);
        }
        let mut base = [0u16; GROUPS];
        for (g, b) in base.iter_mut().enumerate() {
            *b = rd16(&hdr, BASE_TABLE + 2 * g);
        }
        let clusters = entry.size / cs as u32;
        let first = base[0] as u32;
        if first == 0 || first > clusters || first as usize * cs < HEADER + 2 * PER_GROUP {
            return Err(Error::EaCorrupt);
        }
        let tables = ((first as usize * cs - HEADER) / (2 * PER_GROUP)).min(GROUPS);
        let mut raw = vec![0u8; tables * 2 * PER_GROUP];
        self.read(&entry, HEADER as u64, &mut raw)?;
        let offsets = (0..tables * PER_GROUP).map(|i| rd16(&raw, 2 * i)).collect();
        let mut head = [0u8; BASE_TABLE];
        head.copy_from_slice(&hdr[..BASE_TABLE]);
        Ok(Some(EaFile { entry, head, base, offsets }))
    }

    /// Create an empty `EA DATA. SF` in the root directory.
    fn create_ea_file(&mut self, now: Timestamp) -> Result<EaFile> {
        let slot = self.free_slot(Dir::FixedRoot)?;
        let mut e = Entry {
            name: String::from(EA_FILE_NAME),
            short: String::from(EA_FILE_NAME),
            attr: EA_FILE_ATTR,
            size: 0,
            cluster: 0,
            created: now,
            accessed: now.date,
            modified: now,
            ea_handle: 0,
            nt_flags: 0,
            slot: Some(slot),
        };
        let raw = self.raw_entry(&e, &EA_FILE_83);
        self.write_entry_raw(slot, &raw)?;
        let cs = self.cluster_size() as usize;
        let header_clusters = (HEADER + 2 * PER_GROUP).div_ceil(cs) as u32;
        self.insert_clusters(&mut e, 0, header_clusters)?;
        let tables = ((header_clusters as usize * cs - HEADER) / (2 * PER_GROUP)).min(GROUPS);
        let mut head = [0u8; BASE_TABLE];
        head[0..2].copy_from_slice(FILE_SIGNATURE);
        let mut f = EaFile { entry: e, head, base: [header_clusters as u16; GROUPS], offsets: vec![UNUSED; tables * PER_GROUP] };
        self.store_header(&mut f, now)?;
        Ok(f)
    }

    /// Write the header and handle tables back.
    pub(crate) fn store_header(&mut self, f: &mut EaFile, now: Timestamp) -> Result<()> {
        let mut b = vec![0u8; HEADER + f.offsets.len() * 2];
        b[..BASE_TABLE].copy_from_slice(&f.head);
        // No recovery log is pending after a completed update.
        b[4..22].fill(0);
        for (g, v) in f.base.iter().enumerate() {
            b[BASE_TABLE + 2 * g..BASE_TABLE + 2 * g + 2].copy_from_slice(&v.to_le_bytes());
        }
        for (i, v) in f.offsets.iter().enumerate() {
            b[HEADER + 2 * i..HEADER + 2 * i + 2].copy_from_slice(&v.to_le_bytes());
        }
        let mut e = f.entry.clone();
        self.write(&mut e, 0, &b, now)?;
        f.entry = e;
        Ok(())
    }

    /// The bytes of handle `h`'s EA set (header and FEA list).
    pub(crate) fn read_set(&self, f: &EaFile, h: u16) -> Result<Vec<u8>> {
        let loc = f.location(h).ok_or(Error::EaCorrupt)?;
        let cs = self.cluster_size() as u64;
        let at = loc as u64 * cs;
        let mut hdr = [0u8; SET_HEADER + 4];
        if at + hdr.len() as u64 > f.entry.size as u64 || self.read(&f.entry, at, &mut hdr)? != hdr.len() {
            return Err(Error::EaCorrupt);
        }
        if &hdr[0..2] != SET_SIGNATURE || rd16(&hdr, 2) != h {
            return Err(Error::EaCorrupt);
        }
        let cb = u32::from_le_bytes([hdr[26], hdr[27], hdr[28], hdr[29]]) as u64;
        let len = SET_HEADER as u64 + cb;
        if cb < 4 || cb > MAX_FEALIST as u64 || at + len > f.entry.size as u64 {
            return Err(Error::EaCorrupt);
        }
        let mut set = vec![0u8; len as usize];
        self.read(&f.entry, at, &mut set)?;
        Ok(set)
    }

    /// Clusters taken by handle `h`'s set.
    fn set_clusters(&self, f: &EaFile, h: u16) -> Result<u32> {
        let loc = f.location(h).ok_or(Error::EaCorrupt)?;
        let cs = self.cluster_size() as u64;
        let mut cb = [0u8; 4];
        self.read(&f.entry, loc as u64 * cs + SET_HEADER as u64, &mut cb)?;
        let len = SET_HEADER as u64 + u32::from_le_bytes(cb) as u64;
        Ok(len.div_ceil(cs) as u32)
    }

    /// First cluster after group `g`'s sets.
    fn group_end(&self, f: &EaFile, g: usize) -> u32 {
        let total = f.entry.size / self.cluster_size();
        let b = f.base[g] as u32;
        f.base[g + 1..].iter().map(|&x| x as u32).filter(|&x| x >= b).min().unwrap_or(total).min(total)
    }

    /// A free handle (never 0), growing the handle tables if needed.
    fn alloc_handle(&mut self, f: &mut EaFile, now: Timestamp) -> Result<u16> {
        loop {
            if let Some(h) = (1..f.offsets.len()).find(|&h| f.offsets[h] == UNUSED) {
                return Ok(h as u16);
            }
            if f.offsets.len() >= GROUPS * PER_GROUP {
                return Err(Error::EaTableFull);
            }
            // One more cluster of handle tables: everything after the
            // header moves up by a cluster.
            let at = f.header_clusters();
            let mut e = f.entry.clone();
            self.insert_clusters(&mut e, at, 1)?;
            f.entry = e;
            for b in f.base.iter_mut() {
                *b += 1;
            }
            let cs = self.cluster_size() as usize;
            let tables = (((at as usize + 1) * cs - HEADER) / (2 * PER_GROUP)).min(GROUPS);
            f.offsets.resize(tables * PER_GROUP, UNUSED);
            self.store_header(f, now)?;
        }
    }

    /// Store `set` as handle `h`'s EA set (replacing an old one).
    fn store_set(&mut self, f: &mut EaFile, h: u16, set: &[u8], now: Timestamp) -> Result<()> {
        let cs = self.cluster_size() as usize;
        let need = set.len().div_ceil(cs) as u32;
        let mut padded = set.to_vec();
        padded.resize(need as usize * cs, 0);
        if f.location(h).is_some() {
            if self.set_clusters(f, h)? == need {
                let at = f.location(h).unwrap() as u64 * cs as u64;
                let mut e = f.entry.clone();
                self.write(&mut e, at, &padded, now)?;
                f.entry = e;
                return Ok(());
            }
            self.remove_set(f, h, false, now)?;
        }
        let g = h as usize / PER_GROUP;
        let at = self.group_end(f, g);
        let mut e = f.entry.clone();
        self.insert_clusters(&mut e, at, need)?;
        let base_g = f.base[g] as u32;
        for k in 0..GROUPS {
            if k != g && f.base[k] as u32 >= at && (k > g || f.base[k] as u32 > base_g) {
                f.base[k] += need as u16;
            }
        }
        f.offsets[h as usize] = (at - base_g) as u16;
        self.write(&mut e, at as u64 * cs as u64, &padded, now)?;
        f.entry = e;
        self.store_header(f, now)
    }

    /// Remove handle `h`'s set; `free` also releases the handle (written
    /// to disk then; otherwise the caller stores a new set).
    pub(crate) fn remove_set(&mut self, f: &mut EaFile, h: u16, free: bool, now: Timestamp) -> Result<()> {
        let loc = f.location(h).ok_or(Error::EaCorrupt)?;
        let n = self.set_clusters(f, h)?;
        let g = h as usize / PER_GROUP;
        let mut e = f.entry.clone();
        self.remove_clusters(&mut e, loc, n)?;
        f.entry = e;
        for k in 0..GROUPS {
            if k != g && f.base[k] as u32 > loc {
                f.base[k] -= n as u16;
            }
        }
        let first = g * PER_GROUP;
        let base_g = f.base[g] as u32;
        for i in first..(first + PER_GROUP).min(f.offsets.len()) {
            let off = f.offsets[i];
            if off != UNUSED && base_g + (off & OFFSET_MASK) as u32 > loc {
                f.offsets[i] = off - n as u16;
            }
        }
        f.offsets[h as usize] = UNUSED;
        if free {
            self.store_header(f, now)?;
        }
        Ok(())
    }

    // ---- Cluster chain surgery ----

    /// Insert `n` zeroed clusters before cluster index `at` of a file.
    pub(crate) fn insert_clusters(&mut self, e: &mut Entry, at: u32, n: u32) -> Result<()> {
        if n == 0 {
            return Ok(());
        }
        let chain = if e.cluster == 0 { Vec::new() } else { self.chain(e.cluster)? };
        if at as usize > chain.len() {
            return Err(Error::Corrupt);
        }
        let mut first = 0;
        let mut last: Option<u32> = None;
        for _ in 0..n {
            let c = self.alloc_cluster(last)?;
            if last.is_none() {
                first = c;
            }
            last = Some(c);
        }
        let last = last.unwrap();
        if let Some(&next) = chain.get(at as usize) {
            self.set_fat_entry(last, next)?;
        }
        if at == 0 {
            e.cluster = first;
        } else {
            self.set_fat_entry(chain[at as usize - 1], first)?;
        }
        e.size += n * self.cluster_size();
        self.update_entry(e)
    }

    /// Remove `n` clusters starting at cluster index `at` of a file.
    pub(crate) fn remove_clusters(&mut self, e: &mut Entry, at: u32, n: u32) -> Result<()> {
        if n == 0 {
            return Ok(());
        }
        let chain = self.chain(e.cluster)?;
        let (at, n) = (at as usize, n as usize);
        if at + n > chain.len() {
            return Err(Error::Corrupt);
        }
        let after = chain.get(at + n).copied();
        if at == 0 {
            e.cluster = after.unwrap_or(0);
        } else {
            let v = after.unwrap_or(self.eoc());
            self.set_fat_entry(chain[at - 1], v)?;
        }
        for &c in &chain[at..at + n] {
            self.set_fat_entry(c, 0)?;
            if let Some(f) = self.free_count.as_mut() {
                *f += 1;
            }
        }
        e.size -= (n as u32) * self.cluster_size();
        self.update_entry(e)
    }

    // ---- Long names (FAT32 EA files) ----

    /// Create an empty file with long name `long` in `dir`: long-name
    /// entries and a unique `NAME~N.EXT` alias.
    pub(crate) fn create_long(&mut self, dir: Dir, long: &str, attrib: u8, now: Timestamp) -> Result<Entry> {
        let units: Vec<u16> = long.encode_utf16().collect();
        if units.is_empty() || units.len() > 255 {
            return Err(Error::InvalidName);
        }
        let existing = self.read_dir(dir)?;
        let short = alias(long, |s| existing.iter().any(|e| e.short.eq_ignore_ascii_case(s)))?;
        let name83 = name::pack_83(&short).ok_or(Error::InvalidName)?;
        let parts = units.len().div_ceil(13);
        let slot = self.free_slots(dir, parts as u32 + 1)?;
        let sum = lfn_checksum(&name83);
        for p in 0..parts {
            // On disk the last part comes first.
            let seq = (parts - p) as u8;
            let mut d = [0u8; 32];
            d[0] = seq | if p == 0 { 0x40 } else { 0 };
            d[11] = attr::LFN;
            d[13] = sum;
            let start = (seq as usize - 1) * 13;
            for (k, o) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().enumerate() {
                let i = start + k;
                let ch = match i.cmp(&units.len()) {
                    core::cmp::Ordering::Less => units[i],
                    core::cmp::Ordering::Equal => 0,
                    core::cmp::Ordering::Greater => 0xffff,
                };
                d[*o..*o + 2].copy_from_slice(&ch.to_le_bytes());
            }
            self.write_entry_raw(Slot { dir, index: slot.index + p as u32 }, &d)?;
        }
        let e = Entry {
            name: String::from(long),
            short,
            attr: attrib,
            size: 0,
            cluster: 0,
            created: now,
            accessed: now.date,
            modified: now,
            ea_handle: 0,
            nt_flags: 0,
            slot: Some(Slot { dir, index: slot.index + parts as u32 }),
        };
        self.write_entry(&e)?;
        Ok(e)
    }

    /// `n` consecutive free slots in `dir`, growing it if needed.
    fn free_slots(&mut self, dir: Dir, n: u32) -> Result<Slot> {
        let mut sec = vec![0u8; self.bps as usize];
        let mut run_start = 0;
        let mut run = 0;
        let mut index = 0;
        loop {
            match self.entry_pos(dir, index)? {
                Some((lba, off)) => {
                    self.read_sector(lba, &mut sec)?;
                    if sec[off] == 0 || sec[off] == 0xe5 {
                        if run == 0 {
                            run_start = index;
                        }
                        run += 1;
                        if run == n {
                            return Ok(Slot { dir, index: run_start });
                        }
                    } else {
                        run = 0;
                    }
                    index += 1;
                }
                None => {
                    let Dir::Cluster(start) = dir else { return Err(Error::DiskFull) };
                    let last = self.chain(start)?.last().copied();
                    self.alloc_cluster(last)?;
                }
            }
        }
    }
}

/// The FAT32 EA file name of `name`.
pub fn companion_name(name: &str) -> String {
    let mut s = String::from(name);
    s.push_str(EA_SUFFIX);
    s
}

/// Checksum of an 8.3 name stored in its long-name entries.
pub(crate) fn lfn_checksum(n: &[u8; 11]) -> u8 {
    n.iter().fold(0u8, |s, &c| (s >> 1 | (s & 1) << 7).wrapping_add(c))
}

/// A short alias `BASE~N.EXT` for a long name, unique per `taken`.
fn alias(long: &str, taken: impl Fn(&str) -> bool) -> Result<String> {
    let clean = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || "!#$%&'()-@^_`{}~".contains(*c))
            .map(|c| c.to_ascii_uppercase())
            .collect()
    };
    let (b, x) = match long.rfind('.') {
        Some(i) => (&long[..i], &long[i + 1..]),
        None => (long, ""),
    };
    let mut base = clean(b);
    if base.is_empty() {
        base = String::from("EA");
    }
    let ext: String = clean(x).chars().take(3).collect();
    for n in 1..1_000_000u32 {
        let tail = alloc::format!("~{n}");
        let keep = 8 - tail.len();
        let mut s: String = base.chars().take(keep).collect();
        s.push_str(&tail);
        if !ext.is_empty() {
            s.push('.');
            s.push_str(&ext);
        }
        if !taken(&s) {
            return Ok(s);
        }
    }
    Err(Error::Exists)
}
