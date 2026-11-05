//! RXSOCK.DLL: the REXX functions for TCP/IP sockets (SockLoadFuncs,
//! SockSocket, SockConnect, ...), binary compatible with IBM's RxSock 2.0.
//! As in IBM's, the DLL exports two entry points: SockLoadFuncs registers
//! the 26 functions with SockFunctionGateWay, which finds the function by
//! the name it was called by and afterwards sets the REXX variables `errno`
//! and `h_errno`. Socket addresses and host entries are stems; the stem
//! argument is used as it is given, `addr.!` making `addr.!family`.

#![no_std]
#![no_main]
#![allow(non_snake_case, dead_code)]

extern crate alloc;
extern crate os2;

mod api;
#[path = "../../rexxutil/src/rx.rs"]
mod rx;

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::DosWrite;

use api::*;
use rx::{Args, INVALID, Ret, RxString, SHV_SYFET, SHV_SYSET, c_len, cz, dec, eqi, fetch_var, set_var, tail};

const AF_INET: u8 = 2;
const SOL_SOCKET: i32 = 0xffff;
const SO_LINGER: i32 = 0x80;
const SO_SNDBUF: i32 = 0x1001;
const SO_RCVBUF: i32 = 0x1002;
const SO_SNDTIMEO: i32 = 0x1005;
const SO_RCVTIMEO: i32 = 0x1006;
const SO_ERROR: i32 = 0x1007;
const SO_TYPE: i32 = 0x1008;
const FIONBIO: u32 = 0x8004_667e;
const FIONREAD: u32 = 0x4004_667f;
const MSG_OOB: i32 = 1;
const MSG_PEEK: i32 = 2;
const MSG_DONTROUTE: i32 = 4;
/// What sockaddr_in takes: length, family, port, address, padding.
const ADDR_LEN: i32 = 16;

/// _DLL_InitTerm(hmod, flag): nothing to set up.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

type Function = fn(&Args, &mut Ret) -> u32;

/// The functions, by the upper-case names REXX calls them by.
const FUNCTIONS: [(&[u8], Function); 26] = [
    (b"SOCKDROPFUNCS\0", sock_drop_funcs),
    (b"SOCKACCEPT\0", sock_accept),
    (b"SOCKBIND\0", sock_bind),
    (b"SOCKCLOSE\0", sock_soclose),
    (b"SOCKCONNECT\0", sock_connect),
    (b"SOCKGETHOSTBYADDR\0", sock_gethostbyaddr),
    (b"SOCKGETHOSTBYNAME\0", sock_gethostbyname),
    (b"SOCKGETHOSTID\0", sock_gethostid),
    (b"SOCKGETPEERNAME\0", sock_getpeername),
    (b"SOCKGETSOCKNAME\0", sock_getsockname),
    (b"SOCKGETSOCKOPT\0", sock_getsockopt),
    (b"SOCKINIT\0", sock_sock_init),
    (b"SOCKIOCTL\0", sock_ioctl),
    (b"SOCKLISTEN\0", sock_listen),
    (b"SOCKPSOCK_ERRNO\0", sock_psock_errno),
    (b"SOCKRECV\0", sock_recv),
    (b"SOCKRECVFROM\0", sock_recvfrom),
    (b"SOCKSELECT\0", sock_select),
    (b"SOCKSEND\0", sock_send),
    (b"SOCKSENDTO\0", sock_sendto),
    (b"SOCKSETSOCKOPT\0", sock_setsockopt),
    (b"SOCKSHUTDOWN\0", sock_shutdown),
    (b"SOCKSOCK_ERRNO\0", sock_sock_errno),
    (b"SOCKSOCKET\0", sock_socket),
    (b"SOCKSOCLOSE\0", sock_soclose),
    (b"SOCKVERSION\0", sock_version),
];

