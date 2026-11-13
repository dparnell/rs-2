//! Semaphores: 32-bit event, mutex and muxwait semaphores; 16-bit RAM and
//! system semaphores.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::irq;
use crate::sync::SpinLock;
use crate::task::WaitQueue;

const SEM_INDEFINITE_WAIT: u32 = 0xffff_ffff;
const DC_SEM_SHARED: u32 = 1;
const DCMW_WAIT_ANY: u32 = 2;
const DCMW_WAIT_ALL: u32 = 4;
const ERROR_TIMEOUT: u32 = 640;
const ERROR_SEM_OWNER_DIED: u32 = 105;
const ERROR_TOO_MANY_HANDLES: u32 = 290;
const ERROR_DUPLICATE_NAME: u32 = 285;
const ERROR_NOT_OWNER: u32 = 288;
const ERROR_ALREADY_POSTED: u32 = 299;
const ERROR_ALREADY_RESET: u32 = 300;
const ERROR_SEM_BUSY: u32 = 301;
const ERROR_TOO_MANY_POSTS: u32 = 298;
const ERROR_EMPTY_MUXWAIT: u32 = 286;
const ERROR_WRONG_TYPE: u32 = 292;
const ERROR_EXCL_SEM_ALREADY_OWNED: u32 = 101;
/// DosCreateEventSem flags (OS/2 4.5 kernels): a post releases one waiting
/// thread, or the semaphore resets itself when a wait is satisfied.
const DCE_POSTONE: u32 = 0x0800;
const DCE_AUTORESET: u32 = 0x1000;

enum Kind {
    /// `mode`: DCE_POSTONE / DCE_AUTORESET; `round` counts auto-reset posts
    /// that released the threads waiting at the time.
    Event { posts: u32, mode: u32, round: u32 },
    Mutex { owner: Option<(u32, u32)>, count: u32, died: bool },
    MuxWait { recs: Vec<(u32, u32)>, all: bool },
}

struct Sem {
    name: Option<String>,
    shared: bool,
    creator: u32,
    kind: SpinLock<Kind>,
    waiters: WaitQueue,
    /// Processes that have the semaphore open (with open counts).
    opens: SpinLock<Vec<(u32, u32)>>,
    /// Closed for good (taken out of the table): a thread that started to
    /// wait on it meanwhile gets ERROR_INVALID_HANDLE.
    closed: core::sync::atomic::AtomicBool,
}

struct Table {
    /// By key(): shared semaphores by handle, private ones by process
    /// and handle.
    sems: BTreeMap<u64, Arc<Sem>>,
    next: u32,
}

static TABLE: SpinLock<Table> = SpinLock::new(Table { sems: BTreeMap::new(), next: 1 });

const SHARED_BIT: u32 = 0x8000_0000;
const PRIVATE_BASE: u32 = 0x0001_0000;

/// Table key of handle `h` as process `pid` sees it.
fn key(pid: u32, h: u32) -> u64 {
    if h & SHARED_BIT != 0 { h as u64 } else { (pid as u64) << 32 | h as u64 }
}

/// Handles: shared semaphores get bit 31 set (as OS/2 does) and are
/// unique in the system; private ones are numbered per process, the
/// lowest free number first, as on OS/2 (kLIBC's fork recreates the
/// parent's private semaphores in the child by creating semaphores until
/// it gets the same handles).
fn new_handle(t: &mut Table, shared: bool, pid: u32) -> u32 {
    if shared {
        let h = t.next;
        t.next += 1;
        return h | SHARED_BIT;
    }
    let lo = (pid as u64) << 32;
    let mut want = PRIVATE_BASE + 1;
    for (&k, _) in t.sems.range(lo..lo + SHARED_BIT as u64) {
        if k as u32 != want {
            break;
        }
        want += 1;
    }
    want
}

/// Add a semaphore; returns its handle. A name is checked under the same
/// lock (two processes may create one name at once).
fn insert(shared: bool, pid: u32, s: Arc<Sem>) -> Result<u32, u32> {
    let mut t = TABLE.lock();
    if let Some(n) = &s.name {
        if find_named_in(&t, n).is_some() {
            return Err(ERROR_DUPLICATE_NAME);
        }
    }
    let h = new_handle(&mut t, shared, pid);
    t.sems.insert(key(pid, h), s);
    Ok(h)
}

fn get(c: &Ctx, h: u32) -> Option<Arc<Sem>> {
    let pid = c.process.pid;
    let s = TABLE.lock().sems.get(&key(pid, h)).cloned()?;
    if !s.shared && s.creator != pid {
        return None;
    }
    let open = s.opens.lock().iter().any(|&(p, _)| p == pid);
    open.then_some(s)
}

