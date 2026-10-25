//! Printer output: PrtOpen/PrtWrite/PrtClose, which presentation drivers
//! write a job's printer data with (rs2's PostScript device in PMWIN does,
//! on OD_DIRECT DCs). The name given to PrtOpen is a printer (device)
//! name or a port name; the data goes to the printer's port:
//!
//! - `FILE`: to the file the port's settings name (`C:\OUT.PS;`), or into
//!   the directory they name (ending in `\`), as `JOBnnnnn.PS`; without
//!   settings to `<spool dir>\OUTPUT\JOBnnnnn.PS`.
//! - `LPT1`-`LPT3`, `COM1`-`COM4`: the kernel's device of that name.
//! - `LPRn`: settings `host;queue;port;` (port 515, queue `lp` by
//!   default): RFC 1179, "receive a printer job" with a control file and
//!   one data file, sent when the job is closed.
//! - `IPPn`: settings `host;path;port;` (port 631, path `/ipp/print` by
//!   default): an IPP/1.1 Print-Job over HTTP/1.1.
//!
//! A job deleted while it prints (SplDeleteJob, SplControlDevice) makes
//! PrtWrite fail, which ends the queue processor's work.

use alloc::vec::Vec;

use crate::ini::{self, Printer};
use crate::jobs::{self, PRJ_QS_PRINTING};
use crate::util::{cat, cstr, dec, eq, fields, mkdir, parse_dec, put, upper, write_all, Table};

const HPRT_BASE: u32 = 0x5052_0001;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_WRITE_FAULT: u32 = 29;
const ERROR_PATH_NOT_FOUND: u32 = 3;
pub const ERROR_PRINT_ABORTED: u32 = 0x8001;

enum Sink {
    /// A file or device handle.
    Handle(u32),
    /// Collected for a network port, sent at close.
    Net { lpr: bool, settings: Vec<u8>, data: Vec<u8> },
}

struct Out {
    sink: Sink,
    job: u32,
    document: Vec<u8>,
    failed: bool,
}

static OUTS: Table<Out> = Table::new();

/// The job a printer is printing (for file names and aborts).
fn printing_job(printer: &[u8]) -> Option<jobs::Job> {
    jobs::all_jobs().into_iter().find(|j| j.qstatus() == PRJ_QS_PRINTING && eq(&j.printer, printer))
}

fn open_handle(name: &[u8]) -> Result<u32, u32> {
    let p = crate::util::z(name);
    let (mut hf, mut act) = (0, 0);
    match unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, os2::OPEN_ACTION_CREATE_IF_NEW | os2::OPEN_ACTION_REPLACE_IF_EXISTS, os2::OPEN_ACCESS_READWRITE | 0x0010 /* deny read/write */, core::ptr::null_mut()) } {
        0 => Ok(hf),
        e => Err(e),
    }
}

/// The file a FILE port writes job `id` to.
fn file_target(settings: &[u8], id: u32) -> Vec<u8> {
    let f = fields(settings);
    let name = f.first().copied().unwrap_or(b"");
    let leaf = cat(&[b"JOB", &dec(id, 5), b".PS"]);
    if name.is_empty() {
        let dir = cat(&[&ini::spool_dir(), b"\\OUTPUT"]);
        mkdir(&ini::spool_dir());
        mkdir(&dir);
        return cat(&[&dir, b"\\", &leaf]);
    }
    if name.ends_with(b"\\") {
        mkdir(&name[..name.len() - 1]);
        return cat(&[name, &leaf]);
    }
    name.to_vec()
}

