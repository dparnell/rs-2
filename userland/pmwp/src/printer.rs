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

use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
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
    pub fn SplSetQueue(comp: *const u8, q: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    #[link_name = "PMSPL.396"]
    fn SplDeleteQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.394"]
    fn SplHoldQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.395"]
    fn SplReleaseQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.404"]
    fn SplPurgeQueue(comp: *const u8, q: *const u8) -> u32;
    #[link_name = "PMSPL.401"]
    fn SplCreateDevice(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32;
    #[link_name = "PMSPL.402"]
    fn SplSetDevice(comp: *const u8, dev: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    #[link_name = "PMSPL.381"]
    fn SplQueryDevice(comp: *const u8, dev: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.403"]
    fn SplDeleteDevice(comp: *const u8, dev: *const u8) -> u32;
    #[link_name = "PMSPL.413"]
    fn SplCreatePort(comp: *const u8, port: *const u8, drv: *const u8, ver: u32, buf: *const u8, cb: u32) -> u32;
    #[link_name = "PMGPI.607"]
    fn DevQueryDeviceNames(hab: u32, drv: *const u8, n: *mut i32, names: *mut u8, descs: *mut u8, ntypes: *mut i32, types: *mut u8) -> u32;
    #[link_name = "PMWIN.841"]
    fn WinQueryWindowText(h: u32, cb: i32, buf: *mut u8) -> i32;
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
    pub priority: u16,
    pub qproc: Vec<u8>,
    /// The separator file and the print times (minutes after midnight).
    pub sepfile: Vec<u8>,
    pub start: u16,
    pub until: u16,
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
        priority: rd16(b, o + 4),
        qproc: ptr_str(rd32(b, o + 16)),
        sepfile: ptr_str(rd32(b, o + 12)),
        start: rd16(b, o + 6),
        until: rd16(b, o + 8),
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

pub fn port_settings(port: &[u8]) -> Vec<u8> {
    let mut buf = [0u8; 512];
    let n = unsafe { PrfQueryProfileString(HINI_SYSTEM, b"PM_SPOOLER_PORT\0".as_ptr(), z(port).as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let mut v = buf[..n.min(buf.len())].to_vec();
    while v.last() == Some(&0) {
        v.pop();
    }
    v
}

pub fn set_port_settings(port: &[u8], s: &[u8]) {
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
    /// The View page: the job view in details (1) or icons (0), its
    /// refresh interval in seconds, the details columns hidden (bits as
    /// printobj::JOB_COLUMNS).
    details: u32,
    refresh: u32,
    hidden: u32,
    /// A network printer's job view (wpSetRemoteOptions): only the jobs
    /// of the user logged on (1) or all (0).
    own_jobs: u32,
}

/// The job view's settings (the View page): details?, refresh seconds,
/// hidden columns.
pub fn view_settings(o: usize) -> (bool, u32, u32) {
    let d = pd(o);
    (d.details != 0, if d.refresh == 0 { DEFAULT_REFRESH } else { d.refresh }, d.hidden)
}

pub fn set_view_settings(o: usize, details: bool, refresh: u32, hidden: u32) {
    let d = pd(o);
    d.details = details as u32;
    d.refresh = refresh.clamp(1, 3600);
    d.hidden = hidden;
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
}

/// The details columns a printer's job view leaves out.
pub fn hidden_columns(o: usize) -> u32 {
    pd(o).hidden
}

/// The job view's refresh interval unless the View page sets one.
pub const DEFAULT_REFRESH: u32 = 2;

fn pd(o: usize) -> &'static mut PrinterData {
    som::data(wps::ccd("WPPrinter"), o)
}

fn queue_of(o: usize) -> Vec<u8> {
    str_bytes(&pd(o).queue).to_vec()
}

/// The printer's queue (a network printer's local alias).
pub fn queue_name(o: usize) -> Vec<u8> {
    queue_of(o)
}

pub fn set_queue_name(o: usize, q: &[u8]) {
    pd(o).queue = str_new(q);
}

/// The user whose jobs a network printer's "own jobs" view shows: the
/// one logged on to the network (jobs say LOCAL when nobody is).
fn own_user() -> Vec<u8> {
    let u = lanreq::status().user;
    if u.is_empty() { b"LOCAL".to_vec() } else { u.bytes().collect() }
}

/// wpSetRemoteOptions(ulRefreshInterval, flAllJobs): a network printer's
/// job view.
extern "C" fn wpSetRemoteOptions(o: usize, refresh: u32, all: u32) -> u32 {
    let d = pd(o);
    if refresh != 0 {
        d.refresh = refresh.clamp(1, 3600);
    }
    d.own_jobs = (all == 0) as u32;
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
    jobs_changed(o);
    1
}

extern "C" fn wpQueryRemoteOptions(o: usize, refresh: *mut u32, all: *mut u32) -> u32 {
    let (_, r, _) = view_settings(o);
    unsafe {
        if !refresh.is_null() {
            *refresh = r;
        }
        if !all.is_null() {
            *all = (pd(o).own_jobs == 0) as u32;
        }
    }
    1
}

fn one_line(t: &[u8]) -> Vec<u8> {
    t.iter().map(|&c| if c == b'\n' { b' ' } else { c }).collect()
}

/// Open job views (printer, frame, when last brought up to date in ms)
/// and the printers' job objects (printer, job id, object, the title it
/// has).
static mut JOB_VIEWS: Vec<(usize, u32, u32)> = Vec::new();
static mut JOB_OBJS: Vec<(usize, u32, usize, Vec<u8>)> = Vec::new();
/// The desktop's timer for the job views (every second; each view is
/// brought up to date at its printer's refresh interval).
pub const TID_JOBS: u32 = 0x5713;

fn ms_now() -> u32 {
    let mut ms = 0u32;
    // QSV_MS_COUNT
    unsafe { os2::DosQuerySysInfo(14, 14, &mut ms, 4) };
    ms
}

fn dec(n: u32) -> Vec<u8> {
    alloc::format!("{n}").into_bytes()
}

/// The view's title: the printer's, and IBM's status line ("Status:
/// Queue held  Job(s) 2").
fn view_title(o: usize) -> Vec<u8> {
    let mut t = one_line(&obj::title(o));
    match queue_info(&queue_of(o)) {
        Some(i) => {
            t.extend_from_slice(b" (");
            if i.held {
                t.extend_from_slice(b"Queue held, ");
            }
            t.extend_from_slice(&dec(i.jobs as u32));
            t.extend_from_slice(b" Job(s))");
        }
        None => t.extend_from_slice(b" (no queue)"),
    }
    t
}

/// The printer's job objects brought up to date with its queue's jobs:
/// (objects, whether some came or went).
fn job_objects(o: usize) -> (Vec<usize>, bool) {
    let q = queue_of(o);
    let mut js = crate::printobj::jobs(&q);
    if pd(o).own_jobs != 0 && wps::is(o, "WPRPrinter") {
        let me = own_user();
        js.retain(|j| j.user.eq_ignore_ascii_case(&me));
    }
    let objs = unsafe { &mut JOB_OBJS };
    let mut changed = false;
    // Jobs that are gone.
    let gone: Vec<usize> = objs.iter().filter(|e| e.0 == o && !js.iter().any(|j| j.id == e.1)).map(|e| e.2).collect();
    if !gone.is_empty() {
        changed = true;
        objs.retain(|e| !gone.contains(&e.2));
    }
    let mut list = Vec::new();
    for j in &js {
        let mut title = crate::printobj::job_title(j);
        if j.status & 3 != 0 {
            title.push(b'\n');
            title.extend_from_slice(crate::printobj::job_status(j.status));
        }
        match objs.iter_mut().find(|e| e.0 == o && e.1 == j.id) {
            Some(e) => {
                if e.3 != title {
                    let t = z(&title);
                    obj::call(m::WPObject::wpSetTitle, &[e.2, t.as_ptr() as usize]);
                    e.3 = title;
                }
                list.push(e.2);
            }
            None => {
                let x = crate::printobj::make_job(o, &q, j);
                if x != 0 {
                    if title != crate::printobj::job_title(j) {
                        let t = z(&title);
                        obj::call(m::WPObject::wpSetTitle, &[x, t.as_ptr() as usize]);
                    }
                    objs.push((o, j.id, x, title));
                    list.push(x);
                    changed = true;
                }
            }
        }
    }
    // Objects of gone jobs leave the view before they are freed.
    if !gone.is_empty() {
        if let Some(w) = view_of(o) {
            crate::view::set_results(w, &view_title(o), list.clone());
        }
        for g in gone {
            crate::printobj::free_job(g);
        }
        changed = false;
    }
    (list, changed)
}

fn view_of(o: usize) -> Option<usize> {
    let frame = unsafe { JOB_VIEWS.iter().find(|v| v.0 == o) }?.1;
    crate::view::wins().iter().position(|w| w.frame == frame)
}

/// A job changed (held, deleted...): its view follows at once.
pub fn jobs_changed(o: usize) {
    if o != 0 && view_of(o).is_some() {
        refresh(o);
    }
}

fn refresh(o: usize) {
    let Some(w) = view_of(o) else { return };
    let (objs, changed) = job_objects(o);
    let title = view_title(o);
    let mut cur = [0u8; 256];
    let n = unsafe { WinQueryWindowText(crate::view::wins()[w].frame, 256, cur.as_mut_ptr()) }.max(0) as usize;
    if changed || !cur[..n].starts_with(&title) {
        crate::view::set_results(w, &title, objs);
    }
}

/// The desktop's timer: the open job views follow their queues; the
/// objects of closed views go.
pub fn tick() {
    let views: Vec<(usize, u32, u32)> = unsafe { JOB_VIEWS.clone() };
    let now = ms_now();
    for (o, frame, last) in views {
        if crate::view::wins().iter().any(|w| w.frame == frame) {
            if now.wrapping_sub(last) + 500 >= view_settings(o).1 * 1000 {
                unsafe {
                    if let Some(v) = JOB_VIEWS.iter_mut().find(|v| v.1 == frame) {
                        v.2 = now;
                    }
                }
                refresh(o);
            }
        } else {
            unsafe { JOB_VIEWS.retain(|v| v.1 != frame) };
            let mine: Vec<usize> = unsafe { JOB_OBJS.iter().filter(|e| e.0 == o).map(|e| e.2).collect() };
            unsafe { JOB_OBJS.retain(|e| e.0 != o) };
            for x in mine {
                crate::printobj::free_job(x);
            }
        }
    }
    if unsafe { JOB_VIEWS.is_empty() } {
        let client = crate::view::desktop_client();
        unsafe { WinStopTimer(WinQueryAnchorBlock(HWND_DESKTOP), client, TID_JOBS) };
    }
}

/// The job view: the queue's jobs as job objects (IBM's job icon view),
/// in a view window whose own pop-up menu is the printer's.
fn open_jobs(o: usize) -> u32 {
    if let Some(w) = view_of(o) {
        let f = crate::view::wins()[w].frame;
        crate::view::activate(f);
        return f;
    }
    let (objs, _) = job_objects(o);
    let view = if view_settings(o).0 { obj::OPEN_DETAILS } else { obj::OPEN_CONTENTS };
    let w = crate::view::open_owned(&view_title(o), objs, o, view);
    let frame = crate::view::wins()[w].frame;
    unsafe { JOB_VIEWS.push((o, frame, ms_now())) };
    let client = crate::view::desktop_client();
    if client != 0 {
        unsafe { WinStartTimer(WinQueryAnchorBlock(HWND_DESKTOP), client, TID_JOBS, 1000) };
    }
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
    if wps::is(o, "WPRPrinter") {
        crate::rprinter::copied_from_template(o);
        return;
    }
    let q = queue_of(o);
    if !q.is_empty() && queue_info(&q).is_some() {
        return;
    }
    let title = one_line(&obj::title(o));
    // IBM's "Create a printer": the name, the driver and the port;
    // cancelled, the new object goes again.
    let Some((title, drv, port)) = crate::printobj::create_dialog(&title) else {
        obj::call(m::WPObject::wpDelete, &[o, 0]);
        return;
    };
    if let Some(q) = create_queue(&title) {
        pd(o).queue = str_new(&q);
        set_driver(&q, &drv);
        set_port(&q, &port);
        let t = z(&title);
        obj::call(m::WPObject::wpSetTitle, &[o, t.as_ptr() as usize]);
        let id = z(&[b"<RS2_PRINTER_", &q[..], b">"].concat());
        obj::call(m::WPObject::wpSetObjectID, &[o, id.as_ptr() as usize]);
        obj::call(m::WPObject::wpSaveDeferred, &[o]);
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
    let d = pd(o);
    obj::wpSaveLong(o, b"WPPrinter\0".as_ptr(), 2, d.details);
    obj::wpSaveLong(o, b"WPPrinter\0".as_ptr(), 3, d.refresh);
    obj::wpSaveLong(o, b"WPPrinter\0".as_ptr(), 4, d.hidden);
    obj::wpSaveLong(o, b"WPPrinter\0".as_ptr(), 5, d.own_jobs);
    1
}

extern "C" fn printer_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(q) = obj::restored_string(o, b"WPPrinter", 1) {
        pd(o).queue = str_new(q.strip_suffix(&[0]).unwrap_or(&q));
    }
    let d = pd(o);
    d.details = obj::restored_long(o, b"WPPrinter", 2).unwrap_or(0);
    d.refresh = obj::restored_long(o, b"WPPrinter", 3).unwrap_or(0);
    d.hidden = obj::restored_long(o, b"WPPrinter", 4).unwrap_or(0);
    d.own_jobs = obj::restored_long(o, b"WPPrinter", 5).unwrap_or(0);
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
    // Jobs of other printers move here (IBM's wpMoveJobObject).
    let items = crate::dnd::items(pdinfo);
    let jobs: Vec<usize> = items.iter().map(|&it| crate::dnd::item_object(it)).filter(|&j| j != 0 && wps::is(j, "WPJob")).collect();
    if !jobs.is_empty() && jobs.len() == items.len() {
        let q = queue_of(o);
        return if jobs.iter().all(|&j| crate::printobj::job_queue(j).is_some_and(|jq| !jq.eq_ignore_ascii_case(&q))) { mp(DOR_DROP, DO_MOVE) } else { mp(DOR_NODROP, DO_MOVE) };
    }
    match crate::dnd::dropped_files(pdinfo) {
        Some(v) if !v.is_empty() => mp(DOR_DROP, DO_COPY),
        _ => mp(DOR_NODROPOP, 0),
    }
}

extern "C" fn printer_drop(o: usize, _cnr: u32, _pdinfo: u32, it: u32) -> u32 {
    let f = crate::dnd::item_object(it);
    if f != 0 && wps::is(f, "WPJob") {
        if !crate::printobj::move_job(f, o, &queue_of(o)) {
            message(HWND_DESKTOP, b"The job could not be moved to this printer.", &one_line(&obj::title(o)), MB_OK | MB_ERROR);
        }
        return crate::delete::RC_DROP_DROPCOMPLETE;
    }
    let path = if f != 0 { wps::path_of(f) } else { None };
    let q = queue_of(o);
    let ok = path.as_deref().is_some_and(|p| print_file(&q, p));
    if !ok {
        message(HWND_DESKTOP, b"The file could not be printed.", &one_line(&obj::title(o)), MB_OK | MB_ERROR);
    }
    crate::delete::RC_DROP_DROPCOMPLETE
}

// ---- What the printer's pages and objects change ----

/// The first printer of an object's queue.
fn first_printer(info: &QueueInfo) -> Vec<u8> {
    info.printers.split(|&c| c == b',').find(|p| !p.is_empty()).unwrap_or(b"").to_vec()
}

/// The spooler's ports.
pub fn port_names() -> Vec<Vec<u8>> {
    ports()
}

/// A new port (LPRn, IPPn, FILEn).
pub fn create_port(name: &[u8]) -> bool {
    let n = z(name);
    unsafe { SplCreatePort(null(), n.as_ptr(), b"\0".as_ptr(), 0, null(), 0) == 0 }
}

/// The port the queue's printer prints on.
pub fn port_of(queue: &[u8]) -> Vec<u8> {
    queue_info(queue).and_then(|i| device(&first_printer(&i))).map(|d| d.0).unwrap_or_default()
}

pub fn set_port(queue: &[u8], port: &[u8]) -> bool {
    let Some(info) = queue_info(queue) else { return false };
    let (p, pz) = (z(&first_printer(&info)), z(port));
    let ok = unsafe { SplSetDevice(null(), p.as_ptr(), 3, pz.as_ptr(), pz.len() as u32, 3) } == 0;
    if !ok {
        message(HWND_DESKTOP, b"The port could not be set.", b"Printer", MB_OK | MB_ERROR);
    }
    ok
}

/// The queue's printer driver (DRIVER.Device).
pub fn set_driver(queue: &[u8], drv: &[u8]) {
    let (q, d) = (z(queue), z(drv));
    unsafe { SplSetQueue(null(), q.as_ptr(), 3, d.as_ptr(), d.len() as u32, 13) };
}

/// The default printer (PRQ3_TYPE_APPDEFAULT: the queue programs print
/// to unless they choose).
pub fn set_default(queue: &[u8]) {
    if let Some(info) = queue_info(queue) {
        let fl = info.flags | 4;
        unsafe { SplSetQueue(null(), z(queue).as_ptr(), 3, fl.to_le_bytes().as_ptr(), 2, 10) };
    }
}

/// The queue's priority and PRQINFO3 fsType.
pub fn queue_priority_flags(queue: &[u8]) -> (u16, u16) {
    queue_info(queue).map_or((5, 0), |i| (i.priority.clamp(1, 9), i.flags))
}

pub fn set_queue_priority(queue: &[u8], p: u16) {
    unsafe { SplSetQueue(null(), z(queue).as_ptr(), 3, p.to_le_bytes().as_ptr(), 2, 2) };
}

pub fn set_queue_flags(queue: &[u8], f: u16) {
    unsafe { SplSetQueue(null(), z(queue).as_ptr(), 3, f.to_le_bytes().as_ptr(), 2, 10) };
}

/// The queue's queue driver (processor).
pub fn queue_driver(queue: &[u8]) -> Vec<u8> {
    queue_info(queue).map(|i| i.qproc).filter(|q| !q.is_empty()).unwrap_or_else(|| b"PMPRINT".to_vec())
}

pub fn set_queue_driver(queue: &[u8], qp: &[u8]) {
    let (q, p) = (z(queue), z(qp));
    unsafe { SplSetQueue(null(), q.as_ptr(), 3, p.as_ptr(), p.len() as u32, 6) };
}

/// A presentation driver's devices (DevQueryDeviceNames).
pub fn driver_devices(drv: &[u8]) -> Vec<Vec<u8>> {
    let d = z(drv);
    let (mut n, mut types) = (16i32, 0i32);
    let mut names = alloc::vec![0u8; 32 * 16];
    let mut descs = alloc::vec![0u8; 64 * 16];
    let ok = unsafe { DevQueryDeviceNames(WinQueryAnchorBlock(HWND_DESKTOP), d.as_ptr(), &mut n, names.as_mut_ptr(), descs.as_mut_ptr(), &mut types, null_mut()) } != 0;
    if !ok || n <= 0 {
        return alloc::vec![b"Generic PostScript Printer".to_vec()];
    }
    (0..n.min(16) as usize).map(|i| names[i * 32..i * 32 + 32].iter().copied().take_while(|&c| c != 0).collect()).collect()
}

/// The driver's job properties dialog for the queue (DevPostDeviceModes,
/// kept with the queue).
pub fn job_properties(o: usize) {
    let q = queue_of(o);
    let qz = z(&q);
    let Some(info) = queue_info(&q) else { return };
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

/// IBM's printer pages: View, Printer driver, Output port, Queue options,
/// Print options (in that order of tabs: each goes in front of the ones
/// before it).
extern "C" fn printer_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    crate::printobj::add_print_options_page(o, nb);
    crate::printobj::wpAddQueueOptionsPage(o, nb);
    crate::printobj::wpAddPrinterPortPage(o, nb);
    crate::printobj::wpAddPrinterDriverPage(o, nb);
    crate::printobj::add_printer_view_page(o, nb);
    1
}

// The printer's pop-up menu: hold or release the queue, delete all its
// jobs, make it the default printer.
const ID_MHOLD: u32 = 0x7a01;
const ID_MRELEASE: u32 = 0x7a02;
const ID_MPURGE: u32 = 0x7a03;
const ID_MDEFAULT: u32 = 0x7a04;

#[repr(C)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    item: u32,
}

extern "C" fn printer_modify_menu(o: usize, menu: u32, _cnr: u32, _pos: u32) -> u32 {
    let Some(info) = queue_info(&queue_of(o)) else { return 1 };
    let add = |id: u32, t: &[u8]| {
        let mi = MenuItem { pos: -1, style: 0x0001, attr: 0, id: id as u16, sub: 0, item: 0 };
        let t = z(t);
        send(menu, 0x0180, &mi as *const MenuItem as u32, t.as_ptr() as u32);
    };
    if info.held {
        add(ID_MRELEASE, b"~Release");
    } else {
        add(ID_MHOLD, b"~Hold");
    }
    add(ID_MPURGE, b"Delete ~all jobs...");
    if info.flags & 4 == 0 {
        add(ID_MDEFAULT, b"Set ~default");
    }
    1
}

extern "C" fn printer_menu_selected(o: usize, frame: u32, id: u32) -> u32 {
    let q = z(&queue_of(o));
    match id {
        ID_MHOLD => unsafe {
            SplHoldQueue(null(), q.as_ptr());
        },
        ID_MRELEASE => unsafe {
            SplReleaseQueue(null(), q.as_ptr());
        },
        ID_MPURGE => {
            if message(HWND_DESKTOP, b"Delete all jobs of this printer?", &one_line(&obj::title(o)), MB_YESNO | MB_WARNING) == MBID_YES {
                unsafe { SplPurgeQueue(null(), q.as_ptr()) };
            }
        }
        ID_MDEFAULT => set_default(&queue_of(o)),
        _ => return som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpMenuItemSelected, &[o, frame as usize, id as usize]) as u32,
    }
    jobs_changed(o);
    1
}

/// Every queue without a printer object gets one (the shell starts);
/// network printers' local aliases belong to their network printer
/// objects (rprinter.rs). The Printers folder gets its network printer
/// template.
pub fn sync_printers() {
    let remote: Vec<Vec<u8>> = crate::rprinter::remote_queues().into_iter().map(|q| q.0).collect();
    crate::rprinter::ensure_template();
    for q in queues() {
        if remote.iter().any(|r| r.eq_ignore_ascii_case(&q.name)) {
            continue;
        }
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
    __m_printer_modify_menu => printer_modify_menu, 4;
    __m_printer_menu_selected => printer_menu_selected, 3;
    __m_wpQueryPrinterName => wpQueryPrinterName, 2;
    __m_wpSetPrinterName => wpSetPrinterName, 2;
    __m_wpDeleteAllJobs => wpDeleteAllJobs, 1;
    __m_wpSetRemoteOptions => wpSetRemoteOptions, 3;
    __m_wpQueryRemoteOptions => wpQueryRemoteOptions, 3;
}

pub fn impl_printer() -> Impl {
    Impl {
        data: core::mem::size_of::<PrinterData>() as u32,
        methods: vec![
            ("wpQueryPrinterName", __m_wpQueryPrinterName as usize),
            ("wpSetPrinterName", __m_wpSetPrinterName as usize),
            ("wpDeleteAllJobs", __m_wpDeleteAllJobs as usize),
            ("wpSetRemoteOptions", __m_wpSetRemoteOptions as usize),
            ("wpQueryRemoteOptions", __m_wpQueryRemoteOptions as usize),
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
            ("wpModifyPopupMenu", __m_printer_modify_menu as usize),
            ("wpMenuItemSelected", __m_printer_menu_selected as usize),
        ],
    }
}
