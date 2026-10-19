//! The socket calls over the kernel's sockets (RS2API 201-218).
//!
//! [`Abi`] selects the structures and constants of the two generations
//! of OS/2's socket API: TCP/IP 4.0 (SO32DLL: 4.3BSD `sockaddr` with a
//! USHORT family, message flags up to MSG_FULLREAD, `omsghdr`, bit-mask
//! `fd_set`s for bsdselect) and 4.1+ (TCPIP32: 4.4BSD `sockaddr` with
//! `sin_len`, MSG_WAITALL 0x80 / MSG_DONTWAIT 0x100, `msghdr` with
//! control data, Winsock-style `fd_set` {count, array}).

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::null_mut;

use os2::net::*;

use crate::errno::{self, fail, EFAULT, EINVAL, ENOPROTOOPT, EOPNOTSUPP, ENXIO};
use crate::netdb;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Abi {
    /// TCP/IP 4.0 (SO32DLL, the 16-bit SOCKETS).
    V40,
    /// TCP/IP 4.1+ (TCPIP32).
    V41,
}

const K_MSG_OOB: u32 = 1;
const K_MSG_PEEK: u32 = 2;
const K_MSG_DONTWAIT: u32 = 0x80;

/// Message flags of the API to the kernel's, and whether MSG_WAITALL
/// (4.0: MSG_FULLREAD) was asked for.
fn kflags(abi: Abi, f: i32) -> (u32, bool) {
    let f = f as u32;
    let mut k = f & (K_MSG_OOB | K_MSG_PEEK);
    let waitall = match abi {
        Abi::V40 => f & 0x8 != 0,
        Abi::V41 => {
            if f & 0x100 != 0 {
                k |= K_MSG_DONTWAIT;
            }
            f & 0x88 != 0
        }
    };
    (k, waitall)
}

fn r(rc: u32) -> Result<(), u32> {
    if rc == 0 { Ok(()) } else { Err(rc) }
}

/// Store a kernel address (4.3 layout) in the caller's `sockaddr` of
/// `*len` bytes, in the API's layout; `*len` becomes 16.
pub unsafe fn put_addr(abi: Abi, a: &SockAddrIn, addr: *mut u8, len: *mut i32) {
    if addr.is_null() {
        return;
    }
    let mut b = [0u8; 16];
    match abi {
        Abi::V40 => b[0] = 2,
        Abi::V41 => {
            b[0] = 16;
            b[1] = 2;
        }
    }
    b[2..4].copy_from_slice(&a.port.to_le_bytes());
    b[4..8].copy_from_slice(&a.addr);
    let room = if len.is_null() { 16 } else { unsafe { *len }.clamp(0, 16) as usize };
    unsafe {
        core::ptr::copy_nonoverlapping(b.as_ptr(), addr, room);
        if !len.is_null() {
            *len = 16;
        }
    }
}

pub fn socket(d: i32, t: i32, p: i32) -> i32 {
    // AF_UNSPEC with SOCK_STREAM/DGRAM means the Internet.
    let d = if d == 0 { AF_INET as i32 } else { d };
    // A raw socket of protocol 0 (used for ioctls) is IPPROTO_RAW.
    let p = if t as u32 == SOCK_RAW && p == 0 { 255 } else { p };
    let mut s = 0;
    match unsafe { Rs2SockCreate(d as u32, t as u32, p as u32, &mut s) } {
        0 => s as i32,
        e => fail(e),
    }
}

pub unsafe fn bind(s: i32, addr: *const u8, len: i32) -> i32 {
    if addr.is_null() {
        return fail(EFAULT);
    }
    match unsafe { Rs2SockBind(s as u32, addr as *const SockAddrIn, len.clamp(0, 16) as u32) } {
        0 => 0,
        e => fail(e),
    }
}

/// A destination address with INADDR_ANY (this host) replaced by the
/// loopback address, as BSD stacks treat it.
unsafe fn dest(addr: *const u8, len: i32) -> [u8; 16] {
    let mut b = [0u8; 16];
    unsafe { core::ptr::copy_nonoverlapping(addr, b.as_mut_ptr(), len.clamp(0, 16) as usize) };
    if b[4..8] == [0; 4] {
        b[4..8].copy_from_slice(&[127, 0, 0, 1]);
    }
    b
}

