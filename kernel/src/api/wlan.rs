//! RS2API 620-622: the WiFi adapters (`net::wlan`) for WIFI.EXE. Calls
//! return 0 or an OS/2 error; text comes back as in `api::lan`.
//!
//! | Ord | Entry |
//! |-----|-------|
//! | 620 | Rs2WlanQuery(PCHAR buf, ULONG cb, PULONG pcb): `key=value` lines (`net::wlan::status_text`) |
//! | 621 | Rs2WlanScan(ULONG adapter, ULONG flags, PCHAR buf, ULONG cb, PULONG pcb): scan (flags bit 0: return the last results without scanning); one network per line: SSID, RSSI, channel, security, BSSID, known, tab-separated |
//! | 622 | Rs2WlanReload(): read `\MPTN\ETC\WIFI.CFG` again |
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
