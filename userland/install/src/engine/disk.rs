//! Physical disks (DosPhysicalDisk and the category 9 IOCtls) and the GPT
//! the installer writes.

use alloc::vec;
use alloc::vec::Vec;

use os2::*;

unsafe extern "C" {
    #[link_name = "DOSCALLS.287"]
    fn DosPhysicalDisk(func: u32, buf: *mut u8, cb: u32, params: *const u8, cbp: u32) -> APIRET;
    #[link_name = "DOSCALLS.284"]
    pub fn DosDevIOCtl(h: u32, cat: u32, func: u32, params: *const u8, cbp: u32, pcbp: *mut u32, data: *mut u8, cbd: u32, pcbd: *mut u32) -> APIRET;
}

/// A disk, or a window onto part of it (a partition).
#[derive(Clone, Copy)]
pub struct Disk {
    pub handle: u32,
    pub sectors: u64,
    pub sector_size: u32,
    /// Offset added to every LBA (partition start).
    pub offset: u64,
    /// Sectors visible through this view.
    pub count: u64,
}

/// How many hard disks there are.
pub fn count() -> u32 {
    let mut n = 0u16;
    unsafe { DosPhysicalDisk(1, &mut n as *mut u16 as *mut u8, 2, core::ptr::null(), 0) };
    n as u32
}

impl Disk {
    pub fn open(n: u32) -> Option<Disk> {
        let name = alloc::format!("{n}:\0");
        let mut h = 0u16;
        let rc = unsafe { DosPhysicalDisk(2, &mut h as *mut u16 as *mut u8, 2, name.as_ptr(), name.len() as u32) };
        if rc != 0 {
            return None;
        }
        let mut info = [0u8; 12];
        let (mut pl, mut dl) = (0, 0);
        let rc = unsafe { DosDevIOCtl(h as u32, 9, 0xa2, core::ptr::null(), 0, &mut pl, info.as_mut_ptr(), 12, &mut dl) };
        if rc != 0 {
            return None;
        }
        let sectors = u64::from_le_bytes(info[0..8].try_into().unwrap());
        let ss = u32::from_le_bytes(info[8..12].try_into().unwrap());
        Some(Disk { handle: h as u32, sectors, sector_size: ss, offset: 0, count: sectors })
    }

    pub fn mb(&self) -> u64 {
        self.count * self.sector_size as u64 >> 20
    }

    /// The partition from `start` for `count` sectors.
    pub fn part(&self, start: u64, count: u64) -> Disk {
        Disk { offset: start, count, ..*self }
    }

