//! Byte-range file locks (DosSetFileLocks, DosSetFileLocksL, 16-bit
//! DosFileLocks, DosCancelLockRequest) and the DosProtect* file API.
//!
//! OS/2's locks are mandatory: DosRead, DosWrite and DosSetFileSize
//! check them. The rules below come from IBM's kernel (OS2KRNLB,
//! filesys\lock.asm: Set_Block, Clr_Block, ScanLock, Chk_Block; the
//! FSD_Read / FSD_Write / w_NewSize callers; w_BigOper16 for the 32-bit
//! calls; ClrSFLock from MFTclose; w_realSetFileLocks and
//! DOS32CANCELLOCKREQUEST), studied with Ghidra:
//!
//! - A lock belongs to an open file instance (IBM's SFT: one DosOpen and
//!   every handle duplicated or inherited from it) *and* the process that
//!   set it. A child that inherited the handle does not own its parent's
//!   locks; a second DosOpen of the file in the same process does not own
//!   the first one's.
//! - Reading a range that overlaps another owner's exclusive lock, or
//!   writing a range that overlaps another owner's lock of either kind,
//!   fails with ERROR_LOCK_VIOLATION (33) and transfers nothing. The owner
//!   reads and writes its exclusive locks freely and reads its shared
//!   locks, but cannot write through its own shared lock either. There is
//!   no retry and no hard-error pop-up for OS/2 programs (IBM's
//!   Read_Lock_Violation retries and raises the hard error only for DOS
//!   sessions' compatibility-mode and FCB opens).
//! - The range checked is the requested one (position .. position+count),
//!   even past the end of the file. A 16-bit DosRead/DosWrite is one
//!   transfer, so a conflict anywhere fails all of it; a zero-byte 16-bit
//!   DosWrite checks 65535 bytes from the position. The 32-bit calls go
//!   to the file system in pieces of at most 64 KB that end at the 64 KB
//!   boundaries of the *buffer's* address (an aligned 64 KB piece is
//!   split 60 KB + 4 KB; buffers above 0x1FFF0000 go in 65535-byte
//!   pieces): the pieces before the first conflicting one are transferred
//!   and counted in the byte count returned with ERROR_LOCK_VIOLATION.
//!   A zero-byte 32-bit call checks nothing.
//! - DosSetFileSize(L) checks the range from the new size for as many
//!   bytes as the low 16 bits of the old size, as a write (an IBM quirk
//!   kept as it is; nothing is checked when those bits are 0).
//! - A new lock conflicts with every overlapping lock, the caller's own
//!   included, unless both are shared. With a timeout it waits for a lock
//!   to go (and gets ERROR_LOCK_VIOLATION when the time runs out,
//!   ERROR_INTERRUPT when a signal comes or DosCancelLockRequest cancels
//!   it, ERROR_INVALID_HANDLE when the handle was closed meanwhile).
//! - Unlocking needs a lock of the caller's with exactly that offset and
//!   length (ERROR_LOCK_VIOLATION otherwise); the unlock is done first and
//!   stays done when the lock that follows fails. Zero-length ranges are
//!   ignored; a call with neither range is ERROR_LOCK_VIOLATION; a range
//!   that runs past 4 GB in the 32-bit calls is ERROR_LOCK_VIOLATION.
//! - LOCK_ATOMIC (flags bit 1) needs both ranges, identical; the caller's
//!   lock of that range becomes shared or exclusive (flags bit 0) in
//!   place, failing at once (no wait) when another overlapping lock (one
//!   with a different range) is in the way.
//! - A process's locks on an open file go when it closes its last handle
//!   to that open file, and all of them when the process ends.
//!
//! Only the user-visible calls are checked: the loader, paging, the
//! block cache and memory-mapped I/O read files directly. DosCopy fails
//! with ERROR_LOCK_VIOLATION for a source with an exclusive lock or a
//! target with any lock. rs2 keeps 64-bit ranges for DosSetFileLocksL on
//! every volume (as a large-file file system does; IBM's FAT and HPFS
//! reject ranges above 4 GB with ERROR_INVALID_PARAMETER).
//!
//! The DosProtect* functions are the ordinary file functions with a
//! "file handle lock ID" for DLLs that protect their handles from the
//! application; rs2 hands out the ID but does not check it.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::{err, Ctx};
use crate::fs::{Handle, OpenFile};
use crate::sync::SpinLock;
use crate::task::process::Process;
use crate::task::WaitQueue;

pub const ERROR_LOCK_VIOLATION: u32 = 33;
const ERROR_CANCEL_VIOLATION: u32 = 173;
const FL_SHARED: u32 = 1;
const FL_ATOMIC: u32 = 2;

