//! VirtualBox's VMMDev (PCI 80EE:CAFE): the guest reports itself and
//! calls host services through HGCM; here the shared clipboard service
//! (`VBoxSharedClipboard`, its legacy protocol, the one VirtualBox's own
//! OS/2 additions use) for the guest agent, VMAGENT (docs/AGENT.md).
//!
//! A request is a structure in physically contiguous memory below 4 GB
//! whose address is written to the device's I/O port; the host handles it
//! during the OUT. HGCM requests are then executed asynchronously: the host
//! sets the DONE flag in the request when it has written the results back.
//! The driver polls that flag (the device's interrupt is turned off with
//! the PCI command register's INTx disable, so the HGCM interrupt the host
//! enables by itself cannot storm).
//!
//! The kernel owns the one clipboard client (a VirtualBox host allows one
//! per machine) and keeps it for good, and its waiting request
//! (MSG_OLD_GET_WAIT) outlives the thread that waits for it: a request the
//! host still holds is never freed, and an agent that is restarted carries
//! on with the same client and wait.
//!
//! `\DEV\VBOXGST$`, IOCtl category 0x80, parameters
//! `{u32 format (in), i32 rc, u32 a, u32 b (out)}` (rc: VirtualBox status):
//!
//! - 1 wait for a host message: a = message (1 quit, 2 read data,
//!   3 formats report, 4 cancelled), b = formats;
//! - 2 report the guest's formats (the format word);
//! - 3 read the host's data in a format into the data buffer: a = the
//!   size of the host's data, b = bytes given (0 if the buffer was too
//!   small: retry with a larger one);
//! - 4 write the guest's data (the data buffer) in reply to a read request.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};

use super::pci;
use crate::drivers::virtio::queue::Dma;
use crate::sync::Mutex;

const REQUEST_VERSION: u32 = 0x0001_0001;
const VMMDEV_VERSION: u32 = 0x0001_0004;
const OSTYPE_OS2_WARP45: u32 = 0x43000;

const REQ_GET_HOST_VERSION: u32 = 4;
const REQ_REPORT_GUEST_INFO: u32 = 50;
const REQ_REPORT_GUEST_STATUS: u32 = 59;
const REQ_HGCM_CONNECT: u32 = 60;
const REQ_HGCM_CALL64: u32 = 63;

const VINF_HGCM_ASYNC_EXECUTE: i32 = 2903;
const SENTINEL: i32 = -225; // VERR_INTERNAL_ERROR: the host did not look
const VERR_NOT_SUPPORTED: i32 = -37;

const PARM_32BIT: u32 = 1;
const PARM_EMBEDDED: u32 = 11;
const DIR_TO_HOST: u32 = 1;
const DIR_FROM_HOST: u32 = 2;

const FN_MSG_OLD_GET_WAIT: u32 = 1;
const FN_REPORT_FORMATS: u32 = 2;
const FN_DATA_READ: u32 = 3;
const FN_DATA_WRITE: u32 = 4;

/// The request and its embedded data stay under this.
const MAX_REQUEST: usize = 1024 * 1024;

static IO: AtomicU16 = AtomicU16::new(0);
static CLIENT: Mutex<u32> = Mutex::new(0);
/// The waiting request (MSG_OLD_GET_WAIT), kept for good once made.
static WAIT: Mutex<Option<Dma>> = Mutex::new(None);
/// Its physical address while the host holds it (0: not submitted).
static WAIT_PENDING: AtomicU64 = AtomicU64::new(0);
static HOST_FEATURES: AtomicU32 = AtomicU32::new(0);

pub fn is_vmmdev(d: &pci::Device) -> bool {
    (d.vendor, d.device) == (0x80ee, 0xcafe)
}

fn header(r: &mut Vec<u8>, size: usize, ty: u32) {
    r.clear();
    r.extend_from_slice(&(size as u32).to_le_bytes());
    r.extend_from_slice(&REQUEST_VERSION.to_le_bytes());
    r.extend_from_slice(&ty.to_le_bytes());
    r.extend_from_slice(&SENTINEL.to_le_bytes());
    r.extend_from_slice(&[0; 8]);
    r.resize(size, 0);
}

fn rd32(m: &Dma, off: usize) -> u32 {
    unsafe { core::ptr::read_volatile(m.ptr(off) as *const u32) }
}

/// Hand a request to the host (synchronous part); its status.
fn submit(m: &Dma) -> i32 {
    let io = IO.load(Ordering::Relaxed);
    core::sync::atomic::fence(Ordering::SeqCst);
    unsafe { crate::arch::outl(io, m.base as u32) };
    core::sync::atomic::fence(Ordering::SeqCst);
    rd32(m, 12) as i32
}

