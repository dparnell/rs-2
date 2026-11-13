//! The LAN requester's redirector: network drives, UNC paths and
//! redirected printer ports on SMB servers (`crates/smb`), the role of
//! IBM's NETWKSTA.200 and its "LAN" file system.
//!
//! * The requester is started (`start`) with this machine's NetBIOS name,
//!   its workgroup and options; `logon` sets the user name, password and
//!   domain sessions use.
//! * A [`Server`] holds one SMB session behind a sleeping lock: every
//!   operation is a request/response pair on it, so the I/O sleeps in the
//!   socket layer and other threads run meanwhile. A transport failure
//!   (or a session the server dropped) reconnects once and retries; open
//!   files reopen themselves after a reconnect.
//! * [`RedirFs`] is the file system of one share: mounted on a drive by
//!   NET USE, or made for `\\SERVER\SHARE` paths (path.rs maps each share
//!   named in a UNC path to a pseudo-drive from [`UNC_BASE`] up).
//! * Uses (`use_add`, `use_del`, `uses`): drives, LPT1-LPT9 and
//!   deviceless connections, what NetUseAdd/Del/Enum report.
//! * Names resolve through dotted quads, LMHOSTS, NBNS (a WINS server, or
//!   a broadcast) and DNS; port 445 is tried first, then the NetBIOS
//!   session service on 139.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use smb::{Credentials, Disposition, OpenArgs, Session, Transport};
use tcpip::{Endpoint, Ipv4};

use super::{attr, Ea, FileInfo, FileSystem, FsFile, SetInfo, Stamp, VolumeInfo};
use crate::net::socket;
use crate::sync::{Mutex, SpinLock};

/// Pseudo-drives for `\\SERVER\SHARE` paths start here.
pub const UNC_BASE: u8 = 32;

// Errors the requester reports beyond the file system's.
pub const ERROR_BAD_NETPATH: u32 = 53;
pub const ERROR_BAD_NET_NAME: u32 = 67;
pub const ERROR_ALREADY_ASSIGNED: u32 = 85;
pub const ERROR_INVALID_PASSWORD: u32 = 86;
pub const ERROR_UNEXP_NET_ERR: u32 = 59;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const ERROR_INVALID_DRIVE: u32 = 15;
pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_NOT_SUPPORTED: u32 = 50;
pub const NERR_WKSTA_NOT_STARTED: u32 = 2138;
pub const NERR_USE_NOT_FOUND: u32 = 2250;
pub const NERR_OPEN_FILES: u32 = 2401;
pub const NERR_BAD_DEV: u32 = 2341;
pub const NERR_NAME_NOT_FOUND: u32 = 2273;

/// How long connecting to a server may take, and a request.
const CONNECT_MS: u64 = 6000;
const REQUEST_S: u32 = 45;

// ---- Configuration ----

#[derive(Clone, Default)]
pub struct Config {
    pub started: bool,
    /// This machine's NetBIOS name and its workgroup/domain.
    pub computer: String,
    pub workgroup: String,
    /// The logged-on user (empty: none; servers see a guest).
    pub user: String,
    pub password: String,
    pub domain: String,
    /// WINS server (and port, 137 normally) for NBNS queries; None:
    /// broadcast.
    pub wins: Option<(Ipv4, u16)>,
    pub smb_port: u16,
    pub nb_port: u16,
    /// The LMHOSTS file.
    pub lmhosts: String,
    /// Extra workgroups to browse (OTHDOMAINS).
    pub other_domains: Vec<String>,
}

static CONFIG: SpinLock<Option<Config>> = SpinLock::new(None);

/// The requester's administration (start, stop, logon, NET USE add and
/// delete): one at a time, held across the network I/O of a use, so a
/// check (the letter is free, the last use of a server goes) stays true
/// until it is acted on. Taken before fs::MOUNTING and the servers'
/// connections.
static ADMIN: crate::sync::Mutex<()> = crate::sync::Mutex::new(());

pub fn config() -> Option<Config> {
    CONFIG.lock().clone().filter(|c| c.started)
}

/// Start the requester. `opts`: `key=value` separated by `;`: wins=a.b.c.d[:port],
/// smbport=, nbport=, lmhosts=path, othdomains=a,b.
pub fn start(computer: &str, workgroup: &str, opts: &str) -> Result<(), u32> {
    let _a = ADMIN.lock();
    let mut c = Config {
        started: true,
        computer: smb::wire::upper(computer),
        workgroup: smb::wire::upper(workgroup),
        smb_port: 445,
        nb_port: 139,
        lmhosts: String::from("C:\\MPTN\\ETC\\LMHOSTS"),
        ..Default::default()
    };
    if let Some(old) = CONFIG.lock().as_ref() {
        c.user = old.user.clone();
        c.password = old.password.clone();
        c.domain = old.domain.clone();
    }
    for kv in opts.split(';') {
        let Some((k, v)) = kv.split_once('=') else { continue };
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "wins" if !v.is_empty() => {
                let (a, p) = v.split_once(':').unwrap_or((v, "137"));
                if let Some(ip) = smb::netbios::parse_ipv4(a) {
                    c.wins = Some((Ipv4(ip), p.parse().unwrap_or(137)));
                }
            }
            "smbport" => c.smb_port = v.parse().unwrap_or(445),
            "nbport" => c.nb_port = v.parse().unwrap_or(139),
            "lmhosts" if !v.is_empty() => c.lmhosts = v.to_string(),
            "othdomains" => c.other_domains = v.split(',').map(|s| smb::wire::upper(s.trim())).filter(|s| !s.is_empty()).collect(),
            _ => {}
        }
    }
    crate::println!("requester: started as {} in {}", c.computer, c.workgroup);
    *CONFIG.lock() = Some(c);
    Ok(())
}

