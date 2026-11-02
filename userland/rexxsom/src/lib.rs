//! REXXSOM.DLL (OREXXSOM.DLL until SWITCHRX.CMD installs Object REXX):
//! SOM classes from Object REXX, rs2's own.
//!
//! - `.SOM~import('Class')` gives a SOM class (the class manager finds it,
//!   loading its DLL; SOMIR's classes after the interface repository is
//!   made); `::CLASS name EXTERNAL 'SOM Class'` the same.
//! - Messages to SOM objects call the method of that name (any case)
//!   through rxsom: SOM's, the interface repository's and the DLF
//!   classes' methods have signatures there.
//! - The DLF classes (DLFLong, DLFULong, DLFShort, DLFUShort, DLFFloat,
//!   DLFDouble, DLFChar, DLFOctet, DLFBoolean, DLFString, DLFObject) are
//!   SOM classes holding one value (`_get_value`, `_set_value`): passed
//!   for an INOUT or OUT argument, the method gets the value's address.
//!
//! OREXX.DLL calls the bridge entries rs2_ext_lookup, rs2_ext_class,
//! rs2_ext_send and rs2_ext_free (see userland/rexx/src/oohost.rs).

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, static_mut_refs, function_casts_as_integer)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;

use rxsom::{Error, Val};
use somsup::{ClassSpec, aligned, build_class, data, methods};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, term: u32) -> u32 {
    if term == 0 {
        unsafe {
            if somsup::SOMClassMgrObject == 0 {
                somsup::somEnvironmentNew();
            }
        }
        make_classes();
    }
    1
}

// ---- the DLF classes ----

macro_rules! class_data {
    ($($cd:ident, $ccd:ident;)*) => {$(
        #[unsafe(no_mangle)]
        pub static mut $cd: [usize; 3] = [0; 3];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

class_data! {
    DLFBasicTypeClassData, DLFBasicTypeCClassData;
    DLFLongClassData, DLFLongCClassData;
    DLFULongClassData, DLFULongCClassData;
    DLFShortClassData, DLFShortCClassData;
    DLFUShortClassData, DLFUShortCClassData;
    DLFFloatClassData, DLFFloatCClassData;
    DLFDoubleClassData, DLFDoubleCClassData;
    DLFCharClassData, DLFCharCClassData;
    DLFOctetClassData, DLFOctetCClassData;
    DLFBooleanClassData, DLFBooleanCClassData;
    DLFStringClassData, DLFStringCClassData;
    DLFObjectClassData, DLFObjectCClassData;
}

macro_rules! p {
    ($a:ident) => {
        (&raw mut $a) as *mut usize
    };
}

/// A DLF object's value (8 bytes).
fn val(ccd: *mut usize, obj: usize) -> &'static mut u64 {
    data::<u64>(ccd, obj)
}

macro_rules! dlf_methods {
    ($($ccd:ident, $get:ident, $set:ident, $t:ty;)*) => {$(
        extern "C" fn $get(obj: usize) -> $t {
            let v = *val(p!($ccd), obj);
            unsafe { core::mem::transmute_copy::<u64, $t>(&v) }
        }
        extern "C" fn $set(obj: usize, x: $t) {
            let mut v = 0u64;
            unsafe { core::ptr::copy_nonoverlapping(&x as *const $t as *const u8, &mut v as *mut u64 as *mut u8, core::mem::size_of::<$t>()) };
            *val(p!($ccd), obj) = v;
        }
    )*};
}

dlf_methods! {
    DLFLongCClassData, long_get, long_set, i32;
    DLFULongCClassData, ulong_get, ulong_set, u32;
    DLFShortCClassData, short_get, short_set, i16;
    DLFUShortCClassData, ushort_get, ushort_set, u16;
    DLFFloatCClassData, float_get, float_set, f32;
    DLFDoubleCClassData, double_get, double_set, f64;
    DLFCharCClassData, char_get, char_set, u8;
    DLFOctetCClassData, octet_get, octet_set, u8;
    DLFBooleanCClassData, bool_get, bool_set, u8;
    DLFObjectCClassData, object_get, object_set, usize;
}

extern "C" fn string_get(obj: usize) -> *const u8 {
    *val(p!(DLFStringCClassData), obj) as usize as *const u8
}

extern "C" fn string_set(obj: usize, s: *const u8) {
    let v = val(p!(DLFStringCClassData), obj);
    let old = *v as usize as *mut u8;
    *v = if s.is_null() { 0 } else { somsup::dup(unsafe { somsup::cstr(s) }) as usize as u64 };
    if !old.is_null() {
        somsup::free(old);
    }
}

methods! {
    __m_long_get => long_get, 1;
    __m_long_set => long_set, 2;
    __m_ulong_get => ulong_get, 1;
    __m_ulong_set => ulong_set, 2;
    __m_short_get => short_get, 1;
    __m_short_set => short_set, 2;
    __m_ushort_get => ushort_get, 1;
    __m_ushort_set => ushort_set, 2;
    __m_float_get => float_get, 1;
    __m_float_set => float_set, 2;
    __m_double_get => double_get, 1;
    __m_double_set => double_set, 3;
    __m_char_get => char_get, 1;
    __m_char_set => char_set, 2;
    __m_octet_get => octet_get, 1;
    __m_octet_set => octet_set, 2;
    __m_bool_get => bool_get, 1;
    __m_bool_set => bool_set, 2;
    __m_object_get => object_get, 1;
    __m_object_set => object_set, 2;
    __m_string_get => string_get, 1;
    __m_string_set => string_set, 2;
}

fn base() {
    unsafe {
        somsup::SOMObjectNewClass(1, 4);
        somsup::SOMClassNewClass(1, 4);
    }
}

pub extern "C" fn DLFBasicTypeNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "DLFBasicType",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 0,
        align: 0,
        methods: &[],
        overrides: &[],
        cd: p!(DLFBasicTypeClassData),
        ccd: p!(DLFBasicTypeCClassData),
    })
}

