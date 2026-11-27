//! FILE streams. A FILE has MSVCRT's layout (32 bytes: _ptr, _cnt, _base,
//! _flag, _file, _charbuf, _bufsiz, _tmpfname), and stdin, stdout and
//! stderr are the first three of `_iob`, as programs built for the old
//! runtime expect; `_cnt` stays 0, so old getc/putc macros call _filbuf
//! and _flsbuf. The stream's state is here, by `_charbuf` (its slot).
//!
//! Buffers hold text as the program sees it; the descriptor layer
//! translates line ends. In text mode ftell counts each LF still buffered
//! as two bytes on disk, as Microsoft's runtime does.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::io::{self, O_APPEND, O_BINARY, O_CREAT, O_EXCL, O_RDONLY, O_RDWR, O_TEXT, O_TRUNC, O_WRONLY};
use crate::{cstr, set_errno, wstr, Lock, EBADF, EINVAL};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct File {
    ptr: u32,
    cnt: i32,
    base: u32,
    flag: u32,
    file: i32,
    charbuf: i32,
    bufsiz: i32,
    tmpfname: u32,
}

const EMPTY: File = File { ptr: 0, cnt: 0, base: 0, flag: 0, file: -1, charbuf: 0, bufsiz: 0, tmpfname: 0 };

#[unsafe(no_mangle)]
pub static mut _iob: [File; 20] = [EMPTY; 20];

// _flag bits (MSVCRT's).
const IOREAD: u32 = 0x1;
const IOWRT: u32 = 0x2;
const IOEOF: u32 = 0x10;
const IOERR: u32 = 0x20;
const IORW: u32 = 0x80;

#[derive(PartialEq, Clone, Copy)]
enum Buf {
    Full,
    Line,
    None,
}

struct Stream {
    fd: i32,
    file: *mut File,
    /// Read data not yet taken: rbuf[rpos..].
    rbuf: Vec<u8>,
    rpos: usize,
    /// Written data not yet flushed.
    wbuf: Vec<u8>,
    mode: Buf,
    size: usize,
    eof: bool,
    err: bool,
    readable: bool,
    writable: bool,
    /// From the heap (fopen), not `_iob`.
    owned: bool,
    tmpname: Option<Vec<u8>>,
}

static LOCK: Lock = Lock::new();
static mut STREAMS: Vec<Option<Stream>> = Vec::new();

#[allow(static_mut_refs)]
fn streams() -> &'static mut Vec<Option<Stream>> {
    unsafe { &mut STREAMS }
}

/// The stream of a FILE (under LOCK).
fn st(f: *mut File) -> Option<&'static mut Stream> {
    if f.is_null() {
        return None;
    }
    let slot = unsafe { (*f).charbuf } - 1;
    if slot < 0 {
        return None;
    }
    streams().get_mut(slot as usize).and_then(|s| s.as_mut()).filter(|s| s.file == f)
}

fn add(s: Stream) -> *mut File {
    let f = s.file;
    let v = streams();
    let slot = match v.iter().position(|x| x.is_none()) {
        Some(i) => {
            v[i] = Some(s);
            i
        }
        None => {
            v.push(Some(s));
            v.len() - 1
        }
    };
    unsafe { (*f).charbuf = slot as i32 + 1 };
    f
}

fn update_flags(s: &Stream) {
    let mut fl = 0;
    if s.readable && s.writable {
        fl |= IORW;
    } else if s.readable {
        fl |= IOREAD;
    } else if s.writable {
        fl |= IOWRT;
    }
    if s.eof {
        fl |= IOEOF;
    }
    if s.err {
        fl |= IOERR;
    }
    unsafe {
        (*s.file).flag = fl;
        (*s.file).file = s.fd;
        (*s.file).cnt = 0;
    }
}

pub fn stream(i: usize) -> *mut File {
    unsafe { (&raw mut _iob[i]) as *mut File }
}

