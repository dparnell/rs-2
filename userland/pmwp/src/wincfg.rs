//! WINCFG.DLL's WIN-OS/2 Setup object (WPWinConfig, `<WP_WINCFG>`),
//! rs2's own implementation after IBM's (studied with Ghidra:
//! win_wpAddSettingsPages, WinDDEDlgProc, WinSessionDlgProc,
//! ProgCatFromSessionButtons). Its settings are where IBM's keeps them,
//! OS2.INI's `WINOS2` application:
//!
//! - the Data Exchange page: whether WIN-OS/2's DDE and clipboard are
//!   shared with OS/2 programs (public) or kept to WIN-OS/2 (private),
//!   `PM_DDESupport` and `PM_ClipboardSupport` (1 public, the default);
//! - the WIN-OS/2 Session page: how WIN-OS/2 programs run when they do
//!   not say (`PM_Global31SessionType`: PROG_31_ENH full screen,
//!   PROG_31_ENHSEAMLESSCOMMON in a window sharing one WIN-OS/2 session,
//!   the default, PROG_31_ENHSEAMLESSVDM in a window of a session of its
//!   own), whether WIN-OS/2 is loaded once at startup to start programs
//!   faster (`PM_Global31FastLoad`), and the DOS settings every WIN-OS/2
//!   session gets (`PM_GlobalWindows31Settings`, NAME=value strings
//!   separated by NULs, edited with the program objects' DOS Properties
//!   dialog), which rs2's launcher gives Win-OS/2 sessions under their
//!   programs' own.
//!
//! The pages show only where WIN-OS/2 is installed (OS2.INI `PM_INSTALL`
//! `WINOS2_LOCATION`, as APPINST writes it), and so does the object.
//! WIN-OS/2's national settings (WIN.INI `[intl]`) follow the Country
//! object's (`write_intl`).

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio};
use crate::som;
use crate::syssetup::{checkbox, group, push, text, ID_DEFAULT, ID_UNDO};
use crate::util::{parse_dec, prf_read, prf_write, HINI_USER};
use crate::wps::{self, Impl};

const APP: &[u8] = b"WINOS2";
const PROG_31_ENHSEAMLESSVDM: u32 = 17;
const PROG_31_ENHSEAMLESSCOMMON: u32 = 18;
const PROG_31_ENH: u32 = 19;

/// WIN-OS/2's directory (OS2.INI `PM_INSTALL` `WINOS2_LOCATION`).
pub fn location() -> Option<Vec<u8>> {
    let v = prf_read(HINI_USER, b"PM_INSTALL", b"WINOS2_LOCATION")?;
    let v = v.strip_suffix(&[0]).unwrap_or(&v).to_vec();
    (!v.is_empty()).then_some(v)
}

pub fn installed() -> bool {
    location().is_some()
}

fn read_int(key: &[u8], def: u32) -> u32 {
    prf_read(HINI_USER, APP, key).and_then(|v| parse_dec(v.strip_suffix(&[0]).unwrap_or(&v).trim_ascii())).unwrap_or(def)
}

fn write_int(key: &[u8], v: u32) {
    let s = alloc::format!("{v}\0");
    prf_write(HINI_USER, APP, key, Some(s.as_bytes()));
}

/// The session type WIN-OS/2 programs get when theirs is the default.
pub fn session_type() -> u32 {
    match read_int(b"PM_Global31SessionType", PROG_31_ENHSEAMLESSCOMMON) {
        t @ (PROG_31_ENHSEAMLESSVDM | PROG_31_ENHSEAMLESSCOMMON | PROG_31_ENH) => t,
        _ => PROG_31_ENHSEAMLESSCOMMON,
    }
}

/// The DOS settings of every WIN-OS/2 session ("NAME=value").
pub fn global_settings() -> Vec<Vec<u8>> {
    prf_read(HINI_USER, APP, b"PM_GlobalWindows31Settings")
        .map(|v| v.split(|&c| c == 0).filter(|s| !s.is_empty()).map(|s| s.to_vec()).collect())
        .unwrap_or_default()
}

fn set_global_settings(list: &[Vec<u8>]) {
    if list.is_empty() {
        prf_write(HINI_USER, APP, b"PM_GlobalWindows31Settings", None);
        return;
    }
    let mut v = Vec::new();
    for s in list {
        v.extend_from_slice(s);
        v.push(0);
    }
    v.push(0);
    prf_write(HINI_USER, APP, b"PM_GlobalWindows31Settings", Some(&v));
}

