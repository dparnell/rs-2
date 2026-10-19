//! DHCP client (RFC 2131, options RFC 2132).
//!
//! INIT -> SELECTING (DISCOVER, waiting for an OFFER) -> REQUESTING
//! (REQUEST, waiting for the ACK) -> BOUND; at T1 RENEWING (REQUEST to
//! the server), at T2 REBINDING (REQUEST to everyone), at the end of the
//! lease the address is given up and the client starts over. Messages
//! carry the broadcast flag, so replies reach an interface that has no
//! address yet. The client only produces and consumes DHCP payloads; the
//! stack wraps them in UDP/IP and decides where they go.

use alloc::string::String;
use alloc::vec::Vec;

use crate::wire::{be32, ip_at};
use crate::{Ipv4, Mac};

pub const SERVER_PORT: u16 = 67;
pub const CLIENT_PORT: u16 = 68;

const MAGIC: [u8; 4] = [99, 130, 83, 99];
const FIXED: usize = 236;

// Message types (option 53).
pub const DISCOVER: u8 = 1;
pub const OFFER: u8 = 2;
pub const REQUEST: u8 = 3;
pub const DECLINE: u8 = 4;
pub const ACK: u8 = 5;
pub const NAK: u8 = 6;
pub const RELEASE: u8 = 7;

// Options.
const OPT_PAD: u8 = 0;
const OPT_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_HOSTNAME: u8 = 12;
const OPT_DOMAIN: u8 = 15;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_PARAMS: u8 = 55;
const OPT_MAX_SIZE: u8 = 57;
const OPT_T1: u8 = 58;
const OPT_T2: u8 = 59;
const OPT_CLIENT_ID: u8 = 61;
const OPT_END: u8 = 255;

/// Retransmission interval of the first try (doubled up to 64 s, RFC
/// 2131 4.1, without the random jitter).
const FIRST_RETRY: u64 = 4000;
const MAX_RETRY: u64 = 64_000;
/// REQUESTs sent before going back to INIT.
const REQUEST_TRIES: u32 = 4;

/// A configuration handed out by a server.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Lease {
    pub addr: Ipv4,
    pub mask: Ipv4,
    pub router: Option<Ipv4>,
    pub dns: Vec<Ipv4>,
    pub domain: String,
    pub server: Ipv4,
    /// Seconds; u32::MAX is infinite.
    pub lease_time: u32,
    pub t1: u32,
    pub t2: u32,
}

/// A parsed DHCP message (only what the client needs).
#[derive(Clone, Debug, Default)]
pub struct Message {
    pub op: u8,
    pub xid: u32,
    pub yiaddr: Ipv4,
    pub chaddr: Mac,
    pub msg_type: u8,
    pub mask: Option<Ipv4>,
    pub router: Option<Ipv4>,
    pub dns: Vec<Ipv4>,
    pub domain: String,
    pub server_id: Option<Ipv4>,
    pub lease_time: Option<u32>,
    pub t1: Option<u32>,
    pub t2: Option<u32>,
}

pub fn parse(b: &[u8]) -> Option<Message> {
    if b.len() < FIXED + 4 || b[1] != 1 || b[2] != 6 || b[FIXED..FIXED + 4] != MAGIC {
        return None;
    }
    let mut m = Message { op: b[0], xid: be32(b, 4), yiaddr: ip_at(b, 16), ..Default::default() };
    m.chaddr.copy_from_slice(&b[28..34]);
    let mut i = FIXED + 4;
    while i < b.len() {
        let code = b[i];
        if code == OPT_PAD {
            i += 1;
            continue;
        }
        if code == OPT_END || i + 1 >= b.len() {
            break;
        }
        let len = b[i + 1] as usize;
        let Some(v) = b.get(i + 2..i + 2 + len) else { break };
        let ips = || v.chunks_exact(4).map(|c| Ipv4([c[0], c[1], c[2], c[3]])).collect::<Vec<_>>();
        match code {
            OPT_MSG_TYPE if len == 1 => m.msg_type = v[0],
            OPT_MASK if len == 4 => m.mask = Some(ip_at(v, 0)),
            OPT_ROUTER if len >= 4 => m.router = Some(ip_at(v, 0)),
            OPT_DNS => m.dns = ips(),
            OPT_DOMAIN => m.domain = v.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect(),
            OPT_SERVER_ID if len == 4 => m.server_id = Some(ip_at(v, 0)),
            OPT_LEASE if len == 4 => m.lease_time = Some(be32(v, 0)),
            OPT_T1 if len == 4 => m.t1 = Some(be32(v, 0)),
            OPT_T2 if len == 4 => m.t2 = Some(be32(v, 0)),
            _ => {}
        }
        i += 2 + len;
    }
    Some(m)
}

