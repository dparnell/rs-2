//! LSI Logic Fusion-MPT SCSI/SAS host adapters: the 53c1030 ("LSI Logic
//! Parallel" in VMware, also in VirtualBox) and the SAS1068 ("LSI Logic
//! SAS" in VMware and VirtualBox, `mptsas1068` in QEMU).
//!
//! MPT controllers talk in message frames (Message Passing Interface
//! 1.x). Set-up messages (IOC_FACTS, IOC_INIT, PORT_ENABLE) go through
//! the doorbell register handshake, one dword at a time, and their replies
//! come back through it 16 bits at a time. SCSI I/O requests are frames
//! in host memory whose address is written to the request FIFO; the
//! reply FIFO returns either the request's context (success) or the
//! address of a reply frame from the pool the host posted to the reply
//! free FIFO (errors, short transfers).
//!
//! As with PVSCSI, one request is in flight at a time through a bounce
//! buffer. With MSI-X or MSI the reply interrupt is unmasked once the
//! IOC is operational and wakes the requester, which then reads the
//! reply FIFO (which also clears the interrupt); without either the FIFO
//! is polled. Units are handled by `scsi`.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use super::pci;
use super::scsi::{self, Dir, Error, Hba};
use crate::fs::block::BlockDev;
use crate::mm::{mmio, phys};
use crate::msi::{self, Event};
use crate::sync::Mutex;

const PAGE: usize = 4096;
const BOUNCE: usize = 64 * 1024;
const TIMEOUT_MS: u64 = 10_000;
/// Targets scanned on bus 0 (a wide SCSI bus, or the first SAS phys).
const MAX_TARGETS: u8 = 16;

// System interface registers.
const DOORBELL: usize = 0x00;
const HOST_INT_STATUS: usize = 0x30;
const HOST_INT_MASK: usize = 0x34;
const REQUEST_FIFO: usize = 0x40;
const REPLY_FIFO: usize = 0x44;

// Doorbell.
const STATE_MASK: u32 = 0xf000_0000;
const STATE_READY: u32 = 0x1000_0000;
const STATE_OPERATIONAL: u32 = 0x2000_0000;
const STATE_FAULT: u32 = 0x4000_0000;
const DOORBELL_ACTIVE: u32 = 0x0800_0000;
// Host interrupt status / mask.
const HIS_DOORBELL: u32 = 0x0000_0001;
const HIS_IOP_DOORBELL: u32 = 0x8000_0000;
const HIM_ALL: u32 = 0x0000_0009;
/// Doorbell interrupt masked, reply interrupt enabled.
const HIM_DOORBELL: u32 = 0x0000_0001;

// Functions.
const FN_SCSI_IO: u8 = 0x00;
const FN_IOC_INIT: u8 = 0x02;
const FN_IOC_FACTS: u8 = 0x03;
const FN_PORT_ENABLE: u8 = 0x06;
const FN_IOC_MESSAGE_UNIT_RESET: u8 = 0x40;
const FN_HANDSHAKE: u8 = 0x42;

const WHOINIT_HOST_DRIVER: u32 = 0x04;
const REPLY_FRAME_SIZE: usize = 128;
const REPLY_FRAMES: usize = 16;
const SENSE_LEN: usize = 32;

// SCSI I/O request.
const CONTROL_WRITE: u32 = 0x0100_0000;
const CONTROL_READ: u32 = 0x0200_0000;
const SGE_SIMPLE_LAST: u32 = 0x8000_0000 | 0x4000_0000 | 0x1000_0000 | 0x0100_0000;
const SGE_HOST_TO_IOC: u32 = 0x0400_0000;
const ADDRESS_REPLY: u32 = 0x8000_0000;
const CONTEXT_MASK: u32 = 0x1fff_ffff;
const SCSI_STATE_AUTOSENSE_VALID: u8 = 0x01;
// IOC status.
const IOC_SUCCESS: u16 = 0x0000;
const IOC_SCSI_RECOVERED_ERROR: u16 = 0x0040;
const IOC_SCSI_DATA_UNDERRUN: u16 = 0x0045;

