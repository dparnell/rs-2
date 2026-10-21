//! PMSPL.DLL: the spooler API (Spl*) of a system that has no printers
//! (rs2 has no printer drivers or spooler yet). Programs that list
//! queues, devices, jobs, drivers or ports get empty lists, queries of a
//! named queue, device, job or port report that it does not exist, and
//! print jobs (SplQmOpen) cannot be opened. EPM, the system log viewer
//! (SYSLOGPM) and the printer utilities ask for queues this way when they
//! start.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

mod exports;

const NO_ERROR: u32 = 0;
const ERROR_NOT_SUPPORTED: u32 = 50;
const ERROR_INVALID_PARAMETER: u32 = 87;
const NERR_JOBNOTFOUND: u32 = 2151;
const NERR_DESTNOTFOUND: u32 = 2152;
const NERR_QNOTFOUND: u32 = 2150;
const NERR_DRIVERNOTFOUND: u32 = 2163;
/// SPL_ERROR (no queue manager handle).
const SPL_ERROR: u32 = 0;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// The common tail of the Spl*Enum calls: nothing returned, nothing
/// there, no buffer needed.
fn empty(returned: *mut u32, total: *mut u32, needed: *mut u32) -> u32 {
    for p in [returned, total, needed] {
        if !p.is_null() {
            unsafe { p.write_unaligned(0) };
        }
    }
    NO_ERROR
}

fn none(needed: *mut u32, err: u32) -> u32 {
    if !needed.is_null() {
        unsafe { needed.write_unaligned(0) };
    }
    err
}

// ---- Queues ----

/// SplEnumQueue(pszComputerName, ulLevel, pBuf, cbBuf, pcReturned,
/// pcTotal, pcbNeeded, pReserved)
#[unsafe(no_mangle)]
pub extern "C" fn SplEnumQueue(_comp: u32, _level: u32, _buf: u32, _cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    empty(returned, total, needed)
}

/// SplQueryQueue(pszComputerName, pszQueueName, ulLevel, pBuf, cbBuf,
/// pcbNeeded)
#[unsafe(no_mangle)]
pub extern "C" fn SplQueryQueue(_comp: u32, _q: u32, _level: u32, _buf: u32, _cb: u32, needed: *mut u32) -> u32 {
    none(needed, NERR_QNOTFOUND)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplCreateQueue(_comp: u32, _level: u32, _buf: u32, _cb: u32) -> u32 {
    ERROR_NOT_SUPPORTED
}

#[unsafe(no_mangle)]
pub extern "C" fn SplSetQueue(_comp: u32, _q: u32, _level: u32, _buf: u32, _cb: u32, _parm: u32) -> u32 {
    NERR_QNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplDeleteQueue(_comp: u32, _q: u32) -> u32 {
    NERR_QNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplHoldQueue(_comp: u32, _q: u32) -> u32 {
    NERR_QNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplReleaseQueue(_comp: u32, _q: u32) -> u32 {
    NERR_QNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplPurgeQueue(_comp: u32, _q: u32) -> u32 {
    NERR_QNOTFOUND
}

// ---- Jobs ----

/// SplEnumJob(pszComputerName, pszQueueName, ulLevel, pBuf, cbBuf,
/// pcReturned, pcTotal, pcbNeeded, pReserved)
#[unsafe(no_mangle)]
pub extern "C" fn SplEnumJob(_comp: u32, _q: u32, _level: u32, _buf: u32, _cb: u32, _returned: *mut u32, _total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    none(needed, NERR_QNOTFOUND)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQueryJob(_comp: u32, _q: u32, _job: u32, _level: u32, _buf: u32, _cb: u32, needed: *mut u32) -> u32 {
    none(needed, NERR_JOBNOTFOUND)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplSetJob(_comp: u32, _q: u32, _job: u32, _level: u32, _buf: u32, _cb: u32, _parm: u32) -> u32 {
    NERR_JOBNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplDeleteJob(_comp: u32, _q: u32, _job: u32) -> u32 {
    NERR_JOBNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplHoldJob(_comp: u32, _q: u32, _job: u32) -> u32 {
    NERR_JOBNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplReleaseJob(_comp: u32, _q: u32, _job: u32) -> u32 {
    NERR_JOBNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplCopyJob(_comp: u32, _q: u32, _job: u32, _qt: u32, _pjob: u32) -> u32 {
    NERR_JOBNOTFOUND
}

// ---- Devices (printers), drivers, ports, queue processors ----

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumDevice(_comp: u32, _level: u32, _buf: u32, _cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    empty(returned, total, needed)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQueryDevice(_comp: u32, _dev: u32, _level: u32, _buf: u32, _cb: u32, needed: *mut u32) -> u32 {
    none(needed, NERR_DESTNOTFOUND)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplCreateDevice(_comp: u32, _level: u32, _buf: u32, _cb: u32) -> u32 {
    ERROR_NOT_SUPPORTED
}

#[unsafe(no_mangle)]
pub extern "C" fn SplSetDevice(_comp: u32, _dev: u32, _level: u32, _buf: u32, _cb: u32, _parm: u32) -> u32 {
    NERR_DESTNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplDeleteDevice(_comp: u32, _dev: u32) -> u32 {
    NERR_DESTNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplControlDevice(_comp: u32, _dev: u32, _ctl: u32) -> u32 {
    NERR_DESTNOTFOUND
}

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumPrinter(_comp: u32, _level: u32, _type: u32, _buf: u32, _cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    empty(returned, total, needed)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumDriver(_comp: u32, _level: u32, _buf: u32, _cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    empty(returned, total, needed)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQueryDriver(_comp: u32, _drv: u32, _model: u32, _level: u32, _buf: u32, _cb: u32, needed: *mut u32) -> u32 {
    none(needed, NERR_DRIVERNOTFOUND)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumPort(_comp: u32, _level: u32, _buf: u32, _cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    empty(returned, total, needed)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQueryPort(_comp: u32, _port: u32, _level: u32, _buf: u32, _cb: u32, needed: *mut u32) -> u32 {
    none(needed, ERROR_INVALID_PARAMETER)
}

#[unsafe(no_mangle)]
pub extern "C" fn SplEnumQueueProcessor(_comp: u32, _level: u32, _buf: u32, _cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, _r: u32) -> u32 {
    empty(returned, total, needed)
}

// ---- Print jobs through the queue manager ----

/// SplQmOpen(pszToken, lCount, pqmdopData): no queue to print to.
#[unsafe(no_mangle)]
pub extern "C" fn SplQmOpen(_token: u32, _count: i32, _data: u32) -> u32 {
    SPL_ERROR
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmStartDoc(_hspl: u32, _name: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmWrite(_hspl: u32, _cb: i32, _data: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmEndDoc(_hspl: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmAbort(_hspl: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmAbortDoc(_hspl: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn SplQmClose(_hspl: u32) -> u32 {
    0
}
