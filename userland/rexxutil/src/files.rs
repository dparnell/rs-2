//! Files, directories, drives and extended attributes.

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::null_mut;

use os2::*;

use crate::api::*;
use crate::rx::*;

const ATTR_FILES: u32 = 0x27;
const ATTR_ALL: u32 = 0x37;
const FIL_QUERYFULLNAME: u32 = 5;

/// SysFileTree option bits (IBM's).
const RECURSE: u32 = 2;
const DO_DIRS: u32 = 4;
const DO_FILES: u32 = 8;
const NAME_ONLY: u32 = 0x10;
const EDITABLE_TIME: u32 = 0x20;
const LONG_TIME: u32 = 0x40;
/// An attribute mask that matches anything / changes nothing.
const IGNORE: i32 = 2;

/// Walk a DosFindFirst/DosFindNext search; `f` returns false to stop
/// (the search then reports false too).
fn find(spec: &[u8], attr: u32, mut f: impl FnMut(&FileFindBuf3) -> bool) -> bool {
    let spec = cz(spec);
    let mut h = HDIR_CREATE;
    let mut count = 1u32;
    let mut buf: FileFindBuf3 = unsafe { core::mem::zeroed() };
    let sz = core::mem::size_of::<FileFindBuf3>() as u32;
    if unsafe { DosFindFirst(spec.as_ptr(), &mut h, attr, &mut buf as *mut _ as *mut u8, sz, &mut count, FIL_STANDARD) } != 0 {
        return true;
    }
    loop {
        if !f(&buf) {
            unsafe { DosFindClose(h) };
            return false;
        }
        count = 1;
        if unsafe { DosFindNext(h, &mut buf as *mut _ as *mut u8, sz, &mut count) } != 0 {
            break;
        }
    }
    unsafe { DosFindClose(h) };
    true
}

fn found_name(f: &FileFindBuf3) -> &[u8] {
    &f.name[..f.name.iter().position(|&c| c == 0).unwrap_or(f.name.len())]
}

