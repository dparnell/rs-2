//! File and directory APIs (16- and 32-bit).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::fs::{self, attr, mode, path, Device, FileInfo, FullPath, Handle, OpenFile, SetInfo, Stamp};

// ---- Handle table ----

pub const MAX_HANDLES: usize = 32768;

pub fn get(c: &Ctx, hf: u32) -> Option<Handle> {
    c.process.handles.lock().get(hf as usize).cloned().flatten()
}

/// Put a handle in the lowest free slot.
pub fn add(c: &Ctx, h: Handle) -> Result<u32, u32> {
    let mut hs = c.process.handles.lock();
    if let Some(i) = hs.iter().position(|s| s.is_none()) {
        hs[i] = Some(h);
        return Ok(i as u32);
    }
    if hs.len() >= MAX_HANDLES {
        return Err(err::TOO_MANY_OPEN_FILES);
    }
    hs.push(Some(h));
    Ok(hs.len() as u32 - 1)
}

/// Resolve a path for the calling process. An empty name is not a path
/// (OS/2 does not treat it as the current directory).
pub fn resolve(c: &Ctx, p: &str) -> Result<FullPath, u32> {
    if p.is_empty() {
        return Err(err::FILE_NOT_FOUND);
    }
    let cwd = c.process.cwd.lock();
    path::canonical(p, cwd.0, &cwd.1)
}

fn read_path(c: &Ctx, addr: u32) -> Result<String, u32> {
    c.read_str(addr, path::MAX_PATH).ok_or(err::INVALID_ADDRESS)
}

/// Check that each component fits the file system's naming rules.
fn check_names(fs: &dyn fs::FileSystem, p: &FullPath) -> Result<(), u32> {
    let max = fs.max_component();
    for part in &p.parts {
        if part.len() > max || (max == 12 && !fat::valid_83(part)) {
            return Err(err::FILENAME_EXCED_RANGE);
        }
    }
    Ok(())
}

// ---- Read / write ----

pub fn write_handle(c: &Ctx, hf: u32, buf: u32, len: u32) -> (u32, u32) {
    let Some(h) = get(c, hf) else { return (err::INVALID_HANDLE, 0) };
    if let Handle::Drive(d) = &h {
        return dasd_rw(c, d, buf, len, true);
    }
    let Some(data) = c.read_vec(buf, len) else { return (err::INVALID_ADDRESS, 0) };
    match h {
        Handle::Con | Handle::Screen => {
            crate::console::write(&data);
            (0, len)
        }
        Handle::Clock => match crate::chardev::clock_write(&data) {
            Ok(()) => (0, len),
            Err(e) => (e, 0),
        },
        Handle::Char(d) => (0, crate::chardev::write(d, &data) as u32),
        Handle::Nul | Handle::Kbd | Handle::DosDev | Handle::Drive(_) => (0, len),
        Handle::PhysDisk(_) => (err::ACCESS_DENIED, 0),
        Handle::File(f) => {
            if f.access() == mode::READONLY {
                return (err::ACCESS_DENIED, 0);
            }
            let mut pos = f.pos.lock();
            match f.file.write(*pos, &data) {
                Ok(n) => {
                    *pos += n as u64;
                    (0, n as u32)
                }
                Err(e) => (e, 0),
            }
        }
        Handle::Pipe(p) => match p.write(&data) {
            Ok(n) => (0, n as u32),
            Err(e) => (e, 0),
        },
        Handle::NPipe(p) => match p.write(&data) {
            Ok(n) => (0, n as u32),
            Err(e) => (e, 0),
        },
    }
}

pub fn read_handle(c: &Ctx, hf: u32, buf: u32, len: u32) -> (u32, u32) {
    let Some(h) = get(c, hf) else { return (err::INVALID_HANDLE, 0) };
    if let Handle::Drive(d) = &h {
        return dasd_rw(c, d, buf, len, false);
    }
    let mut status = 0;
    let data: Vec<u8> = match h {
        Handle::Con | Handle::Kbd => {
            crate::conkbd::read_cooked(len as usize)
        }
        Handle::Clock => crate::chardev::clock_read(),
        Handle::Char(d) => crate::chardev::read(d, len as usize),
        Handle::Nul | Handle::Screen | Handle::DosDev | Handle::PhysDisk(_) | Handle::Drive(_) => Vec::new(),
        Handle::File(f) => {
            if f.access() == mode::WRITEONLY {
                return (err::ACCESS_DENIED, 0);
            }
            let mut pos = f.pos.lock();
            let mut v = alloc::vec![0u8; len as usize];
            match f.file.read(*pos, &mut v) {
                Ok(n) => {
                    v.truncate(n);
                    *pos += n as u64;
                    v
                }
                Err(e) => return (e, 0),
            }
        }
        Handle::Pipe(p) => match p.read(len as usize) {
            Ok(v) => v,
            Err(e) => return (e, 0),
        },
        Handle::NPipe(p) => match p.read(len as usize) {
            Ok((v, e)) => {
                status = e;
                v
            }
            Err(e) => return (e, 0),
        },
    };
    let n = data.len().min(len as usize);
    if n > 0 && !c.write(buf, &data[..n]) {
        return (err::INVALID_ADDRESS, 0);
    }
    (status, n as u32)
}

/// DosReadAsync / DosWriteAsync(hf, phsemRam, pusErrCode, pvBuf, cbBuf,
/// pcbBytes): done synchronously, then the RAM semaphore is cleared.
fn async_io(c: &mut Ctx, write: bool) -> u32 {
    let (hf, psem, perr, buf, cb, pn) = (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3), c.arg(4), c.ptr(5));
    let (e, n) = if write { write_handle(c, hf, buf, cb) } else { read_handle(c, hf, buf, cb) };
    c.write_u16(perr, e as u16);
    c.write_u16(pn, n as u16);
    super::sem::clear_ram16(c, psem);
    0
}

pub fn dos16_read_async(c: &mut Ctx) -> u32 {
    async_io(c, false)
}

pub fn dos16_write_async(c: &mut Ctx) -> u32 {
    async_io(c, true)
}

pub fn dos16_write(c: &mut Ctx) -> u32 {
    let (e, n) = write_handle(c, c.arg(0), c.ptr(1), c.arg(2));
    c.write_u16(c.ptr(3), n as u16);
    e
}

pub fn dos_write(c: &mut Ctx) -> u32 {
    let (e, n) = write_handle(c, c.arg(0), c.arg(1), c.arg(2));
    c.write_u32(c.arg(3), n);
    e
}

pub fn dos16_read(c: &mut Ctx) -> u32 {
    let (e, n) = read_handle(c, c.arg(0), c.ptr(1), c.arg(2));
    c.write_u16(c.ptr(3), n as u16);
    e
}

pub fn dos_read(c: &mut Ctx) -> u32 {
    let (e, n) = read_handle(c, c.arg(0), c.arg(1), c.arg(2));
    c.write_u32(c.arg(3), n);
    e
}

pub fn dos_close(c: &mut Ctx) -> u32 {
    let hf = c.arg(0) as usize;
    let old = {
        let mut hs = c.process.handles.lock();
        match hs.get_mut(hf) {
            Some(slot @ Some(_)) => slot.take(),
            _ => return err::INVALID_HANDLE,
        }
    };
    c.process.noinherit.lock().remove(&(hf as u32));
    drop(old);
    0
}

