//! REXXAPI.DLL: the REXX application programming interfaces that are
//! not the interpreter itself, with IBM's ordinals: external data queues
//! (RexxCreateQueue...), subcommand handlers, exits and external
//! functions (registration, query, and the undocumented call entries
//! REXX.DLL uses: RexxCallSubcom, RexxCallExit, RexxCallFunction), and
//! the macro space.
//!
//! Queues, DLL registrations and macros are system-wide (shared memory,
//! src/shared.rs); registrations of executable entry points belong to the
//! registering process. Queue names are upper-cased; "SESSION" is the
//! caller's session queue (session number from _RXQSESSION or the
//! process's session). The macro space keeps REXX source (IBM's keeps its
//! tokenized image), and RexxSaveMacroSpace files are rs2's own format.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
extern crate os2;

mod a16;
mod shared;

use alloc::vec::Vec;
use core::ptr::{copy_nonoverlapping, null, null_mut};

use os2::*;
use shared::{Guard, cstr, eq_nocase};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RxString {
    pub len: u32,
    pub ptr: *mut u8,
}

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// Exported entry points: realign the stack to 16 bytes (callers may
/// give only 4) and pass up to seven arguments on.
macro_rules! exports {
    ($($tramp:ident => $f:path;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "push esi", "push edi", "push ebx",
                "and esp, -16", "sub esp, 32",
                "mov eax, [ebp + 8]", "mov [esp], eax",
                "mov eax, [ebp + 12]", "mov [esp + 4], eax",
                "mov eax, [ebp + 16]", "mov [esp + 8], eax",
                "mov eax, [ebp + 20]", "mov [esp + 12], eax",
                "mov eax, [ebp + 24]", "mov [esp + 16], eax",
                "mov eax, [ebp + 28]", "mov [esp + 20], eax",
                "mov eax, [ebp + 32]", "mov [esp + 24], eax",
                "call {f}",
                "lea esp, [ebp - 12]", "pop ebx", "pop edi", "pop esi", "pop ebp", "ret",
                f = sym $f,
            );
        }
    )*};
}

exports! {
    __a_RexxCreateQueue => create_queue;
    __a_RexxDeleteQueue => delete_queue;
    __a_RexxAddQueue => add_queue;
    __a_RexxPullQueue => pull_queue;
    __a_RexxQueryQueue => query_queue;
    __a_RexxRegisterSubcomDll => register_subcom_dll;
    __a_RexxRegisterSubcomExe => register_subcom_exe;
    __a_RexxQuerySubcom => query_subcom;
    __a_RexxDeregisterSubcom => deregister_subcom;
    __a_RexxLoadSubcom => load_subcom;
    __a_RexxRegisterExitDll => register_exit_dll;
    __a_RexxRegisterExitExe => register_exit_exe;
    __a_RexxDeregisterExit => deregister_exit;
    __a_RexxQueryExit => query_exit;
    __a_RexxCallExit => call_exit;
    __a_RexxCallSubcom => call_subcom;
    __a_RexxRegisterFunctionDll => register_function_dll;
    __a_RexxRegisterFunctionExe => register_function_exe;
    __a_RexxDeregisterFunction => deregister_function;
    __a_RexxQueryFunction => query_function;
    __a_RexxCallFunction => call_function;
    __a_RexxAddMacro => add_macro;
    __a_RexxDropMacro => drop_macro;
    __a_RexxClearMacroSpace => clear_macro_space;
    __a_RexxLoadMacroSpace => load_macro_space;
    __a_RexxQueryMacro => query_macro;
    __a_RexxReorderMacro => reorder_macro;
    __a_RexxSaveMacroSpace => save_macro_space;
    __a_RexxExecuteMacroFunction => execute_macro_function;
}

// ---- queues ----

const RXQUEUE_STORAGE: u32 = 1;
const RXQUEUE_SIZE: u32 = 2;
const RXQUEUE_NOEMEM: u32 = 4;
const RXQUEUE_BADQNAME: u32 = 5;
const RXQUEUE_PRIORITY: u32 = 6;
const RXQUEUE_BADWAITFLAG: u32 = 7;
const RXQUEUE_EMPTY: u32 = 8;
const RXQUEUE_NOTREG: u32 = 9;
const RXQUEUE_MEMFAIL: u32 = 12;
const RXQUEUE_NOTINIT: u32 = 1000;

/// A valid queue name: 1 to 250 characters A-Z 0-9 . ? ! _ (any case).
fn valid_name(n: &[u8]) -> bool {
    !n.is_empty() && n.len() < 251 && n.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'?' | b'!' | b'_'))
}

