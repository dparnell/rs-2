//! Pointers and icons (HPOINTER): loaded from RT_POINTER resources and
//! files, made from bitmaps, rs2's own system pointers (SPTR_*), drawn
//! with WinDrawPointer; and the mouse pointer itself: a hardware cursor
//! when the display has one, otherwise drawn in software with a
//! save-under (drawing code hides it first so it never ends up in saved
//! pixels).
//!
//! Pointer objects live in the shared state (so the PM server can draw
//! any process's icons, e.g. for a minimized frame of a hung program);
//! a process's own pointers go when it ends. System pointers belong to
//! no process and cannot be destroyed.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::icon::{self, Image};
use gpi::{Point, Rect};
use pmwin::sysptr;

use crate::gpiapi;
use crate::state::{pid, st, State};

/// A pointer or icon.
pub struct PtrObj {
    /// Owning process (0: a system pointer).
    pub pid: u32,
    /// Hot spot of `img`, from its bottom-left corner.
    pub hot: Point,
    /// A pointer rather than an icon.
    pub pointer: bool,
    pub img: Image,
    pub mini: Image,
    /// Monochrome (as created): POINTERINFO has no colour bitmap.
    pub mono: bool,
}

/// Handles are `BASE + index`.
const BASE: u32 = 0x2000_0001;

/// Size of icons and pointers on this display (SV_CXICON).
pub const ICON: i32 = 32;
/// Size of mini icons.
pub const MINI: i32 = 16;

unsafe extern "C" {
    #[link_name = "RS2API.5"]
    fn Rs2SetPointerShape(cx: u32, cy: u32, hx: u32, hy: u32, pixels: *const u32) -> u32;
    #[link_name = "RS2API.6"]
    fn Rs2ShowPointer(show: u32) -> u32;
}

pub fn get(h: u32) -> Option<&'static PtrObj> {
    st().ptrs.get(h.wrapping_sub(BASE) as usize)?.as_ref()
}

fn add(p: PtrObj) -> u32 {
    let s = st();
    let i = match s.ptrs.iter().position(|x| x.is_none()) {
        Some(i) => {
            s.ptrs[i] = Some(p);
            i
        }
        None => {
            s.ptrs.push(Some(p));
            s.ptrs.len() - 1
        }
    };
    BASE + i as u32
}

/// System pointer `id`, created on first use.
pub fn system(id: u32) -> u32 {
    if id == 0 || id > sysptr::SPTR_CPTR {
        return 0;
    }
    let s = st();
    if s.sys_ptrs.len() <= id as usize {
        s.sys_ptrs.resize(id as usize + 1, 0);
    }
    if s.sys_ptrs[id as usize] == 0 {
        let Some(p) = sysptr::system(id) else { return 0 };
        let h = add(PtrObj { pid: 0, hot: p.hot, pointer: p.pointer, img: p.image, mini: p.mini, mono: false });
        st().sys_ptrs[id as usize] = h;
    }
    st().sys_ptrs[id as usize]
}

/// A pointer from a pointer/icon file or RT_POINTER resource.
pub fn from_data(b: &[u8], owner: u32) -> u32 {
    let entries = icon::parse(b);
    let screen = {
        let s = st();
        (s.surface.width, s.surface.height)
    };
    let Some(big) = icon::choose(&entries, ICON, screen) else { return 0 };
    let mini = icon::choose(&entries, MINI, screen).filter(|e| e.image.w <= ICON).unwrap_or(big);
    let scale = |e: &icon::Entry, n: i32| e.image.scaled(n, n);
    let k = |v: i32, from: i32| if from > 0 { v * ICON / from } else { v };
    let hot = Point::new(k(big.hot.x, big.image.w), k(big.hot.y, big.image.h));
    let p = PtrObj {
        pid: owner,
        hot,
        pointer: big.is_pointer(),
        img: scale(big, ICON),
        mini: scale(mini, MINI),
        mono: big.bits == 1,
    };
    add(p)
}

/// Pointers of a process that ended (or let go of PM).
pub fn process_gone(p: u32) {
    let s = st();
    for slot in s.ptrs.iter_mut() {
        if slot.as_ref().is_some_and(|x| x.pid == p) {
            *slot = None;
        }
    }
}

// ---- The API ----

