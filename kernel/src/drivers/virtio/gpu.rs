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
//! The timer tick copies what changed about 60 times a second: the
//! damaged rectangles ([`crate::display::take_damage`]: rows the text
//! screens drew, areas PM reports with `Rs2UpdateDisplay`), each with a
//! TRANSFER_TO_HOST_2D and a RESOURCE_FLUSH. Tick commands are queued
//! without waiting (from a page of command slots) and reaped on the next
//! tick; commands from thread context (setting a mode) busy-wait for the
//! device under the driver's spin lock.
//!
//! The device is reset when the driver attaches, which ends whatever the
//! firmware showed (OVMF's GOP draws on `virtio-vga` through its VGA
//! compatible framebuffer; on `virtio-gpu-pci` it only offers Blt, so the
//! loader finds no framebuffer and the screens start here). The mode the
//! driver sets is `video=WxH` from the kernel command line if given, else
//! the size of the firmware's framebuffer (what the loader picked), else
//! the display's preferred size from GET_DISPLAY_INFO (QEMU: its `xres` x
//! `yres`, 1280x800 by default). Any size works (the host scales the
//! resource to its window); [`set_mode`] changes it. When PM maps the
//! framebuffer ([`pm_area`]) the backing grows to what the largest of
//! [`modes`] needs, and from then on every mode uses that memory (a new
//! resource on the same backing), so PM's mapping stays valid. When the
//! host's display configuration changes (its window resized, with
//! QEMU's `zoom-to-fit` off) PM is told and switches to the new
//! preferred size.
//!
//! The mouse pointer can be a hardware cursor (the cursor queue): PM's
//! server defines its image once ([`define_cursor`], a 64x64 B8G8R8A8
//! resource) and the timer tick moves it to where the pointer is
//! ([`move_cursor`]), so it follows the mouse whatever the applications
//! do.

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
const CMD_UPDATE_CURSOR: u32 = 0x0300;
const CMD_MOVE_CURSOR: u32 = 0x0301;
/// VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM (a cursor's pixels: 0xAARRGGBB).
const FORMAT_B8G8R8A8: u32 = 1;
/// Cursor resources are 64x64.
pub const CURSOR_DIM: u32 = 64;
const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;

/// VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: bytes B, G, R, X.
const FORMAT_B8G8R8X8: u32 = 2;
/// Framebuffer.format of that layout (BGRX).
const FB_BGRX: u32 = 1;

/// struct virtio_gpu_ctrl_hdr.
const HDR: usize = 24;
const MAX_SCANOUTS: usize = 16;

// Offsets in the command page: synchronous request and response.
const SYNC_REQ: usize = 0;
const SYNC_RESP: usize = 0x400;
/// Tick commands: slots in a page of their own (request, then the
/// response at +RESP_AT), two per rectangle.
const SLOT: usize = 0x80;
const RESP_AT: usize = 0x40;
/// Rectangles copied per tick at most (the rest wait for the next).
const TICK_RECTS: usize = 12;
/// Control queue size (two descriptors per command).
const QUEUE_SIZE: u16 = 64;
/// struct virtio_gpu_update_cursor on the cursor queue (no response).
const CURSOR_REQ: usize = 0xa00;
const CURSOR_LEN: usize = HDR + 32;

