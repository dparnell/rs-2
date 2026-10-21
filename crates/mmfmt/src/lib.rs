//! Multimedia file formats shared by rs2's MMIO.DLL and MDM.DLL, with no
//! OS dependencies (host-tested):
//!
//! - [`riff`]: RIFF chunk headers and walking the chunks of a form;
//! - [`wav`]: RIFF WAVE files (the format chunk and the sample data), and
//!   decoding µ-law, A-law and IMA ADPCM samples to 16-bit PCM;
//! - [`flc`]: Autodesk FLI/FLC animations, decoded frame by frame into an
//!   8-bit indexed frame buffer and a 256-colour palette.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod flc;
pub mod riff;
pub mod wav;

/// Little-endian reads that fail instead of panicking past the end.
pub(crate) fn le16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

pub(crate) fn le32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
