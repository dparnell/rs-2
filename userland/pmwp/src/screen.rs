//! The System object's Screen and font pages (WPCONFIG's
//! wpAddSystemScreenPage, wpAddDMQSDisplayTypePage and
//! wpAddSystemFontSettingPages; rs2's own pages).
//!
//! IBM's Screen page comes from the display driver's configuration
//! module (VideoCfgServicesModule's pfnScreenPageDlgProc: resolutions,
//! colours, refresh rates) and its second page lists monitor types
//! (DMQS). rs2's display drivers report the resolutions they can switch
//! to themselves, so:
//!
//! - **Screen** lists them (RS2API Rs2QueryDisplayModes) with the
//!   colours (16 million, or 256 from a palette); Apply switches at once
//!   (Rs2SetDisplayMode: PM lays the desktop out again) and the new
//!   resolution stays only if Keep is chosen within 15 seconds, as the
//!   Display program does. A kept resolution is saved as `VIDEO=WxH`
//!   (`x256` for 256 colours, which take effect at the next start) in
//!   CONFIG.SYS, which the kernel applies when it starts.
//! - **Screen, page 2 (display type)** shows what the display is: its
//!   resolution, colours, line length and whether its driver can change
//!   the resolution.
//! - **Fonts** chooses the font of icon titles in folders and on the
//!   desktop that have no font of their own (`PM_SystemFonts` IconText,
//!   the key IBM's PMWP reads); folder windows follow at once.

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::book;
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio};
use crate::syssetup::{group, listbox, push, text};
use crate::util::{prf_read, prf_write, HINI_USER};

unsafe extern "C" {
    #[link_name = "RS2API.21"]
    fn Rs2QueryDisplay(info: *mut u32) -> u32;
    #[link_name = "RS2API.22"]
    fn Rs2QueryDisplayModes(pul: *mut u32, max: u32, pc: *mut u32) -> u32;
    #[link_name = "RS2API.23"]
    fn Rs2SetDisplayMode(cx: u32, cy: u32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn current() -> (u32, u32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, 20) as u32, WinQuerySysValue(HWND_DESKTOP, 21) as u32) }
}

fn modes() -> Vec<(u32, u32)> {
    let mut v = [0u32; 64];
    let mut n = 0;
    if unsafe { Rs2QueryDisplayModes(v.as_mut_ptr(), 32, &mut n) } != 0 {
        return Vec::new();
    }
    (0..n.min(32) as usize).map(|i| (v[i * 2], v[i * 2 + 1])).collect()
}

/// The display now: (width, height, line length in bytes, 256 colours).
fn display_info() -> Option<(u32, u32, u32, bool)> {
    let mut d = [0u32; 6];
    (unsafe { Rs2QueryDisplay(d.as_mut_ptr()) } == 0).then(|| (d[1], d[2], d[3], d[4] & 2 != 0))
}

// ---- CONFIG.SYS's VIDEO= ----

fn config_path() -> Vec<u8> {
    let mut p = alloc::vec![crate::install::boot_drive()];
    p.extend_from_slice(b":\\CONFIG.SYS");
    p
}

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut act) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS; OPEN_SHARE_DENYNONE | read only.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut got = 0;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 || got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(out)
}

fn write_file(path: &[u8], data: &[u8]) -> bool {
    let p = z(path);
    let (mut hf, mut act) = (0u32, 0u32);
    // OPEN_ACTION_REPLACE_IF_EXISTS | CREATE_IF_NEW; deny write, write only.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, data.len() as u32, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut done = 0;
    let rc = unsafe { os2::DosWrite(hf, data.as_ptr(), data.len() as u32, &mut done) };
    unsafe { os2::DosClose(hf) };
    rc == 0 && done as usize == data.len()
}

