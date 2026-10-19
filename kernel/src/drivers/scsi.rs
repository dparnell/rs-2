//! SCSI disks and CD/DVD drives behind a parallel/SAS-style host adapter
//! (PVSCSI, LSI Logic MPT): the command set on top of an adapter's
//! "execute one CDB" primitive. Error handling: sense data comes from the
//! adapter's automatic sense buffer or REQUEST SENSE; UNIT ATTENTION and
//! "becoming ready" are retried.
//!
//! (USB mass storage, `storage.rs`, has its own transport-specific
//! version of this.)

use alloc::string::String;
use alloc::sync::Arc;

use crate::api::err;
use crate::fs::block::BlockDev;
use crate::sync::Mutex;

// Operation codes.
pub const TEST_UNIT_READY: u8 = 0x00;
pub const REQUEST_SENSE: u8 = 0x03;
pub const INQUIRY: u8 = 0x12;
pub const READ_CAPACITY_10: u8 = 0x25;
pub const SERVICE_ACTION_IN: u8 = 0x9e;
pub const READ_CAPACITY_16: u8 = 0x10;
// Status.
pub const STATUS_GOOD: u8 = 0x00;
pub const STATUS_CHECK_CONDITION: u8 = 0x02;
pub const STATUS_BUSY: u8 = 0x08;
// Sense keys.
const NOT_READY: u8 = 2;
const UNIT_ATTENTION: u8 = 6;
const DATA_PROTECT: u8 = 7;
const ASC_NO_MEDIUM: u8 = 0x3a;
// Peripheral device types.
const TYPE_DISK: u8 = 0x00;
const TYPE_CDROM: u8 = 0x05;
const TYPE_OPTICAL: u8 = 0x07;

/// How long a unit may take to become ready at attach time.
const READY_MS: u64 = 5000;

#[derive(Clone, Copy, Debug)]
pub enum Error {
    /// CHECK CONDITION with sense key, ASC, ASCQ.
    Sense(u8, u8, u8),
    /// CHECK CONDITION without sense data (REQUEST SENSE needed).
    Check,
    /// Another SCSI status (busy, reservation conflict, ...).
    Status(u8),
    /// Host adapter error: no device at the target, bus errors.
    Host(u32),
    Timeout,
}

impl Error {
    pub fn code(self) -> u32 {
        match self {
            Error::Sense(DATA_PROTECT, _, _) => err::WRITE_PROTECT,
            _ => err::NOT_READY,
        }
    }
}

/// Direction and length of a command's data (in the bounce buffer).
#[derive(Clone, Copy)]
pub enum Dir {
    None,
    In(usize),
    Out(usize),
}

/// A host adapter: runs one command at a time on its bounce buffer.
pub trait Hba: Send {
    /// Name for messages ("pvscsi", "mpt").
    fn name(&self) -> &'static str;
    /// One command, no retries: Ok(bytes transferred).
    fn exec(&mut self, target: u8, lun: u8, cdb: &[u8], dir: Dir) -> Result<usize, Error>;
    /// The DMA bounce buffer and its size.
    fn bounce(&self) -> *mut u8;
    fn bounce_size(&self) -> usize;
}

/// Sense data parsed from fixed-format sense bytes, if valid.
pub fn parse_sense(s: &[u8]) -> Option<Error> {
    (s.len() >= 14 && matches!(s[0] & 0x7f, 0x70 | 0x71)).then(|| Error::Sense(s[2] & 0xf, s[12], s[13]))
}

/// Poll `cond` for up to `ms` milliseconds, yielding the CPU (time is
/// estimated from the number of polls before the timer runs).
pub fn wait(ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let t0 = crate::irq::ticks();
    let limit = ms * crate::irq::HZ / 1000;
    let mut polls = 0u64;
    loop {
        if cond() {
            return true;
        }
        polls += 1;
        let now = crate::irq::ticks();
        let expired = if now != t0 { now - t0 > limit } else { polls > ms * 2000 };
        if expired {
            return cond();
        }
        if polls > 100 {
            crate::task::sched::yield_now();
        }
        core::hint::spin_loop();
    }
}

