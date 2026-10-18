//! Storage hot-plug: a kernel thread sets up USB mass storage devices that
//! are connected after boot, mounts their volumes on the next free drive
//! letters and unmounts them again when the device is removed.
//!
//! The xHCI driver notices connects and removals in its polling (timer
//! interrupt) and posts them to `xhci::STORAGE_EVENTS`; setting up a device
//! needs SCSI commands that wait for transfers, so it runs here in thread
//! context.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::xhci::{self, Handle, StorageEvent};
use super::{part, storage};
use crate::fs::block::BlockDev;
use crate::fs::FileSystem;
use crate::sync::SpinLock;

/// Mounted hot-plugged devices: the device, its drive letters and its
/// logical units.
static MOUNTED: SpinLock<Vec<(Handle, Vec<u8>, Vec<Arc<dyn BlockDev>>)>> = SpinLock::new(Vec::new());

/// Start the hot-plug thread.
pub fn start() {
    crate::task::kthread::spawn("storage hot-plug", || loop {
        match xhci::STORAGE_EVENTS.take() {
            StorageEvent::Added(h) => added(h),
            StorageEvent::Removed(h) => removed(h),
        }
    });
}

/// Recognise and mount one volume; returns its drive letter.
fn mount_volume(dev: Arc<dyn BlockDev>, optical: bool) -> Option<u8> {
    let fs: Arc<dyn FileSystem> = if optical {
        crate::fs::iso9660::Iso9660::mount(dev).ok()?
    } else if crate::fs::hpfsfs::probe(&dev) {
        crate::fs::hpfsfs::HpfsFs::mount(dev).ok()?
    } else {
        crate::fs::fatfs::FatFs::mount(dev).ok()?
    };
    let d = super::next_letter()?;
    crate::fs::mount(d, fs);
    Some(d)
}

fn added(h: Handle) {
    let Some(info) = xhci::hotplug_storage(h) else { return };
    let name = info.name.clone();
    let units = storage::attach(info);
    let mut letters = Vec::new();
    let mut devs = Vec::new();
    for (dev, optical) in units {
        devs.push(dev.clone());
        if optical {
            letters.extend(mount_volume(dev, true));
            continue;
        }
        let parts = part::scan(&dev);
        if parts.is_empty() {
            letters.extend(mount_volume(dev.clone(), false));
        }
        for p in parts {
            letters.extend(mount_volume(Arc::new(p), false));
        }
        super::DISKS.lock().push(dev);
    }
    let names: Vec<char> = letters.iter().map(|&d| (b'A' + d) as char).collect();
    if names.is_empty() {
        crate::println!("{name}: no volume mounted");
    } else {
        crate::println!("{name}: mounted as {}", names.iter().map(|c| alloc::format!("{c}:")).collect::<Vec<_>>().join(" "));
    }
    // The device may have gone while it was being set up.
    if xhci::hotplug_storage(h).is_none() {
        for d in &letters {
            crate::fs::unmount(*d);
        }
        forget_disks(&devs);
        return;
    }
    MOUNTED.lock().push((h, letters, devs));
}

fn forget_disks(devs: &[Arc<dyn BlockDev>]) {
    let same = |a: &Arc<dyn BlockDev>, b: &Arc<dyn BlockDev>| Arc::as_ptr(a) as *const u8 == Arc::as_ptr(b) as *const u8;
    super::DISKS.lock().retain(|d| !devs.iter().any(|x| same(d, x)));
}

fn removed(h: Handle) {
    let entry = {
        let mut m = MOUNTED.lock();
        m.iter().position(|e| e.0 == h).map(|i| m.remove(i))
    };
    let Some((_, letters, devs)) = entry else { return };
    for d in letters {
        crate::fs::unmount(d);
    }
    forget_disks(&devs);
}

/// Drive letters of hot-plugged volumes currently mounted.
pub fn mounted_letters() -> Vec<u8> {
    MOUNTED.lock().iter().flat_map(|e| e.1.iter().copied()).collect()
}
