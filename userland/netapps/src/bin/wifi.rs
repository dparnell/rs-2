//! WIFI: rs2's WiFi adapters (docs/SURFACE-PRO-4.md, "Phase 4: WiFi"),
//! through RS2API 620-622 (`kernel/src/api/wlan.rs`).
//!
//! `WIFI [STATUS]`: the adapters and their state.
//! `WIFI SCAN [n]`: scan with adapter n (default 0) and list the networks.
//! `WIFI LIST [n]`: the networks the last scan found.
//! `WIFI RELOAD`: read `\MPTN\ETC\WIFI.CFG` again.
//! `WIFI CONNECT ssid [passphrase]`: join a network (saved in WIFI.CFG,
//! first, so it is joined again at the next boot; the passphrase goes into
//! the system keychain, not the file) and wait until the link is up or it
//! fails. Quote an SSID with blanks: `WIFI CONNECT "My Net" pw`.
//! `WIFI DISCONNECT`: leave the network (until the next CONNECT or boot).
//! `WIFI FORGET ssid`: remove a network from WIFI.CFG and its passphrase
//! from the keychain.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::{String, ToString};
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
    #[link_name = "RS2API.623"]
    fn Rs2WlanConnect(adapter: u32, ssid: *const u8, passphrase: *const u8) -> u32;
    #[link_name = "RS2API.624"]
    fn Rs2WlanDisconnect(adapter: u32) -> u32;
    #[link_name = "RS2API.630"]
    fn Rs2KeyQuery(buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.633"]
    fn Rs2KeyDelete(service: *const u8, account: *const u8) -> u32;
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

const CONFIG: &str = "C:\\MPTN\\ETC\\WIFI.CFG";

fn cstr(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).chain(core::iter::once(0)).collect()
}

fn bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

fn read_config() -> Vec<wlan::config::Profile> {
    match utils::files::read_file(CONFIG) {
        Ok(d) => wlan::config::parse(&d.iter().map(|&b| b as char).collect::<String>()).profiles,
        Err(_) => Vec::new(),
    }
}

fn write_config(p: &[wlan::config::Profile]) -> bool {
    let t = wlan::config::write(p);
    match utils::files::write_file(CONFIG, &bytes(&t)) {
        Ok(()) => true,
        Err(rc) => {
            eprint(&alloc::format!("WIFI: cannot write {CONFIG} (error {rc})\r\n"));
            false
        }
    }
}

fn query() -> String {
    text(|b, cb, pcb| unsafe { Rs2WlanQuery(b, cb, pcb) }, |b, cb, pcb| unsafe { Rs2WlanQuery(b, cb, pcb) }).unwrap_or_default()
}

fn field<'a>(t: &'a str, k: &str) -> Option<&'a str> {
    t.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
}

fn keychain_locked() -> bool {
    let mut buf = [0u8; 256];
    let mut n = 0u32;
    let rc = unsafe { Rs2KeyQuery(buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    (rc == 0 || rc == ERROR_BUFFER_OVERFLOW) && buf.starts_with(b"state=locked")
}

fn connect(ssid: &str, pass: Option<&str>) -> u32 {
    if ssid.is_empty() || ssid.len() > 32 {
        eprint("WIFI: an SSID is 1 to 32 characters\r\n");
        return 1;
    }
    if pass.is_some_and(|p| !(8..=63).contains(&p.len())) {
        eprint("WIFI: a WPA2 passphrase is 8 to 63 characters\r\n");
        return 1;
    }
    // Save it first in WIFI.CFG. The passphrase is not written there: the
    // kernel keeps it in the system keychain (a passphrase still in the
    // file is moved there when it is read again).
    let mut ps = read_config();
    let mut p = ps.iter().position(|p| p.ssid == bytes(ssid)).map(|i| ps.remove(i)).unwrap_or_else(|| wlan::config::Profile::new(&bytes(ssid), None));
    if pass.is_some() {
        p.passphrase = None;
    }
    p.autoconnect = true;
    ps.insert(0, p);
    if write_config(&ps) {
        unsafe { Rs2WlanReload() };
    }
    let (s, k) = (cstr(ssid), pass.map(cstr));
    let rc = unsafe { Rs2WlanConnect(0, s.as_ptr(), k.as_ref().map_or(core::ptr::null(), |k| k.as_ptr())) };
    if rc != 0 {
        eprint(&alloc::format!("WIFI: {} (error {rc})\r\n", why(rc)));
        return 2;
    }
    if pass.is_some() && keychain_locked() {
        outln("Note: the keychain is locked, so the passphrase is not saved (KEYCHAIN UNLOCK).");
    }
    outln(&alloc::format!("Connecting to {ssid}..."));
    let mut last = String::new();
    for _ in 0..90 {
        unsafe { os2::DosSleep(500) };
        let t = query();
        let state = field(&t, "state").unwrap_or("").to_string();
        let err = field(&t, "error").unwrap_or("").to_string();
        if state != last {
            outln(&alloc::format!("  {state}"));
            last = state.clone();
        }
        if state == "connected" {
            outln(&alloc::format!("Connected to {ssid}; the address comes from DHCP (IFCONFIG shows it)."));
            return 0;
        }
        if state == "disconnected" && !err.is_empty() {
            outln(&alloc::format!("Not connected: {err}"));
            outln("(rs2 keeps trying; WIFI STATUS shows how it goes, WIFI DISCONNECT stops it)");
            return 2;
        }
    }
    outln("Still not connected after 45 seconds; rs2 keeps trying (WIFI STATUS).");
    2
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let cmd = a.first().map(|c| c.to_ascii_uppercase()).unwrap_or_default();
    let n = a.get(1).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    match cmd.as_str() {
        "" | "STATUS" => status(),
        "SCAN" => list(n, true),
        "LIST" => list(n, false),
        "CONNECT" => match a.get(1) {
            Some(ssid) => connect(ssid, a.get(2).map(|s| s.as_str())),
            None => {
                eprint("Usage: WIFI CONNECT ssid [passphrase]\r\n");
                1
            }
        },
        "DISCONNECT" => {
            let rc = unsafe { Rs2WlanDisconnect(n) };
            if rc != 0 {
                eprint(&alloc::format!("WIFI: {} (error {rc})\r\n", why(rc)));
                return 2;
            }
            outln("Disconnected.");
            0
        }
        "FORGET" => {
            let Some(ssid) = a.get(1) else {
                eprint("Usage: WIFI FORGET ssid\r\n");
                return 1;
            };
            let mut ps = read_config();
            let before = ps.len();
            ps.retain(|p| p.ssid != bytes(ssid));
            let in_keychain = unsafe { Rs2KeyDelete(cstr("wifi").as_ptr(), cstr(ssid).as_ptr()) } == 0;
            if ps.len() == before {
                if in_keychain {
                    outln(&alloc::format!("The passphrase of {ssid} removed from the keychain."));
                    return 0;
                }
                eprint(&alloc::format!("WIFI: {ssid} is not in {CONFIG}\r\n"));
                return 1;
            }
            if !write_config(&ps) {
                return 2;
            }
            unsafe { Rs2WlanReload() };
            outln(&alloc::format!("{ssid} removed from {CONFIG}."));
            0
        }
        "RELOAD" => {
            unsafe { Rs2WlanReload() };
            status()
        }
        _ => {
            eprint("Usage: WIFI [STATUS | SCAN [n] | LIST [n] | CONNECT ssid [passphrase] | DISCONNECT | FORGET ssid | RELOAD]\r\n");
            1
        }
    }
}
