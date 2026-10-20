//! HMTEST.EXE: a PM program for checking the help manager with its help
//! library HMTEST.HLP (userland/hmtest/HMTEST.IPF): `HMTEST [res]` shows
//! panel `res` (default 1) of the library; the application answers
//! HM_QUERY_DDF_DATA with dynamic data made with the Ddf* functions (text
//! styles, colours, fonts, a list, links, a bitmap) and HM_INFORM by
//! counting and invalidating the data. `HMTEST /MB` opens a message box
//! with a Help button (MB_HELP) instead; its help is panel 20.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

os2::entry!(main);

#[repr(C)]
#[derive(Default, Clone, Copy)]
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

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct RectL {
    l: i32,
    b: i32,
    r: i32,
    t: i32,
}

type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.913"]
    fn WinDrawText(hps: u32, n: i32, s: *const u8, r: *mut RectL, fg: i32, bg: i32, fl: u32) -> i32;
    #[link_name = "PMWIN.757"]
    fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.789"]
    fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "PMGPI.598"]
    fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, init: *const u8, bmi: *const u8) -> u32;
    #[link_name = "HELPMGR.51"]
    fn WinCreateHelpInstance(hab: u32, init: *mut u32) -> u32;
    #[link_name = "HELPMGR.52"]
    fn WinDestroyHelpInstance(h: u32) -> u32;
    #[link_name = "HELPMGR.54"]
    fn WinAssociateHelpInstance(help: u32, app: u32) -> u32;
    #[link_name = "HELPMGR.74"]
    fn DdfInitialize(help: u32, cb: u32, inc: u32) -> u32;
    #[link_name = "HELPMGR.75"]
    fn DdfPara(h: u32) -> u32;
    #[link_name = "HELPMGR.76"]
    fn DdfSetFormat(h: u32, f: u32) -> u32;
    #[link_name = "HELPMGR.77"]
    fn DdfSetTextAlign(h: u32, a: u32) -> u32;
    #[link_name = "HELPMGR.78"]
    fn DdfSetColor(h: u32, back: i32, fore: i32) -> u32;
    #[link_name = "HELPMGR.79"]
    fn DdfInform(h: u32, text: *const u8, res: u32) -> u32;
    #[link_name = "HELPMGR.80"]
    fn DdfSetFontStyle(h: u32, st: u32) -> u32;
    #[link_name = "HELPMGR.81"]
    fn DdfHyperText(h: u32, text: *const u8, reference: *const u8, ty: u32) -> u32;
    #[link_name = "HELPMGR.82"]
    fn DdfBeginList(h: u32, width: u32, brk: u32, spacing: u32) -> u32;
    #[link_name = "HELPMGR.83"]
    fn DdfListItem(h: u32, term: *const u8, desc: *const u8) -> u32;
    #[link_name = "HELPMGR.84"]
    fn DdfEndList(h: u32) -> u32;
    #[link_name = "HELPMGR.85"]
    fn DdfText(h: u32, text: *const u8) -> u32;
    #[link_name = "HELPMGR.87"]
    fn DdfSetFont(h: u32, face: *const u8, w: u32, ht: u32) -> u32;
    #[link_name = "HELPMGR.88"]
    fn DdfBitmap(h: u32, hbm: u32, align: u32) -> u32;
}

const WM_PAINT: u32 = 0x23;
const WM_USER: u32 = 0x1000;
const HM_DISPLAY_HELP: u32 = 0x222;
const HM_INFORM: u32 = 0x234;
const HM_QUERY_DDF_DATA: u32 = 0x23a;
const HM_INVALIDATE_DDF_DATA: u32 = 0x23b;
const HM_ERROR: u32 = 0x22e;
const CLR_DEFAULT: i32 = -3;
const CLR_BLUE: i32 = 1;
const CLR_RED: i32 = 2;
const CLR_YELLOW: i32 = 6;
const CLR_UNCHANGED: i32 = -6;
const SYSCLR_WINDOW: i32 = -20;
const SYSCLR_WINDOWTEXT: i32 = -17;

struct Globals {
    help: u32,
    panel: u32,
    informs: u32,
    bitmap: u32,
    error: u32,
}

static mut G: Globals = Globals { help: 0, panel: 1, informs: 0, bitmap: 0, error: 0 };

fn g() -> &'static mut Globals {
    unsafe { &mut *core::ptr::addr_of_mut!(G) }
}

fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}

/// The help table: the frame (id 1) has panel 1 as its extended help;
/// the message box (id 20) panel 20, its Help button too.
static SUBTABLE: [u16; 5] = [2, 20, 20, 0, 0];
#[repr(C, packed)]
struct HelpTableEntry {
    id: u16,
    sub: u32,
    ext: u16,
}

fn help_table() -> Vec<HelpTableEntry> {
    let sub = SUBTABLE.as_ptr() as u32;
    alloc::vec![HelpTableEntry { id: 1, sub, ext: 1 }, HelpTableEntry { id: 20, sub, ext: 20 }, HelpTableEntry { id: 0, sub: 0, ext: 0 }]
}

/// A bitmap for DdfBitmap: 48 by 24 colour bars.
fn make_bitmap(hps: u32) -> u32 {
    let (w, h) = (48usize, 24usize);
    let mut hdr = [0u8; 12];
    hdr[0] = 12;
    hdr[4..6].copy_from_slice(&(w as u16).to_le_bytes());
    hdr[6..8].copy_from_slice(&(h as u16).to_le_bytes());
    hdr[8] = 1;
    hdr[10] = 24;
    let stride = (w * 3).div_ceil(4) * 4;
    let mut bits = alloc::vec![0u8; stride * h];
    for y in 0..h {
        for x in 0..w {
            let (b, gr, r) = match x * 4 / w {
                0 => (0, 0, 255),
                1 => (0, 200, 0),
                2 => (255, 0, 0),
                _ => (220, 220, 0),
            };
            let shade = (y * 255 / h) as u8 / 2;
            let o = y * stride + x * 3;
            bits[o] = (b as u8).saturating_sub(shade);
            bits[o + 1] = (gr as u8).saturating_sub(shade);
            bits[o + 2] = (r as u8).saturating_sub(shade);
        }
    }
    unsafe { GpiCreateBitmap(hps, hdr.as_ptr(), 4, bits.as_ptr(), hdr.as_ptr()) }
}

