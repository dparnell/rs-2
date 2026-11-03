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
    #[link_name = "RS2API.218"]
    pub fn Rs2SockCancel(s: u32) -> u32;
    #[link_name = "RS2API.219"]
    pub fn Rs2NetQuerySockets(info: *mut SockInfo, max: u32, count: *mut u32) -> u32;
    #[link_name = "RS2API.220"]
    pub fn Rs2SockTake(s: u32) -> u32;
    #[link_name = "RS2API.221"]
    pub fn Rs2SockRelease(s: u32) -> u32;
    #[link_name = "RS2API.240"]
    pub fn Rs2NetControl(func: u32, input: *const u8, cb_in: u32, out: *mut u8, cb_out: *mut u32) -> u32;
}

// Rs2NetControl functions (kernel/src/api/sockets.rs).
pub const NC_ROUTES: u32 = 1;
pub const NC_ROUTE_ADD: u32 = 2;
pub const NC_ROUTE_DELETE: u32 = 3;
pub const NC_ROUTE_FLUSH: u32 = 4;
pub const NC_ARP_LIST: u32 = 5;
pub const NC_ARP_SET: u32 = 6;
pub const NC_ARP_DELETE: u32 = 7;
pub const NC_IF_SET: u32 = 8;
pub const NC_IF_ADDRS: u32 = 9;
pub const NC_STATS: u32 = 10;
pub const NC_SYSCTL: u32 = 11;

// NC_IF_SET operations.
pub const IFOP_ADDRESS: u32 = 1;
pub const IFOP_NETMASK: u32 = 2;
pub const IFOP_UP: u32 = 3;
pub const IFOP_DOWN: u32 = 4;
pub const IFOP_ALIAS_ADD: u32 = 5;
pub const IFOP_ALIAS_DELETE: u32 = 6;
pub const IFOP_MTU: u32 = 7;
pub const IFOP_METRIC: u32 = 8;
pub const IFOP_DHCP_RENEW: u32 = 9;
pub const IFOP_DHCP_RELEASE: u32 = 10;

// NC_SYSCTL variables.
pub const NV_FORWARDING: u32 = 1;
pub const NV_KEEPIDLE: u32 = 2;
pub const NV_KEEPINTVL: u32 = 3;
pub const NV_KEEPCNT: u32 = 4;
pub const NV_SENDSPACE: u32 = 5;
pub const NV_RECVSPACE: u32 = 6;

/// Rs2NetControl with an output buffer that grows to the answer.
pub fn net_control(func: u32, input: &[u8]) -> Result<alloc::vec::Vec<u8>, u32> {
    let mut room = 1024u32;
    loop {
        let mut out = alloc::vec![0u8; room as usize];
        let mut n = room;
        let rc = unsafe { Rs2NetControl(func, input.as_ptr(), input.len() as u32, out.as_mut_ptr(), &mut n) };
        if rc != 0 {
            return Err(rc);
        }
        if n <= room {
            out.truncate(n as usize);
            return Ok(out);
        }
        room = n + 256;
    }
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn q4(b: &[u8], o: usize) -> [u8; 4] {
    [b[o], b[o + 1], b[o + 2], b[o + 3]]
}

fn words(v: &[u32]) -> alloc::vec::Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The OS/2 name of the interface with kernel index `index` ("lo",
/// "lan0", ...).
pub fn iface_name(index: u32) -> String {
    let ifs = interfaces();
    match ifs.get(index as usize) {
        Some(i) if i.flags & IF_LOOPBACK != 0 => String::from("lo"),
        Some(_) => {
            let n = ifs[..index as usize].iter().filter(|i| i.flags & IF_LOOPBACK == 0).count();
            alloc::format!("lan{n}")
        }
        None => alloc::format!("if{index}"),
    }
}

/// The kernel index of an interface named as OS/2 names it ("lan0",
/// "lo"), or as the kernel does ("eth0").
pub fn iface_index(name: &str) -> Option<u32> {
    let ifs = interfaces();
    let l = name.to_ascii_lowercase();
    if l == "lo" || l == "lo0" || l == "loopback" {
        return ifs.iter().position(|i| i.flags & IF_LOOPBACK != 0).map(|p| p as u32);
    }
    if let Some(n) = l.strip_prefix("lan").and_then(|x| x.parse::<usize>().ok()) {
        let mut k = 0;
        for (p, i) in ifs.iter().enumerate() {
            if i.flags & IF_LOOPBACK == 0 {
                if k == n {
                    return Some(p as u32);
                }
                k += 1;
            }
        }
        return None;
    }
    ifs.iter().position(|i| i.name().eq_ignore_ascii_case(name)).map(|p| p as u32)
}

/// One socket of the stack (Rs2NetQuerySockets).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SockInfo {
    /// 1 TCP, 2 UDP, 3 raw.
    pub kind: u8,
    pub protocol: u8,
    /// TCP state, 1 (CLOSED) to 11 (TIME_WAIT); 0 for other sockets.
    pub state: u8,
    /// Bit 0 listening, 1 closed by its owner.
    pub flags: u8,
    pub laddr: [u8; 4],
    pub lport: u16,
    pub rport: u16,
    pub raddr: [u8; 4],
    pub pid: u32,
    pub socket: u32,
}

/// Names of the TCP states of [`SockInfo::state`].
pub fn tcp_state_name(s: u8) -> &'static str {
    match s {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RECEIVED",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT_1",
        7 => "FIN_WAIT_2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        _ => "",
    }
}

