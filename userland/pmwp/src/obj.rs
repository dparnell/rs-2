//! WPObject and M_WPObject: what every Workplace Shell object has (title,
//! style, icon, folder, default view, handle, object ID), setup strings,
//! and saving and restoring state (wpSaveState/wpRestoreState write and
//! read values with wpSaveLong/String/Data and wpRestoreLong/String/Data;
//! where they are kept depends on the kind of object, see wps.rs).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::som;
use crate::util::*;
use crate::wps::{self, Impl, Rec};

unsafe extern "C" {
    #[link_name = "PMWIN.828"]
    fn WinQuerySysPointer(desk: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.780"]
    fn WinLoadPointer(desk: u32, hmod: u32, id: u32) -> u32;
    #[link_name = "PMWIN.9004"]
    fn Rs2LoadFileIcon(path: *const u8, private: u32) -> u32;
}

pub const OBJSTYLE_NOMOVE: u32 = 0x2;
pub const OBJSTYLE_NOLINK: u32 = 0x4;
pub const OBJSTYLE_NOCOPY: u32 = 0x8;
pub const OBJSTYLE_TEMPLATE: u32 = 0x20;
pub const OBJSTYLE_NODELETE: u32 = 0x40;
pub const OBJSTYLE_NOPRINT: u32 = 0x80;
pub const OBJSTYLE_NODRAG: u32 = 0x100;
pub const OBJSTYLE_NOTVISIBLE: u32 = 0x200;
pub const OBJSTYLE_NOSETTINGS: u32 = 0x400;
pub const OBJSTYLE_NORENAME: u32 = 0x800;
pub const OBJSTYLE_NODROP: u32 = 0x1000;
pub const OBJSTYLE_LOCKEDINPLACE: u32 = 0x20000;

pub const OPEN_DEFAULT: u32 = 0;
pub const OPEN_CONTENTS: u32 = 1;
pub const OPEN_SETTINGS: u32 = 2;
pub const OPEN_RUNNING: u32 = 4;
pub const OPEN_TREE: u32 = 101;
pub const OPEN_DETAILS: u32 = 102;

const F_RESTORING: u32 = 1;
const F_DELETED: u32 = 2;

/// WPObject's instance data.
#[repr(C)]
pub struct Data {
    pub title: Str,
    pub style: u32,
    pub icon: u32,
    pub folder: usize,
    pub default_view: u32,
    pub handle: u32,
    pub object_id: Str,
    pub locks: u32,
    pub flags: u32,
    pub state: Option<Box<Vec<Rec>>>,
    pub help_panel: u32,
    /// CCVIEW_DEFAULT (the System object's choice), CCVIEW_ON, CCVIEW_OFF.
    pub ccview: u32,
}

pub const CCVIEW_DEFAULT: u32 = 0;
pub const CCVIEW_ON: u32 = 1;
pub const CCVIEW_OFF: u32 = 2;

pub fn d<'a>(obj: usize) -> &'a mut Data {
    som::data(wps::ccd("WPObject"), obj)
}

pub fn call(i: usize, args: &[usize]) -> usize {
    wps::call("WPObject", i, args)
}

pub fn folder_of(obj: usize) -> usize {
    d(obj).folder
}

/// The handle (made if the object has none).
pub fn handle(obj: usize) -> u32 {
    call(m::WPObject::wpQueryHandle, &[obj]) as u32
}

pub fn set_handle(obj: usize, h: u32) {
    d(obj).handle = h;
}

pub fn object_id(obj: usize) -> Vec<u8> {
    str_bytes(&d(obj).object_id).to_vec()
}

pub fn title(obj: usize) -> Vec<u8> {
    cstr(call(m::WPObject::wpQueryTitle, &[obj]) as *const u8).to_vec()
}

pub fn restoring(obj: usize) -> bool {
    d(obj).flags & F_RESTORING != 0
}

/// Bring a woken object's state back (wpRestoreState through the
/// object's class, so every class restores its part).
pub fn wake(obj: usize, folder: usize, recs: Vec<Rec>) {
    let dd = d(obj);
    dd.folder = folder;
    dd.state = Some(Box::new(recs));
    dd.flags |= F_RESTORING;
    call(m::WPObject::wpRestoreState, &[obj, 0]);
    d(obj).flags &= !F_RESTORING;
}

// ---- Saved values ----

fn rec_find<'a>(obj: usize, class: *const u8, key: u32) -> Option<&'a Rec> {
    let c = cstr(class);
    d(obj).state.as_ref()?.iter().find(|r| r.key == key && eq_nocase(&r.class, c))
}

fn rec_put(obj: usize, class: *const u8, key: u32, data: &[u8]) {
    let c = cstr(class).to_vec();
    let st = d(obj).state.get_or_insert_with(|| Box::new(Vec::new()));
    st.retain(|r| !(r.key == key && eq_nocase(&r.class, &c)));
    st.push(Rec { class: c, key, data: data.to_vec() });
}

pub extern "C" fn wpSaveLong(obj: usize, class: *const u8, key: u32, v: u32) -> u32 {
    rec_put(obj, class, key, &v.to_le_bytes());
    1
}

pub extern "C" fn wpSaveString(obj: usize, class: *const u8, key: u32, s: *const u8) -> u32 {
    rec_put(obj, class, key, &z(cstr(s)));
    1
}

pub extern "C" fn wpSaveData(obj: usize, class: *const u8, key: u32, p: *const u8, cb: u32) -> u32 {
    let data = if p.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(p, cb as usize) } };
    rec_put(obj, class, key, data);
    1
}

