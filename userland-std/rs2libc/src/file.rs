//! File descriptors, files and directories.
//!
//! Descriptors below [`SOCK_BASE`] are OS/2 file handles (0-2 are the
//! standard handles, inherited by child processes as usual); sockets are
//! [`SOCK_BASE`] + the rs2 socket handle; [`DIR_BASE`] and up are
//! directories opened with `O_DIRECTORY` (std's `remove_dir_all` works
//! with `openat`/`unlinkat` relative to them), which remember their path.
//!
//! Paths may use `/` or `\`; `/dev/null` is `NUL`. Writes to character
//! devices (the console) turn `\n` into `\r\n` and console reads turn
//! `\r\n` into `\n`, as OS/2 C runtimes do in text mode.

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void};
use core::sync::atomic::{AtomicU8, Ordering};

use libc::{mode_t, off64_t, size_t, ssize_t};
type stat64 = libc::stat64;

use crate::errno::{fail, fail_os2};
use crate::spin::SpinLock;
use crate::string::cstr;
use crate::sys::*;

pub const SOCK_BASE: c_int = 0x1_0000;
pub const DIR_BASE: c_int = 0x2_0000;

const MAX_HANDLES: usize = 4096;
/// Per-handle flags: checked, console, append, nonblocking...
static FLAGS: [AtomicU8; MAX_HANDLES] = [const { AtomicU8::new(0) }; MAX_HANDLES];
const F_CHECKED: u8 = 1;
const F_CHARDEV: u8 = 2;
const F_APPEND: u8 = 4;
const F_PIPE: u8 = 8;

static DIRS: SpinLock<Vec<Option<Vec<u8>>>> = SpinLock::new(Vec::new());

pub enum Fd {
    File(u32),
    Socket(u32),
    Dir(usize),
}

pub fn classify(fd: c_int) -> Option<Fd> {
    if fd < 0 {
        None
    } else if fd < SOCK_BASE {
        Some(Fd::File(fd as u32))
    } else if fd < crate::net::ALIAS_BASE {
        Some(Fd::Socket((fd - SOCK_BASE) as u32))
    } else if fd < DIR_BASE {
        crate::net::alias(fd).map(Fd::Socket)
    } else {
        Some(Fd::Dir((fd - DIR_BASE) as usize))
    }
}

fn flags(h: u32) -> u8 {
    let Some(f) = FLAGS.get(h as usize) else { return F_CHECKED };
    let v = f.load(Ordering::Relaxed);
    if v & F_CHECKED != 0 {
        return v;
    }
    let (mut ty, mut attr) = (0, 0);
    let mut v = F_CHECKED;
    if unsafe { DosQueryHType(h, &mut ty, &mut attr) } == 0 {
        match ty & 0xff {
            1 => v |= F_CHARDEV,
            2 => v |= F_PIPE,
            _ => {}
        }
    }
    f.fetch_or(v, Ordering::Relaxed);
    f.load(Ordering::Relaxed)
}

fn set_flag(h: u32, bit: u8, on: bool) {
    if let Some(f) = FLAGS.get(h as usize) {
        if on {
            f.fetch_or(bit, Ordering::Relaxed);
        } else {
            f.fetch_and(!bit, Ordering::Relaxed);
        }
    }
}

fn forget(h: u32) {
    if let Some(f) = FLAGS.get(h as usize) {
        f.store(0, Ordering::Relaxed);
    }
}

/// A NUL-terminated OS/2 path for a C path.
pub fn os2_path(p: &[u8]) -> Vec<u8> {
    if p == b"/dev/null" {
        return b"NUL\0".to_vec();
    }
    if p == b"/dev/tty" || p == b"/dev/console" {
        return b"CON\0".to_vec();
    }
    let mut v: Vec<u8> = p.iter().map(|&c| if c == b'/' { b'\\' } else { c }).collect();
    // "C:" alone means that drive's current directory; "dir\" is "dir".
    while v.len() > 1 && v.ends_with(b"\\") && !(v.len() == 3 && v[1] == b':') && !(v.len() == 1) {
        v.pop();
    }
    v.push(0);
    v
}

pub unsafe fn c_path(p: *const c_char) -> Vec<u8> {
    os2_path(unsafe { cstr(p) })
}

fn is_absolute(p: &[u8]) -> bool {
    p.first().is_some_and(|&c| c == b'/' || c == b'\\') || (p.len() >= 2 && p[1] == b':')
}

/// The path of `name` relative to descriptor `dirfd` (AT_FDCWD or a
/// directory descriptor), NUL-terminated.
unsafe fn at_path(dirfd: c_int, name: *const c_char) -> Option<Vec<u8>> {
    let n = unsafe { cstr(name) };
    if dirfd == libc::AT_FDCWD || is_absolute(n) {
        return Some(os2_path(n));
    }
    let Some(Fd::Dir(i)) = classify(dirfd) else { return None };
    let dirs = DIRS.lock();
    let mut p = dirs.get(i)?.clone()?;
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(n);
    Some(os2_path(&p))
}

