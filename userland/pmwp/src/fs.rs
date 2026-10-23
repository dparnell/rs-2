//! File system objects (files and directories; their state in the
//! .CLASSINFO extended attribute): WPFileSystem, WPDataFile (opened with
//! the program associated with its type), WPProgramFile (a program
//! file), WPFolder (a directory and the abstract objects in it), WPDrives
//! (the drives) and the desktop.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::launch;
use crate::methods;
use crate::obj::{self, OPEN_DEFAULT, OPEN_RUNNING, setup_pairs};
use crate::som;
use crate::util::*;
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMWIN.9004"]
    pub fn Rs2LoadFileIcon(path: *const u8, private: u32) -> u32;
}

// ---- WPFileSystem ----

#[repr(C)]
pub struct FsData {
    /// The full path.
    pub real: Str,
    /// Set while the object renames its file itself.
    pub renaming: u32,
}

fn fd<'a>(obj: usize) -> &'a mut FsData {
    som::data(wps::ccd("WPFileSystem"), obj)
}

pub fn path(obj: usize) -> Vec<u8> {
    str_bytes(&fd(obj).real).to_vec()
}

/// The title a path gives an object ("Drive C" for a root, as IBM's
/// root folders are called).
fn title_for(path: &[u8]) -> Vec<u8> {
    if path.len() == 3 {
        return [&b"Drive "[..], &path[..1]].concat();
    }
    split_path(path).1
}

/// Set the path (and the title from it: its .LONGNAME if it has one)
/// without touching the file.
pub fn set_real_name_raw(obj: usize, p: &[u8]) {
    let p = normalize(p);
    fd(obj).real = str_new(&p);
    let title = match ea_read(&p, b".LONGNAME") {
        Some((EAT_ASCII, t)) if !t.is_empty() => t,
        _ => title_for(&p),
    };
    obj::d(obj).title = str_new(&title);
    wps::register(obj, 0, &p);
}

/// An 8.3 name for a long one, not yet used in the directory.
fn short_name(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let keep = |c: &u8| c.is_ascii_alphanumeric() || b"_-$#!@%&".contains(c);
    let (base, ext) = match name.iter().rposition(|&c| c == b'.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, &b""[..]),
    };
    let base: Vec<u8> = base.iter().filter(|c| keep(c)).map(|c| c.to_ascii_uppercase()).collect();
    let ext: Vec<u8> = ext.iter().filter(|c| keep(c)).map(|c| c.to_ascii_uppercase()).take(3).collect();
    let base = if base.is_empty() { b"OBJECT".to_vec() } else { base };
    for n in 0..1000u32 {
        let mut b = base.clone();
        if n > 0 {
            let suffix = alloc::format!("{}", n).into_bytes();
            b.truncate(8 - suffix.len());
            b.extend_from_slice(&suffix);
        } else {
            b.truncate(8);
        }
        if !ext.is_empty() {
            b.push(b'.');
            b.extend_from_slice(&ext);
        }
        if !file_exists(&join(dir, &b)) {
            return b;
        }
    }
    base
}

/// Make the file or directory of a new object (wpclsNew) named after
/// its title in its folder.
pub fn create(obj: usize, folder: usize, title: &[u8]) -> bool {
    debug(&alloc::format!("create: {:x} in {:x} fs {} path {:?}", obj, folder, wps::is(folder, "WPFileSystem"), core::str::from_utf8(&path(folder))));
    if folder == 0 || !wps::is(folder, "WPFileSystem") {
        debug(&alloc::format!("create: folder {:x} is not a file system object", folder));
        return false;
    }
    let name: Vec<u8> = title
        .iter()
        .map(|&c| if c == b'\n' || c == b'\r' { b' ' } else if b"\\/:*?\"<>|".contains(&c) { b'_' } else { c })
        .collect();
    if name.is_empty() {
        return false;
    }
    let dir = path(folder);
    let is_folder = wps::is(obj, "WPFolder");
    let make = |p: &[u8]| if is_folder { is_dir(p) || create_dir(p) } else { file_exists(p) || create_file(p) };
    let mut p = join(&dir, &name);
    let mut ok = make(&p);
    let mut long = false;
    if !ok {
        // A file system that only takes short names (FAT): a short name,
        // and the title in the .LONGNAME extended attribute (as IBM's
        // shell does it).
        p = join(&dir, &short_name(&dir, &name));
        ok = make(&p);
        long = ok;
    }
    if ok && !long {
        // The name as the file system keeps it (FAT: upper case).
        let stored = list_dir(&dir).into_iter().map(|e| e.0).find(|n| eq_nocase(n, &name));
        long = stored.is_some_and(|n| n != name);
    }
    if ok {
        if long {
            ea_write(&p, b".LONGNAME", Some((EAT_ASCII, &name)));
        }
        set_real_name_raw(obj, &p);
    }
    debug(&alloc::format!("create: {:?} {}", core::str::from_utf8(&p), ok));
    ok
}

