//! File icons (PMWP's WinLoadFileIcon, WinFreeFileIcon, WinSetFileIcon,
//! which forward here as rs2-private PMWIN entries 9004-9006): the
//! .ICON extended attribute of a file or directory, else the icon of an
//! executable (its RT_DEFAULTICON, else RT_POINTER 1, else its first
//! RT_POINTER resource), else rs2's icon for the kind of file (folder,
//! program, data file).
//!
//! Files are read with the PM lock released (only the pointer is made
//! with it), into private memory.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use pmwin::sysptr;

use crate::ctl::cstr;
use crate::lock;
use crate::pointer;

const FIL_STANDARD: u32 = 1;
const FIL_QUERYEASIZE: u32 = 2;
const FIL_QUERYEASFROMLIST: u32 = 3;
const EAT_ICON: u16 = 0xfff9;
const FILE_DIRECTORY: u32 = 0x10;
const RT_POINTER: u16 = 1;
const RT_DEFAULTICON: u16 = 22;

/// Private memory (freed when dropped).
struct Mem {
    p: *mut u8,
    cb: usize,
}

impl Mem {
    fn new(cb: usize) -> Option<Mem> {
        let mut p = core::ptr::null_mut();
        // PAG_READ | PAG_WRITE | PAG_COMMIT
        (unsafe { os2::DosAllocMem(&mut p, cb.max(1) as u32, 0x13) } == 0).then_some(Mem { p, cb })
    }

    fn slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.p, self.cb) }
    }

    fn slice_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.p, self.cb) }
    }
}

impl Drop for Mem {
    fn drop(&mut self) {
        unsafe { os2::DosFreeMem(self.p) };
    }
}

/// The .ICON EA's value (an icon file), if the file has one.
fn icon_ea(path: &[u8]) -> Option<Mem> {
    // GEA2LIST with ".ICON".
    let mut gea = [0u8; 16];
    gea[0..4].copy_from_slice(&16u32.to_le_bytes());
    gea[8] = 5;
    gea[9..14].copy_from_slice(b".ICON");
    let mut fea = Mem::new(0x10000)?;
    fea.slice_mut()[0..4].copy_from_slice(&0x10000u32.to_le_bytes());
    let eaop: [u32; 3] = [gea.as_ptr() as u32, fea.p as u32, 0];
    let mut z = path.to_vec();
    z.push(0);
    if unsafe { os2::DosQueryPathInfo(z.as_ptr(), FIL_QUERYEASFROMLIST, eaop.as_ptr() as *mut u8, 12) } != 0 {
        return None;
    }
    // FEA2: oNextEntryOffset, fEA, cbName, cbValue, szName, value.
    let b = fea.slice();
    let cb_name = b[9] as usize;
    let cb_value = u16::from_le_bytes([b[10], b[11]]) as usize;
    let v = b.get(12 + cb_name + 1..12 + cb_name + 1 + cb_value)?;
    if v.len() < 4 || u16::from_le_bytes([v[0], v[1]]) != EAT_ICON {
        return None;
    }
    let n = (u16::from_le_bytes([v[2], v[3]]) as usize).min(v.len() - 4);
    let mut out = Mem::new(n)?;
    out.slice_mut().copy_from_slice(&v[4..4 + n]);
    Some(out)
}

/// The file's attributes (FIL_STANDARD attrFile), if it exists.
fn attributes(path: &[u8]) -> Option<u32> {
    let mut z = path.to_vec();
    z.push(0);
    let mut fs3 = [0u8; 24];
    (unsafe { os2::DosQueryPathInfo(z.as_ptr(), FIL_STANDARD, fs3.as_mut_ptr(), 24) } == 0).then(|| u32::from_le_bytes([fs3[20], fs3[21], fs3[22], fs3[23]]))
}

/// The whole file (up to 8 MB).
fn read_file(path: &[u8]) -> Option<Mem> {
    let mut z = path.to_vec();
    z.push(0);
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS; OPEN_SHARE_DENYNONE | OPEN_ACCESS_READONLY.
    if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut size = 0u32;
    unsafe { os2::DosSetFilePtr(hf, 0, 2, &mut size) };
    let mut pos = 0u32;
    unsafe { os2::DosSetFilePtr(hf, 0, 0, &mut pos) };
    let r = (size > 0 && size <= 8 << 20).then(|| Mem::new(size as usize)).flatten().and_then(|m| {
        let mut got = 0u32;
        let ok = unsafe { os2::DosRead(hf, m.p, size, &mut got) } == 0 && got == size;
        ok.then_some(m)
    });
    unsafe { os2::DosClose(hf) };
    r
}

/// An executable's icon resource.
fn exe_icon(data: &[u8]) -> Option<Vec<u8>> {
    let list = os2_exe::res::list(data);
    let pick = list
        .iter()
        .find(|&&(t, _)| t == RT_DEFAULTICON)
        .or_else(|| list.iter().find(|&&(t, n)| t == RT_POINTER && n == 1))
        .or_else(|| list.iter().filter(|&&(t, _)| t == RT_POINTER).min_by_key(|&&(_, n)| n))?;
    os2_exe::res::get(data, pick.0, pick.1)
}

fn has_ext(path: &[u8], exts: &[&[u8]]) -> bool {
    let name = &path[path.iter().rposition(|&c| c == b'\\' || c == b'/' || c == b':').map_or(0, |i| i + 1)..];
    let Some(dot) = name.iter().rposition(|&c| c == b'.') else { return false };
    exts.iter().any(|e| name[dot + 1..].eq_ignore_ascii_case(e))
}

