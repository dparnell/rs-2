//! Threads and synchronisation.
//!
//! Events, mutexes, semaphores, threads and processes are
//! [`Waitable`]s: their state is kept here under one lock, and a thread
//! that has to wait puts its own OS/2 event semaphore on the objects'
//! waiter lists (reset under the lock, so no wakeup is lost) and waits on
//! it; whoever changes an object posts its waiters. A process is not told
//! when a child ends, so waits on processes look every 20 ms.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::handles::{self, Obj, CURRENT_THREAD};
use crate::{err, fail, teb_get, teb_set, Global, Lock, FALSE, TEB_TID, TEB_TLS_SLOTS, TRUE};

pub const INFINITE: u32 = 0xffff_ffff;
pub const WAIT_OBJECT_0: u32 = 0;
pub const WAIT_ABANDONED_0: u32 = 0x80;
pub const WAIT_TIMEOUT: u32 = 258;
pub const WAIT_FAILED: u32 = 0xffff_ffff;
pub const STILL_ACTIVE: u32 = 259;

pub enum Kind {
    Event { manual: bool, signaled: bool },
    Mutex { owner: u32, count: u32 },
    Semaphore { count: i32, max: i32 },
    Thread { tid: u32, done: bool, code: u32 },
    Process { pid: u32, done: bool, code: u32 },
}

pub struct Waitable {
    pub kind: UnsafeCell<Kind>,
    waiters: UnsafeCell<Vec<u32>>,
}

unsafe impl Sync for Waitable {}
unsafe impl Send for Waitable {}

/// The lock over every Waitable's state.
static SYNC: Lock = Lock::new();

impl Waitable {
    pub fn new(k: Kind) -> Arc<Waitable> {
        Arc::new(Waitable { kind: UnsafeCell::new(k), waiters: UnsafeCell::new(Vec::new()) })
    }

    /// Change the state under the lock and wake the waiters.
    pub fn update<R>(&self, f: impl FnOnce(&mut Kind) -> R) -> R {
        let _g = SYNC.lock();
        let r = f(unsafe { &mut *self.kind.get() });
        for &ev in unsafe { &*self.waiters.get() } {
            unsafe { os2::DosPostEventSem(ev) };
        }
        r
    }

    pub fn read<R>(&self, f: impl FnOnce(&Kind) -> R) -> R {
        let _g = SYNC.lock();
        f(unsafe { &*self.kind.get() })
    }
}

fn tid() -> u32 {
    teb_get(TEB_TID)
}

/// Can a wait by `tid` be satisfied now? (Under the lock.)
fn ready(k: &Kind, tid: u32) -> bool {
    match k {
        Kind::Event { signaled, .. } => *signaled,
        Kind::Mutex { owner, .. } => *owner == 0 || *owner == tid,
        Kind::Semaphore { count, .. } => *count > 0,
        Kind::Thread { done, .. } | Kind::Process { done, .. } => *done,
    }
}

/// Take what a satisfied wait takes. (Under the lock.)
fn consume(k: &mut Kind, tid: u32) {
    match k {
        Kind::Event { manual: false, signaled } => *signaled = false,
        Kind::Mutex { owner, count } => {
            *owner = tid;
            *count += 1;
        }
        Kind::Semaphore { count, .. } => *count -= 1,
        _ => {}
    }
}

/// Look whether children waited on have ended (outside the lock).
fn poll_processes(objs: &[Arc<Waitable>]) -> bool {
    let mut any = false;
    for o in objs {
        let pid = o.read(|k| match k {
            Kind::Process { pid, done: false, .. } => Some(*pid),
            _ => None,
        });
        if let Some(pid) = pid {
            any = true;
            if let Some(code) = crate::process::child_ended(pid) {
                o.update(|k| {
                    if let Kind::Process { done, code: c, .. } = k {
                        *done = true;
                        *c = code;
                    }
                });
            }
        }
    }
    any
}

/// Each thread's OS/2 event semaphore for its waits, by thread ID.
static EVENTS: Global<Vec<u32>> = Global::new(Vec::new());

fn thread_event() -> u32 {
    let t = tid() as usize;
    let ev = EVENTS.with(|v| v.get(t).copied().unwrap_or(0));
    if ev != 0 {
        return ev;
    }
    let mut h = 0;
    unsafe { os2::DosCreateEventSem(core::ptr::null(), &mut h, 0, 0) };
    EVENTS.with(|v| {
        if v.len() <= t {
            v.resize(t + 1, 0);
        }
        v[t] = h;
    });
    h
}

