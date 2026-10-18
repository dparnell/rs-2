//! DosDevIOCtl and DosPhysicalDisk.
//!
//! Category 9 (physical disk) supports the standard OS/2 functions on a
//! synthetic CHS geometry (255 heads, 63 sectors per track) plus three rs2
//! extensions for LBA access, which tools need for disks larger than CHS
//! can describe:
//!
//! | function | parameters | data |
//! |----------|------------|------|
//! | 0xA0 read  | ULONG lba lo, ULONG lba hi, ULONG count | sectors |
//! | 0xA1 write | same | sectors |
//! | 0xA2 size  | — | ULONGLONG sectors, ULONG bytes per sector |

use alloc::sync::Arc;
use alloc::vec;

use super::{err, Ctx};
use crate::fs::block::BlockDev;
use crate::fs::{CharDev, Handle};

const HEADS: u64 = 255;
const SPT: u64 = 63;

/// Arguments common to the 16- and 32-bit forms.
struct Req {
    handle: u32,
    category: u32,
    function: u32,
    params: u32,
    params_len: u32,
    data: u32,
    data_len: u32,
    /// 32-bit form: where to store the used lengths.
    pcb_params: u32,
    pcb_data: u32,
}

fn disk(n: u32) -> Option<Arc<dyn BlockDev>> {
    crate::drivers::DISKS.lock().get(n as usize).cloned()
}

pub fn dos_physical_disk(c: &mut Ctx) -> u32 {
    // (function, pBuf, cbBuf, pParams, cbParams)
    let (f, buf, cb, params) = if c.is16 {
        (c.arg(0), c.ptr(1), c.arg(2), c.ptr(3))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3))
    };
    match f {
        1 => {
            // INFO_COUNT_PARTITIONABLE_DISKS
            if cb < 2 {
                return err::INVALID_PARAMETER;
            }
            let n = crate::drivers::DISKS.lock().len() as u16;
            c.write_u16(buf, n);
            0
        }
        2 => {
            // INFO_GETIOCTLHANDLE: params "n:" (1-based)
            let Some(s) = c.read_str(params, 8) else { return err::INVALID_ADDRESS };
            let n: u32 = s.trim_end_matches(':').parse().unwrap_or(0);
            if n == 0 || disk(n - 1).is_none() {
                return err::INVALID_PARAMETER;
            }
            match super::file::add(c, Handle::PhysDisk(n - 1)) {
                Ok(h) => {
                    c.write_u16(buf, h as u16);
                    0
                }
                Err(e) => e,
            }
        }
        3 => {
            // INFO_FREEIOCTLHANDLE: params = handle
            let Some(h) = c.read_u16(params) else { return err::INVALID_ADDRESS };
            let mut hs = c.process.handles.lock();
            match hs.get_mut(h as usize) {
                Some(slot @ Some(Handle::PhysDisk(_))) => {
                    *slot = None;
                    0
                }
                _ => err::INVALID_HANDLE,
            }
        }
        _ => err::INVALID_FUNCTION,
    }
}

pub fn dos_dev_ioctl(c: &mut Ctx) -> u32 {
    let r = if c.is16 {
        // DosDevIOCtl(pvData, pvParms, usFunction, usCategory, hDevice)
        Req {
            handle: c.arg(4),
            category: c.arg(3),
            function: c.arg(2),
            params: c.ptr(1),
            params_len: 0xffff,
            data: c.ptr(0),
            data_len: 0xffff,
            pcb_params: 0,
            pcb_data: 0,
        }
    } else {
        // DosDevIOCtl(hDevice, category, function, pParams, cbParmLenMax,
        //             pcbParmLen, pData, cbDataLenMax, pcbDataLen)
        Req {
            handle: c.arg(0),
            category: c.arg(1),
            function: c.arg(2),
            params: c.arg(3),
            params_len: c.arg(4),
            data: c.arg(6),
            data_len: c.arg(7),
            pcb_params: c.arg(5),
            pcb_data: c.arg(8),
        }
    };
    dispatch(c, &r)
}