/// FILESTATUS3 of a path.
fn path_info(path: &[u8]) -> Option<[u8; 24]> {
    let p = cz(path);
    let mut fs = [0u8; 24];
    if unsafe { DosQueryPathInfo(p.as_ptr(), FIL_STANDARD, fs.as_mut_ptr(), 24) } == 0 { Some(fs) } else { None }
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

/// The full name of a path (FIL_QUERYFULLNAME), empty if it fails.
fn full_name(path: &[u8]) -> Vec<u8> {
    let p = cz(path);
    let mut buf = [0u8; 260];
    if unsafe { DosQueryPathInfo(p.as_ptr(), FIL_QUERYFULLNAME, buf.as_mut_ptr(), 256) } != 0 {
        return Vec::new();
    }
    buf[..c_len(buf.as_ptr())].to_vec()
}

fn same_attr(mask: &[i32; 5], attr: u32) -> bool {
    if mask[0] == IGNORE {
        return true;
    }
    const BITS: [u32; 5] = [0x20, 0x10, 0x02, 0x01, 0x04];
    for (m, b) in mask.iter().zip(BITS) {
        if (*m < 0 && attr & b != 0) || (*m > 0 && attr & b == 0) {
            return false;
        }
    }
    true
}

fn new_attr(mask: &[i32; 5], mut attr: u32) -> u32 {
    if mask[0] == IGNORE {
        return attr;
    }
    const BITS: [u32; 5] = [0x20, 0x10, 0x02, 0x01, 0x04];
    for (m, b) in mask.iter().zip(BITS) {
        if *m < 0 {
            attr &= !b;
        }
        if *m > 0 {
            attr |= b;
        }
    }
    attr
}

fn set_file_mode(file: &[u8], attr: u32) -> u32 {
    let p = cz(file);
    let mut fs = [0u8; 24];
    let mut rc = unsafe { DosQueryPathInfo(p.as_ptr(), FIL_STANDARD, fs.as_mut_ptr(), 24) };
    if rc == 0 {
        fs[20..24].copy_from_slice(&attr.to_le_bytes());
        rc = unsafe { DosSetPathInfo(p.as_ptr(), FIL_STANDARD, fs.as_ptr(), 24, 0) };
    }
    rc
}

/// Split a file specification into the directory to search (a full
/// path ending in a backslash) and the name pattern, as IBM's getpath.
fn split_spec(spec: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut s: Vec<u8> = spec.to_vec();
    // Leading blanks belong to the name unless a path follows them.
    let k = s.iter().take_while(|&&c| c == b' ').count();
    if k > 0 {
        let l = s.len();
        let at = |i: usize| s.get(i).copied().unwrap_or(0);
        let c = at(k);
        let slash = |c: u8| c == b'\\' || c == b'/';
        if slash(c)
            || (c == b'.' && ((k < l && slash(at(k + 1))) || (k + 1 < l && at(k + 1) == b'.' && slash(at(k + 2)))))
            || (k < l && at(k + 1) == b':')
        {
            s.drain(..k);
        }
    }
    if s == b"." {
        s = b"*.*".to_vec();
    } else if s == b".." {
        s = b"..\\*.*".to_vec();
    }
    for c in s.iter_mut() {
        if *c == b'/' {
            *c = b'\\';
        }
    }
    let colon = s.get(1) == Some(&b':');
    let last = s.iter().rposition(|&c| c == b'\\');
    let (mut path, name_start) = match last {
        None if !colon => (full_name(b"."), 0),
        None => {
            let mut p = vec![s[0], b':', b'\\'];
            let mut buf = [0u8; 260];
            let mut cb = 256u32;
            let drive = (upper(s[0]) as i32 - 0x40) as u32;
            unsafe { DosQueryCurrentDir(drive, buf.as_mut_ptr(), &mut cb) };
            p.extend_from_slice(&buf[..c_len(buf.as_ptr())]);
            (p, 2)
        }
        Some(i) if !colon => {
            let mut p = full_name(&s[..i]);
            if p.is_empty() {
                p = s[..=i].to_vec();
            }
            (p, i + 1)
        }
        Some(i) => (s[..=i].to_vec(), i + 1),
    };
    if path.last() != Some(&b'\\') {
        path.push(b'\\');
    }
    let mut name = s[name_start..].to_vec();
    if name.is_empty() {
        name = b"*.*".to_vec();
    }
    (path, name)
}

struct Tree {
    stem: Vec<u8>,
    count: u32,
    smask: [i32; 5],
    dmask: [i32; 5],
    opts: u32,
}

impl Tree {
    /// One stem line for a file found (IBM's FormatFile).
    fn add(&mut self, full: &[u8], f: &FileFindBuf3) -> bool {
        let mut attr = new_attr(&self.dmask, f.attr);
        if f.attr != attr && set_file_mode(full, attr & !0x10) != 0 {
            attr = f.attr;
        }
        let line = if self.opts & NAME_ONLY != 0 {
            full.to_vec()
        } else {
            let (d, t) = (f.fdate_write as u32, f.ftime_write as u32);
            let (year, month, day) = ((d >> 9) as i64, ((d & 0x1ff) >> 5) as i64, (d & 0x1f) as i64);
            let (hour, min, sec) = ((t >> 11) as i64, ((t & 0x7ff) >> 5) as i64, ((t & 0x1f) * 2) as i64);
            let mut v = Vec::new();
            if self.opts & LONG_TIME != 0 {
                v.extend(dec_w(year + 1980, 4, false));
                for (sep, n) in [(b'-', month), (b'-', day), (b' ', hour), (b':', min), (b':', sec)] {
                    v.push(sep);
                    v.extend(dec_w(n, 2, true));
                }
            } else if self.opts & EDITABLE_TIME != 0 {
                v.extend(dec_w((year + 80) % 100, 2, true));
                for n in [month, day, hour, min] {
                    v.push(b'/');
                    v.extend(dec_w(n, 2, true));
                }
            } else {
                v.extend(dec_w(month, 2, false));
                v.push(b'/');
                v.extend(dec_w(day, 2, true));
                v.push(b'/');
                v.extend(dec_w((year + 80) % 100, 2, true));
                v.extend_from_slice(b"  ");
                v.extend(dec_w(if hour < 13 { hour } else { hour - 12 }, 2, false));
                v.push(b':');
                v.extend(dec_w(min, 2, true));
                v.push(if hour < 12 || hour == 24 { b'a' } else { b'p' });
            }
            v.extend_from_slice(b"  ");
            v.extend(dec_w(f.cb_file as i64, 10, false));
            v.extend_from_slice(b"  ");
            for (bit, ch) in [(0x20, b'A'), (0x10, b'D'), (0x02, b'H'), (0x01, b'R'), (0x04, b'S')] {
                v.push(if attr & bit != 0 { ch } else { b'-' });
            }
            v.extend_from_slice(b"  ");
            v.extend_from_slice(full);
            v
        };
        self.count += 1;
        let name = tail(&self.stem, &dec(self.count as i64));
        set_var(SHV_SET, &name, &line) & 0x18 == 0
    }

    /// IBM's RecursiveFindFile: files, then directories, then the
    /// subdirectories' contents.
    fn search(&mut self, path: &[u8], name: &[u8]) -> bool {
        let spec = [path, name].concat();
        let dot = |n: &[u8]| n == b"." || n == b"..";
        if self.opts & DO_FILES != 0 {
            let ok = find(&spec, ATTR_FILES, |f| {
                if same_attr(&self.smask, f.attr) {
                    let full = [path, found_name(f)].concat();
                    return self.add(&full, f);
                }
                true
            });
            if !ok {
                return false;
            }
        }
        if self.opts & DO_DIRS != 0 {
            let ok = find(&spec, ATTR_ALL, |f| {
                if f.attr & 0x10 != 0 && !dot(found_name(f)) && same_attr(&self.smask, f.attr) {
                    let full = [path, found_name(f)].concat();
                    return self.add(&full, f);
                }
                true
            });
            if !ok {
                return false;
            }
        }
        if self.opts & RECURSE != 0 {
            let all = [path, b"*"].concat();
            let mut dirs = Vec::new();
            find(&all, ATTR_ALL, |f| {
                if f.attr & 0x10 != 0 && !dot(found_name(f)) {
                    dirs.push([path, found_name(f), b"\\"].concat());
                }
                true
            });
            for d in dirs {
                if !self.search(&d, name) {
                    return false;
                }
            }
        }
        true
    }
}

/// Parse an attribute mask argument ("+-*" per attribute ADHRS).
fn mask(s: &[u8]) -> Option<[i32; 5]> {
    if s.len() > 5 {
        return None;
    }
    let mut m = [0i32; 5];
    for (i, c) in s.iter().enumerate() {
        m[i] = match c {
            b'+' => 1,
            b'-' => -1,
            b'*' => 0,
            _ => return None,
        };
    }
    Some(m)
}

/// SysFileTree(spec, stem [, options [, tattrib [, nattrib]]]).
pub fn sys_file_tree(a: &Args, r: &mut Ret) -> u32 {
    r.set(b"0");
    if a.n() < 2 || a.n() > 5 || !a.valid(0) || !a.valid(1) || a.len(0) > 255 {
        return INVALID;
    }
    let stem = stem_name(a.cstr(1));
    let mut spec = a.cstr(0).to_vec();
    if matches!(spec.last(), Some(b'\\') | Some(b'/')) {
        spec.extend_from_slice(b"*.*");
    }
    let mut opts = DO_FILES | DO_DIRS;
    if a.n() > 2 && a.exists(2) {
        if a.len(2) == 0 {
            return INVALID;
        }
        for &c in a.cstr(2) {
            match upper(c) {
                b'B' => opts |= DO_FILES | DO_DIRS,
                b'D' => opts = (opts & !DO_FILES) | DO_DIRS,
                b'F' => opts = (opts & !DO_DIRS) | DO_FILES,
                b'L' => opts |= LONG_TIME,
                b'O' => opts |= NAME_ONLY,
                b'S' => opts |= RECURSE,
                b'T' => opts |= EDITABLE_TIME,
                _ => return INVALID,
            }
        }
    }
    let mut smask = [IGNORE, 0, 0, 0, 0];
    let mut dmask = [IGNORE, 0, 0, 0, 0];
    if a.n() > 3 && a.exists(3) {
        match mask(a.cstr(3)) {
            Some(m) => smask = m,
            None => return INVALID,
        }
    }
    if a.n() == 5 {
        match mask(a.cstr(4)) {
            Some(m) => dmask = m,
            None => return INVALID,
        }
        dmask[1] = 0;
    }
    let (path, name) = split_spec(&spec);
    let mut t = Tree { stem, count: 0, smask, dmask, opts };
    if !t.search(&path, &name) {
        return INVALID;
    }
    if set_var(SHV_SET, &tail(&t.stem, b"0"), &dec(t.count as i64)) == SHV_BADN {
        return INVALID;
    }
    0
}

/// The lines of a file as IBM's GetLine returns them: split at line
/// feeds, a carriage return before one dropped, at most 4095 characters
/// (the rest of a longer line is lost, but a last line without a line
/// feed is split).
fn lines(data: &[u8]) -> Vec<&[u8]> {
    const MAX: usize = 4095;
    let mut v = Vec::new();
    let mut s = data;
    while !s.is_empty() {
        match s.iter().position(|&c| c == b'\n') {
            Some(i) => {
                let mut l = &s[..i.min(MAX)];
                if l.last() == Some(&b'\r') {
                    l = &l[..l.len() - 1];
                }
                v.push(l);
                s = &s[i + 1..];
            }
            None => {
                let n = s.len().min(MAX);
                let mut l = &s[..n];
                if l.last() == Some(&b'\r') {
                    l = &l[..l.len() - 1];
                }
                v.push(l);
                s = &s[n..];
            }
        }
    }
    v
}

/// Read a whole file (opened as IBM's OpenFile does): None if it cannot
/// be opened or read, or is empty.
fn read_file(name: &[u8]) -> Option<Vec<u8>> {
    let n = cz(name);
    let (mut h, mut act) = (0u32, 0u32);
    if unsafe { DosOpen(n.as_ptr(), &mut h, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, 0x6020, null_mut()) } != 0 {
        return None;
    }
    let mut fs = [0u8; 24];
    let size = if unsafe { DosQueryFileInfo(h, FIL_STANDARD, fs.as_mut_ptr(), 24) } == 0 {
        u32::from_le_bytes([fs[12], fs[13], fs[14], fs[15]])
    } else {
        0
    };
    if size == 0 {
        unsafe { DosClose(h) };
        return None;
    }
    let mut data = vec![0u8; size as usize];
    let mut got = 0u32;
    let rc = unsafe { DosRead(h, data.as_mut_ptr(), size, &mut got) };
    unsafe { DosClose(h) };
    if rc != 0 {
        return None;
    }
    data.truncate(got as usize);
    Some(data)
}

/// IBM's mystrstr: zeros become blanks, both sides upper-cased unless
/// the search is case sensitive.
fn contains(line: &[u8], target: &[u8], sensitive: bool) -> bool {
    let conv = |c: u8| if c == 0 { b' ' } else if sensitive { c } else { upper(c) };
    let l: Vec<u8> = line.iter().map(|&c| conv(c)).collect();
    let t: Vec<u8> = target.iter().map(|&c| conv(c)).collect();
    t.is_empty() || l.windows(t.len()).any(|w| w == &t[..])
}

/// SysFileSearch(target, file, stem [, options]).
pub fn sys_file_search(a: &Args, r: &mut Ret) -> u32 {
    r.set(b"0");
    if a.n() < 3 || a.n() > 4 || !a.valid(0) || !a.valid(1) || !a.valid(2) {
        return INVALID;
    }
    let (mut numbers, mut sensitive) = (false, false);
    if a.n() == 4 {
        let o = a.cstr(3);
        numbers = o.contains(&b'N') || o.contains(&b'n');
        sensitive = o.contains(&b'C') || o.contains(&b'c');
    }
    let stem = stem_name(a.cstr(2));
    let Some(data) = read_file(a.cstr(1)) else {
        r.set(b"3");
        return 0;
    };
    let target = a.bytes(0);
    let mut count = 0u32;
    for (i, line) in lines(&data).into_iter().enumerate() {
        let line = &line[..line.iter().position(|&c| c == 0).unwrap_or(line.len())];
        if !contains(line, target, sensitive) {
            continue;
        }
        let mut v = Vec::new();
        if numbers {
            v.extend(dec(i as i64 + 1));
            v.push(b' ');
        }
        v.extend_from_slice(line);
        count += 1;
        if set_var(SHV_SET, &tail(&stem, &dec(count as i64)), &v) == SHV_BADN {
            return INVALID;
        }
    }
    if set_var(SHV_SET, &tail(&stem, b"0"), &dec(count as i64)) == SHV_BADN {
        return INVALID;
    }
    0
}

/// SysFileDelete(file): DosDelete's return code.
pub fn sys_file_delete(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let n = cz(a.cstr(0));
    r.num(unsafe { DosDelete(n.as_ptr()) } as i32);
    0
}

/// SysMkDir(dir): DosCreateDir's return code.
pub fn sys_mkdir(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let n = cz(a.cstr(0));
    r.num(unsafe { DosCreateDir(n.as_ptr(), null_mut()) } as i32);
    0
}

/// SysRmDir(dir): DosDeleteDir's return code.
pub fn sys_rmdir(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let n = cz(a.cstr(0));
    r.num(unsafe { DosDeleteDir(n.as_ptr()) } as i32);
    0
}

/// SysSearchPath(var, file [, 'N'|'C']): the full name or "".
pub fn sys_search_path(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 3 || !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let mut flags = 7;
    if a.n() == 3 {
        match a.first(2) {
            b'N' | b'n' => flags = 6,
            b'C' | b'c' => flags = 7,
            _ => return INVALID,
        }
    }
    let var = cz(a.cstr(0));
    let file = cz(a.cstr(1));
    let mut buf = [0u8; 260];
    let rc = unsafe { DosSearchPath(flags, var.as_ptr(), file.as_ptr(), buf.as_mut_ptr(), 256) };
    if rc == 203 {
        // ERROR_ENVVAR_NOT_FOUND: try the variable's name in capitals.
        let var: Vec<u8> = var.iter().map(|&c| upper(c)).collect();
        unsafe { DosSearchPath(flags, var.as_ptr(), file.as_ptr(), buf.as_mut_ptr(), 256) };
    }
    r.set(&buf[..c_len(buf.as_ptr())]);
    0
}

/// IBM's GetUniqueFileName.
fn unique_name(template: &[u8], filler: u8) -> Vec<u8> {
    let j = template.iter().filter(|&&c| c == filler).count();
    if j == 0 || j > 5 {
        return Vec::new();
    }
    let max = 10u32.pow(j as u32);
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    let seed = ((dt.hours as u32 * 60 + dt.minutes as u32) * 60 + dt.seconds as u32) * 100 + dt.hundredths as u32;
    let start = seed.wrapping_mul(0x19660d).wrapping_add(1) % max;
    let mut num = start;
    loop {
        let digits = dec_w(num as i64, j, true);
        let mut k = 0;
        let file: Vec<u8> = template
            .iter()
            .map(|&c| {
                if c == filler {
                    k += 1;
                    digits[k - 1]
                } else {
                    c
                }
            })
            .collect();
        let fz = cz(&file);
        let mut h = HDIR_CREATE;
        let mut count = 1u32;
        let mut buf: FileFindBuf3 = unsafe { core::mem::zeroed() };
        unsafe { DosError(0) };
        let rc = unsafe {
            DosFindFirst(fz.as_ptr(), &mut h, ATTR_ALL, &mut buf as *mut _ as *mut u8, core::mem::size_of::<FileFindBuf3>() as u32, &mut count, FIL_STANDARD)
        };
        let result = match rc {
            2 | 18 => Some(file),
            0 => None,
            _ => Some(Vec::new()),
        };
        if rc == 0 {
            unsafe { DosFindClose(h) };
        }
        unsafe { DosError(1) };
        if let Some(f) = result {
            return f;
        }
        num = (num + 1) % max;
        if num == start {
            return Vec::new();
        }
    }
}

/// SysTempFileName(template [, filler]).
pub fn sys_temp_file_name(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 2 || !a.valid(0) || a.len(0) > 512 {
        return INVALID;
    }
    let mut filler = b'?';
    if a.n() == 2 && a.exists(1) {
        if a.len(1) != 1 {
            return INVALID;
        }
        filler = a.first(1);
    }
    let f = unique_name(a.cstr(0), filler);
    r.set(&f);
    0
}

/// SysDriveInfo(drive): "d:  free total label" or "".
pub fn sys_drive_info(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 || a.len(0) > 2 || a.len(0) == 0 {
        return INVALID;
    }
    let s = a.cstr(0);
    if s.len() == 2 && s[1] != b':' {
        return INVALID;
    }
    let drive = upper(a.first(0)) as i8 as i32 - 0x40;
    if drive == 0 || drive > 26 {
        return INVALID;
    }
    unsafe { DosError(0) };
    let mut info = [0u8; 256];
    let rc = unsafe { DosQueryFSInfo(drive as u32, 2, info.as_mut_ptr(), 256) };
    let mut alloc = [0u8; 20];
    unsafe { DosQueryFSInfo(drive as u32, 1, alloc.as_mut_ptr(), 20) };
    if rc == 0 || rc == 125 {
        let w = |o: usize| u32::from_le_bytes([alloc[o], alloc[o + 1], alloc[o + 2], alloc[o + 3]]);
        let (sec, units, avail, bps) = (w(4), w(8), w(12), u16_at(&alloc, 16) as u32);
        let label = &info[5..5 + c_len(info[5..].as_ptr()).min(250)];
        let mut v = vec![(drive + 0x40) as u8, b':', b' ', b' '];
        if avail.wrapping_mul(sec) < 0x200001 && sec.wrapping_mul(units) < 0x200001 {
            v.extend(left_w(&dec(sec.wrapping_mul(avail).wrapping_mul(bps) as i64), 12));
            v.push(b' ');
            v.extend(left_w(&dec(units.wrapping_mul(sec).wrapping_mul(bps) as i64), 12));
        } else {
            let lo_f = sec.wrapping_mul(avail % 1000).wrapping_mul(bps);
            let hi_f = sec.wrapping_mul(bps).wrapping_mul(avail / 1000).wrapping_add(lo_f / 1000);
            let lo_t = (units % 1000).wrapping_mul(sec).wrapping_mul(bps);
            let hi_t = (lo_t / 1000).wrapping_add((units / 1000).wrapping_mul(sec).wrapping_mul(bps));
            v.extend(dec_w(hi_f as i64, 9, false));
            v.extend(dec_w((lo_f % 1000) as i64, 3, true));
            v.push(b' ');
            v.extend(dec_w(hi_t as i64, 9, false));
            v.extend(dec_w((lo_t % 1000) as i64, 3, true));
        }
        v.push(b' ');
        v.extend(left_w(label, 13));
        r.set(&v);
    } else {
        r.set(b"");
    }
    unsafe { DosError(1) };
    0
}

/// SysDriveMap([drive] [, 'FREE'|'USED'|'DETACHED'|'REMOTE'|'LOCAL']).
pub fn sys_drive_map(a: &Args, r: &mut Ret) -> u32 {
    if a.n() > 2 {
        return INVALID;
    }
    let mut start = 3i32;
    if a.n() >= 1 && a.exists(0) {
        let s = a.cstr(0);
        if (s.len() == 2 && s[1] != b':') || s.len() >= 3 || s.is_empty() {
            return INVALID;
        }
        start = upper(s[0]) as i8 as i32 - 0x40;
    }
    if start == 0 || start > 26 {
        return INVALID;
    }
    // 0 used, 1 free, 2 detached, 3 remote, 4 local
    let mut mode = 0;
    if a.n() == 2 && a.len(1) != 0 {
        let m = a.cstr(1);
        mode = if eqi(m, b"FREE") {
            1
        } else if eqi(m, b"USED") {
            0
        } else if eqi(m, b"DETACHED") {
            2
        } else if eqi(m, b"REMOTE") {
            3
        } else if eqi(m, b"LOCAL") {
            4
        } else {
            return INVALID;
        };
    }
    unsafe { DosError(0) };
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut cur, &mut map) };
    map >>= (start - 1) as u32 & 31;
    let mut out = Vec::new();
    for d in start as u32..27 {
        let letter = (d + 0x40) as u8;
        let used = map & 1 != 0;
        let add = if !used {
            mode == 1
        } else if mode == 0 {
            true
        } else if mode == 1 {
            false
        } else {
            let dev = [letter, b':', 0];
            let mut fsq = [0u8; 12];
            let mut cb = 12u32;
            unsafe { DosQueryFSAttach(dev.as_ptr(), 0, 1, fsq.as_mut_ptr(), &mut cb) };
            let ty = u16_at(&fsq, 0);
            let mut buf = [0u8; 256];
            let rc = unsafe { DosQueryFSInfo(d, 2, buf.as_mut_ptr(), 256) };
            (rc == 67 && ty == 4 && mode == 2) || (ty == 4 && mode == 3) || (ty == 3 && mode == 4)
        };
        if add {
            out.extend_from_slice(&[letter, b':', b' ']);
        }
        map >>= 1;
    }
    out.pop();
    r.set(&out);
    unsafe { DosError(1) };
    0
}

