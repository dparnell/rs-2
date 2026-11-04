//! BSD sockets on rs2's kernel socket interface (RS2API 201-218,
//! kernel/src/api/sockets.rs), with Linux's constants translated to the
//! 4.3BSD ones that interface uses. IPv4 only. Also `poll` (sockets
//! through Rs2SockSelect; files are always ready) and `getaddrinfo`
//! (Rs2NetResolve, the system's DNS client).

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use libc::{addrinfo, size_t, sockaddr, socklen_t, ssize_t};

use crate::errno::{fail, fail_bsd};
use crate::file::{Fd, SOCK_BASE, classify};
use crate::spin::SpinLock;
use crate::sys::*;

const MAX_SOCKETS: usize = 4096;
static NONBLOCK: [AtomicBool; MAX_SOCKETS] = [const { AtomicBool::new(false) }; MAX_SOCKETS];

fn sock(fd: c_int) -> Result<u32, c_int> {
    match classify(fd) {
        Some(Fd::Socket(s)) => Ok(s),
        Some(_) => Err(fail(libc::ENOTSOCK)),
        None => Err(fail(libc::EBADF)),
    }
}

macro_rules! sock {
    ($fd:expr) => {
        match sock($fd) {
            Ok(s) => s,
            Err(e) => return e as _,
        }
    };
}

fn check(rc: u32) -> c_int {
    if rc != 0 { fail_bsd(rc) } else { 0 }
}

pub fn is_nonblocking(s: u32) -> bool {
    NONBLOCK.get(s as usize).is_some_and(|b| b.load(Ordering::Relaxed))
}

