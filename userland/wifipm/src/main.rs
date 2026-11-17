//! WIFIPM.EXE: the WiFi window, the Presentation Manager counterpart of
//! WIFI.EXE (docs/SURFACE-PRO-4.md, "Phase 4: WiFi"), through the same
//! RS2API 620-624 (`kernel/src/api/wlan.rs`) and the keychain's 630/633.
//!
//! One window (a dialog, as PMSEEK's): the connection (adapter, state,
//! network, signal, IP address), refreshed every few seconds; the
//! networks found, strongest first, with their signal, security and
//! whether they are saved (hidden networks are left out and counted);
//! Connect, Disconnect, Forget, Scan and Close. Connect asks for the
//! passphrase of a secured network not saved yet (or saved without one
//! in the keychain); the network is saved in `\MPTN\ETC\WIFI.CFG` first,
//! as WIFI CONNECT does, and the kernel keeps the passphrase in the
//! system keychain. The kernel joins in the background: the status shows
//! how it goes. A scan runs on a second thread (it takes a few seconds).
//! Forget removes the network from WIFI.CFG and its passphrase from the
//! keychain (as WIFI FORGET), and leaves it when connected to it.
//! "Power saving" sets `POWERSAVE=` in WIFI.CFG (as WIFI POWERSAVE) and
//! has the kernel read the file again; next to it, whether the chip is
//! asleep or awake now. Every writer of WIFI.CFG keeps what it does not
//! change.
//!
//! Without an adapter (QEMU has none) the window says so and offers only
//! Close. A line on the serial port (`[wifipm] ...`) tells what it found.
//!
//! `WIFIPM /DEMO` shows made-up networks and a connection without asking
//! the kernel (for screenshots and trying the window in QEMU); Connect
//! there asks for the passphrase but joins nothing, and Power saving
//! writes nothing.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use pmkit::*;

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
const ERROR_NOT_READY: u32 = 21;
const ERROR_DEV_NOT_EXIST: u32 = 55;
const ES_UNREADABLE: u32 = 0x0080;

const DLG_MAIN: u16 = 100;
const DLG_PASS: u16 = 200;

const ID_ADAPTER: u32 = 10;
const ID_STATE: u32 = 11;
const ID_SIGNAL: u32 = 12;
const ID_ADDRESS: u32 = 13;
const ID_NOTE: u32 = 14;
const ID_NETLABEL: u32 = 15;
const ID_LIST: u32 = 16;
const ID_POWERSAVE: u32 = 17;
const ID_POWER: u32 = 18;
const ID_CONNECT: u32 = 20;
const ID_DISCONNECT: u32 = 21;
const ID_FORGET: u32 = 22;
const ID_SCAN: u32 = 23;
const ID_CLOSE: u32 = 24;
const ID_PASS: u32 = 30;
const ID_PASSLABEL: u32 = 31;

const TID_REFRESH: u32 = 1;
/// The status is read again this often (ms).
const REFRESH_MS: u32 = 3000;
/// A scan's results (a boxed `Result<String, u32>` in mp1).
const WM_SCANNED: u32 = WM_USER + 1;
/// The height of a network's line (pixels).
const ROW: u32 = 20;

const TITLE: &[u8] = b"WiFi";

/// A network of the list (the strongest access point of an SSID).
#[derive(Clone, PartialEq)]
struct Net {
    ssid: Vec<u8>,
    rssi: i32,
    channel: u32,
    security: String,
    known: bool,
}

/// Adapter 0 as Rs2WlanQuery describes it.
#[derive(Default, Clone)]
struct Status {
    /// An adapter exists.
    present: bool,
    name: String,
    /// "found", "starting", "up", "failed: ...".
    status: String,
    /// "off", "disconnected", "connecting", "authenticating", "connected".
    state: String,
    ssid: String,
    signal: String,
    channel: String,
    security: String,
    /// The standard the link runs ("802.11ac", "802.11n", "802.11g"...).
    standard: String,
    wanted: String,
    error: String,
    mac: Option<[u8; 6]>,
    /// "on"/"off", and "awake"/"asleep" while on (empty from a kernel
    /// that does not tell).
    power_save: String,
    power_state: String,
}

impl Status {
    fn up(&self) -> bool {
        self.present && self.status == "up"
    }
    /// Joining, joined or trying again: something Disconnect stops.
    fn busy(&self) -> bool {
        self.up() && (matches!(self.state.as_str(), "connecting" | "authenticating" | "connected") || (self.state == "disconnected" && (!self.error.is_empty() || !self.wanted.is_empty())))
    }
}