/// WinLoadPointer(hwndDesktop, hmod, idres): a pointer or icon from an
/// RT_POINTER resource.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadPointer(_desk: u32, hmod: u32, id: u32) -> u32 {
    const RT_POINTER: u32 = 1;
    match crate::res::load(hmod, RT_POINTER, id) {
        Some(b) => from_data(b, pid()),
        None => 0,
    }
}

/// WinQuerySysPointer(hwndDesktop, iptr, fLoad): a handle of system
/// pointer `iptr` (with fLoad a copy the caller may destroy).
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysPointer(_desk: u32, id: i32, copy: u32) -> u32 {
    let h = system(id as u32);
    if h == 0 || copy == 0 {
        return h;
    }
    let Some(p) = get(h) else { return 0 };
    let c = PtrObj { pid: pid(), hot: p.hot, pointer: p.pointer, img: p.img.clone(), mini: p.mini.clone(), mono: p.mono };
    add(c)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyPointer(h: u32) -> u32 {
    let s = st();
    let i = h.wrapping_sub(BASE) as usize;
    match s.ptrs.get(i) {
        Some(Some(p)) if p.pid != 0 => {
            if s.cur_ptr == h {
                set_shape(system(sysptr::SPTR_ARROW));
            }
            st().ptrs[i] = None;
            1
        }
        _ => 0,
    }
}

/// WinSetPointerOwner(hptr, pid, fDestroy): the pointer belongs to
/// `pid` (destroyed when it ends), or to nobody with fDestroy FALSE.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetPointerOwner(h: u32, p: u32, destroy: u32) -> u32 {
    let s = st();
    match s.ptrs.get_mut(h.wrapping_sub(BASE) as usize) {
        Some(Some(x)) if x.pid != 0 => {
            x.pid = if destroy != 0 { p } else { u32::MAX };
            1
        }
        _ => 0,
    }
}

/// The bitmaps of a pointer: the AND/XOR mask and the colour bitmap.
fn bitmaps(img: &Image, mono: bool) -> (u32, u32) {
    let (m, c) = img.to_bitmaps(!mono);
    let hm = crate::gpiobj::new_bitmap(m);
    let hc = c.map_or(0, crate::gpiobj::new_bitmap);
    (hm, hc)
}

/// POINTERINFO: fPointer, xHotspot, yHotspot, hbmPointer, hbmColor,
/// hbmMiniPointer, hbmMiniColor.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PointerInfo {
    pub f_pointer: u32,
    pub x_hot: i32,
    pub y_hot: i32,
    pub hbm_pointer: u32,
    pub hbm_color: u32,
    pub hbm_mini_pointer: u32,
    pub hbm_mini_color: u32,
}

/// WinQueryPointerInfo(hptr, pPointerInfo): new bitmaps of the caller
/// with the pointer's images.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryPointerInfo(h: u32, pi: *mut PointerInfo) -> u32 {
    let Some(p) = get(h) else { return 0 };
    if pi.is_null() {
        return 0;
    }
    let (hot, pointer, mono) = (p.hot, p.pointer, p.mono);
    let (img, mini) = (p.img.clone(), p.mini.clone());
    let (a, b) = bitmaps(&img, mono);
    let (c, d) = bitmaps(&mini, mono);
    let v = PointerInfo { f_pointer: pointer as u32, x_hot: hot.x, y_hot: hot.y, hbm_pointer: a, hbm_color: b, hbm_mini_pointer: c, hbm_mini_color: d };
    unsafe { pi.write_unaligned(v) };
    1
}

/// An image from the caller's mask bitmap and colour bitmap.
fn image_from(hbm: u32, hcolor: u32) -> Option<Image> {
    let colour = if hcolor != 0 { crate::gpiobj::with_bitmap(hcolor, |b| gpi::Bitmap { width: b.width, height: b.height, planes: b.planes, bitcount: b.bitcount, pixels: b.pixels.clone() }) } else { None };
    crate::gpiobj::with_bitmap(hbm, |m| Image::from_bitmaps(m, colour.as_ref()))
}

/// WinCreatePointer(hwndDesktop, hbmPointer, fPointer, xHot, yHot): a
/// pointer from a mask bitmap (AND mask above the XOR mask).
#[unsafe(no_mangle)]
pub extern "C" fn WinCreatePointer(_desk: u32, hbm: u32, pointer: u32, x: i32, y: i32) -> u32 {
    let Some(img) = image_from(hbm, 0) else { return 0 };
    make(img, None, pointer != 0, Point::new(x, y), true)
}

