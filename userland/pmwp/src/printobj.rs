//! WPPRINT's other objects, on rs2's spooler ([PRINTING.md]): print jobs
//! (WPJob), printer drivers (WPPdr), queue drivers (WPQdr), ports
//! (WPPort) and the Spooler (WPSpool), and IBM's "Create a printer"
//! dialog. All but the Spooler are transient: a printer's job view and
//! its settings pages make them for what the spooler has.
//!
//! - **Jobs**: one per spool job of the printer's queue, titled with the
//!   job's document; their pop-up menus have IBM's Change status (Hold,
//!   Release, Start again, Print next) and Delete; their settings show
//!   IBM's Submission data (date, time, owner, size, comment) and Queue
//!   options (job id, position, priority, form, status) pages. The
//!   Toolkit's methods: wpDeleteJob, wpHoldJob, wpReleaseJob,
//!   wpPrintJobNext, wpStartJobAgain, wpQueryPrintObject, wpQueryJobId,
//!   wpQueryJobFile, wpQueryJobType.
//! - **Printer drivers**: one per device of each installed driver
//!   (`PM_SPOOLER_DD`; rs2's PostScript driver: PSCRIPT.Generic
//!   PostScript Printer); opening one shows its printer properties
//!   (DevPostDeviceModes).
//! - **Queue drivers**: one per queue processor (SplEnumQueueProcessor:
//!   PMPRINT).
//! - **Ports**: one per spooler port (SplEnumPort: LPT1-3, COM1-4, FILE,
//!   LPRn, IPPn); opening one edits its settings (the output file; LPR
//!   host;queue;port; IPP host;path;port; COM baud;parity;...).
//! - **Spooler** (`<WP_SPOOL>` in System Setup): the Spool path page
//!   (`PM_SPOOLER` `DIR`, where new jobs are kept) and the Print priority
//!   page (`PM_SPOOLER` `PRIORITY`, kept), Enable and Disable spooler in
//!   its menu (`PM_SPOOLER` `SPOOL`, kept; rs2's spooler always spools).
//! - **Create a printer** (opening or dragging the Printers folder's
//!   template, IBM's dialog 20000): the name, the printer driver and the
//!   output port (or Output to file); Create makes the queue and printer,
//!   Cancel drops the new object.

#![allow(static_mut_refs)]

