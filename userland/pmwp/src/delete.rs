//! Deleting objects as IBM's shell does (studied in PMWP.DLL with
//! Ghidra: obj_wpQueryConfirmations, obj/fdr_wpConfirmDelete,
//! obj/fdr_wpDelete, the Shredder's methods):
//!
//! - **Confirmations** are the System object's: `PM_ControlPanel`
//!   ConfirmDelete, ConfirmSubDelete, ConfirmRenameFilesWithExt,
//!   ConfirmKeepAssoc, ConfirmCopyMoveEtc, DisplayProgressInd,
//!   AllowForceDelete and ConfirmForceDelete ("1" or "0"; all on but
//!   ConfirmCopyMoveEtc when not set), returned by wpQueryConfirmations
//!   as CONFIRM_DELETE, CONFIRM_DELETEFOLDER, ...
//! - **Deleting** (the Delete menu item, the Delete key, a drop on the
//!   Shredder) first shows the Delete Object(s) dialog (where they are,
//!   which objects, Confirm on folder delete, Confirm on object delete)
//!   when a confirmation is on; then each object's wpDelete(flags):
//!   wpConfirmDelete asks ("Are you sure you want to delete %0?", for a
//!   folder whether it and all its contents should go, for a shadow
//!   which object it is a shadow of) with Yes, Yes to all (when there
//!   are more), No and Cancel; a folder deletes its contents
//!   (wpDeleteContents, each asked about) before itself.
//! - **The Shredder** (WPShredder, `<WP_SHRED>`): objects dropped on it
//!   are deleted so; other programs' items that offer DRM_DISCARD get
//!   DM_DISCARDOBJECT, as IBM's ShredderThread sends it.

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OBJSTYLE_NODELETE, OBJSTYLE_TEMPLATE};
use crate::pm::*;
use crate::util::{delete_path, join, list_dir, parse_dec, prf_read, prf_write, FILE_DIRECTORY, HINI_USER};
use crate::wps::{self, Impl};
use crate::{abs, dnd, som};

pub const CONFIRM_DELETE: u32 = 0x01;
pub const CONFIRM_DELETEFOLDER: u32 = 0x02;
pub const CONFIRM_RENAMEFILESWITHEXT: u32 = 0x04;
pub const CONFIRM_KEEPASSOC: u32 = 0x08;
pub const CONFIRM_ACTION: u32 = 0x10;
pub const CONFIRM_PROGRESS: u32 = 0x20;

pub const OK_DELETE: u32 = 1;
pub const NO_DELETE: u32 = 2;
pub const CANCEL_DELETE: u32 = 3;

const APP: &[u8] = b"PM_ControlPanel";

/// (key, flag, default) as PMWP reads them.
pub const KEYS: [(&[u8], u32, bool); 8] = [
    (b"ConfirmDelete", CONFIRM_DELETE, true),
    (b"ConfirmSubDelete", CONFIRM_DELETEFOLDER, true),
    (b"ConfirmRenameFilesWithExt", CONFIRM_RENAMEFILESWITHEXT, true),
    (b"ConfirmKeepAssoc", CONFIRM_KEEPASSOC, true),
    (b"ConfirmCopyMoveEtc", CONFIRM_ACTION, false),
    (b"DisplayProgressInd", CONFIRM_PROGRESS, true),
    (b"AllowForceDelete", 0x40, true),
    (b"ConfirmForceDelete", 0x80, true),
];

static mut CONFIRMATIONS: Option<u32> = None;

/// wpQueryConfirmations: the confirmations the System object sets.
pub fn confirmations() -> u32 {
    if let Some(c) = unsafe { CONFIRMATIONS } {
        return c;
    }
    let mut c = 0;
    for (k, f, dflt) in KEYS {
        let on = match prf_read(HINI_USER, APP, k) {
            Some(v) => parse_dec(v.strip_suffix(&[0]).unwrap_or(&v)).unwrap_or(0) != 0,
            None => dflt,
        };
        if on {
            c |= f;
        }
    }
    unsafe { CONFIRMATIONS = Some(c) };
    c
}

