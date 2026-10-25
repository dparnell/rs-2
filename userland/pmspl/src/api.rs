//! The spooler's query and control calls: queues, jobs, printers
//! (devices), ports, drivers and queue processors.
//!
//! Results are packed as IBM's PMSPL packs them: the fixed structures
//! (PRQINFO3, PRJINFO2...) from the start of the caller's buffer, their
//! strings from its end. Enumerations return as many whole entries as
//! fit (ERROR_MORE_DATA when not all do); queries need room for the whole
//! answer (NERR_BufTooSmall). The computer name must be NULL or empty:
//! there are no remote queues.

use alloc::vec::Vec;

use crate::ini::{self, Printer, Queue};
use crate::jobs::{self, Job, PRJ_DELETED, PRJ_QS_PRINTING};
use crate::util::{cstr, enum_result, eq, join, list, opt, put, query_result, upper, Rec};

pub const NO_ERROR: u32 = 0;
pub const ERROR_INVALID_PARAMETER: u32 = 87;
pub const ERROR_INVALID_LEVEL: u32 = 124;
pub const NERR_QNOTFOUND: u32 = 2150;
pub const NERR_JOBNOTFOUND: u32 = 2151;
pub const NERR_DESTNOTFOUND: u32 = 2152;
pub const NERR_DESTEXISTS: u32 = 2153;
pub const NERR_QEXISTS: u32 = 2154;
pub const NERR_DESTINVALIDOP: u32 = 2159;
pub const NERR_DESTINVALIDSTATE: u32 = 2162;
pub const NERR_QINVALIDSTATE: u32 = 2163;
pub const NERR_JOBINVALIDSTATE: u32 = 2164;
pub const NERR_DRIVERNOTFOUND: u32 = 2166;
pub const NERR_INVALIDCOMPUTER: u32 = 2351;
pub const NERR_BADDEV: u32 = 2341;

fn local(comp: *const u8) -> bool {
    cstr(comp).is_empty()
}

// ---- Records ----

fn prq3(q: &Queue, jobs: &[Job], six: bool) -> Rec {
    let printers: Vec<Vec<u8>> = q.printers().into_iter().map(|p| p.name).collect();
    let mut r = Rec::new();
    r.str(&q.name)
        .u16(q.priority)
        .u16(q.start)
        .u16(q.until)
        .u16(q.flags)
        .str(&q.sepfile)
        .str(&q.qproc)
        .str(&q.params)
        .str(&q.descr)
        .u16(q.held as u16)
        .u16(jobs.len() as u16)
        .str(&join(&printers, b','))
        .str(q.drivers.first().map_or(&b""[..], |d| d))
        .block(&q.drivdata);
    if six {
        r.str(b"").str(b"");
    }
    r
}

fn prq1(q: &Queue, jobs: &[Job]) -> Rec {
    let printers: Vec<Vec<u8>> = q.printers().into_iter().map(|p| p.name).collect();
    let mut r = Rec::new();
    r.chars(&q.name, 13)
        .chars(b"", 1)
        .u16(q.priority)
        .u16(q.start)
        .u16(q.until)
        .str(&q.sepfile)
        .str(&q.qproc)
        .str(&join(&printers, b' '))
        .str(&q.params)
        .str(&q.descr)
        .u16(if q.held { 1 } else { 0 })
        .u16(jobs.len() as u16);
    r
}

