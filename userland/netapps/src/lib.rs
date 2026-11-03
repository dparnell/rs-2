//! Shared code of rs2's TCP/IP applications: sockets over RS2API (as
//! PING uses them), buffered line reading, files, console input and the
//! ETC directory.

#![no_std]

extern crate alloc;

pub mod lpd;
pub mod rcmd;
pub mod talk;
pub mod tftp;

use alloc::string::String;
use alloc::vec::Vec;

use os2::net::*;
use os2::path::cstr;
use os2::*;

pub use utils::{args, eprint, out, outln, stdin_is_console};

/// A socket, closed when dropped.
pub struct Sock(pub u32);

impl Drop for Sock {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { Rs2SockClose(self.0) };
        }
    }
}

impl Sock {
    pub fn new(typ: u32) -> Result<Sock, u32> {
        let mut s = 0;
        match unsafe { Rs2SockCreate(AF_INET, typ, 0, &mut s) } {
            0 => Ok(Sock(s)),
            e => Err(e),
        }
    }

    pub fn tcp() -> Result<Sock, u32> {
        Sock::new(SOCK_STREAM)
    }

    pub fn udp() -> Result<Sock, u32> {
        Sock::new(SOCK_DGRAM)
    }

    /// Give up the handle without closing it.
    pub fn into_raw(mut self) -> u32 {
        core::mem::replace(&mut self.0, 0)
    }

    pub fn connect_addr(addr: [u8; 4], port: u16) -> Result<Sock, u32> {
        let s = Sock::tcp()?;
        let a = SockAddrIn::new(addr, port);
        match unsafe { Rs2SockConnect(s.0, &a, 16) } {
            0 => Ok(s),
            e => Err(e),
        }
    }

    /// Connect to a host by name or address; the error is a message.
    pub fn connect(host: &str, port: u16) -> Result<(Sock, [u8; 4]), String> {
        let addr = lookup(host)?;
        Sock::connect_addr(addr, port).map(|s| (s, addr)).map_err(|e| String::from(strerror(e)))
    }

    pub fn bind(&self, addr: [u8; 4], port: u16) -> Result<(), u32> {
        let a = SockAddrIn::new(addr, port);
        match unsafe { Rs2SockBind(self.0, &a, 16) } {
            0 => Ok(()),
            e => Err(e),
        }
    }

    pub fn reuse(&self) {
        let one = 1u32;
        unsafe { Rs2SockSetOpt(self.0, SOL_SOCKET, SO_REUSEADDR, &one as *const u32 as *const u8, 4) };
    }

    pub fn listen(&self, n: u32) -> Result<(), u32> {
        match unsafe { Rs2SockListen(self.0, n) } {
            0 => Ok(()),
            e => Err(e),
        }
    }

    pub fn accept(&self) -> Result<(Sock, SockAddrIn), u32> {
        let mut peer = SockAddrIn::default();
        let mut ns = 0;
        match unsafe { Rs2SockAccept(self.0, &mut peer, &mut ns) } {
            0 => Ok((Sock(ns), peer)),
            e => Err(e),
        }
    }

    pub fn send(&self, b: &[u8]) -> Result<(), u32> {
        let mut off = 0;
        while off < b.len() {
            let mut n = 0;
            let rc = unsafe { Rs2SockSend(self.0, b[off..].as_ptr(), (b.len() - off) as u32, 0, &mut n) };
            if rc != 0 {
                return Err(rc);
            }
            if n == 0 {
                return Err(32);
            }
            off += n as usize;
        }
        Ok(())
    }

    pub fn send_str(&self, s: &str) -> Result<(), u32> {
        self.send(&os2::text_bytes(s))
    }

    /// Receive; Ok(0) at the end of the stream.
    pub fn recv(&self, buf: &mut [u8]) -> Result<usize, u32> {
        let mut n = 0;
        match unsafe { Rs2SockRecv(self.0, buf.as_mut_ptr(), buf.len() as u32, 0, &mut n) } {
            0 => Ok(n as usize),
            e => Err(e),
        }
    }

    pub fn send_to(&self, b: &[u8], to: SockAddrIn) -> Result<(), u32> {
        let mut n = 0;
        match unsafe { Rs2SockSendTo(self.0, b.as_ptr(), b.len() as u32, 0, &to, &mut n) } {
            0 => Ok(()),
            e => Err(e),
        }
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SockAddrIn), u32> {
        let mut n = 0;
        let mut from = SockAddrIn::default();
        match unsafe { Rs2SockRecvFrom(self.0, buf.as_mut_ptr(), buf.len() as u32, 0, &mut from, &mut n) } {
            0 => Ok((n as usize, from)),
            e => Err(e),
        }
    }

