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
pub(crate) struct Req {
    handle: u32,
    pub category: u32,
    pub function: u32,
    pub params: u32,
    pub params_len: u32,
    pub data: u32,
    pub data_len: u32,
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
    if !c.is16 {
        // A buffer with a zero maximum length must be NULL.
        if (r.params_len == 0 && r.params != 0) || (r.data_len == 0 && r.data != 0) {
            return err::INVALID_PARAMETER;
        }
    }
    ioctl(c, &r)
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
    ioctl(c, &r)
}

/// Device driver status codes (the error byte of a request packet's
/// status word).
pub(crate) mod status {
    pub const UNKNOWN_COMMAND: u8 = 0x03;
    pub const GEN_FAILURE: u8 = 0x0c;
    pub const MONITORS_NOT_SUPPORTED: u8 = 0x12;
    pub const INVALID_PARAMETER: u8 = 0x13;
}

/// The return code of DosDevIOCtl for a device driver's error status, as
/// IBM's kernel makes it (ErrorTriage): categories 0x80 and above give
/// 0xFF00 + the code; the others the standard error for the code (19 + code
/// up to general failure, then disk change, wrong disk, uncertain media,
/// interrupted, monitors not supported, invalid parameter, device in use).
pub(crate) fn dd_error(category: u32, code: u8) -> u32 {
    const MAP: [u32; 0x15] = [19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 107, 31, 34, 163, 95, 165, 87, 99];
    if category & 0x80 != 0 {
        0xff00 | code as u32
    } else {
        MAP.get(code as usize).copied().unwrap_or(31)
    }
}

/// Handles whose IOCtls reach a device (and so the device driver error
/// rules); files, pipes and the printer ports are handled elsewhere.
fn is_device(h: &Handle) -> bool {
    !matches!(h, Handle::File(_) | Handle::Pipe(_) | Handle::NPipe(_) | Handle::Spool(..) | Handle::Char(CharDev::Lpt(_)))
}

/// DosDevIOCtl as IBM's kernel routes it. Functions below 0x40 of
/// categories below 0x80 are the kernel's own (logical disk lock, unlock,
/// media, logical map, removability; physical disk lock and unlock; flush
/// input and output): any other is ERROR_INVALID_FUNCTION in categories 1
/// to 13 and ERROR_INVALID_CATEGORY outside them. Everything else goes to
/// the device, whose "unknown command" becomes ERROR_BAD_COMMAND (0xFF03
/// in the user categories). Before the device is called the 32-bit form
/// stores the maximum lengths in the length words (IBM's kernel stores
/// the low words and never the lengths a driver used); handlers that know
/// how much they used store that afterwards.
fn ioctl(c: &mut Ctx, r: &Req) -> u32 {
    let Some(h) = super::file::get(c, r.handle) else { return err::INVALID_HANDLE };
    if !is_device(&h) {
        return dispatch(c, r);
    }
    let kernel = r.category < 0x80 && r.function & 0x40 == 0;
    if kernel {
        let ours = match r.category {
            8 => matches!(r.function, 0..=4 | 0x20..=0x22),
            // rs2's LBA functions (0xA0-0xA2) included.
            9 => matches!(r.function, 0 | 1 | 0xa0..=0xa2),
            0xb => matches!(r.function, 1 | 2),
            _ => false,
        };
        if !ours {
            return if (1..=13).contains(&r.category) { err::INVALID_FUNCTION } else { err::INVALID_CATEGORY };
        }
    }
    for (p, max) in [(r.pcb_params, r.params_len), (r.pcb_data, r.data_len)] {
        if p != 0 && !c.write_u16(p, max as u16) {
            return err::INVALID_ADDRESS;
        }
    }
    match dispatch(c, r) {
        err::INVALID_FUNCTION | err::INVALID_CATEGORY if !kernel => dd_error(r.category, status::UNKNOWN_COMMAND),
        rc => rc,
    }
}

