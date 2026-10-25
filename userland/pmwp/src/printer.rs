//! Printer objects (WPPrinter, IBM's WPPRINT.DLL class) on rs2's spooler
//! (PMSPL, docs/PRINTING.md). A printer object stands for a queue
//! (QUEUENAME=): its title is the queue's description.
//!
//! - Opening it shows the job view: the queue's jobs (number, document,
//!   status, size), brought up to date every two seconds, with Hold,
//!   Release and Delete for the selected job and Hold/Release printer
//!   for the queue.
//! - Its Printer page shows the queue, the printer and the driver, has
//!   Job properties (the driver's dialog, DevPostDeviceModes, kept with
//!   the queue), the output port (LPT1-3, COM1-4, FILE, LPRn, IPPn) with
//!   the port's settings (FILE: the output file; LPR: host;queue;port;
//!   IPP: host;path;port), and "default printer".
//! - Files dropped on it are printed, as data files' Print does: a
//!   PostScript file (starting with `%!`) as it is, anything else as
//!   text (Courier 10 points, 60 lines a page, tabs every 8 columns, form
//!   feeds start pages).
//! - A printer made from the template (the Printers folder's "Printer",
//!   IBM's Install New Printer) gets a new queue and printer of its own:
//!   the PostScript driver on the FILE port, its settings open.
//! - Deleting a printer object deletes its queue (and the printer, when
//!   no other queue uses it).
//! - When the shell starts, every queue without a printer object gets
//!   one in the Printers folder (`<RS2_PRINTER_queue>`).

