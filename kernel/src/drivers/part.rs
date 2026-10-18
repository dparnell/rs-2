//! Partition tables: MBR (with extended/logical partitions, as OS/2 uses
//! them) and GPT.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use crate::api::err;
use crate::fs::block::BlockDev;

/// A window onto part of a disk.
pub struct Partition {
    pub dev: Arc<dyn BlockDev>,
    pub start: u64,
    pub count: u64,
    /// MBR partition type (0 for GPT partitions).
    pub mbr_type: u8,
    /// GPT type GUID (zero for MBR partitions).
    pub gpt_type: [u8; 16],
    pub primary: bool,
}

impl BlockDev for Partition {
    fn sector_size(&self) -> usize {
        self.dev.sector_size()
    }

    fn usb_device(&self) -> Option<crate::drivers::xhci::Handle> {
        self.dev.usb_device()
    }

    fn sector_count(&self) -> u64 {
        self.count
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        let n = (buf.len() / self.dev.sector_size()) as u64;
        if lba + n > self.count {
            return Err(err::INVALID_PARAMETER);
        }
        self.dev.read(self.start + lba, buf)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let n = (buf.len() / self.dev.sector_size()) as u64;
        if lba + n > self.count {
            return Err(err::INVALID_PARAMETER);
        }
        self.dev.write(self.start + lba, buf)
    }
}

/// Microsoft basic data / EFI system partition GUIDs (mixed-endian form as
/// stored on disk).
pub const GPT_EFI_SYSTEM: [u8; 16] =
    [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];
pub const GPT_BASIC_DATA: [u8; 16] =
    [0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7];

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn le64(b: &[u8], o: usize) -> u64 {
    le32(b, o) as u64 | (le32(b, o + 4) as u64) << 32
}

fn is_extended(t: u8) -> bool {
    matches!(t, 0x05 | 0x0f | 0x85)
}

/// Scan a disk; returns its partitions (primary first, then logical).
pub fn scan(dev: &Arc<dyn BlockDev>) -> Vec<Partition> {
    let ss = dev.sector_size();
    let mut out = Vec::new();
    let mut mbr = vec![0u8; ss];
    if dev.read(0, &mut mbr).is_err() || mbr[510] != 0x55 || mbr[511] != 0xaa {
        return out;
    }
    let entries: Vec<(u8, u64, u64)> = (0..4)
        .map(|i| {
            let e = 446 + i * 16;
            (mbr[e + 4], le32(&mbr, e + 8) as u64, le32(&mbr, e + 12) as u64)
        })
        .collect();
    if entries.iter().any(|e| e.0 == 0xee) {
        return scan_gpt(dev);
    }
    // A FAT boot sector (no partition table) has a jump instruction and
    // no valid entries: treat the whole disk as one volume elsewhere.
    for &(t, start, count) in &entries {
        if t == 0 || count == 0 || is_extended(t) {
            continue;
        }
        out.push(Partition { dev: dev.clone(), start, count, mbr_type: t, gpt_type: [0; 16], primary: true });
    }
    for &(t, ext_start, _) in &entries {
        if !is_extended(t) {
            continue;
        }
        let mut ebr_lba = ext_start;
        for _ in 0..128 {
            let mut ebr = vec![0u8; ss];
            if dev.read(ebr_lba, &mut ebr).is_err() || ebr[510] != 0x55 || ebr[511] != 0xaa {
                break;
            }
            let (lt, ls, lc) = (ebr[446 + 4], le32(&ebr, 446 + 8) as u64, le32(&ebr, 446 + 12) as u64);
            if lt != 0 && lc != 0 {
                out.push(Partition {
                    dev: dev.clone(),
                    start: ebr_lba + ls,
                    count: lc,
                    mbr_type: lt,
                    gpt_type: [0; 16],
                    primary: false,
                });
            }
            let (nt, ns) = (ebr[462 + 4], le32(&ebr, 462 + 8) as u64);
            if !is_extended(nt) || ns == 0 {
                break;
            }
            ebr_lba = ext_start + ns;
        }
    }
    out
}

fn scan_gpt(dev: &Arc<dyn BlockDev>) -> Vec<Partition> {
    let ss = dev.sector_size();
    let mut out = Vec::new();
    let mut h = vec![0u8; ss];
    if dev.read(1, &mut h).is_err() || &h[0..8] != b"EFI PART" {
        return out;
    }
    let first = le64(&h, 72);
    let n = le32(&h, 80) as usize;
    let esz = le32(&h, 84) as usize;
    if esz < 128 || n > 1024 {
        return out;
    }
    let bytes = (n * esz).next_multiple_of(ss);
    let mut tbl = vec![0u8; bytes];
    if dev.read(first, &mut tbl).is_err() {
        return out;
    }
    for i in 0..n {
        let e = &tbl[i * esz..(i + 1) * esz];
        let mut ty = [0u8; 16];
        ty.copy_from_slice(&e[0..16]);
        if ty == [0; 16] {
            continue;
        }
        let s = le64(e, 32);
        let l = le64(e, 40);
        if l < s {
            continue;
        }
        out.push(Partition { dev: dev.clone(), start: s, count: l - s + 1, mbr_type: 0, gpt_type: ty, primary: true });
    }
    out
}
