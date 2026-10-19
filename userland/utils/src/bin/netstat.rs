//! NETSTAT [-a] [-t] [-u] [-r] [-i] [-s]: network status. Without
//! options the TCP connections (-a adds listening and UDP/raw sockets);
//! -r the routing table, -i the interfaces, -s counters.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use os2::net::*;
use utils::*;
os2::entry!(main);

fn ep(a: [u8; 4], port: u16) -> String {
    if port == 0 && a == [0; 4] {
        return String::from("*.*");
    }
    let host = if a == [0; 4] { String::from("*") } else { ip(a) };
    if port == 0 { format!("{host}.*") } else { format!("{host}.{port}") }
}

fn sockets(all: bool, tcp: bool, udp: bool) {
    outln("Active Internet connections");
    outln("Proto Local Address           Foreign Address         State          PID  Socket");
    let mut list = os2::net::sockets();
    list.sort_by_key(|s| (s.kind, s.lport));
    for s in list {
        let proto = match s.kind {
            1 => "tcp",
            2 => "udp",
            _ => "raw",
        };
        if (s.kind == 1 && !tcp) || (s.kind != 1 && !udp) {
            continue;
        }
        let listening = s.flags & 1 != 0;
        if !all && (s.kind != 1 || listening) {
            continue;
        }
        let state = if s.kind == 1 {
            String::from(if listening { "LISTEN" } else { tcp_state_name(s.state) })
        } else if s.kind == 3 {
            format!("proto {}", s.protocol)
        } else {
            String::new()
        };
        let owner = if s.pid != 0 { format!("{:4} {:7}", s.pid, s.socket) } else { String::from("   -       -") };
        outln(&format!("{proto:<5} {:<23} {:<23} {state:<12} {owner}", ep(s.laddr, s.lport), ep(s.raddr, s.rport)));
    }
}

fn routes() {
    outln("Routing table");
    outln("Destination      Gateway          Netmask          Flags  Interface");
    for r in os2::net::routes() {
        let dest = if r.dest == [0; 4] && r.flags & 2 != 0 { String::from("default") } else { ip(r.dest) };
        let flags = if r.flags & 2 != 0 { "UG" } else { "U" };
        outln(&format!("{dest:<16} {:<16} {:<16} {flags:<6} {}", ip(r.gateway), ip(r.mask), r.iface));
    }
}

fn interfaces() {
    outln("Name   Mtu    Address          Ipkts      Ierrs  Opkts      Link");
    for (name, i) in os2_interfaces() {
        let link = if i.flags & IF_LOOPBACK != 0 {
            "loopback"
        } else if i.flags & IF_LINK != 0 {
            "up"
        } else {
            "down"
        };
        outln(&format!("{name:<6} {:<6} {:<16} {:<10} {:<6} {:<10} {link}", i.mtu, ip(i.addr), i.rx_packets, i.rx_dropped, i.tx_packets));
    }
}

fn stats() {
    let list = os2::net::sockets();
    let count = |k: u8| list.iter().filter(|s| s.kind == k).count();
    let est = list.iter().filter(|s| s.kind == 1 && s.state == 5).count();
    let listen = list.iter().filter(|s| s.kind == 1 && s.flags & 1 != 0).count();
    outln("sockets:");
    outln(&format!("    {} TCP ({est} established, {listen} listening), {} UDP, {} raw", count(1), count(2), count(3)));
    for (name, i) in os2_interfaces() {
        outln(&format!("{name}:"));
        outln(&format!("    {} packets received ({} bytes), {} dropped", i.rx_packets, i.rx_bytes, i.rx_dropped));
        outln(&format!("    {} packets sent ({} bytes)", i.tx_packets, i.tx_bytes));
    }
}

fn main(s: &os2::Startup) -> u32 {
    let a: Vec<String> = args(s).into_iter().map(|x| x.to_ascii_lowercase()).collect();
    let has = |c: char| a.iter().any(|x| (x.starts_with('-') || x.starts_with('/')) && x[1..].contains(c));
    if has('?') || has('h') {
        outln("Usage: NETSTAT [-a] [-t] [-u] [-r] [-i] [-s]");
        outln("  (none) TCP connections   -a  all sockets   -t/-u  TCP/UDP only");
        outln("  -r  routing table   -i  interfaces   -s  counters");
        return 0;
    }
    if os2_interfaces().is_empty() {
        eprint("NETSTAT: TCP/IP is not running\r\n");
        return 1;
    }
    let mut did = false;
    if has('r') {
        routes();
        did = true;
    }
    if has('i') {
        interfaces();
        did = true;
    }
    if has('s') {
        stats();
        did = true;
    }
    if !did || has('a') || has('t') || has('u') {
        let (t, u) = (has('t'), has('u'));
        let (tcp, udp) = if !t && !u { (true, true) } else { (t, u) };
        sockets(has('a') || u, tcp, udp);
    }
    0
}