fn prj(j: &Job, level: u32) -> Option<Rec> {
    let mut r = Rec::new();
    let pos = jobs::position(j);
    let size = if j.size != 0 { j.size } else { jobs::spl_size(j) };
    match level {
        0 => {
            r.u16(j.id as u16);
        }
        1 => {
            r.u16(j.id as u16)
                .chars(&j.user, 21)
                .chars(b"", 1)
                .chars(&j.notify, 16)
                .chars(&j.datatype, 10)
                .u16(0)
                .str(&j.spl_params)
                .u16(pos)
                .u16(j.fs_status())
                .str(&j.status_text)
                .u32(j.submitted)
                .u32(size)
                .str(&j.comment);
        }
        2 | 3 => {
            r.u16(j.id as u16).u16(j.priority).str(&j.user).u16(pos).u16(j.fs_status()).u32(j.submitted).u32(size).str(&j.comment).str(&j.document);
            if level == 3 {
                r.str(&j.notify)
                    .str(&j.datatype)
                    .str(&j.spl_params)
                    .str(&j.status_text)
                    .str(&j.queue)
                    .str(&j.qproc)
                    .str(&j.qproc_params)
                    .str(&j.driver)
                    .block(&j.drivdata)
                    .str(&j.printer);
            }
        }
        _ => return None,
    }
    Some(r)
}

fn queue_rec(q: &Queue, level: u32) -> Option<Rec> {
    let js = jobs::queue_jobs(&q.name);
    Some(match level {
        1 => prq1(q, &js),
        2 => {
            let mut r = prq1(q, &js);
            for j in &js {
                r.append(prj(j, 1)?);
            }
            r
        }
        3 => prq3(q, &js, false),
        4 => {
            let mut r = prq3(q, &js, false);
            for j in &js {
                r.append(prj(j, 2)?);
            }
            r
        }
        5 => {
            let mut r = Rec::new();
            r.str(&q.name);
            r
        }
        6 => prq3(q, &js, true),
        _ => return None,
    })
}

fn printer_job(p: &Printer) -> Option<Job> {
    jobs::all_jobs().into_iter().find(|j| j.qstatus() == PRJ_QS_PRINTING && eq(&j.printer, &p.name))
}

fn device_rec(p: &Printer, level: u32) -> Option<Rec> {
    let mut r = Rec::new();
    let job = printer_job(p);
    let status = p.held as u16;
    match level {
        0 => {
            r.chars(&p.name, 9);
        }
        2 => {
            r.str(&p.name);
        }
        3 => {
            r.str(&p.name)
                .str(job.as_ref().map_or(&b""[..], |j| &j.user))
                .str(&p.port)
                .u16(job.as_ref().map_or(0, |j| j.id as u16))
                .u16(status)
                .str(b"")
                .str(&p.descr)
                .str(&join(&p.drivers, b','))
                .u16(0)
                .u16(p.timeout);
        }
        _ => return None,
    }
    Some(r)
}

/// The port driver that owns a port: rs2's port kinds.
pub fn port_driver(port: &[u8]) -> &'static [u8] {
    let p = upper(port);
    if p.starts_with(b"LPT") {
        b"PARALLEL"
    } else if p.starts_with(b"COM") {
        b"SERIAL"
    } else if p.starts_with(b"LPR") {
        b"LPRPDRVR"
    } else if p.starts_with(b"IPP") {
        b"IPPPDRVR"
    } else {
        b"FILE"
    }
}

fn port_rec(port: &[u8], level: u32) -> Option<Rec> {
    let mut r = Rec::new();
    match level {
        0 => {
            r.chars(port, 9);
        }
        1 => {
            r.str(port).str(port_driver(port)).str(b"");
        }
        2 => {
            r.str(port).str(port_driver(port)).str(b"").u32(0).u32(1).u32(0);
        }
        _ => return None,
    }
    Some(r)
}

fn all_recs<T>(items: &[T], f: impl Fn(&T) -> Option<Rec>) -> Option<Vec<Rec>> {
    items.iter().map(f).collect()
}

// ---- Queues ----

/// SplEnumQueue(pszComputerName, ulLevel, pBuf, cbBuf, pcReturned,
/// pcTotal, pcbNeeded, pReserved): levels 1-6.
#[unsafe(no_mangle)]
pub extern "C" fn SplEnumQueue(comp: *const u8, level: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let qs = ini::queues();
        let Some(recs) = all_recs(&qs, |q| queue_rec(q, level)) else { return ERROR_INVALID_LEVEL };
        enum_result(&recs, buf, cb, returned, total, needed)
    })
}

