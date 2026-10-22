//! Self-checks of the Workplace Shell classes in PMWP.DLL, run with the
//! shell's objects in this process (Rs2WpsStart). Each check prints PASS
//! or FAIL; the last line is "wpstest done: N failure(s)" and the exit
//! code N.
//!
//! `WPSTEST` makes objects (a folder on the desktop, a program, a data
//! file, a shadow), changes, moves and copies them, and checks what was
//! stored. `WPSTEST CHECK`, run as a new process afterwards, wakes them
//! from OS2.INI and their extended attributes, checks them and deletes
//! them. `WPSTEST CLASS dll class` registers a class DLL (one of the
//! Toolkit's samples, built for IBM's PMWP) and makes an object of it.

#![no_std]
#![no_main]
#![allow(non_snake_case, static_mut_refs, function_casts_as_integer)]

extern crate alloc;

use alloc::vec::Vec;

use os2::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWP.9100"]
    fn Rs2WpsStart() -> u32;
    #[link_name = "PMWP.281"]
    fn WinCreateObject(class: *const u8, title: *const u8, setup: *const u8, loc: *const u8, flags: u32) -> u32;
    #[link_name = "PMWP.252"]
    fn WinQueryObject(id: *const u8) -> u32;
    #[link_name = "PMWP.250"]
    fn WinSetObjectData(h: u32, setup: *const u8) -> u32;
    #[link_name = "PMWP.251"]
    fn WinDestroyObject(h: u32) -> u32;
    #[link_name = "PMWP.287"]
    fn WinMoveObject(h: u32, dest: u32, flags: u32) -> u32;
    #[link_name = "PMWP.288"]
    fn WinCopyObject(h: u32, dest: u32, flags: u32) -> u32;
    #[link_name = "PMWP.289"]
    fn WinCreateShadow(h: u32, dest: u32, flags: u32) -> u32;
    #[link_name = "PMWP.263"]
    fn WinQueryObjectPath(h: u32, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "PMWP.200"]
    fn WinRegisterObjectClass(class: *const u8, module: *const u8) -> u32;
    #[link_name = "PMWP.201"]
    fn WinDeregisterObjectClass(class: *const u8) -> u32;
    #[link_name = "PMWP.205"]
    fn WinEnumObjectClasses(buf: *mut u8, size: *mut u32) -> u32;
    #[link_name = "PMWP.24"]
    fn WPObjectNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "PMWP.23"]
    static WPObjectClassData: [usize; 2];
    #[link_name = "PMWP.323"]
    static M_WPObjectClassData: [usize; 2];
    #[link_name = "SOM.61"]
    fn somResolveByName(obj: usize, name: *const u8) -> usize;
    #[link_name = "SOM.82"]
    fn somGetClassFromMToken(token: usize) -> usize;
    #[link_name = "PMSHAPI.117"]
    fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
}

const HINI_USER: u32 = 0xffff_ffff;
const CO_REPLACEIFEXISTS: u32 = 1;

static mut FAILS: u32 = 0;

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {}", name);
    } else {
        println!("FAIL {}", name);
        unsafe { FAILS += 1 };
    }
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }.to_vec()
}

fn s(b: &[u8]) -> &str {
    core::str::from_utf8(b).unwrap_or("?")
}

fn create(class: &[u8], title: &[u8], setup: &[u8], loc: &[u8]) -> u32 {
    let (c, t, st, l) = (z(class), z(title), z(setup), z(loc));
    unsafe { WinCreateObject(c.as_ptr(), t.as_ptr(), st.as_ptr(), l.as_ptr(), CO_REPLACEIFEXISTS) }
}

fn query(id: &[u8]) -> u32 {
    let i = z(id);
    unsafe { WinQueryObject(i.as_ptr()) }
}

/// The object a handle stands for (M_WPObject::wpclsObjectFromHandle on
/// WPObject's class).
fn object(h: u32) -> usize {
    let cls = unsafe { WPObjectClassData[0] };
    let p = unsafe { somResolveByName(cls, b"wpclsObjectFromHandle\0".as_ptr()) };
    if p == 0 {
        return 0;
    }
    let f: extern "C" fn(usize, u32) -> usize = unsafe { core::mem::transmute(p) };
    f(cls, h)
}

/// Call a method by name.
fn call(obj: usize, name: &[u8], args: &[usize]) -> usize {
    let n = z(name);
    let p = unsafe { somResolveByName(obj, n.as_ptr()) };
    if p == 0 {
        println!("  (no method {})", s(name));
        return 0;
    }
    unsafe {
        match args.len() {
            0 => core::mem::transmute::<usize, extern "C" fn(usize) -> usize>(p)(obj),
            1 => core::mem::transmute::<usize, extern "C" fn(usize, usize) -> usize>(p)(obj, args[0]),
            2 => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize) -> usize>(p)(obj, args[0], args[1]),
            _ => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize, usize) -> usize>(p)(obj, args[0], args[1], args[2]),
        }
    }
}

