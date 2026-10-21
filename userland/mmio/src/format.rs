//! Media headers and file formats: mmioGetHeader, mmioSetHeader,
//! mmioQueryHeaderLength, mmioSet, mmioIdentifyFile, the format list
//! (mmioQueryFormatCount, mmioGetFormats, mmioGetFormatName).
//!
//! The format list holds the file-format procedure for WAVE (the built-in
//! one, or one installed in its place).

use alloc::vec::Vec;

use crate::procs::{FormatInfo, format_info, format_name};
use crate::*;

/// IBM returns 0 when the procedure gave a positive count, else MMIO_ERROR.
fn count_rc(n: i32) -> u32 {
    if n > 0 { 0 } else { MMIO_ERROR }
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioGetHeader(hmmio: u32, header: *mut u8, len: i32, read: *mut i32, _res: u32, _flags: u32) -> u32 {
    if header.is_null() || len <= 0 || read.is_null() {
        return err::INVALID_PARAMETER;
    }
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    let mut n = h.send(MMIOM_GETHEADER, header as i32, len);
    if n == -2 {
        n = 0;
    }
    unsafe { *read = n };
    count_rc(n)
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioSetHeader(hmmio: u32, header: *mut u8, len: i32, written: *mut i32, _res: u32, _flags: u32) -> u32 {
    if header.is_null() || len <= 0 || written.is_null() {
        return err::INVALID_PARAMETER;
    }
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    let mut n = h.send(MMIOM_SETHEADER, header as i32, len);
    if n == -2 {
        n = 0;
    }
    unsafe { *written = n };
    count_rc(n)
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioQueryHeaderLength(hmmio: u32, plen: *mut i32, _res: u32, _flags: u32) -> u32 {
    if plen.is_null() {
        return err::INVALID_PARAMETER;
    }
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    let mut n = h.send(MMIOM_QUERYHEADERLENGTH, 0, 0);
    if n == -2 {
        n = 0;
    }
    unsafe { *plen = n };
    count_rc(n)
}

/// mmioSet: pass an MMEXTENDINFO to the procedure (MMIO_SET_EXTENDEDINFO,
/// MMIO_QUERY_EXTENDEDINFO_BASE, MMIO_QUERY_EXTENDEDINFO_ALL).
#[unsafe(no_mangle)]
pub extern "C" fn mmioSet(hmmio: u32, ext: *mut u8, flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    if ext.is_null() || flags & 7 == 0 {
        return err::INVALID_PARAMETER;
    }
    h.send(MMIOM_SET, ext as i32, flags as i32) as u32
}

/// The formats known: the built-in WAVE and installed file-format
/// procedures that describe themselves.
fn formats() -> Vec<FormatInfo> {
    let mut v = Vec::new();
    for fcc in [FOURCC_WAVE] {
        if let Some(p) = procs::find(fcc) {
            let mut f = format_info(fcc).unwrap();
            let pf: IoProc = unsafe { core::mem::transmute(p) };
            unsafe { pf(core::ptr::null_mut(), MMIOM_GETFORMATINFO, &mut f as *mut FormatInfo as i32, 0) };
            v.push(f);
        }
    }
    v
}

fn matches(f: &FormatInfo, want: &FormatInfo) -> bool {
    (want.fccIOProc == 0 || f.fccIOProc == want.fccIOProc) && (want.ulMediaType == 0 || f.ulMediaType == want.ulMediaType)
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioQueryFormatCount(want: *const FormatInfo, count: *mut i32, _res: u32, _flags: u32) -> u32 {
    let (Some(want), false) = (unsafe { want.as_ref() }, count.is_null()) else { return err::INVALID_PARAMETER };
    let n = formats().iter().filter(|f| matches(f, want)).count() as i32;
    unsafe { *count = n };
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioGetFormats(want: *const FormatInfo, max: i32, list: *mut FormatInfo, read: *mut i32, _res: u32, _flags: u32) -> u32 {
    let Some(want) = (unsafe { want.as_ref() }) else { return err::INVALID_PARAMETER };
    if max < 1 || list.is_null() || read.is_null() {
        return err::INVALID_PARAMETER;
    }
    let mut n = 0;
    for f in formats().iter().filter(|f| matches(f, want)).take(max as usize) {
        unsafe { list.add(n).write_unaligned(*f) };
        n += 1;
    }
    unsafe { *read = n as i32 };
    0
}

/// mmioGetFormatName: the descriptive name of the format, at most
/// lNameLength bytes (not NUL-terminated).
#[unsafe(no_mangle)]
pub extern "C" fn mmioGetFormatName(fmt: *const FormatInfo, name: *mut u8, read: *mut i32, _res: u32, _flags: u32) -> u32 {
    let Some(fmt) = (unsafe { fmt.as_ref() }) else { return err::INVALID_PARAMETER };
    if name.is_null() || read.is_null() || fmt.lNameLength < 1 {
        return err::INVALID_PARAMETER;
    }
    let mut n = 0;
    if let Some(p) = procs::find(fmt.fccIOProc) {
        let pf: IoProc = unsafe { core::mem::transmute(p) };
        n = unsafe { pf(core::ptr::null_mut(), MMIOM_GETFORMATNAME, name as i32, fmt.lNameLength) };
        if n < 0 {
            let s = format_name(fmt.fccIOProc);
            n = s.len().min(fmt.lNameLength as usize) as i32;
            unsafe { core::ptr::copy(s.as_ptr(), name, n as usize) };
        }
    }
    unsafe { *read = n };
    if n > 0 { 0 } else { MMIO_ERROR }
}

/// mmioIdentifyFile: the file-format procedure for a file (by name, or the
/// open file pmmioinfo->hmmio) and its storage system. A file no format
/// procedure recognises is a plain DOS file.
#[unsafe(no_mangle)]
pub extern "C" fn mmioIdentifyFile(
    name: *const u8,
    pinfo: *const MmioInfo,
    out: *mut FormatInfo,
    pss: *mut u32,
    _res: u32,
    _flags: u32,
) -> u32 {
    let open = unsafe { pinfo.as_ref() }.map(|i| i.hmmio).filter(|&h| handle(h).is_some());
    if out.is_null() || pss.is_null() || (name.is_null() && open.is_none()) {
        return err::INVALID_PARAMETER;
    }
    let fcc = match open {
        Some(h) => {
            let mem = handle(h).is_some_and(|h| h.mem);
            unsafe { *pss = if mem { FOURCC_MEM } else { FOURCC_DOS } };
            let p = procs::find(FOURCC_WAVE).unwrap();
            let pf: IoProc = unsafe { core::mem::transmute(p) };
            if unsafe { pf(core::ptr::null_mut(), MMIOM_IDENTIFYFILE, 0, h as i32) } == 0 { FOURCC_WAVE } else { FOURCC_DOS }
        }
        None => {
            if procs::DOSIOProc(core::ptr::null_mut(), MMIOM_IDENTIFYFILE, name as i32, 0) != 0 {
                return MMIO_ERROR;
            }
            unsafe { *pss = FOURCC_DOS };
            procs::identify(name).unwrap_or(FOURCC_DOS)
        }
    };
    let mut f = format_info(fcc).unwrap_or(format_info(FOURCC_DOS).unwrap());
    if let Some(p) = procs::find(fcc) {
        let pf: IoProc = unsafe { core::mem::transmute(p) };
        unsafe { pf(core::ptr::null_mut(), MMIOM_GETFORMATINFO, &mut f as *mut FormatInfo as i32, 0) };
    }
    unsafe { out.write_unaligned(f) };
    0
}

/// mmioIdentifyStorageSystem: DOS for any file that exists.
#[unsafe(no_mangle)]
pub extern "C" fn mmioIdentifyStorageSystem(name: *const u8, _pinfo: *const MmioInfo, pss: *mut u32) -> u32 {
    if name.is_null() || pss.is_null() {
        return err::INVALID_PARAMETER;
    }
    match procs::DOSIOProc(core::ptr::null_mut(), MMIOM_IDENTIFYFILE, name as i32, 0) {
        0 => {
            unsafe { *pss = FOURCC_DOS };
            0
        }
        _ => MMIO_ERROR,
    }
}
