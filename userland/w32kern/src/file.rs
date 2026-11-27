//! Files, directories and paths on DosOpen and friends.

use alloc::vec::Vec;

use crate::handles::{self, Obj, INVALID_HANDLE_VALUE};
use crate::text::{from_wide, to_wide};
use crate::time::{ft_from_os2, get_ft, os2_from_ft, put_ft};
use crate::{cstr, err, fail, put_str, put_wstr, set_error, win_error, wstr, z, FALSE, TRUE};

unsafe extern "C" {
    #[link_name = "DOSCALLS.254"]
    fn DosResetBuffer(hf: u32) -> u32;
}

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const GENERIC_ALL: u32 = 0x1000_0000;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;

const CREATE_NEW: u32 = 1;
const CREATE_ALWAYS: u32 = 2;
const OPEN_EXISTING: u32 = 3;
const OPEN_ALWAYS: u32 = 4;
const TRUNCATE_EXISTING: u32 = 5;

pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
pub const INVALID_FILE_ATTRIBUTES: u32 = 0xffff_ffff;

const ERROR_OPEN_FAILED: u32 = 110;

/// Windows attributes from OS/2's (the same bits), NORMAL for none.
fn win_attr(a: u32) -> u32 {
    let a = a & 0x37;
    if a == 0 { FILE_ATTRIBUTE_NORMAL } else { a }
}

/// A path for OS/2: '/' becomes '\', and a trailing '\' of a directory
/// (not a root) goes.
pub fn os2_path(p: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = p.iter().map(|&c| if c == b'/' { b'\\' } else { c }).collect();
    while v.len() > 1 && v.ends_with(b"\\") && !(v.len() == 3 && v[1] == b':') {
        v.pop();
    }
    v
}

