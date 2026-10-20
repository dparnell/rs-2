//! The editor's dialogs: the standard file and font dialogs (PMCTLS),
//! and its own, built from templates in memory: Save notification (no
//! file type), Type, File Changed, Autosave, Find (modeless), Set Colors
//! and Product information. Help buttons show the dialog's help panel.

use alloc::vec::Vec;

use crate::pm::*;
use crate::*;

// ---- Simple button dialogs ----

/// A dialog whose buttons dismiss it with their ids. QWL_USER holds its
/// help panel; the focus starts on the button `mp2 >> 16` if given.
extern "C" fn buttons_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, mp2 & 0xffff) };
            place_over(h, app().frame);
            let focus = mp2 >> 16;
            if focus != 0 {
                unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, focus)) };
                return 1;
            }
            0
        }
        WM_COMMAND => {
            unsafe { WinDismissDlg(h, mp1 & 0xffff) };
            0
        }
        WM_HELP => {
            help_panel(unsafe { WinQueryWindowULong(h, QWL_USER) } as u16);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

const QWL_USER: i32 = 0;

const ID_SAVE: u32 = 10;
const ID_SAVEAS: u32 = 11;
const ID_DISCARD: u32 = 12;
const ID_TYPE: u32 = 13;
const ID_HELP: u32 = 14;

pub enum Changed {
    Save,
    SaveAs,
    Discard,
    Cancel,
}

/// "Warning: File Changed": NAME has changed. Do you want to save it?
pub fn file_changed() -> Changed {
    let name = app().path.clone().unwrap_or_else(|| UNTITLED.to_vec());
    let mut text = name;
    text.extend_from_slice(b" has changed. Do you want to save it?");
    let tpl = template(dialog_frame(b"Warning: File Changed", 232, 62), alloc::vec![
        item(WC_STATIC, b"#14", WS_VISIBLE | SS_SYSICON, (10, 30, 20, 20), 0xffff),
        label(&text, (40, 26, 186, 30), DT_CENTER | DT_VCENTER | DT_WORDBREAK),
        button(b"~Save", (4, 4, 40), ID_SAVE, WS_GROUP),
        button(b"Save ~as...", (48, 4, 50), ID_SAVEAS, 0),
        button(b"~Discard", (102, 4, 40), ID_DISCARD, 0),
        button(b"Cancel", (146, 4, 40), DID_CANCEL, BS_DEFAULT),
        button(b"Help", (190, 4, 38), ID_HELP, BS_HELP),
    ]);
    match run_dialog(app().frame, buttons_proc, &tpl, mp(PANEL_CHANGED as u32, DID_CANCEL)) {
        ID_SAVE => Changed::Save,
        ID_SAVEAS => Changed::SaveAs,
        ID_DISCARD => Changed::Discard,
        _ => Changed::Cancel,
    }
}

pub enum NoType {
    Type,
    SaveAs,
    Cancel,
}

/// "Save notification": the file has no type.
pub fn no_type() -> NoType {
    let tpl = template(dialog_frame(b"Save notification", 244, 56), alloc::vec![
        label(b"There is no file type associated with this file.", (8, 40, 230, 8), 0),
        label(b"Select the Type button to add one.", (8, 26, 230, 8), 0),
        button(b"~Type...", (6, 4, 54), ID_TYPE, WS_GROUP | BS_DEFAULT),
        button(b"~Save as...", (66, 4, 54), ID_SAVEAS, 0),
        button(b"Cancel", (126, 4, 54), DID_CANCEL, 0),
        button(b"Help", (186, 4, 54), ID_HELP, BS_HELP),
    ]);
    match run_dialog(app().frame, buttons_proc, &tpl, PANEL_NOTYPE as u32) {
        ID_TYPE => NoType::Type,
        ID_SAVEAS => NoType::SaveAs,
        _ => NoType::Cancel,
    }
}

const ID_TYPELIST: u32 = 20;

extern "C" fn type_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            place_over(h, app().frame);
            for t in TYPES {
                let s = z(t);
                unsafe { WinSendDlgItemMsg(h, ID_TYPELIST, LM_INSERTITEM, LIT_END, s.as_ptr() as u32) };
            }
            unsafe { WinSendDlgItemMsg(h, ID_TYPELIST, LM_SELECTITEM, 0, 1) };
            0
        }
        WM_CONTROL if mp1 & 0xffff == ID_TYPELIST && mp1 >> 16 == LN_ENTER => {
            // Enter or a double click: Set.
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            let mut r = mp1 & 0xffff;
            if r == DID_OK {
                let sel = unsafe { WinSendDlgItemMsg(h, ID_TYPELIST, LM_QUERYSELECTION, LIT_FIRST, 0) } as i32;
                r = if sel >= 0 { 100 + sel as u32 } else { DID_CANCEL };
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

/// The Type dialog: the index in TYPES chosen with Set.
pub fn choose_type() -> Option<usize> {
    let tpl = template(dialog_frame(b"Type", 146, 74), alloc::vec![
        item(WC_STATIC, b"", WS_VISIBLE | SS_GROUPBOX, (4, 22, 138, 50), 0xffff),
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (10, 26, 126, 40), ID_TYPELIST as u16),
        button(b"~Set", (4, 4, 42), DID_OK, WS_GROUP | BS_DEFAULT),
        button(b"Cancel", (52, 4, 42), DID_CANCEL, 0),
        button(b"Help", (100, 4, 42), ID_HELP, BS_HELP),
    ]);
    let r = run_dialog(app().frame, type_proc, &tpl, 0);
    (100..100 + TYPES.len() as u32).contains(&r).then(|| (r - 100) as usize)
}

/// Product information.
pub fn product_info() {
    let tpl = template(dialog_frame(b"System Editor - Product information", 220, 76), alloc::vec![
        label(b"rs2 System Editor", (40, 58, 176, 8), 0),
        label(b"Version 1.0", (40, 48, 176, 8), 0),
        label(b"A re-implementation of the OS/2 System Editor", (40, 38, 176, 8), 0),
        label(b"for rs2. MIT or Apache-2.0 licence.", (40, 28, 176, 8), 0),
        item(WC_STATIC, b"#10", WS_VISIBLE | SS_SYSICON, (10, 40, 20, 20), 0xffff),
        button(b"OK", (88, 4, 44), DID_OK, WS_GROUP | BS_DEFAULT),
    ]);
    run_dialog(app().frame, buttons_proc, &tpl, 0);
}

// ---- File and font dialogs ----

const FILEDLG_SIZE: usize = 328;
const FDS_CENTER: u32 = 0x0001;
const FDS_OPEN_DIALOG: u32 = 0x0100;
const FDS_SAVEAS_DIALOG: u32 = 0x0200;

fn wr32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn rd32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// The standard file dialog; (file, EA type index) when OK was pressed.
fn run_file_dialog(save: bool) -> Option<(Vec<u8>, Option<usize>)> {
    let a = app();
    let mut fd = alloc::vec![0u8; FILEDLG_SIZE];
    wr32(&mut fd, 0, FILEDLG_SIZE as u32);
    wr32(&mut fd, 4, FDS_CENTER | if save { FDS_SAVEAS_DIALOG } else { FDS_OPEN_DIALOG });
    let title: &[u8] = if save { b"Save as\0" } else { b"Open\0" };
    wr32(&mut fd, 20, title.as_ptr() as u32);
    // Save as offers the file types.
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
    // sEAType: none.
    fd[326..328].copy_from_slice(&(-1i16).to_le_bytes());
    let ok = unsafe { WinFileDlg(HWND_DESKTOP, a.frame, fd.as_mut_ptr()) } != 0;
    if !ok || rd32(&fd, 12) != DID_OK {
        return None;
    }
    let name = until_nul(&fd[52..312]).to_vec();
    if name.is_empty() {
        return None;
    }
    let ty = i16::from_le_bytes([fd[326], fd[327]]);
    Some((name, (save && ty >= 0 && (ty as usize) < TYPES.len()).then_some(ty as usize)))
}

/// File > Open's dialog: the file chosen.
pub fn file_dialog() -> Option<Vec<u8>> {
    run_file_dialog(false).map(|(n, _)| n)
}

/// File > Save as's dialog: the file and the type chosen.
pub fn save_as_dialog() -> Option<(Vec<u8>, Option<usize>)> {
    run_file_dialog(true)
}

const FONTDLG_SIZE: usize = 168;
const FNTS_CENTER: u32 = 0x0001;
const FNTS_INITFROMFATTRS: u32 = 0x0080;

/// Options > Set font: the font dialog, then the MLE's font.
pub fn font() {
    let a = app();
    let mut fd = alloc::vec![0u8; FONTDLG_SIZE];
    let mut family = [0u8; 64];
    let face = until_nul(&a.fattrs[8..40]);
    family[..face.len()].copy_from_slice(face);
    wr32(&mut fd, 0, FONTDLG_SIZE as u32);
    wr32(&mut fd, 12, b"Font\0".as_ptr() as u32);
    wr32(&mut fd, 28, family.as_mut_ptr() as u32);
    wr32(&mut fd, 32, a.points << 16);
    wr32(&mut fd, 36, FNTS_CENTER | FNTS_INITFROMFATTRS);
    wr32(&mut fd, 60, a.fg as u32);
    wr32(&mut fd, 64, a.bg as u32);
    fd[96..152].copy_from_slice(&a.fattrs);
    fd[164..166].copy_from_slice(&(family.len() as u16).to_le_bytes());
    let ok = unsafe { WinFontDlg(HWND_DESKTOP, a.frame, fd.as_mut_ptr()) } != 0;
    if !ok || rd32(&fd, 72) != DID_OK {
        return;
    }
    a.fattrs.copy_from_slice(&fd[96..152]);
    a.points = (rd32(&fd, 32) >> 16).max(1);
    send(a.mle, MLM_SETFONT, a.fattrs.as_ptr() as u32, 0);
    trace(alloc::format!("font {} {}", txt(until_nul(&a.fattrs[8..40])), a.points).as_bytes());
    save_settings();
}

// ---- Autosave ----

const ID_SPIN: u32 = 30;
const ID_AUTOON: u32 = 31;
const ID_SPINLABEL: u32 = 32;

extern "C" fn autosave_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            unsafe {
                WinSendDlgItemMsg(h, ID_SPIN, SPBM_SETLIMITS, 9999, 1);
                WinSendDlgItemMsg(h, ID_SPIN, SPBM_SETCURRENTVALUE, a.autosave_every, 0);
            }
            set_check(h, ID_AUTOON, a.autosave);
            enable_spin(h, a.autosave);
            // The focus starts on the check box (the spin button may be
            // disabled).
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_AUTOON)) };
            1
        }
        WM_CONTROL if mp1 & 0xffff == ID_AUTOON && mp1 >> 16 == BN_CLICKED => {
            enable_spin(h, checked(h, ID_AUTOON));
            0
        }
        WM_COMMAND => {
            if mp1 & 0xffff == DID_OK {
                let mut v = 0u32;
                unsafe { WinSendDlgItemMsg(h, ID_SPIN, SPBM_QUERYVALUE, &mut v as *mut u32 as u32, mp(0, SPBQ_ALWAYSUPDATE)) };
                a.autosave_every = v.clamp(1, 9999);
                a.autosave = checked(h, ID_AUTOON);
                a.changes = 0;
                save_settings();
                trace(alloc::format!("autosave {} every {}", a.autosave as u32, a.autosave_every).as_bytes());
            }
            unsafe { WinDismissDlg(h, mp1 & 0xffff) };
            0
        }
        WM_HELP => {
            help_panel(PANEL_AUTOSAVE);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn enable_spin(h: u32, on: bool) {
    enable(h, ID_SPIN, on);
    enable(h, ID_SPINLABEL, on);
}

/// File > Autosave: number of changes between saves, on or off.
pub fn autosave() {
    let mut spin = item(WC_SPINBUTTON, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | SPBS_MASTER | SPBS_NUMERICONLY | SPBS_JUSTRIGHT, (104, 36, 54, 12), ID_SPIN as u16);
    spin.ctl = Vec::new();
    let mut lbl = label(b"Number of changes between saves:", (6, 32, 94, 20), DT_WORDBREAK);
    lbl.id = ID_SPINLABEL as u16;
    let tpl = template(dialog_frame(b"Autosave", 164, 56), alloc::vec![
        lbl,
        spin,
        item(WC_BUTTON, b"~Autosave on", WS_VISIBLE | WS_TABSTOP | WS_GROUP | BS_AUTOCHECKBOX, (8, 20, 90, 10), ID_AUTOON as u16),
        button(b"~Set", (4, 4, 46), DID_OK, WS_GROUP | BS_DEFAULT),
        button(b"Cancel", (58, 4, 46), DID_CANCEL, 0),
        button(b"Help", (112, 4, 46), ID_HELP, BS_HELP),
    ]);
    run_dialog(app().frame, autosave_proc, &tpl, 0);
}

// ---- Find (modeless) ----

const ID_FINDTEXT: u32 = 40;
const ID_CHANGETEXT: u32 = 41;
const ID_CASE: u32 = 42;
const ID_WRAPSEARCH: u32 = 43;
const ID_FINDNEXT: u32 = 44;
const ID_CHANGEFIND: u32 = 45;
const ID_CHANGE: u32 = 46;
const ID_CHANGEALL: u32 = 47;

extern "C" fn find_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            unsafe {
                WinSendDlgItemMsg(h, ID_FINDTEXT, EM_SETTEXTLIMIT, 255, 0);
                WinSendDlgItemMsg(h, ID_CHANGETEXT, EM_SETTEXTLIMIT, 255, 0);
            }
            set_check(h, ID_CASE, true);
            update_find_buttons(h);
            0
        }
        WM_CONTROL if mp1 & 0xffff == ID_FINDTEXT && mp1 >> 16 == EN_CHANGE => {
            update_find_buttons(h);
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                ID_FINDNEXT => {
                    find_next(h);
                }
                ID_CHANGE => {
                    change(h);
                }
                ID_CHANGEFIND => {
                    if change(h) {
                        find_next(h);
                    }
                }
                ID_CHANGEALL => change_all(h),
                _ => {
                    unsafe { WinDestroyWindow(h) };
                    a.find = 0;
                    unsafe { WinSetFocus(HWND_DESKTOP, a.mle) };
                }
            }
            0
        }
        WM_CLOSE => {
            unsafe { WinDestroyWindow(h) };
            a.find = 0;
            0
        }
        WM_HELP => {
            help_panel(PANEL_FIND);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn update_find_buttons(h: u32) {
    let on = !item_text(h, ID_FINDTEXT).is_empty();
    for id in [ID_FINDNEXT, ID_CHANGEFIND, ID_CHANGE, ID_CHANGEALL] {
        enable(h, id, on);
    }
}

/// MLE_SEARCHDATA: cb, pchFind, pchReplace, cchFind, cchReplace,
/// iptStart, iptStop, cchFound.
fn search(find: &[u8], repl: &[u8], start: i32, stop: i32, fl: u32) -> Option<(u32, u32)> {
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

fn case_flag(h: u32) -> u32 {
    if checked(h, ID_CASE) { MLFSEARCH_CASESENSITIVE } else { 0 }
}

/// Find the next match after the selection (from the top again with
/// Wrap). False (and a message) when there is none.
pub fn find_next(h: u32) -> bool {
    let a = app();
    let find = item_text(h, ID_FINDTEXT);
    if find.is_empty() {
        return false;
    }
    let fl = MLFSEARCH_SELECTMATCH | case_flag(h);
    let from = send(a.mle, MLM_QUERYSEL, MLFQS_MAXSEL, 0) as i32;
    if search(&find, b"", from, -1, fl).is_some() {
        return true;
    }
    if checked(h, ID_WRAPSEARCH) && from > 0 && search(&find, b"", 0, -1, fl).is_some() {
        return true;
    }
    let t = alloc::format!("\"{}\" was not found.", txt(&find));
    message(h, t.as_bytes(), b"Find", MB_OK | MB_INFORMATION);
    false
}

/// Change the selection if it is the text looked for.
fn change(h: u32) -> bool {
    let a = app();
    let find = item_text(h, ID_FINDTEXT);
    let repl = item_text(h, ID_CHANGETEXT);
    let (lo, hi) = (send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0), send(a.mle, MLM_QUERYSEL, MLFQS_MAXSEL, 0));
    let n = (hi - lo) as usize;
    let mut sel = alloc::vec![0u8; n + 1];
    let got = if n > 0 { send(a.mle, MLM_QUERYSELTEXT, sel.as_mut_ptr() as u32, 0) as usize } else { 0 };
    sel.truncate(got.min(n));
    let same = if case_flag(h) != 0 { sel == find } else { sel.eq_ignore_ascii_case(&find) };
    if !same || find.is_empty() {
        // Not on a match: find one first.
        find_next(h);
        return false;
    }
    let r = z(&repl);
    send(a.mle, MLM_INSERT, r.as_ptr() as u32, 0);
    true
}

fn change_all(h: u32) {
    let find = item_text(h, ID_FINDTEXT);
    let repl = item_text(h, ID_CHANGETEXT);
    if find.is_empty() {
        return;
    }
    if search(&find, &repl, 0, -1, MLFSEARCH_CHANGEALL | case_flag(h)).is_none() {
        let t = alloc::format!("\"{}\" was not found.", txt(&find));
        message(h, t.as_bytes(), b"Find", MB_OK | MB_INFORMATION);
    }
}

/// Edit > Find: the modeless Find dialog (brought forward if open).
pub fn find() {
    let a = app();
    if a.find != 0 {
        unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(a.find, ID_FINDTEXT)) };
        return;
    }
    let ef = |id: u32, y: i16| item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | ES_MARGIN | ES_AUTOSCROLL, (54, y, 136, 8), id as u16);
    let tpl = template(dialog_frame(b"Find", 196, 70), alloc::vec![
        label(b"~Find:", (4, 56, 46, 8), DT_MNEMONIC),
        ef(ID_FINDTEXT, 56),
        label(b"C~hange to:", (4, 42, 46, 8), DT_MNEMONIC),
        ef(ID_CHANGETEXT, 42),
        item(WC_BUTTON, b"Case ~sensitive", WS_VISIBLE | WS_TABSTOP | WS_GROUP | BS_AUTOCHECKBOX, (28, 30, 80, 10), ID_CASE as u16),
        item(WC_BUTTON, b"~Wrap", WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX, (130, 30, 50, 10), ID_WRAPSEARCH as u16),
        button(b"~Find", (4, 17, 44), ID_FINDNEXT, WS_GROUP | BS_DEFAULT),
        button(b"Ch~ange, then find", (52, 17, 88), ID_CHANGEFIND, 0),
        button(b"~Change", (144, 17, 44), ID_CHANGE, 0),
        button(b"Change ~all", (4, 2, 54), ID_CHANGEALL, 0),
        button(b"Cancel", (82, 2, 44), DID_CANCEL, 0),
        button(b"Help", (144, 2, 44), ID_HELP, BS_HELP),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, a.frame, find_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return;
    }
    a.find = dlg;
    unsafe {
        WinSetWindowPos(dlg, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, WinWindowFromID(dlg, ID_FINDTEXT));
    }
}

