//! Byte-range file locks (DosSetFileLocks, DosSetFileLocksL, 16-bit
//! DosFileLocks, DosCancelLockRequest) and the DosProtect* file API.
//!
//! A lock belongs to the open file (all handles duplicated from it), is
//! exclusive or shared (FL_SHARED), and conflicts with overlapping locks
//! of other opens of the same file. A lock request waits up to its
//! timeout for conflicting locks to go away. Locks are dropped when the
//! open file is closed. Reads and writes are not checked against the
//! locks (OS/2's locks are mandatory; rs2's are advisory).
//!
//! The DosProtect* functions are the ordinary file functions with a
//! "file handle lock ID" for DLLs that protect their handles from the
//! application; rs2 hands out the ID but does not check it.

use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::fs::{Handle, OpenFile};
use crate::sync::SpinLock;
use crate::task::WaitQueue;

const ERROR_LOCK_VIOLATION: u32 = 33;
const FL_SHARED: u32 = 1;

struct Lock {
    /// Upper-case full path of the file.
    file: String,
    owner: Weak<OpenFile>,
    start: u64,
    len: u64,
    shared: bool,
}

static LOCKS: SpinLock<Vec<Lock>> = SpinLock::new(Vec::new());
static RELEASED: WaitQueue = WaitQueue::new();

fn same(a: &Weak<OpenFile>, b: &Arc<OpenFile>) -> bool {
    core::ptr::eq(a.as_ptr(), Arc::as_ptr(b))
}

/// Would a lock of [start, start+len) by `of` conflict with another?
fn conflicts(locks: &[Lock], key: &str, of: &Arc<OpenFile>, start: u64, len: u64, shared: bool) -> bool {
    locks.iter().any(|l| {
        l.owner.strong_count() > 0
            && l.file == key
            && !same(&l.owner, of)
            && start < l.start + l.len
            && l.start < start + len
            && !(shared && l.shared)
    })
}

fn open_file(c: &Ctx, hf: u32) -> Result<Arc<OpenFile>, u32> {
    match super::file::get(c, hf) {
        Some(Handle::File(f)) => Ok(f),
        Some(_) => Err(err::INVALID_HANDLE),
        None => Err(err::INVALID_HANDLE),
    }
}

/// Unlock `unlock` (if non-empty), then lock `lock` (if non-empty).
fn set_locks(c: &Ctx, hf: u32, unlock: Option<(u64, u64)>, lock: Option<(u64, u64)>, timeout: u32, flags: u32) -> u32 {
    let of = match open_file(c, hf) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let key = of.path.display().to_ascii_uppercase();
    if let Some((start, len)) = unlock {
        let mut l = LOCKS.lock();
        l.retain(|x| x.owner.strong_count() > 0);
        let Some(i) = l.iter().position(|x| x.file == key && same(&x.owner, &of) && x.start == start && x.len == len) else {
            return ERROR_LOCK_VIOLATION;
        };
        l.remove(i);
        drop(l);
        RELEASED.wake_all();
    }
    let Some((start, len)) = lock else { return 0 };
    let shared = flags & FL_SHARED != 0;
    let free = || !conflicts(&LOCKS.lock(), &key, &of, start, len, shared);
    if !free() {
        if timeout == 0 {
            return ERROR_LOCK_VIOLATION;
        }
        let deadline = if timeout == u32::MAX { u64::MAX } else { crate::irq::ticks() + timeout as u64 * crate::irq::HZ / 1000 };
        // Closing a file drops its locks without a wake-up: look again
        // every 100 ms.
        loop {
            let now = crate::irq::ticks();
            if now >= deadline {
                return ERROR_LOCK_VIOLATION;
            }
            if RELEASED.wait_until(Some(deadline.min(now + crate::irq::HZ / 10)), free) {
                break;
            }
            if c.thread.exit_requested() || c.thread.interrupted() {
                return err::INTERRUPT;
            }
        }
    }
    LOCKS.lock().push(Lock { file: key, owner: Arc::downgrade(&of), start, len, shared });
    0
}

