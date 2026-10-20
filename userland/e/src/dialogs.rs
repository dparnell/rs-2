//! The editor's dialogs, laid out and behaving as OS/2's E's: the
//! standard file dialogs (Save as with a Type... button) and font dialog
//! (with Apply), and its own, built from templates in memory: Warning:
//! File Changed, Save notification (no type), the unrecognised-type
//! question, Type, NUL characters, Autosave, Find (modeless), Set Colors
//! and Product information. Dialog and control ids are E's, so the help
//! table finds their panels; Help buttons show the dialog's panel.

use alloc::vec::Vec;

use crate::pm::*;
use crate::*;

const QWL_USER: i32 = 0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Save,
    SaveAs,
    Discard,
    Cancel,
    Type,
    Yes,
    No,
}

/// A warning icon (SPTR_ICONWARNING) static.
fn warning_icon(x: i16, y: i16) -> Item {
    item(WC_STATIC, b"#14", WS_VISIBLE | SS_SYSICON, (x, y, 22, 16), 0x7501)
}

fn help_button(x: i16, y: i16, cx: i16) -> Item {
    button(b"Help", (x, y, cx), ID_HELP, BS_HELP)
}

// ---- Warning: File Changed ----

const ID_CHANGED_TEXT: u32 = 0x3002;
const ID_CH_SAVE: u32 = 0x3003;
const ID_CH_SAVEAS: u32 = 0x3004;
const ID_CH_DISCARD: u32 = 0x3005;