/// The dynamic data of `:ddf res=`.
fn ddf(client: u32, res: u32) -> u32 {
    let gl = g();
    let d = unsafe { DdfInitialize(gl.help, 0, 0) };
    if d == 0 {
        return 0;
    }
    let t = |s: &[u8]| unsafe { DdfText(d, z(s).as_ptr()) };
    unsafe {
        match res {
            1 => {
                t(b"This text comes from the application, made with the Ddf functions: ");
                DdfSetFontStyle(d, 0x20);
                t(b"bold");
                DdfSetFontStyle(d, 0);
                t(b", ");
                DdfSetFontStyle(d, 1);
                t(b"italic");
                DdfSetFontStyle(d, 0);
                t(b", ");
                DdfSetFontStyle(d, 2);
                t(b"underlined");
                DdfSetFontStyle(d, 0);
                t(b" and ");
                DdfSetColor(d, CLR_YELLOW, CLR_RED);
                t(b"red on yellow");
                DdfSetColor(d, CLR_DEFAULT, CLR_DEFAULT);
                t(b" text, a link to ");
                DdfHyperText(d, z(b"a panel by name").as_ptr(), z(b"target").as_ptr(), 0);
                t(b", one to ");
                DdfHyperText(d, z(b"panel 4 by number").as_ptr(), z(b"4").as_ptr(), 1);
                t(b" and an ");
                DdfInform(d, z(b"inform link").as_ptr(), 77);
                let n = alloc::format!(" (chosen {} times).", gl.informs);
                t(n.as_bytes());
                DdfPara(d);
                DdfSetFont(d, z(b"Times New Roman").as_ptr(), 18, 18);
                t(b"A Times New Roman font, 18 points.");
                DdfSetFont(d, z(b"").as_ptr(), 10, 10);
                DdfPara(d);
                DdfBeginList(d, 12, 1, 1);
                DdfListItem(d, z(b"Term").as_ptr(), z(b"Its description, a definition list from DdfBeginList and DdfListItem.").as_ptr());
                DdfListItem(d, z(b"Second").as_ptr(), z(b"Another item.").as_ptr());
                DdfEndList(d);
                DdfPara(d);
                DdfSetFormat(d, 0);
                t(b"Text as it is:\n    indented line\n");
                DdfSetFormat(d, 1);
                DdfSetTextAlign(d, 3);
                DdfSetFormat(d, 0);
                t(b"centred line\n");
                DdfSetFormat(d, 1);
                DdfPara(d);
                t(b"A bitmap: ");
                if gl.bitmap == 0 {
                    let hps = WinGetPS(client);
                    gl.bitmap = make_bitmap(hps);
                    WinReleasePS(hps);
                }
                if gl.bitmap != 0 {
                    DdfBitmap(d, gl.bitmap, 0x10);
                }
                t(b" (in the text).");
                DdfSetColor(d, CLR_UNCHANGED, CLR_BLUE);
                DdfPara(d);
                t(b"End of the dynamic data.");
            }
            _ => {
                let n = alloc::format!("Dynamic data {res}.");
                t(n.as_bytes());
            }
        }
    }
    d
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let gl = g();
    match msg {
        WM_USER => {
            unsafe { WinSendMsg(gl.help, HM_DISPLAY_HELP, gl.panel, 0) };
            0
        }
        HM_QUERY_DDF_DATA => ddf(h, mp2),
        HM_INFORM => {
            gl.informs += 1;
            if mp1 == 77 {
                let ids: [u16; 1] = [1];
                unsafe { WinSendMsg(gl.help, HM_INVALIDATE_DDF_DATA, 1, ids.as_ptr() as u32) };
            }
            0
        }
        HM_ERROR => {
            gl.error = mp1;
            0
        }
        WM_PAINT => {
            let hps = unsafe { WinBeginPaint(h, 0, core::ptr::null_mut()) };
            let mut r = RectL::default();
            unsafe {
                WinQueryWindowRect(h, &mut r);
                WinFillRect(hps, &r, SYSCLR_WINDOW);
                let s = alloc::format!("Help manager test. Last HM_ERROR: {:#x}", gl.error);
                WinDrawText(hps, s.len() as i32, s.as_ptr(), &mut r, SYSCLR_WINDOWTEXT, 0, 0x0100 | 0x0400);
                WinEndPaint(hps);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn main(s: &os2::Startup) -> u32 {
    let args = s.args();
    let mb = args.windows(3).any(|w| w.eq_ignore_ascii_case(b"/MB"));
    if let Some(n) = args.split(|&c| c == b' ').find(|w| !w.is_empty() && w.iter().all(|c| c.is_ascii_digit())) {
        g().panel = n.iter().fold(0u32, |a, &c| a * 10 + (c - b'0') as u32);
    }
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    unsafe { WinRegisterClass(hab, b"HMTest\0".as_ptr(), client_proc, 0, 0) };
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX |
    // FCF_TASKLIST.
    let mut fcf = 0x0001 | 0x0002 | 0x0008 | 0x0030 | 0x0800;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(1, 0, &mut fcf, b"HMTest\0".as_ptr(), b"Help test\0".as_ptr(), 0, 0, 1, &mut client) };
    unsafe { WinSetWindowPos(frame, 3, 16, 16, 300, 120, 0x0001 | 0x0002 | 0x0008 | 0x0080) };
    let table = help_table();
    // HELPINIT: cb, ulReturnCode, pszTutorialName, phtHelpTable,
    // hmodHelpTableModule, hmodAccelActionBarModule, idAccelTable,
    // idActionBar, pszHelpWindowTitle, fShowPanelId, pszHelpLibraryName.
    let mut init: [u32; 11] = [44, 0, 0, table.as_ptr() as u32, 0, 0, 0, 0, b"Help test\0".as_ptr() as u32, 0, b"HMTEST.HLP\0".as_ptr() as u32];
    let help = unsafe { WinCreateHelpInstance(hab, init.as_mut_ptr()) };
    g().help = help;
    if help != 0 {
        unsafe { WinAssociateHelpInstance(help, frame) };
    }
    if mb {
        // MB_OK | MB_HELP | MB_INFORMATION | MB_MOVEABLE, id 20.
        unsafe {
            WinMessageBox(1, frame, b"A message box with a Help button.\nPress Help or F1.\0".as_ptr(), b"Help test\0".as_ptr(), 20, 0x2000 | 0x0030 | 0x4000)
        };
    } else {
        unsafe { WinPostMsg(client, WM_USER, 0, 0) };
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    unsafe {
        if help != 0 {
            WinDestroyHelpInstance(help);
        }
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
