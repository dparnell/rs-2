//! Child processes: `posix_spawn(p)` over DosExecPgm (EXEC_ASYNCRESULT),
//! `waitpid` over DosWaitChild, `kill` over DosKillProcess. There is no
//! `fork`; `execvp` runs the program and exits with its result, as OS/2 C
//! runtimes do.
//!
//! The child inherits the parent's handles, so file actions (std's
//! redirections of the standard handles to pipes or NUL) are applied to
//! the parent around the DosExecPgm call, under a lock, and undone after.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void};

use crate::errno::{fail, from_os2};
use crate::spin::SpinLock;
use crate::string::cstr;
use crate::sys::*;

enum Action {
    Dup2(c_int, c_int),
    Close(c_int),
    Open(c_int, Vec<u8>, c_int, u32),
    Chdir(Vec<u8>),
}

const MAGIC: usize = 0x5253_3246;

/// posix_spawn_file_actions_t holds a pointer to our list.
unsafe fn actions(fa: *mut libc::posix_spawn_file_actions_t) -> &'static mut Vec<Action> {
    unsafe {
        let w = fa as *mut usize;
        if *w != MAGIC {
            *w = MAGIC;
            *w.add(1) = Box::into_raw(Box::new(Vec::<Action>::new())) as usize;
        }
        &mut *(*w.add(1) as *mut Vec<Action>)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn_file_actions_init(fa: *mut libc::posix_spawn_file_actions_t) -> c_int {
    unsafe {
        core::ptr::write_bytes(fa, 0, 1);
        actions(fa);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn_file_actions_destroy(fa: *mut libc::posix_spawn_file_actions_t) -> c_int {
    unsafe {
        let w = fa as *mut usize;
        if *w == MAGIC {
            drop(Box::from_raw(*w.add(1) as *mut Vec<Action>));
            *w = 0;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn_file_actions_adddup2(fa: *mut libc::posix_spawn_file_actions_t, fd: c_int, new: c_int) -> c_int {
    unsafe { actions(fa).push(Action::Dup2(fd, new)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn_file_actions_addclose(fa: *mut libc::posix_spawn_file_actions_t, fd: c_int) -> c_int {
    unsafe { actions(fa).push(Action::Close(fd)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn_file_actions_addopen(
    fa: *mut libc::posix_spawn_file_actions_t,
    fd: c_int,
    path: *const c_char,
    flags: c_int,
    mode: u32,
) -> c_int {
    unsafe { actions(fa).push(Action::Open(fd, cstr(path).to_vec(), flags, mode)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn_file_actions_addchdir_np(fa: *mut libc::posix_spawn_file_actions_t, path: *const c_char) -> c_int {
    unsafe { actions(fa).push(Action::Chdir(cstr(path).to_vec())) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn_file_actions_addchdir(fa: *mut libc::posix_spawn_file_actions_t, path: *const c_char) -> c_int {
    unsafe { posix_spawn_file_actions_addchdir_np(fa, path) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnattr_init(_a: *mut libc::posix_spawnattr_t) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnattr_destroy(_a: *mut libc::posix_spawnattr_t) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnattr_setflags(_a: *mut libc::posix_spawnattr_t, _f: i16) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnattr_setpgroup(_a: *mut libc::posix_spawnattr_t, _g: c_int) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnattr_setsigdefault(_a: *mut libc::posix_spawnattr_t, _s: *const libc::sigset_t) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnattr_setsigmask(_a: *mut libc::posix_spawnattr_t, _s: *const libc::sigset_t) -> c_int {
    0
}

fn exists(p: &[u8]) -> bool {
    let mut st = FileStatus3::default();
    unsafe { DosQueryPathInfo(p.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) == 0 && st.attr & 0x10 == 0 }
}

/// The program file for `name` (with NUL): as given, with ".EXE" added,
/// and (`search`) along PATH when it has no directory part.
fn find_program(name: &[u8], search: bool) -> Option<Vec<u8>> {
    let base = crate::file::os2_path(name);
    let base = &base[..base.len() - 1];
    let has_ext = base.rsplit(|&c| c == b'\\').next().is_some_and(|f| f.contains(&b'.'));
    let candidates = |p: &[u8]| -> Option<Vec<u8>> {
        let mut a = p.to_vec();
        a.push(0);
        if has_ext && exists(&a) {
            return Some(a);
        }
        for ext in [&b".EXE"[..], b".COM", b".CMD"] {
            let mut b = p.to_vec();
            b.extend_from_slice(ext);
            b.push(0);
            if exists(&b) {
                return Some(b);
            }
        }
        if exists(&a) { Some(a) } else { None }
    };
    if let Some(p) = candidates(base) {
        return Some(p);
    }
    if !search || base.contains(&b'\\') || base.contains(&b':') {
        return None;
    }
    let path = unsafe { crate::misc::getenv(c"PATH".as_ptr()) };
    if path.is_null() {
        return None;
    }
    for dir in unsafe { cstr(path) }.split(|&c| c == b';').filter(|d| !d.is_empty()) {
        let mut p = dir.to_vec();
        if !p.ends_with(b"\\") {
            p.push(b'\\');
        }
        p.extend_from_slice(base);
        if let Some(f) = candidates(&p) {
            return Some(f);
        }
    }
    None
}

fn quote(out: &mut Vec<u8>, a: &[u8]) {
    if !a.is_empty() && !a.iter().any(|&c| c == b' ' || c == b'\t' || c == b'"') {
        out.extend_from_slice(a);
        return;
    }
    out.push(b'"');
    for &c in a {
        if c == b'"' {
            out.push(b'\\');
        }
        out.push(c);
    }
    out.push(b'"');
}

static SPAWN: SpinLock<()> = SpinLock::new(());

unsafe fn spawn(
    pid: *mut c_int,
    file: *const c_char,
    fa: *const libc::posix_spawn_file_actions_t,
    argv: *const *const c_char,
    envp: *const *const c_char,
    search: bool,
) -> c_int {
    let name = unsafe { cstr(file) };
    let Some(prog) = find_program(name, search) else { return libc::ENOENT };
    // Arguments: "program\0arg1 arg2...\0\0".
    let mut args = Vec::new();
    unsafe {
        let a0 = if (*argv).is_null() { name } else { cstr(*argv) };
        args.extend_from_slice(a0);
        args.push(0);
        let mut i = 1;
        while !(*argv.add(i)).is_null() {
            if i > 1 {
                args.push(b' ');
            }
            quote(&mut args, cstr(*argv.add(i)));
            i += 1;
        }
    }
    args.extend_from_slice(b"\0\0");
    let mut env = Vec::new();
    if !envp.is_null() {
        unsafe {
            let mut i = 0;
            while !(*envp.add(i)).is_null() {
                env.extend_from_slice(cstr(*envp.add(i)));
                env.push(0);
                i += 1;
            }
        }
    }
    env.extend_from_slice(b"\0\0");

    let _g = SPAWN.lock();
    // Apply the file actions to this process, remembering how to undo them.
    let mut saved: Vec<(c_int, Option<u32>)> = Vec::new();
    let mut old_cwd = None;
    let save = |fd: c_int, saved: &mut Vec<(c_int, Option<u32>)>| {
        if saved.iter().any(|s| s.0 == fd) {
            return;
        }
        let mut copy = 0xffff_ffff;
        let ok = unsafe { DosDupHandle(fd as u32, &mut copy) } == 0;
        if ok {
            unsafe { crate::file::fcntl(copy as c_int, libc::F_SETFD, 1) };
        }
        saved.push((fd, ok.then_some(copy)));
    };
    let mut err = 0;
    if !fa.is_null() && unsafe { *(fa as *const usize) } == MAGIC {
        for a in unsafe { actions(fa as *mut _) }.iter() {
            match a {
                Action::Dup2(from, to) => {
                    save(*to, &mut saved);
                    let mut t = *to as u32;
                    unsafe { DosDupHandle(*from as u32, &mut t) };
                    // The child must inherit the copy.
                    unsafe { crate::file::fcntl(*to, libc::F_SETFD, 0) };
                }
                Action::Close(fd) => {
                    save(*fd, &mut saved);
                    unsafe { DosClose(*fd as u32) };
                }
                Action::Open(fd, path, flags, mode) => {
                    save(*fd, &mut saved);
                    let p = crate::file::os2_path(path);
                    let h = unsafe { crate::file::open64(p.as_ptr() as *const c_char, *flags & !libc::O_CLOEXEC, *mode) };
                    if h < 0 {
                        err = crate::errno::errno();
                        break;
                    }
                    if h != *fd {
                        let mut t = *fd as u32;
                        unsafe {
                            DosDupHandle(h as u32, &mut t);
                            DosClose(h as u32);
                        }
                    }
                }
                Action::Chdir(dir) => {
                    if old_cwd.is_none() {
                        let mut c = crate::file::current_dir();
                        c.push(0);
                        old_cwd = Some(c);
                    }
                    let p = crate::file::os2_path(dir);
                    if unsafe { crate::file::chdir(p.as_ptr() as *const c_char) } != 0 {
                        err = crate::errno::errno();
                        break;
                    }
                }
            }
        }
    }
    let mut res = [0u32; 2];
    let mut fail_name = [0u8; 260];
    if err == 0 {
        let rc = unsafe {
            DosExecPgm(fail_name.as_mut_ptr(), 260, 2, args.as_ptr(), if envp.is_null() { core::ptr::null() } else { env.as_ptr() }, &mut res, prog.as_ptr())
        };
        if rc != 0 {
            err = from_os2(rc);
        }
    }
    // Undo.
    for (fd, copy) in saved.iter().rev() {
        unsafe {
            match copy {
                Some(c) => {
                    let mut t = *fd as u32;
                    DosDupHandle(*c, &mut t);
                    DosClose(*c);
                }
                None => {
                    DosClose(*fd as u32);
                }
            }
        }
    }
    if let Some(c) = old_cwd {
        unsafe { crate::file::chdir(c.as_ptr() as *const c_char) };
    }
    if err != 0 {
        return err;
    }
    if !pid.is_null() {
        unsafe { *pid = res[0] as c_int };
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn(
    pid: *mut c_int,
    path: *const c_char,
    fa: *const libc::posix_spawn_file_actions_t,
    _attr: *const libc::posix_spawnattr_t,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    unsafe { spawn(pid, path, fa, argv, envp, false) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnp(
    pid: *mut c_int,
    file: *const c_char,
    fa: *const libc::posix_spawn_file_actions_t,
    _attr: *const libc::posix_spawnattr_t,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    unsafe { spawn(pid, file, fa, argv, envp, true) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int {
    let mut res = [0u32; 2];
    let mut got = 0;
    // DCWA_PROCESS, DCWW_WAIT or DCWW_NOWAIT.
    let wait = if options & libc::WNOHANG != 0 { 1 } else { 0 };
    let rc = unsafe { DosWaitChild(0, wait, &mut res, &mut got, if pid < 0 { 0 } else { pid as u32 }) };
    if rc == 129 && wait == 1 {
        // ERROR_CHILD_NOT_COMPLETE
        return 0;
    }
    if rc != 0 {
        return fail(if rc == 128 { libc::ECHILD } else { from_os2(rc) });
    }
    if !status.is_null() {
        // Normal end: exit status in bits 8-15; otherwise "killed" (9).
        unsafe { *status = if res[0] == 0 { ((res[1] & 0xff) << 8) as c_int } else { 9 } };
    }
    got as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wait(status: *mut c_int) -> c_int {
    unsafe { waitpid(-1, status, 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn waitid(_idtype: c_int, _id: u32, _info: *mut c_void, _options: c_int) -> c_int {
    fail(libc::ENOSYS)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kill(pid: c_int, sig: c_int) -> c_int {
    if sig == 0 {
        return 0;
    }
    // DKP_PROCESS
    let rc = unsafe { DosKillProcess(1, pid as u32) };
    if rc != 0 { fail(libc::ESRCH) } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fork() -> c_int {
    fail(libc::ENOSYS)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int {
    let mut pid = 0;
    let env = unsafe { crate::misc::environ as *const *const c_char };
    let rc = unsafe { spawn(&mut pid, file, core::ptr::null(), argv, env, true) };
    if rc != 0 {
        return fail(rc);
    }
    let mut st = 0;
    unsafe {
        waitpid(pid, &mut st, 0);
        crate::start::_exit((st >> 8) & 0xff)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execv(file: *const c_char, argv: *const *const c_char) -> c_int {
    unsafe { execvp(file, argv) }
}