    /// Wait up to `ms` for data (or the end of the stream, or a connection).
    pub fn readable(&self, ms: i32) -> bool {
        let mut s = [self.0];
        let mut n = 0;
        unsafe { Rs2SockSelect(s.as_mut_ptr(), 1, 0, 0, ms, &mut n) == 0 && n > 0 }
    }

    /// Receive timeout in milliseconds (0: none).
    pub fn set_timeout(&self, ms: u32) {
        let tv = [ms / 1000, ms % 1000 * 1000];
        unsafe { Rs2SockSetOpt(self.0, SOL_SOCKET, SO_RCVTIMEO, tv.as_ptr() as *const u8, 8) };
    }

    pub fn shutdown_write(&self) {
        unsafe { Rs2SockShutdown(self.0, 1) };
    }

    pub fn local(&self) -> SockAddrIn {
        let mut a = SockAddrIn::default();
        unsafe { Rs2SockGetName(self.0, 0, &mut a) };
        a
    }

    pub fn peer(&self) -> SockAddrIn {
        let mut a = SockAddrIn::default();
        unsafe { Rs2SockGetName(self.0, 1, &mut a) };
        a
    }
}

/// Connect to `addr:port` from one of the local `ports` (reserved ports
/// for the r-commands and LPD), the first that is free. A port whose
/// connection does not complete within 3 seconds (the server may still
/// hold an earlier connection from it) is given up for the next one.
pub fn connect_from(addr: [u8; 4], port: u16, ports: impl Iterator<Item = u16>) -> Result<Sock, u32> {
    let mut last = 60; // ETIMEDOUT
    let mut slow = 0;
    for lp in ports {
        let s = Sock::tcp()?;
        s.reuse();
        if s.bind([0; 4], lp).is_err() {
            continue;
        }
        let mut on = 1u32;
        unsafe { Rs2SockIoctl(s.0, FIONBIO, &mut on) };
        let a = SockAddrIn::new(addr, port);
        let rc = unsafe { Rs2SockConnect(s.0, &a, 16) };
        match rc {
            0 => {}
            EINPROGRESS => {
                let mut v = [s.0];
                let mut n = 0;
                unsafe { Rs2SockSelect(v.as_mut_ptr(), 0, 1, 0, 3000, &mut n) };
                if n == 0 {
                    slow += 1;
                    if slow >= 4 {
                        return Err(60);
                    }
                    continue;
                }
                let mut err = 0u32;
                let mut cb = 4u32;
                unsafe { Rs2SockGetOpt(s.0, SOL_SOCKET, SO_ERROR, &mut err as *mut u32 as *mut u8, &mut cb) };
                if err != 0 {
                    if err == 48 || err == 49 {
                        continue;
                    }
                    return Err(err);
                }
            }
            48 | 49 => continue,
            e => {
                last = e;
                return Err(last);
            }
        }
        let mut off = 0u32;
        unsafe { Rs2SockIoctl(s.0, FIONBIO, &mut off) };
        return Ok(s);
    }
    Err(last)
}

/// An address for a host name or dotted quad; the error is a message.
pub fn lookup(host: &str) -> Result<[u8; 4], String> {
    if let Some(a) = parse_ip(host) {
        return Ok(a);
    }
    match resolve(host) {
        Ok(v) if !v.is_empty() => Ok(v[0]),
        Ok(_) => Err(alloc::format!("{host}: unknown host")),
        Err(e) => Err(alloc::format!("{host}: {}", hstrerror(e))),
    }
}

/// A port number or a service name from the SERVICES file.
pub fn service_port(s: &str, proto: &str, default: u16) -> u16 {
    if let Ok(n) = s.parse::<u16>() {
        return n;
    }
    if let Some(data) = read_file(&etc_path("SERVICES")) {
        for l in utils::lines(&data) {
            let l = l.split('#').next().unwrap_or("");
            let mut w = l.split_whitespace();
            let (Some(name), Some(pp)) = (w.next(), w.next()) else { continue };
            let aliases: Vec<&str> = w.collect();
            if let Some((p, pr)) = pp.split_once('/')
                && pr.eq_ignore_ascii_case(proto)
                && (name.eq_ignore_ascii_case(s) || aliases.iter().any(|a| a.eq_ignore_ascii_case(s)))
                && let Ok(n) = p.parse()
            {
                return n;
            }
        }
    }
    default
}

