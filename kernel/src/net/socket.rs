//! Sockets for processes: the kernel-internal socket API that the gate
//! (`api::sockets`, RS2API ordinals 200+) and later OS/2's socket DLLs
//! sit on.
//!
//! Every process has its own table of socket handles (small numbers from
//! [`FIRST_HANDLE`]), each naming a socket of the stack plus per-handle
//! options (non-blocking mode, timeouts). Calls return BSD error numbers
//! (`tcpip::Error::errno`; OS/2's SOCBASEERR offset is the DLL's
//! business). Blocking calls retry the stack operation whenever
//! [`super::EVENTS`] moves, sleeping on [`super::WAIT`] in between; they
//! end early with EINTR when the thread is killed or signalled, and with
//! EWOULDBLOCK when a SO_RCVTIMEO/SO_SNDTIMEO timeout expires. A
//! process's sockets are closed when it ends.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use tcpip::stack::{Error, Kind, Ready};
use tcpip::{Endpoint, Ipv4, Stack};

use crate::sync::SpinLock;

pub const FIRST_HANDLE: u32 = 3;

// Address family, socket types, protocols.
pub const AF_INET: u32 = 2;
pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;
pub const SOCK_RAW: u32 = 3;
pub const IPPROTO_IP: u32 = 0;
pub const IPPROTO_ICMP: u32 = 1;
pub const IPPROTO_TCP: u32 = 6;
pub const IPPROTO_UDP: u32 = 17;

// Message flags.
pub const MSG_OOB: u32 = 1;
pub const MSG_PEEK: u32 = 2;
pub const MSG_DONTWAIT: u32 = 0x80;

// errno values the socket layer itself produces.
pub const EBADF: u32 = 9;
pub const EINTR: u32 = 4;
pub const EINVAL: u32 = 22;
pub const EWOULDBLOCK: u32 = 35;
pub const EINPROGRESS: u32 = 36;
pub const ENOTSOCK: u32 = 38;
pub const ENOPROTOOPT: u32 = 42;
pub const EPROTONOSUPPORT: u32 = 43;
pub const ESOCKTNOSUPPORT: u32 = 44;
pub const EOPNOTSUPP: u32 = 45;
pub const EAFNOSUPPORT: u32 = 47;
pub const ENETDOWN: u32 = 50;
pub const EMFILE: u32 = 24;

/// Sockets per process.
const MAX_SOCKETS: usize = 2048;
/// Time a name lookup may take.
const RESOLVE_TIMEOUT_MS: u64 = 12_000;

#[derive(Clone, Copy)]
struct Handle {
    id: usize,
    kind: Kind,
    nonblock: bool,
    rcv_timeout: Option<u64>,
    snd_timeout: Option<u64>,
    linger: Option<u32>,
    keepalive: bool,
    oobinline: bool,
}

#[derive(Default)]
struct Table {
    map: BTreeMap<u32, Handle>,
}

static TABLES: SpinLock<BTreeMap<u32, Table>> = SpinLock::new(BTreeMap::new());

pub type Result<T> = core::result::Result<T, u32>;

fn get(pid: u32, s: u32) -> Result<Handle> {
    TABLES.lock().get(&pid).and_then(|t| t.map.get(&s).copied()).ok_or(ENOTSOCK)
}

fn update(pid: u32, s: u32, f: impl FnOnce(&mut Handle)) -> Result<()> {
    let mut g = TABLES.lock();
    let h = g.get_mut(&pid).and_then(|t| t.map.get_mut(&s)).ok_or(ENOTSOCK)?;
    f(h);
    Ok(())
}

fn insert(pid: u32, id: usize, kind: Kind) -> Result<u32> {
    let mut g = TABLES.lock();
    let t = g.entry(pid).or_default();
    if t.map.len() >= MAX_SOCKETS {
        return Err(EMFILE);
    }
    let mut n = FIRST_HANDLE;
    while t.map.contains_key(&n) {
        n += 1;
    }
    t.map.insert(
        n,
        Handle { id, kind, nonblock: false, rcv_timeout: None, snd_timeout: None, linger: None, keepalive: false, oobinline: false },
    );
    Ok(n)
}

fn stack<R>(f: impl FnOnce(&mut Stack, u64) -> R) -> Result<R> {
    super::with_stack(f).ok_or(ENETDOWN)
}

