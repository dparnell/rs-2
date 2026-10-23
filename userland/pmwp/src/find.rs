//! Find (a folder's, a drive's or the desktop's Find... menu item): the
//! objects whose names match a pattern (wildcards `*` and `?`), in the
//! folder and, if asked, in all its subfolders. A worker thread walks
//! the directories and matches real names and the titles of `.LONGNAME`;
//! the shell's thread then wakes
//! the objects found, adds the abstract ones (programs, shadows) of the
//! folders searched that are awake and match by title, and shows them
//! in a Find Results window, where they open, show their menus and drag
//! as anywhere else.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::fileops::{self, join};
use crate::pm::*;
use crate::sortinc::wildcard;
use crate::{obj, view, wps};

pub const WM_FIND_DONE: u32 = WM_USER + 30;
/// At most this many objects are found.
const LIMIT: usize = 5000;

struct Job {
    root: Vec<u8>,
    pattern: Vec<u8>,
    subfolders: bool,
    /// The results window's client.
    client: u32,
    /// What was found (paths) and the directories searched.
    found: Vec<Vec<u8>>,
    dirs: Vec<Vec<u8>>,
}

const ID_PATTERN: u16 = 560;
const ID_SUBFOLDERS: u16 = 561;

static mut ANSWER: Option<(Vec<u8>, bool)> = None;

extern "C" fn dlg_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            center(h);
            set_item_text(h, ID_PATTERN as u32, b"*");
            unsafe {
                WinSendDlgItemMsg(h, ID_PATTERN as u32, EM_SETTEXTLIMIT, 255, 0);
                WinSendDlgItemMsg(h, ID_SUBFOLDERS as u32, BM_SETCHECK, 1, 0);
            }
            0
        }
        WM_COMMAND => {
            let id = a & 0xffff;
            if id == DID_OK {
                let p = item_text(h, ID_PATTERN as u32);
                let sub = unsafe { WinSendDlgItemMsg(h, ID_SUBFOLDERS as u32, BM_QUERYCHECK, 0, 0) } != 0;
                unsafe { ANSWER = Some((p, sub)) };
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn label(text: &[u8], (x, y, cx): (i16, i16, i16)) -> Item {
    item(WC_STATIC, text, WS_VISIBLE | 0x0001, (x, y, cx, 8), 0xffff)
}

/// Find... in a folder: ask, then search.
pub fn ask(folder: usize) {
    let Some(root) = wps::path_of(folder) else { return };
    let mut within = b"In: ".to_vec();
    within.extend(obj::title(folder).into_iter().map(|c| if c == b'\n' { b' ' } else { c }));
    let tpl = template(dialog_frame(b"Find", 230, 86), alloc::vec![
        label(&within, (6, 72, 218)),
        label(b"Find the objects whose names match:", (6, 58, 218)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 46, 214, 8), ID_PATTERN),
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"Search all ~subfolders", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (8, 26, 200, 10), ID_SUBFOLDERS),
        item(WC_BUTTON, b"~Find", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 54, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 4, 54, 14), DID_CANCEL as u16),
    ]);
    unsafe { ANSWER = None };
    let owner = view::wins().iter().find(|w| w.desktop).map_or(HWND_DESKTOP, |w| w.frame);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, dlg_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return;
    }
    unsafe {
        WinProcessDlg(dlg);
        WinDestroyWindow(dlg);
    }
    let Some((pattern, subfolders)) = (unsafe { (*&raw mut ANSWER).take() }) else { return };
    let pattern = if pattern.trim_ascii().is_empty() { b"*".to_vec() } else { pattern.trim_ascii().to_vec() };
    let w = view::open_results(b"Find Results - Searching", Vec::new());
    let client = view::wins()[w].client;
    let job = Box::into_raw(Box::new(Job { root, pattern, subfolders, client, found: Vec::new(), dirs: Vec::new() }));
    let mut tid = 0;
    if unsafe { os2::DosCreateThread(&mut tid, worker, job as u32, 0, 64 * 1024) } != 0 {
        unsafe { drop(Box::from_raw(job)) };
    }
}

/// The volume's own files are not objects.
fn hidden_name(n: &[u8]) -> bool {
    n.eq_ignore_ascii_case(b"EA DATA. SF") || n.eq_ignore_ascii_case(b"WP ROOT. SF")
}

extern "C" fn worker(param: u32) {
    let job = unsafe { &mut *(param as *mut Job) };
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let mut todo = alloc::vec![job.root.clone()];
    while let Some(dir) = todo.pop() {
        if job.found.len() >= LIMIT {
            break;
        }
        job.dirs.push(dir.clone());
        for (name, is_dir, _) in fileops::listing(&dir) {
            if hidden_name(&name) {
                continue;
            }
            let p = join(&dir, &name);
            // The real name, or the title a FAT name stands for
            // (.LONGNAME).
            let long = || crate::util::ea_read(&p, b".LONGNAME").is_some_and(|(_, t)| wildcard(&job.pattern, &t));
            if wildcard(&job.pattern, &name) || long() {
                job.found.push(p.clone());
            }
            if is_dir && job.subfolders {
                todo.push(p);
            }
        }
    }
    let client = job.client;
    if unsafe { WinIsWindow(0, client) } != 0 {
        post(client, WM_FIND_DONE, param, 0);
    } else {
        unsafe { drop(Box::from_raw(param as *mut Job)) };
    }
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

/// WM_FIND_DONE: the search is over; the results window shows them.
pub fn done(client: u32, param: u32) {
    let job = unsafe { Box::from_raw(param as *mut Job) };
    let Some(w) = view::find_client(client) else { return };
    let mut objs: Vec<usize> = job.found.iter().map(|p| wps::object_from_path(p)).filter(|&o| o != 0).collect();
    // Abstract objects of the folders searched that are awake.
    for d in &job.dirs {
        let Some(f) = wps::by_path(&crate::util::normalize(d)) else { continue };
        if !wps::is(f, "WPFolder") {
            continue;
        }
        let mut o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, 0, 0]);
        let mut n = 0;
        while o != 0 && n < 10000 {
            n += 1;
            if wps::path_of(o).is_none() && !objs.contains(&o) {
                let t: Vec<u8> = obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
                if wildcard(&job.pattern, &t) {
                    objs.push(o);
                }
            }
            o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, o, 1]);
        }
    }
    let mut title = b"Find Results: ".to_vec();
    title.extend_from_slice(&job.pattern);
    title.extend_from_slice(alloc::format!(" ({} found)", objs.len()).as_bytes());
    if objs.is_empty() {
        message(view::wins()[w].frame, b"No objects were found.", b"Find", MB_OK | MB_INFORMATION);
    }
    view::set_results(w, &title, objs);
}