extern "C" fn changed_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            // Untitled has no Save.
            if a.path.is_none() {
                enable(h, ID_CH_SAVE, false);
            }
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, DID_CANCEL)) };
            1
        }
        WM_COMMAND => {
            // Save and Save as stay in the dialog until the file is
            // written.
            let r = match mp1 & 0xffff {
                ID_CH_SAVE => save().then_some(Answer::Save),
                ID_CH_SAVEAS => save_as().then_some(Answer::SaveAs),
                ID_CH_DISCARD => Some(Answer::Discard),
                DID_CANCEL => Some(Answer::Cancel),
                _ => None,
            };
            if let Some(r) = r {
                unsafe { WinDismissDlg(h, 100 + r as u32) };
            }
            0
        }
        WM_HELP => {
            help_panel(PANEL_CHANGED);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// "Warning: File Changed": NAME has changed. Do you want to save it?
/// Save, Save as (both save first), Discard or Cancel.
pub fn file_changed() -> Answer {
    let name = app().path.clone().unwrap_or_else(|| UNTITLED.to_vec());
    let mut text = name;
    text.extend_from_slice(b" has changed. Do you want to save it?");
    let mut t = label(&text, (48, 24, 177, 28), DT_CENTER | DT_WORDBREAK);
    t.id = ID_CHANGED_TEXT as u16;
    let tpl = template(dialog_frame(b"Warning: File Changed", DLG_CHANGED, 231, 58), alloc::vec![
        warning_icon(11, 30),
        t,
        button(b"~Save", (5, 5, 38), ID_CH_SAVE, WS_GROUP),
        button(b"Save ~as...", (47, 5, 48), ID_CH_SAVEAS, 0),
        button(b"~Discard", (101, 5, 38), ID_CH_DISCARD, 0),
        button(b"Cancel", (143, 5, 38), DID_CANCEL, BS_DEFAULT),
        help_button(187, 5, 38),
    ]);
    match run_dialog(app().frame, changed_proc, &tpl, 0) {
        100 => Answer::Save,
        101 => Answer::SaveAs,
        102 => Answer::Discard,
        _ => Answer::Cancel,
    }
}

// ---- Save notification (no type), unrecognised types ----

const ID_NT_TYPE: u32 = 0x425;
const ID_NT_SAVEAS: u32 = 0x426;
const ID_BT_YES: u32 = 0x42f;
const ID_BT_NO: u32 = 0x430;

/// Dialog parameters: (help panel, autosaving, button with the focus).
static mut QUESTION: (u16, bool, u32) = (0, false, 0);

extern "C" fn question_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    let (panel, autosaving, focus) = unsafe { QUESTION };
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            if autosaving {
                let t = z(b"Autosave notification");
                unsafe { WinSetWindowText(h, t.as_ptr()) };
            }
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, focus)) };
            1
        }
        WM_COMMAND => {
            let r = match mp1 & 0xffff {
                // Type...: the Type dialog; Set saves.
                ID_NT_TYPE => choose_type().then_some(Answer::Type),
                ID_NT_SAVEAS => Some(Answer::SaveAs),
                ID_BT_YES => Some(Answer::Yes),
                ID_BT_NO => Some(Answer::No),
                DID_CANCEL => {
                    // Cancel while autosaving turns autosave off.
                    if autosaving {
                        a.autosave = false;
                    }
                    Some(Answer::Cancel)
                }
                _ => None,
            };
            if let Some(r) = r {
                unsafe { WinDismissDlg(h, 100 + r as u32) };
            }
            0
        }
        WM_HELP => {
            help_panel(panel);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn run_question(tpl: &[u8], panel: u16, autosaving: bool, focus: u32) -> Answer {
    unsafe { QUESTION = (panel, autosaving, focus) };
    match run_dialog(app().frame, question_proc, tpl, 0) {
        103 => Answer::Cancel,
        104 => Answer::Type,
        101 => Answer::SaveAs,
        105 => Answer::Yes,
        106 => Answer::No,
        _ => Answer::Cancel,
    }
}

/// "Save notification": the file has no type. Type... (the Type dialog,
/// then the file is saved with that type), Save as..., Cancel.
pub fn no_type(autosaving: bool) -> Answer {
    let tpl = template(dialog_frame(b"Save notification", DLG_NOTYPE, 245, 60), alloc::vec![
        label(b"There is no file type associated with this file.", (10, 41, 225, 11), DT_WORDBREAK),
        label(b"Select the Type button to add one.", (10, 25, 225, 12), DT_WORDBREAK),
        button(b"~Type...", (5, 3, 55), ID_NT_TYPE, WS_GROUP | BS_DEFAULT),
        button(b"~Save as...", (65, 3, 55), ID_NT_SAVEAS, 0),
        button(b"Cancel", (125, 3, 54), DID_CANCEL, 0),
        help_button(185, 3, 56),
    ]);
    run_question(&tpl, PANEL_NOTYPE, autosaving, ID_NT_TYPE)
}

/// The file's types are none the editor knows: add "Plain Text" (Yes),
/// save as it is (No), Save as..., Cancel.
pub fn bad_type(autosaving: bool) -> Answer {
    let tpl = template(dialog_frame(b"Save notification", DLG_BADTYPE, 186, 77), alloc::vec![
        label(b"The System Editor does not recognize the type of this file.", (10, 52, 166, 19), DT_WORDBREAK),
        label(b"Add the type \"Plain Text\"?", (10, 39, 166, 12), DT_WORDBREAK),
        button(b"~Yes", (5, 20, 55), ID_BT_YES, WS_GROUP | BS_DEFAULT),
        button(b"~No", (65, 20, 54), ID_BT_NO, 0),
        button(b"~Save as...", (125, 20, 55), ID_NT_SAVEAS, 0),
        button(b"Cancel", (65, 3, 54), DID_CANCEL, 0),
        help_button(125, 3, 56),
    ]);
    run_question(&tpl, PANEL_BADTYPE, autosaving, ID_BT_YES)
}

// ---- Type ----

const ID_TYPELIST: u32 = 0x41b;

extern "C" fn type_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            for t in TYPES {
                let s = z(t);
                unsafe { WinSendDlgItemMsg(h, ID_TYPELIST, LM_INSERTITEM, LIT_END, s.as_ptr() as u32) };
            }
            unsafe {
                WinSendDlgItemMsg(h, ID_TYPELIST, LM_SELECTITEM, a.default_type as u32, 1);
                WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_TYPELIST));
            }
            1
        }
        WM_CONTROL if mp1 & 0xffff == ID_TYPELIST && mp1 >> 16 == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            let mut r = 0;
            if mp1 & 0xffff == DID_OK {
                let sel = unsafe { WinSendDlgItemMsg(h, ID_TYPELIST, LM_QUERYSELECTION, LIT_FIRST, 0) } as i32;
                if sel >= 0 && (sel as usize) < TYPES.len() {
                    // The type chosen becomes the default.
                    a.default_type = sel as usize;
                    save_int(b"USDEFAULTTYPEEA\0", sel);
                    r = 1;
                }
            }
            unsafe { WinDismissDlg(h, r) };
            0
        }
        WM_HELP => {
            help_panel(PANEL_TYPE);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// The Type dialog: Set makes the type chosen the default type.
pub fn choose_type() -> bool {
    let tpl = template(dialog_frame(b"Type", DLG_TYPE, 150, 72), alloc::vec![
        item(WC_STATIC, b"", WS_VISIBLE | SS_GROUPBOX, (5, 21, 140, 54), 0xffff),
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (9, 25, 132, 42), ID_TYPELIST as u16),
        button(b"~Set", (8, 3, 38), DID_OK, WS_GROUP | BS_DEFAULT),
        button(b"Cancel", (58, 3, 38), DID_CANCEL, 0),
        help_button(105, 3, 38),
    ]);
    run_dialog(app().frame, type_proc, &tpl, 0) == 1
}

// ---- NUL characters ----