fn new_dir_fd(path: &[u8]) -> c_int {
    let mut d = DIRS.lock();
    let p = path.strip_suffix(b"\0").unwrap_or(path).to_vec();
    if let Some(i) = d.iter().position(|e| e.is_none()) {
        d[i] = Some(p);
        return DIR_BASE + i as c_int;
    }
    d.push(Some(p));
    DIR_BASE + d.len() as c_int - 1
}

fn dir_path(i: usize) -> Option<Vec<u8>> {
    DIRS.lock().get(i).cloned().flatten()
}

fn query_path(p: &[u8]) -> Result<FileStatus3, u32> {
    let mut st = FileStatus3::default();
    let rc = unsafe { DosQueryPathInfo(p.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) };
    if rc == 0 {
        return Ok(st);
    }
    // A drive's root directory has no directory entry of its own.
    let root = p.len() == 4 && p[1] == b':' && p[2] == b'\\' || p == b"\\\0" || p == b"/\0";
    if root {
        st.attr = 0x10;
        return Ok(st);
    }
    Err(rc)
}

fn is_dir(p: &[u8]) -> bool {
    query_path(p).is_ok_and(|s| s.attr & 0x10 != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open64(path: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    let p = unsafe { c_path(path) };
    open_os2(&p, flags, mode)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open(path: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    unsafe { open64(path, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat64(dirfd: c_int, path: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    let Some(p) = (unsafe { at_path(dirfd, path) }) else { return fail(libc::EBADF) };
    open_os2(&p, flags, mode)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat(dirfd: c_int, path: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    unsafe { openat64(dirfd, path, flags, mode) }
}

fn open_os2(p: &[u8], flags: c_int, mode: mode_t) -> c_int {
    let acc = flags & libc::O_ACCMODE;
    if flags & libc::O_DIRECTORY != 0 || (acc == libc::O_RDONLY && flags & libc::O_CREAT == 0 && is_dir(p)) {
        return match query_path(p) {
            Ok(s) if s.attr & 0x10 != 0 => new_dir_fd(p),
            Ok(_) => fail(libc::ENOTDIR),
            Err(rc) => fail_os2(rc),
        };
    }
    let mut open_mode = match acc {
        libc::O_WRONLY => 1,
        libc::O_RDWR => 2,
        _ => 0,
    } | 0x40; // OPEN_SHARE_DENYNONE
    if flags & libc::O_CLOEXEC != 0 {
        open_mode |= 0x80; // OPEN_FLAGS_NOINHERIT
    }
    let create = flags & libc::O_CREAT != 0;
    let open_flags = match (create, flags & libc::O_EXCL != 0, flags & libc::O_TRUNC != 0) {
        (true, true, _) => 0x10,
        (true, false, true) => 0x12,
        (true, false, false) => 0x11,
        (false, _, true) => 0x02,
        (false, _, false) => 0x01,
    };
    let attr = if create && mode & 0o222 == 0 { 1 } else { 0 };
    let (mut h, mut action) = (0, 0);
    let rc = unsafe { DosOpen(p.as_ptr(), &mut h, &mut action, 0, attr, open_flags, open_mode, core::ptr::null_mut()) };
    if rc != 0 {
        if rc == 5 && is_dir(p) {
            return fail(libc::EISDIR);
        }
        return fail_os2(rc);
    }
    forget(h);
    if flags & libc::O_APPEND != 0 {
        set_flag(h, F_APPEND, true);
    }
    h as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn creat(path: *const c_char, mode: mode_t) -> c_int {
    unsafe { open64(path, libc::O_CREAT | libc::O_WRONLY | libc::O_TRUNC, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn close(fd: c_int) -> c_int {
    match classify(fd) {
        Some(Fd::File(h)) => {
            forget(h);
            let rc = unsafe { DosClose(h) };
            if rc != 0 { fail_os2(rc) } else { 0 }
        }
        Some(Fd::Socket(s)) => crate::net::close_socket(fd, s),
        Some(Fd::Dir(i)) => {
            let mut d = DIRS.lock();
            match d.get_mut(i) {
                Some(e @ Some(_)) => {
                    *e = None;
                    0
                }
                _ => fail(libc::EBADF),
            }
        }
        None => fail(libc::EBADF),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn read(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t {
    match classify(fd) {
        Some(Fd::File(h)) => {
            let mut got = 0;
            let rc = unsafe { DosRead(h, buf as *mut u8, n as u32, &mut got) };
            if rc == 109 {
                return 0; // broken pipe: end of file
            }
            if rc != 0 {
                return fail_os2(rc) as ssize_t;
            }
            if flags(h) & F_CHARDEV != 0 && got > 0 {
                // Console input: "\r\n" ends a line as "\n".
                let b = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, got as usize) };
                let mut w = 0;
                for r in 0..b.len() {
                    if b[r] == b'\r' && r + 1 < b.len() && b[r + 1] == b'\n' {
                        continue;
                    }
                    b[w] = b[r];
                    w += 1;
                }
                // Ctrl+Z at the start of a console line is end of file.
                if w > 0 && b[0] == 0x1a {
                    return 0;
                }
                return w as ssize_t;
            }
            got as ssize_t
        }
        Some(Fd::Socket(s)) => unsafe { crate::net::recv(SOCK_BASE + s as c_int, buf, n, 0) },
        Some(Fd::Dir(_)) => fail(libc::EISDIR) as ssize_t,
        None => fail(libc::EBADF) as ssize_t,
    }
}

fn write_handle(h: u32, b: &[u8]) -> ssize_t {
    let f = flags(h);
    if f & F_APPEND != 0 {
        let mut pos = 0;
        unsafe { DosSetFilePtr(h, 0, 2, &mut pos) };
    }
    if f & F_CHARDEV != 0 && b.contains(&b'\n') {
        let mut out = Vec::with_capacity(b.len() + 16);
        for &c in b {
            if c == b'\n' {
                out.push(b'\r');
            }
            out.push(c);
        }
        let mut n = 0;
        let rc = unsafe { DosWrite(h, out.as_ptr(), out.len() as u32, &mut n) };
        if rc != 0 {
            return fail_os2(rc) as ssize_t;
        }
        return b.len() as ssize_t;
    }
    let mut n = 0;
    let rc = unsafe { DosWrite(h, b.as_ptr(), b.len() as u32, &mut n) };
    if rc != 0 {
        return fail_os2(rc) as ssize_t;
    }
    if n == 0 && !b.is_empty() && f & F_CHARDEV == 0 && f & F_PIPE == 0 {
        return fail(libc::ENOSPC) as ssize_t;
    }
    n as ssize_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn write(fd: c_int, buf: *const c_void, n: size_t) -> ssize_t {
    match classify(fd) {
        Some(Fd::File(h)) => write_handle(h, unsafe { core::slice::from_raw_parts(buf as *const u8, n) }),
        Some(Fd::Socket(_)) => unsafe { crate::net::send(fd, buf, n, 0) },
        Some(Fd::Dir(_)) => fail(libc::EISDIR) as ssize_t,
        None => fail(libc::EBADF) as ssize_t,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn writev(fd: c_int, iov: *const libc::iovec, cnt: c_int) -> ssize_t {
    let mut total = 0;
    for i in 0..cnt.max(0) as usize {
        let v = unsafe { &*iov.add(i) };
        if v.iov_len == 0 {
            continue;
        }
        let n = unsafe { write(fd, v.iov_base, v.iov_len) };
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
        if (n as usize) < v.iov_len {
            break;
        }
    }
    total
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readv(fd: c_int, iov: *const libc::iovec, cnt: c_int) -> ssize_t {
    // One read, into the first non-empty buffer (allowed: reads may be short).
    for i in 0..cnt.max(0) as usize {
        let v = unsafe { &*iov.add(i) };
        if v.iov_len != 0 {
            return unsafe { read(fd, v.iov_base, v.iov_len) };
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pread64(fd: c_int, buf: *mut c_void, n: size_t, off: off64_t) -> ssize_t {
    let Some(Fd::File(h)) = classify(fd) else { return fail(libc::ESPIPE) as ssize_t };
    let mut old = 0;
    unsafe {
        DosSetFilePtr(h, 0, 1, &mut old);
        let mut p = 0;
        DosSetFilePtr(h, off as i32, 0, &mut p);
        let r = read(fd, buf, n);
        DosSetFilePtr(h, old as i32, 0, &mut p);
        r
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pwrite64(fd: c_int, buf: *const c_void, n: size_t, off: off64_t) -> ssize_t {
    let Some(Fd::File(h)) = classify(fd) else { return fail(libc::ESPIPE) as ssize_t };
    let mut old = 0;
    unsafe {
        DosSetFilePtr(h, 0, 1, &mut old);
        let mut p = 0;
        DosSetFilePtr(h, off as i32, 0, &mut p);
        let r = write(fd, buf, n);
        DosSetFilePtr(h, old as i32, 0, &mut p);
        r
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lseek64(fd: c_int, off: off64_t, whence: c_int) -> off64_t {
    let Some(Fd::File(h)) = classify(fd) else { return fail(libc::ESPIPE) as off64_t };
    if flags(h) & (F_CHARDEV | F_PIPE) != 0 {
        return fail(libc::ESPIPE) as off64_t;
    }
    if off > i32::MAX as i64 || off < i32::MIN as i64 {
        return fail(libc::EOVERFLOW) as off64_t;
    }
    let mut pos = 0;
    let rc = unsafe { DosSetFilePtr(h, off as i32, whence as u32, &mut pos) };
    if rc != 0 {
        return fail_os2(rc) as off64_t;
    }
    pos as off64_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lseek(fd: c_int, off: libc::off_t, whence: c_int) -> libc::off_t {
    unsafe { lseek64(fd, off as off64_t, whence) as libc::off_t }
}

/// Seconds since 1970 of an FDATE/FTIME pair (local time).
fn file_time(date: u16, time: u16) -> i64 {
    if date == 0 {
        return 0;
    }
    let (y, m, d) = (1980 + (date >> 9) as i64, ((date >> 5) & 15) as u32, (date & 31) as u32);
    let (h, mi, s) = ((time >> 11) as i64, ((time >> 5) & 63) as i64, ((time & 31) * 2) as i64);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m.max(1) as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d.max(1) as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + h * 3600 + mi * 60 + s
}

fn hash_path(p: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &c in p {
        h = (h ^ c.to_ascii_uppercase() as u64).wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn fill_stat(buf: *mut stat64, s: &FileStatus3, path: &[u8]) {
    unsafe {
        core::ptr::write_bytes(buf, 0, 1);
        let b = &mut *buf;
        let dir = s.attr & 0x10 != 0;
        let ro = s.attr & 1 != 0;
        b.st_mode = if dir { libc::S_IFDIR | 0o755 } else { libc::S_IFREG | if ro { 0o444 } else { 0o644 } };
        b.st_nlink = 1;
        b.st_size = s.cb_file as i64;
        b.st_blksize = 4096;
        b.st_blocks = (s.cb_alloc as i64 + 511) / 512;
        b.st_mtime = file_time(s.wdate, s.wtime) as _;
        b.st_atime = if s.adate != 0 { file_time(s.adate, s.atime) as _ } else { b.st_mtime };
        b.st_ctime = if s.cdate != 0 { file_time(s.cdate, s.ctime) as _ } else { b.st_mtime };
        b.st_ino = hash_path(path);
        b.st_dev = path.first().map_or(0, |c| c.to_ascii_uppercase() as u64);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn stat64(path: *const c_char, buf: *mut stat64) -> c_int {
    let p = unsafe { c_path(path) };
    stat_os2(&p, buf)
}

fn stat_os2(p: &[u8], buf: *mut stat64) -> c_int {
    if p == b"NUL\0" || p == b"CON\0" {
        unsafe {
            core::ptr::write_bytes(buf, 0, 1);
            (*buf).st_mode = libc::S_IFCHR | 0o666;
        }
        return 0;
    }
    match query_path(p) {
        Ok(s) => {
            fill_stat(buf, &s, &full_path(p));
            0
        }
        Err(rc) => fail_os2(rc),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lstat64(path: *const c_char, buf: *mut stat64) -> c_int {
    unsafe { stat64(path, buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fstatat64(dirfd: c_int, path: *const c_char, buf: *mut stat64, flags: c_int) -> c_int {
    if flags & libc::AT_EMPTY_PATH != 0 && unsafe { *path } == 0 {
        return unsafe { fstat64(dirfd, buf) };
    }
    let Some(p) = (unsafe { at_path(dirfd, path) }) else { return fail(libc::EBADF) };
    stat_os2(&p, buf)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fstat64(fd: c_int, buf: *mut stat64) -> c_int {
    match classify(fd) {
        Some(Fd::File(h)) => {
            let f = flags(h);
            if f & (F_CHARDEV | F_PIPE) != 0 {
                unsafe {
                    core::ptr::write_bytes(buf, 0, 1);
                    (*buf).st_mode = if f & F_PIPE != 0 { libc::S_IFIFO | 0o600 } else { libc::S_IFCHR | 0o620 };
                    (*buf).st_nlink = 1;
                    (*buf).st_blksize = 4096;
                }
                return 0;
            }
            let mut s = FileStatus3::default();
            let rc = unsafe { DosQueryFileInfo(h, 1, &mut s as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) };
            if rc != 0 {
                return fail_os2(rc);
            }
            let mut id = [0u8; 4];
            id.copy_from_slice(&h.to_le_bytes());
            fill_stat(buf, &s, &id);
            0
        }
        Some(Fd::Socket(_)) => {
            unsafe {
                core::ptr::write_bytes(buf, 0, 1);
                (*buf).st_mode = libc::S_IFSOCK | 0o777;
            }
            0
        }
        Some(Fd::Dir(i)) => match dir_path(i) {
            Some(mut p) => {
                p.push(0);
                stat_os2(&p, buf)
            }
            None => fail(libc::EBADF),
        },
        None => fail(libc::EBADF),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn access(path: *const c_char, _mode: c_int) -> c_int {
    let p = unsafe { c_path(path) };
    match query_path(&p) {
        Ok(_) => 0,
        Err(rc) => fail_os2(rc),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mkdir(path: *const c_char, _mode: mode_t) -> c_int {
    let p = unsafe { c_path(path) };
    let rc = unsafe { DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
    if rc == 0 {
        return 0;
    }
    if (rc == 5 || rc == 80 || rc == 183) && query_path(&p).is_ok() {
        return fail(libc::EEXIST);
    }
    fail_os2(rc)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mkdirat(dirfd: c_int, path: *const c_char, _mode: mode_t) -> c_int {
    let Some(p) = (unsafe { at_path(dirfd, path) }) else { return fail(libc::EBADF) };
    let rc = unsafe { DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
    if rc != 0 { fail_os2(rc) } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmdir(path: *const c_char) -> c_int {
    let p = unsafe { c_path(path) };
    rmdir_os2(&p)
}

fn rmdir_os2(p: &[u8]) -> c_int {
    let rc = unsafe { DosDeleteDir(p.as_ptr()) };
    if rc == 0 {
        return 0;
    }
    if rc == 5 {
        // OS/2 says "access denied" for a directory that is not empty.
        let mut spec = p[..p.len() - 1].to_vec();
        spec.extend_from_slice(b"\\*\0");
        let mut h = 0xffff_ffff;
        let mut buf = [0u8; 300];
        let mut count = 3;
        let r = unsafe { DosFindFirst(spec.as_ptr(), &mut h, 0x37, buf.as_mut_ptr(), 300, &mut count, 1) };
        if r == 0 {
            unsafe { DosFindClose(h) };
            if count > 2 {
                return fail(libc::ENOTEMPTY);
            }
        }
    }
    fail_os2(rc)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn unlink(path: *const c_char) -> c_int {
    let p = unsafe { c_path(path) };
    unlink_os2(&p)
}

fn unlink_os2(p: &[u8]) -> c_int {
    let rc = unsafe { DosDelete(p.as_ptr()) };
    if rc == 0 {
        return 0;
    }
    if rc == 5 && is_dir(p) {
        return fail(libc::EISDIR);
    }
    fail_os2(rc)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remove(path: *const c_char) -> c_int {
    let p = unsafe { c_path(path) };
    if is_dir(&p) { rmdir_os2(&p) } else { unlink_os2(&p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int {
    let Some(p) = (unsafe { at_path(dirfd, path) }) else { return fail(libc::EBADF) };
    if flags & libc::AT_REMOVEDIR != 0 { rmdir_os2(&p) } else { unlink_os2(&p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rename(from: *const c_char, to: *const c_char) -> c_int {
    let (f, t) = unsafe { (c_path(from), c_path(to)) };
    let mut rc = unsafe { DosMove(f.as_ptr(), t.as_ptr()) };
    if rc != 0 && query_path(&t).is_ok() && !is_dir(&t) {
        // POSIX rename replaces an existing file.
        unsafe { DosDelete(t.as_ptr()) };
        rc = unsafe { DosMove(f.as_ptr(), t.as_ptr()) };
    }
    if rc != 0 { fail_os2(rc) } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn renameat(fd1: c_int, from: *const c_char, fd2: c_int, to: *const c_char) -> c_int {
    let (Some(f), Some(t)) = (unsafe { at_path(fd1, from) }, unsafe { at_path(fd2, to) }) else {
        return fail(libc::EBADF);
    };
    let rc = unsafe { DosMove(f.as_ptr(), t.as_ptr()) };
    if rc != 0 { fail_os2(rc) } else { 0 }
}

/// The current directory, "C:\dir".
pub fn current_dir() -> Vec<u8> {
    let (mut disk, mut map) = (0, 0);
    unsafe { DosQueryCurrentDisk(&mut disk, &mut map) };
    let mut buf = [0u8; 260];
    let mut cb = buf.len() as u32;
    unsafe { DosQueryCurrentDir(0, buf.as_mut_ptr(), &mut cb) };
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    let mut v = alloc::vec![b'A' + disk as u8 - 1, b':', b'\\'];
    v.extend_from_slice(&buf[..n]);
    v
}

/// An absolute, normalised path (no NUL) for an OS/2 path (with NUL).
pub fn full_path(p: &[u8]) -> Vec<u8> {
    let p = p.strip_suffix(b"\0").unwrap_or(p);
    let cwd = current_dir();
    let (drive, rest): (Vec<u8>, &[u8]) = if p.len() >= 2 && p[1] == b':' {
        (p[..2].to_vec(), &p[2..])
    } else {
        (cwd[..2].to_vec(), p)
    };
    let mut parts: Vec<&[u8]> = Vec::new();
    let base: Vec<u8>;
    if !rest.first().is_some_and(|&c| c == b'\\') {
        // Relative: to the current directory of that drive.
        base = if drive.eq_ignore_ascii_case(&cwd[..2]) {
            cwd[3..].to_vec()
        } else {
            let mut buf = [0u8; 260];
            let mut cb = buf.len() as u32;
            unsafe { DosQueryCurrentDir((drive[0].to_ascii_uppercase() - b'A' + 1) as u32, buf.as_mut_ptr(), &mut cb) };
            let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
            buf[..n].to_vec()
        };
        parts.extend(base.split(|&c| c == b'\\').filter(|s| !s.is_empty()));
    }
    for s in rest.split(|&c| c == b'\\') {
        match s {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let mut out = drive;
    out[0] = out[0].to_ascii_uppercase();
    out.push(b'\\');
    for (i, s) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'\\');
        }
        out.extend_from_slice(s);
    }
    out
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getcwd(buf: *mut c_char, size: size_t) -> *mut c_char {
    let mut d = current_dir();
    d.push(0);
    unsafe {
        if buf.is_null() {
            let p = crate::malloc::malloc(d.len().max(size)) as *mut c_char;
            if !p.is_null() {
                core::ptr::copy_nonoverlapping(d.as_ptr(), p as *mut u8, d.len());
            }
            return p;
        }
        if d.len() > size {
            fail(libc::ERANGE);
            return core::ptr::null_mut();
        }
        core::ptr::copy_nonoverlapping(d.as_ptr(), buf as *mut u8, d.len());
    }
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chdir(path: *const c_char) -> c_int {
    let p = unsafe { c_path(path) };
    if p.len() >= 3 && p[1] == b':' {
        let rc = unsafe { DosSetDefaultDisk((p[0].to_ascii_uppercase() - b'A' + 1) as u32) };
        if rc != 0 {
            return fail_os2(rc);
        }
        if p.len() == 3 {
            return 0;
        }
    }
    let rc = unsafe { DosSetCurrentDir(p.as_ptr()) };
    if rc != 0 { fail_os2(rc) } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fchdir(fd: c_int) -> c_int {
    let Some(Fd::Dir(i)) = classify(fd) else { return fail(libc::ENOTDIR) };
    let Some(mut p) = dir_path(i) else { return fail(libc::EBADF) };
    p.push(0);
    unsafe { chdir(p.as_ptr() as *const c_char) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn realpath(path: *const c_char, resolved: *mut c_char) -> *mut c_char {
    let p = unsafe { c_path(path) };
    if let Err(rc) = query_path(&p) {
        fail_os2(rc);
        return core::ptr::null_mut();
    }
    let mut full = if p == b"NUL\0" { b"NUL".to_vec() } else { full_path(&p) };
    full.push(0);
    unsafe {
        let out = if resolved.is_null() { crate::malloc::malloc(full.len()) as *mut c_char } else { resolved };
        if !out.is_null() {
            core::ptr::copy_nonoverlapping(full.as_ptr(), out as *mut u8, full.len().min(4096));
        }
        out
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readlink(path: *const c_char, buf: *mut c_char, size: size_t) -> ssize_t {
    let p = unsafe { cstr(path) };
    if p == b"/proc/self/exe" {
        let exe = crate::start::program_path();
        let n = exe.len().min(size);
        unsafe { core::ptr::copy_nonoverlapping(exe.as_ptr(), buf as *mut u8, n) };
        return n as ssize_t;
    }
    // OS/2 file systems have no symbolic links.
    let p = os2_path(p);
    match query_path(&p) {
        Ok(_) => fail(libc::EINVAL) as ssize_t,
        Err(rc) => fail_os2(rc) as ssize_t,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn symlink(_a: *const c_char, _b: *const c_char) -> c_int {
    fail(libc::EPERM)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn link(_a: *const c_char, _b: *const c_char) -> c_int {
    fail(libc::EPERM)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chmod(path: *const c_char, mode: mode_t) -> c_int {
    let p = unsafe { c_path(path) };
    let Ok(mut s) = query_path(&p) else { return fail(libc::ENOENT) };
    if s.attr & 0x10 != 0 {
        return 0;
    }
    s.attr = if mode & 0o222 == 0 { s.attr | 1 } else { s.attr & !1 };
    let rc = unsafe { DosSetPathInfo(p.as_ptr(), 1, &s as *const _ as *const u8, core::mem::size_of::<FileStatus3>() as u32, 0) };
    if rc != 0 { fail_os2(rc) } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fchmod(_fd: c_int, _mode: mode_t) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fsync(fd: c_int) -> c_int {
    if classify(fd).is_none() { fail(libc::EBADF) } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fdatasync(fd: c_int) -> c_int {
    unsafe { fsync(fd) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ftruncate64(fd: c_int, len: off64_t) -> c_int {
    let Some(Fd::File(h)) = classify(fd) else { return fail(libc::EINVAL) };
    let rc = unsafe { DosSetFileSize(h, len as u32) };
    if rc != 0 { fail_os2(rc) } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ftruncate(fd: c_int, len: libc::off_t) -> c_int {
    unsafe { ftruncate64(fd, len as off64_t) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dup(fd: c_int) -> c_int {
    match classify(fd) {
        Some(Fd::File(h)) => {
            let mut new = 0xffff_ffff;
            let rc = unsafe { DosDupHandle(h, &mut new) };
            if rc != 0 {
                return fail_os2(rc);
            }
            forget(new);
            if flags(h) & F_APPEND != 0 {
                set_flag(new, F_APPEND, true);
            }
            new as c_int
        }
        Some(Fd::Socket(s)) => crate::net::dup_socket(s),
        Some(Fd::Dir(i)) => match dir_path(i) {
            Some(p) => new_dir_fd(&p),
            None => fail(libc::EBADF),
        },
        None => fail(libc::EBADF),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dup2(old: c_int, new: c_int) -> c_int {
    let (Some(Fd::File(h)), Some(Fd::File(n))) = (classify(old), classify(new)) else { return fail(libc::EBADF) };
    let mut n2 = n;
    let rc = unsafe { DosDupHandle(h, &mut n2) };
    if rc != 0 {
        return fail_os2(rc);
    }
    forget(n2);
    n2 as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dup3(old: c_int, new: c_int, _flags: c_int) -> c_int {
    unsafe { dup2(old, new) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fcntl(fd: c_int, cmd: c_int, arg: usize) -> c_int {
    match (classify(fd), cmd) {
        (None, _) => fail(libc::EBADF),
        (Some(_), libc::F_DUPFD | libc::F_DUPFD_CLOEXEC) => unsafe { dup(fd) },
        (Some(Fd::File(h)), libc::F_GETFD) => {
            let mut st = 0;
            let rc = unsafe { DosQueryFHState(h, &mut st) };
            if rc != 0 {
                return fail_os2(rc);
            }
            (st & 0x80 != 0) as c_int
        }
        (Some(Fd::File(h)), libc::F_SETFD) => {
            let mut st = 0;
            let rc = unsafe { DosQueryFHState(h, &mut st) };
            if rc != 0 {
                return fail_os2(rc);
            }
            // DosSetFHState takes only the inheritance, write-through and
            // fail-on-error bits.
            let st = st & 0x6000 | if arg & 1 != 0 { 0x80 } else { 0 };
            unsafe { DosSetFHState(h, st) };
            0
        }
        (Some(Fd::File(h)), libc::F_GETFL) => {
            let mut st = 0;
            unsafe { DosQueryFHState(h, &mut st) };
            (st & 3) as c_int | if flags(h) & F_APPEND != 0 { libc::O_APPEND } else { 0 }
        }
        (Some(Fd::File(h)), libc::F_SETFL) => {
            set_flag(h, F_APPEND, arg as c_int & libc::O_APPEND != 0);
            0
        }
        (Some(Fd::Socket(s)), libc::F_GETFL) => libc::O_RDWR | if crate::net::is_nonblocking(s) { libc::O_NONBLOCK } else { 0 },
        (Some(Fd::Socket(s)), libc::F_SETFL) => crate::net::set_nonblocking(s, arg as c_int & libc::O_NONBLOCK != 0),
        (Some(_), libc::F_GETFD | libc::F_SETFD) => 0,
        (Some(_), libc::F_GETFL) => libc::O_RDONLY,
        (Some(_), libc::F_SETFL) => 0,
        _ => fail(libc::EINVAL),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fcntl64(fd: c_int, cmd: c_int, arg: usize) -> c_int {
    unsafe { fcntl(fd, cmd, arg) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioctl(fd: c_int, req: core::ffi::c_ulong, arg: *mut c_void) -> c_int {
    match (classify(fd), req) {
        (None, _) => fail(libc::EBADF),
        (Some(Fd::Socket(s)), libc::FIONBIO) => crate::net::set_nonblocking(s, unsafe { *(arg as *const c_int) } != 0),
        (Some(Fd::Socket(s)), libc::FIONREAD) => crate::net::nread(s, arg as *mut c_int),
        // Anonymous pipes cannot be non-blocking: reads block (std's
        // Command::output then reads standard output to its end first).
        (Some(Fd::File(_)), libc::FIONBIO) => 0,
        (Some(_), libc::FIOCLEX) => unsafe { fcntl(fd, libc::F_SETFD, 1) },
        (Some(_), libc::FIONCLEX) => unsafe { fcntl(fd, libc::F_SETFD, 0) },
        (Some(Fd::File(h)), libc::TCGETS) if flags(h) & F_CHARDEV != 0 => 0,
        (Some(Fd::File(h)), libc::TIOCGWINSZ) if flags(h) & F_CHARDEV != 0 => {
            let ws = arg as *mut libc::winsize;
            unsafe {
                (*ws).ws_row = 25;
                (*ws).ws_col = 80;
            }
            0
        }
        _ => fail(libc::ENOTTY),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn isatty(fd: c_int) -> c_int {
    match classify(fd) {
        Some(Fd::File(h)) if flags(h) & F_CHARDEV != 0 => 1,
        _ => {
            fail(libc::ENOTTY);
            0
        }
    }
}

/// True if `fd` is an open OS/2 file handle (poll's POLLNVAL check).
pub fn handle_valid(h: u32) -> bool {
    let mut st = 0;
    unsafe { DosQueryFHState(h, &mut st) == 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pipe2(fds: *mut c_int, flags: c_int) -> c_int {
    let (mut r, mut w) = (0, 0);
    let rc = unsafe { DosCreatePipe(&mut r, &mut w, 65536) };
    if rc != 0 {
        return fail_os2(rc);
    }
    forget(r);
    forget(w);
    if flags & libc::O_CLOEXEC != 0 {
        unsafe {
            fcntl(r as c_int, libc::F_SETFD, 1);
            fcntl(w as c_int, libc::F_SETFD, 1);
        }
    }
    unsafe {
        *fds = r as c_int;
        *fds.add(1) = w as c_int;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pipe(fds: *mut c_int) -> c_int {
    unsafe { pipe2(fds, 0) }
}

// ---- Directories ----

#[repr(C)]
pub struct Dir {
    hdir: u32,
    fd: c_int,
    first: bool,
    done: bool,
    buf: FileFindBuf3,
    ent: libc::dirent64,
}

unsafe fn open_dir(path: &[u8], fd: c_int) -> *mut Dir {
    let p = path.strip_suffix(b"\0").unwrap_or(path);
    if let Err(rc) = query_path(path) {
        fail_os2(rc);
        return core::ptr::null_mut();
    }
    if !is_dir(path) {
        fail(libc::ENOTDIR);
        return core::ptr::null_mut();
    }
    let mut spec = p.to_vec();
    if !spec.ends_with(b"\\") {
        spec.push(b'\\');
    }
    spec.extend_from_slice(b"*\0");
    unsafe {
        let d = crate::malloc::malloc(core::mem::size_of::<Dir>()) as *mut Dir;
        if d.is_null() {
            return d;
        }
        core::ptr::write_bytes(d, 0, 1);
        (*d).hdir = 0xffff_ffff; // HDIR_CREATE
        (*d).fd = if fd >= 0 { fd } else { new_dir_fd(path) };
        let mut count = 1;
        let rc = DosFindFirst(spec.as_ptr(), &mut (*d).hdir, 0x37, &mut (*d).buf as *mut _ as *mut u8,
            core::mem::size_of::<FileFindBuf3>() as u32, &mut count, 1);
        if rc == 18 || (rc == 0 && count == 0) {
            (*d).done = true;
        } else if rc != 0 {
            close((*d).fd);
            crate::malloc::free(d as *mut c_void);
            fail_os2(rc);
            return core::ptr::null_mut();
        }
        (*d).first = true;
        d
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn opendir(path: *const c_char) -> *mut Dir {
    let p = unsafe { c_path(path) };
    unsafe { open_dir(&p, -1) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fdopendir(fd: c_int) -> *mut Dir {
    let Some(Fd::Dir(i)) = classify(fd) else {
        fail(libc::ENOTDIR);
        return core::ptr::null_mut();
    };
    let Some(mut p) = dir_path(i) else {
        fail(libc::EBADF);
        return core::ptr::null_mut();
    };
    p.push(0);
    unsafe { open_dir(&p, fd) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readdir64(d: *mut Dir) -> *mut libc::dirent64 {
    unsafe {
        let d = &mut *d;
        if d.done {
            return core::ptr::null_mut();
        }
        if !d.first {
            let mut count = 1;
            let rc = DosFindNext(d.hdir, &mut d.buf as *mut _ as *mut u8, core::mem::size_of::<FileFindBuf3>() as u32, &mut count);
            if rc != 0 || count == 0 {
                d.done = true;
                return core::ptr::null_mut();
            }
        }
        d.first = false;
        let n = (d.buf.cch_name as usize).min(255);
        let name = &d.buf.name[..n];
        let e = &mut d.ent;
        e.d_ino = hash_path(name) | 1;
        e.d_off = 0;
        e.d_reclen = core::mem::size_of::<libc::dirent64>() as u16;
        e.d_type = if d.buf.st.attr & 0x10 != 0 { libc::DT_DIR } else { libc::DT_REG };
        for (i, &c) in name.iter().enumerate() {
            e.d_name[i] = c as c_char;
        }
        e.d_name[n] = 0;
        e
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readdir(d: *mut Dir) -> *mut libc::dirent64 {
    unsafe { readdir64(d) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn closedir(d: *mut Dir) -> c_int {
    unsafe {
        if (*d).hdir != 0xffff_ffff {
            DosFindClose((*d).hdir);
        }
        close((*d).fd);
        crate::malloc::free(d as *mut c_void);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dirfd(d: *mut Dir) -> c_int {
    unsafe { (*d).fd }
}

// Zero-copy transfers std tries first (fs::copy, io::copy): not here, so
// std copies through a buffer.

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sendfile64(_out: c_int, _in: c_int, _off: *mut off64_t, _n: size_t) -> ssize_t {
    fail(libc::ENOSYS) as ssize_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn splice(_a: c_int, _ao: *mut off64_t, _b: c_int, _bo: *mut off64_t, _n: size_t, _f: u32) -> ssize_t {
    fail(libc::ENOSYS) as ssize_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn copy_file_range(_a: c_int, _ao: *mut off64_t, _b: c_int, _bo: *mut off64_t, _n: size_t, _f: u32) -> ssize_t {
    fail(libc::ENOSYS) as ssize_t
}
