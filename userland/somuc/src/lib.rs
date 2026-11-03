//! SOMUC.DLL: SOM's utility classes, rs2's own.
//!
//! IBM's names, ordinals and release orders (from the SOMobjects
//! Toolkit's IDL: mcollect.idl, tcollect.idl, tdeq.idl...); the code is
//! rs2's. The classes:
//!
//! - **somf_MCollectible** (the root of OpenDoc's Apple-event terminology
//!   classes): somfClone, somfClonePointer, somfHash (the object's
//!   address), somfIsEqual (somfIsSame), somfIsSame (the same object),
//!   somfIsNotEqual. somf_MOrderableCollectible adds the comparisons
//!   (subclasses implement somfCompare; the others are made from it).
//! - Values: somf_TCollectibleLong, somf_TAssoc (key and value).
//! - Links: somf_MLinkable, somf_TDequeLinkable; somf_TPrimitiveLinkedList
//!   (a list of links) and its iterator.
//! - Collections (somf_TCollection: Add, Remove, Member, Count, iterators,
//!   with a test function, the token of a compare method, somfIsEqual by
//!   default): somf_TSequence, somf_TDeque, somf_TSet, somf_TDictionary
//!   (keys and values), somf_TPriorityQueue (pops the greatest),
//!   somf_TSortedSequence (kept in order by its sequencing function);
//!   somf_THashTable (keys and values); somf_TSortedSequenceNode. Each
//!   has its iterator class (somfFirst, somfNext, somfRemove; sequences
//!   also somfLast, somfPrevious).
//!
//! A test function is a method token called as `fn(obj, ev, other)`; a
//! hash function `fn(obj, ev)`; an orderable compare function returns
//! kLessThan (0), kEqual (1) or kGreaterThan (2).

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, static_mut_refs, function_casts_as_integer, clippy::missing_safety_doc)]

extern crate alloc;
extern crate os2;

use alloc::boxed::Box;
use alloc::vec::Vec;

use somsup::{ClassSpec, aligned, build_class};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, term: u32) -> u32 {
    if term == 0 {
        unsafe {
            if somsup::SOMClassMgrObject == 0 {
                somsup::somEnvironmentNew();
            }
        }
    }
    1
}

// ---- method trampolines ----

macro_rules! tramp {
    ($tramp:ident => $target:path, $copy:expr) => {
        #[unsafe(naked)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "sub esp, {pad}",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 8]", "lea edi, [esp + 12]", "mov ecx, {n}", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                pad = const ($copy * 4 + 15) / 16 * 16, n = const $copy, f = sym $target,
            );
        }
    };
}

macro_rules! one {
    ($x:ident) => {
        1
    };
}

/// `fn name(args) -> usize { }` and `name::t`, the method table entry.
macro_rules! meths {
    ($(fn $name:ident($($a:ident: $t:ty),* $(,)?) -> usize $body:block)*) => {$(
        #[allow(non_snake_case, unused_variables, clippy::all)]
        pub extern "C" fn $name($($a: $t),*) -> usize $body
        #[allow(non_snake_case)]
        pub mod $name {
            tramp!(t => super::$name, (0 $(+ one!($a))*));
        }
    )*};
}

// ---- class data ----

