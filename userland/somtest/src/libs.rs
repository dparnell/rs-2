//! Checks of SOM's class libraries: SOMTC.DLL (TypeCodes), SOMU.DLL (the
//! utility classes and metaclasses) and SOMIR.DLL (the interface
//! repository). They are loaded at run time and called by name, so the
//! same checks run against IBM's DLLs (from the current directory, LIBPATH
//! starting with `.`) and rs2's; a library that does not load is skipped.

use alloc::vec::Vec;

use os2::*;

use crate::{check, check_eq, cstr};

pub fn load(name: &[u8]) -> u32 {
    let mut fail = [0u8; 64];
    let mut h = 0;
    let n = crate::z(name);
    if unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, n.as_ptr(), &mut h) } != 0 {
        return 0;
    }
    let mut path = [0u8; 260];
    unsafe { DosQueryModuleName(h, path.len() as u32, path.as_mut_ptr()) };
    println!("{} is {}", core::str::from_utf8(name).unwrap_or("?"), core::str::from_utf8(cstr(path.as_ptr())).unwrap_or("?"));
    h
}

pub fn proc_addr(h: u32, name: &[u8]) -> usize {
    let n = crate::z(name);
    let mut p = 0usize;
    if unsafe { DosQueryProcAddr(h, 0, n.as_ptr(), &mut p) } != 0 {
        return 0;
    }
    p
}

#[repr(C)]
pub struct Env {
    pub major: u32,
    pub name: *const u8,
    pub params: usize,
    pub anchor: usize,
}

impl Env {
    pub fn new() -> Env {
        Env { major: 0, name: core::ptr::null(), params: 0, anchor: 0 }
    }
}

#[repr(C)]
struct Any {
    tc: usize,
    value: *const u8,
}

#[repr(C)]
struct Seq {
    max: u32,
    len: u32,
    buf: *const usize,
}

const TK_SHORT: u32 = 3;
const TK_LONG: u32 = 4;
const TK_STRUCT: u32 = 16;
const TK_UNION: u32 = 17;
const TK_ENUM: u32 = 18;
const TK_STRING: u32 = 19;
const TK_SEQUENCE: u32 = 20;
const TK_ARRAY: u32 = 21;

struct Tc {
    h: u32,
}

impl Tc {
    fn f(&self, name: &[u8]) -> usize {
        let p = proc_addr(self.h, name);
        if p == 0 {
            println!("FAIL SOMTC has no {}", core::str::from_utf8(name).unwrap_or("?"));
        }
        p
    }
    fn kind(&self, t: usize) -> u32 {
        let f: extern "C" fn(usize, *mut Env) -> u32 = unsafe { core::mem::transmute(self.f(b"tcKind")) };
        f(t, core::ptr::null_mut())
    }
    fn size(&self, t: usize) -> i32 {
        let f: extern "C" fn(usize, *mut Env) -> i32 = unsafe { core::mem::transmute(self.f(b"tcSize")) };
        f(t, &mut Env::new())
    }
    fn align(&self, t: usize) -> i32 {
        let f: extern "C" fn(usize, *mut Env) -> i32 = unsafe { core::mem::transmute(self.f(b"tcAlignment")) };
        f(t, &mut Env::new()) as i16 as i32
    }
    fn count(&self, t: usize) -> i32 {
        let f: extern "C" fn(usize, *mut Env) -> i32 = unsafe { core::mem::transmute(self.f(b"tcParmCount")) };
        f(t, &mut Env::new())
    }
    fn param(&self, t: usize, i: i32, ev: &mut Env) -> Any {
        let f: extern "C" fn(*mut Any, usize, *mut Env, i32) -> *mut Any = unsafe { core::mem::transmute(self.f(b"tcParameter")) };
        let mut a = Any { tc: 0, value: core::ptr::null() };
        f(&mut a, t, ev, i);
        a
    }
    fn equal(&self, a: usize, b: usize) -> bool {
        let f: extern "C" fn(usize, *mut Env, usize) -> u8 = unsafe { core::mem::transmute(self.f(b"tcEqual")) };
        f(a, &mut Env::new(), b) != 0
    }
    fn copy(&self, t: usize) -> usize {
        let f: extern "C" fn(usize, *mut Env) -> usize = unsafe { core::mem::transmute(self.f(b"tcCopy")) };
        f(t, &mut Env::new())
    }
    fn free(&self, t: usize) {
        let f: extern "C" fn(usize, *mut Env) = unsafe { core::mem::transmute(self.f(b"tcFree")) };
        f(t, &mut Env::new())
    }
    fn set_align(&self, t: usize, a: i16) {
        let f: extern "C" fn(usize, *mut Env, i32) = unsafe { core::mem::transmute(self.f(b"tcSetAlignment")) };
        f(t, &mut Env::new(), a as i32)
    }
    fn new(&self) -> unsafe extern "C" fn(u32, ...) -> usize {
        unsafe { core::mem::transmute(self.f(b"tcNew")) }
    }
    fn constant(&self, name: &[u8]) -> usize {
        self.f(name)
    }
}

