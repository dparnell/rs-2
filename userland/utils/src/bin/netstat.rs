//! NETSTAT: network status, with the options of OS/2's TCP/IP 4.x
//! NETSTAT (one letter each, `-` or `/`, combinable):
//!
//! * `-a` the addresses of the interfaces (aliases included)
//! * `-c` ICMP counters, `-i` IP counters, `-u` UDP counters (and the
//!   UDP sockets), `-g` IGMP (none: rs2 has no multicast groups)
//! * `-m` memory: the stack's buffers
//! * `-n` the interfaces: MTU, packets, bytes, errors, link
//! * `-p` the ARP table
//! * `-r` the routing table
//! * `-s` every socket with its owner (process and socket number)
//! * `-t` TCP connections with their queues, windows and timers (the
//!   default); `-l` adds listening sockets
//! * `-x` TCP counters
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

fn owner(s: &SockInfo) -> String {
    if s.pid != 0 { format!("{:5} {:6}", s.pid, s.socket) } else { String::from("    -      -") }
}

fn tcp_connections(listening: bool) {
    outln("TCP connections");
    outln("Local Address           Foreign Address         State          PID  Socket");
    let mut list = sockets();
    list.sort_by_key(|s| (s.lport, s.rport));
    let mut n = 0;
    for s in list.iter().filter(|s| s.kind == 1) {
        let listen = s.flags & 1 != 0;
        if listen && !listening {
            continue;
        }
        let state = if listen { "LISTEN" } else { tcp_state_name(s.state) };
        outln(&format!("{:<23} {:<23} {state:<12} {}", ep(s.laddr, s.lport), ep(s.raddr, s.rport), owner(s)));
        n += 1;
    }
    if n == 0 {
        outln("(none)");
    }
}

fn all_sockets() {
    outln("Sockets");
    outln("Type   Local Address           Foreign Address         State          PID  Socket");
    let mut list = sockets();
    list.sort_by_key(|s| (s.kind, s.lport));
    for s in list {
        let (kind, state) = match s.kind {
            1 => ("STREAM", String::from(if s.flags & 1 != 0 { "LISTEN" } else { tcp_state_name(s.state) })),
            2 => ("DGRAM", String::new()),
            _ => ("RAW", format!("proto {}", s.protocol)),
        };
        let closed = if s.flags & 2 != 0 { " (closed)" } else { "" };
        outln(&format!("{kind:<6} {:<23} {:<23} {state:<12} {}{closed}", ep(s.laddr, s.lport), ep(s.raddr, s.rport), owner(&s)));
    }
}

fn addresses() {
    outln("Interface addresses");
    outln("Interface  Address          Netmask          Broadcast");
    for (k, i) in interfaces().iter().enumerate() {
        let name = iface_name(k as u32);
        let list = if_addrs(k as u32);
        if list.is_empty() {
            outln(&format!("{name:<10} (no address)"));
        }
        for (a, m) in list {
            let mut b = [0u8; 4];
            for x in 0..4 {
                b[x] = a[x] | !m[x];
            }
            let bc = if i.flags & IF_LOOPBACK != 0 { String::new() } else { ip(b) };
            outln(&format!("{name:<10} {:<16} {:<16} {bc}", ip(a), ip(m)));
        }
    }
}

fn ifaces() {
    outln("Interfaces");
    outln("Name   Mtu    Ipkts      Ibytes       Idrops  Opkts      Obytes       Link    Hardware address");
    for (k, i) in interfaces().iter().enumerate() {
        let link = if i.flags & IF_LOOPBACK != 0 {
            "loop"
        } else if i.flags & IF_UP == 0 {
            "down*"
        } else if i.flags & IF_LINK != 0 {
            "up"
        } else {
            "down"
        };
        let m = i.mac;
        let hw = if i.flags & IF_LOOPBACK != 0 {
            String::new()
        } else {
            format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
        };
        outln(&format!(
            "{:<6} {:<6} {:<10} {:<12} {:<7} {:<10} {:<12} {link:<7} {hw}",
            iface_name(k as u32),
            i.mtu,
            i.rx_packets,
            i.rx_bytes,
            i.rx_dropped,
            i.tx_packets,
            i.tx_bytes
        ));
    }
    outln("(down*: turned off with IFCONFIG)");
}

fn arp() {
    outln("ARP table");
    outln("Address          Ethernet address   Intf   Flags");
    let list = arp_table();
    if list.is_empty() {
        outln("(empty)");
    }
    for e in list {
        let hw = if e.complete {
            let m = e.mac;
            format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
        } else {
            String::from("(incomplete)")
        };
        let mut f = String::new();
        if e.permanent {
            f.push_str("permanent ");
        } else if e.complete {
            f.push_str(&format!("{} s ", e.ttl));
        }
        if e.publish {
            f.push_str("published");
        }
        outln(&format!("{:<16} {hw:<18} {:<6} {f}", ip(e.ip), iface_name(e.index)));
    }
}

fn route_table() {
    outln("Routing table");
    outln("Destination      Router           Netmask          Metric Flags  Use      Intrfc");
    for r in routes() {
        let dest = if r.dest == [0; 4] && r.mask == [0; 4] { String::from("default") } else { ip(r.dest) };
        let mut f = String::from("U");
        if r.flags & 2 != 0 {
            f.push('G');
        }
        if r.flags & 4 != 0 {
            f.push('H');
        }
        if r.flags & 0x800 != 0 {
            f.push('S');
        }
        outln(&format!("{dest:<16} {:<16} {:<16} {:<6} {f:<6} {:<8} {}", ip(r.gateway), ip(r.mask), r.metric, r.uses, r.iface));
    }
}

