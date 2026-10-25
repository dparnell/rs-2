//! Helpers: strings, files, the clock, and the packing of Spl* result
//! buffers (fixed structures first, their strings from the end).

use alloc::vec::Vec;

use os2::{DateTime, FileFindBuf3};

/// The bytes of a NUL-terminated string (empty for NULL).
pub fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// Some string, or None for NULL.
pub fn opt<'a>(p: *const u8) -> Option<&'a [u8]> {
    (!p.is_null()).then(|| cstr(p))
}

/// A NUL-terminated copy.
pub fn z(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s);
    v.push(0);
    v
}

pub fn upper(s: &[u8]) -> Vec<u8> {
    s.iter().map(|c| c.to_ascii_uppercase()).collect()
}

pub fn eq(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `a` + `b` + ... as one string.
pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

/// Decimal digits of `n`, at least `w` of them.
pub fn dec(n: u32, w: usize) -> Vec<u8> {
    let mut d = Vec::new();
    let mut n = n;
    loop {
        d.push(b'0' + (n % 10) as u8);
        n /= 10;
        if n == 0 && d.len() >= w {
            break;
        }
    }
    d.reverse();
    d
}

pub fn parse_dec(s: &[u8]) -> Option<u32> {
    let s = trim(s);
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0u32, |a, &c| a.wrapping_mul(10).wrapping_add((c - b'0') as u32)))
}

pub fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(a, |i| i + 1);
    &s[a..b]
}

/// The fields of an INI value: `a;b;c;` -> [a, b, c].
pub fn fields(s: &[u8]) -> Vec<&[u8]> {
    let s = s.strip_suffix(b"\0").unwrap_or(s);
    let mut v: Vec<&[u8]> = s.split(|&c| c == b';').collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// A comma-separated list, without empty items.
pub fn list(s: &[u8]) -> Vec<Vec<u8>> {
    s.split(|&c| c == b',').map(trim).filter(|x| !x.is_empty()).map(|x| x.to_vec()).collect()
}

pub fn join(items: &[Vec<u8>], sep: u8) -> Vec<u8> {
    let mut v = Vec::new();
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            v.push(sep);
        }
        v.extend_from_slice(x);
    }
    v
}

// ---- Files ----

pub fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, os2::OPEN_ACTION_OPEN_IF_EXISTS | os2::OPEN_ACTION_FAIL_IF_NEW, os2::OPEN_ACCESS_READONLY | os2::OPEN_SHARE_DENYNONE, core::ptr::null_mut()) };
    if rc != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(out)
}

/// Open a file for writing, replacing it.
pub fn create(path: &[u8]) -> Option<u32> {
    let p = z(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, os2::OPEN_ACTION_CREATE_IF_NEW | os2::OPEN_ACTION_REPLACE_IF_EXISTS, os2::OPEN_ACCESS_READWRITE | os2::OPEN_SHARE_DENYWRITE, core::ptr::null_mut()) };
    (rc == 0).then_some(hf)
}

pub fn write_all(hf: u32, d: &[u8]) -> bool {
    let mut off = 0;
    while off < d.len() {
        let mut n = 0;
        if unsafe { os2::DosWrite(hf, d[off..].as_ptr(), (d.len() - off) as u32, &mut n) } != 0 || n == 0 {
            return false;
        }
        off += n as usize;
    }
    true
}

pub fn write_file(path: &[u8], d: &[u8]) -> bool {
    let Some(hf) = create(path) else { return false };
    let ok = write_all(hf, d);
    unsafe { os2::DosClose(hf) };
    ok
}

pub fn delete(path: &[u8]) -> bool {
    let p = z(path);
    unsafe { os2::DosDelete(p.as_ptr()) == 0 }
}

