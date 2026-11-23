//! Program start and exit.
//!
//! The LX entry point gets (hmod, 0, environment, command line) from the
//! loader (`os2::Startup` in userland/os2 describes them). `_start` gives
//! thread 1 its TLS, builds `argv` (the command line split as OS/2 C
//! runtimes do: blanks separate, double quotes group, `\"` is a quote) and
//! `environ` (pointers into the environment block), runs `.init_array`
//! with (argc, argv, envp) as glibc does (std's `env::args` gets its
//! arguments that way) and calls `main`.

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void};
use core::ptr::null_mut;
#[cfg(target_arch = "x86_64")]
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::spin::SpinLock;
use crate::sys::*;

#[cfg(target_arch = "x86")]
unsafe extern "C" {
    fn main(argc: c_int, argv: *const *const c_char, envp: *const *const c_char) -> c_int;
    static __init_array_start: Option<unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char)>;
    static __init_array_end: Option<unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char)>;
}

/// The module handle of the program.
pub static mut HMOD: u32 = 0;
pub static mut ARGC: c_int = 0;
pub static mut ARGV: *const *const c_char = core::ptr::null();

/// The arguments, the environment: once per process.
#[cfg(target_arch = "x86_64")]
unsafe fn init_process(env: *const u8, cmdline: *const u8) {
    unsafe {
        if !ARGV.is_null() {
            return;
        }
        crate::misc::init_environ(env);
        let argv = build_argv(cmdline);
        ARGC = argv.len() as c_int - 1;
        ARGV = Vec::leak(argv).as_ptr();
    }
}

#[cfg(target_arch = "x86_64")]
type Main = unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char) -> c_int;

/// A module's constructors, with (argc, argv, envp) as glibc does.
#[cfg(target_arch = "x86_64")]
unsafe fn run_init(m: &crate::tls::Module) {
    type Init = Option<unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char)>;
    let r = m.init_array();
    let mut f = r.start;
    unsafe {
        while f < r.end {
            if let Some(init) = *(f as *const Init) {
                init(ARGC, ARGV, crate::misc::environ as *const *const c_char);
            }
            f += size_of::<Init>();
        }
    }
}

/// A 64-bit program's start (crt64.rs).
#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __rs2_exe_start(m: *mut crate::tls::Module, hmod: u32, env: *const u8, cmdline: *const u8, main: Main) -> ! {
    unsafe {
        HMOD = hmod;
        crate::tls::set_program(m);
        crate::frames::add(m);
        crate::thread::init_main();
        init_process(env, cmdline);
        run_init(&*m);
        exit(main(ARGC, ARGV, crate::misc::environ as *const *const c_char))
    }
}

/// Where a 64-bit program's threads start, whoever made them (crt64.rs):
/// the program's thread-local storage first.
#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __rs2_thread_start(_m: *mut crate::tls::Module, param: usize, f: unsafe extern "C" fn(usize)) {
    unsafe {
        crate::tls::setup(null_mut());
        f(param);
        // (pthreads do not come back.)
        crate::thread::run_tls_dtors();
        crate::tls::teardown();
    }
}

/// A 64-bit DLL's _DLL_InitTerm (crt64.rs). The first call is for the DLL
/// rs2libc is linked into: the DLLs and programs that share it come later.
#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __rs2_dll_init(m: *mut crate::tls::Module, _hmod: u32, flag: u32) -> u32 {
    static HOST: AtomicPtr<crate::tls::Module> = AtomicPtr::new(null_mut());
    unsafe {
        if flag == 0 {
            if HOST.load(Ordering::Relaxed).is_null() {
                HOST.store(m, Ordering::Relaxed);
                crate::tls::in_dll();
                let pib = info_blocks().1;
                HMOD = pib.hmte;
                // A program that shares this rs2libc (its crt64.rs says so):
                // its thread-local storage comes with the first thread's.
                let (mut host, mut program) = (null_mut(), null_mut());
                if DosQueryProcAddr(HMOD, 0, c"RS2_HOST".as_ptr().cast(), &mut host) == 0
                    && *(host as *const usize) == __rs2_exe_start as *const () as usize
                    && DosQueryProcAddr(HMOD, 0, c"RS2_MODULE".as_ptr().cast(), &mut program) == 0
                {
                    crate::tls::set_program(program.cast());
                }
                crate::thread::init_main();
                init_process(pib.pchenv, pib.pchcmd);
            }
            crate::frames::add(m);
            run_init(&*m);
        } else {
            if HOST.load(Ordering::Relaxed) == m {
                run_atexit();
            }
            crate::frames::remove(m);
        }
    }
    1
}