pub extern "C" fn wpRestoreLong(obj: usize, class: *const u8, key: u32, v: *mut u32) -> u32 {
    match rec_find(obj, class, key) {
        Some(r) if r.data.len() >= 4 => {
            if !v.is_null() {
                unsafe { *v = u32::from_le_bytes([r.data[0], r.data[1], r.data[2], r.data[3]]) };
            }
            1
        }
        _ => 0,
    }
}

/// Copy saved data to a buffer (or tell its size when the buffer is
/// null), as wpRestoreString and wpRestoreData do.
fn restore_bytes(obj: usize, class: *const u8, key: u32, buf: *mut u8, cb: *mut u32) -> u32 {
    let Some(r) = rec_find(obj, class, key) else { return 0 };
    if cb.is_null() {
        return 0;
    }
    unsafe {
        if buf.is_null() {
            *cb = r.data.len() as u32;
            return 1;
        }
        if (*cb as usize) < r.data.len() {
            *cb = r.data.len() as u32;
            return 0;
        }
        core::ptr::copy_nonoverlapping(r.data.as_ptr(), buf, r.data.len());
        *cb = r.data.len() as u32;
    }
    1
}

pub extern "C" fn wpRestoreString(obj: usize, class: *const u8, key: u32, buf: *mut u8, cb: *mut u32) -> u32 {
    restore_bytes(obj, class, key, buf, cb)
}

pub extern "C" fn wpRestoreData(obj: usize, class: *const u8, key: u32, buf: *mut u8, cb: *mut u32) -> u32 {
    restore_bytes(obj, class, key, buf, cb)
}

/// A string saved under (class, key), if there is one.
pub fn restored_string(obj: usize, class: &[u8], key: u32) -> Option<Vec<u8>> {
    let c = z(class);
    let r = rec_find(obj, c.as_ptr(), key)?;
    Some(r.data.strip_suffix(&[0]).unwrap_or(&r.data).to_vec())
}

pub fn restored_long(obj: usize, class: &[u8], key: u32) -> Option<u32> {
    let c = z(class);
    let mut v = 0;
    (wpRestoreLong(obj, c.as_ptr(), key, &mut v) != 0).then_some(v)
}

const CLS: &[u8] = b"WPObject\0";

extern "C" fn wpSaveState(obj: usize) -> u32 {
    let dd = d(obj);
    let c = CLS.as_ptr();
    if !wps::is(obj, "WPFileSystem") {
        wpSaveString(obj, c, 1, str_ptr(&dd.title));
    }
    wpSaveLong(obj, c, 2, dd.style);
    wpSaveLong(obj, c, 3, dd.default_view);
    if dd.object_id.is_some() {
        wpSaveString(obj, c, 4, str_ptr(&d(obj).object_id));
    }
    if d(obj).help_panel != 0 {
        wpSaveLong(obj, c, 5, d(obj).help_panel);
    }
    if d(obj).ccview != CCVIEW_DEFAULT {
        wpSaveLong(obj, c, 6, d(obj).ccview);
    }
    1
}

extern "C" fn wpRestoreState(obj: usize, _reserved: u32) -> u32 {
    if let Some(t) = restored_string(obj, b"WPObject", 1) {
        d(obj).title = str_new(&t);
    }
    if let Some(v) = restored_long(obj, b"WPObject", 2) {
        d(obj).style = v;
    }
    if let Some(v) = restored_long(obj, b"WPObject", 3) {
        d(obj).default_view = v;
    }
    if let Some(v) = restored_string(obj, b"WPObject", 4) {
        d(obj).object_id = str_new(&v);
    }
    if let Some(v) = restored_long(obj, b"WPObject", 5) {
        d(obj).help_panel = v;
    }
    if let Some(v) = restored_long(obj, b"WPObject", 6) {
        d(obj).ccview = v;
    }
    1
}

extern "C" fn wpQueryConcurrentView(obj: usize) -> u32 {
    d(obj).ccview
}

extern "C" fn wpSetConcurrentView(obj: usize, v: u32) -> u32 {
    d(obj).ccview = v;
    changed(obj);
    1
}

/// Whether opening the object makes a new view even with one open.
pub fn concurrent(obj: usize) -> bool {
    match call(m::WPObject::wpQueryConcurrentView, &[obj]) as u32 {
        CCVIEW_ON => true,
        CCVIEW_OFF => false,
        _ => crate::system::concurrent_view(),
    }
}

/// Save now: every class's wpSaveState, then the values written where
/// the object keeps them.
extern "C" fn wpSaveImmediate(obj: usize) -> u32 {
    if d(obj).flags & F_DELETED != 0 || restoring(obj) {
        return 0;
    }
    d(obj).state = Some(Box::new(Vec::new()));
    call(m::WPObject::wpSaveState, &[obj]);
    let recs = d(obj).state.take().map(|b| *b).unwrap_or_default();
    let ok = wps::persist(obj, &recs);
    d(obj).state = Some(Box::new(recs));
    ok as u32
}

// ---- The lazy writer ----
//
// As in IBM's shell, wpSaveDeferred only notes that an object changed;
// the lazy writer saves it a moment later (the desktop's timer, on the
// shell's thread, where the objects are), so a burst of changes is
// written once. Shutting down saves what is left. Without a desktop (a
// process that is not the shell) objects are saved at once.

