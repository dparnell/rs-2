//! Dialogs from templates and window ownership across threads.
//!
//! A dialog paints its background under its controls unless its template
//! asks for WS_CLIPCHILDREN (Borland's installer has a frame static and a
//! control that draws only part of itself, both showing the dialog
//! there); WinDestroyWindow works only on the calling thread's windows
//! (its worker thread tries to destroy the progress dialog and later
//! posts it a message). The dialog procedure gets WM_MEASUREITEM from
//! owner-drawn controls before WM_INITDLG; the file dialog is set up
//! before the application's procedure gets WM_INITDLG (Borland C++'s
//! keeps it).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::check;

unsafe extern "C" {
    #[link_name = "PMWIN.922"]
    fn WinCreateDlg(parent: u32, owner: u32, proc_: u32, tmpl: *const u8, params: u32) -> u32;
    #[link_name = "PMWIN.843"]
    fn WinQueryWindowULong(h: u32, index: i32) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.772"]
    fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.753"]
    fn WinGetLastError(hab: u32) -> u32;
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.910"]
    fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.903"]
    fn WinSendDlgItemMsg(h: u32, id: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.841"]
    fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.839"]
    fn WinQueryWindowPtr(h: u32, index: i32) -> u32;
    #[link_name = "PMCTLS.4"]
    fn WinFileDlg(parent: u32, owner: u32, pfild: *mut u8) -> u32;
    #[link_name = "PMCTLS.5"]
    fn WinDefFileDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
}

const QWL_STYLE: i32 = -2;
const WS_CLIPCHILDREN: u32 = 0x2000_0000;
const FS_DLGBORDER: u32 = 0x80;
const SS_FGNDFRAME: u32 = 0x08;

/// A DLGTEMPLATE: a frame with one frame static (id 50).
fn template(frame_style: u32) -> Vec<u8> {
    template_with(frame_style, 5, 0x8000_0000 | SS_FGNDFRAME)
}

/// A DLGTEMPLATE: a frame with one control of class `class` (id 50).
fn template_with(frame_style: u32, class: u16, child_style: u32) -> Vec<u8> {
    let mut t = Vec::new();
    let w = |t: &mut Vec<u8>, v: u16| t.extend_from_slice(&v.to_le_bytes());
    let items = 14 + 2 * 30;
    let text = items as u16;
    // cbTemplate, type, codepage, offadlgti, fsTemplateStatus,
    // iItemFocus, coffPresParams
    for v in [0u16, 0, 437, 14, 1, 0xffff, 0] {
        w(&mut t, v);
    }
    // fsItemStatus, cChildren, cchClassName, offClassName (WC_FRAME 1,
    // WC_STATIC 5), cchText, offText, flStyle, x, y, cx, cy, id,
    // offPresParams, offCtlData
    for (children, class, cch, style, id) in [(1u16, 1u16, 4u16, frame_style, 1u16), (0, class, 0, child_style, 50)] {
        for v in [0, children, 0, class, cch, text] {
            w(&mut t, v);
        }
        t.extend_from_slice(&style.to_le_bytes());
        for v in [10u16, 10, 100, 60, id, 0xffff, 0xffff] {
            w(&mut t, v);
        }
    }
    t.extend_from_slice(b"Dlg\0");
    let n = t.len() as u16;
    t[0..2].copy_from_slice(&n.to_le_bytes());
    t
}

/// WM_MEASUREITEM before WM_INITDLG, and WM_INITDLG's mp1 and mp2.
static MEASURED: AtomicU32 = AtomicU32::new(0);
static INIT_ARGS: AtomicU32 = AtomicU32::new(0xffff_ffff);
static INIT_DIRS: AtomicU32 = AtomicU32::new(0xffff_ffff);

extern "C" fn measuring_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        // WM_MEASUREITEM: 23 pels high.
        0x0037 => {
            if MEASURED.load(Ordering::Relaxed) == 0 {
                MEASURED.store(1, Ordering::Relaxed);
            }
            23
        }
        0x003b => {
            if MEASURED.load(Ordering::Relaxed) == 1 {
                MEASURED.store(2, Ordering::Relaxed);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, mp1, mp2) },
    }
}

/// A file dialog procedure that handles WM_INITDLG itself and passes the
/// rest on (Borland C++'s does so).
extern "C" fn file_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == 0x003b {
        INIT_ARGS.store(mp1 | mp2, Ordering::Relaxed);
        // LM_QUERYITEMCOUNT of DID_DIRECTORY_LB.
        INIT_DIRS.store(unsafe { WinSendDlgItemMsg(h, 264, 0x0160, 0, 0) }, Ordering::Relaxed);
        return 0;
    }
    unsafe { WinDefFileDlgProc(h, msg, mp1, mp2) }
}

