//! FAT12/16/32 file system.
//!
//! Shared by the kernel (as its FAT file system driver) and the build tool
//! (to create boot volumes). Names follow OS/2 rules: directory entries are
//! created in 8.3 form, case-insensitive matching; VFAT long names written by
//! other systems are read so such volumes are usable.
//!
//! Paths are relative to the volume root, components separated by `\` or
//! `/`.

#![no_std]

extern crate alloc;

pub mod check;
pub mod ea;
mod name;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub use name::{to_83, valid_83};

/// Errors, mapped to OS/2 codes by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Io,
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
    /// `EA DATA. SF` (or an EA file) is damaged or does not match.
    EaCorrupt,
    /// No free EA handle is left.
    EaTableFull,
    /// The EA set is larger than 64 KB.
    EaTooLong,
}

pub type Result<T> = core::result::Result<T, Error>;

/// A sector-addressed device.
pub trait BlockDevice {
    fn sector_size(&self) -> usize;
    fn sector_count(&self) -> u64;
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<()>;
    fn write(&self, lba: u64, buf: &[u8]) -> Result<()>;
}

pub mod attr {
    pub const READONLY: u8 = 0x01;
    pub const HIDDEN: u8 = 0x02;
    pub const SYSTEM: u8 = 0x04;
    pub const VOLUME: u8 = 0x08;
    pub const DIRECTORY: u8 = 0x10;
    pub const ARCHIVE: u8 = 0x20;
    pub const LFN: u8 = 0x0f;
}

/// DOS packed date/time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Timestamp {
    pub date: u16,
    pub time: u16,
}

impl Timestamp {
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        let y = year.saturating_sub(1980).min(127);
        Timestamp {
            date: y << 9 | (month as u16) << 5 | day as u16,
            time: (hour as u16) << 11 | (minute as u16) << 5 | (second as u16 / 2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatType {
    Fat12,
    Fat16,
    Fat32,
}

/// Where a directory's entries live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// The fixed root directory of FAT12/16.
    FixedRoot,
    Cluster(u32),
}

/// Location of a directory entry (for updates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub dir: Dir,
    pub index: u32,
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Name as displayed: the long name if present, else `NAME.EXT`.
    pub name: String,
    /// The 8.3 name (`NAME.EXT`).
    pub short: String,
    pub attr: u8,
    pub size: u32,
    pub cluster: u32,
    pub created: Timestamp,
    pub accessed: u16,
    pub modified: Timestamp,
    /// OS/2 extended attribute handle (FAT12/16 only; offset 0x14).
    pub ea_handle: u16,
    /// Byte 0x0C: Windows NT lower-case flags (0x08, 0x10) and, on FAT32,
    /// the FAT32.IFS EA flags ([`ea::FAT32_HAS_EAS`]).
    pub nt_flags: u8,
    pub slot: Option<Slot>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.attr & attr::DIRECTORY != 0
    }

