//! TCPIPDLL.DLL: the socket and network database calls for 16-bit OS/2
//! programs (TFTP, INETD, LPD, TELNETD, BOOTP and the 16-bit RPC DLL
//! import them by name: `_socket`, `_gethostbyname`, ...). They are
//! Microsoft C `_cdecl far` functions with 16-bit `int`s and far
//! pointers; `thunk16` brings each call into 32-bit code, where the
//! arguments are widened and the work done by `tcpipcore` (the 4.0
//! structure layouts, which 16-bit `sockaddr`s share). Results that hold
//! pointers (`hostent`, `servent`, `protoent`, `inet_ntoa`'s text) are
//! rebuilt with 16:16 pointers in per-thread buffers of the DLL's data.
//!
//! `_alarm(seconds)` and `_xsignal(sig, handler)` give 16-bit programs the
//! BSD alarm signal (IBM's TFTP times its retransmissions with them): a
//! timer thread watches the deadline; when it passes, it cancels the
//! socket call the alarm's thread is blocked in (so_cancel), and that
//! thread, on its way out of the DLL (or into its next call), calls the
//! handler on its own 16-bit stack, as a signal would arrive. The handler
//! may longjmp out (thunk16 lets frames be abandoned).

#![no_std]
#![no_main]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
extern crate os2;

use core::ptr::null_mut;

use tcpipcore::netdb::{self, Hostent, Protoent, Servent};
use tcpipcore::sock::{self, Abi};
use tcpipcore::{cstr, errno, inet};
use thunk16::{far, Args};

const ABI: Abi = Abi::V40;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// `_h_errno`: the 16-bit programs' h_errno variable.
#[unsafe(no_mangle)]
pub static mut tcpipdll_h_errno: i16 = 0;

thunk16::entries16! {
    dispatch = dispatch;
    t16_accept = 1;
    t16_addsockettolist = 2;
    t16_alarm = 3;
    t16_bind = 4;
    t16_bswap = 5;
    t16_cleanupsockets = 6;
    t16_connect = 7;
    t16_dn_comp = 9;
    t16_dn_expand = 10;
    t16_dn_skipname = 11;
    t16_endhostent = 12;
    t16_endnetent = 13;
    t16_endprotoent = 14;
    t16_endservent = 15;
    t16_gethostent = 20;
    t16_getnetbyaddr = 23;
    t16_getnetbyname = 24;
    t16_getnetent = 25;
    t16_getprotoent = 31;
    t16_getservent = 34;
    t16_putlong = 53;
    t16_putshort = 54;
    t16_recvmsg = 65;
    t16_res_init = 67;
    t16_res_mkquery = 68;
    t16_res_query = 69;
    t16_res_querydomain = 70;
    t16_res_search = 71;
    t16_res_send = 72;
    t16_rexec = 73;
    t16_sendmsg = 77;
    t16_sethostent = 79;
    t16_setnetent = 81;
    t16_setprotoent = 82;
    t16_setservent = 83;
    t16_getlong = 101;
    t16_getshort = 102;
    t16_bsdselect = 110;
    t16_gethostbyaddr = 18;
    t16_gethostbyname = 19;
    t16_gethostid = 21;
    t16_getinetversion = 22;
    t16_getpeername = 27;
    t16_getpid = 28;
    t16_getprotobyname = 29;
    t16_getprotobynumber = 30;
    t16_getservbyname = 32;
    t16_getservbyport = 33;
    t16_getsockname = 35;
    t16_getsockopt = 36;
    t16_gettimeofday = 37;
    t16_inet_addr = 40;
    t16_inet_lnaof = 41;
    t16_inet_makeaddr = 42;
    t16_inet_netof = 43;
    t16_inet_network = 44;
    t16_inet_ntoa = 45;
    t16_ioctl = 46;
    t16_listen = 47;
    t16_lswap = 48;
    t16_soabort = 50;
    t16_readv = 62;
    t16_recv = 63;
    t16_recvfrom = 64;
    t16_removesocketfromlist = 66;
    t16_rindex = 74;
    t16_select = 75;
    t16_send = 76;
    t16_sendto = 78;
    t16_setsockopt = 84;
    t16_shutdown = 85;
    t16_socket = 86;
    t16_sock_cancel = 88;
    t16_so_cancel = 89;
    t16_sock_init = 90;
    t16_sock_term = 91;
    t16_soclose = 92;
    t16_tcperrno = 93;
    t16_writev = 94;
    t16_xsignal = 95;
    t16_perror = 104;
}