extern "C" fn nulls_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            place_over(h, app().frame);
            0
        }
        WM_COMMAND => {
            unsafe { WinDismissDlg(h, (mp1 & 0xffff == DID_OK) as u32) };
            0
        }
        WM_HELP => {
            help_panel(PANEL_NULLS);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// The file has NUL characters: load it with blanks for them (true), or
/// not at all.
pub fn nulls() -> bool {
    let tpl = template(dialog_frame(b"System Editor - Warning", DLG_NULLS, 248, 70), alloc::vec![
        warning_icon(12, 46),
        label(b"The file contains NUL characters.", (44, 50, 196, 10), DT_WORDBREAK),
        label(b"Continue to load it with blanks in their place, or stop loading it.", (6, 20, 236, 24), DT_CENTER | DT_WORDBREAK),
        button(b"~Continue loading file", (4, 2, 107), DID_OK, WS_GROUP | BS_DEFAULT),
        button(b"~Stop loading file", (117, 2, 85), DID_CANCEL, 0),
        help_button(207, 2, 38),
    ]);
    run_dialog(app().frame, nulls_proc, &tpl, 0) == 1
}

// ---- Product information ----

extern "C" fn product_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            place_over(h, app().frame);
            0
        }
        WM_COMMAND => {
            unsafe { WinDismissDlg(h, 1) };
            0
        }
        WM_HELP => {
            help_panel(PANEL_PRODUCT);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

pub fn product_info() {
    let tpl = template(dialog_frame(b"System Editor - Product information", DLG_PRODUCT, 229, 82), alloc::vec![
        label(b"rs2", (44, 70, 180, 8), 0),
        label(b"System Editor", (44, 60, 180, 8), 0),
        label(b"Version 1.0", (44, 50, 180, 8), 0),
        item(WC_STATIC, b"#10", WS_VISIBLE | SS_SYSICON, (11, 39, 21, 16), 0x7501),
        label(b"A re-implementation of the OS/2 System Editor.", (44, 40, 180, 8), 0),
        label(b"MIT or Apache-2.0 licence.", (44, 30, 180, 8), 0),
        button(b"OK", (92, 5, 36), DID_OK, WS_GROUP | BS_DEFAULT),
    ]);
    run_dialog(app().frame, product_proc, &tpl, 0);
}

// ---- File dialogs ----

const FILEDLG_SIZE: usize = 328;
const FDS_CENTER: u32 = 0x0001;
const FDS_HELPBUTTON: u32 = 0x0008;
const FDS_OPEN_DIALOG: u32 = 0x0100;
const FDS_SAVEAS_DIALOG: u32 = 0x0200;
const FDS_ENABLEFILELB: u32 = 0x0800;
/// The Type... button added to Save as.
const ID_FD_TYPE: u32 = 7000;
/// The file dialog's Help button.
const DID_HELP_PB: u32 = 0x10b;

fn wr32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn rd32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

extern "C" fn open_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m == WM_HELP {
        help_panel(PANEL_OPEN);
        return 0;
    }
    unsafe { WinDefFileDlgProc(h, m, mp1, mp2) }
}

extern "C" fn saveas_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            let r = unsafe { WinDefFileDlgProc(h, m, mp1, mp2) };
            // A Type... button where Cancel was; Cancel and Help move
            // right.
            let (cancel, help) = unsafe { (WinWindowFromID(h, DID_CANCEL), WinWindowFromID(h, DID_HELP_PB)) };
            let (c, hp) = (window_pos(cancel), window_pos(help));
            let t = z(b"~Type...");
            unsafe {
                WinCreateWindow(h, 0xffff_0003, t.as_ptr(), WS_VISIBLE | WS_TABSTOP, c.x, c.y, c.cx, c.cy, h, cancel, ID_FD_TYPE, core::ptr::null(), core::ptr::null());
                if help != 0 {
                    WinSetWindowPos(cancel, 0, hp.x, hp.y, 0, 0, SWP_MOVE);
                    WinSetWindowPos(help, 0, 2 * hp.x - c.x, hp.y, 0, 0, SWP_MOVE);
                } else {
                    WinSetWindowPos(cancel, 0, 2 * c.x - c.x + c.cx + 6, c.y, 0, 0, SWP_MOVE);
                }
            }
            r
        }
        WM_COMMAND if mp1 & 0xffff == ID_FD_TYPE => {
            choose_type();
            0
        }
        WM_HELP => {
            help_panel(PANEL_SAVEAS);
            0
        }
        _ => unsafe { WinDefFileDlgProc(h, m, mp1, mp2) },
    }
}

/// The standard file dialog; the file chosen.
fn run_file_dialog(save: bool) -> Option<Vec<u8>> {
    let a = app();
    let mut fd = alloc::vec![0u8; FILEDLG_SIZE];
    wr32(&mut fd, 0, FILEDLG_SIZE as u32);
    let fl = FDS_CENTER | FDS_HELPBUTTON | if save { FDS_SAVEAS_DIALOG | FDS_ENABLEFILELB } else { FDS_OPEN_DIALOG };
    wr32(&mut fd, 4, fl);
    let (title, ok): (&[u8], &[u8]) = if save { (b"Save as\0", b"~Save\0") } else { (b"Open\0", b"~Open\0") };
    wr32(&mut fd, 20, title.as_ptr() as u32);
    wr32(&mut fd, 24, ok.as_ptr() as u32);
    wr32(&mut fd, 28, if save { saveas_proc as *const () as u32 } else { open_proc as *const () as u32 });
    // Save as lists the file types.
    let names: Vec<Vec<u8>> = TYPES.iter().map(|t| z(t)).collect();
    let mut list: Vec<u32> = names.iter().map(|n| n.as_ptr() as u32).collect();
    list.push(0);
    if save {
        wr32(&mut fd, 36, list.as_ptr() as u32);
    }
    let initial: Vec<u8> = match &a.path {
        Some(p) => p.clone(),
        None if save => Vec::new(),
        None => b"*.*".to_vec(),
    };
    let n = initial.len().min(259);
    fd[52..52 + n].copy_from_slice(&initial[..n]);
    fd[326..328].copy_from_slice(&(-1i16).to_le_bytes());
    let ok = unsafe { WinFileDlg(HWND_DESKTOP, a.client, fd.as_mut_ptr()) } != 0;
    if !ok || rd32(&fd, 12) != DID_OK {
        return None;
    }
    let name = until_nul(&fd[52..312]).to_vec();
    (!name.is_empty()).then_some(name)
}

