//! virtio-scsi host adapters: the logical units on them become disks and
//! optical drives.
//!
//! Commands go through the first request queue (queue 2), one at a time
//! per adapter: the request header (LUN address, tag, CDB), data-out,
//! then the response (status, sense data, residue) and data-in. Sense
//! data comes back with the response, so a CHECK CONDITION needs no
//! REQUEST SENSE: unit attentions (reset, medium changed) are retried,
//! NOT READY "becoming ready" is waited for. Targets are found with
//! REPORT LUNS (LUN 0 if a target does not support it), then each unit
//! gets INQUIRY, TEST UNIT READY and READ CAPACITY; data moves with
//! READ/WRITE(10), or (16) beyond 32-bit block numbers. The control and
//! event queues are not used (no task management, no hot-plug events).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::api::err;
use crate::fs::block::BlockDev;
use crate::sync::Mutex;

// Configuration offsets.
const CFG_MAX_SECTORS: usize = 8;
const CFG_SENSE_SIZE: usize = 20;
const CFG_CDB_SIZE: usize = 24;
const CFG_MAX_TARGET: usize = 30;

/// The first request queue.
const REQUEST_QUEUE: u16 = 2;

// Response codes.
const S_OK: u8 = 0;
const S_BAD_TARGET: u8 = 3;

// SCSI operation codes.
const TEST_UNIT_READY: u8 = 0x00;
const INQUIRY: u8 = 0x12;
const READ_CAPACITY_10: u8 = 0x25;
const READ_10: u8 = 0x28;
const WRITE_10: u8 = 0x2a;
const READ_16: u8 = 0x88;
const WRITE_16: u8 = 0x8a;
const SERVICE_ACTION_IN: u8 = 0x9e;
const READ_CAPACITY_16: u8 = 0x10;
const REPORT_LUNS: u8 = 0xa0;

// Status and sense keys.
const CHECK_CONDITION: u8 = 2;
const NOT_READY: u8 = 2;
const UNIT_ATTENTION: u8 = 6;
const DATA_PROTECT: u8 = 7;
const ASC_NO_MEDIUM: u8 = 0x3a;

// Peripheral device types.
const TYPE_DISK: u8 = 0x00;
const TYPE_CDROM: u8 = 0x05;
const TYPE_OPTICAL: u8 = 0x07;
const TYPE_RBC: u8 = 0x0e;

/// Request header at 0, response at 512, data at 4096.
const REQ: usize = 0;
const RESP: usize = 512;
const DATA: usize = 4096;
const MAX_XFER: usize = 64 * 1024;
const TIMEOUT_MS: u64 = 30_000;
/// Logical units looked at per target.
const MAX_LUNS: usize = 8;
const READY_MS: u64 = 5000;

#[derive(Clone, Copy, Debug)]
enum Error {
    /// CHECK CONDITION with sense key, ASC, ASCQ.
    Sense(u8, u8, u8),
    /// Another SCSI status (BUSY, RESERVATION CONFLICT...).
    Status(u8),
    /// The transport failed (virtio-scsi response code).
    Response(u8),
    Timeout,
}

impl Error {
    fn code(self) -> u32 {
        match self {
            Error::Sense(DATA_PROTECT, _, _) => err::WRITE_PROTECT,
            _ => err::NOT_READY,
        }
    }
}

#[derive(Clone, Copy)]
enum Data {
    None,
    In(usize),
    Out(usize),
}

struct Ctrl {
    name: String,
    q: Virtqueue,
    mem: Dma,
    cdb_size: usize,
    sense_size: usize,
    next_tag: u64,
    /// Largest transfer the adapter wants, in 512-byte sectors.
    max_sectors: usize,
    failed: bool,
    dev: Device,
}

/// The 8-byte LUN address of `target`/`lun` (single-level, flat).
fn lun_address(target: u8, lun: u16) -> [u8; 8] {
    let l = 0x4000 | (lun & 0x3fff);
    [1, target, (l >> 8) as u8, l as u8, 0, 0, 0, 0]
}

impl Ctrl {
    /// Busy-wait `ms` (also before the timer runs, during boot).
    fn delay(&self, ms: u64) {
        super::wait(ms, || { self.dev.status(); }, || false);
    }

