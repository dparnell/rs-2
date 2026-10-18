//! Device drivers.

pub mod ahci;
pub mod part;
pub mod pci;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::fs::block::BlockDev;
use crate::sync::SpinLock;

/// Physical disks (for DosPhysicalDisk and the installer).
pub static DISKS: SpinLock<Vec<Arc<dyn BlockDev>>> = SpinLock::new(Vec::new());

/// MBR partition types holding FAT file systems.
fn is_fat_type(t: u8) -> bool {
    matches!(t, 0x01 | 0x04 | 0x06 | 0x0b | 0x0c | 0x0e | 0x14 | 0x16 | 0x1b | 0x1c | 0x1e | 0xef)
}

fn next_letter() -> Option<u8> {
    (2..26u8).find(|&d| crate::fs::drive(d).is_none())
}

fn try_mount_fat(dev: Arc<dyn BlockDev>, what: &str) -> bool {
    match crate::fs::fatfs::FatFs::mount(dev) {
        Ok(fs) => {
            if let Some(d) = next_letter() {
                crate::println!("{what}");
                crate::fs::mount(d, fs);
            }
            true
        }
        Err(_) => false,
    }
}

/// Find storage, scan partitions and assign drive letters: hard disk
/// volumes first (primary partitions, then logical drives), then optical
/// drives — the order OS/2 uses.
pub fn init() {
    let devs = pci::enumerate();
    crate::println!("pci: {} functions", devs.len());
    let mut disks: Vec<Arc<dyn BlockDev>> = Vec::new();
    let mut optical: Vec<Arc<dyn BlockDev>> = Vec::new();
    for d in &devs {
        if (d.class, d.subclass, d.progif) == (1, 6, 1) {
            for p in ahci::probe(d) {
                match p.kind {
                    ahci::Kind::Ata => disks.push(p),
                    ahci::Kind::Atapi => optical.push(p),
                }
            }
        }
    }
    let mut parts: Vec<part::Partition> = Vec::new();
    for disk in &disks {
        let found = part::scan(disk);
        if found.is_empty() {
            // Unpartitioned ("superfloppy") volume.
            try_mount_fat(disk.clone(), "disk: unpartitioned FAT volume");
        }
        parts.extend(found);
    }
    // Primary partitions, then logical drives.
    parts.sort_by_key(|p| !p.primary);
    for p in parts {
        let fat = if p.mbr_type != 0 {
            is_fat_type(p.mbr_type)
        } else {
            p.gpt_type == part::GPT_BASIC_DATA || p.gpt_type == part::GPT_EFI_SYSTEM
        };
        if !fat {
            if matches!(p.mbr_type, 0x07 | 0x35) {
                crate::println!("disk: partition type {:#x} (HPFS/JFS) not supported yet", p.mbr_type);
            }
            continue;
        }
        let t = p.mbr_type;
        try_mount_fat(Arc::new(p), &alloc::format!("disk: FAT partition type {t:#x}"));
    }
    for cd in optical {
        if cd.sector_count() == 0 {
            continue;
        }
        match crate::fs::iso9660::Iso9660::mount(cd.clone()) {
            Ok(fs) => {
                if let Some(d) = next_letter() {
                    crate::fs::mount(d, fs);
                }
            }
            Err(_) => crate::println!("cd: unrecognised medium"),
        }
    }
    *DISKS.lock() = disks;
}