fn name_arg(c: &Ctx, p: u32) -> Result<Option<String>, u32> {
    if p == 0 {
        return Ok(None);
    }
    let n = c.read_str(p, 260).ok_or(err::INVALID_ADDRESS)?;
    let up = n.to_ascii_uppercase();
    if !up.starts_with("\\SEM32\\") {
        return Err(err::INVALID_NAME);
    }
    Ok(Some(up))
}

fn find_named(name: &str) -> Option<(u32, Arc<Sem>)> {
    find_named_in(&TABLE.lock(), name)
}

fn find_named_in(t: &Table, name: &str) -> Option<(u32, Arc<Sem>)> {
    t.sems.iter().find(|(_, s)| s.name.as_deref() == Some(name)).map(|(&h, s)| (h as u32, s.clone()))
}

fn create(c: &mut Ctx, kind: Kind) -> u32 {
    let (pname, ph, attr) = (c.arg(0), c.arg(1), c.arg(2));
    let name = match name_arg(c, pname) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let shared = name.is_some() || attr & DC_SEM_SHARED != 0;
    let s = Arc::new(Sem {
        name,
        shared,
        creator: c.process.pid,
        kind: SpinLock::new(kind),
        waiters: WaitQueue::new(),
        opens: SpinLock::new(alloc::vec![(c.process.pid, 1)]),
        closed: core::sync::atomic::AtomicBool::new(false),
    });
    let h = match insert(shared, c.process.pid, s) {
        Ok(h) => h,
        Err(e) => return e,
    };
    c.write_u32(ph, h);
    0
}

fn open(c: &mut Ctx, want_event: Option<bool>) -> u32 {
    let (pname, ph) = (c.arg(0), c.arg(1));
    let name = if pname != 0 {
        match name_arg(c, pname) {
            Ok(Some(n)) => Some(n),
            Ok(None) => return err::INVALID_NAME,
            Err(e) => return e,
        }
    } else {
        None
    };
    let by_handle = if name.is_none() {
        let Some(h) = c.read_u32(ph) else { return err::INVALID_ADDRESS };
        h
    } else {
        0
    };
    // Found and counted under the table lock: a close of the last open
    // elsewhere cannot take it out of the table in between.
    let t = TABLE.lock();
    let (h, s) = if let Some(name) = &name {
        match find_named_in(&t, name) {
            Some(x) => x,
            None => return err::SEM_NOT_FOUND,
        }
    } else {
        let h = by_handle;
        // By handle: a shared semaphore, or a private one of this
        // process (OS/2 counts the open; kLIBC's _fmutex_open opens its
        // own private event semaphores again this way).
        let pid = c.process.pid;
        match t.sems.get(&key(pid, h)).cloned() {
            Some(s) if s.shared || (s.creator == pid && s.opens.lock().iter().any(|&(p, _)| p == pid)) => (h, s),
            _ => return err::INVALID_HANDLE,
        }
    };
    let ok_type = match (&*s.kind.lock(), want_event) {
        (Kind::Event { .. }, Some(true)) | (Kind::Mutex { .. }, Some(false)) | (Kind::MuxWait { .. }, None) => true,
        _ => false,
    };
    if !ok_type {
        return ERROR_WRONG_TYPE;
    }
    let pid = c.process.pid;
    {
        let mut o = s.opens.lock();
        match o.iter_mut().find(|(p, _)| *p == pid) {
            Some(e) => {
                if e.1 >= 0xffff {
                    return ERROR_TOO_MANY_HANDLES;
                }
                e.1 += 1;
            }
            None => o.push((pid, 1)),
        }
    }
    drop(t);
    c.write_u32(ph, h);
    0
}

fn close(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    let Some(s) = get(c, h) else { return err::INVALID_HANDLE };
    let pid = c.process.pid;
    if let Kind::Mutex { owner: Some((op, _)), .. } = &*s.kind.lock() {
        if *op == pid {
            return ERROR_SEM_BUSY;
        }
    }
    // Threads of this process still wait on it.
    if s.waiters.has_waiter(|t| t.process().is_some_and(|p| p.pid == pid)) {
        return ERROR_SEM_BUSY;
    }
    // The last close takes it out of the table in the same critical
    // section (see open).
    let empty = {
        let mut t = TABLE.lock();
        let mut o = s.opens.lock();
        if let Some(i) = o.iter().position(|(p, _)| *p == pid) {
            o[i].1 -= 1;
            if o[i].1 == 0 {
                o.remove(i);
            }
        }
        let empty = o.is_empty();
        drop(o);
        if empty {
            t.sems.remove(&key(pid, h));
        }
        empty
    };
    if empty {
        s.closed.store(true, core::sync::atomic::Ordering::Release);
        s.waiters.wake_all();
    }
    0
}

/// Convert an OS/2 timeout (ms, -1 = forever) to a tick deadline.
fn deadline(ms: u32) -> Option<u64> {
    if ms == SEM_INDEFINITE_WAIT { None } else { Some(irq::ticks() + (ms as u64) * irq::HZ / 1000) }
}

