//! IFCONFIG [interface]: show the network interfaces (addresses, flags,
//! hardware address, counters). Interfaces have OS/2's names (lan0.., lo)
//! with the kernel's name beside them.
//!
//! `IFCONFIG interface [address] [netmask mask] [broadcast addr]
//! [metric n] [mtu n] [up|down] [alias|-alias] [delete addr] [dhcp]
//! [release]` changes one at once (until the next start; the boot
//! configuration is CONFIG.SYS's NETWORK=, which TCPCFG2 writes): a
//! fixed address ends DHCP on it; `alias` adds the address beside the
//! others, `-alias` or `delete` removes one; `dhcp` starts DHCP or renews
//! the lease, `release` gives the lease back.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::String;
use os2::net::*;
use utils::*;
os2::entry!(main);

fn flag_names(i: &IfInfo) -> (u32, String) {
    let mut v = alloc::vec::Vec::new();
    let mut f = 0;
    if i.flags & IF_UP != 0 {
        f |= 0x1;
        v.push("UP");
    }
    if i.flags & IF_LOOPBACK != 0 {
        f |= 0x8;
        v.push("LOOPBACK");
    } else {
        f |= 0x2;
        v.push("BROADCAST");
    }
    if i.flags & (IF_LINK | IF_LOOPBACK) != 0 {
        f |= 0x40;
        v.push("RUNNING");
    }
    (f, v.join(","))
}

