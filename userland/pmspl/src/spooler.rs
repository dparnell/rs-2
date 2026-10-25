//! The spooler itself, run by PMSPOOL.EXE through SplQmInitialize
//! (ordinal 312, SPL32QMINITIALIZE, as IBM's PMSPOOL calls it): a thread
//! that waits for ready jobs and gives each to its queue processor when
//! one of the queue's printers is free.
//!
//! A printer prints one job at a time; the next job of a printer is the
//! first ready job (not held, not spooling) of the first of its queues
//! that is not held, in queue order (priority, then submission). The
//! queue processor (PM_SPOOLER_QP: PMPRINT.QPR) gets the job as IBM's
//! spooler hands it over: SplQpOpen with the twelve QPOPENDATA items
//! (the printer's name as the logical address, the job's driver, job
//! properties, data type, comment, parameters, document and queue
//! names, token and job id), SplQpPrint with the spool file, SplQpClose.
//! A printed (or deleted) job is removed; a job the queue processor
//! could not print is held with an error status.

use alloc::vec::Vec;

use crate::ini::{self, Printer, Queue};
use crate::jobs::{self, Job, PRJ_DELETED, PRJ_ERROR, PRJ_QS_PRINTING, PRJ_QS_QUEUED};
use crate::util::{eq, z};

static mut HEV: u32 = 0;

type QpOpen = unsafe extern "C" fn(i32, *const u32) -> u32;
type QpPrint = unsafe extern "C" fn(u32, *const u8) -> u32;
type QpClose = unsafe extern "C" fn(u32) -> u32;

/// SPL32QMINITIALIZE(pulReserved): start the spooler in this process. 0
/// when it runs; non-zero if a spooler is already running.
#[unsafe(no_mangle)]
pub extern "C" fn SplQmInitialize(_r: *mut u32) -> u32 {
    let mut h = 0;
    // DC_SEM_SHARED is implied by the name.
    let rc = unsafe { os2::DosCreateEventSem(jobs::WAKE.as_ptr(), &mut h, 0, 0) };
    if rc != 0 {
        return rc;
    }
    unsafe { HEV = h };
    jobs::locked(jobs::recover);
    let mut tid = 0;
    let rc = unsafe { os2::DosCreateThread(&mut tid, spooler_thread, 0, 0, 512 * 1024) };
    if rc != 0 {
        return rc;
    }
    0
}

/// SplQmSpoolerPresent? IBM: SPL32QMSPOOLERPRESENT (ordinal 310): TRUE
/// when the spooler runs.
#[unsafe(no_mangle)]
pub extern "C" fn SplQmSpoolerPresent() -> u32 {
    let mut h = 0;
    unsafe extern "C" {
        #[link_name = "DOSCALLS.325"]
        fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
    }
    if unsafe { DosOpenEventSem(jobs::WAKE.as_ptr(), &mut h) } != 0 {
        return 0;
    }
    unsafe { os2::DosCloseEventSem(h) };
    1
}

extern "C" fn spooler_thread(_: u32) {
    loop {
        while let Some((job, printer)) = jobs::locked(next) {
            print(job, printer);
        }
        let mut n = 0;
        unsafe {
            os2::DosWaitEventSem(HEV, 5000);
            os2::DosResetEventSem(HEV, &mut n);
        }
    }
}

/// The next job to print and its printer, marked as printing.
fn next() -> Option<(Job, Printer)> {
    let all = jobs::all_jobs();
    for p in ini::printers() {
        if p.held || p.port.is_empty() || all.iter().any(|j| j.qstatus() == PRJ_QS_PRINTING && eq(&j.printer, &p.name)) {
            continue;
        }
        for qn in &p.queues {
            let Some(q) = Queue::load(qn) else { continue };
            if q.held {
                continue;
            }
            let ready = jobs::queue_jobs(&q.name).into_iter().find(|j| j.qstatus() == PRJ_QS_QUEUED && !j.held && j.status & (PRJ_DELETED | PRJ_ERROR) == 0);
            if let Some(mut j) = ready {
                j.status = (j.status & !3) | PRJ_QS_PRINTING;
                j.printer = p.name.clone();
                j.save();
                return Some((j, p));
            }
        }
    }
    None
}

fn proc_addr(hmod: u32, name: &[u8], ord: u32) -> usize {
    let n = z(name);
    let mut f = 0usize;
    if unsafe { os2::DosQueryProcAddr(hmod, 0, n.as_ptr(), &mut f) } != 0 {
        f = 0;
        unsafe { os2::DosQueryProcAddr(hmod, ord, core::ptr::null(), &mut f) };
    }
    f
}

/// Hand a job to its queue processor; true if it printed.
fn run_qproc(j: &Job, p: &Printer) -> bool {
    let qproc = if j.qproc.is_empty() { b"PMPRINT".to_vec() } else { j.qproc.clone() };
    let Some(path) = jobs::locked(|| ini::qproc_path(&qproc)) else { return false };
    let path = z(&path);
    let mut hmod = 0;
    let mut fail = [0u8; 64];
    if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, path.as_ptr(), &mut hmod) } != 0 {
        return false;
    }
    let (fo, fp, fc) = (proc_addr(hmod, b"SplQpOpen", 1), proc_addr(hmod, b"SplQpPrint", 3), proc_addr(hmod, b"SplQpClose", 2));
    if fo == 0 || fp == 0 || fc == 0 {
        unsafe { os2::DosFreeModule(hmod) };
        return false;
    }
    let (open, prt, close): (QpOpen, QpPrint, QpClose) = unsafe { (core::mem::transmute(fo), core::mem::transmute(fp), core::mem::transmute(fc)) };
    let strs: Vec<Vec<u8>> = [&p.name, &j.driver, &Vec::new(), &j.datatype, &j.comment, &j.qproc_params, &j.spl_params, &j.net_params, &j.document, &j.queue, &j.token].iter().map(|s| z(s)).collect();
    let mut data: [u32; 12] = [0; 12];
    for (i, s) in strs.iter().enumerate() {
        data[i] = s.as_ptr() as u32;
    }
    data[2] = if j.drivdata.is_empty() { 0 } else { j.drivdata.as_ptr() as u32 };
    data[11] = j.id & 0xffff;
    let spl = z(&j.spl_path());
    let ok = unsafe {
        let h = open(12, data.as_ptr());
        if h == 0 {
            false
        } else {
            let r = prt(h, spl.as_ptr()) != 0;
            close(h);
            r
        }
    };
    unsafe { os2::DosFreeModule(hmod) };
    ok
}

fn print(j: Job, p: Printer) {
    let ok = run_qproc(&j, &p);
    jobs::locked(|| {
        let Some(mut now) = jobs::find_in(&j.queue, j.id) else { return };
        let aborted = jobs::abort_requested(j.id);
        if ok || now.status & PRJ_DELETED != 0 || aborted {
            now.destroy();
        } else {
            now.status = (now.status & !3) | PRJ_QS_QUEUED | PRJ_ERROR;
            now.held = true;
            now.printer.clear();
            now.status_text = b"The job could not be printed".to_vec();
            now.save();
        }
        if aborted {
            jobs::set_abort(0);
        }
    });
}