#![allow(static_mut_refs)]

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
use crate::setup::add_page;
use crate::som;
use crate::util::{cstr, str_bytes, str_new, z, Str};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
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
    #[link_name = "PMSPL.404"]
    fn SplPurgeQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.392"]
    fn SplEnumJob(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.386"]
    fn SplDeleteJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.385"]
    fn SplHoldJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.384"]
    fn SplReleaseJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.401"]
    fn SplCreateDevice(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32;
    #[link_name = "PMSPL.402"]
    fn SplSetDevice(comp: *const u8, dev: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    #[link_name = "PMSPL.381"]
    fn SplQueryDevice(comp: *const u8, dev: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.403"]
    fn SplDeleteDevice(comp: *const u8, dev: *const u8) -> u32;
    #[link_name = "PMSPL.408"]
    fn SplEnumPort(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
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
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.605"]
    fn DevEscape(hdc: u32, code: i32, nin: i32, pin: *const u8, nout: *mut i32, pout: *mut u8) -> i32;
    #[link_name = "PMGPI.606"]
    fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32;
    #[link_name = "PMGPI.609"]
    fn DevPostDeviceModes(hab: u32, drv: *mut u8, name: *const u8, dev: *const u8, pname: *const u8, fl: u32) -> i32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    fn GpiAssociate(hps: u32, hdc: u32) -> u32;
    #[link_name = "PMGPI.359"]
    fn GpiCharStringAt(hps: u32, p: *const i32, n: i32, s: *const u8) -> i32;
    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
}

const HINI_SYSTEM: u32 = 0xffff_fffe;

// ---- The spooler, read ----

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn ptr_str(p: u32) -> Vec<u8> {
    cstr(p as *const u8).to_vec()
}

/// A queue as PRQINFO3 tells it.
#[derive(Default, Clone)]
pub struct QueueInfo {
    pub name: Vec<u8>,
    pub descr: Vec<u8>,
    pub held: bool,
    pub jobs: u16,
    pub printers: Vec<u8>,
    pub driver: Vec<u8>,
    pub drivdata: Vec<u8>,
    pub flags: u16,
}

fn queue_info_at(b: &[u8], o: usize) -> QueueInfo {
    let dd = rd32(b, o + 40);
    let drivdata = if dd == 0 {
        Vec::new()
    } else {
        let cb = unsafe { (dd as *const u32).read_unaligned() } as usize;
        if (40..=0x10000).contains(&cb) { unsafe { core::slice::from_raw_parts(dd as *const u8, cb) }.to_vec() } else { Vec::new() }
    };
    QueueInfo {
        name: ptr_str(rd32(b, o)),
        flags: rd16(b, o + 10),
        descr: ptr_str(rd32(b, o + 24)),
        held: rd16(b, o + 28) & 1 != 0,
        jobs: rd16(b, o + 30),
        printers: ptr_str(rd32(b, o + 32)),
        driver: ptr_str(rd32(b, o + 36)),
        drivdata,
    }
}

pub fn queue_info(q: &[u8]) -> Option<QueueInfo> {
    let name = z(q);
    let mut buf = vec![0u8; 4096];
    let mut need = 0;
    if unsafe { SplQueryQueue(null(), name.as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) } != 0 {
        return None;
    }
    Some(queue_info_at(&buf, 0))
}

pub fn queues() -> Vec<QueueInfo> {
    let (mut ret, mut total, mut need) = (0, 0, 0);
    unsafe { SplEnumQueue(null(), 3, null_mut(), 0, &mut ret, &mut total, &mut need, 0) };
    let mut buf = vec![0u8; need as usize + 64];
    if unsafe { SplEnumQueue(null(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) } != 0 {
        return Vec::new();
    }
    (0..ret as usize).map(|i| queue_info_at(&buf, i * 44)).collect()
}

/// A printer's port and description.
fn device(name: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let n = z(name);
    let mut buf = vec![0u8; 2048];
    let mut need = 0;
    if unsafe { SplQueryDevice(null(), n.as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) } != 0 {
        return None;
    }
    Some((ptr_str(rd32(&buf, 8)), ptr_str(rd32(&buf, 20))))
}

fn ports() -> Vec<Vec<u8>> {
    let (mut ret, mut total, mut need) = (0, 0, 0);
    let mut buf = vec![0u8; 4096];
    if unsafe { SplEnumPort(null(), 1, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) } != 0 {
        return Vec::new();
    }
    (0..ret as usize).map(|i| ptr_str(rd32(&buf, i * 12))).collect()
}

fn port_settings(port: &[u8]) -> Vec<u8> {
    let mut buf = [0u8; 512];
    let n = unsafe { PrfQueryProfileString(HINI_SYSTEM, b"PM_SPOOLER_PORT\0".as_ptr(), z(port).as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let mut v = buf[..n.min(buf.len())].to_vec();
    while v.last() == Some(&0) {
        v.pop();
    }
    v
}

fn set_port_settings(port: &[u8], s: &[u8]) {
    let mut v = s.to_vec();
    if !v.ends_with(b";") {
        v.push(b';');
    }
    unsafe { PrfWriteProfileString(HINI_SYSTEM, b"PM_SPOOLER_PORT\0".as_ptr(), z(port).as_ptr(), z(&v).as_ptr()) };
}

/// The default queue (PM_SPOOLER QUEUE).
pub fn default_queue() -> Option<Vec<u8>> {
    let mut buf = [0u8; 64];
    let n = unsafe { PrfQueryProfileString(HINI_SYSTEM, b"PM_SPOOLER\0".as_ptr(), b"QUEUE\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let v: Vec<u8> = buf[..n.min(64)].iter().copied().take_while(|&c| c != 0 && c != b';').collect();
    (!v.is_empty()).then_some(v).or_else(|| queues().first().map(|q| q.name.clone()))
}

// ---- Printing files ----

const OD_QUEUED: i32 = 2;
const DEVESC_STARTDOC: i32 = 8150;
const DEVESC_ENDDOC: i32 = 8151;
const DEVESC_NEWFRAME: i32 = 16300;

/// Print a file on a queue: PostScript as it is, else as text. False if
/// it could not be spooled.
pub fn print_file(queue: &[u8], path: &[u8]) -> bool {
    let Some(data) = crate::mindex::read_all(path) else { return false };
    let name = path.rsplit(|&c| c == b'\\').next().unwrap_or(path);
    if data.starts_with(b"%!") {
        let q = z(queue);
        let dop = [q.as_ptr() as u32, 0, 0, b"PM_Q_RAW\0".as_ptr() as u32];
        let h = unsafe { SplQmOpen(b"*\0".as_ptr(), 4, dop.as_ptr()) };
        if h == 0 {
            return false;
        }
        let ok = unsafe { SplQmStartDoc(h, z(name).as_ptr()) } != 0 && data.chunks(32768).all(|c| unsafe { SplQmWrite(h, c.len() as i32, c.as_ptr()) } != 0);
        let id = unsafe { SplQmEndDoc(h) };
        unsafe { SplQmClose(h) };
        return ok && id != 0;
    }
    print_text(queue, name, &data)
}

/// Text through the GPI on a queued DC (PM_Q_STD).
pub fn print_text(queue: &[u8], name: &[u8], data: &[u8]) -> bool {
    let q = z(queue);
    let dop = [q.as_ptr() as u32, 0, 0, b"PM_Q_STD\0".as_ptr() as u32];
    let hdc = unsafe { DevOpenDC(0, OD_QUEUED, b"*\0".as_ptr(), 4, dop.as_ptr(), 0) };
    if hdc == 0 {
        return false;
    }
    let mut caps = [0i32; 39];
    unsafe { DevQueryCaps(hdc, 0, 39, caps.as_mut_ptr()) };
    let (w, h, dpi) = (caps[4].max(1), caps[5].max(1), caps[38].max(72));
    let size = [0i32, 0];
    // PU_PELS | GPIA_ASSOC
    let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), 0x0008 | 0x4000) };
    let margin = dpi * 3 / 4;
    let lh = dpi * 12 / 72;
    let cw = dpi * 6 / 72;
    let cols = ((w - 2 * margin) / cw).max(10) as usize;
    let rows = ((h - 2 * margin) / lh).max(5);
    let doc = z(name);
    let mut ok = hps != 0 && unsafe { DevEscape(hdc, DEVESC_STARTDOC, name.len() as i32, doc.as_ptr(), null_mut(), null_mut()) } == 1;
    let mut row = 0;
    let newpage = |row: &mut i32, ok: &mut bool| {
        *ok &= unsafe { DevEscape(hdc, DEVESC_NEWFRAME, 0, null(), null_mut(), null_mut()) } == 1;
        *row = 0;
    };
    let text = data.strip_suffix(b"\x1a").unwrap_or(data);
    for line in text.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        // Form feeds start pages.
        for (k, part) in line.split(|&c| c == 0x0c).enumerate() {
            if k > 0 && row > 0 {
                newpage(&mut row, &mut ok);
            }
            let mut out = Vec::new();
            for &c in part {
                if c == b'\t' {
                    let n = 8 - out.len() % 8;
                    out.extend(core::iter::repeat_n(b' ', n));
                } else {
                    out.push(c);
                }
            }
            let mut pieces: Vec<&[u8]> = out.chunks(cols).collect();
            if pieces.is_empty() {
                pieces.push(b"");
            }
            for p in pieces {
                if row >= rows {
                    newpage(&mut row, &mut ok);
                }
                let at = [margin, h - margin - (row + 1) * lh + lh / 4];
                if !p.is_empty() {
                    unsafe { GpiCharStringAt(hps, at.as_ptr(), p.len() as i32, p.as_ptr()) };
                }
                row += 1;
            }
        }
    }
    let mut id = [0u8; 2];
    let mut n = 2;
    ok &= unsafe { DevEscape(hdc, DEVESC_ENDDOC, 0, null(), &mut n, id.as_mut_ptr()) } == 1;
    unsafe {
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
    ok
}

/// A data file's Print (WPMENUID_PRINT): on the default printer.
pub fn print_object(o: usize) -> bool {
    let Some(path) = wps::path_of(o) else { return false };
    let Some(q) = default_queue() else {
        message(HWND_DESKTOP, b"There is no printer to print on: make one from the Printer template in the Printers folder.", b"Print", MB_OK | MB_WARNING);
        return false;
    };
    let ok = print_file(&q, &path);
    if !ok {
        message(HWND_DESKTOP, b"The file could not be printed.", &obj::title(o), MB_OK | MB_ERROR);
    }
    ok
}

// ---- The object ----

#[repr(C)]
pub struct PrinterData {
    queue: Str,
}

fn pd(o: usize) -> &'static mut PrinterData {
    som::data(wps::ccd("WPPrinter"), o)
}

fn queue_of(o: usize) -> Vec<u8> {
    str_bytes(&pd(o).queue).to_vec()
}

fn one_line(t: &[u8]) -> Vec<u8> {
    t.iter().map(|&c| if c == b'\n' { b' ' } else { c }).collect()
}

/// Open job views: (printer, frame, client, list box, job ids shown).
struct JobView {
    obj: usize,
    frame: u32,
    client: u32,
    list: u32,
    ids: Vec<u32>,
}

static mut JOB_VIEWS: Vec<JobView> = Vec::new();
static JOBS_CLASS: &[u8] = b"RS2PrinterJobs\0";

const ID_LIST: u32 = 10;
const ID_HOLD: u32 = 11;
const ID_RELEASE: u32 = 12;
const ID_DELETE: u32 = 13;
const ID_HOLDQ: u32 = 14;
const ID_RELEASEQ: u32 = 15;
const ID_PURGE: u32 = 16;
const BUTTONS: [(u32, &[u8]); 6] = [(ID_HOLD, b"~Hold"), (ID_RELEASE, b"~Release"), (ID_DELETE, b"~Delete"), (ID_HOLDQ, b"Hold ~printer"), (ID_RELEASEQ, b"Release p~rinter"), (ID_PURGE, b"Delete ~all")];
const BAR: i32 = 30;
const STATUS: i32 = 22;

fn view_of_client(h: u32) -> Option<&'static mut JobView> {
    unsafe { JOB_VIEWS.iter_mut().find(|v| v.client == h) }
}

fn status_text(s: u16) -> &'static [u8] {
    if s & 0x10 != 0 {
        return b"Error";
    }
    match s & 3 {
        0 => b"Waiting",
        1 => b"Held",
        2 => b"Spooling",
        _ => b"Printing",
    }
}

fn dec(n: u32) -> Vec<u8> {
    alloc::format!("{n}").into_bytes()
}

/// Fill the job list again (keeping the selected job selected).
fn refresh(v: &mut JobView) {
    let q = queue_of(v.obj);
    let sel = send(v.list, LM_QUERYSELECTION, 0xffff, 0);
    let keep = v.ids.get(sel as usize).copied();
    let (mut ret, mut total, mut need) = (0, 0, 0);
    let qz = z(&q);
    unsafe { SplEnumJob(null(), qz.as_ptr(), 2, null_mut(), 0, &mut ret, &mut total, &mut need, 0) };
    let mut buf = vec![0u8; need as usize + 64];
    let mut lines: Vec<(u32, Vec<u8>)> = Vec::new();
    if unsafe { SplEnumJob(null(), qz.as_ptr(), 2, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) } == 0 {
        for i in 0..ret as usize {
            let o = i * 32;
            let id = rd16(&buf, o) as u32;
            let doc = ptr_str(rd32(&buf, o + 28));
            let doc = if doc.is_empty() { ptr_str(rd32(&buf, o + 24)) } else { doc };
            let mut t = dec(id);
            t.extend_from_slice(b"   ");
            t.extend_from_slice(if doc.is_empty() { b"(untitled)" } else { &doc });
            t.extend_from_slice(b"   ");
            t.extend_from_slice(status_text(rd16(&buf, o + 10)));
            t.extend_from_slice(b"   ");
            t.extend_from_slice(&dec(rd32(&buf, o + 16)));
            t.extend_from_slice(b" bytes");
            lines.push((id, t));
        }
    }
    let ids: Vec<u32> = lines.iter().map(|l| l.0).collect();
    // Redraw only when something changed.
    let same = ids == v.ids && (0..lines.len()).all(|i| {
        let mut b = [0u8; 256];
        let n = send(v.list, LM_QUERYITEMTEXT, mp(i as u32, 256), b.as_mut_ptr() as u32) as usize;
        b[..n.min(256)] == lines[i].1[..]
    });
    if !same {
        send(v.list, LM_DELETEALL, 0, 0);
        for (_, t) in &lines {
            let t = z(t);
            send(v.list, LM_INSERTITEM, 0xffff, t.as_ptr() as u32);
        }
        v.ids = ids;
        if let Some(k) = keep.and_then(|k| v.ids.iter().position(|&x| x == k)) {
            send(v.list, LM_SELECTITEM, k as u32, 1);
        }
    }
    unsafe { WinInvalidateRect(v.client, null(), 0) };
}

fn layout(v: &JobView) {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(v.client, &mut r) };
    let w = r.r - r.l;
    unsafe {
        WinSetWindowPos(v.list, 0, 4, BAR + 2, (w - 8).max(10), (r.t - BAR - STATUS - 4).max(10), SWP_MOVE | SWP_SIZE | SWP_SHOW);
        let bw = ((w - 8) / BUTTONS.len() as i32).max(40);
        for (k, (id, _)) in BUTTONS.iter().enumerate() {
            let b = WinWindowFromID(v.client, *id);
            WinSetWindowPos(b, 0, 4 + k as i32 * bw, 4, bw - 4, BAR - 8, SWP_MOVE | SWP_SIZE | SWP_SHOW);
        }
    }
}

extern "C" fn jobs_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            unsafe {
                let hps = WinBeginPaint(h, 0, &mut r);
                WinQueryWindowRect(h, &mut r);
                WinFillRect(hps, &r, -2);
                if let Some(v) = view_of_client(h) {
                    let q = queue_of(v.obj);
                    let info = queue_info(&q);
                    let mut t = Vec::new();
                    match &info {
                        Some(i) => {
                            t.extend_from_slice(b"Queue ");
                            t.extend_from_slice(&i.name);
                            for p in i.printers.split(|&c| c == b',').filter(|p| !p.is_empty()) {
                                t.extend_from_slice(b", printer ");
                                t.extend_from_slice(p);
                                if let Some((port, _)) = device(p) {
                                    t.extend_from_slice(b" on ");
                                    t.extend_from_slice(&port);
                                }
                            }
                            t.extend_from_slice(alloc::format!(", {} job(s)", i.jobs).as_bytes());
                            if i.held {
                                t.extend_from_slice(b"  (held)");
                            }
                        }
                        None => t.extend_from_slice(b"This printer's queue does not exist."),
                    }
                    let mut tr = RectL { l: r.l + 6, b: r.t - STATUS, r: r.r - 6, t: r.t - 2 };
                    WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut tr, -1, -2, 0x0400);
                }
                WinEndPaint(hps);
            }
            0
        }
        WM_SIZE => {
            if let Some(v) = view_of_client(h) {
                layout(v);
            }
            unsafe { WinInvalidateRect(h, null(), 0) };
            0
        }
        WM_TIMER => {
            if let Some(v) = view_of_client(h) {
                refresh(v);
            }
            0
        }
        WM_COMMAND => {
            let id = a & 0xffff;
            let Some(v) = view_of_client(h) else { return 0 };
            let q = z(&queue_of(v.obj));
            let sel = send(v.list, LM_QUERYSELECTION, 0xffff, 0);
            let job = v.ids.get(sel as usize).copied();
            unsafe {
                match (id, job) {
                    (ID_HOLD, Some(j)) => {
                        SplHoldJob(null(), q.as_ptr(), j);
                    }
                    (ID_RELEASE, Some(j)) => {
                        SplReleaseJob(null(), q.as_ptr(), j);
                    }
                    (ID_DELETE, Some(j)) => {
                        SplDeleteJob(null(), q.as_ptr(), j);
                    }
                    (ID_HOLDQ, _) => {
                        SplHoldQueue(null(), q.as_ptr());
                    }
                    (ID_RELEASEQ, _) => {
                        SplReleaseQueue(null(), q.as_ptr());
                    }
                    (ID_PURGE, _) => {
                        SplPurgeQueue(null(), q.as_ptr());
                    }
                    _ => {}
                }
            }
            refresh(v);
            0
        }
        WM_CLOSE => {
            if let Some(i) = unsafe { JOB_VIEWS.iter().position(|v| v.client == h) } {
                let v = unsafe { JOB_VIEWS.remove(i) };
                let hab = unsafe { WinQueryAnchorBlock(h) };
                unsafe {
                    WinStopTimer(hab, h, 1);
                    WinDestroyWindow(v.frame);
                }
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

/// The job view: the queue's jobs.
fn open_jobs(o: usize) -> u32 {
    if let Some(v) = unsafe { JOB_VIEWS.iter().find(|v| v.obj == o) } {
        crate::view::activate(v.frame);
        return v.frame;
    }
    let hab = unsafe { WinQueryAnchorBlock(HWND_DESKTOP) };
    // CS_SIZEREDRAW
    unsafe { WinRegisterClass(hab, JOBS_CLASS.as_ptr(), jobs_proc, 0x0004, 0) };
    let mut t = one_line(&obj::title(o));
    t.extend_from_slice(b" - Job Icon View");
    let t = z(&t);
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, JOBS_CLASS.as_ptr(), t.as_ptr(), 0, 0, 1, &mut client) };
    if frame == 0 {
        return 0;
    }
    // WS_VISIBLE | LS_NOADJUSTPOS
    let list = unsafe { WinCreateWindow(client, 0xffff_0007, b"\0".as_ptr(), WS_VISIBLE | 0x4, 0, 0, 10, 10, client, HWND_TOP, ID_LIST, null(), null()) };
    for (id, text) in BUTTONS {
        let t = z(text);
        // WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON
        unsafe { WinCreateWindow(client, 0xffff_0003, t.as_ptr(), WS_VISIBLE | WS_TABSTOP, 0, 0, 10, 10, client, HWND_TOP, id, null(), null()) };
    }
    unsafe { JOB_VIEWS.push(JobView { obj: o, frame, client, list, ids: Vec::new() }) };
    send(frame, 0x0047, obj::call(m::WPObject::wpQueryIcon, &[o]) as u32, 0);
    let (sw, sh) = screen();
    unsafe { WinSetWindowPos(frame, HWND_TOP, sw / 5, sh / 3, 560.min(sw - 40), 240, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER) };
    if let Some(v) = view_of_client(client) {
        layout(v);
        refresh(v);
    }
    unsafe { WinStartTimer(hab, client, 1, 2000) };
    frame
}