struct App {
    hab: u32,
    dlg: u32,
    st: Status,
    nets: Vec<Net>,
    hidden: usize,
    scanning: bool,
    /// What the last action did (shown under the status), and the state
    /// it was said in (cleared once the state moves on).
    note: String,
    note_state: String,
    /// The network a Connect asked for (until it is joined or fails).
    joining: Option<Vec<u8>>,
    /// The passphrase the dialog took.
    pass: Option<Vec<u8>>,
    pass_ssid: Vec<u8>,
    /// `/DEMO`: made-up data, no kernel calls.
    demo: bool,
    /// WIFI.CFG's `POWERSAVE=` (missing: off), as the check box shows it.
    powersave: bool,
    /// The text beside the check box, as last set.
    power_text: String,
}

static mut APP: App = App { hab: 0, dlg: 0, st: Status { present: false, name: String::new(), status: String::new(), state: String::new(), ssid: String::new(), signal: String::new(), channel: String::new(), security: String::new(), standard: String::new(), wanted: String::new(), error: String::new(), mac: None, power_save: String::new(), power_state: String::new() }, nets: Vec::new(), hidden: 0, scanning: false, note: String::new(), note_state: String::new(), joining: None, pass: None, pass_ssid: Vec::new(), demo: false, powersave: false, power_text: String::new() };

fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

// ---- The kernel's interface ----

/// Call `f(buf, cb, pcb)` with a buffer large enough for its text;
/// `again` is the call repeated after an overflow (a scan is not redone).
fn text(mut f: impl FnMut(*mut u8, u32, *mut u32) -> u32, mut again: impl FnMut(*mut u8, u32, *mut u32) -> u32) -> Result<String, u32> {
    let mut cb = 16384u32;
    let mut first = true;
    loop {
        let mut buf = alloc::vec![0u8; cb as usize];
        let mut need = 0u32;
        let rc = if first { f(buf.as_mut_ptr(), cb, &mut need) } else { again(buf.as_mut_ptr(), cb, &mut need) };
        first = false;
        match rc {
            0 => return Ok(until_nul(&buf).iter().map(|&b| b as char).collect()),
            ERROR_BUFFER_OVERFLOW if need > cb => cb = need,
            e => return Err(e),
        }
    }
}

/// `/DEMO`'s adapter and networks.
const DEMO_QUERY: &str = "adapter=0\nname=88W8897 02:00.0 (demo)\nstatus=up\nmac=02:00:00:00:00:01\nstate=connected\nssid=Home\nbssid=02:00:00:00:00:02\nchannel=36\nsignal=-52 dBm\nsecurity=WPA2\nstandard=802.11ac\nprofiles=1\n";
const DEMO_SCAN: &str = "Home\t-52\t36\tWPA2\t02:00:00:00:00:02\t1\nHome\t-70\t6\tWPA2\t02:00:00:00:00:03\t1\nCafe Guest\t-61\t11\topen\t02:00:00:00:00:04\t0\nNeighbour 5G\t-72\t149\tWPA2/WPA3\t02:00:00:00:00:05\t0\nOffice\t-80\t1\t802.1X\t02:00:00:00:00:06\t0\nOld Router\t-88\t6\tWEP\t02:00:00:00:00:07\t0\n\0\0\0\t-75\t6\tWPA2\t02:00:00:00:00:08\t0\n";

fn query_text() -> String {
    let a = app();
    if a.demo {
        return String::from(DEMO_QUERY) + if a.powersave { "power_save=on\npower_state=asleep\n" } else { "power_save=off\n" };
    }
    text(|b, cb, pcb| unsafe { Rs2WlanQuery(b, cb, pcb) }, |b, cb, pcb| unsafe { Rs2WlanQuery(b, cb, pcb) }).unwrap_or_default()
}

/// Scan (`fresh`), or the last scan's results.
fn scan_text(fresh: bool) -> Result<String, u32> {
    if app().demo {
        return Ok(String::from(DEMO_SCAN));
    }
    let flags = if fresh { 0 } else { 1 };
    text(|b, cb, pcb| unsafe { Rs2WlanScan(0, flags, b, cb, pcb) }, |b, cb, pcb| unsafe { Rs2WlanScan(0, 1, b, cb, pcb) })
}

fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut m = [0u8; 6];
    let mut it = s.split(':');
    for b in m.iter_mut() {
        *b = u8::from_str_radix(it.next()?, 16).ok()?;
    }
    Some(m)
}

