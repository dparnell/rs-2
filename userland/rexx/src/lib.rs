//! REXX.DLL: rs2's REXX interpreter (the language is crates/rexx), with
//! IBM's 32-bit entry points and ordinals: RexxStart, RexxVariablePool,
//! RexxSetHalt, RexxSetTrace, RexxResetTrace.
//!
//! RexxStart runs a procedure from a file (default extension .CMD, the
//! current directory then PATH), from memory (instore[0]: source;
//! instore[1] gets an "image" that is rs2's: the source again behind a
//! marker) or from the macro space. Exits named in the exit list are
//! called through REXXAPI (RexxCallExit) with IBM's parameter blocks.
//! External functions, subcommand handlers and queues are REXXAPI's. The
//! interpreter runs on a stack of its own (src/stack.rs). While a
//! callback runs (an exit, a function package, a subcommand handler), the
//! thread's interpreter state is reachable from RexxVariablePool.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
extern crate os2;

mod api;
mod host;
#[cfg(orexx)]
mod oohost;
#[cfg(orexx)]
mod osa;
mod r16;
mod stack;

use alloc::vec::Vec;
use core::ptr::{copy_nonoverlapping, null_mut};
use core::sync::atomic::{AtomicU8, Ordering};

use os2::*;
use rexx::{CallType, Interp, State};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RxString {
    pub len: u32,
    pub ptr: *mut u8,
}

#[repr(C)]
pub struct RxSysExit {
    pub name: *const u8,
    pub code: i32,
}

#[repr(C)]
pub struct ShvBlock {
    pub next: *mut ShvBlock,
    pub name: RxString,
    pub value: RxString,
    pub namelen: u32,
    pub valuelen: u32,
    pub code: u8,
    pub ret: u8,
}

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// Exported entry points: realign the stack to 16 bytes (callers may
/// give only 4) and pass up to nine arguments on.
macro_rules! exports {
    ($($tramp:ident => $f:path;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "push esi", "push edi", "push ebx",
                "and esp, -16", "sub esp, 48",
                "mov eax, [ebp + 8]", "mov [esp], eax",
                "mov eax, [ebp + 12]", "mov [esp + 4], eax",
                "mov eax, [ebp + 16]", "mov [esp + 8], eax",
                "mov eax, [ebp + 20]", "mov [esp + 12], eax",
                "mov eax, [ebp + 24]", "mov [esp + 16], eax",
                "mov eax, [ebp + 28]", "mov [esp + 20], eax",
                "mov eax, [ebp + 32]", "mov [esp + 24], eax",
                "mov eax, [ebp + 36]", "mov [esp + 28], eax",
                "mov eax, [ebp + 40]", "mov [esp + 32], eax",
                "call {f}",
                "lea esp, [ebp - 12]", "pop ebx", "pop edi", "pop esi", "pop ebp", "ret",
                f = sym $f,
            );
        }
    )*};
}

exports! {
    __a_RexxStart => rexx_start;
    __a_RexxVariablePool => variable_pool;
    __a_RexxSetHalt => set_halt;
    __a_RexxSetTrace => set_trace;
    __a_RexxResetTrace => reset_trace;
}

#[cfg(orexx)]
exports! {
    __a_RexxTranslateProgram => oohost::translate_program;
}

