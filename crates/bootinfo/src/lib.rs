//! The structure the loader hands to the kernel.
//!
//! Everything is `repr(C)` and position independent: pointers are physical
//! addresses, which the kernel reads through its direct map.

#![no_std]

/// Virtual address at which all physical memory is mapped.
pub const PHYS_MAP_BASE: u64 = 0xFFFF_8000_0000_0000;
/// Virtual base of the kernel image.
pub const KERNEL_BASE: u64 = 0xFFFF_FFFF_8000_0000;
/// `BootInfo::magic`.
pub const MAGIC: u64 = u64::from_le_bytes(*b"RS2BOOT1");

pub const MAX_MEMORY_REGIONS: usize = 512;
pub const MAX_MODULES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryKind {
    /// Free for the kernel to use.
    Usable = 1,
    /// Holds the kernel image, boot info, page tables or a module. Must not
    /// be reused until the kernel is done with what is there.
    Loader = 2,
    /// Firmware runtime services; must stay mapped.
    Runtime = 3,
    /// ACPI tables that can be reclaimed after they have been parsed.
    AcpiReclaim = 4,
    /// Anything else (MMIO, reserved, ACPI NVS, broken).
    Reserved = 5,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryRegion {
    pub start: u64,
    pub len: u64,
    pub kind: MemoryKind,
    pub _pad: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Framebuffer {
    /// Physical address; 0 if there is no framebuffer.
    pub base: u64,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    /// Pixels per scan line.
    pub stride: u32,
    /// 0 = RGBX (red in the low byte), 1 = BGRX.
    pub format: u32,
}

/// A file loaded by the loader on the kernel's behalf.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Module {
    pub name: [u8; 32],
    pub base: u64,
    pub len: u64,
}

impl Module {
    pub fn name(&self) -> &str {
        let n = self.name.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.name[..n]).unwrap_or("?")
    }
}

#[repr(C)]
pub struct BootInfo {
    pub magic: u64,
    pub framebuffer: Framebuffer,
    /// Physical address of the ACPI RSDP, 0 if unknown.
    pub rsdp: u64,
    /// Kernel command line (NUL padded).
    pub cmdline: [u8; 256],
    pub memory_count: u64,
    pub memory: [MemoryRegion; MAX_MEMORY_REGIONS],
    pub module_count: u64,
    pub modules: [Module; MAX_MODULES],
    /// Physical address and size of the kernel image as loaded.
    pub kernel_phys: u64,
    pub kernel_len: u64,
}

impl BootInfo {
    pub fn memory(&self) -> &[MemoryRegion] {
        &self.memory[..self.memory_count as usize]
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules[..self.module_count as usize]
    }

    pub fn cmdline(&self) -> &str {
        let n = self.cmdline.iter().position(|&b| b == 0).unwrap_or(256);
        core::str::from_utf8(&self.cmdline[..n]).unwrap_or("")
    }
}
