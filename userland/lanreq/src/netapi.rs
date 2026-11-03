//! The LAN Server API (Net*) on rs2's requester, shared by NETAPI32.DLL
//! (32-bit `_System`, flat pointers, ULONG counts) and NETAPI.DLL (16-bit
//! `_pascal far`, 16:16 pointers, USHORT counts). Structures are the
//! Toolkit's (`#pragma pack(1)`); enumerations put the fixed-size records
//! at the start of the caller's buffer and their strings at its end, and
//! answer ERROR_MORE_DATA when not everything fits (the records that do
//! are returned), as LAN Server does.

use alloc::string::String;
use alloc::vec::Vec;

use crate::*;

/// How the caller's pointers look.
#[derive(Clone, Copy)]
pub struct Abi {
    /// A flat address as the caller's pointer.
    pub out: fn(u32) -> u32,
    /// The caller's pointer as a flat address.
    pub inp: fn(u32) -> u32,
    /// Counts (*pcEntriesRead, ...) are USHORTs.
    pub short_counts: bool,
}

fn ident(x: u32) -> u32 {
    x
}

pub const ABI32: Abi = Abi { out: ident, inp: ident, short_counts: false };

/// A C string at flat `p` (None for null).
pub unsafe fn str_at(p: *const u8) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut s = String::new();
    let mut q = p;
    unsafe {
        while *q != 0 && s.len() < 1024 {
            s.push(*q as char);
            q = q.add(1);
        }
    }
    Some(s)
}

/// Store a count through the caller's pointer.
pub unsafe fn put_count(abi: Abi, p: *mut u8, v: u32) {
    if p.is_null() {
        return;
    }
    unsafe {
        if abi.short_counts {
            (p as *mut u16).write_unaligned(v.min(0xFFFF) as u16)
        } else {
            (p as *mut u32).write_unaligned(v)
        }
    }
}

/// Fills a caller's buffer: records from the front, strings from the back.
struct Packer {
    buf: *mut u8,
    len: usize,
    front: usize,
    back: usize,
    abi: Abi,
    /// Bytes everything would need.
    need: usize,
    overflow: bool,
}

impl Packer {
    fn new(buf: *mut u8, len: usize, abi: Abi) -> Packer {
        Packer { buf, len, front: 0, back: len, abi, need: 0, overflow: false }
    }

    /// Room for a record of `fixed` bytes with these strings?
    fn fits(&self, fixed: usize, strings: &[&str]) -> bool {
        let s: usize = strings.iter().map(|x| x.len() + 1).sum();
        self.front + fixed + s <= self.back
    }

    /// Put a string at the back; its pointer as the caller sees it.
    fn string(&mut self, s: &str) -> u32 {
        let b = cstr(s);
        self.back -= b.len();
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), self.buf.add(self.back), b.len()) };
        (self.abi.out)(self.buf as u32 + self.back as u32)
    }

    /// Put a record (its bytes, built with `string` pointers) at the front.
    fn record(&mut self, r: &[u8]) {
        unsafe { core::ptr::copy_nonoverlapping(r.as_ptr(), self.buf.add(self.front), r.len()) };
        self.front += r.len();
    }
}

fn fixed_str(s: &str, n: usize) -> Vec<u8> {
    let mut v: Vec<u8> = s.chars().take(n - 1).map(|c| c as u32 as u8).collect();
    v.resize(n, 0);
    v
}

