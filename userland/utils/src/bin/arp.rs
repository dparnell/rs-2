//! ARP: show and change the address resolution table, in the form of
//! OS/2's TCP/IP 4.x command:
//!
//! ```text
//! ARP host                       the entry of a host
//! ARP -a [-n]                    every entry
//! ARP -d host                    delete an entry
//! ARP -s host ether [temp] [pub] set an entry (permanent unless temp;
//!                                pub: answer requests for the host)
//! ARP -f file                    set entries from a file of "host ether
//!                                [temp] [pub]" lines
//! ```
//!
//! Ethernet addresses are six hex bytes separated by `:` or `-`.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use os2::net::*;
use utils::*;
os2::entry!(main);

fn usage() -> u32 {
    outln("Usage: ARP host               show the entry of a host");
    outln("       ARP -a [-n]            show every entry");
    outln("       ARP -d host            delete an entry");
    outln("       ARP -s host ether [temp] [pub]");
    outln("       ARP -f file            set entries from a file");
    1
}

fn mac_text(m: [u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
}

fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(|c| c == ':' || c == '-' || c == '.').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut m = [0u8; 6];
    for (k, p) in parts.iter().enumerate() {
        if p.is_empty() || p.len() > 2 {
            return None;
        }
        m[k] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(m)
}

fn host(s: &str) -> Option<[u8; 4]> {
    parse_ip(s).or_else(|| resolve(s).ok().and_then(|v| v.first().copied()))
}

fn show(e: &ArpEntry) {
    let hw = if e.complete { mac_text(e.mac) } else { String::from("(incomplete)") };
    let mut extra = String::new();
    if e.permanent {
        extra.push_str(" permanent");
    } else if e.complete {
        extra.push_str(&format!(" {} s", e.ttl));
    }
    if e.publish {
        extra.push_str(" published");
    }
    outln(&format!("{:<16} {:<18} {:<6}{extra}", ip(e.ip), hw, iface_name(e.index)));
}

fn set(words: &[String]) -> Result<String, String> {
    let (Some(h), Some(m)) = (words.first(), words.get(1)) else { return Err(String::from("-s needs a host and an Ethernet address")) };
    let Some(a) = host(h) else { return Err(format!("{h}: unknown host")) };
    let Some(mac) = parse_mac(m) else { return Err(format!("{m}: bad Ethernet address")) };
    let rest: Vec<String> = words[2..].iter().map(|w| w.to_ascii_lowercase()).collect();
    let temp = rest.iter().any(|w| w == "temp");
    let publish = rest.iter().any(|w| w == "pub" || w == "publish");
    match arp_set(a, mac, !temp, publish) {
        Ok(()) => Ok(format!("{} at {}{}{}", ip(a), mac_text(mac), if temp { "" } else { " permanent" }, if publish { " published" } else { "" })),
        Err(51) => Err(format!("{}: not on a connected network", ip(a))),
        Err(e) => Err(format!("{}: {}", ip(a), strerror(e))),
    }
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.is_empty() {
        return usage();
    }
    if interfaces().is_empty() {
        eprint("ARP: TCP/IP is not running\r\n");
        return 1;
    }
    let opt = a[0].to_ascii_lowercase();
    match opt.as_str() {
        "-a" | "/a" => {
            let numeric = a.iter().any(|x| x.eq_ignore_ascii_case("-n"));
            let _ = numeric;
            let list = arp_table();
            if list.is_empty() {
                outln("The ARP table is empty.");
            } else {
                outln("Address          Ethernet address   Intf");
                for e in &list {
                    show(e);
                }
            }
            0
        }
        "-d" | "/d" => {
            let Some(h) = a.get(1) else { return usage() };
            let Some(x) = host(h) else {
                eprint(&format!("ARP: {h}: unknown host\r\n"));
                return 1;
            };
            match arp_delete(x) {
                Ok(()) => {
                    outln(&format!("{} deleted", ip(x)));
                    0
                }
                Err(_) => {
                    eprint(&format!("ARP: {}: no entry\r\n", ip(x)));
                    1
                }
            }
        }
        "-s" | "/s" => match set(&a[1..]) {
            Ok(m) => {
                outln(&m);
                0
            }
            Err(m) => {
                eprint(&format!("ARP: {m}\r\n"));
                1
            }
        },
        "-f" | "/f" => {
            let Some(f) = a.get(1) else { return usage() };
            let hf = match open_read(f) {
                Ok(h) => h,
                Err(e) => {
                    eprint(&format!("ARP: {f}: {}\r\n", sys_message(e)));
                    return 1;
                }
            };
            let data = read_all(hf);
            unsafe { os2::DosClose(hf) };
            let mut bad = 0;
            for l in lines(&data) {
                let t = l.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                let w: Vec<String> = t.split_whitespace().map(String::from).collect();
                match set(&w) {
                    Ok(m) => outln(&m),
                    Err(m) => {
                        eprint(&format!("ARP: {m}\r\n"));
                        bad += 1;
                    }
                }
            }
            (bad > 0) as u32
        }
        _ if opt.starts_with('-') || opt.starts_with('/') => usage(),
        _ => {
            let Some(x) = host(&a[0]) else {
                eprint(&format!("ARP: {}: unknown host\r\n", a[0]));
                return 1;
            };
            match arp_table().into_iter().find(|e| e.ip == x) {
                Some(e) => {
                    show(&e);
                    0
                }
                None => {
                    outln(&format!("{} ({}): no entry", a[0], ip(x)));
                    1
                }
            }
        }
    }
}
