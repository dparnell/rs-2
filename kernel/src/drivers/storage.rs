//! USB mass storage: Bulk-Only Transport (USB MSC BOT 1.0) with the SCSI
//! transparent command set. Every logical unit with a medium becomes a
//! block device.
//!
//! Commands run in the thread doing the I/O: the CBW, data and CSW stages
//! are bulk transfers that `xhci` queues and then waits for (sleeping
//! until the controller's interrupt, or polling and yielding the CPU
//! without one). BOT allows one command at a time per device, so a sleeping mutex
//! serialises the logical units, and it also owns the DMA buffers. Error
//! handling follows the BOT specification: a stalled data stage clears
//! the endpoint halt and still reads the CSW (retried once after a stall);
//! a CBW that is not accepted, a CSW that cannot be read or makes no sense,
//! and a phase error lead to reset recovery (Bulk-Only Mass Storage Reset,
//! then clearing the halt of both bulk endpoints). A failed command
//! fetches sense data; UNIT ATTENTION and NOT READY (becoming ready) are
//! retried.
//!
//! Every command costs three bulk round trips, and the file systems read
//! the same sectors (FAT, directories) over and over, one at a time. So
//! small reads go through a cache of aligned 4 KB chunks per unit; writes
//! go straight to the device and update the chunks they overlap.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::usb::{self, Setup};
use super::xhci::{self, Handle, StorageDev, XferError};
use crate::api::err;
use crate::fs::block::BlockDev;
use crate::mm::phys;
use crate::sync::Mutex;

const PAGE: usize = 4096;
/// Data buffer: the largest transfer per command.
const DATA_PAGES: usize = 16;
const MAX_XFER: usize = DATA_PAGES * PAGE;
/// Offsets in the command page: CBW, CSW, small replies (sense, LUNs).
const CBW_OFF: u64 = 0;
const CSW_OFF: u64 = 64;
const SMALL_OFF: u64 = 128;

const CBW_SIG: u32 = 0x4342_5355;
const CSW_SIG: u32 = 0x5342_5355;
const CBW_LEN: usize = 31;
const CSW_LEN: usize = 13;

const TIMEOUT_MS: u64 = 10_000;
const CONTROL_TIMEOUT_MS: u64 = 2000;
/// How long a unit may take to become ready at attach time.
const READY_MS: u64 = 5000;
/// Read cache: chunk size and number of chunks per unit.
const CHUNK: usize = 4096;
const CACHE_CHUNKS: usize = 64;

// SCSI operation codes.
const TEST_UNIT_READY: u8 = 0x00;
const REQUEST_SENSE: u8 = 0x03;
const INQUIRY: u8 = 0x12;
const READ_CAPACITY_10: u8 = 0x25;
const READ_10: u8 = 0x28;
const WRITE_10: u8 = 0x2a;
const READ_16: u8 = 0x88;
const WRITE_16: u8 = 0x8a;
const SERVICE_ACTION_IN: u8 = 0x9e;
const READ_CAPACITY_16: u8 = 0x10;

// Sense keys.
const NOT_READY: u8 = 2;
const UNIT_ATTENTION: u8 = 6;
const DATA_PROTECT: u8 = 7;
/// Additional sense code: medium not present.
const ASC_NO_MEDIUM: u8 = 0x3a;

/// Peripheral device types (INQUIRY byte 0).
const TYPE_DISK: u8 = 0x00;
const TYPE_CDROM: u8 = 0x05;
const TYPE_OPTICAL: u8 = 0x07;
const TYPE_RBC: u8 = 0x0e;

#[derive(Clone, Copy, Debug)]
enum Error {
    /// The command failed; sense key, ASC, ASCQ.
    Sense(u8, u8, u8),
    /// The command failed and sense data could not be read.
    Check,
    /// Transport error (after reset recovery).
    Io(XferError),
}

impl Error {
    fn code(self) -> u32 {
        match self {
            Error::Sense(DATA_PROTECT, _, _) => err::WRITE_PROTECT,
            _ => err::NOT_READY,
        }
    }
}

impl From<XferError> for Error {
    fn from(e: XferError) -> Error {
        Error::Io(e)
    }
}

/// Physically contiguous pages, freed on drop.
struct Dma {
    base: u64,
    pages: usize,
}

impl Dma {
    fn new(pages: usize) -> Option<Dma> {
        // 32-bit addresses suit every controller.
        Some(Dma { base: phys::alloc_contig(pages as u64, 1 << 32)?, pages })
    }