/// The options by name; an unknown one is 0, for the stack to refuse.
const OPTIONS: [(&[u8], i32); 16] = [
    (b"SO_DEBUG", 1),
    (b"SO_REUSEADDR", 4),
    (b"SO_KEEPALIVE", 8),
    (b"SO_DONTROUTE", 0x10),
    (b"SO_BROADCAST", 0x20),
    (b"SO_USELOOPBACK", 0x40),
    (b"SO_LINGER", SO_LINGER),
    (b"SO_OOBINLINE", 0x100),
    (b"SO_SNDBUF", SO_SNDBUF),
    (b"SO_RCVBUF", SO_RCVBUF),
    (b"SO_SNDLOWAT", 0x1003),
    (b"SO_RCVLOWAT", 0x1004),
    (b"SO_SNDTIMEO", SO_SNDTIMEO),
    (b"SO_RCVTIMEO", SO_RCVTIMEO),
    (b"SO_ERROR", SO_ERROR),
    (b"SO_TYPE", SO_TYPE),
];

/// `errno`'s names from SOCEWOULDBLOCK (10035) on; others stay numbers.
const ERRNO_FIRST: i32 = 10035;
const ERRNOS: [&[u8]; 32] = [
    b"EWOULDBLOCK", b"EINPROGRESS", b"EALREADY", b"ENOTSOCK", b"EDESTADDRREQ", b"EMSGSIZE", b"EPROTOTYPE", b"ENOPROTOOPT",
    b"EPROTONOSUPPORT", b"ESOCKTNOSUPPORT", b"EOPNOTSUPP", b"EPFNOSUPPORT", b"EAFNOSUPPORT", b"EADDRINUSE", b"EADDRNOTAVAIL",
    b"ENETDOWN", b"ENETUNREACH", b"ENETRESET", b"ECONNABORTED", b"ECONNRESET", b"ENOBUFS", b"EISCONN", b"ENOTCONN", b"ESHUTDOWN",
    b"ETOOMANYREFS", b"ETIMEDOUT", b"ECONNREFUSED", b"ELOOP", b"ENAMETOOLONG", b"EHOSTDOWN", b"EHOSTUNREACH", b"ENOTEMPTY",
];

// ---- The entry points ----

/// Both are REXX external functions, `ULONG _System f(PUCHAR name, ULONG
/// argc, RXSTRING argv[], PSZ queue, PRXSTRING ret)`, called with 4-byte
/// stack alignment.
macro_rules! entry {
    ($name:ident, $f:ident) => {
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 32",
                "mov eax, [ebp + 8]", "mov [esp], eax",
                "mov eax, [ebp + 12]", "mov [esp + 4], eax",
                "mov eax, [ebp + 16]", "mov [esp + 8], eax",
                "mov eax, [ebp + 20]", "mov [esp + 12], eax",
                "mov eax, [ebp + 24]", "mov [esp + 16], eax",
                "call {f}",
                "mov esp, ebp", "pop ebp", "ret",
                f = sym $f,
            );
        }
    };
}

entry!(__a_SockLoadFuncs, load_funcs);
entry!(__a_SockFunctionGateWay, gateway);

extern "C" fn load_funcs(_name: *const u8, argc: u32, _argv: *const RxString, _queue: *const u8, ret: *mut RxString) -> u32 {
    Ret::new(ret).set(b"");
    if argc == 0 {
        let msg = b"RxSock 2.0: REXX functions for TCP/IP sockets (rs2)\r\n";
        let mut n = 0;
        unsafe { DosWrite(1, msg.as_ptr(), msg.len() as u32, &mut n) };
    }
    for (name, _) in FUNCTIONS {
        unsafe { RexxRegisterFunctionDll(name.as_ptr(), b"RxSock\0".as_ptr(), b"SockFunctionGateWay\0".as_ptr()) };
    }
    0
}

