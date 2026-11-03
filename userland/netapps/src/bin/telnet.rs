//! TELNET: a terminal for remote hosts (RFC 854) in the text window or
//! full screen it runs in.
//!
//! `TELNET [-p port] [-t termtype] [-l logfile] [-e] [hostname [port]]`.
//! The host's output goes through a VT100/VT220/ANSI emulator
//! (crates/vt100) onto the VIO screen; keys go out as a VT220's do
//! (cursor keys, F1-F12, editing keys). Options: ECHO and SUPPRESS GO
//! AHEAD (the host echoes; else TELNET echoes locally), TERMINAL TYPE
//! (-t, VT100 by default) and NAWS (the window's size). Ctrl+] ends the
//! session. Without a host name it asks for one (`open host [port]`).
//! `-cp codepage` (819, ISO8859-1, IBM-850...): the host sends and takes
//! single bytes of that code page rather than UTF-8; they are translated
//! to and from the local code page.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use netapps::*;
use os2::vio::{call16, ptr_words};
use vt100::{Key, Term};
os2::entry!(main);

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;

unsafe extern "C" {
    #[link_name = "VIOCALLS.10"]
    fn VIOWRTCELLSTR();
    #[link_name = "VIOCALLS.15"]
    fn VIOSETCURPOS();
}

static SOCK: AtomicU32 = AtomicU32::new(0);
/// -cp: (inbound, outbound) translation tables.
static mut XLATE: Option<([u8; 256], [u8; 256])> = None;

fn xlate() -> Option<&'static ([u8; 256], [u8; 256])> {
    unsafe { (*(&raw const XLATE)).as_ref() }
}
static REMOTE_ECHO: AtomicBool = AtomicBool::new(false);
static ENTER_CRLF: AtomicBool = AtomicBool::new(true);
static APP_CURSOR: AtomicBool = AtomicBool::new(false);
/// Locally echoed input waiting for the screen (main thread draws it).
static LOCAL: os2_lock::Locked = os2_lock::Locked::new();

mod os2_lock {
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicBool, Ordering};
    pub struct Locked {
        f: AtomicBool,
        v: core::cell::UnsafeCell<Vec<u8>>,
    }
    unsafe impl Sync for Locked {}
    impl Locked {
        pub const fn new() -> Locked {
            Locked { f: AtomicBool::new(false), v: core::cell::UnsafeCell::new(Vec::new()) }
        }
        pub fn with<R>(&self, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
            while self.f.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
                unsafe { os2::DosSleep(0) };
            }
            let r = f(unsafe { &mut *self.v.get() });
            self.f.store(false, Ordering::Release);
            r
        }
    }
}

fn sock_send(b: &[u8]) {
    let s = SOCK.load(Ordering::Relaxed);
    let mut n = 0;
    unsafe { os2::net::Rs2SockSend(s, b.as_ptr(), b.len() as u32, 0, &mut n) };
}

/// The keyboard thread: keys to the host.
unsafe extern "C" fn keyboard(_: u32) {
    loop {
        let Some(k) = key_in(true) else {
            unsafe { os2::DosSleep(10) };
            continue;
        };
        let mut out: Vec<u8> = Vec::new();
        let ext = k.ch == 0 || (k.ch == 0xe0 && k.scan != 0);
        if ext {
            let key = match k.scan {
                0x48 => Some(Key::Up),
                0x50 => Some(Key::Down),
                0x4b => Some(Key::Left),
                0x4d => Some(Key::Right),
                0x47 => Some(Key::Home),
                0x4f => Some(Key::End),
                0x49 => Some(Key::PageUp),
                0x51 => Some(Key::PageDown),
                0x52 => Some(Key::Insert),
                0x53 => Some(Key::Delete),
                0x3b..=0x44 => Some(Key::F(k.scan - 0x3a)),
                0x85 => Some(Key::F(11)),
                0x86 => Some(Key::F(12)),
                _ => None,
            };
            if let Some(key) = key {
                // The cursor keys' form follows the host's DECCKM.
                out.extend_from_slice(vt100::key_bytes(key, APP_CURSOR.load(Ordering::Relaxed)));
            }
        } else {
            match k.ch {
                0x1d => {
                    // Ctrl+]: leave.
                    let s = SOCK.load(Ordering::Relaxed);
                    unsafe { os2::net::Rs2SockShutdown(s, 2) };
                    return;
                }
                b'\r' => out.extend_from_slice(if ENTER_CRLF.load(Ordering::Relaxed) { b"\r\n" } else { b"\r\0" }),
                8 => out.push(0x7f),
                IAC => out.extend_from_slice(&[IAC, IAC]),
                c => out.push(xlate().map_or(c, |t| t.1[c as usize])),
            }
        }
        if out.is_empty() {
            continue;
        }
        if !REMOTE_ECHO.load(Ordering::Relaxed) {
            let echo: Vec<u8> = out.iter().copied().filter(|&b| b != 0).map(|b| if b == 0x7f { 8 } else { b }).collect();
            LOCAL.with(|v| v.extend_from_slice(&echo));
        }
        sock_send(&out);
    }
}

