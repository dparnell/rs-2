//! WIFI: rs2's WiFi adapters (docs/SURFACE-PRO-4.md, "Phase 4: WiFi"),
//! through RS2API 620-622 (`kernel/src/api/wlan.rs`).
//!
//! `WIFI [STATUS]`: the adapters and their state.
//! `WIFI SCAN [n]`: scan with adapter n (default 0) and list the networks.
//! `WIFI LIST [n]`: the networks the last scan found.
//! `WIFI RELOAD`: read `\MPTN\ETC\WIFI.CFG` again.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "RS2API.620"]
    fn Rs2WlanQuery(buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.621"]
    fn Rs2WlanScan(adapter: u32, flags: u32, buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.622"]
    fn Rs2WlanReload() -> u32;
}

const ERROR_BUFFER_OVERFLOW: u32 = 111;

/// Call `f(buf, cb, pcb)` with a buffer large enough for its text. `again`
/// is the call to repeat after an overflow (a scan is not redone).
fn text(mut f: impl FnMut(*mut u8, u32, *mut u32) -> u32, mut again: impl FnMut(*mut u8, u32, *mut u32) -> u32) -> Result<String, u32> {
    let mut cb = 16384u32;
    let mut first = true;
    loop {
        let mut buf = alloc::vec![0u8; cb as usize];
        let mut need = 0u32;
        let rc = if first { f(buf.as_mut_ptr(), cb, &mut need) } else { again(buf.as_mut_ptr(), cb, &mut need) };
        first = false;
        match rc {
            0 => {
                let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                return Ok(buf[..n].iter().map(|&b| b as char).collect());
            }
            ERROR_BUFFER_OVERFLOW if need > cb => cb = need,
            e => return Err(e),
        }
    }
}

fn pad(s: &str, w: usize) -> String {
    let mut v: String = s.chars().take(w).collect();
    while v.chars().count() < w {
        v.push(' ');
    }
    v
}

fn why(rc: u32) -> &'static str {
    match rc {
        21 => "the adapter is not ready (still starting, or it failed: see WIFI STATUS)",
        55 => "no such WiFi adapter",
        31 => "the scan failed (see the boot log)",
        _ => "error",
    }
}

fn status() -> u32 {
    match text(|b, cb, pcb| unsafe { Rs2WlanQuery(b, cb, pcb) }, |b, cb, pcb| unsafe { Rs2WlanQuery(b, cb, pcb) }) {
        Ok(t) => {
            if !t.lines().any(|l| l.starts_with("adapter=")) {
                outln("No WiFi adapter found.");
            }
            for l in t.lines() {
                let Some((k, v)) = l.split_once('=') else { continue };
                if k == "adapter" {
                    outln(&alloc::format!("Adapter {v}:"));
                } else if k == "profiles" {
                    outln(&alloc::format!("Networks in \\MPTN\\ETC\\WIFI.CFG: {v}"));
                } else {
                    outln(&alloc::format!("  {}{v}", pad(&alloc::format!("{k}:"), 12)));
                }
            }
            0
        }
        Err(rc) => {
            eprint(&alloc::format!("WIFI: status failed (error {rc})\r\n"));
            2
        }
    }
}

fn list(adapter: u32, scan: bool) -> u32 {
    if scan {
        outln("Scanning...");
    }
    let flags = if scan { 0 } else { 1 };
    let r = text(|b, cb, pcb| unsafe { Rs2WlanScan(adapter, flags, b, cb, pcb) }, |b, cb, pcb| unsafe { Rs2WlanScan(adapter, 1, b, cb, pcb) });
    let t = match r {
        Ok(t) => t,
        Err(rc) => {
            eprint(&alloc::format!("WIFI: {} (error {rc})\r\n", why(rc)));
            return 2;
        }
    };
    let rows: Vec<Vec<&str>> = t.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').collect()).filter(|r: &Vec<&str>| r.len() >= 6).collect();
    if rows.is_empty() {
        outln("No networks found.");
        return 0;
    }
    outln(&alloc::format!("{} {} {} {} {}", pad("SSID", 32), pad("Signal", 8), pad("Channel", 8), pad("Security", 10), "BSSID"));
    for r in &rows {
        let ssid = if r[0].bytes().all(|b| b == 0) || r[0].is_empty() { "(hidden)" } else { r[0] };
        let known = if r[5] == "1" { " *" } else { "" };
        outln(&alloc::format!("{} {} {} {} {}{known}", pad(ssid, 32), pad(&alloc::format!("{} dBm", r[1]), 8), pad(r[2], 8), pad(r[3], 10), r[4]));
    }
    outln(&alloc::format!("{} network(s); * = in WIFI.CFG", rows.len()));
    0
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let cmd = a.first().map(|c| c.to_ascii_uppercase()).unwrap_or_default();
    let n = a.get(1).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    match cmd.as_str() {
        "" | "STATUS" => status(),
        "SCAN" => list(n, true),
        "LIST" => list(n, false),
        "RELOAD" => {
            unsafe { Rs2WlanReload() };
            status()
        }
        _ => {
            eprint("Usage: WIFI [STATUS | SCAN [adapter] | LIST [adapter] | RELOAD]\r\n");
            1
        }
    }
}
