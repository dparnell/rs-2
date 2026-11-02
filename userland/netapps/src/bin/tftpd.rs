//! TFTPD: the trivial file transfer server (RFC 1350; RFC 2348 block sizes).
//!
//! `TFTPD [path] [-l] [-n] [-t timeout_seconds] [-b maximum_block_size]
//! [-p port]`: files are read from and written to the directory `path`
//! (the current one by default; names may not leave it), each request on
//! a thread of its own. -l logs the requests, -n refuses block size
//! negotiation.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::tftp::*;
use netapps::*;
use os2::net::*;
os2::entry!(main);

struct Config {
    root: String,
    log: bool,
    negotiate: bool,
    timeout: u32,
    max_block: usize,
}

static mut CONFIG: Option<Config> = None;

fn cfg() -> &'static Config {
    unsafe { (*core::ptr::addr_of!(CONFIG)).as_ref().unwrap() }
}

struct Request {
    pkt: Vec<u8>,
    from: SockAddrIn,
}

unsafe extern "C" fn serve(param: u32) {
    let r = unsafe { Box::from_raw(param as *mut Request) };
    let c = cfg();
    let Ok(s) = Sock::udp() else { return };
    let _ = s.bind([0; 4], 0);
    let to = r.from;
    let opcode = op(&r.pkt);
    let st = strings(&r.pkt);
    let file = st.first().cloned().unwrap_or_default();
    let mode = st.get(1).map(|m| m.to_ascii_lowercase()).unwrap_or_default();
    let binary = mode == "octet";
    if c.log {
        outln(&alloc::format!("TFTPD: {} {file} ({mode}) from {}:{}", if opcode == RRQ { "read" } else { "write" }, ip(to.addr), to.port()));
    }
    let name: String = file.chars().map(|ch| if ch == '/' { '\\' } else { ch }).collect();
    let name = name.trim_start_matches('\\');
    if name.is_empty() || name.split('\\').any(|p| p == "..") || name.contains(':') || !(binary || mode == "netascii") {
        let _ = s.send_to(&error(2, "Access violation"), to);
        return;
    }
    let path = alloc::format!("{}\\{name}", c.root.trim_end_matches('\\'));
    let mut blk = 512usize;
    let mut oack = Vec::new();
    if c.negotiate
        && let Some(i) = st.iter().position(|k| k.eq_ignore_ascii_case("blksize"))
        && let Some(v) = st.get(i + 1).and_then(|v| v.parse::<usize>().ok())
    {
        blk = v.clamp(8, c.max_block);
        oack = alloc::vec![0, OACK as u8];
        oack.extend_from_slice(b"blksize\0");
        oack.extend_from_slice(alloc::format!("{blk}").as_bytes());
        oack.push(0);
    }
    let (rexmt, total) = (2000, c.timeout);
    if opcode == RRQ {
        let Some(raw) = read_file(&path) else {
            let _ = s.send_to(&error(1, "File not found"), to);
            return;
        };
        let data = if binary { raw } else { to_netascii(&raw) };
        if !oack.is_empty() && exchange(&s, &oack, to, false, rexmt, total, &|p| op(p) == ACK && num(p) == 0).is_none() {
            return;
        }
        let mut block = 1u16;
        let mut off = 0;
        loop {
            let end = (off + blk).min(data.len());
            let p = netapps::tftp::data(block, &data[off..end]);
            let want = block;
            match exchange(&s, &p, to, false, rexmt, total, &|p| op(p) == ACK && num(p) == want) {
                Some((a, _)) if op(&a) == ACK => {}
                _ => return,
            }
            if end - off < blk {
                break;
            }
            off = end;
            block = block.wrapping_add(1);
        }
    } else {
        let hf = match create_file(&path, false) {
            Ok(h) => h,
            Err(_) => {
                let _ = s.send_to(&error(2, "Access violation"), to);
                return;
            }
        };
        let mut out = Vec::new();
        let first = if oack.is_empty() { ack(0).to_vec() } else { oack };
        let mut reply = first;
        let mut expect = 1u16;
        loop {
            let want = expect;
            let Some((p, _)) = exchange(&s, &reply, to, false, rexmt, total, &|p| op(p) == DATA && num(p) == want) else { break };
            if op(&p) != DATA {
                break;
            }
            let d = &p[4..];
            out.extend_from_slice(d);
            reply = ack(expect).to_vec();
            if d.len() < blk {
                let _ = s.send_to(&reply, to);
                let data = if binary { out } else { from_netascii(&out) };
                let _ = write_file(hf, &data);
                if c.log {
                    outln(&alloc::format!("TFTPD: {file}: {} bytes received", data.len()));
                }
                break;
            }
            expect = expect.wrapping_add(1);
        }
        unsafe { os2::DosClose(hf) };
    }
}

fn usage() -> u32 {
    eprint("Syntax: TFTPD [path] [-l] [-n] [-t timeout_seconds] [-b maximum_block_size] [-p port]\r\n");
    1
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut c = Config { root: current_dir(), log: false, negotiate: true, timeout: 10000, max_block: 8192 };
    let mut port = 69;
    let mut i = 0;
    while i < a.len() {
        let x = a[i].to_ascii_lowercase();
        let val = |i: &mut usize| {
            *i += 1;
            a.get(*i).and_then(|v| v.parse::<u32>().ok())
        };
        match x.as_str() {
            "-l" => c.log = true,
            "-n" => c.negotiate = false,
            "-s" => {}
            "-t" => match val(&mut i) {
                Some(t) => c.timeout = t.max(1) * 1000,
                None => return usage(),
            },
            "-b" => match val(&mut i) {
                Some(b) => c.max_block = (b as usize).clamp(512, 65464),
                None => return usage(),
            },
            "-p" => match val(&mut i) {
                Some(p) => port = p as u16,
                None => return usage(),
            },
            "-cp" | "-a" | "-mn" | "-mx" => i += 1,
            "-?" | "/?" => return usage(),
            _ if !x.starts_with('-') => c.root = a[i].clone(),
            _ => return usage(),
        }
        i += 1;
    }
    let sock = match Sock::udp() {
        Ok(s) => s,
        Err(_) => return 2,
    };
    sock.reuse();
    if let Err(e) = sock.bind([0; 4], port) {
        eprint(&alloc::format!("TFTPD: port {port}: {}\r\n", strerror(e)));
        return 2;
    }
    outln(&alloc::format!("TFTPD: serving {} on port {port}", c.root));
    unsafe { CONFIG = Some(c) };
    let mut buf = alloc::vec![0u8; 2048];
    loop {
        let Ok((n, from)) = sock.recv_from(&mut buf) else {
            unsafe { os2::DosSleep(100) };
            continue;
        };
        let pkt = buf[..n].to_vec();
        match op(&pkt) {
            RRQ | WRQ => {
                let r = Box::into_raw(Box::new(Request { pkt, from }));
                if thread(serve, r as u32).is_err() {
                    drop(unsafe { Box::from_raw(r) });
                }
            }
            _ => {
                let _ = sock.send_to(&error(4, "Illegal TFTP operation"), from);
            }
        }
    }
}
