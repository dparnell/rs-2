//! The standard file dialog (WinFileDlg): Open and Save as, with a
//! file name field, file type and drive lists, the directories and the
//! files of the current directory. The dialog is built from a template
//! made in memory; its logic is WinDefFileDlgProc, which an application
//! procedure (FILEDLG.pfnDlgProc) passes what it does not handle to.
//! The FILEDLG is the dialog's QWL_USER window word.

use alloc::vec::Vec;

use crate::pm::*;

// FILEDLG offsets.
const F_FL: usize = 4;
const F_RETURN: usize = 12;
const F_SRC: usize = 16;
const F_TITLE: usize = 20;
const F_OKBUTTON: usize = 24;
const F_DLGPROC: usize = 28;
const F_ITYPE: usize = 32;
const F_TYPELIST: usize = 36;
const F_IDRIVE: usize = 40;
const F_FULLFILE: usize = 52;
const F_FQFILENAME: usize = 312;
const F_FQFCOUNT: usize = 316;
const F_X: usize = 322;
const F_Y: usize = 324;
const F_EATYPE: usize = 326;
const CCHMAXPATH: usize = 260;

const FDS_CENTER: u32 = 0x0001;
const FDS_HELPBUTTON: u32 = 0x0008;
const FDS_APPLYBUTTON: u32 = 0x0010;
const FDS_MODELESS: u32 = 0x0040;
const FDS_SAVEAS_DIALOG: u32 = 0x0200;

const FDS_SUCCESSFUL: u32 = 0;
const FDS_ERR_INVALID_DIALOG: u32 = 3;

const FDM_FILTER: u32 = WM_USER + 40;
const FDM_VALIDATE: u32 = WM_USER + 41;
const FDM_ERROR: u32 = WM_USER + 42;

const DID_FILENAME_TXT: u16 = 257;
const DID_FILENAME_ED: u16 = 258;
const DID_DRIVE_TXT: u16 = 259;
const DID_DRIVE_CB: u16 = 260;
const DID_FILTER_TXT: u16 = 261;
const DID_FILTER_CB: u16 = 262;
const DID_DIRECTORY_TXT: u16 = 263;
const DID_DIRECTORY_LB: u16 = 264;
const DID_FILES_TXT: u16 = 265;
const DID_FILES_LB: u16 = 266;
const DID_HELP_PB: u16 = 267;
const DID_APPLY_PB: u16 = 268;
const DID_DIRECTORY_SELECTED: u16 = 270;

/// Per-dialog state (process-private: dialogs run in the caller).
struct State {
    hwnd: u32,
    fild: *mut u8,
    /// Current directory, with its drive (`C:\` or `C:\DIR`).
    dir: Vec<u8>,
    /// File name pattern of the files list.
    pattern: Vec<u8>,
}

static mut STATES: Vec<State> = Vec::new();

fn states() -> &'static mut Vec<State> {
    unsafe { &mut *core::ptr::addr_of_mut!(STATES) }
}

fn state(h: u32) -> Option<&'static mut State> {
    states().iter_mut().find(|s| s.hwnd == h)
}

fn rd32(p: *const u8, off: usize) -> u32 {
    unsafe { (p.add(off) as *const u32).read_unaligned() }
}

fn wr32(p: *mut u8, off: usize, v: u32) {
    unsafe { (p.add(off) as *mut u32).write_unaligned(v) }
}

fn wr16(p: *mut u8, off: usize, v: u16) {
    unsafe { (p.add(off) as *mut u16).write_unaligned(v) }
}

fn upper(t: &[u8]) -> Vec<u8> {
    t.iter().map(|c| c.to_ascii_uppercase()).collect()
}

fn has_wild(t: &[u8]) -> bool {
    t.iter().any(|&c| c == b'*' || c == b'?')
}

/// The current directory of the process (`C:\DIR`).
fn current_dir() -> Vec<u8> {
    let (mut disk, mut map) = (3u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut disk, &mut map) };
    dir_of_drive(disk)
}

