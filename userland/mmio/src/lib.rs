//! MMIO.DLL: the MMPM/2 multimedia I/O manager.
//!
//! An MMIO handle (HMMIO) is an open file with an I/O procedure: the DOS
//! procedure for files, the MEM procedure for memory files, or a
//! file-format procedure (WAVE, built in; others installed with
//! mmioInstallIOProc) that presents a file's media data and header. As in
//! IBM's MMIO, the handle is an MMIOINFO the I/O procedure gets with each
//! message (MMIOM_OPEN, READ, WRITE, SEEK, CLOSE, GETHEADER...), and MMIO
//! keeps an optional I/O buffer on top: `lDiskOffset` is the file offset
//! of the buffer's first byte (unbuffered: the I/O procedure's position),
//! `lLogicalFilePos` the caller's position. A memory file is all buffer.
//!
//! mmioOpen without an I/O procedure identifies the file (a RIFF WAVE
//! file gets the WAVE procedure, which reads and seeks within the sample
//! data and gives the MMAUDIOHEADER); anything else is a DOS file.
//! mmioDescend/Ascend/CreateChunk walk and write RIFF chunks through the
//! handle's procedure. Compound files (BND, mmioCF*), CODECs and the
//! MMPMMMIO.INI procedure list are not supported: those calls return
//! MMIOERR_UNSUPPORTED_FUNCTION (or NULL).
//!
//! Handles are per process (DLL data is not shared). Structures and
//! constants are the Toolkit's (mmioos2.h, meerror.h); return codes follow
//! IBM's MMIO.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod chunk;
mod exports;
mod format;
mod io;
mod procs;
mod stubs;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

pub const fn fourcc(c: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*c)
}

pub const FOURCC_RIFF: u32 = fourcc(b"RIFF");
pub const FOURCC_LIST: u32 = fourcc(b"LIST");
pub const FOURCC_MEM: u32 = fourcc(b"MEM ");
pub const FOURCC_DOS: u32 = fourcc(b"DOS ");
pub const FOURCC_BND: u32 = fourcc(b"BND ");
pub const FOURCC_WAVE: u32 = fourcc(b"WAVE");

/// mmioOpen flags (ulFlags).
pub const MMIO_CREATE: u32 = 0x1;
pub const MMIO_READ: u32 = 0x4;
pub const MMIO_WRITE: u32 = 0x8;
pub const MMIO_READWRITE: u32 = 0x10;
pub const MMIO_COMPAT: u32 = 0x20;
pub const MMIO_EXCLUSIVE: u32 = 0x40;
pub const MMIO_DENYWRITE: u32 = 0x80;
pub const MMIO_DENYREAD: u32 = 0x100;
pub const MMIO_DENYNONE: u32 = 0x200;
pub const MMIO_ALLOCBUF: u32 = 0x400;
pub const MMIO_DELETE: u32 = 0x800;
pub const MMIO_DIRTY: u32 = 0x4000;
pub const MMIO_APPEND: u32 = 0x20000;
pub const MMIO_NOIDENTIFY: u32 = 0x40000;
pub const MMIO_FHOPEN: u32 = 0x1;
pub const MMIO_EMPTYBUF: u32 = 0x1;
pub const MMIO_DEFAULTBUFFER: i32 = 8192;

/// I/O procedure messages.
pub const MMIOM_CLOSE: u32 = 0xE03;
pub const MMIOM_OPEN: u32 = 0xE04;
pub const MMIOM_READ: u32 = 0xE05;
pub const MMIOM_SEEK: u32 = 0xE06;
pub const MMIOM_WRITE: u32 = 0xE07;
pub const MMIOM_IDENTIFYFILE: u32 = 0xE08;
pub const MMIOM_GETHEADER: u32 = 0xE09;
pub const MMIOM_SETHEADER: u32 = 0xE0A;
pub const MMIOM_QUERYHEADERLENGTH: u32 = 0xE0B;
pub const MMIOM_GETFORMATNAME: u32 = 0xE0C;
pub const MMIOM_GETFORMATINFO: u32 = 0xE0D;
pub const MMIOM_SET: u32 = 0xE13;

pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

pub const MMIO_SUCCESS: u32 = 0;
pub const MMIO_ERROR: u32 = 0xFFFF_FFFF;
/// MMIO_ERROR as the USHORT that some calls return.
pub const MMIO_ERROR16: u32 = 0xFFFF;

/// meerror.h: MMIOERR_BASE = MEBASE + 1000 = 6500.
pub mod err {
    pub const UNBUFFERED: u32 = 6501;
    pub const CANNOTWRITE: u32 = 6502;
    pub const CHUNKNOTFOUND: u32 = 6503;
    pub const INVALID_HANDLE: u32 = 6504;
    pub const INVALID_PARAMETER: u32 = 6505;
    pub const INTERNAL_SYSTEM: u32 = 6506;
    pub const INVALID_BUFFER_LENGTH: u32 = 6510;
    pub const NO_BUFFER_ALLOCATED: u32 = 6511;
    pub const NO_FLUSH_FOR_MEM_FILE: u32 = 6512;
    pub const NO_FLUSH_NEEDED: u32 = 6513;
    pub const READ_ONLY_FILE: u32 = 6514;
    pub const WRITE_ONLY_FILE: u32 = 6515;
    pub const INSTALL_PROC_FAILED: u32 = 6516;
    pub const READ_FAILED: u32 = 6517;
    pub const WRITE_FAILED: u32 = 6518;
    pub const SEEK_FAILED: u32 = 6519;
    pub const CANNOTEXPAND: u32 = 6520;
    pub const EOF_SEEN: u32 = 6522;
    pub const INVALID_ACCESS_FLAG: u32 = 6523;
    pub const INVALID_FILENAME: u32 = 6526;
    pub const DELETE_FAILED: u32 = 6533;
    pub const OUTOFMEMORY: u32 = 6534;
    pub const INVALID_FILE: u32 = 6539;
    pub const UNSUPPORTED_FUNCTION: u32 = 6545;
    pub const UNSUPPORTED_MESSAGE: u32 = 0xFFFF_FFFE;
}

/// An I/O procedure: (pmmioinfo, usMsg, lParam1, lParam2) -> LONG.
pub type IoProc = unsafe extern "C" fn(*mut MmioInfo, u32, i32, i32) -> i32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MmioInfo {
    pub ulFlags: u32,
    pub fccIOProc: u32,
    pub pIOProc: usize,
    pub ulErrorRet: u32,
    pub cchBuffer: i32,
    pub pchBuffer: *mut u8,
    pub pchNext: *mut u8,
    pub pchEndRead: *mut u8,
    pub pchEndWrite: *mut u8,
    pub lBufOffset: i32,
    pub lDiskOffset: i32,
    pub aulInfo: [u32; 4],
    pub lLogicalFilePos: i32,
    pub ulTranslate: u32,
    pub fccChildIOProc: u32,
    pub pExtraInfoStruct: usize,
    pub hmmio: u32,
}