/// Stop the requester: every use is deleted, every session ends.
pub fn stop() {
    let _a = ADMIN.lock();
    let all: Vec<Use> = USES.lock().drain(..).collect();
    for u in all {
        if let Local::Drive(d) = u.local {
            super::unmount(d);
        }
    }
    let servers: Vec<Arc<Server>> = SERVERS.lock().values().cloned().collect();
    for s in servers {
        s.disconnect();
    }
    SERVERS.lock().clear();
    if let Some(c) = CONFIG.lock().as_mut() {
        c.started = false;
    }
}

/// LOGON (user, password, domain) or LOGOFF (empty user).
pub fn logon(user: &str, password: &str, domain: &str) -> Result<(), u32> {
    let _a = ADMIN.lock();
    let mut g = CONFIG.lock();
    let c = g.as_mut().filter(|c| c.started).ok_or(NERR_WKSTA_NOT_STARTED)?;
    c.user = user.to_string();
    c.password = password.to_string();
    c.domain = if domain.is_empty() { c.workgroup.clone() } else { smb::wire::upper(domain) };
    Ok(())
}

// ---- Transport ----

/// A TCP connection of the redirector (sockets owned by no process).
struct Tcp(u32);

const OWNER: u32 = 0;

impl Transport for Tcp {
    fn send(&mut self, d: &[u8]) -> smb::Result<()> {
        let mut at = 0;
        while at < d.len() {
            match socket::send(OWNER, self.0, &d[at..], 0, None) {
                Ok(0) => return Err(smb::Error::Closed),
                Ok(n) => at += n,
                Err(e) => return Err(smb::Error::Io(e)),
            }
        }
        Ok(())
    }
    fn recv(&mut self, b: &mut [u8]) -> smb::Result<usize> {
        let (v, _) = socket::recv(OWNER, self.0, b.len(), 0).map_err(smb::Error::Io)?;
        b[..v.len()].copy_from_slice(&v);
        Ok(v.len())
    }
}

impl Drop for Tcp {
    fn drop(&mut self) {
        let _ = socket::close(OWNER, self.0);
    }
}

struct KEnv;

impl smb::Env for KEnv {
    fn now(&self) -> u64 {
        smb::FILETIME_1970 + crate::rtc::now_ms() * 10_000
    }
    fn random(&self, b: &mut [u8]) {
        let n = crate::chardev::random_fill(b);
        let mut x = crate::irq::uptime_ms() ^ 0x9e37_79b9_7f4a_7c15 ^ crate::rtc::now_ms().rotate_left(17);
        for v in &mut b[n..] {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *v = x as u8;
        }
    }
}

fn timeval(secs: u32) -> [u8; 8] {
    let mut t = [0u8; 8];
    t[..4].copy_from_slice(&secs.to_le_bytes());
    t
}

/// A TCP connection to `ep`, or None after CONNECT_MS.
fn tcp_connect(ep: Endpoint) -> Result<Tcp, u32> {
    let s = socket::create(OWNER, socket::AF_INET, socket::SOCK_STREAM, 0)?;
    let t = Tcp(s);
    socket::ioctl(OWNER, s, socket::FIONBIO, 1)?;
    match socket::connect(OWNER, s, ep) {
        Ok(()) => {}
        Err(socket::EINPROGRESS) => {
            let mut set = [s];
            let n = socket::select(OWNER, &mut set, 0, 1, CONNECT_MS as i32)?;
            if n == 0 {
                return Err(60); // ETIMEDOUT
            }
            let e = socket::get_option(OWNER, s, socket::SOL_SOCKET, socket::SO_ERROR)?;
            let e = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
            if e != 0 {
                return Err(e);
            }
        }
        Err(e) => return Err(e),
    }
    socket::ioctl(OWNER, s, socket::FIONBIO, 0)?;
    socket::set_option(OWNER, s, socket::SOL_SOCKET, socket::SO_RCVTIMEO, &timeval(REQUEST_S))?;
    socket::set_option(OWNER, s, socket::SOL_SOCKET, socket::SO_SNDTIMEO, &timeval(REQUEST_S))?;
    Ok(t)
}

// ---- Names ----

fn lmhosts(c: &Config) -> Vec<smb::netbios::LmHost> {
    super::read_file(&c.lmhosts).map(|d| smb::netbios::parse_lmhosts(&String::from_utf8_lossy(&d))).unwrap_or_default()
}

/// One NBNS exchange: send `q` to `to`, collect what `parse` accepts until
/// `ms` pass (all answers for a broadcast, the first otherwise).
fn nbns<T>(q: &[u8], to: Endpoint, ms: u32, all: bool, mut parse: impl FnMut(&[u8], Endpoint) -> Option<T>) -> Vec<T> {
    let mut out = Vec::new();
    let Ok(s) = socket::create(OWNER, socket::AF_INET, socket::SOCK_DGRAM, 0) else { return out };
    let _ = socket::set_option(OWNER, s, socket::SOL_SOCKET, socket::SO_BROADCAST, &1u32.to_le_bytes());
    let deadline = crate::irq::uptime_ms() + ms as u64;
    if socket::send(OWNER, s, q, 0, Some(to)).is_ok() {
        loop {
            let left = deadline.saturating_sub(crate::irq::uptime_ms());
            if left == 0 {
                break;
            }
            let mut set = [s];
            if socket::select(OWNER, &mut set, 1, 0, left as i32).unwrap_or(0) == 0 {
                break;
            }
            let Ok((p, from)) = socket::recv(OWNER, s, 1500, 0) else { break };
            if let Some(v) = parse(&p, from) {
                out.push(v);
                if !all {
                    break;
                }
            }
        }
    }
    let _ = socket::close(OWNER, s);
    out
}

