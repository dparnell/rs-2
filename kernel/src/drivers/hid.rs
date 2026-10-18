//! USB HID boot-protocol keyboards and mice.
//!
//! Keyboard reports (modifier byte, reserved byte, up to six usages) are
//! compared with the previous report; every press and release becomes the
//! set-1 scancode a PS/2 keyboard would send and goes through
//! `kbd::scancode`, so shift state, Ctrl+C and character translation stay in
//! one place. USB keyboards do not repeat by themselves (we ask for idle
//! rate 0), so typematic repeat is generated here.
//!
//! Mouse reports are collected into a button/motion state and a small event
//! queue for a future MOUCALLS implementation.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::sync::SpinLock;
use crate::task::WaitQueue;

/// Typematic delay and rate (30 characters per second).
const REPEAT_DELAY_MS: u64 = 500;
const REPEAT_MS: u64 = 33;
/// Mouse events kept when nobody reads them.
const MOUSE_QUEUE: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Keyboard,
    Mouse,
}

impl Kind {
    pub fn from_protocol(p: u8) -> Option<Kind> {
        match p {
            1 => Some(Kind::Keyboard),
            2 => Some(Kind::Mouse),
            _ => None,
        }
    }
}

struct Keyboard {
    handle: u32,
    /// Previous report: modifiers and key usages.
    mods: u8,
    keys: [u8; 6],
    /// Key being repeated and when it repeats next.
    repeat: Option<(u8, u64)>,
}

/// One mouse report.
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseEvent {
    /// Bit 0 = left, 1 = right, 2 = middle.
    pub buttons: u8,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    pub time: u32,
}

/// Current mouse buttons and the motion accumulated since the last
/// [`mouse_take`].
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseState {
    pub buttons: u8,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
}

struct State {
    next: u32,
    keyboards: Vec<Keyboard>,
    mice: Vec<u32>,
    mouse: MouseState,
    events: VecDeque<MouseEvent>,
}

static STATE: SpinLock<State> = SpinLock::new(State {
    next: 1,
    keyboards: Vec::new(),
    mice: Vec::new(),
    mouse: MouseState { buttons: 0, dx: 0, dy: 0, wheel: 0 },
    events: VecDeque::new(),
});
static MOUSE_WAITERS: WaitQueue = WaitQueue::new();

/// Set-1 scancode for a keyboard usage (page 7); bit 8 = 0xE0 prefix.
fn usage_scancode(u: u8) -> Option<u16> {
    const E: u16 = 0x100;
    // Usages 0x04..=0x65.
    const MAIN: [u16; 0x62] = [
        0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, // a-p
        0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c, // q-z
        0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, // 1-0
        0x1c, 0x01, 0x0e, 0x0f, 0x39, // Enter Esc Backspace Tab Space
        0x0c, 0x0d, 0x1a, 0x1b, 0x2b, 0x2b, 0x27, 0x28, 0x29, 0x33, 0x34, 0x35, // - = [ ] \ # ; ' ` , . /
        0x3a, // CapsLock
        0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x57, 0x58, // F1-F12
        E | 0x37, 0x46, 0, // PrintScreen ScrollLock Pause (special)
        E | 0x52, E | 0x47, E | 0x49, E | 0x53, E | 0x4f, E | 0x51, // Ins Home PgUp Del End PgDn
        E | 0x4d, E | 0x4b, E | 0x50, E | 0x48, // Right Left Down Up
        0x45, E | 0x35, 0x37, 0x4a, 0x4e, E | 0x1c, // NumLock KP/ KP* KP- KP+ KPEnter
        0x4f, 0x50, 0x51, 0x4b, 0x4c, 0x4d, 0x47, 0x48, 0x49, 0x52, 0x53, // KP1-KP9 KP0 KP.
        0x56, E | 0x5d, // non-US \, Application
    ];
    match u {
        0x04..=0x65 => Some(MAIN[(u - 4) as usize]).filter(|&s| s != 0),
        _ => None,
    }
}

