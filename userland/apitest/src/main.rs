//! Kernel API regression test. Run it on rs2 (or real OS/2 for
//! comparison); every check prints PASS or FAIL, and the exit code is the
//! number of failures.

#![no_std]
#![no_main]

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

extern crate alloc;

mod eas;
mod highmem;
mod klibc;
mod t16;

os2::entry!(main);

mod console;

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

fn main(s: &Startup) -> u32 {
    println!("rs2 apitest, args: {:?}", core::str::from_utf8(s.args()).unwrap_or("?"));
    if core::str::from_utf8(s.args()).is_ok_and(|a| a.trim() == "keys") {
        console::keys();
        return 0;
    }
    if core::str::from_utf8(s.args()).is_ok_and(|a| a.trim() == "dbgchild") {
        return debug_child();
    }
    if core::str::from_utf8(s.args()).is_ok_and(|a| a.trim() == "dbgspin") {
        return debug_spin_child();
    }
    if core::str::from_utf8(s.args()).is_ok_and(|a| a.trim() == "debug") {
        // The DosDebug checks alone.
        debugging();
        let n = FAILS.load(Ordering::Relaxed);
        println!("apitest done: {n} failure(s)");
        return n;
    }
    if s.args().trim_ascii().starts_with(b"highchild") {
        return highmem::child(s.args().trim_ascii());
    }
    info_blocks();
    memory();
    suballoc();
    files();
    eas::run();
    threads_and_sems();
    t16::run();
    klibc::run();
    exceptions();
    unwind_own_report();
    pipes();
    named_pipes();
    dlls();
    app_types();
    timers();
    perf_counters();
    shared_memory();
    highmem::run();
    queues();
    sessions();
    debugging();
    cmd_beginlibpath();
    libpath_errors();
    exe_formats();
    messages();
    nls();
    console::run();
    exit_list();
    let f = FAILS.load(Ordering::Relaxed);
    println!("apitest done: {f} failure(s)");
    f
}

fn info_blocks() {
    let mut tib = null_mut();
    let mut pib = null_mut();
    let rc = unsafe { DosGetInfoBlocks(&mut tib, &mut pib) };
    check("DosGetInfoBlocks", rc == 0 && !tib.is_null() && !pib.is_null());
    unsafe {
        check("TIB2 thread 1", (*(*tib).ptib2).tid == 1);
        check("PIB pid", (*pib).pid != 0);
        // FS:[0] is the TIB's exception chain head.
        let fs0: u32;
        core::arch::asm!("mov {0}, fs:[0]", out(reg) fs0);
        check("FS:[0] = tib->pexchain", fs0 == (*tib).pexchain as u32);
    }
    let mut v = [0u32; 2];
    let rc = unsafe { DosQuerySysInfo(11, 12, v.as_mut_ptr(), 8) };
    check("DosQuerySysInfo version 20.45", rc == 0 && v == [20, 45]);
    // Python's os2 module reads the Toolkit's whole range, 1 to QSV_MAX
    // (31); IBM's kernel takes up to 32.
    let mut all = [0u32; 32];
    let rc = unsafe { DosQuerySysInfo(1, 32, all.as_mut_ptr(), 128) };
    check("DosQuerySysInfo 1..32", rc == 0 && all[0] == 260);
    let rc = unsafe { DosQuerySysInfo(1, 33, all.as_mut_ptr(), 128) };
    check("DosQuerySysInfo past the last index", rc == 87);
    let mut name: *const u8 = null();
    let rc = unsafe { DosScanEnv(b"PATH\0".as_ptr(), &mut name) };
    check("DosScanEnv PATH", rc == 0 && !name.is_null());
}

fn memory() {
    unsafe {
        let mut p = null_mut();
        let rc = DosAllocMem(&mut p, 0x20000, PAG_READ | PAG_WRITE);
        check("DosAllocMem reserve", rc == 0 && (p as u32) % 0x10000 == 0);
        let mut cb = 0x20000;
        let mut fl = 0;
        DosQueryMem(p, &mut cb, &mut fl);
        check("DosQueryMem reserved", fl & PAG_COMMIT == 0 && fl & PAG_BASE != 0);
        let rc = DosSetMem(p, 0x1000, PAG_COMMIT | PAG_READ | PAG_WRITE);
        *p = 42;
        check("DosSetMem commit + touch", rc == 0 && *p == 42);
        cb = 0x20000;
        DosQueryMem(p, &mut cb, &mut fl);
        check("DosQueryMem committed run", fl & PAG_COMMIT != 0 && cb == 0x1000);
        check("DosFreeMem", DosFreeMem(p) == 0);
        check("DosFreeMem twice fails", DosFreeMem(p) != 0);
    }
}

fn suballoc() {
    unsafe {
        let mut heap = null_mut();
        // Sparse: only reserved; the suballocator commits what it uses.
        let rc = DosAllocMem(&mut heap, 0x4000, PAG_READ | PAG_WRITE);
        check("DosSubSetMem init sparse", rc == 0 && DosSubSetMem(heap, DOSSUB_INIT | DOSSUB_SPARSE_OBJ, 0x2000) == 0);
        let (mut a, mut b, mut c) = (null_mut(), null_mut(), null_mut());
        let ok = DosSubAllocMem(heap, &mut a, 100) == 0
            && DosSubAllocMem(heap, &mut b, 100) == 0
            && DosSubAllocMem(heap, &mut c, 0x1000) == 0;
        check("DosSubAllocMem x3", ok && a != b && b != c && (a as u32) % 8 == 0);
        *a = 1;
        *c.add(0xfff) = 2;
        check("suballocated blocks usable", *a == 1 && *c.add(0xfff) == 2);
        let mut big = null_mut();
        check("DosSubAllocMem too much", DosSubAllocMem(heap, &mut big, 0x1800) == 311);
        check("DosSubFreeMem", DosSubFreeMem(heap, b, 100) == 0 && DosSubFreeMem(heap, a, 100) == 0);
        check("DosSubFreeMem twice overlaps", DosSubFreeMem(heap, a, 100) == 312);
        let mut d = null_mut();
        check("freed space reused", DosSubAllocMem(heap, &mut d, 200) == 0 && d == a);
        check("DosSubSetMem shrink fails", DosSubSetMem(heap, DOSSUB_GROW, 0x1000) == 310);
        check("DosSubSetMem grow", DosSubSetMem(heap, DOSSUB_GROW | DOSSUB_SPARSE_OBJ, 0x4000) == 0);
        check("DosSubAllocMem after grow", DosSubAllocMem(heap, &mut big, 0x1800) == 0);
        check("DosSubUnsetMem", DosSubUnsetMem(heap) == 0);
        DosFreeMem(heap);
    }
}

fn edit_name() {
    let cases: [(&[u8], &[u8], &[u8]); 5] = [
        (b"HELLO.CMD\0", b"*.BAK\0", b"HELLO.BAK"),
        (b"README.TXT\0", b"*.*\0", b"README.TXT"),
        (b"ABCDEF.TXT\0", b"??X*\0", b"ABXDEF.TXT"),
        (b"FILE.TAR.GZ\0", b"*.Z\0", b"FILE.TAR.Z"),
        (b"C:\\DIR\\NAME.EXT\0", b"NEW.*\0", b"NEW.EXT"),
    ];
    let mut ok = true;
    for (src, edit, want) in cases {
        let mut buf = [0u8; 64];
        let rc = unsafe { DosEditName(1, src.as_ptr(), edit.as_ptr(), buf.as_mut_ptr(), 64) };
        let n = buf.iter().position(|&b| b == 0).unwrap_or(0);
        if rc != 0 || &buf[..n] != want {
            ok = false;
            println!("  DosEditName {:?}: rc {rc} {:?}", core::str::from_utf8(edit), core::str::from_utf8(&buf[..n]));
        }
    }
    check("DosEditName", ok);
}