/// DosDevIOCtl2 (16-bit): DosDevIOCtl with buffer lengths.
pub fn dos16_dev_ioctl2(c: &mut Ctx) -> u32 {
    // (pData, cbData, pParms, cbParms, usFunction, usCategory, hDevice)
    let r = Req {
        handle: c.arg(6),
        category: c.arg(5),
        function: c.arg(4),
        params: c.ptr(2),
        params_len: c.arg(3),
        data: c.ptr(0),
        data_len: c.arg(1),
        pcb_params: 0,
        pcb_data: 0,
    };
    dispatch(c, &r)
}

fn dispatch(c: &mut Ctx, r: &Req) -> u32 {
    let Some(h) = super::file::get(c, r.handle) else { return err::INVALID_HANDLE };
    match (h, r.category) {
        (Handle::PhysDisk(n), 9) => phys(c, n, r),
        (Handle::Drive(d), 8) => logical(c, &d, r),
        (_, 0xd5) => dos_dev(c, r),
        (Handle::Kbd | Handle::Con, 4) => kbd(c, r),
        (Handle::Kbd | Handle::Con, 0xb) => general_kbd(c, r),
        (Handle::Char(CharDev::Com(n)), 1) => {
            let need = match r.function {
                0x41 | 0x46 => 2,
                0x42 => 3,
                0x43 => 5,
                0x44 => 1,
                0x53 => 11,
                _ => 0,
            };
            let params = if need > 0 {
                match c.read_vec(r.params, need) {
                    Some(p) => p,
                    None => return err::INVALID_ADDRESS,
                }
            } else {
                alloc::vec::Vec::new()
            };
            match crate::chardev::com_ioctl(n as usize, r.function, &params) {
                Ok(d) => put_data(c, r, need, &d),
                Err(e) => e,
            }
        }
        (Handle::Char(CharDev::Com(n)), 0xb) => crate::chardev::com_general(n as usize, r.function).err().unwrap_or(0),
        (Handle::Char(CharDev::Lpt(_)), 5) => match crate::chardev::lpt_ioctl(r.function) {
            Ok(d) => put_data(c, r, 0, &d),
            Err(e) => e,
        },
        (Handle::Char(CharDev::Mouse | CharDev::Pointer), 7) => mouse(c, r),
        (Handle::Screen, 3) => screen(c, r),
        _ => err::INVALID_CATEGORY,
    }
}

/// Copy IOCtl result data to the caller (ERROR_BUFFER_OVERFLOW if it does
/// not fit) and report the lengths used.
fn put_data(c: &Ctx, r: &Req, plen: u32, d: &[u8]) -> u32 {
    if d.len() as u32 > r.data_len {
        return err::BUFFER_OVERFLOW;
    }
    if !d.is_empty() && !c.write(r.data, d) {
        return err::INVALID_ADDRESS;
    }
    set_lens(c, r, plen, d.len() as u32);
    0
}

fn set_lens(c: &Ctx, r: &Req, p: u32, d: u32) {
    if r.pcb_params != 0 {
        c.write_u32(r.pcb_params, p);
    }
    if r.pcb_data != 0 {
        c.write_u32(r.pcb_data, d);
    }
}

