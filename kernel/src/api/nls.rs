//! NLS: country information, case mapping, collation, DBCS ranges.

use alloc::vec::Vec;

use super::{err, Ctx};

/// Upper case for code page 850 / 437 (code pages share the ASCII range
/// and most accented letters in 0x80-0xA5).
pub fn upper(cp: u16, b: u8) -> u8 {
    if b.is_ascii_lowercase() {
        return b - 32;
    }
    const COMMON: &[(u8, u8)] = &[
        (0x81, 0x9a), (0x82, 0x90), (0x84, 0x8e), (0x86, 0x8f), (0x87, 0x80),
        (0x91, 0x92), (0x94, 0x99), (0xa4, 0xa5),
    ];
    const CP850: &[(u8, u8)] = &[
        (0xa0, 0xb5), (0x83, 0xb6), (0x85, 0xb7), (0xc6, 0xc7), (0x88, 0xd2),
        (0x89, 0xd3), (0x8a, 0xd4), (0xa1, 0xd6), (0x8c, 0xd7), (0x8b, 0xd8),
        (0x8d, 0xde), (0xa2, 0xe0), (0x93, 0xe2), (0x95, 0xe3), (0xe4, 0xe5),
        (0xa3, 0xe9), (0x96, 0xea), (0x97, 0xeb), (0xec, 0xed), (0x9b, 0x9d),
        (0xd0, 0xd1), (0xe7, 0xe8),
    ];
    if let Some(&(_, u)) = COMMON.iter().find(|&&(l, _)| l == b) {
        return u;
    }
    if cp == 850 {
        if let Some(&(_, u)) = CP850.iter().find(|&&(l, _)| l == b) {
            return u;
        }
    }
    b
}

/// Read a COUNTRYCODE (country, code page); 0 means the current value.
fn country_code(c: &Ctx, p: u32) -> (u32, u32) {
    if c.is16 {
        (c.read_u16(p).unwrap_or(0) as u32, c.read_u16(p + 2).unwrap_or(0) as u32)
    } else {
        (c.read_u32(p).unwrap_or(0), c.read_u32(p + 4).unwrap_or(0))
    }
}

fn effective_cp(cp: u32) -> u16 {
    if cp == 0 { 850 } else { cp as u16 }
}

/// COUNTRYINFO for the United States.
fn country_info(is16: bool, country: u32, cp: u16) -> Vec<u8> {
    let mut b = Vec::new();
    let country = if country == 0 { 1 } else { country };
    if is16 {
        b.extend_from_slice(&(country as u16).to_le_bytes());
        b.extend_from_slice(&cp.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes()); // fsDateFmt: MDY
    } else {
        b.extend_from_slice(&country.to_le_bytes());
        b.extend_from_slice(&(cp as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
    }
    b.extend_from_slice(b"$\0\0\0\0"); // szCurrency
    b.extend_from_slice(b",\0"); // thousands
    b.extend_from_slice(b".\0"); // decimal
    b.extend_from_slice(b"-\0"); // date separator
    b.extend_from_slice(b":\0"); // time separator
    b.push(0); // fsCurrencyFmt
    b.push(2); // cDecimalPlace
    b.push(0); // fsTimeFmt: 12 hour
    b.extend_from_slice(&[0; 4]); // abReserved1
    b.extend_from_slice(b",\0"); // data separator
    b.extend_from_slice(&[0; 10]); // abReserved2
    b
}

pub fn dos_get_ctry_info(c: &mut Ctx) -> u32 {
    // (cb, pctryc, pctryi, pcbActual)
    let (cb, pcc, pci, pcb) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    let (country, cp) = country_code(c, pcc);
    let b = country_info(c.is16, country, effective_cp(cp));
    let n = b.len().min(cb as usize);
    c.write(pci, &b[..n]);
    if c.is16 { c.write_u16(pcb, n as u16) } else { c.write_u32(pcb, n as u32) };
    if n < b.len() { err::BUFFER_OVERFLOW } else { 0 }
}

pub fn dos_case_map(c: &mut Ctx) -> u32 {
    // (cb, pctryc, pch)
    let (len, pcc, p) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let (_, cp) = country_code(c, pcc);
    let cp = effective_cp(cp);
    let Some(mut v) = c.read_vec(p, len) else { return err::INVALID_ADDRESS };
    for b in v.iter_mut() {
        *b = upper(cp, *b);
    }
    c.write(p, &v);
    0
}

pub fn dos_get_collate(c: &mut Ctx) -> u32 {
    // (cb, pctryc, pch, pcch)
    let (cb, pcc, p, pcb) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    let (_, cp) = country_code(c, pcc);
    let cp = effective_cp(cp);
    let table: Vec<u8> = (0..=255u8).map(|b| upper(cp, b)).collect();
    let n = table.len().min(cb as usize);
    c.write(p, &table[..n]);
    if c.is16 { c.write_u16(pcb, n as u16) } else { c.write_u32(pcb, n as u32) };
    0
}

pub fn dos_get_dbcs_env(c: &mut Ctx) -> u32 {
    // (cb, pctryc, pch): pairs of lead-byte ranges, terminated by 0,0.
    let (cb, p) = if c.is16 { (c.arg(0), c.ptr(2)) } else { (c.arg(0), c.arg(2)) };
    if cb < 2 {
        return err::BUFFER_OVERFLOW;
    }
    c.write(p, &[0, 0]);
    0
}