fn files() {
    edit_name();
    unsafe {
        let mut hf = 0;
        let mut action = 0;
        let rc = DosOpen(
            b"C:\\APITEST.TMP\0".as_ptr(),
            &mut hf,
            &mut action,
            0,
            FILE_NORMAL,
            OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
            OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE,
            null_mut(),
        );
        check("DosOpen create", rc == 0);
        let msg = b"Hello from rs2 apitest\r\n";
        let mut n = 0;
        check("DosWrite file", DosWrite(hf, msg.as_ptr(), msg.len() as u32, &mut n) == 0 && n == msg.len() as u32);
        let mut pos = 0;
        DosSetFilePtr(hf, 0, FILE_BEGIN, &mut pos);
        let mut buf = [0u8; 64];
        DosRead(hf, buf.as_mut_ptr(), 64, &mut n);
        check("DosRead back", n == msg.len() as u32 && &buf[..n as usize] == msg);
        // A second read/write open is refused by the deny-write share mode.
        let mut hf2 = 0;
        let rc2 = DosOpen(
            b"C:\\APITEST.TMP\0".as_ptr(),
            &mut hf2,
            &mut action,
            0,
            0,
            OPEN_ACTION_OPEN_IF_EXISTS,
            OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE,
            null_mut(),
        );
        check("share violation", rc2 == 32);
        DosClose(hf);
        let mut hdir = HDIR_CREATE;
        let mut count = 1;
        let mut fb: FileFindBuf3 = core::mem::zeroed();
        let rc = DosFindFirst(
            b"C:\\APITEST.*\0".as_ptr(),
            &mut hdir,
            0,
            &mut fb as *mut _ as *mut u8,
            core::mem::size_of::<FileFindBuf3>() as u32,
            &mut count,
            FIL_STANDARD,
        );
        check("DosFindFirst", rc == 0 && count == 1 && fb.cb_file == msg.len() as u32);
        DosFindClose(hdir);
        // DosCopy: new target, refusal without DCPY_EXISTING, replace, append.
        let size_of = |p: &[u8]| -> u32 {
            let mut fs: FileFindBuf3 = core::mem::zeroed();
            let mut h = HDIR_CREATE;
            let mut n = 1;
            let rc = DosFindFirst(p.as_ptr(), &mut h, 0, &mut fs as *mut _ as *mut u8, core::mem::size_of::<FileFindBuf3>() as u32, &mut n, FIL_STANDARD);
            DosFindClose(h);
            if rc == 0 { fs.cb_file } else { u32::MAX }
        };
        let src = b"C:\\APITEST.TMP\0".as_ptr();
        let dst = b"C:\\APICOPY.TMP\0";
        let l = msg.len() as u32;
        check("DosCopy", DosCopy(src, dst.as_ptr(), 0) == 0 && size_of(dst) == l);
        check("DosCopy existing refused", DosCopy(src, dst.as_ptr(), 0) != 0);
        check("DosCopy DCPY_EXISTING", DosCopy(src, dst.as_ptr(), DCPY_EXISTING) == 0 && size_of(dst) == l);
        check("DosCopy DCPY_APPEND", DosCopy(src, dst.as_ptr(), DCPY_APPEND) == 0 && size_of(dst) == 2 * l);
        check("DosForceDelete", DosForceDelete(dst.as_ptr()) == 0 && size_of(dst) == u32::MAX);
        check("DosDelete", DosDelete(b"C:\\APITEST.TMP\0".as_ptr()) == 0);
        check("DosCreateDir", DosCreateDir(b"C:\\APIDIR\0".as_ptr(), null_mut()) == 0);
        check("DosDeleteDir", DosDeleteDir(b"C:\\APIDIR\0".as_ptr()) == 0);
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);
static mut HEV: u32 = 0;
static mut HMTX: u32 = 0;

unsafe extern "C" fn worker(n: u32) {
    for _ in 0..n {
        unsafe {
            DosRequestMutexSem(HMTX, SEM_INDEFINITE_WAIT);
            let v = COUNTER.load(Ordering::Relaxed);
            DosSleep(0);
            COUNTER.store(v + 1, Ordering::Relaxed);
            DosReleaseMutexSem(HMTX);
        }
    }
    unsafe { DosPostEventSem(HEV) };
}

fn threads_and_sems() {
    unsafe {
        let mut hev = 0;
        let mut hmtx = 0;
        check("DosCreateEventSem", DosCreateEventSem(null(), &mut hev, 0, 0) == 0);
        check("DosCreateMutexSem", DosCreateMutexSem(null(), &mut hmtx, 0, 0) == 0);
        HEV = hev;
        HMTX = hmtx;
        check("DosWaitEventSem timeout", DosWaitEventSem(hev, 50) == 640);
        let mut t1 = 0;
        let mut t2 = 0;
        let rc1 = DosCreateThread(&mut t1, worker, 50, 0, 16384);
        let rc2 = DosCreateThread(&mut t2, worker, 50, 0, 16384);
        check("DosCreateThread x2", rc1 == 0 && rc2 == 0 && t1 != t2);
        check("DosWaitThread t1", DosWaitThread(&mut t1, DCWW_WAIT) == 0);
        check("DosWaitThread t2", DosWaitThread(&mut t2, DCWW_WAIT) == 0);
        check("mutex protected counter", COUNTER.load(Ordering::Relaxed) == 100);
        check("event posted", DosWaitEventSem(hev, SEM_IMMEDIATE_RETURN) == 0);
        let mut posts = 0;
        DosResetEventSem(hev, &mut posts);
        check("two posts", posts == 2);
        let recs = [SemRecord { hsem: hev, user: 7 }];
        let mut hmux = 0;
        check("DosCreateMuxWaitSem", DosCreateMuxWaitSem(null(), &mut hmux, 1, recs.as_ptr(), DCMW_WAIT_ANY) == 0);
        DosPostEventSem(hev);
        let mut user = 0;
        check("DosWaitMuxWaitSem", DosWaitMuxWaitSem(hmux, 100, &mut user) == 0 && user == 7);
        check("release unowned mutex fails", DosReleaseMutexSem(hmtx) == 288);
        DosCloseMutexSem(hmtx);
        DosCloseEventSem(hev);
    }
}

static mut FAULTS: u32 = 0;
static mut GUARD_AT: u32 = 0;
static mut GUARD_ACCESS: u32 = 0;

static mut UNWOUND: u32 = 0;

unsafe extern "C" fn unwind_handler(rep: *mut ExceptionReportRecord, _reg: *mut ExceptionRegistrationRecord, _ctx: *mut ContextRecord, _d: *mut u8) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 {
            UNWOUND += 1;
        }
    }
    XCPT_CONTINUE_SEARCH
}

/// DosUnwindException with a report record in the caller's own frame (Open
/// Watcom's C++ throw passes one): the system's records and the handlers'
/// frames go below the caller's stack, not below the report, which is
/// live stack (rs2 wrote its context record there: the throwing
/// function's saved registers became 0 and the program faulted).
fn unwind_own_report() {
    #[repr(C)]
    struct Frame {
        below: [u32; 96],
        report: ExceptionReportRecord,
        above: [u32; 8],
    }
    unsafe {
        let mut outer = ExceptionRegistrationRecord { prev: null_mut(), handler };
        DosSetExceptionHandler(&mut outer);
        let mut inner = ExceptionRegistrationRecord { prev: null_mut(), handler: unwind_handler };
        DosSetExceptionHandler(&mut inner);
        let mut fr: Frame = core::mem::zeroed();
        fr.below = [0x5a5a_5a5a; 96];
        fr.above = [0xa5a5_a5a5; 8];
        fr.report.num = 0x2000_0002;
        UNWOUND = 0;
        let rc = DosUnwindException(&mut outer, null(), &mut fr.report);
        let fr = core::ptr::read_volatile(&fr);
        let mut head: u32;
        core::arch::asm!("mov {0}, fs:[0]", out(reg) head);
        check("DosUnwindException with the caller's report: the handler below the target is called", rc == 0 && UNWOUND == 1 && head == &raw mut outer as u32);
        check("the report gets EH_UNWINDING", fr.report.flags & EH_UNWINDING != 0);
        check("and the caller's stack around the report is left alone", fr.below.iter().all(|&w| w == 0x5a5a_5a5a) && fr.above.iter().all(|&w| w == 0xa5a5_a5a5));
        DosUnsetExceptionHandler(&mut outer);
    }
}

unsafe extern "C" fn handler(
    rep: *mut ExceptionReportRecord,
    _reg: *mut ExceptionRegistrationRecord,
    ctx: *mut ContextRecord,
    _d: *mut u8,
) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 {
            return XCPT_CONTINUE_SEARCH;
        }
        match (*rep).num {
            XCPT_ACCESS_VIOLATION => {
                FAULTS += 1;
                // Skip the faulting instruction: "mov dword ptr [eax], ecx" (2 bytes).
                (*ctx).eip += 2;
                XCPT_CONTINUE_EXECUTION
            }
            0x2000_0001 => {
                FAULTS += 10;
                XCPT_CONTINUE_EXECUTION
            }
            // A guard page: recorded (its address, the access), then the
            // access is retried (the page is committed now).
            0x8000_0001 => {
                FAULTS += 100;
                GUARD_AT = (*rep).info[1];
                GUARD_ACCESS = (*rep).info[0];
                XCPT_CONTINUE_EXECUTION
            }
            _ => XCPT_CONTINUE_SEARCH,
        }
    }
}

fn exceptions() {
    unsafe {
        let mut rec = ExceptionRegistrationRecord { prev: null_mut(), handler };
        check("DosSetExceptionHandler", DosSetExceptionHandler(&mut rec) == 0);
        // Write to address 0: access violation, handled and skipped.
        core::arch::asm!("xor eax, eax", "mov dword ptr [eax], ecx", out("eax") _, in("ecx") 1);
        check("access violation handled", FAULTS == 1);
        let mut r: ExceptionReportRecord = core::mem::zeroed();
        r.num = 0x2000_0001;
        let rc = DosRaiseException(&mut r);
        check("DosRaiseException continued", rc == 0 && FAULTS == 11);
        // A guard page (DosSetMem PAG_GUARD) raises XCPT_GUARD_PAGE_VIOLATION
        // to the handlers on the first touch (the page is then plain
        // committed memory); programs grow their heaps so (Borland's TLINK).
        let mut p: *mut u8 = null_mut();
        let ok = DosAllocMem(&mut p, 0x4000, PAG_READ | PAG_WRITE) == 0
            && DosSetMem(p, 0x1000, 0x400 | PAG_COMMIT) == 0
            && DosSetMem(p.add(0x1000), 0x1000, 0x400 | PAG_COMMIT | PAG_GUARD) == 0;
        check("DosSetMem PAG_GUARD", ok);
        if ok {
            core::ptr::write_volatile(p.add(0x1010), 7u8);
            check("the guard page raises XCPT_GUARD_PAGE_VIOLATION to the handler", FAULTS == 111 && GUARD_AT == p as u32 + 0x1010 && GUARD_ACCESS == 2);
            core::ptr::write_volatile(p.add(0x1020), 8u8);
            check("only once, and the write went through", FAULTS == 111 && core::ptr::read_volatile(p.add(0x1010)) == 7);
            DosFreeMem(p);
        }
        check("DosUnsetExceptionHandler", DosUnsetExceptionHandler(&mut rec) == 0);
    }
}

fn pipes() {
    unsafe {
        let mut r = 0;
        let mut w = 0;
        let rc = DosCreatePipe(&mut r, &mut w, 4096);
        check("DosCreatePipe", rc == 0);
        let mut n = 0;
        DosWrite(w, b"pipe!".as_ptr(), 5, &mut n);
        let mut buf = [0u8; 8];
        DosRead(r, buf.as_mut_ptr(), 8, &mut n);
        check("pipe round trip", n == 5 && &buf[..5] == b"pipe!");
        DosClose(w);
        DosRead(r, buf.as_mut_ptr(), 8, &mut n);
        check("pipe EOF after close", n == 0);
        DosClose(r);
    }
}

static HPIPE: AtomicU32 = AtomicU32::new(0);
/// Bit 0: connected, 1: request read, 2: EOF after the client closed.
static SERVER: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn pipe_server(_: u32) {
    let h = HPIPE.load(Ordering::Relaxed);
    let mut buf = [0u8; 16];
    let mut n = 0;
    unsafe {
        if DosConnectNPipe(h) == 0 {
            SERVER.fetch_or(1, Ordering::Relaxed);
        }
        if DosRead(h, buf.as_mut_ptr(), 16, &mut n) == 0 && &buf[..n as usize] == b"hello" {
            SERVER.fetch_or(2, Ordering::Relaxed);
        }
        DosWrite(h, b"HELLO".as_ptr(), 5, &mut n);
        DosWrite(h, b"ab".as_ptr(), 2, &mut n);
        DosWrite(h, b"cd".as_ptr(), 2, &mut n);
        if DosRead(h, buf.as_mut_ptr(), 16, &mut n) == 0 && n == 0 {
            SERVER.fetch_or(4, Ordering::Relaxed);
        }
        DosDisConnectNPipe(h);
    }
}

