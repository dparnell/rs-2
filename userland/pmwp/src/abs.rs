//! Abstract objects (kept in OS2.INI): WPAbstract, WPProgram (a program
//! with its parameters), WPShadow (a link to another object) and WPDisk
//! (a drive).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::launch;
use crate::methods;
use crate::obj::{self, OPEN_DEFAULT, OPEN_RUNNING, restored_long, restored_string, setup_pairs};
use crate::som;
use crate::util::*;
use crate::wps::{self, Impl};

pub fn impl_abstract() -> Impl {
    Impl::none()
}

// ---- WPProgram ----

#[repr(C)]
pub struct ProgData {
    pub exe: Str,
    pub params: Str,
    pub dir: Str,
    pub progc: u32,
    /// The PROGDETAILS last handed out by wpQueryProgDetails' strings.
    pub env: Str,
}

fn pd<'a>(obj: usize) -> &'a mut ProgData {
    som::data(wps::ccd("WPProgram"), obj)
}

fn progtype(v: &[u8]) -> u32 {
    match v.to_ascii_uppercase().as_slice() {
        b"FULLSCREEN" => launch::PROG_FULLSCREEN,
        b"WINDOWABLEVIO" => launch::PROG_WINDOWABLEVIO,
        b"PM" => launch::PROG_PM,
        _ => launch::PROG_DEFAULT,
    }
}

extern "C" fn prog_setup(obj: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPProgram"), wps::cd("WPObject"), m::WPObject::wpSetup, &[obj, setup as usize]);
    let p = pd(obj);
    for (k, v) in setup_pairs(cstr(setup)) {
        match k.as_slice() {
            b"EXENAME" => p.exe = str_new(&v),
            b"PARAMETERS" => p.params = str_new(&v),
            b"STARTUPDIR" => p.dir = str_new(&v),
            b"PROGTYPE" => p.progc = progtype(&v),
            b"ASSOCFILTER" => {
                // PMWP_ASSOC_FILTER: a pattern's programs (handles in
                // decimal, NUL separated), as IBM's shell keeps them.
                let h = alloc::format!("{}", obj::handle(obj)).into_bytes();
                for pat in v.split(|&c| c == b',').map(|f| f.trim_ascii()).filter(|f| !f.is_empty()) {
                    let pat = pat.to_ascii_uppercase();
                    let old = prf_read(HINI_USER, b"PMWP_ASSOC_FILTER", &pat).unwrap_or_default();
                    let mut list: Vec<Vec<u8>> = old.split(|&c| c == 0).filter(|x| !x.is_empty()).map(|x| x.to_vec()).collect();
                    if !list.contains(&h) {
                        list.push(h.clone());
                    }
                    let mut val = Vec::new();
                    for x in list {
                        val.extend_from_slice(&x);
                        val.push(0);
                    }
                    prf_write(HINI_USER, b"PMWP_ASSOC_FILTER", &pat, Some(&val));
                }
            }
            _ => {}
        }
    }
    1
}

const PCLS: &[u8] = b"WPProgram\0";

