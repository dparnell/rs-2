//! The resource manager: the machine's hardware as a tree of nodes
//! (adapters, the devices on them, logical devices and the drivers that
//! own them) with the resources each uses (I/O ports, interrupts, memory,
//! DMA channels), shown through the `RESMGR$` device as OS/2's
//! RESOURCE.SYS shows its own, so that IBM's RMINFO.DLL and RMVIEW.EXE,
//! rs2's RMINFO.DLL and the Hardware Manager (PMWP's WPHwManager) read it.
//!
//! The tree is built when it is first asked for, and again after a USB
//! device came or went, from what the kernel found: the PCI functions (their BARs, sized when the bus was scanned
//! at boot, before any driver started, and their interrupt), the disks
//! and CD drives on them, the ACPI namespace's devices with a hardware
//! ID and fixed current resources (the keyboard controller, serial and
//! parallel ports, the clock...), the standard PC devices that ACPI
//! leaves out, the processors, I/O APICs and the memory, and last the
//! USB devices the xHCI driver configured (hubs, keyboards, mice, mass
//! storage with its disks and CD drives, and the devices it has no
//! driver for), each under the controller or hub it is on. Handles are
//! the same from one build to the next: the nodes before the USB ones
//! are always made in the same order, and a USB node keeps the handle
//! it was first given for as long as its device stays connected.
//!
//! `RESMGR$` IOCtls (category 0x80), with IBM's structures (rmbase.h,
//! as RMINFO.DLL and RMVIEW.EXE use them, studied with Ghidra):
//!
//! | function | parameters | data |
//! |----------|------------|------|
//! | 0 | — | ULONG (RM's entry point for 16-bit callers; 0 here) |
//! | 1 GetNodeInfo | RMHANDLE, ULONG linear address of the data | ULONG RMNodeSize, RMNODE, then the node structure, its strings and its resource list (pointers are linear addresses in the data) |
//! | 2 EnumNodes | USHORT command: 0 physical, 1 logical, 2 drivers, 3/4 detected | ULONG NumEntries, then { RMHANDLE, ULONG Depth } per node (NumEntries is the whole count even when they do not all fit) |
//!
//! A buffer that is too small fails with the device error 0x0C (RMINFO
//! turns it into RMRC_BUFFER_TOO_SMALL); RMNodeSize then holds the size
//! needed.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::api::{err, Ctx};
use crate::drivers::pci;
use crate::fs::block::BlockDev;
use crate::drivers::xhci;
use crate::sync::SpinLock;

// ---- IBM's values (rmbase.h) ----

pub const RMTYPE_ADAPTER: u32 = 0;
pub const RMTYPE_DEVICE: u32 = 1;
pub const RMTYPE_LDEV: u32 = 2;
pub const RMTYPE_DRIVER: u32 = 4;

/// Adapter base types (the PCI class codes' numbering).
pub const AS_BASE_RESERVED: u16 = 0;
pub const AS_BASE_MSD: u16 = 1;
pub const AS_BASE_DISPLAY: u16 = 3;
pub const AS_BASE_MMEDIA: u16 = 4;
pub const AS_BASE_MEMORY: u16 = 5;
pub const AS_BASE_BRIDGE: u16 = 6;
pub const AS_BASE_COMM: u16 = 7;
pub const AS_BASE_PERIPH: u16 = 8;
pub const AS_BASE_INPUT: u16 = 9;

pub const AS_HOSTBUS_OTHER: u16 = 0;
pub const AS_HOSTBUS_ISA: u16 = 1;
pub const AS_HOSTBUS_PCI: u16 = 4;
pub const AS_HOSTBUS_PLANAR: u16 = 8;
pub const AS_BUSWIDTH_8: u16 = 0x10;
pub const AS_BUSWIDTH_16: u16 = 0x20;
pub const AS_BUSWIDTH_32: u16 = 0x30;
pub const AS_BUSWIDTH_64: u16 = 0x40;

/// Device types.
pub const DS_TYPE_DISK: u16 = 0;
pub const DS_TYPE_CDROM: u16 = 5;
pub const DS_TYPE_IO: u16 = 0x0f;
pub const DS_REMOVABLE_MEDIA: u16 = 1;

/// Logical device classes.
pub const LDEV_CLASS_DASD: u16 = 2;
pub const LDEV_CLASS_CDROM: u16 = 3;
pub const LDEV_CLASS_SERIAL: u16 = 4;
pub const LDEV_CLASS_PARALLEL: u16 = 5;

/// Driver kinds: DrvrType << 8 | DrvrSubType. OS/2 character and block
/// drivers (type 3, subtypes 1 and 2), network, video, audio and service
/// drivers (types 4-7; a service driver of subtype 1 is a snooper).
pub const DRS_CHAR: u16 = 0x0301;
pub const DRS_BLOCK: u16 = 0x0302;
pub const DRS_NETWORK: u16 = 0x0400;
pub const DRS_VIDEO: u16 = 0x0500;
pub const DRS_AUDIO: u16 = 0x0600;
pub const DRS_SERVICE: u16 = 0x0700;

/// Resource flags.
pub const RS_EXCLUSIVE: u16 = 1;
pub const RS_SHARED: u16 = 4;

const RS_TYPE_IO: u32 = 1;
const RS_TYPE_IRQ: u32 = 2;
const RS_TYPE_MEM: u32 = 3;
const RS_TYPE_DMA: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub enum Res {
    Io { base: u16, len: u16, flags: u16, lines: u16 },
    Irq { level: u16, pin: u16, flags: u16 },
    Mem { base: u32, size: u32, flags: u16 },
    Dma { channel: u16, flags: u16 },
}

#[derive(Clone)]
pub enum Kind {
    Adapter { base: u16, sub: u16, intf: u16, bus: u16, width: u16 },
    Device { ty: u16, flags: u16 },
    LDev { ty: u16, host: u32 },
    Driver { name: String, vendor: String, ty: u16, sub: u16 },
}

#[derive(Clone)]
pub struct Node {
    pub handle: u32,
    /// The node above (index in the tree), physical and driver views.
    pub parent: Option<usize>,
    pub kind: Kind,
    pub desc: String,
    /// The driver's handle (0: none).
    pub driver: u32,
    pub res: Vec<Res>,
}

pub struct Tree {
    pub nodes: Vec<Node>,
}

/// Handles: an index with a tag, never 0.
const HANDLE_BASE: u32 = 0x0001_0000;

