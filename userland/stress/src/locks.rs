//! Mandatory byte-range file locks: DosRead/DosWrite/DosSetFileSize
//! against another open's locks, shared and exclusive locks, partial
//! overlaps, the 32-bit calls' 64 KB pieces, LOCK_ATOMIC, waits, time-outs
//! and DosCancelLockRequest, release on the last close and at process
//! exit, inherited handles, DosCopy. The expected results are IBM's
//! (kernel/src/api/filelock.rs lists the rules).

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, counters, join, ms, start_child, thread, wait_child};

/// The test files, on drive C: or in the directory STRESSDIR names (to
/// run the checks on another file system).
fn file(name: &str) -> alloc::vec::Vec<u8> {
    let mut v = alloc::vec::Vec::new();
    let mut p: *const u8 = null();
    if unsafe { DosScanEnv(b"STRESSDIR\0".as_ptr(), &mut p) } == 0 && !p.is_null() {
        let mut i = 0;
        while unsafe { *p.add(i) } != 0 {
            v.push(unsafe { *p.add(i) });
            i += 1;
        }
    } else {
        v.extend_from_slice(b"C:");
    }
    if !v.ends_with(b"\\") {
        v.push(b'\\');
    }
    v.extend_from_slice(name.as_bytes());
    v.push(0);
    v
}
const HELD: &[u8] = b"\\SEM32\\STRESSLK\\HELD\0";
const GO: &[u8] = b"\\SEM32\\STRESSLK\\GO\0";
const SIZE: u32 = 200_000;

/// Open the test file read/write, deny none (inheritable).
fn open(create: bool) -> u32 {
    let (mut h, mut act) = (0, 0);
    let flags = if create { 0x12 } else { 0x01 };
    let rc = unsafe { DosOpen(file("STRESSL2.TMP").as_ptr(), &mut h, &mut act, 0, 0, flags, 0x42, null_mut()) };
    if rc != 0 {
        println!("  DosOpen rc {rc}");
    }
    h
}

fn seek(h: u32, off: u32) {
    let mut p = 0;
    unsafe { DosSetFilePtr(h, off as i32, 0, &mut p) };
}

/// Read `n` bytes at `off` into `buf`: (rc, bytes read).
fn rd(h: u32, off: u32, buf: *mut u8, n: u32) -> (u32, u32) {
    seek(h, off);
    let mut got = 0xdead;
    let rc = unsafe { DosRead(h, buf, n, &mut got) };
    (rc, got)
}

fn wr(h: u32, off: u32, buf: *const u8, n: u32) -> (u32, u32) {
    seek(h, off);
    let mut done = 0xdead;
    let rc = unsafe { DosWrite(h, buf, n, &mut done) };
    (rc, done)
}

/// DosSetFileLocks with optional (offset, length) ranges.
fn lk(h: u32, unlock: Option<(u32, u32)>, lock: Option<(u32, u32)>, timeout: u32, flags: u32) -> u32 {
    let u = unlock.map(|(o, l)| [o, l]);
    let l = lock.map(|(o, l)| [o, l]);
    let pu = u.as_ref().map_or(null(), |a| a.as_ptr());
    let pl = l.as_ref().map_or(null(), |a| a.as_ptr());
    unsafe { DosSetFileLocks(h, pu, pl, timeout, flags) }
}

fn lock(h: u32, off: u32, len: u32, flags: u32) -> u32 {
    lk(h, None, Some((off, len)), 0, flags)
}

fn unlock(h: u32, off: u32, len: u32) -> u32 {
    lk(h, Some((off, len)), None, 0, 0)
}

static WAIT_RC: AtomicU32 = AtomicU32::new(0);
static WAIT_TIMEOUT: AtomicU32 = AtomicU32::new(0);

/// A thread waiting for [100, 110) through handle `h`.
unsafe extern "C" fn waiter(h: u32) {
    WAIT_RC.store(0xffff, Ordering::Relaxed);
    let rc = lk(h, None, Some((100, 10)), WAIT_TIMEOUT.load(Ordering::Relaxed), 0);
    WAIT_RC.store(rc, Ordering::Relaxed);
}