static NB_ID: AtomicU32 = AtomicU32::new(0x5200);

fn nb_id() -> u16 {
    NB_ID.fetch_add(1, Ordering::Relaxed) as u16
}

/// NBNS name query: through WINS, else a broadcast.
pub fn nbns_query(c: &Config, name: &str, typ: u8) -> Option<Ipv4> {
    wait_network();
    let id = nb_id();
    let (to, bcast) = match c.wins {
        Some((a, p)) => (Endpoint::new(a, p), false),
        None => (Endpoint::new(Ipv4::BROADCAST, 137), true),
    };
    let q = smb::netbios::name_query(id, name, typ, bcast);
    for _ in 0..3 {
        let r = nbns(&q, to, 800, false, |p, _| smb::netbios::parse_query_response(p, id).and_then(|v| v.first().copied()));
        if let Some(a) = r.first() {
            return Some(Ipv4(*a));
        }
    }
    None
}

/// NBNS node status of the node at `addr`: its names.
pub fn node_status(c: &Config, addr: Ipv4) -> Vec<smb::netbios::NodeName> {
    let id = nb_id();
    let port = c.wins.map_or(137, |w| w.1);
    let q = smb::netbios::node_status_query(id);
    nbns(&q, Endpoint::new(addr, port), 1000, false, |p, _| smb::netbios::parse_node_status(p, id)).into_iter().next().unwrap_or_default()
}

/// Wait (up to 30 s) until an interface has an address: right after
/// boot DHCP may not be done yet.
fn wait_network() {
    for _ in 0..300 {
        let up = (1..8).filter_map(crate::net::iface_info).any(|i| i.addr != Ipv4::UNSPECIFIED && i.flags & 1 != 0);
        if up || !crate::net::running() || crate::net::nic_count() == 0 {
            return;
        }
        crate::task::sched::sleep_ms(100);
    }
}

/// The address of server `name`.
pub fn resolve(c: &Config, name: &str) -> Result<Ipv4, u32> {
    wait_network();
    if let Some(a) = smb::netbios::parse_ipv4(name) {
        return Ok(Ipv4(a));
    }
    let up = smb::wire::upper(name);
    if let Some(h) = lmhosts(c).into_iter().find(|h| h.name == up) {
        return Ok(Ipv4(h.addr));
    }
    if !name.contains('.') {
        if let Some(a) = nbns_query(c, name, smb::netbios::SERVER) {
            return Ok(a);
        }
    }
    socket::resolve(name).ok().and_then(|v| v.first().copied()).ok_or(ERROR_BAD_NETPATH)
}

// ---- Servers ----

struct Conn {
    sess: Option<Box<dyn Session>>,
    trees: BTreeMap<String, smb::Tree>,
    /// Bumped by every new session: file handles of older ones are gone.
    generation: u32,
    /// Credentials given with NET USE for this server (else the logon's).
    cred: Option<Credentials>,
}

pub struct Server {
    pub name: String,
    conn: Mutex<Conn>,
    /// A session was set up with it once (it exists).
    seen: core::sync::atomic::AtomicBool,
}

static SERVERS: SpinLock<BTreeMap<String, Arc<Server>>> = SpinLock::new(BTreeMap::new());

pub fn server(name: &str) -> Arc<Server> {
    let key = smb::wire::upper(name);
    SERVERS
        .lock()
        .entry(key.clone())
        .or_insert_with(|| Arc::new(Server {
                name: key,
                conn: Mutex::new(Conn { sess: None, trees: BTreeMap::new(), generation: 0, cred: None }),
                seen: core::sync::atomic::AtomicBool::new(false),
            }))
        .clone()
}

/// A session-level failure: reconnect.
fn session_lost(e: &smb::Error) -> bool {
    e.fatal()
        || matches!(
            e,
            smb::Error::Status(smb::status::NETWORK_NAME_DELETED | smb::status::USER_SESSION_DELETED | smb::status::NETWORK_SESSION_EXPIRED)
        )
}

impl Server {
    fn open_session(&self, cred: &Credentials) -> Result<Box<dyn Session>, u32> {
        let c = config().ok_or(NERR_WKSTA_NOT_STARTED)?;
        let addr = resolve(&c, &self.name)?;
        let mut guid = [0u8; 16];
        smb::Env::random(&KEnv, &mut guid);
        let is_ip = smb::netbios::parse_ipv4(&self.name).is_some();
        let mut o = smb::Options { server: self.name.clone(), calling: c.computer.clone(), cred: cred.clone(), client_guid: guid, ..Default::default() };
        let mut last = ERROR_BAD_NETPATH;
        // Port 445, then the NetBIOS session service.
        for (port, netbios) in [(c.smb_port, false), (c.nb_port, true)] {
            let t = match tcp_connect(Endpoint::new(addr, port)) {
                Ok(t) => t,
                Err(_) => continue,
            };
            o.netbios = netbios;
            if netbios && is_ip {
                o.server = String::from("*SMBSERVER");
            }
            match smb::connect(Box::new(t), Box::new(KEnv), &o) {
                Ok(s) => {
                    crate::trace!("requester: {} on {}:{} ({})", self.name, addr, port, s.dialect());
                    return Ok(s);
                }
                Err(e) => {
                    crate::trace!("requester: {} port {}: {:?}", self.name, port, e);
                    last = match e {
                        smb::Error::Status(_) | smb::Error::Dos(..) => return Err(e.os2()),
                        _ => ERROR_BAD_NETPATH,
                    };
                }
            }
        }
        Err(last)
    }