/// Whether CONFIG.SYS's VIDEO= asks for 256 colours.
fn saved256() -> bool {
    read_file(&config_path()).unwrap_or_default().split(|&b| b == b'\n').any(|l| {
        let t: Vec<u8> = l.iter().filter(|b| !b.is_ascii_whitespace()).map(|b| b.to_ascii_uppercase()).collect();
        t.starts_with(b"VIDEO=") && t.ends_with(b"X256")
    })
}

/// CONFIG.SYS with its VIDEO= statement (any there replaced).
pub fn with_video(text: &[u8], w: u32, h: u32, c256: bool) -> Vec<u8> {
    let text = match text.iter().position(|&b| b == 0x1a) {
        Some(i) => &text[..i],
        None => text,
    };
    let mut out = Vec::new();
    for line in text.split_inclusive(|&b| b == b'\n') {
        let t: Vec<u8> = line.iter().copied().skip_while(|&b| b == b' ' || b == b'\t').collect();
        if t.len() >= 6 && t[..6].eq_ignore_ascii_case(b"VIDEO=") {
            continue;
        }
        out.extend_from_slice(line);
    }
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(alloc::format!("VIDEO={w}x{h}{}\r\n", if c256 { "x256" } else { "" }).as_bytes());
    out
}

fn save(w: u32, h: u32, c256: bool) -> bool {
    let path = config_path();
    let old = read_file(&path).unwrap_or_default();
    write_file(&path, &with_video(&old, w, h, c256))
}

// ---- Screen ----

const ID_MODES: u16 = 1240;
const ID_TRUE: u16 = 1241;
const ID_256: u16 = 1242;
const ID_APPLY: u16 = 1243;
const ID_REVERT: u16 = 1244;
const ID_STATUS: u16 = 1245;
const ID_STATUS2: u16 = 1246;
const TID_KEEP: u32 = 0x5730;
const KEEP_SECONDS: u32 = 15;

/// A switch waiting for Keep: (page, old size, new size, seconds left).
static mut ASKING: Option<(u32, (u32, u32), (u32, u32), u32)> = None;
static mut LISTED: Vec<(u32, u32)> = Vec::new();

fn status(h: u32, a: &str, b: &str) {
    set_item_text(h, ID_STATUS as u32, a.as_bytes());
    set_item_text(h, ID_STATUS2 as u32, b.as_bytes());
}

fn colors_note() -> &'static str {
    let now = display_info().is_some_and(|d| d.3);
    match (now, saved256()) {
        (true, true) => "256 colors.",
        (false, false) => "16 million colors.",
        (true, false) => "256 colors now, 16 million after the next start.",
        (false, true) => "16 million colors now, 256 after the next start.",
    }
}

