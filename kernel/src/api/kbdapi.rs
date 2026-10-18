//! KBDCALLS: the keyboard API.

use alloc::vec::Vec;

use super::{err, Ctx};
use crate::kbd::{self, KeyInfo};
use crate::sync::SpinLock;

/// KBDINFO for the session: fsMask, chTurnAround, fsInterim.
struct Status {
    mask: u16,
    turnaround: u16,
    interim: u16,
}

static STATUS: SpinLock<Status> = SpinLock::new(Status { mask: 0x0009, turnaround: 0x0d, interim: 0 });

const ECHO_ON: u16 = 0x0001;
const ECHO_OFF: u16 = 0x0002;
const BINARY: u16 = 0x0004;
const ASCII: u16 = 0x0008;
const SHIFT_STATE: u16 = 0x0010;

fn key_bytes(k: &KeyInfo) -> [u8; 10] {
    let mut b = [0u8; 10];
    b[0] = k.ch;
    b[1] = k.scan;
    b[2] = k.status;
    b[3] = 0;
    b[4..6].copy_from_slice(&k.shift.to_le_bytes());
    b[6..10].copy_from_slice(&k.time.to_le_bytes());
    b
}

pub fn kbd_char_in(c: &mut Ctx) -> u32 {
    let (p, nowait) = (c.ptr(0), c.arg(1) != 0);
    let k = kbd::get_key(!nowait).unwrap_or(KeyInfo { status: 0, shift: kbd::shift_state(), ..Default::default() });
    if c.write(p, &key_bytes(&k)) { 0 } else { err::INVALID_ADDRESS }
}

pub fn kbd_peek(c: &mut Ctx) -> u32 {
    let k = kbd::peek_key().unwrap_or(KeyInfo { status: 0, shift: kbd::shift_state(), ..Default::default() });
    if c.write(c.ptr(0), &key_bytes(&k)) { 0 } else { err::INVALID_ADDRESS }
}

pub fn kbd_string_in(c: &mut Ctx) -> u32 {
    // KbdStringIn(pch, pchIn {cb, cchIn}, fsWait, hkbd)
    let (p, pin, nowait) = (c.ptr(0), c.ptr(1), c.arg(2) != 0);
    let Some(cb) = c.read_u16(pin) else { return err::INVALID_ADDRESS };
    let (mask, turnaround) = {
        let s = STATUS.lock();
        (s.mask, s.turnaround)
    };
    let mut out: Vec<u8> = Vec::new();
    if mask & BINARY != 0 {
        while out.len() < cb as usize {
            let Some(k) = kbd::get_key(!nowait) else { break };
            out.push(k.ch);
        }
    } else {
        let echo = mask & ECHO_OFF == 0;
        loop {
            let Some(k) = kbd::get_key(true) else { break };
            match k.ch {
                ch if ch as u16 == turnaround => break,
                8 => {
                    if out.pop().is_some() && echo {
                        crate::console::write(b"\x08 \x08");
                    }
                }
                0 | 0xe0 => {}
                ch => {
                    if out.len() + 1 < cb as usize {
                        out.push(ch);
                        if echo {
                            crate::console::write(&[ch]);
                        }
                    }
                }
            }
        }
        if echo {
            crate::console::write(b"\r");
        }
    }
    c.write(p, &out);
    c.write_u16(pin + 2, out.len() as u16);
    0
}

pub fn kbd_flush_buffer(_c: &mut Ctx) -> u32 {
    kbd::flush();
    0
}

pub fn kbd_get_status(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let s = STATUS.lock();
    let mut b = Vec::new();
    for w in [10u16, s.mask, s.turnaround, s.interim, kbd::shift_state()] {
        b.extend_from_slice(&w.to_le_bytes());
    }
    drop(s);
    if c.write(p, &b) { 0 } else { err::INVALID_ADDRESS }
}

pub fn kbd_set_status(c: &mut Ctx) -> u32 {
    let Some(raw) = c.read_vec(c.ptr(0), 10) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
    let mask = w(1);
    let mut s = STATUS.lock();
    if mask & (ECHO_ON | ECHO_OFF) != 0 {
        s.mask = (s.mask & !(ECHO_ON | ECHO_OFF)) | (mask & (ECHO_ON | ECHO_OFF));
    }
    if mask & (BINARY | ASCII) != 0 {
        s.mask = (s.mask & !(BINARY | ASCII)) | (mask & (BINARY | ASCII));
    }
    if mask & 0x0040 != 0 {
        s.turnaround = w(2);
    }
    if mask & 0x0020 != 0 {
        s.interim = w(3);
    }
    let _ = SHIFT_STATE;
    0
}

pub fn kbd_get_cp(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(1), 850);
    0
}

pub fn kbd_open(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), 1);
    0
}

pub fn kbd_get_hwid(c: &mut Ctx) -> u32 {
    // KBDHWID: cb, idKbd (0xAB41 = enhanced 101/102), usReserved1, usReserved2
    let p = c.ptr(0);
    let Some(cb) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    let vals = [0xab41u16, 0, 0];
    for (i, v) in vals.iter().enumerate() {
        if 2 + i * 2 + 2 <= cb as usize {
            c.write_u16(p + 2 + i as u32 * 2, *v);
        }
    }
    0
}

pub fn ok(_c: &mut Ctx) -> u32 {
    0
}
