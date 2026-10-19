//! The DNS resolver calls (res_init, res_query, res_search, res_mkquery,
//! res_send, dn_expand, dn_comp, ...), done in the DLL over a UDP socket.
//! Name servers and the domain come from %ETC%\RESOLV2 (or RESOLV:
//! `nameserver A.B.C.D`, `domain D`, `search D...`), else from the
//! interfaces' (DHCP) configuration.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::net::*;

use crate::cstr;
use crate::errno::{self, HOST_NOT_FOUND, NO_DATA, NO_RECOVERY, TRY_AGAIN};
use crate::files;
use crate::inet;

pub const MAXNS: usize = 3;
pub const MAXDNSRCH: usize = 6;

pub const RES_INIT: u32 = 0x1;
pub const RES_RECURSE: u32 = 0x40;
pub const RES_DEFNAMES: u32 = 0x80;
pub const RES_DNSRCH: u32 = 0x200;

pub const C_IN: i32 = 1;
pub const T_A: i32 = 1;
pub const T_PTR: i32 = 12;

/// `struct __res_state` (pack 4, 512 bytes): retrans 0, retry 4, options
/// 8, nscount 12, nsaddr_list[3] 16, id 64, defdname[256] 66, dnsrch[7]
/// 324, pfcode 352, ndots:4/nsort:4 356, sort_list 360.
#[repr(C, align(4))]
pub struct ResState(pub [u8; 512]);

const O_RETRANS: usize = 0;
const O_RETRY: usize = 4;
const O_OPTIONS: usize = 8;
const O_NSCOUNT: usize = 12;
const O_NSADDR: usize = 16;
const O_ID: usize = 64;
const O_DEFDNAME: usize = 66;
const O_DNSRCH: usize = 324;
const O_NDOTS: usize = 356;

/// `_ress[2]` (TCPIP32); `_res` is its first element (TCP32DLL).
#[unsafe(no_mangle)]
pub static mut tcpipcore_ress: [ResState; 2] = [const { ResState([0; 512]) }; 2];

/// Storage for the dnsrch pointers.
static mut SEARCH: [[u8; 256]; MAXDNSRCH] = [[0; 256]; MAXDNSRCH];

fn res() -> &'static mut [u8; 512] {
    unsafe { &mut (*core::ptr::addr_of_mut!(tcpipcore_ress))[0].0 }
}

fn rd32(o: usize) -> u32 {
    let r = res();
    u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]])
}