/// The first adapter's lines of the query.
fn parse_status(t: &str) -> Status {
    let mut s = Status::default();
    let mut adapters = 0;
    for l in t.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        if k == "adapter" {
            adapters += 1;
            s.present = true;
            continue;
        }
        if adapters != 1 {
            continue;
        }
        let v = String::from(v);
        match k {
            "name" => s.name = v,
            "status" => s.status = v,
            "state" => s.state = v,
            "ssid" => s.ssid = v,
            "signal" => s.signal = v,
            "channel" => s.channel = v,
            "security" => s.security = v,
            "standard" => s.standard = v,
            "wanted" => s.wanted = v,
            "error" => s.error = v,
            "mac" => s.mac = parse_mac(&v),
            "power_save" => s.power_save = v,
            "power_state" => s.power_state = v,
            _ => {}
        }
    }
    s
}

/// Scan lines (SSID, RSSI, channel, security, BSSID, known): one entry
/// per SSID (its strongest access point), strongest first; and how many
/// hidden ones were left out.
fn parse_networks(t: &str) -> (Vec<Net>, usize) {
    let mut v: Vec<Net> = Vec::new();
    let mut hidden: Vec<String> = Vec::new();
    for l in t.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let ssid: Vec<u8> = f[0].chars().map(|c| c as u32 as u8).collect();
        if ssid.iter().all(|&b| b == 0) {
            if !hidden.iter().any(|b| b == f[4]) {
                hidden.push(String::from(f[4]));
            }
            continue;
        }
        let n = Net { ssid, rssi: f[1].trim().parse().unwrap_or(-100), channel: f[2].trim().parse().unwrap_or(0), security: String::from(f[3]), known: f[5].trim() == "1" };
        match v.iter_mut().find(|x| x.ssid == n.ssid) {
            Some(x) => {
                x.known |= n.known;
                if n.rssi > x.rssi {
                    *x = Net { known: x.known, ..n };
                }
            }
            None => v.push(n),
        }
    }
    v.sort_by(|a, b| b.rssi.cmp(&a.rssi));
    (v, hidden.len())
}

/// Bars of signal (0-4) for an RSSI in dBm.
fn bars(rssi: i32) -> u32 {
    match rssi {
        r if r >= -55 => 4,
        r if r >= -67 => 3,
        r if r >= -75 => 2,
        r if r >= -85 => 1,
        _ => 0,
    }
}

fn quality(rssi: i32) -> &'static str {
    ["very weak", "weak", "fair", "good", "excellent"][bars(rssi) as usize]
}

/// The keychain: whether it is locked, and whether it holds a passphrase
/// for `ssid` (service "wifi").
fn keychain(ssid: &[u8]) -> (bool, bool) {
    let t = text(|b, cb, pcb| unsafe { Rs2KeyQuery(b, cb, pcb) }, |b, cb, pcb| unsafe { Rs2KeyQuery(b, cb, pcb) }).unwrap_or_default();
    let locked = t.lines().any(|l| l == "state=locked");
    let name: String = ssid.iter().map(|&b| if b < 0x20 { '?' } else { b as char }).collect();
    let has = t.lines().filter_map(|l| l.strip_prefix("entry=")).any(|e| {
        let mut f = e.split('\t');
        f.next() == Some("wifi") && f.next() == Some(name.as_str())
    });
    (locked, has)
}

// ---- WIFI.CFG ----

fn config_path() -> Vec<u8> {
    [&[boot_drive()][..], b":\\MPTN\\ETC\\WIFI.CFG"].concat()
}

/// The whole file (settings and networks): a writer changes only its part
/// and writes all of it back, so the other settings are kept.
fn read_config() -> wlan::config::Config {
    match read_file(&config_path()) {
        Some(d) => wlan::config::parse(&d.iter().map(|&b| b as char).collect::<String>()),
        None => wlan::config::Config::default(),
    }
}

fn write_config(c: &wlan::config::Config) -> Result<(), u32> {
    let t: Vec<u8> = wlan::config::write_config(c).chars().map(|c| c as u32 as u8).collect();
    write_file(&config_path(), &t)
}

fn ssid_text(s: &[u8]) -> String {
    s.iter().map(|&b| b as char).collect()
}

/// As shown in the window (Latin-1 bytes, as the kernel gives them).
fn bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

// ---- The window ----