/// Build a client message.
fn build(msg_type: u8, xid: u32, secs: u16, mac: Mac, ciaddr: Ipv4, hostname: &str, requested: Option<Ipv4>, server: Option<Ipv4>) -> Vec<u8> {
    let mut b = alloc::vec![0u8; FIXED];
    b[0] = 1; // BOOTREQUEST
    b[1] = 1; // Ethernet
    b[2] = 6;
    b[4..8].copy_from_slice(&xid.to_be_bytes());
    b[8..10].copy_from_slice(&secs.to_be_bytes());
    // Broadcast flag, unless renewing from a configured address.
    if ciaddr.is_unspecified() {
        b[10] = 0x80;
    }
    b[12..16].copy_from_slice(&ciaddr.0);
    b[28..34].copy_from_slice(&mac);
    b.extend_from_slice(&MAGIC);
    b.extend_from_slice(&[OPT_MSG_TYPE, 1, msg_type]);
    b.extend_from_slice(&[OPT_CLIENT_ID, 7, 1]);
    b.extend_from_slice(&mac);
    if let Some(ip) = requested {
        b.extend_from_slice(&[OPT_REQUESTED_IP, 4]);
        b.extend_from_slice(&ip.0);
    }
    if let Some(ip) = server {
        b.extend_from_slice(&[OPT_SERVER_ID, 4]);
        b.extend_from_slice(&ip.0);
    }
    if !hostname.is_empty() && hostname.len() < 64 {
        b.extend_from_slice(&[OPT_HOSTNAME, hostname.len() as u8]);
        b.extend_from_slice(hostname.as_bytes());
    }
    if msg_type != RELEASE {
        b.extend_from_slice(&[OPT_PARAMS, 7, OPT_MASK, OPT_ROUTER, OPT_DNS, OPT_DOMAIN, OPT_LEASE, OPT_T1, OPT_T2]);
        b.extend_from_slice(&[OPT_MAX_SIZE, 2]);
        b.extend_from_slice(&1500u16.to_be_bytes());
    }
    b.push(OPT_END);
    // Some servers want at least the 300 bytes of a BOOTP message.
    if b.len() < 300 {
        b.resize(300, 0);
    }
    b
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Init,
    Selecting,
    Requesting,
    Bound,
    Renewing,
    Rebinding,
}

/// What the client wants done.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Broadcast this payload (from 0.0.0.0 unless the client is renewing
    /// or rebinding with an address).
    Broadcast(Vec<u8>),
    /// Send it to the server (renewing).
    Unicast(Ipv4, Vec<u8>),
    /// Configure the interface.
    Bound(Lease),
    /// The lease is gone: remove the address.
    Lost,
}

pub struct Client {
    pub state: State,
    mac: Mac,
    hostname: String,
    xid: u32,
    /// Start of the current exchange (for the secs field).
    started: u64,
    retry_at: u64,
    interval: u64,
    tries: u32,
    offer: Option<Lease>,
    pub lease: Option<Lease>,
    /// When the lease was obtained.
    bound_at: u64,
}

impl Client {
    pub fn new(mac: Mac, hostname: &str, seed: u32) -> Client {
        Client {
            state: State::Init,
            mac,
            hostname: String::from(hostname),
            xid: seed,
            started: 0,
            retry_at: 0,
            interval: FIRST_RETRY,
            tries: 0,
            offer: None,
            lease: None,
            bound_at: 0,
        }
    }

    fn secs(&self, now: u64) -> u16 {
        ((now.saturating_sub(self.started)) / 1000).min(0xffff) as u16
    }

    fn next_xid(&mut self) {
        // xorshift: transaction ids that differ between exchanges.
        let mut x = self.xid;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.xid = x;
    }

    fn discover(&mut self, now: u64, out: &mut Vec<Action>) {
        out.push(Action::Broadcast(build(DISCOVER, self.xid, self.secs(now), self.mac, Ipv4::UNSPECIFIED, &self.hostname, None, None)));
    }

    fn request(&mut self, now: u64, out: &mut Vec<Action>) {
        match self.state {
            State::Requesting => {
                let o = self.offer.as_ref().unwrap();
                let (addr, server) = (o.addr, o.server);
                out.push(Action::Broadcast(build(REQUEST, self.xid, self.secs(now), self.mac, Ipv4::UNSPECIFIED, &self.hostname, Some(addr), Some(server))));
            }
            State::Renewing | State::Rebinding => {
                let l = self.lease.as_ref().unwrap();
                let (addr, server) = (l.addr, l.server);
                let m = build(REQUEST, self.xid, self.secs(now), self.mac, addr, &self.hostname, None, None);
                out.push(if self.state == State::Renewing { Action::Unicast(server, m) } else { Action::Broadcast(m) });
            }
            _ => {}
        }
    }