macro_rules! class_data {
    ($($cd:ident, $ccd:ident, $n:expr;)*) => {$(
        #[unsafe(no_mangle)]
        pub static mut $cd: [usize; $n + 4] = [0; $n + 4];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

class_data! {
    somf_MCollectibleClassData, somf_MCollectibleCClassData, 6;
    somf_MOrderableCollectibleClassData, somf_MOrderableCollectibleCClassData, 11;
    somf_MLinkableClassData, somf_MLinkableCClassData, 5;
    somf_TAssocClassData, somf_TAssocCClassData, 6;
    somf_TCollectibleLongClassData, somf_TCollectibleLongCClassData, 3;
    somf_TCollectionClassData, somf_TCollectionCClassData, 11;
    somf_TIteratorClassData, somf_TIteratorCClassData, 3;
    somf_TSequenceClassData, somf_TSequenceCClassData, 6;
    somf_TSequenceIteratorClassData, somf_TSequenceIteratorCClassData, 2;
    somf_TDequeClassData, somf_TDequeCClassData, 15;
    somf_TDequeIteratorClassData, somf_TDequeIteratorCClassData, 1;
    somf_TDequeLinkableClassData, somf_TDequeLinkableCClassData, 4;
    somf_TDictionaryClassData, somf_TDictionaryCClassData, 23;
    somf_TDictionaryIteratorClassData, somf_TDictionaryIteratorCClassData, 1;
    somf_THashTableClassData, somf_THashTableCClassData, 26;
    somf_THashTableIteratorClassData, somf_THashTableIteratorCClassData, 1;
    somf_TPrimitiveLinkedListClassData, somf_TPrimitiveLinkedListCClassData, 13;
    somf_TPrimitiveLinkedListIteratorClassData, somf_TPrimitiveLinkedListIteratorCClassData, 5;
    somf_TPriorityQueueClassData, somf_TPriorityQueueCClassData, 15;
    somf_TPriorityQueueIteratorClassData, somf_TPriorityQueueIteratorCClassData, 1;
    somf_TSetClassData, somf_TSetCClassData, 21;
    somf_TSetIteratorClassData, somf_TSetIteratorCClassData, 1;
    somf_TSortedSequenceClassData, somf_TSortedSequenceCClassData, 12;
    somf_TSortedSequenceIteratorClassData, somf_TSortedSequenceIteratorCClassData, 2;
    somf_TSortedSequenceNodeClassData, somf_TSortedSequenceNodeCClassData, 14;
}

macro_rules! p {
    ($a:ident) => {
        (&raw mut $a) as *mut usize
    };
}

// ---- helpers ----

fn state<T: Default>(ccd: *const usize, obj: usize) -> &'static mut T {
    let slot: &mut usize = somsup::data(ccd, obj);
    if *slot == 0 {
        *slot = Box::into_raw(Box::new(T::default())) as usize;
    }
    unsafe { &mut *(*slot as *mut T) }
}

fn drop_state<T>(ccd: *const usize, obj: usize) {
    let slot: &mut usize = somsup::data(ccd, obj);
    if *slot != 0 {
        drop(unsafe { Box::from_raw(*slot as *mut T) });
        *slot = 0;
    }
}

fn call(cd: *const usize, i: usize, args: &[usize]) -> usize {
    somsup::call(cd, i, args)
}

/// MCollectible's method tokens.
mod mc {
    pub const CLONE: usize = 0;
    pub const HASH: usize = 2;
    pub const IS_EQUAL: usize = 3;
    pub const IS_SAME: usize = 4;
}

fn token(cd: *const usize, i: usize) -> usize {
    unsafe { *cd.add(1 + i) }
}

fn is_equal_token() -> usize {
    token(p!(somf_MCollectibleClassData), mc::IS_EQUAL)
}

fn hash_token() -> usize {
    token(p!(somf_MCollectibleClassData), mc::HASH)
}

fn compare_token() -> usize {
    token(p!(somf_MOrderableCollectibleClassData), 8)
}

/// `test(a, ev, b)` of a test function (a method token).
fn test(f: usize, ev: usize, a: usize, b: usize) -> bool {
    if a == 0 || b == 0 {
        return a == b;
    }
    somsup::call_fn(if f == 0 { is_equal_token() } else { f }, &[a, ev, b]) & 0xff != 0
}

/// An orderable comparison: -1, 0 or 1.
fn compare(f: usize, ev: usize, a: usize, b: usize) -> i32 {
    match somsup::call_fn(if f == 0 { compare_token() } else { f }, &[a, ev, b]) & 0xff {
        0 => -1,
        1 => 0,
        _ => 1,
    }
}

fn new_of(cd: *mut usize, make: extern "C" fn(i32, i32) -> usize) -> usize {
    unsafe {
        if *cd == 0 {
            make(2, 1);
        }
        somsup::new_object(*cd)
    }
}

fn class_is(obj: usize, cd: *mut usize) -> bool {
    obj != 0 && unsafe { *cd != 0 } && somsup::is_a(obj, unsafe { *cd })
}

fn som_init_parent(ccd: *const usize, obj: usize) {
    somsup::call_parent(ccd, somsup::so(), somsup::om::SOM_INIT, &[obj]);
}

fn som_uninit_parent(ccd: *const usize, obj: usize) {
    somsup::call_parent(ccd, somsup::so(), somsup::om::SOM_UNINIT, &[obj]);
}

// ---- somf_MCollectible, somf_MOrderableCollectible ----

meths! {
    fn mc_clone(this: usize, ev: usize) -> usize {
        let cls = somsup::class_of(this);
        somsup::new_object(cls)
    }
    fn mc_clone_pointer(this: usize, ev: usize, clonee: usize) -> usize {
        if clonee == 0 { 0 } else { call(p!(somf_MCollectibleClassData), mc::CLONE, &[clonee, ev]) }
    }
    fn mc_hash(this: usize, ev: usize) -> usize { this }
    fn mc_is_equal(this: usize, ev: usize, obj: usize) -> usize {
        call(p!(somf_MCollectibleClassData), mc::IS_SAME, &[this, ev, obj]) & 0xff
    }
    fn mc_is_same(this: usize, ev: usize, obj: usize) -> usize { (this == obj) as usize }
    fn mc_is_not_equal(this: usize, ev: usize, obj: usize) -> usize {
        (call(p!(somf_MCollectibleClassData), mc::IS_EQUAL, &[this, ev, obj]) & 0xff == 0) as usize
    }
    // MOrderableCollectible: somfCompare (8) is the subclass's; equal by
    // default.
    fn mo_compare(this: usize, ev: usize, obj: usize) -> usize {
        if this == obj { 1 } else { 1 }
    }
    fn mo_gt(this: usize, ev: usize, obj: usize) -> usize { (compare(0, ev, this, obj) > 0) as usize }
    fn mo_lt(this: usize, ev: usize, obj: usize) -> usize { (compare(0, ev, this, obj) < 0) as usize }
    fn mo_ge(this: usize, ev: usize, obj: usize) -> usize { (compare(0, ev, this, obj) >= 0) as usize }
    fn mo_le(this: usize, ev: usize, obj: usize) -> usize { (compare(0, ev, this, obj) <= 0) as usize }
    fn mo_is_equal(this: usize, ev: usize, obj: usize) -> usize {
        (obj != 0 && compare(0, ev, this, obj) == 0) as usize
    }
}

// ---- somf_TCollectibleLong, somf_TAssoc ----

meths! {
    fn tcl_init(this: usize, ev: usize, v: i32) -> usize {
        *state::<i32>(p!(somf_TCollectibleLongCClassData), this) = v;
        this
    }
    fn tcl_get(this: usize, ev: usize) -> usize { *state::<i32>(p!(somf_TCollectibleLongCClassData), this) as usize }
    fn tcl_set(this: usize, ev: usize, v: i32) -> usize {
        *state::<i32>(p!(somf_TCollectibleLongCClassData), this) = v;
        0
    }
    fn tcl_hash(this: usize, ev: usize) -> usize { *state::<i32>(p!(somf_TCollectibleLongCClassData), this) as usize }
    fn tcl_is_equal(this: usize, ev: usize, obj: usize) -> usize {
        (class_is(obj, p!(somf_TCollectibleLongClassData))
            && *state::<i32>(p!(somf_TCollectibleLongCClassData), this) == *state::<i32>(p!(somf_TCollectibleLongCClassData), obj)) as usize
    }
    fn tcl_clone(this: usize, ev: usize) -> usize {
        let n = new_of(p!(somf_TCollectibleLongClassData), somf_TCollectibleLongNewClass);
        *state::<i32>(p!(somf_TCollectibleLongCClassData), n) = *state::<i32>(p!(somf_TCollectibleLongCClassData), this);
        n
    }
    fn tcl_uninit(this: usize) -> usize {
        drop_state::<i32>(p!(somf_TCollectibleLongCClassData), this);
        som_uninit_parent(p!(somf_TCollectibleLongCClassData), this);
        0
    }
    fn ta_init_mm(this: usize, ev: usize, k: usize, v: usize) -> usize {
        *state::<(usize, usize)>(p!(somf_TAssocCClassData), this) = (k, v);
        this
    }
    fn ta_init_m(this: usize, ev: usize, k: usize) -> usize {
        *state::<(usize, usize)>(p!(somf_TAssocCClassData), this) = (k, 0);
        this
    }
    fn ta_key(this: usize, ev: usize) -> usize { state::<(usize, usize)>(p!(somf_TAssocCClassData), this).0 }
    fn ta_value(this: usize, ev: usize) -> usize { state::<(usize, usize)>(p!(somf_TAssocCClassData), this).1 }
    fn ta_set_key(this: usize, ev: usize, k: usize) -> usize {
        state::<(usize, usize)>(p!(somf_TAssocCClassData), this).0 = k;
        0
    }
    fn ta_set_value(this: usize, ev: usize, v: usize) -> usize {
        state::<(usize, usize)>(p!(somf_TAssocCClassData), this).1 = v;
        0
    }
    // Assocs are equal when their keys are.
    fn ta_is_equal(this: usize, ev: usize, obj: usize) -> usize {
        (class_is(obj, p!(somf_TAssocClassData))
            && test(0, ev, state::<(usize, usize)>(p!(somf_TAssocCClassData), this).0, state::<(usize, usize)>(p!(somf_TAssocCClassData), obj).0)) as usize
    }
    fn ta_hash(this: usize, ev: usize) -> usize {
        let k = state::<(usize, usize)>(p!(somf_TAssocCClassData), this).0;
        if k == 0 { 0 } else { somsup::call_fn(hash_token(), &[k, ev]) }
    }
    fn ta_uninit(this: usize) -> usize {
        drop_state::<(usize, usize)>(p!(somf_TAssocCClassData), this);
        som_uninit_parent(p!(somf_TAssocCClassData), this);
        0
    }
}

// ---- somf_MLinkable, somf_TDequeLinkable, somf_TPrimitiveLinkedList ----

#[derive(Default, Clone, Copy)]
struct Link {
    next: usize,
    prev: usize,
}

fn link(obj: usize) -> &'static mut Link {
    state::<Link>(p!(somf_MLinkableCClassData), obj)
}

meths! {
    fn ml_init(this: usize, ev: usize, n: usize, pv: usize) -> usize {
        *link(this) = Link { next: n, prev: pv };
        this
    }
    fn ml_next(this: usize, ev: usize) -> usize { link(this).next }
    fn ml_set_next(this: usize, ev: usize, l: usize) -> usize {
        link(this).next = l;
        0
    }
    fn ml_prev(this: usize, ev: usize) -> usize { link(this).prev }
    fn ml_set_prev(this: usize, ev: usize, l: usize) -> usize {
        link(this).prev = l;
        0
    }
    fn ml_uninit(this: usize) -> usize {
        drop_state::<Link>(p!(somf_MLinkableCClassData), this);
        som_uninit_parent(p!(somf_MLinkableCClassData), this);
        0
    }
    fn tdl_init_ddm(this: usize, ev: usize, pv: usize, n: usize, v: usize) -> usize {
        *link(this) = Link { next: n, prev: pv };
        *state::<usize>(p!(somf_TDequeLinkableCClassData), this) = v;
        this
    }
    fn tdl_init_dd(this: usize, ev: usize, pv: usize, n: usize) -> usize {
        *link(this) = Link { next: n, prev: pv };
        this
    }
    fn tdl_get(this: usize, ev: usize) -> usize { *state::<usize>(p!(somf_TDequeLinkableCClassData), this) }
    fn tdl_set(this: usize, ev: usize, v: usize) -> usize {
        *state::<usize>(p!(somf_TDequeLinkableCClassData), this) = v;
        0
    }
    fn tdl_uninit(this: usize) -> usize {
        drop_state::<usize>(p!(somf_TDequeLinkableCClassData), this);
        som_uninit_parent(p!(somf_TDequeLinkableCClassData), this);
        0
    }
}

#[derive(Default)]
struct PList {
    first: usize,
    last: usize,
    count: usize,
}

fn plist(obj: usize) -> &'static mut PList {
    state::<PList>(p!(somf_TPrimitiveLinkedListCClassData), obj)
}

fn pl_insert(this: usize, prev: usize, obj: usize) {
    // obj after prev (0: first)
    let l = plist(this);
    let next = if prev == 0 { l.first } else { link(prev).next };
    *link(obj) = Link { next, prev };
    if prev == 0 { l.first = obj } else { link(prev).next = obj }
    if next == 0 { l.last = obj } else { link(next).prev = obj }
    l.count += 1;
}

fn pl_unlink(this: usize, obj: usize) {
    if obj == 0 {
        return;
    }
    let l = plist(this);
    let Link { next, prev } = *link(obj);
    if prev == 0 { l.first = next } else { link(prev).next = next }
    if next == 0 { l.last = prev } else { link(next).prev = prev }
    *link(obj) = Link::default();
    l.count = l.count.saturating_sub(1);
}

meths! {
    fn pl_count(this: usize, ev: usize) -> usize { plist(this).count }
    fn pl_remove(this: usize, ev: usize, l: usize) -> usize {
        pl_unlink(this, l);
        0
    }
    fn pl_remove_all(this: usize, ev: usize) -> usize {
        while plist(this).first != 0 {
            let f = plist(this).first;
            pl_unlink(this, f);
        }
        0
    }
    fn pl_remove_first(this: usize, ev: usize) -> usize {
        let f = plist(this).first;
        pl_unlink(this, f);
        f
    }
    fn pl_remove_last(this: usize, ev: usize) -> usize {
        let f = plist(this).last;
        pl_unlink(this, f);
        f
    }
    fn pl_add_before(this: usize, ev: usize, existing: usize, obj: usize) -> usize {
        let prev = if existing == 0 { plist(this).last } else { link(existing).prev };
        pl_insert(this, prev, obj);
        0
    }
    fn pl_add_after(this: usize, ev: usize, existing: usize, obj: usize) -> usize {
        pl_insert(this, existing, obj);
        0
    }
    fn pl_add_first(this: usize, ev: usize, obj: usize) -> usize {
        pl_insert(this, 0, obj);
        0
    }
    fn pl_add_last(this: usize, ev: usize, obj: usize) -> usize {
        let last = plist(this).last;
        pl_insert(this, last, obj);
        0
    }
    fn pl_after(this: usize, ev: usize, e: usize) -> usize { if e == 0 { 0 } else { link(e).next } }
    fn pl_before(this: usize, ev: usize, e: usize) -> usize { if e == 0 { 0 } else { link(e).prev } }
    fn pl_first(this: usize, ev: usize) -> usize { plist(this).first }
    fn pl_last(this: usize, ev: usize) -> usize { plist(this).last }
    fn pl_uninit(this: usize) -> usize {
        drop_state::<PList>(p!(somf_TPrimitiveLinkedListCClassData), this);
        som_uninit_parent(p!(somf_TPrimitiveLinkedListCClassData), this);
        0
    }
}

