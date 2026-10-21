//! The PCI device list in the boot log: every function with its IDs,
//! class and, for devices met in virtual machines, a name. Storage
//! controllers rs2 has no driver for are reported on the screen, since
//! their disks will be missing.

use super::pci::Device;

/// Names of devices commonly found in VMware, VirtualBox and QEMU VMs.
fn name(vendor: u16, device: u16) -> Option<&'static str> {
    Some(match (vendor, device) {
        (0x15ad, 0x0405) => "VMware SVGA II",
        (0x15ad, 0x0710) => "VMware SVGA (legacy)",
        (0x15ad, 0x0720) => "VMware VMXNET Ethernet",
        (0x15ad, 0x07b0) => "VMware VMXNET3 Ethernet",
        (0x15ad, 0x07c0) => "VMware PVSCSI",
        (0x15ad, 0x0740) => "VMware VMCI",
        (0x15ad, 0x0770) => "VMware USB 2 (EHCI)",
        (0x15ad, 0x0774) => "VMware USB 1.1 (UHCI)",
        (0x15ad, 0x0778 | 0x0779) => "VMware USB 3 (xHCI)",
        (0x15ad, 0x07e0) => "VMware SATA (AHCI)",
        (0x15ad, 0x07f0) => "VMware NVMe",
        (0x15ad, 0x0790) => "VMware PCI bridge",
        (0x15ad, 0x07a0) => "VMware PCI Express root port",
        (0x15ad, 0x1977) => "VMware HD Audio",
        (0x1000, 0x0030) => "LSI Logic 53c1030 (MPT Fusion, VMware \"LSI Logic Parallel\")",
        (0x1000, 0x0054) => "LSI Logic SAS1068 (MPT Fusion, VMware \"LSI Logic SAS\")",
        (0x1000, 0x0012) => "LSI Logic 53c895a",
        (0x1000, 0x0001) => "LSI Logic 53c810",
        (0x1000, 0x0060) => "LSI MegaRAID SAS 1078",
        (0x1000, 0x0079) => "LSI MegaRAID SAS 2108",
        (0x104b, 0x1040) => "BusLogic MultiMaster SCSI",
        (0x8086, 0x100e) => "Intel 82540EM Ethernet (e1000)",
        (0x8086, 0x100f) => "Intel 82545EM Ethernet (e1000)",
        (0x8086, 0x10d3) => "Intel 82574L Ethernet (e1000e)",
        (0x1022, 0x2000) => "AMD PCnet Ethernet",
        (0x1274, 0x1371) => "Ensoniq AudioPCI ES1371",
        (0x80ee, 0xcafe) => "VirtualBox guest device (VMMDev)",
        (0x80ee, 0xbeef) => "VirtualBox graphics adapter",
        (0x1234, 0x1111) => "QEMU/Bochs display",
        (0x1af4, _) => "virtio device",
        _ => return None,
    })
}

/// Whether the kernel drives this function (storage controllers).
fn has_storage_driver(d: &Device) -> bool {
    matches!((d.class, d.subclass, d.progif), (1, 6, 1) | (1, 8, 2)) || (d.class, d.subclass) == (1, 1)
        || matches!((d.vendor, d.device), (0x15ad, 0x07c0) | (0x1000, 0x0030 | 0x0054))
}

/// Log the devices; warn about mass storage controllers without a driver.
pub fn log(devs: &[Device]) {
    for d in devs {
        let n = name(d.vendor, d.device).unwrap_or("");
        crate::trace!(
            "pci {:02x}:{:02x}.{} {:04x}:{:04x} class {:02x}.{:02x}.{:02x} {n}",
            d.bus,
            d.dev,
            d.func,
            d.vendor,
            d.device,
            d.class,
            d.subclass,
            d.progif
        );
        // Mass storage other than IDE (class 1, subclass 1: the disks may
        // still be reachable through AHCI/legacy emulation).
        if d.class == 1 && d.subclass != 1 && !has_storage_driver(d) && d.vendor != 0x1af4 {
            let what = name(d.vendor, d.device).unwrap_or("storage controller");
            crate::println!(
                "pci: {what} ({:04x}:{:04x}) not supported, its disks are not available",
                d.vendor,
                d.device
            );
        }
    }
}