// ---- 16-bit result buffers ----

const SLOTS: usize = 16;
const BUF: usize = 2048;

static mut HOST16: [[u8; BUF]; SLOTS] = [[0; BUF]; SLOTS];
static mut SERV16: [[u8; BUF]; SLOTS] = [[0; BUF]; SLOTS];
static mut PROTO16: [[u8; BUF]; SLOTS] = [[0; BUF]; SLOTS];
static mut NTOA16: [[u8; 20]; SLOTS] = [[0; 20]; SLOTS];
static mut NET16: [[u8; BUF]; SLOTS] = [[0; BUF]; SLOTS];
static mut REXEC16: [[u8; 256]; SLOTS] = [[0; 256]; SLOTS];

fn slot() -> usize {
    tcpipcore::tls::tid() as usize % SLOTS
}

/// Builds a structure with 16:16 pointers in a buffer: the fixed part
/// first, then arrays and strings.
struct W {
    base: *mut u8,
    at: usize,
}

impl W {
    fn new(base: *mut u8, fixed: usize) -> W {
        W { base, at: fixed }
    }
    fn room(&self, n: usize) -> bool {
        self.at + n <= BUF
    }
    fn bytes(&mut self, b: &[u8]) -> u32 {
        if !self.room(b.len() + 1) {
            return 0;
        }
        let p = unsafe { self.base.add(self.at) };
        unsafe {
            core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
            *p.add(b.len()) = 0;
        }
        self.at += b.len() + 1;
        far(p)
    }
    fn raw(&mut self, b: &[u8]) -> u32 {
        if !self.room(b.len()) {
            return 0;
        }
        let p = unsafe { self.base.add(self.at) };
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
        self.at += b.len();
        far(p)
    }
    /// A NULL-terminated array of far pointers.
    fn list(&mut self, v: &[u32]) -> u32 {
        self.at = (self.at + 3) & !3;
        let mut b = alloc::vec::Vec::with_capacity(4 * (v.len() + 1));
        for x in v.iter().chain(core::iter::once(&0u32)) {
            b.extend_from_slice(&x.to_le_bytes());
        }
        self.raw(&b)
    }
    fn put32(&mut self, off: usize, v: u32) {
        unsafe { (self.base.add(off) as *mut u32).write_unaligned(v) };
    }
    fn put16(&mut self, off: usize, v: u16) {
        unsafe { (self.base.add(off) as *mut u16).write_unaligned(v) };
    }
}

unsafe fn strings(mut p: *const *mut u8) -> alloc::vec::Vec<&'static [u8]> {
    let mut v = alloc::vec::Vec::new();
    if p.is_null() {
        return v;
    }
    unsafe {
        while !(*p).is_null() {
            v.push(cstr::bytes(*p));
            p = p.add(1);
        }
    }
    v
}

fn sync_h() {
    unsafe { tcpipdll_h_errno = errno::h() as i16 };
}