/// PrtOpen(pszDeviceName, phDevice, pActionTaken, cbFileSize, uFileAttr,
/// openFlag, openMode, reserved)
#[unsafe(no_mangle)]
pub extern "C" fn PrtOpen(dev: *const u8, ph: *mut u32, action: *mut u32, _size: u32, _attr: u32, _flag: u32, _mode: u32, _r: u32) -> u32 {
    let name = upper(cstr(dev));
    let found = jobs::locked(|| {
        let (printer, port) = match Printer::load(&name) {
            Some(p) => (p.name.clone(), p.port.clone()),
            None => (Vec::new(), name.clone()),
        };
        let settings = ini::port_settings(&port)?;
        let job = if printer.is_empty() { None } else { printing_job(&printer) };
        Some((port, settings, job))
    });
    let Some((port, settings, job)) = found else { return ERROR_PATH_NOT_FOUND };
    let id = job.as_ref().map_or(0, |j| j.id);
    let document = job.map(|j| j.document).unwrap_or_default();
    let sink = if port.starts_with(b"LPR") || port.starts_with(b"IPP") {
        Sink::Net { lpr: port.starts_with(b"LPR"), settings, data: Vec::new() }
    } else if port == b"FILE" {
        let id = if id == 0 { jobs::locked(jobs::next_id) } else { id };
        match open_handle(&file_target(&settings, id)) {
            Ok(h) => Sink::Handle(h),
            Err(e) => return e,
        }
    } else {
        match open_handle(&port) {
            Ok(h) => Sink::Handle(h),
            Err(e) => return e,
        }
    };
    put(action, 1);
    put(ph, HPRT_BASE + OUTS.add(Out { sink, job: id, document, failed: false }) as u32);
    0
}

fn with<R>(h: u32, f: impl FnOnce(&mut Out) -> R) -> Option<R> {
    OUTS.with(h.checked_sub(HPRT_BASE)? as usize, f)
}

/// PrtWrite(hDevice, pchData, cbData, pcbWritten)
#[unsafe(no_mangle)]
pub extern "C" fn PrtWrite(h: u32, data: *const u8, cb: u32, written: *mut u32) -> u32 {
    put(written, 0);
    with(h, |o| {
        if jobs::abort_requested(o.job) {
            o.failed = true;
            return ERROR_PRINT_ABORTED;
        }
        let d = if cb == 0 || data.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(data, cb as usize) } };
        match &mut o.sink {
            Sink::Handle(hf) => {
                if !write_all(*hf, d) {
                    o.failed = true;
                    return ERROR_WRITE_FAULT;
                }
            }
            Sink::Net { data, .. } => data.extend_from_slice(d),
        }
        put(written, cb);
        0
    })
    .unwrap_or(ERROR_INVALID_HANDLE)
}

/// PrtClose(hDevice): network ports send the job now.
#[unsafe(no_mangle)]
pub extern "C" fn PrtClose(h: u32) -> u32 {
    let Some(o) = h.checked_sub(HPRT_BASE).and_then(|i| OUTS.take(i as usize)) else { return ERROR_INVALID_HANDLE };
    match o.sink {
        Sink::Handle(hf) => {
            unsafe { os2::DosClose(hf) };
            if o.failed { ERROR_WRITE_FAULT } else { 0 }
        }
        Sink::Net { lpr, settings, data } => {
            if o.failed || jobs::abort_requested(o.job) {
                return ERROR_PRINT_ABORTED;
            }
            let r = if lpr { lpr_send(&settings, o.job, &o.document, &data) } else { ipp_send(&settings, &o.document, &data) };
            match r {
                Ok(()) => 0,
                Err(e) => e,
            }
        }
    }
}