// ---- Events ----

pub fn dos_create_event_sem(c: &mut Ctx) -> u32 {
    let posted = c.arg(3) != 0;
    let mode = c.arg(2) & (DCE_POSTONE | DCE_AUTORESET);
    create(c, Kind::Event { posts: posted as u32, mode, round: 0 })
}

pub fn dos_open_event_sem(c: &mut Ctx) -> u32 {
    open(c, Some(true))
}

pub fn dos_close_event_sem(c: &mut Ctx) -> u32 {
    close(c)
}

pub fn dos_post_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    post(&s).unwrap_or(err::INVALID_HANDLE)
}

/// Post an event semaphore: None if it is not one. Qt's mutexes rely on
/// DCE_POSTONE (each post hands the lock to one waiter); with it ignored
/// a stale post let a waiter through and Qt Designer crashed unlocking.
fn post(s: &Sem) -> Option<u32> {
    // Whether threads wait and the post itself are one step (under the
    // waiters' lock, the order the wait takes them in): a waiter timing
    // out in between must not make the post release nobody.
    let r = s.waiters.with_waiters(|waiting| {
        let mut k = s.kind.lock();
        let Kind::Event { posts, mode, round } = &mut *k else { return None };
        Some(if *mode & DCE_POSTONE == 0 && *mode & DCE_AUTORESET != 0 && waiting && *posts == 0 {
            // Release the threads waiting now; the semaphore stays reset.
            *round = round.wrapping_add(1);
            0
        } else if *posts >= 0xffff {
            ERROR_TOO_MANY_POSTS
        } else {
            *posts += 1;
            if *posts > 1 { ERROR_ALREADY_POSTED } else { 0 }
        })
    })?;
    s.waiters.wake_all();
    wake_muxwaits();
    Some(r)
}

/// Whether a wait that started at auto-reset generation `g0` is satisfied;
/// takes the post when the semaphore resets itself.
fn take_event(s: &Sem, g0: u32) -> bool {
    let mut k = s.kind.lock();
    let Kind::Event { posts, mode, round } = &mut *k else { return false };
    if *mode == 0 {
        return *posts > 0;
    }
    if *round != g0 {
        return true;
    }
    if *posts == 0 {
        return false;
    }
    if *mode & DCE_POSTONE != 0 {
        *posts -= 1;
    } else {
        *posts = 0;
    }
    true
}

fn event_round(s: &Sem) -> u32 {
    match &*s.kind.lock() {
        Kind::Event { round, .. } => *round,
        _ => 0,
    }
}

pub fn dos_reset_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let mut k = s.kind.lock();
    let Kind::Event { posts, .. } = &mut *k else { return err::INVALID_HANDLE };
    let n = *posts;
    *posts = 0;
    drop(k);
    c.write_u32(c.arg(1), n);
    if n == 0 { ERROR_ALREADY_RESET } else { 0 }
}

pub fn dos_query_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let n = match &*s.kind.lock() {
        Kind::Event { posts, .. } => *posts,
        _ => return err::INVALID_HANDLE,
    };
    c.write_u32(c.arg(1), n);
    0
}

fn event_posted(s: &Sem) -> bool {
    matches!(&*s.kind.lock(), Kind::Event { posts, .. } if *posts > 0)
}

pub fn dos_wait_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let ms = c.arg(1);
    let g0 = event_round(&s);
    if ms == 0 {
        return if take_event(&s, g0) { 0 } else { ERROR_TIMEOUT };
    }
    let closed = || s.closed.load(core::sync::atomic::Ordering::Acquire);
    if s.waiters.wait_until(deadline(ms), || closed() || take_event(&s, g0)) {
        if closed() { err::INVALID_HANDLE } else { 0 }
    } else if c.thread.exit_requested() || c.thread.interrupted() {
        err::INTERRUPT
    } else {
        ERROR_TIMEOUT
    }
}

// ---- Mutexes ----

pub fn dos_create_mutex_sem(c: &mut Ctx) -> u32 {
    let owned = c.arg(3) != 0;
    let owner = owned.then(|| (c.process.pid, c.thread.tid));
    create(c, Kind::Mutex { owner, count: owned as u32, died: false })
}

pub fn dos_open_mutex_sem(c: &mut Ctx) -> u32 {
    open(c, Some(false))
}

pub fn dos_close_mutex_sem(c: &mut Ctx) -> u32 {
    close(c)
}

