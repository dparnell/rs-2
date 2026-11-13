//! Three devices that IBM's OS/2 had as device drivers, answering
//! DosDevIOCtl the way IBM's do (studied in OS2LDR, TESTCFG.SYS and
//! APM.SYS with Ghidra):
//!
//! * `OEMHLP$` (category 0x80), which IBM's kernel loader carries: machine,
//!   video, memory, disk and PCI information. rs2 boots by UEFI, so the
//!   answers that IBM's took from the PC BIOS (ROM fonts, the BIOS
//!   configuration table, the PCI BIOS) come from rs2's own knowledge:
//!   PCI configuration space through the 0xCF8 mechanism, the disks rs2
//!   found, the memory it manages.
//! * `TESTCFG$` (category 0x80): the bus type, copies of the adapter ROM
//!   area (C0000-FFFFF) and port I/O for installation programs. Ports owned
//!   by a driver in the resource manager are refused, as IBM's driver's
//!   exclusive RMAllocResource refuses them.
//! * `APM$` (category 0xC, IBM's APM.SYS interface) on rs2's ACPI power
//!   code: power status (AC power, no battery), the power-off event.
//!
//! IBM's drivers write their whole data structure whatever the data
//! length the caller gave (they only check that the memory is writable),
//! so these do too. Errors are device driver status codes, which DosDevIOCtl
//! turns into return codes (`ioctl::dd_error`): category 0x80 gives
//! 0xFF00 + code (0xFF03 unknown function, 0xFF13 invalid parameter,
//! 0xFF0C general failure), category 0xC 87.

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, Ordering};

use super::ioctl::{dd_error, status, Req};
use super::Ctx;
use crate::drivers::pci;

// ---- OEMHLP$ ----

/// Category 0x80 on OEMHLP$.
pub fn oemhlp(c: &Ctx, r: &Req) -> u32 {
    if r.category != 0x80 {
        return dd_error(r.category, status::UNKNOWN_COMMAND);
    }
    let bad = dd_error(r.category, status::INVALID_PARAMETER);
    let out: Vec<u8> = match r.function {
        // OEM adaptation: name (20 bytes) and revision (10 bytes).
        0x00 => {
            let mut v = vec![0u8; 30];
            v[..3].copy_from_slice(b"rs2");
            v[20..25].copy_from_slice(b"20.45");
            v
        }
        // Machine: manufacturer (20), model (10), ROM revision (10).
        // IBM's leaves them empty unless the BIOS carries an IBM machine
        // ID.
        0x01 => vec![0; 40],
        // Display combination code: VGA with an analog colour display.
        0x02 => vec![0x08],
        // ROM fonts: up to six real-mode far pointers, stopping at a
        // word of the caller's buffer that holds 0xFFFF. A UEFI machine
        // has no video BIOS fonts: zeros.
        0x03 => {
            let Some(cur) = c.read_vec(r.data, 24) else { return bad };
            let mut n = 0;
            while n < 12 && u16::from_le_bytes([cur[n * 2], cur[n * 2 + 1]]) != 0xffff {
                n += 1;
            }
            vec![0; n * 2]
        }
        // EISA configuration: not an EISA machine (IBM's answers like an
        // unknown function then).
        0x04 => return dd_error(r.category, status::UNKNOWN_COMMAND),
        // ROM BIOS: model, submodel, revision, ABIOS present (words). A
        // PC/AT-compatible model byte.
        0x05 => vec![0xfc, 0, 0, 0, 0, 0, 0, 0],
        // Miscellaneous video state (INT 10h 1B00h byte 2Dh): VGA colour,
        // blinking.
        0x06 => vec![0x21],
        // Video adapter: VGA (bit 3) with colour memory (bit 1).
        0x07 => vec![0x0a],
        // SVGA: adapter type, chip type, memory (as SCREEN$ reports
        // them; rs2 has no SVGA chip driver of IBM's kind: unknown).
        0x08 => vec![0; 8],
        // Memory: KB below 1 MB (word), KB above 1 MB (dword).
        0x09 => {
            let kb = crate::mm::phys::total_frames() * 4;
            let high = kb.saturating_sub(1024).min(u32::MAX as u64) as u32;
            let mut v = vec![0x80, 0x02];
            v.extend_from_slice(&high.to_le_bytes());
            v
        }
        // DMQS information: a far pointer, none.
        0x0a => vec![0; 4],
        0x0b => return pci_ioctl(c, r),
        // Undocumented (parameter: a dword selector 0-2): IBM's answers
        // with values its loader found; none here.
        0x0c => {
            if r.params_len != 0xffff && r.params_len != 4 {
                return bad;
            }
            match c.read_u16(r.params) {
                Some(0) => vec![0],
                Some(1) => vec![0; 12],
                Some(2) => vec![0; 4],
                Some(_) => Vec::new(),
                None => return bad,
            }
        }
        // Undocumented word and 18-byte structures: nothing to report.
        0x0d => vec![0; 2],
        0x0f => vec![0; 18],
        0x0e => match disk_info(c, r) {
            Some(v) => v,
            None => return bad,
        },
        _ => return dd_error(r.category, status::UNKNOWN_COMMAND),
    };
    if !out.is_empty() && !c.write(r.data, &out) {
        return bad;
    }
    0
}