extern "C" fn wpSetRealName(obj: usize, name: *const u8) -> u32 {
    let name = cstr(name);
    let old = path(obj);
    if old.is_empty() || old.len() == 3 {
        return 0;
    }
    let new = if name.contains(&b'\\') { normalize(name) } else { join(&split_path(&old).0, name) };
    if !eq_nocase(&new, &old) || new != old {
        if !rename_path(&old, &new) {
            return 0;
        }
    }
    ea_write(&new, b".LONGNAME", None);
    set_real_name_raw(obj, &new);
    if obj::d(obj).handle != 0 {
        wps::move_fs_handle(obj::d(obj).handle, &new);
    }
    subtree_moved(&old, &new);
    1
}

extern "C" fn wpQueryRealName(obj: usize, buf: *mut u8, cb: *mut u32, qualified: u32) -> u32 {
    let p = path(obj);
    let s = if qualified != 0 { p } else { split_path(&p).1 };
    if cb.is_null() {
        return 0;
    }
    unsafe {
        if buf.is_null() || (*cb as usize) < s.len() + 1 {
            *cb = (s.len() + 1) as u32;
            return 0;
        }
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
        *cb = (s.len() + 1) as u32;
    }
    1
}

/// Renaming the object renames its file.
extern "C" fn fs_set_title(obj: usize, title: *const u8) -> u32 {
    let t = cstr(title);
    let p = path(obj);
    if !obj::restoring(obj) && p.len() > 3 && t != str_bytes(&obj::d(obj).title) {
        let n = z(t);
        if wpSetRealName(obj, n.as_ptr()) != 0 {
            return 1;
        }
        // A name the file system does not take: the title is kept in
        // .LONGNAME.
        if !ea_write(&p, b".LONGNAME", Some((EAT_ASCII, t))) {
            return 0;
        }
    }
    som::call_parent(wps::ccd("WPFileSystem"), wps::cd("WPObject"), m::WPObject::wpSetTitle, &[obj, title as usize]) as u32
}

extern "C" fn fs_query_handle(obj: usize) -> u32 {
    let od = obj::d(obj);
    if od.handle == 0 {
        let p = path(obj);
        if !p.is_empty() {
            od.handle = wps::fs_handle(&p);
            wps::register(obj, od.handle, &p);
        }
    }
    obj::d(obj).handle
}

extern "C" fn fs_query_icon(obj: usize) -> u32 {
    let od = obj::d(obj);
    // A folder has its own icon only through its .ICON EA (or one set up
    // for it); otherwise the one for its object ID or its class's.
    if od.icon == 0 && wps::is(obj, "WPFolder") && ea_read(&path(obj), b".ICON").is_none() {
        let i = crate::icons::of_object_id(&obj::object_id(obj));
        if i != 0 {
            return i;
        }
        return som::call_parent(wps::ccd("WPFileSystem"), wps::cd("WPObject"), m::WPObject::wpQueryIcon, &[obj]) as u32;
    }
    if od.icon == 0 {
        let p = z(&path(obj));
        od.icon = unsafe { Rs2LoadFileIcon(p.as_ptr(), 0) };
    }
    if obj::d(obj).icon != 0 {
        return obj::d(obj).icon;
    }
    som::call_parent(wps::ccd("WPFileSystem"), wps::cd("WPObject"), m::WPObject::wpQueryIcon, &[obj]) as u32
}