/// What the icon comes from.
enum Source {
    Data(Mem),
    Bytes(Vec<u8>),
    System(u32),
}

fn find(path: &[u8]) -> Option<Source> {
    let attr = attributes(path)?;
    if let Some(m) = icon_ea(path) {
        return Some(Source::Data(m));
    }
    if attr & FILE_DIRECTORY != 0 {
        return Some(Source::System(sysptr::SPTR_FOLDER));
    }
    if has_ext(path, &[b"EXE", b"DLL"]) {
        if let Some(b) = read_file(path).and_then(|d| exe_icon(d.slice())) {
            return Some(Source::Bytes(b));
        }
    }
    if has_ext(path, &[b"EXE", b"COM", b"CMD", b"BAT"]) {
        return Some(Source::System(sysptr::SPTR_PROGRAM));
    }
    Some(Source::System(sysptr::SPTR_FILE))
}

/// Rs2LoadFileIcon(pszFileName, fPrivate) = WinLoadFileIcon: the file's
/// icon, a pointer of the caller (0 if the file does not exist).
#[unsafe(no_mangle)]
pub extern "C" fn Rs2LoadFileIcon(name: *const u8, _private: u32) -> u32 {
    let path = cstr(name);
    if path.is_empty() {
        return 0;
    }
    let found = lock::unlocked(|| find(&path));
    let me = crate::state::pid();
    let h = match &found {
        Some(Source::Data(m)) => pointer::from_data(m.slice(), me),
        Some(Source::Bytes(b)) => pointer::from_data(b, me),
        _ => 0,
    };
    if h != 0 {
        return h;
    }
    match found {
        // A private copy of the system icon (WinFreeFileIcon frees it).
        Some(Source::System(id)) => pointer::WinQuerySysPointer(1, id as i32, 1),
        Some(_) => pointer::WinQuerySysPointer(1, sysptr::SPTR_FILE as i32, 1),
        None => 0,
    }
}

/// Rs2FreeFileIcon(hptr) = WinFreeFileIcon.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2FreeFileIcon(h: u32) -> u32 {
    pointer::WinDestroyPointer(h)
}

/// Write (or with `data` empty remove) a file's .ICON EA.
fn set_icon_ea(path: &[u8], data: &[u8]) -> bool {
    if data.len() > 0xfff0 {
        return false;
    }
    let value_len = if data.is_empty() { 0 } else { 4 + data.len() };
    let cb = (4 + 4 + 6 + value_len + 3) & !3;
    let Some(mut m) = Mem::new(cb + 8) else { return false };
    let b = m.slice_mut();
    b[0..4].copy_from_slice(&(cb as u32).to_le_bytes());
    // FEA2: oNextEntryOffset 0, fEA 0, cbName 5, cbValue.
    b[8] = 0;
    b[9] = 5;
    b[10..12].copy_from_slice(&(value_len as u16).to_le_bytes());
    b[12..17].copy_from_slice(b".ICON");
    b[17] = 0;
    if !data.is_empty() {
        b[18..20].copy_from_slice(&EAT_ICON.to_le_bytes());
        b[20..22].copy_from_slice(&(data.len() as u16).to_le_bytes());
        b[22..22 + data.len()].copy_from_slice(data);
    }
    let eaop: [u32; 3] = [0, m.p as u32, 0];
    let mut z = path.to_vec();
    z.push(0);
    unsafe { os2::DosSetPathInfo(z.as_ptr(), FIL_QUERYEASIZE, eaop.as_ptr() as *const u8, 12, 0) == 0 }
}

/// ICONINFO: cb, fFormat, pszFileName, hmod, resid, cbIconData, pIconData.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IconInfo {
    cb: u32,
    format: u32,
    file: *const u8,
    hmod: u32,
    resid: u32,
    cb_data: u32,
    data: *const u8,
}

/// Rs2SetFileIcon(pszFileName, pIconInfo) = WinSetFileIcon: set the
/// file's .ICON EA from an icon file (ICON_FILE), a resource
/// (ICON_RESOURCE) or data (ICON_DATA); ICON_CLEAR removes it.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2SetFileIcon(name: *const u8, pi: *const IconInfo) -> u32 {
    const ICON_FILE: u32 = 1;
    const ICON_RESOURCE: u32 = 2;
    const ICON_DATA: u32 = 3;
    const ICON_CLEAR: u32 = 4;
    let path = cstr(name);
    if path.is_empty() || pi.is_null() {
        return 0;
    }
    let info = unsafe { pi.read_unaligned() };
    let data: Vec<u8> = match info.format {
        ICON_FILE => {
            let f = cstr(info.file);
            match lock::unlocked(|| read_file(&f)) {
                Some(m) => m.slice().to_vec(),
                None => return 0,
            }
        }
        ICON_RESOURCE => match crate::res::load(info.hmod, RT_POINTER as u32, info.resid) {
            Some(b) => b.to_vec(),
            None => return 0,
        },
        ICON_DATA if !info.data.is_null() => unsafe { core::slice::from_raw_parts(info.data, info.cb_data as usize) }.to_vec(),
        ICON_CLEAR => Vec::new(),
        _ => return 0,
    };
    if !data.is_empty() && gpi::icon::parse(&data).is_empty() {
        return 0;
    }
    lock::unlocked(|| set_icon_ea(&path, &data)) as u32
}