impl Tree {
    fn add(&mut self, parent: Option<usize>, kind: Kind, desc: String, driver: u32, res: Vec<Res>) -> usize {
        let i = self.nodes.len();
        self.nodes.push(Node { handle: HANDLE_BASE + i as u32, parent, kind, desc, driver, res });
        i
    }

    fn adapter(&mut self, parent: Option<usize>, desc: String, base: u16, sub: u16, bus: u16, width: u16, driver: u32, res: Vec<Res>) -> usize {
        // Display adapters are SVGA (interface 2 of the VGA subtype).
        let intf = if (base, sub) == (AS_BASE_DISPLAY, 1) { 2 } else { 0 };
        self.add(parent, Kind::Adapter { base, sub, intf, bus, width }, desc, driver, res)
    }

    fn device(&mut self, parent: usize, desc: String, ty: u16, flags: u16) -> usize {
        let drv = self.nodes[parent].driver;
        self.add(Some(parent), Kind::Device { ty, flags }, desc, drv, Vec::new())
    }

    fn by_handle(&self, h: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.handle == h)
    }

    /// The driver node called `name` (made on first use).
    fn driver(&mut self, name: &str, desc: &str, sub: u16) -> u32 {
        if let Some(n) = self.nodes.iter().find(|n| matches!(&n.kind, Kind::Driver { name: x, .. } if x == name)) {
            return n.handle;
        }
        let i = self.add(None, Kind::Driver { name: String::from(name), vendor: String::from("rs2"), ty: sub >> 8, sub: sub & 0xff }, String::from(desc), 0, Vec::new());
        self.nodes[i].handle
    }

    /// The nodes of a view in tree order with their depths.
    pub fn enumerate(&self, command: u16) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        match command {
            0 => {
                let phys = |n: &Node| matches!(n.kind, Kind::Adapter { .. } | Kind::Device { .. });
                fn walk(t: &Tree, i: usize, depth: u32, out: &mut Vec<(u32, u32)>) {
                    out.push((t.nodes[i].handle, depth));
                    for (j, n) in t.nodes.iter().enumerate() {
                        if n.parent == Some(i) && matches!(n.kind, Kind::Adapter { .. } | Kind::Device { .. }) {
                            walk(t, j, depth + 1, out);
                        }
                    }
                }
                for (i, n) in self.nodes.iter().enumerate() {
                    if n.parent.is_none() && phys(n) {
                        walk(self, i, 0, &mut out);
                    }
                }
            }
            1 => {
                for n in self.nodes.iter().filter(|n| matches!(n.kind, Kind::LDev { .. })) {
                    out.push((n.handle, 0));
                }
            }
            2 => {
                // Each driver, then the adapters and devices it owns.
                for d in self.nodes.iter().filter(|n| matches!(n.kind, Kind::Driver { .. })) {
                    out.push((d.handle, 0));
                    for n in self.nodes.iter().filter(|n| n.driver == d.handle && matches!(n.kind, Kind::Adapter { .. })) {
                        out.push((n.handle, 1));
                    }
                }
            }
            _ => {}
        }
        out
    }
}

// ---- What the drivers found at boot ----

/// A PCI function, its BARs (I/O or memory, base and size) as they were
/// before any driver started.
struct PciFn {
    dev: pci::Device,
    bars: Vec<(bool, u64, u64)>,
    /// Header type 1: the secondary bus behind the bridge.
    secondary: Option<u8>,
}

/// A disk or CD drive and the PCI function it is on.
struct Storage {
    bdf: (u8, u8, u8),
    optical: bool,
    bytes: u64,
}

static PCI: SpinLock<Vec<PciFn>> = SpinLock::new(Vec::new());
static STORAGE: SpinLock<Vec<Storage>> = SpinLock::new(Vec::new());
/// The tree and the change count it was built at.
static TREE: SpinLock<Option<(u32, Arc<Tree>)>> = SpinLock::new(None);

