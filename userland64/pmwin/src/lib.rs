//! PMWIN for 64-bit programs (docs/API64.md, "PM for 64-bit programs").
//!
//! PM's shared state belongs to rs2's 32-bit PMWIN.DLL, so this DLL is a
//! layer over it in the same process: every export calls the 32-bit
//! function of the same name in compatibility mode ([`call32`]), with the
//! arguments as 32-bit values. Memory above 4 GB (DosAllocMem with
//! OBJ_ANY) is out of the 32-bit PMWIN's reach: a pointer argument above
//! 4 GB, or an MPARAM that is not a 32-bit value, makes the call fail
//! (0, PMERR_INVALID_PARAMETERS) instead of being truncated ([`fits`]). Window procedures go the other way through 32-bit stubs that
//! switch to long mode ([`to32`], `rs2_entry64`), and 32-bit procedures
//! PMWIN hands out (WinSubclassWindow's old procedure) get 64-bit stubs
//! ([`from32`]). Structures with pointers in them are converted: QMSG,
//! CLASSINFO, CREATESTRUCT (WM_CREATE's mp2), DEVOPENSTRUC.
//!
//! Most exports are generated (thunks.rs, tools/scripts/gen_pm64.py);
//! the ones listed on the `special:` lines below are written here.

#![no_std]
#![allow(non_snake_case, clippy::missing_safety_doc)]

use core::arch::global_asm;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use os2::*;

// special: WinRegisterClass WinSubclassWindow WinDlgBox WinLoadDlg WinCreateDlg
// special: WinGetMsg WinPeekMsg WinDispatchMsg WinTranslateAccel WinCallMsgFilter
// special: WinQueryClassInfo WinQueryWindowPtr WinSetWindowPtr WinSetHook WinReleaseHook
// special: DevOpenDC WinDefWindowProc WinDefDlgProc

macro_rules! thunks {
    ($($name:ident [$ptr:literal, $mp:literal] ($($a:ident)*);)*) => {$(
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name($($a: u64),*) -> u64 {
            static ADDR: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
            if !crate::fits($ptr, $mp, &[$($a),*]) {
                return crate::refuse();
            }
            let f = crate::proc32(&ADDR, concat!(stringify!($name), "\0"));
            crate::call32(f, &[$($a as u32),*]) as u64
        }
    )*};
}

// ---- Arguments above 4 GB ----

/// True if the 32-bit PMWIN can take these arguments: no pointer
/// argument (bit n of `ptr`) above 4 GB, every MPARAM (bit n of `mp`) a
/// 32-bit value (zero- or sign-extended: MPFROMLONG(-1) is fine, a
/// pointer above 4 GB is not). Other arguments are ULONGs, LONGs and
/// handles, whose upper halves mean nothing.
pub fn fits(ptr: u32, mp: u32, args: &[u64]) -> bool {
    args.iter().enumerate().all(|(i, &v)| {
        (ptr & (1 << i) == 0 || low(v)) && (mp & (1 << i) == 0 || mp32(v).is_some())
    })
}

/// A pointer 32-bit code can take (below 4 GB).
fn low(p: u64) -> bool {
    p <= u32::MAX as u64
}

/// An MPARAM as a 32-bit value, if it is one.
fn mp32(v: u64) -> Option<u32> {
    (v <= u32::MAX as u64 || v as i32 as i64 as u64 == v).then_some(v as u32)
}

const PMERR_INVALID_PARAMETERS: u32 = 0x1208;

/// A call refused for memory above 4 GB: PMERR_INVALID_PARAMETERS, and 0
/// (FALSE, NULLHANDLE, or MRESULT 0).
pub fn refuse() -> u64 {
    static ADDR: AtomicU32 = AtomicU32::new(0);
    call32(proc32(&ADDR, "WinSetErrorInfo\0"), &[PMERR_INVALID_PARAMETERS, 0]);
    0
}

mod thunks;

/// A DLL has no entry point; the linker script names one.
#[unsafe(no_mangle)]
pub extern "C" fn _start() {}

// ---- Calling 32-bit code ----

unsafe extern "C" {
    #[link_name = "DOSCALLS.2040"]
    fn Dos64LoadModule32(fail: *mut u8, cbfail: u32, name: *const u8, phmod: *mut u32) -> APIRET;
    fn rs2_call32(target: u32, args: *const u32, n: usize) -> u32;
    static rs2_entry64: u8;
    static rs2_proc32: u8;
}

