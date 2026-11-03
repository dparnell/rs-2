//! TCPCFG2: the TCP/IP Configuration notebook.
//!
//! Pages: Network (the first adapter, lan0: on or off, DHCP or a fixed
//! address, subnet mask and router), Host Names (the host name, the local
//! domain and up to three name servers) and Hosts (the HOSTS file). Save
//! writes the settings where rs2 reads them: `NETWORK=dhcp|off|address/
//! prefix,router,dns...` and `SET HOSTNAME=` in CONFIG.SYS (the kernel
//! and the programs take them at the next start), %ETC%\RESOLV2
//! (`domain`, `nameserver`: the resolver calls of the socket DLLs) and
//! %ETC%\HOSTS. The current state of the interfaces is shown too.
//! Save also applies the Network page to lan0 at once (on or off, DHCP
//! or the fixed address, mask and router, through IFCONFIG's and ROUTE's
//! kernel calls, with its name servers); the host name follows at the
//! next start.
//!
//! Without PM, or for scripts: `TCPCFG2 /QUERY` prints the settings,
//! `TCPCFG2 setting=value ...` changes them without the window
//! (NETWORK=, HOSTNAME=, DOMAIN=, NAMESERVERS=a[,b,c]).
#![no_std]
#![no_main]
extern crate alloc;

#[path = "../../../appinst/src/pm.rs"]
#[allow(dead_code, unused_imports)]
mod pm;

use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
use os2::net::*;
use pm::*;
os2::entry!(main);

const WC_BUTTON: &[u8] = b"#3\0";
const WC_STATIC: &[u8] = b"#5\0";
const WC_ENTRYFIELD: &[u8] = b"#6\0";
const WC_MLE: &[u8] = b"#10\0";
const WC_NOTEBOOK: &[u8] = b"#40\0";
const BS_AUTOCHECKBOX: u32 = 2;
const BS_AUTORADIOBUTTON: u32 = 4;
const BM_QUERYCHECK: u32 = 0x0124;
const BM_SETCHECK: u32 = 0x0125;
const ES_MARGIN: u32 = 0x0008;
const ES_AUTOSCROLL: u32 = 0x0004;
const EM_SETTEXTLIMIT: u32 = 0x0143;
const MLS_BORDER: u32 = 0x0002;
const MLS_VSCROLL: u32 = 0x0004;
const MLS_HSCROLL: u32 = 0x0008;
const MLM_SETTEXTLIMIT: u32 = 0x01b0;
const BKS_BACKPAGESBR: u32 = 0x0001;
const BKS_MAJORTABRIGHT: u32 = 0x0010;
const BKS_SPIRALBIND: u32 = 0x0400;
const BKS_ROUNDEDTABS: u32 = 0x0100;
const BKS_STATUSTEXTCENTER: u32 = 0x2000;
const BKS_TABTEXTCENTER: u32 = 0x8000;
const BKM_CALCPAGERECT: u32 = 0x0353;
const BKM_INSERTPAGE: u32 = 0x0355;
const BKM_SETDIMENSIONS: u32 = 0x035e;
const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
const BKM_SETTABTEXT: u32 = 0x0363;
const BKA_LAST: u32 = 0x0002;
const BKA_MAJOR: u32 = 0x0040;
const BKA_STATUSTEXTON: u32 = 0x0001;
const BKA_AUTOPAGESIZE: u32 = 0x0100;
const BKA_MAJORTAB: u32 = 0x0001;
const BN_CLICKED: u32 = 1;
const DT_TOP: u32 = 0x0000;

unsafe extern "C" {
    #[link_name = "PMWIN.842"]
    fn WinQueryWindowTextLength(h: u32) -> i32;
}

const ID_BOOK: u32 = 100;
const ID_SAVE: u32 = 101;
const ID_UNDO: u32 = 102;
const ID_CLOSE: u32 = 103;
const ID_MSG: u32 = 104;
const ID_ENABLE: u32 = 200;
const ID_DHCP: u32 = 201;
const ID_MANUAL: u32 = 202;
const ID_ADDR: u32 = 203;
const ID_MASK: u32 = 204;
const ID_ROUTER: u32 = 205;
const ID_NOW: u32 = 206;
const ID_HOST: u32 = 300;
const ID_DOMAIN: u32 = 301;
const ID_NS: u32 = 302; // ..304
const ID_HOSTS: u32 = 400;

