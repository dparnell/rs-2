//! HOST name | address: look a host up through TCPIP32.DLL's resolver
//! (HOSTS file, then DNS; addresses by a PTR query).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::vec::Vec;
use utils::*;
os2::entry!(main);

#[repr(C)]
struct Hostent {
    name: *const u8,
    aliases: *const *const u8,
    addrtype: i32,
    length: i32,
    addrs: *const *const u8,
}

unsafe extern "C" {
    #[link_name = "TCPIP32.111"]
    fn gethostbyname(name: *const u8) -> *const Hostent;
    #[link_name = "TCPIP32.112"]
    fn gethostbyaddr(addr: *const u8, len: i32, typ: i32) -> *const Hostent;
    #[link_name = "TCPIP32.151"]
    fn tcp_h_errno1() -> *const i32;
}

unsafe fn text(p: *const u8) -> alloc::string::String {
    let mut s = alloc::string::String::new();
    let mut p = p;
    if p.is_null() {
        return s;
    }
    unsafe {
        while *p != 0 {
            s.push(*p as char);
            p = p.add(1);
        }
    }
    s
}

unsafe fn list(mut p: *const *const u8) -> Vec<*const u8> {
    let mut v = Vec::new();
    if p.is_null() {
        return v;
    }
    unsafe {
        while !(*p).is_null() {
            v.push(*p);
            p = p.add(1);
        }
    }
    v
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let Some(q) = a.first() else {
        outln("Usage: HOST name | address");
        return 1;
    };
    let h = match os2::net::parse_ip(q) {
        Some(addr) => unsafe { gethostbyaddr(addr.as_ptr(), 4, 2) },
        None => {
            let z = os2::path::cstr(q);
            unsafe { gethostbyname(z.as_ptr()) }
        }
    };
    if h.is_null() {
        let e = unsafe { *tcp_h_errno1() };
        eprint(&format!("HOST: {q}: {}\r\n", os2::net::hstrerror(e as u32)));
        return 2;
    }
    let h = unsafe { &*h };
    let name = unsafe { text(h.name) };
    for p in unsafe { list(h.addrs) } {
        let mut addr = [0u8; 4];
        unsafe { core::ptr::copy_nonoverlapping(p, addr.as_mut_ptr(), 4) };
        outln(&format!("{name} has address {}", os2::net::ip(addr)));
    }
    for al in unsafe { list(h.aliases) } {
        outln(&format!("{name} is also known as {}", unsafe { text(al) }));
    }
    0
}
