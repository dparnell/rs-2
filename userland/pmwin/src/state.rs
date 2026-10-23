//! The PM state shared by every PM process: display, window tree,
//! classes, message queues, focus and input state, presentation spaces.
//!
//! It lives in the shared object of `shared.rs` (same address in every
//! process) and is only touched with the PM lock held. Things that only
//! make sense in one process (private window classes, presentation
//! spaces, GPI objects, accelerator tables) carry that process's pid and
//! go away with it; window procedures are only called on the thread that
//! owns the window's queue.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use core::any::Any;

use gpi::{Point, Ps, Surface};
use pmwin::{Desktop, MsgQueue};

pub use pmwin::QMsg;
pub use crate::shared::{pid, tid};

/// A window procedure: (hwnd, msg, mp1, mp2) -> result, `_System`.
pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

pub struct Class {
    pub name: Vec<u8>,
    pub proc_: u32,
    pub style: u32,
    /// Extra window bytes.
    pub words: u32,
    /// Process that registered it (0: built in or public, usable by all).
    pub pid: u32,
}

pub struct Queue {
    pub pid: u32,
    pub tid: u32,
    /// Shared event semaphore the thread waits on.
    pub sem: u32,
    pub msgs: MsgQueue,
    /// Messages sent from other threads, waiting to be processed.
    pub sent: VecDeque<Sent>,
    /// Menu tracking state (the queue's menu window has the capture).
    pub menu_mode: Option<crate::menu::Mode>,
    /// The queue's accelerator table.
    pub accel: u32,
    /// Position and time of the message last taken off the queue.
    pub msg_pos: Point,
    pub msg_time: u32,
    /// Marked "not responding" by the PM server.
    pub hung: bool,
}

pub struct PsEntry {
    pub ps: Ps,
    pub hwnd: u32,
    /// Colour indices as set (for GpiQueryColor).
    pub color_index: i32,
    pub back_index: i32,
    pub rgb_mode: bool,
    pub gpi: crate::gpiapi::PsExtra,
    /// Process that owns the PS.
    pub pid: u32,
}

pub struct Timer {
    pub hwnd: u32,
    /// Queue the WM_TIMER goes to.
    pub queue: u32,
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
    /// The frame's icon (WM_SETICON, FCF_ICON).
    pub icon: u32,
    /// While minimized: what restoring brings back.
    pub min: Option<Minimized>,
    /// Where the frame was last shown minimized.
    pub icon_pos: Option<(i32, i32)>,
    /// The help instance (WM_SETHELPINFO).
    pub help: u32,
    /// FCF_SHELLPOSITION and not shown yet: the first time it is shown
    /// while it still has no size, the frame gets the shell's position.
    pub shell_pos: bool,
}

