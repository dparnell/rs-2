//! Network test for rs2's kernel socket interface (RS2API 200+).
//!
//! `NETTEST [section...] [-host IP] [-port N] [-listen N] [-name NAME]
//! [-wait S]` runs the sections (default: all) and prints PASS/FAIL per
//! check, then "nettest done: N failure(s)"; the exit code is N.
//!
//! * `config`: wait for an interface address (DHCP), print every interface.
//! * `loop`: TCP (non-blocking connect, select, 200 KB both ways) and UDP
//!   over 127.0.0.1, a refused connection.
//! * `ping`: ICMP echo to 127.0.0.1 and to the gateway.
//! * `dns`: resolve `-name` (default www.ibm.com) and localhost.
//! * `connect`: TCP to `-host`:`-port` (default the gateway, 5556): send
//!   a line, print the answer (a host-side `nc -l 5556`).
//! * `bulk`: 1 MB to the host (`-port`+1, a sink) and 1 MB from it
//!   (`-port`+2, a source of the pattern i % 251), checked.
//! * `listen`: accept one connection on port `-listen` (default 23)
//!   within `-wait` seconds (default 60), echo a line back (QEMU
//!   `hostfwd=tcp::5555-:23` and a host-side `nc localhost 5555`).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::net::*;
use os2::println;

os2::entry!(main);

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) -> bool {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
    ok
}

struct Opts {
    host: Option<[u8; 4]>,
    port: u16,
    listen: u16,
    name: String,
    wait: u32,
}

fn socket(typ: u32, proto: u32) -> Result<u32, u32> {
    let mut s = 0;
    match unsafe { Rs2SockCreate(AF_INET, typ, proto, &mut s) } {
        0 => Ok(s),
        e => Err(e),
    }
}

fn close(s: u32) {
    unsafe { Rs2SockClose(s) };
}

fn set_int(s: u32, opt: u32, v: u32) -> u32 {
    unsafe { Rs2SockSetOpt(s, SOL_SOCKET, opt, &v as *const u32 as *const u8, 4) }
}

fn set_timeout(s: u32, ms: u32) -> u32 {
    let tv = [ms / 1000, (ms % 1000) * 1000];
    unsafe { Rs2SockSetOpt(s, SOL_SOCKET, SO_RCVTIMEO, tv.as_ptr() as *const u8, 8) }
}

fn nonblocking(s: u32, on: bool) -> u32 {
    let mut v = on as u32;
    unsafe { Rs2SockIoctl(s, FIONBIO, &mut v) }
}

fn send(s: u32, d: &[u8]) -> Result<usize, u32> {
    let mut n = 0;
    match unsafe { Rs2SockSend(s, d.as_ptr(), d.len() as u32, 0, &mut n) } {
        0 => Ok(n as usize),
        e => Err(e),
    }
}

fn recv(s: u32, buf: &mut [u8]) -> Result<usize, u32> {
    let mut n = 0;
    match unsafe { Rs2SockRecv(s, buf.as_mut_ptr(), buf.len() as u32, 0, &mut n) } {
        0 => Ok(n as usize),
        e => Err(e),
    }
}

/// Wait until `s` is readable (`write` false) or writable.
fn wait_ready(s: u32, write: bool, ms: i32) -> bool {
    let mut socks = [s];
    let mut n = 0;
    let (r, w) = if write { (0, 1) } else { (1, 0) };
    unsafe { Rs2SockSelect(socks.as_mut_ptr(), r, w, 0, ms, &mut n) == 0 && n == 1 }
}

fn text(b: &[u8]) -> String {
    b.iter().map(|&c| if c == b'\n' || c == b'\r' { ' ' } else { c as char }).collect::<String>().trim_end().into()
}

fn ifaces() -> Vec<IfInfo> {
    let mut v = Vec::new();
    for i in 0..16 {
        let mut info = IfInfo::default();
        if unsafe { Rs2NetQueryIface(i, &mut info, core::mem::size_of::<IfInfo>() as u32) } != 0 {
            break;
        }
        v.push(info);
    }
    v
}

fn prefix(m: [u8; 4]) -> u32 {
    u32::from_be_bytes(m).leading_ones()
}

/// The first configured Ethernet interface.
fn ethernet() -> Option<IfInfo> {
    ifaces().into_iter().find(|i| i.flags & IF_LOOPBACK == 0 && i.addr != [0; 4])
}