fn le16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn le32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// Enumerate `items` with `make(packer, item) -> record bytes`, given the
/// record's fixed size and strings.
unsafe fn enumerate<T>(
    abi: Abi,
    buf: *mut u8,
    cb: u32,
    pread: *mut u8,
    ptotal: *mut u8,
    items: &[T],
    fixed: usize,
    strings: impl Fn(&T) -> Vec<String>,
    make: impl Fn(&mut Packer, &T) -> Vec<u8>,
) -> u32 {
    let mut p = Packer::new(buf, cb as usize, abi);
    let mut read = 0;
    for it in items {
        let ss = strings(it);
        let refs: Vec<&str> = ss.iter().map(|s| s.as_str()).collect();
        p.need += fixed + refs.iter().map(|s| s.len() + 1).sum::<usize>();
        if p.overflow || buf.is_null() || !p.fits(fixed, &refs) {
            p.overflow = true;
            continue;
        }
        let r = make(&mut p, it);
        p.record(&r);
        read += 1;
    }
    unsafe {
        put_count(abi, pread, read);
        put_count(abi, ptotal, items.len() as u32);
    }
    if p.overflow { ERROR_MORE_DATA } else { NERR_SUCCESS }
}

/// One record for GetInfo calls: NERR_BufTooSmall when even the fixed
/// part does not fit, ERROR_MORE_DATA when the strings do not.
unsafe fn info(abi: Abi, buf: *mut u8, cb: u32, ptotal: *mut u8, fixed: usize, strings: &[String], make: impl Fn(&mut Packer) -> Vec<u8>) -> u32 {
    let need = fixed + strings.iter().map(|s| s.len() + 1).sum::<usize>();
    unsafe { put_count(abi, ptotal, need as u32) };
    if buf.is_null() || (cb as usize) < fixed {
        return NERR_BUF_TOO_SMALL;
    }
    let mut p = Packer::new(buf, cb as usize, abi);
    let refs: Vec<&str> = strings.iter().map(|s| s.as_str()).collect();
    if !p.fits(fixed, &refs) {
        return ERROR_MORE_DATA;
    }
    let r = make(&mut p);
    p.record(&r);
    NERR_SUCCESS
}

/// A local server name (NULL, "" or our own) or a remote one.
fn is_local(server: &Option<String>) -> bool {
    match server {
        None => true,
        Some(s) => {
            let n = s.trim_start_matches('\\');
            n.is_empty() || n.eq_ignore_ascii_case(&status().computer)
        }
    }
}

fn server_name(s: &str) -> String {
    s.trim_start_matches(['\\', '/']).to_ascii_uppercase()
}

// ---- Uses ----

/// NetUseAdd: levels 1 (use_info_1) and 0 is not allowed.
pub unsafe fn use_add(abi: Abi, server: Option<String>, level: u32, buf: *const u8, _cb: u32) -> u32 {
    if !is_local(&server) {
        return ERROR_NOT_SUPPORTED;
    }
    if level != 1 {
        return ERROR_INVALID_LEVEL;
    }
    if buf.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let local: String = unsafe { core::slice::from_raw_parts(buf, 9) }.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    let rd = |off: usize| unsafe { (buf.add(off) as *const u32).read_unaligned() };
    let remote = unsafe { str_at((abi.inp)(rd(10)) as *const u8) };
    let password = unsafe { str_at((abi.inp)(rd(14)) as *const u8) };
    let Some(remote) = remote else { return ERROR_INVALID_PARAMETER };
    match crate::use_add(local.trim_end_matches(':'), &remote, password.as_deref(), None) {
        Ok(()) => NERR_SUCCESS,
        Err(e) => e,
    }
}

pub fn use_del(server: Option<String>, dev: Option<String>, force: u32) -> u32 {
    if !is_local(&server) {
        return ERROR_NOT_SUPPORTED;
    }
    let Some(d) = dev else { return ERROR_INVALID_PARAMETER };
    match crate::use_del(d.trim_end_matches(':'), force != 0) {
        Ok(()) => NERR_SUCCESS,
        Err(e) => e,
    }
}

const USE_INFO_0: usize = 14;
const USE_INFO_1: usize = 26;

fn use_record(p: &mut Packer, u: &UseInfo, level: u32) -> Vec<u8> {
    let mut r = fixed_str(&u.local, 9);
    r.push(0);
    let rem = p.string(&u.remote);
    le32(&mut r, rem);
    if level == 1 {
        le32(&mut r, 0);
        le16(&mut r, u.status as u16);
        le16(&mut r, u.typ as u16);
        le16(&mut r, u.opens as u16);
        le16(&mut r, 1);
    }
    r
}

