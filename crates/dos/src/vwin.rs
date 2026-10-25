//! VWIN, OS/2's Win-OS/2 virtual device driver (VWIN.SYS), as far as
//! Win-OS/2 needs it in rs2's DOS machine (studied in Ghidra with
//! vwin.sym, winsheld.sym and winsmsg.sym; docs/WINOS2.md). INT 66h's AH
//! selects a router table:
//!
//! | AH | VWIN |
//! |----|------|
//! | 01h-08h | WINSHIELD: add a user routine, send a message (02h waits for the reply, 03h not), make dirty, get the next message, delete the user routine, send an error, fix a lost child |
//! | 7Fh-8Ah | WINDD: did the display driver initialize, am I seamless (80h), the seamless display driver's calls |
//! | E0h-E5h | pipes for the clipboard and DDE: open, close, write, read, set blocking mode, pass data |
//!
//! The status comes back in AX (0 done, 2 no message, 7Ah the buffer is
//! too small). WINSHELD.EXE and WINSMSG.DLL call it through WINSMSG's
//! `_INT386` (the protected-mode vector of INT 66h with 32-bit
//! registers) and through the entry point AH=01h returns, which takes the
//! same registers.
//!
//! The messages between Windows and PM (the "shield" messages) are
//! 2Ch-byte records: a sequence number (VWIN numbers every message), the
//! session's id, a rectangle (top, left, height, width), two window tags
//! (session id, hwnd), flags and a code at +24h. Windows reports its
//! top-level windows with them (created 1, destroyed 2, activated 3,
//! moved, sized, shown, hidden, minimised 4, title 12h); PM sends the
//! same codes back to activate, close, place and start programs (16h).
//! This module keeps the window list the messages describe for the
//! seamless host and makes the host's requests into messages for
//! WINSHELD, calling its user routine as VWIN does when one is queued.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

use x86emu::{DS, EAX, EBX, ECX, EDI, EDX};

use crate::{Host, Machine};

/// Message codes (at +24h).
pub const MSG_CREATE: u16 = 1;
pub const MSG_DESTROY: u16 = 2;
pub const MSG_ACTIVATE: u16 = 3;
pub const MSG_POSITION: u16 = 4;
pub const MSG_INIT: u16 = 5;
pub const MSG_SHOW: u16 = 0x0a;
pub const MSG_TITLE: u16 = 0x12;
pub const MSG_RUN: u16 = 0x16;

/// Flags of a MSG_POSITION (at +18h).
pub const POS_TOP: u16 = 0x0001;
pub const POS_BOTTOM: u16 = 0x0002;
pub const POS_BEHIND: u16 = 0x0004;
pub const POS_SHOW: u16 = 0x0008;
pub const POS_HIDE: u16 = 0x0010;
pub const POS_RECT: u16 = 0x0020;
pub const POS_RESTORE: u16 = 0x0100;
pub const POS_MINIMIZED: u16 = 0x1000;
pub const POS_MAXIMIZED: u16 = 0x4000;

/// A top-level Windows window as the messages describe it.
#[derive(Clone, Debug, Default)]
pub struct Window {
    pub hwnd: u16,
    /// The window's rectangle on the Windows screen.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub style: u32,
    pub owner: u16,
    pub visible: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub title: Vec<u8>,
}

/// What the seamless host asks Windows to do.
#[derive(Clone, Copy, Debug)]
pub enum Request {
    Activate { hwnd: u16 },
    Close { hwnd: u16 },
    /// Move and size the whole window (Windows screen coordinates).
    Place { hwnd: u16, x: i16, y: i16, w: i16, h: i16 },
    Minimize { hwnd: u16 },
    Restore { hwnd: u16 },
    Maximize { hwnd: u16 },
    /// Start a program (a command line, NUL-terminated).
    Run([u8; 128]),
}

#[derive(Default)]
pub struct Vwin {
    /// This session is a seamless one (WINAMISEAMLESS says so).
    pub seamless: bool,
    /// The session's id in the messages.
    pub id: u32,
    /// WINSHELD's user routine (selector, offset) and its data segment.
    routine: Option<(u16, u16, u16)>,
    /// Messages for Windows, and whether the routine is due a call.
    inbox: VecDeque<Vec<u8>>,
    pub call_due: bool,
    pub in_call: bool,
    /// Other far calls into Windows to make (selector, offset, data
    /// segment, Pascal arguments): the notification routine of the
    /// clipboard and DDE pipe.
    calls: VecDeque<(u16, u16, u16, Vec<u16>)>,
    /// VWIN numbers every message.
    seq: u32,
    /// The top-level windows, bottom of the z-order first.
    pub windows: Vec<Window>,
    /// The active window (0: none).
    pub active: u16,
    /// Bumped by every change of `windows` or `active`.
    pub generation: u32,
    /// Log every call and message (debugging); lines made where no host
    /// is at hand wait here.
    pub trace: bool,
    pub log: Vec<alloc::string::String>,
}