/// SysFileSystemType(drive): the file system's name or "".
pub fn sys_file_system_type(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 || a.len(0) > 2 || a.len(0) == 0 {
        return INVALID;
    }
    let s = a.cstr(0);
    if s.len() == 2 && s[1] != b':' {
        return INVALID;
    }
    let dev = [a.first(0), b':', 0];
    let mut buf = [0u8; 260];
    let mut cb = 256u32;
    unsafe { DosError(0) };
    let rc = unsafe { DosQueryFSAttach(dev.as_ptr(), 1, 1, buf.as_mut_ptr(), &mut cb) };
    unsafe { DosError(1) };
    if rc == 0 {
        let o = 9 + u16_at(&buf, 2) as usize;
        let o = o.min(259);
        r.set(&buf[o..o + c_len(buf[o..].as_ptr())]);
    } else {
        r.set(b"");
    }
    0
}

/// SysWildCard(source, wildcard): DosEditName's result or "".
pub fn sys_wild_card(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(0) {
        return INVALID;
    }
    let src = cz(a.cstr(0));
    let wild = cz(a.cstr(1));
    let mut buf = [0u8; 260];
    let cb = r.cap().min(256) as u32;
    if unsafe { DosEditName(1, src.as_ptr(), wild.as_ptr(), buf.as_mut_ptr(), cb) } == 0 {
        r.set(&buf[..c_len(buf.as_ptr())]);
    } else {
        r.set(b"");
    }
    0
}