    fn io(&self, lba: u64, buf: *mut u8, bytes: usize, write: bool) -> Result<(), u32> {
        let count = (bytes / self.sector_size as usize) as u32;
        let abs = self.offset + lba;
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&(abs as u32).to_le_bytes());
        p[4..8].copy_from_slice(&((abs >> 32) as u32).to_le_bytes());
        p[8..12].copy_from_slice(&count.to_le_bytes());
        let (mut pl, mut dl) = (0, 0);
        let f = if write { 0xa1 } else { 0xa0 };
        let rc = unsafe { DosDevIOCtl(self.handle, 9, f, p.as_ptr(), 12, &mut pl, buf, bytes as u32, &mut dl) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    pub fn read_at(&self, lba: u64, bytes: usize) -> Result<Vec<u8>, u32> {
        let ss = self.sector_size as usize;
        let mut v = vec![0u8; bytes.div_ceil(ss) * ss];
        self.io(lba, v.as_mut_ptr(), v.len(), false)?;
        v.truncate(bytes);
        Ok(v)
    }

    pub fn write_at(&self, lba: u64, data: &[u8]) -> Result<(), u32> {
        let mut v = data.to_vec();
        self.io(lba, v.as_mut_ptr(), v.len(), true)
    }
}

impl fat::BlockDevice for Disk {
    fn sector_size(&self) -> usize {
        self.sector_size as usize
    }
    fn sector_count(&self) -> u64 {
        self.count
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> fat::Result<()> {
        self.io(lba, buf.as_mut_ptr(), buf.len(), false).map_err(|_| fat::Error::Io)
    }
    fn write(&self, lba: u64, buf: &[u8]) -> fat::Result<()> {
        self.write_at(lba, buf).map_err(|_| fat::Error::Io)
    }
}

impl hpfs::BlockDevice for Disk {
    fn sector_size(&self) -> usize {
        self.sector_size as usize
    }
    fn sector_count(&self) -> u64 {
        self.count
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> hpfs::Result<()> {
        self.io(lba, buf.as_mut_ptr(), buf.len(), false).map_err(|_| hpfs::Error::Io)
    }
    fn write(&self, lba: u64, buf: &[u8]) -> hpfs::Result<()> {
        self.write_at(lba, buf).map_err(|_| hpfs::Error::Io)
    }
}

impl jfs::BlockDevice for Disk {
    fn sector_size(&self) -> usize {
        self.sector_size as usize
    }
    fn sector_count(&self) -> u64 {
        self.count
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> jfs::Result<()> {
        self.io(lba, buf.as_mut_ptr(), buf.len(), false).map_err(|_| jfs::Error::Io)
    }
    fn write(&self, lba: u64, buf: &[u8]) -> jfs::Result<()> {
        self.write_at(lba, buf).map_err(|_| jfs::Error::Io)
    }
}

// ---- GPT ----

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// EFI system partition, C12A7328-F81F-11D2-BA4B-00A0C93EC93B.
pub const ESP_TYPE: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];
/// Basic data partition, EBD0A0A2-B9E5-4433-87C0-68B6B72699C7: the
/// system partition (HPFS or JFS).
pub const DATA_TYPE: [u8; 16] = [0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7];

/// A partition GUID from a seed (the time of day: unique enough on one
/// machine).
pub fn guid(seed: u32) -> [u8; 16] {
    let mut g = [0u8; 16];
    let mut x = seed ^ 0x9e37_79b9;
    for b in g.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x as u8;
    }
    g[7] = (g[7] & 0x0f) | 0x40; // version 4
    g[8] = (g[8] & 0x3f) | 0x80;
    g
}

/// A GUID as the kernel's root= takes it (the first three fields
/// little-endian, as GPT stores them).
pub fn guid_text(g: &[u8; 16]) -> alloc::string::String {
    alloc::format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g[3], g[2], g[1], g[0], g[5], g[4], g[7], g[6], g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
    )
}

#[derive(Clone, Copy)]
pub struct NewPart {
    pub kind: [u8; 16],
    pub guid: [u8; 16],
    pub start: u64,
    pub count: u64,
    pub name: &'static str,
}

/// The usable sectors of a GPT on `d`: (first, last).
pub fn usable(d: &Disk) -> (u64, u64) {
    let entry_sectors = (128 * 128) / d.sector_size as u64;
    (2048, d.sectors - 1 - entry_sectors - 1)
}

/// Write a protective MBR and a GPT (both copies) with these partitions.
pub fn write_gpt(d: &Disk, parts: &[NewPart], seed: u32) -> Result<(), u32> {
    let ss = d.sector_size as usize;
    let last = d.sectors - 1;
    let entries_lba = 2u64;
    let entry_sectors = (128 * 128) / ss as u64;
    let (first_usable, last_usable) = usable(d);
    let mut mbr = vec![0u8; ss];
    mbr[446 + 1..446 + 4].copy_from_slice(&[0, 2, 0]);
    mbr[446 + 4] = 0xee;
    mbr[446 + 5..446 + 8].copy_from_slice(&[0xff, 0xff, 0xff]);
    mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    mbr[446 + 12..446 + 16].copy_from_slice(&((last.min(0xffff_ffff)) as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    d.write_at(0, &mbr)?;
    let mut ents = vec![0u8; (entry_sectors as usize) * ss];
    for (i, p) in parts.iter().enumerate() {
        let e = &mut ents[i * 128..(i + 1) * 128];
        e[0..16].copy_from_slice(&p.kind);
        e[16..32].copy_from_slice(&p.guid);
        e[32..40].copy_from_slice(&p.start.to_le_bytes());
        e[40..48].copy_from_slice(&(p.start + p.count - 1).to_le_bytes());
        for (j, ch) in p.name.encode_utf16().take(36).enumerate() {
            e[56 + j * 2..58 + j * 2].copy_from_slice(&ch.to_le_bytes());
        }
    }
    let ents_crc = crc32(&ents[..128 * 128]);
    let disk_guid = guid(seed.wrapping_add(0x5a5a));
    let header = |my: u64, alt: u64, ents_at: u64| -> Vec<u8> {
        let mut h = vec![0u8; ss];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&my.to_le_bytes());
        h[32..40].copy_from_slice(&alt.to_le_bytes());
        h[40..48].copy_from_slice(&first_usable.to_le_bytes());
        h[48..56].copy_from_slice(&last_usable.to_le_bytes());
        h[56..72].copy_from_slice(&disk_guid);
        h[72..80].copy_from_slice(&ents_at.to_le_bytes());
        h[80..84].copy_from_slice(&128u32.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&ents_crc.to_le_bytes());
        let c = crc32(&h[..92]);
        h[16..20].copy_from_slice(&c.to_le_bytes());
        h
    };
    d.write_at(entries_lba, &ents)?;
    d.write_at(1, &header(1, last, entries_lba))?;
    let backup_ents = last - entry_sectors;
    d.write_at(backup_ents, &ents)?;
    d.write_at(last, &header(last, 1, backup_ents))?;
    Ok(())
}

/// The GPT partitions of a disk: (type, unique GUID, start, count).
pub fn read_gpt(d: &Disk) -> Vec<([u8; 16], [u8; 16], u64, u64)> {
    let mut out = Vec::new();
    let Ok(h) = d.read_at(1, d.sector_size as usize) else { return out };
    if &h[0..8] != b"EFI PART" {
        return out;
    }
    let u64_at = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let (lba, n, size) = (u64_at(&h, 72), u32_at(&h, 80).min(256) as usize, u32_at(&h, 84) as usize);
    if size < 128 {
        return out;
    }
    let Ok(ents) = d.read_at(lba, n * size) else { return out };
    for e in ents.chunks_exact(size) {
        let mut t = [0u8; 16];
        t.copy_from_slice(&e[0..16]);
        if t == [0; 16] {
            continue;
        }
        let mut g = [0u8; 16];
        g.copy_from_slice(&e[16..32]);
        let (start, end) = (u64_at(e, 32), u64_at(e, 40));
        if end < start || end >= d.sectors {
            continue;
        }
        out.push((t, g, start, end - start + 1));
    }
    out
}

/// Write the disk's caches out and restart the computer (DOS$ category
/// 0xD5 function 0xAB).
pub fn restart() {
    let c = os2::path::cstr("DOS$");
    let (mut h, mut act) = (0, 0);
    unsafe {
        if DosOpen(c.as_ptr(), &mut h, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, core::ptr::null_mut()) == 0 {
            let (mut pl, mut dl) = (0, 0);
            DosDevIOCtl(h, 0xd5, 0xab, core::ptr::null(), 0, &mut pl, core::ptr::null_mut(), 0, &mut dl);
        }
    }
}