/// A Win-OS/2 session's environment: its program's strings, then the
/// global DOS settings it does not set itself.
pub fn session_env(progc: u32, env: &[u8]) -> Vec<u8> {
    if !(13..=20).contains(&progc) {
        return env.to_vec();
    }
    let name = |s: &[u8]| s.split(|&c| c == b'=').next().unwrap_or(b"").to_ascii_uppercase();
    let own: Vec<Vec<u8>> = env.split(|&c| c == 0).filter(|s| !s.is_empty()).map(|s| name(s)).collect();
    let mut out: Vec<u8> = env.to_vec();
    while out.last() == Some(&0) {
        out.pop();
    }
    if !out.is_empty() {
        out.push(0);
    }
    for g in global_settings() {
        if !own.contains(&name(&g)) {
            out.extend_from_slice(&g);
            out.push(0);
        }
    }
    out
}

// ---- WIN.INI [intl] ----

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut act) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS; OPEN_SHARE_DENYWRITE | read only.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, 0x01, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 2048];
    loop {
        let mut n = 0u32;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(out)
}

fn write_file(path: &[u8], data: &[u8]) -> bool {
    let p = z(path);
    let (mut hf, mut act) = (0u32, 0u32);
    // OPEN_ACTION_REPLACE_IF_EXISTS | CREATE_IF_NEW; deny write, write only.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut n = 0u32;
    let ok = unsafe { os2::DosWrite(hf, data.as_ptr(), data.len() as u32, &mut n) } == 0;
    unsafe { os2::DosClose(hf) };
    ok
}

/// `text` (an INI file) with `key`s in `[section]` set (the section made
/// at the end when missing); other keys and sections as they were.
pub fn set_ini_keys(text: &[u8], section: &[u8], keys: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut done: Vec<bool> = alloc::vec![false; keys.len()];
    let mut seen = false;
    let flush = |out: &mut Vec<u8>, done: &mut Vec<bool>| {
        for (i, (k, v)) in keys.iter().enumerate() {
            if !done[i] {
                out.extend_from_slice(k);
                out.push(b'=');
                out.extend_from_slice(v);
                out.extend_from_slice(b"\r\n");
                done[i] = true;
            }
        }
    };
    let lines: Vec<&[u8]> = text.split(|&c| c == b'\n').collect();
    let n = lines.len();
    for (li, l) in lines.into_iter().enumerate() {
        let l = l.strip_suffix(b"\r").unwrap_or(l);
        if li + 1 == n && l.is_empty() {
            break;
        }
        let t = l.trim_ascii();
        if t.starts_with(b"[") {
            if inside {
                flush(&mut out, &mut done);
            }
            inside = t.len() == section.len() + 2 && t[1..t.len() - 1].eq_ignore_ascii_case(section);
            seen |= inside;
        } else if inside {
            if let Some(eq) = t.iter().position(|&c| c == b'=') {
                let name = t[..eq].trim_ascii();
                if let Some(i) = keys.iter().position(|(k, _)| k.eq_ignore_ascii_case(name)) {
                    if !done[i] {
                        out.extend_from_slice(keys[i].0);
                        out.push(b'=');
                        out.extend_from_slice(&keys[i].1);
                        out.extend_from_slice(b"\r\n");
                        done[i] = true;
                    }
                    continue;
                }
            }
        }
        out.extend_from_slice(l);
        out.extend_from_slice(b"\r\n");
    }
    if inside {
        flush(&mut out, &mut done);
    }
    if !seen {
        out.push(b'[');
        out.extend_from_slice(section);
        out.extend_from_slice(b"]\r\n");
        flush(&mut out, &mut done);
    }
    out
}

