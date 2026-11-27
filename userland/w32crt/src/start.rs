//! Program start and end, the arguments, the environment, atexit,
//! signals, locales; the runtime's exported variables.

use alloc::vec::Vec;
use core::ptr::null_mut;

use crate::{call, cstr, k, wstr, z, Global};

// ---- Exported variables (MSVCRT's data exports) ----

#[unsafe(no_mangle)]
pub static mut __argc: i32 = 0;
#[unsafe(no_mangle)]
pub static mut __argv: *mut *mut u8 = null_mut();
#[unsafe(no_mangle)]
pub static mut __wargv: *mut *mut u16 = null_mut();
#[unsafe(no_mangle)]
pub static mut _environ: *mut *mut u8 = null_mut();
#[unsafe(no_mangle)]
pub static mut _wenviron: *mut *mut u16 = null_mut();
#[unsafe(no_mangle)]
pub static mut __initenv: *mut *mut u8 = null_mut();
#[unsafe(no_mangle)]
pub static mut __winitenv: *mut *mut u16 = null_mut();
#[unsafe(no_mangle)]
pub static mut _acmdln: *mut u8 = null_mut();
#[unsafe(no_mangle)]
pub static mut _wcmdln: *mut u16 = null_mut();
#[unsafe(no_mangle)]
pub static mut _pgmptr: *mut u8 = null_mut();
#[unsafe(no_mangle)]
pub static mut _wpgmptr: *mut u16 = null_mut();
/// _O_TEXT (0x4000) by default.
#[unsafe(no_mangle)]
pub static mut _fmode: i32 = 0x4000;
#[unsafe(no_mangle)]
pub static mut _commode: i32 = 0;
#[unsafe(no_mangle)]
pub static mut __lc_codepage: u32 = 0;
#[unsafe(no_mangle)]
pub static mut __lc_collate_cp: u32 = 0;
#[unsafe(no_mangle)]
pub static mut __mb_cur_max: i32 = 1;
#[unsafe(no_mangle)]
pub static mut _osver: u32 = 2600;
#[unsafe(no_mangle)]
pub static mut _winver: u32 = 0x0501;
#[unsafe(no_mangle)]
pub static mut _winmajor: u32 = 5;
#[unsafe(no_mangle)]
pub static mut _winminor: u32 = 1;
#[unsafe(no_mangle)]
pub static mut _osplatform: u32 = 2;
#[unsafe(no_mangle)]
pub static mut __app_type: i32 = 1;

macro_rules! getter {
    ($name:ident, $var:ident, $ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() -> *mut $ty {
            &raw mut $var
        }
    };
}

getter!(__p___argc, __argc, i32);
getter!(__p___argv, __argv, *mut *mut u8);
getter!(__p___wargv, __wargv, *mut *mut u16);
getter!(__p__environ, _environ, *mut *mut u8);
getter!(__p__wenviron, _wenviron, *mut *mut u16);
getter!(__p___initenv, __initenv, *mut *mut u8);
getter!(__p___winitenv, __winitenv, *mut *mut u16);
getter!(__p__acmdln, _acmdln, *mut u8);
getter!(__p__wcmdln, _wcmdln, *mut u16);
getter!(__p__pgmptr, _pgmptr, *mut u8);
getter!(__p__wpgmptr, _wpgmptr, *mut u16);
getter!(__p__fmode, _fmode, i32);
getter!(__p__commode, _commode, i32);
getter!(__p___mb_cur_max, __mb_cur_max, i32);

#[unsafe(no_mangle)]
pub extern "C" fn ___lc_codepage_func() -> u32 {
    unsafe { __lc_codepage }
}

#[unsafe(no_mangle)]
pub extern "C" fn ___lc_collate_cp_func() -> u32 {
    unsafe { __lc_collate_cp }
}