fn now_ms() -> u32 {
    let mut ms = 0u32;
    unsafe { os2::DosQuerySysInfo(14, 14, &mut ms, 4) };
    ms
}

/// Wait for one (`all` false) or all of `objs`: WAIT_OBJECT_0 + i,
/// WAIT_TIMEOUT.
pub fn wait(objs: &[Arc<Waitable>], all: bool, timeout: u32) -> u32 {
    let me = tid();
    let start = now_ms();
    let has_process = objs.iter().any(|o| o.read(|k| matches!(k, Kind::Process { .. })));
    loop {
        if has_process {
            poll_processes(objs);
        }
        let ev;
        {
            let _g = SYNC.lock();
            let states: Vec<bool> = objs.iter().map(|o| ready(unsafe { &*o.kind.get() }, me)).collect();
            if all {
                if states.iter().all(|&r| r) {
                    for o in objs {
                        consume(unsafe { &mut *o.kind.get() }, me);
                    }
                    return WAIT_OBJECT_0;
                }
            } else if let Some(i) = states.iter().position(|&r| r) {
                consume(unsafe { &mut *objs[i].kind.get() }, me);
                return WAIT_OBJECT_0 + i as u32;
            }
            let elapsed = now_ms().wrapping_sub(start);
            if timeout != INFINITE && elapsed >= timeout {
                return WAIT_TIMEOUT;
            }
            ev = thread_event();
            let mut n = 0;
            unsafe { os2::DosResetEventSem(ev, &mut n) };
            for o in objs {
                unsafe { (*o.waiters.get()).push(ev) };
            }
        }
        let left = if timeout == INFINITE { INFINITE } else { timeout - now_ms().wrapping_sub(start).min(timeout) };
        let ms = if has_process { left.min(20) } else { left };
        unsafe { os2::DosWaitEventSem(ev, ms) };
        let _g = SYNC.lock();
        for o in objs {
            let w = unsafe { &mut *o.waiters.get() };
            if let Some(i) = w.iter().position(|&x| x == ev) {
                w.swap_remove(i);
            }
        }
    }
}

