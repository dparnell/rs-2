//! NVMe (NVM Express) controllers: each active namespace is a disk.
//!
//! One admin queue pair and one I/O queue pair per controller, one command
//! in flight at a time. DMA goes through a physically contiguous bounce
//! buffer described by PRP1/PRP2 (with a PRP list beyond two pages).
//! Completions interrupt through MSI-X entry 0 (or MSI) for both queues;
//! the requesting thread sleeps until the handler wakes it and then checks
//! the phase bit. Without MSI/MSI-X (or before interrupts are delivered,
//! during boot) the phase bit is polled, yielding the CPU.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::pci;
use crate::api::err;
use crate::fs::block::BlockDev;
use crate::mm::{mmio, phys};
use crate::msi::{self, Event};
use crate::sync::Mutex;

const PAGE: usize = 4096;
/// Bounce buffer size; transfers are split into chunks of at most this.
const BOUNCE: usize = 64 * 1024;
/// Entries per queue (64-byte submissions fill one page).
const QUEUE_DEPTH: u16 = 64;
/// Minimum timeout for ordinary commands.
const CMD_TIMEOUT_MS: u64 = 5000;
/// Namespaces probed when the active namespace list is unavailable.
const MAX_NAMESPACES: u32 = 16;

// Controller registers (byte offsets).
const CAP: usize = 0x00;
const VS: usize = 0x08;
const INTMS: usize = 0x0c;
const INTMC: usize = 0x10;
const CC: usize = 0x14;
const CSTS: usize = 0x1c;
const AQA: usize = 0x24;
const ASQ: usize = 0x28;
const ACQ: usize = 0x30;
const DOORBELLS: usize = 0x1000;

const CC_EN: u32 = 1;
const CSTS_RDY: u32 = 1;
const CSTS_CFS: u32 = 2;

// Admin opcodes.
const ADM_CREATE_SQ: u8 = 0x01;
const ADM_CREATE_CQ: u8 = 0x05;
const ADM_IDENTIFY: u8 = 0x06;
// NVM opcodes.
const NVM_WRITE: u8 = 0x01;
const NVM_READ: u8 = 0x02;

// Identify CNS values.
const CNS_NAMESPACE: u32 = 0;
const CNS_CONTROLLER: u32 = 1;
const CNS_ACTIVE_NS: u32 = 2;

/// A submission/completion queue pair.
struct Queue {
    qid: u16,
    sq: u64,
    cq: u64,
    tail: u16,
    head: u16,
    phase: bool,
}

/// Controller state behind the lock.
struct Ctrl {
    regs: *mut u8,
    /// Doorbell stride in bytes (4 << CAP.DSTRD).
    stride: usize,
    /// CAP.TO in milliseconds (enable/disable timeout).
    ready_ms: u64,
    admin: Queue,
    io: Queue,
    next_cid: u16,
    /// Identify data, PRP list and bounce buffer pages.
    ident: u64,
    prp_list: u64,
    buf: u64,
    /// Largest transfer per command, in bytes.
    max_xfer: usize,
    /// Set after a command timed out: the queues are out of step.
    failed: bool,
    /// Signalled by the completion interrupt, if there is one.
    event: Option<&'static Event>,
    msix: bool,
}

unsafe impl Send for Ctrl {}

/// A namespace exposed as a block device.
pub struct Namespace {
    pub nsid: u32,
    pub model: String,
    pub sector_size: usize,
    pub sectors: u64,
    ctrl: Arc<Mutex<Ctrl>>,
}

unsafe impl Send for Namespace {}
unsafe impl Sync for Namespace {}

fn rd32(regs: *mut u8, off: usize) -> u32 {
    unsafe { read_volatile(regs.add(off) as *const u32) }
}

fn wr32(regs: *mut u8, off: usize, v: u32) {
    unsafe { write_volatile(regs.add(off) as *mut u32, v) }
}

/// 64-bit registers as two dword accesses (low first), which every
/// controller accepts.
fn rd64(regs: *mut u8, off: usize) -> u64 {
    rd32(regs, off) as u64 | (rd32(regs, off + 4) as u64) << 32
}

fn wr64(regs: *mut u8, off: usize, v: u64) {
    wr32(regs, off, v as u32);
    wr32(regs, off + 4, (v >> 32) as u32);
}

/// Poll `cond` for up to `ms` milliseconds, yielding the CPU (a status
/// register read paces the polls).
fn wait(regs: *mut u8, ms: u64, cond: impl FnMut() -> bool) -> bool {
    msi::poll(ms, || { let _ = rd32(regs, CSTS); }, cond)
}

