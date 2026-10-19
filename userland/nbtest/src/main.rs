//! Self-checks of PMWIN's notebook control (WC_NOTEBOOK): inserting,
//! deleting, querying and turning pages, tab and status texts, page
//! data, page windows (sized to the page, only the top one shown), pages
//! loaded from a dialog template when first turned to, the BKN_*
//! notifications, keys and clicks. Each check prints PASS or FAIL; then
//! "nbtest done: N failure(s)", and the window with a settings-style
//! notebook and a tabbed-dialog notebook stays up until it is closed
//! (`NBTEST QUIT` ends at once). The exit code is N.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec::Vec;
use core::ptr::null;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use os2::*;

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
    #[link_name = "PMWIN.910"]
    fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.918"]
    fn WinPeekMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.837"]
    fn WinQueryWindowPos(h: u32, swp: *mut i32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(h: u32, r: *mut i32) -> u32;
    #[link_name = "PMWIN.775"]
    fn WinIsWindowVisible(h: u32) -> u32;
    #[link_name = "PMWIN.834"]
    fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.860"]
    fn WinSetFocus(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.817"]
    fn WinQueryFocus(desk: u32) -> u32;
    #[link_name = "PMWIN.829"]
    fn WinQuerySysValue(desk: u32, i: i32) -> i32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, r: *mut i32) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const i32, c: i32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.598"]
    fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, init: *const u8, bmi: *const u8) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
}

const WM_PAINT: u32 = 0x0023;
const WM_COMMAND: u32 = 0x0020;
const WM_CONTROL: u32 = 0x0030;
const WM_INITDLG: u32 = 0x003b;
const WM_CHAR: u32 = 0x007a;
const WM_BUTTON1DOWN: u32 = 0x0071;
const WM_MATCHMNEMONIC: u32 = 0x003d;

const BKM_CALCPAGERECT: u32 = 0x0353;
const BKM_DELETEPAGE: u32 = 0x0354;
const BKM_INSERTPAGE: u32 = 0x0355;
const BKM_INVALIDATETABS: u32 = 0x0356;
const BKM_TURNTOPAGE: u32 = 0x0357;
const BKM_QUERYPAGECOUNT: u32 = 0x0358;
const BKM_QUERYPAGEID: u32 = 0x0359;
const BKM_QUERYPAGEDATA: u32 = 0x035a;
const BKM_QUERYPAGEWINDOWHWND: u32 = 0x035b;
const BKM_QUERYTABBITMAP: u32 = 0x035c;
const BKM_QUERYTABTEXT: u32 = 0x035d;
const BKM_SETDIMENSIONS: u32 = 0x035e;
const BKM_SETPAGEDATA: u32 = 0x035f;
const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
const BKM_SETTABBITMAP: u32 = 0x0362;
const BKM_SETTABTEXT: u32 = 0x0363;
const BKM_SETNOTEBOOKCOLORS: u32 = 0x0364;
const BKM_QUERYPAGESTYLE: u32 = 0x0365;
const BKM_QUERYSTATUSLINETEXT: u32 = 0x0366;
const BKM_SETPAGEINFO: u32 = 0x0367;
const BKM_QUERYPAGEINFO: u32 = 0x0368;
const BKM_SETTABCOLOR: u32 = 0x0374;
const BKM_SETNOTEBOOKBUTTONS: u32 = 0x0375;

const BKN_PAGESELECTED: u32 = 130;
const BKN_NEWPAGESIZE: u32 = 131;
const BKN_HELP: u32 = 132;
const BKN_PAGEDELETED: u32 = 133;
const BKN_PAGESELECTEDPENDING: u32 = 134;

const BKA_SINGLE: u32 = 0x0002;
const BKA_TAB: u32 = 0x0004;
const BKA_LAST: u32 = 0x0002;
const BKA_FIRST: u32 = 0x0004;
const BKA_NEXT: u32 = 0x0008;
const BKA_TOP: u32 = 0x0020;
const BKA_STATUSTEXTON: u32 = 0x0001;
const BKA_MAJOR: u32 = 0x0040;
const BKA_MINOR: u32 = 0x0080;
const BKA_AUTOPAGESIZE: u32 = 0x0100;
const BKA_END: u32 = 0x0200;
const BKA_MAJORTAB: u32 = 0x0001;

