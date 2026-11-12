//! The clipboard in folders.
//!
//! **Paste**, as Warp 4's PMWP (studied with Ghidra: fdr_wpFilterMenu,
//! fdr_wpMenuItemSelected, PasteToFolderDlgProc, SetSpecialCaseAtoms,
//! EnumerateObjectClasses, df_wpRenderFromClipboard, ClipboardAddClassAssoc,
//! ClipboardEnumClassAssoc): a folder's Paste... (715) is there whenever
//! the clipboard holds anything. It asks for the new object's name, the
//! clipboard format to take (text, bitmap and metafile by name, the
//! others by their atoms) and the class to make, among those associated
//! with that format (M_WPDataFile's wpclsAddClipboardAssoc and
//! wpclsEnumClipboardClasses: class names kept as system atoms). The
//! object is made in the folder (wpclsNew) and fills itself from the
//! clipboard (wpRenderFromClipboard, which hands the clipboard's handle
//! to wpSetContentsFromHandle): a data file takes text, WPBitmap a bitmap
//! (written as an OS/2 2.0 .BMP), WPMet a metafile. Text is associated
//! with WPDataFile, bitmaps with WPBitmap and metafiles with WPMet.
//!
//! **Objects on the clipboard** (rs2's own; IBM's shell has no object
//! clipboard): in a folder window Ctrl+C (Ctrl+Insert) copies the
//! selected objects to the clipboard and Ctrl+X (Shift+Delete) cuts them;
//! Ctrl+V (Shift+Insert) or Paste... in a folder then copies or moves
//! them there (a cut is pasted once). The clipboard holds them in rs2's
//! format `RS2_WPSObjects` (the operation and the objects' handles) and,
//! for programs that paste text, as CF_TEXT: the objects' full paths (the
//! titles of abstract objects), one a line.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::util::{cstr, z};
use crate::view::{self, wins};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMWIN.793"]
    fn WinOpenClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.707"]
    fn WinCloseClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.733"]
    fn WinEmptyClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.854"]
    fn WinSetClipbrdData(hab: u32, data: u32, fmt: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.806"]
    fn WinQueryClipbrdData(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.739"]
    fn WinEnumClipbrdFmts(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.802"]
    fn WinQueryAtomName(table: u32, atom: u32, buf: *mut u8, len: u32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMGPI.601"]
    fn GpiQueryBitmapInfoHeader(hbm: u32, p: *mut u8) -> u32;
    #[link_name = "PMGPI.599"]
    fn GpiQueryBitmapBits(hps: u32, start: i32, scans: i32, buf: *mut u8, bmi: *mut u8) -> i32;
    #[link_name = "PMGPI.506"]
    fn GpiSetBitmap(hps: u32, hbm: u32) -> u32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, sz: *const i32, fl: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, token: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.365"]
    fn GpiCopyMetaFile(hmf: u32) -> u32;
    #[link_name = "PMGPI.500"]
    fn GpiSaveMetaFile(hmf: u32, name: *const u8) -> u32;
}

pub const CF_TEXT: u32 = 1;
pub const CF_BITMAP: u32 = 2;
pub const CF_DSPTEXT: u32 = 3;
pub const CF_DSPBITMAP: u32 = 4;
pub const CF_METAFILE: u32 = 5;
pub const CF_DSPMETAFILE: u32 = 6;
const CFI_POINTER: u32 = 0x0400;
/// rs2's format for objects on the clipboard.
const OBJECTS_FORMAT: &[u8] = b"RS2_WPSObjects\0";

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn atoms() -> u32 {
    unsafe { WinQuerySystemAtomTable() }
}

fn atom_name(a: u32) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let n = unsafe { WinQueryAtomName(atoms(), a, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    buf[..n.min(255)].iter().copied().take_while(|&c| c != 0).collect()
}

/// Does the clipboard hold anything (Paste is offered)?
pub fn has_data() -> bool {
    let hab = hab();
    if unsafe { WinOpenClipbrd(hab) } == 0 {
        return false;
    }
    let any = unsafe { WinEnumClipbrdFmts(hab, 0) } != 0;
    unsafe { WinCloseClipbrd(hab) };
    any
}

// ---- Clipboard formats and classes (M_WPDataFile) ----

/// (format, class name atom), in the order they were added.
static mut ASSOC: Vec<(u32, u32)> = Vec::new();

#[allow(static_mut_refs)]
fn assoc() -> &'static mut Vec<(u32, u32)> {
    unsafe {
        if ASSOC.is_empty() {
            let t = atoms();
            for (f, c) in [
                (CF_TEXT, &b"WPDataFile\0"[..]),
                (CF_DSPTEXT, b"WPDataFile\0"),
                (CF_BITMAP, b"WPBitmap\0"),
                (CF_DSPBITMAP, b"WPBitmap\0"),
                (CF_METAFILE, b"WPMet\0"),
                (CF_DSPMETAFILE, b"WPMet\0"),
            ] {
                ASSOC.push((f, WinAddAtom(t, c.as_ptr())));
            }
        }
        &mut ASSOC
    }
}

/// wpclsAddClipboardAssoc(aCBFormat, pszClassName): 0 added, 1 there
/// already, 2 failed (IBM's ClipboardAddClassAssoc).
extern "C" fn wpclsAddClipboardAssoc(_cls: usize, fmt: u32, name: *const u8) -> u32 {
    if name.is_null() || fmt == 0 {
        return 2;
    }
    let a = unsafe { WinAddAtom(atoms(), name) };
    if a == 0 {
        return 2;
    }
    if assoc().iter().any(|&(f, c)| f == fmt && c == a) {
        return 1;
    }
    assoc().push((fmt, a));
    0
}

/// wpclsEnumClipboardClasses(aCBFormat, aPrevious): the next class (its
/// name's atom) associated with a format; 0 at the end.
extern "C" fn wpclsEnumClipboardClasses(_cls: usize, fmt: u32, prev: u32) -> u32 {
    let list: Vec<u32> = assoc().iter().filter(|a| a.0 == fmt).map(|a| a.1).collect();
    if prev == 0 {
        return list.first().copied().unwrap_or(0);
    }
    list.iter().position(|&c| c == prev).and_then(|i| list.get(i + 1).copied()).unwrap_or(0)
}

fn classes_for(fmt: u32) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut a = wpclsEnumClipboardClasses(0, fmt, 0);
    while a != 0 && v.len() < 64 {
        v.push(atom_name(a));
        a = wpclsEnumClipboardClasses(0, fmt, a);
    }
    v
}

// ---- Rendering (WPDataFile and the image classes) ----

/// wpRenderFromClipboard(aRenderAs): the clipboard's data in that format
/// becomes the object's contents (wpSetContentsFromHandle).
extern "C" fn wpRenderFromClipboard(o: usize, fmt: u32) -> u32 {
    let hab = hab();
    if unsafe { WinOpenClipbrd(hab) } == 0 {
        return 0;
    }
    let data = unsafe { WinQueryClipbrdData(hab, fmt) };
    let ok = data != 0 && wps::call("WPDataFile", m::WPDataFile::wpSetContentsFromHandle, &[o, data as usize]) != 0;
    unsafe { WinCloseClipbrd(hab) };
    ok as u32
}

fn write_file(path: &[u8], data: &[u8]) -> bool {
    let p = z(path);
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_REPLACE_IF_EXISTS | CREATE_IF_NEW; write only, deny write.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut h, &mut act, 0, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut n = 0u32;
    let rc = unsafe { os2::DosWrite(h, data.as_ptr(), data.len() as u32, &mut n) };
    unsafe { os2::DosClose(h) };
    rc == 0 && n as usize == data.len()
}

/// A data file's contents from a handle: text (a pointer to it).
extern "C" fn df_set_contents_from_handle(o: usize, h: u32) -> u32 {
    let Some(path) = wps::path_of(o) else { return 0 };
    if h == 0 {
        return 0;
    }
    let text = cstr(h as *const u8);
    write_file(&path, text) as u32
}

/// A bitmap's contents from an HBITMAP: an OS/2 2.0 .BMP, 24 bits a
/// pixel.
extern "C" fn bmp_set_contents_from_handle(o: usize, hbm: u32) -> u32 {
    let Some(path) = wps::path_of(o) else { return 0 };
    let mut hdr = [0u8; 64];
    hdr[0..4].copy_from_slice(&64u32.to_le_bytes());
    if unsafe { GpiQueryBitmapInfoHeader(hbm, hdr.as_mut_ptr()) } == 0 {
        return 0;
    }
    let (w, h) = (u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]), u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]));
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return 0;
    }
    // A memory PS to read the bits through (OD_MEMORY; PU_PELS | GPIA_ASSOC).
    let hdc = unsafe { DevOpenDC(hab(), 8, b"*\0".as_ptr(), 0, core::ptr::null(), 0) };
    if hdc == 0 {
        return 0;
    }
    let sz = [0i32, 0];
    let hps = unsafe { GpiCreatePS(hab(), hdc, sz.as_ptr(), 0x0008 | 0x4000) };
    let stride = (w * 3).div_ceil(4) * 4;
    let mut bits = alloc::vec![0u8; (stride * h) as usize];
    // BITMAPINFOHEADER2 (cbFix 16): 24 bits, one plane.
    let mut bmi = [0u8; 16 + 1024];
    bmi[0..4].copy_from_slice(&16u32.to_le_bytes());
    bmi[4..8].copy_from_slice(&w.to_le_bytes());
    bmi[8..12].copy_from_slice(&h.to_le_bytes());
    bmi[12..14].copy_from_slice(&1u16.to_le_bytes());
    bmi[14..16].copy_from_slice(&24u16.to_le_bytes());
    let got = unsafe {
        GpiSetBitmap(hps, hbm);
        let n = GpiQueryBitmapBits(hps, 0, h as i32, bits.as_mut_ptr(), bmi.as_mut_ptr());
        GpiSetBitmap(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
        n
    };
    if got <= 0 {
        return 0;
    }
    // BITMAPFILEHEADER2: usType "BM", cbSize, xHotspot, yHotspot, offBits,
    // then the BITMAPINFOHEADER2 (cbFix 16).
    let off = 14 + 16;
    let mut f = Vec::with_capacity(off + bits.len());
    f.extend_from_slice(b"BM");
    f.extend_from_slice(&((off + bits.len()) as u32).to_le_bytes());
    f.extend_from_slice(&0u16.to_le_bytes());
    f.extend_from_slice(&0u16.to_le_bytes());
    f.extend_from_slice(&(off as u32).to_le_bytes());
    f.extend_from_slice(&bmi[..16]);
    f.extend_from_slice(&bits);
    write_file(&path, &f) as u32
}