fn device(name: &[u8]) -> Option<u32> {
    let up = name.to_ascii_uppercase();
    match &up[..] {
        b"CONIN$" => Some(0),
        b"CONOUT$" => Some(1),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateFileA(name: *const u8, access: u32, share: u32, sa: *const u32, disposition: u32, flags: u32, _template: u32) -> u32 {
    let n = unsafe { cstr(name) };
    if n.is_empty() {
        return fail(err::PATH_NOT_FOUND, INVALID_HANDLE_VALUE);
    }
    if let Some(std) = device(n) {
        let mut hf = 0xffff_ffffu32;
        if unsafe { os2::DosDupHandle(std, &mut hf) } != 0 {
            return fail(err::INVALID_HANDLE, INVALID_HANDLE_VALUE);
        }
        return handles::add(Obj::File { hf });
    }
    let open_flags = match disposition {
        CREATE_NEW => 0x10,
        CREATE_ALWAYS => 0x12,
        OPEN_EXISTING => 0x01,
        OPEN_ALWAYS => 0x11,
        TRUNCATE_EXISTING => 0x02,
        _ => return fail(err::INVALID_PARAMETER, INVALID_HANDLE_VALUE),
    };
    let rw = access & (GENERIC_READ | GENERIC_ALL) != 0 || access & 1 != 0;
    let ww = access & (GENERIC_WRITE | GENERIC_ALL) != 0 || access & 2 != 0;
    let mut mode = match (rw, ww) {
        (_, false) => 0,
        (false, true) => 1,
        (true, true) => 2,
    };
    mode |= match share & (FILE_SHARE_READ | FILE_SHARE_WRITE) {
        0 => 0x10,
        FILE_SHARE_READ => 0x20,
        FILE_SHARE_WRITE => 0x30,
        _ => 0x40,
    };
    // Windows handles are not inherited unless the security attributes
    // say so (bInheritHandle, the third word).
    let inherit = !sa.is_null() && unsafe { *sa.add(2) } != 0;
    if !inherit {
        mode |= 0x80;
    }
    // FILE_FLAG_WRITE_THROUGH
    if flags & 0x8000_0000 != 0 {
        mode |= 0x4000;
    }
    let attr = flags & 0x27;
    let path = z(&os2_path(n));
    let (mut hf, mut action) = (0u32, 0u32);
    let rc = unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, 0, attr, open_flags, mode, core::ptr::null_mut()) };
    if rc != 0 {
        let e = match (rc, disposition) {
            (ERROR_OPEN_FAILED, CREATE_NEW) => err::FILE_EXISTS,
            (ERROR_OPEN_FAILED, _) => err::FILE_NOT_FOUND,
            (r, _) => win_error(r),
        };
        return fail(e, INVALID_HANDLE_VALUE);
    }
    // OPEN_ALWAYS and CREATE_ALWAYS say whether the file was there.
    let existed = action == 1 || action == 3;
    set_error(if existed && (disposition == OPEN_ALWAYS || disposition == CREATE_ALWAYS) { err::ALREADY_EXISTS } else { 0 });
    handles::add(Obj::File { hf })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *const u32, disposition: u32, flags: u32, template: u32) -> u32 {
    let n = z(&from_wide(unsafe { wstr(name) }));
    unsafe { CreateFileA(n.as_ptr(), access, share, sa, disposition, flags, template) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ReadFile(h: u32, buf: *mut u8, n: u32, read: *mut u32, _ov: u32) -> i32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    if hf == 0 && crate::console::is_console(hf) {
        return unsafe { crate::console::read_line(buf, n, read) };
    }
    let mut got = 0u32;
    let rc = unsafe { os2::DosRead(hf, buf, n, &mut got) };
    if !read.is_null() {
        unsafe { *read = got };
    }
    // A broken pipe is the end of the data (ERROR_BROKEN_PIPE 109).
    if rc == 109 {
        return fail(109, FALSE);
    }
    if rc != 0 { fail(win_error(rc), FALSE) } else { TRUE }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WriteFile(h: u32, buf: *const u8, n: u32, written: *mut u32, _ov: u32) -> i32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let mut done = 0u32;
    let rc = if n == 0 { 0 } else { unsafe { crate::console::write(hf, buf, n, &mut done) } };
    if !written.is_null() {
        unsafe { *written = done };
    }
    if rc != 0 { fail(win_error(rc), FALSE) } else { TRUE }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetFilePointer(h: u32, lo: i32, hi: *mut i32, method: u32) -> u32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, 0xffff_ffff) };
    if !hi.is_null() {
        let high = unsafe { *hi };
        if high != 0 && !(high == -1 && lo < 0) {
            return fail(err::INVALID_PARAMETER, 0xffff_ffff);
        }
    }
    let mut new = 0u32;
    let rc = unsafe { os2::DosSetFilePtr(hf, lo, method, &mut new) };
    if rc != 0 {
        return fail(if rc == 131 { err::NEGATIVE_SEEK } else { win_error(rc) }, 0xffff_ffff);
    }
    if !hi.is_null() {
        unsafe { *hi = 0 };
    }
    set_error(0);
    new
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetFilePointerEx(h: u32, dist: i64, new: *mut i64, method: u32) -> i32 {
    if dist > i32::MAX as i64 || dist < i32::MIN as i64 {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    let r = unsafe { SetFilePointer(h, dist as i32, core::ptr::null_mut(), method) };
    if r == 0xffff_ffff && crate::last_error() != 0 {
        return FALSE;
    }
    if !new.is_null() {
        unsafe { new.write_unaligned(r as i64) };
    }
    TRUE
}

/// FILESTATUS3 of a handle.
fn file_status(hf: u32) -> Option<[u8; 24]> {
    let mut b = [0u8; 24];
    (unsafe { os2::DosQueryFileInfo(hf, 1, b.as_mut_ptr(), 24) } == 0).then_some(b)
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFileSize(h: u32, hi: *mut u32) -> u32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, 0xffff_ffff) };
    let Some(b) = file_status(hf) else { return fail(err::INVALID_HANDLE, 0xffff_ffff) };
    if !hi.is_null() {
        unsafe { *hi = 0 };
    }
    set_error(0);
    u32_at(&b, 12)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFileSizeEx(h: u32, size: *mut i64) -> i32 {
    let s = unsafe { GetFileSize(h, core::ptr::null_mut()) };
    if s == 0xffff_ffff && crate::last_error() != 0 {
        return FALSE;
    }
    unsafe { size.write_unaligned(s as i64) };
    TRUE
}