    fn credentials(&self, conn: &Conn) -> Credentials {
        if let Some(c) = &conn.cred {
            return c.clone();
        }
        let c = config().unwrap_or_default();
        Credentials { user: c.user, password: c.password, domain: c.domain, workstation: c.computer }
    }

    /// Run `f` on the session with share `share` connected (None: no
    /// tree, e.g. share lists). Reconnects once when the session is lost.
    pub fn with<R>(&self, share: Option<&str>, mut f: impl FnMut(&mut dyn Session, u32, u32) -> smb::Result<R>) -> Result<R, u32> {
        let mut g = self.conn.lock();
        for attempt in 0..2 {
            if g.sess.is_none() {
                let cred = self.credentials(&g);
                g.sess = Some(self.open_session(&cred)?);
                self.seen.store(true, Ordering::Relaxed);
                g.trees.clear();
                g.generation = g.generation.wrapping_add(1);
            }
            let generation = g.generation;
            let conn = &mut *g;
            let sess = conn.sess.as_mut().unwrap();
            let r = (|| {
                let tid = match share {
                    Some(s) => {
                        let key = smb::wire::upper(s);
                        match conn.trees.get(&key) {
                            Some(t) => t.id,
                            None => {
                                let t = sess.tree_connect(s)?;
                                conn.trees.insert(key, t);
                                t.id
                            }
                        }
                    }
                    None => 0,
                };
                f(&mut **sess, tid, generation)
            })();
            match r {
                Ok(v) => return Ok(v),
                Err(e) if session_lost(&e) => {
                    crate::trace!("requester: {} session lost ({:?})", self.name, e);
                    g.sess = None;
                    if attempt == 1 {
                        return Err(ERROR_UNEXP_NET_ERR);
                    }
                }
                Err(e) => return Err(e.os2()),
            }
        }
        Err(ERROR_UNEXP_NET_ERR)
    }

    /// The share's tree kind (connecting it).
    pub fn tree(&self, share: &str) -> Result<smb::ShareKind, u32> {
        let key = smb::wire::upper(share);
        self.with(Some(share), |_, _, _| Ok(()))?;
        Ok(self.conn.lock().trees.get(&key).map_or(smb::ShareKind::Disk, |t| t.kind))
    }

    /// Use these credentials from now on (a new session if they differ).
    pub fn set_credentials(&self, cred: Option<Credentials>) {
        let mut g = self.conn.lock();
        if g.cred != cred {
            g.cred = cred;
            if let Some(mut s) = g.sess.take() {
                let _ = s.logoff();
            }
            g.trees.clear();
        }
    }

    pub fn disconnect(&self) {
        let mut g = self.conn.lock();
        if let Some(mut s) = g.sess.take() {
            let _ = s.logoff();
        }
        g.trees.clear();
    }

    pub fn dialect(&self) -> Option<&'static str> {
        self.conn.lock().sess.as_ref().map(|s| s.dialect())
    }
}

/// Split `\\SERVER\SHARE` (or with `/`), upper-casing the server.
pub fn split_unc(remote: &str) -> Option<(String, String)> {
    let r = remote.strip_prefix("\\\\").or_else(|| remote.strip_prefix("//"))?;
    let mut it = r.splitn(2, ['\\', '/']);
    let server = it.next()?.trim();
    let share = it.next()?.trim_end_matches(['\\', '/']).trim();
    if server.is_empty() || share.is_empty() || share.contains(['\\', '/']) {
        return None;
    }
    Some((smb::wire::upper(server), smb::wire::upper(share)))
}

// ---- Time ----

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// FILETIME to FDATE/FTIME (rs2 keeps local time as UTC).
pub fn stamp(ft: u64) -> Stamp {
    if ft <= smb::FILETIME_1970 {
        return Stamp::default();
    }
    let secs = ((ft - smb::FILETIME_1970) / 10_000_000).min(u32::MAX as u64) as u32;
    let t = crate::rtc::from_unix(secs);
    if t.year < 1980 {
        return Stamp::default();
    }
    let f = fat::Timestamp::new(t.year, t.month, t.day, t.hour, t.minute, t.second);
    Stamp { date: f.date, time: f.time }
}

/// FDATE/FTIME to FILETIME (0 for an empty stamp).
pub fn filetime(s: Stamp) -> u64 {
    if s.date == 0 {
        return 0;
    }
    let (y, m, d) = (1980 + (s.date >> 9) as i64, ((s.date >> 5) & 15) as u32, (s.date & 31) as u32);
    let secs = days_from_civil(y, m.max(1), d.max(1)) * 86400 + (s.time >> 11) as i64 * 3600 + ((s.time >> 5) & 63) as i64 * 60 + (s.time & 31) as i64 * 2;
    smb::FILETIME_1970 + secs.max(0) as u64 * 10_000_000
}