/// Try to take a mutex for (pid, tid). Some(result) when decided.
fn try_mutex(s: &Sem, me: (u32, u32)) -> Option<u32> {
    let mut k = s.kind.lock();
    let Kind::Mutex { owner, count, died } = &mut *k else { return Some(err::INVALID_HANDLE) };
    // An owner that no longer exists counts as died.
    if let Some((op, ot)) = *owner {
        let alive = crate::task::process::by_pid(op)
            .is_some_and(|p| p.threads.lock().iter().any(|t| t.tid == ot && t.state() != crate::task::thread::State::Dead));
        if !alive {
            *owner = None;
            *count = 0;
            *died = true;
        }
    }
    match *owner {
        None => {
            *owner = Some(me);
            *count = 1;
            Some(if core::mem::take(died) { ERROR_SEM_OWNER_DIED } else { 0 })
        }
        Some(o) if o == me => {
            if *count >= 0xffff {
                return Some(err::TOO_MANY_SEM_REQUESTS);
            }
            *count += 1;
            Some(0)
        }
        Some(_) => None,
    }
}

pub fn dos_request_mutex_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let ms = c.arg(1);
    let me = (c.process.pid, c.thread.tid);
    if let Some(r) = try_mutex(&s, me) {
        return r;
    }
    if ms == 0 {
        return ERROR_TIMEOUT;
    }
    let mut result = None;
    s.waiters.wait_until(deadline(ms), || {
        if s.closed.load(core::sync::atomic::Ordering::Acquire) {
            result = Some(err::INVALID_HANDLE);
            return true;
        }
        result = try_mutex(&s, me);
        result.is_some()
    });
    match result {
        Some(r) => r,
        None if c.thread.exit_requested() || c.thread.interrupted() => err::INTERRUPT,
        None => ERROR_TIMEOUT,
    }
}

/// Undo one try_mutex (a muxwait that could not take every member).
fn give_back(s: &Sem, me: (u32, u32)) {
    let freed = {
        let mut k = s.kind.lock();
        let Kind::Mutex { owner, count, .. } = &mut *k else { return };
        if *owner != Some(me) {
            return;
        }
        *count = count.saturating_sub(1);
        if *count == 0 {
            *owner = None;
        }
        *count == 0
    };
    if freed {
        s.waiters.wake_all();
    }
}

pub fn dos_release_mutex_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let me = (c.process.pid, c.thread.tid);
    {
        let mut k = s.kind.lock();
        let Kind::Mutex { owner, count, .. } = &mut *k else { return err::INVALID_HANDLE };
        if *owner != Some(me) {
            return ERROR_NOT_OWNER;
        }
        *count -= 1;
        if *count > 0 {
            return 0;
        }
        *owner = None;
    }
    s.waiters.wake_all();
    wake_muxwaits();
    0
}

pub fn dos_query_mutex_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let (pid, tid, n, died) = match &*s.kind.lock() {
        Kind::Mutex { owner, count, died } => (owner.map_or(0, |o| o.0), owner.map_or(0, |o| o.1), *count, *died),
        _ => return err::INVALID_HANDLE,
    };
    c.write_u32(c.arg(1), pid);
    c.write_u32(c.arg(2), tid);
    c.write_u32(c.arg(3), n);
    if died { ERROR_SEM_OWNER_DIED } else { 0 }
}

// ---- MuxWait ----

/// Every muxwait is woken when any member changes; waiters re-check.
static MUX_WAITERS: WaitQueue = WaitQueue::new();

fn wake_muxwaits() {
    MUX_WAITERS.wake_all();
}

fn read_recs(c: &Ctx, p: u32, n: u32) -> Option<Vec<(u32, u32)>> {
    let mut v = Vec::new();
    for i in 0..n {
        v.push((c.read_u32(p + i * 8)?, c.read_u32(p + i * 8 + 4)?));
    }
    Some(v)
}

pub fn dos_create_mux_wait_sem(c: &mut Ctx) -> u32 {
    // DosCreateMuxWaitSem(pszName, phmux, cSemRec, pSemRec, flAttr)
    let (n, prec, attr) = (c.arg(2), c.arg(3), c.arg(4));
    let Some(recs) = read_recs(c, prec, n) else { return err::INVALID_ADDRESS };
    for &(h, _) in &recs {
        if get(c, h).is_none() {
            return err::INVALID_HANDLE;
        }
    }
    let all = attr & DCMW_WAIT_ALL != 0;
    if attr & (DCMW_WAIT_ANY | DCMW_WAIT_ALL) == 0 {
        return err::INVALID_PARAMETER;
    }
    // create() reads (name, ph, attr) from args 0, 1 and 4 via arg(2):
    // pass the shared bit through flAttr.
    let pname = c.arg(0);
    let ph = c.arg(1);
    let name = match name_arg(c, pname) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let shared = name.is_some() || attr & DC_SEM_SHARED != 0;
    let h = match insert(
        shared,
        c.process.pid,
        Arc::new(Sem {
            name,
            shared,
            creator: c.process.pid,
            kind: SpinLock::new(Kind::MuxWait { recs, all }),
            waiters: WaitQueue::new(),
            opens: SpinLock::new(alloc::vec![(c.process.pid, 1)]),
            closed: core::sync::atomic::AtomicBool::new(false),
        }),
    ) {
        Ok(h) => h,
        Err(e) => return e,
    };
    c.write_u32(ph, h);
    0
}

