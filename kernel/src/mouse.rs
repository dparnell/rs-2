//! Mouse: the PS/2 auxiliary device and USB mice feed motion here; it is
//! turned into OS/2 mouse events (MOUEVENTINFO) for the foreground
//! session and moves the text-mode pointer.

use alloc::collections::{BTreeMap, VecDeque};

use crate::arch::idt::TrapFrame;
use crate::arch::{inb, outb};
use crate::sync::SpinLock;
use crate::task::WaitQueue;

/// Event bits (MOUEVENTINFO.fs and the event mask).
pub mod ev {
    pub const MOTION: u16 = 0x0001;
    pub const MOTION_WITH_BN1: u16 = 0x0002;
    pub const BN1: u16 = 0x0004;
    pub const MOTION_WITH_BN2: u16 = 0x0008;
    pub const BN2: u16 = 0x0010;
    pub const MOTION_WITH_BN3: u16 = 0x0020;
    pub const BN3: u16 = 0x0040;
}

#[derive(Clone, Copy, Default)]
pub struct Event {
    pub fs: u16,
    pub time: u32,
    pub row: i16,
    pub col: i16,
}

const QUEUE_MAX: usize = 64;

/// Per-session mouse state.
pub struct Session {
    pub open: bool,
    pub queue: VecDeque<Event>,
    pub event_mask: u16,
    pub dev_status: u16,
    /// Mickeys per 8 pixels (rows, columns).
    pub scale: (u16, u16),
    pub row: i32,
    pub col: i32,
    acc: (i32, i32),
    pub shown: bool,
    /// Cell under the drawn pointer: position and original content.
    drawn: Option<(usize, usize, u16)>,
}

impl Session {
    const fn new() -> Self {
        Session {
            open: false,
            queue: VecDeque::new(),
            event_mask: 0x7f,
            dev_status: 0,
            scale: (16, 8),
            row: 12,
            col: 40,
            acc: (0, 0),
            shown: false,
            drawn: None,
        }
    }
}

struct State {
    sessions: BTreeMap<u32, Session>,
    buttons: u8,
    /// PS/2 packet being assembled.
    packet: [u8; 3],
    have: usize,
    ps2: bool,
}

static STATE: SpinLock<State> = SpinLock::new(State { sessions: BTreeMap::new(), buttons: 0, packet: [0; 3], have: 0, ps2: false });
static WAITERS: WaitQueue = WaitQueue::new();

/// Run `f` on the mouse state of session `sid`.
pub fn with<R>(sid: u32, f: impl FnOnce(&mut Session) -> R) -> R {
    let mut s = STATE.lock();
    f(s.sessions.entry(sid).or_insert_with(Session::new))
}

pub fn present() -> bool {
    let ps2 = STATE.lock().ps2;
    ps2 || crate::drivers::hid::mouse_present()
}

pub fn drop_session(sid: u32) {
    STATE.lock().sessions.remove(&sid);
}

/// Motion from a device: button bits (0 left, 1 right, 2 middle) and
/// movement with y growing downwards.
pub fn motion(buttons: u8, dx: i32, dy: i32) {
    let sid = crate::session::foreground();
    let time = crate::irq::uptime_ms() as u32;
    let mut s = STATE.lock();
    let prev = core::mem::replace(&mut s.buttons, buttons);
    let Some(m) = s.sessions.get_mut(&sid) else { return };
    if !m.open {
        return;
    }
    let (rows, cols) = crate::screen::with_session(sid, |sc| (sc.rows as i32, sc.cols as i32)).unwrap_or((25, 80));
    m.acc.0 += dy;
    m.acc.1 += dx;
    let (rs, cs) = (m.scale.0.max(1) as i32, m.scale.1.max(1) as i32);
    let (drow, dcol) = (m.acc.0 / rs, m.acc.1 / cs);
    m.acc.0 -= drow * rs;
    m.acc.1 -= dcol * cs;
    let (orow, ocol) = (m.row, m.col);
    m.row = (m.row + drow).clamp(0, rows - 1);
    m.col = (m.col + dcol).clamp(0, cols - 1);
    let moved = (m.row, m.col) != (orow, ocol);
    if !moved && prev == buttons {
        return;
    }
    // OS/2 button 1 is the left button, 2 the right, 3 the middle.
    let mut fs = 0;
    let bits = [(1u8, ev::BN1, ev::MOTION_WITH_BN1), (2, ev::BN2, ev::MOTION_WITH_BN2), (4, ev::BN3, ev::MOTION_WITH_BN3)];
    for (b, down, mv) in bits {
        if buttons & b != 0 {
            fs |= if moved { mv } else { down };
        }
    }
    if fs == 0 && moved {
        fs = ev::MOTION;
    }
    if fs & m.event_mask != 0 || (fs == 0 && prev != 0) {
        if m.queue.len() >= QUEUE_MAX {
            m.queue.pop_front();
        }
        m.queue.push_back(Event { fs: fs & m.event_mask, time, row: m.row as i16, col: m.col as i16 });
    }
    if moved && m.shown {
        redraw(sid, m);
    }
    drop(s);
    WAITERS.wake_all();
}