fn range32(c: &Ctx, p: u32) -> Option<(u64, u64)> {
    if p == 0 {
        return None;
    }
    let (off, len) = (c.read_u32(p)? as i32 as i64, c.read_u32(p + 4)? as i32 as i64);
    (len > 0 && off >= 0).then_some((off as u64, len as u64))
}

fn range64(c: &Ctx, p: u32) -> Option<(u64, u64)> {
    if p == 0 {
        return None;
    }
    let q = |a: u32| -> Option<i64> { Some((c.read_u32(a)? as u64 | (c.read_u32(a + 4)? as u64) << 32) as i64) };
    let (off, len) = (q(p)?, q(p + 8)?);
    (len > 0 && off >= 0).then_some((off as u64, len as u64))
}

/// DosSetFileLocks(hFile, pflUnlock, pflLock, timeout, flags)
pub fn dos_set_file_locks(c: &mut Ctx) -> u32 {
    let (hf, pu, pl, t, f) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    set_locks(c, hf, range32(c, pu), range32(c, pl), t, f)
}

/// DosSetFileLocksL: the same with 64-bit FILELOCKL ranges.
pub fn dos_set_file_locks_l(c: &mut Ctx) -> u32 {
    let (hf, pu, pl, t, f) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    set_locks(c, hf, range64(c, pu), range64(c, pl), t, f)
}

/// 16-bit DosFileLocks(hf, pUnlockRange, pLockRange): exclusive, no wait.
pub fn dos16_file_locks(c: &mut Ctx) -> u32 {
    let (hf, pu, pl) = (c.arg(0), c.ptr(1), c.ptr(2));
    set_locks(c, hf, range32(c, pu), range32(c, pl), 0, 0)
}

/// DosCancelLockRequest(hFile, pflLock): a request that is waiting gives
/// up. Requests wait in the requesting thread here, so there is nothing
/// queued to cancel from another thread.
pub fn dos_cancel_lock_request(c: &mut Ctx) -> u32 {
    match open_file(c, c.arg(0)) {
        Ok(_) => {
            RELEASED.wake_all();
            0
        }
        Err(e) => e,
    }
}

/// Number of locks held (leak checks).
pub fn count() -> u32 {
    let mut l = LOCKS.lock();
    l.retain(|x| x.owner.strong_count() > 0);
    l.len() as u32
}

// ---- DosProtect* ----

/// The lock ID handed out for a protected handle.
fn lock_id(hf: u32) -> u32 {
    0x5052_0000 | (hf & 0xffff)
}

/// DosProtectOpen(name, phf, pulAction, cbFile, attr, openFlags, openMode,
/// peaop2, pfhFileHandleLockID)
pub fn dos_protect_open(c: &mut Ctx) -> u32 {
    let r = super::file::dos_open(c);
    if r == 0 {
        let hf = c.read_u32(c.arg(1)).unwrap_or(0);
        c.write_u32(c.arg(8), lock_id(hf));
    }
    r
}

/// DosProtectOpenL: cbFile is 64-bit, so the lock ID is argument 9.
pub fn dos_protect_open_l(c: &mut Ctx) -> u32 {
    let r = super::file::dos_open_l(c);
    if r == 0 {
        let hf = c.read_u32(c.arg(1)).unwrap_or(0);
        c.write_u32(c.arg(9), lock_id(hf));
    }
    r
}

/// DosProtectSetFileLocks(hFile, pflUnlock, pflLock, timeout, flags, fhLockID)
pub fn dos_protect_set_file_locks(c: &mut Ctx) -> u32 {
    dos_set_file_locks(c)
}

pub fn dos_protect_set_file_locks_l(c: &mut Ctx) -> u32 {
    dos_set_file_locks_l(c)
}
