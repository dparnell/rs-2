//! Directory lists in dialogs: LB_DIR and CB_DIR, DlgDirList and
//! DlgDirListComboBox, DlgDirSelectEx and DlgDirSelectComboBoxEx, as
//! Windows 3.1's USER makes them: files by name, directories as
//! "[name]" (with "[..]" below a drive's root), drives as "[-c-]", in
//! lower case.

use alloc::vec;
use alloc::vec::Vec;

const DDL_DIRECTORY: u32 = 0x10;
const DDL_DRIVES: u32 = 0x4000;
const DDL_EXCLUSIVE: u32 = 0x8000;

const LB_RESETCONTENT: u16 = 0x0184;
const LB_DIR: u16 = 0x018d;
const LB_GETCURSEL: u16 = 0x0188;
const LB_GETTEXT: u16 = 0x0189;
const LB_GETTEXTLEN: u16 = 0x018a;
const CB_RESETCONTENT: u16 = 0x014b;
const CB_DIR: u16 = 0x0145;
const CB_GETCURSEL: u16 = 0x0147;
const CB_GETLBTEXT: u16 = 0x0148;
const CB_GETLBTEXTLEN: u16 = 0x0149;

/// The directory and the pattern of a file spec ("c:\dir\*.txt": "c:\dir",
/// "*.txt"; "*.txt": none, "*.txt").
fn split(spec: &[u8]) -> (Option<Vec<u8>>, Vec<u8>) {
    match spec.iter().rposition(|&c| c == b'\\' || c == b'/' || c == b':') {
        Some(i) => {
            let mut d = spec[..=i].to_vec();
            if d.len() > 1 && d.ends_with(b"\\") && !d.ends_with(b":\\") {
                d.pop();
            }
            let p = &spec[i + 1..];
            (Some(d), if p.is_empty() { b"*.*".to_vec() } else { p.to_vec() })
        }
        None => (None, if spec.is_empty() { b"*.*".to_vec() } else { spec.to_vec() }),
    }
}

/// Names in `dir` (the current directory if none) matching `pattern`
/// with attributes `attr` (FILE_*): (name, is a directory).
fn find(dir: Option<&[u8]>, pattern: &[u8], attr: u32) -> Vec<(Vec<u8>, bool)> {
    let mut path = dir.map_or_else(Vec::new, |d| d.to_vec());
    if !path.is_empty() && !path.ends_with(b"\\") && !path.ends_with(b":") {
        path.push(b'\\');
    }
    path.extend_from_slice(pattern);
    // (File names are in PM's code page; a Windows 3.x task's in ANSI.)
    let z = crate::z(&crate::kern::to_pm(&path));
    let mut out = Vec::new();
    let mut hdir = 0xffff_ffffu32;
    let mut buf = vec![0u8; 4096];
    let mut n = 128u32;
    let mut rc = unsafe { os2::DosFindFirst(z.as_ptr(), &mut hdir, attr, buf.as_mut_ptr(), buf.len() as u32, &mut n, 1) };
    while rc == 0 && n > 0 {
        let mut off = 0usize;
        for _ in 0..n {
            let rd = |o: usize| u32::from_le_bytes(buf[off + o..off + o + 4].try_into().unwrap());
            let (next, a, len) = (rd(0), rd(24), buf[off + 28] as usize);
            let name = &buf[off + 29..off + 29 + len];
            if name != b"." {
                out.push((crate::kern::from_pm_str(&name.to_ascii_lowercase()), a & 0x10 != 0));
            }
            if next == 0 {
                break;
            }
            off += next as usize;
        }
        n = 128;
        rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    }
    unsafe { os2::DosFindClose(hdir) };
    out
}

