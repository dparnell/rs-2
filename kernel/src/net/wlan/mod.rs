//! Wireless LAN (docs/SURFACE-PRO-4.md, "Phase 4: WiFi"): the layer
//! between WiFi chip drivers and the rest of rs2.
//!
//! Chip drivers implement [`WlanDriver`] and [`register`] their adapters
//! while the PCI bus is probed. A chip that needs firmware cannot start
//! then (no file system yet), so [`start`], called once the boot volume
//! and CONFIG.SYS are there, runs a kernel thread ("wlan") that brings
//! each adapter up ([`WlanDriver::start`]: firmware download, chip
//! initialisation), reads `\MPTN\ETC\WIFI.CFG`, scans once (logging what
//! it finds) and from then on runs the connection manager:
//!
//! * It joins the network asked for ([`connect`], WIFI CONNECT) or else
//!   the first network of WIFI.CFG that is in range and not marked
//!   `AUTOCONNECT=NO`, retrying with growing pauses after failures and
//!   after the link is lost.
//! * For WPA2 networks it runs the supplicant (`wlan::wpa`) on the EAPOL
//!   frames that arrive after association and installs the keys it
//!   derives in the chip ([`WlanDriver::set_key`]): the firmware encrypts.
//! * Each adapter is a `net::Nic` (`wlan`N's interface `eth`N in the
//!   stack), registered once the chip is up: its link is up while the
//!   port is open (associated, and keyed on WPA2), so DHCP runs then.
//!
//! The thread waits for the chip's interrupt ([`kick`]) or 5 ms, then
//! takes the driver's events and received frames ([`WlanDriver::poll`]).
//!
//! Programs reach it through RS2API 620-624 (`api::wlan`, WIFI.EXE).

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use wlan::config::Profile;
use wlan::ie::{Bss, Security};
use wlan::wpa::{Action, Supplicant};
use wlan::MacFmt;

use crate::sync::{Mutex, SpinLock};

/// The last lines the WiFi layer and its drivers logged (shown by WIFI
/// STATUS: the boot log is not always at hand).
static LOG: SpinLock<VecDeque<String>> = SpinLock::new(VecDeque::new());
const LOG_LINES: usize = 40;

/// Log a line to the console/boot log and to [`LOG`].
pub fn log(line: String) {
    crate::println!("{line}");
    let ms = crate::irq::uptime_ms();
    let mut l = LOG.lock();
    if l.len() >= LOG_LINES {
        l.pop_front();
    }
    l.push_back(alloc::format!("{}.{:03} {line}", ms / 1000, ms % 1000));
}

/// `crate::println!` that also keeps the line for WIFI STATUS.
#[macro_export]
macro_rules! wlog {
    ($($a:tt)*) => { $crate::net::wlan::log(alloc::format!($($a)*)) };
}
use crate::task::WaitQueue;

pub use wlan::mwifiex::Key;

pub const CONFIG_FILE: &str = "C:\\MPTN\\ETC\\WIFI.CFG";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WlanError {
    /// The adapter is not up (yet, or it failed to start).
    NotReady,
    /// A firmware file is missing.
    NoFirmware(String),
    /// The chip did not answer in time.
    Timeout(&'static str),
    /// The chip refused a command: (what, its result code).
    Refused(&'static str, u16),
    /// Anything else, described.
    Failed(String),
    Unsupported,
}

impl core::fmt::Display for WlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            WlanError::NotReady => write!(f, "adapter not ready"),
            WlanError::NoFirmware(p) => write!(f, "firmware {p} not found"),
            WlanError::Timeout(w) => write!(f, "timeout ({w})"),
            WlanError::Refused(w, r) => write!(f, "{w} refused (result {r:#x})"),
            WlanError::Failed(s) => write!(f, "{s}"),
            WlanError::Unsupported => write!(f, "not supported"),
        }
    }
}

/// What a scan looks for.
#[derive(Clone, Debug, Default)]
pub struct ScanRequest {
    /// Probe for this SSID (hidden networks); empty: all.
    pub ssid: Vec<u8>,
}