fn phys(c: &Ctx, n: u32, r: &Req) -> u32 {
    let Some(d) = disk(n) else { return err::INVALID_HANDLE };
    let ss = d.sector_size() as u32;
    match r.function {
        0x63 => {
            // PDSK_GETPHYSDEVICEPARAMS: DEVICEPARAMETERBLOCK
            let sectors = d.sector_count();
            let cyl = (sectors / (HEADS * SPT)).min(0xffff) as u16;
            let mut b = [0u8; 16];
            b[2..4].copy_from_slice(&cyl.to_le_bytes());
            b[4..6].copy_from_slice(&(HEADS as u16).to_le_bytes());
            b[6..8].copy_from_slice(&(SPT as u16).to_le_bytes());
            if !c.write(r.data, &b) {
                return err::INVALID_ADDRESS;
            }
            set_lens(c, r, 1, 16);
            0
        }
        0x64 | 0x44 => {
            // PDSK_READPHYSTRACK / PDSK_WRITEPHYSTRACK:
            // TRACKLAYOUT { bCommand, usHead, usCylinder, usFirstSector,
            //               cSectors, TrackTable[] }
            let Some(p) = c.read_vec(r.params, 9) else { return err::INVALID_ADDRESS };
            let head = u16::from_le_bytes([p[1], p[2]]) as u64;
            let cyl = u16::from_le_bytes([p[3], p[4]]) as u64;
            let first = u16::from_le_bytes([p[5], p[6]]) as u64;
            let count = u16::from_le_bytes([p[7], p[8]]) as u32;
            let lba = (cyl * HEADS + head) * SPT + first;
            rw(c, &d, lba, count, r.data, r.function == 0x44)
        }
        0xa0 | 0xa1 => {
            let Some(p) = c.read_vec(r.params, 12) else { return err::INVALID_ADDRESS };
            let lba = u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as u64
                | (u32::from_le_bytes([p[4], p[5], p[6], p[7]]) as u64) << 32;
            let count = u32::from_le_bytes([p[8], p[9], p[10], p[11]]);
            if count as u64 * ss as u64 > r.data_len as u64 {
                return err::INVALID_PARAMETER;
            }
            let e = rw(c, &d, lba, count, r.data, r.function == 0xa1);
            if e == 0 {
                set_lens(c, r, 12, count * ss);
            }
            e
        }
        0xa2 => {
            let mut b = [0u8; 12];
            b[0..8].copy_from_slice(&d.sector_count().to_le_bytes());
            b[8..12].copy_from_slice(&ss.to_le_bytes());
            if !c.write(r.data, &b) {
                return err::INVALID_ADDRESS;
            }
            set_lens(c, r, 0, 12);
            0
        }
        _ => err::INVALID_FUNCTION,
    }
}