fn config() -> Option<IfInfo> {
    let start = ms();
    let has_nic = ifaces().iter().any(|i| i.flags & IF_LOOPBACK == 0);
    while has_nic && ethernet().is_none() && ms().wrapping_sub(start) < 15_000 {
        unsafe { os2::DosSleep(200) };
    }
    for i in ifaces() {
        let m = i.mac;
        let dns: Vec<String> = i.dns[..i.ndns as usize].iter().map(|d| ip(*d)).collect();
        println!(
            "{} {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} {}/{} gateway {} dns {} mtu {}{}{}{}",
            i.name(),
            m[0], m[1], m[2], m[3], m[4], m[5],
            ip(i.addr),
            prefix(i.mask),
            ip(i.gateway),
            if dns.is_empty() { String::from("-") } else { dns.join(",") },
            i.mtu,
            if i.flags & IF_LINK != 0 { " link up" } else { " link down" },
            if i.flags & IF_DHCP != 0 { " dhcp" } else { "" },
            if i.flags & IF_BOUND != 0 { " bound" } else { "" },
        );
        println!("  rx {} packets {} bytes, tx {} packets {} bytes, dropped {}", i.rx_packets, i.rx_bytes, i.tx_packets, i.tx_bytes, i.rx_dropped);
    }
    let e = ethernet();
    check("config: an Ethernet interface has an address", e.is_some() || !has_nic);
    e
}

fn loopback() {
    const PORT: u16 = 7000;
    let lo = SockAddrIn::new([127, 0, 0, 1], PORT);
    let Ok(l) = socket(SOCK_STREAM, 0) else {
        check("loop: socket", false);
        return;
    };
    set_int(l, SO_REUSEADDR, 1);
    check("loop: bind 127.0.0.1:7000", unsafe { Rs2SockBind(l, &lo, 16) } == 0);
    check("loop: listen", unsafe { Rs2SockListen(l, 4) } == 0);
    let c = socket(SOCK_STREAM, 0).unwrap_or(0);
    nonblocking(c, true);
    let rc = unsafe { Rs2SockConnect(c, &lo, 16) };
    check("loop: non-blocking connect", rc == 0 || rc == EINPROGRESS);
    check("loop: connected socket writable", wait_ready(c, true, 2000));
    check("loop: listener readable", wait_ready(l, false, 2000));
    let mut peer = SockAddrIn::default();
    let mut s = 0;
    let rc = unsafe { Rs2SockAccept(l, &mut peer, &mut s) };
    let mut me = SockAddrIn::default();
    unsafe { Rs2SockGetName(c, 0, &mut me) };
    check("loop: accept gives the client's address", rc == 0 && peer.addr == [127, 0, 0, 1] && peer.port() == me.port());
    // 200 KB each way, interleaved, both ends non-blocking (one thread).
    nonblocking(s, true);
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (mut sent, mut got_s, mut back_c) = (0, Vec::new(), Vec::new());
    let mut echo: Vec<u8> = Vec::new();
    let mut buf = alloc::vec![0u8; 16384];
    let t0 = ms();
    while (got_s.len() < data.len() || back_c.len() < data.len()) && ms().wrapping_sub(t0) < 20_000 {
        if sent < data.len() {
            match send(c, &data[sent..]) {
                Ok(n) => sent += n,
                Err(EWOULDBLOCK) => {}
                Err(e) => {
                    println!("  send: {}", strerror(e));
                    break;
                }
            }
        }
        // Server: echo what arrived.
        if let Ok(n) = recv(s, &mut buf) {
            got_s.extend_from_slice(&buf[..n]);
            echo.extend_from_slice(&buf[..n]);
        }
        if !echo.is_empty() {
            if let Ok(k) = send(s, &echo) {
                echo.drain(..k);
            }
        }
        while let Ok(n) = recv(c, &mut buf) {
            if n == 0 {
                break;
            }
            back_c.extend_from_slice(&buf[..n]);
        }
    }
    check("loop: 200 KB client to server", got_s == data);
    check("loop: 200 KB echoed back", back_c == data);
    println!("  loopback TCP: {} ms", ms().wrapping_sub(t0));
    nonblocking(c, false);
    nonblocking(s, false);
    close(c);
    let n = recv(s, &mut buf);
    check("loop: EOF after the client closed", n == Ok(0));
    close(s);
    close(l);
    // UDP to ourselves.
    let u = socket(SOCK_DGRAM, 0).unwrap_or(0);
    let ua = SockAddrIn::new([127, 0, 0, 1], 7001);
    unsafe { Rs2SockBind(u, &ua, 16) };
    let mut n = 0;
    unsafe { Rs2SockSendTo(u, b"datagram".as_ptr(), 8, 0, &ua, &mut n) };
    let mut from = SockAddrIn::default();
    let mut got = 0;
    let rc = unsafe { Rs2SockRecvFrom(u, buf.as_mut_ptr(), 64, 0, &mut from, &mut got) };
    check("loop: UDP datagram to self", rc == 0 && &buf[..got as usize] == b"datagram" && from.port() == 7001);
    close(u);
    // Nobody listens on 7002.
    let r = socket(SOCK_STREAM, 0).unwrap_or(0);
    let rc = unsafe { Rs2SockConnect(r, &SockAddrIn::new([127, 0, 0, 1], 7002), 16) };
    check("loop: connection refused", rc == ECONNREFUSED);
    close(r);
}

