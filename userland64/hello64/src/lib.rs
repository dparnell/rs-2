//! HELLO64.EXE: a 64-bit OS/2 text program, the smoke test of rs2's
//! 64-bit API (docs/API64.md). It prints what it can find out about
//! itself and exercises memory, threads, semaphores and files, printing
//! "ok" or the failing step, and exits with 0 when all passed. The last
//! part checks memory above 4 GB (DosAllocMem with OBJ_ANY): it commits
//! and touches 1 GB there, so give the system 4 GB (`--mem 4G`).

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

    high_memory(&mut failed);

    if failed == 0 {
        println!("HELLO64: all tests passed");
    } else {
        println!("HELLO64: {failed} tests failed");
    }
    failed
}

const GB: usize = 1 << 30;
const ERROR_INVALID_ADDRESS: u32 = 487;

/// DosQueryMem: (size, flags) of the run of pages at `p`, up to `max`.
fn query(p: *mut u8, max: u32) -> (u32, u32, u32) {
    let (mut cb, mut fl) = (max, 0);
    let rc = unsafe { DosQueryMem(p, &mut cb, &mut fl) };
    (rc, cb, fl)
}

/// Memory above 4 GB: a 2 GB object, 1 GB of it committed and touched,
/// a guard page, DosWrite and DosRead with buffers there, a call that
/// refuses such buffers, the heap growing there, DosFreeMem.
fn high_memory(failed: &mut u32) {
    let mut p: *mut u8 = core::ptr::null_mut();
    let rc = unsafe { DosAllocMem(&mut p, 2 * GB as u32, PAG_READ | PAG_WRITE | OBJ_ANY) };
    println!("  2 GB with OBJ_ANY at {:#x} (rc {rc})", p as usize);
    check("DosAllocMem above 4 GB", rc == 0 && p as usize >= 1 << 32, failed);
    if rc != 0 {
        return;
    }
    // Reserved only: nothing committed yet.
    let (rc, cb, fl) = query(p, 2 * GB as u32);
    check("DosQueryMem (reserved)", rc == 0 && cb == 2 * GB as u32 && fl & PAG_COMMIT == 0 && fl & PAG_BASE != 0, failed);

    // Commit 1 GB and touch every page of it.
    let rc = unsafe { DosSetMem(p, GB as u32, PAG_COMMIT | PAG_READ | PAG_WRITE) };
    check("DosSetMem commit 1 GB", rc == 0, failed);
    let pages = GB / 4096;
    for i in 0..pages {
        unsafe { (p.add(i * 4096) as *mut u64).write_volatile((i as u64) ^ 0x5a5a_0000_0000) };
    }
    let mut bad = 0;
    for i in 0..pages {
        if unsafe { (p.add(i * 4096) as *const u64).read_volatile() } != (i as u64) ^ 0x5a5a_0000_0000 {
            bad += 1;
        }
    }
    check("touch 1 GB above 4 GB", bad == 0, failed);
    let (rc, cb, fl) = query(p, 2 * GB as u32);
    check(
        "DosQueryMem (committed)",
        rc == 0 && cb == GB as u32 && fl & (PAG_COMMIT | PAG_READ | PAG_WRITE | PAG_BASE) == PAG_COMMIT | PAG_READ | PAG_WRITE | PAG_BASE,
        failed,
    );

    // Touching an uncommitted page would end the program; DosQueryMem
    // says it is reserved.
    let (rc, _, fl) = query(unsafe { p.add(GB) }, 4096);
    check("DosQueryMem (reserved half)", rc == 0 && fl & PAG_COMMIT == 0, failed);

    // A guard page: the first touch commits it, the page below becomes
    // the guard page.
    let g = unsafe { p.add(GB + 64 * 4096) };
    let rc = unsafe { DosSetMem(g, 4096, PAG_COMMIT | PAG_GUARD | PAG_READ | PAG_WRITE) };
    let (_, _, before) = query(g, 4096);
    unsafe { g.write_volatile(42) };
    let (_, _, after) = query(g, 4096);
    let (_, _, below) = query(unsafe { g.sub(4096) }, 4096);
    check(
        "guard page above 4 GB",
        rc == 0 && before & PAG_GUARD != 0 && after & PAG_GUARD == 0 && after & PAG_COMMIT != 0 && below & PAG_GUARD != 0 && unsafe { g.read() } == 42,
        failed,
    );

    // DosWrite to the screen from above 4 GB, the count written there too.
    let msg = b"  DosWrite from above 4 GB: ok\r\n";
    let buf = unsafe { p.add(GB / 2 + 123) };
    let pn = unsafe { p.add(GB / 2 + 8192) } as *mut u32;
    unsafe { core::ptr::copy_nonoverlapping(msg.as_ptr(), buf, msg.len()) };
    let rc = unsafe { DosWrite(1, buf, msg.len() as u32, pn) };
    if rc != 0 || unsafe { *pn } != msg.len() as u32 {
        check("DosWrite from above 4 GB", false, failed);
    }

    // 1 MB to a file and back, both buffers above 4 GB.
    let (src, dst) = unsafe { (p.add(100 << 20), p.add(300 << 20)) };
    for i in 0..1 << 20 {
        unsafe { src.add(i).write((i * 7 % 251) as u8) };
    }
    let name = c"HELLO64.BIG".as_ptr().cast();
    let (mut hf, mut action, mut n, mut m) = (0, 0, 0, 0);
    let ok = unsafe {
        DosOpen(name, &mut hf, &mut action, 0, FILE_NORMAL, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, core::ptr::null_mut()) == 0
            && DosWrite(hf, src, 1 << 20, &mut n) == 0
            && DosSetFilePtr(hf, 0, FILE_BEGIN, &mut m) == 0
            && DosRead(hf, dst, 1 << 20, &mut m) == 0
            && DosClose(hf) == 0
    };
    unsafe { DosDelete(name) };
    let same = unsafe { core::slice::from_raw_parts(src, 1 << 20) == core::slice::from_raw_parts(dst, 1 << 20) };
    check("DosWrite/DosRead 1 MB above 4 GB", ok && n == 1 << 20 && m == 1 << 20 && same, failed);

    // A call not written for memory above 4 GB refuses it (no truncation).
    let mut cb = 64u32;
    let rc = unsafe { DosQueryCurrentDir(0, buf, &mut cb) };
    check("other calls refuse buffers above 4 GB", rc == ERROR_INVALID_ADDRESS, failed);

    // Decommit, then free.
    let rc = unsafe { DosSetMem(p, GB as u32, PAG_DECOMMIT) };
    let (_, _, fl) = query(p, 4096);
    check("DosSetMem decommit", rc == 0 && fl & PAG_COMMIT == 0, failed);
    let rc = unsafe { DosFreeMem(p) };
    let (qrc, _, fl) = query(p, 4096);
    check("DosFreeMem above 4 GB", rc == 0 && qrc == 0 && fl & PAG_FREE != 0, failed);

    // The heap: 600 MB does not fit below 512 MB, so it comes from above
    // 4 GB.
    let mut v: Vec<u8> = Vec::new();
    let ok = v.try_reserve_exact(600 << 20).is_ok();
    let at = v.as_ptr() as usize;
    if ok {
        v.resize(600 << 20, 1);
    }
    let sum: usize = v.iter().step_by(1 << 20).map(|&b| b as usize).sum();
    println!("  600 MB heap block at {at:#x}");
    check("heap above 4 GB", ok && at >= 1 << 32 && sum == 600, failed);
}
