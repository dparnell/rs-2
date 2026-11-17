//! RS2API 620-624: the WiFi adapters (`net::wlan`) for WIFI.EXE. Calls
//! return 0 or an OS/2 error; text comes back as in `api::lan`.
//!
//! | Ord | Entry |
//! |-----|-------|
//! | 620 | Rs2WlanQuery(PCHAR buf, ULONG cb, PULONG pcb): `key=value` lines (`net::wlan::status_text`) |
//! | 621 | Rs2WlanScan(ULONG adapter, ULONG flags, PCHAR buf, ULONG cb, PULONG pcb): scan (flags bit 0: return the last results without scanning); one network per line: SSID, RSSI, channel, security, BSSID, known, tab-separated |
//! | 622 | Rs2WlanReload(): read `\MPTN\ETC\WIFI.CFG` again |
//! | 623 | Rs2WlanConnect(ULONG adapter, PSZ ssid, PSZ passphrase or NULL): join that network (in the background; Rs2WlanQuery's `state=` follows it) |
//! | 624 | Rs2WlanDisconnect(ULONG adapter): leave the network, join none until asked |
//!
//! A text larger than `cb` gives ERROR_BUFFER_OVERFLOW with the size
//! needed in `*pcb` (a scan's results are kept: ask again with flags 1).

use super::{err, Ctx};
use crate::net::wlan::{self, ScanRequest, WlanError};

/// ERROR_NOT_READY.
const NOT_READY: u32 = 21;
/// ERROR_GEN_FAILURE.
const GEN_FAILURE: u32 = 31;
/// ERROR_DEV_NOT_EXIST.
const DEV_NOT_EXIST: u32 = 55;

/// Copy a text result to (buf, cb, pcb) at arguments i, i+1, i+2.
fn out(c: &Ctx, text: &str, i: usize) -> u32 {
    let (buf, cb, pcb) = (c.arg(i), c.arg(i + 1), c.arg(i + 2));
    let b: alloc::vec::Vec<u8> = text.chars().map(|ch| ch as u32 as u8).chain(core::iter::once(0)).collect();
    if pcb != 0 {
        c.write_u32(pcb, b.len() as u32);
    }
    if b.len() > cb as usize {
        return err::BUFFER_OVERFLOW;
    }
    if c.write(buf, &b) { 0 } else { err::INVALID_ADDRESS }
}

pub fn query(c: &mut Ctx) -> u32 {
    let t = wlan::status_text();
    out(c, &t, 0)
}

pub fn scan(c: &mut Ctx) -> u32 {
    let (i, flags) = (c.arg(0) as usize, c.arg(1));
    if i >= wlan::count() {
        return DEV_NOT_EXIST;
    }
    let r = if flags & 1 != 0 { wlan::last_results(i).ok_or(WlanError::NotReady) } else { wlan::scan(i, &ScanRequest::default()) };
    match r {
        Ok(v) => {
            let t = wlan::results_text(&v);
            out(c, &t, 2)
        }
        Err(WlanError::NotReady) => NOT_READY,
        Err(e) => {
            crate::println!("wlan{i}: scan failed: {e}");
            GEN_FAILURE
        }
    }
}

pub fn reload(_c: &mut Ctx) -> u32 {
    wlan::load_config();
    0
}

pub fn connect(c: &mut Ctx) -> u32 {
    let i = c.arg(0) as usize;
    if i >= wlan::count() {
        return DEV_NOT_EXIST;
    }
    let Some(ssid) = c.read_str(c.arg(1), 33) else { return err::INVALID_ADDRESS };
    let pass = match c.arg(2) {
        0 => None,
        at => match c.read_str(at, 64) {
            Some(p) if p.is_empty() => None,
            Some(p) => Some(p),
            None => return err::INVALID_ADDRESS,
        },
    };
    let ssid: alloc::vec::Vec<u8> = ssid.chars().map(|ch| ch as u32 as u8).collect();
    if ssid.is_empty() || ssid.len() > 32 || pass.as_ref().is_some_and(|p| !(8..=63).contains(&p.len())) {
        return err::INVALID_PARAMETER;
    }
    match wlan::connect(i, &ssid, pass.as_deref()) {
        Ok(()) => 0,
        Err(_) => NOT_READY,
    }
}

pub fn disconnect(c: &mut Ctx) -> u32 {
    let i = c.arg(0) as usize;
    if i >= wlan::count() {
        return DEV_NOT_EXIST;
    }
    match wlan::disconnect(i) {
        Ok(()) => 0,
        Err(_) => NOT_READY,
    }
}
