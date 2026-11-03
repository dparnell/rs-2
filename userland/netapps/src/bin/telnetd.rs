//! TELNETD: the telnet server. `TELNETD [-p port] [-cp codepage] [-n]`
//! gives each client a command prompt (%COMSPEC%, else CMD.EXE) on a
//! thread of its own.
//!
//! The command processor runs in a pseudo terminal session (the kernel's
//! crate::pty: a text session with its own screen and keyboard that is
//! never shown). TELNETD reads that screen whenever it changes and sends
//! the differences as VT100/ANSI sequences (crates/vt100's Renderer:
//! colours, the cursor, scrolling), so full-screen VIO programs (E, the
//! editors, CMD's colours) work; what the client types becomes PC
//! keystrokes (cursor, editing and function keys from their VT220/xterm
//! sequences) in the session's keyboard queue. The client's window size
//! (NAWS) sets the session's screen size when it starts. Characters go
//! out as UTF-8, or with -cp as single bytes of that code page (-cp 437
//! on a CP437 screen sends the PC characters as they are).
//!
//! With -n, or when the kernel has no pseudo terminals, the command
//! processor runs on pipes instead: TELNETD echoes, edits the line and
//! passes it on at Enter, and only standard output is seen (Ctrl+C
//! starts a new command prompt).
//!
//! With TELNET.PASSWORD set in the environment (IBM's TELNETD's
//! password), clients must give it first.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use netapps::*;
use vt100::server::{Encoding, KeyDecoder, Renderer};
use os2::net::*;
use os2::*;
os2::entry!(main);

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

/// Held while standard handles are switched to a child's pipes.
static EXEC_LOCK: AtomicBool = AtomicBool::new(false);

struct Child {
    pid: u32,
    stdin: u32,
    stdout: u32,
}

/// Start the command processor on pipes.
fn spawn() -> Option<Child> {
    let (mut in_r, mut in_w, mut out_r, mut out_w) = (0, 0, 0, 0);
    unsafe {
        if DosCreatePipe(&mut in_r, &mut in_w, 4096) != 0 || DosCreatePipe(&mut out_r, &mut out_w, 4096) != 0 {
            return None;
        }
        DosSetFHState(in_w, OPEN_FLAGS_NOINHERIT);
        DosSetFHState(out_r, OPEN_FLAGS_NOINHERIT);
    }
    let comspec = env("COMSPEC").filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("C:\\OS2\\CMD.EXE"));
    let mut prog: Vec<u8> = comspec.bytes().collect();
    prog.push(0);
    let mut argv = prog.clone();
    argv.push(0);
    while EXEC_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        unsafe { DosSleep(1) };
    }
    let mut saved = [0xffff_ffffu32; 3];
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe {
        for (h, s) in saved.iter_mut().enumerate() {
            DosDupHandle(h as u32, s);
            DosSetFHState(*s, OPEN_FLAGS_NOINHERIT);
        }
        let mut t = 0;
        DosDupHandle(in_r, &mut t);
        for h in [1u32, 2] {
            let mut t = h;
            DosDupHandle(out_w, &mut t);
        }
        let mut fail = [0u8; 64];
        // EXEC_ASYNCRESULT: the PID comes back in code_terminate.
        let rc = DosExecPgm(fail.as_mut_ptr(), 64, 2, argv.as_ptr(), core::ptr::null(), &mut res, prog.as_ptr());
        for (h, s) in saved.iter().enumerate() {
            let mut t = h as u32;
            DosDupHandle(*s, &mut t);
            DosClose(*s);
        }
        rc
    };
    EXEC_LOCK.store(false, Ordering::Release);
    unsafe {
        DosClose(in_r);
        DosClose(out_w);
    }
    if rc != 0 {
        unsafe {
            DosClose(in_w);
            DosClose(out_r);
        }
        return None;
    }
    Some(Child { pid: res.code_terminate, stdin: in_w, stdout: out_r })
}

struct Pump {
    sock: u32,
    pipe: u32,
    /// Set when the session goes on with a new command processor.
    restarting: alloc::sync::Arc<AtomicBool>,
}