pub const TID_LAZY: u32 = 0x5703;
const LAZY_MS: u32 = 1000;

static mut DIRTY: Vec<usize> = Vec::new();

#[allow(static_mut_refs)]
fn dirty() -> &'static mut Vec<usize> {
    unsafe { &mut DIRTY }
}

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}
use crate::pm::{WinStartTimer, WinStopTimer};

extern "C" fn wpSaveDeferred(obj: usize) -> u32 {
    let client = crate::view::desktop_client();
    if client == 0 {
        return call(m::WPObject::wpSaveImmediate, &[obj]) as u32;
    }
    let first = dirty().is_empty();
    if !dirty().contains(&obj) {
        dirty().push(obj);
    }
    if first {
        unsafe { WinStartTimer(WinQueryAnchorBlock(crate::pm::HWND_DESKTOP), client, TID_LAZY, LAZY_MS) };
    }
    1
}

/// The lazy writer's turn (and shutdown): save the changed objects.
pub fn flush_dirty() {
    let client = crate::view::desktop_client();
    if client != 0 {
        unsafe { WinStopTimer(WinQueryAnchorBlock(crate::pm::HWND_DESKTOP), client, TID_LAZY) };
    }
    let list = core::mem::take(dirty());
    debug(&alloc::format!("lazy: saving {} object(s)", list.len()));
    for o in list {
        call(m::WPObject::wpSaveImmediate, &[o]);
    }
}

/// An object that goes leaves the lazy writer's list.
fn undirty(obj: usize) {
    dirty().retain(|&o| o != obj);
}

fn changed(obj: usize) {
    if !restoring(obj) && d(obj).handle != 0 {
        call(m::WPObject::wpSaveDeferred, &[obj]);
    }
}

extern "C" fn wpInitData(_obj: usize) {}

extern "C" fn wpUnInitData(obj: usize) {
    undirty(obj);
    let dd = d(obj);
    dd.title = None;
    dd.object_id = None;
    dd.state = None;
    wps::unregister(obj);
}

/// somInit and somUninit call wpInitData and wpUnInitData (through the
/// object's class: subclasses override those).
extern "C" fn somInit(obj: usize) {
    call(m::WPObject::wpInitData, &[obj]);
}

extern "C" fn somUninit(obj: usize) {
    call(m::WPObject::wpUnInitData, &[obj]);
}

extern "C" fn wpSetTitle(obj: usize, title: *const u8) -> u32 {
    d(obj).title = str_new(cstr(title));
    changed(obj);
    crate::view::changed(obj);
    1
}

extern "C" fn wpQueryTitle(obj: usize) -> *const u8 {
    str_ptr(&d(obj).title)
}

extern "C" fn wpSetStyle(obj: usize, style: u32) -> u32 {
    let template = (d(obj).style ^ style) & OBJSTYLE_TEMPLATE != 0;
    d(obj).style = style;
    changed(obj);
    // A template looks different in its views.
    if template {
        crate::view::changed(obj);
    }
    1
}

extern "C" fn wpQueryStyle(obj: usize) -> u32 {
    let cls = som::class_of(obj);
    d(obj).style | wps::call("M_WPObject", m::M_WPObject::wpclsQueryStyle, &[cls]) as u32
}

extern "C" fn wpSetIcon(obj: usize, icon: u32) -> u32 {
    d(obj).icon = icon;
    crate::view::changed(obj);
    1
}

extern "C" fn wpQueryIcon(obj: usize) -> u32 {
    let i = d(obj).icon;
    if i != 0 {
        return i;
    }
    let cls = som::class_of(obj);
    wps::call("M_WPObject", m::M_WPObject::wpclsQueryIcon, &[cls]) as u32
}

extern "C" fn wpSetFolder(obj: usize, folder: usize) -> u32 {
    d(obj).folder = folder;
    1
}

extern "C" fn wpQueryFolder(obj: usize) -> usize {
    d(obj).folder
}

extern "C" fn wpSetDefaultView(obj: usize, view: u32) -> u32 {
    d(obj).default_view = view;
    changed(obj);
    1
}

extern "C" fn wpQueryDefaultView(obj: usize) -> u32 {
    let v = d(obj).default_view;
    if v != OPEN_DEFAULT {
        return v;
    }
    let cls = som::class_of(obj);
    wps::call("M_WPObject", m::M_WPObject::wpclsQueryDefaultView, &[cls]) as u32
}

/// Abstract objects get a handle when first asked (file system objects'
/// handles come from their paths, see WPFileSystem).
extern "C" fn wpQueryHandle(obj: usize) -> u32 {
    let dd = d(obj);
    if dd.handle == 0 && !wps::is(obj, "WPTransient") {
        dd.handle = wps::new_abstract_handle();
        wps::register(obj, dd.handle, b"");
    }
    d(obj).handle
}

extern "C" fn wpSetObjectID(obj: usize, id: *const u8) -> u32 {
    let id = cstr(id).trim_ascii().to_vec();
    if !id.is_empty() && !(id.starts_with(b"<") && id.ends_with(b">")) {
        return 0;
    }
    let old = object_id(obj);
    if !old.is_empty() {
        wps::remove_object_id(&old);
    }
    if id.is_empty() {
        d(obj).object_id = None;
    } else {
        let h = handle(obj);
        wps::set_object_id(&id, h);
        d(obj).object_id = str_new(&id);
    }
    changed(obj);
    1
}

