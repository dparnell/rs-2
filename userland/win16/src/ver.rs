//! VER: version information (the RT_VERSION resource of NE files: a
//! tree of blocks, each a length, a value length, a key and its value,
//! then its children, aligned to 4 bytes), and the installer's helpers
//! VerFindFile and VerInstallFile.

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::res::{self, Name};

const RT_VERSION: u16 = 16;

/// The version resource of a file.
fn version_of(path: &[u8]) -> Option<Vec<u8>> {
    let file = crate::module::read_file(&crate::compat::find_program(path)?, usize::MAX)?;
    let (off, len) = res::list_type(&file, RT_VERSION).first().copied().or_else(|| res::find_in_file(&file, RT_VERSION, &Name::Id(1)))?;
    Some(file[off..off + len].to_vec())
}

fn w(b: &[u8], o: usize) -> usize {
    res::w16(b, o) as usize
}

fn align4(o: usize) -> usize {
    (o + 3) & !3
}

/// A block at `o`: (its end, its key, where its value is and how long,
/// where its children start).
fn block(b: &[u8], o: usize) -> Option<(usize, &[u8], usize, usize, usize)> {
    let len = w(b, o);
    if len < 4 || o + len > b.len() {
        return None;
    }
    let vlen = w(b, o + 2);
    let key_end = b[o + 4..].iter().position(|&c| c == 0).map(|i| o + 4 + i)?;
    let key = &b[o + 4..key_end];
    let value = align4(key_end + 1);
    let children = align4(value + vlen);
    Some((o + len, key, value, vlen, children))
}

