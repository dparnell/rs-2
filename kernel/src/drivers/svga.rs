//! VMware SVGA II display adapter (PCI 15ad:0405), in VMware VMs and
//! QEMU's `vmware-svga`.
//!
//! The firmware leaves the adapter in the GOP mode the loader picked, and
//! as long as nobody asks for another resolution rs2 leaves it alone.
//! With `video=WxH` on the kernel command line (when the firmware does
//! not offer that mode itself), or through [`set_mode`], the driver takes
//! the adapter over: it programs the mode through the index/value
//! register ports (BAR 0), initialises the command FIFO (BAR 2) and moves
//! the screen to the new framebuffer layout in VRAM (BAR 1).
//!
//! Once the FIFO is enabled the host may only refresh the screen when it
//! gets UPDATE commands (QEMU does that; VMware usually traces writes to
//! the framebuffer, which SVGA_REG_TRACES asks for explicitly), so the
//! timer sends an UPDATE for each damaged rectangle
//! ([`crate::display::take_damage`]: what the text screens and PM drew)
//! about 60 times a second.
//!
//! With the cursor capabilities the mouse pointer is a hardware cursor:
//! PM's server defines it once (DEFINE_CURSOR, a 32x32 AND mask and
//! 32-bit image) and the timer tick moves it through the cursor
//! registers. The driver takes the adapter over (at the mode it has) when
//! PM starts, so that the FIFO is there for it.

use core::sync::atomic::{AtomicBool, Ordering};

use bootinfo::Framebuffer;

use super::pci;
use crate::arch::{inl, outl};
use crate::mm::mmio;
use crate::sync::SpinLock;

const SVGA_ID_2: u32 = 0x9000_0002;

// Registers (index port: BAR 0 + 0, value port: BAR 0 + 1).
const REG_ID: u32 = 0;
const REG_ENABLE: u32 = 1;
const REG_WIDTH: u32 = 2;
const REG_HEIGHT: u32 = 3;
const REG_MAX_WIDTH: u32 = 4;
const REG_MAX_HEIGHT: u32 = 5;
const REG_BITS_PER_PIXEL: u32 = 7;
const REG_RED_MASK: u32 = 9;
const REG_BYTES_PER_LINE: u32 = 12;
const REG_FB_START: u32 = 13;
const REG_FB_OFFSET: u32 = 14;
const REG_VRAM_SIZE: u32 = 15;
const REG_FB_SIZE: u32 = 16;
const REG_CAPABILITIES: u32 = 17;
const REG_MEM_START: u32 = 18;
const REG_MEM_SIZE: u32 = 19;
const REG_CONFIG_DONE: u32 = 20;
const REG_SYNC: u32 = 21;
const REG_BUSY: u32 = 22;
const REG_GUEST_ID: u32 = 23;
const REG_CURSOR_ID: u32 = 24;
const REG_CURSOR_X: u32 = 25;
const REG_CURSOR_Y: u32 = 26;
const REG_CURSOR_ON: u32 = 27;
const REG_TRACES: u32 = 45;

const CAP_CURSOR: u32 = 0x0000_0020;
const CAP_CURSOR_BYPASS: u32 = 0x0000_0040;
const CAP_CURSOR_BYPASS_2: u32 = 0x0000_0080;
const CAP_EXTENDED_FIFO: u32 = 0x0000_8000;
const CAP_TRACES: u32 = 0x0020_0000;

// FIFO registers (dword indexes).
const FIFO_MIN: usize = 0;
const FIFO_MAX: usize = 1;
const FIFO_NEXT_CMD: usize = 2;
const FIFO_STOP: usize = 3;
/// Registers of the extended FIFO (SVGA_FIFO_NUM_REGS).
const FIFO_NUM_REGS: u32 = 291;

const CMD_UPDATE: u32 = 1;
const CMD_DEFINE_CURSOR: u32 = 19;
/// Cursor images are 32x32 (one mask word per row).
const CURSOR_DIM: u32 = 32;

/// Guest OS identifier for SVGA_REG_GUEST_ID: OS/2 in VMware's list.
const GUEST_OS2: u32 = 0x5009;

/// Modes offered to a display settings dialog (those that fit).
const MODES: [(u32, u32); 10] = [
    (640, 480),
    (800, 600),
    (1024, 768),
    (1152, 864),
    (1280, 800),
    (1280, 1024),
    (1400, 1050),
    (1600, 1200),
    (1920, 1080),
    (1920, 1200),
];