struct Screen {
    term: Term,
    /// A 64 KB block for 16-bit buffers (never crosses a tile).
    buf: *mut u8,
}

impl Screen {
    fn draw(&mut self) {
        for r in self.term.take_dirty() {
            let cells = self.term.row_cells(r);
            let n = cells.len() * 2;
            unsafe { core::ptr::copy_nonoverlapping(cells.as_ptr() as *const u8, self.buf, n) };
            let p = ptr_words(self.buf);
            unsafe { call16(VIOWRTCELLSTR as *const () as usize, &[p[0], p[1], n as u16, r as u16, 0, 0]) };
        }
        let (r, c) = self.term.cursor();
        unsafe { call16(VIOSETCURPOS as *const () as usize, &[r as u16, c as u16, 0]) };
    }
}

struct Telnet {
    sb: Option<Vec<u8>>,
    state: u8,
    cmd: u8,
    ttype: String,
    cols: u16,
    rows: u16,
    will: [bool; 256],
    do_: [bool; 256],
}

impl Telnet {
    fn send_naws(&self) {
        let c = self.cols.to_be_bytes();
        let r = self.rows.to_be_bytes();
        let mut v = alloc::vec![IAC, SB, NAWS];
        for b in [c[0], c[1], r[0], r[1]] {
            v.push(b);
            if b == IAC {
                v.push(IAC);
            }
        }
        v.extend_from_slice(&[IAC, SE]);
        sock_send(&v);
    }

