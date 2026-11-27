//! W32NTDLL.DLL: the NTDLL of rs2's Win32 layer (docs/WIN32.md).
//!
//! - **Process start.** The kernel starts a PE program at
//!   [`Rs2W32Start`], which registers the exception dispatcher, sets up the
//!   program's and its DLLs' TLS, calls each PE DLL's entry point
//!   (DllMain, DLL_PROCESS_ATTACH) in dependency order, and then the
//!   program's entry point.
//! - **Threads.** W32KERN's thread start calls [`Rs2W32ThreadAttach`]
//!   (TLS blocks, DLL_THREAD_ATTACH) and its ExitThread
//!   [`Rs2W32ThreadDetach`]; ExitProcess calls [`Rs2W32ProcessDetach`].
//!   LoadLibrary calls [`Rs2W32InitModules`] for the DLLs it brought in.
//! - **Exceptions.** The kernel hands CPU exceptions to [`dispatch`] with
//!   an EXCEPTION_RECORD and a CONTEXT (kernel/src/api/win32.rs), as NT
//!   calls KiUserExceptionDispatcher; RaiseException comes here through
//!   [`Rs2W32Raise`]. The dispatcher walks the registration records at
//!   FS:[0], calling each handler as `cdecl (record, frame, context,
//!   dispatcher context)`; ExceptionContinueExecution resumes in the
//!   context, nothing handling it ends in the unhandled filter
//!   (SetUnhandledExceptionFilter) and then the kernel's default action.
//!   RtlUnwind calls the handlers of the frames above a target with
//!   EXCEPTION_UNWINDING and takes the frames off the chain.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub mod pe;
mod thunks;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

unsafe extern "C" {
    #[link_name = "RS2API.430"]
    fn Rs2W32Init(dispatcher: u32) -> u32;
    #[link_name = "RS2API.431"]
    fn Rs2W32Continue(ctx: *const Context) -> u32;
    #[link_name = "RS2API.432"]
    fn Rs2W32Unhandled(rec: *const ExceptionRecord, ctx: *const Context) -> u32;
    #[link_name = "RS2API.433"]
    fn Rs2W32Modules(buf: *mut u32, max: u32) -> u32;
}

// ---- Windows structures ----

pub const EXCEPTION_NONCONTINUABLE: u32 = 0x01;
pub const EXCEPTION_UNWINDING: u32 = 0x02;
pub const EXCEPTION_EXIT_UNWIND: u32 = 0x04;
pub const EXCEPTION_NESTED_CALL: u32 = 0x10;

pub const STATUS_UNWIND: u32 = 0xC000_0027;
pub const STATUS_NONCONTINUABLE_EXCEPTION: u32 = 0xC000_0025;
pub const STATUS_INVALID_DISPOSITION: u32 = 0xC000_0026;

const DISPOSITION_CONTINUE_EXECUTION: u32 = 0;
const DISPOSITION_CONTINUE_SEARCH: u32 = 1;

const END_OF_CHAIN: u32 = 0xffff_ffff;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExceptionRecord {
    pub code: u32,
    pub flags: u32,
    pub record: *mut ExceptionRecord,
    pub address: u32,
    pub nparams: u32,
    pub info: [u32; 15],
}

/// CONTEXT (i386), 0x2cc bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Context {
    pub flags: u32,
    pub dr: [u32; 6],
    pub float_save: [u8; 112],
    pub gs: u32,
    pub fs: u32,
    pub es: u32,
    pub ds: u32,
    pub edi: u32,
    pub esi: u32,
    pub ebx: u32,
    pub edx: u32,
    pub ecx: u32,
    pub eax: u32,
    pub ebp: u32,
    pub eip: u32,
    pub cs: u32,
    pub eflags: u32,
    pub esp: u32,
    pub ss: u32,
    pub extended: [u8; 512],
}

const _: () = assert!(core::mem::size_of::<Context>() == 0x2cc);
const _: () = assert!(core::mem::size_of::<ExceptionRecord>() == 0x50);

