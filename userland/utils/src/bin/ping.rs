//! PING: send ICMP echo requests. PING [-c count] [-s size] [-w ms] host
#![no_std]
#![no_main]
extern crate alloc;
use os2::net::*;
use utils::*;
os2::entry!(main);

fn usage() -> u32 {
    eprint("Usage: PING [-c count] [-s size] [-w timeout_ms] host\r\n");
    1
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let (mut count, mut size, mut wait) = (4u32, 56usize, 1000u32);
    let mut host = None;
    let mut i = 0;
    while i < a.len() {
        let v = |i: usize| a.get(i + 1).and_then(|x| x.parse::<u32>().ok());
        match a[i].to_ascii_lowercase().as_str() {
            "-c" | "-n" | "/n" => {
                let Some(n) = v(i) else { return usage() };
                count = n.max(1);
                i += 1;
            }
            "-s" | "-l" | "/l" => {
                let Some(n) = v(i) else { return usage() };
                size = (n as usize).min(65000);
                i += 1;
            }
            "-w" | "/w" => {
                let Some(n) = v(i) else { return usage() };
                wait = n.max(1);
                i += 1;
            }
            "-?" | "/?" => return usage(),
            _ => host = Some(a[i].clone()),
        }
        i += 1;
    }
    let Some(host) = host else { return usage() };
    let dst = match resolve(&host) {
        Ok(v) if !v.is_empty() => v[0],
        Ok(_) | Err(_) => {
            eprint(&alloc::format!("PING: {host}: unknown host\r\n"));
            return 2;
        }
    };
    let mut sock = 0;
    let rc = unsafe { Rs2SockCreate(AF_INET, SOCK_RAW, IPPROTO_ICMP, &mut sock) };
    if rc != 0 {
        eprint(&alloc::format!("PING: socket: {}\r\n", strerror(rc)));
        return 3;
    }
    outln(&alloc::format!("PING {host} ({}): {size} data bytes", ip(dst)));
    let ident = (ms() as u16) ^ 0x5253;
    let (mut received, mut min, mut max, mut total) = (0u32, u32::MAX, 0u32, 0u32);
    for seq in 1..=count {
        let start = ms();
        match ping_once(sock, dst, ident, seq as u16, size, wait) {
            Ok(r) => {
                received += 1;
                min = min.min(r.rtt_ms);
                max = max.max(r.rtt_ms);
                total += r.rtt_ms;
                outln(&alloc::format!("{} bytes from {}: icmp_seq={seq} ttl={} time={} ms", r.bytes + 8, ip(r.from), r.ttl, r.rtt_ms));
            }
            Err(EWOULDBLOCK) => outln(&alloc::format!("Request timed out (icmp_seq={seq})")),
            Err(e) => outln(&alloc::format!("icmp_seq={seq}: {}", strerror(e))),
        }
        // One request per second.
        let spent = ms().wrapping_sub(start);
        if seq < count && spent < 1000 {
            unsafe { os2::DosSleep(1000 - spent) };
        }
    }
    unsafe { Rs2SockClose(sock) };
    outln(&alloc::format!("--- {host} ping statistics ---"));
    let loss = (count - received) * 100 / count;
    outln(&alloc::format!("{count} packets transmitted, {received} packets received, {loss}% packet loss"));
    if received > 0 {
        outln(&alloc::format!("round-trip min/avg/max = {min}/{}/{max} ms", total / received));
        0
    } else {
        1
    }
}