pub fn mkdir(path: &[u8]) {
    let p = z(path);
    unsafe { os2::DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
}

pub fn file_size(path: &[u8]) -> Option<u32> {
    let p = z(path);
    let mut b = [0u8; 32];
    // FILESTATUS3: cbFile at 12.
    (unsafe { os2::DosQueryPathInfo(p.as_ptr(), 1, b.as_mut_ptr(), b.len() as u32) } == 0).then(|| u32::from_le_bytes([b[12], b[13], b[14], b[15]]))
}

/// The names of the files matching a wildcard (`dirs` true: directories).
pub fn find(spec: &[u8], dirs: bool) -> Vec<Vec<u8>> {
    let p = z(spec);
    let mut out = Vec::new();
    let mut hdir: u32 = 0xffff_ffff; // HDIR_CREATE
    let mut buf: FileFindBuf3 = unsafe { core::mem::zeroed() };
    let mut count = 1;
    let attr = if dirs { 0x10 | 0x07 } else { 0x07 };
    let sz = core::mem::size_of::<FileFindBuf3>() as u32;
    let mut rc = unsafe { os2::DosFindFirst(p.as_ptr(), &mut hdir, attr, &mut buf as *mut _ as *mut u8, sz, &mut count, 1) };
    while rc == 0 && count > 0 {
        let name = &buf.name[..buf.cch_name as usize];
        let is_dir = buf.attr & 0x10 != 0;
        if is_dir == dirs && name != b"." && name != b".." {
            out.push(name.to_vec());
        }
        count = 1;
        rc = unsafe { os2::DosFindNext(hdir, &mut buf as *mut _ as *mut u8, sz, &mut count) };
    }
    if hdir != 0xffff_ffff {
        unsafe { os2::DosFindClose(hdir) };
    }
    out
}

pub fn boot_drive() -> u8 {
    let mut d = 3u32;
    unsafe { os2::DosQuerySysInfo(5, 5, &mut d, 4) };
    b'A' + (d.clamp(1, 26) - 1) as u8
}

// ---- Time ----

/// Seconds since 1970-01-01 (local time, as OS/2's spooler reports
/// submission times).
pub fn now() -> u32 {
    let mut dt = DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    let (y, m, d) = (dt.year as i64, dt.month as i64, dt.day as i64);
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days * 86400 + dt.hours as i64 * 3600 + dt.minutes as i64 * 60 + dt.seconds as i64) as u32
}

// ---- Result buffers ----

/// One entry of a result: its fixed bytes, and the strings or blocks its
/// pointer fields point to (offset of the field in `fixed`, data).
pub struct Rec {
    pub fixed: Vec<u8>,
    pub ptrs: Vec<(usize, Vec<u8>)>,
}

impl Rec {
    pub fn new() -> Rec {
        Rec { fixed: Vec::new(), ptrs: Vec::new() }
    }