/// A file system object's wpCopyObject and wpMoveObject copy and move
/// its file.
extern "C" fn fs_move(obj: usize, folder: usize) -> u32 {
    if folder == 0 || !wps::is(folder, "WPFileSystem") {
        return 0;
    }
    let old = path(obj);
    let new = join(&path(folder), &split_path(&old).1);
    if !rename_path(&old, &new) {
        return 0;
    }
    moved(obj, folder, &new);
    1
}

/// An object whose file has moved to `new` (into `folder`): its path,
/// handle and folder follow, and so do the objects in it (a directory).
pub fn moved(obj: usize, folder: usize, new: &[u8]) {
    let old = path(obj);
    // An object that was at the target (replaced) is gone.
    if let Some(o) = wps::by_path(&normalize(new)) {
        if o != obj {
            let f = obj::d(o).folder;
            if f != 0 {
                wps::call("WPFolder", m::WPFolder::wpDeleteFromContent, &[f, o]);
            }
            wps::unregister(o);
        }
    }
    let from = obj::d(obj).folder;
    if from != 0 {
        wps::call("WPFolder", m::WPFolder::wpDeleteFromContent, &[from, obj]);
    }
    set_real_name_raw(obj, new);
    if obj::d(obj).handle != 0 {
        wps::move_fs_handle(obj::d(obj).handle, new);
    }
    subtree_moved(&old, new);
    obj::d(obj).folder = folder;
    if folder != 0 {
        wps::call("WPFolder", m::WPFolder::wpAddToContent, &[folder, obj]);
    }
}

/// The objects in a directory that moved: their paths and handles follow.
pub fn subtree_moved(old: &[u8], new: &[u8]) {
    let mut pre = normalize(old);
    pre.push(b'\\');
    let list: Vec<(usize, Vec<u8>)> =
        wps::awake().iter().filter(|e| e.path.len() > pre.len() && eq_nocase(&e.path[..pre.len()], &pre)).map(|e| (e.obj, e.path.clone())).collect();
    for (o, p) in list {
        let np = join(new, &p[pre.len()..]);
        set_real_name_raw(o, &np);
    }
    wps::fs_handles_moved(old, new);
}

extern "C" fn fs_copy(obj: usize, folder: usize, _lock: u32) -> usize {
    if folder == 0 || !wps::is(folder, "WPFileSystem") {
        return 0;
    }
    let old = path(obj);
    let new = join(&path(folder), &split_path(&old).1);
    let (f, t) = (z(&old), z(&new));
    if unsafe { os2::DosCopy(f.as_ptr(), t.as_ptr(), 0) } != 0 {
        return 0;
    }
    let o = wps::object_from_path(&new);
    if o != 0 {
        wps::call("WPFolder", m::WPFolder::wpAddToContent, &[folder, o]);
    }
    o
}

