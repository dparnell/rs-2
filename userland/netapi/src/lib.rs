//! NETAPI.DLL: the 16-bit LAN Server API (`_pascal far`, 16:16 pointers,
//! USHORT sizes and counts) on rs2's LAN requester, with the names and
//! ordinals of IBM's NETAPI.DLL (LAN Server 5.0 requester). Each export
//! is a 16-bit stub (src/entries.rs, generated from netapi.tsv) that
//! `thunk16` brings to `dispatch16`; the work is `lanreq::netapi`, shared
//! with NETAPI32.DLL. The calls rs2 has nothing for answer
//! ERROR_NOT_SUPPORTED.

#![no_std]
#![no_main]
#![allow(clippy::missing_safety_doc, non_upper_case_globals)]

extern crate alloc;
extern crate os2;

use alloc::string::String;
use lanreq::netapi::{self, str_at, Abi};
use thunk16::Pascal;

mod data;
mod entries;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

fn far_of(flat: u32) -> u32 {
    thunk16::far(flat as *const u8)
}

fn flat_of(far: u32) -> u32 {
    thunk16::flat(far) as u32
}

const ABI16: Abi = Abi { out: far_of, inp: flat_of, short_counts: true };
const ERROR_NOT_SUPPORTED: u32 = 50;

/// The string a far pointer argument points at.
fn s(p: *mut u8) -> Option<String> {
    unsafe { str_at(p) }
}