/// A metafile's contents from an HMF: a copy saved in its file.
extern "C" fn met_set_contents_from_handle(o: usize, hmf: u32) -> u32 {
    let Some(path) = wps::path_of(o) else { return 0 };
    let copy = unsafe { GpiCopyMetaFile(hmf) };
    if copy == 0 {
        return 0;
    }
    // GpiSaveMetaFile makes the file.
    crate::fileops::delete_path(&path);
    let p = z(&path);
    (unsafe { GpiSaveMetaFile(copy, p.as_ptr()) } != 0) as u32
}

// ---- The Paste dialog ----

const ID_NAME: u16 = 106;
const ID_FORMATS: u16 = 107;
const ID_CLASSES: u16 = 104;
const LM_QUERYITEMCOUNT: u32 = 0x0160;
const LM_SETITEMHANDLE: u32 = 0x0169;
const LM_QUERYITEMHANDLE: u32 = 0x016a;
const LIT_FIRST: u32 = 0xffff_fffd;
const LIT_END: u32 = 0xffff_ffff;

/// The name a format is shown by (IBM's SetSpecialCaseAtoms: the
/// standard ones by name, the others by their atoms).
fn format_name(fmt: u32) -> Vec<u8> {
    match fmt {
        CF_TEXT => b"Text".to_vec(),
        CF_DSPTEXT => b"Text (display)".to_vec(),
        CF_BITMAP => b"Bitmap".to_vec(),
        CF_DSPBITMAP => b"Bitmap (display)".to_vec(),
        CF_METAFILE => b"Metafile".to_vec(),
        CF_DSPMETAFILE => b"Metafile (display)".to_vec(),
        f => {
            let n = atom_name(f);
            if n.is_empty() { alloc::format!("Format {f}").into_bytes() } else { n }
        }
    }
}