pub fn dos_open_mux_wait_sem(c: &mut Ctx) -> u32 {
    open(c, None)
}

pub fn dos_close_mux_wait_sem(c: &mut Ctx) -> u32 {
    close(c)
}

pub fn dos_wait_mux_wait_sem(c: &mut Ctx) -> u32 {
    let Some(m) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let (ms, puser) = (c.arg(1), c.arg(2));
    let (recs, all) = match &*m.kind.lock() {
        Kind::MuxWait { recs, all } => (recs.clone(), *all),
        _ => return err::INVALID_HANDLE,
    };
    if recs.is_empty() {
        return ERROR_EMPTY_MUXWAIT;
    }
    let me = (c.process.pid, c.thread.tid);
    let sems: Vec<(Arc<Sem>, u32)> = recs.iter().filter_map(|&(h, u)| get(c, h).map(|s| (s, u))).collect();
    let check = |done: &mut Option<u32>| -> bool {
        if all {
            // All events posted / all mutexes obtainable.
            let ready = sems.iter().all(|(s, _)| match &*s.kind.lock() {
                Kind::Event { posts, .. } => *posts > 0,
                Kind::Mutex { owner, .. } => owner.is_none() || *owner == Some(me),
                _ => false,
            });
            if !ready {
                return false;
            }
            // Another CPU may take a member between the check and here:
            // then the ones taken are given back and the wait goes on.
            let mut taken: Vec<&Arc<Sem>> = Vec::new();
            for (s, _) in &sems {
                if matches!(&*s.kind.lock(), Kind::Mutex { .. }) {
                    match try_mutex(s, me) {
                        Some(r) if r == 0 || r == ERROR_SEM_OWNER_DIED => taken.push(s),
                        _ => {
                            for t in taken {
                                give_back(t, me);
                            }
                            return false;
                        }
                    }
                }
            }
            *done = Some(sems.last().map_or(0, |x| x.1));
            true
        } else {
            for (s, u) in &sems {
                let hit = match &*s.kind.lock() {
                    Kind::Event { posts, .. } => *posts > 0,
                    Kind::Mutex { .. } => false,
                    _ => false,
                };
                if hit || (matches!(&*s.kind.lock(), Kind::Mutex { .. }) && try_mutex(s, me).is_some()) {
                    *done = Some(*u);
                    return true;
                }
            }
            false
        }
    };
    let mut done = None;
    if !check(&mut done) {
        if ms == 0 {
            return ERROR_TIMEOUT;
        }
        MUX_WAITERS.wait_until(deadline(ms), || check(&mut done));
    }
    match done {
        Some(u) => {
            c.write_u32(puser, u);
            0
        }
        None if c.thread.exit_requested() || c.thread.interrupted() => err::INTERRUPT,
        None => ERROR_TIMEOUT,
    }
}

pub fn dos_add_mux_wait_sem(c: &mut Ctx) -> u32 {
    let Some(m) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let Some(rec) = read_recs(c, c.arg(1), 1) else { return err::INVALID_ADDRESS };
    if get(c, rec[0].0).is_none() {
        return err::INVALID_HANDLE;
    }
    match &mut *m.kind.lock() {
        Kind::MuxWait { recs, .. } => {
            if recs.iter().any(|r| r.0 == rec[0].0) {
                return 284; // ERROR_DUPLICATE_HANDLE
            }
            recs.push(rec[0]);
            0
        }
        _ => err::INVALID_HANDLE,
    }
}

pub fn dos_delete_mux_wait_sem(c: &mut Ctx) -> u32 {
    let Some(m) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let h = c.arg(1);
    match &mut *m.kind.lock() {
        Kind::MuxWait { recs, .. } => {
            let n = recs.len();
            recs.retain(|r| r.0 != h);
            if recs.len() == n { err::INVALID_HANDLE } else { 0 }
        }
        _ => err::INVALID_HANDLE,
    }
}

pub fn dos_query_mux_wait_sem(c: &mut Ctx) -> u32 {
    // (hmux, pcSemRec, pSemRec, pflAttr)
    let Some(m) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let (pn, prec, pattr) = (c.arg(1), c.arg(2), c.arg(3));
    let (recs, all) = match &*m.kind.lock() {
        Kind::MuxWait { recs, all } => (recs.clone(), *all),
        _ => return err::INVALID_HANDLE,
    };
    let Some(have) = c.read_u32(pn) else { return err::INVALID_ADDRESS };
    c.write_u32(pn, recs.len() as u32);
    if (have as usize) < recs.len() {
        return err::BUFFER_OVERFLOW;
    }
    for (i, (h, u)) in recs.iter().enumerate() {
        c.write_u32(prec + i as u32 * 8, *h);
        c.write_u32(prec + i as u32 * 8 + 4, *u);
    }
    let shared = if m.shared { DC_SEM_SHARED } else { 0 };
    c.write_u32(pattr, shared | if all { DCMW_WAIT_ALL } else { DCMW_WAIT_ANY });
    0
}