fn rd16(m: &[u8], o: usize) -> u16 {
    m.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn rd32(m: &[u8], o: usize) -> u32 {
    m.get(o..o + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn cstr(m: &[u8], o: usize) -> Vec<u8> {
    let s = m.get(o..).unwrap_or(&[]);
    s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())].to_vec()
}

impl Vwin {
    fn window(&mut self, hwnd: u16) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.hwnd == hwnd)
    }

    /// Learn from a message Windows sends to PM.
    pub fn learn(&mut self, m: &[u8]) {
        let code = rd16(m, 0x24);
        let hwnd = rd16(m, 0x20);
        let (top, left, height, width) = (rd16(m, 8) as i16 as i32, rd16(m, 0x0a) as i16 as i32, rd16(m, 0x0c) as i16 as i32, rd16(m, 0x0e) as i16 as i32);
        let flags = rd16(m, 0x18);
        match code {
            MSG_CREATE if hwnd != 0 => {
                let style = rd32(m, 0x18);
                let title = if rd16(m, 0x12) != 0 { cstr(m, rd16(m, 0x12) as usize) } else { Vec::new() };
                let w = Window {
                    hwnd,
                    x: left,
                    y: top,
                    w: width,
                    h: height,
                    style,
                    owner: rd16(m, 0x28),
                    visible: style & 0x1000_0000 != 0,
                    minimized: style & 0x2000_0000 != 0,
                    maximized: style & 0x0100_0000 != 0,
                    title,
                };
                self.windows.retain(|o| o.hwnd != hwnd);
                self.windows.push(w);
            }
            MSG_DESTROY => {
                self.windows.retain(|o| o.hwnd != hwnd);
                if self.active == hwnd {
                    self.active = 0;
                }
            }
            MSG_ACTIVATE => self.active = hwnd,
            MSG_POSITION => {
                let Some(i) = self.windows.iter().position(|o| o.hwnd == hwnd) else { return };
                let w = &mut self.windows[i];
                if flags & POS_RECT != 0 {
                    (w.x, w.y, w.w, w.h) = (left, top, width, height);
                }
                if flags & POS_SHOW != 0 {
                    w.visible = true;
                }
                if flags & POS_HIDE != 0 {
                    w.visible = false;
                }
                if flags & POS_RECT != 0 {
                    w.minimized = flags & POS_MINIMIZED != 0;
                    w.maximized = flags & POS_MAXIMIZED != 0;
                }
                // Z-order: top, bottom or behind the window tagged at
                // +10h (session id, hwnd).
                if flags & (POS_TOP | POS_BOTTOM | POS_BEHIND) != 0 {
                    let w = self.windows.remove(i);
                    let pos = if flags & POS_TOP != 0 {
                        self.windows.len()
                    } else if flags & POS_BOTTOM != 0 {
                        0
                    } else {
                        let behind = rd16(m, 0x14);
                        self.windows.iter().position(|o| o.hwnd == behind).unwrap_or(self.windows.len())
                    };
                    self.windows.insert(pos, w);
                }
            }
            MSG_SHOW => {
                if let Some(w) = self.window(hwnd) {
                    w.visible = flags & 0x40 != 0;
                }
            }
            MSG_TITLE => {
                let t = cstr(m, 0x2c);
                if let Some(w) = self.window(hwnd) {
                    w.title = t;
                }
            }
            _ => return,
        }
        self.generation = self.generation.wrapping_add(1);
    }

    /// A message for Windows about `hwnd`.
    fn message(&mut self, code: u16, hwnd: u16, extra: usize) -> Vec<u8> {
        let mut m = vec![0u8; 0x2c + extra];
        self.seq = self.seq.wrapping_add(1);
        m[0..4].copy_from_slice(&self.seq.to_le_bytes());
        m[4..8].copy_from_slice(&self.id.to_le_bytes());
        m[0x1c..0x20].copy_from_slice(&self.id.to_le_bytes());
        m[0x20..0x22].copy_from_slice(&hwnd.to_le_bytes());
        m[0x24..0x26].copy_from_slice(&code.to_le_bytes());
        m
    }

    /// The next message for Windows (as WINSHELD reads it).
    pub fn next_message(&mut self) -> Option<Vec<u8>> {
        self.inbox.pop_front()
    }

    /// Queue a request of the host for Windows.
    pub fn request(&mut self, r: Request) {
        let put = |m: &mut Vec<u8>, o: usize, v: u16| m[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let m = match r {
            Request::Activate { hwnd } => self.message(MSG_ACTIVATE, hwnd, 0),
            Request::Close { hwnd } => self.message(MSG_DESTROY, hwnd, 0),
            Request::Place { hwnd, x, y, w, h } => {
                let mut m = self.message(MSG_POSITION, hwnd, 0);
                for (o, v) in [(8, y), (0x0a, x), (0x0c, h), (0x0e, w)] {
                    put(&mut m, o, v as u16);
                }
                put(&mut m, 0x18, POS_RECT);
                m
            }
            Request::Minimize { hwnd } => {
                let mut m = self.message(MSG_POSITION, hwnd, 0);
                put(&mut m, 0x18, POS_MINIMIZED);
                m
            }
            Request::Restore { hwnd } => {
                let mut m = self.message(MSG_POSITION, hwnd, 0);
                put(&mut m, 0x18, POS_RESTORE);
                m
            }
            Request::Maximize { hwnd } => {
                let mut m = self.message(MSG_POSITION, hwnd, 0);
                put(&mut m, 0x18, POS_MAXIMIZED);
                m
            }
            Request::Run(cmd) => {
                // The command line after the record, its offset at +8.
                let n = cmd.iter().position(|&c| c == 0).unwrap_or(cmd.len());
                let mut m = self.message(MSG_RUN, 0, n + 1);
                put(&mut m, 8, 0x2c);
                m[0x2c..0x2c + n].copy_from_slice(&cmd[..n]);
                m
            }
        };
        self.inbox.push_back(m);
        self.call_due = true;
    }
}