/// Modifier bit `n` of the report: LCtrl LShift LAlt LGUI RCtrl RShift
/// RAlt RGUI.
const MODIFIERS: [u16; 8] = [0x1d, 0x2a, 0x38, 0x15b, 0x11d, 0x36, 0x138, 0x15c];

/// Keys that must not repeat (lock keys toggle on every make code).
fn repeats(u: u8) -> bool {
    !matches!(u, 0x39 | 0x47 | 0x48 | 0x53)
}

/// Scancode bytes produced by one report, fed to `kbd` after the lock is
/// released.
struct Codes {
    bytes: [u8; 64],
    n: usize,
}

impl Codes {
    fn new() -> Codes {
        Codes { bytes: [0; 64], n: 0 }
    }

    fn key(&mut self, sc: u16, release: bool) {
        if self.n + 2 > self.bytes.len() {
            return;
        }
        if sc & 0x100 != 0 {
            self.bytes[self.n] = 0xe0;
            self.n += 1;
        }
        self.bytes[self.n] = sc as u8 | if release { 0x80 } else { 0 };
        self.n += 1;
    }

    fn usage(&mut self, u: u8, release: bool, ctrl: bool) {
        // Pause: only Ctrl+Pause (Break) means anything to the console.
        if u == 0x48 {
            if ctrl {
                self.key(0x146, release);
            }
            return;
        }
        if let Some(sc) = usage_scancode(u) {
            self.key(sc, release);
        }
    }

    fn feed(&self) {
        for &b in &self.bytes[..self.n] {
            crate::kbd::scancode(b);
        }
    }
}

/// Register a new boot-protocol device; returns its handle.
pub fn attach(kind: Kind) -> u32 {
    let mut s = STATE.lock();
    let h = s.next;
    s.next += 1;
    match kind {
        Kind::Keyboard => s.keyboards.push(Keyboard { handle: h, mods: 0, keys: [0; 6], repeat: None }),
        Kind::Mouse => s.mice.push(h),
    }
    h
}

/// The device went away: release whatever it still holds down.
pub fn detach(handle: u32) {
    let mut codes = Codes::new();
    {
        let mut s = STATE.lock();
        if let Some(i) = s.keyboards.iter().position(|k| k.handle == handle) {
            let k = s.keyboards.remove(i);
            release_all(&k, &mut codes);
        }
        if let Some(i) = s.mice.iter().position(|&m| m == handle) {
            s.mice.remove(i);
            if s.mice.is_empty() {
                s.mouse.buttons = 0;
            }
        }
    }
    codes.feed();
}

fn release_all(k: &Keyboard, codes: &mut Codes) {
    let ctrl = k.mods & 0x11 != 0;
    for &u in k.keys.iter().filter(|&&u| u > 3) {
        codes.usage(u, true, ctrl);
    }
    for (bit, &sc) in MODIFIERS.iter().enumerate() {
        if k.mods & (1 << bit) != 0 {
            codes.key(sc, true);
        }
    }
}

/// An interrupt-IN report arrived from device `handle`.
pub fn report(handle: u32, data: &[u8]) {
    let now = crate::irq::uptime_ms();
    let mut codes = Codes::new();
    {
        let mut s = STATE.lock();
        if let Some(k) = s.keyboards.iter_mut().find(|k| k.handle == handle) {
            keyboard_report(k, data, now, &mut codes);
        } else if s.mice.contains(&handle) {
            mouse_report(&mut s, data, now);
        }
    }
    codes.feed();
}

