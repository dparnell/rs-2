//! MONCALLS: device monitors (16-bit `_Pascal`), see `crate::monitor`.
//!
//! The monitor buffers start with their length (a word); rs2 keeps the
//! packets itself, so only the length is checked.

use super::{err, Ctx};
use crate::monitor;

/// Smallest monitor buffer: the length word plus room for a packet.
const MIN_BUFFER: u16 = 20;

/// DosMonOpen(pszDevName, phmon).
pub fn dos_mon_open(c: &mut Ctx) -> u32 {
    let (pname, ph) = (c.ptr(0), c.ptr(1));
    let Some(name) = c.read_str(pname, 260) else { return err::INVALID_ADDRESS };
    match monitor::open(&name, c.process.pid) {
        Ok(h) => {
            c.write_u16(ph, h);
            0
        }
        Err(e) => e,
    }
}

/// DosMonClose(hmon).
pub fn dos_mon_close(c: &mut Ctx) -> u32 {
    monitor::close(c.arg(0) as u16, c.process.pid).err().unwrap_or(0)
}

/// DosMonReg(hmon, pbInBuf, pbOutBuf, fPosition, index): index is the
/// session (screen group); -1 means the caller's.
pub fn dos_mon_reg(c: &mut Ctx) -> u32 {
    let (h, inb, outb, pos, index) = (c.arg(0) as u16, c.ptr(1), c.ptr(2), c.arg(3) as u16, c.arg(4) as u16);
    let (Some(li), Some(lo)) = (c.read_u16(inb), c.read_u16(outb)) else { return err::INVALID_ADDRESS };
    if li < MIN_BUFFER || lo < MIN_BUFFER {
        return monitor::ERROR_MON_BUFFER_TOO_SMALL;
    }
    let sid = if index == 0xffff { crate::session::current() } else { index as u32 };
    monitor::register(h, c.process.pid, inb, outb, pos, sid).err().unwrap_or(0)
}

/// DosMonRead(pbInBuf, fWait, pbDataBuf, pcbData): fWait 0 waits,
/// 1 returns ERROR_MON_BUFFER_EMPTY when nothing is there.
pub fn dos_mon_read(c: &mut Ctx) -> u32 {
    let (inb, wait, data, pcb) = (c.ptr(0), c.arg(1), c.ptr(2), c.ptr(3));
    if wait > 1 {
        return monitor::ERROR_MON_INVALID_PARMS;
    }
    let Some(max) = c.read_u16(pcb) else { return err::INVALID_ADDRESS };
    match monitor::read(c.process.pid, inb, wait == 0, max as usize) {
        Ok(p) => {
            if !c.write(data, &p) {
                return err::INVALID_ADDRESS;
            }
            c.write_u16(pcb, p.len() as u16);
            0
        }
        Err(e) => e,
    }
}

/// DosMonWrite(pbOutBuf, pbDataBuf, cbData).
pub fn dos_mon_write(c: &mut Ctx) -> u32 {
    let (outb, data, cb) = (c.ptr(0), c.ptr(1), c.arg(2));
    let Some(lo) = c.read_u16(outb) else { return err::INVALID_ADDRESS };
    if cb as u16 > lo.saturating_sub(MIN_BUFFER / 2) {
        return monitor::ERROR_MON_DATA_TOO_LARGE;
    }
    let Some(p) = c.read_vec(data, cb) else { return err::INVALID_ADDRESS };
    monitor::write(c.process.pid, outb, &p).err().unwrap_or(0)
}
