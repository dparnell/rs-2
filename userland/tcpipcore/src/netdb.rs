//! The network database: hosts (HOSTS file, then DNS), services,
//! protocols and networks. Results live in per-thread buffers, as the
//! 4.1+ DLLs keep them; the *ent() iterators are per process.

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::null_mut;

use crate::cstr::{self, eq_ignore_case};
use crate::errno::{self, HOST_NOT_FOUND, NO_RECOVERY};
use crate::files;
use crate::inet;
use crate::sync::Lock;
use crate::tls;

pub const AF_INET: i32 = 2;

#[repr(C)]
pub struct Hostent {
    pub h_name: *mut u8,
    pub h_aliases: *mut *mut u8,
    pub h_addrtype: i32,
    pub h_length: i32,
    pub h_addr_list: *mut *mut u8,
}

#[repr(C)]
pub struct Servent {
    pub s_name: *mut u8,
    pub s_aliases: *mut *mut u8,
    /// Network byte order.
    pub s_port: i32,
    pub s_proto: *mut u8,
}

#[repr(C)]
pub struct Protoent {
    pub p_name: *mut u8,
    pub p_aliases: *mut *mut u8,
    pub p_proto: i32,
}

#[repr(C)]
pub struct Netent {
    pub n_name: *mut u8,
    pub n_aliases: *mut *mut u8,
    pub n_addrtype: i32,
    pub n_net: u32,
}

/// A result structure and the memory its pointers point into.
pub struct EntBuf<T> {
    pub ent: T,
    strings: Vec<Vec<u8>>,
    lists: Vec<Vec<*mut u8>>,
    addrs: Vec<[u8; 4]>,
}

impl<T> EntBuf<T> {
    pub fn new() -> EntBuf<T> {
        EntBuf { ent: unsafe { core::mem::zeroed() }, strings: Vec::new(), lists: Vec::new(), addrs: Vec::new() }
    }

    fn reset(&mut self) {
        self.strings.clear();
        self.lists.clear();
        self.addrs.clear();
    }

    fn str(&mut self, s: &[u8]) -> *mut u8 {
        let mut v = cstr::z(s);
        let p = v.as_mut_ptr();
        self.strings.push(v);
        p
    }

    fn list(&mut self, items: &[&[u8]]) -> *mut *mut u8 {
        let mut l: Vec<*mut u8> = items.iter().map(|s| self.str(s)).collect();
        l.push(null_mut());
        let p = l.as_mut_ptr();
        self.lists.push(l);
        p
    }
}

impl<T> Default for EntBuf<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// One line of a database file.
#[derive(Clone)]
pub struct Record {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    pub aliases: Vec<Vec<u8>>,
}