/// struct hostent { char far *h_name; char far * far *h_aliases; int
/// h_addrtype; int h_length; char far * far *h_addr_list; } (16 bytes).
unsafe fn host16(h: *mut Hostent) -> u32 {
    sync_h();
    if h.is_null() {
        return 0;
    }
    let h = unsafe { &*h };
    let base = unsafe { (*core::ptr::addr_of_mut!(HOST16))[slot()].as_mut_ptr() };
    let mut w = W::new(base, 16);
    let name = w.bytes(unsafe { cstr::bytes(h.h_name) });
    let al: alloc::vec::Vec<u32> = unsafe { strings(h.h_aliases) }.iter().map(|a| w.bytes(a)).collect();
    let aliases = w.list(&al);
    let mut addrs = alloc::vec::Vec::new();
    let mut p = h.h_addr_list as *const *mut u8;
    unsafe {
        while !p.is_null() && !(*p).is_null() {
            addrs.push(w.raw(core::slice::from_raw_parts(*p, 4)));
            p = p.add(1);
        }
    }
    let list = w.list(&addrs);
    w.put32(0, name);
    w.put32(4, aliases);
    w.put16(8, h.h_addrtype as u16);
    w.put16(10, h.h_length as u16);
    w.put32(12, list);
    far(base)
}

/// struct servent { char far *s_name; char far * far *s_aliases; int
/// s_port; char far *s_proto; } (14 bytes).
unsafe fn serv16(s: *mut Servent) -> u32 {
    if s.is_null() {
        return 0;
    }
    let s = unsafe { &*s };
    let base = unsafe { (*core::ptr::addr_of_mut!(SERV16))[slot()].as_mut_ptr() };
    let mut w = W::new(base, 16);
    let name = w.bytes(unsafe { cstr::bytes(s.s_name) });
    let al: alloc::vec::Vec<u32> = unsafe { strings(s.s_aliases) }.iter().map(|a| w.bytes(a)).collect();
    let aliases = w.list(&al);
    let proto = w.bytes(unsafe { cstr::bytes(s.s_proto) });
    w.put32(0, name);
    w.put32(4, aliases);
    w.put16(8, s.s_port as u16);
    w.put32(10, proto);
    far(base)
}

/// struct protoent { char far *p_name; char far * far *p_aliases; int
/// p_proto; } (10 bytes).
unsafe fn proto16(p: *mut Protoent) -> u32 {
    if p.is_null() {
        return 0;
    }
    let p = unsafe { &*p };
    let base = unsafe { (*core::ptr::addr_of_mut!(PROTO16))[slot()].as_mut_ptr() };
    let mut w = W::new(base, 12);
    let name = w.bytes(unsafe { cstr::bytes(p.p_name) });
    let al: alloc::vec::Vec<u32> = unsafe { strings(p.p_aliases) }.iter().map(|a| w.bytes(a)).collect();
    let aliases = w.list(&al);
    w.put32(0, name);
    w.put32(4, aliases);
    w.put16(8, p.p_proto as u16);
    far(base)
}

/// struct netent { char far *n_name; char far * far *n_aliases; int
/// n_addrtype; u_long n_net; } (14 bytes).
unsafe fn net16(n: *mut netdb::Netent) -> u32 {
    if n.is_null() {
        return 0;
    }
    let n = unsafe { &*n };
    let base = unsafe { (*core::ptr::addr_of_mut!(NET16))[slot()].as_mut_ptr() };
    let mut w = W::new(base, 16);
    let name = w.bytes(unsafe { cstr::bytes(n.n_name) });
    let al: alloc::vec::Vec<u32> = unsafe { strings(n.n_aliases) }.iter().map(|a| w.bytes(a)).collect();
    let aliases = w.list(&al);
    w.put32(0, name);
    w.put32(4, aliases);
    w.put16(8, n.n_addrtype as u16);
    w.put32(10, n.n_net);
    far(base)
}

/// The 16-bit msghdr { caddr_t far msg_name; int msg_namelen; struct
/// iovec far *msg_iov; int msg_iovlen; caddr_t far msg_accrights; int
/// msg_accrightslen; } as tcpipcore's flat 4.3 `omsghdr`, and its
/// iovecs.
unsafe fn msg16(m: *const u8) -> ([u32; 6], alloc::vec::Vec<u32>) {
    let a = Args(m);
    let iov = unsafe { iov16(a.p(6), a.i(10)) };
    let h = [a.p(0) as u32, a.i(4) as u32, iov.as_ptr() as u32, (iov.len() / 2) as u32, a.p(12) as u32, a.i(16) as u32];
    (h, iov)
}