    /// One command; Ok(bytes transferred).
    fn exec(&mut self, target: u8, lun: u16, cdb: &[u8], data: Data) -> Result<usize, Error> {
        if self.failed {
            return Err(Error::Timeout);
        }
        self.next_tag += 1;
        let mut req = alloc::vec![0u8; 19 + self.cdb_size];
        req[0..8].copy_from_slice(&lun_address(target, lun));
        req[8..16].copy_from_slice(&self.next_tag.to_le_bytes());
        req[19..19 + cdb.len().min(self.cdb_size)].copy_from_slice(&cdb[..cdb.len().min(self.cdb_size)]);
        self.mem.write(REQ, &req);
        let resp_len = 12 + self.sense_size;
        unsafe { core::ptr::write_bytes(self.mem.ptr(RESP), 0, resp_len) };
        let r = Buf::out(self.mem.addr(REQ), req.len());
        let s = Buf::into(self.mem.addr(RESP), resp_len);
        let (bufs, len): (Vec<Buf>, usize) = match data {
            Data::None => (alloc::vec![r, s], 0),
            Data::Out(n) => (alloc::vec![r, Buf::out(self.mem.addr(DATA), n), s], n),
            Data::In(n) => (alloc::vec![r, s, Buf::into(self.mem.addr(DATA), n)], n),
        };
        if self.q.call(&bufs, TIMEOUT_MS).is_none() {
            crate::println!("{}: command {:#x} timed out", self.name, cdb[0]);
            self.failed = true;
            return Err(Error::Timeout);
        }
        let b = self.mem.bytes(RESP, resp_len);
        let le32 = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let (sense_len, resid, status, response) = (le32(0) as usize, le32(4) as usize, b[10], b[11]);
        if response != S_OK {
            return Err(Error::Response(response));
        }
        match status {
            0 => Ok(len.saturating_sub(resid)),
            CHECK_CONDITION => {
                let sense = &b[12..12 + sense_len.min(self.sense_size)];
                Err(match sense.first().map(|c| c & 0x7f) {
                    // Fixed format.
                    Some(0x70 | 0x71) if sense.len() >= 14 => Error::Sense(sense[2] & 0xf, sense[12], sense[13]),
                    // Descriptor format.
                    Some(0x72 | 0x73) if sense.len() >= 4 => Error::Sense(sense[1] & 0xf, sense[2], sense[3]),
                    _ => Error::Status(status),
                })
            }
            s => Err(Error::Status(s)),
        }
    }

    /// A command with retries for unit attentions and units becoming ready.
    fn command(&mut self, target: u8, lun: u16, cdb: &[u8], data: Data) -> Result<usize, Error> {
        let mut tries = 0;
        loop {
            tries += 1;
            let e = match self.exec(target, lun, cdb, data) {
                Ok(n) => return Ok(n),
                Err(e) => e,
            };
            let retry = match e {
                Error::Sense(UNIT_ATTENTION, _, _) => true,
                Error::Sense(NOT_READY, 0x04, 0x01) => {
                    self.delay(100);
                    true
                }
                Error::Status(0x08) => true, // BUSY
                _ => false,
            };
            if !retry || tries >= 4 {
                return Err(e);
            }
        }
    }
}

/// A logical unit as a block device.
pub struct Unit {
    name: String,
    target: u8,
    lun: u16,
    block: usize,
    blocks: u64,
    ctrl: Arc<Mutex<Ctrl>>,
}

/// READ/WRITE(10), or (16) beyond 32-bit block numbers.
fn rw_cdb(write: bool, lba: u64, n: u32) -> Vec<u8> {
    if lba + n as u64 <= u32::MAX as u64 {
        let op = if write { WRITE_10 } else { READ_10 };
        let l = (lba as u32).to_be_bytes();
        let c = (n as u16).to_be_bytes();
        alloc::vec![op, 0, l[0], l[1], l[2], l[3], 0, c[0], c[1], 0]
    } else {
        let op = if write { WRITE_16 } else { READ_16 };
        let mut v = alloc::vec![op, 0];
        v.extend_from_slice(&lba.to_be_bytes());
        v.extend_from_slice(&n.to_be_bytes());
        v.extend_from_slice(&[0, 0]);
        v
    }
}

