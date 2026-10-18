//! The PM state of this process: display, window tree, classes, message
//! queues and presentation spaces.
//!
//! For now every PM process has its own state (one PM application at a
//! time draws on the desktop); moving it into the DLL's shared data object
//! with a shared heap is the next step towards several applications.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use gpi::{Point, Ps, Surface};
use pmwin::Desktop;

/// A window procedure: (hwnd, msg, mp1, mp2) -> result, `_System`.
pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

pub struct Class {
    pub name: Vec<u8>,
    pub proc_: u32,
    pub style: u32,
    /// Extra window bytes.
    pub words: u32,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct QMsg {
    pub hwnd: u32,
    pub msg: u32,
    pub mp1: u32,
    pub mp2: u32,
    pub time: u32,
    pub x: i32,
    pub y: i32,
    pub reserved: u32,
}

pub struct Queue {
    pub msgs: VecDeque<QMsg>,
    pub tid: u32,
}

pub struct PsEntry {
    pub ps: Ps,
    pub hwnd: u32,
    /// Colour indices as set (for GpiQueryColor).
    pub color_index: i32,
    pub back_index: i32,
    pub rgb_mode: bool,
    pub gpi: crate::gpiapi::PsExtra,
}

pub struct Timer {
    pub hwnd: u32,
    pub id: u32,
    pub interval: u32,
    pub next: u32,
}

/// Frame window data: creation flags and the client.
pub struct Frame {
    pub fcf: u32,
    pub client: u32,
}

/// Window being moved with the mouse: frame and the pointer offset.
pub struct Drag {
    pub hwnd: u32,
    pub dx: i32,
    pub dy: i32,
}

pub struct State {
    pub surface: Surface,
    pub desk: Desktop,
    pub classes: Vec<Class>,
    pub queues: BTreeMap<u32, Queue>,
    pub frames: BTreeMap<u32, Frame>,
    pub pss: Vec<Option<PsEntry>>,
    pub timers: Vec<Timer>,
    pub focus: u32,
    pub active: u32,
    pub capture: u32,
    pub drag: Option<Drag>,
    pub pointer: Point,
    pub buttons: u8,
    pub last_error: u32,
    /// Pointer image save-under: position and pixels, when drawn.
    pub ptr_saved: Option<(Point, Vec<u32>)>,
    pub ptr_hidden: u32,
    pub next_hmq: u32,
    /// Next position for FCF_SHELLPOSITION windows.
    pub cascade: i32,
}

static mut STATE: Option<State> = None;

pub fn get() -> Option<&'static mut State> {
    unsafe { (*core::ptr::addr_of_mut!(STATE)).as_mut() }
}

pub fn st() -> &'static mut State {
    get().expect("PM not initialised")
}

pub fn init(surface: Surface) {
    let desk = Desktop::new(surface.width, surface.height);
    unsafe {
        *core::ptr::addr_of_mut!(STATE) = Some(State {
            surface,
            desk,
            classes: Vec::new(),
            queues: BTreeMap::new(),
            frames: BTreeMap::new(),
            pss: Vec::new(),
            timers: Vec::new(),
            focus: 0,
            active: 0,
            capture: 0,
            drag: None,
            pointer: Point::new(surface.width / 2, surface.height / 2),
            buttons: 0,
            last_error: 0,
            ptr_saved: None,
            ptr_hidden: 0,
            next_hmq: 1,
            cascade: 0,
        });
    }
}

/// The calling thread's ID.
pub fn tid() -> u32 {
    let mut tib: *mut os2::Tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe {
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        (*(*tib).ptib2).tid
    }
}

pub fn now() -> u32 {
    let mut v = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(14, 14, v.as_mut_ptr(), 4) };
    v[0]
}

impl State {
    /// The calling thread's message queue.
    pub fn my_queue(&self) -> u32 {
        let t = tid();
        self.queues.iter().find(|(_, q)| q.tid == t).map_or(0, |(&h, _)| h)
    }

    pub fn class(&self, name: &[u8]) -> Option<&Class> {
        self.classes.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }
}