/// A plain request: status and the request as the host left it.
fn request(body: &[u8]) -> Option<(i32, Dma)> {
    let m = Dma::new(body.len())?;
    m.write(0, body);
    let rc = submit(&m);
    Some((rc, m))
}

fn done(m: &Dma) -> bool {
    rd32(m, 24) & 1 != 0
}

/// Wait for an HGCM request the host accepted; false if this thread is
/// to end first (the request is then still the host's).
fn wait_done(m: &Dma) -> bool {
    let t = crate::task::sched::current();
    let mut ms = 1;
    while !done(m) {
        if t.exit_requested() {
            return false;
        }
        crate::task::sched::sleep_ms(ms);
        ms = (ms * 2).min(20);
    }
    true
}

pub fn probe(d: &pci::Device) {
    let bar = d.bar(0);
    if d.read(0x10) & 1 == 0 || bar == 0 {
        return;
    }
    d.enable();
    d.set_intx_disabled(true);
    IO.store(bar as u16, Ordering::Relaxed);
    let mut r = Vec::new();
    header(&mut r, 40, REQ_GET_HOST_VERSION);
    if let Some((0, m)) = request(&r) {
        let v = |o| rd32(&m, o);
        HOST_FEATURES.store(v(36), Ordering::Relaxed);
        crate::println!("vbox: VMMDev {}, host {}.{}.{} (features {:#x})", d.bdf(), v(24) & 0xffff, v(24) >> 16, v(28), v(36));
    }
    header(&mut r, 32, REQ_REPORT_GUEST_INFO);
    r[24..28].copy_from_slice(&VMMDEV_VERSION.to_le_bytes());
    r[28..32].copy_from_slice(&OSTYPE_OS2_WARP45.to_le_bytes());
    match request(&r) {
        Some((0, _)) => {}
        other => {
            crate::println!("vbox: guest info refused ({:?})", other.map(|o| o.0));
            IO.store(0, Ordering::Relaxed);
            return;
        }
    }
    // The guest driver is active (facility 20, status 50).
    header(&mut r, 36, REQ_REPORT_GUEST_STATUS);
    r[24..28].copy_from_slice(&20u32.to_le_bytes());
    r[28..32].copy_from_slice(&50u32.to_le_bytes());
    let _ = request(&r);
}

pub fn present() -> bool {
    IO.load(Ordering::Relaxed) != 0
}

/// The clipboard client, connected on first use.
fn client() -> Result<u32, i32> {
    let mut c = CLIENT.lock();
    if *c != 0 {
        return Ok(*c);
    }
    let mut r = Vec::new();
    header(&mut r, 168, REQ_HGCM_CONNECT);
    r[32..36].copy_from_slice(&2u32.to_le_bytes());
    let name = b"VBoxSharedClipboard";
    r[36..36 + name.len()].copy_from_slice(name);
    let m = Dma::new(168).ok_or(VERR_NOT_SUPPORTED)?;
    m.write(0, &r);
    let rc = submit(&m);
    if rc != VINF_HGCM_ASYNC_EXECUTE {
        return Err(if rc == 0 { VERR_NOT_SUPPORTED } else { rc });
    }
    if !wait_done(&m) {
        // Keep it: the host still writes to it.
        core::mem::forget(m);
        return Err(-39);
    }
    let result = rd32(&m, 28) as i32;
    if result < 0 {
        return Err(result);
    }
    *c = rd32(&m, 164);
    crate::println!("vbox: shared clipboard client {}", *c);
    Ok(*c)
}

/// A Call64 request: header, parameters (32-bit values, or one embedded
/// buffer of `buf` bytes after them, `dir`), and the buffer's data.
fn call_request(client: u32, func: u32, values: &[Option<u32>], buf: usize, dir: u32, data: &[u8]) -> Vec<u8> {
    let extra = 44 + 16 * values.len();
    let size = extra + buf;
    let mut r = Vec::new();
    header(&mut r, size, REQ_HGCM_CALL64);
    r[32..36].copy_from_slice(&client.to_le_bytes());
    r[36..40].copy_from_slice(&func.to_le_bytes());
    r[40..44].copy_from_slice(&(values.len() as u32).to_le_bytes());
    for (i, v) in values.iter().enumerate() {
        let p = 44 + 16 * i;
        match v {
            Some(v) => {
                r[p..p + 4].copy_from_slice(&PARM_32BIT.to_le_bytes());
                r[p + 4..p + 8].copy_from_slice(&v.to_le_bytes());
            }
            None => {
                r[p..p + 4].copy_from_slice(&PARM_EMBEDDED.to_le_bytes());
                r[p + 4..p + 8].copy_from_slice(&(dir | (extra as u32) << 8).to_le_bytes());
                r[p + 8..p + 12].copy_from_slice(&(buf as u32).to_le_bytes());
            }
        }
    }
    r[extra..extra + data.len()].copy_from_slice(data);
    r
}