/// SplQueryQueue(pszComputerName, pszQueueName, ulLevel, pBuf, cbBuf,
/// pcbNeeded)
#[unsafe(no_mangle)]
pub extern "C" fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(q) = Queue::load(cstr(q)) else {
            put(needed, 0);
            return NERR_QNOTFOUND;
        };
        let Some(r) = queue_rec(&q, level) else { return ERROR_INVALID_LEVEL };
        query_result(r, buf, cb, needed)
    })
}

fn rd16(p: *const u8, off: usize) -> u16 {
    unsafe { (p.add(off) as *const u16).read_unaligned() }
}

fn rdp(p: *const u8, off: usize) -> *const u8 {
    unsafe { (p.add(off) as *const u32).read_unaligned() as *const u8 }
}

/// A DRIVDATA's bytes (its cb field gives the length).
pub fn drivdata(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let cb = unsafe { (p as *const u32).read_unaligned() } as usize;
    if !(40..=0x10000).contains(&cb) {
        return Vec::new();
    }
    unsafe { core::slice::from_raw_parts(p, cb) }.to_vec()
}

/// Make the printers in `names` (and only them) print queue `q`.
fn link_printers(q: &[u8], names: &[Vec<u8>]) {
    for mut p in ini::printers() {
        let has = p.queues.iter().any(|x| eq(x, q));
        let want = names.iter().any(|n| eq(n, &p.name));
        if has != want {
            if want {
                p.queues.push(q.to_vec());
            } else {
                p.queues.retain(|x| !eq(x, q));
            }
            p.save();
        }
    }
}

/// Apply PRQINFO3 fields to a queue.
fn set_from_prq3(qu: &mut Queue, b: *const u8) {
    qu.priority = rd16(b, 4);
    qu.start = rd16(b, 6);
    qu.until = rd16(b, 8);
    qu.flags = rd16(b, 10);
    qu.sepfile = cstr(rdp(b, 12)).to_vec();
    if let Some(p) = opt(rdp(b, 16)).filter(|p| !p.is_empty()) {
        qu.qproc = p.to_vec();
    }
    qu.params = cstr(rdp(b, 20)).to_vec();
    qu.descr = cstr(rdp(b, 24)).to_vec();
    if let Some(d) = opt(rdp(b, 36)).filter(|d| !d.is_empty()) {
        qu.drivers = list(d);
    }
    let dd = drivdata(rdp(b, 40));
    if !dd.is_empty() {
        qu.drivdata = dd;
    }
}

/// SplCreateQueue(pszComputerName, ulLevel, pBuf, cbBuf): level 3 or 6.
#[unsafe(no_mangle)]
pub extern "C" fn SplCreateQueue(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if level != 3 && level != 6 {
        return ERROR_INVALID_LEVEL;
    }
    if buf.is_null() || cb < 44 {
        return ERROR_INVALID_PARAMETER;
    }
    jobs::locked(|| {
        let name = upper(cstr(rdp(buf, 0)));
        if name.is_empty() || name.len() > 255 || name.iter().any(|c| b"\\/:*?\"<>|;, ".contains(c)) {
            return ERROR_INVALID_PARAMETER;
        }
        if Queue::load(&name).is_some() {
            return NERR_QEXISTS;
        }
        let mut q = Queue { name: name.clone(), qproc: b"PMPRINT".to_vec(), priority: 5, ..Queue::default() };
        set_from_prq3(&mut q, buf);
        if q.priority == 0 {
            q.priority = 5;
        }
        if q.drivdata.is_empty()
            && let Some(d) = q.drivers.first()
        {
            let dev = d.split(|&c| c == b'.').nth(1).unwrap_or(ini::PS_DEVICE);
            q.drivdata = ini::default_drivdata(dev);
        }
        q.save();
        crate::util::mkdir(&ini::spool_dir());
        crate::util::mkdir(&jobs::queue_dir(&name));
        link_printers(&name, &list(cstr(rdp(buf, 32))));
        NO_ERROR
    })
}