/// A printer opens on its jobs (IBM's job icon view) unless its setup
/// says otherwise.
extern "C" fn printer_default_view(o: usize) -> u32 {
    if obj::d(o).default_view == obj::OPEN_DEFAULT {
        return obj::OPEN_CONTENTS;
    }
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpQueryDefaultView, &[o]) as u32
}

extern "C" fn printer_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32;
    }
    open_jobs(o)
}

/// A name for a new queue or printer: letters and digits of `title`
/// (8 at most), made unique with a number.
fn new_name(title: &[u8], exists: impl Fn(&[u8]) -> bool, fallback: &[u8]) -> Vec<u8> {
    let mut base: Vec<u8> = title.iter().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).take(8).collect();
    if base.is_empty() || base[0].is_ascii_digit() {
        base = fallback.to_vec();
    }
    if !exists(&base) {
        return base;
    }
    for n in 1..1000u32 {
        let d = dec(n);
        let mut v = base[..base.len().min(8 - d.len().min(7))].to_vec();
        v.extend_from_slice(&d);
        if !exists(&v) {
            return v;
        }
    }
    base
}

/// Make a queue and a printer (PostScript, FILE port) for an object.
fn create_queue(title: &[u8]) -> Option<Vec<u8>> {
    let qname = new_name(title, |n| queue_info(n).is_some(), b"PRINTER");
    let pname = new_name(&[b"PRT", &qname[..qname.len().min(5)]].concat(), |n| device(n).is_some(), b"PRINTER");
    let drv = b"PSCRIPT.Generic PostScript Printer\0";
    let (qz, pz, descr) = (z(&qname), z(&pname), z(title));
    let mut d = Vec::new();
    for p in [pz.as_ptr() as u32, 0, b"FILE\0".as_ptr() as u32, 0, 0] {
        d.extend_from_slice(&p.to_le_bytes());
    }
    d.truncate(12);
    d.extend_from_slice(&[0, 0, 0, 0]);
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&(descr.as_ptr() as u32).to_le_bytes());
    d.extend_from_slice(&(drv.as_ptr() as u32).to_le_bytes());
    d.extend_from_slice(&[0, 0, 45, 0]);
    if unsafe { SplCreateDevice(null(), 3, d.as_ptr(), d.len() as u32) } != 0 {
        return None;
    }
    let mut q = Vec::new();
    q.extend_from_slice(&(qz.as_ptr() as u32).to_le_bytes());
    for v in [5u16, 0, 0, 0] {
        q.extend_from_slice(&v.to_le_bytes());
    }
    for p in [0u32, b"PMPRINT\0".as_ptr() as u32, 0, descr.as_ptr() as u32] {
        q.extend_from_slice(&p.to_le_bytes());
    }
    q.extend_from_slice(&[0, 0, 0, 0]);
    for p in [pz.as_ptr() as u32, drv.as_ptr() as u32, 0] {
        q.extend_from_slice(&p.to_le_bytes());
    }
    if unsafe { SplCreateQueue(null(), 3, q.as_ptr(), q.len() as u32) } != 0 {
        unsafe { SplDeleteDevice(null(), pz.as_ptr()) };
        return None;
    }
    Some(qname)
}

