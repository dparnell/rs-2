//! Display and raw input services for rs2's Presentation Manager DLLs
//! (gate module RS2API): the framebuffer mapped into PM processes, and a
//! queue of key, mouse and repaint events for the PM session.
//!
//! What changed on the screen is kept as a short list of damaged
//! rectangles ([`damage`], [`take_damage`]): the text screens add the
//! rows they draw, PM (PMWIN, when it lets go of its lock) the areas it
//! drew (`Rs2UpdateDisplay`). Display drivers that copy changes to the
//! display (VMware SVGA II's UPDATE, virtio-gpu's TRANSFER_TO_HOST_2D and
//! RESOURCE_FLUSH) take the list on the timer tick and copy only those
//! areas.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use crate::sync::{Once, SpinLock};
use crate::task::WaitQueue;
use crate::vm::{self, SharedMem, PAGE};

pub const EV_KEY: u16 = 1;
pub const EV_MOUSE: u16 = 2;
pub const EV_REPAINT: u16 = 3;
/// A process that had mapped the display ended (`a` = its pid): the PM
/// server cleans up its windows.
pub const EV_PROCEXIT: u16 = 4;
/// A text session (or a pop-up) took the screen from PM: PM draws into
/// memory until the next EV_REPAINT.
pub const EV_BACKGROUND: u16 = 5;
/// The display mode changed (`a` x `b` pixels): PM takes the new geometry
/// (Rs2QueryDisplay) and lays the desktop out again.
pub const EV_MODE: u16 = 6;
/// The host asks for another display size (virtio-gpu's display
/// configuration changed): PM calls Rs2SetDisplayMode(0, 0).
pub const EV_HOSTSIZE: u16 = 7;

/// One raw input event (INPUTEVENT, 16 bytes).
#[derive(Clone, Copy, Default)]
pub struct Input {
    pub kind: u16,
    /// Keys: shift state. Mouse: buttons (bit 0 left, 1 right, 2 middle).
    pub flags: u16,
    /// Keys: character | scan << 8 | status << 16 | key-up << 24.
    /// Mouse: x in pixels.
    pub a: u32,
    /// Mouse: y in pixels, from the bottom.
    pub b: u32,
    pub time: u32,
}

const QUEUE_MAX: usize = 256;

static QUEUE: SpinLock<VecDeque<Input>> = SpinLock::new(VecDeque::new());
/// A hardware cursor is defined and PM wants it shown.
static CURSOR: AtomicBool = AtomicBool::new(false);
static CURSOR_DEFINED: AtomicBool = AtomicBool::new(false);
/// Processes that mapped the display (PM programs).
static PM_PIDS: SpinLock<alloc::vec::Vec<u32>> = SpinLock::new(alloc::vec::Vec::new());
static WAITERS: WaitQueue = WaitQueue::new();
static DISPLAY: Once<Mapped> = Once::new();

/// The memory PM processes draw into: the display's framebuffer (VMware
/// SVGA II's VRAM, virtio-gpu's backing), large enough for every mode the
/// driver can switch to, or, on a plain firmware framebuffer, a back
/// buffer in RAM that the timer tick copies to the screen.
struct Mapped {
    obj: Arc<SharedMem>,
    /// Physical address of its first byte, and its size.
    phys: u64,
    size: u64,
    /// A back buffer (else the framebuffer itself).
    back: bool,
}

/// Display mode changes under PM so far (`Rs2QueryDisplay` reports it,
/// so that PM notices a change even if it missed the EV_MODE event).
static MODE_GEN: AtomicU32 = AtomicU32::new(0);

pub fn mode_generation() -> u32 {
    MODE_GEN.load(Ordering::Relaxed)
}

