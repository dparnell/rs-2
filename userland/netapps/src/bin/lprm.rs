//! LPRM: remove jobs from a remote printer's queue (RFC 1179).
//! `LPRM [-p printer] [-s server] [-a] [job ...]`: -a (or `-`) removes
//! all of the user's jobs.
#![no_std]
#![no_main]
extern crate alloc;
use netapps::*;
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let (t, opts, mut rest) = match lpd::parse(&a, &[]) {
        Ok(v) => v,
        Err(e) => {
            if !e.is_empty() {
                eprint(&alloc::format!("LPRM: {e}\r\n"));
            }
            eprint("Usage: LPRM [-p printer] [-s server] [-a | job ...]\r\n");
            return 1;
        }
    };
    let user = lpd::user();
    if opts.iter().any(|(f, _)| f == "a") || rest.iter().any(|r| r == "-") {
        rest.retain(|r| r != "-");
        rest.push(user.clone());
    }
    let sock = match lpd::connect(&t) {
        Ok(s) => s,
        Err(e) => {
            eprint(&alloc::format!("LPRM: {e}\r\n"));
            return 3;
        }
    };
    let mut req = alloc::format!("\x05{} {user}", t.printer);
    for r in &rest {
        req.push(' ');
        req.push_str(r);
    }
    req.push('\n');
    if let Err(e) = sock.send_str(&req) {
        eprint(&alloc::format!("LPRM: {}\r\n", os2::net::strerror(e)));
        return 4;
    }
    lpd::copy_reply(&sock);
    0
}