extern "C" fn printer_setup(o: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpSetup, &[o, setup as usize]);
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        if k == b"QUEUENAME" {
            pd(o).queue = str_new(&v);
            obj::call(m::WPObject::wpSaveDeferred, &[o]);
        }
    }
    1
}

/// A printer made from the template gets a queue of its own (templates.rs
/// calls this for the new object).
pub fn copied_from_template(o: usize) {
    let q = queue_of(o);
    if !q.is_empty() && queue_info(&q).is_some() {
        return;
    }
    let title = one_line(&obj::title(o));
    if let Some(q) = create_queue(&title) {
        pd(o).queue = str_new(&q);
        let id = z(&[b"<RS2_PRINTER_", &q[..], b">"].concat());
        obj::call(m::WPObject::wpSetObjectID, &[o, id.as_ptr() as usize]);
        obj::call(m::WPObject::wpSaveDeferred, &[o]);
        obj::call(m::WPObject::wpViewObject, &[o, 0, OPEN_SETTINGS as usize, 0]);
    }
}

extern "C" fn printer_set_title(o: usize, title: *const u8) -> u32 {
    let r = som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpSetTitle, &[o, title as usize]) as u32;
    // The queue's description follows the title.
    let q = queue_of(o);
    if !q.is_empty() && !obj::restoring(o) {
        let t = z(&one_line(cstr(title)));
        unsafe { SplSetQueue(null(), z(&q).as_ptr(), 3, t.as_ptr(), t.len() as u32, 9) };
    }
    r
}