fn ping(target: [u8; 4], what: &str) {
    let Ok(s) = socket(SOCK_RAW, IPPROTO_ICMP) else {
        check("ping: raw socket", false);
        return;
    };
    let mut ok = 0;
    for seq in 1..=3u16 {
        match ping_once(s, target, 0x4e54, seq, 56, 2000) {
            Ok(r) => {
                ok += 1;
                println!("  {} bytes from {}: icmp_seq={seq} ttl={} time={} ms", r.bytes + 8, ip(r.from), r.ttl, r.rtt_ms);
            }
            Err(e) => println!("  icmp_seq={seq}: {}", strerror(e)),
        }
    }
    close(s);
    check(&alloc::format!("ping: {what} {} answers", ip(target)), ok > 0);
}

fn dns(name: &str) {
    match resolve(name) {
        Ok(v) => {
            let a: Vec<String> = v.iter().map(|x| ip(*x)).collect();
            println!("  {name} = {}", a.join(" "));
            check(&alloc::format!("dns: {name} resolves"), !v.is_empty());
        }
        Err(h) => {
            println!("  {name}: {}", hstrerror(h));
            check(&alloc::format!("dns: {name} resolves"), false);
        }
    }
    check("dns: localhost is 127.0.0.1", resolve("localhost").is_ok_and(|v| v == [[127, 0, 0, 1]]));
}

fn connect(host: [u8; 4], port: u16) {
    let s = socket(SOCK_STREAM, 0).unwrap_or(0);
    let t0 = ms();
    let rc = unsafe { Rs2SockConnect(s, &SockAddrIn::new(host, port), 16) };
    if !check(&alloc::format!("connect: TCP to {}:{port}", ip(host)), rc == 0) {
        println!("  {}", strerror(rc));
        close(s);
        return;
    }
    let mut me = SockAddrIn::default();
    unsafe { Rs2SockGetName(s, 0, &mut me) };
    println!("  connected from {}:{} in {} ms", ip(me.addr), me.port(), ms().wrapping_sub(t0));
    check("connect: send", send(s, b"hello from rs2\n") == Ok(15));
    set_timeout(s, 10_000);
    let mut reply = Vec::new();
    let mut buf = [0u8; 512];
    while !reply.contains(&b'\n') && reply.len() < 4096 {
        match recv(s, &mut buf) {
            Ok(0) => break,
            Ok(n) => reply.extend_from_slice(&buf[..n]),
            Err(e) => {
                println!("  recv: {}", strerror(e));
                break;
            }
        }
    }
    println!("  host said: {}", text(&reply));
    check("connect: the host answered", !reply.is_empty());
    unsafe { Rs2SockShutdown(s, 1) };
    close(s);
}

/// The bulk test pattern.
fn pattern(i: usize) -> u8 {
    (i % 251) as u8
}

/// 1 MB to the host (port+1, a host-side `nc -l` writing a file) and
/// 1 MB from it (port+2, `nc -l` reading the same pattern).
fn bulk(host: [u8; 4], port: u16) {
    const SIZE: usize = 1 << 20;
    let s = socket(SOCK_STREAM, 0).unwrap_or(0);
    let rc = unsafe { Rs2SockConnect(s, &SockAddrIn::new(host, port + 1), 16) };
    if check(&alloc::format!("bulk: connect to {}:{}", ip(host), port + 1), rc == 0) {
        let data: Vec<u8> = (0..SIZE).map(pattern).collect();
        let t0 = ms();
        let r = send(s, &data);
        let dt = ms().wrapping_sub(t0).max(1);
        check("bulk: 1 MB sent", r == Ok(SIZE));
        println!("  sent 1 MB in {dt} ms ({} KB/s)", 1024 * 1000 / dt);
        unsafe { Rs2SockShutdown(s, 1) };
        // The host closes when it has everything.
        set_timeout(s, 20_000);
        let mut b = [0u8; 64];
        check("bulk: the host closed", recv(s, &mut b) == Ok(0));
    }
    close(s);
    let s = socket(SOCK_STREAM, 0).unwrap_or(0);
    let rc = unsafe { Rs2SockConnect(s, &SockAddrIn::new(host, port + 2), 16) };
    if check(&alloc::format!("bulk: connect to {}:{}", ip(host), port + 2), rc == 0) {
        set_timeout(s, 20_000);
        let mut buf = alloc::vec![0u8; 32768];
        let (mut n, mut good) = (0usize, true);
        let t0 = ms();
        loop {
            match recv(s, &mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    good &= buf[..k].iter().enumerate().all(|(i, &b)| b == pattern(n + i));
                    n += k;
                }
                Err(e) => {
                    println!("  recv: {}", strerror(e));
                    break;
                }
            }
        }
        let dt = ms().wrapping_sub(t0).max(1);
        println!("  received {n} bytes in {dt} ms ({} KB/s)", n as u32 / dt * 1000 / 1024);
        check("bulk: 1 MB received intact", n == SIZE && good);
    }
    close(s);
}