/// Category 8 (logical disk) on a drive opened with OPEN_FLAGS_DASD. Track
/// addresses use the same synthetic geometry as category 9, relative to
/// the start of the volume; the BPB reports it.
fn logical(c: &Ctx, d: &crate::fs::Dasd, r: &Req) -> u32 {
    use core::sync::atomic::Ordering;
    match r.function {
        // DSK_LOCKDRIVE / DSK_UNLOCKDRIVE
        0x00 => match crate::fs::lock_drive(d.drive) {
            Ok(()) => {
                d.locked.store(true, Ordering::Relaxed);
                0
            }
            Err(e) => e,
        },
        0x01 => {
            if d.locked.swap(false, Ordering::Relaxed) {
                crate::fs::unlock_drive(d.drive);
            }
            d.remount_if_written();
            0
        }
        // DSK_REDETERMINEMEDIA: mount whatever is on the volume now.
        0x02 => {
            d.written.store(false, Ordering::Relaxed);
            crate::fs::remount(d.drive).err().unwrap_or(0)
        }
        // DSK_BEGINFORMAT (the file system to be written; nothing to do)
        // and DSK_FORMATVERIFY (low-level format of a track: a hard disk
        // needs none).
        0x04 | 0x45 => 0,
        // DSK_BLOCKREMOVABLE: fixed disk.
        0x20 => {
            c.write_u8(r.data, 1);
            set_lens(c, r, 1, 1);
            0
        }
        // DSK_GETDEVICEPARAMS: BIOSPARAMETERBLOCK (36 bytes).
        0x63 => {
            let b = device_params(&d.dev);
            if !c.write(r.data, &b) {
                return err::INVALID_ADDRESS;
            }
            set_lens(c, r, 2, b.len() as u32);
            0
        }
        // DSK_READTRACK / DSK_WRITETRACK: TRACKLAYOUT.
        0x64 | 0x44 => {
            let Some(p) = c.read_vec(r.params, 9) else { return err::INVALID_ADDRESS };
            let head = u16::from_le_bytes([p[1], p[2]]) as u64;
            let cyl = u16::from_le_bytes([p[3], p[4]]) as u64;
            let first = u16::from_le_bytes([p[5], p[6]]) as u32;
            let count = u16::from_le_bytes([p[7], p[8]]) as u32;
            // Sector numbers (1-based) come from the track table.
            let Some(t) = c.read_vec(r.params + 9, (first + count) * 4) else { return err::INVALID_ADDRESS };
            let num = |i: u32| u16::from_le_bytes([t[i as usize * 4], t[i as usize * 4 + 1]]) as u64;
            let track = (cyl * HEADS + head) * SPT;
            let ss = d.dev.sector_size() as u32;
            let write = r.function == 0x44;
            // Runs of consecutive sector numbers in one transfer each.
            let mut i = first;
            while i < first + count {
                let start = num(i);
                if start == 0 {
                    return err::INVALID_PARAMETER;
                }
                let mut n = 1;
                while i + n < first + count && num(i + n) == start + n as u64 {
                    n += 1;
                }
                let e = rw(c, &d.dev, track + start - 1, n, r.data + (i - first) * ss, write);
                if e != 0 {
                    return e;
                }
                i += n;
            }
            if write {
                d.written.store(true, Ordering::Relaxed);
            }
            set_lens(c, r, 9 + (first + count) * 4, count * ss);
            0
        }
        _ => err::INVALID_FUNCTION,
    }
}

/// The BPB of the medium (when it holds a FAT boot sector) with the
/// device's size and synthetic geometry.
fn device_params(dev: &Arc<dyn BlockDev>) -> [u8; 36] {
    let ss = dev.sector_size();
    let sectors = dev.sector_count().min(u32::MAX as u64) as u32;
    let mut bs = vec![0u8; ss.max(512)];
    let mut b = [0u8; 36];
    let have = dev.read(0, &mut bs).is_ok() && u16::from_le_bytes([bs[11], bs[12]]) as usize == ss && bs[13] != 0;
    if have {
        // usBytesPerSector .. usSectorsPerFAT, cHiddenSectors
        b[0..13].copy_from_slice(&bs[11..24]);
        b[17..21].copy_from_slice(&bs[28..32]);
    } else {
        b[0..2].copy_from_slice(&(ss as u16).to_le_bytes());
        b[10] = 0xf8;
    }
    let small = if sectors < 0x10000 { sectors as u16 } else { 0 };
    b[8..10].copy_from_slice(&small.to_le_bytes());
    b[13..15].copy_from_slice(&(SPT as u16).to_le_bytes());
    b[15..17].copy_from_slice(&(HEADS as u16).to_le_bytes());
    b[21..25].copy_from_slice(&(if small == 0 { sectors } else { 0 }).to_le_bytes());
    let cyl = (sectors as u64).div_ceil(HEADS * SPT).min(0xffff) as u16;
    b[31..33].copy_from_slice(&cyl.to_le_bytes());
    b[33] = 5; // DEVTYPE_FIXED
    b[34..36].copy_from_slice(&1u16.to_le_bytes()); // non-removable
    b
}

