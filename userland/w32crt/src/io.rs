//! File descriptors (_open, _read, _write...) over Windows handles. In
//! text mode reads turn CR LF into LF and stop at Ctrl+Z, writes turn LF
//! into CR LF.

use alloc::vec::Vec;

use crate::{cstr, k, set_dos_errno, set_errno, wstr, z, Global, EBADF, EINVAL, EMFILE};

pub const O_RDONLY: i32 = 0;
pub const O_WRONLY: i32 = 1;
pub const O_RDWR: i32 = 2;
pub const O_APPEND: i32 = 0x8;
pub const O_CREAT: i32 = 0x100;
pub const O_TRUNC: i32 = 0x200;
pub const O_EXCL: i32 = 0x400;
pub const O_TEXT: i32 = 0x4000;
pub const O_BINARY: i32 = 0x8000;
pub const O_NOINHERIT: i32 = 0x80;

#[derive(Clone, Copy)]
pub struct Fd {
    pub h: u32,
    pub text: bool,
    pub append: bool,
    /// Ctrl+Z seen (text mode).
    pub eof: bool,
    /// A CR read at the end of a buffer, waiting to see its LF.
    pub pending_cr: bool,
    pub dev: bool,
}

static FDS: Global<Vec<Option<Fd>>> = Global::new(Vec::new());

pub fn init() {
    let text = (unsafe { crate::start::_fmode }) & O_BINARY == 0;
    FDS.with(|v| {
        for (i, n) in [0xffff_fff6u32, 0xffff_fff5, 0xffff_fff4].iter().enumerate() {
            let h = unsafe { k::GetStdHandle(*n) };
            let dev = unsafe { k::GetFileType(h) } == 2;
            v.push(Some(Fd { h, text, append: false, eof: false, pending_cr: false, dev }));
            let _ = i;
        }
    });
}

pub fn get(fd: i32) -> Option<Fd> {
    if fd < 0 {
        return None;
    }
    FDS.with(|v| v.get(fd as usize).copied().flatten())
}

fn set(fd: i32, f: Fd) {
    FDS.with(|v| {
        if let Some(x) = v.get_mut(fd as usize) {
            *x = Some(f);
        }
    });
}

pub fn alloc_fd(f: Fd) -> i32 {
    FDS.with(|v| match v.iter().position(|x| x.is_none()) {
        Some(i) => {
            v[i] = Some(f);
            i as i32
        }
        None => {
            if v.len() >= 2048 {
                return -1;
            }
            v.push(Some(f));
            v.len() as i32 - 1
        }
    })
}