fn list_insert(h: u32, id: u16, text: &[u8]) -> u32 {
    let t = z(text);
    unsafe { WinSendDlgItemMsg(h, id as u32, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) }
}

fn selection(h: u32, id: u16) -> Option<u32> {
    let i = unsafe { WinSendDlgItemMsg(h, id as u32, LM_QUERYSELECTION, LIT_FIRST, 0) };
    (i != 0xffff && i != 0xffff_ffff).then_some(i & 0xffff)
}

fn list_text(h: u32, id: u16, i: u32) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let n = unsafe { WinSendDlgItemMsg(h, id as u32, LM_QUERYITEMTEXT, mp(i, buf.len() as u32), buf.as_mut_ptr() as u32) } as usize;
    buf[..n.min(255)].to_vec()
}

/// The classes for the chosen format (IBM's EnumerateObjectClasses).
fn fill_classes(h: u32) {
    unsafe { WinSendDlgItemMsg(h, ID_CLASSES as u32, LM_DELETEALL, 0, 0) };
    let fmt = selection(h, ID_FORMATS).map_or(0, |i| unsafe { WinSendDlgItemMsg(h, ID_FORMATS as u32, LM_QUERYITEMHANDLE, i, 0) });
    let classes = if fmt != 0 { classes_for(fmt) } else { Vec::new() };
    for c in &classes {
        list_insert(h, ID_CLASSES, c);
    }
    if !classes.is_empty() {
        unsafe { WinSendDlgItemMsg(h, ID_CLASSES as u32, LM_SELECTITEM, 0, 1) };
    }
    unsafe { WinEnableWindow(WinWindowFromID(h, DID_OK), !classes.is_empty() as u32) };
}