/// SysGetEA(file, name, var): DosQueryPathInfo's return code; the value
/// goes to the variable.
pub fn sys_get_ea(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 3 || !a.valid(0) || !a.valid(1) || !a.valid(2) {
        return INVALID;
    }
    let var: Vec<u8> = a.cstr(2).iter().map(|&c| upper(c)).collect();
    let mut fea: *mut u8 = null_mut();
    if unsafe { DosAllocMem(&mut fea, 0x10000, PAG_COMMIT | PAG_WRITE) } != 0 {
        r.set(b"2");
        return 0;
    }
    // GEA2LIST: cbList, then one GEA2 (oNextEntryOffset, cbName, szName).
    let name = a.cstr(1);
    let mut gea = vec![0u8; name.len() + 16];
    gea[0..4].copy_from_slice(&((a.len(1) + 10) as u32).to_le_bytes());
    gea[8] = a.len(1) as u8;
    gea[9..9 + name.len()].copy_from_slice(name);
    unsafe { (fea as *mut u32).write(0xffff) };
    let eaop: [u32; 3] = [gea.as_ptr() as u32, fea as u32, 0];
    let file = cz(a.cstr(0));
    let mut rc = unsafe { DosQueryPathInfo(file.as_ptr(), 3, eaop.as_ptr() as *mut u8, 12) };
    if unsafe { (fea as *const u32).read() } < 5 {
        rc = 282;
    }
    r.num(rc as i32);
    if rc == 0 {
        let p = unsafe { core::slice::from_raw_parts(fea, 0x10000) };
        let cb_name = p[9] as i8 as isize;
        let cb_value = u16_at(p, 10) as usize;
        let off = (13 + cb_name) as usize;
        if set_var(SHV_SET, &var, &p[off..off + cb_value]) == SHV_BADN {
            unsafe { DosFreeMem(fea) };
            return INVALID;
        }
    }
    unsafe { DosFreeMem(fea) };
    0
}