extern "C" fn dispatch16(ord: u32, args: *const u8) -> u32 {
    let size = match ord {
        73 => 12,
        74 => 10,
        75 | 76 | 67 | 68 | 58 | 116 => 20,
        363 => 28,
        55 | 84 | 172 => 16,
        277 => 34,
        59 | 57 | 25 | 26 => 18,
        196 | 197 => 26,
        189 => 14,
        _ => return ERROR_NOT_SUPPORTED,
    };
    let mut a = Pascal::new(args, size);
    unsafe {
        match ord {
            // NetUseAdd(server, sLevel, buf, cbBuf)
            73 => {
                let (srv, lvl, buf, cb) = (a.p(), a.i(), a.p(), a.w());
                netapi::use_add(ABI16, s(srv), lvl as u32, buf, cb as u32)
            }
            // NetUseDel(server, device, usForce)
            74 => {
                let (srv, dev, force) = (a.p(), a.p(), a.w());
                netapi::use_del(s(srv), s(dev), force as u32)
            }
            // NetUseEnum(server, sLevel, buf, cbBuf, pcEntriesRead, pcTotalAvail)
            75 => {
                let (srv, lvl, buf, cb, pr, pt) = (a.p(), a.i(), a.p(), a.w(), a.p(), a.p());
                netapi::use_enum(ABI16, s(srv), lvl as u32, buf, cb as u32, pr, pt)
            }
            // NetUseGetInfo(server, usename, sLevel, buf, cbBuf, pcbTotalAvail)
            76 => {
                let (srv, name, lvl, buf, cb, pt) = (a.p(), a.p(), a.i(), a.p(), a.w(), a.p());
                netapi::use_get_info(ABI16, s(srv), s(name), lvl as u32, buf, cb as u32, pt)
            }
            // NetShareEnum(server, sLevel, buf, cbBuf, pcEntriesRead, pcTotalAvail)
            67 => {
                let (srv, lvl, buf, cb, pr, pt) = (a.p(), a.i(), a.p(), a.w(), a.p(), a.p());
                netapi::share_enum(ABI16, s(srv), lvl as u32, buf, cb as u32, pr, pt)
            }
            // NetShareGetInfo(server, netname, sLevel, buf, cbBuf, pcbTotalAvail)
            68 => {
                let (srv, name, lvl, buf, cb, pt) = (a.p(), a.p(), a.i(), a.p(), a.w(), a.p());
                netapi::share_get_info(ABI16, s(srv), s(name), lvl as u32, buf, cb as u32, pt)
            }
            // NetServerEnum2(server, sLevel, buf, cbBuf, pcEntriesRead, pcTotalEntries, flServerType, pszDomain)
            363 => {
                let (_srv, lvl, buf, cb, pr, pt, typ, dom) = (a.p(), a.i(), a.p(), a.w(), a.p(), a.p(), a.l(), a.p());
                netapi::server_enum2(ABI16, lvl as u32, buf, cb as u32, pr, pt, typ, s(dom))
            }
            // NetServerGetInfo(server, sLevel, buf, cbBuf, pcbTotalAvail)
            55 => {
                let (srv, lvl, buf, cb, pt) = (a.p(), a.i(), a.p(), a.w(), a.p());
                netapi::server_get_info(ABI16, s(srv), lvl as u32, buf, cb as u32, pt)
            }
            // NetWkstaGetInfo(server, sLevel, buf, cbBuf, pcbTotalAvail)
            84 => {
                let (srv, lvl, buf, cb, pt) = (a.p(), a.i(), a.p(), a.w(), a.p());
                netapi::wksta_get_info(ABI16, s(srv), lvl as u32, buf, cb as u32, pt)
            }
            // NetWkstaSetUID2(reserved, domain, user, password, parms, usLogoffForce, sLevel, buf, cbBuf, pcbTotalAvail)
            277 => {
                let (_r, dom, user, pw, _parms, _force, _lvl, _buf, _cb, pt) = (a.p(), a.p(), a.p(), a.p(), a.p(), a.w(), a.i(), a.p(), a.w(), a.p());
                netapi::put_count(ABI16, pt, 0);
                netapi::wksta_set_uid2(s(dom), s(user), s(pw))
            }
            // NetServiceInstall(server, service, cmdArgs, buf, cbBuf)
            59 => {
                let (_srv, svc, args, buf, cb) = (a.p(), a.p(), a.p(), a.p(), a.w());
                let list = netapi::arg_list(args);
                netapi::service_install(s(svc), &list, buf, cb as u32)
            }
            // NetServiceControl(server, service, opcode, arg, buf, cbBuf)
            57 => {
                let (_srv, svc, op, _arg, buf, cb) = (a.p(), a.p(), a.w(), a.w(), a.p(), a.w());
                netapi::service_control(s(svc), op as u32 & 0xFF, buf, cb as u32)
            }
            // NetServiceEnum(server, sLevel, buf, cbBuf, pcEntriesRead, pcTotalAvail)
            58 => {
                let (_srv, lvl, buf, cb, pr, pt) = (a.p(), a.i(), a.p(), a.w(), a.p(), a.p());
                netapi::service_enum(ABI16, lvl as u32, buf, cb as u32, pr, pt)
            }
            // NetServiceGetInfo(server, service, sLevel, buf, cbBuf, pcbTotalAvail)
            116 => {
                let (_srv, svc, lvl, buf, cb, pt) = (a.p(), a.p(), a.i(), a.p(), a.w(), a.p());
                netapi::service_get_info(ABI16, s(svc), lvl as u32, buf, cb as u32, pt)
            }
            // NetConfigGet(component, parameter, buf, cbBuf, pcbParmlen)
            25 => {
                let (comp, param, buf, cb, pl) = (a.p(), a.p(), a.p(), a.w(), a.p());
                netapi::config_get2(ABI16, s(comp), s(param), buf, cb as u32, pl)
            }
            // NetConfigGetAll(component, buf, cbBuf, pcbReturned, pcbTotalAvail)
            26 => {
                let (comp, buf, cb, pr, pt) = (a.p(), a.p(), a.w(), a.p(), a.p());
                netapi::config_get_all2(ABI16, s(comp), buf, cb as u32, pr, pt)
            }
            // NetConfigGet2(server, reserved, component, parameter, buf, cbBuf, pcbParmlen)
            196 => {
                let (_srv, _r, comp, param, buf, cb, pl) = (a.p(), a.p(), a.p(), a.p(), a.p(), a.w(), a.p());
                netapi::config_get2(ABI16, s(comp), s(param), buf, cb as u32, pl)
            }
            // NetConfigGetAll2(server, reserved, component, buf, cbBuf, pcbReturned, pcbTotalAvail)
            197 => {
                let (_srv, _r, comp, buf, cb, pr, pt) = (a.p(), a.p(), a.p(), a.p(), a.w(), a.p(), a.p());
                netapi::config_get_all2(ABI16, s(comp), buf, cb as u32, pr, pt)
            }
            // NetAccessGetUserPerms(server, ugname, resource, pusPerms)
            172 => {
                let (_srv, _ug, res, pp) = (a.p(), a.p(), a.p(), a.p());
                netapi::access_get_user_perms(ABI16, s(res), pp)
            }
            // NetGetDCName(server, domain, buf, cbBuf)
            189 => netapi::get_dc_name(),
            _ => ERROR_NOT_SUPPORTED,
        }
    }
}