/// A thread ended: mutexes it owned are free now, marked "owner died", and
/// their waiters (including muxwaits) must look again.
pub fn thread_ended(pid: u32, tid: u32) {
    let sems: Vec<Arc<Sem>> = TABLE.lock().sems.values().cloned().collect();
    let mut any = false;
    for s in sems {
        let freed = match &mut *s.kind.lock() {
            Kind::Mutex { owner, count, died } if *owner == Some((pid, tid)) => {
                *owner = None;
                *count = 0;
                *died = true;
                true
            }
            _ => false,
        };
        if freed {
            s.waiters.wake_all();
            any = true;
        }
    }
    // 16-bit system semaphores owned by the thread are released.
    for s in SYS16.lock().values_mut() {
        if s.owner == Some((pid, tid)) {
            s.owner = None;
            s.count = 0;
            any = true;
        }
    }
    if any {
        wake_muxwaits();
        SYS16_WAIT.wake_all();
        RAM16_WAIT.wake_all();
    }
}

/// A 32-bit semaphore as DosQuerySysState describes it.
pub struct Sem32Info {
    pub handle: u32,
    pub name: Option<String>,
    /// QS_DC_SEM_SHARED, QS_DCM_MUTEX_SEM, QS_DCE_EVENT_SEM, ... flags.
    pub flags: u16,
    /// Posts (event), request count (mutex) or records (muxwait).
    pub count: u16,
    /// Owning thread's tid (mutex) or waiters (muxwait).
    pub extra: u16,
    pub opens: Vec<(u32, u32)>,
}

pub fn snapshot32() -> Vec<Sem32Info> {
    let sems: Vec<(u32, Arc<Sem>)> = TABLE.lock().sems.iter().map(|(&h, s)| (h as u32, s.clone())).collect();
    let mut out = Vec::new();
    for (h, s) in sems {
        let mut flags = if s.shared { 0x0001 } else { 0 };
        let (count, extra) = match &*s.kind.lock() {
            Kind::Event { posts, .. } => {
                flags |= 0x0010;
                if *posts > 0 {
                    flags |= 0x0040;
                }
                (*posts as u16, 0)
            }
            Kind::Mutex { owner, count, died } => {
                flags |= 0x0008;
                if *died {
                    flags |= 0x0080;
                }
                (*count as u16, owner.map_or(0, |o| o.1 as u16))
            }
            Kind::MuxWait { recs, all } => {
                flags |= 0x0020 | if *all { 0x0004 } else { 0x0002 };
                (recs.len() as u16, 0)
            }
        };
        out.push(Sem32Info { handle: h, name: s.name.clone(), flags, count, extra, opens: s.opens.lock().clone() });
    }
    out
}

/// 16-bit system semaphores: (name, owner tid, flags, references, owner
/// request count, processes that have it open).
pub fn snapshot16() -> Vec<(String, u16, u8, u8, u8, Vec<u32>)> {
    SYS16
        .lock()
        .values()
        .map(|s| {
            let flags = if s.exclusive { 0x08 } else { 0 };
            (s.name.clone(), s.owner.map_or(0, |o| o.1 as u16), flags, s.opens.len().min(255) as u8, s.count.min(255) as u8, s.opens.clone())
        })
        .collect()
}

/// Number of 32-bit and 16-bit system semaphores (leak checks).
pub fn counts() -> (u32, u32) {
    (TABLE.lock().sems.len() as u32, SYS16.lock().len() as u32)
}

/// Release semaphore resources of a process that ended.
pub fn process_ended(pid: u32) {
    let mut t = TABLE.lock();
    t.sems.retain(|_, s| {
        let mut o = s.opens.lock();
        o.retain(|(p, _)| *p != pid);
        !o.is_empty()
    });
    drop(t);
    MUX_WAITERS.wake_all();
}

// ---- 16-bit semaphores ----
//
// A 16-bit semaphore handle is either the far address of a RAM semaphore
// (a ULONG in user memory: 0 = clear) or a system semaphore handle created
// by DosCreateSem. System handles use selector 0x0008 (RPL 0), which can
// never be a user data selector.

const SYS_SEL: u32 = 0x0008;

struct SysSem {
    name: String,
    exclusive: bool,
    owner: Option<(u32, u32)>,
    count: u32,
    set: bool,
    opens: Vec<u32>,
}

static SYS16: SpinLock<BTreeMap<u32, SysSem>> = SpinLock::new(BTreeMap::new());
static SYS16_WAIT: WaitQueue = WaitQueue::new();
static RAM16_WAIT: WaitQueue = WaitQueue::new();
/// Held while DosSemRequest tests and claims a RAM semaphore in user
/// memory (another CPU may be claiming the same one).
static RAM16: SpinLock<()> = SpinLock::new(());

