//! The LAN requester's user side, shared by NET.EXE, NETAPI32.DLL and
//! NETAPI.DLL: the RS2API 600-608 calls into the kernel's redirector
//! (`kernel/src/api/lan.rs`), IBMLAN.INI, starting the requester, and
//! NET.MSG output.

#![no_std]
#![allow(non_snake_case, clippy::missing_safety_doc)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub mod ini;
pub mod msg;
pub mod netapi;

unsafe extern "C" {
    #[link_name = "RS2API.600"]
    fn Rs2LanStart(computer: *const u8, workgroup: *const u8, opts: *const u8) -> u32;
    #[link_name = "RS2API.601"]
    fn Rs2LanStop(force: u32) -> u32;
    #[link_name = "RS2API.602"]
    fn Rs2LanQuery(buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.603"]
    fn Rs2LanLogon(user: *const u8, password: *const u8, domain: *const u8) -> u32;
    #[link_name = "RS2API.604"]
    fn Rs2LanUseAdd(local: *const u8, remote: *const u8, password: *const u8, user: *const u8) -> u32;
    #[link_name = "RS2API.605"]
    fn Rs2LanUseDel(name: *const u8, force: u32) -> u32;
    #[link_name = "RS2API.606"]
    fn Rs2LanUseEnum(buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.607"]
    fn Rs2LanShareEnum(server: *const u8, buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.608"]
    fn Rs2LanServerEnum(domain: *const u8, buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
}

// NERR_ codes (neterr.h).
pub const NERR_SUCCESS: u32 = 0;
pub const NERR_NET_NOT_STARTED: u32 = 2102;
pub const NERR_UNKNOWN_SERVER: u32 = 2103;
pub const NERR_BUF_TOO_SMALL: u32 = 2123;
pub const NERR_WKSTA_NOT_STARTED: u32 = 2138;
pub const NERR_SERVICE_INSTALLED: u32 = 2182;
pub const NERR_SERVICE_NOT_INSTALLED: u32 = 2184;
pub const NERR_BAD_SERVICE_NAME: u32 = 2185;
pub const NERR_USE_NOT_FOUND: u32 = 2250;
pub const NERR_BAD_DEV: u32 = 2341;
pub const NERR_OPEN_FILES: u32 = 2401;
pub const NERR_BAD_PASSWORD_CORE: u32 = 2403;
pub const NERR_LOCAL_DRIVE: u32 = 2405;
pub const NERR_BAD_USERNAME: u32 = 2202;
pub const NERR_BAD_PASSWORD: u32 = 2203;
pub const NERR_NOT_LOGGED_ON: u32 = 2201;
pub const NERR_INVALID_COMPUTER: u32 = 2351;
pub const ERROR_MORE_DATA: u32 = 234;
pub const ERROR_NOT_SUPPORTED: u32 = 50;
pub const NERR_SERVER_NOT_STARTED: u32 = 2114;
pub const NERR_CFG_COMP_NOT_FOUND: u32 = 2146;
pub const NERR_CFG_PARAM_NOT_FOUND: u32 = 2147;
pub const NERR_SERVICE_CTL_NOT_VALID: u32 = 2191;
pub const NERR_RESOURCE_NOT_FOUND: u32 = 2222;
pub const NERR_NET_NAME_NOT_FOUND: u32 = 2310;
pub const NERR_DC_NOT_FOUND: u32 = 2453;
pub const ERROR_BUFFER_OVERFLOW: u32 = 111;
pub const ERROR_INVALID_PARAMETER: u32 = 87;
pub const ERROR_INVALID_LEVEL: u32 = 124;

/// A NUL-terminated copy.
pub fn cstr(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
    v.push(0);
    v
}

fn opt(s: &Option<Vec<u8>>) -> *const u8 {
    s.as_ref().map_or(core::ptr::null(), |v| v.as_ptr())
}

fn rc(r: u32) -> Result<(), u32> {
    if r == 0 { Ok(()) } else { Err(r) }
}

/// Call a text-returning entry, growing the buffer as needed.
fn text(mut f: impl FnMut(*mut u8, u32, *mut u32) -> u32) -> Result<String, u32> {
    let mut cb = 4096u32;
    loop {
        let mut buf = alloc::vec![0u8; cb as usize];
        let mut need = 0u32;
        match f(buf.as_mut_ptr(), cb, &mut need) {
            0 => {
                let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                return Ok(buf[..n].iter().map(|&b| b as char).collect());
            }
            ERROR_BUFFER_OVERFLOW if need > cb => cb = need,
            e => return Err(e),
        }
    }
}

fn rows(t: &str) -> Vec<Vec<String>> {
    t.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').map(String::from).collect()).collect()
}

pub fn start(computer: &str, workgroup: &str, opts: &str) -> Result<(), u32> {
    let (c, w, o) = (cstr(computer), cstr(workgroup), cstr(opts));
    rc(unsafe { Rs2LanStart(c.as_ptr(), w.as_ptr(), o.as_ptr()) })
}

pub fn stop(force: bool) -> Result<(), u32> {
    rc(unsafe { Rs2LanStop(force as u32) })
}

/// The requester's state.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub started: bool,
    pub computer: String,
    pub workgroup: String,
    pub user: String,
    pub domain: String,
    pub other_domains: String,
}

pub fn status() -> Status {
    let t = text(|b, cb, pcb| unsafe { Rs2LanQuery(b, cb, pcb) }).unwrap_or_default();
    let mut s = Status::default();
    for l in t.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        let v = v.to_string();
        match k {
            "started" => s.started = v == "1",
            "computer" => s.computer = v,
            "workgroup" => s.workgroup = v,
            "user" => s.user = v,
            "domain" => s.domain = v,
            "othdomains" => s.other_domains = v,
            _ => {}
        }
    }
    s
}

pub fn logon(user: &str, password: &str, domain: &str) -> Result<(), u32> {
    let (u, p, d) = (cstr(user), cstr(password), cstr(domain));
    rc(unsafe { Rs2LanLogon(u.as_ptr(), p.as_ptr(), d.as_ptr()) })
}

pub fn use_add(local: &str, remote: &str, password: Option<&str>, user: Option<&str>) -> Result<(), u32> {
    let (l, r) = (cstr(local), cstr(remote));
    let (p, u) = (password.map(cstr), user.map(cstr));
    rc(unsafe { Rs2LanUseAdd(l.as_ptr(), r.as_ptr(), opt(&p), opt(&u)) })
}

pub fn use_del(name: &str, force: bool) -> Result<(), u32> {
    let n = cstr(name);
    rc(unsafe { Rs2LanUseDel(n.as_ptr(), force as u32) })
}

/// Resource types (asg_type / shi1_type).
pub const TYPE_DISK: u32 = 0;
pub const TYPE_PRINT: u32 = 1;
pub const TYPE_COMM: u32 = 2;
pub const TYPE_IPC: u32 = 3;

#[derive(Clone, Debug)]
pub struct UseInfo {
    /// "X:", "LPT1" or "" (deviceless).
    pub local: String,
    pub remote: String,
    pub typ: u32,
    pub status: u32,
    pub opens: u32,
}

pub fn uses() -> Result<Vec<UseInfo>, u32> {
    let t = text(|b, cb, pcb| unsafe { Rs2LanUseEnum(b, cb, pcb) })?;
    Ok(rows(&t)
        .into_iter()
        .filter(|r| r.len() >= 5)
        .map(|r| UseInfo { local: r[0].clone(), remote: r[1].clone(), typ: r[2].parse().unwrap_or(0), status: r[3].parse().unwrap_or(0), opens: r[4].parse().unwrap_or(0) })
        .collect())
}

#[derive(Clone, Debug)]
pub struct Share {
    pub name: String,
    pub typ: u32,
    pub remark: String,
}

/// The shares of `\\server` (or `server`).
pub fn shares(server: &str) -> Result<Vec<Share>, u32> {
    let s = cstr(server);
    let t = text(|b, cb, pcb| unsafe { Rs2LanShareEnum(s.as_ptr(), b, cb, pcb) })?;
    Ok(rows(&t)
        .into_iter()
        .filter(|r| r.len() >= 3)
        .map(|r| Share { name: r[0].clone(), typ: r[1].parse().unwrap_or(0), remark: r[2].clone() })
        .collect())
}

#[derive(Clone, Debug)]
pub struct Server {
    pub name: String,
    pub workgroup: String,
    pub typ: u32,
    pub comment: String,
}

/// The servers of a workgroup (None: all known).
pub fn servers(domain: Option<&str>) -> Result<Vec<Server>, u32> {
    let d = domain.map(cstr);
    let t = text(|b, cb, pcb| unsafe { Rs2LanServerEnum(opt(&d), b, cb, pcb) })?;
    Ok(rows(&t)
        .into_iter()
        .filter(|r| r.len() >= 4)
        .map(|r| Server { name: r[0].clone(), workgroup: r[1].clone(), typ: r[2].parse().unwrap_or(0), comment: r[3].clone() })
        .collect())
}

/// Start the requester from IBMLAN.INI unless it runs. `cmdline`
/// switches (`/COMPUTERNAME:x`, `/DOMAIN:x`, `/OTHDOMAINS:a,b`) override
/// the file. Ok(true) when it was started now.
pub fn ensure_started(cmdline: &[String]) -> Result<bool, u32> {
    if status().started {
        return Ok(false);
    }
    start_requester(cmdline)?;
    Ok(true)
}

pub fn start_requester(cmdline: &[String]) -> Result<(), u32> {
    let ini = ini::Ini::load(&ini::path());
    let get = |key: &str, default: &str| -> String {
        for a in cmdline {
            if let Some((k, v)) = a.trim_start_matches('/').split_once(':') {
                if k.eq_ignore_ascii_case(key) {
                    return v.to_string();
                }
            }
        }
        ini.get("requester", key).map(String::from).unwrap_or_else(|| default.to_string())
    };
    let host = env("HOSTNAME").unwrap_or_else(|| String::from("RS2"));
    let computer = get("computername", &host);
    let domain = get("domain", "WORKGROUP");
    let mut opts = String::new();
    for (k, key) in [("othdomains", "othdomains"), ("wins", "wins"), ("smbport", "smbport"), ("nbport", "nbport"), ("lmhosts", "lmhosts")] {
        let v = get(key, "");
        if !v.is_empty() {
            opts.push_str(&alloc::format!("{k}={v};"));
        }
    }
    if get("lmhosts", "").is_empty() {
        opts.push_str(&alloc::format!("lmhosts={}\\LMHOSTS;", etc_dir()));
    }
    start(&computer, &domain, &opts)
}

use os2::dos::{DosQuerySysInfo, DosScanEnv};

/// An environment variable.
pub fn env(name: &str) -> Option<String> {
    let c = cstr(name);
    let mut v: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(c.as_ptr(), &mut v) } != 0 || v.is_null() {
        return None;
    }
    let mut s = String::new();
    let mut p = v;
    unsafe {
        while *p != 0 {
            s.push(*p as char);
            p = p.add(1);
        }
    }
    Some(s)
}

/// The boot drive's letter.
pub fn boot_drive() -> char {
    let mut d = 3u32;
    unsafe { DosQuerySysInfo(5, 5, &mut d, 4) };
    (b'A' + d.clamp(1, 26) as u8 - 1) as char
}

/// %ETC% (TCP/IP's configuration directory), else \MPTN\ETC.
pub fn etc_dir() -> String {
    env("ETC").map(|s| String::from(s.trim_end_matches('\\'))).unwrap_or_else(|| alloc::format!("{}:\\MPTN\\ETC", boot_drive()))
}

#[repr(C, align(16))]
#[derive(Default, Clone, Copy)]
struct KeyInfo {
    ch: u8,
    scan: u8,
    status: u8,
    nls_shift: u8,
    state: u16,
    time: u32,
}

unsafe extern "C" {
    #[link_name = "KBDCALLS.4"]
    fn KBDCHARIN();
}

fn key_in() -> Option<KeyInfo> {
    let mut k = KeyInfo::default();
    let p = os2::vio::ptr_words(&k);
    let rc = unsafe { os2::vio::call16(KBDCHARIN as *const () as usize, &[p[0], p[1], 0, 0]) };
    let k2 = unsafe { core::ptr::read_volatile(&mut k) };
    (rc == 0 && k2.status & 0x40 != 0).then_some(k2)
}

/// Read a password without echo (a line of redirected input as is).
pub fn read_password() -> Option<String> {
    if !utils::stdin_is_console() {
        let mut buf = Vec::new();
        loop {
            let mut c = [0u8; 1];
            let mut got = 0;
            if unsafe { os2::dos::DosRead(0, c.as_mut_ptr(), 1, &mut got) } != 0 || got == 0 || c[0] == b'\n' {
                break;
            }
            buf.push(c[0]);
        }
        os2::write_text(1, "\r\n");
        let s: String = buf.iter().map(|&b| b as char).collect();
        return Some(String::from(s.trim_end_matches('\r')));
    }
    let mut s = String::new();
    loop {
        let k = key_in()?;
        match k.ch {
            b'\r' | b'\n' => break,
            8 => {
                s.pop();
            }
            3 | 27 => {
                os2::write_text(1, "\r\n");
                return None;
            }
            0 | 0xe0 => {}
            c => s.push(c as char),
        }
    }
    os2::write_text(1, "\r\n");
    Some(s)
}