pub const FILE_TYPE_UNKNOWN: u32 = 0;
pub const FILE_TYPE_DISK: u32 = 1;
pub const FILE_TYPE_CHAR: u32 = 2;
pub const FILE_TYPE_PIPE: u32 = 3;

pub fn handle_type(hf: u32) -> u32 {
    let (mut ty, mut attr) = (0u32, 0u32);
    if unsafe { os2::DosQueryHType(hf, &mut ty, &mut attr) } != 0 {
        return FILE_TYPE_UNKNOWN;
    }
    match ty & 0xff {
        0 => FILE_TYPE_DISK,
        1 => FILE_TYPE_CHAR,
        2 => FILE_TYPE_PIPE,
        _ => FILE_TYPE_UNKNOWN,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetFileType(h: u32) -> u32 {
    match handles::file(h) {
        Some(hf) => handle_type(hf),
        None => fail(err::INVALID_HANDLE, FILE_TYPE_UNKNOWN),
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FlushFileBuffers(h: u32) -> i32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    if handle_type(hf) == FILE_TYPE_DISK {
        unsafe { DosResetBuffer(hf) };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetEndOfFile(h: u32) -> i32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let mut pos = 0u32;
    if unsafe { os2::DosSetFilePtr(hf, 0, 1, &mut pos) } != 0 {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let rc = unsafe { os2::DosSetFileSize(hf, pos) };
    if rc != 0 { fail(win_error(rc), FALSE) } else { TRUE }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFileTime(h: u32, created: *mut u32, accessed: *mut u32, written: *mut u32) -> i32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let Some(b) = file_status(hf) else { return fail(err::INVALID_HANDLE, FALSE) };
    unsafe {
        put_ft(created, ft_from_os2(u16_at(&b, 0), u16_at(&b, 2)));
        put_ft(accessed, ft_from_os2(u16_at(&b, 4), u16_at(&b, 6)));
        put_ft(written, ft_from_os2(u16_at(&b, 8), u16_at(&b, 10)));
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetFileTime(h: u32, created: *const u32, accessed: *const u32, written: *const u32) -> i32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let Some(mut b) = file_status(hf) else { return fail(err::INVALID_HANDLE, FALSE) };
    for (p, o) in [(created, 0), (accessed, 4), (written, 8)] {
        if !p.is_null() {
            let (d, t) = os2_from_ft(unsafe { get_ft(p) });
            b[o..o + 2].copy_from_slice(&d.to_le_bytes());
            b[o + 2..o + 4].copy_from_slice(&t.to_le_bytes());
        }
    }
    unsafe extern "C" {
        #[link_name = "DOSCALLS.218"]
        fn DosSetFileInfo(hf: u32, level: u32, buf: *const u8, cb: u32) -> u32;
    }
    let rc = unsafe { DosSetFileInfo(hf, 1, b.as_ptr(), 24) };
    if rc != 0 { fail(win_error(rc), FALSE) } else { TRUE }
}

/// BY_HANDLE_FILE_INFORMATION.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFileInformationByHandle(h: u32, info: *mut u32) -> i32 {
    let Some(hf) = handles::file(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let Some(b) = file_status(hf) else { return fail(err::INVALID_HANDLE, FALSE) };
    unsafe {
        core::ptr::write_bytes(info, 0, 13);
        *info = win_attr(u32_at(&b, 20));
        put_ft(info.add(1), ft_from_os2(u16_at(&b, 0), u16_at(&b, 2)));
        put_ft(info.add(3), ft_from_os2(u16_at(&b, 4), u16_at(&b, 6)));
        put_ft(info.add(5), ft_from_os2(u16_at(&b, 8), u16_at(&b, 10)));
        *info.add(8) = u32_at(&b, 12);
        *info.add(9) = 1;
        *info.add(11) = hf;
    }
    TRUE
}

// ---- Paths ----

fn path_status(path: &[u8]) -> Result<[u8; 24], u32> {
    let p = z(&os2_path(path));
    let mut b = [0u8; 24];
    match unsafe { os2::DosQueryPathInfo(p.as_ptr(), 1, b.as_mut_ptr(), 24) } {
        0 => Ok(b),
        rc => Err(win_error(rc)),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFileAttributesA(name: *const u8) -> u32 {
    match path_status(unsafe { cstr(name) }) {
        Ok(b) => win_attr(u32_at(&b, 20)),
        Err(e) => fail(e, INVALID_FILE_ATTRIBUTES),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFileAttributesW(name: *const u16) -> u32 {
    let n = z(&from_wide(unsafe { wstr(name) }));
    unsafe { GetFileAttributesA(n.as_ptr()) }
}

/// WIN32_FILE_ATTRIBUTE_DATA.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFileAttributesExA(name: *const u8, _level: u32, info: *mut u32) -> i32 {
    match path_status(unsafe { cstr(name) }) {
        Ok(b) => {
            unsafe {
                *info = win_attr(u32_at(&b, 20));
                put_ft(info.add(1), ft_from_os2(u16_at(&b, 0), u16_at(&b, 2)));
                put_ft(info.add(3), ft_from_os2(u16_at(&b, 4), u16_at(&b, 6)));
                put_ft(info.add(5), ft_from_os2(u16_at(&b, 8), u16_at(&b, 10)));
                *info.add(7) = 0;
                *info.add(8) = u32_at(&b, 12);
            }
            TRUE
        }
        Err(e) => fail(e, FALSE),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetFileAttributesA(name: *const u8, attr: u32) -> i32 {
    let n = unsafe { cstr(name) };
    let mut b = match path_status(n) {
        Ok(b) => b,
        Err(e) => return fail(e, FALSE),
    };
    let dir = u32_at(&b, 20) & 0x10;
    b[20..24].copy_from_slice(&((attr & 0x27) | dir).to_le_bytes());
    let p = z(&os2_path(n));
    let rc = unsafe { os2::DosSetPathInfo(p.as_ptr(), 1, b.as_ptr(), 24, 0) };
    if rc != 0 { fail(win_error(rc), FALSE) } else { TRUE }
}

fn path_call(name: *const u8, f: impl FnOnce(*const u8) -> u32) -> i32 {
    let p = z(&os2_path(unsafe { cstr(name) }));
    match f(p.as_ptr()) {
        0 => TRUE,
        rc => fail(win_error(rc), FALSE),
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DeleteFileA(name: *const u8) -> i32 {
    path_call(name, |p| unsafe { os2::DosDelete(p) })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn DeleteFileW(name: *const u16) -> i32 {
    let n = z(&from_wide(unsafe { wstr(name) }));
    DeleteFileA(n.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDirectoryA(name: *const u8, _sa: u32) -> i32 {
    let r = path_call(name, |p| unsafe { os2::DosCreateDir(p, core::ptr::null_mut()) });
    // An existing directory: ERROR_ALREADY_EXISTS (OS/2 says access denied).
    if r == FALSE && crate::last_error() == err::ACCESS_DENIED {
        if let Ok(b) = path_status(unsafe { cstr(name) }) {
            if u32_at(&b, 20) & 0x10 != 0 {
                set_error(err::ALREADY_EXISTS);
            }
        }
    }
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateDirectoryW(name: *const u16, sa: u32) -> i32 {
    let n = z(&from_wide(unsafe { wstr(name) }));
    CreateDirectoryA(n.as_ptr(), sa)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RemoveDirectoryA(name: *const u8) -> i32 {
    path_call(name, |p| unsafe { os2::DosDeleteDir(p) })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn MoveFileA(from: *const u8, to: *const u8) -> i32 {
    let t = z(&os2_path(unsafe { cstr(to) }));
    path_call(from, |p| unsafe { os2::DosMove(p, t.as_ptr()) })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn MoveFileExA(from: *const u8, to: *const u8, flags: u32) -> i32 {
    if to.is_null() {
        // Delete at reboot: not done.
        return TRUE;
    }
    // MOVEFILE_REPLACE_EXISTING
    if flags & 1 != 0 && unsafe { GetFileAttributesA(to) } != INVALID_FILE_ATTRIBUTES {
        DeleteFileA(to);
    }
    let r = unsafe { MoveFileA(from, to) };
    // MOVEFILE_COPY_ALLOWED: across drives.
    if r == FALSE && flags & 2 != 0 && unsafe { CopyFileA(from, to, FALSE) } == TRUE {
        DeleteFileA(from);
        return TRUE;
    }
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CopyFileA(from: *const u8, to: *const u8, fail_if_exists: i32) -> i32 {
    let t = z(&os2_path(unsafe { cstr(to) }));
    if fail_if_exists != 0 && unsafe { GetFileAttributesA(to) } != INVALID_FILE_ATTRIBUTES {
        return fail(err::FILE_EXISTS, FALSE);
    }
    // DCPY_EXISTING
    path_call(from, |p| unsafe { os2::DosCopy(p, t.as_ptr(), 1) })
}

// ---- Searches ----

/// FILEFINDBUF3 to WIN32_FIND_DATAA.
unsafe fn find_data(b: &[u8], fd: *mut u8) {
    let name_len = b[28] as usize;
    let name = &b[29..29 + name_len.min(255)];
    unsafe {
        core::ptr::write_bytes(fd, 0, 320);
        let w = fd as *mut u32;
        *w = win_attr(u32_at(b, 24));
        put_ft(w.add(1), ft_from_os2(u16_at(b, 4), u16_at(b, 6)));
        put_ft(w.add(3), ft_from_os2(u16_at(b, 8), u16_at(b, 10)));
        put_ft(w.add(5), ft_from_os2(u16_at(b, 12), u16_at(b, 14)));
        *w.add(8) = u32_at(b, 16);
        core::ptr::copy_nonoverlapping(name.as_ptr(), fd.add(44), name.len());
        // The 8.3 name: the long one if it is one.
        if name.len() <= 12 {
            core::ptr::copy_nonoverlapping(name.as_ptr(), fd.add(304), name.len());
        }
    }
}

/// WIN32_FIND_DATAA to WIN32_FIND_DATAW.
unsafe fn find_data_w(a: *const u8, w: *mut u8) {
    unsafe {
        core::ptr::copy_nonoverlapping(a, w, 44);
        let name = to_wide(cstr(a.add(44)));
        let alt = to_wide(cstr(a.add(304)));
        core::ptr::write_bytes(w.add(44), 0, 520 + 28);
        core::ptr::copy_nonoverlapping(name.as_ptr(), w.add(44) as *mut u16, name.len().min(259));
        core::ptr::copy_nonoverlapping(alt.as_ptr(), w.add(44 + 520) as *mut u16, alt.len().min(13));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindFirstFileA(spec: *const u8, fd: *mut u8) -> u32 {
    let mut s = os2_path(unsafe { cstr(spec) });
    // "DIR\" means everything in it, as "DIR\*".
    if s.ends_with(b"\\") {
        s.push(b'*');
    }
    let s = z(&s);
    let mut hdir = 0xffff_ffffu32; // HDIR_CREATE
    let mut buf = [0u8; 300];
    let mut count = 1u32;
    let rc = unsafe { os2::DosFindFirst(s.as_ptr(), &mut hdir, 0x37, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    if rc != 0 {
        return fail(if rc == err::NO_MORE_FILES { err::FILE_NOT_FOUND } else { win_error(rc) }, INVALID_HANDLE_VALUE);
    }
    unsafe { find_data(&buf, fd) };
    handles::add(Obj::Find { hdir })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindNextFileA(h: u32, fd: *mut u8) -> i32 {
    let Some(Obj::Find { hdir }) = handles::get(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let mut buf = [0u8; 300];
    let mut count = 1u32;
    let rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    if rc != 0 || count == 0 {
        return fail(if rc == 0 { err::NO_MORE_FILES } else { win_error(rc) }, FALSE);
    }
    unsafe { find_data(&buf, fd) };
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindFirstFileW(spec: *const u16, fd: *mut u8) -> u32 {
    let s = z(&from_wide(unsafe { wstr(spec) }));
    let mut a = [0u8; 320];
    let h = unsafe { FindFirstFileA(s.as_ptr(), a.as_mut_ptr()) };
    if h != INVALID_HANDLE_VALUE {
        unsafe { find_data_w(a.as_ptr(), fd) };
    }
    h
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindNextFileW(h: u32, fd: *mut u8) -> i32 {
    let mut a = [0u8; 320];
    let r = unsafe { FindNextFileA(h, a.as_mut_ptr()) };
    if r == TRUE {
        unsafe { find_data_w(a.as_ptr(), fd) };
    }
    r
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FindClose(h: u32) -> i32 {
    match handles::remove(h) {
        Some(Obj::Find { hdir }) => {
            unsafe { os2::DosFindClose(hdir) };
            TRUE
        }
        _ => fail(err::INVALID_HANDLE, FALSE),
    }
}

// ---- Directories and drives ----

pub fn current_drive() -> u32 {
    let (mut d, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
    d
}

/// The current directory of drive `d` (1 = A:), "X:\DIR".
pub fn current_dir(d: u32) -> Vec<u8> {
    let mut buf = [0u8; 260];
    let mut n = buf.len() as u32;
    unsafe { os2::DosQueryCurrentDir(d, buf.as_mut_ptr(), &mut n) };
    let dir = unsafe { cstr(buf.as_ptr()) };
    let mut v = alloc::vec![b'A' + d as u8 - 1, b':', b'\\'];
    v.extend_from_slice(dir);
    v
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetCurrentDirectoryA(size: u32, buf: *mut u8) -> u32 {
    let d = current_dir(current_drive());
    unsafe { put_str(&d, buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetCurrentDirectoryW(size: u32, buf: *mut u16) -> u32 {
    let d = to_wide(&current_dir(current_drive()));
    unsafe { put_wstr(&d, buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetCurrentDirectoryA(path: *const u8) -> i32 {
    let p = os2_path(unsafe { cstr(path) });
    let mut rest = &p[..];
    if p.len() >= 2 && p[1] == b':' {
        let d = (p[0].to_ascii_uppercase().wrapping_sub(b'A')) as u32 + 1;
        let rc = unsafe { os2::DosSetDefaultDisk(d) };
        if rc != 0 {
            return fail(win_error(rc), FALSE);
        }
        rest = &p[2..];
        if rest.is_empty() {
            return TRUE;
        }
    }
    let r = z(rest);
    let rc = unsafe { os2::DosSetCurrentDir(r.as_ptr()) };
    if rc != 0 { fail(win_error(rc), FALSE) } else { TRUE }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetCurrentDirectoryW(path: *const u16) -> i32 {
    let p = z(&from_wide(unsafe { wstr(path) }));
    unsafe { SetCurrentDirectoryA(p.as_ptr()) }
}

/// The full path of `p`: drive and directory filled in, "." and ".."
/// folded.
pub fn full_path(p: &[u8]) -> Vec<u8> {
    let p = os2_path(p);
    let (drive, rest): (u32, &[u8]) = if p.len() >= 2 && p[1] == b':' {
        ((p[0].to_ascii_uppercase().wrapping_sub(b'A')) as u32 + 1, &p[2..])
    } else {
        (current_drive(), &p[..])
    };
    let mut parts: Vec<Vec<u8>> = Vec::new();
    if !rest.starts_with(b"\\") {
        let cd = current_dir(drive);
        for s in cd[3..].split(|&c| c == b'\\').filter(|s| !s.is_empty()) {
            parts.push(s.to_vec());
        }
    }
    for s in rest.split(|&c| c == b'\\') {
        match s {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s.to_vec()),
        }
    }
    let mut v = alloc::vec![b'A' + drive as u8 - 1, b':', b'\\'];
    for (i, s) in parts.iter().enumerate() {
        if i > 0 {
            v.push(b'\\');
        }
        v.extend_from_slice(s);
    }
    // "X:\dir\" kept a trailing separator the caller gave.
    if rest.ends_with(b"\\") && !parts.is_empty() {
        v.push(b'\\');
    }
    v
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFullPathNameA(name: *const u8, size: u32, buf: *mut u8, file_part: *mut *mut u8) -> u32 {
    let f = full_path(unsafe { cstr(name) });
    let r = unsafe { put_str(&f, buf, size) };
    if !file_part.is_null() && (r as usize) == f.len() {
        let i = f.iter().rposition(|&c| c == b'\\').map_or(0, |i| i + 1);
        unsafe { *file_part = if i < f.len() { buf.add(i) } else { core::ptr::null_mut() } };
    }
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetFullPathNameW(name: *const u16, size: u32, buf: *mut u16, file_part: *mut *mut u16) -> u32 {
    let f = to_wide(&full_path(&from_wide(unsafe { wstr(name) })));
    let r = unsafe { put_wstr(&f, buf, size) };
    if !file_part.is_null() && (r as usize) == f.len() {
        let i = f.iter().rposition(|&c| c == b'\\' as u16).map_or(0, |i| i + 1);
        unsafe { *file_part = if i < f.len() { buf.add(i) } else { core::ptr::null_mut() } };
    }
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetShortPathNameA(long: *const u8, buf: *mut u8, size: u32) -> u32 {
    let s = unsafe { cstr(long) }.to_vec();
    unsafe { put_str(&s, buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetLongPathNameA(short: *const u8, buf: *mut u8, size: u32) -> u32 {
    let s = unsafe { cstr(short) }.to_vec();
    unsafe { put_str(&s, buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetTempPathA(size: u32, buf: *mut u8) -> u32 {
    let mut p = ["TMP", "TEMP"]
        .iter()
        .find_map(|v| crate::process::env_get(v.as_bytes()))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| alloc::vec![b'A' + boot_drive() as u8 - 1, b':', b'\\']);
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    unsafe { put_str(&p, buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetTempPathW(size: u32, buf: *mut u16) -> u32 {
    let mut a = [0u8; 260];
    let n = unsafe { GetTempPathA(260, a.as_mut_ptr()) } as usize;
    let w = to_wide(&a[..n.min(259)]);
    unsafe { put_wstr(&w, buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetTempFileNameA(dir: *const u8, prefix: *const u8, unique: u32, out: *mut u8) -> u32 {
    let d = unsafe { cstr(dir) };
    let pre = unsafe { cstr(prefix) };
    let mut n = if unique != 0 { unique } else { crate::time::GetTickCount() & 0xffff };
    loop {
        let mut v = d.to_vec();
        if !v.ends_with(b"\\") && !v.is_empty() {
            v.push(b'\\');
        }
        v.extend_from_slice(&pre[..pre.len().min(3)]);
        v.extend_from_slice(alloc::format!("{:04X}.TMP", n & 0xffff).as_bytes());
        let path = z(&v);
        if unique != 0 {
            unsafe { core::ptr::copy_nonoverlapping(path.as_ptr(), out, path.len()) };
            return unique;
        }
        let h = unsafe { CreateFileA(path.as_ptr(), GENERIC_WRITE, 0, core::ptr::null(), CREATE_NEW, 0, 0) };
        if h != INVALID_HANDLE_VALUE {
            crate::handles::CloseHandle(h);
            unsafe { core::ptr::copy_nonoverlapping(path.as_ptr(), out, path.len()) };
            return n & 0xffff;
        }
        if crate::last_error() != err::FILE_EXISTS {
            return 0;
        }
        n = n.wrapping_add(1);
    }
}

pub fn boot_drive() -> u32 {
    let mut d = 3u32;
    unsafe { os2::DosQuerySysInfo(5, 5, &mut d, 4) };
    d
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetLogicalDrives() -> u32 {
    let (mut d, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
    map
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetLogicalDriveStringsA(size: u32, buf: *mut u8) -> u32 {
    let map = GetLogicalDrives();
    let mut v = Vec::new();
    for i in 0..26 {
        if map & (1 << i) != 0 {
            v.extend_from_slice(&[b'A' + i as u8, b':', b'\\', 0]);
        }
    }
    v.push(0);
    if (size as usize) < v.len() {
        return v.len() as u32;
    }
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf, v.len()) };
    v.len() as u32 - 1
}

fn drive_of(root: *const u8) -> u32 {
    let r = unsafe { cstr(root) };
    if r.len() >= 2 && r[1] == b':' { (r[0].to_ascii_uppercase().wrapping_sub(b'A')) as u32 + 1 } else { current_drive() }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDriveTypeA(root: *const u8) -> u32 {
    let d = drive_of(root);
    if GetLogicalDrives() & (1 << (d - 1)) == 0 {
        return 1; // DRIVE_NO_ROOT_DIR
    }
    // DRIVE_REMOVABLE for A: and B:, else DRIVE_FIXED.
    if d <= 2 { 2 } else { 3 }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetDiskFreeSpaceA(root: *const u8, spc: *mut u32, bps: *mut u32, free: *mut u32, total: *mut u32) -> i32 {
    let mut b = [0u8; 18];
    let rc = unsafe { os2::DosQueryFSInfo(drive_of(root), 1, b.as_mut_ptr(), 18) };
    if rc != 0 {
        return fail(win_error(rc), FALSE);
    }
    unsafe {
        if !spc.is_null() {
            *spc = u32_at(&b, 4);
        }
        if !total.is_null() {
            *total = u32_at(&b, 8);
        }
        if !free.is_null() {
            *free = u32_at(&b, 12);
        }
        if !bps.is_null() {
            *bps = u16_at(&b, 16) as u32;
        }
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetDiskFreeSpaceExA(root: *const u8, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32 {
    let (mut spc, mut bps, mut f, mut t) = (0, 0, 0, 0);
    if unsafe { GetDiskFreeSpaceA(root, &mut spc, &mut bps, &mut f, &mut t) } == FALSE {
        return FALSE;
    }
    let unit = spc as u64 * bps as u64;
    unsafe {
        if !avail.is_null() {
            avail.write_unaligned(f as u64 * unit);
        }
        if !total.is_null() {
            total.write_unaligned(t as u64 * unit);
        }
        if !free.is_null() {
            free.write_unaligned(f as u64 * unit);
        }
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetVolumeInformationA(root: *const u8, name: *mut u8, nsize: u32, serial: *mut u32, maxlen: *mut u32, flags: *mut u32, fsname: *mut u8, fssize: u32) -> i32 {
    let mut b = [0u8; 18];
    let rc = unsafe { os2::DosQueryFSInfo(drive_of(root), 2, b.as_mut_ptr(), 18) };
    if rc != 0 {
        return fail(win_error(rc), FALSE);
    }
    let label_len = (b[4] as usize).min(11);
    unsafe {
        if !name.is_null() {
            put_str(&b[5..5 + label_len], name, nsize);
        }
        if !serial.is_null() {
            *serial = u32_at(&b, 0);
        }
        if !maxlen.is_null() {
            *maxlen = 255;
        }
        if !flags.is_null() {
            *flags = 0x2; // FILE_CASE_PRESERVED_NAMES
        }
        if !fsname.is_null() {
            put_str(b"FAT", fsname, fssize);
        }
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SearchPathA(path: *const u8, file: *const u8, ext: *const u8, size: u32, buf: *mut u8, file_part: *mut *mut u8) -> u32 {
    let f = unsafe { cstr(file) };
    let mut names = alloc::vec![f.to_vec()];
    if !ext.is_null() && !f.contains(&b'.') {
        let mut n = f.to_vec();
        n.extend_from_slice(unsafe { cstr(ext) });
        names.insert(0, n);
    }
    let dirs: Vec<Vec<u8>> = if path.is_null() {
        crate::process::search_dirs()
    } else {
        unsafe { cstr(path) }.split(|&c| c == b';').filter(|d| !d.is_empty()).map(|d| d.to_vec()).collect()
    };
    for d in dirs {
        for n in &names {
            let mut p = d.clone();
            if !p.ends_with(b"\\") {
                p.push(b'\\');
            }
            p.extend_from_slice(n);
            if path_status(&p).is_ok() {
                let full = full_path(&p);
                return unsafe { GetFullPathNameA(z(&full).as_ptr(), size, buf, file_part) };
            }
        }
    }
    fail(err::FILE_NOT_FOUND, 0)
}
