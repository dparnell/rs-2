//! virtio-gpu: the paravirtual display of QEMU/KVM (`virtio-vga`,
//! `virtio-gpu-pci`), driven in 2D mode.
//!
//! The device has no framebuffer of its own: the driver creates a 2D
//! resource on the host (B8G8R8X8, i.e. BGRX in memory, the format the
//! screens and PM call 1), backs it with guest memory, and shows it on
//! scanout 0. The backing is one physically contiguous block of RAM, so it
//! is an ordinary linear framebuffer for `screen` and for PM processes
//! (which map it through `display`). The host only sees what the driver
//! copies over: TRANSFER_TO_HOST_2D moves a rectangle from the backing
//! into the resource and RESOURCE_FLUSH puts it on the display.
//!
//! Nobody tells the driver when pixels change, so the timer tick flushes
//! about 30 times a second: the rows `screen` reports as drawn
//! ([`crate::screen::take_dirty`]) while a text session is shown, the
//! whole screen while PM (which draws straight into the framebuffer) is in
//! the foreground. Tick commands are queued without waiting and reaped on
//! the next tick; commands from thread context (setting a mode) busy-wait
//! for the device under the driver's spin lock.
//!
//! The device is reset when the driver attaches, which ends whatever the
//! firmware showed (OVMF's GOP draws on `virtio-vga` through its VGA
//! compatible framebuffer; on `virtio-gpu-pci` it only offers Blt, so the
//! loader finds no framebuffer and the screens start here). The mode the
//! driver sets is `video=WxH` from the kernel command line if given, else
//! the size of the firmware's framebuffer (what the loader picked), else
//! the display's preferred size from GET_DISPLAY_INFO (QEMU: its `xres` x
//! `yres`, 1280x800 by default). Any size works (no list of modes: the
//! host scales the resource to its window); [`set_mode`] changes it until
//! PM has mapped the framebuffer, as with VMware SVGA II.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use bootinfo::Framebuffer;

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::sync::SpinLock;

// Command and response types.
const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_RESOURCE_UNREF: u32 = 0x0102;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;

/// VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: bytes B, G, R, X.
const FORMAT_B8G8R8X8: u32 = 2;
/// Framebuffer.format of that layout (BGRX).
const FB_BGRX: u32 = 1;

/// struct virtio_gpu_ctrl_hdr.
const HDR: usize = 24;
const MAX_SCANOUTS: usize = 16;

// Offsets in the command page: synchronous request and response, then
// the two commands of a tick flush and their responses.
const SYNC_REQ: usize = 0;
const SYNC_RESP: usize = 0x400;
const XFER_REQ: usize = 0x800;
const XFER_RESP: usize = 0x880;
const FLUSH_REQ: usize = 0x900;
const FLUSH_RESP: usize = 0x980;

/// Polls (each with an uncached register read) before a synchronous
/// command is given up.
const SYNC_POLLS: u32 = 5_000_000;
/// Largest width or height accepted.
const MAX_DIM: u32 = 8192;

/// Sizes offered to a display settings dialog (those that fit in memory).
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

struct Gpu {
    dev: Device,
    q: Virtqueue,
    /// Command and response buffers.
    mem: Dma,
    /// Tick commands on the queue, not yet returned.
    pending: u32,
    /// The resource on scanout 0 and its backing (the framebuffer).
    resource: u32,
    backing: Option<Dma>,
    width: u32,
    height: u32,
    next_id: u32,
    /// The display's preferred size.
    preferred: (u32, u32),
}

static GPU: SpinLock<Option<Gpu>> = SpinLock::new(None);
/// The driver shows a framebuffer ([`tick`] has flushing to do).
static ACTIVE: AtomicBool = AtomicBool::new(false);

impl Gpu {
    fn put(&self, off: usize, words: &[u32]) {
        for (i, w) in words.iter().enumerate() {
            unsafe { (self.mem.ptr(off + i * 4) as *mut u32).write_volatile(*w) };
        }
    }

    fn get(&self, off: usize) -> u32 {
        unsafe { (self.mem.ptr(off) as *const u32).read_volatile() }
    }

    /// Write a request (header of type `cmd`, then `body`) at `off`;
    /// returns its length.
    fn request(&self, off: usize, cmd: u32, body: &[u32]) -> usize {
        self.put(off, &[cmd, 0, 0, 0, 0, 0]);
        self.put(off + HDR, body);
        HDR + body.len() * 4
    }

    /// Collect returned buffers; true once `head` (if any) came back.
    fn reap(&mut self, head: Option<u16>) -> bool {
        let mut found = false;
        while let Some(u) = self.q.pop() {
            if Some(u.head) == head {
                found = true;
            } else {
                self.pending = self.pending.saturating_sub(1);
            }
        }
        found
    }