fn info_of(name: &str, a: &smb::FileAttrs) -> FileInfo {
    FileInfo {
        name: name.to_string(),
        attr: a.attr & (attr::READONLY | attr::HIDDEN | attr::SYSTEM | attr::DIRECTORY | attr::ARCHIVED),
        size: a.size,
        alloc: a.alloc.max(a.size),
        created: stamp(a.created),
        accessed: stamp(a.accessed),
        modified: stamp(a.written),
        ea_size: a.ea_size,
    }
}

fn basic_of(i: &SetInfo) -> smb::SetBasic {
    smb::SetBasic {
        created: i.created.map_or(0, filetime),
        accessed: i.accessed.map_or(0, filetime),
        written: i.modified.map_or(0, filetime),
        attr: i.attr.map(|a| a & (attr::READONLY | attr::HIDDEN | attr::SYSTEM | attr::ARCHIVED)),
    }
}

fn to_smb_eas(eas: &[Ea]) -> Vec<smb::Ea> {
    eas.iter().map(|e| smb::Ea { flags: e.flags, name: e.name.clone(), value: e.value.clone() }).collect()
}

// ---- The file system of a share ----

pub struct RedirFs {
    pub server: Arc<Server>,
    pub share: String,
}

impl RedirFs {
    pub fn new(server: &str, share: &str) -> Arc<RedirFs> {
        Arc::new(RedirFs { server: self::server(server), share: smb::wire::upper(share) })
    }

    fn with<R>(&self, f: impl FnMut(&mut dyn Session, u32, u32) -> smb::Result<R>) -> Result<R, u32> {
        self.server.with(Some(&self.share), f)
    }

    pub fn unc(&self) -> String {
        alloc::format!("\\\\{}\\{}", self.server.name, self.share)
    }

    fn open_file(&self, path: &str, args: OpenArgs, writable: bool) -> Result<Arc<dyn FsFile>, u32> {
        let (fid, a, generation) = self.with(|s, tid, g| s.open(tid, path, &args).map(|(f, a, _)| (f, a, g)))?;
        let mut reopen = args.clone();
        reopen.disposition = Disposition::Open;
        Ok(Arc::new(RemoteFile {
            fs: RedirFs { server: self.server.clone(), share: self.share.clone() },
            path: path.to_string(),
            args: reopen,
            writable,
            state: SpinLock::new(FileState { fid, generation, attrs: a, name: path.rsplit('\\').next().unwrap_or("").to_string() }),
        }))
    }
}

