//! FINGER: ask a host's finger server about its users (RFC 1288).
//! `FINGER [-l] [user]@host`.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use netapps::*;
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut long = false;
    let mut target = None;
    for x in &a {
        match x.as_str() {
            "-l" | "/l" | "-L" => long = true,
            "-?" | "/?" => target = None,
            _ => target = Some(x.clone()),
        }
    }
    let Some(t) = target else {
        eprint("Usage: finger [-?] [-l] [user]@<host>\r\n");
        return 1;
    };
    let (user, host) = match t.rsplit_once('@') {
        Some((u, h)) => (String::from(u), String::from(h)),
        None => (String::new(), t.clone()),
    };
    let port = service_port("finger", "tcp", 79);
    let (sock, addr) = match Sock::connect(&host, port) {
        Ok(v) => v,
        Err(e) => {
            eprint(&alloc::format!("finger: {host}: {e}\r\n"));
            return 2;
        }
    };
    outln(&alloc::format!("[{}]", os2::net::ip(addr)));
    let q = alloc::format!("{}{user}\r\n", if long { "/W " } else { "" });
    if sock.send_str(&q).is_err() {
        return 3;
    }
    lpd::copy_reply(&sock);
    0
}