extern "C" fn fs_uninit(obj: usize) {
    fd(obj).real = None;
    som::call_parent(wps::ccd("WPFileSystem"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[obj]);
}

extern "C" fn wpclsQueryObjectFromPath(_cls: usize, p: *const u8) -> usize {
    wps::object_from_path(cstr(p))
}

// ---- WPDataFile ----

/// The program objects associated with a file: OS2.INI's
/// PMWP_ASSOC_FILTER (key: a pattern like "*.TXT"; value: handles in
/// decimal, NUL separated), as IBM's shell keeps them.
fn associations(name: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let up = name.to_ascii_uppercase();
    for k in prf_keys(HINI_USER, b"PMWP_ASSOC_FILTER") {
        let pat = k.to_ascii_uppercase();
        let ok = match pat.strip_prefix(b"*") {
            Some(suffix) => up.ends_with(suffix),
            None => pat == up,
        };
        if !ok {
            continue;
        }
        if let Some(v) = prf_read(HINI_USER, b"PMWP_ASSOC_FILTER", &k) {
            out.extend(v.split(|&c| c == 0).filter_map(parse_dec));
        }
    }
    out
}

extern "C" fn df_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_RUNNING || view == OPEN_DEFAULT {
        let p = path(obj);
        for h in associations(&split_path(&p).1) {
            let prog = wps::object_from_handle(h);
            if prog != 0 && wps::is(prog, "WPProgram") {
                let mut buf = alloc::vec![0u8; 1024];
                let mut cb = buf.len() as u32;
                if wps::call("WPProgram", m::WPProgram::wpQueryProgDetails, &[prog, buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize])
                    != 0
                {
                    let w = buf.as_ptr() as *const u32;
                    let s = |i: usize| unsafe { cstr(*w.add(i) as *const u8).to_vec() };
                    let mut params = s(5);
                    if !params.is_empty() {
                        params.push(b' ');
                    }
                    params.extend_from_slice(&p);
                    launch::start(&s(4), &params, &s(6), &s(3), unsafe { *w.add(1) });
                    return 0;
                }
            }
        }
        // No association: the System Editor.
        launch::start(b"E.EXE", &p, b"", b"", launch::PROG_PM);
        return 0;
    }
    som::call_parent(wps::ccd("WPDataFile"), wps::cd("WPObject"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize])
        as u32
}

// ---- WPProgramFile ----

extern "C" fn pf_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_RUNNING || view == OPEN_DEFAULT {
        let p = path(obj);
        launch::start(&p, b"", &split_path(&p).0, b"", launch::PROG_DEFAULT);
        return 0;
    }
    som::call_parent(wps::ccd("WPProgramFile"), wps::cd("WPObject"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize])
        as u32
}

extern "C" fn pf_query_prog_details(obj: usize, buf: *mut u8, cb: *mut u32) -> u32 {
    let p = path(obj);
    crate::abs::fill_prog_details(&obj::title(obj), &p, b"", &split_path(&p).0, launch::PROG_DEFAULT, buf, cb)
}

// ---- WPFolder ----

pub const QC_FIRST: u32 = 0;
pub const QC_NEXT: u32 = 1;
pub const QC_LAST: u32 = 2;

#[repr(C)]
pub struct FolderData {
    pub content: Option<alloc::boxed::Box<Vec<usize>>>,
    pub populated: u32,
    pub flags: u32,
    /// The background colour (RGB), if the folder has its own.
    pub bg: u32,
    pub has_bg: u32,
    /// The sort attribute (sortinc.rs) and "always maintain sort order".
    pub sort_by: u32,
    pub always_sort: u32,
    /// The Include page's patterns (empty: everything).
    pub include: Option<alloc::boxed::Box<Vec<u8>>>,
}

/// (sort attribute, always maintain sort order).
pub fn sort_of(obj: usize) -> (u32, bool) {
    if obj == 0 || !wps::is(obj, "WPFolder") {
        return (0, false);
    }
    (fo(obj).sort_by, fo(obj).always_sort != 0)
}

pub fn set_sort(obj: usize, by: u32, always: bool) {
    fo(obj).sort_by = by;
    fo(obj).always_sort = always as u32;
    obj::call(m::WPObject::wpSaveDeferred, &[obj]);
}

pub fn include_of(obj: usize) -> Vec<u8> {
    if obj == 0 || !wps::is(obj, "WPFolder") {
        return Vec::new();
    }
    fo(obj).include.as_ref().map(|b| (**b).clone()).unwrap_or_default()
}

pub fn set_include(obj: usize, pats: &[u8]) {
    fo(obj).include = (!pats.is_empty()).then(|| alloc::boxed::Box::new(pats.to_vec()));
    obj::call(m::WPObject::wpSaveDeferred, &[obj]);
}

/// A folder's setup: ALWAYSSORT=YES|NO (as INI.RC uses it).
extern "C" fn folder_setup(obj: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpSetup, &[obj, setup as usize]);
    for (k, v) in setup_pairs(cstr(setup)) {
        if k == b"ALWAYSSORT" {
            let on = v.eq_ignore_ascii_case(b"YES");
            let f = fo(obj);
            f.always_sort = on as u32;
            if on && f.sort_by == 0 {
                f.sort_by = crate::sortinc::SORT_NAME;
            }
        }
    }
    1
}