    fn restart(&mut self, now: u64, out: &mut Vec<Action>) {
        self.state = State::Selecting;
        self.next_xid();
        self.started = now;
        self.interval = FIRST_RETRY;
        self.tries = 1;
        self.retry_at = now + self.interval;
        self.offer = None;
        self.discover(now, out);
    }

    /// Timers. Call regularly.
    pub fn poll(&mut self, now: u64, out: &mut Vec<Action>) {
        match self.state {
            State::Init => self.restart(now, out),
            State::Selecting if now >= self.retry_at => {
                self.interval = (self.interval * 2).min(MAX_RETRY);
                self.tries += 1;
                self.retry_at = now + self.interval;
                self.discover(now, out);
            }
            State::Requesting if now >= self.retry_at => {
                if self.tries >= REQUEST_TRIES {
                    self.restart(now, out);
                    return;
                }
                self.interval = (self.interval * 2).min(MAX_RETRY);
                self.tries += 1;
                self.retry_at = now + self.interval;
                self.request(now, out);
            }
            State::Bound | State::Renewing | State::Rebinding => {
                let Some(l) = &self.lease else { return };
                if l.lease_time == u32::MAX {
                    return;
                }
                let at = |s: u32| self.bound_at + s as u64 * 1000;
                let (t1, t2, end) = (at(l.t1), at(l.t2), at(l.lease_time));
                if now >= end {
                    self.lease = None;
                    out.push(Action::Lost);
                    self.restart(now, out);
                } else if now >= t2 && self.state != State::Rebinding {
                    self.state = State::Rebinding;
                    self.next_xid();
                    self.retry_at = now + ((end - now) / 2).max(60_000);
                    self.request(now, out);
                } else if now >= t1 && self.state == State::Bound {
                    self.state = State::Renewing;
                    self.next_xid();
                    self.started = now;
                    self.retry_at = now + ((t2 - now) / 2).max(60_000);
                    self.request(now, out);
                } else if self.state != State::Bound && now >= self.retry_at {
                    // RFC 2131 4.4.5: retry at half the remaining time.
                    let limit = if self.state == State::Renewing { t2 } else { end };
                    self.retry_at = now + ((limit.saturating_sub(now)) / 2).max(60_000);
                    self.request(now, out);
                }
            }
            _ => {}
        }
    }

    /// A message arrived on the client port.
    pub fn input(&mut self, payload: &[u8], now: u64, out: &mut Vec<Action>) {
        let Some(m) = parse(payload) else { return };
        if m.op != 2 || m.xid != self.xid || m.chaddr != self.mac {
            return;
        }
        match (self.state, m.msg_type) {
            (State::Selecting, OFFER) => {
                let Some(server) = m.server_id else { return };
                self.offer = Some(lease_of(&m, server));
                self.state = State::Requesting;
                self.interval = FIRST_RETRY;
                self.tries = 1;
                self.retry_at = now + self.interval;
                self.request(now, out);
            }
            (State::Requesting | State::Renewing | State::Rebinding, ACK) => {
                let server = m.server_id.or(self.offer.as_ref().map(|o| o.server)).or(self.lease.as_ref().map(|l| l.server));
                let lease = lease_of(&m, server.unwrap_or(Ipv4::UNSPECIFIED));
                self.state = State::Bound;
                self.bound_at = now;
                self.lease = Some(lease.clone());
                out.push(Action::Bound(lease));
            }
            (State::Requesting | State::Renewing | State::Rebinding, NAK) => {
                if self.lease.take().is_some() {
                    out.push(Action::Lost);
                }
                self.restart(now, out);
            }
            _ => {}
        }
    }
}

fn lease_of(m: &Message, server: Ipv4) -> Lease {
    let lease_time = m.lease_time.unwrap_or(3600);
    let infinite = lease_time == u32::MAX;
    let mask = m.mask.unwrap_or_else(|| {
        // Classful default.
        match m.yiaddr.0[0] {
            0..=127 => Ipv4::mask(8),
            128..=191 => Ipv4::mask(16),
            _ => Ipv4::mask(24),
        }
    });
    Lease {
        addr: m.yiaddr,
        mask,
        router: m.router,
        dns: m.dns.clone(),
        domain: m.domain.clone(),
        server,
        lease_time,
        t1: if infinite { u32::MAX } else { m.t1.unwrap_or(lease_time / 2) },
        t2: if infinite { u32::MAX } else { m.t2.unwrap_or((lease_time as u64 * 7 / 8) as u32) },
    }
}

