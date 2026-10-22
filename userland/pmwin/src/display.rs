//! The screen under PM: telling the kernel what was drawn (damage), and
//! display mode changes while PM runs.
//!
//! Everything drawn on the screen surface is noted in `State::damage`
//! (gpi's surface does it); when a thread lets go of the PM lock the list
//! goes to the kernel (`Rs2UpdateDisplay`), whose display driver copies
//! only those areas to the display.
//!
//! When the mode changes (the Display settings program calls
//! `Rs2SetDisplayMode`; the host resizes a virtio-gpu display) the kernel
//! sends EV_MODE: the server takes the new geometry (`Rs2QueryDisplay`),
//! sizes the desktop window, gives the shell's desktop window (the pinned
//! one) and maximized frames the new screen, brings frames that ended up
//! off the screen back, tells the desktop window (WM_SIZE) and repaints
//! everything.

use gpi::{Point, Surface};
use pmwin::{ws, HWND_DESKTOP};

use crate::state::{get, st};
use crate::win::WinSetWindowPos;

#[repr(C)]
#[derive(Default)]
pub struct DisplayInfo {
    pub address: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub flags: u32,
    /// Mode changes so far (Rs2QueryDisplay).
    pub generation: u32,
}

/// The mode generation PM has laid itself out for.
static GENERATION: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

unsafe extern "C" {
    #[link_name = "RS2API.20"]
    fn Rs2UpdateDisplay(rects: *const u32, n: u32) -> u32;
    #[link_name = "RS2API.21"]
    fn Rs2QueryDisplay(info: *mut DisplayInfo) -> u32;
    #[link_name = "RS2API.23"]
    fn Rs2SetDisplayMode(cx: u32, cy: u32) -> u32;
}

/// Tell the kernel what was drawn on the screen (when the PM lock is
/// let go). Drawing into the shadow buffer is not reported (everything
/// is repainted when PM gets the screen back).
pub fn flush_damage() {
    let Some(s) = get() else { return };
    if s.damage.is_empty() {
        return;
    }
    if s.surface.pixels() as usize == s.screen {
        let h = s.surface.height;
        let mut v = [0u32; 4 * gpi::damage::MAX];
        let rects = s.damage.rects();
        for (i, r) in rects.iter().enumerate() {
            // Framebuffer pixels, rows from the top.
            v[i * 4..i * 4 + 4].copy_from_slice(&[r.left as u32, (h - r.top) as u32, r.right as u32, (h - r.bottom) as u32]);
        }
        unsafe { Rs2UpdateDisplay(v.as_ptr(), rects.len() as u32) };
    }
    s.damage.clear();
}

/// EV_HOSTSIZE: the host wants another size; the kernel picks the
/// display's preferred one (EV_MODE follows).
pub fn host_size() {
    unsafe { Rs2SetDisplayMode(0, 0) };
}

/// The input thread, with the lock, now and then: catch a mode change
/// whose EV_MODE was missed.
pub fn check_mode() {
    let mut info = DisplayInfo::default();
    if unsafe { Rs2QueryDisplay(&mut info) } == 0 && info.generation != GENERATION.load(core::sync::atomic::Ordering::Relaxed) {
        apply(info);
    }
}

/// EV_MODE (the input thread, with the lock): the screen has a new size.
pub fn mode_changed() {
    let mut info = DisplayInfo::default();
    if unsafe { Rs2QueryDisplay(&mut info) } == 0 {
        apply(info);
    }
}

fn apply(info: DisplayInfo) {
    GENERATION.store(info.generation, core::sync::atomic::Ordering::Relaxed);
    let s = st();
    let (ow, oh) = (s.surface.width, s.surface.height);
    let (w, h) = (info.width as i32, info.height as i32);
    let shadowed = s.surface.pixels() as usize != s.screen;
    s.screen = info.address as usize;
    let mut surface = unsafe { Surface::new(info.address as *mut u32, w, h, info.stride as i32, info.flags & 1 != 0) };
    unsafe { surface.set_damage(&mut s.damage) };
    s.shadow = alloc::vec::Vec::new();
    s.surface = surface;
    s.damage.clear();
    s.ptr_saved = None;
    if shadowed {
        // A text session has the screen: keep drawing into memory.
        crate::state::to_shadow();
    }
    s.pointer = Point::new(s.pointer.x.min(w - 1), s.pointer.y.min(h - 1));
    s.desk.set_pos(HWND_DESKTOP, 0, 0, w, h);
    crate::trace(format_args!("display: {ow}x{oh} -> {w}x{h}"));
    // Top-level windows: the shell's desktop and maximized frames cover
    // the new screen; others are kept (partly) on it.
    let tops: alloc::vec::Vec<u32> = s.desk.get(HWND_DESKTOP).map_or(alloc::vec::Vec::new(), |d| d.children.clone());
    for t in tops {
        let Some(win) = st().desk.get(t) else { continue };
        let (x, y, cx, cy, style) = (win.x, win.y, win.cx, win.cy, win.style);
        if t == st().desk.pinned || (x, y, cx, cy) == (0, 0, ow, oh) {
            WinSetWindowPos(t, 0, 0, 0, w, h, 0x0003);
        } else if style & ws::MAXIMIZED != 0 && st().frames.contains_key(&t) {
            let b = crate::frame::border(crate::frame::fcf(t));
            WinSetWindowPos(t, 0, -b, -b, w + 2 * b, h + 2 * b, 0x0003);
        } else if style & ws::MINIMIZED == 0 {
            // At least 64 pixels of it on the screen, its top edge (the
            // title bar) below the top of the screen.
            let nx = x.min(w - 64).max(64 - cx);
            let ny = (y + cy).min(h) - cy;
            let ny = ny.max(64 - cy);
            if (nx, ny) != (x, y) {
                WinSetWindowPos(t, 0, nx, ny, 0, 0, 0x0002);
            }
        } else if x >= w || y >= h {
            WinSetWindowPos(t, 0, x.min(w - cx.min(w)), y.min(h - cy.min(h)), 0, 0, 0x0002);
        }
    }
    let pack = |a: i32, b: i32| (a as u32 & 0xffff) | (b as u32) << 16;
    crate::msg::post(HWND_DESKTOP, 0x0007, pack(ow, oh), pack(w, h));
    let all = gpi::Rect::new(0, 0, w, h);
    st().desk.invalidate_screen(all);
    for win in st().desk.all_mut() {
        win.ghost = pmwin::Region::new();
    }
    crate::pointer::draw(st());
}