/// Polls (each with an uncached register read) before a synchronous
/// command is given up.
const SYNC_POLLS: u32 = 5_000_000;
/// Configuration: events_read, events_clear; the display event.
const CFG_EVENTS_READ: usize = 0;
const CFG_EVENTS_CLEAR: usize = 4;
const EVENT_DISPLAY: u32 = 1;
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
    /// Command slots of tick flushes.
    tick_mem: Dma,
    /// Tick commands on the queue, not yet returned.
    pending: u32,
    /// The resource on scanout 0 and its backing (the framebuffer), and
    /// the backing's size (it may be larger than the mode, once PM has
    /// mapped it: every mode then uses it).
    resource: u32,
    backing: Option<Dma>,
    backing_size: usize,
    width: u32,
    height: u32,
    next_id: u32,
    /// The display's preferred size.
    preferred: (u32, u32),
    /// The cursor queue, the cursor resource and its backing, the
    /// position and visibility last sent, a command on the queue.
    cursorq: Option<Virtqueue>,
    cursor: u32,
    cursor_backing: Option<Dma>,
    cursor_sent: Option<(u32, u32, bool)>,
    cursor_busy: bool,
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

    /// Create a resource backed by `backing` (`size` bytes; None: the
    /// current backing), show it on scanout 0 and copy it over. Returns
    /// the previous resource and backing (None if it stays).
    fn show(&mut self, w: u32, h: u32, backing: Option<(Dma, usize)>) -> Result<(u32, Option<Dma>), Option<Dma>> {
        let id = self.next_id;
        self.next_id += 1;
        let Some(addr) = backing.as_ref().map(|b| b.0.addr(0)).or(self.backing.as_ref().map(|b| b.addr(0))) else {
            return Err(backing.map(|b| b.0));
        };
        if !self.ok(CMD_RESOURCE_CREATE_2D, &[id, FORMAT_B8G8R8X8, w, h]) {
            return Err(backing.map(|b| b.0));
        }
        let len = w * h * 4;
        if !self.ok(CMD_RESOURCE_ATTACH_BACKING, &[id, 1, addr as u32, (addr >> 32) as u32, len, 0])
            || !self.ok(CMD_SET_SCANOUT, &[0, 0, w, h, 0, id])
        {
            self.ok(CMD_RESOURCE_UNREF, &[id, 0]);
            return Err(backing.map(|b| b.0));
        }
        let mut old = (self.resource, None);
        if let Some((b, size)) = backing {
            old.1 = self.backing.replace(b);
            self.backing_size = size;
        }
        self.resource = id;
        self.width = w;
        self.height = h;
        self.ok(CMD_TRANSFER_TO_HOST_2D, &[0, 0, w, h, 0, 0, id, 0]);
        self.ok(CMD_RESOURCE_FLUSH, &[0, 0, w, h, id, 0]);
        Ok(old)
    }

    /// Queue a transfer and a flush of each rectangle (left, top, right,
    /// bottom) without waiting; returns how many were queued.
    fn flush_rects(&mut self, rects: &[(u32, u32, u32, u32)]) -> usize {
        let (w, id) = (self.width, self.resource);
        if id == 0 {
            return rects.len();
        }
        let mut done = 0;
        for (i, &(x0, y0, x1, y1)) in rects.iter().take(TICK_RECTS).enumerate() {
            let (x1, y1) = (x1.min(w), y1.min(self.height));
            if x0 >= x1 || y0 >= y1 {
                done += 1;
                continue;
            }
            if self.q.free_count() < 4 {
                break;
            }
            let rect = [x0, y0, x1 - x0, y1 - y0];
            let off = (y0 as u64 * w as u64 + x0 as u64) * 4;
            let (xs, fs) = (2 * i * SLOT, (2 * i + 1) * SLOT);
            let t = &self.tick_mem;
            let put = |at: usize, words: &[u32]| {
                for (k, v) in words.iter().enumerate() {
                    unsafe { (t.ptr(at + k * 4) as *mut u32).write_volatile(*v) };
                }
            };
            put(xs, &[CMD_TRANSFER_TO_HOST_2D, 0, 0, 0, 0, 0, rect[0], rect[1], rect[2], rect[3], off as u32, (off >> 32) as u32, id, 0]);
            put(fs, &[CMD_RESOURCE_FLUSH, 0, 0, 0, 0, 0, rect[0], rect[1], rect[2], rect[3], id, 0]);
            let x = [Buf::out(t.addr(xs), HDR + 32), Buf::into(t.addr(xs + RESP_AT), HDR)];
            let f = [Buf::out(t.addr(fs), HDR + 24), Buf::into(t.addr(fs + RESP_AT), HDR)];
            if self.q.add(&x).is_none() {
                break;
            }
            self.pending += 1;
            if self.q.add(&f).is_some() {
                self.pending += 1;
            }
            crate::display::count_update(rect[2], rect[3]);
            done += 1;
        }
        if done > 0 {
            self.q.kick();
        }
        done
    }

    /// Queue a cursor command (UPDATE with a new image, or MOVE); false if
    /// the previous one is still on the queue.
    fn cursor_cmd(&mut self, cmd: u32, x: u32, y: u32, resource: u32, hot: (u32, u32)) -> bool {
        let Some(q) = self.cursorq.as_mut() else { return false };
        while q.pop().is_some() {
            self.cursor_busy = false;
        }
        if self.cursor_busy {
            return false;
        }
        // Header, cursor_pos (scanout, x, y, padding), resource, hot spot.
        let words = [cmd, 0, 0, 0, 0, 0, 0, x, y, 0, resource, hot.0, hot.1, 0];
        for (i, w) in words.iter().enumerate() {
            unsafe { (self.mem.ptr(CURSOR_REQ + i * 4) as *mut u32).write_volatile(*w) };
        }
        let b = [Buf::out(self.mem.addr(CURSOR_REQ), CURSOR_LEN)];
        let Some(q) = self.cursorq.as_mut() else { return false };
        if q.add(&b).is_none() {
            return false;
        }
        q.kick();
        self.cursor_busy = true;
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
    let (Some(q), Some(mem), Some(tick_mem)) = (dev.queue(0, QUEUE_SIZE), Dma::new(4096), Dma::new(4096)) else {
        crate::println!("virtio-gpu {}: cannot set up the control queue", dev.name());
        dev.fail();
        return;
    };
    // The cursor queue (for a hardware mouse pointer), set up before the
    // device goes live.
    let cursorq = dev.queue(1, 16);
    dev.ready();
    let scanouts = dev.cfg32(8);
    let mut g = Gpu {
        dev,
        q,
        mem,
        tick_mem,
        pending: 0,
        resource: 0,
        backing: None,
        backing_size: 0,
        width: 0,
        height: 0,
        next_id: 1,
        preferred: (0, 0),
        cursorq,
        cursor: 0,
        cursor_backing: None,
        cursor_sent: None,
        cursor_busy: false,
    };
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
/// the new framebuffer. Once PM has mapped the framebuffer, every mode
/// uses the backing PM mapped ([`pm_area`]).
pub fn set_mode(w: u32, h: u32) -> bool {
    let reuse = crate::display::pm_mapped();
    match GPU.lock().as_ref() {
        Some(g) if reuse && (w as usize * h as usize * 4 > g.backing_size || w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM) => return false,
        Some(_) => {}
        None => return false,
    }
    if !reuse && !fits(w, h) {
        return false;
    }
    switch(w, h, (!reuse).then(|| w as usize * h as usize * 4))
}

/// Show a `w` x `h` mode, with a new backing of `size` bytes (allocated
/// and zeroed without holding the spin lock) or the current one.
fn switch(w: u32, h: u32, size: Option<usize>) -> bool {
    let backing = match size {
        Some(n) => match Dma::new(n) {
            Some(b) => Some((b, n)),
            None => {
                crate::println!("virtio-gpu: no memory for a {w}x{h} framebuffer");
                return false;
            }
        },
        None => None,
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
        gpu.cursor_sent = None;
        gpu.flush_rects(&[(0, 0, w, h)]);
    }
    true
}

/// The memory PM maps: the backing, made large enough for every mode in
/// [`modes`] first (the current mode is shown again from a new one), so
/// that the mode can change under PM. None if there is not enough
/// memory for that (PM then maps the current framebuffer).
pub fn pm_area() -> Option<(u64, u64)> {
    let (w, h, have) = {
        let g = GPU.lock();
        let gpu = g.as_ref()?;
        (gpu.width, gpu.height, gpu.backing_size)
    };
    let need = modes().iter().chain([(w, h)].iter()).map(|&(a, b)| a as usize * b as usize * 4).max().unwrap_or(0);
    if need > have && !switch(w, h, Some(need)) {
        return None;
    }
    let g = GPU.lock();
    let gpu = g.as_ref()?;
    Some((gpu.backing.as_ref()?.addr(0), gpu.backing_size as u64))
}

/// The display's preferred size, asked now (GET_DISPLAY_INFO).
pub fn preferred() -> Option<(u32, u32)> {
    let mut g = GPU.lock();
    let gpu = g.as_mut()?;
    let p = gpu.display_info().first().copied()?;
    gpu.preferred = p;
    Some(p)
}

/// Timer tick: copy what changed to the host about 60 times a second;
/// about twice a second, look whether the host changed the display
/// configuration (VIRTIO_GPU_EVENT_DISPLAY: its window was resized).
pub fn tick(ticks: u64) {
    if ticks % 16 != 8 || !ACTIVE.load(Ordering::Relaxed) || crate::smp::this().index() != 0 {
        return;
    }
    let Some(mut g) = GPU.try_lock() else { return };
    let Some(gpu) = g.as_mut() else { return };
    if ticks % 512 == 8 && gpu.dev.cfg32(CFG_EVENTS_READ) & EVENT_DISPLAY != 0 {
        gpu.dev.cfg_write8(CFG_EVENTS_CLEAR, EVENT_DISPLAY as u8);
        crate::display::host_size_changed();
    }
    gpu.reap(None);
    if gpu.pending > 0 {
        // The previous flush is still on its way; try again next time.
        return;
    }
    let d = crate::display::take_damage(gpu.width, gpu.height);
    let done = gpu.flush_rects(d.rects());
    if done < d.n {
        crate::display::requeue_damage(&d.rects()[done..]);
    }
}

/// Define the hardware cursor: `pixels` (0xAARRGGBB, `w` x `h`, at most
/// 64x64) with the hot spot at (`hx`, `hy`) from the top left. False
/// without a virtio-gpu cursor queue.
pub fn define_cursor(w: u32, h: u32, hx: u32, hy: u32, pixels: &[u32]) -> bool {
    if w > CURSOR_DIM || h > CURSOR_DIM || pixels.len() < (w * h) as usize {
        return false;
    }
    let Some(backing) = Dma::new((CURSOR_DIM * CURSOR_DIM * 4) as usize) else { return false };
    for y in 0..h {
        for x in 0..w {
            let v = pixels[(y * w + x) as usize];
            unsafe { (backing.ptr(((y * CURSOR_DIM + x) * 4) as usize) as *mut u32).write_volatile(v) };
        }
    }
    let mut g = GPU.lock();
    let Some(gpu) = g.as_mut() else { return false };
    if gpu.cursorq.is_none() {
        return false;
    }
    let id = gpu.next_id;
    gpu.next_id += 1;
    let (addr, len) = (backing.addr(0), CURSOR_DIM * CURSOR_DIM * 4);
    if !gpu.ok(CMD_RESOURCE_CREATE_2D, &[id, FORMAT_B8G8R8A8, CURSOR_DIM, CURSOR_DIM])
        || !gpu.ok(CMD_RESOURCE_ATTACH_BACKING, &[id, 1, addr as u32, (addr >> 32) as u32, len, 0])
        || !gpu.ok(CMD_TRANSFER_TO_HOST_2D, &[0, 0, CURSOR_DIM, CURSOR_DIM, 0, 0, id, 0])
    {
        return false;
    }
    let old = core::mem::replace(&mut gpu.cursor, id);
    gpu.cursor_backing = Some(backing);
    gpu.cursor_sent = None;
    // Wait for room on the cursor queue for the new image.
    let mut sent = false;
    for _ in 0..SYNC_POLLS {
        if gpu.cursor_cmd(CMD_UPDATE_CURSOR, 0, 0, id, (hx, hy)) {
            sent = true;
            break;
        }
        gpu.dev.status_reg().read8();
    }
    if old != 0 {
        gpu.ok(CMD_RESOURCE_UNREF, &[old, 0]);
    }
    crate::println!("virtio-gpu: hardware cursor {w}x{h}");
    sent
}

/// Timer tick: put the cursor at (`x`, `y`) (pixels from the top left),
/// or hide it.
pub fn move_cursor(x: u32, y: u32, visible: bool) {
    let Some(mut g) = GPU.try_lock() else { return };
    let Some(gpu) = g.as_mut() else { return };
    if gpu.cursor == 0 || gpu.cursor_sent == Some((x, y, visible)) {
        return;
    }
    let id = if visible { gpu.cursor } else { 0 };
    if gpu.cursor_cmd(CMD_MOVE_CURSOR, x, y, id, (0, 0)) {
        gpu.cursor_sent = Some((x, y, visible));
    }
}
