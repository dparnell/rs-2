//! Stack tests: two stacks on a simulated Ethernet segment (optionally
//! losing frames), loopback, DHCP and DNS against scripted servers.

use super::*;
use alloc::vec;

const MAC_A: Mac = [0x52, 0x54, 0, 0x12, 0x34, 0x56];
const MAC_B: Mac = [0x52, 0x55, 0x0a, 0, 2, 2];

fn ip_a() -> Ipv4 {
    Ipv4::new(10, 0, 2, 15)
}
fn ip_b() -> Ipv4 {
    Ipv4::new(10, 0, 2, 2)
}

struct Net {
    a: Stack,
    b: Stack,
    now: u64,
    frames: u64,
    /// Lose every n-th frame (0: none).
    lose: u64,
}

impl Net {
    fn new() -> Net {
        let mut a = Stack::new(1, "rs2");
        let mut b = Stack::new(2, "host");
        let ia = a.add_ethernet("eth0", MAC_A, 1500);
        let ib = b.add_ethernet("eth0", MAC_B, 1500);
        a.set_link(ia, true);
        b.set_link(ib, true);
        a.configure(ia, ip_a(), Ipv4::mask(24), Some(ip_b()), &[ip_b()]);
        b.configure(ib, ip_b(), Ipv4::mask(24), None, &[]);
        Net { a, b, now: 0, frames: 0, lose: 0 }
    }

    /// Move frames between the stacks until both are quiet.
    fn run(&mut self) {
        for _ in 0..100_000 {
            let mut moved = false;
            while let Some((_, f)) = self.a.take_frame() {
                moved = true;
                self.frames += 1;
                if self.lose == 0 || self.frames % self.lose != 0 {
                    self.b.receive(1, &f, self.now);
                }
            }
            while let Some((_, f)) = self.b.take_frame() {
                moved = true;
                self.frames += 1;
                if self.lose == 0 || self.frames % self.lose != 0 {
                    self.a.receive(1, &f, self.now);
                }
            }
            if !moved {
                return;
            }
        }
        panic!("frames keep flowing");
    }

    /// Advance time by `ms`, polling both stacks.
    fn tick(&mut self, ms: u64) {
        self.now += ms;
        self.a.poll(self.now);
        self.b.poll(self.now);
        self.run();
    }
}

fn ep(a: Ipv4, p: u16) -> Endpoint {
    Endpoint::new(a, p)
}

#[test]
fn arp_and_ping() {
    let mut n = Net::new();
    let s = n.a.socket(Kind::Raw(ipv4::PROTO_ICMP)).unwrap();
    let req = icmp::echo(icmp::ECHO_REQUEST, 0x77, 1, b"ping data");
    n.a.send_to(s, &req, ep(ip_b(), 0), 0).unwrap();
    n.run();
    let mut buf = [0u8; 200];
    let (len, from) = n.a.recv_from(s, &mut buf, false, 0).unwrap();
    assert_eq!(from.addr, ip_b());
    let h = ipv4::parse(&buf[..len]).unwrap();
    let m = icmp::parse(&buf[h.ihl..len]).unwrap();
    assert_eq!((m.kind, m.ident(), m.seq(), m.data), (icmp::ECHO_REPLY, 0x77, 1, &b"ping data"[..]));
    // The reply taught both sides each other's address.
    assert_eq!(n.a.arp.lookup(1, ip_b(), 0), Some(MAC_B));
    assert_eq!(n.b.arp.lookup(1, ip_a(), 0), Some(MAC_A));
}

#[test]
fn udp_exchange_and_port_unreachable() {
    let mut n = Net::new();
    let srv = n.b.socket(Kind::Udp).unwrap();
    n.b.bind(srv, ep(Ipv4::UNSPECIFIED, 7)).unwrap();
    let cli = n.a.socket(Kind::Udp).unwrap();
    n.a.send_to(cli, b"hello", ep(ip_b(), 7), 0).unwrap();
    n.run();
    let mut buf = [0u8; 64];
    let (len, from) = n.b.recv_from(srv, &mut buf, false, 0).unwrap();
    assert_eq!(&buf[..len], b"hello");
    assert_eq!(from.addr, ip_a());
    n.b.send_to(srv, b"olleh", from, 0).unwrap();
    n.run();
    assert_eq!(n.a.recv(cli, &mut buf, false, 0).unwrap(), 5);
    assert_eq!(n.a.recv(cli, &mut buf, false, 0), Err(Error::WouldBlock));
    // A connected socket learns about a closed port from ICMP.
    let c2 = n.a.socket(Kind::Udp).unwrap();
    n.a.connect(c2, ep(ip_b(), 9), 0).unwrap();
    n.a.send(c2, b"x", 0).unwrap();
    n.run();
    assert_eq!(n.a.recv(c2, &mut buf, false, 0), Err(Error::ConnRefused));
}

