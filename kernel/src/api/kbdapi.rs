//! KBDCALLS: the keyboard API.
//!
//! Every session has a default logical keyboard (handle 0) and those
//! created with KbdOpen, each with its own KBDINFO (echo, ASCII/binary
//! mode, turnaround character, shift reporting). They share the session's
//! key queue; a logical keyboard other than the default must hold the focus
//! (KbdGetFocus) to read. The state lives in `crate::conkbd`.

use alloc::vec::Vec;

use super::{err, Ctx};
use crate::conkbd::{self, mask, Info};
use crate::kbd::{self, KeyInfo};

const ERROR_KBD_INVALID_IOWAIT: u32 = 375;
const ERROR_KBD_INVALID_LENGTH: u32 = 376;
const ERROR_KBD_INVALID_ECHO_MASK: u32 = 377;
const ERROR_KBD_INVALID_INPUT_MASK: u32 = 378;
const ERROR_KBD_INVALID_CODEPAGE: u32 = 448;

fn sid() -> u32 {
    crate::session::io_session()
}

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

fn no_key() -> KeyInfo {
    KeyInfo { status: 0, shift: kbd::shift_state(), ..Default::default() }
}

/// KbdCharIn(pkbci, fWait, hkbd).
pub fn kbd_char_in(c: &mut Ctx) -> u32 {
    let (p, wait, h) = (c.ptr(0), c.arg(1), c.arg(2) as u16);
    if wait > 1 {
        return ERROR_KBD_INVALID_IOWAIT;
    }
    if let Err(e) = conkbd::may_read(sid(), h) {
        return e;
    }
    // Interrupted by a signal: an empty record, as with IO_NOWAIT.
    let k = kbd::get_key(wait == 0).unwrap_or_else(no_key);
    if c.write(p, &key_bytes(&k)) { 0 } else { err::INVALID_ADDRESS }
}

/// KbdPeek(pkbci, hkbd): fbStatus 0 when no key is waiting.
pub fn kbd_peek(c: &mut Ctx) -> u32 {
    let (p, h) = (c.ptr(0), c.arg(1) as u16);
    if let Err(e) = conkbd::may_read(sid(), h) {
        return e;
    }
    let k = kbd::peek_key().unwrap_or_else(no_key);
    if c.write(p, &key_bytes(&k)) { 0 } else { err::INVALID_ADDRESS }
}

/// KbdStringIn(pch, pchIn {cb, cchIn}, fsWait, hkbd). ASCII mode: an
/// edited line (echoed if ECHO_ON), ended by the turnaround character,
/// which is stored after the text but not counted. Binary mode: cb
/// characters as typed (with IO_NOWAIT only those already waiting).
pub fn kbd_string_in(c: &mut Ctx) -> u32 {
    let (p, pin, wait, h) = (c.ptr(0), c.ptr(1), c.arg(2), c.arg(3) as u16);
    if wait > 1 {
        return ERROR_KBD_INVALID_IOWAIT;
    }
    let Some(cb) = c.read_u16(pin) else { return err::INVALID_ADDRESS };
    if cb == 0 || cb > 255 {
        return ERROR_KBD_INVALID_LENGTH;
    }
    let s = sid();
    if let Err(e) = conkbd::may_read(s, h) {
        return e;
    }
    let info = conkbd::info(s, h).unwrap_or(Info { mask: mask::ECHO_ON | mask::ASCII, turnaround: 0x0d, interim: 0 });
    let mut out: Vec<u8> = Vec::new();
    if info.mask & mask::BINARY != 0 {
        while out.len() < cb as usize {
            let Some(k) = kbd::get_key_from(s, wait == 0) else { break };
            out.push(k.ch);
        }
        if out.is_empty() && wait == 0 && c.thread.interrupted() {
            return err::INTERRUPT;
        }
        c.write(p, &out);
        c.write_u16(pin + 2, out.len() as u16);
        return 0;
    }
    let echo = info.mask & mask::ECHO_OFF == 0;
    let turnaround = info.turnaround as u8;
    let Some(mut line) = conkbd::read_line(s, cb as usize - 1, echo, turnaround) else { return err::INTERRUPT };
    if echo {
        crate::console::write(b"\r");
    }
    let n = line.len();
    // The turnaround character follows the text but is not counted.
    if n < cb as usize {
        line.push(turnaround);
    }
    c.write(p, &line);
    c.write_u16(pin + 2, n as u16);
    0
}

pub fn kbd_flush_buffer(c: &mut Ctx) -> u32 {
    if let Err(e) = conkbd::info(sid(), c.arg(0) as u16) {
        return e;
    }
    kbd::flush();
    0
}

/// KbdGetStatus(KBDINFO {cb, fsMask, chTurnAround, fsInterim, fsState}).
pub fn kbd_get_status(c: &mut Ctx) -> u32 {
    let (p, h) = (c.ptr(0), c.arg(1) as u16);
    let Some(cb) = c.read_u16(p) else { return err::INVALID_ADDRESS };
    if cb < 10 {
        return ERROR_KBD_INVALID_LENGTH;
    }
    let i = match conkbd::info(sid(), h) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let mut b = Vec::new();
    for w in [10u16, i.mask, i.turnaround, i.interim, kbd::shift_state()] {
        b.extend_from_slice(&w.to_le_bytes());
    }
    if c.write(p, &b) { 0 } else { err::INVALID_ADDRESS }
}