extern "C" fn gateway(name: *const u8, argc: u32, argv: *const RxString, queue: *const u8, ret: *mut RxString) -> u32 {
    static mut INITED: bool = false;
    let mut r = Ret::new(ret);
    r.set(b"");
    unsafe {
        if !INITED {
            INITED = true;
            sock_init();
        }
    }
    let called = unsafe { core::slice::from_raw_parts(name, c_len(name)) };
    let f = FUNCTIONS.iter().find(|(n, _)| eqi(&n[..n.len() - 1], called));
    let rc = match f {
        Some((_, f)) => f(&Args::new(argc, argv, queue), &mut r),
        None => INVALID,
    };
    set_errno();
    set_h_errno();
    rc
}

fn set_errno() {
    let e = unsafe { sock_errno() };
    let i = e - ERRNO_FIRST;
    let named = usize::try_from(i).ok().and_then(|i| ERRNOS.get(i));
    match named {
        Some(n) => set_var(SHV_SYSET, b"errno", n),
        None => set_var(SHV_SYSET, b"errno", &dec(e as i64)),
    };
}

/// IBM's reads a constant here and always sets 1541; this is the variable
/// its documentation describes.
fn set_h_errno() {
    let e = unsafe { *tcp_h_errno1() };
    let v = match e {
        -1 => b"NETDB_INTERNAL".to_vec(),
        1 => b"HOST_NOT_FOUND".to_vec(),
        2 => b"TRY_AGAIN".to_vec(),
        3 => b"NO_RECOVERY".to_vec(),
        4 => b"NO_ADDRESS".to_vec(),
        _ => dec(e as i64),
    };
    set_var(SHV_SYSET, b"h_errno", &v);
}

// ---- Arguments ----

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&c| c != b' ').unwrap_or(s.len());
    let b = s.iter().rposition(|&c| c != b' ').map_or(a, |i| i + 1);
    &s[a..b]
}

/// strtol in base 10: the number and how much of `s` it took.
fn strtol(s: &[u8]) -> (i32, usize) {
    let start = s.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(s.len());
    let mut i = start;
    let neg = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let digits = i;
    let mut n: i64 = 0;
    while let Some(c) = s.get(i).filter(|c| c.is_ascii_digit()) {
        n = (n * 10 + (*c - b'0') as i64).min(i32::MAX as i64 + 1);
        i += 1;
    }
    if i == digits {
        return (0, 0);
    }
    let n = if neg { -n } else { n };
    (n.clamp(i32::MIN as i64, i32::MAX as i64) as i32, i)
}

/// The leading number of a string, 0 if there is none (atoi).
fn number(s: &[u8]) -> i32 {
    strtol(s).0
}

/// A numeric argument, all of it a number.
fn int_arg(a: &Args, i: usize) -> Option<i32> {
    if !a.valid(i) {
        return None;
    }
    let s = trim(a.cstr(i));
    let (n, used) = strtol(s);
    (used == s.len()).then_some(n)
}

/// `a b`, two numbers.
fn two_numbers(s: &[u8]) -> (i32, i32) {
    let (a, used) = strtol(s);
    (a, number(&s[used..]))
}

/// The value of a stem's tail; empty if the pool has none.
fn fetch(stem: &[u8], t: &[u8]) -> Vec<u8> {
    let name = tail(trim(stem), t);
    fetch_var(SHV_SYFET, &name).1
}

fn store(stem: &[u8], t: &[u8], value: &[u8]) {
    set_var(SHV_SYSET, &tail(trim(stem), t), value);
}

/// The sockaddr_in a stem describes.
fn read_addr(stem: &[u8]) -> [u8; ADDR_LEN as usize] {
    let mut sa = [0u8; ADDR_LEN as usize];
    let family = fetch(stem, b"family");
    let port = fetch(stem, b"port");
    let addr = fetch(stem, b"addr");
    sa[1] = if eqi(trim(&family), b"AF_INET") { AF_INET } else { number(trim(&family)) as u8 };
    sa[2..4].copy_from_slice(&(number(trim(&port)) as u16).to_be_bytes());
    let addr = trim(&addr);
    if !eqi(addr, b"INADDR_ANY") {
        let a = unsafe { inet_addr(cz(addr).as_ptr()) };
        sa[4..8].copy_from_slice(&a.to_le_bytes());
    }
    sa
}