#[test]
fn fragmented_udp() {
    let mut n = Net::new();
    let srv = n.b.socket(Kind::Udp).unwrap();
    n.b.bind(srv, ep(Ipv4::UNSPECIFIED, 5000)).unwrap();
    let cli = n.a.socket(Kind::Udp).unwrap();
    let big: Vec<u8> = (0..9000u32).map(|i| i as u8).collect();
    n.a.send_to(cli, &big, ep(ip_b(), 5000), 0).unwrap();
    n.run();
    let mut buf = vec![0u8; 10000];
    let (len, _) = n.b.recv_from(srv, &mut buf, false, 0).unwrap();
    assert_eq!(&buf[..len], &big[..]);
}

/// Connect a to b:port (b listening); returns (client, listener, server).
fn tcp_pair(n: &mut Net, port: u16) -> (usize, usize, usize) {
    let l = n.b.socket(Kind::Tcp).unwrap();
    n.b.bind(l, ep(Ipv4::UNSPECIFIED, port)).unwrap();
    n.b.listen(l, 4).unwrap();
    let c = n.a.socket(Kind::Tcp).unwrap();
    n.a.connect(c, ep(ip_b(), port), n.now).unwrap();
    assert_eq!(n.a.connect_status(c), Ok(false));
    for _ in 0..100 {
        n.tick(10);
        if n.a.connect_status(c) == Ok(true) {
            break;
        }
    }
    assert_eq!(n.a.connect_status(c), Ok(true));
    assert!(n.b.ready(l).unwrap().read);
    let (s, peer) = n.b.accept(l).unwrap();
    assert_eq!(peer, n.a.local_endpoint(c).unwrap());
    assert_eq!(n.b.accept(l), Err(Error::WouldBlock));
    (c, l, s)
}

#[test]
fn tcp_transfer_with_loss_and_close() {
    let mut n = Net::new();
    let (c, l, s) = tcp_pair(&mut n, 23);
    n.lose = 9;
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
    let mut sent = 0;
    let mut got = Vec::new();
    let mut buf = vec![0u8; 8192];
    while got.len() < data.len() {
        if sent < data.len() {
            match n.a.send(c, &data[sent..], n.now) {
                Ok(k) => sent += k,
                Err(Error::WouldBlock) => {}
                Err(e) => panic!("send: {e:?}"),
            }
        }
        n.tick(5);
        loop {
            match n.b.recv(s, &mut buf, false, n.now) {
                Ok(0) => panic!("early EOF"),
                Ok(k) => got.extend_from_slice(&buf[..k]),
                Err(Error::WouldBlock) => break,
                Err(e) => panic!("recv: {e:?}"),
            }
        }
        assert!(n.now < 3_000_000, "stalled at {}", got.len());
    }
    assert!(got == data);
    n.lose = 0;
    // Client closes; server sees EOF, closes too.
    n.a.close(c, n.now).unwrap();
    n.tick(10);
    assert_eq!(n.b.recv(s, &mut buf, false, n.now), Ok(0));
    n.b.close(s, n.now).unwrap();
    n.tick(10);
    // The client's connection lingers in TIME_WAIT, then disappears.
    assert_eq!(n.a.socket_count(), 1);
    n.tick(tcp::TIME_WAIT + 1);
    assert_eq!(n.a.socket_count(), 0);
    // The listener is still there; nothing else on b.
    assert_eq!(n.b.socket_count(), 1);
    n.b.close(l, n.now).unwrap();
    assert_eq!(n.b.socket_count(), 0);
}

#[test]
fn tcp_refused_and_unreachable_net() {
    let mut n = Net::new();
    let c = n.a.socket(Kind::Tcp).unwrap();
    n.a.connect(c, ep(ip_b(), 81), 0).unwrap();
    n.run();
    assert_eq!(n.a.connect_status(c), Err(Error::ConnRefused));
    // No route: the interface has no address.
    let mut lone = Stack::new(3, "x");
    let s = lone.socket(Kind::Tcp).unwrap();
    assert_eq!(lone.connect(s, ep(ip_b(), 80), 0), Err(Error::NetUnreach));
}