/// Make a call and wait for it: (service status, the request).
fn call(func: u32, values: &[Option<u32>], buf: usize, dir: u32, data: &[u8]) -> Result<(i32, Dma), i32> {
    let client = client()?;
    let r = call_request(client, func, values, buf, dir, data);
    let m = Dma::new(r.len()).ok_or(VERR_NOT_SUPPORTED)?;
    m.write(0, &r);
    let rc = submit(&m);
    if rc != VINF_HGCM_ASYNC_EXECUTE {
        return Err(if rc == 0 { VERR_NOT_SUPPORTED } else { rc });
    }
    if !wait_done(&m) {
        core::mem::forget(m);
        return Err(-39);
    }
    Ok((rd32(&m, 28) as i32, m))
}

/// Wait for the host's next clipboard message: (status, message, formats).
fn wait_message() -> Result<(i32, u32, u32), i32> {
    let client = client()?;
    let base = {
        let mut w = WAIT.lock();
        if w.is_none() {
            *w = Some(Dma::new(76).ok_or(VERR_NOT_SUPPORTED)?);
        }
        let m = w.as_ref().unwrap();
        if WAIT_PENDING.load(Ordering::Relaxed) == 0 {
            m.write(0, &call_request(client, FN_MSG_OLD_GET_WAIT, &[Some(0), Some(0)], 0, 0, &[]));
            let rc = submit(m);
            if rc != VINF_HGCM_ASYNC_EXECUTE {
                return Err(if rc == 0 { VERR_NOT_SUPPORTED } else { rc });
            }
            WAIT_PENDING.store(m.base, Ordering::Relaxed);
        }
        m.base
    };
    let t = crate::task::sched::current();
    let flags = crate::mm::phys::phys_to_virt(base + 24) as *const u32;
    let mut ms = 1;
    while unsafe { core::ptr::read_volatile(flags) } & 1 == 0 {
        if t.exit_requested() {
            return Err(-39);
        }
        crate::task::sched::sleep_ms(ms);
        ms = (ms * 2).min(20);
    }
    let w = WAIT.lock();
    let m = w.as_ref().unwrap();
    if WAIT_PENDING.swap(0, Ordering::Relaxed) == 0 {
        // Another thread took this message.
        return Ok((0, 4, 0));
    }
    Ok((rd32(m, 28) as i32, rd32(m, 0x30), rd32(m, 0x40)))
}

/// What an IOCtl returns: status and two values, and data.
pub struct Reply {
    pub rc: i32,
    pub a: u32,
    pub b: u32,
    pub data: Vec<u8>,
}

impl Reply {
    fn err(rc: i32) -> Reply {
        Reply { rc, a: 0, b: 0, data: Vec::new() }
    }
}

/// IOCtl function `f` with the format word and the caller's data buffer
/// (its contents for writes, its size for reads).
pub fn clipboard(f: u32, format: u32, data: &[u8], cap: usize) -> Reply {
    if !present() {
        return Reply::err(VERR_NOT_SUPPORTED);
    }
    match f {
        1 => match wait_message() {
            Ok((rc, msg, fmts)) => Reply { rc, a: msg, b: fmts, data: Vec::new() },
            Err(rc) => Reply::err(rc),
        },
        2 => match call(FN_REPORT_FORMATS, &[Some(format)], 0, 0, &[]) {
            Ok((rc, _)) => Reply::err(rc),
            Err(rc) => Reply::err(rc),
        },
        3 => {
            let n = cap.min(MAX_REQUEST - 92 - 4096);
            match call(FN_DATA_READ, &[Some(format), None, Some(0)], n, DIR_FROM_HOST, &[]) {
                Ok((rc, m)) => {
                    let need = rd32(&m, 0x50) as usize;
                    // On 7.x a buffer exactly the size of the data is
                    // reported as too small.
                    let data = if rc >= 0 && need < n { m.bytes(92, need).to_vec() } else { Vec::new() };
                    let b = data.len() as u32;
                    Reply { rc, a: need as u32, b, data }
                }
                Err(rc) => Reply::err(rc),
            }
        }
        4 => {
            let data = &data[..data.len().min(MAX_REQUEST - 76 - 4096)];
            match call(FN_DATA_WRITE, &[Some(format), None], data.len(), DIR_TO_HOST, data) {
                Ok((rc, _)) => Reply::err(rc),
                Err(rc) => Reply::err(rc),
            }
        }
        _ => Reply::err(-2),
    }
}
