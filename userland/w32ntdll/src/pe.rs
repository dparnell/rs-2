//! Reading a loaded PE image's headers in memory (the loader maps the
//! headers at the image base). W32KERN uses this file too.

#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Default)]
pub struct Tls {
    pub start: u32,
    pub end: u32,
    pub index: u32,
    pub callbacks: u32,
    pub zero_fill: u32,
}

unsafe fn rd32(a: u32) -> u32 {
    unsafe { (a as *const u32).read_unaligned() }
}

unsafe fn rd16(a: u32) -> u16 {
    unsafe { (a as *const u16).read_unaligned() }
}

/// The optional header's address, if `base` holds a PE image.
pub unsafe fn optional(base: u32) -> Option<u32> {
    unsafe {
        if base == 0 || rd16(base) != 0x5a4d {
            return None;
        }
        let pe = base + rd32(base + 0x3c);
        if rd32(pe) != 0x4550 {
            return None;
        }
        Some(pe + 24)
    }
}

/// A data directory (RVA, size).
pub unsafe fn dir(base: u32, i: u32) -> Option<(u32, u32)> {
    unsafe {
        let o = optional(base)?;
        if i >= rd32(o + 92) {
            return None;
        }
        let rva = rd32(o + 96 + i * 8);
        let size = rd32(o + 100 + i * 8);
        (rva != 0 && size != 0).then_some((rva, size))
    }
}

pub unsafe fn entry(base: u32) -> u32 {
    unsafe {
        match optional(base) {
            Some(o) if rd32(o + 16) != 0 => base + rd32(o + 16),
            _ => 0,
        }
    }
}

pub unsafe fn image_size(base: u32) -> u32 {
    unsafe { optional(base).map_or(0, |o| rd32(o + 56)) }
}

pub unsafe fn subsystem(base: u32) -> u16 {
    unsafe { optional(base).map_or(0, |o| rd16(o + 68)) }
}

/// The TLS directory (virtual addresses, relocated).
pub unsafe fn tls(base: u32) -> Option<Tls> {
    unsafe {
        let (rva, _) = dir(base, 9)?;
        let a = base + rva;
        Some(Tls { start: rd32(a), end: rd32(a + 4), index: rd32(a + 8), callbacks: rd32(a + 12), zero_fill: rd32(a + 16) })
    }
}

fn cstr_eq(a: u32, name: &[u8]) -> bool {
    for (i, &c) in name.iter().enumerate() {
        if unsafe { *((a + i as u32) as *const u8) } != c {
            return false;
        }
    }
    unsafe { *((a + name.len() as u32) as *const u8) == 0 }
}

/// An export by name (without its NUL) or ordinal: its address, or a
/// forwarder string's address with `true`.
pub unsafe fn export(base: u32, name: Option<&[u8]>, ordinal: u32) -> Option<(u32, bool)> {
    unsafe {
        let (rva, size) = dir(base, 0)?;
        let e = base + rva;
        let ord_base = rd32(e + 16);
        let nfuncs = rd32(e + 20);
        let nnames = rd32(e + 24);
        let funcs = base + rd32(e + 28);
        let names = base + rd32(e + 32);
        let ords = base + rd32(e + 36);
        let index = match name {
            Some(n) => {
                let mut found = None;
                for i in 0..nnames {
                    if cstr_eq(base + rd32(names + i * 4), n) {
                        found = Some(rd16(ords + i * 2) as u32);
                        break;
                    }
                }
                found?
            }
            None => ordinal.checked_sub(ord_base)?,
        };
        if index >= nfuncs {
            return None;
        }
        let f = rd32(funcs + index * 4);
        if f == 0 {
            return None;
        }
        Some((base + f, f >= rva && f < rva + size))
    }
}

/// The resource directory's address.
pub unsafe fn resources(base: u32) -> Option<u32> {
    unsafe { dir(base, 2).map(|(rva, _)| base + rva) }
}