pub fn set_confirmation(flag: u32, on: bool) {
    if let Some(&(k, _, _)) = KEYS.iter().find(|e| e.1 == flag) {
        prf_write(HINI_USER, APP, k, Some(if on { b"1\0" } else { b"0\0" }));
    }
    let c = confirmations();
    unsafe { CONFIRMATIONS = Some(if on { c | flag } else { c & !flag }) };
}

// ---- This deletion's own flags (IBM's ShlQueryDeleteFlags) ----

/// Yes to all for folders, Yes to all for objects, more than one object.
const NO_MORE_FOLDERS: u32 = 2;
const NO_MORE_OBJECTS: u32 = 4;
const SEVERAL: u32 = 8;

static mut SESSION: u32 = 0;
/// What this deletion freed (a folder's contents may be in the list too).
static mut DELETED: Vec<usize> = Vec::new();
static mut OWNER: u32 = 0;

fn session() -> u32 {
    unsafe { SESSION }
}

fn owner() -> u32 {
    unsafe { OWNER }
}

// ---- The question ----

const ID_TEXT: u16 = 600;
const ID_YES: u16 = 601;
const ID_YESALL: u16 = 602;
const ID_NO: u16 = 603;

extern "C" fn ask_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            center(h);
            // The focus on No, the default.
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_NO as u32)) };
            1
        }
        WM_COMMAND => {
            unsafe { WinDismissDlg(h, a & 0xffff) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// IBM's four buttons (WinMessageBox2): Yes, Yes to all (only with more
/// objects to come), No (the default) and Cancel. OK_DELETE, NO_DELETE
/// or CANCEL_DELETE; Yes to all also sets `all` in this deletion's flags.
fn ask(text: &[u8], title: &[u8], all: u32) -> u32 {
    let several = session() & SEVERAL != 0;
    let tpl = template(dialog_frame(title, 250, 62), alloc::vec![
        // SS_TEXT | DT_WORDBREAK
        item(WC_STATIC, text, WS_VISIBLE | 0x0001 | 0x4000, (8, 22, 234, 34), ID_TEXT),
        item(WC_BUTTON, b"~Yes", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (6, 4, 54, 14), ID_YES),
        item(WC_BUTTON, b"Yes to ~all", WS_VISIBLE | WS_TABSTOP | if several { 0 } else { WS_DISABLED }, (64, 4, 54, 14), ID_YESALL),
        item(WC_BUTTON, b"~No", WS_VISIBLE | WS_TABSTOP | 0x0400, (122, 4, 54, 14), ID_NO),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (180, 4, 54, 14), DID_CANCEL as u16),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner(), ask_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return CANCEL_DELETE;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    match r as u16 {
        ID_YES => OK_DELETE,
        ID_YESALL => {
            unsafe { SESSION |= all };
            OK_DELETE
        }
        ID_NO => NO_DELETE,
        _ => CANCEL_DELETE,
    }
}

fn sub(text: &[u8], args: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'%' && i + 1 < text.len() && text[i + 1].is_ascii_digit() {
            if let Some(a) = args.get((text[i + 1] - b'0') as usize) {
                out.extend(a.iter().map(|&c| if c == b'\n' { b' ' } else { c }));
            }
            i += 2;
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

fn class_title(o: usize) -> Vec<u8> {
    let cls = som::class_of(o);
    cstr(wps::call("M_WPObject", m::M_WPObject::wpclsQueryTitle, &[cls]) as *const u8).to_vec()
}

// ---- Renaming and dropping (the other confirmations) ----

/// A data file renamed by the user to a name with another extension:
/// with "Confirm on rename of files with extensions" IBM's shell asks
/// whether to keep the extension (Yes adds it back).
pub fn confirm_rename(o: usize, new: &[u8]) -> Vec<u8> {
    if confirmations() & CONFIRM_RENAMEFILESWITHEXT == 0 || !wps::is(o, "WPDataFile") {
        return new.to_vec();
    }
    let Some(p) = wps::path_of(o) else { return new.to_vec() };
    let old = crate::util::extension(&crate::util::split_path(&p).1);
    if old.is_empty() || crate::util::extension(new).eq_ignore_ascii_case(&old) {
        return new.to_vec();
    }
    let text: &[u8] = if wps::is(o, "WPProgramFile") {
        b"Renaming this program without keeping its current extension may prevent this program from running.  Do you want to keep the extension?"
    } else {
        b"Renaming this data file without keeping its current extension may prevent this file from being recognized by the program required to run it.  Do you want to keep the extension?"
    };
    if message(HWND_DESKTOP, text, b"Rename", MB_YESNO | MB_WARNING) != MBID_YES {
        return new.to_vec();
    }
    let mut n = new.to_vec();
    n.push(b'.');
    n.extend(old.iter().map(|c| c.to_ascii_lowercase()));
    n
}

/// "Confirm on copy, move, create shadow": a drop asks first.
pub fn confirm_action(op_copy: bool, o: usize, folder: usize) -> bool {
    if confirmations() & CONFIRM_ACTION == 0 {
        return true;
    }
    let (t, f) = (obj::title(o), obj::title(folder));
    let text = if op_copy { sub(b"Are you sure you want to copy %0 to %1?", &[&t, &f]) } else { sub(b"Are you sure you want to move %0 to %1?", &[&t, &f]) };
    message(HWND_DESKTOP, &text, if op_copy { b"Copy" } else { b"Move" }, MB_YESNO | MB_ICONQUESTION) == MBID_YES
}

// ---- The methods ----

/// WPObject's: "Are you sure you want to delete %0?".
extern "C" fn wpConfirmDelete(o: usize, confirm: u32) -> u32 {
    if confirm & CONFIRM_DELETE == 0 || session() & NO_MORE_OBJECTS != 0 {
        return OK_DELETE;
    }
    let t = obj::title(o);
    let title = sub(b"Confirm on %0 delete", &[&class_title(o).to_ascii_lowercase()]);
    ask(&sub(b"Are you sure you want to delete %0?", &[&t]), &title, NO_MORE_OBJECTS)
}

/// WPFolder's: the folder and all its contents; then (the folder asked
/// about) WPObject's question is not asked again.
extern "C" fn folder_confirm_delete(o: usize, confirm: u32) -> u32 {
    if confirm & CONFIRM_DELETEFOLDER != 0 && session() & NO_MORE_FOLDERS == 0 {
        let t = obj::title(o);
        let text = sub(b"%0 is a folder.  Are you sure you want to delete it and all of its contents?  Select Yes to continue.  Select No to end this task.", &[&t]);
        return ask(&text, b"Confirm on folder delete", NO_MORE_FOLDERS);
    }
    som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpConfirmDelete, &[o, confirm as usize]) as u32
}

/// WPShadow's: which object it is a shadow of (and where that is).
extern "C" fn shadow_confirm_delete(o: usize, confirm: u32) -> u32 {
    if confirm & CONFIRM_DELETE == 0 || session() & NO_MORE_OBJECTS != 0 {
        return OK_DELETE;
    }
    let l = abs::linked(o);
    if l == 0 {
        return som::call_parent(wps::ccd("WPShadow"), wps::cd("WPObject"), m::WPObject::wpConfirmDelete, &[o, confirm as usize]) as u32;
    }
    let (t, lt) = (obj::title(o), obj::title(l));
    let f = obj::folder_of(l);
    let text = if f != 0 {
        sub(b"Are you sure you want to delete %0 which is a shadow of %1 contained in %2?", &[&t, &lt, &obj::title(f)])
    } else {
        sub(b"Are you sure you want to delete %0 which is a shadow of %1?", &[&t, &lt])
    };
    ask(&text, b"Confirm on Shadow delete", NO_MORE_OBJECTS)
}

extern "C" fn wpIsDeleteable(o: usize) -> u32 {
    (obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & OBJSTYLE_NODELETE == 0) as u32
}

/// WPObject's wpDelete: asked (wpConfirmDelete), then freed; an object
/// that cannot go says so.
extern "C" fn wpDelete(o: usize, confirm: u32) -> u32 {
    if confirm & (CONFIRM_DELETE | CONFIRM_DELETEFOLDER | 0x80) != 0 {
        let r = obj::call(m::WPObject::wpConfirmDelete, &[o, confirm as usize]) as u32;
        if r != OK_DELETE {
            return r;
        }
    }
    let style = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32;
    if style & OBJSTYLE_NODELETE == 0 && obj::call(m::WPObject::wpFree, &[o]) != 0 {
        #[allow(static_mut_refs)]
        unsafe { DELETED.push(o) };
        return OK_DELETE;
    }
    let t = obj::title(o);
    let text = sub(b"Cannot delete %0 for one of the following reasons:  The object is non-deleteable, a shadow of the object is non-deleteable or an open view cannot be closed.", &[&t]);
    // MBID_CANCEL ends the deletion.
    if message(owner(), &text, b"Delete", MB_OKCANCEL | MB_WARNING) == 2 { CANCEL_DELETE } else { NO_DELETE }
}

/// WPFolder's wpDelete: asked about once, then its contents (each asked
/// about as an object), then the folder.
extern "C" fn folder_delete(o: usize, confirm: u32) -> u32 {
    let mut confirm = confirm;
    if confirm & (CONFIRM_DELETEFOLDER | 0x80) != 0 {
        let r = obj::call(m::WPObject::wpConfirmDelete, &[o, confirm as usize]) as u32;
        if r != OK_DELETE {
            return r;
        }
        confirm &= !(CONFIRM_DELETEFOLDER | 0x80);
    }
    if obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & OBJSTYLE_NODELETE != 0 {
        return wpDelete(o, 0);
    }
    unsafe { SESSION |= SEVERAL };
    let r = wps::call("WPFolder", m::WPFolder::wpDeleteContents, &[o, confirm as usize]) as u32;
    if r != OK_DELETE {
        return r;
    }
    wpDelete(o, 0)
}

/// wpDeleteContents: every object in the folder (OK_DELETE when all
/// went); hidden files the folder does not show go with it.
extern "C" fn wpDeleteContents(f: usize, confirm: u32) -> u32 {
    let mut list = Vec::new();
    let mut o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, 0, QC_FIRST as usize]);
    while o != 0 {
        list.push(o);
        o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, o, QC_NEXT as usize]);
    }
    let mut all = true;
    for o in list {
        match obj::call(m::WPObject::wpDelete, &[o, confirm as usize]) as u32 {
            OK_DELETE => {}
            CANCEL_DELETE => return CANCEL_DELETE,
            _ => all = false,
        }
    }
    if !all {
        return NO_DELETE;
    }
    if let Some(p) = wps::path_of(f) {
        for (name, attr) in list_dir(&p) {
            if attr & FILE_DIRECTORY == 0 {
                delete_path(&join(&p, &name));
            }
        }
    }
    OK_DELETE
}

