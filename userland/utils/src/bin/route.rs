//! ROUTE: change the routing table, in the form of OS/2's TCP/IP 4.x
//! command:
//!
//! ```text
//! ROUTE [-n] [-v] [-f] ADD    [net|host|-net|-host] destination gateway [metric] [-netmask mask]
//! ROUTE [-n] [-v] [-f] DELETE [net|host|-net|-host] destination [gateway] [-netmask mask]
//! ROUTE -f | -fh | FLUSH
//! ROUTE PRINT
//! ```
//!
//! `destination` is `default`, a dotted address, `address/bits` or a name
//! (resolved unless -n); `host` routes take the whole address, `net`
//! routes the -netmask, else the class mask (or a longer one when the
//! address has bits beyond it). -f first deletes every route through a
//! gateway. The table is shown by NETSTAT -r (and ROUTE PRINT).
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
    outln("Usage: ROUTE [-n] [-v] [-f] ADD [net|host] destination gateway [metric] [-netmask mask]");
    outln("       ROUTE [-n] [-v] [-f] DELETE [net|host] destination [gateway] [-netmask mask]");
    outln("       ROUTE -f | FLUSH          delete every route through a gateway");
    outln("       ROUTE PRINT               show the routing table");
    outln("  destination: default, an address, address/bits or a host or network name");
    1
}

fn addr(s: &str, numeric: bool) -> Option<[u8; 4]> {
    if let Some(a) = parse_ip(s) {
        return Some(a);
    }
    // Short forms of networks: "10", "192.168.5".
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit())) {
        let mut a = [0u8; 4];
        for (k, p) in parts.iter().enumerate() {
            a[k] = p.parse::<u32>().ok().filter(|&v| v < 256)? as u8;
        }
        return Some(a);
    }
    if numeric {
        return None;
    }
    resolve(s).ok().and_then(|v| v.first().copied())
}

/// The mask of a network route without -netmask: the address class's,
/// or longer when the address has bits beyond it.
fn natural_mask(a: [u8; 4]) -> [u8; 4] {
    let v = u32::from_be_bytes(a);
    let class = match a[0] {
        0..=127 => 8,
        128..=191 => 16,
        _ => 24,
    };
    let need = if v == 0 { 0 } else { 32 - v.trailing_zeros() };
    let bits = class.max(need);
    (if bits == 0 { 0 } else { u32::MAX << (32 - bits) }).to_be_bytes()
}

fn mask_bits(bits: u32) -> [u8; 4] {
    (if bits == 0 { 0 } else { u32::MAX << (32 - bits.min(32)) }).to_be_bytes()
}

