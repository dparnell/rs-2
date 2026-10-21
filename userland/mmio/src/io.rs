//! Opening and closing files, reading, writing and seeking through the
//! I/O buffer, and direct buffer access (mmioGetInfo, mmioSetInfo,
//! mmioAdvance).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr;

use crate::*;

/// Bytes of valid data in the buffer.
fn valid(h: &Handle) -> i32 {
    (h.info.pchEndRead as usize).wrapping_sub(h.info.pchBuffer as usize) as i32
}

/// Point pchNext at the logical position when the buffer holds it.
fn locate(h: &mut Handle) {
    let i = &mut h.info;
    let off = i.lLogicalFilePos.wrapping_sub(i.lDiskOffset);
    if !i.pchBuffer.is_null() && off >= 0 && off <= i.cchBuffer {
        i.lBufOffset = off;
        i.pchNext = unsafe { i.pchBuffer.add(off as usize) };
    }
}

/// Give the handle a buffer of `size` bytes (MMIO's own), empty, at the
/// logical position.
fn set_own_buffer(h: &mut Handle, size: usize) {
    let mut v: Vec<u8> = vec![0; size.max(1)];
    let p = v.as_mut_ptr();
    h.own = Some(v);
    let i = &mut h.info;
    i.pchBuffer = p;
    i.cchBuffer = size as i32;
    i.pchNext = p;
    i.pchEndRead = p;
    i.pchEndWrite = unsafe { p.add(size) };
    i.lBufOffset = 0;
}

/// Write the buffer out if it holds changes. Errors: IBM's mmioFlush codes.
pub fn flush(h: &mut Handle) -> u32 {
    if !h.buffered() {
        return err::NO_BUFFER_ALLOCATED;
    }
    if h.mem {
        return err::NO_FLUSH_FOR_MEM_FILE;
    }
    if h.info.ulFlags & MMIO_DIRTY == 0 {
        return err::NO_FLUSH_NEEDED;
    }
    let n = valid(h);
    let disk = h.info.lDiskOffset;
    if h.send(MMIOM_SEEK, disk, SEEK_SET) == -1 {
        return err::SEEK_FAILED;
    }
    let w = h.send(MMIOM_WRITE, h.info.pchBuffer as i32, n);
    if w == -1 {
        return if h.info.ulErrorRet == err::CANNOTWRITE { err::CANNOTWRITE } else { err::WRITE_FAILED };
    }
    if w != n {
        return err::CANNOTWRITE;
    }
    h.info.ulFlags &= !MMIO_DIRTY;
    0
}

/// Flush, taking "nothing to flush" as success.
fn flush_ok(h: &mut Handle) -> Result<(), u32> {
    match flush(h) {
        0 | err::NO_BUFFER_ALLOCATED | err::NO_FLUSH_FOR_MEM_FILE | err::NO_FLUSH_NEEDED => Ok(()),
        e => Err(e),
    }
}

/// Grow a memory file's buffer to hold at least `need` bytes.
fn expand(h: &mut Handle, need: i32) -> bool {
    if h.expand == 0 || h.own.is_none() {
        return false;
    }
    let mut cap = h.info.cchBuffer.max(0) as usize;
    while (cap as i64) < need as i64 {
        cap += h.expand as usize;
    }
    let n = valid(h) as usize;
    let mut v: Vec<u8> = vec![0; cap];
    v[..n].copy_from_slice(unsafe { core::slice::from_raw_parts(h.info.pchBuffer, n) });
    let p = v.as_mut_ptr();
    h.own = Some(v);
    let i = &mut h.info;
    i.pchBuffer = p;
    i.cchBuffer = cap as i32;
    i.pchEndRead = unsafe { p.add(n) };
    i.pchEndWrite = unsafe { p.add(cap) };
    locate(h);
    true
}