const QC_FIRST: u32 = 0;
const QC_NEXT: u32 = 1;

// ---- The Delete Object(s) dialog ----

const ID_LOCATION: u16 = 610;
const ID_LIST: u16 = 611;
const ID_CFOLDER: u16 = 612;
const ID_COBJECT: u16 = 613;

static mut DLG_OBJS: Vec<usize> = Vec::new();
static mut DLG_FLAGS: u32 = 0;

extern "C" fn dialog_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            center(h);
            #[allow(static_mut_refs)]
            let objs = unsafe { DLG_OBJS.clone() };
            let f = objs.first().map_or(0, |&o| obj::folder_of(o));
            let loc = if f != 0 { wps::path_of(f).unwrap_or_else(|| obj::title(f)) } else { Vec::new() };
            set_item_text(h, ID_LOCATION as u32, &loc);
            let lb = unsafe { WinWindowFromID(h, ID_LIST as u32) };
            for &o in &objs {
                let t: Vec<u8> = obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
                let t = z(&t);
                // LIT_END
                send(lb, LM_INSERTITEM, 0xffff, t.as_ptr() as u32);
            }
            let fl = unsafe { DLG_FLAGS };
            unsafe {
                WinSendDlgItemMsg(h, ID_CFOLDER as u32, BM_SETCHECK, (fl & CONFIRM_DELETEFOLDER != 0) as u32, 0);
                WinSendDlgItemMsg(h, ID_COBJECT as u32, BM_SETCHECK, (fl & CONFIRM_DELETE != 0) as u32, 0);
                // The focus on Delete (TRUE: the dialog set it).
                WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, DID_OK));
            }
            1
        }
        WM_COMMAND => {
            let id = a & 0xffff;
            if id == DID_OK {
                let on = |i: u16| unsafe { WinSendDlgItemMsg(h, i as u32, BM_QUERYCHECK, 0, 0) } != 0;
                let mut fl = 0;
                if on(ID_CFOLDER) {
                    fl |= CONFIRM_DELETEFOLDER;
                }
                if on(ID_COBJECT) {
                    fl |= CONFIRM_DELETE;
                }
                unsafe { DLG_FLAGS = fl };
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// The Delete Object(s) dialog; the confirmations the user leaves on,
/// or None for Cancel.
fn dialog(objs: &[usize], flags: u32) -> Option<u32> {
    let check = |t: &[u8], y: i16, id: u16| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | 0x0001, (10, y, 200, 10), id);
    let tpl = template(dialog_frame(b"Delete Object(s)", 240, 150), alloc::vec![
        // SS_GROUPBOX
        item(WC_STATIC, b"From", WS_VISIBLE | 0x0002, (4, 104, 232, 40), 0xffff),
        item(WC_STATIC, b"Location:", WS_VISIBLE | 0x0001, (10, 120, 60, 8), 0xffff),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001, (10, 110, 220, 8), ID_LOCATION),
        item(WC_STATIC, b"Object:", WS_VISIBLE | 0x0001, (6, 92, 60, 8), 0xffff),
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (6, 46, 228, 46), ID_LIST),
        check(b"Confirm on ~folder delete", 32, ID_CFOLDER),
        check(b"Confirm on ~object delete", 22, ID_COBJECT),
        item(WC_BUTTON, b"~Delete", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 54, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 4, 54, 14), DID_CANCEL as u16),
    ]);
    unsafe {
        DLG_OBJS = objs.to_vec();
        DLG_FLAGS = flags;
    }
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner(), dialog_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return None;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    (r == DID_OK).then(|| unsafe { DLG_FLAGS })
}