#[cfg(orexx)]
exports! {
    __a_RexxTerminate => osa::RexxTerminate;
    __a_RexxSomInitialize => osa::RexxSomInitialize;
    __a_REXX_ARRAY_AT => osa::REXX_ARRAY_AT;
    __a_REXX_ARRAY_PUT => osa::REXX_ARRAY_PUT;
    __a_REXX_ARRAY_SIZE => osa::REXX_ARRAY_SIZE;
    __a_REXX_ARRAY_NEW => osa::REXX_ARRAY_NEW;
    __a_REXX_ARRAY_NEW1 => osa::REXX_ARRAY_NEW1;
    __a_REXX_ARRAY_NEW2 => osa::REXX_ARRAY_NEW2;
    __a_REXX_BUFFER_NEW => osa::REXX_BUFFER_NEW;
    __a_REXX_INTEGER_VALUE => osa::REXX_INTEGER_VALUE;
    __a_REXX_INTEGER_NEW => osa::REXX_INTEGER_NEW;
    __a_REXX_DOUBLE => osa::REXX_DOUBLE;
    __a_REXX_EXCEPT => osa::REXX_EXCEPT;
    __a_REXX_INTEGER => osa::REXX_INTEGER;
    __a_REXX_ISINTEGER => osa::REXX_ISINTEGER;
    __a_REXX_ISASTRING => osa::REXX_ISASTRING;
    __a_REXX_SEND => osa::REXX_SEND;
    __a_REXX_STRING_DATA => osa::REXX_STRING_DATA;
    __a_REXX_STRING_NEW => osa::REXX_STRING_NEW;
    __a_REXX_NIL => osa::REXX_NIL;
    __a_REXX_TRUE => osa::REXX_TRUE;
    __a_REXX_ENVIRONMENT => osa::REXX_ENVIRONMENT;
    __a_REXX_LOCAL => osa::REXX_LOCAL;
    __a_REXX_UNSIGNED_INTEGER => osa::REXX_UNSIGNED_INTEGER;
    __a_REXX_ISDIRECTORY => osa::REXX_ISDIRECTORY;
    __a_RexxCreateMethod => osa::RexxCreateMethod;
    __a_RexxRunMethod => osa::RexxRunMethod;
    __a_RexxStoreMethod => osa::RexxStoreMethod;
    __a_RexxLoadMethod => osa::RexxLoadMethod;
    __a_RexxCreateDirectory => osa::RexxCreateDirectory;
    __a_RexxRemoveDirectory => osa::RexxRemoveDirectory;
    __a_RexxDispose => osa::RexxDispose;
    __a_RexxSetOSAInfo => osa::RexxSetOSAInfo;
    __a_RexxGetOSAInfo => osa::RexxGetOSAInfo;
    __a_RexxResultString => osa::RexxResultString;
    __a_RexxGetSource => osa::RexxGetSource;
    __a_RexxCopyMethod => osa::RexxCopyMethod;
    __a_RexxValidObject => osa::RexxValidObject;
}

// ---- per-thread state ----

const THREADS: usize = 1024;
static mut CURRENT: [*mut State; THREADS] = [null_mut(); THREADS];
static HALT: [AtomicU8; THREADS] = [const { AtomicU8::new(0) }; THREADS];
/// 1: trace requested, 2: trace reset requested.
static TRACE: [AtomicU8; THREADS] = [const { AtomicU8::new(0) }; THREADS];
static mut DEPTH: [u32; THREADS] = [0; THREADS];

fn slot() -> usize {
    stack::tid() as usize % THREADS
}

/// Run `f` with the interpreter state reachable from RexxVariablePool.
pub fn with_state<R>(st: &mut State, f: impl FnOnce() -> R) -> R {
    let t = slot();
    let prev = unsafe { CURRENT[t] };
    unsafe { CURRENT[t] = st as *mut State };
    let r = f();
    unsafe { CURRENT[t] = prev };
    r
}

pub fn take_halt(tid: u32) -> bool {
    HALT[tid as usize % THREADS].swap(0, Ordering::Relaxed) != 0
}