#[test]
fn tcp_reset_on_close_with_unread_data() {
    let mut n = Net::new();
    let (c, _l, s) = tcp_pair(&mut n, 2323);
    n.a.send(c, b"unread", n.now).unwrap();
    n.tick(1);
    n.b.close(s, n.now).unwrap();
    n.tick(1);
    let mut buf = [0u8; 8];
    assert_eq!(n.a.recv(c, &mut buf, false, n.now), Err(Error::ConnReset));
}

#[test]
fn loopback_tcp_and_select() {
    let mut st = Stack::new(9, "rs2");
    let l = st.socket(Kind::Tcp).unwrap();
    st.bind(l, ep(Ipv4::LOOPBACK, 7777)).unwrap();
    st.listen(l, 1).unwrap();
    let c = st.socket(Kind::Tcp).unwrap();
    st.connect(c, ep(Ipv4::LOOPBACK, 7777), 0).unwrap();
    // The handshake ran during connect().
    assert_eq!(st.connect_status(c), Ok(true));
    assert!(st.ready(c).unwrap().write);
    let (s, _) = st.accept(l).unwrap();
    assert!(!st.ready(s).unwrap().read);
    st.send(c, b"over loopback", 0).unwrap();
    assert!(st.ready(s).unwrap().read);
    let mut buf = [0u8; 32];
    assert_eq!(st.recv(s, &mut buf, true, 0), Ok(13));
    assert_eq!(st.available(s), Ok(13));
    assert_eq!(st.recv(s, &mut buf, false, 0), Ok(13));
    assert_eq!(&buf[..13], b"over loopback");
    // Binding the same port again fails, with SO_REUSEADDR too while the
    // listener exists.
    let x = st.socket(Kind::Tcp).unwrap();
    st.set_reuse_addr(x, true).unwrap();
    assert_eq!(st.bind(x, ep(Ipv4::UNSPECIFIED, 7777)), Err(Error::AddrInUse));
    st.close(s, 0).unwrap();
    st.close(c, 0).unwrap();
    st.close(l, 0).unwrap();
    // TIME_WAIT is left, which SO_REUSEADDR ignores.
    assert_eq!(st.bind(x, ep(Ipv4::UNSPECIFIED, 7777)), Ok(()));
}

#[test]
fn loopback_ping_and_udp() {
    let mut st = Stack::new(5, "rs2");
    let r = st.socket(Kind::Raw(ipv4::PROTO_ICMP)).unwrap();
    st.send_to(r, &icmp::echo(icmp::ECHO_REQUEST, 1, 2, b"lo"), ep(Ipv4::LOOPBACK, 0), 0).unwrap();
    let mut buf = [0u8; 100];
    // Our own request and the reply.
    let mut kinds = Vec::new();
    while let Ok((len, _)) = st.recv_from(r, &mut buf, false, 0) {
        kinds.push(buf[20]);
        assert!(len >= 28);
    }
    assert_eq!(kinds, vec![icmp::ECHO_REQUEST, icmp::ECHO_REPLY]);
    let u = st.socket(Kind::Udp).unwrap();
    st.bind(u, ep(Ipv4::LOOPBACK, 9999)).unwrap();
    st.send_to(u, b"self", ep(Ipv4::LOOPBACK, 9999), 0).unwrap();
    assert_eq!(st.recv_from(u, &mut buf, false, 0), Ok((4, ep(Ipv4::LOOPBACK, 9999))));
}