/// SplSetQueue(pszComputerName, pszQueueName, ulLevel, pBuf, cbBuf,
/// ulParmNum): parmnum 0 sets every field of a PRQINFO3/6; otherwise
/// pBuf holds the one value (PRQ_*_PARMNUM).
#[unsafe(no_mangle)]
pub extern "C" fn SplSetQueue(comp: *const u8, q: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if buf.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    jobs::locked(|| {
        let Some(mut qu) = Queue::load(cstr(q)) else { return NERR_QNOTFOUND };
        match parm {
            0 => {
                if (level != 3 && level != 6) || cb < 44 {
                    return ERROR_INVALID_LEVEL;
                }
                set_from_prq3(&mut qu, buf);
                link_printers(&qu.name, &list(cstr(rdp(buf, 32))));
            }
            2 => qu.priority = rd16(buf, 0),
            3 => qu.start = rd16(buf, 0),
            4 => qu.until = rd16(buf, 0),
            5 => qu.sepfile = cstr(buf).to_vec(),
            6 => qu.qproc = cstr(buf).to_vec(),
            8 => qu.params = cstr(buf).to_vec(),
            9 => qu.descr = cstr(buf).to_vec(),
            10 => qu.flags = rd16(buf, 0),
            12 => link_printers(&qu.name, &list(cstr(buf))),
            13 => qu.drivers = list(cstr(buf)),
            14 => {
                let d = drivdata(buf);
                if d.is_empty() {
                    return ERROR_INVALID_PARAMETER;
                }
                qu.drivdata = d;
            }
            _ => return ERROR_INVALID_PARAMETER,
        }
        qu.save();
        jobs::wake();
        NO_ERROR
    })
}

/// SplDeleteQueue(pszComputerName, pszQueueName): the queue, its jobs
/// and its directory.
#[unsafe(no_mangle)]
pub extern "C" fn SplDeleteQueue(comp: *const u8, q: *const u8) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(qu) = Queue::load(cstr(q)) else { return NERR_QNOTFOUND };
        for j in jobs::queue_jobs(&qu.name) {
            if j.qstatus() == PRJ_QS_PRINTING {
                return NERR_QINVALIDSTATE;
            }
            j.destroy();
        }
        let dir = crate::util::z(&jobs::queue_dir(&qu.name));
        unsafe { os2::DosDeleteDir(dir.as_ptr()) };
        link_printers(&qu.name, &[]);
        Queue::remove(&qu.name);
        if ini::default_queue().is_some_and(|d| d == qu.name) {
            ini::set_str(ini::SPOOLER, b"QUEUE", None);
        }
        NO_ERROR
    })
}

fn hold_queue(comp: *const u8, q: *const u8, held: bool) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(mut qu) = Queue::load(cstr(q)) else { return NERR_QNOTFOUND };
        qu.held = held;
        qu.save();
        jobs::wake();
        NO_ERROR
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplHoldQueue(comp: *const u8, q: *const u8) -> u32 {
    hold_queue(comp, q, true)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplReleaseQueue(comp: *const u8, q: *const u8) -> u32 {
    hold_queue(comp, q, false)
}

/// Delete a job (a printing job is aborted; the spooler removes it).
fn delete_job(mut j: Job) {
    if j.qstatus() == PRJ_QS_PRINTING {
        j.status |= PRJ_DELETED;
        j.save();
        jobs::set_abort(j.id);
    } else {
        j.destroy();
    }
}

/// SplPurgeQueue(pszComputerName, pszQueueName): every job goes.
#[unsafe(no_mangle)]
pub extern "C" fn SplPurgeQueue(comp: *const u8, q: *const u8) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(qu) = Queue::load(cstr(q)) else { return NERR_QNOTFOUND };
        for j in jobs::queue_jobs(&qu.name) {
            delete_job(j);
        }
        NO_ERROR
    })
}

// ---- Jobs ----