fn dispatch(c: &mut Ctx, r: &Req) -> u32 {
    let Some(h) = super::file::get(c, r.handle) else { return err::INVALID_HANDLE };
    match (h, r.category) {
        (Handle::PhysDisk(n), 9) => phys(c, n, r),
        (Handle::Drive(d), 8) => logical(c, &d, r),
        (Handle::Drive(d), 0x80 | 0x81) => super::cdioctl::ioctl(c, &d.dev, r),
        (_, 0xd5) => dos_dev(c, r),
        (Handle::Kbd | Handle::Con, 4) => kbd(c, r),
        (Handle::Kbd | Handle::Con, 0xb) => general_kbd(c, r),
        (Handle::Char(CharDev::Com(n)), 1) => {
            let need = match r.function {
                0x41 | 0x46 => 2,
                0x42 => 3,
                0x43 | 0x54 => 5,
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
        (Handle::Char(CharDev::Lpt(n)), 5) => match crate::chardev::lpt_ioctl(n as usize, r.function) {
            Ok(d) => put_data(c, r, 0, &d),
            Err(e) => e,
        },
        (Handle::Spool(..), 5) => match crate::chardev::lpt_spool_ioctl(r.function) {
            Ok(d) => put_data(c, r, 0, &d),
            Err(e) => e,
        },
        (Handle::Char(CharDev::Mouse | CharDev::Pointer), 7) => mouse(c, r),
        (Handle::Char(CharDev::ResMgr), 0x80) => crate::resmgr::ioctl(c, r),
        (Handle::Char(CharDev::VBox), 0x80) => vbox(c, r),
        (Handle::Char(CharDev::VMware), 0x80) if r.function == 1 => vmware(c, r),
        (Handle::Screen, 3) => screen(c, r),
        (Handle::Char(CharDev::OemHlp), _) => super::oemdev::oemhlp(c, r),
        (Handle::Char(CharDev::TestCfg), _) => super::oemdev::testcfg(c, r),
        (Handle::Char(CharDev::Apm), _) => super::oemdev::apm(c, r),
        (Handle::Kbd | Handle::Con, 0xa) => monitor_reg(c, crate::monitor::Dev::Kbd, r),
        (Handle::Char(CharDev::Mouse), 0xa) => monitor_reg(c, crate::monitor::Dev::Mouse, r),
        (Handle::Char(CharDev::Mouse | CharDev::Pointer), 0xb) => general_mouse(c, r),
        (Handle::Char(CharDev::Pointer), 3) => pointer_draw(c, r),
        (ref d, 0xb) if is_device(d) => general_other(r),
        _ => err::INVALID_CATEGORY,
    }
}

/// Copy IOCtl result data to the caller (ERROR_BUFFER_OVERFLOW if it does
/// not fit) and report the lengths used.
pub(crate) fn put_data(c: &Ctx, r: &Req, plen: u32, d: &[u8]) -> u32 {
    if d.len() as u32 > r.data_len {
        return err::BUFFER_OVERFLOW;
    }
    if !d.is_empty() && !c.write(r.data, d) {
        return err::INVALID_ADDRESS;
    }
    set_lens(c, r, plen, d.len() as u32);
    0
}

pub(crate) fn set_lens(c: &Ctx, r: &Req, p: u32, d: u32) {
    if r.pcb_params != 0 {
        c.write_u32(r.pcb_params, p);
    }
    if r.pcb_data != 0 {
        c.write_u32(r.pcb_data, d);
    }
}

/// A request on a physical disk goes past the volumes' block caches: they
/// write out first, and forget what a write may have changed.
fn phys(c: &Ctx, n: u32, r: &Req) -> u32 {
    crate::fs::cache::sync_all();
    let rc = phys_request(c, n, r);
    if matches!(r.function, 0x44 | 0x45) {
        crate::fs::cache::invalidate_all();
        crate::fs::namecache::clear_all();
    }
    rc
}

fn phys_request(c: &Ctx, n: u32, r: &Req) -> u32 {
    let Some(d) = disk(n) else { return err::INVALID_HANDLE };
    let ss = d.sector_size() as u32;
    match r.function {
        // PDSK_LOCKPHYSDRIVE / PDSK_UNLOCKPHYSDRIVE (parameter: a reserved
        // byte). The caches were written out above; after the unlock
        // what was changed on the disk is read anew.
        0x00 | 0x01 => {
            if r.function == 1 {
                crate::fs::cache::invalidate_all();
                crate::fs::namecache::clear_all();
            }
            0
        }
        // PDSK_VERIFYPHYSTRACK: TRACKLAYOUT; the sectors must read.
        0x65 => {
            let Some(p) = c.read_vec(r.params, 9) else { return err::INVALID_ADDRESS };
            let head = u16::from_le_bytes([p[1], p[2]]) as u64;
            let cyl = u16::from_le_bytes([p[3], p[4]]) as u64;
            let first = u16::from_le_bytes([p[5], p[6]]) as u64;
            let count = u16::from_le_bytes([p[7], p[8]]) as u32;
            verify(&d, (cyl * HEADS + head) * SPT + first, count)
        }
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
        // DSK_SETLOGICALMAP / DSK_GETLOGICALMAP (parameter: a reserved
        // byte): one drive letter per volume, so 0 ("only one").
        0x03 | 0x21 => {
            if !c.write_u8(r.data, 0) {
                return err::INVALID_ADDRESS;
            }
            set_lens(c, r, 1, 1);
            0
        }
        // DSK_GETDEVICEPARAMS (parameter: 0 the device's BPB, 1 the
        // medium's): BIOSPARAMETERBLOCK (36 bytes).
        0x63 => {
            let which = c.read_u8(r.params).unwrap_or(0);
            let set = BPBS.lock().iter().find(|b| b.0 == d.drive && b.1 == (which != 0)).map(|b| b.2);
            let b = set.unwrap_or_else(|| device_params(&d.dev));
            if !c.write(r.data, &b) {
                return err::INVALID_ADDRESS;
            }
            set_lens(c, r, 1, b.len() as u32);
            0
        }
        // DSK_SETDEVICEPARAMS: parameter 0 build the BPB from the medium
        // again, 1 replace the device's, 2 replace the medium's (what
        // DSK_GETDEVICEPARAMS answers until the next 0).
        0x43 => {
            let Some(cmd) = c.read_u8(r.params) else { return err::INVALID_ADDRESS };
            match cmd {
                0 => BPBS.lock().retain(|b| b.0 != d.drive),
                1 | 2 => {
                    let mut b = [0u8; 31];
                    if !c.read(r.data, &mut b) {
                        return err::INVALID_ADDRESS;
                    }
                    // The geometry fields after the BPB proper stay the
                    // device's.
                    let mut keep = device_params(&d.dev);
                    keep[..31].copy_from_slice(&b);
                    let medium = cmd == 2;
                    let mut bpbs = BPBS.lock();
                    bpbs.retain(|x| !(x.0 == d.drive && x.1 == medium));
                    bpbs.push((d.drive, medium, keep));
                }
                _ => return err::INVALID_PARAMETER,
            }
            0
        }
        // DSK_GETLOCKSTATUS: lock status not supported (fixed medium),
        // medium present.
        0x66 => put_data(c, r, 0, &4u16.to_le_bytes()),
        // DSK_READTRACK / DSK_WRITETRACK / DSK_VERIFYTRACK: TRACKLAYOUT.
        0x64 | 0x44 | 0x65 => {
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
                let e = if r.function == 0x65 {
                    verify(&d.dev, track + start - 1, n)
                } else {
                    rw(c, &d.dev, track + start - 1, n, r.data + (i - first) * ss, write)
                };
                if e != 0 {
                    return e;
                }
                i += n;
            }
            if write {
                d.written.store(true, Ordering::Relaxed);
            }
            set_lens(c, r, 9 + (first + count) * 4, if r.function == 0x65 { 0 } else { count * ss });
            0
        }
        _ => err::INVALID_FUNCTION,
    }
}

/// BPBs set by DSK_SETDEVICEPARAMS: drive, for the medium (else the
/// device), the BPB.
static BPBS: crate::sync::SpinLock<alloc::vec::Vec<(u8, bool, [u8; 36])>> = crate::sync::SpinLock::new(alloc::vec::Vec::new());

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

/// Read sectors and discard them (the verify functions).
fn verify(d: &Arc<dyn BlockDev>, lba: u64, count: u32) -> u32 {
    let ss = d.sector_size();
    if lba + count as u64 > d.sector_count() {
        return 27; // ERROR_SECTOR_NOT_FOUND
    }
    let per = (65536 / ss) as u32;
    let mut done = 0;
    while done < count {
        let n = (count - done).min(per);
        let mut buf = vec![0u8; n as usize * ss];
        if let Err(e) = d.read(lba + done as u64, &mut buf) {
            return e;
        }
        done += n;
    }
    0
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

/// Category 0xD5 on DOS$: function 0xAB reboots the machine (after
/// committing the disks' write caches, as the installer's restart needs).
fn dos_dev(_c: &Ctx, r: &Req) -> u32 {
    if r.function == 0xab {
        crate::drivers::flush_disks();
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

/// Category 0xB (general) on the keyboard: DEV_FLUSHINPUT,
/// DEV_FLUSHOUTPUT (nothing to flush), DEV_QUERYMONSUPPORT (KBD$ takes
/// monitors).
fn general_kbd(_c: &Ctx, r: &Req) -> u32 {
    match r.function {
        1 => {
            crate::kbd::flush();
            0
        }
        2 | 0x60 => 0,
        _ => err::INVALID_FUNCTION,
    }
}

/// Category 0xB on MOUSE$ and POINTER$: flush the event queue, flush
/// output (nothing), monitors (MOUSE$ takes them).
fn general_mouse(c: &Ctx, r: &Req) -> u32 {
    let sid = c.process.session.load(core::sync::atomic::Ordering::Relaxed);
    match (r.function, r.handle_is_pointer(c)) {
        (1, _) => {
            crate::mouse::with(sid, |m| m.queue.clear());
            0
        }
        (2, _) => 0,
        (0x60, false) => 0,
        (0x60, true) => dd_error(r.category, status::MONITORS_NOT_SUPPORTED),
        _ => err::INVALID_FUNCTION,
    }
}

/// Category 0xB on the other character devices: the flushes have
/// nothing to do; no monitors.
fn general_other(r: &Req) -> u32 {
    match r.function {
        1 | 2 => 0,
        0x60 => dd_error(r.category, status::MONITORS_NOT_SUPPORTED),
        _ => err::INVALID_FUNCTION,
    }
}

impl Req {
    /// Whether the handle is POINTER$ (MOUSE$ shares its IOCtls).
    fn handle_is_pointer(&self, c: &Ctx) -> bool {
        matches!(super::file::get(c, self.handle), Some(Handle::Char(CharDev::Pointer)))
    }
}

/// Category 0xA (MON_REGISTERMONITOR) on KBD$ and MOUSE$: what DosMonReg
/// does through MONCALLS. Parameters MONITORPOSITION { USHORT fPosition,
/// USHORT index (session, 0xFFFF the caller's), 16:16 pbInBuf, USHORT
/// offOutBuf } (the output buffer is in the input buffer's segment).
fn monitor_reg(c: &Ctx, dev: crate::monitor::Dev, r: &Req) -> u32 {
    let bad = dd_error(r.category, status::INVALID_PARAMETER);
    if r.function != 0x40 {
        return err::INVALID_FUNCTION;
    }
    let Some(p) = c.read_vec(r.params, 10) else { return bad };
    let w = |o: usize| u16::from_le_bytes([p[o], p[o + 1]]);
    let far = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
    let inbuf = c.far16_to_flat(far);
    let outbuf = c.far16_to_flat((far & 0xffff_0000) | w(8) as u32);
    let (Some(li), Some(lo)) = (c.read_u16(inbuf), c.read_u16(outbuf)) else { return bad };
    if li < 20 || lo < 20 {
        return bad;
    }
    let sid = if w(2) == 0xffff { crate::session::current() } else { w(2) as u32 };
    match crate::monitor::register_device(dev, c.process.pid, inbuf, outbuf, w(0), sid) {
        Ok(()) => 0,
        Err(_) => bad,
    }
}

/// Category 7 (pointing device) on MOUSE$ and POINTER$, as IBM's
/// MOUSE.SYS answers for the session of the caller.
fn mouse(c: &Ctx, r: &Req) -> u32 {
    use crate::mouse;
    let sid = c.process.session.load(core::sync::atomic::Ordering::Relaxed);
    let bad = dd_error(r.category, status::INVALID_PARAMETER);
    let pword = |o: u32| c.read_u16(r.params + o);
    let words: alloc::vec::Vec<u16> = match r.function {
        // MOU_ALLOWPTRDRAW, MOU_UPDATEDISPLAYMODE, MOU_SCREENSWITCH,
        // MOU_SETHOTKEYBUTTON, MOU_SETPTRSHAPE (the text pointer is a
        // fixed inverting cell), MOU_SETPROTDRAWADDRESS,
        // MOU_SETREALDRAWADDRESS, MOU_DISPLAYMODECHANGE: accepted.
        0x50 | 0x51 | 0x52 | 0x55 | 0x56 | 0x5a | 0x5b | 0x5d => return 0,
        // MOU_SETSCALEFACTORS: SCALEFACT { row, col }.
        0x53 => {
            let (Some(a), Some(b)) = (pword(0), pword(2)) else { return bad };
            if a == 0 || b == 0 {
                return bad;
            }
            mouse::with(sid, |m| m.scale = (a, b));
            return 0;
        }
        // MOU_SETEVENTMASK
        0x54 => {
            let Some(v) = pword(0) else { return bad };
            mouse::with(sid, |m| m.event_mask = v & 0x7f);
            return 0;
        }
        // MOU_DRAWPTR (unmark the collision area) and MOU_REMOVEPTR
        // (mark it: the whole screen here).
        0x57 | 0x58 => {
            mouse::show(sid, r.function == 0x57);
            return 0;
        }
        // MOU_SETPTRPOS: PTRLOC { row, col }.
        0x59 => {
            let (Some(row), Some(col)) = (pword(0), pword(2)) else { return bad };
            let (rows, cols) = crate::screen::with_session(sid, |s| (s.rows as u16, s.cols as u16)).unwrap_or((25, 80));
            if row >= rows || col >= cols {
                return bad;
            }
            mouse::set_pos(sid, row as i32, col as i32);
            return 0;
        }
        // MOU_SETMOUSTATUS: the settable bits (disabled, mickeys).
        0x5c => {
            let Some(v) = pword(0) else { return bad };
            mouse::with(sid, |m| m.dev_status = (m.dev_status & 0xff) | (v & 0x0300));
            return 0;
        }
        0x60 => alloc::vec![3],
        0x61 => alloc::vec![50],
        0x62 => alloc::vec![mouse::with(sid, |m| m.dev_status)],
        // MOU_READEVENTQUE: parameter USHORT 1 = wait; MOUEVENTINFO.
        0x63 => {
            let Some(wait) = pword(0) else { return bad };
            let e = mouse::read(sid, wait == 1).unwrap_or_default();
            let mut b = [0u8; 10];
            b[0..2].copy_from_slice(&e.fs.to_le_bytes());
            b[2..6].copy_from_slice(&e.time.to_le_bytes());
            b[6..8].copy_from_slice(&e.row.to_le_bytes());
            b[8..10].copy_from_slice(&e.col.to_le_bytes());
            return put_data(c, r, 2, &b);
        }
        0x64 => alloc::vec![mouse::with(sid, |m| m.queue.len() as u16), 64],
        0x65 => alloc::vec![mouse::with(sid, |m| m.event_mask)],
        0x66 => {
            let (a, b) = mouse::with(sid, |m| m.scale);
            alloc::vec![a, b]
        }
        0x67 => {
            let (a, b) = mouse::with(sid, |m| (m.row as u16, m.col as u16));
            alloc::vec![a, b]
        }
        // MOU_GETPTRSHAPE: data PTRSHAPE { cb (in: buffer size, out: the
        // size needed), col, row, colHot, rowHot }; parameters the AND and
        // XOR masks (a 1x1 text cell: AND FFFF, XOR 7700).
        0x68 => {
            let Some(cb) = c.read_u16(r.data) else { return bad };
            let mut shape = [0u8; 10];
            for (i, v) in [4u16, 1, 1, 0, 0].into_iter().enumerate() {
                shape[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
            if !c.write(r.data, &shape) {
                return bad;
            }
            if cb < 4 || !c.write(r.params, &[0xff, 0xff, 0x00, 0x77]) {
                return bad;
            }
            set_lens(c, r, 4, 10);
            return 0;
        }
        // MOU_GETHOTKEYBUTTON: none.
        0x69 => alloc::vec![0],
        // MOU_VER: IBM's MOUSE.SYS answers without data.
        0x6a => return 0,
        // MOU_QUERYPOINTERID: the pointer ID and type, as words.
        0x6b => alloc::vec![0, 0, 0],
        _ => return err::INVALID_FUNCTION,
    };
    let b: alloc::vec::Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    put_data(c, r, 0, &b)
}

/// Category 3 on POINTER$: PTR_GETPTRDRAWADDRESS gives a pointer draw
/// routine for a mouse device driver to call at ring 0. rs2 draws the
/// text pointer itself and has no such routine for drivers: unknown
/// function, as a pointer driver without one answers.
fn pointer_draw(_c: &Ctx, _r: &Req) -> u32 {
    err::INVALID_FUNCTION
}

/// Category 3 on SCREEN$: SCR_ALLOCLDT maps the text buffer (physical
/// B8000/B0000) and returns its selector; SCR_DEALLOCLDT.
fn screen(c: &Ctx, r: &Req) -> u32 {
    match r.function {
        // SCR_ALLOCLDTOFF: the same with an address anywhere in the
        // buffer: data a 16:16 pointer to it.
        0x70 | 0x75 => {
            // Parameters: LDTADDRINFO { PULONG pulPhysAddr, USHORT cb }.
            let Some(pp) = c.read_u32(r.params) else { return err::INVALID_ADDRESS };
            let pp = if c.is16 { c.far16_to_flat(pp) } else { pp };
            let Some(phys) = c.read_u32(pp) else { return err::INVALID_ADDRESS };
            let cb = c.read_u16(r.params + 4).unwrap_or(0) as u32;
            if !(0xa0000..0xc0000).contains(&phys) || phys + cb > 0xc0000 {
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
            if r.function == 0x75 {
                // The buffer object maps the text buffer (B8000 or B0000)
                // at its start.
                let start = if phys >= 0xb8000 { 0xb8000 } else if phys >= 0xb0000 { 0xb0000 } else { 0xa0000 };
                let off = ((phys - start) & (crate::screen::PhysBuf::SIZE - 1)) as u16;
                let mut b = [0u8; 4];
                b[0..2].copy_from_slice(&off.to_le_bytes());
                b[2..4].copy_from_slice(&sel.to_le_bytes());
                return put_data(c, r, 6, &b);
            }
            put_data(c, r, 6, &sel.to_le_bytes())
        }
        0x71 => 0,
        _ => err::INVALID_FUNCTION,
    }
}

/// \\DEV\\VBOXGST$ (crate::drivers::vbox): parameters {format, rc, a, b},
/// data the buffer.
fn vbox(c: &mut Ctx, r: &Req) -> u32 {
    let Some(p) = c.read_vec(r.params, 16) else { return err::INVALID_ADDRESS };
    let format = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
    let input = if r.function == 4 {
        match c.read_vec(r.data, r.data_len.min(1024 * 1024)) {
            Some(d) => d,
            None => return err::INVALID_ADDRESS,
        }
    } else {
        alloc::vec::Vec::new()
    };
    let rep = crate::drivers::vbox::clipboard(r.function, format, &input, r.data_len as usize);
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&format.to_le_bytes());
    out[4..8].copy_from_slice(&rep.rc.to_le_bytes());
    out[8..12].copy_from_slice(&rep.a.to_le_bytes());
    out[12..16].copy_from_slice(&rep.b.to_le_bytes());
    if !c.write(r.params, &out) || (!rep.data.is_empty() && !c.write(r.data, &rep.data)) {
        return err::INVALID_ADDRESS;
    }
    set_lens(c, r, 16, rep.data.len() as u32);
    0
}

/// \\DEV\\VMWARE$ function 1: a backdoor call; parameters EAX, EBX, ECX,
/// EDX, ESI, EDI in and out (crate::vmware::call_regs).
fn vmware(c: &mut Ctx, r: &Req) -> u32 {
    let Some(p) = c.read_vec(r.params, 24) else { return err::INVALID_ADDRESS };
    let w = |i: usize| u32::from_le_bytes([p[4 * i], p[4 * i + 1], p[4 * i + 2], p[4 * i + 3]]);
    let regs = crate::vmware::Regs { eax: w(0), ebx: w(1), ecx: w(2), edx: w(3), esi: w(4), edi: w(5) };
    let Some(o) = crate::vmware::call_regs(regs) else { return err::INVALID_CATEGORY };
    let mut out = [0u8; 24];
    for (i, v) in [o.eax, o.ebx, o.ecx, o.edx, o.esi, o.edi].iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    if !c.write(r.params, &out) {
        return err::INVALID_ADDRESS;
    }
    set_lens(c, r, 24, 0);
    0
}
