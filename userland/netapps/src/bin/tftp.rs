//! TFTP: the trivial file transfer client (RFC 1350; RFC 2348 block sizes).
//!
//! `TFTP [-b block-size] host-name [port]`, then commands: connect, mode,
//! ascii, binary, get, put, verbose, trace, status, rexmt, timeout,
//! blksize, quit, ?. `get host:file` and `put file host:file` name the
//! host in the file name.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::tftp::*;
use netapps::*;
use os2::net::*;
os2::entry!(main);

struct Client {
    host: Option<([u8; 4], String)>,
    port: u16,
    binary: bool,
    verbose: bool,
    trace: bool,
    rexmt: u32,
    timeout: u32,
    blksize: u16,
}

impl Client {
    fn mode(&self) -> &'static str {
        if self.binary { "octet" } else { "netascii" }
    }

    fn opts(&self) -> Vec<(&'static str, String)> {
        if self.blksize != 512 { alloc::vec![("blksize", alloc::format!("{}", self.blksize))] } else { Vec::new() }
    }

    fn target(&self, name: &str) -> Option<([u8; 4], String)> {
        if let Some((h, f)) = name.split_once(':')
            && h.len() > 1
        {
            return match lookup(h) {
                Ok(a) => Some((a, String::from(f))),
                Err(e) => {
                    outln(&e);
                    None
                }
            };
        }
        match &self.host {
            Some((a, _)) => Some((*a, String::from(name))),
            None => {
                outln("No target machine specified.");
                None
            }
        }
    }

    fn tr(&self, dir: &str, p: &[u8]) {
        if self.trace {
            let what = match op(p) {
                RRQ => "RRQ",
                WRQ => "WRQ",
                DATA => "DATA",
                ACK => "ACK",
                ERROR => "ERROR",
                OACK => "OACK",
                _ => "?",
            };
            outln(&alloc::format!("{dir} {what} <block={}, {} bytes>", num(p), p.len()));
        }
    }

    fn get(&self, remote: &str, local: &str) {
        let Some((addr, file)) = self.target(remote) else { return };
        let Ok(s) = Sock::udp() else { return };
        let _ = s.bind([0; 4], 0);
        let mut blk = 512usize;
        let req = request(RRQ, &file, self.mode(), &self.opts());
        self.tr("sent", &req);
        let start = ms();
        let first = exchange(&s, &req, SockAddrIn::new(addr, self.port), true, self.rexmt, self.timeout, &|p| (op(p) == DATA && num(p) == 1) || op(p) == OACK);
        let Some((mut pkt, tid)) = first else {
            outln("Transfer timed out.");
            return;
        };
        let mut out = Vec::new();
        let mut expect = 1u16;
        loop {
            self.tr("received", &pkt);
            match op(&pkt) {
                ERROR => {
                    outln(&error_text(&pkt));
                    return;
                }
                OACK => {
                    let s2 = strings(&pkt);
                    if let Some(i) = s2.iter().position(|k| k.eq_ignore_ascii_case("blksize")) {
                        blk = s2.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(512);
                    }
                    let a = ack(0);
                    self.tr("sent", &a);
                    match exchange(&s, &a, tid, false, self.rexmt, self.timeout, &|p| op(p) == DATA && num(p) == 1) {
                        Some((p, _)) => pkt = p,
                        None => {
                            outln("Transfer timed out.");
                            return;
                        }
                    }
                    continue;
                }
                _ => {}
            }
            let d = &pkt[4..];
            out.extend_from_slice(d);
            let a = ack(expect);
            let last = d.len() < blk;
            if last {
                self.tr("sent", &a);
                let _ = s.send_to(&a, tid);
                break;
            }
            expect = expect.wrapping_add(1);
            let want = expect;
            self.tr("sent", &a);
            match exchange(&s, &a, tid, false, self.rexmt, self.timeout, &|p| op(p) == DATA && num(p) == want) {
                Some((p, _)) => pkt = p,
                None => {
                    outln("Transfer timed out.");
                    return;
                }
            }
        }
        let data = if self.binary { out } else { from_netascii(&out) };
        match create_file(local, false) {
            Ok(hf) => {
                let _ = write_file(hf, &data);
                unsafe { os2::DosClose(hf) };
                outln(&alloc::format!("Received {} bytes in {}", data.len(), secs(start)));
            }
            Err(e) => outln(&alloc::format!("{local}: cannot create (error {e})")),
        }
    }

    fn put(&self, local: &str, remote: &str) {
        let Some((addr, file)) = self.target(remote) else { return };
        let Some(raw) = read_file(local) else {
            outln(&alloc::format!("{local}: cannot open"));
            return;
        };
        let data = if self.binary { raw } else { to_netascii(&raw) };
        let Ok(s) = Sock::udp() else { return };
        let _ = s.bind([0; 4], 0);
        let mut blk = 512usize;
        let req = request(WRQ, &file, self.mode(), &self.opts());
        self.tr("sent", &req);
        let start = ms();
        let Some((pkt, tid)) = exchange(&s, &req, SockAddrIn::new(addr, self.port), true, self.rexmt, self.timeout, &|p| (op(p) == ACK && num(p) == 0) || op(p) == OACK) else {
            outln("Transfer timed out.");
            return;
        };
        self.tr("received", &pkt);
        match op(&pkt) {
            ERROR => {
                outln(&error_text(&pkt));
                return;
            }
            OACK => {
                let s2 = strings(&pkt);
                if let Some(i) = s2.iter().position(|k| k.eq_ignore_ascii_case("blksize")) {
                    blk = s2.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(512);
                }
            }
            _ => {}
        }
        let mut block = 1u16;
        let mut off = 0usize;
        loop {
            let end = (off + blk).min(data.len());
            let p = netapps::tftp::data(block, &data[off..end]);
            self.tr("sent", &p);
            let want = block;
            match exchange(&s, &p, tid, false, self.rexmt, self.timeout, &|p| op(p) == ACK && num(p) == want) {
                Some((a, _)) if op(&a) == ERROR => {
                    outln(&error_text(&a));
                    return;
                }
                Some((a, _)) => self.tr("received", &a),
                None => {
                    outln("Transfer timed out.");
                    return;
                }
            }
            if end - off < blk {
                break;
            }
            off = end;
            block = block.wrapping_add(1);
        }
        outln(&alloc::format!("Sent {} bytes in {}", data.len(), secs(start)));
    }

    fn command(&mut self, line: &str) -> bool {
        let w = utils::split(line);
        let Some(c) = w.first() else { return true };
        match c.to_ascii_lowercase().as_str() {
            "connect" => match w.get(1) {
                Some(h) => match lookup(h) {
                    Ok(a) => {
                        self.host = Some((a, h.clone()));
                        if let Some(p) = w.get(2).and_then(|p| p.parse().ok()) {
                            self.port = p;
                        }
                    }
                    Err(e) => outln(&e),
                },
                None => outln("usage: connect host-name [port]"),
            },
            "mode" => match w.get(1).map(|m| m.to_ascii_lowercase()) {
                Some(m) if m == "binary" || m == "octet" => self.binary = true,
                Some(m) if m == "ascii" || m == "netascii" => self.binary = false,
                _ => outln(&alloc::format!("Using {} mode to transfer files.", self.mode())),
            },
            "binary" | "octet" => self.binary = true,
            "ascii" | "netascii" => self.binary = false,
            "verbose" => {
                self.verbose = !self.verbose;
                outln(&alloc::format!("Verbose mode {}.", if self.verbose { "on" } else { "off" }));
            }
            "trace" => {
                self.trace = !self.trace;
                outln(&alloc::format!("Packet tracing {}.", if self.trace { "on" } else { "off" }));
            }
            "status" => {
                match &self.host {
                    Some((a, h)) => outln(&alloc::format!("Connected to {h} ({}), port {}.", ip(*a), self.port)),
                    None => outln("Not connected."),
                }
                outln(&alloc::format!("Mode: {} Verbose: {} Tracing: {} Block size: {}", self.mode(), self.verbose, self.trace, self.blksize));
                outln(&alloc::format!("Rexmt-interval: {} seconds, Max-timeout: {} seconds", self.rexmt / 1000, self.timeout / 1000));
            }
            "rexmt" => match w.get(1).and_then(|v| v.parse::<u32>().ok()) {
                Some(v) if v > 0 => self.rexmt = v * 1000,
                _ => outln("usage: rexmt value"),
            },
            "timeout" => match w.get(1).and_then(|v| v.parse::<u32>().ok()) {
                Some(v) if v > 0 => self.timeout = v * 1000,
                _ => outln("usage: Maximum-timeout value"),
            },
            "blksize" => match w.get(1).and_then(|v| v.parse::<u16>().ok()) {
                Some(v) if (8..=65464).contains(&v) => self.blksize = v,
                _ => outln("usage: blksize value"),
            },
            "get" => match (w.get(1), w.get(2)) {
                (Some(r), l) => {
                    let local = l.cloned().unwrap_or_else(|| String::from(r.rsplit([':', '/', '\\']).next().unwrap_or(r)));
                    self.get(r, &local);
                }
                _ => outln("usage: get host:file host:file ... file, or get file"),
            },
            "put" => match (w.get(1), w.get(2)) {
                (Some(l), r) => {
                    let remote = r.cloned().unwrap_or_else(|| String::from(l.rsplit(['\\', ':']).next().unwrap_or(l)));
                    self.put(l, &remote);
                }
                _ => outln("usage: put file ... host:target, or put file"),
            },
            "quit" | "bye" | "exit" => return false,
            "?" | "help" => {
                outln("connect   connect to remote tftp");
                outln("mode      set file transfer mode");
                outln("put       send file");
                outln("get       receive file");
                outln("quit      exit tftp");
                outln("verbose   toggle verbose mode");
                outln("trace     toggle packet tracing");
                outln("status    show current status");
                outln("binary    set mode to octet");
                outln("ascii     set mode to netascii");
                outln("rexmt     set per-packet retransmission timeout");
                outln("timeout   set total retransmission timeout");
                outln("blksize   set the block size asked for");
            }
            _ => outln("?Invalid command"),
        }
        true
    }
}

fn secs(start: u32) -> String {
    let t = ms().wrapping_sub(start);
    alloc::format!("{}.{:01} seconds", t / 1000, t % 1000 / 100)
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut c = Client { host: None, port: 69, binary: false, verbose: false, trace: false, rexmt: 5000, timeout: 25000, blksize: 512 };
    let mut rest = Vec::new();
    let mut i = 0;
    while i < a.len() {
        match a[i].to_ascii_lowercase().as_str() {
            "-b" => {
                i += 1;
                match a.get(i).and_then(|v| v.parse::<u16>().ok()) {
                    Some(b) if b >= 8 => c.blksize = b,
                    _ => {
                        eprint("usage: tftp [-b block-size] host-name [port]\r\n");
                        return 1;
                    }
                }
            }
            "-cp" => i += 1,
            _ => rest.push(a[i].clone()),
        }
        i += 1;
    }
    if let Some(h) = rest.first() {
        let mut l = alloc::format!("connect {h}");
        if let Some(p) = rest.get(1) {
            l.push(' ');
            l.push_str(p);
        }
        c.command(&l);
    }
    while let Some(line) = read_line("tftp> ") {
        if !c.command(&line) {
            break;
        }
    }
    0
}