/// Reads lines from a socket (CR LF or LF ends a line).
pub struct LineReader {
    pub buf: Vec<u8>,
}

impl LineReader {
    pub const fn new() -> LineReader {
        LineReader { buf: Vec::new() }
    }

    /// The next line without its end; None when the stream ends first.
    pub fn line(&mut self, s: &Sock) -> Option<String> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let mut l: Vec<u8> = self.buf.drain(..=i).collect();
                l.pop();
                if l.last() == Some(&b'\r') {
                    l.pop();
                }
                return Some(l.iter().map(|&b| b as char).collect());
            }
            let mut tmp = [0u8; 2048];
            match s.recv(&mut tmp) {
                Ok(0) | Err(_) => {
                    if self.buf.is_empty() {
                        return None;
                    }
                    let l: String = self.buf.drain(..).map(|b| b as char).collect();
                    return Some(l);
                }
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
            }
        }
    }
}

impl Default for LineReader {
    fn default() -> Self {
        Self::new()
    }
}

/// An environment variable.
pub fn env(name: &str) -> Option<String> {
    let c = cstr(name);
    let mut v: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(c.as_ptr(), &mut v) } != 0 || v.is_null() {
        return None;
    }
    let mut s = String::new();
    let mut p = v;
    unsafe {
        while *p != 0 {
            s.push(*p as char);
            p = p.add(1);
        }
    }
    Some(String::from(s.trim()))
}

/// A file of the TCP/IP ETC directory (%ETC%, else \MPTN\ETC).
pub fn etc_path(file: &str) -> String {
    let dir = env("ETC").filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("C:\\MPTN\\ETC"));
    let dir = dir.trim_end_matches('\\');
    alloc::format!("{dir}\\{file}")
}

pub fn read_file(path: &str) -> Option<Vec<u8>> {
    let hf = utils::open_read(path).ok()?;
    let d = utils::read_all(hf);
    unsafe { DosClose(hf) };
    Some(d)
}

/// Create (or replace) a file for writing; append = keep and add to it.
pub fn create_file(path: &str, append: bool) -> Result<u32, u32> {
    let c = cstr(path);
    let (mut hf, mut act) = (0, 0);
    let action = if append { OPEN_ACTION_OPEN_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW } else { OPEN_ACTION_REPLACE_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW };
    let rc = unsafe { DosOpen(c.as_ptr(), &mut hf, &mut act, 0, FILE_NORMAL, action, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE | OPEN_FLAGS_NOINHERIT, core::ptr::null_mut()) };
    if rc != 0 {
        return Err(rc);
    }
    if append {
        let mut pos = 0;
        unsafe { DosSetFilePtr(hf, 0, FILE_END, &mut pos) };
    }
    Ok(hf)
}

pub fn write_file(hf: u32, b: &[u8]) -> Result<(), u32> {
    let mut n = 0;
    match unsafe { DosWrite(hf, b.as_ptr(), b.len() as u32, &mut n) } {
        0 if n as usize == b.len() => Ok(()),
        0 => Err(112),
        e => Err(e),
    }
}

/// A file's size (None if it does not exist or is a directory).
pub fn file_size(path: &str) -> Option<u32> {
    let e = utils::find(path, 0x07);
    e.first().map(|e| e.size)
}

pub fn is_dir(path: &str) -> bool {
    let c = cstr(path);
    let mut st = [0u8; 24];
    unsafe { DosQueryPathInfo(c.as_ptr(), 1, st.as_mut_ptr(), 24) == 0 && st[20] & 0x10 != 0 }
}

pub fn current_dir() -> String {
    let (mut d, mut map) = (0, 0);
    unsafe { DosQueryCurrentDisk(&mut d, &mut map) };
    let mut buf = [0u8; 260];
    let mut len = buf.len() as u32;
    unsafe { DosQueryCurrentDir(0, buf.as_mut_ptr(), &mut len) };
    let n = buf.iter().position(|&b| b == 0).unwrap_or(0);
    let rest: String = buf[..n].iter().map(|&b| b as char).collect();
    alloc::format!("{}:\\{}", (b'A' + d as u8 - 1) as char, rest)
}