pub fn init() {
    let _g = LOCK.lock();
    for i in 0..3 {
        let tty = io::_isatty(i as i32) != 0;
        let s = Stream {
            fd: i as i32,
            file: stream(i),
            rbuf: Vec::new(),
            rpos: 0,
            wbuf: Vec::new(),
            // stdout is line buffered on a terminal, else fully; stderr
            // never.
            mode: match (i, tty) {
                (2, _) => Buf::None,
                (_, true) => Buf::Line,
                _ => Buf::Full,
            },
            size: 4096,
            eof: false,
            err: false,
            readable: i == 0,
            writable: i != 0,
            owned: false,
            tmpname: None,
        };
        update_flags(&s);
        add(s);
    }
}

fn flush_stream(s: &mut Stream) -> bool {
    if s.wbuf.is_empty() {
        return true;
    }
    let data = core::mem::take(&mut s.wbuf);
    let mut off = 0;
    while off < data.len() {
        let n = io::write_fd(s.fd, &data[off..]);
        if n <= 0 {
            s.err = true;
            update_flags(s);
            return false;
        }
        off += n as usize;
    }
    true
}

/// Give up read-ahead (before a write or a seek): the descriptor goes
/// back to the program's position.
fn drop_readahead(s: &mut Stream) {
    let left = s.rbuf.len() - s.rpos;
    if left > 0 {
        let text = io::get(s.fd).is_some_and(|f| f.text);
        let back = left + if text { s.rbuf[s.rpos..].iter().filter(|&&c| c == b'\n').count() } else { 0 };
        io::seek_fd(s.fd, -(back as i64), 1);
    }
    s.rbuf.clear();
    s.rpos = 0;
}

pub fn flush_all() {
    let _g = LOCK.lock();
    for s in streams().iter_mut().flatten() {
        flush_stream(s);
    }
}

fn write_locked(s: &mut Stream, b: &[u8]) -> bool {
    if !s.writable {
        s.err = true;
        update_flags(s);
        return false;
    }
    if !s.rbuf.is_empty() {
        drop_readahead(s);
    }
    match s.mode {
        Buf::None => {
            s.wbuf.extend_from_slice(b);
            flush_stream(s)
        }
        Buf::Line => {
            s.wbuf.extend_from_slice(b);
            if b.contains(&b'\n') || s.wbuf.len() >= s.size { flush_stream(s) } else { true }
        }
        Buf::Full => {
            s.wbuf.extend_from_slice(b);
            if s.wbuf.len() >= s.size { flush_stream(s) } else { true }
        }
    }
}

/// Write bytes to a stream: their number, or -1.
pub fn write_all(f: *mut File, b: &[u8]) -> i32 {
    let _g = LOCK.lock();
    let Some(s) = st(f) else {
        set_errno(EBADF);
        return -1;
    };
    if write_locked(s, b) { b.len() as i32 } else { -1 }
}

/// Fill the read buffer; false at the end or on an error.
fn fill(s: &mut Stream) -> bool {
    if !s.readable {
        s.err = true;
        update_flags(s);
        return false;
    }
    // Reading from the terminal flushes what was written to it.
    if s.fd == 0 {
        for o in streams().iter_mut().flatten() {
            if o.fd == 1 || o.fd == 2 {
                flush_stream(o);
            }
        }
    }
    flush_stream(s);
    let mut buf = alloc::vec![0u8; if s.mode == Buf::None { 1 } else { s.size.max(1) }];
    let n = io::read_fd(s.fd, &mut buf);
    if n <= 0 {
        if n == 0 {
            s.eof = true;
        } else {
            s.err = true;
        }
        update_flags(s);
        return false;
    }
    buf.truncate(n as usize);
    s.rbuf = buf;
    s.rpos = 0;
    true
}

fn getc_locked(s: &mut Stream) -> i32 {
    if s.rpos >= s.rbuf.len() && !fill(s) {
        return -1;
    }
    let c = s.rbuf[s.rpos];
    s.rpos += 1;
    c as i32
}

pub fn getc_impl(f: *mut File) -> i32 {
    let _g = LOCK.lock();
    match st(f) {
        Some(s) => getc_locked(s),
        None => -1,
    }
}

pub fn ungetc_impl(c: i32, f: *mut File) -> i32 {
    if c < 0 {
        return -1;
    }
    let _g = LOCK.lock();
    let Some(s) = st(f) else { return -1 };
    if s.rpos > 0 {
        s.rpos -= 1;
        s.rbuf[s.rpos] = c as u8;
    } else {
        s.rbuf.insert(0, c as u8);
    }
    s.eof = false;
    update_flags(s);
    c & 0xff
}

