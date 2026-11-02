//! Checks of SOMU.DLL (rs2's or IBM's): every class's name, parent and
//! class data, the class data order, a before/after metaclass, a single
//! instance class, SOMBuf and the UUID functions.

use alloc::vec::Vec;

use os2::println;

use crate::libs::{load, proc_addr};
use crate::{Spec, build, cd, check, check_eq, cstr, m1, m2, sc, sid, so, tok};

macro_rules! tramp {
    ($($tramp:ident => $target:path, $copy:literal;)*) => {$(
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
    )*};
}

static mut BEFORES: u32 = 0;
static mut AFTERS: u32 = 0;
static mut LAST_METHOD: [u8; 32] = [0; 32];
static mut LAST_RESULT: u32 = 0;
static mut INITS: u32 = 0;
static mut PINGS: u32 = 0;

unsafe extern "C" {
    #[link_name = "SOM.40"]
    fn somStringFromId(id: *mut *const u8) -> *const u8;
    #[link_name = "SOM.48"]
    fn somClassResolve(cls: usize, token: usize) -> usize;
}

extern "C" fn count_before(_cls: usize, _ev: usize, _obj: usize, mid: *mut *const u8, _ap: usize) -> u32 {
    let n = cstr(unsafe { somStringFromId(mid) });
    unsafe {
        BEFORES += 1;
        LAST_METHOD = [0; 32];
        let k = n.len().min(31);
        LAST_METHOD[..k].copy_from_slice(&n[..k]);
    }
    (n != b"blocked") as u32
}
extern "C" fn count_after(_cls: usize, _ev: usize, _obj: usize, _mid: usize, ret: *const u32, _ap: usize) {
    unsafe {
        AFTERS += 1;
        LAST_RESULT = if ret.is_null() { 0 } else { *ret };
    }
}
extern "C" fn w_ping(_o: usize) -> u32 {
    unsafe { PINGS += 1 };
    42
}
extern "C" fn w_add(_o: usize, x: u32) -> u32 {
    x + 1
}
extern "C" fn w_blocked(_o: usize) -> u32 {
    unsafe { PINGS += 100 };
    7
}
extern "C" fn solo_init(_o: usize) {
    unsafe { INITS += 1 };
}

tramp! {
    __t_count_before => count_before, 5;
    __t_count_after => count_after, 6;
    __t_w_ping => w_ping, 1;
    __t_w_add => w_add, 2;
    __t_w_blocked => w_blocked, 1;
    __t_solo_init => solo_init, 1;
}

static mut CAPTURE: Vec<u8> = Vec::new();
static mut SAVED_OUT: extern "C" fn(u32) -> u32 = capture;

tramp! {
    __t_capture => capture, 1;
}

unsafe extern "C" {
    #[link_name = "SOM.85"]
    fn somSetOutChar(f: extern "C" fn(u32) -> u32);
    #[link_name = "SOM.15"]
    static SOMOutCharRoutine: extern "C" fn(u32) -> u32;
}

extern "C" fn capture(c: u32) -> u32 {
    unsafe { CAPTURE.push(c as u8) };
    1
}

fn class_name_of(cls: usize) -> &'static [u8] {
    cstr(m1(sc(), 4, cls) as *const u8)
}

