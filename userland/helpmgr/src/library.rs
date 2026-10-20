//! Help libraries and books: files found along HELP (help libraries) or
//! BOOKSHELF (books), read whole and kept parsed.

use crate::pm::*;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::ManuallyDrop;
use os2::dos::*;

/// An open IPF file.
pub struct Library {
    doc: ManuallyDrop<ipf::Doc<'static>>,
    data: *mut [u8],
    /// The file found.
    pub path: Vec<u8>,
    /// First TOC index of this file in the instance's numbering.
    pub base: u32,
}

impl Library {
    pub fn doc(&self) -> &ipf::Doc<'static> {
        &self.doc
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.doc);
            drop(Box::from_raw(self.data));
        }
    }
}

const SEARCH_CUR_DIRECTORY: u32 = 1;
const SEARCH_ENVIRONMENT: u32 = 2;
const SEARCH_IGNORENETERRS: u32 = 4;

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS, read-only, deny write.
    if unsafe { DosOpen(p.as_ptr(), &mut hf, &mut action, 0, 0, 0x0001, 0x0020, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut size = 0u32;
    unsafe { DosSetFilePtr(hf, 0, 2, &mut size) };
    unsafe { DosSetFilePtr(hf, 0, 0, &mut 0) };
    let mut buf = vec![0u8; size as usize];
    let mut got = 0u32;
    let rc = unsafe { DosRead(hf, buf.as_mut_ptr(), size, &mut got) };
    unsafe { DosClose(hf) };
    if rc != 0 || got != size {
        return None;
    }
    Some(buf)
}

fn has_ext(name: &[u8]) -> bool {
    let base = name.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(name);
    base.contains(&b'.')
}

/// Where file `name` is: as given if it has a path, else in the current
/// directory or along the environment variables `vars`.
fn find(name: &[u8], vars: &[&[u8]]) -> Option<Vec<u8>> {
    let mut buf = [0u8; 260];
    let n = z(name);
    if name.iter().any(|&c| c == b'\\' || c == b':' || c == b'/') {
        return read_file(name).map(|_| name.to_vec());
    }
    if unsafe { DosSearchPath(SEARCH_CUR_DIRECTORY, b"\0".as_ptr(), n.as_ptr(), buf.as_mut_ptr(), 260) } == 0 {
        return Some(unsafe { cstr(buf.as_ptr()) }.to_vec());
    }
    for v in vars {
        let var = z(v);
        if unsafe { DosSearchPath(SEARCH_ENVIRONMENT | SEARCH_IGNORENETERRS, var.as_ptr(), n.as_ptr(), buf.as_mut_ptr(), 260) } == 0 {
            return Some(unsafe { cstr(buf.as_ptr()) }.to_vec());
        }
    }
    None
}

/// Open help library or book `name` (with no extension: `.HLP`, or for
/// books `.INF`).
pub fn open(name: &[u8], book: bool) -> Result<Library, u32> {
    let mut tries: Vec<Vec<u8>> = Vec::new();
    if has_ext(name) {
        tries.push(name.to_vec());
    } else {
        let exts: &[&[u8]] = if book { &[b".INF", b".HLP"] } else { &[b".HLP", b".INF"] };
        for e in exts {
            let mut n = name.to_vec();
            n.extend_from_slice(e);
            tries.push(n);
        }
    }
    let vars: &[&[u8]] = if book { &[b"BOOKSHELF", b"HELP"] } else { &[b"HELP", b"BOOKSHELF"] };
    for t in &tries {
        let Some(path) = find(t, vars) else { continue };
        let Some(data) = read_file(&path) else { return Err(crate::HMERR_READ_LIB_FILE) };
        let data: &'static mut [u8] = Box::leak(data.into_boxed_slice());
        let raw = data as *mut [u8];
        match ipf::Doc::parse(data) {
            Ok(doc) if book || doc.is_help() || doc.header.kind == ipf::KIND_BOOK => {
                return Ok(Library { doc: ManuallyDrop::new(doc), data: raw, path, base: 0 });
            }
            _ => {
                unsafe { drop(Box::from_raw(raw)) };
                return Err(crate::HMERR_LOAD_DLL_BAD_FORMAT);
            }
        }
    }
    Err(crate::HMERR_OPEN_LIB_FILE)
}

/// The files of a library name list ("A.HLP B.HLP", "A+B").
pub fn names(list: &[u8]) -> Vec<Vec<u8>> {
    list.split(|&c| c == b' ' || c == b'+' || c == b',').filter(|n| !n.is_empty()).map(|n| n.to_vec()).collect()
}