struct Lock {
    owner: Weak<OpenFile>,
    pid: u32,
    start: u64,
    /// Last byte (inclusive).
    end: u64,
    shared: bool,
}

/// A DosSetFileLocks call waiting for a conflicting lock to go.
struct Waiter {
    id: u64,
    pid: u32,
    hf: u32,
    start: u64,
    end: u64,
    cancelled: bool,
}

#[derive(Default)]
struct State {
    /// Newest first (unlock and LOCK_ATOMIC take the newest match).
    locks: Vec<Lock>,
    waiters: Vec<Waiter>,
}

/// The locks of one file, shared by all its opens (`OpenFile::locks`).
pub struct FileLocks {
    key: String,
    /// Number of locks: the read/write check is one load when it is 0.
    held: AtomicUsize,
    state: SpinLock<State>,
}

impl Drop for FileLocks {
    fn drop(&mut self) {
        TOTAL.fetch_sub(*self.held.get_mut(), Ordering::Relaxed);
        let mut m = FILES.lock();
        if m.get(&self.key).is_some_and(|w| w.strong_count() == 0) {
            m.remove(&self.key);
        }
    }
}

/// Upper-case full path -> the file's locks while it is open.
static FILES: SpinLock<BTreeMap<String, Weak<FileLocks>>> = SpinLock::new(BTreeMap::new());
/// Woken when a lock goes or a waiting request is cancelled. Never wake
/// it with a `State` lock held: its waiters' checks take that lock.
static RELEASED: WaitQueue = WaitQueue::new();
static NEXT_WAITER: AtomicU64 = AtomicU64::new(1);
/// Locks held in the whole system (process exit skips the search at 0).
static TOTAL: AtomicUsize = AtomicUsize::new(0);

/// The lock list for a file being opened (key: upper-case full path).
pub fn for_file(key: &str) -> Arc<FileLocks> {
    let mut m = FILES.lock();
    if let Some(f) = m.get(key).and_then(Weak::upgrade) {
        return f;
    }
    let f = Arc::new(FileLocks { key: String::from(key), held: AtomicUsize::new(0), state: SpinLock::new(State::default()) });
    m.insert(String::from(key), Arc::downgrade(&f));
    f
}

/// Every file's lock list (taken out of FILES before use: dropping the
/// last reference under its lock would deadlock in `FileLocks::drop`).
fn all_files() -> Vec<Arc<FileLocks>> {
    let v: Vec<Arc<FileLocks>> = FILES.lock().values().filter_map(Weak::upgrade).collect();
    v
}

fn same(a: &Weak<OpenFile>, b: &Arc<OpenFile>) -> bool {
    core::ptr::eq(a.as_ptr(), Arc::as_ptr(b))
}

fn owned(l: &Lock, of: &Arc<OpenFile>, pid: u32) -> bool {
    l.pid == pid && same(&l.owner, of)
}

fn overlaps(l: &Lock, start: u64, end: u64) -> bool {
    l.start <= end && start <= l.end && l.owner.strong_count() > 0
}

impl State {
    /// Drop the locks of open files that are gone; how many went.
    fn prune(&mut self) -> usize {
        let n = self.locks.len();
        self.locks.retain(|l| l.owner.strong_count() > 0);
        n - self.locks.len()
    }

    /// Does a read (or write) of [start, end] by (of, pid) hit a lock?
    /// (IBM's ScanLock as Chk_Block calls it.)
    fn blocks_io(&self, of: &Arc<OpenFile>, pid: u32, start: u64, end: u64, write: bool) -> bool {
        self.locks.iter().any(|l| {
            overlaps(l, start, end) && if owned(l, of, pid) { l.shared && write } else { write || !l.shared }
        })
    }

    /// Does a new lock of [start, end] conflict? The caller's own locks
    /// count too; two shared locks never conflict.
    fn blocks_lock(&self, start: u64, end: u64, shared: bool) -> bool {
        self.locks.iter().any(|l| overlaps(l, start, end) && !(shared && l.shared))
    }

    fn find_own(&self, of: &Arc<OpenFile>, pid: u32, start: u64, end: u64) -> Option<usize> {
        self.locks.iter().position(|l| owned(l, of, pid) && l.start == start && l.end == end)
    }

    fn cancelled(&self, id: u64) -> bool {
        self.waiters.iter().any(|w| w.id == id && w.cancelled)
    }
}