pub fn take_trace(tid: u32) -> Option<bool> {
    match TRACE[tid as usize % THREADS].swap(0, Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

fn own_pid() -> u32 {
    let mut tib: *mut Tib = null_mut();
    let mut pib: *mut Pib = null_mut();
    unsafe { DosGetInfoBlocks(&mut tib, &mut pib) };
    if pib.is_null() { 0 } else { unsafe { (*pib).pid } }
}

fn flag_threads(arr: &[AtomicU8; THREADS], pid: u32, tid: u32, v: u8) -> u32 {
    if pid != own_pid() {
        return 1; // RXARI_NOT_FOUND
    }
    if tid == 0 {
        for (i, a) in arr.iter().enumerate() {
            if unsafe { DEPTH[i] } > 0 {
                a.store(v, Ordering::Relaxed);
            }
        }
    } else {
        arr[tid as usize % THREADS].store(v, Ordering::Relaxed);
    }
    0
}

pub extern "C" fn set_halt(pid: u32, tid: u32) -> u32 {
    flag_threads(&HALT, pid, tid, 1)
}

pub extern "C" fn set_trace(pid: u32, tid: u32) -> u32 {
    flag_threads(&TRACE, pid, tid, 1)
}

pub extern "C" fn reset_trace(pid: u32, tid: u32) -> u32 {
    flag_threads(&TRACE, pid, tid, 2)
}

// ---- Ctrl+Break and Ctrl+C halt the program ----

const XCPT_SIGNAL: u32 = 0xC001_0003;
const XCPT_CONTINUE_EXECUTION: u32 = 0xFFFF_FFFF;
const XCPT_CONTINUE_SEARCH: u32 = 0;

unsafe extern "C" fn signal_handler(rep: *mut ExceptionReportRecord, _reg: *mut ExceptionRegistrationRecord, _ctx: *mut ContextRecord, _d: *mut u8) -> u32 {
    unsafe {
        if (*rep).num == XCPT_SIGNAL && matches!((*rep).info[0], 1 | 4) {
            for (i, h) in HALT.iter().enumerate() {
                if DEPTH[i] > 0 {
                    h.store(1, Ordering::Relaxed);
                }
            }
            api::DosAcknowledgeSignalException((*rep).info[0]);
            return XCPT_CONTINUE_EXECUTION;
        }
    }
    XCPT_CONTINUE_SEARCH
}

// ---- RexxStart ----

const IMAGE_MARK: &[u8] = b"RS2 REXX IMAGE\x1a";

fn rx_bytes(r: &RxString) -> Vec<u8> {
    if r.ptr.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(r.ptr, r.len as usize).to_vec() } }
}

fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return b"";
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

fn alloc_copy(v: &[u8]) -> *mut u8 {
    let mut p: *mut u8 = null_mut();
    if unsafe { DosAllocMem(&mut p, (v.len() as u32).max(1), PAG_COMMIT | PAG_READ | PAG_WRITE) } != 0 {
        return null_mut();
    }
    unsafe { copy_nonoverlapping(v.as_ptr(), p, v.len()) };
    p
}

