//! Keyboard: scancode set 1 to OS/2 key events.
//!
//! The PS/2 controller delivers set-1 scancodes on IRQ 1; USB keyboards
//! (`drivers::hid`) translate their reports into the same scancodes and
//! feed them through [`scancode`], so shift state and key translation live
//! here only. Character assignment follows the selected national layout
//! ([`layout`]).

pub mod layout;
mod layouts;

use alloc::collections::{BTreeMap, VecDeque};

use crate::arch::idt::TrapFrame;
use crate::arch::inb;
use crate::sync::SpinLock;
use crate::task::WaitQueue;

/// `KBDKEYINFO` without the status byte bookkeeping.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyInfo {
    pub ch: u8,
    pub scan: u8,
    /// `fbStatus`: 0x40 = final character, 0x02 = extended (0xE0) key.
    pub status: u8,
    pub shift: u16,
    pub time: u32,
}

pub mod shift {
    pub const RIGHTSHIFT: u16 = 0x0001;
    pub const LEFTSHIFT: u16 = 0x0002;
    pub const CONTROL: u16 = 0x0004;
    pub const ALT: u16 = 0x0008;
    pub const SCROLLLOCK_ON: u16 = 0x0010;
    pub const NUMLOCK_ON: u16 = 0x0020;
    pub const CAPSLOCK_ON: u16 = 0x0040;
    pub const INSERT_ON: u16 = 0x0080;
    pub const LEFTCONTROL: u16 = 0x0100;
    pub const LEFTALT: u16 = 0x0200;
    pub const RIGHTCONTROL: u16 = 0x0400;
    pub const RIGHTALT: u16 = 0x0800;
}

struct State {
    /// Key events by session.
    queues: BTreeMap<u32, VecDeque<KeyInfo>>,
    shift: u16,
    e0: bool,
    /// A second character produced by the last key (a dead key that did
    /// not combine).
    extra: Option<KeyInfo>,
}

static STATE: SpinLock<State> = SpinLock::new(State { queues: BTreeMap::new(), shift: 0, e0: false, extra: None });
static WAITERS: WaitQueue = WaitQueue::new();

// Keys outside the layout tables (Esc, Backspace, Tab, Enter, keypad *,
// Space): scancode set 1, unshifted and shifted. The rest is the US layout
// used when a key is not in the layout.
const NORMAL: &[u8; 0x3a] = b"\x00\x1b1234567890-=\x08\tqwertyuiop[]\r\x00asdfghjkl;'`\x00\\zxcvbnm,./\x00*\x00 ";
const SHIFTED: &[u8; 0x3a] = b"\x00\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\x00ASDFGHJKL:\"~\x00|ZXCVBNM<>?\x00*\x00 ";

pub fn init() {
    // Drain anything pending, then take IRQ 1.
    unsafe {
        while inb(0x64) & 1 != 0 {
            inb(0x60);
        }
    }
    crate::irq::register(1, irq);
}

fn irq(_f: &mut TrapFrame) {
    scancode(unsafe { inb(0x60) });
}

/// Process one set-1 scancode byte (0xE0 prefixes included). Safe to call
/// from interrupt context (the PS/2 IRQ, the USB poll in the timer tick).
pub fn scancode(sc: u8) {
    if crate::conkbd::raw_byte(sc) {
        return;
    }
    let (ev, extra, before, after) = {
        let mut s = STATE.lock();
        let before = s.shift;
        let ev = translate(&mut s, sc);
        (ev, s.extra.take(), before, s.shift)
    };
    crate::conkbd::key_event(sc, before, after, ev);
    // A dead key that did not combine also yields the following character.
    if extra.is_some() {
        crate::conkbd::key_event(sc, after, after, extra);
    }
}

/// Queue a key event for session `sid` (after hot keys and monitors).
pub fn push(sid: u32, k: KeyInfo) {
    STATE.lock().queues.entry(sid).or_default().push_back(k);
    WAITERS.wake_all();
}

/// Put a key event back at the front of the queue of session `sid`.
pub fn unget(sid: u32, k: KeyInfo) {
    STATE.lock().queues.entry(sid).or_default().push_front(k);
    WAITERS.wake_all();
}

/// Translate a scan code in a given shift state without side effects
/// (KbdXlate); `ext` for E0-prefixed keys.
pub fn xlate(scan: u8, shift: u16, ext: bool) -> Option<KeyInfo> {
    let mut s = State { queues: BTreeMap::new(), shift, e0: ext, extra: None };
    translate(&mut s, scan & 0x7f)
}

/// Set the shift and lock state (KbdSetStatus, KBD_SETSHIFTSTATE).
pub fn set_shift_state(v: u16) {
    STATE.lock().shift = v;
}