/// What a chip can do.
#[derive(Clone, Copy, Debug, Default)]
pub struct WlanCaps {
    pub band_2ghz: bool,
    pub band_5ghz: bool,
    /// The firmware does the 802.11 MAC work (scan, association,
    /// encryption): the driver implements the high-level calls.
    pub full_mac: bool,
}

/// What a driver reports from [`WlanDriver::poll`].
#[derive(Clone, Debug)]
pub enum WlanEvent {
    /// A received Ethernet II frame (EAPOL included).
    Frame(Vec<u8>),
    /// The association ended: why, and the 802.11 reason code.
    Disconnected { why: &'static str, reason: u16 },
    /// Another firmware event (logged).
    Other(u32),
}

/// One WiFi chip. Full-MAC drivers implement the high-level calls; a
/// soft-MAC chip (later) would implement raw frames and get the rest from
/// a shared MAC layer.
pub trait WlanDriver: Send + Sync {
    /// "88W8897 02:00.0".
    fn name(&self) -> String;
    fn caps(&self) -> WlanCaps;
    /// The station address (known once started).
    fn mac_address(&self) -> [u8; 6];
    /// Bring the chip up: firmware, initialisation. Runs on the wlan
    /// thread and may sleep. Returns a line describing the chip.
    fn start(&self) -> Result<String, WlanError>;
    /// Scan and return what was found. Blocks (a few seconds) and may
    /// sleep.
    fn scan(&self, req: &ScanRequest) -> Result<Vec<Bss>, WlanError>;
    /// Diagnostic lines (`key=value`) for WIFI STATUS.
    fn diagnostics(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Authenticate and associate with `bss` (found by the last scan),
    /// offering `rsn_ie` (the station's RSN element) on WPA2 networks.
    /// Blocks until the access point answers; returns the association ID.
    fn connect(&self, _bss: &Bss, _rsn_ie: Option<&[u8]>) -> Result<u16, WlanError> {
        Err(WlanError::Unsupported)
    }
    /// Leave the network (deauthenticate).
    fn disconnect(&self) {}
    /// Install a pairwise or group key (the chip encrypts).
    fn set_key(&self, _key: &Key) -> Result<(), WlanError> {
        Err(WlanError::Unsupported)
    }
    /// Queue an Ethernet II frame; false if there is no room.
    fn send(&self, _frame: &[u8]) -> bool {
        false
    }
    /// Wait (up to `ms`) until the frames queued so far are sent.
    fn flush_tx(&self, _ms: u64) {}
    /// Hand over events and received frames. Called by the wlan thread
    /// only; may sleep briefly.
    fn poll(&self, _f: &mut dyn FnMut(WlanEvent)) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Found on the bus, not started.
    Found,
    Starting,
    Up,
    Failed(String),
}

impl Status {
    fn text(&self) -> String {
        match self {
            Status::Found => "found".into(),
            Status::Starting => "starting".into(),
            Status::Up => "up".into(),
            Status::Failed(e) => alloc::format!("failed: {e}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    Idle,
    Associating,
    /// Associated, the 4-way handshake running (since this time).
    Handshake(u64),
    Connected,
}

impl Link {
    fn name(self) -> &'static str {
        match self {
            Link::Idle => "disconnected",
            Link::Associating => "connecting",
            Link::Handshake(_) => "authenticating",
            Link::Connected => "connected",
        }
    }
}

/// The connection manager's state for one adapter.
struct Conn {
    link: Link,
    /// The network asked for by WIFI CONNECT (else WIFI.CFG decides).
    target: Option<Profile>,
    /// Do not join anything (WIFI DISCONNECT) until asked.
    off: bool,
    bss: Option<Bss>,
    supp: Option<Supplicant>,
    gtk_set: bool,
    next_try: u64,
    failures: u32,
    error: String,
    /// The PMK of the last (SSID, passphrase): PBKDF2 is 8192 SHA-1s.
    pmk: Option<(Vec<u8>, String, [u8; 32])>,
}

struct State {
    status: Status,
    info: String,
    results: Vec<Bss>,
    last_scan_ms: u64,
}

pub struct Adapter {
    drv: Arc<dyn WlanDriver>,
    state: SpinLock<State>,
    /// One scan or other chip operation at a time.
    busy: Mutex<()>,
    conn: SpinLock<Conn>,
    /// Associated and keyed: the interface's link is up.
    port_open: AtomicBool,
    /// Received frames for the network stack.
    rxq: SpinLock<VecDeque<Vec<u8>>>,
}

static ADAPTERS: SpinLock<Vec<Arc<Adapter>>> = SpinLock::new(Vec::new());
static PROFILES: SpinLock<Vec<Profile>> = SpinLock::new(Vec::new());
static KICK: AtomicBool = AtomicBool::new(false);
static WAIT: WaitQueue = WaitQueue::new();

/// Wake the wlan thread (a chip's interrupt; a request from a program).
pub fn kick() {
    KICK.store(true, Ordering::Release);
    WAIT.wake_all();
}

/// A driver found a chip.
pub fn register(drv: Arc<dyn WlanDriver>) {
    crate::wlog!("wlan: {} found", drv.name());
    let a = Adapter {
        drv,
        state: SpinLock::new(State { status: Status::Found, info: String::new(), results: Vec::new(), last_scan_ms: 0 }),
        busy: Mutex::new(()),
        conn: SpinLock::new(Conn {
            link: Link::Idle,
            target: None,
            off: false,
            bss: None,
            supp: None,
            gtk_set: false,
            next_try: 0,
            failures: 0,
            error: String::new(),
            pmk: None,
        }),
        port_open: AtomicBool::new(false),
        rxq: SpinLock::new(VecDeque::new()),
    };
    ADAPTERS.lock().push(Arc::new(a));
}

pub fn count() -> usize {
    ADAPTERS.lock().len()
}

fn adapter(i: usize) -> Option<Arc<Adapter>> {
    ADAPTERS.lock().get(i).cloned()
}

/// Read WIFI.CFG (missing: no profiles).
pub fn load_config() {
    let Some(data) = crate::fs::read_file(CONFIG_FILE) else {
        PROFILES.lock().clear();
        return;
    };
    let text: String = data.iter().map(|&b| b as char).collect();
    let c = wlan::config::parse(&text);
    for (line, why) in &c.errors {
        crate::wlog!("wlan: {CONFIG_FILE} line {line}: {why}");
    }
    crate::wlog!("wlan: {} network(s) in {CONFIG_FILE}", c.profiles.len());
    // Passphrases written into WIFI.CFG (by hand, or by an older WIFI.EXE)
    // move into the system keychain, and the file is written without them.
    let mut moved = false;
    let mut plain = c.profiles.clone();
    for p in &mut plain {
        if let Some(k) = p.passphrase.take() {
            match crate::keychain::set(KEYCHAIN_SERVICE, &p.ssid, k.as_bytes(), false) {
                Ok(()) => {
                    crate::wlog!("wlan: the passphrase of {} moved from {CONFIG_FILE} into the keychain", wlan::ie::ssid_text(&p.ssid));
                    moved = true;
                }
                Err(e) => {
                    crate::wlog!("wlan: the passphrase of {} stays in {CONFIG_FILE}: {e}", wlan::ie::ssid_text(&p.ssid));
                    p.passphrase = Some(k);
                }
            }
        }
    }
    if moved {
        let t: alloc::vec::Vec<u8> = wlan::config::write(&plain).chars().map(|c| c as u32 as u8).collect();
        if let Err(e) = crate::fs::write_file(CONFIG_FILE, &t) {
            crate::wlog!("wlan: cannot rewrite {CONFIG_FILE} (error {e})");
        }
    }
    *PROFILES.lock() = c.profiles;
    kick();
}

/// The keychain service of WiFi passphrases (account: the SSID).
pub const KEYCHAIN_SERVICE: &str = "wifi";

/// A network's passphrase: given with WIFI CONNECT or in WIFI.CFG, else
/// from the keychain.
fn passphrase(p: &Profile) -> Result<Option<String>, String> {
    if let Some(k) = &p.passphrase {
        return Ok(Some(k.clone()));
    }
    match crate::keychain::get(KEYCHAIN_SERVICE, &p.ssid) {
        Some(k) => Ok(Some(k.iter().map(|&b| b as char).collect())),
        None if crate::keychain::is_locked() => Err("the keychain is locked (KEYCHAIN UNLOCK)".into()),
        None => Ok(None),
    }
}

/// The network stack's view of an adapter.
struct WlanNic {
    a: Arc<Adapter>,
    name: String,
    mac: [u8; 6],
}

impl super::Nic for WlanNic {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac(&self) -> tcpip::Mac {
        self.mac
    }

    fn link_up(&self) -> bool {
        self.a.port_open.load(Ordering::Acquire)
    }

    fn send(&self, frame: &[u8]) -> bool {
        self.link_up() && self.a.drv.send(frame)
    }

    fn receive(&self, f: &mut dyn FnMut(&[u8])) -> usize {
        let frames: Vec<Vec<u8>> = self.a.rxq.lock().drain(..).collect();
        for fr in &frames {
            f(fr);
        }
        frames.len()
    }

    fn rx_pending(&self) -> bool {
        self.a.rxq.try_lock().is_some_and(|q| !q.is_empty())
    }
}

/// Start the adapters on the wlan thread. Called once at boot, after
/// CONFIG.SYS is read.
pub fn start() {
    if count() == 0 {
        return;
    }
    crate::task::kthread::spawn("wlan", || {
        load_config();
        let n = count();
        for i in 0..n {
            let Some(a) = adapter(i) else { continue };
            a.state.lock().status = Status::Starting;
            let t0 = crate::irq::uptime_ms();
            let r = {
                let _g = a.busy.lock();
                a.drv.start()
            };
            match r {
                Ok(info) => {
                    crate::wlog!("wlan{i}: {} up in {} ms: {info}", a.drv.name(), crate::irq::uptime_ms() - t0);
                    let mut s = a.state.lock();
                    s.status = Status::Up;
                    s.info = info;
                }
                Err(e) => {
                    crate::wlog!("wlan{i}: {} failed to start: {e}", a.drv.name());
                    a.state.lock().status = Status::Failed(alloc::format!("{e}"));
                    continue;
                }
            }
            let nic = WlanNic { a: a.clone(), name: alloc::format!("wlan{i} {}", a.drv.name()), mac: a.drv.mac_address() };
            super::register(Arc::new(nic));
            match scan(i, &ScanRequest::default()) {
                Ok(v) => {
                    crate::wlog!("wlan{i}: scan found {} network(s)", v.len());
                    for b in &v {
                        crate::wlog!("wlan{i}:   {}", describe(b));
                    }
                }
                Err(e) => crate::wlog!("wlan{i}: scan failed: {e}"),
            }
        }
        manage();
    });
}

/// The connection manager: forever.
fn manage() {
    loop {
        let all = ADAPTERS.lock().clone();
        for (i, a) in all.iter().enumerate() {
            if a.state.lock().status != Status::Up {
                continue;
            }
            let mut events = Vec::new();
            a.drv.poll(&mut |e| events.push(e));
            for e in events {
                event(i, a, e);
            }
            step(i, a);
        }
        let deadline = crate::irq::ticks() + 5 * crate::irq::HZ / 1000;
        WAIT.wait_until(Some(deadline), || KICK.load(Ordering::Acquire));
        KICK.store(false, Ordering::Release);
    }
}

const ETHERTYPE_EAPOL: u16 = 0x888e;

fn event(i: usize, a: &Arc<Adapter>, e: WlanEvent) {
    match e {
        WlanEvent::Frame(f) => {
            if f.len() >= 14 && u16::from_be_bytes([f[12], f[13]]) == ETHERTYPE_EAPOL {
                eapol(i, a, &f[14..]);
            } else if a.port_open.load(Ordering::Acquire) {
                let mut q = a.rxq.lock();
                if q.len() < 256 {
                    q.push_back(f);
                }
                drop(q);
                super::kick();
            }
        }
        WlanEvent::Disconnected { why, reason } => {
            let mut c = a.conn.lock();
            if c.link != Link::Idle {
                crate::wlog!("wlan{i}: {why} (reason {reason})");
                lost(a, &mut c, alloc::format!("{why} (reason {reason})"), 2000);
            }
        }
        WlanEvent::Other(cause) => crate::wlog!("wlan{i}: firmware event {cause:#x}"),
    }
}

/// Back to idle: port closed, try again after `pause` ms (more after
/// repeated failures).
fn lost(a: &Adapter, c: &mut Conn, why: String, pause: u64) {
    a.port_open.store(false, Ordering::Release);
    c.link = Link::Idle;
    c.supp = None;
    c.failures += 1;
    let backoff = pause.saturating_mul(1 << c.failures.min(5)).min(60_000);
    c.next_try = crate::irq::uptime_ms() + backoff;
    c.error = why;
}

/// The network to join now, if any: the one asked for, else the first
/// WIFI.CFG profile in range.
fn wanted(c: &Conn, results: &[Bss]) -> Option<Profile> {
    if c.off {
        return None;
    }
    if let Some(t) = &c.target {
        return Some(t.clone());
    }
    let profiles = PROFILES.lock().clone();
    if results.is_empty() {
        return profiles.into_iter().find(|p| p.autoconnect);
    }
    profiles.into_iter().filter(|p| p.autoconnect).find(|p| results.iter().any(|b| b.ssid == p.ssid))
}

fn step(i: usize, a: &Arc<Adapter>) {
    let now = crate::irq::uptime_ms();
    let profile = {
        let mut c = a.conn.lock();
        if let Link::Handshake(since) = c.link {
            if now - since > 10_000 {
                crate::wlog!("wlan{i}: no WPA2 handshake in 10 s (wrong passphrase?)");
                lost(a, &mut c, "the WPA2 handshake did not finish (wrong passphrase?)".into(), 5000);
                drop(c);
                a.drv.disconnect();
            }
            return;
        }
        if c.link != Link::Idle || now < c.next_try {
            return;
        }
        let results = a.state.lock().results.clone();
        match wanted(&c, &results) {
            Some(p) => {
                c.link = Link::Associating;
                p
            }
            None => {
                c.next_try = now + 10_000;
                return;
            }
        }
    };
    let r = join(i, a, &profile);
    let mut c = a.conn.lock();
    match r {
        Ok(()) => {}
        Err(e) => {
            crate::wlog!("wlan{i}: cannot join {}: {e}", wlan::ie::ssid_text(&profile.ssid));
            lost(a, &mut c, e, 5000);
        }
    }
}

/// The RSN element the station offers: version 1, the AP's group
/// cipher, CCMP pairwise, PSK.
fn own_rsn_ie(group: u8) -> Vec<u8> {
    let mut v = alloc::vec![48, 20, 1, 0, 0x00, 0x0f, 0xac, group, 1, 0, 0x00, 0x0f, 0xac, wlan::ie::CIPHER_CCMP, 1, 0, 0x00, 0x0f, 0xac, wlan::ie::AKM_PSK];
    v.extend([0, 0]);
    v
}

/// The AP's RSN element as sent (id, length, body).
fn ap_rsn_ie(b: &Bss) -> Option<Vec<u8>> {
    let mut p = &b.ies[..];
    while p.len() >= 2 && p.len() >= 2 + p[1] as usize {
        let n = 2 + p[1] as usize;
        if p[0] == wlan::ie::EID_RSN {
            return Some(p[..n].to_vec());
        }
        p = &p[n..];
    }
    None
}

fn random32() -> [u8; 32] {
    let mut r = [0u8; 32];
    let n = crate::chardev::random_fill(&mut r);
    if n < 32 {
        // No RDRAND/virtio-rng: hash what varies.
        let mut s = wlan::crypto::Sha1::new();
        for _ in 0..4 {
            s.update(&crate::arch::rdtsc().to_le_bytes());
            s.update(&crate::irq::uptime_ms().to_le_bytes());
        }
        let h = s.finish();
        let h2 = wlan::crypto::sha1(&[&h[..], &crate::arch::rdtsc().to_le_bytes()[..]].concat());
        r[..20].copy_from_slice(&h);
        r[20..].copy_from_slice(&h2[..12]);
    }
    r
}

/// Scan, pick the best access point of `p`'s network, associate, and set
/// up the handshake. Errors are described for WIFI STATUS.
fn join(i: usize, a: &Arc<Adapter>, p: &Profile) -> Result<(), String> {
    let name = wlan::ie::ssid_text(&p.ssid);
    let found = scan(i, &ScanRequest { ssid: p.ssid.clone() }).map_err(|e| alloc::format!("scan: {e}"))?;
    let bss = found
        .iter()
        .filter(|b| b.ssid == p.ssid && p.bssid.is_none_or(|m| m == b.bssid))
        .max_by_key(|b| b.rssi)
        .ok_or_else(|| alloc::format!("{name} is not in range"))?
        .clone();
    let sec = bss.security();
    let rsn = match sec {
        Security::Open => None,
        Security::Wpa2Psk | Security::Wpa2Wpa3 => {
            let r = bss.rsn.clone().ok_or("no RSN element")?;
            if !r.has_pairwise(wlan::ie::CIPHER_CCMP) {
                return Err("the network offers no CCMP (AES) encryption".into());
            }
            if !r.has_akm(wlan::ie::AKM_PSK) {
                return Err("the network offers no WPA2-PSK".into());
            }
            if r.caps & 1 << 6 != 0 {
                return Err("the network requires protected management frames".into());
            }
            if passphrase(p)?.is_none() {
                return Err(alloc::format!("{name} needs a passphrase (WIFI CONNECT \"{name}\" passphrase)"));
            }
            Some(own_rsn_ie(r.group))
        }
        s => return Err(alloc::format!("{} networks are not supported", s.name())),
    };
    crate::wlog!("wlan{i}: joining {name} ({}, channel {}, {} dBm, {})", MacFmt(bss.bssid), bss.channel, bss.rssi, sec.name());
    let pass = if rsn.is_some() { passphrase(p)? } else { None };
    let pmk = match (&rsn, &pass) {
        (Some(_), Some(pass)) => {
            let cached = a.conn.lock().pmk.as_ref().filter(|c| c.0 == p.ssid && &c.1 == pass).map(|c| c.2);
            Some(cached.unwrap_or_else(|| {
                let k = wlan::crypto::psk(pass.as_bytes(), &p.ssid);
                a.conn.lock().pmk = Some((p.ssid.clone(), pass.clone(), k));
                k
            }))
        }
        _ => None,
    };
    let aid = {
        let _g = a.busy.lock();
        a.drv.connect(&bss, rsn.as_deref()).map_err(|e| alloc::format!("{e}"))?
    };
    let mut c = a.conn.lock();
    c.bss = Some(bss.clone());
    c.gtk_set = false;
    match (rsn, pmk) {
        (Some(ie), Some(pmk)) => {
            crate::wlog!("wlan{i}: associated (AID {aid}); WPA2 handshake");
            let mut s = Supplicant::new(pmk, bss.bssid, a.drv.mac_address(), ie, random32());
            if let Some(ap) = ap_rsn_ie(&bss) {
                s.set_ap_rsn_ie(ap);
            }
            c.supp = Some(s);
            c.link = Link::Handshake(crate::irq::uptime_ms());
        }
        _ => {
            crate::wlog!("wlan{i}: connected to {name} (AID {aid}, open)");
            c.link = Link::Connected;
            c.failures = 0;
            c.error.clear();
            a.port_open.store(true, Ordering::Release);
            super::kick();
        }
    }
    Ok(())
}

/// An EAPOL packet from the AP: through the supplicant, and act on what
/// it says.
fn eapol(i: usize, a: &Arc<Adapter>, pkt: &[u8]) {
    let (mut s, bssid) = {
        let mut c = a.conn.lock();
        let Some(s) = c.supp.take() else { return };
        let Some(b) = c.bss.as_ref().map(|b| b.bssid) else { return };
        (s, b)
    };
    let what = match wlan::wpa::KeyFrame::parse(pkt) {
        Some(k) if k.info & 1 << 3 == 0 => ("group message 1", "group message 2"),
        Some(k) if k.info & 1 << 8 == 0 => ("message 1", "message 2"),
        Some(_) => ("message 3", "message 4"),
        None => ("EAPOL frame (not an EAPOL-Key it knows)", "reply"),
    };
    let info = wlan::wpa::KeyFrame::parse(pkt).map_or(0, |k| k.info);
    let actions = s.rx_eapol(pkt);
    crate::wlog!("wlan{i}: WPA2 {} in ({} bytes, key info {info:#06x}): {} action(s)", what.0, pkt.len(), actions.len());
    let mac = a.drv.mac_address();
    let mut failed = None;
    let mut complete = false;
    for act in actions {
        match act {
            Action::Send(p) => {
                let mut f = Vec::with_capacity(14 + p.len());
                f.extend(bssid);
                f.extend(mac);
                f.extend(ETHERTYPE_EAPOL.to_be_bytes());
                f.extend(&p);
                if a.drv.send(&f) {
                    crate::wlog!("wlan{i}: WPA2 {} sent ({} bytes)", what.1, p.len());
                } else {
                    crate::wlog!("wlan{i}: WPA2 {} not sent (transmit ring full)", what.1);
                }
            }
            Action::InstallPtk { tk } => {
                // Message 4 must leave unencrypted, before the key.
                a.drv.flush_tx(200);
                let first = !a.conn.lock().gtk_set;
                let k = Key { pairwise: true, peer: bssid, index: 0, key: tk.to_vec(), rsc: 0, first };
                if let Err(e) = a.drv.set_key(&k) {
                    failed = Some(alloc::format!("pairwise key: {e}"));
                }
            }
            Action::InstallGtk { key, index, rsc } => {
                let first = !a.conn.lock().gtk_set;
                let k = Key { pairwise: false, peer: [0xff; 6], index, key, rsc, first };
                match a.drv.set_key(&k) {
                    Ok(()) => a.conn.lock().gtk_set = true,
                    Err(e) => failed = Some(alloc::format!("group key: {e}")),
                }
            }
            Action::Complete => complete = true,
            Action::Failed(w) => failed = Some(String::from(w)),
        }
    }
    let mut c = a.conn.lock();
    if let Some(why) = failed {
        crate::wlog!("wlan{i}: WPA2 handshake failed: {why}");
        lost(a, &mut c, alloc::format!("WPA2: {why}"), 5000);
        drop(c);
        a.drv.disconnect();
        return;
    }
    c.supp = Some(s);
    if complete && c.link != Link::Connected {
        let name = c.bss.as_ref().map(|b| b.ssid_text()).unwrap_or_default();
        crate::wlog!("wlan{i}: connected to {name} (WPA2)");
        c.link = Link::Connected;
        c.failures = 0;
        c.error.clear();
        a.port_open.store(true, Ordering::Release);
        super::kick();
    }
}

/// Join `ssid` now (WIFI CONNECT): it replaces the current network.
/// `passphrase` None takes the saved one (keychain), if any.
pub fn connect(i: usize, ssid: &[u8], passphrase: Option<&str>) -> Result<(), WlanError> {
    let a = adapter(i).ok_or(WlanError::NotReady)?;
    let mut p = PROFILES.lock().iter().find(|p| p.ssid == ssid).cloned().unwrap_or_else(|| Profile::new(ssid, None));
    if let Some(k) = passphrase {
        // Kept for the next boot in the keychain (not in WIFI.CFG).
        if let Err(e) = crate::keychain::set(KEYCHAIN_SERVICE, ssid, k.as_bytes(), false) {
            crate::wlog!("wlan{i}: the passphrase is not saved: {e}");
        }
        p.passphrase = Some(k.into());
    }
    let was = {
        let mut c = a.conn.lock();
        c.target = Some(p);
        c.off = false;
        c.failures = 0;
        c.next_try = 0;
        c.error.clear();
        let was = c.link != Link::Idle;
        if was {
            c.link = Link::Idle;
            c.supp = None;
            a.port_open.store(false, Ordering::Release);
        }
        was
    };
    if was {
        a.drv.disconnect();
    }
    kick();
    Ok(())
}

/// Leave the network and stay off until [`connect`] (WIFI DISCONNECT).
pub fn disconnect(i: usize) -> Result<(), WlanError> {
    let a = adapter(i).ok_or(WlanError::NotReady)?;
    {
        let mut c = a.conn.lock();
        c.off = true;
        c.target = None;
        c.link = Link::Idle;
        c.supp = None;
        a.port_open.store(false, Ordering::Release);
    }
    a.drv.disconnect();
    Ok(())
}

/// Scan on adapter `i` and keep the results.
pub fn scan(i: usize, req: &ScanRequest) -> Result<Vec<Bss>, WlanError> {
    let a = adapter(i).ok_or(WlanError::NotReady)?;
    if a.state.lock().status != Status::Up {
        return Err(WlanError::NotReady);
    }
    let _g = a.busy.lock();
    let v = a.drv.scan(req)?;
    let mut found = Vec::new();
    wlan::ie::merge(&mut found, v);
    let mut s = a.state.lock();
    s.results = found.clone();
    s.last_scan_ms = crate::irq::uptime_ms();
    Ok(found)
}

/// One network on one line: SSID, signal, channel, security, BSSID.
pub fn describe(b: &Bss) -> String {
    let ssid = if b.hidden() { String::from("(hidden)") } else { b.ssid_text() };
    alloc::format!("{ssid:<32} {:>4} dBm  ch {:>3} ({})  {:<9} {}", b.rssi, b.channel, b.band(), b.security().name(), MacFmt(b.bssid))
}

/// The scan results of adapter `i` as text for RS2API: one network per
/// line, fields separated by tabs: SSID, RSSI (dBm), channel, security,
/// BSSID, known (1 if WIFI.CFG has it).
pub fn results_text(v: &[Bss]) -> String {
    let profiles = PROFILES.lock().clone();
    let mut s = String::new();
    for b in v {
        let known = profiles.iter().any(|p| p.ssid == b.ssid);
        let ssid: String = b.ssid.iter().map(|&c| c as char).collect();
        let _ = write!(s, "{ssid}\t{}\t{}\t{}\t{}\t{}\n", b.rssi, b.channel, b.security().name(), MacFmt(b.bssid), known as u8);
    }
    s
}

/// The adapters' state as `key=value` lines (`adapter=N` starts each).
pub fn status_text() -> String {
    let mut s = String::new();
    let all = ADAPTERS.lock().clone();
    for (i, a) in all.iter().enumerate() {
        let st = a.state.lock();
        let _ = write!(s, "adapter={i}\nname={}\nstatus={}\n", a.drv.name(), st.status.text());
        let up = st.status == Status::Up;
        if up {
            let _ = write!(s, "mac={}\ninfo={}\nnetworks={}\n", MacFmt(a.drv.mac_address()), st.info, st.results.len());
        }
        drop(st);
        if up {
            let c = a.conn.lock();
            let _ = write!(s, "state={}\n", if c.off { "off" } else { c.link.name() });
            if c.link != Link::Idle {
                if let Some(b) = &c.bss {
                    let ssid: String = b.ssid.iter().map(|&ch| ch as char).collect();
                    let _ = write!(s, "ssid={ssid}\nbssid={}\nchannel={}\nsignal={} dBm\nsecurity={}\n", MacFmt(b.bssid), b.channel, b.rssi, b.security().name());
                }
            } else if let Some(t) = &c.target {
                let ssid: String = t.ssid.iter().map(|&ch| ch as char).collect();
                let _ = write!(s, "wanted={ssid}\n");
            }
            if !c.error.is_empty() {
                let _ = write!(s, "error={}\n", c.error);
            }
        }
        for (k, v) in a.drv.diagnostics() {
            let _ = write!(s, "{k}={v}\n");
        }
    }
    let _ = write!(s, "profiles={}\n", PROFILES.lock().len());
    for l in LOG.lock().iter() {
        let _ = write!(s, "log={l}\n");
    }
    s
}

/// The last scan's results of adapter `i`.
pub fn last_results(i: usize) -> Option<Vec<Bss>> {
    adapter(i).map(|a| a.state.lock().results.clone())
}