fn make(img: Image, mini: Option<Image>, pointer: bool, hot: Point, mono: bool) -> u32 {
    let (w, h) = (img.w, img.h);
    let k = |v: i32, from: i32| if from > 0 { v * ICON / from } else { v };
    let hot = Point::new(k(hot.x, w), k(hot.y, h));
    let mini = mini.map_or_else(|| img.scaled(MINI, MINI), |m| m.scaled(MINI, MINI));
    add(PtrObj { pid: pid(), hot, pointer, img: img.scaled(ICON, ICON), mini, mono })
}

/// WinCreatePointerIndirect(hwndDesktop, pptri): from POINTERINFO.
#[unsafe(no_mangle)]
pub extern "C" fn WinCreatePointerIndirect(_desk: u32, pi: *const PointerInfo) -> u32 {
    if pi.is_null() {
        return 0;
    }
    let v = unsafe { pi.read_unaligned() };
    let Some(img) = image_from(v.hbm_pointer, v.hbm_color) else { return 0 };
    let mini = if v.hbm_mini_pointer != 0 { image_from(v.hbm_mini_pointer, v.hbm_mini_color) } else { None };
    make(img, mini, v.f_pointer != 0, Point::new(v.x_hot, v.y_hot), v.hbm_color == 0)
}

pub const DP_HALFTONED: u32 = 0x0001;
pub const DP_INVERTED: u32 = 0x0002;
pub const DP_MINI: u32 = 0x0004;

/// WinDrawPointer(hps, x, y, hptr, fs): draw an icon or pointer with its
/// bottom-left corner at (x, y): DP_NORMAL, DP_MINI, DP_HALFTONED,
/// DP_INVERTED.
#[unsafe(no_mangle)]
pub extern "C" fn WinDrawPointer(hps: u32, x: i32, y: i32, h: u32, fs: u32) -> u32 {
    let Some(p) = get(h) else { return 0 };
    let img: &Image = if fs & DP_MINI != 0 { &p.mini } else { &p.img };
    let inverted;
    let img = if fs & DP_INVERTED != 0 {
        let mut i = img.clone();
        for v in i.px.iter_mut() {
            if *v & icon::OPAQUE == icon::OPAQUE {
                *v = icon::OPAQUE | !*v & 0xff_ffff;
            }
        }
        inverted = i;
        &inverted
    } else {
        img
    };
    // On the PS's target: the screen, or the bitmap of a memory PS (OS/2's
    // Klondike draws its cards' pictures into bitmaps this way).
    gpiapi::draw(hps, |e, surface| {
        let (dx, dy) = e.ps.world_to_device().apply(x as f64, y as f64);
        let o = e.ps.origin;
        icon::draw(surface, &e.ps.clip, dx as i32 + o.x, dy as i32 + o.y, img, fs & DP_HALFTONED != 0);
    })
    .is_some() as u32
}

/// Draw an icon (mini when `mini`) centred in `r` of a canvas.
pub fn draw_in(c: &mut crate::draw::Canvas, r: Rect, h: u32, mini: bool, halftone: bool) -> bool {
    let Some(p) = get(h) else { return false };
    let img = if mini { &p.mini } else { &p.img };
    let (x, y) = (r.left + (r.width() - img.w) / 2, r.bottom + (r.height() - img.h) / 2);
    c.icon(x, y, img, halftone);
    true
}

/// WinQueryPointer(hwndDesktop): the pointer shown now.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryPointer(_desk: u32) -> u32 {
    let s = st();
    if s.cur_ptr == 0 { system(sysptr::SPTR_ARROW) } else { s.cur_ptr }
}

/// WinSetPointer(hwndDesktop, hptr): the mouse pointer's shape (0
/// hides it until another is set).
#[unsafe(no_mangle)]
pub extern "C" fn WinSetPointer(_desk: u32, h: u32) -> u32 {
    if h != 0 && get(h).is_none() {
        return 0;
    }
    set_shape(h);
    1
}

/// WinSetPointerPos(hwndDesktop, x, y): not moved (the pointer follows
/// the mouse only); accepted.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetPointerPos(_desk: u32, _x: i32, _y: i32) -> u32 {
    1
}

/// ICONINFO for WinQuerySysPointerData: only the size is filled in
/// (ICON_DATA would need the file format back); not supported.
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysPointerData(_desk: u32, _id: u32, _pi: *mut u8) -> u32 {
    0
}