fn title(obj: usize) -> Vec<u8> {
    cstr(call(obj, b"wpQueryTitle", &[]) as *const u8)
}

fn path(h: u32) -> Vec<u8> {
    let mut b = [0u8; 260];
    if unsafe { WinQueryObjectPath(h, b.as_mut_ptr(), b.len() as u32) } == 0 {
        return Vec::new();
    }
    cstr(b.as_ptr())
}

fn exists(p: &[u8]) -> bool {
    let z = z(p);
    let mut fs3 = [0u8; 24];
    unsafe { DosQueryPathInfo(z.as_ptr(), 1, fs3.as_mut_ptr(), 24) == 0 }
}

fn ini_has(app: &[u8], key: &[u8]) -> bool {
    let (a, k) = (z(app), z(key));
    let mut buf = [0u8; 512];
    let mut n = buf.len() as u32;
    unsafe { PrfQueryProfileData(HINI_USER, a.as_ptr(), k.as_ptr(), buf.as_mut_ptr(), &mut n) != 0 }
}

fn hex(v: u32) -> Vec<u8> {
    alloc::format!("{:X}", v).into_bytes()
}

/// The objects of a folder (wpQueryContent).
fn contents(folder: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut o = call(folder, b"wpQueryContent", &[0, 0]);
    while o != 0 && v.len() < 1000 {
        v.push(o);
        o = call(folder, b"wpQueryContent", &[o, 1]);
    }
    v
}

fn show(what: &str, t: &[Vec<u8>]) {
    let mut line = alloc::string::String::new();
    for x in t {
        line.push_str(s(x));
        line.push_str(" | ");
    }
    println!("  {}: {}", what, line);
}

fn titles(objs: &[usize]) -> Vec<Vec<u8>> {
    objs.iter().map(|&o| title(o)).collect()
}

fn make() {
    let desk = query(b"<WP_DESKTOP>");
    check("desktop handle", desk >> 16 == 3);
    check("desktop path", path(desk) == b"C:\\DESKTOP");
    let f = create(b"WPFolder", b"Test Folder", b"OBJECTID=<RS2_TESTFLDR>", b"<WP_DESKTOP>");
    check("folder made", f >> 16 == 3);
    let fp = path(f);
    check("folder directory", !fp.is_empty() && exists(&fp));
    check("folder by ID", query(b"<RS2_TESTFLDR>") == f);
    let p = create(b"WPProgram", b"Editor", b"EXENAME=C:\\OS2\\E.EXE;PARAMETERS=README;OBJECTID=<RS2_TESTPGM>", b"<RS2_TESTFLDR>");
    check("program made (abstract handle)", p >> 16 == 2);
    check("program in OS2.INI", ini_has(b"PM_Abstract:Objects", &hex(p & 0xffff)));
    check("folder content in OS2.INI", ini_has(b"PM_Abstract:FldrContent", &hex(f)));
    check("program by ID", query(b"<RS2_TESTPGM>") == p);
    let po = object(p);
    check("program object", po != 0);
    check("program title", title(po) == b"Editor");
    let mut buf = [0u8; 512];
    let mut cb = buf.len() as u32;
    let ok = call(po, b"wpQueryProgDetails", &[buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize]) != 0;
    let w = buf.as_ptr() as *const u32;
    check("program details", ok && cstr(unsafe { *w.add(4) } as *const u8) == b"C:\\OS2\\E.EXE" && cstr(unsafe { *w.add(5) } as *const u8) == b"README");
    let t = z(b"TITLE=Edit 2;NODELETE=NO");
    check("WinSetObjectData", unsafe { WinSetObjectData(p, t.as_ptr()) } != 0 && title(po) == b"Edit 2");
    let d = create(b"WPDataFile", b"notes.txt", b"", b"<RS2_TESTFLDR>");
    check("data file made", d >> 16 == 3 && exists(&path(d)));
    let fo = object(f);
    let c = titles(&contents(fo));
    check("folder content", c.iter().any(|t| t == b"Edit 2") && c.iter().any(|t| t == b"notes.txt"));
    let sh = unsafe { WinCreateShadow(p, desk, 0) };
    check("shadow made", sh >> 16 == 2);
    check("shadow shows the program's title", title(object(sh)) == b"Edit 2");
    let m = unsafe { WinMoveObject(p, desk, 0) };
    check("program moved to the desktop", m == p && ini_has(b"PM_Abstract:FldrContent", &hex(desk)));
    let cp = unsafe { WinCopyObject(d, desk, 0) };
    check("data file copied", cp >> 16 == 3 && exists(b"C:\\DESKTOP\\notes.txt"));
    let sub = create(b"WPFolder", b"Sub", b"", b"<RS2_TESTFLDR>");
    check("subfolder", sub != 0 && exists(&path(sub)));
    let cls = unsafe { WPObjectNewClass(1, 2) };
    check("WPObjectNewClass", cls != 0 && cls == unsafe { WPObjectClassData[0] });
    let title_tok = unsafe { WPObjectClassData[1] };
    check("WPObjectClassData's first token is WPObject's", unsafe { somGetClassFromMToken(title_tok) } == cls);
    let mut size = 0u32;
    unsafe { WinEnumObjectClasses(core::ptr::null_mut(), &mut size) };
    let mut eb = alloc::vec![0u8; size as usize];
    check("WinEnumObjectClasses", unsafe { WinEnumObjectClasses(eb.as_mut_ptr(), &mut size) } != 0);
    let _ = unsafe { M_WPObjectClassData[0] };
}