// rs2_call32(target, args, n): call the 32-bit function `target` with n
// dwords of arguments (_System: the caller removes them) in compatibility
// mode, on this stack (below 4 GB), and return its EAX.
//
// The arguments go to a 16-byte aligned A, the return address into the
// 32-bit stub `ret` below A; RETFQ enters `target` with CS 0x5B. The
// function returns to `ret`, which jumps far through [EBX] (the callee
// keeps EBX and EBP) back to long mode at `back`, where EBP gives the
// saved frame again. The upper halves of the registers are not to be
// trusted after compatibility mode: everything is reloaded from the
// stack.
//
// rs2_entry64: where the 32-bit stubs of 64-bit window procedures land
// (in long mode, EAX = the stub's slot, [ESP] = the 32-bit return
// address, then hwnd, msg, mp1, mp2). Calls
// rs2_wndproc(slot, hwnd, msg, mp1, mp2) and returns to the caller in
// compatibility mode through a RET at rs2_ret32.
//
// rs2_proc32: the 64-bit stubs of 32-bit window procedures jump here with
// EAX = the procedure: rs2_proc4(target, hwnd, msg, mp1, mp2).
global_asm!(
    ".globl rs2_call32",
    "rs2_call32:",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov rbp, rsp",
    "    lea rax, [rip + 3f]",
    "    mov [rbp - 8], eax",
    "    mov word ptr [rbp - 4], 0x6b",
    "    lea rbx, [rbp - 8]",
    "    lea rax, [rdx * 4]",
    "    mov rcx, rbp",
    "    sub rcx, rax",
    "    sub rcx, 32",
    "    and rcx, -16",
    "    xor eax, eax",
    "1:  cmp rax, rdx",
    "    jae 2f",
    "    mov r8d, [rsi + rax * 4]",
    "    mov [rcx + rax * 4], r8d",
    "    inc rax",
    "    jmp 1b",
    "2:  lea rax, [rip + 4f]",
    "    mov [rcx - 4], eax",
    "    mov qword ptr [rcx - 12], 0x5b",
    "    mov eax, edi",
    "    mov [rcx - 20], rax",
    "    lea rsp, [rcx - 20]",
    "    cld",
    "    retfq",
    // 32-bit code: jmp far dword ptr [ebx]
    "4:  .byte 0xff, 0x2b",
    "3:  mov ebp, ebp",
    "    mov rsp, rbp",
    "    mov eax, eax",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    ret",
    "",
    ".globl rs2_entry64",
    "rs2_entry64:",
    "    mov esp, esp",
    "    push rbx",
    "    push rbp",
    "    push rsi",
    "    push rdi",
    "    mov rbp, rsp",
    "    and rsp, -16",
    "    mov edi, eax",
    "    mov esi, [rbp + 36]",
    "    mov edx, [rbp + 40]",
    "    mov ecx, [rbp + 44]",
    "    mov r8d, [rbp + 48]",
    "    cld",
    "    call {wndproc}",
    "    mov rsp, rbp",
    "    pop rdi",
    "    pop rsi",
    "    pop rbp",
    "    pop rbx",
    "    sub rsp, 16",
    "    lea rcx, [rip + rs2_ret32]",
    "    mov [rsp], rcx",
    "    mov qword ptr [rsp + 8], 0x5b",
    "    retfq",
    ".globl rs2_ret32",
    "rs2_ret32:",
    "    .byte 0xc3",
    "",
    ".globl rs2_proc32",
    "rs2_proc32:",
    "    mov r8, rcx",
    "    mov rcx, rdx",
    "    mov rdx, rsi",
    "    mov rsi, rdi",
    "    mov edi, eax",
    "    jmp {proc4}",
    wndproc = sym rs2_wndproc,
    proc4 = sym rs2_proc4,
);

/// Call a 32-bit function.
pub fn call32(target: u32, args: &[u32]) -> u32 {
    if target == 0 {
        return 0;
    }
    unsafe { rs2_call32(target, args.as_ptr(), args.len()) }
}

static HMOD32: AtomicU32 = AtomicU32::new(0);