/// Every interface (Rs2NetQueryIface from index 0 on).
pub fn interfaces() -> alloc::vec::Vec<IfInfo> {
    let mut v = alloc::vec::Vec::new();
    loop {
        let mut i = IfInfo::default();
        if unsafe { Rs2NetQueryIface(v.len() as u32, &mut i, core::mem::size_of::<IfInfo>() as u32) } != 0 {
            return v;
        }
        v.push(i);
    }
}

/// Interfaces with the names OS/2's TCP/IP gives them ("lan0".. for the
/// adapters, in order, then "lo" for loopback), as the socket DLLs and
/// the network utilities show them.
pub fn os2_interfaces() -> alloc::vec::Vec<(String, IfInfo)> {
    let mut lan = 0;
    let mut v: alloc::vec::Vec<(String, IfInfo)> = alloc::vec::Vec::new();
    for i in interfaces() {
        let name = if i.flags & IF_LOOPBACK != 0 {
            String::from("lo")
        } else {
            lan += 1;
            alloc::format!("lan{}", lan - 1)
        };
        v.push((name, i));
    }
    v.sort_by_key(|(_, i)| i.flags & IF_LOOPBACK != 0);
    v
}

/// A route of [`routes`].
pub struct Route {
    pub dest: [u8; 4],
    pub mask: [u8; 4],
    /// The next hop; for a network the interface is on, the interface's
    /// address (as OS/2's netstat shows it).
    pub gateway: [u8; 4],
    /// RTF_UP 1, RTF_GATEWAY 2, RTF_HOST 4, RTF_STATIC 0x800.
    pub flags: u16,
    pub iface: String,
    pub index: u32,
    pub metric: u32,
    pub uses: u32,
    /// Made from an interface's configuration, not by ROUTE.
    pub auto: bool,
}

/// The routing table: the networks of the interfaces' addresses, then
/// the routes (default gateways, ROUTE's).
pub fn routes() -> alloc::vec::Vec<Route> {
    let mut v = alloc::vec::Vec::new();
    let Ok(b) = net_control(NC_ROUTES, &[]) else { return v };
    let ifs = interfaces();
    for e in b.chunks_exact(28) {
        let index = le32(e, 12);
        let mut gateway = q4(e, 8);
        let (dest, mask) = (q4(e, 0), q4(e, 4));
        if gateway == [0; 4] {
            // A directly reached network: the interface's address on it.
            if let Some(i) = ifs.get(index as usize) {
                gateway = if_addrs(index)
                    .into_iter()
                    .find(|(a, m)| (0..4).all(|k| a[k] & m[k] == dest[k] & mask[k]))
                    .map_or(i.addr, |x| x.0);
            }
        }
        v.push(Route {
            dest,
            mask,
            gateway,
            flags: u16::from_le_bytes([e[16], e[17]]),
            auto: e[18] != 0,
            iface: iface_name(index),
            index,
            metric: le32(e, 20),
            uses: le32(e, 24),
        });
    }
    v
}

pub fn route_add(dest: [u8; 4], mask: [u8; 4], gateway: [u8; 4], metric: u32) -> Result<(), u32> {
    let mut b = alloc::vec::Vec::new();
    b.extend_from_slice(&dest);
    b.extend_from_slice(&mask);
    b.extend_from_slice(&gateway);
    b.extend_from_slice(&metric.to_le_bytes());
    net_control(NC_ROUTE_ADD, &b).map(|_| ())
}

/// Delete a route; a zero `gateway` matches any.
pub fn route_delete(dest: [u8; 4], mask: [u8; 4], gateway: [u8; 4]) -> Result<(), u32> {
    let mut b = alloc::vec::Vec::new();
    b.extend_from_slice(&dest);
    b.extend_from_slice(&mask);
    b.extend_from_slice(&gateway);
    net_control(NC_ROUTE_DELETE, &b).map(|_| ())
}