pub unsafe fn connect(s: i32, addr: *const u8, len: i32) -> i32 {
    if addr.is_null() {
        return fail(EFAULT);
    }
    let a = unsafe { dest(addr, len) };
    match unsafe { Rs2SockConnect(s as u32, a.as_ptr() as *const SockAddrIn, len.clamp(0, 16) as u32) } {
        0 => 0,
        e => fail(e),
    }
}

pub fn listen(s: i32, backlog: i32) -> i32 {
    match unsafe { Rs2SockListen(s as u32, backlog.clamp(1, 1024) as u32) } {
        0 => 0,
        e => fail(e),
    }
}

pub unsafe fn accept(abi: Abi, s: i32, addr: *mut u8, len: *mut i32) -> i32 {
    let mut peer = SockAddrIn::default();
    let mut ns = 0;
    match unsafe { Rs2SockAccept(s as u32, &mut peer, &mut ns) } {
        0 => {
            unsafe { put_addr(abi, &peer, addr, len) };
            ns as i32
        }
        e => fail(e),
    }
}

pub unsafe fn send(abi: Abi, s: i32, buf: *const u8, len: i32, flags: i32) -> i32 {
    if len < 0 || buf.is_null() && len > 0 {
        return fail(EFAULT);
    }
    let (k, _) = kflags(abi, flags);
    let mut sent = 0;
    match unsafe { Rs2SockSend(s as u32, buf, len as u32, k & !K_MSG_PEEK, &mut sent) } {
        0 => sent as i32,
        e => fail(e),
    }
}

pub unsafe fn sendto(abi: Abi, s: i32, buf: *const u8, len: i32, flags: i32, to: *const u8, tolen: i32) -> i32 {
    if to.is_null() || tolen == 0 {
        return unsafe { send(abi, s, buf, len, flags) };
    }
    if len < 0 || buf.is_null() && len > 0 || tolen < 8 {
        return fail(if tolen < 8 { EINVAL } else { EFAULT });
    }
    let (k, _) = kflags(abi, flags);
    let mut sent = 0;
    let a = unsafe { dest(to, tolen) };
    match unsafe { Rs2SockSendTo(s as u32, buf, len as u32, k & !K_MSG_PEEK, a.as_ptr() as *const SockAddrIn, &mut sent) } {
        0 => sent as i32,
        e => fail(e),
    }
}

/// recv/recvfrom; MSG_WAITALL repeats until `len` bytes came (or EOF).
pub unsafe fn recvfrom(abi: Abi, s: i32, buf: *mut u8, len: i32, flags: i32, from: *mut u8, fromlen: *mut i32) -> i32 {
    if len < 0 || buf.is_null() && len > 0 {
        return fail(EFAULT);
    }
    let (k, waitall) = kflags(abi, flags);
    if k & K_MSG_OOB != 0 {
        // No urgent data is ever pending.
        return fail(EINVAL);
    }
    let mut total = 0u32;
    loop {
        let mut a = SockAddrIn::default();
        let mut got = 0;
        let rc = unsafe { Rs2SockRecvFrom(s as u32, buf.add(total as usize), len as u32 - total, k, &mut a, &mut got) };
        if rc != 0 {
            if total > 0 {
                return total as i32;
            }
            return fail(rc);
        }
        if total == 0 {
            unsafe { put_addr(abi, &a, from, fromlen) };
        }
        total += got;
        if !waitall || got == 0 || total >= len as u32 || k & K_MSG_PEEK != 0 {
            return total as i32;
        }
    }
}

pub fn shutdown(s: i32, how: i32) -> i32 {
    match unsafe { Rs2SockShutdown(s as u32, how as u32) } {
        0 => 0,
        e => fail(e),
    }
}

pub fn soclose(s: i32) -> i32 {
    match unsafe { Rs2SockClose(s as u32) } {
        0 => 0,
        e => fail(e),
    }
}

pub fn so_cancel(s: i32) -> i32 {
    match unsafe { Rs2SockCancel(s as u32) } {
        0 => 0,
        e => fail(e),
    }
}