/// The 32-bit PMWIN's procedure `name` (NUL-terminated), cached in `cache`.
fn proc32(cache: &AtomicU32, name: &str) -> u32 {
    let f = cache.load(Ordering::Relaxed);
    if f != 0 {
        return f;
    }
    let mut h = HMOD32.load(Ordering::Relaxed);
    if h == 0 {
        let mut fail = [0u8; 16];
        unsafe { Dos64LoadModule32(fail.as_mut_ptr(), 16, c"PMWIN".as_ptr().cast(), &mut h) };
        HMOD32.store(h, Ordering::Relaxed);
    }
    let mut pfn = 0usize;
    if unsafe { DosQueryProcAddr(h, 0, name.as_ptr(), (&raw mut pfn).cast()) } != 0 {
        os2::println!("PMWIN (64-bit): {} not found in the 32-bit PMWIN", name.trim_end_matches('\0'));
        return 0;
    }
    cache.store(pfn as u32, Ordering::Relaxed);
    pfn as u32
}

macro_rules! target {
    ($name:literal) => {{
        static ADDR: AtomicU32 = AtomicU32::new(0);
        proc32(&ADDR, concat!($name, "\0"))
    }};
}

// ---- Procedures across the two modes ----

const SLOTS: usize = 512;
const STUB: usize = 16;

/// The 64-bit procedure behind each 32-bit stub (0: free).
static PROC64: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
/// The 32-bit procedure behind each 64-bit stub (0: free).
static PROC32: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
/// The stub page: 32-bit stubs, then 64-bit stubs, then the far pointer
/// to rs2_entry64 and the address of rs2_proc32.
static PAGE: AtomicU64 = AtomicU64::new(0);
static LOCK: AtomicU32 = AtomicU32::new(0);

const STUBS64: usize = SLOTS * STUB;
const FAR64: usize = 2 * SLOTS * STUB;
const PROC32_AT: usize = FAR64 + 8;

fn lock() {
    while LOCK.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
        unsafe { DosSleep(0) };
    }
}

fn unlock() {
    LOCK.store(0, Ordering::Release);
}

/// The stub page, made on first use.
fn page() -> *mut u8 {
    let p = PAGE.load(Ordering::Acquire);
    if p != 0 {
        return p as *mut u8;
    }
    let mut m: *mut u8 = core::ptr::null_mut();
    unsafe { DosAllocMem((&raw mut m).cast(), (PROC32_AT + 8) as u32, PAG_READ | PAG_WRITE | PAG_EXECUTE | PAG_COMMIT) };
    unsafe {
        let far = m.add(FAR64);
        (far as *mut u32).write_unaligned(&rs2_entry64 as *const u8 as u32);
        (far.add(4) as *mut u16).write_unaligned(0x6b);
        (m.add(PROC32_AT) as *mut u64).write_unaligned(&rs2_proc32 as *const u8 as u64);
    }
    PAGE.store(m as u64, Ordering::Release);
    m
}

/// A 64-bit procedure as PMWIN can call it: a 32-bit stub (the same one
/// for the same procedure); a 64-bit stub of a 32-bit procedure is that
/// procedure again.
pub fn to32(pfn: u64) -> u32 {
    if pfn == 0 {
        return 0;
    }
    lock();
    let base = page() as u64;
    if (base + STUBS64 as u64..base + FAR64 as u64).contains(&pfn) {
        let slot = (pfn - base - STUBS64 as u64) as usize / STUB;
        unlock();
        return PROC32[slot].load(Ordering::Relaxed);
    }
    let slot = match PROC64.iter().position(|p| p.load(Ordering::Relaxed) == pfn) {
        Some(s) => s,
        None => {
            let Some(s) = PROC64.iter().position(|p| p.load(Ordering::Relaxed) == 0) else {
                unlock();
                return 0;
            };
            // mov eax, slot ; jmp far dword ptr [FAR64]
            let at = (base as usize + s * STUB) as *mut u8;
            let far = (base as usize + FAR64) as u32;
            let mut code = [0xccu8; STUB];
            code[0] = 0xb8;
            code[1..5].copy_from_slice(&(s as u32).to_le_bytes());
            code[5] = 0xff;
            code[6] = 0x2d;
            code[7..11].copy_from_slice(&far.to_le_bytes());
            unsafe { core::ptr::copy_nonoverlapping(code.as_ptr(), at, STUB) };
            PROC64[s].store(pfn, Ordering::Relaxed);
            s
        }
    };
    unlock();
    (base as usize + slot * STUB) as u32
}