/// A new process: the objects are woken from what was stored.
fn verify() {
    let p = query(b"<RS2_TESTPGM>");
    check("program found by ID", p >> 16 == 2);
    let po = object(p);
    check("program woken from OS2.INI", po != 0 && title(po) == b"Edit 2");
    let mut buf = [0u8; 512];
    let mut cb = buf.len() as u32;
    call(po, b"wpQueryProgDetails", &[buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize]);
    let w = buf.as_ptr() as *const u32;
    check("program details restored", cstr(unsafe { *w.add(4) } as *const u8) == b"C:\\OS2\\E.EXE");
    let desk = query(b"<WP_DESKTOP>");
    let dobj = object(desk);
    let dc = titles(&contents(dobj));
    show("desktop", &dc);
    check("desktop has the program, its shadow and the copy", dc.iter().filter(|t| t.as_slice() == b"Edit 2").count() == 2 && dc.iter().any(|t| t == b"notes.txt"));
    let f = query(b"<RS2_TESTFLDR>");
    let fo = object(f);
    let fc = titles(&contents(fo));
    show("folder", &fc);
    let fp = path(f);
    check("folder has the data file and the subfolder", fc.iter().any(|t| t == b"notes.txt") && fc.iter().any(|t| t == b"Sub"));
    // Clean up: the copies, the shadow, the program, the folder.
    for o in contents(dobj) {
        let t = title(o);
        if t == b"Edit 2" || t == b"notes.txt" {
            call(o, b"wpFree", &[]);
        }
    }
    for o in contents(fo) {
        call(o, b"wpFree", &[]);
    }
    call(fo, b"wpFree", &[]);
    check("cleaned up", !exists(&fp) && query(b"<RS2_TESTPGM>") == 0 && !exists(b"C:\\DESKTOP\\NOTES.TXT"));
}

fn class_dll(args: &[u8]) {
    let mut it = args.split(|&c| c == b' ').filter(|w| !w.is_empty());
    let _ = it.next();
    let (Some(dll), Some(class)) = (it.next(), it.next()) else {
        println!("usage: WPSTEST CLASS dll class");
        return;
    };
    let (c, d) = (z(class), z(dll));
    check("WinRegisterObjectClass", unsafe { WinRegisterObjectClass(c.as_ptr(), d.as_ptr()) } != 0);
    let h = create(class, b"Sample Object", b"", b"<WP_DESKTOP>");
    check("object of the class made", h != 0);
    let o = object(h);
    if o != 0 {
        println!("  title {}", s(&title(o)));
        let cls = call(o, b"somGetClass", &[]);
        let ct = cstr(call(cls, b"wpclsQueryTitle", &[]) as *const u8);
        println!("  class title {}", s(&ct));
        println!("  metaclass {}", s(&cstr(call(cls, b"somGetClassName", &[]) as *const u8)));
        let want: &[u8] = match class {
            b"Car" => b"Car",
            // The compiled sample installs the override that returns its
            // parent's title (the one returning "Text Folder" is unused).
            b"TextFolder" => b"Folder",
            _ => b"Data File",
        };
        check("class title (the DLL's wpclsQueryTitle)", ct == want);
        if class == b"Car" {
            call(o, b"SetSpeed", &[42]);
            check("Car speed", call(o, b"QuerySpeed", &[]) == 42);
        }
        check("deleted", unsafe { WinDestroyObject(h) } != 0);
    }
    unsafe { WinDeregisterObjectClass(c.as_ptr()) };
}

fn main(st: &Startup) -> u32 {
    let args = st.args();
    let word = args.split(|&c| c == b' ').find(|w| !w.is_empty()).unwrap_or(b"");
    check("Rs2WpsStart", unsafe { Rs2WpsStart() } != 0);
    if word.eq_ignore_ascii_case(b"CHECK") {
        verify();
    } else if word.eq_ignore_ascii_case(b"CLASS") {
        class_dll(args);
    } else {
        make();
    }
    let n = unsafe { FAILS };
    println!("wpstest done: {} failure(s)", n);
    n
}
