//! Whole diskettes for DISKCOPY and DISKCOMP: a drive opened for DASD
//! access (its sectors through the track IOCtls), or a diskette image
//! file (`DISKCOPY A: B.IMG` and the like make and write images).

use alloc::string::String;
use alloc::vec::Vec;

use crate::disk::Disk;
use os2::BiosParameterBlock;

pub enum Medium {
    Drive(Disk),
    Image(String),
}

/// The largest diskette (2.88 MB): bigger volumes are hard disks, which
/// DISKCOPY and DISKCOMP refuse, as on OS/2.
pub const MAX_SECTORS: u64 = 5760;

impl Medium {
    /// `d:` or a file name.
    pub fn open(arg: &str, write: bool) -> Result<Medium, u32> {
        let b = arg.as_bytes();
        if b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
            let d = Disk::open(b[0].to_ascii_uppercase() - b'A', write)?;
            if d.sectors > MAX_SECTORS {
                return Err(1231);
            }
            Ok(Medium::Drive(d))
        } else {
            Ok(Medium::Image(String::from(arg)))
        }
    }

    pub fn is_drive(&self) -> bool {
        matches!(self, Medium::Drive(_))
    }

    /// Read the whole medium.
    pub fn read(&self) -> Result<Vec<u8>, u32> {
        match self {
            Medium::Drive(d) => {
                let mut v = alloc::vec![0u8; d.sectors as usize * d.bps];
                d.read(0, &mut v)?;
                Ok(v)
            }
            Medium::Image(p) => crate::files::read_file(p),
        }
    }

    /// Write a whole medium (a drive: exactly its size).
    pub fn write(&mut self, data: &[u8]) -> Result<(), u32> {
        match self {
            Medium::Drive(d) if d.removable() => {
                // A diskette drive: written in the source's format, track
                // by track; a track that does not take the data (a new
                // diskette) is formatted first.
                let (tracks, spt, heads) = geometry(data);
                let bpb = BiosParameterBlock {
                    bytes_per_sector: 512,
                    sectors: (data.len() / 512) as u16,
                    sectors_per_track: spt as u16,
                    heads: heads as u16,
                    ..Default::default()
                };
                let rc = d.lock();
                if rc != 0 {
                    return Err(rc);
                }
                let r = (|| {
                    if d.set_medium(bpb) != 0 {
                        return Err(1240);
                    }
                    let per = spt as usize * 512;
                    for t in 0..(tracks * heads) as u64 {
                        let piece = &data[t as usize * per..(t as usize + 1) * per];
                        if d.write(t * spt as u64, piece).is_err() {
                            let rc = d.format_track(t);
                            if rc != 0 {
                                return Err(rc);
                            }
                            d.write(t * spt as u64, piece)?;
                        }
                    }
                    Ok(())
                })();
                d.unlock();
                d.redetermine();
                r
            }
            Medium::Drive(d) => {
                if data.len() != d.sectors as usize * d.bps {
                    return Err(1240);
                }
                let rc = d.lock();
                if rc != 0 {
                    return Err(rc);
                }
                let r = d.write(0, data);
                d.unlock();
                d.redetermine();
                r
            }
            Medium::Image(p) => crate::files::write_file(p, data),
        }
    }
}

/// Tracks, sectors per track and sides of a diskette image, from its
/// boot sector's BPB or, failing that, from its size.
pub fn geometry(data: &[u8]) -> (u32, u32, u32) {
    if data.len() >= 512 && data[510] == 0x55 && data[511] == 0xaa {
        let spt = u16::from_le_bytes([data[24], data[25]]) as u32;
        let heads = u16::from_le_bytes([data[26], data[27]]) as u32;
        let total = u16::from_le_bytes([data[19], data[20]]) as u32;
        if spt > 0 && heads > 0 && total > 0 {
            return (total / spt / heads, spt, heads);
        }
    }
    let sectors = (data.len() / 512) as u32;
    let (spt, heads) = match sectors {
        720 => (9, 2),
        1440 => (9, 2),
        2400 => (15, 2),
        2880 => (18, 2),
        5760 => (36, 2),
        _ => (18, 2),
    };
    (sectors / spt / heads, spt, heads)
}
