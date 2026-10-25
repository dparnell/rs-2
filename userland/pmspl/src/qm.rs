//! The queue manager calls programs (and PMWIN's queued DCs) print with:
//! SplQmOpen, SplQmStartDoc, SplQmWrite, SplQmEndDoc, SplQmClose and the
//! abort calls. The job's data goes straight to its spool file in the
//! calling process; SplQmEndDoc makes the job ready and wakes the
//! spooler, starting PMSPOOL.EXE when none is running.

use alloc::vec::Vec;

use crate::ini::{self, Queue};
use crate::jobs::{self, Job, PRJ_QS_QUEUED, PRJ_QS_SPOOLING};
use crate::util::{cat, cstr, put, upper, write_all, Table};

/// HSPL values (SPL_ERROR is 0).
const HSPL_BASE: u32 = 0x5350_0001;

struct Open {
    queue: Vec<u8>,
    token: Vec<u8>,
    /// The DEVOPENSTRUC strings: driver, data type, comment, queue
    /// processor, its parameters, spooler and network parameters.
    driver: Vec<u8>,
    datatype: Vec<u8>,
    comment: Vec<u8>,
    qproc: Vec<u8>,
    qproc_params: Vec<u8>,
    spl_params: Vec<u8>,
    net_params: Vec<u8>,
    drivdata: Vec<u8>,
    job: Option<Job>,
    hf: u32,
    pages: u32,
    last_job: u32,
}

static OPENS: Table<Open> = Table::new();

fn psz(a: *const u32, n: i32, i: i32) -> Vec<u8> {
    if i >= n || a.is_null() {
        return Vec::new();
    }
    cstr((unsafe { a.add(i as usize).read_unaligned() }) as *const u8).to_vec()
}

/// SplQmOpen(pszToken, lCount, pqmdopData): pqmdopData is a
/// DEVOPENSTRUC (an array of lCount pointers); the logical address names
/// the queue (the default queue when empty).
#[unsafe(no_mangle)]
pub extern "C" fn SplQmOpen(token: *const u8, n: i32, dop: *const u32) -> u32 {
    if n < 1 || dop.is_null() {
        crate::set_error(crate::PMERR_INVALID_PARM);
        return 0;
    }
    let logaddr = upper(&psz(dop, n, 0));
    let r = jobs::locked(|| {
        let q = if logaddr.is_empty() { ini::default_queue().and_then(|d| Queue::load(&d)) } else { Queue::load(&logaddr) };
        // A printer's name finds its first queue.
        let q = q.or_else(|| ini::Printer::load(&logaddr).and_then(|p| p.queues.first().and_then(|n| Queue::load(n))));
        q
    });
    let Some(q) = r else {
        crate::set_error(crate::PMERR_SPL_QUEUE_NOT_FOUND);
        return 0;
    };
    let pdriv = if n > 2 { (unsafe { dop.add(2).read_unaligned() }) as *const u8 } else { core::ptr::null() };
    let mut drivdata = crate::api::drivdata(pdriv);
    if drivdata.is_empty() {
        drivdata = q.drivdata.clone();
    }
    let mut driver = psz(dop, n, 1);
    if driver.is_empty() {
        driver = q.drivers.first().cloned().unwrap_or_default();
    }
    let mut datatype = psz(dop, n, 3);
    if datatype.is_empty() {
        datatype = b"PM_Q_STD".to_vec();
    }
    let qproc = match psz(dop, n, 5) {
        p if p.is_empty() => q.qproc.clone(),
        p => p,
    };
    let o = Open {
        queue: q.name,
        token: cstr(token).to_vec(),
        driver,
        datatype,
        comment: psz(dop, n, 4),
        qproc,
        qproc_params: psz(dop, n, 6),
        spl_params: psz(dop, n, 7),
        net_params: psz(dop, n, 8),
        drivdata,
        job: None,
        hf: 0,
        pages: 0,
        last_job: 0,
    };
    HSPL_BASE + OPENS.add(o) as u32
}

fn with<R>(h: u32, f: impl FnOnce(&mut Open) -> R) -> Option<R> {
    let i = h.checked_sub(HSPL_BASE)? as usize;
    OPENS.with(i, f)
}

fn start(o: &mut Open, doc: &[u8]) -> bool {
    if o.job.is_some() {
        return false;
    }
    let made = jobs::locked(|| {
        let id = jobs::next_id();
        if id == 0 {
            return None;
        }
        let j = Job {
            id,
            queue: o.queue.clone(),
            priority: 50,
            seq: jobs::next_seq(),
            status: PRJ_QS_SPOOLING,
            submitted: crate::util::now(),
            user: b"LOCAL".to_vec(),
            datatype: o.datatype.clone(),
            comment: o.comment.clone(),
            document: doc.to_vec(),
            qproc: o.qproc.clone(),
            qproc_params: o.qproc_params.clone(),
            spl_params: o.spl_params.clone(),
            net_params: o.net_params.clone(),
            driver: o.driver.clone(),
            logaddr: o.queue.clone(),
            token: o.token.clone(),
            drivdata: o.drivdata.clone(),
            ..Job::default()
        };
        if !j.save() {
            return None;
        }
        let hf = crate::util::create(&j.spl_path())?;
        Some((j, hf))
    });
    match made {
        Some((j, hf)) => {
            o.job = Some(j);
            o.hf = hf;
            o.pages = 0;
            true
        }
        None => false,
    }
}

