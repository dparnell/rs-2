//! Wireless LAN (docs/SURFACE-PRO-4.md, "Phase 4: WiFi"): the layer
//! between WiFi chip drivers and the rest of rs2.
//!
//! Chip drivers implement [`WlanDriver`] and [`register`] their adapters
//! while the PCI bus is probed. A chip that needs firmware cannot start
//! then (no file system yet), so [`start`], called once the boot volume
//! and CONFIG.SYS are there, runs a kernel thread ("wlan") that brings
//! each adapter up ([`WlanDriver::start`]: firmware download, chip
//! initialisation), reads `\MPTN\ETC\WIFI.CFG` and scans once, logging
//! what it finds.
//!
//! Programs reach it through RS2API 620-621 (`api::wlan`, WIFI.EXE):
//! the adapters' state and a scan.
//!
//! Still to come (the next rounds of phase 4): association with a
//! profile from WIFI.CFG, the WPA2 supplicant, and a `net::Nic` per
//! adapter that is up only while associated and keyed.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write;

use wlan::ie::Bss;
use wlan::MacFmt;

use crate::sync::{Mutex, SpinLock};

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
}

static ADAPTERS: SpinLock<Vec<Arc<Adapter>>> = SpinLock::new(Vec::new());
static PROFILES: SpinLock<Vec<wlan::config::Profile>> = SpinLock::new(Vec::new());

/// A driver found a chip.
pub fn register(drv: Arc<dyn WlanDriver>) {
    crate::println!("wlan: {} found", drv.name());
    let a = Adapter {
        drv,
        state: SpinLock::new(State { status: Status::Found, info: String::new(), results: Vec::new(), last_scan_ms: 0 }),
        busy: Mutex::new(()),
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
    let Some(data) = crate::fs::read_file(CONFIG_FILE) else { return };
    let text: String = data.iter().map(|&b| b as char).collect();
    let c = wlan::config::parse(&text);
    for (line, why) in &c.errors {
        crate::println!("wlan: {CONFIG_FILE} line {line}: {why}");
    }
    crate::println!("wlan: {} network(s) in {CONFIG_FILE}", c.profiles.len());
    *PROFILES.lock() = c.profiles;
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
                    crate::println!("wlan{i}: {} up in {} ms: {info}", a.drv.name(), crate::irq::uptime_ms() - t0);
                    let mut s = a.state.lock();
                    s.status = Status::Up;
                    s.info = info;
                }
                Err(e) => {
                    crate::println!("wlan{i}: {} failed to start: {e}", a.drv.name());
                    a.state.lock().status = Status::Failed(alloc::format!("{e}"));
                    continue;
                }
            }
            match scan(i, &ScanRequest::default()) {
                Ok(v) => {
                    crate::println!("wlan{i}: scan found {} network(s)", v.len());
                    for b in &v {
                        crate::println!("wlan{i}:   {}", describe(b));
                    }
                }
                Err(e) => crate::println!("wlan{i}: scan failed: {e}"),
            }
        }
    });
}

/// Scan on adapter `i` and keep the results.
pub fn scan(i: usize, req: &ScanRequest) -> Result<Vec<Bss>, WlanError> {
    let a = adapter(i).ok_or(WlanError::NotReady)?;
    if a.state.lock().status != Status::Up {
        return Err(WlanError::NotReady);
    }
    let _g = a.busy.lock();
    let mut v = a.drv.scan(req)?;
    let mut found = Vec::new();
    wlan::ie::merge(&mut found, core::mem::take(&mut v));
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
        if st.status == Status::Up {
            let _ = write!(s, "mac={}\ninfo={}\nnetworks={}\n", MacFmt(a.drv.mac_address()), st.info, st.results.len());
        }
        drop(st);
        for (k, v) in a.drv.diagnostics() {
            let _ = write!(s, "{k}={v}\n");
        }
    }
    let _ = write!(s, "profiles={}\n", PROFILES.lock().len());
    s
}

/// The last scan's results of adapter `i`.
pub fn last_results(i: usize) -> Option<Vec<Bss>> {
    adapter(i).map(|a| a.state.lock().results.clone())
}