/// Current directory of drive `disk` (1 = A:).
fn dir_of_drive(disk: u32) -> Vec<u8> {
    let mut buf = [0u8; CCHMAXPATH];
    let mut cb = buf.len() as u32;
    let mut d = alloc::vec![b'A' + (disk as u8).saturating_sub(1), b':', b'\\'];
    if unsafe { DosQueryCurrentDir(disk, buf.as_mut_ptr(), &mut cb) } == 0 {
        d.extend_from_slice(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(0)]);
    }
    d
}

fn is_dir(path: &[u8]) -> bool {
    // FILESTATUS3: attrFile at offset 24.
    let p = z(path);
    let mut fs = [0u8; 24 + 4];
    unsafe { DosQueryPathInfo(p.as_ptr(), 1, fs.as_mut_ptr(), fs.len() as u32) == 0 && u32::from_le_bytes([fs[24], fs[25], fs[26], fs[27]]) & 0x10 != 0 }
}

fn exists(path: &[u8]) -> bool {
    let p = z(path);
    let mut fs = [0u8; 28];
    unsafe { DosQueryPathInfo(p.as_ptr(), 1, fs.as_mut_ptr(), fs.len() as u32) == 0 }
}

/// `dir` joined with `name` (dir is `C:\` or `C:\DIR`).
fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut v = dir.to_vec();
    if v.last() != Some(&b'\\') {
        v.push(b'\\');
    }
    v.extend_from_slice(name);
    v
}

/// A full path from what was typed: absolute (`C:\X`, `\X`, `C:X`) or
/// relative to `dir`; `.` and `..` resolved.
fn full_path(dir: &[u8], typed: &[u8]) -> Vec<u8> {
    let typed: Vec<u8> = typed.iter().map(|&c| if c == b'/' { b'\\' } else { c }).collect();
    let (drive, rest): (Vec<u8>, Vec<u8>) = if typed.len() >= 2 && typed[1] == b':' {
        let d = typed[..2].to_vec();
        let r = &typed[2..];
        if r.first() == Some(&b'\\') {
            (d, r.to_vec())
        } else {
            let disk = (typed[0].to_ascii_uppercase() - b'A' + 1) as u32;
            let base = if dir.len() >= 2 && dir[..2].eq_ignore_ascii_case(&d) { dir.to_vec() } else { dir_of_drive(disk) };
            (d, join(&base[2..], r))
        }
    } else if typed.first() == Some(&b'\\') {
        (dir[..2].to_vec(), typed.clone())
    } else {
        (dir[..2].to_vec(), join(&dir[2..], &typed))
    };
    let mut parts: Vec<&[u8]> = Vec::new();
    for p in rest.split(|&c| c == b'\\') {
        match p {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let mut out = upper(&drive);
    out.push(b'\\');
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'\\');
        }
        out.extend_from_slice(p);
    }
    out
}

