//! RS2API networking entries (ordinals 200-217): rs2's kernel socket
//! interface, for rs2's own programs and for the OS/2 socket DLLs built on
//! it (SO32DLL/TCP32DLL/TCPIP32).
//!
//! All are 32-bit `_System` calls returning 0 or a BSD errno value (EBADF
//! 9, EWOULDBLOCK 35, ECONNREFUSED 61, ...; OS/2's SOCBASEERR offset of
//! 10000 is added by the DLL layer). Socket handles are per process.
//! Addresses are `struct sockaddr_in` in the 4.3BSD layout OS/2's TCP/IP
//! 4.0 uses: `USHORT sin_family` (AF_INET = 2), `USHORT sin_port`
//! (network order), `ULONG sin_addr` (network order), 8 bytes of zero;
//! the 4.4BSD form (`UCHAR sin_len = 16, UCHAR sin_family`) of TCP/IP 4.1+
//! is accepted as input too.
//!
//! | Ord | Entry |
//! |-----|-------|
//! | 200 | Rs2NetQueryIface(ULONG index, PRS2IFINFO, ULONG cb) |
//! | 201 | Rs2SockCreate(ULONG domain, ULONG type, ULONG protocol, PULONG ps) |
//! | 202 | Rs2SockBind(ULONG s, PSOCKADDR_IN, ULONG cb) |
//! | 203 | Rs2SockListen(ULONG s, ULONG backlog) |
//! | 204 | Rs2SockAccept(ULONG s, PSOCKADDR_IN peer or 0, PULONG ps) |
//! | 205 | Rs2SockConnect(ULONG s, PSOCKADDR_IN, ULONG cb) |
//! | 206 | Rs2SockSend(ULONG s, PVOID buf, ULONG cb, ULONG flags, PULONG sent) |
//! | 207 | Rs2SockRecv(ULONG s, PVOID buf, ULONG cb, ULONG flags, PULONG got) |
//! | 208 | Rs2SockSendTo(ULONG s, PVOID buf, ULONG cb, ULONG flags, PSOCKADDR_IN to, PULONG sent) |
//! | 209 | Rs2SockRecvFrom(ULONG s, PVOID buf, ULONG cb, ULONG flags, PSOCKADDR_IN from or 0, PULONG got) |
//! | 210 | Rs2SockShutdown(ULONG s, ULONG how) |
//! | 211 | Rs2SockClose(ULONG s) |
//! | 212 | Rs2SockSelect(PULONG socks, ULONG nread, ULONG nwrite, ULONG nexcept, LONG ms, PULONG nready) |
//! | 213 | Rs2SockIoctl(ULONG s, ULONG cmd, PULONG arg) |
//! | 214 | Rs2SockSetOpt(ULONG s, ULONG level, ULONG opt, PVOID val, ULONG cb) |
//! | 215 | Rs2SockGetOpt(ULONG s, ULONG level, ULONG opt, PVOID val, PULONG cb) |
//! | 216 | Rs2SockGetName(ULONG s, ULONG peer, PSOCKADDR_IN) |
//! | 217 | Rs2NetResolve(PSZ name, PULONG addrs, ULONG max, PULONG count) |
//! | 218 | Rs2SockCancel(ULONG s) |
//! | 219 | Rs2NetQuerySockets(PRS2SOCKINFO, ULONG max, PULONG count) |
//! | 220 | Rs2SockTake(ULONG s) |
//! | 221 | Rs2SockRelease(ULONG s) |
//! | 240 | Rs2NetControl(ULONG func, PVOID in, ULONG cbIn, PVOID out, PULONG pcbOut) |
//!
//! Rs2SockSelect has OS/2's `select()` semantics: `socks` lists the read,
//! write and exception sockets in that order, entries that are not ready
//! are set to -1, `ms` < 0 waits forever. Rs2NetResolve returns 0 or an
//! h_errno value (1 HOST_NOT_FOUND, 2 TRY_AGAIN, 4 NO_DATA) and the
//! addresses in network order. RS2IFINFO is: `CHAR name[16]`, `BYTE
//! mac[6]`, 2 pad, `ULONG addr, mask, gateway`, `ULONG dns[4]`, `ULONG
//! ndns, mtu, flags` (bit 0 link, 1 up, 2 loopback, 3 DHCP, 4 lease
//! bound), `ULONG rx_packets, tx_packets, rx_bytes, tx_bytes, rx_dropped`
//! (84 bytes).
//!
//! Rs2SockCancel (so_cancel) ends the calls other threads of the process
//! are blocked in on the socket with EINTR. Rs2NetQuerySockets lists every
//! socket of the stack (netstat), `count` getting the total (entries
//! beyond `max` are not written). RS2SOCKINFO is: `BYTE kind` (1 TCP, 2
//! UDP, 3 raw), `BYTE protocol` (raw sockets), `BYTE state` (TCP: 1
//! CLOSED, 2 LISTEN, 3 SYN_SENT, 4 SYN_RECEIVED, 5 ESTABLISHED, 6
//! FIN_WAIT_1, 7 FIN_WAIT_2, 8 CLOSE_WAIT, 9 CLOSING, 10 LAST_ACK, 11
//! TIME_WAIT; 0 none), `BYTE flags` (bit 0 listening, 1 closed by its
//! owner), `BYTE laddr[4]`, `USHORT lport, rport` (host order), `BYTE
//! raddr[4]`, `ULONG pid, socket` of the owner (0 if none) (24 bytes).
//!
//! Rs2NetControl is the configuration interface (ROUTE, ARP, IFCONFIG,
//! NETSTAT -s, the socket DLLs' SIOC* ioctls and sysctl). Its functions:
//! 1 routes (out: 28-byte entries `ULONG dest, mask, gateway, iface;
//! USHORT flags (RTF_*), auto; ULONG metric, uses`, the interfaces'
//! networks first), 2 add a route (in: `dest, mask, gateway, metric`),
//! 3 delete one (in: `dest, mask, gateway` or 0), 4 delete every route
//! through a gateway (out: ULONG count), 5 the ARP table (out: 20-byte
//! entries `ULONG ip; BYTE mac[6]; USHORT flags` (1 complete, 2
//! permanent, 4 published) `ULONG iface, seconds left`), 6 set an entry
//! (in: `ULONG ip; BYTE mac[6]; USHORT flags`), 7 delete one (in: ip),
//! 8 change an interface (in: `ULONG index, op, a, b`; op 1 address a
//! with mask b (0: keep or classful), 2 netmask a, 3 up, 4 down, 5 add
//! alias a/b, 6 delete address a, 7 MTU a, 8 metric a, 9 DHCP renew (or
//! start), 10 DHCP release), 9 an interface's addresses (in: index; out:
//! address/mask pairs), 10 counters (out: 39 ULONGs: IP received, bad,
//! delivered, sent, forwarded, no route, unknown protocol, fragments,
//! reassembled, fragmented; ICMP received, sent, echo requests, unreachables
//! received; UDP received, sent, no port, bad; TCP active and passive
//! opens, failed attempts, resets sent, bad segments; ARP requests and
//! replies sent; TCP segments sent and received, bytes sent and received,
//! retransmissions, bytes retransmitted, timeouts, out-of-order segments,
//! duplicates, delayed ACKs, keepalive probes and drops, SACK blocks,
//! urgent notifications), 11 a variable (in: `ULONG id, set, value`;
//! out: the value before; id 1 IP forwarding, 2-4 keepalive idle time,
//! interval (seconds) and probes, 5-6 default send and receive buffer).
//! Interface indexes are those of Rs2NetQueryIface (0 is loopback).
//!
//! Socket numbers are unique in the system and every process may use
//! every socket (OS/2's model); a socket belongs to one process, which
//! closes it by ending. Rs2SockTake makes the caller the owner
//! (addsockettolist), Rs2SockRelease gives the caller's socket up so it
//! survives the caller (removesocketfromlist; ENOTSOCK if the caller did
//! not own it).