fn screen_fill(h: u32) {
    let lb = unsafe { WinWindowFromID(h, ID_MODES as u32) };
    unsafe { WinSendMsg(lb, LM_DELETEALL, 0, 0) };
    let cur = current();
    let list = modes();
    let mut sel = None;
    for (i, &(w, hh)) in list.iter().enumerate() {
        let mark = if (w, hh) == cur { "  (current)" } else { "" };
        if (w, hh) == cur {
            sel = Some(i);
        }
        let t = z(alloc::format!("{w} x {hh}{mark}").as_bytes());
        unsafe { WinSendMsg(lb, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
    }
    if let Some(i) = sel {
        unsafe { WinSendMsg(lb, LM_SELECTITEM, i as u32, 1) };
    }
    check(h, if saved256() { ID_256 } else { ID_TRUE }, true);
    crate::syssetup::enable(h, ID_REVERT, false);
    set_item_text(h, ID_APPLY as u32, b"~Apply");
    if list.is_empty() {
        status(h, &alloc::format!("The screen stays at {} x {}: its driver", cur.0, cur.1), "cannot change the resolution.");
    } else {
        status(h, &alloc::format!("The screen is at {} x {}.", cur.0, cur.1), colors_note());
    }
    unsafe { LISTED = list };
}

fn selected(h: u32) -> Option<(u32, u32)> {
    let i = unsafe { WinSendDlgItemMsg(h, ID_MODES as u32, LM_QUERYSELECTION, LIT_NONE, 0) };
    unsafe { LISTED.get(i as usize).copied() }
}

fn apply(h: u32) {
    let old = current();
    let (w, hh) = selected(h).unwrap_or(old);
    let c = checked(h, ID_256);
    if c != saved256() && !save(old.0, old.1, c) {
        status(h, "The colors could not be saved", "in CONFIG.SYS.");
        return;
    }
    if (w, hh) == old {
        screen_fill(h);
        return;
    }
    if unsafe { Rs2SetDisplayMode(w, hh) } != 0 {
        status(h, &alloc::format!("The display cannot switch to {w} x {hh}."), "");
        return;
    }
    unsafe {
        ASKING = Some((h, old, (w, hh), KEEP_SECONDS));
        WinStartTimer(hab(), h, TID_KEEP, 1000);
    }
    set_item_text(h, ID_APPLY as u32, b"~Keep");
    crate::syssetup::enable(h, ID_REVERT, true);
    crate::syssetup::enable(h, ID_MODES, false);
    ask_text(h);
}

fn ask_text(h: u32) {
    if let Some((_, _, (w, hh), left)) = unsafe { ASKING } {
        status(h, &alloc::format!("Keep {w} x {hh}?"), &alloc::format!("The old resolution comes back in {left} seconds."));
    }
}

/// Keep the new resolution (saved for the next start), or go back.
fn answer(h: u32, keep: bool) {
    let Some((_, old, new, _)) = (unsafe { ASKING.take() }) else { return };
    unsafe { WinStopTimer(hab(), h, TID_KEEP) };
    if !keep {
        unsafe { Rs2SetDisplayMode(old.0, old.1) };
    }
    crate::syssetup::enable(h, ID_MODES, true);
    screen_fill(h);
    if keep && !save(new.0, new.1, saved256()) {
        status(h, &alloc::format!("The screen is at {} x {}.", new.0, new.1), "It could not be saved in CONFIG.SYS.");
    }
}

extern "C" fn screen_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            screen_fill(h);
            0
        }
        WM_TIMER if a & 0xffff == TID_KEEP => {
            let left = unsafe { ASKING.as_mut().map(|x| { x.3 = x.3.saturating_sub(1); x.3 }) };
            match left {
                Some(0) => answer(h, false),
                Some(_) => ask_text(h),
                None => unsafe {
                    WinStopTimer(hab(), h, TID_KEEP);
                },
            }
            0
        }
        WM_COMMAND => {
            match (a & 0xffff) as u16 {
                ID_APPLY if unsafe { ASKING.is_some() } => answer(h, true),
                ID_APPLY => apply(h),
                ID_REVERT => answer(h, false),
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            // Closing the notebook while asking: the old resolution.
            if unsafe { ASKING.is_some_and(|x| x.0 == h) } {
                answer(h, false);
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

pub extern "C" fn wpAddSystemScreenPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, screen_proc, alloc::vec![
        book::label(b"Screen resolution:", (6, 108, 120)),
        listbox((6, 46, 120, 60), ID_MODES),
        group(b"Colors", (134, 66, 98, 50)),
        radio(b"~16 million", (140, 92, 88), ID_TRUE, true),
        radio(b"~256 (palette)", (140, 78, 88), ID_256, false),
        text(b"", (6, 32, 226), ID_STATUS),
        text(b"", (6, 22, 226), ID_STATUS2),
        push(b"~Apply", (4, 2), ID_APPLY),
        push(b"~Revert", (48, 2), ID_REVERT),
    ], b"~Screen\0")
}

// ---- Screen, page 2: the display type ----

const ID_TYPE: u16 = 1250;

extern "C" fn type_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let lines: Vec<alloc::string::String> = match display_info() {
                Some((w, hh, stride, c256)) => alloc::vec![
                    alloc::format!("Resolution: {w} x {hh}"),
                    alloc::format!("Colors: {}", if c256 { "256 (palette)" } else { "16 million (32 bits a pel)" }),
                    alloc::format!("Line length: {stride} pels"),
                    alloc::format!("Resolutions the driver can switch to: {}", modes().len()),
                    "Monitor: Plug and Play (the driver reports the sizes)".into(),
                ],
                None => alloc::vec!["The display could not be queried.".into()],
            };
            for (i, l) in lines.iter().enumerate() {
                set_item_text(h, (ID_TYPE + i as u16) as u32, l.as_bytes());
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

pub extern "C" fn wpAddDMQSDisplayTypePage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![group(b"Display", (4, 40, 228, 76))];
    for i in 0..5u16 {
        items.push(text(b"", (10, 96 - 12 * i as i16, 216), ID_TYPE + i));
    }
    let tpl = template(book::page_frame(), items);
    let page = book::make_page(nb, type_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert_after(o, nb, page, b"Screen - Display type\0", b"~Screen")
}

// ---- Fonts ----

const ID_ICONFONT: u16 = 1260;
const ID_FONTDEFAULT: u16 = 1261;

/// The icon text font of folders without their own (`PM_SystemFonts`
/// IconText, "8.Helv"), if set.
pub fn icon_text_font() -> Option<Vec<u8>> {
    let v = prf_read(HINI_USER, b"PM_SystemFonts", b"IconText")?;
    let v = v.strip_suffix(&[0]).unwrap_or(&v).to_vec();
    (!v.is_empty()).then_some(v)
}

fn set_icon_text_font(name: Option<&[u8]>) {
    match name {
        Some(n) => {
            let mut v = n.to_vec();
            v.push(0);
            prf_write(HINI_USER, b"PM_SystemFonts", b"IconText", Some(&v));
        }
        None => {
            prf_write(HINI_USER, b"PM_SystemFonts", b"IconText", None);
        }
    }
    // Open folder windows follow.
    let objs: Vec<usize> = crate::view::wins().iter().map(|w| w.obj).collect();
    for o in objs {
        crate::view::background_changed(o);
    }
}

fn fonts_fill(h: u32) {
    let lb = unsafe { WinWindowFromID(h, ID_ICONFONT as u32) };
    unsafe { WinSendMsg(lb, LM_DELETEALL, 0, 0) };
    let cur = icon_text_font();
    let mut sel = 0;
    let def = z(b"System font (the default)");
    unsafe { WinSendMsg(lb, LM_INSERTITEM, LIT_END, def.as_ptr() as u32) };
    for (i, f) in crate::setup::FONTS.iter().enumerate() {
        let t = z(f);
        unsafe { WinSendMsg(lb, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
        if cur.as_deref() == Some(*f) {
            sel = i + 1;
        }
    }
    unsafe { WinSendMsg(lb, LM_SELECTITEM, sel as u32, 1) };
}

extern "C" fn fonts_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            fonts_fill(h);
            0
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_ICONFONT && a >> 16 == LN_SELECT => {
            let i = unsafe { WinSendDlgItemMsg(h, ID_ICONFONT as u32, LM_QUERYSELECTION, LIT_NONE, 0) } as usize;
            let want = if i == 0 { None } else { crate::setup::FONTS.get(i - 1).copied() };
            if want.map(|w| w.to_vec()) != icon_text_font() {
                set_icon_text_font(want);
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_FONTDEFAULT {
                set_icon_text_font(None);
                fonts_fill(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

pub extern "C" fn wpAddSystemFontSettingPages(o: usize, nb: u32) -> u32 {
    add_page(o, nb, fonts_proc, alloc::vec![
        book::label(b"Icon text in folders and on the desktop:", (6, 108, 220)),
        listbox((6, 26, 160, 80), ID_ICONFONT),
        book::label(b"A folder's own font (Font Palette) comes first.", (6, 14, 226)),
        push(b"~Default", (180, 26), ID_FONTDEFAULT),
    ], b"~Fonts\0")
}