/// OEMHLP_QUERYDISKINFO: parameter the BIOS drive number (0x80 + n);
/// data OEMHLPDISKINFO (46 bytes). rs2's geometry is the synthetic one of
/// category 9 (255 heads, 63 sectors per track).
fn disk_info(c: &Ctx, r: &Req) -> Option<Vec<u8>> {
    let n = c.read_u8(r.params)?;
    let d = crate::drivers::DISKS.lock().get(n.checked_sub(0x80)? as usize).cloned()?;
    let sectors = d.sector_count();
    let (heads, spt) = (255u64, 63u64);
    let cyl = (sectors / (heads * spt)).min(u32::MAX as u64) as u32;
    let mut v = vec![0u8; 46];
    v[2..4].copy_from_slice(&0x0002u16.to_le_bytes()); // CHS information valid
    v[4..8].copy_from_slice(&cyl.to_le_bytes());
    v[8..12].copy_from_slice(&(heads as u32).to_le_bytes());
    v[12..16].copy_from_slice(&(spt as u32).to_le_bytes());
    v[16..24].copy_from_slice(&sectors.to_le_bytes());
    v[24..26].copy_from_slice(&(d.sector_size() as u16).to_le_bytes());
    Some(v)
}

/// PCI BIOS return codes.
const PCI_DEVICE_NOT_FOUND: u8 = 0x86;
const PCI_BAD_VENDOR_ID: u8 = 0x83;
const PCI_BAD_REGISTER_NUMBER: u8 = 0x87;

static PCI_DEVS: crate::sync::SpinLock<Option<Vec<pci::Device>>> = crate::sync::SpinLock::new(None);

fn pci_devices() -> Vec<pci::Device> {
    let mut g = PCI_DEVS.lock();
    if g.is_none() {
        *g = Some(pci::enumerate());
    }
    g.clone().unwrap_or_default()
}