extern "C" fn paste_dlg_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe {
                WinSetWindowULong(h, QWL_USER, b);
                WinSendDlgItemMsg(h, ID_NAME as u32, EM_SETTEXTLIMIT, 255, 0);
            }
            // The new object's name: a data file's class title, as IBM's.
            let cls = wps::class("WPDataFile");
            let t = wps::call("M_WPObject", m::M_WPObject::wpclsQueryTitle, &[cls]) as *const u8;
            set_item_text(h, ID_NAME as u32, if t.is_null() { b"Data File" } else { cstr(t) });
            let hab = hab();
            let mut first_text = None;
            if unsafe { WinOpenClipbrd(hab) } != 0 {
                let mut f = unsafe { WinEnumClipbrdFmts(hab, 0) };
                while f != 0 {
                    if f != unsafe { WinFindAtom(atoms(), OBJECTS_FORMAT.as_ptr()) } {
                        let i = list_insert(h, ID_FORMATS, &format_name(f));
                        unsafe { WinSendDlgItemMsg(h, ID_FORMATS as u32, LM_SETITEMHANDLE, i, f) };
                        if f == CF_TEXT && first_text.is_none() {
                            first_text = Some(i);
                        }
                    }
                    f = unsafe { WinEnumClipbrdFmts(hab, f) };
                }
                unsafe { WinCloseClipbrd(hab) };
            }
            let n = unsafe { WinSendDlgItemMsg(h, ID_FORMATS as u32, LM_QUERYITEMCOUNT, 0, 0) };
            if n > 0 {
                unsafe { WinSendDlgItemMsg(h, ID_FORMATS as u32, LM_SELECTITEM, first_text.unwrap_or(0), 1) };
            }
            fill_classes(h);
            0
        }
        WM_CONTROL if a & 0xffff == ID_FORMATS as u32 && a >> 16 == LN_SELECT => {
            fill_classes(h);
            0
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            let folder = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let name = item_text(h, ID_NAME as u32);
            let fmt = selection(h, ID_FORMATS).map_or(0, |i| unsafe { WinSendDlgItemMsg(h, ID_FORMATS as u32, LM_QUERYITEMHANDLE, i, 0) });
            let class = selection(h, ID_CLASSES).map(|i| list_text(h, ID_CLASSES, i)).unwrap_or_default();
            if name.is_empty() || fmt == 0 || class.is_empty() {
                unsafe { WinAlarm(HWND_DESKTOP, 2) };
                return 0;
            }
            unsafe { WinDismissDlg(h, DID_OK) };
            paste_as(folder, &name, fmt, &class);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// A new object of a class in the folder, filled from the clipboard (it
/// goes again if it cannot take the data).
fn paste_as(folder: usize, name: &[u8], fmt: u32, class: &[u8]) -> usize {
    let cls = crate::som::find_class(class);
    if cls == 0 {
        return 0;
    }
    let t = z(name);
    let o = wps::call("M_WPObject", m::M_WPObject::wpclsNew, &[cls, t.as_ptr() as usize, b"\0".as_ptr() as usize, folder, 0]);
    if o == 0 {
        return 0;
    }
    if !wps::is(o, "WPDataFile") || wps::call("WPDataFile", m::WPDataFile::wpRenderFromClipboard, &[o, fmt as usize]) == 0 {
        obj::call(m::WPObject::wpDelete, &[o, 0]);
        unsafe { WinAlarm(HWND_DESKTOP, 2) };
        return 0;
    }
    view::changed(o);
    o
}

fn paste_dialog(folder: usize) {
    let lb = |id: u16, (x, y, cx, cy): (i16, i16, i16, i16)| item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, y, cx, cy), id);
    let items = alloc::vec![
        crate::book::label(b"Name of the new object:", (8, 150, 200)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (10, 138, 236, 8), ID_NAME),
        crate::book::label(b"Clipboard format:", (8, 122, 116)),
        lb(ID_FORMATS, (8, 30, 116, 90)),
        crate::book::label(b"Object class:", (132, 122, 116)),
        lb(ID_CLASSES, (132, 30, 116, 90)),
        item(WC_BUTTON, b"~Paste", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (8, 6, 50, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 6, 50, 14), DID_CANCEL as u16),
    ];
    let tpl = template(dialog_frame(b"Paste", 256, 172), items);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, view::desktop_client(), paste_dlg_proc as *const () as u32, tpl.as_ptr(), folder as u32) };
    if dlg == 0 {
        return;
    }
    center(dlg);
    unsafe {
        WinProcessDlg(dlg);
        WinDestroyWindow(dlg);
    }
}