pub fn folder_background(obj: usize) -> Option<u32> {
    if obj == 0 || !wps::is(obj, "WPFolder") {
        return None;
    }
    let f = fo(obj);
    (f.has_bg != 0).then_some(f.bg)
}

/// Set (or with None drop) a folder's own background colour.
pub fn set_folder_background(obj: usize, rgb: Option<u32>) {
    let f = fo(obj);
    f.has_bg = rgb.is_some() as u32;
    f.bg = rgb.unwrap_or(0);
    obj::call(m::WPObject::wpSaveDeferred, &[obj]);
    crate::view::background_changed(obj);
}

extern "C" fn folder_save_state(obj: usize) -> u32 {
    som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[obj]);
    if fo(obj).has_bg != 0 {
        obj::wpSaveLong(obj, b"WPFolder\0".as_ptr(), 1, fo(obj).bg);
    }
    let f = fo(obj);
    if f.sort_by != 0 || f.always_sort != 0 {
        obj::wpSaveLong(obj, b"WPFolder\0".as_ptr(), 2, f.sort_by | f.always_sort << 16);
    }
    if let Some(p) = &f.include {
        let s = z(p);
        obj::wpSaveString(obj, b"WPFolder\0".as_ptr(), 3, s.as_ptr());
    }
    1
}

extern "C" fn folder_restore_state(obj: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[obj, r as usize]);
    if let Some(v) = obj::restored_long(obj, b"WPFolder", 1) {
        fo(obj).bg = v;
        fo(obj).has_bg = 1;
    }
    if let Some(v) = obj::restored_long(obj, b"WPFolder", 2) {
        fo(obj).sort_by = v & 0xffff;
        fo(obj).always_sort = v >> 16;
    }
    if let Some(p) = obj::restored_string(obj, b"WPFolder", 3) {
        let p = p.strip_suffix(&[0]).unwrap_or(&p).to_vec();
        fo(obj).include = (!p.is_empty()).then(|| alloc::boxed::Box::new(p));
    }
    1
}

fn fo<'a>(obj: usize) -> &'a mut FolderData {
    som::data(wps::ccd("WPFolder"), obj)
}

fn content<'a>(obj: usize) -> &'a mut Vec<usize> {
    fo(obj).content.get_or_insert_with(|| alloc::boxed::Box::new(Vec::new()))
}

/// The objects of the folder: its directory's files and directories,
/// and the abstract objects kept as its content.
extern "C" fn wpPopulate(obj: usize, _reserved: u32, _path: *const u8, folders_only: u32) -> u32 {
    let p = path(obj);
    let mut list: Vec<usize> = Vec::new();
    if !p.is_empty() {
        for (name, attr) in list_dir(&p) {
            if folders_only != 0 && attr & FILE_DIRECTORY == 0 {
                continue;
            }
            // Hidden files (FAT's EA DATA. SF among them) are not shown,
            // as in IBM's folders by default.
            if attr & 0x02 != 0 {
                continue;
            }
            let o = wps::object_from_path(&join(&p, &name));
            if o != 0 && !list.contains(&o) {
                list.push(o);
            }
        }
    }
    if folders_only == 0 {
        let h = obj::handle(obj);
        for ah in wps::abstract_content(h) {
            let o = wps::object_from_handle(ah);
            if o != 0 && !list.contains(&o) {
                list.push(o);
            }
        }
    }
    for &o in &list {
        obj::d(o).folder = obj;
    }
    *content(obj) = list;
    fo(obj).populated = if folders_only != 0 { 1 } else { 2 };
    1
}

fn ensure_populated(obj: usize) {
    if fo(obj).populated < 2 {
        wps::call("WPFolder", m::WPFolder::wpPopulate, &[obj, 0, 0, 0]);
    }
}

extern "C" fn wpQueryContent(obj: usize, prev: usize, option: u32) -> usize {
    ensure_populated(obj);
    let c = content(obj);
    match option {
        QC_FIRST => c.first().copied().unwrap_or(0),
        QC_LAST => c.last().copied().unwrap_or(0),
        _ => c.iter().position(|&o| o == prev).and_then(|i| c.get(i + 1).copied()).unwrap_or(0),
    }
}