    fn root(cluster: u32) -> Entry {
        Entry {
            name: String::new(),
            short: String::new(),
            attr: attr::DIRECTORY,
            size: 0,
            cluster,
            created: Timestamp::default(),
            accessed: 0,
            modified: Timestamp::default(),
            ea_handle: 0,
            nt_flags: 0,
            slot: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Info {
    pub fat_type: FatType,
    pub bytes_per_cluster: u32,
    pub total_clusters: u32,
    pub free_clusters: u32,
    pub serial: u32,
    pub sector_size: u32,
}

pub struct Volume<D: BlockDevice> {
    pub dev: D,
    pub fat_type: FatType,
    bps: u32,
    spc: u32,
    reserved: u32,
    fats: u32,
    fat_size: u32,
    root_entries: u32,
    root_start: u32,
    root_sectors: u32,
    data_start: u32,
    clusters: u32,
    root_cluster: u32,
    pub serial: u32,
    pub label: [u8; 11],
    /// Next cluster to try when allocating.
    hint: u32,
    free_count: Option<u32>,
    readonly: bool,
    /// Recently read FAT sectors (lba, contents), most recent last: chain
    /// walks touch the same sector for many clusters in a row.
    fat_cache: core::cell::RefCell<Vec<(u64, Vec<u8>)>>,
}

/// FAT sectors kept in [`Volume::fat_cache`].
const FAT_CACHE: usize = 16;

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn split(path: &str) -> impl Iterator<Item = &str> {
    path.split(['\\', '/']).filter(|s| !s.is_empty() && *s != ".")
}

impl<D: BlockDevice> Volume<D> {
    pub fn mount(dev: D) -> Result<Self> {
        let mut bs = vec![0u8; dev.sector_size().max(512)];
        dev.read(0, &mut bs)?;
        if bs[510] != 0x55 || bs[511] != 0xaa {
            // Some media omit the signature; require a sane BPB instead.
            if rd16(&bs, 11) == 0 {
                return Err(Error::Corrupt);
            }
        }
        let bps = rd16(&bs, 11) as u32;
        let spc = bs[13] as u32;
        let reserved = rd16(&bs, 14) as u32;
        let fats = bs[16] as u32;
        let root_entries = rd16(&bs, 17) as u32;
        let total16 = rd16(&bs, 19) as u32;
        let fat16 = rd16(&bs, 22) as u32;
        let total32 = rd32(&bs, 32);
        if !matches!(bps, 512 | 1024 | 2048 | 4096) || spc == 0 || !spc.is_power_of_two() || fats == 0 {
            return Err(Error::Corrupt);
        }
        let fat_size = if fat16 != 0 { fat16 } else { rd32(&bs, 36) };
        let total = if total16 != 0 { total16 } else { total32 };
        let root_sectors = (root_entries * 32).div_ceil(bps);
        let root_start = reserved + fats * fat_size;
        let data_start = root_start + root_sectors;
        if total <= data_start {
            return Err(Error::Corrupt);
        }
        let clusters = (total - data_start) / spc;
        let fat_type = if clusters < 4085 {
            FatType::Fat12
        } else if clusters < 65525 {
            FatType::Fat16
        } else {
            FatType::Fat32
        };
        let (root_cluster, serial, label) = if fat_type == FatType::Fat32 {
            let mut l = [0u8; 11];
            l.copy_from_slice(&bs[71..82]);
            (rd32(&bs, 44), rd32(&bs, 67), l)
        } else {
            let mut l = [0u8; 11];
            l.copy_from_slice(&bs[43..54]);
            (0, rd32(&bs, 39), l)
        };
        let mut v = Volume {
            dev,
            fat_type,
            bps,
            spc,
            reserved,
            fats,
            fat_size,
            root_entries,
            root_start,
            root_sectors,
            data_start,
            clusters,
            root_cluster,
            serial,
            label,
            hint: 2,
            free_count: None,
            readonly: false,
            fat_cache: core::cell::RefCell::new(Vec::new()),
        };
        // The label proper is the root directory entry; the boot sector
        // holds a copy.
        if let Ok((_, Some(l))) = v.label_entry() {
            v.label = l;
        }
        Ok(v)
    }

    pub fn set_readonly(&mut self, ro: bool) {
        self.readonly = ro;
    }

    pub fn cluster_size(&self) -> u32 {
        self.bps * self.spc
    }

    fn cluster_lba(&self, c: u32) -> u64 {
        (self.data_start + (c - 2) * self.spc) as u64
    }

    fn in_fat(&self, lba: u64) -> bool {
        (self.reserved as u64..(self.reserved + self.fats * self.fat_size) as u64).contains(&lba)
    }

    fn read_sector(&self, lba: u64, buf: &mut [u8]) -> Result<()> {
        if buf.len() != self.bps as usize || !self.in_fat(lba) {
            return self.dev.read(lba, buf);
        }
        let mut cache = self.fat_cache.borrow_mut();
        if let Some(i) = cache.iter().position(|(l, _)| *l == lba) {
            let e = cache.remove(i);
            buf.copy_from_slice(&e.1);
            cache.push(e);
            return Ok(());
        }
        self.dev.read(lba, buf)?;
        if cache.len() >= FAT_CACHE {
            cache.remove(0);
        }
        cache.push((lba, buf.to_vec()));
        Ok(())
    }

    fn write_sector(&self, lba: u64, buf: &[u8]) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        // Keep cached FAT sectors in step (whole-sector writes only reach
        // the FAT; drop anything a larger write covers).
        let n = (buf.len() / self.bps as usize).max(1) as u64;
        let mut cache = self.fat_cache.borrow_mut();
        cache.retain_mut(|(l, data)| {
            if *l < lba || *l >= lba + n {
                return true;
            }
            if buf.len() == data.len() {
                data.copy_from_slice(buf);
                return true;
            }
            false
        });
        drop(cache);
        self.dev.write(lba, buf)
    }

    // ---- FAT access ----

    fn fat_entry(&self, c: u32) -> Result<u32> {
        let bps = self.bps;
        let mut sec = vec![0u8; bps as usize];
        match self.fat_type {
            FatType::Fat12 => {
                let off = c + c / 2;
                let lba = (self.reserved + off / bps) as u64;
                self.read_sector(lba, &mut sec)?;
                let i = (off % bps) as usize;
                let lo = sec[i];
                let hi = if i + 1 < bps as usize {
                    sec[i + 1]
                } else {
                    let mut next = vec![0u8; bps as usize];
                    self.read_sector(lba + 1, &mut next)?;
                    next[0]
                };
                let v = u16::from_le_bytes([lo, hi]) as u32;
                Ok(if c & 1 != 0 { v >> 4 } else { v & 0xfff })
            }
            FatType::Fat16 => {
                let off = c * 2;
                self.read_sector((self.reserved + off / bps) as u64, &mut sec)?;
                Ok(rd16(&sec, (off % bps) as usize) as u32)
            }
            FatType::Fat32 => {
                let off = c * 4;
                self.read_sector((self.reserved + off / bps) as u64, &mut sec)?;
                Ok(rd32(&sec, (off % bps) as usize) & 0x0fff_ffff)
            }
        }
    }

    fn set_fat_entry(&mut self, c: u32, v: u32) -> Result<()> {
        let bps = self.bps;
        let mut sec = vec![0u8; bps as usize];
        for copy in 0..self.fats {
            let base = self.reserved + copy * self.fat_size;
            match self.fat_type {
                FatType::Fat12 => {
                    let off = c + c / 2;
                    let lba = (base + off / bps) as u64;
                    let i = (off % bps) as usize;
                    self.read_sector(lba, &mut sec)?;
                    let mut next = vec![0u8; bps as usize];
                    let straddle = i + 1 >= bps as usize;
                    if straddle {
                        self.read_sector(lba + 1, &mut next)?;
                    }
                    let lo = sec[i];
                    let hi = if straddle { next[0] } else { sec[i + 1] };
                    let mut w = u16::from_le_bytes([lo, hi]);
                    if c & 1 != 0 {
                        w = (w & 0x000f) | ((v as u16) << 4);
                    } else {
                        w = (w & 0xf000) | (v as u16 & 0x0fff);
                    }
                    let [a, b] = w.to_le_bytes();
                    sec[i] = a;
                    if straddle {
                        next[0] = b;
                        self.write_sector(lba + 1, &next)?;
                    } else {
                        sec[i + 1] = b;
                    }
                    self.write_sector(lba, &sec)?;
                }
                FatType::Fat16 => {
                    let off = c * 2;
                    let lba = (base + off / bps) as u64;
                    self.read_sector(lba, &mut sec)?;
                    let i = (off % bps) as usize;
                    sec[i..i + 2].copy_from_slice(&(v as u16).to_le_bytes());
                    self.write_sector(lba, &sec)?;
                }
                FatType::Fat32 => {
                    let off = c * 4;
                    let lba = (base + off / bps) as u64;
                    self.read_sector(lba, &mut sec)?;
                    let i = (off % bps) as usize;
                    let old = rd32(&sec, i);
                    sec[i..i + 4].copy_from_slice(&((old & 0xf000_0000) | (v & 0x0fff_ffff)).to_le_bytes());
                    self.write_sector(lba, &sec)?;
                }
            }
        }
        Ok(())
    }

    fn eoc(&self) -> u32 {
        match self.fat_type {
            FatType::Fat12 => 0xfff,
            FatType::Fat16 => 0xffff,
            FatType::Fat32 => 0x0fff_ffff,
        }
    }

    fn is_eoc(&self, v: u32) -> bool {
        match self.fat_type {
            FatType::Fat12 => v >= 0xff8,
            FatType::Fat16 => v >= 0xfff8,
            FatType::Fat32 => v >= 0x0fff_fff8,
        }
    }

    /// Next cluster in a chain, None at the end.
    pub fn next_cluster(&self, c: u32) -> Result<Option<u32>> {
        let v = self.fat_entry(c)?;
        if self.is_eoc(v) {
            return Ok(None);
        }
        if v < 2 || v >= self.clusters + 2 {
            return Err(Error::Corrupt);
        }
        Ok(Some(v))
    }

    fn chain(&self, start: u32) -> Result<Vec<u32>> {
        let mut v = Vec::new();
        let mut c = start;
        while c >= 2 {
            v.push(c);
            if v.len() as u32 > self.clusters {
                return Err(Error::Corrupt);
            }
            match self.next_cluster(c)? {
                Some(n) => c = n,
                None => break,
            }
        }
        Ok(v)
    }

    /// Allocate a zeroed cluster and link it after `prev` (if any).
    fn alloc_cluster(&mut self, prev: Option<u32>) -> Result<u32> {
        let n = self.clusters;
        for i in 0..n {
            let c = 2 + (self.hint - 2 + i) % n;
            if self.fat_entry(c)? == 0 {
                self.set_fat_entry(c, self.eoc())?;
                if let Some(p) = prev {
                    self.set_fat_entry(p, c)?;
                }
                let zero = vec![0u8; self.bps as usize];
                for s in 0..self.spc as u64 {
                    self.write_sector(self.cluster_lba(c) + s, &zero)?;
                }
                self.hint = c + 1;
                if let Some(f) = self.free_count.as_mut() {
                    *f = f.saturating_sub(1);
                }
                return Ok(c);
            }
        }
        Err(Error::DiskFull)
    }

    fn free_chain(&mut self, start: u32) -> Result<()> {
        for c in self.chain(start)? {
            self.set_fat_entry(c, 0)?;
            if let Some(f) = self.free_count.as_mut() {
                *f += 1;
            }
        }
        Ok(())
    }

    /// Free clusters, from the first FAT read in large pieces (one request
    /// per 64 KB rather than one per cluster).
    fn count_free(&self) -> Result<u32> {
        let width = match self.fat_type {
            FatType::Fat12 => {
                let mut f = 0;
                for c in 2..self.clusters + 2 {
                    if self.fat_entry(c)? == 0 {
                        f += 1;
                    }
                }
                return Ok(f);
            }
            FatType::Fat16 => 2u64,
            FatType::Fat32 => 4u64,
        };
        let bps = self.bps as u64;
        let end = (self.clusters as u64 + 2) * width;
        let per = (65536 / bps).max(1);
        let mut buf = vec![0u8; (per * bps) as usize];
        let mut f = 0;
        let mut sector = 0u64;
        while sector * bps < end {
            let n = per.min((end - sector * bps).div_ceil(bps));
            let chunk = &mut buf[..(n * bps) as usize];
            self.read_sector(self.reserved as u64 + sector, chunk)?;
            let first = sector * bps;
            for (i, e) in chunk.chunks_exact(width as usize).enumerate() {
                let off = first + i as u64 * width;
                if off < 2 * width || off >= end {
                    continue;
                }
                let v = if width == 2 { rd16(e, 0) as u32 } else { rd32(e, 0) & 0x0fff_ffff };
                if v == 0 {
                    f += 1;
                }
            }
            sector += n;
        }
        Ok(f)
    }

    pub fn info(&mut self) -> Result<Info> {
        let free = match self.free_count {
            Some(f) => f,
            None => {
                let f = self.count_free()?;
                self.free_count = Some(f);
                f
            }
        };
        Ok(Info {
            fat_type: self.fat_type,
            bytes_per_cluster: self.cluster_size(),
            total_clusters: self.clusters,
            free_clusters: free,
            serial: self.serial,
            sector_size: self.bps,
        })
    }

    // ---- Directories ----

    pub fn root_dir(&self) -> Dir {
        if self.fat_type == FatType::Fat32 { Dir::Cluster(self.root_cluster) } else { Dir::FixedRoot }
    }

    pub fn root_entry(&self) -> Entry {
        Entry::root(if self.fat_type == FatType::Fat32 { self.root_cluster } else { 0 })
    }

    fn dir_of(&self, e: &Entry) -> Dir {
        if e.cluster == 0 { self.root_dir() } else { Dir::Cluster(e.cluster) }
    }

    /// Sector LBA and byte offset of entry `index` in `dir`, None past the
    /// end of the directory.
    fn entry_pos(&self, dir: Dir, index: u32) -> Result<Option<(u64, usize)>> {
        let per_sector = self.bps / 32;
        match dir {
            Dir::FixedRoot => {
                if index >= self.root_entries {
                    return Ok(None);
                }
                Ok(Some(((self.root_start + index / per_sector) as u64, (index % per_sector * 32) as usize)))
            }
            Dir::Cluster(start) => {
                let per_cluster = per_sector * self.spc;
                let mut c = start;
                for _ in 0..index / per_cluster {
                    match self.next_cluster(c)? {
                        Some(n) => c = n,
                        None => return Ok(None),
                    }
                }
                let within = index % per_cluster;
                Ok(Some((self.cluster_lba(c) + (within / per_sector) as u64, (within % per_sector * 32) as usize)))
            }
        }
    }

    /// Read all live entries of a directory (excluding `.`, `..` and the
    /// volume label).
    pub fn read_dir(&self, dir: Dir) -> Result<Vec<Entry>> {
        let mut out = Vec::new();
        let mut lfn: Vec<u16> = Vec::new();
        // (sequence number, checksum, characters)
        let mut lfn_parts: Vec<(u8, u8, [u16; 13])> = Vec::new();
        let mut sec = vec![0u8; self.bps as usize];
        let mut cur_lba = u64::MAX;
        let mut index = 0;
        loop {
            let Some((lba, off)) = self.entry_pos(dir, index)? else { break };
            if lba != cur_lba {
                self.read_sector(lba, &mut sec)?;
                cur_lba = lba;
            }
            let d = &sec[off..off + 32];
            index += 1;
            if d[0] == 0 {
                break;
            }
            if d[0] == 0xe5 {
                lfn_parts.clear();
                continue;
            }
            if d[11] == attr::LFN {
                let mut chars = [0u16; 13];
                for (k, o) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().enumerate() {
                    chars[k] = rd16(d, *o);
                }
                lfn_parts.push((d[0] & 0x1f, d[13], chars));
                continue;
            }
            if d[11] & attr::VOLUME != 0 {
                lfn_parts.clear();
                continue;
            }
            let short = name::from_83(&d[0..11]);
            if short == "." || short == ".." {
                lfn_parts.clear();
                continue;
            }
            lfn.clear();
            // Long-name parts left over from an entry rewritten in place
            // (or by a system without long names) do not match.
            let mut n83 = [0u8; 11];
            n83.copy_from_slice(&d[0..11]);
            let sum = ea::lfn_checksum(&n83);
            if lfn_parts.iter().any(|p| p.1 != sum) {
                lfn_parts.clear();
            }
            lfn_parts.sort_by_key(|p| p.0);
            for (_, _, chars) in &lfn_parts {
                for &ch in chars {
                    if ch == 0 || ch == 0xffff {
                        break;
                    }
                    lfn.push(ch);
                }
            }
            lfn_parts.clear();
            let long: String = char::decode_utf16(lfn.iter().copied()).map(|r| r.unwrap_or('?')).collect();
            let hi = rd16(d, 20) as u32;
            let cluster = rd16(d, 26) as u32 | if self.fat_type == FatType::Fat32 { hi << 16 } else { 0 };
            out.push(Entry {
                name: if long.is_empty() { short.clone() } else { long },
                short,
                attr: d[11],
                size: rd32(d, 28),
                cluster,
                created: Timestamp { time: rd16(d, 14), date: rd16(d, 16) },
                accessed: rd16(d, 18),
                modified: Timestamp { time: rd16(d, 22), date: rd16(d, 24) },
                ea_handle: if self.fat_type == FatType::Fat32 { 0 } else { hi as u16 },
                nt_flags: d[12],
                slot: Some(Slot { dir, index: index - 1 }),
            });
        }
        Ok(out)
    }

    fn find_in(&self, dir: Dir, component: &str) -> Result<Option<Entry>> {
        let want83 = to_83(component);
        for e in self.read_dir(dir)? {
            if e.name.eq_ignore_ascii_case(component)
                || e.short.eq_ignore_ascii_case(component)
                || want83.as_deref().is_some_and(|w| e.short.eq_ignore_ascii_case(w))
            {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }

    /// Look up a path; the empty path is the root directory.
    pub fn lookup(&self, path: &str) -> Result<Entry> {
        let mut cur = self.root_entry();
        let parts: Vec<&str> = split(path).collect();
        for (i, p) in parts.iter().enumerate() {
            if !cur.is_dir() {
                return Err(Error::PathNotFound);
            }
            if *p == ".." {
                // Resolve via the parent directory's ".." entry.
                cur = self.parent_of(&cur)?;
                continue;
            }
            match self.find_in(self.dir_of(&cur), p)? {
                Some(e) => cur = e,
                None => return Err(if i + 1 == parts.len() { Error::NotFound } else { Error::PathNotFound }),
            }
        }
        Ok(cur)
    }

    fn parent_of(&self, dir: &Entry) -> Result<Entry> {
        if dir.cluster == 0 || dir.cluster == self.root_cluster {
            return Ok(self.root_entry());
        }
        let Some((lba, off)) = self.entry_pos(Dir::Cluster(dir.cluster), 1)? else { return Err(Error::Corrupt) };
        let mut sec = vec![0u8; self.bps as usize];
        self.read_sector(lba, &mut sec)?;
        let d = &sec[off..off + 32];
        let c = rd16(d, 26) as u32 | if self.fat_type == FatType::Fat32 { (rd16(d, 20) as u32) << 16 } else { 0 };
        if c == 0 {
            return Ok(self.root_entry());
        }
        Ok(Entry::root(c))
    }

    /// Split a path into (parent directory entry, final component).
    fn parent(&self, path: &str) -> Result<(Entry, String)> {
        let parts: Vec<&str> = split(path).collect();
        let Some((last, dirs)) = parts.split_last() else { return Err(Error::InvalidName) };
        let mut p = String::new();
        for d in dirs {
            p.push_str(d);
            p.push('\\');
        }
        let parent = self.lookup(&p).map_err(|e| if e == Error::NotFound { Error::PathNotFound } else { e })?;
        if !parent.is_dir() {
            return Err(Error::PathNotFound);
        }
        Ok((parent, String::from(*last)))
    }

    fn write_entry_raw(&mut self, slot: Slot, raw: &[u8; 32]) -> Result<()> {
        let (lba, off) = self.entry_pos(slot.dir, slot.index)?.ok_or(Error::Corrupt)?;
        let mut sec = vec![0u8; self.bps as usize];
        self.read_sector(lba, &mut sec)?;
        sec[off..off + 32].copy_from_slice(raw);
        self.write_sector(lba, &sec)
    }

    fn raw_entry(&self, e: &Entry, name83: &[u8; 11]) -> [u8; 32] {
        let mut d = [0u8; 32];
        d[0..11].copy_from_slice(name83);
        d[11] = e.attr;
        d[12] = e.nt_flags;
        d[14..16].copy_from_slice(&e.created.time.to_le_bytes());
        d[16..18].copy_from_slice(&e.created.date.to_le_bytes());
        d[18..20].copy_from_slice(&e.accessed.to_le_bytes());
        let hi = if self.fat_type == FatType::Fat32 { (e.cluster >> 16) as u16 } else { e.ea_handle };
        d[20..22].copy_from_slice(&hi.to_le_bytes());
        d[22..24].copy_from_slice(&e.modified.time.to_le_bytes());
        d[24..26].copy_from_slice(&e.modified.date.to_le_bytes());
        d[26..28].copy_from_slice(&(e.cluster as u16).to_le_bytes());
        d[28..32].copy_from_slice(&e.size.to_le_bytes());
        d
    }

    /// Rewrite an entry's metadata (size, cluster, attributes, times).
    /// The EA reference on disk (handle or FAT32 flags) is kept: it belongs
    /// to the EA code, and `e` may be an older copy.
    pub fn update_entry(&mut self, e: &Entry) -> Result<()> {
        let Some(slot) = e.slot else { return Ok(()) };
        let name83 = name::pack_83(&e.short).ok_or(Error::InvalidName)?;
        let mut raw = self.raw_entry(e, &name83);
        let (lba, off) = self.entry_pos(slot.dir, slot.index)?.ok_or(Error::Corrupt)?;
        let mut sec = vec![0u8; self.bps as usize];
        self.read_sector(lba, &mut sec)?;
        let d = &sec[off..off + 32];
        if d[0] != 0 && d[0] != 0xe5 && d[11] != attr::LFN {
            if self.fat_type == FatType::Fat32 {
                raw[12] = (raw[12] & !ea::FAT32_EA_MASK) | (d[12] & ea::FAT32_EA_MASK);
            } else {
                raw[20..22].copy_from_slice(&d[20..22]);
            }
        }
        sec[off..off + 32].copy_from_slice(&raw);
        self.write_sector(lba, &sec)
    }

    /// Write an entry exactly as given (EA reference included).
    fn write_entry(&mut self, e: &Entry) -> Result<()> {
        let Some(slot) = e.slot else { return Ok(()) };
        let name83 = name::pack_83(&e.short).ok_or(Error::InvalidName)?;
        let raw = self.raw_entry(e, &name83);
        self.write_entry_raw(slot, &raw)
    }

    /// Find a free slot in a directory, growing it if needed.
    fn free_slot(&mut self, dir: Dir) -> Result<Slot> {
        let mut sec = vec![0u8; self.bps as usize];
        let mut index = 0;
        let mut last_cluster = match dir {
            Dir::Cluster(c) => Some(c),
            Dir::FixedRoot => None,
        };
        loop {
            match self.entry_pos(dir, index)? {
                Some((lba, off)) => {
                    self.read_sector(lba, &mut sec)?;
                    if sec[off] == 0 || sec[off] == 0xe5 {
                        return Ok(Slot { dir, index });
                    }
                    index += 1;
                }
                None => {
                    let Dir::Cluster(start) = dir else { return Err(Error::DiskFull) };
                    // Find the last cluster and extend.
                    let chain = self.chain(start)?;
                    last_cluster = chain.last().copied().or(last_cluster);
                    self.alloc_cluster(last_cluster)?;
                    return Ok(Slot { dir, index });
                }
            }
        }
    }

    fn create_entry(&mut self, path: &str, attr: u8, now: Timestamp) -> Result<Entry> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        let (parent, name) = self.parent(path)?;
        let short = to_83(&name).ok_or(Error::InvalidName)?;
        let pdir = self.dir_of(&parent);
        if self.find_in(pdir, &name)?.is_some() {
            return Err(Error::Exists);
        }
        let slot = self.free_slot(pdir)?;
        let e = Entry {
            name: short.clone(),
            short,
            attr,
            size: 0,
            cluster: 0,
            created: now,
            accessed: now.date,
            modified: now,
            ea_handle: 0,
            nt_flags: 0,
            slot: Some(slot),
        };
        self.write_entry(&e)?;
        Ok(e)
    }

    /// Create an empty file. Fails if it exists.
    pub fn create_file(&mut self, path: &str, attr: u8, now: Timestamp) -> Result<Entry> {
        self.create_entry(path, attr | attr::ARCHIVE, now)
    }

    pub fn mkdir(&mut self, path: &str, now: Timestamp) -> Result<Entry> {
        let (parent, _) = self.parent(path)?;
        let mut e = self.create_entry(path, attr::DIRECTORY, now)?;
        let c = self.alloc_cluster(None)?;
        e.cluster = c;
        self.update_entry(&e)?;
        // "." and ".."
        let mut dot = e.clone();
        dot.slot = Some(Slot { dir: Dir::Cluster(c), index: 0 });
        let raw = self.raw_entry(&dot, b".          ");
        self.write_entry_raw(Slot { dir: Dir::Cluster(c), index: 0 }, &raw)?;
        let mut dotdot = parent.clone();
        if parent.cluster == self.root_cluster {
            dotdot.cluster = 0;
        }
        dotdot.attr = attr::DIRECTORY;
        dotdot.created = now;
        dotdot.modified = now;
        dotdot.size = 0;
        dotdot.ea_handle = 0;
        dotdot.nt_flags = 0;
        let raw = self.raw_entry(&dotdot, b"..         ");
        self.write_entry_raw(Slot { dir: Dir::Cluster(c), index: 1 }, &raw)?;
        Ok(e)
    }

    fn mark_deleted(&mut self, slot: Slot) -> Result<()> {
        let mut sec = vec![0u8; self.bps as usize];
        let mut index = slot.index;
        let mut first = true;
        // The entry, then the long-name entries before it.
        loop {
            let (lba, off) = self.entry_pos(slot.dir, index)?.ok_or(Error::Corrupt)?;
            self.read_sector(lba, &mut sec)?;
            if !first && (sec[off + 11] != attr::LFN || sec[off] == 0xe5 || sec[off] == 0) {
                break;
            }
            // The last long-name part (the first on disk) carries 0x40.
            let last_part = !first && sec[off] & 0x40 != 0;
            sec[off] = 0xe5;
            self.write_sector(lba, &sec)?;
            first = false;
            if index == 0 || last_part {
                break;
            }
            index -= 1;
        }
        Ok(())
    }

    pub fn delete(&mut self, path: &str) -> Result<()> {
        let e = self.lookup(path)?;
        if e.is_dir() {
            return Err(Error::IsADirectory);
        }
        if e.attr & attr::READONLY != 0 || self.is_ea_file(&e) {
            return Err(Error::AccessDenied);
        }
        let slot = e.slot.ok_or(Error::AccessDenied)?;
        self.drop_eas(&e)?;
        if e.cluster != 0 {
            self.free_chain(e.cluster)?;
        }
        self.mark_deleted(slot)
    }

    pub fn rmdir(&mut self, path: &str) -> Result<()> {
        let e = self.lookup(path)?;
        if !e.is_dir() {
            return Err(Error::NotADirectory);
        }
        let slot = e.slot.ok_or(Error::AccessDenied)?;
        if !self.read_dir(Dir::Cluster(e.cluster))?.is_empty() {
            return Err(Error::DirectoryNotEmpty);
        }
        self.drop_eas(&e)?;
        self.free_chain(e.cluster)?;
        self.mark_deleted(slot)
    }

    /// Rename or move within the volume.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        let e = self.lookup(from)?;
        let old_slot = e.slot.ok_or(Error::AccessDenied)?;
        if self.is_ea_file(&e) {
            return Err(Error::AccessDenied);
        }
        let (parent, name) = self.parent(to)?;
        let short = to_83(&name).ok_or(Error::InvalidName)?;
        let pdir = self.dir_of(&parent);
        if let Some(existing) = self.find_in(pdir, &name)? {
            if existing.slot != Some(old_slot) {
                return Err(Error::Exists);
            }
        }
        let same_dir = old_slot.dir == pdir;
        let mut ne = e.clone();
        ne.short = short.clone();
        ne.name = short;
        if same_dir {
            // Overwrite in place (drops any long name).
            self.update_entry(&ne)?;
            return self.eas_renamed(&e, &ne);
        }
        let slot = self.free_slot(pdir)?;
        ne.slot = Some(slot);
        self.write_entry(&ne)?;
        self.mark_deleted(old_slot)?;
        self.eas_renamed(&e, &ne)?;
        if ne.is_dir() {
            // Fix "..".
            let raw_parent = if parent.cluster == self.root_cluster { 0 } else { parent.cluster };
            let (lba, off) = self.entry_pos(Dir::Cluster(ne.cluster), 1)?.ok_or(Error::Corrupt)?;
            let mut sec = vec![0u8; self.bps as usize];
            self.read_sector(lba, &mut sec)?;
            sec[off + 26..off + 28].copy_from_slice(&(raw_parent as u16).to_le_bytes());
            if self.fat_type == FatType::Fat32 {
                sec[off + 20..off + 22].copy_from_slice(&((raw_parent >> 16) as u16).to_le_bytes());
            }
            self.write_sector(lba, &sec)?;
        }
        Ok(())
    }

    // ---- File data ----

    /// Read file data at `offset`; returns bytes read.
    pub fn read(&self, e: &Entry, offset: u64, buf: &mut [u8]) -> Result<usize> {
        if offset >= e.size as u64 {
            return Ok(0);
        }
        let len = buf.len().min((e.size as u64 - offset) as usize);
        let cs = self.cluster_size() as u64;
        let mut c = e.cluster;
        for _ in 0..offset / cs {
            c = self.next_cluster(c)?.ok_or(Error::Corrupt)?;
        }
        let mut done = 0usize;
        let mut pos = offset;
        let mut sec = vec![0u8; self.bps as usize];
        while done < len {
            let in_cluster = pos % cs;
            let s = in_cluster / self.bps as u64;
            let o = (in_cluster % self.bps as u64) as usize;
            let n = (self.bps as usize - o).min(len - done);
            self.read_sector(self.cluster_lba(c) + s, &mut sec)?;
            buf[done..done + n].copy_from_slice(&sec[o..o + n]);
            done += n;
            pos += n as u64;
            if pos % cs == 0 && done < len {
                c = self.next_cluster(c)?.ok_or(Error::Corrupt)?;
            }
        }
        Ok(done)
    }

    /// Write file data at `offset`, extending the file. Updates `e`.
    pub fn write(&mut self, e: &mut Entry, offset: u64, data: &[u8], now: Timestamp) -> Result<usize> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset + data.len() as u64;
        if end > u32::MAX as u64 {
            return Err(Error::DiskFull);
        }
        self.ensure_clusters(e, end)?;
        let cs = self.cluster_size() as u64;
        let mut c = e.cluster;
        for _ in 0..offset / cs {
            c = self.next_cluster(c)?.ok_or(Error::Corrupt)?;
        }
        let mut done = 0usize;
        let mut pos = offset;
        let mut sec = vec![0u8; self.bps as usize];
        while done < data.len() {
            let in_cluster = pos % cs;
            let s = in_cluster / self.bps as u64;
            let o = (in_cluster % self.bps as u64) as usize;
            let n = (self.bps as usize - o).min(data.len() - done);
            let lba = self.cluster_lba(c) + s;
            if n != self.bps as usize {
                self.read_sector(lba, &mut sec)?;
            }
            sec[o..o + n].copy_from_slice(&data[done..done + n]);
            self.write_sector(lba, &sec)?;
            done += n;
            pos += n as u64;
            if pos % cs == 0 && done < data.len() {
                c = self.next_cluster(c)?.ok_or(Error::Corrupt)?;
            }
        }
        if end > e.size as u64 {
            e.size = end as u32;
        }
        e.modified = now;
        e.attr |= attr::ARCHIVE;
        self.update_entry(e)?;
        Ok(done)
    }

    /// Make sure the chain covers `len` bytes.
    fn ensure_clusters(&mut self, e: &mut Entry, len: u64) -> Result<()> {
        let cs = self.cluster_size() as u64;
        let need = len.div_ceil(cs) as usize;
        let chain = if e.cluster == 0 { Vec::new() } else { self.chain(e.cluster)? };
        let mut last = chain.last().copied();
        for _ in chain.len()..need {
            let c = self.alloc_cluster(last)?;
            if e.cluster == 0 {
                e.cluster = c;
            }
            last = Some(c);
        }
        Ok(())
    }

    /// Change the file size (truncate or extend with zeros).
    pub fn set_size(&mut self, e: &mut Entry, len: u32, now: Timestamp) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        let cs = self.cluster_size();
        if len > e.size {
            self.ensure_clusters(e, len as u64)?;
            // Zero the tail of the old last cluster beyond the old size.
            let old = e.size;
            let zeros = vec![0u8; ((len - old) as usize).min(cs as usize)];
            if !zeros.is_empty() && old % cs != 0 {
                let n = ((cs - old % cs) as usize).min((len - old) as usize);
                let mut tmp = e.clone();
                tmp.size = len;
                self.write(&mut tmp, old as u64, &zeros[..n], now)?;
            }
        } else if e.cluster != 0 {
            let keep = len.div_ceil(cs) as usize;
            let chain = self.chain(e.cluster)?;
            if keep == 0 {
                self.free_chain(e.cluster)?;
                e.cluster = 0;
            } else if keep < chain.len() {
                self.free_chain(chain[keep])?;
                self.set_fat_entry(chain[keep - 1], self.eoc())?;
            }
        }
        e.size = len;
        e.modified = now;
        self.update_entry(e)
    }

