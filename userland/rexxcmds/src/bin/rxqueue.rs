//! RXQUEUE: copies standard input, line by line, into a REXX data queue.
//!
//! `RXQUEUE [name] [/FIFO | /LIFO | /CLEAR]` (a `-` may stand for the `/`).
//! Without a name the queue is %RXQUEUE%, else SESSION; the queue must
//! exist (REXX's RXQUEUE('Create') makes named ones). /FIFO (the default)
//! queues lines at the end, /LIFO at the front, /CLEAR empties the queue
//! and reads nothing. As IBM's: a line ends at CR, LF or CR LF, lines are
//! cut at 4096 characters, a Ctrl+Z ends the input (the text before it on
//! its line is dropped; a Ctrl+Z starting a line queues one empty line).
//! Errors are REX.MSG/OSO001.MSG messages on standard error, and the
//! return code stays 0 as IBM's (it is -6 when the message can't be read).

#![no_std]
#![no_main]

extern crate alloc;

use rexxcmds::*;

os2::entry!(main);

const RXQUEUE_FIFO: u32 = 0;
const RXQUEUE_LIFO: u32 = 1;
/// /CLEAR (not an RexxAddQueue flag).
const CLEAR: u32 = 0xffff_fffe;

const LINE_MAX: usize = 0x1000;

/// Standard input read a character at a time, as IBM's does (so that the
/// program writing the pipe and RXQUEUE stay in step).
struct Input {
    /// The character read after a CR that did not turn out to be a LF.
    held: Option<u8>,
    eof: bool,
}

impl Input {
    fn byte(&mut self) -> Option<u8> {
        let mut c = 0u8;
        let mut n = 0;
        let rc = unsafe { os2::DosRead(0, &mut c, 1, &mut n) };
        if rc != 0 || n == 0 { None } else { Some(c) }
    }

    /// The next line into `buf`; None at the end of the input.
    fn line(&mut self, buf: &mut alloc::vec::Vec<u8>) -> Option<()> {
        if self.eof {
            return None;
        }
        buf.clear();
        if let Some(c) = self.held.take() {
            buf.push(c);
        }
        loop {
            let Some(c) = self.byte() else {
                if buf.is_empty() {
                    return None;
                }
                self.eof = true;
                return Some(());
            };
            match c {
                b'\r' => {
                    if let Some(c) = self.byte() {
                        if c != b'\n' {
                            self.held = Some(c);
                        }
                    }
                    return Some(());
                }
                b'\n' => return Some(()),
                0x1a => {
                    self.eof = true;
                    return if buf.is_empty() { Some(()) } else { None };
                }
                _ if buf.len() < LINE_MAX => buf.push(c),
                _ => {}
            }
        }
    }
}

/// Report an error (a REXXAPI return code, or 0 for bad options) and end.
fn fail(rc: u32, name: *const u8) -> ! {
    let ins = if name.is_null() { b"\0".as_ptr() } else { name };
    let ok = match rc {
        0 => message(1003, b"OSO001.MSG\0", None),
        2 => message(120, b"REX.MSG\0", None),
        4 => message(121, b"REX.MSG\0", None),
        5 => message(122, b"REX.MSG\0", Some(ins)),
        6 => message(123, b"REX.MSG\0", None),
        9 => message(124, b"REX.MSG\0", Some(ins)),
        1000 => message(119, b"REX.MSG\0", None),
        _ => message(87, b"OSO001.MSG\0", None),
    };
    unsafe { os2::DosExit(1, if ok { 0 } else { (-6i32) as u32 }) };
    loop {}
}

fn main(s: &os2::Startup) -> u32 {
    let argv = args(s);
    let mut mode = None;
    let mut name: *const u8 = core::ptr::null();
    for a in &argv {
        if a[0] == b'/' || a[0] == b'-' {
            let opt = &a[1..];
            let m = if is(opt, "FIFO") {
                RXQUEUE_FIFO
            } else if is(opt, "LIFO") {
                RXQUEUE_LIFO
            } else if is(opt, "CLEAR") {
                CLEAR
            } else {
                fail(0, name)
            };
            if mode.is_some() {
                fail(0, name);
            }
            mode = Some(m);
        } else if name.is_null() {
            name = a.as_ptr();
        } else {
            fail(0, name);
        }
    }
    let mode = mode.unwrap_or(RXQUEUE_FIFO);
    if name.is_null() {
        let mut v: *const u8 = core::ptr::null();
        let rc = unsafe { os2::DosScanEnv(b"RXQUEUE\0".as_ptr(), &mut v) };
        name = if rc != 0 || v.is_null() { b"SESSION\0".as_ptr() } else { v };
    }
    let mut count = 0;
    let rc = unsafe { RexxQueryQueue(name, &mut count) };
    if rc != 0 {
        fail(rc, name);
    }
    if mode == CLEAR {
        let mut dt = [0u8; 16];
        loop {
            let mut d = RxString { len: 0, ptr: core::ptr::null_mut() };
            if unsafe { RexxPullQueue(name, &mut d, dt.as_mut_ptr(), 0) } != 0 {
                break;
            }
            if d.len != 0 && !d.ptr.is_null() {
                unsafe { os2::DosFreeMem(d.ptr) };
            }
        }
    } else {
        let mut input = Input { held: None, eof: false };
        let mut buf = alloc::vec::Vec::with_capacity(LINE_MAX + 1);
        while input.line(&mut buf).is_some() {
            let d = RxString { len: buf.len() as u32, ptr: buf.as_mut_ptr() };
            let rc = unsafe { RexxAddQueue(name, &d, mode) };
            if rc != 0 {
                fail(rc, name);
            }
        }
    }
    0
}