    fn ptr(&self, off: u64) -> *mut u8 {
        phys::phys_to_virt(self.base + off)
    }
}

impl Drop for Dma {
    fn drop(&mut self) {
        for i in 0..self.pages {
            phys::free(self.base + (i * PAGE) as u64);
        }
    }
}

/// Command state behind the device lock.
struct State {
    tag: u32,
    /// CBW, CSW and small replies.
    cmd: Dma,
    data: Dma,
}

unsafe impl Send for State {}

/// A Bulk-Only device.
struct Bot {
    dev: StorageDev,
    state: Mutex<State>,
}

/// Direction and buffer of a command's data stage.
#[derive(Clone, Copy)]
enum Data {
    None,
    /// `len` bytes into the buffer at physical `addr`.
    In(u64, usize),
    Out(u64, usize),
}

impl Bot {
    fn h(&self) -> Handle {
        self.dev.handle
    }

    /// CLEAR_FEATURE(ENDPOINT_HALT) on both sides of a bulk endpoint.
    fn clear_halt(&self, st: &State, ep: &usb::Endpoint) -> Result<(), XferError> {
        xhci::clear_halt(self.h(), ep.dci())?;
        xhci::control(self.h(), Setup::clear_halt(ep.address), st.cmd.base + SMALL_OFF, CONTROL_TIMEOUT_MS)?;
        Ok(())
    }

    /// Reset recovery: Bulk-Only Mass Storage Reset, then clear both halts.
    fn reset_recovery(&self, st: &State) {
        crate::println!("{}: reset recovery", self.dev.name);
        let r = xhci::control(self.h(), Setup::msc_reset(self.dev.iface), st.cmd.base + SMALL_OFF, CONTROL_TIMEOUT_MS);
        if r == Err(XferError::Stall) {
            let _ = xhci::clear_halt(self.h(), 1);
        }
        let _ = self.clear_halt(st, &self.dev.ep_in);
        let _ = self.clear_halt(st, &self.dev.ep_out);
    }

    /// Fail with `e`, recovering the device first unless it is gone.
    fn recover<T>(&self, st: &State, e: XferError) -> Result<T, Error> {
        if e != XferError::Gone {
            self.reset_recovery(st);
        }
        Err(Error::Io(e))
    }

    fn read_csw(&self, st: &State) -> Result<usize, XferError> {
        xhci::bulk(self.h(), &self.dev.ep_in, st.cmd.base + CSW_OFF, CSW_LEN, TIMEOUT_MS)
    }

    /// One command through the three BOT stages. Ok(residue) if it
    /// passed, Err(Check) if it failed.
    fn transport(&self, st: &mut State, lun: u8, cdb: &[u8], data: Data) -> Result<u32, Error> {
        st.tag = st.tag.wrapping_add(1);
        let tag = st.tag;
        let (len, dir_in) = match data {
            Data::None => (0, false),
            Data::In(_, n) => (n, true),
            Data::Out(_, n) => (n, false),
        };
        let mut cbw = [0u8; CBW_LEN];
        cbw[0..4].copy_from_slice(&CBW_SIG.to_le_bytes());
        cbw[4..8].copy_from_slice(&tag.to_le_bytes());
        cbw[8..12].copy_from_slice(&(len as u32).to_le_bytes());
        cbw[12] = if dir_in { 0x80 } else { 0 };
        cbw[13] = lun;
        cbw[14] = cdb.len() as u8;
        cbw[15..15 + cdb.len()].copy_from_slice(cdb);
        unsafe { core::ptr::copy_nonoverlapping(cbw.as_ptr(), st.cmd.ptr(CBW_OFF), CBW_LEN) };
        if let Err(e) = xhci::bulk(self.h(), &self.dev.ep_out, st.cmd.base + CBW_OFF, CBW_LEN, TIMEOUT_MS) {
            return self.recover(st, e);
        }
        let stage = match data {
            Data::None => Ok(0),
            Data::In(addr, n) => xhci::bulk(self.h(), &self.dev.ep_in, addr, n, TIMEOUT_MS),
            Data::Out(addr, n) => xhci::bulk(self.h(), &self.dev.ep_out, addr, n, TIMEOUT_MS),
        };
        match stage {
            Ok(_) => {}
            // The device ends the data stage early with a stall: clear it
            // and read the CSW.
            Err(XferError::Stall) => {
                let ep = if dir_in { self.dev.ep_in } else { self.dev.ep_out };
                if let Err(e) = self.clear_halt(st, &ep) {
                    return self.recover(st, e);
                }
            }
            Err(e) => return self.recover(st, e),
        }
        unsafe { core::ptr::write_bytes(st.cmd.ptr(CSW_OFF), 0, CSW_LEN) };
        let csw = match self.read_csw(st) {
            Err(XferError::Stall) => match self.clear_halt(st, &self.dev.ep_in) {
                Ok(()) => self.read_csw(st),
                Err(e) => Err(e),
            },
            r => r,
        };
        if let Err(e) = csw {
            return self.recover(st, e);
        }
        let mut c = [0u8; CSW_LEN];
        unsafe { core::ptr::copy_nonoverlapping(st.cmd.ptr(CSW_OFF), c.as_mut_ptr(), CSW_LEN) };
        let le32 = |o: usize| u32::from_le_bytes([c[o], c[o + 1], c[o + 2], c[o + 3]]);
        let valid = csw == Ok(CSW_LEN) && le32(0) == CSW_SIG && le32(4) == tag;
        match c[12] {
            0 if valid => Ok(le32(8)),
            1 if valid => Err(Error::Check),
            _ => {
                // Phase error or a meaningless CSW.
                self.reset_recovery(st);
                Err(Error::Io(XferError::Failed(0)))
            }
        }
    }

