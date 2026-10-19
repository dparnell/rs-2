//! The stack: interfaces, routing, ARP, IP input/output, sockets, the
//! DHCP clients and the resolver.
//!
//! Everything is driven from outside:
//!
//! * [`Stack::receive`] takes a frame an interface received;
//! * [`Stack::poll`] runs the timers (call it every few tens of ms, and
//!   after [`Stack::next_timer`]);
//! * [`Stack::take_frame`] hands out the frames to transmit;
//! * the socket calls ([`Stack::socket`], [`Stack::connect`], ...) never
//!   block: they fail with [`Error::WouldBlock`] and the caller waits for
//!   [`Stack::take_changed`] before trying again.
//!
//! Interface 0 is the loopback interface (127.0.0.1/8). Datagrams to
//! 127/8 or to one of our own addresses go through its queue and are
//! delivered before the call that sent them returns.
//!
//! Socket ids are indices into the socket table. A socket the application
//! closed stays until its TCP connection is finished (FIN handshake,
//! TIME_WAIT), then its slot is reused.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use crate::tcp::{self, Segment, Tcb};
use crate::{arp, dhcp, dns, eth, icmp, ipv4, udp};
use crate::{Endpoint, Ipv4, Mac, MacFmt, MAC_BROADCAST};

/// Socket errors; [`Error::errno`] gives the BSD error number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    WouldBlock,
    InProgress,
    Already,
    IsConnected,
    NotConnected,
    ConnRefused,
    ConnReset,
    ConnAborted,
    TimedOut,
    AddrInUse,
    AddrNotAvail,
    NetUnreach,
    HostUnreach,
    InvalidArg,
    MsgSize,
    BadSocket,
    /// Sending after shutdown (EPIPE).
    Shutdown,
    OpNotSupported,
    NoBufs,
    DestAddrRequired,
    /// Name resolution: no such host.
    HostNotFound,
    /// Name resolution: no answer (server failure or timeout).
    TryAgain,
    /// Name resolution: the name exists but has no address.
    NoData,
}

impl Error {
    /// The 4.4BSD errno value (OS/2's socket library adds SOCBASEERR).
    pub fn errno(self) -> u32 {
        match self {
            Error::BadSocket => 9,
            Error::InvalidArg => 22,
            Error::Shutdown => 32,
            Error::WouldBlock => 35,
            Error::InProgress => 36,
            Error::Already => 37,
            Error::DestAddrRequired => 39,
            Error::MsgSize => 40,
            Error::OpNotSupported => 45,
            Error::AddrInUse => 48,
            Error::AddrNotAvail => 49,
            Error::NetUnreach => 51,
            Error::ConnAborted => 53,
            Error::ConnReset => 54,
            Error::NoBufs => 55,
            Error::IsConnected => 56,
            Error::NotConnected => 57,
            Error::TimedOut => 60,
            Error::ConnRefused => 61,
            Error::HostUnreach => 65,
            Error::HostNotFound | Error::TryAgain | Error::NoData => 0,
        }
    }

