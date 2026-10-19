//! TCP32DLL.DLL: the network database, resolver and address calls of
//! OS/2 TCP/IP 4.0 (gethostbyname, getservbyname, inet_addr, res_*, the
//! SOCKS R* calls, ...), with `h_errno` and `_res` as exported variables.
//! Same implementation as TCPIP32 (`tcpipcore`).

#![no_std]
#![no_main]
#![allow(non_snake_case, clippy::missing_safety_doc, clippy::too_many_arguments)]

extern crate alloc;
extern crate os2;

mod exports;

use tcpipcore::errno;
use tcpipcore::netdb::{self, Hostent, Netent, Protoent, Servent};
use tcpipcore::resolv;
use tcpipcore::sock::{self, Abi};
use tcpipcore::inet;

const ABI: Abi = Abi::V40;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn settimeofday(_tv: *const i32, _tz: *const i32) -> i32 {
    errno::fail(errno::EPERM)
}
#[unsafe(no_mangle)]
pub extern "C" fn gettimeofday(tv: *mut i32, tz: *mut i32) -> i32 {
    unsafe { sock::gettimeofday(tv, tz) }
}
#[unsafe(no_mangle)]
pub extern "C" fn lswap(v: u32) -> u32 {
    v.swap_bytes()
}
#[unsafe(no_mangle)]
pub extern "C" fn bswap(v: u32) -> u32 {
    (v as u16).swap_bytes() as u32
}
#[unsafe(no_mangle)]
pub extern "C" fn inet_addr(s: *const u8) -> u32 {
    unsafe { inet::inet_addr(s) }
}
#[unsafe(no_mangle)]
pub extern "C" fn inet_lnaof(a: u32) -> u32 {
    inet::inet_lnaof(a)
}
#[unsafe(no_mangle)]
pub extern "C" fn inet_netof(a: u32) -> u32 {
    inet::inet_netof(a)
}
#[unsafe(no_mangle)]
pub extern "C" fn inet_makeaddr(net: u32, host: u32) -> u32 {
    inet::inet_makeaddr(net, host)
}
#[unsafe(no_mangle)]
pub extern "C" fn inet_network(s: *const u8) -> u32 {
    unsafe { inet::inet_network(s) }
}
#[unsafe(no_mangle)]
pub extern "C" fn inet_ntoa(a: u32) -> *const u8 {
    inet::inet_ntoa(a)
}
#[unsafe(no_mangle)]
pub extern "C" fn gethostbyname(name: *const u8) -> *mut Hostent {
    unsafe { netdb::gethostbyname(name) }
}
#[unsafe(no_mangle)]
pub extern "C" fn gethostbyaddr(addr: *const u8, len: i32, typ: i32) -> *mut Hostent {
    unsafe { netdb::gethostbyaddr(addr, len, typ) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getnetbyname(name: *const u8) -> *mut Netent {
    unsafe { netdb::getnetbyname(name) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getnetbyaddr(net: u32, typ: i32) -> *mut Netent {
    netdb::getnetbyaddr(net, typ)
}
#[unsafe(no_mangle)]
pub extern "C" fn setnetent(f: i32) {
    netdb::setnetent(f)
}
#[unsafe(no_mangle)]
pub extern "C" fn endnetent() {
    netdb::endnetent()
}
#[unsafe(no_mangle)]
pub extern "C" fn getnetent() -> *mut Netent {
    netdb::getnetent()
}
#[unsafe(no_mangle)]
pub extern "C" fn setprotoent(f: i32) {
    netdb::setprotoent(f)
}
#[unsafe(no_mangle)]
pub extern "C" fn endprotoent() {
    netdb::endprotoent()
}
#[unsafe(no_mangle)]
pub extern "C" fn getprotoent() -> *mut Protoent {
    netdb::getprotoent()
}
#[unsafe(no_mangle)]
pub extern "C" fn getprotobyname(name: *const u8) -> *mut Protoent {
    unsafe { netdb::getprotobyname(name) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getprotobynumber(n: i32) -> *mut Protoent {
    netdb::getprotobynumber(n)
}
#[unsafe(no_mangle)]
pub extern "C" fn getservbyport(port: i32, proto: *const u8) -> *mut Servent {
    unsafe { netdb::getservbyport(port, proto) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getservbyname(name: *const u8, proto: *const u8) -> *mut Servent {
    unsafe { netdb::getservbyname(name, proto) }
}
#[unsafe(no_mangle)]
pub extern "C" fn setservent(f: i32) {
    netdb::setservent(f)
}
#[unsafe(no_mangle)]
pub extern "C" fn endservent() {
    netdb::endservent()
}
#[unsafe(no_mangle)]
pub extern "C" fn getservent() -> *mut Servent {
    netdb::getservent()
}
#[unsafe(no_mangle)]
pub extern "C" fn sethostent(f: i32) {
    netdb::sethostent(f)
}
#[unsafe(no_mangle)]
pub extern "C" fn endhostent() {
    netdb::endhostent()
}
#[unsafe(no_mangle)]
pub extern "C" fn gethostent() -> *mut Hostent {
    netdb::gethostent()
}
#[unsafe(no_mangle)]
pub extern "C" fn dn_expand(msg: *const u8, eom: *const u8, comp: *const u8, exp: *mut u8, len: i32) -> i32 {
    unsafe { resolv::dn_expand(msg, eom, comp, exp, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn dn_comp(exp: *const u8, comp: *mut u8, len: i32, dnptrs: *mut *mut u8, last: *mut *mut u8) -> i32 {
    unsafe { resolv::dn_comp(exp, comp, len, dnptrs, last) }
}
#[unsafe(no_mangle)]
pub extern "C" fn dn_skipname(comp: *const u8, eom: *const u8) -> i32 {
    unsafe { resolv::dn_skipname(comp, eom) }
}
#[unsafe(no_mangle)]
pub extern "C" fn dn_find(_exp: *const u8, _msg: *const u8, _dnptrs: *mut *mut u8, _last: *mut *mut u8) -> i32 {
    -1
}
#[unsafe(no_mangle)]
pub extern "C" fn _getshort(p: *const u8) -> u32 {
    unsafe { resolv::getshort(p) }
}
#[unsafe(no_mangle)]
pub extern "C" fn _getlong(p: *const u8) -> u32 {
    unsafe { resolv::getlong(p) }
}
#[unsafe(no_mangle)]
pub extern "C" fn putshort(v: u32, p: *mut u8) {
    unsafe { resolv::putshort(v, p) }
}
#[unsafe(no_mangle)]
pub extern "C" fn putlong(v: u32, p: *mut u8) {
    unsafe { resolv::putlong(v, p) }
}
#[unsafe(no_mangle)]
pub extern "C" fn res_init() -> i32 {
    resolv::res_init()
}
#[unsafe(no_mangle)]
pub extern "C" fn res_mkquery(op: i32, dname: *const u8, class: i32, typ: i32, data: *const u8, datalen: i32, newrr: *const u8, buf: *mut u8, buflen: i32) -> i32 {
    unsafe { resolv::res_mkquery(op, dname, class, typ, data, datalen, newrr, buf, buflen) }
}
#[unsafe(no_mangle)]
pub extern "C" fn res_send(msg: *const u8, len: i32, ans: *mut u8, anslen: i32) -> i32 {
    unsafe { resolv::res_send(msg, len, ans, anslen) }
}
#[unsafe(no_mangle)]
pub extern "C" fn res_search(name: *const u8, class: i32, typ: i32, ans: *mut u8, anslen: i32) -> i32 {
    unsafe { resolv::res_search(name, class, typ, ans, anslen) }
}
#[unsafe(no_mangle)]
pub extern "C" fn res_query(name: *const u8, class: i32, typ: i32, ans: *mut u8, anslen: i32) -> i32 {
    unsafe { resolv::res_query(name, class, typ, ans, anslen) }
}
#[unsafe(no_mangle)]
pub extern "C" fn res_querydomain(name: *const u8, domain: *const u8, class: i32, typ: i32, ans: *mut u8, anslen: i32) -> i32 {
    unsafe { resolv::res_querydomain(name, domain, class, typ, ans, anslen) }
}
#[unsafe(no_mangle)]
pub extern "C" fn gethostname(buf: *mut u8, len: i32) -> i32 {
    unsafe { netdb::gethostname(buf, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn rexec(_host: *mut *mut u8, _port: i32, _user: *const u8, _pass: *const u8, _cmd: *const u8, _fd2: *mut i32) -> i32 {
    sock::unsupported()
}
/// The 4.0 h_errno: a function returning the value.
#[unsafe(no_mangle)]
pub extern "C" fn tcp_h_errno() -> i32 {
    errno::h()
}

/// The reentrant forms: the result goes to `*ent`; its strings stay in
/// the thread's buffers (as the plain calls keep them) rather than in
/// the caller's `hostent_data`/`servent_data`.
unsafe fn copy_ent<T>(p: *mut T, out: *mut T) -> i32 {
    if p.is_null() || out.is_null() {
        return -1;
    }
    unsafe { core::ptr::copy_nonoverlapping(p, out, 1) };
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn gethostbyname_r(name: *const u8, ent: *mut Hostent, _data: *mut u8) -> i32 {
    unsafe { copy_ent(netdb::gethostbyname(name), ent) }
}
#[unsafe(no_mangle)]
pub extern "C" fn gethostbyaddr_r(addr: *const u8, len: i32, typ: i32, ent: *mut Hostent, _data: *mut u8) -> i32 {
    unsafe { copy_ent(netdb::gethostbyaddr(addr, len, typ), ent) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getservbyname_r(name: *const u8, proto: *const u8, ent: *mut Servent, _data: *mut u8) -> i32 {
    unsafe { copy_ent(netdb::getservbyname(name, proto), ent) }
}

// SOCKS calls (no proxy: the plain socket calls).
#[unsafe(no_mangle)]
pub extern "C" fn Raccept(s: i32, addr: *mut u8, len: *mut i32) -> i32 {
    unsafe { sock::accept(ABI, s, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn Rbind(s: i32, addr: *const u8, len: i32, _remote: *const u8) -> i32 {
    unsafe { sock::bind(s, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn Rconnect(s: i32, addr: *const u8, len: i32) -> i32 {
    unsafe { sock::connect(s, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn Rgetsockname(s: i32, addr: *mut u8, len: *mut i32) -> i32 {
    unsafe { sock::getname(ABI, s, false, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn Rlisten(s: i32, backlog: i32) -> i32 {
    sock::listen(s, backlog)
}
#[unsafe(no_mangle)]
pub extern "C" fn Rgethostbyname(name: *const u8) -> *mut Hostent {
    unsafe { netdb::gethostbyname(name) }
}