macro_rules! dlf_class {
    ($($newclass:ident, $name:literal, $cd:ident, $ccd:ident, $get:path, $set:path;)*) => {$(
        pub extern "C" fn $newclass(major: i32, minor: i32) -> usize {
            DLFBasicTypeNewClass(1, 1);
            build_class(&ClassSpec {
                name: $name,
                parents: &["DLFBasicType"],
                meta: None,
                major,
                minor,
                data: 8,
                align: 4,
                methods: &[("_get_value", $get as usize), ("_set_value", $set as usize)],
                overrides: &[],
                cd: p!($cd),
                ccd: p!($ccd),
            })
        }
    )*};
}

dlf_class! {
    DLFLongNewClass, "DLFLong", DLFLongClassData, DLFLongCClassData, __m_long_get, __m_long_set;
    DLFULongNewClass, "DLFULong", DLFULongClassData, DLFULongCClassData, __m_ulong_get, __m_ulong_set;
    DLFShortNewClass, "DLFShort", DLFShortClassData, DLFShortCClassData, __m_short_get, __m_short_set;
    DLFUShortNewClass, "DLFUShort", DLFUShortClassData, DLFUShortCClassData, __m_ushort_get, __m_ushort_set;
    DLFFloatNewClass, "DLFFloat", DLFFloatClassData, DLFFloatCClassData, __m_float_get, __m_float_set;
    DLFDoubleNewClass, "DLFDouble", DLFDoubleClassData, DLFDoubleCClassData, __m_double_get, __m_double_set;
    DLFCharNewClass, "DLFChar", DLFCharClassData, DLFCharCClassData, __m_char_get, __m_char_set;
    DLFOctetNewClass, "DLFOctet", DLFOctetClassData, DLFOctetCClassData, __m_octet_get, __m_octet_set;
    DLFBooleanNewClass, "DLFBoolean", DLFBooleanClassData, DLFBooleanCClassData, __m_bool_get, __m_bool_set;
    DLFStringNewClass, "DLFString", DLFStringClassData, DLFStringCClassData, __m_string_get, __m_string_set;
    DLFObjectNewClass, "DLFObject", DLFObjectClassData, DLFObjectCClassData, __m_object_get, __m_object_set;
}

fn make_classes() {
    DLFLongNewClass(1, 1);
    DLFULongNewClass(1, 1);
    DLFShortNewClass(1, 1);
    DLFUShortNewClass(1, 1);
    DLFFloatNewClass(1, 1);
    DLFDoubleNewClass(1, 1);
    DLFCharNewClass(1, 1);
    DLFOctetNewClass(1, 1);
    DLFBooleanNewClass(1, 1);
    DLFStringNewClass(1, 1);
    DLFObjectNewClass(1, 1);
}

