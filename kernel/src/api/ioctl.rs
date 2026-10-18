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
use crate::fs::Handle;

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
    let Some(h) = super::file::get(c, r.handle) else { return err::INVALID_HANDLE };
    match (h, r.category) {
        (Handle::PhysDisk(n), 9) => phys(c, n, &r),
        (_, 0xd5) => dos_dev(c, &r),
        (Handle::Kbd | Handle::Con, 4) => kbd(c, &r),
        _ => err::INVALID_CATEGORY,
    }
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

/// Category 4 (keyboard): a few query functions.
fn kbd(c: &Ctx, r: &Req) -> u32 {
    match r.function {
        // KBD_GETSESMGRHOTKEY / KBD_GETINPUTMODE: report defaults.
        0x71 | 0x76 => {
            c.write_u8(r.data, 0);
            0
        }
        // KBD_GETSHIFTSTATE: SHIFTSTATE { fsState, fNLS }
        0x73 => {
            c.write_u16(r.data, crate::kbd::shift_state());
            c.write_u8(r.data + 2, 0);
            0
        }
        _ => err::INVALID_FUNCTION,
    }
}