/// OS/2's select(): one array of read, write and exception sockets;
/// those not ready become -1. `timeout` in ms, -1 forever.
pub unsafe fn os2_select(socks: *mut i32, nr: i32, nw: i32, ne: i32, timeout: i32) -> i32 {
    if nr < 0 || nw < 0 || ne < 0 || socks.is_null() && nr + nw + ne > 0 {
        return fail(EINVAL);
    }
    let mut n = 0;
    let mut none = [0u32; 1];
    let p = if socks.is_null() { none.as_mut_ptr() } else { socks as *mut u32 };
    match unsafe { Rs2SockSelect(p, nr as u32, nw as u32, ne as u32, timeout.max(-1), &mut n) } {
        0 => n as i32,
        e => fail(e),
    }
}

unsafe fn tv_ms(tv: *const i32) -> i32 {
    if tv.is_null() {
        return -1;
    }
    let (sec, usec) = unsafe { (*tv, *tv.add(1)) };
    let ms = sec.max(0) as i64 * 1000 + usec.max(0) as i64 / 1000;
    ms.min(i32::MAX as i64) as i32
}

/// Run select over three socket lists; each list is rewritten to the
/// ready ones. Returns the total ready.
fn select_lists(lists: &mut [Vec<i32>; 3], ms: i32) -> i32 {
    let mut all: Vec<i32> = lists.iter().flatten().copied().collect();
    let (nr, nw, ne) = (lists[0].len() as i32, lists[1].len() as i32, lists[2].len() as i32);
    let n = unsafe { os2_select(if all.is_empty() { null_mut() } else { all.as_mut_ptr() }, nr, nw, ne, ms) };
    if n < 0 {
        return n;
    }
    let mut at = 0;
    for l in lists.iter_mut() {
        let k = l.len();
        *l = all[at..at + k].iter().copied().filter(|&s| s != -1).collect();
        at += k;
    }
    n
}

/// TCPIP32 select(): `fd_set` is {USHORT fd_count (padded to 4), int
/// fd_array[]}.
pub unsafe fn select_v41(_nfds: i32, rd: *mut u8, wr: *mut u8, ex: *mut u8, tv: *const i32) -> i32 {
    let read = |p: *mut u8| -> Vec<i32> {
        if p.is_null() {
            return Vec::new();
        }
        let n = unsafe { (p as *const u16).read_unaligned() } as usize;
        (0..n.min(8192)).map(|i| unsafe { (p.add(4 + 4 * i) as *const i32).read_unaligned() }).collect()
    };
    let mut lists = [read(rd), read(wr), read(ex)];
    let n = select_lists(&mut lists, unsafe { tv_ms(tv) });
    if n < 0 {
        return n;
    }
    for (p, l) in [rd, wr, ex].into_iter().zip(lists.iter()) {
        if p.is_null() {
            continue;
        }
        unsafe {
            (p as *mut u16).write_unaligned(l.len() as u16);
            for (i, s) in l.iter().enumerate() {
                (p.add(4 + 4 * i) as *mut i32).write_unaligned(*s);
            }
        }
    }
    n
}

/// __TCPFDIsSet(fd, set) for the TCPIP32 fd_set.
pub unsafe fn fd_isset(fd: i32, set: *const u8) -> i32 {
    if set.is_null() {
        return 0;
    }
    let n = unsafe { (set as *const u16).read_unaligned() } as usize;
    (0..n).any(|i| unsafe { (set.add(4 + 4 * i) as *const i32).read_unaligned() } == fd) as i32
}

/// SO32DLL bsdselect(): `fd_set` is a bit mask of `nfds` bits.
pub unsafe fn select_v40(nfds: i32, rd: *mut u32, wr: *mut u32, ex: *mut u32, tv: *const i32) -> i32 {
    let nfds = nfds.clamp(0, 2048) as usize;
    let read = |p: *mut u32| -> Vec<i32> {
        if p.is_null() {
            return Vec::new();
        }
        (0..nfds).filter(|&i| unsafe { *p.add(i / 32) } & (1 << (i % 32)) != 0).map(|i| i as i32).collect()
    };
    let mut lists = [read(rd), read(wr), read(ex)];
    let n = select_lists(&mut lists, unsafe { tv_ms(tv) });
    if n < 0 {
        return n;
    }
    for (p, l) in [rd, wr, ex].into_iter().zip(lists.iter()) {
        if p.is_null() {
            continue;
        }
        unsafe {
            for w in 0..nfds.div_ceil(32) {
                *p.add(w) = 0;
            }
            for &s in l {
                *p.add(s as usize / 32) |= 1 << (s as usize % 32);
            }
        }
    }
    n
}