/// Next event of the calling thread's session.
pub fn read(sid: u32, wait: bool) -> Option<Event> {
    loop {
        if let Some(e) = with(sid, |m| m.queue.pop_front()) {
            return Some(e);
        }
        if !wait || !WAITERS.wait_until(None, || with(sid, |m| !m.queue.is_empty())) {
            return None;
        }
    }
}

// ---- The text-mode pointer: the cell under it with inverted colours ----

fn undraw(sid: u32, m: &mut Session) {
    let Some((r, c, cell)) = m.drawn.take() else { return };
    crate::screen::with_session(sid, |s| {
        // Leave the cell alone if it was rewritten meanwhile.
        if r < s.rows && c < s.cols && s.cell(r, c) == invert(cell) {
            s.put(r, c, cell);
        }
    });
}

fn invert(cell: u16) -> u16 {
    cell ^ 0x7700
}

fn redraw(sid: u32, m: &mut Session) {
    undraw(sid, m);
    if !m.shown {
        return;
    }
    let (r, c) = (m.row as usize, m.col as usize);
    m.drawn = crate::screen::with_session(sid, |s| {
        if r >= s.rows || c >= s.cols {
            return None;
        }
        let cell = s.cell(r, c);
        s.put(r, c, invert(cell));
        Some((r, c, cell))
    })
    .flatten();
}

pub fn show(sid: u32, on: bool) {
    let mut s = STATE.lock();
    let m = s.sessions.entry(sid).or_insert_with(Session::new);
    m.shown = on;
    redraw(sid, m);
}

pub fn set_pos(sid: u32, row: i32, col: i32) {
    let mut s = STATE.lock();
    let m = s.sessions.entry(sid).or_insert_with(Session::new);
    m.row = row;
    m.col = col;
    m.acc = (0, 0);
    if m.shown {
        redraw(sid, m);
    }
}

// ---- PS/2 auxiliary device ----

fn wait_write() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 2 == 0 {
            return true;
        }
    }
    false
}

fn wait_read() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 1 != 0 {
            return true;
        }
    }
    false
}

/// Write a controller command (port 0x64) or data byte (port 0x60).
fn ctl(cmd: u8) -> bool {
    wait_write() && {
        unsafe { outb(0x64, cmd) };
        true
    }
}

fn data(b: u8) -> bool {
    wait_write() && {
        unsafe { outb(0x60, b) };
        true
    }
}

/// Send a byte to the mouse and expect an acknowledgement.
fn aux_command(b: u8) -> bool {
    ctl(0xd4) && data(b) && wait_read() && unsafe { inb(0x60) } == 0xfa
}

/// Enable the PS/2 mouse, if there is one, on IRQ 12.
pub fn init() {
    // Enable the auxiliary port and its interrupt in the configuration byte.
    let ok = ctl(0xa8) && ctl(0x20) && wait_read() && {
        let cfg = (unsafe { inb(0x60) } | 0x02) & !0x20;
        ctl(0x60) && data(cfg)
    };
    // Defaults, then streaming.
    if !ok || !aux_command(0xf6) || !aux_command(0xf4) {
        return;
    }
    STATE.lock().ps2 = true;
    crate::irq::register(12, irq);
    crate::println!("mouse: PS/2");
}

fn irq(_f: &mut TrapFrame) {
    let b = unsafe { inb(0x60) };
    let pkt = {
        let mut s = STATE.lock();
        // Byte 0 always has bit 3 set: resynchronise on it (skipping
        // stray acknowledgements, 0xFA, which look like overflow).
        if s.have == 0 && (b & 0x08 == 0 || b & 0xc0 != 0) {
            return;
        }
        let n = s.have;
        s.packet[n] = b;
        s.have += 1;
        if s.have < 3 {
            return;
        }
        s.have = 0;
        s.packet
    };
    let mut dx = pkt[1] as i32;
    let mut dy = pkt[2] as i32;
    if pkt[0] & 0x10 != 0 {
        dx -= 256;
    }
    if pkt[0] & 0x20 != 0 {
        dy -= 256;
    }
    if pkt[0] & 0xc0 != 0 {
        return; // overflow
    }
    // PS/2 y grows upwards.
    motion(pkt[0] & 7, dx, -dy);
}
