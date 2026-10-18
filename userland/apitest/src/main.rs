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
    files();
    threads_and_sems();
    exceptions();
    pipes();
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

fn files() {
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

unsafe extern "C" fn on_exit(code: u32) {
    println!("exit list routine ran (termination code {code})");
    unsafe { DosExitList(EXLST_EXIT, on_exit) };
}

fn exit_list() {
    let rc = unsafe { DosExitList(EXLST_ADD | 0x100, on_exit) };
    check("DosExitList add", rc == 0);
}