#[derive(Default)]
struct PIter {
    list: usize,
    cur: usize,
}

fn piter(obj: usize) -> &'static mut PIter {
    state::<PIter>(p!(somf_TPrimitiveLinkedListIteratorCClassData), obj)
}

meths! {
    fn pli_init(this: usize, ev: usize, list: usize) -> usize {
        *piter(this) = PIter { list, cur: 0 };
        this
    }
    fn pli_first(this: usize, ev: usize) -> usize {
        let it = piter(this);
        it.cur = if it.list == 0 { 0 } else { plist(it.list).first };
        it.cur
    }
    fn pli_next(this: usize, ev: usize) -> usize {
        let it = piter(this);
        it.cur = if it.cur == 0 { 0 } else { link(it.cur).next };
        it.cur
    }
    fn pli_last(this: usize, ev: usize) -> usize {
        let it = piter(this);
        it.cur = if it.list == 0 { 0 } else { plist(it.list).last };
        it.cur
    }
    fn pli_prev(this: usize, ev: usize) -> usize {
        let it = piter(this);
        it.cur = if it.cur == 0 { 0 } else { link(it.cur).prev };
        it.cur
    }
    fn pli_uninit(this: usize) -> usize {
        drop_state::<PIter>(p!(somf_TPrimitiveLinkedListIteratorCClassData), this);
        som_uninit_parent(p!(somf_TPrimitiveLinkedListIteratorCClassData), this);
        0
    }
}

// ---- collections ----

#[derive(Default, Clone)]
struct Coll {
    /// The items (a dictionary's keys).
    items: Vec<usize>,
    /// A dictionary's values (the same length as the keys).
    vals: Vec<usize>,
    /// The test function (0: somfIsEqual), the hash function (0:
    /// somfHash), a sorted sequence's or priority queue's compare
    /// function (0: somfCompare).
    test: usize,
    hash: usize,
    order: usize,
    /// THashTable's growth rate and rehash threshold (kept for their
    /// getters).
    rate: i32,
    threshold: i32,
}

fn coll(obj: usize) -> &'static mut Coll {
    state::<Coll>(p!(somf_TCollectionCClassData), obj)
}

fn ht(obj: usize) -> &'static mut Coll {
    state::<Coll>(p!(somf_THashTableCClassData), obj)
}

fn find(c: &Coll, ev: usize, obj: usize) -> Option<usize> {
    let t = c.test;
    c.items.iter().position(|&x| test(t, ev, obj, x))
}

fn is_dict(obj: usize) -> bool {
    class_is(obj, p!(somf_TDictionaryClassData))
}

fn is_set(obj: usize) -> bool {
    class_is(obj, p!(somf_TSetClassData))
}

/// Add keeping a sorted sequence or priority queue in order (the
/// greatest last).
fn add_ordered(this: usize, ev: usize, obj: usize) {
    let c = coll(this);
    let f = c.order;
    let at = c.items.iter().position(|&x| compare(f, ev, obj, x) < 0).unwrap_or(c.items.len());
    c.items.insert(at, obj);
}

fn ordered(obj: usize) -> bool {
    class_is(obj, p!(somf_TSortedSequenceClassData)) || class_is(obj, p!(somf_TPriorityQueueClassData))
}

meths! {
    fn tc_init(this: usize, ev: usize, f: usize) -> usize {
        coll(this).test = f;
        this
    }
    fn tc_add(this: usize, ev: usize, obj: usize) -> usize {
        if obj == 0 {
            return 0;
        }
        if is_set(this) {
            if let Some(i) = find(coll(this), ev, obj) {
                return coll(this).items[i];
            }
        }
        if ordered(this) {
            add_ordered(this, ev, obj);
        } else {
            coll(this).items.push(obj);
            if is_dict(this) {
                coll(this).vals.push(0);
            }
        }
        obj
    }
    fn tc_add_all(this: usize, ev: usize, other: usize) -> usize {
        if other != 0 {
            for x in coll(other).items.clone() {
                call(p!(somf_TCollectionClassData), 0, &[this, ev, x]);
            }
        }
        0
    }
    fn tc_remove(this: usize, ev: usize, obj: usize) -> usize {
        let c = coll(this);
        match find(c, ev, obj) {
            Some(i) => {
                let x = c.items.remove(i);
                if i < c.vals.len() {
                    c.vals.remove(i);
                }
                x
            }
            None => 0,
        }
    }
    fn tc_remove_all(this: usize, ev: usize) -> usize {
        let c = coll(this);
        c.items.clear();
        c.vals.clear();
        0
    }
    fn tc_delete_all(this: usize, ev: usize) -> usize {
        let c = core::mem::take(&mut coll(this).items);
        coll(this).vals.clear();
        for x in c {
            somsup::free_object(x);
        }
        0
    }
    fn tc_count(this: usize, ev: usize) -> usize { coll(this).items.len() }
    fn tc_member(this: usize, ev: usize, obj: usize) -> usize {
        let c = coll(this);
        find(c, ev, obj).map_or(0, |i| c.items[i])
    }
    fn tc_create_iterator(this: usize, ev: usize) -> usize { make_iterator(this) }
    fn tc_test_function(this: usize, ev: usize) -> usize {
        let t = coll(this).test;
        if t == 0 { is_equal_token() } else { t }
    }
    fn tc_set_test_function(this: usize, ev: usize, f: usize) -> usize {
        coll(this).test = f;
        0
    }
    // Collections are equal with the same items in the same order.
    fn tc_is_equal(this: usize, ev: usize, obj: usize) -> usize {
        if !class_is(obj, p!(somf_TCollectionClassData)) {
            return 0;
        }
        let (a, b) = (coll(this).items.clone(), coll(obj).items.clone());
        let t = coll(this).test;
        (a.len() == b.len() && a.iter().zip(b.iter()).all(|(&x, &y)| test(t, ev, x, y))) as usize
    }
    fn tc_uninit(this: usize) -> usize {
        drop_state::<Coll>(p!(somf_TCollectionCClassData), this);
        som_uninit_parent(p!(somf_TCollectionCClassData), this);
        0
    }
}

/// The iterator class for a collection.
fn make_iterator(c: usize) -> usize {
    let it = if class_is(c, p!(somf_TDequeClassData)) {
        new_of(p!(somf_TDequeIteratorClassData), somf_TDequeIteratorNewClass)
    } else if class_is(c, p!(somf_TSortedSequenceClassData)) {
        new_of(p!(somf_TSortedSequenceIteratorClassData), somf_TSortedSequenceIteratorNewClass)
    } else if class_is(c, p!(somf_TSequenceClassData)) {
        new_of(p!(somf_TSequenceIteratorClassData), somf_TSequenceIteratorNewClass)
    } else if class_is(c, p!(somf_TDictionaryClassData)) {
        new_of(p!(somf_TDictionaryIteratorClassData), somf_TDictionaryIteratorNewClass)
    } else if class_is(c, p!(somf_TPriorityQueueClassData)) {
        new_of(p!(somf_TPriorityQueueIteratorClassData), somf_TPriorityQueueIteratorNewClass)
    } else if class_is(c, p!(somf_TSetClassData)) {
        new_of(p!(somf_TSetIteratorClassData), somf_TSetIteratorNewClass)
    } else {
        new_of(p!(somf_TSequenceIteratorClassData), somf_TSequenceIteratorNewClass)
    };
    *iter(it) = Iter { coll: c, pos: 0, hash: false };
    it
}

// ---- sequences and deques ----