fn rw(c: &Ctx, d: &Arc<dyn BlockDev>, lba: u64, count: u32, data: u32, write: bool) -> u32 {
    let ss = d.sector_size();
    if lba + count as u64 > d.sector_count() {
        return 27; // ERROR_SECTOR_NOT_FOUND
    }
    // In 64 KB pieces.
    let per = (65536 / ss) as u32;
    let mut done = 0;
    while done < count {
        let n = (count - done).min(per);
        let mut buf = vec![0u8; n as usize * ss];
        let at = data + done * ss as u32;
        if write {
            if !c.read(at, &mut buf) {
                return err::INVALID_ADDRESS;
            }
            if let Err(e) = d.write(lba + done as u64, &buf) {
                return e;
            }
        } else {
            if let Err(e) = d.read(lba + done as u64, &mut buf) {
                return e;
            }
            if !c.write(at, &buf) {
                return err::INVALID_ADDRESS;
            }
        }
        done += n;
    }
    0
}

/// Category 0xD5 on DOS$: function 0xAB reboots the machine.
fn dos_dev(_c: &Ctx, r: &Req) -> u32 {
    if r.function == 0xab {
        crate::power::reboot();
    }
    err::INVALID_FUNCTION
}

/// Category 4 (keyboard) on KBD$ and CON, plus two rs2 extensions used by
/// KEYB to switch the national layout:
///
/// | function | parameters | data |
/// |----------|------------|------|
/// | 0x5F set layout   | char id[4] ("GR"), USHORT code page (0 = keep) | — |
/// | 0x7C query layout | — | char id[4], USHORT code page |
fn kbd(c: &Ctx, r: &Req) -> u32 {
    use crate::conkbd;
    use crate::kbd::layout;
    let sid = crate::session::io_session();
    let pbyte = || c.read_u8(r.params);
    let pword = |o: u32| c.read_u16(r.params + o);
    match r.function {
        // rs2: set and query the keyboard layout.
        0x5f => {
            let Some(p) = c.read_vec(r.params, 6) else { return err::INVALID_ADDRESS };
            let id: alloc::string::String = p[..4].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
            let cp = u16::from_le_bytes([p[4], p[5]]);
            if cp != 0 {
                match crate::nls::keyboard_cp(cp) {
                    Ok(cp) => layout::set_codepage(cp),
                    Err(e) => return e,
                }
            }
            if layout::set_layout(&id) { 0 } else { err::INVALID_PARAMETER }
        }
        0x7c => {
            let mut b = [0u8; 6];
            let id = layout::layout_id().as_bytes();
            b[..id.len().min(4)].copy_from_slice(&id[..id.len().min(4)]);
            b[4..].copy_from_slice(&layout::codepage().to_le_bytes());
            if c.write(r.data, &b) { 0 } else { err::INVALID_ADDRESS }
        }
        // KBD_SETTRANSTABLE (a translation table: the layout tables belong
        // to the keyboard layer), KBD_SETFGNDSCREENGRP,
        // KBD_SETSESMGRHOTKEY, KBD_SETFOCUS, KBD_SETKCB,
        // KBD_SETREADNOTIFICATION, KBD_SETNLS, KBD_CREATE, KBD_DESTROY:
        // accepted.
        0x50 | 0x55 | 0x56 | 0x57 | 0x58 | 0x59 | 0x5c | 0x5d | 0x5e => 0,
        // KBD_SETINPUTMODE: BINARY_MODE 0x80, SHIFT_REPORT_MODE 0x01.
        0x51 => {
            let Some(m) = pbyte() else { return err::INVALID_ADDRESS };
            let Ok(mut i) = conkbd::info(sid, 0) else { return err::INVALID_HANDLE };
            i.mask &= !(conkbd::mask::BINARY | conkbd::mask::ASCII | conkbd::mask::SHIFT_REPORT);
            i.mask |= if m & 0x80 != 0 { conkbd::mask::BINARY } else { conkbd::mask::ASCII };
            if m & 1 != 0 {
                i.mask |= conkbd::mask::SHIFT_REPORT;
            }
            conkbd::set_info(sid, 0, i).err().unwrap_or(0)
        }
        // KBD_SETINTERIMFLAG
        0x52 => {
            let Some(m) = pbyte() else { return err::INVALID_ADDRESS };
            let Ok(mut i) = conkbd::info(sid, 0) else { return err::INVALID_HANDLE };
            i.interim = m as u16;
            conkbd::set_info(sid, 0, i).err().unwrap_or(0)
        }
        // KBD_SETSHIFTSTATE: SHIFTSTATE { fsState, fNLS }
        0x53 => {
            let Some(v) = pword(0) else { return err::INVALID_ADDRESS };
            conkbd::set_shift(v);
            0
        }
        // KBD_SETTYPAMATICRATE: RATEDELAY { usDelay ms, usRate cps }
        0x54 => {
            let (Some(d), Some(rate)) = (pword(0), pword(2)) else { return err::INVALID_ADDRESS };
            conkbd::set_typematic(d as u32, rate as u32);
            0
        }
        // KBD_ALTERKBDLED: show the shift state's locks.
        0x5a => {
            conkbd::set_shift(crate::kbd::shift_state());
            0
        }
        0x71 => {
            let m = conkbd::info(sid, 0).map_or(0, |i| i.mask);
            let v = if m & conkbd::mask::BINARY != 0 { 0x80 } else { 0 } | (m & conkbd::mask::SHIFT_REPORT != 0) as u8;
            put_data(c, r, 0, &[v])
        }
        0x72 => {
            let v = conkbd::info(sid, 0).map_or(0, |i| i.interim) as u8;
            put_data(c, r, 0, &[v])
        }
        // KBD_GETSHIFTSTATE: SHIFTSTATE { fsState, fNLS }
        0x73 => {
            let s = crate::kbd::shift_state().to_le_bytes();
            put_data(c, r, 0, &[s[0], s[1], 0])
        }
        // KBD_READCHAR: param = count | KBD_READ_NOWAIT; KBDKEYINFO records.
        0x74 => {
            let Some(p) = pword(0) else { return err::INVALID_ADDRESS };
            let (n, nowait) = ((p & 0x7fff).max(1), p & 0x8000 != 0);
            let mut out = alloc::vec::Vec::new();
            for i in 0..n {
                let Some(k) = crate::kbd::get_key_from(sid, !nowait && i == 0) else { break };
                out.extend_from_slice(&key_record(&k));
            }
            c.write_u16(r.params, (out.len() / 10) as u16);
            put_data(c, r, 2, &out)
        }
        // KBD_PEEKCHAR: param = status (KBD_DATA_RECEIVED, KBD_DATA_BINARY).
        0x75 => {
            let k = crate::kbd::peek_key();
            let bin = if conkbd::binary(sid) { 0x8000 } else { 0 };
            c.write_u16(r.params, k.is_some() as u16 | bin);
            put_data(c, r, 2, &key_record(&k.unwrap_or_default()))
        }
        // KBD_GETSESMGRHOTKEY: no hot keys to report.
        0x76 => put_data(c, r, 2, &0u16.to_le_bytes()),
        // KBD_GETKEYBDTYPE: KBDTYPE, enhanced keyboard.
        0x77 => put_data(c, r, 0, &[1, 0, 0, 0, 0, 0]),
        // KBD_GETCODEPAGEID: CPID
        0x78 => {
            let mut cp = conkbd::codepage(sid);
            if cp == 0 {
                cp = layout::codepage();
            }
            let b = cp.to_le_bytes();
            put_data(c, r, 0, &[b[0], b[1], 0, 0])
        }
        // KBD_XLATESCAN: KBDTRANS
        0x79 => {
            let Some(mut t) = c.read_vec(r.data, 18) else { return err::INVALID_ADDRESS };
            let state = u16::from_le_bytes([t[4], t[5]]);
            let ext = u16::from_le_bytes([t[10], t[11]]) & 0x80 != 0;
            match crate::kbd::xlate(t[1], state, ext) {
                Some(k) => {
                    t[0] = k.ch;
                    t[2] = k.status;
                }
                None => t[2] = 0x01,
            }
            t[12] = 1;
            t[13] = 0;
            put_data(c, r, 2, &t)
        }
        // KBD_QUERYKBDHARDWAREID: KBDHWID
        0x7a => put_data(c, r, 0, &[8, 0, 0x41, 0xab, 0, 0, 0, 0]),
        // KBD_QUERYKBDCODEPAGESUPPORT: count, then the code pages.
        0x7b => put_data(c, r, 0, &[2, 0, 0x52, 0x03, 0xb5, 0x01]),
        _ => err::INVALID_FUNCTION,
    }
}