static PAGE_CLASS: &[u8] = b"Rs2TcpCfgPage\0";
static CLASS: &[u8] = b"Rs2TcpCfg\0";

/// The settings.
#[derive(Clone, Default)]
struct Settings {
    enabled: bool,
    dhcp: bool,
    addr: String,
    mask: String,
    router: String,
    host: String,
    domain: String,
    ns: Vec<String>,
    hosts: String,
}

fn config_path() -> String {
    let mut d = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(5, 5, d.as_mut_ptr(), 4) };
    let letter = if (1..=26).contains(&d[0]) { (b'A' + d[0] as u8 - 1) as char } else { 'C' };
    alloc::format!("{letter}:\\CONFIG.SYS")
}

fn text(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0x1a).map(|&c| c as char).collect()
}

fn mask_of(prefix: u32) -> String {
    let m: u32 = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix.min(32)) };
    ip(m.to_be_bytes())
}

fn prefix_of(mask: &str) -> Option<u32> {
    let m = u32::from_be_bytes(parse_ip(mask)?);
    let p = m.leading_ones();
    (m.checked_shl(p).unwrap_or(0) == 0).then_some(p)
}

/// The NETWORK= value's key (upper case) of a CONFIG.SYS line.
fn config_key(line: &str) -> Option<(String, String)> {
    let t = line.trim_start();
    let (k, v) = t.split_once('=')?;
    let k = k.trim().to_ascii_uppercase();
    let k = if let Some(rest) = k.strip_prefix("SET ") { alloc::format!("SET {}", rest.trim()) } else { k };
    Some((k, String::from(v.trim())))
}

fn load() -> Settings {
    let mut s = Settings { enabled: true, dhcp: true, mask: String::from("255.255.255.0"), ..Default::default() };
    let cfg = text(&read_file(&config_path()).unwrap_or_default());
    for l in cfg.lines() {
        match config_key(l) {
            Some((k, v)) if k == "NETWORK" => apply_network(&mut s, &v),
            Some((k, v)) if k == "SET HOSTNAME" => s.host = v,
            _ => {}
        }
    }
    let res = text(&read_file(&etc_path("RESOLV2")).or_else(|| read_file(&etc_path("RESOLV"))).unwrap_or_default());
    let mut ns = Vec::new();
    for l in res.lines() {
        let mut w = l.split_whitespace();
        match (w.next(), w.next()) {
            (Some("domain"), Some(d)) => s.domain = String::from(d),
            (Some("nameserver"), Some(a)) => ns.push(String::from(a)),
            _ => {}
        }
    }
    if s.ns.is_empty() {
        s.ns = ns;
    }
    s.hosts = text(&read_file(&etc_path("HOSTS")).unwrap_or_default());
    s
}

fn apply_network(s: &mut Settings, v: &str) {
    let v = v.trim();
    if v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("none") {
        s.enabled = false;
        return;
    }
    s.enabled = true;
    if v.eq_ignore_ascii_case("dhcp") || v.is_empty() {
        s.dhcp = true;
        return;
    }
    s.dhcp = false;
    let mut parts = v.split(',').map(str::trim);
    let first = parts.next().unwrap_or("");
    let (a, p) = first.split_once('/').unwrap_or((first, "24"));
    s.addr = String::from(a);
    s.mask = mask_of(p.parse().unwrap_or(24));
    s.router = String::from(parts.next().unwrap_or(""));
    s.ns = parts.filter(|x| !x.is_empty()).map(String::from).collect();
}

/// The NETWORK= value for the settings, or why they are wrong.
fn network_value(s: &Settings) -> Result<String, String> {
    if !s.enabled {
        return Ok(String::from("off"));
    }
    if s.dhcp {
        return Ok(String::from("dhcp"));
    }
    if parse_ip(s.addr.trim()).is_none() {
        return Err(alloc::format!("The IP address \"{}\" is not valid.", s.addr));
    }
    let Some(p) = prefix_of(s.mask.trim()) else {
        return Err(alloc::format!("The subnet mask \"{}\" is not valid.", s.mask));
    };
    let mut v = alloc::format!("{}/{p}", s.addr.trim());
    let router = s.router.trim();
    if !router.is_empty() && parse_ip(router).is_none() {
        return Err(alloc::format!("The router address \"{router}\" is not valid."));
    }
    let ns: Vec<&str> = s.ns.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).collect();
    if !router.is_empty() || !ns.is_empty() {
        v.push(',');
        v.push_str(router);
    }
    for n in ns {
        v.push(',');
        v.push_str(n);
    }
    Ok(v)
}