extern "C" fn wpAddToContent(obj: usize, o: usize) -> u32 {
    if fo(obj).populated != 0 {
        let c = content(obj);
        if !c.contains(&o) {
            c.push(o);
        }
    }
    crate::view::added(obj, o);
    1
}

extern "C" fn wpDeleteFromContent(obj: usize, o: usize) -> u32 {
    content(obj).retain(|&x| x != o);
    crate::view::removed(obj, o);
    1
}

extern "C" fn wpSetFldrFlags(obj: usize, f: u32) -> u32 {
    fo(obj).flags = f;
    1
}

extern "C" fn wpQueryFldrFlags(obj: usize) -> u32 {
    fo(obj).flags
}

extern "C" fn folder_uninit(obj: usize) {
    fo(obj).content = None;
    fo(obj).include = None;
    som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[obj]);
}

extern "C" fn folder_query_default_view(obj: usize) -> u32 {
    som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpQueryDefaultView, &[obj]) as u32
}

extern "C" fn folder_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == obj::OPEN_SETTINGS {
        return som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize])
            as u32;
    }
    crate::view::open_folder(obj, cnr, view, param)
}

// ---- WPDrives ----

/// The drives: a WPDisk for each drive there is (made when missing).
extern "C" fn drives_populate(obj: usize, r: u32, p: *const u8, folders_only: u32) -> u32 {
    som::call_parent(wps::ccd("WPDrives"), wps::cd("WPFolder"), m::WPFolder::wpPopulate, &[obj, r as usize, p as usize, folders_only as usize]);
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut cur, &mut map) };
    let have: Vec<u32> = content(obj)
        .iter()
        .filter(|&&o| wps::is(o, "WPDisk"))
        .map(|&o| wps::call("WPDisk", m::WPDisk::wpQueryLogicalDrive, &[o]) as u32)
        .collect();
    let cls = wps::class("WPDisk");
    for d in 1..=26u32 {
        if map & (1 << (d - 1)) == 0 || have.contains(&d) {
            continue;
        }
        let letter = b'A' + d as u8 - 1;
        let title = z(alloc::format!("Drive {}", letter as char).as_bytes());
        let setup = z(alloc::format!("DRIVE={};NOMOVE=YES;NOCOPY=YES;NODELETE=YES", letter as char).as_bytes());
        wps::call("M_WPObject", m::M_WPObject::wpclsNew, &[cls, title.as_ptr() as usize, setup.as_ptr() as usize, obj, 0]);
    }
    1
}

// ---- The desktop ----

/// The desktop folder: <WP_DESKTOP>, or the boot drive's DESKTOP
/// directory made a WPDesktop.
pub fn desktop() -> usize {
    if let Some(h) = wps::handle_of_id(b"<WP_DESKTOP>") {
        let o = wps::object_from_handle(h);
        if o != 0 {
            return o;
        }
    }
    let mut boot = 3u32;
    unsafe { os2::DosQuerySysInfo(5, 5, &mut boot, 4) };
    let p = alloc::vec![b'A' + boot as u8 - 1, b':', b'\\', b'D', b'E', b'S', b'K', b'T', b'O', b'P'];
    if !is_dir(&p) && !create_dir(&p) {
        return 0;
    }
    if ea_read(&p, b".CLASSINFO").is_none() {
        ea_write(&p, b".CLASSINFO", Some((EAT_BINARY, &wps::encode(b"WPDesktop", 0, &[]))));
        ea_write(&p, b".LONGNAME", Some((EAT_ASCII, b"Desktop")));
    }
    let o = wps::object_from_path(&p);
    if o != 0 {
        let id = z(b"<WP_DESKTOP>");
        obj::call(m::WPObject::wpSetObjectID, &[o, id.as_ptr() as usize]);
    }
    o
}

