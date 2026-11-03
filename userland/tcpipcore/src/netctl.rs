//! The configuration and statistics ioctls (routes, ARP, interfaces,
//! SIOSTAT*), `sysctl()` and the memory-mapped send buffers of OS/2's
//! high-performance sockets (MSG_MAPIO), over the kernel's Rs2NetControl
//! (RS2API 240). Structure layouts are the 4.5 Toolkit's (net/route.h,
//! net/if_arp.h, net/if.h, netinet/*_var.h, sys/sysctl.h).

use alloc::vec;
use alloc::vec::Vec;

use os2::net::*;

use crate::errno::{fail, EFAULT, EINVAL, ENOMEM, ENOPROTOOPT, ENXIO};
use crate::sock::Abi;

unsafe fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}

unsafe fn wr(p: *mut u8, o: usize, b: &[u8]) {
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p.add(o), b.len()) };
}

/// The IPv4 address of a `sockaddr` at `p + o` (both layouts keep it at
/// bytes 4-7).
unsafe fn sa_addr(p: *const u8, o: usize) -> [u8; 4] {
    unsafe { rd32(p, o + 4) }.to_le_bytes()
}

unsafe fn ifr_index(data: *const u8) -> Option<u32> {
    let name = unsafe { crate::cstr::bytes(data) };
    let name = &name[..name.len().min(16)];
    let s: alloc::string::String = name.iter().map(|&b| b as char).collect();
    iface_index(&s)
}

