//! ACPI tables: MADT (interrupt controllers), FADT (power management,
//! reset), DSDT (just enough to find the S5 sleep type), MCFG.

use alloc::vec::Vec;

use crate::mm::phys::phys_to_virt;
use crate::sync::Once;

#[derive(Debug, Clone, Copy)]
pub struct IoApic {
    pub id: u8,
    pub address: u64,
    pub gsi_base: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Override {
    pub irq: u8,
    pub gsi: u32,
    /// MPS INTI flags: polarity (bits 0-1), trigger mode (bits 2-3).
    pub flags: u16,
}

#[derive(Debug, Default)]
pub struct Info {
    pub lapic: u64,
    pub cpus: Vec<u8>,
    pub ioapics: Vec<IoApic>,
    pub overrides: Vec<Override>,
    pub pm1a_cnt: u32,
    pub pm1b_cnt: u32,
    pub slp_typa: Option<u16>,
    pub slp_typb: Option<u16>,
    /// Reset register (I/O port) and value, if supported.
    pub reset: Option<(u64, u8, u8)>,
    pub sci_irq: u16,
    /// PCI Express configuration space (base, first bus, last bus).
    pub ecam: Option<(u64, u8, u8)>,
}

static INFO: Once<Info> = Once::new();

pub fn info() -> Option<&'static Info> {
    INFO.get()
}

unsafe fn u8_at(a: u64) -> u8 {
    unsafe { *phys_to_virt(a) }
}

unsafe fn u16_at(a: u64) -> u16 {
    unsafe { (phys_to_virt(a) as *const u16).read_unaligned() }
}

unsafe fn u32_at(a: u64) -> u32 {
    unsafe { (phys_to_virt(a) as *const u32).read_unaligned() }
}

unsafe fn u64_at(a: u64) -> u64 {
    unsafe { (phys_to_virt(a) as *const u64).read_unaligned() }
}

fn sig(a: u64) -> [u8; 4] {
    unsafe { [u8_at(a), u8_at(a + 1), u8_at(a + 2), u8_at(a + 3)] }
}

/// All tables from the RSDT/XSDT as (signature, physical address).
fn tables(rsdp: u64) -> Vec<([u8; 4], u64)> {
    let mut v = Vec::new();
    unsafe {
        let rev = u8_at(rsdp + 15);
        let (root, entry) = if rev >= 2 && u64_at(rsdp + 24) != 0 { (u64_at(rsdp + 24), 8) } else { (u32_at(rsdp + 16) as u64, 4) };
        let len = u32_at(root + 4) as u64;
        let n = (len.saturating_sub(36)) / entry;
        for i in 0..n {
            let a = root + 36 + i * entry;
            let t = if entry == 8 { u64_at(a) } else { u32_at(a) as u64 };
            if t != 0 {
                v.push((sig(t), t));
            }
        }
    }
    v
}

fn parse_madt(t: u64, info: &mut Info) {
    unsafe {
        info.lapic = u32_at(t + 36) as u64;
        let len = u32_at(t + 4) as u64;
        let mut o = t + 44;
        while o + 2 <= t + len {
            let ty = u8_at(o);
            let l = u8_at(o + 1) as u64;
            if l < 2 {
                break;
            }
            match ty {
                0 => {
                    if u32_at(o + 4) & 1 != 0 {
                        info.cpus.push(u8_at(o + 3));
                    }
                }
                1 => info.ioapics.push(IoApic { id: u8_at(o + 2), address: u32_at(o + 4) as u64, gsi_base: u32_at(o + 8) }),
                2 => info.overrides.push(Override { irq: u8_at(o + 3), gsi: u32_at(o + 4), flags: u16_at(o + 8) }),
                5 => info.lapic = u64_at(o + 4),
                _ => {}
            }
            o += l;
        }
    }
}

