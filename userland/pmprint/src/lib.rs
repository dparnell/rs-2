//! PMPRINT.QPR: the queue processor (docs/PRINTING.md). The spooler hands
//! it each job: SplQpOpen with the job's QPOPENDATA (the printer's name
//! as the logical address, driver, job properties, data type, comment,
//! parameters, document and queue names, token, job id), SplQpPrint with
//! the spool file, SplQpClose. As IBM's PMPRINT does:
//!
//! - PM_Q_STD: the spool file is a metafile. It is loaded
//!   (GpiLoadMetaFile), a presentation space on an OD_DIRECT DC of the
//!   printer takes the metafile's page setup (a first GpiPlayMetaFile
//!   with RES_RESET that draws nothing), then the document is played
//!   between DEVESC_STARTDOC and DEVESC_ENDDOC; its NEWFRAME escapes make
//!   the pages.
//! - PM_Q_RAW: the file is printer data: DEVESC_RAWDATA in 4 KB pieces on
//!   an OD_DIRECT DC opened with DATATYPE=PM_Q_RAW.
//!
//! SplQpControl(SPLC_ABORT) stops a job between pieces (raw) or with
//! GpiSetStopDraw (metafiles).

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

unsafe extern "C" {
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.605"]
    fn DevEscape(hdc: u32, code: i32, nin: i32, pin: *const u8, nout: *mut i32, pout: *mut u8) -> i32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    fn GpiAssociate(hps: u32, hdc: u32) -> u32;
    #[link_name = "PMGPI.401"]
    fn GpiLoadMetaFile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMGPI.411"]
    fn GpiPlayMetaFile(hps: u32, hmf: u32, n1: i32, opts: *const i32, segs: *mut i32, n2: i32, desc: *mut u8) -> i32;
    #[link_name = "PMGPI.375"]
    fn GpiDeleteMetaFile(hmf: u32) -> u32;
    #[link_name = "PMGPI.550"]
    fn GpiSetStopDraw(hps: u32, v: i32) -> u32;
}

const OD_DIRECT: i32 = 5;
const DEVESC_STARTDOC: i32 = 8150;
const DEVESC_ENDDOC: i32 = 8151;
const DEVESC_ABORTDOC: i32 = 8153;
const DEVESC_RAWDATA: i32 = 16303;
const DEVESC_ERROR: i32 = -1;
const PU_PELS: u32 = 0x0008;
const GPIA_ASSOC: u32 = 0x4000;
const SPLC_ABORT: i32 = 1;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// One open job (HPROC = its index + 1 in the table below).
struct Proc {
    /// QPOPENDATA strings: address, driver, data type, comment, queue
    /// processor parameters, spooler and network parameters, document.
    strs: [Vec<u8>; 8],
    drivdata: Vec<u8>,
    raw: bool,
    hps: AtomicU32,
}

const MAX: usize = 16;
static mut PROCS: [Option<Proc>; MAX] = [const { None }; MAX];
static ABORT: [AtomicU32; MAX] = [const { AtomicU32::new(0) }; MAX];

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

fn get(h: u32) -> Option<(usize, &'static Proc)> {
    let i = (h as usize).checked_sub(1)?;
    if i >= MAX {
        return None;
    }
    unsafe { (*core::ptr::addr_of!(PROCS))[i].as_ref().map(|p| (i, p)) }
}

/// SplQpOpen(cData, pQPDataIn): QPOPENDATA items 0-11.
#[unsafe(no_mangle)]
pub extern "C" fn SplQpOpen(n: i32, data: *const u32) -> u32 {
    if n < 4 || data.is_null() {
        return 0;
    }
    let item = |i: i32| -> u32 { if i < n { unsafe { data.add(i as usize).read_unaligned() } } else { 0 } };
    let s = |i: i32| cstr(item(i) as *const u8).to_vec();
    let mut datatype = s(3);
    if datatype.is_empty() {
        datatype = b"PM_Q_RAW".to_vec();
    }
    let raw = match &datatype[..] {
        b"PM_Q_STD" => false,
        b"PM_Q_RAW" => true,
        _ => return 0,
    };
    let dd = item(2) as *const u8;
    let drivdata = if dd.is_null() {
        Vec::new()
    } else {
        let cb = unsafe { (dd as *const u32).read_unaligned() } as usize;
        if (40..=0x10000).contains(&cb) { unsafe { core::slice::from_raw_parts(dd, cb) }.to_vec() } else { Vec::new() }
    };
    let p = Proc { strs: [s(0), s(1), datatype, s(4), s(5), s(6), s(7), s(8)], drivdata, raw, hps: AtomicU32::new(0) };
    unsafe {
        let t = &mut *core::ptr::addr_of_mut!(PROCS);
        let Some(i) = t.iter().position(Option::is_none) else { return 0 };
        t[i] = Some(p);
        ABORT[i].store(0, Ordering::Relaxed);
        i as u32 + 1
    }
}