#[repr(C)]
pub struct ExceptionPointers {
    pub record: *mut ExceptionRecord,
    pub context: *mut Context,
}

// ---- The TEB ----

pub mod teb {
    pub const EXCEPTION_LIST: usize = 0x00;
    pub const STACK_BASE: usize = 0x04;
    pub const STACK_LIMIT: usize = 0x08;
    pub const SELF: usize = 0x18;
    pub const TLS_POINTER: usize = 0x2c;
    pub const PEB: usize = 0x30;
    pub const LAST_ERROR: usize = 0x34;
    pub const TLS_SLOTS: usize = 0xe10;
}

/// The running thread's TEB.
#[inline]
pub fn teb() -> *mut u8 {
    let t: u32;
    unsafe { core::arch::asm!("mov {}, fs:[0x18]", out(reg) t, options(nostack, readonly, preserves_flags)) };
    t as *mut u8
}

unsafe fn teb_u32(off: usize) -> u32 {
    unsafe { (teb().add(off) as *const u32).read_volatile() }
}

unsafe fn set_teb_u32(off: usize, v: u32) {
    unsafe { (teb().add(off) as *mut u32).write_volatile(v) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn NtCurrentTeb() -> *mut u8 {
    teb()
}

// ---- Calling into Windows code ----
//
// Windows code may use any convention for its callbacks and need not
// keep EBX, ESI, EDI and EBP the way the C ABI says (Delphi's handlers),
// nor keep the stack 16-byte aligned: these shims save the registers,
// restore ESP whoever popped the arguments, and realign for Rust.

core::arch::global_asm!(
    // rs2_call4(fn, a, b, c, d): fn(a, b, c, d), cdecl or stdcall.
    ".globl rs2_call4",
    "rs2_call4:",
    "push ebp",
    "mov ebp, esp",
    "push ebx",
    "push esi",
    "push edi",
    "push dword ptr [ebp+24]",
    "push dword ptr [ebp+20]",
    "push dword ptr [ebp+16]",
    "push dword ptr [ebp+12]",
    "call dword ptr [ebp+8]",
    "lea esp, [ebp-12]",
    "pop edi",
    "pop esi",
    "pop ebx",
    "pop ebp",
    "ret",
    // The kernel's entry to the dispatcher: [esp+4] the record, [esp+8]
    // the context.
    ".globl rs2_w32_kidispatch",
    "rs2_w32_kidispatch:",
    "mov eax, [esp+4]",
    "mov ecx, [esp+8]",
    "and esp, -16",
    "sub esp, 8",
    "push ecx",
    "push eax",
    "call rs2_w32_dispatch",
    "ud2",
    // Rs2W32Start(entry, base, peb), entered by the kernel.
    ".globl Rs2W32Start",
    "Rs2W32Start:",
    "mov eax, [esp+4]",
    "mov ecx, [esp+8]",
    "mov edx, [esp+12]",
    "and esp, -16",
    "sub esp, 4",
    "push edx",
    "push ecx",
    "push eax",
    "call rs2_w32_start",
    "ud2",
    // Rs2W32Raise(code, flags, nparams, params), stdcall: RaiseException.
    // The context is the caller's as after the return.
    ".globl Rs2W32Raise",
    "Rs2W32Raise:",
    "push ebp",
    "mov ebp, esp",
    "sub esp, 0x330",
    "and esp, -16",
    "mov [esp+0xb0], eax",
    "mov [esp+0xac], ecx",
    "mov [esp+0xa8], edx",
    "mov [esp+0xa4], ebx",
    "mov [esp+0xa0], esi",
    "mov [esp+0x9c], edi",
    "mov eax, [ebp]",
    "mov [esp+0xb4], eax",
    "mov eax, [ebp+4]",
    "mov [esp+0xb8], eax",
    "lea eax, [ebp+24]",
    "mov [esp+0xc4], eax",
    "xor eax, eax",
    "mov ax, cs",
    "mov [esp+0xbc], eax",
    "mov ax, ss",
    "mov [esp+0xc8], eax",
    "mov ax, ds",
    "mov [esp+0x98], eax",
    "mov ax, es",
    "mov [esp+0x94], eax",
    "mov ax, fs",
    "mov [esp+0x90], eax",
    "mov ax, gs",
    "mov [esp+0x8c], eax",
    "pushfd",
    "pop eax",
    "mov [esp+0xc0], eax",
    "mov eax, esp",
    "lea ecx, [esp+0x2d0]",
    "push 0",
    "push 0",
    "push dword ptr [ebp+20]",
    "push dword ptr [ebp+16]",
    "push dword ptr [ebp+12]",
    "push dword ptr [ebp+8]",
    "push eax",
    "push ecx",
    "call rs2_w32_raise",
    "ud2",
    // RtlCaptureContext(ctx), stdcall: the caller's registers, as after
    // the return.
    ".globl RtlCaptureContext",
    "RtlCaptureContext:",
    "push eax",
    "mov eax, [esp+8]",
    "mov dword ptr [eax], 0x10007",
    "mov [eax+0xac], ecx",
    "mov [eax+0xa8], edx",
    "mov [eax+0xa4], ebx",
    "mov [eax+0xa0], esi",
    "mov [eax+0x9c], edi",
    "mov [eax+0xb4], ebp",
    "mov ecx, [esp+4]",
    "mov [eax+0xb8], ecx",
    "lea ecx, [esp+12]",
    "mov [eax+0xc4], ecx",
    "pop ecx",
    "mov [eax+0xb0], ecx",
    "xor ecx, ecx",
    "mov cx, cs",
    "mov [eax+0xbc], ecx",
    "mov cx, ss",
    "mov [eax+0xc8], ecx",
    "mov cx, ds",
    "mov [eax+0x98], ecx",
    "mov cx, es",
    "mov [eax+0x94], ecx",
    "mov cx, fs",
    "mov [eax+0x90], ecx",
    "mov cx, gs",
    "mov [eax+0x8c], ecx",
    "pushfd",
    "pop ecx",
    "mov [eax+0xc0], ecx",
    "mov ecx, [eax+0xac]",
    "mov eax, [eax+0xb0]",
    "ret 4",
);

unsafe extern "C" {
    fn rs2_call4(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32;
    fn rs2_w32_kidispatch();
}

/// Call Windows code `f(a, b, c, d)` (cdecl or stdcall; extra arguments
/// are harmless for either).
pub fn call(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    unsafe { rs2_call4(f, a, b, c, d) }
}

// ---- A lock ----

pub struct Lock(AtomicBool);

impl Lock {
    pub const fn new() -> Lock {
        Lock(AtomicBool::new(false))
    }

    pub fn lock(&self) -> LockGuard<'_> {
        while self.0.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            unsafe { os2::DosSleep(0) };
        }
        LockGuard(self)
    }
}

pub struct LockGuard<'a>(&'a Lock);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        (self.0).0.store(false, Ordering::Release);
    }
}

