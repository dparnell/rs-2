//! `E /SELFTEST`: checks of the editor's file and editing logic on a
//! hidden window: opening a file into the MLE, editing, saving (CR LF
//! text, the .TYPE EA set and kept), a missing file opening as a new
//! one, a large file's round trip, Find, autosave, New, and closing
//! (unchanged text closes without asking). Prints PASS/FAIL lines and
//! "e selftest done: N failure(s)".

use alloc::vec::Vec;

use crate::pm::*;
use crate::*;

static mut FAILS: u32 = 0;

fn check(name: &str, ok: bool) {
    let line = alloc::format!("{} {}", if ok { "PASS" } else { "FAIL" }, name);
    os2::println!("{}", line);
    trace(line.as_bytes());
    if !ok {
        unsafe { FAILS += 1 };
    }
}

unsafe extern "C" {
    #[link_name = "PMWIN.918"]
    fn WinPeekMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32, fl: u32) -> u32;
}

fn changed() -> bool {
    send(app().mle, MLM_QUERYCHANGED, 0, 0) != 0
}

fn insert_at(at: u32, t: &[u8]) {
    let mle = app().mle;
    send(mle, MLM_SETSEL, at, at);
    let s = z(t);
    send(mle, MLM_INSERT, s.as_ptr() as u32, 0);
}

fn delete(p: &[u8]) {
    let s = z(p);
    unsafe { os2::DosDelete(s.as_ptr()) };
}

pub fn run() -> u32 {
    let a = app();
    // Defaults, whatever OS2.INI says.
    (a.wrap, a.autosave, a.autosave_every, a.fg, a.bg) = (true, false, 100, CLR_BLACK, CLR_WHITE);
    a.fattrs = default_fattrs();
    check("window created", create_window() && a.mle != 0 && a.menu != 0);
    check("starts Untitled", a.path.is_none() && !changed());

    let path = file::full_name(b"C:\\ETEST.TXT");
    delete(&path);
    let text = b"line one\r\nline two\r\n";
    check("test file written", file::write(&path, text).is_ok());
    check("open", open_file(&path));
    check("open: title is the full name", a.path.as_deref() == Some(&path[..]));
    check("open: LF text in the MLE", send(a.mle, MLM_QUERYTEXTLENGTH, 0, 0) == 18);
    check("open: unchanged", !changed() && a.changes == 0);
    check("export gives the file back", get_text() == text);
    check("no type yet", file::types(&path).is_empty());

    insert_at(0, b"x");
    check("edit: changed", changed() && a.changes == 1);
    check("save with a type", save_to(&path, Some(TYPES[0])));
    check("saved: unchanged", !changed() && a.changes == 0);
    check("saved: the file", file::read(&path).ok().as_deref() == Some(&b"xline one\r\nline two\r\n"[..]));
    check("saved: .TYPE Plain Text", file::types(&path) == [b"Plain Text".to_vec()]);
    check("File > Save keeps a typed file's type", {
        insert_at(1, b"y");
        save() && file::types(&path) == [b"Plain Text".to_vec()]
    });
    check("rewritten", file::read(&path).ok().as_deref() == Some(&b"xyline one\r\nline two\r\n"[..]));
    check("set_type replaces", file::set_type(&path, TYPES[1]) == 0 && file::types(&path) == [b"OS/2 Command File".to_vec()]);

    // Find (MLM_SEARCH as the Find dialog uses it).
    let mut sd = [0u8; 28];
    let find = b"two";
    sd[0..2].copy_from_slice(&28u16.to_le_bytes());
    sd[4..8].copy_from_slice(&(find.as_ptr() as u32).to_le_bytes());
    sd[12..14].copy_from_slice(&3u16.to_le_bytes());
    sd[16..20].copy_from_slice(&0i32.to_le_bytes());
    sd[20..24].copy_from_slice(&(-1i32).to_le_bytes());
    let found = send(a.mle, MLM_SEARCH, MLFSEARCH_SELECTMATCH | MLFSEARCH_CASESENSITIVE, sd.as_mut_ptr() as u32) != 0;
    let at = u32::from_le_bytes([sd[16], sd[17], sd[18], sd[19]]);
    check("find selects the match", found && at == 16 && send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0) == 16 && send(a.mle, MLM_QUERYSEL, MLFQS_MAXSEL, 0) == 19);

    // Autosave after 3 changes.
    (a.autosave, a.autosave_every) = (true, 3);
    insert_at(0, b"1");
    insert_at(0, b"2");
    check("autosave: not yet", changed() && file::read(&path).ok().as_deref() == Some(&b"xyline one\r\nline two\r\n"[..]));
    insert_at(0, b"3");
    check("autosave: saved", !changed() && file::read(&path).ok().as_deref() == Some(&b"321xyline one\r\nline two\r\n"[..]));
    a.autosave = false;

    // A large file.
    let mut big = Vec::new();
    for i in 0..4000 {
        big.extend_from_slice(alloc::format!("{i:05} the quick brown fox jumps over the lazy dog\r\n").as_bytes());
    }
    check("large file written", file::write(&path, &big).is_ok());
    check("large file round trip", open_file(&path) && get_text() == big && !changed());

    // Missing file: a new document with that name.
    let missing = file::full_name(b"C:\\ENEW.TXT");
    delete(&missing);
    check("missing file opens empty", open_file(&missing) && a.path.as_deref() == Some(&missing[..]) && send(a.mle, MLM_QUERYTEXTLENGTH, 0, 0) == 0);
    insert_at(0, b"new\n");
    check("new file saved", save_to(&missing, Some(TYPES[0])) && file::read(&missing).ok().as_deref() == Some(&b"new\r\n"[..]));

    // New and close with no changes: no questions.
    send(a.client, WM_COMMAND, IDM_NEW as u32, 0);
    check("New: Untitled, empty", a.path.is_none() && send(a.mle, MLM_QUERYTEXTLENGTH, 0, 0) == 0 && !changed());
    send(a.client, WM_SAVEAPPLICATION, 0, 0);
    check("WM_SAVEAPPLICATION unchanged: nothing pending", a.pending_close.is_none());
    send(a.client, WM_CLOSE, 0, 0);
    let mut q = QMsg::default();
    check("WM_CLOSE unchanged: quits", unsafe { WinPeekMsg(a.hab, &mut q, 0, WM_QUIT, WM_QUIT, 1) } != 0);
    // An answer given for WM_SAVEAPPLICATION is used by the WM_CLOSE.
    a.pending_close = Some(false);
    send(a.client, WM_CLOSE, 0, 0);
    check("WM_CLOSE after a cancelled question: stays", unsafe { WinPeekMsg(a.hab, &mut q, 0, WM_QUIT, WM_QUIT, 1) } == 0 && a.pending_close.is_none());

    delete(&path);
    delete(&missing);
    unsafe { WinDestroyWindow(a.frame) };
    let fails = unsafe { FAILS };
    let line = alloc::format!("e selftest done: {fails} failure(s)");
    os2::println!("{}", line);
    trace(line.as_bytes());
    fails
}