/// KbdSetStatus(KBDINFO): fsMask selects what changes.
pub fn kbd_set_status(c: &mut Ctx) -> u32 {
    let (p, h) = (c.ptr(0), c.arg(1) as u16);
    let Some(raw) = c.read_vec(p, 10) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
    if w(0) < 10 {
        return ERROR_KBD_INVALID_LENGTH;
    }
    let m = w(1);
    if m & mask::ECHO_ON != 0 && m & mask::ECHO_OFF != 0 {
        return ERROR_KBD_INVALID_ECHO_MASK;
    }
    if m & mask::BINARY != 0 && m & mask::ASCII != 0 {
        return ERROR_KBD_INVALID_INPUT_MASK;
    }
    if m & 0xfe00 != 0 {
        return err::INVALID_PARAMETER;
    }
    let s = sid();
    let mut i = match conkbd::info(s, h) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let echo = mask::ECHO_ON | mask::ECHO_OFF;
    let input = mask::BINARY | mask::ASCII;
    // Echo cannot be turned on while the keyboard is in binary mode; a
    // call that switches to binary mode is judged by the mode it starts
    // from (IBM's TELNET sets binary, echo and shift reports at once).
    let was_binary = i.mask & mask::BINARY != 0;
    if m & echo != 0 {
        i.mask = (i.mask & !echo) | (m & echo);
    }
    if m & input != 0 {
        i.mask = (i.mask & !input) | (m & input);
        i.mask = (i.mask & !mask::SHIFT_REPORT) | (m & mask::SHIFT_REPORT);
    } else if m & mask::SHIFT_REPORT != 0 {
        i.mask |= mask::SHIFT_REPORT;
    }
    if was_binary && m & mask::ECHO_ON != 0 && m & mask::ASCII == 0 {
        return ERROR_KBD_INVALID_ECHO_MASK;
    }
    if m & mask::MODIFY_TURNAROUND != 0 {
        i.turnaround = if m & mask::TWO_BYTE_TURNAROUND != 0 { w(2) } else { w(2) & 0xff };
    }
    if m & mask::MODIFY_INTERIM != 0 {
        i.interim = w(3);
    }
    if m & mask::MODIFY_STATE != 0 {
        conkbd::set_shift(w(4));
    }
    match conkbd::set_info(s, h, i) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// KbdGetCp(ulReserved, pidCP, hkbd).
pub fn kbd_get_cp(c: &mut Ctx) -> u32 {
    let mut cp = conkbd::codepage(sid());
    if cp == 0 {
        cp = crate::screen::with(|s| s.codepage().number()).unwrap_or(850);
    }
    c.write_u16(c.ptr(1), cp);
    0
}

/// KbdSetCp(usReserved, idCP, hkbd): 0 selects the default.
pub fn kbd_set_cp(c: &mut Ctx) -> u32 {
    let cp = c.arg(1) as u16;
    if cp != 0 && crate::codepage::CodePage::from_number(cp).is_none() {
        return ERROR_KBD_INVALID_CODEPAGE;
    }
    conkbd::set_codepage(sid(), cp);
    0
}

pub fn kbd_open(c: &mut Ctx) -> u32 {
    let h = conkbd::open(sid(), c.process.pid);
    c.write_u16(c.ptr(0), h);
    0
}

pub fn kbd_close(c: &mut Ctx) -> u32 {
    match conkbd::close(sid(), c.arg(0) as u16) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// KbdGetFocus(fWait, hkbd).
pub fn kbd_get_focus(c: &mut Ctx) -> u32 {
    let (wait, h) = (c.arg(0), c.arg(1) as u16);
    if wait > 1 {
        return ERROR_KBD_INVALID_IOWAIT;
    }
    match conkbd::get_focus(sid(), h, wait == 0) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

pub fn kbd_free_focus(c: &mut Ctx) -> u32 {
    match conkbd::free_focus(sid(), c.arg(0) as u16) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// KbdXlate(KBDTRANS, hkbd): the character for chScan in the shift state
/// fsState (fsDD 0x80 = E0-prefixed key); fsXlate 1 = complete.
pub fn kbd_xlate(c: &mut Ctx) -> u32 {
    let p = c.ptr(0);
    let Some(raw) = c.read_vec(p, 18) else { return err::INVALID_ADDRESS };
    let scan = raw[1];
    let state = u16::from_le_bytes([raw[4], raw[5]]);
    let dd = u16::from_le_bytes([raw[10], raw[11]]);
    let k = kbd::xlate(scan, state, dd & 0x80 != 0);
    let (ch, status) = match k {
        Some(k) => (k.ch, k.status),
        None => (0, 0x01),
    };
    c.write_u8(p, ch);
    c.write_u8(p + 2, status);
    c.write_u16(p + 12, 1);
    0
}

/// KbdSetCustXt(pTable, hkbd): a custom translation table (accepted; the
/// layout tables are the keyboard layer's).
pub fn kbd_set_cust_xt(c: &mut Ctx) -> u32 {
    if c.ptr(0) == 0 {
        return err::INVALID_ADDRESS;
    }
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
