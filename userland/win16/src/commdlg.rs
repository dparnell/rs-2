//! COMMDLG: a facade over W32USER's COMDLG32 (docs/WIN16.md, "One window
//! manager"). Open, Save As, Color, Font, Find, Replace, Print and Print
//! Setup are COMDLG32's dialogs; the entries here copy the program's
//! Win16 structures (OPENFILENAME, CHOOSECOLOR, CHOOSEFONT, FINDREPLACE,
//! PRINTDLG) to Win32's and back. The program's hooks get its own
//! structure (16:16) with WM_INITDIALOG and the common dialogs'
//! registered messages (msg16.rs asks `map_lp`); its templates come from
//! its resources (W32USER asks WIN16 for them) or, with a handle, are
//! converted here.

use alloc::boxed::Box;
use alloc::vec::Vec;
use thunk16::{flat, Pascal};

use crate::msg16::proc16to32;
use crate::user::{hinst32, str32};
use crate::w32::*;
use crate::Global;

fn rd16(p: *const u8, o: usize) -> u16 {
    unsafe { (p.add(o) as *const u16).read_unaligned() }
}
fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}
fn wr16(p: *mut u8, o: usize, v: u16) {
    unsafe { (p.add(o) as *mut u16).write_unaligned(v) }
}
fn wr32(p: *mut u8, o: usize, v: u32) {
    unsafe { (p.add(o) as *mut u32).write_unaligned(v) }
}

/// The Win32 structures of the dialogs open now and the program's own:
/// (Win32 address, 16:16 address, kind).
static OPEN: Global<Vec<(u32, u32, Kind)>> = Global::new(Vec::new());

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    File,
    Color,
    Font,
    Find,
    Print,
}

/// The program's structure for a Win32 one a hook or the owner is given
/// (WM_INITDIALOG, FINDMSGSTRING...), brought up to date first.
pub fn map_lp(lp: u32) -> Option<u32> {
    let (p32, far_, kind) = OPEN.get().iter().find(|e| e.0 == lp).copied()?;
    let (s, q) = (p32 as *const u8, flat(far_));
    match kind {
        // FINDREPLACE's flags say what the owner is to do.
        Kind::Find => wr32(q, 8, rd32(s, 12)),
        Kind::Font => {
            wr16(q, 12, rd32(s, 16) as u16);
            wr32(q, 14, rd32(s, 20));
            wr32(q, 18, rd32(s, 24));
        }
        Kind::Color => wr32(q, 8, rd32(s, 12)),
        Kind::File => wr32(q, 48, rd32(s, 52)),
        Kind::Print => wr32(q, 12, rd32(s, 20)),
    }
    Some(far_)
}

fn opened(p32: u32, far_: u32, kind: Kind) {
    OPEN.get().push((p32, far_, kind));
}

fn closed(p32: u32) {
    OPEN.get().retain(|e| e.0 != p32);
}

/// A template in a global block (the *_ENABLETEMPLATEHANDLE flags) as
/// Win32's: kept while the dialog runs.
fn template_handle(h: u16) -> Vec<u8> {
    let far_ = ((h | 7) as u32) << 16;
    let n = crate::kernel::global_size(h).clamp(16, 0xffff) as usize;
    crate::tmpl16::dialog(unsafe { core::slice::from_raw_parts(flat(far_), n) })
}

/// The Win32 structure's size for the program's: 0 (COMDLG32's
/// CDERR_STRUCTSIZE) when the program's is too small.
fn size32(p: *const u8, size16: u32, size32: u32) -> u32 {
    if rd32(p, 0) < size16 { 0 } else { size32 }
}

// ---- Open and Save As ----

