//! Checks of SOMIR.DLL (rs2's or IBM's) against an interface repository
//! file: SOMIR in the environment naming IBM's SOM.IR (the facts below are
//! that file's). Without SOMIR set the checks are skipped.

use alloc::vec::Vec;

use os2::println;

use crate::libs::{Env, load, proc_addr};
use crate::{check, check_eq, cstr, m1, sc, so};

#[repr(C)]
#[derive(Clone, Copy)]
struct Seq {
    max: u32,
    len: u32,
    buf: *const usize,
}

#[repr(C)]
struct Description {
    name: *const u8,
    tc: usize,
    value: *const usize,
}

unsafe extern "C" {
    #[link_name = "SOM.10"]
    static SOMFree: extern "C" fn(*mut u8);
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

pub fn run() {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"SOMIR\0".as_ptr(), &mut v) } != 0 {
        println!("SKIP SOMIR is not set");
        return;
    }
    let h = load(b"SOMIR");
    if h == 0 {
        println!("SKIP SOMIR.DLL did not load");
        return;
    }
    let cdata = |n: &[u8]| -> *const usize {
        let mut name = n.to_vec();
        name.extend_from_slice(b"ClassData");
        proc_addr(h, &name) as *const usize
    };
    let tok = |cls: &[u8], i: usize| -> usize { unsafe { *cdata(cls).add(1 + i) } };
    unsafe {
        let new: extern "C" fn(i32, i32) -> usize = core::mem::transmute(proc_addr(h, b"RepositoryNewClass"));
        let repc = new(0, 0);
        check("RepositoryNewClass", repc != 0);
        let repo = m1(sc(), 0, repc);
        check("a repository", repo != 0);
        // Every class (the tokens are read below).
        for n in [&b"Contained"[..], b"Container", b"InterfaceDef", b"OperationDef", b"AttributeDef", b"TypeDef", b"ConstantDef", b"ParameterDef", b"ModuleDef", b"ExceptionDef"] {
            let mut name = n.to_vec();
            name.extend_from_slice(b"NewClass");
            let f: extern "C" fn(i32, i32) -> usize = core::mem::transmute(proc_addr(h, &name));
            f(0, 0);
        }
        let mut ev = Env::new();
        let e = &mut ev as *mut Env as usize;
        let lookup_id: extern "C" fn(usize, usize, *const u8) -> usize = core::mem::transmute(tok(b"Repository", 0));
        let modifier: extern "C" fn(usize, usize, *const u8, *const u8) -> *mut u8 = core::mem::transmute(tok(b"Repository", 1));
        let get_name: extern "C" fn(usize, usize) -> *const u8 = core::mem::transmute(tok(b"Contained", 2));
        let get_id: extern "C" fn(usize, usize) -> *const u8 = core::mem::transmute(tok(b"Contained", 4));
        let get_defined_in: extern "C" fn(usize, usize) -> *const u8 = core::mem::transmute(tok(b"Contained", 6));
        let within: extern "C" fn(*mut Seq, usize, usize) -> *mut Seq = core::mem::transmute(tok(b"Contained", 0));
        let describe: extern "C" fn(*mut Description, usize, usize) -> *mut Description = core::mem::transmute(tok(b"Contained", 1));
        let contents: extern "C" fn(*mut Seq, usize, usize, *const u8, u32) -> *mut Seq = core::mem::transmute(tok(b"Container", 0));
        let lookup_name: extern "C" fn(*mut Seq, usize, usize, *const u8, i32, *const u8, u32) -> *mut Seq =
            core::mem::transmute(tok(b"Container", 1));
        let kind: extern "C" fn(usize, usize) -> u32 = core::mem::transmute(proc_addr(load(b"SOMTC"), b"tcKind"));

        let obj = lookup_id(repo, e, b"::SOMObject\0".as_ptr());
        check("lookup_id ::SOMObject", obj != 0);
        if obj == 0 {
            return;
        }
        check("its class is InterfaceDef", cstr(m1(so(0), 4, obj) as *const u8) == b"InterfaceDef");
        check("_get_name, _get_id", cstr(get_name(obj, e)) == b"SOMObject" && cstr(get_id(obj, e)) == b"::SOMObject");
        let dll = modifier(repo, e, b"::SOMObject\0".as_ptr(), b"dllname\0".as_ptr());
        check("lookup_modifier dllname", cstr(dll) == b"som.dll");
        let maj = modifier(repo, e, b"SOMClass\0".as_ptr(), b"majorversion\0".as_ptr());
        check("lookup_modifier SOMClass majorversion", cstr(maj) == b"1");
        check("lookup_modifier: no such modifier", modifier(repo, e, b"::SOMObject\0".as_ptr(), b"nothing\0".as_ptr()).is_null());

        let op = lookup_id(repo, e, b"::SOMObject::somGetClassName\0".as_ptr());
        check("lookup_id ::SOMObject::somGetClassName", op != 0 && cstr(m1(so(0), 4, op) as *const u8) == b"OperationDef");
        check("_get_defined_in", cstr(get_defined_in(op, e)) == b"::SOMObject");
        let result: extern "C" fn(usize, usize) -> usize = core::mem::transmute(tok(b"OperationDef", 0));
        check_eq("somGetClassName's result (tk_string)", kind(result(op, e), 0) as i64, 19);
        let mut d = Description { name: core::ptr::null(), tc: 0, value: core::ptr::null() };
        describe(&mut d, op, e);
        check("describe: OperationDescription", cstr(d.name) == b"OperationDescription" && !d.value.is_null());
        if !d.value.is_null() {
            let s = d.value;
            check(
                "  its name, id, defined_in, mode",
                cstr(*s as *const u8) == b"somGetClassName" && cstr(*s.add(1) as *const u8) == b"::SOMObject::somGetClassName" && cstr(*s.add(2) as *const u8) == b"::SOMObject" && *s.add(4) == 1,
            );
            check_eq("  its result's kind", kind(*s.add(3), 0) as i64, 19);
            (SOMFree)(d.value as *mut u8);
        }
        let mut w = Seq { max: 0, len: 0, buf: core::ptr::null() };
        within(&mut w, op, e);
        check("within: SOMObject and its subclasses", w.len >= 1 && *w.buf == obj);

        let mut s = Seq { max: 0, len: 0, buf: core::ptr::null() };
        contents(&mut s, obj, e, b"OperationDef\0".as_ptr(), 1);
        check_eq("SOMObject's operations", s.len as i64, 30);
        let mut s = Seq { max: 0, len: 0, buf: core::ptr::null() };
        contents(&mut s, repo, e, b"InterfaceDef\0".as_ptr(), 1);
        check_eq("interfaces at the top level", s.len as i64, 123);
        let mut s = Seq { max: 0, len: 0, buf: core::ptr::null() };
        lookup_name(&mut s, obj, e, b"somInit\0".as_ptr(), -1, b"all\0".as_ptr(), 0);
        check("lookup_name somInit", s.len == 1 && cstr(get_id(*s.buf, e)) == b"::SOMObject::somInit");

        let cls = lookup_id(repo, e, b"::SOMClass\0".as_ptr());
        let bases: extern "C" fn(*mut Seq, usize, usize) -> *mut Seq = core::mem::transmute(tok(b"InterfaceDef", 1));
        let mut b = Seq { max: 0, len: 0, buf: core::ptr::null() };
        bases(&mut b, cls, e);
        check("SOMClass's base interfaces", b.len == 1 && cstr(*b.buf as *const u8) == b"::SOMObject");
        let mut s = Seq { max: 0, len: 0, buf: core::ptr::null() };
        lookup_name(&mut s, cls, e, b"somGetClassName\0".as_ptr(), 1, b"OperationDef\0".as_ptr(), 0);
        check("lookup_name finds inherited operations", s.len == 1 && *s.buf == op);

        let full: extern "C" fn(*mut [usize; 9], usize, usize) -> *mut [usize; 9] = core::mem::transmute(tok(b"InterfaceDef", 0));
        let mut f = [0usize; 9];
        full(&mut f, obj, e);
        check("describe_interface", cstr(f[0] as *const u8) == b"SOMObject" && f[4] == 30 && f[7] == 0);

        let attr = lookup_id(repo, e, b"::Contained::name\0".as_ptr());
        let atype: extern "C" fn(usize, usize) -> usize = core::mem::transmute(tok(b"AttributeDef", 0));
        let amode: extern "C" fn(usize, usize) -> u32 = core::mem::transmute(tok(b"AttributeDef", 2));
        check("AttributeDef ::Contained::name", attr != 0 && kind(atype(attr, e), 0) == 19 && amode(attr, e) == 1);

        let td = lookup_id(repo, e, b"::somId\0".as_ptr());
        let ttype: extern "C" fn(usize, usize) -> usize = core::mem::transmute(tok(b"TypeDef", 0));
        check("TypeDef ::somId (tk_foreign)", td != 0 && kind(ttype(td, e), 0) == 103);

        let c = lookup_id(repo, e, b"::MAXTHREADS\0".as_ptr());
        let value: extern "C" fn(*mut [usize; 2], usize, usize) -> *mut [usize; 2] = core::mem::transmute(tok(b"ConstantDef", 2));
        let mut a = [0usize; 2];
        if c != 0 {
            value(&mut a, c, e);
        }
        check("ConstantDef ::MAXTHREADS = 256", c != 0 && a[1] != 0 && cstr(*(a[1] as *const *const u8)) == b"256");
        check("no exception raised", ev.major == 0);
    }
}