fn seek(c: &Ctx, hf: u32, off: i64, method: u32) -> Result<u64, u32> {
    let Some(h) = get(c, hf) else { return Err(err::INVALID_HANDLE) };
    match h {
        Handle::File(f) => {
            let mut pos = f.pos.lock();
            let base = match method {
                0 => 0i64,
                1 => *pos as i64,
                2 => f.file.size() as i64,
                _ => return Err(err::INVALID_FUNCTION),
            };
            let np = base + off;
            if np < 0 {
                return Err(err::NEGATIVE_SEEK);
            }
            *pos = np as u64;
            Ok(np as u64)
        }
        Handle::Drive(d) => {
            let mut pos = d.pos.lock();
            let base = match method {
                0 => 0i64,
                1 => *pos as i64,
                2 => (d.dev.sector_count() * d.dev.sector_size() as u64) as i64,
                _ => return Err(err::INVALID_FUNCTION),
            };
            if base + off < 0 {
                return Err(err::NEGATIVE_SEEK);
            }
            *pos = (base + off) as u64;
            Ok(*pos)
        }
        Handle::Pipe(_) | Handle::NPipe(_) => Err(err::SEEK_ON_DEVICE),
        _ => Ok(0),
    }
}

pub fn dos_set_file_ptr(c: &mut Ctx) -> u32 {
    match seek(c, c.arg(0), c.arg(1) as i32 as i64, c.arg(2)) {
        Ok(p) => {
            c.write_u32(c.arg(3), p as u32);
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_chg_file_ptr(c: &mut Ctx) -> u32 {
    match seek(c, c.arg(0), c.arg(1) as i32 as i64, c.arg(2)) {
        Ok(p) => {
            c.write_u32(c.ptr(3), p as u32);
            0
        }
        Err(e) => e,
    }
}

pub fn dos_set_file_ptr_l(c: &mut Ctx) -> u32 {
    // DosSetFilePtrL(hf, LONGLONG ib, method, PLONGLONG pibActual)
    let off = (c.arg(1) as u64 | (c.arg(2) as u64) << 32) as i64;
    match seek(c, c.arg(0), off, c.arg(3)) {
        Ok(p) => {
            let a = c.arg(4);
            c.write_u32(a, p as u32);
            c.write_u32(a + 4, (p >> 32) as u32);
            0
        }
        Err(e) => e,
    }
}

fn set_size(c: &Ctx, hf: u32, size: u64) -> u32 {
    match get(c, hf) {
        Some(Handle::File(f)) => {
            if f.access() == mode::READONLY {
                return err::ACCESS_DENIED;
            }
            f.file.set_size(size).err().unwrap_or(0)
        }
        Some(_) => err::INVALID_HANDLE,
        None => err::INVALID_HANDLE,
    }
}

pub fn dos_set_file_size(c: &mut Ctx) -> u32 {
    set_size(c, c.arg(0), c.arg(1) as u64)
}

pub fn dos_set_file_size_l(c: &mut Ctx) -> u32 {
    set_size(c, c.arg(0), c.arg(1) as u64 | (c.arg(2) as u64) << 32)
}

pub fn dos_reset_buffer(c: &mut Ctx) -> u32 {
    let hf = c.arg(0);
    if hf == 0xffff || hf == 0xffff_ffff {
        return 0; // flush all
    }
    match get(c, hf) {
        Some(Handle::File(f)) => f.file.flush().err().unwrap_or(0),
        Some(_) => 0,
        None => err::INVALID_HANDLE,
    }
}

// ---- Open ----

/// Result of an open: the handle and the action taken.
fn open(c: &Ctx, name: &str, size: u64, attrib: u32, flags: u32, omode: u32) -> Result<(u32, u32), u32> {
    if let Some(pn) = crate::npipe::pipe_name(name) {
        let end = crate::npipe::open(&pn)?;
        return Ok((add(c, Handle::NPipe(end))?, 1));
    }
    if let Some(d) = fs::device_name(name) {
        let h = add(c, Handle::open_device(d)?)?;
        return Ok((h, 1));
    }
    if omode & mode::DASD != 0 {
        return open_dasd(c, name);
    }
    let p = resolve(c, name)?;
    if p.parts.is_empty() {
        return Err(err::ACCESS_DENIED);
    }
    let fsys = fs::fs_of(&p)?;
    check_names(fsys.as_ref(), &p)?;
    let rel = p.rel();
    let access = omode & mode::ACCESS_MASK;
    if access > mode::READWRITE {
        return Err(err::INVALID_ACCESS);
    }
    let (file, action) = match fsys.stat(&rel) {
        Ok(info) => {
            if info.is_dir() {
                return Err(err::ACCESS_DENIED);
            }
            match flags & 0x0f {
                0 => return Err(err::OPEN_FAILED),
                1 => {
                    if info.attr & attr::READONLY != 0 && access != mode::READONLY {
                        return Err(err::ACCESS_DENIED);
                    }
                    (fsys.open(&rel)?, 1)
                }
                2 => {
                    if info.attr & attr::READONLY != 0 {
                        return Err(err::ACCESS_DENIED);
                    }
                    let f = fsys.open(&rel)?;
                    f.set_size(size)?;
                    f.set_info(&SetInfo { attr: Some(attrib | attr::ARCHIVED), ..Default::default() })?;
                    (f, 3)
                }
                _ => return Err(err::INVALID_PARAMETER),
            }
        }
        Err(e) if e == err::FILE_NOT_FOUND => match flags & 0xf0 {
            0 => return Err(err::OPEN_FAILED),
            0x10 => {
                let f = fsys.create(&rel, attrib)?;
                if size > 0 {
                    f.set_size(size)?;
                }
                (f, 2)
            }
            _ => return Err(err::INVALID_PARAMETER),
        },
        Err(e) => return Err(e),
    };
    let of = OpenFile::new(p, file, omode)?;
    let h = add(c, Handle::File(of))?;
    if omode & mode::NOINHERIT != 0 {
        c.process.noinherit.lock().insert(h);
    }
    Ok((h, action))
}

/// OPEN_FLAGS_DASD: `"D:"` opens the volume of drive D: as sectors.
fn open_dasd(c: &Ctx, name: &str) -> Result<(u32, u32), u32> {
    let b = name.as_bytes();
    if b.len() != 2 || b[1] != b':' || !b[0].is_ascii_alphabetic() {
        return Err(err::INVALID_PARAMETER);
    }
    let d = b[0].to_ascii_uppercase() - b'A';
    if fs::locked_by_other(d) {
        return Err(err::DRIVE_LOCKED);
    }
    let fsys = fs::drive(d).ok_or(err::INVALID_DRIVE)?;
    let dev = fsys.device().ok_or(err::NOT_SUPPORTED)?;
    Ok((add(c, Handle::Drive(fs::Dasd::new(d, dev)))?, 1))
}

/// DosRead/DosWrite on a DASD handle: bytes of the volume at the file
/// pointer (sector-granular underneath).
fn dasd_rw(c: &Ctx, d: &fs::Dasd, buf: u32, len: u32, write: bool) -> (u32, u32) {
    let ss = d.dev.sector_size() as u64;
    let size = d.dev.sector_count() * ss;
    let mut pos = d.pos.lock();
    let len = (len as u64).min(size.saturating_sub(*pos)) as usize;
    if len == 0 {
        return (0, 0);
    }
    let first = *pos / ss;
    let last = (*pos + len as u64).div_ceil(ss);
    let mut data = alloc::vec![0u8; ((last - first) * ss) as usize];
    if let Err(e) = d.dev.read(first, &mut data) {
        return (e, 0);
    }
    let at = (*pos - first * ss) as usize;
    if write {
        if !c.read(buf, &mut data[at..at + len]) {
            return (err::INVALID_ADDRESS, 0);
        }
        if let Err(e) = d.dev.write(first, &data) {
            return (e, 0);
        }
        d.written.store(true, core::sync::atomic::Ordering::Relaxed);
    } else if !c.write(buf, &data[at..at + len]) {
        return (err::INVALID_ADDRESS, 0);
    }
    *pos += len as u64;
    (0, len as u32)
}

pub fn dos_open(c: &mut Ctx) -> u32 {
    // DosOpen(psz, phf, pulAction, cbFile, ulAttribute, fsOpenFlags, fsOpenMode, peaop2)
    let name = match read_path(c, c.arg(0)) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match open(c, &name, c.arg(3) as u64, c.arg(4), c.arg(5), c.arg(6)) {
        Ok((h, act)) => {
            c.write_u32(c.arg(1), h);
            c.write_u32(c.arg(2), act);
            0
        }
        Err(e) => e,
    }
}

pub fn dos_open_l(c: &mut Ctx) -> u32 {
    // DosOpenL(psz, phf, pulAction, LONGLONG cbFile, ulAttribute, flags, mode, peaop2)
    let name = match read_path(c, c.arg(0)) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let size = c.arg(3) as u64 | (c.arg(4) as u64) << 32;
    match open(c, &name, size, c.arg(5), c.arg(6), c.arg(7)) {
        Ok((h, act)) => {
            c.write_u32(c.arg(1), h);
            c.write_u32(c.arg(2), act);
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_open(c: &mut Ctx) -> u32 {
    // DosOpen(psz, phf, pusAction, ulFSize, usAttr, fsOpenFlags, fsOpenMode, ulReserved)
    let name = match read_path(c, c.ptr(0)) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match open(c, &name, c.arg(3) as u64, c.arg(4), c.arg(5), c.arg(6)) {
        Ok((h, act)) => {
            c.write_u16(c.ptr(1), h as u16);
            c.write_u16(c.ptr(2), act as u16);
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_open_l(c: &mut Ctx) -> u32 {
    // Dos16OpenL(psz, phf, pusAction, LONGLONG cbFile, usAttr, fsOpenFlags,
    //            ULONG fsOpenMode, peaop2, reserved)
    let name = match read_path(c, c.ptr(0)) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let size = c.arg(3) as u64 | (c.arg(4) as u64) << 32;
    match open(c, &name, size, c.arg(5), c.arg(6), c.arg(7)) {
        Ok((h, act)) => {
            c.write_u16(c.ptr(1), h as u16);
            c.write_u16(c.ptr(2), act as u16);
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_set_file_ptr_l(c: &mut Ctx) -> u32 {
    // Dos16SetFilePtrL(hf, LONGLONG ib, ULONG method, PLONGLONG pibActual)
    // The LONGLONG arrives as two dwords, low part first (CMD.EXE seeks
    // back with 0xfffffff1, 0xffffffff).
    let off = (c.arg(1) as u64 | (c.arg(2) as u64) << 32) as i64;
    match seek(c, c.arg(0), off, c.arg(3)) {
        Ok(p) => {
            let a = c.ptr(4);
            c.write_u32(a, p as u32);
            c.write_u32(a + 4, (p >> 32) as u32);
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_set_file_size_l(c: &mut Ctx) -> u32 {
    set_size(c, c.arg(0), c.arg(1) as u64 | (c.arg(2) as u64) << 32)
}

pub fn dos16_open2(c: &mut Ctx) -> u32 {
    // DosOpen2(psz, phf, pusAction, ulFSize, usAttr, usOpenFlags, ulOpenMode, pEABuf, ulReserved)
    let name = match read_path(c, c.ptr(0)) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match open(c, &name, c.arg(3) as u64, c.arg(4), c.arg(5), c.arg(6)) {
        Ok((h, act)) => {
            c.write_u16(c.ptr(1), h as u16);
            c.write_u16(c.ptr(2), act as u16);
            0
        }
        Err(e) => e,
    }
}

// ---- Handle state ----

pub fn dos_dup_handle(c: &mut Ctx) -> u32 {
    let (old, pnew) = (c.arg(0), if c.is16 { c.ptr(1) } else { c.arg(1) });
    let Some(h) = get(c, old) else { return err::INVALID_HANDLE };
    let want = if c.is16 { c.read_u16(pnew).map(|v| if v == 0xffff { u32::MAX } else { v as u32 }) } else { c.read_u32(pnew) };
    let Some(want) = want else { return err::INVALID_ADDRESS };
    let new = if want == u32::MAX {
        match add(c, h) {
            Ok(n) => n,
            Err(e) => return e,
        }
    } else {
        if want as usize >= MAX_HANDLES {
            return err::INVALID_TARGET_HANDLE;
        }
        let old_h = {
            let mut hs = c.process.handles.lock();
            if hs.len() <= want as usize {
                hs.resize(want as usize + 1, None);
            }
            hs[want as usize].replace(h)
        };
        drop(old_h);
        want
    };
    // A duplicate does not take over the no-inherit mark.
    c.process.noinherit.lock().remove(&new);
    if c.is16 { c.write_u16(pnew, new as u16) } else { c.write_u32(pnew, new) };
    0
}

fn fh_state(c: &Ctx, hf: u32) -> Result<u32, u32> {
    let base = match get(c, hf).ok_or(err::INVALID_HANDLE)? {
        Handle::File(f) => f.mode.load(core::sync::atomic::Ordering::Relaxed) & 0x7fff,
        Handle::Pipe(p) => p.mode(),
        Handle::NPipe(p) => p.open_mode() & 0x3 | mode::DENYNONE,
        _ => mode::READWRITE | mode::DENYNONE,
    };
    let ni = if c.process.noinherit.lock().contains(&hf) { mode::NOINHERIT } else { 0 };
    Ok((base & !mode::NOINHERIT) | ni)
}

pub fn dos_query_fh_state(c: &mut Ctx) -> u32 {
    match fh_state(c, c.arg(0)) {
        Ok(m) => {
            if c.is16 { c.write_u16(c.ptr(1), m as u16) } else { c.write_u32(c.arg(1), m) };
            0
        }
        Err(e) => e,
    }
}

pub fn dos_set_fh_state(c: &mut Ctx) -> u32 {
    let (hf, state) = (c.arg(0), c.arg(1));
    if get(c, hf).is_some() {
        let mut ni = c.process.noinherit.lock();
        if state & mode::NOINHERIT != 0 {
            ni.insert(hf);
        } else {
            ni.remove(&hf);
        }
    }
    // Only the inheritance, write-through and fail-on-error bits change.
    let changeable = mode::NOINHERIT | mode::WRITE_THROUGH | mode::FAILONERROR | 0x1000;
    match get(c, hf) {
        Some(Handle::File(f)) => {
            let old = f.mode.load(core::sync::atomic::Ordering::Relaxed);
            f.mode.store((old & !changeable) | (state & changeable), core::sync::atomic::Ordering::Relaxed);
            0
        }
        Some(Handle::Pipe(p)) => {
            p.set_mode(state);
            0
        }
        Some(_) => 0,
        None => err::INVALID_HANDLE,
    }
}

/// (type, device attribute) for DosQHandType / DosQueryHType.
pub fn htype(c: &Ctx, hf: u32) -> Option<(u32, u32)> {
    Some(match get(c, hf)? {
        // HANDTYPE_DEVICE with the character device attribute word.
        Handle::Con => (1, 0x8083),
        Handle::Kbd => (1, 0x8081),
        Handle::Screen => (1, 0x8082),
        Handle::Nul => (1, 0x8084),
        Handle::Clock => (1, 0x8088),
        Handle::File(_) => (0, 0),
        Handle::Pipe(_) | Handle::NPipe(_) => (2, 0),
        Handle::PhysDisk(_) => (1, 0x8000),
        Handle::Drive(_) => (0, 0),
        Handle::DosDev => (1, 0x8080),
        Handle::Char(_) => (1, 0x8080),
    })
}

// ---- Directories and drives ----

pub fn dos_query_current_disk(c: &mut Ctx) -> u32 {
    let d = c.process.cwd.lock().0 as u32 + 1;
    let map = fs::drive_map();
    if c.is16 {
        c.write_u16(c.ptr(0), d as u16);
        c.write_u32(c.ptr(1), map);
    } else {
        c.write_u32(c.arg(0), d);
        c.write_u32(c.arg(1), map);
    }
    0
}

pub fn dos_set_default_disk(c: &mut Ctx) -> u32 {
    let d = c.arg(0);
    if d == 0 || d > 26 || fs::drive(d as u8 - 1).is_none() {
        return err::INVALID_DRIVE;
    }
    c.process.cwd.lock().0 = d as u8 - 1;
    0
}

pub fn dos_query_current_dir(c: &mut Ctx) -> u32 {
    let (d, buf, pcb) = if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let dir = {
        let cwd = c.process.cwd.lock();
        let drv = if d == 0 { cwd.0 } else { d as u8 - 1 };
        if drv >= 26 || fs::drive(drv).is_none() {
            return err::INVALID_DRIVE;
        }
        cwd.1[drv as usize].clone()
    };
    let have = if c.is16 { c.read_u16(pcb).map(|v| v as u32) } else { c.read_u32(pcb) };
    let Some(have) = have else { return err::INVALID_ADDRESS };
    let need = dir.len() as u32 + 1;
    if have < need {
        if c.is16 { c.write_u16(pcb, need as u16) } else { c.write_u32(pcb, need) };
        return err::BUFFER_OVERFLOW;
    }
    c.write_str(buf, &dir);
    0
}

pub fn dos_set_current_dir(c: &mut Ctx) -> u32 {
    let a = if c.is16 { c.ptr(0) } else { c.arg(0) };
    let name = match read_path(c, a) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let p = match resolve(c, &name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match fs::stat(&p) {
        Ok(i) if i.is_dir() || p.parts.is_empty() => {
            c.process.cwd.lock().1[p.drive as usize] = p.rel();
            0
        }
        Ok(_) => err::PATH_NOT_FOUND,
        Err(e) if e == err::FILE_NOT_FOUND => err::PATH_NOT_FOUND,
        Err(e) => e,
    }
}

fn path_op(c: &Ctx, arg: usize, op: impl FnOnce(&dyn fs::FileSystem, &FullPath) -> Result<(), u32>) -> u32 {
    let a = if c.is16 { c.ptr(arg) } else { c.arg(arg) };
    let name = match read_path(c, a) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if fs::device_name(&name).is_some() {
        return err::ACCESS_DENIED;
    }
    let p = match resolve(c, &name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if super::tracing() {
        crate::trace!("    path {}", p.display());
    }
    let fsys = match fs::fs_of(&p) {
        Ok(f) => f,
        Err(e) => return e,
    };
    if let Err(e) = check_names(fsys.as_ref(), &p) {
        return e;
    }
    op(fsys.as_ref(), &p).err().unwrap_or(0)
}

pub fn dos_delete(c: &mut Ctx) -> u32 {
    path_op(c, 0, |f, p| {
        if path::has_wildcards(p.file_name()) {
            return Err(err::FILE_NOT_FOUND);
        }
        f.delete(&p.rel())
    })
}

pub fn dos_create_dir(c: &mut Ctx) -> u32 {
    path_op(c, 0, |f, p| f.mkdir(&p.rel()))
}

pub fn dos_delete_dir(c: &mut Ctx) -> u32 {
    let cur: Vec<(u8, String)> = {
        let cwd = c.process.cwd.lock();
        (0..26u8).map(|d| (d, cwd.1[d as usize].to_ascii_uppercase())).collect()
    };
    path_op(c, 0, |f, p| {
        if cur.iter().any(|(d, s)| *d == p.drive && *s == p.rel().to_ascii_uppercase()) {
            return Err(err::CURRENT_DIRECTORY);
        }
        f.rmdir(&p.rel())
    })
}

pub fn dos_move(c: &mut Ctx) -> u32 {
    let b = if c.is16 { c.ptr(1) } else { c.arg(1) };
    let to = match read_path(c, b) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let to = match resolve(c, &to) {
        Ok(p) => p,
        Err(e) => return e,
    };
    path_op(c, 0, |f, p| {
        if p.drive != to.drive {
            return Err(err::NOT_SAME_DEVICE);
        }
        f.rename(&p.rel(), &to.rel())
    })
}

// ---- Information ----

/// FILESTATUS (16-bit, level 1) / FILESTATUS3 (32-bit, level 1), with
/// cbList appended for level 2. Levels 11 and 12 are the large-file
/// variants (FILESTATUS3L/4L) with 64-bit sizes and a 32-bit attribute.
fn status_bytes(i: &FileInfo, is16: bool, level: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(40);
    for s in [i.created, i.accessed, i.modified] {
        b.extend_from_slice(&s.date.to_le_bytes());
        b.extend_from_slice(&s.time.to_le_bytes());
    }
    let large = level >= 11;
    if large {
        b.extend_from_slice(&i.size.to_le_bytes());
        b.extend_from_slice(&i.alloc.to_le_bytes());
    } else {
        b.extend_from_slice(&(i.size.min(u32::MAX as u64) as u32).to_le_bytes());
        b.extend_from_slice(&(i.alloc.min(u32::MAX as u64) as u32).to_le_bytes());
    }
    if is16 && !large {
        b.extend_from_slice(&(i.attr as u16).to_le_bytes());
    } else {
        b.extend_from_slice(&i.attr.to_le_bytes());
    }
    if level == 2 || level == 12 {
        // cbList: size of an empty FEA2LIST.
        b.extend_from_slice(&(if i.ea_size == 0 { 4u32 } else { i.ea_size }).to_le_bytes());
    }
    b
}

fn write_status(c: &Ctx, info: &FileInfo, level: u32, buf: u32, cb: u32) -> u32 {
    match level {
        1 | 2 | 11 | 12 => {
            let b = status_bytes(info, c.is16, level);
            if (cb as usize) < b.len() {
                return err::BUFFER_OVERFLOW;
            }
            if c.write(buf, &b) { 0 } else { err::INVALID_ADDRESS }
        }
        3 | 4 => {
            // FIL_QUERYEASFROMLIST / FIL_QUERYALLEAS: EAOP2 -> report no
            // EAs (empty FEA2LIST).
            let eaop = buf;
            let Some(fpfea2) = c.read_u32(eaop + 4) else { return err::INVALID_ADDRESS };
            let p = if c.is16 { c.far16_to_flat(fpfea2) } else { fpfea2 };
            if p != 0 {
                c.write_u32(p, 4);
            }
            0
        }
        _ => {
            if super::tracing() {
                crate::trace!("    info level {level} not supported");
            }
            err::INVALID_LEVEL
        }
    }
}

pub fn dos_query_path_info(c: &mut Ctx) -> u32 {
    // DosQueryPathInfo(psz, level, pInfo, cbInfo)
    let (pa, level, buf, cb) = if c.is16 { (c.ptr(0), c.arg(1), c.ptr(2), c.arg(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    let name = match read_path(c, pa) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let p = match resolve(c, &name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if level == 5 {
        // FIL_QUERYFULLNAME
        let full = p.display();
        if full.len() + 1 > cb as usize {
            return err::BUFFER_OVERFLOW;
        }
        c.write_str(buf, &full);
        return 0;
    }
    if level == 6 {
        // FIL_NAMEISVALID: the name resolved.
        return 0;
    }
    if path::has_wildcards(&name) {
        return err::INVALID_NAME;
    }
    let info = match fs::stat(&p) {
        Ok(i) => i,
        Err(e) => return e,
    };
    write_status(c, &info, level, buf, cb)
}

pub fn dos_query_file_info(c: &mut Ctx) -> u32 {
    let (hf, level, buf, cb) = if c.is16 { (c.arg(0), c.arg(1), c.ptr(2), c.arg(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    let info = match get(c, hf) {
        Some(Handle::File(f)) => f.file.info(),
        Some(_) => FileInfo { attr: 0, ..Default::default() },
        None => return err::INVALID_HANDLE,
    };
    write_status(c, &info, level, buf, cb)
}

/// FILESTATUS (level 1) or FILESTATUS3L (level 11, 64-bit sizes: the
/// attributes move from offset 20 to 28).
fn parse_set_info(c: &Ctx, buf: u32, cb: u32, level: u32) -> Option<SetInfo> {
    let raw = c.read_vec(buf, cb.min(32))?;
    if raw.len() < 12 {
        return None;
    }
    let st = |i: usize| Stamp { date: u16::from_le_bytes([raw[i], raw[i + 1]]), time: u16::from_le_bytes([raw[i + 2], raw[i + 3]]) };
    let attr_off = if level == 11 { 28 } else { 20 };
    let attr = if raw.len() >= attr_off + 2 { Some(u16::from_le_bytes([raw[attr_off], raw[attr_off + 1]]) as u32) } else { None };
    Some(SetInfo { attr, created: Some(st(0)), accessed: Some(st(4)), modified: Some(st(8)) })
}

pub fn dos_set_path_info(c: &mut Ctx) -> u32 {
    // DosSetPathInfo(psz, level, pInfo, cbInfo, flOptions)
    let (level, buf, cb) = if c.is16 { (c.arg(1), c.ptr(2), c.arg(3)) } else { (c.arg(1), c.arg(2), c.arg(3)) };
    if level == 2 || level == 12 {
        return 0; // EAs: accepted, not stored yet
    }
    if level != 1 && level != 11 {
        return err::INVALID_LEVEL;
    }
    let Some(info) = parse_set_info(c, buf, cb, level) else { return err::INVALID_ADDRESS };
    path_op(c, 0, |f, p| f.set_info(&p.rel(), &info))
}

pub fn dos_set_file_info(c: &mut Ctx) -> u32 {
    let (hf, level, buf, cb) = if c.is16 { (c.arg(0), c.arg(1), c.ptr(2), c.arg(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    if level == 2 || level == 12 {
        return 0;
    }
    if level != 1 && level != 11 {
        return err::INVALID_LEVEL;
    }
    let Some(info) = parse_set_info(c, buf, cb, level) else { return err::INVALID_ADDRESS };
    match get(c, hf) {
        Some(Handle::File(f)) => f.file.set_info(&info).err().unwrap_or(0),
        Some(_) => 0,
        None => err::INVALID_HANDLE,
    }
}

pub fn dos16_qfile_mode(c: &mut Ctx) -> u32 {
    let name = match read_path(c, c.ptr(0)) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let p = match resolve(c, &name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match fs::stat(&p) {
        Ok(i) => {
            c.write_u16(c.ptr(1), i.attr as u16);
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_set_file_mode(c: &mut Ctx) -> u32 {
    let a = c.arg(1);
    if a & !0x27 != 0 {
        return err::ACCESS_DENIED;
    }
    path_op(c, 0, |f, p| f.set_info(&p.rel(), &SetInfo { attr: Some(a), ..Default::default() }))
}

// ---- File system information ----

fn fs_drive(c: &Ctx, d: u32) -> Result<(u8, Arc<dyn fs::FileSystem>), u32> {
    let drv = if d == 0 { c.process.cwd.lock().0 } else { (d - 1) as u8 };
    Ok((drv, fs::drive(drv).ok_or(err::INVALID_DRIVE)?))
}

pub fn dos_query_fs_info(c: &mut Ctx) -> u32 {
    let (d, level, buf, cb) = if c.is16 { (c.arg(0), c.arg(1), c.ptr(2), c.arg(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    let (_, f) = match fs_drive(c, d) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let v = match f.volume() {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut b = Vec::new();
    match level {
        1 => {
            // FSALLOCATE: idFileSystem, cSectorUnit, cUnit, cUnitAvail, cbSector(u16)
            b.extend_from_slice(&v.serial.to_le_bytes());
            b.extend_from_slice(&v.sectors_per_unit.to_le_bytes());
            b.extend_from_slice(&v.units.to_le_bytes());
            b.extend_from_slice(&v.free_units.to_le_bytes());
            b.extend_from_slice(&(v.bytes_per_sector as u16).to_le_bytes());
        }
        2 => {
            // FSINFO: ULONG serial number (the FDATE/FTIME of 1.x), then
            // VOLUMELABEL {cch, szVolLabel[12]}.
            b.extend_from_slice(&v.serial.to_le_bytes());
            b.push(v.label.len() as u8);
            let mut l = [0u8; 12];
            for (d, s) in l.iter_mut().zip(v.label.bytes()) {
                *d = s;
            }
            b.extend_from_slice(&l);
        }
        _ => return err::INVALID_LEVEL,
    }
    if (cb as usize) < b.len() {
        return err::BUFFER_OVERFLOW;
    }
    if c.write(buf, &b) { 0 } else { err::INVALID_ADDRESS }
}

pub fn dos_set_fs_info(c: &mut Ctx) -> u32 {
    let (d, level, buf) = if c.is16 { (c.arg(0), c.arg(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    if level != 2 {
        return err::INVALID_LEVEL;
    }
    let (_, f) = match fs_drive(c, d) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let Some(n) = c.read_u8(buf) else { return err::INVALID_ADDRESS };
    let Some(l) = c.read_vec(buf + 1, n as u32) else { return err::INVALID_ADDRESS };
    let s: String = l.iter().map(|&b| b as char).collect();
    f.set_label(&s).err().unwrap_or(0)
}

pub fn dos_query_fs_attach(c: &mut Ctx) -> u32 {
    // DosQueryFSAttach(pszDeviceName, ulOrdinal, ulFSAInfoLevel, pfsqb, pcbBuffLength)
    let (pdev, level, buf, pcb) = if c.is16 { (c.ptr(0), c.arg(2), c.ptr(3), c.ptr(4)) } else { (c.arg(0), c.arg(2), c.arg(3), c.arg(4)) };
    if level != 1 {
        return err::INVALID_LEVEL;
    }
    let Some(dev) = c.read_str(pdev, 16) else { return err::INVALID_ADDRESS };
    let b = dev.as_bytes();
    if b.len() < 2 || b[1] != b':' {
        return err::INVALID_DRIVE;
    }
    let drv = b[0].to_ascii_uppercase().wrapping_sub(b'A');
    let Some(f) = fs::drive(drv) else { return err::INVALID_DRIVE };
    // FSQBUFFER2: iType(u16) cbName(u16) cbFSDName(u16) cbFSAData(u16)
    // szName szFSDName rgFSAData
    let name = alloc::format!("{}:", (b'A' + drv) as char);
    let fsd = f.name();
    let mut out = Vec::new();
    out.extend_from_slice(&3u16.to_le_bytes()); // FSAT_LOCALDRV
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    if c.is16 {
        // DosQFSAttach fills an FSQBUFFER: each length precedes its string.
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(&(fsd.len() as u16).to_le_bytes());
        out.extend_from_slice(fsd.as_bytes());
        out.push(0);
        out.extend_from_slice(&0u16.to_le_bytes());
    } else {
        out.extend_from_slice(&(fsd.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(fsd.as_bytes());
        out.push(0);
        out.push(0);
    }
    let have = if c.is16 { c.read_u16(pcb).map(|v| v as u32) } else { c.read_u32(pcb) }.unwrap_or(0);
    if (have as usize) < out.len() {
        return err::BUFFER_OVERFLOW;
    }
    c.write(buf, &out);
    if c.is16 { c.write_u16(pcb, out.len() as u16) } else { c.write_u32(pcb, out.len() as u32) };
    0
}

// ---- Find ----

pub struct Search {
    results: Vec<FileInfo>,
    next: usize,
    level: u32,
}

/// Collect directory entries matching `spec` and the attribute rules.
fn search(c: &Ctx, spec: &str, attrib: u32) -> Result<Vec<FileInfo>, u32> {
    if spec.is_empty() {
        return Err(err::NO_MORE_FILES);
    }
    let p = resolve(c, spec)?;
    let fsys = fs::fs_of(&p)?;
    let pattern = String::from(p.file_name());
    let dir = p.parent();
    let must = (attrib >> 8) & 0x37;
    let allowed = attrib & 0x37;
    let ok = |i: &FileInfo| {
        let special = i.attr & (attr::HIDDEN | attr::SYSTEM | attr::DIRECTORY);
        special & !allowed == 0 && i.attr & must == must
    };
    if pattern.is_empty() {
        return Err(err::NO_MORE_FILES);
    }
    if !path::has_wildcards(&pattern) {
        let mut i = fsys.stat(&p.rel()).map_err(|e| if e == err::FILE_NOT_FOUND { err::NO_MORE_FILES } else { e })?;
        if i.name.is_empty() {
            i.name = pattern;
        }
        return if ok(&i) { Ok(alloc::vec![i]) } else { Err(err::NO_MORE_FILES) };
    }
    let mut list = fsys.list(&dir.rel())?;
    // "." and ".." for subdirectories, as OS/2 reports them.
    if !dir.parts.is_empty() && allowed & attr::DIRECTORY != 0 {
        if let Ok(d) = fsys.stat(&dir.rel()) {
            for n in ["..", "."] {
                let mut e = d.clone();
                e.name = String::from(n);
                e.size = 0;
                e.alloc = 0;
                list.insert(0, e);
            }
        }
    }
    let r: Vec<FileInfo> = list.into_iter().filter(|i| path::matches(&pattern, &i.name) && ok(i)).collect();
    if r.is_empty() {
        return Err(err::NO_MORE_FILES);
    }
    Ok(r)
}

/// Encode entries into the caller's buffer; returns (bytes, count).
fn fill(c: &Ctx, s: &mut Search, buf: u32, cb: u32, max: u32) -> Result<u32, u32> {
    let mut out: Vec<u8> = Vec::new();
    let mut n = 0u32;
    let mut last_off: Option<usize> = None;
    while (s.next < s.results.len()) && n < max {
        let i = &s.results[s.next];
        let name = i.name.as_bytes();
        let mut e = Vec::new();
        if c.is16 && s.level < 11 {
            e.extend_from_slice(&status_bytes(i, true, s.level));
            e.push(name.len() as u8);
            e.extend_from_slice(name);
            e.push(0);
        } else {
            e.extend_from_slice(&0u32.to_le_bytes()); // oNextEntryOffset
            e.extend_from_slice(&status_bytes(i, false, s.level));
            e.push(name.len() as u8);
            e.extend_from_slice(name);
            e.push(0);
            while e.len() % 4 != 0 {
                e.push(0);
            }
        }
        if out.len() + e.len() > cb as usize {
            if n == 0 {
                return Err(err::BUFFER_OVERFLOW);
            }
            break;
        }
        if !c.is16 {
            if let Some(lo) = last_off {
                let delta = (out.len() - lo) as u32;
                out[lo..lo + 4].copy_from_slice(&delta.to_le_bytes());
            }
            last_off = Some(out.len());
        }
        out.extend_from_slice(&e);
        s.next += 1;
        n += 1;
    }
    if !c.write(buf, &out) {
        return Err(err::INVALID_ADDRESS);
    }
    Ok(n)
}

fn find_first(c: &mut Ctx, spec_addr: u32, phdir: u32, attrib: u32, buf: u32, cb: u32, pcount: u32, level: u32) -> u32 {
    if !matches!(level, 1 | 2 | 11 | 12) {
        crate::trace!("DosFindFirst: unsupported level {level}");
        return err::INVALID_LEVEL;
    }
    let spec = match read_path(c, spec_addr) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let want = if c.is16 { c.read_u16(pcount).map(|v| v as u32) } else { c.read_u32(pcount) };
    let Some(want) = want else { return err::INVALID_ADDRESS };
    let req = if c.is16 { c.read_u16(phdir).map(|v| if v == 0xffff { u32::MAX } else { v as u32 }) } else { c.read_u32(phdir) };
    let Some(req) = req else { return err::INVALID_ADDRESS };
    let results = match search(c, &spec, attrib) {
        Ok(r) => r,
        Err(e) => {
            if c.is16 { c.write_u16(pcount, 0) } else { c.write_u32(pcount, 0) };
            return e;
        }
    };
    let mut s = Search { results, next: 0, level };
    let n = match fill(c, &mut s, buf, cb, want.max(1)) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let h = {
        let mut sr = c.process.searches.lock();
        let h = if req == u32::MAX {
            let mut h = 2;
            while sr.contains_key(&h) {
                h += 1;
            }
            h
        } else {
            1 // HDIR_SYSTEM (or a caller-supplied handle is reused)
        };
        sr.insert(h, s);
        h
    };
    if c.is16 {
        c.write_u16(phdir, h as u16);
        c.write_u16(pcount, n as u16);
    } else {
        c.write_u32(phdir, h);
        c.write_u32(pcount, n);
    }
    0
}

pub fn dos_find_first(c: &mut Ctx) -> u32 {
    // DosFindFirst(pszFileSpec, phdir, flAttribute, pfindbuf, cbBuf, pcFileNames, ulInfoLevel)
    let (a0, a1, a2, a3, a4, a5, a6) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5), c.arg(6));
    find_first(c, a0, a1, a2, a3, a4, a5, a6)
}

pub fn dos16_find_first(c: &mut Ctx) -> u32 {
    // DosFindFirst(psz, phdir, usAttr, pffb, cbBuf, pcSearch, ulReserved)
    let (a0, a1, a2, a3, a4, a5) = (c.ptr(0), c.ptr(1), c.arg(2), c.ptr(3), c.arg(4), c.ptr(5));
    find_first(c, a0, a1, a2 & 0x37, a3, a4, a5, 1)
}

pub fn dos16_find_first2(c: &mut Ctx) -> u32 {
    // DosFindFirst2(psz, phdir, usAttr, pBuf, cbBuf, pcSearch, usInfoLevel, ulReserved)
    let (a0, a1, a2, a3, a4, a5, a6) = (c.ptr(0), c.ptr(1), c.arg(2), c.ptr(3), c.arg(4), c.ptr(5), c.arg(6));
    find_first(c, a0, a1, a2 & 0x37, a3, a4, a5, a6)
}

fn find_next(c: &mut Ctx, h: u32, buf: u32, cb: u32, pcount: u32) -> u32 {
    let want = if c.is16 { c.read_u16(pcount).map(|v| v as u32) } else { c.read_u32(pcount) };
    let Some(want) = want else { return err::INVALID_ADDRESS };
    let Some(mut s) = c.process.searches.lock().remove(&h) else { return err::INVALID_HANDLE };
    let r = if s.next >= s.results.len() { Err(err::NO_MORE_FILES) } else { fill(c, &mut s, buf, cb, want.max(1)) };
    c.process.searches.lock().insert(h, s);
    match r {
        Ok(n) => {
            if c.is16 { c.write_u16(pcount, n as u16) } else { c.write_u32(pcount, n) };
            0
        }
        Err(e) => {
            if c.is16 { c.write_u16(pcount, 0) } else { c.write_u32(pcount, 0) };
            e
        }
    }
}

pub fn dos_find_next(c: &mut Ctx) -> u32 {
    let (h, b, cb, pc) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    find_next(c, h, b, cb, pc)
}

pub fn dos16_find_next(c: &mut Ctx) -> u32 {
    let (h, b, cb, pc) = (c.arg(0), c.ptr(1), c.arg(2), c.ptr(3));
    find_next(c, h, b, cb, pc)
}

pub fn dos_find_close(c: &mut Ctx) -> u32 {
    match c.process.searches.lock().remove(&c.arg(0)) {
        Some(_) => 0,
        None => err::INVALID_HANDLE,
    }
}

// ---- Search path ----

pub fn dos_search_path(c: &mut Ctx) -> u32 {
    // DosSearchPath(flSearch, pszPathOrName, pszFilename, pBuf, cbBuf)
    let (flags, ppath, pname, buf, cb) = if c.is16 {
        (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3), c.arg(4))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4))
    };
    let Some(name) = c.read_str(pname, path::MAX_PATH) else { return err::INVALID_ADDRESS };
    let Some(mut list) = c.read_str(ppath, 4096) else { return err::INVALID_ADDRESS };
    if flags & 2 != 0 {
        // SEARCH_ENVIRONMENT: the argument names an environment variable.
        match super::doscalls::env_value(c, &list) {
            Some(v) => list = v,
            None => return err::ENVVAR_NOT_FOUND,
        }
    }
    let mut dirs: Vec<String> = Vec::new();
    if flags & 1 != 0 {
        dirs.push(String::from("."));
    }
    dirs.extend(list.split(';').filter(|s| !s.is_empty()).map(String::from));
    for d in dirs {
        let cand = alloc::format!("{}\\{}", d.trim_end_matches('\\'), name);
        if let Ok(p) = resolve(c, &cand) {
            if let Ok(i) = fs::stat(&p) {
                if !i.is_dir() {
                    let full = p.display();
                    if full.len() + 1 > cb as usize {
                        return err::BUFFER_OVERFLOW;
                    }
                    c.write_str(buf, &full);
                    return 0;
                }
            }
        }
    }
    err::FILE_NOT_FOUND
}

pub fn dos_query_htype_any(c: &mut Ctx) -> u32 {
    let Some((t, a)) = htype(c, c.arg(0)) else { return err::INVALID_HANDLE };
    if c.is16 {
        c.write_u16(c.ptr(1), t as u16);
        c.write_u16(c.ptr(2), a as u16);
    } else {
        c.write_u32(c.arg(1), t);
        c.write_u32(c.arg(2), a);
    }
    0
}

/// Build the device handle for the standard handles of a new session.
pub fn console_handles() -> Vec<Option<Handle>> {
    alloc::vec![Some(Handle::Con), Some(Handle::Con), Some(Handle::Con)]
}

pub fn device_handle(d: Device) -> Handle {
    Handle::device(d)
}

// ---- Pipes ----

fn make_pipe(c: &Ctx, size: u32) -> Result<(u32, u32), u32> {
    let (r, w) = crate::pipe::create(size as usize);
    let hr = add(c, Handle::Pipe(r))?;
    let hw = match add(c, Handle::Pipe(w)) {
        Ok(h) => h,
        Err(e) => {
            c.process.handles.lock()[hr as usize] = None;
            return Err(e);
        }
    };
    Ok((hr, hw))
}

pub fn dos_create_pipe(c: &mut Ctx) -> u32 {
    match make_pipe(c, c.arg(2)) {
        Ok((r, w)) => {
            c.write_u32(c.arg(0), r);
            c.write_u32(c.arg(1), w);
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_make_pipe(c: &mut Ctx) -> u32 {
    match make_pipe(c, c.arg(2)) {
        Ok((r, w)) => {
            c.write_u16(c.ptr(0), r as u16);
            c.write_u16(c.ptr(1), w as u16);
            0
        }
        Err(e) => e,
    }
}

/// Apply a wildcard edit string to a file name component (the target of
/// `COPY *.TXT *.BAK`): `?` takes the source character at that position,
/// `*` the source up to the edit string's next character, `.` moves to
/// the source's next extension.
pub fn edit_name(src: &[u8], edit: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    for (k, &ch) in edit.iter().enumerate() {
        match ch {
            b'?' => {
                if i < src.len() && src[i] != b'.' {
                    out.push(src[i]);
                    i += 1;
                }
            }
            b'*' => {
                let stop = match edit.get(k + 1) {
                    None => src.len(),
                    Some(b'.') => src[i..].iter().rposition(|&c| c == b'.').map_or(src.len(), |p| i + p),
                    Some(&n) => src[i..].iter().position(|&c| c.eq_ignore_ascii_case(&n)).map_or(src.len(), |p| i + p),
                };
                out.extend_from_slice(&src[i..stop]);
                i = stop;
            }
            b'.' => {
                while i < src.len() && src[i] != b'.' {
                    i += 1;
                }
                if i < src.len() {
                    i += 1;
                }
                out.push(b'.');
            }
            c => {
                out.push(c);
                if i < src.len() && src[i] != b'.' {
                    i += 1;
                }
            }
        }
    }
    while out.last() == Some(&b'.') {
        out.pop();
    }
    out
}

/// DosEditName(level, pszSource, pszEdit, pszTarget, cbTarget)
pub fn dos_edit_name(c: &mut Ctx) -> u32 {
    let (level, ps, pe, pt, cb) =
        if c.is16 { (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3), c.arg(4)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4)) };
    if level != 1 {
        return err::INVALID_PARAMETER;
    }
    let (Some(s), Some(e)) = (c.read_str(ps, 260), c.read_str(pe, 260)) else { return err::INVALID_ADDRESS };
    let last = |x: &str| -> Vec<u8> { x.rsplit(['\\', '/', ':']).next().unwrap_or("").bytes().collect() };
    let mut t = edit_name(&last(&s), &last(&e));
    if t.len() + 1 > cb as usize {
        return err::BUFFER_OVERFLOW;
    }
    t.push(0);
    if c.write(pt, &t) { 0 } else { err::INVALID_ADDRESS }
}

/// DosEnumAttribute(ulRefType, pvFile, ulEntry, pvBuf, cbBuf, pulCount,
/// ulInfoLevel): extended attributes are not stored yet, so every file
/// has none (the count comes back 0).
pub fn dos_enum_attribute(c: &mut Ctx) -> u32 {
    let (reftype, pfile, pcount, level) = if c.is16 {
        (c.arg(0), c.arg(1), c.ptr(5), c.arg(6))
    } else {
        (c.arg(0), c.arg(1), c.arg(5), c.arg(6))
    };
    if level != 1 {
        return err::INVALID_LEVEL;
    }
    match reftype {
        // ENUMEA_REFTYPE_FHANDLE: pvFile points at the handle.
        0 => {
            let p = if c.is16 { c.far16_to_flat(pfile) } else { pfile };
            let hf = if c.is16 { c.read_u16(p).map(u32::from) } else { c.read_u32(p) };
            let Some(hf) = hf else { return err::INVALID_ADDRESS };
            if get(c, hf).is_none() {
                return err::INVALID_HANDLE;
            }
        }
        // ENUMEA_REFTYPE_PATH
        1 => {
            let p = if c.is16 { c.far16_to_flat(pfile) } else { pfile };
            let name = match read_path(c, p) {
                Ok(n) => n,
                Err(e) => return e,
            };
            let path = match resolve(c, &name) {
                Ok(p) => p,
                Err(e) => return e,
            };
            if let Err(e) = fs::stat(&path) {
                return e;
            }
        }
        _ => return err::INVALID_PARAMETER,
    }
    if !c.write_u32(pcount, 0) {
        return err::INVALID_ADDRESS;
    }
    0
}

// ---- DosCopy ----

const DCPY_EXISTING: u32 = 1;
const DCPY_APPEND: u32 = 2;

fn child(p: &FullPath, name: &str) -> FullPath {
    let mut c = p.clone();
    c.parts.push(String::from(name));
    c
}

/// Copy a file or a directory tree.
pub fn copy_path(src: &FullPath, dst: &FullPath, opt: u32) -> Result<(), u32> {
    let sfs = fs::fs_of(src)?;
    let dfs = fs::fs_of(dst)?;
    let info = sfs.stat(&src.rel())?;
    if info.is_dir() {
        match dfs.stat(&dst.rel()) {
            Ok(d) if d.is_dir() => {}
            Ok(_) => return Err(err::ACCESS_DENIED),
            Err(_) => dfs.mkdir(&dst.rel())?,
        }
        for e in sfs.list(&src.rel())? {
            if e.name == "." || e.name == ".." {
                continue;
            }
            copy_path(&child(src, &e.name), &child(dst, &e.name), opt)?;
        }
        return Ok(());
    }
    let from = sfs.open(&src.rel())?;
    let existing = dfs.stat(&dst.rel()).ok();
    if existing.as_ref().is_some_and(|d| d.is_dir()) {
        return copy_path(src, &child(dst, src.file_name()), opt);
    }
    let (to, mut at) = match existing {
        Some(_) if opt & DCPY_APPEND != 0 => {
            let f = dfs.open(&dst.rel())?;
            let n = f.size();
            (f, n)
        }
        Some(d) if opt & DCPY_EXISTING != 0 => {
            if d.attr & attr::READONLY != 0 {
                return Err(err::ACCESS_DENIED);
            }
            let f = dfs.open(&dst.rel())?;
            f.set_size(0)?;
            (f, 0)
        }
        Some(_) => return Err(err::ACCESS_DENIED),
        None => (dfs.create(&dst.rel(), info.attr & 0x27)?, 0),
    };
    let mut buf = alloc::vec![0u8; 0x10000];
    let mut off = 0u64;
    loop {
        let n = from.read(off, &mut buf)?;
        if n == 0 {
            break;
        }
        to.write(at, &buf[..n])?;
        off += n as u64;
        at += n as u64;
    }
    let _ = to.set_info(&SetInfo {
        attr: Some(info.attr & 0x27),
        created: Some(info.created),
        accessed: Some(info.accessed),
        modified: Some(info.modified),
    });
    to.flush()
}

/// DosCopy(pszSource, pszTarget, ulOptions)
pub fn dos_copy(c: &mut Ctx) -> u32 {
    let (ps, pd, opt) = if c.is16 { (c.ptr(0), c.ptr(1), c.arg(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let (src, dst) = match (read_path(c, ps), read_path(c, pd)) {
        (Ok(s), Ok(d)) => (s, d),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    if path::has_wildcards(&src) || path::has_wildcards(&dst) {
        return err::INVALID_NAME;
    }
    let (s, d) = match (resolve(c, &src), resolve(c, &dst)) {
        (Ok(s), Ok(d)) => (s, d),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    if super::tracing() {
        crate::trace!("    copy {} -> {}", s.display(), d.display());
    }
    copy_path(&s, &d, opt).err().unwrap_or(0)
}

/// DosReplaceModule(pszOld, pszNew, pszBackup): modules are not locked
/// while in use here, so this is a copy (with an optional backup first).
pub fn dos_replace_module(c: &mut Ctx) -> u32 {
    let (pold, pnew, pbak) = (c.arg(0), c.arg(1), c.arg(2));
    let old = match read_path(c, pold).and_then(|n| resolve(c, &n)) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if pbak != 0 {
        let bak = match read_path(c, pbak).and_then(|n| resolve(c, &n)) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if let Err(e) = copy_path(&old, &bak, DCPY_EXISTING) {
            return e;
        }
    }
    if pnew == 0 {
        return 0;
    }
    let new = match read_path(c, pnew).and_then(|n| resolve(c, &n)) {
        Ok(p) => p,
        Err(e) => return e,
    };
    copy_path(&new, &old, DCPY_EXISTING).err().unwrap_or(0)
}