pub unsafe fn use_enum(abi: Abi, server: Option<String>, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    if !is_local(&server) {
        return ERROR_NOT_SUPPORTED;
    }
    let fixed = match level {
        0 => USE_INFO_0,
        1 => USE_INFO_1,
        _ => return ERROR_INVALID_LEVEL,
    };
    let v = match uses() {
        Ok(v) => v,
        Err(e) => return e,
    };
    unsafe { enumerate(abi, buf, cb, pread, ptotal, &v, fixed, |u| alloc::vec![u.remote.clone()], |p, u| use_record(p, u, level)) }
}

pub unsafe fn use_get_info(abi: Abi, server: Option<String>, name: Option<String>, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    if !is_local(&server) {
        return ERROR_NOT_SUPPORTED;
    }
    let fixed = match level {
        0 => USE_INFO_0,
        1 => USE_INFO_1,
        _ => return ERROR_INVALID_LEVEL,
    };
    let Some(name) = name else { return ERROR_INVALID_PARAMETER };
    let n = name.trim_end_matches(':').to_ascii_uppercase();
    let v = match uses() {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(u) = v.iter().find(|u| u.local.trim_end_matches(':') == n || (u.local.is_empty() && u.remote.eq_ignore_ascii_case(&n))) else {
        return NERR_USE_NOT_FOUND;
    };
    unsafe { info(abi, buf, cb, ptotal, fixed, &[u.remote.clone()], |p| use_record(p, u, level)) }
}

// ---- Shares and servers ----

pub unsafe fn share_enum(abi: Abi, server: Option<String>, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    let fixed = match level {
        0 => 13,
        1 => 20,
        _ => return ERROR_INVALID_LEVEL,
    };
    let Some(srv) = server.filter(|_| true) else { return NERR_NET_NOT_STARTED };
    if is_local(&Some(srv.clone())) {
        // rs2 has no server.
        return NERR_SERVER_NOT_STARTED;
    }
    let v = match shares(&server_name(&srv)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    unsafe {
        enumerate(abi, buf, cb, pread, ptotal, &v, fixed, |s| if level == 1 { alloc::vec![s.remark.clone()] } else { Vec::new() }, |p, s| share_record(p, s, level))
    }
}

fn share_record(p: &mut Packer, s: &Share, level: u32) -> Vec<u8> {
    let mut r = fixed_str(&s.name, 13);
    if level == 1 {
        r.push(0);
        le16(&mut r, s.typ as u16);
        let rem = p.string(&s.remark);
        le32(&mut r, rem);
    }
    r
}

pub unsafe fn share_get_info(abi: Abi, server: Option<String>, netname: Option<String>, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    let fixed = match level {
        0 => 13,
        1 => 20,
        _ => return ERROR_INVALID_LEVEL,
    };
    let (Some(srv), Some(name)) = (server, netname) else { return ERROR_INVALID_PARAMETER };
    let v = match shares(&server_name(&srv)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(s) = v.iter().find(|s| s.name.eq_ignore_ascii_case(&name)) else { return NERR_NET_NAME_NOT_FOUND };
    let strings = if level == 1 { alloc::vec![s.remark.clone()] } else { Vec::new() };
    unsafe { info(abi, buf, cb, ptotal, fixed, &strings, |p| share_record(p, s, level)) }
}

pub const SV_TYPE_WORKSTATION: u32 = 0x1;
pub const SV_TYPE_SERVER: u32 = 0x2;
pub const SV_TYPE_DOMAIN_ENUM: u32 = 0x8000_0000;

fn server_record(p: &mut Packer, name: &str, typ: u32, comment: &str, level: u32) -> Vec<u8> {
    let mut r = fixed_str(name, 16);
    if level == 1 {
        r.push(4);
        r.push(0);
        le32(&mut r, typ);
        let c = p.string(comment);
        le32(&mut r, c);
    }
    r
}

/// NetServerEnum2: the servers of a domain (or, with SV_TYPE_DOMAIN_ENUM,
/// the domains/workgroups seen).
pub unsafe fn server_enum2(abi: Abi, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8, typ: u32, domain: Option<String>) -> u32 {
    let fixed = match level {
        0 => 16,
        1 => 26,
        _ => return ERROR_INVALID_LEVEL,
    };
    let dom = domain.filter(|d| !d.is_empty());
    if typ & SV_TYPE_DOMAIN_ENUM != 0 && typ != 0xFFFF_FFFF {
        let v = match servers(None) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let mut groups: Vec<String> = v.iter().map(|s| s.workgroup.clone()).collect();
        let st = status();
        groups.push(st.workgroup.clone());
        groups.extend(st.other_domains.split(',').map(String::from).filter(|s| !s.is_empty()));
        groups.sort();
        groups.dedup();
        return unsafe { enumerate(abi, buf, cb, pread, ptotal, &groups, fixed, |_| alloc::vec![String::new()], |p, g| server_record(p, g, SV_TYPE_DOMAIN_ENUM, "", level)) };
    }
    let v = match servers(dom.as_deref()) {
        Ok(v) => v,
        Err(e) => return e,
    };
    unsafe {
        enumerate(abi, buf, cb, pread, ptotal, &v, fixed, |s| alloc::vec![s.comment.clone()], |p, s| server_record(p, &s.name, s.typ | SV_TYPE_SERVER, &s.comment, level))
    }
}

pub unsafe fn server_get_info(abi: Abi, server: Option<String>, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    let fixed = match level {
        0 => 16,
        1 => 26,
        _ => return ERROR_INVALID_LEVEL,
    };
    if is_local(&server) {
        return NERR_SERVER_NOT_STARTED;
    }
    let name = server_name(&server.unwrap_or_default());
    // The server must answer.
    if let Err(e) = shares(&name) {
        return e;
    }
    let comment = servers(None).ok().and_then(|v| v.into_iter().find(|s| s.name == name)).map(|s| s.comment).unwrap_or_default();
    unsafe { info(abi, buf, cb, ptotal, fixed, &[comment.clone()], |p| server_record(p, &name, SV_TYPE_SERVER | SV_TYPE_WORKSTATION, &comment, level)) }
}

// ---- Workstation ----

/// NetWkstaGetInfo level 10 (wksta_info_10).
pub unsafe fn wksta_get_info(abi: Abi, server: Option<String>, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    if !is_local(&server) {
        return ERROR_NOT_SUPPORTED;
    }
    if level != 10 {
        return ERROR_INVALID_LEVEL;
    }
    let st = status();
    if !st.started {
        return NERR_WKSTA_NOT_STARTED;
    }
    let strings = [st.computer.clone(), st.user.clone(), st.workgroup.clone(), st.domain.clone(), st.other_domains.clone()];
    unsafe {
        info(abi, buf, cb, ptotal, 22, &strings, |p| {
            let mut r = Vec::new();
            for (i, s) in strings.iter().enumerate() {
                if i == 3 {
                    r.push(5);
                    r.push(0);
                }
                let at = p.string(s);
                le32(&mut r, at);
            }
            r
        })
    }
}

/// NetWkstaSetUID2: log on (a user) or off (no user).
pub fn wksta_set_uid2(domain: Option<String>, user: Option<String>, password: Option<String>) -> u32 {
    if !status().started {
        return NERR_WKSTA_NOT_STARTED;
    }
    let r = match user.filter(|u| !u.is_empty()) {
        Some(u) => logon(&u, &password.unwrap_or_default(), &domain.unwrap_or_default()),
        None => logon("", "", ""),
    };
    r.err().unwrap_or(NERR_SUCCESS)
}

// ---- Services ----

fn is_requester(s: &str) -> bool {
    matches!(s.to_ascii_uppercase().as_str(), "REQUESTER" | "WORKSTATION" | "WKSTA")
}

fn service_info(level: u32, running: bool) -> Vec<u8> {
    let mut r = fixed_str("REQUESTER", 16);
    if level >= 1 {
        le16(&mut r, if running { 3 } else { 0 });
        le32(&mut r, 0);
        le16(&mut r, 0);
    }
    if level >= 2 {
        r.extend_from_slice(&[0u8; 64]);
    }
    r
}

fn put_record(buf: *mut u8, cb: u32, r: &[u8]) -> u32 {
    if buf.is_null() || (cb as usize) < r.len() {
        return NERR_BUF_TOO_SMALL;
    }
    unsafe { core::ptr::copy_nonoverlapping(r.as_ptr(), buf, r.len()) };
    NERR_SUCCESS
}

/// NetServiceInstall("REQUESTER", args): start the requester.
pub fn service_install(service: Option<String>, args: &[String], buf: *mut u8, cb: u32) -> u32 {
    let s = service.unwrap_or_default();
    if !is_requester(&s) {
        return NERR_BAD_SERVICE_NAME;
    }
    if status().started {
        return NERR_SERVICE_INSTALLED;
    }
    if let Err(e) = start_requester(args) {
        return e;
    }
    if !buf.is_null() {
        let _ = put_record(buf, cb, &service_info(2, true));
    }
    NERR_SUCCESS
}

/// NetServiceControl: INTERROGATE (0) and UNINSTALL (3) of the requester.
pub fn service_control(service: Option<String>, opcode: u32, buf: *mut u8, cb: u32) -> u32 {
    if !is_requester(&service.unwrap_or_default()) {
        return NERR_BAD_SERVICE_NAME;
    }
    let running = status().started;
    if !running {
        return NERR_SERVICE_NOT_INSTALLED;
    }
    match opcode {
        0 => {}
        3 => {
            if let Err(e) = stop(true) {
                return e;
            }
        }
        _ => return NERR_SERVICE_CTL_NOT_VALID,
    }
    if !buf.is_null() {
        let _ = put_record(buf, cb, &service_info(2, opcode != 3));
    }
    NERR_SUCCESS
}

pub unsafe fn service_enum(abi: Abi, level: u32, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    if level > 2 {
        return ERROR_INVALID_LEVEL;
    }
    let running = status().started;
    let r = service_info(level, true);
    let n = running as u32;
    unsafe {
        put_count(abi, ptotal, n);
        if n == 0 {
            put_count(abi, pread, 0);
            return NERR_SUCCESS;
        }
        if buf.is_null() || (cb as usize) < r.len() {
            put_count(abi, pread, 0);
            return ERROR_MORE_DATA;
        }
        core::ptr::copy_nonoverlapping(r.as_ptr(), buf, r.len());
        put_count(abi, pread, 1);
    }
    NERR_SUCCESS
}

pub unsafe fn service_get_info(abi: Abi, service: Option<String>, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u8) -> u32 {
    if level > 2 {
        return ERROR_INVALID_LEVEL;
    }
    if !is_requester(&service.unwrap_or_default()) {
        return NERR_BAD_SERVICE_NAME;
    }
    if !status().started {
        return NERR_SERVICE_NOT_INSTALLED;
    }
    let r = service_info(level, true);
    unsafe { put_count(abi, ptotal, r.len() as u32) };
    put_record(buf, cb, &r)
}

// ---- Configuration (IBMLAN.INI) ----

/// NetConfigGet2: one value of IBMLAN.INI.
pub unsafe fn config_get2(abi: Abi, component: Option<String>, param: Option<String>, buf: *mut u8, cb: u32, plen: *mut u8) -> u32 {
    let (Some(c), Some(k)) = (component, param) else { return ERROR_INVALID_PARAMETER };
    let ini = ini::Ini::load(&ini::path());
    let Some(v) = ini.get(&c, &k) else {
        return if ini.text().to_ascii_lowercase().contains(&alloc::format!("[{}]", c.to_ascii_lowercase())) { NERR_CFG_PARAM_NOT_FOUND } else { NERR_CFG_COMP_NOT_FOUND };
    };
    let b = cstr(v);
    unsafe { put_count(abi, plen, b.len() as u32) };
    if buf.is_null() || (cb as usize) < b.len() {
        return NERR_BUF_TOO_SMALL;
    }
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), buf, b.len()) };
    NERR_SUCCESS
}

