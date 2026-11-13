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
//! * `race`: select() on a socket that another thread closes while a new
//!   socket takes its place (and its stack id), made readable at once.
//! * `tcp`: urgent data over loopback (MSG_OOB, select's exception set,
//!   SIOCATMARK, SO_OOBINLINE), TCP_NODELAY and SO_KEEPALIVE.
//! * `tables`: routes, ARP entries and an interface alias set and removed
//!   at run time (Rs2NetControl), counters, a stack variable.
//! * `urgent`: urgent data from and to the host (`-port`+3, +4) and
//!   keepalives on an idle connection (`-port`+5); the host side of
//!   these and of `connect`/`bulk`/`listen` is tools/scripts/nettesthost.py.

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
    // The socket list shows it with its owner.
    let me_pid = {
        let (mut tib, mut pib) = (core::ptr::null_mut(), core::ptr::null_mut());
        unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
        unsafe { (*pib).pid }
    };
    let listed = os2::net::sockets().iter().any(|i| i.kind == 2 && i.lport == 7001 && i.pid == me_pid && i.socket == u);
    check("loop: socket list shows the UDP socket and its owner", listed);
    // so_cancel: a receive blocked in another thread ends with EINTR.
    unsafe {
        CANCEL_SOCK = u;
        CANCEL_RC = 0;
        let mut tid = 0;
        os2::DosCreateThread(&mut tid, cancel_waiter, 0, 0, 65536);
        os2::DosSleep(300);
        Rs2SockCancel(u);
        let mut t = tid;
        os2::DosWaitThread(&mut t, 0);
    }
    check("loop: so_cancel ends a blocked recvfrom with EINTR", unsafe { CANCEL_RC } == EINTR);
    close(u);
    // Nobody listens on 7002.
    let r = socket(SOCK_STREAM, 0).unwrap_or(0);
    let rc = unsafe { Rs2SockConnect(r, &SockAddrIn::new([127, 0, 0, 1], 7002), 16) };
    check("loop: connection refused", rc == ECONNREFUSED);
    close(r);
}

static RACE_SOCK: AtomicU32 = AtomicU32::new(0);
static RACE_NEW: AtomicU32 = AtomicU32::new(0);

/// Close the socket a select() in the main thread waits for, then at once
/// open another (it gets the stack's id the closed one had, and often its
/// number) and make it readable.
unsafe extern "C" fn race_closer(round: u32) {
    unsafe { os2::DosSleep(20 + round % 7) };
    close(RACE_SOCK.load(Ordering::Relaxed));
    let Ok(u) = socket(SOCK_DGRAM, 0) else { return };
    let a = SockAddrIn::new([127, 0, 0, 1], 7020);
    unsafe { Rs2SockBind(u, &a, 16) };
    let mut n = 0;
    unsafe { Rs2SockSendTo(u, b"x".as_ptr(), 1, 0, &a, &mut n) };
    RACE_NEW.store(u, Ordering::Relaxed);
}

/// select() resolves its sockets when called; one closed by another
/// thread meanwhile must not be reported through the socket that took
/// its place (the call ends with ENOTSOCK or EBADF, or the socket is not
/// ready), as every other socket call.
fn select_close_race() {
    let mut wrong = 0;
    let mut rounds = 0;
    for round in 0..20 {
        let Ok(u) = socket(SOCK_DGRAM, 0) else { break };
        let a = SockAddrIn::new([127, 0, 0, 1], 7019);
        set_int(u, SO_REUSEADDR, 1);
        unsafe { Rs2SockBind(u, &a, 16) };
        RACE_SOCK.store(u, Ordering::Relaxed);
        RACE_NEW.store(0, Ordering::Relaxed);
        let mut tid = 0;
        unsafe { os2::DosCreateThread(&mut tid, race_closer, round, 0, 65536) };
        let mut socks = [u];
        let mut n = 0;
        let rc = unsafe { Rs2SockSelect(socks.as_mut_ptr(), 1, 0, 0, 1000, &mut n) };
        let mut t = tid;
        unsafe { os2::DosWaitThread(&mut t, 0) };
        if rc == 0 && n == 1 {
            println!("  round {round}: select reported socket {u} ready after another thread closed it (now socket {})", RACE_NEW.load(Ordering::Relaxed));
            wrong += 1;
        }
        rounds += 1;
        close(RACE_NEW.load(Ordering::Relaxed));
    }
    check("race: select never sees a socket closed meanwhile through its successor", rounds == 20 && wrong == 0);
}