    pub fn u16(&mut self, v: u16) -> &mut Rec {
        self.fixed.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Rec {
        self.fixed.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// A pointer to a NUL-terminated copy of `s`.
    pub fn str(&mut self, s: &[u8]) -> &mut Rec {
        self.ptrs.push((self.fixed.len(), z(s)));
        self.u32(0)
    }

    /// A pointer to a block (NULL when empty).
    pub fn block(&mut self, d: &[u8]) -> &mut Rec {
        if d.is_empty() {
            return self.u32(0);
        }
        self.ptrs.push((self.fixed.len(), d.to_vec()));
        self.u32(0)
    }

    /// A fixed-size character array.
    pub fn chars(&mut self, s: &[u8], n: usize) -> &mut Rec {
        let k = s.len().min(n - 1);
        self.fixed.extend_from_slice(&s[..k]);
        self.fixed.resize(self.fixed.len() + n - k, 0);
        self
    }

    /// Another record's fields appended (its pointers move with it).
    pub fn append(&mut self, r: Rec) -> &mut Rec {
        let base = self.fixed.len();
        self.fixed.extend_from_slice(&r.fixed);
        self.ptrs.extend(r.ptrs.into_iter().map(|(o, d)| (o + base, d)));
        self
    }

    pub fn size(&self) -> usize {
        self.fixed.len() + self.ptrs.iter().map(|(_, d)| d.len()).sum::<usize>()
    }
}

pub const ERROR_MORE_DATA: u32 = 234;
pub const NERR_BUF_TOO_SMALL: u32 = 2123;

/// Lay records into the caller's buffer: as many whole entries as fit,
/// fixed parts from the start, data from the end (pointers are flat
/// addresses). Returns (entries placed, bytes needed for all).
pub fn pack(recs: &[Rec], buf: *mut u8, cb: u32) -> (u32, u32) {
    let need: usize = recs.iter().map(Rec::size).sum();
    if buf.is_null() {
        return (0, need as u32);
    }
    let cb = cb as usize;
    let (mut lo, mut hi, mut n) = (0usize, cb, 0u32);
    for r in recs {
        let data: usize = r.ptrs.iter().map(|(_, d)| d.len()).sum();
        if lo + r.fixed.len() + data > hi {
            break;
        }
        unsafe { core::ptr::copy_nonoverlapping(r.fixed.as_ptr(), buf.add(lo), r.fixed.len()) };
        for (off, d) in &r.ptrs {
            hi -= d.len();
            unsafe {
                core::ptr::copy_nonoverlapping(d.as_ptr(), buf.add(hi), d.len());
                (buf.add(lo + off) as *mut u32).write_unaligned(buf.add(hi) as u32);
            }
        }
        lo += r.fixed.len();
        n += 1;
    }
    (n, need as u32)
}

pub fn put(p: *mut u32, v: u32) {
    if !p.is_null() {
        unsafe { p.write_unaligned(v) };
    }
}

/// An enumeration's result: entries placed, total, bytes needed.
pub fn enum_result(recs: &[Rec], buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32) -> u32 {
    let (n, need) = pack(recs, buf, cb);
    put(returned, n);
    put(total, recs.len() as u32);
    put(needed, need);
    if (n as usize) < recs.len() { ERROR_MORE_DATA } else { 0 }
}

/// A query's result: the one record, or NERR_BufTooSmall.
pub fn query_result(r: Rec, buf: *mut u8, cb: u32, needed: *mut u32) -> u32 {
    let need = r.size() as u32;
    put(needed, need);
    if buf.is_null() || cb < need {
        return NERR_BUF_TOO_SMALL;
    }
    pack(core::slice::from_ref(&r), buf, cb);
    0
}

// ---- Handle tables ----

enum Slot<T> {
    Free,
    /// Taken out by `with` (not free for `add`).
    Busy,
    Used(T),
}

/// A per-process table of open objects (handles are indexes), safe to
/// use from several threads.
pub struct Table<T> {
    lock: core::sync::atomic::AtomicBool,
    items: core::cell::UnsafeCell<Vec<Slot<T>>>,
}

unsafe impl<T> Sync for Table<T> {}

impl<T> Table<T> {
    pub const fn new() -> Table<T> {
        Table { lock: core::sync::atomic::AtomicBool::new(false), items: core::cell::UnsafeCell::new(Vec::new()) }
    }

    fn locked<R>(&self, f: impl FnOnce(&mut Vec<Slot<T>>) -> R) -> R {
        use core::sync::atomic::Ordering;
        while self.lock.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            unsafe { os2::DosSleep(0) };
        }
        let r = f(unsafe { &mut *self.items.get() });
        self.lock.store(false, Ordering::Release);
        r
    }

    pub fn add(&self, x: T) -> usize {
        self.locked(|v| match v.iter().position(|s| matches!(s, Slot::Free)) {
            Some(i) => {
                v[i] = Slot::Used(x);
                i
            }
            None => {
                v.push(Slot::Used(x));
                v.len() - 1
            }
        })
    }

    /// Run `f` on an item (taken out meanwhile, so other threads wait
    /// for nothing and see it as busy), then put it back.
    pub fn with<R>(&self, i: usize, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut item = self.locked(|v| match v.get_mut(i) {
            Some(s @ Slot::Used(_)) => match core::mem::replace(s, Slot::Busy) {
                Slot::Used(x) => Some(x),
                _ => None,
            },
            _ => None,
        })?;
        let r = f(&mut item);
        self.locked(|v| v[i] = Slot::Used(item));
        Some(r)
    }

    pub fn take(&self, i: usize) -> Option<T> {
        self.locked(|v| match v.get_mut(i) {
            Some(s @ Slot::Used(_)) => match core::mem::replace(s, Slot::Free) {
                Slot::Used(x) => Some(x),
                _ => None,
            },
            _ => None,
        })
    }
}