/// NetConfigGetAll2: `key=value\0` ... `\0` of a component.
pub unsafe fn config_get_all2(abi: Abi, component: Option<String>, buf: *mut u8, cb: u32, pread: *mut u8, ptotal: *mut u8) -> u32 {
    let Some(c) = component else { return ERROR_INVALID_PARAMETER };
    let text = ini::Ini::load(&ini::path()).text();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut found = false;
    for l in text.lines() {
        let t = l.trim();
        if let Some(s) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            cur = String::from(s.trim());
            found |= cur.eq_ignore_ascii_case(&c);
            continue;
        }
        if cur.eq_ignore_ascii_case(&c) && !t.starts_with(';') {
            if let Some((k, v)) = t.split_once('=') {
                out.extend(cstr(&alloc::format!("{}={}", k.trim(), v.trim())));
            }
        }
    }
    if !found {
        return NERR_CFG_COMP_NOT_FOUND;
    }
    out.push(0);
    unsafe {
        put_count(abi, ptotal, out.len() as u32);
        if buf.is_null() || (cb as usize) < out.len() {
            put_count(abi, pread, 0);
            return NERR_BUF_TOO_SMALL;
        }
        core::ptr::copy_nonoverlapping(out.as_ptr(), buf, out.len());
        put_count(abi, pread, out.len() as u32);
    }
    NERR_SUCCESS
}