// ---- Set Colors ----

/// The colours (CLR_*) by name, as the dialog lists them.
const COLORS: [(&[u8], i32); 16] = [
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
    (b"Green", 4),
    (b"Pale gray", 15),
    (b"Pink", 3),
    (b"Red", 2),
    (b"White", -2),
    (b"Yellow", 6),
];

const ID_FG: u32 = 50;
const ID_BG: u32 = 51;
const ID_SAMPLEBOX: u32 = 52;
const ID_APPLY: u32 = 53;
const ID_SAMPLE: u32 = 54;
const SAMPLE_CLASS: &[u8] = b"RS2EditorColorSample\0";

/// The colours being chosen.
static mut CHOICE: (i32, i32) = (CLR_BLACK, CLR_WHITE);

extern "C" fn sample_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m == WM_PAINT {
        let (fg, bg) = unsafe { CHOICE };
        let mut r = RectL::default();
        unsafe {
            let hps = WinBeginPaint(h, 0, core::ptr::null_mut());
            WinQueryWindowRect(h, &mut r);
            WinFillRect(hps, &r, bg);
            let t = b"The quick brown fox jumps over the lazy dog";
            WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut r, fg, bg, DT_CENTER | DT_VCENTER);
            WinEndPaint(hps);
        }
        return 0;
    }
    unsafe { WinDefWindowProc(h, m, mp1, mp2) }
}

