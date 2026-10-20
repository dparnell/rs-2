//! Files: reading and writing them whole, full path names, and the
//! .TYPE extended attribute (a multi-valued list of ASCII types).

use alloc::vec::Vec;

use os2::*;

use crate::pm::z;

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

/// The types in a file's .TYPE EA (empty: none).
pub fn types(path: &[u8]) -> Vec<Vec<u8>> {
    let p = z(path);
    // GEA2LIST: cbList, then one GEA2 (oNextEntryOffset, cbName, szName).
    let mut gea = Vec::new();
    gea.extend_from_slice(&0u32.to_le_bytes());
    gea.extend_from_slice(&0u32.to_le_bytes());
    gea.push(5);
    gea.extend_from_slice(b".TYPE\0");
    let n = gea.len() as u32;
    gea[0..4].copy_from_slice(&n.to_le_bytes());
    let mut fea = alloc::vec![0u8; 2048];
    let cb = fea.len() as u32;
    fea[0..4].copy_from_slice(&cb.to_le_bytes());
    let mut op = EaOp2 { gea2: gea.as_mut_ptr(), fea2: fea.as_mut_ptr(), error: 0 };
    let rc = unsafe { DosQueryPathInfo(p.as_ptr(), FIL_QUERYEASFROMLIST, &mut op as *mut EaOp2 as *mut u8, core::mem::size_of::<EaOp2>() as u32) };
    if rc != 0 {
        return Vec::new();
    }
    // FEA2LIST: cbList, FEA2 (oNext, fEA, cbName, cbValue, szName, value).
    let name_len = fea[9] as usize;
    let value_len = u16::from_le_bytes([fea[10], fea[11]]) as usize;
    let at = 12 + name_len + 1;
    if value_len == 0 || at + value_len > fea.len() {
        return Vec::new();
    }
    parse_types(&fea[at..at + value_len])
}

/// The ASCII strings of an EA value (EAT_MVMT of EAT_ASCII, or one
/// EAT_ASCII).
fn parse_types(v: &[u8]) -> Vec<Vec<u8>> {
    let u16_at = |i: usize| v.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let mut out = Vec::new();
    match u16_at(0) {
        Some(EAT_MVMT) => {
            let count = u16_at(4).unwrap_or(0) as usize;
            let mut i = 6;
            for _ in 0..count {
                let (Some(t), Some(n)) = (u16_at(i), u16_at(i + 2)) else { break };
                let n = n as usize;
                if t == EAT_ASCII && i + 4 + n <= v.len() {
                    out.push(v[i + 4..i + 4 + n].to_vec());
                }
                i += 4 + n;
            }
        }
        Some(EAT_ASCII) => {
            let n = u16_at(2).unwrap_or(0) as usize;
            if 4 + n <= v.len() {
                out.push(v[4..4 + n].to_vec());
            }
        }
        _ => {}
    }
    out.retain(|t| !t.is_empty());
    out
}

/// Set a file's .TYPE EA to one type.
pub fn set_type(path: &[u8], ty: &[u8]) -> u32 {
    let p = z(path);
    let mut value = Vec::new();
    value.extend_from_slice(&EAT_MVMT.to_le_bytes());
    value.extend_from_slice(&0u16.to_le_bytes());
    value.extend_from_slice(&1u16.to_le_bytes());
    value.extend_from_slice(&EAT_ASCII.to_le_bytes());
    value.extend_from_slice(&(ty.len() as u16).to_le_bytes());
    value.extend_from_slice(ty);
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