fn named_pipes() {
    let name = b"\\PIPE\\RS2TEST\0".as_ptr();
    unsafe {
        let mut h = 0;
        let rc = DosCreateNPipe(name, &mut h, NP_ACCESS_DUPLEX, NP_TYPE_MESSAGE | NP_READMODE_MESSAGE | 1, 512, 512, 0);
        check("DosCreateNPipe", rc == 0);
        HPIPE.store(h, Ordering::Relaxed);
        let mut tid = 0;
        DosCreateThread(&mut tid, pipe_server, 0, 0, 16384);
        check("DosWaitNPipe", DosWaitNPipe(name, 1000) == 0);
        let (mut hc, mut act) = (0, 0);
        let rc = DosOpen(name, &mut hc, &mut act, 0, 0, 1, 0x42, null_mut());
        check("DosOpen pipe", rc == 0);
        let (mut h2, mut act2) = (0, 0);
        check("second client busy", DosOpen(name, &mut h2, &mut act2, 0, 0, 1, 0x42, null_mut()) == 231);
        check("DosSetNPHState", DosSetNPHState(hc, NP_READMODE_MESSAGE) == 0);
        let mut st = 0;
        DosQueryNPHState(hc, &mut st);
        check("DosQueryNPHState", st & 0x4500 == 0x0500 && st & 0xff == 1);
        let mut buf = [0u8; 16];
        let mut n = 0;
        let rc = DosTransactNPipe(hc, b"hello".as_ptr(), 5, buf.as_mut_ptr(), 16, &mut n);
        check("DosTransactNPipe", rc == 0 && &buf[..n as usize] == b"HELLO");
        // Wait for both messages to be queued.
        let mut avail = [0u16; 2];
        let mut state = 0;
        for _ in 0..100 {
            DosPeekNPipe(hc, buf.as_mut_ptr(), 16, &mut n, avail.as_mut_ptr(), &mut state);
            if avail[0] == 4 {
                break;
            }
            DosSleep(10);
        }
        check("DosPeekNPipe", avail == [4, 2] && n == 2 && &buf[..2] == b"ab" && state == 3);
        let rc = DosRead(hc, buf.as_mut_ptr(), 1, &mut n);
        check("message read ERROR_MORE_DATA", rc == 234 && n == 1 && buf[0] == b'a');
        let rc = DosRead(hc, buf.as_mut_ptr(), 16, &mut n);
        check("rest of message", rc == 0 && n == 1 && buf[0] == b'b');
        let rc = DosRead(hc, buf.as_mut_ptr(), 16, &mut n);
        check("next message", rc == 0 && &buf[..n as usize] == b"cd");
        let mut info = [0u8; 32];
        DosQueryNPipeInfo(hc, 1, info.as_mut_ptr(), 32);
        check("DosQueryNPipeInfo", info[4] == 1 && &info[7..7 + info[6] as usize] == b"\\PIPE\\RS2TEST");
        DosClose(hc);
        DosWaitThread(&mut tid, DCWW_WAIT);
        check("server side", SERVER.load(Ordering::Relaxed) == 7);
        DosClose(h);
        check("pipe gone", DosWaitNPipe(name, 10) == 2);
    }
    reply_then_disconnect();
    byte_mode_messages();
}

/// A client in byte read mode (as DosOpen leaves it) on a message pipe
/// reads one message at a time, as IBM's kernel copies them (Open
/// Watcom's NMP link sends a 1-byte packet and then its length as two
/// messages and tells them apart by the count read).
fn byte_mode_messages() {
    let name = b"\\PIPE\\RS2BYTE\0".as_ptr();
    unsafe {
        let mut h = 0;
        DosCreateNPipe(name, &mut h, NP_ACCESS_DUPLEX, NP_TYPE_MESSAGE | NP_READMODE_MESSAGE | 1, 64, 64, 0);
        let (mut hc, mut act) = (0, 0);
        let rc = DosOpen(name, &mut hc, &mut act, 0, 0, 1, 0x42, null_mut());
        DosConnectNPipe(h);
        let mut n = 0;
        DosWrite(h, b"x".as_ptr(), 1, &mut n);
        DosWrite(h, b"1234".as_ptr(), 4, &mut n);
        DosWrite(h, b"abcdef".as_ptr(), 6, &mut n);
        let mut buf = [0u8; 16];
        let (mut avail, mut state) = ([0u16; 2], 0);
        DosPeekNPipe(hc, buf.as_mut_ptr(), 16, &mut n, avail.as_mut_ptr(), &mut state);
        check("byte read mode: DosPeekNPipe of a message pipe gives the first message", rc == 0 && n == 1 && buf[0] == b'x' && avail == [11, 1]);
        let r1 = DosRead(hc, buf.as_mut_ptr(), 16, &mut n);
        check("byte read mode: DosRead of a message pipe reads one message", r1 == 0 && n == 1 && buf[0] == b'x');
        let r2 = DosRead(hc, buf.as_mut_ptr(), 16, &mut n);
        check("byte read mode: then the next one", r2 == 0 && &buf[..n as usize] == b"1234");
        let r3 = DosRead(hc, buf.as_mut_ptr(), 4, &mut n);
        let r4 = DosRead(hc, buf[4..].as_mut_ptr(), 16, &mut n);
        check("byte read mode: part of a message, without ERROR_MORE_DATA, then its rest", r3 == 0 && r4 == 0 && n == 2 && &buf[..6] == b"abcdef");
        DosClose(hc);
        DosClose(h);
    }
}

static PIPE_CLIENT: AtomicU32 = AtomicU32::new(0);
static PIPE_GOT: AtomicU32 = AtomicU32::new(0);

/// A client blocked in DosRead (its request sent).
unsafe extern "C" fn pipe_reader(_: u32) {
    let mut buf = [0u8; 8];
    let mut n = 0;
    let rc = unsafe { DosRead(PIPE_CLIENT.load(Ordering::Relaxed), buf.as_mut_ptr(), 8, &mut n) };
    PIPE_GOT.store(if rc == 0 && n == 1 { buf[0] as u32 } else { 0x100 | rc }, Ordering::Relaxed);
}

/// A server that writes its one-byte answer and disconnects at once (Open
/// Watcom's NMPBIND): the client already waiting in DosRead gets it, as
/// on OS/2, where the write completes the waiting read.
fn reply_then_disconnect() {
    let name = b"\\PIPE\\RS2BIND\0".as_ptr();
    unsafe {
        let mut h = 0;
        DosCreateNPipe(name, &mut h, NP_ACCESS_DUPLEX, NP_TYPE_MESSAGE | NP_READMODE_MESSAGE | 1, 64, 64, 0);
        let (mut hc, mut act) = (0, 0);
        let rc = DosOpen(name, &mut hc, &mut act, 0, 0, 1, 0x42, null_mut());
        DosConnectNPipe(h);
        PIPE_CLIENT.store(hc, Ordering::Relaxed);
        PIPE_GOT.store(0, Ordering::Relaxed);
        let mut tid = 0;
        DosCreateThread(&mut tid, pipe_reader, 0, 0, 16384);
        DosSleep(100);
        let mut n = 0;
        DosWrite(h, b"\x06".as_ptr(), 1, &mut n);
        DosDisConnectNPipe(h);
        DosWaitThread(&mut tid, DCWW_WAIT);
        let got = PIPE_GOT.load(Ordering::Relaxed);
        check(&alloc::format!("a reply written just before DosDisConnectNPipe reaches the waiting client (got {got:#x})"), rc == 0 && got == 6);
        let mut buf = [0u8; 4];
        check("then the client's handle is not connected", DosRead(hc, buf.as_mut_ptr(), 4, &mut n) == 233);
        DosClose(hc);
        DosClose(h);
    }
}

fn sessions() {
    unsafe {
        let mut hq = 0;
        DosCreateQueue(&mut hq, 0, b"\\QUEUES\\APITEST\\TERM\0".as_ptr());
        let sd = StartData {
            length: core::mem::size_of::<StartData>() as u16,
            related: 1,
            fgbg: 1,
            trace: 0,
            title: b"apitest child\0".as_ptr(),
            name: b"C:\\OS2\\CMD.EXE\0".as_ptr(),
            inputs: b"/C exit 7\0".as_ptr(),
            termq: b"\\QUEUES\\APITEST\\TERM\0".as_ptr(),
            env: null(),
            inherit: 0,
            session_type: 0,
        };
        let (mut sid, mut pid) = (0, 0);
        let rc = DosStartSession(&sd, &mut sid, &mut pid);
        check("DosStartSession related", rc == 0 && sid != 0 && pid != 0);
        let mut req = RequestData::default();
        let mut cb = 0;
        let mut pb: *const u8 = null();
        let mut prio = 0u8;
        let rc = DosReadQueue(hq, &mut req, &mut cb, &mut pb, 0, 0, &mut prio, 0);
        let ok = rc == 0 && cb == 4 && *(pb as *const u16) == sid as u16 && *(pb.add(2) as *const u16) == 7;
        check("session termination queue", ok);
        check("stop ended session fails", DosStopSession(0, sid) == 371);
        DosCloseQueue(hq);
    }
}