extern "C" fn wpQueryObjectID(obj: usize) -> *const u8 {
    match &d(obj).object_id {
        Some(_) => str_ptr(&d(obj).object_id),
        None => core::ptr::null(),
    }
}

/// The value of a YES/NO setting.
fn yes(v: &[u8]) -> bool {
    eq_nocase(v, b"YES")
}

/// The keys and values of a setup string (`KEY=value;...`).
pub fn setup_pairs(s: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    s.split(|&c| c == b';')
        .filter_map(|kv| {
            let eq = kv.iter().position(|&c| c == b'=')?;
            Some((kv[..eq].trim_ascii().to_ascii_uppercase(), kv[eq + 1..].trim_ascii().to_vec()))
        })
        .collect()
}

fn view_from_name(v: &[u8]) -> Option<u32> {
    let up = v.to_ascii_uppercase();
    Some(match up.as_slice() {
        b"DEFAULT" => OPEN_DEFAULT,
        b"ICON" | b"CONTENTS" => OPEN_CONTENTS,
        b"SETTINGS" => OPEN_SETTINGS,
        b"RUNNING" => OPEN_RUNNING,
        b"TREE" => OPEN_TREE,
        b"DETAILS" => OPEN_DETAILS,
        _ => parse_dec(v)?,
    })
}

/// An icon from a resource ("id module").
fn icon_resource(v: &[u8]) -> u32 {
    let mut it = v.split(|&c| c == b' ' || c == b',').filter(|s| !s.is_empty());
    let (Some(id), Some(module)) = (it.next().and_then(parse_dec), it.next()) else { return 0 };
    let mut h = 0u32;
    let mut fail = [0u8; 16];
    let name = z(module);
    if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 16, name.as_ptr(), &mut h) } != 0 {
        return 0;
    }
    unsafe { WinLoadPointer(1, h, id) }
}

extern "C" fn wpSetup(obj: usize, setup: *const u8) -> u32 {
    let style_bits: &[(&[u8], u32)] = &[
        (b"NOMOVE", OBJSTYLE_NOMOVE),
        (b"NOLINK", OBJSTYLE_NOLINK),
        (b"NOCOPY", OBJSTYLE_NOCOPY),
        (b"TEMPLATE", OBJSTYLE_TEMPLATE),
        (b"NODELETE", OBJSTYLE_NODELETE),
        (b"NOPRINT", OBJSTYLE_NOPRINT),
        (b"NODRAG", OBJSTYLE_NODRAG),
        (b"NOTVISIBLE", OBJSTYLE_NOTVISIBLE),
        (b"NOSETTINGS", OBJSTYLE_NOSETTINGS),
        (b"NORENAME", OBJSTYLE_NORENAME),
        (b"NODROP", OBJSTYLE_NODROP),
        (b"LOCKEDINPLACE", OBJSTYLE_LOCKEDINPLACE),
    ];
    for (k, v) in setup_pairs(cstr(setup)) {
        match k.as_slice() {
            b"TITLE" => {
                let t: Vec<u8> = v.iter().map(|&c| if c == b'^' { b'\n' } else { c }).collect();
                let t = z(&t);
                call(m::WPObject::wpSetTitle, &[obj, t.as_ptr() as usize]);
            }
            b"OBJECTID" => {
                let i = z(&v);
                call(m::WPObject::wpSetObjectID, &[obj, i.as_ptr() as usize]);
            }
            b"DEFAULTVIEW" => {
                if let Some(view) = view_from_name(&v) {
                    call(m::WPObject::wpSetDefaultView, &[obj, view as usize]);
                }
            }
            b"HELPPANEL" => d(obj).help_panel = parse_dec(&v).unwrap_or(0),
            b"CCVIEW" => {
                let cc = match v.to_ascii_uppercase().as_slice() {
                    b"YES" => CCVIEW_ON,
                    b"NO" => CCVIEW_OFF,
                    _ => CCVIEW_DEFAULT,
                };
                call(m::WPObject::wpSetConcurrentView, &[obj, cc as usize]);
            }
            b"ICONFILE" => {
                let p = z(&v);
                let h = unsafe { Rs2LoadFileIcon(p.as_ptr(), 0) };
                if h != 0 {
                    call(m::WPObject::wpSetIcon, &[obj, h as usize]);
                }
            }
            b"ICONRESOURCE" => {
                let h = icon_resource(&v);
                if h != 0 {
                    call(m::WPObject::wpSetIcon, &[obj, h as usize]);
                }
            }
            _ => {
                if let Some(&(_, bit)) = style_bits.iter().find(|(n, _)| *n == k.as_slice()) {
                    let s = d(obj).style;
                    let s = if yes(&v) { s | bit } else { s & !bit };
                    call(m::WPObject::wpSetStyle, &[obj, s as usize]);
                }
            }
        }
    }
    1
}

extern "C" fn wpSetupOnce(obj: usize, setup: *const u8) -> u32 {
    call(m::WPObject::wpSetup, &[obj, setup as usize]) as u32
}

extern "C" fn wpIsObjectInitialized(_obj: usize) -> u32 {
    1
}

extern "C" fn wpLockObject(obj: usize) {
    d(obj).locks += 1;
}