/// Adapter state behind the lock.
pub struct State {
    regs: *mut u8,
    /// Request frame, reply frames, sense buffer, bounce buffer (all
    /// below 4 GB: MPT addresses them with 32 bits plus a common high
    /// part, which we leave at 0).
    req: u64,
    replies: u64,
    sense: u64,
    buf: u64,
    context: u32,
    failed: bool,
    /// Signalled by the reply interrupt, if there is one.
    event: Option<&'static Event>,
}

unsafe impl Send for State {}

impl State {
    fn rd(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.regs.add(off) as *const u32) }
    }

    fn wr(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.regs.add(off) as *mut u32, v) }
    }

    fn state(&self) -> u32 {
        self.rd(DOORBELL) & STATE_MASK
    }

    fn wait_doorbell_int(&self) -> bool {
        scsi::wait(5000, || self.rd(HOST_INT_STATUS) & HIS_DOORBELL != 0)
    }

    fn wait_doorbell_ack(&self) -> bool {
        scsi::wait(5000, || self.rd(HOST_INT_STATUS) & HIS_IOP_DOORBELL == 0)
    }

    /// A handshake: send `req` through the doorbell, read the reply (up
    /// to `reply.len()` 16-bit words kept). Returns the reply length in
    /// bytes.
    fn handshake(&self, req: &[u32], reply: &mut [u16]) -> Option<usize> {
        if self.rd(DOORBELL) & DOORBELL_ACTIVE != 0 {
            return None;
        }
        self.wr(HOST_INT_STATUS, 0);
        self.wr(DOORBELL, (FN_HANDSHAKE as u32) << 24 | (req.len() as u32) << 16);
        if !self.wait_doorbell_int() || self.rd(DOORBELL) & DOORBELL_ACTIVE == 0 {
            return None;
        }
        self.wr(HOST_INT_STATUS, 0);
        if !self.wait_doorbell_ack() {
            return None;
        }
        for &w in req {
            self.wr(DOORBELL, w);
            if !self.wait_doorbell_ack() {
                return None;
            }
        }
        // The reply, a word per doorbell interrupt; its length (in
        // dwords) is in byte 2.
        let mut n = 0;
        let mut total = 2;
        while n < total {
            if !self.wait_doorbell_int() {
                return None;
            }
            let w = self.rd(DOORBELL) as u16;
            self.wr(HOST_INT_STATUS, 0);
            if n < reply.len() {
                reply[n] = w;
            }
            if n == 1 {
                total = 2 * (w & 0xff) as usize;
            }
            n += 1;
        }
        self.wait_doorbell_int();
        self.wr(HOST_INT_STATUS, 0);
        Some(n * 2)
    }

    /// Bring the IOC to the READY state (resetting the message unit if
    /// the firmware left it operational).
    fn make_ready(&self) -> bool {
        match self.state() {
            STATE_READY => return true,
            STATE_OPERATIONAL | STATE_FAULT => {
                self.wr(DOORBELL, (FN_IOC_MESSAGE_UNIT_RESET as u32) << 24);
                self.wait_doorbell_ack();
            }
            _ => {}
        }
        scsi::wait(15_000, || self.state() == STATE_READY)
    }

    fn next_context(&mut self) -> u32 {
        self.context = (self.context + 1) & CONTEXT_MASK;
        if self.context == 0 {
            self.context = 1;
        }
        self.context
    }
}