// ---- Opening and closing ----

/// fopen's mode string: the _open flags, or None.
fn mode_flags(m: &[u8]) -> Option<(i32, bool, bool)> {
    let first = *m.first()?;
    let plus = m.contains(&b'+');
    let (mut flags, r, w) = match first {
        b'r' => (if plus { O_RDWR } else { O_RDONLY }, true, plus),
        b'w' => ((if plus { O_RDWR } else { O_WRONLY }) | O_CREAT | O_TRUNC, plus, true),
        b'a' => ((if plus { O_RDWR } else { O_WRONLY }) | O_CREAT | O_APPEND, plus, true),
        _ => return None,
    };
    for &c in &m[1..] {
        match c {
            b'b' => flags |= O_BINARY,
            b't' => flags |= O_TEXT,
            b'x' => flags |= O_EXCL,
            b'N' => flags |= io::O_NOINHERIT,
            _ => {}
        }
    }
    Some((flags, r, w))
}

fn new_stream(fd: i32, r: bool, w: bool, file: *mut File, owned: bool) -> *mut File {
    let tty = io::_isatty(fd) != 0;
    let s = Stream {
        fd,
        file,
        rbuf: Vec::new(),
        rpos: 0,
        wbuf: Vec::new(),
        mode: if tty { Buf::Line } else { Buf::Full },
        size: 4096,
        eof: false,
        err: false,
        readable: r,
        writable: w,
        owned,
        tmpname: None,
    };
    unsafe { *file = EMPTY };
    update_flags(&s);
    let _g = LOCK.lock();
    add(s)
}

