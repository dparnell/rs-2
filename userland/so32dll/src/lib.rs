//! SO32DLL.DLL: the socket calls of OS/2 TCP/IP 4.0 (4.3BSD structures:
//! `sockaddr` with a USHORT family, `select()` in OS/2's form and
//! `bsdselect()` with bit-mask `fd_set`s, four-argument `ioctl()`), as
//! the 4.0-era programs (RPC, SNMP, the dialer) import them. Same
//! implementation as TCPIP32 (`tcpipcore`), other structure layouts.

#![no_std]
#![no_main]
#![allow(non_snake_case, clippy::missing_safety_doc)]

extern crate alloc;
extern crate os2;

mod exports;

use core::ptr::null_mut;

use tcpipcore::errno;
use tcpipcore::sock::{self, Abi};

const ABI: Abi = Abi::V40;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn accept(s: i32, addr: *mut u8, len: *mut i32) -> i32 {
    unsafe { sock::accept(ABI, s, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn bind(s: i32, addr: *const u8, len: i32) -> i32 {
    unsafe { sock::bind(s, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn connect(s: i32, addr: *const u8, len: i32) -> i32 {
    unsafe { sock::connect(s, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn gethostid() -> u32 {
    sock::gethostid()
}
#[unsafe(no_mangle)]
pub extern "C" fn getpeername(s: i32, addr: *mut u8, len: *mut i32) -> i32 {
    unsafe { sock::getname(ABI, s, true, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getsockname(s: i32, addr: *mut u8, len: *mut i32) -> i32 {
    unsafe { sock::getname(ABI, s, false, addr, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getsockopt(s: i32, level: i32, opt: i32, val: *mut u8, len: *mut i32) -> i32 {
    unsafe { sock::getsockopt(s, level, opt, val, len) }
}
/// ioctl(s, cmd, data, len).
#[unsafe(no_mangle)]
pub extern "C" fn ioctl(s: i32, cmd: u32, data: *mut u8, len: i32) -> i32 {
    unsafe { sock::ioctl_len(ABI, s, cmd, data, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn listen(s: i32, backlog: i32) -> i32 {
    sock::listen(s, backlog)
}
#[unsafe(no_mangle)]
pub extern "C" fn recv(s: i32, buf: *mut u8, len: i32, flags: i32) -> i32 {
    unsafe { sock::recvfrom(ABI, s, buf, len, flags, null_mut(), null_mut()) }
}
#[unsafe(no_mangle)]
pub extern "C" fn recvfrom(s: i32, buf: *mut u8, len: i32, flags: i32, from: *mut u8, fromlen: *mut i32) -> i32 {
    unsafe { sock::recvfrom(ABI, s, buf, len, flags, from, fromlen) }
}
/// select(int *s, nread, nwrite, nexcept, long timeout_ms).
#[unsafe(no_mangle)]
pub extern "C" fn select(socks: *mut i32, nr: i32, nw: i32, ne: i32, timeout: i32) -> i32 {
    unsafe { sock::os2_select(socks, nr, nw, ne, timeout) }
}
#[unsafe(no_mangle)]
pub extern "C" fn send(s: i32, buf: *const u8, len: i32, flags: i32) -> i32 {
    unsafe { sock::send(ABI, s, buf, len, flags) }
}
#[unsafe(no_mangle)]
pub extern "C" fn sendto(s: i32, buf: *const u8, len: i32, flags: i32, to: *const u8, tolen: i32) -> i32 {
    unsafe { sock::sendto(ABI, s, buf, len, flags, to, tolen) }
}
#[unsafe(no_mangle)]
pub extern "C" fn setsockopt(s: i32, level: i32, opt: i32, val: *const u8, len: i32) -> i32 {
    unsafe { sock::setsockopt(s, level, opt, val, len) }
}
#[unsafe(no_mangle)]
pub extern "C" fn socket(d: i32, t: i32, p: i32) -> i32 {
    sock::socket(d, t, p)
}
#[unsafe(no_mangle)]
pub extern "C" fn soclose(s: i32) -> i32 {
    sock::soclose(s)
}
#[unsafe(no_mangle)]
pub extern "C" fn so_cancel(s: i32) -> i32 {
    sock::so_cancel(s)
}
#[unsafe(no_mangle)]
pub extern "C" fn soabort(s: i32) -> i32 {
    sock::so_cancel(s)
}
#[unsafe(no_mangle)]
pub extern "C" fn sock_errno() -> i32 {
    errno::get()
}
#[unsafe(no_mangle)]
pub extern "C" fn recvmsg(s: i32, msg: *mut u32, flags: i32) -> i32 {
    unsafe { sock::recvmsg(ABI, s, msg, flags) }
}
#[unsafe(no_mangle)]
pub extern "C" fn sendmsg(s: i32, msg: *const u32, flags: i32) -> i32 {
    unsafe { sock::sendmsg(ABI, s, msg, flags) }
}
#[unsafe(no_mangle)]
pub extern "C" fn readv(s: i32, iov: *const u32, n: i32) -> i32 {
    unsafe { sock::readv(ABI, s, iov, n) }
}
#[unsafe(no_mangle)]
pub extern "C" fn writev(s: i32, iov: *const u32, n: i32) -> i32 {
    unsafe { sock::writev(ABI, s, iov, n) }
}
#[unsafe(no_mangle)]
pub extern "C" fn shutdown(s: i32, how: i32) -> i32 {
    sock::shutdown(s, how)
}
#[unsafe(no_mangle)]
pub extern "C" fn sock_init() -> i32 {
    sock::sock_init()
}
#[unsafe(no_mangle)]
pub extern "C" fn addsockettolist(_s: i32) {}
#[unsafe(no_mangle)]
pub extern "C" fn removesocketfromlist(_s: i32) -> i32 {
    1
}
#[unsafe(no_mangle)]
pub extern "C" fn psock_errno(s: *const u8) {
    unsafe { sock::psock_errno(s) }
}
#[unsafe(no_mangle)]
pub extern "C" fn getinetversion(buf: *mut u8) -> i32 {
    unsafe { sock::getinetversion(buf) }
}
/// bsdselect(nfds, fd_set *, fd_set *, fd_set *, struct timeval *) with
/// bit-mask sets.
#[unsafe(no_mangle)]
pub extern "C" fn bsdselect(nfds: i32, rd: *mut u32, wr: *mut u32, ex: *mut u32, tv: *const i32) -> i32 {
    unsafe { sock::select_v40(nfds, rd, wr, ex, tv) }
}
#[unsafe(no_mangle)]
pub extern "C" fn set_errno(e: i32) {
    errno::set_raw(e)
}
