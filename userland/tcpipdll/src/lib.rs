//! TCPIPDLL.DLL: the socket and network database calls for 16-bit OS/2
//! programs (TFTP, INETD, LPD, TELNETD, BOOTP and the 16-bit RPC DLL
//! import them by name: `_socket`, `_gethostbyname`, ...). They are
//! Microsoft C `_cdecl far` functions with 16-bit `int`s and far
//! pointers; `thunk16` brings each call into 32-bit code, where the
//! arguments are widened and the work done by `tcpipcore` (the 4.0
//! structure layouts, which 16-bit `sockaddr`s share). Results that hold
//! pointers (`hostent`, `servent`, `protoent`, `inet_ntoa`'s text) are
//! rebuilt with 16:16 pointers in per-thread buffers of the DLL's data.

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
    let a = Args(args);
    let s = a.i(0);
    let r: i32 = unsafe {
        match ord {
            1 => with_len16(a.p(6), |l| sock::accept(ABI, s, a.p(2), l)),
            2 => {
                sock::addsockettolist(s);
                0
            }
            6 => 0,
            3 | 95 => 0,
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
            63 => sock::recvfrom(ABI, s, a.p(2), a.w(6) as i32, a.i(8), null_mut(), null_mut()),
            64 => {
                let (from, fromlen) = (a.p(10), a.p(14));
                with_len16(fromlen, |l| sock::recvfrom(ABI, s, a.p(2), a.w(6) as i32, a.i(8), from, l))
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
                let r = sock::os2_select(if n == 0 { null_mut() } else { v.as_mut_ptr() }, nr, nw, ne, a.l(10) as i32);
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