    fn request_sense(&self, st: &mut State, lun: u8) -> Result<(u8, u8, u8), Error> {
        let cdb = [REQUEST_SENSE, 0, 0, 0, 18, 0];
        let buf = st.cmd.base + SMALL_OFF;
        unsafe { core::ptr::write_bytes(st.cmd.ptr(SMALL_OFF), 0, 18) };
        self.transport(st, lun, &cdb, Data::In(buf, 18))?;
        let s = unsafe { core::slice::from_raw_parts(st.cmd.ptr(SMALL_OFF), 18) };
        Ok((s[2] & 0xf, s[12], s[13]))
    }

    /// A command with sense handling and retries; returns the bytes
    /// transferred.
    fn command(&self, st: &mut State, lun: u8, cdb: &[u8], data: Data) -> Result<usize, Error> {
        let len = match data {
            Data::None => 0,
            Data::In(_, n) | Data::Out(_, n) => n,
        };
        let mut tries = 0;
        loop {
            tries += 1;
            let e = match self.transport(st, lun, cdb, data) {
                Ok(residue) => return Ok(len.saturating_sub(residue as usize)),
                Err(Error::Check) => match self.request_sense(st, lun) {
                    Ok((key, asc, ascq)) => Error::Sense(key, asc, ascq),
                    Err(_) => Error::Check,
                },
                Err(e) => e,
            };
            let retry = match e {
                Error::Sense(UNIT_ATTENTION, _, _) => true,
                // Becoming ready.
                Error::Sense(NOT_READY, 0x04, 0x01) => {
                    xhci::delay(self.h(), 100);
                    true
                }
                Error::Io(XferError::Gone) => false,
                Error::Io(_) => true,
                _ => false,
            };
            if !retry || tries >= 4 {
                return Err(e);
            }
        }
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be64(b: &[u8]) -> u64 {
    (be32(b) as u64) << 32 | be32(&b[4..]) as u64
}

/// Recently read chunks: (chunk number, last use, data).
struct Cache {
    chunks: Vec<(u64, u64, Vec<u8>)>,
    uses: u64,
}

/// A logical unit as a block device.
pub struct Lun {
    bot: Arc<Bot>,
    lun: u8,
    block: usize,
    blocks: u64,
    cache: Mutex<Cache>,
}

impl Lun {
    /// Blocks per cache chunk.
    fn per_chunk(&self) -> u64 {
        (CHUNK / self.block).max(1) as u64
    }

    /// A read within one chunk, through the cache.
    fn cached_read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        let per = self.per_chunk();
        let n = lba / per;
        let mut c = self.cache.lock();
        c.uses += 1;
        let uses = c.uses;
        let i = match c.chunks.iter().position(|e| e.0 == n) {
            Some(i) => i,
            None => {
                let first = n * per;
                let count = per.min(self.blocks - first) as usize;
                let mut data = alloc::vec![0u8; count * self.block];
                self.transfer(first, &mut data, false)?;
                if c.chunks.len() >= CACHE_CHUNKS {
                    let old = (0..c.chunks.len()).min_by_key(|&i| c.chunks[i].1).unwrap_or(0);
                    c.chunks.swap_remove(old);
                }
                c.chunks.push((n, uses, data));
                c.chunks.len() - 1
            }
        };
        let e = &mut c.chunks[i];
        e.1 = uses;
        let off = ((lba - n * per) as usize) * self.block;
        buf.copy_from_slice(&e.2[off..off + buf.len()]);
        Ok(())
    }

    /// Blocks at `lba` were written with `data` (None: the write failed):
    /// update the cached chunks they overlap, or forget them.
    fn written(&self, lba: u64, count: u64, data: Option<&[u8]>) {
        let (per, bs) = (self.per_chunk(), self.block as u64);
        let end = lba + count;
        let mut c = self.cache.lock();
        c.chunks.retain(|e| data.is_some() || e.0 * per >= end || (e.0 + 1) * per <= lba);
        let Some(data) = data else { return };
        for e in c.chunks.iter_mut() {
            let (first, last) = (e.0 * per, e.0 * per + (e.2.len() as u64) / bs);
            let (from, to) = (lba.max(first), end.min(last));
            if from < to {
                let src = &data[((from - lba) * bs) as usize..((to - lba) * bs) as usize];
                e.2[((from - first) * bs) as usize..((to - first) * bs) as usize].copy_from_slice(src);
            }
        }
    }

    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let bs = self.block;
        if buf.len() % bs != 0 || lba + (buf.len() / bs) as u64 > self.blocks {
            return Err(err::INVALID_PARAMETER);
        }
        let mut st = self.bot.state.lock();
        let per = MAX_XFER / bs;
        let mut done = 0;
        let total = buf.len() / bs;
        while done < total {
            let n = (total - done).min(per);
            let bytes = n * bs;
            let at = lba + done as u64;
            let chunk = &mut buf[done * bs..done * bs + bytes];
            let addr = st.data.base;
            if write {
                unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), st.data.ptr(0), bytes) };
            }
            let cdb = rw_cdb(write, at, n as u32);
            let data = if write { Data::Out(addr, bytes) } else { Data::In(addr, bytes) };
            match self.bot.command(&mut st, self.lun, &cdb, data) {
                Ok(got) if got == bytes => {}
                Ok(got) => {
                    crate::println!("{} LUN {}: short transfer ({got} of {bytes} bytes)", self.bot.dev.name, self.lun);
                    return Err(err::NOT_READY);
                }
                Err(e) => {
                    let op = if write { "write" } else { "read" };
                    crate::println!("{} LUN {}: {op} at {at} failed: {e:?}", self.bot.dev.name, self.lun);
                    return Err(e.code());
                }
            }
            if !write {
                unsafe { core::ptr::copy_nonoverlapping(st.data.ptr(0), chunk.as_mut_ptr(), bytes) };
            }
            done += n;
        }
        Ok(())
    }
}