// ---- Deleting ----

/// Delete objects as the user asked: the dialog (when confirmations are
/// on), then each object's wpDelete.
pub fn delete_objects(owner: u32, objs: &[usize]) {
    if objs.is_empty() {
        return;
    }
    unsafe { OWNER = owner };
    let mut flags = confirmations() & (CONFIRM_DELETE | CONFIRM_DELETEFOLDER);
    if flags != 0 {
        match dialog(objs, flags) {
            Some(f) => flags = f,
            None => return,
        }
    }
    unsafe { SESSION = if objs.len() > 1 { SEVERAL } else { 0 } };
    for &o in objs {
        #[allow(static_mut_refs)]
        if unsafe { DELETED.contains(&o) } {
            continue;
        }
        if obj::call(m::WPObject::wpDelete, &[o, flags as usize]) as u32 == CANCEL_DELETE {
            break;
        }
    }
    unsafe {
        SESSION = 0;
        DELETED.clear();
    }
}

/// Deletions waiting for the shell's thread to be free (a drop on the
/// Shredder is answered first).
static mut PENDING: Vec<(u32, Vec<usize>)> = Vec::new();
pub const WM_DELETE_PENDING: u32 = WM_USER + 40;

pub fn run_pending() {
    #[allow(static_mut_refs)]
    let list = core::mem::take(unsafe { &mut PENDING });
    for (owner, objs) in list {
        delete_objects(owner, &objs);
    }
}