/// A minimized frame's state before minimizing.
pub struct Minimized {
    pub pos: (i32, i32, i32, i32),
    pub was_max: bool,
    /// Frame controls and client that were visible.
    pub hidden: Vec<u32>,
    /// Hidden for a minimized window viewer instead of shown as an icon.
    pub viewer: bool,
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

/// A message sent to a window of another thread.
pub struct Sent {
    pub id: u32,
    pub hwnd: u32,
    pub msg: u32,
    pub mp1: u32,
    pub mp2: u32,
    /// Queue to wake with the reply (0: the sender has none and polls).
    pub from: u32,
    /// The sender waits for the result (a send from the PM server does
    /// not: it never blocks on an application).
    pub reply: bool,
}

pub struct AccelTable {
    pub pid: u32,
    pub keys: Vec<crate::res::Accel>,
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
    /// Pointer image save-under: position and pixels, when drawn.
    pub ptr_saved: Option<(Point, i32, i32, Vec<u32>)>,
    /// Pointer and icon objects (HPOINTER, see pointer.rs).
    pub ptrs: Vec<Option<crate::pointer::PtrObj>>,
    /// Handles of the system pointers by SPTR_* id (0: not made yet).
    pub sys_ptrs: Vec<u32>,
    /// The mouse pointer's shape.
    pub cur_ptr: u32,
    /// A window that takes minimized frames over (Rs2SetMinimizeViewer):
    /// they are hidden and it is told; 0: they become desktop icons.
    pub min_viewer: u32,
    /// The window the pointer was last over.
    pub ptr_window: u32,
    pub ptr_hidden: u32,
    /// The pointer is a hardware cursor (the kernel moves it).
    pub ptr_hardware: bool,
    /// WinShowPointer level (shown while >= 0).
    pub ptr_level: i32,
    pub next_hmq: u32,
    /// Next position for FCF_SHELLPOSITION windows.
    pub cascade: i32,
    /// Data of the built-in control windows, by window.
    pub ctl: BTreeMap<u32, Box<dyn Any>>,
    /// Accelerator tables (handle = index + 1).
    pub accels: Vec<Option<AccelTable>>,
    /// Last button press (time, position, button) for double clicks.
    pub last_click: (u32, Point, u8),
    pub next_sent: u32,
    /// Replies to sent messages: (id, result).
    pub replies: Vec<(u32, u32)>,
    /// Queues to wake when the lock is released.
    pub wake: BTreeSet<u32>,
    /// GPI objects (regions, bitmaps, DCs, palettes) of each process.
    pub objects: BTreeMap<u32, crate::gpiobj::Objects>,
    /// PM processes (pids) that have called WinInitialize.
    pub procs: Vec<u32>,
    /// The PM server process and its window thread's queue (0 while
    /// there is none).
    pub server_pid: u32,
    pub server_queue: u32,
    /// Milliseconds after which a queue that does not take its input is
    /// "not responding" (PM_HANGTIME).
    pub hang_ms: u32,
    pub clip: crate::clip::Clipboard,
    pub atoms: Vec<Option<crate::clip::AtomTable>>,
    /// Hooks (WinSetHook).
    pub hooks: Vec<Hook>,
    /// Windows showing windowed VIO sessions: (hwnd, session, VW_* flags,
    /// host pid).
    pub vio: Vec<(u32, u32, u32, u32)>,
    /// The framebuffer, and the memory PM draws into while a text
    /// session has the screen (so nothing PM draws then reaches it).
    pub screen: usize,
    pub shadow: Vec<u32>,
    /// What was drawn on the screen since the kernel was last told
    /// (`display::flush_damage`).
    pub damage: gpi::Damage,
    /// Presentation parameter colours set on windows: (foreground,
    /// background) as 0x00RRGGBB, NO_COLOR where not set.
    pub pres: BTreeMap<u32, (u32, u32)>,
    /// System values and colours set with WinSetSysValue and WinSetSysColors
    /// (the rest are the defaults).
    pub sysvals: BTreeMap<i32, i32>,
    pub syscolors: BTreeMap<i32, u32>,
    /// Virtual key states by VK_* (0x80: down, 0x01: toggled), as the
    /// input thread last saw them (WinGetKeyState).
    pub keys: [u8; 256],
    /// The switch list (WinAddSwitchEntry, see switch.rs).
    pub switch: crate::switch::SwitchList,
    /// The text cursor (WinCreateCursor).
    pub cursor: pmwin::cursor::CursorState,
    /// Metafiles (gpiseg.rs).
    pub metafiles: Vec<Option<crate::gpiseg::Mf>>,
}

/// No presentation parameter colour.
pub const NO_COLOR: u32 = u32::MAX;

/// A hook set with WinSetHook: for one queue (or all, 0), of one kind
/// (HK_*), in a process.
pub struct Hook {
    pub queue: u32,
    pub kind: u32,
    pub pfn: u32,
    #[allow(dead_code)]
    pub hmod: u32,
    pub pid: u32,
}

/// The state, if PM has been initialised in the system.
pub fn get() -> Option<&'static mut State> {
    let h = crate::shared::header();
    unsafe { ((*h).state as *mut State).as_mut() }
}

pub fn st() -> &'static mut State {
    get().expect("PM not initialised")
}