fn wr32(o: usize, v: u32) {
    res()[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

pub fn random_id() -> u16 {
    let mut v = NEXT_ID.load(Ordering::Relaxed);
    if v == 0 {
        v = os2::net::ms() ^ crate::tls::tid() << 16 | 1;
    }
    v ^= v << 13;
    v ^= v >> 17;
    v ^= v << 5;
    NEXT_ID.store(v, Ordering::Relaxed);
    v as u16
}

/// res_init(): read the configuration into `_res`.
pub fn res_init() -> i32 {
    let mut servers: Vec<[u8; 4]> = Vec::new();
    let mut domain: Vec<u8> = Vec::new();
    let mut search: Vec<Vec<u8>> = Vec::new();
    let text = files::read_etc(b"RESOLV2").or_else(|| files::read_etc(b"RESOLV")).or_else(|| files::read_etc(b"RESOLV.CONF"));
    if let Some(t) = &text {
        for f in files::records(t) {
            let key = f[0].to_ascii_lowercase();
            match (key.as_slice(), f.get(1)) {
                (b"nameserver", Some(a)) => {
                    if let Some(a) = inet::aton(a) {
                        if servers.len() < MAXNS {
                            servers.push(a);
                        }
                    }
                }
                (b"domain", Some(d)) => domain = d.to_vec(),
                (b"search", Some(_)) => search = f[1..].iter().take(MAXDNSRCH).map(|d| d.to_vec()).collect(),
                _ => {}
            }
        }
    }
    if servers.is_empty() {
        for i in os2::net::interfaces() {
            for d in &i.dns[..(i.ndns as usize).min(4)] {
                if servers.len() < MAXNS && !servers.contains(d) {
                    servers.push(*d);
                }
            }
        }
    }
    if domain.is_empty() {
        if let Some(d) = files::env(c"DOMAIN") {
            domain = d.trim_ascii().to_vec();
        }
    }
    if search.is_empty() && !domain.is_empty() {
        search.push(domain.clone());
    }
    let r = res();
    r.fill(0);
    wr32(O_RETRANS, 4);
    wr32(O_RETRY, 2);
    wr32(O_OPTIONS, RES_INIT | RES_RECURSE | RES_DEFNAMES | RES_DNSRCH);
    wr32(O_NSCOUNT, servers.len() as u32);
    for (i, s) in servers.iter().enumerate() {
        let o = O_NSADDR + 16 * i;
        r[o] = 2;
        r[o + 2..o + 4].copy_from_slice(&53u16.to_be_bytes());
        r[o + 4..o + 8].copy_from_slice(s);
    }
    r[O_ID..O_ID + 2].copy_from_slice(&random_id().to_le_bytes());
    let n = domain.len().min(255);
    r[O_DEFDNAME..O_DEFDNAME + n].copy_from_slice(&domain[..n]);
    for (i, d) in search.iter().enumerate() {
        let buf = unsafe { &mut (*core::ptr::addr_of_mut!(SEARCH))[i] };
        let n = d.len().min(255);
        buf[..n].copy_from_slice(&d[..n]);
        buf[n] = 0;
        let p = buf.as_ptr() as u32;
        r[O_DNSRCH + 4 * i..O_DNSRCH + 4 * i + 4].copy_from_slice(&p.to_le_bytes());
    }
    r[O_NDOTS] = 1;
    0
}

fn ensure_init() {
    if rd32(O_OPTIONS) & RES_INIT == 0 {
        res_init();
    }
}

fn servers() -> Vec<SockAddrIn> {
    let r = res();
    let n = (rd32(O_NSCOUNT) as usize).min(MAXNS);
    (0..n)
        .map(|i| {
            let o = O_NSADDR + 16 * i;
            let mut a = [0u8; 4];
            a.copy_from_slice(&r[o + 4..o + 8]);
            let port = u16::from_be_bytes([r[o + 2], r[o + 3]]);
            SockAddrIn::new(a, if port == 0 { 53 } else { port })
        })
        .filter(|s| s.addr != [0; 4])
        .collect()
}

fn search_list() -> Vec<Vec<u8>> {
    let r = res();
    let mut v = Vec::new();
    for i in 0..MAXDNSRCH {
        let o = O_DNSRCH + 4 * i;
        let p = u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]) as *const u8;
        if p.is_null() {
            break;
        }
        v.push(unsafe { cstr::bytes(p) }.to_vec());
    }
    v
}

// ---- message building and parsing ----

/// Encode a name as labels into `out`; None if malformed.
fn put_name(name: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let name = name.strip_suffix(b".").unwrap_or(name);
    if !name.is_empty() {
        for l in name.split(|&c| c == b'.') {
            if l.is_empty() || l.len() > 63 {
                return None;
            }
            out.push(l.len() as u8);
            out.extend_from_slice(l);
        }
    }
    out.push(0);
    Some(())
}

/// Expand the name at `at` of `msg`: the text and the bytes it takes at
/// `at`.
pub fn get_name(msg: &[u8], at: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    let mut p = at;
    let mut used = None;
    let mut hops = 0;
    loop {
        let len = *msg.get(p)? as usize;
        if len & 0xc0 == 0xc0 {
            let target = (len & 0x3f) << 8 | *msg.get(p + 1)? as usize;
            if used.is_none() {
                used = Some(p + 2 - at);
            }
            hops += 1;
            if hops > 64 {
                return None;
            }
            p = target;
            continue;
        }
        if len & 0xc0 != 0 {
            return None;
        }
        if len == 0 {
            if used.is_none() {
                used = Some(p + 1 - at);
            }
            break;
        }
        if !out.is_empty() {
            out.push(b'.');
        }
        out.extend_from_slice(msg.get(p + 1..p + 1 + len)?);
        p += 1 + len;
        if out.len() > 1024 {
            return None;
        }
    }
    Some((out, used?))
}