/// The logical units of a USB mass storage device: (device, optical,
/// bytes).
static USB_STORAGE: SpinLock<Vec<(xhci::Handle, bool, u64)>> = SpinLock::new(Vec::new());
/// Bumped when USB_STORAGE changes (its units are set up after the
/// device is configured).
static USB_STORAGE_GEN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Handles of USB nodes: (device, role: 0 the device, 1 + n its unit n,
/// 0x80 + n the unit's logical device) and the handle given.
static USB_HANDLES: SpinLock<(u32, Vec<((xhci::Handle, u8), u32)>)> = SpinLock::new((0, Vec::new()));
/// USB handles are counted from here (far above the index-based ones).
const USB_HANDLE_BASE: u32 = 0x0010_0000;

/// Size the BARs of every function (memory and I/O decoding off while
/// the all-ones pattern is in the register). Called right after the bus
/// scan, before the drivers.
pub fn scan_pci(devs: &[pci::Device]) {
    let mut v = Vec::new();
    for d in devs {
        let hdr = (d.read(0x0c) >> 16) as u8 & 0x7f;
        let nbars = match hdr {
            0 => 6,
            1 => 2,
            _ => 0,
        };
        let cmd = d.read(0x04);
        d.write(0x04, cmd & !3 & 0xffff);
        let mut bars = Vec::new();
        let mut n = 0;
        while n < nbars {
            let off = 0x10 + n * 4;
            let lo = d.read(off);
            if lo & 1 != 0 {
                d.write(off, 0xffff_ffff);
                let mask = d.read(off) & !3 & 0xffff;
                d.write(off, lo);
                if mask != 0 {
                    bars.push((true, (lo & !3) as u64, ((!mask & 0xffff) + 1) as u64));
                }
                n += 1;
                continue;
            }
            let is64 = (lo >> 1) & 3 == 2;
            d.write(off, 0xffff_ffff);
            let mlo = (d.read(off) & !0xf) as u64;
            d.write(off, lo);
            let mut base = (lo & !0xf) as u64;
            let mask = if is64 && n + 1 < nbars {
                let hi = d.read(off + 4);
                d.write(off + 4, 0xffff_ffff);
                let mhi = d.read(off + 4) as u64;
                d.write(off + 4, hi);
                base |= (hi as u64) << 32;
                n += 1;
                mhi << 32 | mlo
            } else {
                0xffff_ffff_0000_0000 | mlo
            };
            if mlo != 0 && base != 0 {
                bars.push((false, base, (!mask).wrapping_add(1)));
            }
            n += 1;
        }
        d.write(0x04, cmd & 0xffff);
        let secondary = (hdr == 1).then(|| (d.read(0x18) >> 8) as u8);
        v.push(PciFn { dev: *d, bars, secondary });
    }
    *PCI.lock() = v;
}

/// The disks and CD drives a PCI function's driver found.
pub fn note_storage(d: &pci::Device, disks: &[Arc<dyn BlockDev>], optical: &[Arc<dyn BlockDev>]) {
    let mut s = STORAGE.lock();
    for (list, opt) in [(disks, false), (optical, true)] {
        for b in list {
            s.push(Storage { bdf: (d.bus, d.dev, d.func), optical: opt, bytes: b.sector_count() * b.sector_size() as u64 });
        }
    }
}

/// The logical units of a USB mass storage device (at boot or when it
/// is connected later).
pub fn note_usb_storage(h: xhci::Handle, units: &[(Arc<dyn BlockDev>, bool)]) {
    let mut s = USB_STORAGE.lock();
    s.retain(|(x, _, _)| *x != h);
    for (b, optical) in units {
        s.push((h, *optical, b.sector_count() * b.sector_size() as u64));
    }
    drop(s);
    USB_STORAGE_GEN.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

// ---- Building the tree ----

fn s(x: &str) -> String {
    String::from(x)
}

fn size_text(bytes: u64) -> String {
    let mb = bytes >> 20;
    if mb >= 10 * 1024 {
        alloc::format!("{} GB", mb >> 10)
    } else {
        alloc::format!("{mb} MB")
    }
}

/// The processor's name (CPUID's brand string).
fn cpu_brand() -> String {
    use core::arch::x86_64::__cpuid;
    let max = __cpuid(0x8000_0000).eax;
    if max < 0x8000_0004 {
        return s("x86 processor");
    }
    let mut b = Vec::new();
    for leaf in 0x8000_0002..=0x8000_0004u32 {
        let r = __cpuid(leaf);
        for v in [r.eax, r.ebx, r.ecx, r.edx] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    let t: String = b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { s("x86 processor") } else { t }
}

/// What a PCI function is, by its class (the boot log's names first).
fn pci_text(d: &pci::Device) -> String {
    if let Some(n) = crate::drivers::pcilist::name(d.vendor, d.device) {
        if d.vendor != 0x1af4 {
            return s(n);
        }
    }
    let t = match (d.class, d.subclass, d.progif) {
        (1, 0, _) => "SCSI controller",
        (1, 1, _) => "IDE controller",
        (1, 2, _) => "Diskette controller",
        (1, 4, _) => "RAID controller",
        (1, 5, _) => "ATA controller",
        (1, 6, _) => "SATA controller (AHCI)",
        (1, 7, _) => "SAS controller",
        (1, 8, _) => "NVMe controller",
        (1, _, _) => "Storage controller",
        (2, 0, _) => "Ethernet controller",
        (2, _, _) => "Network controller",
        (3, 0, _) => "VGA display adapter",
        (3, _, _) => "Display adapter",
        (4, 0, _) => "Video device",
        (4, 1, _) => "Audio device",
        (4, 3, _) => "High Definition Audio controller",
        (4, _, _) => "Multimedia device",
        (5, _, _) => "Memory controller",
        (6, 0, _) => "Host bridge",
        (6, 1, _) => "PCI-to-ISA bridge",
        (6, 4, _) => "PCI-to-PCI bridge",
        (6, _, _) => "Bridge",
        (7, 0, _) => "Serial controller",
        (7, 1, _) => "Parallel port",
        (7, _, _) => "Communication controller",
        (8, _, _) => "System peripheral",
        (9, _, _) => "Input controller",
        (0x0c, 3, 0x00) => "USB controller (UHCI)",
        (0x0c, 3, 0x10) => "USB controller (OHCI)",
        (0x0c, 3, 0x20) => "USB controller (EHCI)",
        (0x0c, 3, 0x30) => "USB controller (xHCI)",
        (0x0c, 5, _) => "SMBus controller",
        (0x0c, _, _) => "Serial bus controller",
        _ => "PCI device",
    };
    if d.vendor == 0x1af4 {
        let what = match d.device {
            0x1000 | 0x1041 => "network",
            0x1001 | 0x1042 => "block",
            0x1003 | 0x1043 => "console",
            0x1004 | 0x1048 => "SCSI",
            0x1005 | 0x1044 => "entropy",
            0x1050 => "GPU",
            0x1052 => "input",
            _ => return alloc::format!("Virtio {t}"),
        };
        return alloc::format!("Virtio {what} device");
    }
    s(t)
}

/// RM's base type and subtype of a PCI function.
fn pci_types(d: &pci::Device) -> (u16, u16) {
    let base = if d.class <= 9 { d.class as u16 } else { AS_BASE_RESERVED };
    let sub = match (d.class, d.subclass) {
        // SCSI-like and ATA-like controllers.
        (1, 0 | 7) => 1,
        (1, 1 | 4 | 5 | 6 | 8) => 2,
        (1, 2) => 3,
        (3, 1) => 2,
        (3, _) => 1,
        (4, 0) => 1,
        (4, 1 | 3) => 2,
        (7, 0) => 1,
        (7, 1) => 2,
        (8, 0) => 1,
        (8, 1) => 2,
        (8, 2) => 3,
        (8, 3) => 4,
        (9, 0) => 1,
        (9, 1) => 2,
        (9, 2) => 3,
        (_, 0x80) => 0x80,
        (_, x) => x as u16 + 1,
    };
    (base, sub)
}

/// The kernel driver that runs a PCI function: (name, description, subtype).
fn pci_driver(d: &pci::Device) -> Option<(&'static str, &'static str, u16)> {
    use crate::drivers::*;
    Some(match (d.class, d.subclass, d.progif) {
        _ if d.vendor == 0x1af4 => ("VIRTIO", "Virtio devices", DRS_SERVICE),
        (1, 6, 1) => ("AHCI", "SATA (AHCI) disks and CD drives", DRS_BLOCK),
        (1, 1, _) => ("IDE", "IDE disks and CD drives", DRS_BLOCK),
        (1, 8, 2) => ("NVME", "NVM Express disks", DRS_BLOCK),
        (0x0c, 3, 0x30) => ("XHCI", "USB 3 (xHCI): keyboards, mice, disks", DRS_SERVICE),
        (4, 3, _) => ("HDA", "High Definition Audio", DRS_AUDIO),
        (7, 1, _) => ("LPT", "Parallel ports LPT1-LPT3", DRS_CHAR),
        _ if ac97::is_ac97(d) => ("AC97", "AC'97 audio", DRS_AUDIO),
        _ if es137x::is_es137x(d) => ("ES1371", "Ensoniq AudioPCI", DRS_AUDIO),
        _ if e1000::model(d).is_some() => ("E1000", "Intel PRO/1000 Ethernet", DRS_NETWORK),
        _ if vmxnet3::is_vmxnet3(d) => ("VMXNET3", "VMware VMXNET3 Ethernet", DRS_NETWORK),
        _ if pcnet::is_pcnet(d) => ("PCNET", "AMD PCnet Ethernet", DRS_NETWORK),
        _ if (d.vendor, d.device) == (0x15ad, 0x0405) => ("SVGA", "VMware SVGA II display", DRS_VIDEO),
        _ if (d.vendor, d.device) == (0x15ad, 0x07c0) => ("PVSCSI", "VMware paravirtual SCSI", DRS_BLOCK),
        _ if (d.vendor, d.device) == (0x1000, 0x0030) || (d.vendor, d.device) == (0x1000, 0x0054) => ("MPT", "LSI Logic MPT Fusion SCSI", DRS_BLOCK),
        (3, _, _) => ("DISPLAY", "Frame buffer display", DRS_VIDEO),
        (6, _, _) => ("PCI", "PCI bus", DRS_SERVICE),
        _ => return None,
    })
}

/// The resources of an ACPI resource template (`_CRS`).
pub(crate) fn crs_resources(b: &[u8]) -> Vec<Res> {
    let mut v = Vec::new();
    let mut i = 0;
    let u16_at = |o: usize| b.get(o..o + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]));
    let u32_at = |o: usize| b.get(o..o + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    while i < b.len() {
        let t = b[i];
        if t & 0x80 == 0 {
            let (kind, len) = ((t >> 3) & 0xf, (t & 7) as usize);
            match kind {
                0xf => break,
                // IRQ: a mask of interrupts.
                4 if len >= 2 => {
                    let mask = u16_at(i + 1);
                    let flags = if len >= 3 { b[i + 3] } else { 1 };
                    for l in 0..16 {
                        if mask & (1 << l) != 0 {
                            v.push(Res::Irq { level: l, pin: 0, flags: if flags & 0x10 != 0 { RS_SHARED } else { RS_EXCLUSIVE } });
                        }
                    }
                }
                // DMA: a mask of channels.
                5 if len >= 2 => {
                    for c in 0..8 {
                        if b[i + 1] & (1 << c) != 0 {
                            v.push(Res::Dma { channel: c, flags: RS_EXCLUSIVE });
                        }
                    }
                }
                // I/O port: decode, min, max, alignment, length.
                8 if len >= 7 => {
                    let lines = if b[i + 1] & 1 != 0 { 16 } else { 10 };
                    let n = b[i + 7] as u16;
                    if n > 0 {
                        v.push(Res::Io { base: u16_at(i + 2), len: n, flags: RS_EXCLUSIVE, lines });
                    }
                }
                // Fixed I/O port: 10-bit decode.
                9 if len >= 3 => {
                    let n = b[i + 3] as u16;
                    if n > 0 {
                        v.push(Res::Io { base: u16_at(i + 1) & 0x3ff, len: n, flags: RS_EXCLUSIVE, lines: 10 });
                    }
                }
                _ => {}
            }
            i += 1 + len;
        } else {
            let len = u16_at(i + 1) as usize;
            let d = i + 3;
            match t & 0x7f {
                // 24-bit memory: in units of 256 bytes.
                0x01 if len >= 9 => {
                    let n = u16_at(d + 7) as u32 * 256;
                    if n > 0 {
                        v.push(Res::Mem { base: u16_at(d + 1) as u32 * 256, size: n, flags: RS_EXCLUSIVE });
                    }
                }
                // 32-bit memory range.
                0x05 if len >= 17 => {
                    let n = u32_at(d + 13);
                    if n > 0 {
                        v.push(Res::Mem { base: u32_at(d + 1), size: n, flags: RS_EXCLUSIVE });
                    }
                }
                // Fixed 32-bit memory.
                0x06 if len >= 9 => {
                    let n = u32_at(d + 5);
                    if n > 0 {
                        v.push(Res::Mem { base: u32_at(d + 1), size: n, flags: RS_EXCLUSIVE });
                    }
                }
                // Extended interrupts.
                0x09 if len >= 6 => {
                    let flags = b[d];
                    for k in 0..b[d + 1] as usize {
                        let g = u32_at(d + 2 + 4 * k);
                        v.push(Res::Irq { level: g as u16, pin: 0, flags: if flags & 0x08 != 0 { RS_SHARED } else { RS_EXCLUSIVE } });
                    }
                }
                _ => {}
            }
            i += 3 + len;
        }
    }
    v
}

fn io(base: u16, len: u16) -> Res {
    Res::Io { base, len, flags: RS_EXCLUSIVE, lines: 16 }
}

fn irq(level: u16) -> Res {
    Res::Irq { level, pin: 0, flags: RS_EXCLUSIVE }
}

/// The standard PC devices by ACPI hardware ID: (name prefix, text,
/// base type, subtype, kernel driver) and the resources they use when
/// ACPI does not say.
fn known(hid: &str) -> Option<(&'static str, &'static str, u16, u16, Vec<Res>)> {
    Some(match hid {
        "PNP0000" | "PNP0001" => ("PIC", "Programmable interrupt controller", AS_BASE_PERIPH, 1, alloc::vec![io(0x20, 2), io(0xa0, 2), irq(2)]),
        "PNP0200" => ("DMA", "DMA controller", AS_BASE_PERIPH, 2, alloc::vec![io(0, 16), io(0x80, 16), io(0xc0, 32), Res::Dma { channel: 4, flags: RS_EXCLUSIVE }]),
        "PNP0100" => ("TIMER", "System timer", AS_BASE_PERIPH, 3, alloc::vec![io(0x40, 4), irq(0)]),
        "PNP0B00" | "PNP0B01" | "PNP0B02" => ("RTC", "Real-time clock", AS_BASE_PERIPH, 4, alloc::vec![io(0x70, 2), irq(8)]),
        "PNP0103" => ("HPET", "High precision event timer", AS_BASE_PERIPH, 3, Vec::new()),
        "PNP0303" | "PNP030B" | "PNP0320" => ("KBC", "Keyboard controller", AS_BASE_INPUT, 1, alloc::vec![io(0x60, 1), io(0x64, 1), irq(1)]),
        "PNP0F03" | "PNP0F0B" | "PNP0F0E" | "PNP0F13" => ("PS2M", "PS/2 mouse port", AS_BASE_INPUT, 3, alloc::vec![irq(12)]),
        "PNP0500" | "PNP0501" => ("SERIAL", "Serial port", AS_BASE_COMM, 1, Vec::new()),
        "PNP0400" | "PNP0401" => ("PAR", "Parallel port", AS_BASE_COMM, 2, Vec::new()),
        "PNP0700" => ("FDC", "Diskette controller", AS_BASE_MSD, 3, alloc::vec![io(0x3f0, 6), io(0x3f7, 1), irq(6), Res::Dma { channel: 2, flags: RS_EXCLUSIVE }]),
        "PNP0800" => ("SPKR", "PC speaker", AS_BASE_PERIPH, 0x80, alloc::vec![io(0x61, 1)]),
        "PNP0C04" => ("FPU", "Numeric coprocessor", AS_BASE_RESERVED, 0, alloc::vec![io(0xf0, 16), irq(13)]),
        _ => return None,
    })
}

/// The kernel driver of a standard device (by name prefix).
fn legacy_driver(t: &mut Tree, prefix: &str) -> u32 {
    match prefix {
        "KBC" => t.driver("KBD", "Keyboard (8042 controller)", DRS_CHAR),
        "PS2M" => t.driver("MOUSE", "Mouse", DRS_CHAR),
        "SERIAL" => t.driver("COM", "Serial ports COM1-COM4", DRS_CHAR),
        "PAR" => t.driver("LPT", "Parallel ports LPT1-LPT3", DRS_CHAR),
        "TIMER" | "RTC" | "HPET" | "PIC" => t.driver("CLOCK", "Interrupts, timers and the clock", DRS_SERVICE),
        _ => 0,
    }
}

fn build() -> Tree {
    let mut t = Tree { nodes: Vec::new() };
    let kernel = t.driver("RS2KRNL", "rs2 kernel", DRS_SERVICE);
    // Processors.
    let brand = cpu_brand();
    let cpus = crate::smp::online_count().max(1);
    for n in 0..cpus {
        t.adapter(None, alloc::format!("CPU_{n} {brand}"), AS_BASE_RESERVED, 0, AS_HOSTBUS_PLANAR, AS_BUSWIDTH_64, kernel, Vec::new());
    }
    // Memory.
    let bytes = crate::mm::phys::total_frames() * 4096;
    let below4g = bytes.min(0xffff_f000) as u32;
    t.adapter(
        None,
        alloc::format!("MEM_0 System memory, {}", size_text(bytes)),
        AS_BASE_MEMORY,
        1,
        AS_HOSTBUS_PLANAR,
        AS_BUSWIDTH_64,
        kernel,
        alloc::vec![Res::Mem { base: 0, size: 0xa0000, flags: RS_EXCLUSIVE }, Res::Mem { base: 0x10_0000, size: below4g.saturating_sub(0x10_0000), flags: RS_EXCLUSIVE }],
    );
    // Interrupt controllers of the processors.
    if let Some(i) = crate::acpi::info() {
        let clock = t.driver("CLOCK", "Interrupts, timers and the clock", DRS_SERVICE);
        for (n, a) in i.ioapics.iter().enumerate() {
            t.adapter(
                None,
                alloc::format!("IOAPIC_{n} I/O APIC"),
                AS_BASE_PERIPH,
                1,
                AS_HOSTBUS_PLANAR,
                AS_BUSWIDTH_32,
                clock,
                alloc::vec![Res::Mem { base: a.address as u32, size: 0x1000, flags: RS_EXCLUSIVE }],
            );
        }
    }
    // The PCI buses: functions under their bus, a bridge's bus under it.
    let pci = PCI.lock();
    let storage = STORAGE.lock();
    let pci_drv = t.driver("PCI", "PCI bus", DRS_SERVICE);
    let root = t.adapter(None, s("PCI_0 PCI bus"), AS_BASE_RESERVED, 0, AS_HOSTBUS_PCI, AS_BUSWIDTH_32, pci_drv, Vec::new());
    let mut bus_node: Vec<(u8, usize)> = alloc::vec![(0, root)];
    let mut isa_bridge = None;
    let (mut disks, mut cds) = (0, 0);
    for f in pci.iter() {
        let d = &f.dev;
        let parent = bus_node.iter().find(|(b, _)| *b == d.bus).map_or(root, |&(_, n)| n);
        let mut res = Vec::new();
        for &(is_io, base, size) in &f.bars {
            if is_io {
                res.push(Res::Io { base: base as u16, len: size.min(0xffff) as u16, flags: RS_EXCLUSIVE, lines: 16 });
            } else if base < 0x1_0000_0000 {
                res.push(Res::Mem { base: base as u32, size: size.min(0xffff_ffff) as u32, flags: RS_EXCLUSIVE });
            }
        }
        let pin = (d.read(0x3c) >> 8) as u8;
        if pin != 0 && d.irq != 0 && d.irq != 0xff {
            let level = crate::acpi::pci_routing().and_then(|r| r.route(d.bus, d.dev, pin, d.irq)).map_or(d.irq as u16, |g| g.gsi as u16);
            res.push(Res::Irq { level, pin: pin as u16, flags: RS_SHARED });
        }
        let driver = match pci_driver(d) {
            Some((name, desc, sub)) => t.driver(name, desc, sub),
            None => 0,
        };
        let (base, sub) = pci_types(d);
        let desc = alloc::format!("PCI {} {}", d.bdf(), pci_text(d));
        let n = t.adapter(Some(parent), desc, base, sub, AS_HOSTBUS_PCI, AS_BUSWIDTH_32, driver, res);
        if let Some(sec) = f.secondary {
            bus_node.push((sec, n));
        }
        if (d.class, d.subclass) == (6, 1) && isa_bridge.is_none() {
            isa_bridge = Some(n);
        }
        // Its disks and CD drives.
        for st in storage.iter().filter(|s| s.bdf == (d.bus, d.dev, d.func)) {
            let (desc, ty, flags, class) = if st.optical {
                cds += 1;
                (alloc::format!("CDROM_{} CD-ROM drive", cds - 1), DS_TYPE_CDROM, DS_REMOVABLE_MEDIA, LDEV_CLASS_CDROM)
            } else {
                disks += 1;
                (alloc::format!("HD_{} Hard disk, {}", disks - 1, size_text(st.bytes)), DS_TYPE_DISK, 0, LDEV_CLASS_DASD)
            };
            let dev = t.device(n, desc.clone(), ty, flags);
            let host = t.nodes[dev].handle;
            let drv = t.nodes[dev].driver;
            t.add(None, Kind::LDev { ty: class, host }, desc, drv, Vec::new());
        }
    }
    drop(storage);
    drop(pci);
    // The ISA bus and the standard devices on it.
    let isa_drv = t.driver("ISA", "ISA bus and system board devices", DRS_SERVICE);
    let isa = t.adapter(Some(isa_bridge.unwrap_or(root)), s("ISA_0 ISA bus"), AS_BASE_RESERVED, 0, AS_HOSTBUS_ISA, AS_BUSWIDTH_16, isa_drv, Vec::new());
    let mut count: Vec<(&'static str, u32)> = Vec::new();
    let mut add_isa = |t: &mut Tree, prefix: &'static str, text: String, base: u16, sub: u16, res: Vec<Res>| -> usize {
        let n = match count.iter_mut().find(|(p, _)| *p == prefix) {
            Some((_, c)) => {
                *c += 1;
                *c - 1
            }
            None => {
                count.push((prefix, 1));
                0
            }
        };
        let drv = legacy_driver(t, prefix);
        t.adapter(Some(isa), alloc::format!("{prefix}_{n} {text}"), base, sub, AS_HOSTBUS_ISA, AS_BUSWIDTH_16, drv, res)
    };
    let mut seen: Vec<&'static str> = Vec::new();
    let mut coms = 0u32;
    let mut lpts = 0u32;
    for a in crate::acpi::devices() {
        let Some((prefix, text, base, sub, default)) = known(&a.hid) else { continue };
        if a.absent {
            continue;
        }
        let res = a.crs.as_deref().map(crs_resources).filter(|r| !r.is_empty()).unwrap_or(default);
        let first_io = res.iter().find_map(|r| if let Res::Io { base, .. } = r { Some(*base) } else { None });
        let mut text = String::from(text);
        // Serial and parallel ports that answer (a `_STA` method is not
        // run).
        match prefix {
            "SERIAL" => {
                let Some(i) = first_io.and_then(|b| [0x3f8u16, 0x2f8, 0x3e8, 0x2e8].iter().position(|&x| x == b)) else { continue };
                if !crate::chardev::com_present(i) {
                    continue;
                }
                text = alloc::format!("{text} COM{}", i + 1);
                coms |= 1 << i;
            }
            "PAR" => {
                let Some(i) = first_io.and_then(|b| (0..3).find(|&i| crate::parport::info(i).is_some_and(|f| f.base == b))) else { continue };
                text = alloc::format!("{text} LPT{}", i + 1);
                lpts |= 1 << i;
            }
            "FDC" if crate::rtc::cmos(0x10) == 0 => continue,
            "PS2M" if !crate::mouse::ps2_present() => continue,
            _ => {}
        }
        let n = add_isa(&mut t, prefix, text, base, sub, res);
        seen.push(prefix);
        child_devices(&mut t, prefix, n);
    }
    // What ACPI leaves out (QEMU's tables have no PIC, DMA or timer).
    for (hid, prefix) in [("PNP0000", "PIC"), ("PNP0200", "DMA"), ("PNP0100", "TIMER"), ("PNP0B00", "RTC"), ("PNP0303", "KBC")] {
        if !seen.contains(&prefix) {
            let (prefix, text, base, sub, res) = known(hid).unwrap();
            let n = add_isa(&mut t, prefix, s(text), base, sub, res);
            child_devices(&mut t, prefix, n);
        }
    }
    for i in 0..4 {
        if coms & (1 << i) == 0 && crate::chardev::com_present(i) {
            let base = [0x3f8u16, 0x2f8, 0x3e8, 0x2e8][i];
            let level = [4u16, 3, 4, 3][i];
            add_isa(&mut t, "SERIAL", alloc::format!("Serial port COM{}", i + 1), AS_BASE_COMM, 1, alloc::vec![io(base, 8), irq(level)]);
        }
    }
    // Parallel ports found by probing (PCI cards are PCI functions).
    for i in 0..3 {
        let Some(f) = crate::parport::info(i).filter(|f| f.pci.is_none()) else { continue };
        if lpts & (1 << i) == 0 {
            let mut res = alloc::vec![io(f.base, if f.base == 0x3bc { 4 } else { 8 })];
            if f.ecr != 0 {
                res.push(io(f.ecr - 2, 8));
            }
            if let Some(q) = f.irq {
                res.push(irq(q as u16));
            }
            add_isa(&mut t, "PAR", alloc::format!("Parallel port LPT{}", i + 1), AS_BASE_COMM, 2, res);
        }
    }
    // Logical serial and parallel ports.
    let ports: Vec<(u32, String, u16, u32)> = t
        .nodes
        .iter()
        .filter_map(|n| match n.kind {
            Kind::Adapter { base: AS_BASE_COMM, sub, .. } => Some((n.handle, n.desc.clone(), sub, n.driver)),
            _ => None,
        })
        .collect();
    for (h, desc, sub, drv) in ports {
        let class = if sub == 1 { LDEV_CLASS_SERIAL } else { LDEV_CLASS_PARALLEL };
        t.add(None, Kind::LDev { ty: class, host: h }, desc, drv, Vec::new());
    }
    usb_devices(&mut t, disks, cds);
    t
}

/// The text and RM types of a USB device: (name prefix, text, base type,
/// subtype).
fn usb_text(d: &xhci::UsbInfo) -> (&'static str, String, u16, u16) {
    use crate::drivers::hid::Kind as Hid;
    match &d.kind {
        xhci::UsbKind::Hub { ports, usb3 } => ("USBHUB", alloc::format!("USB {} hub, {ports} ports", if *usb3 { 3 } else { 2 }), AS_BASE_RESERVED, 0),
        xhci::UsbKind::Storage => ("USBMSD", s("USB mass storage device"), AS_BASE_MSD, 0x80),
        xhci::UsbKind::Hid(k) => match k.as_slice() {
            [Hid::Keyboard] => ("USBKBD", s("USB keyboard"), AS_BASE_INPUT, 1),
            [Hid::Mouse] => ("USBMOUSE", s("USB mouse"), AS_BASE_INPUT, 3),
            _ => ("USBKBD", s("USB keyboard and mouse"), AS_BASE_INPUT, 1),
        },
        xhci::UsbKind::Other { class } => {
            let (text, base, sub) = match class {
                1 => ("audio device", AS_BASE_MMEDIA, 2),
                2 | 0x0a => ("communications device", AS_BASE_COMM, 0x80),
                3 => ("input device", AS_BASE_INPUT, 0x80),
                6 => ("imaging device", AS_BASE_MMEDIA, 0x80),
                7 => ("printer", AS_BASE_PERIPH, 0x80),
                8 => ("mass storage device", AS_BASE_MSD, 0x80),
                0x0b => ("smart card reader", AS_BASE_PERIPH, 0x80),
                0x0e => ("video device", AS_BASE_MMEDIA, 1),
                0xe0 => ("wireless controller", AS_BASE_COMM, 0x80),
                _ => ("device", AS_BASE_RESERVED, 0),
            };
            ("USB", alloc::format!("USB {text} (no driver)"), base, sub)
        }
    }
}

fn speed_text(speed: u8) -> &'static str {
    use crate::drivers::usb::speed;
    match speed {
        speed::LOW => "1.5 Mb/s",
        speed::FULL => "12 Mb/s",
        speed::HIGH => "480 Mb/s",
        speed::SUPER => "5 Gb/s",
        _ => "10 Gb/s",
    }
}

/// The USB devices, under their controller or hub, with the disks and CD
/// drives of mass storage devices below them. `disks` and `cds` are the
/// numbers of those already in the tree.
fn usb_devices(t: &mut Tree, mut disks: u32, mut cds: u32) {
    let devs = xhci::devices();
    let units = USB_STORAGE.lock().clone();
    let mut handles = USB_HANDLES.lock();
    // Forget the handles of devices that went.
    handles.1.retain(|((h, _), _)| devs.iter().any(|d| d.handle == *h));
    let mut handle = |h: xhci::Handle, role: u8| -> u32 {
        if let Some(&(_, x)) = handles.1.iter().find(|(k, _)| *k == (h, role)) {
            return x;
        }
        let x = USB_HANDLE_BASE + handles.0;
        handles.0 += 1;
        handles.1.push(((h, role), x));
        x
    };
    let mut count: Vec<(&'static str, u32)> = Vec::new();
    let mut placed: Vec<(xhci::Handle, usize)> = Vec::new();
    for d in &devs {
        // The controller's adapter, or the hub's.
        let parent = match d.parent {
            Some(p) => placed.iter().find(|(h, _)| *h == p).map(|&(_, n)| n),
            None => t.nodes.iter().position(|n| {
                matches!(n.kind, Kind::Adapter { .. }) && n.desc.starts_with(&alloc::format!("PCI {:02x}:{:02x}.{} ", d.bdf.0, d.bdf.1, d.bdf.2))
            }),
        };
        let Some(parent) = parent else { continue };
        let (prefix, text, base, sub) = usb_text(d);
        let n = match count.iter_mut().find(|(p, _)| *p == prefix) {
            Some((_, c)) => {
                *c += 1;
                *c - 1
            }
            None => {
                count.push((prefix, 1));
                0
            }
        };
        let driver = if matches!(d.kind, xhci::UsbKind::Other { .. }) { 0 } else { t.nodes[parent].driver };
        let desc = alloc::format!(
            "{prefix}_{n} {text}, {:04x}:{:04x}, USB {}.{}, {}, port {}",
            d.vendor,
            d.product,
            d.usb >> 8,
            (d.usb >> 4) & 0xf,
            speed_text(d.speed),
            d.path
        );
        let i = t.add(Some(parent), Kind::Adapter { base, sub, intf: 0, bus: AS_HOSTBUS_OTHER, width: AS_BUSWIDTH_32 }, desc, driver, Vec::new());
        t.nodes[i].handle = handle(d.handle, 0);
        placed.push((d.handle, i));
        // Its disks and CD drives.
        for (u, &(_, optical, bytes)) in units.iter().filter(|(h, _, _)| *h == d.handle).enumerate().take(0x7f) {
            let (desc, ty, flags, class) = if optical {
                cds += 1;
                (alloc::format!("CDROM_{} CD-ROM drive", cds - 1), DS_TYPE_CDROM, DS_REMOVABLE_MEDIA, LDEV_CLASS_CDROM)
            } else {
                disks += 1;
                (alloc::format!("HD_{} Hard disk, {}", disks - 1, size_text(bytes)), DS_TYPE_DISK, DS_REMOVABLE_MEDIA, LDEV_CLASS_DASD)
            };
            let dev = t.device(i, desc.clone(), ty, flags);
            t.nodes[dev].handle = handle(d.handle, 1 + u as u8);
            let host = t.nodes[dev].handle;
            let drv = t.nodes[dev].driver;
            let l = t.add(None, Kind::LDev { ty: class, host }, desc, drv, Vec::new());
            t.nodes[l].handle = handle(d.handle, 0x80 + u as u8);
        }
    }
}

/// The devices below a standard adapter: the keyboard on its
/// controller, the mouse on its port, the diskette drive.
fn child_devices(t: &mut Tree, prefix: &str, n: usize) {
    match prefix {
        "KBC" => {
            t.device(n, s("KBD_0 Keyboard"), DS_TYPE_IO, 0);
        }
        "PS2M" => {
            t.device(n, s("MOUSE_0 Mouse"), DS_TYPE_IO, 0);
        }
        "FDC" => {
            let types = crate::rtc::cmos(0x10);
            for (i, ty) in [types >> 4, types & 0xf].iter().enumerate() {
                if *ty != 0 {
                    t.device(n, alloc::format!("DSKT_{i} Diskette drive {}:", (b'A' + i as u8) as char), DS_TYPE_DISK, DS_REMOVABLE_MEDIA);
                }
            }
        }
        _ => {}
    }
}

/// The tree (built on first use, and again once USB devices changed).
pub fn tree() -> Arc<Tree> {
    let count = xhci::generation().wrapping_add(USB_STORAGE_GEN.load(core::sync::atomic::Ordering::Relaxed));
    let mut g = TREE.lock();
    if let Some((at, t)) = &*g {
        if *at == count {
            return t.clone();
        }
    }
    let t = Arc::new(build());
    crate::trace!("resmgr: {} nodes", t.nodes.len());
    *g = Some((count, t.clone()));
    t
}

// ---- RESMGR$ ----

/// The node's RMNODE, structure, strings and resources laid out at
/// linear address `lin` (after RMNodeSize).
fn node_info(t: &Tree, n: &Node, lin: u32) -> Vec<u8> {
    let mut b = alloc::vec![0u8; 0x14];
    let put32 = |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let put16 = |b: &mut Vec<u8>, o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    // Strings go after the structure: (offset of the pointer, text).
    let mut strings: Vec<(usize, &str)> = Vec::new();
    let st = b.len();
    let (ty, size) = match &n.kind {
        Kind::Adapter { .. } => (RMTYPE_ADAPTER, 0x18),
        Kind::Device { .. } => (RMTYPE_DEVICE, 0x0c),
        Kind::LDev { .. } => (RMTYPE_LDEV, 0x10),
        Kind::Driver { .. } => (RMTYPE_DRIVER, 0x1c),
    };
    b.resize(st + size, 0);
    match &n.kind {
        Kind::Adapter { base, sub, intf, bus, width } => {
            strings.push((st, &n.desc));
            put16(&mut b, st + 4, 0);
            put16(&mut b, st + 6, *base);
            put16(&mut b, st + 8, *sub);
            put16(&mut b, st + 0x0a, *intf);
            put16(&mut b, st + 0x0c, *bus);
            put16(&mut b, st + 0x0e, *width);
        }
        Kind::Device { ty, flags } => {
            strings.push((st, &n.desc));
            put16(&mut b, st + 4, *flags);
            put16(&mut b, st + 6, *ty);
        }
        Kind::LDev { ty, host } => {
            strings.push((st, &n.desc));
            put16(&mut b, st + 6, *ty);
            put32(&mut b, st + 8, *host);
        }
        Kind::Driver { name, vendor, ty, sub } => {
            strings.push((st, name));
            strings.push((st + 4, &n.desc));
            strings.push((st + 8, vendor));
            let v = env!("CARGO_PKG_VERSION");
            let mut it = v.split('.').map(|x| x.parse::<u8>().unwrap_or(0));
            b[st + 0x0c] = it.next().unwrap_or(0);
            b[st + 0x0d] = it.next().unwrap_or(0);
            put16(&mut b, st + 0x14, *ty);
            put16(&mut b, st + 0x16, *sub);
        }
    }
    for (o, text) in strings {
        let at = lin + b.len() as u32;
        put32(&mut b, o, at);
        b.extend(text.bytes());
        b.push(0);
    }
    while b.len() % 4 != 0 {
        b.push(0);
    }
    // RMNODE.
    put32(&mut b, 0, 0x0001_0000);
    put32(&mut b, 4, ty);
    put32(&mut b, 8, n.driver);
    put32(&mut b, 0x0c, lin + st as u32);
    if !n.res.is_empty() {
        let at = lin + b.len() as u32;
        put32(&mut b, 0x10, at);
        b.extend((n.res.len() as u32).to_le_bytes());
        for r in &n.res {
            let mut e = [0u8; 20];
            let (kind, body): (u32, Vec<u16>) = match *r {
                Res::Io { base, len, flags, lines } => (RS_TYPE_IO, alloc::vec![base, len, flags, lines]),
                Res::Irq { level, pin, flags } => (RS_TYPE_IRQ, alloc::vec![level, pin, flags]),
                Res::Mem { base, size, flags } => (RS_TYPE_MEM, alloc::vec![base as u16, (base >> 16) as u16, size as u16, (size >> 16) as u16, flags]),
                Res::Dma { channel, flags } => (RS_TYPE_DMA, alloc::vec![channel, flags]),
            };
            e[0..4].copy_from_slice(&kind.to_le_bytes());
            for (k, w) in body.iter().enumerate() {
                e[4 + 2 * k..6 + 2 * k].copy_from_slice(&w.to_le_bytes());
            }
            b.extend_from_slice(&e);
        }
    }
    let _ = t;
    b
}

/// DosDevIOCtl on `RESMGR$` (category 0x80).
pub(crate) fn ioctl(c: &mut Ctx, r: &crate::api::ioctl::Req) -> u32 {
    use crate::api::ioctl::{put_data, set_lens};
    const TOO_SMALL: u32 = 0xff0c;
    match r.function {
        0 => put_data(c, r, 0, &0u32.to_le_bytes()),
        1 => {
            let Some(p) = c.read_vec(r.params, 8) else { return err::INVALID_ADDRESS };
            let h = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            let lin = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
            let t = tree();
            let Some(n) = t.by_handle(h) else { return 0xff13 };
            let base = if lin != 0 { lin } else { r.data };
            let body = node_info(&t, n, base + 4);
            let need = body.len() as u32 + 4;
            if r.data_len < 4 || !c.write_u32(r.data, need) {
                return err::INVALID_ADDRESS;
            }
            if need > r.data_len {
                set_lens(c, r, 8, 4);
                return TOO_SMALL;
            }
            if !c.write(r.data + 4, &body) {
                return err::INVALID_ADDRESS;
            }
            set_lens(c, r, 8, need);
            0
        }
        2 => {
            let Some(cmd) = c.read_u16(r.params) else { return err::INVALID_ADDRESS };
            if cmd > 4 {
                return err::INVALID_PARAMETER;
            }
            let list = tree().enumerate(cmd);
            if r.data_len < 4 || !c.write_u32(r.data, list.len() as u32) {
                return err::INVALID_ADDRESS;
            }
            let room = ((r.data_len - 4) / 8) as usize;
            let mut b = Vec::new();
            for &(h, depth) in list.iter().take(room) {
                b.extend(h.to_le_bytes());
                b.extend(depth.to_le_bytes());
            }
            if !b.is_empty() && !c.write(r.data + 4, &b) {
                return err::INVALID_ADDRESS;
            }
            set_lens(c, r, 2, 4 + b.len() as u32);
            if list.len() > room { TOO_SMALL } else { 0 }
        }
        _ => err::INVALID_FUNCTION,
    }
}