fn print_table() {
    outln("Destination      Gateway          Netmask          Flags   Metric Use    Interface");
    for r in routes() {
        let dest = if r.dest == [0; 4] && r.mask == [0; 4] { String::from("default") } else { ip(r.dest) };
        let mut f = String::from("U");
        if r.flags & 2 != 0 {
            f.push('G');
        }
        if r.flags & 4 != 0 {
            f.push('H');
        }
        if r.flags & 0x800 != 0 {
            f.push('S');
        }
        if r.flags & 0x10 != 0 {
            f.push('D');
        }
        outln(&format!("{dest:<16} {:<16} {:<16} {f:<7} {:<6} {:<6} {}", ip(r.gateway), ip(r.mask), r.metric, r.uses, r.iface));
    }
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let (mut numeric, mut verbose, mut flush) = (false, false, false);
    let mut rest: Vec<String> = Vec::new();
    for x in &a {
        let l = x.to_ascii_lowercase();
        if (l.starts_with('-') || l.starts_with('/')) && rest.is_empty() && l != "-net" && l != "-host" && l != "-netmask" {
            for c in l[1..].chars() {
                match c {
                    'n' => numeric = true,
                    'v' => verbose = true,
                    'f' => flush = true,
                    'h' | 'q' => {}
                    '?' => return usage(),
                    _ => {
                        eprint(&format!("ROUTE: unknown option {x}\r\n"));
                        return usage();
                    }
                }
            }
        } else {
            rest.push(x.clone());
        }
    }
    if interfaces().is_empty() {
        eprint("ROUTE: TCP/IP is not running\r\n");
        return 1;
    }
    let cmd = rest.first().map(|c| c.to_ascii_lowercase());
    if flush || cmd.as_deref() == Some("flush") {
        match route_flush() {
            Ok(n) => {
                if verbose || cmd.as_deref() == Some("flush") {
                    outln(&format!("{n} route(s) through gateways deleted"));
                }
            }
            Err(e) => {
                eprint(&format!("ROUTE: flush: {}\r\n", strerror(e)));
                return 1;
            }
        }
        if cmd.is_none() || cmd.as_deref() == Some("flush") {
            return 0;
        }
    }
    let Some(cmd) = cmd else { return usage() };
    if cmd == "print" || cmd == "show" {
        print_table();
        return 0;
    }
    let add = match cmd.as_str() {
        "add" => true,
        "delete" | "del" => false,
        _ => {
            eprint(&format!("ROUTE: unknown command {}\r\n", rest[0]));
            return usage();
        }
    };
    // Modifiers and arguments.
    let (mut host, mut net, mut mask) = (false, false, None);
    let mut words: Vec<String> = Vec::new();
    let mut i = 1;
    while i < rest.len() {
        let l = rest[i].to_ascii_lowercase();
        match l.as_str() {
            "host" | "-host" => host = true,
            "net" | "-net" => net = true,
            "-netmask" | "netmask" | "mask" => {
                i += 1;
                match rest.get(i).and_then(|m| parse_ip(m)) {
                    Some(m) => mask = Some(m),
                    None => {
                        eprint("ROUTE: -netmask needs a mask like 255.255.255.0\r\n");
                        return 1;
                    }
                }
            }
            _ => words.push(rest[i].clone()),
        }
        i += 1;
    }
    let Some(d) = words.first() else { return usage() };
    let (dest, dmask) = if d.eq_ignore_ascii_case("default") {
        ([0u8; 4], [0u8; 4])
    } else {
        let (text, bits) = match d.split_once('/') {
            Some((t, b)) => match b.parse::<u32>() {
                Ok(n) if n <= 32 => (t, Some(n)),
                _ => {
                    eprint(&format!("ROUTE: {d}: bad prefix length\r\n"));
                    return 1;
                }
            },
            None => (d.as_str(), None),
        };
        let Some(a) = addr(text, numeric) else {
            eprint(&format!("ROUTE: {text}: unknown host or network\r\n"));
            return 1;
        };
        let m = match (mask, bits) {
            (Some(m), _) => m,
            (None, Some(b)) => mask_bits(b),
            // A full address with host bits is a host route (as BSD's).
            (None, None) if host || (!net && parse_ip(text).is_some() && natural_mask(a) == [255; 4]) => [255; 4],
            (None, None) => natural_mask(a),
        };
        (a, if host { [255; 4] } else { m })
    };
    let gw = match words.get(1) {
        Some(g) => match addr(g, numeric) {
            Some(x) => x,
            None => {
                eprint(&format!("ROUTE: {g}: unknown host\r\n"));
                return 1;
            }
        },
        None if add => return usage(),
        None => [0; 4],
    };
    let metric = words.get(2).and_then(|m| m.parse::<u32>().ok()).unwrap_or(1);
    let what = if dmask == [255; 4] { "host" } else { "net" };
    let shown = if dest == [0; 4] && dmask == [0; 4] { String::from("default") } else { ip(dest) };
    let r = if add { route_add(dest, dmask, gw, metric) } else { route_delete(dest, dmask, gw) };
    match r {
        Ok(()) => {
            outln(&format!(
                "{} {what} {shown}{}{}",
                if add { "add" } else { "delete" },
                if gw != [0; 4] { format!(": gateway {}", ip(gw)) } else { String::new() },
                if verbose && dmask != [0; 4] && dmask != [255; 4] { format!(" netmask {}", ip(dmask)) } else { String::new() }
            ));
            0
        }
        Err(e) => {
            let why = match e {
                17 => String::from("route already in table"),
                3 => String::from("not in table"),
                51 => String::from("gateway is not on a connected network"),
                _ => String::from(strerror(e)),
            };
            eprint(&format!("ROUTE: {} {what} {shown}: {why}\r\n", if add { "add" } else { "delete" }));
            1
        }
    }
}