fn skip_name(msg: &[u8], at: usize) -> Option<usize> {
    let mut p = at;
    loop {
        let len = *msg.get(p)? as usize;
        if len & 0xc0 == 0xc0 {
            msg.get(p + 1)?;
            return Some(p + 2 - at);
        }
        if len == 0 {
            return Some(p + 1 - at);
        }
        p += 1 + len;
    }
}

/// A standard query for `name`.
pub fn make_query(name: &[u8], class: i32, typ: i32) -> Option<Vec<u8>> {
    ensure_init();
    let mut m = Vec::with_capacity(64);
    let id = random_id();
    m.extend_from_slice(&id.to_be_bytes());
    let rd = rd32(O_OPTIONS) & RES_RECURSE != 0;
    m.extend_from_slice(&[if rd { 1 } else { 0 }, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    put_name(name, &mut m)?;
    m.extend_from_slice(&(typ as u16).to_be_bytes());
    m.extend_from_slice(&(class as u16).to_be_bytes());
    Some(m)
}

/// Send a query to the name servers; the answer.
pub fn send(msg: &[u8]) -> Result<Vec<u8>, u32> {
    ensure_init();
    let ns = servers();
    if ns.is_empty() || msg.len() < 12 {
        return Err(errno::ECONNREFUSED);
    }
    let mut s = 0;
    let rc = unsafe { Rs2SockCreate(AF_INET, SOCK_DGRAM, 0, &mut s) };
    if rc != 0 {
        return Err(rc);
    }
    let retrans = rd32(O_RETRANS).clamp(1, 30) * 1000;
    let retry = rd32(O_RETRY).clamp(1, 8);
    let mut answer = alloc::vec![0u8; 4096];
    let mut result = Err(errno::ETIMEDOUT);
    'outer: for _ in 0..retry {
        for to in &ns {
            let mut sent = 0;
            if unsafe { Rs2SockSendTo(s, msg.as_ptr(), msg.len() as u32, 0, to, &mut sent) } != 0 {
                continue;
            }
            let start = os2::net::ms();
            loop {
                let left = retrans.saturating_sub(os2::net::ms().wrapping_sub(start));
                if left == 0 {
                    break;
                }
                let mut socks = [s];
                let mut n = 0;
                if unsafe { Rs2SockSelect(socks.as_mut_ptr(), 1, 0, 0, left as i32, &mut n) } != 0 || n == 0 {
                    break;
                }
                let mut from = SockAddrIn::default();
                let mut got = 0;
                let rc = unsafe { Rs2SockRecvFrom(s, answer.as_mut_ptr(), answer.len() as u32, 0, &mut from, &mut got) };
                if rc != 0 {
                    if rc == errno::ECONNREFUSED {
                        result = Err(rc);
                        break;
                    }
                    continue;
                }
                let a = &answer[..got as usize];
                if a.len() >= 12 && a[0..2] == msg[0..2] && a[2] & 0x80 != 0 {
                    result = Ok(a.to_vec());
                    break 'outer;
                }
            }
        }
    }
    unsafe { Rs2SockClose(s) };
    result
}

/// Query and check the answer's code: h_errno on failure.
pub fn query(name: &[u8], class: i32, typ: i32) -> Result<Vec<u8>, i32> {
    let Some(q) = make_query(name, class, typ) else { return Err(NO_RECOVERY) };
    let a = send(&q).map_err(|_| TRY_AGAIN)?;
    let rcode = a[3] & 0xf;
    let ancount = u16::from_be_bytes([a[6], a[7]]);
    match rcode {
        0 if ancount == 0 => Err(NO_DATA),
        0 => Ok(a),
        3 => Err(HOST_NOT_FOUND),
        2 => Err(TRY_AGAIN),
        _ => Err(NO_RECOVERY),
    }
}

/// res_search's order: the name as is when it has dots (or ends in one),
/// then with each search domain.
pub fn search(name: &[u8], class: i32, typ: i32) -> Result<Vec<u8>, i32> {
    ensure_init();
    if name.ends_with(b".") {
        return query(name, class, typ);
    }
    let dots = name.iter().filter(|&&c| c == b'.').count();
    let mut last = HOST_NOT_FOUND;
    if dots >= 1 {
        match query(name, class, typ) {
            Ok(a) => return Ok(a),
            Err(h) => last = h,
        }
    }
    if rd32(O_OPTIONS) & RES_DNSRCH != 0 {
        for d in search_list() {
            let mut n = name.to_vec();
            n.push(b'.');
            n.extend_from_slice(&d);
            match query(&n, class, typ) {
                Ok(a) => return Ok(a),
                Err(h) => last = h,
            }
        }
    }
    if dots == 0 {
        match query(name, class, typ) {
            Ok(a) => return Ok(a),
            Err(h) => last = h,
        }
    }
    Err(last)
}

/// The first answer record of type `typ`: its data offset and length.
fn first_answer(a: &[u8], typ: u16) -> Option<(usize, usize)> {
    let qd = u16::from_be_bytes([a[4], a[5]]) as usize;
    let an = u16::from_be_bytes([a[6], a[7]]) as usize;
    let mut p = 12;
    for _ in 0..qd {
        p += skip_name(a, p)? + 4;
    }
    for _ in 0..an {
        p += skip_name(a, p)?;
        let t = u16::from_be_bytes([*a.get(p)?, *a.get(p + 1)?]);
        let len = u16::from_be_bytes([*a.get(p + 8)?, *a.get(p + 9)?]) as usize;
        p += 10;
        if t == typ {
            return Some((p, len));
        }
        p += len;
    }
    None
}

/// The name of an address (PTR query).
pub fn lookup_ptr(a: [u8; 4]) -> Result<Vec<u8>, i32> {
    // "d.c.b.a.in-addr.arpa": the dotted quad of the reversed address.
    let mut n = inet::ntoa_vec([a[3], a[2], a[1], a[0]]);
    n.extend_from_slice(b".in-addr.arpa");
    let ans = query(&n, C_IN, T_PTR)?;
    let (at, _) = first_answer(&ans, T_PTR as u16).ok_or(NO_DATA)?;
    let (name, _) = get_name(&ans, at).ok_or(NO_RECOVERY)?;
    Ok(name)
}

// ---- the C entry points ----

pub unsafe fn res_mkquery(op: i32, dname: *const u8, class: i32, typ: i32, _data: *const u8, _datalen: i32, _newrr: *const u8, buf: *mut u8, buflen: i32) -> i32 {
    if op != 0 {
        return -1;
    }
    let Some(q) = make_query(unsafe { cstr::bytes(dname) }, class, typ) else { return -1 };
    if buf.is_null() || (buflen as usize) < q.len() {
        return -1;
    }
    unsafe { core::ptr::copy_nonoverlapping(q.as_ptr(), buf, q.len()) };
    q.len() as i32
}

unsafe fn put_answer(a: &[u8], answer: *mut u8, anslen: i32) -> i32 {
    if answer.is_null() || anslen <= 0 {
        return -1;
    }
    let n = a.len().min(anslen as usize);
    unsafe { core::ptr::copy_nonoverlapping(a.as_ptr(), answer, n) };
    a.len() as i32
}

pub unsafe fn res_send(msg: *const u8, len: i32, answer: *mut u8, anslen: i32) -> i32 {
    if msg.is_null() || len < 12 {
        return errno::fail(errno::EINVAL);
    }
    let m = unsafe { core::slice::from_raw_parts(msg, len as usize) };
    match send(m) {
        Ok(a) => unsafe { put_answer(&a, answer, anslen) },
        Err(e) => errno::fail(e),
    }
}

pub unsafe fn res_query(name: *const u8, class: i32, typ: i32, answer: *mut u8, anslen: i32) -> i32 {
    match query(unsafe { cstr::bytes(name) }, class, typ) {
        Ok(a) => {
            errno::set_h(0);
            unsafe { put_answer(&a, answer, anslen) }
        }
        Err(h) => {
            errno::set_h(h);
            -1
        }
    }
}

pub unsafe fn res_search(name: *const u8, class: i32, typ: i32, answer: *mut u8, anslen: i32) -> i32 {
    match search(unsafe { cstr::bytes(name) }, class, typ) {
        Ok(a) => {
            errno::set_h(0);
            unsafe { put_answer(&a, answer, anslen) }
        }
        Err(h) => {
            errno::set_h(h);
            -1
        }
    }
}

pub unsafe fn res_querydomain(name: *const u8, domain: *const u8, class: i32, typ: i32, answer: *mut u8, anslen: i32) -> i32 {
    let mut n = unsafe { cstr::bytes(name) }.to_vec();
    let d = unsafe { cstr::bytes(domain) };
    if !d.is_empty() {
        n.push(b'.');
        n.extend_from_slice(d);
    }
    let z = cstr::z(&n);
    unsafe { res_query(z.as_ptr(), class, typ, answer, anslen) }
}

/// dn_expand(msg, eom, comp_dn, exp_dn, length).
pub unsafe fn dn_expand(msg: *const u8, eom: *const u8, comp: *const u8, exp: *mut u8, length: i32) -> i32 {
    if msg.is_null() || eom.is_null() || comp.is_null() || (eom as usize) < (msg as usize) || (comp as usize) < (msg as usize) {
        return -1;
    }
    let m = unsafe { core::slice::from_raw_parts(msg, eom as usize - msg as usize) };
    match get_name(m, comp as usize - msg as usize) {
        Some((name, used)) if (name.len() as i32) < length => {
            unsafe { cstr::copy_out(&name, exp, length as usize) };
            used as i32
        }
        _ => -1,
    }
}

/// dn_comp(exp_dn, comp_dn, length, dnptrs, lastdnptr): no compression.
pub unsafe fn dn_comp(exp: *const u8, comp: *mut u8, length: i32, _dnptrs: *mut *mut u8, _last: *mut *mut u8) -> i32 {
    let mut v = Vec::new();
    if put_name(unsafe { cstr::bytes(exp) }, &mut v).is_none() || comp.is_null() || v.len() > length as usize {
        return -1;
    }
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), comp, v.len()) };
    v.len() as i32
}