meths! {
    fn ts_init(this: usize, ev: usize, f: usize) -> usize {
        coll(this).test = f;
        this
    }
    fn ts_first(this: usize, ev: usize) -> usize { coll(this).items.first().copied().unwrap_or(0) }
    fn ts_last(this: usize, ev: usize) -> usize { coll(this).items.last().copied().unwrap_or(0) }
    fn ts_after(this: usize, ev: usize, obj: usize) -> usize {
        let c = coll(this);
        find(c, ev, obj).and_then(|i| c.items.get(i + 1).copied()).unwrap_or(0)
    }
    fn ts_before(this: usize, ev: usize, obj: usize) -> usize {
        let c = coll(this);
        find(c, ev, obj).and_then(|i| if i > 0 { c.items.get(i - 1).copied() } else { None }).unwrap_or(0)
    }
    fn ts_occurrences(this: usize, ev: usize, obj: usize) -> usize {
        let c = coll(this);
        let t = c.test;
        c.items.iter().filter(|&&x| test(t, ev, obj, x)).count()
    }
    fn tq_init_f(this: usize, ev: usize, f: usize) -> usize {
        coll(this).test = f;
        this
    }
    fn tq_init_d(this: usize, ev: usize, other: usize) -> usize {
        if other != 0 {
            *coll(this) = coll(other).clone();
        }
        this
    }
    fn tq_assign(this: usize, ev: usize, other: usize) -> usize {
        if other != 0 && other != this {
            *coll(this) = coll(other).clone();
        }
        0
    }
    fn tq_add_after(this: usize, ev: usize, existing: usize, obj: usize) -> usize {
        let c = coll(this);
        let at = find(c, ev, existing).map_or(c.items.len(), |i| i + 1);
        c.items.insert(at, obj);
        0
    }
    fn tq_add_before(this: usize, ev: usize, existing: usize, obj: usize) -> usize {
        let c = coll(this);
        let at = find(c, ev, existing).unwrap_or(0);
        c.items.insert(at, obj);
        0
    }
    fn tq_add_last(this: usize, ev: usize, obj: usize) -> usize {
        coll(this).items.push(obj);
        0
    }
    fn tq_add_first(this: usize, ev: usize, obj: usize) -> usize {
        coll(this).items.insert(0, obj);
        0
    }
    fn tq_remove_last(this: usize, ev: usize) -> usize { coll(this).items.pop().unwrap_or(0) }
    fn tq_remove_first(this: usize, ev: usize) -> usize {
        let c = coll(this);
        if c.items.is_empty() { 0 } else { c.items.remove(0) }
    }
    fn tq_create_seq_iterator(this: usize, ev: usize) -> usize { make_iterator(this) }
    fn tq_create_new_link(this: usize, ev: usize, pv: usize, n: usize, v: usize) -> usize {
        let l = new_of(p!(somf_TDequeLinkableClassData), somf_TDequeLinkableNewClass);
        tdl_init_ddm(l, ev, pv, n, v);
        l
    }
}

// ---- sets ----

fn set_of(items: Vec<usize>, t: usize) -> Coll {
    Coll { items, test: t, ..Coll::default() }
}

fn has(c: &Coll, ev: usize, x: usize) -> bool {
    find(c, ev, x).is_some()
}

/// `op`: 0 difference, 1 intersection, 2 union, 3 exclusive or.
fn set_op(this: usize, ev: usize, other: usize, op: u32) -> Vec<usize> {
    let a = coll(this).clone();
    let b = if other == 0 { Coll::default() } else { coll(other).clone() };
    let mut out = Vec::new();
    for &x in &a.items {
        let inb = has(&b, ev, x);
        if (op == 0 && !inb) || (op == 1 && inb) || op == 2 || (op == 3 && !inb) {
            out.push(x);
        }
    }
    if op >= 2 {
        for &x in &b.items {
            if !has(&a, ev, x) {
                out.push(x);
            }
        }
    }
    out
}

fn set_result(this: usize, out: Vec<usize>, result: usize) {
    let t = coll(this).test;
    let r = if result == 0 { this } else { result };
    *coll(r) = set_of(out, t);
}

meths! {
    fn tset_init_fl(this: usize, ev: usize, f: usize, hint: i32) -> usize {
        coll(this).test = f;
        this
    }
    fn tset_init_f(this: usize, ev: usize, f: usize) -> usize {
        coll(this).test = f;
        this
    }
    fn tset_init_lf(this: usize, ev: usize, hint: i32, f: usize) -> usize {
        coll(this).test = f;
        this
    }
    fn tset_init_l(this: usize, ev: usize, hint: i32) -> usize { this }
    fn tset_init_s(this: usize, ev: usize, other: usize) -> usize { tq_init_d(this, ev, other) }
    fn tset_assign(this: usize, ev: usize, other: usize) -> usize { tq_assign(this, ev, other) }
    fn tset_diff(this: usize, ev: usize, s: usize) -> usize {
        let o = set_op(this, ev, s, 0);
        set_result(this, o, 0);
        0
    }
    fn tset_diff2(this: usize, ev: usize, s: usize, r: usize) -> usize {
        let o = set_op(this, ev, s, 0);
        set_result(this, o, r);
        0
    }
    fn tset_inter(this: usize, ev: usize, s: usize) -> usize {
        let o = set_op(this, ev, s, 1);
        set_result(this, o, 0);
        0
    }
    fn tset_inter2(this: usize, ev: usize, s: usize, r: usize) -> usize {
        let o = set_op(this, ev, s, 1);
        set_result(this, o, r);
        0
    }
    fn tset_union(this: usize, ev: usize, s: usize) -> usize {
        let o = set_op(this, ev, s, 2);
        set_result(this, o, 0);
        0
    }
    fn tset_union2(this: usize, ev: usize, s: usize, r: usize) -> usize {
        let o = set_op(this, ev, s, 2);
        set_result(this, o, r);
        0
    }
    fn tset_xor(this: usize, ev: usize, s: usize) -> usize {
        let o = set_op(this, ev, s, 3);
        set_result(this, o, 0);
        0
    }
    fn tset_xor2(this: usize, ev: usize, s: usize, r: usize) -> usize {
        let o = set_op(this, ev, s, 3);
        set_result(this, o, r);
        0
    }
    fn tset_set_hash(this: usize, ev: usize, f: usize) -> usize {
        coll(this).hash = f;
        0
    }
    fn tset_get_hash(this: usize, ev: usize) -> usize {
        let h = coll(this).hash;
        if h == 0 { hash_token() } else { h }
    }
}

// ---- dictionaries and hash tables (keys and values) ----

fn kv_find(c: &Coll, ev: usize, key: usize) -> Option<usize> {
    find(c, ev, key)
}

fn kv_add(c: &mut Coll, ev: usize, key: usize, val: usize, replace: bool) -> usize {
    match kv_find(c, ev, key) {
        Some(i) => {
            let old = c.vals[i];
            if replace {
                c.vals[i] = val;
            }
            old
        }
        None => {
            c.items.push(key);
            c.vals.push(val);
            0
        }
    }
}

fn kv_remove(c: &mut Coll, ev: usize, key: usize) -> Option<(usize, usize)> {
    let i = kv_find(c, ev, key)?;
    Some((c.items.remove(i), c.vals.remove(i)))
}

fn free_all(v: Vec<usize>) {
    for x in v {
        if x != 0 {
            somsup::free_object(x);
        }
    }
}

