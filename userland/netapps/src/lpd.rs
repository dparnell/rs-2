//! The line printer daemon protocol (RFC 1179) for LPR, LPQ and LPRM.
//!
//! The printer and server come from `-p printer` and `-s server` (also
//! written `-pprinter`, `-sserver`, or `printer@server`), else from the
//! LPR_PRINTER and LPR_SERVER environment variables, as IBM's programs
//! take them.

use alloc::string::String;
use alloc::vec::Vec;

use crate::*;

pub struct Target {
    pub printer: String,
    pub server: String,
    pub port: u16,
}

/// Options shared by the three programs; the other arguments are returned.
pub fn parse(a: &[String], flags_with_value: &[&str]) -> Result<(Target, Vec<(String, String)>, Vec<String>), String> {
    let mut printer = None;
    let mut server = None;
    let mut port = 515u16;
    let mut opts = Vec::new();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < a.len() {
        let s = &a[i];
        let lower = s.to_ascii_lowercase();
        if (lower.starts_with('-') || lower.starts_with('/')) && s.len() >= 2 {
            let flag = &lower[1..2];
            let attached = &s[2..];
            let value = |i: &mut usize| -> Result<String, String> {
                if !attached.is_empty() {
                    Ok(String::from(attached))
                } else if *i + 1 < a.len() {
                    *i += 1;
                    Ok(a[*i].clone())
                } else {
                    Err(alloc::format!("{s}: value missing"))
                }
            };
            match flag {
                "p" | "q" => printer = Some(value(&mut i)?),
                "s" | "h" => server = Some(value(&mut i)?),
                "?" => return Err(String::new()),
                f if flags_with_value.contains(&f) => {
                    let v = value(&mut i)?;
                    opts.push((String::from(f), v));
                }
                f => opts.push((String::from(f), String::from(attached))),
            }
        } else {
            rest.push(s.clone());
        }
        i += 1;
    }
    // `printer@server` as an argument of its own.
    let printer = printer.or_else(|| {
        let i = rest.iter().position(|r| r.contains('@') && !r.starts_with('@'))?;
        Some(rest.remove(i))
    });
    let mut printer = printer.or_else(|| env("LPR_PRINTER")).unwrap_or_default();
    let mut server = server.or_else(|| env("LPR_SERVER")).unwrap_or_default();
    if let Some((p, s)) = printer.clone().split_once('@') {
        printer = String::from(p);
        server = String::from(s);
    }
    // rs2's own: server:port reaches an LPD on another port.
    if let Some((s, p)) = server.clone().rsplit_once(':')
        && let Ok(n) = p.parse()
    {
        server = String::from(s);
        port = n;
    }
    if printer.is_empty() {
        return Err(String::from("no printer: give -p printer or set LPR_PRINTER"));
    }
    if server.is_empty() {
        return Err(String::from("no server: give -s server or set LPR_SERVER"));
    }
    Ok((Target { printer, server, port }, opts, rest))
}

/// Connect to the server's LPD from a reserved port (721-731) when one is
/// free, as RFC 1179 asks.
pub fn connect(t: &Target) -> Result<Sock, String> {
    let addr = lookup(&t.server)?;
    // Start at another port each time: the server may still hold the last
    // connection from a port (TIME_WAIT) and refuse the same one again.
    let first = (ms() / 100 % 11) as u16;
    match connect_from(addr, t.port, (0..11u16).map(|k| 721 + (first + k) % 11)) {
        Ok(s) => Ok(s),
        // No reserved port: any port (servers that do not insist).
        Err(e) if e != 61 => Sock::connect_addr(addr, t.port).map_err(|e| alloc::format!("{}: {}", t.server, strerror(e))),
        Err(e) => Err(alloc::format!("{}: {}", t.server, strerror(e))),
    }
}

/// Wait for the one-byte acknowledgement (0 = accepted).
pub fn ack(s: &Sock) -> Result<(), String> {
    let mut b = [0u8; 1];
    match s.recv(&mut b) {
        Ok(1) if b[0] == 0 => Ok(()),
        Ok(1) => Err(alloc::format!("the server refused the request (code {})", b[0])),
        Ok(_) => Err(String::from("the server closed the connection")),
        Err(e) => Err(String::from(strerror(e))),
    }
}

/// The user name sent to the server: %USER%, %LOGNAME%, else "user".
pub fn user() -> String {
    env("USER").or_else(|| env("LOGNAME")).filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("user"))
}

/// Print what the server says until it closes the connection.
pub fn copy_reply(s: &Sock) {
    let mut buf = [0u8; 4096];
    let mut last = 0u8;
    while let Ok(n) = s.recv(&mut buf) {
        if n == 0 {
            break;
        }
        let mut o = Vec::new();
        to_crlf(&buf[..n], &mut o, &mut last);
        os2::write(&o);
    }
}