    /// The root directory's volume label entry: (index of the first free
    /// or label slot, label if present).
    fn label_entry(&self) -> Result<(Option<u32>, Option<[u8; 11]>)> {
        let dir = self.root_dir();
        let mut sec = vec![0u8; self.bps as usize];
        let mut cur = u64::MAX;
        let mut free = None;
        let mut index = 0;
        while let Some((lba, off)) = self.entry_pos(dir, index)? {
            if lba != cur {
                self.read_sector(lba, &mut sec)?;
                cur = lba;
            }
            let d = &sec[off..off + 32];
            if d[0] == 0 {
                return Ok((free.or(Some(index)), None));
            }
            if d[0] == 0xe5 {
                free = free.or(Some(index));
            } else if d[11] != attr::LFN && d[11] & attr::VOLUME != 0 {
                let mut l = [0u8; 11];
                l.copy_from_slice(&d[0..11]);
                return Ok((Some(index), Some(l)));
            }
            index += 1;
        }
        Ok((free, None))
    }

    /// Set the volume label (root directory entry and boot sector); an
    /// empty label removes it.
    pub fn set_label(&mut self, label: &str) -> Result<()> {
        if self.readonly {
            return Err(Error::ReadOnly);
        }
        let mut l = [b' '; 11];
        for (d, s) in l.iter_mut().zip(label.bytes()) {
            *d = s.to_ascii_uppercase();
        }
        let (slot, _) = self.label_entry()?;
        let dir = self.root_dir();
        if label.is_empty() {
            if let (Some(i), Some(_)) = self.label_entry()? {
                self.mark_deleted(Slot { dir, index: i })?;
            }
        } else {
            let slot = match slot {
                Some(i) => Slot { dir, index: i },
                None => self.free_slot(dir)?,
            };
            let mut raw = [0u8; 32];
            raw[0..11].copy_from_slice(&l);
            raw[11] = attr::VOLUME;
            self.write_entry_raw(slot, &raw)?;
        }
        self.label = l;
        let mut bs = vec![0u8; self.bps as usize];
        self.read_sector(0, &mut bs)?;
        let at = if self.fat_type == FatType::Fat32 { 71 } else { 43 };
        bs[at..at + 11].copy_from_slice(if label.is_empty() { b"NO NAME    " } else { &l });
        self.write_sector(0, &bs)
    }
}

