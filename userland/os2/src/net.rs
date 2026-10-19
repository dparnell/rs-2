//! rs2's kernel socket interface (RS2API ordinals 200-217; see
//! kernel/src/api/sockets.rs). Calls return 0 or a BSD errno value.

#![allow(non_snake_case, clippy::missing_safety_doc)]

use alloc::string::String;

pub const AF_INET: u32 = 2;
pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;
pub const SOCK_RAW: u32 = 3;
pub const IPPROTO_ICMP: u32 = 1;
pub const IPPROTO_TCP: u32 = 6;
pub const IPPROTO_UDP: u32 = 17;

pub const MSG_PEEK: u32 = 2;
pub const MSG_DONTWAIT: u32 = 0x80;

pub const SOL_SOCKET: u32 = 0xffff;
pub const SO_REUSEADDR: u32 = 0x0004;
pub const SO_BROADCAST: u32 = 0x0020;
pub const SO_RCVTIMEO: u32 = 0x1006;
pub const SO_ERROR: u32 = 0x1007;
pub const SO_TYPE: u32 = 0x1008;
pub const FIONBIO: u32 = 0x8004_667e;
pub const FIONREAD: u32 = 0x4004_667f;

pub const EINTR: u32 = 4;
pub const EWOULDBLOCK: u32 = 35;
pub const EINPROGRESS: u32 = 36;
pub const ECONNREFUSED: u32 = 61;

/// IFF flags of [`IfInfo::flags`].
pub const IF_LINK: u32 = 1;
pub const IF_UP: u32 = 2;
pub const IF_LOOPBACK: u32 = 4;
pub const IF_DHCP: u32 = 8;
pub const IF_BOUND: u32 = 16;

/// `struct sockaddr_in` (4.3BSD layout).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SockAddrIn {
    pub family: u16,
    /// Network byte order.
    pub port: u16,
    pub addr: [u8; 4],
    pub zero: [u8; 8],
}

impl SockAddrIn {
    pub fn new(addr: [u8; 4], port: u16) -> SockAddrIn {
        SockAddrIn { family: AF_INET as u16, port: port.to_be(), addr, zero: [0; 8] }
    }