use alloc::vec::Vec;

use super::Ctx;
use crate::net::socket::{self as so, EINVAL};
use tcpip::{Endpoint, Ipv4};

const EFAULT: u32 = 14;
/// Largest transfer per call.
const MAX_IO: u32 = 1 << 20;
pub const IFINFO_SIZE: usize = 84;

fn read_addr(c: &Ctx, p: u32, len: u32) -> Result<Endpoint, u32> {
    if len < 8 {
        return Err(EINVAL);
    }
    let mut b = [0u8; 8];
    if !c.read(p, &mut b) {
        return Err(EFAULT);
    }
    // 4.3BSD: family 2 as a little-endian USHORT; 4.4BSD: sin_len (16,
    // or 0 from programs that leave it unset), then family 2.
    let family = if b[1] == 2 && b[0] != 2 { 2 } else { u16::from_le_bytes([b[0], b[1]]) };
    if family != 2 {
        return Err(47);
    }
    Ok(Endpoint::new(Ipv4([b[4], b[5], b[6], b[7]]), u16::from_be_bytes([b[2], b[3]])))
}

fn write_addr(c: &Ctx, p: u32, ep: Endpoint) -> bool {
    if p == 0 {
        return true;
    }
    let mut b = [0u8; 16];
    b[0] = 2;
    b[2..4].copy_from_slice(&ep.port.to_be_bytes());
    b[4..8].copy_from_slice(&ep.addr.0);
    c.write(p, &b)
}