/// What `Rs2MapDisplay` and `Rs2QueryDisplay` report.
pub struct Info {
    pub obj: Arc<SharedMem>,
    /// Offset of the first pixel in the object.
    pub offset: u32,
    pub width: u32,
    pub height: u32,
    /// Pixels per row.
    pub stride: u32,
    /// Red in the low byte.
    pub red_low: bool,
}
/// Pointer position in pixels (PM coordinates, origin bottom-left).
static PTR_X: AtomicI32 = AtomicI32::new(0);
static PTR_Y: AtomicI32 = AtomicI32::new(0);
static BUTTONS: AtomicU32 = AtomicU32::new(0);
/// Pixels and update commands sent to the display (`dispstat` on the
/// kernel command line logs them every 5 s).
static STAT_PX: AtomicU64 = AtomicU64::new(0);
static STAT_CMDS: AtomicU64 = AtomicU64::new(0);
/// 0: `dispstat` not looked for yet, 1: off, 2: on.
static STAT_ON: AtomicU32 = AtomicU32::new(0);
static STAT_LAST: AtomicU64 = AtomicU64::new(0);

/// Damaged rectangles kept at most (more are merged).
pub const DAMAGE_MAX: usize = 16;
/// Area (pixels) a merge may add and still be taken when the list is not
/// full.
const DAMAGE_SLACK: u64 = 64 * 64;

/// Damaged screen areas: framebuffer pixels, rows from the top, as
/// (left, top, right, bottom), right and bottom exclusive.
#[derive(Clone, Copy)]
pub struct Damage {
    pub rects: [(u32, u32, u32, u32); DAMAGE_MAX],
    pub n: usize,
}

type R = (u32, u32, u32, u32);

fn r_area(r: &R) -> u64 {
    (r.2 - r.0) as u64 * (r.3 - r.1) as u64
}

fn r_union(a: &R, b: &R) -> R {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

fn r_covers(a: &R, b: &R) -> bool {
    a.0 <= b.0 && a.1 <= b.1 && a.2 >= b.2 && a.3 >= b.3
}

/// Area a merge of `a` and `b` adds beyond them.
fn r_waste(a: &R, b: &R) -> u64 {
    let i = (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3));
    let overlap = if i.0 < i.2 && i.1 < i.3 { r_area(&i) } else { 0 };
    (r_area(&r_union(a, b)) + overlap).saturating_sub(r_area(a) + r_area(b))
}

impl Damage {
    pub const fn new() -> Damage {
        Damage { rects: [(0, 0, 0, 0); DAMAGE_MAX], n: 0 }
    }

    pub fn rects(&self) -> &[R] {
        &self.rects[..self.n]
    }

    /// Add `r` (as gpi's damage list does: join the rectangle it adds
    /// least area to when that is little or the list is full).
    pub fn add(&mut self, r: R) {
        if r.0 >= r.2 || r.1 >= r.3 || self.rects().iter().any(|e| r_covers(e, &r)) {
            return;
        }
        let best = self.rects().iter().enumerate().map(|(i, e)| (i, r_waste(e, &r))).min_by_key(|&(_, w)| w);
        match best {
            Some((i, w)) if w <= DAMAGE_SLACK || self.n == DAMAGE_MAX => {
                self.rects[i] = r_union(&self.rects[i], &r);
                // The grown rectangle swallows those it now covers or
                // nearly meets.
                let mut i = i;
                let mut j = 0;
                while j < self.n {
                    if j != i && (r_covers(&self.rects[i], &self.rects[j]) || r_waste(&self.rects[i], &self.rects[j]) <= DAMAGE_SLACK) {
                        self.rects[i] = r_union(&self.rects[i], &self.rects[j]);
                        self.n -= 1;
                        self.rects[j] = self.rects[self.n];
                        if i == self.n {
                            i = j;
                        }
                        j = 0;
                    } else {
                        j += 1;
                    }
                }
            }
            _ => {
                self.rects[self.n] = r;
                self.n += 1;
            }
        }
    }

    /// Clip every rectangle to a `w` x `h` screen.
    fn clip(&mut self, w: u32, h: u32) {
        let mut k = 0;
        for i in 0..self.n {
            let r = self.rects[i];
            let c = (r.0.min(w), r.1.min(h), r.2.min(w), r.3.min(h));
            if c.0 < c.2 && c.1 < c.3 {
                self.rects[k] = c;
                k += 1;
            }
        }
        self.n = k;
    }
}