fn color_index(c: i32) -> u32 {
    COLORS.iter().position(|&(_, v)| v == c).unwrap_or(0) as u32
}

fn selected_color(h: u32, id: u32) -> i32 {
    let i = unsafe { WinSendDlgItemMsg(h, id, LM_QUERYSELECTION, LIT_FIRST, 0) } as i32;
    COLORS.get(i.max(0) as usize).map_or(CLR_BLACK, |c| c.1)
}

fn apply_colors() {
    let a = app();
    (a.fg, a.bg) = unsafe { CHOICE };
    send(a.mle, MLM_SETTEXTCOLOR, a.fg as u32, 0);
    send(a.mle, MLM_SETBACKCOLOR, a.bg as u32, 0);
    save_settings();
    trace(alloc::format!("colors {} {}", a.fg, a.bg).as_bytes());
}

extern "C" fn colors_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            place_over(h, a.frame);
            unsafe { CHOICE = (a.fg, a.bg) };
            for id in [ID_FG, ID_BG] {
                for (name, _) in COLORS {
                    let s = z(name);
                    unsafe { WinSendDlgItemMsg(h, id, LM_INSERTITEM, LIT_END, s.as_ptr() as u32) };
                }
            }
            unsafe {
                WinSendDlgItemMsg(h, ID_FG, LM_SELECTITEM, color_index(a.fg), 1);
                WinSendDlgItemMsg(h, ID_BG, LM_SELECTITEM, color_index(a.bg), 1);
                // The sample inside its group box.
                let b = window_pos(WinWindowFromID(h, ID_SAMPLEBOX));
                WinRegisterClass(a.hab, SAMPLE_CLASS.as_ptr(), sample_proc, 0, 0);
                WinCreateWindow(h, SAMPLE_CLASS.as_ptr() as u32, b"\0".as_ptr(), WS_VISIBLE, b.x + 8, b.y + 6, b.cx - 16, b.cy - 22, h, HWND_TOP, ID_SAMPLE, core::ptr::null(), core::ptr::null());
            }
            0
        }
        WM_CONTROL if (mp1 & 0xffff == ID_FG || mp1 & 0xffff == ID_BG) && mp1 >> 16 == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_CONTROL if (mp1 & 0xffff == ID_FG || mp1 & 0xffff == ID_BG) && mp1 >> 16 == LN_SELECT => {
            unsafe {
                CHOICE = (selected_color(h, ID_FG), selected_color(h, ID_BG));
                WinInvalidateRect(WinWindowFromID(h, ID_SAMPLE), core::ptr::null(), 0);
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                DID_OK => {
                    apply_colors();
                    unsafe { WinDismissDlg(h, DID_OK) };
                }
                ID_APPLY => apply_colors(),
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

/// Options > Set colors: text and background colours.
pub fn colors() {
    let lb = |id: u32, x: i16| item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, 54, 84, 40), id as u16);
    let tpl = template(dialog_frame(b"Set Colors", 200, 104), alloc::vec![
        item(WC_STATIC, b"Foreground", WS_VISIBLE | SS_GROUPBOX, (4, 50, 94, 54), 0xffff),
        lb(ID_FG, 9),
        item(WC_STATIC, b"Background", WS_VISIBLE | SS_GROUPBOX, (102, 50, 94, 54), 0xffff),
        lb(ID_BG, 107),
        item(WC_STATIC, b"Color sample", WS_VISIBLE | SS_GROUPBOX, (4, 22, 192, 28), ID_SAMPLEBOX as u16),
        button(b"~Set", (4, 4, 44), DID_OK, WS_GROUP | BS_DEFAULT),
        button(b"~Apply", (52, 4, 44), ID_APPLY, 0),
        button(b"Cancel", (100, 4, 44), DID_CANCEL, 0),
        button(b"Help", (152, 4, 44), ID_HELP, BS_HELP),
    ]);
    run_dialog(app().frame, colors_proc, &tpl, 0);
}
