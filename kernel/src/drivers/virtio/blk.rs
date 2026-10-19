//! virtio-blk: every device is a disk.
//!
//! One request queue, one request in flight: a request is the 16-byte
//! header (type, sector), the data buffer and the status byte, each its
//! own descriptor (which legacy devices without ANY_LAYOUT require).
//! Data goes through a bounce buffer; the request sector is always in
//! 512-byte units whatever the device's logical block size. Read-only
//! devices refuse writes; FLUSH is sent on [`BlockDev::flush`] when the
//! device has a volatile write cache.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::pci::Device;
use super::queue::{Buf, Dma, Virtqueue};
use crate::api::err;
use crate::fs::block::BlockDev;
use crate::sync::Mutex;

// Feature bits.
const F_SIZE_MAX: u64 = 1 << 1;
const F_SEG_MAX: u64 = 1 << 2;
const F_RO: u64 = 1 << 5;
const F_BLK_SIZE: u64 = 1 << 6;
const F_FLUSH: u64 = 1 << 9;

// Configuration offsets.
const CFG_CAPACITY: usize = 0;
const CFG_SIZE_MAX: usize = 8;
const CFG_BLK_SIZE: usize = 20;

// Request types and status.
const T_IN: u32 = 0;
const T_OUT: u32 = 1;
const T_FLUSH: u32 = 4;
const T_GET_ID: u32 = 8;
const S_OK: u8 = 0;
const S_UNSUPP: u8 = 2;

/// Bounce buffer: the largest transfer per request.
const BOUNCE: usize = 64 * 1024;
/// Header at 0, status byte at 16, ID string at 64, data at 4096.
const HDR: usize = 0;
const STATUS: usize = 16;
const ID: usize = 64;
const DATA: usize = 4096;
const TIMEOUT_MS: u64 = 30_000;

struct State {
    q: Virtqueue,
    mem: Dma,
    /// Set after a request timed out: the queue is out of step.
    failed: bool,
    _dev: Device,
}

pub struct Blk {
    pub name: String,
    sector_size: usize,
    /// Capacity in `sector_size` blocks.
    sectors: u64,
    read_only: bool,
    can_flush: bool,
    max_xfer: usize,
    state: Mutex<State>,
}

impl State {
    /// Run one request: header, optional data (`len` bytes of the bounce
    /// buffer, written by the device if `into`), status.
    fn request(&mut self, typ: u32, sector: u64, len: usize, into: bool) -> Result<u32, u8> {
        if self.failed {
            return Err(0xff);
        }
        let mut hdr = [0u8; 16];
        hdr[0..4].copy_from_slice(&typ.to_le_bytes());
        hdr[8..16].copy_from_slice(&sector.to_le_bytes());
        self.mem.write(HDR, &hdr);
        self.mem.write(STATUS, &[0xff]);
        let head = Buf::out(self.mem.addr(HDR), 16);
        let status = Buf::into(self.mem.addr(STATUS), 1);
        let data_off = if typ == T_GET_ID { ID } else { DATA };
        let data = Buf { addr: self.mem.addr(data_off), len: len as u32, device_writes: into };
        let r = if len > 0 { self.q.call(&[head, data, status], TIMEOUT_MS) } else { self.q.call(&[head, status], TIMEOUT_MS) };
        let Some(written) = r else {
            self.failed = true;
            return Err(0xff);
        };
        match self.mem.bytes(STATUS, 1)[0] {
            S_OK => Ok(written),
            s => Err(s),
        }
    }
}

impl Blk {
    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let ss = self.sector_size;
        if buf.len() % ss != 0 || lba + (buf.len() / ss) as u64 > self.sectors {
            return Err(err::INVALID_PARAMETER);
        }
        if write && self.read_only {
            return Err(err::WRITE_PROTECT);
        }
        let mut st = self.state.lock();
        let per = (self.max_xfer / ss).max(1);
        let total = buf.len() / ss;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(per);
            let bytes = n * ss;
            let chunk = &mut buf[done * ss..done * ss + bytes];
            if write {
                st.mem.write(DATA, chunk);
            }
            let sector = (lba + done as u64) * (ss / 512) as u64;
            if let Err(s) = st.request(if write { T_OUT } else { T_IN }, sector, bytes, !write) {
                crate::println!("{}: {} at {} failed (status {s:#x})", self.name, if write { "write" } else { "read" }, lba + done as u64);
                return Err(err::NOT_READY);
            }
            if !write {
                chunk.copy_from_slice(st.mem.bytes(DATA, bytes));
            }
            done += n;
        }
        Ok(())
    }
}

impl BlockDev for Blk {
    fn sector_size(&self) -> usize {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.transfer(lba, buf, false)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut tmp = buf.to_vec();
        self.transfer(lba, &mut tmp, true)
    }

    fn flush(&self) -> Result<(), u32> {
        if !self.can_flush || self.read_only {
            return Ok(());
        }
        match self.state.lock().request(T_FLUSH, 0, 0, false) {
            Ok(_) | Err(S_UNSUPP) => Ok(()),
            Err(_) => Err(err::NOT_READY),
        }
    }
}

/// The device's serial number (GET_ID), if it has one.
fn serial(st: &mut State) -> Option<String> {
    st.request(T_GET_ID, 0, 20, true).ok()?;
    let s: String = st.mem.bytes(ID, 20).iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
    let s = String::from(s.trim());
    (!s.is_empty()).then_some(s)
}

/// Set up a virtio-blk device.
pub fn attach(mut dev: Device) -> Vec<Arc<dyn BlockDev>> {
    let mut out: Vec<Arc<dyn BlockDev>> = Vec::new();
    if dev.negotiate(F_SIZE_MAX | F_SEG_MAX | F_RO | F_BLK_SIZE | F_FLUSH).is_none() {
        return out;
    }
    let Some(q) = dev.queue_irq(0, 64) else {
        crate::println!("virtio-blk {}: no request queue", dev.name());
        dev.fail();
        return out;
    };
    let Some(mem) = Dma::new(DATA + BOUNCE) else {
        dev.fail();
        return out;
    };
    dev.ready();
    let capacity = dev.cfg64(CFG_CAPACITY);
    let mut sector_size = 512usize;
    if dev.has(F_BLK_SIZE) {
        let b = dev.cfg32(CFG_BLK_SIZE) as usize;
        if b.is_power_of_two() && (512..=4096).contains(&b) {
            sector_size = b;
        }
    }
    let mut max_xfer = BOUNCE;
    if dev.has(F_SIZE_MAX) {
        let m = dev.cfg32(CFG_SIZE_MAX) as usize;
        if m >= sector_size {
            max_xfer = max_xfer.min(m / sector_size * sector_size);
        }
    }
    let read_only = dev.has(F_RO);
    let can_flush = dev.has(F_FLUSH);
    let name = alloc::format!("virtio-blk {}", dev.name());
    let mut st = State { q, mem, failed: false, _dev: dev };
    let id = serial(&mut st);
    let sectors = capacity * 512 / sector_size as u64;
    crate::println!(
        "{name}: {} MB, {sector_size}-byte sectors{}{}{}",
        capacity >> 11,
        if read_only { ", read-only" } else { "" },
        if can_flush { ", write cache" } else { "" },
        id.map(|s| alloc::format!(", serial {s}")).unwrap_or_default()
    );
    if sectors == 0 {
        return out;
    }
    out.push(Arc::new(Blk { name, sector_size, sectors, read_only, can_flush, max_xfer, state: Mutex::new(st) }));
    out
}
