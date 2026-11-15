//! `bootdiag` on the kernel command line: show how far boot got on a
//! machine without a serial port, where a hang before the screen works
//! (or one that leaves the loader's text standing) shows nothing else.
//!
//! Every stage draws the next small square in a row at the top of the
//! framebuffer (right-aligned, 48 to a row), straight into its pixels (no font, no screen lock, no
//! interrupts needed), and once the screen works also prints
//! `bootdiag: N name`. The loader draws the first squares (blue: boot
//! services left, about to enter the kernel), the kernel the rest (green,
//! every fifth one yellow for counting). The last square tells where it
//! stopped: count them and look the number up in the serial log of a
//! QEMU boot with `bootdiag`, which prints the same numbers.
//!
//! `bootdiag=N` also stops the machine after square N (to photograph the
//! screen before the desktop covers the kernel's text, or to look at the
//! squares in QEMU, where boot is too quick to see them).

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use bootinfo::{BootInfo, Framebuffer};

static ON: AtomicBool = AtomicBool::new(false);
/// `bootdiag=N`: stop after square N.
static STOP_AT: AtomicU32 = AtomicU32::new(u32::MAX);
/// The next square. The loader's are before it.
static NEXT: AtomicU32 = AtomicU32::new(bootinfo::DIAG_LOADER_SQUARES);
static mut FB: Framebuffer = Framebuffer { base: 0, size: 0, width: 0, height: 0, stride: 0, format: 0 };

/// First thing in the kernel: `info` as the loader passed it (checked
/// here, as nothing else has looked at it yet).
pub fn init(info: &BootInfo) {
    if info.magic != bootinfo::MAGIC {
        return;
    }
    let stop = info.cmdline().split_whitespace().find_map(|w| w.strip_prefix("bootdiag=")?.parse::<u32>().ok());
    if stop.is_none() && !bootinfo::has_word(info.cmdline(), "bootdiag") {
        return;
    }
    STOP_AT.store(stop.unwrap_or(u32::MAX), Ordering::Relaxed);
    unsafe { FB = info.framebuffer };
    ON.store(true, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Boot reached `name`.
pub fn stage(name: &str) {
    if !on() {
        return;
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let fb = unsafe { FB };
    // The whole row again: setting up the screen clears what was drawn
    // before it.
    for i in 0..=n {
        let rgb = if i < bootinfo::DIAG_LOADER_SQUARES {
            0x0060ff
        } else if i % 5 == 4 {
            0xe0e000
        } else {
            0x00c000
        };
        bootinfo::diag_square(&fb, bootinfo::PHYS_MAP_BASE, i, rgb);
    }
    crate::println!("bootdiag: {n} {name}");
    if n == STOP_AT.load(Ordering::Relaxed) {
        crate::println!("bootdiag: stopped here (bootdiag={n})");
        crate::arch::halt_forever();
    }
}

/// A finer step inside a stage (a PCI function being probed): text only,
/// so the squares keep counting stages.
pub fn note(args: core::fmt::Arguments) {
    if on() {
        crate::println!("bootdiag: {args}");
    }
}