// ---- Modules ----

/// A PE module of the process.
#[derive(Clone, Copy)]
struct Mod {
    base: u32,
    hmte: u32,
    /// DLL_PROCESS_ATTACH done (or the program itself).
    attached: bool,
    /// Its TLS index, if it has a TLS directory.
    tls: Option<u32>,
}

static MODS: Lock = Lock::new();
static mut MODULES: Vec<Mod> = Vec::new();
static mut TLS_TEMPLATES: Vec<pe::Tls> = Vec::new();
static EXE_BASE: AtomicU32 = AtomicU32::new(0);

const DLL_PROCESS_DETACH: u32 = 0;
const DLL_PROCESS_ATTACH: u32 = 1;
const DLL_THREAD_ATTACH: u32 = 2;
const DLL_THREAD_DETACH: u32 = 3;

#[allow(static_mut_refs)]
fn modules() -> &'static mut Vec<Mod> {
    unsafe { &mut MODULES }
}

#[allow(static_mut_refs)]
fn tls_templates() -> &'static mut Vec<pe::Tls> {
    unsafe { &mut TLS_TEMPLATES }
}

/// The kernel's list of the process's PE modules, dependencies first.
fn kernel_modules() -> Vec<(u32, u32)> {
    let mut buf = alloc::vec![0u32; 128];
    loop {
        let max = buf.len() as u32 / 2;
        let n = unsafe { Rs2W32Modules(buf.as_mut_ptr(), max) };
        if n <= max {
            return buf[..n as usize * 2].chunks(2).map(|c| (c[0], c[1])).collect();
        }
        buf = alloc::vec![0u32; n as usize * 2 + 16];
    }
}