const BKS_BACKPAGESBR: u32 = 0x0001;
const BKS_MAJORTABRIGHT: u32 = 0x0010;
const BKS_ROUNDEDTABS: u32 = 0x0100;
const BKS_SPIRALBIND: u32 = 0x0400;
const BKS_TABBEDDIALOG: u32 = 0x0800;
const BKS_BUTTONAREA: u32 = 0x0200;
const BKS_STATUSTEXTCENTER: u32 = 0x2000;
const BKS_TABTEXTCENTER: u32 = 0x8000;

const BFA_PAGEDATA: u32 = 0x0001;
const BFA_PAGEFROMHWND: u32 = 0x0002;
const BFA_PAGEFROMDLGTEMPLATE: u32 = 0x0004;
const BFA_STATUSLINE: u32 = 0x0010;
const BFA_MAJORTABTEXT: u32 = 0x0080;

const WS_VISIBLE: u32 = 0x8000_0000;
const WS_GROUP: u32 = 0x0001_0000;
const WS_TABSTOP: u32 = 0x0002_0000;
const KC_CHAR: u32 = 0x0001;
const KC_VIRTUALKEY: u32 = 0x0002;
const KC_ALT: u32 = 0x0020;
const VK_TAB: u32 = 0x06;
const VK_PAGEDOWN: u32 = 0x12;
const VK_HOME: u32 = 0x14;
const VK_DOWN: u32 = 0x18;

const ERR: u32 = 0xffff_ffff;

static FAILS: AtomicU32 = AtomicU32::new(0);
/// Refuse the next page turn in BKN_PAGESELECTEDPENDING.
static REFUSE: AtomicBool = AtomicBool::new(false);
static INITDLG: AtomicU32 = AtomicU32::new(0);

/// A notification: (control id, code, the notification's fields).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Note {
    id: u32,
    code: u32,
    a: u32,
    b: u32,
    c: u32,
}

static mut NOTES: Vec<Note> = Vec::new();

fn notes() -> &'static mut Vec<Note> {
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

fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}

fn create(parent: u32, class: u32, text: &[u8], style: u32, (x, y, cx, cy): (i32, i32, i32, i32), owner: u32, id: u32) -> u32 {
    let t = z(text);
    let class = if class < 0x10000 { 0xffff_0000 | class } else { class };
    unsafe { WinCreateWindow(parent, class, t.as_ptr(), style, x, y, cx, cy, owner, 3, id, null(), 0) }
}

fn rect(h: u32) -> [i32; 4] {
    let mut r = [0i32; 4];
    unsafe { WinQueryWindowRect(h, r.as_mut_ptr()) };
    r
}

/// (x, y, cx, cy) of a window.
fn pos(h: u32) -> (i32, i32, i32, i32) {
    let mut swp = [0i32; 9];
    unsafe { WinQueryWindowPos(h, swp.as_mut_ptr()) };
    (swp[4], swp[3], swp[2], swp[1])
}

/// The page rectangle of a notebook (BKM_CALCPAGERECT from its window
/// rectangle).
fn page_rect(nb: u32) -> [i32; 4] {
    let mut r = rect(nb);
    send(nb, BKM_CALCPAGERECT, r.as_mut_ptr() as u32, 1);
    r
}

fn tab_text(nb: u32, id: u32, status: bool) -> Vec<u8> {
    let mut buf = [0u8; 64];
    let bt = [buf.as_mut_ptr() as u32, buf.len() as u32];
    let n = send(nb, if status { BKM_QUERYSTATUSLINETEXT } else { BKM_QUERYTABTEXT }, id, bt.as_ptr() as u32);
    if n == ERR {
        return b"<error>".to_vec();
    }
    buf[..(n as usize).min(63)].to_vec()
}

fn top(nb: u32) -> u32 {
    send(nb, BKM_QUERYPAGEID, 0, mp(BKA_TOP, 0))
}