/// File > Open's dialog.
pub fn open_dialog() -> Option<Vec<u8>> {
    run_file_dialog(false)
}

/// File > Save as's dialog.
pub fn save_as_dialog() -> Option<Vec<u8>> {
    run_file_dialog(true)
}

// ---- Font ----

const FONTDLG_SIZE: usize = 168;
const FNTS_CENTER: u32 = 0x0001;
const FNTS_HELPBUTTON: u32 = 0x0008;
const FNTS_APPLYBUTTON: u32 = 0x0010;
const FNTS_INITFROMFATTRS: u32 = 0x0080;
const DID_APPLY_BUTTON: u32 = 0x137;

/// The FONTDLG's choice on the MLE (OK and Apply), kept in OS2.INI.
fn apply_font(fd: *const u8) {
    let a = app();
    let mut f = [0u8; 56];
    unsafe { core::ptr::copy_nonoverlapping(fd.add(96), f.as_mut_ptr(), 56) };
    a.fattrs = Some(f);
    a.point_size = unsafe { (fd.add(32) as *const u32).read_unaligned() };
    send(a.mle, MLM_SETFONT, f.as_ptr() as u32, 0);
    set_tabs();
    save_data(b"FONTATTRS\0", &f);
    save_data(b"FXPTSIZE\0", &a.point_size.to_le_bytes());
    trace(alloc::format!("font {} {}", txt(until_nul(&f[8..40])), a.point_size >> 16).as_bytes());
}

extern "C" fn font_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_COMMAND if mp1 & 0xffff == DID_APPLY_BUTTON => {
            let r = unsafe { WinDefFontDlgProc(h, m, mp1, mp2) };
            let fd = unsafe { WinQueryWindowULong(h, QWL_USER) } as *const u8;
            if !fd.is_null() {
                apply_font(fd);
            }
            r
        }
        WM_HELP => {
            help_panel(PANEL_FONT);
            0
        }
        _ => unsafe { WinDefFontDlgProc(h, m, mp1, mp2) },
    }
}

/// Options > Set font: the font dialog (OK and Apply set the font).
pub fn font() {
    let a = app();
    let mut fd = alloc::vec![0u8; FONTDLG_SIZE];
    let mut family = [0u8; 64];
    let mut current = [0u8; 56];
    send(a.mle, MLM_QUERYFONT, current.as_mut_ptr() as u32, 0);
    let current = a.fattrs.unwrap_or(current);
    let face = until_nul(&current[8..40]);
    family[..face.len()].copy_from_slice(face);
    wr32(&mut fd, 0, FONTDLG_SIZE as u32);
    wr32(&mut fd, 12, b"Font\0".as_ptr() as u32);
    wr32(&mut fd, 24, font_proc as *const () as u32);
    wr32(&mut fd, 28, family.as_mut_ptr() as u32);
    wr32(&mut fd, 32, a.point_size);
    wr32(&mut fd, 36, FNTS_CENTER | FNTS_HELPBUTTON | FNTS_APPLYBUTTON | FNTS_INITFROMFATTRS);
    wr32(&mut fd, 60, a.fg as u32);
    wr32(&mut fd, 64, a.bg as u32);
    fd[96..152].copy_from_slice(&current);
    fd[164..166].copy_from_slice(&(family.len() as u16).to_le_bytes());
    let ok = unsafe { WinFontDlg(HWND_DESKTOP, a.client, fd.as_mut_ptr()) } != 0;
    if ok && rd32(&fd, 72) == DID_OK {
        apply_font(fd.as_ptr());
    }
}

// ---- Autosave ----

const ID_AS_SET: u32 = 0x5001;
const ID_AS_ON: u32 = 0x5002;
const ID_AS_LABEL: u32 = 0x5003;
const ID_AS_SPIN: u32 = 0x5004;
const ID_AS_CANCEL: u32 = 0x5005;

fn spin_value(h: u32) -> Option<u32> {
    let mut v = 0u32;
    let ok = unsafe { WinSendDlgItemMsg(h, ID_AS_SPIN, SPBM_QUERYVALUE, &mut v as *mut u32 as u32, mp(0, 3 /* SPBQ_DONOTUPDATE */)) } != 0;
    (ok && (1..=9999).contains(&v)).then_some(v)
}

