//! The PM state of this process: display, window tree, classes, message
//! queues and presentation spaces.
//!
//! For now every PM process has its own state (one PM application at a
//! time draws on the desktop); moving it into the DLL's shared data object
//! with a shared heap is the next step towards several applications.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use core::any::Any;
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
    /// Messages sent from other threads, and the replies to them.
    pub sent: VecDeque<Sent>,
    pub replies: Vec<(u32, u32)>,
}

pub struct PsEntry {
    pub ps: Ps,
    pub hwnd: u32,
    /// Colour indices as set (for GpiQueryColor).
    pub color_index: i32,
    pub back_index: i32,
    pub rgb_mode: bool,
}

pub struct Timer {
    pub hwnd: u32,
    pub id: u32,
    pub interval: u32,
    pub next: u32,
}

/// Frame window data.
pub struct Frame {
    /// Frame creation flags (FCF_*).
    pub fcf: u32,
    pub client: u32,
    /// Accelerator table (0: none).
    pub accel: u32,
    /// Window that had the focus when the frame was last active.
    pub focus_save: u32,
    /// Position before maximizing.
    pub restore: Option<(i32, i32, i32, i32)>,
}

/// Frame being moved or sized with the mouse.
pub struct Drag {
    pub hwnd: u32,
    /// Pointer position when the drag started.
    pub start: Point,
    /// Window position and size then.
    pub orig: (i32, i32, i32, i32),
    /// Edges being moved (1 left, 2 right, 4 bottom, 8 top); 0 = move.
    pub edges: u8,
}

/// A message sent to a window of another thread, waiting for its reply.
pub struct Sent {
    pub id: u32,
    pub hwnd: u32,
    pub msg: u32,
    pub mp1: u32,
    pub mp2: u32,
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
    /// Data of the built-in control windows, by window.
    pub ctl: BTreeMap<u32, Box<dyn Any>>,
    /// Accelerator tables (handle = index + 1).
    pub accels: Vec<Option<Vec<crate::res::Accel>>>,
    /// The message queue's accelerator table.
    pub queue_accel: u32,
    /// Menu tracking state.
    pub menu_mode: Option<crate::menu::Mode>,
    /// Last button press (time, position, button) for double clicks.
    pub last_click: (u32, Point, u8),
    pub next_sent: u32,
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
            ctl: BTreeMap::new(),
            accels: Vec::new(),
            queue_accel: 0,
            menu_mode: None,
            last_click: (0, Point::new(-100, -100), 0),
            next_sent: 1,
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

    /// The queue whose thread reads the kernel input (the first one
    /// created): window procedures then run on their own threads.
    pub fn input_queue(&self) -> u32 {
        self.queues.keys().next().copied().unwrap_or(0)
    }

    pub fn class(&self, name: &[u8]) -> Option<&Class> {
        self.classes.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }
}