/// What LB_DIR (CB_DIR) adds for `attr` (DDL_*) and `spec`, in order:
/// the files, the directories, the drives.
pub fn items(attr: u32, spec: &[u8]) -> Vec<Vec<u8>> {
    let (dir, pattern) = split(spec);
    let mut out = Vec::new();
    if attr & DDL_EXCLUSIVE == 0 || attr & 0x27 != 0 {
        // Read-only, hidden, system, archived as asked (plain files
        // always unless DDL_EXCLUSIVE).
        let mut v: Vec<_> = find(dir.as_deref(), &pattern, attr & 0x27).into_iter().filter(|e| !e.1).map(|e| e.0).collect();
        v.sort();
        out.extend(v);
    }
    if attr & DDL_DIRECTORY != 0 {
        let mut v: Vec<_> = find(dir.as_deref(), b"*.*", 0x37).into_iter().filter(|e| e.1).map(|e| e.0).collect();
        v.sort();
        for d in v {
            let mut s = vec![b'['];
            s.extend_from_slice(&d);
            s.push(b']');
            out.push(s);
        }
    }
    if attr & DDL_DRIVES != 0 {
        let (mut d, mut map) = (0u32, 0u32);
        unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
        for i in 0..26u8 {
            if map & (1 << i) != 0 {
                out.push(vec![b'[', b'-', b'a' + i, b'-', b']']);
            }
        }
    }
    out
}

/// LB_DIR / CB_DIR: the items added through `add` (LB_ADDSTRING's
/// result); the last one's index, or LB_ERR (-1).
pub fn fill(attr: u32, spec: &[u8], mut add: impl FnMut(&[u8]) -> u32) -> u32 {
    let mut last = u32::MAX;
    for s in items(attr, spec) {
        last = add(&crate::z(&s));
    }
    last
}

/// DlgDirList / DlgDirListComboBox (A form): `spec` (in and out) names a
/// directory, a pattern or both; the directory becomes the current one,
/// the list shows what it holds, the static control its path.
fn dir_list(h: u32, spec: u32, id: i32, st: i32, attr: u32, combo: bool) -> i32 {
    let given = if spec == 0 { Vec::new() } else { unsafe { crate::cstr(spec as *const u8) }.to_vec() };
    let (mut dir, mut pattern) = split(&given);
    // A spec that is a directory lists it.
    if dir.is_none() && !pattern.iter().any(|&c| c == b'*' || c == b'?') && crate::comdlg::is_dir(&pattern) {
        dir = Some(pattern.clone());
        pattern = b"*.*".to_vec();
    }
    if let Some(d) = &dir {
        let mut full = d.clone();
        if full.len() == 2 && full[1] == b':' {
            // A drive alone: its current directory.
            let drive = full[0].to_ascii_lowercase().wrapping_sub(b'a') as u32 + 1;
            if unsafe { os2::DosSetDefaultDisk(drive) } != 0 {
                return 0;
            }
            full = crate::comdlg::cur_dir();
        }
        if !crate::comdlg::set_dir(&full) && !set_rel(&full) {
            return 0;
        }
    }
    if spec != 0 {
        let z = crate::z(&pattern);
        unsafe { core::ptr::copy_nonoverlapping(z.as_ptr(), spec as *mut u8, z.len()) };
    }
    let hw = h as u16;
    if id != 0 {
        let c = crate::dialog::get_item(hw, id as u16);
        if c == 0 {
            return 0;
        }
        let (reset, dir_msg) = if combo { (CB_RESETCONTENT, CB_DIR) } else { (LB_RESETCONTENT, LB_DIR) };
        crate::wnd::send(c, reset, 0, 0);
        let z = crate::z(&pattern);
        crate::wnd::send(c, dir_msg, attr, z.as_ptr() as u32);
    }
    if st != 0 {
        let c = crate::dialog::get_item(hw, st as u16);
        if c != 0 {
            let z = crate::z(&crate::comdlg::cur_dir());
            crate::wnd::send(c, crate::wnd::W_SETTEXT, 0, z.as_ptr() as u32);
        }
    }
    1
}

/// A directory relative to the current one.
fn set_rel(d: &[u8]) -> bool {
    let z = crate::z(&crate::kern::to_pm(d));
    unsafe { os2::DosSetCurrentDir(z.as_ptr()) == 0 }
}