impl FileSystem for RedirFs {
    fn name(&self) -> &'static str {
        "LAN"
    }

    fn remote_name(&self) -> Option<String> {
        Some(self.unc())
    }

    fn stat(&self, path: &str) -> Result<FileInfo, u32> {
        if path.is_empty() {
            self.with(|s, tid, _| s.fs_size(tid).map(|_| ()))?;
            return Ok(FileInfo { name: String::new(), attr: attr::DIRECTORY, ..Default::default() });
        }
        // A print share has no files: every CREATE there starts a print
        // job on the server, so looking a name up (DosOpen does before it
        // creates) would print an empty job. A name there is a new file.
        if matches!(self.server.tree(&self.share), Ok(smb::ShareKind::Print)) {
            return Err(crate::api::err::FILE_NOT_FOUND);
        }
        let a = self.with(|s, tid, _| s.stat_path(tid, path))?;
        Ok(info_of(path.rsplit('\\').next().unwrap_or(""), &a))
    }

    fn list(&self, dir: &str) -> Result<Vec<FileInfo>, u32> {
        let v = self.with(|s, tid, _| s.list(tid, dir, "*"))?;
        if v.is_empty() && !dir.is_empty() {
            // An empty listing of a path that is not a directory.
            if !self.stat(dir)?.is_dir() {
                return Err(crate::api::err::PATH_NOT_FOUND);
            }
        }
        Ok(v.iter().map(|e| info_of(&e.name, &e.attrs)).collect())
    }

    fn open(&self, path: &str) -> Result<Arc<dyn FsFile>, u32> {
        let mut a = OpenArgs::read();
        a.directory = Some(false);
        a.write = true;
        match self.open_file(path, a.clone(), true) {
            Err(ERROR_ACCESS_DENIED) => {
                a.write = false;
                self.open_file(path, a, false)
            }
            r => r,
        }
    }

    fn create(&self, path: &str, at: u32) -> Result<Arc<dyn FsFile>, u32> {
        let mut a = OpenArgs::read();
        a.write = true;
        a.directory = Some(false);
        a.disposition = Disposition::Create;
        a.attr = (at & (attr::READONLY | attr::HIDDEN | attr::SYSTEM | attr::ARCHIVED)) | smb::attr::ARCHIVE;
        // A read-only file is created writable for this open, then marked.
        let ro = a.attr & attr::READONLY != 0;
        a.attr &= !attr::READONLY;
        let f = self.open_file(path, a, true)?;
        if ro {
            let _ = self.with(|s, tid, _| s.set_basic_path(tid, path, &smb::SetBasic { attr: Some(at), ..Default::default() }));
        }
        Ok(f)
    }

    fn delete(&self, path: &str) -> Result<(), u32> {
        self.with(|s, tid, _| s.delete(tid, path))
    }

    fn mkdir(&self, path: &str) -> Result<(), u32> {
        self.with(|s, tid, _| s.mkdir(tid, path))
    }

    fn rmdir(&self, path: &str) -> Result<(), u32> {
        self.with(|s, tid, _| s.rmdir(tid, path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), u32> {
        self.with(|s, tid, _| s.rename(tid, from, to))
    }

    fn set_info(&self, path: &str, info: &SetInfo) -> Result<(), u32> {
        let b = basic_of(info);
        self.with(|s, tid, _| s.set_basic_path(tid, path, &b))
    }

    fn volume(&self) -> Result<VolumeInfo, u32> {
        let f = self.with(|s, tid, _| s.fs_size(tid))?;
        // OS/2's FSALLOCATE has 32-bit counts: scale big volumes.
        let (mut spu, mut units, mut free) = (f.sectors_per_unit.max(1) as u64, f.total_units, f.free_units);
        while units > u32::MAX as u64 {
            spu *= 2;
            units /= 2;
            free /= 2;
        }
        let mut label = self.share.clone();
        label.truncate(11);
        Ok(VolumeInfo {
            serial: 0,
            label,
            sectors_per_unit: spu as u32,
            units: units as u32,
            free_units: free as u32,
            bytes_per_sector: f.bytes_per_sector.max(1),
        })
    }

    fn read_eas(&self, path: &str) -> Result<Vec<Ea>, u32> {
        let v = self.with(|s, tid, _| s.eas(tid, path))?;
        Ok(v.into_iter().map(|e| Ea { flags: e.flags, name: e.name, value: e.value }).collect())
    }

    fn write_eas(&self, path: &str, eas: &[Ea]) -> Result<(), u32> {
        // Replace: names no longer listed are removed (empty values).
        let old = self.read_eas(path).unwrap_or_default();
        let mut list = to_smb_eas(eas);
        for o in old {
            if !eas.iter().any(|e| e.name.eq_ignore_ascii_case(&o.name)) {
                list.push(smb::Ea { flags: 0, name: o.name, value: Vec::new() });
            }
        }
        self.with(|s, tid, _| s.set_eas(tid, path, &list)).map_err(|e| if e == ERROR_NOT_SUPPORTED { crate::api::err::EAS_NOT_SUPPORTED } else { e })
    }
}

struct FileState {
    fid: smb::FileId,
    generation: u32,
    attrs: smb::FileAttrs,
    name: String,
}

/// An open file on a share.
struct RemoteFile {
    fs: RedirFs,
    path: String,
    args: OpenArgs,
    writable: bool,
    state: SpinLock<FileState>,
}

impl RemoteFile {
    /// Run `f` with the file's handle, reopening it after a reconnect.
    fn with<R>(&self, mut f: impl FnMut(&mut dyn Session, u32, smb::FileId) -> smb::Result<R>) -> Result<R, u32> {
        let (fid, generation) = {
            let s = self.state.lock();
            (s.fid, s.generation)
        };
        let path = self.path.clone();
        let args = self.args.clone();
        let mut fresh = None;
        let r = self.fs.with(|s, tid, g| {
            let fid = if g != generation {
                let (nf, _, _) = s.open(tid, &path, &args)?;
                fresh = Some((nf, g));
                nf
            } else {
                fid
            };
            f(s, tid, fid)
        });
        if let Some((nf, g)) = fresh {
            let mut s = self.state.lock();
            s.fid = nf;
            s.generation = g;
        }
        r
    }
}

impl FsFile for RemoteFile {
    fn read(&self, off: u64, buf: &mut [u8]) -> Result<usize, u32> {
        let mut done = 0;
        while done < buf.len() {
            let want = (buf.len() - done) as u32;
            let at = off + done as u64;
            let v = self.with(|s, tid, f| {
                let n = want.min(s.max_io());
                s.read(tid, f, at, n)
            })?;
            if v.is_empty() {
                break;
            }
            buf[done..done + v.len()].copy_from_slice(&v);
            done += v.len();
        }
        Ok(done)
    }

    fn write(&self, off: u64, data: &[u8]) -> Result<usize, u32> {
        if !self.writable {
            return Err(ERROR_ACCESS_DENIED);
        }
        let mut done = 0;
        while done < data.len() {
            let at = off + done as u64;
            let rest = &data[done..];
            let n = self.with(|s, tid, f| {
                let n = rest.len().min(s.max_io() as usize);
                s.write(tid, f, at, &rest[..n])
            })? as usize;
            if n == 0 {
                return Err(crate::api::err::DISK_FULL);
            }
            done += n;
        }
        let mut s = self.state.lock();
        s.attrs.size = s.attrs.size.max(off + done as u64);
        Ok(done)
    }

    fn size(&self) -> u64 {
        self.state.lock().attrs.size
    }

    fn set_size(&self, size: u64) -> Result<(), u32> {
        self.with(|s, tid, f| s.set_size(tid, f, size))?;
        self.state.lock().attrs.size = size;
        Ok(())
    }

    fn info(&self) -> FileInfo {
        if let Ok(a) = self.with(|s, tid, f| s.stat(tid, f)) {
            self.state.lock().attrs = a;
        }
        let s = self.state.lock();
        info_of(&s.name, &s.attrs)
    }

    fn set_info(&self, info: &SetInfo) -> Result<(), u32> {
        let b = basic_of(info);
        self.with(|s, tid, f| s.set_basic(tid, f, &b))
    }

    fn flush(&self) -> Result<(), u32> {
        Ok(())
    }

    fn read_eas(&self) -> Result<Vec<Ea>, u32> {
        self.fs.read_eas(&self.path)
    }

    fn write_eas(&self, eas: &[Ea]) -> Result<(), u32> {
        self.fs.write_eas(&self.path, eas)
    }
}

impl Drop for RemoteFile {
    fn drop(&mut self) {
        let (fid, generation) = {
            let s = self.state.lock();
            (s.fid, s.generation)
        };
        // Close on the session the handle belongs to (not after a reconnect).
        let g = &self.fs.server.conn;
        let mut c = g.lock();
        if c.generation != generation {
            return;
        }
        let key = self.fs.share.clone();
        let Some(tid) = c.trees.get(&key).map(|t| t.id) else { return };
        if let Some(s) = c.sess.as_mut() {
            if let Err(e) = s.close(tid, fid) {
                if session_lost(&e) {
                    c.sess = None;
                }
            }
        }
    }
}

// ---- UNC pseudo-drives ----

static UNC: SpinLock<Vec<(String, String)>> = SpinLock::new(Vec::new());

/// The pseudo-drive of `\\server\share` (made on first use).
pub fn unc_drive(server: &str, share: &str) -> Result<u8, u32> {
    let (sv, sh) = (smb::wire::upper(server), smb::wire::upper(share));
    let mut u = UNC.lock();
    if let Some(i) = u.iter().position(|(a, b)| *a == sv && *b == sh) {
        return Ok(UNC_BASE + i as u8);
    }
    if u.len() >= (255 - UNC_BASE as usize) {
        return Err(ERROR_BAD_NETPATH);
    }
    u.push((sv, sh));
    Ok(UNC_BASE + (u.len() - 1) as u8)
}

/// `\\SERVER\SHARE` of a pseudo-drive.
pub fn unc_name(d: u8) -> Option<(String, String)> {
    UNC.lock().get(d.checked_sub(UNC_BASE)? as usize).cloned()
}

/// The file system behind a pseudo-drive.
pub fn unc_fs(d: u8) -> Option<Arc<dyn FileSystem>> {
    let (server, share) = unc_name(d)?;
    config()?;
    Some(RedirFs::new(&server, &share) as Arc<dyn FileSystem>)
}

// ---- Uses ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Local {
    /// Drive (0 = A:).
    Drive(u8),
    /// LPT1-LPT9 (0-based).
    Lpt(u8),
    /// A deviceless connection.
    None,
}