fn r(res: Result<(), u32>) -> i32 {
    match res {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// The mask of an `ortentry` route, which carries none: a host's, the
/// default route's, the network's of an interface on it, else the
/// class mask (longer when the address has bits beyond it).
fn route_mask(dst: [u8; 4], host: bool) -> [u8; 4] {
    if host {
        return [255; 4];
    }
    if dst == [0; 4] {
        return [0; 4];
    }
    for (k, _) in interfaces().iter().enumerate() {
        for (a, m) in if_addrs(k as u32) {
            if (0..4).all(|x| a[x] & m[x] == dst[x]) && (0..4).all(|x| dst[x] & !m[x] == 0) {
                return m;
            }
        }
    }
    let v = u32::from_be_bytes(dst);
    let class = match dst[0] {
        0..=127 => 8,
        128..=191 => 16,
        _ => 24,
    };
    let bits = class.max(32 - v.trailing_zeros());
    (u32::MAX << (32 - bits)).to_be_bytes()
}

/// The ioctls `sock::ioctl_len` leaves to this module: Some(result), or
/// None when the command is not one of them.
pub unsafe fn ioctl(abi: Abi, group: u8, num: u8, data: *mut u8, len: i32) -> Option<i32> {
    let known = matches!(
        (group, num),
        (b'r', 10 | 11)
            | (b'i', 12 | 16 | 22 | 24 | 30 | 31 | 32 | 45 | 63 | 64 | 156)
            | (b'n', 41 | 42 | 43 | 46 | 47 | 48 | 49 | 50 | 51 | 53 | 55 | 241 | 242 | 243 | 247)
    );
    if !known {
        return None;
    }
    if data.is_null() && !matches!((group, num), (b'n', 46) | (b'i', 156)) {
        return Some(fail(EFAULT));
    }
    let room = if len < 0 { 65536 } else { len as usize };
    Some(unsafe {
        match (group, num) {
            // SIOCADDRT / SIOCDELRT: struct ortentry.
            (b'r', 10) | (b'r', 11) => {
                let dst = sa_addr(data, 4);
                let gw = sa_addr(data, 20);
                let flags = (rd32(data, 36) & 0xffff) as u16;
                let mask = route_mask(dst, flags & 4 != 0);
                if num == 10 {
                    let metric = rd32(data, 48);
                    r(route_add(dst, mask, gw, if metric == 0 || metric > 255 { 1 } else { metric }))
                } else {
                    r(route_delete(dst, mask, gw))
                }
            }
            // SIOCSIFADDR, SIOCSIFNETMASK.
            (b'i', 12) | (b'i', 22) => {
                let Some(i) = ifr_index(data) else { return Some(fail(ENXIO)) };
                let a = sa_addr(data, 16);
                if num == 12 { r(if_set(i, IFOP_ADDRESS, a, [0; 4])) } else { r(if_set(i, IFOP_NETMASK, a, [0; 4])) }
            }
            // SIOCSIFFLAGS: IFF_UP.
            (b'i', 16) => {
                let Some(i) = ifr_index(data) else { return Some(fail(ENXIO)) };
                let up = rd32(data, 16) & 1 != 0;
                r(if_set(i, if up { IFOP_UP } else { IFOP_DOWN }, [0; 4], [0; 4]))
            }
            // SIOCSIFMETRIC, SIOCSIFMTU: an int after the name.
            (b'i', 24) | (b'i', 45) => {
                let Some(i) = ifr_index(data) else { return Some(fail(ENXIO)) };
                let v = rd32(data, 16);
                r(if_set(i, if num == 24 { IFOP_METRIC } else { IFOP_MTU }, v.to_le_bytes(), [0; 4]))
            }
            // SIOCAIFADDR: struct ifaliasreq {name, addr, broadaddr, mask}.
            (b'i', 63) => {
                let Some(i) = ifr_index(data) else { return Some(fail(ENXIO)) };
                r(if_set(i, IFOP_ALIAS_ADD, sa_addr(data, 16), sa_addr(data, 48)))
            }
            // SIOCDIFADDR.
            (b'i', 64) => {
                let Some(i) = ifr_index(data) else { return Some(fail(ENXIO)) };
                r(if_set(i, IFOP_ALIAS_DELETE, sa_addr(data, 16), [0; 4]))
            }
            // SIOCSARP / SIOCGARP / SIOCDARP: struct arpreq {pa, ha, flags}.
            (b'i', 30) => {
                let ip = sa_addr(data, 0);
                let mut mac = [0u8; 6];
                core::ptr::copy_nonoverlapping(data.add(18), mac.as_mut_ptr(), 6);
                let f = rd32(data, 32);
                r(arp_set(ip, mac, f & 0x04 != 0, f & 0x08 != 0))
            }
            (b'i', 31) => {
                let ip = sa_addr(data, 0);
                match arp_table().into_iter().find(|e| e.ip == ip) {
                    Some(e) => {
                        let mut ha = [0u8; 16];
                        ha[0] = if abi == Abi::V41 { 16 } else { 0 };
                        ha[2..8].copy_from_slice(&e.mac);
                        wr(data, 16, &ha);
                        let mut f = 0x01u32;
                        if e.complete {
                            f |= 0x02;
                        }
                        if e.permanent {
                            f |= 0x04;
                        }
                        if e.publish {
                            f |= 0x08;
                        }
                        wr(data, 32, &f.to_le_bytes());
                        0
                    }
                    None => fail(ENXIO),
                }
            }
            (b'i', 32) => match arp_delete(sa_addr(data, 0)) {
                Ok(()) => 0,
                Err(_) => fail(ENXIO),
            },
            // SIOCARP: send an ARP request (resolution happens on use).
            (b'i', 156) => 0,
            // SIOFLUSHRT: every route through a gateway.
            (b'n', 46) => r(route_flush().map(|_| ())),
            // SIOFLUSHRTIFP: those of one interface (its index in *data).
            (b'n', 55) => {
                let want = rd32(data, 0);
                let mut res = Ok(());
                for rt in routes() {
                    if rt.flags & 2 != 0 && !rt.auto && rt.index == want {
                        res = route_delete(rt.dest, rt.mask, rt.gateway);
                    }
                }
                r(res)
            }
            (b'n', 41) | (b'n', 241) => put(data, room, &tcpstat()),
            (b'n', 42) | (b'n', 242) => put(data, room, &udpstat()),
            (b'n', 43) | (b'n', 243) => put(data, room, &ipstat()),
            (b'n', 47) | (b'n', 247) => put(data, room, &icmpstat()),
            (b'n', 48) | (b'n', 51) => put(data, room, &ifmib()),
            (b'n', 49) => put(data, room, &statat()),
            (b'n', 50) => put(data, room, &arptab()),
            (b'n', 53) => put(data, room, &(arp_table().len() as u32).to_le_bytes()),
            _ => fail(EINVAL),
        }
    })
}

unsafe fn put(data: *mut u8, room: usize, b: &[u8]) -> i32 {
    let n = b.len().min(room);
    unsafe { wr(data, 0, &b[..n]) };
    0
}

fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn st() -> Vec<u32> {
    let mut s = stats();
    s.resize(39, 0);
    s
}

/// struct tcpstat (60 u_longs).
pub fn tcpstat() -> Vec<u8> {
    let s = st();
    let mut t = [0u32; 60];
    t[0] = s[18]; // connattempt
    t[1] = s[19]; // accepts
    t[2] = (s[18] + s[19]).saturating_sub(s[20]); // connects
    t[4] = s[20]; // conndrops
    t[8] = s[34]; // delack
    t[10] = s[31]; // rexmttimeo
    t[13] = s[35]; // keepprobe
    t[14] = s[36]; // keepdrops
    t[15] = s[25]; // sndtotal
    t[17] = s[27]; // sndbyte
    t[18] = s[29] + s[31]; // sndrexmitpack
    t[19] = s[30]; // sndrexmitbyte
    t[25] = s[21]; // sndrst
    t[26] = s[26]; // rcvtotal
    t[28] = s[28]; // rcvbyte
    t[29] = s[22]; // rcvbadsum
    t[32] = s[33]; // rcvduppack
    t[36] = s[32]; // rcvoopack
    words(&t)
}

/// struct udpstat (9 u_longs).
pub fn udpstat() -> Vec<u8> {
    let s = st();
    words(&[s[14], 0, s[17], 0, s[16], 0, 0, 0, s[15]])
}

/// struct ipstat (24 u_longs).
pub fn ipstat() -> Vec<u8> {
    let s = st();
    let mut t = [0u32; 24];
    t[0] = s[0];
    t[1] = s[1];
    t[6] = s[7];
    t[9] = s[4];
    t[12] = s[6];
    t[13] = s[2];
    t[14] = s[3];
    t[16] = s[8];
    t[17] = s[9];
    t[21] = s[5];
    words(&t)
}

/// struct icmpstat (the u_short histograms; ICMP_MAXTYPE 18).
pub fn icmpstat() -> Vec<u8> {
    let s = st();
    let mut v = [0u16; 3 + 19 + 5 + 19];
    // outhist[ECHOREPLY], reflect, inhist[UNREACH], inhist[ECHO].
    v[3] = s[12] as u16;
    v[3 + 19 + 4] = s[12] as u16;
    v[3 + 19 + 5 + 3] = s[13] as u16;
    v[3 + 19 + 5 + 8] = s[12] as u16;
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// struct ifmib: short ifNumber, then a packed iftable (111 bytes) per
/// interface.
pub fn ifmib() -> Vec<u8> {
    let ifs = interfaces();
    let mut v = (ifs.len() as u16).to_le_bytes().to_vec();
    for (k, i) in ifs.iter().enumerate() {
        let lo = i.flags & IF_LOOPBACK != 0;
        let mut e = vec![0u8; 111];
        e[0..2].copy_from_slice(&(k as u16).to_le_bytes());
        let descr = alloc::format!("{} {}", iface_name(k as u32), i.name());
        let d = descr.as_bytes();
        e[2..2 + d.len().min(44)].copy_from_slice(&d[..d.len().min(44)]);
        e[47..49].copy_from_slice(&(if lo { 24u16 } else { 6 }).to_le_bytes());
        e[49..51].copy_from_slice(&(i.mtu as u16).to_le_bytes());
        e[51..57].copy_from_slice(&i.mac);
        let up = i.flags & IF_UP != 0 && (lo || i.flags & IF_LINK != 0);
        e[57..59].copy_from_slice(&(if up { 1u16 } else { 2 }).to_le_bytes());
        let longs = [if lo { 0 } else { 1_000_000_000u32 }, 0, i.rx_bytes, i.tx_bytes, 0, i.rx_dropped, 0, 0, 0, i.rx_packets, i.tx_packets, 0, 0];
        for (n, x) in longs.iter().enumerate() {
            e[59 + 4 * n..63 + 4 * n].copy_from_slice(&x.to_le_bytes());
        }
        v.extend_from_slice(&e);
    }
    v
}

/// SIOSTATAT: packed struct statatreq {u_long addr; short interface;
/// u_long mask, broadcast} per address, then a zero entry.
pub fn statat() -> Vec<u8> {
    let mut v = Vec::new();
    for (k, _) in interfaces().iter().enumerate() {
        for (a, m) in if_addrs(k as u32) {
            let mut b = [0u8; 4];
            for x in 0..4 {
                b[x] = a[x] | !m[x];
            }
            v.extend_from_slice(&a);
            v.extend_from_slice(&(k as u16).to_le_bytes());
            v.extend_from_slice(&m);
            v.extend_from_slice(&b);
        }
    }
    v.extend_from_slice(&[0u8; 14]);
    v
}

/// SIOSTATARP: packed struct oarptab (40 bytes) per entry (their number
/// is SIOSTATCNTAT's).
pub fn arptab() -> Vec<u8> {
    let mut v = Vec::new();
    for e in arp_table() {
        let mut x = [0u8; 40];
        x[0..4].copy_from_slice(&e.ip);
        x[4..10].copy_from_slice(&e.mac);
        x[10] = if e.permanent { 0 } else { ((600u32.saturating_sub(e.ttl)) / 60).min(255) as u8 };
        x[11] = 0x01 | if e.complete { 0x02 } else { 0 } | if e.permanent { 0x04 } else { 0 } | if e.publish { 0x08 } else { 0 };
        x[38..40].copy_from_slice(&(e.index as u16).to_le_bytes());
        v.extend_from_slice(&x);
    }
    v
}

// ---- sysctl ----

const CTL_KERN: i32 = 1;
const CTL_NET: i32 = 4;
const CTL_OS2: i32 = 9;
const KERN_HOSTNAME: i32 = 10;
const KERN_HOSTID: i32 = 11;
const KERNCTL_INETVER: i32 = 70;
const OS2_MEMMAPIO: i32 = 1;
const OS2_QUERY_MEMMAPIO: i32 = 2;

/// sysctl(name, namelen, oldp, oldlenp, newp, newlen): the network
/// variables of CTL_NET.PF_INET (IP forwarding; TCP keepalive probes,
/// window sizes, MSS, TTL, window scaling and timestamps; the
/// statistics structures), the host name and id, the stack version, and
/// CTL_OS2's memory-mapped send buffers.
pub unsafe fn sysctl(name: *const i32, namelen: u32, old: *mut u8, oldlen: *mut u32, new: *const u8, newlen: u32) -> i32 {
    if name.is_null() || namelen < 2 || namelen > 12 {
        return fail(EINVAL);
    }
    let mib: Vec<i32> = (0..namelen as usize).map(|i| unsafe { *name.add(i) }).collect();
    let new_int = || -> Option<u32> {
        if new.is_null() || newlen < 4 { None } else { Some(unsafe { rd32(new, 0) }) }
    };
    // Answer with `v`: copied to `old` as far as `*oldlen` allows.
    let answer = |v: &[u8]| -> i32 {
        if !oldlen.is_null() {
            let room = unsafe { *oldlen } as usize;
            if !old.is_null() {
                unsafe { wr(old, 0, &v[..v.len().min(room)]) };
            }
            unsafe { *oldlen = v.len().min(if old.is_null() { v.len() } else { room }) as u32 };
        }
        0
    };
    // A variable of the stack: report it, change it when asked.
    let var = |id: u32, scale: u32| -> i32 {
        let set = new_int().map(|v| v / scale.max(1));
        match net_var(id, set) {
            Ok(before) => answer(&(before * scale.max(1)).to_le_bytes()),
            Err(e) => fail(e),
        }
    };
    match (mib[0], mib.get(1).copied(), mib.get(2).copied(), mib.get(3).copied()) {
        (CTL_NET, Some(2), Some(0), Some(1 | 42)) => var(NV_FORWARDING, 1),
        (CTL_NET, Some(2), Some(6), Some(v)) => match v {
            1 => answer(&536u32.to_le_bytes()),
            2 => answer(&tcpstat()),
            8 => var(NV_KEEPCNT, 1),
            9 => var(NV_SENDSPACE, 1),
            10 => var(NV_RECVSPACE, 1),
            11 => answer(&64u32.to_le_bytes()),
            13 | 14 => answer(&1u32.to_le_bytes()),
            _ => fail(ENOPROTOOPT),
        },
        (CTL_NET, Some(2), Some(17), Some(v)) => match v {
            1 => answer(&1u32.to_le_bytes()),
            2 => answer(&udpstat()),
            32 => answer(&64u32.to_le_bytes()),
            _ => fail(ENOPROTOOPT),
        },
        (CTL_NET, Some(2), Some(1), Some(v)) => match v {
            2 => answer(&icmpstat()),
            1 | 3 => answer(&1u32.to_le_bytes()),
            50 => answer(&64u32.to_le_bytes()),
            _ => fail(ENOPROTOOPT),
        },
        (CTL_KERN, Some(KERN_HOSTNAME), _, _) => {
            let mut h = hostname().into_bytes();
            h.push(0);
            answer(&h)
        }
        (CTL_KERN, Some(KERN_HOSTID), _, _) => answer(&crate::sock::gethostid().to_le_bytes()),
        (CTL_KERN, Some(KERNCTL_INETVER), _, _) => {
            // struct inetvers_ctl {float version; char versionstr[10]}.
            let mut v = 4.21f32.to_le_bytes().to_vec();
            let mut s = [0u8; 10];
            s[..4].copy_from_slice(b"4.21");
            v.extend_from_slice(&s);
            answer(&v)
        }
        (CTL_OS2, _, _, Some(OS2_MEMMAPIO)) => unsafe { memmapio(old, oldlen) },
        (CTL_OS2, _, _, Some(OS2_QUERY_MEMMAPIO)) => 0,
        _ => fail(ENOPROTOOPT),
    }
}

// ---- Memory-mapped send buffers (HPS) ----

/// Blocks handed out by OS2_MEMMAPIO with the event semaphore (if the
/// caller gave one) to post when the stack is done with the block.
static MAPPED: crate::sync::Lock<Vec<(u32, u32)>> = crate::sync::Lock::new(Vec::new());

const BLOCK: u32 = 4096;
const BLOCKS: u32 = 15;

/// OS2_MEMMAPIO: 15 blocks of 4 KB in `old` (an array of 15 pointers,
/// whose entries may hold event semaphores to post when a block is free
/// again).
unsafe fn memmapio(old: *mut u8, oldlen: *mut u32) -> i32 {
    if old.is_null() || oldlen.is_null() || unsafe { *oldlen } < BLOCKS * 4 {
        return fail(EINVAL);
    }
    let mut p: *mut u8 = core::ptr::null_mut();
    // PAG_COMMIT | PAG_READ | PAG_WRITE.
    if unsafe { os2::DosAllocMem(&mut p, BLOCK * BLOCKS, 0x13) } != 0 || p.is_null() {
        return fail(ENOMEM);
    }
    let mut g = MAPPED.lock();
    for k in 0..BLOCKS {
        let at = old as *mut u32;
        let sem = unsafe { at.add(k as usize).read_unaligned() };
        let block = p as u32 + k * BLOCK;
        g.push((block, sem));
        unsafe { at.add(k as usize).write_unaligned(block) };
    }
    unsafe { *oldlen = BLOCKS * 4 };
    0
}

/// After a MSG_MAPIO send: the data was copied, so the blocks are free
/// again; post their semaphores.
pub fn mapio_sent(bases: &[u32]) {
    let g = MAPPED.lock();
    for b in bases {
        if let Some(&(_, sem)) = g.iter().find(|(blk, _)| *blk == (*b & !(BLOCK - 1))) {
            if sem != 0 {
                unsafe { os2::DosPostEventSem(sem) };
            }
        }
    }
}

pub const MSG_MAPIO: i32 = 0x400;