meths! {
    fn td_init_fll(this: usize, ev: usize, f: usize, hint: i32, rate: i32) -> usize {
        coll(this).test = f;
        this
    }
    fn td_init_fl(this: usize, ev: usize, f: usize, hint: i32) -> usize {
        coll(this).test = f;
        this
    }
    fn td_init_f(this: usize, ev: usize, f: usize) -> usize {
        coll(this).test = f;
        this
    }
    fn td_init_llf(this: usize, ev: usize, hint: i32, rate: i32, f: usize) -> usize {
        coll(this).test = f;
        this
    }
    fn td_init_ll(this: usize, ev: usize, hint: i32, rate: i32) -> usize { this }
    fn td_init_l(this: usize, ev: usize, hint: i32) -> usize { this }
    fn td_init_d(this: usize, ev: usize, other: usize) -> usize { tq_init_d(this, ev, other) }
    fn td_assign(this: usize, ev: usize, other: usize) -> usize { tq_assign(this, ev, other) }
    fn td_delete_all_keys(this: usize, ev: usize) -> usize {
        let c = coll(this);
        let k = core::mem::take(&mut c.items);
        c.vals.clear();
        free_all(k);
        0
    }
    fn td_delete_all_values(this: usize, ev: usize) -> usize {
        let c = coll(this);
        let v = core::mem::take(&mut c.vals);
        c.items.clear();
        free_all(v);
        0
    }
    fn td_value_at(this: usize, ev: usize, key: usize) -> usize {
        let c = coll(this);
        kv_find(c, ev, key).map_or(0, |i| c.vals[i])
    }
    fn td_key_at_mf(this: usize, ev: usize, val: usize, f: usize) -> usize {
        let c = coll(this);
        c.vals.iter().position(|&v| test(f, ev, val, v)).map_or(0, |i| c.items[i])
    }
    fn td_key_at_m(this: usize, ev: usize, val: usize) -> usize { td_key_at_mf(this, ev, val, 0) }
    fn td_delete_key(this: usize, ev: usize, key: usize) -> usize {
        match kv_remove(coll(this), ev, key) {
            Some((k, v)) => {
                if k != key && k != 0 {
                    somsup::free_object(k);
                }
                v
            }
            None => 0,
        }
    }
    fn td_add_mmb(this: usize, ev: usize, key: usize, val: usize, replace: u8) -> usize {
        kv_add(coll(this), ev, key, val, replace != 0)
    }
    fn td_add_mm(this: usize, ev: usize, key: usize, val: usize) -> usize { kv_add(coll(this), ev, key, val, true) }
    fn td_new_impl(this: usize, ev: usize) -> usize {
        let h = new_of(p!(somf_THashTableClassData), somf_THashTableNewClass);
        ht(h).test = coll(this).test;
        h
    }
    fn td_new_impl_f(this: usize, ev: usize, f: usize) -> usize {
        let h = new_of(p!(somf_THashTableClassData), somf_THashTableNewClass);
        ht(h).test = f;
        h
    }
    fn td_new_impl_fl(this: usize, ev: usize, f: usize, a: i32) -> usize { td_new_impl_f(this, ev, f) }
    fn td_new_impl_fll(this: usize, ev: usize, f: usize, a: i32, b: i32) -> usize { td_new_impl_f(this, ev, f) }
    fn td_new_impl_flll(this: usize, ev: usize, f: usize, a: i32, b: i32, c: i32) -> usize { td_new_impl_f(this, ev, f) }
    fn td_copy_impl(this: usize, ev: usize) -> usize {
        let h = new_of(p!(somf_THashTableClassData), somf_THashTableNewClass);
        *ht(h) = coll(this).clone();
        h
    }
    // THashTable
    fn th_init_flll(this: usize, ev: usize, f: usize, size: i32, rate: i32, thr: i32) -> usize {
        let c = ht(this);
        c.test = f;
        c.rate = rate;
        c.threshold = thr;
        this
    }
    fn th_init_fll(this: usize, ev: usize, f: usize, size: i32, rate: i32) -> usize { th_init_flll(this, ev, f, size, rate, 0) }
    fn th_init_fl(this: usize, ev: usize, f: usize, size: i32) -> usize { th_init_flll(this, ev, f, size, 0, 0) }
    fn th_init_h(this: usize, ev: usize, other: usize) -> usize {
        if other != 0 {
            *ht(this) = ht(other).clone();
        }
        this
    }
    fn th_assign(this: usize, ev: usize, other: usize) -> usize {
        if other != 0 && other != this {
            *ht(this) = ht(other).clone();
        }
        0
    }
    fn th_count(this: usize, ev: usize) -> usize { ht(this).items.len() }
    fn th_remove(this: usize, ev: usize, key: usize) -> usize { kv_remove(ht(this), ev, key).map_or(0, |(k, _)| k) }
    fn th_delete(this: usize, ev: usize, key: usize) -> usize {
        match kv_remove(ht(this), ev, key) {
            Some((k, v)) => {
                if k != key && k != 0 {
                    somsup::free_object(k);
                }
                v
            }
            None => 0,
        }
    }
    fn th_member(this: usize, ev: usize, key: usize) -> usize {
        let c = ht(this);
        kv_find(c, ev, key).map_or(0, |i| c.items[i])
    }
    fn th_remove_all(this: usize, ev: usize) -> usize {
        let c = ht(this);
        c.items.clear();
        c.vals.clear();
        0
    }
    fn th_delete_all(this: usize, ev: usize) -> usize {
        let c = ht(this);
        let (k, v) = (core::mem::take(&mut c.items), core::mem::take(&mut c.vals));
        free_all(k);
        free_all(v);
        0
    }
    fn th_delete_all_keys(this: usize, ev: usize) -> usize {
        let c = ht(this);
        let k = core::mem::take(&mut c.items);
        c.vals.clear();
        free_all(k);
        0
    }
    fn th_delete_all_values(this: usize, ev: usize) -> usize {
        let c = ht(this);
        let v = core::mem::take(&mut c.vals);
        c.items.clear();
        free_all(v);
        0
    }
    fn th_add_mmb(this: usize, ev: usize, key: usize, val: usize, replace: u8) -> usize { kv_add(ht(this), ev, key, val, replace != 0) }
    fn th_add_mm(this: usize, ev: usize, key: usize, val: usize) -> usize { kv_add(ht(this), ev, key, val, true) }
    fn th_grow(this: usize, ev: usize) -> usize { 0 }
    fn th_retrieve(this: usize, ev: usize, key: usize) -> usize {
        let c = ht(this);
        kv_find(c, ev, key).map_or(0, |i| c.vals[i])
    }
    fn th_set_rate(this: usize, ev: usize, r: i32) -> usize {
        ht(this).rate = r;
        0
    }
    fn th_set_threshold(this: usize, ev: usize, t: i32) -> usize {
        ht(this).threshold = t;
        0
    }
    fn th_get_rate(this: usize, ev: usize) -> usize { ht(this).rate as usize }
    fn th_get_threshold(this: usize, ev: usize) -> usize { ht(this).threshold as usize }
    fn th_set_hash(this: usize, ev: usize, f: usize) -> usize {
        ht(this).hash = f;
        0
    }
    fn th_get_hash(this: usize, ev: usize) -> usize {
        let h = ht(this).hash;
        if h == 0 { hash_token() } else { h }
    }
    fn th_find(this: usize, ev: usize, key: usize) -> usize { th_member(this, ev, key) }
    fn th_uninit(this: usize) -> usize {
        drop_state::<Coll>(p!(somf_THashTableCClassData), this);
        som_uninit_parent(p!(somf_THashTableCClassData), this);
        0
    }
}

// ---- priority queues and sorted sequences ----

meths! {
    fn pq_init_f(this: usize, ev: usize, f: usize) -> usize {
        coll(this).order = f;
        this
    }
    fn pq_init_p(this: usize, ev: usize, other: usize) -> usize { tq_init_d(this, ev, other) }
    fn pq_assign(this: usize, ev: usize, other: usize) -> usize { tq_assign(this, ev, other) }
    fn pq_insert(this: usize, ev: usize, obj: usize) -> usize {
        add_ordered(this, ev, obj);
        0
    }
    fn pq_pop(this: usize, ev: usize) -> usize { coll(this).items.pop().unwrap_or(0) }
    fn pq_peek(this: usize, ev: usize) -> usize { coll(this).items.last().copied().unwrap_or(0) }
    fn pq_replace(this: usize, ev: usize, obj: usize) -> usize {
        let top = coll(this).items.pop().unwrap_or(0);
        add_ordered(this, ev, obj);
        top
    }
    fn pq_change(this: usize, ev: usize, obj: usize) -> usize {
        // the object's priority changed: put it in its place again
        let c = coll(this);
        if let Some(i) = c.items.iter().position(|&x| x == obj) {
            c.items.remove(i);
            add_ordered(this, ev, obj);
        }
        0
    }
    fn pq_set_eq(this: usize, ev: usize, f: usize) -> usize {
        coll(this).test = f;
        0
    }
    fn pq_get_eq(this: usize, ev: usize) -> usize { tc_test_function(this, ev) }
    fn ss_init_f(this: usize, ev: usize, f: usize) -> usize {
        coll(this).order = f;
        this
    }
    fn ss_init_s(this: usize, ev: usize, other: usize) -> usize { tq_init_d(this, ev, other) }
    fn ss_assign(this: usize, ev: usize, other: usize) -> usize { tq_assign(this, ev, other) }
    fn ss_get_seq(this: usize, ev: usize) -> usize {
        let o = coll(this).order;
        if o == 0 { compare_token() } else { o }
    }
    fn ss_set_seq(this: usize, ev: usize, f: usize) -> usize {
        coll(this).order = f;
        let items = core::mem::take(&mut coll(this).items);
        for x in items {
            add_ordered(this, ev, x);
        }
        0
    }
    fn ss_create_node(this: usize, ev: usize, n1: usize, obj: usize, n2: usize) -> usize {
        let n = new_of(p!(somf_TSortedSequenceNodeClassData), somf_TSortedSequenceNodeNewClass);
        *node(n) = Node { left: n1, right: n2, key: obj, ..Node::default() };
        n
    }
}

#[derive(Default)]
struct Node {
    left: usize,
    right: usize,
    parent: usize,
    key: usize,
    red: bool,
}

fn node(obj: usize) -> &'static mut Node {
    state::<Node>(p!(somf_TSortedSequenceNodeCClassData), obj)
}

meths! {
    fn sn_init_tmt(this: usize, ev: usize, n1: usize, obj: usize, n2: usize) -> usize {
        *node(this) = Node { left: n1, right: n2, key: obj, ..Node::default() };
        this
    }
    fn sn_init_tm(this: usize, ev: usize, n1: usize, obj: usize) -> usize { sn_init_tmt(this, ev, n1, obj, 0) }
    fn sn_init_t(this: usize, ev: usize, n1: usize) -> usize { sn_init_tmt(this, ev, n1, 0, 0) }
    fn sn_left(this: usize, ev: usize) -> usize { node(this).left }
    fn sn_right(this: usize, ev: usize) -> usize { node(this).right }
    fn sn_parent(this: usize, ev: usize) -> usize { node(this).parent }
    fn sn_key(this: usize, ev: usize) -> usize { node(this).key }
    fn sn_red(this: usize, ev: usize) -> usize { node(this).red as usize }
    fn sn_set_parent(this: usize, ev: usize, n: usize) -> usize {
        node(this).parent = n;
        0
    }
    fn sn_set_left(this: usize, ev: usize, n: usize) -> usize {
        node(this).left = n;
        0
    }
    fn sn_set_right(this: usize, ev: usize, n: usize) -> usize {
        node(this).right = n;
        0
    }
    fn sn_set_key(this: usize, ev: usize, k: usize) -> usize {
        node(this).key = k;
        0
    }
    fn sn_set_red(this: usize, ev: usize, on: u8) -> usize {
        node(this).red = on != 0;
        0
    }
    fn sn_set_red_on(this: usize, ev: usize) -> usize {
        node(this).red = true;
        0
    }
    fn sn_uninit(this: usize) -> usize {
        drop_state::<Node>(p!(somf_TSortedSequenceNodeCClassData), this);
        som_uninit_parent(p!(somf_TSortedSequenceNodeCClassData), this);
        0
    }
}

