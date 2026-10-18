//! Keyboard: scancode set 1 to OS/2 key events.
//!
//! The PS/2 controller delivers set-1 scancodes on IRQ 1; USB keyboards
//! (`drivers::hid`) translate their reports into the same scancodes and
//! feed them through [`scancode`], so shift state and key translation live
//! here only.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

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
    queue: VecDeque<KeyInfo>,
    shift: u16,
    e0: bool,
}

static STATE: SpinLock<State> = SpinLock::new(State { queue: VecDeque::new(), shift: 0, e0: false });
static WAITERS: WaitQueue = WaitQueue::new();

// US layout, scancode set 1, unshifted and shifted.
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
    let ev = {
        let mut s = STATE.lock();
        translate(&mut s, sc)
    };
    if let Some(k) = ev {
        let ctrl = k.shift & shift::CONTROL != 0;
        if k.ch == 3 && ctrl {
            crate::api::signal::keyboard(crate::api::signal::SIG_CTRLC);
            return;
        }
        if ctrl && k.scan == 0x46 {
            crate::api::signal::keyboard(crate::api::signal::SIG_CTRLBREAK);
            return;
        }
        STATE.lock().queue.push_back(k);
        WAITERS.wake_all();
    }
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
        k.ch = PAD[(code - 0x47) as usize];
    }
    Some(k)
}

/// Next key event, waiting if `wait`.
pub fn get_key(wait: bool) -> Option<KeyInfo> {
    loop {
        if let Some(k) = STATE.lock().queue.pop_front() {
            return Some(k);
        }
        if !wait {
            return None;
        }
        if !WAITERS.wait_until(None, || !STATE.lock().queue.is_empty()) {
            // Interrupted by a signal or termination.
            return None;
        }
    }
}

pub fn peek_key() -> Option<KeyInfo> {
    STATE.lock().queue.front().copied()
}

pub fn flush() {
    STATE.lock().queue.clear();
    COOKED.lock().clear();
}

/// The part of an edited CON input line not yet consumed by DosRead.
static COOKED: SpinLock<VecDeque<u8>> = SpinLock::new(VecDeque::new());

/// DosRead from CON in cooked mode: hands out an edited line, keeping what
/// does not fit in the caller's buffer for the next read (programs read
/// one byte at a time).
pub fn read_cooked(len: usize) -> Vec<u8> {
    if COOKED.lock().is_empty() {
        let line = read_line(256);
        COOKED.lock().extend(line);
    }
    let mut q = COOKED.lock();
    let n = len.min(q.len());
    q.drain(..n).collect()
}

pub fn shift_state() -> u16 {
    STATE.lock().shift
}

/// Read an edited line from the keyboard with echo (CON input). The result
/// ends with CR LF, as OS/2 returns it.
pub fn read_line(max: usize) -> Vec<u8> {
    let mut line: Vec<u8> = Vec::new();
    loop {
        let Some(k) = get_key(true) else {
            // Interrupted: return what we have.
            return line;
        };
        match k.ch {
            b'\r' => {
                crate::console::write(b"\r\n");
                line.extend_from_slice(b"\r\n");
                return line;
            }
            8 => {
                if line.pop().is_some() {
                    crate::console::write(b"\x08 \x08");
                }
            }
            0x1b => {
                for _ in 0..line.len() {
                    crate::console::write(b"\x08 \x08");
                }
                line.clear();
            }
            0 | 0xe0 => {}
            c => {
                if line.len() + 2 < max.max(3) {
                    line.push(c);
                    crate::console::write(&[c]);
                }
            }
        }
    }
}