/// WIN.INI's `[intl]` from `PM_National`'s keys (where WIN-OS/2 is
/// installed and has a WIN.INI).
pub fn write_intl(national: &[(&'static [u8], Vec<u8>)]) {
    let Some(mut path) = location() else { return };
    if path.last() != Some(&b'\\') {
        path.push(b'\\');
    }
    path.extend_from_slice(b"WIN.INI");
    let Some(old) = read_file(&path) else { return };
    let new = set_ini_keys(&old, b"intl", national);
    if new != old {
        write_file(&path, &new);
    }
}

// ---- The Data Exchange page ----

const ID_DDE_PRIVATE: u16 = 0x67;
const ID_DDE_PUBLIC: u16 = 0x68;
const ID_CLIP_PRIVATE: u16 = 0x70;
const ID_CLIP_PUBLIC: u16 = 0x71;

static mut DDE_UNDO: (u32, u32) = (1, 1);

fn dde_fill(h: u32) {
    let (dde, clip) = (read_int(b"PM_DDESupport", 1), read_int(b"PM_ClipboardSupport", 1));
    check(h, if dde != 0 { ID_DDE_PUBLIC } else { ID_DDE_PRIVATE }, true);
    check(h, if clip != 0 { ID_CLIP_PUBLIC } else { ID_CLIP_PRIVATE }, true);
}

extern "C" fn dde_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { DDE_UNDO = (read_int(b"PM_DDESupport", 1), read_int(b"PM_ClipboardSupport", 1)) };
            dde_fill(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_DDE_PRIVATE | ID_DDE_PUBLIC => write_int(b"PM_DDESupport", checked(h, ID_DDE_PUBLIC) as u32),
                ID_CLIP_PRIVATE | ID_CLIP_PUBLIC => write_int(b"PM_ClipboardSupport", checked(h, ID_CLIP_PUBLIC) as u32),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let (dde, clip) = match (a & 0xffff) as u16 {
                ID_UNDO => unsafe { DDE_UNDO },
                ID_DEFAULT => (1, 1),
                _ => return 0,
            };
            write_int(b"PM_DDESupport", dde);
            write_int(b"PM_ClipboardSupport", clip);
            dde_fill(h);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddDDEPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, dde_proc, alloc::vec![
        group(b"Dynamic data exchange (DDE)", (4, 72, 228, 44)),
        radio(b"~Public: WIN-OS/2 and OS/2 programs exchange data", (10, 94, 216), ID_DDE_PUBLIC, true),
        radio(b"P~rivate: only within WIN-OS/2", (10, 80, 216), ID_DDE_PRIVATE, false),
        group(b"Clipboard", (4, 22, 228, 44)),
        radio(b"Pu~blic: one clipboard for WIN-OS/2 and OS/2", (10, 44, 216), ID_CLIP_PUBLIC, true),
        radio(b"Pri~vate: WIN-OS/2 keeps its own clipboard", (10, 30, 216), ID_CLIP_PRIVATE, false),
        push(b"~Undo", (4, 2), ID_UNDO),
        push(b"~Default", (48, 2), ID_DEFAULT),
    ], b"~Data Exchange\0")
}

// ---- The WIN-OS/2 Session page ----

const ID_FULLSCREEN: u16 = 0xc9;
const ID_WINDOW: u16 = 0xca;
const ID_SEPARATE: u16 = 0xcb;
const ID_SETTINGS: u16 = 0xcc;
const ID_FASTLOAD: u16 = 0x12d;
const ID_STATE: u16 = 0x12e;

static mut SESSION_UNDO: (u32, u32, Vec<Vec<u8>>) = (PROG_31_ENHSEAMLESSCOMMON, 0, Vec::new());

fn session_fill(h: u32) {
    let t = session_type();
    check(h, if t == PROG_31_ENH { ID_FULLSCREEN } else { ID_WINDOW }, true);
    check(h, ID_SEPARATE, t == PROG_31_ENHSEAMLESSVDM);
    crate::syssetup::enable(h, ID_SEPARATE, t != PROG_31_ENH);
    check(h, ID_FASTLOAD, read_int(b"PM_Global31FastLoad", 0) != 0);
    crate::syssetup::enable(h, ID_FASTLOAD, t != PROG_31_ENH);
    let n = global_settings().len();
    set_item_text(h, ID_STATE as u32, &alloc::format!("{n} WIN-OS/2 setting{} changed from the defaults", if n == 1 { "" } else { "s" }).into_bytes());
}

/// The session type the buttons say (IBM's ProgCatFromSessionButtons for
/// enhanced-mode WIN-OS/2 3.1).
fn buttons_type(h: u32) -> u32 {
    if checked(h, ID_FULLSCREEN) {
        PROG_31_ENH
    } else if checked(h, ID_SEPARATE) {
        PROG_31_ENHSEAMLESSVDM
    } else {
        PROG_31_ENHSEAMLESSCOMMON
    }
}

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

extern "C" fn session_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe {
                WinSetWindowULong(h, QWL_USER, b);
                SESSION_UNDO = (session_type(), read_int(b"PM_Global31FastLoad", 0), global_settings());
            }
            session_fill(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_FULLSCREEN | ID_WINDOW | ID_SEPARATE => {
                    write_int(b"PM_Global31SessionType", buttons_type(h));
                    if buttons_type(h) == PROG_31_ENH {
                        // Fast load is for windowed sessions only.
                        write_int(b"PM_Global31FastLoad", 0);
                    }
                    session_fill(h);
                }
                ID_FASTLOAD => write_int(b"PM_Global31FastLoad", checked(h, ID_FASTLOAD) as u32),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match (a & 0xffff) as u16 {
                ID_SETTINGS => {
                    let o = page_obj(h);
                    if let Some(new) = crate::dosset::run(h, &obj::title(o), &global_settings()) {
                        set_global_settings(&new);
                        session_fill(h);
                    }
                }
                ID_UNDO => {
                    let (t, f, s) = unsafe { SESSION_UNDO.clone() };
                    write_int(b"PM_Global31SessionType", t);
                    write_int(b"PM_Global31FastLoad", f);
                    set_global_settings(&s);
                    session_fill(h);
                }
                ID_DEFAULT => {
                    write_int(b"PM_Global31SessionType", PROG_31_ENHSEAMLESSCOMMON);
                    write_int(b"PM_Global31FastLoad", 0);
                    set_global_settings(&[]);
                    session_fill(h);
                }
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSession31Page(o: usize, nb: u32) -> u32 {
    add_page(o, nb, session_proc, alloc::vec![
        group(b"WIN-OS/2 programs run", (4, 58, 228, 58)),
        radio(b"WIN-OS/2 ~full screen", (10, 94, 210), ID_FULLSCREEN, true),
        radio(b"WIN-OS/2 ~window", (10, 80, 210), ID_WINDOW, false),
        checkbox(b"~Separate session for each program", (22, 66, 200), ID_SEPARATE),
        checkbox(b"~Load WIN-OS/2 at startup (fast load)", (6, 44, 220), ID_FASTLOAD),
        item(WC_BUTTON, b"WIN-OS/2 ~settings...", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (6, 24, 90, 14), ID_SETTINGS),
        text(b"", (100, 27, 132), ID_STATE),
        push(b"~Undo", (4, 2), ID_UNDO),
        push(b"~Default", (48, 2), ID_DEFAULT),
    ], b"WIN-OS/2 ~Session\0")
}

/// IBM's Session page for WIN-OS/2 3.0 (`PM_GlobalSessionType`); rs2's
/// WIN-OS/2 is 3.1, so the 3.1 page stands for both.
extern "C" fn wpAddSessionPage(o: usize, nb: u32) -> u32 {
    wpAddSession31Page(o, nb)
}

/// IBM's order: Data Exchange, then the Session page (each inserted in
/// front of those before it).
extern "C" fn wincfg_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPWinConfig"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    if !installed() {
        return 1;
    }
    wps::call("WPWinConfig", m::WPWinConfig::wpAddSession31Page, &[o, nb as usize]);
    wps::call("WPWinConfig", m::WPWinConfig::wpAddDDEPage, &[o, nb as usize]);
    1
}

methods! {
    __m_wpAddDDEPage => wpAddDDEPage, 2;
    __m_wpAddSessionPage => wpAddSessionPage, 2;
    __m_wpAddSession31Page => wpAddSession31Page, 2;
    __m_wincfg_add_pages => wincfg_add_pages, 2;
}

pub fn impl_win_config() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![
            ("wpAddDDEPage", __m_wpAddDDEPage as usize),
            ("wpAddSessionPage", __m_wpAddSessionPage as usize),
            ("wpAddSession31Page", __m_wpAddSession31Page as usize),
        ],
        overrides: alloc::vec![("wpAddSettingsPages", __m_wincfg_add_pages as usize), crate::pages::no_window_page_override()],
    }
}
