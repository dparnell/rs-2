//! MMPMMMIO.INI: the file-format I/O procedures installed in the system,
//! and mmioIniFileHandler.
//!
//! As in IBM's MMIO (studied in Ghidra: mmioIniFileHandler,
//! mmiohlpLoadDefIOProcs), the profile `%MMBASE%\MMPMMMIO.INI` holds
//! application `MMIOINIT` with `IOCOUNT` (a USHORT) and `IOMODINFO`, an
//! array of MMINIFILEINFO records (316 bytes: FOURCC, DLL name, entry
//! point name, flags, extended length, media type, procedure type,
//! default extension). Each process reads the list when MMIO first needs
//! it; the procedures load on first use (DosLoadModule, DosQueryProcAddr),
//! in the list's order after the ones the process installed itself.
//! Without the profile (or with no IOCOUNT) the list is rs2's own
//! procedures (`ioprocs.txt`).

use alloc::vec::Vec;

use os2::*;

use crate::*;

pub const RECORD: usize = 316;
const OFF_DLL: usize = 4;
const OFF_PROC: usize = 264;
const OFF_FLAGS: usize = 296;
const OFF_EXTLEN: usize = 300;
const OFF_MEDIA: usize = 304;
const OFF_TYPE: usize = 308;
const OFF_EXT: usize = 312;

const MMIO_INSTALLPROC: u32 = 1;
const MMIO_REMOVEPROC: u32 = 2;
const MMIO_FINDPROC: u32 = 4;
const MMIO_MATCHFIRST: u32 = 0x10;
const MMIO_MATCHNEXT: u32 = 0x20;
const MMIO_MATCHFOURCC: u32 = 0x40;
const MMIO_MATCHDLL: u32 = 0x80;
const MMIO_MATCHPROCEDURENAME: u32 = 0x100;
const MMIO_FULLPATH: u32 = 0x200;
const MMIO_NOVERIFY: u32 = 0x400;
const MMIO_EXTENDED_STRUCT: u32 = 0x1000;

const INI_OPEN: u32 = 6508;
const LOADLIBRARY_FAILED: u32 = 6535;
const INVALID_PROCEDURENAME: u32 = 6536;
const MATCH_NOT_FOUND: u32 = 6537;

/// One installed procedure: its record and, once loaded, its address.
#[derive(Clone)]
pub struct Entry {
    pub rec: [u8; RECORD],
    pub proc_: Option<usize>,
}

impl Entry {
    pub fn fcc(&self) -> u32 {
        u32::from_le_bytes(self.rec[0..4].try_into().unwrap())
    }

    pub fn media(&self) -> u32 {
        u32::from_le_bytes(self.rec[OFF_MEDIA..OFF_MEDIA + 4].try_into().unwrap())
    }

    fn str_at(&self, off: usize, len: usize) -> &[u8] {
        let s = &self.rec[off..off + len];
        &s[..s.iter().position(|&c| c == 0).unwrap_or(len)]
    }

    pub fn dll(&self) -> &[u8] {
        self.str_at(OFF_DLL, 260)
    }

    pub fn proc_name(&self) -> &[u8] {
        self.str_at(OFF_PROC, 32)
    }
}

fn put(rec: &mut [u8; RECORD], off: usize, len: usize, s: &[u8]) {
    let n = s.len().min(len - 1);
    rec[off..off + len].fill(0);
    rec[off..off + n].copy_from_slice(&s[..n]);
}

