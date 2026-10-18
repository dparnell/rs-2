//! Parser for OS/2 executable modules.
//!
//! Supports the DOS `MZ` stub, the 32-bit linear executable format (`LX`)
//! used by OS/2 2.x+ and the 16-bit segmented format (`NE`) used by OS/2 1.x
//! modules that are still shipped with Warp 4.
//!
//! The crate is `no_std` + `alloc` so the kernel loader and host tools share
//! exactly the same code.

#![no_std]

extern crate alloc;

pub mod lx;
pub mod ne;
mod read;

pub use read::Error;

/// Which executable format a file contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Plain DOS executable (no new header).
    Mz,
    /// 16-bit segmented executable.
    Ne { header: usize },
    /// 32-bit linear executable.
    Lx { header: usize },
    /// Linear executable (VxD style). Not used for OS/2 applications.
    Le { header: usize },
}

/// Identify the format of an executable image.
pub fn detect(data: &[u8]) -> Result<Format, Error> {
    let r = read::Reader::new(data);
    let sig = r.u16(0)?;
    if sig == u16::from_le_bytes(*b"LX") {
        return Ok(Format::Lx { header: 0 });
    }
    if sig == u16::from_le_bytes(*b"NE") {
        return Ok(Format::Ne { header: 0 });
    }
    if sig != u16::from_le_bytes(*b"MZ") {
        return Err(Error::BadMagic);
    }
    // New-style executables have e_lfarlc >= 0x40 and a pointer at 0x3C.
    let lfanew = r.u32(0x3c).unwrap_or(0) as usize;
    if lfanew == 0 || lfanew + 2 > data.len() {
        return Ok(Format::Mz);
    }
    match &data[lfanew..lfanew + 2] {
        b"LX" => Ok(Format::Lx { header: lfanew }),
        b"LE" => Ok(Format::Le { header: lfanew }),
        b"NE" => Ok(Format::Ne { header: lfanew }),
        _ => Ok(Format::Mz),
    }
}
