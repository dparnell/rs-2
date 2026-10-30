//! What the Open Watcom 1.9 installer (its GUI library) relies on: the
//! CREATESTRUCT layout, NULLHANDLE parents as IBM treats them, a dialog
//! dismissed before WinProcessDlg, WinWaitMsg.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::check;
use crate::pm::*;

unsafe extern "C" {
    #[link_name = "PMWIN.774"]
    fn WinIsWindowShowing(h: u32) -> u32;
    #[link_name = "PMWIN.922"]
    fn WinCreateDlg(parent: u32, owner: u32, proc_: WndProc, t: *const u8, params: u32) -> u32;
    #[link_name = "PMWIN.796"]
    fn WinProcessDlg(h: u32) -> u32;
    #[link_name = "PMWIN.910"]
    fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.897"]
    fn WinWaitMsg(hab: u32, first: u32, last: u32) -> u32;
    #[link_name = "PMWIN.907"]
    fn WinCreateMenu(parent: u32, t: *const u8) -> u32;
}

/// MENUITEM: iPosition, afStyle, afAttribute, id, hwndSubMenu, hItem.
#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    item: u32,
}

const MM_INSERTITEM: u32 = 0x0180;
const MM_QUERYITEM: u32 = 0x0182;
const MM_QUERYITEMTEXT: u32 = 0x018b;

/// What WM_CREATE brought: mp1, then the CREATESTRUCT's pPresParams,
/// pCtlData, id and hwndParent.
static CREATE: [AtomicU32; 5] = [const { AtomicU32::new(0xdead) }; 5];

extern "C" fn create_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_CREATE && mp2 != 0 {
        let cs = mp2 as *const u32;
        let v = unsafe { [mp1, cs.read_unaligned(), cs.add(1).read_unaligned(), cs.add(2).read_unaligned(), cs.add(12).read_unaligned()] };
        for (a, v) in CREATE.iter().zip(v) {
            a.store(v, Ordering::Relaxed);
        }
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

extern "C" fn dlg_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { WinDefDlgProc(h, msg, mp1, mp2) }
}

/// A DLGTEMPLATE with one item, an empty frame (WC_FRAME).
fn frame_template() -> [u8; 46] {
    let mut t = [0u8; 46];
    let mut put = |at: usize, v: u16| t[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put(0, 46);
    put(4, 850);
    put(6, 14);
    put(8, 1);
    put(10, 0xffff);
    put(14 + 6, 1); // WC_FRAME
    put(14 + 8, 1);
    put(14 + 10, 44); // the text, ""
    put(14 + 20, 100);
    put(14 + 22, 60);
    put(14 + 24, 500);
    put(14 + 26, 0xffff);
    put(14 + 28, 0xffff);
    t
}

pub fn owatcom_checks(hab: u32, hmq: u32, client: u32) {
    unsafe {
        WinRegisterClass(hab, b"OwCreate\0".as_ptr(), create_proc, 0, 4);
        let ctl = [0x55u8; 8];
        let h = WinCreateWindow(client, b"OwCreate\0".as_ptr(), b"\0".as_ptr(), 0, 0, 0, 10, 10, client, 3, 77, ctl.as_ptr(), core::ptr::null());
        let got: [u32; 5] = core::array::from_fn(|i| CREATE[i].load(Ordering::Relaxed));
        let p = ctl.as_ptr() as u32;
        check("WM_CREATE CREATESTRUCT: pPresParams, pCtlData, id, hwndParent", h != 0 && got == [p, 0, p, 77, client]);
        WinDestroyWindow(h);

        // NULLHANDLE parents: WinCreateWindow makes a window that is never
        // shown, WinCreateStdWindow and WinGetPS fail.
        let h = WinCreateWindow(0, b"OwCreate\0".as_ptr(), b"\0".as_ptr(), 0x8000_0000, 0, 0, 10, 10, 0, 3, 1, core::ptr::null(), core::ptr::null());
        check("WinCreateWindow(NULLHANDLE) is never showing", h != 0 && WinIsWindowShowing(h) == 0);
        WinDestroyWindow(h);
        let mut fcf = 1u32;
        let mut cl = 0;
        let f = WinCreateStdWindow(0, 0, &mut fcf, core::ptr::null(), b"x\0".as_ptr(), 0, 0, 0, &mut cl);
        check("WinCreateStdWindow(NULLHANDLE) fails", f == 0);
        check("WinGetPS(NULLHANDLE) fails", WinGetPS(0) == 0);

        // A dialog dismissed before WinProcessDlg: it returns the result at
        // once and the dialog stays hidden.
        let t = frame_template();
        let d = WinCreateDlg(HWND_DESKTOP, client, dlg_proc, t.as_ptr(), 0);
        WinDismissDlg(d, 7);
        let r = WinProcessDlg(d);
        check("WinProcessDlg of a dismissed dialog", d != 0 && r == 7 && WinIsWindowShowing(d) == 0);
        WinDestroyWindow(d);

        // WinCreateMenu without a template: an empty menu. Items inserted
        // with style 0 are text items, one with a submenu is MIS_SUBMENU.
        let bar = WinCreateMenu(HWND_DESKTOP, core::ptr::null());
        let sub = WinCreateMenu(HWND_DESKTOP, core::ptr::null());
        let mut mi = MenuItem { pos: -1, id: 1, ..Default::default() };
        WinSendMsg(bar, MM_INSERTITEM, &mi as *const MenuItem as u32, b"~File\0".as_ptr() as u32);
        mi.id = 2;
        mi.sub = sub;
        WinSendMsg(bar, MM_INSERTITEM, &mi as *const MenuItem as u32, b"~Edit\0".as_ptr() as u32);
        let mut buf = [0u8; 16];
        let n = WinSendMsg(bar, MM_QUERYITEMTEXT, 1 | 16 << 16, buf.as_mut_ptr() as u32);
        let mut q = MenuItem::default();
        WinSendMsg(bar, MM_QUERYITEM, 2, &mut q as *mut MenuItem as u32);
        check("WinCreateMenu(NULL) and MM_INSERTITEM with style 0", bar != 0 && n == 5 && &buf[..5] == b"~File" && { q.style } == 0x11);
        WinDestroyWindow(bar);

        // WinWaitMsg: returns once a message in the range is queued and
        // leaves it there.
        WinPostQueueMsg(hmq, WM_USER + 5, 1, 2);
        let w = WinWaitMsg(hab, WM_USER + 5, WM_USER + 5);
        let mut q = QMsg::default();
        let left = WinPeekMsg(hab, &mut q, 0, WM_USER + 5, WM_USER + 5, 1);
        check("WinWaitMsg", w != 0 && left != 0 && q.mp1 == 1);
    }
}