/// The printer's OD_DIRECT DC (raw: with DATATYPE=PM_Q_RAW in the
/// queue processor parameters, as IBM's PMPRINT asks for it).
fn open_dc(p: &Proc) -> u32 {
    let strs: Vec<Vec<u8>> = p.strs.iter().map(|s| z(s)).collect();
    let raw = z(b"DATATYPE=PM_Q_RAW");
    let mut dop = [0u32; 9];
    dop[0] = strs[0].as_ptr() as u32;
    dop[1] = strs[1].as_ptr() as u32;
    dop[2] = if p.drivdata.is_empty() { 0 } else { p.drivdata.as_ptr() as u32 };
    dop[3] = strs[2].as_ptr() as u32;
    dop[4] = strs[3].as_ptr() as u32;
    dop[6] = if p.raw { raw.as_ptr() as u32 } else { strs[4].as_ptr() as u32 };
    dop[7] = strs[5].as_ptr() as u32;
    dop[8] = strs[6].as_ptr() as u32;
    unsafe { DevOpenDC(0, OD_DIRECT, b"*\0".as_ptr(), 9, dop.as_ptr(), 0) }
}

fn start_doc(hdc: u32, p: &Proc) -> bool {
    let doc = &p.strs[7];
    let d = z(doc);
    let n = if doc.is_empty() { 0 } else { d.len() as i32 };
    unsafe { DevEscape(hdc, DEVESC_STARTDOC, n, d.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut()) != DEVESC_ERROR }
}

fn end_doc(hdc: u32, aborted: bool) -> bool {
    let code = if aborted { DEVESC_ABORTDOC } else { DEVESC_ENDDOC };
    let mut id = [0u8; 4];
    let mut n = 2;
    unsafe { DevEscape(hdc, code, 0, core::ptr::null(), &mut n, id.as_mut_ptr()) != DEVESC_ERROR }
}

fn print_raw(i: usize, p: &Proc, file: &[u8]) -> bool {
    let Some(data) = read_file(file) else { return false };
    let hdc = open_dc(p);
    if hdc == 0 {
        return false;
    }
    let mut ok = start_doc(hdc, p);
    for chunk in data.chunks(4096) {
        if !ok || ABORT[i].load(Ordering::Relaxed) != 0 {
            break;
        }
        ok = unsafe { DevEscape(hdc, DEVESC_RAWDATA, chunk.len() as i32, chunk.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut()) } != DEVESC_ERROR;
    }
    let aborted = !ok || ABORT[i].load(Ordering::Relaxed) != 0;
    ok = end_doc(hdc, aborted) && !aborted;
    unsafe { DevCloseDC(hdc) };
    ok
}

