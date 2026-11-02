//! RSH: run a command on a host's rsh server (port 514), which trusts
//! the user by name (from a reserved port, 512-1023).
//!
//! `RSH host [-l user] [-n] command`: the local user is %USER% (else
//! "user"), the remote one -l's or the same.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
use os2::net::*;
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut host = None;
    let mut ruser = None;
    let mut no_input = false;
    let mut cmd: Vec<String> = Vec::new();
    let mut i = 0;
    while i < a.len() {
        if !cmd.is_empty() {
            cmd.push(a[i].clone());
        } else {
            match a[i].as_str() {
                "-l" => {
                    i += 1;
                    ruser = a.get(i).cloned();
                }
                "-n" => no_input = true,
                x if host.is_none() => host = Some(String::from(x)),
                x => cmd.push(String::from(x)),
            }
        }
        i += 1;
    }
    let (Some(host), false) = (host, cmd.is_empty()) else {
        eprint("Usage: RSH host [-l user] [-n] command\r\n");
        return 1;
    };
    let luser = lpd::user();
    let ruser = ruser.unwrap_or_else(|| luser.clone());
    let addr = match lookup(&host) {
        Ok(a) => a,
        Err(e) => {
            eprint(&alloc::format!("RSH: {e}\r\n"));
            return 2;
        }
    };
    let port = service_port("shell", "tcp", 514);
    let mut sock = None;
    for lp in (512..1024u16).rev() {
        let Ok(s) = Sock::tcp() else { break };
        s.reuse();
        if s.bind([0; 4], lp).is_err() {
            continue;
        }
        let sa = SockAddrIn::new(addr, port);
        match unsafe { Rs2SockConnect(s.0, &sa, 16) } {
            0 => {
                sock = Some(s);
                break;
            }
            48 | 49 => continue,
            e => {
                eprint(&alloc::format!("RSH: {host}: {}\r\n", strerror(e)));
                return 2;
            }
        }
    }
    let Some(sock) = sock else {
        eprint("RSH: no reserved port is free\r\n");
        return 2;
    };
    let mut req = Vec::new();
    for f in ["0", &luser, &ruser, &cmd.join(" ")] {
        req.extend_from_slice(f.as_bytes());
        req.push(0);
    }
    if sock.send(&req).is_err() {
        return 3;
    }
    run_remote(&sock, no_input, "RSH")
}