// ---- options, names ----

pub unsafe fn setsockopt(s: i32, level: i32, opt: i32, val: *const u8, len: i32) -> i32 {
    if val.is_null() && len > 0 {
        return fail(EFAULT);
    }
    match unsafe { Rs2SockSetOpt(s as u32, level as u32, opt as u32, val, len.clamp(0, 64) as u32) } {
        0 => 0,
        // IP-level options the stack does not have (TTL, TOS,
        // multicast) are accepted and ignored, as programs set them
        // in passing.
        e if e == ENOPROTOOPT && level == 0 => 0,
        e => fail(e),
    }
}

pub unsafe fn getsockopt(s: i32, level: i32, opt: i32, val: *mut u8, len: *mut i32) -> i32 {
    if val.is_null() || len.is_null() {
        return fail(EFAULT);
    }
    let mut n = unsafe { *len }.max(0) as u32;
    match unsafe { Rs2SockGetOpt(s as u32, level as u32, opt as u32, val, &mut n) } {
        0 => {
            unsafe { *len = n as i32 };
            0
        }
        e => fail(e),
    }
}

pub unsafe fn getname(abi: Abi, s: i32, peer: bool, addr: *mut u8, len: *mut i32) -> i32 {
    if addr.is_null() {
        return fail(EFAULT);
    }
    let mut a = SockAddrIn::default();
    match unsafe { Rs2SockGetName(s as u32, peer as u32, &mut a) } {
        0 => {
            unsafe { put_addr(abi, &a, addr, len) };
            0
        }
        e => fail(e),
    }
}

// ---- ioctl ----

pub const FIONBIO: u32 = 0x667e;
pub const FIONREAD: u32 = 0x667f;
pub const FIOASYNC: u32 = 0x667d;

/// Interfaces as OS/2 names them ("lan0".., "lo").
pub fn os2_interfaces() -> Vec<(Vec<u8>, IfInfo)> {
    os2::net::os2_interfaces().into_iter().map(|(n, i)| (n.into_bytes(), i)).collect()
}

fn find_iface(name: &[u8]) -> Option<IfInfo> {
    os2_interfaces().into_iter().find(|(n, i)| crate::cstr::eq_ignore_case(n, name) || crate::cstr::eq_ignore_case(i.name().as_bytes(), name)).map(|(_, i)| i)
}

/// IFF_* flags of an interface.
pub fn iff_flags(i: &IfInfo) -> u32 {
    let mut f = 0;
    if i.flags & IF_UP != 0 {
        f |= 0x1; // IFF_UP
    }
    if i.flags & IF_LINK != 0 || i.flags & IF_LOOPBACK != 0 {
        f |= 0x40; // IFF_RUNNING
    }
    if i.flags & IF_LOOPBACK != 0 {
        f |= 0x8; // IFF_LOOPBACK
    } else {
        f |= 0x2 | 0x4000 | 0x800; // IFF_BROADCAST, IFF_ETHER, IFF_MULTICAST
    }
    f
}

fn sockaddr(abi: Abi, a: [u8; 4]) -> [u8; 16] {
    let mut b = [0u8; 16];
    unsafe { put_addr(abi, &SockAddrIn::new(a, 0), b.as_mut_ptr(), null_mut()) };
    b
}

fn broadcast(i: &IfInfo) -> [u8; 4] {
    let mut b = [0u8; 4];
    for k in 0..4 {
        b[k] = i.addr[k] | !i.mask[k];
    }
    b
}

/// ioctl / os2_ioctl. `cmd` is matched on its low 16 bits (group and
/// number), which the 4.0 (`ioc(g,n)`) and 4.4 (`_IOWR(g,n,t)`) forms
/// share; the 4.4 numbers of the interface queries (33-37) are accepted
/// beside the old ones (13-21).
/// The routing table: (destination, gateway, mask, RTF_* flags,
/// interface name).
pub fn routes() -> Vec<([u8; 4], [u8; 4], [u8; 4], u16, Vec<u8>)> {
    os2::net::routes().into_iter().map(|r| (r.dest, r.gateway, r.mask, r.flags, r.iface.into_bytes())).collect()
}