impl Local {
    pub fn parse(s: &str) -> Result<Local, u32> {
        let s = s.trim().trim_end_matches(':').to_ascii_uppercase();
        let b = s.as_bytes();
        match b.len() {
            0 => Ok(Local::None),
            1 if b[0].is_ascii_uppercase() => Ok(Local::Drive(b[0] - b'A')),
            4 if s.starts_with("LPT") && (b'1'..=b'9').contains(&b[3]) => Ok(Local::Lpt(b[3] - b'1')),
            _ => Err(NERR_BAD_DEV),
        }
    }

    pub fn name(&self) -> String {
        match *self {
            Local::Drive(d) => alloc::format!("{}:", (b'A' + d) as char),
            Local::Lpt(n) => alloc::format!("LPT{}", n + 1),
            Local::None => String::new(),
        }
    }
}

#[derive(Clone)]
pub struct Use {
    pub local: Local,
    pub remote: String,
    pub server: Arc<Server>,
    pub share: String,
    pub kind: smb::ShareKind,
}

static USES: SpinLock<Vec<Use>> = SpinLock::new(Vec::new());

/// NET USE / NetUseAdd: connect `local` (a drive, LPTn, or "") to
/// `\\server\share`. `password`/`user` (None: the logon's) apply to the
/// server's session.
pub fn use_add(local: &str, remote: &str, password: Option<&str>, user: Option<&str>) -> Result<(), u32> {
    let _a = ADMIN.lock();
    let c = config().ok_or(NERR_WKSTA_NOT_STARTED)?;
    let local = Local::parse(local)?;
    let (sv, sh) = split_unc(remote).ok_or(crate::api::err::INVALID_PARAMETER)?;
    if local != Local::None && USES.lock().iter().any(|u| u.local == local) {
        return Err(ERROR_ALREADY_ASSIGNED);
    }
    if let Local::Drive(d) = local {
        if super::drive(d).is_some() {
            return Err(ERROR_ALREADY_ASSIGNED);
        }
    }
    let server = server(&sv);
    if password.is_some() || user.is_some() {
        let cred = Credentials {
            user: user.map_or(c.user.clone(), String::from),
            password: password.map_or(c.password.clone(), String::from),
            domain: c.domain.clone(),
            workstation: c.computer.clone(),
        };
        server.set_credentials(Some(cred));
    }
    let kind = server.tree(&sh)?;
    match (local, kind) {
        (Local::Drive(_), smb::ShareKind::Disk) | (Local::Lpt(_), smb::ShareKind::Print) | (Local::None, _) => {}
        (Local::Lpt(_), _) | (Local::Drive(_), _) => return Err(NERR_BAD_DEV),
    }
    if let Local::Drive(d) = local {
        // The letter may have gone to a USB stick meanwhile.
        let _m = super::MOUNTING.lock();
        if super::drive(d).is_some() {
            return Err(ERROR_ALREADY_ASSIGNED);
        }
        super::mount(d, RedirFs::new(&sv, &sh));
    }
    let remote = alloc::format!("\\\\{sv}\\{sh}");
    USES.lock().push(Use { local, remote, server, share: sh, kind });
    Ok(())
}