extern "C" fn autosave_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            unsafe {
                WinSendDlgItemMsg(h, ID_AS_SPIN, SPBM_SETLIMITS, 9999, 1);
                WinSendDlgItemMsg(h, ID_AS_SPIN, SPBM_SETTEXTLIMIT, 4, 0);
                WinSendDlgItemMsg(h, ID_AS_SPIN, SPBM_SETCURRENTVALUE, a.autosave_every.clamp(1, 9999), 0);
            }
            set_check(h, ID_AS_ON, a.autosave);
            enable(h, ID_AS_SPIN, a.autosave);
            enable(h, ID_AS_LABEL, a.autosave);
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, if a.autosave { ID_AS_SPIN } else { ID_AS_SET })) };
            1
        }
        WM_CONTROL if mp1 & 0xffff == ID_AS_ON && mp1 >> 16 == BN_CLICKED => {
            let on = checked(h, ID_AS_ON);
            enable(h, ID_AS_SPIN, on);
            enable(h, ID_AS_LABEL, on);
            0
        }
        // Set only with a number of changes to set.
        WM_CONTROL if mp1 & 0xffff == ID_AS_SPIN && mp1 >> 16 == SPBN_CHANGE => {
            enable(h, ID_AS_SET, spin_value(h).is_some());
            0
        }
        WM_COMMAND => {
            if mp1 & 0xffff == ID_AS_SET {
                let Some(v) = spin_value(h) else {
                    unsafe { WinAlarm(HWND_DESKTOP, 1) };
                    return 0;
                };
                a.autosave_every = v;
                a.autosave = checked(h, ID_AS_ON);
                a.changes = 0;
                save_int(b"IAUTOSAVE\0", v as i32);
                save_int(b"BAUTOSAVEON\0", a.autosave as i32);
                trace(alloc::format!("autosave {} every {}", a.autosave as u32, a.autosave_every).as_bytes());
            }
            unsafe { WinDismissDlg(h, 0) };
            0
        }
        WM_HELP => {
            help_panel(PANEL_AUTOSAVE);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// File > Autosave: on or off, and the number of changes between saves.
pub fn autosave() {
    let mut lbl = label(b"Number of changes between saves:", (7, 31, 94, 16), DT_WORDBREAK);
    lbl.id = ID_AS_LABEL as u16;
    let tpl = template(dialog_frame(b"Autosave", DLG_AUTOSAVE, 165, 53), alloc::vec![
        item(WC_BUTTON, b"~Autosave on", WS_VISIBLE | WS_TABSTOP | WS_GROUP | BS_AUTOCHECKBOX, (9, 18, 84, 10), ID_AS_ON as u16),
        button(b"~Set", (6, 2, 38), ID_AS_SET, WS_GROUP | BS_DEFAULT),
        button(b"Cancel", (60, 2, 38), ID_AS_CANCEL, 0),
        help_button(118, 2, 38),
        lbl,
        item(WC_SPINBUTTON, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | SPBS_MASTER | SPBS_NUMERICONLY | SPBS_JUSTRIGHT, (108, 33, 49, 12), ID_AS_SPIN as u16),
    ]);
    run_dialog(app().frame, autosave_proc, &tpl, 0);
}

// ---- Find (modeless) ----

const ID_FIND_TEXT: u32 = 0x6005;
const ID_CHANGE_TEXT: u32 = 0x6006;
const ID_CASE: u32 = 0x6050;
const ID_WRAPSEARCH: u32 = 0x6051;
const ID_FIND_NEXT: u32 = DID_OK;
const ID_CHANGE: u32 = 0x6001;
const ID_CHANGE_FIND: u32 = 0x6003;
const ID_CHANGE_ALL: u32 = 0x6004;

extern "C" fn find_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            unsafe {
                WinSendDlgItemMsg(h, ID_FIND_TEXT, EM_SETTEXTLIMIT, 260, 0);
                WinSendDlgItemMsg(h, ID_CHANGE_TEXT, EM_SETTEXTLIMIT, 260, 0);
            }
            set_item_text(h, ID_FIND_TEXT, &a.find_text);
            set_item_text(h, ID_CHANGE_TEXT, &a.change_text);
            set_check(h, ID_CASE, a.find_case);
            set_check(h, ID_WRAPSEARCH, a.find_wrap);
            find_buttons(h, false);
            0
        }
        // Back to the dialog: its text selected for typing over.
        WM_ACTIVATE if mp1 & 0xffff != 0 => {
            let n = unsafe { WinQueryWindowTextLength(WinWindowFromID(h, ID_FIND_TEXT)) } as u32;
            unsafe { WinSendDlgItemMsg(h, ID_FIND_TEXT, EM_SETSEL, mp(0, n), 0) };
            unsafe { WinDefDlgProc(h, m, mp1, mp2) }
        }
        WM_CONTROL => {
            match (mp1 & 0xffff, mp1 >> 16) {
                (ID_FIND_TEXT, EN_CHANGE) => {
                    a.find_text = item_text(h, ID_FIND_TEXT);
                    find_buttons(h, false);
                }
                (ID_CHANGE_TEXT, EN_CHANGE) => a.change_text = item_text(h, ID_CHANGE_TEXT),
                (ID_CASE, BN_CLICKED) => {
                    a.find_case = checked(h, ID_CASE);
                    save_int(b"BCASESENSEARCH\0", a.find_case as i32);
                }
                (ID_WRAPSEARCH, BN_CLICKED) => {
                    a.find_wrap = checked(h, ID_WRAPSEARCH);
                    save_int(b"BWRAPSEARCH\0", a.find_wrap as i32);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                ID_FIND_NEXT => {
                    let ok = find_next();
                    after_search(h, ok);
                }
                ID_CHANGE => {
                    let ok = change();
                    after_search(h, ok);
                }
                ID_CHANGE_FIND => {
                    let ok = change() && find_next();
                    after_search(h, ok);
                }
                ID_CHANGE_ALL => {
                    let ok = change_all();
                    after_search(h, ok);
                }
                DID_CANCEL => close_find(h),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            close_find(h);
            0
        }
        WM_HELP => {
            help_panel(PANEL_FIND);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn close_find(h: u32) {
    let a = app();
    unsafe { WinDestroyWindow(h) };
    a.find = 0;
    unsafe { WinSetFocus(HWND_DESKTOP, a.mle) };
}

/// Find is there with text to find; Change, Change then find and Change
/// all once something was found.
fn find_buttons(h: u32, found: bool) {
    let any = !app().find_text.is_empty();
    enable(h, ID_FIND_NEXT, any);
    for id in [ID_CHANGE, ID_CHANGE_FIND, ID_CHANGE_ALL] {
        enable(h, id, any && found);
    }
}

/// After a search: the change buttons follow; a message when nothing
/// was found.
fn after_search(h: u32, found: bool) {
    find_buttons(h, found);
    if !found {
        message(h, b"The text in the \"Find:\" field was not found.", b"Find Notification", MB_OK | MB_INFORMATION);
    }
}

fn case_flag() -> u32 {
    if app().find_case { MLFSEARCH_CASESENSITIVE } else { 0 }
}

/// MLE_SEARCHDATA: cb, pchFind, pchReplace, cchFind, cchReplace,
/// iptStart, iptStop, cchFound.
pub fn search(find: &[u8], repl: &[u8], start: i32, stop: i32, fl: u32) -> Option<(u32, u32)> {
    let mut sd = [0u8; 28];
    sd[0..2].copy_from_slice(&28u16.to_le_bytes());
    sd[4..8].copy_from_slice(&(find.as_ptr() as u32).to_le_bytes());
    sd[8..12].copy_from_slice(&(repl.as_ptr() as u32).to_le_bytes());
    sd[12..14].copy_from_slice(&(find.len() as u16).to_le_bytes());
    sd[14..16].copy_from_slice(&(repl.len() as u16).to_le_bytes());
    sd[16..20].copy_from_slice(&start.to_le_bytes());
    sd[20..24].copy_from_slice(&stop.to_le_bytes());
    let found = send(app().mle, MLM_SEARCH, fl, sd.as_mut_ptr() as u32) != 0;
    found.then(|| (rd32(&sd, 16), u16::from_le_bytes([sd[24], sd[25]]) as u32))
}

/// The next match after the selection (from the top again with Wrap),
/// selected.
pub fn find_next() -> bool {
    let a = app();
    let find = a.find_text.clone();
    if find.is_empty() {
        return false;
    }
    let fl = MLFSEARCH_SELECTMATCH | case_flag();
    let from = send(a.mle, MLM_QUERYSEL, MLFQS_MAXSEL, 0) as i32;
    search(&find, b"", from, -1, fl).is_some() || (a.find_wrap && from > 0 && search(&find, b"", 0, -1, fl).is_some())
}

/// Replace the selection with the change text if it is the text looked
/// for.
fn change() -> bool {
    let a = app();
    let (find, repl) = (a.find_text.clone(), a.change_text.clone());
    let (lo, hi) = (send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0), send(a.mle, MLM_QUERYSEL, MLFQS_MAXSEL, 0));
    let n = hi.saturating_sub(lo) as usize;
    let mut sel = alloc::vec![0u8; n + 1];
    let got = if n > 0 { send(a.mle, MLM_QUERYSELTEXT, sel.as_mut_ptr() as u32, 0) as usize } else { 0 };
    sel.truncate(got.min(n));
    let same = if a.find_case { sel == find } else { sel.eq_ignore_ascii_case(&find) };
    if !same || find.is_empty() {
        return false;
    }
    let r = z(&repl);
    send(a.mle, MLM_INSERT, r.as_ptr() as u32, 0);
    send(a.mle, MLM_SETSEL, lo, lo + repl.len() as u32);
    true
}

/// Change every match from the cursor on (and before it with Wrap); the
/// cursor stays.
fn change_all() -> bool {
    let a = app();
    let (find, repl) = (a.find_text.clone(), a.change_text.clone());
    if find.is_empty() {
        return false;
    }
    let at = send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0) as i32;
    let fl = MLFSEARCH_CHANGEALL | case_flag();
    let mut any = search(&find, &repl, at, -1, fl).is_some();
    if a.find_wrap && at > 0 {
        any |= search(&find, &repl, 0, at, fl).is_some();
    }
    send(a.mle, MLM_SETSEL, at as u32, at as u32);
    any
}

/// Edit > Find: the modeless Find dialog (brought forward if open).
pub fn find() {
    let a = app();
    if a.find != 0 {
        unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(a.find, ID_FIND_TEXT)) };
        return;
    }
    let ef = |id: u32, y: i16| item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | ES_MARGIN | ES_AUTOSCROLL, (53, y, 134, 8), id as u16);
    let tpl = template(dialog_frame(b"Find", DLG_FIND, 196, 73), alloc::vec![
        label(b"~Find:", (2, 60, 48, 8), DT_MNEMONIC),
        ef(ID_FIND_TEXT, 60),
        label(b"Change ~to:", (2, 46, 48, 8), DT_MNEMONIC),
        ef(ID_CHANGE_TEXT, 46),
        item(WC_BUTTON, b"Case ~sensitive", WS_VISIBLE | WS_TABSTOP | WS_GROUP | BS_AUTOCHECKBOX, (26, 32, 80, 10), ID_CASE as u16),
        item(WC_BUTTON, b"~Wrap", WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX, (129, 32, 40, 10), ID_WRAPSEARCH as u16),
        button(b"~Find", (3, 17, 44), ID_FIND_NEXT, WS_GROUP | BS_DEFAULT),
        button(b"C~hange, then find", (52, 17, 88), ID_CHANGE_FIND, 0),
        button(b"~Change", (145, 17, 44), ID_CHANGE, 0),
        button(b"Change ~all", (3, 2, 56), ID_CHANGE_ALL, 0),
        button(b"Cancel", (81, 2, 44), DID_CANCEL, 0),
        help_button(151, 2, 38),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, a.frame, find_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return;
    }
    a.find = dlg;
    unsafe {
        WinSetWindowPos(dlg, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, WinWindowFromID(dlg, ID_FIND_TEXT));
    }
}

