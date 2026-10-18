//! NLS: country information, case mapping, collation, DBCS ranges and
//! code page APIs (the tables live in `crate::nls`).

use super::{err, Ctx};
use crate::nls;

/// Read a COUNTRYCODE (country, code page); 0 means the current value.
fn country_code(c: &Ctx, p: u32) -> Result<(u16, u16), u32> {
    let (country, cp) = if c.is16 {
        let (Some(a), Some(b)) = (c.read_u16(p), c.read_u16(p + 2)) else { return Err(err::INVALID_ADDRESS) };
        (a as u32, b as u32)
    } else {
        let (Some(a), Some(b)) = (c.read_u32(p), c.read_u32(p + 4)) else { return Err(err::INVALID_ADDRESS) };
        (a, b)
    };
    nls::resolve(c.process.pid, country, cp)
}

fn write_count(c: &Ctx, pcb: u32, n: u32) {
    if c.is16 {
        c.write_u16(pcb, n as u16);
    } else {
        c.write_u32(pcb, n);
    }
}

/// DosQueryCtryInfo / DosGetCtryInfo (cb, pctryc, pctryi, pcbActual).
pub fn dos_get_ctry_info(c: &mut Ctx) -> u32 {
    let (cb, pcc, pci, pcb) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    let (country, cp) = match country_code(c, pcc) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let b = nls::country_info(country, cp, c.is16);
    let n = b.len().min(cb as usize);
    if !c.write(pci, &b[..n]) {
        return err::INVALID_ADDRESS;
    }
    write_count(c, pcb, n as u32);
    if n < b.len() { err::BUFFER_OVERFLOW } else { 0 }
}

/// DosMapCase / DosCaseMap (cb, pctryc, pch).
pub fn dos_case_map(c: &mut Ctx) -> u32 {
    let (len, pcc, p) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let (country, cp) = match country_code(c, pcc) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(mut v) = c.read_vec(p, len) else { return err::INVALID_ADDRESS };
    let map = nls::case_map(country, cp);
    let lead = nls::dbcs_ranges(cp);
    let mut i = 0;
    while i < v.len() {
        let b = v[i];
        if lead.iter().any(|&(lo, hi)| (lo..=hi).contains(&b)) {
            // Double-byte characters are left alone.
            i += 2;
            continue;
        }
        v[i] = map[b as usize];
        i += 1;
    }
    c.write(p, &v);
    0
}

/// DosQueryCollate / DosGetCollate (cb, pctryc, pch, pcch).
pub fn dos_get_collate(c: &mut Ctx) -> u32 {
    let (cb, pcc, p, pcb) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    let (country, cp) = match country_code(c, pcc) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let table = nls::collate(country, cp);
    let n = table.len().min(cb as usize);
    if !c.write(p, &table[..n]) {
        return err::INVALID_ADDRESS;
    }
    write_count(c, pcb, n as u32);
    if n < table.len() { err::BUFFER_OVERFLOW } else { 0 }
}

/// DosQueryDBCSEnv / DosGetDBCSEv (cb, pctryc, pch): pairs of lead byte
/// ranges ending with 0,0.
pub fn dos_get_dbcs_env(c: &mut Ctx) -> u32 {
    let (cb, pcc, p) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let (_, cp) = match country_code(c, pcc) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut b = alloc::vec::Vec::new();
    for &(lo, hi) in nls::dbcs_ranges(cp) {
        b.extend_from_slice(&[lo, hi]);
    }
    b.extend_from_slice(&[0, 0]);
    let n = b.len().min(cb as usize);
    if !c.write(p, &b[..n]) {
        return err::INVALID_ADDRESS;
    }
    if n < b.len() { err::BUFFER_OVERFLOW } else { 0 }
}

/// The DosQueryCp list: the process's code page, then the prepared ones.
fn cp_list(c: &Ctx) -> alloc::vec::Vec<u16> {
    let mut v = alloc::vec![nls::process_cp(c.process.pid)];
    v.extend(nls::prepared().iter().copied().filter(|&cp| cp != 0));
    v
}

/// DosQueryCp (cb, arCP, pcCP): ULONG entries; the count is in bytes.
pub fn dos_query_cp(c: &mut Ctx) -> u32 {
    let (cb, buf, pcb) = (c.arg(0), c.arg(1), c.arg(2));
    let cps = cp_list(c);
    let n = (cb as usize / 4).min(cps.len());
    for (i, cp) in cps.iter().take(n).enumerate() {
        c.write_u32(buf + 4 * i as u32, *cp as u32);
    }
    c.write_u32(pcb, (n * 4) as u32);
    if n < cps.len() { err::BUFFER_OVERFLOW } else { 0 }
}

/// 16-bit DosGetCp (cb, pCodePageList, pcbCodePageList): USHORT entries.
pub fn dos16_get_cp(c: &mut Ctx) -> u32 {
    let (cb, p, pcb) = (c.arg(0), c.ptr(1), c.ptr(2));
    let cps = cp_list(c);
    let n = (cb as usize / 2).min(cps.len());
    for (i, cp) in cps.iter().take(n).enumerate() {
        c.write_u16(p + 2 * i as u32, *cp);
    }
    c.write_u16(pcb, (n * 2) as u16);
    if n < cps.len() { err::BUFFER_OVERFLOW } else { 0 }
}

/// DosSetProcessCp / DosSetProcCp: the process code page.
pub fn dos_set_process_cp(c: &mut Ctx) -> u32 {
    let cp = c.arg(0) as u16;
    match nls::set_process_cp(c.process.pid, cp) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// DosSetCp: the process code page and the session's display and
/// keyboard code pages.
pub fn dos_set_cp(c: &mut Ctx) -> u32 {
    let cp = c.arg(0) as u16;
    if let Err(e) = nls::set_process_cp(c.process.pid, cp) {
        return e;
    }
    if let Ok(page) = nls::screen_cp(cp) {
        crate::screen::with(|s| s.set_codepage(page));
    }
    if let Ok(k) = nls::keyboard_cp(cp) {
        crate::kbd::layout::set_codepage(k);
    }
    0
}

/// KbdSetCp (usReserved, usCodePage, hkbd).
pub fn kbd_set_cp(c: &mut Ctx) -> u32 {
    match nls::keyboard_cp(c.arg(1) as u16) {
        Ok(cp) => {
            crate::kbd::layout::set_codepage(cp);
            0
        }
        Err(e) => e,
    }
}

/// KbdGetCp (ulReserved, pidCP, hkbd).
pub fn kbd_get_cp(c: &mut Ctx) -> u32 {
    if c.write_u16(c.ptr(1), crate::kbd::layout::codepage()) { 0 } else { err::INVALID_ADDRESS }
}