fn dotted(a: u32) -> Vec<u8> {
    let p = unsafe { inet_ntoa(a) };
    unsafe { core::slice::from_raw_parts(p, c_len(p)) }.to_vec()
}

/// A sockaddr_in into a stem.
fn write_addr(sa: &[u8; ADDR_LEN as usize], stem: &[u8]) {
    store(stem, b"family", &dec(sa[1] as i64));
    store(stem, b"port", &dec(u16::from_be_bytes([sa[2], sa[3]]) as i64));
    store(stem, b"addr", &dotted(u32::from_le_bytes([sa[4], sa[5], sa[6], sa[7]])));
}

/// A host entry into a stem: name, alias.N, addrtype, addr, addr.N.
fn write_host(h: &HostEnt, stem: &[u8]) {
    let mut n: i64 = 0;
    store(stem, b"name", unsafe { core::slice::from_raw_parts(h.name, c_len(h.name)) });
    if !h.aliases.is_null() {
        loop {
            let p = unsafe { *h.aliases.add(n as usize) };
            if p.is_null() {
                break;
            }
            n += 1;
            store(stem, &tail(b"alias.", &dec(n)), unsafe { core::slice::from_raw_parts(p, c_len(p)) });
        }
    }
    store(stem, b"alias.0", &dec(n));
    store(stem, b"addrtype", b"AF_INET");
    n = 0;
    if !h.addr_list.is_null() {
        loop {
            let p = unsafe { *h.addr_list.add(n as usize) };
            if p.is_null() {
                break;
            }
            let a = dotted(unsafe { p.read_unaligned() });
            if n == 0 {
                store(stem, b"addr", &a);
            }
            n += 1;
            store(stem, &tail(b"addr.", &dec(n)), &a);
        }
    }
    store(stem, b"addr.0", &dec(n));
}

/// The flags named by the words of an argument.
fn flags(s: &[u8], names: &[(&[u8], i32)]) -> i32 {
    let mut f = 0;
    for w in s.split(|&c| c == b' ').filter(|w| !w.is_empty()) {
        if let Some((_, v)) = names.iter().find(|(n, _)| eqi(n, w)) {
            f |= v;
        }
    }
    f
}

const RECV_FLAGS: [(&[u8], i32); 2] = [(b"MSG_OOB", MSG_OOB), (b"MSG_PEEK", MSG_PEEK)];
const SEND_FLAGS: [(&[u8], i32); 2] = [(b"MSG_OOB", MSG_OOB), (b"MSG_DONTROUTE", MSG_DONTROUTE)];
const SENDTO_FLAGS: [(&[u8], i32); 1] = [(b"MSG_DONTROUTE", MSG_DONTROUTE)];

// ---- The functions ----

fn sock_drop_funcs(_a: &Args, _r: &mut Ret) -> u32 {
    unsafe { RexxDeregisterFunction(b"SockLoadFuncs\0".as_ptr()) };
    for (name, _) in FUNCTIONS {
        unsafe { RexxDeregisterFunction(name.as_ptr()) };
    }
    0
}

fn sock_version(_a: &Args, r: &mut Ret) -> u32 {
    r.set(b"2.0");
    0
}

fn sock_sock_init(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    r.num(unsafe { sock_init() });
    0
}

fn sock_sock_errno(_a: &Args, r: &mut Ret) -> u32 {
    r.num(unsafe { sock_errno() });
    0
}

fn sock_psock_errno(a: &Args, _r: &mut Ret) -> u32 {
    let text = if a.n() == 1 { a.ptr(0) } else { null() };
    unsafe { psock_errno(text) };
    0
}