    /// The resolver error number (h_errno: HOST_NOT_FOUND 1, TRY_AGAIN 2,
    /// NO_DATA 4).
    pub fn h_errno(self) -> u32 {
        match self {
            Error::HostNotFound => 1,
            Error::NoData => 4,
            _ => 2,
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub const LOOPBACK: usize = 0;
const LOOPBACK_MTU: usize = 16384;
const EPHEMERAL: core::ops::RangeInclusive<u16> = 49152..=65535;
/// Datagram receive queue limit (bytes) per socket.
const DGRAM_QUEUE: usize = 65536;
/// Largest UDP payload.
const MAX_UDP: usize = 65507;
/// Loopback deliveries per call (guards against endless ping-pong).
const LOOPBACK_BUDGET: usize = 10_000;
/// DNS: time per try, tries (servers are rotated), cache bound.
const DNS_TIMEOUT: u64 = 2000;
const DNS_TRIES: u32 = 4;
const DNS_CACHE: usize = 64;

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_dropped: u64,
}

/// A network interface.
pub struct Iface {
    pub name: String,
    pub mac: Mac,
    pub addr: Ipv4,
    pub mask: Ipv4,
    pub gateway: Option<Ipv4>,
    pub dns: Vec<Ipv4>,
    pub domain: String,
    pub mtu: usize,
    /// Link state as the driver reports it.
    pub link: bool,
    /// Administratively up.
    pub up: bool,
    pub loopback: bool,
    pub dhcp: Option<dhcp::Client>,
    pub stats: Stats,
}

impl Iface {
    fn configured(&self) -> bool {
        !self.addr.is_unspecified()
    }

    fn subnet_broadcast(&self) -> Ipv4 {
        Ipv4::from_u32(self.addr.to_u32() | !self.mask.to_u32())
    }

    fn is_broadcast(&self, a: Ipv4) -> bool {
        a.is_broadcast() || (self.configured() && self.mask.to_u32() != u32::MAX && a == self.subnet_broadcast())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tcp,
    Udp,
    /// Raw IP for a protocol (ICMP): sendto takes the protocol payload,
    /// recvfrom returns the IP header and payload.
    Raw(u8),
}

struct Listener {
    backlog: usize,
    /// Connections being set up (SYN_RECEIVED).
    pending: Vec<usize>,
    /// Established connections waiting for accept.
    ready: VecDeque<usize>,
}

struct Socket {
    kind: Kind,
    local: Endpoint,
    /// UDP/raw: the connected peer.
    remote: Option<Endpoint>,
    tcb: Option<Tcb>,
    listen: Option<Listener>,
    /// The listener of a connection not accepted yet.
    parent: Option<usize>,
    dgrams: VecDeque<(Endpoint, Vec<u8>)>,
    dgram_bytes: usize,
    send_buf: usize,
    recv_buf: usize,
    user_closed: bool,
    shut_rd: bool,
    shut_wr: bool,
    reuse_addr: bool,
    broadcast: bool,
    /// Pending asynchronous error (SO_ERROR).
    error: Option<Error>,
    /// Owned by the stack (the resolver).
    internal: bool,
}

impl Socket {
    fn new(kind: Kind) -> Socket {
        Socket {
            kind,
            local: Endpoint::default(),
            remote: None,
            tcb: None,
            listen: None,
            parent: None,
            dgrams: VecDeque::new(),
            dgram_bytes: 0,
            send_buf: tcp::SEND_BUFFER,
            recv_buf: tcp::RECV_BUFFER,
            user_closed: false,
            shut_rd: false,
            shut_wr: false,
            reuse_addr: false,
            broadcast: false,
            error: None,
            internal: false,
        }
    }

    fn tcp_active(&self) -> bool {
        self.tcb.as_ref().is_some_and(|t| !t.is_closed())
    }
}

/// Readiness of a socket for select().
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ready {
    pub read: bool,
    pub write: bool,
    pub except: bool,
}

/// A socket's TCP state for status displays.
#[derive(Clone, Copy, Debug)]
pub struct SocketInfo {
    pub kind: Kind,
    pub local: Endpoint,
    pub remote: Option<Endpoint>,
    pub state: Option<tcp::State>,
    pub listening: bool,
    pub user_closed: bool,
}

/// How to reach a destination.
#[derive(Clone, Copy, Debug)]
struct Route {
    iface: usize,
    next_hop: Ipv4,
    src: Ipv4,
    broadcast: bool,
}

struct Query {
    id: u16,
    name: String,
    tries: u32,
    sent_at: u64,
    result: Option<Result<Vec<Ipv4>>>,
}

struct CacheEntry {
    name: String,
    addrs: Vec<Ipv4>,
    expires: u64,
}

pub struct Stack {
    pub ifaces: Vec<Iface>,
    arp: arp::Cache,
    reasm: ipv4::Reassembler,
    sockets: Vec<Option<Socket>>,
    out: VecDeque<(usize, Vec<u8>)>,
    loopq: VecDeque<Vec<u8>>,
    next_port: u16,
    ip_id: u16,
    secret: u32,
    rand: u32,
    changed: bool,
    log: Vec<String>,
    pub hostname: String,
    queries: Vec<Query>,
    dns_socket: Option<usize>,
    dns_cache: Vec<CacheEntry>,
    now: u64,
}

fn mix(mut x: u32) -> u32 {
    // A 32-bit integer hash (lowbias32).
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

impl Stack {
    /// A stack with only the loopback interface. `seed` makes initial
    /// sequence numbers, ports and transaction ids unpredictable.
    pub fn new(seed: u32, hostname: &str) -> Stack {
        let lo = Iface {
            name: String::from("lo"),
            mac: [0; 6],
            addr: Ipv4::LOOPBACK,
            mask: Ipv4::mask(8),
            gateway: None,
            dns: Vec::new(),
            domain: String::new(),
            mtu: LOOPBACK_MTU,
            link: true,
            up: true,
            loopback: true,
            dhcp: None,
            stats: Stats::default(),
        };
        let r = mix(seed ^ 0x5bd1_e995);
        Stack {
            ifaces: alloc::vec![lo],
            arp: arp::Cache::default(),
            reasm: ipv4::Reassembler::default(),
            sockets: Vec::new(),
            out: VecDeque::new(),
            loopq: VecDeque::new(),
            next_port: *EPHEMERAL.start() + (r % 16384) as u16,
            ip_id: r as u16,
            secret: mix(seed),
            rand: r | 1,
            changed: false,
            log: Vec::new(),
            hostname: String::from(hostname),
            queries: Vec::new(),
            dns_socket: None,
            dns_cache: Vec::new(),
            now: 0,
        }
    }

    fn random(&mut self) -> u32 {
        let mut x = self.rand;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rand = x;
        x
    }

    fn note(&mut self, s: String) {
        self.log.push(s);
    }

    /// Messages for the system log (DHCP leases, ...).
    pub fn take_log(&mut self) -> Vec<String> {
        core::mem::take(&mut self.log)
    }

    /// Whether socket state may have changed since the last call (wake
    /// the threads waiting on sockets).
    pub fn take_changed(&mut self) -> bool {
        core::mem::replace(&mut self.changed, false)
    }

    // ---- Interfaces ----

    /// Add an Ethernet interface; returns its index.
    pub fn add_ethernet(&mut self, name: &str, mac: Mac, mtu: usize) -> usize {
        self.ifaces.push(Iface {
            name: String::from(name),
            mac,
            addr: Ipv4::UNSPECIFIED,
            mask: Ipv4::UNSPECIFIED,
            gateway: None,
            dns: Vec::new(),
            domain: String::new(),
            mtu: mtu.clamp(576, eth::MTU),
            link: false,
            up: true,
            loopback: false,
            dhcp: None,
            stats: Stats::default(),
        });
        self.ifaces.len() - 1
    }

    pub fn set_link(&mut self, iface: usize, up: bool) {
        let i = &mut self.ifaces[iface];
        if i.link != up {
            i.link = up;
            let s = alloc::format!("{}: link {}", i.name, if up { "up" } else { "down" });
            self.note(s);
        }
    }

    /// Static configuration.
    pub fn configure(&mut self, iface: usize, addr: Ipv4, mask: Ipv4, gateway: Option<Ipv4>, dns: &[Ipv4]) {
        let i = &mut self.ifaces[iface];
        i.addr = addr;
        i.mask = mask;
        i.gateway = gateway;
        i.dns = dns.to_vec();
        self.arp.flush(iface);
        self.changed = true;
    }

    /// Start DHCP on an interface (its address is unset until a lease).
    pub fn start_dhcp(&mut self, iface: usize, now: u64) {
        let seed = mix(self.secret ^ iface as u32 ^ now as u32);
        let mac = self.ifaces[iface].mac;
        let host = self.hostname.clone();
        self.ifaces[iface].dhcp = Some(dhcp::Client::new(mac, &host, seed));
        self.poll(now);
    }

    pub fn iface_up(&mut self, iface: usize, up: bool) {
        self.ifaces[iface].up = up;
    }

    /// The DNS servers of all interfaces.
    pub fn dns_servers(&self) -> Vec<Ipv4> {
        let mut v: Vec<Ipv4> = Vec::new();
        for i in &self.ifaces {
            for d in &i.dns {
                if !v.contains(d) {
                    v.push(*d);
                }
            }
        }
        v
    }

    fn is_local(&self, a: Ipv4) -> bool {
        a.is_loopback() || self.ifaces.iter().any(|i| i.configured() && i.addr == a)
    }

    fn route(&self, dst: Ipv4, src_hint: Ipv4) -> Result<Route> {
        if self.is_local(dst) {
            let src = if dst.is_loopback() { Ipv4::LOOPBACK } else { dst };
            return Ok(Route { iface: LOOPBACK, next_hop: dst, src, broadcast: false });
        }
        let usable = |i: &Iface| !i.loopback && i.up && i.link;
        if dst.is_broadcast() {
            // Limited broadcast: the interface of the source address, else
            // the first one.
            let i = (0..self.ifaces.len())
                .find(|&n| usable(&self.ifaces[n]) && !src_hint.is_unspecified() && self.ifaces[n].addr == src_hint)
                .or_else(|| (0..self.ifaces.len()).find(|&n| usable(&self.ifaces[n])))
                .ok_or(Error::NetUnreach)?;
            return Ok(Route { iface: i, next_hop: dst, src: self.ifaces[i].addr, broadcast: true });
        }
        for (n, i) in self.ifaces.iter().enumerate() {
            if usable(i) && i.configured() && dst.same_net(i.addr, i.mask) {
                return Ok(Route { iface: n, next_hop: dst, src: i.addr, broadcast: i.is_broadcast(dst) });
            }
        }
        for (n, i) in self.ifaces.iter().enumerate() {
            if let (true, true, Some(gw)) = (usable(i), i.configured(), i.gateway) {
                return Ok(Route { iface: n, next_hop: gw, src: i.addr, broadcast: false });
            }
        }
        Err(Error::NetUnreach)
    }

    // ---- Output ----

    /// A frame to transmit on an interface.
    pub fn take_frame(&mut self) -> Option<(usize, Vec<u8>)> {
        self.out.pop_front()
    }

    fn send_frame(&mut self, iface: usize, frame: Vec<u8>) {
        let i = &mut self.ifaces[iface];
        i.stats.tx_packets += 1;
        i.stats.tx_bytes += frame.len() as u64;
        self.out.push_back((iface, frame));
    }

    fn send_arp(&mut self, iface: usize, op: u16, target_mac: Mac, target_ip: Ipv4, dst: Mac) {
        let i = &self.ifaces[iface];
        let p = arp::Packet { op, sender_mac: i.mac, sender_ip: i.addr, target_mac, target_ip };
        let f = eth::build(dst, i.mac, eth::TYPE_ARP, &arp::build(&p));
        self.send_frame(iface, f);
    }

    /// Put a datagram on the wire towards `next_hop`.
    fn transmit(&mut self, route: Route, datagram: Vec<u8>, now: u64) {
        if route.iface == LOOPBACK {
            let i = &mut self.ifaces[LOOPBACK];
            i.stats.tx_packets += 1;
            i.stats.tx_bytes += datagram.len() as u64;
            self.loopq.push_back(datagram);
            return;
        }
        let mac = self.ifaces[route.iface].mac;
        if route.broadcast {
            let f = eth::build(MAC_BROADCAST, mac, eth::TYPE_IPV4, &datagram);
            self.send_frame(route.iface, f);
            return;
        }
        match self.arp.lookup(route.iface, route.next_hop, now) {
            Some(dst) => {
                let f = eth::build(dst, mac, eth::TYPE_IPV4, &datagram);
                self.send_frame(route.iface, f);
            }
            None => {
                if self.arp.park(route.iface, route.next_hop, datagram, now) {
                    self.send_arp(route.iface, arp::OP_REQUEST, [0; 6], route.next_hop, MAC_BROADCAST);
                }
                // Raced with an answer: deliver what is parked.
                if let Some(dst) = self.arp.lookup(route.iface, route.next_hop, now) {
                    for d in self.arp.take_parked(route.iface, route.next_hop) {
                        let f = eth::build(dst, mac, eth::TYPE_IPV4, &d);
                        self.send_frame(route.iface, f);
                    }
                }
            }
        }
    }

    /// Send an IP datagram; `src` unspecified picks the route's address.
    fn ip_send(&mut self, src: Ipv4, dst: Ipv4, proto: u8, payload: &[u8], now: u64) -> Result<()> {
        let route = self.route(dst, src)?;
        let src = if src.is_unspecified() { route.src } else { src };
        self.ip_id = self.ip_id.wrapping_add(1);
        let mtu = self.ifaces[route.iface].mtu;
        for frag in ipv4::fragment(src, dst, proto, self.ip_id, payload, mtu) {
            self.transmit(route, frag, now);
        }
        Ok(())
    }

    fn tcp_send(&mut self, src: Ipv4, dst: Ipv4, seg: &Segment, now: u64) {
        let b = tcp::build(src, dst, seg);
        let _ = self.ip_send(src, dst, ipv4::PROTO_TCP, &b, now);
    }

    // ---- Input ----

    /// A frame received on `iface`.
    pub fn receive(&mut self, iface: usize, frame: &[u8], now: u64) {
        self.now = now;
        {
            let i = &mut self.ifaces[iface];
            i.stats.rx_packets += 1;
            i.stats.rx_bytes += frame.len() as u64;
        }
        let Some(f) = eth::parse(frame) else { return };
        let mac = self.ifaces[iface].mac;
        if f.dst != mac && f.dst != MAC_BROADCAST {
            return;
        }
        match f.ethertype {
            eth::TYPE_ARP => self.arp_input(iface, f.payload, now),
            eth::TYPE_IPV4 => self.ip_input(iface, f.payload, now),
            _ => self.ifaces[iface].stats.rx_dropped += 1,
        }
        self.finish(now);
    }

    fn arp_input(&mut self, iface: usize, b: &[u8], now: u64) {
        let Some(p) = arp::parse(b) else { return };
        let me = self.ifaces[iface].addr;
        let for_me = self.ifaces[iface].configured() && p.target_ip == me;
        if p.sender_ip.is_unspecified() {
            return;
        }
        let parked = self.arp.update(iface, p.sender_ip, p.sender_mac, now, for_me);
        let mac = self.ifaces[iface].mac;
        for d in parked {
            let f = eth::build(p.sender_mac, mac, eth::TYPE_IPV4, &d);
            self.send_frame(iface, f);
        }
        if for_me && p.op == arp::OP_REQUEST {
            self.send_arp(iface, arp::OP_REPLY, p.sender_mac, p.sender_ip, p.sender_mac);
        }
    }

    fn accepts(&self, iface: usize, dst: Ipv4) -> bool {
        let i = &self.ifaces[iface];
        if i.loopback {
            return true;
        }
        (i.configured() && i.addr == dst) || i.is_broadcast(dst) || (!i.configured() && i.dhcp.is_some())
    }

    fn ip_input(&mut self, iface: usize, b: &[u8], now: u64) {
        let Some(h) = ipv4::parse(b) else {
            self.ifaces[iface].stats.rx_dropped += 1;
            return;
        };
        if !self.accepts(iface, h.dst) {
            return;
        }
        let whole;
        let payload: &[u8] = if h.more_fragments || h.frag_offset > 0 {
            match self.reasm.add(&h, &b[h.ihl..h.total_len], now) {
                Some(p) => {
                    whole = p;
                    &whole
                }
                None => return,
            }
        } else {
            &b[h.ihl..h.total_len]
        };
        let broadcast = self.ifaces[iface].is_broadcast(h.dst) || h.dst.is_multicast();
        self.raw_deliver(&h, payload);
        match h.proto {
            ipv4::PROTO_ICMP => self.icmp_input(&h, payload, broadcast, now),
            ipv4::PROTO_UDP => self.udp_input(iface, &h, b, payload, broadcast, now),
            ipv4::PROTO_TCP if !broadcast => self.tcp_input(&h, payload, now),
            ipv4::PROTO_TCP => {}
            _ => {
                if !broadcast && !self.sockets.iter().flatten().any(|s| s.kind == Kind::Raw(h.proto)) {
                    let m = icmp::unreachable(icmp::UNREACH_PROTOCOL, b, h.ihl);
                    let _ = self.ip_send(h.dst, h.src, ipv4::PROTO_ICMP, &m, now);
                }
            }
        }
    }

    /// Copies for the raw sockets of the protocol: IP header and payload.
    fn raw_deliver(&mut self, h: &ipv4::Header, payload: &[u8]) {
        let kind = Kind::Raw(h.proto);
        if !self.sockets.iter().flatten().any(|s| s.kind == kind) {
            return;
        }
        let mut d = Vec::with_capacity(ipv4::HEADER + payload.len());
        d.extend_from_slice(&ipv4::header(h.src, h.dst, h.proto, h.id, payload.len(), 0, h.ttl));
        d.extend_from_slice(payload);
        for s in self.sockets.iter_mut().flatten() {
            if s.kind == kind && !s.shut_rd && s.remote.is_none_or(|r| r.addr == h.src) && s.dgram_bytes + d.len() <= DGRAM_QUEUE {
                s.dgram_bytes += d.len();
                s.dgrams.push_back((Endpoint::new(h.src, 0), d.clone()));
                self.changed = true;
            }
        }
    }

    fn icmp_input(&mut self, h: &ipv4::Header, b: &[u8], broadcast: bool, now: u64) {
        let Some(m) = icmp::parse(b) else { return };
        match m.kind {
            icmp::ECHO_REQUEST if !broadcast => {
                let r = icmp::build(icmp::ECHO_REPLY, 0, m.rest, m.data);
                let _ = self.ip_send(h.dst, h.src, ipv4::PROTO_ICMP, &r, now);
            }
            icmp::DEST_UNREACHABLE => self.icmp_error(m.code, m.data),
            _ => {}
        }
    }

    /// Destination unreachable: hard errors for connected UDP sockets and
    /// for connection attempts.
    fn icmp_error(&mut self, code: u8, quoted: &[u8]) {
        if quoted.len() < ipv4::HEADER + 8 || quoted[0] >> 4 != 4 {
            return;
        }
        let ihl = (quoted[0] & 0xf) as usize * 4;
        if quoted.len() < ihl + 8 {
            return;
        }
        let proto = quoted[9];
        let src = crate::wire::ip_at(quoted, 12);
        let dst = crate::wire::ip_at(quoted, 16);
        let sport = crate::wire::be16(quoted, ihl);
        let dport = crate::wire::be16(quoted, ihl + 2);
        let err = match code {
            icmp::UNREACH_PORT | icmp::UNREACH_PROTOCOL => Error::ConnRefused,
            icmp::UNREACH_NET => Error::NetUnreach,
            _ => Error::HostUnreach,
        };
        for s in self.sockets.iter_mut().flatten() {
            if s.local.port != sport || s.local.addr != src && !s.local.addr.is_unspecified() {
                continue;
            }
            let remote = Endpoint::new(dst, dport);
            if proto == ipv4::PROTO_UDP && s.kind == Kind::Udp && s.remote == Some(remote) {
                s.error = Some(err);
                self.changed = true;
            }
            if proto == ipv4::PROTO_TCP && s.kind == Kind::Tcp {
                if let Some(t) = &mut s.tcb {
                    // Hard error only while connecting (RFC 1122 4.2.3.9).
                    if t.remote == remote && t.state == tcp::State::SynSent && code != icmp::UNREACH_NET {
                        t.error = Some(err);
                        t.state = tcp::State::Closed;
                        self.changed = true;
                    }
                }
            }
        }
    }

    fn udp_input(&mut self, iface: usize, h: &ipv4::Header, packet: &[u8], b: &[u8], broadcast: bool, now: u64) {
        let Some(d) = udp::parse(h.src, h.dst, b) else {
            self.ifaces[iface].stats.rx_dropped += 1;
            return;
        };
        if d.dst_port == dhcp::CLIENT_PORT && d.src_port == dhcp::SERVER_PORT && self.ifaces[iface].dhcp.is_some() {
            let mut acts = Vec::new();
            if let Some(c) = &mut self.ifaces[iface].dhcp {
                c.input(d.data, now, &mut acts);
            }
            self.dhcp_actions(iface, acts, now);
            return;
        }
        let from = Endpoint::new(h.src, d.src_port);
        let mut delivered = false;
        for s in self.sockets.iter_mut().flatten() {
            if s.kind != Kind::Udp || s.local.port != d.dst_port || s.shut_rd {
                continue;
            }
            if !s.local.addr.is_unspecified() && s.local.addr != h.dst && !broadcast {
                continue;
            }
            if s.remote.is_some_and(|r| r != from) {
                continue;
            }
            delivered = true;
            if s.dgram_bytes + d.data.len() <= DGRAM_QUEUE.max(s.recv_buf) {
                s.dgram_bytes += d.data.len();
                s.dgrams.push_back((from, d.data.to_vec()));
                self.changed = true;
            }
            if !broadcast {
                break;
            }
        }
        if !delivered && !broadcast {
            let m = icmp::unreachable(icmp::UNREACH_PORT, packet, h.ihl);
            let _ = self.ip_send(h.dst, h.src, ipv4::PROTO_ICMP, &m, now);
        }
    }

    fn tcp_input(&mut self, h: &ipv4::Header, b: &[u8], now: u64) {
        let Some(seg) = tcp::parse(h.src, h.dst, b) else { return };
        let local = Endpoint::new(h.dst, seg.dst_port);
        let remote = Endpoint::new(h.src, seg.src_port);
        // An existing connection.
        let conn = self.sockets.iter().position(|s| {
            s.as_ref().is_some_and(|s| s.tcb.as_ref().is_some_and(|t| !t.is_closed() && t.local == local && t.remote == remote))
        });
        if let Some(id) = conn {
            let s = self.sockets[id].as_mut().unwrap();
            let t = s.tcb.as_mut().unwrap();
            t.input(&seg, now);
            self.changed = true;
            self.tcp_after_input(id);
            return;
        }
        // A listener.
        let lst = self.sockets.iter().position(|s| {
            s.as_ref().is_some_and(|s| {
                s.listen.is_some() && !s.user_closed && s.local.port == local.port && (s.local.addr.is_unspecified() || s.local.addr == local.addr)
            })
        });
        match lst {
            Some(l) if seg.has(tcp::SYN) && !seg.has(tcp::ACK) && !seg.has(tcp::RST) => {
                let (backlog, full, bufs) = {
                    let s = self.sockets[l].as_ref().unwrap();
                    let q = s.listen.as_ref().unwrap();
                    (q.backlog, q.pending.len() + q.ready.len(), (s.send_buf, s.recv_buf))
                };
                if full >= backlog {
                    // Dropped; the peer retries.
                    return;
                }
                let iss = self.iss(local, remote, now);
                let mss = self.mss_for(remote.addr);
                let mut t = Tcb::accept(local, remote, &seg, iss, mss);
                t.set_buffers(bufs.0, bufs.1);
                let mut child = Socket::new(Kind::Tcp);
                child.local = local;
                child.remote = Some(remote);
                child.send_buf = bufs.0;
                child.recv_buf = bufs.1;
                child.tcb = Some(t);
                child.parent = Some(l);
                let id = self.alloc_socket(child);
                self.sockets[l].as_mut().unwrap().listen.as_mut().unwrap().pending.push(id);
            }
            Some(_) if seg.has(tcp::RST) => {}
            _ => {
                if let Some(r) = tcp::reset_for(&seg) {
                    self.tcp_send(h.dst, h.src, &r, now);
                }
            }
        }
    }

    /// Bookkeeping after a connection's state may have changed: move
    /// established connections to their listener's accept queue.
    fn tcp_after_input(&mut self, id: usize) {
        let Some(s) = self.sockets[id].as_ref() else { return };
        let Some(parent) = s.parent else { return };
        let state = s.tcb.as_ref().map(|t| t.state);
        let established = !matches!(state, Some(tcp::State::SynReceived) | Some(tcp::State::Closed) | None);
        if !established {
            return;
        }
        if let Some(q) = self.sockets[parent].as_mut().and_then(|p| p.listen.as_mut()) {
            if let Some(i) = q.pending.iter().position(|&x| x == id) {
                q.pending.remove(i);
                q.ready.push_back(id);
                self.changed = true;
            }
        }
    }

    fn mss_for(&self, dst: Ipv4) -> u16 {
        let mtu = self.route(dst, Ipv4::UNSPECIFIED).map_or(eth::MTU, |r| self.ifaces[r.iface].mtu);
        (mtu - ipv4::HEADER - tcp::HEADER) as u16
    }

    /// RFC 6528 initial sequence number: a 4 µs clock plus a keyed hash of
    /// the connection.
    fn iss(&self, l: Endpoint, r: Endpoint, now: u64) -> u32 {
        let h = mix(self.secret ^ l.addr.to_u32())
            ^ mix(r.addr.to_u32().rotate_left(7) ^ ((l.port as u32) << 16 | r.port as u32) ^ self.secret.rotate_left(13));
        h.wrapping_add((now as u32).wrapping_mul(250))
    }

    // ---- Timers ----

    /// Run the timers; call every few tens of milliseconds.
    pub fn poll(&mut self, now: u64) {
        self.now = now;
        let mut acts = Vec::new();
        self.arp.poll(now, &mut acts);
        for arp::Action::Request(i, ip) in acts {
            self.send_arp(i, arp::OP_REQUEST, [0; 6], ip, MAC_BROADCAST);
        }
        self.reasm.poll(now);
        for i in 0..self.ifaces.len() {
            let mut acts = Vec::new();
            let link = self.ifaces[i].link && self.ifaces[i].up;
            if let (true, Some(c)) = (link, &mut self.ifaces[i].dhcp) {
                c.poll(now, &mut acts);
            }
            self.dhcp_actions(i, acts, now);
        }
        for s in self.sockets.iter_mut().flatten() {
            if let Some(t) = &mut s.tcb {
                let before = t.state;
                t.poll(now);
                if t.state != before {
                    self.changed = true;
                }
            }
        }
        self.dns_poll(now);
        self.finish(now);
    }

    /// The earliest TCP timer (for a caller that sleeps between polls).
    pub fn next_timer(&self) -> Option<u64> {
        self.sockets.iter().flatten().filter_map(|s| s.tcb.as_ref().and_then(|t| t.next_timer())).min()
    }

    /// End of every operation: transmit what the connections have to
    /// say, run the loopback queue to completion, free finished sockets.
    fn finish(&mut self, now: u64) {
        let mut budget = LOOPBACK_BUDGET;
        loop {
            self.tcp_output_all(now);
            let Some(d) = self.loopq.pop_front() else { break };
            {
                let i = &mut self.ifaces[LOOPBACK];
                i.stats.rx_packets += 1;
                i.stats.rx_bytes += d.len() as u64;
            }
            self.ip_input(LOOPBACK, &d, now);
            budget -= 1;
            if budget == 0 {
                self.loopq.clear();
                break;
            }
        }
        self.reap();
    }

    fn tcp_output_all(&mut self, now: u64) {
        let mut segs = Vec::new();
        for id in 0..self.sockets.len() {
            let Some(s) = self.sockets[id].as_mut() else { continue };
            let Some(t) = &mut s.tcb else { continue };
            t.output(now, &mut segs);
            if segs.is_empty() {
                continue;
            }
            let (src, dst) = (t.local.addr, t.remote.addr);
            for seg in segs.drain(..) {
                self.tcp_send(src, dst, &seg, now);
            }
        }
    }

    /// Free sockets nobody uses any more.
    fn reap(&mut self) {
        for id in 0..self.sockets.len() {
            let Some(s) = self.sockets[id].as_ref() else { continue };
            let tcp_done = s.tcb.as_ref().is_none_or(|t| t.is_closed());
            if let Some(p) = s.parent {
                // A connection that died before it was accepted.
                if tcp_done && s.tcb.as_ref().is_some_and(|t| t.is_closed()) {
                    let still_pending = self.sockets[p]
                        .as_ref()
                        .and_then(|l| l.listen.as_ref())
                        .is_some_and(|q| q.pending.contains(&id));
                    if still_pending {
                        if let Some(q) = self.sockets[p].as_mut().and_then(|l| l.listen.as_mut()) {
                            q.pending.retain(|&x| x != id);
                        }
                        self.sockets[id] = None;
                    }
                }
                continue;
            }
            if s.user_closed && tcp_done {
                self.sockets[id] = None;
            }
        }
    }

    // ---- DHCP ----

    fn dhcp_actions(&mut self, iface: usize, acts: Vec<dhcp::Action>, now: u64) {
        for a in acts {
            match a {
                dhcp::Action::Broadcast(p) => {
                    let src = self.ifaces[iface].addr;
                    let u = udp::build(src, dhcp::CLIENT_PORT, Ipv4::BROADCAST, dhcp::SERVER_PORT, &p);
                    self.ip_id = self.ip_id.wrapping_add(1);
                    let d = ipv4::build(src, Ipv4::BROADCAST, ipv4::PROTO_UDP, self.ip_id, &u);
                    let route = Route { iface, next_hop: Ipv4::BROADCAST, src, broadcast: true };
                    self.transmit(route, d, now);
                }
                dhcp::Action::Unicast(server, p) => {
                    let src = self.ifaces[iface].addr;
                    let u = udp::build(src, dhcp::CLIENT_PORT, server, dhcp::SERVER_PORT, &p);
                    let _ = self.ip_send(src, server, ipv4::PROTO_UDP, &u, now);
                }
                dhcp::Action::Bound(l) => {
                    let changed = self.ifaces[iface].addr != l.addr;
                    let i = &mut self.ifaces[iface];
                    i.addr = l.addr;
                    i.mask = l.mask;
                    i.gateway = l.router;
                    i.dns = l.dns.clone();
                    i.domain = l.domain.clone();
                    if changed {
                        self.arp.flush(iface);
                        let gw = l.router.map_or(String::from("none"), |g| alloc::format!("{g}"));
                        let dns: Vec<String> = l.dns.iter().map(|d| alloc::format!("{d}")).collect();
                        let msg = alloc::format!(
                            "{}: DHCP lease {}/{} gateway {} dns {} from {} ({} s)",
                            self.ifaces[iface].name,
                            l.addr,
                            l.mask.prefix_len(),
                            gw,
                            if dns.is_empty() { String::from("none") } else { dns.join(",") },
                            l.server,
                            l.lease_time
                        );
                        self.note(msg);
                        // Announce ourselves (gratuitous ARP).
                        let me = l.addr;
                        self.send_arp(iface, arp::OP_REQUEST, [0; 6], me, MAC_BROADCAST);
                    }
                    self.changed = true;
                }
                dhcp::Action::Lost => {
                    let i = &mut self.ifaces[iface];
                    let msg = alloc::format!("{}: DHCP lease of {} lost", i.name, i.addr);
                    i.addr = Ipv4::UNSPECIFIED;
                    i.mask = Ipv4::UNSPECIFIED;
                    i.gateway = None;
                    i.dns.clear();
                    self.arp.flush(iface);
                    self.note(msg);
                    self.changed = true;
                }
            }
        }
    }

    // ---- Sockets ----

    fn alloc_socket(&mut self, s: Socket) -> usize {
        match self.sockets.iter().position(|x| x.is_none()) {
            Some(i) => {
                self.sockets[i] = Some(s);
                i
            }
            None => {
                self.sockets.push(Some(s));
                self.sockets.len() - 1
            }
        }
    }

    fn sock(&mut self, id: usize) -> Result<&mut Socket> {
        match self.sockets.get_mut(id) {
            Some(Some(s)) if !s.user_closed => Ok(s),
            _ => Err(Error::BadSocket),
        }
    }

    fn sock_ref(&self, id: usize) -> Result<&Socket> {
        match self.sockets.get(id) {
            Some(Some(s)) if !s.user_closed => Ok(s),
            _ => Err(Error::BadSocket),
        }
    }

    pub fn socket(&mut self, kind: Kind) -> Result<usize> {
        Ok(self.alloc_socket(Socket::new(kind)))
    }

    fn port_in_use(&self, kind: Kind, ep: Endpoint, reuse: bool, except: Option<usize>) -> bool {
        self.sockets.iter().enumerate().any(|(i, s)| {
            let Some(s) = s else { return false };
            if Some(i) == except || s.kind != kind || s.local.port != ep.port || s.parent.is_some() {
                return false;
            }
            if s.user_closed && !s.tcp_active() {
                return false;
            }
            // SO_REUSEADDR: connections (TIME_WAIT ones included) do not
            // block a new listener; datagram sockets may share the port
            // when they all ask for it.
            if reuse && (s.tcb.is_some() || (kind != Kind::Tcp && s.reuse_addr)) {
                return false;
            }
            s.local.addr.is_unspecified() || ep.addr.is_unspecified() || s.local.addr == ep.addr
        })
    }

    fn ephemeral(&mut self, kind: Kind, addr: Ipv4) -> Result<u16> {
        let span = (*EPHEMERAL.end() - *EPHEMERAL.start()) as u32 + 1;
        for _ in 0..span {
            let p = self.next_port;
            self.next_port = if p >= *EPHEMERAL.end() { *EPHEMERAL.start() } else { p + 1 };
            if !self.port_in_use(kind, Endpoint::new(addr, p), false, None) {
                return Ok(p);
            }
        }
        Err(Error::AddrInUse)
    }

    pub fn bind(&mut self, id: usize, ep: Endpoint) -> Result<()> {
        let (kind, bound, reuse) = {
            let s = self.sock(id)?;
            (s.kind, s.local.port != 0 || s.tcb.is_some(), s.reuse_addr)
        };
        if bound {
            return Err(Error::InvalidArg);
        }
        if !ep.addr.is_unspecified() && !self.is_local(ep.addr) && !ep.addr.is_broadcast() {
            return Err(Error::AddrNotAvail);
        }
        let port = if ep.port == 0 && !matches!(kind, Kind::Raw(_)) {
            self.ephemeral(kind, ep.addr)?
        } else {
            if ep.port != 0 && self.port_in_use(kind, ep, reuse, Some(id)) {
                return Err(Error::AddrInUse);
            }
            ep.port
        };
        let s = self.sock(id)?;
        s.local = Endpoint::new(ep.addr, port);
        Ok(())
    }

    fn autobind(&mut self, id: usize) -> Result<()> {
        if self.sock(id)?.local.port == 0 {
            let addr = self.sock(id)?.local.addr;
            self.bind_port(id, addr)?;
        }
        Ok(())
    }

    fn bind_port(&mut self, id: usize, addr: Ipv4) -> Result<()> {
        let kind = self.sock(id)?.kind;
        let p = self.ephemeral(kind, addr)?;
        self.sock(id)?.local = Endpoint::new(addr, p);
        Ok(())
    }

    pub fn listen(&mut self, id: usize, backlog: usize) -> Result<()> {
        let s = self.sock(id)?;
        if s.kind != Kind::Tcp {
            return Err(Error::OpNotSupported);
        }
        if s.tcb.is_some() {
            return Err(Error::IsConnected);
        }
        let backlog = backlog.clamp(1, 64);
        if let Some(l) = &mut s.listen {
            l.backlog = backlog;
            return Ok(());
        }
        if s.local.port == 0 {
            self.autobind(id)?;
        }
        let s = self.sock(id)?;
        s.listen = Some(Listener { backlog, pending: Vec::new(), ready: VecDeque::new() });
        Ok(())
    }

    /// Take an established connection from a listener.
    pub fn accept(&mut self, id: usize) -> Result<(usize, Endpoint)> {
        let s = self.sock(id)?;
        let Some(l) = &mut s.listen else { return Err(Error::InvalidArg) };
        let Some(child) = l.ready.pop_front() else { return Err(Error::WouldBlock) };
        let c = self.sockets[child].as_mut().unwrap();
        c.parent = None;
        let peer = c.remote.unwrap_or_default();
        Ok((child, peer))
    }

    /// Start a TCP connection (completion: [`Stack::connect_status`]) or
    /// set a UDP/raw socket's peer.
    pub fn connect(&mut self, id: usize, to: Endpoint, now: u64) -> Result<()> {
        self.now = now;
        let kind = self.sock(id)?.kind;
        if to.addr.is_unspecified() || (kind == Kind::Tcp && to.port == 0) {
            return Err(Error::InvalidArg);
        }
        let src_hint = self.sock(id)?.local.addr;
        let route = self.route(to.addr, src_hint)?;
        match kind {
            Kind::Tcp => {
                let s = self.sock(id)?;
                if s.listen.is_some() {
                    return Err(Error::OpNotSupported);
                }
                if let Some(t) = &s.tcb {
                    return Err(if t.is_connecting() { Error::Already } else { Error::IsConnected });
                }
                if route.broadcast {
                    return Err(Error::NetUnreach);
                }
                if s.local.addr.is_unspecified() {
                    s.local.addr = route.src;
                }
                let addr = s.local.addr;
                if s.local.port == 0 {
                    self.bind_port(id, addr)?;
                }
                let local = self.sock(id)?.local;
                let iss = self.iss(local, to, now);
                let mss = self.mss_for(to.addr);
                let s = self.sock(id)?;
                let mut t = Tcb::connect(local, to, iss, mss);
                t.set_buffers(s.send_buf, s.recv_buf);
                s.tcb = Some(t);
                s.remote = Some(to);
                self.finish(now);
                Ok(())
            }
            Kind::Udp | Kind::Raw(_) => {
                if route.broadcast && !self.sock(id)?.broadcast {
                    return Err(Error::InvalidArg);
                }
                if kind == Kind::Udp {
                    self.autobind(id)?;
                }
                let s = self.sock(id)?;
                s.remote = Some(to);
                Ok(())
            }
        }
    }

    /// Ok(true) once a TCP connection is established, Ok(false) while it
    /// is being set up, the error if it failed.
    pub fn connect_status(&mut self, id: usize) -> Result<bool> {
        let s = self.sock(id)?;
        let Some(t) = &mut s.tcb else { return Err(Error::NotConnected) };
        if t.is_connecting() {
            return Ok(false);
        }
        if let Some(e) = t.error {
            return Err(e);
        }
        if t.is_closed() {
            return Err(Error::ConnRefused);
        }
        Ok(true)
    }

    /// Send on a connected socket.
    pub fn send(&mut self, id: usize, data: &[u8], now: u64) -> Result<usize> {
        self.now = now;
        let s = self.sock(id)?;
        if s.shut_wr {
            return Err(Error::Shutdown);
        }
        match s.kind {
            Kind::Tcp => {
                let Some(t) = &mut s.tcb else { return Err(Error::NotConnected) };
                if let Some(e) = t.error.take() {
                    return Err(e);
                }
                if t.is_connecting() {
                    return Err(Error::WouldBlock);
                }
                if t.is_closed() {
                    return Err(Error::Shutdown);
                }
                let n = t.send(data)?;
                if n == 0 && !data.is_empty() {
                    return Err(Error::WouldBlock);
                }
                self.finish(now);
                Ok(n)
            }
            _ => {
                let Some(to) = s.remote else { return Err(Error::DestAddrRequired) };
                self.send_to(id, data, to, now)
            }
        }
    }

    /// Send a datagram (UDP: payload; raw: the protocol message).
    pub fn send_to(&mut self, id: usize, data: &[u8], to: Endpoint, now: u64) -> Result<usize> {
        self.now = now;
        let (kind, broadcast_ok, shut) = {
            let s = self.sock(id)?;
            (s.kind, s.broadcast, s.shut_wr)
        };
        if shut {
            return Err(Error::Shutdown);
        }
        match kind {
            Kind::Tcp => {
                // sendto on a connected stream ignores the address.
                self.send(id, data, now)
            }
            Kind::Udp => {
                if data.len() > MAX_UDP {
                    return Err(Error::MsgSize);
                }
                self.autobind(id)?;
                let local = self.sock(id)?.local;
                let route = self.route(to.addr, local.addr)?;
                if route.broadcast && !broadcast_ok {
                    return Err(Error::InvalidArg);
                }
                let src = if local.addr.is_unspecified() || local.addr.is_broadcast() { route.src } else { local.addr };
                let u = udp::build(src, local.port, to.addr, to.port, data);
                self.ip_send(src, to.addr, ipv4::PROTO_UDP, &u, now)?;
                self.finish(now);
                Ok(data.len())
            }
            Kind::Raw(proto) => {
                if data.len() > 65515 {
                    return Err(Error::MsgSize);
                }
                let local = self.sock(id)?.local;
                let route = self.route(to.addr, local.addr)?;
                if route.broadcast && !broadcast_ok {
                    return Err(Error::InvalidArg);
                }
                let src = if local.addr.is_unspecified() { route.src } else { local.addr };
                self.ip_send(src, to.addr, proto, data, now)?;
                self.finish(now);
                Ok(data.len())
            }
        }
    }

    /// Receive from a connected socket: Ok(0) at the end of a TCP stream.
    pub fn recv(&mut self, id: usize, buf: &mut [u8], peek: bool, now: u64) -> Result<usize> {
        self.recv_from(id, buf, peek, now).map(|(n, _)| n)
    }

    /// Receive with the sender's address. A datagram longer than `buf` is
    /// truncated (the rest is lost, as with BSD sockets).
    pub fn recv_from(&mut self, id: usize, buf: &mut [u8], peek: bool, now: u64) -> Result<(usize, Endpoint)> {
        self.now = now;
        let s = self.sock(id)?;
        match s.kind {
            Kind::Tcp => {
                let peer = s.remote.unwrap_or_default();
                let shut = s.shut_rd;
                let Some(t) = &mut s.tcb else {
                    return Err(if s.listen.is_some() { Error::InvalidArg } else { Error::NotConnected });
                };
                if t.available() > 0 && !shut {
                    let n = if peek { t.peek(buf) } else { t.recv(buf) };
                    self.finish(now);
                    return Ok((n, peer));
                }
                if let Some(e) = t.error.take() {
                    return Err(e);
                }
                if t.at_eof() || t.is_closed() || shut {
                    return Ok((0, peer));
                }
                if t.is_connecting() {
                    return Err(Error::WouldBlock);
                }
                Err(Error::WouldBlock)
            }
            _ => {
                if let Some(e) = s.error.take() {
                    return Err(e);
                }
                let Some((from, d)) = (if peek { s.dgrams.front().cloned() } else { s.dgrams.pop_front() }) else {
                    return Err(if s.shut_rd { Error::Shutdown } else { Error::WouldBlock });
                };
                if !peek {
                    s.dgram_bytes -= d.len();
                }
                let n = d.len().min(buf.len());
                buf[..n].copy_from_slice(&d[..n]);
                Ok((n, from))
            }
        }
    }

    /// shutdown(): 0 receive, 1 send, 2 both.
    pub fn shutdown(&mut self, id: usize, how: u32, now: u64) -> Result<()> {
        let s = self.sock(id)?;
        if how > 2 {
            return Err(Error::InvalidArg);
        }
        if s.kind == Kind::Tcp && s.tcb.is_none() {
            return Err(Error::NotConnected);
        }
        if how != 1 {
            s.shut_rd = true;
            if let Some(t) = &mut s.tcb {
                t.discard_received();
            }
        }
        if how != 0 {
            s.shut_wr = true;
            if let Some(t) = &mut s.tcb {
                t.close();
            }
        }
        self.changed = true;
        self.finish(now);
        Ok(())
    }

    /// Close a socket. A TCP connection is finished gracefully (unread data
    /// makes it a reset, as BSD does); a listener resets the connections
    /// that were not accepted.
    pub fn close(&mut self, id: usize, now: u64) -> Result<()> {
        self.now = now;
        let s = self.sock(id)?;
        s.user_closed = true;
        let mut segs = Vec::new();
        let listen = s.listen.take();
        if let Some(t) = &mut s.tcb {
            if t.available() > 0 || t.is_connecting() && !t.passive {
                t.abort(&mut segs);
            } else {
                t.close();
            }
            t.orphan(now);
            let (src, dst) = (t.local.addr, t.remote.addr);
            for seg in segs.drain(..) {
                self.tcp_send(src, dst, &seg, now);
            }
        }
        if let Some(l) = listen {
            for c in l.pending.into_iter().chain(l.ready) {
                if let Some(cs) = self.sockets[c].as_mut() {
                    cs.parent = None;
                    cs.user_closed = true;
                    if let Some(t) = &mut cs.tcb {
                        t.abort(&mut segs);
                        t.orphan(now);
                        let (src, dst) = (t.local.addr, t.remote.addr);
                        for seg in segs.drain(..) {
                            self.tcp_send(src, dst, &seg, now);
                        }
                    }
                }
            }
        }
        self.changed = true;
        self.finish(now);
        Ok(())
    }

    /// Close with a reset (SO_LINGER with a zero timeout).
    pub fn abort(&mut self, id: usize, now: u64) -> Result<()> {
        let s = self.sock(id)?;
        if let Some(t) = &mut s.tcb {
            let mut segs = Vec::new();
            t.abort(&mut segs);
            let (src, dst) = (t.local.addr, t.remote.addr);
            for seg in segs {
                self.tcp_send(src, dst, &seg, now);
            }
        }
        self.close(id, now)
    }

    /// Readiness for select().
    pub fn ready(&self, id: usize) -> Result<Ready> {
        let s = self.sock_ref(id)?;
        let mut r = Ready::default();
        match s.kind {
            Kind::Tcp => {
                if let Some(l) = &s.listen {
                    r.read = !l.ready.is_empty();
                } else if let Some(t) = &s.tcb {
                    let dead = t.is_closed() || t.error.is_some();
                    r.read = t.available() > 0 || t.at_eof() || dead || s.shut_rd;
                    r.write = (t.can_send() && t.send_space() > 0) || dead || (s.shut_wr && !t.is_connecting());
                    r.except = t.error.is_some();
                }
            }
            _ => {
                r.read = !s.dgrams.is_empty() || s.error.is_some();
                r.write = true;
                r.except = s.error.is_some();
            }
        }
        Ok(r)
    }

    /// Bytes ready to read (FIONREAD).
    pub fn available(&self, id: usize) -> Result<usize> {
        let s = self.sock_ref(id)?;
        Ok(match (&s.tcb, s.kind) {
            (Some(t), _) => t.available(),
            (None, Kind::Tcp) => 0,
            _ => s.dgrams.front().map_or(0, |d| d.1.len()),
        })
    }

    /// Take the pending error (SO_ERROR).
    pub fn take_error(&mut self, id: usize) -> Result<Option<Error>> {
        let s = self.sock(id)?;
        if let Some(e) = s.error.take() {
            return Ok(Some(e));
        }
        Ok(s.tcb.as_mut().and_then(|t| t.error.take()))
    }

    pub fn local_endpoint(&self, id: usize) -> Result<Endpoint> {
        let s = self.sock_ref(id)?;
        Ok(s.tcb.as_ref().map_or(s.local, |t| t.local))
    }

    pub fn peer_endpoint(&self, id: usize) -> Result<Endpoint> {
        let s = self.sock_ref(id)?;
        match (&s.tcb, s.remote) {
            (Some(t), _) if !t.is_connecting() || t.passive => Ok(t.remote),
            (None, Some(r)) => Ok(r),
            _ => Err(Error::NotConnected),
        }
    }

    pub fn kind(&self, id: usize) -> Result<Kind> {
        Ok(self.sock_ref(id)?.kind)
    }

    pub fn set_reuse_addr(&mut self, id: usize, on: bool) -> Result<()> {
        self.sock(id)?.reuse_addr = on;
        Ok(())
    }

    pub fn set_broadcast(&mut self, id: usize, on: bool) -> Result<()> {
        self.sock(id)?.broadcast = on;
        Ok(())
    }

    /// Buffer sizes (SO_SNDBUF/SO_RCVBUF); applies to new connections.
    pub fn set_buffers(&mut self, id: usize, send: Option<usize>, recv: Option<usize>) -> Result<()> {
        let s = self.sock(id)?;
        if let Some(n) = send {
            s.send_buf = n.clamp(512, 1 << 20);
        }
        if let Some(n) = recv {
            s.recv_buf = n.clamp(512, 65535);
        }
        Ok(())
    }

    pub fn buffers(&self, id: usize) -> Result<(usize, usize)> {
        let s = self.sock_ref(id)?;
        Ok((s.send_buf, s.recv_buf))
    }

    pub fn options(&self, id: usize) -> Result<(bool, bool)> {
        let s = self.sock_ref(id)?;
        Ok((s.reuse_addr, s.broadcast))
    }

    pub fn is_listening(&self, id: usize) -> Result<bool> {
        Ok(self.sock_ref(id)?.listen.is_some())
    }

    /// All sockets (netstat).
    pub fn sockets(&self) -> Vec<SocketInfo> {
        self.sockets_with_ids().into_iter().map(|(_, i)| i).collect()
    }

    /// All sockets with their ids.
    pub fn sockets_with_ids(&self) -> Vec<(usize, SocketInfo)> {
        self.sockets
            .iter()
            .enumerate()
            .filter_map(|(id, s)| Some((id, s.as_ref()?)))
            .filter(|(_, s)| !s.internal)
            .map(|(id, s)| {
                (
                    id,
                    SocketInfo {
                        kind: s.kind,
                        local: s.tcb.as_ref().map_or(s.local, |t| t.local),
                        remote: s.tcb.as_ref().map(|t| t.remote).or(s.remote),
                        state: s.tcb.as_ref().map(|t| t.state),
                        listening: s.listen.is_some(),
                        user_closed: s.user_closed,
                    },
                )
            })
            .collect()
    }

    pub fn socket_count(&self) -> usize {
        self.sockets.iter().flatten().count()
    }

    // ---- Resolver ----

    /// Start resolving `name`; the result comes from
    /// [`Stack::resolve_result`]. Addresses and cached names resolve at once.
    pub fn resolve_start(&mut self, name: &str, now: u64) -> Result<u16> {
        self.now = now;
        let name = name.trim().trim_end_matches('.');
        let lname = name.to_ascii_lowercase();
        let id = loop {
            let id = self.random() as u16;
            if id != 0 && !self.queries.iter().any(|q| q.id == id) {
                break id;
            }
        };
        let mut q = Query { id, name: lname.clone(), tries: 0, sent_at: now, result: None };
        if let Some(a) = Ipv4::parse(name) {
            q.result = Some(Ok(alloc::vec![a]));
        } else if lname == "localhost" {
            q.result = Some(Ok(alloc::vec![Ipv4::LOOPBACK]));
        } else if lname == self.hostname.to_ascii_lowercase() {
            let me = self.ifaces.iter().find(|i| !i.loopback && i.configured()).map_or(Ipv4::LOOPBACK, |i| i.addr);
            q.result = Some(Ok(alloc::vec![me]));
        } else if let Some(c) = self.dns_cache.iter().find(|c| c.name == lname && c.expires > now) {
            q.result = Some(if c.addrs.is_empty() { Err(Error::HostNotFound) } else { Ok(c.addrs.clone()) });
        } else if dns::query(id, &lname).is_none() {
            q.result = Some(Err(Error::HostNotFound));
        } else if self.dns_servers().is_empty() {
            q.result = Some(Err(Error::TryAgain));
        }
        let pending = q.result.is_none();
        self.queries.push(q);
        if pending {
            self.dns_send(id, now);
        }
        Ok(id)
    }

    /// None while the query is running; the result ends the query.
    pub fn resolve_result(&mut self, id: u16) -> Option<Result<Vec<Ipv4>>> {
        let i = self.queries.iter().position(|q| q.id == id)?;
        self.queries[i].result.as_ref()?;
        self.queries.swap_remove(i).result
    }

    pub fn resolve_cancel(&mut self, id: u16) {
        self.queries.retain(|q| q.id != id);
    }

    fn dns_socket(&mut self) -> Result<usize> {
        if let Some(s) = self.dns_socket {
            return Ok(s);
        }
        let id = self.socket(Kind::Udp)?;
        self.sockets[id].as_mut().unwrap().internal = true;
        self.bind(id, Endpoint::default())?;
        self.dns_socket = Some(id);
        Ok(id)
    }

    fn dns_send(&mut self, id: u16, now: u64) {
        let servers = self.dns_servers();
        let Some(q) = self.queries.iter_mut().find(|q| q.id == id) else { return };
        if servers.is_empty() {
            q.result = Some(Err(Error::TryAgain));
            return;
        }
        let server = servers[q.tries as usize % servers.len()];
        q.tries += 1;
        q.sent_at = now;
        let msg = dns::query(id, &q.name).unwrap_or_default();
        if let Ok(s) = self.dns_socket() {
            let _ = self.send_to(s, &msg, Endpoint::new(server, dns::PORT), now);
        }
    }

    fn dns_poll(&mut self, now: u64) {
        // Answers.
        if let Some(sid) = self.dns_socket {
            let mut buf = alloc::vec![0u8; 1500];
            while let Ok((n, from)) = self.recv_from(sid, &mut buf, false, now) {
                if from.port != dns::PORT {
                    continue;
                }
                let m = &buf[..n];
                let Some(id) = (n >= 2).then(|| crate::wire::be16(m, 0)) else { continue };
                let Some(q) = self.queries.iter_mut().find(|q| q.id == id && q.result.is_none()) else { continue };
                let Some(a) = dns::parse_response(m, id, &q.name) else { continue };
                let result = match a.rcode {
                    dns::NOERROR if !a.addrs.is_empty() => Ok(a.addrs.iter().map(|x| x.0).collect::<Vec<_>>()),
                    dns::NOERROR => Err(Error::NoData),
                    dns::NXDOMAIN => Err(Error::HostNotFound),
                    _ => Err(Error::TryAgain),
                };
                let ttl = a.addrs.iter().map(|x| x.1).min().unwrap_or(60).clamp(5, 86400) as u64;
                let name = q.name.clone();
                q.result = Some(result.clone());
                self.changed = true;
                if result.is_ok() || a.rcode == dns::NXDOMAIN {
                    if self.dns_cache.len() >= DNS_CACHE {
                        self.dns_cache.remove(0);
                    }
                    self.dns_cache.retain(|c| c.name != name);
                    self.dns_cache.push(CacheEntry { name, addrs: result.unwrap_or_default(), expires: now + ttl * 1000 });
                }
            }
        }
        // Timeouts.
        let due: Vec<u16> = self.queries.iter().filter(|q| q.result.is_none() && now >= q.sent_at + DNS_TIMEOUT).map(|q| q.id).collect();
        for id in due {
            let q = self.queries.iter_mut().find(|q| q.id == id).unwrap();
            if q.tries >= DNS_TRIES {
                q.result = Some(Err(Error::TryAgain));
                self.changed = true;
            } else {
                self.dns_send(id, now);
            }
        }
    }

    /// A one-line description of an interface.
    pub fn describe(&self, iface: usize) -> String {
        let i = &self.ifaces[iface];
        alloc::format!(
            "{} {} {}/{} link {}",
            i.name,
            MacFmt(i.mac),
            i.addr,
            i.mask.prefix_len(),
            if i.link { "up" } else { "down" }
        )
    }
}

#[cfg(test)]
mod tests;