/// Deleting a printer object deletes its queue (and its printer when no
/// other queue prints there).
extern "C" fn printer_delete(o: usize, fl: u32) -> u32 {
    let q = queue_of(o);
    let template = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & obj::OBJSTYLE_TEMPLATE != 0;
    let r = som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpDelete, &[o, fl as usize]) as u32;
    if r != 0 && !template && !q.is_empty() {
        if let Some(info) = queue_info(&q) {
            unsafe { SplDeleteQueue(null(), z(&q).as_ptr()) };
            for p in info.printers.split(|&c| c == b',').filter(|p| !p.is_empty()) {
                if !queues().iter().any(|x| x.printers.split(|&c| c == b',').any(|y| y == p)) {
                    unsafe { SplDeleteDevice(null(), z(p).as_ptr()) };
                }
            }
        }
    }
    r
}

extern "C" fn printer_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    let q = z(str_bytes(&pd(o).queue));
    obj::wpSaveString(o, b"WPPrinter\0".as_ptr(), 1, q.as_ptr());
    1
}

extern "C" fn printer_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(q) = obj::restored_string(o, b"WPPrinter", 1) {
        pd(o).queue = str_new(q.strip_suffix(&[0]).unwrap_or(&q));
    }
    1
}

extern "C" fn printer_uninit(o: usize) {
    pd(o).queue = str_new(b"");
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[o]);
}