fn key(h: u32, fs: u32, ch: u32, vk: u32) -> u32 {
    send(h, WM_CHAR, fs, ch | vk << 16)
}

fn noted(code: u32) -> Option<Note> {
    notes().iter().rev().find(|n| n.code == code).copied()
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            let field = |i: u32| if mp2 != 0 && mp2 > 0xffff { unsafe { ((mp2 + 4 * i) as *const u32).read_unaligned() } } else { 0 };
            let n = match code {
                BKN_PAGESELECTED | BKN_PAGESELECTEDPENDING => Note { id, code, a: field(0), b: field(1), c: field(2) },
                BKN_PAGEDELETED => Note { id, code, a: field(1), b: field(2), c: field(3) },
                _ => Note { id, code, a: mp2, b: 0, c: 0 },
            };
            if code == BKN_PAGESELECTEDPENDING && REFUSE.swap(false, Ordering::Relaxed) {
                // ulPageIdNew = 0 refuses the turn.
                unsafe { ((mp2 + 8) as *mut u32).write_unaligned(0) };
            }
            notes().push(n);
            0
        }
        WM_PAINT => {
            unsafe {
                let mut r = [0i32; 4];
                let hps = WinBeginPaint(h, 0, r.as_mut_ptr());
                WinFillRect(hps, r.as_ptr(), -34);
                WinEndPaint(hps);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

/// A page window class: dialog background, keys go up to the notebook.
extern "C" fn page_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            unsafe {
                let mut r = [0i32; 4];
                let hps = WinBeginPaint(h, 0, r.as_mut_ptr());
                WinFillRect(hps, r.as_ptr(), -34);
                WinEndPaint(hps);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

extern "C" fn page_dlg_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            INITDLG.fetch_add(1, Ordering::Relaxed);
            0
        }
        // Page dialogs stay: commands are not dismissals.
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, mp1, mp2) },
    }
}

// ---- A dialog template in memory (DLGTEMPLATE) ----

struct Item {
    class: u16,
    text: &'static [u8],
    style: u32,
    r: (i16, i16, i16, i16),
    id: u16,
}