fn sock_socket(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 3 || !a.exists(0) || !a.exists(1) || !a.exists(2) {
        return INVALID;
    }
    if !eqi(trim(a.cstr(0)), b"AF_INET") {
        return INVALID;
    }
    let kind = trim(a.cstr(1));
    let kind = if eqi(kind, b"SOCK_STREAM") {
        1
    } else if eqi(kind, b"SOCK_DGRAM") {
        2
    } else if eqi(kind, b"SOCK_RAW") {
        3
    } else {
        return INVALID;
    };
    let proto = trim(a.cstr(2));
    let proto = if eqi(proto, b"IPPROTO_UDP") {
        17
    } else if eqi(proto, b"IPPROTO_TCP") {
        6
    } else if proto == b"0" {
        0
    } else {
        return INVALID;
    };
    r.num(unsafe { socketmain(AF_INET as i32, kind, proto, 0) });
    0
}

fn sock_soclose(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    r.num(unsafe { soclose(s) });
    0
}

fn sock_accept(a: &Args, r: &mut Ret) -> u32 {
    if a.n() == 0 || a.n() > 2 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    let mut sa = [0u8; ADDR_LEN as usize];
    let mut len = ADDR_LEN;
    let n = unsafe { accept(s, sa.as_mut_ptr(), &mut len) };
    if a.n() == 2 {
        write_addr(&sa, a.cstr(1));
    }
    r.num(n);
    0
}

fn sock_bind(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    let sa = read_addr(a.cstr(1));
    r.num(unsafe { bind(s, sa.as_ptr(), ADDR_LEN) });
    0
}

fn sock_connect(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    let sa = read_addr(a.cstr(1));
    r.num(unsafe { connect(s, sa.as_ptr(), ADDR_LEN) });
    0
}

fn sock_listen(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 {
        return INVALID;
    }
    let (Some(s), Some(backlog)) = (int_arg(a, 0), int_arg(a, 1)) else { return INVALID };
    r.num(unsafe { listen(s, backlog) });
    0
}

fn sock_shutdown(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 {
        return INVALID;
    }
    let (Some(s), Some(how)) = (int_arg(a, 0), int_arg(a, 1)) else { return INVALID };
    r.num(unsafe { shutdown(s, how) });
    0
}

/// SockGetHostByAddr(addr, stem [, domain]): 1, or 0 if not found.
fn sock_gethostbyaddr(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 3 {
        return INVALID;
    }
    let addr = unsafe { inet_addr(cz(a.cstr(0)).as_ptr()) };
    let domain = if a.n() == 2 { AF_INET as i32 } else { int_arg(a, 2).unwrap_or(0) };
    let h = unsafe { gethostbyaddr(&addr as *const u32 as *const u8, 4, domain) };
    host_result(h, a.cstr(1), r);
    0
}

/// SockGetHostByName(name, stem): 1, or 0 if not found.
fn sock_gethostbyname(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 {
        return INVALID;
    }
    let h = unsafe { gethostbyname(cz(a.cstr(0)).as_ptr()) };
    host_result(h, a.cstr(1), r);
    0
}

fn host_result(h: *const HostEnt, stem: &[u8], r: &mut Ret) {
    if h.is_null() {
        r.num(0);
    } else {
        write_host(unsafe { &*h }, stem);
        r.num(1);
    }
}

fn sock_gethostid(_a: &Args, r: &mut Ret) -> u32 {
    r.set(&dotted(unsafe { gethostid() }.to_be()));
    0
}

fn name_of(a: &Args, r: &mut Ret, f: unsafe extern "C" fn(i32, *mut u8, *mut i32) -> i32) -> u32 {
    if a.n() != 2 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    let mut sa = [0u8; ADDR_LEN as usize];
    let mut len = ADDR_LEN;
    let n = unsafe { f(s, sa.as_mut_ptr(), &mut len) };
    write_addr(&sa, a.cstr(1));
    r.num(n);
    0
}

fn sock_getpeername(a: &Args, r: &mut Ret) -> u32 {
    name_of(a, r, getpeername)
}