/// wpQueryPrinterName: the queue's name.
extern "C" fn wpQueryPrinterName(o: usize, buf: *mut u8) -> u32 {
    let q = str_bytes(&pd(o).queue);
    if !buf.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(q.as_ptr(), buf, q.len());
            *buf.add(q.len()) = 0;
        }
    }
    1
}

extern "C" fn wpSetPrinterName(o: usize, name: *const u8) -> u32 {
    pd(o).queue = str_new(cstr(name));
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
    1
}

extern "C" fn wpDeleteAllJobs(o: usize) -> u32 {
    let q = queue_of(o);
    (!q.is_empty() && unsafe { SplPurgeQueue(null(), z(&q).as_ptr()) } == 0) as u32
}

/// Files can be dropped to be printed.
extern "C" fn printer_drag_over(o: usize, _cnr: u32, pdinfo: u32) -> u32 {
    if queue_of(o).is_empty() {
        return mp(DOR_NEVERDROP, 0);
    }
    match crate::dnd::dropped_files(pdinfo) {
        Some(v) if !v.is_empty() => mp(DOR_DROP, DO_COPY),
        _ => mp(DOR_NODROPOP, 0),
    }
}

extern "C" fn printer_drop(o: usize, _cnr: u32, _pdinfo: u32, it: u32) -> u32 {
    let f = crate::dnd::item_object(it);
    let path = if f != 0 { wps::path_of(f) } else { None };
    let q = queue_of(o);
    let ok = path.as_deref().is_some_and(|p| print_file(&q, p));
    if !ok {
        message(HWND_DESKTOP, b"The file could not be printed.", &one_line(&obj::title(o)), MB_OK | MB_ERROR);
    }
    crate::delete::RC_DROP_DROPCOMPLETE
}

// ---- The Printer page ----