/// Bring the module list up to date: new modules get their TLS index.
/// Returns the ones that need DLL_PROCESS_ATTACH, in order.
fn refresh() -> Vec<Mod> {
    let _g = MODS.lock();
    let exe = EXE_BASE.load(Ordering::Relaxed);
    let mut new = Vec::new();
    for (base, hmte) in kernel_modules() {
        if modules().iter().any(|m| m.base == base) {
            continue;
        }
        let mut m = Mod { base, hmte, attached: base == exe, tls: None };
        if let Some(t) = unsafe { pe::tls(base) } {
            let index = tls_templates().len() as u32;
            unsafe { (t.index as *mut u32).write(index) };
            tls_templates().push(t);
            m.tls = Some(index);
        }
        modules().push(m);
        if !m.attached {
            new.push(m);
        }
    }
    new
}

/// The running thread's TLS blocks: one per module with a TLS directory,
/// a copy of its template (TEB+0x2C points to the array).
fn tls_alloc_thread() {
    let templates: Vec<pe::Tls> = {
        let _g = MODS.lock();
        tls_templates().clone()
    };
    if templates.is_empty() {
        return;
    }
    let old = unsafe { teb_u32(teb::TLS_POINTER) } as *mut u32;
    let mut have = 0;
    if !old.is_null() {
        // The array's length is kept in the word before it.
        have = unsafe { old.sub(1).read() } as usize;
    }
    if have >= templates.len() {
        return;
    }
    let mut arr: Vec<u32> = Vec::with_capacity(templates.len() + 1);
    arr.push(templates.len() as u32);
    for (i, t) in templates.iter().enumerate() {
        if i < have {
            arr.push(unsafe { old.add(i).read() });
            continue;
        }
        let size = (t.end - t.start) as usize + t.zero_fill as usize;
        let mut b: Vec<u8> = alloc::vec![0u8; size.max(4)];
        unsafe { core::ptr::copy_nonoverlapping(t.start as *const u8, b.as_mut_ptr(), (t.end - t.start) as usize) };
        arr.push(b.as_mut_ptr() as u32);
        core::mem::forget(b);
    }
    let p = arr.as_mut_ptr();
    core::mem::forget(arr);
    unsafe { set_teb_u32(teb::TLS_POINTER, p.add(1) as u32) };
}

fn tls_callbacks(m: &Mod, reason: u32) {
    if m.tls.is_none() {
        return;
    }
    let Some(t) = (unsafe { pe::tls(m.base) }) else { return };
    let mut at = t.callbacks;
    if at == 0 {
        return;
    }
    loop {
        let f = unsafe { (at as *const u32).read() };
        if f == 0 {
            break;
        }
        call(f, m.base, reason, 0, 0);
        at += 4;
    }
}

/// Call a DLL's entry point (and TLS callbacks). False if DllMain
/// refused DLL_PROCESS_ATTACH.
fn dll_main(m: &Mod, reason: u32, reserved: u32) -> bool {
    tls_callbacks(m, reason);
    let entry = unsafe { pe::entry(m.base) };
    if entry == 0 || m.base == EXE_BASE.load(Ordering::Relaxed) {
        return true;
    }
    call(entry, m.base, reason, reserved, 0) != 0 || reason != DLL_PROCESS_ATTACH
}

