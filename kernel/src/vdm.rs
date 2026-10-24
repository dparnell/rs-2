//! DOS sessions (see docs/DOS.md): sessions whose leader is `VDM.EXE`,
//! the CPU emulator running a DOS machine. The kernel's part is small:
//! such a session gets its keyboard as raw set-1 scan codes (make and
//! break, as a DOS program reads them from port 60h) and its mouse as
//! raw motion, both queued here for the VDM (RS2API `Rs2VdmReadInput`);
//! its screen is the session's text screen, or a picture in a graphics
//! mode (`screen::Pixmap`) that the kernel scales onto the framebuffer
//! and PMVIOWIN shows in a window.

use alloc::collections::{BTreeMap, VecDeque};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::sync::SpinLock;
use crate::task::WaitQueue;

/// Where `VDM.EXE` and rs2's `COMMAND.COM` live.
pub const VDM_PATH: &str = "C:\\OS2\\MDOS\\VDM.EXE";
pub const COMMAND_PATH: &str = "C:\\OS2\\MDOS\\COMMAND.COM";

pub const EV_KEY: u16 = 1;
pub const EV_MOUSE: u16 = 2;
pub const EV_MOUSE_ABS: u16 = 3;

/// One input event: kind, flags (key: the character the layout gives;
/// mouse: buttons), a (key: scan code byte; mouse: dx or x 0-65535), b
/// (mouse: dy or y).
#[derive(Clone, Copy, Default)]
pub struct Event {
    pub kind: u16,
    pub flags: u16,
    pub a: i32,
    pub b: i32,
}

struct Vdm {
    events: VecDeque<Event>,
    /// Shift, Ctrl and Alt as the session last saw them pressed (keys
    /// from its window).
    held: u8,
}

const QUEUE_MAX: usize = 512;

static VDMS: SpinLock<BTreeMap<u32, Vdm>> = SpinLock::new(BTreeMap::new());
static WAIT: WaitQueue = WaitQueue::new();
/// Any DOS session exists (a quick test for the input paths).
static ANY: AtomicBool = AtomicBool::new(false);

/// Make `sid` a DOS session.
pub fn register(sid: u32) {
    VDMS.lock().entry(sid).or_insert_with(|| Vdm { events: VecDeque::new(), held: 0 });
    ANY.store(true, Ordering::Relaxed);
}

pub fn is_dos(sid: u32) -> bool {
    ANY.load(Ordering::Relaxed) && VDMS.lock().contains_key(&sid)
}

pub fn drop_session(sid: u32) {
    let mut g = VDMS.lock();
    g.remove(&sid);
    ANY.store(!g.is_empty(), Ordering::Relaxed);
    drop(g);
    WAIT.wake_all();
}

fn push(sid: u32, e: Event) -> bool {
    let mut g = VDMS.lock();
    let Some(v) = g.get_mut(&sid) else { return false };
    if v.events.len() < QUEUE_MAX {
        v.events.push_back(e);
    }
    drop(g);
    WAIT.wake_all();
    true
}

/// A key for DOS session `sid`: its scan code (bit 7 for a release),
/// whether it had an E0 prefix, and the character the layout gives a
/// press. False if `sid` is not a DOS session.
pub fn key(sid: u32, code: u8, ext: bool, ch: u8) -> bool {
    if !is_dos(sid) {
        return false;
    }
    if ext {
        push(sid, Event { kind: EV_KEY, flags: 0, a: 0xe0, b: 0 });
    }
    push(sid, Event { kind: EV_KEY, flags: ch as u16, a: code as i32, b: 0 })
}

/// The modifiers' scan codes (left Shift, Ctrl, Alt) and their bits in
/// `Vdm::held` and in the keyboard's shift state.
const MODIFIERS: [(u8, u8, u16); 3] = [(0x2a, 1, 0x0003), (0x1d, 2, 0x0004), (0x38, 4, 0x0008)];

/// A key from the PM window showing DOS session `sid`, with the shift
/// state it was typed in. PM does not pass on Shift, Ctrl and Alt by
/// themselves, so their presses and releases are made up from the
/// shift state when it differs from what the session saw.
pub fn key_window(sid: u32, code: u8, ext: bool, ch: u8, shift: u16) -> bool {
    if !is_dos(sid) {
        return false;
    }
    let is_mod = matches!(code & 0x7f, 0x2a | 0x36 | 0x1d | 0x38);
    let mut g = VDMS.lock();
    let Some(v) = g.get_mut(&sid) else { return false };
    for (sc, bit, mask) in MODIFIERS {
        let want = shift & mask != 0;
        let had = v.held & bit != 0;
        if is_mod && (code & 0x7f == sc || (sc == 0x2a && code & 0x7f == 0x36)) {
            v.held = if code & 0x80 == 0 { v.held | bit } else { v.held & !bit };
        } else if want != had && v.events.len() < QUEUE_MAX {
            v.events.push_back(Event { kind: EV_KEY, flags: 0, a: (sc | if want { 0 } else { 0x80 }) as i32, b: 0 });
            v.held ^= bit;
        }
    }
    drop(g);
    key(sid, code, ext, ch)
}

/// Mouse motion while DOS session `sid` has the screen.
pub fn mouse(sid: u32, buttons: u8, dx: i32, dy: i32) -> bool {
    is_dos(sid) && push(sid, Event { kind: EV_MOUSE, flags: buttons as u16, a: dx, b: dy })
}

/// The mouse at a point of the screen (0-65535 across and down).
pub fn mouse_abs(sid: u32, buttons: u8, x: u32, y: u32) -> bool {
    is_dos(sid) && push(sid, Event { kind: EV_MOUSE_ABS, flags: buttons as u16, a: x.min(65535) as i32, b: y.min(65535) as i32 })
}

/// Take up to `max` events of `sid`, waiting until `deadline` (ticks)
/// for the first. None if `sid` is not a DOS session (or interrupted).
pub fn read(sid: u32, max: usize, deadline: Option<u64>) -> Option<alloc::vec::Vec<Event>> {
    let take = || -> Option<alloc::vec::Vec<Event>> {
        let mut g = VDMS.lock();
        let v = g.get_mut(&sid)?;
        let n = v.events.len().min(max);
        Some(v.events.drain(..n).collect())
    };
    let first = take()?;
    if !first.is_empty() || deadline == Some(0) {
        return Some(first);
    }
    WAIT.wait_until(deadline, || VDMS.lock().get(&sid).is_none_or(|v| !v.events.is_empty()));
    take()
}