/// CMD's SET BEGINLIBPATH is the process's extended LIBPATH (IBM CMD
/// calls DosSetExtLIBPATH; VisualAge Smalltalk's VAWINDOW.CMD relies on
/// it), and %BEGINLIBPATH% reads it back; `CMD /C` runs a batch file to
/// its end.
fn cmd_beginlibpath() {
    unsafe {
        let mut hf = 0;
        let mut action = 0;
        let rc = DosOpen(b"C:\\RS2LP.CMD\0".as_ptr(), &mut hf, &mut action, 0, FILE_NORMAL,
            OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE, null_mut());
        let text = b"@SET BEGINLIBPATH=C:\\RS2LP\r\n@ECHO %BEGINLIBPATH%>C:\\RS2LP.TXT\r\n";
        let mut n = 0;
        DosWrite(hf, text.as_ptr(), text.len() as u32, &mut n);
        DosClose(hf);
        let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
        let rc2 = DosExecPgm(null_mut(), 0, 0, b"CMD.EXE\0/C C:\\RS2LP.CMD\0\0".as_ptr(), null(), &mut res, b"C:\\OS2\\CMD.EXE\0".as_ptr());
        let mut buf = [0u8; 32];
        let mut got = 0;
        let rc3 = DosOpen(b"C:\\RS2LP.TXT\0".as_ptr(), &mut hf, &mut action, 0, FILE_NORMAL, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, null_mut());
        if rc3 == 0 {
            DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut got);
            DosClose(hf);
        }
        DosDelete(b"C:\\RS2LP.CMD\0".as_ptr());
        DosDelete(b"C:\\RS2LP.TXT\0".as_ptr());
        check("CMD: SET BEGINLIBPATH is the extended LIBPATH", rc == 0 && rc2 == 0 && buf[..got as usize].starts_with(b"C:\\RS2LP\r\n"));
    }
}

/// The DLL search passes a drive that does not exist, and stops with the
/// error at one that is not ready (the empty CD drive, when there is one).
fn libpath_errors() {
    unsafe {
        let mut h = 0;
        let begin = |dir: &[u8]| DosSetExtLIBPATH(dir.as_ptr(), 1);
        DosError(0);
        let rc = begin(b"Q:\\NOPE\0");
        let rc2 = DosLoadModule(null_mut(), 0, b"NOSUCH\0".as_ptr(), &mut h);
        check("DLL search passes a drive that does not exist", rc == 0 && rc2 == 126);
        let mut hf = 0;
        let mut action = 0;
        let rc = DosOpen(b"E:\\X\0".as_ptr(), &mut hf, &mut action, 0, FILE_NORMAL, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, null_mut());
        if rc == 21 {
            begin(b"E:\\USR\\LIB\0");
            let rc = DosLoadModule(null_mut(), 0, b"NOSUCH\0".as_ptr(), &mut h);
            check("DLL search stops at a drive that is not ready", rc == 21);
        } else {
            println!("E: is not an empty drive ({rc}): not-ready search not tested");
        }
        begin(b"\0");
        DosError(1);
    }
}

/// Executables OS/2 does not run itself, as IBM's loader judges them
/// (_ldrOpenNewExe, LDRQAppType): a DOS extender's LE program (DOS/4GW's)
/// fails with ERROR_INVALID_EXE_SIGNATURE in DosQueryAppType and
/// DosExecPgm, a Win32 PE program is 0x5000, a plain MZ program a DOS one.
fn exe_formats() {
    unsafe {
        let make = |name: &[u8], sig: &[u8]| {
            let mut img = [0u8; 0x100];
            img[0] = b'M';
            img[1] = b'Z';
            img[0x3c] = 0x80;
            img[0x80..0x80 + sig.len()].copy_from_slice(sig);
            let (mut hf, mut action, mut n) = (0, 0, 0);
            DosOpen(name.as_ptr(), &mut hf, &mut action, 0, FILE_NORMAL,
                OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE, null_mut());
            DosWrite(hf, img.as_ptr(), img.len() as u32, &mut n);
            DosClose(hf);
        };
        make(b"C:\\APITLE.EXE\0", b"LE\0\0");
        make(b"C:\\APITPE.EXE\0", b"PE\0\0");
        make(b"C:\\APITMZ.EXE\0", b"");
        let mut t = 0;
        let rc = DosQueryAppType(b"C:\\APITLE.EXE\0".as_ptr(), &mut t);
        check("DosQueryAppType of an LE (DOS extender) program fails with 191", rc == 191);
        let rc = DosQueryAppType(b"C:\\APITPE.EXE\0".as_ptr(), &mut t);
        check("DosQueryAppType of a PE program is 0x5000", rc == 0 && t == 0x5000);
        let rc = DosQueryAppType(b"C:\\APITMZ.EXE\0".as_ptr(), &mut t);
        check("DosQueryAppType of an MZ program is FAPPTYP_DOS", rc == 0 && t == 0x20);
        let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
        let mut fail = [0u8; 64];
        let rc = DosExecPgm(fail.as_mut_ptr(), 64, 0, b"APITLE\0\0".as_ptr(), null(), &mut res, b"C:\\APITLE.EXE\0".as_ptr());
        check("DosExecPgm of an LE program fails with 191", rc == 191);
        for n in [&b"C:\\APITLE.EXE\0"[..], b"C:\\APITPE.EXE\0", b"C:\\APITMZ.EXE\0"] {
            DosDelete(n.as_ptr());
        }
    }
}

unsafe extern "C" fn on_exit(code: u32) {
    println!("exit list routine ran (termination code {code})");
    unsafe { DosExitList(EXLST_EXIT, on_exit) };
}

fn exit_list() {
    let rc = unsafe { DosExitList(EXLST_ADD | 0x100, on_exit) };
    check("DosExitList add", rc == 0);
}

unsafe extern "C" {
    #[link_name = "TESTDLL.1"]
    fn TestAdd(a: u32, b: u32) -> u32;
    #[link_name = "TESTDLL.TestInitCount"]
    fn TestInitCount() -> u32;
    #[link_name = "TESTDLL.6"]
    fn TestSelfAdd(a: u32, b: u32) -> u32;
}

/// DosQueryAppType and DosExecPgm of files that are not OS/2 programs, as
/// IBM's loader (_LDRQAppType, _ldrOpenNewExe): a plain MZ program is a
/// DOS program, a PE file FAPPTYP_32BIT | FAPPTYP_WINDOWSPROT31, an LE file
/// (a DOS/4GW program) ERROR_INVALID_EXE_SIGNATURE, and DosExecPgm of
/// the LE and PE files fails with that error (rs2 called LE a virtual device driver).
fn app_types() {
    let mk = |name: &[u8], sig: &[u8], extra: &[(usize, u8)]| {
        let mut img = [0u8; 0x100];
        img[0] = b'M';
        img[1] = b'Z';
        if !sig.is_empty() {
            img[0x3c] = 0x80;
            img[0x80..0x80 + sig.len()].copy_from_slice(sig);
        }
        for &(at, v) in extra {
            img[at] = v;
        }
        let (mut hf, mut action, mut n) = (0, 0, 0);
        unsafe {
            DosOpen(name.as_ptr(), &mut hf, &mut action, 0, FILE_NORMAL, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE, null_mut());
            DosWrite(hf, img.as_ptr(), img.len() as u32, &mut n);
            DosClose(hf);
        }
    };
    let files: [(&[u8], &[u8], &[(usize, u8)], Result<u32, u32>); 4] = [
        (b"C:\\AT_DOS.EXE\0", b"", &[], Ok(0x20)),
        (b"C:\\AT_LE.EXE\0", b"LE\0\0", &[], Err(191)),
        (b"C:\\AT_PE.EXE\0", b"PE\0\0", &[], Ok(0x5000)),
        (b"C:\\AT_PED.DLL\0", b"PE\0\0", &[(0x80 + 0x17, 0x20)], Ok(0x5010)),
    ];
    for (name, sig, extra, want) in files {
        mk(name, sig, extra);
        let mut ty = 0;
        let rc = unsafe { DosQueryAppType(name.as_ptr(), &mut ty) };
        let got = if rc == 0 { Ok(ty) } else { Err(rc) };
        let label = core::str::from_utf8(&name[3..name.len() - 1]).unwrap_or("?");
        check(&alloc::format!("DosQueryAppType {label}: {want:x?} (got {got:x?})"), got == want);
        // (DosExecPgm of a DOS program starts a DOS session.)
        if !sig.is_empty() && !name.ends_with(b".DLL\0") {
            let mut fail = [0u8; 64];
            let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
            let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), 64, EXEC_SYNC, b"x\0\0".as_ptr(), null(), &mut res, name.as_ptr()) };
            check(&alloc::format!("DosExecPgm {label}: ERROR_INVALID_EXE_SIGNATURE (got {rc})"), rc == 191);
        }
        unsafe { DosDelete(name.as_ptr()) };
    }
    let mut ty = 0;
    let rc = unsafe { DosQueryAppType(b"C:\\RS2\\TESTDLL.DLL\0".as_ptr(), &mut ty) };
    check(&alloc::format!("DosQueryAppType of an LX DLL: FAPPTYP_32BIT | FAPPTYP_DLL (0x10) (got {ty:#x})"), rc == 0 && ty & 0x4018 == 0x4010);
}

