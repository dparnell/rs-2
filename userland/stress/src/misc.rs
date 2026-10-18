//! The less common kernel APIs: thread-local memory, must-complete
//! sections, DosQuerySysState semaphores and shared memory, timers,
//! DosPerfSysCall, DosQueryModFromEIP, DosErrClass, DosFSCtl, DosSysCtl,
//! file locks, RAS and ABIOS queries.

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, join, thread};

static TLS: AtomicU32 = AtomicU32::new(0);
static TLS_OK: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn tls_user(n: u32) {
    let p = TLS.load(Ordering::Relaxed) as *mut u32;
    unsafe {
        let ok = *p == 0 && *p.add(1) == 0;
        for i in 0..1000u32 {
            *p = n * 1000 + i;
            *p.add(1) = !(n * 1000 + i);
            if i % 100 == 0 {
                DosSleep(1);
            }
            if *p != n * 1000 + i || *p.add(1) != !(n * 1000 + i) {
                return;
            }
        }
        if ok {
            TLS_OK.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub fn run() {
    unsafe {
        // Thread-local memory.
        let mut p: *mut u32 = null_mut();
        let rc = DosAllocThreadLocalMemory(2, &mut p);
        check("DosAllocThreadLocalMemory", rc == 0 && !p.is_null());
        TLS.store(p as u32, Ordering::Relaxed);
        *p = 0x1234;
        TLS_OK.store(0, Ordering::Relaxed);
        let ts: alloc::vec::Vec<u32> = (1..=8).map(|n| thread(tls_user, n)).collect();
        for t in ts {
            join(t);
        }
        check("thread-local memory is per thread", TLS_OK.load(Ordering::Relaxed) == 8 && *p == 0x1234);
        check("DosFreeThreadLocalMemory", DosFreeThreadLocalMemory(p) == 0);
        check("free twice fails", DosFreeThreadLocalMemory(p) != 0);
        let mut q: *mut u32 = null_mut();
        check("too much thread-local memory fails", DosAllocThreadLocalMemory(9, &mut q) != 0);

        // Must-complete sections nest.
        let mut n = 0;
        DosEnterMustComplete(&mut n);
        let a = n;
        DosEnterMustComplete(&mut n);
        let b = n;
        DosExitMustComplete(&mut n);
        let c = n;
        DosExitMustComplete(&mut n);
        check("DosEnter/ExitMustComplete nesting", a == 1 && b == 2 && c == 1 && n == 0);
        check("exit without enter fails", DosExitMustComplete(&mut n) != 0);

        // DosQuerySysState: our semaphores and shared memory are listed.
        let mut ev = 0;
        DosCreateEventSem(b"\\SEM32\\STRESS\\STATE\0".as_ptr(), &mut ev, 0, 1);
        let mut shm = null_mut();
        DosAllocSharedMem(&mut shm, b"\\SHAREMEM\\STRESS\\STATE\0".as_ptr(), 4096, PAG_READ | PAG_WRITE | PAG_COMMIT);
        let mut buf = alloc::vec![0u8; 64 * 1024];
        let base = buf.as_mut_ptr() as u32;
        let rc = DosQuerySysState(QS_PROCESS | QS_SEMAPHORE | QS_MTE | QS_FILESYS | QS_SHMEMORY, 0, 0, 0, buf.as_mut_ptr(), buf.len() as u32);
        check("DosQuerySysState", rc == 0);
        let off = |a: u32| (a - base) as usize;
        // 32-bit semaphores.
        let mut found_sem = false;
        let mut s = rd32(&buf, 12);
        let mut guard = 0;
        while s != 0 && guard < 1000 {
            let o = off(s);
            let flags = rd16(&buf, o + 4);
            let pname = rd32(&buf, o + 8);
            if pname != 0 {
                let n = off(pname);
                let end = buf[n..].iter().position(|&b| b == 0).unwrap_or(0);
                if &buf[n..n + end] == b"\\SEM32\\STRESS\\STATE" && flags & 0x10 != 0 && flags & 0x40 != 0 {
                    found_sem = true;
                }
            }
            s = rd32(&buf, o);
            guard += 1;
        }
        check("DosQuerySysState lists the posted named event", found_sem);
        // Shared memory.
        let mut found_mem = false;
        let mut m = rd32(&buf, 16);
        let mut guard = 0;
        while m != 0 && guard < 1000 {
            let o = off(m);
            let name = &buf[o + 10..];
            let end = name.iter().position(|&b| b == 0).unwrap_or(0);
            if &name[..end] == b"\\SHAREMEM\\STRESS\\STATE" && rd16(&buf, o + 8) >= 1 {
                found_mem = true;
            }
            let next = rd32(&buf, o);
            m = if next == 0 { 0 } else { next };
            guard += 1;
        }
        check("DosQuerySysState lists the named shared memory", found_mem);
        // Our process record has its private semaphore count.
        let mut prec = rd32(&buf, 4);
        let me = crate::procs::my_pid();
        let mut ok = false;
        while prec != 0 {
            let o = off(prec);
            if rd32(&buf, o) != 1 {
                break;
            }
            if rd16(&buf, o + 8) as u32 == me {
                ok = rd16(&buf, o + 26) >= 2 && rd16(&buf, o + 40) >= 1;
            }
            let thr = rd32(&buf, o + 4);
            let nthr = rd16(&buf, o + 26) as u32;
            prec = thr + nthr * 28;
        }
        check("our process record counts threads and shared memory", ok);
        DosCloseEventSem(ev);
        DosFreeMem(shm);

        // High-resolution timer.
        let mut f = 0;
        DosTmrQueryFreq(&mut f);
        let (mut t0, mut t1) = (0u64, 0u64);
        DosTmrQueryTime(&mut t0);
        DosTmrQueryTime(&mut t1);
        let mut t2 = 0u64;
        let mut spins = 0;
        loop {
            DosTmrQueryTime(&mut t2);
            spins += 1;
            if t2 != t1 || spins > 100_000 {
                break;
            }
        }
        check("DosTmrQueryTime finer than a millisecond", f > 1000 && t1 >= t0 && t2 > t1 && (t2 - t1) * 1000 < f as u64);
        let mut tlast = t2;
        let mut mono = true;
        for _ in 0..1000 {
            let mut t = 0u64;
            DosTmrQueryTime(&mut t);
            mono &= t >= tlast;
            tlast = t;
        }
        check("DosTmrQueryTime monotonic", mono);

        // DosPerfSysCall: CPU utilisation snapshot (CMD_KI_RDCNT).
        // CPUUTIL: time, idle, busy, interrupt time (64-bit each).
        let q = |c: &[u32; 8], i: usize| c[i * 2] as u64 | (c[i * 2 + 1] as u64) << 32;
        let mut cpu = [0u32; 8];
        let rc = DosPerfSysCall(0x63, cpu.as_mut_ptr() as u32, 0, 0);
        DosSleep(100);
        let mut cpu2 = [0u32; 8];
        DosPerfSysCall(0x63, cpu2.as_mut_ptr() as u32, 0, 0);
        let dt = q(&cpu2, 0) - q(&cpu, 0);
        let parts = (q(&cpu2, 1) - q(&cpu, 1)) + (q(&cpu2, 2) - q(&cpu, 2)) + (q(&cpu2, 3) - q(&cpu, 3));
        check("DosPerfSysCall CMD_KI_RDCNT", rc == 0 && dt * 1000 / f as u64 >= 90 && parts <= dt + dt / 50 && parts + dt / 10 >= dt);
        check("DosPerfSysCall unknown command", DosPerfSysCall(0x7777, 0, 0, 0) != 0);

        // DosQueryModFromEIP on our own code.
        let mut hmod = 0;
        let mut obj = 0;
        let mut name = [0u8; 260];
        let mut offs = 0;
        let eip = run as *const () as u32;
        let rc = DosQueryModFromEIP(&mut hmod, &mut obj, 260, name.as_mut_ptr(), &mut offs, eip);
        let n = name.iter().position(|&b| b == 0).unwrap_or(0);
        check("DosQueryModFromEIP", rc == 0 && hmod != 0 && &name[..n] == b"STRESS");
        check("DosQueryModFromEIP outside any module", DosQueryModFromEIP(&mut hmod, &mut obj, 260, name.as_mut_ptr(), &mut offs, 0x10) != 0);

        // DosErrClass.
        let (mut class, mut action, mut locus) = (0, 0, 0);
        DosErrClass(2, &mut class, &mut action, &mut locus);
        check("DosErrClass file not found", class == 8 && locus == 2);

        // DosFSCtl: the file system name of C:.
        let mut out = [0u8; 64];
        let mut cbout = 0;
        let rc = DosFSCtl(out.as_mut_ptr(), 64, &mut cbout, null_mut(), 0, null_mut(), 2, b"C:\\\0".as_ptr(), 0xffff_ffff, 2);
        check("DosFSCtl FSCTL_MAX_EASIZE by path", rc == 0 && cbout == 6 && u16::from_le_bytes([out[0], out[1]]) == 65535);
        let rc_bad = DosFSCtl(null_mut(), 0, null_mut(), null_mut(), 0, null_mut(), 0x8123, b"C:\\\0".as_ptr(), 0xffff_ffff, 2);
        check("DosFSCtl unknown function", rc_bad == 1);
        let mut route = [0u8; 32];
        let mut cbr = 0;
        let rc = DosFSCtl(route.as_mut_ptr(), 32, &mut cbr, null_mut(), 0, null_mut(), 3, b"FAT\0".as_ptr(), 0xffff_ffff, 3);
        check("DosFSCtl FSCTL_GET_NEXT_ROUTE_NAME", rc == 0 && &route[4..7] == b"FAT");

        // File locks.
        let (mut h, mut act) = (0, 0);
        DosOpen(b"C:\\STRESSLK.TMP\0".as_ptr(), &mut h, &mut act, 0, 0, 0x12, 0x42, null_mut());
        let mut nw = 0;
        DosWrite(h, [0u8; 512].as_ptr(), 512, &mut nw);
        let lock = [0u32, 100];
        let none = [0u32, 0];
        let rc1 = DosSetFileLocks(h, none.as_ptr(), lock.as_ptr(), 0, 0);
        let (mut h2, mut act2) = (0, 0);
        DosOpen(b"C:\\STRESSLK.TMP\0".as_ptr(), &mut h2, &mut act2, 0, 0, 1, 0x42, null_mut());
        let rc2 = DosSetFileLocks(h2, none.as_ptr(), [50u32, 10].as_ptr(), 0, 0);
        let rc3 = DosSetFileLocks(h2, none.as_ptr(), [200u32, 10].as_ptr(), 0, 0);
        let rc4 = DosSetFileLocks(h, lock.as_ptr(), none.as_ptr(), 0, 0);
        let rc5 = DosSetFileLocks(h2, none.as_ptr(), [50u32, 10].as_ptr(), 0, 0);
        check("DosSetFileLocks", rc1 == 0 && rc2 == 33 && rc3 == 0 && rc4 == 0 && rc5 == 0);
        DosClose(h2);
        DosClose(h);
        DosDelete(b"C:\\STRESSLK.TMP\0".as_ptr());

        // Queries that report "not available" on a PC without them.
        check("DosQueryABIOSSupport", DosQueryABIOSSupport(0) != 0);
        let mut ras = null_mut();
        let rc = DosQueryRASInfo(0, &mut ras);
        check("DosQueryRASInfo answers", rc == 0 || rc == 87 || rc == 50);
        let mut cb = 0;
        check("DosQueryResourceSize of a missing resource", DosQueryResourceSize(0, 1, 9999, &mut cb) != 0);
        let _ = null::<u8>();
    }
}