// ---- Objects on the clipboard (rs2's) ----

/// Put objects on the clipboard to be copied (`cut`: moved) by a paste.
pub fn put_objects(objs: &[usize], cut: bool) -> bool {
    if objs.is_empty() {
        return false;
    }
    let mut list = alloc::format!("{}\n", if cut { "CUT" } else { "COPY" }).into_bytes();
    let mut text = Vec::new();
    for &o in objs {
        list.extend_from_slice(alloc::format!("{:08X}\n", obj::handle(o)).as_bytes());
        match wps::path_of(o) {
            Some(p) if wps::is(o, "WPFileSystem") => text.extend_from_slice(&p),
            _ => text.extend_from_slice(&obj::title(o)),
        }
        text.extend_from_slice(b"\r\n");
    }
    let fmt = unsafe { WinAddAtom(atoms(), OBJECTS_FORMAT.as_ptr()) };
    let hab = hab();
    if unsafe { WinOpenClipbrd(hab) } == 0 {
        return false;
    }
    unsafe { WinEmptyClipbrd(hab) };
    for (f, data) in [(fmt, list), (CF_TEXT, text)] {
        if let Some(p) = giveable(&data) {
            unsafe { WinSetClipbrdData(hab, p, f, CFI_POINTER) };
        }
    }
    unsafe { WinCloseClipbrd(hab) };
    true
}

/// Giveable shared memory holding `data` and a NUL.
fn giveable(data: &[u8]) -> Option<u32> {
    let mut p: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), data.len() as u32 + 1, 0x01 | 0x02 | 0x10 | 0x200) } != 0 || p.is_null() {
        return None;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        *p.add(data.len()) = 0;
    }
    Some(p as u32)
}

/// The objects on the clipboard (rs2's format): whether they were cut,
/// and the objects.
fn clipboard_objects() -> Option<(bool, Vec<usize>)> {
    let fmt = unsafe { WinFindAtom(atoms(), OBJECTS_FORMAT.as_ptr()) };
    if fmt == 0 {
        return None;
    }
    let hab = hab();
    if unsafe { WinOpenClipbrd(hab) } == 0 {
        return None;
    }
    let p = unsafe { WinQueryClipbrdData(hab, fmt) };
    let text = if p != 0 { cstr(p as *const u8).to_vec() } else { Vec::new() };
    unsafe { WinCloseClipbrd(hab) };
    let mut lines = text.split(|&c| c == b'\n').filter(|l| !l.is_empty());
    let cut = lines.next()? == b"CUT";
    let objs: Vec<usize> = lines
        .filter_map(|l| u32::from_str_radix(core::str::from_utf8(l).ok()?, 16).ok())
        .map(wps::object_from_handle)
        .filter(|&o| o != 0)
        .collect();
    Some((cut, objs))
}