// ---- Set Colors ----

/// The colours offered, by name (sorted), as CLR_* values; "Default" is
/// black on white.
const COLORS: [(&[u8], i32); 17] = [
    (b"Black", -1),
    (b"Blue", 1),
    (b"Brown", 14),
    (b"Cyan", 5),
    (b"Dark blue", 9),
    (b"Dark cyan", 13),
    (b"Dark gray", 8),
    (b"Dark green", 12),
    (b"Dark pink", 11),
    (b"Dark red", 10),
    (b"Default", i32::MIN),
    (b"Green", 4),
    (b"Pale gray", 15),
    (b"Pink", 3),
    (b"Red", 2),
    (b"White", -2),
    (b"Yellow", 6),
];

const ID_FG: u32 = 0x4001;
const ID_BG: u32 = 0x4002;
const ID_SAMPLE_SEL: u32 = 0x4003;
const ID_SAMPLE: u32 = 0x4004;
const ID_APPLY: u32 = 0x4006;
const SAMPLE_CLASS: &[u8] = b"EHXColorSample\0";

/// The colours being chosen, and whether Apply set them already.
static mut CHOICE: (i32, i32, bool) = (CLR_BLACK, CLR_WHITE, false);

/// A colour as the MLE draws it (Default: black text on white).
fn real(c: i32, fg: bool) -> i32 {
    match c {
        i32::MIN | CLR_NEUTRAL if fg => CLR_BLACK,
        i32::MIN | CLR_BACKGROUND if !fg => CLR_WHITE,
        c => c,
    }
}

