//! MMIOPROC.DLL: rs2's file-format I/O procedures, the ones IBM ships as
//! separate DLLs (OS13PROC, OS20PROC, WI30PROC, JPEGIO, GIFPROC, PCXPROC,
//! TIFPROC, TGAPROC, AIFFPROC, SNDBLAST, MIDIIO, AVIO). MMIO finds them
//! through MMPMMMIO.INI (mmioIniFileHandler, `MMIOINIT` `IOMODINFO`), one
//! entry point per FOURCC, as IBM's are registered.
//!
//! Every procedure works as IBM's image and audio procedures do (the
//! Toolkit's M-Motion sample shows the protocol): the file is read through
//! a storage-system handle (the DOS procedure, or `fccChildIOProc`) and
//! decoded at MMIOM_OPEN. With MMIO_TRANSLATEDATA an image reads as an
//! OS/2 bitmap: rows bottom-up, each padded to four bytes, 1, 4, 8 or 24
//! bits (B, G, R) per pel, described by the MMIMAGEHEADER that
//! mmioGetHeader returns (MMIO_TRANSLATEHEADER) with its palette. Without
//! translation the file's own bytes are read. Sounds read as PCM (8-bit
//! unsigned or 16-bit signed, as in a WAVE file) with an MMAUDIOHEADER.
//! A file opened MMIO_WRITE with translation takes the header from
//! mmioSetHeader and the bitmap or samples from mmioWrite and is encoded
//! when it is closed. MIDI files read as they are (an RMID file as its
//! Standard MIDI data) with an MMMIDIHEADER; AVI files are only
//! identified and read untranslated (MDM's digital video device plays
//! them).
//!
//! The formats themselves are in `crates/mmimage` and `crates/mmfmt`.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;
mod procs;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}