/// SplEnumJob(pszComputerName, pszQueueName, ulLevel, pBuf, cbBuf,
/// pcReturned, pcTotal, pcbNeeded, pReserved): levels 0-3.
#[unsafe(no_mangle)]
pub extern "C" fn SplEnumJob(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(qu) = Queue::load(cstr(q)) else {
            put(returned, 0);
            put(total, 0);
            put(needed, 0);
            return NERR_QNOTFOUND;
        };
        let js = jobs::queue_jobs(&qu.name);
        let Some(recs) = all_recs(&js, |j| prj(j, level)) else { return ERROR_INVALID_LEVEL };
        enum_result(&recs, buf, cb, returned, total, needed)
    })
}

/// SplQueryJob(pszComputerName, pszQueueName, ulJob, ulLevel, pBuf,
/// cbBuf, pcbNeeded)
#[unsafe(no_mangle)]
pub extern "C" fn SplQueryJob(comp: *const u8, q: *const u8, id: u32, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(j) = jobs::find_in(cstr(q), id) else {
            put(needed, 0);
            return NERR_JOBNOTFOUND;
        };
        let Some(r) = prj(&j, level) else { return ERROR_INVALID_LEVEL };
        query_result(r, buf, cb, needed)
    })
}

/// SplSetJob(pszComputerName, pszQueueName, ulJob, ulLevel, pBuf, cbBuf,
/// ulParmNum): parmnum 0 with a PRJINFO2/3, or one value: position (6),
/// comment (11), document (12), status comment (13), priority (14),
/// queue processor parameters (16), job properties (18).
#[unsafe(no_mangle)]
pub extern "C" fn SplSetJob(comp: *const u8, q: *const u8, id: u32, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if buf.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    jobs::locked(|| {
        let Some(mut j) = jobs::find_in(cstr(q), id) else { return NERR_JOBNOTFOUND };
        match parm {
            0 => {
                if !(level == 2 || level == 3) || cb < 32 {
                    return ERROR_INVALID_LEVEL;
                }
                let prio = rd16(buf, 2);
                if (1..=99).contains(&prio) {
                    j.priority = prio;
                }
                j.comment = cstr(rdp(buf, 20)).to_vec();
                j.document = cstr(rdp(buf, 24)).to_vec();
                if level == 3 && cb >= 68 {
                    j.notify = cstr(rdp(buf, 28)).to_vec();
                    j.status_text = cstr(rdp(buf, 40)).to_vec();
                    j.qproc_params = cstr(rdp(buf, 52)).to_vec();
                    let d = drivdata(rdp(buf, 60));
                    if !d.is_empty() {
                        j.drivdata = d;
                    }
                }
                j.save();
                let pos = rd16(buf, 8);
                if pos != 0 {
                    jobs::set_position(&j, pos);
                }
            }
            6 => {
                jobs::set_position(&j, rd16(buf, 0));
            }
            11 => {
                j.comment = cstr(buf).to_vec();
                j.save();
            }
            12 => {
                j.document = cstr(buf).to_vec();
                j.save();
            }
            13 => {
                j.status_text = cstr(buf).to_vec();
                j.save();
            }
            14 => {
                let p = rd16(buf, 0);
                if !(1..=99).contains(&p) {
                    return ERROR_INVALID_PARAMETER;
                }
                j.priority = p;
                j.save();
            }
            16 => {
                j.qproc_params = cstr(buf).to_vec();
                j.save();
            }
            18 => {
                j.drivdata = drivdata(buf);
                j.save();
            }
            _ => return ERROR_INVALID_PARAMETER,
        }
        NO_ERROR
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplDeleteJob(comp: *const u8, q: *const u8, id: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(j) = jobs::find_in(cstr(q), id) else { return NERR_JOBNOTFOUND };
        delete_job(j);
        NO_ERROR
    })
}

fn hold_job(comp: *const u8, q: *const u8, id: u32, held: bool) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(mut j) = jobs::find_in(cstr(q), id) else { return NERR_JOBNOTFOUND };
        if held && j.qstatus() == PRJ_QS_PRINTING {
            return NERR_JOBINVALIDSTATE;
        }
        j.held = held;
        if !held {
            j.status &= !jobs::PRJ_ERROR;
        }
        j.save();
        if !held {
            jobs::wake();
        }
        NO_ERROR
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplHoldJob(comp: *const u8, q: *const u8, id: u32) -> u32 {
    hold_job(comp, q, id, true)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplReleaseJob(comp: *const u8, q: *const u8, id: u32) -> u32 {
    hold_job(comp, q, id, false)
}

/// SplCopyJob(pszSrcComputerName, pszSrcQueueName, ulSrcJob,
/// pszTrgComputerName, pszTrgQueueName, pulTrgJob): a new job with the
/// same data on the target queue (the source's queue when NULL).
#[unsafe(no_mangle)]
pub extern "C" fn SplCopyJob(sc: *const u8, sq: *const u8, id: u32, tc: *const u8, tq: *const u8, out: *mut u32) -> u32 {
    if !local(sc) || !local(tc) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(src) = jobs::find_in(cstr(sq), id) else { return NERR_JOBNOTFOUND };
        let target = if cstr(tq).is_empty() { src.queue.clone() } else { upper(cstr(tq)) };
        if Queue::load(&target).is_none() {
            return NERR_QNOTFOUND;
        }
        let data = crate::util::read_file(&src.spl_path()).unwrap_or_default();
        let mut j = src.clone();
        j.id = jobs::next_id();
        j.queue = target;
        j.seq = jobs::next_seq();
        j.status &= !(3 | PRJ_DELETED);
        j.printer.clear();
        j.submitted = crate::util::now();
        j.save();
        crate::util::write_file(&j.spl_path(), &data);
        put(out, j.id);
        jobs::wake();
        NO_ERROR
    })
}