/// Paste in a folder: objects on the clipboard are copied (or moved,
/// once, when they were cut) into it; anything else goes through the
/// Paste dialog.
pub fn paste(folder: usize) {
    if folder == 0 || !wps::is(folder, "WPFolder") {
        return;
    }
    if let Some((cut, objs)) = clipboard_objects() {
        let objs: Vec<usize> = objs.into_iter().filter(|&o| o != folder && !(cut && obj::folder_of(o) == folder)).collect();
        if objs.is_empty() {
            return;
        }
        let (fs, other): (Vec<usize>, Vec<usize>) = objs.into_iter().partition(|&o| wps::is(o, "WPFileSystem") && wps::is(folder, "WPFileSystem"));
        if !fs.is_empty() {
            let op = if cut { crate::filejob::Op::Move } else { crate::filejob::Op::Copy };
            crate::filejob::transfer(op, &fs, folder, None);
        }
        for o in other {
            if cut {
                obj::call(m::WPObject::wpMoveObject, &[o, folder]);
            } else {
                obj::call(m::WPObject::wpCopyObject, &[o, folder, 0]);
            }
        }
        if cut {
            let hab = hab();
            if unsafe { WinOpenClipbrd(hab) } != 0 {
                unsafe {
                    WinEmptyClipbrd(hab);
                    WinCloseClipbrd(hab);
                }
            }
        }
        return;
    }
    if has_data() {
        paste_dialog(folder);
    }
}

/// Ctrl+C, Ctrl+X, Ctrl+V (and Ctrl+Insert, Shift+Delete, Shift+Insert)
/// in a folder window.
pub fn keys(w: usize, a: u32, b: u32) -> bool {
    const KC_VIRTUALKEY: u32 = 0x0002;
    const KC_SHIFT: u32 = 0x0008;
    const KC_KEYUP: u32 = 0x0040;
    const VK_INSERT: u32 = 0x1a;
    if a & KC_KEYUP != 0 || wins()[w].obj == 0 {
        return false;
    }
    let (ctrl, shift) = (a & KC_CTRL != 0, a & KC_SHIFT != 0);
    let vk = if a & KC_VIRTUALKEY != 0 { b >> 16 } else { 0 };
    let ch = if a & KC_CHAR != 0 { (b & 0xff) as u8 } else { 0 };
    let scan = (a >> 24) as u8;
    let letter = |c: u8, sc: u8| ctrl && !shift && (ch.eq_ignore_ascii_case(&c) || ch == c - b'a' + 1 || (ch == 0 && scan == sc));
    let op = if letter(b'c', 0x2e) || (ctrl && vk == VK_INSERT) {
        1
    } else if letter(b'x', 0x2d) || (shift && !ctrl && vk == VK_DELETE) {
        2
    } else if letter(b'v', 0x2f) || (shift && !ctrl && vk == VK_INSERT) {
        3
    } else {
        return false;
    };
    match op {
        1 | 2 => {
            let sel = view::selected(w, 0);
            let sel: Vec<usize> = sel
                .into_iter()
                .filter(|&o| {
                    let style = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32;
                    style & if op == 1 { obj::OBJSTYLE_NOCOPY } else { obj::OBJSTYLE_NOMOVE } == 0
                })
                .collect();
            if !put_objects(&sel, op == 2) {
                unsafe { WinAlarm(HWND_DESKTOP, 2) };
            }
        }
        _ => paste(wins()[w].obj),
    }
    true
}

methods! {
    __m_wpclsAddClipboardAssoc => wpclsAddClipboardAssoc, 3;
    __m_wpclsEnumClipboardClasses => wpclsEnumClipboardClasses, 3;
    __m_wpRenderFromClipboard => wpRenderFromClipboard, 2;
    __m_df_set_contents_from_handle => df_set_contents_from_handle, 2;
    __m_bmp_set_contents_from_handle => bmp_set_contents_from_handle, 2;
    __m_met_set_contents_from_handle => met_set_contents_from_handle, 2;
}

/// WPDataFile's methods from here.
pub fn data_file_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpRenderFromClipboard", __m_wpRenderFromClipboard as usize),
        ("wpSetContentsFromHandle", __m_df_set_contents_from_handle as usize),
    ]
}

pub fn impl_m_data_file() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![
            ("wpclsAddClipboardAssoc", __m_wpclsAddClipboardAssoc as usize),
            ("wpclsEnumClipboardClasses", __m_wpclsEnumClipboardClasses as usize),
        ],
        overrides: Vec::new(),
    }
}

pub fn impl_bitmap() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpSetContentsFromHandle", __m_bmp_set_contents_from_handle as usize)] }
}

pub fn impl_met() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpSetContentsFromHandle", __m_met_set_contents_from_handle as usize)] }
}
