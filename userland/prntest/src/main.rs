//! RS2\PRNTEST.EXE: printing self-checks (docs/PRINTING.md), printing
//! PASS/FAIL and `prntest done: N failure(s)` (the exit code is N).
//!
//! It sets up a queue (PRNTESTQ) on a printer (PRNTEST) whose port is a
//! file port (FILE9, writing C:\PRNTEST.PS), then checks the spooler
//! calls (queues, printers, ports, jobs: hold, release, set, delete),
//! spools a raw job, prints pages through a PM_Q_STD queued DC (the
//! metafile is played by PMPRINT in the spooler), a PM_Q_RAW queued DC
//! and an OD_DIRECT DC, and checks the PostScript each produced: the DSC
//! structure, the pages, text shown with xshow, areas with patterns,
//! lines, images. The configuration is removed at the end (KEEP keeps
//! it).
//!
//! `PRNTEST LPR host port [queue]` also prints a page to an LPD at
//! host:port and `PRNTEST IPP host port [path]` to an IPP printer; the
//! listener checks what arrives.
//!
//! `PRNTEST SETPORT printer port settings [RUN program args]` only sets a
//! printer's port (and the port's settings), then runs the program: how
//! the tests point the default printer at a host listener before an IBM
//! program prints.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.605"]
    fn DevEscape(hdc: u32, code: i32, nin: i32, pin: *const u8, nout: *mut i32, pout: *mut u8) -> i32;
    #[link_name = "PMGPI.606"]
    fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32;
    #[link_name = "PMGPI.608"]
    fn DevQueryHardcopyCaps(hdc: u32, start: i32, n: i32, out: *mut u8) -> i32;
    #[link_name = "PMGPI.609"]
    fn DevPostDeviceModes(hab: u32, drv: *mut u8, name: *const u8, dev: *const u8, pname: *const u8, fl: u32) -> i32;
    #[link_name = "PMGPI.607"]
    fn DevQueryDeviceNames(hab: u32, drv: *const u8, n: *mut i32, names: *mut u8, desc: *mut u8, nt: *mut i32, types: *mut u8) -> u32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    fn GpiAssociate(hps: u32, hdc: u32) -> u32;
    #[link_name = "PMGPI.404"]
    fn GpiMove(hps: u32, p: *const i32) -> u32;
    #[link_name = "PMGPI.398"]
    fn GpiLine(hps: u32, p: *const i32) -> i32;
    #[link_name = "PMGPI.356"]
    fn GpiBox(hps: u32, ctl: i32, p: *const i32, h: i32, v: i32) -> i32;
    #[link_name = "PMGPI.415"]
    fn GpiPolyLine(hps: u32, n: i32, p: *const i32) -> i32;
    #[link_name = "PMGPI.359"]
    fn GpiCharStringAt(hps: u32, p: *const i32, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.517"]
    fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.541"]
    fn GpiSetPattern(hps: u32, p: i32) -> u32;
    #[link_name = "PMGPI.530"]
    fn GpiSetLineType(hps: u32, t: i32) -> u32;
    #[link_name = "PMGPI.352"]
    fn GpiBeginArea(hps: u32, fl: u32) -> u32;
    #[link_name = "PMGPI.385"]
    fn GpiEndArea(hps: u32) -> i32;
    #[link_name = "PMGPI.393"]
    fn GpiFullArc(hps: u32, ctl: i32, mult: i32) -> i32;
    #[link_name = "PMGPI.368"]
    fn GpiCreateLogFont(hps: u32, name: *const u8, lcid: i32, fat: *const u8) -> i32;
    #[link_name = "PMGPI.513"]
    fn GpiSetCharSet(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.510"]
    fn GpiSetCharBox(hps: u32, p: *const i32) -> u32;
    #[link_name = "PMGPI.598"]
    fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, bits: *const u8, info: *const u8) -> u32;
    #[link_name = "PMGPI.557"]
    fn GpiWCBitBlt(hps: u32, hbm: u32, n: i32, p: *const i32, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.371"]
    fn GpiDeleteBitmap(hbm: u32) -> u32;
    #[link_name = "PMGPI.471"]
    fn GpiQueryPS(hps: u32, size: *mut i32) -> u32;
    #[link_name = "PMGPI.401"]
    fn GpiLoadMetaFile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMGPI.411"]
    fn GpiPlayMetaFile(hps: u32, hmf: u32, n1: i32, opts: *const i32, segs: *mut i32, n2: i32, desc: *mut u8) -> i32;

    #[link_name = "PMSPL.399"]
    fn SplEnumQueue(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.397"]
    fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.393"]
    fn SplCreateQueue(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32;
    #[link_name = "PMSPL.398"]
    fn SplSetQueue(comp: *const u8, q: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    #[link_name = "PMSPL.396"]
    fn SplDeleteQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.394"]
    fn SplHoldQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.395"]
    fn SplReleaseQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.392"]
    fn SplEnumJob(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.390"]
    fn SplQueryJob(comp: *const u8, q: *const u8, id: u32, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.391"]
    fn SplSetJob(comp: *const u8, q: *const u8, id: u32, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    #[link_name = "PMSPL.386"]
    fn SplDeleteJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.385"]
    fn SplHoldJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.384"]
    fn SplReleaseJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.382"]
    fn SplEnumDevice(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.401"]
    fn SplCreateDevice(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32;
    #[link_name = "PMSPL.403"]
    fn SplDeleteDevice(comp: *const u8, dev: *const u8) -> u32;
    #[link_name = "PMSPL.381"]
    fn SplQueryDevice(comp: *const u8, dev: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.408"]
    fn SplEnumPort(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.413"]
    fn SplCreatePort(comp: *const u8, port: *const u8, drv: *const u8, ver: u32, buf: *const u8, cb: u32) -> u32;
    #[link_name = "PMSPL.414"]
    fn SplDeletePort(comp: *const u8, port: *const u8) -> u32;
    #[link_name = "PMSPL.441"]
    fn SplEnumPrinter(comp: *const u8, level: u32, ty: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.301"]
    fn SplQmOpen(tok: *const u8, n: i32, dop: *const u32) -> u32;
    #[link_name = "PMSPL.302"]
    fn SplQmStartDoc(h: u32, name: *const u8) -> u32;
    #[link_name = "PMSPL.304"]
    fn SplQmWrite(h: u32, n: i32, d: *const u8) -> u32;
    #[link_name = "PMSPL.303"]
    fn SplQmEndDoc(h: u32) -> u32;
    #[link_name = "PMSPL.306"]
    fn SplQmClose(h: u32) -> u32;

    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
}

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) -> bool {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
    ok
}

const Q: &[u8] = b"PRNTESTQ\0";
const PRN: &[u8] = b"PRNTEST\0";
const PORT: &[u8] = b"FILE9\0";
const OUT: &[u8] = b"C:\\PRNTEST.PS";

const OD_QUEUED: i32 = 2;
const OD_DIRECT: i32 = 5;
const OD_INFO: i32 = 6;
const DEVESC_STARTDOC: i32 = 8150;
const DEVESC_ENDDOC: i32 = 8151;
const DEVESC_NEWFRAME: i32 = 16300;
const DEVESC_RAWDATA: i32 = 16303;
const PU_LOENGLISH: u32 = 0x0014;
const PU_PELS: u32 = 0x0008;
const GPIA_ASSOC: u32 = 0x4000;
const NERR_JOBNOTFOUND: u32 = 2151;

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn cstr(p: u32) -> Vec<u8> {
    let mut v = Vec::new();
    if p == 0 {
        return v;
    }
    let mut q = p as *const u8;
    unsafe {
        while *q != 0 {
            v.push(*q);
            q = q.add(1);
        }
    }
    v
}

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = [path, b"\0"].concat();
    let (mut hf, mut act) = (0, 0);
    if unsafe { DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(hf) };
    Some(out)
}

fn delete(path: &[u8]) {
    let p = [path, b"\0"].concat();
    unsafe { DosDelete(p.as_ptr()) };
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Wait until job `id` has left the queue (printed), up to `secs`.
fn wait_printed(id: u32, secs: u32) -> bool {
    for _ in 0..secs * 10 {
        let mut need = 0;
        let mut b = [0u8; 64];
        if unsafe { SplQueryJob(null(), Q.as_ptr(), id, 0, b.as_mut_ptr(), b.len() as u32, &mut need) } == NERR_JOBNOTFOUND {
            return true;
        }
        unsafe { DosSleep(100) };
    }
    false
}

/// Wait for the output file to appear and stop growing.
fn wait_output(secs: u32) -> Option<Vec<u8>> {
    let mut last = 0usize;
    for _ in 0..secs * 10 {
        if let Some(d) = read_file(OUT)
            && !d.is_empty()
        {
            if d.len() == last {
                return Some(d);
            }
            last = d.len();
        }
        unsafe { DosSleep(100) };
    }
    read_file(OUT)
}

fn set_port(port: &[u8], settings: &[u8]) {
    unsafe { PrfWriteProfileString(0xffff_fffe, b"PM_SPOOLER_PORT\0".as_ptr(), port.as_ptr(), [settings, b"\0"].concat().as_ptr()) };
}

/// The test queue on its printer on a file port.
fn setup() {
    unsafe {
        SplDeleteQueue(null(), Q.as_ptr());
        SplDeleteDevice(null(), PRN.as_ptr());
        SplDeletePort(null(), PORT.as_ptr());
    }
    check("SplCreatePort FILE9", unsafe { SplCreatePort(null(), PORT.as_ptr(), b"FILE\0".as_ptr(), 0, null(), 0) } == 0);
    set_port(PORT, b"C:\\PRNTEST.PS;");
    // PRDINFO3: printer, user, port, job, status, status text, comment,
    // drivers, time, timeout.
    let drv = b"PSCRIPT.Generic PostScript Printer\0";
    let mut d = Vec::new();
    for p in [PRN.as_ptr() as u32, 0, PORT.as_ptr() as u32] {
        d.extend_from_slice(&p.to_le_bytes());
    }
    d.extend_from_slice(&[0, 0, 0, 0]);
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&(b"PRNTEST printer\0".as_ptr() as u32).to_le_bytes());
    d.extend_from_slice(&(drv.as_ptr() as u32).to_le_bytes());
    d.extend_from_slice(&[0, 0, 45, 0]);
    check("SplCreateDevice PRNTEST", unsafe { SplCreateDevice(null(), 3, d.as_ptr(), d.len() as u32) } == 0);
    // PRQINFO3: name, priority, start, until, type, separator, queue
    // processor, parameters, comment, status, jobs, printers, driver,
    // driver data.
    let mut q = Vec::new();
    q.extend_from_slice(&(Q.as_ptr() as u32).to_le_bytes());
    for v in [5u16, 0, 0, 0] {
        q.extend_from_slice(&v.to_le_bytes());
    }
    for p in [0u32, b"PMPRINT\0".as_ptr() as u32, 0, b"PRNTEST queue\0".as_ptr() as u32] {
        q.extend_from_slice(&p.to_le_bytes());
    }
    q.extend_from_slice(&[0, 0, 0, 0]);
    for p in [PRN.as_ptr() as u32, drv.as_ptr() as u32, 0] {
        q.extend_from_slice(&p.to_le_bytes());
    }
    check("SplCreateQueue PRNTESTQ", unsafe { SplCreateQueue(null(), 3, q.as_ptr(), q.len() as u32) } == 0);
    check("SplCreateQueue again: NERR_QExists", unsafe { SplCreateQueue(null(), 3, q.as_ptr(), q.len() as u32) } == 2154);
}

fn spooler_calls() {
    // Queues: the default printer and the test queue.
    let mut buf = vec![0u8; 8192];
    let (mut ret, mut total, mut need) = (0, 0, 0);
    let rc = unsafe { SplEnumQueue(null(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) };
    let names: Vec<Vec<u8>> = (0..ret as usize).map(|i| cstr(rd32(&buf, i * 44))).collect();
    check("SplEnumQueue level 3 lists PRNTESTQ", rc == 0 && ret == total && names.iter().any(|n| n == b"PRNTESTQ"));
    check("SplEnumQueue has a default queue (PSPRINT)", names.iter().any(|n| n == b"PSPRINT"));
    let rc = unsafe { SplEnumQueue(null(), 3, null_mut(), 0, &mut ret, &mut total, &mut need, 0) };
    check("SplEnumQueue with no buffer: ERROR_MORE_DATA and the size", rc == 234 && ret == 0 && need > 0);
    let rc = unsafe { SplQueryQueue(null(), Q.as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) };
    check("SplQueryQueue level 3", rc == 0 && cstr(rd32(&buf, 24)) == b"PRNTEST queue" && cstr(rd32(&buf, 32)) == b"PRNTEST" && cstr(rd32(&buf, 36)).starts_with(b"PSCRIPT") && rd32(&buf, 40) != 0);
    let rc = unsafe { SplQueryQueue(null(), b"NOSUCHQ\0".as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) };
    check("SplQueryQueue of an unknown queue: NERR_QNotFound", rc == 2150);
    let rc = unsafe { SplQueryQueue(null(), Q.as_ptr(), 3, buf.as_mut_ptr(), 8, &mut need) };
    check("SplQueryQueue into a small buffer: NERR_BufTooSmall", rc == 2123 && need > 44);
    let c = b"Changed comment\0";
    let rc = unsafe { SplSetQueue(null(), Q.as_ptr(), 3, c.as_ptr(), c.len() as u32, 9) };
    unsafe { SplQueryQueue(null(), Q.as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) };
    check("SplSetQueue comment", rc == 0 && cstr(rd32(&buf, 24)) == b"Changed comment");
    // Printers, ports, the printer list.
    let rc = unsafe { SplEnumDevice(null(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) };
    let devs: Vec<(Vec<u8>, Vec<u8>)> = (0..ret as usize).map(|i| (cstr(rd32(&buf, i * 32)), cstr(rd32(&buf, i * 32 + 8)))).collect();
    check("SplEnumDevice level 3: PRNTEST on FILE9", rc == 0 && devs.iter().any(|(n, p)| n == b"PRNTEST" && p == b"FILE9"));
    let rc = unsafe { SplQueryDevice(null(), PRN.as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) };
    check("SplQueryDevice level 3", rc == 0 && cstr(rd32(&buf, 20)) == b"PRNTEST printer");
    let rc = unsafe { SplEnumPort(null(), 1, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) };
    let ports: Vec<Vec<u8>> = (0..ret as usize).map(|i| cstr(rd32(&buf, i * 12))).collect();
    check("SplEnumPort level 1: LPT1, FILE, LPR1, IPP1, FILE9", rc == 0 && [&b"LPT1"[..], b"FILE", b"LPR1", b"IPP1", b"FILE9"].iter().all(|p| ports.iter().any(|x| x == p)));
    let rc = unsafe { SplEnumPrinter(null(), 0, 1 | 2 | 4, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) };
    let any_q = (0..ret as usize).any(|i| rd32(&buf, i * 20) == 1 && cstr(rd32(&buf, i * 20 + 8)) == b"PRNTESTQ");
    check("SplEnumPrinter lists the queue", rc == 0 && any_q);
}

fn qm_open(datatype: &[u8]) -> u32 {
    let dt = [datatype, b"\0"].concat();
    let dop = [Q.as_ptr() as u32, b"PSCRIPT\0".as_ptr() as u32, 0, dt.as_ptr() as u32, b"prntest\0".as_ptr() as u32];
    unsafe { SplQmOpen(b"*\0".as_ptr(), 5, dop.as_ptr()) }
}

/// A raw job: held, queried, changed, released, printed unchanged.
fn raw_job() {
    delete(OUT);
    unsafe { SplHoldQueue(null(), Q.as_ptr()) };
    let h = qm_open(b"PM_Q_RAW");
    check("SplQmOpen", h != 0);
    let data = b"%!PS-Adobe-3.0\n%%Title: raw job\n/Courier findfont 12 scalefont setfont 72 720 moveto (raw job) show showpage\n";
    let ok = unsafe { SplQmStartDoc(h, b"Raw document\0".as_ptr()) } != 0 && unsafe { SplQmWrite(h, data.len() as i32, data.as_ptr()) } != 0;
    let id = unsafe { SplQmEndDoc(h) };
    unsafe { SplQmClose(h) };
    check("SplQmStartDoc/SplQmWrite/SplQmEndDoc give a job id", ok && id != 0);
    let mut buf = vec![0u8; 2048];
    let mut need = 0;
    let rc = unsafe { SplQueryJob(null(), Q.as_ptr(), id, 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) };
    check("SplQueryJob level 3: document, data type, size, queue", rc == 0 && rd16(&buf, 0) as u32 == id && cstr(rd32(&buf, 24)) == b"Raw document" && cstr(rd32(&buf, 32)) == b"PM_Q_RAW" && rd32(&buf, 16) == data.len() as u32 && cstr(rd32(&buf, 44)) == b"PRNTESTQ");
    let (mut ret, mut total) = (0, 0);
    let rc = unsafe { SplEnumJob(null(), Q.as_ptr(), 2, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) };
    check("SplEnumJob level 2 lists the job (queued)", rc == 0 && ret == 1 && rd16(&buf, 0) as u32 == id && rd16(&buf, 10) & 3 == 0);
    check("SplHoldJob", unsafe { SplHoldJob(null(), Q.as_ptr(), id) } == 0);
    unsafe { SplQueryJob(null(), Q.as_ptr(), id, 2, buf.as_mut_ptr(), buf.len() as u32, &mut need) };
    check("a held job is PRJ_QS_PAUSED", rd16(&buf, 10) & 3 == 1);
    let c = b"a comment\0";
    unsafe { SplSetJob(null(), Q.as_ptr(), id, 3, c.as_ptr(), c.len() as u32, 11) };
    unsafe { SplQueryJob(null(), Q.as_ptr(), id, 2, buf.as_mut_ptr(), buf.len() as u32, &mut need) };
    check("SplSetJob comment", cstr(rd32(&buf, 20)) == b"a comment");
    check("SplReleaseJob", unsafe { SplReleaseJob(null(), Q.as_ptr(), id) } == 0);
    // A second job, deleted before it prints.
    let h2 = qm_open(b"PM_Q_RAW");
    unsafe {
        SplQmWrite(h2, 4, b"junk".as_ptr());
    }
    let id2 = unsafe { SplQmEndDoc(h2) };
    unsafe { SplQmClose(h2) };
    check("SplDeleteJob", id2 != 0 && unsafe { SplDeleteJob(null(), Q.as_ptr(), id2) } == 0 && unsafe { SplQueryJob(null(), Q.as_ptr(), id2, 0, buf.as_mut_ptr(), 64, &mut need) } == NERR_JOBNOTFOUND);
    check("SplReleaseQueue", unsafe { SplReleaseQueue(null(), Q.as_ptr()) } == 0);
    check("the spooler prints the raw job", wait_printed(id, 30));
    let out = wait_output(10).unwrap_or_default();
    check("the raw job reaches the FILE port unchanged", out == data);
}

/// DEVOPENSTRUC for the test queue.
fn dop(datatype: &'static [u8]) -> [u32; 9] {
    [Q.as_ptr() as u32, b"PSCRIPT\0".as_ptr() as u32, 0, datatype.as_ptr() as u32, b"prntest\0".as_ptr() as u32, 0, 0, 0, 0]
}

/// A small colour bitmap (8 x 8, 24 bpp: red and blue quarters).
fn bitmap(hps: u32) -> u32 {
    let mut hdr = [0u8; 12];
    hdr[0] = 12;
    hdr[4] = 8;
    hdr[6] = 8;
    hdr[8] = 1;
    hdr[10] = 24;
    let mut bits = Vec::new();
    for y in 0..8 {
        for x in 0..8 {
            // BGR
            bits.extend_from_slice(if (x < 4) == (y < 4) { &[0, 0, 255] } else { &[255, 0, 0] });
        }
    }
    unsafe { GpiCreateBitmap(hps, hdr.as_ptr(), 4, bits.as_ptr(), hdr.as_ptr()) }
}

/// Draw the test page in LOENGLISH units (hundredths of an inch).
fn draw_page(hps: u32, page: u32) {
    let p = |x: i32, y: i32| [x, y];
    unsafe {
        GpiSetColor(hps, 2); // red
        GpiMove(hps, p(100, 100).as_ptr());
        GpiBox(hps, 2, p(750, 1000).as_ptr(), 0, 0);
        GpiSetColor(hps, 1); // blue
        GpiSetLineType(hps, 2);
        GpiMove(hps, p(100, 100).as_ptr());
        GpiLine(hps, p(750, 1000).as_ptr());
        GpiSetLineType(hps, 0);
        // An area with a pattern.
        GpiSetColor(hps, 4); // green... (CLR_PINK 3, CLR_GREEN 4)
        GpiSetPattern(hps, 6);
        GpiBeginArea(hps, 0);
        GpiMove(hps, p(150, 150).as_ptr());
        GpiPolyLine(hps, 2, [300, 150, 225, 300].as_ptr());
        GpiEndArea(hps);
        GpiSetPattern(hps, 0);
        // A circle.
        GpiSetColor(hps, -1); // CLR_DEFAULT
        GpiMove(hps, p(500, 300).as_ptr());
        GpiFullArc(hps, 2, 50 << 16);
        // Text in the default font, then in Times New Roman 24 points.
        let t: &[u8] = if page == 1 { b"Hello, printer" } else { b"Page two" };
        GpiCharStringAt(hps, p(100, 900).as_ptr(), t.len() as i32, t.as_ptr());
        let mut fat = [0u8; 56];
        fat[0] = 56;
        let face = b"Times New Roman";
        fat[8..8 + face.len()].copy_from_slice(face);
        fat[54] = 4; // FATTR_FONTUSE_OUTLINE
        GpiCreateLogFont(hps, null(), 1, fat.as_ptr());
        GpiSetCharSet(hps, 1);
        // 24 points = 33 hundredths of an inch.
        GpiSetCharBox(hps, [33 << 16, 33 << 16].as_ptr());
        GpiCharStringAt(hps, p(100, 800).as_ptr(), 11, b"rs2 prints!".as_ptr());
        GpiSetCharSet(hps, 0);
        // A bitmap, 1 inch square.
        let hbm = bitmap(hps);
        GpiWCBitBlt(hps, hbm, 4, [500, 500, 599, 599, 0, 0, 8, 8].as_ptr(), 0xcc, 0);
        GpiDeleteBitmap(hbm);
    }
}

fn check_postscript(what: &str, out: &[u8], pages: usize) {
    check(&alloc::format!("{what}: a DSC PostScript document"), out.starts_with(b"%!PS-Adobe-3.0\n") && has(out, b"%%EOF"));
    check(&alloc::format!("{what}: {pages} page(s)"), count(out, b"%%Page: ") == pages && has(out, alloc::format!("%%Pages: {pages}\n").as_bytes()));
    check(&alloc::format!("{what}: text shown with xshow"), has(out, b"(Hello, printer) [") && has(out, b"] xshow"));
    check(&alloc::format!("{what}: an outline font's glyphs (Type 3)"), has(out, b"setcharwidth") && has(out, b"(rs2 prints!) ["));
    check(&alloc::format!("{what}: a patterned area"), has(out, b"makepattern") && has(out, b"setcolor"));
    check(&alloc::format!("{what}: a dashed blue line"), has(out, b"0 0 1 rg") && has(out, b"[12.5 12.5] 0 d"));
    check(&alloc::format!("{what}: a red box"), has(out, b"1 0 0 rg"));
    check(&alloc::format!("{what}: the bitmap as an image"), has(out, b"false 3 colorimage"));
}

/// Print the two test pages on a printer DC; returns the job id
/// (queued DCs).
fn print_pages(ty: i32, d: &[u32; 9]) -> (bool, u32) {
    let hdc = unsafe { DevOpenDC(0, ty, b"*\0".as_ptr(), 9, d.as_ptr(), 0) };
    if hdc == 0 {
        return (false, 0);
    }
    let size = [0i32, 0];
    let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), PU_LOENGLISH | GPIA_ASSOC) };
    let mut ok = hps != 0;
    ok &= unsafe { DevEscape(hdc, DEVESC_STARTDOC, 10, b"Test pages".as_ptr(), null_mut(), null_mut()) } == 1;
    draw_page(hps, 1);
    ok &= unsafe { DevEscape(hdc, DEVESC_NEWFRAME, 0, null(), null_mut(), null_mut()) } == 1;
    draw_page(hps, 2);
    let mut id = [0u8; 2];
    let mut n = 2;
    ok &= unsafe { DevEscape(hdc, DEVESC_ENDDOC, 0, null(), &mut n, id.as_mut_ptr()) } == 1;
    unsafe {
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
    (ok, u16::from_le_bytes(id) as u32)
}

fn queued_std() {
    delete(OUT);
    let d = dop(b"PM_Q_STD\0");
    // Queries on an OD_INFO DC of the queue first.
    let hdc = unsafe { DevOpenDC(0, OD_INFO, b"*\0".as_ptr(), 4, d.as_ptr(), 0) };
    let mut caps = [0i32; 10];
    let ok = hdc != 0 && unsafe { DevQueryCaps(hdc, 0, 10, caps.as_mut_ptr()) } == 1;
    check("DevQueryCaps on the printer: PostScript, 8.5 x 11 inches at 300 dpi", ok && caps[2] == 5 && caps[4] == 2550 && caps[5] == 3300 && caps[8] == 11811);
    let n = unsafe { DevQueryHardcopyCaps(hdc, 0, 0, null_mut()) };
    let mut hc = vec![0u8; 68 * 8];
    let k = unsafe { DevQueryHardcopyCaps(hdc, 0, n, hc.as_mut_ptr()) };
    check("DevQueryHardcopyCaps: the forms, Letter current", n >= 2 && k == n && hc.starts_with(b"Letter\0") && rd32(&hc, 64) & 1 != 0 && rd32(&hc, 32) == 215);
    unsafe { DevCloseDC(hdc) };
    let (ok, id) = print_pages(OD_QUEUED, &d);
    check("a PM_Q_STD queued DC: STARTDOC, two pages, ENDDOC gives a job", ok && id != 0);
    check("the spooler prints the metafile job (PMPRINT)", wait_printed(id, 60));
    let out = wait_output(10).unwrap_or_default();
    check_postscript("PM_Q_STD", &out, 2);
}

fn queued_raw() {
    delete(OUT);
    let d = dop(b"PM_Q_RAW\0");
    let (ok, id) = print_pages(OD_QUEUED, &d);
    check("a PM_Q_RAW queued DC gives a job", ok && id != 0);
    check("the spooler prints the PostScript job", wait_printed(id, 60));
    let out = wait_output(10).unwrap_or_default();
    check_postscript("PM_Q_RAW", &out, 2);
}

fn direct() {
    delete(OUT);
    let mut d = dop(b"PM_Q_STD\0");
    d[0] = PRN.as_ptr() as u32;
    let (ok, _) = print_pages(OD_DIRECT, &d);
    check("an OD_DIRECT DC on the printer", ok);
    let out = read_file(OUT).unwrap_or_default();
    check_postscript("OD_DIRECT", &out, 2);
    // Raw data on a direct DC.
    delete(OUT);
    let raw = b"DATATYPE=PM_Q_RAW\0";
    let mut d = dop(b"PM_Q_RAW\0");
    d[0] = PRN.as_ptr() as u32;
    d[6] = raw.as_ptr() as u32;
    let hdc = unsafe { DevOpenDC(0, OD_DIRECT, b"*\0".as_ptr(), 9, d.as_ptr(), 0) };
    let data = b"%!PS\nshowpage\n";
    let ok = hdc != 0
        && unsafe { DevEscape(hdc, DEVESC_STARTDOC, 3, b"raw".as_ptr(), null_mut(), null_mut()) } == 1
        && unsafe { DevEscape(hdc, DEVESC_RAWDATA, data.len() as i32, data.as_ptr(), null_mut(), null_mut()) } == 1
        && unsafe { DevEscape(hdc, DEVESC_ENDDOC, 0, null(), null_mut(), null_mut()) } == 1;
    unsafe { DevCloseDC(hdc) };
    check("DEVESC_RAWDATA on a raw OD_DIRECT DC goes out unchanged", ok && read_file(OUT).as_deref() == Some(&data[..]));
}

fn job_properties() {
    let need = unsafe { DevPostDeviceModes(0, null_mut(), b"PSCRIPT\0".as_ptr(), b"Generic PostScript Printer\0".as_ptr(), Q.as_ptr(), 2) };
    let mut dd = vec![0u8; need.max(0) as usize];
    let rc = unsafe { DevPostDeviceModes(0, dd.as_mut_ptr(), b"PSCRIPT\0".as_ptr(), b"Generic PostScript Printer\0".as_ptr(), Q.as_ptr(), 2) };
    check("DevPostDeviceModes: the DRIVDATA size, the job properties", need >= 40 && rc == 1 && rd32(&dd, 0) as i32 == need && dd[8..].starts_with(b"Generic PostScript Printer"));
    let (mut n, mut nt) = (4, 4);
    let mut names = [0u8; 32 * 4];
    let mut descs = [0u8; 64 * 4];
    let mut types = [0u8; 16 * 4];
    let rc = unsafe { DevQueryDeviceNames(0, b"PSCRIPT\0".as_ptr(), &mut n, names.as_mut_ptr(), descs.as_mut_ptr(), &mut nt, types.as_mut_ptr()) };
    check("DevQueryDeviceNames PSCRIPT", rc == 1 && n == 1 && names.starts_with(b"Generic PostScript Printer") && nt == 2 && types.starts_with(b"PM_Q_STD"));
    // A landscape A4 job.
    dd[44] = 1;
    dd[48] = 1;
    let mut d = dop(b"PM_Q_STD\0");
    d[0] = PRN.as_ptr() as u32;
    d[2] = dd.as_ptr() as u32;
    let hdc = unsafe { DevOpenDC(0, OD_INFO, b"*\0".as_ptr(), 4, d.as_ptr(), 0) };
    let mut caps = [0i32; 6];
    unsafe { DevQueryCaps(hdc, 0, 6, caps.as_mut_ptr()) };
    let size = [0i32, 0];
    let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), PU_PELS | GPIA_ASSOC) };
    let mut ps = [0i32; 2];
    unsafe { GpiQueryPS(hps, ps.as_mut_ptr()) };
    check("job properties: A4 landscape is 3508 x 2479 pels", caps[4] == 3508 && caps[5] == 2479);
    unsafe {
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
}

/// Print to a network printer port (LPRn/IPPn) the listener checks.
fn network(kind: &[u8], host: &[u8], port: &[u8], extra: &[u8]) {
    let pname = if kind == b"LPR" { b"LPR9\0" } else { b"IPP9\0" };
    unsafe { SplCreatePort(null(), pname.as_ptr(), null(), 0, null(), 0) };
    set_port(pname, &[host, b";", extra, b";", port, b";"].concat());
    // The test printer now prints to the network port.
    let mut d = Vec::new();
    for p in [PRN.as_ptr() as u32, 0, pname.as_ptr() as u32] {
        d.extend_from_slice(&p.to_le_bytes());
    }
    d.extend_from_slice(&[0u8; 20]);
    unsafe extern "C" {
        #[link_name = "PMSPL.402"]
        fn SplSetDevice(comp: *const u8, dev: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    }
    check(&alloc::format!("{} port: SplSetDevice", core::str::from_utf8(kind).unwrap()), unsafe { SplSetDevice(null(), PRN.as_ptr(), 3, pname.as_ptr(), 5, 3) } == 0);
    let (ok, id) = print_pages(OD_QUEUED, &dop(b"PM_Q_STD\0"));
    let printed = ok && id != 0 && wait_printed(id, 60);
    let mut buf = [0u8; 64];
    let mut need = 0;
    check(&alloc::format!("{} port: the job is sent (not held in error)", core::str::from_utf8(kind).unwrap()), printed && unsafe { SplQueryJob(null(), Q.as_ptr(), id, 0, buf.as_mut_ptr(), 64, &mut need) } == NERR_JOBNOTFOUND);
    unsafe { SplSetDevice(null(), PRN.as_ptr(), 3, PORT.as_ptr(), 6, 3) };
    unsafe { SplDeletePort(null(), pname.as_ptr()) };
}

fn cleanup() {
    check("SplDeleteQueue", unsafe { SplDeleteQueue(null(), Q.as_ptr()) } == 0);
    check("SplDeleteDevice", unsafe { SplDeleteDevice(null(), PRN.as_ptr()) } == 0);
    check("SplDeletePort", unsafe { SplDeletePort(null(), PORT.as_ptr()) } == 0);
}

/// SETPORT printer port settings [RUN program args].
fn set_port_and_run(args: &[&[u8]]) -> u32 {
    unsafe extern "C" {
        #[link_name = "PMSPL.402"]
        fn SplSetDevice(comp: *const u8, dev: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    }
    let (Some(prn), Some(port)) = (args.get(1), args.get(2)) else {
        println!("usage: PRNTEST SETPORT printer port settings [RUN program args]");
        return 1;
    };
    let port = upper(port);
    let pz = [&port[..], b"\0"].concat();
    unsafe { SplCreatePort(null(), pz.as_ptr(), null(), 0, null(), 0) };
    set_port(&pz, args.get(3).copied().unwrap_or(b""));
    let prn = [&upper(prn)[..], b"\0"].concat();
    let rc = unsafe { SplSetDevice(null(), prn.as_ptr(), 3, pz.as_ptr(), pz.len() as u32, 3) };
    println!("prntest: port set: {rc}");
    if let Some(i) = args.iter().position(|a| a.eq_ignore_ascii_case(b"RUN")) {
        let Some(prog) = args.get(i + 1) else { return rc };
        let mut a = Vec::new();
        a.extend_from_slice(prog);
        a.push(0);
        a.extend_from_slice(&args[i + 2..].join(&b' '));
        a.extend_from_slice(b"\0\0");
        let path = [*prog, b"\0"].concat();
        let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
        let mut fail = [0u8; 64];
        let r = unsafe { DosExecPgm(fail.as_mut_ptr(), 64, 0, a.as_ptr(), null(), &mut res, path.as_ptr()) };
        println!("prntest: {} ended: {r} {}", core::str::from_utf8(prog).unwrap_or("?"), res.code_result);
    }
    rc
}

fn main(s: &Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    unsafe { WinCreateMsgQueue(hab, 0) };
    let args: Vec<&[u8]> = s.args().split(|&c| c == b' ').filter(|a| !a.is_empty()).collect();
    if args.first().is_some_and(|a| a.eq_ignore_ascii_case(b"SETPORT")) {
        return set_port_and_run(&args);
    }
    if args.first().is_some_and(|a| a.eq_ignore_ascii_case(b"MET")) {
        return metafile_colors(args.get(1).copied().unwrap_or(b""));
    }
    let keep = args.iter().any(|a| a.eq_ignore_ascii_case(b"KEEP"));
    setup();
    spooler_calls();
    raw_job();
    queued_std();
    queued_raw();
    direct();
    job_properties();
    if let Some(i) = args.iter().position(|a| a.eq_ignore_ascii_case(b"LPR") || a.eq_ignore_ascii_case(b"IPP")) {
        let kind = crate::upper(args[i]);
        let host = args.get(i + 1).copied().unwrap_or(b"10.0.2.2");
        let port = args.get(i + 2).copied().unwrap_or(if kind == b"LPR" { b"515" } else { b"631" });
        let extra = args.get(i + 3).copied().unwrap_or(b"");
        network(&kind, host, port, extra);
    }
    if !keep {
        cleanup();
    }
    let f = FAILS.load(Ordering::Relaxed);
    println!("prntest done: {f} failure(s)");
    f
}

/// MET file: play a metafile on a direct and on a queued printer DC and
/// list the colours the PostScript sets (a diagnostic).
fn metafile_colors(file: &[u8]) -> u32 {
    setup();
    let name = [file, b"\0"].concat();
    for ty in [OD_DIRECT, OD_QUEUED] {
        delete(OUT);
        let mut d = dop(b"PM_Q_STD\0");
        if ty == OD_DIRECT {
            d[0] = PRN.as_ptr() as u32;
        }
        let hmf = unsafe { GpiLoadMetaFile(0, name.as_ptr()) };
        let hdc = unsafe { DevOpenDC(0, ty, b"*\0".as_ptr(), 9, d.as_ptr(), 0) };
        let size = [0i32, 0];
        let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), PU_PELS | GPIA_ASSOC) };
        unsafe { DevEscape(hdc, DEVESC_STARTDOC, 3, b"met".as_ptr(), null_mut(), null_mut()) };
        let opts = [0, 1, 0, 3, 1, 1, 3, 2, 3];
        let mut segs = 0;
        let mut desc = [0u8; 2];
        let rc = unsafe { GpiPlayMetaFile(hps, hmf, 9, opts.as_ptr(), &mut segs, 0, desc.as_mut_ptr()) };
        let mut id = [0u8; 2];
        let mut n = 2;
        unsafe {
            DevEscape(hdc, DEVESC_ENDDOC, 0, null(), &mut n, id.as_mut_ptr());
            GpiAssociate(hps, 0);
            GpiDestroyPS(hps);
            DevCloseDC(hdc);
        }
        let jid = u16::from_le_bytes(id) as u32;
        if ty == OD_QUEUED {
            wait_printed(jid, 60);
        }
        let out = wait_output(10).unwrap_or_default();
        let mut colors: Vec<&[u8]> = out.split(|&c| c == b'\n').filter(|l| l.ends_with(b" rg")).collect();
        colors.sort();
        colors.dedup();
        println!("prntest met: {} play {} job {} bytes {} colours {}", if ty == OD_DIRECT { "direct" } else { "queued" }, rc, jid, out.len(), colors.len());
        for c in colors.iter().take(20) {
            println!("  {}", core::str::from_utf8(c).unwrap_or("?"));
        }
    }
    cleanup();
    0
}

fn upper(s: &[u8]) -> Vec<u8> {
    s.iter().map(|c| c.to_ascii_uppercase()).collect()
}