fn sock_getsockname(a: &Args, r: &mut Ret) -> u32 {
    name_of(a, r, getsockname)
}

fn option(name: &[u8]) -> i32 {
    let name = trim(name);
    OPTIONS.iter().find(|(n, _)| eqi(n, name)).map_or(0, |(_, v)| *v)
}

/// SockGetSockOpt(socket, 'SOL_SOCKET', option, variable).
fn sock_getsockopt(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 4 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    if !a.exists(1) || !eqi(trim(a.cstr(1)), b"SOL_SOCKET") || !a.valid(3) {
        return INVALID;
    }
    let opt = option(a.cstr(2));
    let mut v = [0i32; 2];
    let mut len = if opt == SO_LINGER { 8 } else { 4 };
    let n = unsafe { getsockopt(s, SOL_SOCKET, opt, v.as_mut_ptr() as *mut u8, &mut len) };
    let value = match opt {
        SO_LINGER => {
            let mut t = dec(v[0] as i64);
            t.push(b' ');
            t.extend_from_slice(&dec(v[1] as i64));
            t
        }
        SO_TYPE => match v[0] {
            1 => b"STREAM".to_vec(),
            2 => b"DGRAM".to_vec(),
            3 => b"RAW".to_vec(),
            _ => b"UNKNOWN".to_vec(),
        },
        _ => dec(v[0] as i64),
    };
    set_var(SHV_SYSET, a.cstr(3), &value);
    r.num(n);
    0
}

/// SockSetSockOpt(socket, 'SOL_SOCKET', option, value): "-1" for the two
/// options that can only be read.
fn sock_setsockopt(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 4 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    if !a.exists(1) || !eqi(trim(a.cstr(1)), b"SOL_SOCKET") || !a.valid(3) {
        return INVALID;
    }
    let opt = option(a.cstr(2));
    let value = trim(a.cstr(3));
    let (v, len) = match opt {
        SO_LINGER | SO_SNDTIMEO | SO_RCVTIMEO => {
            let (x, y) = two_numbers(value);
            ([x, y], 8)
        }
        SO_ERROR | SO_TYPE => {
            r.set(b"-1");
            return 0;
        }
        _ => ([number(value), 0], 4),
    };
    r.num(unsafe { setsockopt(s, SOL_SOCKET, opt, v.as_ptr() as *const u8, len) });
    0
}

/// SockIoctl(socket, 'FIONBIO'|'FIONREAD', data): the second sets the
/// variable `data` names.
fn sock_ioctl(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 3 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    if !a.valid(1) || !a.valid(2) {
        return INVALID;
    }
    let cmd = trim(a.cstr(1));
    let (cmd, mut data) = if eqi(cmd, b"FIONBIO") {
        (FIONBIO, int_arg(a, 2).unwrap_or(0))
    } else if eqi(cmd, b"FIONREAD") {
        (FIONREAD, 0)
    } else {
        r.set(b"-1");
        return 0;
    };
    let n = unsafe { os2_ioctl(s, cmd, &mut data as *mut i32 as *mut u8, 4) };
    if cmd == FIONREAD {
        set_var(SHV_SYSET, a.cstr(2), &dec(data as i64));
    }
    r.num(n);
    0
}

/// SockRecv(socket, variable, length [, flags]).
fn sock_recv(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 3 || a.n() > 4 {
        return INVALID;
    }
    let (Some(s), Some(len)) = (int_arg(a, 0), int_arg(a, 2)) else { return INVALID };
    let f = if a.n() == 4 { flags(a.cstr(3), &RECV_FLAGS) } else { 0 };
    let mut buf = vec![0u8; len.max(0) as usize];
    let n = unsafe { recv(s, buf.as_mut_ptr(), len, f) };
    set_var(SHV_SYSET, a.cstr(1), &buf[..n.max(0) as usize]);
    r.num(n);
    0
}