fn translate(s: &mut State, sc: u8) -> Option<KeyInfo> {
    use shift::*;
    if sc == 0xe0 {
        s.e0 = true;
        return None;
    }
    let ext = core::mem::take(&mut s.e0);
    let release = sc & 0x80 != 0;
    let code = sc & 0x7f;
    // Modifier keys.
    let m = match (code, ext) {
        (0x2a, false) => LEFTSHIFT,
        (0x36, false) => RIGHTSHIFT,
        (0x1d, false) => LEFTCONTROL | CONTROL,
        (0x1d, true) => RIGHTCONTROL | CONTROL,
        (0x38, false) => LEFTALT | ALT,
        (0x38, true) => RIGHTALT | ALT,
        _ => 0,
    };
    if m != 0 {
        if release {
            s.shift &= !m;
            // Keep the combined bit while the other side is still down.
            if s.shift & (LEFTCONTROL | RIGHTCONTROL) != 0 {
                s.shift |= CONTROL;
            }
            if s.shift & (LEFTALT | RIGHTALT) != 0 {
                s.shift |= ALT;
            }
        } else {
            s.shift |= m;
        }
        return None;
    }
    if release {
        return None;
    }
    match (code, ext) {
        (0x3a, false) => {
            s.shift ^= CAPSLOCK_ON;
            return None;
        }
        (0x45, false) => {
            s.shift ^= NUMLOCK_ON;
            return None;
        }
        (0x46, false) => {
            s.shift ^= SCROLLLOCK_ON;
            return None;
        }
        _ => {}
    }
    let time = crate::irq::uptime_ms() as u32;
    let shifted = s.shift & (LEFTSHIFT | RIGHTSHIFT) != 0;
    let mut k = KeyInfo { ch: 0, scan: code, status: 0x40, shift: s.shift, time };
    if ext {
        // Grey cursor-pad keys report 0xE0 as the character.
        k.ch = match code {
            0x1c => b'\r',
            0x35 => b'/',
            _ => 0xe0,
        };
        k.status |= 0x02;
        return Some(k);
    }
    match layout::key(code, s.shift) {
        layout::Key::Other => {}
        layout::Key::Nothing => return None,
        layout::Key::Chars(ch, second) => {
            k.ch = ch;
            if let Some(b) = second {
                s.extra = Some(KeyInfo { ch: b, ..k });
            }
            return Some(k);
        }
    }
    if (code as usize) < NORMAL.len() {
        let mut c = if shifted { SHIFTED[code as usize] } else { NORMAL[code as usize] };
        if s.shift & CAPSLOCK_ON != 0 && c.is_ascii_alphabetic() {
            c ^= 0x20;
        }
        if s.shift & CONTROL != 0 && c.is_ascii_alphabetic() {
            c &= 0x1f;
        }
        if s.shift & ALT != 0 && c != 0 {
            // Alt+key: character 0, scan code identifies the key.
            c = 0;
        }
        k.ch = c;
        return Some(k);
    }
    // Keypad with NumLock, function keys and the rest: scan code only.
    if (0x47..=0x53).contains(&code) && s.shift & NUMLOCK_ON != 0 && !shifted {
        const PAD: &[u8; 13] = b"789-456+1230.";
        k.ch = if code == 0x53 { layout::keypad_decimal() } else { PAD[(code - 0x47) as usize] };
    }
    Some(k)
}

fn pending(sid: u32) -> bool {
    STATE.lock().queues.get(&sid).is_some_and(|q| !q.is_empty())
}

/// Next key event for the calling thread's session, waiting if `wait`.
pub fn get_key(wait: bool) -> Option<KeyInfo> {
    get_key_from(crate::session::io_session(), wait)
}

/// Next key event of session `sid`, waiting if `wait`.
pub fn get_key_from(sid: u32, wait: bool) -> Option<KeyInfo> {
    loop {
        if let Some(k) = STATE.lock().queues.get_mut(&sid).and_then(|q| q.pop_front()) {
            return Some(k);
        }
        if !wait {
            return None;
        }
        if !WAITERS.wait_until(None, || pending(sid)) {
            // Interrupted by a signal or termination.
            return None;
        }
    }
}

pub fn peek_key() -> Option<KeyInfo> {
    let sid = crate::session::io_session();
    STATE.lock().queues.get(&sid).and_then(|q| q.front().copied())
}

pub fn flush() {
    let sid = crate::session::io_session();
    STATE.lock().queues.remove(&sid);
    crate::conkbd::flush(sid);
}

/// A session ended.
pub fn drop_queue(sid: u32) {
    STATE.lock().queues.remove(&sid);
}

pub fn shift_state() -> u16 {
    STATE.lock().shift
}

/// The shift state, or 0 if the keyboard state is busy (for interrupt
/// handlers).
pub fn shift_state_now() -> u16 {
    STATE.try_lock().map_or(0, |s| s.shift)
}
