//! LPQ: show the queue of a remote printer (RFC 1179).
//! `LPQ [-p printer] [-s server] [-l] [job | user ...]`.
#![no_std]
#![no_main]
extern crate alloc;
use netapps::*;
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let (t, opts, rest) = match lpd::parse(&a, &[]) {
        Ok(v) => v,
        Err(e) => {
            if !e.is_empty() {
                eprint(&alloc::format!("LPQ: {e}\r\n"));
            }
            eprint("Usage: LPQ [-p printer] [-s server] [-l] [job | user ...]\r\n");
            return 1;
        }
    };
    let long = opts.iter().any(|(f, _)| f == "l");
    let sock = match lpd::connect(&t) {
        Ok(s) => s,
        Err(e) => {
            eprint(&alloc::format!("LPQ: {e}\r\n"));
            return 3;
        }
    };
    let mut req = alloc::format!("{}{}", if long { '\x04' } else { '\x03' }, t.printer);
    for r in &rest {
        req.push(' ');
        req.push_str(r);
    }
    req.push('\n');
    if let Err(e) = sock.send_str(&req) {
        eprint(&alloc::format!("LPQ: {}\r\n", os2::net::strerror(e)));
        return 4;
    }
    lpd::copy_reply(&sock);
    0
}