enum Sem16 {
    Ram(u32),
    Sys(u32),
}

fn sem16(c: &Ctx, h: u32) -> Option<Sem16> {
    if h >> 16 == SYS_SEL {
        let idx = h & 0xffff;
        return SYS16.lock().contains_key(&idx).then_some(Sem16::Sys(idx));
    }
    let lin = c.far16_to_flat(h);
    (lin != 0 && lin != 0xffff_ffff).then_some(Sem16::Ram(lin))
}

fn ram_value(c: &Ctx, lin: u32) -> Option<u32> {
    c.read_u32(lin)
}

/// Is the semaphore currently set (owned)?
fn is_set(c: &Ctx, s: &Sem16) -> bool {
    match s {
        Sem16::Ram(l) => ram_value(c, *l).unwrap_or(0) != 0,
        Sem16::Sys(i) => SYS16.lock().get(i).is_some_and(|x| x.set || x.owner.is_some()),
    }
}

fn wait16(c: &Ctx, ms: u32, cond: impl FnMut() -> bool) -> u32 {
    let q = &RAM16_WAIT;
    if q.wait_until(deadline(ms), cond) {
        0
    } else if c.thread.exit_requested() || c.thread.interrupted() {
        err::INTERRUPT
    } else {
        err::SEM_TIMEOUT
    }
}

pub fn dos16_sem_set(c: &mut Ctx) -> u32 {
    match sem16(c, c.arg(0)) {
        Some(Sem16::Ram(l)) => {
            c.write_u32(l, 1);
            0
        }
        Some(Sem16::Sys(i)) => {
            if let Some(s) = SYS16.lock().get_mut(&i) {
                s.set = true;
            }
            0
        }
        None => err::INVALID_HANDLE,
    }
}

/// Clear a 16-bit RAM semaphore at a flat address (asynchronous I/O
/// completion).
pub fn clear_ram16(c: &Ctx, flat: u32) {
    c.write_u32(flat, 0);
    RAM16_WAIT.wake_all();
}

pub fn dos16_sem_clear(c: &mut Ctx) -> u32 {
    let me = (c.process.pid, c.thread.tid);
    match sem16(c, c.arg(0)) {
        Some(Sem16::Ram(l)) => {
            c.write_u32(l, 0);
        }
        Some(Sem16::Sys(i)) => {
            let mut t = SYS16.lock();
            if let Some(s) = t.get_mut(&i) {
                if s.exclusive && s.owner.is_some() && s.owner != Some(me) {
                    return ERROR_EXCL_SEM_ALREADY_OWNED;
                }
                if s.count > 1 {
                    s.count -= 1;
                } else {
                    s.owner = None;
                    s.count = 0;
                    s.set = false;
                }
            }
        }
        None => return err::INVALID_HANDLE,
    }
    RAM16_WAIT.wake_all();
    SYS16_WAIT.wake_all();
    0
}