// ---- The mouse pointer ----

/// Show system pointer `id`.
pub fn set_system(id: u32) {
    set_shape(system(id));
}

/// Change the pointer's shape (0: no pointer).
pub fn set_shape(h: u32) {
    let s = st();
    if s.cur_ptr == h && s.cur_ptr != 0 {
        return;
    }
    erase(s);
    s.cur_ptr = h;
    if s.ptr_hardware {
        upload(s);
    }
    draw(s);
}

/// Give the shape to the hardware cursor.
fn upload(s: &mut State) {
    let Some(p) = get(s.cur_ptr) else {
        let none = [0u32; 1];
        unsafe { Rs2SetPointerShape(1, 1, 0, 0, none.as_ptr()) };
        return;
    };
    let px = p.img.argb();
    let (w, h) = (p.img.w as u32, p.img.h as u32);
    // The kernel wants the hot spot from the top-left corner.
    let (hx, hy) = (p.hot.x.max(0) as u32, (p.img.h - 1 - p.hot.y).max(0) as u32);
    unsafe { Rs2SetPointerShape(w, h, hx, hy, px.as_ptr()) };
}

/// The PM server takes the pointer over: a hardware cursor if the display
/// has one (the kernel moves it, so it never waits for PM), otherwise
/// drawn here in software.
pub fn init(s: &mut State) {
    erase(s);
    s.ptr_saved = None;
    s.ptr_hidden = 0;
    let arrow = system(sysptr::SPTR_ARROW);
    let s = st();
    s.cur_ptr = arrow;
    let p = get(arrow).expect("arrow");
    let px = p.img.argb();
    let (hx, hy) = (p.hot.x as u32, (p.img.h - 1 - p.hot.y) as u32);
    s.ptr_hardware = unsafe { Rs2SetPointerShape(p.img.w as u32, p.img.h as u32, hx, hy, px.as_ptr()) } == 0;
    draw(s);
}

/// WinShowPointer(hwndDesktop, fShow): a show count; the pointer is seen
/// while it is not negative.
#[unsafe(no_mangle)]
pub extern "C" fn WinShowPointer(_desk: u32, show: u32) -> u32 {
    let s = st();
    if show != 0 {
        s.ptr_level = s.ptr_level.saturating_add(1);
    } else {
        s.ptr_level = s.ptr_level.saturating_sub(1);
    }
    let on = s.ptr_level >= 0;
    if s.ptr_hardware {
        unsafe { Rs2ShowPointer(on as u32) };
    } else if on {
        draw(s);
    } else {
        erase(s);
    }
    1
}

/// Where the current shape goes with the hot spot at `p`: its bottom-left
/// corner, and the image.
fn placed(s: &State, p: Point) -> Option<(Point, &'static Image)> {
    let o = get(s.cur_ptr)?;
    Some((Point::new(p.x - o.hot.x, p.y - o.hot.y), &o.img))
}

pub fn draw(s: &mut State) {
    if s.ptr_hardware || s.ptr_hidden > 0 || s.ptr_level < 0 || s.ptr_saved.is_some() {
        return;
    }
    let Some((at, img)) = placed(s, s.pointer) else { return };
    let mut saved = Vec::with_capacity((img.w * img.h) as usize);
    for y in 0..img.h {
        for x in 0..img.w {
            saved.push(s.surface.get(at.x + x, at.y + y).unwrap_or(0));
        }
    }
    let all = s.surface.bounds();
    let mut surface = s.surface;
    icon::draw(&mut surface, &[all], at.x, at.y, img, false);
    s.ptr_saved = Some((at, img.w, img.h, saved));
}

pub fn erase(s: &mut State) {
    let Some((at, w, h, saved)) = s.ptr_saved.take() else { return };
    let mut i = 0;
    for y in 0..h {
        for x in 0..w {
            s.surface.set(at.x + x, at.y + y, saved[i]);
            i += 1;
        }
    }
}

/// Hide the pointer around drawing (only ever within one PMWIN call:
/// never across application code).
pub fn hide(s: &mut State) {
    erase(s);
    s.ptr_hidden += 1;
}

pub fn show(s: &mut State) {
    s.ptr_hidden = s.ptr_hidden.saturating_sub(1);
    draw(s);
}

pub fn move_to(s: &mut State, p: Point) {
    erase(s);
    s.pointer = p;
    draw(s);
}