fn key_record(k: &crate::kbd::KeyInfo) -> [u8; 10] {
    let s = k.shift.to_le_bytes();
    let t = k.time.to_le_bytes();
    [k.ch, k.scan, k.status, 0, s[0], s[1], t[0], t[1], t[2], t[3]]
}

/// Category 0xB (general) on the keyboard: DEV_FLUSHINPUT.
fn general_kbd(_c: &Ctx, r: &Req) -> u32 {
    match r.function {
        1 => {
            crate::kbd::flush();
            0
        }
        2 => 0,
        _ => err::INVALID_FUNCTION,
    }
}

/// Category 7 (pointing device) on MOUSE$: the query functions.
fn mouse(c: &Ctx, r: &Req) -> u32 {
    let sid = c.process.session.load(core::sync::atomic::Ordering::Relaxed);
    let words: alloc::vec::Vec<u16> = match r.function {
        0x60 => alloc::vec![3],
        0x61 => alloc::vec![50],
        0x62 => alloc::vec![crate::mouse::with(sid, |m| m.dev_status)],
        0x64 => alloc::vec![crate::mouse::with(sid, |m| m.queue.len() as u16), 64],
        0x65 => alloc::vec![crate::mouse::with(sid, |m| m.event_mask)],
        0x66 => {
            let (a, b) = crate::mouse::with(sid, |m| m.scale);
            alloc::vec![a, b]
        }
        0x67 => {
            let (a, b) = crate::mouse::with(sid, |m| (m.row as u16, m.col as u16));
            alloc::vec![a, b]
        }
        0x69 => alloc::vec![0],
        0x50..=0x5d => return 0,
        _ => return err::INVALID_FUNCTION,
    };
    let b: alloc::vec::Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    put_data(c, r, 0, &b)
}

