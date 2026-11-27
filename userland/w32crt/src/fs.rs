//! Files and directories by name: remove, rename, _stat (all its sizes),
//! _findfirst, directories, paths.

use alloc::vec::Vec;

use crate::{cstr, k, set_dos_errno, set_errno, wstr, z, EINVAL, ENOENT};

fn last() -> u32 {
    unsafe { k::GetLastError() }
}

fn ok_or_errno(ok: bool) -> i32 {
    if ok {
        0
    } else {
        set_dos_errno(last());
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remove(p: *const u8) -> i32 {
    ok_or_errno(unsafe { k::DeleteFileA(p) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _unlink(p: *const u8) -> i32 {
    unsafe { remove(p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rename(from: *const u8, to: *const u8) -> i32 {
    ok_or_errno(unsafe { k::MoveFileA(from, to) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wremove(p: *const u16) -> i32 {
    let a = z(&crate::narrow(unsafe { wstr(p) }));
    unsafe { remove(a.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wunlink(p: *const u16) -> i32 {
    unsafe { _wremove(p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wrename(from: *const u16, to: *const u16) -> i32 {
    let (a, b) = (z(&crate::narrow(unsafe { wstr(from) })), z(&crate::narrow(unsafe { wstr(to) })));
    unsafe { rename(a.as_ptr(), b.as_ptr()) }
}

/// _access: 0 exists, 2 writable, 4 readable, 6 both.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _access(p: *const u8, mode: i32) -> i32 {
    let a = unsafe { k::GetFileAttributesA(p) };
    if a == 0xffff_ffff {
        set_dos_errno(last());
        return -1;
    }
    if mode & 2 != 0 && a & 1 != 0 && a & 0x10 == 0 {
        set_errno(crate::EACCES);
        return -1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _access_s(p: *const u8, mode: i32) -> i32 {
    if unsafe { _access(p, mode) } == 0 { 0 } else { crate::errno() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _waccess(p: *const u16, mode: i32) -> i32 {
    let a = z(&crate::narrow(unsafe { wstr(p) }));
    unsafe { _access(a.as_ptr(), mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _chmod(p: *const u8, mode: i32) -> i32 {
    let a = unsafe { k::GetFileAttributesA(p) };
    if a == 0xffff_ffff {
        set_dos_errno(last());
        return -1;
    }
    // _S_IWRITE (0x80): not read-only.
    let na = if mode & 0x80 != 0 { a & !1 } else { a | 1 };
    ok_or_errno(unsafe { k::SetFileAttributesA(p, na & !0x80) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _mkdir(p: *const u8) -> i32 {
    ok_or_errno(unsafe { k::CreateDirectoryA(p, 0) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _rmdir(p: *const u8) -> i32 {
    ok_or_errno(unsafe { k::RemoveDirectoryA(p) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _chdir(p: *const u8) -> i32 {
    ok_or_errno(unsafe { k::SetCurrentDirectoryA(p) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wmkdir(p: *const u16) -> i32 {
    let a = z(&crate::narrow(unsafe { wstr(p) }));
    unsafe { _mkdir(a.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wrmdir(p: *const u16) -> i32 {
    let a = z(&crate::narrow(unsafe { wstr(p) }));
    unsafe { _rmdir(a.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wchdir(p: *const u16) -> i32 {
    let a = z(&crate::narrow(unsafe { wstr(p) }));
    unsafe { _chdir(a.as_ptr()) }
}

fn cwd() -> Vec<u8> {
    let mut b = [0u8; 260];
    let n = unsafe { k::GetCurrentDirectoryA(260, b.as_mut_ptr()) } as usize;
    b[..n.min(259)].to_vec()
}

/// A string for a caller's buffer, or (NULL) a new one of at least `n`.
unsafe fn give(s: &[u8], buf: *mut u8, n: i32) -> *mut u8 {
    if buf.is_null() {
        let p = crate::mem::heap_alloc(s.len().max(n.max(0) as usize) + 1);
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            *p.add(s.len()) = 0;
        }
        return p;
    }
    if (n as usize) < s.len() + 1 {
        set_errno(crate::ERANGE);
        return core::ptr::null_mut();
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _getcwd(buf: *mut u8, n: i32) -> *mut u8 {
    unsafe { give(&cwd(), buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _getdcwd(drive: i32, buf: *mut u8, n: i32) -> *mut u8 {
    let mut p = alloc::vec![if drive == 0 { cwd()[0] } else { b'A' + drive as u8 - 1 }, b':', b'.'];
    p.push(0);
    let mut out = [0u8; 260];
    let len = unsafe { k::GetFullPathNameA(p.as_ptr(), 260, out.as_mut_ptr(), core::ptr::null_mut()) } as usize;
    unsafe { give(&out[..len.min(259)], buf, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wgetcwd(buf: *mut u16, n: i32) -> *mut u16 {
    let w = crate::widen(&cwd());
    if buf.is_null() {
        let p = crate::mem::heap_alloc((w.len().max(n.max(0) as usize) + 1) * 2) as *mut u16;
        unsafe {
            core::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            *p.add(w.len()) = 0;
        }
        return p;
    }
    if (n as usize) < w.len() + 1 {
        set_errno(crate::ERANGE);
        return core::ptr::null_mut();
    }
    unsafe {
        core::ptr::copy_nonoverlapping(w.as_ptr(), buf, w.len());
        *buf.add(w.len()) = 0;
    }
    buf
}

#[unsafe(no_mangle)]
pub extern "C" fn _getdrive() -> i32 {
    (cwd()[0].to_ascii_uppercase() - b'A' + 1) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn _chdrive(d: i32) -> i32 {
    let p = [b'A' + d as u8 - 1, b':', 0];
    ok_or_errno(unsafe { k::SetCurrentDirectoryA(p.as_ptr()) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _fullpath(buf: *mut u8, rel: *const u8, n: u32) -> *mut u8 {
    let mut out = [0u8; 260];
    let len = unsafe { k::GetFullPathNameA(rel, 260, out.as_mut_ptr(), core::ptr::null_mut()) } as usize;
    if len == 0 || len >= 260 {
        set_errno(EINVAL);
        return core::ptr::null_mut();
    }
    unsafe { give(&out[..len], buf, n as i32) }
}

unsafe fn put_part(dst: *mut u8, s: &[u8]) {
    if !dst.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), dst, s.len());
            *dst.add(s.len()) = 0;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _splitpath(path: *const u8, drive: *mut u8, dir: *mut u8, name: *mut u8, ext: *mut u8) {
    let p = unsafe { cstr(path) };
    let (d, rest) = if p.len() >= 2 && p[1] == b':' { (&p[..2], &p[2..]) } else { (&p[..0], p) };
    let split = rest.iter().rposition(|&c| c == b'\\' || c == b'/').map_or(0, |i| i + 1);
    let (dirp, file) = rest.split_at(split);
    let dot = file.iter().rposition(|&c| c == b'.').unwrap_or(file.len());
    let (nm, ex) = file.split_at(dot);
    unsafe {
        put_part(drive, d);
        put_part(dir, dirp);
        put_part(name, nm);
        put_part(ext, ex);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _makepath(out: *mut u8, drive: *const u8, dir: *const u8, name: *const u8, ext: *const u8) {
    let mut v = Vec::new();
    let d = unsafe { cstr(drive) };
    if !d.is_empty() {
        v.push(d[0]);
        v.push(b':');
    }
    let di = unsafe { cstr(dir) };
    v.extend_from_slice(di);
    if !di.is_empty() && !di.ends_with(b"\\") && !di.ends_with(b"/") {
        v.push(b'\\');
    }
    v.extend_from_slice(unsafe { cstr(name) });
    let e = unsafe { cstr(ext) };
    if !e.is_empty() && !e.starts_with(b".") {
        v.push(b'.');
    }
    v.extend_from_slice(e);
    unsafe { put_part(out, &v) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _searchenv(file: *const u8, var: *const u8, out: *mut u8) {
    let f = unsafe { cstr(file) };
    unsafe { *out = 0 };
    if unsafe { k::GetFileAttributesA(file) } != 0xffff_ffff {
        let mut b = [0u8; 260];
        let n = unsafe { k::GetFullPathNameA(file, 260, b.as_mut_ptr(), core::ptr::null_mut()) } as usize;
        unsafe { put_part(out, &b[..n.min(259)]) };
        return;
    }
    let Some(paths) = crate::misc::env_value(unsafe { cstr(var) }) else {
        set_errno(ENOENT);
        return;
    };
    for d in paths.split(|&c| c == b';').filter(|d| !d.is_empty()) {
        let mut p = d.to_vec();
        if !p.ends_with(b"\\") {
            p.push(b'\\');
        }
        p.extend_from_slice(f);
        let pz = z(&p);
        if unsafe { k::GetFileAttributesA(pz.as_ptr()) } != 0xffff_ffff {
            unsafe { put_part(out, &p) };
            return;
        }
    }
    set_errno(ENOENT);
}

// ---- _stat ----

/// What _stat says about a file.
pub struct Info {
    pub mode: u16,
    pub size: u64,
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
    pub drive: u32,
}

pub fn ft_to_time(ft: u64) -> i64 {
    if ft == 0 { 0 } else { (ft as i64 - 116_444_736_000_000_000) / 10_000_000 }
}

fn mode_of(attr: u32, name: &[u8]) -> u16 {
    let mut m: u16 = if attr & 0x10 != 0 { 0x4000 | 0x49 } else { 0x8000 };
    m |= 0x124; // readable
    if attr & 1 == 0 {
        m |= 0x92;
    }
    let ext: Vec<u8> = name.iter().rev().take(4).rev().map(|c| c.to_ascii_uppercase()).collect();
    if [&b".EXE"[..], b".COM", b".BAT", b".CMD"].contains(&&ext[..]) {
        m |= 0x49;
    }
    m
}

pub fn stat_path(p: &[u8]) -> Option<Info> {
    let mut path = p.to_vec();
    // "dir\" and "dir/" name the directory.
    while path.len() > 3 && (path.ends_with(b"\\") || path.ends_with(b"/")) {
        path.pop();
    }
    let pz = z(&path);
    let mut d = [0u32; 9];
    if unsafe { k::GetFileAttributesExA(pz.as_ptr(), 0, d.as_mut_ptr()) } == 0 {
        set_dos_errno(last());
        return None;
    }
    let ft = |i: usize| d[i] as u64 | (d[i + 1] as u64) << 32;
    let full = {
        let mut b = [0u8; 260];
        let n = unsafe { k::GetFullPathNameA(pz.as_ptr(), 260, b.as_mut_ptr(), core::ptr::null_mut()) } as usize;
        b[..n.min(259)].to_vec()
    };
    let drive = full.first().map_or(0, |c| (c.to_ascii_uppercase().wrapping_sub(b'A')) as u32);
    Some(Info {
        mode: mode_of(d[0], &path),
        size: d[8] as u64 | (d[7] as u64) << 32,
        ctime: ft_to_time(ft(1)),
        atime: ft_to_time(ft(3)),
        mtime: ft_to_time(ft(5)),
        drive,
    })
}

pub fn stat_fd(fd: i32) -> Option<Info> {
    let Some(f) = crate::io::get(fd) else {
        set_errno(crate::EBADF);
        return None;
    };
    if f.dev {
        return Some(Info { mode: 0x2000 | 0x1b6, size: 0, atime: 0, mtime: 0, ctime: 0, drive: 0 });
    }
    let mut d = [0u32; 13];
    if unsafe { k::GetFileInformationByHandle(f.h, d.as_mut_ptr()) } == 0 {
        // A pipe.
        return Some(Info { mode: 0x1000 | 0x1b6, size: 0, atime: 0, mtime: 0, ctime: 0, drive: 0 });
    }
    let ft = |i: usize| d[i] as u64 | (d[i + 1] as u64) << 32;
    Some(Info {
        mode: mode_of(d[0], b""),
        size: d[8] as u64 | (d[7] as u64) << 32,
        ctime: ft_to_time(ft(1)),
        atime: ft_to_time(ft(3)),
        mtime: ft_to_time(ft(5)),
        drive: 0,
    })
}

/// The common head of every struct _stat: st_dev, st_ino, st_mode,
/// st_nlink, st_uid, st_gid, st_rdev.
unsafe fn head(p: *mut u8, i: &Info) {
    unsafe {
        core::ptr::write_bytes(p, 0, 20);
        (p as *mut u32).write_unaligned(i.drive);
        (p.add(6) as *mut u16).write_unaligned(i.mode);
        (p.add(8) as *mut i16).write_unaligned(1);
        (p.add(16) as *mut u32).write_unaligned(i.drive);
    }
}

/// Which struct _stat: (64-bit times, 64-bit size).
unsafe fn fill(p: *mut u8, i: &Info, t64: bool, s64: bool) {
    unsafe {
        head(p, i);
        match (t64, s64) {
            (false, false) => {
                (p.add(20) as *mut i32).write_unaligned(i.size as i32);
                (p.add(24) as *mut i32).write_unaligned(i.atime as i32);
                (p.add(28) as *mut i32).write_unaligned(i.mtime as i32);
                (p.add(32) as *mut i32).write_unaligned(i.ctime as i32);
            }
            (true, false) => {
                (p.add(20) as *mut i32).write_unaligned(i.size as i32);
                (p.add(24) as *mut i64).write_unaligned(i.atime);
                (p.add(32) as *mut i64).write_unaligned(i.mtime);
                (p.add(40) as *mut i64).write_unaligned(i.ctime);
            }
            (false, true) => {
                (p.add(24) as *mut u64).write_unaligned(i.size);
                (p.add(32) as *mut i32).write_unaligned(i.atime as i32);
                (p.add(36) as *mut i32).write_unaligned(i.mtime as i32);
                (p.add(40) as *mut i32).write_unaligned(i.ctime as i32);
            }
            (true, true) => {
                (p.add(24) as *mut u64).write_unaligned(i.size);
                (p.add(32) as *mut i64).write_unaligned(i.atime);
                (p.add(40) as *mut i64).write_unaligned(i.mtime);
                (p.add(48) as *mut i64).write_unaligned(i.ctime);
            }
        }
    }
}

macro_rules! stat_fns {
    ($path:ident, $wpath:ident, $fd:ident, $t64:expr, $s64:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $path(p: *const u8, out: *mut u8) -> i32 {
            match stat_path(unsafe { cstr(p) }) {
                Some(i) => {
                    unsafe { fill(out, &i, $t64, $s64) };
                    0
                }
                None => -1,
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $wpath(p: *const u16, out: *mut u8) -> i32 {
            match stat_path(&crate::narrow(unsafe { wstr(p) })) {
                Some(i) => {
                    unsafe { fill(out, &i, $t64, $s64) };
                    0
                }
                None => -1,
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $fd(fd: i32, out: *mut u8) -> i32 {
            match stat_fd(fd) {
                Some(i) => {
                    unsafe { fill(out, &i, $t64, $s64) };
                    0
                }
                None => -1,
            }
        }
    };
}

// The old runtime's _stat has 32-bit times and size.
stat_fns!(_stat, _wstat, _fstat, false, false);
stat_fns!(_stat32, _wstat32, _fstat32, false, false);
stat_fns!(_stat64i32, _wstat64i32, _fstat64i32, true, false);
stat_fns!(_stati64, _wstati64, _fstati64, false, true);
stat_fns!(_stat32i64, _wstat32i64, _fstat32i64, false, true);
stat_fns!(_stat64, _wstat64, _fstat64, true, true);

// ---- _findfirst ----

/// Fill a _finddata structure from WIN32_FIND_DATAA. Kinds: 0
/// _finddata32_t, 1 _finddata64i32_t, 2 _finddata32i64_t, 3
/// _finddata64_t.
unsafe fn find_fill(out: *mut u8, fd: &[u8; 320], kind: u32) {
    let u32_at = |o: usize| u32::from_le_bytes([fd[o], fd[o + 1], fd[o + 2], fd[o + 3]]);
    let ft = |o: usize| ft_to_time(u32_at(o) as u64 | (u32_at(o + 4) as u64) << 32);
    let attr = if u32_at(0) == 0x80 { 0 } else { u32_at(0) };
    let size = u32_at(32) as u64 | (u32_at(28) as u64) << 32;
    let name = unsafe { cstr(fd.as_ptr().add(44)) };
    unsafe {
        (out as *mut u32).write_unaligned(attr);
        let name_at = match kind {
            0 => {
                (out.add(4) as *mut i32).write_unaligned(ft(4) as i32);
                (out.add(8) as *mut i32).write_unaligned(ft(12) as i32);
                (out.add(12) as *mut i32).write_unaligned(ft(20) as i32);
                (out.add(16) as *mut u32).write_unaligned(size as u32);
                20
            }
            1 => {
                (out.add(8) as *mut i64).write_unaligned(ft(4));
                (out.add(16) as *mut i64).write_unaligned(ft(12));
                (out.add(24) as *mut i64).write_unaligned(ft(20));
                (out.add(32) as *mut u32).write_unaligned(size as u32);
                36
            }
            2 => {
                (out.add(4) as *mut i32).write_unaligned(ft(4) as i32);
                (out.add(8) as *mut i32).write_unaligned(ft(12) as i32);
                (out.add(12) as *mut i32).write_unaligned(ft(20) as i32);
                (out.add(16) as *mut u64).write_unaligned(size);
                24
            }
            _ => {
                (out.add(8) as *mut i64).write_unaligned(ft(4));
                (out.add(16) as *mut i64).write_unaligned(ft(12));
                (out.add(24) as *mut i64).write_unaligned(ft(20));
                (out.add(32) as *mut u64).write_unaligned(size);
                40
            }
        };
        core::ptr::copy_nonoverlapping(name.as_ptr(), out.add(name_at), name.len().min(259));
        *out.add(name_at + name.len().min(259)) = 0;
    }
}

unsafe fn find_first(spec: *const u8, out: *mut u8, kind: u32) -> i32 {
    let mut fd = [0u8; 320];
    let h = unsafe { k::FindFirstFileA(spec, fd.as_mut_ptr()) };
    if h == 0xffff_ffff {
        set_dos_errno(last());
        return -1;
    }
    unsafe { find_fill(out, &fd, kind) };
    h as i32
}

unsafe fn find_next(h: i32, out: *mut u8, kind: u32) -> i32 {
    let mut fd = [0u8; 320];
    if unsafe { k::FindNextFileA(h as u32, fd.as_mut_ptr()) } == 0 {
        set_errno(ENOENT);
        return -1;
    }
    unsafe { find_fill(out, &fd, kind) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findfirst(spec: *const u8, out: *mut u8) -> i32 {
    unsafe { find_first(spec, out, 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findfirst32(spec: *const u8, out: *mut u8) -> i32 {
    unsafe { find_first(spec, out, 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findfirst64i32(spec: *const u8, out: *mut u8) -> i32 {
    unsafe { find_first(spec, out, 1) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findfirsti64(spec: *const u8, out: *mut u8) -> i32 {
    unsafe { find_first(spec, out, 2) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findfirst32i64(spec: *const u8, out: *mut u8) -> i32 {
    unsafe { find_first(spec, out, 2) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findfirst64(spec: *const u8, out: *mut u8) -> i32 {
    unsafe { find_first(spec, out, 3) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findnext(h: i32, out: *mut u8) -> i32 {
    unsafe { find_next(h, out, 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findnext32(h: i32, out: *mut u8) -> i32 {
    unsafe { find_next(h, out, 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findnext64i32(h: i32, out: *mut u8) -> i32 {
    unsafe { find_next(h, out, 1) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findnexti64(h: i32, out: *mut u8) -> i32 {
    unsafe { find_next(h, out, 2) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findnext32i64(h: i32, out: *mut u8) -> i32 {
    unsafe { find_next(h, out, 2) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _findnext64(h: i32, out: *mut u8) -> i32 {
    unsafe { find_next(h, out, 3) }
}

#[unsafe(no_mangle)]
pub extern "C" fn _findclose(h: i32) -> i32 {
    ok_or_errno(unsafe { k::FindClose(h as u32) } != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _utime(_p: *const u8, _t: *const u8) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _utime64(_p: *const u8, _t: *const u8) -> i32 {
    0
}