/// Counters by name (the order of Rs2NetControl's NC_STATS).
struct Stats(Vec<u32>);

impl Stats {
    fn get(&self, i: usize) -> u32 {
        self.0.get(i).copied().unwrap_or(0)
    }
}

fn line(n: u32, what: &str) {
    outln(&format!("    {n:>10} {what}"));
}

fn ip_stats(s: &Stats) {
    outln("IP:");
    line(s.get(0), "datagrams received");
    line(s.get(1), "with header errors");
    line(s.get(2), "delivered to protocols");
    line(s.get(3), "datagrams sent");
    line(s.get(4), "forwarded");
    line(s.get(5), "without a route");
    line(s.get(6), "for unknown protocols");
    line(s.get(7), "fragments received");
    line(s.get(8), "datagrams reassembled");
    line(s.get(9), "datagrams fragmented");
    let fwd = net_var(NV_FORWARDING, None).unwrap_or(0);
    outln(&format!("    IP forwarding is {}", if fwd != 0 { "on" } else { "off" }));
}

fn icmp_stats(s: &Stats) {
    outln("ICMP:");
    line(s.get(10), "messages received");
    line(s.get(11), "messages sent");
    line(s.get(12), "echo requests received");
    line(s.get(13), "destination unreachables received");
}

fn udp_stats(s: &Stats) {
    outln("UDP:");
    line(s.get(14), "datagrams received");
    line(s.get(15), "datagrams sent");
    line(s.get(16), "for ports nobody listens on");
    line(s.get(17), "with bad checksums or lengths");
    outln("UDP sockets");
    outln("Local Address           Foreign Address           PID  Socket");
    for so in sockets().iter().filter(|x| x.kind == 2) {
        outln(&format!("{:<23} {:<23} {}", ep(so.laddr, so.lport), ep(so.raddr, so.rport), owner(so)));
    }
}

fn tcp_stats(s: &Stats) {
    outln("TCP:");
    line(s.get(18), "connections initiated");
    line(s.get(19), "connections accepted");
    line(s.get(20), "connection attempts failed");
    line(s.get(21), "resets sent for unknown connections");
    line(s.get(22), "segments with bad checksums");
    line(s.get(25), "segments sent");
    line(s.get(26), "segments received");
    line(s.get(27), "data bytes sent");
    line(s.get(28), "data bytes received");
    line(s.get(29), "fast retransmissions");
    line(s.get(30), "bytes retransmitted");
    line(s.get(31), "retransmission timeouts");
    line(s.get(32), "segments received out of order");
    line(s.get(33), "duplicate segments received");
    line(s.get(34), "delayed ACKs sent");
    line(s.get(35), "keepalive probes sent");
    line(s.get(36), "connections dropped by keepalive");
    line(s.get(37), "SACK blocks received");
    line(s.get(38), "urgent data notifications received");
    let k = |id| net_var(id, None).unwrap_or(0);
    outln(&format!(
        "    keepalive after {} s idle, every {} s, {} probes; buffers {} / {} bytes",
        k(NV_KEEPIDLE),
        k(NV_KEEPINTVL),
        k(NV_KEEPCNT),
        k(NV_SENDSPACE),
        k(NV_RECVSPACE)
    ));
    outln("ARP:");
    line(s.get(23), "requests sent");
    line(s.get(24), "replies sent");
}

fn usage() -> u32 {
    outln("Usage: NETSTAT [-a] [-c] [-g] [-i] [-l] [-m] [-n] [-p] [-r] [-s] [-t] [-u] [-x]");
    outln("  -a  interface addresses     -c  ICMP counters      -g  IGMP counters");
    outln("  -i  IP counters             -l  TCP with listeners -m  memory");
    outln("  -n  interfaces              -p  ARP table          -r  routing table");
    outln("  -s  all sockets             -t  TCP connections    -u  UDP counters and sockets");
    outln("  -x  TCP counters");
    0
}

fn main(s: &os2::Startup) -> u32 {
    let a: Vec<String> = args(s).into_iter().map(|x| x.to_ascii_lowercase()).collect();
    let mut opts = String::new();
    for x in &a {
        if x.starts_with('-') || x.starts_with('/') {
            opts.push_str(&x[1..]);
        } else {
            eprint(&format!("NETSTAT: {x}: not an option\r\n"));
            usage();
            return 1;
        }
    }
    if opts.contains('?') || opts.contains('h') {
        return usage();
    }
    if interfaces().is_empty() {
        eprint("NETSTAT: TCP/IP is not running\r\n");
        return 1;
    }
    if opts.is_empty() {
        opts.push('t');
    }
    let stats = Stats(os2::net::stats());
    let mut first = true;
    for c in opts.chars() {
        if !first {
            outln("");
        }
        first = false;
        match c {
            'a' => addresses(),
            'c' => icmp_stats(&stats),
            'g' => {
                outln("IGMP:");
                outln("    no multicast groups (rs2 does not join any)");
            }
            'i' => ip_stats(&stats),
            'l' if !opts.contains('t') => tcp_connections(true),
            'l' => {}
            'm' => {
                let list = sockets();
                outln("Memory:");
                outln(&format!("    {} sockets; buffers are allocated as data arrives (no fixed pool)", list.len()));
            }
            'n' => ifaces(),
            'p' => arp(),
            'r' => route_table(),
            's' => all_sockets(),
            't' => tcp_connections(opts.contains('l')),
            'u' => udp_stats(&stats),
            'x' => tcp_stats(&stats),
            _ => {
                eprint(&format!("NETSTAT: -{c}: unknown option\r\n"));
                usage();
                return 1;
            }
        }
    }
    0
}