/// Parameters for [`format`].
pub struct FormatParams<'a> {
    pub label: &'a str,
    pub serial: u32,
    /// Force a FAT type; None picks by size.
    pub fat_type: Option<FatType>,
    /// Sectors before the volume (partition offset), recorded in the BPB.
    pub hidden_sectors: u32,
}

/// Create an empty FAT file system on `dev`.
pub fn format<D: BlockDevice>(dev: &D, p: &FormatParams) -> Result<()> {
    let bps = dev.sector_size() as u32;
    let total = dev.sector_count().min(u32::MAX as u64) as u32;
    let mb = total as u64 * bps as u64 / (1024 * 1024);
    let fat_type = p.fat_type.unwrap_or(if mb < 16 {
        FatType::Fat12
    } else if mb < 512 {
        FatType::Fat16
    } else {
        FatType::Fat32
    });
    let spc: u32 = match fat_type {
        FatType::Fat12 => {
            if total <= 2880 * 2 { 1 } else { 8 }
        }
        FatType::Fat16 => match mb {
            0..=64 => 4,
            65..=128 => 8,
            129..=256 => 16,
            _ => 32,
        },
        FatType::Fat32 => match mb {
            0..=8192 => 8,
            8193..=16384 => 16,
            _ => 32,
        },
    } * 512
        / bps;
    let mut spc = spc.max(1);
    // The FAT type is defined by the cluster count: keep it in range.
    let approx = |spc: u32| total / spc;
    match fat_type {
        FatType::Fat32 => {
            while approx(spc) < 65_600 && spc > 1 {
                spc /= 2;
            }
        }
        FatType::Fat16 => {
            while approx(spc) >= 65_500 && spc < 128 {
                spc *= 2;
            }
            while approx(spc) < 4_100 && spc > 1 {
                spc /= 2;
            }
        }
        FatType::Fat12 => {
            while approx(spc) >= 4_080 && spc < 128 {
                spc *= 2;
            }
        }
    }
    let reserved: u32 = if fat_type == FatType::Fat32 { 32 } else { 1 };
    let root_entries: u32 = match fat_type {
        FatType::Fat32 => 0,
        FatType::Fat12 if total <= 2880 => 224,
        _ => 512,
    };
    let root_sectors = (root_entries * 32).div_ceil(bps);
    let fats = 2;
    // Solve for the FAT size iteratively.
    let mut fat_size = 1;
    loop {
        let data = total.saturating_sub(reserved + fats * fat_size + root_sectors);
        let clusters = data / spc;
        let bytes = match fat_type {
            FatType::Fat12 => (clusters + 2) * 3 / 2 + 1,
            FatType::Fat16 => (clusters + 2) * 2,
            FatType::Fat32 => (clusters + 2) * 4,
        };
        let need = bytes.div_ceil(bps);
        if need <= fat_size {
            break;
        }
        fat_size = need;
    }
    let mut bs = vec![0u8; bps as usize];
    bs[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
    bs[3..11].copy_from_slice(b"RS2 4.52");
    bs[11..13].copy_from_slice(&(bps as u16).to_le_bytes());
    bs[13] = spc as u8;
    bs[14..16].copy_from_slice(&(reserved as u16).to_le_bytes());
    bs[16] = fats as u8;
    bs[17..19].copy_from_slice(&(root_entries as u16).to_le_bytes());
    if total < 0x10000 && fat_type != FatType::Fat32 {
        bs[19..21].copy_from_slice(&(total as u16).to_le_bytes());
    } else {
        bs[32..36].copy_from_slice(&total.to_le_bytes());
    }
    bs[21] = if total <= 2880 { 0xf0 } else { 0xf8 };
    bs[24..26].copy_from_slice(&63u16.to_le_bytes());
    bs[26..28].copy_from_slice(&255u16.to_le_bytes());
    bs[28..32].copy_from_slice(&p.hidden_sectors.to_le_bytes());
    let mut label = [b' '; 11];
    for (d, s) in label.iter_mut().zip(p.label.bytes()) {
        *d = s.to_ascii_uppercase();
    }
    if p.label.is_empty() {
        label = *b"NO NAME    ";
    }
    if fat_type == FatType::Fat32 {
        bs[36..40].copy_from_slice(&fat_size.to_le_bytes());
        bs[44..48].copy_from_slice(&2u32.to_le_bytes());
        bs[48..50].copy_from_slice(&1u16.to_le_bytes());
        bs[50..52].copy_from_slice(&6u16.to_le_bytes());
        bs[64] = 0x80;
        bs[66] = 0x29;
        bs[67..71].copy_from_slice(&p.serial.to_le_bytes());
        bs[71..82].copy_from_slice(&label);
        bs[82..90].copy_from_slice(b"FAT32   ");
    } else {
        bs[22..24].copy_from_slice(&(fat_size as u16).to_le_bytes());
        bs[36] = 0x80;
        bs[38] = 0x29;
        bs[39..43].copy_from_slice(&p.serial.to_le_bytes());
        bs[43..54].copy_from_slice(&label);
        bs[54..62].copy_from_slice(if fat_type == FatType::Fat12 { b"FAT12   " } else { b"FAT16   " });
    }
    bs[510] = 0x55;
    bs[511] = 0xaa;
    dev.write(0, &bs)?;
    let zero = vec![0u8; bps as usize];
    if fat_type == FatType::Fat32 {
        // FSInfo and backup boot sector.
        let mut fsi = vec![0u8; bps as usize];
        fsi[0..4].copy_from_slice(&0x4161_5252u32.to_le_bytes());
        fsi[484..488].copy_from_slice(&0x6141_7272u32.to_le_bytes());
        fsi[488..492].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        fsi[492..496].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        fsi[510] = 0x55;
        fsi[511] = 0xaa;
        dev.write(1, &fsi)?;
        dev.write(6, &bs)?;
        dev.write(7, &fsi)?;
    }
    // Clear FATs, root directory and (FAT32) the root cluster.
    let clear_end = reserved + fats * fat_size + root_sectors + if fat_type == FatType::Fat32 { spc } else { 0 };
    for s in reserved..clear_end {
        dev.write(s as u64, &zero)?;
    }
    // Media descriptor entries.
    for copy in 0..fats {
        let base = (reserved + copy * fat_size) as u64;
        let mut sec = vec![0u8; bps as usize];
        match fat_type {
            FatType::Fat12 => sec[0..3].copy_from_slice(&[bs[21], 0xff, 0xff]),
            FatType::Fat16 => sec[0..4].copy_from_slice(&[bs[21], 0xff, 0xff, 0xff]),
            FatType::Fat32 => {
                sec[0..4].copy_from_slice(&(0x0fff_ff00 | bs[21] as u32).to_le_bytes());
                sec[4..8].copy_from_slice(&0x0fff_ffffu32.to_le_bytes());
                sec[8..12].copy_from_slice(&0x0fff_ffffu32.to_le_bytes()); // root cluster
            }
        }
        dev.write(base, &sec)?;
    }
    if !p.label.is_empty() {
        // The label entry opens the root directory.
        let mut sec = vec![0u8; bps as usize];
        sec[0..11].copy_from_slice(&label);
        sec[11] = attr::VOLUME;
        dev.write((reserved + fats * fat_size) as u64, &sec)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use core::cell::RefCell;
    use std::vec::Vec;

    struct Mem(RefCell<Vec<u8>>);

    impl BlockDevice for Mem {
        fn sector_size(&self) -> usize {
            512
        }
        fn sector_count(&self) -> u64 {
            self.0.borrow().len() as u64 / 512
        }
        fn read(&self, lba: u64, buf: &mut [u8]) -> Result<()> {
            let o = lba as usize * 512;
            buf.copy_from_slice(&self.0.borrow()[o..o + buf.len()]);
            Ok(())
        }
        fn write(&self, lba: u64, buf: &[u8]) -> Result<()> {
            let o = lba as usize * 512;
            self.0.borrow_mut()[o..o + buf.len()].copy_from_slice(buf);
            Ok(())
        }
    }

    fn roundtrip(bytes: usize, ty: Option<FatType>, expect: FatType) {
        let dev = Mem(RefCell::new(std::vec![0u8; bytes]));
        format(&dev, &FormatParams { label: "TEST", serial: 0x1234, fat_type: ty, hidden_sectors: 0 }).unwrap();
        let mut v = Volume::mount(dev).unwrap();
        assert_eq!(v.fat_type, expect);
        let now = Timestamp::new(2026, 10, 18, 12, 0, 0);
        v.mkdir("OS2", now).unwrap();
        v.mkdir("OS2\\DLL", now).unwrap();
        let mut e = v.create_file("OS2\\DLL\\test.dll", 0, now).unwrap();
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7) as u8).collect();
        v.write(&mut e, 0, &data, now).unwrap();
        let e2 = v.lookup("os2\\dll\\TEST.DLL").unwrap();
        assert_eq!(e2.size, 100_000);
        let mut back = std::vec![0u8; 100_000];
        assert_eq!(v.read(&e2, 0, &mut back).unwrap(), 100_000);
        assert_eq!(back, data);
        // Many files force directory growth.
        for i in 0..200 {
            v.create_file(&std::format!("OS2\\F{i}.TXT"), 0, now).unwrap();
        }
        assert_eq!(v.read_dir(Dir::Cluster(v.lookup("OS2").unwrap().cluster)).unwrap().len(), 201);
        v.rename("OS2\\F1.TXT", "OS2\\DLL\\MOVED.TXT").unwrap();
        assert!(v.lookup("OS2\\DLL\\MOVED.TXT").is_ok());
        assert_eq!(v.lookup("OS2\\F1.TXT").unwrap_err(), Error::NotFound);
        assert_eq!(v.lookup("OS2\\DLL\\..\\F2.TXT").unwrap().short, "F2.TXT");
        let mut e3 = v.lookup("OS2\\DLL\\TEST.DLL").unwrap();
        v.set_size(&mut e3, 10, now).unwrap();
        assert_eq!(v.lookup("OS2\\DLL\\TEST.DLL").unwrap().size, 10);
        let free_before = v.info().unwrap().free_clusters;
        v.delete("OS2\\DLL\\TEST.DLL").unwrap();
        v.delete("OS2\\DLL\\MOVED.TXT").unwrap();
        v.rmdir("OS2\\DLL").unwrap();
        assert!(v.info().unwrap().free_clusters > free_before);
        assert_eq!(v.create_file("LONGFILENAME.TEXT", 0, now).unwrap_err(), Error::InvalidName);
    }

    fn sample(ty: FatType, bytes: usize) -> Volume<Mem> {
        let dev = Mem(RefCell::new(std::vec![0u8; bytes]));
        format(&dev, &FormatParams { label: "CHK", serial: 1, fat_type: Some(ty), hidden_sectors: 0 }).unwrap();
        let mut v = Volume::mount(dev).unwrap();
        let now = Timestamp::new(2026, 10, 18, 12, 0, 0);
        v.mkdir("DIR", now).unwrap();
        for (name, len) in [("A.TXT", 10_000usize), ("DIR\\B.TXT", 20_000), ("C.TXT", 5_000)] {
            let mut e = v.create_file(name, 0, now).unwrap();
            v.write(&mut e, 0, &std::vec![7u8; len], now).unwrap();
        }
        v
    }

    fn scan(v: &mut Volume<Mem>, fix: bool) -> (Vec<check::Finding>, check::Totals) {
        let mut c = check::Checker::new(v, fix).unwrap();
        c.scan().unwrap();
        c.recover(true, Timestamp::default()).unwrap();
        (c.findings.clone(), c.totals.clone())
    }

    #[test]
    fn check_clean_and_damaged() {
        for (ty, size) in [(FatType::Fat12, 1_474_560), (FatType::Fat16, 32 << 20), (FatType::Fat32, 80 << 20)] {
            let mut v = sample(ty, size);
            let (f, t) = scan(&mut v, false);
            assert!(f.is_empty(), "{ty:?}: {f:?}");
            assert_eq!((t.files, t.dirs, t.lost_chains), (3, 1, 0));
            let cs = v.cluster_size();
            assert_eq!(t.file_clusters, 10_000u32.div_ceil(cs) + 20_000u32.div_ceil(cs) + 5_000u32.div_ceil(cs));
            // A lost two-cluster chain, a cross-link (C.TXT's chain points
            // into A.TXT's) and a file whose size is too large.
            let free = v.clusters - 10;
            v.set_fat_entry(free, free + 1).unwrap();
            v.set_fat_entry(free + 1, v.eoc()).unwrap();
            let a = v.lookup("A.TXT").unwrap().cluster;
            let c = v.lookup("C.TXT").unwrap().cluster;
            v.set_fat_entry(c, a).unwrap();
            let mut b = v.lookup("DIR\\B.TXT").unwrap();
            b.size = 1_000_000;
            v.update_entry(&b).unwrap();
            let (f, t) = scan(&mut v, false);
            assert!(f.contains(&check::Finding::CrossLinked { path: "\\C.TXT".into(), cluster: a }), "{ty:?}: {f:?}");
            assert!(f.contains(&check::Finding::SizeAdjusted("\\DIR\\B.TXT".into())));
            // The planted chain, and the rest of C.TXT cut off by the link.
            assert_eq!(t.lost_chains, 2);
            assert_eq!(t.lost_clusters, 2 + 5_000u32.div_ceil(cs) - 1);
            // Nothing was written without fix.
            assert_eq!(v.lookup("DIR\\B.TXT").unwrap().size, 1_000_000);
            let (_, t) = scan(&mut v, true);
            assert!(t.recovered >= 1);
            assert!(v.lookup("FILE0000.CHK").is_ok());
            assert_eq!(v.lookup("DIR\\B.TXT").unwrap().size, 20_000u32.div_ceil(cs) * cs);
            // The cross-linked file was cut before the shared cluster.
            let (f, t) = scan(&mut v, false);
            assert!(f.is_empty(), "{ty:?}: {f:?}");
            assert_eq!(t.lost_chains, 0);
        }
    }

    /// The bulk free count agrees with counting entry by entry.
    #[test]
    fn free_count_in_chunks() {
        for (ty, size) in [(FatType::Fat12, 1_474_560), (FatType::Fat16, 32 << 20), (FatType::Fat32, 80 << 20)] {
            let v = sample(ty, size);
            let slow = (2..v.clusters + 2).filter(|&c| v.fat_entry(c).unwrap() == 0).count() as u32;
            assert_eq!(v.count_free().unwrap(), slow, "{ty:?}");
            assert!(slow > 0 && slow < v.clusters);
        }
    }

    #[test]
    fn check_broken_chain() {
        let mut v = sample(FatType::Fat16, 32 << 20);
        let a = v.lookup("A.TXT").unwrap().cluster;
        // Point A.TXT's first cluster at a free cluster: the chain ends
        // there (that cluster is kept, as its link is the bad one).
        v.set_fat_entry(a, v.clusters - 1).unwrap();
        let (f, _) = scan(&mut v, true);
        assert!(f.contains(&check::Finding::BadChain("\\A.TXT".into())), "{f:?}");
        let (f, t) = scan(&mut v, false);
        assert!(f.is_empty(), "{f:?}");
        assert_eq!(t.lost_chains, 0);
        assert_eq!(v.lookup("A.TXT").unwrap().size, 2 * v.cluster_size());
    }

    #[test]
    fn labels() {
        let mut v = sample(FatType::Fat16, 32 << 20);
        assert_eq!(&v.label, b"CHK        ");
        v.set_label("NEW LABEL").unwrap();
        let v2 = Volume::mount(v.dev).unwrap();
        assert_eq!(&v2.label, b"NEW LABEL  ");
        assert_eq!(v2.read_dir(v2.root_dir()).unwrap().len(), 3);
        let mut v2 = v2;
        v2.set_label("").unwrap();
        let v3 = Volume::mount(v2.dev).unwrap();
        assert_eq!(&v3.label, b"NO NAME    ");
    }

    #[test]
    fn fat12() {
        roundtrip(1_474_560, None, FatType::Fat12);
    }

    #[test]
    fn fat16() {
        roundtrip(32 << 20, None, FatType::Fat16);
    }

    #[test]
    fn fat32() {
        roundtrip(80 << 20, Some(FatType::Fat32), FatType::Fat32);
    }

    fn ea(name: &str, len: usize, seed: u8) -> ea::Ea {
        ea::Ea { flags: 0, name: name.as_bytes().to_vec(), value: (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect() }
    }

    fn clean(v: &mut Volume<Mem>) {
        let (f, t) = scan(v, false);
        assert!(f.is_empty(), "{f:?}");
        assert_eq!(t.lost_chains, 0);
    }

    fn ea_roundtrip(ty: FatType, bytes: usize) {
        let dev = Mem(RefCell::new(std::vec![0u8; bytes]));
        format(&dev, &FormatParams { label: "EA", serial: 7, fat_type: Some(ty), hidden_sectors: 0 }).unwrap();
        let mut v = Volume::mount(dev).unwrap();
        let now = Timestamp::new(2026, 10, 18, 12, 0, 0);
        let free0 = v.info().unwrap().free_clusters;
        v.mkdir("DIR", now).unwrap();
        let mut a = v.create_file("A.TXT", 0, now).unwrap();
        v.write(&mut a, 0, b"hello", now).unwrap();
        v.create_file("DIR\\B.TXT", 0, now).unwrap();
        let a = v.lookup("A.TXT").unwrap();
        assert!(v.read_eas(&a).unwrap().is_empty());
        let set1 = std::vec![ea(".TYPE", 20, 1), ea(".LONGNAME", 9, 2)];
        v.write_eas(&a, &set1, now).unwrap();
        assert_eq!(v.read_eas(&a).unwrap(), set1);
        // A stale copy of the entry (as an open file holds) still finds them,
        // and rewriting its metadata keeps the reference.
        let mut stale = a.clone();
        v.write(&mut stale, 5, b" world", now).unwrap();
        assert_eq!(v.read_eas(&v.lookup("A.TXT").unwrap()).unwrap(), set1);
        let d = v.lookup("DIR").unwrap();
        let dset = std::vec![ea(".ICON", 3000, 3)];
        v.write_eas(&d, &dset, now).unwrap();
        let b = v.lookup("DIR\\B.TXT").unwrap();
        let bset = std::vec![ea("BIG", 60_000, 4)];
        v.write_eas(&b, &bset, now).unwrap();
        clean(&mut v);
        // Grow and shrink sets in the middle of the file.
        let set2 = std::vec![ea(".TYPE", 5000, 5)];
        v.write_eas(&a, &set2, now).unwrap();
        assert_eq!(v.read_eas(&v.lookup("DIR").unwrap()).unwrap(), dset);
        assert_eq!(v.read_eas(&v.lookup("DIR\\B.TXT").unwrap()).unwrap(), bset);
        assert_eq!(v.read_eas(&v.lookup("A.TXT").unwrap()).unwrap(), set2);
        v.write_eas(&d, &std::vec![ea("X", 1, 6)], now).unwrap();
        assert_eq!(v.read_eas(&v.lookup("DIR\\B.TXT").unwrap()).unwrap(), bset);
        assert_eq!(v.read_eas(&v.lookup("A.TXT").unwrap()).unwrap(), set2);
        clean(&mut v);
        // Too large.
        assert_eq!(v.write_eas(&a, &[ea("HUGE", 65_530, 0)], now).unwrap_err(), Error::EaTooLong);
        // Rename and move keep EAs.
        v.rename("A.TXT", "C.TXT").unwrap();
        v.rename("C.TXT", "DIR\\D.TXT").unwrap();
        assert_eq!(v.read_eas(&v.lookup("DIR\\D.TXT").unwrap()).unwrap(), set2);
        clean(&mut v);
        // Remount and read back.
        let mut v = Volume::mount(v.dev).unwrap();
        assert_eq!(v.read_eas(&v.lookup("DIR\\D.TXT").unwrap()).unwrap(), set2);
        // Many files with small EAs (FAT12/16: more handle tables).
        for i in 0..300 {
            let mut e = v.create_file(&std::format!("DIR\\F{i}.X"), 0, now).unwrap();
            v.write(&mut e, 0, b"x", now).unwrap();
            let e = v.lookup(&std::format!("DIR\\F{i}.X")).unwrap();
            v.write_eas(&e, &[ea("N", i % 50, i as u8)], now).unwrap();
        }
        clean(&mut v);
        for i in (0..300).step_by(7) {
            assert_eq!(v.read_eas(&v.lookup(&std::format!("DIR\\F{i}.X")).unwrap()).unwrap(), std::vec![ea("N", i % 50, i as u8)]);
        }
        // Deleting files and removing EAs frees the space again.
        for i in 0..300 {
            v.delete(&std::format!("DIR\\F{i}.X")).unwrap();
        }
        let d = v.lookup("DIR").unwrap();
        v.write_eas(&d, &[], now).unwrap();
        assert!(v.read_eas(&v.lookup("DIR").unwrap()).unwrap().is_empty());
        v.delete("DIR\\D.TXT").unwrap();
        v.delete("DIR\\B.TXT").unwrap();
        clean(&mut v);
        assert!(v.read_dir(Dir::Cluster(v.lookup("DIR").unwrap().cluster)).unwrap().is_empty());
        v.rmdir("DIR").unwrap();
        clean(&mut v);
        let left = free0 - v.info().unwrap().free_clusters;
        if ty == FatType::Fat32 {
            assert_eq!(left, 0);
        } else {
            // Only EA DATA. SF's header remains.
            let f = v.load_ea_file().unwrap().unwrap();
            assert_eq!(left, f.entry.size / v.cluster_size());
            assert!(f.offsets.iter().all(|&o| o == ea::UNUSED));
            let root = v.read_dir(v.root_dir()).unwrap();
            let e = root.iter().find(|e| e.short == ea::EA_FILE_NAME).unwrap();
            assert_eq!(e.attr, 0x27);
        }
    }

    #[test]
    fn eas_fat12() {
        ea_roundtrip(FatType::Fat12, 1_474_560);
    }

    #[test]
    fn eas_fat16() {
        ea_roundtrip(FatType::Fat16, 32 << 20);
    }

    #[test]
    fn eas_fat32() {
        ea_roundtrip(FatType::Fat32, 80 << 20);
    }

    #[test]
    fn eas_checker_repairs() {
        let mut v = sample(FatType::Fat16, 32 << 20);
        let now = Timestamp::new(2026, 10, 18, 12, 0, 0);
        let a = v.lookup("A.TXT").unwrap();
        let c = v.lookup("C.TXT").unwrap();
        v.write_eas(&a, &[ea("ONE", 10, 1)], now).unwrap();
        v.write_eas(&c, &[ea("TWO", 10, 2)], now).unwrap();
        clean(&mut v);
        // C.TXT's handle copied to B.TXT.
        let h = v.ea_handle_of(&v.lookup("C.TXT").unwrap()).unwrap();
        let mut b = v.lookup("DIR\\B.TXT").unwrap();
        b.ea_handle = h;
        v.write_entry(&b).unwrap();
        let (f, _) = scan(&mut v, false);
        assert_eq!(f, std::vec![check::Finding::EaShared("\\DIR\\B.TXT".into())]);
        // Point A.TXT at a handle that does not exist: its set is orphaned.
        let mut a = v.lookup("A.TXT").unwrap();
        a.ea_handle = 200;
        v.write_entry(&a).unwrap();
        let (f, _) = scan(&mut v, true);
        assert!(f.contains(&check::Finding::EaMissing("\\A.TXT".into())), "{f:?}");
        assert!(f.iter().any(|x| matches!(x, check::Finding::EaOrphaned(_))), "{f:?}");
        clean(&mut v);
        assert!(v.read_eas(&v.lookup("DIR\\B.TXT").unwrap()).unwrap().is_empty());
        assert!(v.read_eas(&v.lookup("A.TXT").unwrap()).unwrap().is_empty());
        assert_eq!(v.read_eas(&v.lookup("C.TXT").unwrap()).unwrap(), std::vec![ea("TWO", 10, 2)]);
    }

    /// OS/2's own EA DATA. SF on the Warp 4.52 boot floppy.
    #[test]
    fn os2_floppy_eas() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/client/bootimgs/disk_0_1.img");
        let Ok(img) = std::fs::read(path) else { return };
        let mut v = Volume::mount(Mem(RefCell::new(img))).unwrap();
        let k = v.lookup("OS2KRNL").unwrap();
        assert_ne!(k.ea_handle, 0);
        let eas = v.read_eas(&k).unwrap();
        assert!(eas.iter().any(|e| e.name == b".TYPE"), "{eas:?}");
        let d = v.lookup("OS2DASD.DMD").unwrap();
        let eas = v.read_eas(&d).unwrap();
        assert_eq!(eas[0].name, b".APPTYPE");
        clean(&mut v);
        // Our writes keep it well formed.
        let now = Timestamp::new(2026, 10, 18, 12, 0, 0);
        v.write_eas(&d, &[ea(".SUBJECT", 2000, 1)], now).unwrap();
        clean(&mut v);
        assert!(v.read_eas(&v.lookup("OS2KRNL").unwrap()).unwrap().iter().any(|e| e.name == b".TYPE"));
    }

    /// Test disks built by `cargo xtask disk` (and written by rs2 in QEMU):
    /// check them, EAs included, if present.
    #[test]
    fn xtask_disk_images() {
        for name in ["disk.img", "t16.img", "f32.img"] {
            let path = std::format!("{}/../../work/{name}", env!("CARGO_MANIFEST_DIR"));
            let Ok(img) = std::fs::read(&path) else { continue };
            let mut v = Volume::mount(Mem(RefCell::new(img[2048 * 512..].to_vec()))).unwrap();
            let (f, t) = scan(&mut v, false);
            assert!(f.is_empty(), "{path}: {f:?}");
            let mut with_eas = 0;
            let mut stack = std::vec![v.root_dir()];
            while let Some(d) = stack.pop() {
                for e in v.read_dir(d).unwrap() {
                    if e.is_dir() {
                        stack.push(Dir::Cluster(e.cluster));
                    }
                    let eas = v.read_eas(&e).unwrap();
                    if !eas.is_empty() {
                        with_eas += 1;
                        std::println!("{name}: {}: {:?}", e.name, eas.iter().map(|x| std::string::String::from_utf8_lossy(&x.name).into_owned()).collect::<Vec<_>>());
                    }
                }
            }
            std::println!("{path}: {:?}, {} files, {} with EAs, {} EA clusters", v.fat_type, t.files, with_eas, t.ea_clusters);
        }
    }

    #[test]
    fn os2_boot_floppy() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/client/bootimgs/disk_0_1.img");
        let Ok(img) = std::fs::read(path) else { return };
        let v = Volume::mount(Mem(RefCell::new(img))).unwrap();
        let root = v.read_dir(v.root_dir()).unwrap();
        let krnl = root.iter().find(|e| e.name == "OS2KRNL").expect("OS2KRNL");
        let mut buf = std::vec![0u8; krnl.size as usize];
        v.read(krnl, 0, &mut buf).unwrap();
        assert_eq!(&buf[0..2], b"MZ");
    }
}