methods! {
    __m_wpSetRealName => wpSetRealName, 2;
    __m_wpQueryRealName => wpQueryRealName, 4;
    __m_fs_set_title => fs_set_title, 2;
    __m_fs_query_handle => fs_query_handle, 1;
    __m_fs_query_icon => fs_query_icon, 1;
    __m_fs_move => fs_move, 2;
    __m_fs_copy => fs_copy, 3;
    __m_fs_uninit => fs_uninit, 1;
    __m_wpclsQueryObjectFromPath => wpclsQueryObjectFromPath, 2;
    __m_df_open => df_open, 4;
    __m_pf_open => pf_open, 4;
    __m_pf_query_prog_details => pf_query_prog_details, 3;
    __m_wpPopulate => wpPopulate, 4;
    __m_wpQueryContent => wpQueryContent, 3;
    __m_wpAddToContent => wpAddToContent, 2;
    __m_wpDeleteFromContent => wpDeleteFromContent, 2;
    __m_wpSetFldrFlags => wpSetFldrFlags, 2;
    __m_wpQueryFldrFlags => wpQueryFldrFlags, 1;
    __m_folder_uninit => folder_uninit, 1;
    __m_folder_query_default_view => folder_query_default_view, 1;
    __m_folder_open => folder_open, 4;
    __m_drives_populate => drives_populate, 4;
    __m_folder_save_state => folder_save_state, 1;
    __m_folder_setup => folder_setup, 2;
    __m_folder_restore_state => folder_restore_state, 2;
}

pub fn impl_file_system() -> Impl {
    Impl {
        data: core::mem::size_of::<FsData>() as u32,
        methods: alloc::vec![("wpSetRealName", __m_wpSetRealName as usize), ("wpQueryRealName", __m_wpQueryRealName as usize)],
        overrides: alloc::vec![
            ("wpSetTitle", __m_fs_set_title as usize),
            ("wpQueryHandle", __m_fs_query_handle as usize),
            ("wpQueryIcon", __m_fs_query_icon as usize),
            ("wpMoveObject", __m_fs_move as usize),
            ("wpCopyObject", __m_fs_copy as usize),
            ("wpUnInitData", __m_fs_uninit as usize),
        ],
    }
}

pub fn impl_m_file_system() -> Impl {
    Impl { data: 0, methods: alloc::vec![("wpclsQueryObjectFromPath", __m_wpclsQueryObjectFromPath as usize)], overrides: Vec::new() }
}

pub fn impl_data_file() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_df_open as usize)] }
}

pub fn impl_program_file() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![("wpQueryProgDetails", __m_pf_query_prog_details as usize)],
        overrides: [("wpOpen", __m_pf_open as usize)].into_iter().chain(crate::dnd::overrides("WPProgramFile")).collect(),
    }
}

pub fn impl_folder() -> Impl {
    Impl {
        data: core::mem::size_of::<FolderData>() as u32,
        methods: alloc::vec![
            ("wpPopulate", __m_wpPopulate as usize),
            ("wpQueryContent", __m_wpQueryContent as usize),
            ("wpAddToContent", __m_wpAddToContent as usize),
            ("wpDeleteFromContent", __m_wpDeleteFromContent as usize),
            ("wpSetFldrFlags", __m_wpSetFldrFlags as usize),
            ("wpQueryFldrFlags", __m_wpQueryFldrFlags as usize),
            ("wpQueryDefaultView", __m_folder_query_default_view as usize),
        ]
        .into_iter()
        .chain(crate::book::folder_methods())
        .chain(crate::sortinc::folder_methods())
        .collect(),
        overrides: [
            ("wpUnInitData", __m_folder_uninit as usize),
            ("wpOpen", __m_folder_open as usize),
            ("wpSaveState", __m_folder_save_state as usize),
            ("wpRestoreState", __m_folder_restore_state as usize),
            ("wpSetup", __m_folder_setup as usize),
        ]
        .into_iter()
        .chain(crate::dnd::overrides("WPFolder"))
        .chain(crate::book::folder_overrides())
        .collect(),
    }
}

pub fn impl_drives() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpPopulate", __m_drives_populate as usize)] }
}

pub fn _setup_unused() -> usize {
    setup_pairs(b"").len()
}
