//! What RXSOCK imports: REXX, REXXAPI, and TCPIP32 by ordinal, as IBM's.

use crate::rx::ShvBlock;

#[repr(C)]
pub struct HostEnt {
    pub name: *const u8,
    pub aliases: *const *const u8,
    pub addrtype: i32,
    pub length: i32,
    pub addr_list: *const *const u32,
}

unsafe extern "C" {
    #[link_name = "REXX.2"]
    pub fn RexxVariablePool(p: *mut ShvBlock) -> u32;
    #[link_name = "REXXAPI.17"]
    pub fn RexxRegisterFunctionDll(name: *const u8, dll: *const u8, entry: *const u8) -> u32;
    #[link_name = "REXXAPI.19"]
    pub fn RexxDeregisterFunction(name: *const u8) -> u32;

    #[link_name = "TCPIP32.1"]
    pub fn accept(s: i32, addr: *mut u8, len: *mut i32) -> i32;
    #[link_name = "TCPIP32.2"]
    pub fn bind(s: i32, addr: *const u8, len: i32) -> i32;
    #[link_name = "TCPIP32.3"]
    pub fn connect(s: i32, addr: *const u8, len: i32) -> i32;
    #[link_name = "TCPIP32.4"]
    pub fn gethostid() -> u32;
    #[link_name = "TCPIP32.5"]
    pub fn getpeername(s: i32, addr: *mut u8, len: *mut i32) -> i32;
    #[link_name = "TCPIP32.6"]
    pub fn getsockname(s: i32, addr: *mut u8, len: *mut i32) -> i32;
    #[link_name = "TCPIP32.7"]
    pub fn getsockopt(s: i32, level: i32, opt: i32, val: *mut u8, len: *mut i32) -> i32;
    #[link_name = "TCPIP32.9"]
    pub fn listen(s: i32, backlog: i32) -> i32;
    #[link_name = "TCPIP32.10"]
    pub fn recv(s: i32, buf: *mut u8, len: i32, flags: i32) -> i32;
    #[link_name = "TCPIP32.11"]
    pub fn recvfrom(s: i32, buf: *mut u8, len: i32, flags: i32, addr: *mut u8, alen: *mut i32) -> i32;
    #[link_name = "TCPIP32.12"]
    pub fn os2_select(socks: *mut i32, nr: i32, nw: i32, ne: i32, timeout: i32) -> i32;
    #[link_name = "TCPIP32.13"]
    pub fn send(s: i32, buf: *const u8, len: i32, flags: i32) -> i32;
    #[link_name = "TCPIP32.14"]
    pub fn sendto(s: i32, buf: *const u8, len: i32, flags: i32, addr: *const u8, alen: i32) -> i32;
    #[link_name = "TCPIP32.15"]
    pub fn setsockopt(s: i32, level: i32, opt: i32, val: *const u8, len: i32) -> i32;
    #[link_name = "TCPIP32.17"]
    pub fn soclose(s: i32) -> i32;
    #[link_name = "TCPIP32.20"]
    pub fn sock_errno() -> i32;
    #[link_name = "TCPIP32.25"]
    pub fn shutdown(s: i32, how: i32) -> i32;
    #[link_name = "TCPIP32.26"]
    pub fn sock_init() -> i32;
    #[link_name = "TCPIP32.30"]
    pub fn psock_errno(s: *const u8);
    #[link_name = "TCPIP32.105"]
    pub fn inet_addr(s: *const u8) -> u32;
    #[link_name = "TCPIP32.110"]
    pub fn inet_ntoa(a: u32) -> *const u8;
    #[link_name = "TCPIP32.111"]
    pub fn gethostbyname(name: *const u8) -> *const HostEnt;
    #[link_name = "TCPIP32.112"]
    pub fn gethostbyaddr(addr: *const u8, len: i32, kind: i32) -> *const HostEnt;
    #[link_name = "TCPIP32.151"]
    pub fn tcp_h_errno1() -> *mut i32;
    #[link_name = "TCPIP32.200"]
    pub fn os2_ioctl(s: i32, cmd: u32, data: *mut u8, len: i32) -> i32;
    #[link_name = "TCPIP32.302"]
    pub fn socketmain(domain: i32, kind: i32, proto: i32, flags: i32) -> i32;
}