fn listen(port: u16, wait_s: u32) {
    let l = socket(SOCK_STREAM, 0).unwrap_or(0);
    set_int(l, SO_REUSEADDR, 1);
    let rc = unsafe { Rs2SockBind(l, &SockAddrIn::new([0; 4], port), 16) };
    check(&alloc::format!("listen: bind port {port}"), rc == 0);
    check("listen: listen", unsafe { Rs2SockListen(l, 2) } == 0);
    println!("  listening on port {port} for {wait_s} s");
    if !check("listen: a connection arrived", wait_ready(l, false, (wait_s * 1000) as i32)) {
        close(l);
        return;
    }
    let mut peer = SockAddrIn::default();
    let mut s = 0;
    check("listen: accept", unsafe { Rs2SockAccept(l, &mut peer, &mut s) } == 0);
    println!("  connection from {}:{}", ip(peer.addr), peer.port());
    send(s, b"rs2 nettest: send a line\r\n").ok();
    set_timeout(s, 15_000);
    let mut line = Vec::new();
    let mut buf = [0u8; 256];
    while !line.contains(&b'\n') && line.len() < 1024 {
        match recv(s, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => line.extend_from_slice(&buf[..n]),
        }
    }
    println!("  received: {}", text(&line));
    check("listen: received a line", !line.is_empty());
    let mut answer = Vec::from(&b"rs2 got: "[..]);
    answer.extend_from_slice(&line);
    check("listen: echoed it", send(s, &answer) == Ok(answer.len()));
    close(s);
    close(l);
}

fn main(st: &os2::Startup) -> u32 {
    let args: String = st.args().iter().map(|&b| b as char).collect();
    let words: Vec<&str> = args.split_whitespace().collect();
    let mut o = Opts { host: None, port: 5556, listen: 23, name: String::from("www.ibm.com"), wait: 60 };
    let mut sections: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let next = words.get(i + 1).copied().unwrap_or("");
        match words[i].to_ascii_lowercase().as_str() {
            "-host" => {
                o.host = parse_ip(next);
                i += 1;
            }
            "-port" => {
                o.port = next.parse().unwrap_or(o.port);
                i += 1;
            }
            "-listen" => {
                o.listen = next.parse().unwrap_or(o.listen);
                i += 1;
            }
            "-name" => {
                o.name = String::from(next);
                i += 1;
            }
            "-wait" => {
                o.wait = next.parse().unwrap_or(o.wait);
                i += 1;
            }
            w => sections.push(String::from(w)),
        }
        i += 1;
    }
    let run = |s: &str| sections.is_empty() || sections.iter().any(|x| x == s);
    let eth = config();
    if run("loop") {
        loopback();
    }
    let gw = eth.as_ref().map(|e| e.gateway).filter(|g| *g != [0; 4]);
    if run("ping") {
        ping([127, 0, 0, 1], "loopback");
        match gw {
            Some(g) => ping(g, "gateway"),
            None => println!("  (no gateway to ping)"),
        }
    }
    if run("dns") {
        dns(&o.name.clone());
    }
    if run("connect") {
        match o.host.or(gw) {
            Some(h) => connect(h, o.port),
            None => println!("  (no host to connect to)"),
        }
    }
    if run("bulk") {
        match o.host.or(gw) {
            Some(h) => bulk(h, o.port),
            None => println!("  (no host for the bulk transfer)"),
        }
    }
    if run("listen") {
        listen(o.listen, o.wait);
    }
    let n = FAILS.load(Ordering::Relaxed);
    println!("nettest done: {n} failure(s)");
    n
}