/// mmioRead's work: bytes read, 0 at the end (MMIOERR_EOF_SEEN), -1.
pub fn read(h: &mut Handle, dst: *mut u8, n: i32) -> i32 {
    h.info.ulErrorRet = 0;
    let f = h.info.ulFlags;
    if f & MMIO_WRITE != 0 {
        h.info.ulErrorRet = err::WRITE_ONLY_FILE;
        return -1;
    }
    if n <= 0 {
        return 0;
    }
    if !h.buffered() && !h.mem {
        let pos = h.info.lLogicalFilePos;
        if h.info.lDiskOffset != pos {
            let r = h.send(MMIOM_SEEK, pos, SEEK_SET);
            if r == -1 {
                return -1;
            }
            h.info.lDiskOffset = r;
        }
        let r = h.send(MMIOM_READ, dst as i32, n);
        if r < 0 {
            return -1;
        }
        h.info.lLogicalFilePos += r;
        h.info.lDiskOffset = h.info.lLogicalFilePos;
        if r == 0 {
            h.info.ulErrorRet = err::EOF_SEEN;
        }
        return r;
    }
    let mut done = 0i32;
    while done < n {
        let pos = h.info.lLogicalFilePos;
        let off = pos.wrapping_sub(h.info.lDiskOffset);
        let v = valid(h);
        if off >= 0 && off < v {
            let k = (v - off).min(n - done);
            unsafe { ptr::copy(h.info.pchBuffer.add(off as usize), dst.add(done as usize), k as usize) };
            done += k;
            h.info.lLogicalFilePos += k;
            continue;
        }
        if h.mem {
            break;
        }
        if let Err(e) = flush_ok(h) {
            h.info.ulErrorRet = e;
            return -1;
        }
        if h.send(MMIOM_SEEK, pos, SEEK_SET) == -1 {
            return -1;
        }
        h.info.lDiskOffset = pos;
        let got = h.send(MMIOM_READ, h.info.pchBuffer as i32, h.info.cchBuffer);
        if got < 0 {
            h.info.pchEndRead = h.info.pchBuffer;
            return -1;
        }
        h.info.pchEndRead = unsafe { h.info.pchBuffer.add(got as usize) };
        if got == 0 {
            break;
        }
    }
    locate(h);
    if done == 0 {
        h.info.ulErrorRet = err::EOF_SEEN;
    }
    done
}

/// mmioWrite's work: bytes written or -1.
pub fn write(h: &mut Handle, src: *const u8, n: i32) -> i32 {
    h.info.ulErrorRet = 0;
    if h.info.ulFlags & MMIO_READ != 0 {
        h.info.ulErrorRet = err::READ_ONLY_FILE;
        return -1;
    }
    if n <= 0 {
        return 0;
    }
    if !h.buffered() && !h.mem {
        let pos = h.info.lLogicalFilePos;
        if h.info.lDiskOffset != pos {
            let r = h.send(MMIOM_SEEK, pos, SEEK_SET);
            if r == -1 {
                return -1;
            }
            h.info.lDiskOffset = r;
        }
        let r = h.send(MMIOM_WRITE, src as i32, n);
        if r < 0 {
            return -1;
        }
        h.info.lLogicalFilePos += r;
        h.info.lDiskOffset = h.info.lLogicalFilePos;
        return r;
    }
    let mut done = 0i32;
    while done < n {
        let pos = h.info.lLogicalFilePos;
        let off = pos.wrapping_sub(h.info.lDiskOffset);
        let v = valid(h);
        let cap = h.info.cchBuffer;
        if !(off >= 0 && off <= v && off < cap) {
            if h.mem {
                if off >= 0 && off <= v && expand(h, off + (n - done)) {
                    continue;
                }
                h.info.ulErrorRet = err::CANNOTEXPAND;
                break;
            }
            if let Err(e) = flush_ok(h) {
                h.info.ulErrorRet = e;
                return -1;
            }
            h.info.lDiskOffset = pos;
            h.info.pchEndRead = h.info.pchBuffer;
            continue;
        }
        let k = (cap - off).min(n - done);
        unsafe { ptr::copy(src.add(done as usize), h.info.pchBuffer.add(off as usize), k as usize) };
        done += k;
        h.info.lLogicalFilePos += k;
        if off + k > v {
            h.info.pchEndRead = unsafe { h.info.pchBuffer.add((off + k) as usize) };
        }
        h.info.ulFlags |= MMIO_DIRTY;
    }
    locate(h);
    if done == 0 { -1 } else { done }
}

