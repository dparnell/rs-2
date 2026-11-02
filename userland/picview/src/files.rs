//! The Open and Save As windows, as OS/2's Picture Viewer has them (its
//! own, not the standard file dialog): the current path, a file name
//! field, lists of drives, directories and files; Open has buttons that
//! list *.MET, *.PIF or *.SPL files, takes several files (shown one after
//! the other: a sequence) and asks how to step through them
//! (interactively with Ctrl+N, or automatically after a delay).

use alloc::vec::Vec;

use pmkit::*;

pub const DLG_OPEN: u16 = 400;
pub const DLG_SAVEAS: u16 = 599;
pub const ID_PATH: u32 = 401;
pub const ID_NAME: u32 = 402;
pub const ID_DIRS: u32 = 403;
pub const ID_FILES: u32 = 404;
pub const ID_DRIVES: u32 = 405;
pub const ID_MET: u32 = 407;
pub const ID_PIF: u32 = 408;
pub const ID_SPL: u32 = 409;
pub const ID_HELP: u32 = 410;
pub const ID_INTERACTIVE: u32 = 411;
pub const ID_AUTOMATIC: u32 = 412;
pub const ID_DELAY: u32 = 413;

/// At most this many files make a sequence (OS/2's limit).
pub const MAX_FILES: usize = 50;

/// What Open chose.
pub struct OpenChoice {
    pub files: Vec<Vec<u8>>,
    /// Step through them by themselves, every `delay` seconds.
    pub automatic: bool,
    pub delay: u32,
}

struct State {
    save: bool,
    filter: Vec<u8>,
    automatic: bool,
    delay: u32,
    result: Option<OpenChoice>,
    save_name: Option<Vec<u8>>,
    /// While the lists are filled (their notifications are ignored).
    filling: bool,
}

static mut STATE: State = State { save: false, filter: Vec::new(), automatic: false, delay: 5, result: None, save_name: None, filling: false };

fn st() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

const LS_EXTENDEDSEL: u32 = 0x0010;

/// The current drive and directory, "C:\OS2\BITMAP".
pub fn current_dir() -> Vec<u8> {
    let (mut disk, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut disk, &mut map) };
    let mut buf = [0u8; 260];
    let mut n = buf.len() as u32;
    unsafe { os2::DosQueryCurrentDir(0, buf.as_mut_ptr(), &mut n) };
    let mut p = alloc::vec![b'@' + disk as u8, b':', b'\\'];
    p.extend_from_slice(until_nul(&buf));
    p
}

/// A name made full with the current directory.
pub fn full_name(name: &[u8]) -> Vec<u8> {
    if name.len() >= 2 && name[1] == b':' {
        if name.get(2) == Some(&b'\\') {
            return name.to_vec();
        }
        // "D:NAME": relative to that drive's directory.
        let mut buf = [0u8; 260];
        let mut n = buf.len() as u32;
        let disk = (name[0].to_ascii_uppercase() - b'@') as u32;
        unsafe { os2::DosQueryCurrentDir(disk, buf.as_mut_ptr(), &mut n) };
        let mut p = alloc::vec![name[0].to_ascii_uppercase(), b':', b'\\'];
        p.extend_from_slice(until_nul(&buf));
        if !p.ends_with(b"\\") {
            p.push(b'\\');
        }
        p.extend_from_slice(&name[2..]);
        return p;
    }
    let mut p = current_dir();
    if name.first() == Some(&b'\\') {
        p.truncate(2);
    } else if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(name);
    p
}

