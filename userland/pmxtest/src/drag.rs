//! PMDRAG beyond DrgDrag: lazy drag (dropped with the drag button and
//! Ctrl through the input hook, with DrgLazyDrop, cancelled), the drag
//! images held as the pointer, and DrgDragFiles onto a window that
//! DrgAcceptDroppedFiles made a target (the file copied by the source's
//! thread).

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use crate::pm::*;
use crate::{check, pump, pump_for};

unsafe extern "C" {
    #[link_name = "PMDRAG.33"]
    fn DrgAddStrHandle(s: *const u8) -> u32;
    #[link_name = "PMDRAG.34"]
    fn DrgAllocDraginfo(n: u32) -> u32;
    #[link_name = "PMDRAG.36"]
    fn DrgDeleteDraginfoStrHandles(p: u32) -> u32;
    #[link_name = "PMDRAG.39"]
    fn DrgFreeDraginfo(p: u32) -> u32;
    #[link_name = "PMDRAG.57"]
    fn DrgSetDragitem(p: u32, item: *const u32, cb: u32, i: u32) -> u32;
    #[link_name = "PMDRAG.65"]
    fn DrgDragFiles(h: u32, files: *const *const u8, types: *const *const u8, targets: *const *const u8, n: u32, hptr: u32, vk: u32, render: u32, rsvd: u32) -> u32;
    #[link_name = "PMDRAG.66"]
    fn DrgAcceptDroppedFiles(h: u32, path: *const u8, types: *const u8, op: u32, rsvd: u32) -> u32;
    #[link_name = "PMDRAG.67"]
    fn DrgLazyDrag(src: u32, pdinfo: u32, pdimg: *const u8, n: u32, rsvd: u32) -> u32;
    #[link_name = "PMDRAG.68"]
    fn DrgCancelLazyDrag() -> u32;
    #[link_name = "PMDRAG.69"]
    fn DrgLazyDrop(target: u32, op: u32, ptl: *const i32) -> u32;
    #[link_name = "PMDRAG.70"]
    fn DrgQueryDraginfoPtr(rsvd: u32) -> u32;
    #[link_name = "PMDRAG.73"]
    fn DrgQueryDragStatus() -> u32;
    #[link_name = "PMWIN.821"]
    fn WinQueryPointer(desk: u32) -> u32;
    #[link_name = "PMWIN.823"]
    fn WinQueryPointerPos(desk: u32, p: *mut i32) -> u32;
    #[link_name = "PMWIN.866"]
    fn WinSetPointer(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.828"]
    fn WinQuerySysPointer(desk: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.921"]
    fn WinSetKeyboardStateTable(desk: u32, table: *mut u8, set: u32) -> u32;
}

const DM_DROPNOTIFY: u32 = 0x031e;
const DM_DRAGOVERNOTIFY: u32 = 0x0321;
const DM_EMPHASIZETARGET: u32 = 0x0325;
const DM_DRAGFILECOMPLETE: u32 = 0x0326;
const DM_DRAGLEAVE: u32 = 0x032d;
const DM_DRAGOVER: u32 = 0x032e;
const DM_DROP: u32 = 0x032f;
const WM_MOUSEMOVE: u32 = 0x0070;
const WM_BUTTON2DOWN: u32 = 0x0074;
const WM_BUTTON2UP: u32 = 0x0075;
const DO_COPY: u32 = 0x10;
const DO_MOVE: u32 = 0x20;
const DOR_DROP: u32 = 1;
const VK_BUTTON2: u32 = 2;
const VK_CTRL: usize = 10;
const SPTR_ARROW: i32 = 1;
const SPTR_FILE: i32 = 19;
const DF_MOVE: u32 = 1;
const DF_SOURCE: u32 = 2;
const DF_SUCCESSFUL: u32 = 4;

#[derive(Default)]
struct Seen {
    /// Lazy targets (answer DOR_DROP, DO_COPY).
    targets: Vec<u32>,
    drops: Vec<(u32, u16)>,
    notify: Vec<(u32, u32)>,
    overs: u32,
    leaves: u32,
    emphasis: Vec<(u32, u32)>,
    files: Vec<(u32, u32)>,
}

static mut SEEN: Option<Seen> = None;

fn seen() -> &'static mut Seen {
    unsafe { (*core::ptr::addr_of_mut!(SEEN)).get_or_insert_with(Seen::default) }
}

/// The client's part (its PmxTest windows).
pub fn client_msg(h: u32, msg: u32, mp1: u32, mp2: u32) -> Option<u32> {
    let s = seen();
    match msg {
        DM_DRAGOVER if s.targets.contains(&h) => {
            s.overs += 1;
            Some(DOR_DROP | DO_COPY << 16)
        }
        DM_DRAGLEAVE if s.targets.contains(&h) => {
            s.leaves += 1;
            Some(0)
        }
        DM_DROP => {
            let op = unsafe { *((mp1 + 6) as *const u16) };
            s.drops.push((h, op));
            Some(0)
        }
        DM_DROPNOTIFY => {
            s.notify.push((h, mp2));
            Some(0)
        }
        DM_DRAGOVERNOTIFY => Some(0),
        DM_EMPHASIZETARGET => {
            s.emphasis.push((h, mp2));
            Some(0)
        }
        DM_DRAGFILECOMPLETE => {
            s.files.push((h, mp2));
            Some(0)
        }
        _ => None,
    }
}

fn draginfo(src: u32) -> u32 {
    let di = unsafe { DrgAllocDraginfo(1) };
    let s = |t: &[u8]| unsafe { DrgAddStrHandle(t.as_ptr()) };
    // hwndItem, ulItemID, type, RMF, container, source, target, offsets,
    // fsControl | fsSupportedOps << 16.
    let it = [src, 1, s(b"Plain Text\0"), s(b"<DRM_OS2FILE,DRF_TEXT>\0"), s(b"C:\\\0"), s(b"X.TXT\0"), s(b"X.TXT\0"), 0, 3 << 16];
    unsafe { DrgSetDragitem(di, it.as_ptr(), 36, 0) };
    di
}

/// A DRAGIMAGE: an icon, its middle under the pointer.
fn image() -> [u8; 24] {
    let mut img = [0u8; 24];
    img[0] = 24;
    img[4..8].copy_from_slice(&unsafe { WinQuerySysPointer(HWND_DESKTOP, SPTR_FILE, 0) }.to_le_bytes());
    img[16] = 1;
    img[20..22].copy_from_slice(&(-16i16).to_le_bytes());
    img[22..24].copy_from_slice(&(-16i16).to_le_bytes());
    img
}

fn set_ctrl(down: bool) {
    let mut keys = [0u8; 256];
    unsafe { WinSetKeyboardStateTable(HWND_DESKTOP, keys.as_mut_ptr(), 0) };
    keys[VK_CTRL] = if down { 0x80 } else { 0 };
    unsafe { WinSetKeyboardStateTable(HWND_DESKTOP, keys.as_mut_ptr(), 1) };
}

fn z(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn exists(path: &str) -> bool {
    let mut buf = [0u8; 64];
    let p = z(path);
    unsafe { os2::DosQueryPathInfo(p.as_ptr(), 1, buf.as_mut_ptr(), buf.len() as u32) == 0 }
}

pub fn drag_checks(hab: u32, client: u32) {
    let s = seen();
    let mut pt = [0i32; 2];
    unsafe { WinQueryPointerPos(HWND_DESKTOP, pt.as_mut_ptr()) };
    let src = unsafe { WinCreateWindow(client, b"PmxTest\0".as_ptr(), null(), 0x8000_0000, 10, 10, 30, 30, client, 3, 90, null(), null()) };
    // A top-level target under the pointer.
    let tgt = unsafe { WinCreateWindow(HWND_DESKTOP, b"PmxTest\0".as_ptr(), null(), 0x8000_0000, pt[0] - 40, pt[1] - 40, 80, 80, client, 3, 91, null(), null()) };
    s.targets.push(tgt);
    let arrow = unsafe { WinQuerySysPointer(HWND_DESKTOP, SPTR_ARROW, 0) };
    let img = image();

    // Picked up, then dropped with the drag button and Ctrl.
    let di = draginfo(src);
    let ok = unsafe { DrgLazyDrag(src, di, img.as_ptr(), 1, 0) } != 0;
    check("DrgLazyDrag returns at once, DGS_LAZYDRAGINPROGRESS", ok && unsafe { DrgQueryDragStatus() } == 2 && unsafe { DrgQueryDraginfoPtr(0) } == di);
    let held = unsafe { WinQueryPointer(HWND_DESKTOP) };
    unsafe { WinSetPointer(HWND_DESKTOP, arrow) };
    check("the drag images go with the pointer, kept while the drag runs", held != 0 && held != arrow && unsafe { WinQueryPointer(HWND_DESKTOP) } == held);
    set_ctrl(true);
    unsafe {
        WinPostMsg(tgt, WM_BUTTON2DOWN, 0, 0);
        WinPostMsg(tgt, WM_MOUSEMOVE, 0, 0);
        WinPostMsg(tgt, WM_BUTTON2UP, 0, 0);
    }
    pump_for(hab, 100);
    set_ctrl(false);
    check("the drag button with Ctrl: DM_DRAGOVER, then DM_DROP with DO_COPY", s.overs > 0 && s.drops.contains(&(tgt, DO_COPY as u16)));
    check("DM_DROPNOTIFY to the source with the target; the drag is over", s.notify.contains(&(src, tgt)) && unsafe { DrgQueryDragStatus() } == 0 && unsafe { WinQueryPointer(HWND_DESKTOP) } == arrow);

    // Dropped by the target (its pop-up menu's Drop).
    s.drops.clear();
    s.notify.clear();
    unsafe { DrgLazyDrag(src, di, img.as_ptr(), 1, 0) };
    // The drag button without Ctrl or Shift is the window's own click.
    unsafe {
        WinPostMsg(tgt, WM_BUTTON2DOWN, 0, 0);
        WinPostMsg(tgt, WM_BUTTON2UP, 0, 0);
    }
    pump(hab);
    let still = unsafe { DrgQueryDragStatus() } == 2 && s.drops.is_empty();
    let drop = unsafe { DrgLazyDrop(tgt, DO_MOVE, pt.as_ptr()) } != 0;
    check("a click without Ctrl/Shift leaves it; DrgLazyDrop drops (DO_MOVE)", still && drop && s.drops.contains(&(tgt, DO_MOVE as u16)) && s.notify.contains(&(src, tgt)));

    // Put down.
    s.drops.clear();
    s.notify.clear();
    unsafe { DrgLazyDrag(src, di, img.as_ptr(), 1, 0) };
    let ok = unsafe { DrgCancelLazyDrag() } != 0;
    check("DrgCancelLazyDrag: DM_DROPNOTIFY without a target", ok && s.drops.is_empty() && s.notify.contains(&(src, 0)) && unsafe { DrgQueryDragStatus() } == 0 && unsafe { DrgCancelLazyDrag() } == 0);
    unsafe {
        DrgDeleteDraginfoStrHandles(di);
        DrgFreeDraginfo(di);
    }
    s.targets.clear();

    // DrgDragFiles onto a DrgAcceptDroppedFiles window.
    let dir = z("C:\\PMXDRAG");
    let to = z("C:\\PMXDRAG\\TO");
    unsafe {
        os2::DosCreateDir(dir.as_ptr(), null_mut());
        os2::DosCreateDir(to.as_ptr(), null_mut());
    }
    let file = z("C:\\PMXDRAG\\A.TXT");
    let (mut hf, mut act) = (0u32, 0u32);
    let mut w = 0;
    unsafe {
        os2::DosOpen(file.as_ptr(), &mut hf, &mut act, 0, 0, 0x12, 0x42, null_mut());
        os2::DosWrite(hf, b"hello".as_ptr(), 5, &mut w);
        os2::DosClose(hf);
    }
    let copied = "C:\\PMXDRAG\\TO\\A.TXT";
    let cp = z(copied);
    unsafe { os2::DosDelete(cp.as_ptr()) };
    let accepted = unsafe { DrgAcceptDroppedFiles(tgt, to.as_ptr(), null(), DO_COPY, 0) } != 0;
    let again = unsafe { DrgAcceptDroppedFiles(tgt, to.as_ptr(), null(), DO_COPY, 0) } != 0;
    check("DrgAcceptDroppedFiles (once a window)", accepted && !again);
    s.emphasis.clear();
    s.files.clear();
    // The drag ends as the drag button goes up.
    unsafe { WinPostMsg(src, WM_BUTTON2UP, 0, 0) };
    let files = [file.as_ptr()];
    let hptr = unsafe { WinQuerySysPointer(HWND_DESKTOP, SPTR_FILE, 0) };
    let dropped = unsafe { DrgDragFiles(src, files.as_ptr(), null(), null(), 1, hptr, VK_BUTTON2, 0, 0) } != 0;
    pump_for(hab, 500);
    check("DrgDragFiles: dropped, the target's emphasis asked for", dropped && s.emphasis.contains(&(tgt, 1)));
    check("the file copied into the target's directory", exists(copied) && exists("C:\\PMXDRAG\\A.TXT"));
    check("DM_DRAGFILECOMPLETE to the target and (DF_SOURCE) to the source", s.files.contains(&(tgt, DF_SUCCESSFUL)) && s.files.contains(&(src, DF_SOURCE | DF_SUCCESSFUL)) && !s.files.iter().any(|f| f.1 & DF_MOVE != 0));
    unsafe {
        os2::DosDelete(cp.as_ptr());
        os2::DosDelete(file.as_ptr());
        os2::DosDeleteDir(to.as_ptr());
        os2::DosDeleteDir(dir.as_ptr());
        WinDestroyWindow(tgt);
        WinDestroyWindow(src);
    }
}