fn write_all(path: &str, data: &[u8]) -> bool {
    match create_file(path, false) {
        Ok(hf) => {
            let ok = write_file(hf, data).is_ok();
            unsafe { os2::DosClose(hf) };
            ok
        }
        Err(_) => false,
    }
}

/// Write the settings; Err says what is wrong.
fn save(s: &Settings) -> Result<(), String> {
    let net = network_value(s)?;
    for n in s.ns.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        if parse_ip(n).is_none() {
            return Err(alloc::format!("The name server address \"{n}\" is not valid."));
        }
    }
    let host = s.host.trim();
    if host.contains(' ') {
        return Err(String::from("The host name may not contain blanks."));
    }
    let path = config_path();
    let old = text(&read_file(&path).unwrap_or_default());
    let mut out = String::new();
    for l in old.lines() {
        match config_key(l) {
            Some((k, _)) if k == "NETWORK" || k == "SET HOSTNAME" => continue,
            _ => {}
        }
        out.push_str(l.trim_end_matches('\r'));
        out.push_str("\r\n");
    }
    out.push_str(&alloc::format!("NETWORK={net}\r\n"));
    if !host.is_empty() {
        out.push_str(&alloc::format!("SET HOSTNAME={host}\r\n"));
    }
    if !write_all(&path, &os2::text_bytes(&out)) {
        return Err(alloc::format!("{path} could not be written."));
    }
    let mut res = String::new();
    if !s.domain.trim().is_empty() {
        res.push_str(&alloc::format!("domain {}\r\n", s.domain.trim()));
    }
    for n in s.ns.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        res.push_str(&alloc::format!("nameserver {n}\r\n"));
    }
    let etc = etc_path("");
    let etc = etc.trim_end_matches('\\');
    let c = os2::path::cstr(etc);
    unsafe { os2::DosCreateDir(c.as_ptr(), core::ptr::null_mut()) };
    if !write_all(&etc_path("RESOLV2"), &os2::text_bytes(&res)) {
        return Err(String::from("RESOLV2 could not be written."));
    }
    let mut hosts = s.hosts.replace("\r\n", "\n").replace('\n', "\r\n");
    if !hosts.is_empty() && !hosts.ends_with("\r\n") {
        hosts.push_str("\r\n");
    }
    if !write_all(&etc_path("HOSTS"), &os2::text_bytes(&hosts)) {
        return Err(String::from("HOSTS could not be written."));
    }
    Ok(())
}

/// Apply the Network page to lan0 now. Ok says what was done.
fn apply_now(s: &Settings) -> Result<String, String> {
    let Some(i) = iface_index("lan0") else { return Ok(String::from("No network adapter: nothing to change now.")) };
    let info = interfaces().get(i as usize).copied().unwrap_or_default();
    let fail = |what: &str, e: u32| alloc::format!("Saved, but {what} could not be changed now ({}).", strerror(e));
    if !s.enabled {
        if info.flags & IF_UP != 0 {
            if_set(i, IFOP_DOWN, [0; 4], [0; 4]).map_err(|e| fail("lan0", e))?;
        }
        return Ok(String::from("lan0 is off now."));
    }
    if info.flags & IF_UP == 0 {
        if_set(i, IFOP_UP, [0; 4], [0; 4]).map_err(|e| fail("lan0", e))?;
    }
    if s.dhcp {
        if info.flags & IF_DHCP == 0 {
            if_set(i, IFOP_DHCP_RENEW, [0; 4], [0; 4]).map_err(|e| fail("lan0", e))?;
            return Ok(String::from("lan0 asks DHCP for an address now."));
        }
        return Ok(String::from("lan0 keeps its DHCP lease."));
    }
    let (Some(a), Some(m)) = (parse_ip(s.addr.trim()), parse_ip(s.mask.trim())) else { return Ok(String::new()) };
    if info.addr != a || info.mask != m || info.flags & IF_DHCP != 0 {
        if_set(i, IFOP_ADDRESS, a, m).map_err(|e| fail("the address", e))?;
    }
    if let Some(r) = parse_ip(s.router.trim()) {
        let has = routes().iter().any(|x| x.dest == [0; 4] && x.mask == [0; 4] && x.gateway == r);
        if !has {
            let _ = route_delete([0; 4], [0; 4], [0; 4]);
            route_add([0; 4], [0; 4], r, 1).map_err(|e| fail("the router", e))?;
        }
    }
    let ns: Vec<[u8; 4]> = s.ns.iter().filter_map(|n| parse_ip(n.trim())).collect();
    let _ = if_set(i, IFOP_DNS, ns.first().copied().unwrap_or([0; 4]), ns.get(1).copied().unwrap_or([0; 4]));
    Ok(alloc::format!("lan0 uses {} now.", ip(a)))
}