// ---- Devices (printers) ----

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumDevice(comp: *const u8, level: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let ps = ini::printers();
        let Some(recs) = all_recs(&ps, |p| device_rec(p, level)) else { return ERROR_INVALID_LEVEL };
        enum_result(&recs, buf, cb, returned, total, needed)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQueryDevice(comp: *const u8, dev: *const u8, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(p) = Printer::load(cstr(dev)) else {
            put(needed, 0);
            return NERR_DESTNOTFOUND;
        };
        let Some(r) = device_rec(&p, level) else { return ERROR_INVALID_LEVEL };
        query_result(r, buf, cb, needed)
    })
}

/// SplCreateDevice(pszComputerName, ulLevel, pBuf, cbBuf): a PRDINFO3.
#[unsafe(no_mangle)]
pub extern "C" fn SplCreateDevice(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if level != 3 {
        return ERROR_INVALID_LEVEL;
    }
    if buf.is_null() || cb < 32 {
        return ERROR_INVALID_PARAMETER;
    }
    jobs::locked(|| {
        let name = upper(cstr(rdp(buf, 0)));
        if name.is_empty() || name.len() > 32 {
            return ERROR_INVALID_PARAMETER;
        }
        if Printer::load(&name).is_some() {
            return NERR_DESTEXISTS;
        }
        let port = upper(cstr(rdp(buf, 8)));
        if !port.is_empty() && ini::port_settings(&port).is_none() {
            return NERR_BADDEV;
        }
        let to = rd16(buf, 30);
        let p = Printer { name, descr: cstr(rdp(buf, 20)).to_vec(), port, drivers: list(cstr(rdp(buf, 24))), queues: Vec::new(), held: false, timeout: if to == 0 { 45 } else { to } };
        p.save();
        NO_ERROR
    })
}

