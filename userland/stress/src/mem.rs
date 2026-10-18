//! Memory: allocate/commit/decommit/free storms, address space
//! exhaustion, shared memory given to and named by child processes, and a
//! check that processes give all their physical memory back.

use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{avail_mem, check, counters, join, run_child, start_child, thread, wait_child, Rng};

/// One storm of `n` operations; returns false on any unexpected result.
fn storm(seed: u32, n: u32) -> bool {
    let mut rng = Rng(seed);
    let mut live: alloc::vec::Vec<(*mut u8, u32)> = alloc::vec::Vec::new();
    let mut ok = true;
    for i in 0..n {
        match rng.below(4) {
            0 | 1 if live.len() < 64 => unsafe {
                let size = (rng.below(256) + 1) * 4096;
                let mut p = null_mut();
                let flags = PAG_READ | PAG_WRITE | if rng.below(2) == 0 { PAG_COMMIT } else { 0 };
                let rc = DosAllocMem(&mut p, size, flags);
                if rc != 0 {
                    println!("  storm {i}: DosAllocMem {size:#x} rc {rc}");
                    ok = false;
                    continue;
                }
                if flags & PAG_COMMIT == 0 {
                    let pages = rng.below(size / 4096) + 1;
                    if DosSetMem(p, pages * 4096, PAG_COMMIT | PAG_READ | PAG_WRITE) != 0 {
                        ok = false;
                    }
                    *p = i as u8;
                } else {
                    *p.add(size as usize - 1) = i as u8;
                    *p = i as u8;
                }
                live.push((p, size));
            },
            2 if !live.is_empty() => unsafe {
                let (p, size) = live[rng.below(live.len() as u32) as usize];
                // Decommit part of it and commit it again.
                let pages = size / 4096;
                let first = rng.below(pages);
                let count = rng.below(pages - first) + 1;
                let at = p.add(first as usize * 4096);
                let r1 = DosSetMem(at, count * 4096, PAG_DECOMMIT);
                let r2 = DosSetMem(at, count * 4096, PAG_COMMIT | PAG_READ | PAG_WRITE);
                *at = 7;
                if r1 != 0 || r2 != 0 || *at != 7 {
                    ok = false;
                }
            },
            _ if !live.is_empty() => unsafe {
                let k = rng.below(live.len() as u32) as usize;
                let (p, _) = live.swap_remove(k);
                if DosFreeMem(p) != 0 {
                    ok = false;
                }
            },
            _ => {}
        }
    }
    for (p, _) in live {
        unsafe { DosFreeMem(p) };
    }
    ok
}

static STORM_OK: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn storm_thread(seed: u32) {
    if storm(seed, 400) {
        STORM_OK.fetch_add(1, Ordering::Relaxed);
    }
}

/// Child: a memory storm, exit code 0 on success.
pub fn child_storm(seed: u32) -> u32 {
    // Leave memory behind on purpose: the kernel must free it at exit.
    let mut p = null_mut();
    unsafe {
        DosAllocMem(&mut p, 0x40000, PAG_READ | PAG_WRITE | PAG_COMMIT);
        for i in 0..64 {
            *p.add(i * 4096) = 1;
        }
    }
    if storm(seed, 300) { 0 } else { 1 }
}

/// Child: get shared memory given to us at `addr`, add 1 to its first
/// dword, return the new value.
pub fn child_getshm(addr: u32) -> u32 {
    unsafe {
        let p = addr as *mut u32;
        // Wait until the parent has given it.
        for _ in 0..200 {
            let (mut cb, mut fl) = (4096u32, 0u32);
            if DosQueryMem(p as *mut u8, &mut cb, &mut fl) == 0 && fl & PAG_FREE == 0 {
                break;
            }
            DosSleep(10);
        }
        *p += 1;
        *p
    }
}

/// Child: open the named shared memory and write a marker.
pub fn child_namedshm() -> u32 {
    let mut p = null_mut();
    unsafe {
        if DosGetNamedSharedMem(&mut p, b"\\SHAREMEM\\STRESS\0".as_ptr(), PAG_READ | PAG_WRITE) != 0 {
            return 1;
        }
        *(p as *mut u32).add(1) = 0x5354_5253;
        DosFreeMem(p);
    }
    0
}

