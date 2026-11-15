//! Crash reports as QR codes, for machines without a serial port.
//!
//! The kernel's panic screen packs its report into one QR code:
//! `RS2C1:` + base45(zlib(report text)). Base45 (RFC 9285) uses exactly
//! the 45 characters of the QR alphanumeric mode, which stores them at
//! 5.5 bits each, so the compressed bytes cost 8.25 bits each in the code
//! (byte mode would be 8, but many phone scanners mangle binary data).
//! `tools/scripts/crashqr.py` turns the scanned text back into the report.
//!
//! Everything works on caller-supplied buffers: the panic path cannot
//! allocate (the heap may be what broke).

#![cfg_attr(not(test), no_std)]

pub mod base45;
pub mod deflate;
pub mod qr;

/// What every payload starts with (format version 1).
pub const PREFIX: &[u8] = b"RS2C1:";

/// Pack `report` into `out` as `PREFIX` + base45(zlib(report)); `tmp`
/// holds the compressed bytes. The length, or None if `out` or `tmp` is
/// too small.
pub fn pack(z: &mut deflate::Deflate, report: &[u8], tmp: &mut [u8], out: &mut [u8]) -> Option<usize> {
    let n = z.zlib(report, tmp)?;
    if out.len() < PREFIX.len() {
        return None;
    }
    out[..PREFIX.len()].copy_from_slice(PREFIX);
    let m = base45::encode(&tmp[..n], &mut out[PREFIX.len()..])?;
    Some(PREFIX.len() + m)
}