/// Entries of `dir` matching `pattern`: (directories, files), sorted.
fn list(dir: &[u8], pattern: &[u8]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for (spec, want_dirs) in [(join(dir, b"*"), true), (join(dir, pattern), false)] {
        let s = z(&spec);
        let mut h: u32 = 0xffff_ffff;
        let mut buf = [0u8; 1024];
        let mut n: u32 = 32;
        // FILE_DIRECTORY | FILE_READONLY | FILE_ARCHIVED, level 1.
        let attr = if want_dirs { 0x10 | 0x01 | 0x20 } else { 0x01 | 0x20 };
        let mut rc = unsafe { os2::DosFindFirst(s.as_ptr(), &mut h, attr, buf.as_mut_ptr(), buf.len() as u32, &mut n, 1) };
        while rc == 0 && n > 0 {
            let mut off = 0usize;
            for _ in 0..n {
                let e = &buf[off..];
                let next = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
                let attr = u32::from_le_bytes([e[24], e[25], e[26], e[27]]);
                let len = e[28] as usize;
                let name = e[29..29 + len].to_vec();
                if want_dirs {
                    if attr & 0x10 != 0 && name != b"." && name != b".." {
                        dirs.push(name);
                    }
                } else if attr & 0x10 == 0 {
                    files.push(name);
                }
                if next == 0 {
                    break;
                }
                off += next;
            }
            n = 32;
            rc = unsafe { os2::DosFindNext(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        }
        if h != 0xffff_ffff {
            unsafe { os2::DosFindClose(h) };
        }
    }
    let key = |a: &Vec<u8>| upper(a);
    dirs.sort_by_key(key);
    files.sort_by_key(key);
    (dirs, files)
}

/// Fill the directory and file lists and the path text.
fn refresh(dlg: u32) {
    let Some(s) = state(dlg) else { return };
    let (dir, pattern) = (s.dir.clone(), s.pattern.clone());
    let (dirs, files) = list(&dir, &pattern);
    list_clear(dlg, DID_DIRECTORY_LB as u32);
    if dir.len() > 3 {
        list_add(dlg, DID_DIRECTORY_LB as u32, b"..");
    }
    for d in &dirs {
        list_add(dlg, DID_DIRECTORY_LB as u32, d);
    }
    list_clear(dlg, DID_FILES_LB as u32);
    for f in &files {
        list_add(dlg, DID_FILES_LB as u32, f);
    }
    set_item_text(dlg, DID_DIRECTORY_SELECTED as u32, &dir);
    // The drive list shows the current drive.
    let n = unsafe { WinSendDlgItemMsg(dlg, DID_DRIVE_CB as u32, LM_QUERYITEMCOUNT, 0, 0) };
    for i in 0..n {
        let t = list_text(dlg, DID_DRIVE_CB as u32, i);
        if t.len() >= 2 && t[..2].eq_ignore_ascii_case(&dir[..2]) {
            set_item_text(dlg, DID_DRIVE_CB as u32, &t);
        }
    }
}

fn change_dir(dlg: u32, to: Vec<u8>) {
    if let Some(s) = state(dlg) {
        s.dir = to;
    }
    refresh(dlg);
}

fn init(dlg: u32, fild: *mut u8) {
    unsafe { WinSetWindowPtr(dlg, 0, fild as u32) };
    let fl = rd32(fild, F_FL);
    let full = cstr(unsafe { fild.add(F_FULLFILE) });
    // The initial directory and name (or pattern).
    let (mut dir, name) = match full.iter().rposition(|&c| c == b'\\' || c == b':') {
        Some(i) => {
            let d = full_path(&current_dir(), &full[..=i]);
            (d, full[i + 1..].to_vec())
        }
        None => (current_dir(), full.clone()),
    };
    if !is_dir(&dir) {
        dir = current_dir();
    }
    let pattern = if has_wild(&name) { name.clone() } else { b"*".to_vec() };
    states().retain(|s| s.hwnd != dlg);
    states().push(State { hwnd: dlg, fild, dir, pattern });
    let title = rd32(fild, F_TITLE);
    let t: Vec<u8> = if title != 0 { cstr(title as *const u8) } else if fl & FDS_SAVEAS_DIALOG != 0 { b"Save As".to_vec() } else { b"Open".to_vec() };
    unsafe { WinSetWindowText(dlg, z(&t).as_ptr()) };
    let ok = rd32(fild, F_OKBUTTON);
    if ok != 0 {
        unsafe { WinSetDlgItemText(dlg, DID_OK, ok as *const u8) };
    }
    unsafe { WinSendDlgItemMsg(dlg, DID_FILENAME_ED as u32, EM_SETTEXTLIMIT, CCHMAXPATH as u32 - 1, 0) };
    set_item_text(dlg, DID_FILENAME_ED as u32, &name);
    // Drives.
    let (mut disk, mut map) = (0u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut disk, &mut map) };
    for i in 0..26 {
        if map & (1 << i) != 0 {
            list_add(dlg, DID_DRIVE_CB as u32, &[b'A' + i as u8, b':']);
        }
    }
    // Types: <All Files> and the application's list.
    list_add(dlg, DID_FILTER_CB as u32, b"<All Files>");
    let types = rd32(fild, F_TYPELIST);
    if types != 0 {
        for k in 0..64 {
            let p = rd32(types as *const u8, k * 4);
            if p == 0 {
                break;
            }
            list_add(dlg, DID_FILTER_CB as u32, &cstr(p as *const u8));
        }
    }
    let itype = rd32(fild, F_ITYPE);
    set_item_text(dlg, DID_FILTER_CB as u32, &if itype != 0 { cstr(itype as *const u8) } else { b"<All Files>".to_vec() });
    let idrive = rd32(fild, F_IDRIVE);
    if idrive != 0 {
        let d = cstr(idrive as *const u8);
        if d.len() >= 2 && d[1] == b':' {
            let disk = (d[0].to_ascii_uppercase() - b'A' + 1) as u32;
            if let Some(s) = state(dlg) {
                s.dir = dir_of_drive(disk);
            }
        }
    }
    refresh(dlg);
    unsafe {
        WinSendDlgItemMsg(dlg, DID_FILENAME_ED as u32, EM_SETSEL, mp(0, 0x7fff), 0);
        WinSetFocus(1, WinWindowFromID(dlg, DID_FILENAME_ED as u32));
    }
}

/// OK: take the file name (a pattern or a directory changes the lists).
fn ok(dlg: u32) {
    let Some(s) = state(dlg) else { return };
    let (fild, dir) = (s.fild, s.dir.clone());
    let typed = item_text(dlg, DID_FILENAME_ED as u32);
    let typed: Vec<u8> = typed.iter().copied().skip_while(|&c| c == b' ').collect();
    if typed.is_empty() {
        unsafe { WinAlarm(1, 0) };
        return;
    }
    let path = full_path(&dir, &typed);
    let (pdir, name) = match path.iter().rposition(|&c| c == b'\\') {
        Some(i) => (if i == 2 { path[..3].to_vec() } else { path[..i].to_vec() }, path[i + 1..].to_vec()),
        None => (dir.clone(), path.clone()),
    };
    if has_wild(&name) {
        if let Some(s) = state(dlg) {
            s.pattern = name;
        }
        change_dir(dlg, pdir);
        set_item_text(dlg, DID_FILENAME_ED as u32, &state(dlg).map_or(Vec::new(), |s| s.pattern.clone()));
        return;
    }
    if name.is_empty() || is_dir(&path) {
        change_dir(dlg, path);
        set_item_text(dlg, DID_FILENAME_ED as u32, b"");
        return;
    }
    let fl = rd32(fild, F_FL);
    if fl & FDS_SAVEAS_DIALOG == 0 && !exists(&path) {
        let mut m = path.clone();
        m.extend_from_slice(b"\nThe file was not found.");
        message(dlg, &m, b"Open");
        return;
    }
    if path.len() >= CCHMAXPATH {
        message(dlg, b"The path is too long.", b"File");
        return;
    }
    let zp = z(&path);
    unsafe { core::ptr::copy_nonoverlapping(zp.as_ptr(), fild.add(F_FULLFILE), zp.len()) };
    // The application (its dialog procedure) may refuse the name.
    if unsafe { WinSendMsg(dlg, FDM_VALIDATE, fild.add(F_FULLFILE) as u32, DID_FILENAME_ED as u32) } == 0 {
        return;
    }
    let t = list_selection(dlg, DID_FILTER_CB as u32).map_or(-1i16, |i| i as i16 - 1);
    wr16(fild, F_EATYPE, t as u16);
    wr32(fild, F_FQFCOUNT, 1);
    wr32(fild, F_FQFILENAME, 0);
    wr32(fild, F_RETURN, DID_OK);
    unsafe { WinDismissDlg(dlg, DID_OK) };
}

/// The dialog's window procedure: makes the FILEDLG the QWL_USER window
/// word before anything else and passes every message to the
/// application's procedure (FILEDLG.pfnDlgProc), else to
/// WinDefFileDlgProc.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2FileDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_INITDLG {
        unsafe { WinSetWindowPtr(hwnd, 0, mp2) };
    }
    let fild = unsafe { WinQueryWindowPtr(hwnd, 0) } as *const u8;
    let user = if fild.is_null() { 0 } else { rd32(fild, F_DLGPROC) };
    if user != 0 {
        let f: extern "C" fn(u32, u32, u32, u32) -> u32 = unsafe { core::mem::transmute(user as usize) };
        return f(hwnd, msg, mp1, mp2);
    }
    WinDefFileDlgProc(hwnd, msg, mp1, mp2)
}