fn pid(c: &Ctx) -> u32 {
    c.process.pid
}

fn done(r: Result<(), u32>) -> u32 {
    r.err().unwrap_or(0)
}

/// Rs2NetQueryIface.
pub fn query_iface(c: &mut Ctx) -> u32 {
    let (index, at, cb) = (c.arg(0), c.arg(1), c.arg(2));
    if (cb as usize) < IFINFO_SIZE {
        return EINVAL;
    }
    let Some(i) = crate::net::iface_info(index as usize) else { return EINVAL };
    let mut b = Vec::with_capacity(IFINFO_SIZE);
    let mut name = [0u8; 16];
    for (d, s) in name.iter_mut().zip(i.name.bytes().take(15)) {
        *d = s;
    }
    b.extend_from_slice(&name);
    b.extend_from_slice(&i.mac);
    b.extend_from_slice(&[0, 0]);
    for a in [i.addr, i.mask, i.gateway] {
        b.extend_from_slice(&a.0);
    }
    for n in 0..4 {
        b.extend_from_slice(&i.dns.get(n).copied().unwrap_or_default().0);
    }
    let st = i.stats;
    for v in [i.dns.len().min(4) as u32, i.mtu, i.flags, st.rx_packets as u32, st.tx_packets as u32, st.rx_bytes as u32, st.tx_bytes as u32, st.rx_dropped as u32] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    debug_assert_eq!(b.len(), IFINFO_SIZE);
    if !c.write(at, &b) {
        return EFAULT;
    }
    0
}

/// Rs2SockCreate.
pub fn create(c: &mut Ctx) -> u32 {
    let (d, t, p, out) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    match so::create(pid(c), d, t, p) {
        Ok(s) => {
            if !c.write_u32(out, s) {
                let _ = so::close(pid(c), s);
                return EFAULT;
            }
            0
        }
        Err(e) => e,
    }
}

/// Rs2SockBind.
pub fn bind(c: &mut Ctx) -> u32 {
    let (s, p, len) = (c.arg(0), c.arg(1), c.arg(2));
    match read_addr(c, p, len) {
        Ok(ep) => done(so::bind(pid(c), s, ep)),
        Err(e) => e,
    }
}

/// Rs2SockListen.
pub fn listen(c: &mut Ctx) -> u32 {
    done(so::listen(pid(c), c.arg(0), c.arg(1)))
}

/// Rs2SockAccept.
pub fn accept(c: &mut Ctx) -> u32 {
    let (s, peer_at, out) = (c.arg(0), c.arg(1), c.arg(2));
    match so::accept(pid(c), s) {
        Ok((n, peer)) => {
            if !write_addr(c, peer_at, peer) || !c.write_u32(out, n) {
                let _ = so::close(pid(c), n);
                return EFAULT;
            }
            0
        }
        Err(e) => e,
    }
}

/// Rs2SockConnect.
pub fn connect(c: &mut Ctx) -> u32 {
    let (s, p, len) = (c.arg(0), c.arg(1), c.arg(2));
    match read_addr(c, p, len) {
        Ok(ep) => done(so::connect(pid(c), s, ep)),
        Err(e) => e,
    }
}

fn send_common(c: &mut Ctx, to: Option<Endpoint>, out: u32) -> u32 {
    let (s, buf, len, flags) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let len = len.min(MAX_IO);
    let data = if len == 0 { Vec::new() } else {
        match c.read_vec(buf, len) {
            Some(d) => d,
            None => return EFAULT,
        }
    };
    match so::send(pid(c), s, &data, flags, to) {
        Ok(n) => {
            c.write_u32(out, n as u32);
            0
        }
        Err(e) => e,
    }
}