extern "C" fn prog_save_state(obj: usize) -> u32 {
    som::call_parent(wps::ccd("WPProgram"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[obj]);
    let p = pd(obj);
    let c = PCLS.as_ptr();
    obj::wpSaveString(obj, c, 1, str_ptr(&p.exe));
    obj::wpSaveString(obj, c, 2, str_ptr(&p.params));
    obj::wpSaveString(obj, c, 3, str_ptr(&p.dir));
    obj::wpSaveLong(obj, c, 4, p.progc);
    1
}

extern "C" fn prog_restore_state(obj: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPProgram"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[obj, r as usize]);
    let p = pd(obj);
    if let Some(v) = restored_string(obj, b"WPProgram", 1) {
        p.exe = str_new(&v);
    }
    if let Some(v) = restored_string(obj, b"WPProgram", 2) {
        p.params = str_new(&v);
    }
    if let Some(v) = restored_string(obj, b"WPProgram", 3) {
        p.dir = str_new(&v);
    }
    if let Some(v) = restored_long(obj, b"WPProgram", 4) {
        p.progc = v;
    }
    1
}

extern "C" fn prog_uninit(obj: usize) {
    let p = pd(obj);
    p.exe = None;
    p.params = None;
    p.dir = None;
    p.env = None;
    som::call_parent(wps::ccd("WPProgram"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[obj]);
}

extern "C" fn prog_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_RUNNING || view == OPEN_DEFAULT {
        let p = pd(obj);
        let (e, a, d) = (str_bytes(&p.exe).to_vec(), str_bytes(&p.params).to_vec(), str_bytes(&p.dir).to_vec());
        let t = obj::title(obj);
        launch::start(&e, &a, &d, &t, p.progc);
        return 0;
    }
    som::call_parent(wps::ccd("WPProgram"), wps::cd("WPObject"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize])
        as u32
}

/// PROGDETAILS: Length, progt (progc, fbVisible), pszTitle, pszExecutable,
/// pszParameters, pszStartupDir, pszIcon, pszEnvironment, swpInitial.
const PROGDETAILS_SIZE: usize = 72;

pub fn fill_prog_details(title: &[u8], exe: &[u8], params: &[u8], dir: &[u8], progc: u32, buf: *mut u8, cb: *mut u32) -> u32 {
    let strings: [&[u8]; 4] = [title, exe, params, dir];
    let need = PROGDETAILS_SIZE + strings.iter().map(|s| s.len() + 1).sum::<usize>();
    if cb.is_null() {
        return 0;
    }
    unsafe {
        if buf.is_null() || (*cb as usize) < need {
            *cb = need as u32;
            return buf.is_null() as u32;
        }
        core::ptr::write_bytes(buf, 0, PROGDETAILS_SIZE);
        let w = buf as *mut u32;
        *w = PROGDETAILS_SIZE as u32;
        *w.add(1) = progc;
        let mut at = buf.add(PROGDETAILS_SIZE);
        for (i, s) in strings.iter().enumerate() {
            *w.add(3 + i) = at as u32;
            core::ptr::copy_nonoverlapping(s.as_ptr(), at, s.len());
            *at.add(s.len()) = 0;
            at = at.add(s.len() + 1);
        }
        *cb = need as u32;
    }
    1
}

extern "C" fn wpQueryProgDetails(obj: usize, buf: *mut u8, cb: *mut u32) -> u32 {
    let p = pd(obj);
    let t = obj::title(obj);
    fill_prog_details(&t, str_bytes(&p.exe), str_bytes(&p.params), str_bytes(&p.dir), p.progc, buf, cb)
}

extern "C" fn wpSetProgDetails(obj: usize, det: *const u32) -> u32 {
    if det.is_null() {
        return 0;
    }
    let p = pd(obj);
    unsafe {
        p.progc = *det.add(1);
        let s = |i: usize| cstr(*det.add(i) as *const u8).to_vec();
        if *det.add(4) != 0 {
            p.exe = str_new(&s(4));
        }
        if *det.add(5) != 0 {
            p.params = str_new(&s(5));
        }
        if *det.add(6) != 0 {
            p.dir = str_new(&s(6));
        }
        if *det.add(3) != 0 {
            let t = z(&s(3));
            obj::call(m::WPObject::wpSetTitle, &[obj, t.as_ptr() as usize]);
        }
    }
    obj::call(m::WPObject::wpSaveDeferred, &[obj]);
    1
}

// ---- WPShadow ----

#[repr(C)]
pub struct ShadowData {
    pub link: u32,
}

fn sd<'a>(obj: usize) -> &'a mut ShadowData {
    som::data(wps::ccd("WPShadow"), obj)
}

pub fn linked(obj: usize) -> usize {
    let h = sd(obj).link;
    if h == 0 { 0 } else { wps::object_from_handle(h) }
}

extern "C" fn wpSetLinkToObject(obj: usize, to: usize) -> u32 {
    if to == 0 {
        return 0;
    }
    sd(obj).link = obj::handle(to);
    obj::call(m::WPObject::wpSaveDeferred, &[obj]);
    1
}

extern "C" fn wpQueryShadowedObject(obj: usize, _lock: u32) -> usize {
    linked(obj)
}

extern "C" fn shadow_setup(obj: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPShadow"), wps::cd("WPObject"), m::WPObject::wpSetup, &[obj, setup as usize]);
    for (k, v) in setup_pairs(cstr(setup)) {
        if k == b"SHADOWID" {
            let to = wps::object_from_location(&v);
            if to != 0 {
                sd(obj).link = obj::handle(to);
            }
        }
    }
    1
}

extern "C" fn shadow_save_state(obj: usize) -> u32 {
    som::call_parent(wps::ccd("WPShadow"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[obj]);
    obj::wpSaveLong(obj, b"WPShadow\0".as_ptr(), 1, sd(obj).link);
    1
}

extern "C" fn shadow_restore_state(obj: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPShadow"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[obj, r as usize]);
    if let Some(v) = restored_long(obj, b"WPShadow", 1) {
        sd(obj).link = v;
    }
    1
}

/// A shadow shows its object's title and icon, and opens it.
extern "C" fn shadow_title(obj: usize) -> *const u8 {
    let l = linked(obj);
    if l != 0 {
        return obj::call(m::WPObject::wpQueryTitle, &[l]) as *const u8;
    }
    som::call_parent(wps::ccd("WPShadow"), wps::cd("WPObject"), m::WPObject::wpQueryTitle, &[obj]) as *const u8
}

extern "C" fn shadow_icon(obj: usize) -> u32 {
    let l = linked(obj);
    if l != 0 {
        return obj::call(m::WPObject::wpQueryIcon, &[l]) as u32;
    }
    som::call_parent(wps::ccd("WPShadow"), wps::cd("WPObject"), m::WPObject::wpQueryIcon, &[obj]) as u32
}

extern "C" fn shadow_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    let l = linked(obj);
    if l == 0 {
        return 0;
    }
    obj::call(m::WPObject::wpViewObject, &[l, cnr as usize, view as usize, param as usize]) as u32
}

// ---- WPDisk ----

#[repr(C)]
pub struct DiskData {
    /// 1 = A:.
    pub drive: u32,
}

fn dd<'a>(obj: usize) -> &'a mut DiskData {
    som::data(wps::ccd("WPDisk"), obj)
}

extern "C" fn wpQueryLogicalDrive(obj: usize) -> u32 {
    dd(obj).drive
}

pub fn root_of(obj: usize) -> usize {
    let d = dd(obj).drive;
    if d == 0 || d > 26 {
        return 0;
    }
    let path = [b'A' + d as u8 - 1, b':', b'\\'];
    wps::object_from_path(&path)
}

extern "C" fn wpQueryRootFolder(obj: usize) -> usize {
    root_of(obj)
}

extern "C" fn disk_setup(obj: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPDisk"), wps::cd("WPObject"), m::WPObject::wpSetup, &[obj, setup as usize]);
    for (k, v) in setup_pairs(cstr(setup)) {
        if k == b"DRIVE" {
            if let Some(&c) = v.first() {
                if c.is_ascii_alphabetic() {
                    dd(obj).drive = (c.to_ascii_uppercase() - b'A' + 1) as u32;
                }
            }
        }
    }
    1
}

extern "C" fn disk_save_state(obj: usize) -> u32 {
    som::call_parent(wps::ccd("WPDisk"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[obj]);
    obj::wpSaveLong(obj, b"WPDisk\0".as_ptr(), 1, dd(obj).drive);
    1
}

extern "C" fn disk_restore_state(obj: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPDisk"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[obj, r as usize]);
    if let Some(v) = restored_long(obj, b"WPDisk", 1) {
        dd(obj).drive = v;
    }
    1
}

extern "C" fn disk_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    let root = root_of(obj);
    if root == 0 {
        return 0;
    }
    obj::call(m::WPObject::wpViewObject, &[root, cnr as usize, view as usize, param as usize]) as u32
}

methods! {
    __m_prog_setup => prog_setup, 2;
    __m_prog_save_state => prog_save_state, 1;
    __m_prog_restore_state => prog_restore_state, 2;
    __m_prog_uninit => prog_uninit, 1;
    __m_prog_open => prog_open, 4;
    __m_wpQueryProgDetails => wpQueryProgDetails, 3;
    __m_wpSetProgDetails => wpSetProgDetails, 2;
    __m_wpSetLinkToObject => wpSetLinkToObject, 2;
    __m_wpQueryShadowedObject => wpQueryShadowedObject, 2;
    __m_shadow_setup => shadow_setup, 2;
    __m_shadow_save_state => shadow_save_state, 1;
    __m_shadow_restore_state => shadow_restore_state, 2;
    __m_shadow_title => shadow_title, 1;
    __m_shadow_icon => shadow_icon, 1;
    __m_shadow_open => shadow_open, 4;
    __m_wpQueryLogicalDrive => wpQueryLogicalDrive, 1;
    __m_wpQueryRootFolder => wpQueryRootFolder, 1;
    __m_disk_setup => disk_setup, 2;
    __m_disk_save_state => disk_save_state, 1;
    __m_disk_restore_state => disk_restore_state, 2;
    __m_disk_open => disk_open, 4;
}

pub fn impl_program() -> Impl {
    Impl {
        data: core::mem::size_of::<ProgData>() as u32,
        methods: alloc::vec![
            ("wpQueryProgDetails", __m_wpQueryProgDetails as usize),
            ("wpSetProgDetails", __m_wpSetProgDetails as usize),
        ]
        .into_iter()
        .chain(crate::book::program_methods())
        .collect(),
        overrides: alloc::vec![
            ("wpSetup", __m_prog_setup as usize),
            ("wpSaveState", __m_prog_save_state as usize),
            ("wpRestoreState", __m_prog_restore_state as usize),
            ("wpUnInitData", __m_prog_uninit as usize),
            ("wpOpen", __m_prog_open as usize),
        ]
        .into_iter()
        .chain(crate::book::program_overrides())
        .chain(crate::dnd::overrides("WPProgram"))
        .collect(),
    }
}

pub fn impl_shadow() -> Impl {
    Impl {
        data: core::mem::size_of::<ShadowData>() as u32,
        methods: alloc::vec![
            ("wpSetLinkToObject", __m_wpSetLinkToObject as usize),
            ("wpQueryShadowedObject", __m_wpQueryShadowedObject as usize),
        ],
        overrides: alloc::vec![
            ("wpSetup", __m_shadow_setup as usize),
            ("wpSaveState", __m_shadow_save_state as usize),
            ("wpRestoreState", __m_shadow_restore_state as usize),
            ("wpQueryTitle", __m_shadow_title as usize),
            ("wpQueryIcon", __m_shadow_icon as usize),
            ("wpOpen", __m_shadow_open as usize),
        ]
        .into_iter()
        .chain(crate::dnd::overrides("WPShadow"))
        .collect(),
    }
}

pub fn impl_disk() -> Impl {
    Impl {
        data: core::mem::size_of::<DiskData>() as u32,
        methods: alloc::vec![
            ("wpQueryLogicalDrive", __m_wpQueryLogicalDrive as usize),
            ("wpQueryRootFolder", __m_wpQueryRootFolder as usize),
        ],
        overrides: alloc::vec![
            ("wpSetup", __m_disk_setup as usize),
            ("wpSaveState", __m_disk_save_state as usize),
            ("wpRestoreState", __m_disk_restore_state as usize),
            ("wpOpen", __m_disk_open as usize),
        ]
        .into_iter()
        .chain(crate::dnd::overrides("WPDisk"))
        .collect(),
    }
}

pub fn _unused(_: Vec<u8>) {}