/// OEMHLP_PCI: the PCI BIOS functions. Parameters start with the
/// subfunction; data with the PCI BIOS return code.
///
/// | sub | parameters | data |
/// |-----|------------|------|
/// | 0 query BIOS | — | rc, hardware mechanism, major, minor, last bus |
/// | 1 find device | USHORT device, USHORT vendor, BYTE index | rc, bus, device/function |
/// | 2 find class | ULONG class code (24 bits), BYTE index | rc, bus, device/function |
/// | 3 read configuration | BYTE bus, devfn, register, size (1, 2, 4) | rc, ULONG value |
/// | 4 write configuration | bus, devfn, register, size, ULONG value | rc |
fn pci_ioctl(c: &Ctx, r: &Req) -> u32 {
    let bad = dd_error(r.category, status::INVALID_PARAMETER);
    let fail = dd_error(r.category, status::GEN_FAILURE);
    let Some(sub) = c.read_u8(r.params) else { return bad };
    let p = |n: u32| c.read_vec(r.params, n);
    // A BIOS error: the code in the data, general failure.
    let error = |code: u8| if c.write_u8(r.data, code) { fail } else { bad };
    match sub {
        0 => {
            let last = pci_devices().iter().map(|d| d.bus).max().unwrap_or(0);
            if c.write(r.data, &[0, 0x01, 0x02, 0x10, last]) { 0 } else { bad }
        }
        1 | 2 => {
            let Some(p) = p(6) else { return bad };
            let index = p[5] as usize;
            let found = if sub == 1 {
                let (device, vendor) = (u16::from_le_bytes([p[1], p[2]]), u16::from_le_bytes([p[3], p[4]]));
                if vendor == 0xffff {
                    return error(PCI_BAD_VENDOR_ID);
                }
                pci_devices().into_iter().filter(|d| d.vendor == vendor && d.device == device).nth(index)
            } else {
                let class = u32::from_le_bytes([p[1], p[2], p[3], 0]);
                pci_devices().into_iter().filter(|d| u32::from_le_bytes([d.progif, d.subclass, d.class, 0]) == class).nth(index)
            };
            match found {
                Some(d) => {
                    if c.write(r.data, &[0, d.bus, d.dev << 3 | d.func]) { 0 } else { bad }
                }
                None => error(PCI_DEVICE_NOT_FOUND),
            }
        }
        3 | 4 => {
            let Some(p) = p(if sub == 3 { 5 } else { 9 }) else { return bad };
            let (bus, dev, func, reg, size) = (p[1], p[2] >> 3, p[2] & 7, p[3], p[4]);
            if !matches!(size, 1 | 2 | 4) {
                return bad;
            }
            if reg as u32 % size as u32 != 0 {
                return error(PCI_BAD_REGISTER_NUMBER);
            }
            let shift = (reg & 3) as u32 * 8;
            let mask = match size {
                1 => 0xffu32,
                2 => 0xffff,
                _ => u32::MAX,
            };
            if sub == 3 {
                let v = (pci::read32(bus, dev, func, reg) >> shift) & mask;
                let mut out = [0u8; 5];
                out[1..].copy_from_slice(&v.to_le_bytes());
                if c.write(r.data, &out) { 0 } else { bad }
            } else {
                let v = u32::from_le_bytes([p[5], p[6], p[7], p[8]]) & mask;
                let old = if size == 4 { 0 } else { pci::read32(bus, dev, func, reg) };
                pci::write32(bus, dev, func, reg, (old & !(mask << shift)) | v << shift);
                if c.write_u8(r.data, 0) { 0 } else { bad }
            }
        }
        _ => bad,
    }
}

// ---- TESTCFG$ ----

/// The ports the kernel uses itself: PCI configuration.
fn kernel_port(port: u32, width: u32) -> bool {
    port < 0xd00 && port + width > 0xcf8
}

/// Whether a driver in the resource manager owns a port in the range.
fn port_owned(port: u16, width: u16) -> bool {
    let (port, width) = (port as u32, width as u32);
    let t = crate::resmgr::tree();
    let owned = t.nodes.iter().any(|n| {
        n.res.iter().any(|r| match *r {
            crate::resmgr::Res::Io { base, len, .. } => (base as u32) < port + width && port < base as u32 + len as u32,
            _ => false,
        })
    });
    owned || kernel_port(port, width)
}

/// Category 0x80 on TESTCFG$:
///
/// | function | parameters | data |
/// |----------|------------|------|
/// | 0x40 copy adapter ROM | ULONG 0, ULONG physical address (C0000-FFFFF), USHORT count | the bytes |
/// | 0x41 IN | USHORT port (0x100 and above), USHORT width (1, 2, 4) | the value |
/// | 0x42 OUT | USHORT port, USHORT width, value | — |
/// | 0x60 query | ULONG 0: bus type / 1: APM BIOS present | ULONG (0 ISA, 1 MCA, 2 EISA) |
/// | 0x61 POS IDs, 0x62 EISA IDs | ULONG 0 | not on this machine: invalid parameter |
/// | 0x63 SCSI inquiry | ULONG 0, BYTE | USHORT count of devices, inquiry data |
pub fn testcfg(c: &Ctx, r: &Req) -> u32 {
    let bad = dd_error(r.category, status::INVALID_PARAMETER);
    if r.category != 0x80 {
        // Category 5 function 0x74 asks a parallel port for its device ID;
        // anything else is refused.
        return bad;
    }
    let zero_param = || c.read_u32(r.params) == Some(0);
    match r.function {
        0x40 => {
            let Some(p) = c.read_vec(r.params, 10) else { return bad };
            let w = |o: usize| u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
            let (phys, n) = (w(4), u16::from_le_bytes([p[8], p[9]]) as u32);
            if w(0) != 0 || !(0xc0000..=0xfffff).contains(&phys) || phys + n > 0x10_0000 {
                return bad;
            }
            let mut buf = vec![0u8; n as usize];
            // The legacy ROM area is always in the kernel's map of physical
            // memory.
            unsafe { core::ptr::copy_nonoverlapping(crate::mm::phys::phys_to_virt(phys as u64), buf.as_mut_ptr(), buf.len()) };
            if c.write(r.data, &buf) { 0 } else { bad }
        }
        0x41 | 0x42 => {
            let Some(p) = c.read_vec(r.params, 8) else { return bad };
            let (port, width) = (u16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]]));
            if port < 0x100 || !matches!(width, 1 | 2 | 4) || port_owned(port, width) {
                return bad;
            }
            use crate::arch::{inb, inl, inw, outb, outl, outw};
            if r.function == 0x41 {
                let v = unsafe {
                    match width {
                        1 => inb(port) as u32,
                        2 => inw(port) as u32,
                        _ => inl(port),
                    }
                };
                if c.write(r.data, &v.to_le_bytes()[..width as usize]) { 0 } else { bad }
            } else {
                let v = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
                unsafe {
                    match width {
                        1 => outb(port, v as u8),
                        2 => outw(port, v as u16),
                        _ => outl(port, v),
                    }
                }
                0
            }
        }
        0x60 => {
            let v: u32 = match c.read_u32(r.params) {
                // ISA (not MCA, not EISA).
                Some(0) => 0,
                // No APM BIOS (rs2's power management is ACPI's).
                Some(1) => 0,
                _ => return bad,
            };
            if c.write_u32(r.data, v) { 0 } else { bad }
        }
        0x61 | 0x62 => bad,
        0x63 => {
            if !zero_param() || c.read_u8(r.params + 4).is_none() {
                return bad;
            }
            if c.write_u16(r.data, 0) { 0 } else { bad }
        }
        _ => bad,
    }
}