/// Rs2SockSend.
pub fn send(c: &mut Ctx) -> u32 {
    let out = c.arg(4);
    send_common(c, None, out)
}

/// Rs2SockSendTo.
pub fn send_to(c: &mut Ctx) -> u32 {
    let (p, out) = (c.arg(4), c.arg(5));
    match read_addr(c, p, 16) {
        Ok(ep) => send_common(c, Some(ep), out),
        Err(e) => e,
    }
}

fn recv_common(c: &mut Ctx, from_at: u32, out: u32) -> u32 {
    let (s, buf, len, flags) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    match so::recv(pid(c), s, len.min(MAX_IO) as usize, flags) {
        Ok((data, from)) => {
            if !data.is_empty() && !c.write(buf, &data) {
                return EFAULT;
            }
            write_addr(c, from_at, from);
            c.write_u32(out, data.len() as u32);
            0
        }
        Err(e) => e,
    }
}

/// Rs2SockRecv.
pub fn recv(c: &mut Ctx) -> u32 {
    let out = c.arg(4);
    recv_common(c, 0, out)
}

/// Rs2SockRecvFrom.
pub fn recv_from(c: &mut Ctx) -> u32 {
    let (from, out) = (c.arg(4), c.arg(5));
    recv_common(c, from, out)
}

/// Rs2SockShutdown.
pub fn shutdown(c: &mut Ctx) -> u32 {
    done(so::shutdown(pid(c), c.arg(0), c.arg(1)))
}

/// Rs2SockClose.
pub fn close(c: &mut Ctx) -> u32 {
    done(so::close(pid(c), c.arg(0)))
}

/// Rs2SockSelect.
pub fn select(c: &mut Ctx) -> u32 {
    let (at, nr, nw, ne, ms, out) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4) as i32, c.arg(5));
    let n = nr as usize + nw as usize + ne as usize;
    if n > 2048 {
        return EINVAL;
    }
    let mut socks = alloc::vec![0u32; n];
    for (i, s) in socks.iter_mut().enumerate() {
        match c.read_u32(at + 4 * i as u32) {
            Some(v) => *s = v,
            None => return EFAULT,
        }
    }
    match so::select(pid(c), &mut socks, nr as usize, nw as usize, ms) {
        Ok(k) => {
            for (i, s) in socks.iter().enumerate() {
                c.write_u32(at + 4 * i as u32, *s);
            }
            c.write_u32(out, k);
            0
        }
        Err(e) => e,
    }
}

/// Rs2SockIoctl: FIONBIO reads *arg, FIONREAD writes it.
pub fn ioctl(c: &mut Ctx) -> u32 {
    let (s, cmd, arg) = (c.arg(0), c.arg(1), c.arg(2));
    let v = if cmd == so::FIONBIO {
        match c.read_u32(arg) {
            Some(v) => v,
            None => return EFAULT,
        }
    } else {
        0
    };
    match so::ioctl(pid(c), s, cmd, v) {
        Ok(r) => {
            if cmd == so::FIONREAD {
                c.write_u32(arg, r);
            }
            0
        }
        Err(e) => e,
    }
}

/// Rs2SockSetOpt.
pub fn set_opt(c: &mut Ctx) -> u32 {
    let (s, level, opt, val, len) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    let Some(v) = c.read_vec(val, len.min(64)) else { return EFAULT };
    done(so::set_option(pid(c), s, level, opt, &v))
}

/// Rs2SockGetOpt.
pub fn get_opt(c: &mut Ctx) -> u32 {
    let (s, level, opt, val, len_at) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    let Some(room) = c.read_u32(len_at) else { return EFAULT };
    match so::get_option(pid(c), s, level, opt) {
        Ok(v) => {
            let n = v.len().min(room as usize);
            if !c.write(val, &v[..n]) {
                return EFAULT;
            }
            c.write_u32(len_at, n as u32);
            0
        }
        Err(e) => e,
    }
}

/// Rs2SockGetName.
pub fn get_name(c: &mut Ctx) -> u32 {
    let (s, peer, at) = (c.arg(0), c.arg(1), c.arg(2));
    match so::name(pid(c), s, peer != 0) {
        Ok(ep) => {
            if !write_addr(c, at, ep) {
                return EFAULT;
            }
            0
        }
        Err(e) => e,
    }
}