struct Svga {
    io: u16,
    /// VRAM: physical address and size.
    vram: u64,
    vram_size: u64,
    /// The FIFO, mapped, and its size in bytes.
    fifo: *mut u32,
    fifo_size: u32,
    caps: u32,
    max: (u32, u32),
    /// The FIFO is set up (the driver owns the adapter).
    enabled: bool,
    width: u32,
    height: u32,
    /// A cursor is defined; the position and visibility last set.
    cursor: bool,
    cursor_sent: Option<(u32, u32, bool)>,
}

unsafe impl Send for Svga {}

static SVGA: SpinLock<Option<Svga>> = SpinLock::new(None);
/// Whether [`tick`] has updates to send.
static ACTIVE: AtomicBool = AtomicBool::new(false);

impl Svga {
    fn read(&self, reg: u32) -> u32 {
        unsafe {
            outl(self.io, reg);
            inl(self.io + 1)
        }
    }

    fn write(&self, reg: u32, v: u32) {
        unsafe {
            outl(self.io, reg);
            outl(self.io + 1, v);
        }
    }

    fn fifo_get(&self, i: usize) -> u32 {
        unsafe { self.fifo.add(i).read_volatile() }
    }

    fn fifo_set(&self, i: usize, v: u32) {
        unsafe { self.fifo.add(i).write_volatile(v) }
    }

    /// Set up the command FIFO and tell the device the driver is ready.
    fn enable_fifo(&mut self) {
        let min = if self.caps & CAP_EXTENDED_FIFO != 0 { FIFO_NUM_REGS * 4 } else { 16 };
        self.fifo_set(FIFO_MIN, min);
        self.fifo_set(FIFO_MAX, self.fifo_size);
        self.fifo_set(FIFO_NEXT_CMD, min);
        self.fifo_set(FIFO_STOP, min);
        self.write(REG_CONFIG_DONE, 1);
        if self.caps & CAP_TRACES != 0 {
            self.write(REG_TRACES, 1);
        }
        self.enabled = true;
    }

    /// Append a command to the FIFO; with `wait`, make room by letting
    /// the device catch up, otherwise give up when the FIFO is full.
    fn fifo_cmd(&self, words: &[u32], wait: bool) -> bool {
        let (min, max) = (self.fifo_get(FIFO_MIN), self.fifo_get(FIFO_MAX));
        let range = max - min;
        let need = words.len() as u32 * 4;
        let free = |s: &Self| {
            let (next, stop) = (s.fifo_get(FIFO_NEXT_CMD), s.fifo_get(FIFO_STOP));
            range - (next + range - stop) % range - 4
        };
        if free(self) < need {
            if !wait {
                return false;
            }
            self.write(REG_SYNC, 1);
            for _ in 0..1_000_000 {
                if self.read(REG_BUSY) == 0 || free(self) >= need {
                    break;
                }
            }
            if free(self) < need {
                return false;
            }
        }
        let mut next = self.fifo_get(FIFO_NEXT_CMD);
        for &w in words {
            self.fifo_set(next as usize / 4, w);
            next += 4;
            if next >= max {
                next = min;
            }
        }
        core::sync::atomic::fence(Ordering::SeqCst);
        self.fifo_set(FIFO_NEXT_CMD, next);
        true
    }

    fn update(&self, wait: bool) -> bool {
        self.update_rect(0, 0, self.width, self.height, wait)
    }

    fn update_rect(&self, x: u32, y: u32, w: u32, h: u32, wait: bool) -> bool {
        let ok = self.fifo_cmd(&[CMD_UPDATE, x, y, w, h], wait);
        if ok {
            crate::display::count_update(w, h);
        }
        ok
    }

