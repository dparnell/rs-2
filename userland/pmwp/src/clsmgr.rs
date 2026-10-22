//! WPClassManager (the Workplace Shell's SOM class manager: classes are
//! found in the DLLs registered for them) and the registered classes:
//! OS2SYS.INI's `PM_Objects` `ClassTable`, pairs of NUL-terminated class
//! and DLL names. WinRegisterObjectClass and friends are in api.rs.

use alloc::vec::Vec;

use crate::methods;
use crate::som::{self, SomId};
use crate::util::*;
use crate::wps::{self, Impl};

const APP: &[u8] = b"PM_Objects";
const KEY: &[u8] = b"ClassTable";

/// The registered classes: (class, DLL).
pub fn table() -> Vec<(Vec<u8>, Vec<u8>)> {
    let v = prf_read(HINI_SYSTEM, APP, KEY).unwrap_or_default();
    let parts: Vec<&[u8]> = v.split(|&c| c == 0).collect();
    parts.chunks(2).filter(|p| p.len() == 2 && !p[0].is_empty()).map(|p| (p[0].to_vec(), p[1].to_vec())).collect()
}

pub fn set_table(t: &[(Vec<u8>, Vec<u8>)]) -> bool {
    let mut v = Vec::new();
    for (c, d) in t {
        v.extend_from_slice(c);
        v.push(0);
        v.extend_from_slice(d);
        v.push(0);
    }
    prf_write(HINI_SYSTEM, APP, KEY, Some(&v))
}

pub fn module_of(class: &[u8]) -> Option<Vec<u8>> {
    table().into_iter().find(|(c, _)| eq_nocase(c, class)).map(|(_, d)| d)
}

/// somLocateClassFile: the DLL registered for the class.
extern "C" fn locate(mgr: usize, cid: SomId, major: i32, minor: i32) -> *const u8 {
    let name = cstr(unsafe { som::somStringFromId(cid) });
    // PMWP.DLL's own classes are built when first asked for.
    if let Ok(n) = core::str::from_utf8(name) {
        if wps::index_of(n).is_some() && wps::class(n) != 0 {
            return b"PMWP\0".as_ptr();
        }
    }
    if let Some(d) = module_of(name) {
        return alloc::boxed::Box::leak(z(&d).into_boxed_slice()).as_ptr();
    }
    let p = som::parent(wps::ccd("WPClassManager"), unsafe { &raw const som::SOMClassMgrClassData } as *const usize, som::scm::SOM_LOCATE_CLASS_FILE);
    if p == 0 {
        return core::ptr::null();
    }
    let f: extern "C" fn(usize, SomId, i32, i32) -> *const u8 = unsafe { core::mem::transmute(p) };
    f(mgr, cid, major, minor)
}

methods! {
    __m_locate => locate, 4;
}

pub fn impl_class_manager() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("somLocateClassFile", __m_locate as usize)] }
}

/// Make a WPClassManager the class manager (SOMClassMgr's registry
/// merged into it).
pub fn install() {
    let cls = wps::class("WPClassManager");
    if cls == 0 {
        return;
    }
    let old = unsafe { som::SOMClassMgrObject };
    if som::is_a(old, cls) {
        return;
    }
    let mgr = som::sc_call(som::sc::SOM_NEW, &[cls]);
    if mgr != 0 {
        som::call(unsafe { &raw const som::SOMClassMgrClassData } as *const usize, som::scm::SOM_MERGE_INTO, &[old, mgr]);
    }
}
