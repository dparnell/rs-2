//! File > Print (rs2's addition to OS/2's System Editor, which has no
//! printing): the text on the default printer (PM_SPOOLER QUEUE),
//! through a queued DC (PM_Q_STD) in the printer's default font,
//! Courier 10 points: 12-point lines, margins of three quarters of an
//! inch, long lines wrapped, tabs every 8 columns, form feeds starting
//! pages.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

unsafe extern "C" {
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
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
    #[link_name = "PMSPL.397"]
    fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
}

const OD_QUEUED: i32 = 2;
const DEVESC_STARTDOC: i32 = 8150;
const DEVESC_ENDDOC: i32 = 8151;
const DEVESC_NEWFRAME: i32 = 16300;

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// The default queue and its description.
pub fn default_printer() -> Option<(Vec<u8>, Vec<u8>)> {
    let mut buf = [0u8; 64];
    let n = unsafe { PrfQueryProfileString(0xffff_fffe, b"PM_SPOOLER\0".as_ptr(), b"QUEUE\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), 64) } as usize;
    let q: Vec<u8> = buf[..n.min(64)].iter().copied().take_while(|&c| c != 0 && c != b';').collect();
    if q.is_empty() {
        return None;
    }
    let mut info = alloc::vec![0u8; 2048];
    let mut need = 0;
    if unsafe { SplQueryQueue(null(), z(&q).as_ptr(), 3, info.as_mut_ptr(), info.len() as u32, &mut need) } != 0 {
        return None;
    }
    let p = u32::from_le_bytes([info[24], info[25], info[26], info[27]]) as *const u8;
    let mut descr = Vec::new();
    let mut i = 0;
    while !p.is_null() && unsafe { *p.add(i) } != 0 {
        descr.push(unsafe { *p.add(i) });
        i += 1;
    }
    Some((q, descr))
}

/// Print `text` on `queue` as the document `name`.
pub fn print_text(queue: &[u8], name: &[u8], text: &[u8]) -> bool {
    let q = z(queue);
    let dop = [q.as_ptr() as u32, 0, 0, b"PM_Q_STD\0".as_ptr() as u32];
    let hdc = unsafe { DevOpenDC(0, OD_QUEUED, b"*\0".as_ptr(), 4, dop.as_ptr(), 0) };
    if hdc == 0 {
        return false;
    }
    let mut caps = [0i32; 39];
    unsafe { DevQueryCaps(hdc, 0, 39, caps.as_mut_ptr()) };
    let (w, h, dpi) = (caps[4].max(1), caps[5].max(1), caps[38].max(72));
    let size = [0i32, 0];
    // PU_PELS | GPIA_ASSOC
    let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), 0x0008 | 0x4000) };
    let (margin, lh, cw) = (dpi * 3 / 4, dpi * 12 / 72, dpi * 6 / 72);
    let cols = ((w - 2 * margin) / cw).max(10) as usize;
    let rows = ((h - 2 * margin) / lh).max(5);
    let doc = z(name);
    let mut ok = hps != 0 && unsafe { DevEscape(hdc, DEVESC_STARTDOC, name.len() as i32, doc.as_ptr(), null_mut(), null_mut()) } == 1;
    let mut row = 0;
    let new_page = |row: &mut i32, ok: &mut bool| {
        *ok &= unsafe { DevEscape(hdc, DEVESC_NEWFRAME, 0, null(), null_mut(), null_mut()) } == 1;
        *row = 0;
    };
    let text = text.strip_suffix(b"\x1a").unwrap_or(text);
    for line in text.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        for (k, part) in line.split(|&c| c == 0x0c).enumerate() {
            if k > 0 && row > 0 {
                new_page(&mut row, &mut ok);
            }
            let mut out = Vec::new();
            for &c in part {
                if c == b'\t' {
                    out.extend(core::iter::repeat_n(b' ', 8 - out.len() % 8));
                } else {
                    out.push(c);
                }
            }
            let mut pieces: Vec<&[u8]> = out.chunks(cols).collect();
            if pieces.is_empty() {
                pieces.push(b"");
            }
            for p in pieces {
                if row >= rows {
                    new_page(&mut row, &mut ok);
                }
                let at = [margin, h - margin - (row + 1) * lh + lh / 4];
                if !p.is_empty() {
                    unsafe { GpiCharStringAt(hps, at.as_ptr(), p.len() as i32, p.as_ptr()) };
                }
                row += 1;
            }
        }
    }
    let mut id = [0u8; 2];
    let mut n = 2;
    ok &= unsafe { DevEscape(hdc, DEVESC_ENDDOC, 0, null(), &mut n, id.as_mut_ptr()) } == 1;
    unsafe {
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
    ok
}
