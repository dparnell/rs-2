//! IFCONFIG [interface]: show the network interfaces (addresses, flags,
//! hardware address, counters). Interfaces have OS/2's names (lan0.., lo)
//! with the kernel's name beside them; they are configured at boot (DHCP,
//! `net=` / NETWORK=), not with this command.
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
    outln(&format!("        RX packets {} bytes {}  dropped {}", i.rx_packets, i.rx_bytes, i.rx_dropped));
    outln(&format!("        TX packets {} bytes {}", i.tx_packets, i.tx_bytes));
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.iter().any(|x| x == "-?" || x == "/?") {
        outln("Usage: IFCONFIG [interface | -a]");
        return 0;
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