pub fn dos16_sem_wait(c: &mut Ctx) -> u32 {
    let Some(s) = sem16(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let ms = c.arg(1);
    if !is_set(c, &s) {
        return 0;
    }
    if ms == 0 {
        return err::SEM_TIMEOUT;
    }
    wait16(c, ms, || !is_set(c, &s))
}

pub fn dos16_sem_set_wait(c: &mut Ctx) -> u32 {
    let r = dos16_sem_set(c);
    if r != 0 {
        return r;
    }
    let Some(s) = sem16(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let ms = c.arg(1);
    if ms == 0 {
        return err::SEM_TIMEOUT;
    }
    wait16(c, ms, || !is_set(c, &s))
}

/// Try to claim a 16-bit semaphore for DosSemRequest.
fn try_request(c: &Ctx, s: &Sem16, me: (u32, u32)) -> bool {
    match s {
        Sem16::Ram(l) => {
            let _g = RAM16.lock();
            if ram_value(c, *l).unwrap_or(1) == 0 {
                // Owner id in the semaphore word.
                c.write_u32(*l, me.0 << 16 | me.1);
                true
            } else {
                false
            }
        }
        Sem16::Sys(i) => {
            let mut t = SYS16.lock();
            let Some(x) = t.get_mut(i) else { return true };
            match x.owner {
                None if !x.set => {
                    x.owner = Some(me);
                    x.count = 1;
                    true
                }
                Some(o) if o == me && x.exclusive => {
                    x.count += 1;
                    true
                }
                _ => false,
            }
        }
    }
}

pub fn dos16_sem_request(c: &mut Ctx) -> u32 {
    let Some(s) = sem16(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let ms = c.arg(1);
    let me = (c.process.pid, c.thread.tid);
    if try_request(c, &s, me) {
        return 0;
    }
    if ms == 0 {
        return err::SEM_TIMEOUT;
    }
    wait16(c, ms, || try_request(c, &s, me))
}

pub fn dos16_mux_sem_wait(c: &mut Ctx) -> u32 {
    // DosMuxSemWait(pisemCleared, pmsxl {cmxs, {zero, hsem}[]}, lTimeOut)
    let (pidx, plist, ms) = (c.ptr(0), c.ptr(1), c.arg(2));
    let Some(n) = c.read_u16(plist) else { return err::INVALID_ADDRESS };
    let mut sems = Vec::new();
    for i in 0..n as u32 {
        let Some(h) = c.read_u32(plist + 2 + i * 6 + 2) else { return err::INVALID_ADDRESS };
        let Some(s) = sem16(c, h) else { return err::INVALID_HANDLE };
        sems.push(s);
    }
    let find = || sems.iter().position(|s| !is_set(c, s));
    let mut hit = find();
    if hit.is_none() {
        if ms == 0 {
            return err::SEM_TIMEOUT;
        }
        let r = wait16(c, ms, || {
            hit = find();
            hit.is_some()
        });
        if r != 0 {
            return r;
        }
    }
    c.write_u16(pidx, hit.unwrap_or(0) as u16);
    0
}

pub fn dos16_create_sem(c: &mut Ctx) -> u32 {
    // DosCreateSem(fExclusive, phsem, pszSemName)
    let (excl, ph, pname) = (c.arg(0) == 0, c.ptr(1), c.ptr(2));
    let Some(name) = c.read_str(pname, 260) else { return err::INVALID_ADDRESS };
    let name = name.to_ascii_uppercase();
    if !name.starts_with("\\SEM\\") {
        return err::INVALID_NAME;
    }
    let mut t = SYS16.lock();
    if t.values().any(|s| s.name == name) {
        return 183; // ERROR_ALREADY_EXISTS
    }
    let idx = (1..0xffff).find(|i| !t.contains_key(i)).unwrap_or(0);
    t.insert(idx, SysSem { name, exclusive: excl, owner: None, count: 0, set: false, opens: alloc::vec![c.process.pid] });
    drop(t);
    c.write_u32(ph, SYS_SEL << 16 | idx);
    0
}

pub fn dos16_open_sem(c: &mut Ctx) -> u32 {
    let (ph, pname) = (c.ptr(0), c.ptr(1));
    let Some(name) = c.read_str(pname, 260) else { return err::INVALID_ADDRESS };
    let name = name.to_ascii_uppercase();
    let mut t = SYS16.lock();
    let Some((&idx, s)) = t.iter_mut().find(|(_, s)| s.name == name) else { return err::SEM_NOT_FOUND };
    s.opens.push(c.process.pid);
    drop(t);
    c.write_u32(ph, SYS_SEL << 16 | idx);
    0
}

pub fn dos16_close_sem(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    if h >> 16 != SYS_SEL {
        return err::INVALID_HANDLE;
    }
    let idx = h & 0xffff;
    let mut t = SYS16.lock();
    let Some(s) = t.get_mut(&idx) else { return err::INVALID_HANDLE };
    if let Some(i) = s.opens.iter().position(|&p| p == c.process.pid) {
        s.opens.remove(i);
    }
    if s.opens.is_empty() {
        t.remove(&idx);
    }
    0
}


// ---- Hooks for timers (no calling process) ----

/// Post a 32-bit event semaphore by handle.
/// Post the named event semaphore `name` (upper case) if it exists.
pub fn post_named(name: &str) -> bool {
    let Some((h, _)) = find_named(name) else { return false };
    post_event_handle(0, h);
    true
}

/// The owner (pid, tid) of the named mutex semaphore `name`, if owned.
pub fn mutex_owner(name: &str) -> Option<(u32, u32)> {
    let (_, s) = find_named(name)?;
    match &*s.kind.lock() {
        Kind::Mutex { owner, .. } => *owner,
        _ => None,
    }
}

/// Post event semaphore `h` of process `pid` (any process for a shared
/// semaphore).
pub fn post_event_handle(pid: u32, h: u32) {
    let Some(s) = TABLE.lock().sems.get(&key(pid, h)).cloned() else { return };
    let _ = post(&s);
}

pub fn is_event_handle(c: &Ctx, h: u32) -> bool {
    get(c, h).is_some_and(|s| matches!(&*s.kind.lock(), Kind::Event { .. }))
}

/// Wake waiters of 16-bit RAM semaphores (after a timer cleared one).
pub fn wake_16bit() {
    RAM16_WAIT.wake_all();
}

/// Clear a 16-bit system semaphore by handle.
pub fn clear_sys16(h: u32) {
    if let Some(s) = SYS16.lock().get_mut(&(h & 0xffff)) {
        s.owner = None;
        s.count = 0;
        s.set = false;
    }
    RAM16_WAIT.wake_all();
}