static DAMAGE: SpinLock<Damage> = SpinLock::new(Damage::new());

/// Note that the screen changed in (`x0`, `y0`)..(`x1`, `y1`) (pixels,
/// rows from the top; clipped to the screen when taken).
pub fn damage(x0: u32, y0: u32, x1: u32, y1: u32) {
    DAMAGE.lock().add((x0, y0, x1, y1));
}

/// The screen areas changed since the last call, clipped to `w` x `h`.
pub fn take_damage(w: u32, h: u32) -> Damage {
    let mut d = core::mem::replace(&mut *DAMAGE.lock(), Damage::new());
    d.clip(w, h);
    d
}

/// Put back what a driver could not copy this time.
pub fn requeue_damage(rects: &[R]) {
    let mut g = DAMAGE.lock();
    for &r in rects {
        g.add(r);
    }
}

/// A driver copied a `w` x `h` area to the display.
pub fn count_update(w: u32, h: u32) {
    STAT_PX.fetch_add(w as u64 * h as u64, Ordering::Relaxed);
    STAT_CMDS.fetch_add(1, Ordering::Relaxed);
}

/// `dispstat`: every 5 s, the pixels and updates per second sent.
fn log_stats() {
    let on = match STAT_ON.load(Ordering::Relaxed) {
        0 => {
            let on = crate::boot_info().cmdline().split_whitespace().any(|w| w.eq_ignore_ascii_case("dispstat"));
            STAT_ON.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        v => v == 2,
    };
    let now = crate::irq::uptime_ms();
    if !on || now < STAT_LAST.load(Ordering::Relaxed) + 5000 {
        return;
    }
    let ms = now - STAT_LAST.swap(now, Ordering::Relaxed);
    let px = STAT_PX.swap(0, Ordering::Relaxed);
    let cmds = STAT_CMDS.swap(0, Ordering::Relaxed);
    crate::println!("dispstat: {} px/s, {} updates/s", px * 1000 / ms.max(1), cmds * 1000 / ms.max(1));
}

fn push(e: Input) {
    {
        let mut q = QUEUE.lock();
        if q.len() >= QUEUE_MAX {
            // Drop the oldest input event (never a process exit).
            match q.iter().position(|e| e.kind != EV_PROCEXIT) {
                Some(i) => {
                    q.remove(i);
                }
                None => {
                    q.pop_front();
                }
            }
        }
        q.push_back(e);
    }
    WAITERS.wake_all();
}

pub fn post_background() {
    push(Input { kind: EV_BACKGROUND, time: crate::irq::uptime_ms() as u32, ..Default::default() });
}

pub fn post_repaint() {
    push(Input { kind: EV_REPAINT, time: crate::irq::uptime_ms() as u32, ..Default::default() });
}

/// A process mapped the display: it is a PM program.
pub fn add_pm(pid: u32) {
    let mut v = PM_PIDS.lock();
    if !v.contains(&pid) {
        v.push(pid);
    }
}

pub fn is_pm(pid: u32) -> bool {
    PM_PIDS.lock().contains(&pid)
}

/// A process ended: if it was a PM program, tell the PM server.
pub fn process_ended(pid: u32) {
    let was = {
        let mut v = PM_PIDS.lock();
        let n = v.len();
        v.retain(|&p| p != pid);
        v.len() != n
    };
    if was {
        push(Input { kind: EV_PROCEXIT, a: pid, time: crate::irq::uptime_ms() as u32, ..Default::default() });
    }
}

pub fn reset_input() {
    QUEUE.lock().clear();
}

/// A key event while the PM session is in the foreground.
pub fn key(k: &crate::kbd::KeyInfo, up: bool) {
    let a = k.ch as u32 | (k.scan as u32) << 8 | (k.status as u32) << 16 | (up as u32) << 24;
    push(Input { kind: EV_KEY, flags: k.shift, a, b: 0, time: k.time });
}

/// Mouse motion while the PM session is in the foreground (y grows
/// downwards, as devices report it).
pub fn mouse(buttons: u8, dx: i32, dy: i32) {
    let x = PTR_X.load(Ordering::Relaxed) + dx;
    let y = PTR_Y.load(Ordering::Relaxed) - dy;
    set_pointer(buttons, x, y);
}

/// An absolute pointer position (tablets, hypervisor pointers) while the
/// PM session is in the foreground: `x` in 0..=`max_x` from the left, `y`
/// in 0..=`max_y` from the top, scaled to the screen.
pub fn mouse_abs(buttons: u8, x: u32, y: u32, max_x: u32, max_y: u32) {
    let Some(fb) = crate::screen::framebuffer() else { return };
    let px = scale(x, max_x, fb.width);
    let py = fb.height as i32 - 1 - scale(y, max_y, fb.height);
    set_pointer(buttons, px, py);
}

/// `v` of 0..=`max` as one of `n` positions.
fn scale(v: u32, max: u32, n: u32) -> i32 {
    (v.min(max) as u64 * n.saturating_sub(1) as u64 / max.max(1) as u64) as i32
}

/// Move the pointer to `x`, `y` (PM coordinates, clamped to the screen)
/// and queue a mouse event if it moved or the buttons changed.
fn set_pointer(buttons: u8, x: i32, y: i32) {
    let Some(fb) = crate::screen::framebuffer() else { return };
    let x = x.clamp(0, fb.width as i32 - 1);
    let y = y.clamp(0, fb.height as i32 - 1);
    let moved = x != PTR_X.load(Ordering::Relaxed) || y != PTR_Y.load(Ordering::Relaxed);
    let changed = BUTTONS.swap(buttons as u32, Ordering::Relaxed) != buttons as u32;
    PTR_X.store(x, Ordering::Relaxed);
    PTR_Y.store(y, Ordering::Relaxed);
    if moved || changed {
        // flags: the buttons, and the keyboard's shift state (Shift,
        // Ctrl, Alt: bits 0-3 as in key events) in the high byte.
        let shift = (crate::kbd::shift_state_now() & 0x0f) << 8;
        push(Input { kind: EV_MOUSE, flags: buttons as u16 | shift, a: x as u32, b: y as u32, time: crate::irq::uptime_ms() as u32 });
    }
}

/// Framebuffer pixel (y down) of an absolute position.
pub fn abs_to_pixels(fb: &bootinfo::Framebuffer, x: u32, y: u32, max_x: u32, max_y: u32) -> (i32, i32) {
    let scale = |v: u32, max: u32, size: u32| (v.min(max) as u64 * (size as u64).saturating_sub(1) / max.max(1) as u64) as i32;
    (scale(x, max_x, fb.width), scale(y, max_y, fb.height))
}

/// Next input event, waiting up to `ms` (u32::MAX = forever).
pub fn read(ms: u32) -> Option<Input> {
    let deadline = (ms != u32::MAX).then(|| crate::irq::ticks() + ms as u64 * crate::irq::HZ / 1000);
    loop {
        if let Some(e) = QUEUE.lock().pop_front() {
            return Some(e);
        }
        if ms == 0 || !WAITERS.wait_until(deadline, || !QUEUE.lock().is_empty()) {
            return None;
        }
    }
}

/// The display as a shared object for PM (created on first use), with
/// its geometry.
pub fn display() -> Option<Info> {
    if DISPLAY.get().is_none() {
        // The memory PM draws into: what the display driver offers
        // (virtio-gpu, VMware SVGA II, whose driver takes the adapter
        // over now: the framebuffer may move), else a back buffer.
        use crate::drivers::{svga, virtio::gpu};
        let area = if gpu::present() { gpu::pm_area() } else { svga::pm_area() };
        let fb = crate::screen::framebuffer()?;
        let (phys, size, back) = match area {
            Some((p, n)) => (p, n, false),
            // virtio-gpu without room for more: its current framebuffer.
            None if gpu::present() => (fb.base & !(PAGE as u64 - 1), (fb.base & (PAGE as u64 - 1)) + fb.stride as u64 * fb.height as u64 * 4, false),
            None => {
                let n = (fb.stride as u64 * fb.height as u64 * 4).next_multiple_of(PAGE as u64);
                match crate::mm::phys::alloc_contig(n / PAGE as u64, u64::MAX) {
                    Some(p) => (p, n, true),
                    None => (fb.base & !(PAGE as u64 - 1), (fb.base & (PAGE as u64 - 1)) + fb.stride as u64 * fb.height as u64 * 4, false),
                }
            }
        };
        let size = size.next_multiple_of(PAGE as u64);
        let base = vm::reserve_shared(size as u32, None)?;
        let obj = SharedMem::new_device(base, size as u32, vm::pag::READ | vm::pag::WRITE, phys);
        DISPLAY.set(Mapped { obj, phys, size, back });
        if back {
            crate::println!("display: PM draws into a back buffer ({} KB)", size / 1024);
        }
        PTR_X.store(fb.width as i32 / 2, Ordering::Relaxed);
        PTR_Y.store(fb.height as i32 / 2, Ordering::Relaxed);
    }
    query()
}

/// The display's geometry for PM (once it is mapped).
pub fn query() -> Option<Info> {
    let m = DISPLAY.get()?;
    let fb = crate::screen::framebuffer()?;
    let offset = if m.back { 0 } else { fb.base.checked_sub(m.phys)? };
    if offset + fb.stride as u64 * fb.height as u64 * 4 > m.size {
        return None;
    }
    Some(Info { obj: m.obj.clone(), offset: offset as u32, width: fb.width, height: fb.height, stride: fb.stride, red_low: fb.format == 0 })
}

/// Whether PM has mapped the display.
pub fn pm_mapped() -> bool {
    DISPLAY.get().is_some()
}

/// Change the screen resolution (`video=`, `VIDEO=` in CONFIG.SYS, the
/// display settings dialog); needs a display driver (virtio-gpu, VMware
/// SVGA II). Under PM the new framebuffer must fit in what PM mapped;
/// PM is told (EV_MODE) and lays its desktop out again. (0, 0) is the
/// display's preferred size (virtio-gpu).
pub fn set_mode(width: u32, height: u32) -> bool {
    use crate::drivers::{svga, virtio::gpu};
    let (width, height) = match (width, height) {
        (0, 0) if gpu::present() => match gpu::preferred() {
            Some(p) => p,
            None => return false,
        },
        m => m,
    };
    let Some(old) = crate::screen::framebuffer() else { return false };
    if (old.width, old.height) == (width, height) {
        return true;
    }
    let set = |w, h| if gpu::present() { gpu::set_mode(w, h) } else { svga::set_mode(w, h) };
    if !set(width, height) {
        return false;
    }
    if DISPLAY.get().is_some() {
        if query().is_none() {
            crate::println!("display: {width}x{height} does not fit in PM's mapping");
            set(old.width, old.height);
            return false;
        }
        // Keep the pointer on the screen; PM repaints everything.
        PTR_X.store(PTR_X.load(Ordering::Relaxed).min(width as i32 - 1), Ordering::Relaxed);
        PTR_Y.store(PTR_Y.load(Ordering::Relaxed).min(height as i32 - 1), Ordering::Relaxed);
        damage(0, 0, width, height);
        MODE_GEN.fetch_add(1, Ordering::Relaxed);
        push(Input { kind: EV_MODE, a: width, b: height, time: crate::irq::uptime_ms() as u32, ..Default::default() });
    }
    true
}

/// The host asks for another size (virtio-gpu): PM picks it up.
pub fn host_size_changed() {
    if DISPLAY.get().is_some() {
        push(Input { kind: EV_HOSTSIZE, time: crate::irq::uptime_ms() as u32, ..Default::default() });
    }
}

/// Resolutions [`set_mode`] can switch to (only those that fit in PM's
/// mapping once PM runs).
pub fn modes() -> alloc::vec::Vec<(u32, u32)> {
    use crate::drivers::{svga, virtio::gpu};
    let mut v = if gpu::present() { gpu::modes() } else { svga::modes() };
    if let Some(m) = DISPLAY.get() {
        if m.back {
            v.clear();
        }
        v.retain(|&(w, h)| w as u64 * h as u64 * 4 <= m.size);
    }
    v
}

/// Timer tick (boot processor, about 60 times a second): with a back
/// buffer, copy what PM drew to the screen while PM is shown.
fn copy_back(ticks: u64) {
    let Some(m) = DISPLAY.get().filter(|m| m.back) else { return };
    if ticks % 16 != 8 {
        return;
    }
    let Some(fb) = crate::screen::framebuffer() else { return };
    let d = take_damage(fb.width, fb.height);
    let pm = crate::session::pm_session();
    if pm == 0 || crate::session::foreground() != pm || crate::session::popup_active() {
        return;
    }
    let src = crate::mm::phys::phys_to_virt(m.phys) as *const u32;
    let dst = (bootinfo::PHYS_MAP_BASE + fb.base) as *mut u32;
    for &(x0, y0, x1, y1) in d.rects() {
        for y in y0..y1 {
            let at = (y * fb.stride + x0) as usize;
            unsafe { core::ptr::copy_nonoverlapping(src.add(at), dst.add(at), (x1 - x0) as usize) };
        }
        count_update(x1 - x0, y1 - y0);
    }
}

pub fn pointer() -> (i32, i32) {
    (PTR_X.load(Ordering::Relaxed), PTR_Y.load(Ordering::Relaxed))
}

/// Define the mouse pointer as a hardware cursor (virtio-gpu, VMware SVGA
/// II): `pixels` 0xAARRGGBB, `w` x `h`, hot spot (`hx`, `hy`) from the
/// top left. False if there is no hardware cursor (PM draws the pointer
/// itself then).
pub fn define_cursor(w: u32, h: u32, hx: u32, hy: u32, pixels: &[u32]) -> bool {
    use crate::drivers::{svga, virtio::gpu};
    let ok = if gpu::present() { gpu::define_cursor(w, h, hx, hy, pixels) } else { svga::define_cursor(w, h, hx, hy, pixels) };
    if ok {
        CURSOR_DEFINED.store(true, Ordering::Relaxed);
        CURSOR.store(true, Ordering::Relaxed);
    }
    ok
}

/// Show or hide the hardware cursor.
pub fn show_cursor(on: bool) {
    CURSOR.store(on, Ordering::Relaxed);
}

/// Timer tick (boot processor): the hardware cursor follows the pointer,
/// shown while the PM session is in the foreground. The kernel moves it
/// itself, so it never waits for a PM process.
pub fn tick() {
    if crate::smp::this().index() == 0 {
        log_stats();
        copy_back(crate::irq::ticks());
    }
    if !CURSOR_DEFINED.load(Ordering::Relaxed) || crate::smp::this().index() != 0 {
        return;
    }
    let Some(fb) = crate::screen::framebuffer() else { return };
    let pm = crate::session::pm_session();
    let visible = CURSOR.load(Ordering::Relaxed) && pm != 0 && crate::session::foreground() == pm && !crate::session::popup_active();
    let x = PTR_X.load(Ordering::Relaxed).clamp(0, fb.width as i32 - 1) as u32;
    let y = (fb.height as i32 - 1 - PTR_Y.load(Ordering::Relaxed)).clamp(0, fb.height as i32 - 1) as u32;
    crate::drivers::virtio::gpu::move_cursor(x, y, visible);
    crate::drivers::svga::move_cursor(x, y, visible);
}

/// The PM session ended: no more hardware cursor until PM defines one.
pub fn reset_cursor() {
    CURSOR.store(false, Ordering::Relaxed);
    tick();
    CURSOR_DEFINED.store(false, Ordering::Relaxed);
}
