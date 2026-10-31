//! Self-checks of PMWIN's standard controls and clipboard: the MLE,
//! entry fields, combo boxes, spin buttons, sliders, value sets, the
//! clipboard (also from a second process: CTLTEST runs itself with
//! CHILD), atoms, a few helpers and the 16-bit PM API (pm16.rs). Each
//! check prints PASS or FAIL;
//! the last line is "ctltest done: N failure(s)" and the exit code N.
//! The controls live in a hidden window.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

mod dlg;
mod pm16;
mod wm;

os2::entry!(main);

#[repr(C)]
#[derive(Default)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: u32, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.841"]
    fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.877"]
    fn WinSetWindowText(h: u32, t: *const u8) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.918"]
    fn WinPeekMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.793"]
    fn WinOpenClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.707"]
    fn WinCloseClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.733"]
    fn WinEmptyClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.854"]
    fn WinSetClipbrdData(hab: u32, data: u32, fmt: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.806"]
    fn WinQueryClipbrdData(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.807"]
    fn WinQueryClipbrdFmtInfo(hab: u32, fmt: u32, pfl: *mut u32) -> u32;
    #[link_name = "PMWIN.739"]
    fn WinEnumClipbrdFmts(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.700"]
    fn WinAddAtom(h: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.744"]
    fn WinFindAtom(h: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.802"]
    fn WinQueryAtomName(h: u32, atom: u32, buf: *mut u8, cch: u32) -> u32;
    #[link_name = "PMWIN.721"]
    fn WinDeleteAtom(h: u32, atom: u32) -> u32;
    #[link_name = "PMWIN.893"]
    fn WinUpper(hab: u32, cp: u32, cc: u32, psz: *mut u8) -> u32;
    #[link_name = "PMWIN.764"]
    fn WinIntersectRect(hab: u32, d: *mut i32, a: *const i32, b: *const i32) -> u32;
    #[link_name = "PMWIN.833"]
    fn WinQueryVersion(hab: u32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.598"]
    fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, init: *const u8, bmi: *const u8) -> u32;
    #[link_name = "PMGPI.573"]
    fn GpiQueryBitmapParameters(hbm: u32, p: *mut u8) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
}

static FAILS: AtomicU32 = AtomicU32::new(0);
/// WM_CONTROL notifications the client got (mp1: id, code << 16).
static mut NOTES: Vec<u32> = Vec::new();

fn notes() -> &'static mut Vec<u32> {
    unsafe { &mut *core::ptr::addr_of_mut!(NOTES) }
}

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

fn noted(id: u32, code: u32) -> bool {
    notes().contains(&(code << 16 | id))
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == 0x0030 {
        notes().push(mp1);
        // Entry field 113 overflowing: make room (EM_SETTEXTLIMIT) and
        // say so, as VisualAge Smalltalk's text widgets do.
        if mp1 == mp(113, 0x0020) {
            send(mp2, 0x0143, 64, 0);
            return 1;
        }
        return 0;
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

fn text(h: u32) -> Vec<u8> {
    let mut b = [0u8; 512];
    let n = unsafe { WinQueryWindowText(h, b.len() as i32, b.as_mut_ptr()) } as usize;
    b[..n].to_vec()
}

fn create(parent: u32, class: u32, text: &[u8], style: u32, (x, y, cx, cy): (i32, i32, i32, i32), id: u32, ctl: *const u8) -> u32 {
    let mut t = text.to_vec();
    t.push(0);
    unsafe { WinCreateWindow(parent, 0xffff_0000 | class, t.as_ptr(), 0x8000_0000 | style, x, y, cx, cy, parent, 3, id, ctl, 0) }
}

/// Giveable shared memory with `t` and a NUL (for WinSetClipbrdData).
fn giveable(t: &[u8]) -> u32 {
    let mut p: *mut u8 = null_mut();
    let rc = unsafe { DosAllocSharedMem(&mut p, null(), t.len() as u32 + 1, PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE) };
    if rc != 0 {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), p, t.len());
        *p.add(t.len()) = 0;
    }
    p as u32
}

fn clip_text(hab: u32) -> Option<Vec<u8>> {
    let p = unsafe { WinQueryClipbrdData(hab, 1) };
    (p != 0).then(|| (0..).map(|i| unsafe { *(p as *const u8).add(i) }).take_while(|&b| b != 0).collect())
}

const SHARED_TEXT: &[u8] = b"Clipboard text from CTLTEST\r\nline two";

// ---- The MLE ----

const MLM_SETTEXTLIMIT: u32 = 0x01b0;
const MLM_SETWRAP: u32 = 0x01b4;
const MLM_QUERYCHANGED: u32 = 0x01ba;
const MLM_SETCHANGED: u32 = 0x01bb;
const MLM_QUERYLINECOUNT: u32 = 0x01bc;
const MLM_CHARFROMLINE: u32 = 0x01bd;
const MLM_LINEFROMCHAR: u32 = 0x01be;
const MLM_QUERYTEXTLENGTH: u32 = 0x01c0;
const MLM_SETIMPORTEXPORT: u32 = 0x01c2;
const MLM_IMPORT: u32 = 0x01c3;
const MLM_EXPORT: u32 = 0x01c4;
const MLM_DELETE: u32 = 0x01c6;
const MLM_QUERYFORMATTEXTLENGTH: u32 = 0x01c8;
const MLM_INSERT: u32 = 0x01c9;
const MLM_SETSEL: u32 = 0x01ca;
const MLM_QUERYSEL: u32 = 0x01cb;
const MLM_QUERYUNDO: u32 = 0x01cd;
const MLM_UNDO: u32 = 0x01ce;
const MLM_RESETUNDO: u32 = 0x01cf;
const MLM_SETREADONLY: u32 = 0x01b8;
const MLM_CUT: u32 = 0x01d8;
const MLM_COPY: u32 = 0x01d9;
const MLM_PASTE: u32 = 0x01da;
const MLM_SEARCH: u32 = 0x01de;

fn mle_checks(hab: u32, client: u32) {
    // MLS_BORDER | MLS_VSCROLL.
    let m = create(client, 10, b"one\r\ntwo\r\nthree", 0x0002 | 0x0004, (0, 0, 300, 200), 100, null());
    check("MLE created with its window text", m != 0 && text(m) == b"one\r\ntwo\r\nthree");
    check("MLM_QUERYTEXTLENGTH counts a line break as one", send(m, MLM_QUERYTEXTLENGTH, 0, 0) == 13);
    check("MLM_QUERYFORMATTEXTLENGTH in CF_TEXT", send(m, MLM_QUERYFORMATTEXTLENGTH, 0, 13) == 15);
    check("MLM_QUERYLINECOUNT", send(m, MLM_QUERYLINECOUNT, 0, 0) == 3);
    check("MLM_CHARFROMLINE / MLM_LINEFROMCHAR", send(m, MLM_CHARFROMLINE, 2, 0) == 8 && send(m, MLM_LINEFROMCHAR, 5, 0) == 1);
    send(m, MLM_SETCHANGED, 0, 0);
    send(m, MLM_SETSEL, 4, 7);
    check("MLM_QUERYSEL", send(m, MLM_QUERYSEL, 0, 0) == mp(4, 7) && send(m, MLM_QUERYSEL, 4, 0) == 7);
    let n = send(m, MLM_INSERT, b"TWO\0".as_ptr() as u32, 0);
    check("MLM_INSERT replaces the selection", n == 3 && text(m) == b"one\r\nTWO\r\nthree");
    check("MLM_QUERYCHANGED after an edit", send(m, MLM_QUERYCHANGED, 0, 0) == 1);
    check("MLN_CHANGE sent to the owner", noted(100, 7));
    check("MLM_QUERYUNDO", send(m, MLM_QUERYUNDO, 0, 0) & 0xffff != 0);
    check("MLM_UNDO", send(m, MLM_UNDO, 0, 0) == 1 && text(m) == b"one\r\ntwo\r\nthree");
    send(m, MLM_RESETUNDO, 0, 0);
    check("MLM_RESETUNDO", send(m, MLM_QUERYUNDO, 0, 0) == 0);
    // Export and import through a buffer.
    let mut buf = [0u8; 64];
    send(m, MLM_SETIMPORTEXPORT, buf.as_mut_ptr() as u32, buf.len() as u32);
    let (mut ipt, mut cb) = (0u32, 13u32);
    let got = send(m, MLM_EXPORT, &mut ipt as *mut u32 as u32, &mut cb as *mut u32 as u32);
    check("MLM_EXPORT", got == 15 && &buf[..15] == b"one\r\ntwo\r\nthree" && ipt == 13 && cb == 0);
    buf[..6].copy_from_slice(b"\r\nfour");
    send(m, MLM_SETIMPORTEXPORT, buf.as_mut_ptr() as u32, 6);
    let mut at = 13u32;
    let used = send(m, MLM_IMPORT, &mut at as *mut u32 as u32, 6);
    check("MLM_IMPORT", used == 6 && at == 18 && text(m) == b"one\r\ntwo\r\nthree\r\nfour");
    check("MLM_DELETE", send(m, MLM_DELETE, 13, 5) == 5 && send(m, MLM_QUERYTEXTLENGTH, 0, 0) == 13);
    // Search: MLE_SEARCHDATA cb, pchFind, pchReplace, cchFind, cchReplace,
    // iptStart, iptStop, cchFound.
    let mut sd = [0u8; 28];
    sd[0..2].copy_from_slice(&28u16.to_le_bytes());
    sd[4..8].copy_from_slice(&(b"THREE".as_ptr() as u32).to_le_bytes());
    sd[12..14].copy_from_slice(&5u16.to_le_bytes());
    sd[20..24].copy_from_slice(&(-1i32).to_le_bytes());
    let found = send(m, MLM_SEARCH, 2, sd.as_mut_ptr() as u32);
    let at = u32::from_le_bytes([sd[16], sd[17], sd[18], sd[19]]);
    check("MLM_SEARCH case-insensitive, selecting the match", found == 1 && at == 8 && send(m, MLM_QUERYSEL, 0, 0) == mp(8, 13));
    let found = send(m, MLM_SEARCH, 1 | 2, sd.as_mut_ptr() as u32);
    check("MLM_SEARCH case-sensitive misses", found == 0);
    // Word wrap: a long line in a narrow MLE.
    let w = create(client, 10, b"aaaa bbbb cccc dddd eeee ffff", 0x0001, (0, 0, 60, 100), 101, null());
    let lines = send(w, MLM_QUERYLINECOUNT, 0, 0);
    check("MLS_WORDWRAP wraps at blanks", lines >= 3);
    send(w, MLM_SETWRAP, 0, 0);
    check("MLM_SETWRAP off joins the lines", send(w, MLM_QUERYLINECOUNT, 0, 0) == 1);
    // Text limit and read-only.
    send(w, MLM_SETTEXTLIMIT, 32, 0);
    send(w, MLM_SETSEL, 29, 29);
    send(w, MLM_INSERT, b"1234567\0".as_ptr() as u32, 0);
    check("MLM_SETTEXTLIMIT refuses text beyond it (MLN_OVERFLOW)", send(w, MLM_QUERYTEXTLENGTH, 0, 0) == 29 && noted(101, 1));
    send(w, MLM_SETREADONLY, 1, 0);
    send(w, MLM_INSERT, b"x\0".as_ptr() as u32, 0);
    check("MLM_SETREADONLY", send(w, MLM_QUERYTEXTLENGTH, 0, 0) == 29);
    // Clipboard: cut and paste.
    send(m, MLM_SETSEL, 0, 3);
    check("MLM_COPY puts CF_TEXT on the clipboard", send(m, MLM_COPY, 0, 0) == 1 && unsafe { WinOpenClipbrd(hab) } != 0 && clip_text(hab).as_deref() == Some(b"one"));
    unsafe { WinCloseClipbrd(hab) };
    send(m, MLM_SETSEL, 0, 4);
    send(m, MLM_CUT, 0, 0);
    send(m, MLM_SETSEL, 0, 0);
    send(m, MLM_PASTE, 0, 0);
    send(m, MLM_PASTE, 0, 0);
    check("MLM_CUT and MLM_PASTE (line breaks too)", text(m) == b"one\r\none\r\ntwo\r\nthree");
    unsafe {
        WinDestroyWindow(m);
        WinDestroyWindow(w);
    }
}

// ---- Entry fields, combo boxes ----

fn entry_combo_checks(hab: u32, client: u32) {
    let e = create(client, 6, b"entry text", 0, (0, 0, 200, 20), 110, null());
    send(e, 0x0142, mp(0, 5), 0); // EM_SETSEL
    send(e, 0x0145, 0, 0); // EM_COPY
    let ok = unsafe { WinOpenClipbrd(hab) } != 0 && clip_text(hab).as_deref() == Some(b"entry");
    unsafe { WinCloseClipbrd(hab) };
    check("EM_COPY", ok);
    send(e, 0x0142, mp(10, 10), 0);
    send(e, 0x0147, 0, 0); // EM_PASTE
    check("EM_PASTE", text(e) == b"entry textentry");
    unsafe { WinDestroyWindow(e) };
    // At the text limit (32 without ENTRYFDATA) EN_OVERFLOW goes to the
    // owner; when it makes room the character goes in.
    let e = create(client, 6, b"0123456789012345678901234567890", 0, (0, 0, 300, 20), 113, null());
    send(e, 0x0142, mp(31, 31), 0); // EM_SETSEL at the end
    send(e, 0x007a, 0x0001 | 1 << 16 | 0x20 << 24, b'd' as u32); // WM_CHAR 'd'
    send(e, 0x007a, 0x0001 | 1 << 16 | 0x20 << 24, b'd' as u32);
    check("EN_OVERFLOW handled by the owner: the character goes in", noted(113, 0x20) && text(e) == b"0123456789012345678901234567890dd");
    unsafe { WinDestroyWindow(e) };
    // CBS_DROPDOWN.
    let c = create(client, 2, b"", 0x0002, (0, 0, 150, 120), 111, null());
    for t in [&b"Red\0"[..], b"Green\0", b"Blue\0"] {
        send(c, 0x0161, 0xffff_ffff, t.as_ptr() as u32); // LM_INSERTITEM, LIT_END
    }
    check("combo LM_QUERYITEMCOUNT", send(c, 0x0160, 0, 0) == 3);
    send(c, 0x0164, 1, 1); // LM_SELECTITEM
    check("combo selection fills the entry (CBN_LBSELECT)", text(c) == b"Green" && noted(111, 4));
    unsafe { WinSetWindowText(c, b"typed\0".as_ptr()) };
    check("combo window text is the entry's (CBN_EFCHANGE)", text(c) == b"typed" && noted(111, 1));
    check("combo list is up only when shown", send(c, 0x0172, 0, 0) == 0);
    send(c, 0x0170, 1, 0); // CBM_SHOWLIST
    check("CBM_SHOWLIST (CBN_SHOWLIST)", send(c, 0x0172, 0, 0) == 1 && noted(111, 6));
    send(c, 0x0170, 0, 0);
    unsafe { WinDestroyWindow(c) };
    // CBS_DROPDOWNLIST made with no size and sized after an item was
    // selected (VisualAge Smalltalk): the field shows its text from the
    // start.
    let c = create(client, 2, b"", 0x0004, (0, 0, 0, 0), 112, null());
    send(c, 0x0161, 0xffff_ffff, b"English\0".as_ptr() as u32);
    send(c, 0x0164, 0, 1);
    unsafe { WinSetWindowPos(c, 0, 10, 10, 150, 120, 3) }; // SWP_SIZE | SWP_MOVE
    let e = unsafe { WinWindowFromID(c, 0x029b) };
    check("combo sized after its selection shows the text from the start", e != 0 && text(c) == b"English" && send(e, 0x0148, 0, 0) == 0); // EM_QUERYFIRSTCHAR
    unsafe { WinDestroyWindow(c) };
}

// ---- Spin buttons, sliders, value sets ----

fn spin_slider_valueset_checks(client: u32) {
    // SPBS_MASTER | SPBS_NUMERICONLY.
    let s = create(client, 32, b"", 0x0010 | 0x0001, (0, 0, 80, 20), 120, null());
    check("SPBM_SETLIMITS", send(s, 0x0207, 10, 1) == 1);
    send(s, 0x0208, 9, 0); // SPBM_SETCURRENTVALUE
    send(s, 0x0203, 1, 0); // SPBM_SPINUP
    let mut v: i32 = 0;
    send(s, 0x0205, &mut v as *mut i32 as u32, mp(0, 0)); // SPBM_QUERYVALUE
    check("SPBM_SPINUP (SPBN_UPARROW)", v == 10 && noted(120, 0x20a));
    send(s, 0x0203, 1, 0);
    send(s, 0x0205, &mut v as *mut i32 as u32, 0);
    check("spinning past the upper limit wraps", v == 1);
    let mut b = [0u8; 16];
    send(s, 0x0205, b.as_mut_ptr() as u32, mp(16, 0));
    check("SPBM_QUERYVALUE as text", &b[..2] == b"1\0");
    let items = [b"Red\0".as_ptr() as u32, b"Green\0".as_ptr() as u32, b"Blue\0".as_ptr() as u32];
    send(s, 0x0206, items.as_ptr() as u32, 3); // SPBM_SETARRAY
    send(s, 0x0204, 1, 0); // SPBM_SPINDOWN
    send(s, 0x0205, b.as_mut_ptr() as u32, mp(16, 0));
    check("SPBM_SETARRAY and spinning down wraps", &b[..5] == b"Blue\0");
    unsafe { WinDestroyWindow(s) };
    // A horizontal slider with 11 increments.
    let mut sld = [0u8; 12];
    sld[0..4].copy_from_slice(&12u32.to_le_bytes());
    sld[4..6].copy_from_slice(&11u16.to_le_bytes());
    let sl = create(client, 38, b"", 0, (0, 0, 220, 40), 130, sld.as_ptr());
    // SLM_SETSLIDERINFO(SMA_SLIDERARMPOSITION, SMA_INCREMENTVALUE).
    send(sl, 0x0371, mp(3, 1), 7);
    check("SLM_SETSLIDERINFO / QUERYSLIDERINFO (SLN_CHANGE)", send(sl, 0x036c, mp(3, 1), 0) == 7 && noted(130, 1));
    check("SLM_SETTICKSIZE / QUERYTICKSIZE", send(sl, 0x0372, mp(4, 6), 0) == 1 && send(sl, 0x036e, 4, 0) == 6);
    send(sl, 0x0370, 10, b"Ten\0".as_ptr() as u32);
    let mut tb = [0u8; 8];
    check("SLM_SETSCALETEXT / QUERYSCALETEXT", send(sl, 0x036b, mp(10, 8), tb.as_mut_ptr() as u32) == 3 && &tb[..4] == b"Ten\0");
    unsafe { WinDestroyWindow(sl) };
    // A 2 x 3 value set of colours.
    let mut vsc = [0u8; 8];
    vsc[0..4].copy_from_slice(&8u32.to_le_bytes());
    vsc[4..6].copy_from_slice(&2u16.to_le_bytes());
    vsc[6..8].copy_from_slice(&3u16.to_le_bytes());
    // VS_RGB | VS_BORDER.
    let vs = create(client, 39, b"", 0x0008 | 0x0020, (0, 0, 120, 60), 140, vsc.as_ptr());
    check("VM_SETITEM / VM_QUERYITEM", send(vs, 0x037a, mp(2, 3), 0x00ff_0000) == 1 && send(vs, 0x0375, mp(2, 3), 0) == 0x00ff_0000);
    check("VM_SETITEMATTR / VM_QUERYITEMATTR", send(vs, 0x037b, mp(1, 2), mp(0x0040, 1)) == 1 && send(vs, 0x0376, mp(1, 2), 0) & 0x0040 != 0);
    check("a disabled item cannot be selected", send(vs, 0x0379, mp(1, 2), 0) == 0);
    send(vs, 0x0379, mp(2, 3), 0); // VM_SELECTITEM
    check("VM_SELECTITEM / VM_QUERYSELECTEDITEM (VN_SELECT)", send(vs, 0x0378, 0, 0) == mp(2, 3) && noted(140, 120));
    unsafe { WinDestroyWindow(vs) };
}

// ---- Clipboard and atoms ----

fn clipboard_checks(hab: u32) -> u32 {
    let ok = unsafe { WinOpenClipbrd(hab) } != 0;
    check("WinOpenClipbrd", ok);
    unsafe { WinEmptyClipbrd(hab) };
    let p = giveable(SHARED_TEXT);
    // CFI_POINTER.
    check("WinSetClipbrdData CF_TEXT from giveable memory", p != 0 && unsafe { WinSetClipbrdData(hab, p, 1, 0x0400) } != 0);
    // A 7 x 5 bitmap.
    let bm = unsafe {
        let hdc = DevOpenDC(hab, 8, b"*\0".as_ptr(), 0, null(), 0);
        let size = [0i32, 0];
        let hps = GpiCreatePS(hab, hdc, size.as_ptr(), 8 | 0x1000 | 0x4000);
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(&16u32.to_le_bytes());
        h[4..8].copy_from_slice(&7u32.to_le_bytes());
        h[8..12].copy_from_slice(&5u32.to_le_bytes());
        h[12..14].copy_from_slice(&1u16.to_le_bytes());
        h[14..16].copy_from_slice(&24u16.to_le_bytes());
        let bm = GpiCreateBitmap(hps, h.as_ptr(), 0, null(), null());
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
        bm
    };
    // CFI_HANDLE.
    check("WinSetClipbrdData CF_BITMAP", bm != 0 && unsafe { WinSetClipbrdData(hab, bm, 2, 0x0200) } != 0);
    let reg = unsafe { WinAddAtom(WinQuerySystemAtomTable(), b"CtlTest private format\0".as_ptr()) };
    let q = giveable(b"private");
    check("a registered format", reg >= 0xc000 && unsafe { WinSetClipbrdData(hab, q, reg, 0x0400) } != 0);
    let mut fl = 0;
    check("WinQueryClipbrdFmtInfo", unsafe { WinQueryClipbrdFmtInfo(hab, 1, &mut fl) } != 0 && fl & 0x0400 != 0);
    let f1 = unsafe { WinEnumClipbrdFmts(hab, 0) };
    let f2 = unsafe { WinEnumClipbrdFmts(hab, f1) };
    let f3 = unsafe { WinEnumClipbrdFmts(hab, f2) };
    check("WinEnumClipbrdFmts", f1 == 1 && f2 == 2 && f3 == reg && unsafe { WinEnumClipbrdFmts(hab, f3) } == 0);
    check("WinQueryClipbrdData CF_TEXT", clip_text(hab).as_deref() == Some(SHARED_TEXT));
    unsafe { WinCloseClipbrd(hab) };
    reg
}

fn atom_checks() {
    let t = unsafe { WinQuerySystemAtomTable() };
    let a = unsafe { WinAddAtom(t, b"Rs2 Test Atom\0".as_ptr()) };
    let b = unsafe { WinAddAtom(t, b"rs2 test atom\0".as_ptr()) };
    check("WinAddAtom (case-insensitive names share an atom)", a >= 0xc000 && a == b);
    let mut buf = [0u8; 32];
    let n = unsafe { WinQueryAtomName(t, a, buf.as_mut_ptr(), 32) } as usize;
    check("WinQueryAtomName", &buf[..n] == b"Rs2 Test Atom");
    check("WinFindAtom", unsafe { WinFindAtom(t, b"RS2 TEST ATOM\0".as_ptr()) } == a);
    check("integer atoms", unsafe { WinAddAtom(t, b"#42\0".as_ptr()) } == 42);
    unsafe {
        WinDeleteAtom(t, a);
        WinDeleteAtom(t, a);
    }
    check("WinDeleteAtom", unsafe { WinFindAtom(t, b"Rs2 Test Atom\0".as_ptr()) } == 0);
}

fn helper_checks(hab: u32) {
    let mut s = *b"abc\x87\x82xyz\0";
    let n = unsafe { WinUpper(hab, 850, 0, s.as_mut_ptr()) };
    check("WinUpper (code page 850)", n == 8 && &s[..8] == b"ABC\x80\x90XYZ");
    let (a, b) = ([0i32, 0, 10, 10], [5i32, 5, 20, 20]);
    let mut d = [0i32; 4];
    check("WinIntersectRect", unsafe { WinIntersectRect(hab, d.as_mut_ptr(), a.as_ptr(), b.as_ptr()) } != 0 && d == [5, 5, 10, 10]);
    check("WinQueryVersion", unsafe { WinQueryVersion(hab) } & 0xff == 20);
}

/// The child process: reads what the parent left on the clipboard and
/// replaces it; the exit code is the number of mismatches.
fn child(hab: u32, reg: u32) -> u32 {
    let mut bad = 0;
    unsafe {
        if WinOpenClipbrd(hab) == 0 {
            return 99;
        }
        if clip_text(hab).as_deref() != Some(SHARED_TEXT) {
            bad += 1;
        }
        let bm = WinQueryClipbrdData(hab, 2);
        let mut p = [0u8; 12];
        if bm == 0 || GpiQueryBitmapParameters(bm, p.as_mut_ptr()) == 0 || u16::from_le_bytes([p[4], p[5]]) != 7 || u16::from_le_bytes([p[6], p[7]]) != 5 {
            bad += 2;
        }
        let q = WinQueryClipbrdData(hab, reg);
        if q == 0 || core::slice::from_raw_parts(q as *const u8, 8) != b"private\0" {
            bad += 4;
        }
        WinEmptyClipbrd(hab);
        WinSetClipbrdData(hab, giveable(b"from the child"), 1, 0x0400);
        WinCloseClipbrd(hab);
    }
    bad
}

fn pump(hab: u32) {
    let mut q = QMsg::default();
    while unsafe { WinPeekMsg(hab, &mut q, 0, 0, 0, 1) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
}

fn main(s: &Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let args = s.args();
    let args: Vec<u8> = args.iter().copied().skip_while(|&c| c == b' ').collect();
    if args.starts_with(b"CHILD") {
        let reg = unsafe { WinAddAtom(WinQuerySystemAtomTable(), b"CtlTest private format\0".as_ptr()) };
        let r = child(hab, reg);
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return r;
    }
    check("WinInitialize/WinCreateMsgQueue", hab != 0 && hmq != 0);
    unsafe { WinRegisterClass(hab, b"CtlTest\0".as_ptr(), client_proc, 0, 0) };
    let mut fcf = 1u32 | 2; // FCF_TITLEBAR | FCF_SYSMENU (hidden)
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(1, 0, &mut fcf, b"CtlTest\0".as_ptr(), b"Controls test\0".as_ptr(), 0, 0, 1, &mut client) };
    check("hidden test window", frame != 0 && client != 0);
    mle_checks(hab, client);
    pump(hab);
    entry_combo_checks(hab, client);
    spin_slider_valueset_checks(client);
    atom_checks();
    helper_checks(hab);
    wm::wm_checks(hab, hmq, client, frame);
    pm16::pm16_checks(hab, client);
    dlg::checks(hab, client);
    clipboard_checks(hab);
    // The clipboard seen from another process.
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, b"CTLTEST\0CHILD\0\0".as_ptr(), null(), &mut res, b"C:\\RS2\\CTLTEST.EXE\0".as_ptr()) };
    check("another process reads CF_TEXT, CF_BITMAP and a registered format", rc == 0 && res.code_result == 0);
    if res.code_result != 0 {
        println!("  child result {}", res.code_result);
    }
    let back = unsafe {
        WinOpenClipbrd(hab);
        let t = clip_text(hab);
        WinCloseClipbrd(hab);
        t
    };
    check("what another process put on the clipboard", back.as_deref() == Some(b"from the child"));
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    let f = FAILS.load(Ordering::Relaxed);
    println!("ctltest done: {f} failure(s)");
    f
}