fn dlls() {
    unsafe {
        check("import by ordinal", TestAdd(40, 2) == 42);
        check("DLL init ran once", TestInitCount() == 1);
        check("a DLL importing itself (a circular reference)", TestSelfAdd(40, 1) == 42);
        let mut h = 0;
        let rc = DosLoadModule(null_mut(), 0, b"TESTDLL\0".as_ptr(), &mut h);
        check("DosLoadModule", rc == 0 && h != 0);
        let mut h2 = 0;
        check("DosQueryModuleHandle", DosQueryModuleHandle(b"TESTDLL\0".as_ptr(), &mut h2) == 0 && h2 == h);
        let mut pfn: usize = 0;
        let rc = DosQueryProcAddr(h, 0, b"TestGreeting\0".as_ptr(), &mut pfn);
        let greet: extern "C" fn() -> *const u8 = core::mem::transmute(pfn);
        let p = greet();
        check("DosQueryProcAddr by name", rc == 0 && *p == b'h');
        check("init not repeated", TestInitCount() == 1);
        let rc = DosLoadModule(null_mut(), 0, b"NOSUCH\0".as_ptr(), &mut h2);
        check("DosLoadModule missing", rc == 2 || rc == 126);
        // Loaded by its path, a DLL is a module of its own even with the
        // name of another (Python's lib-dynload\bz2.pyd and BZ2.DLL).
        let copied = DosCopy(b"C:\\RS2\\TESTDLL.DLL\0".as_ptr(), b"C:\\TESTDLL.PYD\0".as_ptr(), 1);
        let (mut hs, mut hs2, mut hn) = (0, 0, 0);
        let rc = DosLoadModule(null_mut(), 0, b"C:\\TESTDLL.PYD\0".as_ptr(), &mut hs);
        check("a DLL by its path is another module", copied == 0 && rc == 0 && hs != 0 && hs != h);
        let rc = DosLoadModule(null_mut(), 0, b"C:\\TESTDLL.PYD\0".as_ptr(), &mut hs2);
        check("the same path finds it again", rc == 0 && hs2 == hs);
        let mut hs3 = 0;
        let rc = DosLoadModule(null_mut(), 0, b"c:/testdll.pyd\0".as_ptr(), &mut hs3);
        check("so does the path with slashes", rc == 0 && hs3 == hs);
        DosFreeModule(hs3);
        let rc = DosLoadModule(null_mut(), 0, b"TESTDLL\0".as_ptr(), &mut hn);
        check("the name still finds the DLL found by name", rc == 0 && hn == h);
        // Still in use, without a termination routine: its data stays.
        let (mut hg, mut hg2) = (0, 0);
        let mut pg: usize = 0;
        let rc = DosLoadModule(null_mut(), 0, b"TESTDLLN\0".as_ptr(), &mut hg);
        let ok = rc == 0 && DosQueryProcAddr(hg, 2, null(), &mut pg) == 0;
        check("a DLL with global init loads", ok);
        if ok {
            let count: extern "C" fn() -> u32 = core::mem::transmute(pg);
            let before = count();
            let rc = DosLoadModule(null_mut(), 0, b"TESTDLLN\0".as_ptr(), &mut hg2);
            check("loading a DLL in use keeps its data", rc == 0 && before == 1 && count() == 1);
            DosFreeModule(hg2);
            DosFreeModule(hg);
        }
        DosFreeModule(hs);
        DosFreeModule(hs2);
        DosFreeModule(hn);
        check("DosFreeModule", DosFreeModule(h) == 0);
        // TESTDLL's twin flagged as having its internal fixups removed
        // (yet carrying them), its preferred bases taken by TESTDLL: moved
        // and fixed up all the same (VisualAge Smalltalk's EMNTV50.DLL).
        let mut hb = 0;
        let rc = DosLoadModule(null_mut(), 0, b"TESTDLLB\0".as_ptr(), &mut hb);
        let mut pa: usize = 0;
        let mut pc: usize = 0;
        let ok = rc == 0
            && DosQueryProcAddr(hb, 1, null(), &mut pa) == 0
            && DosQueryProcAddr(hb, 2, null(), &mut pc) == 0;
        let (sum, inits) = if ok {
            let add: extern "C" fn(u32, u32) -> u32 = core::mem::transmute(pa);
            let count: extern "C" fn() -> u32 = core::mem::transmute(pc);
            (add(40, 2), count())
        } else {
            (0, 0)
        };
        check("DLL marked without internal fixups moved off taken bases", ok && pa != TestAdd as usize && sum == 42 && inits == 1);
        if rc == 0 {
            DosFreeModule(hb);
            // Let go everywhere, loaded again: its shared data starts over
            // (OS/2 reads it in again; VisualAge's ABTBMP50.DLL relies on it).
            let mut hb2 = 0;
            let mut ps: usize = 0;
            let rc = DosLoadModule(null_mut(), 0, b"TESTDLLB\0".as_ptr(), &mut hb2);
            let shared = if rc == 0 && DosQueryProcAddr(hb2, 5, null(), &mut ps) == 0 {
                let f: extern "C" fn() -> u32 = core::mem::transmute(ps);
                f()
            } else {
                0
            };
            check("a DLL loaded again after it was let go starts with fresh shared data", shared == 1);
            if rc == 0 {
                DosFreeModule(hb2);
            }
        }
    }
}

fn timers() {
    unsafe {
        let mut hev = 0;
        DosCreateEventSem(null(), &mut hev, 0, 0);
        let mut f = 0;
        DosTmrQueryFreq(&mut f);
        let mut t0 = 0u64;
        DosTmrQueryTime(&mut t0);
        let mut ht = 0;
        check("DosAsyncTimer", DosAsyncTimer(100, hev, &mut ht) == 0);
        check("timer posts event", DosWaitEventSem(hev, 2000) == 0);
        let mut t1 = 0u64;
        DosTmrQueryTime(&mut t1);
        let ms = (t1 - t0) * 1000 / f as u64;
        check("about 100 ms elapsed", (80..1000).contains(&ms));
        let mut posts = 0;
        DosResetEventSem(hev, &mut posts);
        check("DosStartTimer", DosStartTimer(30, hev, &mut ht) == 0);
        DosSleep(200);
        DosStopTimer(ht);
        DosResetEventSem(hev, &mut posts);
        check("periodic timer posted several times", posts >= 3);
        DosCloseEventSem(hev);
    }
}

/// The performance counters OS/2's Pulse reads (DosConfigurePerf,
/// DosAliasPerfCtrs): one block a processor, named "CPU", "CPU1"...,
/// with the time it was not idle at 0x18.
fn perf_counters() {
    unsafe {
        let rd = |p: *const u8, o: usize| (p.add(o) as *const u32).read_unaligned();
        let mut n = 0;
        DosQuerySysInfo(26, 26, &mut n, 4);
        let mut st = 0;
        check("DosGetProcessorStatus last processor", DosGetProcessorStatus(n, &mut st) == 0 && st == 1);
        check("DosGetProcessorStatus beyond", DosGetProcessorStatus(n + 1, &mut st) == 87);
        check("DosConfigurePerf deconfigure", DosConfigurePerf(0, 4, 0, 0, null(), 0) == 0);
        let (mut p, mut cb) = (null_mut(), 0);
        check("DosAliasPerfCtrs unconfigured", DosAliasPerfCtrs(0xb, 0, &mut p, &mut cb) == 259);
        check("DosConfigurePerf configure", DosConfigurePerf(0x28, 0, 0, 0, b"CPU\0".as_ptr(), 0) == 0);
        // Walk the blocks: text block of each, then its counters.
        let mut handle = 0u32;
        let mut cpus = 0;
        let mut ok = true;
        while DosAliasPerfCtrs(0xb, handle, &mut p, &mut cb) == 0 {
            let id = rd(p, 4) >> 16;
            let name = p.add(rd(p, 0x10) as usize);
            let want = if cpus == 0 { alloc::string::String::from("CPU") } else { alloc::format!("CPU{cpus}") };
            let got = core::slice::from_raw_parts(name, want.len() + 1);
            ok &= rd(p, 8) & 0x50 == 0x50 && id == cpus + 1 && got[..want.len()] == *want.as_bytes() && got[want.len()] == 0;
            ok &= rd(p, 0x28) == 4 && cb as usize > 0x60;
            handle = id << 16;
            let mut cb2 = 0;
            ok &= DosAliasPerfCtrs(0x11, handle, &mut p, &mut cb2) == 0 && cb2 == 0x30 && rd(p, 0) == 0x30;
            cpus += 1;
        }
        check("DosAliasPerfCtrs a block per processor", ok && cpus == n);
        check("DosAliasPerfCtrs bad handle", DosAliasPerfCtrs(0x11, (n + 1) << 16, &mut p, &mut cb) == 539);
        // This thread spins: processor time not idle grows by about as
        // much as the elapsed time, summed over the processors.
        let busy = || {
            let mut sum = 0u64;
            let mut now = 0u64;
            DosTmrQueryTime(&mut now);
            for i in 0..n {
                let (mut d, mut c) = (null_mut(), 0);
                DosAliasPerfCtrs(0x11, (i + 1) << 16, &mut d, &mut c);
                let v = (d.add(0x18) as *const u64).read_volatile();
                sum += if (v as i64) < 0 { v.wrapping_add(now) } else { v };
            }
            (sum, now)
        };
        let (b0, t0) = busy();
        let mut t = t0;
        while t - t0 < 119_318 {
            DosTmrQueryTime(&mut t);
        }
        let (b1, t1) = busy();
        let pct = (b1 - b0) * 100 / (t1 - t0).max(1);
        check("processor not idle while spinning", (80..=100 * n as u64 + 5).contains(&pct));
    }
}

fn shared_memory() {
    unsafe {
        let mut p = null_mut();
        let rc = DosAllocSharedMem(&mut p, b"\\SHAREMEM\\APITEST\0".as_ptr(), 8192, PAG_READ | PAG_WRITE | PAG_COMMIT);
        check("DosAllocSharedMem named", rc == 0 && !p.is_null());
        *p = 0x5a;
        let mut q = null_mut();
        let rc = DosGetNamedSharedMem(&mut q, b"\\SHAREMEM\\APITEST\0".as_ptr(), PAG_READ);
        check("DosGetNamedSharedMem same address", rc == 0 && q == p && *q == 0x5a);
        check("shared memory above private arena", p as u32 >= 0x0400_0000);
        check("DosFreeMem shared", DosFreeMem(p) == 0);
    }
}

fn queues() {
    unsafe {
        let mut hq = 0;
        let rc = DosCreateQueue(&mut hq, QUE_PRIORITY, b"\\QUEUES\\APITEST\0".as_ptr());
        check("DosCreateQueue", rc == 0);
        let mut owner = 0;
        let mut hq2 = 0;
        check("DosOpenQueue", DosOpenQueue(&mut owner, &mut hq2, b"\\QUEUES\\APITEST\0".as_ptr()) == 0);
        DosWriteQueue(hq2, 1, 5, b"low!!".as_ptr(), 1);
        DosWriteQueue(hq2, 2, 5, b"high!".as_ptr(), 9);
        let mut n = 0;
        DosQueryQueue(hq, &mut n);
        check("two elements queued", n == 2);
        let mut req = RequestData::default();
        let mut cb = 0;
        let mut pb: *const u8 = null();
        let mut prio = 0u8;
        let rc = DosReadQueue(hq, &mut req, &mut cb, &mut pb, 0, 0, &mut prio, 0);
        check("priority order", rc == 0 && req.data == 2 && prio == 9 && *pb == b'h');
        DosReadQueue(hq, &mut req, &mut cb, &mut pb, 0, 0, &mut prio, 0);
        let rc = DosReadQueue(hq, &mut req, &mut cb, &mut pb, 0, 1, &mut prio, 0);
        check("empty queue no-wait", rc == 342);
        check("DosCloseQueue", DosCloseQueue(hq) == 0);
    }
}

