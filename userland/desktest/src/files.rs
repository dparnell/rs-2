//! The shell's file operations (rs2shell/src/fileops.rs): a tree with
//! EAs on a file and a directory copied (each file reported), moved on
//! the drive, measured, a folder refused inside itself, free names for
//! the name collision dialog, deleting the trees.

use alloc::vec::Vec;

use crate::check;
use crate::fileops::{self, join};
use crate::pm::z;

pub fn write(path: &[u8], data: &[u8]) -> bool {
    let p = z(path);
    let (mut hf, mut action) = (0u32, 0u32);
    // Create or replace; write only, deny none.
    let rc = unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut action, 0, 0, 0x12, 0x0041, core::ptr::null_mut()) };
    if rc != 0 {
        return false;
    }
    let mut n = 0;
    unsafe {
        os2::DosWrite(hf, data.as_ptr(), data.len() as u32, &mut n);
        os2::DosClose(hf);
    }
    n as usize == data.len()
}

pub fn read(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut action) = (0u32, 0u32);
    // Open existing; read only, deny none.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x0040, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut buf = alloc::vec![0u8; 4096];
    let mut n = 0;
    unsafe {
        os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n);
        os2::DosClose(hf);
    }
    buf.truncate(n as usize);
    Some(buf)
}

/// Set one ASCII EA (FEA2LIST with one FEA2: EAT_ASCII value).
fn set_ea(path: &[u8], name: &[u8], value: &[u8]) -> u32 {
    let mut fea = Vec::new();
    fea.extend_from_slice(&0u32.to_le_bytes());
    fea.extend_from_slice(&0u32.to_le_bytes());
    fea.push(0);
    fea.push(name.len() as u8);
    fea.extend_from_slice(&((value.len() + 4) as u16).to_le_bytes());
    fea.extend_from_slice(name);
    fea.push(0);
    fea.extend_from_slice(&0xfffdu16.to_le_bytes());
    fea.extend_from_slice(&(value.len() as u16).to_le_bytes());
    fea.extend_from_slice(value);
    let n = fea.len() as u32;
    fea[0..4].copy_from_slice(&n.to_le_bytes());
    let op = os2::EaOp2 { gea2: core::ptr::null_mut(), fea2: fea.as_mut_ptr(), error: 0 };
    let p = z(path);
    unsafe { os2::DosSetPathInfo(p.as_ptr(), 2, &op as *const _ as *const u8, core::mem::size_of::<os2::EaOp2>() as u32, 0) }
}

/// The EA names of a path.
fn ea_names(path: &[u8]) -> Vec<Vec<u8>> {
    let p = z(path);
    let mut buf = alloc::vec![0u8; 4096];
    let mut count = 0xffff_ffffu32;
    let mut out = Vec::new();
    if unsafe { os2::DosEnumAttribute(1, p.as_ptr(), 1, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) } != 0 {
        return out;
    }
    let mut at = 0usize;
    for _ in 0..count {
        let next = u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]) as usize;
        let n = buf[at + 5] as usize;
        out.push(buf[at + 8..at + 8 + n].to_vec());
        if next == 0 {
            break;
        }
        at += next;
    }
    out
}

fn mkdir(p: &[u8]) -> bool {
    let c = z(p);
    unsafe { os2::DosCreateDir(c.as_ptr(), core::ptr::null_mut()) == 0 }
}

pub fn checks() {
    let (a, b, c) = (&b"C:\\DTCOPY"[..], &b"C:\\DTCOPY2"[..], &b"C:\\DTCOPY3"[..]);
    for p in [a, b, c] {
        fileops::delete_path(p);
    }
    let ok = mkdir(a) && mkdir(&join(a, b"SUB")) && write(&join(a, b"ONE.TXT"), b"one") && write(&join(a, b"SUB\\TWO.TXT"), b"two two");
    check("a tree to copy", ok);
    check("an EA on a file", set_ea(&join(a, b"ONE.TXT"), b".SUBJECT", b"test file") == 0);
    check("an EA on a directory", set_ea(&join(a, b"SUB"), b".LONGNAME", b"Sub folder") == 0);
    let t = fileops::measure(a);
    check("measure: files and bytes of a tree", t.files == 2 && t.bytes == 10);
    let mut seen = Vec::new();
    let rc = fileops::copy_tree(a, b, false, &mut |f, size| {
        seen.push((f.to_vec(), size));
        true
    });
    check("copy_tree", rc == 0);
    check("each file reported with its size", seen.len() == 2 && seen.iter().any(|(f, s)| f.ends_with(b"TWO.TXT") && *s == 7));
    check("the copies' contents", read(&join(b, b"ONE.TXT")).as_deref() == Some(&b"one"[..]) && read(&join(b, b"SUB\\TWO.TXT")).as_deref() == Some(&b"two two"[..]));
    check("the file's EA came along (DosCopy)", ea_names(&join(b, b"ONE.TXT")).iter().any(|n| n == b".SUBJECT"));
    check("the directory's EA came along", ea_names(&join(b, b"SUB")).iter().any(|n| n == b".LONGNAME"));
    check("a copy onto an existing name fails", fileops::copy_tree(a, b, false, &mut |_, _| true) != 0);
    check("... and replaces it when asked", fileops::copy_tree(&join(a, b"ONE.TXT"), &join(b, b"SUB\\TWO.TXT"), true, &mut |_, _| true) == 0 && read(&join(b, b"SUB\\TWO.TXT")).as_deref() == Some(&b"one"[..]));
    check("a folder is not copied into itself", fileops::copy_tree(a, &join(a, b"SUB\\INNER"), false, &mut |_, _| true) != 0);
    let mut stopped = 0;
    let rc = fileops::copy_tree(a, c, false, &mut |_, _| {
        stopped += 1;
        false
    });
    check("a copy stops when the progress says so", rc == fileops::ERROR_INTERRUPT && stopped == 1);
    fileops::delete_path(c);
    check("move_path on the drive (DosMove)", fileops::move_path(b, c, false, &mut |_, _| true) == 0 && !fileops::exists(b) && fileops::is_dir(c) && ea_names(&join(c, b"SUB")).iter().any(|n| n == b".LONGNAME"));
    check("is_within", fileops::is_within(b"C:\\A\\B", b"C:\\A") && !fileops::is_within(b"C:\\AB", b"C:\\A") && fileops::is_within(b"C:\\A", b"C:\\A\\"));
    let taken = |n: &[u8]| fileops::exists(&join(a, n));
    check("a free name on FAT for ONE.TXT", pmwin::drag::unique_name(b"ONE.TXT", false, taken) == b"ONE~2.TXT");
    check("delete_path of trees", fileops::delete_path(a) == 0 && fileops::delete_path(c) == 0 && !fileops::exists(a) && !fileops::exists(c));
}