use alloc::vec::Vec;
use core::ptr::null;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
use crate::printer::{self, QueueInfo};
use crate::setup::{add_page, check, checked, spin, spin_init, spin_value};
use crate::som;
use crate::util::{cstr, str_bytes, str_new, z, Str};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMSPL.386"]
    fn SplDeleteJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.385"]
    fn SplHoldJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.384"]
    fn SplReleaseJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.390"]
    fn SplQueryJob(comp: *const u8, q: *const u8, id: u32, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.391"]
    fn SplSetJob(comp: *const u8, q: *const u8, id: u32, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    #[link_name = "PMSPL.392"]
    fn SplEnumJob(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.407"]
    fn SplEnumQueueProcessor(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMGPI.609"]
    fn DevPostDeviceModes(hab: u32, drv: *mut u8, name: *const u8, dev: *const u8, pname: *const u8, fl: u32) -> i32;
}

const HINI_SYSTEM: u32 = 0xffff_fffe;

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn pstr(p: u32) -> Vec<u8> {
    if p == 0 { Vec::new() } else { cstr(p as *const u8).to_vec() }
}

fn sys_str(app: &[u8], key: &[u8]) -> Vec<u8> {
    let mut buf = [0u8; 300];
    let n = unsafe { PrfQueryProfileString(HINI_SYSTEM, z(app).as_ptr(), z(key).as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    buf[..n.min(buf.len())].iter().copied().take_while(|&c| c != 0).collect()
}

fn set_sys_str(app: &[u8], key: &[u8], v: &[u8]) {
    unsafe { PrfWriteProfileString(HINI_SYSTEM, z(app).as_ptr(), z(key).as_ptr(), z(v).as_ptr()) };
}

/// A transient object of one of these classes in no folder.
fn make(class: &str, title: &[u8], style: u32) -> usize {
    let cls = wps::class(class);
    let t = z(title);
    wps::call("M_WPObject", m::M_WPObject::wpclsMakeAwake, &[cls, t.as_ptr() as usize, style as usize, 0, 0, 0, 0])
}

fn free(o: usize) {
    if o != 0 {
        // A settings notebook of it closes first.
        if let Some(w) = crate::view::find(o, OPEN_SETTINGS) {
            crate::view::close_frame(crate::view::wins()[w].frame);
        }
        som::so_call(som::so::SOM_FREE, &[o]);
    }
}

const TRANSIENT: u32 = obj::OBJSTYLE_NOMOVE | obj::OBJSTYLE_NOCOPY | obj::OBJSTYLE_NOLINK | obj::OBJSTYLE_NORENAME | obj::OBJSTYLE_NODRAG;

// ---- Containers of these objects (on settings pages) ----

struct Cnr {
    hwnd: u32,
    recs: Vec<(u32, usize, Vec<u8>)>,
}

static mut CNRS: Vec<Cnr> = Vec::new();
const REC_EXTRA: u32 = 24;
const CV_NAME: u32 = 0x0004;
const CV_MINI: u32 = 0x0020;
const CN_EMPHASIS: u32 = 4;
const CN_ENTER: u32 = 7;
const CN_CONTEXTMENU: u32 = 6;

fn cnr(h: u32) -> &'static mut Cnr {
    unsafe {
        if let Some(i) = CNRS.iter().position(|c| c.hwnd == h) {
            return &mut CNRS[i];
        }
        CNRS.retain(|c| WinIsWindow(0, c.hwnd) != 0);
        CNRS.push(Cnr { hwnd: h, recs: Vec::new() });
        CNRS.last_mut().unwrap()
    }
}

/// Show `objs` (small icons with names) in a container of a page.
fn fill(h: u32, objs: &[usize]) {
    let c = cnr(h);
    send(h, CM_REMOVERECORD, 0, mp(0, CMA_FREE | CMA_INVALIDATE));
    c.recs.clear();
    let mut ci = [0u32; 23];
    ci[0] = 92;
    ci[5] = CV_NAME | CV_MINI;
    send(h, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    for &o in objs {
        let r = send(h, CM_ALLOCRECORD, REC_EXTRA, 1);
        if r == 0 {
            break;
        }
        let mut t: Vec<u8> = obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
        t.push(0);
        for off in [R_PSZICON, R_PSZTEXT, R_PSZNAME, R_PSZTREE] {
            wr(r, off, t.as_ptr() as u32);
        }
        let icon = obj::call(m::WPObject::wpQueryIcon, &[o]) as u32;
        wr(r, R_HPTR, icon);
        wr(r, R_HPTRMINI, icon);
        wr(r, R_KEY, o as u32);
        c.recs.push((r, o, t));
        let ri: [u32; 6] = [24, CMA_END, 0, 0, 1, 1];
        send(h, CM_INSERTRECORD, r, ri.as_ptr() as u32);
    }
    unsafe { WinInvalidateRect(h, null(), 1) };
}

fn obj_of(h: u32, rec: u32) -> usize {
    cnr(h).recs.iter().find(|r| r.0 == rec).map_or(0, |r| r.1)
}

fn select(h: u32, o: usize) {
    let recs: Vec<(u32, usize)> = cnr(h).recs.iter().map(|r| (r.0, r.1)).collect();
    for (r, x) in recs {
        send(h, CM_SETRECORDEMPHASIS, r, mp((x == o) as u32, CRA_SELECTED));
    }
}

/// A container's notification on a page: CN_ENTER opens the object's
/// settings, CN_CONTEXTMENU shows its menu (WM_COMMAND to the page goes
/// to the object then, see `page_command`); a selection is returned.
fn notify(page: u32, h: u32, code: u32, b: u32) -> Option<usize> {
    match code {
        CN_ENTER => {
            let o = obj_of(h, rd(b, 8));
            if o != 0 {
                obj::call(m::WPObject::wpViewObject, &[o, 0, OPEN_SETTINGS as usize, 0]);
            }
            None
        }
        CN_CONTEXTMENU => {
            let o = obj_of(h, b);
            if o != 0 {
                obj::call(m::WPObject::wpDisplayMenu, &[o, page as usize, page as usize, 0, crate::menu::MENU_OBJECTPOPUP as usize, 0]);
            }
            None
        }
        // NOTIFYRECORDEMPHASIS: hwndCnr, pRecord, fEmphasisMask.
        CN_EMPHASIS if rd(b, 8) & CRA_SELECTED != 0 => {
            let rec = rd(b, 4);
            (rec != 0 && send(h, CM_QUERYRECORDEMPHASIS, 0xffff_ffff, CRA_SELECTED) == rec).then(|| obj_of(h, rec))
        }
        _ => None,
    }
}

/// WM_COMMAND on a page after an object's pop-up menu: the object's.
fn page_command(id: u32) -> bool {
    let (_menu, o) = unsafe { crate::view::POPUP };
    if o == 0 {
        return false;
    }
    unsafe { crate::view::POPUP = (0, 0) };
    obj::call(m::WPObject::wpMenuItemSelected, &[o, 0, id as usize]);
    true
}

// ---- Jobs ----

#[repr(C)]
pub struct JobData {
    queue: Str,
    id: u32,
    printer: usize,
}

fn jd(o: usize) -> &'static mut JobData {
    som::data(wps::ccd("WPJob"), o)
}

/// A job as PRJINFO3 tells it.
#[derive(Default, Clone, PartialEq)]
pub struct JobInfo {
    pub id: u32,
    pub priority: u16,
    pub user: Vec<u8>,
    pub position: u16,
    pub status: u16,
    pub submitted: u32,
    pub size: u32,
    pub comment: Vec<u8>,
    pub document: Vec<u8>,
    pub datatype: Vec<u8>,
}

fn job_at(b: &[u8], o: usize, level3: bool) -> JobInfo {
    JobInfo {
        id: rd16(b, o) as u32,
        priority: rd16(b, o + 2),
        user: pstr(rd32(b, o + 4)),
        position: rd16(b, o + 8),
        status: rd16(b, o + 10),
        submitted: rd32(b, o + 12),
        size: rd32(b, o + 16),
        comment: pstr(rd32(b, o + 20)),
        document: pstr(rd32(b, o + 24)),
        datatype: if level3 { pstr(rd32(b, o + 32)) } else { Vec::new() },
    }
}

/// The queue's jobs (PRJINFO2: 28 bytes each).
pub fn jobs(queue: &[u8]) -> Vec<JobInfo> {
    let q = z(queue);
    let (mut ret, mut total, mut need) = (0, 0, 0);
    unsafe { SplEnumJob(null(), q.as_ptr(), 2, core::ptr::null_mut(), 0, &mut ret, &mut total, &mut need, 0) };
    let mut buf = alloc::vec![0u8; need as usize + 64];
    if unsafe { SplEnumJob(null(), q.as_ptr(), 2, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) } != 0 {
        return Vec::new();
    }
    (0..ret as usize).map(|i| job_at(&buf, i * 28, false)).collect()
}

fn job_info(o: usize) -> Option<JobInfo> {
    let q = z(str_bytes(&jd(o).queue));
    let mut buf = alloc::vec![0u8; 4096];
    let mut need = 0;
    if unsafe { SplQueryJob(null(), q.as_ptr(), jd(o).id, 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) } != 0 {
        return None;
    }
    Some(job_at(&buf, 0, true))
}

/// IBM's status words (WPPRTMRI's strings).
pub fn job_status(s: u16) -> &'static [u8] {
    if s & 0x10 != 0 {
        return b"Hard-Error";
    }
    match s & 3 {
        0 => b"Waiting",
        1 => b"Job held",
        2 => b"Job spooling",
        _ => b"Printing",
    }
}

/// A job's title: its document (or IBM's "<untitled>").
pub fn job_title(j: &JobInfo) -> Vec<u8> {
    if j.document.is_empty() { b"<untitled>".to_vec() } else { j.document.clone() }
}

pub fn make_job(printer: usize, queue: &[u8], j: &JobInfo) -> usize {
    let o = make("WPJob", &job_title(j), TRANSIENT);
    if o != 0 {
        jd(o).queue = str_new(queue);
        jd(o).id = j.id;
        jd(o).printer = printer;
    }
    o
}

pub fn free_job(o: usize) {
    free(o);
}

fn job_call(o: usize, f: unsafe extern "C" fn(*const u8, *const u8, u32) -> u32) -> u32 {
    let q = z(str_bytes(&jd(o).queue));
    let r = (unsafe { f(null(), q.as_ptr(), jd(o).id) } == 0) as u32;
    printer::jobs_changed(jd(o).printer);
    r
}

extern "C" fn wpDeleteJob(o: usize) -> u32 {
    job_call(o, SplDeleteJob)
}

extern "C" fn wpHoldJob(o: usize) -> u32 {
    job_call(o, SplHoldJob)
}

extern "C" fn wpReleaseJob(o: usize) -> u32 {
    job_call(o, SplReleaseJob)
}

/// Print next: the job moves to the front of the queue.
extern "C" fn wpPrintJobNext(o: usize) -> u32 {
    let q = z(str_bytes(&jd(o).queue));
    let pos = 1u16.to_le_bytes();
    let r = (unsafe { SplSetJob(null(), q.as_ptr(), jd(o).id, 3, pos.as_ptr(), 2, 6) } == 0) as u32;
    printer::jobs_changed(jd(o).printer);
    r
}

/// Start again: the job goes back to the front, released (rs2's spooler
/// cannot stop a job that is printing).
extern "C" fn wpStartJobAgain(o: usize) -> u32 {
    wpPrintJobNext(o);
    wpReleaseJob(o)
}

extern "C" fn wpQueryPrintObject(o: usize) -> usize {
    jd(o).printer
}

extern "C" fn wpQueryJobId(o: usize) -> u32 {
    jd(o).id
}

fn copy_out(v: &[u8], buf: *mut u8, cb: *mut u32) -> u32 {
    if cb.is_null() {
        return 0;
    }
    unsafe {
        if buf.is_null() || (*cb as usize) <= v.len() {
            *cb = v.len() as u32 + 1;
            return 0;
        }
        core::ptr::copy_nonoverlapping(v.as_ptr(), buf, v.len());
        *buf.add(v.len()) = 0;
    }
    1
}

/// The job's spool file: `\SPOOL\queue\nnnnn.SPL` in the spool directory.
extern "C" fn wpQueryJobFile(o: usize, buf: *mut u8, cb: *mut u32) -> u32 {
    let mut dir = sys_str(b"PM_SPOOLER", b"DIR");
    if let Some(i) = dir.iter().position(|&c| c == b';') {
        dir.truncate(i);
    }
    if dir.is_empty() {
        dir = b"C:\\SPOOL".to_vec();
    }
    let f = [&dir[..], b"\\", str_bytes(&jd(o).queue), alloc::format!("\\{:05}.SPL", jd(o).id).as_bytes()].concat();
    copy_out(&f, buf, cb)
}

/// The job's data type (PM_Q_STD, PM_Q_RAW).
extern "C" fn wpQueryJobType(o: usize, buf: *mut u8, cb: *mut u32) -> u32 {
    let t = job_info(o).map(|j| j.datatype).unwrap_or_default();
    copy_out(&t, buf, cb)
}

// The pop-up menu: IBM's Change status submenu.
const WPPOIDM_JOB_STATUS: u32 = 5013;
const WPPOIDM_JOB_HOLD: u32 = 5009;
const WPPOIDM_JOB_RELEASE: u32 = 5008;
const WPPOIDM_JOB_STARTAGAIN: u32 = 5006;
const WPPOIDM_JOB_PRINTNEXT: u32 = 5007;

extern "C" fn job_filter_menu(_o: usize, flags: u32, _cnr: u32, _multi: u32) -> u32 {
    flags & (crate::menu::CTXT_OPEN | crate::menu::CTXT_SETTINGS | crate::menu::CTXT_DELETE | crate::menu::CTXT_HELP)
}

#[repr(C)]
#[derive(Default)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    item: u32,
}

extern "C" fn job_modify_menu(o: usize, menu: u32, _cnr: u32, _pos: u32) -> u32 {
    let held = job_info(o).is_some_and(|j| j.status & 3 == 1);
    let sub = popup_menu(&[
        Menu::Item(WPPOIDM_JOB_HOLD as u16, b"~Hold", !held),
        Menu::Item(WPPOIDM_JOB_RELEASE as u16, b"~Release", held),
        Menu::Item(WPPOIDM_JOB_STARTAGAIN as u16, b"~Start again", true),
        Menu::Item(WPPOIDM_JOB_PRINTNEXT as u16, b"~Print next", true),
    ]);
    // MIS_TEXT | MIS_SUBMENU, at the end.
    let mi = MenuItem { pos: -1, style: 0x0011, attr: 0, id: WPPOIDM_JOB_STATUS as u16, sub, item: 0 };
    send(menu, 0x0180, &mi as *const MenuItem as u32, b"Ch~ange status\0".as_ptr() as u32);
    1
}

extern "C" fn job_menu_selected(o: usize, frame: u32, id: u32) -> u32 {
    match id {
        WPPOIDM_JOB_HOLD => wpHoldJob(o),
        WPPOIDM_JOB_RELEASE => wpReleaseJob(o),
        WPPOIDM_JOB_STARTAGAIN => wpStartJobAgain(o),
        WPPOIDM_JOB_PRINTNEXT => wpPrintJobNext(o),
        _ => som::call_parent(wps::ccd("WPJob"), wps::cd("WPObject"), m::WPObject::wpMenuItemSelected, &[o, frame as usize, id as usize]) as u32,
    }
}

/// Deleting a job object deletes the spool job.
extern "C" fn job_delete(o: usize, _fl: u32) -> u32 {
    wpDeleteJob(o);
    1
}

/// Jobs look like documents.
extern "C" fn job_icon(_o: usize) -> u32 {
    wps::call("M_WPObject", m::M_WPObject::wpclsQueryIcon, &[wps::class("WPDataFile")]) as u32
}

extern "C" fn job_uninit(o: usize) {
    jd(o).queue = str_new(b"");
    som::call_parent(wps::ccd("WPJob"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[o]);
}

// The job's settings: Submission data (IBM's 20800) and Queue options
// (20900).
const ID_JDATE: u16 = 20801;
const ID_JTIME: u16 = 20802;
const ID_JOWNER: u16 = 20803;
const ID_JCOMMENT: u16 = 20804;
const ID_JSIZE: u16 = 20805;
const ID_JID: u16 = 20901;
const ID_JPRIO: u16 = 20903;
const ID_JFORM: u16 = 20904;
const ID_JSTATUS: u16 = 20905;
const ID_JPOS: u16 = 20906;
const SPBN_CHANGE: u32 = 0x20d;

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

fn dec(n: u32) -> Vec<u8> {
    alloc::format!("{n}").into_bytes()
}

extern "C" fn job_data_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            if let Some(j) = job_info(b as usize) {
                // ulSubmitted: seconds since 1970 (as IBM's spooler keeps it).
                let t = j.submitted;
                let days = t / 86400;
                let (y, mo, d) = civil(days as i64);
                set_item_text(h, ID_JDATE as u32, alloc::format!("{mo:02}-{d:02}-{y:04}").as_bytes());
                set_item_text(h, ID_JTIME as u32, alloc::format!("{:02}:{:02}:{:02}", t % 86400 / 3600, t % 3600 / 60, t % 60).as_bytes());
                set_item_text(h, ID_JOWNER as u32, &j.user);
                set_item_text(h, ID_JSIZE as u32, &dec(j.size));
                set_item_text(h, ID_JCOMMENT as u32, &j.comment);
            }
            0
        }
        // EN_KILLFOCUS on the comment: kept with the job.
        WM_CONTROL if a & 0xffff == ID_JCOMMENT as u32 && a >> 16 == 2 => {
            let o = page_obj(h);
            let c = z(&item_text(h, ID_JCOMMENT as u32));
            let q = z(str_bytes(&jd(o).queue));
            unsafe { SplSetJob(null(), q.as_ptr(), jd(o).id, 3, c.as_ptr(), c.len() as u32, 11) };
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// Days since 1970-01-01 as (year, month, day).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

extern "C" fn job_queue_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            if let Some(j) = job_info(b as usize) {
                set_item_text(h, ID_JID as u32, &dec(j.id));
                set_item_text(h, ID_JPOS as u32, &dec(j.position as u32));
                spin_init(h, ID_JPRIO, 1, 99, j.priority as i32);
                set_item_text(h, ID_JFORM as u32, b"");
                set_item_text(h, ID_JSTATUS as u32, job_status(j.status));
            }
            0
        }
        WM_CONTROL if a & 0xffff == ID_JPRIO as u32 && a >> 16 == SPBN_CHANGE => {
            let o = page_obj(h);
            if let Some(p) = spin_value(h, ID_JPRIO).filter(|p| (1..=99).contains(p)) {
                let q = z(str_bytes(&jd(o).queue));
                let v = (p as u16).to_le_bytes();
                unsafe { SplSetJob(null(), q.as_ptr(), jd(o).id, 3, v.as_ptr(), 2, 14) };
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn ro(y: i16, x: i16, cx: i16, id: u16) -> Item {
    // ES_AUTOSCROLL | ES_MARGIN | ES_READONLY
    item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_GROUP | 0x0004 | 0x0008 | 0x0020, (x, y, cx, 8), id)
}

extern "C" fn job_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPJob"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    add_page(o, nb, job_queue_proc, alloc::vec![
        book::label(b"Job identifier", (6, 100, 60)),
        ro(100, 70, 40, ID_JID),
        book::label(b"Job position", (6, 86, 60)),
        ro(86, 70, 40, ID_JPOS),
        book::label(b"Priority", (124, 100, 40)),
        spin((166, 98, 40), ID_JPRIO),
        book::label(b"Form name", (6, 70, 60)),
        ro(70, 70, 140, ID_JFORM),
        book::label(b"Job status", (6, 54, 60)),
        ro(54, 70, 140, ID_JSTATUS),
    ], b"~Queue options\0");
    add_page(o, nb, job_data_proc, alloc::vec![
        book::label(b"Date", (6, 100, 40)),
        ro(100, 50, 70, ID_JDATE),
        book::label(b"Time", (6, 86, 40)),
        ro(86, 50, 70, ID_JTIME),
        book::label(b"Owner", (6, 72, 40)),
        ro(72, 50, 70, ID_JOWNER),
        book::label(b"Size", (6, 58, 40)),
        ro(58, 50, 70, ID_JSIZE),
        book::label(b"bytes", (124, 58, 30)),
        book::label(b"Comment", (6, 44, 40)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (50, 44, 170, 8), ID_JCOMMENT),
    ], b"~Submission data\0")
}