extern "C" fn sample_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m == WM_PAINT {
        let (fg, bg, _) = unsafe { CHOICE };
        let (fg, bg) = (real(fg, true), real(bg, false));
        let selected = unsafe { WinQueryWindowULong(h, QWL_USER) } != 0;
        let (fg, bg) = if selected { (bg, fg) } else { (fg, bg) };
        let mut r = RectL::default();
        unsafe {
            let hps = WinBeginPaint(h, 0, core::ptr::null_mut());
            WinQueryWindowRect(h, &mut r);
            WinFillRect(hps, &r, bg);
            let t: &[u8] = if selected { b"This is selected text." } else { b"This is text." };
            WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut r, fg, bg, DT_LEFT | DT_VCENTER);
            WinEndPaint(hps);
        }
        return 0;
    }
    unsafe { WinDefWindowProc(h, m, mp1, mp2) }
}

fn color_index(c: i32, fg: bool) -> u32 {
    let c = if (fg && c == CLR_NEUTRAL) || (!fg && c == CLR_BACKGROUND) { i32::MIN } else { c };
    COLORS.iter().position(|&(_, v)| v == c).unwrap_or(10) as u32
}

fn selected_color(h: u32, id: u32) -> i32 {
    let i = unsafe { WinSendDlgItemMsg(h, id, LM_QUERYSELECTION, LIT_FIRST, 0) } as i32;
    let fg = id == ID_FG;
    match COLORS.get(i.max(0) as usize).map(|c| c.1) {
        Some(i32::MIN) | None => {
            if fg {
                CLR_NEUTRAL
            } else {
                CLR_BACKGROUND
            }
        }
        Some(c) => c,
    }
}