/// The interfaces as they are now.
fn current() -> String {
    let mut t = String::new();
    for (name, i) in os2_interfaces() {
        if i.flags & IF_LOOPBACK != 0 {
            continue;
        }
        let how = if i.flags & IF_DHCP != 0 { "DHCP" } else { "fixed" };
        let state = if i.flags & IF_UP == 0 {
            String::from("off")
        } else if i.addr == [0; 4] {
            alloc::format!("waiting for {how}")
        } else {
            let gw = if i.gateway != [0; 4] { alloc::format!(", router {}", ip(i.gateway)) } else { String::new() };
            alloc::format!("{} mask {}{gw} ({how})", ip(i.addr), ip(i.mask))
        };
        t.push_str(&alloc::format!("{name}: {state}\n"));
    }
    if t.is_empty() {
        t.push_str("No network adapter was found.");
    }
    t
}

fn query(s: &Settings) {
    outln(&alloc::format!("NETWORK={}", network_value(s).unwrap_or_else(|e| e)));
    outln(&alloc::format!("HOSTNAME={}", s.host));
    outln(&alloc::format!("DOMAIN={}", s.domain));
    outln(&alloc::format!("NAMESERVERS={}", s.ns.join(",")));
    for l in current().lines() {
        outln(&alloc::format!("now: {l}"));
    }
}

/// `setting=value ...` from the command line.
fn command_line(a: &[String]) -> u32 {
    let mut s = load();
    if a.iter().any(|x| x.eq_ignore_ascii_case("/QUERY") || x.eq_ignore_ascii_case("-q")) {
        query(&s);
        return 0;
    }
    for x in a {
        let Some((k, v)) = x.split_once('=') else {
            eprint(&alloc::format!("TCPCFG2: {x}: expected setting=value\r\n"));
            return 1;
        };
        match k.to_ascii_uppercase().as_str() {
            "NETWORK" => {
                let keep = s.ns.clone();
                apply_network(&mut s, v);
                if s.ns.is_empty() {
                    s.ns = keep;
                }
            }
            "HOSTNAME" => s.host = String::from(v),
            "DOMAIN" => s.domain = String::from(v),
            "NAMESERVERS" => s.ns = v.split(',').filter(|n| !n.is_empty()).map(String::from).collect(),
            _ => {
                eprint(&alloc::format!("TCPCFG2: unknown setting {k}\r\n"));
                return 1;
            }
        }
    }
    match save(&s) {
        Ok(()) => {
            let now = apply_now(&s);
            query(&s);
            match now {
                Ok(m) => outln(&alloc::format!("TCPCFG2: saved. {m} The host name takes effect when rs2 starts again.")),
                Err(m) => outln(&alloc::format!("TCPCFG2: {m}")),
            }
            0
        }
        Err(e) => {
            eprint(&alloc::format!("TCPCFG2: {e}\r\n"));
            2
        }
    }
}

struct App {
    client: u32,
    pages: Vec<u32>,
    saved: Settings,
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

fn z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = os2::text_bytes(s).into_owned();
    v.push(0);
    v
}

/// A control by ID on any page or the client.
fn item(id: u32) -> u32 {
    let a = app();
    for &p in a.pages.iter().chain(core::iter::once(&a.client)) {
        let h = unsafe { WinWindowFromID(p, id) };
        if h != 0 {
            return h;
        }
    }
    0
}

