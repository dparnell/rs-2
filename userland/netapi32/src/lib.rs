//! NETAPI32.DLL: the 32-bit LAN Server API (`Net32*`, `_System`) on
//! rs2's LAN requester, with the ordinals of IBM's NETAPI32.DLL (File
//! and Print Client / LAN Server 5.0 requester). The work is in
//! `lanreq::netapi`, which NETAPI.DLL (16-bit) shares: uses, shares,
//! servers, the workstation and logon, the REQUESTER service, IBMLAN.INI.
//! Calls rs2 has nothing for (server administration, users and groups
//! on a domain controller, messaging, mailslots, remote IPL...) are in
//! `stubs.rs` and return ERROR_NOT_SUPPORTED.

#![no_std]
#![no_main]
#![allow(non_snake_case, clippy::missing_safety_doc)]

extern crate alloc;
extern crate os2;

use alloc::string::String;
use lanreq::netapi::{self, str_at, ABI32};

mod exports;
mod stubs;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

fn s(p: *const u8) -> Option<String> {
    unsafe { str_at(p) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32UseAdd(server: *const u8, level: u32, buf: *const u8, cb: u32) -> u32 {
    unsafe { netapi::use_add(ABI32, s(server), level, buf, cb) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32UseDel(server: *const u8, dev: *const u8, force: u32) -> u32 {
    netapi::use_del(s(server), s(dev), force)
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32UseEnum(server: *const u8, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    unsafe { netapi::use_enum(ABI32, s(server), level, buf, cb, pread, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32UseGetInfo(server: *const u8, name: *const u8, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    unsafe { netapi::use_get_info(ABI32, s(server), s(name), level, buf, cb, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ShareEnum(server: *const u8, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    unsafe { netapi::share_enum(ABI32, s(server), level, buf, cb, pread, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ShareGetInfo(server: *const u8, name: *const u8, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    unsafe { netapi::share_get_info(ABI32, s(server), s(name), level, buf, cb, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ServerEnum2(server: *const u8, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8, typ: u32, domain: *const u8) -> u32 {
    let _ = server;
    unsafe { netapi::server_enum2(ABI32, level, buf, cb, pread, ptotal, typ, s(domain)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ServerGetInfo(server: *const u8, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    unsafe { netapi::server_get_info(ABI32, s(server), level, buf, cb, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32WkstaGetInfo(server: *const u8, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    unsafe { netapi::wksta_get_info(ABI32, s(server), level, buf, cb, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32WkstaSetUID2(
    _reserved: *const u8,
    domain: *const u8,
    user: *const u8,
    password: *const u8,
    _parms: *const u8,
    _force: u32,
    _level: u32,
    _buf: *mut u8,
    _cb: u32,
    ptotal: *mut u8,
) -> u32 {
    unsafe { netapi::put_count(ABI32, ptotal, 0) };
    netapi::wksta_set_uid2(s(domain), s(user), s(password))
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ServiceInstall(server: *const u8, service: *const u8, args: *const u8, buf: *mut u8, cb: u32) -> u32 {
    let _ = server;
    let a = unsafe { netapi::arg_list(args) };
    netapi::service_install(s(service), &a, buf, cb)
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ServiceControl(server: *const u8, service: *const u8, opcode: u32, _arg: u32, buf: *mut u8, cb: u32) -> u32 {
    let _ = server;
    netapi::service_control(s(service), opcode & 0xFF, buf, cb)
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ServiceEnum(server: *const u8, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    let _ = server;
    unsafe { netapi::service_enum(ABI32, level, buf, cb, pread, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ServiceGetInfo(server: *const u8, service: *const u8, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    let _ = server;
    unsafe { netapi::service_get_info(ABI32, s(service), level, buf, cb, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ConfigGet2(server: *const u8, _reserved: *const u8, component: *const u8, param: *const u8, buf: *mut u8, cb: u32, plen: *mut u8) -> u32 {
    let _ = server;
    unsafe { netapi::config_get2(ABI32, s(component), s(param), buf, cb, plen) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ConfigGetAll2(server: *const u8, _reserved: *const u8, component: *const u8, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    let _ = server;
    unsafe { netapi::config_get_all2(ABI32, s(component), buf, cb, pread, ptotal) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32ConfigSet(server: *const u8, _r1: u32, component: *const u8, _r2: u32, _r3: u32, buf: *const u8, _cb: u32, _r4: u32) -> u32 {
    let _ = server;
    unsafe { netapi::config_set(s(component), buf) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32AccessGetUserPerms(server: *const u8, _ugname: *const u8, resource: *const u8, pperms: *mut u8) -> u32 {
    let _ = server;
    unsafe { netapi::access_get_user_perms(ABI32, s(resource), pperms) }
}

#[unsafe(no_mangle)]
pub extern "C" fn Net32GetDCName(_server: *const u8, _domain: *const u8, _buf: *mut u8, _cb: u32) -> u32 {
    netapi::get_dc_name()
}

/// NetUserValidate2: the logon check is the server's (at the first
/// connection); accepted here.
#[unsafe(no_mangle)]
pub extern "C" fn Net32UserValidate2(_server: *const u8, _level: u32, _buf: *mut u8, _cb: u32, _r2: u32, ptotal: *mut u8, _r: *const u8) -> u32 {
    unsafe { netapi::put_count(ABI32, ptotal, 0) };
    if lanreq::status().started { 0 } else { lanreq::NERR_WKSTA_NOT_STARTED }
}

/// Net32PackString(dst, src): helper of IBM's thunks; copies a string.
#[unsafe(no_mangle)]
pub extern "C" fn Net32PackString(dst: *mut u8, src: *const u8) -> u32 {
    if dst.is_null() || src.is_null() {
        return 0;
    }
    let mut n = 0;
    unsafe {
        loop {
            let c = *src.add(n);
            *dst.add(n) = c;
            if c == 0 {
                break;
            }
            n += 1;
        }
    }
    n as u32
}
