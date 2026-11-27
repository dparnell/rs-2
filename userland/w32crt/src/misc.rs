//! setjmp and longjmp, threads, processes, the environment, the console
//! (conio).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::start::{_environ, _wenviron};
use crate::{cstr, k, set_dos_errno, set_errno, wstr, z, Global, EINVAL, ENOENT};

// ---- setjmp ----
//
// _JUMP_BUFFER (i386): Ebp, Ebx, Edi, Esi, Esp, Eip, Registration (the
// SEH chain head), TryLevel, Cookie, UnwindFunc, UnwindData[6]. longjmp
// unwinds the SEH frames the jump leaves (their __finally blocks run),
// then restores the registers.

core::arch::global_asm!(
    ".globl _setjmp3",
    "_setjmp3:",
    ".globl _setjmp",
    "_setjmp:",
    ".globl setjmp",
    "setjmp:",
    "mov edx, [esp+4]",
    "mov [edx], ebp",
    "mov [edx+4], ebx",
    "mov [edx+8], edi",
    "mov [edx+12], esi",
    "lea eax, [esp+4]",
    "mov [edx+16], eax",
    "mov eax, [esp]",
    "mov [edx+20], eax",
    "mov eax, fs:[0]",
    "mov [edx+24], eax",
    "mov dword ptr [edx+28], -1",
    "xor eax, eax",
    "ret",
    ".globl longjmp",
    "longjmp:",
    "mov esi, [esp+4]",
    "mov edi, [esp+8]",
    "test edi, edi",
    "jnz 2f",
    "inc edi",
    "2:",
    "and esp, -16",
    "sub esp, 12",
    "push dword ptr [esi+24]",
    "call rs2_crt_longjmp_unwind",
    "mov edx, esi",
    "mov ecx, edi",
    "mov ebp, [edx]",
    "mov ebx, [edx+4]",
    "mov edi, [edx+8]",
    "mov esi, [edx+12]",
    "mov esp, [edx+16]",
    "mov eax, ecx",
    "jmp dword ptr [edx+20]",
);

unsafe extern "stdcall" {
    #[link_name = "W32NTDLL.RtlUnwind"]
    fn RtlUnwind(target: u32, ip: u32, rec: u32, ret: u32) -> u32;
}

#[unsafe(no_mangle)]
extern "C" fn rs2_crt_longjmp_unwind(reg: u32) {
    let head: u32;
    unsafe { core::arch::asm!("mov {}, fs:[0]", out(reg) head, options(nostack, readonly)) };
    if head != reg && reg != 0 {
        unsafe { RtlUnwind(reg, 0, 0, 0) };
    }
}

// ---- Threads ----

struct Start {
    f: u32,
    arg: u32,
}

core::arch::global_asm!(
    // The thread's first code (CreateThread calls it stdcall with one
    // argument): realign for Rust.
    ".globl rs2_crt_thread_entry",
    "rs2_crt_thread_entry:",
    "mov eax, [esp+4]",
    "and esp, -16",
    "sub esp, 12",
    "push eax",
    "call rs2_crt_thread_main",
    "ud2",
);

unsafe extern "C" {
    fn rs2_crt_thread_entry();
}

#[unsafe(no_mangle)]
extern "C" fn rs2_crt_thread_main(p: u32) -> ! {
    let s = unsafe { Box::from_raw(p as *mut Start) };
    crate::call(s.f, s.arg, 0, 0, 0);
    _endthread()
}

/// _beginthread(start (cdecl void (void *)), stack, arg).
#[unsafe(no_mangle)]
pub extern "C" fn _beginthread(f: u32, stack: u32, arg: u32) -> u32 {
    let b = Box::into_raw(Box::new(Start { f, arg })) as u32;
    let h = unsafe { k::CreateThread(0, stack, rs2_crt_thread_entry as *const () as u32, b, 0, core::ptr::null_mut()) };
    if h == 0 {
        unsafe { drop(Box::from_raw(b as *mut Start)) };
        set_dos_errno(unsafe { k::GetLastError() });
        return u32::MAX;
    }
    h
}

