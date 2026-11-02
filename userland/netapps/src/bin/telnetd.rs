//! TELNETD: the telnet server. `TELNETD [-p port]` gives each client a
//! command prompt (%COMSPEC%, else CMD.EXE) on a thread of its own.
//!
//! The command processor runs with its standard input, output and error
//! on pipes: TELNETD echoes what the client types (it says WILL ECHO and
//! WILL SUPPRESS-GO-AHEAD), handles Backspace, passes each line on at
//! Enter and sends the output back with CR LF line ends. Programs that
//! write to the screen through VIO rather than their standard output are
//! not seen by the client. With TELNET.PASSWORD set in the environment
//! (IBM's TELNETD's password), clients must give it first. Ctrl+C ends
//! the program running and starts a new command prompt.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use netapps::*;
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

/// What the client typed, without the telnet commands.
struct Input {
    state: u8,
    in_sb: bool,
}

impl Input {
    fn strip(&mut self, s: &Sock, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in data {
            match self.state {
                0 if b == IAC => self.state = 1,
                0 if self.in_sb => {}
                0 => out.push(b),
                1 => {
                    self.state = 0;
                    match b {
                        IAC if !self.in_sb => out.push(IAC),
                        SB => self.in_sb = true,
                        SE => self.in_sb = false,
                        DO | DONT | WILL | WONT => self.state = b,
                        _ => {}
                    }
                }
                cmd => {
                    self.state = 0;
                    // Refuse what we did not offer; ECHO and SGA are ours.
                    match cmd {
                        DO if b != 1 && b != 3 => {
                            let _ = s.send(&[IAC, WONT, b]);
                        }
                        WILL if b != 24 && b != 31 => {
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

unsafe extern "C" fn serve(raw: u32) {
    let s = Sock(raw);
    let peer = s.peer();
    outln(&alloc::format!("TELNETD: connection from {}", ip(peer.addr)));
    let _ = s.send(&[IAC, WILL, 1, IAC, WILL, 3, IAC, DO, 3]);
    let _ = s.send_str(&alloc::format!("\r\n{} (rs2) TELNETD\r\n\r\n", hostname()));
    let mut inp = Input { state: 0, in_sb: false };
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
            for b in inp.strip(&s, &buf[..n]) {
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
    outln(&alloc::format!("TELNETD: {} disconnected", ip(peer.addr)));
}

fn main(st: &os2::Startup) -> u32 {
    let a = args(st);
    let mut port = service_port("telnet", "tcp", 23);
    let mut i = 0;
    while i < a.len() {
        let x = a[i].to_ascii_lowercase();
        if let Some(v) = x.strip_prefix("-p") {
            let v = if v.is_empty() {
                i += 1;
                a.get(i).cloned().unwrap_or_default()
            } else {
                String::from(v)
            };
            match v.parse() {
                Ok(p) => port = p,
                Err(_) => {
                    eprint("Usage: telnetd [-p port]\r\n");
                    return 1;
                }
            }
        } else if x == "-cp" || x == "-e" {
            i += 1;
        } else if !matches!(x.as_str(), "-b" | "-d" | "-l") {
            eprint("Usage: telnetd [-cp codepage] [-e escapedelay] [-p port] [-b] [-d] [-l]\r\n");
            return 1;
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