fn attach_new(reserved: u32) -> bool {
    let new = refresh();
    tls_alloc_thread();
    let mut ok = true;
    for m in &new {
        if !dll_main(m, DLL_PROCESS_ATTACH, reserved) {
            ok = false;
        }
        let _g = MODS.lock();
        if let Some(x) = modules().iter_mut().find(|x| x.base == m.base) {
            x.attached = true;
        }
    }
    ok
}

/// LoadLibrary brought DLLs in: their DLL_PROCESS_ATTACH. 0, or 1 if a
/// DllMain refused.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32InitModules() -> u32 {
    if attach_new(0) { 0 } else { 1 }
}

/// A new thread (W32KERN's thread start): its TLS, DLL_THREAD_ATTACH.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32ThreadAttach() {
    tls_alloc_thread();
    let ms: Vec<Mod> = {
        let _g = MODS.lock();
        modules().iter().filter(|m| m.attached).copied().collect()
    };
    for m in &ms {
        dll_main(m, DLL_THREAD_ATTACH, 0);
    }
}

/// A thread ends (ExitThread): DLL_THREAD_DETACH, last loaded first.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32ThreadDetach() {
    let ms: Vec<Mod> = {
        let _g = MODS.lock();
        modules().iter().filter(|m| m.attached).copied().collect()
    };
    for m in ms.iter().rev() {
        dll_main(m, DLL_THREAD_DETACH, 0);
    }
}

static DETACHED: AtomicBool = AtomicBool::new(false);

/// The process ends (ExitProcess): DLL_PROCESS_DETACH, last loaded first.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32ProcessDetach() {
    if DETACHED.swap(true, Ordering::AcqRel) {
        return;
    }
    let ms: Vec<Mod> = {
        let _g = MODS.lock();
        modules().iter().filter(|m| m.attached).copied().collect()
    };
    for m in ms.iter().rev() {
        dll_main(m, DLL_PROCESS_DETACH, 1);
    }
}

/// The OS/2 module handle of the PE module at `base` (or a module
/// handle: itself), 0 if there is none.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32HmteOfBase(base: u32) -> u32 {
    if base != 0 && base < 0x10000 {
        return base;
    }
    let found = {
        let _g = MODS.lock();
        modules().iter().find(|m| m.base == base).map(|m| m.hmte)
    };
    match found {
        Some(h) => h,
        None => {
            // Loaded since the list was last brought up to date.
            refresh();
            let _g = MODS.lock();
            modules().iter().find(|m| m.base == base).map_or(0, |m| m.hmte)
        }
    }
}

/// The image base of the PE module with OS/2 module handle `hmte`, 0 if
/// it is not a PE module.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32BaseOfHmte(hmte: u32) -> u32 {
    refresh();
    let _g = MODS.lock();
    modules().iter().find(|m| m.hmte == hmte).map_or(0, |m| m.base)
}