/// Find `_S5_` in AML and read the SLP_TYPa/b values of its package.
fn parse_s5(dsdt: u64, info: &mut Info) {
    unsafe {
        let len = u32_at(dsdt + 4) as u64;
        let mut o = dsdt + 36;
        while o + 4 < dsdt + len {
            if u8_at(o) == b'_' && u8_at(o + 1) == b'S' && u8_at(o + 2) == b'5' && u8_at(o + 3) == b'_' {
                // NameOp (0x08) precedes; PackageOp (0x12) follows.
                let mut p = o + 4;
                if u8_at(p) != 0x12 {
                    o += 1;
                    continue;
                }
                p += 1;
                // PkgLength: the top two bits of the first byte give the
                // number of following bytes.
                let extra = (u8_at(p) >> 6) as u64;
                p += 1 + extra;
                p += 1; // NumElements
                let mut read = || -> u16 {
                    let b = u8_at(p);
                    match b {
                        0x0a => {
                            p += 2;
                            u8_at(p - 1) as u16
                        }
                        0x0b => {
                            p += 3;
                            u16_at(p - 2)
                        }
                        0x00 | 0x01 => {
                            p += 1;
                            b as u16
                        }
                        _ => {
                            p += 1;
                            b as u16
                        }
                    }
                };
                info.slp_typa = Some(read());
                info.slp_typb = Some(read());
                return;
            }
            o += 1;
        }
    }
}

fn parse_fadt(t: u64, info: &mut Info) {
    unsafe {
        let len = u32_at(t + 4);
        info.sci_irq = u16_at(t + 46);
        info.pm1a_cnt = u32_at(t + 64);
        info.pm1b_cnt = u32_at(t + 68);
        if len >= 129 && u32_at(t + 112) & (1 << 10) != 0 {
            // RESET_REG: Generic Address Structure (space, width, offset,
            // access size, address).
            let space = u8_at(t + 116);
            let addr = u64_at(t + 120);
            info.reset = Some((addr, space, u8_at(t + 128)));
        }
        let dsdt = if len >= 148 && u64_at(t + 140) != 0 { u64_at(t + 140) } else { u32_at(t + 40) as u64 };
        if dsdt != 0 && &sig(dsdt) == b"DSDT" {
            parse_s5(dsdt, info);
        }
    }
}

pub fn init(rsdp: u64) {
    if rsdp == 0 {
        crate::println!("acpi: no RSDP");
        return;
    }
    let mut info = Info::default();
    for (s, t) in tables(rsdp) {
        match &s {
            b"APIC" => parse_madt(t, &mut info),
            b"FACP" => parse_fadt(t, &mut info),
            b"MCFG" => unsafe {
                if u32_at(t + 4) >= 44 + 16 {
                    info.ecam = Some((u64_at(t + 44), u8_at(t + 54), u8_at(t + 55)));
                }
            },
            _ => {}
        }
    }
    crate::println!(
        "acpi: {} cpu(s), {} ioapic(s), lapic {:#x}{}",
        info.cpus.len(),
        info.ioapics.len(),
        info.lapic,
        if info.slp_typa.is_some() { ", S5 available" } else { "" }
    );
    INFO.set(info);
}

/// Enter S5 (soft off).
pub fn power_off() {
    let Some(i) = info() else { return };
    let Some(a) = i.slp_typa else { return };
    unsafe {
        if i.pm1a_cnt != 0 {
            crate::arch::outw(i.pm1a_cnt as u16, (a << 10) | (1 << 13));
        }
        if i.pm1b_cnt != 0 {
            if let Some(b) = i.slp_typb {
                crate::arch::outw(i.pm1b_cnt as u16, (b << 10) | (1 << 13));
            }
        }
    }
}

/// Use the FADT reset register, if any.
pub fn reset() {
    let Some((addr, space, val)) = info().and_then(|i| i.reset) else { return };
    unsafe {
        match space {
            1 => crate::arch::outb(addr as u16, val),
            0 => (crate::mm::mmio::map(addr, 1)).write_volatile(val),
            _ => {}
        }
    }
}