extern "C" fn wpUnlockObject(obj: usize) -> u32 {
    let dd = d(obj);
    dd.locks = dd.locks.saturating_sub(1);
    1
}

extern "C" fn wpIsLocked(obj: usize) -> u32 {
    (d(obj).locks > 0) as u32
}

/// WPObject's views: its settings notebook.
extern "C" fn wpOpen(obj: usize, _hwnd_cnr: u32, view: u32, _param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return crate::view::open_settings(obj);
    }
    0
}

/// A view that is open comes to the front; otherwise wpOpen opens it.
extern "C" fn wpViewObject(obj: usize, hwnd_cnr: u32, view: u32, param: u32) -> u32 {
    // Opening a template makes a new object beside it.
    if view == OPEN_DEFAULT && crate::templates::is_template(obj) {
        crate::templates::create_from(obj, d(obj).folder, None);
        return 0;
    }
    let view = if view == OPEN_DEFAULT { call(m::WPObject::wpQueryDefaultView, &[obj]) as u32 } else { view };
    // CCVIEW (or the System object's "Create new window") opens another
    // view; settings notebooks are never doubled.
    let f = if view != OPEN_SETTINGS && concurrent(obj) { 0 } else { crate::view::frame_of(obj, if view == OPEN_DEFAULT { OPEN_CONTENTS } else { view }) };
    if f != 0 {
        crate::view::activate(f);
        return f;
    }
    call(m::WPObject::wpOpen, &[obj, hwnd_cnr as usize, view as usize, param as usize]) as u32
}

/// wpFree: the object's stored state goes, then the object.
extern "C" fn wpFree(obj: usize) -> u32 {
    if d(obj).style & OBJSTYLE_NODELETE != 0 {
        return 0;
    }
    let folder = d(obj).folder;
    if folder != 0 && wps::is(folder, "WPFolder") {
        wps::call("WPFolder", m::WPFolder::wpDeleteFromContent, &[folder, obj]);
    }
    wps::forget(obj);
    if wps::is(obj, "WPFileSystem") {
        if let Some(p) = wps::path_of(obj) {
            if !delete_path(&p) {
                return 0;
            }
        }
    }
    d(obj).flags |= F_DELETED;
    undirty(obj);
    crate::view::removed(0, obj);
    som::so_call(som::so::SOM_FREE, &[obj]);
    1
}

/// A shadow of the object in a folder.
extern "C" fn wpCreateShadowObject(obj: usize, folder: usize, _lock: u32) -> usize {
    if d(obj).style & OBJSTYLE_NOLINK != 0 {
        return 0;
    }
    let h = handle(obj);
    let setup = z(alloc::format!("SHADOWID={}", h).as_bytes());
    let cls = wps::class("WPShadow");
    let t = z(&title(obj));
    wps::call("M_WPObject", m::M_WPObject::wpclsNew, &[cls, t.as_ptr() as usize, setup.as_ptr() as usize, folder, 0])
}

/// Copying an abstract object: a new one of its class with its state.
extern "C" fn wpCopyObject(obj: usize, folder: usize, _lock: u32) -> usize {
    if d(obj).style & OBJSTYLE_NOCOPY != 0 {
        return 0;
    }
    let cls = som::class_of(obj);
    let t = z(&title(obj));
    let n = wps::call("M_WPObject", m::M_WPObject::wpclsNew, &[cls, t.as_ptr() as usize, b"\0".as_ptr() as usize, folder, 0]);
    if n == 0 {
        return 0;
    }
    // The original's values (but not its object ID), restored into the
    // copy and saved.
    d(obj).state = Some(Box::new(Vec::new()));
    call(m::WPObject::wpSaveState, &[obj]);
    let mut recs = d(obj).state.take().map(|b| *b).unwrap_or_default();
    recs.retain(|r| !(r.key == 4 && eq_nocase(&r.class, b"WPObject")));
    let f = d(n).folder;
    wake(n, f, recs);
    call(m::WPObject::wpSaveImmediate, &[n]);
    n
}

extern "C" fn wpMoveObject(obj: usize, folder: usize) -> u32 {
    if d(obj).style & OBJSTYLE_NOMOVE != 0 || folder == 0 {
        return 0;
    }
    let old = d(obj).folder;
    if old == folder {
        return 1;
    }
    let h = handle(obj);
    if wps::is_abstract_handle(h) {
        if old != 0 {
            wps::content_remove(handle(old), h);
            wps::call("WPFolder", m::WPFolder::wpDeleteFromContent, &[old, obj]);
        }
        wps::content_add(handle(folder), h);
        d(obj).folder = folder;
        wps::call("WPFolder", m::WPFolder::wpAddToContent, &[folder, obj]);
        call(m::WPObject::wpSaveImmediate, &[obj]);
        return 1;
    }
    0
}

/// The System object's confirmations (delete.rs).
extern "C" fn wpQueryConfirmations(_obj: usize) -> u32 {
    crate::delete::confirmations()
}

// ---- M_WPObject ----