// ---- alarm and xsignal ----

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// When the alarm goes off (ms clock; 0: not set).
static ALARM_AT: AtomicU32 = AtomicU32::new(0);
/// The thread that set it, which the handler runs on.
static ALARM_TID: AtomicU32 = AtomicU32::new(0);
static ALARM_PENDING: AtomicBool = AtomicBool::new(false);
static TIMER_STARTED: AtomicBool = AtomicBool::new(false);
/// The handler (16:16) and the signal it was set for.
static HANDLER: AtomicU32 = AtomicU32::new(0);
static HANDLER_SIG: AtomicU32 = AtomicU32::new(14);
/// The sockets the alarm's thread is blocked on (0: none).
static BLOCKED: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];

fn now_ms() -> u32 {
    let mut v = 0u32;
    unsafe { os2::DosQuerySysInfo(14, 14, &mut v, 4) };
    v
}

unsafe extern "C" fn alarm_timer(_: u32) {
    loop {
        unsafe { os2::DosSleep(50) };
        let at = ALARM_AT.load(Ordering::Acquire);
        if at == 0 || (now_ms().wrapping_sub(at) as i32) < 0 {
            continue;
        }
        if ALARM_AT.compare_exchange(at, 0, Ordering::AcqRel, Ordering::Relaxed).is_err() {
            continue;
        }
        ALARM_PENDING.store(true, Ordering::Release);
        for b in &BLOCKED {
            let s = b.load(Ordering::Acquire);
            if s != 0 {
                sock::so_cancel(s as i32);
            }
        }
    }
}

/// _alarm(seconds): the seconds left of the previous alarm.
fn alarm(secs: u32) -> u32 {
    let now = now_ms();
    let prev = ALARM_AT.swap(if secs == 0 { 0 } else { now.wrapping_add(secs * 1000).max(1) }, Ordering::AcqRel);
    ALARM_TID.store(tcpipcore::tls::tid(), Ordering::Release);
    if secs != 0 && !TIMER_STARTED.swap(true, Ordering::AcqRel) {
        let mut tid = 0;
        unsafe { os2::DosCreateThread(&mut tid, alarm_timer, 0, 2, 16384) };
    }
    if prev == 0 { 0 } else { (prev.wrapping_sub(now) as i32).max(0) as u32 / 1000 + 1 }
}

/// A pending alarm for this thread: run its handler (it may not return).
fn deliver_alarm() {
    if !ALARM_PENDING.load(Ordering::Acquire) || ALARM_TID.load(Ordering::Acquire) != tcpipcore::tls::tid() {
        return;
    }
    ALARM_PENDING.store(false, Ordering::Release);
    let h = HANDLER.load(Ordering::Acquire);
    // SIG_DFL (0) and SIG_IGN (1) and the like: nothing to call.
    if h >> 16 == 0 {
        return;
    }
    let sig = HANDLER_SIG.load(Ordering::Relaxed) as u16;
    thunk16::call16(h, &sig.to_le_bytes());
}

/// Run a blocking call on sockets `socks`, so the alarm can interrupt it.
fn blocking(socks: &[i32], f: impl FnOnce() -> i32) -> i32 {
    let mine = ALARM_TID.load(Ordering::Acquire) == tcpipcore::tls::tid() && ALARM_AT.load(Ordering::Acquire) != 0;
    if mine {
        for (b, &s) in BLOCKED.iter().zip(socks) {
            b.store(s as u32, Ordering::Release);
        }
    }
    let r = f();
    if mine {
        for b in &BLOCKED {
            b.store(0, Ordering::Release);
        }
    }
    r
}