/// SIOSTATRT: struct rtentries {short hostcount, netcount; then per
/// route a packed struct ortentry (64 bytes) and the interface name}.
unsafe fn routing_table(abi: Abi, data: *mut u8, len: usize) -> i32 {
    let rt = routes();
    let mut out = vec![0u8; 4];
    let mut n = 0u16;
    for (dst, gw, _mask, flags, name) in &rt {
        let mut e = [0u8; 64];
        // rt_hash: 0 marks the default route.
        let hash = if *dst == [0; 4] { 0u32 } else { u32::from_le_bytes(*dst) | 1 };
        e[0..4].copy_from_slice(&hash.to_le_bytes());
        e[4..20].copy_from_slice(&sockaddr(abi, *dst));
        e[20..36].copy_from_slice(&sockaddr(abi, *gw));
        e[36..38].copy_from_slice(&flags.to_le_bytes());
        e[48..52].copy_from_slice(&1u32.to_le_bytes());
        if out.len() + 64 + name.len() + 1 > len {
            break;
        }
        out.extend_from_slice(&e);
        out.extend_from_slice(name);
        out.push(0);
        n += 1;
    }
    out[2..4].copy_from_slice(&n.to_le_bytes());
    unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), data, out.len()) };
    0
}

pub unsafe fn ioctl(abi: Abi, s: i32, cmd: u32, data: *mut u8) -> i32 {
    unsafe { ioctl_len(abi, s, cmd, data, -1) }
}

/// ioctl with the data length os2_ioctl() passes (-1: unknown).
pub unsafe fn ioctl_len(abi: Abi, s: i32, cmd: u32, data: *mut u8, len: i32) -> i32 {
    let base = cmd & 0xffff;
    let (group, num) = ((base >> 8) as u8, (base & 0xff) as u8);
    match (group, num) {
        (b'f', 126) => {
            if data.is_null() {
                return fail(EFAULT);
            }
            let mut v = unsafe { (data as *const u32).read_unaligned() };
            match unsafe { Rs2SockIoctl(s as u32, FIONBIO | 0x8004_0000, &mut v) } {
                0 => 0,
                e => fail(e),
            }
        }
        (b'f', 127) => {
            if data.is_null() {
                return fail(EFAULT);
            }
            let mut v = 0u32;
            match unsafe { Rs2SockIoctl(s as u32, FIONREAD | 0x4004_0000, &mut v) } {
                0 => {
                    unsafe { (data as *mut u32).write_unaligned(v) };
                    0
                }
                e => fail(e),
            }
        }
        (b'f', 125) => 0,
        (b's', 7) => {
            // SIOCATMARK: never at an urgent mark.
            if !data.is_null() {
                unsafe { (data as *mut u32).write_unaligned(0) };
            }
            0
        }
        (b'i', 20) | (b'i', 36) => {
            // SIOCGIFCONF: struct ifconf {long len; char *buf}.
            if data.is_null() {
                return fail(EFAULT);
            }
            let (len, buf) = unsafe { ((data as *const i32).read_unaligned(), (data.add(4) as *const *mut u8).read_unaligned()) };
            let mut used = 0usize;
            for (name, i) in os2_interfaces() {
                if used + 32 > len.max(0) as usize || buf.is_null() {
                    break;
                }
                let mut req = [0u8; 32];
                req[..name.len().min(15)].copy_from_slice(&name[..name.len().min(15)]);
                req[16..32].copy_from_slice(&sockaddr(abi, i.addr));
                unsafe { core::ptr::copy_nonoverlapping(req.as_ptr(), buf.add(used), 32) };
                used += 32;
            }
            unsafe { (data as *mut i32).write_unaligned(used as i32) };
            0
        }
        (b'i', 13 | 33 | 15 | 34 | 17 | 18 | 35 | 21 | 37 | 23) => {
            if data.is_null() {
                return fail(EFAULT);
            }
            let name = unsafe { crate::cstr::bytes(data) };
            let name = &name[..name.len().min(16)];
            let Some(i) = find_iface(name) else { return fail(ENXIO) };
            let out: [u8; 16] = match num {
                13 | 33 => sockaddr(abi, i.addr),
                15 | 34 => sockaddr(abi, i.addr),
                18 | 35 => sockaddr(abi, broadcast(&i)),
                21 | 37 => sockaddr(abi, i.mask),
                17 => {
                    let mut b = [0u8; 16];
                    b[..4].copy_from_slice(&iff_flags(&i).to_le_bytes());
                    b
                }
                _ => [0; 16],
            };
            unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), data.add(16), 16) };
            0
        }
        (b'i', 57) => {
            // SIOCGIFMTU: ifr_mtu (int) after the name.
            if data.is_null() {
                return fail(EFAULT);
            }
            let name = unsafe { crate::cstr::bytes(data) };
            let Some(i) = find_iface(&name[..name.len().min(16)]) else { return fail(ENXIO) };
            unsafe { (data.add(16) as *mut u32).write_unaligned(i.mtu) };
            0
        }
        (b's', 14) => 0,
        (b'n', 52) => {
            // SIOSTATCNTRT: number of routes.
            if data.is_null() {
                return fail(EFAULT);
            }
            unsafe { (data as *mut u32).write_unaligned(routes().len() as u32) };
            0
        }
        (b'n', 45) => {
            if data.is_null() {
                return fail(EFAULT);
            }
            unsafe { routing_table(abi, data, if len < 4 { 65535 } else { len as usize }) }
        }
        (b'i', _) | (b'r', _) => fail(errno::EPERM),
        _ => fail(EINVAL),
    }
}