impl MmioInfo {
    pub const fn zeroed() -> MmioInfo {
        let n = core::ptr::null_mut();
        MmioInfo {
            ulFlags: 0,
            fccIOProc: 0,
            pIOProc: 0,
            ulErrorRet: 0,
            cchBuffer: 0,
            pchBuffer: n,
            pchNext: n,
            pchEndRead: n,
            pchEndWrite: n,
            lBufOffset: 0,
            lDiskOffset: 0,
            aulInfo: [0; 4],
            lLogicalFilePos: 0,
            ulTranslate: 0,
            fccChildIOProc: 0,
            pExtraInfoStruct: 0,
            hmmio: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MmckInfo {
    pub ckid: u32,
    pub ckSize: u32,
    pub fccType: u32,
    pub ulDataOffset: u32,
    pub ulFlags: u32,
}

/// An open file. `info` comes first: I/O procedures get a pointer to it.
#[repr(C)]
pub struct Handle {
    pub info: MmioInfo,
    /// A memory file (the MEM procedure): the data is the buffer.
    pub mem: bool,
    /// MMIO allocated the memory file's buffer and may grow it by
    /// `expand` bytes (aulInfo[0] at open).
    pub expand: u32,
    /// The buffer MMIO allocated (I/O buffer or memory file).
    pub own: Option<Vec<u8>>,
}

impl Handle {
    pub fn buffered(&self) -> bool {
        !self.info.pchBuffer.is_null() && self.info.cchBuffer > 0
    }

    /// Call the handle's I/O procedure.
    pub fn send(&mut self, msg: u32, p1: i32, p2: i32) -> i32 {
        let f: IoProc = unsafe { core::mem::transmute(self.info.pIOProc) };
        unsafe { f(&mut self.info, msg, p1, p2) }
    }
}

/// Process data behind a spin lock (DLL data is per process).
pub struct Locked<T> {
    lock: AtomicBool,
    v: core::cell::UnsafeCell<T>,
}

unsafe impl<T> Sync for Locked<T> {}

impl<T> Locked<T> {
    pub const fn new(v: T) -> Locked<T> {
        Locked { lock: AtomicBool::new(false), v: core::cell::UnsafeCell::new(v) }
    }

    /// Run `f` with the lock held (`f` must not take it again).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            unsafe { os2::DosSleep(0) };
        }
        let r = f(unsafe { &mut *self.v.get() });
        self.lock.store(false, Ordering::Release);
        r
    }
}

/// Open handles of this process (the Box addresses, which are the HMMIOs).
static HANDLES: Locked<Vec<usize>> = Locked::new(Vec::new());

fn table<R>(f: impl FnOnce(&mut Vec<usize>) -> R) -> R {
    HANDLES.with(f)
}

pub fn register(h: Box<Handle>) -> u32 {
    let p = Box::into_raw(h);
    unsafe { (*p).info.hmmio = p as u32 };
    table(|t| t.push(p as usize));
    p as u32
}

/// The handle an HMMIO names.
pub fn handle<'a>(hmmio: u32) -> Option<&'a mut Handle> {
    if hmmio == 0 || !table(|t| t.contains(&(hmmio as usize))) {
        return None;
    }
    Some(unsafe { &mut *(hmmio as *mut Handle) })
}

pub fn unregister(hmmio: u32) -> Option<Box<Handle>> {
    let found = table(|t| {
        let i = t.iter().position(|&p| p == hmmio as usize)?;
        t.swap_remove(i);
        Some(())
    });
    found.map(|_| unsafe { Box::from_raw(hmmio as *mut Handle) })
}

/// A NUL-terminated string's bytes (None for NULL).
pub unsafe fn cstr<'a>(p: *const u8) -> Option<&'a [u8]> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    Some(unsafe { core::slice::from_raw_parts(p, n) })
}

/// mmioGetLastError: the handle's ulErrorRet.
#[unsafe(no_mangle)]
pub extern "C" fn mmioGetLastError(hmmio: u32) -> u32 {
    match handle(hmmio) {
        Some(h) => h.info.ulErrorRet,
        None => err::INVALID_HANDLE,
    }
}

/// mmioStringToFOURCC: up to four characters, padded with blanks;
/// MMIO_TOUPPER (1) converts them to upper case.
#[unsafe(no_mangle)]
pub extern "C" fn mmioStringToFOURCC(psz: *const u8, flags: u32) -> u32 {
    match unsafe { cstr(psz) } {
        Some(s) => mmfmt::riff::fourcc_from_str(s, flags & 1 != 0),
        None => 0,
    }
}