#[allow(clippy::too_many_arguments)]
pub extern "C" fn rexx_start(
    argc: i32,
    argv: *const RxString,
    name: *const u8,
    instore: *mut RxString,
    env: *const u8,
    calltype: i32,
    exits: *const RxSysExit,
    rc: *mut i16,
    result: *mut RxString,
) -> i32 {
    let name = cstr(name).to_vec();
    // the program text
    let (src, path) = if !instore.is_null() {
        let (s0, s1) = unsafe { (*instore, *instore.add(1)) };
        if !s0.ptr.is_null() {
            let src = rx_bytes(&s0);
            let mut img = IMAGE_MARK.to_vec();
            img.extend_from_slice(&src);
            let p = alloc_copy(&img);
            if !p.is_null() {
                unsafe { *instore.add(1) = RxString { len: img.len() as u32, ptr: p } };
            }
            (src, name.clone())
        } else if !s1.ptr.is_null() {
            let img = rx_bytes(&s1);
            if !img.starts_with(IMAGE_MARK) {
                return -3;
            }
            (img[IMAGE_MARK.len()..].to_vec(), name.clone())
        } else {
            // the macro space
            let nz = { let mut v = name.clone(); v.push(0); v };
            let mut img = RxString { len: 0, ptr: null_mut() };
            if unsafe { api::RexxExecuteMacroFunction(nz.as_ptr(), &mut img) } != 0 {
                return -3;
            }
            (rx_bytes(&img), name.clone())
        }
    } else {
        let Some(path) = host::full_name(&name).or_else(|| {
            // the default extension .CMD
            let base = name.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(&name);
            if base.contains(&b'.') {
                None
            } else {
                let mut n = name.clone();
                n.extend_from_slice(b".CMD");
                host::full_name(&n)
            }
        }) else {
            return -3;
        };
        let Some(src) = host::read_file(&path) else { return -3 };
        // a program REXXC tokenized
        #[cfg(orexx)]
        let src = oohost::untokenize(&src).unwrap_or(src);
        (src, path)
    };
    // the initial environment: as given, else the file's extension
    let env = if env.is_null() || cstr(env).is_empty() {
        let base = path.rsplit(|&c| c == b'\\' || c == b'/').next().unwrap_or(&path);
        match base.iter().rposition(|&c| c == b'.') {
            Some(p) if p + 1 < base.len() => base[p + 1..].to_ascii_uppercase(),
            _ => b"CMD".to_vec(),
        }
    } else {
        cstr(env).to_vec()
    };
    let mut args = Vec::new();
    for i in 0..argc.max(0) as usize {
        let a = unsafe { *argv.add(i) };
        args.push(if a.ptr.is_null() { None } else { Some(rx_bytes(&a)) });
    }
    let ct = match calltype {
        1 => CallType::Subroutine,
        2 => CallType::Function,
        _ => CallType::Command,
    };
    let mut ex: [Option<Vec<u8>>; 11] = Default::default();
    if !exits.is_null() {
        let mut i = 0;
        loop {
            let e = unsafe { &*exits.add(i) };
            if e.code == 0 {
                break;
            }
            if (e.code as usize) < ex.len() && ex[e.code as usize].is_none() && !e.name.is_null() {
                ex[e.code as usize] = Some(cstr(e.name).to_vec());
            }
            i += 1;
        }
    }
    let t = slot();
    let tid = stack::tid();
    unsafe { DEPTH[t] += 1 };
    // Ctrl+Break and Ctrl+C halt the procedure
    let mut reg = ExceptionRegistrationRecord { prev: null_mut(), handler: signal_handler };
    let outer = unsafe { DEPTH[t] } == 1;
    if outer {
        unsafe { DosSetExceptionHandler(&mut reg) };
    }
    let out = stack::deep(move || {
        let mut h = host::OsHost { exits: ex, locals: Vec::new(), tid };
        #[cfg(orexx)]
        let mut it = Interp::new_oo(&mut h);
        #[cfg(not(orexx))]
        let mut it = Interp::new(&mut h);
        #[cfg(orexx)]
        oohost::lock();
        let r = it.run_program(&src, &path, ct, args, &env);
        // the program's activities end before RexxStart returns
        #[cfg(orexx)]
        {
            if outer {
                oohost::wait_activities();
                it.end_uninits();
            }
            oohost::unlock();
        }
        r
    });
    if outer {
        unsafe { DosUnsetExceptionHandler(&mut reg) };
    }
    unsafe { DEPTH[t] -= 1 };
    match out {
        Ok(v) => {
            if !rc.is_null() {
                let n = v.as_ref().and_then(|v| core::str::from_utf8(v).ok()).and_then(|s| s.trim().parse::<i32>().ok());
                unsafe { *rc = n.filter(|n| (-32768..=32767).contains(n)).unwrap_or(0) as i16 };
            }
            if !result.is_null() {
                let r = unsafe { &mut *result };
                match v {
                    Some(v) => {
                        if !r.ptr.is_null() && r.len as usize >= v.len() {
                            unsafe { copy_nonoverlapping(v.as_ptr(), r.ptr, v.len()) };
                        } else {
                            r.ptr = alloc_copy(&v);
                        }
                        r.len = v.len() as u32;
                    }
                    None => {
                        r.len = 0;
                        r.ptr = null_mut();
                    }
                }
            }
            0
        }
        Err(e) => {
            if !rc.is_null() {
                unsafe { *rc = e as i16 };
            }
            if !result.is_null() {
                unsafe {
                    (*result).len = 0;
                    (*result).ptr = null_mut();
                }
            }
            e
        }
    }
}

// ---- RexxVariablePool ----

const RXSHV_SET: u8 = 0;
const RXSHV_FETCH: u8 = 1;
const RXSHV_DROPV: u8 = 2;
const RXSHV_SYSET: u8 = 3;
const RXSHV_SYFET: u8 = 4;
const RXSHV_SYDRO: u8 = 5;
const RXSHV_NEXTV: u8 = 6;
const RXSHV_PRIV: u8 = 7;
const RXSHV_NEWV: u8 = 1;
const RXSHV_LVAR: u8 = 2;
const RXSHV_TRUNC: u8 = 4;
const RXSHV_BADN: u8 = 8;
const RXSHV_MEMFL: u8 = 0x10;
const RXSHV_BADF: u8 = 0x80;
const RXSHV_NOAVL: u32 = 144;