/// A 64-byte submission queue entry.
fn command(opcode: u8, nsid: u32, prp1: u64, prp2: u64, cdw: [u32; 6]) -> [u32; 16] {
    let mut c = [0u32; 16];
    c[0] = opcode as u32;
    c[1] = nsid;
    c[6] = prp1 as u32;
    c[7] = (prp1 >> 32) as u32;
    c[8] = prp2 as u32;
    c[9] = (prp2 >> 32) as u32;
    c[10..16].copy_from_slice(&cdw);
    c
}

fn ascii(b: &[u8]) -> String {
    let s: String = b.iter().filter(|&&c| c != 0).map(|&c| c as char).collect();
    String::from(s.trim())
}

impl Ctrl {
    fn sq_doorbell(&self, qid: u16) -> usize {
        DOORBELLS + (2 * qid as usize) * self.stride
    }

    fn cq_doorbell(&self, qid: u16) -> usize {
        DOORBELLS + (2 * qid as usize + 1) * self.stride
    }

    fn cmd_ms(&self) -> u64 {
        self.ready_ms.max(CMD_TIMEOUT_MS)
    }

    /// Submit one command on the admin (`io == false`) or I/O queue and
    /// wait for its completion. Returns completion dword 0.
    fn submit(&mut self, io: bool, mut cmd: [u32; 16]) -> Result<u32, u32> {
        if self.failed {
            return Err(err::NOT_READY);
        }
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        cmd[0] |= (cid as u32) << 16;
        let regs = self.regs;
        let ms = self.cmd_ms();
        let (sq_db, cq_db) = {
            let q = if io { &self.io } else { &self.admin };
            (self.sq_doorbell(q.qid), self.cq_doorbell(q.qid))
        };
        let q = if io { &mut self.io } else { &mut self.admin };
        unsafe {
            let slot = phys::phys_to_virt(q.sq).add(q.tail as usize * 64) as *mut u32;
            for (i, d) in cmd.iter().enumerate() {
                write_volatile(slot.add(i), *d);
            }
        }
        q.tail = (q.tail + 1) % QUEUE_DEPTH;
        fence(Ordering::SeqCst);
        wr32(regs, sq_db, q.tail as u32);
        let entry = unsafe { phys::phys_to_virt(q.cq).add(q.head as usize * 16) as *const u32 };
        let phase = q.phase;
        let cond = || {
            let dw3 = unsafe { read_volatile(entry.add(3)) };
            (dw3 >> 16 & 1 != 0) == phase || rd32(regs, CSTS) & CSTS_CFS != 0
        };
        let done = match self.event {
            Some(e) => e.wait(ms, || { let _ = rd32(regs, CSTS); }, cond),
            None => wait(regs, ms, cond),
        };
        let q = if io { &mut self.io } else { &mut self.admin };
        let dw3 = unsafe { read_volatile(entry.add(3)) };
        if !done || (dw3 >> 16 & 1 != 0) != phase {
            crate::println!("nvme: command {:#x} timed out", cmd[0] & 0xff);
            self.failed = true;
            return Err(err::NOT_READY);
        }
        fence(Ordering::SeqCst);
        let dw0 = unsafe { read_volatile(entry) };
        q.head = (q.head + 1) % QUEUE_DEPTH;
        if q.head == 0 {
            q.phase = !q.phase;
        }
        wr32(regs, cq_db, q.head as u32);
        let status = (dw3 >> 17) & 0x7fff;
        if (dw3 & 0xffff) as u16 != cid || status != 0 {
            return Err(err::NOT_READY);
        }
        Ok(dw0)
    }

    /// IDENTIFY into the identify page.
    fn identify(&mut self, cns: u32, nsid: u32) -> Result<&[u8], u32> {
        self.submit(false, command(ADM_IDENTIFY, nsid, self.ident, 0, [cns, 0, 0, 0, 0, 0]))?;
        Ok(unsafe { core::slice::from_raw_parts(phys::phys_to_virt(self.ident), PAGE) })
    }

    /// Create the I/O completion and submission queues (queue 1).
    fn create_io_queues(&mut self) -> Result<(), u32> {
        let size = (QUEUE_DEPTH as u32 - 1) << 16;
        let qid = self.io.qid as u32;
        // Physically contiguous; interrupts on vector 0 when there is one.
        let ien = if self.event.is_some() { 2 } else { 0 };
        let cq = command(ADM_CREATE_CQ, 0, self.io.cq, 0, [size | qid, ien | 1, 0, 0, 0, 0]);
        self.submit(false, cq)?;
        // Contiguous, bound to completion queue `qid`.
        let sq = command(ADM_CREATE_SQ, 0, self.io.sq, 0, [size | qid, qid << 16 | 1, 0, 0, 0, 0]);
        self.submit(false, sq)?;
        Ok(())
    }