/// SysPutEA(file, name, value): DosSetPathInfo's return code.
pub fn sys_put_ea(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 3 || !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let mut p: *mut u8 = null_mut();
    if unsafe { DosAllocMem(&mut p, 0x10000, PAG_COMMIT | PAG_WRITE) } != 0 {
        r.set(b"2");
        return 0;
    }
    let buf = unsafe { core::slice::from_raw_parts_mut(p, 0x10000) };
    // FEA2LIST: cbList, then one FEA2 (oNextEntryOffset, fEA, cbName,
    // cbValue, szName, value).
    let name = a.cstr(1);
    let value = a.bytes(2);
    let cb_name = a.len(1) as u8;
    let cb_value = a.len(2) as u16;
    buf[8] = 0;
    buf[9] = cb_name;
    buf[10..12].copy_from_slice(&cb_value.to_le_bytes());
    let n = name.len().min(255);
    buf[12..12 + n].copy_from_slice(&name[..n]);
    buf[12 + n] = 0;
    let off = 4 + cb_name as i8 as isize as usize + 9;
    let vn = value.len().min(0x10000 - off - 1);
    buf[off..off + vn].copy_from_slice(&value[..vn]);
    buf[4..8].copy_from_slice(&0u32.to_le_bytes());
    let cb_list = cb_value as u32 + cb_name as i8 as i32 as u32 + 0xd;
    buf[0..4].copy_from_slice(&cb_list.to_le_bytes());
    let eaop: [u32; 3] = [0, p as u32, 0];
    let file = cz(a.cstr(0));
    let rc = unsafe { DosSetPathInfo(file.as_ptr(), 2, eaop.as_ptr() as *const u8, 12, 0) };
    unsafe { DosFreeMem(p) };
    r.num(rc as i32);
    0
}