/// A 16-bit `int *` length in and out around a call taking `*mut i32`.
unsafe fn with_len16(p: *mut u8, f: impl FnOnce(*mut i32) -> i32) -> i32 {
    if p.is_null() {
        return f(null_mut());
    }
    let mut v = unsafe { (p as *const i16).read_unaligned() } as i32;
    let r = f(&mut v);
    unsafe { (p as *mut i16).write_unaligned(v as i16) };
    r
}

/// 16-bit iovec { char far *iov_base; int iov_len; } to the flat pair
/// layout tcpipcore takes.
unsafe fn iov16(p: *const u8, n: i32) -> alloc::vec::Vec<u32> {
    let mut v = alloc::vec::Vec::new();
    for i in 0..n.clamp(0, 1024) as usize {
        let a = Args(unsafe { p.add(6 * i) });
        v.push(a.p(0) as u32);
        v.push(a.w(4) as u32);
    }
    v
}

extern "C" fn dispatch(ord: u32, args: *const u8) -> u32 {
    // An alarm that went off while the program was busy elsewhere.
    if ord != 3 && ord != 95 {
        deliver_alarm();
    }
    let r = dispatch1(ord, args);
    if ord != 3 && ord != 95 {
        deliver_alarm();
    }
    r
}

fn dispatch1(ord: u32, args: *const u8) -> u32 {
    let a = Args(args);
    let s = a.i(0);
    let r: i32 = unsafe {
        match ord {
            1 => blocking(&[s], || with_len16(a.p(6), |l| sock::accept(ABI, s, a.p(2), l))),
            2 => {
                sock::addsockettolist(s);
                0
            }
            6 => 0,
            3 => return alarm(a.w(0) as u32),
            95 => {
                // _xsignal(sig, handler): the previous handler.
                HANDLER_SIG.store(a.w(0) as u32, Ordering::Relaxed);
                return HANDLER.swap(a.l(2), Ordering::AcqRel);
            }
            9 => {
                // dnptrs: a NULL-ended array of far pointers.
                let (dp, lp) = (a.p(10) as *mut u32, a.p(14) as *mut u32);
                if dp.is_null() {
                    tcpipcore::resolv::dn_comp(a.p(0), a.p(4), a.i(8), null_mut(), null_mut())
                } else {
                    let mut v: alloc::vec::Vec<*mut u8> = alloc::vec::Vec::new();
                    let mut i = 0;
                    while i < 64 && dp.add(i).read_unaligned() != 0 {
                        v.push(thunk16::flat(dp.add(i).read_unaligned()));
                        i += 1;
                    }
                    let room = if lp.is_null() { v.len() + 1 } else { ((lp as usize).wrapping_sub(dp as usize) / 4 + 1).clamp(v.len() + 1, 64) };
                    v.resize(room, null_mut());
                    let last = v.as_mut_ptr().add(room - 1);
                    let r = tcpipcore::resolv::dn_comp(a.p(0), a.p(4), a.i(8), v.as_mut_ptr(), last);
                    for (k, p) in v.iter().enumerate().take(room) {
                        dp.add(k).write_unaligned(if p.is_null() { 0 } else { far(*p) });
                    }
                    r
                }
            }
            10 => tcpipcore::resolv::dn_expand(a.p(0), a.p(4), a.p(8), a.p(12), a.i(16)),
            11 => tcpipcore::resolv::dn_skipname(a.p(0), a.p(4)),
            12 => {
                netdb::endhostent();
                0
            }
            13 => {
                netdb::endnetent();
                0
            }
            14 => {
                netdb::endprotoent();
                0
            }
            15 => {
                netdb::endservent();
                0
            }
            20 => return host16(netdb::gethostent()),
            23 => return net16(netdb::getnetbyaddr(a.l(0), a.i(4))),
            24 => return net16(netdb::getnetbyname(a.p(0))),
            25 => return net16(netdb::getnetent()),
            31 => return proto16(netdb::getprotoent()),
            34 => return serv16(netdb::getservent()),
            53 => {
                tcpipcore::resolv::putlong(a.l(0), a.p(4));
                0
            }
            54 => {
                tcpipcore::resolv::putshort(a.w(0) as u32, a.p(2));
                0
            }
            101 => return tcpipcore::resolv::getlong(a.p(0)),
            102 => return tcpipcore::resolv::getshort(a.p(0)) & 0xffff,
            65 | 77 => {
                let m = a.p(2);
                if m.is_null() {
                    errno::fail(errno::EFAULT)
                } else {
                    let (mut h, _iov) = msg16(m);
                    if ord == 77 {
                        blocking(&[s], || sock::sendmsg(ABI, s, h.as_ptr(), a.i(6)))
                    } else {
                        let r = blocking(&[s], || sock::recvmsg(ABI, s, h.as_mut_ptr(), a.i(6)));
                        if r >= 0 {
                            (m.add(4) as *mut i16).write_unaligned(h[1] as i16);
                            (m.add(16) as *mut i16).write_unaligned(0);
                        }
                        r
                    }
                }
            }
            67 => tcpipcore::resolv::res_init(),
            68 => tcpipcore::resolv::res_mkquery(a.i(0), a.p(2), a.i(6), a.i(8), a.p(10), a.i(14), a.p(16), a.p(20), a.i(24)),
            69 => tcpipcore::resolv::res_query(a.p(0), a.i(4), a.i(6), a.p(8), a.i(12)),
            70 => tcpipcore::resolv::res_querydomain(a.p(0), a.p(4), a.i(8), a.i(10), a.p(12), a.i(16)),
            71 => tcpipcore::resolv::res_search(a.p(0), a.i(4), a.i(6), a.p(8), a.i(12)),
            72 => tcpipcore::resolv::res_send(a.p(0), a.i(4), a.p(6), a.i(10)),
            73 => {
                // _rexec(char far * far *ahost, u_short rport, user,
                // passwd, cmd, int far *fd2p)
                let ah = a.p(0) as *mut u32;
                if ah.is_null() {
                    errno::fail(errno::EFAULT)
                } else {
                    let mut host = thunk16::flat(ah.read_unaligned());
                    let fd2p = a.p(18);
                    let mut fd2 = -1i32;
                    let r = tcpipcore::rexec::rexec(&mut host, a.w(4) as i32, a.p(6), a.p(10), a.p(14), if fd2p.is_null() { null_mut() } else { &mut fd2 });
                    if r >= 0 {
                        // The official name, in a buffer 16-bit code can see.
                        let buf = &mut (*core::ptr::addr_of_mut!(REXEC16))[slot()];
                        let n = cstr::bytes(host);
                        let k = n.len().min(255);
                        buf[..k].copy_from_slice(&n[..k]);
                        buf[k] = 0;
                        ah.write_unaligned(far(buf.as_ptr()));
                        if !fd2p.is_null() {
                            (fd2p as *mut i16).write_unaligned(fd2 as i16);
                        }
                    }
                    r
                }
            }
            79 => {
                netdb::sethostent(a.i(0));
                0
            }
            81 => {
                netdb::setnetent(a.i(0));
                0
            }
            82 => {
                netdb::setprotoent(a.i(0));
                0
            }
            83 => {
                netdb::setservent(a.i(0));
                0
            }
            110 => {
                // _bsdselect(nfds, fd_set far *r, *w, *e, struct timeval
                // far *tv): fd_sets of u_long bit masks and timeval {long,
                // long}, as 32-bit code has them.
                let mut socks: alloc::vec::Vec<i32> = alloc::vec::Vec::new();
                for set in [a.p(2), a.p(6), a.p(10)] {
                    if set.is_null() {
                        continue;
                    }
                    for fd in 0..a.i(0).clamp(0, 2048) as usize {
                        if (set.add(fd / 8).read() >> (fd % 8)) & 1 != 0 && socks.len() < 8 {
                            socks.push(fd as i32);
                        }
                    }
                }
                blocking(&socks, || sock::select_v40(a.i(0), a.p(2) as *mut u32, a.p(6) as *mut u32, a.p(10) as *mut u32, a.p(14) as *const i32))
            }
            4 => sock::bind(s, a.p(2), a.i(6)),
            5 => return (a.w(0)).swap_bytes() as u32,
            7 => sock::connect(s, a.p(2), a.i(6)),
            18 => return host16(netdb::gethostbyaddr(a.p(0), a.i(4), a.i(6))),
            19 => return host16(netdb::gethostbyname(a.p(0))),
            21 => return sock::gethostid(),
            22 => sock::getinetversion(a.p(0)),
            27 => with_len16(a.p(6), |l| sock::getname(ABI, s, true, a.p(2), l)),
            28 => {
                let (mut tib, mut pib) = (null_mut(), null_mut());
                os2::DosGetInfoBlocks(&mut tib, &mut pib);
                (*pib).pid as i32
            }
            29 => return proto16(netdb::getprotobyname(a.p(0))),
            30 => return proto16(netdb::getprotobynumber(a.i(0))),
            32 => return serv16(netdb::getservbyname(a.p(0), a.p(4))),
            33 => return serv16(netdb::getservbyport(a.w(0) as i32, a.p(2))),
            35 => with_len16(a.p(6), |l| sock::getname(ABI, s, false, a.p(2), l)),
            36 => {
                // 16-bit int options come back as 2 bytes.
                let (val, lenp) = (a.p(6), a.p(10));
                let mut buf = [0u8; 16];
                let mut n = 16i32;
                let r = sock::getsockopt(s, a.i(2), a.i(4), buf.as_mut_ptr(), &mut n);
                if r == 0 && !val.is_null() && !lenp.is_null() {
                    let room = (lenp as *const i16).read_unaligned() as usize;
                    let out: alloc::vec::Vec<u8> = if n == 4 && room == 2 {
                        buf[..2].to_vec()
                    } else if n == 8 && room == 4 {
                        alloc::vec![buf[0], buf[1], buf[4], buf[5]]
                    } else {
                        buf[..(n as usize).min(room)].to_vec()
                    };
                    core::ptr::copy_nonoverlapping(out.as_ptr(), val, out.len());
                    (lenp as *mut i16).write_unaligned(out.len() as i16);
                }
                r
            }
            37 => {
                let tz = a.p(4);
                let mut t = [0i32; 2];
                let r = sock::gettimeofday(a.p(0) as *mut i32, t.as_mut_ptr());
                if !tz.is_null() {
                    (tz as *mut i16).write_unaligned(t[0] as i16);
                    (tz.add(2) as *mut i16).write_unaligned(0);
                }
                r
            }
            40 => return inet::inet_addr(a.p(0)),
            41 => return inet::inet_lnaof(a.l(0)),
            42 => return inet::inet_makeaddr(a.l(0), a.l(4)),
            43 => return inet::inet_netof(a.l(0)),
            44 => return inet::inet_network(a.p(0)),
            45 => {
                let buf = &mut (*core::ptr::addr_of_mut!(NTOA16))[slot()];
                inet::ntoa_into(inet::net_bytes(a.l(0)), buf);
                return far(buf.as_ptr());
            }
            46 => {
                // ioctl(s, cmd, data, len): FIONBIO/FIONREAD take a
                // 16-bit int.
                let (cmd, data) = (a.w(2) as u32, a.p(4));
                if (cmd == 0x667e || cmd == 0x667f) && !data.is_null() {
                    let mut v = (data as *const i16).read_unaligned() as i32;
                    let r = sock::ioctl_len(ABI, s, cmd, &mut v as *mut i32 as *mut u8, 4);
                    if cmd == 0x667f && r == 0 {
                        (data as *mut i16).write_unaligned(v.min(0x7fff) as i16);
                    }
                    r
                } else {
                    sock::ioctl_len(ABI, s, cmd, data, a.i(6))
                }
            }
            47 => sock::listen(s, a.i(2)),
            48 => return a.l(0).swap_bytes(),
            50 | 88 | 89 => sock::so_cancel(s),
            62 => {
                let v = iov16(a.p(2), a.i(6));
                sock::readv(ABI, s, v.as_ptr(), (v.len() / 2) as i32)
            }
            94 => {
                let v = iov16(a.p(2), a.i(6));
                sock::writev(ABI, s, v.as_ptr(), (v.len() / 2) as i32)
            }
            63 => blocking(&[s], || sock::recvfrom(ABI, s, a.p(2), a.w(6) as i32, a.i(8), null_mut(), null_mut())),
            64 => {
                let (from, fromlen) = (a.p(10), a.p(14));
                blocking(&[s], || with_len16(fromlen, |l| sock::recvfrom(ABI, s, a.p(2), a.w(6) as i32, a.i(8), from, l)))
            }
            66 => sock::removesocketfromlist(s),
            74 => {
                let p = a.p(0);
                let c = a.w(4) as u8;
                let b = cstr::bytes(p);
                return match b.iter().rposition(|&x| x == c) {
                    Some(i) => far(p.add(i)),
                    None if c == 0 => far(p.add(b.len())),
                    None => 0,
                };
            }
            75 => {
                // select(int far *s, nread, nwrite, nexcept, long ms):
                // 16-bit socket numbers.
                let p = a.p(0);
                let (nr, nw, ne) = (a.i(4), a.i(6), a.i(8));
                let n = (nr + nw + ne).clamp(0, 2048) as usize;
                let mut v: alloc::vec::Vec<i32> = (0..n).map(|i| (p.add(2 * i) as *const i16).read_unaligned() as i32).collect();
                let socks = v.clone();
                let r = blocking(&socks, || sock::os2_select(if n == 0 { null_mut() } else { v.as_mut_ptr() }, nr, nw, ne, a.l(10) as i32));
                if r >= 0 {
                    for (i, x) in v.iter().enumerate() {
                        (p.add(2 * i) as *mut i16).write_unaligned(*x as i16);
                    }
                }
                r
            }
            76 => sock::send(ABI, s, a.p(2), a.w(6) as i32, a.i(8)),
            78 => sock::sendto(ABI, s, a.p(2), a.w(6) as i32, a.i(8), a.p(10), a.i(14)),
            84 => {
                // 16-bit ints (2 bytes) and struct linger {int, int}.
                let (val, len) = (a.p(6), a.i(10));
                let mut buf = [0u8; 16];
                let n = match len {
                    2 if !val.is_null() => {
                        buf[..4].copy_from_slice(&((val as *const i16).read_unaligned() as i32).to_le_bytes());
                        4
                    }
                    4 if a.i(4) == 0x80 && !val.is_null() => {
                        buf[..4].copy_from_slice(&((val as *const i16).read_unaligned() as i32).to_le_bytes());
                        buf[4..8].copy_from_slice(&((val.add(2) as *const i16).read_unaligned() as i32).to_le_bytes());
                        8
                    }
                    l if l > 0 && !val.is_null() => {
                        let l = (l as usize).min(16);
                        core::ptr::copy_nonoverlapping(val, buf.as_mut_ptr(), l);
                        l as i32
                    }
                    _ => 0,
                };
                sock::setsockopt(s, a.i(2), a.i(4), buf.as_ptr(), n)
            }
            85 => sock::shutdown(s, a.i(2)),
            86 => sock::socket(a.i(0), a.i(2), a.i(4)),
            90 => sock::sock_init(),
            91 => 0,
            92 => sock::soclose(s),
            93 => errno::get(),
            104 => {
                sock::psock_errno(a.p(0));
                0
            }
            _ => errno::fail(errno::EOPNOTSUPP),
        }
    };
    r as u32
}