/// Run `op` until it does not say WouldBlock (or at once when
/// `nonblock`), waiting for stack events in between.
fn blocking<T>(nonblock: bool, timeout_ms: Option<u64>, mut op: impl FnMut(&mut Stack, u64) -> core::result::Result<T, Error>) -> Result<T> {
    let deadline = timeout_ms.filter(|&t| t > 0).map(|t| crate::irq::ticks() + t * crate::irq::HZ / 1000);
    loop {
        let seen = super::EVENTS.load(Ordering::Acquire);
        match stack(&mut op)? {
            Ok(v) => return Ok(v),
            Err(Error::WouldBlock) if !nonblock => {
                if !super::WAIT.wait_until(deadline, || super::EVENTS.load(Ordering::Acquire) != seen) {
                    if deadline.is_some_and(|d| crate::irq::ticks() >= d) {
                        return Err(EWOULDBLOCK);
                    }
                    let t = crate::task::sched::current();
                    if t.exit_requested() || t.interrupted() {
                        return Err(EINTR);
                    }
                }
            }
            Err(e) => return Err(e.errno()),
        }
    }
}

/// socket(): a new socket for process `pid`.
pub fn create(pid: u32, domain: u32, typ: u32, proto: u32) -> Result<u32> {
    if domain != AF_INET {
        return Err(EAFNOSUPPORT);
    }
    let kind = match (typ, proto) {
        (SOCK_STREAM, IPPROTO_IP | IPPROTO_TCP) => Kind::Tcp,
        (SOCK_DGRAM, IPPROTO_IP | IPPROTO_UDP) => Kind::Udp,
        (SOCK_STREAM | SOCK_DGRAM, _) => return Err(EPROTONOSUPPORT),
        (SOCK_RAW, IPPROTO_ICMP) => Kind::Raw(IPPROTO_ICMP as u8),
        (SOCK_RAW, p) if p != 0 && p < 256 && p != IPPROTO_TCP && p != IPPROTO_UDP => Kind::Raw(p as u8),
        (SOCK_RAW, _) => return Err(EPROTONOSUPPORT),
        _ => return Err(ESOCKTNOSUPPORT),
    };
    let id = stack(|st, _| st.socket(kind))?.map_err(Error::errno)?;
    match insert(pid, id, kind) {
        Ok(h) => Ok(h),
        Err(e) => {
            let _ = stack(|st, now| st.close(id, now));
            Err(e)
        }
    }
}

pub fn bind(pid: u32, s: u32, ep: Endpoint) -> Result<()> {
    let h = get(pid, s)?;
    stack(|st, _| st.bind(h.id, ep))?.map_err(Error::errno)
}

pub fn listen(pid: u32, s: u32, backlog: u32) -> Result<()> {
    let h = get(pid, s)?;
    stack(|st, _| st.listen(h.id, backlog as usize))?.map_err(Error::errno)
}

/// accept(): the new socket and the peer's address.
pub fn accept(pid: u32, s: u32) -> Result<(u32, Endpoint)> {
    let h = get(pid, s)?;
    if h.kind != Kind::Tcp {
        return Err(EOPNOTSUPP);
    }
    let (id, peer) = blocking(h.nonblock, h.rcv_timeout, |st, _| st.accept(h.id))?;
    match insert(pid, id, Kind::Tcp) {
        Ok(n) => {
            let _ = update(pid, n, |x| x.keepalive = h.keepalive);
            Ok((n, peer))
        }
        Err(e) => {
            let _ = stack(|st, now| st.close(id, now));
            Err(e)
        }
    }
}

/// connect(): blocking sockets wait for the connection (or its failure);
/// non-blocking ones get EINPROGRESS and select() for writing.
pub fn connect(pid: u32, s: u32, ep: Endpoint) -> Result<()> {
    let h = get(pid, s)?;
    stack(|st, now| st.connect(h.id, ep, now))?.map_err(Error::errno)?;
    if h.kind != Kind::Tcp {
        return Ok(());
    }
    if h.nonblock {
        return match stack(|st, _| st.connect_status(h.id))? {
            Ok(true) => Ok(()),
            Ok(false) => Err(EINPROGRESS),
            Err(e) => Err(e.errno()),
        };
    }
    blocking(false, None, |st, _| match st.connect_status(h.id) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Error::WouldBlock),
        Err(e) => Err(e),
    })
}