static TARGET: AtomicU32 = AtomicU32::new(0);
static RESULT: AtomicU32 = AtomicU32::new(0xffff_ffff);
static ERROR: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn destroyer(_: u32) {
    unsafe {
        let hab = WinInitialize(0);
        let hmq = WinCreateMsgQueue(hab, 0);
        WinGetLastError(hab);
        RESULT.store(WinDestroyWindow(TARGET.load(Ordering::Relaxed)), Ordering::Relaxed);
        ERROR.store(WinGetLastError(hab) & 0xffff, Ordering::Relaxed);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

pub fn checks(hab: u32, owner: u32) {
    unsafe {
        let t = template(FS_DLGBORDER);
        let d = WinCreateDlg(1, owner, 0, t.as_ptr(), 0);
        check("WinCreateDlg from a template", d != 0 && WinWindowFromID(d, 50) != 0);
        check("a dialog does not clip its controls unless its template says so", WinQueryWindowULong(d, QWL_STYLE) & WS_CLIPCHILDREN == 0);
        let t2 = template(FS_DLGBORDER | WS_CLIPCHILDREN);
        let d2 = WinCreateDlg(1, owner, 0, t2.as_ptr(), 0);
        check("one whose template has WS_CLIPCHILDREN does", d2 != 0 && WinQueryWindowULong(d2, QWL_STYLE) & WS_CLIPCHILDREN != 0);
        WinDestroyWindow(d2);

        // Another thread cannot destroy the dialog.
        TARGET.store(d, Ordering::Relaxed);
        let mut tid = 0;
        if os2::DosCreateThread(&mut tid, destroyer, 0, 0, 64 * 1024) == 0 {
            os2::DosWaitThread(&mut tid, 0);
        }
        check(
            "WinDestroyWindow from another thread fails (PMERR_CALL_FROM_WRONG_THREAD)",
            RESULT.load(Ordering::Relaxed) == 0 && ERROR.load(Ordering::Relaxed) == 0x1009 && WinIsWindow(hab, d) != 0,
        );
        check("the window's own thread can", WinDestroyWindow(d) != 0 && WinIsWindow(hab, d) == 0);

        // The dialog procedure is there when the controls are made: an
        // owner-drawn list box asks it how high its items are first.
        let t3 = template_with(FS_DLGBORDER, 7, 0x8000_0000 | 0x0002);
        let d3 = WinCreateDlg(1, owner, measuring_proc as *const () as u32, t3.as_ptr(), 0);
        check("WM_MEASUREITEM reaches the dialog procedure before WM_INITDLG", d3 != 0 && MEASURED.load(Ordering::Relaxed) == 2);
        WinDestroyWindow(d3);

        // WinFileDlg: the dialog is set up before the application's
        // procedure gets WM_INITDLG (mp1 and mp2 0), the lists are filled
        // after it, whether or not it passes WM_INITDLG on.
        let mut fd = [0u8; 0x148];
        fd[0..4].copy_from_slice(&0x148u32.to_le_bytes());
        // FDS_OPEN_DIALOG | FDS_MODELESS.
        fd[4..8].copy_from_slice(&0x0140u32.to_le_bytes());
        let title = b"CtlTest Open\0";
        fd[20..24].copy_from_slice(&(title.as_ptr() as u32).to_le_bytes());
        fd[28..32].copy_from_slice(&(file_proc as *const () as u32).to_le_bytes());
        fd[52..56].copy_from_slice(b"C:\\*");
        let fdlg = WinFileDlg(1, owner, fd.as_mut_ptr());
        let mut tb = [0u8; 32];
        let n = WinQueryWindowText(fdlg, 32, tb.as_mut_ptr()) as usize;
        check("WinFileDlg (modeless) with an application procedure", fdlg != 0 && WinQueryWindowPtr(fdlg, 0) == fd.as_ptr() as u32);
        check("its title set before WM_INITDLG", &tb[..n] == b"CtlTest Open");
        check("WM_INITDLG to the application's procedure with mp1 and mp2 0", INIT_ARGS.load(Ordering::Relaxed) == 0);
        check("the directories listed although it kept WM_INITDLG", WinSendDlgItemMsg(fdlg, 264, 0x0160, 0, 0) > 0);
        WinDestroyWindow(fdlg);
    }
}