const ID_QUEUE: u32 = 740;
const ID_DEVICE: u32 = 741;
const ID_DRIVER: u32 = 742;
const ID_JOBPROPS: u32 = 743;
const ID_PORT: u32 = 744;
const ID_SETTINGS: u32 = 745;
const ID_APPLY: u32 = 746;
const ID_DEFAULT: u32 = 747;
const ID_HINT: u32 = 748;

const LM_INSERT: u32 = 0x0161;
const LM_SEARCHSTRING: u32 = 0x016b;

/// The first printer of an object's queue.
fn first_printer(info: &QueueInfo) -> Vec<u8> {
    info.printers.split(|&c| c == b',').find(|p| !p.is_empty()).unwrap_or(b"").to_vec()
}

fn hint(port: &[u8]) -> &'static [u8] {
    let p: Vec<u8> = port.iter().map(|c| c.to_ascii_uppercase()).collect();
    if p.starts_with(b"LPR") {
        b"Settings: host;queue;port;  (port 515 and queue lp if left out)"
    } else if p.starts_with(b"IPP") {
        b"Settings: host;path;port;  (port 631, path /ipp/print if left out)"
    } else if p.starts_with(b"FILE") {
        b"Settings: the output file (or a directory ending in \\); empty: \\SPOOL\\OUTPUT"
    } else if p.starts_with(b"COM") {
        b"Settings: baud;parity;data bits;stop bits;handshake;"
    } else {
        b"The printer's data goes to this parallel port."
    }
}

fn fill_page(h: u32, o: usize) {
    let q = queue_of(o);
    set_item_text(h, ID_QUEUE, if q.is_empty() { b"(none)" } else { &q });
    let Some(info) = queue_info(&q) else {
        set_item_text(h, ID_DEVICE, b"(the queue does not exist)");
        return;
    };
    let p = first_printer(&info);
    set_item_text(h, ID_DEVICE, if p.is_empty() { b"(none)" } else { &p });
    set_item_text(h, ID_DRIVER, &info.driver);
    let port = device(&p).map(|d| d.0).unwrap_or_default();
    let list = unsafe { WinWindowFromID(h, ID_PORT) };
    send(list, LM_DELETEALL, 0, 0);
    for x in ports() {
        let t = z(&x);
        send(list, LM_INSERT, 0xffff, t.as_ptr() as u32);
    }
    let pz = z(&port);
    let at = send(list, LM_SEARCHSTRING, mp(1, 0xffff), pz.as_ptr() as u32);
    if at != 0xffff && at < 0x8000 {
        send(list, LM_SELECTITEM, at, 1);
    }
    let s = port_settings(&port);
    let s = s.strip_suffix(b";").unwrap_or(&s);
    set_item_text(h, ID_SETTINGS, s);
    set_item_text(h, ID_HINT, hint(&port));
    send(unsafe { WinWindowFromID(h, ID_DEFAULT) }, BM_SETCHECK, (info.flags & 4 != 0) as u32, 0);
}

fn selected_port(h: u32) -> Vec<u8> {
    let list = unsafe { WinWindowFromID(h, ID_PORT) };
    let sel = send(list, LM_QUERYSELECTION, 0xffff, 0);
    if sel == 0xffff || sel >= 0x8000 {
        return Vec::new();
    }
    let mut b = [0u8; 64];
    let n = send(list, LM_QUERYITEMTEXT, mp(sel, 64), b.as_mut_ptr() as u32) as usize;
    b[..n.min(64)].to_vec()
}