/// OPENFILENAME16 (72 bytes) as OPENFILENAMEA (76).
fn file_dlg(st: u32, save: bool) -> u32 {
    let p = flat(st);
    if p.is_null() {
        return 0;
    }
    let fl = rd32(p, 48);
    let tpl = if fl & 0x40 != 0 { template_handle(rd16(p, 6)) } else { Vec::new() };
    let mut o = [0u32; 19];
    o[0] = size32(p, 72, 76);
    o[1] = rd16(p, 4) as u32;
    o[2] = if fl & 0x40 != 0 { tpl.as_ptr() as u32 } else { hinst32(rd16(p, 6)) };
    o[3] = str32(rd32(p, 8));
    o[4] = str32(rd32(p, 12));
    o[5] = rd32(p, 16);
    o[6] = rd32(p, 20);
    o[7] = str32(rd32(p, 24));
    o[8] = rd32(p, 28);
    o[9] = str32(rd32(p, 32));
    o[10] = rd32(p, 36);
    o[11] = str32(rd32(p, 40));
    o[12] = str32(rd32(p, 44));
    o[13] = fl;
    o[14] = rd16(p, 52) as u32 | (rd16(p, 54) as u32) << 16;
    o[15] = str32(rd32(p, 56));
    o[16] = rd32(p, 60);
    o[17] = proc16to32(rd32(p, 64));
    o[18] = str32(rd32(p, 68));
    let op = o.as_ptr() as u32;
    opened(op, st, Kind::File);
    let r = unsafe { if save { GetSaveFileNameA(op) } else { GetOpenFileNameA(op) } };
    closed(op);
    wr32(p, 20, o[6]);
    wr32(p, 48, o[13]);
    wr16(p, 52, o[14] as u16);
    wr16(p, 54, (o[14] >> 16) as u16);
    r
}

// ---- Color ----

/// CHOOSECOLOR16 (32 bytes) as CHOOSECOLORA (36).
fn color_dlg(st: u32) -> u32 {
    let p = flat(st);
    if p.is_null() {
        return 0;
    }
    let fl = rd32(p, 16);
    let tpl = if fl & 0x20 != 0 { template_handle(rd16(p, 6)) } else { Vec::new() };
    let c = [size32(p, 32, 36), rd16(p, 4) as u32, if fl & 0x20 != 0 { tpl.as_ptr() as u32 } else { hinst32(rd16(p, 6)) }, rd32(p, 8), str32(rd32(p, 12)), fl, rd32(p, 20), proc16to32(rd32(p, 24)), str32(rd32(p, 28))];
    let cp = c.as_ptr() as u32;
    opened(cp, st, Kind::Color);
    let r = unsafe { ChooseColorA(cp) };
    closed(cp);
    wr32(p, 8, c[3]);
    wr32(p, 16, c[5]);
    r
}

// ---- Font ----

/// CHOOSEFONT16 (46 bytes) as CHOOSEFONTA (60), its LOGFONT copied both
/// ways.
fn font_dlg(st: u32) -> u32 {
    let p = flat(st);
    if p.is_null() {
        return 0;
    }
    let fl = rd32(p, 14);
    let lf16 = flat(rd32(p, 8));
    let mut lf = if lf16.is_null() { [0u8; 60] } else { crate::gdi::logfont32(lf16) };
    let tpl = if fl & 0x20 != 0 { template_handle(rd16(p, 34)) } else { Vec::new() };
    let mut c = [0u32; 15];
    c[0] = size32(p, 46, 60);
    c[1] = rd16(p, 4) as u32;
    c[2] = rd16(p, 6) as u32;
    c[3] = if lf16.is_null() { 0 } else { lf.as_mut_ptr() as u32 };
    c[4] = rd16(p, 12) as i16 as i32 as u32;
    c[5] = fl;
    c[6] = rd32(p, 18);
    c[7] = rd32(p, 22);
    c[8] = proc16to32(rd32(p, 26));
    c[9] = str32(rd32(p, 30));
    c[10] = if fl & 0x20 != 0 { tpl.as_ptr() as u32 } else { hinst32(rd16(p, 34)) };
    c[11] = str32(rd32(p, 36));
    c[12] = rd16(p, 40) as u32;
    c[13] = rd16(p, 42) as i16 as i32 as u32;
    c[14] = rd16(p, 44) as i16 as i32 as u32;
    let cp = c.as_ptr() as u32;
    opened(cp, st, Kind::Font);
    let r = unsafe { ChooseFontA(cp) };
    closed(cp);
    if !lf16.is_null() {
        crate::gdi::logfont16(lf.as_ptr(), lf16);
    }
    wr16(p, 12, c[4] as u16);
    wr32(p, 14, c[5]);
    wr32(p, 18, c[6]);
    wr16(p, 40, c[12] as u16);
    r
}

// ---- Find and Replace (modeless) ----