fn mac(m: [u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
}

fn show(name: &str, i: &IfInfo) {
    let (f, names) = flag_names(i);
    outln(&format!("{name}: flags={f:x}<{names}> mtu {}  ({})", i.mtu, i.name()));
    if i.addr != [0; 4] {
        let mut b = [0u8; 4];
        for k in 0..4 {
            b[k] = i.addr[k] | !i.mask[k];
        }
        let bcast = if i.flags & IF_LOOPBACK != 0 { String::new() } else { format!(" broadcast {}", ip(b)) };
        outln(&format!("        inet {} netmask {}{bcast}", ip(i.addr), ip(i.mask)));
    } else {
        outln("        no address");
    }
    if i.flags & IF_LOOPBACK == 0 {
        let how = if i.flags & IF_DHCP != 0 {
            if i.flags & IF_BOUND != 0 { "DHCP, lease bound" } else { "DHCP, no lease" }
        } else {
            "static"
        };
        outln(&format!("        ether {}  link {}  {how}", mac(i.mac), if i.flags & IF_LINK != 0 { "up" } else { "down" }));
        if i.gateway != [0; 4] {
            outln(&format!("        gateway {}", ip(i.gateway)));
        }
        if i.ndns > 0 {
            let d: alloc::vec::Vec<String> = i.dns[..(i.ndns as usize).min(4)].iter().map(|a| ip(*a)).collect();
            outln(&format!("        dns {}", d.join(" ")));
        }
    }
    if let Some(index) = iface_index(name) {
        for (a, m) in if_addrs(index).into_iter().skip(1) {
            outln(&format!("        inet {} netmask {} alias", ip(a), ip(m)));
        }
    }
    outln(&format!("        RX packets {} bytes {}  dropped {}", i.rx_packets, i.rx_bytes, i.rx_dropped));
    outln(&format!("        TX packets {} bytes {}", i.tx_packets, i.tx_bytes));
}

fn usage() -> u32 {
    outln("Usage: IFCONFIG [interface | -a]");
    outln("       IFCONFIG interface [address] [netmask mask] [broadcast addr] [metric n] [mtu n]");
    outln("                [up | down] [alias | -alias] [delete address] [dhcp] [release]");
    0
}

/// Change an interface as the words after its name say.
fn configure(name: &str, w: &[String]) -> u32 {
    let Some(index) = iface_index(name) else {
        eprint(&format!("IFCONFIG: {name}: no such interface\r\n"));
        return 1;
    };
    let (mut addr, mut mask, mut alias, mut unalias) = (None, None, false, false);
    let mut ops: alloc::vec::Vec<(u32, [u8; 4], [u8; 4], String)> = alloc::vec::Vec::new();
    let mut i = 0;
    let num = |x: Option<&String>| x.and_then(|v| v.parse::<u32>().ok());
    while i < w.len() {
        let l = w[i].to_ascii_lowercase();
        match l.as_str() {
            "inet" => {}
            "netmask" | "mask" => {
                i += 1;
                let m = w.get(i).and_then(|m| parse_ip(m).or_else(|| hex_mask(m)));
                match m {
                    Some(m) => mask = Some(m),
                    None => {
                        eprint("IFCONFIG: netmask needs a mask like 255.255.255.0\r\n");
                        return 1;
                    }
                }
            }
            "broadcast" => i += 1,
            "metric" => {
                i += 1;
                let Some(n) = num(w.get(i)) else { return usage() + 1 };
                ops.push((IFOP_METRIC, n.to_le_bytes(), [0; 4], format!("metric {n}")));
            }
            "mtu" => {
                i += 1;
                let Some(n) = num(w.get(i)) else { return usage() + 1 };
                ops.push((IFOP_MTU, n.to_le_bytes(), [0; 4], format!("mtu {n}")));
            }
            "up" => ops.push((IFOP_UP, [0; 4], [0; 4], String::from("up"))),
            "down" => ops.push((IFOP_DOWN, [0; 4], [0; 4], String::from("down"))),
            "alias" | "add" => alias = true,
            "-alias" | "delete" | "remove" => {
                unalias = true;
                if let Some(a) = w.get(i + 1).and_then(|x| parse_ip(x)) {
                    addr = Some(a);
                    i += 1;
                }
            }
            "dhcp" | "renew" => ops.push((IFOP_DHCP_RENEW, [0; 4], [0; 4], String::from("dhcp"))),
            "release" | "-dhcp" => ops.push((IFOP_DHCP_RELEASE, [0; 4], [0; 4], String::from("dhcp release"))),
            "arp" | "-arp" | "trailers" | "-trailers" | "debug" | "-debug" | "allrs" | "-allrs" | "802.3" | "-802.3" | "snap" | "-snap" => {}
            _ => match parse_ip(&w[i]) {
                Some(a) if addr.is_none() => addr = Some(a),
                _ => {
                    eprint(&format!("IFCONFIG: {}: not understood\r\n", w[i]));
                    return usage() + 1;
                }
            },
        }
        i += 1;
    }
    let mut first: alloc::vec::Vec<(u32, [u8; 4], [u8; 4], String)> = alloc::vec::Vec::new();
    match (addr, alias, unalias) {
        (Some(a), _, true) => first.push((IFOP_ALIAS_DELETE, a, [0; 4], format!("{} removed", ip(a)))),
        (Some(a), true, _) => first.push((IFOP_ALIAS_ADD, a, mask.unwrap_or([0; 4]), format!("alias {}", ip(a)))),
        (Some(a), false, false) => first.push((IFOP_ADDRESS, a, mask.unwrap_or([0; 4]), format!("inet {}", ip(a)))),
        (None, _, _) => {
            if let Some(m) = mask {
                first.push((IFOP_NETMASK, m, [0; 4], format!("netmask {}", ip(m))));
            }
        }
    }
    first.extend(ops);
    if first.is_empty() {
        return usage() + 1;
    }
    for (op, a, b, what) in first {
        if let Err(e) = if_set(index, op, a, b) {
            let why = match e {
                17 => "the address is in use",
                49 => "no such address on the interface",
                22 => "not possible on this interface",
                6 => "no such interface",
                _ => strerror(e),
            };
            eprint(&format!("IFCONFIG: {name}: {what}: {why}\r\n"));
            return 1;
        }
    }
    0
}

/// A mask in hex (0xffffff00), as BSD's ifconfig takes it.
fn hex_mask(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    u32::from_str_radix(h, 16).ok().map(|v| v.to_be_bytes())
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.iter().any(|x| x == "-?" || x == "/?") {
        return usage();
    }
    if a.len() >= 2 && !a[0].starts_with('-') {
        if os2_interfaces().is_empty() {
            eprint("IFCONFIG: no network interfaces (TCP/IP is not running)\r\n");
            return 1;
        }
        let rc = configure(&a[0], &a[1..]);
        if rc != 0 {
            return rc;
        }
    }
    let list = os2_interfaces();
    if list.is_empty() {
        eprint("IFCONFIG: no network interfaces (TCP/IP is not running)\r\n");
        return 1;
    }
    let want = a.iter().find(|x| !x.starts_with('-'));
    let mut found = false;
    for (name, i) in &list {
        if let Some(w) = want {
            if !w.eq_ignore_ascii_case(name) && !w.eq_ignore_ascii_case(i.name()) {
                continue;
            }
        }
        found = true;
        show(name, i);
    }
    if !found {
        eprint(&format!("IFCONFIG: {}: no such interface\r\n", want.unwrap()));
        return 1;
    }
    0
}
