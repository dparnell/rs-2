//! ISO 9660 (CDFS) with Joliet long names; read only.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use super::block::BlockDev;
use super::{attr, FileInfo, FileSystem, FsFile, SetInfo, Stamp, VolumeInfo};
use crate::api::err;

const SECTOR: usize = 2048;

#[derive(Clone)]
struct Rec {
    name: String,
    lba: u32,
    size: u32,
    dir: bool,
    hidden: bool,
    stamp: Stamp,
}

pub struct Iso9660 {
    dev: Arc<dyn BlockDev>,
    root: Rec,
    joliet: bool,
    label: String,
    blocks: u32,
}

/// Convert ISO directory date (7 bytes) to an FDATE/FTIME pair.
fn stamp(d: &[u8]) -> Stamp {
    let y = 1900 + d[0] as u16;
    let f = fat::Timestamp::new(y, d[1], d[2], d[3], d[4], d[5]);
    Stamp { date: f.date, time: f.time }
}

impl Iso9660 {
    pub fn mount(dev: Arc<dyn BlockDev>) -> Result<Arc<dyn FileSystem>, u32> {
        if dev.sector_size() != SECTOR {
            return Err(err::INVALID_PARAMETER);
        }
        let mut pvd = None;
        let mut svd = None;
        for lba in 16..64u64 {
            let mut s = vec![0u8; SECTOR];
            dev.read(lba, &mut s)?;
            if &s[1..6] != b"CD001" {
                return Err(err::INVALID_PARAMETER);
            }
            match s[0] {
                1 => pvd = Some(s),
                2 if s[88] == 0x25 && s[89] == 0x2f && matches!(s[90], 0x40 | 0x43 | 0x45) => svd = Some(s),
                255 => break,
                _ => {}
            }
        }
        let pvd = pvd.ok_or(err::INVALID_PARAMETER)?;
        let joliet = svd.is_some();
        let vd = svd.as_ref().unwrap_or(&pvd);
        let root = parse_rec(&vd[156..190], joliet).ok_or(err::INVALID_PARAMETER)?;
        let label: String = pvd[40..72].iter().map(|&b| b as char).collect::<String>().trim_end().into();
        let blocks = u32::from_le_bytes([pvd[80], pvd[81], pvd[82], pvd[83]]);
        Ok(Arc::new(Iso9660 { dev, root, joliet, label, blocks }))
    }

    fn read_dir(&self, d: &Rec) -> Result<Vec<Rec>, u32> {
        let mut out = Vec::new();
        let sectors = d.size.div_ceil(SECTOR as u32);
        let mut buf = vec![0u8; SECTOR];
        for s in 0..sectors {
            self.dev.read((d.lba + s) as u64, &mut buf)?;
            let mut o = 0;
            while o < SECTOR {
                let len = buf[o] as usize;
                if len == 0 {
                    break;
                }
                if o + len > SECTOR {
                    break;
                }
                if let Some(r) = parse_rec(&buf[o..o + len], self.joliet) {
                    if r.name != "\u{0}" && r.name != "\u{1}" {
                        out.push(r);
                    }
                }
                o += len;
            }
        }
        Ok(out)
    }

    fn lookup(&self, path: &str) -> Result<Rec, u32> {
        let mut cur = self.root.clone();
        let parts: Vec<&str> = path.split('\\').filter(|s| !s.is_empty()).collect();
        for (i, p) in parts.iter().enumerate() {
            if !cur.dir {
                return Err(err::PATH_NOT_FOUND);
            }
            let list = self.read_dir(&cur)?;
            match list.into_iter().find(|r| r.name.eq_ignore_ascii_case(p)) {
                Some(r) => cur = r,
                None => return Err(if i + 1 == parts.len() { err::FILE_NOT_FOUND } else { err::PATH_NOT_FOUND }),
            }
        }
        Ok(cur)
    }
}