/// NET USE /D / NetUseDel: `name` is a local device or a UNC name.
/// Without `force`, a drive with open files is refused.
pub fn use_del(name: &str, force: bool) -> Result<(), u32> {
    let _a = ADMIN.lock();
    let target = if name.starts_with("\\\\") || name.starts_with("//") {
        let (sv, sh) = split_unc(name).ok_or(NERR_USE_NOT_FOUND)?;
        let r = alloc::format!("\\\\{sv}\\{sh}");
        let us = USES.lock();
        us.iter().position(|u| u.local == Local::None && u.remote == r).or_else(|| us.iter().position(|u| u.remote == r)).map(|i| us[i].clone())
    } else {
        let l = Local::parse(name).map_err(|_| NERR_USE_NOT_FOUND)?;
        let us = USES.lock();
        us.iter().position(|u| u.local == l).map(|i| us[i].clone())
    };
    let u = target.ok_or(NERR_USE_NOT_FOUND)?;
    if let Local::Drive(d) = u.local {
        if !force && super::open_count(d) > 0 {
            return Err(NERR_OPEN_FILES);
        }
        super::unmount(d);
    }
    USES.lock().retain(|x| !(x.local == u.local && x.remote == u.remote));
    // The last use of a server ends its session.
    if !USES.lock().iter().any(|x| Arc::ptr_eq(&x.server, &u.server)) {
        u.server.disconnect();
        u.server.set_credentials(None);
    }
    Ok(())
}

pub fn uses() -> Vec<Use> {
    USES.lock().clone()
}

/// The print share LPTn is redirected to.
pub fn lpt_use(n: u8) -> Option<Use> {
    USES.lock().iter().find(|u| u.local == Local::Lpt(n)).cloned()
}

static JOBS: AtomicU32 = AtomicU32::new(1);

/// An open of LPTn that is redirected: a new print file on the share.
pub fn open_printer(n: u8) -> Option<Result<(super::FullPath, Arc<dyn FsFile>), u32>> {
    let u = lpt_use(n)?;
    let job = JOBS.fetch_add(1, Ordering::Relaxed);
    let name = alloc::format!("RS2JOB{job:04}");
    let fs = RedirFs { server: u.server.clone(), share: u.share.clone() };
    let mut a = OpenArgs::read();
    a.read = false;
    a.write = true;
    a.disposition = Disposition::OverwriteIf;
    let r = (|| {
        let d = unc_drive(&u.server.name, &u.share)?;
        let f = fs.open_file(&name, a, true)?;
        Ok((super::FullPath { drive: d, parts: alloc::vec![name.clone()] }, f))
    })();
    Some(r)
}

/// The shares of a server.
pub fn shares(server_name: &str) -> Result<Vec<smb::ShareInfo>, u32> {
    config().ok_or(NERR_WKSTA_NOT_STARTED)?;
    server(server_name).with(None, |s, _, _| s.shares())
}

/// A server seen on the network: name, workgroup, comment.
#[derive(Clone, Debug)]
pub struct Seen {
    pub name: String,
    pub workgroup: String,
    pub comment: String,
    pub typ: u32,
}

/// The servers of `domain` (None: all known): LMHOSTS entries, servers
/// with sessions, and the browse lists of SMB1 servers among them.
pub fn servers(domain: Option<&str>) -> Result<Vec<Seen>, u32> {
    let c = config().ok_or(NERR_WKSTA_NOT_STARTED)?;
    let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
    let add = |seen: &mut BTreeMap<String, Seen>, s: Seen| {
        let e = seen.entry(s.name.clone()).or_insert(s.clone());
        if e.comment.is_empty() {
            e.comment = s.comment;
        }
        if e.workgroup.is_empty() {
            e.workgroup = s.workgroup;
        }
    };
    for h in lmhosts(&c) {
        let wg = h.domain.clone().unwrap_or_default();
        add(&mut seen, Seen { name: h.name, workgroup: wg, comment: String::new(), typ: 3 });
    }
    let known: Vec<Arc<Server>> = SERVERS.lock().values().cloned().collect();
    for s in &known {
        if smb::netbios::parse_ipv4(&s.name).is_none() && s.seen.load(Ordering::Relaxed) {
            add(&mut seen, Seen { name: s.name.clone(), workgroup: String::new(), comment: String::new(), typ: 3 });
        }
    }
    // The master browser of our workgroup, if NBNS knows one.
    if let Some(a) = nbns_query(&c, &c.workgroup, smb::netbios::MASTER_BROWSER) {
        let names = node_status(&c, a);
        if let Some(n) = names.iter().find(|n| n.typ == smb::netbios::SERVER) {
            add(&mut seen, Seen { name: n.name.clone(), workgroup: c.workgroup.clone(), comment: String::new(), typ: 3 });
        }
    }
    // Browse lists (SMB1 servers keep them) and each server's workgroup.
    let names: Vec<String> = seen.keys().cloned().collect();
    for n in names {
        let srv = server(&n);
        if let Ok(list) = srv.with(None, |s, _, _| s.servers(None, 0xFFFF_FFFF)) {
            for x in list {
                let wg = if x.name == n { String::new() } else { String::new() };
                add(&mut seen, Seen { name: x.name, workgroup: wg, comment: x.comment, typ: x.typ });
            }
        }
        let wg = srv.with(None, |s, _, _| Ok(s.server_domain())).unwrap_or_default();
        if let Some(e) = seen.get_mut(&n) {
            if e.workgroup.is_empty() {
                e.workgroup = wg;
            }
        }
    }
    let mut v: Vec<Seen> = seen.into_values().collect();
    for s in v.iter_mut() {
        if s.workgroup.is_empty() {
            s.workgroup = c.workgroup.clone();
        }
    }
    if let Some(d) = domain {
        let d = smb::wire::upper(d);
        v.retain(|s| s.workgroup == d);
    }
    Ok(v)
}
