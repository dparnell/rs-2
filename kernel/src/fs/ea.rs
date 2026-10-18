//! Extended attribute lists as the API exchanges them (bsedos.h).
//!
//! 32-bit calls use FEA2LIST/GEA2LIST: entries start on 4-byte boundaries
//! and are chained by `oNextEntryOffset` (0 in the last one). 16-bit calls
//! use the OS/2 1.x FEALIST/GEALIST, whose entries follow each other
//! without padding. An FEA is flags (0x80 = FEA_NEEDEA), name length,
//! 16-bit value length, the name, a NUL and the value; a GEA is a name
//! length and the name with its NUL. Both lists start with cbList, the
//! size of the whole list.
//!
//! EA names are stored upper-cased (case-insensitive, as on OS/2); a value
//! of length 0 in a set request deletes the EA.

use alloc::vec::Vec;

use crate::api::err;

pub const FEA_NEEDEA: u8 = 0x80;
/// Largest EA set of one file: its FEALIST must fit in 64 KB.
pub const MAX_FEALIST: usize = 0xffff;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ea {
    pub flags: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// Characters a FAT file name may not contain are not allowed in EA
/// names either (wildcards included).
fn legal(c: u8) -> bool {
    c >= 0x20 && !b"\"*+,/:;<=>?[\\]|".contains(&c)
}

pub fn valid_name(n: &[u8]) -> bool {
    !n.is_empty() && n.len() <= 255 && n.iter().all(|&c| legal(c))
}

pub fn upper(n: &[u8]) -> Vec<u8> {
    n.iter().map(|c| c.to_ascii_uppercase()).collect()
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn rd16(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}

fn rd32(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize
}

/// Size of the FEA2LIST describing `eas`.
pub fn fea2_size(eas: &[Ea]) -> u32 {
    (4 + eas.iter().map(|e| align4(9 + e.name.len() + e.value.len())).sum::<usize>()) as u32
}

/// Size of the (16-bit) FEALIST describing `eas`.
pub fn fea_size(eas: &[Ea]) -> u32 {
    (4 + eas.iter().map(|e| 5 + e.name.len() + e.value.len()).sum::<usize>()) as u32
}

/// cbList as reported by the EA-size info levels for a 16- or 32-bit
/// caller (4 for no EAs).
pub fn list_size(eas: &[Ea], is16: bool) -> u32 {
    if is16 { fea_size(eas) } else { fea2_size(eas) }
}

/// Encode an FEA2LIST (or, for `with_values` false, a DENA2 list).
pub fn encode_fea2(eas: &[Ea]) -> Vec<u8> {
    let mut b = Vec::with_capacity(fea2_size(eas) as usize);
    b.extend_from_slice(&fea2_size(eas).to_le_bytes());
    for (i, e) in eas.iter().enumerate() {
        let len = align4(9 + e.name.len() + e.value.len());
        let next = if i + 1 == eas.len() { 0 } else { len as u32 };
        let start = b.len();
        b.extend_from_slice(&next.to_le_bytes());
        b.push(e.flags);
        b.push(e.name.len() as u8);
        b.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
        b.extend_from_slice(&e.name);
        b.push(0);
        b.extend_from_slice(&e.value);
        b.resize(start + len, 0);
    }
    b
}

/// Encode a 16-bit FEALIST.
pub fn encode_fea(eas: &[Ea]) -> Vec<u8> {
    let mut b = Vec::with_capacity(fea_size(eas) as usize);
    b.extend_from_slice(&fea_size(eas).to_le_bytes());
    for e in eas {
        b.push(e.flags);
        b.push(e.name.len() as u8);
        b.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
        b.extend_from_slice(&e.name);
        b.push(0);
        b.extend_from_slice(&e.value);
    }
    b
}

pub fn encode(eas: &[Ea], is16: bool) -> Vec<u8> {
    if is16 { encode_fea(eas) } else { encode_fea2(eas) }
}

/// A list the caller passed is malformed: the error and the offset of the
/// offending entry (for EAOP2.oError).
#[derive(Clone, Copy, Debug)]
pub struct ListError {
    pub code: u32,
    pub offset: u32,
}

fn bad(code: u32, offset: usize) -> ListError {
    ListError { code, offset: offset as u32 }
}

/// Parse an FEA2LIST (`b` holds cbList bytes). Names are validated and
/// upper-cased.
pub fn parse_fea2(b: &[u8]) -> Result<Vec<Ea>, ListError> {
    let mut v = Vec::new();
    if b.len() <= 4 {
        return Ok(v);
    }
    let mut i = 4;
    loop {
        if i + 8 > b.len() {
            return Err(bad(err::EA_LIST_INCONSISTENT, i));
        }
        let next = rd32(b, i);
        let nl = b[i + 5] as usize;
        let vl = rd16(b, i + 6);
        let name = i + 8;
        let value = name + nl + 1;
        if value + vl > b.len() || (next != 0 && next < value + vl - i) {
            return Err(bad(err::EA_LIST_INCONSISTENT, i));
        }
        let n = &b[name..name + nl];
        if !valid_name(n) {
            return Err(bad(err::INVALID_EA_NAME, i));
        }
        v.push(Ea { flags: b[i + 4] & FEA_NEEDEA, name: upper(n), value: b[value..value + vl].to_vec() });
        if next == 0 {
            return Ok(v);
        }
        i += next;
    }
}

/// Parse a 16-bit FEALIST.
pub fn parse_fea(b: &[u8]) -> Result<Vec<Ea>, ListError> {
    let mut v = Vec::new();
    let mut i = 4;
    while i < b.len() {
        if i + 4 > b.len() {
            return Err(bad(err::EA_LIST_INCONSISTENT, i));
        }
        let nl = b[i + 1] as usize;
        let vl = rd16(b, i + 2);
        let name = i + 4;
        let value = name + nl + 1;
        if value + vl > b.len() {
            return Err(bad(err::EA_LIST_INCONSISTENT, i));
        }
        let n = &b[name..name + nl];
        if !valid_name(n) {
            return Err(bad(err::INVALID_EA_NAME, i));
        }
        v.push(Ea { flags: b[i] & FEA_NEEDEA, name: upper(n), value: b[value..value + vl].to_vec() });
        i = value + vl;
    }
    Ok(v)
}

pub fn parse(b: &[u8], is16: bool) -> Result<Vec<Ea>, ListError> {
    if is16 { parse_fea(b) } else { parse_fea2(b) }
}

/// Parse a GEA2LIST (32-bit) or GEALIST (16-bit): the requested names.
pub fn parse_gea(b: &[u8], is16: bool) -> Result<Vec<Vec<u8>>, ListError> {
    let mut v = Vec::new();
    let mut i = 4;
    if is16 {
        while i < b.len() {
            let nl = b[i] as usize;
            if i + 1 + nl > b.len() {
                return Err(bad(err::EA_LIST_INCONSISTENT, i));
            }
            let n = &b[i + 1..i + 1 + nl];
            if !valid_name(n) {
                return Err(bad(err::INVALID_EA_NAME, i));
            }
            v.push(upper(n));
            // The NUL after the name is optional at the very end.
            i += 2 + nl;
        }
        return Ok(v);
    }
    if b.len() <= 4 {
        return Ok(v);
    }
    loop {
        if i + 5 > b.len() {
            return Err(bad(err::EA_LIST_INCONSISTENT, i));
        }
        let next = rd32(b, i);
        let nl = b[i + 4] as usize;
        if i + 5 + nl > b.len() {
            return Err(bad(err::EA_LIST_INCONSISTENT, i));
        }
        let n = &b[i + 5..i + 5 + nl];
        if !valid_name(n) {
            return Err(bad(err::INVALID_EA_NAME, i));
        }
        v.push(upper(n));
        if next == 0 {
            return Ok(v);
        }
        if next < 5 + nl {
            return Err(bad(err::EA_LIST_INCONSISTENT, i));
        }
        i += next;
    }
}

/// The requested EAs in request order; names a file does not have come
/// back with an empty value, as on OS/2.
pub fn select(eas: &[Ea], names: &[Vec<u8>]) -> Vec<Ea> {
    names
        .iter()
        .map(|n| match eas.iter().find(|e| same(&e.name, n)) {
            Some(e) => e.clone(),
            None => Ea { flags: 0, name: n.clone(), value: Vec::new() },
        })
        .collect()
}

/// Apply set requests to an EA set: each EA is added or replaced, a
/// zero-length value removes it. Fails if the result exceeds 64 KB.
pub fn merge(mut eas: Vec<Ea>, changes: &[Ea]) -> Result<Vec<Ea>, u32> {
    for c in changes {
        let at = eas.iter().position(|e| same(&e.name, &c.name));
        match (at, c.value.is_empty()) {
            (Some(i), true) => {
                eas.remove(i);
            }
            (Some(i), false) => eas[i] = c.clone(),
            (None, true) => {}
            (None, false) => eas.push(c.clone()),
        }
    }
    if fea_size(&eas) as usize > MAX_FEALIST {
        return Err(err::EA_LIST_TOO_LONG);
    }
    Ok(eas)
}

/// DosEnumAttribute level 1 records for `eas` (DENA2 for 32-bit callers:
/// FEA2 without the value; DENA1 for 16-bit ones), as many as fit in
/// `cb` bytes: (bytes, count).
pub fn encode_dena(eas: &[Ea], is16: bool, cb: usize) -> (Vec<u8>, usize) {
    let mut b = Vec::new();
    let mut n = 0;
    let mut last: Option<usize> = None;
    for e in eas {
        let start = b.len();
        if is16 {
            if start + 5 + e.name.len() > cb {
                break;
            }
            b.push(0);
            b.push(e.name.len() as u8);
            b.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
            b.extend_from_slice(&e.name);
            b.push(0);
        } else {
            let len = align4(9 + e.name.len());
            // The last record need not be padded.
            if start + 9 + e.name.len() > cb {
                break;
            }
            if let Some(l) = last {
                let d = (start - l) as u32;
                b[l..l + 4].copy_from_slice(&d.to_le_bytes());
            }
            last = Some(start);
            b.extend_from_slice(&0u32.to_le_bytes());
            b.push(e.flags);
            b.push(e.name.len() as u8);
            b.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
            b.extend_from_slice(&e.name);
            b.push(0);
            b.resize((start + len).min(cb.max(start + 9 + e.name.len())), 0);
        }
        n += 1;
    }
    (b, n)
}