    /// PRP entries for `bytes` of the bounce buffer.
    fn prps(&self, bytes: usize) -> (u64, u64) {
        let pages = bytes.div_ceil(PAGE);
        match pages {
            0 | 1 => (self.buf, 0),
            2 => (self.buf, self.buf + PAGE as u64),
            _ => {
                let list = phys::phys_to_virt(self.prp_list) as *mut u64;
                for i in 1..pages {
                    unsafe { write_volatile(list.add(i - 1), self.buf + (i * PAGE) as u64) };
                }
                (self.buf, self.prp_list)
            }
        }
    }

    /// READ or WRITE `n` blocks at `lba` through the bounce buffer.
    fn rw(&mut self, nsid: u32, write: bool, lba: u64, n: usize, bytes: usize) -> Result<(), u32> {
        let (prp1, prp2) = self.prps(bytes);
        let op = if write { NVM_WRITE } else { NVM_READ };
        let cdw = [lba as u32, (lba >> 32) as u32, (n as u32 - 1) & 0xffff, 0, 0, 0];
        self.submit(true, command(op, nsid, prp1, prp2, cdw)).map(|_| ())
    }

    fn bounce(&self) -> *mut u8 {
        phys::phys_to_virt(self.buf)
    }
}

impl Namespace {
    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let ss = self.sector_size;
        if buf.len() % ss != 0 || lba + (buf.len() / ss) as u64 > self.sectors {
            return Err(err::INVALID_PARAMETER);
        }
        let mut c = self.ctrl.lock();
        let per = (c.max_xfer / ss).max(1);
        let total = buf.len() / ss;
        let mut done = 0usize;
        while done < total {
            let n = (total - done).min(per);
            let bytes = n * ss;
            let chunk = &mut buf[done * ss..done * ss + bytes];
            if write {
                unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), c.bounce(), bytes) };
            }
            c.rw(self.nsid, write, lba + done as u64, n, bytes)?;
            if !write {
                unsafe { core::ptr::copy_nonoverlapping(c.bounce(), chunk.as_mut_ptr(), bytes) };
            }
            done += n;
        }
        Ok(())
    }
}

impl BlockDev for Namespace {
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
}

/// Disable the controller, program the admin queues and enable it again.
fn reset(c: &Ctrl) -> bool {
    let regs = c.regs;
    if rd32(regs, CC) & CC_EN != 0 {
        wr32(regs, CC, rd32(regs, CC) & !CC_EN);
    }
    if !wait(regs, c.ready_ms, || rd32(regs, CSTS) & CSTS_RDY == 0) {
        crate::println!("nvme: controller does not stop");
        return false;
    }
    let n = QUEUE_DEPTH as u32 - 1;
    wr32(regs, AQA, n << 16 | n);
    wr64(regs, ASQ, c.admin.sq);
    wr64(regs, ACQ, c.admin.cq);
    // NVM command set, 4 KB pages, round robin, 64-byte SQ and 16-byte
    // CQ entries.
    wr32(regs, CC, 6 << 16 | 4 << 20 | CC_EN);
    let ok = wait(regs, c.ready_ms, || rd32(regs, CSTS) & (CSTS_RDY | CSTS_CFS) != 0);
    if !ok || rd32(regs, CSTS) & CSTS_CFS != 0 {
        crate::println!("nvme: controller does not become ready (CSTS {:#x})", rd32(regs, CSTS));
        return false;
    }
    // INTMS/INTMC are for INTx and MSI only.
    if !c.msix {
        wr32(regs, INTMS, 0xffff_ffff);
        if c.event.is_some() {
            wr32(regs, INTMC, 1);
        }
    }
    true
}

/// Namespace IDs to probe: the active list, or 1..=NN on old controllers.
fn namespace_ids(c: &mut Ctrl, nn: u32) -> Vec<u32> {
    let version = rd32(c.regs, VS);
    if version >= 0x0001_0100 {
        if let Ok(list) = c.identify(CNS_ACTIVE_NS, 0) {
            return list
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .take_while(|&id| id != 0)
                .collect();
        }
    }
    (1..=nn.min(MAX_NAMESPACES)).collect()
}

