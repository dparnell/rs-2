//! RS2API 630-635: the system keychain (`crate::keychain`) for
//! KEYCHAIN.EXE and other programs. Calls return 0 or an OS/2 error;
//! text comes back as in `api::wlan`. Strings are PSZs, one byte per
//! character; a secret is (pointer, length).
//!
//! | Ord | Entry |
//! |-----|-------|
//! | 630 | Rs2KeyQuery(PCHAR buf, ULONG cb, PULONG pcb): `key=value` lines (`keychain::status_text`, no secrets) |
//! | 631 | Rs2KeySet(PSZ service, PSZ account, PVOID secret, ULONG cbSecret, ULONG flags): store or replace; flags bit 0: programs may read it back |
//! | 632 | Rs2KeyGet(PSZ service, PSZ account, PVOID buf, ULONG cb, PULONG pcb): a readable entry's secret (system-only entries: ERROR_ACCESS_DENIED) |
//! | 633 | Rs2KeyDelete(PSZ service, PSZ account) |
//! | 634 | Rs2KeyUnlock(PSZ password): give the master password; NULL locks the keychain again |
//! | 635 | Rs2KeySetPassword(PSZ old or NULL, PSZ new or NULL): set, change or remove the master password |
//!
//! Errors: ERROR_FILE_NOT_FOUND (no such entry), ERROR_ACCESS_DENIED
//! (system-only entry), ERROR_NOT_READY (locked), ERROR_INVALID_PASSWORD,
//! ERROR_GEN_FAILURE (damaged file), ERROR_BUFFER_OVERFLOW (`*pcb` = size
//! needed), or the file system's error when it cannot be saved.

use super::{err, Ctx};
use crate::keychain::{self, KcError};
use alloc::string::String;
use alloc::vec::Vec;

/// ERROR_INVALID_PASSWORD.
const INVALID_PASSWORD: u32 = 86;
/// ERROR_GEN_FAILURE.
const GEN_FAILURE: u32 = 31;

fn code(e: KcError) -> u32 {
    match e {
        KcError::Locked => err::NOT_READY,
        KcError::BadPassword => INVALID_PASSWORD,
        KcError::Damaged => GEN_FAILURE,
        KcError::NotFound => err::FILE_NOT_FOUND,
        KcError::NotReadable => err::ACCESS_DENIED,
        KcError::TooLong => err::INVALID_PARAMETER,
        KcError::Io(e) => e,
    }
}

fn bytes(s: String) -> Vec<u8> {
    s.chars().map(|ch| ch as u32 as u8).collect()
}

/// A PSZ argument; `None` for NULL. Err: a bad address.
fn opt_str(c: &Ctx, at: u32, max: usize) -> Result<Option<Vec<u8>>, u32> {
    match at {
        0 => Ok(None),
        a => c.read_str(a, max).map(|s| Some(bytes(s))).ok_or(err::INVALID_ADDRESS),
    }
}

/// (service, account) from arguments 0 and 1.
fn names(c: &Ctx) -> Result<(String, Vec<u8>), u32> {
    let s = c.read_str(c.arg(0), 256).ok_or(err::INVALID_ADDRESS)?;
    let a = opt_str(c, c.arg(1), 256)?.unwrap_or_default();
    if s.is_empty() || s.len() > 255 || a.len() > 255 {
        return Err(err::INVALID_PARAMETER);
    }
    Ok((s, a))
}

/// Copy `data` to (buf, cb, pcb) at arguments i, i+1, i+2.
fn out(c: &Ctx, data: &[u8], i: usize) -> u32 {
    let (buf, cb, pcb) = (c.arg(i), c.arg(i + 1), c.arg(i + 2));
    if pcb != 0 {
        c.write_u32(pcb, data.len() as u32);
    }
    if data.len() > cb as usize {
        return err::BUFFER_OVERFLOW;
    }
    if c.write(buf, data) { 0 } else { err::INVALID_ADDRESS }
}

pub fn query(c: &mut Ctx) -> u32 {
    let t: Vec<u8> = keychain::status_text().chars().map(|ch| ch as u32 as u8).chain(core::iter::once(0)).collect();
    out(c, &t, 0)
}

pub fn set(c: &mut Ctx) -> u32 {
    let (s, a) = match names(c) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (p, n, flags) = (c.arg(2), c.arg(3), c.arg(4));
    if n > 0xffff {
        return err::INVALID_PARAMETER;
    }
    let Some(mut secret) = c.read_vec(p, n) else { return err::INVALID_ADDRESS };
    let r = keychain::set(&s, &a, &secret, flags & 1 != 0);
    secret.iter_mut().for_each(|b| *b = 0);
    r.err().map_or(0, code)
}

pub fn get(c: &mut Ctx) -> u32 {
    let (s, a) = match names(c) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match keychain::get_readable(&s, &a) {
        Ok(mut v) => {
            let rc = out(c, &v, 2);
            v.iter_mut().for_each(|b| *b = 0);
            rc
        }
        Err(e) => code(e),
    }
}

pub fn delete(c: &mut Ctx) -> u32 {
    match names(c) {
        Ok((s, a)) => keychain::remove(&s, &a).err().map_or(0, code),
        Err(e) => e,
    }
}

pub fn unlock(c: &mut Ctx) -> u32 {
    let r = match opt_str(c, c.arg(0), 256) {
        Ok(Some(p)) => keychain::unlock(&p),
        Ok(None) => keychain::lock(),
        Err(e) => return e,
    };
    r.err().map_or(0, code)
}

pub fn set_password(c: &mut Ctx) -> u32 {
    let (old, new) = match (opt_str(c, c.arg(0), 256), opt_str(c, c.arg(1), 256)) {
        (Ok(o), Ok(n)) => (o.filter(|v| !v.is_empty()), n.filter(|v| !v.is_empty())),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    keychain::set_password(old.as_deref(), new.as_deref()).err().map_or(0, code)
}