fn open_file(name: &[u8], mode: &[u8]) -> *mut File {
    let Some((flags, r, w)) = mode_flags(mode) else {
        set_errno(EINVAL);
        return core::ptr::null_mut();
    };
    let fd = io::open_path(name, flags);
    if fd < 0 {
        return core::ptr::null_mut();
    }
    let file = Box::into_raw(Box::new(EMPTY));
    new_stream(fd, r, w, file, true)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fopen(name: *const u8, mode: *const u8) -> *mut File {
    open_file(unsafe { cstr(name) }, unsafe { cstr(mode) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fopen_s(pf: *mut *mut File, name: *const u8, mode: *const u8) -> i32 {
    let f = unsafe { fopen(name, mode) };
    unsafe { *pf = f };
    if f.is_null() { crate::errno() } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _fsopen(name: *const u8, mode: *const u8, _share: i32) -> *mut File {
    unsafe { fopen(name, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wfopen(name: *const u16, mode: *const u16) -> *mut File {
    open_file(&crate::narrow(unsafe { wstr(name) }), &crate::narrow(unsafe { wstr(mode) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wfsopen(name: *const u16, mode: *const u16, _share: i32) -> *mut File {
    unsafe { _wfopen(name, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _fdopen(fd: i32, mode: *const u8) -> *mut File {
    let Some((_, r, w)) = mode_flags(unsafe { cstr(mode) }) else {
        set_errno(EINVAL);
        return core::ptr::null_mut();
    };
    if io::get(fd).is_none() {
        set_errno(EBADF);
        return core::ptr::null_mut();
    }
    let file = Box::into_raw(Box::new(EMPTY));
    new_stream(fd, r, w, file, true)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _wfdopen(fd: i32, mode: *const u16) -> *mut File {
    let m = crate::z(&crate::narrow(unsafe { wstr(mode) }));
    unsafe { _fdopen(fd, m.as_ptr()) }
}

fn close_locked(f: *mut File) -> i32 {
    let slot = unsafe { (*f).charbuf } - 1;
    let Some(s) = st(f) else {
        set_errno(EBADF);
        return -1;
    };
    let ok = flush_stream(s);
    let fd = s.fd;
    let owned = s.owned;
    let tmp = s.tmpname.take();
    streams()[slot as usize] = None;
    let r = io::_close(fd);
    if let Some(t) = tmp {
        let p = crate::z(&t);
        unsafe { crate::k::DeleteFileA(p.as_ptr()) };
    }
    unsafe {
        (*f).charbuf = 0;
        (*f).flag = 0;
    }
    if owned {
        unsafe { drop(Box::from_raw(f)) };
    }
    if ok && r == 0 { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn fclose(f: *mut File) -> i32 {
    let _g = LOCK.lock();
    close_locked(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn _fcloseall() -> i32 {
    let _g = LOCK.lock();
    let files: Vec<*mut File> = streams().iter().flatten().filter(|s| s.owned).map(|s| s.file).collect();
    for f in &files {
        close_locked(*f);
    }
    files.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn freopen(name: *const u8, mode: *const u8, f: *mut File) -> *mut File {
    let Some((flags, r, w)) = mode_flags(unsafe { cstr(mode) }) else {
        set_errno(EINVAL);
        return core::ptr::null_mut();
    };
    let std_fd = {
        let _g = LOCK.lock();
        st(f).map(|s| (s.fd, s.owned))
    };
    let Some((old_fd, owned)) = std_fd else {
        set_errno(EBADF);
        return core::ptr::null_mut();
    };
    {
        let _g = LOCK.lock();
        if let Some(s) = st(f) {
            flush_stream(s);
            let slot = unsafe { (*f).charbuf } - 1;
            streams()[slot as usize] = None;
        }
    }
    let fd = io::open_path(unsafe { cstr(name) }, flags);
    if fd < 0 {
        return core::ptr::null_mut();
    }
    // A standard stream keeps its descriptor number.
    let fd = if old_fd <= 2 {
        io::_dup2(fd, old_fd);
        io::_close(fd);
        old_fd
    } else {
        io::_close(old_fd);
        fd
    };
    new_stream(fd, r, w, f, owned)
}

#[unsafe(no_mangle)]
pub extern "C" fn tmpfile() -> *mut File {
    let mut dir = [0u8; 260];
    let n = unsafe { crate::k::GetTempPathA(260, dir.as_mut_ptr()) } as usize;
    for i in 0..1000u32 {
        let mut name = dir[..n].to_vec();
        name.extend_from_slice(alloc::format!("T{:07X}.TMP", (unsafe { crate::k::GetTickCount() } ^ i * 7919) & 0xfff_ffff).as_bytes());
        let fd = io::open_path(&name, O_RDWR | O_CREAT | O_EXCL | O_BINARY);
        if fd >= 0 {
            let file = Box::into_raw(Box::new(EMPTY));
            let f = new_stream(fd, true, true, file, true);
            let _g = LOCK.lock();
            if let Some(s) = st(f) {
                s.tmpname = Some(name);
            }
            return f;
        }
    }
    core::ptr::null_mut()
}

static mut TMPNAM: [u8; 260] = [0; 260];
static TMPN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tmpnam(buf: *mut u8) -> *mut u8 {
    let n = TMPN.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let name = alloc::format!("\\s{:x}.{}", unsafe { crate::k::GetCurrentProcessId() }, n + 1);
    let out = if buf.is_null() { unsafe { (&raw mut TMPNAM) as *mut u8 } } else { buf };
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), out, name.len());
        *out.add(name.len()) = 0;
    }
    out
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _tempnam(dir: *const u8, prefix: *const u8) -> *mut u8 {
    let mut d = unsafe { cstr(dir) }.to_vec();
    if d.is_empty() {
        let mut t = [0u8; 260];
        let n = unsafe { crate::k::GetTempPathA(260, t.as_mut_ptr()) } as usize;
        d = t[..n].to_vec();
    }
    if !d.ends_with(b"\\") {
        d.push(b'\\');
    }
    d.extend_from_slice(unsafe { cstr(prefix) });
    let n = TMPN.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    d.extend_from_slice(alloc::format!("{:x}{}", unsafe { crate::k::GetCurrentProcessId() }, n + 1).as_bytes());
    crate::c_alloc_str(&d)
}

// ---- Data ----

#[unsafe(no_mangle)]
pub extern "C" fn fflush(f: *mut File) -> i32 {
    let _g = LOCK.lock();
    if f.is_null() {
        for s in streams().iter_mut().flatten() {
            flush_stream(s);
        }
        return 0;
    }
    match st(f) {
        Some(s) => {
            if !s.rbuf.is_empty() && s.writable {
                drop_readahead(s);
            }
            if flush_stream(s) { 0 } else { -1 }
        }
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _flushall() -> i32 {
    fflush(core::ptr::null_mut());
    streams().iter().flatten().count() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fwrite(p: *const u8, size: usize, n: usize, f: *mut File) -> usize {
    let total = size.saturating_mul(n);
    if total == 0 {
        return 0;
    }
    let b = unsafe { core::slice::from_raw_parts(p, total) };
    if write_all(f, b) < 0 { 0 } else { n }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fread(p: *mut u8, size: usize, n: usize, f: *mut File) -> usize {
    let total = size.saturating_mul(n);
    if total == 0 {
        return 0;
    }
    let _g = LOCK.lock();
    let Some(s) = st(f) else { return 0 };
    let mut got = 0;
    while got < total {
        if s.rpos >= s.rbuf.len() && !fill(s) {
            break;
        }
        let k = (s.rbuf.len() - s.rpos).min(total - got);
        unsafe { core::ptr::copy_nonoverlapping(s.rbuf.as_ptr().add(s.rpos), p.add(got), k) };
        s.rpos += k;
        got += k;
    }
    got / size
}

#[unsafe(no_mangle)]
pub extern "C" fn fgetc(f: *mut File) -> i32 {
    getc_impl(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn getc(f: *mut File) -> i32 {
    getc_impl(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn _fgetc_nolock(f: *mut File) -> i32 {
    getc_impl(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn _getc_nolock(f: *mut File) -> i32 {
    getc_impl(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn getchar() -> i32 {
    getc_impl(stream(0))
}

#[unsafe(no_mangle)]
pub extern "C" fn _fgetchar() -> i32 {
    getchar()
}

/// _filbuf: the old getc macro's slow path.
#[unsafe(no_mangle)]
pub extern "C" fn _filbuf(f: *mut File) -> i32 {
    getc_impl(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn fputc(c: i32, f: *mut File) -> i32 {
    if write_all(f, &[c as u8]) == 1 { c & 0xff } else { -1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn putc(c: i32, f: *mut File) -> i32 {
    fputc(c, f)
}

#[unsafe(no_mangle)]
pub extern "C" fn _fputc_nolock(c: i32, f: *mut File) -> i32 {
    fputc(c, f)
}

#[unsafe(no_mangle)]
pub extern "C" fn _putc_nolock(c: i32, f: *mut File) -> i32 {
    fputc(c, f)
}

#[unsafe(no_mangle)]
pub extern "C" fn putchar(c: i32) -> i32 {
    fputc(c, stream(1))
}

#[unsafe(no_mangle)]
pub extern "C" fn _fputchar(c: i32) -> i32 {
    putchar(c)
}

/// _flsbuf: the old putc macro's slow path.
#[unsafe(no_mangle)]
pub extern "C" fn _flsbuf(c: i32, f: *mut File) -> i32 {
    fputc(c, f)
}

#[unsafe(no_mangle)]
pub extern "C" fn ungetc(c: i32, f: *mut File) -> i32 {
    ungetc_impl(c, f)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fgets(buf: *mut u8, n: i32, f: *mut File) -> *mut u8 {
    if n <= 0 {
        return core::ptr::null_mut();
    }
    let _g = LOCK.lock();
    let Some(s) = st(f) else { return core::ptr::null_mut() };
    let mut i = 0usize;
    while i + 1 < n as usize {
        let c = getc_locked(s);
        if c < 0 {
            break;
        }
        unsafe { *buf.add(i) = c as u8 };
        i += 1;
        if c == b'\n' as i32 {
            break;
        }
    }
    if i == 0 {
        return core::ptr::null_mut();
    }
    unsafe { *buf.add(i) = 0 };
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gets(buf: *mut u8) -> *mut u8 {
    let _g = LOCK.lock();
    let Some(s) = st(stream(0)) else { return core::ptr::null_mut() };
    let mut i = 0usize;
    loop {
        let c = getc_locked(s);
        if c < 0 {
            if i == 0 {
                return core::ptr::null_mut();
            }
            break;
        }
        if c == b'\n' as i32 {
            break;
        }
        unsafe { *buf.add(i) = c as u8 };
        i += 1;
    }
    unsafe { *buf.add(i) = 0 };
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fputs(s: *const u8, f: *mut File) -> i32 {
    if write_all(f, unsafe { cstr(s) }) < 0 { -1 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn puts(s: *const u8) -> i32 {
    let _g = LOCK.lock();
    let Some(st) = st(stream(1)) else { return -1 };
    let ok = write_locked(st, unsafe { cstr(s) }) && write_locked(st, b"\n");
    if ok { 0 } else { -1 }
}

// ---- Wide characters (through the code page) ----

#[unsafe(no_mangle)]
pub extern "C" fn fgetwc(f: *mut File) -> u32 {
    let c = getc_impl(f);
    if c < 0 { 0xffff } else { crate::widen(&[c as u8]).first().copied().unwrap_or(c as u16) as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn getwc(f: *mut File) -> u32 {
    fgetwc(f)
}

#[unsafe(no_mangle)]
pub extern "C" fn getwchar() -> u32 {
    fgetwc(stream(0))
}

#[unsafe(no_mangle)]
pub extern "C" fn fputwc(c: u32, f: *mut File) -> u32 {
    let b = crate::narrow(&[c as u16]);
    if write_all(f, &b) < 0 { 0xffff } else { c }
}

#[unsafe(no_mangle)]
pub extern "C" fn putwc(c: u32, f: *mut File) -> u32 {
    fputwc(c, f)
}

#[unsafe(no_mangle)]
pub extern "C" fn putwchar(c: u32) -> u32 {
    fputwc(c, stream(1))
}

#[unsafe(no_mangle)]
pub extern "C" fn ungetwc(c: u32, f: *mut File) -> u32 {
    let b = crate::narrow(&[c as u16]);
    if ungetc_impl(b.first().copied().unwrap_or(b'?') as i32, f) < 0 { 0xffff } else { c }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fputws(s: *const u16, f: *mut File) -> i32 {
    if write_all(f, &crate::narrow(unsafe { wstr(s) })) < 0 { -1 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _putws(s: *const u16) -> i32 {
    let mut b = crate::narrow(unsafe { wstr(s) });
    b.push(b'\n');
    if write_all(stream(1), &b) < 0 { -1 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fgetws(buf: *mut u16, n: i32, f: *mut File) -> *mut u16 {
    let mut tmp = alloc::vec![0u8; n.max(1) as usize];
    if unsafe { fgets(tmp.as_mut_ptr(), n, f) }.is_null() {
        return core::ptr::null_mut();
    }
    let w = crate::widen(unsafe { cstr(tmp.as_ptr()) });
    unsafe {
        core::ptr::copy_nonoverlapping(w.as_ptr(), buf, w.len());
        *buf.add(w.len()) = 0;
    }
    buf
}

// ---- Positions ----

fn tell_locked(s: &Stream) -> i64 {
    let pos = io::seek_fd(s.fd, 0, 1);
    if pos < 0 {
        return -1;
    }
    let text = io::get(s.fd).is_some_and(|f| f.text);
    let lf = |b: &[u8]| if text { b.iter().filter(|&&c| c == b'\n').count() as i64 } else { 0 };
    let left = &s.rbuf[s.rpos.min(s.rbuf.len())..];
    pos - left.len() as i64 - lf(left) + s.wbuf.len() as i64 + lf(&s.wbuf)
}

#[unsafe(no_mangle)]
pub extern "C" fn ftell(f: *mut File) -> i32 {
    let _g = LOCK.lock();
    st(f).map_or(-1, |s| tell_locked(s) as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn _ftelli64(f: *mut File) -> i64 {
    let _g = LOCK.lock();
    st(f).map_or(-1, |s| tell_locked(s))
}

#[unsafe(no_mangle)]
pub extern "C" fn ftello(f: *mut File) -> i32 {
    ftell(f)
}

fn seek(f: *mut File, off: i64, whence: i32) -> i32 {
    let _g = LOCK.lock();
    let Some(s) = st(f) else {
        set_errno(EBADF);
        return -1;
    };
    let target = if whence == 1 { tell_locked(s) + off } else { off };
    flush_stream(s);
    s.rbuf.clear();
    s.rpos = 0;
    let r = io::seek_fd(s.fd, target, if whence == 1 { 0 } else { whence });
    s.eof = false;
    update_flags(s);
    if r < 0 { -1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn fseek(f: *mut File, off: i32, whence: i32) -> i32 {
    seek(f, off as i64, whence)
}

#[unsafe(no_mangle)]
pub extern "C" fn _fseeki64(f: *mut File, off: i64, whence: i32) -> i32 {
    seek(f, off, whence)
}

#[unsafe(no_mangle)]
pub extern "C" fn fseeko(f: *mut File, off: i32, whence: i32) -> i32 {
    seek(f, off as i64, whence)
}

#[unsafe(no_mangle)]
pub extern "C" fn rewind(f: *mut File) {
    seek(f, 0, 0);
    let _g = LOCK.lock();
    if let Some(s) = st(f) {
        s.err = false;
        update_flags(s);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fgetpos(f: *mut File, pos: *mut i64) -> i32 {
    let p = _ftelli64(f);
    if p < 0 {
        return -1;
    }
    unsafe { pos.write_unaligned(p) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fsetpos(f: *mut File, pos: *const i64) -> i32 {
    seek(f, unsafe { pos.read_unaligned() }, 0)
}

// ---- State ----

#[unsafe(no_mangle)]
pub extern "C" fn feof(f: *mut File) -> i32 {
    let _g = LOCK.lock();
    st(f).is_some_and(|s| s.eof) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn ferror(f: *mut File) -> i32 {
    let _g = LOCK.lock();
    st(f).is_some_and(|s| s.err) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn clearerr(f: *mut File) {
    let _g = LOCK.lock();
    if let Some(s) = st(f) {
        s.eof = false;
        s.err = false;
        update_flags(s);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _fileno(f: *mut File) -> i32 {
    let _g = LOCK.lock();
    st(f).map_or(-1, |s| s.fd)
}

#[unsafe(no_mangle)]
pub extern "C" fn fileno(f: *mut File) -> i32 {
    _fileno(f)
}

/// setvbuf: _IOFBF 0, _IOLBF 0x40, _IONBF 4 (the buffer given is not
/// used: streams keep their own).
#[unsafe(no_mangle)]
pub extern "C" fn setvbuf(f: *mut File, _buf: *mut u8, mode: i32, size: usize) -> i32 {
    let _g = LOCK.lock();
    let Some(s) = st(f) else { return -1 };
    flush_stream(s);
    s.mode = match mode {
        4 => Buf::None,
        0x40 => Buf::Line,
        0 => Buf::Full,
        _ => return -1,
    };
    if (2..=0x7fff_ffff).contains(&size) {
        s.size = size;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn setbuf(f: *mut File, buf: *mut u8) {
    setvbuf(f, buf, if buf.is_null() { 4 } else { 0 }, 512);
}

#[unsafe(no_mangle)]
pub extern "C" fn _lock_file(_f: *mut File) {}

#[unsafe(no_mangle)]
pub extern "C" fn _unlock_file(_f: *mut File) {}

#[unsafe(no_mangle)]
pub extern "C" fn __iob_func() -> *mut File {
    stream(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn __p__iob() -> *mut File {
    stream(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn __acrt_iob_func(i: u32) -> *mut File {
    stream(i.min(19) as usize)
}

#[unsafe(no_mangle)]
pub extern "C" fn _getmaxstdio() -> i32 {
    512
}

#[unsafe(no_mangle)]
pub extern "C" fn _setmaxstdio(n: i32) -> i32 {
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn perror(s: *const u8) {
    let mut v = Vec::new();
    let p = unsafe { cstr(s) };
    if !p.is_empty() {
        v.extend_from_slice(p);
        v.extend_from_slice(b": ");
    }
    v.extend_from_slice(crate::string::message(crate::errno()).as_bytes());
    v.push(b'\n');
    write_all(stream(2), &v);
}

#[unsafe(no_mangle)]
pub extern "C" fn _getw(f: *mut File) -> i32 {
    let mut b = [0u8; 4];
    if unsafe { fread(b.as_mut_ptr(), 4, 1, f) } != 1 {
        return -1;
    }
    i32::from_le_bytes(b)
}

#[unsafe(no_mangle)]
pub extern "C" fn _putw(v: i32, f: *mut File) -> i32 {
    if write_all(f, &v.to_le_bytes()) == 4 { v } else { -1 }
}
