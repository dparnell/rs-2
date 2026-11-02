//! Partition tables as OS/2's Logical Volume Manager keeps them.
//!
//! MBR disks: four primary entries in sector 0, logical partitions in a
//! chain of extended boot records (EBRs) inside one extended partition,
//! and OS/2 LVM's *drive letter assignment tables* (DLAT): one in the last
//! sector of the first track of each partition table (LBA 62 for the MBR,
//! EBR + 62 for each EBR; 63 sectors per track), describing the
//! partitions of that table:
//!
//! ```text
//! DLA table sector (512 bytes)
//!   +0x00 u32  signature 1  0x424D5202
//!   +0x04 u32  signature 2  0x44464D50
//!   +0x08 u32  CRC-32 of the sector with this field 0 (reflected
//!              polynomial 0xEDB88320, start 0xFFFFFFFF, no final xor)
//!   +0x0C u32  disk serial number
//!   +0x10 u32  boot disk serial number
//!   +0x14 u32  install flags
//!   +0x18 u32  cylinders, +0x1C heads, +0x20 sectors per track
//!   +0x24      disk name (20 bytes)
//!   +0x38 u8   reboot flag, 3 reserved
//!   +0x3C      4 entries of 60 bytes:
//!     +0x00 u32 volume serial (0: the partition is in no volume)
//!     +0x04 u32 partition serial
//!     +0x08 u32 partition size, +0x0C partition start (sectors)
//!     +0x10 u8  on the Boot Manager menu, +0x11 installable
//!     +0x12 u8  drive letter ('C'..'Z', 0 none, '*' any)
//!     +0x13 u8  reserved
//!     +0x14     volume name (20 bytes), +0x28 partition name (20 bytes)
//! ```
//!
//! Partitions are matched to DLA entries by start and size. rs2's disks
//! use a synthetic geometry of 255 heads and 63 sectors per track (the
//! category 9 IOCtls report it), so new partitions start and end on
//! 16065-sector cylinder boundaries, as IBM's LVM places them; the first
//! one starts at sector 63 and a logical partition 63 sectors after its
//! EBR.
//!
//! GPT disks (rs2's installer writes them): the primary and backup
//! headers and entry arrays, entries of 128 bytes with UTF-16 names. They
//! have no DLAT: rs2 gives their volumes letters in order.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub const SECTOR: usize = 512;
pub const HEADS: u64 = 255;
pub const SPT: u64 = 63;
pub const CYLINDER: u64 = HEADS * SPT;
pub const DLA_SIG1: u32 = 0x424D_5202;
pub const DLA_SIG2: u32 = 0x4446_4D50;
/// GPT basic data partition, EBD0A0A2-B9E5-4433-87C0-68B6B72699C7.
pub const GPT_DATA: [u8; 16] = [0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7];
/// EFI system partition, C12A7328-F81F-11D2-BA4B-00A0C93EC93B.
pub const GPT_ESP: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Error {
    Io(u32),
    /// No free space of that size where asked.
    NoSpace,
    /// No such partition or volume.
    NotFound,
    /// Four primary partitions (counting the extended one) already.
    TableFull,
    /// A name or letter already in use.
    InUse,
    /// The operation does not apply (e.g. drive letters on a GPT disk).
    NotSupported,
    /// The disk's tables cannot be read.
    Corrupt,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Sector access to one whole disk (512-byte sectors).
pub trait Disk {
    fn sectors(&self) -> u64;
    fn read(&self, lba: u64, buf: &mut [u8]) -> core::result::Result<(), u32>;
    fn write(&self, lba: u64, buf: &[u8]) -> core::result::Result<(), u32>;
}

fn rd(d: &dyn Disk, lba: u64) -> Result<Vec<u8>> {
    let mut b = vec![0u8; SECTOR];
    d.read(lba, &mut b).map_err(Error::Io)?;
    Ok(b)
}

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn le64(b: &[u8], o: usize) -> u64 {
    le32(b, o) as u64 | (le32(b, o + 4) as u64) << 32
}
fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// OS/2 LVM's CRC-32 (no final inversion).
pub fn lvm_crc(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    c
}

/// The standard CRC-32 (GPT).
pub fn crc32(data: &[u8]) -> u32 {
    !lvm_crc(data)
}

fn name20(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}

fn put_name20(b: &mut [u8], s: &str) {
    b[..20].fill(0);
    for (i, c) in s.chars().take(20).enumerate() {
        b[i] = c as u32 as u8;
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DlaEntry {
    pub volume_serial: u32,
    pub partition_serial: u32,
    pub size: u32,
    pub start: u32,
    pub on_boot_menu: bool,
    pub installable: bool,
    /// 'C'..'Z', 0 for none, '*' for any.
    pub letter: u8,
    pub volume_name: String,
    pub partition_name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DlaHeader {
    pub disk_serial: u32,
    pub boot_disk_serial: u32,
    pub install_flags: u32,
    pub disk_name: String,
    pub reboot: bool,
}

/// Parse a DLA table sector; `None` if the signatures or the CRC do not
/// match.
pub fn parse_dla(s: &[u8]) -> Option<(DlaHeader, [DlaEntry; 4])> {
    if s.len() < SECTOR || le32(s, 0) != DLA_SIG1 || le32(s, 4) != DLA_SIG2 {
        return None;
    }
    let mut c = s[..SECTOR].to_vec();
    put32(&mut c, 8, 0);
    if lvm_crc(&c) != le32(s, 8) {
        return None;
    }
    let h = DlaHeader { disk_serial: le32(s, 0x0c), boot_disk_serial: le32(s, 0x10), install_flags: le32(s, 0x14), disk_name: name20(&s[0x24..0x38]), reboot: s[0x38] != 0 };
    let mut e: [DlaEntry; 4] = Default::default();
    for (i, x) in e.iter_mut().enumerate() {
        let o = 0x3c + i * 60;
        *x = DlaEntry {
            volume_serial: le32(s, o),
            partition_serial: le32(s, o + 4),
            size: le32(s, o + 8),
            start: le32(s, o + 12),
            on_boot_menu: s[o + 16] != 0,
            installable: s[o + 17] != 0,
            letter: s[o + 18],
            volume_name: name20(&s[o + 20..o + 40]),
            partition_name: name20(&s[o + 40..o + 60]),
        };
    }
    Some((h, e))
}

/// Build a DLA table sector.
pub fn make_dla(h: &DlaHeader, entries: &[DlaEntry]) -> Vec<u8> {
    let mut s = vec![0u8; SECTOR];
    put32(&mut s, 0, DLA_SIG1);
    put32(&mut s, 4, DLA_SIG2);
    put32(&mut s, 0x0c, h.disk_serial);
    put32(&mut s, 0x10, h.boot_disk_serial);
    put32(&mut s, 0x14, h.install_flags);
    put32(&mut s, 0x18, 0); // cylinders: set by the caller's geometry
    put32(&mut s, 0x1c, HEADS as u32);
    put32(&mut s, 0x20, SPT as u32);
    put_name20(&mut s[0x24..0x38], &h.disk_name);
    s[0x38] = h.reboot as u8;
    for (i, e) in entries.iter().take(4).enumerate() {
        let o = 0x3c + i * 60;
        put32(&mut s, o, e.volume_serial);
        put32(&mut s, o + 4, e.partition_serial);
        put32(&mut s, o + 8, e.size);
        put32(&mut s, o + 12, e.start);
        s[o + 16] = e.on_boot_menu as u8;
        s[o + 17] = e.installable as u8;
        s[o + 18] = e.letter;
        put_name20(&mut s[o + 20..o + 40], &e.volume_name);
        put_name20(&mut s[o + 40..o + 60], &e.partition_name);
    }
    s
}

fn seal_dla(s: &mut [u8], cylinders: u32) {
    put32(s, 0x18, cylinders);
    put32(s, 8, 0);
    let c = lvm_crc(&s[..SECTOR]);
    put32(s, 8, c);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Logical,
    Gpt,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    pub kind: Kind,
    pub start: u64,
    pub count: u64,
    /// MBR partition type (0 on GPT disks).
    pub mbr_type: u8,
    pub active: bool,
    pub gpt_type: [u8; 16],
    pub guid: [u8; 16],
    /// GPT name, or the DLA partition name.
    pub name: String,
    /// The partition's DLA entry (MBR disks).
    pub dla: Option<DlaEntry>,
}

impl Partition {
    /// In a volume (DLA volume serial set).
    pub fn volume(&self) -> Option<&DlaEntry> {
        self.dla.as_ref().filter(|d| d.volume_serial != 0)
    }

    pub fn mb(&self) -> u64 {
        self.count / 2048
    }

    /// A short description of the partition type.
    pub fn type_name(&self) -> &'static str {
        if self.kind == Kind::Gpt {
            return match self.gpt_type {
                GPT_ESP => "EFI system",
                GPT_DATA => "Data",
                _ => "Other",
            };
        }
        match self.mbr_type {
            0x01 | 0x04 | 0x06 | 0x0e => "FAT",
            0x0b | 0x0c => "FAT32",
            0x07 => "HPFS/NTFS",
            0x35 => "LVM (JFS)",
            0x0a => "Boot Manager",
            0x82 => "Linux swap",
            0x83 => "Linux",
            0xef => "EFI",
            0x11 | 0x14 | 0x16 | 0x17 | 0x1b | 0x1c | 0x1e => "Hidden",
            _ => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scheme {
    /// No partition table.
    Empty,
    Mbr,
    Gpt,
}

/// A free stretch of the disk: where a primary or a logical partition
/// could go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Free {
    pub start: u64,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub scheme: Scheme,
    pub sectors: u64,
    pub parts: Vec<Partition>,
    pub dla: DlaHeader,
    /// MBR disk signature (0x1B8).
    pub signature: u32,
    /// GPT disk GUID.
    pub disk_guid: [u8; 16],
}

fn is_extended(t: u8) -> bool {
    matches!(t, 0x05 | 0x0f | 0x85)
}

fn find_dla(dla: &Option<(DlaHeader, [DlaEntry; 4])>, start: u64, count: u64) -> Option<DlaEntry> {
    dla.as_ref()?.1.iter().find(|e| e.start as u64 == start && e.size as u64 == count && (e.start != 0 || e.size != 0)).cloned()
}

/// A serial number from a seed (xorshift; never 0).
pub fn serial(seed: &mut u32) -> u32 {
    let mut x = *seed | 1;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x | 1
}

impl Layout {
    pub fn read(d: &dyn Disk) -> Result<Layout> {
        let sectors = d.sectors();
        let mbr = rd(d, 0)?;
        let mut l = Layout { scheme: Scheme::Empty, sectors, parts: Vec::new(), dla: DlaHeader::default(), signature: le32(&mbr, 0x1b8), disk_guid: [0; 16] };
        if mbr[510] != 0x55 || mbr[511] != 0xaa {
            return Ok(l);
        }
        let ents: Vec<(u8, u8, u64, u64)> = (0..4).map(|i| {
            let e = 446 + i * 16;
            (mbr[e], mbr[e + 4], le32(&mbr, e + 8) as u64, le32(&mbr, e + 12) as u64)
        }).collect();
        if ents.iter().any(|e| e.1 == 0xee) {
            l.scheme = Scheme::Gpt;
            l.read_gpt(d)?;
            return Ok(l);
        }
        // A volume boot record (no table) has no valid entries.
        if ents.iter().all(|e| e.1 == 0) && mbr[0] == 0xeb {
            return Ok(l);
        }
        l.scheme = Scheme::Mbr;
        let top = if sectors > SPT { parse_dla(&rd(d, SPT - 1)?) } else { None };
        if let Some((h, _)) = &top {
            l.dla = h.clone();
        }
        for &(flag, t, start, count) in &ents {
            if t == 0 || count == 0 || is_extended(t) {
                continue;
            }
            let dla = find_dla(&top, start, count);
            l.parts.push(Partition {
                kind: Kind::Primary,
                start,
                count,
                mbr_type: t,
                active: flag & 0x80 != 0,
                gpt_type: [0; 16],
                guid: [0; 16],
                name: dla.as_ref().map(|e| e.partition_name.clone()).unwrap_or_default(),
                dla,
            });
        }
        for &(_, t, ext, _) in &ents {
            if !is_extended(t) {
                continue;
            }
            let mut ebr_lba = ext;
            for _ in 0..256 {
                let ebr = rd(d, ebr_lba)?;
                if ebr[510] != 0x55 || ebr[511] != 0xaa {
                    break;
                }
                let (flag, lt, ls, lc) = (ebr[446], ebr[446 + 4], le32(&ebr, 446 + 8) as u64, le32(&ebr, 446 + 12) as u64);
                if lt != 0 && lc != 0 {
                    let tbl = parse_dla(&rd(d, ebr_lba + SPT - 1)?);
                    let start = ebr_lba + ls;
                    let dla = find_dla(&tbl, start, lc);
                    if l.dla.disk_serial == 0 {
                        if let Some((h, _)) = &tbl {
                            l.dla = h.clone();
                        }
                    }
                    l.parts.push(Partition {
                        kind: Kind::Logical,
                        start,
                        count: lc,
                        mbr_type: lt,
                        active: flag & 0x80 != 0,
                        gpt_type: [0; 16],
                        guid: [0; 16],
                        name: dla.as_ref().map(|e| e.partition_name.clone()).unwrap_or_default(),
                        dla,
                    });
                }
                let (nt, ns) = (ebr[462 + 4], le32(&ebr, 462 + 8) as u64);
                if !is_extended(nt) || ns == 0 {
                    break;
                }
                ebr_lba = ext + ns;
            }
        }
        l.parts.sort_by_key(|p| p.start);
        Ok(l)
    }

    fn read_gpt(&mut self, d: &dyn Disk) -> Result<()> {
        let h = rd(d, 1)?;
        if &h[0..8] != b"EFI PART" {
            return Err(Error::Corrupt);
        }
        self.disk_guid.copy_from_slice(&h[56..72]);
        let at = le64(&h, 72);
        let n = le32(&h, 80) as usize;
        let esz = le32(&h, 84) as usize;
        if !(128..=SECTOR).contains(&esz) || n > 1024 {
            return Err(Error::Corrupt);
        }
        let secs = (n * esz).div_ceil(SECTOR);
        let mut t = vec![0u8; secs * SECTOR];
        d.read(at, &mut t).map_err(Error::Io)?;
        for i in 0..n {
            let e = &t[i * esz..(i + 1) * esz];
            let mut ty = [0u8; 16];
            ty.copy_from_slice(&e[0..16]);
            if ty == [0; 16] {
                continue;
            }
            let mut guid = [0u8; 16];
            guid.copy_from_slice(&e[16..32]);
            let (s, last) = (le64(e, 32), le64(e, 40));
            if last < s {
                continue;
            }
            let name: String = (0..36).map(|k| le16(e, 56 + 2 * k)).take_while(|&c| c != 0).map(|c| char::from_u32(c as u32).unwrap_or('?')).collect();
            self.parts.push(Partition { kind: Kind::Gpt, start: s, count: last - s + 1, mbr_type: 0, active: false, gpt_type: ty, guid, name, dla: None });
        }
        self.parts.sort_by_key(|p| p.start);
        Ok(())
    }

    /// GPT usable range (first, last) for the standard 128-entry array.
    pub fn gpt_usable(&self) -> (u64, u64) {
        (34, self.sectors.saturating_sub(34))
    }

    /// Free stretches for a new primary (or GPT) partition, or for a
    /// logical one. Cylinder-aligned on MBR disks.
    pub fn free(&self, logical: bool) -> Vec<Free> {
        let mut used: Vec<(u64, u64)> = self.parts.iter().map(|p| {
            // A logical partition's EBR track belongs to it.
            let s = if p.kind == Kind::Logical { p.start.saturating_sub(SPT) } else { p.start };
            (s, p.start + p.count)
        }).collect();
        used.sort();
        let (lo, hi) = match self.scheme {
            Scheme::Gpt => {
                let (a, b) = self.gpt_usable();
                (a.next_multiple_of(2048), b + 1)
            }
            _ => (SPT, self.sectors / CYLINDER * CYLINDER),
        };
        let mut out = Vec::new();
        let mut at = lo;
        for &(s, e) in used.iter().chain(core::iter::once(&(hi, hi))) {
            if s > at {
                let (fs, fe) = if self.scheme == Scheme::Gpt {
                    (at.next_multiple_of(2048), s)
                } else if logical {
                    // EBR on a cylinder boundary, data one track later.
                    (at.next_multiple_of(CYLINDER) + SPT, s / CYLINDER * CYLINDER)
                } else {
                    let a = if at <= SPT { SPT } else { at.next_multiple_of(CYLINDER) };
                    (a, s / CYLINDER * CYLINDER)
                };
                if fe > fs && fe - fs >= if self.scheme == Scheme::Gpt { 2048 } else { CYLINDER - SPT } {
                    out.push(Free { start: fs, count: fe - fs });
                }
            }
            at = at.max(e);
        }
        // The logical partitions share one extended partition: no primary
        // may sit inside its span, and it may not grow over one.
        let logi: Vec<&Partition> = self.parts.iter().filter(|p| p.kind == Kind::Logical).collect();
        if let (Some(f), Some(l)) = (logi.first(), logi.last()) {
            let (es, ee) = (f.start - SPT, l.start + l.count);
            let prim: Vec<(u64, u64)> = self.parts.iter().filter(|p| p.kind == Kind::Primary).map(|p| (p.start, p.start + p.count)).collect();
            out.retain(|fr| {
                let (fs, fe) = (fr.start, fr.start + fr.count);
                if logical {
                    let (us, ue) = (fs.min(es), fe.max(ee));
                    !prim.iter().any(|&(ps, pe)| ps < ue && pe > us)
                } else {
                    fe <= es || fs >= ee
                }
            });
        }
        out
    }

    fn primaries_used(&self) -> usize {
        let p = self.parts.iter().filter(|p| p.kind == Kind::Primary).count();
        p + self.parts.iter().any(|p| p.kind == Kind::Logical) as usize
    }

    /// Add a partition of `mb` megabytes (0: all of the free space) in
    /// free stretch `free` (from [`Layout::free`]), at its start or end.
    /// Returns its index in `parts`.
    pub fn create(&mut self, free: Free, mb: u64, logical: bool, from_end: bool, mbr_type: u8, name: &str, seed: &mut u32) -> Result<usize> {
        let want = if mb == 0 { free.count } else { mb * 2048 };
        if free.count == 0 || want > free.count + CYLINDER {
            return Err(Error::NoSpace);
        }
        let gpt = self.scheme == Scheme::Gpt;
        let align = if gpt { 2048 } else { CYLINDER };
        let end_all = free.start + free.count;
        // Ends on an alignment boundary; MBR partitions start a track into
        // their first cylinder when it is cylinder 0 or holds their EBR.
        let (start, end) = if from_end {
            let c = end_all.saturating_sub(want) / align * align;
            let s = if gpt { c } else if logical && c == 0 { 2 * SPT } else if logical || c == 0 { c + SPT } else { c };
            (s.max(free.start), end_all)
        } else {
            // The size in whole alignment units, rounded to the nearest.
            let n = ((free.start + want + align / 2) / align).max(free.start / align + 1);
            (free.start, (n * align).min(end_all))
        };
        if end <= start {
            return Err(Error::NoSpace);
        }
        let count = end - start;
        match self.scheme {
            Scheme::Gpt => {
                if self.parts.len() >= 128 {
                    return Err(Error::TableFull);
                }
                let mut guid = [0u8; 16];
                for c in guid.chunks_mut(4) {
                    c.copy_from_slice(&serial(seed).to_le_bytes());
                }
                guid[7] = (guid[7] & 0x0f) | 0x40;
                guid[8] = (guid[8] & 0x3f) | 0x80;
                self.parts.push(Partition { kind: Kind::Gpt, start, count, mbr_type: 0, active: false, gpt_type: GPT_DATA, guid, name: String::from(name), dla: None });
            }
            Scheme::Empty | Scheme::Mbr => {
                if self.scheme == Scheme::Empty {
                    self.scheme = Scheme::Mbr;
                }
                if logical {
                    if !self.parts.iter().any(|p| p.kind == Kind::Logical) && self.primaries_used() >= 4 {
                        return Err(Error::TableFull);
                    }
                } else if self.primaries_used() >= 4 {
                    return Err(Error::TableFull);
                }
                let dla = DlaEntry { partition_serial: serial(seed), size: count as u32, start: start as u32, partition_name: String::from(name), ..Default::default() };
                self.parts.push(Partition {
                    kind: if logical { Kind::Logical } else { Kind::Primary },
                    start,
                    count,
                    mbr_type,
                    active: false,
                    gpt_type: [0; 16],
                    guid: [0; 16],
                    name: String::from(name),
                    dla: Some(dla),
                });
            }
        }
        self.parts.sort_by_key(|p| p.start);
        Ok(self.parts.iter().position(|p| p.start == start).unwrap())
    }

    pub fn delete(&mut self, i: usize) -> Result<()> {
        if i >= self.parts.len() {
            return Err(Error::NotFound);
        }
        self.parts.remove(i);
        Ok(())
    }

    /// Make partition `i` the one the MBR boot code starts (primary only).
    pub fn set_active(&mut self, i: usize) -> Result<()> {
        if self.parts.get(i).is_none_or(|p| p.kind != Kind::Primary) {
            return Err(Error::NotSupported);
        }
        for (k, p) in self.parts.iter_mut().enumerate() {
            p.active = k == i;
        }
        Ok(())
    }

    /// Make partition `i` a compatibility volume (or rename it / change
    /// its letter if it is one). `letter` 0: none, `*`: any.
    pub fn set_volume(&mut self, i: usize, name: &str, letter: u8, seed: &mut u32) -> Result<()> {
        if self.scheme == Scheme::Gpt {
            return Err(Error::NotSupported);
        }
        if letter.is_ascii_alphabetic() && self.parts.iter().enumerate().any(|(k, p)| k != i && p.volume().is_some_and(|v| v.letter.eq_ignore_ascii_case(&letter))) {
            return Err(Error::InUse);
        }
        if self.parts.iter().enumerate().any(|(k, p)| k != i && p.volume().is_some_and(|v| v.volume_name.eq_ignore_ascii_case(name))) {
            return Err(Error::InUse);
        }
        let p = self.parts.get_mut(i).ok_or(Error::NotFound)?;
        let pname = p.name.clone();
        let d = p.dla.get_or_insert_with(|| DlaEntry { partition_serial: 0, partition_name: pname, ..Default::default() });
        if d.partition_serial == 0 {
            d.partition_serial = serial(seed);
        }
        if d.volume_serial == 0 {
            d.volume_serial = serial(seed);
        }
        d.volume_name = String::from(name);
        d.letter = letter.to_ascii_uppercase();
        Ok(())
    }

    /// Take partition `i` out of its volume (the partition stays).
    pub fn delete_volume(&mut self, i: usize) -> Result<()> {
        let p = self.parts.get_mut(i).ok_or(Error::NotFound)?;
        let d = p.dla.as_mut().filter(|d| d.volume_serial != 0).ok_or(Error::NotFound)?;
        d.volume_serial = 0;
        d.volume_name.clear();
        d.letter = 0;
        d.on_boot_menu = false;
        d.installable = false;
        Ok(())
    }

    pub fn rename_partition(&mut self, i: usize, name: &str) -> Result<()> {
        let p = self.parts.get_mut(i).ok_or(Error::NotFound)?;
        p.name = String::from(name);
        if let Some(d) = p.dla.as_mut() {
            d.partition_name = String::from(name);
        }
        Ok(())
    }

    /// Index of the partition holding the volume `name`.
    pub fn volume_index(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.volume().is_some_and(|v| v.volume_name.eq_ignore_ascii_case(name)))
    }

    pub fn partition_index(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Write the tables back. `boot_code` is put in an MBR that has none.
    pub fn write(&self, d: &dyn Disk, seed: &mut u32) -> Result<()> {
        match self.scheme {
            Scheme::Gpt => self.write_gpt(d),
            Scheme::Empty => Ok(()),
            Scheme::Mbr => self.write_mbr(d, seed),
        }
    }

    fn chs(lba: u64) -> [u8; 3] {
        let c = lba / CYLINDER;
        if c > 1023 {
            return [0xfe, 0xff, 0xff];
        }
        let h = (lba / SPT) % HEADS;
        let s = lba % SPT + 1;
        [h as u8, ((c >> 2) & 0xc0) as u8 | s as u8, c as u8]
    }

    fn entry(t: &mut [u8], slot: usize, active: bool, ty: u8, start: u64, count: u64, rel: u64) {
        let e = 446 + slot * 16;
        t[e] = if active { 0x80 } else { 0 };
        t[e + 1..e + 4].copy_from_slice(&Self::chs(start));
        t[e + 4] = ty;
        t[e + 5..e + 8].copy_from_slice(&Self::chs(start + count - 1));
        put32(t, e + 8, (start - rel) as u32);
        put32(t, e + 12, count as u32);
    }

    fn write_mbr(&self, d: &dyn Disk, seed: &mut u32) -> Result<()> {
        let mut head = self.dla.clone();
        if head.disk_serial == 0 {
            head.disk_serial = serial(seed);
        }
        if head.disk_name.is_empty() {
            head.disk_name = String::from("[ D1 ]");
        }
        let cyl = (self.sectors / CYLINDER) as u32;
        let mut mbr = rd(d, 0)?;
        mbr[446..510].fill(0);
        mbr[510] = 0x55;
        mbr[511] = 0xaa;
        let prim: Vec<&Partition> = self.parts.iter().filter(|p| p.kind == Kind::Primary).collect();
        let logi: Vec<&Partition> = self.parts.iter().filter(|p| p.kind == Kind::Logical).collect();
        if prim.len() + (!logi.is_empty()) as usize > 4 {
            return Err(Error::TableFull);
        }
        // Slots in disk order, the extended partition where its first
        // logical partition is.
        let ext = logi.first().map(|f| {
            let s = f.start - SPT;
            let e = logi.last().map(|l| l.start + l.count).unwrap();
            (s, e - s)
        });
        let mut slots: Vec<(u64, Option<&Partition>)> = prim.iter().map(|p| (p.start, Some(*p))).collect();
        if let Some((s, _)) = ext {
            slots.push((s, None));
        }
        slots.sort_by_key(|x| x.0);
        let mut top: Vec<DlaEntry> = Vec::new();
        for (i, (_, p)) in slots.iter().enumerate() {
            match p {
                Some(p) => {
                    Self::entry(&mut mbr, i, p.active, p.mbr_type, p.start, p.count, 0);
                    top.push(self.dla_of(p));
                }
                None => {
                    let (s, c) = ext.unwrap();
                    Self::entry(&mut mbr, i, false, 0x05, s, c, 0);
                }
            }
        }
        d.write(0, &mbr).map_err(Error::Io)?;
        let mut t = make_dla(&head, &top);
        seal_dla(&mut t, cyl);
        d.write(SPT - 1, &t).map_err(Error::Io)?;
        // The EBR chain.
        if let Some((ext_start, _)) = ext {
            for (k, p) in logi.iter().enumerate() {
                let ebr_lba = p.start - SPT;
                let mut ebr = vec![0u8; SECTOR];
                ebr[510] = 0x55;
                ebr[511] = 0xaa;
                Self::entry(&mut ebr, 0, p.active, p.mbr_type, p.start, p.count, ebr_lba);
                if let Some(n) = logi.get(k + 1) {
                    let ns = n.start - SPT;
                    Self::entry(&mut ebr, 1, false, 0x05, ns, n.start + n.count - ns, ext_start);
                }
                d.write(ebr_lba, &ebr).map_err(Error::Io)?;
                // Clear the rest of the EBR track, then its DLA table.
                let z = vec![0u8; SECTOR];
                for s in 1..SPT - 1 {
                    d.write(ebr_lba + s, &z).map_err(Error::Io)?;
                }
                let mut t = make_dla(&head, &[self.dla_of(p)]);
                seal_dla(&mut t, cyl);
                d.write(ebr_lba + SPT - 1, &t).map_err(Error::Io)?;
            }
        }
        Ok(())
    }

    fn dla_of(&self, p: &Partition) -> DlaEntry {
        let mut e = p.dla.clone().unwrap_or_default();
        e.start = p.start as u32;
        e.size = p.count as u32;
        e.partition_name = p.name.clone();
        e
    }

    fn write_gpt(&self, d: &dyn Disk) -> Result<()> {
        let h = rd(d, 1)?;
        let n = (le32(&h, 80) as usize).max(128);
        let esz = 128usize;
        let mut ents = vec![0u8; (n * esz).div_ceil(SECTOR) * SECTOR];
        for (i, p) in self.parts.iter().enumerate().take(n) {
            let e = &mut ents[i * esz..(i + 1) * esz];
            e[0..16].copy_from_slice(&p.gpt_type);
            e[16..32].copy_from_slice(&p.guid);
            put64(e, 32, p.start);
            put64(e, 40, p.start + p.count - 1);
            for (j, ch) in p.name.encode_utf16().take(36).enumerate() {
                e[56 + j * 2..58 + j * 2].copy_from_slice(&ch.to_le_bytes());
            }
        }
        let ents_crc = crc32(&ents[..n * esz]);
        let last = self.sectors - 1;
        let secs = ents.len() as u64 / SECTOR as u64;
        let header = |my: u64, alt: u64, at: u64| {
            let mut x = h.clone();
            put64(&mut x, 24, my);
            put64(&mut x, 32, alt);
            put64(&mut x, 72, at);
            put32(&mut x, 80, n as u32);
            put32(&mut x, 84, esz as u32);
            put32(&mut x, 88, ents_crc);
            put32(&mut x, 16, 0);
            let c = crc32(&x[..92]);
            put32(&mut x, 16, c);
            x
        };
        d.write(2, &ents).map_err(Error::Io)?;
        d.write(1, &header(1, last, 2)).map_err(Error::Io)?;
        d.write(last - secs, &ents).map_err(Error::Io)?;
        d.write(last, &header(last, 1, last - secs)).map_err(Error::Io)?;
        Ok(())
    }

    /// Replace the partition tables with an empty MBR (keeping any boot
    /// code); every partition is lost.
    pub fn new_mbr(d: &dyn Disk) -> Result<()> {
        let mut mbr = rd(d, 0)?;
        mbr[446..510].fill(0);
        mbr[510] = 0x55;
        mbr[511] = 0xaa;
        d.write(0, &mbr).map_err(Error::Io)?;
        // A GPT header left behind would still be found.
        let z = vec![0u8; SECTOR];
        d.write(1, &z).map_err(Error::Io)?;
        d.write(SPT - 1, &z).map_err(Error::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