/// Category 3 on SCREEN$: SCR_ALLOCLDT maps the text buffer (physical
/// B8000/B0000) and returns its selector; SCR_DEALLOCLDT.
fn screen(c: &Ctx, r: &Req) -> u32 {
    match r.function {
        0x70 => {
            // Parameters: LDTADDRINFO { PULONG pulPhysAddr, USHORT cb }.
            let Some(pp) = c.read_u32(r.params) else { return err::INVALID_ADDRESS };
            let pp = if c.is16 { c.far16_to_flat(pp) } else { pp };
            let Some(phys) = c.read_u32(pp) else { return err::INVALID_ADDRESS };
            if !(0xa0000..0xc0000).contains(&phys) {
                return err::INVALID_PARAMETER;
            }
            let sid = crate::session::io_session();
            let Some(obj) = crate::screen::with_session(sid, |s| s.phys_buf()).flatten() else { return err::NOT_ENOUGH_MEMORY };
            {
                let mut sp = c.space();
                if !sp.regions.contains_key(&obj.base) {
                    sp.map_shared(obj.clone(), crate::vm::pag::READ | crate::vm::pag::WRITE, crate::vm::SegKind::Data);
                }
            }
            let sel = crate::vm::tile_selector(obj.base);
            put_data(c, r, 6, &sel.to_le_bytes())
        }
        0x71 => 0,
        _ => err::INVALID_FUNCTION,
    }
}