/// A 32-bit procedure from PMWIN as 64-bit code can call it: our own
/// 32-bit stubs give their 64-bit procedure back.
pub fn from32(pfn: u32) -> u64 {
    if pfn == 0 {
        return 0;
    }
    lock();
    let base = page() as u64;
    if (base..base + STUBS64 as u64).contains(&(pfn as u64)) {
        let slot = (pfn as u64 - base) as usize / STUB;
        unlock();
        return PROC64[slot].load(Ordering::Relaxed);
    }
    let slot = match PROC32.iter().position(|p| p.load(Ordering::Relaxed) == pfn) {
        Some(s) => s,
        None => {
            let Some(s) = PROC32.iter().position(|p| p.load(Ordering::Relaxed) == 0) else {
                unlock();
                return 0;
            };
            // mov eax, pfn ; jmp qword ptr [rip + to PROC32_AT]
            let at = base as usize + STUBS64 + s * STUB;
            let rel = (base as usize + PROC32_AT) as i64 - (at + 11) as i64;
            let mut code = [0xccu8; STUB];
            code[0] = 0xb8;
            code[1..5].copy_from_slice(&pfn.to_le_bytes());
            code[5] = 0xff;
            code[6] = 0x25;
            code[7..11].copy_from_slice(&(rel as i32).to_le_bytes());
            unsafe { core::ptr::copy_nonoverlapping(code.as_ptr(), at as *mut u8, STUB) };
            PROC32[s].store(pfn, Ordering::Relaxed);
            s
        }
    };
    unlock();
    base + (STUBS64 + slot * STUB) as u64
}

pub type WndProc = unsafe extern "C" fn(u32, u32, u64, u64) -> u64;

const WM_CREATE: u32 = 0x0001;

/// CREATESTRUCT as 64-bit code sees it, with the 32-bit original's
/// address after it (so that passing mp2 on to 32-bit code gives that
/// back: [`mp2_to32`]).
#[repr(C)]
struct CreateStruct64 {
    presparams: u64,
    ctldata: u64,
    id: u32,
    insert_behind: u32,
    owner: u32,
    cy: i32,
    cx: i32,
    y: i32,
    x: i32,
    style: u32,
    text: u64,
    class: u64,
    parent: u32,
    _pad: u32,
    magic: u64,
    orig: u64,
}

const CS_MAGIC: u64 = 0x3233_5453_4552_4352; // "RCREST32"

unsafe fn rd(p: u32, i: usize) -> u32 {
    unsafe { (p as usize as *const u32).add(i).read_unaligned() }
}

/// mp2 of a message 64-bit code passes on to 32-bit code (None: not a
/// 32-bit value, see [`fits`]).
fn mp2_to32(msg: u32, mp2: u64) -> Option<u32> {
    if msg == WM_CREATE && mp2 != 0 && low(mp2) {
        let cs = unsafe { &*(mp2 as *const CreateStruct64) };
        if cs.magic == CS_MAGIC {
            return Some(cs.orig as u32);
        }
    }
    mp32(mp2)
}

/// A message's parameters for 32-bit code.
fn mps_to32(msg: u32, mp1: u64, mp2: u64) -> Option<(u32, u32)> {
    Some((mp32(mp1)?, mp2_to32(msg, mp2)?))
}