pub fn set_nonblocking(s: u32, on: bool) -> c_int {
    let mut v = on as u32;
    let rc = unsafe { Rs2SockIoctl(s, 0x8004_667e, &mut v) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    if let Some(b) = NONBLOCK.get(s as usize) {
        b.store(on, Ordering::Relaxed);
    }
    0
}

pub fn nread(s: u32, out: *mut c_int) -> c_int {
    let mut v = 0;
    let rc = unsafe { Rs2SockIoctl(s, 0x4004_667f, &mut v) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    unsafe { *out = v as c_int };
    0
}

/// Duplicates of socket descriptors (`dup`, std's `try_clone`): rs2's
/// sockets have no duplicate handles, so further descriptors from
/// [`ALIAS_BASE`] name the same socket, which is closed with the last one.
pub const ALIAS_BASE: c_int = 0x1_8000;
static ALIASES: SpinLock<Vec<Option<u32>>> = SpinLock::new(Vec::new());
static REFS: [AtomicU32; MAX_SOCKETS] = [const { AtomicU32::new(0) }; MAX_SOCKETS];

pub fn alias(fd: c_int) -> Option<u32> {
    ALIASES.lock().get((fd - ALIAS_BASE) as usize).copied().flatten()
}

pub fn close_socket(fd: c_int, s: u32) -> c_int {
    if fd >= ALIAS_BASE {
        let mut a = ALIASES.lock();
        match a.get_mut((fd - ALIAS_BASE) as usize) {
            Some(e @ Some(_)) => *e = None,
            _ => return fail(libc::EBADF),
        }
    }
    if let Some(r) = REFS.get(s as usize) {
        if r.fetch_sub(1, Ordering::AcqRel) > 1 {
            return 0;
        }
    }
    if let Some(b) = NONBLOCK.get(s as usize) {
        b.store(false, Ordering::Relaxed);
    }
    check(unsafe { Rs2SockClose(s) })
}

pub fn dup_socket(s: u32) -> c_int {
    let mut a = ALIASES.lock();
    let i = match a.iter().position(|e| e.is_none()) {
        Some(i) => {
            a[i] = Some(s);
            i
        }
        None => {
            a.push(Some(s));
            a.len() - 1
        }
    };
    if i >= (crate::file::DIR_BASE - ALIAS_BASE) as usize {
        a[i] = None;
        return fail(libc::EMFILE);
    }
    if let Some(r) = REFS.get(s as usize) {
        r.fetch_add(1, Ordering::AcqRel);
    }
    ALIAS_BASE + i as c_int
}

fn new_socket(s: u32) {
    if let Some(b) = NONBLOCK.get(s as usize) {
        b.store(false, Ordering::Relaxed);
    }
    if let Some(r) = REFS.get(s as usize) {
        r.store(1, Ordering::Release);
    }
}

fn sockaddr_in(addr: *const sockaddr, len: socklen_t) -> Result<SockAddrIn, c_int> {
    if addr.is_null() || (len as usize) < 8 {
        return Err(fail(libc::EINVAL));
    }
    let a = unsafe { &*(addr as *const libc::sockaddr_in) };
    if a.sin_family as c_int != libc::AF_INET {
        return Err(fail(libc::EAFNOSUPPORT));
    }
    Ok(SockAddrIn { family: 2, port: a.sin_port, addr: a.sin_addr.s_addr.to_ne_bytes(), zero: [0; 8] })
}

fn put_addr(a: &SockAddrIn, addr: *mut sockaddr, len: *mut socklen_t) {
    if addr.is_null() || len.is_null() {
        return;
    }
    let mut out: libc::sockaddr_in = unsafe { core::mem::zeroed() };
    out.sin_family = libc::AF_INET as u16;
    out.sin_port = a.port;
    out.sin_addr.s_addr = u32::from_ne_bytes(a.addr);
    let n = (unsafe { *len } as usize).min(core::mem::size_of::<libc::sockaddr_in>());
    unsafe {
        core::ptr::copy_nonoverlapping(&out as *const _ as *const u8, addr as *mut u8, n);
        *len = core::mem::size_of::<libc::sockaddr_in>() as socklen_t;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn socket(domain: c_int, ty: c_int, proto: c_int) -> c_int {
    if domain != libc::AF_INET {
        return fail(libc::EAFNOSUPPORT);
    }
    let base = ty & 0xf;
    let mut s = 0;
    let rc = unsafe { Rs2SockCreate(2, base as u32, proto as u32, &mut s) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    if s as usize >= MAX_SOCKETS {
        unsafe { Rs2SockClose(s) };
        return fail(libc::EMFILE);
    }
    new_socket(s);
    if ty & libc::SOCK_NONBLOCK != 0 {
        set_nonblocking(s, true);
    }
    SOCK_BASE + s as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn socketpair(_d: c_int, _t: c_int, _p: c_int, _sv: *mut c_int) -> c_int {
    fail(libc::EOPNOTSUPP)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn connect(fd: c_int, addr: *const sockaddr, len: socklen_t) -> c_int {
    let s = sock!(fd);
    let a = match sockaddr_in(addr, len) {
        Ok(a) => a,
        Err(e) => return e,
    };
    check(unsafe { Rs2SockConnect(s, &a, 16) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bind(fd: c_int, addr: *const sockaddr, len: socklen_t) -> c_int {
    let s = sock!(fd);
    let a = match sockaddr_in(addr, len) {
        Ok(a) => a,
        Err(e) => return e,
    };
    check(unsafe { Rs2SockBind(s, &a, 16) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn listen(fd: c_int, backlog: c_int) -> c_int {
    let s = sock!(fd);
    check(unsafe { Rs2SockListen(s, backlog.max(1) as u32) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn accept4(fd: c_int, addr: *mut sockaddr, len: *mut socklen_t, flags: c_int) -> c_int {
    let s = sock!(fd);
    let mut peer = SockAddrIn::default();
    let mut ns = 0;
    let rc = unsafe { Rs2SockAccept(s, &mut peer, &mut ns) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    new_socket(ns);
    if flags & libc::SOCK_NONBLOCK != 0 {
        set_nonblocking(ns, true);
    }
    put_addr(&peer, addr, len);
    SOCK_BASE + ns as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn accept(fd: c_int, addr: *mut sockaddr, len: *mut socklen_t) -> c_int {
    unsafe { accept4(fd, addr, len, 0) }
}

fn bsd_flags(flags: c_int) -> u32 {
    let mut f = 0;
    if flags & libc::MSG_PEEK != 0 {
        f |= 2;
    }
    if flags & libc::MSG_OOB != 0 {
        f |= 1;
    }
    if flags & libc::MSG_DONTWAIT != 0 {
        f |= 0x80;
    }
    if flags & libc::MSG_WAITALL != 0 {
        f |= 0x40;
    }
    f
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn send(fd: c_int, buf: *const c_void, n: size_t, flags: c_int) -> ssize_t {
    let s = sock!(fd);
    let mut sent = 0;
    let rc = unsafe { Rs2SockSend(s, buf as *const u8, n as u32, bsd_flags(flags), &mut sent) };
    if rc != 0 {
        return fail_bsd(rc) as ssize_t;
    }
    sent as ssize_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sendto(fd: c_int, buf: *const c_void, n: size_t, flags: c_int, to: *const sockaddr, len: socklen_t) -> ssize_t {
    if to.is_null() {
        return unsafe { send(fd, buf, n, flags) };
    }
    let s = sock!(fd);
    let a = match sockaddr_in(to, len) {
        Ok(a) => a,
        Err(e) => return e as ssize_t,
    };
    let mut sent = 0;
    let rc = unsafe { Rs2SockSendTo(s, buf as *const u8, n as u32, bsd_flags(flags), &a, &mut sent) };
    if rc != 0 {
        return fail_bsd(rc) as ssize_t;
    }
    sent as ssize_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn recv(fd: c_int, buf: *mut c_void, n: size_t, flags: c_int) -> ssize_t {
    let s = sock!(fd);
    let mut got = 0;
    let rc = unsafe { Rs2SockRecv(s, buf as *mut u8, n as u32, bsd_flags(flags), &mut got) };
    if rc != 0 {
        return fail_bsd(rc) as ssize_t;
    }
    got as ssize_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn recvfrom(fd: c_int, buf: *mut c_void, n: size_t, flags: c_int, from: *mut sockaddr, len: *mut socklen_t) -> ssize_t {
    let s = sock!(fd);
    let mut got = 0;
    let mut a = SockAddrIn::default();
    let rc = unsafe { Rs2SockRecvFrom(s, buf as *mut u8, n as u32, bsd_flags(flags), &mut a, &mut got) };
    if rc != 0 {
        return fail_bsd(rc) as ssize_t;
    }
    put_addr(&a, from, len);
    got as ssize_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sendmsg(fd: c_int, msg: *const libc::msghdr, flags: c_int) -> ssize_t {
    let m = unsafe { &*msg };
    let mut total = 0;
    for i in 0..m.msg_iovlen as usize {
        let v = unsafe { &*m.msg_iov.add(i) };
        let n = unsafe { sendto(fd, v.iov_base, v.iov_len, flags, m.msg_name as *const sockaddr, m.msg_namelen) };
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
    }
    total
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn recvmsg(fd: c_int, msg: *mut libc::msghdr, flags: c_int) -> ssize_t {
    let m = unsafe { &mut *msg };
    m.msg_controllen = 0;
    m.msg_flags = 0;
    if m.msg_iovlen == 0 {
        return 0;
    }
    let v = unsafe { &*m.msg_iov };
    let mut len = m.msg_namelen;
    let n = unsafe { recvfrom(fd, v.iov_base, v.iov_len, flags, m.msg_name as *mut sockaddr, &mut len) };
    m.msg_namelen = len;
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn shutdown(fd: c_int, how: c_int) -> c_int {
    let s = sock!(fd);
    check(unsafe { Rs2SockShutdown(s, how as u32) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getsockname(fd: c_int, addr: *mut sockaddr, len: *mut socklen_t) -> c_int {
    let s = sock!(fd);
    let mut a = SockAddrIn::default();
    let rc = unsafe { Rs2SockGetName(s, 0, &mut a) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    put_addr(&a, addr, len);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpeername(fd: c_int, addr: *mut sockaddr, len: *mut socklen_t) -> c_int {
    let s = sock!(fd);
    let mut a = SockAddrIn::default();
    let rc = unsafe { Rs2SockGetName(s, 1, &mut a) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    put_addr(&a, addr, len);
    0
}

/// Linux (level, option) to BSD's.
fn bsd_opt(level: c_int, opt: c_int) -> Option<(u32, u32)> {
    Some(match (level, opt) {
        (libc::SOL_SOCKET, libc::SO_REUSEADDR) => (0xffff, 0x0004),
        (libc::SOL_SOCKET, libc::SO_KEEPALIVE) => (0xffff, 0x0008),
        (libc::SOL_SOCKET, libc::SO_BROADCAST) => (0xffff, 0x0020),
        (libc::SOL_SOCKET, libc::SO_LINGER) => (0xffff, 0x0080),
        (libc::SOL_SOCKET, libc::SO_SNDBUF) => (0xffff, 0x1001),
        (libc::SOL_SOCKET, libc::SO_RCVBUF) => (0xffff, 0x1002),
        (libc::SOL_SOCKET, libc::SO_SNDTIMEO) => (0xffff, 0x1005),
        (libc::SOL_SOCKET, libc::SO_RCVTIMEO) => (0xffff, 0x1006),
        (libc::SOL_SOCKET, libc::SO_ERROR) => (0xffff, 0x1007),
        (libc::SOL_SOCKET, libc::SO_TYPE) => (0xffff, 0x1008),
        (libc::IPPROTO_TCP, libc::TCP_NODELAY) => (6, 1),
        (libc::IPPROTO_IP, libc::IP_TTL) => (0, 4),
        (libc::IPPROTO_IP, libc::IP_MULTICAST_TTL) => (0, 10),
        (libc::IPPROTO_IP, libc::IP_MULTICAST_LOOP) => (0, 11),
        (libc::IPPROTO_IP, libc::IP_ADD_MEMBERSHIP) => (0, 12),
        (libc::IPPROTO_IP, libc::IP_DROP_MEMBERSHIP) => (0, 13),
        _ => return None,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setsockopt(fd: c_int, level: c_int, opt: c_int, val: *const c_void, len: socklen_t) -> c_int {
    let s = sock!(fd);
    let Some((l, o)) = bsd_opt(level, opt) else { return fail(libc::ENOPROTOOPT) };
    let rc = unsafe { Rs2SockSetOpt(s, l, o, val as *const u8, len) };
    // Options the stack does not keep (buffer sizes, multicast...) are
    // accepted: callers treat them as hints.
    if rc == 42 { 0 } else { check(rc) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getsockopt(fd: c_int, level: c_int, opt: c_int, val: *mut c_void, len: *mut socklen_t) -> c_int {
    let s = sock!(fd);
    let Some((l, o)) = bsd_opt(level, opt) else { return fail(libc::ENOPROTOOPT) };
    let mut cb = unsafe { *len };
    let rc = unsafe { Rs2SockGetOpt(s, l, o, val as *mut u8, &mut cb) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    unsafe {
        if l == 0xffff && o == 0x1007 && cb >= 4 {
            // SO_ERROR holds a BSD errno.
            let e = *(val as *const u32);
            *(val as *mut c_int) = if e == 0 { 0 } else { crate::errno::from_bsd(e) };
        }
        if l == 0xffff && o == 0x1008 && cb >= 4 {
            // SOCK_STREAM/DGRAM have the same numbers.
        }
        *len = cb;
    }
    0
}

// ---- poll ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn poll(fds: *mut libc::pollfd, n: libc::nfds_t, timeout: c_int) -> c_int {
    let fds = unsafe { core::slice::from_raw_parts_mut(fds, n as usize) };
    let mut ready = 0;
    let mut reads: Vec<u32> = Vec::new();
    let mut writes: Vec<u32> = Vec::new();
    for p in fds.iter_mut() {
        p.revents = 0;
        match classify(p.fd) {
            _ if p.fd < 0 => {}
            Some(Fd::File(h)) => {
                if !crate::file::handle_valid(h) {
                    p.revents = libc::POLLNVAL;
                } else {
                    p.revents = p.events & (libc::POLLIN | libc::POLLOUT);
                }
                if p.revents != 0 {
                    ready += 1;
                }
            }
            Some(Fd::Dir(_)) => {
                p.revents = libc::POLLNVAL;
                ready += 1;
            }
            Some(Fd::Socket(s)) => {
                if p.events & (libc::POLLIN | libc::POLLPRI) != 0 {
                    reads.push(s);
                }
                if p.events & libc::POLLOUT != 0 {
                    writes.push(s);
                }
            }
            None => {}
        }
    }
    if reads.is_empty() && writes.is_empty() {
        if ready == 0 && timeout != 0 {
            let ms = if timeout < 0 { u32::MAX } else { timeout as u32 };
            unsafe { DosSleep(ms) };
        }
        return ready;
    }
    let nr = reads.len();
    let nw = writes.len();
    let mut socks = reads;
    socks.extend_from_slice(&writes);
    let ms = if ready > 0 { 0 } else { timeout };
    let mut count = 0;
    let rc = unsafe { Rs2SockSelect(socks.as_mut_ptr(), nr as u32, nw as u32, 0, ms, &mut count) };
    if rc != 0 {
        return fail_bsd(rc);
    }
    let (rs, ws) = socks.split_at(nr);
    for p in fds.iter_mut() {
        if let Some(Fd::Socket(s)) = classify(p.fd) {
            if p.events & (libc::POLLIN | libc::POLLPRI) != 0 && rs.contains(&s) {
                p.revents |= libc::POLLIN;
            }
            if p.events & libc::POLLOUT != 0 && ws.contains(&s) {
                p.revents |= libc::POLLOUT;
            }
            if p.revents != 0 {
                ready += 1;
            }
        }
    }
    let _ = nw;
    ready
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn select(
    _n: c_int,
    _r: *mut libc::fd_set,
    _w: *mut libc::fd_set,
    _e: *mut libc::fd_set,
    _t: *mut libc::timeval,
) -> c_int {
    fail(libc::ENOSYS)
}

// ---- Name resolution ----

fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = s.split(|&c| c == b'.');
    for o in out.iter_mut() {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || !p.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let v: u32 = p.iter().fold(0, |a, &c| a * 10 + (c - b'0') as u32);
        if v > 255 {
            return None;
        }
        *o = v as u8;
    }
    if parts.next().is_some() { None } else { Some(out) }
}

#[repr(C)]
struct AddrInfoBlock {
    ai: addrinfo,
    sa: libc::sockaddr_in,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getaddrinfo(
    node: *const c_char,
    service: *const c_char,
    hints: *const addrinfo,
    res: *mut *mut addrinfo,
) -> c_int {
    let (family, socktype, protocol, flags) = if hints.is_null() {
        (0, 0, 0, 0)
    } else {
        unsafe { ((*hints).ai_family, (*hints).ai_socktype, (*hints).ai_protocol, (*hints).ai_flags) }
    };
    if family != libc::AF_UNSPEC && family != libc::AF_INET {
        return libc::EAI_FAMILY;
    }
    let port: u16 = if service.is_null() {
        0
    } else {
        let s = unsafe { crate::string::cstr(service) };
        if s.is_empty() || !s.iter().all(u8::is_ascii_digit) || s.len() > 5 {
            return libc::EAI_SERVICE;
        }
        let v = s.iter().fold(0u32, |a, &c| a * 10 + (c - b'0') as u32);
        if v > 65535 {
            return libc::EAI_SERVICE;
        }
        v as u16
    };
    let mut addrs: Vec<[u8; 4]> = Vec::new();
    if node.is_null() {
        addrs.push(if flags & libc::AI_PASSIVE != 0 { [0; 4] } else { [127, 0, 0, 1] });
    } else {
        let name = unsafe { crate::string::cstr(node) };
        if let Some(a) = parse_ipv4(name) {
            addrs.push(a);
        } else if flags & libc::AI_NUMERICHOST != 0 {
            return libc::EAI_NONAME;
        } else if name.eq_ignore_ascii_case(b"localhost") {
            addrs.push([127, 0, 0, 1]);
        } else {
            let mut buf = [[0u8; 4]; 8];
            let mut count = 0;
            let rc = unsafe { Rs2NetResolve(node as *const u8, buf.as_mut_ptr(), 8, &mut count) };
            match rc {
                0 => addrs.extend_from_slice(&buf[..(count as usize).min(8)]),
                2 => return libc::EAI_AGAIN,
                _ => return libc::EAI_NONAME,
            }
            if addrs.is_empty() {
                return libc::EAI_NONAME;
            }
        }
    }
    let types: &[c_int] = if socktype == 0 { &[libc::SOCK_STREAM, libc::SOCK_DGRAM] } else { core::slice::from_ref(&socktype) };
    let mut head: *mut addrinfo = core::ptr::null_mut();
    let mut tail: *mut *mut addrinfo = &mut head;
    for a in &addrs {
        for &t in types {
            unsafe {
                let b = crate::malloc::calloc(1, core::mem::size_of::<AddrInfoBlock>()) as *mut AddrInfoBlock;
                if b.is_null() {
                    freeaddrinfo(head);
                    return libc::EAI_MEMORY;
                }
                (*b).sa.sin_family = libc::AF_INET as u16;
                (*b).sa.sin_port = port.to_be();
                (*b).sa.sin_addr.s_addr = u32::from_ne_bytes(*a);
                (*b).ai.ai_family = libc::AF_INET;
                (*b).ai.ai_socktype = t;
                (*b).ai.ai_protocol = if protocol != 0 { protocol } else if t == libc::SOCK_STREAM { libc::IPPROTO_TCP } else { libc::IPPROTO_UDP };
                (*b).ai.ai_addrlen = core::mem::size_of::<libc::sockaddr_in>() as socklen_t;
                (*b).ai.ai_addr = &mut (*b).sa as *mut _ as *mut sockaddr;
                *tail = &mut (*b).ai;
                tail = &mut (*b).ai.ai_next;
            }
        }
    }
    unsafe { *res = head };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn freeaddrinfo(mut ai: *mut addrinfo) {
    while !ai.is_null() {
        let next = unsafe { (*ai).ai_next };
        unsafe { crate::malloc::free(ai as *mut c_void) };
        ai = next;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __res_init() -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn htons(v: u16) -> u16 {
    v.to_be()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ntohs(v: u16) -> u16 {
    u16::from_be(v)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn htonl(v: u32) -> u32 {
    v.to_be()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ntohl(v: u32) -> u32 {
    u32::from_be(v)
}