    /// Program a mode; returns the new framebuffer.
    fn set_mode(&mut self, w: u32, h: u32) -> Option<Framebuffer> {
        if w == 0 || h == 0 || w > self.max.0 || h > self.max.1 || w as u64 * h as u64 * 4 > self.vram_size {
            return None;
        }
        self.write(REG_WIDTH, w);
        self.write(REG_HEIGHT, h);
        self.write(REG_BITS_PER_PIXEL, 32);
        self.write(REG_ENABLE, 1);
        self.write(REG_GUEST_ID, GUEST_OS2);
        if !self.enabled {
            self.enable_fifo();
        }
        let (rw, rh) = (self.read(REG_WIDTH), self.read(REG_HEIGHT));
        let pitch = self.read(REG_BYTES_PER_LINE);
        let offset = self.read(REG_FB_OFFSET) as u64;
        let fb_size = self.read(REG_FB_SIZE) as u64;
        if (rw, rh) != (w, h) || pitch < w * 4 || offset + pitch as u64 * h as u64 > self.vram_size {
            crate::println!("svga: mode {w}x{h} refused (got {rw}x{rh}, pitch {pitch})");
            return None;
        }
        self.width = w;
        self.height = h;
        let format = if self.read(REG_RED_MASK) == 0x00ff_0000 { 1 } else { 0 };
        Some(Framebuffer {
            base: self.vram + offset,
            size: fb_size.max(pitch as u64 * h as u64),
            width: w,
            height: h,
            stride: pitch / 4,
            format,
        })
    }
}

/// Find the adapter; switch to the `video=` mode if the loader could not
/// set it through the firmware.
pub fn probe(dev: &pci::Device) {
    dev.enable();
    let io = dev.bar(0) as u16;
    let mut s = Svga {
        io,
        vram: 0,
        vram_size: 0,
        fifo: core::ptr::null_mut(),
        fifo_size: 0,
        caps: 0,
        max: (0, 0),
        enabled: false,
        width: 0,
        height: 0,
        cursor: false,
        cursor_sent: None,
    };
    s.write(REG_ID, SVGA_ID_2);
    if io == 0 || s.read(REG_ID) != SVGA_ID_2 {
        crate::println!("svga: VMware SVGA II does not support version 2");
        return;
    }
    s.vram = s.read(REG_FB_START) as u64;
    s.vram_size = s.read(REG_VRAM_SIZE) as u64;
    s.caps = s.read(REG_CAPABILITIES);
    s.max = (s.read(REG_MAX_WIDTH), s.read(REG_MAX_HEIGHT));
    let (fifo, fifo_size) = (s.read(REG_MEM_START) as u64, s.read(REG_MEM_SIZE));
    if s.vram == 0 || fifo == 0 || fifo_size < 4096 {
        crate::println!("svga: bad VRAM/FIFO configuration");
        return;
    }
    s.fifo = mmio::map(fifo, fifo_size as u64) as *mut u32;
    s.fifo_size = fifo_size;
    crate::println!(
        "svga: VMware SVGA II, {} MB VRAM, up to {}x{}, capabilities {:#x}",
        s.vram_size >> 20,
        s.max.0,
        s.max.1,
        s.caps
    );
    *SVGA.lock() = Some(s);
    let want = bootinfo::video_mode(crate::boot_info().cmdline());
    let cur = crate::screen::framebuffer().map(|f| (f.width, f.height));
    if let Some((w, h)) = want {
        if Some((w, h)) != cur && !set_mode(w, h) {
            crate::println!("svga: cannot set {w}x{h}");
        }
    }
}

/// Whether the driver can change modes.
pub fn present() -> bool {
    SVGA.lock().is_some()
}

/// Resolutions that fit the adapter (for a display settings dialog).
pub fn modes() -> alloc::vec::Vec<(u32, u32)> {
    let g = SVGA.lock();
    let Some(s) = g.as_ref() else { return alloc::vec::Vec::new() };
    MODES.iter().copied().filter(|&(w, h)| w <= s.max.0 && h <= s.max.1 && w as u64 * h as u64 * 4 <= s.vram_size).collect()
}

/// Switch to `w` x `h` (32 bits per pixel) and move the screens to the
/// new framebuffer. The framebuffer must stay in the kernel's physical
/// memory map.
pub fn set_mode(w: u32, h: u32) -> bool {
    let fb = {
        let mut g = SVGA.lock();
        let Some(s) = g.as_mut() else { return false };
        let Some(fb) = s.set_mode(w, h) else { return false };
        fb
    };
    // The loader maps physical memory up to 4 GB or the end of the boot
    // framebuffer, whichever is higher.
    let top = crate::screen::framebuffer().map_or(0, |f| f.base + f.size).max(4 << 30);
    if fb.base + fb.stride as u64 * 4 * fb.height as u64 > top {
        crate::println!("svga: framebuffer at {:#x} outside the memory map", fb.base);
        return false;
    }
    crate::screen::set_framebuffer(fb);
    crate::println!("svga: mode {w}x{h}");
    if let Some(s) = SVGA.lock().as_mut() {
        s.update(true);
        s.cursor_sent = None;
    }
    ACTIVE.store(true, Ordering::Relaxed);
    true
}