fn put32(rec: &mut [u8; RECORD], off: usize, v: u32) {
    rec[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// rs2's own procedures (ioprocs.txt), as records.
pub fn defaults() -> Vec<[u8; RECORD]> {
    let mut v = Vec::new();
    for line in include_str!("../ioprocs.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        if line.starts_with('#') || w.len() < 6 {
            continue;
        }
        let mut rec = [0u8; RECORD];
        let fcc: Vec<u8> = w[0].bytes().map(|b| if b == b'_' { b' ' } else { b }).collect();
        rec[0..4].copy_from_slice(&fcc[..4]);
        put(&mut rec, OFF_DLL, 260, w[1].as_bytes());
        put(&mut rec, OFF_PROC, 32, w[2].as_bytes());
        put32(&mut rec, OFF_FLAGS, w[3].parse().unwrap_or(0));
        put32(&mut rec, OFF_EXTLEN, 16);
        put32(&mut rec, OFF_MEDIA, w[4].parse().unwrap_or(0));
        put32(&mut rec, OFF_TYPE, 2);
        put(&mut rec, OFF_EXT, 4, w[5].as_bytes());
        v.push(rec);
    }
    v
}

/// `%MMBASE%\MMPMMMIO.INI` (the first directory of MMBASE), else
/// `\MMOS2\MMPMMMIO.INI` on the boot drive.
fn ini_path() -> Vec<u8> {
    let mut p: *const u8 = core::ptr::null();
    let mut out = Vec::new();
    if unsafe { DosScanEnv(b"MMBASE\0".as_ptr(), &mut p) } == 0 && !p.is_null() {
        let s = unsafe { cstr(p) }.unwrap_or(b"");
        let first = s.split(|&c| c == b';').next().unwrap_or(b"");
        out.extend_from_slice(first);
    }
    if out.is_empty() {
        let mut drive = 3u32;
        unsafe { DosQuerySysInfo(5, 5, &mut drive, 4) };
        out.extend_from_slice(&[b'@' + drive as u8, b':', b'\\', b'M', b'M', b'O', b'S', b'2']);
    }
    if !out.ends_with(b"\\") && !out.ends_with(b"/") {
        out.push(b'\\');
    }
    out.extend_from_slice(b"MMPMMMIO.INI\0");
    out
}

fn read_file(name: &[u8]) -> Option<Vec<u8>> {
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS, OPEN_SHARE_DENYWRITE | OPEN_ACCESS_READONLY.
    if unsafe { DosOpen(name.as_ptr(), &mut h, &mut act, 0, 0, 1, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 16384];
    loop {
        let mut n = 0;
        if unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(h) };
    Some(out)
}

fn write_file(name: &[u8], data: &[u8]) -> bool {
    let (mut h, mut act) = (0u32, 0u32);
    // Create or replace, OPEN_SHARE_DENYREADWRITE | OPEN_ACCESS_WRITEONLY.
    if unsafe { DosOpen(name.as_ptr(), &mut h, &mut act, data.len() as u32, 0, 0x12, 0x11, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut n = 0;
    let ok = unsafe { DosWrite(h, data.as_ptr(), data.len() as u32, &mut n) } == 0 && n as usize == data.len();
    unsafe { DosClose(h) };
    ok
}

/// The records in the profile: None when it has no list.
fn read_records() -> Option<Vec<[u8; RECORD]>> {
    let file = read_file(&ini_path())?;
    let prof = inifile::parse(&file).profile;
    let count = prof.get(b"MMIOINIT", b"IOCOUNT")?;
    let count = u16::from_le_bytes([*count.first()?, *count.get(1).unwrap_or(&0)]) as usize;
    let data = prof.get(b"MMIOINIT", b"IOMODINFO").unwrap_or(&[]);
    Some(data.chunks_exact(RECORD).take(count).map(|c| c.try_into().unwrap()).collect())
}

fn write_records(recs: &[[u8; RECORD]]) -> bool {
    let path = ini_path();
    let mut prof = read_file(&path).map(|f| inifile::parse(&f).profile).unwrap_or_default();
    let data: Vec<u8> = recs.iter().flat_map(|r| r.iter().copied()).collect();
    let ok = prof.set(b"MMIOINIT", b"IOCOUNT", &(recs.len() as u16).to_le_bytes()).is_ok()
        && prof.set(b"MMIOINIT", b"IOMODINFO", &data).is_ok()
        && prof.set(b"MMIOINIT", b"VERSION", b"MULTIMEDIA MMIO INI FILE VERSION 1.0\0").is_ok();
    ok && write_file(&path, &prof.to_bytes())
}

/// The process's copy of the list (None until first read).
static LIST: Locked<Option<Vec<Entry>>> = Locked::new(None);

fn records() -> Vec<[u8; RECORD]> {
    read_records().unwrap_or_else(defaults)
}

fn with_list<R>(f: impl FnOnce(&mut Vec<Entry>) -> R) -> R {
    LIST.with(|l| {
        let v = l.get_or_insert_with(|| records().into_iter().map(|rec| Entry { rec, proc_: None }).collect());
        f(v)
    })
}

/// Load an entry's procedure (DosLoadModule; the module handle is kept).
fn load(e: &Entry) -> Option<usize> {
    let mut dll = e.dll().to_vec();
    dll.push(0);
    let mut name = e.proc_name().to_vec();
    name.push(0);
    let mut fail = [0u8; 64];
    let mut hmod = 0u32;
    if unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, dll.as_ptr(), &mut hmod) } != 0 {
        return None;
    }
    let mut pfn = 0usize;
    if unsafe { DosQueryProcAddr(hmod, 0, name.as_ptr(), &mut pfn) } != 0 || pfn == 0 {
        return None;
    }
    Some(pfn)
}

/// The installed (INI) procedures: (FOURCC, media type), in order.
pub fn list() -> Vec<(u32, u32)> {
    with_list(|l| l.iter().map(|e| (e.fcc(), e.media())).collect())
}

/// The procedure for `fcc` from the INI list, loading it if need be.
pub fn find(fcc: u32) -> Option<usize> {
    let e = with_list(|l| l.iter().find(|e| e.fcc() == fcc).cloned())?;
    if let Some(p) = e.proc_ {
        return Some(p);
    }
    let p = load(&e)?;
    with_list(|l| {
        for x in l.iter_mut().filter(|x| x.fcc() == fcc) {
            x.proc_ = Some(p);
        }
    });
    Some(p)
}

fn eq_nocase(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// Whether a record matches the caller's (the MATCH flags).
fn matches(rec: &[u8; RECORD], want: &[u8; RECORD], flags: u32) -> bool {
    let e = Entry { rec: *rec, proc_: None };
    let w = Entry { rec: *want, proc_: None };
    if flags & MMIO_MATCHDLL != 0 && !eq_nocase(e.dll(), w.dll()) {
        return false;
    }
    if flags & MMIO_MATCHPROCEDURENAME != 0 && e.proc_name() != w.proc_name() {
        return false;
    }
    if flags & MMIO_MATCHFOURCC != 0 && e.fcc() != w.fcc() {
        return false;
    }
    if flags & (MMIO_MATCHDLL | MMIO_MATCHPROCEDURENAME | MMIO_MATCHFOURCC) == 0 && flags & MMIO_MATCHFIRST == 0 {
        // No criterion: the FOURCC.
        return e.fcc() == w.fcc();
    }
    true
}

/// The module name of a DLL path ("C:\MMOS2\DLL\JPEGIO.DLL" -> "JPEGIO"),
/// as IBM's handler keeps it unless MMIO_FULLPATH.
fn module_name(path: &[u8]) -> Vec<u8> {
    let base = path.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(path);
    base.split(|&c| c == b'.').next().unwrap_or(base).to_vec()
}

/// mmioIniFileHandler(pmminifileinfo, ulFlags).
#[unsafe(no_mangle)]
pub extern "C" fn mmioIniFileHandler(info: *mut [u8; RECORD], flags: u32) -> u32 {
    let Some(info) = (unsafe { info.as_mut() }) else { return err::INVALID_PARAMETER };
    let ops = flags & (MMIO_INSTALLPROC | MMIO_REMOVEPROC | MMIO_FINDPROC);
    if ops.count_ones() != 1
        || flags & (MMIO_MATCHFIRST | MMIO_MATCHNEXT) == MMIO_MATCHFIRST | MMIO_MATCHNEXT
        || (ops != MMIO_FINDPROC && flags & (MMIO_MATCHFIRST | MMIO_MATCHNEXT) != 0)
        || (flags & (MMIO_FULLPATH | MMIO_NOVERIFY) != 0 && ops != MMIO_INSTALLPROC)
    {
        return err::INVALID_PARAMETER;
    }
    if flags & MMIO_EXTENDED_STRUCT != 0 && u32::from_le_bytes(info[OFF_EXTLEN..OFF_EXTLEN + 4].try_into().unwrap()) != 16 {
        return err::INVALID_PARAMETER;
    }
    let mut recs = records();
    match ops {
        MMIO_INSTALLPROC => {
            let mut rec = *info;
            let e = Entry { rec, proc_: None };
            if e.fcc() == 0 || e.dll().is_empty() || e.proc_name().is_empty() {
                return err::INVALID_PARAMETER;
            }
            if flags & MMIO_NOVERIFY == 0 {
                let mut dll = e.dll().to_vec();
                dll.push(0);
                let mut name = e.proc_name().to_vec();
                name.push(0);
                let (mut fail, mut hmod, mut pfn) = ([0u8; 64], 0u32, 0usize);
                if unsafe { DosLoadModule(fail.as_mut_ptr(), 64, dll.as_ptr(), &mut hmod) } != 0 {
                    return LOADLIBRARY_FAILED;
                }
                if unsafe { DosQueryProcAddr(hmod, 0, name.as_ptr(), &mut pfn) } != 0 {
                    return INVALID_PROCEDURENAME;
                }
                // The procedure describes itself.
                let mut fi = crate::procs::format_info(FOURCC_DOS).unwrap();
                let f: IoProc = unsafe { core::mem::transmute(pfn) };
                if unsafe { f(core::ptr::null_mut(), MMIOM_GETFORMATINFO, &mut fi as *mut _ as i32, 0) } == 0 {
                    put32(&mut rec, OFF_MEDIA, fi.ulMediaType);
                    put32(&mut rec, OFF_FLAGS, fi.ulFlags);
                    put32(&mut rec, OFF_TYPE, fi.ulIOProcType);
                }
            }
            if flags & MMIO_FULLPATH == 0 {
                let m = module_name(e.dll());
                put(&mut rec, OFF_DLL, 260, &m);
            }
            put32(&mut rec, OFF_EXTLEN, 16);
            // A procedure for the same FOURCC is replaced in place.
            match recs.iter().position(|r| r[0..4] == rec[0..4]) {
                Some(i) => recs[i] = rec,
                None => recs.push(rec),
            }
        }
        MMIO_REMOVEPROC => {
            let crit = flags | if flags & (MMIO_MATCHDLL | MMIO_MATCHPROCEDURENAME | MMIO_MATCHFOURCC) == 0 { MMIO_MATCHFOURCC } else { 0 };
            match recs.iter().position(|r| matches(r, info, crit)) {
                Some(i) => {
                    recs.remove(i);
                }
                None => return MATCH_NOT_FOUND,
            }
        }
        _ => {
            // FINDPROC: MATCHFIRST takes the first record that matches,
            // MATCHNEXT the first after the caller's (by FOURCC).
            let start = if flags & MMIO_MATCHNEXT != 0 {
                match recs.iter().position(|r| r[0..4] == info[0..4]) {
                    Some(i) => i + 1,
                    None => return MATCH_NOT_FOUND,
                }
            } else {
                0
            };
            let crit = flags & !MMIO_MATCHNEXT | if flags & MMIO_MATCHNEXT != 0 { MMIO_MATCHFIRST } else { 0 };
            return match recs[start..].iter().find(|r| matches(r, info, crit)) {
                Some(r) => {
                    *info = *r;
                    0
                }
                None => MATCH_NOT_FOUND,
            };
        }
    }
    if !write_records(&recs) {
        return INI_OPEN;
    }
    // This process sees the change at once.
    LIST.with(|l| *l = None);
    0
}