    fn option(&mut self, cmd: u8, opt: u8) {
        let o = opt as usize;
        match cmd {
            DO => {
                let ok = matches!(opt, TTYPE | NAWS | SGA);
                if ok != self.will[o] || !ok {
                    if ok {
                        self.will[o] = true;
                        sock_send(&[IAC, WILL, opt]);
                    } else {
                        sock_send(&[IAC, WONT, opt]);
                    }
                }
                if opt == NAWS && ok {
                    self.send_naws();
                }
            }
            DONT => {
                if self.will[o] {
                    self.will[o] = false;
                    sock_send(&[IAC, WONT, opt]);
                }
            }
            WILL => {
                let ok = matches!(opt, ECHO | SGA);
                if ok {
                    if !self.do_[o] {
                        self.do_[o] = true;
                        sock_send(&[IAC, DO, opt]);
                    }
                    if opt == ECHO {
                        REMOTE_ECHO.store(true, Ordering::Relaxed);
                    }
                } else {
                    sock_send(&[IAC, DONT, opt]);
                }
            }
            WONT => {
                if self.do_[o] {
                    self.do_[o] = false;
                    sock_send(&[IAC, DONT, opt]);
                }
                if opt == ECHO {
                    REMOTE_ECHO.store(false, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    fn subneg(&mut self, sb: &[u8]) {
        if sb.len() >= 2 && sb[0] == TTYPE && sb[1] == 1 {
            let mut v = alloc::vec![IAC, SB, TTYPE, 0];
            v.extend_from_slice(self.ttype.as_bytes());
            v.extend_from_slice(&[IAC, SE]);
            sock_send(&v);
        }
    }

    /// Strip the protocol out of what arrived; returns the data.
    fn input(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for &b in data {
            match self.state {
                0 => {
                    if b == IAC {
                        self.state = 1;
                    } else if let Some(sb) = &mut self.sb {
                        sb.push(b);
                    } else {
                        out.push(b);
                    }
                }
                1 => {
                    self.state = 0;
                    match b {
                        IAC => {
                            if let Some(sb) = &mut self.sb {
                                sb.push(IAC);
                            } else {
                                out.push(IAC);
                            }
                        }
                        DO | DONT | WILL | WONT => {
                            self.cmd = b;
                            self.state = 2;
                        }
                        SB => self.sb = Some(Vec::new()),
                        SE => {
                            if let Some(sb) = self.sb.take() {
                                self.subneg(&sb);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {
                    self.state = 0;
                    let c = self.cmd;
                    self.option(c, b);
                }
            }
        }
        out
    }
}

fn session(host: &str, port: u16, ttype: &str, log: Option<u32>) -> u32 {
    let (sock, addr) = match Sock::connect(host, port) {
        Ok(v) => v,
        Err(e) => {
            outln(&alloc::format!("telnet: {e}"));
            return 2;
        }
    };
    outln(&alloc::format!("Trying {}...", os2::net::ip(addr)));
    let m = os2::vio::vio_get_mode().ok();
    let (cols, rows) = m.map(|m| (m.col as usize, m.row as usize)).filter(|&(c, r)| c >= 20 && r >= 5).unwrap_or((80, 25));
    let mut block: *mut u8 = core::ptr::null_mut();
    unsafe { os2::DosAllocMem(&mut block as *mut *mut u8 as *mut _, 65536, 0x13) };
    let mut scr = Screen { term: Term::new(cols, rows), buf: block };
    if xlate().is_some() {
        scr.term.utf8_enabled = false;
    }
    let mut tn = Telnet { sb: None, state: 0, cmd: 0, ttype: String::from(ttype), cols: cols as u16, rows: rows as u16, will: [false; 256], do_: [false; 256] };
    SOCK.store(sock.0, Ordering::Relaxed);
    scr.term.feed(alloc::format!("Connected to {host}.\r\nEscape character is 'Ctrl+]'.\r\n").as_bytes());
    scr.draw();
    // Offer what we do; hosts that want line mode leave ECHO off.
    tn.will[TTYPE as usize] = true;
    tn.will[NAWS as usize] = true;
    tn.do_[SGA as usize] = true;
    sock_send(&[IAC, WILL, TTYPE, IAC, WILL, NAWS, IAC, DO, SGA]);
    if thread(keyboard, 0).is_err() {
        return 3;
    }
    let mut buf = alloc::vec![0u8; 8192];
    loop {
        let ready = sock.readable(50);
        let local = LOCAL.with(core::mem::take);
        if !local.is_empty() {
            let mut v = Vec::new();
            for b in local {
                if b == b'\r' {
                    continue;
                }
                if b == b'\n' {
                    v.extend_from_slice(b"\r\n");
                } else {
                    v.push(b);
                }
            }
            scr.term.feed(&v);
        }
        if ready {
            match sock.recv(&mut buf) {
                Ok(n) if n > 0 => {
                    let mut d = tn.input(&buf[..n]);
                    if let Some(t) = xlate() {
                        for b in d.iter_mut() {
                            *b = t.0[*b as usize];
                        }
                    }
                    if let Some(hf) = log {
                        let _ = write_file(hf, &d);
                    }
                    scr.term.feed(&d);
                    let r = scr.term.take_reply();
                    if !r.is_empty() {
                        sock_send(&r);
                    }
                    APP_CURSOR.store(scr.term.app_cursor, Ordering::Relaxed);
                    ENTER_CRLF.store(true, Ordering::Relaxed);
                }
                _ => break,
            }
        }
        scr.draw();
    }
    scr.term.feed(b"\r\nConnection closed by foreign host.\r\n");
    scr.draw();
    0
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut port = None;
    let mut ttype = String::from("VT100");
    let mut logfile = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < a.len() {
        let x = a[i].to_ascii_lowercase();
        let val = |i: &mut usize| {
            *i += 1;
            a.get(*i).cloned()
        };
        match x.as_str() {
            "-p" => port = val(&mut i).map(|p| service_port(&p, "tcp", 23)),
            "-t" => ttype = val(&mut i).unwrap_or(ttype).to_ascii_uppercase(),
            "-l" => logfile = val(&mut i),
            "-cp" => {
                let name = val(&mut i).unwrap_or_default();
                let mut cps = [0u32; 4];
                let mut n = 0;
                unsafe { os2::DosQueryCp(16, cps.as_mut_ptr(), &mut n) };
                let local = if cps[0] == 0 { 850 } else { cps[0] as u16 };
                match uls::codepage_by_name(&name).and_then(|net| Some((uls::translation(net, local, b'?')?, uls::translation(local, net, b'?')?))) {
                    Some(t) => unsafe { XLATE = Some(t) },
                    None => {
                        eprint(&alloc::format!("telnet: translation {name} is not available\r\n"));
                        return 1;
                    }
                }
            }
            "-a" | "-d" | "-e" | "-f" | "-h" | "-k" | "-o" | "-u" | "-w" => {
                val(&mut i);
            }
            "-n" => {}
            "-?" | "/?" => {
                eprint("Usage: telnet [-p port] [-t termtype] [-l filename] [-cp codepage] [hostname [port]]\r\n");
                return 1;
            }
            _ => rest.push(a[i].clone()),
        }
        i += 1;
    }
    let log = logfile.and_then(|f| create_file(&f, false).ok());
    let (host, p2) = match rest.first() {
        Some(h) => (h.clone(), rest.get(1).cloned()),
        None => loop {
            let Some(l) = read_line("telnet> ") else { return 0 };
            let w = utils::split(&l);
            match w.first().map(|c| c.to_ascii_lowercase()) {
                Some(c) if c == "open" || c == "o" => match w.get(1) {
                    Some(h) => break (h.clone(), w.get(2).cloned()),
                    None => outln("Usage: open host-name [port]"),
                },
                Some(c) if c == "quit" || c == "q" || c == "close" => return 0,
                Some(_) => outln("Commands: open host-name [port], quit"),
                None => {}
            }
        },
    };
    let port = port.or_else(|| p2.map(|p| service_port(&p, "tcp", 23))).unwrap_or(23);
    let rc = session(&host, port, &ttype, log);
    if let Some(hf) = log {
        unsafe { os2::DosClose(hf) };
    }
    unsafe { os2::DosExit(1, rc) };
    rc
}