/// Change the current drive and directory.
pub fn change_dir(path: &str) -> Result<(), u32> {
    let b = path.as_bytes();
    if b.len() >= 2 && b[1] == b':' {
        let d = (b[0].to_ascii_uppercase() - b'A' + 1) as u32;
        let rc = unsafe { DosSetDefaultDisk(d) };
        if rc != 0 {
            return Err(rc);
        }
        if b.len() == 2 {
            return Ok(());
        }
    }
    let c = cstr(path);
    match unsafe { DosSetCurrentDir(c.as_ptr()) } {
        0 => Ok(()),
        e => Err(e),
    }
}

/// Read a line of input after a prompt (None at the end of redirected
/// input). Redirected lines are echoed, as if typed.
pub fn read_line(prompt: &str) -> Option<String> {
    out(prompt);
    let mut buf = Vec::new();
    if stdin_is_console() {
        let mut b = [0u8; 512];
        let mut n = 0;
        if unsafe { DosRead(0, b.as_mut_ptr(), b.len() as u32, &mut n) } != 0 || n == 0 {
            return None;
        }
        buf.extend_from_slice(&b[..n as usize]);
    } else {
        loop {
            let mut c = [0u8; 1];
            let mut got = 0;
            if unsafe { DosRead(0, c.as_mut_ptr(), 1, &mut got) } != 0 || got == 0 {
                if buf.is_empty() {
                    return None;
                }
                break;
            }
            if c[0] == 0x1a {
                if buf.is_empty() {
                    return None;
                }
                break;
            }
            buf.push(c[0]);
            if c[0] == b'\n' {
                break;
            }
        }
        os2::write(&buf);
        if buf.last() != Some(&b'\n') {
            out("\r\n");
        }
    }
    let s: String = buf.iter().map(|&b| b as char).collect();
    Some(String::from(s.trim_end_matches(['\r', '\n'])))
}

/// KBDKEYINFO.
#[repr(C, align(16))]
#[derive(Default, Clone, Copy)]
pub struct KeyInfo {
    pub ch: u8,
    pub scan: u8,
    pub status: u8,
    pub nls_shift: u8,
    pub state: u16,
    pub time: u32,
}

unsafe extern "C" {
    #[link_name = "KBDCALLS.4"]
    fn KBDCHARIN();
}

/// KbdCharIn: the next key (waiting for one when `wait`); None without one.
pub fn key_in(wait: bool) -> Option<KeyInfo> {
    let mut k = KeyInfo::default();
    let p = os2::vio::ptr_words(&k);
    let rc = unsafe { os2::vio::call16(KBDCHARIN as *const () as usize, &[p[0], p[1], if wait { 0 } else { 1 }, 0]) };
    let k2 = unsafe { core::ptr::read_volatile(&mut k) };
    // fbStatus bit 6 (final character): a key arrived.
    (rc == 0 && k2.status & 0x40 != 0).then_some(k2)
}

/// Read a password without echoing it (a line of redirected input).
pub fn read_password(prompt: &str) -> Option<String> {
    if !stdin_is_console() {
        out(prompt);
        let mut buf = Vec::new();
        loop {
            let mut c = [0u8; 1];
            let mut got = 0;
            if unsafe { DosRead(0, c.as_mut_ptr(), 1, &mut got) } != 0 || got == 0 || c[0] == b'\n' {
                break;
            }
            buf.push(c[0]);
        }
        out("\r\n");
        let s: String = buf.iter().map(|&b| b as char).collect();
        return Some(String::from(s.trim_end_matches('\r')));
    }
    out(prompt);
    let mut s = String::new();
    loop {
        let k = key_in(true)?;
        match k.ch {
            b'\r' | b'\n' => break,
            8 => {
                s.pop();
            }
            3 | 27 => {
                out("\r\n");
                return None;
            }
            0 | 0xe0 => {}
            c => s.push(c as char),
        }
    }
    out("\r\n");
    Some(s)
}

/// Seconds and tenths since `start` (milliseconds), and a rate in KB/s.
pub fn rate(bytes: u64, start: u32) -> String {
    let ms = ms().wrapping_sub(start).max(1) as u64;
    let kbs = bytes * 1000 / ms / 1024;
    alloc::format!("{bytes} bytes transferred in {}.{:03} seconds ({kbs} Kbytes/s)", ms / 1000, ms % 1000)
}