/// The PE module containing `addr`: its base, or 0.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32ModuleOfAddress(addr: u32) -> u32 {
    let _g = MODS.lock();
    for m in modules().iter() {
        let size = unsafe { pe::image_size(m.base) };
        if addr >= m.base && addr - m.base < size {
            return m.base;
        }
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rs2_w32_start(entry: u32, base: u32, _peb: u32) -> ! {
    EXE_BASE.store(base, Ordering::Relaxed);
    unsafe { Rs2W32Init(rs2_w32_kidispatch as *const () as u32) };
    if !attach_new(1) {
        // A DLL refused to start: as Windows, the program does not run
        // (STATUS_DLL_INIT_FAILED).
        Rs2W32ProcessDetach();
        unsafe { os2::DosExit(1, 0xC000_0142) };
    }
    // The program's own TLS callbacks, after its DLLs' entry points (as
    // Windows calls them).
    let exe = {
        let _g = MODS.lock();
        modules().iter().find(|m| m.base == base).copied()
    };
    if let Some(m) = exe {
        tls_callbacks(&m, DLL_PROCESS_ATTACH);
    }
    let peb = unsafe { teb_u32(teb::PEB) };
    let code = call(entry, peb, 0, 0, 0);
    Rs2W32ProcessDetach();
    unsafe { os2::DosExit(1, code) };
    loop {}
}

// ---- Exceptions ----

static TOP_FILTER: AtomicU32 = AtomicU32::new(0);

/// SetUnhandledExceptionFilter: the previous filter.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32SetUnhandledFilter(f: u32) -> u32 {
    TOP_FILTER.swap(f, Ordering::AcqRel)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rs2W32QueryUnhandledFilter() -> u32 {
    TOP_FILTER.load(Ordering::Acquire)
}

const EXCEPTION_EXECUTE_HANDLER: i32 = 1;
const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;

fn frame_ok(frame: u32) -> bool {
    let (base, limit) = unsafe { (teb_u32(teb::STACK_BASE), teb_u32(teb::STACK_LIMIT)) };
    // The limit is the lowest committed page; frames are above it, and
    // below the base. A thread whose TEB says nothing useful passes.
    frame & 3 == 0 && (base == 0 || (frame < base && frame >= limit.saturating_sub(0x1000_0000)))
}

#[unsafe(no_mangle)]
extern "C" fn rs2_w32_dispatch(rec: *mut ExceptionRecord, ctx: *mut Context) -> ! {
    unsafe { dispatch(rec, ctx) }
}

#[unsafe(no_mangle)]
extern "C" fn rs2_w32_raise(rec: *mut ExceptionRecord, ctx: *mut Context, code: u32, flags: u32, nparams: u32, params: *const u32) -> ! {
    unsafe {
        let c = &mut *ctx;
        c.flags = 0x10007;
        c.dr = [0; 6];
        c.float_save = [0; 112];
        c.extended = [0; 512];
        let r = &mut *rec;
        r.code = code;
        r.flags = flags & EXCEPTION_NONCONTINUABLE;
        r.record = core::ptr::null_mut();
        r.address = c.eip;
        r.nparams = nparams.min(15);
        r.info = [0; 15];
        if !params.is_null() {
            for i in 0..r.nparams as usize {
                r.info[i] = params.add(i).read();
            }
        }
        dispatch(rec, ctx)
    }
}

/// RtlRaiseException(record): raise it with the caller's context.
#[unsafe(no_mangle)]
pub extern "stdcall" fn RtlRaiseException(rec: *const ExceptionRecord) {
    let r = unsafe { &*rec };
    let n = r.nparams.min(15);
    unsafe { Rs2W32Raise(r.code, r.flags, n, r.info.as_ptr()) };
}

unsafe extern "stdcall" {
    fn Rs2W32Raise(code: u32, flags: u32, nparams: u32, params: *const u32);
}

/// Walk the frames; resume or end.
unsafe fn dispatch(rec: *mut ExceptionRecord, ctx: *mut Context) -> ! {
    unsafe {
        let mut frame = teb_u32(teb::EXCEPTION_LIST);
        let mut dc: u32 = 0;
        while frame != END_OF_CHAIN && frame != 0 {
            if !frame_ok(frame) {
                (*rec).flags |= 0x08; // EXCEPTION_STACK_INVALID
                break;
            }
            let handler = (frame as *const u32).add(1).read();
            let r = call(handler, rec as u32, frame, ctx as u32, &mut dc as *mut u32 as u32);
            match r {
                DISPOSITION_CONTINUE_EXECUTION => {
                    if (*rec).flags & EXCEPTION_NONCONTINUABLE != 0 {
                        raise_nested(rec, STATUS_NONCONTINUABLE_EXCEPTION);
                    }
                    Rs2W32Continue(ctx);
                }
                DISPOSITION_CONTINUE_SEARCH => {}
                _ => {
                    // ExceptionNestedException and the others: go on
                    // searching (the nested frame logic is not here).
                }
            }
            frame = (frame as *const u32).read();
        }
        // Nobody: the unhandled exception filter.
        let f = TOP_FILTER.load(Ordering::Acquire);
        if f != 0 {
            let mut ep = ExceptionPointers { record: rec, context: ctx };
            match call(f, &mut ep as *mut ExceptionPointers as u32, 0, 0, 0) as i32 {
                EXCEPTION_CONTINUE_EXECUTION => {
                    Rs2W32Continue(ctx);
                }
                EXCEPTION_EXECUTE_HANDLER => {
                    // The process ends with the exception code.
                    Rs2W32ProcessDetach();
                    os2::DosExit(1, (*rec).code);
                }
                _ => {}
            }
        }
        Rs2W32Unhandled(rec, ctx);
        loop {}
    }
}

/// A handler did something not allowed (continued a noncontinuable
/// exception): a new exception whose record points to the first.
unsafe fn raise_nested(rec: *mut ExceptionRecord, code: u32) -> ! {
    unsafe {
        let mut r = *rec;
        r.code = code;
        r.flags = EXCEPTION_NONCONTINUABLE;
        r.record = rec;
        r.nparams = 0;
        Rs2W32Unhandled(&r, core::ptr::null());
        loop {}
    }
}

/// RtlUnwind(targetFrame, targetIp, record, returnValue): call the
/// handlers of the frames above `target` with EXCEPTION_UNWINDING and take
/// them off the chain, then return (as Windows on i386, to the caller,
/// with the return value).
#[unsafe(no_mangle)]
pub extern "stdcall" fn RtlUnwind(target: u32, _target_ip: u32, rec: *mut ExceptionRecord, retval: u32) -> u32 {
    unsafe {
        let mut own = ExceptionRecord { code: STATUS_UNWIND, flags: 0, record: core::ptr::null_mut(), address: 0, nparams: 0, info: [0; 15] };
        let rec = if rec.is_null() { &mut own as *mut ExceptionRecord } else { rec };
        (*rec).flags |= EXCEPTION_UNWINDING | if target == 0 || target == END_OF_CHAIN { EXCEPTION_EXIT_UNWIND } else { 0 };
        let mut ctx: Context = core::mem::zeroed();
        ctx.flags = 0x10007;
        let mut dc: u32 = 0;
        let mut frame = teb_u32(teb::EXCEPTION_LIST);
        while frame != END_OF_CHAIN && frame != 0 && frame != target {
            if !frame_ok(frame) {
                break;
            }
            let handler = (frame as *const u32).add(1).read();
            call(handler, rec as u32, frame, &mut ctx as *mut Context as u32, &mut dc as *mut u32 as u32);
            let next = (frame as *const u32).read();
            set_teb_u32(teb::EXCEPTION_LIST, next);
            frame = next;
        }
        retval
    }
}

// ---- Small things NTDLL has ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn RtlZeroMemory(d: *mut u8, n: usize) {
    unsafe { core::ptr::write_bytes(d, 0, n) };
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RtlFillMemory(d: *mut u8, n: usize, v: u8) {
    unsafe { core::ptr::write_bytes(d, v, n) };
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RtlMoveMemory(d: *mut u8, s: *const u8, n: usize) {
    unsafe { core::ptr::copy(s, d, n) };
}

/// RtlGetVersion(PRTL_OSVERSIONINFOW): Windows XP.
#[unsafe(no_mangle)]
pub extern "stdcall" fn RtlGetVersion(info: *mut u32) -> u32 {
    unsafe {
        let size = info.read();
        info.add(1).write(5);
        info.add(2).write(1);
        info.add(3).write(2600);
        info.add(4).write(2);
        let csd = info.add(5) as *mut u16;
        let n = ((size as usize).saturating_sub(20) / 2).min(128);
        for i in 0..n {
            csd.add(i).write(0);
        }
    }
    0
}