/// `STRESS -child lockhold`: lock [1000, 1100) exclusively and
/// [2000, 2100) shared, say so, wait for the go and end without
/// unlocking.
pub fn child_hold() -> u32 {
    let h = open(false);
    let (mut held, mut go) = (0, 0);
    unsafe {
        if DosOpenEventSem(HELD.as_ptr(), &mut held) != 0 || DosOpenEventSem(GO.as_ptr(), &mut go) != 0 {
            return 1;
        }
    }
    if lock(h, 1000, 100, 0) != 0 || lock(h, 2000, 100, 1) != 0 {
        return 2;
    }
    unsafe {
        DosPostEventSem(held);
        DosWaitEventSem(go, 20_000);
    }
    0
}

/// `STRESS -child lockinh H`: read [3000, 3010) through the inherited
/// handle H (its parent's lock is not ours); the exit code is the rc.
pub fn child_inherited(args: &[alloc::string::String]) -> u32 {
    let h: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut b = [0u8; 16];
    let (rc, n) = rd(h, 3000, b.as_mut_ptr(), 10);
    if n != 0 { 1000 + n } else { rc }
}

/// `STRESS -child lockwait PATH MS`: lock the first 100 bytes of PATH
/// exclusively for MS milliseconds (to try other programs, such as
/// IBM CMD's COPY and TYPE, against a lock: `DETACH`).
pub fn child_wait(args: &[alloc::string::String]) -> u32 {
    let (Some(path), Some(t)) = (args.first(), args.get(1).and_then(|s| s.parse::<u32>().ok())) else { return 87 };
    let mut name = alloc::vec::Vec::from(path.as_bytes());
    name.push(0);
    let (mut h, mut act) = (0, 0);
    // Read-only, deny none: other programs may open it to write too.
    let rc = unsafe { DosOpen(name.as_ptr(), &mut h, &mut act, 0, 0, 0x01, 0x40, null_mut()) };
    if rc != 0 {
        return rc;
    }
    let rc = lock(h, 0, 100, 0);
    println!("lockwait: {} locked ({rc}) for {t} ms", path);
    unsafe { DosSleep(t) };
    rc
}