/// The entries of the current directory matching `pattern`: (name, is a
/// directory).
fn entries(pattern: &[u8], dirs: bool) -> Vec<Vec<u8>> {
    let pat = z(pattern);
    let mut h: u32 = 0xffff_ffff;
    let mut buf = alloc::vec![0u8; 8192];
    let mut count: u32 = 100;
    // Directories (MUST_HAVE_DIRECTORY) or files.
    let attr = if dirs { 0x1010 } else { 0x0001 | 0x0020 };
    let mut out = Vec::new();
    let mut rc = unsafe { os2::DosFindFirst(pat.as_ptr(), &mut h, attr, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    while rc == 0 && count > 0 {
        let mut off = 0usize;
        for _ in 0..count {
            // FILEFINDBUF3: oNextEntryOffset, dates and times, sizes,
            // attributes, cchName, achName.
            let next = rd32(&buf, off) as usize;
            let attrs = rd32(&buf, off + 24);
            let n = buf[off + 28] as usize;
            let name = buf[off + 29..off + 29 + n].to_vec();
            let is_dir = attrs & 0x10 != 0;
            if is_dir == dirs && name != b"." {
                out.push(name);
            }
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 100;
        rc = unsafe { os2::DosFindNext(h, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    }
    unsafe { os2::DosFindClose(h) };
    out.sort_by_key(|n| n.to_ascii_uppercase());
    out
}

/// The drives there are, "A:".."Z:".
fn drives() -> Vec<Vec<u8>> {
    let (mut disk, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut disk, &mut map) };
    (0..26).filter(|i| map & (1 << i) != 0).map(|i| alloc::vec![b'A' + i as u8, b':']).collect()
}

fn fill(h: u32) {
    let s = st();
    s.filling = true;
    set_item_text(h, ID_PATH, &current_dir());
    for id in [ID_DRIVES, ID_DIRS, ID_FILES] {
        dlg_msg(h, id, LM_DELETEALL, 0, 0);
    }
    let cur = current_dir();
    for (i, d) in drives().iter().enumerate() {
        lb_add(h, ID_DRIVES, d);
        if d[0] == cur[0] {
            dlg_msg(h, ID_DRIVES, LM_SELECTITEM, i as u32, 1);
        }
    }
    for d in entries(b"*", true) {
        lb_add(h, ID_DIRS, &d);
    }
    for f in entries(&s.filter, false) {
        lb_add(h, ID_FILES, &f);
    }
    s.filling = false;
}

fn change_dir(dir: &[u8]) -> bool {
    // "D:\PATH": the drive first (the directory call leaves it).
    let dir = if dir.len() >= 2 && dir[1] == b':' {
        if !change_drive(dir[0]) {
            return false;
        }
        if dir.len() == 2 {
            return true;
        }
        &dir[2..]
    } else {
        dir
    };
    let d = z(dir);
    unsafe { os2::DosSetCurrentDir(d.as_ptr()) == 0 }
}

fn change_drive(letter: u8) -> bool {
    unsafe { os2::DosSetDefaultDisk((letter.to_ascii_uppercase() - b'@') as u32) == 0 }
}

/// The names selected in the files list.
fn selected(h: u32) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut i = LIT_FIRST;
    loop {
        let r = dlg_msg(h, ID_FILES, LM_QUERYSELECTION, i, 0);
        if r as u16 == 0xffff || v.len() > 500 {
            break;
        }
        v.push(lb_text(h, ID_FILES, r & 0xffff));
        i = r & 0xffff;
    }
    v
}

/// The name typed: a directory (changed to), a pattern (listed), a
/// drive (changed to), or a file. Returns the file's full name.
fn typed(h: u32) -> Option<Vec<u8>> {
    let s = st();
    let name: Vec<u8> = item_text(h, ID_NAME).into_iter().filter(|&c| c != b'"').collect();
    let name = name.trim_ascii().to_vec();
    if name.is_empty() {
        return None;
    }
    if name.len() == 2 && name[1] == b':' {
        if !change_drive(name[0]) {
            message(h, b"Drive is not ready or media type is invalid. Insert a formatted diskette in the drive and try again, or select another drive.", crate::TITLE, MB_OK | MB_WARNING);
        }
        set_item_text(h, ID_NAME, b"");
        fill(h);
        return None;
    }
    if name.contains(&b'*') || name.contains(&b'?') {
        // A pattern; a directory part changes the directory first.
        if let Some(i) = name.iter().rposition(|&c| c == b'\\') {
            if !change_dir(&name[..i.max(1)]) {
                message(h, b"The program cannot find the specified path.", crate::TITLE, MB_OK | MB_WARNING);
                return None;
            }
            s.filter = name[i + 1..].to_vec();
        } else {
            s.filter = name;
        }
        set_item_text(h, ID_NAME, b"");
        fill(h);
        return None;
    }
    if change_dir(&name) {
        set_item_text(h, ID_NAME, b"");
        fill(h);
        return None;
    }
    Some(full_name(&name))
}

extern "C" fn dlg_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let s = st();
    match m {
        WM_INITDLG => {
            dlg_msg(h, ID_NAME, EM_SETTEXTLIMIT, 255, 0);
            if !s.save {
                set_check(h, if s.automatic { ID_AUTOMATIC } else { ID_INTERACTIVE }, true);
                dlg_msg(h, ID_DELAY, EM_SETTEXTLIMIT, 3, 0);
                set_item_text(h, ID_DELAY, &num(s.delay as i64));
                enable(h, ID_DELAY, s.automatic);
            }
            if let Some(n) = &s.save_name {
                set_item_text(h, ID_NAME, n);
            }
            fill(h);
            unsafe { WinSetFocus(HWND_DESKTOP, item(h, ID_NAME)) };
            1
        }
        WM_CONTROL => {
            let (id, code) = (lo(mp1), hi(mp1));
            if s.filling {
                return 0;
            }
            match (id, code) {
                (ID_DRIVES, LN_SELECT) => {
                    let i = dlg_msg(h, ID_DRIVES, LM_QUERYSELECTION, LIT_FIRST, 0);
                    let d = lb_text(h, ID_DRIVES, i & 0xffff);
                    if let Some(&l) = d.first() {
                        if l != current_dir()[0] {
                            if !change_drive(l) {
                                message(h, b"Drive is not ready or media type is invalid. Insert a formatted diskette in the drive and try again, or select another drive.", crate::TITLE, MB_OK | MB_WARNING);
                            }
                            post(h, WM_USER, 0, 0);
                        }
                    }
                }
                (ID_DIRS, LN_ENTER) => {
                    let i = dlg_msg(h, ID_DIRS, LM_QUERYSELECTION, LIT_FIRST, 0);
                    let d = lb_text(h, ID_DIRS, i & 0xffff);
                    if !d.is_empty() && change_dir(&d) {
                        post(h, WM_USER, 0, 0);
                    }
                }
                (ID_FILES, LN_SELECT) => {
                    let sel = selected(h);
                    if sel.len() == 1 {
                        set_item_text(h, ID_NAME, &sel[0]);
                    } else if sel.len() > 1 {
                        set_item_text(h, ID_NAME, b"");
                    }
                }
                (ID_FILES, LN_ENTER) => {
                    post(h, WM_COMMAND, DID_OK, 0);
                }
                (ID_INTERACTIVE, BN_CLICKED) | (ID_AUTOMATIC, BN_CLICKED) => {
                    enable(h, ID_DELAY, checked(h, ID_AUTOMATIC));
                }
                _ => {}
            }
            0
        }
        // The lists again, after a change of drive or directory.
        WM_USER => {
            fill(h);
            0
        }
        WM_COMMAND => {
            crate::trace(alloc::format!("file dialog command {}", lo(mp1)).as_bytes());
            match lo(mp1) {
                DID_OK => ok(h),
                DID_CANCEL => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
                ID_MET | ID_PIF | ID_SPL => {
                    s.filter = match lo(mp1) {
                        ID_MET => b"*.MET".to_vec(),
                        ID_PIF => b"*.PIF".to_vec(),
                        _ => b"*.SPL".to_vec(),
                    };
                    fill(h);
                }
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn ok(h: u32) {
    let s = st();
    if s.save {
        let Some(mut name) = typed(h) else {
            let sel = selected(h);
            if let Some(f) = sel.first() {
                set_item_text(h, ID_NAME, f);
            }
            return;
        };
        if !pmkit::file_name(&name).contains(&b'.') {
            name.extend_from_slice(b".MET");
        }
        s.save_name = Some(name);
        unsafe { WinDismissDlg(h, DID_OK) };
        return;
    }
    let mut files: Vec<Vec<u8>> = Vec::new();
    let sel = selected(h);
    let typed_name = item_text(h, ID_NAME);
    if sel.len() > 1 || (sel.len() == 1 && typed_name.trim_ascii() == sel[0].as_slice()) {
        files = sel.iter().map(|f| full_name(f)).collect();
    } else {
        match typed(h) {
            Some(f) => files.push(f),
            None => return,
        }
    }
    if files.is_empty() {
        return;
    }
    if files.len() > MAX_FILES {
        message(h, b"You have selected too many files. Only the first 50 selected files will be shown.", crate::TITLE, MB_OK | MB_INFORMATION);
        files.truncate(MAX_FILES);
    }
    let automatic = checked(h, ID_AUTOMATIC);
    let mut delay = s.delay;
    if automatic {
        match parse_num(&item_text(h, ID_DELAY)) {
            Some(d) if (1..=999).contains(&d) => delay = d as u32,
            _ => {
                message(h, b"The delay is not valid. Type a number of seconds from 1 to 999.", crate::TITLE, MB_OK | MB_WARNING);
                unsafe { WinSetFocus(HWND_DESKTOP, item(h, ID_DELAY)) };
                return;
            }
        }
    }
    s.automatic = automatic;
    s.delay = delay;
    s.result = Some(OpenChoice { files, automatic, delay });
    unsafe { WinDismissDlg(h, DID_OK) };
}

/// The Open window; the files chosen.
pub fn open(owner: u32) -> Option<OpenChoice> {
    let s = st();
    s.save = false;
    s.result = None;
    s.save_name = None;
    if s.filter.is_empty() {
        s.filter = b"*.MET".to_vec();
    }
    let tpl = template(
        dialog_frame(b"Picture Viewer - Open", DLG_OPEN, 316, 180),
        alloc::vec![
            label(b"Choose a file with the mouse, or type its name.", (8, 162, 300, 8), DT_LEFT),
            label(b"Current path:", (8, 148, 62, 8), DT_LEFT),
            text_id(b"", (72, 148, 236, 8), ID_PATH, DT_LEFT),
            label(b"File name:", (8, 132, 48, 8), DT_LEFT),
            entry((58, 132, 248, 8), ID_NAME, 0),
            label(b"Drives", (8, 116, 32, 8), DT_LEFT),
            label(b"Directories", (46, 116, 120, 8), DT_LEFT),
            label(b"Files", (182, 116, 120, 8), DT_LEFT),
            list_box((6, 64, 34, 50), ID_DRIVES, 0),
            list_box((46, 64, 128, 50), ID_DIRS, 0),
            list_box((182, 64, 128, 50), ID_FILES, LS_MULTIPLESEL | LS_EXTENDEDSEL),
            label(b"Show the pictures:", (8, 48, 96, 8), DT_LEFT),
            radio(b"~Interactively", (106, 46, 80, 10), ID_INTERACTIVE).group(),
            radio(b"~Automatically", (196, 46, 80, 10), ID_AUTOMATIC),
            label(b"Delay (seconds):", (8, 32, 96, 8), DT_LEFT),
            entry((110, 32, 26, 8), ID_DELAY, 0).group(),
            button(b"~Open", (8, 8, 40), DID_OK, BS_DEFAULT).group(),
            button(b"*.~MET", (56, 8, 40), ID_MET, 0),
            button(b"*.~PIF", (102, 8, 40), ID_PIF, 0),
            button(b"*.~SPL", (148, 8, 40), ID_SPL, 0),
            button(b"Cancel", (196, 8, 56), DID_CANCEL, 0),
            help_button((262, 8, 46), ID_HELP),
        ],
    );
    if run_dialog(owner, dlg_proc, &tpl, 0) == DID_OK { s.result.take() } else { None }
}

/// The Save As window; the file name chosen.
pub fn save_as(owner: u32, initial: &[u8]) -> Option<Vec<u8>> {
    let s = st();
    s.save = true;
    s.save_name = (!initial.is_empty()).then(|| initial.to_vec());
    let old = core::mem::replace(&mut s.filter, b"*.MET".to_vec());
    let tpl = template(
        dialog_frame(b"Picture Viewer - Save As", DLG_SAVEAS, 316, 148),
        alloc::vec![
            label(b"Choose a file with the mouse, or type its name.", (8, 130, 300, 8), DT_LEFT),
            label(b"Current path:", (8, 116, 62, 8), DT_LEFT),
            text_id(b"", (72, 116, 236, 8), ID_PATH, DT_LEFT),
            label(b"File name:", (8, 100, 48, 8), DT_LEFT),
            entry((58, 100, 248, 8), ID_NAME, 0),
            label(b"Drives", (8, 84, 32, 8), DT_LEFT),
            label(b"Directories", (46, 84, 120, 8), DT_LEFT),
            label(b"Files", (182, 84, 120, 8), DT_LEFT),
            list_box((6, 32, 34, 50), ID_DRIVES, 0),
            list_box((46, 32, 128, 50), ID_DIRS, 0),
            list_box((182, 32, 128, 50), ID_FILES, 0),
            button(b"~Save", (50, 8, 46), DID_OK, BS_DEFAULT).group(),
            button(b"Cancel", (120, 8, 56), DID_CANCEL, 0),
            help_button((220, 8, 46), ID_HELP),
        ],
    );
    let r = run_dialog(owner, dlg_proc, &tpl, 0);
    s.filter = old;
    if r == DID_OK { s.save_name.take() } else { None }
}