/// _beginthreadex: CreateThread's arguments, its start a stdcall
/// function.
#[unsafe(no_mangle)]
pub extern "C" fn _beginthreadex(_sa: u32, stack: u32, f: u32, arg: u32, flags: u32, tid: *mut u32) -> u32 {
    let h = unsafe { k::CreateThread(0, stack, f, arg, flags, tid) };
    if h == 0 {
        set_dos_errno(unsafe { k::GetLastError() });
    }
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn _endthread() -> ! {
    unsafe { k::ExitThread(0) }
}

#[unsafe(no_mangle)]
pub extern "C" fn _endthreadex(code: u32) -> ! {
    unsafe { k::ExitThread(code) }
}

// ---- Processes ----

#[unsafe(no_mangle)]
pub extern "C" fn _getpid() -> i32 {
    unsafe { k::GetCurrentProcessId() as i32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn _sleep(ms: u32) {
    unsafe { k::Sleep(ms) };
}

#[unsafe(no_mangle)]
pub extern "C" fn _beep(_f: u32, _d: u32) {}

/// A command line from arguments, quoted as the C runtime splits them.
fn quote_args(args: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            v.push(b' ');
        }
        if !a.is_empty() && !a.iter().any(|&c| c == b' ' || c == b'\t' || c == b'"') {
            v.extend_from_slice(a);
            continue;
        }
        v.push(b'"');
        let mut bs = 0;
        for &c in a.iter() {
            if c == b'\\' {
                bs += 1;
                continue;
            }
            if c == b'"' {
                v.extend(core::iter::repeat_n(b'\\', bs * 2 + 1));
            } else {
                v.extend(core::iter::repeat_n(b'\\', bs));
            }
            bs = 0;
            v.push(c);
        }
        v.extend(core::iter::repeat_n(b'\\', bs * 2));
        v.push(b'"');
    }
    v
}

/// Start a program: its process handle, or None.
fn start(path: &[u8], cmdline: &[u8]) -> Option<u32> {
    let p = z(path);
    let mut cl = z(cmdline);
    let mut si = [0u32; 17];
    si[0] = 68;
    let mut pi = [0u32; 4];
    crate::stdio::flush_all();
    let ok = unsafe { k::CreateProcessA(p.as_ptr(), cl.as_mut_ptr(), 0, 0, 1, 0, core::ptr::null(), core::ptr::null(), si.as_ptr(), pi.as_mut_ptr()) };
    if ok == 0 {
        set_dos_errno(unsafe { k::GetLastError() });
        return None;
    }
    unsafe { k::CloseHandle(pi[1]) };
    Some(pi[0])
}

fn wait_code(h: u32) -> i32 {
    unsafe { k::WaitForSingleObject(h, 0xffff_ffff) };
    let mut code = 0u32;
    unsafe {
        k::GetExitCodeProcess(h, &mut code);
        k::CloseHandle(h);
    }
    code as i32
}

pub fn env_value(name: &[u8]) -> Option<Vec<u8>> {
    let p = unsafe { getenv(z(name).as_ptr()) };
    (!p.is_null()).then(|| unsafe { cstr(p) }.to_vec())
}

/// system(): the command through COMSPEC (rs2's CMD.EXE).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn system(cmd: *const u8) -> i32 {
    let comspec = env_value(b"COMSPEC").unwrap_or_else(|| b"CMD.EXE".to_vec());
    if cmd.is_null() {
        return 1;
    }
    let mut cl = quote_args(&[&comspec]);
    cl.extend_from_slice(b" /C ");
    cl.extend_from_slice(unsafe { cstr(cmd) });
    match start(&comspec, &cl) {
        Some(h) => wait_code(h),
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wsystem(cmd: *const u16) -> i32 {
    if cmd.is_null() {
        return 1;
    }
    let c = z(&crate::narrow(unsafe { wstr(cmd) }));
    unsafe { system(c.as_ptr()) }
}

unsafe fn argv_list<'a>(argv: *const *const u8) -> Vec<&'a [u8]> {
    let mut v = Vec::new();
    let mut p = argv;
    while !p.is_null() && !unsafe { *p }.is_null() {
        v.push(unsafe { cstr(*p) });
        p = unsafe { p.add(1) };
    }
    v
}

/// A program by name: with ".EXE" if it has no extension, along the PATH
/// for the "p" forms.
fn find(name: &[u8], search: bool) -> Vec<u8> {
    let exists = |p: &[u8]| unsafe { k::GetFileAttributesA(z(p).as_ptr()) } != 0xffff_ffff;
    let mut cands = alloc::vec![name.to_vec()];
    let base = name.rsplit(|&c| c == b'\\' || c == b'/').next().unwrap_or(name);
    if !base.contains(&b'.') {
        let mut e = name.to_vec();
        e.extend_from_slice(b".EXE");
        cands.insert(0, e);
    }
    for c in &cands {
        if exists(c) {
            return c.clone();
        }
    }
    if search && !name.contains(&b'\\') && !name.contains(&b':') {
        if let Some(path) = env_value(b"PATH") {
            for d in path.split(|&c| c == b';').filter(|d| !d.is_empty()) {
                for c in &cands {
                    let mut p = d.to_vec();
                    if !p.ends_with(b"\\") {
                        p.push(b'\\');
                    }
                    p.extend_from_slice(c);
                    if exists(&p) {
                        return p;
                    }
                }
            }
        }
    }
    cands.pop().unwrap_or_default()
}

/// _spawnv's modes: _P_WAIT 0, _P_NOWAIT 1, _P_OVERLAY 2, _P_NOWAITO 3,
/// _P_DETACH 4.
fn spawn(mode: i32, path: *const u8, argv: *const *const u8, search: bool) -> i32 {
    let args = unsafe { argv_list(argv) };
    let prog = find(unsafe { cstr(path) }, search);
    let cl = quote_args(&args);
    let Some(h) = start(&prog, &cl) else { return -1 };
    match mode {
        0 => wait_code(h),
        2 => {
            let c = wait_code(h);
            crate::start::exit(c)
        }
        _ => h as i32,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _spawnv(mode: i32, path: *const u8, argv: *const *const u8) -> i32 {
    spawn(mode, path, argv, false)
}

#[unsafe(no_mangle)]
pub extern "C" fn _spawnvp(mode: i32, path: *const u8, argv: *const *const u8) -> i32 {
    spawn(mode, path, argv, true)
}

#[unsafe(no_mangle)]
pub extern "C" fn _spawnve(mode: i32, path: *const u8, argv: *const *const u8, _env: u32) -> i32 {
    spawn(mode, path, argv, false)
}

#[unsafe(no_mangle)]
pub extern "C" fn _spawnvpe(mode: i32, path: *const u8, argv: *const *const u8, _env: u32) -> i32 {
    spawn(mode, path, argv, true)
}

#[unsafe(no_mangle)]
pub extern "C" fn _execv(path: *const u8, argv: *const *const u8) -> i32 {
    spawn(2, path, argv, false)
}

#[unsafe(no_mangle)]
pub extern "C" fn _execvp(path: *const u8, argv: *const *const u8) -> i32 {
    spawn(2, path, argv, true)
}

#[unsafe(no_mangle)]
pub extern "C" fn _execve(path: *const u8, argv: *const *const u8, _env: u32) -> i32 {
    spawn(2, path, argv, false)
}

#[unsafe(no_mangle)]
pub extern "C" fn _execvpe(path: *const u8, argv: *const *const u8, _env: u32) -> i32 {
    spawn(2, path, argv, true)
}

/// _cwait(&status, process handle, action).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _cwait(status: *mut i32, h: u32, _action: i32) -> i32 {
    let c = wait_code(h);
    if !status.is_null() {
        unsafe { *status = c };
    }
    h as i32
}

// ---- The environment (the runtime's copy, _environ) ----

static ENV_LOCK: Global<()> = Global::new(());

fn key_len(e: &[u8]) -> usize {
    e.iter().position(|&c| c == b'=').unwrap_or(e.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getenv(name: *const u8) -> *const u8 {
    let n = unsafe { cstr(name) };
    if n.is_empty() {
        return core::ptr::null();
    }
    ENV_LOCK.with(|_| {
        let mut p = unsafe { _environ };
        while !p.is_null() && !unsafe { *p }.is_null() {
            let e = unsafe { cstr(*p) };
            let k = key_len(e);
            if k == n.len() && e[..k].eq_ignore_ascii_case(n) {
                return unsafe { (*p).add(k + 1) } as *const u8;
            }
            p = unsafe { p.add(1) };
        }
        core::ptr::null()
    })
}

fn set_env(name: &[u8], value: Option<&[u8]>) {
    ENV_LOCK.with(|_| {
        let mut items: Vec<Vec<u8>> = Vec::new();
        let mut p = unsafe { _environ };
        while !p.is_null() && !unsafe { *p }.is_null() {
            let e = unsafe { cstr(*p) };
            let k = key_len(e);
            if !(k == name.len() && e[..k].eq_ignore_ascii_case(name)) {
                items.push(e.to_vec());
            }
            p = unsafe { p.add(1) };
        }
        if let Some(v) = value {
            let mut e = name.to_vec();
            e.push(b'=');
            e.extend_from_slice(v);
            items.push(e);
        }
        let mut ptrs: Vec<*mut u8> = items.iter().map(|e| crate::c_alloc_str(e)).collect();
        ptrs.push(core::ptr::null_mut());
        let wide: Vec<*mut u16> = items
            .iter()
            .map(|e| {
                let mut w = crate::widen(e);
                w.push(0);
                let p = w.as_mut_ptr();
                core::mem::forget(w);
                p
            })
            .chain(core::iter::once(core::ptr::null_mut()))
            .collect();
        unsafe {
            _environ = Box::leak(ptrs.into_boxed_slice()).as_mut_ptr();
            _wenviron = Box::leak(wide.into_boxed_slice()).as_mut_ptr();
        }
    });
    let n = z(name);
    let v = value.map(z);
    unsafe { k::SetEnvironmentVariableA(n.as_ptr(), v.as_ref().map_or(core::ptr::null(), |v| v.as_ptr())) };
}

/// _putenv("NAME=value"); "NAME=" removes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _putenv(s: *const u8) -> i32 {
    let e = unsafe { cstr(s) };
    let k = key_len(e);
    if k == 0 || k == e.len() {
        set_errno(EINVAL);
        return -1;
    }
    let v = &e[k + 1..];
    set_env(&e[..k], if v.is_empty() { None } else { Some(v) });
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _putenv_s(name: *const u8, value: *const u8) -> i32 {
    let v = unsafe { cstr(value) };
    set_env(unsafe { cstr(name) }, if v.is_empty() { None } else { Some(v) });
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wputenv(s: *const u16) -> i32 {
    let a = z(&crate::narrow(unsafe { wstr(s) }));
    unsafe { _putenv(a.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wgetenv(name: *const u16) -> *const u16 {
    let n = crate::narrow(unsafe { wstr(name) });
    ENV_LOCK.with(|_| {
        let mut p = unsafe { _wenviron };
        while !p.is_null() && !unsafe { *p }.is_null() {
            let e = crate::narrow(unsafe { wstr(*p) });
            let k = key_len(&e);
            if k == n.len() && e[..k].eq_ignore_ascii_case(&n) {
                return unsafe { (*p).add(k + 1) } as *const u16;
            }
            p = unsafe { p.add(1) };
        }
        core::ptr::null()
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getenv_s(len: *mut usize, buf: *mut u8, n: usize, name: *const u8) -> i32 {
    let v = unsafe { getenv(name) };
    if v.is_null() {
        unsafe { *len = 0 };
        if n > 0 && !buf.is_null() {
            unsafe { *buf = 0 };
        }
        return 0;
    }
    let s = unsafe { cstr(v) };
    unsafe { *len = s.len() + 1 };
    if buf.is_null() || n < s.len() + 1 {
        return crate::ERANGE;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _dupenv_s(out: *mut *mut u8, len: *mut usize, name: *const u8) -> i32 {
    let v = unsafe { getenv(name) };
    unsafe {
        if v.is_null() {
            *out = core::ptr::null_mut();
            if !len.is_null() {
                *len = 0;
            }
            return 0;
        }
        let s = cstr(v);
        *out = crate::c_alloc_str(s);
        if !len.is_null() {
            *len = s.len() + 1;
        }
    }
    0
}

// ---- The console (conio) ----

static PENDING: Global<Vec<i32>> = Global::new(Vec::new());

/// The next key: its character, or 0 then the scan code for an extended
/// key (two calls).
fn key(echo: bool) -> i32 {
    if let Some(c) = PENDING.with(|p| p.pop()) {
        return c;
    }
    let h = unsafe { k::GetStdHandle(0xffff_fff6) };
    loop {
        let mut rec = [0u8; 20];
        let mut n = 0u32;
        if unsafe { k::ReadConsoleInputA(h, rec.as_mut_ptr(), 1, &mut n) } == 0 || n == 0 {
            return -1;
        }
        if u16::from_le_bytes([rec[0], rec[1]]) != 1 || rec[4] == 0 {
            continue;
        }
        let ch = rec[14] as i32;
        let scan = rec[12] as i32;
        if ch != 0 {
            if echo {
                _putch(ch);
            }
            return ch;
        }
        PENDING.with(|p| p.push(scan));
        return 0;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _getch() -> i32 {
    key(false)
}

#[unsafe(no_mangle)]
pub extern "C" fn _getche() -> i32 {
    key(true)
}

#[unsafe(no_mangle)]
pub extern "C" fn _getwch() -> u32 {
    key(false) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn _ungetch(c: i32) -> i32 {
    PENDING.with(|p| p.push(c));
    c
}

#[unsafe(no_mangle)]
pub extern "C" fn _kbhit() -> i32 {
    if PENDING.with(|p| !p.is_empty()) {
        return 1;
    }
    let h = unsafe { k::GetStdHandle(0xffff_fff6) };
    let mut n = 0u32;
    unsafe { k::GetNumberOfConsoleInputEvents(h, &mut n) };
    (n > 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn _putch(c: i32) -> i32 {
    crate::stdio::flush_all();
    crate::io::write_fd(1, &[c as u8]);
    c
}

#[unsafe(no_mangle)]
pub extern "C" fn _putwch(c: u16) -> u16 {
    let b = crate::narrow(&[c]);
    crate::io::write_fd(1, &b);
    c
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _cputs(s: *const u8) -> i32 {
    crate::stdio::flush_all();
    if crate::io::write_fd(1, unsafe { cstr(s) }) < 0 { -1 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _cgets(buf: *mut u8) -> *mut u8 {
    let max = unsafe { *buf } as usize;
    let mut v = alloc::vec![0u8; max + 2];
    let n = crate::io::read_fd(0, &mut v);
    let mut n = n.max(0) as usize;
    while n > 0 && (v[n - 1] == b'\n' || v[n - 1] == b'\r') {
        n -= 1;
    }
    let n = n.min(max.saturating_sub(1));
    unsafe {
        *buf.add(1) = n as u8;
        core::ptr::copy_nonoverlapping(v.as_ptr(), buf.add(2), n);
        *buf.add(2 + n) = 0;
        buf.add(2)
    }
}

#[allow(dead_code)]
fn unused() {
    let _ = ENOENT;
}