// ---- The Shredder ----

const DRM_DISCARD: &[u8] = b"DRM_DISCARD\0";
const DM_DISCARDOBJECT: u32 = 0x031f;
/// wpDrop's answer: the whole drop is done (no call for the other items).
pub const RC_DROP_DROPCOMPLETE: u32 = 2;

fn discards(it: u32) -> bool {
    unsafe { DrgVerifyRMF(it, DRM_DISCARD.as_ptr(), core::ptr::null()) != 0 }
}

/// Objects that can be deleted move onto it; templates only move (they
/// are not copied there); other programs' items need DRM_DISCARD.
extern "C" fn shredder_drag_over(_o: usize, _cnr: u32, pdinfo: u32) -> u32 {
    let list = dnd::items(pdinfo);
    if list.is_empty() {
        return mp(DOR_NEVERDROP, 0);
    }
    let op = rd(pdinfo, 4) >> 16;
    for it in list {
        let src = dnd::item_object(it);
        if src == 0 {
            if !discards(it) {
                return mp(DOR_NEVERDROP, 0);
            }
            continue;
        }
        if obj::call(m::WPObject::wpIsDeleteable, &[src]) == 0 {
            return mp(DOR_NEVERDROP, 0);
        }
        let style = obj::call(m::WPObject::wpQueryStyle, &[src]) as u32;
        if op == DO_COPY || op == DO_LINK || (op != DO_MOVE && style & OBJSTYLE_TEMPLATE != 0 && op != DO_DEFAULT) {
            return mp(DOR_NODROPOP, 0);
        }
    }
    mp(DOR_DROP, DO_MOVE)
}