/// mmioSeek's work: the new position or -1.
pub fn seek(h: &mut Handle, off: i32, origin: i32) -> i32 {
    h.info.ulErrorRet = 0;
    let base = match origin {
        SEEK_SET => 0,
        SEEK_CUR => h.info.lLogicalFilePos,
        SEEK_END if h.mem => valid(h),
        SEEK_END => {
            let end = h.send(MMIOM_SEEK, 0, SEEK_END);
            if end < 0 {
                if h.info.ulErrorRet == 0 {
                    h.info.ulErrorRet = err::SEEK_FAILED;
                }
                return -1;
            }
            if !h.buffered() {
                h.info.lDiskOffset = end;
            }
            if h.buffered() { end.max(h.info.lDiskOffset + valid(h)) } else { end }
        }
        _ => {
            h.info.ulErrorRet = err::INVALID_PARAMETER;
            return -1;
        }
    };
    let new = base.wrapping_add(off);
    if new < 0 || (h.mem && new > valid(h)) {
        h.info.ulErrorRet = err::SEEK_FAILED;
        return -1;
    }
    if !h.buffered() && !h.mem {
        let r = h.send(MMIOM_SEEK, new, SEEK_SET);
        if r < 0 {
            if h.info.ulErrorRet == 0 {
                h.info.ulErrorRet = err::SEEK_FAILED;
            }
            return -1;
        }
        h.info.lDiskOffset = r;
        h.info.lLogicalFilePos = r;
        return r;
    }
    h.info.lLogicalFilePos = new;
    locate(h);
    new
}