/// The caller's session number (_RXQSESSION overrides it, as IBM's).
fn session() -> u32 {
    unsafe {
        let mut v: *const u8 = null();
        if DosScanEnv(b"_RXQSESSION\0".as_ptr(), &mut v) == 0 && !v.is_null() {
            let s = cstr(v);
            let mut n = 0u32;
            for &c in s.iter().take_while(|c| c.is_ascii_digit()) {
                n = n.wrapping_mul(10).wrapping_add((c - b'0') as u32);
            }
            return n;
        }
        // rs2 keeps the local information segment 64 KB after the PIB;
        // sgCurrent is at +8.
        let mut tib: *mut Tib = null_mut();
        let mut pib: *mut Pib = null_mut();
        DosGetInfoBlocks(&mut tib, &mut pib);
        if pib.is_null() {
            return 0;
        }
        *((pib as *const u8).add(0x1_0000 + 8) as *const u16) as u32
    }
}

fn find_queue(g: &Guard, name: &[u8]) -> *mut Queue {
    let root = g.root();
    let mut q = root.queues;
    if eq_nocase(name, b"SESSION") {
        let s = session();
        while !q.is_null() {
            if unsafe { (*q).session } == s + 1 {
                return q;
            }
            q = unsafe { (*q).next };
        }
        // the session queue always exists
        let nq = new_queue(g, b"SESSION", s + 1);
        return nq;
    }
    while !q.is_null() {
        unsafe {
            if (*q).session == 0 && eq_nocase(cstr((*q).name), name) {
                return q;
            }
            q = (*q).next;
        }
    }
    null_mut()
}

use shared::{Item, Queue};

fn new_queue(g: &Guard, name: &[u8], session: u32) -> *mut Queue {
    let q = shared::alloc(core::mem::size_of::<Queue>()) as *mut Queue;
    if q.is_null() {
        return q;
    }
    let up: Vec<u8> = name.iter().map(|c| c.to_ascii_uppercase()).collect();
    let n = shared::dup(&up);
    if n.is_null() {
        shared::free(q as *mut u8);
        return null_mut();
    }
    let root = g.root();
    unsafe {
        q.write(Queue { next: root.queues, first: null_mut(), last: null_mut(), count: 0, session, name: n });
    }
    root.queues = q;
    q
}

/// RexxCreateQueue(buffer, size, requested name, duplicate flag).
pub extern "C" fn create_queue(buf: *mut u8, size: u32, req: *const u8, dup: *mut u32) -> u32 {
    let Some(g) = shared::lock() else { return RXQUEUE_NOTINIT };
    if !dup.is_null() {
        unsafe { *dup = 0 };
    }
    let mut want: Option<&[u8]> = None;
    if !req.is_null() {
        let r = cstr(req);
        if !valid_name(r) || eq_nocase(r, b"SESSION") {
            return RXQUEUE_BADQNAME;
        }
        if find_queue(&g, r).is_null() {
            want = Some(r);
        } else if !dup.is_null() {
            unsafe { *dup = 1 };
        }
    }
    let name: Vec<u8> = match want {
        Some(n) => n.iter().map(|c| c.to_ascii_uppercase()).collect(),
        None => loop {
            // S<session>Q<counter>, as IBM's names its queues
            let root = g.root();
            root.counter = root.counter.wrapping_add(1);
            let n = alloc::format!("S{:02}Q{:010}", session() % 100, root.counter).into_bytes();
            if find_queue(&g, &n).is_null() {
                break n;
            }
        },
    };
    if (size as usize) < name.len() + 1 {
        return RXQUEUE_STORAGE;
    }
    if new_queue(&g, &name, 0).is_null() {
        return RXQUEUE_NOEMEM;
    }
    unsafe {
        copy_nonoverlapping(name.as_ptr(), buf, name.len());
        *buf.add(name.len()) = 0;
    }
    0
}

fn free_items(q: *mut Queue) {
    unsafe {
        let mut it = (*q).first;
        while !it.is_null() {
            let n = (*it).next;
            shared::free(it as *mut u8);
            it = n;
        }
        (*q).first = null_mut();
        (*q).last = null_mut();
        (*q).count = 0;
    }
}