fn parse_rec(r: &[u8], joliet: bool) -> Option<Rec> {
    if r.len() < 34 {
        return None;
    }
    let lba = u32::from_le_bytes([r[2], r[3], r[4], r[5]]);
    let size = u32::from_le_bytes([r[10], r[11], r[12], r[13]]);
    let flags = r[25];
    let nlen = r[32] as usize;
    let raw = r.get(33..33 + nlen)?;
    let name = if nlen == 1 && raw[0] <= 1 {
        String::from(raw[0] as char)
    } else if joliet {
        let u: Vec<u16> = raw.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        char::decode_utf16(u).map(|c| c.unwrap_or('?')).collect()
    } else {
        raw.iter().map(|&b| b as char).collect()
    };
    let name = name.split(';').next().unwrap_or("").trim_end_matches('.').into();
    Some(Rec { name, lba, size, dir: flags & 2 != 0, hidden: flags & 1 != 0, stamp: stamp(&r[18..25]) })
}

fn info(r: &Rec) -> FileInfo {
    let mut a = attr::READONLY;
    if r.dir {
        a |= attr::DIRECTORY;
    }
    if r.hidden {
        a |= attr::HIDDEN;
    }
    FileInfo {
        name: r.name.clone(),
        attr: a,
        size: if r.dir { 0 } else { r.size as u64 },
        alloc: (r.size as u64).next_multiple_of(SECTOR as u64),
        created: r.stamp,
        accessed: r.stamp,
        modified: r.stamp,
        ea_size: 0,
    }
}

impl FileSystem for Iso9660 {
    fn name(&self) -> &'static str {
        "CDFS"
    }

    fn stat(&self, path: &str) -> Result<FileInfo, u32> {
        let r = self.lookup(path)?;
        let mut i = info(&r);
        if path.is_empty() {
            i.name.clear();
        }
        Ok(i)
    }

    fn list(&self, dir: &str) -> Result<Vec<FileInfo>, u32> {
        let d = self.lookup(dir)?;
        if !d.dir {
            return Err(err::PATH_NOT_FOUND);
        }
        Ok(self.read_dir(&d)?.iter().map(info).collect())
    }

    fn open(&self, path: &str) -> Result<Arc<dyn FsFile>, u32> {
        let r = self.lookup(path)?;
        if r.dir {
            return Err(err::ACCESS_DENIED);
        }
        Ok(Arc::new(IsoFile { dev: self.dev.clone(), rec: r }))
    }

    fn create(&self, _: &str, _: u32) -> Result<Arc<dyn FsFile>, u32> {
        Err(err::WRITE_PROTECT)
    }
    fn delete(&self, _: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn mkdir(&self, _: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn rmdir(&self, _: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn rename(&self, _: &str, _: &str) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
    fn set_info(&self, _: &str, _: &SetInfo) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }

    fn volume(&self) -> Result<VolumeInfo, u32> {
        Ok(VolumeInfo {
            serial: self.root.lba.wrapping_mul(0x9e37_79b9),
            label: self.label.clone(),
            sectors_per_unit: 1,
            units: self.blocks,
            free_units: 0,
            bytes_per_sector: SECTOR as u32,
        })
    }
}

struct IsoFile {
    dev: Arc<dyn BlockDev>,
    rec: Rec,
}

impl FsFile for IsoFile {
    fn read(&self, off: u64, buf: &mut [u8]) -> Result<usize, u32> {
        let size = self.rec.size as u64;
        if off >= size {
            return Ok(0);
        }
        let len = buf.len().min((size - off) as usize);
        let first = off / SECTOR as u64;
        let last = (off + len as u64 - 1) / SECTOR as u64;
        let n = (last - first + 1) as usize;
        let mut tmp = vec![0u8; n * SECTOR];
        self.dev.read(self.rec.lba as u64 + first, &mut tmp)?;
        let s = (off % SECTOR as u64) as usize;
        buf[..len].copy_from_slice(&tmp[s..s + len]);
        Ok(len)
    }

    fn write(&self, _: u64, _: &[u8]) -> Result<usize, u32> {
        Err(err::WRITE_PROTECT)
    }

    fn size(&self) -> u64 {
        self.rec.size as u64
    }

    fn set_size(&self, _: u64) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }

    fn info(&self) -> FileInfo {
        info(&self.rec)
    }

    fn set_info(&self, _: &SetInfo) -> Result<(), u32> {
        Err(err::WRITE_PROTECT)
    }
}