/// WinDefFileDlgProc(hwnd, msg, mp1, mp2)
#[unsafe(no_mangle)]
pub extern "C" fn WinDefFileDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            init(hwnd, mp2 as *mut u8);
            1
        }
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            match (id as u16, code) {
                (DID_FILES_LB, LN_SELECT) => {
                    if let Some(i) = list_selection(hwnd, id) {
                        let t = list_text(hwnd, id, i);
                        set_item_text(hwnd, DID_FILENAME_ED as u32, &t);
                    }
                }
                (DID_FILES_LB, LN_ENTER) => ok(hwnd),
                (DID_DIRECTORY_LB, LN_ENTER) => {
                    if let (Some(i), Some(dir)) = (list_selection(hwnd, id), state(hwnd).map(|s| s.dir.clone())) {
                        let t = list_text(hwnd, id, i);
                        change_dir(hwnd, full_path(&dir, &t));
                    }
                }
                (DID_DRIVE_CB, CBN_LBSELECT) | (DID_DRIVE_CB, CBN_ENTER) => {
                    if let Some(i) = list_selection(hwnd, id) {
                        let t = list_text(hwnd, id, i);
                        let cur = state(hwnd).map_or(Vec::new(), |s| s.dir.clone());
                        if t.len() >= 2 && (cur.len() < 2 || !t[..2].eq_ignore_ascii_case(&cur[..2])) {
                            let disk = (t[0].to_ascii_uppercase() - b'A' + 1) as u32;
                            change_dir(hwnd, dir_of_drive(disk));
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                DID_OK => ok(hwnd),
                DID_CANCEL => {
                    if let Some(s) = state(hwnd) {
                        wr32(s.fild, F_RETURN, DID_CANCEL);
                    }
                    unsafe { WinDismissDlg(hwnd, DID_CANCEL) };
                }
                x if x == DID_HELP_PB as u32 => {
                    unsafe { WinSendMsg(hwnd, WM_HELP, mp1, mp2) };
                }
                _ => {}
            }
            0
        }
        // The defaults: keep every file, accept every name, use the
        // standard error messages.
        FDM_FILTER | FDM_VALIDATE => 1,
        FDM_ERROR => 0,
        _ => unsafe { WinDefDlgProc(hwnd, msg, mp1, mp2) },
    }
}