static mut CANCEL_SOCK: u32 = 0;
static mut CANCEL_RC: u32 = 0;

unsafe extern "C" fn cancel_waiter(_: u32) {
    let mut buf = [0u8; 16];
    let mut from = SockAddrIn::default();
    let mut got = 0;
    unsafe { CANCEL_RC = Rs2SockRecvFrom(CANCEL_SOCK, buf.as_mut_ptr(), 16, 0, &mut from, &mut got) };
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
    if run("race") {
        select_close_race();
    }
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
    if run("tcp") {
        tcp_features();
    }
    if run("tables") {
        config_tables(eth.as_ref());
    }
    if run("urgent") {
        match o.host.or(gw) {
            Some(h) => host_features(h, o.port),
            None => println!("  (no host for the urgent data and keepalive checks)"),
        }
    }
    if run("listen") {
        listen(o.listen, o.wait);
    }
    let n = FAILS.load(Ordering::Relaxed);
    println!("nettest done: {n} failure(s)");
    n
}

const MSG_OOB: u32 = 1;
const SIOCATMARK: u32 = 0x4004_7307;
const SO_KEEPALIVE: u32 = 0x0008;
const SO_OOBINLINE: u32 = 0x0100;
const TCP_NODELAY: u32 = 1;

fn send_flags(s: u32, d: &[u8], flags: u32) -> Result<usize, u32> {
    let mut n = 0;
    match unsafe { Rs2SockSend(s, d.as_ptr(), d.len() as u32, flags, &mut n) } {
        0 => Ok(n as usize),
        e => Err(e),
    }
}

fn recv_flags(s: u32, buf: &mut [u8], flags: u32) -> Result<usize, u32> {
    let mut n = 0;
    match unsafe { Rs2SockRecv(s, buf.as_mut_ptr(), buf.len() as u32, flags, &mut n) } {
        0 => Ok(n as usize),
        e => Err(e),
    }
}

fn at_mark(s: u32) -> bool {
    let mut v = 0;
    (unsafe { Rs2SockIoctl(s, SIOCATMARK, &mut v) }) == 0 && v != 0
}

fn wait_except(s: u32, ms: i32) -> bool {
    let mut socks = [s];
    let mut n = 0;
    unsafe { Rs2SockSelect(socks.as_mut_ptr(), 0, 0, 1, ms, &mut n) == 0 && n == 1 }
}

/// A connected loopback pair (client, server) on `port`.
fn lo_pair(port: u16) -> Option<(u32, u32, u32)> {
    let a = SockAddrIn::new([127, 0, 0, 1], port);
    let l = socket(SOCK_STREAM, 0).ok()?;
    set_int(l, SO_REUSEADDR, 1);
    if unsafe { Rs2SockBind(l, &a, 16) } != 0 || unsafe { Rs2SockListen(l, 2) } != 0 {
        close(l);
        return None;
    }
    let c = socket(SOCK_STREAM, 0).ok()?;
    if unsafe { Rs2SockConnect(c, &a, 16) } != 0 {
        return None;
    }
    let (mut peer, mut s) = (SockAddrIn::default(), 0);
    if unsafe { Rs2SockAccept(l, &mut peer, &mut s) } != 0 {
        return None;
    }
    Some((c, s, l))
}

