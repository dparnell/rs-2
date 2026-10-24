//! The Connections folder's objects, as far as rs2 has what they stand
//! for:
//!
//! - **Network** (WPNetwork, `<WP_NETWORK>`): IBM's folder of network
//!   groups, which LAN requesters fill. rs2 has no LAN client yet, so it
//!   holds only what is put in it; its Network page says so.
//! - **Printers** (WPPrinter, IBM's WPPRINT.DLL class): print
//!   destinations. rs2 has no spooler or printer drivers yet: a printer
//!   object keeps its queue name (QUEUENAME=), opens a job view that is
//!   always empty and says printing is not available, and files dropped
//!   on it are refused with that message. The Printers folder holds a
//!   template, as IBM's INI.RC puts one there (`<WP_NEWPRINTER>`).

#![allow(static_mut_refs)]

use alloc::vec::Vec;

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
}

const NOT_AVAILABLE: &[u8] = b"Printing is not available: rs2 has no print spooler or printer drivers yet. This printer's queue stays empty.";

// ---- Network ----

extern "C" fn network_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn network_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPNetwork"), wps::cd("WPFolder"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    // SS_TEXT | DT_WORDBREAK
    add_page(o, nb, network_page_proc, alloc::vec![
        book::label(b"Network status", (6, 106, 220)),
        item(WC_STATIC, b"No network is available: rs2 has no LAN requester (no network groups to show). Objects you put in this folder stay here.", WS_VISIBLE | 0x0001 | 0x4000, (12, 60, 214, 42), 0xffff),
    ], b"~Network\0");
    1
}

// ---- Printers ----

#[repr(C)]
pub struct PrinterData {
    queue: Str,
}

fn pd(o: usize) -> &'static mut PrinterData {
    som::data(wps::ccd("WPPrinter"), o)
}

/// Open job views: (printer, frame, client).
static mut JOB_VIEWS: Vec<(usize, u32, u32)> = Vec::new();
static JOBS_CLASS: &[u8] = b"RS2PrinterJobs\0";

extern "C" fn jobs_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            unsafe {
                let hps = WinBeginPaint(h, 0, &mut r);
                WinQueryWindowRect(h, &mut r);
                WinFillRect(hps, &r, -2);
                let mut tr = RectL { l: r.l + 12, b: r.b + 8, r: r.r - 12, t: r.t - 12 };
                let t = b"No print jobs.";
                // DT_LEFT | DT_TOP
                WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut tr, -1, -2, 0);
                tr.t -= 24;
                // DT_WORDBREAK: one line at a time.
                let mut rest: &[u8] = NOT_AVAILABLE;
                while !rest.is_empty() && tr.t > tr.b {
                    let n = WinDrawText(hps, rest.len() as i32, rest.as_ptr(), &mut tr, 8, -2, 0x4000);
                    if n <= 0 {
                        break;
                    }
                    rest = &rest[(n as usize).min(rest.len())..];
                    while rest.first() == Some(&b' ') {
                        rest = &rest[1..];
                    }
                    tr.t -= 16;
                }
                WinEndPaint(hps);
            }
            0
        }
        WM_SIZE => {
            unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
            0
        }
        WM_CLOSE => {
            if let Some(i) = unsafe { JOB_VIEWS.iter().position(|v| v.2 == h) } {
                let f = unsafe { JOB_VIEWS.remove(i).1 };
                unsafe { WinDestroyWindow(f) };
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

/// The job view: the queue's jobs (none).
fn open_jobs(o: usize) -> u32 {
    if let Some(&(_, f, _)) = unsafe { JOB_VIEWS.iter().find(|v| v.0 == o) } {
        crate::view::activate(f);
        return f;
    }
    let hab = unsafe { WinQueryAnchorBlock(HWND_DESKTOP) };
    // CS_SIZEREDRAW
    unsafe { WinRegisterClass(hab, JOBS_CLASS.as_ptr(), jobs_proc, 0x0004, 0) };
    let mut t: Vec<u8> = obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
    t.extend_from_slice(b" - Job Icon View");
    let t = z(&t);
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, JOBS_CLASS.as_ptr(), t.as_ptr(), 0, 0, 1, &mut client) };
    if frame == 0 {
        return 0;
    }
    unsafe { JOB_VIEWS.push((o, frame, client)) };
    send(frame, 0x0047, obj::call(m::WPObject::wpQueryIcon, &[o]) as u32, 0);
    let (sw, sh) = screen();
    unsafe { WinSetWindowPos(frame, HWND_TOP, sw / 5, sh / 3, 400.min(sw - 40), 200, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER) };
    frame
}

extern "C" fn printer_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32;
    }
    open_jobs(o)
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

/// wpQueryPrinterName: the queue's name (a buffer of 9 or more bytes).
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

extern "C" fn wpDeleteAllJobs(_o: usize) -> u32 {
    1
}

/// Anything dropped on a printer would be printed: say it cannot be.
extern "C" fn printer_drag_over(_o: usize, _cnr: u32, _pdinfo: u32) -> u32 {
    // DOR_DROP, DO_COPY: files to print.
    mp(DOR_DROP, DO_COPY)
}

extern "C" fn printer_drop(o: usize, _cnr: u32, _pdinfo: u32, _it: u32) -> u32 {
    let t: Vec<u8> = obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
    message(HWND_DESKTOP, NOT_AVAILABLE, &t, MB_OK | MB_WARNING);
    crate::delete::RC_DROP_DROPCOMPLETE
}

const ID_QUEUE: u16 = 740;

extern "C" fn printer_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let q = str_bytes(&pd(b as usize).queue);
            set_item_text(h, ID_QUEUE as u32, if q.is_empty() { b"(none)" } else { q });
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn printer_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPPrinter"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    add_page(o, nb, printer_page_proc, alloc::vec![
        book::label(b"Queue name:", (6, 106, 60)),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001, (70, 106, 150, 8), ID_QUEUE),
        // SS_TEXT | DT_WORDBREAK
        item(WC_STATIC, NOT_AVAILABLE, WS_VISIBLE | 0x0001 | 0x4000, (6, 56, 220, 40), 0xffff),
    ], b"~Printer\0");
    1
}

methods! {
    __m_network_add_pages => network_add_pages, 2;
    __m_printer_open => printer_open, 4;
    __m_printer_setup => printer_setup, 2;
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

pub fn impl_network() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpAddSettingsPages", __m_network_add_pages as usize)] }
}

pub fn impl_printer() -> Impl {
    Impl {
        data: core::mem::size_of::<PrinterData>() as u32,
        methods: alloc::vec![
            ("wpQueryPrinterName", __m_wpQueryPrinterName as usize),
            ("wpSetPrinterName", __m_wpSetPrinterName as usize),
            ("wpDeleteAllJobs", __m_wpDeleteAllJobs as usize),
        ],
        overrides: alloc::vec![
            ("wpOpen", __m_printer_open as usize),
            ("wpSetup", __m_printer_setup as usize),
            ("wpSaveState", __m_printer_save_state as usize),
            ("wpRestoreState", __m_printer_restore_state as usize),
            ("wpUnInitData", __m_printer_uninit as usize),
            ("wpDragOver", __m_printer_drag_over as usize),
            ("wpDrop", __m_printer_drop as usize),
            ("wpAddSettingsPages", __m_printer_add_pages as usize),
        ],
    }
}