pub fn run() {
    let before = counters().file_locks;
    let mut mem: *mut u8 = null_mut();
    // 64 KB-aligned buffer: the 32-bit calls' pieces follow its address.
    if unsafe { DosAllocMem(&mut mem, 0x40000, 0x13) } != 0 {
        check("locks: buffer", false);
        return;
    }
    let buf = mem;
    let (f, g) = (file("STRESSL2.TMP"), file("STRESSL3.TMP"));
    let h1 = open(true);
    let (rc, n) = wr(h1, 0, buf, SIZE);
    check("locks: test file written", rc == 0 && n == SIZE);
    let h2 = open(false);

    // Exclusive lock of [100, 200) through h1.
    check("lock [100,200) exclusive", lock(h1, 100, 100, 0) == 0);
    check("other open reads before the lock", rd(h2, 0, buf, 50) == (0, 50));
    check("read overlapping the lock fails entirely", rd(h2, 50, buf, 100) == (33, 0));
    check("read of the lock's last byte fails", rd(h2, 199, buf, 1) == (33, 0));
    check("read right after the lock", rd(h2, 200, buf, 10) == (0, 10));
    check("write into the lock fails", wr(h2, 150, buf, 10) == (33, 0));
    check("owner reads its exclusive lock", rd(h1, 150, buf, 10) == (0, 10));
    check("owner writes its exclusive lock", wr(h1, 150, buf, 10) == (0, 10));
    check("read past the end of file is still checked", {
        let r = lock(h1, SIZE + 100, 10, 0);
        let x = rd(h2, SIZE - 10, buf, 200);
        unlock(h1, SIZE + 100, 10);
        r == 0 && x == (33, 0)
    });

    // Lock against lock.
    check("other open cannot lock inside it", lock(h2, 150, 10, 0) == 33);
    check("other open cannot lock it shared", lock(h2, 150, 10, 1) == 33);
    check("owner's overlapping lock conflicts with its own", lock(h1, 150, 100, 0) == 33);
    check("unlock of a range not locked", unlock(h1, 100, 50) == 33);
    check("unlock through the other open", unlock(h2, 100, 100) == 33);
    check("no ranges at all", lk(h1, None, None, 0, 0) == 33);
    check("zero-length ranges only", lk(h1, Some((0, 0)), Some((5, 0)), 0, 0) == 33);
    check("range past 4 GB", lock(h1, 0xffff_fff0, 0x20, 0) == 33);
    check("range ending at 4 GB", lock(h1, 0xffff_fff0, 0x10, 0) == 0 && unlock(h1, 0xffff_fff0, 0x10) == 0);
    check("time-out", {
        let t = ms();
        let rc = lk(h2, None, Some((150, 10)), 300, 0);
        let dt = ms().wrapping_sub(t);
        rc == 33 && dt >= 250
    });

    // Shared lock.
    check("unlock [100,200)", unlock(h1, 100, 100) == 0);
    check("lock [100,200) shared", lock(h1, 100, 100, 1) == 0);
    check("shared: other open reads", rd(h2, 120, buf, 10) == (0, 10));
    check("shared: other open cannot write", wr(h2, 120, buf, 10) == (33, 0));
    check("shared: owner reads", rd(h1, 120, buf, 10) == (0, 10));
    check("shared: owner cannot write either", wr(h1, 120, buf, 10) == (33, 0));
    check("shared over shared", lock(h2, 150, 10, 1) == 0);
    check("exclusive over shared", lock(h2, 160, 10, 0) == 33);
    check("unlock the second shared lock", unlock(h2, 150, 10) == 0);

    // LOCK_ATOMIC: change the lock's type in place.
    check("atomic: shared -> exclusive", lk(h1, Some((100, 100)), Some((100, 100)), 0, 2) == 0);
    check("atomic: now exclusive", rd(h2, 120, buf, 10) == (33, 0) && wr(h1, 120, buf, 10) == (0, 10));
    check("atomic: exclusive -> shared", lk(h1, Some((100, 100)), Some((100, 100)), 0, 3) == 0);
    check("atomic: now shared", rd(h2, 120, buf, 10) == (0, 10));
    check("atomic: ranges must match", lk(h1, Some((100, 100)), Some((100, 50)), 0, 2) == 33);
    check("atomic: blocked by another lock", {
        let a = lock(h2, 150, 10, 1);
        let r = lk(h1, Some((100, 100)), Some((100, 100)), 0, 2);
        a == 0 && r == 33 && unlock(h2, 150, 10) == 0
    });
    check("unlock and lock in one call", lk(h1, Some((100, 100)), Some((100, 100)), 0, 0) == 0 && rd(h2, 120, buf, 10) == (33, 0));

    // Waiting requests: cancelled, then satisfied by an unlock.
    WAIT_TIMEOUT.store(u32::MAX, Ordering::Relaxed);
    let t = thread(waiter, h2);
    unsafe { DosSleep(200) };
    let cr = [100u32, 10];
    let rc = unsafe { DosCancelLockRequest(h2, cr.as_ptr()) };
    join(t);
    check("DosCancelLockRequest ends the wait with ERROR_INTERRUPT", rc == 0 && WAIT_RC.load(Ordering::Relaxed) == 95);
    check("DosCancelLockRequest with nothing waiting", unsafe { DosCancelLockRequest(h2, cr.as_ptr()) } == 173);
    WAIT_TIMEOUT.store(5000, Ordering::Relaxed);
    let t0 = ms();
    let t = thread(waiter, h2);
    unsafe { DosSleep(200) };
    let u = unlock(h1, 100, 100);
    join(t);
    let dt = ms().wrapping_sub(t0);
    check("a waiting lock gets the range when it is unlocked", u == 0 && WAIT_RC.load(Ordering::Relaxed) == 0 && dt < 3000);
    check("unlock the waiter's lock", unlock(h2, 100, 10) == 0);

    // The 32-bit calls go in 64 KB pieces of the buffer.
    check("lock [100000,100010)", lock(h1, 100_000, 10, 0) == 0);
    check("32-bit read stops before the locked piece", rd(h2, 0, buf, 150_000) == (33, 65536));
    check("unaligned buffer: shorter first piece", rd(h2, 0, unsafe { buf.add(0x100) }, 150_000) == (33, 65280));
    check("32-bit write stops before the locked piece", wr(h2, 0, buf, 150_000) == (33, 65536));
    check("whole read inside one piece fails", rd(h2, 99_000, buf, 2000) == (33, 0));
    check("zero-byte 32-bit read and write", rd(h2, 100_000, buf, 0) == (0, 0) && wr(h2, 100_000, buf, 0) == (0, 0));

    // DosSetFileSize: [new size, + low 16 bits of the old size).
    check("set size across the lock", unsafe { DosSetFileSize(h2, 99_000) } == 33);
    check("set size above the lock", unsafe { DosSetFileSize(h2, 210_000) } == 0);
    check("owner sets the size", unsafe { DosSetFileSize(h1, 99_000) } == 0 && unsafe { DosSetFileSize(h1, SIZE) } == 0);

    // DosCopy reads through a handle of its own.
    check("DosCopy of a locked file", unsafe { DosCopy(f.as_ptr(), g.as_ptr(), 1) } == 33);
    check("unlock [100000,100010)", unlock(h1, 100_000, 10) == 0);
    check("DosCopy once unlocked", unsafe { DosCopy(f.as_ptr(), g.as_ptr(), 1) } == 0);
    unsafe { DosDelete(g.as_ptr()) };

    // Locks go with the last handle to the open file.
    let mut h1b = u32::MAX;
    unsafe { DosDupHandle(h1, &mut h1b) };
    check("lock through a duplicate", lock(h1b, 500, 10, 0) == 0 && rd(h1, 500, buf, 10) == (0, 10));
    unsafe { DosClose(h1b) };
    check("closing a duplicate keeps the locks", rd(h2, 500, buf, 10) == (33, 0));
    let h3 = open(false);
    check("another open's lock", lock(h3, 600, 10, 0) == 0 && rd(h2, 600, buf, 10) == (33, 0));
    unsafe { DosClose(h3) };
    check("closing its last handle releases it", rd(h2, 600, buf, 10) == (0, 10));

    // Another process: its locks, its exit.
    let (mut held, mut go) = (0, 0);
    unsafe {
        DosCreateEventSem(HELD.as_ptr(), &mut held, 0, 0);
        DosCreateEventSem(GO.as_ptr(), &mut go, 0, 0);
    }
    let pid = start_child("lockhold");
    let ok = unsafe { DosWaitEventSem(held, 10_000) } == 0;
    check("child holds its locks", ok);
    check("child's exclusive lock stops our read", rd(h2, 1050, buf, 10) == (33, 0));
    check("child's shared lock lets us read", rd(h2, 2050, buf, 10) == (0, 10));
    check("child's shared lock stops our write", wr(h2, 2050, buf, 10) == (33, 0));
    check("child's lock stops ours", lock(h2, 1050, 10, 0) == 33);
    unsafe { DosPostEventSem(go) };
    let t = ms();
    let rc = lk(h2, None, Some((1050, 10)), 10_000, 0);
    let dt = ms().wrapping_sub(t);
    let (wrc, _, code, _) = wait_child(pid);
    check("child's exit releases its locks", rc == 0 && dt < 9000 && wrc == 0 && code == 0);
    check("unlock after the child", unlock(h2, 1050, 10) == 0 && rd(h2, 2050, buf, 10) == (0, 10));
    unsafe {
        DosCloseEventSem(held);
        DosCloseEventSem(go);
    }

    // An inherited handle does not own its parent's locks.
    check("lock [3000,3010)", lock(h1, 3000, 10, 0) == 0);
    let pid = start_child(&alloc::format!("lockinh {h1}"));
    let (wrc, _, code, _) = wait_child(pid);
    check("child reading through the inherited handle is stopped", wrc == 0 && code == 33);
    check("the child's exit leaves the parent's lock", rd(h2, 3000, buf, 10) == (33, 0));

    unsafe {
        DosClose(h2);
        DosClose(h1);
        DosDelete(f.as_ptr());
        DosFreeMem(mem);
    }
    check("locks: none left", counters().file_locks == before);
}
