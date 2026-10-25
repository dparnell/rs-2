//! Services' Print (IBM's help manager prints sections of the help on
//! the default printer): a dialog asks for this section, all sections or
//! the contents, then the text goes to the default queue (PM_SPOOLER
//! QUEUE) through a PM_Q_STD queued DC: each section's title in
//! Helvetica Bold 12 points, its text in Helvetica 10 points, word
//! wrapped to the page with margins of three quarters of an inch.

use crate::pm::*;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};

unsafe extern "C" {
    #[link_name = "PMWIN.922"]
    fn WinCreateDlg(parent: u32, owner: u32, proc_: WndProc, pdlgt: *const u8, params: u32) -> u32;
    #[link_name = "PMWIN.796"]
    fn WinProcessDlg(h: u32) -> u32;
    #[link_name = "PMWIN.729"]
    fn WinDismissDlg(h: u32, r: u32) -> u32;
    #[link_name = "PMWIN.910"]
    fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.605"]
    fn DevEscape(hdc: u32, code: i32, nin: i32, pin: *const u8, nout: *mut i32, pout: *mut u8) -> i32;
    #[link_name = "PMGPI.606"]
    fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    fn GpiAssociate(hps: u32, hdc: u32) -> u32;
    #[link_name = "PMGPI.359"]
    fn GpiCharStringAt(hps: u32, p: *const i32, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.368"]
    fn GpiCreateLogFont(hps: u32, name: *const u8, lcid: i32, fat: *const u8) -> i32;
    #[link_name = "PMGPI.513"]
    fn GpiSetCharSet(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.510"]
    fn GpiSetCharBox(hps: u32, p: *const i32) -> u32;
    #[link_name = "PMGPI.489"]
    fn GpiQueryTextBox(hps: u32, n: i32, s: *const u8, npts: i32, pts: *mut i32) -> u32;
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
    #[link_name = "PMSPL.397"]
    fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
}

/// The default queue and its description (None: no printer).
pub fn default_printer() -> Option<(Vec<u8>, Vec<u8>)> {
    let mut buf = [0u8; 64];
    let n = unsafe { PrfQueryProfileString(0xffff_fffe, b"PM_SPOOLER\0".as_ptr(), b"QUEUE\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), 64) } as usize;
    let q: Vec<u8> = buf[..n.min(64)].iter().copied().take_while(|&c| c != 0 && c != b';').collect();
    if q.is_empty() {
        return None;
    }
    let qz = z(&q);
    let mut info = vec![0u8; 2048];
    let mut need = 0;
    if unsafe { SplQueryQueue(null(), qz.as_ptr(), 3, info.as_mut_ptr(), info.len() as u32, &mut need) } != 0 {
        return None;
    }
    let p = u32::from_le_bytes([info[24], info[25], info[26], info[27]]) as *const u8;
    let mut descr = Vec::new();
    if !p.is_null() {
        let mut i = 0;
        while unsafe { *p.add(i) } != 0 {
            descr.push(unsafe { *p.add(i) });
            i += 1;
        }
    }
    Some((q, descr))
}

// ---- The dialog ----

pub const WHAT_THIS: u32 = 101;
pub const WHAT_ALL: u32 = 102;
pub const WHAT_CONTENTS: u32 = 103;
const ID_PRINTER: u32 = 110;
const WM_INITDLG: u32 = 0x003b;
const BM_SETCHECK: u32 = 0x0125;
const ID_OK: u32 = 1;
const DID_CANCEL: u32 = 2;

struct Dlg {
    has_section: bool,
    printer: Vec<u8>,
    choice: u32,
}

fn dlg(h: u32) -> Option<&'static mut Dlg> {
    unsafe { (WinQueryWindowULong(h, QWL_USER) as *mut Dlg).as_mut() }
}

extern "C" fn print_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    crate::stack::deep(|| print_proc_(h, msg, mp1, mp2))
}

fn print_proc_(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_INITDLG {
        unsafe { WinSetWindowULong(h, QWL_USER, mp2) };
        let Some(d) = dlg(h) else { return 0 };
        if !d.has_section {
            unsafe { WinEnableWindow(WinWindowFromID(h, WHAT_THIS), 0) };
        }
        let first = if d.has_section { WHAT_THIS } else { WHAT_ALL };
        d.choice = first;
        send(unsafe { WinWindowFromID(h, first) }, BM_SETCHECK, 1, 0);
        let mut t = b"Printer: ".to_vec();
        t.extend_from_slice(&d.printer);
        unsafe { WinSetWindowText(WinWindowFromID(h, ID_PRINTER), z(&t).as_ptr()) };
        return 0;
    }
    let Some(d) = dlg(h) else { return unsafe { WinDefDlgProc(h, msg, mp1, mp2) } };
    match msg {
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            if code == BN_CLICKED && (WHAT_THIS..=WHAT_CONTENTS).contains(&id) {
                d.choice = id;
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                ID_OK => unsafe {
                    WinDismissDlg(h, d.choice);
                },
                DID_CANCEL => unsafe {
                    WinDismissDlg(h, 0);
                },
                _ => {}
            }
            0
        }
        WM_HELP => 0,
        _ => unsafe { WinDefDlgProc(h, msg, mp1, mp2) },
    }
}

/// Ask what to print: WHAT_* or 0 (cancelled).
pub fn ask(owner: u32, has_section: bool, printer: &[u8]) -> u32 {
    const RADIO: u32 = 0x8000_0004;
    use crate::services::{dialog, Ctl};
    let t = dialog(
        b"Print",
        Ctl(1, b"", 0x9400_0090, [41, 21, 220, 100], 20003),
        FCF_TITLEBAR | FCF_SYSMENU,
        &[
            Ctl(5, b"", 0x8000_0001, [10, 80, 200, 8], ID_PRINTER as u16),
            Ctl(3, b"~This section", RADIO | WS_GROUP | WS_TABSTOP, [17, 62, 120, 10], WHAT_THIS as u16),
            Ctl(3, b"~All sections", RADIO, [17, 49, 120, 10], WHAT_ALL as u16),
            Ctl(3, b"~Contents", RADIO, [17, 36, 120, 10], WHAT_CONTENTS as u16),
            Ctl(3, b"~Print", 0x8003_0400, [15, 6, 55, 12], ID_OK as u16),
            Ctl(3, b"Cancel", 0x8000_0000, [85, 6, 55, 12], DID_CANCEL as u16),
        ],
    );
    let mut d = Dlg { has_section, printer: printer.to_vec(), choice: 0 };
    let h = unsafe { WinCreateDlg(HWND_DESKTOP, owner, print_proc, t.as_ptr(), &mut d as *mut Dlg as u32) };
    if h == 0 {
        return 0;
    }
    let r = unsafe { WinProcessDlg(h) };
    unsafe { WinDestroyWindow(h) };
    r
}

// ---- Printing ----

const OD_QUEUED: i32 = 2;
const DEVESC_STARTDOC: i32 = 8150;
const DEVESC_ENDDOC: i32 = 8151;
const DEVESC_NEWFRAME: i32 = 16300;

struct Page {
    hdc: u32,
    hps: u32,
    w: i32,
    h: i32,
    margin: i32,
    y: i32,
    ok: bool,
}

impl Page {
    fn font(&self, lcid: i32, face: &[u8], points: i32, dpi: i32) {
        let mut fat = [0u8; 56];
        fat[0] = 56;
        fat[8..8 + face.len()].copy_from_slice(face);
        fat[54] = 4; // FATTR_FONTUSE_OUTLINE
        unsafe { GpiCreateLogFont(self.hps, null(), lcid, fat.as_ptr()) };
        let _ = (points, dpi);
    }

    fn select(&self, lcid: i32, px: i32) {
        unsafe {
            GpiSetCharSet(self.hps, lcid);
            GpiSetCharBox(self.hps, [px << 16, px << 16].as_ptr());
        }
    }

    fn width(&self, t: &[u8]) -> i32 {
        let mut pts = [0i32; 10];
        unsafe { GpiQueryTextBox(self.hps, t.len() as i32, t.as_ptr(), 5, pts.as_mut_ptr()) };
        pts[8]
    }

    fn new_page(&mut self) {
        self.ok &= unsafe { DevEscape(self.hdc, DEVESC_NEWFRAME, 0, null(), null_mut(), null_mut()) } == 1;
        self.y = self.h - self.margin;
    }

    /// One line at the current position (a new page when it is full).
    fn line(&mut self, t: &[u8], lh: i32) {
        if self.y - lh < self.margin {
            self.new_page();
        }
        self.y -= lh;
        if !t.is_empty() {
            let at = [self.margin, self.y + lh / 4];
            unsafe { GpiCharStringAt(self.hps, at.as_ptr(), t.len() as i32, t.as_ptr()) };
        }
    }

    /// A paragraph, word wrapped.
    fn wrap(&mut self, text: &[u8], lh: i32) {
        let room = self.w - 2 * self.margin;
        for para in text.split(|&c| c == b'\n') {
            let para = para.strip_suffix(b"\r").unwrap_or(para);
            if para.iter().all(|c| *c == b' ') {
                self.line(b"", lh);
                continue;
            }
            let mut line: Vec<u8> = Vec::new();
            for word in para.split(|&c| c == b' ').filter(|w| !w.is_empty()) {
                let mut cand = line.clone();
                if !cand.is_empty() {
                    cand.push(b' ');
                }
                cand.extend_from_slice(word);
                if !line.is_empty() && self.width(&cand) > room {
                    self.line(&line, lh);
                    line = word.to_vec();
                } else {
                    line = cand;
                }
            }
            self.line(&line, lh);
        }
    }
}

/// Print sections (title, text) on a queue as a document named `name`.
pub fn print(queue: &[u8], name: &[u8], sections: &[(Vec<u8>, Vec<u8>)]) -> bool {
    let q = z(queue);
    let dop = [q.as_ptr() as u32, 0, 0, b"PM_Q_STD\0".as_ptr() as u32];
    let hdc = unsafe { DevOpenDC(0, OD_QUEUED, b"*\0".as_ptr(), 4, dop.as_ptr(), 0) };
    if hdc == 0 {
        return false;
    }
    let mut caps = [0i32; 39];
    unsafe { DevQueryCaps(hdc, 0, 39, caps.as_mut_ptr()) };
    let dpi = caps[38].max(72);
    let size = [0i32, 0];
    // PU_PELS | GPIA_ASSOC
    let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), 0x0008 | 0x4000) };
    let mut p = Page { hdc, hps, w: caps[4].max(1), h: caps[5].max(1), margin: dpi * 3 / 4, y: 0, ok: hps != 0 };
    p.y = p.h - p.margin;
    let doc = z(name);
    p.ok &= unsafe { DevEscape(hdc, DEVESC_STARTDOC, name.len() as i32, doc.as_ptr(), null_mut(), null_mut()) } == 1;
    p.font(1, b"Helvetica Bold", 12, dpi);
    p.font(2, b"Helvetica", 10, dpi);
    let (head, body) = (dpi * 12 / 72, dpi * 10 / 72);
    for (k, (title, text)) in sections.iter().enumerate() {
        if k > 0 {
            p.line(b"", body);
        }
        p.select(1, head);
        p.wrap(title, head * 13 / 10);
        p.line(b"", body / 2);
        p.select(2, body);
        p.wrap(text, body * 12 / 10);
    }
    let mut id = [0u8; 2];
    let mut n = 2;
    p.ok &= unsafe { DevEscape(hdc, DEVESC_ENDDOC, 0, null(), &mut n, id.as_mut_ptr()) } == 1;
    unsafe {
        GpiSetCharSet(hps, 0);
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
    p.ok
}