/// SplSetDevice(pszComputerName, pszPrintDeviceName, ulLevel, pBuf,
/// cbBuf, ulParmNum): a PRDINFO3 (0), the port (3), the description
/// (7), the drivers (8) or the timeout (10).
#[unsafe(no_mangle)]
pub extern "C" fn SplSetDevice(comp: *const u8, dev: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if buf.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    jobs::locked(|| {
        let Some(mut p) = Printer::load(cstr(dev)) else { return NERR_DESTNOTFOUND };
        let set_port = |p: &mut Printer, v: &[u8]| -> bool {
            let v = upper(v);
            if !v.is_empty() && ini::port_settings(&v).is_none() {
                return false;
            }
            p.port = v;
            true
        };
        match parm {
            0 => {
                if level != 3 || cb < 32 {
                    return ERROR_INVALID_LEVEL;
                }
                if !set_port(&mut p, cstr(rdp(buf, 8))) {
                    return NERR_BADDEV;
                }
                p.descr = cstr(rdp(buf, 20)).to_vec();
                p.drivers = list(cstr(rdp(buf, 24)));
                let to = rd16(buf, 30);
                if to != 0 {
                    p.timeout = to;
                }
            }
            3 => {
                if !set_port(&mut p, cstr(buf)) {
                    return NERR_BADDEV;
                }
            }
            7 => p.descr = cstr(buf).to_vec(),
            8 => p.drivers = list(cstr(buf)),
            10 => p.timeout = rd16(buf, 0),
            _ => return ERROR_INVALID_PARAMETER,
        }
        p.save();
        NO_ERROR
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplDeleteDevice(comp: *const u8, dev: *const u8) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(p) = Printer::load(cstr(dev)) else { return NERR_DESTNOTFOUND };
        if printer_job(&p).is_some() {
            return NERR_DESTINVALIDSTATE;
        }
        Printer::remove(&p.name);
        if ini::first(ini::SPOOLER, b"PRINTER").is_some_and(|d| eq(&d, &p.name)) {
            ini::set_str(ini::SPOOLER, b"PRINTER", None);
        }
        NO_ERROR
    })
}

/// SplControlDevice(pszComputerName, pszPrintDeviceName, ulControl):
/// PRD_DELETE (the printing job), PRD_PAUSE, PRD_CONT, PRD_RESTART.
#[unsafe(no_mangle)]
pub extern "C" fn SplControlDevice(comp: *const u8, dev: *const u8, ctl: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let Some(mut p) = Printer::load(cstr(dev)) else { return NERR_DESTNOTFOUND };
        match ctl {
            0 | 3 => match printer_job(&p) {
                Some(mut j) => {
                    if ctl == 0 {
                        j.status |= PRJ_DELETED;
                        j.save();
                    }
                    jobs::set_abort(j.id);
                }
                None => return 2158, // NERR_DestIdle
            },
            1 => {
                p.held = true;
                p.save();
            }
            2 => {
                p.held = false;
                p.save();
                jobs::wake();
            }
            _ => return NERR_DESTINVALIDOP,
        }
        NO_ERROR
    })
}

/// SplEnumPrinter(pszComputerName, uLevel, flType, pBuf, cbBuf,
/// pcReturned, pcTotal, pcbNeeded, pReserved): PRINTERINFO for queues
/// (SPL_PR_QUEUE) and printers (SPL_PR_DIRECT_DEVICE: without a queue,
/// SPL_PR_QUEUED_DEVICE: with one).
#[unsafe(no_mangle)]
pub extern "C" fn SplEnumPrinter(comp: *const u8, level: u32, ty: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if level != 0 {
        return ERROR_INVALID_LEVEL;
    }
    jobs::locked(|| {
        let mut recs = Vec::new();
        if ty & 1 != 0 {
            for q in ini::queues() {
                let mut r = Rec::new();
                r.u32(1).str(b"").str(&q.name).str(&q.descr).str(&q.name);
                recs.push(r);
            }
        }
        for p in ini::printers() {
            let kind = if p.queues.is_empty() { 2 } else { 4 };
            if ty & kind != 0 {
                let mut r = Rec::new();
                r.u32(kind).str(b"").str(&p.name).str(&p.descr).str(&p.name);
                recs.push(r);
            }
        }
        enum_result(&recs, buf, cb, returned, total, needed)
    })
}