// ---- iterators ----

#[derive(Default)]
struct Iter {
    coll: usize,
    /// The index of the next item.
    pos: usize,
    /// Over a THashTable (its keys).
    hash: bool,
}

fn iter(obj: usize) -> &'static mut Iter {
    state::<Iter>(p!(somf_TIteratorCClassData), obj)
}

fn iter_items(it: &Iter) -> &'static mut Vec<usize> {
    if it.hash { &mut ht(it.coll).items } else { &mut coll(it.coll).items }
}

meths! {
    fn ti_next(this: usize, ev: usize) -> usize {
        let it = iter(this);
        if it.coll == 0 {
            return 0;
        }
        let items = iter_items(it);
        match items.get(it.pos) {
            Some(&x) => {
                it.pos += 1;
                x
            }
            None => 0,
        }
    }
    fn ti_first(this: usize, ev: usize) -> usize {
        iter(this).pos = 0;
        ti_next(this, ev)
    }
    fn ti_remove(this: usize, ev: usize) -> usize {
        let it = iter(this);
        if it.coll != 0 && it.pos > 0 {
            let i = it.pos - 1;
            if it.hash {
                let c = ht(it.coll);
                if i < c.items.len() {
                    c.items.remove(i);
                    c.vals.remove(i);
                }
            } else {
                let c = coll(it.coll);
                if i < c.items.len() {
                    c.items.remove(i);
                    if i < c.vals.len() {
                        c.vals.remove(i);
                    }
                }
            }
            it.pos = i;
        }
        0
    }
    fn tsi_last(this: usize, ev: usize) -> usize {
        let it = iter(this);
        if it.coll == 0 {
            return 0;
        }
        let items = iter_items(it);
        it.pos = items.len();
        tsi_prev(this, ev)
    }
    fn tsi_prev(this: usize, ev: usize) -> usize {
        // the item before the one last returned
        let it = iter(this);
        if it.coll == 0 || it.pos < 2 {
            it.pos = 0;
            return 0;
        }
        it.pos -= 1;
        iter_items(it).get(it.pos - 1).copied().unwrap_or(0)
    }
    fn it_init(this: usize, ev: usize, c: usize) -> usize {
        *iter(this) = Iter { coll: c, pos: 0, hash: false };
        this
    }
    fn it_init_hash(this: usize, ev: usize, c: usize) -> usize {
        *iter(this) = Iter { coll: c, pos: 0, hash: true };
        this
    }
    fn ssi_start_here(this: usize, ev: usize, obj: usize) -> usize {
        let it = iter(this);
        if it.coll == 0 {
            return 0;
        }
        let c = coll(it.coll);
        match find(c, ev, obj) {
            Some(i) => {
                it.pos = i + 1;
                c.items[i]
            }
            None => 0,
        }
    }
    fn ti_uninit(this: usize) -> usize {
        drop_state::<Iter>(p!(somf_TIteratorCClassData), this);
        som_uninit_parent(p!(somf_TIteratorCClassData), this);
        0
    }
}

// ---- the classes ----

type M = extern "C" fn();