/// DlgDirSelectEx / DlgDirSelectComboBoxEx (A form): the selected item
/// into `buf`: a drive "c:", a directory "name\", a file its name; TRUE
/// for a drive or directory.
fn dir_select(h: u32, buf: u32, n: i32, id: i32, combo: bool) -> i32 {
    let c = crate::dialog::get_item(h as u16, id as u16);
    if c == 0 || buf == 0 || n <= 0 {
        return 0;
    }
    let (cur, len, get) = if combo { (CB_GETCURSEL, CB_GETLBTEXTLEN, CB_GETLBTEXT) } else { (LB_GETCURSEL, LB_GETTEXTLEN, LB_GETTEXT) };
    let i = crate::wnd::send(c, cur, 0, 0);
    if i as i32 == -1 {
        return 0;
    }
    let l = crate::wnd::send(c, len, i, 0);
    if l as i32 <= 0 {
        return 0;
    }
    let mut t = vec![0u8; l as usize + 1];
    crate::wnd::send(c, get, i, t.as_mut_ptr() as u32);
    t.truncate(l as usize);
    let (out, is_dir) = if t.len() == 5 && t.starts_with(b"[-") && t.ends_with(b"-]") {
        (vec![t[2], b':'], true)
    } else if t.len() > 2 && t[0] == b'[' && t.ends_with(b"]") {
        let mut d = t[1..t.len() - 1].to_vec();
        d.push(b'\\');
        (d, true)
    } else {
        (t, false)
    };
    let k = out.len().min(n as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), buf as *mut u8, k);
        *(buf as *mut u8).add(k) = 0;
    }
    is_dir as i32
}

/// A wide string argument as a narrow one (None for a null pointer).
fn narrow_arg(p: u32) -> Option<Vec<u8>> {
    (p != 0).then(|| crate::z(&crate::narrow(unsafe { crate::wstr(p as *const u16) })))
}

/// Copy a narrow result back into a wide buffer of `n` characters.
fn widen_back(a: &[u8], p: u32, n: usize) {
    let w = crate::to_wide(unsafe { crate::cstr(a.as_ptr()) });
    let k = w.len().min(n.saturating_sub(1));
    unsafe {
        core::ptr::copy_nonoverlapping(w.as_ptr(), p as *mut u16, k);
        *(p as *mut u16).add(k) = 0;
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirListA(h: u32, p: u32, id: i32, s: i32, t: u32) -> i32 {
    dir_list(h, p, id, s, t, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirListComboBoxA(h: u32, p: u32, id: i32, s: i32, t: u32) -> i32 {
    dir_list(h, p, id, s, t, true)
}

fn dir_list_w(h: u32, p: u32, id: i32, s: i32, t: u32, combo: bool) -> i32 {
    let Some(mut a) = narrow_arg(p) else { return dir_list(h, 0, id, s, t, combo) };
    a.resize(a.len().max(260), 0);
    let r = dir_list(h, a.as_ptr() as u32, id, s, t, combo);
    let n = unsafe { crate::wstr(p as *const u16) }.len() + 1;
    widen_back(&a, p, n.max(13));
    r
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirListW(h: u32, p: u32, id: i32, s: i32, t: u32) -> i32 {
    dir_list_w(h, p, id, s, t, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirListComboBoxW(h: u32, p: u32, id: i32, s: i32, t: u32) -> i32 {
    dir_list_w(h, p, id, s, t, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirSelectExA(h: u32, b: u32, n: i32, id: i32) -> i32 {
    dir_select(h, b, n, id, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirSelectComboBoxExA(h: u32, b: u32, n: i32, id: i32) -> i32 {
    dir_select(h, b, n, id, true)
}

fn dir_select_w(h: u32, b: u32, n: i32, id: i32, combo: bool) -> i32 {
    if b == 0 || n <= 0 {
        return 0;
    }
    let a = vec![0u8; n as usize * 2 + 1];
    let r = dir_select(h, a.as_ptr() as u32, a.len() as i32, id, combo);
    widen_back(&a, b, n as usize);
    r
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirSelectExW(h: u32, b: u32, n: i32, id: i32) -> i32 {
    dir_select_w(h, b, n, id, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DlgDirSelectComboBoxExW(h: u32, b: u32, n: i32, id: i32) -> i32 {
    dir_select_w(h, b, n, id, true)
}