/// The flag combinations mmioOpen rejects (MMIOERR_INVALID_ACCESS_FLAG).
fn bad_flags(f: u32) -> bool {
    let access = [MMIO_READ, MMIO_WRITE, MMIO_READWRITE].iter().filter(|&&b| f & b != 0).count();
    let share = [MMIO_COMPAT, MMIO_EXCLUSIVE, MMIO_DENYWRITE, MMIO_DENYREAD, MMIO_DENYNONE].iter().filter(|&&b| f & b != 0).count();
    access > 1
        || share > 1
        || (f & MMIO_DELETE != 0 && f & MMIO_ALLOCBUF != 0)
        || (f & MMIO_APPEND != 0 && f & (MMIO_READ | MMIO_CREATE) != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioOpen(name: *const u8, pinfo: *mut MmioInfo, flags: u32) -> u32 {
    if name.is_null() && pinfo.is_null() {
        return 0;
    }
    let mut user = unsafe { pinfo.as_mut() };
    let mut info = user.as_ref().map(|u| **u).unwrap_or(MmioInfo::zeroed());
    let fail = |user: Option<&mut MmioInfo>, e: u32| {
        if let Some(u) = user {
            u.ulErrorRet = e;
        }
        0
    };
    if bad_flags(flags) {
        return fail(user, err::INVALID_ACCESS_FLAG);
    }
    let mut flags = flags;
    if flags & (MMIO_EXCLUSIVE | MMIO_DENYWRITE | MMIO_DENYREAD | MMIO_DENYNONE) == 0 {
        flags |= MMIO_DENYNONE;
    }
    if info.pIOProc != 0 && info.fccIOProc != 0 {
        return fail(user, err::INVALID_PARAMETER);
    }
    if info.pIOProc == 0 {
        if info.fccIOProc == 0 {
            // Identify the file: a file-format procedure that knows it, else DOS.
            if !name.is_null() && flags & (MMIO_CREATE | MMIO_NOIDENTIFY | MMIO_DELETE) == 0 {
                info.fccIOProc = procs::identify(name).unwrap_or(FOURCC_DOS);
            } else {
                info.fccIOProc = FOURCC_DOS;
            }
        }
        match procs::find(info.fccIOProc) {
            Some(p) => info.pIOProc = p,
            None => return fail(user, err::INSTALL_PROC_FAILED),
        }
    }
    let mem = info.fccIOProc == FOURCC_MEM;
    if flags & MMIO_DELETE != 0 && (mem || name.is_null()) {
        return fail(user, err::DELETE_FAILED);
    }
    if mem {
        flags &= !MMIO_ALLOCBUF;
    }
    info.ulFlags = flags;
    info.ulErrorRet = 0;
    info.lDiskOffset = 0;
    info.lLogicalFilePos = 0;
    info.lBufOffset = 0;
    info.hmmio = 0;
    let mut h = Box::new(Handle { info, mem, expand: 0, own: None });
    if mem {
        let user_buf = !h.info.pchBuffer.is_null();
        if user_buf {
            if h.info.cchBuffer < 1 {
                return fail(user, err::INVALID_BUFFER_LENGTH);
            }
            if h.info.aulInfo[0] != 0 {
                return fail(user, err::INVALID_PARAMETER);
            }
            let i = &mut h.info;
            i.pchNext = i.pchBuffer;
            i.pchEndWrite = unsafe { i.pchBuffer.add(i.cchBuffer as usize) };
            i.pchEndRead = if flags & MMIO_CREATE != 0 { i.pchBuffer } else { i.pchEndWrite };
        } else {
            let size = h.info.cchBuffer.max(0) as usize;
            h.expand = h.info.aulInfo[0];
            set_own_buffer(&mut h, size);
        }
    } else if flags & MMIO_ALLOCBUF != 0 {
        if !h.info.pchBuffer.is_null() {
            return fail(user, err::INVALID_PARAMETER);
        }
        let size = if h.info.cchBuffer > 0 { h.info.cchBuffer } else { MMIO_DEFAULTBUFFER };
        set_own_buffer(&mut h, size as usize);
    } else if !h.info.pchBuffer.is_null() {
        if h.info.cchBuffer < 1 {
            return fail(user, err::INVALID_BUFFER_LENGTH);
        }
        let i = &mut h.info;
        i.pchNext = i.pchBuffer;
        i.pchEndRead = i.pchBuffer;
        i.pchEndWrite = unsafe { i.pchBuffer.add(i.cchBuffer as usize) };
    } else {
        h.info.cchBuffer = 0;
        h.info.pchNext = ptr::null_mut();
        h.info.pchEndRead = ptr::null_mut();
        h.info.pchEndWrite = ptr::null_mut();
    }
    let rc = h.send(MMIOM_OPEN, name as i32, 0) as u32;
    if let Some(u) = user.as_deref_mut() {
        u.ulErrorRet = rc;
    }
    if flags & MMIO_DELETE != 0 {
        return (rc == 0) as u32;
    }
    if rc != 0 {
        return 0;
    }
    if h.mem {
        h.info.lDiskOffset = 0;
        h.info.lLogicalFilePos = 0;
    } else {
        // The procedure sets the starting position (MMIO_APPEND, a DOS handle).
        h.info.lDiskOffset = h.info.lLogicalFilePos;
    }
    let fcc = h.info.fccIOProc;
    let extra = h.info.pExtraInfoStruct;
    let hmmio = register(h);
    if let Some(u) = user {
        u.fccIOProc = fcc;
        u.pExtraInfoStruct = extra;
        u.hmmio = hmmio;
    }
    hmmio
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioClose(hmmio: u32, flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    h.info.ulErrorRet = 0;
    match flush(h) {
        0 => {}
        e @ (err::NO_FLUSH_FOR_MEM_FILE | err::NO_FLUSH_NEEDED) => h.info.ulErrorRet = e,
        err::NO_BUFFER_ALLOCATED => {}
        e => {
            h.info.ulErrorRet = e;
            return e;
        }
    }
    let rc = h.send(MMIOM_CLOSE, (flags & 0xffff) as i32, 0) as u32;
    if rc != 0 && rc != 2 {
        h.info.ulErrorRet = rc;
        return MMIO_ERROR16;
    }
    drop(unregister(hmmio));
    rc & 0xffff
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioRead(hmmio: u32, buf: *mut u8, n: i32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    match handle(hmmio) {
        Some(h) => read(h, buf, n),
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioWrite(hmmio: u32, buf: *const u8, n: i32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    match handle(hmmio) {
        Some(h) => write(h, buf, n),
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioSeek(hmmio: u32, off: i32, origin: i32) -> i32 {
    match handle(hmmio) {
        Some(h) => seek(h, off, origin),
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioFlush(hmmio: u32, flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    h.info.ulErrorRet = 0;
    let rc = flush(h);
    if rc == 0 && flags & MMIO_EMPTYBUF != 0 && !h.mem {
        let i = &mut h.info;
        i.lDiskOffset = i.lLogicalFilePos;
        i.pchNext = i.pchBuffer;
        i.pchEndRead = i.pchBuffer;
        i.lBufOffset = 0;
    }
    if rc != 0 {
        h.info.ulErrorRet = rc;
    }
    rc
}

/// mmioSetBuffer: a caller's buffer (pch, cch), one MMIO allocates
/// (NULL, cch), or none (NULL, 0: unbuffered I/O).
#[unsafe(no_mangle)]
pub extern "C" fn mmioSetBuffer(hmmio: u32, pch: *mut u8, cch: i32, _flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    let e = |h: &mut Handle, e: u32| {
        h.info.ulErrorRet = e;
        e
    };
    if cch < 0 || (!pch.is_null() && cch < 1) {
        return e(h, err::INVALID_BUFFER_LENGTH);
    }
    if pch.is_null() && cch == 0 && !h.buffered() {
        return e(h, err::UNBUFFERED);
    }
    if h.mem {
        // Move the memory file into the new buffer.
        let n = valid(h);
        if cch < n {
            return e(h, err::INVALID_BUFFER_LENGTH);
        }
        let old = h.info.pchBuffer;
        let pos = h.info.lLogicalFilePos;
        let keep = h.own.take();
        if pch.is_null() {
            set_own_buffer(h, cch as usize);
        } else {
            let i = &mut h.info;
            i.pchBuffer = pch;
            i.cchBuffer = cch;
            i.pchEndWrite = unsafe { pch.add(cch as usize) };
            h.expand = 0;
        }
        unsafe { ptr::copy(old, h.info.pchBuffer, n as usize) };
        drop(keep);
        h.info.pchEndRead = unsafe { h.info.pchBuffer.add(n as usize) };
        h.info.lLogicalFilePos = pos;
        locate(h);
        return 0;
    }
    if let Err(x) = flush_ok(h) {
        return e(h, x);
    }
    h.own = None;
    let pos = h.info.lLogicalFilePos;
    if pch.is_null() && cch == 0 {
        let i = &mut h.info;
        i.pchBuffer = ptr::null_mut();
        i.cchBuffer = 0;
        i.pchNext = ptr::null_mut();
        i.pchEndRead = ptr::null_mut();
        i.pchEndWrite = ptr::null_mut();
        i.lBufOffset = 0;
        // The procedure's position is unknown after buffered I/O.
        i.lDiskOffset = -1;
        return 0;
    }
    if pch.is_null() {
        set_own_buffer(h, cch as usize);
    } else {
        let i = &mut h.info;
        i.pchBuffer = pch;
        i.cchBuffer = cch;
        i.pchNext = pch;
        i.pchEndRead = pch;
        i.pchEndWrite = unsafe { pch.add(cch as usize) };
        i.lBufOffset = 0;
    }
    h.info.lDiskOffset = pos;
    0
}

/// Make the buffer hold the logical position (for direct access): write
/// it out and refill it when the position is outside.
fn fill_at_pos(h: &mut Handle) -> u32 {
    if h.mem {
        locate(h);
        return 0;
    }
    let off = h.info.lLogicalFilePos - h.info.lDiskOffset;
    if off >= 0 && off < valid(h) {
        locate(h);
        return 0;
    }
    if let Err(e) = flush_ok(h) {
        return e;
    }
    let pos = h.info.lLogicalFilePos;
    h.info.lDiskOffset = pos;
    h.info.pchEndRead = h.info.pchBuffer;
    h.info.pchNext = h.info.pchBuffer;
    h.info.lBufOffset = 0;
    if h.info.ulFlags & MMIO_WRITE == 0 {
        if h.send(MMIOM_SEEK, pos, SEEK_SET) == -1 {
            return err::SEEK_FAILED;
        }
        let got = h.send(MMIOM_READ, h.info.pchBuffer as i32, h.info.cchBuffer);
        if got < 0 {
            return err::READ_FAILED;
        }
        h.info.pchEndRead = unsafe { h.info.pchBuffer.add(got as usize) };
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioGetInfo(hmmio: u32, pinfo: *mut MmioInfo, _flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    let Some(out) = (unsafe { pinfo.as_mut() }) else { return err::INVALID_PARAMETER };
    out.ulErrorRet = 0;
    h.info.ulErrorRet = 0;
    if !h.buffered() {
        return err::UNBUFFERED;
    }
    let rc = fill_at_pos(h);
    if rc != 0 {
        return rc;
    }
    *out = h.info;
    0
}

/// Take back what direct buffer access changed: pchNext, pchEndRead,
/// pchEndWrite, the error and the dirty flag (and a memory file's buffer).
fn take_info(h: &mut Handle, u: &MmioInfo) {
    let i = &mut h.info;
    i.pchNext = u.pchNext;
    i.pchEndRead = u.pchEndRead;
    i.pchEndWrite = u.pchEndWrite;
    i.ulErrorRet = u.ulErrorRet;
    if h.mem {
        i.pchBuffer = u.pchBuffer;
        i.cchBuffer = u.cchBuffer;
        i.lDiskOffset = 0;
    } else {
        i.lDiskOffset = u.lDiskOffset;
    }
    i.lBufOffset = (i.pchNext as usize).wrapping_sub(i.pchBuffer as usize) as i32;
    i.lLogicalFilePos = i.lDiskOffset + i.lBufOffset;
    if u.ulFlags & MMIO_DIRTY != 0 {
        i.ulFlags |= MMIO_DIRTY;
        // Data written up to pchNext is part of the buffer.
        if (i.pchNext as usize) > (i.pchEndRead as usize) {
            i.pchEndRead = i.pchNext;
        }
    } else {
        i.ulFlags &= !MMIO_DIRTY;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioSetInfo(hmmio: u32, pinfo: *mut MmioInfo, _flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    let Some(u) = (unsafe { pinfo.as_mut() }) else { return err::INVALID_PARAMETER };
    h.info.ulErrorRet = 0;
    u.ulErrorRet = 0;
    if u.hmmio != hmmio {
        return err::INVALID_PARAMETER;
    }
    if !h.buffered() {
        return err::UNBUFFERED;
    }
    let u = *u;
    take_info(h, &u);
    0
}

/// mmioAdvance: the caller has read or filled the buffer up to pchNext;
/// write it out (when dirty) and give the next buffer of the file
/// (MMIO_READ fills it). A memory file grows by its expansion size.
#[unsafe(no_mangle)]
pub extern "C" fn mmioAdvance(hmmio: u32, pinfo: *mut MmioInfo, flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    let Some(u) = (unsafe { pinfo.as_mut() }) else { return err::INVALID_PARAMETER };
    u.ulErrorRet = 0;
    h.info.ulErrorRet = 0;
    if u.hmmio != hmmio {
        return err::INVALID_PARAMETER;
    }
    let fail = |u: &mut MmioInfo, e: u32| {
        u.ulErrorRet = e;
        e
    };
    if !h.buffered() || (h.mem && h.expand == 0) {
        return fail(u, err::UNBUFFERED);
    }
    let of = h.info.ulFlags;
    if of & MMIO_READ != 0 && flags & MMIO_READ == 0 {
        return fail(u, err::READ_ONLY_FILE);
    }
    if of & MMIO_WRITE != 0 && flags & MMIO_WRITE == 0 {
        return fail(u, err::WRITE_ONLY_FILE);
    }
    if flags & (MMIO_READ | MMIO_WRITE) == 0 {
        return fail(u, err::INVALID_PARAMETER);
    }
    let copy = *u;
    take_info(h, &copy);
    if h.mem {
        if flags & MMIO_WRITE == 0 {
            return fail(u, err::READ_FAILED);
        }
        let need = h.info.cchBuffer + h.expand as i32;
        if !expand(h, need) {
            return fail(u, err::CANNOTEXPAND);
        }
        *u = h.info;
        return 0;
    }
    if h.info.ulFlags & MMIO_DIRTY == 0 && flags & MMIO_WRITE != 0 && flags & MMIO_READ == 0 {
        return fail(u, err::NO_FLUSH_NEEDED);
    }
    if let Err(e) = flush_ok(h) {
        return fail(u, e);
    }
    let next = h.info.lDiskOffset + h.info.lBufOffset;
    h.info.lDiskOffset = next;
    h.info.lLogicalFilePos = next;
    h.info.pchNext = h.info.pchBuffer;
    h.info.pchEndRead = h.info.pchBuffer;
    h.info.lBufOffset = 0;
    if flags & MMIO_READ != 0 && of & MMIO_WRITE == 0 {
        if h.send(MMIOM_SEEK, next, SEEK_SET) == -1 {
            return fail(u, err::SEEK_FAILED);
        }
        let got = h.send(MMIOM_READ, h.info.pchBuffer as i32, h.info.cchBuffer);
        if got == -1 {
            return fail(u, err::READ_FAILED);
        }
        h.info.pchEndRead = unsafe { h.info.pchBuffer.add(got.max(0) as usize) };
    }
    *u = h.info;
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioSendMessage(hmmio: u32, msg: u32, p1: i32, p2: i32) -> i32 {
    let Some(h) = handle(hmmio) else { return -1 };
    h.info.ulErrorRet = 0;
    h.send(msg & 0xffff, p1, p2)
}