/// Class titles (a class without its own is called after its nearest
/// ancestor that has one).
fn class_title(name: &[u8]) -> Option<&'static [u8]> {
    Some(match name {
        b"WPObject" => b"Object\0",
        b"WPAbstract" => b"Abstract\0",
        b"WPTransient" => b"Transient\0",
        b"WPFileSystem" => b"File System\0",
        b"WPDataFile" => b"Data File\0",
        b"WPFolder" => b"Folder\0",
        b"WPRootFolder" => b"Root Folder\0",
        b"WPDesktop" => b"Desktop\0",
        b"WPDrives" => b"Drives\0",
        b"WPDisk" => b"Drive\0",
        b"WPProgram" => b"Program\0",
        b"WPProgramFile" => b"Program File\0",
        b"WPCommandFile" => b"Command File\0",
        b"WPShadow" => b"Shadow\0",
        b"WPIcon" => b"Icon\0",
        b"WPPointer" => b"Pointer\0",
        b"WPBitmap" => b"Bitmap\0",
        b"WPMet" => b"Metafile\0",
        b"WPPif" => b"Picture\0",
        b"WPStartup" => b"Startup\0",
        b"WPMinWinViewer" => b"Minimized Window Viewer\0",
        b"WPMinWindow" => b"Minimized Window\0",
        b"RS2Shutdown" => b"Shut Down\0",
        b"RS2WindowList" => b"Window List\0",
        b"RS2System" => b"System\0",
        b"WPMouse" => b"Mouse\0",
        b"WPKeyboard" => b"Keyboard\0",
        b"WPSound" => b"Sound\0",
        b"WPPalette" => b"Palette\0",
        b"WPColorPalette" => b"Color Palette\0",
        b"WPSchemePalette" => b"Scheme Palette\0",
        b"WPFontPalette" => b"Font Palette\0",
        b"Mindex" => b"Index\0",
        b"WPLaunchPad" => b"LaunchPad\0",
        b"WPUrl" => b"URL\0",
        b"WPUrlFolder" => b"URL Folder\0",
        b"WPShredder" => b"Shredder\0",
        b"WPNetwork" => b"Network\0",
        b"WPPrinter" => b"Printer\0",
        b"WPSystem" => b"System\0",
        b"WPClock" => b"System Clock\0",
        _ => return None,
    })
}

fn ancestors_of(cls: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut c = cls;
    while c != 0 && v.len() < 32 {
        v.push(c);
        c = som::sc_call(som::sc::SOM_GET_PARENT, &[c]);
    }
    v
}

extern "C" fn wpclsQueryTitle(cls: usize) -> *const u8 {
    for c in ancestors_of(cls) {
        if let Some(t) = class_title(som::class_name(c)) {
            return t.as_ptr();
        }
    }
    som::sc_call(som::sc::SOM_GET_NAME, &[cls]) as *const u8
}

const SPTR_APPICON: i32 = 10;
const SPTR_FILE: i32 = 19;
const SPTR_FOLDER: i32 = 20;
const SPTR_PROGRAM: i32 = 22;

extern "C" fn wpclsQueryIcon(cls: usize) -> u32 {
    let mut id = SPTR_APPICON;
    for c in ancestors_of(cls) {
        // rs2's own icons for the classes that have one (icons.rs).
        let own = match som::class_name(c) {
            b"WPDrives" => crate::icons::DRIVES,
            b"WPDesktop" => crate::icons::DESKTOP,
            b"WPDisk" => crate::icons::HARD_DISK,
            b"WPMinWinViewer" => crate::icons::MINIMIZED,
            b"RS2Shutdown" => crate::icons::SHUTDOWN,
            b"Mindex" => crate::icons::INDEX,
            b"WPLaunchPad" => crate::icons::LAUNCHPAD,
            b"WPKeyboard" => crate::icons::KEYBOARD,
            b"WPMouse" => crate::icons::MOUSE,
            b"WPSound" => crate::icons::SOUND,
            b"WPColorPalette" => crate::icons::COLOR_PALETTE,
            b"WPSchemePalette" => crate::icons::SCHEME_PALETTE,
            b"WPFontPalette" => crate::icons::FONT_PALETTE,
            b"RS2System" => crate::icons::SYSTEM,
            b"RS2WindowList" => crate::icons::WINDOW_LIST,
            b"WPShredder" => crate::icons::SHREDDER,
            b"WPNetwork" => crate::icons::NETWORK,
            b"WPPrinter" => crate::icons::PRINTER,
            b"WPSystem" => crate::icons::SYSTEM,
            b"WPClock" => crate::icons::CLOCK,
            _ => 0,
        };
        if own != 0 {
            return crate::icons::shell(own);
        }
        match som::class_name(c) {
            b"WPFolder" => id = SPTR_FOLDER,
            b"WPProgram" | b"WPProgramFile" => id = SPTR_PROGRAM,
            b"WPDataFile" => id = SPTR_FILE,
            _ => continue,
        }
        break;
    }
    unsafe { WinQuerySysPointer(1, id, 0) }
}

extern "C" fn wpclsQueryStyle(_cls: usize) -> u32 {
    0
}

extern "C" fn wpclsQueryDefaultView(cls: usize) -> u32 {
    for c in ancestors_of(cls) {
        match som::class_name(c) {
            // A drive opens its root folder.
            b"WPFolder" | b"WPDisk" => return OPEN_CONTENTS,
            // Opening them acts (shuts down, shows the window list,
            // restores the window).
            b"RS2Shutdown" | b"RS2WindowList" | b"WPMinWindow" | b"Mindex" | b"WPLaunchPad" | b"WPPalette" | b"WPClock" => return OPEN_RUNNING,
            b"WPProgram" | b"WPProgramFile" | b"WPDataFile" => return OPEN_RUNNING,
            _ => {}
        }
    }
    OPEN_SETTINGS
}