fn template(items: &[Item]) -> Vec<u8> {
    let n = items.len();
    let head = 14;
    let base = head + 30 * n;
    let mut out = alloc::vec![0u8; base];
    let mut extra: Vec<u8> = Vec::new();
    let put16 = |o: &mut Vec<u8>, at: usize, v: u16| o[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put16(&mut out, 4, 850);
    put16(&mut out, 6, head as u16);
    put16(&mut out, 8, 1);
    put16(&mut out, 10, 0xffff);
    for (i, it) in items.iter().enumerate() {
        let at = head + 30 * i;
        let toff = base + extra.len();
        extra.extend_from_slice(it.text);
        extra.push(0);
        put16(&mut out, at + 2, if i == 0 { (n - 1) as u16 } else { 0 });
        put16(&mut out, at + 6, it.class);
        put16(&mut out, at + 8, it.text.len() as u16 + 1);
        put16(&mut out, at + 10, toff as u16);
        out[at + 12..at + 16].copy_from_slice(&it.style.to_le_bytes());
        put16(&mut out, at + 16, it.r.0 as u16);
        put16(&mut out, at + 18, it.r.1 as u16);
        put16(&mut out, at + 20, it.r.2 as u16);
        put16(&mut out, at + 22, it.r.3 as u16);
        put16(&mut out, at + 24, it.id);
        put16(&mut out, at + 26, 0xffff);
        put16(&mut out, at + 28, 0xffff);
    }
    out.extend_from_slice(&extra);
    let len = out.len() as u16;
    put16(&mut out, 0, len);
    out
}

fn advanced_template() -> Vec<u8> {
    let tab = WS_VISIBLE | WS_TABSTOP;
    template(&[
        Item { class: 1, text: b"", style: 0, r: (0, 0, 150, 100), id: 300 },
        Item { class: 5, text: b"Advanced settings", style: WS_VISIBLE | 0x0001, r: (8, 80, 140, 10), id: 301 },
        Item { class: 3, text: b"~Use the cache", style: tab | WS_GROUP | 0x0005, r: (8, 62, 120, 10), id: 302 },
        Item { class: 3, text: b"~Log requests", style: tab | 0x0005, r: (8, 48, 120, 10), id: 303 },
        Item { class: 5, text: b"Cache size:", style: WS_VISIBLE | 0x0001, r: (8, 30, 50, 10), id: 304 },
        Item { class: 6, text: b"1024", style: tab | WS_GROUP | 0x0001, r: (62, 30, 50, 10), id: 305 },
    ])
}

fn small_bitmap(hab: u32) -> u32 {
    unsafe {
        let hdc = DevOpenDC(hab, 8, b"*\0".as_ptr(), 0, null(), 0);
        let size = [0i32, 0];
        let hps = GpiCreatePS(hab, hdc, size.as_ptr(), 8 | 0x1000 | 0x4000);
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(&16u32.to_le_bytes());
        h[4..8].copy_from_slice(&12u32.to_le_bytes());
        h[8..12].copy_from_slice(&12u32.to_le_bytes());
        h[12..14].copy_from_slice(&1u16.to_le_bytes());
        h[14..16].copy_from_slice(&24u16.to_le_bytes());
        let bm = GpiCreateBitmap(hps, h.as_ptr(), 0, null(), null());
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
        bm
    }
}

fn pump(hab: u32) {
    let mut q = QMsg::default();
    while unsafe { WinPeekMsg(hab, &mut q, 0, 0, 0, 1) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
}

fn visible(h: u32) -> bool {
    unsafe { WinIsWindowVisible(h) != 0 }
}

fn parent(h: u32) -> u32 {
    unsafe { WinQueryWindow(h, 5) }
}

fn checks(hab: u32, client: u32) -> u32 {
    let style = WS_VISIBLE | WS_TABSTOP | BKS_BACKPAGESBR | BKS_MAJORTABRIGHT | BKS_ROUNDEDTABS | BKS_SPIRALBIND | BKS_STATUSTEXTCENTER | BKS_TABTEXTCENTER;
    let nb = create(client, 40, b"", style, (8, 8, 440, 380), client, 10);
    check("WC_NOTEBOOK created", nb != 0);
    check("an empty notebook has no pages", send(nb, BKM_QUERYPAGECOUNT, 0, BKA_END) == 0 && top(nb) == 0);
    let ps = BKA_STATUSTEXTON | BKA_AUTOPAGESIZE;
    let general = send(nb, BKM_INSERTPAGE, 0, mp(ps | BKA_MAJOR, BKA_LAST));
    check("BKM_INSERTPAGE returns a page id", general != 0 && general != ERR);
    check("the first page goes on top", top(nb) == general);
    let advanced = send(nb, BKM_INSERTPAGE, 0, mp(ps | BKA_MAJOR, BKA_LAST));
    let colors = send(nb, BKM_INSERTPAGE, general, mp(ps | BKA_MINOR, BKA_NEXT));
    let about = send(nb, BKM_INSERTPAGE, 0, mp(ps | BKA_MAJOR, BKA_LAST));
    let first = send(nb, BKM_QUERYPAGEID, 0, mp(BKA_FIRST, 0));
    let second = send(nb, BKM_QUERYPAGEID, first, mp(BKA_NEXT, 0));
    let third = send(nb, BKM_QUERYPAGEID, second, mp(BKA_NEXT, 0));
    check("BKA_LAST and BKA_NEXT order the pages", first == general && second == colors && third == advanced);
    check("BKM_QUERYPAGEID with BKA_MAJOR skips minor pages", send(nb, BKM_QUERYPAGEID, general, mp(BKA_NEXT, BKA_MAJOR)) == advanced);
    check("BKM_QUERYPAGEID of a bad page is BOOKERR_INVALID_PARAMETERS", send(nb, BKM_QUERYPAGEID, 0x7777, mp(BKA_NEXT, 0)) == ERR);
    check("BKM_QUERYPAGECOUNT to the end", send(nb, BKM_QUERYPAGECOUNT, 0, BKA_END) == 4);
    check("BKM_QUERYPAGECOUNT of a major section", send(nb, BKM_QUERYPAGECOUNT, general, BKA_MAJOR) == 2);
    check("BKM_QUERYPAGESTYLE", send(nb, BKM_QUERYPAGESTYLE, colors, 0) == ps | BKA_MINOR);
    for (id, t, s) in [(general, b"~General" as &[u8], b"General: page 1 of 2" as &[u8]), (colors, b"~Colors", b"General: page 2 of 2"), (advanced, b"~Advanced", b"Advanced")] {
        let (t, s) = (z(t), z(s));
        send(nb, BKM_SETTABTEXT, id, t.as_ptr() as u32);
        send(nb, BKM_SETSTATUSLINETEXT, id, s.as_ptr() as u32);
    }
    check("BKM_QUERYTABTEXT", tab_text(nb, general, false) == b"~General" && tab_text(nb, colors, false) == b"~Colors");
    check("BKM_QUERYTABTEXT length without a buffer", send(nb, BKM_QUERYTABTEXT, advanced, 0) == 9);
    check("BKM_QUERYSTATUSLINETEXT", tab_text(nb, colors, true) == b"General: page 2 of 2");
    check("BKM_SETPAGEDATA / BKM_QUERYPAGEDATA", send(nb, BKM_SETPAGEDATA, colors, 0x1234_5678) == 1 && send(nb, BKM_QUERYPAGEDATA, colors, 0) == 0x1234_5678);
    check("BKM_QUERYPAGEDATA of a bad page", send(nb, BKM_QUERYPAGEDATA, 0x7777, 0) == ERR);

    // Page windows: children of the client handed to the notebook.
    let page1 = unsafe { WinCreateWindow(client, b"NbPage\0".as_ptr() as u32, b"\0".as_ptr(), WS_VISIBLE, 0, 0, 0, 0, 0, 3, 0, null(), 0) };
    create(page1, 5, b"Settings of the kind a settings notebook holds.", WS_VISIBLE | 0x0001, (8, 200, 340, 20), page1, 200);
    let cb = create(page1, 3, b"~Enable the feature", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (8, 170, 240, 20), page1, 201);
    create(page1, 3, b"~Show hidden files", WS_VISIBLE | WS_TABSTOP | 0x0005, (8, 146, 240, 20), page1, 202);
    create(page1, 5, b"Name:", WS_VISIBLE | 0x0001, (8, 110, 60, 20), page1, 203);
    create(page1, 6, b"rs2", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (72, 112, 160, 18), page1, 204);
    check("BKM_SETPAGEWINDOWHWND", send(nb, BKM_SETPAGEWINDOWHWND, general, page1) == 1);
    check("BKM_QUERYPAGEWINDOWHWND", send(nb, BKM_QUERYPAGEWINDOWHWND, general, 0) == page1);
    check("the page window becomes the notebook's child", parent(page1) == nb);
    let pr = page_rect(nb);
    check("BKA_AUTOPAGESIZE sizes the page window to the page", pos(page1) == (pr[0], pr[1], pr[2] - pr[0], pr[3] - pr[1]) && pr[2] - pr[0] > 200 && pr[3] - pr[1] > 200);
    let mle = create(client, 10, b"Notebook pages are ordinary windows: the notebook takes them as children, sizes them to the page and shows the top one.", WS_VISIBLE | 0x0001 | 0x0008, (0, 0, 0, 0), 0, 0);
    send(nb, BKM_SETPAGEWINDOWHWND, colors, mle);
    check("only the top page's window shows", visible(page1) && !visible(mle));

    // A page from a dialog template, loaded when it is first turned to.
    let t = advanced_template();
    let mut info = [0u32; 19];
    info[0] = 76;
    info[1] = BFA_PAGEFROMDLGTEMPLATE | BFA_PAGEDATA;
    info[2] = 0;
    info[3] = 77;
    info[5] = page_dlg_proc as *const () as u32;
    info[9] = t.as_ptr() as u32;
    check("BKM_SETPAGEINFO with a dialog template", send(nb, BKM_SETPAGEINFO, advanced, info.as_ptr() as u32) == 1);
    check("the page dialog waits until the page is turned to", send(nb, BKM_QUERYPAGEWINDOWHWND, advanced, 0) == 0 && INITDLG.load(Ordering::Relaxed) == 0);
    // A page from a window with the status line and tab text in the
    // BOOKPAGEINFO.
    let about_w = create(client, 5, b"rs2 notebook test\nWC_NOTEBOOK on rs2's own PMWIN.", WS_VISIBLE | 0x0001 | 0x0100 | 0x0400 | 0x4000, (0, 0, 0, 0), 0, 0);
    let (st_about, tab_about) = (z(b"About this test"), z(b"A~bout"));
    let mut info = [0u32; 19];
    info[0] = 76;
    info[1] = BFA_PAGEFROMHWND | BFA_STATUSLINE | BFA_MAJORTABTEXT | BFA_PAGEDATA;
    info[3] = 99;
    info[4] = about_w;
    info[10] = st_about.len() as u32 - 1;
    info[11] = st_about.as_ptr() as u32;
    info[14] = tab_about.len() as u32 - 1;
    info[15] = tab_about.as_ptr() as u32;
    send(nb, BKM_SETPAGEINFO, about, info.as_ptr() as u32);
    let mut q = [0u32; 19];
    let mut sbuf = [0u8; 32];
    q[0] = 76;
    q[1] = BFA_PAGEFROMHWND | BFA_STATUSLINE | BFA_PAGEDATA;
    q[10] = sbuf.len() as u32;
    q[11] = sbuf.as_mut_ptr() as u32;
    let ok = send(nb, BKM_QUERYPAGEINFO, about, q.as_mut_ptr() as u32) == 1;
    check("BKM_SETPAGEINFO / BKM_QUERYPAGEINFO", ok && q[3] == 99 && q[4] == about_w && &sbuf[..15] == b"About this test" && tab_text(nb, about, false) == b"A~bout");

    // Turning pages.
    notes().clear();
    check("BKM_TURNTOPAGE", send(nb, BKM_TURNTOPAGE, colors, 0) == 1 && top(nb) == colors);
    let pend = noted(BKN_PAGESELECTEDPENDING);
    let sel = noted(BKN_PAGESELECTED);
    check("BKN_PAGESELECTEDPENDING and BKN_PAGESELECTED (PAGESELECTNOTIFY)", pend == Some(Note { id: 10, code: BKN_PAGESELECTEDPENDING, a: nb, b: general, c: colors }) && sel == Some(Note { id: 10, code: BKN_PAGESELECTED, a: nb, b: general, c: colors }));
    check("the new top page's window shows, the old one is hidden", visible(mle) && !visible(page1) && pos(mle).2 == pr[2] - pr[0]);
    REFUSE.store(true, Ordering::Relaxed);
    check("clearing ulPageIdNew refuses the turn", send(nb, BKM_TURNTOPAGE, about, 0) == 0 && top(nb) == colors);
    send(nb, BKM_TURNTOPAGE, advanced, 0);
    let adv_w = send(nb, BKM_QUERYPAGEWINDOWHWND, advanced, 0);
    check("turning to the page loads its dialog", adv_w != 0 && adv_w != ERR && INITDLG.load(Ordering::Relaxed) == 1 && parent(adv_w) == nb && visible(adv_w) && !visible(mle));
    check("the page dialog has its controls", unsafe { os2_window_from_id(adv_w, 302) } != 0);

    // Keys.
    unsafe { WinSetFocus(1, nb) };
    key(nb, KC_VIRTUALKEY, 0, VK_HOME);
    check("Home turns to the first page", top(nb) == general);
    key(nb, KC_VIRTUALKEY, 0, VK_DOWN);
    check("Down moves to the next tab (the minor tab)", top(nb) == colors);
    key(nb, KC_VIRTUALKEY, 0, VK_DOWN);
    check("Down again: the next major tab", top(nb) == advanced);
    key(nb, KC_CHAR, b'b' as u32, 0);
    check("a tab's mnemonic turns to its page", top(nb) == about);
    check("WM_MATCHMNEMONIC turns to the matching tab", send(nb, WM_MATCHMNEMONIC, b'g' as u32, 0) == 1 && top(nb) == general);
    check("Tab moves the focus into the page", key(nb, KC_VIRTUALKEY, 0, VK_TAB) == 1 && unsafe { WinQueryFocus(1) } == cb);
    key(cb, KC_VIRTUALKEY | KC_ALT, 0, VK_PAGEDOWN);
    check("Alt+PgDn in the page turns to the next page", top(nb) == colors);
    check("the focus left the page that went away", unsafe { WinQueryFocus(1) } == nb);

    // The page buttons: next is at the right end of the status line,
    // below the page.
    let pr = page_rect(nb);
    let (x, y) = (pr[2] - 10, pr[1] - 14);
    send(nb, WM_BUTTON1DOWN, mp(x as u32, y as u32), 0);
    check("a click on the next-page button turns the page", top(nb) == advanced);
    send(nb, WM_BUTTON1DOWN, mp((x - 21) as u32, y as u32), 0);
    check("a click on the previous-page button", top(nb) == colors);

    // Sizes.
    notes().clear();
    unsafe { WinSetWindowPos(nb, 0, 8, 8, 430, 370, 1 | 2) };
    let pr = page_rect(nb);
    check("BKN_NEWPAGESIZE when the notebook is sized", noted(BKN_NEWPAGESIZE).is_some_and(|n| n.a == nb));
    check("the top page window follows the page size", pos(mle) == (pr[0], pr[1], pr[2] - pr[0], pr[3] - pr[1]));
    unsafe { WinSetWindowPos(nb, 0, 8, 8, 440, 380, 1 | 2) };
    let before = page_rect(nb);
    check("BKM_SETDIMENSIONS", send(nb, BKM_SETDIMENSIONS, mp(100, 30), BKA_MAJORTAB) == 1 && page_rect(nb)[2] < before[2]);
    let mut r = page_rect(nb);
    send(nb, BKM_CALCPAGERECT, r.as_mut_ptr() as u32, 0);
    check("BKM_CALCPAGERECT back from the page to the notebook", r == rect(nb));

    // Bitmaps, colours.
    let bm = small_bitmap(hab);
    check("BKM_SETTABBITMAP / BKM_QUERYTABBITMAP", bm != 0 && send(nb, BKM_SETTABBITMAP, about, bm) == 1 && send(nb, BKM_QUERYTABBITMAP, about, 0) == bm);
    send(nb, BKM_SETTABBITMAP, about, 0);
    check("BKM_SETNOTEBOOKCOLORS, BKM_SETTABCOLOR, BKM_INVALIDATETABS", send(nb, BKM_SETNOTEBOOKCOLORS, 0xe0e0e0, 2) == 1 && send(nb, BKM_SETTABCOLOR, advanced, 0xffffc0) == 1 && send(nb, BKM_INVALIDATETABS, 0, 0) == 1);

    // Deleting.
    let extra = send(nb, BKM_INSERTPAGE, 0, mp(ps | BKA_MAJOR, BKA_LAST));
    let extra2 = send(nb, BKM_INSERTPAGE, extra, mp(ps, BKA_NEXT));
    let ew = create(client, 5, b"temporary", WS_VISIBLE | 1, (0, 0, 0, 0), 0, 0);
    send(nb, BKM_SETPAGEWINDOWHWND, extra, ew);
    send(nb, BKM_SETPAGEDATA, extra, 4242);
    send(nb, BKM_TURNTOPAGE, extra, 0);
    notes().clear();
    check("BKM_DELETEPAGE BKA_TAB deletes the section", send(nb, BKM_DELETEPAGE, extra, BKA_TAB) == 1 && send(nb, BKM_QUERYPAGECOUNT, 0, BKA_END) == 4);
    let dels: Vec<Note> = notes().iter().filter(|n| n.code == BKN_PAGEDELETED).copied().collect();
    check("BKN_PAGEDELETED (DELETENOTIFY) for each page", dels.len() == 2 && dels[0].a == ew && dels[0].b == 4242);
    check("deleting the top page turns to another", top(nb) == about && !visible(ew));
    let _ = extra2;
    let tmp = send(nb, BKM_INSERTPAGE, 0, mp(0, BKA_FIRST));
    check("BKM_DELETEPAGE BKA_SINGLE", send(nb, BKM_DELETEPAGE, tmp, BKA_SINGLE) == 1 && send(nb, BKM_QUERYPAGEID, 0, mp(BKA_FIRST, 0)) == general);
    check("BKM_DELETEPAGE of a bad page fails", send(nb, BKM_DELETEPAGE, 0x7777, BKA_SINGLE) == 0);
    unsafe { WinDestroyWindow(ew) };

    // Help.
    notes().clear();
    send(nb, 0x0022, 0, 0);
    check("WM_HELP sends BKN_HELP with the top page", noted(BKN_HELP).is_some_and(|n| n.a == about));
    send(nb, BKM_TURNTOPAGE, general, 0);
    pump(hab);

    // A Warp 4 tabbed dialog with notebook buttons.
    let tb = create(client, 40, b"", WS_VISIBLE | WS_TABSTOP | BKS_TABBEDDIALOG | BKS_BUTTONAREA, (456, 8, 300, 380), client, 11);
    for (t, txt) in [(b"~View" as &[u8], b"Show the view settings." as &[u8]), (b"~Sort", b"How items are sorted."), (b"~Font", b"The font of the window.")] {
        let id = send(tb, BKM_INSERTPAGE, 0, mp(BKA_MAJOR | BKA_AUTOPAGESIZE, BKA_LAST));
        let tt = z(t);
        send(tb, BKM_SETTABTEXT, id, tt.as_ptr() as u32);
        let w = create(client, 5, txt, WS_VISIBLE | 0x0001 | 0x0400 | 0x4000, (0, 0, 0, 0), 0, 0);
        send(tb, BKM_SETPAGEWINDOWHWND, id, w);
    }
    let (b1, b2) = (z(b"~Undo"), z(b"~Default"));
    let btns: [u32; 8] = [b1.as_ptr() as u32, 500, 0, 0, b2.as_ptr() as u32, 501, 0, 0];
    check("BKM_SETNOTEBOOKBUTTONS", send(tb, BKM_SETNOTEBOOKBUTTONS, 2, btns.as_ptr() as u32) == 1 && unsafe { os2_window_from_id(tb, 500) } != 0);
    let tp = page_rect(tb);
    check("the button area is below the page", tp[1] >= 34);
    pump(hab);
    nb
}

unsafe extern "C" {
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(parent: u32, id: u32) -> u32;
}

unsafe fn os2_window_from_id(h: u32, id: u32) -> u32 {
    unsafe { WinWindowFromID(h, id) }
}

fn main(s: &Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    check("WinInitialize/WinCreateMsgQueue", hab != 0 && hmq != 0);
    let quit = s.args().iter().copied().skip_while(|&c| c == b' ').collect::<Vec<u8>>().starts_with(b"QUIT");
    unsafe {
        WinRegisterClass(hab, b"NbTest\0".as_ptr(), client_proc, 0, 0);
        WinRegisterClass(hab, b"NbPage\0".as_ptr(), page_proc, 0, 0);
    }
    let mut fcf = 1u32 | 2 | 8 | 0x20; // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(1, 0, &mut fcf, b"NbTest\0".as_ptr(), b"Notebook test\0".as_ptr(), 0, 0, 1, &mut client) };
    check("test window", frame != 0 && client != 0);
    unsafe {
        let (sw, sh) = (WinQuerySysValue(1, 20), WinQuerySysValue(1, 21));
        let (w, h) = (780.min(sw), 440.min(sh));
        // SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE
        WinSetWindowPos(frame, 0, (sw - w) / 2, (sh - h) / 2, w, h, 0x1 | 0x2 | 0x8 | 0x80);
    }
    pump(hab);
    let nb = checks(hab, client);
    let f = FAILS.load(Ordering::Relaxed);
    println!("nbtest done: {f} failure(s)");
    if !quit {
        unsafe { WinSetFocus(1, nb) };
        let mut q = QMsg::default();
        while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
            unsafe { WinDispatchMsg(hab, &q) };
        }
    }
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    f
}
