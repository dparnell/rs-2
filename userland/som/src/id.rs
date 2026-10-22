//! somIds: `char **`, a pointer to a pointer to a name. Registering an
//! id rewrites `*id` to point to the id's entry, which starts with a zero
//! word (so `**(long **)id == 0` tells a registered id from a name) and
//! then the name: IBM's SOM does the same, and code compiled for it keeps
//! its ids in static variables that SOM rewrites. Names compare without
//! regard to case, and a leading "::" is ignored. Two registered ids are
//! equal when they point to the same entry.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::sys::*;

#[repr(C)]
pub struct Entry {
    zero: u32,
    pub name: *const u8,
    pad: u32,
}

pub type SomId = *mut *const u8;

static mut TABLE: Option<BTreeMap<Vec<u8>, *mut Entry>> = None;
static mut COUNT: u32 = 0;
static mut PERSISTENT: u32 = 0;

fn key(name: &[u8]) -> Vec<u8> {
    let name = name.strip_prefix(b"::").unwrap_or(name);
    name.iter().map(|c| c.to_ascii_uppercase()).collect()
}

/// The entry for a name, made if new (`new` tells whether it was).
pub fn entry(name: &[u8]) -> (*mut Entry, bool) {
    let _g = lock();
    #[allow(static_mut_refs)]
    let t = unsafe { TABLE.get_or_insert_with(BTreeMap::new) };
    let k = key(name);
    if let Some(&e) = t.get(&k) {
        return (e, false);
    }
    let name = name.strip_prefix(b"::").unwrap_or(name);
    let mut copy = Vec::with_capacity(name.len() + 1);
    copy.extend_from_slice(name);
    copy.push(0);
    let e = leak(Entry { zero: 0, name: leak_vec(copy), pad: 0 });
    t.insert(k, e);
    unsafe { COUNT += 1 };
    (e, true)
}

pub fn is_registered(id: SomId) -> bool {
    !id.is_null() && unsafe { !(*id).is_null() && *(*id as *const u32) == 0 }
}

/// Register `id` (rewriting it); returns its entry and whether the name
/// was new.
pub fn register(id: SomId) -> (*mut Entry, bool) {
    if id.is_null() || unsafe { (*id).is_null() } {
        return (core::ptr::null_mut(), false);
    }
    if is_registered(id) {
        return (unsafe { *id as *mut Entry }, false);
    }
    let (e, new) = entry(unsafe { cstr(*id) });
    unsafe { *id = e as *const u8 };
    (e, new)
}

/// The unique key of an id (its entry), 0 for none.
pub fn key_of(id: SomId) -> usize {
    register(id).0 as usize
}

/// The name of an id.
pub fn name_of(id: SomId) -> *const u8 {
    let e = register(id).0;
    if e.is_null() { core::ptr::null() } else { unsafe { (*e).name } }
}

pub fn name_bytes<'a>(id: SomId) -> &'a [u8] {
    unsafe { cstr(name_of(id)) }
}

/// The entry of a name given as bytes.
pub fn key_of_name(name: &[u8]) -> usize {
    entry(name).0 as usize
}

/// A new id (from SOMMalloc: the caller frees it with SOMFree).
pub fn from_string(s: *const u8) -> SomId {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    let e = entry(unsafe { cstr(s) }).0;
    let id = crate::malloc(4) as SomId;
    if !id.is_null() {
        unsafe { *id = e as *const u8 };
    }
    id
}

/// A registered id for a name, never freed (for SOM's own use).
pub fn static_id(name: &[u8]) -> SomId {
    let e = entry(name).0;
    leak(e as *const u8)
}

pub fn total() -> u32 {
    unsafe { COUNT }
}

pub fn set_persistent(on: bool) {
    unsafe { PERSISTENT = on as u32 };
}

// ---- Exports ----

#[unsafe(no_mangle)]
pub extern "C" fn somCheckId(id: SomId) -> SomId {
    register(id);
    id
}

#[unsafe(no_mangle)]
pub extern "C" fn somRegisterId(id: SomId) -> u32 {
    register(id).1 as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn somIdFromString(s: *const u8) -> SomId {
    from_string(s)
}

#[unsafe(no_mangle)]
pub extern "C" fn somStringFromId(id: SomId) -> *const u8 {
    name_of(id)
}

#[unsafe(no_mangle)]
pub extern "C" fn somCompareIds(a: SomId, b: SomId) -> u32 {
    (key_of(a) == key_of(b) && key_of(a) != 0) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn somTotalRegIds() -> u32 {
    total()
}

#[unsafe(no_mangle)]
pub extern "C" fn somSetExpectedIds(_n: u32) {}

#[unsafe(no_mangle)]
pub extern "C" fn somUniqueKey(id: SomId) -> u32 {
    key_of(id) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn somBeginPersistentIds() {
    set_persistent(true);
}

#[unsafe(no_mangle)]
pub extern "C" fn somEndPersistentIds() {
    set_persistent(false);
}