pub unsafe fn dn_skipname(comp: *const u8, eom: *const u8) -> i32 {
    if comp.is_null() || (eom as usize) <= (comp as usize) {
        return -1;
    }
    let m = unsafe { core::slice::from_raw_parts(comp, eom as usize - comp as usize) };
    skip_name(m, 0).map_or(-1, |n| n as i32)
}

pub unsafe fn getshort(p: *const u8) -> u32 {
    unsafe { u16::from_be_bytes([*p, *p.add(1)]) as u32 }
}

pub unsafe fn getlong(p: *const u8) -> u32 {
    unsafe { u32::from_be_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]) }
}

pub unsafe fn putshort(v: u32, p: *mut u8) {
    unsafe { core::ptr::copy_nonoverlapping((v as u16).to_be_bytes().as_ptr(), p, 2) };
}

pub unsafe fn putlong(v: u32, p: *mut u8) {
    unsafe { core::ptr::copy_nonoverlapping(v.to_be_bytes().as_ptr(), p, 4) };
}

pub fn p_class(c: i32) -> &'static [u8] {
    match c {
        1 => b"IN\0",
        3 => b"CHAOS\0",
        4 => b"HESIOD\0",
        255 => b"ANY\0",
        _ => b"?\0",
    }
}

pub fn p_type(t: i32) -> &'static [u8] {
    match t {
        1 => b"A\0",
        2 => b"NS\0",
        5 => b"CNAME\0",
        6 => b"SOA\0",
        11 => b"WKS\0",
        12 => b"PTR\0",
        13 => b"HINFO\0",
        15 => b"MX\0",
        16 => b"TXT\0",
        28 => b"AAAA\0",
        33 => b"SRV\0",
        252 => b"AXFR\0",
        255 => b"ANY\0",
        _ => b"?\0",
    }
}