impl FileLocks {
    fn add_held(&self, n: isize) {
        if n > 0 {
            self.held.fetch_add(n as usize, Ordering::Relaxed);
            TOTAL.fetch_add(n as usize, Ordering::Relaxed);
        } else if n < 0 {
            self.held.fetch_sub((-n) as usize, Ordering::Relaxed);
            TOTAL.fetch_sub((-n) as usize, Ordering::Relaxed);
        }
    }

    /// Remove the locks matching `f`; whether any went.
    fn release(&self, mut f: impl FnMut(&Lock) -> bool) -> bool {
        let gone = {
            let mut st = self.state.lock();
            let n = st.locks.len();
            st.locks.retain(|l| l.owner.strong_count() > 0 && !f(l));
            n - st.locks.len()
        };
        self.add_held(-(gone as isize));
        gone > 0
    }
}

// ---- The read/write/size checks ----

/// How much of a DosRead/DosWrite of `len` bytes at `pos` through `of`
/// may go ahead, and whether a lock stops it there. `big` is a 32-bit
/// call (split into IBM's 64 KB pieces by the buffer address `buf`).
pub fn io_allowed(of: &Arc<OpenFile>, pid: u32, pos: u64, len: u32, buf: u32, write: bool, big: bool) -> (u32, bool) {
    if of.locks.held.load(Ordering::Relaxed) == 0 {
        return (len, false);
    }
    let st = of.locks.state.lock();
    if !big {
        let n = if len == 0 && write { 0xffff } else { len };
        if n == 0 || !st.blocks_io(of, pid, pos, pos + n as u64 - 1, write) {
            return (len, false);
        }
        return (0, true);
    }
    let (mut addr, mut done) = (buf, 0u32);
    while done < len {
        let rem = len - done;
        let chunk = if addr >= 0x1fff_0000 {
            rem.min(0xffff)
        } else if addr & 0xffff == 0 {
            if rem > 0xffff { 0xf000 } else { rem }
        } else {
            (0x1_0000 - (addr & 0xffff)).min(rem)
        };
        let at = pos + done as u64;
        if st.blocks_io(of, pid, at, at + chunk as u64 - 1, write) {
            return (done, true);
        }
        done += chunk;
        addr = addr.wrapping_add(chunk);
    }
    (len, false)
}

/// DosSetFileSize(L) from `old` to `new` bytes: allowed? (IBM's w_NewSize
/// checks [new, new + (old & 0xFFFF)) as a write.)
pub fn size_allowed(of: &Arc<OpenFile>, pid: u32, old: u64, new: u64) -> bool {
    let n = old & 0xffff;
    if n == 0 || of.locks.held.load(Ordering::Relaxed) == 0 {
        return true;
    }
    !of.locks.state.lock().blocks_io(of, pid, new, new + n - 1, true)
}

/// Whether a new open of the file (upper-case full path `key`, as DosCopy
/// does) would be stopped reading (or writing) [start, start+len).
pub fn path_blocked(key: &str, start: u64, len: u64, write: bool) -> bool {
    if len == 0 {
        return false;
    }
    let Some(f) = FILES.lock().get(key).and_then(Weak::upgrade) else { return false };
    let r = f.held.load(Ordering::Relaxed) != 0
        && f.state.lock().locks.iter().any(|l| overlaps(l, start, start + len - 1) && (write || !l.shared));
    drop(f);
    r
}

// ---- Release on close and exit ----

/// A handle of `p`'s to `of` was closed: the process's locks on it go
/// when that was its last handle to the open file (IBM's MFTclose ->
/// ClrSFLock).
pub fn handle_closed(p: &Process, of: &Arc<OpenFile>) {
    if of.locks.held.load(Ordering::Relaxed) == 0 {
        return;
    }
    let still = p.handles.lock().iter().any(|h| matches!(h, Some(Handle::File(x)) if Arc::ptr_eq(x, of)));
    if !still && of.locks.release(|l| owned(l, of, p.pid)) {
        RELEASED.wake_all();
    }
}

/// The process ended: all its locks go.
pub fn process_ended(pid: u32) {
    if TOTAL.load(Ordering::Relaxed) == 0 {
        return;
    }
    let mut any = false;
    for f in all_files() {
        if f.held.load(Ordering::Relaxed) != 0 && f.release(|l| l.pid == pid) {
            any = true;
        }
    }
    if any {
        RELEASED.wake_all();
    }
}

// ---- DosSetFileLocks ----

fn open_file(c: &Ctx, hf: u32) -> Result<Arc<OpenFile>, u32> {
    match super::file::get(c, hf) {
        Some(Handle::File(f)) => Ok(f),
        _ => Err(err::INVALID_HANDLE),
    }
}