fn nonblock(h: &Handle, flags: u32) -> bool {
    h.nonblock || flags & MSG_DONTWAIT != 0
}

/// send()/sendto(): a blocking stream send returns once everything is
/// queued; a datagram goes out whole or not at all.
pub fn send(pid: u32, s: u32, data: &[u8], flags: u32, to: Option<Endpoint>) -> Result<usize> {
    let h = get(pid, s)?;
    if flags & MSG_OOB != 0 && h.kind != Kind::Tcp {
        return Err(EOPNOTSUPP);
    }
    let nb = nonblock(&h, flags);
    if h.kind != Kind::Tcp {
        return blocking(nb, h.snd_timeout, |st, now| match to {
            Some(ep) => st.send_to(h.id, data, ep, now),
            None => st.send(h.id, data, now),
        });
    }
    let mut done = 0;
    loop {
        let r = blocking(nb || done > 0, h.snd_timeout, |st, now| st.send(h.id, &data[done..], now));
        match r {
            Ok(n) => done += n,
            Err(EWOULDBLOCK) if done > 0 && !nb => {
                // Partly queued: wait for room for the rest.
                let r = blocking(false, h.snd_timeout, |st, _| match st.ready(h.id) {
                    Ok(Ready { write: true, .. }) => Ok(()),
                    Ok(_) => Err(Error::WouldBlock),
                    Err(e) => Err(e),
                });
                if r.is_err() {
                    return Ok(done);
                }
            }
            Err(e) if done == 0 => return Err(e),
            Err(_) => return Ok(done),
        }
        if done >= data.len() || nb {
            return Ok(done);
        }
    }
}

/// recv()/recvfrom(): up to `len` bytes and the sender.
pub fn recv(pid: u32, s: u32, len: usize, flags: u32) -> Result<(Vec<u8>, Endpoint)> {
    let h = get(pid, s)?;
    let mut buf = alloc::vec![0u8; len];
    let peek = flags & MSG_PEEK != 0;
    let (n, from) = blocking(nonblock(&h, flags), h.rcv_timeout, |st, now| st.recv_from(h.id, &mut buf, peek, now))?;
    buf.truncate(n);
    Ok((buf, from))
}

pub fn shutdown(pid: u32, s: u32, how: u32) -> Result<()> {
    let h = get(pid, s)?;
    stack(|st, now| st.shutdown(h.id, how, now))?.map_err(Error::errno)
}

/// soclose(): the handle goes at once; a TCP connection finishes in the
/// background (or is reset with SO_LINGER and a zero timeout).
pub fn close(pid: u32, s: u32) -> Result<()> {
    let h = {
        let mut g = TABLES.lock();
        let t = g.get_mut(&pid).ok_or(ENOTSOCK)?;
        t.map.remove(&s).ok_or(ENOTSOCK)?
    };
    stack(|st, now| if h.linger == Some(0) { st.abort(h.id, now) } else { st.close(h.id, now) })?.map_err(Error::errno)
}

/// select() in OS/2's form: `socks` holds the read, then write, then
/// exception sockets; entries that are not ready become `u32::MAX`.
/// `timeout_ms` < 0 waits forever. Returns the number ready.
pub fn select(pid: u32, socks: &mut [u32], nread: usize, nwrite: usize, timeout_ms: i32) -> Result<u32> {
    let mut ids = Vec::with_capacity(socks.len());
    for &s in socks.iter() {
        ids.push(get(pid, s)?.id);
    }
    let check = |st: &mut Stack| -> core::result::Result<Vec<bool>, Error> {
        let mut v = Vec::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            let r = st.ready(id)?;
            v.push(if i < nread {
                r.read
            } else if i < nread + nwrite {
                r.write
            } else {
                r.except
            });
        }
        Ok(v)
    };
    let timeout = if timeout_ms < 0 { None } else { Some(timeout_ms as u64) };
    let res = blocking(timeout == Some(0), timeout, |st, _| {
        let v = check(st)?;
        if v.iter().any(|&b| b) || ids.is_empty() && timeout.is_none() {
            Ok(v)
        } else {
            Err(Error::WouldBlock)
        }
    });
    let ready = match res {
        Ok(v) => v,
        // Timed out (or a zero timeout): nothing is ready.
        Err(EWOULDBLOCK) => alloc::vec![false; ids.len()],
        Err(e) => return Err(e),
    };
    let mut n = 0;
    for (s, r) in socks.iter_mut().zip(ready) {
        if r {
            n += 1;
        } else {
            *s = u32::MAX;
        }
    }
    Ok(n)
}