#[test]
fn dhcp_configures_interface() {
    let mut a = Stack::new(11, "rs2");
    let i = a.add_ethernet("eth0", MAC_A, 1500);
    a.set_link(i, true);
    a.start_dhcp(i, 0);
    let server = ip_b();
    let lease = dhcp::Lease {
        addr: ip_a(),
        mask: Ipv4::mask(24),
        router: Some(ip_b()),
        dns: vec![Ipv4::new(10, 0, 2, 3)],
        lease_time: 86400,
        ..Default::default()
    };
    // Act as the server for two exchanges (DISCOVER, REQUEST).
    for _ in 0..2 {
        let (_, f) = a.take_frame().expect("a DHCP message");
        let e = eth::parse(&f).unwrap();
        assert_eq!(e.dst, MAC_BROADCAST);
        let h = ipv4::parse(e.payload).unwrap();
        assert_eq!((h.src, h.dst), (Ipv4::UNSPECIFIED, Ipv4::BROADCAST));
        let d = udp::parse(h.src, h.dst, &e.payload[h.ihl..h.total_len]).unwrap();
        let (xid, mac, t) = dhcp::request_ids(d.data).unwrap();
        let reply_type = if t == dhcp::DISCOVER { dhcp::OFFER } else { dhcp::ACK };
        let reply = dhcp::server_reply(reply_type, xid, mac, ip_a(), server, &lease);
        let u = udp::build(server, 67, Ipv4::BROADCAST, 68, &reply);
        let ipd = ipv4::build(server, Ipv4::BROADCAST, ipv4::PROTO_UDP, 1, &u);
        a.receive(i, &eth::build(MAC_BROADCAST, MAC_B, eth::TYPE_IPV4, &ipd), 10);
    }
    assert_eq!(a.ifaces[i].addr, ip_a());
    assert_eq!(a.ifaces[i].gateway, Some(ip_b()));
    assert_eq!(a.dns_servers(), vec![Ipv4::new(10, 0, 2, 3)]);
    let log = a.take_log();
    assert!(log.iter().any(|l| l.contains("DHCP lease 10.0.2.15/24 gateway 10.0.2.2 dns 10.0.2.3")), "{log:?}");
}

#[test]
fn dns_resolution_and_cache() {
    let mut n = Net::new();
    // b plays the DNS server.
    let srv = n.b.socket(Kind::Udp).unwrap();
    n.b.bind(srv, ep(Ipv4::UNSPECIFIED, 53)).unwrap();
    let q = n.a.resolve_start("www.example.com", 0).unwrap();
    assert_eq!(n.a.resolve_result(q), None);
    n.run();
    let mut buf = [0u8; 512];
    let (len, from) = n.b.recv_from(srv, &mut buf, false, 0).unwrap();
    // Answer: the query with QR set and one A record.
    let mut r = buf[..len].to_vec();
    r[2] |= 0x80;
    r[7] = 1;
    r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 93, 184, 216, 34]);
    n.b.send_to(srv, &r, from, 0).unwrap();
    n.run();
    n.tick(1);
    assert_eq!(n.a.resolve_result(q), Some(Ok(vec![Ipv4::new(93, 184, 216, 34)])));
    // Cached now; numeric and local names need no server.
    let q2 = n.a.resolve_start("WWW.Example.com.", 2).unwrap();
    assert_eq!(n.a.resolve_result(q2), Some(Ok(vec![Ipv4::new(93, 184, 216, 34)])));
    let q3 = n.a.resolve_start("10.1.2.3", 2).unwrap();
    assert_eq!(n.a.resolve_result(q3), Some(Ok(vec![Ipv4::new(10, 1, 2, 3)])));
    let q4 = n.a.resolve_start("localhost", 2).unwrap();
    assert_eq!(n.a.resolve_result(q4), Some(Ok(vec![Ipv4::LOOPBACK])));
    // No answer: retries, then TRY_AGAIN.
    let q5 = n.a.resolve_start("silent.example", 3).unwrap();
    for _ in 0..10 {
        n.tick(DNS_TIMEOUT);
    }
    assert_eq!(n.a.resolve_result(q5), Some(Err(Error::TryAgain)));
}

#[test]
fn backlog_limits_pending_connections() {
    let mut n = Net::new();
    let l = n.b.socket(Kind::Tcp).unwrap();
    n.b.bind(l, ep(Ipv4::UNSPECIFIED, 80)).unwrap();
    n.b.listen(l, 1).unwrap();
    let c1 = n.a.socket(Kind::Tcp).unwrap();
    let c2 = n.a.socket(Kind::Tcp).unwrap();
    n.a.connect(c1, ep(ip_b(), 80), 0).unwrap();
    n.run();
    n.a.connect(c2, ep(ip_b(), 80), 0).unwrap();
    n.run();
    assert_eq!(n.a.connect_status(c1), Ok(true));
    // The second SYN was dropped: still trying.
    assert_eq!(n.a.connect_status(c2), Ok(false));
    n.b.accept(l).unwrap();
    // Its retransmitted SYN gets in.
    for _ in 0..40 {
        n.tick(100);
    }
    assert_eq!(n.a.connect_status(c2), Ok(true));
}