fn keyboard_report(k: &mut Keyboard, data: &[u8], now: u64, codes: &mut Codes) {
    if data.len() < 3 {
        return;
    }
    let mut keys = [0u8; 6];
    let n = (data.len() - 2).min(6);
    keys[..n].copy_from_slice(&data[2..2 + n]);
    // Phantom state (too many keys): keep the previous state.
    if keys.iter().any(|&u| (1..=3).contains(&u)) {
        return;
    }
    let mods = data[0];
    // Modifier presses first, so Shift+A in one report shifts the A.
    for (bit, &sc) in MODIFIERS.iter().enumerate() {
        if mods & !k.mods & (1 << bit) != 0 {
            codes.key(sc, false);
        }
    }
    let ctrl = mods & 0x11 != 0;
    for &u in k.keys.iter().filter(|&&u| u != 0 && !keys.contains(&u)) {
        codes.usage(u, true, ctrl);
        if k.repeat.is_some_and(|(r, _)| r == u) {
            k.repeat = None;
        }
    }
    for &u in keys.iter().filter(|&&u| u != 0 && !k.keys.contains(&u)) {
        codes.usage(u, false, ctrl);
        k.repeat = repeats(u).then_some((u, now + REPEAT_DELAY_MS));
    }
    for (bit, &sc) in MODIFIERS.iter().enumerate() {
        if k.mods & !mods & (1 << bit) != 0 {
            codes.key(sc, true);
        }
    }
    k.mods = mods;
    k.keys = keys;
}

fn mouse_report(s: &mut State, data: &[u8], now: u64) {
    if data.len() < 3 {
        return;
    }
    let ev = MouseEvent {
        buttons: data[0] & 7,
        dx: data[1] as i8 as i32,
        dy: data[2] as i8 as i32,
        wheel: data.get(3).map_or(0, |&w| w as i8 as i32),
        time: now as u32,
    };
    s.mouse.buttons = ev.buttons;
    s.mouse.dx += ev.dx;
    s.mouse.dy += ev.dy;
    s.mouse.wheel += ev.wheel;
    if s.events.len() >= MOUSE_QUEUE {
        s.events.pop_front();
    }
    s.events.push_back(ev);
    MOUSE_WAITERS.wake_all();
    // USB y grows downwards, as the mouse layer expects.
    crate::mouse::motion(ev.buttons, ev.dx, ev.dy);
}

/// Typematic repeat; called from the USB poll.
pub fn tick() {
    let now = crate::irq::uptime_ms();
    let mut codes = Codes::new();
    {
        let mut s = STATE.lock();
        for k in s.keyboards.iter_mut() {
            if let Some((u, at)) = k.repeat {
                if now >= at {
                    codes.usage(u, false, k.mods & 0x11 != 0);
                    k.repeat = Some((u, now + REPEAT_MS));
                }
            }
        }
    }
    codes.feed();
}

/// LED byte for SET_REPORT (bit 0 NumLock, 1 CapsLock, 2 ScrollLock),
/// following the console's lock state.
pub fn leds() -> u8 {
    use crate::kbd::shift::*;
    let s = crate::kbd::shift_state();
    (s & NUMLOCK_ON != 0) as u8 | ((s & CAPSLOCK_ON != 0) as u8) << 1 | ((s & SCROLLLOCK_ON != 0) as u8) << 2
}

/// Whether a USB mouse is attached.
pub fn mouse_present() -> bool {
    !STATE.lock().mice.is_empty()
}

/// Buttons and the motion since the previous call (which is reset).
pub fn mouse_take() -> MouseState {
    let mut s = STATE.lock();
    let m = s.mouse;
    s.mouse.dx = 0;
    s.mouse.dy = 0;
    s.mouse.wheel = 0;
    m
}

/// Next queued mouse report, waiting if `wait`.
pub fn mouse_read(wait: bool) -> Option<MouseEvent> {
    loop {
        if let Some(e) = STATE.lock().events.pop_front() {
            return Some(e);
        }
        if !wait || !MOUSE_WAITERS.wait_until(None, || !STATE.lock().events.is_empty()) {
            return None;
        }
    }
}

pub fn mouse_flush() {
    STATE.lock().events.clear();
}