/// PrtAbort(hDevice): drop what has not been sent and close.
#[unsafe(no_mangle)]
pub extern "C" fn PrtAbort(h: u32) -> u32 {
    if let Some(o) = h.checked_sub(HPRT_BASE).and_then(|i| OUTS.take(i as usize))
        && let Sink::Handle(hf) = o.sink
    {
        unsafe { os2::DosClose(hf) };
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn PrtAbortDoc(h: u32, _data: *const u8, _cb: u32, _fl: u32) -> u32 {
    with(h, |o| {
        if let Sink::Net { data, .. } = &mut o.sink {
            data.clear();
        }
        o.failed = true;
        0
    })
    .unwrap_or(ERROR_INVALID_HANDLE)
}

#[unsafe(no_mangle)]
pub extern "C" fn PrtResetAbort(h: u32) -> u32 {
    with(h, |o| {
        o.failed = false;
        0
    })
    .unwrap_or(ERROR_INVALID_HANDLE)
}

#[unsafe(no_mangle)]
pub extern "C" fn PrtNewPage(h: u32, _page: u32) -> u32 {
    with(h, |_| 0).unwrap_or(ERROR_INVALID_HANDLE)
}

/// PrtDevIOCtl(pData, pParms, uFunction, uCategory, hDevice): handles
/// to devices pass it on; others have nothing to report.
#[unsafe(no_mangle)]
pub extern "C" fn PrtDevIOCtl(data: *mut u8, parms: *mut u8, func: u32, cat_: u32, h: u32) -> u32 {
    with(h, |o| match o.sink {
        Sink::Handle(hf) => {
            let (mut pl, mut dl) = (0u32, 0u32);
            unsafe { os2::DosDevIOCtl(hf, cat_, func, parms, 0, &mut pl, data, 0, &mut dl) }
        }
        _ => 0,
    })
    .unwrap_or(ERROR_INVALID_HANDLE)
}

// ---- Network printers ----

fn sock_error(e: u32) -> u32 {
    if e == 0 { ERROR_WRITE_FAULT } else { 0x10000 | e }
}

struct Sock(u32);

impl Drop for Sock {
    fn drop(&mut self) {
        unsafe { os2::net::Rs2SockClose(self.0) };
    }
}

fn connect(host: &[u8], port: u16) -> Result<Sock, u32> {
    let h = core::str::from_utf8(host).map_err(|_| ERROR_PATH_NOT_FOUND)?;
    let addr = match os2::net::parse_ip(h) {
        Some(a) => a,
        None => *os2::net::resolve(h).map_err(sock_error)?.first().ok_or(ERROR_PATH_NOT_FOUND)?,
    };
    let mut s = 0;
    let rc = unsafe { os2::net::Rs2SockCreate(2, 1, 0, &mut s) };
    if rc != 0 {
        return Err(sock_error(rc));
    }
    let s = Sock(s);
    // Receive timeout: 30 seconds.
    let tv = [30u32, 0];
    unsafe { os2::net::Rs2SockSetOpt(s.0, 0xffff, 0x1006, tv.as_ptr() as *const u8, 8) };
    let sa = os2::net::SockAddrIn::new(addr, port);
    let rc = unsafe { os2::net::Rs2SockConnect(s.0, &sa, core::mem::size_of::<os2::net::SockAddrIn>() as u32) };
    if rc != 0 {
        return Err(sock_error(rc));
    }
    Ok(s)
}

fn send(s: &Sock, d: &[u8]) -> Result<(), u32> {
    let mut off = 0;
    while off < d.len() {
        let mut n = 0;
        let rc = unsafe { os2::net::Rs2SockSend(s.0, d[off..].as_ptr(), (d.len() - off).min(32768) as u32, 0, &mut n) };
        if rc != 0 || n == 0 {
            return Err(sock_error(rc));
        }
        off += n as usize;
    }
    Ok(())
}

fn recv(s: &Sock, buf: &mut [u8]) -> Result<usize, u32> {
    let mut n = 0;
    match unsafe { os2::net::Rs2SockRecv(s.0, buf.as_mut_ptr(), buf.len() as u32, 0, &mut n) } {
        0 => Ok(n as usize),
        e => Err(sock_error(e)),
    }
}

/// One LPD acknowledgement byte (0 = accepted).
fn ack(s: &Sock) -> Result<(), u32> {
    let mut b = [1u8];
    if recv(s, &mut b)? == 1 && b[0] == 0 { Ok(()) } else { Err(ERROR_WRITE_FAULT) }
}

fn our_host() -> Vec<u8> {
    let h = os2::net::hostname();
    let h: Vec<u8> = h.bytes().filter(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'.').collect();
    if h.is_empty() { b"rs2".to_vec() } else { h }
}

/// Send a job to an LPD (RFC 1179).
pub fn lpr_send(settings: &[u8], id: u32, document: &[u8], data: &[u8]) -> Result<(), u32> {
    let f = fields(settings);
    let host = f.first().copied().unwrap_or(b"");
    if host.is_empty() {
        return Err(ERROR_PATH_NOT_FOUND);
    }
    let queue = f.get(1).copied().filter(|q| !q.is_empty()).unwrap_or(b"lp");
    let port = f.get(2).and_then(|p| parse_dec(p)).unwrap_or(515) as u16;
    let s = connect(host, port)?;
    let me = our_host();
    let num = dec(id % 1000, 3);
    let name = if document.is_empty() { &b"rs2 job"[..] } else { document };
    let dfile = cat(&[b"dfA", &num, &me]);
    let cfile = cat(&[b"cfA", &num, &me]);
    // PostScript gets the PostScript format code; anything else prints
    // as is.
    let fmt: &[u8] = if data.starts_with(b"%!") { b"o" } else { b"l" };
    let control = cat(&[b"H", &me, b"\nPrs2\nJ", name, b"\nN", name, b"\n", fmt, &dfile, b"\nU", &dfile, b"\n"]);
    send(&s, &cat(&[b"\x02", queue, b"\n"]))?;
    ack(&s)?;
    send(&s, &cat(&[b"\x02", &dec(control.len() as u32, 1), b" ", &cfile, b"\n"]))?;
    ack(&s)?;
    send(&s, &control)?;
    send(&s, b"\0")?;
    ack(&s)?;
    send(&s, &cat(&[b"\x03", &dec(data.len() as u32, 1), b" ", &dfile, b"\n"]))?;
    ack(&s)?;
    send(&s, data)?;
    send(&s, b"\0")?;
    ack(&s)
}

fn ipp_attr(v: &mut Vec<u8>, tag: u8, name: &[u8], value: &[u8]) {
    v.push(tag);
    v.extend_from_slice(&(name.len() as u16).to_be_bytes());
    v.extend_from_slice(name);
    v.extend_from_slice(&(value.len() as u16).to_be_bytes());
    v.extend_from_slice(value);
}

/// Send a job to an IPP printer (Print-Job, RFC 8011) over HTTP/1.1.
pub fn ipp_send(settings: &[u8], document: &[u8], data: &[u8]) -> Result<(), u32> {
    let f = fields(settings);
    let host = f.first().copied().unwrap_or(b"");
    if host.is_empty() {
        return Err(ERROR_PATH_NOT_FOUND);
    }
    let path = f.get(1).copied().filter(|q| !q.is_empty()).unwrap_or(b"/ipp/print");
    let port = f.get(2).and_then(|p| parse_dec(p)).unwrap_or(631) as u16;
    let uri = cat(&[b"ipp://", host, b":", &dec(port as u32, 1), path]);
    let name = if document.is_empty() { &b"rs2 job"[..] } else { document };
    let format: &[u8] = if data.starts_with(b"%!") { b"application/postscript" } else { b"application/octet-stream" };
    let mut body = Vec::new();
    body.extend_from_slice(&[1, 1, 0, 2, 0, 0, 0, 1, 0x01]);
    ipp_attr(&mut body, 0x47, b"attributes-charset", b"utf-8");
    ipp_attr(&mut body, 0x48, b"attributes-natural-language", b"en");
    ipp_attr(&mut body, 0x45, b"printer-uri", &uri);
    ipp_attr(&mut body, 0x42, b"requesting-user-name", b"rs2");
    ipp_attr(&mut body, 0x42, b"job-name", name);
    ipp_attr(&mut body, 0x49, b"document-format", format);
    body.push(0x03);
    let head = cat(&[b"POST ", path, b" HTTP/1.1\r\nHost: ", host, b":", &dec(port as u32, 1), b"\r\nContent-Type: application/ipp\r\nContent-Length: ", &dec((body.len() + data.len()) as u32, 1), b"\r\nConnection: close\r\n\r\n"]);
    let s = connect(host, port)?;
    send(&s, &head)?;
    send(&s, &body)?;
    send(&s, data)?;
    // The status line and the IPP status code.
    let mut resp = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = recv(&s, &mut buf)?;
        if n == 0 {
            break;
        }
        resp.extend_from_slice(&buf[..n]);
        if resp.len() > 16384 {
            break;
        }
        if let Some(i) = resp.windows(4).position(|w| w == b"\r\n\r\n")
            && resp.len() >= i + 4 + 4
        {
            break;
        }
    }
    let ok_http = resp.starts_with(b"HTTP/1.1 200") || resp.starts_with(b"HTTP/1.0 200");
    let ipp_ok = resp.windows(4).position(|w| w == b"\r\n\r\n").and_then(|i| resp.get(i + 4 + 2..i + 4 + 4)).is_some_and(|st| u16::from_be_bytes([st[0], st[1]]) < 0x0100);
    if ok_http && ipp_ok { Ok(()) } else { Err(ERROR_WRITE_FAULT) }
}
