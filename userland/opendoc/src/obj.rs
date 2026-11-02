//! ODObject, ODRefCntObject and ODPersistentObject: the roots of OpenDoc's
//! class tree (extensions, reference counts, a storage unit and an ID).

use crate::rt::*;
use crate::*;

// ---- ODObject ----

pub mod ix {
    pub const INIT_OBJECT: usize = 0;
    pub const HAS_EXTENSION: usize = 1;
    pub const ACQUIRE_EXTENSION: usize = 2;
    pub const RELEASE_EXTENSION: usize = 3;
    pub const PURGE: usize = 4;
    pub const IS_EQUAL_TO: usize = 5;
    pub const SUBCLASS_RESPONSIBILITY: usize = 6;
    // ODRefCntObject
    pub const INIT_REF_CNT_OBJECT: usize = 0;
    pub const ACQUIRE: usize = 1;
    pub const RELEASE: usize = 2;
    pub const GET_REF_COUNT: usize = 3;
    // ODPersistentObject
    pub const INIT_PERSISTENT_OBJECT: usize = 0;
    pub const INIT_PERSISTENT_OBJECT_FROM_STORAGE: usize = 1;
    pub const RELEASE_ALL: usize = 2;
    pub const EXTERNALIZE: usize = 3;
    pub const GET_STORAGE_UNIT: usize = 4;
    pub const GET_ID: usize = 5;
    pub const CLONE_INTO: usize = 6;
}

methods! {
    fn InitObject(this: usize, ev: usize) -> usize { 0 }
    fn HasExtension(this: usize, ev: usize, name: usize) -> usize { 0 }
    fn AcquireExtension(this: usize, ev: usize, name: usize) -> usize { 0 }
    fn ReleaseExtension(this: usize, ev: usize, ext: usize) -> usize { 0 }
    fn Purge(this: usize, ev: usize, size: usize) -> usize { 0 }
    fn IsEqualTo(this: usize, ev: usize, other: usize) -> usize { (this == other) as usize }
    fn SubClassResponsibility(this: usize, ev: usize) -> usize {
        od_error(ev, err::SUBCLASS_RESPONSIBILITY);
        0
    }
}

pub static OBJECT_M: &[(&str, M)] = &[
    ("InitObject", InitObject::t),
    ("HasExtension", HasExtension::t),
    ("AcquireExtension", AcquireExtension::t),
    ("ReleaseExtension", ReleaseExtension::t),
    ("Purge", Purge::t),
    ("IsEqualTo", IsEqualTo::t),
    ("SubClassResponsibility", SubClassResponsibility::t),
];

// ---- ODRefCntObject: the count is its instance data ----

fn count<'a>(this: usize) -> &'a mut i32 {
    somsup::data(cd!(ODRefCntObjectCClassData), this)
}

/// The reference count of an object (any ODRefCntObject).
pub fn ref_count(this: usize) -> i32 {
    *count(this)
}

methods! {
    fn RcSomInit(this: usize) -> usize {
        call_parent(cd!(ODRefCntObjectCClassData), somsup::so(), somsup::om::SOM_INIT, &[this]);
        *count(this) = 1;
        0
    }
    fn InitRefCntObject(this: usize, ev: usize) -> usize {
        *count(this) = 1;
        0
    }
    fn Acquire(this: usize, ev: usize) -> usize {
        *count(this) += 1;
        0
    }
    fn Release(this: usize, ev: usize) -> usize {
        let c = count(this);
        *c -= 1;
        if *c == 0 {
            som_free(this);
        }
        0
    }
    fn GetRefCount(this: usize, ev: usize) -> usize { *count(this) as usize }
}

pub static REFCNT_M: &[(&str, M)] = &[
    ("InitRefCntObject", InitRefCntObject::t),
    ("Acquire", Acquire::t),
    ("Release", Release::t),
    ("GetRefCount", GetRefCount::t),
];
pub static REFCNT_O: &[(&str, M)] = &[("SOMObject::somInit", RcSomInit::t)];

/// Acquire and Release any ODRefCntObject through its tokens.
pub fn acquire(obj: usize, ev: usize) -> usize {
    if obj != 0 {
        call(cd!(ODRefCntObjectClassData), ix::ACQUIRE, &[obj, ev]);
    }
    obj
}

pub fn release(obj: usize, ev: usize) {
    if obj != 0 {
        call(cd!(ODRefCntObjectClassData), ix::RELEASE, &[obj, ev]);
    }
}

// ---- ODPersistentObject: the storage unit is its instance data ----

pub fn su_of(this: usize) -> usize {
    *somsup::data::<usize>(cd!(ODPersistentObjectCClassData), this)
}

pub fn set_su(this: usize, su: usize) {
    *somsup::data::<usize>(cd!(ODPersistentObjectCClassData), this) = su;
}

methods! {
    fn InitPersistentObject(this: usize, ev: usize, su: usize) -> usize {
        call(cd!(ODObjectClassData), ix::INIT_OBJECT, &[this, ev]);
        set_su(this, acquire(su, ev));
        0
    }
    fn InitPersistentObjectFromStorage(this: usize, ev: usize, su: usize) -> usize {
        call(cd!(ODObjectClassData), ix::INIT_OBJECT, &[this, ev]);
        set_su(this, acquire(su, ev));
        0
    }
    fn ReleaseAll(this: usize, ev: usize) -> usize { 0 }
    fn Externalize(this: usize, ev: usize) -> usize { 0 }
    fn GetStorageUnit(this: usize, ev: usize) -> usize { su_of(this) }
    fn GetID(this: usize, ev: usize) -> usize {
        let su = su_of(this);
        if su == 0 { 0 } else { crate::storage::su_id(su) as usize }
    }
    fn CloneInto(this: usize, ev: usize, key: usize, to_su: usize, scope: usize) -> usize {
        let su = su_of(this);
        if su != 0 && to_su != 0 {
            crate::storage::copy_su(su, to_su);
        }
        0
    }
    fn PoSomUninit(this: usize) -> usize {
        let su = su_of(this);
        if su != 0 {
            set_su(this, 0);
            release(su, 0);
        }
        call_parent(cd!(ODPersistentObjectCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static PERSISTENT_M: &[(&str, M)] = &[
    ("InitPersistentObject", InitPersistentObject::t),
    ("InitPersistentObjectFromStorage", InitPersistentObjectFromStorage::t),
    ("ReleaseAll", ReleaseAll::t),
    ("Externalize", Externalize::t),
    ("GetStorageUnit", GetStorageUnit::t),
    ("GetID", GetID::t),
    ("CloneInto", CloneInto::t),
];
pub static PERSISTENT_O: &[(&str, M)] = &[("SOMObject::somUninit", PoSomUninit::t)];
