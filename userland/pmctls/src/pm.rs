//! The PMWIN, PMGPI and DOSCALLS entry points the standard dialogs use
//! (PMCTLS is an ordinary PM client of PMWIN), and small helpers.

#![allow(dead_code)]

use alloc::vec::Vec;

unsafe extern "C" {
    #[link_name = "PMWIN.922"]
    pub fn WinCreateDlg(parent: u32, owner: u32, proc_: u32, pdlgt: *const u8, params: u32) -> u32;
    #[link_name = "PMWIN.796"]
    pub fn WinProcessDlg(hwnd: u32) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(hwnd: u32) -> u32;
    #[link_name = "PMWIN.910"]
    pub fn WinDefDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.903"]
    pub fn WinSendDlgItemMsg(hwnd: u32, id: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.859"]
    pub fn WinSetDlgItemText(hwnd: u32, id: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.815"]
    pub fn WinQueryDlgItemText(hwnd: u32, id: u32, cch: i32, buf: *mut u8) -> u32;
    #[link_name = "PMWIN.899"]
    pub fn WinWindowFromID(hwnd: u32, id: u32) -> u32;
    #[link_name = "PMWIN.876"]
    pub fn WinSetWindowPtr(hwnd: u32, index: i32, p: u32) -> u32;
    #[link_name = "PMWIN.839"]
    pub fn WinQueryWindowPtr(hwnd: u32, index: i32) -> u32;
    #[link_name = "PMWIN.729"]
    pub fn WinDismissDlg(hwnd: u32, result: u32) -> u32;
    #[link_name = "PMWIN.789"]
    pub fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMWIN.877"]
    pub fn WinSetWindowText(hwnd: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.860"]
    pub fn WinSetFocus(desk: u32, hwnd: u32) -> u32;
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.735"]
    pub fn WinEnableWindow(hwnd: u32, on: u32) -> u32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(hwnd: u32, on: u32) -> u32;
    #[link_name = "PMWIN.929"]
    pub fn WinSubclassWindow(hwnd: u32, proc_: u32) -> u32;
    #[link_name = "PMWIN.703"]
    pub fn WinBeginPaint(hwnd: u32, hps: u32, prcl: *mut i32) -> u32;
    #[link_name = "PMWIN.738"]
    pub fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.743"]
    pub fn WinFillRect(hps: u32, prcl: *const i32, color: i32) -> u32;
    #[link_name = "PMWIN.840"]
    pub fn WinQueryWindowRect(hwnd: u32, prcl: *mut i32) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(hwnd: u32, prcl: *const i32, children: u32) -> u32;
    #[link_name = "PMWIN.911"]
    pub fn WinDefWindowProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.701"]
    pub fn WinAlarm(desk: u32, style: u32) -> u32;
    #[link_name = "PMWIN.829"]
    pub fn WinQuerySysValue(desk: u32, i: i32) -> i32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(hwnd: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.837"]
    pub fn WinQueryWindowPos(hwnd: u32, pswp: *mut u32) -> u32;
    #[link_name = "PMGPI.368"]
    pub fn GpiCreateLogFont(hps: u32, name: *const u8, lcid: i32, fat: *const u8) -> i32;
    #[link_name = "PMGPI.513"]
    pub fn GpiSetCharSet(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.378"]
    pub fn GpiDeleteSetId(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.359"]
    pub fn GpiCharStringAt(hps: u32, p: *const i32, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.510"]
    pub fn GpiSetCharBox(hps: u32, p: *const i32) -> u32;
    #[link_name = "PMGPI.517"]
    pub fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "DOSCALLS.275"]
    pub fn DosQueryCurrentDisk(pdisk: *mut u32, pmap: *mut u32) -> u32;
    #[link_name = "DOSCALLS.274"]
    pub fn DosQueryCurrentDir(disk: u32, buf: *mut u8, pcb: *mut u32) -> u32;
    #[link_name = "DOSCALLS.223"]
    pub fn DosQueryPathInfo(path: *const u8, level: u32, buf: *mut u8, cb: u32) -> u32;
}

pub const WM_CONTROL: u32 = 0x0030;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_INITDLG: u32 = 0x003b;
pub const WM_PAINT: u32 = 0x0023;
pub const WM_HELP: u32 = 0x0022;
pub const WM_USER: u32 = 0x1000;
pub const DID_OK: u32 = 1;
pub const DID_CANCEL: u32 = 2;

pub const LM_QUERYITEMCOUNT: u32 = 0x0160;
pub const LM_INSERTITEM: u32 = 0x0161;
pub const LM_SELECTITEM: u32 = 0x0164;
pub const LM_QUERYSELECTION: u32 = 0x0165;
pub const LM_QUERYITEMTEXT: u32 = 0x0168;
pub const LM_DELETEALL: u32 = 0x016e;
pub const LM_SEARCHSTRING: u32 = 0x016b;
pub const LM_SETTOPINDEX: u32 = 0x0162;
pub const LIT_END: u32 = 0xffff_ffff;
pub const LIT_NONE: u32 = 0xffff_ffff;
pub const BM_SETCHECK: u32 = 0x0125;
pub const BM_QUERYCHECK: u32 = 0x0124;
pub const EM_SETTEXTLIMIT: u32 = 0x0143;
pub const EM_SETSEL: u32 = 0x0142;

pub const LN_SELECT: u32 = 1;
pub const LN_ENTER: u32 = 5;
pub const CBN_LBSELECT: u32 = 4;
pub const CBN_ENTER: u32 = 7;
pub const CBN_EFCHANGE: u32 = 1;

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

pub fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

pub fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}

pub fn set_item_text(dlg: u32, id: u32, t: &[u8]) {
    let s = z(t);
    unsafe { WinSetDlgItemText(dlg, id, s.as_ptr()) };
}

pub fn item_text(dlg: u32, id: u32) -> Vec<u8> {
    let mut buf = [0u8; 300];
    let n = unsafe { WinQueryDlgItemText(dlg, id, buf.len() as i32, buf.as_mut_ptr()) } as usize;
    buf[..n.min(buf.len())].to_vec()
}

pub fn list_add(dlg: u32, id: u32, t: &[u8]) -> u32 {
    let s = z(t);
    unsafe { WinSendDlgItemMsg(dlg, id, LM_INSERTITEM, LIT_END, s.as_ptr() as u32) }
}

pub fn list_clear(dlg: u32, id: u32) {
    unsafe { WinSendDlgItemMsg(dlg, id, LM_DELETEALL, 0, 0) };
}

pub fn list_selection(dlg: u32, id: u32) -> Option<u32> {
    let i = unsafe { WinSendDlgItemMsg(dlg, id, LM_QUERYSELECTION, LIT_NONE, 0) };
    (i != LIT_NONE).then_some(i)
}

pub fn list_text(dlg: u32, id: u32, i: u32) -> Vec<u8> {
    let mut buf = [0u8; 300];
    let n = unsafe { WinSendDlgItemMsg(dlg, id, LM_QUERYITEMTEXT, mp(i, buf.len() as u32), buf.as_mut_ptr() as u32) } as usize;
    buf[..n.min(buf.len())].to_vec()
}

pub fn list_select(dlg: u32, id: u32, i: u32) {
    unsafe { WinSendDlgItemMsg(dlg, id, LM_SELECTITEM, i, 1) };
}

/// Message box owned by `owner` (MB_OK | MB_WARNING | MB_MOVEABLE).
pub fn message(owner: u32, text: &[u8], title: &[u8]) {
    let (t, c) = (z(text), z(title));
    unsafe { WinMessageBox(1, owner, t.as_ptr(), c.as_ptr(), 0, 0x0000 | 0x0020 | 0x4000) };
}

/// Put a dialog in the middle of the screen.
pub fn center(dlg: u32) {
    let mut swp = [0u32; 9];
    unsafe {
        WinQueryWindowPos(dlg, swp.as_mut_ptr());
        let (cx, cy) = (swp[2] as i32, swp[1] as i32);
        let (sw, sh) = (WinQuerySysValue(1, 20), WinQuerySysValue(1, 21));
        // SWP_MOVE.
        WinSetWindowPos(dlg, 0, ((sw - cx) / 2).max(0), ((sh - cy) / 2).max(0), 0, 0, 0x0002);
    }
}

/// A dialog template built in memory (the DLGTEMPLATE layout WinCreateDlg
/// takes): the frame and its children.
pub struct Item {
    /// WC_* class number.
    pub class: u16,
    pub text: Vec<u8>,
    pub style: u32,
    pub x: i16,
    pub y: i16,
    pub cx: i16,
    pub cy: i16,
    pub id: u16,
    pub ctl: Vec<u8>,
}

pub fn item(class: u16, text: &[u8], style: u32, (x, y, cx, cy): (i16, i16, i16, i16), id: u16) -> Item {
    Item { class, text: text.to_vec(), style, x, y, cx, cy, id, ctl: Vec::new() }
}

pub const WC_FRAME: u16 = 1;
pub const WC_COMBOBOX: u16 = 2;
pub const WC_BUTTON: u16 = 3;
pub const WC_STATIC: u16 = 5;
pub const WC_ENTRYFIELD: u16 = 6;
pub const WC_LISTBOX: u16 = 7;

pub const WS_VISIBLE: u32 = 0x8000_0000;
pub const WS_GROUP: u32 = 0x0001_0000;
pub const WS_TABSTOP: u32 = 0x0002_0000;

/// Build a template: header (cbTemplate, type, codepage, offadlgti,
/// fsTemplateStatus, iItemFocus, coffPresParams), 30-byte items, then
/// the strings and control data.
pub fn template(frame: Item, children: Vec<Item>) -> Vec<u8> {
    let items: Vec<Item> = core::iter::once(frame).chain(children).collect();
    let n = items.len();
    let head = 14;
    let mut out = alloc::vec![0u8; head + 30 * n];
    let mut extra: Vec<u8> = Vec::new();
    let base = head + 30 * n;
    let put16 = |o: &mut Vec<u8>, at: usize, v: u16| o[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put16(&mut out, 2, 0);
    put16(&mut out, 4, 850);
    put16(&mut out, 6, head as u16);
    put16(&mut out, 8, 1);
    put16(&mut out, 10, 0xffff);
    put16(&mut out, 12, 0);
    for (i, it) in items.iter().enumerate() {
        let at = head + 30 * i;
        let children = if i == 0 { (n - 1) as u16 } else { 0 };
        let toff = base + extra.len();
        extra.extend_from_slice(&it.text);
        extra.push(0);
        let coff = if it.ctl.is_empty() {
            0xffff
        } else {
            while (base + extra.len()) % 4 != 0 {
                extra.push(0);
            }
            let c = base + extra.len();
            extra.extend_from_slice(&it.ctl);
            c as u16
        };
        put16(&mut out, at, 0);
        put16(&mut out, at + 2, children);
        put16(&mut out, at + 4, 0);
        put16(&mut out, at + 6, it.class);
        put16(&mut out, at + 8, it.text.len() as u16 + 1);
        put16(&mut out, at + 10, toff as u16);
        out[at + 12..at + 16].copy_from_slice(&it.style.to_le_bytes());
        put16(&mut out, at + 16, it.x as u16);
        put16(&mut out, at + 18, it.y as u16);
        put16(&mut out, at + 20, it.cx as u16);
        put16(&mut out, at + 22, it.cy as u16);
        put16(&mut out, at + 24, it.id);
        put16(&mut out, at + 26, 0xffff);
        put16(&mut out, at + 28, coff);
    }
    out.extend_from_slice(&extra);
    let len = out.len() as u16;
    put16(&mut out, 0, len);
    out
}