pub fn somtc() {
    let h = load(b"SOMTC");
    if h == 0 {
        println!("SKIP SOMTC.DLL did not load");
        return;
    }
    let tc = Tc { h };
    let long = tc.constant(b"TC__long");
    let short = tc.constant(b"TC__short");
    let double = tc.constant(b"TC__double");
    let chr = tc.constant(b"TC__char");
    let string = tc.constant(b"TC__string");
    let typecode = tc.constant(b"TC__TypeCode");
    check_eq("tcKind TC__long", tc.kind(long) as i64, TK_LONG as i64);
    check_eq("tcKind NULL (tk_null)", tc.kind(0) as i64, 1);
    check_eq("tcSize TC__double", tc.size(double) as i64, 8);
    check_eq("tcSize TC__any", tc.size(tc.constant(b"TC__any")) as i64, 8);
    check_eq("tcKind TC__Object", tc.kind(tc.constant(b"TC__Object")) as i64, 15);
    let od = tc.constant(b"TC__OperationDescription");
    check_eq("TC__OperationDescription members", tc.count(od) as i64, 17);
    check_eq("tcSize TC__OperationDescription", tc.size(od) as i64, 56);
    unsafe {
        let new = tc.new();
        let st = new(TK_STRUCT, b"Pt\0".as_ptr(), b"x\0".as_ptr(), long, b"y\0".as_ptr(), short, b"z\0".as_ptr(), double, 0usize);
        check("tcNew tk_struct", st != 0 && tc.kind(st) == TK_STRUCT);
        check_eq("struct size", tc.size(st) as i64, 16);
        check_eq("struct alignment", tc.align(st) as i64, 4);
        check_eq("struct parameter count", tc.count(st) as i64, 7);
        let mut ev = Env::new();
        let a = tc.param(st, 0, &mut ev);
        check("struct parameter 0 (name)", a.tc == string && cstr(*(a.value as *const *const u8)) == b"Pt");
        let a = tc.param(st, 3, &mut ev);
        check("struct parameter 3 (member y)", a.tc == string && cstr(*(a.value as *const *const u8)) == b"y");
        let a = tc.param(st, 4, &mut ev);
        check("struct parameter 4 (y's type)", a.tc == typecode && *(a.value as *const usize) == short);
        check("no exception", ev.major == 0);
        let a = tc.param(st, 7, &mut ev);
        check("struct parameter 7: Bounds", ev.major == 1 && cstr(ev.name) == b"Bounds" && tc.kind(a.tc) == 1 && a.value.is_null());

        let c = tc.copy(st);
        check("tcCopy equal", c != 0 && c != st && tc.equal(c, st));
        tc.set_align(c, 8);
        check_eq("tcSetAlignment 8: alignment", tc.align(c) as i64, 8);
        check_eq("tcSetAlignment 8: size", tc.size(c) as i64, 24);
        check("aligned copy differs", !tc.equal(c, st));
        tc.set_align(long, 2);
        check_eq("constants keep their alignment", tc.align(long) as i64, 4);
        tc.free(c);

        let en = new(TK_ENUM, b"Color\0".as_ptr(), b"red\0".as_ptr(), b"green\0".as_ptr(), 0usize);
        check_eq("enum parameter count", tc.count(en) as i64, 3);
        check_eq("enum size", tc.size(en) as i64, 4);
        let a = tc.param(en, 2, &mut ev);
        check("enum parameter 2", cstr(*(a.value as *const *const u8)) == b"green");
        check("struct is not enum", !tc.equal(st, en));

        let sq = new(TK_SEQUENCE, long, 10u32);
        check_eq("sequence size", tc.size(sq) as i64, 12);
        let a = tc.param(sq, 1, &mut ev);
        check("sequence bound", *(a.value as *const u32) == 10);
        let ar = new(TK_ARRAY, short, 5u32);
        check_eq("array size", tc.size(ar) as i64, 10);
        let s = new(TK_STRING, 32u32);
        check("string", tc.kind(s) == TK_STRING && tc.size(s) == 4);
        let un = new(TK_UNION, b"U\0".as_ptr(), long, 1u32, 7u32, b"a\0".as_ptr(), double, 2u32, 0u32, b"b\0".as_ptr(), chr, 0u32);
        check("tcNew tk_union", un != 0 && tc.kind(un) == TK_UNION);
        check_eq("union size", tc.size(un) as i64, 12);
        check_eq("union parameter count", tc.count(un) as i64, 8);
        let a = tc.param(un, 2, &mut ev);
        check("union label", a.tc == long && *(a.value as *const u32) == 7);
        let bad = new(TK_UNION, b"V\0".as_ptr(), double, 1u32, 7u32, b"a\0".as_ptr(), long, 0u32);
        check("union switched on double refused", bad == 0);
        let ty = |t: usize| tc.kind(t);
        check("kinds", ty(st) == TK_STRUCT && ty(sq) == TK_SEQUENCE && ty(ar) == TK_ARRAY && TK_SHORT == 3);
        for t in [st, en, sq, ar, s, un] {
            tc.free(t);
        }

        let sfl: extern "C" fn(*mut Seq, *const u8) -> *mut Seq = core::mem::transmute(tc.f(b"tcSeqFromListString"));
        let mut q = Seq { max: 0, len: 0, buf: core::ptr::null() };
        sfl(&mut q, b"a,bc,cxxdecl(x,y),d\0".as_ptr());
        let items: Vec<&[u8]> = (0..q.len as usize).map(|i| cstr(*q.buf.add(i) as *const u8)).collect();
        check("tcSeqFromListString", items == [&b"a"[..], b"bc", b"cxxdecl(x,y)", b"d"]);
        let sn: extern "C" fn(usize, u32) -> *const Seq = core::mem::transmute(tc.f(b"tcSequenceNew"));
        let q = sn(long, 5);
        check("tcSequenceNew", !q.is_null() && (*q).max == 5 && (*q).len == 0 && !(*q).buf.is_null());
        let print: extern "C" fn(usize, *mut Env) = core::mem::transmute(tc.f(b"tcPrint"));
        print(od, &mut ev);
    }
}