impl Record {
    fn matches(&self, n: &[u8]) -> bool {
        eq_ignore_case(&self.name, n) || self.aliases.iter().any(|a| eq_ignore_case(a, n))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Db {
    Hosts,
    Services,
    Protocols,
    Networks,
}

const DEFAULT_HOSTS: &[u8] = b"127.0.0.1 localhost\n";

const DEFAULT_SERVICES: &[u8] = b"echo 7/tcp\necho 7/udp\ndiscard 9/tcp sink null\ndiscard 9/udp sink null\n\
systat 11/tcp users\ndaytime 13/tcp\ndaytime 13/udp\nnetstat 15/tcp\nqotd 17/tcp quote\nchargen 19/tcp ttytst source\n\
chargen 19/udp ttytst source\nftp-data 20/tcp\nftp 21/tcp\nssh 22/tcp\ntelnet 23/tcp\nsmtp 25/tcp mail\ntime 37/tcp timserver\n\
time 37/udp timserver\nrlp 39/udp resource\nnameserver 42/tcp name\nwhois 43/tcp nicname\ndomain 53/tcp nameserver\n\
domain 53/udp nameserver\nbootps 67/udp\nbootpc 68/udp\ntftp 69/udp\ngopher 70/tcp\nfinger 79/tcp\nhttp 80/tcp www www-http\n\
kerberos 88/tcp\npop2 109/tcp\npop3 110/tcp pop\nsunrpc 111/tcp portmapper\nsunrpc 111/udp portmapper\nauth 113/tcp ident\n\
nntp 119/tcp usenet\nntp 123/udp\nnetbios-ns 137/udp\nnetbios-dgm 138/udp\nnetbios-ssn 139/tcp\nimap 143/tcp\nsnmp 161/udp\n\
snmp-trap 162/udp\nirc 194/tcp\nldap 389/tcp\nhttps 443/tcp\nexec 512/tcp\nbiff 512/udp comsat\nlogin 513/tcp\nwho 513/udp whod\n\
shell 514/tcp cmd\nsyslog 514/udp\nprinter 515/tcp spooler\ntalk 517/udp\nntalk 518/udp\nroute 520/udp router routed\n\
timed 525/udp timeserver\nuucp 540/tcp uucpd\nsocks 1080/tcp\n";

const DEFAULT_PROTOCOLS: &[u8] = b"ip 0 IP\nicmp 1 ICMP\nigmp 2 IGMP\nggp 3 GGP\ntcp 6 TCP\negp 8 EGP\npup 12 PUP\nudp 17 UDP\n\
idp 22 IDP\nraw 255 RAW\n";

const DEFAULT_NETWORKS: &[u8] = b"loopback 127\n";

fn load(db: Db) -> Vec<Record> {
    let (file, alt, default): (&[u8], &[u8], &[u8]) = match db {
        Db::Hosts => (b"HOSTS", b"HOSTS", DEFAULT_HOSTS),
        Db::Services => (b"SERVICES", b"SERVICES", DEFAULT_SERVICES),
        Db::Protocols => (b"PROTOCOL", b"PROTOCOLS", DEFAULT_PROTOCOLS),
        Db::Networks => (b"NETWORKS", b"NETWORKS", DEFAULT_NETWORKS),
    };
    let text = files::read_etc(file).or_else(|| files::read_etc(alt));
    let text = text.as_deref().unwrap_or(default);
    let mut v: Vec<Record> = files::records(text)
        .filter(|f| f.len() >= 2)
        .map(|f| {
            // HOSTS lines are "address name aliases", the others "name
            // value aliases".
            let (name, value) = if db == Db::Hosts { (f[1], f[0]) } else { (f[0], f[1]) };
            Record { name: name.to_vec(), value: value.to_vec(), aliases: f[2..].iter().map(|a| a.to_vec()).collect() }
        })
        .collect();
    if db == Db::Hosts && !v.iter().any(|r| r.matches(b"localhost")) {
        v.push(Record { name: b"localhost".to_vec(), value: b"127.0.0.1".to_vec(), aliases: Vec::new() });
    }
    v
}

// ---- hosts ----

/// The host name (%HOSTNAME%, else "rs2").
pub fn hostname() -> Vec<u8> {
    os2::net::hostname().chars().map(|c| c as u8).collect()
}

/// The address of the first configured non-loopback interface.
pub fn primary_address() -> Option<[u8; 4]> {
    os2::net::interfaces().into_iter().find(|i| i.flags & os2::net::IF_LOOPBACK == 0 && i.addr != [0; 4]).map(|i| i.addr)
}

fn fill_host(name: &[u8], aliases: &[Vec<u8>], addrs: &[[u8; 4]]) -> *mut Hostent {
    let t = tls::get();
    let b = &mut t.host;
    b.reset();
    b.addrs.extend_from_slice(addrs);
    let h_name = b.str(name);
    let al: Vec<&[u8]> = aliases.iter().map(|a| a.as_slice()).collect();
    let h_aliases = b.list(&al);
    let mut ptrs: Vec<*mut u8> = (0..b.addrs.len()).map(|i| b.addrs[i].as_mut_ptr()).collect();
    ptrs.push(null_mut());
    let h_addr_list = ptrs.as_mut_ptr();
    b.lists.push(ptrs);
    b.ent = Hostent { h_name, h_aliases, h_addrtype: AF_INET, h_length: 4, h_addr_list };
    &mut b.ent
}

pub unsafe fn gethostbyname(name: *const u8) -> *mut Hostent {
    let n = unsafe { cstr::bytes(name) }.trim_ascii();
    let n = n.strip_suffix(b".").unwrap_or(n);
    if n.is_empty() {
        errno::set_h(HOST_NOT_FOUND);
        return null_mut();
    }
    if let Some(a) = inet::aton(n) {
        errno::set_h(0);
        return fill_host(n, &[], &[a]);
    }
    if let Some(r) = load(Db::Hosts).into_iter().find(|r| r.matches(n)) {
        let addrs: Vec<[u8; 4]> = load(Db::Hosts).iter().filter(|x| x.matches(n)).filter_map(|x| inet::aton(&x.value)).collect();
        if !addrs.is_empty() {
            errno::set_h(0);
            return fill_host(&r.name, &r.aliases, &addrs);
        }
    }
    if eq_ignore_case(n, &hostname()) {
        if let Some(a) = primary_address() {
            errno::set_h(0);
            return fill_host(n, &[], &[a]);
        }
    }
    let z = cstr::z(n);
    let mut addrs = [[0u8; 4]; 16];
    let mut count = 0u32;
    let rc = unsafe { os2::net::Rs2NetResolve(z.as_ptr(), addrs.as_mut_ptr(), addrs.len() as u32, &mut count) };
    if rc != 0 || count == 0 {
        errno::set_h(if rc == 0 { errno::NO_DATA } else { rc as i32 });
        return null_mut();
    }
    errno::set_h(0);
    fill_host(n, &[], &addrs[..count as usize])
}

pub unsafe fn gethostbyaddr(addr: *const u8, len: i32, typ: i32) -> *mut Hostent {
    if addr.is_null() || len != 4 || typ != AF_INET {
        errno::set_h(NO_RECOVERY);
        return null_mut();
    }
    let mut a = [0u8; 4];
    unsafe { core::ptr::copy_nonoverlapping(addr, a.as_mut_ptr(), 4) };
    if let Some(r) = load(Db::Hosts).into_iter().find(|r| inet::aton(&r.value) == Some(a)) {
        errno::set_h(0);
        return fill_host(&r.name, &r.aliases, &[a]);
    }
    if a[0] == 127 {
        errno::set_h(0);
        return fill_host(b"localhost", &[], &[a]);
    }
    if os2::net::interfaces().iter().any(|i| i.addr == a) {
        errno::set_h(0);
        return fill_host(&hostname(), &[], &[a]);
    }
    match crate::resolv::lookup_ptr(a) {
        Ok(name) => {
            errno::set_h(0);
            fill_host(&name, &[], &[a])
        }
        Err(h) => {
            errno::set_h(h);
            null_mut()
        }
    }
}

struct Iter {
    list: Option<Vec<Record>>,
    at: usize,
    stay_open: bool,
}

impl Iter {
    const fn new() -> Iter {
        Iter { list: None, at: 0, stay_open: false }
    }

    fn next(&mut self, db: Db) -> Option<Record> {
        let l = self.list.get_or_insert_with(|| load(db));
        let r = l.get(self.at).cloned();
        self.at += 1;
        r
    }
}

static HOST_ITER: Lock<Iter> = Lock::new(Iter::new());
static SERV_ITER: Lock<Iter> = Lock::new(Iter::new());
static PROTO_ITER: Lock<Iter> = Lock::new(Iter::new());
static NET_ITER: Lock<Iter> = Lock::new(Iter::new());

fn set_iter(i: &Lock<Iter>, stay_open: i32) {
    let mut g = i.lock();
    g.list = None;
    g.at = 0;
    g.stay_open = stay_open != 0;
}

fn end_iter(i: &Lock<Iter>) {
    let mut g = i.lock();
    g.list = None;
    g.at = 0;
}

pub fn sethostent(f: i32) {
    set_iter(&HOST_ITER, f)
}
pub fn endhostent() {
    end_iter(&HOST_ITER)
}
pub fn gethostent() -> *mut Hostent {
    loop {
        let Some(r) = HOST_ITER.lock().next(Db::Hosts) else { return null_mut() };
        if let Some(a) = inet::aton(&r.value) {
            return fill_host(&r.name, &r.aliases, &[a]);
        }
    }
}

// ---- services ----

fn fill_serv(r: &Record) -> Option<*mut Servent> {
    let (port, proto) = split_port(&r.value)?;
    let t = tls::get();
    let b = &mut t.serv;
    b.reset();
    let s_name = b.str(&r.name);
    let al: Vec<&[u8]> = r.aliases.iter().map(|a| a.as_slice()).collect();
    let s_aliases = b.list(&al);
    let s_proto = b.str(proto);
    b.ent = Servent { s_name, s_aliases, s_port: (port as u16).to_be() as i32, s_proto };
    Some(&mut b.ent)
}

fn split_port(v: &[u8]) -> Option<(u32, &[u8])> {
    let i = v.iter().position(|&c| c == b'/')?;
    let port = core::str::from_utf8(&v[..i]).ok()?.parse::<u32>().ok()?;
    Some((port, &v[i + 1..]))
}

fn proto_ok(r: &Record, proto: &[u8]) -> bool {
    proto.is_empty() || split_port(&r.value).is_some_and(|(_, p)| eq_ignore_case(p, proto))
}

pub unsafe fn getservbyname(name: *const u8, proto: *const u8) -> *mut Servent {
    let (n, p) = unsafe { (cstr::bytes(name), cstr::bytes(proto)) };
    load(Db::Services).iter().find(|r| r.matches(n) && proto_ok(r, p)).and_then(fill_serv).unwrap_or(null_mut())
}

pub unsafe fn getservbyport(port: i32, proto: *const u8) -> *mut Servent {
    let p = unsafe { cstr::bytes(proto) };
    let want = u16::from_be(port as u16) as u32;
    load(Db::Services)
        .iter()
        .find(|r| split_port(&r.value).is_some_and(|(n, _)| n == want) && proto_ok(r, p))
        .and_then(fill_serv)
        .unwrap_or(null_mut())
}

pub fn setservent(f: i32) {
    set_iter(&SERV_ITER, f)
}
pub fn endservent() {
    end_iter(&SERV_ITER)
}
pub fn getservent() -> *mut Servent {
    loop {
        let Some(r) = SERV_ITER.lock().next(Db::Services) else { return null_mut() };
        if let Some(p) = fill_serv(&r) {
            return p;
        }
    }
}

// ---- protocols ----

fn fill_proto(r: &Record) -> Option<*mut Protoent> {
    let n = core::str::from_utf8(&r.value).ok()?.parse::<i32>().ok()?;
    let t = tls::get();
    let b = &mut t.proto;
    b.reset();
    let p_name = b.str(&r.name);
    let al: Vec<&[u8]> = r.aliases.iter().map(|a| a.as_slice()).collect();
    let p_aliases = b.list(&al);
    b.ent = Protoent { p_name, p_aliases, p_proto: n };
    Some(&mut b.ent)
}

pub unsafe fn getprotobyname(name: *const u8) -> *mut Protoent {
    let n = unsafe { cstr::bytes(name) };
    load(Db::Protocols).iter().find(|r| r.matches(n)).and_then(fill_proto).unwrap_or(null_mut())
}

pub fn getprotobynumber(num: i32) -> *mut Protoent {
    load(Db::Protocols)
        .iter()
        .find(|r| core::str::from_utf8(&r.value).ok().and_then(|v| v.parse::<i32>().ok()) == Some(num))
        .and_then(fill_proto)
        .unwrap_or(null_mut())
}

pub fn setprotoent(f: i32) {
    set_iter(&PROTO_ITER, f)
}
pub fn endprotoent() {
    end_iter(&PROTO_ITER)
}
pub fn getprotoent() -> *mut Protoent {
    loop {
        let Some(r) = PROTO_ITER.lock().next(Db::Protocols) else { return null_mut() };
        if let Some(p) = fill_proto(&r) {
            return p;
        }
    }
}

// ---- networks ----

fn fill_net(r: &Record) -> Option<*mut Netent> {
    let z = cstr::z(&r.value);
    let n = unsafe { inet::inet_network(z.as_ptr()) };
    if n == inet::INADDR_NONE {
        return None;
    }
    let t = tls::get();
    let b = &mut t.net;
    b.reset();
    let n_name = b.str(&r.name);
    let al: Vec<&[u8]> = r.aliases.iter().map(|a| a.as_slice()).collect();
    let n_aliases = b.list(&al);
    b.ent = Netent { n_name, n_aliases, n_addrtype: AF_INET, n_net: n };
    Some(&mut b.ent)
}

pub unsafe fn getnetbyname(name: *const u8) -> *mut Netent {
    let n = unsafe { cstr::bytes(name) };
    load(Db::Networks).iter().find(|r| r.matches(n)).and_then(fill_net).unwrap_or(null_mut())
}

pub fn getnetbyaddr(net: u32, typ: i32) -> *mut Netent {
    if typ != AF_INET {
        return null_mut();
    }
    load(Db::Networks)
        .iter()
        .find(|r| {
            let z = cstr::z(&r.value);
            unsafe { inet::inet_network(z.as_ptr()) == net }
        })
        .and_then(fill_net)
        .unwrap_or(null_mut())
}

pub fn setnetent(f: i32) {
    set_iter(&NET_ITER, f)
}
pub fn endnetent() {
    end_iter(&NET_ITER)
}
pub fn getnetent() -> *mut Netent {
    loop {
        let Some(r) = NET_ITER.lock().next(Db::Networks) else { return null_mut() };
        if let Some(p) = fill_net(&r) {
            return p;
        }
    }
}

/// gethostname(name, len).
pub unsafe fn gethostname(buf: *mut u8, len: i32) -> i32 {
    if buf.is_null() || len <= 0 {
        return errno::fail(errno::EFAULT);
    }
    unsafe { cstr::copy_out(&hostname(), buf, len as usize) };
    0
}

/// herror(s): "s: text" on standard error.
pub unsafe fn herror(s: *const u8) {
    let s = unsafe { cstr::bytes(s) };
    let mut v = vec![];
    if !s.is_empty() {
        v.extend_from_slice(s);
        v.extend_from_slice(b": ");
    }
    v.extend_from_slice(errno::hstrerror(errno::h()).as_bytes());
    v.extend_from_slice(b"\r\n");
    let mut n = 0;
    unsafe { os2::DosWrite(2, v.as_ptr(), v.len() as u32, &mut n) };
}