// ---- scatter/gather ----

unsafe fn iovecs<'a>(iov: *const u32, n: i32) -> Option<Vec<(*mut u8, usize)>> {
    if n < 0 || n > 1024 || iov.is_null() && n > 0 {
        return None;
    }
    Some((0..n as usize).map(|i| unsafe { (*iov.add(2 * i) as *mut u8, *iov.add(2 * i + 1) as usize) }).collect())
}

unsafe fn gather(v: &[(*mut u8, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(p, n) in v {
        if !p.is_null() {
            out.extend_from_slice(unsafe { core::slice::from_raw_parts(p, n) });
        }
    }
    out
}

unsafe fn scatter(v: &[(*mut u8, usize)], data: &[u8]) {
    let mut at = 0;
    for &(p, n) in v {
        if at >= data.len() {
            break;
        }
        let k = n.min(data.len() - at);
        if !p.is_null() {
            unsafe { core::ptr::copy_nonoverlapping(data.as_ptr().add(at), p, k) };
        }
        at += k;
    }
}

pub unsafe fn writev(abi: Abi, s: i32, iov: *const u32, n: i32) -> i32 {
    let Some(v) = (unsafe { iovecs(iov, n) }) else { return fail(EINVAL) };
    let d = unsafe { gather(&v) };
    unsafe { send(abi, s, d.as_ptr(), d.len() as i32, 0) }
}

pub unsafe fn readv(abi: Abi, s: i32, iov: *const u32, n: i32) -> i32 {
    let Some(v) = (unsafe { iovecs(iov, n) }) else { return fail(EINVAL) };
    let total: usize = v.iter().map(|x| x.1).sum();
    let mut d = vec![0u8; total];
    let got = unsafe { recvfrom(abi, s, d.as_mut_ptr(), total as i32, 0, null_mut(), null_mut()) };
    if got > 0 {
        unsafe { scatter(&v, &d[..got as usize]) };
    }
    got
}

/// sendmsg: `msghdr` {name, namelen, iov, iovlen, ...} (both forms
/// start alike).
pub unsafe fn sendmsg(abi: Abi, s: i32, msg: *const u32, flags: i32) -> i32 {
    if msg.is_null() {
        return fail(EFAULT);
    }
    let (name, namelen, iov, iovlen) = unsafe { (*msg as *const u8, *msg.add(1) as i32, *msg.add(2) as *const u32, *msg.add(3) as i32) };
    let Some(v) = (unsafe { iovecs(iov, iovlen) }) else { return fail(EINVAL) };
    let d = unsafe { gather(&v) };
    unsafe { sendto(abi, s, d.as_ptr(), d.len() as i32, flags, name, if name.is_null() { 0 } else { namelen }) }
}

pub unsafe fn recvmsg(abi: Abi, s: i32, msg: *mut u32, flags: i32) -> i32 {
    if msg.is_null() {
        return fail(EFAULT);
    }
    let (name, iov, iovlen) = unsafe { (*msg as *mut u8, *msg.add(2) as *const u32, *msg.add(3) as i32) };
    let Some(v) = (unsafe { iovecs(iov, iovlen) }) else { return fail(EINVAL) };
    let total: usize = v.iter().map(|x| x.1).sum();
    let mut d = vec![0u8; total];
    let mut nl = unsafe { *msg.add(1) as i32 };
    let got = unsafe { recvfrom(abi, s, d.as_mut_ptr(), total as i32, flags, name, &mut nl) };
    if got < 0 {
        return got;
    }
    unsafe {
        scatter(&v, &d[..got as usize]);
        if !name.is_null() {
            *msg.add(1) = nl as u32;
        }
        // No control data / access rights; 4.4: msg_flags = 0.
        *msg.add(5) = 0;
        if abi == Abi::V41 {
            *msg.add(6) = 0;
        }
    }
    got
}

// ---- OS/2 extras ----

pub fn sock_init() -> i32 {
    let mut i = IfInfo::default();
    if unsafe { Rs2NetQueryIface(0, &mut i, core::mem::size_of::<IfInfo>() as u32) } != 0 {
        errno::set(errno::ENETDOWN);
        return 1;
    }
    0
}

pub unsafe fn psock_errno(s: *const u8) {
    let s = unsafe { crate::cstr::bytes(s) };
    let mut v = Vec::new();
    if !s.is_empty() {
        v.extend_from_slice(s);
        v.extend_from_slice(b": ");
    }
    v.extend_from_slice(errno::strerror(errno::get()).as_bytes());
    v.extend_from_slice(b"\r\n");
    let mut n = 0;
    unsafe { os2::DosWrite(2, v.as_ptr(), v.len() as u32, &mut n) };
}

/// sock_strerror: the text, NUL-terminated (static).
pub fn sock_strerror(e: i32) -> *const u8 {
    let t = crate::tls::get();
    unsafe { crate::cstr::copy_out(errno::strerror(e).as_bytes(), t.text.as_mut_ptr(), t.text.len()) };
    t.text.as_ptr()
}

/// gethostid: the primary address, in host order.
pub fn gethostid() -> u32 {
    u32::from_be_bytes(netdb::primary_address().unwrap_or([127, 0, 0, 1]))
}

pub unsafe fn getinetversion(buf: *mut u8) -> i32 {
    if !buf.is_null() {
        unsafe { crate::cstr::copy_out(b"4.3", buf, 8) };
    }
    0
}

/// socketpair: two connected sockets over loopback (stream: TCP,
/// datagram: UDP).
pub unsafe fn socketpair(_domain: i32, typ: i32, _proto: i32, sv: *mut i32) -> i32 {
    if sv.is_null() {
        return fail(EFAULT);
    }
    let lo = SockAddrIn::new([127, 0, 0, 1], 0);
    let sz = core::mem::size_of::<SockAddrIn>() as u32;
    let mut a = 0;
    let mut b = 0;
    let res = (|| -> Result<(i32, i32), u32> {
        unsafe {
            r(Rs2SockCreate(AF_INET, typ as u32, 0, &mut a))?;
            r(Rs2SockBind(a, &lo, sz))?;
            let mut aa = SockAddrIn::default();
            r(Rs2SockGetName(a, 0, &mut aa))?;
            r(Rs2SockCreate(AF_INET, typ as u32, 0, &mut b))?;
            if typ as u32 == SOCK_STREAM {
                r(Rs2SockListen(a, 1))?;
                r(Rs2SockConnect(b, &aa, sz))?;
                let mut c = 0;
                r(Rs2SockAccept(a, null_mut(), &mut c))?;
                Rs2SockClose(a);
                a = 0;
                Ok((c as i32, b as i32))
            } else {
                r(Rs2SockBind(b, &lo, sz))?;
                let mut ba = SockAddrIn::default();
                r(Rs2SockGetName(b, 0, &mut ba))?;
                r(Rs2SockConnect(a, &ba, sz))?;
                r(Rs2SockConnect(b, &aa, sz))?;
                Ok((a as i32, b as i32))
            }
        }
    })();
    match res {
        Ok((x, y)) => {
            unsafe {
                *sv = x;
                *sv.add(1) = y;
            }
            0
        }
        Err(e) => {
            unsafe {
                if a != 0 {
                    Rs2SockClose(a);
                }
                if b != 0 {
                    Rs2SockClose(b);
                }
            }
            fail(e)
        }
    }
}

/// accept_and_recv(s, *accept_s, remote, *remote_len, local, *local_len,
/// buf, len): accept (unless *accept_s already names a socket), the
/// local address, then one receive.
#[allow(clippy::too_many_arguments)]
pub unsafe fn accept_and_recv(abi: Abi, s: i32, acc: *mut i32, remote: *mut u8, rlen: *mut i32, local: *mut u8, llen: *mut i32, buf: *mut u8, len: i32) -> i32 {
    if acc.is_null() {
        return fail(EFAULT);
    }
    unsafe {
        if *acc == -1 || *acc == 0 {
            let n = accept(abi, s, remote, rlen);
            if n < 0 {
                return -1;
            }
            *acc = n;
        }
        if !local.is_null() {
            getname(abi, *acc, false, local, llen);
        }
        if buf.is_null() || len <= 0 {
            return 0;
        }
        recvfrom(abi, *acc, buf, len, 0, null_mut(), null_mut())
    }
}

/// send_file(*s, struct sf_parms *, flags): header, file part, trailer.
pub unsafe fn send_file(abi: Abi, ps: *mut i32, p: *mut u32, flags: i32) -> i32 {
    if ps.is_null() || p.is_null() {
        return fail(EFAULT);
    }
    let s = unsafe { *ps };
    let f = |i: usize| unsafe { *p.add(i) };
    let mut sent: u32 = 0;
    let mut put = |b: *const u8, n: u32| -> bool {
        let mut at = 0;
        while at < n {
            let k = unsafe { send(abi, s, b.add(at as usize), (n - at) as i32, 0) };
            if k <= 0 {
                return false;
            }
            at += k as u32;
            sent += k as u32;
        }
        true
    };
    let ok = (|| {
        if f(0) != 0 && !put(f(0) as *const u8, f(1)) {
            return false;
        }
        let (h, size, off, bytes) = (f(2), f(3), f(4), f(5));
        let want = if bytes == u32::MAX { size.saturating_sub(off) } else { bytes };
        if want > 0 {
            let mut actual = 0;
            if unsafe { os2::DosSetFilePtr(h, off as i32, os2::FILE_BEGIN, &mut actual) } != 0 {
                return false;
            }
            let mut buf = vec![0u8; 32768];
            let mut left = want;
            while left > 0 {
                let mut n = 0;
                if unsafe { os2::DosRead(h, buf.as_mut_ptr(), left.min(buf.len() as u32), &mut n) } != 0 || n == 0 {
                    return false;
                }
                if !put(buf.as_ptr(), n) {
                    return false;
                }
                left -= n;
            }
        }
        if f(6) != 0 && !put(f(6) as *const u8, f(7)) {
            return false;
        }
        true
    })();
    unsafe { *p.add(8) = sent };
    if !ok {
        return -1;
    }
    if flags & 0x800 != 0 {
        soclose(s);
        unsafe { *ps = -1 };
    }
    0
}

/// gettimeofday(tv, tz): the clock (local time, treated as UTC).
pub unsafe fn gettimeofday(tv: *mut i32, tz: *mut i32) -> i32 {
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    let days = days_from_civil(dt.year as i64, dt.month as i64, dt.day as i64);
    let secs = days * 86400 + dt.hours as i64 * 3600 + dt.minutes as i64 * 60 + dt.seconds as i64;
    unsafe {
        if !tv.is_null() {
            *tv = secs as i32;
            *tv.add(1) = dt.hundredths as i32 * 10000;
        }
        if !tz.is_null() {
            *tz = if dt.timezone == -1 { 0 } else { dt.timezone as i32 };
            *tz.add(1) = 0;
        }
    }
    0
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// For calls the stack does not offer.
pub fn unsupported() -> i32 {
    fail(EOPNOTSUPP)
}