/// Copy a value into an RXSTRING the caller describes (`cap` bytes, or
/// none: allocate). Returns the RXSHV flags.
fn put_value(dst: &mut RxString, cap: &mut u32, v: &[u8]) -> u8 {
    if dst.ptr.is_null() {
        let p = alloc_copy(v);
        if p.is_null() {
            return RXSHV_MEMFL;
        }
        dst.ptr = p;
        dst.len = v.len() as u32;
        *cap = v.len() as u32;
        0
    } else {
        let n = (v.len() as u32).min(*cap);
        unsafe { copy_nonoverlapping(v.as_ptr(), dst.ptr, n as usize) };
        dst.len = n;
        if n < v.len() as u32 { RXSHV_TRUNC } else { 0 }
    }
}

fn shv_one(st: &mut State, b: &mut ShvBlock) -> u8 {
    let name = rx_bytes(&b.name);
    match b.code {
        RXSHV_SET | RXSHV_SYSET => {
            let sym = b.code == RXSHV_SYSET;
            let existed = matches!(st.var_get(&name, sym), Ok(Some(_)));
            match st.var_set(&name, rx_bytes(&b.value), sym) {
                Ok(()) => {
                    if existed {
                        0
                    } else {
                        RXSHV_NEWV
                    }
                }
                Err(()) => RXSHV_BADN,
            }
        }
        RXSHV_FETCH | RXSHV_SYFET => {
            let sym = b.code == RXSHV_SYFET;
            match st.var_get(&name, sym) {
                Ok(Some(v)) => put_value(&mut b.value, &mut b.valuelen, &v),
                Ok(None) => {
                    let n = if sym { name.to_ascii_uppercase() } else { name };
                    put_value(&mut b.value, &mut b.valuelen, &n) | RXSHV_NEWV
                }
                Err(()) => RXSHV_BADN,
            }
        }
        RXSHV_DROPV | RXSHV_SYDRO => {
            let sym = b.code == RXSHV_SYDRO;
            let existed = matches!(st.var_get(&name, sym), Ok(Some(_)));
            match st.var_drop(&name, sym) {
                Ok(()) => {
                    if existed {
                        0
                    } else {
                        RXSHV_NEWV
                    }
                }
                Err(()) => RXSHV_BADN,
            }
        }
        RXSHV_NEXTV => match st.var_next() {
            Some((n, v)) => put_value(&mut b.name, &mut b.namelen, &n) | put_value(&mut b.value, &mut b.valuelen, &v),
            None => RXSHV_LVAR,
        },
        RXSHV_PRIV => {
            let u = name.to_ascii_uppercase();
            let v: Option<Vec<u8>> = match u.as_slice() {
                b"PARM" => Some(alloc::format!("{}", st.args.len()).into_bytes()),
                b"SOURCE" => Some(st.source.clone()),
                b"VERSION" => Some(rexx::VERSION.to_vec()),
                b"QUENAME" => Some(host::queue_name()),
                _ if u.starts_with(b"PARM.") => {
                    let n: usize = core::str::from_utf8(&u[5..]).ok().and_then(|s| s.parse().ok()).unwrap_or(0);
                    if n == 0 { None } else { Some(st.args.get(n - 1).cloned().flatten().map(|v| v.fast_string()).unwrap_or_default()) }
                }
                _ => None,
            };
            match v {
                Some(v) => put_value(&mut b.value, &mut b.valuelen, &v),
                None => RXSHV_BADN,
            }
        }
        _ => RXSHV_BADF,
    }
}

pub extern "C" fn variable_pool(mut p: *mut ShvBlock) -> u32 {
    let st = unsafe { CURRENT[slot()] };
    if st.is_null() {
        return RXSHV_NOAVL;
    }
    let st = unsafe { &mut *st };
    let mut all = 0u32;
    while !p.is_null() {
        let b = unsafe { &mut *p };
        b.ret = shv_one(st, b);
        all |= b.ret as u32;
        p = b.next;
    }
    all
}