/// The whole drop at once: other programs' items are asked to discard
/// themselves (DRR_ABORT from one stops the asking); the objects are
/// deleted once the drop is answered.
extern "C" fn shredder_drop(_o: usize, cnr: u32, pdinfo: u32, _it: u32) -> u32 {
    let mut objs = Vec::new();
    let mut abort = false;
    for it in dnd::items(pdinfo) {
        let src = dnd::item_object(it);
        if src != 0 {
            objs.push(src);
        } else if discards(it) && !abort {
            // DRR_SOURCE 1, DRR_TARGET 2, DRR_ABORT 3.
            if send(rd(it, DI_HWND), DM_DISCARDOBJECT, pdinfo, rd(it, DI_ID)) == 3 {
                abort = true;
            }
        }
    }
    if !objs.is_empty() {
        let owner = if cnr != 0 { unsafe { WinQueryWindow(cnr, 5) } } else { 0 };
        #[allow(static_mut_refs)]
        unsafe { PENDING.push((owner, objs)) };
        post(crate::view::desktop_client(), WM_DELETE_PENDING, 0, 0);
    }
    RC_DROP_DROPCOMPLETE
}

/// IBM's shr_wpFilterMenu: no Create another, Close, Print, Delete or
/// Copy on the Shredder's menu.
extern "C" fn shredder_filter_menu(o: usize, flags: u32, cnr: u32, multi: u32) -> u32 {
    let f = som::call_parent(wps::ccd("WPShredder"), wps::cd("WPObject"), m::WPObject::wpFilterPopupMenu, &[o, flags as usize, cnr as usize, multi as usize]) as u32;
    f & 0xffff_fe56
}

extern "C" fn shredder_open(o: usize, cnr: u32, _view: u32, param: u32) -> u32 {
    // The Shredder has only its settings.
    som::call_parent(wps::ccd("WPShredder"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, obj::OPEN_SETTINGS as usize, param as usize]) as u32
}

extern "C" fn shredder_cls_view(_cls: usize) -> u32 {
    obj::OPEN_SETTINGS
}

methods! {
    __m_wpConfirmDelete => wpConfirmDelete, 2;
    __m_wpDelete => wpDelete, 2;
    __m_wpIsDeleteable => wpIsDeleteable, 1;
    __m_folder_confirm_delete => folder_confirm_delete, 2;
    __m_folder_delete => folder_delete, 2;
    __m_wpDeleteContents => wpDeleteContents, 2;
    __m_shadow_confirm_delete => shadow_confirm_delete, 2;
    __m_shredder_drag_over => shredder_drag_over, 3;
    __m_shredder_drop => shredder_drop, 4;
    __m_shredder_open => shredder_open, 4;
    __m_shredder_filter_menu => shredder_filter_menu, 4;
    __m_shredder_cls_view => shredder_cls_view, 1;
}

pub fn object_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpConfirmDelete", __m_wpConfirmDelete as usize), ("wpDelete", __m_wpDelete as usize), ("wpIsDeleteable", __m_wpIsDeleteable as usize)]
}

pub fn folder_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpDeleteContents", __m_wpDeleteContents as usize)]
}

pub fn folder_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpConfirmDelete", __m_folder_confirm_delete as usize), ("wpDelete", __m_folder_delete as usize)]
}

pub fn shadow_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpConfirmDelete", __m_shadow_confirm_delete as usize)]
}

pub fn impl_shredder() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpDragOver", __m_shredder_drag_over as usize),
            ("wpDrop", __m_shredder_drop as usize),
            ("wpOpen", __m_shredder_open as usize),
            ("wpFilterPopupMenu", __m_shredder_filter_menu as usize),
            crate::pages::no_window_page_override(),
        ],
    }
}

pub fn impl_m_shredder() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpclsQueryDefaultView", __m_shredder_cls_view as usize)] }
}
