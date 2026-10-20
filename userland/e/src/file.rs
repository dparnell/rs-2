//! Files: reading and writing them whole (text ends with a Ctrl+Z
//! end-of-file mark, as OS/2's E writes it), full path names, and the
//! .TYPE extended attribute: how a file's types look to the editor, and
//! setting or extending them.

use alloc::vec::Vec;

use os2::*;

use crate::pm::z;

/// The end-of-file mark written after the text.
pub const EOF_MARK: u8 = 0x1a;

pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_OPEN_FAILED: u32 = 110;
pub const ERROR_EAS_NOT_SUPPORTED: u32 = 282;

/// The whole file, or the OS/2 error.
pub fn read(path: &[u8]) -> Result<Vec<u8>, u32> {
    let p = z(path);
    let (mut hf, mut action) = (0, 0);
    let rc = unsafe {
        DosOpen(p.as_ptr(), &mut hf, &mut action, 0, FILE_NORMAL, OPEN_ACTION_OPEN_IF_EXISTS | OPEN_ACTION_FAIL_IF_NEW, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut())
    };
    if rc != 0 {
        return Err(rc);
    }
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 32768];
    loop {
        let mut n = 0;
        let rc = unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 {
            unsafe { DosClose(hf) };
            return Err(rc);
        }
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(hf) };
    Ok(out)
}

/// A file's text: without its end-of-file mark.
pub fn text_of(mut data: Vec<u8>) -> Vec<u8> {
    if data.last() == Some(&EOF_MARK) {
        data.pop();
    }
    data
}