fn objects(hs: &[u32]) -> Option<Vec<Arc<Waitable>>> {
    hs.iter()
        .map(|&h| match h {
            crate::handles::CURRENT_PROCESS => Some(crate::process::current_process_object()),
            CURRENT_THREAD => current_thread_object(),
            _ => handles::waitable(h),
        })
        .collect()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WaitForSingleObject(h: u32, timeout: u32) -> u32 {
    let Some(o) = objects(&[h]) else { return fail(err::INVALID_HANDLE, WAIT_FAILED) };
    wait(&o, false, timeout)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WaitForSingleObjectEx(h: u32, timeout: u32, _alertable: i32) -> u32 {
    WaitForSingleObject(h, timeout)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WaitForMultipleObjects(n: u32, hs: *const u32, all: i32, timeout: u32) -> u32 {
    if n == 0 || n > 64 || hs.is_null() {
        return fail(err::INVALID_PARAMETER, WAIT_FAILED);
    }
    let hs = unsafe { core::slice::from_raw_parts(hs, n as usize) };
    let Some(o) = objects(hs) else { return fail(err::INVALID_HANDLE, WAIT_FAILED) };
    wait(&o, all != 0, timeout)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WaitForMultipleObjectsEx(n: u32, hs: *const u32, all: i32, timeout: u32, _alertable: i32) -> u32 {
    unsafe { WaitForMultipleObjects(n, hs, all, timeout) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Sleep(ms: u32) {
    unsafe { os2::DosSleep(ms) };
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SleepEx(ms: u32, _alertable: i32) -> u32 {
    unsafe { os2::DosSleep(ms) };
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SwitchToThread() -> i32 {
    unsafe { os2::DosSleep(0) };
    TRUE
}

// ---- Events, mutexes, semaphores ----

/// Named objects: (name, object), so that a second Create of a name opens
/// the first (in this process).
static NAMED: Global<Vec<(Vec<u8>, Arc<Waitable>)>> = Global::new(Vec::new());

fn named(name: *const u8, make: impl FnOnce() -> Arc<Waitable>) -> u32 {
    if name.is_null() {
        crate::set_error(0);
        return handles::add(Obj::Wait(make()));
    }
    let n = unsafe { crate::cstr(name) }.to_ascii_uppercase();
    let found = NAMED.with(|v| v.iter().find(|(x, _)| *x == n).map(|(_, o)| o.clone()));
    if let Some(o) = found {
        crate::set_error(err::ALREADY_EXISTS);
        return handles::add(Obj::Wait(o));
    }
    let o = make();
    NAMED.with(|v| v.push((n, o.clone())));
    crate::set_error(0);
    handles::add(Obj::Wait(o))
}

fn open_named(name: *const u8) -> u32 {
    let n = unsafe { crate::cstr(name) }.to_ascii_uppercase();
    match NAMED.with(|v| v.iter().find(|(x, _)| *x == n).map(|(_, o)| o.clone())) {
        Some(o) => handles::add(Obj::Wait(o)),
        None => fail(err::FILE_NOT_FOUND, 0),
    }
}

fn wide_name(name: *const u16) -> Vec<u8> {
    if name.is_null() {
        return Vec::new();
    }
    crate::z(&crate::text::from_wide(unsafe { crate::wstr(name) }))
}

fn name_ptr(v: &[u8]) -> *const u8 {
    if v.is_empty() { core::ptr::null() } else { v.as_ptr() }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateEventA(_sa: u32, manual: i32, initial: i32, name: *const u8) -> u32 {
    named(name, || Waitable::new(Kind::Event { manual: manual != 0, signaled: initial != 0 }))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateEventW(sa: u32, manual: i32, initial: i32, name: *const u16) -> u32 {
    let n = wide_name(name);
    CreateEventA(sa, manual, initial, name_ptr(&n))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenEventA(_access: u32, _inherit: i32, name: *const u8) -> u32 {
    open_named(name)
}

fn event_op(h: u32, f: impl FnOnce(&mut bool, bool)) -> i32 {
    let Some(o) = handles::waitable(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let ok = o.update(|k| match k {
        Kind::Event { signaled, manual } => {
            f(signaled, *manual);
            true
        }
        _ => false,
    });
    if ok { TRUE } else { fail(err::INVALID_HANDLE, FALSE) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetEvent(h: u32) -> i32 {
    event_op(h, |s, _| *s = true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ResetEvent(h: u32) -> i32 {
    event_op(h, |s, _| *s = false)
}

/// PulseEvent: wakes the waiters there are (approximately: signalled for
/// a moment).
#[unsafe(no_mangle)]
pub extern "stdcall" fn PulseEvent(h: u32) -> i32 {
    let r = event_op(h, |s, _| *s = true);
    unsafe { os2::DosSleep(0) };
    event_op(h, |s, _| *s = false);
    r
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateMutexA(_sa: u32, owned: i32, name: *const u8) -> u32 {
    let me = tid();
    named(name, || Waitable::new(Kind::Mutex { owner: if owned != 0 { me } else { 0 }, count: (owned != 0) as u32 }))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateMutexW(sa: u32, owned: i32, name: *const u16) -> u32 {
    let n = wide_name(name);
    CreateMutexA(sa, owned, name_ptr(&n))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenMutexA(_access: u32, _inherit: i32, name: *const u8) -> u32 {
    open_named(name)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ReleaseMutex(h: u32) -> i32 {
    let Some(o) = handles::waitable(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let me = tid();
    let ok = o.update(|k| match k {
        Kind::Mutex { owner, count } if *owner == me && *count > 0 => {
            *count -= 1;
            if *count == 0 {
                *owner = 0;
            }
            true
        }
        _ => false,
    });
    if ok { TRUE } else { fail(err::NOT_OWNER, FALSE) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateSemaphoreA(_sa: u32, initial: i32, max: i32, name: *const u8) -> u32 {
    if max <= 0 || initial < 0 || initial > max {
        return fail(err::INVALID_PARAMETER, 0);
    }
    named(name, || Waitable::new(Kind::Semaphore { count: initial, max }))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateSemaphoreW(sa: u32, initial: i32, max: i32, name: *const u16) -> u32 {
    let n = wide_name(name);
    CreateSemaphoreA(sa, initial, max, name_ptr(&n))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenSemaphoreA(_access: u32, _inherit: i32, name: *const u8) -> u32 {
    open_named(name)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ReleaseSemaphore(h: u32, n: i32, prev: *mut i32) -> i32 {
    let Some(o) = handles::waitable(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let r = o.update(|k| match k {
        Kind::Semaphore { count, max } if n > 0 && *count + n <= *max => {
            let p = *count;
            *count += n;
            Some(p)
        }
        _ => None,
    });
    match r {
        Some(p) => {
            if !prev.is_null() {
                unsafe { *prev = p };
            }
            TRUE
        }
        None => fail(err::TOO_MANY_POSTS, FALSE),
    }
}

// ---- Critical sections ----

/// CRITICAL_SECTION: DebugInfo, LockCount (-1 free), RecursionCount,
/// OwningThread, LockSemaphore (here a Waitable semaphore, made when
/// first needed), SpinCount.
#[repr(C)]
pub struct CriticalSection {
    debug: u32,
    lock_count: AtomicI32,
    recursion: u32,
    owner: AtomicU32,
    sem: AtomicU32,
    spin: u32,
}

fn cs_sem(cs: &CriticalSection) -> Arc<Waitable> {
    let p = cs.sem.load(Ordering::Acquire);
    if p != 0 {
        unsafe { Arc::increment_strong_count(p as *const Waitable) };
        return unsafe { Arc::from_raw(p as *const Waitable) };
    }
    let w = Waitable::new(Kind::Semaphore { count: 0, max: i32::MAX });
    let raw = Arc::into_raw(w.clone()) as u32;
    match cs.sem.compare_exchange(0, raw, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => w,
        Err(other) => {
            unsafe { drop(Arc::from_raw(raw as *const Waitable)) };
            unsafe { Arc::increment_strong_count(other as *const Waitable) };
            unsafe { Arc::from_raw(other as *const Waitable) }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn InitializeCriticalSection(cs: *mut CriticalSection) {
    unsafe {
        cs.write(CriticalSection { debug: 0, lock_count: AtomicI32::new(-1), recursion: 0, owner: AtomicU32::new(0), sem: AtomicU32::new(0), spin: 0 });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn InitializeCriticalSectionAndSpinCount(cs: *mut CriticalSection, spin: u32) -> i32 {
    unsafe {
        InitializeCriticalSection(cs);
        (*cs).spin = spin;
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn DeleteCriticalSection(cs: *mut CriticalSection) {
    let c = unsafe { &*cs };
    let p = c.sem.swap(0, Ordering::AcqRel);
    if p != 0 {
        unsafe { drop(Arc::from_raw(p as *const Waitable)) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn EnterCriticalSection(cs: *mut CriticalSection) {
    let c = unsafe { &mut *cs };
    let me = tid();
    if c.lock_count.fetch_add(1, Ordering::AcqRel) != -1 {
        if c.owner.load(Ordering::Acquire) == me {
            c.recursion += 1;
            return;
        }
        let s = cs_sem(c);
        wait(core::slice::from_ref(&s), false, INFINITE);
    }
    c.owner.store(me, Ordering::Release);
    c.recursion = 1;
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn TryEnterCriticalSection(cs: *mut CriticalSection) -> i32 {
    let c = unsafe { &mut *cs };
    let me = tid();
    if c.lock_count.compare_exchange(-1, 0, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        c.owner.store(me, Ordering::Release);
        c.recursion = 1;
        return TRUE;
    }
    if c.owner.load(Ordering::Acquire) == me {
        c.lock_count.fetch_add(1, Ordering::AcqRel);
        c.recursion += 1;
        return TRUE;
    }
    FALSE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LeaveCriticalSection(cs: *mut CriticalSection) {
    let c = unsafe { &mut *cs };
    if c.recursion > 1 {
        c.recursion -= 1;
        c.lock_count.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    c.recursion = 0;
    c.owner.store(0, Ordering::Release);
    if c.lock_count.fetch_sub(1, Ordering::AcqRel) > 0 {
        // Someone waits: let one in.
        let s = cs_sem(c);
        s.update(|k| {
            if let Kind::Semaphore { count, .. } = k {
                *count += 1;
            }
        });
    }
}

// ---- Interlocked ----

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn InterlockedIncrement(p: *const AtomicI32) -> i32 {
    unsafe { (*p).fetch_add(1, Ordering::SeqCst) + 1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn InterlockedDecrement(p: *const AtomicI32) -> i32 {
    unsafe { (*p).fetch_sub(1, Ordering::SeqCst) - 1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn InterlockedExchange(p: *const AtomicI32, v: i32) -> i32 {
    unsafe { (*p).swap(v, Ordering::SeqCst) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn InterlockedExchangeAdd(p: *const AtomicI32, v: i32) -> i32 {
    unsafe { (*p).fetch_add(v, Ordering::SeqCst) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn InterlockedCompareExchange(p: *const AtomicI32, v: i32, cmp: i32) -> i32 {
    match unsafe { (*p).compare_exchange(cmp, v, Ordering::SeqCst, Ordering::SeqCst) } {
        Ok(x) | Err(x) => x,
    }
}

// ---- TLS slots (TlsAlloc) ----

const TLS_SLOTS: u32 = 64;
pub const TLS_OUT_OF_INDEXES: u32 = 0xffff_ffff;
static TLS_USED: AtomicU32 = AtomicU32::new(0);
static TLS_USED_HI: AtomicU32 = AtomicU32::new(0);

fn tls_word(i: u32) -> &'static AtomicU32 {
    if i < 32 { &TLS_USED } else { &TLS_USED_HI }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TlsAlloc() -> u32 {
    for i in 0..TLS_SLOTS {
        let bit = 1 << (i % 32);
        if tls_word(i).fetch_or(bit, Ordering::AcqRel) & bit == 0 {
            // A new slot starts as NULL in every thread.
            for_each_thread_teb(|teb| unsafe { ((teb + TEB_TLS_SLOTS as u32 + i * 4) as *mut u32).write_volatile(0) });
            return i;
        }
    }
    fail(err::NOT_ENOUGH_MEMORY, TLS_OUT_OF_INDEXES)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TlsFree(i: u32) -> i32 {
    if i >= TLS_SLOTS {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    let bit = 1 << (i % 32);
    if tls_word(i).fetch_and(!bit, Ordering::AcqRel) & bit == 0 {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TlsGetValue(i: u32) -> u32 {
    if i >= TLS_SLOTS {
        return fail(err::INVALID_PARAMETER, 0);
    }
    crate::set_error(0);
    teb_get(TEB_TLS_SLOTS + i as usize * 4)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TlsSetValue(i: u32, v: u32) -> i32 {
    if i >= TLS_SLOTS {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    teb_set(TEB_TLS_SLOTS + i as usize * 4, v);
    TRUE
}

/// Call `f` with the TEB address of each thread of the process (the TEBs
/// are a page apart from the PEB, thread n's at page n).
fn for_each_thread_teb(f: impl Fn(u32)) {
    let peb = teb_get(crate::TEB_PEB);
    THREADS.with(|ts| {
        for t in ts.iter() {
            if let Some(tid) = t.read(|k| if let Kind::Thread { tid, done: false, .. } = k { Some(*tid) } else { None }) {
                f(peb + tid * 0x1000);
            }
        }
    });
    f(peb + 0x1000);
}

// ---- Threads ----

/// The threads CreateThread made (their objects, for ExitThread and the
/// thread's handle).
static THREADS: Global<Vec<Arc<Waitable>>> = Global::new(Vec::new());

struct Start {
    func: u32,
    param: u32,
    obj: Arc<Waitable>,
}

core::arch::global_asm!(
    // The thread's first code (DosCreateThread): realign the stack for
    // Rust, then thread_main(param).
    ".globl rs2_w32_thread_entry",
    "rs2_w32_thread_entry:",
    "mov eax, [esp+4]",
    "and esp, -16",
    "sub esp, 12",
    "push eax",
    "call rs2_w32_thread_main",
    "ud2",
);

unsafe extern "C" {
    fn rs2_w32_thread_entry(param: u32);
}

#[unsafe(no_mangle)]
extern "C" fn rs2_w32_thread_main(param: u32) -> ! {
    let s = unsafe { Box::from_raw(param as *mut Start) };
    unsafe { crate::Rs2W32ThreadAttach() };
    let code = crate::call(s.func, s.param, 0, 0, 0);
    drop(s);
    ExitThread(code);
}

pub fn current_thread_object() -> Option<Arc<Waitable>> {
    let me = tid();
    THREADS.with(|ts| {
        if let Some(t) = ts.iter().find(|t| t.read(|k| matches!(k, Kind::Thread { tid, done: false, .. } if *tid == me))) {
            return Some(t.clone());
        }
        // A thread CreateThread did not make (thread 1, OS/2 code's): its
        // object from now on.
        let o = Waitable::new(Kind::Thread { tid: me, done: false, code: STILL_ACTIVE });
        ts.push(o.clone());
        Some(o)
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateThread(_sa: u32, stack: u32, func: u32, param: u32, flags: u32, ptid: *mut u32) -> u32 {
    let obj = Waitable::new(Kind::Thread { tid: 0, done: false, code: STILL_ACTIVE });
    let start = Box::into_raw(Box::new(Start { func, param, obj: obj.clone() }));
    // The stack: a megabyte unless the program asks for more
    // (STACK_SIZE_PARAM_IS_A_RESERVATION or not), committed (OS/2's
    // STACK_COMMITTED): the TEB's limit is then its bottom.
    let size = stack.max(0x10_0000).next_multiple_of(0x1000);
    // CREATE_SUSPENDED (4) is OS/2's CREATE_SUSPENDED (1).
    let os2flags = if flags & 4 != 0 { 1 } else { 0 } | 2;
    let mut t = 0u32;
    let entry: os2::ThreadFn = unsafe { core::mem::transmute(rs2_w32_thread_entry as *const ()) };
    // The new thread may look itself up before the list has it.
    let rc = THREADS.with(|ts| {
        let rc = unsafe { os2::DosCreateThread(&mut t, entry, start as u32, os2flags, size) };
        if rc == 0 {
            obj.update(|k| {
                if let Kind::Thread { tid, .. } = k {
                    *tid = t;
                }
            });
            // Ended threads nobody holds a handle to go.
            ts.retain(|x| Arc::strong_count(x) > 1 || x.read(|k| !matches!(k, Kind::Thread { done: true, .. })));
            ts.push(obj.clone());
        }
        rc
    });
    if rc != 0 {
        unsafe { drop(Box::from_raw(start)) };
        return fail(crate::win_error(rc), 0);
    }
    if !ptid.is_null() {
        unsafe { *ptid = t };
    }
    handles::add(Obj::Wait(obj))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExitThread(code: u32) -> ! {
    unsafe { crate::Rs2W32ThreadDetach() };
    if let Some(o) = current_thread_object() {
        o.update(|k| {
            if let Kind::Thread { done, code: c, .. } = k {
                *done = true;
                *c = code;
            }
        });
    }
    unsafe { os2::DosExit(0, code) };
    loop {}
}

fn thread_tid(h: u32) -> Option<(u32, Arc<Waitable>)> {
    let o = if h == CURRENT_THREAD { current_thread_object()? } else { handles::waitable(h)? };
    let t = o.read(|k| if let Kind::Thread { tid, .. } = k { Some(*tid) } else { None })?;
    Some((t, o))
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetExitCodeThread(h: u32, code: *mut u32) -> i32 {
    let Some((_, o)) = thread_tid(h) else {
        if h == CURRENT_THREAD {
            unsafe { *code = STILL_ACTIVE };
            return TRUE;
        }
        return fail(err::INVALID_HANDLE, FALSE);
    };
    let c = o.read(|k| if let Kind::Thread { code, .. } = k { *code } else { STILL_ACTIVE });
    unsafe { *code = c };
    TRUE
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.237"]
    fn DosResumeThread(tid: u32) -> u32;
    #[link_name = "DOSCALLS.238"]
    fn DosSuspendThread(tid: u32) -> u32;
    #[link_name = "DOSCALLS.111"]
    fn DosKillThread(tid: u32) -> u32;
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ResumeThread(h: u32) -> u32 {
    let Some((t, _)) = thread_tid(h) else { return fail(err::INVALID_HANDLE, 0xffff_ffff) };
    if unsafe { DosResumeThread(t) } == 0 { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SuspendThread(h: u32) -> u32 {
    let Some((t, _)) = thread_tid(h) else { return fail(err::INVALID_HANDLE, 0xffff_ffff) };
    if unsafe { DosSuspendThread(t) } == 0 { 0 } else { fail(err::INVALID_HANDLE, 0xffff_ffff) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TerminateThread(h: u32, code: u32) -> i32 {
    let Some((t, o)) = thread_tid(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    if t == tid() {
        ExitThread(code);
    }
    unsafe { DosKillThread(t) };
    o.update(|k| {
        if let Kind::Thread { done, code: c, .. } = k {
            *done = true;
            *c = code;
        }
    });
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCurrentThreadId() -> u32 {
    tid()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCurrentThread() -> u32 {
    CURRENT_THREAD
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetThreadPriority(_h: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetThreadPriority(_h: u32, _p: i32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DisableThreadLibraryCalls(_h: u32) -> i32 {
    TRUE
}

/// SetThreadDescription(h, name): accepted (S_OK).
#[unsafe(no_mangle)]
pub extern "stdcall" fn SetThreadDescription(_h: u32, _name: *const u16) -> u32 {
    0
}