/// SysQueryEAList(file, stem): the names of a file's extended
/// attributes; returns the API's return code.
pub fn sys_query_ea_list(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(0) {
        return INVALID;
    }
    let stem = stem_name(a.cstr(1));
    let mut buf: *mut u8 = null_mut();
    let mut rc = unsafe { DosAllocMem(&mut buf, 0x10000, PAG_COMMIT | PAG_WRITE) };
    if rc != 0 {
        r.num(rc as i32);
        return 0;
    }
    let file = cz(a.cstr(0));
    let mut count = u32::MAX;
    let mut fs = [0u8; 24];
    rc = unsafe { DosQueryPathInfo(file.as_ptr(), FIL_STANDARD, fs.as_mut_ptr(), 24) };
    let dir = fs[20] & 0x10 != 0;
    if rc == 0 {
        // IBM's checks the file opens (unless it is a directory).
        let mut h = 0u32;
        let mut act = 0u32;
        if !dir {
            rc = unsafe { DosOpen(file.as_ptr(), &mut h, &mut act, 0, 0, 1, 0x2020, null_mut()) };
        }
        if rc == 0 {
            rc = unsafe { DosEnumAttribute(1, file.as_ptr(), 1, buf, 0x10000, &mut count, 1) };
            if !dir {
                unsafe { DosClose(h) };
            }
        }
    }
    if rc != 0 {
        count = 0;
    }
    let p = unsafe { core::slice::from_raw_parts(buf, 0x10000) };
    let mut off = 0usize;
    for i in 0..count {
        let cb_name = p[off + 5] as i8 as isize as usize;
        let name = &p[off + 8..off + 8 + cb_name.min(255)];
        if set_var(SHV_SET, &tail(&stem, &dec(i as i64 + 1)), name) == SHV_BADN {
            unsafe { DosFreeMem(buf) };
            return INVALID;
        }
        let next = u32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]) as usize;
        off += next;
    }
    set_var(SHV_SET, &tail(&stem, b"0"), &dec(count as i64));
    unsafe { DosFreeMem(buf) };
    r.num(rc as i32);
    0
}