// ---- Drivers, queue processors, ports ----

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumDriver(comp: *const u8, level: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if level != 0 {
        return ERROR_INVALID_LEVEL;
    }
    jobs::locked(|| {
        let recs: Vec<Rec> = ini::drivers()
            .iter()
            .map(|d| {
                let mut r = Rec::new();
                r.chars(d, 41);
                r
            })
            .collect();
        enum_result(&recs, buf, cb, returned, total, needed)
    })
}

/// SplQueryDriver(pszComputerName, pszDriverName, pszPrinterName,
/// ulLevel, pBuf, cbBuf, pcbNeeded): level 2 (PRDRIVINFO2 without
/// driver properties).
#[unsafe(no_mangle)]
pub extern "C" fn SplQueryDriver(comp: *const u8, drv: *const u8, prt: *const u8, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let d = upper(cstr(drv));
        let name = d.split(|&c| c == b'.').next().unwrap_or(b"").to_vec();
        if !ini::drivers().iter().any(|x| eq(x, &name)) {
            put(needed, 0);
            return NERR_DRIVERNOTFOUND;
        }
        if level != 2 {
            return ERROR_INVALID_LEVEL;
        }
        let mut r = Rec::new();
        r.str(cstr(prt)).str(cstr(drv)).u16(0).u16(0);
        query_result(r, buf, cb, needed)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumQueueProcessor(comp: *const u8, level: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    if level != 0 {
        return ERROR_INVALID_LEVEL;
    }
    jobs::locked(|| {
        let recs: Vec<Rec> = ini::qprocs()
            .iter()
            .map(|d| {
                let mut r = Rec::new();
                r.chars(d, 13);
                r
            })
            .collect();
        enum_result(&recs, buf, cb, returned, total, needed)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumPort(comp: *const u8, level: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let ports = ini::ports();
        let Some(recs) = all_recs(&ports, |p| port_rec(p, level)) else { return ERROR_INVALID_LEVEL };
        enum_result(&recs, buf, cb, returned, total, needed)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQueryPort(comp: *const u8, port: *const u8, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let p = upper(cstr(port));
        if ini::port_settings(&p).is_none() {
            put(needed, 0);
            return ERROR_INVALID_PARAMETER;
        }
        let Some(r) = port_rec(&p, level) else { return ERROR_INVALID_LEVEL };
        query_result(r, buf, cb, needed)
    })
}

/// SplCreatePort(pszComputerName, pszPortName, pszPortDriver, ulVersion,
/// pBuf, cbBuf): a port with empty settings (rs2's port kinds follow
/// the name: LPTn, COMn, FILE, LPRn, IPPn).
#[unsafe(no_mangle)]
pub extern "C" fn SplCreatePort(comp: *const u8, port: *const u8, _drv: *const u8, _ver: u32, _buf: *const u8, _cb: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let p = upper(cstr(port));
        if p.is_empty() || p.len() > 8 {
            return ERROR_INVALID_PARAMETER;
        }
        if ini::port_settings(&p).is_some() {
            return NERR_DESTEXISTS;
        }
        ini::set_str(ini::PORT, &p, Some(b";"));
        NO_ERROR
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SplDeletePort(comp: *const u8, port: *const u8) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| {
        let p = upper(cstr(port));
        if ini::port_settings(&p).is_none() {
            return ERROR_INVALID_PARAMETER;
        }
        if ini::printers().iter().any(|x| eq(&x.port, &p)) {
            return NERR_DESTINVALIDSTATE;
        }
        ini::set_str(ini::PORT, &p, None);
        NO_ERROR
    })
}

/// SplSetPort(pszComputerName, pszPortName, ulLevel, pBuf, cbBuf,
/// ulParmNum): rs2's ports have no driver or mode to change.
#[unsafe(no_mangle)]
pub extern "C" fn SplSetPort(comp: *const u8, port: *const u8, _level: u32, _buf: *const u8, _cb: u32, _parm: u32) -> u32 {
    if !local(comp) {
        return NERR_INVALIDCOMPUTER;
    }
    jobs::locked(|| if ini::port_settings(&upper(cstr(port))).is_none() { ERROR_INVALID_PARAMETER } else { NO_ERROR })
}