/// A bound message segment like MSGBIND makes (version 2, one file
/// TEST.MSG with code pages 850 and 437), at the start of a 64K object.
unsafe fn build_msgseg(p: *mut u8) {
    let put = |off: usize, b: &[u8]| unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p.add(off), b.len()) };
    let w = |off: usize, v: u16| put(off, &v.to_le_bytes());
    put(0, b"\xffMSGSEG32\0");
    put(0x0a, &[2, 0x80, 0, 0]);
    put(0x0e, &0x30u32.to_le_bytes());
    w(0x30, 1); // one file
    w(0x32, 0x40); // its name
    w(0x34, 0x100); // its component
    put(0x40, b"TEST.MSG\0");
    // Code page block after the name + 2: next, header, language 9/1, 850 437.
    w(0x4b, 0);
    w(0x4b + 5, 9);
    w(0x4b + 7, 1);
    w(0x4b + 9, 2);
    w(0x4b + 11, 850);
    w(0x4b + 13, 437);
    put(0x100, b"TST");
    w(0x103, 2);
    w(0x105, 0x120);
    w(0x107, 0x140);
    let text = b"Hello %1.\r\n";
    w(0x120, 1);
    w(0x122, text.len() as u16 + 1);
    put(0x124, b"E");
    put(0x125, text);
    w(0x140, 2);
    w(0x142, 1);
    put(0x144, b"?");
}

fn messages() {
    unsafe {
        // DosInsertMessage: %1..%9 up to the count; anything else is kept.
        let a = b"a\0";
        let b = b"bc\0";
        let tbl = [a.as_ptr(), b.as_ptr()];
        let msg = b"%1-%2-%3 %0 %%";
        let mut out = [0u8; 64];
        let mut n = 0;
        let rc = DosInsertMessage(tbl.as_ptr(), 2, msg.as_ptr(), msg.len() as u32, out.as_mut_ptr(), 64, &mut n);
        check("DosInsertMessage %1 %2, %3 %0 kept", rc == 0 && &out[..n as usize] == b"a-bc-%3 %0 %%");
        let rc = DosInsertMessage(tbl.as_ptr(), 10, msg.as_ptr(), msg.len() as u32, out.as_mut_ptr(), 64, &mut n);
        check("DosInsertMessage 10 insertions", rc == 320);
        let long = b"abcdefghij";
        let rc = DosInsertMessage(null(), 0, long.as_ptr(), long.len() as u32, out.as_mut_ptr(), 6, &mut n);
        check("DosInsertMessage truncated ends CR LF", rc == 316 && n == 6 && &out[..6] == b"abcd\r\n");

        // Bound messages are found before (missing) message files.
        let mut seg = null_mut();
        DosAllocMem(&mut seg, 0x10000, PAG_READ | PAG_WRITE | PAG_COMMIT);
        build_msgseg(seg);
        let world = b"world\0";
        let ins = [world.as_ptr()];
        let file = b"TEST.MSG\0";
        let mut buf = [0u8; 128];
        let rc = DosGetMessageSeg(ins.as_ptr(), 1, buf.as_mut_ptr(), 128, 1, file.as_ptr(), &mut n, seg);
        check("DosGetMessage bound message", rc == 0 && &buf[..n as usize] == b"TST0001: Hello world.\r\n");
        let rc = DosGetMessageSeg(null(), 0, buf.as_mut_ptr(), 128, 2, file.as_ptr(), &mut n, seg);
        check("DosGetMessage unused (?) message", rc == 317 && buf[..n as usize].starts_with(b"SYS0317: "));
        let rc = DosGetMessageSeg(null(), 0, buf.as_mut_ptr(), 128, 3, file.as_ptr(), &mut n, seg);
        let want = b"SYS0318: Message file TEST.MSG cannot be found for message  0003.\r\n";
        check("DosGetMessage missing file", rc == 2 && &buf[..n as usize] == want);
        let rc = DosGetMessageSeg(ins.as_ptr(), 1, buf.as_mut_ptr(), 12, 1, file.as_ptr(), &mut n, seg);
        check("DosGetMessage truncated", rc == 316 && n == 12 && &buf[..12] == b"TST0001: H\r\n");
        let mut cps = [0u16; 8];
        let rc = DosIQueryMessageCP(cps.as_mut_ptr().cast(), 16, file.as_ptr(), &mut n, seg);
        check("DosQueryMessageCP bound", rc == 0 && n == 10 && cps[..5] == [2, 850, 437, 9, 1]);
        DosFreeMem(seg);

        // OSO001.MSG and OSO001H.MSG (rs2's own, userland/msg).
        let oso = b"OSO001.MSG\0";
        let rc = DosGetMessage(null(), 0, buf.as_mut_ptr(), 128, 2, oso.as_ptr(), &mut n);
        check("DosGetMessage SYS0002 from OSO001.MSG", rc == 0 && buf[..n as usize].starts_with(b"SYS0002: ") && buf[..n as usize].ends_with(b"\r\n"));
        let rc = DosQueryMessageCP(cps.as_mut_ptr().cast(), 16, oso.as_ptr(), &mut n);
        check("DosQueryMessageCP OSO001.MSG", rc == 0 && n == 10 && cps[..5] == [2, 850, 437, 9, 1]);
        let rc = DosGetMessage(null(), 0, buf.as_mut_ptr(), 128, 0, oso.as_ptr(), &mut n);
        check("DosGetMessage SYS0000 (response letters)", rc == 0 && &buf[..n as usize] == b"Y N A R I\r\n");
        let name = b"XYZZY\0";
        let ins1 = [name.as_ptr()];
        let rc = DosGetMessage(ins1.as_ptr(), 1, buf.as_mut_ptr(), 128, 1041, oso.as_ptr(), &mut n);
        let t = &buf[..n as usize];
        check("DosGetMessage SYS1041 with %1", rc == 0 && t.starts_with(b"SYS1041: ") && t.windows(5).any(|w| w == b"XYZZY"));
        // A prompt has no line end; the last message stops before the
        // file's extended header.
        let rc = DosGetMessage(null(), 0, buf.as_mut_ptr(), 128, 1695, oso.as_ptr(), &mut n);
        check("DosGetMessage SYS1695 prompt", rc == 0 && n > 0 && !buf[..n as usize].ends_with(b"\n"));
        let mut big = [0u8; 2048];
        let rc = DosGetMessage(null(), 0, big.as_mut_ptr(), 2048, 3506, oso.as_ptr(), &mut n);
        check("DosGetMessage SYS3506 (last)", rc == 0 && big[..n as usize].ends_with(b"\r\n"));
        let osoh = b"OSO001H.MSG\0";
        let rc = DosGetMessage(null(), 0, big.as_mut_ptr(), 2048, 2, osoh.as_ptr(), &mut n);
        check("DosGetMessage SYS0002 from OSO001H.MSG", rc == 0 && big[..n as usize].starts_with(b"EXPLANATION: "));

        // DosPutMessage breaks lines at 80 columns between words.
        let (mut r, mut w) = (0, 0);
        DosCreatePipe(&mut r, &mut w, 4096);
        let text = b"word word word word word word word word word word word word word word word word word word word word ";
        let rc = DosPutMessage(w, text.len() as u32, text.as_ptr());
        let mut got = [0u8; 256];
        DosRead(r, got.as_mut_ptr(), 256, &mut n);
        let ok = n as usize == text.len() + 2 && &got[..75] == &text[..75] && &got[75..77] == b"\r\n" && &got[77..n as usize] == &text[75..];
        check("DosPutMessage wraps at 80 columns", rc == 0 && ok);
        DosClose(r);
        DosClose(w);
    }
}

fn nls() {
    unsafe {
        let mut ci = CountryInfo::default();
        let mut n = 0;
        let de = CountryCode { country: 49, codepage: 850 };
        let rc = DosQueryCtryInfo(44, &de, &mut ci, &mut n);
        check(
            "DosQueryCtryInfo Germany",
            rc == 0 && n == 44 && ci.date_fmt == 1 && ci.decimal[0] == b',' && ci.thousands[0] == b'.' && &ci.currency[..3] == b"DM\0",
        );
        let us = CountryCode { country: 1, codepage: 437 };
        let rc = DosQueryCtryInfo(44, &us, &mut ci, &mut n);
        check("DosQueryCtryInfo US", rc == 0 && ci.country == 1 && ci.codepage == 437 && ci.date_fmt == 0 && ci.currency[0] == b'$');
        let bad = CountryCode { country: 999, codepage: 0 };
        check("DosQueryCtryInfo unknown country", DosQueryCtryInfo(44, &bad, &mut ci, &mut n) == 398);

        let mut s = *b"abc\x84\x94\x81z";
        let cc850 = CountryCode { country: 1, codepage: 850 };
        let rc = DosMapCase(s.len() as u32, &cc850, s.as_mut_ptr());
        check("DosMapCase 850", rc == 0 && &s == b"ABC\x8e\x99\x9aZ");
        let mut e = *b"\x82";
        let cc437 = CountryCode { country: 1, codepage: 437 };
        DosMapCase(1, &cc437, e.as_mut_ptr());
        check("DosMapCase 437 e-acute is E", e[0] == b'E');

        let mut t = [0u8; 256];
        let rc = DosQueryCollate(256, &cc850, t.as_mut_ptr(), &mut n);
        check("DosQueryCollate 850", rc == 0 && n == 256 && t[b'a' as usize] == t[b'A' as usize] && t[0x8f] == t[b'A' as usize]);
        let se = CountryCode { country: 46, codepage: 850 };
        DosQueryCollate(256, &se, t.as_mut_ptr(), &mut n);
        check("DosQueryCollate Sweden: A-ring after Z", t[0x8f] > t[b'Z' as usize] && t[0x86] == t[0x8f]);

        let mut d = [0xffu8; 8];
        let jp = CountryCode { country: 81, codepage: 932 };
        let rc = DosQueryDBCSEnv(8, &jp, d.as_mut_ptr());
        check("DosQueryDBCSEnv 932", rc == 0 && d[..6] == [0x81, 0x9f, 0xe0, 0xfc, 0, 0]);
        let mut d = [0xffu8; 4];
        let rc = DosQueryDBCSEnv(4, &cc850, d.as_mut_ptr());
        check("DosQueryDBCSEnv 850 empty", rc == 0 && d[..2] == [0, 0]);

        let mut cps = [0u32; 4];
        let rc = DosQueryCp(16, cps.as_mut_ptr(), &mut n);
        check("DosQueryCp", rc == 0 && n >= 8 && cps[0] == cps[1]);
        let (cur, other) = (cps[0], cps[2]);
        if n >= 12 && other != 0 {
            let rc = DosSetProcessCp(other);
            DosQueryCp(16, cps.as_mut_ptr(), &mut n);
            check("DosSetProcessCp prepared", rc == 0 && cps[0] == other);
            DosSetProcessCp(cur);
        }
        check("DosSetProcessCp not prepared", DosSetProcessCp(1125) == 472);
    }
}