/// A command with sense handling and retries.
pub fn command<H: Hba + ?Sized>(h: &mut H, target: u8, lun: u8, cdb: &[u8], dir: Dir) -> Result<usize, Error> {
    let mut tries = 0;
    loop {
        tries += 1;
        let e = match h.exec(target, lun, cdb, dir) {
            Ok(n) => return Ok(n),
            Err(Error::Check) => match h.exec(target, lun, &[REQUEST_SENSE, 0, 0, 0, 18, 0], Dir::In(18)) {
                Ok(_) => parse_sense(unsafe { core::slice::from_raw_parts(h.bounce(), 18) }).unwrap_or(Error::Check),
                Err(e) => e,
            },
            Err(e) => e,
        };
        let retry = match e {
            Error::Sense(UNIT_ATTENTION, _, _) => true,
            Error::Sense(NOT_READY, 0x04, 0x01) | Error::Status(STATUS_BUSY) => {
                wait(100, || false);
                true
            }
            _ => false,
        };
        if !retry || tries >= 4 {
            return Err(e);
        }
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn ascii(b: &[u8]) -> String {
    let s: String = b.iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { ' ' }).collect();
    String::from(s.trim())
}

/// A logical unit as a block device.
pub struct Unit<H: Hba> {
    hba: Arc<Mutex<H>>,
    target: u8,
    lun: u8,
    block: usize,
    blocks: u64,
    optical: bool,
}

/// INQUIRY, wait until ready, READ CAPACITY: the unit as a block device
/// and whether it is an optical drive (with 0 blocks if it has no
/// medium), or None if there is no supported unit.
pub fn setup_unit<H: Hba>(hba: &Arc<Mutex<H>>, target: u8, lun: u8) -> Option<(Unit<H>, bool)> {
    let mut h = hba.lock();
    let name = h.name();
    let n = command(&mut *h, target, lun, &[INQUIRY, 0, 0, 0, 36, 0], Dir::In(36)).ok()?;
    if n < 5 {
        return None;
    }
    let inq = unsafe { core::slice::from_raw_parts(h.bounce(), 36) }.to_vec();
    let kind = inq[0] & 0x1f;
    let (vendor, product) = (ascii(&inq[8..16]), ascii(&inq[16..32]));
    if inq[0] >> 5 != 0 {
        return None; // no unit at this LUN
    }
    let optical = matches!(kind, TYPE_CDROM | TYPE_OPTICAL);
    if kind != TYPE_DISK && !optical {
        crate::println!("{name}: target {target}: {vendor} {product}, device type {kind:#x} not supported");
        return None;
    }
    let unit = |block, blocks| Unit { hba: hba.clone(), target, lun, block, blocks, optical };
    let mut waited = 0;
    loop {
        match command(&mut *h, target, lun, &[TEST_UNIT_READY, 0, 0, 0, 0, 0], Dir::None) {
            Ok(_) => break,
            Err(Error::Sense(NOT_READY, ASC_NO_MEDIUM, _)) if optical => {
                crate::println!("{name}: target {target}: optical {vendor} {product}, no medium");
                return Some((unit(2048, 0), true));
            }
            Err(e) if waited >= READY_MS => {
                crate::println!("{name}: target {target}: {vendor} {product}, not ready ({e:?})");
                return optical.then(|| (unit(2048, 0), true));
            }
            Err(_) => {
                wait(100, || false);
                waited += 100;
            }
        }
    }
    command(&mut *h, target, lun, &[READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0], Dir::In(8)).ok()?;
    let cap = unsafe { core::slice::from_raw_parts(h.bounce(), 32) };
    let (mut last, mut block) = (be32(cap) as u64, be32(&cap[4..]) as usize);
    if last == u32::MAX as u64 {
        let mut cdb = [0u8; 16];
        cdb[0] = SERVICE_ACTION_IN;
        cdb[1] = READ_CAPACITY_16;
        cdb[13] = 32;
        command(&mut *h, target, lun, &cdb, Dir::In(32)).ok()?;
        let cap = unsafe { core::slice::from_raw_parts(h.bounce(), 32) };
        last = (be32(cap) as u64) << 32 | be32(&cap[4..]) as u64;
        block = be32(&cap[8..]) as usize;
    }
    if block == 0 || block > h.bounce_size() || !block.is_power_of_two() {
        crate::println!("{name}: target {target}: unsupported block size {block}");
        return None;
    }
    let blocks = last + 1;
    crate::println!(
        "{name}: target {target}: {} {vendor} {product}, {} MB",
        if optical { "optical" } else { "disk" },
        (blocks * block as u64) >> 20
    );
    Some((unit(block, blocks), optical))
}

impl<H: Hba> Unit<H> {
    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let bs = self.block;
        if buf.len() % bs != 0 || lba + (buf.len() / bs) as u64 > self.blocks {
            return Err(err::INVALID_PARAMETER);
        }
        if write && self.optical {
            return Err(err::WRITE_PROTECT);
        }
        let mut h = self.hba.lock();
        let per = h.bounce_size() / bs;
        let total = buf.len() / bs;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(per);
            let bytes = n * bs;
            let at = lba + done as u64;
            let chunk = &mut buf[done * bs..done * bs + bytes];
            if write {
                unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), h.bounce(), bytes) };
            }
            let cdb = super::storage::rw_cdb(write, at, n as u32);
            let dir = if write { Dir::Out(bytes) } else { Dir::In(bytes) };
            match command(&mut *h, self.target, self.lun, &cdb, dir) {
                Ok(got) if got == bytes => {}
                r => {
                    let op = if write { "write" } else { "read" };
                    crate::println!("{}: target {} {op} at {at} failed: {r:?}", h.name(), self.target);
                    return Err(r.err().map_or(err::NOT_READY, Error::code));
                }
            }
            if !write {
                unsafe { core::ptr::copy_nonoverlapping(h.bounce(), chunk.as_mut_ptr(), bytes) };
            }
            done += n;
        }
        Ok(())
    }
}

impl<H: Hba> BlockDev for Unit<H> {
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
}
