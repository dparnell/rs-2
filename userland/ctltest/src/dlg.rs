//! Dialogs from templates and window ownership across threads.
//!
//! A dialog paints its background under its controls unless its template
//! asks for WS_CLIPCHILDREN (Borland's installer has a frame static and a
//! control that draws only part of itself, both showing the dialog
//! there); WinDestroyWindow works only on the calling thread's windows
//! (its worker thread tries to destroy the progress dialog and later
//! posts it a message).

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
}

const QWL_STYLE: i32 = -2;
const WS_CLIPCHILDREN: u32 = 0x2000_0000;
const FS_DLGBORDER: u32 = 0x80;
const SS_FGNDFRAME: u32 = 0x08;

/// A DLGTEMPLATE: a frame with one frame static (id 50).
fn template(frame_style: u32) -> Vec<u8> {
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
    for (children, class, cch, style, id) in [(1u16, 1u16, 4u16, frame_style, 1u16), (0, 5, 0, 0x8000_0000 | SS_FGNDFRAME, 50)] {
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
    }
}
