//! REXECD: the remote execution server (port 512), for REXEC clients and
//! the `rexec()` call.
//!
//! `REXECD [-t seconds | -1] [-p port]`, as IBM's: the client sends the
//! port for standard error (0: none), the user, the password and the
//! command, each NUL-terminated. The user and password must be those of
//! a `user:` line of %ETC%\TRUSERS (FTPD's users) or, when TRUSERS does
//! not name the user, the USER and PASSWORD environment variables (as
//! IBM's REXECD checks them). The command runs under %COMSPEC% /c with
//! the connection as its standard input and output (standard error on the
//! second connection when one was asked for); it is ended after the time
//! given with -t (180 seconds; -1: no limit).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};
use netapps::rcmd;
use netapps::*;
use os2::net::*;
use os2::*;
os2::entry!(main);

static TIMEOUT: AtomicU32 = AtomicU32::new(180);

fn check(user: &str, pass: &str) -> bool {
    if let Some(p) = rcmd::trusers_password(user) {
        return p == pass;
    }
    match (env("USER"), env("PASSWORD")) {
        (Some(u), Some(p)) if !u.is_empty() => u.eq_ignore_ascii_case(user) && p == pass,
        _ => false,
    }
}

unsafe extern "C" fn serve(raw: u32) {
    let s = Sock(raw);
    let peer = s.peer();
    s.set_timeout(60_000);
    let Some(port) = rcmd::read_z(&s, 8) else { return };
    let port: u16 = port.trim().parse().unwrap_or(0);
    let err = if port != 0 {
        match rcmd::stderr_conn(peer.addr, port, false) {
            Some(e) => Some(e),
            None => {
                let _ = s.send(b"\x01can't connect to the standard error port\n");
                return;
            }
        }
    } else {
        None
    };
    let (Some(user), Some(pass), Some(cmd)) = (rcmd::read_z(&s, 64), rcmd::read_z(&s, 64), rcmd::read_z(&s, 4096)) else { return };
    s.set_timeout(0);
    if !check(&user, &pass) {
        outln(&alloc::format!("REXECD: login of {user} from {} refused", ip(peer.addr)));
        let _ = s.send(b"\x01Login incorrect.\n");
        return;
    }
    outln(&alloc::format!("REXECD: {user}@{}: {cmd}", ip(peer.addr)));
    let extra = [alloc::format!("USER={user}")];
    if !rcmd::run(&s, err.as_ref(), &cmd, &extra, TIMEOUT.load(Ordering::Relaxed)) {
        let _ = s.send(b"\x01Cannot start the command processor.\n");
    }
    unsafe { Rs2SockShutdown(s.0, 2) };
}

fn main(st: &os2::Startup) -> u32 {
    let a = args(st);
    let mut port = service_port("exec", "tcp", 512);
    let mut i = 0;
    while i < a.len() {
        match a[i].to_ascii_lowercase().as_str() {
            "-t" => {
                i += 1;
                match a.get(i).and_then(|v| v.parse::<i32>().ok()) {
                    Some(t) if t > 0 => TIMEOUT.store(t as u32, Ordering::Relaxed),
                    Some(-1) => TIMEOUT.store(0, Ordering::Relaxed),
                    _ => {
                        eprint("Usage: rexecd [-t timeout | -1] [-p port]\r\n");
                        return 1;
                    }
                }
            }
            "-1" => TIMEOUT.store(0, Ordering::Relaxed),
            "-p" => {
                i += 1;
                port = a.get(i).and_then(|v| v.parse().ok()).unwrap_or(port);
            }
            _ => {
                eprint("Usage: rexecd [-t timeout | -1] [-p port]\r\n");
                return 1;
            }
        }
        i += 1;
    }
    let l = match Sock::tcp() {
        Ok(l) => l,
        Err(_) => return 2,
    };
    l.reuse();
    if let Err(e) = l.bind([0; 4], port).and_then(|_| l.listen(5)) {
        eprint(&alloc::format!("REXECD: port {port}: {}\r\n", strerror(e)));
        return 2;
    }
    outln(&alloc::format!("REXECD: listening on port {port}"));
    let _ = String::new();
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