// ---- Printer drivers ----

#[repr(C)]
pub struct PdrData {
    /// DRIVER.Device
    name: Str,
}

fn pdd(o: usize) -> &'static mut PdrData {
    som::data(wps::ccd("WPPdr"), o)
}

pub fn pdr_name(o: usize) -> Vec<u8> {
    str_bytes(&pdd(o).name).to_vec()
}

/// The installed printer drivers' devices (`PM_SPOOLER_DD`), as
/// DRIVER.Device; rs2's PostScript driver is always there.
pub fn installed_devices() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = crate::util::prf_keys(HINI_SYSTEM, b"PM_SPOOLER_DD").into_iter().filter(|k| k.contains(&b'.')).collect();
    let ps = b"PSCRIPT.Generic PostScript Printer".to_vec();
    if !v.contains(&ps) {
        v.insert(0, ps);
    }
    v
}

pub fn make_pdr(name: &[u8]) -> usize {
    let title = name.splitn(2, |&c| c == b'.').nth(1).unwrap_or(name);
    let o = make("WPPdr", title, TRANSIENT | obj::OBJSTYLE_NODELETE);
    if o != 0 {
        pdd(o).name = str_new(name);
    }
    o
}

/// Opening a printer driver: its printer properties (the driver's dialog).
extern "C" fn pdr_open(o: usize, _cnr: u32, view: u32, _param: u32) -> u32 {
    let _ = view;
    let n = pdr_name(o);
    let mut parts = n.splitn(2, |&c| c == b'.');
    let (drv, dev) = (parts.next().unwrap_or(b"PSCRIPT").to_vec(), parts.next().unwrap_or(b"").to_vec());
    let (d, v) = (z(&drv), z(&dev));
    unsafe { DevPostDeviceModes(WinQueryAnchorBlock(HWND_DESKTOP), core::ptr::null_mut(), d.as_ptr(), v.as_ptr(), b"\0".as_ptr(), 1) };
    1
}