/// The dialog's template (dialog units: 2x2 pels).
fn template_for(fl: u32) -> Vec<u8> {
    let save = fl & FDS_SAVEAS_DIALOG != 0;
    let st = WS_VISIBLE;
    let tab = WS_VISIBLE | WS_TABSTOP;
    // WS_CLIPSIBLINGS | WS_SAVEBITS | FS_DLGBORDER | FS_NOBYTEALIGN;
    // FCF_TITLEBAR | FCF_SYSMENU.
    let mut frame = item(WC_FRAME, if save { b"Save As" } else { b"Open" }, 0x1400_0000 | 0x0000_0080 | 0x0000_0010, (20, 20, 296, 210), 256);
    frame.ctl = (0x0000_0001u32 | 0x0000_0002).to_le_bytes().to_vec();
    let mut v = Vec::new();
    let name_label: &[u8] = if save { b"Save as filename:" } else { b"Open filename:" };
    v.push(item(WC_STATIC, name_label, st | 0x0000_0001, (6, 196, 280, 8), DID_FILENAME_TXT));
    v.push(item(WC_ENTRYFIELD, b"", tab | WS_GROUP | 0x0000_0008 | 0x0000_0004, (8, 184, 280, 8), DID_FILENAME_ED));
    v.push(item(WC_STATIC, if save { b"Save file as type:" } else { b"Type of file:" }, st | 0x0000_0001, (6, 168, 136, 8), DID_FILTER_TXT));
    // CBS_DROPDOWNLIST.
    v.push(item(WC_COMBOBOX, b"", tab | WS_GROUP | 0x0000_0004, (6, 80, 136, 86), DID_FILTER_CB));
    v.push(item(WC_STATIC, b"Drive:", st | 0x0000_0001, (152, 168, 136, 8), DID_DRIVE_TXT));
    v.push(item(WC_COMBOBOX, b"", tab | WS_GROUP | 0x0000_0004, (152, 80, 136, 86), DID_DRIVE_CB));
    v.push(item(WC_STATIC, b"", st | 0x0000_0001, (6, 142, 282, 8), DID_DIRECTORY_SELECTED));
    v.push(item(WC_STATIC, b"Directory:", st | 0x0000_0001, (6, 130, 136, 8), DID_DIRECTORY_TXT));
    v.push(item(WC_LISTBOX, b"", tab | WS_GROUP, (6, 30, 136, 98), DID_DIRECTORY_LB));
    v.push(item(WC_STATIC, b"File:", st | 0x0000_0001, (152, 130, 136, 8), DID_FILES_TXT));
    v.push(item(WC_LISTBOX, b"", tab | WS_GROUP, (152, 30, 136, 98), DID_FILES_LB));
    // Push buttons in the places of IBM's template (applications add
    // their own between OK and Cancel, where Apply goes): OK
    // (BS_DEFAULT), Apply, Cancel, Help.
    v.push(item(WC_BUTTON, b"OK", tab | WS_GROUP | 0x0400, (7, 7, 50, 14), DID_OK as u16));
    if fl & FDS_APPLYBUTTON != 0 {
        v.push(item(WC_BUTTON, b"~Apply", tab, (63, 7, 50, 14), DID_APPLY_PB));
    }
    v.push(item(WC_BUTTON, b"Cancel", tab, (119, 7, 50, 14), DID_CANCEL as u16));
    if fl & FDS_HELPBUTTON != 0 {
        // BS_HELP | BS_NOPOINTERFOCUS.
        v.push(item(WC_BUTTON, b"Help", tab | 0x0100 | 0x0800, (175, 7, 50, 14), DID_HELP_PB));
    }
    template(frame, v)
}