/// Sector size and count of a namespace, if it is usable (active, and a
/// block format without interleaved metadata).
fn namespace_geometry(c: &mut Ctrl, nsid: u32) -> Option<(usize, u64)> {
    let id = c.identify(CNS_NAMESPACE, nsid).ok()?;
    let nsze = u64::from_le_bytes(id[0..8].try_into().unwrap());
    let nlbaf = id[25] as usize;
    let flbas = id[26] as usize;
    let fmt = (flbas & 0xf) | (flbas >> 5 & 3) << 4;
    if nsze == 0 || fmt > nlbaf || fmt >= 64 {
        return None;
    }
    let o = 128 + fmt * 4;
    let lbaf = u32::from_le_bytes(id[o..o + 4].try_into().unwrap());
    let ms = lbaf & 0xffff;
    let lbads = (lbaf >> 16) & 0xff;
    if ms != 0 || !(9..=12).contains(&lbads) {
        crate::println!("nvme: namespace {nsid}: unsupported LBA format (data 2^{lbads}, metadata {ms})");
        return None;
    }
    Some((1 << lbads, nsze))
}

/// Bring up an NVMe controller and return its namespaces.
pub fn probe(dev: &pci::Device) -> Vec<Arc<Namespace>> {
    let mut out = Vec::new();
    dev.enable();
    // Legacy INTx stays masked (MSI-X/MSI or polling).
    dev.set_intx_disabled(true);
    let bar = dev.bar(0);
    if bar == 0 {
        return out;
    }
    let cap = {
        let r = mmio::map(bar, PAGE as u64);
        rd64(r, CAP)
    };
    let stride = 4usize << ((cap >> 32) & 0xf);
    let ready_ms = ((cap >> 24) & 0xff).max(1) * 500;
    let mpsmin = (cap >> 48) & 0xf;
    let mqes = (cap & 0xffff) as u16;
    if mpsmin != 0 || cap >> 37 & 1 == 0 || mqes + 1 < QUEUE_DEPTH {
        crate::println!("nvme: unsupported controller (CAP {cap:#x})");
        return out;
    }
    // Registers plus doorbells for queues 0 and 1.
    let regs = mmio::map(bar, (DOORBELLS + 4 * stride) as u64);
    // Admin SQ, admin CQ, I/O SQ, I/O CQ, identify, PRP list, bounce.
    let pages = 6 + BOUNCE / PAGE;
    let Some(mem) = phys::alloc_contig(pages as u64, u64::MAX) else { return out };
    let page = |i: usize| mem + (i * PAGE) as u64;
    let queue = |qid, sq, cq| Queue { qid, sq, cq, tail: 0, head: 0, phase: true };
    let mut c = Ctrl {
        regs,
        stride,
        ready_ms,
        admin: queue(0, page(0), page(1)),
        io: queue(1, page(2), page(3)),
        next_cid: 0,
        ident: page(4),
        prp_list: page(5),
        buf: page(6),
        max_xfer: BOUNCE,
        failed: false,
        event: None,
        msix: false,
    };
    // One vector for the admin and I/O completion queues.
    let event = Event::leak();
    if let Some((v, kind)) = msi::setup(dev, true, Event::handler, event as *const Event as usize) {
        event.arm();
        c.event = Some(event);
        c.msix = kind == "MSI-X";
        crate::println!("nvme {}: {kind} vector {v:#x}", dev.bdf());
    }
    if !reset(&c) {
        return out;
    }
    let Ok(id) = c.identify(CNS_CONTROLLER, 0) else {
        crate::println!("nvme: IDENTIFY controller failed");
        return out;
    };
    let model = ascii(&id[24..64]);
    let mdts = id[77];
    let nn = u32::from_le_bytes(id[516..520].try_into().unwrap());
    if mdts != 0 {
        c.max_xfer = BOUNCE.min(PAGE << mdts.min(16));
    }
    if c.create_io_queues().is_err() {
        crate::println!("nvme: {model}: cannot create I/O queues");
        return out;
    }
    let mut found = Vec::new();
    for nsid in namespace_ids(&mut c, nn) {
        if let Some((ss, n)) = namespace_geometry(&mut c, nsid) {
            found.push((nsid, ss, n));
        }
    }
    let ctrl = Arc::new(Mutex::new(c));
    for (nsid, sector_size, sectors) in found {
        crate::println!("nvme: namespace {nsid}: {model}, {} MB", sectors * sector_size as u64 >> 20);
        out.push(Arc::new(Namespace { nsid, model: model.clone(), sector_size, sectors, ctrl: ctrl.clone() }));
    }
    out
}