pub fn run() {
    check("allocation storm", storm(1, 3000));

    // Storms in several threads at once.
    STORM_OK.store(0, Ordering::Relaxed);
    let tids: alloc::vec::Vec<u32> = (0..6).map(|i| thread(storm_thread, 100 + i)).collect();
    for t in tids {
        join(t);
    }
    check("6 concurrent storms", STORM_OK.load(Ordering::Relaxed) == 6);

    // Address space exhaustion: reserve until it fails, then free it all.
    unsafe {
        let mut blocks = alloc::vec::Vec::new();
        let mut rc = 0;
        for _ in 0..200 {
            let mut p = null_mut();
            rc = DosAllocMem(&mut p, 16 << 20, PAG_READ | PAG_WRITE);
            if rc != 0 {
                break;
            }
            blocks.push(p);
        }
        check("private arena exhausted with ERROR_NOT_ENOUGH_MEMORY", rc == 8 && blocks.len() > 10);
        for p in &blocks {
            DosFreeMem(*p);
        }
        let mut p = null_mut();
        check("memory usable again", DosAllocMem(&mut p, 64 << 20, PAG_READ | PAG_WRITE) == 0 && DosFreeMem(p) == 0);
    }

    // Shared memory given to a child.
    unsafe {
        let before = counters();
        let mut p = null_mut();
        let rc = DosAllocSharedMem(&mut p, core::ptr::null(), 0x10000, PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE);
        *(p as *mut u32) = 41;
        let child = start_child(&alloc::format!("getshm {}", p as u32));
        // The child may not be running yet: give right away.
        let grc = DosGiveSharedMem(p, child, PAG_READ | PAG_WRITE);
        let (_, cause, code, _) = wait_child(child);
        check("DosGiveSharedMem to a child", rc == 0 && grc == 0 && cause == 0 && code == 42 && *(p as *mut u32) == 42);
        check("give to a bad pid fails", DosGiveSharedMem(p, 9999, PAG_READ) == 303);
        DosFreeMem(p);

        let mut q = null_mut();
        DosAllocSharedMem(&mut q, b"\\SHAREMEM\\STRESS\0".as_ptr(), 8192, PAG_READ | PAG_WRITE | PAG_COMMIT);
        let (cause, code) = run_child("namedshm");
        check("named shared memory written by a child", cause == 0 && code == 0 && *(q as *mut u32).add(1) == 0x5354_5253);
        let mut q2 = null_mut();
        check("duplicate shared name fails", DosAllocSharedMem(&mut q2, b"\\SHAREMEM\\STRESS\0".as_ptr(), 4096, PAG_READ | PAG_COMMIT) == 183);
        DosFreeMem(q);
        check("name gone after the last free", DosGetNamedSharedMem(&mut q2, b"\\SHAREMEM\\STRESS\0".as_ptr(), PAG_READ) == 2);
        // Many shared objects.
        let mut objs = alloc::vec::Vec::new();
        for _ in 0..100 {
            let mut s = null_mut();
            if DosAllocSharedMem(&mut s, core::ptr::null(), 0x2000, PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GETTABLE) == 0 {
                *s = 1;
                objs.push(s);
            }
        }
        for s in &objs {
            DosFreeMem(*s);
        }
        let after = counters();
        check("100 shared objects allocated and freed", objs.len() == 100 && after.shared_mem == before.shared_mem);
    }

    // Aliases and DosQueryMemState.
    unsafe {
        let mut p = null_mut();
        DosAllocMem(&mut p, 0x3000, PAG_READ | PAG_WRITE | PAG_COMMIT);
        *p = 0x11;
        let mut a = null_mut();
        let rc = DosAliasMem(p, 0x3000, &mut a, 0);
        check("DosAliasMem", rc == 0 && !a.is_null() && a != p && *a == 0x11);
        if rc == 0 {
            *a.add(0x2000) = 0x22;
            check("alias writes through", *p.add(0x2000) == 0x22);
            DosFreeMem(a);
        }
        let (mut cb, mut fl) = (0x3000u32, 0u32);
        let rc = DosQueryMemState(p, &mut cb, &mut fl);
        check("DosQueryMemState", rc == 0 && cb >= 0x1000 && fl & 1 != 0);
        DosFreeMem(p);
    }

    // Physical memory comes back after processes end.
    let (_, _) = run_child("mem 1");
    let base = avail_mem();
    let mut ok = true;
    for i in 0..8 {
        let (cause, code) = run_child(&alloc::format!("mem {}", i + 2));
        ok &= cause == 0 && code == 0;
    }
    unsafe { DosSleep(100) };
    let now = avail_mem();
    let lost = base.saturating_sub(now);
    if lost > 0 {
        println!("  {lost} bytes of physical memory not returned after 8 child storms");
    }
    check("8 child storms", ok);
    check("children return their physical memory", lost <= 64 * 1024);
}