// ---- DosDebug ----

const DBG_C_READ_MEM: i32 = 1;
const DBG_C_READ_REG: i32 = 3;
const DBG_C_WRITE_REG: i32 = 6;
const DBG_C_GO: i32 = 7;
const DBG_C_TERM: i32 = 8;
const DBG_C_SSTEP: i32 = 9;
const DBG_C_STOP: i32 = 10;
const DBG_C_NUM_TO_ADDR: i32 = 13;
const DBG_C_READ_CO_REGS: i32 = 14;
const DBG_C_THRD_STAT: i32 = 17;
const DBG_C_MAP_RO_ALIAS: i32 = 18;
const DBG_C_UNMAP_ALIAS: i32 = 20;
const DBG_C_CONNECT: i32 = 21;
const DBG_C_READ_MEM_BUF: i32 = 22;
const DBG_C_WRITE_MEM_BUF: i32 = 23;
const DBG_C_SET_WATCH: i32 = 24;
const DBG_C_CLEAR_WATCH: i32 = 25;
const DBG_C_CONTINUE: i32 = 27;
const DBG_C_LIN_TO_SEL: i32 = 30;
const DBG_C_SEL_TO_LIN: i32 = 31;
const DBG_N_ERROR: i32 = -1;
const DBG_N_PROC_TERM: i32 = -6;
const DBG_N_EXCEPTION: i32 = -7;
const DBG_N_MODULE_LOAD: i32 = -8;
const DBG_N_WATCHPOINT: i32 = -14;
const DBG_W_LOCAL: u32 = 2;
const DBG_W_WRITE: u32 = 0x2_0000;
const DBG_W_READ_WRITE: u32 = 0x3_0000;
const XCPT_CONTINUE_EXECUTION: u32 = 0xffff_ffff;
const XCPT_CONTINUE_STOP: u32 = 0x0071_6668;
const XCPT_BREAKPOINT: u32 = 0xC000_009F;
const XCPT_SINGLE_STEP: u32 = 0xC000_00A0;

/// The traced child's variable: the debugger reads and rewrites it.
static mut DEBUG_VAR: u32 = 0x1234;
/// Watched by the debugger: the child writes the first, the kernel the
/// second (DosQuerySysInfo's result).
static mut WATCH_VAR: u32 = 0;
static mut KERNEL_WATCH_VAR: u32 = 0;

/// `APITEST dbgchild`: run under the debugger. A breakpoint, two
/// instructions to single step, two writes to watched variables, then the
/// exit code tells what the debugger changed (DEBUG_VAR and EAX).
fn debug_child() -> u32 {
    let eax: u32;
    unsafe {
        core::arch::asm!("int3", "nop", "nop", inout("eax") 0u32 => eax);
        core::ptr::write_volatile(&raw mut WATCH_VAR, 7);
        // QSV_MAX_PATH_LENGTH
        DosQuerySysInfo(1, 1, &raw mut KERNEL_WATCH_VAR, 4);
        DEBUG_VAR + eax
    }
}

fn dbg(b: &mut UDb, cmd: i32) -> i32 {
    b.cmd = cmd;
    let rc = unsafe { DosDebug(b) };
    if rc != 0 { DBG_N_ERROR } else { b.cmd }
}

/// Start `APITEST dbgchild` traced; its pid.
fn start_traced() -> u32 {
    start_traced_with(b"dbgchild")
}

/// Start `APITEST <arg>` traced; its pid.
fn start_traced_with(arg: &[u8]) -> u32 {
    let mut pib = null_mut();
    let mut tib = null_mut();
    let mut path = [0u8; 260];
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        DosQueryModuleName((*pib).hmte, 260, path.as_mut_ptr());
    }
    let mut args = [0u8; 300];
    let n = path.iter().position(|&c| c == 0).unwrap_or(0);
    args[..n].copy_from_slice(&path[..n]);
    args[n + 1..n + 1 + arg.len()].copy_from_slice(arg);
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    // EXEC_TRACE: asynchronous, the child under DosDebug control.
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), 64, 3, args.as_ptr(), null(), &mut res, path.as_ptr()) };
    if rc == 0 { res.code_terminate } else { 0 }
}

/// Written by the spinning thread of `APITEST dbgspin` with its EAX.
static mut SPIN: u32 = 0;

/// Runs user code only, never entering the kernel by itself: stores EAX
/// in SPIN forever.
unsafe extern "C" fn spinner(_: u32) {
    unsafe {
        core::arch::asm!("2:", "mov dword ptr [{p}], eax", "jmp 2b", p = in(reg) &raw mut SPIN, in("eax") 1u32, options(noreturn));
    }
}

/// `APITEST dbgspin`: a thread that spins in user mode, a breakpoint once
/// it runs, then two seconds for the debugger to change the spinner's
/// EAX while the program runs; the exit code is what the spinner stored.
fn debug_spin_child() -> u32 {
    unsafe {
        let mut tid = 0;
        DosCreateThread(&mut tid, spinner, 0, 0, 65536);
        while core::ptr::read_volatile(&raw const SPIN) == 0 {
            DosSleep(10);
        }
        core::arch::asm!("int3");
        DosSleep(2000);
        core::ptr::read_volatile(&raw const SPIN)
    }
}

static SPIN_END: AtomicU32 = AtomicU32::new(0);
static SPIN_CODE: AtomicU32 = AtomicU32::new(0);

/// Lets the debuggee of `pid` run to its end (DBG_C_Go), as a debugger's
/// second thread.
unsafe extern "C" fn spin_runner(pid: u32) {
    let mut b = UDb { pid, ..Default::default() };
    let mut n = dbg(&mut b, DBG_C_GO);
    let mut guard = 0;
    while n != DBG_N_PROC_TERM && n != DBG_N_ERROR && guard < 50 {
        b.value = XCPT_CONTINUE_EXECUTION;
        n = dbg(&mut b, DBG_C_CONTINUE);
        guard += 1;
    }
    SPIN_CODE.store(b.value, Ordering::Relaxed);
    SPIN_END.store(n as u32, Ordering::Relaxed);
}

/// DBG_C_ReadReg/WriteReg on a thread that runs user code on another CPU
/// (it never enters the kernel by itself to be held): while the debuggee
/// is stopped and while another thread of the debugger lets it run, the
/// thread is stopped for the access, so the registers read are its own
/// and the ones written are the ones it goes on with.
fn debug_running_thread() {
    let pid = start_traced_with(b"dbgspin");
    if pid == 0 {
        check("DosExecPgm EXEC_TRACE (dbgspin)", false);
        return;
    }
    let mut b = UDb { pid, value: 1, ..Default::default() };
    if dbg(&mut b, DBG_C_CONNECT) != 0 {
        check("DBG_C_Connect (dbgspin)", false);
        return;
    }
    let mut n;
    let mut guard = 0;
    loop {
        b.tid = 0;
        n = dbg(&mut b, DBG_C_GO);
        guard += 1;
        if n == DBG_N_EXCEPTION || n == DBG_N_PROC_TERM || n == DBG_N_ERROR || guard > 50 {
            break;
        }
    }
    let bp = b.addr;
    check("spinning thread: the breakpoint", n == DBG_N_EXCEPTION && b.buffer == XCPT_BREAKPOINT && b.tid == 1);
    let code = spinner as usize as u32;
    let in_spinner = |b: &UDb| b.eip >= code && b.eip < code + 64;
    let mut s = UDb { pid, tid: 2, ..Default::default() };
    let ok = dbg(&mut s, DBG_C_READ_REG) == 0 && in_spinner(&s) && s.eax == 1;
    unsafe { DosSleep(50) };
    let mut s2 = UDb { pid, tid: 2, ..Default::default() };
    let ok2 = dbg(&mut s2, DBG_C_READ_REG) == 0 && s2.eip == s.eip && s2.eax == 1;
    check("DBG_C_ReadReg of a spinning thread while stopped", ok && ok2);
    // Past the INT 3, then let it run from another thread.
    b.tid = 1;
    dbg(&mut b, DBG_C_READ_REG);
    b.eip = bp + 1;
    dbg(&mut b, DBG_C_WRITE_REG);
    b.value = XCPT_CONTINUE_STOP;
    dbg(&mut b, DBG_C_CONTINUE);
    SPIN_END.store(0, Ordering::Relaxed);
    let mut runner = 0;
    unsafe { DosCreateThread(&mut runner, spin_runner, pid, 0, 65536) };
    unsafe { DosSleep(300) };
    let mut s = UDb { pid, tid: 2, ..Default::default() };
    let read = dbg(&mut s, DBG_C_READ_REG) == 0 && in_spinner(&s) && s.eax == 1;
    check("DBG_C_ReadReg of a thread running on another CPU", read);
    s.eax = 0x5a5a;
    let wrote = dbg(&mut s, DBG_C_WRITE_REG) == 0;
    // The spinner goes on with the new EAX.
    let (mut seen, mut tries) = (0u32, 0);
    while tries < 100 {
        let mut m = UDb { pid, addr: &raw mut SPIN as u32, buffer: &raw mut seen as u32, len: 4, ..Default::default() };
        if dbg(&mut m, DBG_C_READ_MEM_BUF) == 0 && seen == 0x5a5a {
            break;
        }
        unsafe { DosSleep(10) };
        tries += 1;
    }
    check("DBG_C_WriteReg of a thread running on another CPU takes effect", wrote && seen == 0x5a5a);
    let mut t = runner;
    unsafe { DosWaitThread(&mut t, 0) };
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut got = 0;
    let rc = unsafe { DosWaitChild(0, 0, &mut res, &mut got, pid) };
    let end = SPIN_END.load(Ordering::Relaxed) as i32;
    check("spinning debuggee ends with the EAX the debugger wrote", end == DBG_N_PROC_TERM && SPIN_CODE.load(Ordering::Relaxed) == 0x5a5a && rc == 0 && res.code_result == 0x5a5a);
    b.pid = pid;
    dbg(&mut b, DBG_C_TERM);
}