/// Little-endian bytes of handshake reply words.
fn reply_bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Hba for State {
    fn name(&self) -> &'static str {
        "mpt"
    }

    fn bounce(&self) -> *mut u8 {
        phys::phys_to_virt(self.buf)
    }

    fn bounce_size(&self) -> usize {
        BOUNCE
    }

    fn exec(&mut self, target: u8, lun: u8, cdb: &[u8], dir: Dir) -> Result<usize, Error> {
        if self.failed {
            return Err(Error::Timeout);
        }
        let (len, control, sge_dir) = match dir {
            Dir::None => (0, 0, 0),
            Dir::In(n) => (n, CONTROL_READ, 0),
            Dir::Out(n) => (n, CONTROL_WRITE, SGE_HOST_TO_IOC),
        };
        let ctx = self.next_context();
        unsafe {
            core::ptr::write_bytes(phys::phys_to_virt(self.sense), 0, SENSE_LEN);
            let f = phys::phys_to_virt(self.req);
            core::ptr::write_bytes(f, 0, 64);
            *f = target;
            *f.add(1) = 0; // bus
            *f.add(3) = FN_SCSI_IO;
            *f.add(4) = cdb.len() as u8;
            *f.add(5) = SENSE_LEN as u8;
            (f.add(8) as *mut u32).write_volatile(ctx);
            *f.add(0x0c + 1) = lun;
            (f.add(0x14) as *mut u32).write_volatile(control);
            core::ptr::copy_nonoverlapping(cdb.as_ptr(), f.add(0x18), cdb.len().min(16));
            (f.add(0x28) as *mut u32).write_volatile(len as u32);
            (f.add(0x2c) as *mut u32).write_volatile(self.sense as u32);
            // One simple 32-bit SGE (empty without data).
            (f.add(0x30) as *mut u32).write_volatile(SGE_SIMPLE_LAST | sge_dir | len as u32);
            (f.add(0x34) as *mut u32).write_volatile(if len > 0 { self.buf as u32 } else { 0 });
        }
        fence(Ordering::SeqCst);
        self.wr(REQUEST_FIFO, self.req as u32);
        let mut v = u32::MAX;
        let regs = self.regs;
        // Reading the FIFO pops it: keep the first reply.
        let reply = || {
            if v == u32::MAX {
                v = unsafe { read_volatile(regs.add(REPLY_FIFO) as *const u32) };
            }
            v != u32::MAX
        };
        let ok = match self.event {
            Some(e) => e.wait(TIMEOUT_MS, || {}, reply),
            None => scsi::wait(TIMEOUT_MS, reply),
        };
        if !ok {
            crate::println!("mpt: command {:#x} to target {target} timed out", cdb[0]);
            self.failed = true;
            return Err(Error::Timeout);
        }
        if v & ADDRESS_REPLY == 0 {
            // Context reply: complete success.
            if v & CONTEXT_MASK != ctx {
                crate::println!("mpt: reply for request {:#x}, expected {ctx:#x}", v & CONTEXT_MASK);
                self.failed = true;
                return Err(Error::Timeout);
            }
            return Ok(len);
        }
        let addr = (v << 1) as u64;
        let mut r = [0u8; 36];
        unsafe { core::ptr::copy_nonoverlapping(phys::phys_to_virt(addr), r.as_mut_ptr(), r.len()) };
        // Give the frame back to the IOC.
        self.wr(REPLY_FIFO, addr as u32);
        if u32_at(&r, 8) != ctx {
            crate::println!("mpt: reply frame for request {:#x}, expected {ctx:#x}", u32_at(&r, 8));
            self.failed = true;
            return Err(Error::Timeout);
        }
        let (status, state, ioc) = (r[0x0c], r[0x0d], u16_at(&r, 0x0e) & 0x7fff);
        let got = (u32_at(&r, 0x14) as usize).min(len);
        match ioc {
            IOC_SUCCESS | IOC_SCSI_RECOVERED_ERROR | IOC_SCSI_DATA_UNDERRUN => {}
            e => return Err(Error::Host(e as u32)),
        }
        match status {
            scsi::STATUS_GOOD => Ok(got),
            scsi::STATUS_CHECK_CONDITION if state & SCSI_STATE_AUTOSENSE_VALID != 0 => {
                let s = unsafe { core::slice::from_raw_parts(phys::phys_to_virt(self.sense), 18) };
                Err(scsi::parse_sense(s).unwrap_or(Error::Check))
            }
            scsi::STATUS_CHECK_CONDITION => Err(Error::Check),
            s => Err(Error::Status(s)),
        }
    }
}

