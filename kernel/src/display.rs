//! Display and raw input services for rs2's Presentation Manager DLLs
//! (gate module RS2API): the framebuffer mapped into PM processes, and a
//! queue of key, mouse and repaint events for the PM session.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::sync::{Once, SpinLock};
use crate::task::WaitQueue;
use crate::vm::{self, SharedMem, PAGE};

pub const EV_KEY: u16 = 1;
pub const EV_MOUSE: u16 = 2;
pub const EV_REPAINT: u16 = 3;

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
            q.pop_front();
        }
        q.push_back(e);
    }
    WAITERS.wake_all();
}

pub fn post_repaint() {
    push(Input { kind: EV_REPAINT, time: crate::irq::uptime_ms() as u32, ..Default::default() });
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
    let Some(fb) = crate::screen::framebuffer() else { return };
    let x = (PTR_X.load(Ordering::Relaxed) + dx).clamp(0, fb.width as i32 - 1);
    let y = (PTR_Y.load(Ordering::Relaxed) - dy).clamp(0, fb.height as i32 - 1);
    let moved = x != PTR_X.load(Ordering::Relaxed) || y != PTR_Y.load(Ordering::Relaxed);
    let changed = BUTTONS.swap(buttons as u32, Ordering::Relaxed) != buttons as u32;
    PTR_X.store(x, Ordering::Relaxed);
    PTR_Y.store(y, Ordering::Relaxed);
    if moved || changed {
        push(Input { kind: EV_MOUSE, flags: buttons as u16, a: x as u32, b: y as u32, time: crate::irq::uptime_ms() as u32 });
    }
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

pub fn pointer() -> (i32, i32) {
    (PTR_X.load(Ordering::Relaxed), PTR_Y.load(Ordering::Relaxed))
}