fn set_text(id: u32, s: &str) {
    let t = z(&s.replace('\n', "\r\n"));
    unsafe { WinSetWindowText(item(id), t.as_ptr()) };
}

fn get_text(id: u32) -> String {
    let h = item(id);
    let n = unsafe { WinQueryWindowTextLength(h) }.max(0) as usize;
    let mut b = alloc::vec![0u8; n + 1];
    let got = unsafe { WinQueryWindowText(h, b.len() as i32, b.as_mut_ptr()) }.max(0) as usize;
    b[..got.min(n)].iter().map(|&c| c as char).collect()
}

fn checked(id: u32) -> bool {
    unsafe { WinSendMsg(item(id), BM_QUERYCHECK, 0, 0) != 0 }
}

fn check(id: u32, on: bool) {
    unsafe { WinSendMsg(item(id), BM_SETCHECK, on as u32, 0) };
}

fn control(parent: u32, class: &[u8], text: &str, style: u32, id: u32, x: i32, y: i32, cx: i32, cy: i32) -> u32 {
    let t = z(text);
    unsafe { WinCreateWindow(parent, class.as_ptr(), t.as_ptr(), style | WS_VISIBLE, x, y, cx, cy, parent, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

fn label(p: u32, t: &str, x: i32, y: i32, cx: i32) {
    control(p, WC_STATIC, t, SS_TEXT | DT_LEFT | DT_VCENTER, 0xffff, x, y, cx, 22);
}

fn entry(p: u32, id: u32, x: i32, y: i32, cx: i32, limit: u32) {
    let h = control(p, WC_ENTRYFIELD, "", ES_MARGIN | ES_AUTOSCROLL | WS_TABSTOP, id, x, y, cx, 22);
    unsafe { WinSendMsg(h, EM_SETTEXTLIMIT, limit, 0) };
}

/// The settings shown.
fn show(s: &Settings) {
    check(ID_ENABLE, s.enabled);
    check(ID_DHCP, s.dhcp);
    check(ID_MANUAL, !s.dhcp);
    set_text(ID_ADDR, &s.addr);
    set_text(ID_MASK, &s.mask);
    set_text(ID_ROUTER, &s.router);
    set_text(ID_HOST, &s.host);
    set_text(ID_DOMAIN, &s.domain);
    for i in 0..3 {
        set_text(ID_NS + i, s.ns.get(i as usize).map(String::as_str).unwrap_or(""));
    }
    set_text(ID_HOSTS, &s.hosts);
    set_text(ID_NOW, &alloc::format!("Now:\n{}", current()));
    enable_manual();
}

/// The settings as entered.
fn entered() -> Settings {
    Settings {
        enabled: checked(ID_ENABLE),
        dhcp: checked(ID_DHCP),
        addr: get_text(ID_ADDR),
        mask: get_text(ID_MASK),
        router: get_text(ID_ROUTER),
        host: get_text(ID_HOST),
        domain: get_text(ID_DOMAIN),
        ns: (0..3).map(|i| get_text(ID_NS + i)).filter(|n| !n.trim().is_empty()).collect(),
        hosts: get_text(ID_HOSTS),
    }
}

fn enable_manual() {
    let on = checked(ID_ENABLE) && !checked(ID_DHCP);
    for id in [ID_ADDR, ID_MASK, ID_ROUTER] {
        unsafe { WinEnableWindow(item(id), on as u32) };
    }
    for id in [ID_DHCP, ID_MANUAL] {
        unsafe { WinEnableWindow(item(id), checked(ID_ENABLE) as u32) };
    }
}

fn build_network(p: u32, h: i32) {
    let mut y = h - 40;
    control(p, WC_BUTTON, "~Enable the network adapter (lan0)", BS_AUTOCHECKBOX | WS_TABSTOP | WS_GROUP, ID_ENABLE, 16, y, 300, 22);
    y -= 34;
    control(p, WC_BUTTON, "Automatically, using ~DHCP", BS_AUTORADIOBUTTON | WS_TABSTOP | WS_GROUP, ID_DHCP, 32, y, 280, 22);
    y -= 26;
    control(p, WC_BUTTON, "~Manually, using:", BS_AUTORADIOBUTTON, ID_MANUAL, 32, y, 280, 22);
    for (i, (t, id)) in [("IP address:", ID_ADDR), ("Subnet mask:", ID_MASK), ("Router:", ID_ROUTER)].iter().enumerate() {
        let yy = y - 30 - i as i32 * 28;
        label(p, t, 56, yy, 110);
        entry(p, *id, 170, yy, 150, 15);
    }
    y -= 30 + 3 * 28;
    control(p, WC_STATIC, "", SS_TEXT | DT_LEFT | DT_TOP | DT_WORDBREAK, ID_NOW, 16, 8, 380, (y - 4).max(40));
}

fn build_hosts_page(p: u32, h: i32) {
    let mut y = h - 40;
    label(p, "Host name:", 16, y, 140);
    entry(p, ID_HOST, 160, y, 200, 63);
    y -= 30;
    label(p, "Local domain name:", 16, y, 140);
    entry(p, ID_DOMAIN, 160, y, 200, 127);
    y -= 40;
    label(p, "Name servers:", 16, y, 140);
    for i in 0..3 {
        entry(p, ID_NS + i, 160, y - i as i32 * 28, 150, 15);
    }
    y -= 3 * 28 + 6;
    control(
        p,
        WC_STATIC,
        "With DHCP the network's own name servers are used; these are written to RESOLV2 for programs that read it, and with a fixed address they are the system's name servers.",
        SS_TEXT | DT_LEFT | DT_TOP | DT_WORDBREAK,
        0xffff,
        16,
        8,
        380,
        (y - 8).max(30),
    );
}

fn build_hosts_file(p: u32, h: i32, w: i32) {
    label(p, "Names and addresses (HOSTS), one per line: address name [aliases]", 12, h - 34, w - 24);
    let m = control(p, WC_MLE, "", MLS_BORDER | MLS_VSCROLL | MLS_HSCROLL | WS_TABSTOP, ID_HOSTS, 12, 10, w - 24, h - 50);
    unsafe { WinSendMsg(m, MLM_SETTEXTLIMIT, 65535, 0) };
}

fn message(s: &str) {
    set_text(ID_MSG, s);
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if !a.is_empty() {
        return command_line(&a);
    }
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if hmq == 0 {
        query(&load());
        return 0;
    }
    let settings = load();
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App { client: 0, pages: Vec::new(), saved: settings.clone() });
        WinRegisterClass(hab, CLASS.as_ptr(), client_proc, WS_CLIPCHILDREN, 0);
        WinRegisterClass(hab, PAGE_CLASS.as_ptr(), page_proc, WS_CLIPCHILDREN, 0);
    }
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON | FCF_TASKLIST | 0x0200;
    let mut client = 0u32;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), b"TCP/IP Configuration\0".as_ptr(), WS_CLIPCHILDREN, 0, 0, &mut client) };
    app().client = client;
    let (cw, ch) = (560, 430);
    let mut r = RectL { l: 0, b: 0, r: cw, t: ch };
    unsafe {
        WinCalcFrameRect(frame, &mut r, 0);
    }
    let (fw, fh) = (r.r - r.l, r.t - r.b);
    let (sw, sh) = unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) };
    unsafe { WinSetWindowPos(frame, HWND_TOP, (sw - fw) / 2, (sh - fh) / 2, fw, fh, SWP_SIZE | SWP_MOVE) };
    // The notebook over the buttons and the message line.
    let nb_style = BKS_BACKPAGESBR | BKS_MAJORTABRIGHT | BKS_SPIRALBIND | BKS_ROUNDEDTABS | BKS_STATUSTEXTCENTER | BKS_TABTEXTCENTER | WS_TABSTOP;
    let nb = control(client, WC_NOTEBOOK, "", nb_style, ID_BOOK, 6, 74, cw - 12, ch - 80);
    unsafe { WinSendMsg(nb, BKM_SETDIMENSIONS, (100 | 26 << 16) as u32, BKA_MAJORTAB) };
    let mut pr = RectL { l: 0, b: 0, r: cw - 12, t: ch - 80 };
    unsafe { WinSendMsg(nb, BKM_CALCPAGERECT, &mut pr as *mut RectL as u32, 1) };
    let (pw, ph) = ((pr.r - pr.l).max(200), (pr.t - pr.b).max(200));
    let tabs = ["Network", "Host Names", "Hosts"];
    for (i, t) in tabs.iter().enumerate() {
        let page = unsafe { WinCreateWindow(nb, PAGE_CLASS.as_ptr(), b"\0".as_ptr(), WS_CLIPCHILDREN, 0, 0, pw, ph, nb, HWND_TOP, 1000 + i as u32, core::ptr::null(), core::ptr::null()) };
        app().pages.push(page);
        match i {
            0 => build_network(page, ph),
            1 => build_hosts_page(page, ph),
            _ => build_hosts_file(page, ph, pw),
        }
        let id = unsafe { WinSendMsg(nb, BKM_INSERTPAGE, 0, (BKA_MAJOR | BKA_STATUSTEXTON | BKA_AUTOPAGESIZE) | BKA_LAST << 16) };
        let tt = z(t);
        let st = z(&alloc::format!("Page {} of {}", i + 1, tabs.len()));
        unsafe {
            WinSendMsg(nb, BKM_SETPAGEWINDOWHWND, id, page);
            WinSendMsg(nb, BKM_SETTABTEXT, id, tt.as_ptr() as u32);
            WinSendMsg(nb, BKM_SETSTATUSLINETEXT, id, st.as_ptr() as u32);
        }
    }
    control(client, WC_STATIC, "", SS_TEXT | DT_LEFT | DT_WORDBREAK, ID_MSG, 12, 44, cw - 24, 26);
    control(client, WC_BUTTON, "~Save", BS_PUSHBUTTON | WS_TABSTOP | WS_GROUP, ID_SAVE, 12, 8, 96, 30);
    control(client, WC_BUTTON, "~Undo", BS_PUSHBUTTON | WS_TABSTOP, ID_UNDO, 116, 8, 96, 30);
    control(client, WC_BUTTON, "Close", BS_PUSHBUTTON | WS_TABSTOP, ID_CLOSE, 220, 8, 96, 30);
    show(&settings);
    message("Changes take effect when rs2 starts again.");
    unsafe {
        WinSetWindowPos(frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
        WinSetFocus(HWND_DESKTOP, item(ID_ENABLE));
        // The interfaces' state, brought up to date every two seconds.
        WinStartTimer(hab, client, 1, 2000);
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

unsafe extern "C" {
    #[link_name = "PMWIN.704"]
    fn WinCalcFrameRect(h: u32, r: *mut RectL, client: u32) -> u32;
    #[link_name = "PMWIN.884"]
    fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
}

const WM_TIMER: u32 = 0x0024;

fn command(id: u32) {
    match id {
        ID_SAVE => {
            let s = entered();
            match save(&s) {
                Ok(()) => {
                    let now = apply_now(&s);
                    app().saved = s;
                    match now {
                        Ok(m) => message(&alloc::format!("Saved. {m} The host name takes effect when rs2 starts again.")),
                        Err(m) => message(&m),
                    }
                }
                Err(e) => message(&e),
            }
        }
        ID_UNDO => {
            let s = app().saved.clone();
            show(&s);
            message("The saved settings are shown again.");
        }
        ID_CLOSE => unsafe {
            WinPostMsg(app().client, WM_QUIT, 0, 0);
        },
        _ => {}
    }
}

extern "C" fn page_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            unsafe {
                WinFillRect(hps, &r, CLR_PALEGRAY);
                WinEndPaint(hps);
            }
            0
        }
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            if code == BN_CLICKED && matches!(id, ID_ENABLE | ID_DHCP | ID_MANUAL) {
                enable_manual();
            }
            0
        }
        WM_COMMAND => {
            command(mp1 & 0xffff);
            0
        }
        WM_TIMER => {
            set_text(ID_NOW, &alloc::format!("Now:\n{}", current()));
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            unsafe {
                WinFillRect(hps, &r, CLR_PALEGRAY);
                WinEndPaint(hps);
            }
            0
        }
        WM_COMMAND => {
            command(mp1 & 0xffff);
            0
        }
        WM_TIMER => {
            set_text(ID_NOW, &alloc::format!("Now:\n{}", current()));
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}