impl Machine {
    /// The registers of an INT 66h call or of a far call to the message
    /// entry point: the 16:16 pointer in EBX.
    fn vwin_buf(&mut self) -> u32 {
        let ebx = self.cpu.regs[EBX];
        if self.cpu.pm() { self.dpmi_lin((ebx >> 16) as u16, ebx & 0xffff) } else { ((ebx >> 16) << 4) + (ebx & 0xffff) }
    }

    /// INT 66h, in real or protected mode.
    pub(crate) fn vwin_int66<H: Host>(&mut self, host: &mut H) {
        let status = self.vwin_call(host);
        self.cpu.set_ax(status);
        self.set_ret_flag(x86emu::flags::CF, status != 0 && status != 2);
    }

    /// A far call to VWIN's message entry (the address AH=01h gave, or
    /// INT 2Fh AX=4011h for `VWIN`): the INT 66h functions with the same
    /// registers.
    pub(crate) fn vwin_entry<H: Host>(&mut self, host: &mut H) {
        let status = self.vwin_call(host);
        self.cpu.set_ax(status);
        if status != 0 && status != 2 {
            self.cpu.flags |= x86emu::flags::CF;
        } else {
            self.cpu.flags &= !x86emu::flags::CF;
        }
    }

    fn vwin_call<H: Host>(&mut self, host: &mut H) -> u16 {
        let ah = (self.cpu.regs[EAX] >> 8) as u8;
        for l in core::mem::take(&mut self.vwin.log) {
            host.log(&l);
        }
        if self.vwin.trace {
            let r = &self.cpu.regs;
            let msg = alloc::format!("vwin: INT 66h eax={:08x} ebx={:08x} ecx={:08x} edx={:08x} edi={:08x} ds={:04x} pm={}", r[EAX], r[EBX], r[ECX], r[EDX], r[EDI], self.cpu.seg[DS].sel, self.cpu.pm() as u8);
            host.log(&msg);
        }
        let status: u16 = match ah {
            // WINSHIELDADDUSERROUTINE: the routine in EBX (16:16), our
            // message entry point back in EDX.
            0x01 => {
                let ebx = self.cpu.regs[EBX];
                let ds = self.cpu.seg[DS].sel;
                self.vwin.routine = Some(((ebx >> 16) as u16, ebx as u16, ds));
                let (sel, off) = if self.cpu.pm() { self.dpmi_vdd_entry(crate::vdd::VWIN_INT) } else { (crate::bios::ROM, crate::vdd::VDD_CODE + crate::vdd::VWIN_INT * 8) };
                self.cpu.regs[EDX] = (sel as u32) << 16 | off as u32;
                if !self.vwin.inbox.is_empty() {
                    self.vwin.call_due = true;
                }
                0
            }
            // WINSHIELDSENDMESSAGEBLOCK / NOBLOCK: the message at EBX, CX
            // bytes; numbered in place. The blocking one waits for PM's
            // reply in the same buffer (EDI bytes of room): the message
            // itself.
            0x02 | 0x03 => {
                let len = (self.cpu.regs[ECX] & 0xffff) as usize;
                let at = self.vwin_buf();
                let mut m = vec![0u8; len.min(0x1000)];
                self.hw.read_bytes(at, &mut m);
                self.vwin.seq = self.vwin.seq.wrapping_add(1);
                let seq = self.vwin.seq;
                if m.len() >= 4 {
                    m[0..4].copy_from_slice(&seq.to_le_bytes());
                    self.hw.write_bytes(at, &seq.to_le_bytes());
                }
                if self.vwin.trace {
                    let mut t = alloc::format!("vwin: {} code {:#x}:", if ah == 2 { "send" } else { "post" }, rd16(&m, 0x24));
                    for b in m.iter().take(0x2c) {
                        t.push_str(&alloc::format!(" {b:02x}"));
                    }
                    host.log(&t);
                }
                self.vwin.learn(&m);
                if ah == 0x02 {
                    // PM's answer to WINSHELD's first message: the width
                    // of the windows' sizing borders (PM's).
                    if rd16(&m, 0x24) == MSG_INIT && m.len() >= 0x2c {
                        self.hw.wr16(at + 8, 4);
                    }
                    self.cpu.regs[ECX] = (self.cpu.regs[ECX] & 0xffff_0000) | len as u32;
                }
                0
            }
            0x04 => 0,
            // WINSHIELDGETNEXTMESSAGE: into EBX, EDI bytes of room; its
            // size in CX.
            0x05 => match self.vwin.inbox.front() {
                None => 2,
                Some(m) if m.len() > (self.cpu.regs[EDI] & 0xffff) as usize && self.cpu.regs[EDI] & 0xffff != 0 => {
                    self.cpu.regs[EDI] = (self.cpu.regs[EDI] & 0xffff_0000) | m.len() as u32;
                    0x7a
                }
                Some(_) => {
                    let m = self.vwin.inbox.pop_front().unwrap();
                    let at = self.vwin_buf();
                    self.hw.write_bytes(at, &m);
                    self.cpu.regs[ECX] = (self.cpu.regs[ECX] & 0xffff_0000) | m.len() as u32;
                    if !self.vwin.inbox.is_empty() {
                        self.vwin.call_due = true;
                    }
                    0
                }
            },
            0x06 => {
                self.vwin.routine = None;
                0
            }
            0x07 | 0x08 => 0,
            // WINDIDWINDDINITIALIZE: yes (RS2DISP needs no VWIN display
            // calls; IBM's seamless drivers would have made them).
            0x7f => 0,
            // WINAMISEAMLESS: 0 seamless, 1 not; the session's id in BX.
            0x80 => {
                let id = self.vwin.id;
                self.cpu.set_bx(id as u16);
                if self.vwin.seamless { 0 } else { 1 }
            }
            0x81..=0x8a => 0,
            // Pipes: a handle for an open, nothing passes. WINSHELD's
            // notification pipe (AL=3, its routine in EBX) is told at once
            // what PM's side tells it when the session starts: start the
            // programs (21h), the clipboard and DDE with OS/2 off (the
            // flags 0101h: not done yet).
            0xe0 => {
                let ebx = self.cpu.regs[EBX];
                if self.cpu.regs[EAX] & 0xff == 3 && ebx != 0 {
                    let ds = self.cpu.seg[DS].sel;
                    self.vwin.calls.push_back(((ebx >> 16) as u16, ebx as u16, ds, vec![0x21, 0x0101]));
                }
                self.cpu.regs[EDX] = 1;
                0
            }
            0xe1..=0xe5 => 0,
            _ => 0x16,
        };
        status
    }