fn main(s: &os2::Startup) -> u32 {
    let a = app();
    a.demo = pmkit::arg(s).eq_ignore_ascii_case(b"/DEMO");
    a.powersave = if a.demo { true } else { read_config().powersave.unwrap_or(false) };
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        os2::println!("WIFIPM: Presentation Manager is not available (WIFI.EXE works without it)");
        return 1;
    }
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, 0, main_proc as *const () as u32, main_template().as_ptr(), 0) };
    if dlg == 0 {
        trace("wifipm", b"no window");
        return 1;
    }
    a.dlg = dlg;
    set_check(dlg, ID_POWERSAVE, a.powersave);
    let icon = unsafe { WinLoadPointer(HWND_DESKTOP, 0, 1) };
    if icon != 0 {
        send(dlg, WM_SETICON, icon, 0);
    }
    refresh();
    let st = &app().st;
    trace(
        "wifipm",
        alloc::format!("started: {}", if !st.present { String::from("no WiFi adapter") } else { alloc::format!("adapter {} ({}), state {}", st.name, st.status, st.state) }).as_bytes(),
    );
    if app().st.up() {
        // The last scan's results at once, a new scan behind them.
        if let Ok(t) = scan_text(false) {
            set_networks(&t);
        }
        start_scan();
    }
    center(dlg);
    unsafe {
        WinShowWindow(dlg, 1);
        WinSetFocus(HWND_DESKTOP, pmkit::item(dlg, ID_LIST));
        WinStartTimer(a.hab, dlg, TID_REFRESH, REFRESH_MS);
    }
    message_loop(a.hab);
    unsafe {
        WinStopTimer(a.hab, dlg, TID_REFRESH);
        WinDestroyWindow(dlg);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

fn main_template() -> Vec<u8> {
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let mut frame = tpl::item(WC_FRAME, TITLE, WS_CLIPSIBLINGS | WS_SAVEBITS | FS_DLGBORDER | FS_NOBYTEALIGN, (20, 20, 270, 252), DLG_MAIN);
    frame.ctl = fcf.to_le_bytes().to_vec();
    let l = DT_LEFT | DT_VCENTER;
    template(
        frame,
        alloc::vec![
            group_box(b"Connection", (6, 150, 258, 96)),
            label(b"Adapter:", (14, 222, 52, 9), l),
            text_id(b"", (68, 222, 190, 9), ID_ADAPTER, l),
            label(b"State:", (14, 211, 52, 9), l),
            text_id(b"", (68, 211, 190, 9), ID_STATE, l),
            label(b"Signal:", (14, 200, 52, 9), l),
            text_id(b"", (68, 200, 190, 9), ID_SIGNAL, l),
            label(b"IP address:", (14, 189, 52, 9), l),
            text_id(b"", (68, 189, 190, 9), ID_ADDRESS, l),
            check_box(b"~Power saving", (12, 176, 54, 10), ID_POWERSAVE).group(),
            text_id(b"", (68, 177, 190, 9), ID_POWER, l),
            text_id(b"", (14, 154, 244, 20), ID_NOTE, DT_LEFT | DT_TOP | DT_WORDBREAK),
            text_id(b"Networks:", (8, 138, 254, 9), ID_NETLABEL, l),
            list_box((6, 26, 258, 110), ID_LIST, LS_OWNERDRAW | LS_NOADJUSTPOS).group(),
            button(b"~Connect", (6, 6, 48), ID_CONNECT, BS_DEFAULT).group(),
            button(b"~Disconnect", (58, 6, 48), ID_DISCONNECT, 0),
            button(b"~Forget", (110, 6, 48), ID_FORGET, 0),
            button(b"~Scan", (162, 6, 48), ID_SCAN, 0),
            button(b"Close", (216, 6, 48), ID_CLOSE, 0),
        ],
    )
}

/// Read the status again and show it (and the buttons that apply).
fn refresh() {
    let a = app();
    a.st = parse_status(&query_text());
    let st = a.st.clone();
    if st.state != a.note_state && !a.note_state.is_empty() {
        a.note.clear();
        a.note_state.clear();
    }
    let dlg = a.dlg;
    let (adapter, state, signal) = if !st.present {
        (String::from("none"), String::from("No WiFi adapter found"), String::new())
    } else if st.status == "found" || st.status == "starting" {
        (st.name.clone(), String::from("Starting the adapter..."), String::new())
    } else if let Some(e) = st.status.strip_prefix("failed: ") {
        (st.name.clone(), alloc::format!("The adapter failed: {e}"), String::new())
    } else {
        let state = match st.state.as_str() {
            "off" => String::from("Disconnected (Connect joins a network)"),
            "connecting" => alloc::format!("Joining {}...", st.ssid),
            "authenticating" => alloc::format!("Checking the passphrase with {}...", st.ssid),
            "connected" => alloc::format!("Connected to {}", st.ssid),
            _ if !st.error.is_empty() => alloc::format!("Failed: {}", st.error),
            _ if !st.wanted.is_empty() => alloc::format!("Looking for {}...", st.wanted),
            _ => String::from("Disconnected"),
        };
        let signal = match st.signal.trim_end_matches(" dBm").trim().parse::<i32>() {
            Ok(r) if !st.ssid.is_empty() && st.state != "disconnected" => {
                let std = if st.standard.is_empty() { String::new() } else { alloc::format!(", {}", st.standard) };
                alloc::format!("{} ({}), channel {}, {}{std}", st.signal, quality(r), st.channel, st.security)
            }
            _ => String::new(),
        };
        (st.name.clone(), state, signal)
    };
    set_item_text(dlg, ID_ADAPTER, adapter.as_bytes());
    set_item_text(dlg, ID_STATE, &bytes(&state));
    set_item_text(dlg, ID_SIGNAL, signal.as_bytes());
    set_item_text(dlg, ID_ADDRESS, address(&st).as_bytes());
    let power = power_text(&st, a.powersave);
    if power != a.power_text {
        set_item_text(dlg, ID_POWER, power.as_bytes());
        a.power_text = power;
    }
    // A Connect followed to its end.
    if let Some(j) = a.joining.clone() {
        let name = ssid_text(&j);
        if st.state == "connected" && bytes(&st.ssid) == j {
            a.joining = None;
            say(&alloc::format!("Connected to {name}. The address comes from DHCP."));
        } else if st.state == "disconnected" && !st.error.is_empty() {
            a.joining = None;
            say(&alloc::format!("Could not join {name}: {}. rs2 keeps trying; Disconnect stops it, Forget and Connect again to give another passphrase.", st.error));
        }
    }
    if !st.present {
        say("This computer has no WiFi adapter that rs2 can drive (rs2 drives the Marvell 88W8897 of the Surface Pro 4).");
    }
    let note = a.note.clone();
    set_item_text(dlg, ID_NOTE, &bytes(&note));
    // Connected markers in the list follow the state.
    pmkit::invalidate(pmkit::item(dlg, ID_LIST));
    update_buttons();
}

/// The interface of the adapter's MAC address: its IPv4 address.
fn address(st: &Status) -> String {
    let Some(mac) = st.mac else { return String::new() };
    if st.state != "connected" {
        return String::new();
    }
    match os2::net::interfaces().into_iter().find(|i| i.mac == mac) {
        Some(i) if i.addr != [0; 4] => {
            let how = if i.flags & os2::net::IF_DHCP != 0 { " (DHCP)" } else { "" };
            alloc::format!("{}{how}", os2::net::ip(i.addr))
        }
        Some(i) if i.flags & os2::net::IF_DHCP != 0 => String::from("Waiting for DHCP..."),
        _ => String::from("none"),
    }
}

/// Beside the check box: what the adapter does now, when it says.
fn power_text(st: &Status, setting: bool) -> String {
    if !st.up() {
        return String::new();
    }
    match (st.power_save.as_str(), setting) {
        ("on", _) => {
            let now = if st.power_state.is_empty() { String::new() } else { alloc::format!(", {} now", st.power_state) };
            alloc::format!("{}{now}", if setting { "on" } else { "still on" })
        }
        ("off", true) => String::from("not in effect yet"),
        // "on, turning off", "off: the firmware missed a wake request"
        (s, _) if s.starts_with("on,") || s.starts_with("off:") => String::from(s),
        _ => String::new(),
    }
}

/// The check box was clicked: POWERSAVE= in WIFI.CFG, and the kernel
/// reads the file again.
fn set_powersave() {
    let a = app();
    let on = checked(a.dlg, ID_POWERSAVE);
    if on == a.powersave {
        return;
    }
    let what = if on { "on" } else { "off" };
    if a.demo {
        a.powersave = on;
        refresh();
        say(&alloc::format!("(Demo) Would write POWERSAVE={} in WIFI.CFG and have rs2 read it again.", what.to_ascii_uppercase()));
        return;
    }
    let mut c = read_config();
    c.powersave = Some(on);
    if let Err(rc) = write_config(&c) {
        set_check(a.dlg, ID_POWERSAVE, a.powersave);
        say(&alloc::format!("WIFI.CFG could not be written (error {rc}); power saving is unchanged."));
        return;
    }
    a.powersave = on;
    unsafe { Rs2WlanReload() };
    refresh();
    say(&if on {
        String::from("Power saving on: the adapter sleeps between beacons (saved in WIFI.CFG).")
    } else {
        String::from("Power saving off: the adapter stays awake (saved in WIFI.CFG).")
    });
}

/// Show a note under the status (until the state changes).
fn say(s: &str) {
    let a = app();
    a.note = String::from(s);
    a.note_state = a.st.state.clone();
    if a.note_state.is_empty() {
        // Kept while there is no state to change.
        a.note_state = String::from("-");
    }
    set_item_text(a.dlg, ID_NOTE, &bytes(s));
}

fn selected() -> Option<Net> {
    let i = dlg_msg(app().dlg, ID_LIST, LM_QUERYSELECTION, LIT_FIRST, 0) as i32;
    app().nets.get(usize::try_from(i).ok()?).cloned()
}

/// The network the adapter is on (or joining).
fn current(n: &Net) -> bool {
    let st = &app().st;
    st.up() && st.state != "disconnected" && st.state != "off" && bytes(&st.ssid) == n.ssid
}

fn update_buttons() {
    let a = app();
    let (dlg, up) = (a.dlg, a.st.up());
    let sel = selected();
    enable(dlg, ID_CONNECT, up && sel.as_ref().is_some_and(|n| !current(n)));
    enable(dlg, ID_DISCONNECT, a.st.busy());
    enable(dlg, ID_FORGET, up && sel.as_ref().is_some_and(|n| n.known));
    enable(dlg, ID_SCAN, up && !a.scanning);
    set_item_text(dlg, ID_SCAN, if a.scanning { b"Scanning" } else { b"~Scan" });
    enable(dlg, ID_LIST, up);
    enable(dlg, ID_POWERSAVE, a.st.present);
}

/// Fill the list from scan lines (keeping the selected network).
fn set_networks(t: &str) {
    let a = app();
    let (nets, hidden) = parse_networks(t);
    let keep = selected().map(|n| n.ssid);
    let changed = nets != a.nets;
    a.nets = nets;
    a.hidden = hidden;
    let label = match (a.nets.len(), hidden) {
        (0, 0) => String::from("Networks: none found"),
        (n, 0) => alloc::format!("Networks ({n}):"),
        (n, h) => alloc::format!("Networks ({n}; {h} hidden not shown):"),
    };
    set_item_text(a.dlg, ID_NETLABEL, label.as_bytes());
    if changed {
        dlg_msg(a.dlg, ID_LIST, LM_DELETEALL, 0, 0);
        for n in &a.nets {
            // The text is for keyboard search; the line is drawn here.
            lb_add(a.dlg, ID_LIST, &n.ssid);
        }
        if let Some(i) = keep.and_then(|k| a.nets.iter().position(|n| n.ssid == k)) {
            dlg_msg(a.dlg, ID_LIST, LM_SELECTITEM, i as u32, 1);
        }
    }
    update_buttons();
}

fn start_scan() {
    let a = app();
    if a.scanning || !a.st.up() {
        return;
    }
    a.scanning = true;
    update_buttons();
    let mut tid = 0;
    if unsafe { os2::DosCreateThread(&mut tid, scan_thread, a.dlg, 0, 64 * 1024) } != 0 {
        a.scanning = false;
        update_buttons();
    }
}

unsafe extern "C" fn scan_thread(dlg: u32) {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let r = Box::new(scan_text(true));
    post(dlg, WM_SCANNED, Box::into_raw(r) as u32, 0);
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

fn scanned(r: Result<String, u32>) {
    let a = app();
    a.scanning = false;
    match r {
        Ok(t) => set_networks(&t),
        Err(ERROR_NOT_READY) => say("The adapter is busy (joining a network); Scan again in a moment."),
        Err(ERROR_DEV_NOT_EXIST) => say("No WiFi adapter."),
        Err(rc) => say(&alloc::format!("The scan failed (error {rc}); WIFI STATUS shows the adapter's log.")),
    }
    update_buttons();
}

/// Connect to the selected network (asking for a passphrase if needed).
fn connect() {
    let a = app();
    let Some(n) = selected() else { return };
    let name = ssid_text(&n.ssid);
    let pass: Option<Vec<u8>> = match n.security.as_str() {
        "open" => None,
        "WPA2" | "WPA2/WPA3" => {
            let (locked, has) = keychain(&n.ssid);
            if n.known && (has || locked) {
                None
            } else {
                match ask_passphrase(&n.ssid) {
                    Some(p) => Some(p),
                    None => return,
                }
            }
        }
        other => {
            message(a.dlg, alloc::format!("rs2 cannot join {name}: {other} networks are not supported yet (open and WPA2 networks are).").as_bytes(), TITLE, MB_OK | MB_INFORMATION);
            return;
        }
    };
    if a.demo {
        say(&alloc::format!("(Demo) Would join {name}{}.", if pass.is_some() { " with the passphrase given" } else { "" }));
        return;
    }
    // Saved first in WIFI.CFG (so it is joined at the next start), as
    // WIFI CONNECT does; the passphrase goes to the keychain, not there.
    let mut c = read_config();
    let ps = &mut c.profiles;
    let mut p = ps.iter().position(|p| p.ssid == n.ssid).map(|i| ps.remove(i)).unwrap_or_else(|| wlan::config::Profile::new(&n.ssid, None));
    if pass.is_some() {
        p.passphrase = None;
    }
    p.autoconnect = true;
    ps.insert(0, p);
    let saved = write_config(&c);
    if saved.is_ok() {
        unsafe { Rs2WlanReload() };
    }
    let s = z(&n.ssid);
    let mut k = pass.as_ref().map(|p| z(p));
    let rc = unsafe { Rs2WlanConnect(0, s.as_ptr(), k.as_ref().map_or(core::ptr::null(), |k| k.as_ptr())) };
    if let Some(k) = k.as_mut() {
        k.iter_mut().for_each(|b| *b = 0);
    }
    if rc != 0 {
        say(&alloc::format!("Could not start joining {name} (error {rc}: {}).", if rc == ERROR_NOT_READY { "the adapter is not ready" } else { "see WIFI STATUS" }));
        return;
    }
    a.joining = Some(n.ssid.clone());
    refresh();
    let mut msg = alloc::format!("Joining {name}...");
    if let Err(rc) = saved {
        msg += &alloc::format!(" (It could not be saved in WIFI.CFG: error {rc}.)");
    }
    if pass.is_some() && keychain(&n.ssid).0 {
        msg += " The keychain is locked, so the passphrase is not saved (KEYCHAIN UNLOCK).";
    }
    say(&msg);
}

fn disconnect() {
    if app().demo {
        say("(Demo) Would disconnect.");
        return;
    }
    let rc = unsafe { Rs2WlanDisconnect(0) };
    app().joining = None;
    refresh();
    if rc != 0 {
        say(&alloc::format!("Could not disconnect (error {rc})."));
    } else {
        say("Disconnected. rs2 joins no network until Connect (or the next start).");
    }
}

/// Remove the selected network from WIFI.CFG and its passphrase from the
/// keychain (as WIFI FORGET); leave it if connected to it.
fn forget() {
    let a = app();
    let Some(n) = selected() else { return };
    let name = ssid_text(&n.ssid);
    let q = alloc::format!("Forget {name}? It is removed from WIFI.CFG with its passphrase, and rs2 no longer joins it by itself.");
    if message(a.dlg, &bytes(&q), TITLE, MB_YESNO | MB_ICONQUESTION) != MBID_YES {
        return;
    }
    if a.demo {
        say(&alloc::format!("(Demo) Would forget {name}."));
        return;
    }
    let mut c = read_config();
    let before = c.profiles.len();
    c.profiles.retain(|p| p.ssid != n.ssid);
    let account = z(&n.ssid);
    unsafe { Rs2KeyDelete(b"wifi\0".as_ptr(), account.as_ptr()) };
    let mut msg = alloc::format!("{name} forgotten.");
    if c.profiles.len() != before {
        if let Err(rc) = write_config(&c) {
            msg = alloc::format!("WIFI.CFG could not be written (error {rc}).");
        }
    }
    unsafe { Rs2WlanReload() };
    let wanted = bytes(&a.st.wanted) == n.ssid;
    if current(&n) || wanted {
        unsafe { Rs2WlanDisconnect(0) };
        msg += " Disconnected from it.";
    }
    if let Ok(t) = scan_text(false) {
        set_networks(&t);
    }
    a.joining = None;
    refresh();
    say(&msg);
}

extern "C" fn main_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            app().dlg = h;
            0
        }
        WM_MEASUREITEM => ROW,
        WM_DRAWITEM if lo(mp1) == ID_LIST && mp2 != 0 => draw_item(mp2 as *mut u32),
        WM_CONTROL if lo(mp1) == ID_POWERSAVE => {
            // The check box toggles itself; a double click may come as
            // BN_DBLCLICKED: either way its state is compared with the file's.
            if matches!(hi(mp1), BN_CLICKED | BN_DBLCLICKED) {
                set_powersave();
            }
            0
        }
        WM_CONTROL if lo(mp1) == ID_LIST => {
            match hi(mp1) {
                LN_SELECT => update_buttons(),
                LN_ENTER => {
                    if selected().is_some_and(|n| !current(&n)) && app().st.up() {
                        connect();
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_CONNECT => connect(),
                ID_DISCONNECT => disconnect(),
                ID_FORGET => forget(),
                ID_SCAN => start_scan(),
                ID_CLOSE => {
                    post(h, WM_QUIT, 0, 0);
                }
                // Esc (DID_CANCEL) and Enter on no button: nothing.
                _ => {}
            }
            0
        }
        WM_TIMER if lo(mp1) == TID_REFRESH => {
            refresh();
            if app().st.up() && !app().scanning {
                if let Ok(t) = scan_text(false) {
                    set_networks(&t);
                }
            }
            0
        }
        WM_SCANNED => {
            let r = unsafe { Box::from_raw(mp1 as *mut Result<String, u32>) };
            scanned(*r);
            0
        }
        WM_CLOSE => {
            post(h, WM_QUIT, 0, 0);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// WM_DRAWITEM: an OWNERITEM (hwnd, hps, fsState, fsAttribute,
/// fsStateOld, fsAttributeOld, rclItem, idItem, hItem): the signal bars,
/// the name, the security and whether it is saved or in use.
fn draw_item(oi: *mut u32) -> u32 {
    let hps = unsafe { *oi.add(1) };
    let on = unsafe { *oi.add(2) } != 0;
    let (l, b, r, t) = unsafe { (*oi.add(6) as i32, *oi.add(7) as i32, *oi.add(8) as i32, *oi.add(9) as i32) };
    let Some(n) = app().nets.get(unsafe { *oi.add(10) } as usize).cloned() else { return 0 };
    let (bg, fg) = if on { (SYSCLR_HILITEBACKGROUND, SYSCLR_HILITEFOREGROUND) } else { (SYSCLR_WINDOW, SYSCLR_WINDOWTEXT) };
    let w = r - l;
    let rect = RectL { l, b, r, t };
    unsafe { WinFillRect(hps, &rect, bg) };
    // Four bars, rising; those of the signal filled.
    let lit = bars(n.rssi) as i32;
    for i in 0..4 {
        let x = l + 6 + i * 5;
        let bar = RectL { l: x, b: b + 4, r: x + 3, t: b + 7 + i * 3 };
        unsafe { WinFillRect(hps, &bar, if i < lit { fg } else { SYSCLR_BUTTONDARK }) };
    }
    let col = |x0: i32, x1: i32, s: &[u8]| {
        let mut rc = RectL { l: l + x0, b, r: l + x1, t };
        let s = z(s);
        unsafe { WinDrawText(hps, -1, s.as_ptr(), &mut rc, fg, bg, DT_LEFT | DT_VCENTER) };
    };
    let (c1, c2) = (w * 58 / 100, w * 80 / 100);
    col(32, c1 - 4, &n.ssid);
    let sec: &[u8] = if n.security == "open" { b"Open" } else { n.security.as_bytes() };
    col(c1, c2 - 4, sec);
    let what: &[u8] = if current(&n) {
        if app().st.state == "connected" { b"Connected" } else { b"Joining" }
    } else if n.known {
        b"Saved"
    } else {
        b""
    };
    col(c2, w - 2, what);
    // Drawn: nothing for the list box to invert.
    unsafe {
        *oi.add(2) = 0;
        *oi.add(4) = 0;
    }
    1
}

// ---- The passphrase ----

/// Ask for `ssid`'s passphrase (8 to 63 characters).
fn ask_passphrase(ssid: &[u8]) -> Option<Vec<u8>> {
    let a = app();
    a.pass = None;
    a.pass_ssid = ssid.to_vec();
    let prompt = [&b"The passphrase of "[..], ssid, b":"].concat();
    let tpl = template(
        dialog_frame(b"Connect", DLG_PASS, 200, 64),
        alloc::vec![
            text_id(&prompt, (8, 44, 184, 9), ID_PASSLABEL, DT_LEFT | DT_VCENTER),
            entry((10, 30, 180, 8), ID_PASS, ES_UNREADABLE).group(),
            button(b"~Connect", (8, 6, 50), DID_OK, BS_DEFAULT).group(),
            button(b"Cancel", (64, 6, 50), DID_CANCEL, 0),
        ],
    );
    if run_dialog(a.dlg, pass_proc, &tpl, 0) != DID_OK {
        return None;
    }
    a.pass.take()
}

extern "C" fn pass_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            dlg_msg(h, ID_PASS, EM_SETTEXTLIMIT, 63, 0);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_PASS)) };
            // The focus is set here.
            1
        }
        WM_COMMAND if lo(mp1) == DID_OK => {
            let mut p = item_text(h, ID_PASS);
            if !(8..=63).contains(&p.len()) {
                p.iter_mut().for_each(|b| *b = 0);
                message(h, b"A WPA2 passphrase is 8 to 63 characters.", b"Connect", MB_OK | MB_WARNING);
                unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_PASS)) };
                return 0;
            }
            app().pass = Some(p);
            set_item_text(h, ID_PASS, b"");
            unsafe { WinDismissDlg(h, DID_OK) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}