/// Delete every route through a gateway; returns how many went.
pub fn route_flush() -> Result<u32, u32> {
    net_control(NC_ROUTE_FLUSH, &[]).map(|b| if b.len() >= 4 { le32(&b, 0) } else { 0 })
}

/// An ARP table entry.
pub struct ArpEntry {
    pub ip: [u8; 4],
    pub mac: [u8; 6],
    /// Resolved (else a resolution is in progress).
    pub complete: bool,
    pub permanent: bool,
    pub publish: bool,
    pub index: u32,
    /// Seconds until it expires (0: permanent).
    pub ttl: u32,
}

pub fn arp_table() -> alloc::vec::Vec<ArpEntry> {
    let Ok(b) = net_control(NC_ARP_LIST, &[]) else { return alloc::vec::Vec::new() };
    b.chunks_exact(20)
        .map(|e| {
            let f = u16::from_le_bytes([e[10], e[11]]);
            ArpEntry {
                ip: q4(e, 0),
                mac: [e[4], e[5], e[6], e[7], e[8], e[9]],
                complete: f & 1 != 0,
                permanent: f & 2 != 0,
                publish: f & 4 != 0,
                index: le32(e, 12),
                ttl: le32(e, 16),
            }
        })
        .collect()
}

pub fn arp_set(ip: [u8; 4], mac: [u8; 6], permanent: bool, publish: bool) -> Result<(), u32> {
    let mut b = alloc::vec::Vec::new();
    b.extend_from_slice(&ip);
    b.extend_from_slice(&mac);
    b.extend_from_slice(&((permanent as u16) << 1 | (publish as u16) << 2).to_le_bytes());
    net_control(NC_ARP_SET, &b).map(|_| ())
}

pub fn arp_delete(ip: [u8; 4]) -> Result<(), u32> {
    net_control(NC_ARP_DELETE, &ip).map(|_| ())
}

/// Change an interface (an IFOP_* operation with its arguments).
pub fn if_set(index: u32, op: u32, a: [u8; 4], b: [u8; 4]) -> Result<(), u32> {
    let mut v = words(&[index, op]);
    v.extend_from_slice(&a);
    v.extend_from_slice(&b);
    net_control(NC_IF_SET, &v).map(|_| ())
}

/// An interface's addresses and masks (primary first).
pub fn if_addrs(index: u32) -> alloc::vec::Vec<([u8; 4], [u8; 4])> {
    match net_control(NC_IF_ADDRS, &index.to_le_bytes()) {
        Ok(b) => b.chunks_exact(8).map(|e| (q4(e, 0), q4(e, 4))).collect(),
        Err(_) => alloc::vec::Vec::new(),
    }
}

/// The stack's counters (see Rs2NetControl NC_STATS for the order).
pub fn stats() -> alloc::vec::Vec<u32> {
    match net_control(NC_STATS, &[]) {
        Ok(b) => b.chunks_exact(4).map(|e| le32(e, 0)).collect(),
        Err(_) => alloc::vec::Vec::new(),
    }
}

/// Read (and with `set`, change) a variable of the stack; returns the
/// value before.
pub fn net_var(id: u32, set: Option<u32>) -> Result<u32, u32> {
    let b = words(&[id, set.is_some() as u32, set.unwrap_or(0)]);
    net_control(NC_SYSCTL, &b).map(|v| if v.len() >= 4 { le32(&v, 0) } else { 0 })
}

/// The host name: %HOSTNAME%, else "rs2" (the name the kernel's resolver
/// answers for).
pub fn hostname() -> String {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { crate::DosScanEnv(c"HOSTNAME".as_ptr() as *const u8, &mut v) } == 0 && !v.is_null() {
        let mut s = String::new();
        let mut p = v;
        unsafe {
            while *p != 0 {
                s.push(*p as char);
                p = p.add(1);
            }
        }
        let t = s.trim();
        if !t.is_empty() {
            return String::from(t);
        }
    }
    String::from("rs2")
}

/// Every socket of the stack.
pub fn sockets() -> alloc::vec::Vec<SockInfo> {
    let mut n = 0;
    unsafe { Rs2NetQuerySockets(core::ptr::null_mut(), 0, &mut n) };
    let mut v = alloc::vec![SockInfo::default(); n as usize + 16];
    let mut got = 0;
    if unsafe { Rs2NetQuerySockets(v.as_mut_ptr(), v.len() as u32, &mut got) } != 0 {
        return alloc::vec::Vec::new();
    }
    v.truncate(got.min(v.len() as u32) as usize);
    v
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