/// WinFileDlg(hwndParent, hwndOwner, pfild): the dialog's window (for
/// FDS_MODELESS) or TRUE when a modal one ran; 0 on errors (lReturn 0,
/// lSRC an FDS_ERR_* code).
#[unsafe(no_mangle)]
pub extern "C" fn WinFileDlg(parent: u32, owner: u32, fild: *mut u8) -> u32 {
    if fild.is_null() {
        return 0;
    }
    let fl = rd32(fild, F_FL);
    wr32(fild, F_RETURN, 0);
    wr32(fild, F_SRC, FDS_SUCCESSFUL);
    let t = template_for(fl);
    let proc_ = crate::exports::__a_Rs2FileDlgProc as *const () as u32;
    let parent = if parent == 0 { 1 } else { parent };
    let dlg = unsafe { WinCreateDlg(parent, owner, proc_, t.as_ptr(), fild as u32) };
    if dlg == 0 {
        wr32(fild, F_SRC, FDS_ERR_INVALID_DIALOG);
        return 0;
    }
    // FDS_CENTER or not (FILEDLG x, y are ignored): in the middle of the
    // screen.
    let _ = (FDS_CENTER, F_X, F_Y);
    center(dlg);
    if fl & FDS_MODELESS != 0 {
        unsafe { WinShowWindow(dlg, 1) };
        return dlg;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    if rd32(fild, F_RETURN) == 0 {
        wr32(fild, F_RETURN, r);
    }
    states().retain(|s| s.hwnd != dlg);
    unsafe { WinDestroyWindow(dlg) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinFreeFileDlgList(_list: u32) -> u32 {
    1
}