pub extern "C" fn delete_queue(name: *const u8) -> u32 {
    let n = cstr(name);
    if !valid_name(n) {
        return RXQUEUE_BADQNAME;
    }
    let Some(g) = shared::lock() else { return RXQUEUE_NOTINIT };
    let q = find_queue(&g, n);
    if q.is_null() {
        return RXQUEUE_NOTREG;
    }
    free_items(q);
    if eq_nocase(n, b"SESSION") {
        // the session queue is emptied, not removed
        return 0;
    }
    let root = g.root();
    let mut link: *mut *mut Queue = &mut root.queues;
    unsafe {
        while !(*link).is_null() {
            if *link == q {
                *link = (*q).next;
                shared::free((*q).name);
                shared::free(q as *mut u8);
                break;
            }
            link = &mut (**link).next;
        }
    }
    0
}

pub extern "C" fn query_queue(name: *const u8, count: *mut u32) -> u32 {
    let n = cstr(name);
    if !valid_name(n) {
        return RXQUEUE_BADQNAME;
    }
    let Some(g) = shared::lock() else { return RXQUEUE_NOTINIT };
    let q = find_queue(&g, n);
    if q.is_null() {
        return RXQUEUE_NOTREG;
    }
    if !count.is_null() {
        unsafe { *count = (*q).count };
    }
    0
}

/// RexxAddQueue(name, data, flag): 0 FIFO, 1 LIFO.
pub extern "C" fn add_queue(name: *const u8, data: *const RxString, flag: u32) -> u32 {
    let n = cstr(name);
    if !valid_name(n) {
        return RXQUEUE_BADQNAME;
    }
    if flag > 1 {
        return RXQUEUE_PRIORITY;
    }
    if data.is_null() {
        return RXQUEUE_SIZE;
    }
    let d = unsafe { *data };
    if d.len > 0x10_0000 {
        return RXQUEUE_SIZE;
    }
    let Some(g) = shared::lock() else { return RXQUEUE_NOTINIT };
    let q = find_queue(&g, n);
    if q.is_null() {
        return RXQUEUE_NOTREG;
    }
    let it = shared::alloc(core::mem::size_of::<Item>() + d.len as usize) as *mut Item;
    if it.is_null() {
        return RXQUEUE_NOEMEM;
    }
    let mut dt = DateTime::default();
    unsafe {
        DosGetDateTime(&mut dt);
        let mut time = [0u8; 12];
        copy_nonoverlapping(&dt as *const DateTime as *const u8, time.as_mut_ptr(), 11);
        it.write(Item { next: null_mut(), len: d.len, time });
        if d.len > 0 && !d.ptr.is_null() {
            copy_nonoverlapping(d.ptr, (it as *mut u8).add(core::mem::size_of::<Item>()), d.len as usize);
        }
        if (*q).first.is_null() {
            (*q).first = it;
            (*q).last = it;
        } else if flag == 1 {
            (*it).next = (*q).first;
            (*q).first = it;
        } else {
            (*(*q).last).next = it;
            (*q).last = it;
        }
        (*q).count += 1;
    }
    drop(g);
    shared::post();
    0
}

/// RexxPullQueue(name, data, time stamp, wait flag).
pub extern "C" fn pull_queue(name: *const u8, data: *mut RxString, time: *mut u8, wait: u32) -> u32 {
    if wait > 1 {
        return RXQUEUE_BADWAITFLAG;
    }
    let n = cstr(name);
    if !valid_name(n) {
        return RXQUEUE_BADQNAME;
    }
    loop {
        let Some(g) = shared::lock() else { return RXQUEUE_NOTINIT };
        let q = find_queue(&g, n);
        if q.is_null() {
            return RXQUEUE_NOTREG;
        }
        let it = unsafe { (*q).first };
        if it.is_null() {
            drop(g);
            if wait == 0 {
                return RXQUEUE_EMPTY;
            }
            shared::wait();
            continue;
        }
        unsafe {
            (*q).first = (*it).next;
            if (*q).first.is_null() {
                (*q).last = null_mut();
            }
            (*q).count -= 1;
            let len = (*it).len;
            let src = (it as *const u8).add(core::mem::size_of::<Item>());
            if !data.is_null() {
                let d = &mut *data;
                if d.ptr.is_null() || d.len < len {
                    let mut p: *mut u8 = null_mut();
                    if DosAllocMem(&mut p, len.max(1), PAG_COMMIT | PAG_READ | PAG_WRITE) != 0 {
                        shared::free(it as *mut u8);
                        return RXQUEUE_MEMFAIL;
                    }
                    d.ptr = p;
                }
                copy_nonoverlapping(src, d.ptr, len as usize);
                d.len = len;
            }
            if !time.is_null() {
                copy_nonoverlapping((*it).time.as_ptr(), time, 11);
            }
            shared::free(it as *mut u8);
        }
        return 0;
    }
}

// ---- registrations ----

