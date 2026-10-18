//! More cross-process and resource-limit cases: many semaphores and
//! timers, a named pipe whose server dies under a blocked client, a child
//! killed while writing a pipe its parent reads, threads loading and
//! freeing a DLL at once, threads writing one file at different offsets,
//! DosWaitChild for a process tree.

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, counters, join, ms, start_child, thread, wait_child};

static H: AtomicU32 = AtomicU32::new(0);
static OK: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn dll_user(n: u32) {
    let mut good = true;
    for _ in 0..10 {
        let mut h = 0;
        let rc = unsafe { DosLoadModule(null_mut(), 0, b"TESTDLL\0".as_ptr(), &mut h) };
        if rc != 0 {
            good = false;
            break;
        }
        let mut pfn = 0usize;
        good &= unsafe { DosQueryProcAddr(h, 1, null(), &mut pfn) } == 0 && pfn != 0;
        good &= unsafe { DosFreeModule(h) } == 0;
    }
    if good {
        OK.fetch_add(1, Ordering::Relaxed);
    }
    let _ = n;
}

unsafe extern "C" fn file_writer(n: u32) {
    let h = H.load(Ordering::Relaxed);
    let block = [n as u8; 512];
    let mut good = true;
    for i in 0..32u32 {
        // Each thread owns every 8th 512-byte block.
        let off = (i * 8 + n) * 512;
        let mut pos = 0;
        // DosSetFilePtr and DosWrite share the handle's position: use a
        // critical section to keep the pair together.
        unsafe {
            DosEnterCritSec();
            DosSetFilePtr(h, off as i32, FILE_BEGIN, &mut pos);
            let mut w = 0;
            good &= DosWrite(h, block.as_ptr(), 512, &mut w) == 0 && w == 512;
            DosExitCritSec();
        }
    }
    if good {
        OK.fetch_add(1, Ordering::Relaxed);
    }
}

/// Child: write to the pipe handle in args forever.
pub fn child_flood(args: &[alloc::string::String]) -> u32 {
    let h: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let buf = [0x55u8; 1024];
    loop {
        let mut w = 0;
        if unsafe { DosWrite(h, buf.as_ptr(), 1024, &mut w) } != 0 {
            return 1;
        }
    }
}

/// Child: be a named pipe server that dies after connecting.
pub fn child_npserver() -> u32 {
    let mut h = 0;
    unsafe {
        DosCreateNPipe(b"\\PIPE\\STRESS\\DIE\0".as_ptr(), &mut h, NP_ACCESS_DUPLEX, 1, 512, 512, 0);
        DosConnectNPipe(h);
        DosSleep(300);
    }
    0
}

