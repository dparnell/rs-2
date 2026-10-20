//! `E /SELFTEST`: checks of the editor's file and editing logic on a
//! hidden window: opening a file into the MLE (its Ctrl+Z end-of-file
//! mark dropped), editing, saving (CR LF text and the mark written), the
//! .TYPE rules (Save as and malformed types get the default type, known
//! and unknown types are kept, "Plain Text" added to unknown ones), a
//! missing file opening as a new one, a large file's round trip, Find,
//! autosave, New, the settings in OS2.INI, and closing (unchanged text
//! closes without asking). Prints PASS/FAIL lines and "e selftest done:
//! N failure(s)".

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

fn read(p: &[u8]) -> Vec<u8> {
    file::read(p).unwrap_or_default()
}

fn quit_posted() -> bool {
    let mut q = QMsg::default();
    unsafe { WinPeekMsg(app().hab, &mut q, 0, WM_QUIT, WM_QUIT, 1) != 0 }
}

/// Save the current file as File > Save does when no question is due.
fn save_quietly(path: &[u8]) -> bool {
    match types_for(path, false) {
        Some(t) => write_with(path, t),
        None => false,
    }
}

pub fn run() -> u32 {
    let a = app();
    check("window created", create_window() && a.mle != 0 && a.menu != 0);
    check("starts Untitled", a.path.is_none() && !changed());

    let path = file::full_name(b"C:\\ETEST.TXT");
    delete(&path);
    check("test file written", file::write(&path, b"line one\r\nline two\r\n\x1a").is_ok());
    check("open", open_file(&path));
    check("open: title is the full name", a.path.as_deref() == Some(&path[..]) && window_text(a.frame) == path);
    check("open: end-of-file mark dropped, LF text in the MLE", text_length() == 18);
    check("open: unchanged", !changed());
    check("export gives the text back", get_text() == b"line one\r\nline two\r\n");

    // A file with a type the editor knows keeps it, written with the
    // end-of-file mark.
    check("type set", file::set_type(&path, TYPES[1]) == 0);
    check("classified as known", file::classify(&path, &TYPES) == Ok(file::TypeEa::Known));
    insert_at(0, b"x");
    check("edit: changed", changed());
    check("save (known type)", save_quietly(&path));
    check("saved: unchanged", !changed());
    check("saved: text and Ctrl+Z", read(&path) == b"xline one\r\nline two\r\n\x1a");
    check("saved: type kept", file::types(&path) == [TYPES[1].to_vec()]);

    // Save as: the default type.
    let other = file::full_name(b"C:\\ETEST2.TXT");
    delete(&other);
    a.type_checked = false;
    a.default_type = 2;
    check("save as gets the default type", types_for(&other, true).is_some_and(|t| write_with(&other, t)) && file::types(&other) == [TYPES[2].to_vec()]);
    a.default_type = 0;

    // Types the editor does not know: kept, or "Plain Text" added.
    check("unknown types", file::set_types(&path, &[b"Bitmap", b"Icon"]) == 0);
    check("classified as unknown", file::classify(&path, &TYPES) == Ok(file::TypeEa::Unknown(alloc::vec![b"Bitmap".to_vec(), b"Icon".to_vec()])));
    a.type_checked = false;
    insert_at(0, b"u");
    check("save keeping unknown types", write_with(&path, NewTypes::Keep) && file::types(&path) == [b"Bitmap".to_vec(), b"Icon".to_vec()]);
    let add = NewTypes::Set(alloc::vec![b"Bitmap".to_vec(), b"Icon".to_vec(), TYPES[0].to_vec()]);
    check("save adding Plain Text", write_with(&path, add) && file::types(&path) == [b"Bitmap".to_vec(), b"Icon".to_vec(), b"Plain Text".to_vec()]);
    check("now known", file::classify(&path, &TYPES) == Ok(file::TypeEa::Known));
    check("a type value that is not ASCII is malformed", file::parse_types(&[0xfe, 0xff, 1, 0, 0]).is_none());
    check("a single ASCII type", file::parse_types(&[0xfd, 0xff, 2, 0, b'h', b'i']) == Some(alloc::vec![b"hi".to_vec()]));

    // Once a save dealt with the type, later saves do not ask again.
    a.type_checked = true;
    file::set_types(&path, &[]);
    insert_at(0, b"v");
    check("after a save: no question", save_quietly(&path));

    // Find (MLM_SEARCH as the Find dialog uses it).
    set_text(b"alpha beta\r\nBeta gamma\r\n");
    a.find_text = b"beta".to_vec();
    a.find_case = true;
    a.find_wrap = false;
    send(a.mle, MLM_SETSEL, 0, 0);
    check("find: first match", dialogs::find_next() && send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0) == 6);
    check("find: case sensitive, no wrap: no more", !dialogs::find_next());
    a.find_case = false;
    check("find: any case finds Beta", dialogs::find_next() && send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0) == 11);
    a.find_wrap = true;
    check("find: wrap starts over", dialogs::find_next() && send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0) == 6);

    // Autosave after 3 changes (the file has a known type: no question).
    set_text(b"auto\r\n");
    file::set_type(&path, TYPES[0]);
    a.type_checked = false;
    (a.autosave, a.autosave_every) = (true, 3);
    insert_at(0, b"1");
    insert_at(0, b"2");
    check("autosave: not yet", changed());
    insert_at(0, b"3");
    check("autosave: saved", !changed() && read(&path) == b"321auto\r\n\x1a" && a.changes == 0);
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
    check("missing file opens empty", open_file(&missing) && a.path.as_deref() == Some(&missing[..]) && text_length() == 0);
    check("missing file: no file yet", file::classify(&missing, &TYPES) == Ok(file::TypeEa::NoFile));

    // Settings.
    save_int(b"IAUTOSAVE\0", 42);
    save_int(b"ULFGNDCOLOR\0", -1);
    load_settings();
    check("settings: EHXMAIN IAUTOSAVE, ULFGNDCOLOR", a.autosave_every == 42 && a.fg == -1);

    // New and closing with no changes: no questions.
    send(a.client, WM_COMMAND, IDM_NEW as u32, 0);
    check("New: Untitled, empty", a.path.is_none() && text_length() == 0 && !changed() && window_text(a.frame) == UNTITLED);
    check("unchanged: may close", process_save_quit());
    send(a.client, WM_CLOSE, 0, 0);
    check("WM_CLOSE unchanged: quits", quit_posted());
    send(a.client, WM_COMMAND, IDM_CLOSE as u32, 0);
    let mut q = QMsg::default();
    let posted = unsafe { WinPeekMsg(a.hab, &mut q, a.client, WM_CLOSE, WM_CLOSE, 1) } != 0;
    check("F3 (close command) posts WM_CLOSE", posted);

    delete(&path);
    delete(&other);
    delete(&missing);
    unsafe { WinDestroyWindow(a.frame) };
    let fails = unsafe { FAILS };
    let line = alloc::format!("e selftest done: {fails} failure(s)");
    os2::println!("{}", line);
    trace(line.as_bytes());
    fails
}