pub(crate) const KIND_FUNC: u32 = 1;
pub(crate) const KIND_SUBCOM: u32 = 2;
pub(crate) const KIND_EXIT: u32 = 3;

/// Registrations of entry points in executables (this process's).
struct Local {
    kind: u32,
    name: Vec<u8>,
    entry: usize,
    user: [u8; 8],
    /// A 16-bit procedure (16:16 address), registered by a 16-bit caller.
    bits16: bool,
}

static mut LOCAL: Vec<Local> = Vec::new();
/// Resolved DLL entry points of this process: (kind, name, module) ->
/// address.
static mut RESOLVED: Vec<(u32, Vec<u8>, Vec<u8>, usize)> = Vec::new();

#[allow(static_mut_refs)]
fn locals() -> &'static mut Vec<Local> {
    unsafe { &mut LOCAL }
}

fn upper(s: &[u8]) -> Vec<u8> {
    s.iter().map(|c| c.to_ascii_uppercase()).collect()
}

fn find_local(kind: u32, name: &[u8]) -> Option<usize> {
    locals().iter().position(|l| l.kind == kind && eq_nocase(&l.name, name))
}

fn find_dll(g: &Guard, kind: u32, name: &[u8], module: Option<&[u8]>) -> *mut shared::Reg {
    let mut r = g.root().regs;
    while !r.is_null() {
        unsafe {
            if (*r).kind == kind && eq_nocase(cstr((*r).name), name) && module.is_none_or(|m| eq_nocase(cstr((*r).module), m)) {
                return r;
            }
            r = (*r).next;
        }
    }
    null_mut()
}

fn pid() -> u32 {
    let mut tib: *mut Tib = null_mut();
    let mut pib: *mut Pib = null_mut();
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        if pib.is_null() { 0 } else { (*pib).pid }
    }
}

fn opt(p: *const u8) -> Option<&'static [u8]> {
    if p.is_null() {
        None
    } else {
        let s = cstr(p);
        if s.is_empty() { None } else { Some(s) }
    }
}

fn user_of(p: *const u8) -> [u8; 8] {
    let mut u = [0u8; 8];
    if !p.is_null() {
        unsafe { copy_nonoverlapping(p, u.as_mut_ptr(), 8) };
    }
    u
}

/// Register a DLL entry. Return codes: 0, 10 (a registration of that name
/// exists already: for functions nothing is registered), 1002.
pub(crate) fn register_dll(kind: u32, name: *const u8, module: *const u8, proc_: *const u8, user: *const u8, drop: u32) -> u32 {
    let (Some(n), Some(m), Some(p)) = (opt(name), opt(module), opt(proc_)) else { return 1003 };
    let Some(g) = shared::lock() else { return 1004 };
    let exists = !find_dll(&g, kind, n, None).is_null() || find_local(kind, n).is_some();
    if exists && kind == KIND_FUNC {
        return 10;
    }
    if !find_dll(&g, kind, n, Some(m)).is_null() {
        return 10;
    }
    let r = shared::alloc(core::mem::size_of::<shared::Reg>()) as *mut shared::Reg;
    if r.is_null() {
        return 1002;
    }
    let root = g.root();
    unsafe {
        r.write(shared::Reg {
            next: root.regs,
            kind,
            name: shared::dup(&upper(n)),
            module: shared::dup(&upper(m)),
            proc_: shared::dup(p),
            user: user_of(user),
            drop,
            pid: pid(),
        });
    }
    root.regs = r;
    if exists { 10 } else { 0 }
}

pub(crate) fn register_exe(kind: u32, name: *const u8, entry: usize, user: *const u8, bits16: bool) -> u32 {
    let Some(n) = opt(name) else { return 1003 };
    if entry == 0 {
        return 1003;
    }
    if find_local(kind, n).is_some() {
        return if kind == KIND_FUNC { 10 } else { 10 };
    }
    let dll_exists = shared::lock().is_some_and(|g| !find_dll(&g, kind, n, None).is_null());
    if dll_exists && kind == KIND_FUNC {
        return 10;
    }
    locals().push(Local { kind, name: upper(n), entry, user: user_of(user), bits16 });
    if dll_exists { 10 } else { 0 }
}