extern "C" fn pdr_filter_menu(_o: usize, flags: u32, _cnr: u32, _multi: u32) -> u32 {
    flags & (crate::menu::CTXT_OPEN | crate::menu::CTXT_SETTINGS | crate::menu::CTXT_HELP)
}

extern "C" fn pdr_icon(_o: usize) -> u32 {
    wps::call("M_WPObject", m::M_WPObject::wpclsQueryIcon, &[wps::class("WPPrinter")]) as u32
}

// ---- Queue drivers ----

#[repr(C)]
pub struct QdrData {
    name: Str,
}

fn qdd(o: usize) -> &'static mut QdrData {
    som::data(wps::ccd("WPQdr"), o)
}

pub fn qdr_name(o: usize) -> Vec<u8> {
    str_bytes(&qdd(o).name).to_vec()
}

/// The queue processors (SplEnumQueueProcessor level 0: 9-byte names).
pub fn queue_drivers() -> Vec<Vec<u8>> {
    let (mut ret, mut total, mut need) = (0, 0, 0);
    let mut buf = [0u8; 512];
    if unsafe { SplEnumQueueProcessor(null(), 0, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) } != 0 || ret == 0 {
        return alloc::vec![b"PMPRINT".to_vec()];
    }
    (0..ret as usize).map(|i| buf[i * 9..i * 9 + 9].iter().copied().take_while(|&c| c != 0).collect()).collect()
}

pub fn make_qdr(name: &[u8]) -> usize {
    let o = make("WPQdr", name, TRANSIENT | obj::OBJSTYLE_NODELETE);
    if o != 0 {
        qdd(o).name = str_new(name);
    }
    o
}

extern "C" fn qdr_open(_o: usize, _cnr: u32, _view: u32, _param: u32) -> u32 {
    1
}

// ---- Ports ----

#[repr(C)]
pub struct PortData {
    name: Str,
}

fn ptd(o: usize) -> &'static mut PortData {
    som::data(wps::ccd("WPPort"), o)
}

pub fn port_name(o: usize) -> Vec<u8> {
    str_bytes(&ptd(o).name).to_vec()
}

pub fn make_port(name: &[u8]) -> usize {
    let o = make("WPPort", name, TRANSIENT | obj::OBJSTYLE_NODELETE);
    if o != 0 {
        ptd(o).name = str_new(name);
    }
    o
}

/// What a port's settings hold.
pub fn port_hint(port: &[u8]) -> &'static [u8] {
    let p = port.to_ascii_uppercase();
    if p.starts_with(b"FILE") {
        b"The file the output goes to (empty: C:\\SPOOL\\OUTPUT\\job.PS)."
    } else if p.starts_with(b"LPR") {
        b"host;queue;port (port 515 if not given)"
    } else if p.starts_with(b"IPP") {
        b"host;path;port (/ipp/print and 631 if not given)"
    } else if p.starts_with(b"COM") {
        b"baud;parity;data bits;stop bits;handshake"
    } else {
        b"The parallel port: no settings."
    }
}

// The port's settings dialog (IBM opens a port's own dialog).
const ID_PSET: u16 = 101;
const ID_PHINT: u16 = 102;
static mut PORT_EDIT: Vec<u8> = Vec::new();