pub fn run() {
    let h = load(b"SOMU");
    if h == 0 {
        println!("SKIP SOMU.DLL did not load");
        return;
    }
    let newclass = |n: &[u8]| -> usize {
        let mut name = n.to_vec();
        name.extend_from_slice(b"NewClass");
        let p = proc_addr(h, &name);
        if p == 0 {
            return 0;
        }
        let f: extern "C" fn(i32, i32) -> usize = unsafe { core::mem::transmute(p) };
        f(0, 0)
    };
    let cdata = |n: &[u8]| -> *const usize {
        let mut name = n.to_vec();
        name.extend_from_slice(b"ClassData");
        proc_addr(h, &name) as *const usize
    };
    // Every class: its name, parent and class data.
    let expect: [(&[u8], &[u8]); 16] = [
        (b"somutMsgManager", b"SOMObject"),
        (b"M_somutMsgManager", b"SOMClass"),
        (b"SOMMMetaCooperative", b"SOMClass"),
        (b"SOMMCooperative", b"SOMClass"),
        (b"SOMMCooperativeSistered", b"SOMMCooperative"),
        (b"SOMMCooperativeRedispatched", b"SOMMCooperativeSistered"),
        (b"SOMMSingleInstance", b"SOMMProxy::HiddenClassMeta"),
        (b"SOMArgList", b"SOMObject"),
        (b"SOMMBeforeAfter", b"SOMMCooperativeRedispatched"),
        (b"SOMBuf", b"SOMObject"),
        (b"SOMCommand", b"SOMObject"),
        (b"SOMDesc", b"SOMObject"),
        (b"SOMUTId", b"SOMObject"),
        (b"SOMUTStringId", b"SOMUTId"),
        (b"SOMMTraced", b"SOMMBeforeAfter"),
        (b"SOMMBeforeAfterDispatcher", b"SOMMMetaCooperative"),
    ];
    let mut ok = 0;
    for (name, parent) in expect {
        let c = newclass(name);
        let d = cdata(name);
        let parent_name = if c != 0 { class_name_of(m1(sc(), 5, c)) } else { b"" };
        if c != 0 && !d.is_null() && unsafe { *d } == c && class_name_of(c) == name && parent_name == parent {
            ok += 1;
        } else {
            println!("  {} class {:08x} parent {}", core::str::from_utf8(name).unwrap_or("?"), c, core::str::from_utf8(parent_name).unwrap_or("?"));
        }
    }
    check_eq("SOMU classes (name, parent, class data)", ok, 16);
    let traced = newclass(b"SOMMTraced");
    check("SOMMTraced's metaclass", class_name_of(m1(so(0), 5, traced)) != b"SOMClass");
    // Class data order: the tokens resolve to the methods of these names.
    let by_name = |cls: usize, n: &[u8]| -> usize {
        let fm: extern "C" fn(usize, *mut *const u8) -> usize = unsafe { core::mem::transmute(tok(sc(), 24)) };
        fm(cls, sid(n))
    };
    let tcd = cdata(b"SOMMTraced");
    check(
        "SOMMTraced class data order (_set, _get)",
        unsafe {
            somClassResolve(traced, *tcd.add(1)) == by_name(traced, b"_set_sommTraceIsOn")
                && somClassResolve(traced, *tcd.add(2)) == by_name(traced, b"_get_sommTraceIsOn")
        },
    );
    let coop = newclass(b"SOMMCooperative");
    let ccd = cdata(b"SOMMCooperative");
    check(
        "SOMMCooperative class data order",
        unsafe {
            somClassResolve(coop, *ccd.add(1)) == by_name(coop, b"sommRequestFinalClassMethodCall")
                && somClassResolve(coop, *ccd.add(5)) == by_name(coop, b"sommSatisfyRequests")
        },
    );

    // A before/after metaclass counting the calls.
    unsafe {
        let (mc, mcc) = (cd(0), cd(8));
        build(&Spec {
            name: b"M_Counted",
            meta: None,
            parents: &[b"SOMMBeforeAfter"],
            data: 0,
            methods: &[],
            overrides: &[(b"sommBeforeMethod", __t_count_before as usize), (b"sommAfterMethod", __t_count_after as usize)],
            cd: mc,
            ccd: mcc,
        });
        let (wc, wcc) = (cd(3), cd(8));
        let watched = build(&Spec {
            name: b"Watched",
            meta: Some(b"M_Counted"),
            parents: &[b"SOMObject"],
            data: 4,
            methods: &[(b"ping", __t_w_ping as usize), (b"add", __t_w_add as usize), (b"blocked", __t_w_blocked as usize)],
            overrides: &[],
            cd: wc,
            ccd: wcc,
        });
        check("before/after class built", watched != 0 && *mc != 0);
        let o = m1(sc(), 0, watched);
        check("before/after instance", o != 0);
        BEFORES = 0;
        AFTERS = 0;
        let r = m1(wc, 0, o);
        check_eq("ping through before/after", r as i64, 42);
        check("before and after called once", BEFORES == 1 && AFTERS == 1 && cstr(LAST_METHOD.as_ptr()) == b"ping");
        check_eq("after sees the result", LAST_RESULT as i64, 42);
        let r = m2(wc, 1, o, 5);
        check_eq("add(5) through before/after", r as i64, 6);
        check_eq("after sees add's result", LAST_RESULT as i64, 6);
        PINGS = 0;
        BEFORES = 0;
        AFTERS = 0;
        let r = m1(wc, 2, o);
        println!("  blocked: pings {} result {} befores {} afters {}", PINGS, r, BEFORES, AFTERS);
        check("before FALSE: method not called", PINGS == 0 && BEFORES == 1 && AFTERS == 0);
        m1(so(0), 2, o);
    }

    // SOMMTraced: the trace of a call (captured through SOMOutCharRoutine;
    // the arguments and results are described when SOMIR names SOM.IR).
    // IBM's SOMU traces somDefaultInit as rs2's does, then faults in its
    // tracer (its SOMDesc/SOMArgList) on this class: checked with rs2's.
    let mut path = [0u8; 260];
    unsafe { os2::DosQueryModuleName(h, path.len() as u32, path.as_mut_ptr()) };
    let rs2 = cstr(path.as_ptr()).eq_ignore_ascii_case(b"C:\\OS2\\DLL\\SOMU.DLL");
    if !rs2 {
        println!("SKIP SOMMTraced (IBM's SOMU)");
    }
    if rs2 { unsafe {
        let (tcd_, tccd) = (cd(0), cd(8));
        let traced_cls = build(&Spec {
            name: b"Traced",
            meta: Some(b"SOMMTraced"),
            parents: &[b"SOMObject"],
            data: 4,
            methods: &[],
            overrides: &[],
            cd: tcd_,
            ccd: tccd,
        });
        let set_on: extern "C" fn(usize, usize, u32) = core::mem::transmute(*tcd.add(1));
        set_on(traced_cls, 0, 1);
        let t = m1(sc(), 0, traced_cls);
        CAPTURE.clear();
        SAVED_OUT = SOMOutCharRoutine;
        somSetOutChar(core::mem::transmute(__t_capture as usize));
        let name = cstr(m1(so(0), 4, t) as *const u8);
        let isa = m2(so(0), 6, t, traced_cls);
        somSetOutChar(SAVED_OUT);
        let text = core::mem::take(&mut CAPTURE);
        for line in text.split(|&c| c == b'\n').filter(|l| !l.is_empty()) {
            println!("  trace: {}", core::str::from_utf8(line).unwrap_or("?"));
        }
        let has = |s: &[u8]| text.windows(s.len()).any(|w| w == s);
        check("SOMMTraced traces somGetClassName", name == b"Traced" && has(b"somGetClassName( Traced at 0x") && has(b"somGetClassName returns"));
        check("SOMMTraced traces somIsA", isa & 0xff != 0 && has(b"somIsA( Traced at 0x") && has(b") called"));
        let mut v: *const u8 = core::ptr::null();
        if os2::DosScanEnv(b"SOMIR\0".as_ptr(), &mut v) == 0 {
            check("SOMMTraced: results from the interface repository", has(b"somGetClassName returns \"Traced\"") && has(b"somIsA returns TRUE"));
        }
        m1(so(0), 2, t);
    } }

    // SOMMSingleInstance: one instance, counted.
    unsafe {
        newclass(b"SOMMSingleInstance");
        let (scd, sccd) = (cd(0), cd(8));
        let solo = build(&Spec {
            name: b"Solo",
            meta: Some(b"SOMMSingleInstance"),
            parents: &[b"SOMObject"],
            data: 4,
            methods: &[],
            overrides: &[(b"somInit", __t_solo_init as usize)],
            cd: scd,
            ccd: sccd,
        });
        INITS = 0;
        let a = m1(sc(), 0, solo);
        let b = m1(sc(), 0, solo);
        check("SOMMSingleInstance: somNew twice, one instance", a != 0 && a == b && INITS == 1);
        let sicd = cdata(b"SOMMSingleInstance");
        let get: extern "C" fn(usize, usize) -> usize = core::mem::transmute(*sicd.add(1));
        check("sommGetSingleInstance", get(solo, 0) == a);
        m1(so(0), 2, a);
        m1(so(0), 2, a);
        check("still there after two of three frees", INITS == 1 && get(solo, 0) == a);
        m1(so(0), 2, a);
        m1(so(0), 2, a);
        let c = m1(sc(), 0, solo);
        check("a new instance after the last free", c != 0 && INITS == 2);
        m1(so(0), 2, c);
    }

    // SOMBuf.
    unsafe {
        let bcd = cdata(b"SOMBuf");
        let bufc = newclass(b"SOMBuf");
        let b = m1(sc(), 0, bufc);
        let call = |i: usize, args: &[usize]| -> usize {
            let t = *bcd.add(1 + i);
            match args.len() {
                1 => core::mem::transmute::<usize, extern "C" fn(usize) -> usize>(t)(args[0]),
                2 => core::mem::transmute::<usize, extern "C" fn(usize, usize) -> usize>(t)(args[0], args[1]),
                _ => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize) -> usize>(t)(args[0], args[1], args[2]),
            }
        };
        call(2, &[b, 0, 4]);
        for v in [1u32, 2, 3] {
            call(10, &[b, &v as *const u32 as usize]);
        }
        let nine = 9u32;
        call(8, &[b, 1, &nine as *const u32 as usize]);
        let vals = |b: usize| -> Vec<u32> { (0..call(3, &[b])).map(|i| *(call(5, &[b, i]) as *const u32)).collect() };
        check("SOMBuf append/insert", vals(b) == [1, 9, 2, 3]);
        call(9, &[b, 0]);
        check("SOMBuf delete, getLast", vals(b) == [9, 2, 3] && *(call(6, &[b]) as *const u32) == 3);
        let c = call(1, &[b]);
        check("SOMBuf copy compares equal", c != 0 && call(0, &[b, c]) & 0xff != 0);
        let seven = 7u32;
        call(7, &[c, 0, &seven as *const u32 as usize]);
        check("SOMBuf overwrite, compare", vals(c) == [7, 2, 3] && call(0, &[b, c]) & 0xff == 0);
        m1(so(0), 2, b);
        m1(so(0), 2, c);
    }

    // UUIDs (_Optlink).
    unsafe {
        let create = proc_addr(h, b"somCreateUUID");
        let to_string = proc_addr(h, b"somUUIDToString");
        let optlink2 = |f: usize, a: usize, b: usize| -> usize {
            let r: usize;
            core::arch::asm!("sub esp, 8", "call {f}", "add esp, 8", f = in(reg) f, inout("eax") a => r, inout("edx") b => _, out("ecx") _);
            r
        };
        let mut u1 = [0u8; 16];
        let mut u2 = [0u8; 16];
        optlink2(create, u1.as_mut_ptr() as usize, 0);
        optlink2(create, u2.as_mut_ptr() as usize, 0);
        check("somCreateUUID: two differ", u1 != u2 && u1[8] == 0x7f);
        let mut s = [0u8; 40];
        let t = [0x01u8, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe];
        optlink2(to_string, t.as_ptr() as usize, s.as_mut_ptr() as usize);
        check("somUUIDToString", cstr(s.as_ptr()) == b"67452301-efcdab89-10-32-547698badcfe");
    }
}