/// The date and time fields of FILESTATUS3 picked by 'C', 'A' or 'W'.
fn stamp_offset(c: u8) -> Option<usize> {
    match c {
        b'C' | b'c' => Some(0),
        b'A' | b'a' => Some(4),
        b'W' | b'w' => Some(8),
        _ => None,
    }
}

/// SysGetFileDateTime(file [, 'C'|'A'|'W']): "yyyy-mm-dd hh:mm:ss" or -1.
pub fn sys_get_file_date_time(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 2 || (a.n() == 2 && !a.valid(1)) {
        return INVALID;
    }
    let off = if a.n() < 2 {
        8
    } else {
        match stamp_offset(a.first(1)) {
            Some(o) => o,
            None => return INVALID,
        }
    };
    match path_info(a.cstr(0)) {
        Some(fs) => {
            let (d, t) = (u16_at(&fs, off) as i64, u16_at(&fs, off + 2) as i64);
            let mut v = dec_w((d >> 9) + 1980, 4, false);
            for (sep, n) in [(b'-', (d & 0x1ff) >> 5), (b'-', d & 0x1f), (b' ', t >> 11), (b':', (t & 0x7ff) >> 5), (b':', (t & 0x1f) * 2)] {
                v.push(sep);
                v.extend(dec_w(n, 2, true));
            }
            r.set(&v);
        }
        None => {
            r.set(b"-1");
        }
    }
    0
}

