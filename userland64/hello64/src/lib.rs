//! HELLO64.EXE: a 64-bit OS/2 text program, the smoke test of rs2's
//! 64-bit API (docs/API64.md). It prints what it can find out about
//! itself and exercises memory, threads, semaphores and files, printing
//! "ok" or the failing step, and exits with 0 when all passed.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

static SUM: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn worker(param: usize) {
    // The parameter keeps all 64 bits.
    if param == 0x1234_5678_9abc_def0 {
        SUM.fetch_add(1, Ordering::SeqCst);
    }
}

fn check(name: &str, ok: bool, failed: &mut u32) {
    println!("  {name}: {}", if ok { "ok" } else { "FAILED" });
    if !ok {
        *failed += 1;
    }
}

fn main(s: &Startup) -> u32 {
    let mut failed = 0;
    println!("Hello from 64-bit OS/2!");
    let here = main as *const () as usize;
    let mut tib: *mut Tib = core::ptr::null_mut();
    let mut pib: *mut Pib = core::ptr::null_mut();
    unsafe { DosGetInfoBlocks(&mut tib, &mut pib) };
    let (pid, tid, prog) = unsafe {
        let p = &*pib;
        ((*p).pid, (*(*tib).ptib2).tid, String::from_utf8_lossy(cstr(p.pchcmd)).into_owned())
    };
    println!("  pid {pid}, tid {tid}, program {prog}, code at {here:#x}, rsp near {:#x}", &failed as *const _ as usize);
    println!("  arguments: \"{}\"", String::from_utf8_lossy(s.args()));
    check("TIB/PIB", pid != 0 && tid == 1 && s.cmdline == unsafe { (*pib).pchcmd }, &mut failed);

    // DosQuerySysInfo.
    let mut v = [0u32; 2];
    let rc = unsafe { DosQuerySysInfo(QSV_VERSION_MAJOR, QSV_VERSION_MINOR, v.as_mut_ptr(), 8) };
    println!("  OS/2 version {}.{}", v[0], v[1]);
    check("DosQuerySysInfo", rc == 0 && v[0] == 20, &mut failed);

    // The heap (DosAllocMem through the pointer-widening path).
    let mut big: Vec<u64> = Vec::new();
    for i in 0..100_000u64 {
        big.push(i * 3);
    }
    let sum: u64 = big.iter().sum();
    check("heap", sum == 3 * (99_999 * 100_000 / 2), &mut failed);
    let mut p: *mut u8 = core::ptr::null_mut();
    let rc = unsafe { DosAllocMem(&mut p, 65536, PAG_READ | PAG_WRITE | PAG_COMMIT) };
    let ok = rc == 0 && !p.is_null() && unsafe {
        p.add(65535).write(7);
        p.add(65535).read() == 7
    };
    check("DosAllocMem", ok && unsafe { DosFreeMem(p) } == 0, &mut failed);

    // The environment.
    let mut val: *const u8 = core::ptr::null();
    let rc = unsafe { DosScanEnv(c"PATH".as_ptr().cast(), &mut val) };
    check("DosScanEnv", rc == 0 && !val.is_null() && unsafe { !cstr(val).is_empty() }, &mut failed);

    // Threads and an event semaphore.
    let mut tids = [0u32; 4];
    for t in &mut tids {
        let rc = unsafe { DosCreateThread(t, worker, 0x1234_5678_9abc_def0, 0, 65536) };
        if rc != 0 {
            println!("  DosCreateThread: {rc}");
        }
    }
    for t in &mut tids {
        unsafe { DosWaitThread(t, DCWW_WAIT) };
    }
    check("threads", SUM.load(Ordering::SeqCst) == 4, &mut failed);
    let mut hev = 0;
    let ok = unsafe {
        DosCreateEventSem(core::ptr::null(), &mut hev, 0, 0) == 0
            && DosPostEventSem(hev) == 0
            && DosWaitEventSem(hev, 1000) == 0
            && DosCloseEventSem(hev) == 0
    };
    check("event semaphore", ok, &mut failed);

    // A file.
    let name = c"HELLO64.TMP".as_ptr().cast();
    let mut hf = 0;
    let mut action = 0;
    let text = b"64-bit file data";
    let mut n = 0;
    let wrote = unsafe {
        DosOpen(name, &mut hf, &mut action, 0, FILE_NORMAL, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, core::ptr::null_mut()) == 0
            && DosWrite(hf, text.as_ptr(), text.len() as u32, &mut n) == 0
            && DosClose(hf) == 0
    };
    let mut back = [0u8; 32];
    let read = unsafe {
        DosOpen(name, &mut hf, &mut action, 0, FILE_NORMAL, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, core::ptr::null_mut()) == 0
            && DosRead(hf, back.as_mut_ptr(), 32, &mut n) == 0
            && DosClose(hf) == 0
    };
    unsafe { DosDelete(name) };
    check("file", wrote && read && &back[..n as usize] == text, &mut failed);

    // A DLL: DOSCALLS' own handle, and a procedure address.
    let mut hmod = 0;
    let mut pfn = 0usize;
    let ok = unsafe {
        DosQueryModuleHandle(c"DOSCALLS".as_ptr().cast(), &mut hmod) == 0
            && DosQueryProcAddr(hmod, 229, core::ptr::null(), &mut pfn) == 0
    };
    check("DosQueryProcAddr", ok && pfn != 0, &mut failed);
    // Calling the gate stub DosQueryProcAddr returned: DosSleep(1).
    let sleep: unsafe extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(pfn) };
    check("call through a procedure address", unsafe { sleep(1) } == 0, &mut failed);

    if failed == 0 {
        println!("HELLO64: all tests passed");
    } else {
        println!("HELLO64: {failed} tests failed");
    }
    failed
}