pub(crate) fn deregister(kind: u32, name: *const u8, module: *const u8) -> u32 {
    let Some(n) = opt(name) else { return 1003 };
    let m = opt(module);
    if m.is_none() {
        if let Some(i) = find_local(kind, n) {
            locals().remove(i);
            return 0;
        }
    }
    let Some(g) = shared::lock() else { return 1004 };
    let r = find_dll(&g, kind, n, m);
    if r.is_null() {
        return 30;
    }
    unsafe {
        if (*r).drop == 1 && (*r).pid != pid() {
            return 40;
        }
        let root = g.root();
        let mut link: *mut *mut shared::Reg = &mut root.regs;
        while !(*link).is_null() {
            if *link == r {
                *link = (*r).next;
                break;
            }
            link = &mut (**link).next;
        }
        shared::free((*r).name);
        shared::free((*r).module);
        shared::free((*r).proc_);
        shared::free(r as *mut u8);
    }
    0
}

/// The entry point of a registration: Ok((address, user area, 16-bit)).
fn resolve(kind: u32, name: &[u8], module: Option<&[u8]>) -> Result<(usize, [u8; 8], bool), u32> {
    if module.is_none() {
        if let Some(i) = find_local(kind, name) {
            let l = &locals()[i];
            return Ok((l.entry, l.user, l.bits16));
        }
    }
    let (m, p, user) = {
        let Some(g) = shared::lock() else { return Err(1004) };
        let r = find_dll(&g, kind, name, module);
        if r.is_null() {
            return Err(30);
        }
        unsafe { (cstr((*r).module).to_vec(), cstr((*r).proc_).to_vec(), (*r).user) }
    };
    #[allow(static_mut_refs)]
    let cache = unsafe { &mut RESOLVED };
    let key = upper(name);
    if let Some(c) = cache.iter().find(|c| c.0 == kind && c.1 == key && c.2 == m) {
        return Ok((c.3, user, false));
    }
    let mut mz = m.clone();
    mz.push(0);
    let mut fail = [0u8; 32];
    let mut h = 0u32;
    if unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, mz.as_ptr(), &mut h) } != 0 {
        return Err(50);
    }
    let mut addr = 0usize;
    let mut pz = p.clone();
    pz.push(0);
    let mut rc = unsafe { DosQueryProcAddr(h, 0, pz.as_ptr(), &mut addr) };
    if rc != 0 {
        let mut u = upper(&p);
        u.push(0);
        rc = unsafe { DosQueryProcAddr(h, 0, u.as_ptr(), &mut addr) };
    }
    if rc != 0 {
        return Err(127);
    }
    cache.push((kind, key, m, addr));
    Ok((addr, user, false))
}

pub(crate) fn query(kind: u32, name: *const u8, module: *const u8, flag: *mut u16, user: *mut u8) -> u32 {
    let Some(n) = opt(name) else { return 1003 };
    let m = opt(module);
    let mut found: Option<[u8; 8]> = None;
    if m.is_none() {
        if let Some(i) = find_local(kind, n) {
            found = Some(locals()[i].user);
        }
    }
    if found.is_none() {
        let Some(g) = shared::lock() else { return 1004 };
        let r = find_dll(&g, kind, n, m);
        if !r.is_null() {
            found = Some(unsafe { (*r).user });
        }
    }
    if !flag.is_null() {
        unsafe { *flag = found.is_some() as u16 };
    }
    match found {
        Some(u) => {
            if !user.is_null() {
                unsafe { copy_nonoverlapping(u.as_ptr(), user, 8) };
            }
            0
        }
        None => 30,
    }
}

pub extern "C" fn register_subcom_dll(name: *const u8, module: *const u8, proc_: *const u8, user: *const u8, drop: u32) -> u32 {
    register_dll(KIND_SUBCOM, name, module, proc_, user, drop)
}

pub extern "C" fn register_subcom_exe(name: *const u8, entry: usize, user: *const u8) -> u32 {
    register_exe(KIND_SUBCOM, name, entry, user, false)
}

/// RexxQuerySubcom(env, dll, flag, user): IBM's returns 0 with the flag
/// set when found, 30 otherwise.
pub extern "C" fn query_subcom(name: *const u8, module: *const u8, flag: *mut u16, user: *mut u8) -> u32 {
    query(KIND_SUBCOM, name, module, flag, user)
}

pub extern "C" fn deregister_subcom(name: *const u8, module: *const u8) -> u32 {
    deregister(KIND_SUBCOM, name, module)
}

pub extern "C" fn load_subcom(name: *const u8, module: *const u8) -> u32 {
    let Some(n) = opt(name) else { return 1003 };
    match resolve(KIND_SUBCOM, n, opt(module)) {
        Ok(_) => 0,
        Err(e) => e,
    }
}

pub extern "C" fn register_exit_dll(name: *const u8, module: *const u8, proc_: *const u8, user: *const u8, drop: u32) -> u32 {
    register_dll(KIND_EXIT, name, module, proc_, user, drop)
}

