//! The Workplace Shell's runtime: building the classes (as the SOM
//! compiler's output builds them: somBuildClass with a static class
//! information structure, methods in release order), the objects that
//! are awake in this process, object handles and IDs, and where objects
//! keep their state:
//!
//! - abstract objects in OS2.INI: `PM_Abstract:Objects` (key: the
//!   handle's low word in hex; value: the state, below), and the folder
//!   they are in under `PM_Abstract:FldrContent` (key: the folder's
//!   handle in hex; value: the handles, 32-bit);
//! - file system objects in their `.CLASSINFO` extended attribute (the
//!   same state record);
//! - file system handles under `PM_Workplace:Handles` (key: the low word
//!   in hex; value: the path), object IDs under `PM_Workplace:Location`
//!   (key: the ID; value: the handle), as IBM's shell keeps them.
//!
//! The state record: "RS2O", the class name, the folder's handle and the
//! values objects saved with wpSaveLong/String/Data (class name, key,
//! data). Abstract handles are 0x2xxxx, file system handles 0x3xxxx.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::classdata_gen::class_data;
use crate::classes_gen::{CLASSES, ClassDef};
use crate::som::{self, SomId};
use crate::util::*;

/// What a class module provides: its instance data size, the
/// implementations of methods it introduces, and its overrides.
pub struct Impl {
    pub data: u32,
    pub methods: Vec<(&'static str, usize)>,
    pub overrides: Vec<(&'static str, usize)>,
}

impl Impl {
    pub fn none() -> Impl {
        Impl { data: 0, methods: Vec::new(), overrides: Vec::new() }
    }
}

fn implementation(name: &str) -> Impl {
    match name {
        "WPObject" => crate::obj::impl_object(),
        "M_WPObject" => crate::obj::impl_m_object(),
        "WPAbstract" => crate::abs::impl_abstract(),
        "WPTransient" => Impl::none(),
        "WPProgram" => crate::abs::impl_program(),
        "WPShadow" => crate::abs::impl_shadow(),
        "WPDisk" => crate::abs::impl_disk(),
        "WPFileSystem" => crate::fs::impl_file_system(),
        "M_WPFileSystem" => crate::fs::impl_m_file_system(),
        "WPDataFile" => crate::fs::impl_data_file(),
        "WPProgramFile" => crate::fs::impl_program_file(),
        "WPFolder" => crate::fs::impl_folder(),
        "WPDrives" => crate::fs::impl_drives(),
        "WPClassManager" => crate::clsmgr::impl_class_manager(),
        "WPMinWindow" => crate::minwin::impl_min_window(),
        "WPUrl" => crate::url::impl_url(),
        "WPLaunchPad" => crate::launchpad::impl_launchpad(),
        "Mindex" => crate::mindex::impl_mindex(),
        "WPMinWinViewer" => crate::minwin::impl_viewer(),
        "RS2Shutdown" => crate::rs2obj::impl_shutdown(),
        "RS2WindowList" => crate::rs2obj::impl_window_list(),
        "RS2System" => crate::system::impl_rs2_system(),
        "WPSystem" => crate::system::impl_system(),
        "WPClock" => crate::system::impl_clock(),
        "WPNetwork" => crate::connections::impl_network(),
        "WPPrinter" => crate::connections::impl_printer(),
        "WPMouse" => crate::setup::impl_mouse(),
        "WPKeyboard" => crate::setup::impl_keyboard(),
        "WPSound" => crate::setup::impl_sound(),
        "WPColorPalette" => crate::setup::impl_color_palette(),
        "WPSchemePalette" => crate::setup::impl_scheme_palette(),
        "WPFontPalette" => crate::setup::impl_font_palette(),
        "WPShredder" => crate::delete::impl_shredder(),
        "M_WPShredder" => crate::delete::impl_m_shredder(),
        _ => Impl::none(),
    }
}

/// Classes IBM's PMWP has that the Toolkit does not describe stand in
/// for their nearest described ancestor.
fn substitute(name: &str) -> &str {
    match name {
        "WPImageFile" => "WPDataFile",
        "M_WPImageFile" => "M_WPDataFile",
        n => n,
    }
}

pub fn index_of(name: &str) -> Option<usize> {
    CLASSES.iter().position(|c| c.name == name)
}

/// The class object of a class PMWP.DLL provides (built on first use).
pub fn class(name: &str) -> usize {
    match index_of(name) {
        Some(i) => new_class(i, 0, 0),
        None => 0,
    }
}

fn leak_id(name: &[u8]) -> SomId {
    let v = z(name);
    let p = Box::leak(v.into_boxed_slice()).as_ptr();
    Box::leak(Box::new(p))
}

fn leak_id_ptr(name: &[u8]) -> usize {
    Box::leak(Box::new(leak_id(name))) as *mut SomId as usize
}

/// `<Class>NewClass(major, minor)`: build the class (its parents and
/// metaclass first) unless it exists.
pub fn new_class(i: usize, _major: i32, _minor: i32) -> usize {
    let def: &ClassDef = &CLASSES[i];
    let (cd, n, ccd) = class_data(i);
    if cd.is_null() {
        return 0;
    }
    if unsafe { *cd } != 0 {
        return unsafe { *cd };
    }
    unsafe {
        som::SOMObjectNewClass(1, 4);
        som::SOMClassNewClass(1, 4);
        som::SOMClassMgrNewClass(1, 4);
    }
    let parents: Vec<&str> = def.parents.iter().map(|p| substitute(p)).collect();
    for p in &parents {
        if let Some(j) = index_of(p) {
            new_class(j, 0, 0);
        }
    }
    let meta = def.meta.map(substitute);
    if let Some(m) = meta {
        if let Some(j) = index_of(m) {
            new_class(j, 0, 0);
        }
    }
    let imp = implementation(def.name);
    for (mname, _) in &imp.methods {
        if !def.methods.contains(mname) {
            os2::write(alloc::format!("PMWP: {} has no method {}\r\n", def.name, mname).as_bytes());
        }
    }
    let mut smt: Vec<[usize; 6]> = Vec::new();
    for (k, &mname) in def.methods.iter().enumerate() {
        let p = imp.methods.iter().find(|m| m.0 == mname).map_or(som::ret0 as usize, |m| m.1);
        let mut d = def.name.as_bytes().to_vec();
        d.extend_from_slice(b"::");
        d.extend_from_slice(mname.as_bytes());
        smt.push([cd as usize + 4 * (k + 1), leak_id_ptr(mname.as_bytes()), leak_id_ptr(&d), p, usize::MAX, 0]);
    }
    let omt: Vec<[usize; 2]> = imp.overrides.iter().map(|&(m, p)| [leak_id_ptr(m.as_bytes()), p]).collect();
    let pids: Vec<SomId> = parents.iter().map(|p| leak_id(p.as_bytes())).collect();
    let mut sci = vec![0u32; 48];
    sci[0] = 4;
    sci[1] = smt.len() as u32;
    sci[2] = omt.len() as u32;
    sci[6] = def.major;
    sci[7] = def.minor;
    sci[8] = imp.data;
    sci[9] = (n - 1) as u32;
    sci[10] = pids.len() as u32;
    sci[11] = leak_id(def.name.as_bytes()) as u32;
    sci[12] = meta.map_or(0, |m| leak_id(m.as_bytes()) as u32);
    sci[14] = Box::leak(pids.into_boxed_slice()).as_ptr() as u32;
    sci[15] = cd as u32;
    sci[16] = ccd as u32;
    sci[17] = Box::leak(smt.into_boxed_slice()).as_ptr() as u32;
    sci[18] = Box::leak(omt.into_boxed_slice()).as_ptr() as u32;
    sci[24] = 4;
    sci[25] = u32::MAX;
    sci[33] = (n - 1) as u32;
    let sci = Box::leak(sci.into_boxed_slice());
    let inherit = (1i32 << parents.len()) - 1;
    unsafe { som::somBuildClass(inherit, sci.as_ptr(), def.major as i32, def.minor as i32) }
}

/// The class data of a class PMWP.DLL provides, by name.
pub fn cd(name: &str) -> *const usize {
    index_of(name).map_or(core::ptr::null(), |i| class_data(i).0)
}

pub fn ccd(name: &str) -> *const usize {
    index_of(name).map_or(core::ptr::null(), |i| class_data(i).2)
}

/// Call a method of class `class` (by its index constant).
pub fn call(class: &str, i: usize, args: &[usize]) -> usize {
    som::call(cd(class), i, args)
}

pub fn is(obj: usize, class_name: &str) -> bool {
    let c = class(class_name);
    som::is_a(obj, c)
}

// ---- Objects awake in this process ----

pub struct Awake {
    pub obj: usize,
    pub handle: u32,
    /// File system objects: the path (normalized).
    pub path: Vec<u8>,
}

static mut AWAKE: Vec<Awake> = Vec::new();
pub static mut ACTIVE: bool = false;

#[allow(static_mut_refs)]
pub fn awake() -> &'static mut Vec<Awake> {
    unsafe { &mut AWAKE }
}

pub fn register(obj: usize, handle: u32, path: &[u8]) {
    let a = awake();
    if let Some(e) = a.iter_mut().find(|e| e.obj == obj) {
        if handle != 0 {
            e.handle = handle;
        }
        if !path.is_empty() {
            e.path = path.to_vec();
        }
        return;
    }
    a.push(Awake { obj, handle, path: path.to_vec() });
}

pub fn unregister(obj: usize) {
    awake().retain(|e| e.obj != obj);
}

pub fn by_handle(h: u32) -> Option<usize> {
    awake().iter().find(|e| e.handle == h && h != 0).map(|e| e.obj)
}

pub fn by_path(path: &[u8]) -> Option<usize> {
    awake().iter().find(|e| !e.path.is_empty() && eq_nocase(&e.path, path)).map(|e| e.obj)
}

pub fn path_of(obj: usize) -> Option<Vec<u8>> {
    awake().iter().find(|e| e.obj == obj && !e.path.is_empty()).map(|e| e.path.clone())
}

// ---- Handles ----

const APP_OBJECTS: &[u8] = b"PM_Abstract:Objects";
const APP_CONTENT: &[u8] = b"PM_Abstract:FldrContent";
const APP_HANDLES: &[u8] = b"PM_Workplace:Handles";
const APP_LOCATION: &[u8] = b"PM_Workplace:Location";

pub fn is_abstract_handle(h: u32) -> bool {
    h >> 16 == 2
}

/// A new abstract object handle (one not in OS2.INI yet).
pub fn new_abstract_handle() -> u32 {
    let mut used: Vec<u32> = prf_keys(HINI_USER, APP_OBJECTS).iter().filter_map(|k| parse_hex(k)).collect();
    used.extend(awake().iter().filter(|e| is_abstract_handle(e.handle)).map(|e| e.handle & 0xffff));
    let mut n = 1;
    while used.contains(&n) {
        n += 1;
    }
    0x20000 | n
}

/// The handle of a path (made and kept in OS2.INI if new).
pub fn fs_handle(path: &[u8]) -> u32 {
    let path = normalize(path);
    let keys = prf_keys(HINI_USER, APP_HANDLES);
    let mut used = Vec::new();
    for k in &keys {
        if let (Some(n), Some(v)) = (parse_hex(k), prf_read(HINI_USER, APP_HANDLES, k)) {
            used.push(n);
            let v = v.strip_suffix(&[0]).unwrap_or(&v);
            if eq_nocase(v, &path) {
                return 0x30000 | n;
            }
        }
    }
    let mut n = 1;
    while used.contains(&n) {
        n += 1;
    }
    prf_write(HINI_USER, APP_HANDLES, &hex(n), Some(&z(&path)));
    0x30000 | n
}

pub fn path_of_handle(h: u32) -> Option<Vec<u8>> {
    if h >> 16 != 3 {
        return None;
    }
    let v = prf_read(HINI_USER, APP_HANDLES, &hex(h & 0xffff))?;
    Some(v.strip_suffix(&[0]).unwrap_or(&v).to_vec())
}

/// The object a handle stands for (woken if need be).
pub fn object_from_handle(h: u32) -> usize {
    if let Some(o) = by_handle(h) {
        return o;
    }
    if is_abstract_handle(h) {
        return wake_abstract(h);
    }
    match path_of_handle(h) {
        Some(p) => object_from_path(&p),
        None => 0,
    }
}

// ---- Object IDs ----

pub fn set_object_id(id: &[u8], h: u32) {
    if id.is_empty() {
        return;
    }
    prf_write(HINI_USER, APP_LOCATION, id, Some(&h.to_le_bytes()));
}

pub fn remove_object_id(id: &[u8]) {
    if !id.is_empty() {
        prf_write(HINI_USER, APP_LOCATION, id, None);
    }
}

pub fn handle_of_id(id: &[u8]) -> Option<u32> {
    let v = prf_read(HINI_USER, APP_LOCATION, id)?;
    (v.len() >= 4).then(|| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

/// An object by location: an object ID ("<WP_DESKTOP>"), a handle in
/// decimal (as programs pass HOBJECTs to WinCreateObject), or a path.
pub fn object_from_location(loc: &[u8]) -> usize {
    let loc = loc.trim_ascii();
    if loc.starts_with(b"<") {
        return match handle_of_id(loc) {
            Some(h) => object_from_handle(h),
            None => well_known(loc),
        };
    }
    if let Some(h) = parse_dec(loc) {
        if h >> 16 == 2 || h >> 16 == 3 {
            return object_from_handle(h);
        }
    }
    if loc.len() >= 2 && loc[1] == b':' {
        return object_from_path(loc);
    }
    0
}

/// Objects every desktop has, found without OS2.INI's help.
fn well_known(id: &[u8]) -> usize {
    if eq_nocase(id, b"<WP_DESKTOP>") {
        return crate::fs::desktop();
    }
    0
}

// ---- State records ----

pub struct Rec {
    pub class: Vec<u8>,
    pub key: u32,
    pub data: Vec<u8>,
}

pub fn encode(class: &[u8], folder: u32, recs: &[Rec]) -> Vec<u8> {
    let mut b = b"RS2O".to_vec();
    b.push(1);
    b.push(class.len() as u8);
    b.extend_from_slice(class);
    b.extend_from_slice(&folder.to_le_bytes());
    b.extend_from_slice(&(recs.len() as u16).to_le_bytes());
    for r in recs {
        b.push(r.class.len() as u8);
        b.extend_from_slice(&r.class);
        b.extend_from_slice(&r.key.to_le_bytes());
        b.extend_from_slice(&(r.data.len() as u16).to_le_bytes());
        b.extend_from_slice(&r.data);
    }
    b
}

/// A copied file or directory is a new object: its .CLASSINFO loses the
/// original's object ID (WPObject's key 4) before it wakes.
pub fn drop_copied_id(path: &[u8]) {
    let Some((_, b)) = ea_read(path, b".CLASSINFO") else { return };
    let Some((cls, folder, mut recs)) = decode(&b) else { return };
    let n = recs.len();
    recs.retain(|r| !(r.key == 4 && eq_nocase(&r.class, b"WPObject")));
    if recs.len() != n {
        ea_write(path, b".CLASSINFO", Some((EAT_BINARY, &encode(&cls, folder, &recs))));
    }
}

pub fn decode(b: &[u8]) -> Option<(Vec<u8>, u32, Vec<Rec>)> {
    if b.len() < 6 || &b[..4] != b"RS2O" {
        return None;
    }
    let mut i = 5;
    let take = |i: &mut usize, n: usize| -> Option<&[u8]> {
        let s = b.get(*i..*i + n)?;
        *i += n;
        Some(s)
    };
    let cl = *take(&mut i, 1)?.first()? as usize;
    let class = take(&mut i, cl)?.to_vec();
    let f = take(&mut i, 4)?;
    let folder = u32::from_le_bytes([f[0], f[1], f[2], f[3]]);
    let n = take(&mut i, 2)?;
    let n = u16::from_le_bytes([n[0], n[1]]);
    let mut recs = Vec::new();
    for _ in 0..n {
        let l = *take(&mut i, 1)?.first()? as usize;
        let c = take(&mut i, l)?.to_vec();
        let k = take(&mut i, 4)?;
        let key = u32::from_le_bytes([k[0], k[1], k[2], k[3]]);
        let dl = take(&mut i, 2)?;
        let dl = u16::from_le_bytes([dl[0], dl[1]]) as usize;
        let data = take(&mut i, dl)?.to_vec();
        recs.push(Rec { class: c, key, data });
    }
    Some((class, folder, recs))
}

/// Write an object's state where it belongs.
pub fn persist(obj: usize, recs: &[Rec]) -> bool {
    let cls = som::class_name(som::class_of(obj));
    let folder = crate::obj::folder_of(obj);
    let fh = if folder != 0 { crate::obj::handle(folder) } else { 0 };
    if is(obj, "WPTransient") {
        return true;
    }
    if is(obj, "WPFileSystem") {
        let Some(p) = path_of(obj) else { return false };
        let blob = encode(cls, 0, recs);
        let ok = ea_write(&p, b".CLASSINFO", Some((EAT_BINARY, &blob)));
        if !ok {
            debug(&alloc::format!("persist: {:?} not saved", core::str::from_utf8(&p)));
        }
        return ok;
    }
    let h = crate::obj::handle(obj);
    if h == 0 {
        return false;
    }
    prf_write(HINI_USER, APP_OBJECTS, &hex(h & 0xffff), Some(&encode(cls, fh, recs)));
    true
}

// ---- Folder content of abstract objects ----

pub fn abstract_content(folder_handle: u32) -> Vec<u32> {
    let v = prf_read(HINI_USER, APP_CONTENT, &hex(folder_handle)).unwrap_or_default();
    v.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn set_abstract_content(folder_handle: u32, list: &[u32]) {
    let key = hex(folder_handle);
    if list.is_empty() {
        prf_write(HINI_USER, APP_CONTENT, &key, None);
        return;
    }
    let b: Vec<u8> = list.iter().flat_map(|h| h.to_le_bytes()).collect();
    prf_write(HINI_USER, APP_CONTENT, &key, Some(&b));
}

pub fn content_add(folder_handle: u32, h: u32) {
    let mut l = abstract_content(folder_handle);
    if !l.contains(&h) {
        l.push(h);
        set_abstract_content(folder_handle, &l);
    }
}

pub fn content_remove(folder_handle: u32, h: u32) {
    let mut l = abstract_content(folder_handle);
    let n = l.len();
    l.retain(|&x| x != h);
    if l.len() != n {
        set_abstract_content(folder_handle, &l);
    }
}

/// Forget an object's stored state (it is being deleted).
pub fn forget(obj: usize) {
    let h = crate::obj::handle(obj);
    if is_abstract_handle(h) {
        prf_write(HINI_USER, APP_OBJECTS, &hex(h & 0xffff), None);
        let folder = crate::obj::folder_of(obj);
        if folder != 0 {
            content_remove(crate::obj::handle(folder), h);
        }
    }
    let id = crate::obj::object_id(obj);
    if !id.is_empty() {
        remove_object_id(&id);
    }
}

// ---- Waking objects ----

/// Wake an abstract object from OS2.INI.
fn wake_abstract(h: u32) -> usize {
    let Some(b) = prf_read(HINI_USER, APP_OBJECTS, &hex(h & 0xffff)) else { return 0 };
    let Some((cls, folder_h, recs)) = decode(&b) else { return 0 };
    let c = som::find_class(&cls);
    if c == 0 {
        return 0;
    }
    let folder = if folder_h != 0 { object_from_handle(folder_h) } else { 0 };
    let o = som::sc_call(som::sc::SOM_NEW, &[c]);
    if o == 0 {
        return 0;
    }
    crate::obj::set_handle(o, h);
    register(o, h, b"");
    crate::obj::wake(o, folder, recs);
    o
}

/// The class for a file or directory without a .CLASSINFO.
fn class_for_path(path: &[u8], dir: bool) -> &'static str {
    if dir {
        return if path.len() == 3 { "WPRootFolder" } else { "WPFolder" };
    }
    match extension(path).as_slice() {
        b"EXE" | b"COM" => "WPProgramFile",
        b"CMD" | b"BAT" => "WPCommandFile",
        b"ICO" => "WPIcon",
        b"PTR" => "WPPointer",
        b"BMP" => "WPBitmap",
        b"MET" => "WPMet",
        b"PIF" => "WPPif",
        b"URL" => "WPUrl",
        _ => "WPDataFile",
    }
}

/// The object for a file or directory (woken if need be).
pub fn object_from_path(path: &[u8]) -> usize {
    let path = normalize(path);
    if let Some(o) = by_path(&path) {
        return o;
    }
    let Some(attr) = attributes(&path).or(if path.len() == 3 { Some(FILE_DIRECTORY) } else { None }) else { return 0 };
    let dir = attr & FILE_DIRECTORY != 0;
    let saved = ea_read(&path, b".CLASSINFO").and_then(|(_, b)| decode(&b));
    let mut c = 0;
    if let Some((cls, _, _)) = &saved {
        c = som::find_class(cls);
    }
    if c == 0 {
        c = class(class_for_path(&path, dir));
    }
    if c == 0 {
        return 0;
    }
    // The folder: the parent directory's object (none for a root).
    let folder = if path.len() > 3 {
        let (parent, _) = split_path(&path);
        object_from_path(&parent)
    } else {
        0
    };
    let o = som::sc_call(som::sc::SOM_NEW, &[c]);
    if o == 0 {
        return 0;
    }
    register(o, 0, &path);
    crate::fs::set_real_name_raw(o, &path);
    let recs = saved.map(|s| s.2).unwrap_or_default();
    crate::obj::wake(o, folder, recs);
    o
}

/// Start the Workplace Shell in this process: the classes, the class
/// manager, the registered classes.
pub fn start() -> bool {
    if unsafe { ACTIVE } {
        return true;
    }
    unsafe {
        som::somEnvironmentNew();
    }
    for name in ["WPObject", "WPAbstract", "WPTransient", "WPFileSystem", "WPDataFile", "WPProgramFile", "WPCommandFile", "WPFolder",
        "WPRootFolder", "WPDesktop", "WPDrives", "WPDisk", "WPProgram", "WPShadow", "WPIcon", "WPPointer", "WPBitmap", "WPMet", "WPPif",
        "WPClassManager"]
    {
        class(name);
    }
    crate::clsmgr::install();
    unsafe { ACTIVE = true };
    true
}

pub fn active() -> bool {
    unsafe { ACTIVE }
}

/// A directory moved: the handles of what is in it follow (awake or
/// not).
pub fn fs_handles_moved(old: &[u8], new: &[u8]) {
    let mut pre = normalize(old);
    pre.push(b'\\');
    for k in prf_keys(HINI_USER, APP_HANDLES) {
        let Some(v) = prf_read(HINI_USER, APP_HANDLES, &k) else { continue };
        let p = v.strip_suffix(&[0]).unwrap_or(&v);
        if p.len() > pre.len() && eq_nocase(&p[..pre.len()], &pre) {
            let mut np = normalize(new);
            np.push(b'\\');
            np.extend_from_slice(&p[pre.len()..]);
            prf_write(HINI_USER, APP_HANDLES, &k, Some(&z(&np)));
        }
    }
}

/// A file system handle now stands for a new path (its file moved).
pub fn move_fs_handle(h: u32, new: &[u8]) {
    if h >> 16 == 3 {
        prf_write(HINI_USER, APP_HANDLES, &hex(h & 0xffff), Some(&z(&normalize(new))));
    }
}