/// Write a file (replacing it), or the OS/2 error.
pub fn write(path: &[u8], data: &[u8]) -> Result<(), u32> {
    let p = z(path);
    let (mut hf, mut action) = (0, 0);
    let rc = unsafe {
        DosOpen(
            p.as_ptr(),
            &mut hf,
            &mut action,
            data.len() as u32,
            FILE_NORMAL,
            OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
            OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYREADWRITE,
            core::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    let mut done = 0usize;
    while done < data.len() {
        let mut n = 0;
        let rc = unsafe { DosWrite(hf, data[done..].as_ptr(), (data.len() - done) as u32, &mut n) };
        if rc != 0 || n == 0 {
            unsafe { DosClose(hf) };
            // ERROR_DISK_FULL when nothing more goes.
            return Err(if rc != 0 { rc } else { 112 });
        }
        done += n as usize;
    }
    let rc = unsafe { DosClose(hf) };
    if rc != 0 { Err(rc) } else { Ok(()) }
}

/// The fully qualified name of a path (as given if that fails).
pub fn full_name(path: &[u8]) -> Vec<u8> {
    let p = z(path);
    let mut buf = [0u8; 260];
    // FIL_QUERYFULLNAME
    let rc = unsafe { DosQueryPathInfo(p.as_ptr(), 5, buf.as_mut_ptr(), buf.len() as u32) };
    if rc != 0 {
        return path.to_vec();
    }
    crate::pm::until_nul(&buf).to_vec()
}

pub fn exists(path: &[u8]) -> bool {
    let p = z(path);
    let mut buf = [0u8; 64];
    // FIL_STANDARD
    unsafe { DosQueryPathInfo(p.as_ptr(), 1, buf.as_mut_ptr(), buf.len() as u32) == 0 }
}

/// How a file's .TYPE looks to the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeEa {
    /// The file is not there yet.
    NoFile,
    /// The file system keeps no extended attributes.
    Unsupported,
    /// No .TYPE.
    None,
    /// One of the editor's types is among them.
    Known,
    /// Types, none of them the editor's (they are kept).
    Unknown(Vec<Vec<u8>>),
    /// A .TYPE the editor cannot read (it is replaced).
    Malformed,
}

/// The .TYPE value of a file (empty: none).
pub fn type_value(path: &[u8]) -> Result<Vec<u8>, u32> {
    let p = z(path);
    // GEA2LIST: cbList, then one GEA2 (oNextEntryOffset, cbName, szName).
    let mut gea = Vec::new();
    gea.extend_from_slice(&0u32.to_le_bytes());
    gea.extend_from_slice(&0u32.to_le_bytes());
    gea.push(5);
    gea.extend_from_slice(b".TYPE\0");
    let n = gea.len() as u32;
    gea[0..4].copy_from_slice(&n.to_le_bytes());
    let mut fea = alloc::vec![0u8; 4096];
    let cb = fea.len() as u32;
    fea[0..4].copy_from_slice(&cb.to_le_bytes());
    let mut op = EaOp2 { gea2: gea.as_mut_ptr(), fea2: fea.as_mut_ptr(), error: 0 };
    let rc = unsafe { DosQueryPathInfo(p.as_ptr(), FIL_QUERYEASFROMLIST, &mut op as *mut EaOp2 as *mut u8, core::mem::size_of::<EaOp2>() as u32) };
    if rc != 0 {
        return Err(rc);
    }
    // FEA2LIST: cbList, FEA2 (oNext, fEA, cbName, cbValue, szName, value).
    let name_len = fea[9] as usize;
    let value_len = u16::from_le_bytes([fea[10], fea[11]]) as usize;
    let at = 12 + name_len + 1;
    if value_len == 0 || at + value_len > fea.len() {
        return Ok(Vec::new());
    }
    Ok(fea[at..at + value_len].to_vec())
}

/// The ASCII strings of a .TYPE value: Some(strings) for EAT_ASCII,
/// EAT_MVST of EAT_ASCII and EAT_MVMT whose entries are all EAT_ASCII;
/// None for anything else.
pub fn parse_types(v: &[u8]) -> Option<Vec<Vec<u8>>> {
    let u16_at = |i: usize| v.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let mut out = Vec::new();
    match u16_at(0)? {
        EAT_ASCII => {
            let n = u16_at(2)? as usize;
            out.push(v.get(4..4 + n)?.to_vec());
        }
        // Multi-valued, single type: codepage, count, the type, then
        // (length, bytes) entries.
        EAT_MVST => {
            let count = u16_at(4)? as usize;
            if u16_at(6)? != EAT_ASCII {
                return None;
            }
            let mut i = 8;
            for _ in 0..count {
                let n = u16_at(i)? as usize;
                out.push(v.get(i + 2..i + 2 + n)?.to_vec());
                i += 2 + n;
            }
        }
        // Multi-valued, multi-type: codepage, count, (type, length,
        // bytes) entries.
        EAT_MVMT => {
            let count = u16_at(4)? as usize;
            let mut i = 6;
            for _ in 0..count {
                if u16_at(i)? != EAT_ASCII {
                    return None;
                }
                let n = u16_at(i + 2)? as usize;
                out.push(v.get(i + 4..i + 4 + n)?.to_vec());
                i += 4 + n;
            }
        }
        _ => return None,
    }
    Some(out)
}

const EAT_MVST: u16 = 0xffde;

/// How the editor sees a file's types: `known` are the types it offers.
pub fn classify(path: &[u8], known: &[&[u8]]) -> Result<TypeEa, u32> {
    let v = match type_value(path) {
        Ok(v) => v,
        Err(ERROR_FILE_NOT_FOUND | ERROR_OPEN_FAILED | 3) => return Ok(TypeEa::NoFile),
        Err(ERROR_EAS_NOT_SUPPORTED) => return Ok(TypeEa::Unsupported),
        Err(rc) => return Err(rc),
    };
    if v.is_empty() {
        return Ok(TypeEa::None);
    }
    Ok(match parse_types(&v) {
        None => TypeEa::Malformed,
        Some(t) if t.is_empty() => TypeEa::Malformed,
        Some(t) if t.iter().any(|t| known.iter().any(|k| k == t)) => TypeEa::Known,
        Some(t) => TypeEa::Unknown(t),
    })
}

/// The types in a file's .TYPE EA (empty: none).
pub fn types(path: &[u8]) -> Vec<Vec<u8>> {
    type_value(path).ok().and_then(|v| parse_types(&v)).unwrap_or_default()
}

/// Set a file's .TYPE EA to these types (EAT_MVMT of EAT_ASCII).
pub fn set_types(path: &[u8], types: &[&[u8]]) -> u32 {
    let p = z(path);
    let mut value = Vec::new();
    value.extend_from_slice(&EAT_MVMT.to_le_bytes());
    value.extend_from_slice(&0u16.to_le_bytes());
    value.extend_from_slice(&(types.len() as u16).to_le_bytes());
    for t in types {
        value.extend_from_slice(&EAT_ASCII.to_le_bytes());
        value.extend_from_slice(&(t.len() as u16).to_le_bytes());
        value.extend_from_slice(t);
    }
    let mut fea = Vec::new();
    fea.extend_from_slice(&0u32.to_le_bytes());
    fea.extend_from_slice(&0u32.to_le_bytes());
    fea.push(0);
    fea.push(5);
    fea.extend_from_slice(&(value.len() as u16).to_le_bytes());
    fea.extend_from_slice(b".TYPE\0");
    fea.extend_from_slice(&value);
    let n = fea.len() as u32;
    fea[0..4].copy_from_slice(&n.to_le_bytes());
    let mut op = EaOp2 { gea2: core::ptr::null_mut(), fea2: fea.as_mut_ptr(), error: 0 };
    unsafe { DosSetPathInfo(p.as_ptr(), FIL_QUERYEASIZE, &mut op as *mut EaOp2 as *const u8, core::mem::size_of::<EaOp2>() as u32, 0) }
}

/// Set a file's .TYPE EA to one type.
pub fn set_type(path: &[u8], ty: &[u8]) -> u32 {
    set_types(path, &[ty])
}