/// The child's output to the client (LF -> CR LF, IAC doubled).
unsafe extern "C" fn pump(param: u32) {
    let p = unsafe { alloc::boxed::Box::from_raw(param as *mut Pump) };
    let mut buf = [0u8; 2048];
    let mut last = 0u8;
    loop {
        let mut n = 0;
        if unsafe { DosRead(p.pipe, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        let mut o = Vec::with_capacity(n as usize + 16);
        for &b in &buf[..n as usize] {
            if b == b'\n' && last != b'\r' {
                o.push(b'\r');
            }
            if b == IAC {
                o.push(IAC);
            }
            o.push(b);
            last = b;
        }
        let mut sent = 0;
        if unsafe { Rs2SockSend(p.sock, o.as_ptr(), o.len() as u32, 0, &mut sent) } != 0 {
            break;
        }
    }
    unsafe {
        DosClose(p.pipe);
        // The command processor ended: end the session.
        if !p.restarting.load(Ordering::Acquire) {
            Rs2SockShutdown(p.sock, 2);
        }
    }
}

const TTYPE: u8 = 24;
const NAWS: u8 = 31;

unsafe extern "C" {
    #[link_name = "RS2API.12"]
    fn Rs2VioReadScreen(sid: u32, state: *mut u32, cells: *mut u16, cb: u32) -> u32;
    #[link_name = "RS2API.14"]
    fn Rs2VioKey(sid: u32, ev: *const KeyEvent) -> u32;
    #[link_name = "RS2API.16"]
    fn Rs2VioControl(sid: u32, cmd: u32, arg: u32) -> u32;
    #[link_name = "RS2API.18"]
    fn Rs2PtyStart(prog: *const u8, args: *const u8, env: *const u8, size: u32, sid: *mut u32, pid: *mut u32) -> u32;
    #[link_name = "RS2API.19"]
    fn Rs2PtyWait(sid: u32, changes: *mut u32, ms: u32) -> u32;
}

/// Rs2ReadInput's INPUTEVENT, as Rs2VioKey takes it.
#[repr(C)]
struct KeyEvent {
    kind: u16,
    flags: u16,
    a: u32,
    b: u32,
    time: u32,
}

const CTL_STOP: u32 = 3;
const ST_CURSOR: u32 = 0x01;
const ST_BLINK: u32 = 0x02;

/// The client's code page (-cp; 0: UTF-8).
static CLIENT_CP: AtomicU32 = AtomicU32::new(0);
/// -n: pipes, not a pseudo terminal.
static PIPES: AtomicBool = AtomicBool::new(false);

/// What the client typed, without the telnet commands; the options it
/// reported on the way.
struct Input {
    state: u8,
    in_sb: bool,
    sb: Vec<u8>,
    naws: Option<(u16, u16)>,
    ttype: Option<String>,
}

impl Input {
    fn new() -> Input {
        Input { state: 0, in_sb: false, sb: Vec::new(), naws: None, ttype: None }
    }

    fn end_sb(&mut self) {
        let sb = core::mem::take(&mut self.sb);
        match sb.first() {
            Some(&NAWS) if sb.len() >= 5 => {
                let w = u16::from_be_bytes([sb[1], sb[2]]);
                let h = u16::from_be_bytes([sb[3], sb[4]]);
                self.naws = Some((w, h));
            }
            // TERMINAL-TYPE IS name
            Some(&TTYPE) if sb.get(1) == Some(&0) => {
                self.ttype = Some(sb[2..].iter().map(|&b| b as char).collect());
            }
            _ => {}
        }
    }

    fn strip(&mut self, s: &Sock, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in data {
            match self.state {
                0 if b == IAC => self.state = 1,
                0 if self.in_sb => self.sb.push(b),
                0 => out.push(b),
                1 => {
                    self.state = 0;
                    match b {
                        IAC if self.in_sb => self.sb.push(IAC),
                        IAC => out.push(IAC),
                        SB => {
                            self.in_sb = true;
                            self.sb.clear();
                        }
                        SE => {
                            self.in_sb = false;
                            self.end_sb();
                        }
                        DO | DONT | WILL | WONT => self.state = b,
                        _ => {}
                    }
                }
                cmd => {
                    self.state = 0;
                    // Refuse what we did not offer; ECHO and SGA are ours,
                    // the terminal type and window size the client's.
                    match cmd {
                        DO if b != 1 && b != 3 => {
                            let _ = s.send(&[IAC, WONT, b]);
                        }
                        WILL if b == TTYPE => {
                            let _ = s.send(&[IAC, SB, TTYPE, 1, IAC, SE]);
                        }
                        WILL if b != 3 && b != NAWS => {
                            let _ = s.send(&[IAC, DONT, b]);
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }
}

/// Read a line from the client, echoing it (`hide`: echo nothing).
fn read_client_line(s: &Sock, inp: &mut Input, hide: bool) -> Option<String> {
    let mut line = String::new();
    let mut buf = [0u8; 256];
    loop {
        let n = s.recv(&mut buf).ok().filter(|&n| n > 0)?;
        for b in inp.strip(s, &buf[..n]) {
            match b {
                b'\r' | b'\n' => {
                    let _ = s.send(b"\r\n");
                    return Some(line);
                }
                8 | 0x7f => {
                    if line.pop().is_some() && !hide {
                        let _ = s.send(b"\x08 \x08");
                    }
                }
                0 => {}
                c => {
                    line.push(c as char);
                    if !hide {
                        let _ = s.send(&[c]);
                    }
                }
            }
        }
    }
}

/// The screen bytes of code page `cp` as Unicode (CP 437 when unknown).
fn cp_table(cp: u16) -> [u16; 256] {
    *uls::codepage(cp).or_else(|| uls::codepage(437)).unwrap()
}

/// How screen bytes (code page `screen`) go to the client, and the
/// client's bytes back.
fn encodings(screen: u16) -> (Encoding, KeyDecoder) {
    let st = cp_table(screen);
    let client = CLIENT_CP.load(Ordering::Relaxed) as u16;
    let mut dec = KeyDecoder::new();
    if client == 0 {
        dec.table = Some(st);
        return (Encoding::utf8(&st), dec);
    }
    let ct = cp_table(client);
    let mut out = [b'?'; 256];
    let mut back = [0u8; 256];
    for i in 0..256usize {
        back[i] = i as u8;
        out[i] = if i < 0x80 || client == screen { i as u8 } else { (0x80..256usize).find(|&j| ct[j] == st[i]).map_or(b'?', |j| j as u8) };
    }
    if client != screen {
        for i in 0x80..256usize {
            back[i] = (0x80..256usize).find(|&j| st[j] == ct[i]).map_or(b'?', |j| j as u8);
        }
    }
    dec.in_table = Some(back);
    (Encoding::bytes(&out), dec)
}

struct Shared {
    sock: u32,
    sid: u32,
}

/// The pseudo terminal's screen to the client, as it changes.
unsafe extern "C" fn screen_pump(param: u32) {
    let p = unsafe { alloc::boxed::Box::from_raw(param as *mut Shared) };
    let s = Sock(p.sock);
    let mut r = Renderer::new();
    let mut st = [0u32; 32];
    let mut cells: Vec<u16> = alloc::vec![0; 200 * 100];
    let mut seen = u32::MAX;
    let mut enc: Option<(u16, Encoding)> = None;
    let mut out = Vec::with_capacity(8192);
    loop {
        match unsafe { Rs2PtyWait(p.sid, &mut seen, 5000) } {
            0 => {}
            640 => continue,
            _ => break,
        }
        // Let a burst of output settle into one update.
        unsafe { DosSleep(20) };
        if unsafe { Rs2VioReadScreen(p.sid, st.as_mut_ptr(), cells.as_mut_ptr(), (cells.len() * 2) as u32) } != 0 {
            break;
        }
        seen = st[0];
        let (cols, rows) = (st[1] as usize, st[2] as usize);
        let cp = st[8] as u16;
        if enc.as_ref().is_none_or(|e| e.0 != cp) {
            enc = Some((cp, encodings(cp).0));
            r.invalidate();
        }
        r.blink = st[9] & ST_BLINK != 0;
        let cursor = (st[9] & ST_CURSOR != 0).then_some((st[3] as usize, st[4] as usize));
        out.clear();
        r.update(cols, rows, &cells, cursor, &enc.as_ref().unwrap().1, &mut out);
        if out.is_empty() {
            continue;
        }
        let mut o = Vec::with_capacity(out.len() + 8);
        for &b in &out {
            if b == IAC {
                o.push(IAC);
            }
            o.push(b);
        }
        if s.send(&o).is_err() {
            break;
        }
    }
    // The session ended (the command processor exited): so does the
    // connection.
    unsafe { Rs2SockShutdown(p.sock, 2) };
    let _ = s.into_raw();
}

/// Serve the client from a pseudo terminal session. False if the kernel
/// could not start one.
fn serve_pty(s: &Sock, inp: &mut Input) -> bool {
    let comspec = env("COMSPEC").filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("C:\\OS2\\CMD.EXE"));
    let mut prog: Vec<u8> = comspec.bytes().collect();
    prog.push(0);
    let (w, h) = inp.naws.map_or((80, 25), |(w, h)| (w.clamp(40, 132) as u32, h.clamp(10, 60) as u32));
    let (mut sid, mut pid) = (0, 0);
    let rc = unsafe { Rs2PtyStart(prog.as_ptr(), core::ptr::null(), core::ptr::null(), w | h << 16, &mut sid, &mut pid) };
    if rc != 0 {
        outln(&alloc::format!("TELNETD: no pseudo terminal (error {rc}): using pipes"));
        return false;
    }
    if let Some(t) = &inp.ttype {
        outln(&alloc::format!("TELNETD: terminal {t}, {w}x{h}, session {sid}"));
    }
    let shared = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Shared { sock: s.0, sid }));
    if thread(screen_pump, shared as u32).is_err() {
        unsafe { Rs2VioControl(sid, CTL_STOP, 0) };
        return true;
    }
    let mut st = [0u32; 32];
    unsafe { Rs2VioReadScreen(sid, st.as_mut_ptr(), core::ptr::null_mut(), 0) };
    let mut dec = encodings(st[8] as u16).1;
    let mut buf = [0u8; 1024];
    loop {
        let n = match s.recv(&mut buf) {
            Ok(n) if n > 0 => n,
            _ => break,
        };
        let data = inp.strip(s, &buf[..n]);
        for k in dec.feed(&data) {
            let status = 0x40u32 | if k.ext { 0x02 } else { 0 };
            let ev = KeyEvent { kind: 1, flags: k.shift, a: k.ch as u32 | (k.scan as u32) << 8 | status << 16, b: 0, time: 0 };
            if unsafe { Rs2VioKey(sid, &ev) } != 0 {
                // The session is gone.
                return true;
            }
        }
    }
    unsafe { Rs2VioControl(sid, CTL_STOP, 0) };
    true
}

unsafe extern "C" fn serve(raw: u32) {
    let s = Sock(raw);
    let peer = s.peer();
    outln(&alloc::format!("TELNETD: connection from {}", ip(peer.addr)));
    let pipes = PIPES.load(Ordering::Relaxed);
    if pipes {
        let _ = s.send(&[IAC, WILL, 1, IAC, WILL, 3, IAC, DO, 3]);
    } else {
        let _ = s.send(&[IAC, WILL, 1, IAC, WILL, 3, IAC, DO, 3, IAC, DO, TTYPE, IAC, DO, NAWS]);
    }
    let _ = s.send_str(&alloc::format!("\r\n{} (rs2) TELNETD\r\n\r\n", hostname()));
    let mut inp = Input::new();
    if let Some(pw) = env("TELNET.PASSWORD").filter(|p| !p.is_empty()) {
        let _ = s.send(b"Password: ");
        match read_client_line(&s, &mut inp, true) {
            Some(l) if l == pw => {}
            _ => {
                let _ = s.send(b"Password incorrect.\r\n");
                outln(&alloc::format!("TELNETD: {}: wrong password", ip(peer.addr)));
                return;
            }
        }
    }
    if !pipes {
        // Give the client a moment to report its window size.
        let mut buf = [0u8; 256];
        let mut waited = 0;
        while inp.naws.is_none() && waited < 1000 && s.readable(100) {
            match s.recv(&mut buf) {
                Ok(n) if n > 0 => {
                    let _ = inp.strip(&s, &buf[..n]);
                }
                _ => return,
            }
            waited += 100;
        }
        if serve_pty(&s, &mut inp) {
            outln(&alloc::format!("TELNETD: {} disconnected", ip(peer.addr)));
            return;
        }
    }
    serve_pipes(&s, &mut inp);
    outln(&alloc::format!("TELNETD: {} disconnected", ip(peer.addr)));
}

/// The command processor on pipes: line editing here, standard output
/// only.
fn serve_pipes(s: &Sock, inp: &mut Input) {
    'session: loop {
        let Some(child) = spawn() else {
            let _ = s.send(b"Cannot start the command processor.\r\n");
            return;
        };
        let restarting = alloc::sync::Arc::new(AtomicBool::new(false));
        let p = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Pump { sock: s.0, pipe: child.stdout, restarting: restarting.clone() }));
        if thread(pump, p as u32).is_err() {
            return;
        }
        let mut line: Vec<u8> = Vec::new();
        let mut last = 0u8;
        let mut buf = [0u8; 1024];
        loop {
            let n = match s.recv(&mut buf) {
                Ok(n) if n > 0 => n,
                _ => {
                    unsafe {
                        DosKillProcess(0, child.pid);
                        DosClose(child.stdin);
                    }
                    break 'session;
                }
            };
            for b in inp.strip(s, &buf[..n]) {
                match b {
                    b'\r' | b'\n' => {
                        if b == b'\n' && last == b'\r' {
                            last = b;
                            continue;
                        }
                        let _ = s.send(b"\r\n");
                        line.extend_from_slice(b"\r\n");
                        let mut w = 0;
                        unsafe { DosWrite(child.stdin, line.as_ptr(), line.len() as u32, &mut w) };
                        line.clear();
                    }
                    8 | 0x7f => {
                        if line.pop().is_some() {
                            let _ = s.send(b"\x08 \x08");
                        }
                    }
                    3 => {
                        // Ctrl+C: a fresh command prompt.
                        let _ = s.send(b"^C\r\n");
                        restarting.store(true, Ordering::Release);
                        unsafe {
                            DosKillProcess(0, child.pid);
                            DosClose(child.stdin);
                        }
                        unsafe { DosSleep(200) };
                        continue 'session;
                    }
                    0 => {}
                    c => {
                        line.push(c);
                        let _ = s.send(&[c]);
                    }
                }
                last = b;
            }
        }
    }
}

fn usage() -> u32 {
    eprint("Usage: telnetd [-cp codepage] [-e escapedelay] [-p port] [-n] [-b] [-d] [-l]\r\n");
    1
}

fn main(st: &os2::Startup) -> u32 {
    let a = args(st);
    let mut port = service_port("telnet", "tcp", 23);
    let mut i = 0;
    let value = |i: &mut usize, rest: &str| -> String {
        if rest.is_empty() {
            *i += 1;
            a.get(*i).cloned().unwrap_or_default()
        } else {
            String::from(rest)
        }
    };
    while i < a.len() {
        let x = a[i].to_ascii_lowercase();
        if let Some(v) = x.strip_prefix("-cp") {
            let v = value(&mut i, v);
            match v.parse::<u16>() {
                Ok(n) if uls::codepage(n).is_some() => CLIENT_CP.store(n as u32, Ordering::Relaxed),
                _ if v.eq_ignore_ascii_case("utf8") || v == "1208" => CLIENT_CP.store(0, Ordering::Relaxed),
                _ => {
                    eprint(&alloc::format!("TELNETD: unknown code page {v}\r\n"));
                    return 1;
                }
            }
        } else if let Some(v) = x.strip_prefix("-p") {
            match value(&mut i, v).parse() {
                Ok(p) => port = p,
                Err(_) => return usage(),
            }
        } else if x == "-e" {
            i += 1;
        } else if x == "-n" {
            PIPES.store(true, Ordering::Relaxed);
        } else if !matches!(x.as_str(), "-b" | "-d" | "-l") {
            return usage();
        }
        i += 1;
    }
    let l = match Sock::tcp() {
        Ok(l) => l,
        Err(_) => return 2,
    };
    l.reuse();
    if let Err(e) = l.bind([0; 4], port).and_then(|_| l.listen(5)) {
        eprint(&alloc::format!("TELNETD: port {port}: {}\r\n", strerror(e)));
        return 2;
    }
    outln(&alloc::format!("TELNETD: listening on port {port}"));
    loop {
        match l.accept() {
            Ok((c, _)) => {
                let raw = c.into_raw();
                if thread(serve, raw).is_err() {
                    drop(Sock(raw));
                }
            }
            Err(_) => unsafe {
                DosSleep(1000);
            },
        }
    }
}