fn print_std(i: usize, p: &Proc, file: &[u8]) -> bool {
    let name = z(file);
    let hmf = unsafe { GpiLoadMetaFile(0, name.as_ptr()) };
    if hmf == 0 {
        return false;
    }
    let hdc = open_dc(p);
    if hdc == 0 {
        unsafe { GpiDeleteMetaFile(hmf) };
        return false;
    }
    let size = [0i32, 0];
    let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), PU_PELS | GPIA_ASSOC) };
    let mut ok = hps != 0;
    if ok {
        p.hps.store(hps, Ordering::Relaxed);
        // The metafile's page setup (RES_RESET, SUP_SUPPRESS), then the
        // drawing (LT_ORIGINALVIEW, LC_LOADDISC, RES_NORESET,
        // SUP_NOSUPPRESS, CTAB_REPLACE, CREA_NOREALIZE, DDEF_LOADDISC).
        let setup = [0, 4, 0, 3, 2, 2];
        let draw = [0, 4, 0, 3, 1, 1, 3, 2, 3];
        let mut segs = 0;
        let mut desc = [0u8; 2];
        unsafe { GpiPlayMetaFile(hps, hmf, setup.len() as i32, setup.as_ptr(), &mut segs, 0, desc.as_mut_ptr()) };
        ok = start_doc(hdc, p);
        if ok && ABORT[i].load(Ordering::Relaxed) == 0 {
            ok = unsafe { GpiPlayMetaFile(hps, hmf, draw.len() as i32, draw.as_ptr(), &mut segs, 0, desc.as_mut_ptr()) } != 0;
        }
        unsafe { GpiSetStopDraw(hps, 0) };
        let aborted = !ok || ABORT[i].load(Ordering::Relaxed) != 0;
        ok = end_doc(hdc, aborted) && !aborted;
        p.hps.store(0, Ordering::Relaxed);
        unsafe {
            GpiAssociate(hps, 0);
            GpiDestroyPS(hps);
        }
    }
    unsafe {
        DevCloseDC(hdc);
        GpiDeleteMetaFile(hmf);
    }
    ok
}

/// SplQpPrint(hproc, pszFileName)
#[unsafe(no_mangle)]
pub extern "C" fn SplQpPrint(h: u32, file: *const u8) -> u32 {
    let Some((i, p)) = get(h) else { return 0 };
    let f = cstr(file);
    if f.is_empty() {
        return 0;
    }
    (if p.raw { print_raw(i, p, f) } else { print_std(i, p, f) }) as u32
}

/// SplQpClose(hproc)
#[unsafe(no_mangle)]
pub extern "C" fn SplQpClose(h: u32) -> u32 {
    let Some((i, _)) = get(h) else { return 0 };
    unsafe { (*core::ptr::addr_of_mut!(PROCS))[i] = None };
    1
}

/// SplQpControl(hproc, cmdCode): SPLC_ABORT stops the job; SPLC_PAUSE
/// and SPLC_CONTINUE are accepted (the spooler holds jobs itself).
#[unsafe(no_mangle)]
pub extern "C" fn SplQpControl(h: u32, code: i32) -> u32 {
    let Some((i, p)) = get(h) else { return 0 };
    if code == SPLC_ABORT {
        ABORT[i].store(1, Ordering::Relaxed);
        let hps = p.hps.load(Ordering::Relaxed);
        if hps != 0 {
            unsafe { GpiSetStopDraw(hps, 1) };
        }
    }
    1
}

/// SplQpQueryDt(pcDataType, paszDataTypes): the data types, copied into
/// the buffers the array points to (QP_DATATYPE_SIZE + 1 bytes each);
/// *pcDataType gives the room and gets the count.
#[unsafe(no_mangle)]
pub extern "C" fn SplQpQueryDt(pc: *mut i32, types: *mut *mut u8) -> u32 {
    if pc.is_null() {
        return 0;
    }
    let room = unsafe { pc.read_unaligned() };
    unsafe { pc.write_unaligned(2) };
    if types.is_null() || room < 2 {
        // Only the count was asked for.
        return 1;
    }
    for (k, t) in [&b"PM_Q_STD\0"[..], b"PM_Q_RAW\0"].iter().enumerate() {
        let dst = unsafe { types.add(k).read_unaligned() };
        if !dst.is_null() {
            unsafe { core::ptr::copy_nonoverlapping(t.as_ptr(), dst, t.len()) };
        }
    }
    1
}

/// SplQpQueryFlags(pulFlags): no flags (raw jobs go through PMPRINT).
#[unsafe(no_mangle)]
pub extern "C" fn SplQpQueryFlags(pfl: *mut u32) -> u32 {
    if pfl.is_null() {
        return 0;
    }
    unsafe { pfl.write_unaligned(0) };
    1
}

/// SplQpPrintSeparator(hproc, pBuf, cbBuf, ulFlags): rs2 prints no
/// separator pages.
#[unsafe(no_mangle)]
pub extern "C" fn SplQpPrintSeparator(h: u32, _buf: *const u8, _cb: u32, _fl: u32) -> u32 {
    get(h).is_some() as u32
}

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut act) = (0, 0);
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, os2::OPEN_ACTION_OPEN_IF_EXISTS, os2::OPEN_ACCESS_READONLY | os2::OPEN_SHARE_DENYNONE, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(out)
}