/// Timer tick: refresh what changed about 60 times a second while the
/// driver owns the adapter.
pub fn tick(ticks: u64) {
    if ticks % 16 != 8 || !ACTIVE.load(Ordering::Relaxed) || crate::smp::this().index() != 0 {
        return;
    }
    let Some(g) = SVGA.try_lock() else { return };
    let Some(s) = g.as_ref() else { return };
    let d = crate::display::take_damage(s.width, s.height);
    for (i, &(x0, y0, x1, y1)) in d.rects().iter().enumerate() {
        if !s.update_rect(x0, y0, x1 - x0, y1 - y0, false) {
            // The FIFO is full: the rest next time.
            crate::display::requeue_damage(&d.rects()[i..]);
            break;
        }
    }
}

/// PM is starting: own the adapter (at the mode it has, if the firmware
/// set it) so that the FIFO is available for a hardware cursor.
pub fn take_over() {
    let enabled = SVGA.lock().as_ref().map(|s| s.enabled);
    if enabled != Some(false) {
        return;
    }
    if let Some(fb) = crate::screen::framebuffer() {
        set_mode(fb.width, fb.height);
    }
}

/// The memory PM maps (taking the adapter over first): VRAM from its
/// start, as much as the largest mode needs (at most what a 3840x2160
/// screen takes), so that a mode change under PM stays inside.
pub fn pm_area() -> Option<(u64, u64)> {
    take_over();
    let g = SVGA.lock();
    let s = g.as_ref().filter(|s| s.enabled)?;
    let need = (s.max.0 as u64 * s.max.1 as u64 * 4).min(PM_AREA_MAX);
    Some((s.vram, s.vram_size.min(need)))
}

/// Largest VRAM area mapped into PM processes.
const PM_AREA_MAX: u64 = 3840 * 2160 * 4;

/// Define the hardware cursor: `pixels` (0xAARRGGBB, `w` x `h`, at most
/// 32x32; alpha 0 is transparent) with the hot spot at (`hx`, `hy`).
/// False if the adapter has no cursor support.
pub fn define_cursor(w: u32, h: u32, hx: u32, hy: u32, pixels: &[u32]) -> bool {
    if w > CURSOR_DIM || h > CURSOR_DIM || pixels.len() < (w * h) as usize {
        return false;
    }
    let mut g = SVGA.lock();
    let Some(s) = g.as_mut() else { return false };
    let need = CAP_CURSOR | CAP_CURSOR_BYPASS | CAP_CURSOR_BYPASS_2;
    if !s.enabled || s.caps & need != need {
        return false;
    }
    let n = CURSOR_DIM;
    let mut words = alloc::vec![CMD_DEFINE_CURSOR, 0, hx, hy, n, n, 1, 32];
    // AND mask: 1 bit per pixel, most significant bit first in each
    // byte, a 32-bit word per row; set where the cursor is transparent.
    for y in 0..n {
        let mut bytes = [0xffu8; 4];
        for x in 0..n {
            let a = if x < w && y < h { pixels[(y * w + x) as usize] >> 24 } else { 0 };
            if a >= 0x80 {
                bytes[x as usize / 8] &= !(0x80 >> (x % 8));
            }
        }
        words.push(u32::from_le_bytes(bytes));
    }
    for y in 0..n {
        for x in 0..n {
            let v = if x < w && y < h { pixels[(y * w + x) as usize] } else { 0 };
            words.push(if v >> 24 >= 0x80 { v & 0x00ff_ffff } else { 0 });
        }
    }
    if !s.fifo_cmd(&words, true) {
        return false;
    }
    s.write(REG_CURSOR_ID, 0);
    s.cursor = true;
    s.cursor_sent = None;
    crate::println!("svga: hardware cursor {w}x{h}");
    true
}

/// Timer tick: put the cursor at (`x`, `y`) (pixels from the top left),
/// or hide it.
pub fn move_cursor(x: u32, y: u32, visible: bool) {
    let Some(mut g) = SVGA.try_lock() else { return };
    let Some(s) = g.as_mut() else { return };
    if !s.cursor || s.cursor_sent == Some((x, y, visible)) {
        return;
    }
    s.write(REG_CURSOR_ID, 0);
    s.write(REG_CURSOR_X, x);
    s.write(REG_CURSOR_Y, y);
    s.write(REG_CURSOR_ON, visible as u32);
    s.cursor_sent = Some((x, y, visible));
}
