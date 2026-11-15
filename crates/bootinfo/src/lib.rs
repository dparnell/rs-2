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
pub const MAGIC: u64 = u64::from_le_bytes(*b"RS2BOOT2");

/// Room for the kernel command line, its NUL included (RS2BOOT1 had 256,
/// which cut long `init=` argument lists short without a word).
pub const CMDLINE_MAX: usize = 1024;

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

/// The device the loader was started from (from its UEFI device path).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct BootDevice {
    /// 0 = unknown, 1 = hard disk partition, 2 = CD-ROM (El Torito).
    pub kind: u32,
    /// Partition number (1-based) for hard disks.
    pub partition: u32,
    /// Start and size of the partition in 512-byte sectors.
    pub start: u64,
    pub size: u64,
    /// MBR disk signature (first 4 bytes) or GPT partition GUID.
    pub signature: [u8; 16],
    /// 1 = MBR, 2 = GPT.
    pub sig_type: u8,
    pub _pad: [u8; 7],
}

#[repr(C)]
pub struct BootInfo {
    pub magic: u64,
    pub framebuffer: Framebuffer,
    /// Physical address of the ACPI RSDP, 0 if unknown.
    pub rsdp: u64,
    /// Kernel command line (NUL padded).
    pub cmdline: [u8; CMDLINE_MAX],
    pub memory_count: u64,
    pub memory: [MemoryRegion; MAX_MEMORY_REGIONS],
    pub module_count: u64,
    pub modules: [Module; MAX_MODULES],
    /// Physical address and size of the kernel image as loaded.
    pub kernel_phys: u64,
    pub kernel_len: u64,
    pub boot_device: BootDevice,
}

impl BootInfo {
    pub fn memory(&self) -> &[MemoryRegion] {
        &self.memory[..self.memory_count as usize]
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules[..self.module_count as usize]
    }

    pub fn cmdline(&self) -> &str {
        let n = self.cmdline.iter().position(|&b| b == 0).unwrap_or(CMDLINE_MAX);
        core::str::from_utf8(&self.cmdline[..n]).unwrap_or("")
    }
}

/// The display resolution asked for on the kernel command line with
/// `video=WIDTHxHEIGHT` (anything after the height, such as `-32` or
/// `@60`, is ignored).
pub fn video_mode(cmdline: &str) -> Option<(u32, u32)> {
    let v = cmdline.split_whitespace().find_map(|w| w.strip_prefix("video="))?;
    let (w, rest) = v.split_once(['x', 'X'])?;
    let h: &str = rest.split(|c: char| !c.is_ascii_digit()).next()?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w >= 640 && h >= 480).then_some((w, h))
}

/// The number of colours asked for with `video=WIDTHxHEIGHTxCOLORS`
/// (`256`, `65536`/`64K`, `16777216`/`16M`), as OS/2's Screen settings
/// name a mode ("1024 x 768 x 256").
pub fn video_colors(cmdline: &str) -> Option<u32> {
    let v = cmdline.split_whitespace().find_map(|w| w.strip_prefix("video="))?;
    let c = v.split(['x', 'X']).nth(2)?;
    let c = c.split(|c: char| !c.is_ascii_alphanumeric()).next()?;
    if c.eq_ignore_ascii_case("64K") {
        Some(65536)
    } else if c.eq_ignore_ascii_case("16M") {
        Some(1 << 24)
    } else {
        c.parse().ok()
    }
}

/// Whether the command line has `word` (any case).
pub fn has_word(cmdline: &str, word: &str) -> bool {
    cmdline.split_whitespace().any(|w| w.eq_ignore_ascii_case(word))
}

/// `bootdiag` or `bootdiag=N` (stop after stage N) is on the command line.
pub fn diag_on(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|w| w.eq_ignore_ascii_case("bootdiag") || w.starts_with("bootdiag="))
}

/// `bootdiag` squares the loader draws (the kernel's come after them).
pub const DIAG_LOADER_SQUARES: u32 = 2;

/// `bootdiag`: draw square `n` (0 = the leftmost) of the progress row at
/// the top of the framebuffer (right-aligned), colour `rgb` (0xRRGGBB). `map` is
/// where physical memory is mapped (0 in the loader, identity mapped).
/// Squares are 16 pixels with 4 between; a row holds 48, then the next row.
pub fn diag_square(fb: &Framebuffer, map: u64, n: u32, rgb: u32) {
    const SIZE: u32 = 16;
    const STEP: u32 = 20;
    const PER_ROW: u32 = 48;
    if fb.base == 0 || fb.width < PER_ROW * STEP + 16 || fb.height < 4 * STEP + 16 {
        return;
    }
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    let px = if fb.format == 0 { r | g << 8 | b << 16 } else { b | g << 8 | r << 16 };
    let x0 = fb.width - 8 - PER_ROW * STEP + (n % PER_ROW) * STEP;
    let y0 = 8 + (n / PER_ROW).min(3) * STEP;
    let base = (map + fb.base) as *mut u32;
    for y in y0..y0 + SIZE {
        for x in x0..x0 + SIZE {
            // A white frame, so a square shows on any background.
            let edge = y == y0 || y == y0 + SIZE - 1 || x == x0 || x == x0 + SIZE - 1;
            let off = y as usize * fb.stride as usize + x as usize;
            unsafe { base.add(off).write_volatile(if edge { 0x00ff_ffff } else { px }) };
        }
    }
}