/// Net32ConfigSet: write one value (the buffer holds key and value pointers).
pub unsafe fn config_set(component: Option<String>, buf: *const u8) -> u32 {
    let Some(c) = component else { return ERROR_INVALID_PARAMETER };
    if buf.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let (kp, vp) = unsafe { ((buf as *const u32).read_unaligned(), (buf.add(4) as *const u32).read_unaligned()) };
    let (Some(k), v) = (unsafe { str_at(kp as *const u8) }, unsafe { str_at(vp as *const u8) }) else { return ERROR_INVALID_PARAMETER };
    let path = ini::path();
    let mut ini = ini::Ini::load(&path);
    ini.set(&c, &k, &v.unwrap_or_default());
    match utils::files::write_file(&path, &ini.text().chars().map(|c| c as u32 as u8).collect::<Vec<u8>>()) {
        Ok(()) => NERR_SUCCESS,
        Err(e) => e,
    }
}

// ---- Access ----

pub const ACCESS_ALL: u32 = 0x7F;

/// NetAccessGetUserPerms: what the logged-on user may do with a resource.
/// rs2 asks the server: a resource it can see gives full rights (the
/// server still checks every operation).
pub unsafe fn access_get_user_perms(abi: Abi, resource: Option<String>, pperms: *mut u8) -> u32 {
    let Some(r) = resource else { return ERROR_INVALID_PARAMETER };
    let ok = utils::files::exists(&r) || r.len() <= 3;
    unsafe { put_count(abi, pperms, if ok { ACCESS_ALL } else { 0 }) };
    if ok { NERR_SUCCESS } else { NERR_RESOURCE_NOT_FOUND }
}

/// NetGetDCName: workgroups have no domain controller.
pub fn get_dc_name() -> u32 {
    NERR_DC_NOT_FOUND
}

/// The `\0`-separated, `\0\0`-terminated argument list of NetServiceInstall.
pub unsafe fn arg_list(p: *const u8) -> Vec<String> {
    let mut v = Vec::new();
    if p.is_null() {
        return v;
    }
    let mut q = p;
    unsafe {
        loop {
            let Some(s) = str_at(q) else { break };
            if s.is_empty() {
                break;
            }
            q = q.add(s.len() + 1);
            v.extend(s.split_whitespace().map(String::from));
        }
    }
    v
}