pub extern "C" fn register_exit_exe(name: *const u8, entry: usize, user: *const u8) -> u32 {
    register_exe(KIND_EXIT, name, entry, user, false)
}

pub extern "C" fn deregister_exit(name: *const u8, module: *const u8) -> u32 {
    deregister(KIND_EXIT, name, module)
}

pub extern "C" fn query_exit(name: *const u8, module: *const u8, flag: *mut u16, user: *mut u8) -> u32 {
    query(KIND_EXIT, name, module, flag, user)
}

type ExitHandler = unsafe extern "C" fn(i32, i32, *mut u8) -> i32;
type SubcomHandler = unsafe extern "C" fn(*const RxString, *mut u16, *mut RxString) -> u32;
type FunctionHandler = unsafe extern "C" fn(*const u8, u32, *const RxString, *const u8, *mut RxString) -> u32;

/// RexxCallExit(name, dll, function, subfunction, parameters): the
/// handler's result, or -1 if the exit cannot be called.
pub extern "C" fn call_exit(name: *const u8, module: *const u8, func: i32, sub: i32, parm: *mut u8) -> i32 {
    let Some(n) = opt(name) else { return -1 };
    match resolve(KIND_EXIT, n, opt(module)) {
        Ok((_, _, true)) => -1,
        Ok((addr, _, false)) => unsafe {
            let h: ExitHandler = core::mem::transmute(addr);
            h(func, sub, parm)
        },
        Err(_) => -1,
    }
}

/// RexxCallSubcom(env, dll, command, flags, handler rc, result): 0, or
/// 30 when the environment is not registered.
pub extern "C" fn call_subcom(name: *const u8, module: *const u8, cmd: *const RxString, flags: *mut u16, rc: *mut u16, ret: *mut RxString) -> u32 {
    let Some(n) = opt(name) else { return 30 };
    match resolve(KIND_SUBCOM, n, opt(module)) {
        Ok((addr, _, b16)) => unsafe {
            let r = if b16 {
                a16::call_subcom16(addr as u32, cmd, flags, ret)
            } else {
                let h: SubcomHandler = core::mem::transmute(addr);
                h(cmd, flags, ret)
            };
            if !rc.is_null() {
                *rc = r as u16;
            }
            0
        },
        Err(e) => e,
    }
}

pub extern "C" fn register_function_dll(name: *const u8, module: *const u8, entry: *const u8) -> u32 {
    register_dll(KIND_FUNC, name, module, entry, null(), 0)
}

pub extern "C" fn register_function_exe(name: *const u8, entry: usize) -> u32 {
    register_exe(KIND_FUNC, name, entry, null(), false)
}

pub extern "C" fn deregister_function(name: *const u8) -> u32 {
    deregister(KIND_FUNC, name, null())
}

/// RexxQueryFunction: 0 registered, 30 not.
pub extern "C" fn query_function(name: *const u8) -> u32 {
    let Some(n) = opt(name) else { return 30 };
    if find_local(KIND_FUNC, n).is_some() {
        return 0;
    }
    match shared::lock() {
        Some(g) if !find_dll(&g, KIND_FUNC, n, None).is_null() => 0,
        _ => 30,
    }
}

/// RexxCallFunction(name, argc, argv, function rc, result, queue): 0
/// when called, else 30 (not registered), 40/50 (module or entry not
/// found).
pub extern "C" fn call_function(name: *const u8, argc: u32, argv: *const RxString, rc: *mut u16, ret: *mut RxString, queue: *const u8) -> u32 {
    let Some(n) = opt(name) else { return 30 };
    match resolve(KIND_FUNC, n, None) {
        Ok((addr, _, b16)) => unsafe {
            let r = if b16 {
                a16::call_function16(addr as u32, name, argc, argv, queue, ret)
            } else {
                let h: FunctionHandler = core::mem::transmute(addr);
                h(name, argc, argv, queue, ret)
            };
            if !rc.is_null() {
                *rc = r as u16;
            }
            0
        },
        Err(50) => 40,
        Err(127) => 50,
        Err(e) => e,
    }
}

// ---- the macro space ----

const RXMACRO_NO_STORAGE: u32 = 1;
const RXMACRO_NOT_FOUND: u32 = 2;
const RXMACRO_EXTENSION_REQUIRED: u32 = 3;
const RXMACRO_ALREADY_EXISTS: u32 = 4;
const RXMACRO_FILE_ERROR: u32 = 5;
const RXMACRO_SIGNATURE_ERROR: u32 = 6;
const RXMACRO_SOURCE_NOT_FOUND: u32 = 7;
const RXMACRO_INVALID_POSITION: u32 = 8;
const RXMACRO_NOT_INIT: u32 = 9;