/// SockRecvFrom(socket, variable, length [, flags], stem).
fn sock_recvfrom(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 4 || a.n() > 5 {
        return INVALID;
    }
    let (Some(s), Some(len)) = (int_arg(a, 0), int_arg(a, 2)) else { return INVALID };
    let f = if a.n() == 5 { flags(a.cstr(3), &RECV_FLAGS) } else { 0 };
    let stem = a.cstr(a.n() - 1);
    let mut buf = vec![0u8; len.max(0) as usize];
    let mut sa = [0u8; ADDR_LEN as usize];
    let mut alen = ADDR_LEN;
    let n = unsafe { recvfrom(s, buf.as_mut_ptr(), len, f, sa.as_mut_ptr(), &mut alen) };
    write_addr(&sa, stem);
    set_var(SHV_SYSET, a.cstr(1), &buf[..n.max(0) as usize]);
    r.num(n);
    0
}

/// SockSend(socket, data [, flags]).
fn sock_send(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 3 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    if !a.valid(1) {
        return INVALID;
    }
    let f = if a.n() == 3 { flags(a.cstr(2), &SEND_FLAGS) } else { 0 };
    let data = a.bytes(1);
    r.num(unsafe { send(s, data.as_ptr(), data.len() as i32, f) });
    0
}

/// SockSendTo(socket, data [, flags], stem).
fn sock_sendto(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 3 || a.n() > 4 {
        return INVALID;
    }
    let Some(s) = int_arg(a, 0) else { return INVALID };
    if !a.valid(1) {
        return INVALID;
    }
    let f = if a.n() == 4 { flags(a.cstr(2), &SENDTO_FLAGS) } else { 0 };
    let sa = read_addr(a.cstr(a.n() - 1));
    let data = a.bytes(1);
    r.num(unsafe { sendto(s, data.as_ptr(), data.len() as i32, f, sa.as_ptr(), ADDR_LEN) });
    0
}

/// The sockets of `stem.0` and `stem.N`; None if there is no list.
fn read_list(a: &Args, i: usize) -> Option<Vec<i32>> {
    if !a.valid(i) {
        return None;
    }
    let stem = a.cstr(i);
    let count = fetch(stem, b"0");
    let count = trim(&count);
    let (n, used) = strtol(count);
    if used != count.len() || used == 0 {
        return None;
    }
    Some((1..=n.max(0) as i64).map(|k| number(trim(&fetch(stem, &dec(k))))).collect())
}

/// What select left of a list (the others are -1), back into its stem.
fn write_list(a: &Args, i: usize, ready: &[i32]) {
    let stem = a.cstr(i);
    let mut n = 0;
    for &s in ready.iter().filter(|&&s| s != -1) {
        n += 1;
        store(stem, &dec(n), &dec(s as i64));
    }
    store(stem, b"0", &dec(n));
}

/// SockSelect(reads, writes, excepts [, timeout]): stems, or "" for none;
/// without a timeout it waits. The stems keep the sockets that are ready.
fn sock_select(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 3 || a.n() > 4 {
        return INVALID;
    }
    let timeout = if a.n() == 4 && a.valid(3) { number(trim(a.cstr(3))).max(0).saturating_mul(1000) } else { -1 };
    let lists = [read_list(a, 0), read_list(a, 1), read_list(a, 2)];
    let counts = lists.each_ref().map(|l| l.as_ref().map_or(0, |l| l.len()));
    let mut socks: Vec<i32> = lists.iter().flatten().flatten().copied().collect();
    let p = if socks.is_empty() { null_mut() } else { socks.as_mut_ptr() };
    let n = unsafe { os2_select(p, counts[0] as i32, counts[1] as i32, counts[2] as i32, timeout) };
    let mut at = 0;
    for (i, l) in lists.iter().enumerate() {
        if l.is_some() {
            write_list(a, i, &socks[at..at + counts[i]]);
        }
        at += counts[i];
    }
    r.num(n);
    0
}
