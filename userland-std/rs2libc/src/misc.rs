//! The rest: environment, process ids, `syscall`, signals (accepted and
//! ignored: OS/2 reports faults as exceptions), memory mappings
//! (anonymous ones are DosAllocMem objects), random numbers, and the
//! dynamic-linker queries std makes (there is no dynamic linker: answers
//! are "not found").

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_long, c_ulong, c_void};
use core::ptr::null_mut;

use libc::size_t;

use crate::errno::{fail, fail_os2};
use crate::spin::SpinLock;
use crate::string::cstr;
use crate::sys::*;

// ---- Environment ----

/// glibc's `environ`: NULL-terminated array of "NAME=value" strings.
#[unsafe(no_mangle)]
pub static mut environ: *mut *mut c_char = null_mut();

static ENV_LOCK: SpinLock<()> = SpinLock::new(());

/// Our own copy of the array (strings stay in the OS/2 environment block or
/// are leaked on change: other threads may still hold pointers).
static mut ENV: Vec<*mut c_char> = Vec::new();

pub unsafe fn init_environ(block: *const u8) {
    unsafe {
        let env = &mut *(&raw mut ENV);
        let mut p = block;
        while !p.is_null() && *p != 0 {
            env.push(p as *mut c_char);
            p = p.add(crate::string::strlen(p as *const c_char) + 1);
        }
        env.push(null_mut());
        environ = env.as_mut_ptr();
    }
}

