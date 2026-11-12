//! PrtQuery and PrtSet (PMSPL 603 and 604): questions to and settings of
//! a printer, which IBM's spooler passes to the printer's port driver
//! (bidirectional printers). IBM's Workplace Shell uses them for the
//! Print options page's input bin of the separator page (studied in
//! WPPRINT's InitInputBinComboBox and SetSepFileInputBin):
//!
//! - PrtQuery(pszComputerName, pszDeviceName, ulFlags, ulCommand,
//!   pInData, cbInData, pOutData, pcbOutData) with command 0xA017: the
//!   printer's input bins: ULONG cb, ULONG cBins, then cBins records of
//!   96 bytes with the bin's name at offset 32 (rs2's records: ULONG
//!   index, ULONG flags (1: the bin the queue's separator page uses),
//!   then the name);
//! - PrtSet(pszComputerName, pszDeviceName, ulFlags, ulCommand, pInData,
//!   cbInData) with command 0xA018 and one of those records: the
//!   separator page comes from that bin.
//!
//! The device is a queue (or a printer, for its first queue). rs2's
//! PostScript printers have two bins: the printer's own choice and
//! manual feed; a PostScript separator page from manual feed asks for it
//! (`/ManualFeed true` in setpagedevice, crate::sep's caller).

use alloc::vec::Vec;

use crate::api::{ERROR_INVALID_PARAMETER, NERR_QNOTFOUND};
use crate::ini::{Printer, Queue};
use crate::util::{cstr, put};

pub const BIDI_Q_INPUTBINS: u32 = 0xA017;
pub const BIDI_SET_INPUTBIN: u32 = 0xA018;
const ERROR_NOT_SUPPORTED: u32 = 50;
const NERR_BUFTOOSMALL: u32 = 2123;
const BIN_RECORD: usize = 96;
const BIN_NAME: usize = 32;

/// The input bins of rs2's PostScript printers (the first is the
/// default: no request in the separator page).
pub const BINS: [&[u8]; 2] = [b"Automatic selection", b"Manual feed"];

fn queue(dev: *const u8) -> Option<Queue> {
    let name = cstr(dev);
    Queue::load(name).or_else(|| Printer::load(name).and_then(|p| p.queues.first().and_then(|q| Queue::load(q))))
}

/// PrtQuery(pszComputerName, pszDeviceName, ulFlags, ulCommand, pInData,
/// cbInData, pOutData, pcbOutData)
#[unsafe(no_mangle)]
pub extern "C" fn PrtQuery(_comp: *const u8, dev: *const u8, _fl: u32, cmd: u32, _pin: *const u8, _cbin: u32, out: *mut u8, pcb: *mut u32) -> u32 {
    if cmd != BIDI_Q_INPUTBINS {
        return ERROR_NOT_SUPPORTED;
    }
    let Some(q) = crate::jobs::locked(|| queue(dev)) else { return NERR_QNOTFOUND };
    let mut d = Vec::new();
    d.extend_from_slice(&((8 + BINS.len() * BIN_RECORD) as u32).to_le_bytes());
    d.extend_from_slice(&(BINS.len() as u32).to_le_bytes());
    for (i, b) in BINS.iter().enumerate() {
        let mut r = [0u8; BIN_RECORD];
        r[..4].copy_from_slice(&(i as u32).to_le_bytes());
        let current = if q.sep_bin.is_empty() { i == 0 } else { q.sep_bin.eq_ignore_ascii_case(b) };
        r[4] = current as u8;
        r[BIN_NAME..BIN_NAME + b.len()].copy_from_slice(b);
        d.extend_from_slice(&r);
    }
    if pcb.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let cb = unsafe { pcb.read_unaligned() } as usize;
    put(pcb, d.len() as u32);
    if out.is_null() || cb < d.len() {
        return NERR_BUFTOOSMALL;
    }
    unsafe { core::ptr::copy_nonoverlapping(d.as_ptr(), out, d.len()) };
    0
}

/// PrtSet(pszComputerName, pszDeviceName, ulFlags, ulCommand, pInData,
/// cbInData)
#[unsafe(no_mangle)]
pub extern "C" fn PrtSet(_comp: *const u8, dev: *const u8, _fl: u32, cmd: u32, data: *const u8, cb: u32) -> u32 {
    if cmd != BIDI_SET_INPUTBIN {
        return ERROR_NOT_SUPPORTED;
    }
    if data.is_null() || (cb as usize) < BIN_NAME + 1 {
        return ERROR_INVALID_PARAMETER;
    }
    let rec = unsafe { core::slice::from_raw_parts(data, (cb as usize).min(BIN_RECORD)) };
    let name: Vec<u8> = rec[BIN_NAME..].iter().copied().take_while(|&c| c != 0).collect();
    let Some(i) = BINS.iter().position(|b| b.eq_ignore_ascii_case(&name)) else { return ERROR_INVALID_PARAMETER };
    crate::jobs::locked(|| {
        let Some(mut q) = queue(dev) else { return NERR_QNOTFOUND };
        q.sep_bin = if i == 0 { Vec::new() } else { BINS[i].to_vec() };
        q.save();
        0
    })
}

/// A PostScript separator page asks for the queue's separator bin.
pub fn separator_for_bin(q: &Queue, sep: Vec<u8>) -> Vec<u8> {
    if q.sep_bin.is_empty() || !sep.starts_with(b"%!") {
        return sep;
    }
    let eol = sep.iter().position(|&c| c == b'\n').map_or(sep.len(), |p| p + 1);
    let mut d = sep[..eol].to_vec();
    if eol == sep.len() && !d.ends_with(b"\n") {
        d.push(b'\n');
    }
    if q.sep_bin.eq_ignore_ascii_case(BINS[1]) {
        d.extend_from_slice(b"<< /ManualFeed true >> setpagedevice\n");
    }
    d.extend_from_slice(&sep[eol..]);
    d
}