fn apply_colors() {
    let a = app();
    let (fg, bg, _) = unsafe { CHOICE };
    (a.fg, a.bg) = (fg, bg);
    send(a.mle, MLM_SETBACKCOLOR, bg as u32, 0);
    send(a.mle, MLM_SETTEXTCOLOR, fg as u32, 0);
    save_int(b"ULFGNDCOLOR\0", fg);
    save_int(b"ULBGNDCOLOR\0", bg);
    trace(alloc::format!("colors {fg} {bg}").as_bytes());
}

/// Set (or, after Apply, Cancel) is the default button.
fn colors_default(h: u32, applied: bool) {
    let (on, off) = if applied { (DID_CANCEL, DID_OK) } else { (DID_OK, DID_CANCEL) };
    unsafe {
        WinSetWindowULong(WinWindowFromID(h, off), -2, WinQueryWindowULong(WinWindowFromID(h, off), -2) & !BS_DEFAULT);
        WinSetWindowULong(WinWindowFromID(h, on), -2, WinQueryWindowULong(WinWindowFromID(h, on), -2) | BS_DEFAULT);
        WinInvalidateRect(WinWindowFromID(h, on), core::ptr::null(), 0);
        WinInvalidateRect(WinWindowFromID(h, off), core::ptr::null(), 0);
    }
}

extern "C" fn colors_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            unsafe { CHOICE = (a.fg, a.bg, false) };
            for id in [ID_FG, ID_BG] {
                for (name, _) in COLORS {
                    let s = z(name);
                    unsafe { WinSendDlgItemMsg(h, id, LM_INSERTITEM, LIT_END, s.as_ptr() as u32) };
                }
            }
            unsafe {
                WinSendDlgItemMsg(h, ID_FG, LM_SELECTITEM, color_index(a.fg, true), 1);
                WinSendDlgItemMsg(h, ID_BG, LM_SELECTITEM, color_index(a.bg, false), 1);
                // The samples (normal and selected text) in place of the
                // template's placeholders.
                WinRegisterClass(a.hab, SAMPLE_CLASS.as_ptr(), sample_proc, 0, 4);
                for (id, selected) in [(ID_SAMPLE, 0), (ID_SAMPLE_SEL, 1)] {
                    let p = window_pos(WinWindowFromID(h, id));
                    WinDestroyWindow(WinWindowFromID(h, id));
                    let w = WinCreateWindow(h, SAMPLE_CLASS.as_ptr() as u32, b"\0".as_ptr(), WS_VISIBLE, p.x, p.y, p.cx, p.cy, h, HWND_TOP, id, core::ptr::null(), core::ptr::null());
                    WinSetWindowULong(w, QWL_USER, selected);
                }
                WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_FG));
            }
            1
        }
        WM_CONTROL if (mp1 & 0xffff == ID_FG || mp1 & 0xffff == ID_BG) && mp1 >> 16 == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_CONTROL if (mp1 & 0xffff == ID_FG || mp1 & 0xffff == ID_BG) && mp1 >> 16 == LN_SELECT => {
            unsafe {
                let applied = CHOICE.2;
                CHOICE = (selected_color(h, ID_FG), selected_color(h, ID_BG), false);
                if applied {
                    colors_default(h, false);
                }
                for id in [ID_SAMPLE, ID_SAMPLE_SEL] {
                    WinInvalidateRect(WinWindowFromID(h, id), core::ptr::null(), 0);
                }
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                DID_OK => {
                    apply_colors();
                    unsafe { WinDismissDlg(h, DID_OK) };
                }
                // Apply: the colours now; Cancel becomes the default.
                ID_APPLY => {
                    apply_colors();
                    unsafe { CHOICE.2 = true };
                    colors_default(h, true);
                }
                _ => {
                    unsafe { WinDismissDlg(h, DID_CANCEL) };
                }
            }
            0
        }
        WM_HELP => {
            help_panel(PANEL_COLORS);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Options > Set colors: text and background colours (Set, Apply).
pub fn colors() {
    let lb = |id: u32, x: i16| item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, 52, 85, 40), id as u16);
    let tpl = template(dialog_frame(b"Set Colors", DLG_COLORS, 200, 107), alloc::vec![
        item(WC_STATIC, b"Foreground", WS_VISIBLE | SS_GROUPBOX, (4, 49, 93, 52), 0xffff),
        lb(ID_FG, 8),
        item(WC_STATIC, b"Background", WS_VISIBLE | SS_GROUPBOX, (102, 49, 93, 52), 0xffff),
        lb(ID_BG, 106),
        item(WC_STATIC, b"Color sample", WS_VISIBLE | SS_GROUPBOX, (3, 16, 194, 33), 0xffff),
        item(WC_STATIC, b"", 0x0007, (8, 31, 179, 8), ID_SAMPLE as u16),
        item(WC_STATIC, b"", 0x0007, (8, 22, 179, 8), ID_SAMPLE_SEL as u16),
        button(b"~Set", (5, 2, 38), DID_OK, WS_GROUP | BS_DEFAULT),
        button(b"~Apply", (56, 2, 38), ID_APPLY, 0),
        button(b"Cancel", (103, 2, 38), DID_CANCEL, 0),
        help_button(152, 2, 38),
    ]);
    run_dialog(app().frame, colors_proc, &tpl, 0);
}