pub fn run() {
    let before = counters();
    unsafe {
        // Many semaphores at once.
        let mut hs = alloc::vec::Vec::new();
        let mut ok = true;
        for i in 0..3000 {
            let mut h = 0;
            let rc = if i % 2 == 0 { DosCreateEventSem(null(), &mut h, 0, 0) } else { DosCreateMutexSem(null(), &mut h, 0, 0) };
            if rc != 0 {
                ok = false;
                break;
            }
            hs.push((h, i % 2));
        }
        for (h, kind) in hs.drain(..) {
            ok &= if kind == 0 { DosCloseEventSem(h) } else { DosCloseMutexSem(h) } == 0;
        }
        check("3000 semaphores created and closed", ok && counters().sems32 == before.sems32);

        // Many timers, then a process that ends with timers running.
        let mut ev = 0;
        DosCreateEventSem(null(), &mut ev, 0, 0);
        let mut ts = alloc::vec::Vec::new();
        for i in 0..200 {
            let mut t = 0;
            if DosStartTimer(5 + i % 20, ev, &mut t) == 0 {
                ts.push(t);
            }
        }
        DosSleep(100);
        let mut posts = 0;
        DosQueryEventSem(ev, &mut posts);
        let stopped = ts.iter().all(|&t| DosStopTimer(t) == 0);
        check("200 periodic timers post and stop", ts.len() == 200 && posts > 0 && stopped && counters().timers == before.timers);
        DosCloseEventSem(ev);

        // A named pipe server that dies while its client waits in a read.
        let pid = start_child("npserver");
        let mut rc = 1;
        for _ in 0..50 {
            rc = DosWaitNPipe(b"\\PIPE\\STRESS\\DIE\0".as_ptr(), 100);
            if rc == 0 {
                break;
            }
            DosSleep(20);
        }
        let (mut hc, mut act) = (0, 0);
        let orc = DosOpen(b"\\PIPE\\STRESS\\DIE\0".as_ptr(), &mut hc, &mut act, 0, 0, 1, 0x42, null_mut());
        let mut buf = [0u8; 16];
        let mut n = 0;
        let t0 = ms();
        let rrc = DosRead(hc, buf.as_mut_ptr(), 16, &mut n);
        let el = ms() - t0;
        wait_child(pid);
        DosClose(hc);
        check("client read ends when the pipe server dies", rc == 0 && orc == 0 && n == 0 && el < 3000 && (rrc == 0 || rrc == 109 || rrc == 233));

        // A child killed while flooding a pipe its parent reads.
        let (mut r, mut w) = (0, 0);
        DosCreatePipe(&mut r, &mut w, 4096);
        let pid = start_child(&alloc::format!("flood {w}"));
        DosClose(w);
        let mut total = 0u32;
        let mut buf = [0u8; 4096];
        while total < 256 * 1024 {
            let mut n = 0;
            if DosRead(r, buf.as_mut_ptr(), 4096, &mut n) != 0 || n == 0 {
                break;
            }
            total += n;
        }
        let krc = DosKillProcess(0, pid);
        // After the writer is gone the read reaches end of file.
        let mut eof = false;
        for _ in 0..1000 {
            let mut n = 0;
            let rc = DosRead(r, buf.as_mut_ptr(), 4096, &mut n);
            if rc != 0 || n == 0 {
                eof = rc == 0;
                break;
            }
        }
        let (_, cause, _, _) = wait_child(pid);
        DosClose(r);
        check("child killed while writing a pipe; reader sees end of file", total >= 256 * 1024 && krc == 0 && cause == 3 && eof);

        // Threads loading and freeing a DLL at once.
        OK.store(0, Ordering::Relaxed);
        let ts: alloc::vec::Vec<u32> = (0..6).map(|n| thread(dll_user, n)).collect();
        for t in ts {
            join(t);
        }
        check("6 threads x 10 DosLoadModule/DosFreeModule", OK.load(Ordering::Relaxed) == 6);

        // Threads writing one file at different offsets.
        let (mut h, mut act) = (0, 0);
        DosOpen(b"C:\\STRESSMT.TMP\0".as_ptr(), &mut h, &mut act, 0, 0, 0x12, 0x42, null_mut());
        H.store(h, Ordering::Relaxed);
        OK.store(0, Ordering::Relaxed);
        let ts: alloc::vec::Vec<u32> = (0..8).map(|n| thread(file_writer, n)).collect();
        for t in ts {
            join(t);
        }
        let mut good = OK.load(Ordering::Relaxed) == 8;
        let mut pos = 0;
        DosSetFilePtr(h, 0, FILE_BEGIN, &mut pos);
        let mut blk = [0u8; 512];
        for i in 0..256u32 {
            let mut n = 0;
            DosRead(h, blk.as_mut_ptr(), 512, &mut n);
            good &= n == 512 && blk.iter().all(|&b| b as u32 == i % 8);
        }
        DosClose(h);
        DosDelete(b"C:\\STRESSMT.TMP\0".as_ptr());
        check("8 threads writing one file", good);

        // DosWaitChild with DCWA_PROCESSTREE waits for the descendants.
        let pid = start_child("tree 2");
        let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
        let mut got = 0;
        let rc = DosWaitChild(DCWA_PROCESSTREE, DCWW_WAIT, &mut res, &mut got, pid);
        DosSleep(100);
        check("DosWaitChild DCWA_PROCESSTREE", rc == 0 && got == pid && res.code_result == 7 && counters().processes == before.processes);
    }
    let after = counters();
    check("no leaks in the extra tests", after.processes == before.processes && after.sems32 == before.sems32 && after.timers == before.timers && after.open_files == before.open_files);
}