/// Build a server reply (for tests and the loopback test server).
pub fn server_reply(msg_type: u8, xid: u32, mac: Mac, yiaddr: Ipv4, server: Ipv4, lease: &Lease) -> Vec<u8> {
    let mut b = alloc::vec![0u8; FIXED];
    b[0] = 2;
    b[1] = 1;
    b[2] = 6;
    b[4..8].copy_from_slice(&xid.to_be_bytes());
    b[16..20].copy_from_slice(&yiaddr.0);
    b[28..34].copy_from_slice(&mac);
    b.extend_from_slice(&MAGIC);
    b.extend_from_slice(&[OPT_MSG_TYPE, 1, msg_type]);
    b.extend_from_slice(&[OPT_SERVER_ID, 4]);
    b.extend_from_slice(&server.0);
    b.extend_from_slice(&[OPT_MASK, 4]);
    b.extend_from_slice(&lease.mask.0);
    if let Some(r) = lease.router {
        b.extend_from_slice(&[OPT_ROUTER, 4]);
        b.extend_from_slice(&r.0);
    }
    if !lease.dns.is_empty() {
        b.push(OPT_DNS);
        b.push((lease.dns.len() * 4) as u8);
        for d in &lease.dns {
            b.extend_from_slice(&d.0);
        }
    }
    b.extend_from_slice(&[OPT_LEASE, 4]);
    b.extend_from_slice(&lease.lease_time.to_be_bytes());
    b.push(OPT_END);
    b
}

/// Transaction id and client MAC of a client message (tests).
pub fn request_ids(b: &[u8]) -> Option<(u32, Mac, u8)> {
    let m = parse(b)?;
    Some((m.xid, m.chaddr, m.msg_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: Mac = [0x52, 0x54, 0, 0x12, 0x34, 0x56];

    fn lease() -> Lease {
        Lease {
            addr: Ipv4::new(10, 0, 2, 15),
            mask: Ipv4::mask(24),
            router: Some(Ipv4::new(10, 0, 2, 2)),
            dns: alloc::vec![Ipv4::new(10, 0, 2, 3)],
            lease_time: 86400,
            ..Default::default()
        }
    }

    fn payload(a: &Action) -> &[u8] {
        match a {
            Action::Broadcast(p) | Action::Unicast(_, p) => p,
            _ => panic!("not a message"),
        }
    }

    #[test]
    fn full_exchange_and_renewal() {
        let server = Ipv4::new(10, 0, 2, 2);
        let mut c = Client::new(MAC, "rs2", 0x1234_5678);
        let mut out = Vec::new();
        c.poll(0, &mut out);
        let (xid, mac, t) = request_ids(payload(&out[0])).unwrap();
        assert_eq!((mac, t, c.state), (MAC, DISCOVER, State::Selecting));
        out.clear();
        // A reply to someone else's transaction is ignored.
        c.input(&server_reply(OFFER, xid ^ 1, MAC, lease().addr, server, &lease()), 10, &mut out);
        assert!(out.is_empty());
        c.input(&server_reply(OFFER, xid, MAC, lease().addr, server, &lease()), 10, &mut out);
        assert_eq!(c.state, State::Requesting);
        let p = parse(payload(&out[0])).unwrap();
        assert_eq!(p.msg_type, REQUEST);
        out.clear();
        c.input(&server_reply(ACK, xid, MAC, lease().addr, server, &lease()), 20, &mut out);
        let Action::Bound(l) = &out[0] else { panic!() };
        assert_eq!(l.addr, Ipv4::new(10, 0, 2, 15));
        assert_eq!(l.router, Some(server));
        assert_eq!(l.dns, alloc::vec![Ipv4::new(10, 0, 2, 3)]);
        assert_eq!((l.t1, l.t2), (43200, 75600));
        out.clear();
        // T1: unicast renewal to the server.
        c.poll(20 + 43200 * 1000, &mut out);
        assert_eq!(c.state, State::Renewing);
        assert!(matches!(out[0], Action::Unicast(s, _) if s == server));
        let xid2 = parse(payload(&out[0])).unwrap().xid;
        out.clear();
        c.input(&server_reply(ACK, xid2, MAC, lease().addr, server, &lease()), 20 + 43201 * 1000, &mut out);
        assert_eq!(c.state, State::Bound);
    }

    #[test]
    fn retries_and_nak() {
        let mut c = Client::new(MAC, "", 99);
        let mut out = Vec::new();
        c.poll(0, &mut out);
        c.poll(3999, &mut out);
        assert_eq!(out.len(), 1);
        c.poll(4000, &mut out);
        assert_eq!(out.len(), 2);
        let xid = parse(payload(&out[1])).unwrap().xid;
        out.clear();
        let server = Ipv4::new(192, 168, 1, 1);
        c.input(&server_reply(OFFER, xid, MAC, Ipv4::new(192, 168, 1, 50), server, &lease()), 5000, &mut out);
        out.clear();
        c.input(&server_reply(NAK, xid, MAC, Ipv4::UNSPECIFIED, server, &lease()), 5100, &mut out);
        assert_eq!(c.state, State::Selecting);
        assert_eq!(parse(payload(&out[0])).unwrap().msg_type, DISCOVER);
    }
}
