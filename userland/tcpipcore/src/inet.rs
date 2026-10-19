//! inet_addr() and friends. Addresses in network order are `u32`s whose
//! memory bytes are the address (`u32::from_le_bytes([a, b, c, d])`).

use crate::tls;

pub const INADDR_NONE: u32 = 0xffff_ffff;

pub fn net_u32(a: [u8; 4]) -> u32 {
    u32::from_le_bytes(a)
}

pub fn net_bytes(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn number(s: &[u8]) -> Option<u32> {
    let (digits, radix) = if s.len() > 2 && (s.starts_with(b"0x") || s.starts_with(b"0X")) {
        (&s[2..], 16)
    } else if s.len() > 1 && s[0] == b'0' {
        (&s[1..], 8)
    } else {
        (s, 10)
    };
    if digits.is_empty() && radix != 8 {
        return None;
    }
    let mut v: u64 = 0;
    for &c in digits {
        let d = (c as char).to_digit(radix)?;
        v = v * radix as u64 + d as u64;
        if v > 0xffff_ffff {
            return None;
        }
    }
    Some(v as u32)
}

/// inet_aton: "a.b.c.d", "a.b.c" (c 16 bits), "a.b" (b 24 bits), "a";
/// decimal, octal (0...) or hex (0x...) parts.
pub fn aton(s: &[u8]) -> Option<[u8; 4]> {
    let s = s.trim_ascii();
    if s.is_empty() || s.last() == Some(&b'.') {
        return None;
    }
    let mut parts = [0u32; 4];
    let mut n = 0;
    for p in s.split(|&c| c == b'.') {
        if n == 4 {
            return None;
        }
        parts[n] = number(p)?;
        n += 1;
    }
    let v: u32 = match n {
        1 => parts[0],
        2 if parts[0] <= 0xff && parts[1] <= 0xff_ffff => parts[0] << 24 | parts[1],
        3 if parts[0] <= 0xff && parts[1] <= 0xff && parts[2] <= 0xffff => parts[0] << 24 | parts[1] << 16 | parts[2],
        4 if parts.iter().all(|&p| p <= 0xff) => parts[0] << 24 | parts[1] << 16 | parts[2] << 8 | parts[3],
        _ => return None,
    };
    Some(v.to_be_bytes())
}

pub unsafe fn inet_addr(s: *const u8) -> u32 {
    match aton(unsafe { crate::cstr::bytes(s) }) {
        Some(a) => net_u32(a),
        None => INADDR_NONE,
    }
}

pub unsafe fn inet_aton(s: *const u8, out: *mut u32) -> i32 {
    match aton(unsafe { crate::cstr::bytes(s) }) {
        Some(a) => {
            if !out.is_null() {
                unsafe { out.write_unaligned(net_u32(a)) };
            }
            1
        }
        None => 0,
    }
}

/// Dotted-quad text.
pub fn ntoa_into(a: [u8; 4], out: &mut [u8]) -> usize {
    let mut n = 0;
    for (i, b) in a.iter().enumerate() {
        if i > 0 {
            out[n] = b'.';
            n += 1;
        }
        let mut d = [0u8; 3];
        let mut k = 0;
        let mut v = *b;
        loop {
            d[k] = b'0' + v % 10;
            k += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            out[n] = d[k];
            n += 1;
        }
    }
    out[n] = 0;
    n
}

pub fn ntoa_vec(a: [u8; 4]) -> alloc::vec::Vec<u8> {
    let mut b = [0u8; 20];
    let n = ntoa_into(a, &mut b);
    b[..n].to_vec()
}

/// inet_ntoa: a per-thread buffer.
pub fn inet_ntoa(a: u32) -> *const u8 {
    let t = tls::get();
    ntoa_into(net_bytes(a), &mut t.ntoa);
    t.ntoa.as_ptr()
}

/// inet_network: the network number in host order.
pub unsafe fn inet_network(s: *const u8) -> u32 {
    let s = unsafe { crate::cstr::bytes(s) }.trim_ascii();
    let mut v: u32 = 0;
    let mut n = 0;
    for p in s.split(|&c| c == b'.') {
        let Some(x) = number(p) else { return INADDR_NONE };
        if n == 4 || x > 0xff && n > 0 {
            return INADDR_NONE;
        }
        v = if n == 0 { x } else { v << 8 | x };
        n += 1;
    }
    if n == 0 { INADDR_NONE } else { v }
}

fn class_shift(host_order: u32) -> u32 {
    if host_order & 0x8000_0000 == 0 {
        24
    } else if host_order & 0xc000_0000 == 0x8000_0000 {
        16
    } else {
        8
    }
}

/// inet_lnaof: the host part (host order).
pub fn inet_lnaof(a: u32) -> u32 {
    let h = u32::from_be_bytes(net_bytes(a));
    h & ((1u32 << class_shift(h)) - 1)
}

/// inet_netof: the network part (host order).
pub fn inet_netof(a: u32) -> u32 {
    let h = u32::from_be_bytes(net_bytes(a));
    h >> class_shift(h)
}

/// inet_makeaddr(net, host) (both host order): the address in network
/// order.
pub fn inet_makeaddr(net: u32, host: u32) -> u32 {
    let h = if net < 128 {
        net << 24 | host & 0xff_ffff
    } else if net < 65536 {
        net << 16 | host & 0xffff
    } else if net < 0x100_0000 {
        net << 8 | host & 0xff
    } else {
        net | host
    };
    net_u32(h.to_be_bytes())
}