/// SplQmStartDoc(hspl, pszDocName)
#[unsafe(no_mangle)]
pub extern "C" fn SplQmStartDoc(h: u32, name: *const u8) -> u32 {
    with(h, |o| start(o, cstr(name)) as u32).unwrap_or(0)
}

/// SplQmWrite(hspl, lCount, pData): a document starts if none has.
#[unsafe(no_mangle)]
pub extern "C" fn SplQmWrite(h: u32, n: i32, data: *const u8) -> u32 {
    if n < 0 || (n > 0 && data.is_null()) {
        return 0;
    }
    with(h, |o| {
        if o.job.is_none() {
            let doc = o.comment.clone();
            if !start(o, &doc) {
                return 0;
            }
        }
        let d = unsafe { core::slice::from_raw_parts(data, n as usize) };
        write_all(o.hf, d) as u32
    })
    .unwrap_or(0)
}

/// SplQmNewPage(hspl, ulPageNumber)
#[unsafe(no_mangle)]
pub extern "C" fn SplQmNewPage(h: u32, _page: u32) -> u32 {
    with(h, |o| {
        o.pages += 1;
        1
    })
    .unwrap_or(0)
}

fn end(o: &mut Open) -> u32 {
    let Some(mut j) = o.job.take() else { return 0 };
    unsafe { os2::DosClose(o.hf) };
    o.hf = 0;
    let id = j.id;
    let ok = jobs::locked(|| {
        // Deleted while spooling?
        if jobs::find_in(&j.queue, id).is_none() {
            j.destroy();
            return false;
        }
        j.size = jobs::spl_size(&j);
        j.pages = o.pages;
        j.status = (j.status & !3) | PRJ_QS_QUEUED;
        j.save()
    });
    if !ok {
        return 0;
    }
    o.last_job = id;
    if !jobs::wake() {
        start_spooler();
    }
    id
}

/// SplQmEndDoc(hspl): the job id (0 on error).
#[unsafe(no_mangle)]
pub extern "C" fn SplQmEndDoc(h: u32) -> u32 {
    with(h, end).unwrap_or(0)
}

fn abort_doc(o: &mut Open) {
    if let Some(j) = o.job.take() {
        unsafe { os2::DosClose(o.hf) };
        o.hf = 0;
        jobs::locked(|| j.destroy());
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmAbortDoc(h: u32) -> u32 {
    with(h, |o| {
        abort_doc(o);
        1
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmAbort(h: u32) -> u32 {
    let Some(i) = h.checked_sub(HSPL_BASE) else { return 0 };
    match OPENS.take(i as usize) {
        Some(mut o) => {
            abort_doc(&mut o);
            1
        }
        None => 0,
    }
}

/// SplQmClose(hspl): a document still open is ended (printed).
#[unsafe(no_mangle)]
pub extern "C" fn SplQmClose(h: u32) -> u32 {
    let Some(i) = h.checked_sub(HSPL_BASE) else { return 0 };
    match OPENS.take(i as usize) {
        Some(mut o) => {
            if o.job.is_some() {
                end(&mut o);
            }
            1
        }
        None => 0,
    }
}

/// SplQmGetJobID(hspl, ulLevel, pBuf, cbBuf, pcbNeeded): level 0 is a
/// QMJOBINFO for the current (or last) job.
#[unsafe(no_mangle)]
pub extern "C" fn SplQmGetJobID(h: u32, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32 {
    let Some((id, q)) = with(h, |o| (o.job.as_ref().map_or(o.last_job, |j| j.id), o.queue.clone())) else { return crate::api::ERROR_INVALID_PARAMETER };
    if level != 0 {
        return crate::api::ERROR_INVALID_LEVEL;
    }
    let mut r = crate::util::Rec::new();
    r.u32(id).str(b"").str(&q);
    let rc = crate::util::query_result(r, buf, cb, needed);
    if rc != 0 {
        put(needed, 12 + 1 + q.len() as u32 + 1);
    }
    rc
}

/// Start PMSPOOL.EXE (detached).
pub fn start_spooler() {
    let path = cat(&[&[crate::util::boot_drive()], b":\\OS2\\PMSPOOL.EXE\0"]);
    let mut args = Vec::new();
    args.extend_from_slice(b"PMSPOOL\0\0\0");
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    // EXEC_BACKGROUND
    unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 4, args.as_ptr(), core::ptr::null(), &mut res, path.as_ptr()) };
    // Let it create its semaphore, then make sure it sees the job.
    for _ in 0..50 {
        unsafe { os2::DosSleep(20) };
        if jobs::wake() {
            break;
        }
    }
}
