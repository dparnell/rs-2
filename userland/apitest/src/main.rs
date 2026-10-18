//! Kernel API regression test. Run it on rs2 (or real OS/2 for
//! comparison); every check prints PASS or FAIL, and the exit code is the
//! number of failures.

#![no_std]
#![no_main]

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

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
    info_blocks();
    memory();
    suballoc();
    files();
    threads_and_sems();
    exceptions();
    pipes();
    named_pipes();
    dlls();
    timers();
    shared_memory();
    queues();
    sessions();
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
}

fn dlls() {
    unsafe {
        check("import by ordinal", TestAdd(40, 2) == 42);
        check("DLL init ran once", TestInitCount() == 1);
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
        check("DosFreeModule", DosFreeModule(h) == 0);
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