/// FINDREPLACE16 (36 bytes) as FINDREPLACEA (40), kept for the dialog's
/// life (WM_NCDESTROY of a dialog: `forget_find`).
fn find_dlg(st: u32, replace: bool) -> u32 {
    let p = flat(st);
    if p.is_null() {
        return 0;
    }
    let fl = rd32(p, 8);
    let tpl = Box::leak(Box::new(if fl & 0x20 != 0 { template_handle(rd16(p, 6)) } else { Vec::new() }));
    let f = Box::leak(Box::new([
        size32(p, 36, 40),
        rd16(p, 4) as u32,
        if fl & 0x20 != 0 { tpl.as_ptr() as u32 } else { hinst32(rd16(p, 6)) },
        fl,
        str32(rd32(p, 12)),
        str32(rd32(p, 16)),
        rd16(p, 20) as u32 | (rd16(p, 22) as u32) << 16,
        rd32(p, 24),
        proc16to32(rd32(p, 28)),
        str32(rd32(p, 32)),
    ]));
    let fp = f.as_ptr() as u32;
    opened(fp, st, Kind::Find);
    unsafe { (if replace { ReplaceTextA(fp) } else { FindTextA(fp) }) & 0xffff }
}

// ---- Print and Print Setup ----

/// PRINTDLG16 (52 bytes) as PRINTDLGA (66, packed); hDevMode and
/// hDevNames are the task's global blocks (W32USER's host memory).
fn print_dlg(st: u32) -> u32 {
    let p = flat(st);
    if p.is_null() {
        return 0;
    }
    let fl = rd32(p, 12);
    let ptpl = if fl & 0x10000 != 0 { template_handle(rd16(p, 48)) } else { Vec::new() };
    let stpl = if fl & 0x20000 != 0 { template_handle(rd16(p, 50)) } else { Vec::new() };
    let mut d = [0u8; 66];
    let q = d.as_mut_ptr();
    wr32(q, 0, size32(p, 52, 66));
    wr32(q, 4, rd16(p, 4) as u32);
    wr32(q, 8, rd16(p, 6) as u32);
    wr32(q, 12, rd16(p, 8) as u32);
    wr32(q, 20, fl);
    for i in 0..5 {
        wr16(q, 24 + 2 * i, rd16(p, 16 + 2 * i));
    }
    wr32(q, 34, hinst32(rd16(p, 26)));
    wr32(q, 38, rd32(p, 28));
    wr32(q, 42, proc16to32(rd32(p, 32)));
    wr32(q, 46, proc16to32(rd32(p, 36)));
    wr32(q, 50, str32(rd32(p, 40)));
    wr32(q, 54, str32(rd32(p, 44)));
    wr32(q, 58, if ptpl.is_empty() { 0 } else { ptpl.as_ptr() as u32 });
    wr32(q, 62, if stpl.is_empty() { 0 } else { stpl.as_ptr() as u32 });
    let dp = q as u32;
    opened(dp, st, Kind::Print);
    let r = unsafe { PrintDlgA(dp) };
    closed(dp);
    if r != 0 {
        let hdm = rd32(q, 8) as u16;
        wr16(p, 6, hdm);
        wr16(p, 8, rd32(q, 12) as u16);
        wr16(p, 10, rd32(q, 16) as u16);
        // Windows 3.1's DEVMODE (68 bytes, version 3.10).
        if hdm != 0 {
            let dm = flat(((hdm | 7) as u32) << 16);
            wr16(dm, 32, 0x030a);
            wr16(dm, 36, 68);
        }
    }
    wr32(p, 12, rd32(q, 20));
    for i in 0..5 {
        wr16(p, 16 + 2 * i, rd16(q, 24 + 2 * i));
    }
    r
}

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    match ord {
        // GetOpenFileName(lpofn), GetSaveFileName(lpofn)
        1 | 2 => file_dlg(a.l(), ord == 2),
        5 => color_dlg(a.l()),
        // FindText(lpfr), ReplaceText(lpfr)
        11 | 12 => find_dlg(a.l(), ord == 12),
        15 => font_dlg(a.l()),
        20 => print_dlg(a.l()),
        26 => unsafe { CommDlgExtendedError() },
        // GetFileTitle(path, buf, cb): 0, -1, or the size needed
        27 => {
            let (path, buf, cb) = (a.l(), a.l(), a.w());
            unsafe { GetFileTitleA(str32(path), str32(buf), cb as u32) & 0xffff }
        }
        _ => 0,
    }
}