/// READ/WRITE(10), or (16) beyond 32-bit block numbers.
pub(super) fn rw_cdb(write: bool, lba: u64, n: u32) -> Vec<u8> {
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

impl BlockDev for Lun {
    fn sector_size(&self) -> usize {
        self.block
    }

    fn usb_device(&self) -> Option<xhci::Handle> {
        Some(self.bot.dev.handle)
    }

    fn sector_count(&self) -> u64 {
        self.blocks
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        let count = (buf.len() / self.block) as u64;
        let per = self.per_chunk();
        let valid = buf.len() % self.block == 0 && count > 0 && lba + count <= self.blocks;
        if valid && lba / per == (lba + count - 1) / per {
            self.cached_read(lba, buf)
        } else {
            self.transfer(lba, buf, false)
        }
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut tmp = buf.to_vec();
        let r = self.transfer(lba, &mut tmp, true);
        self.written(lba, (buf.len() / self.block) as u64, r.is_ok().then_some(buf));
        r
    }
}

/// Text of an INQUIRY string field.
fn ascii(b: &[u8]) -> String {
    let s: String = b.iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { ' ' }).collect();
    String::from(s.trim())
}

/// Get Max LUN; devices with one LUN may stall it.
fn max_lun(bot: &Bot, st: &State) -> u8 {
    unsafe { *st.cmd.ptr(SMALL_OFF) = 0 };
    match xhci::control(bot.h(), Setup::get_max_lun(bot.dev.iface), st.cmd.base + SMALL_OFF, CONTROL_TIMEOUT_MS) {
        Ok(1) => unsafe { (*st.cmd.ptr(SMALL_OFF)).min(15) },
        Err(XferError::Stall) => {
            let _ = xhci::clear_halt(bot.h(), 1);
            0
        }
        _ => 0,
    }
}