    /// The host's requests: queued for WINSHELD.
    pub(crate) fn vwin_request(&mut self, r: Request) {
        self.vwin.request(r);
    }

    /// Whether a far call into Windows is due (the user routine, a
    /// notification).
    pub(crate) fn vwin_call_ready(&self) -> bool {
        !self.vwin.in_call && self.dpmi.client.is_some() && (!self.vwin.calls.is_empty() || (self.vwin.call_due && self.vwin.routine.is_some()))
    }

    /// Make the call: the user routine is VWINCALLBACK(0), which posts
    /// WINSHELD the message to read the queue. From real mode, as VWIN
    /// does in a context hook.
    pub(crate) fn vwin_call_routine(&mut self) -> bool {
        let (sel, off, ds, args) = match self.vwin.calls.pop_front() {
            Some(c) => c,
            None => {
                let Some((sel, off, ds)) = self.vwin.routine else { return false };
                self.vwin.call_due = false;
                (sel, off, ds, vec![0])
            }
        };
        self.vwin.in_call = true;
        if self.vwin.trace {
            self.vwin.log.push(alloc::format!("vwin: call {sel:04x}:{off:04x} {args:x?}"));
        }
        self.dpmi_call_pm_args(sel, off as u32, &args);
        let _ = self.cpu.load_seg(&mut self.hw, DS, ds);
        true
    }

    /// The window list changed since `generation`.
    pub fn vwin_changed(&self, generation: u32) -> bool {
        self.vwin.generation != generation
    }
}