extern "C" fn wpclsInitData(_cls: usize) {}

/// A class is ready: SOMClass's somClassReady, then wpclsInitData (as
/// IBM's M_WPObject does it), which classes override to set themselves
/// up.
extern "C" fn cls_som_class_ready(cls: usize) {
    som::call_parent(wps::ccd("M_WPObject"), unsafe { &raw const som::SOMClassClassData } as *const usize, 3, &[cls]);
    wps::call("M_WPObject", m::M_WPObject::wpclsInitData, &[cls]);
}
extern "C" fn wpclsUnInitData(_cls: usize) {}

/// wpclsNew: a new object of the class in a folder, set up and saved.
extern "C" fn wpclsNew(cls: usize, title: *const u8, setup: *const u8, folder: usize, _lock: u32) -> usize {
    let o = som::sc_call(som::sc::SOM_NEW, &[cls]);
    debug(&alloc::format!(
        "wpclsNew: class {:x} -> object {:x}, title {:?}, fs {} transient {}",
        cls,
        o,
        core::str::from_utf8(cstr(title)),
        wps::is(o, "WPFileSystem"),
        wps::is(o, "WPTransient")
    ));
    if o == 0 {
        return 0;
    }
    d(o).folder = folder;
    d(o).title = str_new(cstr(title));
    let ok = if wps::is(o, "WPFileSystem") {
        crate::fs::create(o, folder, cstr(title))
    } else if !wps::is(o, "WPTransient") {
        let h = handle(o);
        if folder != 0 {
            wps::content_add(handle(folder), h);
        }
        true
    } else {
        true
    };
    if !ok {
        debug("wpclsNew: the object could not be made");
        som::so_call(som::so::SOM_FREE, &[o]);
        return 0;
    }
    call(m::WPObject::wpSetupOnce, &[o, setup as usize]);
    call(m::WPObject::wpSaveImmediate, &[o]);
    if folder != 0 && wps::is(folder, "WPFolder") {
        wps::call("WPFolder", m::WPFolder::wpAddToContent, &[folder, o]);
    }
    o
}

extern "C" fn wpclsMakeAwake(cls: usize, title: *const u8, style: u32, icon: u32, _objdata: usize, folder: usize, _user: u32) -> usize {
    let o = som::sc_call(som::sc::SOM_NEW, &[cls]);
    if o == 0 {
        return 0;
    }
    let dd = d(o);
    dd.title = str_new(cstr(title));
    dd.style = style;
    dd.icon = icon;
    dd.folder = folder;
    o
}

extern "C" fn wpclsObjectFromHandle(_cls: usize, h: u32) -> usize {
    wps::object_from_handle(h)
}

extern "C" fn wpclsQueryFolder(_cls: usize, location: *const u8, _lock: u32) -> usize {
    let o = wps::object_from_location(cstr(location));
    if o != 0 && wps::is(o, "WPFolder") { o } else { 0 }
}

methods! {
    __m_wpSaveLong => wpSaveLong, 4;
    __m_wpSaveString => wpSaveString, 4;
    __m_wpSaveData => wpSaveData, 5;
    __m_wpRestoreLong => wpRestoreLong, 4;
    __m_wpRestoreString => wpRestoreString, 5;
    __m_wpRestoreData => wpRestoreData, 5;
    __m_wpSaveState => wpSaveState, 1;
    __m_wpRestoreState => wpRestoreState, 2;
    __m_wpSaveImmediate => wpSaveImmediate, 1;
    __m_wpSaveDeferred => wpSaveDeferred, 1;
    __m_wpInitData => wpInitData, 1;
    __m_wpUnInitData => wpUnInitData, 1;
    __m_somInit => somInit, 1;
    __m_somUninit => somUninit, 1;
    __m_wpSetTitle => wpSetTitle, 2;
    __m_wpQueryTitle => wpQueryTitle, 1;
    __m_wpSetStyle => wpSetStyle, 2;
    __m_wpQueryStyle => wpQueryStyle, 1;
    __m_wpSetIcon => wpSetIcon, 2;
    __m_wpQueryIcon => wpQueryIcon, 1;
    __m_wpSetFolder => wpSetFolder, 2;
    __m_wpQueryFolder => wpQueryFolder, 1;
    __m_wpSetDefaultView => wpSetDefaultView, 2;
    __m_wpQueryDefaultView => wpQueryDefaultView, 1;
    __m_wpQueryHandle => wpQueryHandle, 1;
    __m_wpSetObjectID => wpSetObjectID, 2;
    __m_wpQueryObjectID => wpQueryObjectID, 1;
    __m_wpSetup => wpSetup, 2;
    __m_wpSetupOnce => wpSetupOnce, 2;
    __m_wpIsObjectInitialized => wpIsObjectInitialized, 1;
    __m_wpLockObject => wpLockObject, 1;
    __m_wpUnlockObject => wpUnlockObject, 1;
    __m_wpIsLocked => wpIsLocked, 1;
    __m_wpOpen => wpOpen, 4;
    __m_wpViewObject => wpViewObject, 4;
    __m_wpFree => wpFree, 1;
    __m_wpCreateShadowObject => wpCreateShadowObject, 3;
    __m_wpCopyObject => wpCopyObject, 3;
    __m_wpMoveObject => wpMoveObject, 2;
    __m_wpQueryConfirmations => wpQueryConfirmations, 1;
    __m_wpQueryConcurrentView => wpQueryConcurrentView, 1;
    __m_wpSetConcurrentView => wpSetConcurrentView, 2;
    __m_wpclsQueryTitle => wpclsQueryTitle, 1;
    __m_wpclsQueryIcon => wpclsQueryIcon, 1;
    __m_wpclsQueryStyle => wpclsQueryStyle, 1;
    __m_wpclsQueryDefaultView => wpclsQueryDefaultView, 1;
    __m_wpclsInitData => wpclsInitData, 1;
    __m_cls_som_class_ready => cls_som_class_ready, 1;
    __m_wpclsUnInitData => wpclsUnInitData, 1;
    __m_wpclsNew => wpclsNew, 5;
    __m_wpclsMakeAwake => wpclsMakeAwake, 7;
    __m_wpclsObjectFromHandle => wpclsObjectFromHandle, 2;
    __m_wpclsQueryFolder => wpclsQueryFolder, 3;
}