fn name_matches(entry: *const c_char, name: &[u8]) -> bool {
    let e = unsafe { cstr(entry) };
    // OS/2 environment names are case-insensitive.
    e.len() > name.len() && e[name.len()] == b'=' && e[..name.len()].eq_ignore_ascii_case(name)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getenv(name: *const c_char) -> *mut c_char {
    let n = unsafe { cstr(name) };
    let _g = ENV_LOCK.lock();
    unsafe {
        let mut p = environ;
        while !p.is_null() && !(*p).is_null() {
            if name_matches(*p, n) {
                return (*p).add(n.len() + 1);
            }
            p = p.add(1);
        }
    }
    null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn secure_getenv(name: *const c_char) -> *mut c_char {
    unsafe { getenv(name) }
}

unsafe fn set(name: &[u8], value: Option<&[u8]>) {
    let _g = ENV_LOCK.lock();
    unsafe {
        let env = &mut *(&raw mut ENV);
        env.pop(); // the NULL
        env.retain(|&e| !name_matches(e, name));
        if let Some(v) = value {
            let mut s = Vec::with_capacity(name.len() + v.len() + 2);
            s.extend_from_slice(name);
            s.push(b'=');
            s.extend_from_slice(v);
            s.push(0);
            env.push(Vec::leak(s).as_mut_ptr() as *mut c_char);
        }
        env.push(null_mut());
        environ = env.as_mut_ptr();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setenv(name: *const c_char, value: *const c_char, overwrite: c_int) -> c_int {
    let n = unsafe { cstr(name) };
    if n.is_empty() || n.contains(&b'=') {
        return fail(libc::EINVAL);
    }
    if overwrite == 0 && unsafe { !getenv(name).is_null() } {
        return 0;
    }
    unsafe { set(n, Some(cstr(value))) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn unsetenv(name: *const c_char) -> c_int {
    let n = unsafe { cstr(name) };
    if n.is_empty() || n.contains(&b'=') {
        return fail(libc::EINVAL);
    }
    unsafe { set(n, None) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn putenv(s: *mut c_char) -> c_int {
    let b = unsafe { cstr(s) };
    let Some(eq) = b.iter().position(|&c| c == b'=') else { return unsafe { unsetenv(s) } };
    unsafe { set(&b[..eq], Some(&b[eq + 1..])) };
    0
}

// ---- Process ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpid() -> c_int {
    info_blocks().1.pid as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getppid() -> c_int {
    info_blocks().1.ppid as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getuid() -> u32 {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geteuid() -> u32 {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getgid() -> u32 {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getegid() -> u32 {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setuid(_u: u32) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setgid(_g: u32) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setgroups(_n: size_t, _g: *const u32) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setsid() -> c_int {
    fail(libc::EPERM)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setpgid(_p: c_int, _g: c_int) -> c_int {
    fail(libc::EPERM)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn chroot(_p: *const c_char) -> c_int {
    fail(libc::EPERM)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn umask(_m: u32) -> u32 {
    0o022
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysconf(name: c_int) -> c_long {
    match name {
        libc::_SC_PAGESIZE => 4096,
        libc::_SC_NPROCESSORS_ONLN | libc::_SC_NPROCESSORS_CONF => {
            // QSV_NUMPROCESSORS
            let mut n = 0u32;
            if unsafe { DosQuerySysInfo(26, 26, &mut n, 4) } != 0 || n == 0 {
                n = 1;
            }
            n as c_long
        }
        libc::_SC_CLK_TCK => 100,
        libc::_SC_OPEN_MAX => 4096,
        libc::_SC_GETPW_R_SIZE_MAX | libc::_SC_GETGR_R_SIZE_MAX => 1024,
        libc::_SC_NGROUPS_MAX => 0,
        libc::_SC_IOV_MAX => 1024,
        libc::_SC_HOST_NAME_MAX => 64,
        _ => fail(libc::EINVAL) as c_long,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpagesize() -> c_int {
    4096
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gethostname(buf: *mut c_char, len: size_t) -> c_int {
    let name = b"rs2\0";
    let host = unsafe { getenv(c"HOSTNAME".as_ptr()) };
    let src = if host.is_null() { &name[..] } else { unsafe { core::slice::from_raw_parts(host as *const u8, cstr(host).len() + 1) } };
    if src.len() > len {
        return fail(libc::ENAMETOOLONG);
    }
    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), buf as *mut u8, src.len()) };
    0
}

/// `syscall(number, ...)` for the Linux system calls std makes directly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn syscall(nr: c_long, a1: usize, a2: usize, a3: usize, a4: usize, _a5: usize, _a6: usize) -> c_long {
    match nr {
        // futex, futex_time64
        240 => unsafe { crate::futex::sys_futex(a1, a2 as i32, a3 as u32, a4) as c_long },
        422 => {
            // 64-bit timespec: convert to the 32-bit one sys_futex reads.
            let ts = if a4 == 0 {
                0
            } else {
                let t = unsafe { &*(a4 as *const [u32; 4]) };
                let ts32 = libc::timespec { tv_sec: t[0] as _, tv_nsec: t[2] as _ };
                return unsafe { crate::futex::sys_futex(a1, a2 as i32, a3 as u32, &ts32 as *const _ as usize) as c_long };
            };
            unsafe { crate::futex::sys_futex(a1, a2 as i32, a3 as u32, ts) as c_long }
        }
        // getrandom
        355 => unsafe { getrandom(a1 as *mut c_void, a2, a3 as u32) as c_long },
        // gettid
        224 => unsafe { crate::thread::gettid() as c_long },
        _ => fail(libc::ENOSYS) as c_long,
    }
}

// ---- Signals ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigaction(sig: c_int, _act: *const libc::sigaction, old: *mut libc::sigaction) -> c_int {
    if !old.is_null() {
        unsafe {
            core::ptr::write_bytes(old, 0, 1);
            // Faults are OS/2 exceptions: say SIGSEGV/SIGBUS are handled
            // already so that std does not set up alternate signal stacks.
            if sig == libc::SIGSEGV || sig == libc::SIGBUS {
                (*old).sa_sigaction = libc::SIG_IGN;
            }
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn signal(_sig: c_int, _h: libc::sighandler_t) -> libc::sighandler_t {
    libc::SIG_DFL
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigaltstack(_ss: *const libc::stack_t, old: *mut libc::stack_t) -> c_int {
    if !old.is_null() {
        unsafe {
            core::ptr::write_bytes(old, 0, 1);
            (*old).ss_flags = libc::SS_DISABLE;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigemptyset(set: *mut libc::sigset_t) -> c_int {
    unsafe { core::ptr::write_bytes(set, 0, 1) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigfillset(set: *mut libc::sigset_t) -> c_int {
    unsafe { core::ptr::write_bytes(set, 0xff, 1) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigaddset(_set: *mut libc::sigset_t, _sig: c_int) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigdelset(_set: *mut libc::sigset_t, _sig: c_int) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigprocmask(_how: c_int, _set: *const libc::sigset_t, old: *mut libc::sigset_t) -> c_int {
    if !old.is_null() {
        unsafe { core::ptr::write_bytes(old, 0, 1) };
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_sigmask(how: c_int, set: *const libc::sigset_t, old: *mut libc::sigset_t) -> c_int {
    unsafe { sigprocmask(how, set, old) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn raise(sig: c_int) -> c_int {
    unsafe { crate::start::_exit(128 + sig) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pause() -> c_int {
    loop {
        unsafe { DosSleep(u32::MAX) };
    }
}

// ---- Memory mappings ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmap64(addr: *mut c_void, len: size_t, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void {
    let _ = (addr, prot);
    if len == 0 {
        fail(libc::EINVAL);
        return libc::MAP_FAILED;
    }
    let mut p = null_mut();
    let rc = unsafe { DosAllocMem(&mut p, len as u32, PAG_READ | PAG_WRITE | PAG_COMMIT) };
    if rc != 0 {
        fail_os2(rc);
        return libc::MAP_FAILED;
    }
    if flags & libc::MAP_ANONYMOUS == 0 {
        // A private copy of the file's contents.
        unsafe {
            if crate::file::lseek64(fd, off, 0) < 0 {
                DosFreeMem(p);
                return libc::MAP_FAILED;
            }
            let mut done = 0;
            while done < len {
                let n = crate::file::read(fd, p.add(done) as *mut c_void, len - done);
                if n <= 0 {
                    break;
                }
                done += n as usize;
            }
        }
    }
    p as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmap(addr: *mut c_void, len: size_t, prot: c_int, flags: c_int, fd: c_int, off: libc::off_t) -> *mut c_void {
    unsafe { mmap64(addr, len, prot, flags, fd, off as i64) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn munmap(addr: *mut c_void, _len: size_t) -> c_int {
    // Only whole mappings can be released (DosFreeMem takes the base).
    unsafe { DosFreeMem(addr as *mut u8) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mprotect(_addr: *mut c_void, _len: size_t, _prot: c_int) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn madvise(_addr: *mut c_void, _len: size_t, _advice: c_int) -> c_int {
    0
}

// ---- The dynamic linker that is not there ----

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlsym(_handle: *mut c_void, _name: *const c_char) -> *mut c_void {
    null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dl_iterate_phdr(_cb: *mut c_void, _data: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getauxval(_ty: c_ulong) -> c_ulong {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gnu_get_libc_version() -> *const c_char {
    c"2.35".as_ptr()
}

/// Never called: the linker turns every TLS access into the local-exec
/// form (tls.rs). Present so that the reference resolves.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ___tls_get_addr() -> *mut c_void {
    crate::start::fatal("rs2libc: ___tls_get_addr called\r\n");
}

// ---- Random numbers ----

/// ChaCha20 keyed from what entropy the machine offers (RDRAND when the
/// CPU has it, the high-resolution timer and time-stamp counter sampled
/// around sleeps, the date, ids).
struct Rng {
    key: [u32; 8],
    counter: u64,
    seeded: bool,
}

static RNG: SpinLock<Rng> = SpinLock::new(Rng { key: [0; 8], counter: 0, seeded: false });

fn rdtsc() -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack)) };
    (hi as u64) << 32 | lo as u64
}

fn rdrand() -> Option<u32> {
    let ecx = core::arch::x86::__cpuid(1).ecx;
    if ecx & (1 << 30) == 0 {
        return None;
    }
    for _ in 0..10 {
        let (v, ok): (u32, u8);
        unsafe { core::arch::asm!("rdrand {0:e}", "setc {1}", out(reg) v, out(reg_byte) ok, options(nomem, nostack)) };
        if ok != 0 {
            return Some(v);
        }
    }
    None
}

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

fn chacha_block(key: &[u32; 8], counter: u64, nonce: u32) -> [u32; 16] {
    let mut s = [0u32; 16];
    s[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    s[4..12].copy_from_slice(key);
    s[12] = counter as u32;
    s[13] = (counter >> 32) as u32;
    s[14] = nonce;
    s[15] = 0x7273_3221;
    let init = s;
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    for i in 0..16 {
        s[i] = s[i].wrapping_add(init[i]);
    }
    s
}

fn seed(r: &mut Rng) {
    let mut pool = [0u32; 16];
    let mut i = 0;
    let mut mix = |v: u32, pool: &mut [u32; 16]| {
        pool[i % 16] = pool[i % 16].rotate_left(5) ^ v;
        i += 1;
    };
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    mix(u32::from_le_bytes([dt.hours, dt.minutes, dt.seconds, dt.hundredths]), &mut pool);
    mix(u32::from_le_bytes([dt.day, dt.month, dt.year as u8, (dt.year >> 8) as u8]), &mut pool);
    let (tib, pib) = info_blocks();
    mix(pib.pid, &mut pool);
    mix(unsafe { (*tib.ptib2).tid }, &mut pool);
    mix(&pool as *const _ as u32, &mut pool);
    for k in 0..64 {
        let mut t = 0u64;
        unsafe { DosTmrQueryTime(&mut t) };
        let tsc = rdtsc();
        mix(t as u32 ^ (tsc as u32).rotate_left(k), &mut pool);
        mix((tsc >> 32) as u32 ^ (t >> 32) as u32, &mut pool);
        if let Some(v) = rdrand() {
            mix(v, &mut pool);
        }
        if k % 16 == 0 {
            unsafe { DosSleep(0) };
        }
    }
    // Condense through the cipher itself.
    let key: [u32; 8] = core::array::from_fn(|j| pool[j] ^ pool[j + 8]);
    let out = chacha_block(&key, 0, 0x5eed);
    r.key.copy_from_slice(&out[..8]);
    r.counter = 1;
    r.seeded = true;
}

pub fn random_bytes(buf: &mut [u8]) {
    let mut r = RNG.lock();
    if !r.seeded || r.counter > 1 << 20 {
        seed(&mut r);
    }
    for chunk in buf.chunks_mut(64) {
        let block = chacha_block(&r.key, r.counter, 0);
        r.counter += 1;
        let bytes: [u8; 64] = unsafe { core::mem::transmute(block) };
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    // Fast key erasure: the next key comes from the stream.
    let next = chacha_block(&r.key, r.counter, 1);
    r.counter += 1;
    r.key.copy_from_slice(&next[..8]);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getrandom(buf: *mut c_void, len: size_t, _flags: u32) -> isize {
    random_bytes(unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, len) });
    len as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getentropy(buf: *mut c_void, len: size_t) -> c_int {
    if len > 256 {
        return fail(libc::EIO);
    }
    unsafe { getrandom(buf, len, 0) };
    0
}