fn debugging() {
    debug_running_thread();
    let pid = start_traced();
    check("DosExecPgm EXEC_TRACE", pid != 0);
    if pid == 0 {
        return;
    }
    let mut b = UDb { pid, value: 1, ..Default::default() };
    let n = dbg(&mut b, DBG_C_CONNECT);
    check("DBG_C_Connect", n == 0 && b.segs[0].sel == 0x5b && b.eip != 0);
    let entry = b.eip;
    b = UDb { pid, ..Default::default() };
    let n = dbg(&mut b, DBG_C_STOP);
    let mut me = null_mut();
    let mut pib = null_mut();
    unsafe { DosGetInfoBlocks(&mut me, &mut pib) };
    // The child's program module (its own handle) has the same file.
    let (mut name, mut mine) = ([0u8; 260], [0u8; 260]);
    let named = unsafe { DosQueryModuleName(b.value, 260, name.as_mut_ptr()) == 0 && DosQueryModuleName((*pib).hmte, 260, mine.as_mut_ptr()) == 0 };
    check("DBG_C_Stop reports the program's module", n == DBG_N_MODULE_LOAD && named && name == mine);
    let hmte = b.value;
    b.value = XCPT_CONTINUE_STOP;
    check("DBG_C_Continue XCPT_CONTINUE_STOP", dbg(&mut b, DBG_C_CONTINUE) == 0);
    // Stopped before its first instruction.
    b.tid = 1;
    check("stopped at the entry", dbg(&mut b, DBG_C_READ_REG) == 0 && b.eip == entry);
    b.mte = hmte;
    b.value = 1;
    let f = debug_child as usize as u32;
    check("DBG_C_NumToAddr", dbg(&mut b, DBG_C_NUM_TO_ADDR) == 0 && b.addr != 0 && b.addr <= f && f - b.addr < 0x10_0000);
    // Run to the breakpoint (module loads on the way).
    let mut n;
    let mut guard = 0;
    loop {
        b.tid = 0;
        n = dbg(&mut b, DBG_C_GO);
        guard += 1;
        if n != DBG_N_MODULE_LOAD || guard > 50 {
            break;
        }
    }
    let bp = b.addr;
    check("breakpoint: pre-first-chance XCPT_BREAKPOINT", n == DBG_N_EXCEPTION && b.value == 0 && b.buffer == XCPT_BREAKPOINT);
    check("breakpoint: EIP at the INT 3", b.eip == bp && b.tid == 1);
    b.value = XCPT_CONTINUE_STOP;
    dbg(&mut b, DBG_C_CONTINUE);
    let mut byte = 0u8;
    b.addr = bp;
    b.buffer = &raw mut byte as u32;
    b.len = 1;
    check("DBG_C_ReadMemBuf code", dbg(&mut b, DBG_C_READ_MEM_BUF) == 0 && byte == 0xcc);
    // Memory: the child's copy of DEBUG_VAR.
    let var = &raw mut DEBUG_VAR as u32;
    let mut v = 0u32;
    b.addr = var;
    b.buffer = &raw mut v as u32;
    b.len = 4;
    check("DBG_C_ReadMemBuf data", dbg(&mut b, DBG_C_READ_MEM_BUF) == 0 && v == 0x1234);
    v = 0x5600;
    check("DBG_C_WriteMemBuf", dbg(&mut b, DBG_C_WRITE_MEM_BUF) == 0);
    b.addr = var;
    check("DBG_C_ReadMem (word)", dbg(&mut b, DBG_C_READ_MEM) == 0 && b.value == 0x5600);
    // An alias of the page in this process.
    b.addr = var;
    b.len = 4;
    let ok = dbg(&mut b, DBG_C_MAP_RO_ALIAS) == 0 && b.buffer != 0 && unsafe { *(b.buffer as *const u32) } == 0x5600;
    check("DBG_C_MapROAlias", ok && dbg(&mut b, DBG_C_UNMAP_ALIAS) == 0);
    // Registers: past the INT 3, EAX = 0x78.
    b.tid = 1;
    dbg(&mut b, DBG_C_READ_REG);
    b.eip = bp + 1;
    b.eax = 0x78;
    check("DBG_C_WriteReg", dbg(&mut b, DBG_C_WRITE_REG) == 0);
    let mut bad = b;
    bad.segs[0].sel = 0x53;
    check("DBG_C_WriteReg bad CS fails", dbg(&mut bad, DBG_C_WRITE_REG) == DBG_N_ERROR);
    let mut fpu = [0u8; 108];
    b.buffer = fpu.as_mut_ptr() as u32;
    b.len = 108;
    b.value = 1;
    b.index = 0;
    check("DBG_C_ReadCoRegs", dbg(&mut b, DBG_C_READ_CO_REGS) == 0 && fpu[0] & 0x3f == 0x3f);
    let mut ts = [0u8; 4];
    b.buffer = ts.as_mut_ptr() as u32;
    b.len = 4;
    check("DBG_C_ThrdStat", dbg(&mut b, DBG_C_THRD_STAT) == 0 && ts[0] == 0 && ts[1] == 0);
    b.tid = 9;
    check("DBG_C_ThrdStat no thread", dbg(&mut b, DBG_C_THRD_STAT) == DBG_N_ERROR);
    // Selectors.
    b.addr = var;
    let ok = dbg(&mut b, DBG_C_LIN_TO_SEL) == 0 && b.index == var & 0xffff;
    check("DBG_C_LinToSel/SelToLin", ok && dbg(&mut b, DBG_C_SEL_TO_LIN) == 0 && b.addr == var);
    // Watchpoints: IBM's handles are 8 + the debug register.
    let watch = |b: &mut UDb, addr: u32, len: u32, value: u32| {
        b.addr = addr;
        b.len = len;
        b.value = value;
        b.index = 0;
        if dbg(b, DBG_C_SET_WATCH) == 0 { b.index } else { 0 }
    };
    let wvar = &raw mut WATCH_VAR as u32;
    let kvar = &raw mut KERNEL_WATCH_VAR as u32;
    let w1 = watch(&mut b, wvar, 4, DBG_W_LOCAL | DBG_W_WRITE);
    let w2 = watch(&mut b, kvar, 4, DBG_W_LOCAL | DBG_W_WRITE);
    let w3 = watch(&mut b, var, 2, DBG_W_LOCAL | DBG_W_READ_WRITE);
    check("DBG_C_SetWatch", w1 == 8 && w2 == 9 && w3 == 10);
    let bad = [watch(&mut b, var, 3, DBG_W_LOCAL | DBG_W_WRITE), watch(&mut b, var + 1, 2, DBG_W_LOCAL | DBG_W_WRITE)];
    check("DBG_C_SetWatch bad length or alignment fails", bad == [0, 0] && b.cmd == DBG_N_ERROR);
    b.index = w3;
    check("DBG_C_ClearWatch", dbg(&mut b, DBG_C_CLEAR_WATCH) == 0 && dbg(&mut b, DBG_C_CLEAR_WATCH) == DBG_N_ERROR);
    // One instruction (the first NOP).
    b.tid = 1;
    let n = dbg(&mut b, DBG_C_SSTEP);
    check("single step", n == DBG_N_EXCEPTION && b.value == 0 && b.buffer == XCPT_SINGLE_STEP && b.addr == bp + 2);
    check("single step registers", b.eip == bp + 2 && b.eax == 0x78 && b.eflags & 0x100 == 0);
    b.value = XCPT_CONTINUE_EXECUTION;
    let n = dbg(&mut b, DBG_C_CONTINUE);
    let mut seen = 0u32;
    b.addr = wvar;
    b.buffer = &raw mut seen as u32;
    b.len = 4;
    let ok = n == DBG_N_WATCHPOINT && b.index == w1 && b.tid == 1 && dbg(&mut b, DBG_C_READ_MEM_BUF) == 0 && seen == 7;
    check("DBG_N_Watchpoint after the program's write", ok);
    let n = dbg(&mut b, DBG_C_GO);
    b.addr = kvar;
    b.len = 4;
    let ok = n == DBG_N_WATCHPOINT && b.index == w2 && dbg(&mut b, DBG_C_READ_MEM_BUF) == 0 && seen == 260;
    check("DBG_N_Watchpoint after the kernel's write", ok);
    b.index = w1;
    check("a watchpoint that fired is gone", dbg(&mut b, DBG_C_CLEAR_WATCH) == DBG_N_ERROR);
    let mut guard = 0;
    let mut n = dbg(&mut b, DBG_C_GO);
    while n != DBG_N_PROC_TERM && n != DBG_N_ERROR && guard < 50 {
        b.value = XCPT_CONTINUE_EXECUTION;
        n = dbg(&mut b, DBG_C_CONTINUE);
        guard += 1;
    }
    check("runs to the end: DBG_N_ProcTerm", n == DBG_N_PROC_TERM && b.value == 0x5678);
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut got = 0;
    let rc = unsafe { DosWaitChild(0, 0, &mut res, &mut got, pid) };
    check("traced child's result", rc == 0 && res.code_result == 0x5678);
    b.pid = pid;
    dbg(&mut b, DBG_C_TERM);
    check("DosDebug after the end fails", dbg(&mut b, DBG_C_READ_REG) == DBG_N_ERROR);

    // DBG_C_Term ends a debuggee.
    let pid = start_traced();
    let mut b = UDb { pid, value: 1, ..Default::default() };
    let connected = dbg(&mut b, DBG_C_CONNECT) == 0;
    check("DBG_C_Term", connected && dbg(&mut b, DBG_C_TERM) == 0);
    let rc = unsafe { DosWaitChild(0, 0, &mut res, &mut got, pid) };
    check("terminated child", rc == 0 && res.code_terminate == 3);
}