/// Rs2NetResolve.
pub fn resolve(c: &mut Ctx) -> u32 {
    let (name_at, addrs, max, out) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let Some(name) = c.read_str(name_at, 256) else { return 1 };
    match so::resolve(&name) {
        Ok(v) => {
            let n = v.len().min(max as usize);
            for (i, a) in v.iter().take(n).enumerate() {
                c.write(addrs + 4 * i as u32, &a.0);
            }
            c.write_u32(out, n as u32);
            0
        }
        Err(h) => h,
    }
}

/// Rs2SockCancel.
pub fn cancel(c: &mut Ctx) -> u32 {
    done(so::cancel(pid(c), c.arg(0)))
}

pub const SOCKINFO_SIZE: usize = 24;

/// Rs2NetQuerySockets.
pub fn query_sockets(c: &mut Ctx) -> u32 {
    let (at, max, out) = (c.arg(0), c.arg(1), c.arg(2));
    let list = so::list();
    for (i, (info, owner, handle)) in list.iter().take(max.min(4096) as usize).enumerate() {
        let mut b = [0u8; SOCKINFO_SIZE];
        let (kind, proto) = match info.kind {
            tcpip::stack::Kind::Tcp => (1, 6),
            tcpip::stack::Kind::Udp => (2, 17),
            tcpip::stack::Kind::Raw(p) => (3, p),
        };
        b[0] = kind;
        b[1] = proto;
        b[2] = info.state.map_or(0, |s| s as u8 + 1);
        b[3] = info.listening as u8 | (info.user_closed as u8) << 1;
        b[4..8].copy_from_slice(&info.local.addr.0);
        b[8..10].copy_from_slice(&info.local.port.to_le_bytes());
        let remote = info.remote.unwrap_or(Endpoint::new(Ipv4([0; 4]), 0));
        b[10..12].copy_from_slice(&remote.port.to_le_bytes());
        b[12..16].copy_from_slice(&remote.addr.0);
        b[16..20].copy_from_slice(&owner.to_le_bytes());
        b[20..24].copy_from_slice(&handle.to_le_bytes());
        if !c.write(at + (i * SOCKINFO_SIZE) as u32, &b) {
            return EFAULT;
        }
    }
    if !c.write_u32(out, list.len() as u32) {
        return EFAULT;
    }
    0
}

/// Rs2SockTake.
pub fn take(c: &mut Ctx) -> u32 {
    done(so::take(pid(c), c.arg(0)))
}

/// Rs2SockRelease.
pub fn release(c: &mut Ctx) -> u32 {
    done(so::release(pid(c), c.arg(0)))
}

// ---- Rs2NetControl (240): routes, ARP, interfaces, counters, sysctl ----

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
const IF_ADDRESS: u32 = 1;
const IF_NETMASK: u32 = 2;
const IF_UP: u32 = 3;
const IF_DOWN: u32 = 4;
const IF_ALIAS_ADD: u32 = 5;
const IF_ALIAS_DELETE: u32 = 6;
const IF_MTU: u32 = 7;
const IF_METRIC: u32 = 8;
const IF_DHCP_RENEW: u32 = 9;
const IF_DHCP_RELEASE: u32 = 10;

// NC_SYSCTL variables.
const SYS_FORWARDING: u32 = 1;
const SYS_KEEPIDLE: u32 = 2;
const SYS_KEEPINTVL: u32 = 3;
const SYS_KEEPCNT: u32 = 4;
const SYS_SENDSPACE: u32 = 5;
const SYS_RECVSPACE: u32 = 6;

fn le32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn ip_in(b: &[u8], o: usize) -> Ipv4 {
    b.get(o..o + 4).map_or(Ipv4::UNSPECIFIED, |x| Ipv4([x[0], x[1], x[2], x[3]]))
}

/// Rs2NetControl(ULONG func, PVOID in, ULONG cbIn, PVOID out, PULONG
/// pcbOut): `*pcbOut` holds the room at `out` and gets the bytes the
/// answer takes (lists are cut at the room; call again with more).
pub fn net_control(c: &mut Ctx) -> u32 {
    let (func, in_at, in_len, out_at, len_at) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    let input = if in_len == 0 {
        Vec::new()
    } else {
        match c.read_vec(in_at, in_len.min(256)) {
            Some(v) => v,
            None => return EFAULT,
        }
    };
    let room = if len_at == 0 { 0 } else { c.read_u32(len_at).unwrap_or(0) as usize };
    let r = control(func, &input);
    match r {
        Ok(out) => {
            let n = out.len().min(room);
            if n > 0 && !c.write(out_at, &out[..n]) {
                return EFAULT;
            }
            if len_at != 0 && !c.write_u32(len_at, out.len() as u32) {
                return EFAULT;
            }
            0
        }
        Err(e) => e,
    }
}