extern "C" fn port_dlg_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let p = unsafe { PORT_EDIT.clone() };
            let s = printer::port_settings(&p);
            set_item_text(h, ID_PSET as u32, s.strip_suffix(b";").unwrap_or(&s));
            set_item_text(h, ID_PHINT as u32, port_hint(&p));
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_PSET as u32)) };
            1
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            let p = unsafe { PORT_EDIT.clone() };
            printer::set_port_settings(&p, &item_text(h, ID_PSET as u32));
            unsafe { WinDismissDlg(h, DID_OK) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// Opening a port: its settings.
extern "C" fn port_open(o: usize, _cnr: u32, _view: u32, _param: u32) -> u32 {
    let p = port_name(o);
    unsafe { PORT_EDIT = p.clone() };
    let mut title = b"Settings - ".to_vec();
    title.extend_from_slice(&p);
    let tpl = template(dialog_frame(&title, 230, 70), alloc::vec![
        book::label(b"Port settings:", (6, 52, 100)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 40, 214, 8), ID_PSET),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001 | 0x4000, (6, 22, 218, 16), ID_PHINT),
        item(WC_BUTTON, b"~OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 50, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (62, 4, 50, 14), DID_CANCEL as u16),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, HWND_DESKTOP, port_dlg_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg != 0 {
        unsafe {
            WinProcessDlg(dlg);
            WinDestroyWindow(dlg);
        }
    }
    1
}

extern "C" fn port_icon(_o: usize) -> u32 {
    wps::call("M_WPObject", m::M_WPObject::wpclsQueryIcon, &[wps::class("WPDisk")]) as u32
}

// Install new port: another LPR, IPP or FILE port.
const ID_KIND: u16 = 201;

extern "C" fn new_port_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            check(h, ID_KIND, true);
            1
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            let kind: &[u8] = if checked(h, ID_KIND) { b"LPR" } else if checked(h, ID_KIND + 1) { b"IPP" } else { b"FILE" };
            let existing = printer::port_names();
            let name = (1..100).map(|n| [kind, alloc::format!("{n}").as_bytes()].concat()).find(|n| !existing.contains(n));
            if let Some(n) = name {
                printer::create_port(&n);
            }
            unsafe { WinDismissDlg(h, DID_OK) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// IBM's Install New Port, for the port kinds rs2's spooler has.
pub fn install_port(owner: u32) {
    let tpl = template(dialog_frame(b"Install New Port", 200, 86), alloc::vec![
        book::label(b"Output port", (6, 70, 100)),
        crate::setup::radio(b"~LPR network printer (RFC 1179)", (10, 56, 180), ID_KIND, true),
        crate::setup::radio(b"~IPP network printer", (10, 44, 180), ID_KIND + 1, false),
        crate::setup::radio(b"A ~file", (10, 32, 180), ID_KIND + 2, false),
        item(WC_BUTTON, b"~Install", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 50, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (62, 4, 50, 14), DID_CANCEL as u16),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, new_port_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg != 0 {
        unsafe {
            WinProcessDlg(dlg);
            WinDestroyWindow(dlg);
        }
    }
}

// ---- The printer's pages: Printer driver, Output port, Queue options ----

const ID_INSTALLED: u16 = 20301;
const ID_DEFDRV: u16 = 20302;
const ID_JOBPROPS: u16 = 1108;
const ID_PORTS: u16 = 20201;
const ID_TOFILE: u16 = 20202;
const ID_NEWPORT: u16 = 20203;
const ID_QPRIO: u16 = 20506;
const ID_QDRV: u16 = 20501;
const ID_QDIALOG: u16 = 20502;
const ID_QRAW: u16 = 20503;
const ID_QBYPASS: u16 = 20505;
const ID_DEFPRN: u16 = 20310;

/// The transient objects a page made (freed when the page goes).
static mut PAGE_OBJS: Vec<(u32, Vec<usize>)> = Vec::new();

fn keep(page: u32, objs: &[usize]) {
    unsafe {
        if let Some(e) = PAGE_OBJS.iter_mut().find(|e| e.0 == page) {
            let old = core::mem::replace(&mut e.1, objs.to_vec());
            for o in old.into_iter().filter(|o| !objs.contains(o)) {
                free(o);
            }
        } else {
            PAGE_OBJS.push((page, objs.to_vec()));
        }
    }
}

fn page_gone(page: u32) {
    unsafe {
        if let Some(i) = PAGE_OBJS.iter().position(|e| e.0 == page) {
            for o in PAGE_OBJS.remove(i).1 {
                free(o);
            }
        }
    }
}

fn queue_of(o: usize) -> Vec<u8> {
    let mut buf = [0u8; 64];
    wps::call("WPPrinter", m::WPPrinter::wpQueryPrinterName, &[o, buf.as_mut_ptr() as usize]);
    cstr(buf.as_ptr()).to_vec()
}

fn info(o: usize) -> Option<QueueInfo> {
    printer::queue_info(&queue_of(o))
}

fn fill_drivers(h: u32, o: usize) {
    let devs = installed_devices();
    let objs: Vec<usize> = devs.iter().map(|d| make_pdr(d)).filter(|&x| x != 0).collect();
    fill(unsafe { WinWindowFromID(h, ID_INSTALLED as u32) }, &objs);
    let cur = info(o).map(|i| i.driver).unwrap_or_default();
    let def: Vec<usize> = objs.iter().copied().filter(|&x| pdr_name(x) == cur).collect();
    let mut all = objs.clone();
    // The default's own object in the second container.
    let d2: Vec<usize> = def.iter().map(|&x| make_pdr(&pdr_name(x))).collect();
    fill(unsafe { WinWindowFromID(h, ID_DEFDRV as u32) }, &d2);
    if let Some(&d) = def.first() {
        select(unsafe { WinWindowFromID(h, ID_INSTALLED as u32) }, d);
    }
    all.extend(d2);
    keep(h, &all);
}

extern "C" fn driver_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            fill_drivers(h, b as usize);
            check(h, ID_DEFPRN, info(b as usize).is_some_and(|i| i.flags & 4 != 0));
            0
        }
        WM_DESTROY => {
            page_gone(h);
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_INSTALLED || (a & 0xffff) as u16 == ID_DEFDRV => {
            let c = unsafe { WinWindowFromID(h, a & 0xffff) };
            if let Some(d) = notify(h, c, a >> 16, b) {
                // A driver chosen in the installed ones: the printer's.
                if (a & 0xffff) as u16 == ID_INSTALLED {
                    let o = page_obj(h);
                    printer::set_driver(&queue_of(o), &pdr_name(d));
                    fill_drivers(h, o);
                }
            }
            0
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_DEFPRN && a >> 16 == 1 => {
            if checked(h, ID_DEFPRN) {
                printer::set_default(&queue_of(page_obj(h)));
            }
            0
        }
        WM_COMMAND if (a & 0xffff) as u16 == ID_JOBPROPS => {
            printer::job_properties(page_obj(h));
            0
        }
        WM_COMMAND => {
            page_command(a & 0xffff);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn container(id: u16, (x, y, cx, cy): (i16, i16, i16, i16)) -> Item {
    // CCS_SINGLESEL | CCS_AUTOPOSITION
    let mut i = item(WC_STATIC, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001 | 0x0008, (x, y, cx, cy), id);
    i.class = 0x25;
    i
}

pub extern "C" fn wpAddPrinterDriverPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, driver_page_proc, alloc::vec![
        book::label(b"Installed printer drivers", (6, 108, 120)),
        container(ID_INSTALLED, (6, 66, 224, 40)),
        book::label(b"Default printer driver", (6, 54, 100)),
        container(ID_DEFDRV, (6, 22, 224, 30)),
        item(WC_BUTTON, b"~Job properties...", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (150, 52, 80, 13), ID_JOBPROPS),
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"~Default printer", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (6, 6, 120, 10), ID_DEFPRN),
    ], b"~Printer driver\0")
}

fn fill_ports(h: u32, o: usize) {
    let objs: Vec<usize> = printer::port_names().iter().map(|p| make_port(p)).filter(|&x| x != 0).collect();
    let c = unsafe { WinWindowFromID(h, ID_PORTS as u32) };
    fill(c, &objs);
    let cur = printer::port_of(&queue_of(o));
    if let Some(&p) = objs.iter().find(|&&x| port_name(x) == cur) {
        select(c, p);
    }
    check(h, ID_TOFILE, cur.to_ascii_uppercase().starts_with(b"FILE"));
    keep(h, &objs);
}

extern "C" fn port_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            fill_ports(h, b as usize);
            0
        }
        WM_DESTROY => {
            page_gone(h);
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_PORTS => {
            let c = unsafe { WinWindowFromID(h, ID_PORTS as u32) };
            if let Some(p) = notify(h, c, a >> 16, b) {
                let o = page_obj(h);
                let name = port_name(p);
                if printer::port_of(&queue_of(o)) != name {
                    printer::set_port(&queue_of(o), &name);
                }
                check(h, ID_TOFILE, name.to_ascii_uppercase().starts_with(b"FILE"));
            }
            0
        }
        // Output to file: the FILE port.
        WM_CONTROL if (a & 0xffff) as u16 == ID_TOFILE && a >> 16 == 1 => {
            let o = page_obj(h);
            if checked(h, ID_TOFILE) {
                printer::set_port(&queue_of(o), b"FILE");
            } else if let Some(p) = printer::port_names().into_iter().find(|p| p.starts_with(b"LPT")) {
                printer::set_port(&queue_of(o), &p);
            }
            fill_ports(h, o);
            0
        }
        WM_COMMAND if (a & 0xffff) as u16 == ID_NEWPORT => {
            install_port(h);
            fill_ports(h, page_obj(h));
            0
        }
        WM_COMMAND => {
            page_command(a & 0xffff);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

pub extern "C" fn wpAddPrinterPortPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, port_page_proc, alloc::vec![
        book::label(b"Output port", (6, 108, 100)),
        container(ID_PORTS, (6, 36, 224, 70)),
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"Output to ~file", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (6, 22, 100, 10), ID_TOFILE),
        item(WC_BUTTON, b"~Install new port...", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (6, 4, 90, 13), ID_NEWPORT),
        book::label(b"Open a port to change its settings.", (100, 6, 130)),
    ], b"~Output port\0")
}

/// PRQINFO3 fsType: PRQ3_TYPE_RAW (printer-specific format),
/// PRQ3_TYPE_BYPASS (print while spooling); rs2 keeps "job dialog before
/// print" in 0x0008 (IBM's PRQ3_TYPE_APPDEFAULT is 0x0004).
const PRQ3_TYPE_RAW: u16 = 0x0001;
const PRQ3_TYPE_BYPASS: u16 = 0x0002;
const TYPE_JOBDIALOG: u16 = 0x0008;

extern "C" fn queue_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let o = b as usize;
            let (prio, flags) = printer::queue_priority_flags(&queue_of(o));
            spin_init(h, ID_QPRIO, 1, 9, prio as i32);
            check(h, ID_QRAW, flags & PRQ3_TYPE_RAW != 0);
            check(h, ID_QBYPASS, flags & PRQ3_TYPE_BYPASS != 0);
            check(h, ID_QDIALOG, flags & TYPE_JOBDIALOG != 0);
            let objs: Vec<usize> = queue_drivers().iter().map(|q| make_qdr(q)).filter(|&x| x != 0).collect();
            let c = unsafe { WinWindowFromID(h, ID_QDRV as u32) };
            fill(c, &objs);
            let cur = printer::queue_driver(&queue_of(o));
            if let Some(&q) = objs.iter().find(|&&x| qdr_name(x).eq_ignore_ascii_case(&cur)) {
                select(c, q);
            }
            keep(h, &objs);
            0
        }
        WM_DESTROY => {
            page_gone(h);
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_CONTROL => {
            let o = page_obj(h);
            let q = queue_of(o);
            match ((a & 0xffff) as u16, a >> 16) {
                (ID_QPRIO, SPBN_CHANGE) => {
                    if let Some(p) = spin_value(h, ID_QPRIO).filter(|p| (1..=9).contains(p)) {
                        printer::set_queue_priority(&q, p as u16);
                    }
                }
                (ID_QRAW | ID_QBYPASS | ID_QDIALOG, 1) => {
                    let mut f = printer::queue_priority_flags(&q).1 & !(PRQ3_TYPE_RAW | PRQ3_TYPE_BYPASS | TYPE_JOBDIALOG);
                    if checked(h, ID_QRAW) {
                        f |= PRQ3_TYPE_RAW;
                    }
                    if checked(h, ID_QBYPASS) {
                        f |= PRQ3_TYPE_BYPASS;
                    }
                    if checked(h, ID_QDIALOG) {
                        f |= TYPE_JOBDIALOG;
                    }
                    printer::set_queue_flags(&q, f);
                }
                (ID_QDRV, code) => {
                    let c = unsafe { WinWindowFromID(h, ID_QDRV as u32) };
                    if let Some(d) = notify(h, c, code, b) {
                        printer::set_queue_driver(&q, &qdr_name(d));
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            page_command(a & 0xffff);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

pub extern "C" fn wpAddQueueOptionsPage(o: usize, nb: u32) -> u32 {
    // BS_AUTOCHECKBOX
    let cb = |t: &[u8], y: i16, id: u16| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (6, y, 160, 10), id);
    add_page(o, nb, queue_page_proc, alloc::vec![
        book::label(b"Queue priority (1 low - 9 high)", (6, 108, 120)),
        spin((130, 106, 30), ID_QPRIO),
        book::label(b"Queue driver", (6, 94, 100)),
        container(ID_QDRV, (6, 52, 224, 40)),
        cb(b"~Job dialog before print", 36, ID_QDIALOG),
        cb(b"Printer-specific ~format", 24, ID_QRAW),
        cb(b"Print while ~spooling", 12, ID_QBYPASS),
    ], b"~Queue options\0")
}

// ---- The Spooler ----

const ID_SPATH: u16 = 22001;
const ID_SPRIO: u16 = 22401;

pub fn spool_path() -> Vec<u8> {
    let mut d = sys_str(b"PM_SPOOLER", b"DIR");
    if let Some(i) = d.iter().position(|&c| c == b';') {
        d.truncate(i);
    }
    if d.is_empty() { b"C:\\SPOOL".to_vec() } else { d }
}

pub fn spooler_enabled() -> bool {
    sys_str(b"PM_SPOOLER", b"SPOOL").first() != Some(&b'0')
}

extern "C" fn spool_path_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            set_item_text(h, ID_SPATH as u32, &spool_path());
            0
        }
        // EN_KILLFOCUS: the new path (made if it does not exist).
        WM_CONTROL if (a & 0xffff) as u16 == ID_SPATH && a >> 16 == 2 => {
            let p = item_text(h, ID_SPATH as u32);
            if p.len() >= 3 && p != spool_path() {
                let pz = z(&p);
                unsafe { os2::DosCreateDir(pz.as_ptr(), core::ptr::null_mut()) };
                set_sys_str(b"PM_SPOOLER", b"DIR", &[&p[..], b";"].concat());
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSetPathPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, spool_path_proc, alloc::vec![
        book::label(b"Type the new path for storing spool files.", (6, 100, 220)),
        book::label(b"This change is effective immediately.", (6, 90, 220)),
        book::label(b"Spool path", (6, 72, 50)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (60, 72, 160, 8), ID_SPATH),
    ], b"~Spool path\0")
}

extern "C" fn spool_prio_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let p = crate::util::parse_dec(&sys_str(b"PM_SPOOLER", b"PRIORITY").into_iter().take_while(|&c| c != b';').collect::<Vec<u8>>()).unwrap_or(200);
            spin_init(h, ID_SPRIO, 1, 300, p as i32);
            0
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_SPRIO && a >> 16 == SPBN_CHANGE => {
            if let Some(p) = spin_value(h, ID_SPRIO).filter(|p| (1..=300).contains(p)) {
                set_sys_str(b"PM_SPOOLER", b"PRIORITY", alloc::format!("{p};").as_bytes());
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSetPriorityPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, spool_prio_proc, alloc::vec![
        book::label(b"Print priority (1 low - 300 high)", (6, 100, 140)),
        spin((150, 98, 40), ID_SPRIO),
    ], b"~Print priority\0")
}

extern "C" fn spool_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPSpool"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wpAddSetPriorityPage(o, nb);
    wpAddSetPathPage(o, nb);
    1
}

// The menu: Enable spooler / Disable spooler (IBM's ids).
const WPPOIDM_SPOOL_ENABLE: u32 = 5101;
const WPPOIDM_SPOOL_DISABLE: u32 = 5102;

extern "C" fn spool_modify_menu(_o: usize, menu: u32, _cnr: u32, _pos: u32) -> u32 {
    let (id, t): (u32, &[u8]) = if spooler_enabled() { (WPPOIDM_SPOOL_DISABLE, b"~Disable spooler\0") } else { (WPPOIDM_SPOOL_ENABLE, b"~Enable spooler\0") };
    let mi = MenuItem { pos: -1, style: 0x0001, attr: 0, id: id as u16, sub: 0, item: 0 };
    send(menu, 0x0180, &mi as *const MenuItem as u32, t.as_ptr() as u32);
    1
}

extern "C" fn spool_menu_selected(o: usize, frame: u32, id: u32) -> u32 {
    match id {
        WPPOIDM_SPOOL_ENABLE => set_sys_str(b"PM_SPOOLER", b"SPOOL", b"1;"),
        WPPOIDM_SPOOL_DISABLE => set_sys_str(b"PM_SPOOLER", b"SPOOL", b"0;"),
        _ => return som::call_parent(wps::ccd("WPSpool"), wps::cd("WPObject"), m::WPObject::wpMenuItemSelected, &[o, frame as usize, id as usize]) as u32,
    }
    1
}

/// The Spooler opens its settings.
extern "C" fn spool_default_view(_o: usize) -> u32 {
    OPEN_SETTINGS
}

// ---- Create a printer (IBM's dialog 20000) ----

const ID_CNAME: u16 = 20001;
const ID_CDRV: u16 = 20002;
const ID_CPORT: u16 = 20003;
const ID_CFILE: u16 = 20004;
const ID_CREATE: u16 = 1109;
const ID_CINSTALL: u16 = 20005;

struct Create {
    title: Vec<u8>,
    driver: Vec<u8>,
    port: Vec<u8>,
    objs: Vec<usize>,
    done: bool,
}

static mut CREATE: Create = Create { title: Vec::new(), driver: Vec::new(), port: Vec::new(), objs: Vec::new(), done: false };

fn create_fill(h: u32) {
    let c = unsafe { &mut CREATE };
    for o in core::mem::take(&mut c.objs) {
        free(o);
    }
    let drv: Vec<usize> = installed_devices().iter().map(|d| make_pdr(d)).filter(|&x| x != 0).collect();
    let dc = unsafe { WinWindowFromID(h, ID_CDRV as u32) };
    fill(dc, &drv);
    if let Some(&d) = drv.iter().find(|&&x| pdr_name(x) == c.driver).or(drv.first()) {
        select(dc, d);
        c.driver = pdr_name(d);
    }
    let ports: Vec<usize> = printer::port_names().iter().map(|p| make_port(p)).filter(|&x| x != 0).collect();
    let pc = unsafe { WinWindowFromID(h, ID_CPORT as u32) };
    fill(pc, &ports);
    if let Some(&p) = ports.iter().find(|&&x| port_name(x) == c.port) {
        select(pc, p);
    }
    check(h, ID_CFILE, c.port.to_ascii_uppercase().starts_with(b"FILE"));
    c.objs = drv.into_iter().chain(ports).collect();
}

extern "C" fn create_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    let c = unsafe { &mut CREATE };
    match msg {
        WM_INITDLG => {
            set_item_text(h, ID_CNAME as u32, &c.title);
            create_fill(h);
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_CNAME as u32)) };
            1
        }
        WM_CONTROL => {
            let id = (a & 0xffff) as u16;
            match id {
                ID_CDRV | ID_CPORT => {
                    let cn = unsafe { WinWindowFromID(h, id as u32) };
                    if let Some(o) = notify(h, cn, a >> 16, b) {
                        if id == ID_CDRV {
                            c.driver = pdr_name(o);
                        } else {
                            c.port = port_name(o);
                            check(h, ID_CFILE, c.port.to_ascii_uppercase().starts_with(b"FILE"));
                        }
                    }
                }
                ID_CFILE if a >> 16 == 1 => {
                    if checked(h, ID_CFILE) {
                        c.port = b"FILE".to_vec();
                    } else if c.port.to_ascii_uppercase().starts_with(b"FILE") {
                        c.port = printer::port_names().into_iter().find(|p| p.starts_with(b"LPT")).unwrap_or_default();
                    }
                    create_fill(h);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match (a & 0xffff) as u16 {
                ID_CREATE => {
                    let t = item_text(h, ID_CNAME as u32);
                    if !t.is_empty() && !c.port.is_empty() {
                        c.title = t;
                        c.done = true;
                        unsafe { WinDismissDlg(h, DID_OK) };
                    } else {
                        unsafe { WinAlarm(HWND_DESKTOP, 2) };
                    }
                }
                ID_CINSTALL => {
                    install_driver(h);
                    create_fill(h);
                }
                x if x as u32 == DID_CANCEL => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
                x => {
                    page_command(x as u32);
                }
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// The "Create a printer" dialog for a printer made from the template:
/// Some(title, driver, port) when Create was pressed.
pub fn create_dialog(title: &[u8]) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    unsafe {
        CREATE = Create { title: title.to_vec(), driver: b"PSCRIPT.Generic PostScript Printer".to_vec(), port: b"FILE".to_vec(), objs: Vec::new(), done: false };
    }
    let tpl = template(dialog_frame(b"Create a printer", 338, 150), alloc::vec![
        book::label(b"Name", (12, 135, 33)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (51, 135, 112, 8), ID_CNAME),
        book::label(b"Default printer driver", (12, 122, 153)),
        container(ID_CDRV, (12, 47, 154, 73)),
        item(WC_BUTTON, b"~Install new printer driver...", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (11, 25, 120, 14), ID_CINSTALL),
        book::label(b"Output port", (180, 122, 146)),
        container(ID_CPORT, (181, 59, 146, 61)),
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"Output to ~file", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (180, 45, 84, 10), ID_CFILE),
        item(WC_BUTTON, b"~Create", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (11, 5, 50, 14), ID_CREATE),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (69, 5, 50, 14), DID_CANCEL as u16),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, HWND_DESKTOP, create_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return None;
    }
    unsafe {
        WinProcessDlg(dlg);
        WinDestroyWindow(dlg);
        for o in core::mem::take(&mut CREATE.objs) {
            free(o);
        }
        CREATE.done.then(|| (CREATE.title.clone(), CREATE.driver.clone(), CREATE.port.clone()))
    }
}

// Install New Printer Driver: the devices of the drivers rs2 has (its
// PostScript driver's), added to PM_SPOOLER_DD.
const ID_DRVLIST: u16 = 21502;
static mut DRV_OBJS: Vec<usize> = Vec::new();
static mut DRV_PICK: Vec<u8> = Vec::new();

extern "C" fn install_drv_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let objs: Vec<usize> = printer::driver_devices(b"PSCRIPT").iter().map(|d| make_pdr(&[&b"PSCRIPT."[..], d].concat())).filter(|&x| x != 0).collect();
            fill(unsafe { WinWindowFromID(h, ID_DRVLIST as u32) }, &objs);
            unsafe { DRV_OBJS = objs };
            1
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_DRVLIST => {
            let c = unsafe { WinWindowFromID(h, ID_DRVLIST as u32) };
            if let Some(o) = notify(h, c, a >> 16, b) {
                unsafe { DRV_PICK = pdr_name(o) };
            }
            0
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            let p = unsafe { DRV_PICK.clone() };
            if !p.is_empty() {
                set_sys_str(b"PM_SPOOLER_DD", &p, b"PSCRIPT.DRV;;;");
            }
            unsafe { WinDismissDlg(h, DID_OK) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

pub fn install_driver(owner: u32) {
    let tpl = template(dialog_frame(b"Install New Printer Driver", 298, 150), alloc::vec![
        book::label(b"Printer driver shipped with rs2", (9, 132, 200)),
        container(ID_DRVLIST, (9, 24, 280, 104)),
        item(WC_BUTTON, b"~Install", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (10, 4, 50, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (66, 4, 50, 14), DID_CANCEL as u16),
    ]);
    unsafe { DRV_PICK = Vec::new() };
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, install_drv_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg != 0 {
        unsafe {
            WinProcessDlg(dlg);
            WinDestroyWindow(dlg);
            for o in core::mem::take(&mut DRV_OBJS) {
                free(o);
            }
        }
    }
}

methods! {
    __m_wpDeleteJob => wpDeleteJob, 1;
    __m_wpHoldJob => wpHoldJob, 1;
    __m_wpReleaseJob => wpReleaseJob, 1;
    __m_wpPrintJobNext => wpPrintJobNext, 1;
    __m_wpStartJobAgain => wpStartJobAgain, 1;
    __m_wpQueryPrintObject => wpQueryPrintObject, 1;
    __m_wpQueryJobId => wpQueryJobId, 1;
    __m_wpQueryJobFile => wpQueryJobFile, 3;
    __m_wpQueryJobType => wpQueryJobType, 3;
    __m_job_filter_menu => job_filter_menu, 4;
    __m_job_modify_menu => job_modify_menu, 4;
    __m_job_menu_selected => job_menu_selected, 3;
    __m_job_delete => job_delete, 2;
    __m_job_icon => job_icon, 1;
    __m_job_uninit => job_uninit, 1;
    __m_job_add_pages => job_add_pages, 2;
    __m_pdr_open => pdr_open, 4;
    __m_pdr_filter_menu => pdr_filter_menu, 4;
    __m_pdr_icon => pdr_icon, 1;
    __m_qdr_open => qdr_open, 4;
    __m_port_open => port_open, 4;
    __m_port_icon => port_icon, 1;
    __m_wpAddSetPathPage => wpAddSetPathPage, 2;
    __m_wpAddSetPriorityPage => wpAddSetPriorityPage, 2;
    __m_spool_add_pages => spool_add_pages, 2;
    __m_spool_modify_menu => spool_modify_menu, 4;
    __m_spool_menu_selected => spool_menu_selected, 3;
    __m_spool_default_view => spool_default_view, 1;
}

pub fn impl_job() -> Impl {
    Impl {
        data: core::mem::size_of::<JobData>() as u32,
        methods: alloc::vec![
            ("wpDeleteJob", __m_wpDeleteJob as usize),
            ("wpHoldJob", __m_wpHoldJob as usize),
            ("wpReleaseJob", __m_wpReleaseJob as usize),
            ("wpPrintJobNext", __m_wpPrintJobNext as usize),
            ("wpStartJobAgain", __m_wpStartJobAgain as usize),
            ("wpQueryPrintObject", __m_wpQueryPrintObject as usize),
            ("wpQueryJobId", __m_wpQueryJobId as usize),
            ("wpQueryJobFile", __m_wpQueryJobFile as usize),
            ("wpQueryJobType", __m_wpQueryJobType as usize),
        ],
        overrides: alloc::vec![
            ("wpFilterPopupMenu", __m_job_filter_menu as usize),
            ("wpModifyPopupMenu", __m_job_modify_menu as usize),
            ("wpMenuItemSelected", __m_job_menu_selected as usize),
            ("wpDelete", __m_job_delete as usize),
            ("wpQueryIcon", __m_job_icon as usize),
            ("wpUnInitData", __m_job_uninit as usize),
            ("wpAddSettingsPages", __m_job_add_pages as usize),
            crate::pages::no_window_page_override(),
        ],
    }
}

pub fn impl_pdr() -> Impl {
    Impl {
        data: core::mem::size_of::<PdrData>() as u32,
        methods: Vec::new(),
        overrides: alloc::vec![("wpOpen", __m_pdr_open as usize), ("wpFilterPopupMenu", __m_pdr_filter_menu as usize), ("wpQueryIcon", __m_pdr_icon as usize)],
    }
}

pub fn impl_qdr() -> Impl {
    Impl {
        data: core::mem::size_of::<QdrData>() as u32,
        methods: Vec::new(),
        overrides: alloc::vec![("wpOpen", __m_qdr_open as usize), ("wpFilterPopupMenu", __m_pdr_filter_menu as usize), ("wpQueryIcon", __m_pdr_icon as usize)],
    }
}

pub fn impl_port() -> Impl {
    Impl {
        data: core::mem::size_of::<PortData>() as u32,
        methods: Vec::new(),
        overrides: alloc::vec![("wpOpen", __m_port_open as usize), ("wpFilterPopupMenu", __m_pdr_filter_menu as usize), ("wpQueryIcon", __m_port_icon as usize)],
    }
}

pub fn impl_spool() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![("wpAddSetPathPage", __m_wpAddSetPathPage as usize), ("wpAddSetPriorityPage", __m_wpAddSetPriorityPage as usize)],
        overrides: alloc::vec![
            ("wpAddSettingsPages", __m_spool_add_pages as usize),
            ("wpModifyPopupMenu", __m_spool_modify_menu as usize),
            ("wpMenuItemSelected", __m_spool_menu_selected as usize),
            ("wpQueryDefaultView", __m_spool_default_view as usize),
            crate::pages::no_window_page_override(),
        ],
    }
}