fn ms(v: &[(&'static str, M)]) -> Vec<(&'static str, usize)> {
    v.iter().map(|&(n, f)| (n, f as usize)).collect()
}

fn build(name: &str, parent: &str, data: u32, methods: &[(&'static str, M)], overrides: &[(&'static str, M)], cd: *mut usize, ccd: *mut usize) -> usize {
    let m = ms(methods);
    let o = ms(overrides);
    build_class(&ClassSpec {
        name,
        parents: &[parent],
        meta: None,
        major: 2,
        minor: 1,
        data,
        align: 4,
        methods: &m,
        overrides: &o,
        cd,
        ccd,
    })
}

macro_rules! new_class {
    ($fn:ident, $name:literal, $parent:literal, $pre:expr, $data:expr, $cd:ident, $ccd:ident, [$($m:literal => $f:ident),* $(,)?], [$($o:literal => $g:ident),* $(,)?]) => {
        pub extern "C" fn $fn(_major: i32, _minor: i32) -> usize {
            unsafe {
                if $cd[0] != 0 {
                    return $cd[0];
                }
            }
            $pre;
            build($name, $parent, $data, &[$(($m, $f::t)),*], &[$(($o, $g::t)),*], p!($cd), p!($ccd))
        }
    };
}

fn nothing() {}

new_class!(mcollectible_new, "somf_MCollectible", "SOMObject", nothing(), 0, somf_MCollectibleClassData, somf_MCollectibleCClassData,
    ["somfClone" => mc_clone, "somfClonePointer" => mc_clone_pointer, "somfHash" => mc_hash, "somfIsEqual" => mc_is_equal,
     "somfIsSame" => mc_is_same, "somfIsNotEqual" => mc_is_not_equal], []);
new_class!(morderable_new, "somf_MOrderableCollectible", "somf_MCollectible", mcollectible_new(2, 1), 0,
    somf_MOrderableCollectibleClassData, somf_MOrderableCollectibleCClassData,
    ["somfClone" => mc_clone, "somfClonePointer" => mc_clone_pointer, "somfHash" => mc_hash, "somfIsEqual" => mo_is_equal,
     "somfIsSame" => mc_is_same, "somfIsNotEqual" => mc_is_not_equal, "somfIsGreaterThan" => mo_gt, "somfIsLessThan" => mo_lt,
     "somfCompare" => mo_compare, "somfIsGreaterThanOrEqualTo" => mo_ge, "somfIsLessThanOrEqualTo" => mo_le], []);
new_class!(mlinkable_new, "somf_MLinkable", "SOMObject", nothing(), 4, somf_MLinkableClassData, somf_MLinkableCClassData,
    ["somfGetNext" => ml_next, "somfSetNext" => ml_set_next, "somfGetPrevious" => ml_prev, "somfSetPrevious" => ml_set_prev,
     "somfMLinkableInit" => ml_init], ["SOMObject::somUninit" => ml_uninit]);
new_class!(tassoc_new, "somf_TAssoc", "somf_MCollectible", mcollectible_new(2, 1), 4, somf_TAssocClassData, somf_TAssocCClassData,
    ["somfGetKey" => ta_key, "somfGetValue" => ta_value, "somfSetKey" => ta_set_key, "somfSetValue" => ta_set_value,
     "somfTAssocInitMM" => ta_init_mm, "somfTAssocInitM" => ta_init_m],
    ["somf_MCollectible::somfIsEqual" => ta_is_equal, "somf_MCollectible::somfHash" => ta_hash, "SOMObject::somUninit" => ta_uninit]);
new_class!(tclong_new, "somf_TCollectibleLong", "somf_MCollectible", mcollectible_new(2, 1), 4,
    somf_TCollectibleLongClassData, somf_TCollectibleLongCClassData,
    ["somfGetValue" => tcl_get, "somfSetValue" => tcl_set, "somfTCollectibleLongInit" => tcl_init],
    ["somf_MCollectible::somfIsEqual" => tcl_is_equal, "somf_MCollectible::somfHash" => tcl_hash,
     "somf_MCollectible::somfClone" => tcl_clone, "SOMObject::somUninit" => tcl_uninit]);
new_class!(tcollection_new, "somf_TCollection", "somf_MCollectible", mcollectible_new(2, 1), 4,
    somf_TCollectionClassData, somf_TCollectionCClassData,
    ["somfAdd" => tc_add, "somfAddAll" => tc_add_all, "somfRemove" => tc_remove, "somfRemoveAll" => tc_remove_all,
     "somfDeleteAll" => tc_delete_all, "somfCount" => tc_count, "somfMember" => tc_member, "somfCreateIterator" => tc_create_iterator,
     "somfTestFunction" => tc_test_function, "somfSetTestFunction" => tc_set_test_function, "somfTCollectionInit" => tc_init],
    ["somf_MCollectible::somfIsEqual" => tc_is_equal, "SOMObject::somUninit" => tc_uninit]);
new_class!(titerator_new, "somf_TIterator", "SOMObject", nothing(), 4, somf_TIteratorClassData, somf_TIteratorCClassData,
    ["somfNext" => ti_next, "somfFirst" => ti_first, "somfRemove" => ti_remove], ["SOMObject::somUninit" => ti_uninit]);
new_class!(tsequence_new, "somf_TSequence", "somf_TCollection", tcollection_new(2, 1), 0, somf_TSequenceClassData, somf_TSequenceCClassData,
    ["somfFirst" => ts_first, "somfAfter" => ts_after, "somfBefore" => ts_before, "somfLast" => ts_last,
     "somfOccurrencesOf" => ts_occurrences, "somfTSequenceInit" => ts_init], []);
new_class!(tseqiter_new, "somf_TSequenceIterator", "somf_TIterator", titerator_new(2, 1), 0,
    somf_TSequenceIteratorClassData, somf_TSequenceIteratorCClassData, ["somfLast" => tsi_last, "somfPrevious" => tsi_prev], []);
new_class!(tdeque_new, "somf_TDeque", "somf_TSequence", tsequence_new(2, 1), 0, somf_TDequeClassData, somf_TDequeCClassData,
    ["somfAddAfter" => tq_add_after, "somfAddBefore" => tq_add_before, "somfAddLast" => tq_add_last, "somfAddFirst" => tq_add_first,
     "somfRemoveLast" => tq_remove_last, "somfRemoveFirst" => tq_remove_first, "somfCreateSequenceIterator" => tq_create_seq_iterator,
     "somfRemoveQ" => tq_remove_first, "somfInsert" => tq_add_last, "somfPop" => tq_remove_last, "somfPush" => tq_add_last,
     "somfCreateNewLink" => tq_create_new_link, "somfAssign" => tq_assign, "somfTDequeInitF" => tq_init_f, "somfTDequeInitD" => tq_init_d], []);
new_class!(tdequeiter_new, "somf_TDequeIterator", "somf_TSequenceIterator", tseqiter_new(2, 1), 0,
    somf_TDequeIteratorClassData, somf_TDequeIteratorCClassData, ["somfTDequeIteratorInit" => it_init], []);
new_class!(tdequelink_new, "somf_TDequeLinkable", "somf_MLinkable", mlinkable_new(2, 1), 4,
    somf_TDequeLinkableClassData, somf_TDequeLinkableCClassData,
    ["somfGetValue" => tdl_get, "somfSetValue" => tdl_set, "somfTDequeLinkableInitDDM" => tdl_init_ddm, "somfTDequeLinkableInitDD" => tdl_init_dd],
    ["SOMObject::somUninit" => tdl_uninit]);
new_class!(tdict_new, "somf_TDictionary", "somf_TCollection", tcollection_new(2, 1), 0, somf_TDictionaryClassData, somf_TDictionaryCClassData,
    ["somfDeleteAllKeys" => td_delete_all_keys, "somfDeleteAllValues" => td_delete_all_values, "somfValueAt" => td_value_at,
     "somfKeyAtMF" => td_key_at_mf, "somfKeyAtM" => td_key_at_m, "somfDeleteKey" => td_delete_key,
     "somfAddKeyValuePairMMB" => td_add_mmb, "somfAddKeyValuePairMM" => td_add_mm, "somfSetHashFunction" => tset_set_hash,
     "somfGetHashFunction" => tset_get_hash, "somfCreateNewImplementationFLLL" => td_new_impl_flll,
     "somfCreateNewImplementationF" => td_new_impl_f, "somfCreateNewImplementationFL" => td_new_impl_fl,
     "somfCreateNewImplementationFLL" => td_new_impl_fll, "somfCopyImplementation" => td_copy_impl, "somfAssign" => td_assign,
     "somfTDictionaryInitFLL" => td_init_fll, "somfTDictionaryInitFL" => td_init_fl, "somfTDictionaryInitF" => td_init_f,
     "somfTDictionaryInitLLF" => td_init_llf, "somfTDictionaryInitLL" => td_init_ll, "somfTDictionaryInitL" => td_init_l,
     "somfTDictionaryInitD" => td_init_d], []);
new_class!(tdictiter_new, "somf_TDictionaryIterator", "somf_TIterator", titerator_new(2, 1), 0,
    somf_TDictionaryIteratorClassData, somf_TDictionaryIteratorCClassData, ["somfTDictionaryIteratorInit" => it_init], []);
new_class!(thash_new, "somf_THashTable", "somf_MCollectible", mcollectible_new(2, 1), 4, somf_THashTableClassData, somf_THashTableCClassData,
    ["somfCount" => th_count, "somfRemove" => th_remove, "somfDelete" => th_delete, "somfMember" => th_member,
     "somfRemoveAll" => th_remove_all, "somfDeleteAll" => th_delete_all, "somfDeleteAllKeys" => th_delete_all_keys,
     "somfDeleteAllValues" => th_delete_all_values, "somfAddMMB" => th_add_mmb, "somfAddMM" => th_add_mm, "somfGrow" => th_grow,
     "somfRetrieve" => th_retrieve, "somfSetGrowthRate" => th_set_rate, "somfSetRehashThreshold" => th_set_threshold,
     "somfGetGrowthRate" => th_get_rate, "somfGetRehashThreshold" => th_get_threshold, "somfSetHashFunction" => th_set_hash,
     "somfGetHashFunction" => th_get_hash, "somfFind" => th_find, "somfMakeNewTable" => th_grow, "somfCopyIn" => th_grow,
     "somfAssign" => th_assign, "somfTHashTableInitFLLL" => th_init_flll, "somfTHashTableInitFLL" => th_init_fll,
     "somfTHashTableInitFL" => th_init_fl, "somfTHashTableInitH" => th_init_h], ["SOMObject::somUninit" => th_uninit]);
new_class!(thashiter_new, "somf_THashTableIterator", "somf_TIterator", titerator_new(2, 1), 0,
    somf_THashTableIteratorClassData, somf_THashTableIteratorCClassData, ["somfTHashTableIteratorInit" => it_init_hash], []);
new_class!(tpll_new, "somf_TPrimitiveLinkedList", "SOMObject", nothing(), 4,
    somf_TPrimitiveLinkedListClassData, somf_TPrimitiveLinkedListCClassData,
    ["somfCount" => pl_count, "somfRemove" => pl_remove, "somfRemoveAll" => pl_remove_all, "somfRemoveFirst" => pl_remove_first,
     "somfRemoveLast" => pl_remove_last, "somfAddBefore" => pl_add_before, "somfAddAfter" => pl_add_after, "somfAddFirst" => pl_add_first,
     "somfAddLast" => pl_add_last, "somfAfter" => pl_after, "somfBefore" => pl_before, "somfFirst" => pl_first, "somfLast" => pl_last],
    ["SOMObject::somUninit" => pl_uninit]);
new_class!(tplliter_new, "somf_TPrimitiveLinkedListIterator", "SOMObject", nothing(), 4,
    somf_TPrimitiveLinkedListIteratorClassData, somf_TPrimitiveLinkedListIteratorCClassData,
    ["somfFirst" => pli_first, "somfNext" => pli_next, "somfLast" => pli_last, "somfPrevious" => pli_prev,
     "somfTPrimitiveLinkedListIteratorInit" => pli_init], ["SOMObject::somUninit" => pli_uninit]);
new_class!(tpq_new, "somf_TPriorityQueue", "somf_TCollection", tcollection_new(2, 1), 0,
    somf_TPriorityQueueClassData, somf_TPriorityQueueCClassData,
    ["somfInsert" => pq_insert, "somfPop" => pq_pop, "somfPeek" => pq_peek, "somfReplace" => pq_replace, "somfChange" => pq_change,
     "somfSetEqualityComparisonFunction" => pq_set_eq, "somfGetEqualityComparisonFunction" => pq_get_eq, "somfDownHeap" => th_grow,
     "somfUpHeap" => th_grow, "somfGrowHeap" => th_grow, "somfMakeNewTable" => th_grow, "somfCopyIn" => th_grow,
     "somfAssign" => pq_assign, "somfTPriorityQueueInitF" => pq_init_f, "somfTPriorityQueueInitP" => pq_init_p], []);
new_class!(tpqiter_new, "somf_TPriorityQueueIterator", "somf_TIterator", titerator_new(2, 1), 0,
    somf_TPriorityQueueIteratorClassData, somf_TPriorityQueueIteratorCClassData, ["somfTPriorityQueueIteratorInit" => it_init], []);
new_class!(tset_new, "somf_TSet", "somf_TCollection", tcollection_new(2, 1), 0, somf_TSetClassData, somf_TSetCClassData,
    ["somfDifferenceS" => tset_diff, "somfDifferenceSS" => tset_diff2, "somfIntersectionS" => tset_inter,
     "somfIntersectionSS" => tset_inter2, "somfUnionS" => tset_union, "somfUnionSS" => tset_union2, "somfXorS" => tset_xor,
     "somfXorSS" => tset_xor2, "somfSetHashFunction" => tset_set_hash, "somfGetHashFunction" => tset_get_hash,
     "somfRehash" => th_grow, "somfFind" => tc_member, "somfMakeNewTable" => th_grow, "somfCopyIn" => th_grow, "somfGrow" => th_grow,
     "somfAssign" => tset_assign, "somfTSetInitFL" => tset_init_fl, "somfTSetInitF" => tset_init_f, "somfTSetInitLF" => tset_init_lf,
     "somfTSetInitL" => tset_init_l, "somfTSetInitS" => tset_init_s], []);
new_class!(tsetiter_new, "somf_TSetIterator", "somf_TIterator", titerator_new(2, 1), 0,
    somf_TSetIteratorClassData, somf_TSetIteratorCClassData, ["somfTSetIteratorInit" => it_init], []);
new_class!(tss_new, "somf_TSortedSequence", "somf_TSequence", tsequence_new(2, 1), 0,
    somf_TSortedSequenceClassData, somf_TSortedSequenceCClassData,
    ["somfCreateSequenceIterator" => tq_create_seq_iterator, "somfGetSequencingFunction" => ss_get_seq,
     "somfSetSequencingFunction" => ss_set_seq, "somfCreateSortedSequenceNode" => ss_create_node, "somfRotate" => th_grow,
     "somfSplit" => th_grow, "somfBalance" => th_grow, "somfInternalRemoveAll" => tc_remove_all,
     "somfInternalDeleteAll" => tc_delete_all, "somfAssign" => ss_assign, "somfTSortedSequenceInitF" => ss_init_f,
     "somfTSortedSequenceInitS" => ss_init_s], []);
new_class!(tssiter_new, "somf_TSortedSequenceIterator", "somf_TSequenceIterator", tseqiter_new(2, 1), 0,
    somf_TSortedSequenceIteratorClassData, somf_TSortedSequenceIteratorCClassData,
    ["somfStartHere" => ssi_start_here, "somfTSortedSequenceIteratorInit" => it_init], []);
new_class!(tssnode_new, "somf_TSortedSequenceNode", "SOMObject", nothing(), 4,
    somf_TSortedSequenceNodeClassData, somf_TSortedSequenceNodeCClassData,
    ["somfGetLeftChild" => sn_left, "somfGetRightChild" => sn_right, "somfGetParent" => sn_parent, "somfGetKey" => sn_key,
     "somfGetRed" => sn_red, "somfSetParent" => sn_set_parent, "somfSetLeftChild" => sn_set_left, "somfSetRightChild" => sn_set_right,
     "somfSetKey" => sn_set_key, "somfSetRed" => sn_set_red, "somfSetRedOn" => sn_set_red_on,
     "somfTSortedSequenceNodeInitTMT" => sn_init_tmt, "somfTSortedSequenceNodeInitTM" => sn_init_tm,
     "somfTSortedSequenceNodeInitT" => sn_init_t], ["SOMObject::somUninit" => sn_uninit]);

// The exported <Class>NewClass entries.
pub extern "C" fn somf_MCollectibleNewClass(major: i32, minor: i32) -> usize {
    mcollectible_new(major, minor)
}
pub extern "C" fn somf_MOrderableCollectibleNewClass(major: i32, minor: i32) -> usize {
    morderable_new(major, minor)
}
pub extern "C" fn somf_MLinkableNewClass(major: i32, minor: i32) -> usize {
    mlinkable_new(major, minor)
}
pub extern "C" fn somf_TAssocNewClass(major: i32, minor: i32) -> usize {
    tassoc_new(major, minor)
}
pub extern "C" fn somf_TCollectibleLongNewClass(major: i32, minor: i32) -> usize {
    tclong_new(major, minor)
}
pub extern "C" fn somf_TCollectionNewClass(major: i32, minor: i32) -> usize {
    tcollection_new(major, minor)
}
pub extern "C" fn somf_TIteratorNewClass(major: i32, minor: i32) -> usize {
    titerator_new(major, minor)
}
pub extern "C" fn somf_TSequenceNewClass(major: i32, minor: i32) -> usize {
    tsequence_new(major, minor)
}
pub extern "C" fn somf_TSequenceIteratorNewClass(major: i32, minor: i32) -> usize {
    tseqiter_new(major, minor)
}
pub extern "C" fn somf_TDequeNewClass(major: i32, minor: i32) -> usize {
    tdeque_new(major, minor)
}
pub extern "C" fn somf_TDequeIteratorNewClass(major: i32, minor: i32) -> usize {
    tdequeiter_new(major, minor)
}
pub extern "C" fn somf_TDequeLinkableNewClass(major: i32, minor: i32) -> usize {
    tdequelink_new(major, minor)
}
pub extern "C" fn somf_TDictionaryNewClass(major: i32, minor: i32) -> usize {
    tdict_new(major, minor)
}
pub extern "C" fn somf_TDictionaryIteratorNewClass(major: i32, minor: i32) -> usize {
    tdictiter_new(major, minor)
}
pub extern "C" fn somf_THashTableNewClass(major: i32, minor: i32) -> usize {
    thash_new(major, minor)
}
pub extern "C" fn somf_THashTableIteratorNewClass(major: i32, minor: i32) -> usize {
    thashiter_new(major, minor)
}
pub extern "C" fn somf_TPrimitiveLinkedListNewClass(major: i32, minor: i32) -> usize {
    tpll_new(major, minor)
}
pub extern "C" fn somf_TPrimitiveLinkedListIteratorNewClass(major: i32, minor: i32) -> usize {
    tplliter_new(major, minor)
}
pub extern "C" fn somf_TPriorityQueueNewClass(major: i32, minor: i32) -> usize {
    tpq_new(major, minor)
}
pub extern "C" fn somf_TPriorityQueueIteratorNewClass(major: i32, minor: i32) -> usize {
    tpqiter_new(major, minor)
}
pub extern "C" fn somf_TSetNewClass(major: i32, minor: i32) -> usize {
    tset_new(major, minor)
}
pub extern "C" fn somf_TSetIteratorNewClass(major: i32, minor: i32) -> usize {
    tsetiter_new(major, minor)
}
pub extern "C" fn somf_TSortedSequenceNewClass(major: i32, minor: i32) -> usize {
    tss_new(major, minor)
}
pub extern "C" fn somf_TSortedSequenceIteratorNewClass(major: i32, minor: i32) -> usize {
    tssiter_new(major, minor)
}
pub extern "C" fn somf_TSortedSequenceNodeNewClass(major: i32, minor: i32) -> usize {
    tssnode_new(major, minor)
}
aligned! {
    __a_somf_MCollectibleNewClass => somf_MCollectibleNewClass, 2;
    __a_somf_MOrderableCollectibleNewClass => somf_MOrderableCollectibleNewClass, 2;
    __a_somf_MLinkableNewClass => somf_MLinkableNewClass, 2;
    __a_somf_TAssocNewClass => somf_TAssocNewClass, 2;
    __a_somf_TCollectibleLongNewClass => somf_TCollectibleLongNewClass, 2;
    __a_somf_TCollectionNewClass => somf_TCollectionNewClass, 2;
    __a_somf_TIteratorNewClass => somf_TIteratorNewClass, 2;
    __a_somf_TSequenceNewClass => somf_TSequenceNewClass, 2;
    __a_somf_TSequenceIteratorNewClass => somf_TSequenceIteratorNewClass, 2;
    __a_somf_TDequeNewClass => somf_TDequeNewClass, 2;
    __a_somf_TDequeIteratorNewClass => somf_TDequeIteratorNewClass, 2;
    __a_somf_TDequeLinkableNewClass => somf_TDequeLinkableNewClass, 2;
    __a_somf_TDictionaryNewClass => somf_TDictionaryNewClass, 2;
    __a_somf_TDictionaryIteratorNewClass => somf_TDictionaryIteratorNewClass, 2;
    __a_somf_THashTableNewClass => somf_THashTableNewClass, 2;
    __a_somf_THashTableIteratorNewClass => somf_THashTableIteratorNewClass, 2;
    __a_somf_TPrimitiveLinkedListNewClass => somf_TPrimitiveLinkedListNewClass, 2;
    __a_somf_TPrimitiveLinkedListIteratorNewClass => somf_TPrimitiveLinkedListIteratorNewClass, 2;
    __a_somf_TPriorityQueueNewClass => somf_TPriorityQueueNewClass, 2;
    __a_somf_TPriorityQueueIteratorNewClass => somf_TPriorityQueueIteratorNewClass, 2;
    __a_somf_TSetNewClass => somf_TSetNewClass, 2;
    __a_somf_TSetIteratorNewClass => somf_TSetIteratorNewClass, 2;
    __a_somf_TSortedSequenceNewClass => somf_TSortedSequenceNewClass, 2;
    __a_somf_TSortedSequenceIteratorNewClass => somf_TSortedSequenceIteratorNewClass, 2;
    __a_somf_TSortedSequenceNodeNewClass => somf_TSortedSequenceNodeNewClass, 2;
}

/// SOMInitModule: every class.
pub extern "C" fn SOMInitModule(_major: i32, _minor: i32, _class: usize) -> usize {
    for f in [
        mcollectible_new, morderable_new, mlinkable_new, tassoc_new, tclong_new, tcollection_new, titerator_new, tsequence_new,
        tseqiter_new, tdeque_new, tdequeiter_new, tdequelink_new, tdict_new, tdictiter_new, thash_new, thashiter_new, tpll_new,
        tplliter_new, tpq_new, tpqiter_new, tset_new, tsetiter_new, tss_new, tssiter_new, tssnode_new,
    ] {
        f(2, 1);
    }
    0
}

aligned! { __a_SOMInitModule => SOMInitModule, 3; }

#[allow(dead_code)]
fn unused() {
    let _ = som_init_parent;
}
