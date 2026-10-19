//! Display and raw input services for rs2's Presentation Manager DLLs
//! (gate module RS2API): the framebuffer mapped into PM processes, and a
//! queue of key, mouse and repaint events for the PM session.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

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
static DISPLAY: Once<Arc<SharedMem>> = Once::new();
/// Pointer position in pixels (PM coordinates, origin bottom-left).
static PTR_X: AtomicI32 = AtomicI32::new(0);
static PTR_Y: AtomicI32 = AtomicI32::new(0);
static BUTTONS: AtomicU32 = AtomicU32::new(0);

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

/// The framebuffer as a shared object (created on first use), with its
/// geometry: (object, width, height, stride in pixels, red in low byte).
pub fn display() -> Option<(Arc<SharedMem>, u32, u32, u32, bool)> {
    if DISPLAY.get().is_none() {
        // PM starts: the VMware adapter's driver takes over (the FIFO is
        // needed for a hardware cursor). Done first: the framebuffer may
        // move.
        crate::drivers::svga::take_over();
    }
    let fb = crate::screen::framebuffer()?;
    if DISPLAY.get().is_none() {
        let bytes = fb.stride as u64 * fb.height as u64 * 4;
        let size = (bytes.div_ceil(PAGE as u64) * PAGE as u64) as u32;
        let base = vm::reserve_shared(size, None)?;
        let obj = SharedMem::new_device(base, size, vm::pag::READ | vm::pag::WRITE, fb.base & !(PAGE as u64 - 1));
        DISPLAY.set(obj);
        PTR_X.store(fb.width as i32 / 2, Ordering::Relaxed);
        PTR_Y.store(fb.height as i32 / 2, Ordering::Relaxed);
    }
    Some((DISPLAY.get()?.clone(), fb.width, fb.height, fb.stride, fb.format == 0))
}

/// Change the screen resolution (`video=`, a display settings dialog);
/// needs a display driver (virtio-gpu, VMware SVGA II) and is refused
/// once PM has mapped the framebuffer.
pub fn set_mode(width: u32, height: u32) -> bool {
    use crate::drivers::{svga, virtio::gpu};
    DISPLAY.get().is_none()
        && if gpu::present() { gpu::set_mode(width, height) } else { svga::set_mode(width, height) }
}

/// Resolutions [`set_mode`] can switch to.
pub fn modes() -> alloc::vec::Vec<(u32, u32)> {
    use crate::drivers::{svga, virtio::gpu};
    if gpu::present() { gpu::modes() } else { svga::modes() }
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