/// Start a thread running `f(param)` with a 64 KB stack.
pub fn thread(f: unsafe extern "C" fn(u32), param: u32) -> Result<u32, u32> {
    let mut tid = 0;
    match unsafe { DosCreateThread(&mut tid, f, param, 2, 65536) } {
        0 => Ok(tid),
        e => Err(e),
    }
}

/// Wildcard match (`*`, `?`, case-insensitive), as OS/2 file names.
pub fn wild(pat: &str, name: &str) -> bool {
    let p: Vec<u8> = pat.bytes().map(|b| b.to_ascii_uppercase()).collect();
    let n: Vec<u8> = name.bytes().map(|b| b.to_ascii_uppercase()).collect();
    fn m(p: &[u8], n: &[u8]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some(b'*') => (0..=n.len()).any(|i| m(&p[1..], &n[i..])),
            Some(b'?') => !n.is_empty() && m(&p[1..], &n[1..]),
            Some(c) => n.first() == Some(c) && m(&p[1..], &n[1..]),
        }
    }
    m(&p, &n)
}

/// The DOS date and time of a file's last write (FILESTATUS3 at 8..12).
pub struct FileTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub fn decode_fdate(d: u16, t: u16) -> FileTime {
    FileTime {
        year: 1980 + (d >> 9),
        month: ((d >> 5) & 15) as u8,
        day: (d & 31) as u8,
        hour: (t >> 11) as u8,
        minute: ((t >> 5) & 63) as u8,
        second: ((t & 31) * 2) as u8,
    }
}

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Directory entries with their write times.
pub struct DirEntry {
    pub name: String,
    pub attr: u32,
    pub size: u32,
    pub time: FileTime,
}

pub fn list_dir(spec: &str) -> Vec<DirEntry> {
    let mut v = Vec::new();
    let c = cstr(spec);
    let mut hdir = HDIR_CREATE;
    let mut buf = alloc::vec![0u8; 8192];
    let mut count = 64;
    let mut rc = unsafe { DosFindFirst(c.as_ptr(), &mut hdir, 0x17, buf.as_mut_ptr(), buf.len() as u32, &mut count, FIL_STANDARD) };
    if rc != 0 {
        return v;
    }
    loop {
        let mut off = 0usize;
        for _ in 0..count {
            let e = &buf[off..];
            let w = |i: usize| u16::from_le_bytes([e[i], e[i + 1]]);
            let next = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let size = u32::from_le_bytes([e[16], e[17], e[18], e[19]]);
            let attr = u32::from_le_bytes([e[24], e[25], e[26], e[27]]);
            let n = e[28] as usize;
            let name: String = e[29..29 + n].iter().map(|&b| b as char).collect();
            if name != "." && name != ".." {
                v.push(DirEntry { name, attr, size, time: decode_fdate(w(12), w(14)) });
            }
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 64;
        rc = unsafe { DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
        if rc != 0 {
            break;
        }
    }
    unsafe { DosFindClose(hdir) };
    v
}

/// The rexec/rsh exchange after the request: the status byte (0, or 1
/// and a message), then the command's output until the server closes.
/// Redirected standard input goes to the command unless `no_input`.
pub fn run_remote(sock: &Sock, no_input: bool, prog: &str) -> u32 {
    let mut b = [0u8; 1];
    match sock.recv(&mut b) {
        Ok(1) if b[0] == 0 => {}
        Ok(1) => {
            let mut msg = Vec::new();
            let mut buf = [0u8; 512];
            while let Ok(n) = sock.recv(&mut buf) {
                if n == 0 {
                    break;
                }
                msg.extend_from_slice(&buf[..n]);
                if msg.contains(&b'\n') {
                    break;
                }
            }
            let t: String = msg.iter().map(|&b| b as char).collect();
            eprint(&alloc::format!("{prog}: {}\r\n", t.trim_end()));
            return 4;
        }
        _ => {
            eprint(&alloc::format!("{prog}: the server closed the connection\r\n"));
            return 4;
        }
    }
    if !no_input && !stdin_is_console() {
        let input = utils::read_all(0);
        let _ = sock.send(&input);
    }
    sock.shutdown_write();
    lpd::copy_reply(sock);
    0
}

/// Text mode: bare LF becomes CR LF (and a lone CR stays).
pub fn to_crlf(data: &[u8], out: &mut Vec<u8>, last: &mut u8) {
    for &b in data {
        if b == b'\n' && *last != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        *last = b;
    }
}
