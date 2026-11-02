//! `SOMTEST IRW SET` and `SOMTEST IRW CHECK`: updating an interface
//! repository file (SOMIR naming a copy of SOM.IR, the last file in the
//! list being the one written). SET changes a constant's value, adds a
//! modifier to SOMObject and makes an operation oneway, then calls
//! Repository::saveIR; CHECK (another process) finds the changes in the
//! file. Run SET with one SOMIR.DLL and CHECK with the other to see that
//! IBM's and rs2's read each other's updates.

use alloc::vec::Vec;

use os2::println;

use crate::libs::{Env, load, proc_addr};
use crate::{check, cstr, m1, sc};

#[repr(C)]
struct Seq {
    max: u32,
    len: u32,
    buf: *const usize,
}

pub fn run(set: bool) {
    let h = load(b"SOMIR");
    let tch = load(b"SOMTC");
    if h == 0 || tch == 0 {
        check("SOMIR.DLL and SOMTC.DLL loaded", false);
        return;
    }
    let cdata = |n: &[u8]| -> *const usize {
        let mut name = n.to_vec();
        name.extend_from_slice(b"ClassData");
        proc_addr(h, &name) as *const usize
    };
    let tok = |cls: &[u8], i: usize| -> usize { unsafe { *cdata(cls).add(1 + i) } };
    unsafe {
        for c in [&b"Contained"[..], b"ConstantDef", b"OperationDef"] {
            let mut n = c.to_vec();
            n.extend_from_slice(b"NewClass");
            let f: extern "C" fn(i32, i32) -> usize = core::mem::transmute(proc_addr(h, &n));
            f(0, 0);
        }
        let new: extern "C" fn(i32, i32) -> usize = core::mem::transmute(proc_addr(h, b"RepositoryNewClass"));
        let repo = m1(sc(), 0, new(0, 0));
        let mut ev = Env::new();
        let e = &mut ev as *mut Env as usize;
        let lookup_id: extern "C" fn(usize, usize, *const u8) -> usize = core::mem::transmute(tok(b"Repository", 0));
        let modifier: extern "C" fn(usize, usize, *const u8, *const u8) -> *mut u8 = core::mem::transmute(tok(b"Repository", 1));
        let save: extern "C" fn(usize, usize) = core::mem::transmute(tok(b"Repository", 3));
        let get_mods: extern "C" fn(*mut Seq, usize, usize) -> *mut Seq = core::mem::transmute(tok(b"Contained", 8));
        let set_mods: extern "C" fn(usize, usize, *const Seq) = core::mem::transmute(tok(b"Contained", 9));
        let value: extern "C" fn(*mut [usize; 2], usize, usize) -> *mut [usize; 2] = core::mem::transmute(tok(b"ConstantDef", 2));
        let set_value: extern "C" fn(usize, usize, *const [usize; 2]) = core::mem::transmute(tok(b"ConstantDef", 3));
        let get_mode: extern "C" fn(usize, usize) -> u32 = core::mem::transmute(tok(b"OperationDef", 2));
        let set_mode: extern "C" fn(usize, usize, u32) = core::mem::transmute(tok(b"OperationDef", 3));

        let c = lookup_id(repo, e, b"::SOMD::SOMDVersion\0".as_ptr());
        let obj = lookup_id(repo, e, b"::SOMObject\0".as_ptr());
        let op = lookup_id(repo, e, b"::SOMObject::somGetClassName\0".as_ptr());
        check("::SOMD::SOMDVersion, ::SOMObject, its somGetClassName", c != 0 && obj != 0 && op != 0);
        if c == 0 || obj == 0 || op == 0 {
            return;
        }
        if set {
            // The value (a string constant's text, quoted).
            // (SOMTC's TC__string is the TypeCode itself.)
            let tc_string = proc_addr(tch, b"TC__string");
            let text = b"\"2.5\"\0";
            let p: *const u8 = text.as_ptr();
            let any = [tc_string, &p as *const *const u8 as usize];
            set_value(c, e, &any);
            // SOMObject's modifiers and one more.
            let mut s = Seq { max: 0, len: 0, buf: core::ptr::null() };
            get_mods(&mut s, obj, e);
            let mut pairs: Vec<usize> = core::slice::from_raw_parts(s.buf, 2 * s.len as usize).to_vec();
            pairs.push(b"rs2test\0".as_ptr() as usize);
            pairs.push(b"written\0".as_ptr() as usize);
            let n = Seq { max: (pairs.len() / 2) as u32, len: (pairs.len() / 2) as u32, buf: pairs.as_ptr() };
            set_mods(obj, e, &n);
            set_mode(op, e, 2);
            save(repo, e);
            check("saveIR: no exception", ev.major == 0);
            println!("  saved");
        } else {
            let mut a = [0usize; 2];
            value(&mut a, c, e);
            let v = if a[1] != 0 { cstr(*(a[1] as *const *const u8)) } else { b"" };
            println!("  SOMDVersion {}", core::str::from_utf8(v).unwrap_or("?"));
            check("the constant's new value", v == b"\"2.5\"");
            let m = modifier(repo, e, b"::SOMObject\0".as_ptr(), b"rs2test\0".as_ptr());
            check("SOMObject's new modifier", !m.is_null() && cstr(m) == b"written");
            let dll = modifier(repo, e, b"::SOMObject\0".as_ptr(), b"dllname\0".as_ptr());
            check("SOMObject's other modifiers kept", !dll.is_null() && cstr(dll).eq_ignore_ascii_case(b"som.dll"));
            check("somGetClassName is oneway", get_mode(op, e) == 2);
        }
    }
}