/// sscanf of up to three unsigned fields of at most `w` digits each,
/// separated by `sep`: the number of fields converted.
fn scan3(s: &[u8], widths: [usize; 3], sep: u8, out: &mut [u32; 3]) -> usize {
    let mut i = 0;
    for f in 0..3 {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        let mut v = 0u32;
        while i < s.len() && i - start < widths[f] && s[i].is_ascii_digit() {
            v = v.wrapping_mul(10).wrapping_add((s[i] - b'0') as u32);
            i += 1;
        }
        if i == start {
            return f;
        }
        out[f] = v;
        if f < 2 {
            if i < s.len() && s[i] == sep {
                i += 1;
            } else {
                return f + 1;
            }
        }
    }
    3
}

/// SysSetFileDateTime(file [, date] [, time]): 0 or -1.
pub fn sys_set_file_date_time(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 3 || (a.n() == 2 && !a.valid(1)) || (a.n() == 3 && !a.valid(2)) {
        return INVALID;
    }
    let mut ok = true;
    match path_info(a.cstr(0)) {
        Some(mut fs) => {
            let (d, t) = (u16_at(&fs, 8) as u32, u16_at(&fs, 10) as u32);
            let mut date = [(d >> 9) + 1980, (d & 0x1ff) >> 5, d & 0x1f];
            let mut time = [t >> 11, (t & 0x7ff) >> 5, (t & 0x1f) * 2];
            if a.n() > 1 && a.valid(1) {
                if scan3(a.cstr(1), [4, 2, 2], b'-', &mut date) != 3 || date[0] < 1980 {
                    ok = false;
                }
            }
            if a.n() == 3 && a.valid(2) && scan3(a.cstr(2), [2, 2, 2], b':', &mut time) != 3 {
                ok = false;
            }
            if a.n() == 1 {
                let mut dt = DateTime::default();
                if unsafe { DosGetDateTime(&mut dt) } == 0 {
                    date = [dt.year as u32, dt.month as u32, dt.day as u32];
                    time = [dt.hours as u32, dt.minutes as u32, dt.seconds as u32];
                } else {
                    ok = false;
                }
            }
            if ok {
                let fd = (((date[0].wrapping_add(68)) & 0x7f) << 9) | ((date[1] << 5) & 0x1ff) | (date[2] & 0x1f);
                let ft = ((time[0] << 11) & 0xf800) | ((time[1] << 5) & 0x7ff) | ((time[2] >> 1) & 0x1f);
                fs[8..10].copy_from_slice(&(fd as u16).to_le_bytes());
                fs[10..12].copy_from_slice(&(ft as u16).to_le_bytes());
                let p = cz(a.cstr(0));
                if unsafe { DosSetPathInfo(p.as_ptr(), FIL_STANDARD, fs.as_ptr(), 24, 0x10) } != 0 {
                    ok = false;
                }
            }
        }
        None => ok = false,
    }
    r.ok_or_minus1(ok);
    0
}