/// INQUIRY, wait until ready, READ CAPACITY. Returns the block device
/// and whether it is an optical drive, or None if there is no usable
/// medium.
fn setup_lun(bot: &Arc<Bot>, st: &mut State, lun: u8) -> Option<(Lun, bool)> {
    let name = &bot.dev.name;
    let buf = st.data.base;
    bot.command(st, lun, &[INQUIRY, 0, 0, 0, 36, 0], Data::In(buf, 36)).ok()?;
    let inq = unsafe { core::slice::from_raw_parts(st.data.ptr(0), 36) };
    let kind = inq[0] & 0x1f;
    let (vendor, product) = (ascii(&inq[8..16]), ascii(&inq[16..32]));
    if inq[0] >> 5 != 0 || !matches!(kind, TYPE_DISK | TYPE_CDROM | TYPE_OPTICAL | TYPE_RBC) {
        crate::println!("{name} LUN {lun}: {vendor} {product}, device type {kind:#x} not supported");
        return None;
    }
    // TEST UNIT READY until the unit is ready or has no medium.
    let mut waited = 0;
    loop {
        match bot.command(st, lun, &[TEST_UNIT_READY, 0, 0, 0, 0, 0], Data::None) {
            Ok(_) => break,
            Err(Error::Sense(NOT_READY, ASC_NO_MEDIUM, _)) => {
                crate::println!("{name} LUN {lun}: {vendor} {product}, no medium");
                return None;
            }
            Err(Error::Io(XferError::Gone)) => return None,
            Err(e) if waited >= READY_MS => {
                crate::println!("{name} LUN {lun}: {vendor} {product}, not ready ({e:?})");
                return None;
            }
            Err(_) => {
                xhci::delay(bot.h(), 100);
                waited += 100;
            }
        }
    }
    bot.command(st, lun, &[READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0], Data::In(buf, 8)).ok()?;
    let cap = unsafe { core::slice::from_raw_parts(st.data.ptr(0), 32) };
    let (mut last, mut block) = (be32(cap) as u64, be32(&cap[4..]) as usize);
    if last == u32::MAX as u64 {
        let mut cdb = [0u8; 16];
        cdb[0] = SERVICE_ACTION_IN;
        cdb[1] = READ_CAPACITY_16;
        cdb[13] = 32;
        bot.command(st, lun, &cdb, Data::In(buf, 32)).ok()?;
        let cap = unsafe { core::slice::from_raw_parts(st.data.ptr(0), 32) };
        last = be64(cap);
        block = be32(&cap[8..]) as usize;
    }
    if block == 0 || block > MAX_XFER || !block.is_power_of_two() {
        crate::println!("{name} LUN {lun}: unsupported block size {block}");
        return None;
    }
    let blocks = last + 1;
    crate::println!(
        "{name} LUN {lun}: {vendor} {product}, {} MB ({blocks} blocks of {block} bytes)",
        (blocks * block as u64) >> 20
    );
    let optical = matches!(kind, TYPE_CDROM | TYPE_OPTICAL) && block == 2048;
    let cache = Mutex::new(Cache { chunks: Vec::new(), uses: 0 });
    Some((Lun { bot: bot.clone(), lun, block, blocks, cache }, optical))
}

/// Set up a configured Bulk-Only device: its logical units with media,
/// each as a block device with whether it is an optical drive.
pub fn attach(dev: StorageDev) -> Vec<(Arc<dyn BlockDev>, bool)> {
    let mut out: Vec<(Arc<dyn BlockDev>, bool)> = Vec::new();
    let (Some(cmd), Some(data)) = (Dma::new(1), Dma::new(DATA_PAGES)) else { return out };
    crate::println!("{}: mass storage {:04x}:{:04x}", dev.name, dev.vendor, dev.product);
    let bot = Arc::new(Bot { dev, state: Mutex::new(State { tag: 0, cmd, data }) });
    let mut st = bot.state.lock();
    let luns = max_lun(&bot, &st);
    for lun in 0..=luns {
        if let Some((l, optical)) = setup_lun(&bot, &mut st, lun) {
            out.push((Arc::new(l), optical));
        }
    }
    drop(st);
    out
}
