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

enum Kind {
    Event { posts: u32 },
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
}

struct Table {
    sems: BTreeMap<u32, Arc<Sem>>,
    next: u32,
}

static TABLE: SpinLock<Table> = SpinLock::new(Table { sems: BTreeMap::new(), next: 1 });

/// Handles: shared semaphores get bit 31 set (as OS/2 does), private ones
/// do not.
fn new_handle(shared: bool) -> u32 {
    let mut t = TABLE.lock();
    let h = t.next;
    t.next += 1;
    if shared { h | 0x8000_0000 } else { h | 0x0001_0000 }
}

fn get(c: &Ctx, h: u32) -> Option<Arc<Sem>> {
    let s = TABLE.lock().sems.get(&h).cloned()?;
    let pid = c.process.pid;
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
    TABLE.lock().sems.iter().find(|(_, s)| s.name.as_deref() == Some(name)).map(|(&h, s)| (h, s.clone()))
}

fn create(c: &mut Ctx, kind: Kind) -> u32 {
    let (pname, ph, attr) = (c.arg(0), c.arg(1), c.arg(2));
    let name = match name_arg(c, pname) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if let Some(n) = &name {
        if find_named(n).is_some() {
            return ERROR_DUPLICATE_NAME;
        }
    }
    let shared = name.is_some() || attr & DC_SEM_SHARED != 0;
    let h = new_handle(shared);
    let s = Arc::new(Sem {
        name,
        shared,
        creator: c.process.pid,
        kind: SpinLock::new(kind),
        waiters: WaitQueue::new(),
        opens: SpinLock::new(alloc::vec![(c.process.pid, 1)]),
    });
    TABLE.lock().sems.insert(h, s);
    c.write_u32(ph, h);
    0
}

fn open(c: &mut Ctx, want_event: Option<bool>) -> u32 {
    let (pname, ph) = (c.arg(0), c.arg(1));
    let (h, s) = if pname != 0 {
        let name = match name_arg(c, pname) {
            Ok(Some(n)) => n,
            Ok(None) => return err::INVALID_NAME,
            Err(e) => return e,
        };
        match find_named(&name) {
            Some(x) => x,
            None => return err::SEM_NOT_FOUND,
        }
    } else {
        let Some(h) = c.read_u32(ph) else { return err::INVALID_ADDRESS };
        match TABLE.lock().sems.get(&h).cloned() {
            Some(s) if s.shared => (h, s),
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
    let empty = {
        let mut o = s.opens.lock();
        if let Some(i) = o.iter().position(|(p, _)| *p == pid) {
            o[i].1 -= 1;
            if o[i].1 == 0 {
                o.remove(i);
            }
        }
        o.is_empty()
    };
    if empty {
        TABLE.lock().sems.remove(&h);
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
    create(c, Kind::Event { posts: posted as u32 })
}

pub fn dos_open_event_sem(c: &mut Ctx) -> u32 {
    open(c, Some(true))
}

pub fn dos_close_event_sem(c: &mut Ctx) -> u32 {
    close(c)
}

pub fn dos_post_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let r = match &mut *s.kind.lock() {
        Kind::Event { posts } => {
            if *posts >= 0xffff {
                ERROR_TOO_MANY_POSTS
            } else {
                *posts += 1;
                if *posts > 1 { ERROR_ALREADY_POSTED } else { 0 }
            }
        }
        _ => return err::INVALID_HANDLE,
    };
    s.waiters.wake_all();
    wake_muxwaits();
    r
}

pub fn dos_reset_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let mut k = s.kind.lock();
    let Kind::Event { posts } = &mut *k else { return err::INVALID_HANDLE };
    let n = *posts;
    *posts = 0;
    drop(k);
    c.write_u32(c.arg(1), n);
    if n == 0 { ERROR_ALREADY_RESET } else { 0 }
}

pub fn dos_query_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let n = match &*s.kind.lock() {
        Kind::Event { posts } => *posts,
        _ => return err::INVALID_HANDLE,
    };
    c.write_u32(c.arg(1), n);
    0
}

fn event_posted(s: &Sem) -> bool {
    matches!(&*s.kind.lock(), Kind::Event { posts } if *posts > 0)
}

pub fn dos_wait_event_sem(c: &mut Ctx) -> u32 {
    let Some(s) = get(c, c.arg(0)) else { return err::INVALID_HANDLE };
    let ms = c.arg(1);
    if ms == 0 {
        return if event_posted(&s) { 0 } else { ERROR_TIMEOUT };
    }
    if s.waiters.wait_until(deadline(ms), || event_posted(&s)) {
        0
    } else if c.thread.exit_requested() {
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
        result = try_mutex(&s, me);
        result.is_some()
    });
    match result {
        Some(r) => r,
        None if c.thread.exit_requested() => err::INTERRUPT,
        None => ERROR_TIMEOUT,
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
    let h = new_handle(shared);
    TABLE.lock().sems.insert(
        h,
        Arc::new(Sem {
            name,
            shared,
            creator: c.process.pid,
            kind: SpinLock::new(Kind::MuxWait { recs, all }),
            waiters: WaitQueue::new(),
            opens: SpinLock::new(alloc::vec![(c.process.pid, 1)]),
        }),
    );
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
                Kind::Event { posts } => *posts > 0,
                Kind::Mutex { owner, .. } => owner.is_none() || *owner == Some(me),
                _ => false,
            });
            if ready {
                for (s, _) in &sems {
                    if matches!(&*s.kind.lock(), Kind::Mutex { .. }) {
                        try_mutex(s, me);
                    }
                }
                *done = Some(sems.last().map_or(0, |x| x.1));
            }
            ready
        } else {
            for (s, u) in &sems {
                let hit = match &*s.kind.lock() {
                    Kind::Event { posts } => *posts > 0,
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
        None if c.thread.exit_requested() => err::INTERRUPT,
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
    } else if c.thread.exit_requested() {
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
pub fn post_event_handle(h: u32) {
    let Some(s) = TABLE.lock().sems.get(&h).cloned() else { return };
    if let Kind::Event { posts } = &mut *s.kind.lock() {
        if *posts < 0xffff {
            *posts += 1;
        }
    }
    s.waiters.wake_all();
    wake_muxwaits();
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