pub fn open_path(path: &[u8], flags: i32) -> i32 {
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    let access = match flags & 3 {
        O_WRONLY => GENERIC_WRITE,
        O_RDWR => GENERIC_READ | GENERIC_WRITE,
        _ => GENERIC_READ,
    };
    let disp = if flags & O_CREAT != 0 {
        if flags & O_EXCL != 0 {
            1 // CREATE_NEW
        } else if flags & O_TRUNC != 0 {
            2 // CREATE_ALWAYS
        } else {
            4 // OPEN_ALWAYS
        }
    } else if flags & O_TRUNC != 0 {
        5 // TRUNCATE_EXISTING
    } else {
        3 // OPEN_EXISTING
    };
    // Inherited unless _O_NOINHERIT, as the C runtime's are.
    let sa = [12u32, 0, (flags & O_NOINHERIT == 0) as u32];
    let p = z(path);
    let h = unsafe { k::CreateFileA(p.as_ptr(), access, 3, sa.as_ptr(), disp, 0x80, 0) };
    if h == 0xffff_ffff {
        set_dos_errno(unsafe { k::GetLastError() });
        return -1;
    }
    let text = if flags & O_BINARY != 0 {
        false
    } else if flags & O_TEXT != 0 {
        true
    } else {
        (unsafe { crate::start::_fmode }) & O_BINARY == 0
    };
    let dev = unsafe { k::GetFileType(h) } == 2;
    let fd = alloc_fd(Fd { h, text, append: flags & O_APPEND != 0, eof: false, pending_cr: false, dev });
    if fd < 0 {
        unsafe { k::CloseHandle(h) };
        set_errno(EMFILE);
    }
    fd
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _open(path: *const u8, flags: i32, _mode: i32) -> i32 {
    open_path(unsafe { cstr(path) }, flags)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _sopen(path: *const u8, flags: i32, _share: i32, _mode: i32) -> i32 {
    open_path(unsafe { cstr(path) }, flags)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _sopen_s(pfd: *mut i32, path: *const u8, flags: i32, _share: i32, _mode: i32) -> i32 {
    let fd = open_path(unsafe { cstr(path) }, flags);
    unsafe { *pfd = fd };
    if fd < 0 { crate::errno() } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wopen(path: *const u16, flags: i32, _mode: i32) -> i32 {
    open_path(&crate::narrow(unsafe { wstr(path) }), flags)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _creat(path: *const u8, _mode: i32) -> i32 {
    open_path(unsafe { cstr(path) }, O_WRONLY | O_CREAT | O_TRUNC)
}

#[unsafe(no_mangle)]
pub extern "C" fn _close(fd: i32) -> i32 {
    let f = FDS.with(|v| v.get_mut(fd.max(0) as usize).and_then(|x| x.take()));
    match f {
        Some(f) => {
            // The standard handles are the process's: the descriptor goes.
            if fd > 2 {
                unsafe { k::CloseHandle(f.h) };
            }
            0
        }
        None => {
            set_errno(EBADF);
            -1
        }
    }
}

fn raw_read(h: u32, buf: &mut [u8]) -> Result<usize, u32> {
    let mut n = 0u32;
    if unsafe { k::ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, &mut n, 0) } == 0 {
        let e = unsafe { k::GetLastError() };
        // A broken pipe is the end of the data.
        if e == 109 {
            return Ok(0);
        }
        return Err(e);
    }
    Ok(n as usize)
}

/// Read into `buf`; text mode translated. The number of bytes, or -1.
pub fn read_fd(fd: i32, buf: &mut [u8]) -> i32 {
    let Some(mut f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    if buf.is_empty() {
        return 0;
    }
    if !f.text {
        return match raw_read(f.h, buf) {
            Ok(n) => n as i32,
            Err(e) => {
                set_dos_errno(e);
                -1
            }
        };
    }
    if f.eof {
        return 0;
    }
    let mut raw = alloc::vec![0u8; buf.len()];
    let n = match raw_read(f.h, &mut raw) {
        Ok(n) => n,
        Err(e) => {
            set_dos_errno(e);
            return -1;
        }
    };
    let mut out = 0;
    let mut i = 0;
    if f.pending_cr {
        f.pending_cr = false;
        if n > 0 && raw[0] == b'\n' {
            buf[out] = b'\n';
            out += 1;
            i = 1;
        } else {
            buf[out] = b'\r';
            out += 1;
        }
    }
    while i < n && out < buf.len() {
        let c = raw[i];
        if c == 0x1a && !f.dev {
            f.eof = true;
            // Back to just before the Ctrl+Z.
            let back = (n - i) as i32;
            unsafe { k::SetFilePointer(f.h, -back, core::ptr::null_mut(), 1) };
            break;
        }
        if c == b'\r' {
            if i + 1 < n {
                if raw[i + 1] == b'\n' {
                    buf[out] = b'\n';
                    out += 1;
                    i += 2;
                    continue;
                }
            } else {
                // The CR is the last byte read: look at the next one.
                let mut one = [0u8; 1];
                match raw_read(f.h, &mut one) {
                    Ok(1) if one[0] == b'\n' => {
                        buf[out] = b'\n';
                        out += 1;
                        i += 1;
                        continue;
                    }
                    Ok(1) => {
                        unsafe { k::SetFilePointer(f.h, -1, core::ptr::null_mut(), 1) };
                    }
                    _ => {}
                }
            }
        }
        buf[out] = c;
        out += 1;
        i += 1;
    }
    set(fd, f);
    out as i32
}

fn raw_write(h: u32, b: &[u8]) -> Result<usize, u32> {
    let mut n = 0u32;
    if unsafe { k::WriteFile(h, b.as_ptr(), b.len() as u32, &mut n, 0) } == 0 {
        return Err(unsafe { k::GetLastError() });
    }
    Ok(n as usize)
}

/// Write `b`; text mode translated. The number of bytes of `b` written,
/// or -1.
pub fn write_fd(fd: i32, b: &[u8]) -> i32 {
    let Some(f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    if f.append {
        unsafe { k::SetFilePointer(f.h, 0, core::ptr::null_mut(), 2) };
    }
    let r = if f.text && b.contains(&b'\n') {
        let mut v = Vec::with_capacity(b.len() + 16);
        for &c in b {
            if c == b'\n' {
                v.push(b'\r');
            }
            v.push(c);
        }
        raw_write(f.h, &v).map(|n| if n == v.len() { b.len() } else { n.min(b.len()) })
    } else {
        raw_write(f.h, b)
    };
    match r {
        Ok(n) => n as i32,
        Err(e) => {
            set_dos_errno(e);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _read(fd: i32, buf: *mut u8, n: u32) -> i32 {
    read_fd(fd, unsafe { core::slice::from_raw_parts_mut(buf, n as usize) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _write(fd: i32, buf: *const u8, n: u32) -> i32 {
    write_fd(fd, unsafe { core::slice::from_raw_parts(buf, n as usize) })
}

pub fn seek_fd(fd: i32, off: i64, whence: i32) -> i64 {
    let Some(mut f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    if !(0..=2).contains(&whence) || off > i32::MAX as i64 || off < i32::MIN as i64 {
        set_errno(EINVAL);
        return -1;
    }
    let mut hi = if off < 0 { -1i32 } else { 0 };
    let r = unsafe { k::SetFilePointer(f.h, off as i32, &mut hi, whence as u32) };
    if r == 0xffff_ffff && unsafe { k::GetLastError() } != 0 {
        set_dos_errno(unsafe { k::GetLastError() });
        return -1;
    }
    f.eof = false;
    f.pending_cr = false;
    set(fd, f);
    r as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn _lseek(fd: i32, off: i32, whence: i32) -> i32 {
    seek_fd(fd, off as i64, whence) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn _lseeki64(fd: i32, off: i64, whence: i32) -> i64 {
    seek_fd(fd, off, whence)
}

#[unsafe(no_mangle)]
pub extern "C" fn _tell(fd: i32) -> i32 {
    _lseek(fd, 0, 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn _telli64(fd: i32) -> i64 {
    seek_fd(fd, 0, 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn _eof(fd: i32) -> i32 {
    let cur = seek_fd(fd, 0, 1);
    if cur < 0 {
        return -1;
    }
    let end = seek_fd(fd, 0, 2);
    seek_fd(fd, cur, 0);
    (cur >= end) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn _filelength(fd: i32) -> i32 {
    let Some(f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    unsafe { k::GetFileSize(f.h, core::ptr::null_mut()) as i32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn _filelengthi64(fd: i32) -> i64 {
    _filelength(fd) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn _chsize(fd: i32, size: i32) -> i32 {
    let Some(f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    let cur = seek_fd(fd, 0, 1);
    seek_fd(fd, size as i64, 0);
    let ok = unsafe { k::SetEndOfFile(f.h) } != 0;
    seek_fd(fd, cur, 0);
    if ok { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn _chsize_s(fd: i32, size: i64) -> i32 {
    if _chsize(fd, size as i32) == 0 { 0 } else { crate::errno() }
}

#[unsafe(no_mangle)]
pub extern "C" fn _commit(fd: i32) -> i32 {
    match get(fd) {
        Some(f) => {
            unsafe { k::FlushFileBuffers(f.h) };
            0
        }
        None => {
            set_errno(EBADF);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _isatty(fd: i32) -> i32 {
    get(fd).is_some_and(|f| f.dev) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn _setmode(fd: i32, mode: i32) -> i32 {
    let Some(mut f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    let old = if f.text { O_TEXT } else { O_BINARY };
    f.text = mode & O_BINARY == 0;
    set(fd, f);
    old
}

#[unsafe(no_mangle)]
pub extern "C" fn _get_osfhandle(fd: i32) -> u32 {
    match get(fd) {
        Some(f) => f.h,
        None => {
            set_errno(EBADF);
            0xffff_ffff
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _open_osfhandle(h: u32, flags: i32) -> i32 {
    let dev = unsafe { k::GetFileType(h) } == 2;
    alloc_fd(Fd { h, text: flags & O_TEXT != 0, append: flags & O_APPEND != 0, eof: false, pending_cr: false, dev })
}

fn dup_handle(h: u32) -> Option<u32> {
    let mut new = 0u32;
    let p = unsafe { k::GetCurrentProcess() };
    (unsafe { k::DuplicateHandle(p, h, p, &mut new, 0, 1, 2) } != 0).then_some(new)
}

#[unsafe(no_mangle)]
pub extern "C" fn _dup(fd: i32) -> i32 {
    let Some(f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    let Some(h) = dup_handle(f.h) else {
        set_errno(EMFILE);
        return -1;
    };
    alloc_fd(Fd { h, ..f })
}

#[unsafe(no_mangle)]
pub extern "C" fn _dup2(fd: i32, to: i32) -> i32 {
    let Some(f) = get(fd) else {
        set_errno(EBADF);
        return -1;
    };
    if fd == to {
        return 0;
    }
    let Some(h) = dup_handle(f.h) else {
        set_errno(EMFILE);
        return -1;
    };
    if get(to).is_some() {
        _close(to);
    }
    FDS.with(|v| {
        if v.len() <= to as usize {
            v.resize(to as usize + 1, None);
        }
        v[to as usize] = Some(Fd { h, ..f });
    });
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _pipe(fds: *mut i32, _size: u32, _mode: i32) -> i32 {
    let _ = fds;
    set_errno(EMFILE);
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn _locking(_fd: i32, _mode: i32, _n: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _umask(_m: i32) -> i32 {
    0
}