pub fn impl_object() -> Impl {
    let mut i = impl_object_own();
    i.methods.extend(crate::menu::methods());
    i.methods.extend(crate::book::object_methods());
    i.methods.extend(crate::dnd::object_methods());
    i.methods.extend(crate::delete::object_methods());
    i.methods.extend(crate::pages::object_methods());
    i
}

fn impl_object_own() -> Impl {
    Impl {
        data: core::mem::size_of::<Data>() as u32,
        methods: alloc::vec![
            ("wpSaveLong", __m_wpSaveLong as usize),
            ("wpSaveString", __m_wpSaveString as usize),
            ("wpSaveData", __m_wpSaveData as usize),
            ("wpRestoreLong", __m_wpRestoreLong as usize),
            ("wpRestoreString", __m_wpRestoreString as usize),
            ("wpRestoreData", __m_wpRestoreData as usize),
            ("wpSaveState", __m_wpSaveState as usize),
            ("wpRestoreState", __m_wpRestoreState as usize),
            ("wpSaveImmediate", __m_wpSaveImmediate as usize),
            ("wpSaveDeferred", __m_wpSaveDeferred as usize),
            ("wpInitData", __m_wpInitData as usize),
            ("wpUnInitData", __m_wpUnInitData as usize),
            ("wpSetTitle", __m_wpSetTitle as usize),
            ("wpQueryTitle", __m_wpQueryTitle as usize),
            ("wpSetStyle", __m_wpSetStyle as usize),
            ("wpQueryStyle", __m_wpQueryStyle as usize),
            ("wpSetIcon", __m_wpSetIcon as usize),
            ("wpQueryIcon", __m_wpQueryIcon as usize),
            ("wpSetFolder", __m_wpSetFolder as usize),
            ("wpQueryFolder", __m_wpQueryFolder as usize),
            ("wpSetDefaultView", __m_wpSetDefaultView as usize),
            ("wpQueryDefaultView", __m_wpQueryDefaultView as usize),
            ("wpQueryHandle", __m_wpQueryHandle as usize),
            ("wpSetObjectID", __m_wpSetObjectID as usize),
            ("wpQueryObjectID", __m_wpQueryObjectID as usize),
            ("wpSetup", __m_wpSetup as usize),
            ("wpSetupOnce", __m_wpSetupOnce as usize),
            ("wpIsObjectInitialized", __m_wpIsObjectInitialized as usize),
            ("wpLockObject", __m_wpLockObject as usize),
            ("wpUnlockObject", __m_wpUnlockObject as usize),
            ("wpIsLocked", __m_wpIsLocked as usize),
            ("wpOpen", __m_wpOpen as usize),
            ("wpViewObject", __m_wpViewObject as usize),
            ("wpFree", __m_wpFree as usize),
            ("wpCreateShadowObject", __m_wpCreateShadowObject as usize),
            ("wpCopyObject", __m_wpCopyObject as usize),
            ("wpMoveObject", __m_wpMoveObject as usize),
            ("wpQueryConfirmations", __m_wpQueryConfirmations as usize),
            ("wpQueryConcurrentView", __m_wpQueryConcurrentView as usize),
            ("wpSetConcurrentView", __m_wpSetConcurrentView as usize),
        ],
        overrides: alloc::vec![("somInit", __m_somInit as usize), ("somUninit", __m_somUninit as usize)],
    }
}

pub fn impl_m_object() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![
            ("wpclsQueryTitle", __m_wpclsQueryTitle as usize),
            ("wpclsQueryIcon", __m_wpclsQueryIcon as usize),
            ("wpclsQueryStyle", __m_wpclsQueryStyle as usize),
            ("wpclsQueryDefaultView", __m_wpclsQueryDefaultView as usize),
            ("wpclsInitData", __m_wpclsInitData as usize),
            ("wpclsUnInitData", __m_wpclsUnInitData as usize),
            ("wpclsNew", __m_wpclsNew as usize),
            ("wpclsMakeAwake", __m_wpclsMakeAwake as usize),
            ("wpclsObjectFromHandle", __m_wpclsObjectFromHandle as usize),
            ("wpclsQueryObject", __m_wpclsObjectFromHandle as usize),
            ("wpclsQueryFolder", __m_wpclsQueryFolder as usize),
        ],
        overrides: alloc::vec![("somClassReady", __m_cls_som_class_ready as usize)],
    }
}