/// VerQueryValue's path ("\\", "\\StringFileInfo\\040904E4\\FileVersion"):
/// the value's offset and length in the block.
fn query(b: &[u8], path: &[u8]) -> Option<(usize, usize)> {
    let (mut end, _, mut value, mut vlen, mut child) = block(b, 0)?;
    for part in path.split(|&c| c == b'\\').filter(|p| !p.is_empty()) {
        let mut o = child;
        let mut found = None;
        while o < end {
            let Some(bl) = block(b, o) else { break };
            if bl.1.eq_ignore_ascii_case(part) {
                found = Some(bl);
                break;
            }
            o = align4(bl.0);
        }
        let f = found?;
        (end, value, vlen, child) = (f.0, f.2, f.3, f.4);
    }
    Some((value, vlen))
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // GetFileResourceSize(file, type, id, lpdwOffset),
        // GetFileResource(file, type, id, offset, len, data): a resource of
        // an NE file (VS_FILE_INFO is RT_VERSION).
        6302 | 6303 => {
            let (f, t, id) = (a.l(), a.l(), a.l());
            let Some(path) = crate::far_str(f) else { return Some(0) };
            if crate::tracing() {
                crate::trace(format_args!("W16 version resource of {}", path.iter().map(|&c| c as char).collect::<alloc::string::String>()));
            }
            let Some(file) = crate::compat::find_program(path).and_then(|p| crate::module::read_file(&p, usize::MAX)) else { return Some(0) };
            let ty = Name::of(t).unwrap_or(Name::Id(RT_VERSION));
            let name = Name::of(id).unwrap_or(Name::Id(1));
            let found = match (&ty, &name) {
                (Name::Id(t), _) => res::find_in_file(&file, *t, &name).or_else(|| res::list_type(&file, *t).first().copied()),
                _ => None,
            };
            let Some((off, len)) = found else { return Some(0) };
            if ord == 6302 {
                let p = a.p();
                if !p.is_null() {
                    unsafe { (p as *mut u32).write_unaligned(off as u32) };
                }
                len as u32
            } else {
                let (_o, n, d) = (a.l(), a.l(), a.p());
                if d.is_null() {
                    return Some(0);
                }
                let k = len.min(n as usize);
                unsafe { core::ptr::copy_nonoverlapping(file[off..].as_ptr(), d, k) };
                1
            }
        }
        // GetFileVersionInfoSize(file, lpdwHandle)
        6306 => {
            let (f, h) = (a.l(), a.p());
            if !h.is_null() {
                unsafe { (h as *mut u32).write_unaligned(0) };
            }
            crate::far_str(f).and_then(version_of).map_or(0, |v| v.len() as u32)
        }
        // GetFileVersionInfo(file, handle, len, data)
        6307 => {
            let (f, _h, len, d) = (a.l(), a.l(), a.l(), a.p());
            let Some(v) = crate::far_str(f).and_then(version_of) else { return Some(0) };
            if d.is_null() {
                return Some(0);
            }
            let n = v.len().min(len as usize);
            unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), d, n) };
            1
        }
        // VerQueryValue(block, path, lplpBuffer, lpuLen)
        6311 => {
            let (blk, p, out, lenp) = (a.l(), a.l(), a.p(), a.p());
            let b = thunk16::flat(blk);
            if b.is_null() {
                return Some(0);
            }
            let total = unsafe { (b as *const u16).read_unaligned() } as usize;
            let data = unsafe { core::slice::from_raw_parts(b, total) };
            let Some((o, n)) = query(data, crate::far_str(p).unwrap_or(b"\\")) else { return Some(0) };
            unsafe {
                if !out.is_null() {
                    (out as *mut u32).write_unaligned(blk + o as u32);
                }
                if !lenp.is_null() {
                    (lenp as *mut u16).write_unaligned(n as u16);
                }
            }
            (n != 0) as u32
        }
        // VerLanguageName(lang, buf, n)
        6310 => {
            let (l, buf, n) = (a.w(), a.p(), a.w());
            let name: &[u8] = match l & 0x3ff {
                0x09 => b"English",
                0x07 => b"German",
                0x0c => b"French",
                0x0a => b"Spanish",
                0x10 => b"Italian",
                0x11 => b"Japanese",
                _ => b"Language Neutral",
            };
            crate::user::copy_out(name, buf, n as i32) as u32
        }
        // VerFindFile(flags, file, windir, appdir, curdir, lpuCurLen,
        // destdir, lpuDestLen): the file goes to the application's
        // directory (or the Windows directory with VFFF_ISSHAREDFILE);
        // where it is now, if anywhere.
        6308 => {
            let (fl, f, _win, app, cur, curlen, dest, destlen) = (a.w(), a.l(), a.l(), a.l(), a.p(), a.p(), a.p(), a.p());
            let name = crate::far_str(f).unwrap_or(b"");
            let to: Vec<u8> = if fl & 1 != 0 { crate::profile::WINDOWS_DIR.to_vec() } else { crate::far_str(app).unwrap_or(crate::profile::WINDOWS_DIR).to_vec() };
            let here = [&to[..], b"\\", name].concat();
            let curdir: &[u8] = if crate::module::read_file(&here, 1).is_some() { &to } else { b"" };
            let put = |p: *mut u8, lenp: *mut u8, s: &[u8]| {
                if !p.is_null() && !lenp.is_null() {
                    let n = unsafe { (lenp as *const u16).read_unaligned() } as i32;
                    let k = crate::user::copy_out(s, p, n);
                    unsafe { (lenp as *mut u16).write_unaligned(k as u16 + 1) };
                }
            };
            put(cur, curlen, curdir);
            put(dest, destlen, &to);
            0
        }
        // VerInstallFile(flags, src, dest, srcdir, destdir, curdir,
        // tmpfile, lpuTmpLen): copy (expanding) the file into place.
        6309 => {
            let (_fl, s, d, sd, dd, _cd, _tmp, _tl) = (a.w(), a.l(), a.l(), a.l(), a.l(), a.l(), a.p(), a.p());
            let src = [crate::far_str(sd).unwrap_or(b"."), b"\\", crate::far_str(s).unwrap_or(b"")].concat();
            let dst = [crate::far_str(dd).unwrap_or(b"."), b"\\", crate::far_str(d).unwrap_or(b"")].concat();
            let Some(data) = crate::module::read_file(&src, usize::MAX) else { return Some(0x0001_0000) }; // VIF_CANNOTREADSRC
            let data = crate::lz::expand(&data).unwrap_or(data);
            let mut z = dst.clone();
            z.push(0);
            let (mut hf, mut action, mut n) = (0u32, 0u32, 0u32);
            if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
                return Some(0x0000_0800); // VIF_CANNOTCREATE
            }
            let ok = unsafe { os2::DosWrite(hf, data.as_ptr(), data.len() as u32, &mut n) } == 0;
            unsafe { os2::DosClose(hf) };
            if ok { 0 } else { 0x0000_4000 } // VIF_OUTOFSPACE
        }
        _ => return None,
    })
}