impl Unit {
    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let bs = self.block;
        if buf.len() % bs != 0 || lba + (buf.len() / bs) as u64 > self.blocks {
            return Err(err::INVALID_PARAMETER);
        }
        let mut c = self.ctrl.lock();
        let per = (MAX_XFER / bs).min(c.max_sectors * 512 / bs).max(1);
        let total = buf.len() / bs;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(per);
            let bytes = n * bs;
            let at = lba + done as u64;
            let chunk = &mut buf[done * bs..done * bs + bytes];
            if write {
                c.mem.write(DATA, chunk);
            }
            let data = if write { Data::Out(bytes) } else { Data::In(bytes) };
            match c.command(self.target, self.lun, &rw_cdb(write, at, n as u32), data) {
                Ok(got) if got == bytes => {}
                Ok(got) => {
                    crate::println!("{}: short transfer ({got} of {bytes} bytes)", self.name);
                    return Err(err::NOT_READY);
                }
                Err(e) => {
                    crate::println!("{}: {} at {at} failed: {e:?}", self.name, if write { "write" } else { "read" });
                    return Err(e.code());
                }
            }
            if !write {
                chunk.copy_from_slice(c.mem.bytes(DATA, bytes));
            }
            done += n;
        }
        Ok(())
    }
}

impl BlockDev for Unit {
    fn sector_size(&self) -> usize {
        self.block
    }

    fn sector_count(&self) -> u64 {
        self.blocks
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.transfer(lba, buf, false)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut tmp = buf.to_vec();
        self.transfer(lba, &mut tmp, true)
    }