/// Urgent data, TCP options and per-socket waits over loopback.
fn tcp_features() {
    let Some((c, s, l)) = lo_pair(7010) else {
        check("tcp: loopback pair", false);
        return;
    };
    // TCP_NODELAY and SO_KEEPALIVE are kept and read back.
    let opt = |s: u32, level: u32, o: u32| {
        let (mut v, mut n) = (0u32, 4u32);
        unsafe { Rs2SockGetOpt(s, level, o, &mut v as *mut u32 as *mut u8, &mut n) };
        v
    };
    let one = 1u32;
    unsafe { Rs2SockSetOpt(c, IPPROTO_TCP, TCP_NODELAY, &one as *const u32 as *const u8, 4) };
    set_int(c, SO_KEEPALIVE, 1);
    check("tcp: TCP_NODELAY and SO_KEEPALIVE read back", opt(c, IPPROTO_TCP, TCP_NODELAY) == 1 && opt(c, SOL_SOCKET, SO_KEEPALIVE) == 1);
    // Urgent data out of line: the reader stops at the mark.
    send(c, b"ab").ok();
    let oob = send_flags(c, b"!", MSG_OOB);
    send(c, b"cd").ok();
    check("tcp: send MSG_OOB", oob == Ok(1));
    check("tcp: select reports the exception", wait_except(s, 2000));
    set_timeout(s, 3000);
    let mut buf = [0u8; 16];
    let n = recv(s, &mut buf).unwrap_or(0);
    check("tcp: read stops at the urgent mark", &buf[..n] == b"ab" && at_mark(s));
    let n = recv_flags(s, &mut buf, MSG_OOB).unwrap_or(0);
    check("tcp: recv MSG_OOB gets the urgent byte", n == 1 && buf[0] == b'!');
    let n = recv(s, &mut buf).unwrap_or(0);
    check("tcp: the rest after the mark", &buf[..n] == b"cd" && !at_mark(s));
    // Inline: the urgent byte stays in the stream.
    set_int(s, SO_OOBINLINE, 1);
    send_flags(c, b"#", MSG_OOB).ok();
    send(c, b"z").ok();
    unsafe { os2::DosSleep(100) };
    let mut got = Vec::new();
    while got.len() < 2 {
        match recv(s, &mut buf) {
            Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    check("tcp: SO_OOBINLINE keeps the urgent byte in the stream", got == b"#z");
    close(c);
    close(s);
    close(l);
}

/// Routes, ARP entries and interface aliases through Rs2NetControl.
fn config_tables(eth: Option<&IfInfo>) {
    let Some(e) = eth else {
        println!("  (no Ethernet interface for the table checks)");
        return;
    };
    let gw = e.gateway;
    let net = [192, 0, 2, 0];
    let host = [198, 51, 100, 7];
    check("route: add a network route", route_add(net, [255, 255, 255, 0], gw, 2).is_ok());
    check("route: add a host route", route_add(host, [255; 4], gw, 1).is_ok());
    check("route: a second add is refused", route_add(net, [255, 255, 255, 0], gw, 2) == Err(17));
    let rt = routes();
    check(
        "route: both are in the table",
        rt.iter().any(|r| r.dest == net && r.flags & 2 != 0) && rt.iter().any(|r| r.dest == host && r.flags & 4 != 0),
    );
    check("route: an off-link gateway is refused", route_add([203, 0, 113, 0], [255, 255, 255, 0], [203, 0, 113, 1], 1).is_err());
    check("route: delete", route_delete(net, [255, 255, 255, 0], [0; 4]).is_ok() && route_delete(host, [255; 4], gw).is_ok());
    check("route: gone", !routes().iter().any(|r| r.dest == net || r.dest == host));
    // ARP: a static entry, then deleted.
    let mut neigh = e.addr;
    neigh[3] = neigh[3].wrapping_add(100);
    check("arp: set a permanent entry", arp_set(neigh, [2, 0, 0, 0x12, 0x34, 0x56], true, false).is_ok());
    check("arp: listed", arp_table().iter().any(|a| a.ip == neigh && a.permanent && a.mac == [2, 0, 0, 0x12, 0x34, 0x56]));
    check("arp: the gateway was resolved", arp_table().iter().any(|a| a.ip == gw && a.complete));
    check("arp: delete", arp_delete(neigh).is_ok() && !arp_table().iter().any(|a| a.ip == neigh));
    // An alias: the stack answers on it (as a local address).
    let Some(idx) = iface_index("lan0") else { return };
    let mut alias = e.addr;
    alias[3] = alias[3].wrapping_add(50);
    check("ifconfig: add an alias", if_set(idx, IFOP_ALIAS_ADD, alias, e.mask).is_ok());
    check("ifconfig: listed", if_addrs(idx).iter().any(|x| x.0 == alias));
    if let Ok(s) = socket(SOCK_RAW, IPPROTO_ICMP) {
        check("ifconfig: the alias answers pings", ping_once(s, alias, 0x4e55, 1, 16, 2000).is_ok());
        close(s);
    }
    check("ifconfig: delete the alias", if_set(idx, IFOP_ALIAS_DELETE, alias, [0; 4]).is_ok() && !if_addrs(idx).iter().any(|x| x.0 == alias));
    // Counters move.
    let st = stats();
    check("stats: counters", st.len() >= 39 && st[0] > 0 && st[25] > 0);
    // A variable read and written back.
    let old = net_var(NV_KEEPCNT, None).unwrap_or(0);
    check("sysctl: keepalive probes", old == 8 && net_var(NV_KEEPCNT, Some(5)) == Ok(8) && net_var(NV_KEEPCNT, Some(old)) == Ok(5));
}

/// Urgent data and keepalives with the host (nettesthost.py's PORT+3 to
/// PORT+5).
fn host_features(host: [u8; 4], port: u16) {
    // Urgent data from the host.
    let urg0 = stats().get(38).copied().unwrap_or(0);
    let s = socket(SOCK_STREAM, 0).unwrap_or(0);
    if check("urgent: connect to the host's sender", unsafe { Rs2SockConnect(s, &SockAddrIn::new(host, port + 3), 16) } == 0) {
        set_timeout(s, 5000);
        let mut buf = [0u8; 16];
        let (mut inband, mut oob) = (Vec::new(), Vec::new());
        let t0 = ms();
        while ms().wrapping_sub(t0) < 5000 {
            if wait_except(s, 0) && oob.is_empty() {
                if let Ok(1) = recv_flags(s, &mut buf, MSG_OOB) {
                    oob.push(buf[0]);
                }
            }
            if !wait_ready(s, false, 200) {
                continue;
            }
            match recv(s, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => inband.extend_from_slice(&buf[..n]),
            }
        }
        println!("  in band {:?}, urgent {:?}", text(&inband), text(&oob));
        if oob.is_empty() && inband == b"ab!cd" && stats().get(38).copied().unwrap_or(0) == urg0 {
            // No segment came with URG: QEMU's user network passes the
            // host's out-of-band byte on as ordinary data.
            println!("  (the host's network sent the byte without URG: not checked)");
        } else {
            check("urgent: the host's urgent byte arrived out of band", oob == b"!" && inband == b"abcd");
        }
    }
    close(s);
    // Urgent data to the host.
    let s = socket(SOCK_STREAM, 0).unwrap_or(0);
    if check("urgent: connect to the host's receiver", unsafe { Rs2SockConnect(s, &SockAddrIn::new(host, port + 4), 16) } == 0) {
        send(s, b"xy").ok();
        unsafe { os2::DosSleep(300) };
        send_flags(s, b"#", MSG_OOB).ok();
        unsafe { os2::DosSleep(300) };
        send(s, b"z").ok();
        unsafe { os2::DosSleep(300) };
        unsafe { Rs2SockShutdown(s, 1) };
        set_timeout(s, 15_000);
        let mut reply = Vec::new();
        let mut buf = [0u8; 128];
        while let Ok(n) = recv(s, &mut buf) {
            if n == 0 {
                break;
            }
            reply.extend_from_slice(&buf[..n]);
        }
        let r = text(&reply);
        println!("  host saw: {r}");
        check("urgent: the host got the byte out of band", r.contains("oob=#") && r.contains("inband=xyz"));
    }
    close(s);
    // Keepalives: 2 s idle, 1 s apart; QEMU's stack answers them.
    let (idle, intvl) = (net_var(NV_KEEPIDLE, Some(2)).unwrap_or(7200), net_var(NV_KEEPINTVL, Some(1)).unwrap_or(75));
    let before = stats().get(35).copied().unwrap_or(0);
    let s = socket(SOCK_STREAM, 0).unwrap_or(0);
    set_int(s, SO_KEEPALIVE, 1);
    if check("keepalive: connect to the silent host", unsafe { Rs2SockConnect(s, &SockAddrIn::new(host, port + 5), 16) } == 0) {
        unsafe { os2::DosSleep(7000) };
        let probes = stats().get(35).copied().unwrap_or(0) - before;
        println!("  {probes} keepalive probe(s) in 7 s");
        check("keepalive: probes went out", probes >= 2);
        check("keepalive: the connection lives", send(s, b"ping\n") == Ok(5));
        set_timeout(s, 10_000);
        let mut buf = [0u8; 64];
        let n = recv(s, &mut buf).unwrap_or(0);
        check("keepalive: the host answers after the idle time", text(&buf[..n]).contains("still here"));
    }
    close(s);
    let _ = net_var(NV_KEEPIDLE, Some(idle));
    let _ = net_var(NV_KEEPINTVL, Some(intvl));
}