// ioctl commands.
pub const FIONBIO: u32 = 0x8004_667e;
pub const FIONREAD: u32 = 0x4004_667f;

/// ioctl(): FIONBIO (`arg` 0 or 1) and FIONREAD (returns the count).
pub fn ioctl(pid: u32, s: u32, cmd: u32, arg: u32) -> Result<u32> {
    let h = get(pid, s)?;
    match cmd {
        FIONBIO => update(pid, s, |x| x.nonblock = arg != 0).map(|_| 0),
        FIONREAD => Ok(stack(|st, _| st.available(h.id))?.map_err(Error::errno)? as u32),
        _ => Err(EINVAL),
    }
}

// Socket options (level SOL_SOCKET).
pub const SOL_SOCKET: u32 = 0xffff;
pub const SO_ACCEPTCONN: u32 = 0x0002;
pub const SO_REUSEADDR: u32 = 0x0004;
pub const SO_KEEPALIVE: u32 = 0x0008;
pub const SO_DONTROUTE: u32 = 0x0010;
pub const SO_BROADCAST: u32 = 0x0020;
pub const SO_LINGER: u32 = 0x0080;
pub const SO_OOBINLINE: u32 = 0x0100;
pub const SO_SNDBUF: u32 = 0x1001;
pub const SO_RCVBUF: u32 = 0x1002;
pub const SO_SNDLOWAT: u32 = 0x1003;
pub const SO_RCVLOWAT: u32 = 0x1004;
pub const SO_SNDTIMEO: u32 = 0x1005;
pub const SO_RCVTIMEO: u32 = 0x1006;
pub const SO_ERROR: u32 = 0x1007;
pub const SO_TYPE: u32 = 0x1008;
pub const TCP_NODELAY: u32 = 1;

/// setsockopt(): `val` is the option's bytes (int, struct linger, or
/// struct timeval).
pub fn set_option(pid: u32, s: u32, level: u32, opt: u32, val: &[u8]) -> Result<()> {
    let h = get(pid, s)?;
    let int = |o: usize| val.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(EINVAL);
    match (level, opt) {
        (SOL_SOCKET, SO_REUSEADDR) => {
            let on = int(0)? != 0;
            stack(|st, _| st.set_reuse_addr(h.id, on))?.map_err(Error::errno)
        }
        (SOL_SOCKET, SO_BROADCAST) => {
            let on = int(0)? != 0;
            stack(|st, _| st.set_broadcast(h.id, on))?.map_err(Error::errno)
        }
        (SOL_SOCKET, SO_SNDBUF) | (SOL_SOCKET, SO_RCVBUF) => {
            let n = int(0)? as usize;
            let (snd, rcv) = if opt == SO_SNDBUF { (Some(n), None) } else { (None, Some(n)) };
            stack(|st, _| st.set_buffers(h.id, snd, rcv))?.map_err(Error::errno)
        }
        (SOL_SOCKET, SO_KEEPALIVE) => {
            let on = int(0)? != 0;
            update(pid, s, |x| x.keepalive = on)
        }
        (SOL_SOCKET, SO_OOBINLINE) => {
            let on = int(0)? != 0;
            update(pid, s, |x| x.oobinline = on)
        }
        (SOL_SOCKET, SO_LINGER) => {
            let (on, secs) = (int(0)?, int(4)?);
            update(pid, s, |x| x.linger = (on != 0).then_some(secs))
        }
        (SOL_SOCKET, SO_SNDTIMEO) | (SOL_SOCKET, SO_RCVTIMEO) => {
            let ms = int(0)? as u64 * 1000 + int(4)? as u64 / 1000;
            let t = (ms > 0).then_some(ms);
            update(pid, s, |x| if opt == SO_SNDTIMEO { x.snd_timeout = t } else { x.rcv_timeout = t })
        }
        (SOL_SOCKET, SO_DONTROUTE | SO_SNDLOWAT | SO_RCVLOWAT) => Ok(()),
        (IPPROTO_TCP, TCP_NODELAY) if h.kind == Kind::Tcp => Ok(()),
        _ => Err(ENOPROTOOPT),
    }
}