fn st_err(e: tcpip::Error) -> u32 {
    e.errno()
}

fn control(func: u32, i: &[u8]) -> Result<Vec<u8>, u32> {
    use crate::net::with_stack;
    let need = |n: usize| if i.len() < n { Err(EINVAL) } else { Ok(()) };
    let down: u32 = 50; // ENETDOWN
    match func {
        NC_ROUTES => {
            let rs = with_stack(|st, _| st.routes()).ok_or(down)?;
            let mut v = Vec::with_capacity(rs.len() * 28);
            for r in rs {
                v.extend_from_slice(&r.dest.0);
                v.extend_from_slice(&r.mask.0);
                v.extend_from_slice(&r.gateway.unwrap_or_default().0);
                v.extend_from_slice(&(r.iface as u32).to_le_bytes());
                v.extend_from_slice(&r.flags.to_le_bytes());
                v.extend_from_slice(&(r.auto as u16).to_le_bytes());
                v.extend_from_slice(&r.metric.to_le_bytes());
                v.extend_from_slice(&(r.uses.get() as u32).to_le_bytes());
            }
            Ok(v)
        }
        NC_ROUTE_ADD => {
            need(16)?;
            let (d, m, g, metric) = (ip_in(i, 0), ip_in(i, 4), ip_in(i, 8), le32(i, 12));
            with_stack(|st, _| st.add_route(d, m, g, metric)).ok_or(down)?.map_err(st_err)?;
            Ok(Vec::new())
        }
        NC_ROUTE_DELETE => {
            need(12)?;
            let (d, m, g) = (ip_in(i, 0), ip_in(i, 4), ip_in(i, 8));
            let g = (!g.is_unspecified()).then_some(g);
            with_stack(|st, _| st.delete_route(d, m, g)).ok_or(down)?.map_err(st_err)?;
            Ok(Vec::new())
        }
        NC_ROUTE_FLUSH => {
            let n = with_stack(|st, _| st.flush_routes()).ok_or(down)?;
            Ok((n as u32).to_le_bytes().to_vec())
        }
        NC_ARP_LIST => {
            let (list, now) = with_stack(|st, now| (st.arp_list(), now)).ok_or(down)?;
            let mut v = Vec::with_capacity(list.len() * 20);
            for e in list {
                v.extend_from_slice(&e.ip.0);
                v.extend_from_slice(&e.mac.unwrap_or_default());
                let flags = e.mac.is_some() as u16 | (e.permanent as u16) << 1 | (e.publish as u16) << 2;
                v.extend_from_slice(&flags.to_le_bytes());
                v.extend_from_slice(&(e.iface as u32).to_le_bytes());
                let ttl = if e.permanent { 0 } else { (e.expires.saturating_sub(now) / 1000) as u32 };
                v.extend_from_slice(&ttl.to_le_bytes());
            }
            Ok(v)
        }
        NC_ARP_SET => {
            need(12)?;
            let ip = ip_in(i, 0);
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&i[4..10]);
            let flags = u16::from_le_bytes([i[10], i[11]]);
            with_stack(|st, now| st.arp_set(ip, mac, flags & 2 != 0, flags & 4 != 0, now)).ok_or(down)?.map_err(st_err)?;
            Ok(Vec::new())
        }
        NC_ARP_DELETE => {
            need(4)?;
            let ip = ip_in(i, 0);
            with_stack(|st, _| st.arp_delete(ip)).ok_or(down)?.map_err(st_err)?;
            Ok(Vec::new())
        }
        NC_IF_SET => {
            need(16)?;
            let (n, op, a, b) = (le32(i, 0) as usize, le32(i, 4), ip_in(i, 8), ip_in(i, 12));
            let (av, bv) = (le32(i, 8), le32(i, 12));
            with_stack(|st, now| -> Result<(), tcpip::Error> {
                if n >= st.ifaces.len() {
                    return Err(tcpip::Error::NoDevice);
                }
                match op {
                    IF_ADDRESS => st.set_address(n, a, (bv != 0).then_some(b), now),
                    IF_NETMASK => st.set_netmask(n, a),
                    IF_UP => {
                        st.iface_up(n, true);
                        Ok(())
                    }
                    IF_DOWN => {
                        st.iface_up(n, false);
                        Ok(())
                    }
                    IF_ALIAS_ADD => {
                        let m = if bv == 0 { st.ifaces[n].mask } else { b };
                        st.add_alias(n, a, m)
                    }
                    IF_ALIAS_DELETE => st.delete_alias(n, a),
                    IF_MTU => st.set_mtu(n, av as usize),
                    IF_METRIC => st.set_metric(n, av),
                    IF_DHCP_RENEW => st.dhcp_renew(n, now),
                    IF_DHCP_RELEASE => st.dhcp_release(n, now),
                    _ => Err(tcpip::Error::InvalidArg),
                }
            })
            .ok_or(down)?
            .map_err(st_err)?;
            Ok(Vec::new())
        }
        NC_IF_ADDRS => {
            need(4)?;
            let n = le32(i, 0) as usize;
            let a = with_stack(|st, _| st.ifaces.get(n).map(|f| f.addresses())).ok_or(down)?.ok_or(6u32)?;
            let mut v = Vec::new();
            for (x, m) in a {
                v.extend_from_slice(&x.0);
                v.extend_from_slice(&m.0);
            }
            Ok(v)
        }
        NC_STATS => {
            let (k, t) = with_stack(|st, _| (st.counters, st.tcp_stats())).ok_or(down)?;
            let vals = [
                k.ip_received,
                k.ip_bad,
                k.ip_delivered,
                k.ip_sent,
                k.ip_forwarded,
                k.ip_no_route,
                k.ip_no_proto,
                k.ip_fragments,
                k.ip_reassembled,
                k.ip_fragmented,
                k.icmp_received,
                k.icmp_sent,
                k.icmp_echo_received,
                k.icmp_unreach_received,
                k.udp_received,
                k.udp_sent,
                k.udp_no_port,
                k.udp_bad,
                k.tcp_active_opens,
                k.tcp_passive_opens,
                k.tcp_attempt_fails,
                k.tcp_resets_sent,
                k.tcp_bad,
                k.arp_requests,
                k.arp_replies,
                t.segs_sent,
                t.segs_received,
                t.bytes_sent,
                t.bytes_received,
                t.retransmits,
                t.retransmitted_bytes,
                t.timeouts,
                t.out_of_order,
                t.duplicates,
                t.delayed_acks,
                t.keepalive_probes,
                t.keepalive_drops,
                t.sack_blocks,
                t.urgent_received,
            ];
            let mut v = Vec::with_capacity(vals.len() * 4);
            for x in vals {
                v.extend_from_slice(&(x as u32).to_le_bytes());
            }
            Ok(v)
        }
        NC_SYSCTL => {
            need(12)?;
            let (id, set, val) = (le32(i, 0), le32(i, 4) != 0, le32(i, 8));
            let r = with_stack(|st, _| -> Result<u32, u32> {
                let mut k = st.keepalive;
                let cur = match id {
                    SYS_FORWARDING => st.forwarding as u32,
                    SYS_KEEPIDLE => (k.idle / 1000) as u32,
                    SYS_KEEPINTVL => (k.interval / 1000) as u32,
                    SYS_KEEPCNT => k.count,
                    SYS_SENDSPACE => st.default_send_buf as u32,
                    SYS_RECVSPACE => st.default_recv_buf as u32,
                    _ => return Err(2), // ENOENT
                };
                if set {
                    match id {
                        SYS_FORWARDING => st.forwarding = val != 0,
                        SYS_KEEPIDLE | SYS_KEEPINTVL | SYS_KEEPCNT => {
                            if val == 0 {
                                return Err(EINVAL);
                            }
                            match id {
                                SYS_KEEPIDLE => k.idle = val as u64 * 1000,
                                SYS_KEEPINTVL => k.interval = val as u64 * 1000,
                                _ => k.count = val,
                            }
                            st.set_keepalive_timing(k);
                        }
                        SYS_SENDSPACE => st.default_send_buf = (val as usize).clamp(512, 1 << 20),
                        _ => st.default_recv_buf = (val as usize).clamp(512, tcpip::tcp::MAX_BUFFER),
                    }
                }
                Ok(cur)
            })
            .ok_or(down)??;
            Ok(r.to_le_bytes().to_vec())
        }
        _ => Err(EINVAL),
    }
}