    /// Run a command and wait for the device: the response type (0 if the
    /// device did not answer). The response is at SYNC_RESP.
    fn call(&mut self, cmd: u32, body: &[u32], resp_len: usize) -> u32 {
        let len = self.request(SYNC_REQ, cmd, body);
        self.put(SYNC_RESP, &[0]);
        let bufs = [Buf::out(self.mem.addr(SYNC_REQ), len), Buf::into(self.mem.addr(SYNC_RESP), resp_len)];
        // Make room if tick commands fill the queue.
        for _ in 0..SYNC_POLLS {
            if self.q.free_count() >= 2 || self.pending == 0 {
                break;
            }
            self.reap(None);
            self.dev.status_reg().read8();
        }
        let Some(head) = self.q.add(&bufs) else { return 0 };
        self.q.kick();
        let pace = self.dev.status_reg();
        for _ in 0..SYNC_POLLS {
            if self.reap(Some(head)) {
                return self.get(SYNC_RESP);
            }
            pace.read8();
        }
        crate::println!("virtio-gpu: command {cmd:#x} timed out");
        0
    }

    fn ok(&mut self, cmd: u32, body: &[u32]) -> bool {
        let r = self.call(cmd, body, HDR);
        if r != RESP_OK_NODATA {
            crate::println!("virtio-gpu: command {cmd:#x} failed ({r:#x})");
        }
        r == RESP_OK_NODATA
    }

    /// GET_DISPLAY_INFO: the enabled scanouts' sizes.
    fn display_info(&mut self) -> Vec<(u32, u32)> {
        let mut v = Vec::new();
        if self.call(CMD_GET_DISPLAY_INFO, &[], HDR + MAX_SCANOUTS * 24) != RESP_OK_DISPLAY_INFO {
            return v;
        }
        for i in 0..MAX_SCANOUTS {
            let at = SYNC_RESP + HDR + i * 24;
            let (w, h, enabled) = (self.get(at + 8), self.get(at + 12), self.get(at + 16));
            if enabled != 0 && w != 0 && h != 0 {
                v.push((w, h));
            }
        }
        v
    }

    /// Create a resource backed by `backing`, show it on scanout 0 and
    /// copy it over. Returns the previous resource and backing.
    fn show(&mut self, w: u32, h: u32, backing: Dma) -> Result<(u32, Option<Dma>), Dma> {
        let id = self.next_id;
        self.next_id += 1;
        if !self.ok(CMD_RESOURCE_CREATE_2D, &[id, FORMAT_B8G8R8X8, w, h]) {
            return Err(backing);
        }
        let (addr, len) = (backing.addr(0), w * h * 4);
        if !self.ok(CMD_RESOURCE_ATTACH_BACKING, &[id, 1, addr as u32, (addr >> 32) as u32, len, 0])
            || !self.ok(CMD_SET_SCANOUT, &[0, 0, w, h, 0, id])
        {
            self.ok(CMD_RESOURCE_UNREF, &[id, 0]);
            return Err(backing);
        }
        let old = (self.resource, self.backing.take());
        self.resource = id;
        self.backing = Some(backing);
        self.width = w;
        self.height = h;
        self.ok(CMD_TRANSFER_TO_HOST_2D, &[0, 0, w, h, 0, 0, id, 0]);
        self.ok(CMD_RESOURCE_FLUSH, &[0, 0, w, h, id, 0]);
        Ok(old)
    }

    /// Queue a transfer and flush of rows `top..bottom` without waiting.
    fn flush_rows(&mut self, top: u32, bottom: u32) -> bool {
        let (w, id) = (self.width, self.resource);
        let bottom = bottom.min(self.height);
        if top >= bottom || id == 0 {
            return true;
        }
        if self.q.free_count() < 4 {
            return false;
        }
        let off = top as u64 * w as u64 * 4;
        let rect = [0, top, w, bottom - top];
        let xl = self.request(XFER_REQ, CMD_TRANSFER_TO_HOST_2D, &[rect[0], rect[1], rect[2], rect[3], off as u32, (off >> 32) as u32, id, 0]);
        let fl = self.request(FLUSH_REQ, CMD_RESOURCE_FLUSH, &[rect[0], rect[1], rect[2], rect[3], id, 0]);
        let x = [Buf::out(self.mem.addr(XFER_REQ), xl), Buf::into(self.mem.addr(XFER_RESP), HDR)];
        let f = [Buf::out(self.mem.addr(FLUSH_REQ), fl), Buf::into(self.mem.addr(FLUSH_RESP), HDR)];
        if self.q.add(&x).is_none() {
            return false;
        }
        self.pending += 1;
        if self.q.add(&f).is_some() {
            self.pending += 1;
        }
        self.q.kick();
        true
    }

