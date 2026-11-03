//! RSHD: the remote shell server (port 514), for RSH clients.
//!
//! `RSHD [-t seconds | -1] [-p port]`, as IBM's: the client connects from
//! a reserved port (below 1024) and sends the port for standard error (0:
//! none), the remote user, the local user and the command, each
//! NUL-terminated. %ETC%\RHOSTS decides who may run commands: a line
//! `host [user]` lets that host (name or address; `+` any host) in, as
//! any user or only the user given. The command runs under %COMSPEC% /c
//! with USER set to the local user and the connection as its standard
//! input and output (standard error on the second connection, made from
//! a reserved port); it is ended after the time given with -t (180
//! seconds; -1: no limit).
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

/// Whether RHOSTS lets `ruser` at `peer` in.
fn trusted(peer: [u8; 4], ruser: &str) -> bool {
    let Some(d) = read_file(&etc_path("RHOSTS")) else { return false };
    for l in utils::lines(&d) {
        let l = l.split('#').next().unwrap_or("").trim();
        let mut w = l.split_whitespace();
        let Some(host) = w.next() else { continue };
        let host_ok = host == "+" || host == ip(peer) || lookup(host).is_ok_and(|a| a == peer);
        let user_ok = w.next().is_none_or(|u| u == "+" || u.eq_ignore_ascii_case(ruser));
        if host_ok && user_ok {
            return true;
        }
    }
    false
}

unsafe extern "C" fn serve(raw: u32) {
    let s = Sock(raw);
    let peer = s.peer();
    if peer.port() >= 1024 {
        outln(&alloc::format!("RSHD: {}: port {} is not reserved", ip(peer.addr), peer.port()));
        let _ = s.send(b"\x01Permission denied.\n");
        return;
    }
    s.set_timeout(60_000);
    let Some(port) = rcmd::read_z(&s, 8) else { return };
    let port: u16 = port.trim().parse().unwrap_or(0);
    let err = if port != 0 {
        match rcmd::stderr_conn(peer.addr, port, true) {
            Some(e) => Some(e),
            None => {
                let _ = s.send(b"\x01can't connect to the standard error port\n");
                return;
            }
        }
    } else {
        None
    };
    let (Some(ruser), Some(luser), Some(cmd)) = (rcmd::read_z(&s, 64), rcmd::read_z(&s, 64), rcmd::read_z(&s, 4096)) else { return };
    s.set_timeout(0);
    if !trusted(peer.addr, &ruser) {
        outln(&alloc::format!("RSHD: request \"{cmd}\" from {ruser}@{} refused", ip(peer.addr)));
        let _ = s.send(b"\x01Permission denied.\n");
        return;
    }
    outln(&alloc::format!("RSHD: {ruser}@{} as {luser}: {cmd}", ip(peer.addr)));
    let extra = [alloc::format!("USER={luser}")];
    if !rcmd::run(&s, err.as_ref(), &cmd, &extra, TIMEOUT.load(Ordering::Relaxed)) {
        let _ = s.send(b"\x01Cannot start the command processor.\n");
    }
    unsafe { Rs2SockShutdown(s.0, 2) };
}

fn main(st: &os2::Startup) -> u32 {
    let a = args(st);
    let mut port = service_port("shell", "tcp", 514);
    let mut i = 0;
    while i < a.len() {
        match a[i].to_ascii_lowercase().as_str() {
            "-t" => {
                i += 1;
                match a.get(i).and_then(|v| v.parse::<i32>().ok()) {
                    Some(t) if t > 0 => TIMEOUT.store(t as u32, Ordering::Relaxed),
                    Some(-1) => TIMEOUT.store(0, Ordering::Relaxed),
                    _ => {
                        eprint("Usage: rshd [-t timeout | -1] [-p port]\r\n");
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
                eprint("Usage: rshd [-t timeout | -1] [-p port]\r\n");
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
        eprint(&alloc::format!("RSHD: port {port}: {}\r\n", strerror(e)));
        return 2;
    }
    if read_file(&etc_path("RHOSTS")).is_none() {
        outln(&alloc::format!("RSHD: {} not found: nobody may run commands", etc_path("RHOSTS")));
    }
    outln(&alloc::format!("RSHD: listening on port {port}"));
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