/// A range from the caller: (offset, length), or None for a null pointer.
type Range = Option<(u64, u64)>;

/// [start, end] of a range, or None when it is empty or runs past the
/// limit (IBM's CheckRange).
fn span(off: u64, len: u64, limit: u64) -> Option<(u64, u64)> {
    let end = off.checked_add(len)?;
    (len != 0 && end <= limit).then_some((off, end - 1))
}

/// Unlock `unlock`, then lock `lock` (IBM's w_realSetFileLocks).
fn set_locks(c: &Ctx, hf: u32, unlock: Range, lock: Range, timeout: u32, flags: u32, limit: u64) -> u32 {
    let of = match open_file(c, hf) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let pid = c.process.pid;
    let fl = &of.locks;
    if flags & FL_ATOMIC != 0 {
        let (Some(u), Some(l)) = (unlock, lock) else { return ERROR_LOCK_VIOLATION };
        if u != l {
            return ERROR_LOCK_VIOLATION;
        }
        let Some((start, end)) = span(l.0, l.1, limit) else { return ERROR_LOCK_VIOLATION };
        let r = {
            let mut st = fl.state.lock();
            let gone = st.prune();
            fl.add_held(-(gone as isize));
            // Every other overlapping lock is in the way; one with
            // exactly this range is not (whoever holds it).
            if st.locks.iter().any(|x| overlaps(x, start, end) && !(x.start == start && x.end == end)) {
                ERROR_LOCK_VIOLATION
            } else if let Some(i) = st.find_own(&of, pid, start, end) {
                st.locks[i].shared = flags & FL_SHARED != 0;
                0
            } else {
                ERROR_LOCK_VIOLATION
            }
        };
        if r == 0 {
            RELEASED.wake_all();
        }
        return r;
    }
    let mut unlocked = false;
    if let Some((off, len)) = unlock.filter(|r| r.1 != 0) {
        let Some((start, end)) = span(off, len, limit) else { return ERROR_LOCK_VIOLATION };
        let found = {
            let mut st = fl.state.lock();
            match st.find_own(&of, pid, start, end) {
                Some(i) => {
                    st.locks.remove(i);
                    true
                }
                None => false,
            }
        };
        if !found {
            return ERROR_LOCK_VIOLATION;
        }
        fl.add_held(-1);
        RELEASED.wake_all();
        unlocked = true;
    }
    match lock.filter(|r| r.1 != 0) {
        Some((off, len)) => match span(off, len, limit) {
            Some((start, end)) => acquire(c, hf, &of, start, end, flags & FL_SHARED != 0, timeout),
            None => ERROR_LOCK_VIOLATION,
        },
        None if unlocked => 0,
        None => ERROR_LOCK_VIOLATION,
    }
}

/// Take a lock of [start, end], waiting up to `timeout` ms for
/// conflicting locks to go (IBM's Set_Block).
fn acquire(c: &Ctx, hf: u32, of: &Arc<OpenFile>, start: u64, end: u64, shared: bool, timeout: u32) -> u32 {
    let pid = c.process.pid;
    let fl = &of.locks;
    let deadline = if timeout == u32::MAX { None } else { Some(crate::irq::ticks() + (timeout as u64 * crate::irq::HZ).div_ceil(1000)) };
    loop {
        let id = NEXT_WAITER.fetch_add(1, Ordering::Relaxed);
        {
            let mut st = fl.state.lock();
            let gone = st.prune();
            fl.add_held(-(gone as isize));
            if !st.blocks_lock(start, end, shared) {
                st.locks.insert(0, Lock { owner: Arc::downgrade(of), pid, start, end, shared });
                fl.add_held(1);
                return 0;
            }
            if timeout == 0 {
                return ERROR_LOCK_VIOLATION;
            }
            st.waiters.push(Waiter { id, pid, hf, start, end, cancelled: false });
        }
        let ok = RELEASED.wait_until(deadline, || {
            let st = fl.state.lock();
            st.cancelled(id) || !st.blocks_lock(start, end, shared)
        });
        let cancelled = {
            let mut st = fl.state.lock();
            let x = st.cancelled(id);
            st.waiters.retain(|w| w.id != id);
            x
        };
        if cancelled {
            return err::INTERRUPT;
        }
        if !ok {
            // A signal or a kill; otherwise the time ran out.
            if c.thread.exit_requested() || c.thread.interrupted() {
                return err::INTERRUPT;
            }
            return ERROR_LOCK_VIOLATION;
        }
        // The handle must still be the open file we were locking.
        match super::file::get(c, hf) {
            Some(Handle::File(f)) if Arc::ptr_eq(&f, of) => {}
            _ => return err::INVALID_HANDLE,
        }
    }
}