/// SOMInitModule: the class manager loading this DLL for a class.
pub extern "C" fn SOMInitModule(_major: i32, _minor: i32, _class: *const u8) {
    make_classes();
}

aligned! {
    __a_SOMInitModule => SOMInitModule, 3;
    __a_DLFBasicTypeNewClass => DLFBasicTypeNewClass, 2;
    __a_DLFLongNewClass => DLFLongNewClass, 2;
    __a_DLFULongNewClass => DLFULongNewClass, 2;
    __a_DLFShortNewClass => DLFShortNewClass, 2;
    __a_DLFUShortNewClass => DLFUShortNewClass, 2;
    __a_DLFFloatNewClass => DLFFloatNewClass, 2;
    __a_DLFDoubleNewClass => DLFDoubleNewClass, 2;
    __a_DLFCharNewClass => DLFCharNewClass, 2;
    __a_DLFOctetNewClass => DLFOctetNewClass, 2;
    __a_DLFBooleanNewClass => DLFBooleanNewClass, 2;
    __a_DLFStringNewClass => DLFStringNewClass, 2;
    __a_DLFObjectNewClass => DLFObjectNewClass, 2;
}

/// The DLF classes' value signatures.
const DLF_SIGS: &[(&str, &str)] = &[
    ("DLFLong::_get_value", "l()"),
    ("DLFLong::_set_value", "v(l)"),
    ("DLFULong::_get_value", "L()"),
    ("DLFULong::_set_value", "v(L)"),
    ("DLFShort::_get_value", "h()"),
    ("DLFShort::_set_value", "v(h)"),
    ("DLFUShort::_get_value", "H()"),
    ("DLFUShort::_set_value", "v(H)"),
    ("DLFFloat::_get_value", "f()"),
    ("DLFFloat::_set_value", "v(f)"),
    ("DLFDouble::_get_value", "d()"),
    ("DLFDouble::_set_value", "v(d)"),
    ("DLFChar::_get_value", "c()"),
    ("DLFChar::_set_value", "v(c)"),
    ("DLFOctet::_get_value", "H()"),
    ("DLFOctet::_set_value", "v(H)"),
    ("DLFBoolean::_get_value", "b()"),
    ("DLFBoolean::_set_value", "v(b)"),
    ("DLFString::_get_value", "s()"),
    ("DLFString::_set_value", "v(s)"),
    ("DLFObject::_get_value", "o()"),
    ("DLFObject::_set_value", "v(o)"),
];

/// The address of a DLF object's value (for INOUT and OUT arguments).
fn dlf_addr(obj: usize) -> Option<usize> {
    let ccds: [*mut usize; 11] = [
        p!(DLFLongCClassData),
        p!(DLFULongCClassData),
        p!(DLFShortCClassData),
        p!(DLFUShortCClassData),
        p!(DLFFloatCClassData),
        p!(DLFDoubleCClassData),
        p!(DLFCharCClassData),
        p!(DLFOctetCClassData),
        p!(DLFBooleanCClassData),
        p!(DLFStringCClassData),
        p!(DLFObjectCClassData),
    ];
    let cds: [*mut usize; 11] = [
        p!(DLFLongClassData),
        p!(DLFULongClassData),
        p!(DLFShortClassData),
        p!(DLFUShortClassData),
        p!(DLFFloatClassData),
        p!(DLFDoubleClassData),
        p!(DLFCharClassData),
        p!(DLFOctetClassData),
        p!(DLFBooleanClassData),
        p!(DLFStringClassData),
        p!(DLFObjectClassData),
    ];
    let cls = rxsom::class_of(obj);
    for (cd, ccd) in cds.iter().zip(ccds) {
        let c = unsafe { **cd };
        if c != 0 && c == cls {
            return Some(val(ccd, obj) as *mut u64 as usize);
        }
    }
    None
}

// ---- the bridge to OREXX.DLL ----

/// `.SOM`: the object that imports classes.
const SOM_ROOT: u32 = 1;

#[repr(C)]
pub struct ExtVal {
    pub kind: u32,
    pub len: u32,
    pub ptr: *mut u8,
    pub handle: u32,
}

fn put_str(dst: *mut u8, cb: u32, s: &[u8]) {
    if dst.is_null() || cb == 0 {
        return;
    }
    let n = s.len().min(cb as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, n);
        *dst.add(n) = 0;
    }
}