/// Bring up an MPT adapter and return its units, each with whether it
/// is an optical drive.
pub fn probe(dev: &pci::Device) -> Vec<(Arc<dyn BlockDev>, bool)> {
    let mut out: Vec<(Arc<dyn BlockDev>, bool)> = Vec::new();
    dev.enable();
    dev.set_intx_disabled(true);
    let Some(bar) = super::pvscsi::mem_bar(dev) else {
        crate::println!("mpt: no register window");
        return out;
    };
    let regs = mmio::map(bar, PAGE as u64);
    // Request frame + sense (page 0), reply frames (page 1), bounce.
    let pages = 2 + BOUNCE / PAGE;
    let Some(mem) = phys::alloc_contig(pages as u64, 1 << 32) else { return out };
    let st = State { regs, req: mem, replies: mem + PAGE as u64, sense: mem + 2048, buf: mem + 2 * PAGE as u64, context: 0, failed: false, event: None };
    st.wr(HOST_INT_MASK, HIM_ALL);
    if !st.make_ready() {
        crate::println!("mpt: adapter not ready (doorbell {:#x})", st.rd(DOORBELL));
        return out;
    }
    // IOC_FACTS.
    let mut reply = [0u16; 48];
    let Some(n) = st.handshake(&[(FN_IOC_FACTS as u32) << 24, 0, 1], &mut reply) else {
        crate::println!("mpt: IOC_FACTS failed");
        return out;
    };
    let facts = reply_bytes(&reply);
    if n < 0x38 {
        crate::println!("mpt: short IOC_FACTS reply");
        return out;
    }
    let (msg_version, header_version) = (u16_at(&facts, 0), u16_at(&facts, 4));
    let (max_devices, max_buses) = (facts[0x2e], facts[0x2f]);
    let ports = facts[0x26];
    // IOC_INIT: host driver, our reply frame size, all high address
    // parts 0.
    let init = [
        WHOINIT_HOST_DRIVER | (FN_IOC_INIT as u32) << 24,
        (max_devices as u32) << 8 | (max_buses as u32) << 16,
        2,
        REPLY_FRAME_SIZE as u32,
        0,
        0,
        0,
        0,
        0,
        0,
        msg_version as u32 | (header_version as u32) << 16,
    ];
    let ok = st.handshake(&init, &mut reply).is_some() && u16_at(&reply_bytes(&reply), 0x0e) & 0x7fff == 0;
    if !ok || !scsi::wait(5000, || st.state() == STATE_OPERATIONAL) {
        crate::println!("mpt: IOC_INIT failed (doorbell {:#x})", st.rd(DOORBELL));
        return out;
    }
    for i in 0..REPLY_FRAMES {
        st.wr(REPLY_FIFO, (st.replies + (i * REPLY_FRAME_SIZE) as u64) as u32);
    }
    let ok = st.handshake(&[(FN_PORT_ENABLE as u32) << 24, 0, 3], &mut reply).is_some()
        && u16_at(&reply_bytes(&reply), 0x0e) & 0x7fff == 0;
    if !ok {
        crate::println!("mpt: PORT_ENABLE failed");
        return out;
    }
    crate::trace!("mpt: MPI {msg_version:#x}, {ports} port(s), {max_devices} devices");
    // The handler only wakes the requester: reading the reply FIFO
    // acknowledges the interrupt.
    let mut st = st;
    let event = Event::leak();
    if let Some((v, kind)) = msi::setup(dev, true, Event::handler, event as *const Event as usize) {
        event.arm();
        st.event = Some(event);
        st.wr(HOST_INT_MASK, HIM_DOORBELL);
        crate::println!("mpt {}: {kind} vector {v:#x}", dev.bdf());
    }
    let hba = Arc::new(Mutex::new(st));
    for target in 0..MAX_TARGETS {
        if let Some((unit, optical)) = scsi::setup_unit(&hba, target, 0) {
            out.push((Arc::new(unit), optical));
        }
        if hba.lock().failed {
            break;
        }
    }
    crate::println!("mpt: {:02x}:{:02x}.{}: {} unit(s)", dev.bus, dev.dev, dev.func, out.len());
    out
}