/// rs2's macro space file: this signature, then per macro: u32 name
/// length, name, u32 position, u32 source length, source.
const MACRO_SIG: &[u8] = b"RS2 REXX MACROSPACE 1\x1a";

use shared::Macro;

fn find_macro(g: &Guard, name: &[u8]) -> *mut Macro {
    let mut m = g.root().macros;
    while !m.is_null() {
        unsafe {
            if eq_nocase(cstr((*m).name), name) {
                return m;
            }
            m = (*m).next;
        }
    }
    null_mut()
}

fn read_file(name: &[u8]) -> Option<Vec<u8>> {
    let mut z = name.to_vec();
    z.push(0);
    let (mut hf, mut act) = (0u32, 0u32);
    if unsafe { DosOpen(z.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0u32;
        if unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(hf) };
    Some(out)
}

fn write_file(name: &[u8], data: &[u8]) -> bool {
    let mut z = name.to_vec();
    z.push(0);
    let (mut hf, mut act) = (0u32, 0u32);
    let rc = unsafe {
        DosOpen(
            z.as_ptr(),
            &mut hf,
            &mut act,
            0,
            0,
            OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
            OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYREADWRITE,
            null_mut(),
        )
    };
    if rc != 0 {
        return false;
    }
    let mut n = 0u32;
    let ok = unsafe { DosWrite(hf, data.as_ptr(), data.len() as u32, &mut n) } == 0 && n as usize == data.len();
    unsafe { DosClose(hf) };
    ok
}

fn set_macro(g: &Guard, name: &[u8], src: &[u8], pos: u32) -> u32 {
    let data = shared::alloc(src.len().max(1));
    if data.is_null() {
        return RXMACRO_NO_STORAGE;
    }
    unsafe { copy_nonoverlapping(src.as_ptr(), data, src.len()) };
    let m = find_macro(g, name);
    if !m.is_null() {
        unsafe {
            shared::free((*m).data);
            (*m).data = data;
            (*m).len = src.len() as u32;
            (*m).pos = pos;
        }
        return 0;
    }
    let m = shared::alloc(core::mem::size_of::<Macro>()) as *mut Macro;
    let n = shared::dup(&upper(name));
    if m.is_null() || n.is_null() {
        shared::free(data);
        shared::free(n);
        shared::free(m as *mut u8);
        return RXMACRO_NO_STORAGE;
    }
    let root = g.root();
    unsafe {
        m.write(Macro { next: null_mut(), name: n, pos, len: src.len() as u32, data });
        // keep the list in the order added
        let mut link: *mut *mut Macro = &mut root.macros;
        while !(*link).is_null() {
            link = &mut (**link).next;
        }
        *link = m;
    }
    0
}

pub extern "C" fn add_macro(name: *const u8, file: *const u8, pos: u32) -> u32 {
    let (Some(n), Some(f)) = (opt(name), opt(file)) else { return RXMACRO_NOT_FOUND };
    if pos != 1 && pos != 2 {
        return RXMACRO_INVALID_POSITION;
    }
    let Some(src) = read_file(f) else { return RXMACRO_SOURCE_NOT_FOUND };
    let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
    set_macro(&g, n, &src, pos)
}

fn unlink_macro(g: &Guard, m: *mut Macro) {
    let root = g.root();
    let mut link: *mut *mut Macro = &mut root.macros;
    unsafe {
        while !(*link).is_null() {
            if *link == m {
                *link = (*m).next;
                break;
            }
            link = &mut (**link).next;
        }
        shared::free((*m).name);
        shared::free((*m).data);
        shared::free(m as *mut u8);
    }
}

pub extern "C" fn drop_macro(name: *const u8) -> u32 {
    let Some(n) = opt(name) else { return RXMACRO_NOT_FOUND };
    let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
    let m = find_macro(&g, n);
    if m.is_null() {
        return RXMACRO_NOT_FOUND;
    }
    unlink_macro(&g, m);
    0
}

pub extern "C" fn clear_macro_space() -> u32 {
    let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
    if g.root().macros.is_null() {
        return RXMACRO_NOT_FOUND;
    }
    while !g.root().macros.is_null() {
        let m = g.root().macros;
        unlink_macro(&g, m);
    }
    0
}

fn name_list(n: u32, names: *const *const u8) -> Vec<Vec<u8>> {
    (0..n as usize).map(|i| upper(cstr(unsafe { *names.add(i) }))).collect()
}

pub extern "C" fn save_macro_space(n: u32, names: *const *const u8, file: *const u8) -> u32 {
    let Some(f) = opt(file) else { return RXMACRO_FILE_ERROR };
    let want = if n == 0 { None } else { Some(name_list(n, names)) };
    let mut out = MACRO_SIG.to_vec();
    {
        let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
        if let Some(w) = &want {
            if w.iter().any(|x| find_macro(&g, x).is_null()) {
                return RXMACRO_NOT_FOUND;
            }
        }
        let mut m = g.root().macros;
        if m.is_null() {
            return RXMACRO_NOT_FOUND;
        }
        while !m.is_null() {
            unsafe {
                let name = cstr((*m).name);
                if want.as_ref().is_none_or(|w| w.iter().any(|x| x.as_slice() == name)) {
                    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
                    out.extend_from_slice(name);
                    out.extend_from_slice(&(*m).pos.to_le_bytes());
                    out.extend_from_slice(&(*m).len.to_le_bytes());
                    out.extend_from_slice(core::slice::from_raw_parts((*m).data, (*m).len as usize));
                }
                m = (*m).next;
            }
        }
    }
    if write_file(f, &out) { 0 } else { RXMACRO_FILE_ERROR }
}

pub extern "C" fn load_macro_space(n: u32, names: *const *const u8, file: *const u8) -> u32 {
    let Some(f) = opt(file) else { return RXMACRO_FILE_ERROR };
    let Some(d) = read_file(f) else { return RXMACRO_FILE_ERROR };
    if !d.starts_with(MACRO_SIG) {
        return RXMACRO_SIGNATURE_ERROR;
    }
    let want = if n == 0 { None } else { Some(name_list(n, names)) };
    let mut entries = Vec::new();
    let mut i = MACRO_SIG.len();
    let rd = |i: usize| -> Option<u32> { d.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) };
    while i < d.len() {
        let Some(nl) = rd(i) else { return RXMACRO_SIGNATURE_ERROR };
        let name = d.get(i + 4..i + 4 + nl as usize).map(|s| s.to_vec());
        let j = i + 4 + nl as usize;
        let (Some(name), Some(pos), Some(len)) = (name, rd(j), rd(j + 4)) else { return RXMACRO_SIGNATURE_ERROR };
        let Some(src) = d.get(j + 8..j + 8 + len as usize) else { return RXMACRO_SIGNATURE_ERROR };
        entries.push((name, pos, src.to_vec()));
        i = j + 8 + len as usize;
    }
    let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
    if let Some(w) = &want {
        for x in w {
            if !entries.iter().any(|e| &e.0 == x) {
                return RXMACRO_NOT_FOUND;
            }
        }
    }
    for (name, pos, src) in &entries {
        if want.as_ref().is_some_and(|w| !w.contains(name)) {
            continue;
        }
        if !find_macro(&g, name).is_null() {
            return RXMACRO_ALREADY_EXISTS;
        }
        let r = set_macro(&g, name, src, *pos);
        if r != 0 {
            return r;
        }
    }
    let _ = RXMACRO_EXTENSION_REQUIRED;
    0
}

