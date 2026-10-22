//! A file dropped on the editor opens in it (after asking about
//! unsaved changes): the MLE is subclassed for DM_DRAGOVER (one item,
//! a file by DRM_OS2FILE with its directory and name, not a directory)
//! and DM_DROP; the opening happens after the drop returns (posted), and
//! the source hears DM_ENDCONVERSATION.

use alloc::vec::Vec;

use crate::pm::*;

unsafe extern "C" {
    #[link_name = "PMDRAG.32"]
    fn DrgAccessDraginfo(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.39"]
    fn DrgFreeDraginfo(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.36"]
    fn DrgDeleteDraginfoStrHandles(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.45"]
    fn DrgQueryDragitemCount(pdinfo: u32) -> u32;
    #[link_name = "PMDRAG.46"]
    fn DrgQueryDragitemPtr(pdinfo: u32, i: u32) -> u32;
    #[link_name = "PMDRAG.59"]
    fn DrgVerifyRMF(pditem: u32, mech: *const u8, fmt: *const u8) -> u32;
    #[link_name = "PMDRAG.49"]
    fn DrgQueryStrName(h: u32, cb: u32, buf: *mut u8) -> u32;
}

const DM_DROP: u32 = 0x032f;
const DM_DRAGOVER: u32 = 0x032e;
const DM_ENDCONVERSATION: u32 = 0x032b;
const DOR_DROP: u32 = 1;
const DOR_NODROPOP: u32 = 2;
const DOR_NEVERDROP: u32 = 3;
const DO_DEFAULT: u32 = 0xbffe;
const DO_COPY: u32 = 0x10;
const DMFL_TARGETSUCCESSFUL: u32 = 1;
const DMFL_TARGETFAIL: u32 = 2;
/// Posted to the client: open the dropped file.
pub const WM_OPEN_DROPPED: u32 = 0x1000 + 7;

static mut OLD: usize = 0;
static mut DROPPED: Vec<u8> = Vec::new();

fn rd(p: u32, off: u32) -> u32 {
    unsafe { ((p + off) as *const u32).read_unaligned() }
}

fn name(h: u32) -> Vec<u8> {
    let mut b = [0u8; 260];
    let n = if h == 0 { 0 } else { (unsafe { DrgQueryStrName(h, b.len() as u32, b.as_mut_ptr()) }) as usize };
    b[..n].to_vec()
}

/// The file of a DRAGINFO's one item, if it is one.
fn dropped_file(pd: u32) -> Option<(u32, Vec<u8>)> {
    if unsafe { DrgQueryDragitemCount(pd) } != 1 {
        return None;
    }
    let it = unsafe { DrgQueryDragitemPtr(pd, 0) };
    if it == 0 || unsafe { DrgVerifyRMF(it, b"DRM_OS2FILE\0".as_ptr(), core::ptr::null()) } == 0 {
        return None;
    }
    let (dir, file) = (name(rd(it, 16)), name(rd(it, 20)));
    if dir.is_empty() || file.is_empty() {
        return None;
    }
    let mut p = dir;
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(&file);
    // Not a directory.
    let c = z(&p);
    let mut st = [0u8; 24];
    if unsafe { os2::DosQueryPathInfo(c.as_ptr(), 1, st.as_mut_ptr(), st.len() as u32) } == 0 && st[20] & 0x10 != 0 {
        return None;
    }
    Some((it, p))
}

extern "C" fn mle_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        DM_DRAGOVER => {
            if unsafe { DrgAccessDraginfo(a) } == 0 {
                return DOR_NEVERDROP;
            }
            let op = rd(a, 4) >> 16;
            let r = match dropped_file(a) {
                Some(_) if op == DO_DEFAULT || op == DO_COPY => DOR_DROP | DO_COPY << 16,
                Some(_) => DOR_NODROPOP,
                None => DOR_NEVERDROP,
            };
            unsafe { DrgFreeDraginfo(a) };
            r
        }
        DM_DROP => {
            if unsafe { DrgAccessDraginfo(a) } == 0 {
                return 0;
            }
            let got = dropped_file(a);
            if let Some((it, p)) = &got {
                unsafe { *core::ptr::addr_of_mut!(DROPPED) = p.clone() };
                post(crate::app().client, WM_OPEN_DROPPED, 0, 0);
                post(rd(*it, 0), DM_ENDCONVERSATION, rd(*it, 4), DMFL_TARGETSUCCESSFUL);
            } else if unsafe { DrgQueryDragitemCount(a) } > 0 {
                let it = unsafe { DrgQueryDragitemPtr(a, 0) };
                post(rd(it, 0), DM_ENDCONVERSATION, rd(it, 4), DMFL_TARGETFAIL);
            }
            unsafe {
                DrgDeleteDraginfoStrHandles(a);
                DrgFreeDraginfo(a);
            }
            0
        }
        _ => {
            let old: WndProc = unsafe { core::mem::transmute(OLD) };
            old(h, m, a, b)
        }
    }
}

pub fn subclass(mle: u32) {
    unsafe { OLD = WinSubclassWindow(mle, mle_proc) as usize };
}

/// WM_OPEN_DROPPED: open it, if the changes may go.
pub fn open_dropped() {
    let p = unsafe { core::mem::take(&mut *core::ptr::addr_of_mut!(DROPPED)) };
    if !p.is_empty() && crate::process_save_quit() {
        crate::open_file(&p);
    }
}