/// A FILELOCK (two LONGs) at `p`.
fn range32(c: &Ctx, p: u32) -> Result<Range, u32> {
    if p == 0 {
        return Ok(None);
    }
    match (c.read_u32(p), c.read_u32(p + 4)) {
        (Some(off), Some(len)) => Ok(Some((off as u64, len as u64))),
        _ => Err(err::INVALID_PARAMETER),
    }
}

/// A FILELOCKL (two LONGLONGs) at `p`.
fn range64(c: &Ctx, p: u32) -> Result<Range, u32> {
    if p == 0 {
        return Ok(None);
    }
    let q = |a: u32| -> Option<i64> { Some((c.read_u32(a)? as u64 | (c.read_u32(a + 4)? as u64) << 32) as i64) };
    match (q(p), q(p + 8)) {
        (Some(off), Some(len)) if off >= 0 && len >= 0 => Ok(Some((off as u64, len as u64))),
        _ => Err(err::INVALID_PARAMETER),
    }
}

const LIMIT32: u64 = 1 << 32;
const LIMIT64: u64 = i64::MAX as u64 + 1;

/// DosSetFileLocks(hFile, pflUnlock, pflLock, timeout, flags)
pub fn dos_set_file_locks(c: &mut Ctx) -> u32 {
    let (hf, pu, pl, t, f) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    match (range32(c, pu), range32(c, pl)) {
        (Ok(u), Ok(l)) => set_locks(c, hf, u, l, t, f, LIMIT32),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

/// DosSetFileLocksL: the same with 64-bit FILELOCKL ranges.
pub fn dos_set_file_locks_l(c: &mut Ctx) -> u32 {
    let (hf, pu, pl, t, f) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    match (range64(c, pu), range64(c, pl)) {
        (Ok(u), Ok(l)) => set_locks(c, hf, u, l, t, f, LIMIT64),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

/// 16-bit DosFileLocks(hf, pUnlockRange, pLockRange): exclusive, no
/// wait; neither range is not an error here.
pub fn dos16_file_locks(c: &mut Ctx) -> u32 {
    let (hf, pu, pl) = (c.arg(0), c.ptr(1), c.ptr(2));
    let (u, l) = match (range32(c, pu), range32(c, pl)) {
        (Ok(u), Ok(l)) => (u, l),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    if u.is_none() && l.is_none() {
        return match open_file(c, hf) {
            Ok(_) => 0,
            Err(e) => e,
        };
    }
    set_locks(c, hf, u, l, 0, 0, LIMIT32)
}

/// DosCancelLockRequest(hFile, pflLock): requests of this process waiting
/// on that handle for exactly that range give up with ERROR_INTERRUPT.
/// ERROR_CANCEL_VIOLATION when there was none.
fn cancel(c: &Ctx, hf: u32, r: Result<Range, u32>) -> u32 {
    let of = match open_file(c, hf) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let (off, len) = match r {
        Ok(Some(r)) => r,
        Ok(None) => return err::INVALID_PARAMETER,
        Err(e) => return e,
    };
    let (start, end) = (off, off.wrapping_add(len).wrapping_sub(1));
    let pid = c.process.pid;
    let n = {
        let mut st = of.locks.state.lock();
        let mut n = 0;
        for w in st.waiters.iter_mut() {
            if w.pid == pid && w.hf == hf && w.start == start && w.end == end {
                w.cancelled = true;
                n += 1;
            }
        }
        n
    };
    if n == 0 {
        return ERROR_CANCEL_VIOLATION;
    }
    RELEASED.wake_all();
    0
}

pub fn dos_cancel_lock_request(c: &mut Ctx) -> u32 {
    let (hf, p) = (c.arg(0), c.arg(1));
    let r = range32(c, p);
    cancel(c, hf, r)
}

pub fn dos_cancel_lock_request_l(c: &mut Ctx) -> u32 {
    let (hf, p) = (c.arg(0), c.arg(1));
    let r = range64(c, p);
    cancel(c, hf, r)
}

/// Number of locks held (leak checks).
pub fn count() -> u32 {
    let files = all_files();
    let n: usize = files
        .iter()
        .map(|f| {
            let mut st = f.state.lock();
            let gone = st.prune();
            let n = st.locks.len();
            drop(st);
            f.add_held(-(gone as isize));
            n
        })
        .sum();
    drop(files);
    n as u32
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