/// A message from PMWIN for a 64-bit window procedure.
extern "C" fn rs2_wndproc(slot: u32, hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u64 {
    let f = PROC64[slot as usize % SLOTS].load(Ordering::Relaxed);
    if f == 0 {
        return 0;
    }
    let f: WndProc = unsafe { core::mem::transmute(f) };
    if msg == WM_CREATE && mp2 != 0 {
        let cs = unsafe {
            CreateStruct64 {
                presparams: rd(mp2, 0) as u64,
                ctldata: rd(mp2, 1) as u64,
                id: rd(mp2, 2),
                insert_behind: rd(mp2, 3),
                owner: rd(mp2, 4),
                cy: rd(mp2, 5) as i32,
                cx: rd(mp2, 6) as i32,
                y: rd(mp2, 7) as i32,
                x: rd(mp2, 8) as i32,
                style: rd(mp2, 9),
                text: rd(mp2, 10) as u64,
                class: rd(mp2, 11) as u64,
                parent: rd(mp2, 12),
                _pad: 0,
                magic: CS_MAGIC,
                orig: mp2 as u64,
            }
        };
        return unsafe { f(hwnd, msg, mp1 as u64, &cs as *const _ as u64) };
    }
    unsafe { f(hwnd, msg, mp1 as u64, mp2 as u64) }
}

/// 64-bit code calling a 32-bit window procedure.
extern "C" fn rs2_proc4(target: u32, hwnd: u64, msg: u64, mp1: u64, mp2: u64) -> u64 {
    let Some((mp1, mp2)) = mps_to32(msg as u32, mp1, mp2) else { return refuse() };
    call32(target, &[hwnd as u32, msg as u32, mp1, mp2]) as u64
}

// ---- Calls with structures or procedures ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinRegisterClass(hab: u64, class: u64, pfn: u64, style: u64, cb: u64) -> u64 {
    if !low(class) {
        return refuse();
    }
    call32(target!("WinRegisterClass"), &[hab as u32, class as u32, to32(pfn), style as u32, cb as u32]) as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinSubclassWindow(hwnd: u64, pfn: u64) -> u64 {
    from32(call32(target!("WinSubclassWindow"), &[hwnd as u32, to32(pfn)]))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinDlgBox(parent: u64, owner: u64, pfn: u64, hmod: u64, id: u64, params: u64) -> u64 {
    if !low(params) {
        return refuse();
    }
    call32(target!("WinDlgBox"), &[parent as u32, owner as u32, to32(pfn), hmod as u32, id as u32, params as u32]) as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinLoadDlg(parent: u64, owner: u64, pfn: u64, hmod: u64, id: u64, params: u64) -> u64 {
    if !low(params) {
        return refuse();
    }
    call32(target!("WinLoadDlg"), &[parent as u32, owner as u32, to32(pfn), hmod as u32, id as u32, params as u32]) as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinCreateDlg(parent: u64, owner: u64, pfn: u64, tmpl: u64, params: u64) -> u64 {
    if !low(tmpl) || !low(params) {
        return refuse();
    }
    call32(target!("WinCreateDlg"), &[parent as u32, owner as u32, to32(pfn), tmpl as u32, params as u32]) as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinDefWindowProc(hwnd: u64, msg: u64, mp1: u64, mp2: u64) -> u64 {
    let Some((mp1, mp2)) = mps_to32(msg as u32, mp1, mp2) else { return refuse() };
    call32(target!("WinDefWindowProc"), &[hwnd as u32, msg as u32, mp1, mp2]) as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinDefDlgProc(hwnd: u64, msg: u64, mp1: u64, mp2: u64) -> u64 {
    let Some((mp1, mp2)) = mps_to32(msg as u32, mp1, mp2) else { return refuse() };
    call32(target!("WinDefDlgProc"), &[hwnd as u32, msg as u32, mp1, mp2]) as u64
}

/// QMSG: 32 bytes in 32-bit code, 40 in 64-bit (MPARAMs are pointers).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Qmsg64 {
    pub hwnd: u32,
    pub msg: u32,
    pub mp1: u64,
    pub mp2: u64,
    pub time: u32,
    pub x: i32,
    pub y: i32,
    pub reserved: u32,
}

/// None if the message's parameters are not 32-bit values.
fn q64_to32(q: &Qmsg64) -> Option<[u32; 8]> {
    let (mp1, mp2) = mps_to32(q.msg, q.mp1, q.mp2)?;
    Some([q.hwnd, q.msg, mp1, mp2, q.time, q.x as u32, q.y as u32, q.reserved])
}

fn q32_to64(q: &[u32; 8]) -> Qmsg64 {
    Qmsg64 {
        hwnd: q[0],
        msg: q[1],
        mp1: q[2] as u64,
        mp2: q[3] as u64,
        time: q[4],
        x: q[5] as i32,
        y: q[6] as i32,
        reserved: q[7],
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinGetMsg(hab: u64, pq: u64, hwnd: u64, first: u64, last: u64) -> u64 {
    let mut q = [0u32; 8];
    let r = call32(target!("WinGetMsg"), &[hab as u32, q.as_mut_ptr() as u32, hwnd as u32, first as u32, last as u32]);
    if pq != 0 {
        unsafe { *(pq as *mut Qmsg64) = q32_to64(&q) };
    }
    r as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinPeekMsg(hab: u64, pq: u64, hwnd: u64, first: u64, last: u64, fl: u64) -> u64 {
    let mut q = [0u32; 8];
    let r = call32(target!("WinPeekMsg"), &[hab as u32, q.as_mut_ptr() as u32, hwnd as u32, first as u32, last as u32, fl as u32]);
    if r != 0 && pq != 0 {
        unsafe { *(pq as *mut Qmsg64) = q32_to64(&q) };
    }
    r as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinDispatchMsg(hab: u64, pq: u64) -> u64 {
    let Some(q) = q64_to32(unsafe { &*(pq as *const Qmsg64) }) else { return refuse() };
    call32(target!("WinDispatchMsg"), &[hab as u32, q.as_ptr() as u32]) as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinTranslateAccel(hab: u64, hwnd: u64, haccel: u64, pq: u64) -> u64 {
    let Some(mut q) = q64_to32(unsafe { &*(pq as *const Qmsg64) }) else { return refuse() };
    let r = call32(target!("WinTranslateAccel"), &[hab as u32, hwnd as u32, haccel as u32, q.as_mut_ptr() as u32]);
    unsafe { *(pq as *mut Qmsg64) = q32_to64(&q) };
    r as u64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinCallMsgFilter(hab: u64, pq: u64, msgf: u64) -> u64 {
    let Some(q) = q64_to32(unsafe { &*(pq as *const Qmsg64) }) else { return refuse() };
    call32(target!("WinCallMsgFilter"), &[hab as u32, q.as_ptr() as u32, msgf as u32]) as u64
}

/// CLASSINFO: { ULONG flClassStyle; PFNWP pfnWindowProc; ULONG cbWindowData }.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinQueryClassInfo(hab: u64, class: u64, pci: u64) -> u64 {
    if !low(class) {
        return refuse();
    }
    let mut ci = [0u32; 3];
    let r = call32(target!("WinQueryClassInfo"), &[hab as u32, class as u32, ci.as_mut_ptr() as u32]);
    if r != 0 && pci != 0 {
        unsafe {
            let p = pci as *mut u8;
            (p as *mut u32).write_unaligned(ci[0]);
            (p.add(8) as *mut u64).write_unaligned(from32(ci[1]));
            (p.add(16) as *mut u32).write_unaligned(ci[2]);
        }
    }
    r as u64
}

const QWP_PFNWP: i32 = -3;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinQueryWindowPtr(hwnd: u64, index: u64) -> u64 {
    let r = call32(target!("WinQueryWindowPtr"), &[hwnd as u32, index as u32]);
    if index as i32 == QWP_PFNWP { from32(r) } else { r as u64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinSetWindowPtr(hwnd: u64, index: u64, p: u64) -> u64 {
    // Window words are 32 bits: a pointer above 4 GB does not fit.
    let v = if index as i32 == QWP_PFNWP {
        to32(p)
    } else if low(p) {
        p as u32
    } else {
        return refuse();
    };
    call32(target!("WinSetWindowPtr"), &[hwnd as u32, index as u32, v]) as u64
}

/// Hooks are not available to 64-bit programs yet (a hook procedure runs
/// in other processes too).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinSetHook(_hab: u64, _hmq: u64, _ty: u64, _pfn: u64, _hmod: u64) -> u64 {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WinReleaseHook(_hab: u64, _hmq: u64, _ty: u64, _pfn: u64, _hmod: u64) -> u64 {
    0
}

/// DevOpenDC: DEVOPENSTRUC is an array of pointers (`count` of them).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DevOpenDC(hab: u64, ty: u64, token: u64, count: u64, pdop: u64, hdc: u64) -> u64 {
    if !low(token) {
        return refuse();
    }
    let mut d = [0u32; 9];
    let n = (count as usize).min(9);
    for (i, v) in d.iter_mut().enumerate().take(n) {
        let p = if pdop != 0 { unsafe { *(pdop as *const u64).add(i) } } else { 0 };
        if !low(p) {
            return refuse();
        }
        *v = p as u32;
    }
    let p = if pdop != 0 { d.as_ptr() as u32 } else { 0 };
    call32(target!("DevOpenDC"), &[hab as u32, ty as u32, token as u32, count as u32, p, hdc as u32]) as u64
}