#[cfg(target_arch = "x86")]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub unsafe extern "C" fn _start(hmod: u32, _reserved: u32, env: *const u8, cmdline: *const u8) -> u32 {
    unsafe {
        HMOD = hmod;
        crate::thread::init_main();
        crate::misc::init_environ(env);
        let argv = build_argv(cmdline);
        let argc = argv.len() as c_int - 1;
        let argv = Vec::leak(argv).as_ptr();
        ARGC = argc;
        ARGV = argv;
        let envp = crate::misc::environ as *const *const c_char;
        let mut f = &raw const __init_array_start;
        while f < &raw const __init_array_end {
            if let Some(init) = *f {
                init(argc, argv, envp);
            }
            f = f.add(1);
        }
        let rc = main(argc, argv, envp);
        exit(rc);
    }
}

/// argv from the OS/2 command line: "program\0arguments\0\0".
unsafe fn build_argv(cmdline: *const u8) -> Vec<*const c_char> {
    let mut argv: Vec<*const c_char> = Vec::new();
    unsafe {
        let prog = crate::string::cstr(cmdline as *const c_char);
        argv.push(leak_cstr(prog));
        let args = crate::string::cstr(cmdline.add(prog.len() + 1) as *const c_char);
        for a in split_args(args) {
            argv.push(leak_cstr(&a));
        }
    }
    argv.push(core::ptr::null());
    argv
}

/// Split a command line into arguments.
pub fn split_args(s: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        let mut arg = Vec::new();
        let mut quoted = false;
        while i < s.len() {
            let c = s[i];
            if c == b'\\' && i + 1 < s.len() && s[i + 1] == b'"' {
                arg.push(b'"');
                i += 2;
                continue;
            }
            if c == b'"' {
                quoted = !quoted;
                i += 1;
                continue;
            }
            if !quoted && (c == b' ' || c == b'\t') {
                break;
            }
            arg.push(c);
            i += 1;
        }
        out.push(arg);
    }
    out
}

fn leak_cstr(b: &[u8]) -> *const c_char {
    let mut v = Vec::with_capacity(b.len() + 1);
    v.extend_from_slice(b);
    v.push(0);
    Vec::leak(v).as_ptr() as *const c_char
}

/// Print a message and end the process (rs2libc cannot continue).
pub fn fatal(msg: &str) -> ! {
    let mut n = 0;
    unsafe {
        DosWrite(2, msg.as_ptr(), msg.len() as u32, &mut n);
        DosExit(1, 255);
    }
    loop {}
}

static ATEXIT: SpinLock<Vec<(usize, Option<usize>)>> = SpinLock::new(Vec::new());

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atexit(f: extern "C" fn()) -> c_int {
    ATEXIT.lock().push((f as usize, None));
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __cxa_atexit(f: extern "C" fn(*mut c_void), arg: *mut c_void, _dso: *mut c_void) -> c_int {
    ATEXIT.lock().push((f as usize, Some(arg as usize)));
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn exit(code: c_int) -> ! {
    run_atexit();
    crate::thread::run_tls_dtors();
    unsafe { _exit(code) }
}

fn run_atexit() {
    loop {
        let Some((f, arg)) = ATEXIT.lock().pop() else { break };
        unsafe {
            if let Some(arg) = arg {
                let f: extern "C" fn(*mut c_void) = core::mem::transmute(f);
                f(arg as *mut c_void);
            } else {
                let f: extern "C" fn() = core::mem::transmute(f);
                f();
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _exit(code: c_int) -> ! {
    unsafe { DosExit(1, code as u32 & 0xffff) };
    loop {}
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn abort() -> ! {
    fatal("abnormal program termination\r\n");
}

/// The program's full path (DosQueryModuleName).
pub fn program_path() -> Vec<u8> {
    let mut buf = [0u8; 260];
    unsafe { DosQueryModuleName(HMOD, buf.len() as u32, buf.as_mut_ptr()) };
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    buf[..n].to_vec()
}

#[allow(dead_code)]
fn unused() -> *mut u8 {
    null_mut()
}

/// A path with \ or a drive, as `rooted_paths` says.
pub fn rooted(a: &[u8]) -> Option<Vec<u8>> {
    let drive = a.len() >= 2 && a[0].is_ascii_alphabetic() && a[1] == b':';
    if a.contains(&b'"') || !(drive || a.contains(&b'\\')) {
        return None;
    }
    let mut v: Vec<u8> = if drive { alloc::vec![b'/'] } else { Vec::new() };
    v.extend(a.iter().map(|&c| if c == b'\\' { b'/' } else { c }));
    Some(v)
}

/// The program's arguments. (std reads the array each time.)
pub unsafe fn rooted_args() {
    unsafe {
        for i in 1..ARGC as usize {
            let p = ARGV.add(i) as *mut *const c_char;
            if let Some(v) = rooted(crate::string::cstr(*p)) {
                *p = leak_cstr(&v);
            }
        }
    }
}
