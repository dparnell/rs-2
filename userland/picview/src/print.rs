//! File > Print (and `PICVIEW /P file`): the picture on the default
//! printer (OS2.INI PM_SPOOLER QUEUE) through a queued DC of the
//! standard kind (PM_Q_STD), each page fitted to the printable area with
//! its proportions kept, as many copies as asked for.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use pmkit::*;

use crate::doc::Doc;
use crate::view;

pub enum PrintError {
    /// "There is no printer installed in the system."
    NoPrinter,
    /// Printing failed (an error number to report).
    Failed(u32),
}

unsafe extern "C" {
    #[link_name = "PMWIN.753"]
    fn WinGetLastError(hab: u32) -> u32;
}

/// The default queue's name.
pub fn default_queue() -> Option<Vec<u8>> {
    let mut buf = [0u8; 64];
    let n = unsafe { PrfQueryProfileString(HINI_SYSTEMPROFILE, b"PM_SPOOLER\0".as_ptr(), b"QUEUE\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), 64) } as usize;
    let q: Vec<u8> = buf[..n.min(64)].iter().copied().take_while(|&c| c != 0 && c != b';').collect();
    (!q.is_empty()).then_some(q)
}

/// Print every page of the document `copies` times as the job `name`.
pub fn print(hab: u32, doc: &mut Doc, name: &[u8], copies: u32) -> Result<(), PrintError> {
    let queue = default_queue().ok_or(PrintError::NoPrinter)?;
    let q = z(&queue);
    let dop = [q.as_ptr() as u32, 0, 0, b"PM_Q_STD\0".as_ptr() as u32];
    let hdc = unsafe { DevOpenDC(hab, OD_QUEUED, b"*\0".as_ptr(), 4, dop.as_ptr(), 0) };
    if hdc == 0 {
        return Err(PrintError::Failed(unsafe { WinGetLastError(hab) } & 0xffff));
    }
    let mut caps = [0i32; 2];
    unsafe { DevQueryCaps(hdc, CAPS_WIDTH, 2, caps.as_mut_ptr()) };
    let (w, h) = (caps[0].max(1), caps[1].max(1));
    crate::trace(alloc::format!("printer {}x{} pels", w, h).as_bytes());
    let size = SizeL { cx: 0, cy: 0 };
    let hps = unsafe { GpiCreatePS(hab, hdc, &size, PU_PELS | GPIF_DEFAULT | GPIT_NORMAL | GPIA_ASSOC) };
    if hps == 0 {
        unsafe { DevCloseDC(hdc) };
        return Err(PrintError::Failed(unsafe { WinGetLastError(hab) } & 0xffff));
    }
    let doc_name = z(name);
    let mut ok = unsafe { DevEscape(hdc, DEVESC_STARTDOC, name.len() as i32, doc_name.as_ptr(), null_mut(), null_mut()) } == 1;
    let shown = doc.page;
    let area = RectL::new(0, 0, w, h);
    let mut first = true;
    for _ in 0..copies.max(1) {
        for p in 0..doc.pages.len() {
            if !first {
                ok &= unsafe { DevEscape(hdc, DEVESC_NEWFRAME, 0, null(), null_mut(), null_mut()) } == 1;
            }
            first = false;
            doc.page = p;
            view::draw_scaled(hps, doc, area);
        }
    }
    doc.page = shown;
    let mut id = [0u8; 2];
    let mut n = 2;
    ok &= unsafe { DevEscape(hdc, DEVESC_ENDDOC, 0, null(), &mut n, id.as_mut_ptr()) } == 1;
    let err = unsafe { WinGetLastError(hab) } & 0xffff;
    unsafe {
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
    if ok { Ok(()) } else { Err(PrintError::Failed(err)) }
}