    fn packet(&self, cdb: &[u8], buf: &mut [u8]) -> Result<usize, u32> {
        if buf.len() > MAX_XFER {
            return Err(err::NOT_SUPPORTED);
        }
        let mut c = self.ctrl.lock();
        let data = if buf.is_empty() { Data::None } else { Data::In(buf.len()) };
        let n = c.command(self.target, self.lun, cdb, data).map_err(Error::code)?.min(buf.len());
        buf[..n].copy_from_slice(c.mem.bytes(DATA, n));
        Ok(n)
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn ascii(b: &[u8]) -> String {
    let s: String = b.iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { ' ' }).collect();
    String::from(s.trim())
}

/// The LUNs of `target` (REPORT LUNS), None if there is no such target.
fn luns(c: &mut Ctrl, target: u8) -> Option<Vec<u16>> {
    let len = 8 + 8 * 32;
    let mut cdb = [0u8; 12];
    cdb[0] = REPORT_LUNS;
    cdb[6..10].copy_from_slice(&(len as u32).to_be_bytes());
    match c.command(target, 0, &cdb, Data::In(len)) {
        Ok(got) if got >= 8 => {
            let b = c.mem.bytes(DATA, got);
            let n = (be32(b) as usize / 8).min((got - 8) / 8);
            let mut v: Vec<u16> = (0..n)
                .filter_map(|i| {
                    let e = &b[8 + i * 8..16 + i * 8];
                    match e[0] >> 6 {
                        0 if e[0] == 0 => Some(e[1] as u16),
                        1 => Some(((e[0] & 0x3f) as u16) << 8 | e[1] as u16),
                        _ => None,
                    }
                })
                .collect();
            v.sort_unstable();
            v.dedup();
            v.truncate(MAX_LUNS);
            Some(v)
        }
        Err(Error::Response(S_BAD_TARGET)) => None,
        Err(Error::Response(_)) | Err(Error::Timeout) => None,
        // Not supported: probe LUN 0.
        _ => Some(alloc::vec![0]),
    }
}

/// INQUIRY, wait until ready, READ CAPACITY. Returns the unit and whether
/// it is an optical drive (which is kept without a medium).
fn setup_unit(ctrl: &Arc<Mutex<Ctrl>>, target: u8, lun: u16) -> Option<(Unit, bool)> {
    let mut c = ctrl.lock();
    let name = alloc::format!("{} {target}:{lun}", c.name);
    c.command(target, lun, &[INQUIRY, 0, 0, 0, 36, 0], Data::In(36)).ok()?;
    let inq = c.mem.bytes(DATA, 36).to_vec();
    let kind = inq[0] & 0x1f;
    let (vendor, product) = (ascii(&inq[8..16]), ascii(&inq[16..32]));
    if inq[0] >> 5 != 0 {
        return None;
    }
    if !matches!(kind, TYPE_DISK | TYPE_CDROM | TYPE_OPTICAL | TYPE_RBC) {
        crate::println!("{name}: {vendor} {product}, device type {kind:#x} not supported");
        return None;
    }
    let is_optical = matches!(kind, TYPE_CDROM | TYPE_OPTICAL);
    let unit = |block, blocks| Unit { name: name.clone(), target, lun, block, blocks, ctrl: ctrl.clone() };
    let mut waited = 0;
    loop {
        match c.command(target, lun, &[TEST_UNIT_READY, 0, 0, 0, 0, 0], Data::None) {
            Ok(_) => break,
            Err(Error::Sense(NOT_READY, ASC_NO_MEDIUM, _)) => {
                crate::println!("{name}: {vendor} {product}, no medium");
                return is_optical.then(|| (unit(2048, 0), true));
            }
            Err(e @ (Error::Response(_) | Error::Timeout)) => {
                crate::println!("{name}: {vendor} {product}: {e:?}");
                return None;
            }
            Err(e) if waited >= READY_MS => {
                crate::println!("{name}: {vendor} {product}, not ready ({e:?})");
                return None;
            }
            Err(_) => {
                c.delay(100);
                waited += 100;
            }
        }
    }
    c.command(target, lun, &[READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0], Data::In(8)).ok()?;
    let cap = c.mem.bytes(DATA, 8).to_vec();
    let (mut last, mut block) = (be32(&cap) as u64, be32(&cap[4..]) as usize);
    if last == u32::MAX as u64 {
        let mut cdb = [0u8; 16];
        cdb[0] = SERVICE_ACTION_IN;
        cdb[1] = READ_CAPACITY_16;
        cdb[13] = 32;
        c.command(target, lun, &cdb, Data::In(32)).ok()?;
        let cap = c.mem.bytes(DATA, 12).to_vec();
        last = (be32(&cap) as u64) << 32 | be32(&cap[4..]) as u64;
        block = be32(&cap[8..]) as usize;
    }
    if block == 0 || block > MAX_XFER || !block.is_power_of_two() {
        crate::println!("{name}: unsupported block size {block}");
        return None;
    }
    let blocks = last + 1;
    crate::println!(
        "{name}: {} {vendor} {product}, {} MB ({blocks} blocks of {block} bytes)",
        if is_optical { "optical" } else { "disk" },
        (blocks * block as u64) >> 20
    );
    Some((unit(block, blocks), is_optical && block == 2048))
}

/// Set up a virtio-scsi adapter: its units, each with whether it is an
/// optical drive.
pub fn attach(mut dev: Device) -> Vec<(Arc<dyn BlockDev>, bool)> {
    let mut out: Vec<(Arc<dyn BlockDev>, bool)> = Vec::new();
    if dev.negotiate(0).is_none() {
        return out;
    }
    let name = alloc::format!("virtio-scsi {}", dev.name());
    let Some(q) = dev.queue_irq(REQUEST_QUEUE, 64) else {
        crate::println!("{name}: no request queue");
        dev.fail();
        return out;
    };
    let Some(mem) = Dma::new(DATA + MAX_XFER) else {
        dev.fail();
        return out;
    };
    dev.ready();
    let sense_size = (dev.cfg32(CFG_SENSE_SIZE) as usize).clamp(18, RESP - 12);
    let cdb_size = (dev.cfg32(CFG_CDB_SIZE) as usize).clamp(16, RESP - 19);
    let max_target = dev.cfg16(CFG_MAX_TARGET).min(255) as u8;
    let max_sectors = (dev.cfg32(CFG_MAX_SECTORS) as usize).max(8);
    let ctrl = Ctrl { name: name.clone(), q, mem, cdb_size, sense_size, next_tag: 0, max_sectors, failed: false, dev };
    let ctrl = Arc::new(Mutex::new(ctrl));
    for target in 0..=max_target {
        let Some(list) = luns(&mut ctrl.lock(), target) else { continue };
        for lun in list {
            if let Some((u, optical)) = setup_unit(&ctrl, target, lun) {
                out.push((Arc::new(u), optical));
            }
        }
        if ctrl.lock().failed {
            break;
        }
    }
    if out.is_empty() {
        crate::println!("{name}: no units");
    }
    out
}