#[unsafe(no_mangle)]
pub extern "C" fn ___mb_cur_max_func() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _get_pgmptr(p: *mut *mut u8) -> i32 {
    unsafe { *p = _pgmptr };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _get_fmode(p: *mut i32) -> i32 {
    unsafe { *p = _fmode };
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _set_fmode(m: i32) -> i32 {
    unsafe { _fmode = m };
    0
}

// ---- Start ----

/// Split a command line into arguments, as Windows' C runtime does: white
/// space separates outside quotes; 2n backslashes and a quote are n
/// backslashes and a quote that opens or closes, 2n+1 and a quote n and a
/// literal quote; other backslashes are themselves. argv[0] takes no
/// backslash rules.
pub fn split_args<T: Copy + PartialEq + From<u8>>(cl: &[T]) -> Vec<Vec<T>> {
    let (sp, tab, q, bs) = (T::from(b' '), T::from(b'\t'), T::from(b'"'), T::from(b'\\'));
    let mut args = Vec::new();
    let mut i = 0;
    // argv[0].
    let mut a0 = Vec::new();
    let mut inq = false;
    while i < cl.len() {
        let c = cl[i];
        if c == q {
            inq = !inq;
        } else if (c == sp || c == tab) && !inq {
            break;
        } else {
            a0.push(c);
        }
        i += 1;
    }
    args.push(a0);
    loop {
        while i < cl.len() && (cl[i] == sp || cl[i] == tab) {
            i += 1;
        }
        if i >= cl.len() {
            break;
        }
        let mut a = Vec::new();
        let mut inq = false;
        while i < cl.len() {
            let c = cl[i];
            if c == bs {
                let mut n = 0;
                while i < cl.len() && cl[i] == bs {
                    n += 1;
                    i += 1;
                }
                if i < cl.len() && cl[i] == q {
                    a.extend(core::iter::repeat_n(bs, n / 2));
                    if n % 2 == 1 {
                        a.push(q);
                        i += 1;
                    }
                } else {
                    a.extend(core::iter::repeat_n(bs, n));
                }
                continue;
            }
            if c == q {
                // "" inside quotes is a literal quote.
                if inq && i + 1 < cl.len() && cl[i + 1] == q {
                    a.push(q);
                    i += 2;
                    continue;
                }
                inq = !inq;
                i += 1;
                continue;
            }
            if (c == sp || c == tab) && !inq {
                break;
            }
            a.push(c);
            i += 1;
        }
        args.push(a);
    }
    args
}

/// A NULL-terminated array of NUL-terminated copies (never freed).
fn c_array<T: Copy + Default>(items: &[Vec<T>]) -> *mut *mut T {
    let mut ptrs: Vec<*mut T> = Vec::with_capacity(items.len() + 1);
    for it in items {
        let mut v = it.clone();
        v.push(T::default());
        let p = v.as_mut_ptr();
        core::mem::forget(v);
        ptrs.push(p);
    }
    ptrs.push(null_mut());
    let p = ptrs.as_mut_ptr();
    core::mem::forget(ptrs);
    p
}

/// The environment's strings from Windows' block.
fn env_strings() -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut p = unsafe { k::GetEnvironmentStringsA() };
    loop {
        let s = unsafe { cstr(p) };
        if s.is_empty() {
            break;
        }
        // "=C:=C:\" entries are Windows' own.
        if !s.starts_with(b"=") {
            v.push(s.to_vec());
        }
        p = unsafe { p.add(s.len() + 1) };
    }
    v
}

pub fn init() {
    let cl = unsafe { cstr(k::GetCommandLineA()) }.to_vec();
    let args = split_args(&cl);
    let wcl = unsafe { wstr(k::GetCommandLineW()) }.to_vec();
    let wargs = split_args(&wcl);
    let env = env_strings();
    let wenv: Vec<Vec<u16>> = env.iter().map(|e| crate::widen(e)).collect();
    let mut pgm = [0u8; 260];
    let n = unsafe { k::GetModuleFileNameA(0, pgm.as_mut_ptr(), 260) } as usize;
    unsafe {
        __argc = args.len() as i32;
        __argv = c_array(&args);
        __wargv = c_array(&wargs);
        _environ = c_array(&env);
        __initenv = _environ;
        _wenviron = c_array(&wenv);
        __winitenv = _wenviron;
        _acmdln = crate::c_alloc_str(&cl);
        _wcmdln = *c_array(&[wcl]);
        _pgmptr = crate::c_alloc_str(&pgm[..n]);
        _wpgmptr = *c_array(&[crate::widen(&pgm[..n])]);
        __lc_codepage = k::GetACP();
    }
}

/// _startupinfo: newmode.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __getmainargs(argc: *mut i32, argv: *mut *mut *mut u8, env: *mut *mut *mut u8, _glob: i32, si: *mut i32) -> i32 {
    unsafe {
        *argc = __argc;
        *argv = __argv;
        *env = _environ;
        if !si.is_null() {
            crate::mem::_set_new_mode(*si);
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wgetmainargs(argc: *mut i32, argv: *mut *mut *mut u16, env: *mut *mut *mut u16, _glob: i32, _si: *mut i32) -> i32 {
    unsafe {
        *argc = __argc;
        *argv = __wargv;
        *env = _wenviron;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _configure_narrow_argv(_mode: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _configure_wide_argv(_mode: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _initialize_narrow_environment() -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _initialize_wide_environment() -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _get_initial_narrow_environment() -> *mut *mut u8 {
    unsafe { __initenv }
}

#[unsafe(no_mangle)]
pub extern "C" fn _get_initial_wide_environment() -> *mut *mut u16 {
    unsafe { __winitenv }
}

#[unsafe(no_mangle)]
pub extern "C" fn __set_app_type(t: i32) {
    unsafe { __app_type = t };
}

#[unsafe(no_mangle)]
pub extern "C" fn _set_app_type(t: i32) {
    unsafe { __app_type = t };
}

#[unsafe(no_mangle)]
pub extern "C" fn __setusermatherr(_f: u32) {}

#[unsafe(no_mangle)]
pub extern "C" fn _set_invalid_parameter_handler(_f: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _set_thread_local_invalid_parameter_handler(_f: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _set_purecall_handler(_f: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _set_abort_behavior(_flags: u32, _mask: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _set_error_mode(_m: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn __crtSetUnhandledExceptionFilter(_f: u32) -> u32 {
    0
}

/// _initterm(begin, end): each function pointer that is not NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _initterm(begin: *const u32, end: *const u32) {
    let mut p = begin;
    while p < end {
        let f = unsafe { *p };
        if f != 0 {
            call(f, 0, 0, 0, 0);
        }
        p = unsafe { p.add(1) };
    }
}

/// _initterm_e: the first that returns non-zero ends it with that.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _initterm_e(begin: *const u32, end: *const u32) -> i32 {
    let mut p = begin;
    while p < end {
        let f = unsafe { *p };
        if f != 0 {
            let r = call(f, 0, 0, 0, 0) as i32;
            if r != 0 {
                return r;
            }
        }
        p = unsafe { p.add(1) };
    }
    0
}

// ---- atexit and exit ----

static ATEXIT: Global<Vec<u32>> = Global::new(Vec::new());

#[unsafe(no_mangle)]
pub extern "C" fn atexit(f: u32) -> i32 {
    ATEXIT.with(|v| v.push(f));
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _onexit(f: u32) -> u32 {
    ATEXIT.with(|v| v.push(f));
    f
}

#[unsafe(no_mangle)]
pub extern "C" fn _crt_atexit(f: u32) -> i32 {
    atexit(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn _crt_at_quick_exit(_f: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn at_quick_exit(_f: u32) -> i32 {
    0
}

/// __dllonexit(f, &begin, &end): a DLL's own atexit table (MSVCRT DLLs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __dllonexit(f: u32, pbegin: *mut *mut u32, pend: *mut *mut u32) -> u32 {
    unsafe {
        let begin = *pbegin;
        let end = *pend;
        let n = if begin.is_null() { 0 } else { end.offset_from(begin) as usize };
        let new = crate::mem::realloc(begin as *mut u8, (n + 1) * 4) as *mut u32;
        if new.is_null() {
            return 0;
        }
        *new.add(n) = f;
        *pbegin = new;
        *pend = new.add(n + 1);
    }
    f
}

/// _onexit_table_t: first, last, end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _initialize_onexit_table(t: *mut *mut u32) -> i32 {
    unsafe {
        *t = null_mut();
        *t.add(1) = null_mut();
        *t.add(2) = null_mut();
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _register_onexit_function(t: *mut *mut u32, f: u32) -> i32 {
    unsafe {
        let (mut b, mut e) = (*t, *t.add(1));
        __dllonexit(f, &mut b, &mut e);
        *t = b;
        *t.add(1) = e;
        *t.add(2) = e;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _execute_onexit_table(t: *mut *mut u32) -> i32 {
    unsafe {
        let (b, e) = (*t, *t.add(1));
        if !b.is_null() {
            let mut p = e;
            while p > b {
                p = p.sub(1);
                if *p != 0 {
                    call(*p, 0, 0, 0, 0);
                }
            }
            crate::mem::free(b as *mut u8);
        }
        _initialize_onexit_table(t);
    }
    0
}

fn run_atexit() {
    loop {
        let f = ATEXIT.with(|v| v.pop());
        match f {
            Some(f) => {
                call(f, 0, 0, 0, 0);
            }
            None => break,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn exit(code: i32) -> ! {
    run_atexit();
    crate::stdio::flush_all();
    unsafe { k::ExitProcess(code as u32) }
}

#[unsafe(no_mangle)]
pub extern "C" fn _exit(code: i32) -> ! {
    crate::stdio::flush_all();
    unsafe { k::ExitProcess(code as u32) }
}

#[unsafe(no_mangle)]
pub extern "C" fn _Exit(code: i32) -> ! {
    _exit(code)
}

#[unsafe(no_mangle)]
pub extern "C" fn quick_exit(code: i32) -> ! {
    _exit(code)
}

#[unsafe(no_mangle)]
pub extern "C" fn _cexit() {
    run_atexit();
    crate::stdio::flush_all();
}

#[unsafe(no_mangle)]
pub extern "C" fn _c_exit() {
    crate::stdio::flush_all();
}

fn err_write(s: &[u8]) {
    crate::io::write_fd(2, s);
}

/// The kernel's log gets the callers (EBP chain) of whoever ends the
/// program abnormally: return addresses to look up in its map.
fn log_callers(what: &str) {
    let mut ebp: u32;
    unsafe { core::arch::asm!("mov {}, ebp", out(reg) ebp, options(nomem, nostack)) };
    let mut s = alloc::format!("w32crt: {what}, callers");
    for _ in 0..12 {
        if ebp == 0 || ebp & 3 != 0 {
            break;
        }
        let (next, ret) = unsafe { (*(ebp as *const u32), *((ebp + 4) as *const u32)) };
        s.push_str(&alloc::format!(" {ret:08x}"));
        if next <= ebp {
            break;
        }
        ebp = next;
    }
    let z = z(s.as_bytes());
    unsafe { k::OutputDebugStringA(z.as_ptr()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn abort() -> ! {
    log_callers("abort");
    if raise(SIGABRT) == 0 && handler(SIGABRT) != SIG_DFL {
        // A handler that returned: end anyway.
    }
    err_write(b"\r\nabnormal program termination\r\n");
    _exit(3)
}

#[unsafe(no_mangle)]
pub extern "C" fn _amsg_exit(n: i32) -> ! {
    let m = alloc::format!("\r\nruntime error R60{n:02}\r\n");
    err_write(m.as_bytes());
    _exit(255)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _assert(msg: *const u8, file: *const u8, line: u32) -> ! {
    let m = alloc::format!(
        "Assertion failed: {}, file {}, line {line}\r\n",
        alloc::string::String::from_utf8_lossy(unsafe { cstr(msg) }),
        alloc::string::String::from_utf8_lossy(unsafe { cstr(file) })
    );
    err_write(m.as_bytes());
    abort()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wassert(msg: *const u16, file: *const u16, line: u32) -> ! {
    let m = z(&crate::narrow(unsafe { wstr(msg) }));
    let f = z(&crate::narrow(unsafe { wstr(file) }));
    unsafe { _assert(m.as_ptr(), f.as_ptr(), line) }
}

#[unsafe(no_mangle)]
pub extern "C" fn _purecall() -> i32 {
    err_write(b"\r\npure virtual function call\r\n");
    _exit(255)
}

// ---- Signals ----

pub const SIGINT: i32 = 2;
pub const SIGILL: i32 = 4;
pub const SIGFPE: i32 = 8;
pub const SIGSEGV: i32 = 11;
pub const SIGTERM: i32 = 15;
pub const SIGBREAK: i32 = 21;
pub const SIGABRT: i32 = 22;
const SIG_DFL: u32 = 0;
const SIG_IGN: u32 = 1;
const SIG_ERR: u32 = 0xffff_ffff;

static SIGNALS: Global<[u32; 23]> = Global::new([0; 23]);

fn handler(sig: i32) -> u32 {
    if (0..23).contains(&sig) { SIGNALS.with(|s| s[sig as usize]) } else { SIG_DFL }
}

#[unsafe(no_mangle)]
pub extern "C" fn signal(sig: i32, f: u32) -> u32 {
    if !matches!(sig, SIGINT | SIGILL | SIGFPE | SIGSEGV | SIGTERM | SIGBREAK | SIGABRT | 6) {
        crate::set_errno(crate::EINVAL);
        return SIG_ERR;
    }
    let sig = if sig == 6 { SIGABRT } else { sig };
    SIGNALS.with(|s| core::mem::replace(&mut s[sig as usize], f))
}

#[unsafe(no_mangle)]
pub extern "C" fn raise(sig: i32) -> i32 {
    if !(0..23).contains(&sig) {
        return -1;
    }
    let f = handler(sig);
    match f {
        SIG_IGN => 0,
        SIG_DFL => {
            if sig == SIGABRT {
                return 0;
            }
            _exit(3)
        }
        f => {
            // The handler is reset to the default before it runs.
            SIGNALS.with(|s| s[sig as usize] = SIG_DFL);
            call(f, sig as u32, 0, 0, 0);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __pxcptinfoptrs() -> *mut u32 {
    static mut P: u32 = 0;
    unsafe { &raw mut P }
}

// ---- Locales: the "C" locale ----

#[repr(C)]
pub struct Lconv {
    decimal_point: *const u8,
    thousands_sep: *const u8,
    grouping: *const u8,
    int_curr_symbol: *const u8,
    currency_symbol: *const u8,
    mon_decimal_point: *const u8,
    mon_thousands_sep: *const u8,
    mon_grouping: *const u8,
    positive_sign: *const u8,
    negative_sign: *const u8,
    chars: [u8; 8],
    wide: [*const u16; 10],
}

static mut LCONV: Lconv = Lconv {
    decimal_point: c".".as_ptr() as *const u8,
    thousands_sep: c"".as_ptr() as *const u8,
    grouping: c"".as_ptr() as *const u8,
    int_curr_symbol: c"".as_ptr() as *const u8,
    currency_symbol: c"".as_ptr() as *const u8,
    mon_decimal_point: c"".as_ptr() as *const u8,
    mon_thousands_sep: c"".as_ptr() as *const u8,
    mon_grouping: c"".as_ptr() as *const u8,
    positive_sign: c"".as_ptr() as *const u8,
    negative_sign: c"".as_ptr() as *const u8,
    chars: [127; 8],
    wide: [[b'.' as u16, 0].as_ptr(); 10],
};

#[unsafe(no_mangle)]
pub extern "C" fn localeconv() -> *mut Lconv {
    unsafe { &raw mut LCONV }
}

/// setlocale: only "C" (and "" or NULL, which answer "C").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setlocale(_cat: i32, locale: *const u8) -> *const u8 {
    let l = unsafe { cstr(locale) };
    if locale.is_null() || l.is_empty() || l == b"C" || l == b"POSIX" {
        c"C".as_ptr() as *const u8
    } else {
        core::ptr::null()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wsetlocale(_cat: i32, locale: *const u16) -> *const u16 {
    static C: [u16; 2] = [b'C' as u16, 0];
    let l = unsafe { wstr(locale) };
    if locale.is_null() || l.is_empty() || l == [b'C' as u16] { C.as_ptr() } else { core::ptr::null() }
}

#[unsafe(no_mangle)]
pub extern "C" fn _configthreadlocale(_t: i32) -> i32 {
    2 // _DISABLE_PER_THREAD_LOCALE
}

#[unsafe(no_mangle)]
pub extern "C" fn _create_locale(_cat: i32, _l: *const u8) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn _free_locale(_l: u32) {}

#[unsafe(no_mangle)]
pub extern "C" fn _get_current_locale() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn __lconv_init() -> i32 {
    0
}
