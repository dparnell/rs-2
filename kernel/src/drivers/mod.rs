//! Device drivers.

pub mod ac97;
pub mod ahci;
pub mod e1000;
pub mod es137x;
pub mod hda;
pub mod hid;
pub mod hotplug;
pub mod hub;
pub mod ide;
pub mod mpt;
pub mod nvme;
pub mod part;
pub mod pci;
pub mod pcnet;
pub mod pcilist;
pub mod pvscsi;
pub mod scsi;
pub mod storage;
pub mod svga;
pub mod usb;
pub mod vmmouse;
pub mod virtio;
pub mod vmxnet3;
pub mod xhci;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::fs::block::BlockDev;
use crate::sync::SpinLock;

/// Physical disks (for DosPhysicalDisk and the installer).
pub static DISKS: SpinLock<Vec<Arc<dyn BlockDev>>> = SpinLock::new(Vec::new());

/// Commit the write caches of all disks (DosResetBuffer, DosShutdown).
pub fn flush_disks() {
    let disks = DISKS.lock().clone();
    for d in disks {
        let _ = d.flush();
    }
}

/// MBR partition types holding FAT file systems.
fn is_fat_type(t: u8) -> bool {
    matches!(t, 0x01 | 0x04 | 0x06 | 0x0b | 0x0c | 0x0e | 0x14 | 0x16 | 0x1b | 0x1c | 0x1e | 0xef)
}

pub(crate) fn next_letter() -> Option<u8> {
    (2..26u8).find(|&d| crate::fs::drive(d).is_none())
}