/// A class by name: the class manager's, or (for the interface
/// repository's classes) after the repository is made.
fn import(name: &[u8]) -> usize {
    let c = rxsom::find_class(name);
    if c != 0 {
        return c;
    }
    let mgr = unsafe { somsup::SOMClassMgrObject };
    somsup::call(somsup::scm(), somsup::mm::SOM_GET_INTERFACE_REPOSITORY, &[mgr]);
    somsup::call(somsup::scm(), somsup::mm::SOM_CLASS_FROM_ID, &[mgr, rxsom::id(name) as usize])
}

extern "C" fn ext_lookup(name: *const u8, h: *mut u32, is_class: *mut u32, id: *mut u8, cbid: u32) -> u32 {
    let n = unsafe { somsup::cstr(name) };
    if n != b"SOM" {
        return 0;
    }
    unsafe {
        *h = SOM_ROOT;
        *is_class = 0;
    }
    put_str(id, cbid, b"SOM");
    1
}

extern "C" fn ext_class(spec: *const u8, h: *mut u32, id: *mut u8, cbid: u32) -> u32 {
    let n = unsafe { somsup::cstr(spec) };
    let name: Vec<u8> = n.iter().copied().take_while(|&c| c != b' ').collect();
    let c = import(&name);
    if c == 0 {
        return 0;
    }
    unsafe { *h = c as u32 };
    put_str(id, cbid, &rxsom::class_id(c));
    1
}

fn ret_val(r: *mut ExtVal, v: Val) {
    let (kind, s, h) = match v {
        Val::None => (0, Vec::new(), 0),
        Val::Nil => (5, Vec::new(), 0),
        Val::Str(s) => (1, s, 0),
        Val::Obj(o) => (2, rxsom::class_name_of(o), o),
        Val::Class(c) => (3, rxsom::class_id(c), c),
    };
    let p = if s.is_empty() && kind != 1 { core::ptr::null_mut() } else { somsup::dup(&s) };
    unsafe {
        (*r).kind = kind;
        (*r).len = s.len() as u32;
        (*r).ptr = p;
        (*r).handle = h as u32;
    }
}

extern "C" fn ext_send(h: u32, msg: *const u8, argc: u32, args: *const ExtVal, res: *mut ExtVal) -> u32 {
    let m = unsafe { somsup::cstr(msg) };
    let mut av = Vec::new();
    for i in 0..argc as usize {
        let a = unsafe { &*args.add(i) };
        let s = if a.ptr.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(a.ptr, a.len as usize).to_vec() } };
        av.push(match a.kind {
            1 => Val::Str(s),
            2 => Val::Obj(a.handle as usize),
            3 => Val::Class(a.handle as usize),
            _ => Val::None,
        });
    }
    let r: Result<Val, Error> = if h == SOM_ROOT {
        match m {
            b"IMPORT" => match av.first() {
                Some(Val::Str(n)) => {
                    let c = import(n);
                    if c == 0 {
                        let mut t = b"There is no SOM class \"".to_vec();
                        t.extend_from_slice(n);
                        t.push(b'"');
                        Err(Error::Rexx(98912, t))
                    } else {
                        Ok(Val::Class(c))
                    }
                }
                _ => Err(Error::Rexx(93903, b"The method needs argument 1".to_vec())),
            },
            _ => Err(Error::NoMethod),
        }
    } else if m == b"NEW" && somsup::is_a(h as usize, unsafe { *somsup::sc() }) {
        let o = somsup::new_object(h as usize);
        if o == 0 { Ok(Val::Nil) } else { Ok(Val::Obj(o)) }
    } else {
        rxsom::send(h as usize, m, &av, &[DLF_SIGS, rxsom::SOM_SIGS], &dlf_addr)
    };
    match r {
        Ok(v) => {
            ret_val(res, v);
            0
        }
        Err(Error::NoMethod) => 1,
        Err(Error::Rexx(code, text)) => {
            unsafe {
                (*res).kind = 1;
                (*res).len = text.len() as u32;
                (*res).ptr = somsup::dup(&text);
                (*res).handle = code;
            }
            2
        }
    }
}

extern "C" fn ext_free(p: *mut u8) {
    if !p.is_null() {
        somsup::free(p);
    }
}

aligned! {
    rs2_ext_lookup => ext_lookup, 5;
    rs2_ext_class => ext_class, 4;
    rs2_ext_send => ext_send, 5;
    rs2_ext_free => ext_free, 1;
}