/// Create the state (the first PM process does).
pub fn init(surface: Surface) {
    let desk = Desktop::new(surface.width, surface.height);
    let s = Box::new(State {
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
        ptr_saved: None,
        ptrs: Vec::new(),
        sys_ptrs: Vec::new(),
        cur_ptr: 0,
        ptr_window: 0,
        min_viewer: 0,
        pres: BTreeMap::new(),
        sysvals: BTreeMap::new(),
        syscolors: BTreeMap::new(),
        ptr_hidden: 0,
        ptr_hardware: false,
        ptr_level: 0,
        next_hmq: 1,
        cascade: 0,
        ctl: BTreeMap::new(),
        accels: Vec::new(),
        last_click: (0, Point::new(-100, -100), 0),
        next_sent: 1,
        replies: Vec::new(),
        wake: BTreeSet::new(),
        objects: BTreeMap::new(),
        procs: Vec::new(),
        server_pid: 0,
        server_queue: 0,
        hang_ms: 2000,
        clip: Default::default(),
        atoms: Vec::new(),
        hooks: Vec::new(),
        vio: Vec::new(),
        screen: surface.pixels() as usize,
        shadow: Vec::new(),
        damage: gpi::Damage::new(),
        keys: [0; 256],
        switch: Default::default(),
        cursor: Default::default(),
        metafiles: Vec::new(),
    });
    let p = Box::into_raw(s);
    unsafe {
        (*p).surface.set_damage(&mut (*p).damage);
        (*crate::shared::header()).state = p as *mut u8;
    }
}

/// PM left the screen to a text session: draw into the shadow buffer.
pub fn to_shadow() {
    let s = st();
    if s.surface.pixels() as usize != s.screen {
        return;
    }
    if s.shadow.is_empty() {
        s.shadow = alloc::vec![0; (s.surface.stride * s.surface.height) as usize];
    }
    s.ptr_saved = None;
    let p = s.shadow.as_mut_ptr();
    unsafe { s.surface.set_pixels(p) };
}

/// PM has the screen again (everything is repainted).
pub fn to_screen() {
    let s = st();
    let p = s.screen as *mut u32;
    unsafe { s.surface.set_pixels(p) };
}

pub fn now() -> u32 {
    let mut v = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(14, 14, v.as_mut_ptr(), 4) };
    v[0]
}

impl State {
    /// The calling thread's message queue.
    pub fn my_queue(&self) -> u32 {
        let (p, t) = (pid(), tid());
        self.queues.iter().find(|(_, q)| q.tid == t && q.pid == p).map_or(0, |(&h, _)| h)
    }

    /// A class by name: the calling process's own first, then the
    /// built-in and public ones.
    pub fn class(&self, name: &[u8]) -> Option<&Class> {
        let p = pid();
        self.classes
            .iter()
            .find(|c| c.pid == p && c.name.eq_ignore_ascii_case(name))
            .or_else(|| self.classes.iter().find(|c| c.pid == 0 && c.name.eq_ignore_ascii_case(name)))
    }

    /// The queue of a window's thread.
    pub fn queue_of(&self, h: u32) -> u32 {
        self.desk.get(h).map_or(0, |w| w.queue)
    }

    /// The window's thread does not respond.
    pub fn is_hung(&self, h: u32) -> bool {
        self.queues.get(&self.queue_of(h)).is_some_and(|q| q.hung)
    }

    /// The calling thread's queue, if it has one.
    pub fn my_queue_mut(&mut self) -> Option<&mut Queue> {
        let q = self.my_queue();
        self.queues.get_mut(&q)
    }

    /// The calling process is the PM server.
    pub fn is_server(&self) -> bool {
        self.server_pid != 0 && self.server_pid == pid()
    }
}

/// The calling thread's menu mode (menus are tracked per queue).
pub fn menu_mode() -> &'static mut Option<crate::menu::Mode> {
    static mut NONE: Option<crate::menu::Mode> = None;
    match st().my_queue_mut() {
        Some(q) => &mut q.menu_mode,
        None => unsafe {
            let n = &mut *core::ptr::addr_of_mut!(NONE);
            *n = None;
            n
        },
    }
}