    pub fn port(&self) -> u16 {
        u16::from_be(self.port)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfInfo {
    pub name: [u8; 16],
    pub mac: [u8; 6],
    pub pad: [u8; 2],
    pub addr: [u8; 4],
    pub mask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [[u8; 4]; 4],
    pub ndns: u32,
    pub mtu: u32,
    pub flags: u32,
    pub rx_packets: u32,
    pub tx_packets: u32,
    pub rx_bytes: u32,
    pub tx_bytes: u32,
    pub rx_dropped: u32,
}

impl Default for IfInfo {
    fn default() -> IfInfo {
        unsafe { core::mem::zeroed() }
    }
}

impl IfInfo {
    pub fn name(&self) -> &str {
        let n = self.name.iter().position(|&c| c == 0).unwrap_or(16);
        core::str::from_utf8(&self.name[..n]).unwrap_or("?")
    }
}

unsafe extern "C" {
    #[link_name = "RS2API.200"]
    pub fn Rs2NetQueryIface(index: u32, info: *mut IfInfo, cb: u32) -> u32;
    #[link_name = "RS2API.201"]
    pub fn Rs2SockCreate(domain: u32, typ: u32, proto: u32, ps: *mut u32) -> u32;
    #[link_name = "RS2API.202"]
    pub fn Rs2SockBind(s: u32, addr: *const SockAddrIn, cb: u32) -> u32;
    #[link_name = "RS2API.203"]
    pub fn Rs2SockListen(s: u32, backlog: u32) -> u32;
    #[link_name = "RS2API.204"]
    pub fn Rs2SockAccept(s: u32, peer: *mut SockAddrIn, ps: *mut u32) -> u32;
    #[link_name = "RS2API.205"]
    pub fn Rs2SockConnect(s: u32, addr: *const SockAddrIn, cb: u32) -> u32;
    #[link_name = "RS2API.206"]
    pub fn Rs2SockSend(s: u32, buf: *const u8, cb: u32, flags: u32, sent: *mut u32) -> u32;
    #[link_name = "RS2API.207"]
    pub fn Rs2SockRecv(s: u32, buf: *mut u8, cb: u32, flags: u32, got: *mut u32) -> u32;
    #[link_name = "RS2API.208"]
    pub fn Rs2SockSendTo(s: u32, buf: *const u8, cb: u32, flags: u32, to: *const SockAddrIn, sent: *mut u32) -> u32;
    #[link_name = "RS2API.209"]
    pub fn Rs2SockRecvFrom(s: u32, buf: *mut u8, cb: u32, flags: u32, from: *mut SockAddrIn, got: *mut u32) -> u32;
    #[link_name = "RS2API.210"]
    pub fn Rs2SockShutdown(s: u32, how: u32) -> u32;
    #[link_name = "RS2API.211"]
    pub fn Rs2SockClose(s: u32) -> u32;
    #[link_name = "RS2API.212"]
    pub fn Rs2SockSelect(socks: *mut u32, nread: u32, nwrite: u32, nexcept: u32, ms: i32, nready: *mut u32) -> u32;
    #[link_name = "RS2API.213"]
    pub fn Rs2SockIoctl(s: u32, cmd: u32, arg: *mut u32) -> u32;
    #[link_name = "RS2API.214"]
    pub fn Rs2SockSetOpt(s: u32, level: u32, opt: u32, val: *const u8, cb: u32) -> u32;
    #[link_name = "RS2API.215"]
    pub fn Rs2SockGetOpt(s: u32, level: u32, opt: u32, val: *mut u8, cb: *mut u32) -> u32;
    #[link_name = "RS2API.216"]
    pub fn Rs2SockGetName(s: u32, peer: u32, addr: *mut SockAddrIn) -> u32;
    #[link_name = "RS2API.217"]
    pub fn Rs2NetResolve(name: *const u8, addrs: *mut [u8; 4], max: u32, count: *mut u32) -> u32;
}

/// Dotted-quad text of an address.
pub fn ip(a: [u8; 4]) -> String {
    alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

pub fn parse_ip(s: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = s.split('.');
    for o in out.iter_mut() {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 {
            return None;
        }
        let v: u32 = p.parse().ok()?;
        if v > 255 {
            return None;
        }
        *o = v as u8;
    }
    parts.next().is_none().then_some(out)
}

/// Resolve a name (or dotted quad); Err is the h_errno value.
pub fn resolve(name: &str) -> Result<alloc::vec::Vec<[u8; 4]>, u32> {
    let mut z: alloc::vec::Vec<u8> = name.bytes().collect();
    z.push(0);
    let mut addrs = [[0u8; 4]; 8];
    let mut n = 0;
    let rc = unsafe { Rs2NetResolve(z.as_ptr(), addrs.as_mut_ptr(), 8, &mut n) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(addrs[..n as usize].to_vec())
}

/// Text for an errno value.
pub fn strerror(e: u32) -> &'static str {
    match e {
        0 => "no error",
        4 => "interrupted",
        9 => "bad socket",
        14 => "bad address",
        22 => "invalid argument",
        32 => "broken pipe",
        35 => "operation would block",
        36 => "operation now in progress",
        37 => "operation already in progress",
        38 => "not a socket",
        40 => "message too long",
        43 => "protocol not supported",
        45 => "operation not supported",
        47 => "address family not supported",
        48 => "address already in use",
        49 => "cannot assign requested address",
        50 => "network is down",
        51 => "network is unreachable",
        54 => "connection reset by peer",
        56 => "socket is already connected",
        57 => "socket is not connected",
        60 => "connection timed out",
        61 => "connection refused",
        65 => "no route to host",
        _ => "error",
    }
}

/// Milliseconds since boot.
pub fn ms() -> u32 {
    let mut v = 0u32;
    unsafe { crate::DosQuerySysInfo(14, 14, &mut v, 4) };
    v
}

/// The Internet checksum.
pub fn checksum(b: &[u8]) -> u16 {
    let mut s: u32 = 0;
    for c in b.chunks(2) {
        s += (c[0] as u32) << 8 | c.get(1).copied().unwrap_or(0) as u32;
    }
    while s > 0xffff {
        s = (s & 0xffff) + (s >> 16);
    }
    !(s as u16)
}

/// An echo reply (ping) as received.
pub struct EchoReply {
    pub from: [u8; 4],
    pub bytes: usize,
    pub ttl: u8,
    pub rtt_ms: u32,
}

/// Send one ICMP echo request on raw ICMP socket `s` and wait up to
/// `timeout_ms` for the matching reply.
pub fn ping_once(s: u32, dst: [u8; 4], ident: u16, seq: u16, size: usize, timeout_ms: u32) -> Result<EchoReply, u32> {
    let mut m = alloc::vec![0u8; 8 + size];
    m[0] = 8;
    m[4..6].copy_from_slice(&ident.to_be_bytes());
    m[6..8].copy_from_slice(&seq.to_be_bytes());
    for (i, b) in m[8..].iter_mut().enumerate() {
        *b = i as u8;
    }
    let c = checksum(&m);
    m[2..4].copy_from_slice(&c.to_be_bytes());
    let to = SockAddrIn::new(dst, 0);
    let start = ms();
    let mut sent = 0;
    let rc = unsafe { Rs2SockSendTo(s, m.as_ptr(), m.len() as u32, 0, &to, &mut sent) };
    if rc != 0 {
        return Err(rc);
    }
    let mut buf = alloc::vec![0u8; 65536];
    loop {
        let elapsed = ms().wrapping_sub(start);
        if elapsed >= timeout_ms {
            return Err(EWOULDBLOCK);
        }
        let mut socks = [s];
        let mut n = 0;
        let rc = unsafe { Rs2SockSelect(socks.as_mut_ptr(), 1, 0, 0, (timeout_ms - elapsed) as i32, &mut n) };
        if rc != 0 {
            return Err(rc);
        }
        if n == 0 {
            continue;
        }
        let mut from = SockAddrIn::default();
        let mut got = 0;
        let rc = unsafe { Rs2SockRecvFrom(s, buf.as_mut_ptr(), buf.len() as u32, 0, &mut from, &mut got) };
        if rc != 0 {
            return Err(rc);
        }
        let d = &buf[..got as usize];
        if d.len() < 20 {
            continue;
        }
        let ihl = (d[0] & 0xf) as usize * 4;
        if d.len() < ihl + 8 {
            continue;
        }
        let icmp = &d[ihl..];
        let id = u16::from_be_bytes([icmp[4], icmp[5]]);
        let sq = u16::from_be_bytes([icmp[6], icmp[7]]);
        if icmp[0] == 0 && id == ident && sq == seq && from.addr == dst {
            return Ok(EchoReply { from: from.addr, bytes: icmp.len() - 8, ttl: d[8], rtt_ms: ms().wrapping_sub(start) });
        }
    }
}

/// Text for an h_errno value.
pub fn hstrerror(h: u32) -> &'static str {
    match h {
        1 => "host not found",
        2 => "try again (no answer from the name server)",
        4 => "no address for the name",
        _ => "resolver error",
    }
}