/// getsockopt(): the option's bytes.
pub fn get_option(pid: u32, s: u32, level: u32, opt: u32) -> Result<Vec<u8>> {
    let h = get(pid, s)?;
    let int = |v: u32| v.to_le_bytes().to_vec();
    let tv = |t: Option<u64>| {
        let ms = t.unwrap_or(0);
        let mut v = ((ms / 1000) as u32).to_le_bytes().to_vec();
        v.extend_from_slice(&(((ms % 1000) * 1000) as u32).to_le_bytes());
        v
    };
    Ok(match (level, opt) {
        (SOL_SOCKET, SO_TYPE) => int(match h.kind {
            Kind::Tcp => SOCK_STREAM,
            Kind::Udp => SOCK_DGRAM,
            Kind::Raw(_) => SOCK_RAW,
        }),
        (SOL_SOCKET, SO_ERROR) => int(stack(|st, _| st.take_error(h.id))?.map_err(Error::errno)?.map_or(0, Error::errno)),
        (SOL_SOCKET, SO_ACCEPTCONN) => int(stack(|st, _| st.is_listening(h.id))?.map_err(Error::errno)? as u32),
        (SOL_SOCKET, SO_REUSEADDR) | (SOL_SOCKET, SO_BROADCAST) => {
            let (r, b) = stack(|st, _| st.options(h.id))?.map_err(Error::errno)?;
            int(if opt == SO_REUSEADDR { r } else { b } as u32)
        }
        (SOL_SOCKET, SO_SNDBUF) | (SOL_SOCKET, SO_RCVBUF) => {
            let (snd, rcv) = stack(|st, _| st.buffers(h.id))?.map_err(Error::errno)?;
            int(if opt == SO_SNDBUF { snd } else { rcv } as u32)
        }
        (SOL_SOCKET, SO_KEEPALIVE) => int(h.keepalive as u32),
        (SOL_SOCKET, SO_OOBINLINE) => int(h.oobinline as u32),
        (SOL_SOCKET, SO_LINGER) => {
            let mut v = int(h.linger.is_some() as u32);
            v.extend_from_slice(&h.linger.unwrap_or(0).to_le_bytes());
            v
        }
        (SOL_SOCKET, SO_SNDTIMEO) => tv(h.snd_timeout),
        (SOL_SOCKET, SO_RCVTIMEO) => tv(h.rcv_timeout),
        (SOL_SOCKET, SO_SNDLOWAT) | (SOL_SOCKET, SO_RCVLOWAT) => int(1),
        (IPPROTO_TCP, TCP_NODELAY) if h.kind == Kind::Tcp => int(0),
        _ => return Err(ENOPROTOOPT),
    })
}

/// getsockname() / getpeername().
pub fn name(pid: u32, s: u32, peer: bool) -> Result<Endpoint> {
    let h = get(pid, s)?;
    stack(|st, _| if peer { st.peer_endpoint(h.id) } else { st.local_endpoint(h.id) })?.map_err(Error::errno)
}

/// Resolve a host name to IPv4 addresses; the error is an h_errno value
/// (1 HOST_NOT_FOUND, 2 TRY_AGAIN, 4 NO_DATA).
pub fn resolve(name: &str) -> core::result::Result<Vec<Ipv4>, u32> {
    let q = stack(|st, now| st.resolve_start(name, now)).map_err(|_| 2u32)?.map_err(|e| e.h_errno())?;
    let r = blocking(false, Some(RESOLVE_TIMEOUT_MS), |st, _| match st.resolve_result(q) {
        Some(r) => Ok(r),
        None => Err(Error::WouldBlock),
    });
    match r {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.h_errno()),
        Err(_) => {
            let _ = stack(|st, _| st.resolve_cancel(q));
            Err(2)
        }
    }
}

/// The process ended: close its sockets.
pub fn process_ended(pid: u32) {
    let Some(t) = TABLES.lock().remove(&pid) else { return };
    let _ = stack(|st, now| {
        for h in t.map.values() {
            let _ = st.close(h.id, now);
        }
    });
}

/// Sockets a process has open.
pub fn count(pid: u32) -> usize {
    TABLES.lock().get(&pid).map_or(0, |t| t.map.len())
}