fn try_mount_fat(dev: Arc<dyn BlockDev>, what: &str) -> bool {
    let dev = crate::api::harderr::wrap(dev);
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

/// Mount `dev` with the HPFS driver if it holds an HPFS volume.
fn try_mount_hpfs(dev: Arc<dyn BlockDev>, what: &str) -> bool {
    if !crate::fs::hpfsfs::probe(&dev) {
        return false;
    }
    let dev = crate::api::harderr::wrap(dev);
    match crate::fs::hpfsfs::HpfsFs::mount(dev) {
        Ok(fs) => {
            if let Some(d) = next_letter() {
                crate::println!("{what}");
                crate::fs::mount(d, fs);
            }
        }
        Err(e) => crate::println!("{what}: cannot mount ({e})"),
    }
    true
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Whether partition `p` carries the boot device's signature: the MBR
/// disk signature or the GPT unique partition GUID.
fn has_boot_signature(p: &part::Partition, boot: &bootinfo::BootDevice) -> bool {
    let ss = p.dev.sector_size();
    let mut s = alloc::vec![0u8; ss];
    match boot.sig_type {
        1 => p.dev.read(0, &mut s).is_ok() && s[440..444] == boot.signature[..4],
        2 => {
            if p.dev.read(1, &mut s).is_err() || &s[0..8] != b"EFI PART" {
                return false;
            }
            let lba = le32(&s, 72) as u64 | (le32(&s, 76) as u64) << 32;
            let (count, size) = (le32(&s, 80).min(256) as usize, le32(&s, 84) as usize);
            if !(128..=ss).contains(&size) {
                return false;
            }
            let mut e = alloc::vec![0u8; ss];
            (0..count).any(|n| {
                let off = n * size;
                p.dev.read(lba + (off / ss) as u64, &mut e).is_ok() && {
                    let x = &e[off % ss..off % ss + size];
                    le32(x, 32) as u64 | (le32(x, 36) as u64) << 32 == p.start && x[16..32] == boot.signature
                }
            })
        }
        _ => false,
    }
}

/// Whether `p` is a FAT partition with \RS2\KERNEL on it.
fn holds_kernel(p: &part::Partition) -> bool {
    if !is_fat_type(p.mbr_type) && p.gpt_type != part::GPT_BASIC_DATA && p.gpt_type != part::GPT_EFI_SYSTEM {
        return false;
    }
    let w = part::Partition { dev: p.dev.clone(), ..*p };
    crate::fs::fatfs::FatFs::mount(Arc::new(w)).is_ok_and(|fs| fs.stat("RS2\\KERNEL").is_ok())
}

/// Find storage, scan partitions and assign drive letters: hard disk
/// volumes first (primary partitions, then logical drives), then optical
/// drives — the order OS/2 uses.
pub fn init() {
    let devs = pci::enumerate();
    crate::println!("pci: {} functions", devs.len());
    pcilist::log(&devs);
    let mut disks: Vec<Arc<dyn BlockDev>> = Vec::new();
    let mut optical: Vec<Arc<dyn BlockDev>> = Vec::new();
    // USB storage comes after the built-in drives.
    let (mut usb_disks, mut usb_optical) = (Vec::new(), Vec::new());
    for d in &devs {
        if (d.class, d.subclass, d.progif) == (1, 6, 1) {
            for p in ahci::probe(d) {
                match p.kind {
                    ahci::Kind::Ata => disks.push(p),
                    ahci::Kind::Atapi => optical.push(p),
                }
            }
        }
        if (d.class, d.subclass) == (1, 1) {
            for dr in ide::probe(d) {
                match dr.kind {
                    ide::Kind::Ata => disks.push(dr),
                    ide::Kind::Atapi => optical.push(dr),
                }
            }
        }
        if (d.class, d.subclass, d.progif) == (0x0c, 3, 0x30) {
            for (dev, is_optical) in xhci::probe(d) {
                if is_optical { usb_optical.push(dev) } else { usb_disks.push(dev) }
            }
        }
        if (d.class, d.subclass, d.progif) == (1, 8, 2) {
            for ns in nvme::probe(d) {
                disks.push(ns);
            }
        }
        // Sound: the first device that comes up is used.
        if !crate::audio::present() {
            use crate::audio::{Input, Output};
            let found = if (d.class, d.subclass) == (4, 3) {
                hda::probe(d).map(|(o, i)| (Output::Hda(o), i.map(Input::Hda)))
            } else if ac97::is_ac97(d) {
                ac97::probe(d).map(|(o, i)| (Output::Ac97(o), i.map(Input::Ac97)))
            } else if es137x::is_es137x(d) {
                es137x::probe(d).map(|(o, i)| (Output::Es137x(o), i.map(Input::Es137x)))
            } else {
                None
            };
            if let Some((out, inp)) = found {
                crate::audio::attach(out, inp);
            }
        }
        if e1000::model(d).is_some() {
            e1000::probe(d);
        }
        if vmxnet3::is_vmxnet3(d) {
            vmxnet3::probe(d);
        }
        if pcnet::is_pcnet(d) {
            pcnet::probe(d);
        }
        if (d.vendor, d.device) == (0x15ad, 0x0405) {
            svga::probe(d);
        }
        let units = match (d.vendor, d.device) {
            (0x15ad, 0x07c0) => pvscsi::probe(d),
            // LSI Logic 53c1030 and SAS1068 (MPT Fusion).
            (0x1000, 0x0030 | 0x0054) => mpt::probe(d),
            _ => Vec::new(),
        };
        for (dev, is_optical) in units {
            if is_optical { optical.push(dev) } else { disks.push(dev) }
        }
    }
    // Paravirtual (virtio) storage counts as built in.
    let mut vio = virtio::probe(&devs);
    disks.append(&mut vio.disks);
    optical.append(&mut vio.optical);
    disks.append(&mut usb_disks);
    optical.append(&mut usb_optical);
    let mut parts: Vec<part::Partition> = Vec::new();
    for disk in &disks {
        let found = part::scan(disk);
        if found.is_empty() {
            // Unpartitioned ("superfloppy") volume.
            if !try_mount_fat(disk.clone(), "disk: unpartitioned FAT volume")
                && !try_mount_hpfs(disk.clone(), "disk: unpartitioned HPFS volume")
                && crate::fs::jfsfs::probe(disk)
            {
                if let Some((fs, _)) = crate::fs::probe_mount(disk.clone()) {
                    if let Some(d) = next_letter() {
                        crate::println!("disk: unpartitioned JFS volume");
                        crate::fs::mount(d, fs);
                    }
                }
            }
        }
        parts.extend(found);
    }
    // The boot partition is C: unless the loader brought a C.IMG: the one
    // the firmware booted from, by position, signature and, to tell apart
    // disks with the same layout, \RS2\KERNEL on it; failing that, the
    // first FAT partition holding \RS2\KERNEL.
    let boot = crate::boot_info().boot_device;
    // root=GUID (the installer writes it into the EFI system partition's
    // \RS2\CMDLINE when the system is on its own partition, HPFS or
    // JFS): that GPT partition is C:, whatever its file system.
    let root = crate::boot_info().cmdline().split_whitespace().find_map(|w| w.strip_prefix("root=")).map(|g| g.to_ascii_lowercase());
    let mut have_c = false;
    if let Some(g) = root.filter(|_| !crate::fs::has_boot_image(crate::boot_info())) {
        match parts.iter().position(|p| part::guid_text(&p.guid) == g) {
            Some(i) => {
                let p = parts.remove(i);
                match crate::fs::probe_mount(Arc::new(p)) {
                    Some((fs, name)) => {
                        crate::println!("boot partition {g}: {name}");
                        crate::fs::mount(2, fs);
                        have_c = true;
                    }
                    None => crate::println!("boot partition {g}: no file system rs2 can mount"),
                }
            }
            None => crate::println!("boot partition {g} not found"),
        }
    }
    if !have_c && !crate::fs::has_boot_image(crate::boot_info()) {
        let same = |p: &part::Partition| boot.kind == 1 && p.start == boot.start && p.count == boot.size;
        let signed = |p: &part::Partition| same(p) && has_boot_signature(p, &boot);
        let find = |f: &dyn Fn(&part::Partition) -> bool| parts.iter().position(f);
        let found = find(&|p| signed(p) && holds_kernel(p))
            .or_else(|| find(&|p| same(p) && holds_kernel(p)))
            .or_else(|| find(&signed))
            .or_else(|| find(&same))
            .or_else(|| find(&holds_kernel));
        match found {
            Some(i) => match crate::fs::fatfs::FatFs::mount(crate::api::harderr::wrap(Arc::new(parts.remove(i)))) {
                Ok(fs) => crate::fs::mount(2, fs),
                Err(e) => crate::println!("boot partition: cannot mount ({e})"),
            },
            None => crate::println!("boot partition not found (start {})", boot.start),
        }
    }
    // Primary partitions, then logical drives.
    parts.sort_by_key(|p| !p.primary);
    for p in parts {
        // GPT data partitions hold any file system: probe them.
        if p.mbr_type == 0 && p.gpt_type == part::GPT_BASIC_DATA {
            match crate::fs::probe_mount(Arc::new(p)) {
                Some((fs, name)) => {
                    if let Some(d) = next_letter() {
                        crate::println!("disk: {name} partition (GPT)");
                        crate::fs::mount(d, fs);
                    }
                }
                None => crate::println!("disk: GPT data partition with no file system rs2 can mount"),
            }
            continue;
        }
        let fat = if p.mbr_type != 0 { is_fat_type(p.mbr_type) } else { p.gpt_type == part::GPT_EFI_SYSTEM };
        if !fat {
            match p.mbr_type {
                // 0x07 is shared by HPFS, NTFS and exFAT: check the superblock.
                0x07 => {
                    if !try_mount_hpfs(Arc::new(p), "disk: HPFS partition type 0x7") {
                        crate::println!("disk: partition type 0x7 is not HPFS (NTFS/exFAT not supported)");
                    }
                }
                // OS/2 LVM: JFS volumes (others need LVM, not supported).
                0x35 => match crate::fs::probe_mount(Arc::new(p)) {
                    Some((fs, name)) => {
                        if let Some(d) = next_letter() {
                            crate::println!("disk: {name} partition type 0x35");
                            crate::fs::mount(d, fs);
                        }
                    }
                    None => crate::println!("disk: partition type 0x35 is not a JFS volume rs2 can mount"),
                },
                _ => {}
            }
            continue;
        }
        let t = p.mbr_type;
        try_mount_fat(Arc::new(p), &alloc::format!("disk: FAT partition type {t:#x}"));
    }
    // Every CD drive gets a letter, with or without a disc: the disc is
    // mounted when it is used (fs/cdrom.rs). Hard errors (medium removed)
    // go through the pop-up.
    for cd in optical {
        let has_disc = cd.sector_count() != 0;
        if let Some(d) = next_letter() {
            crate::fs::mount(d, crate::fs::cdrom::CdDrive::new(cd.clone(), has_disc));
        }
    }
    *DISKS.lock() = disks;
}