pub extern "C" fn query_macro(name: *const u8, pos: *mut u16) -> u32 {
    let Some(n) = opt(name) else { return RXMACRO_NOT_FOUND };
    let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
    let m = find_macro(&g, n);
    if m.is_null() {
        return RXMACRO_NOT_FOUND;
    }
    if !pos.is_null() {
        unsafe { *pos = (*m).pos as u16 };
    }
    0
}

pub extern "C" fn reorder_macro(name: *const u8, pos: u32) -> u32 {
    let Some(n) = opt(name) else { return RXMACRO_NOT_FOUND };
    if pos != 1 && pos != 2 {
        return RXMACRO_INVALID_POSITION;
    }
    let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
    let m = find_macro(&g, n);
    if m.is_null() {
        return RXMACRO_NOT_FOUND;
    }
    unsafe { (*m).pos = pos };
    0
}

/// RexxExecuteMacroFunction(name, image): the macro's program (its REXX
/// source, in shared memory; not to be freed).
pub extern "C" fn execute_macro_function(name: *const u8, image: *mut RxString) -> u32 {
    let Some(n) = opt(name) else { return RXMACRO_NOT_FOUND };
    let Some(g) = shared::lock() else { return RXMACRO_NOT_INIT };
    let m = find_macro(&g, n);
    if m.is_null() {
        return RXMACRO_NOT_FOUND;
    }
    if !image.is_null() {
        unsafe {
            (*image).ptr = (*m).data;
            (*image).len = (*m).len;
        }
    }
    0
}