extern "C" fn printer_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            fill_page(h, b as usize);
            0
        }
        WM_CONTROL if a & 0xffff == ID_PORT && a >> 16 == LN_SELECT => {
            let port = selected_port(h);
            let s = port_settings(&port);
            set_item_text(h, ID_SETTINGS, s.strip_suffix(b";").unwrap_or(&s));
            set_item_text(h, ID_HINT, hint(&port));
            0
        }
        WM_COMMAND => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let q = queue_of(o);
            let qz = z(&q);
            match a & 0xffff {
                ID_JOBPROPS => {
                    if let Some(info) = queue_info(&q) {
                        let drv = info.driver.clone();
                        let dot = drv.iter().position(|&c| c == b'.').unwrap_or(drv.len());
                        let (dn, dev) = (z(&drv[..dot]), z(drv.get(dot + 1..).unwrap_or(b"")));
                        let mut dd = info.drivdata.clone();
                        let need = unsafe { DevPostDeviceModes(0, null_mut(), dn.as_ptr(), dev.as_ptr(), qz.as_ptr(), 0) };
                        if dd.len() < need.max(0) as usize {
                            dd.resize(need as usize, 0);
                        }
                        // DPDM_CHANGEPROP
                        if !dd.is_empty() && unsafe { DevPostDeviceModes(0, dd.as_mut_ptr(), dn.as_ptr(), dev.as_ptr(), qz.as_ptr(), 1) } == 1 {
                            unsafe { SplSetQueue(null(), qz.as_ptr(), 3, dd.as_ptr(), dd.len() as u32, 14) };
                        }
                    }
                }
                ID_APPLY => {
                    let port = selected_port(h);
                    let mut s = [0u8; 256];
                    let n = unsafe { WinQueryDlgItemText(h, ID_SETTINGS, 256, s.as_mut_ptr()) }.max(0) as usize;
                    if let Some(info) = queue_info(&q)
                        && !port.is_empty()
                    {
                        set_port_settings(&port, &s[..n.min(256)]);
                        let p = z(&first_printer(&info));
                        let pz = z(&port);
                        if unsafe { SplSetDevice(null(), p.as_ptr(), 3, pz.as_ptr(), pz.len() as u32, 3) } != 0 {
                            message(h, b"The port could not be set.", b"Printer", MB_OK | MB_ERROR);
                        }
                    }
                }
                ID_DEFAULT => {}
                _ => {}
            }
            0
        }
        WM_CONTROL if a & 0xffff == ID_DEFAULT => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let q = queue_of(o);
            let on = send(unsafe { WinWindowFromID(h, ID_DEFAULT) }, BM_QUERYCHECK, 0, 0) != 0;
            if let Some(info) = queue_info(&q) {
                let fl = if on { info.flags | 4 } else { info.flags & !4 };
                unsafe { SplSetQueue(null(), z(&q).as_ptr(), 3, fl.to_le_bytes().as_ptr(), 2, 10) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn printer_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    // LS_NOADJUSTPOS; SS_TEXT | DT_WORDBREAK; BS_AUTOCHECKBOX
    add_page(o, nb, printer_page_proc, vec![
        book::label(b"Queue:", (6, 108, 50)),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001, (60, 108, 160, 8), ID_QUEUE as u16),
        book::label(b"Printer:", (6, 98, 50)),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001, (60, 98, 160, 8), ID_DEVICE as u16),
        book::label(b"Driver:", (6, 88, 50)),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001, (60, 88, 110, 8), ID_DRIVER as u16),
        item(WC_BUTTON, b"~Job properties...", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (172, 85, 60, 12), ID_JOBPROPS as u16),
        book::label(b"Output port:", (6, 74, 60)),
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x4, (6, 30, 60, 42), ID_PORT as u16),
        book::label(b"Port settings:", (72, 64, 80)),
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x8, (74, 54, 120, 8), ID_SETTINGS as u16),
        item(WC_BUTTON, b"~Set", WS_VISIBLE | WS_TABSTOP, (198, 51, 34, 12), ID_APPLY as u16),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001 | 0x4000, (72, 30, 160, 20), ID_HINT as u16),
        item(WC_BUTTON, b"~Default printer", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (6, 14, 120, 10), ID_DEFAULT as u16),
    ], b"~Printer\0");
    1
}

/// Every queue without a printer object gets one (the shell starts).
pub fn sync_printers() {
    for q in queues() {
        let id = [b"<RS2_PRINTER_", &q.name[..], b">"].concat();
        if wps::handle_of_id(&id).is_some() {
            continue;
        }
        let title = if q.descr.is_empty() { q.name.clone() } else { q.descr.clone() };
        let setup = [b"QUEUENAME=", &q.name[..], b";OBJECTID=", &id[..], b";"].concat();
        crate::api::create_object(b"WPPrinter", &title, &setup, b"<WP_PRINTERSFOLDER>", 0);
    }
}

methods! {
    __m_printer_open => printer_open, 4;
    __m_printer_default_view => printer_default_view, 1;
    __m_printer_setup => printer_setup, 2;
    __m_printer_set_title => printer_set_title, 2;
    __m_printer_delete => printer_delete, 2;
    __m_printer_save_state => printer_save_state, 1;
    __m_printer_restore_state => printer_restore_state, 2;
    __m_printer_uninit => printer_uninit, 1;
    __m_printer_drag_over => printer_drag_over, 3;
    __m_printer_drop => printer_drop, 4;
    __m_printer_add_pages => printer_add_pages, 2;
    __m_wpQueryPrinterName => wpQueryPrinterName, 2;
    __m_wpSetPrinterName => wpSetPrinterName, 2;
    __m_wpDeleteAllJobs => wpDeleteAllJobs, 1;
}

pub fn impl_printer() -> Impl {
    Impl {
        data: core::mem::size_of::<PrinterData>() as u32,
        methods: vec![
            ("wpQueryPrinterName", __m_wpQueryPrinterName as usize),
            ("wpSetPrinterName", __m_wpSetPrinterName as usize),
            ("wpDeleteAllJobs", __m_wpDeleteAllJobs as usize),
        ],
        overrides: vec![
            ("wpOpen", __m_printer_open as usize),
            ("wpQueryDefaultView", __m_printer_default_view as usize),
            ("wpSetup", __m_printer_setup as usize),
            ("wpSetTitle", __m_printer_set_title as usize),
            ("wpDelete", __m_printer_delete as usize),
            ("wpSaveState", __m_printer_save_state as usize),
            ("wpRestoreState", __m_printer_restore_state as usize),
            ("wpUnInitData", __m_printer_uninit as usize),
            ("wpDragOver", __m_printer_drag_over as usize),
            ("wpDrop", __m_printer_drop as usize),
            ("wpAddSettingsPages", __m_printer_add_pages as usize),
        ],
    }
}