    fn framebuffer(&self) -> Option<Framebuffer> {
        let b = self.backing.as_ref()?;
        Some(Framebuffer {
            base: b.addr(0),
            size: self.width as u64 * self.height as u64 * 4,
            width: self.width,
            height: self.height,
            stride: self.width,
            format: FB_BGRX,
        })
    }
}

pub fn attach(mut dev: Device) {
    if dev.negotiate(0).is_none() {
        return;
    }
    let (Some(q), Some(mem)) = (dev.queue(0, 16), Dma::new(4096)) else {
        crate::println!("virtio-gpu {}: cannot set up the control queue", dev.name());
        dev.fail();
        return;
    };
    dev.ready();
    let scanouts = dev.cfg32(8);
    let mut g = Gpu { dev, q, mem, pending: 0, resource: 0, backing: None, width: 0, height: 0, next_id: 1, preferred: (0, 0) };
    let displays = g.display_info();
    g.preferred = displays.first().copied().unwrap_or((1024, 768));
    crate::println!(
        "virtio-gpu {}: {} scanout(s), display {}x{}",
        g.dev.name(),
        scanouts,
        g.preferred.0,
        g.preferred.1
    );
    let preferred = g.preferred;
    *GPU.lock() = Some(g);
    // video= if given, else what the firmware showed, else the display's
    // preferred size.
    let want = bootinfo::video_mode(crate::boot_info().cmdline());
    let fw = crate::screen::framebuffer().map(|f| (f.width, f.height));
    let (w, h) = want.or(fw).unwrap_or(preferred);
    if !set_mode(w, h) && (w, h) != preferred && !set_mode(preferred.0, preferred.1) {
        crate::println!("virtio-gpu: cannot set a mode");
    }
}

/// Whether the driver can change modes.
pub fn present() -> bool {
    GPU.lock().is_some()
}

/// Whether a `w` x `h` framebuffer would be a reasonable share of memory.
fn fits(w: u32, h: u32) -> bool {
    let pages = (w as u64 * h as u64 * 4).div_ceil(4096);
    (1..=MAX_DIM).contains(&w) && (1..=MAX_DIM).contains(&h) && pages * 4 <= crate::mm::phys::free_count()
}

/// Sizes [`set_mode`] can switch to (for a display settings dialog).
pub fn modes() -> Vec<(u32, u32)> {
    let preferred = match GPU.lock().as_ref() {
        Some(g) => g.preferred,
        None => return Vec::new(),
    };
    let mut v: Vec<(u32, u32)> = MODES.iter().copied().filter(|&(w, h)| fits(w, h)).collect();
    if !v.contains(&preferred) && fits(preferred.0, preferred.1) {
        v.push(preferred);
        v.sort_unstable();
    }
    v
}

/// Switch to `w` x `h` (32 bits per pixel, BGRX) and move the screens to
/// the new framebuffer.
pub fn set_mode(w: u32, h: u32) -> bool {
    if GPU.lock().is_none() || !fits(w, h) {
        return false;
    }
    // Allocate (and zero) the backing without holding the spin lock.
    let Some(backing) = Dma::new(w as usize * h as usize * 4) else {
        crate::println!("virtio-gpu: no memory for a {w}x{h} framebuffer");
        return false;
    };
    let (fb, old) = {
        let mut g = GPU.lock();
        let Some(gpu) = g.as_mut() else { return false };
        match gpu.show(w, h, backing) {
            Ok(old) => (gpu.framebuffer().unwrap(), old),
            Err(_) => return false,
        }
    };
    crate::screen::set_framebuffer(fb);
    ACTIVE.store(true, Ordering::Relaxed);
    crate::println!("virtio-gpu: mode {w}x{h}");
    // The screens draw into the new backing now: drop the old resource.
    let mut g = GPU.lock();
    if let Some(gpu) = g.as_mut() {
        if old.0 != 0 {
            gpu.ok(CMD_RESOURCE_UNREF, &[old.0, 0]);
        }
        drop(old.1);
        gpu.flush_rows(0, h);
    }
    true
}

/// Timer tick: copy what changed to the host about 30 times a second.
pub fn tick(ticks: u64) {
    if ticks % 32 != 16 || !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let Some(mut g) = GPU.try_lock() else { return };
    let Some(gpu) = g.as_mut() else { return };
    gpu.reap(None);
    if gpu.pending > 0 {
        // The previous flush is still on its way; try again next time.
        return;
    }
    if let Some((top, bottom)) = crate::screen::take_dirty() {
        if !gpu.flush_rows(top, bottom) {
            crate::screen::mark_dirty(top, bottom);
        }
    }
}