// ---- APM$ ----

/// Power management state: bit 0 enabled, bit 1 engaged.
static APM_STATE: AtomicU16 = AtomicU16::new(3);

/// APM.SYS return codes (in the data word).
mod rc {
    pub const BAD_SUBID: u16 = 0x01;
    pub const BAD_RESERVED: u16 = 0x02;
    pub const BAD_DEVICE: u16 = 0x03;
    pub const BAD_STATE: u16 = 0x04;
    pub const BAD_LENGTH: u16 = 0x08;
    pub const DISABLED: u16 = 0x09;
    pub const GEN_FAILURE: u16 = 0x0c;
    pub const CANNOT_ENTER_STATE: u16 = 0x0f;
    pub const NOT_SUPPORTED: u16 = 0x10;
    pub const BAD_FLAG: u16 = 0x14;
}

/// Category 0xC on APM$. The data packet is a USHORT return code; the
/// parameter packet (whose first word is its length for the queries)
/// carries the request and the answer:
///
/// | function | parameters |
/// |----------|------------|
/// | 0x40 send power event | USHORT subid, reserved, data1, data2: 3/4 enable/disable, 5 restore defaults, 6 set power state (data1 device, data2 state: 0 ready, 1 standby, 2 suspend, 3 off), 0xB/0xC engage/disengage |
/// | 0x41 set event semaphore | — |
/// | 0x60 query power status | length, flags, AC, battery status, battery life, time form, USHORT time, battery flags, unit, count |
/// | 0x61 query power event | length, events pending (none) |
/// | 0x62 query power info | length, BIOS flags, BIOS major, minor, driver major, minor, APM level major, minor |
/// | 0x63 query power state | length, device, state |
/// | 0x64 query capabilities | length, ... |
pub fn apm(c: &Ctx, r: &Req) -> u32 {
    let bad = dd_error(r.category, status::INVALID_PARAMETER);
    if r.category != 0xc {
        return dd_error(r.category, status::UNKNOWN_COMMAND);
    }
    // The return code word must be writable.
    match c.read_u16(r.data) {
        Some(v) if c.write_u16(r.data, v) => {}
        _ => return bad,
    }
    let size = match r.function {
        0x40 | 0x43 => 8,
        0x41 | 0x42 => 10,
        0x44 => 4,
        0x45 => 0x1c,
        0x46 | 0x65 => 0x0b,
        0x60 => 0x0d,
        0x61 => 0x0c,
        0x62 => 0x0a,
        0x63 | 0x64 | 0x66 => 6,
        _ => return bad,
    };
    let Some(mut p) = c.read_vec(r.params, size) else { return bad };
    let word = |p: &[u8], o: usize| u16::from_le_bytes([p[o], p[o + 1]]);
    let set = |p: &mut [u8], o: usize, v: u16| p[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let len = word(&p, 0);
    let result = match r.function {
        0x40 => send_event(word(&p, 0), word(&p, 2), word(&p, 4), word(&p, 6)),
        // Event semaphores, daemon registration, broadcasts: accepted;
        // rs2 raises no APM events.
        0x41..=0x44 => 0,
        0x45 | 0x46 | 0x65 | 0x66 => rc::NOT_SUPPORTED,
        0x60 if len < 7 => {
            set(&mut p, 0, 0x0d);
            rc::BAD_LENGTH
        }
        0x60 => {
            let n = if len >= 0x0d { 0x0d } else if len >= 0x0b { 0x0b } else { 7 };
            set(&mut p, 0, n);
            set(&mut p, 2, APM_STATE.load(Ordering::Relaxed));
            // On line, battery status unknown, life unknown, time unknown.
            p[4..7].copy_from_slice(&[1, 0xff, 0xff]);
            if n >= 0x0b {
                p[7] = 0xff;
                set(&mut p, 8, 0xffff);
                p[10] = 0x80; // no system battery
            }
            if n >= 0x0d {
                p[12] = 0; // batteries
            }
            0
        }
        0x61 if len < 0x0c => {
            set(&mut p, 0, 0x0c);
            rc::BAD_LENGTH
        }
        0x61 => {
            set(&mut p, 0, 4);
            set(&mut p, 2, 0);
            0
        }
        0x62 if len < 8 => {
            set(&mut p, 0, 0x0a);
            rc::BAD_LENGTH
        }
        0x62 => {
            set(&mut p, 0, if len >= 0x0a { 0x0a } else { 8 });
            set(&mut p, 2, 0x0003); // 16- and 32-bit interfaces
            p[4..8].copy_from_slice(&[1, 2, 1, 2]);
            if len >= 0x0a {
                p[8..10].copy_from_slice(&[1, 2]);
            }
            0
        }
        0x63 | 0x64 if len < 6 => {
            set(&mut p, 0, 6);
            rc::BAD_LENGTH
        }
        0x63 => {
            if APM_STATE.load(Ordering::Relaxed) & 1 == 0 {
                rc::DISABLED
            } else if !valid_device(word(&p, 2)) {
                rc::BAD_DEVICE
            } else {
                set(&mut p, 0, 6);
                set(&mut p, 4, 0); // ready
                0
            }
        }
        0x64 => {
            set(&mut p, 0, 6);
            set(&mut p, 2, 0);
            set(&mut p, 4, 0);
            0
        }
        _ => rc::BAD_SUBID,
    };
    if r.function >= 0x60 && !c.write(r.params, &p) {
        return bad;
    }
    c.write_u16(r.data, result);
    if result != 0 { bad } else { 0 }
}

/// APM device IDs: 0 the BIOS, 1 all devices it manages, 0x01xx display,
/// 0x02xx storage, 0x03xx parallel, 0x04xx serial, 0x05xx network,
/// 0x06xx PCMCIA, 0x80xx batteries.
fn valid_device(d: u16) -> bool {
    matches!(d >> 8, 0x00..=0x06 | 0x80) && (d >> 8 != 0 || d <= 1)
}

fn send_event(subid: u16, reserved: u16, data1: u16, data2: u16) -> u16 {
    let state = APM_STATE.load(Ordering::Relaxed);
    if reserved != 0 {
        return rc::BAD_RESERVED;
    }
    match subid {
        3 | 4 => {
            let on = subid == 3;
            APM_STATE.store(if on { state | 1 } else { state & !1 }, Ordering::Relaxed);
            0
        }
        5 => {
            APM_STATE.store(3, Ordering::Relaxed);
            0
        }
        6 => {
            if state & 1 == 0 {
                return rc::DISABLED;
            }
            if !valid_device(data1) {
                return rc::BAD_DEVICE;
            }
            match data2 {
                0 => 0,
                // Off: what the shutdown's power-off does.
                3 if data1 <= 1 => {
                    crate::drivers::flush_disks();
                    crate::power::try_power_off();
                    rc::GEN_FAILURE
                }
                1..=3 => rc::CANNOT_ENTER_STATE,
                _ => rc::BAD_STATE,
            }
        }
        // Resume notifications.
        8..=0x0a => 0,
        0x0b | 0x0c => {
            let on = subid == 0x0b;
            APM_STATE.store(if on { state | 2 } else { state & !2 }, Ordering::Relaxed);
            0
        }
        // Timer-based requests, resume on ring: off or on.
        0x13 | 0x14 => {
            if data1 > 1 {
                rc::BAD_FLAG
            } else {
                0
            }
        }
        _ => rc::BAD_SUBID,
    }
}
