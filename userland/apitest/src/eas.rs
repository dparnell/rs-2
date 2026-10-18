//! Extended attributes: set, query, enumerate and delete on files and
//! directories (FEA2/GEA2 lists, DosEnumAttribute, DosFindFirst levels 2
//! and 3, DosCopy and DosMove).

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::null_mut;

use os2::*;

use crate::check;

/// An FEA2LIST from (flags, name, value) triples.
fn fea2(eas: &[(u8, &[u8], &[u8])]) -> Vec<u8> {
    let mut b = vec![0u8; 4];
    let mut last = None;
    for (f, n, v) in eas {
        let start = b.len();
        if let Some(l) = last {
            let d = (start - l) as u32;
            b[l..l + 4].copy_from_slice(&d.to_le_bytes());
        }
        last = Some(start);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.push(*f);
        b.push(n.len() as u8);
        b.extend_from_slice(&(v.len() as u16).to_le_bytes());
        b.extend_from_slice(n);
        b.push(0);
        b.extend_from_slice(v);
        while b.len() % 4 != 0 {
            b.push(0);
        }
    }
    let n = b.len() as u32;
    b[0..4].copy_from_slice(&n.to_le_bytes());
    b
}

/// A GEA2LIST of names.
fn gea2(names: &[&[u8]]) -> Vec<u8> {
    let mut b = vec![0u8; 4];
    let mut last = None;
    for n in names {
        let start = b.len();
        if let Some(l) = last {
            let d = (start - l) as u32;
            b[l..l + 4].copy_from_slice(&d.to_le_bytes());
        }
        last = Some(start);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.push(n.len() as u8);
        b.extend_from_slice(n);
        b.push(0);
        while b.len() % 4 != 0 {
            b.push(0);
        }
    }
    let n = b.len() as u32;
    b[0..4].copy_from_slice(&n.to_le_bytes());
    b
}

/// Decode an FEA2LIST into (flags, name, value).
fn parse_fea2(b: &[u8]) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    let mut v = Vec::new();
    let cb = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    if cb <= 4 {
        return v;
    }
    let mut i = 4;
    loop {
        let next = u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let nl = b[i + 5] as usize;
        let vl = u16::from_le_bytes([b[i + 6], b[i + 7]]) as usize;
        let name = b[i + 8..i + 8 + nl].to_vec();
        let value = b[i + 9 + nl..i + 9 + nl + vl].to_vec();
        v.push((b[i + 4], name, value));
        if next == 0 {
            return v;
        }
        i += next;
    }
}

fn z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().collect();
    v.push(0);
    v
}

fn set_path(path: &[u8], list: &mut [u8]) -> (u32, u32) {
    let mut op = EaOp2 { gea2: null_mut(), fea2: list.as_mut_ptr(), error: 0 };
    let rc = unsafe { DosSetPathInfo(path.as_ptr(), FIL_QUERYEASIZE, &mut op as *mut _ as *const u8, 12, 0) };
    (rc, op.error)
}

/// DosQueryPathInfo level 3 for `names`: (rc, decoded list).
fn query_path(path: &[u8], names: &[&[u8]], room: u32) -> (u32, Vec<(u8, Vec<u8>, Vec<u8>)>) {
    let mut g = gea2(names);
    let mut out = vec![0u8; room.max(4) as usize];
    out[0..4].copy_from_slice(&room.to_le_bytes());
    let mut op = EaOp2 { gea2: g.as_mut_ptr(), fea2: out.as_mut_ptr(), error: 0 };
    let rc = unsafe { DosQueryPathInfo(path.as_ptr(), FIL_QUERYEASFROMLIST, &mut op as *mut _ as *mut u8, 12) };
    (rc, if rc == 0 { parse_fea2(&out) } else { Vec::new() })
}

/// cbList from DosQueryPathInfo level 2 (FILESTATUS4).
fn ea_size(path: &[u8]) -> u32 {
    let mut b = [0u8; 28];
    let rc = unsafe { DosQueryPathInfo(path.as_ptr(), FIL_QUERYEASIZE, b.as_mut_ptr(), 28) };
    if rc != 0 {
        return u32::MAX;
    }
    u32::from_le_bytes([b[24], b[25], b[26], b[27]])
}

/// DosEnumAttribute by path: the names (and value lengths) of all EAs.
fn enum_path(path: &[u8]) -> (u32, Vec<(Vec<u8>, u16)>) {
    let mut buf = vec![0u8; 4096];
    let mut count = u32::MAX;
    let rc = unsafe {
        DosEnumAttribute(ENUMEA_REFTYPE_PATH, path.as_ptr(), 1, buf.as_mut_ptr(), buf.len() as u32, &mut count, ENUMEA_LEVEL_NO_VALUE)
    };
    let mut v = Vec::new();
    if rc != 0 {
        return (rc, v);
    }
    let mut i = 0;
    for _ in 0..count {
        let next = u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]) as usize;
        let nl = buf[i + 5] as usize;
        let vl = u16::from_le_bytes([buf[i + 6], buf[i + 7]]);
        v.push((buf[i + 8..i + 8 + nl].to_vec(), vl));
        if next == 0 {
            break;
        }
        i += next;
    }
    (rc, v)
}

fn ascii(text: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&EAT_ASCII.to_le_bytes());
    v.extend_from_slice(&(text.len() as u16).to_le_bytes());
    v.extend_from_slice(text);
    v
}

pub fn run() {
    let mut drives = vec![String::from("C:")];
    // And the test disks made by `cargo xtask disk` (labels RS2DISK and
    // RS2HPFS; other drives, such as QEMU's virtual FAT, are left alone).
    for d in b'D'..=b'Z' {
        let mut fsi: FsInfo = unsafe { core::mem::zeroed() };
        let rc = unsafe { DosQueryFSInfo((d - b'A' + 1) as u32, FSIL_VOLSER, &mut fsi as *mut _ as *mut u8, 17) };
        let label = &fsi.label[..(fsi.cch_label as usize).min(11)];
        if rc == 0 && (label == b"RS2DISK" || label == b"RS2HPFS") {
            drives.push(format!("{}:", d as char));
        }
    }
    for d in drives {
        on_drive(&d);
    }
}

fn on_drive(d: &str) {
    let file = z(&format!("{d}\\EATEST.TMP"));
    let copy = z(&format!("{d}\\EACOPY.TMP"));
    let moved = z(&format!("{d}\\EADIR\\EAMOVED.TMP"));
    let dir = z(&format!("{d}\\EADIR"));
    let t = |s: &str| format!("{d} {s}");
    unsafe {
        DosDelete(file.as_ptr());
        DosDelete(copy.as_ptr());
        DosDelete(moved.as_ptr());
        DosDeleteDir(dir.as_ptr());
        // EAs given at creation.
        let ty = ascii(b"Plain Text");
        let mut l = fea2(&[(0, b".TYPE", &ty)]);
        let mut op = EaOp2 { gea2: null_mut(), fea2: l.as_mut_ptr(), error: 0 };
        let mut hf = 0;
        let mut action = 0;
        let rc = DosOpen(
            file.as_ptr(),
            &mut hf,
            &mut action,
            0,
            FILE_NORMAL,
            OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
            OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE,
            &mut op as *mut _ as *mut u8,
        );
        check(&t("DosOpen with EAs"), rc == 0 && action == 2);
        let mut n = 0;
        DosWrite(hf, b"data".as_ptr(), 4, &mut n);
        check(&t("EA size at creation"), ea_size(&file) == 4 + ((9 + 5 + ty.len() as u32 + 3) & !3));
        // Set more through the path; names are stored upper-case.
        let long = ascii(b"A long name for an 8.3 file");
        let big: Vec<u8> = (0..40_000u32).map(|i| (i * 7) as u8).collect();
        let mut l = fea2(&[(0, b".longname", &long), (FEA_NEEDEA, b"rs2.test", b"critical"), (0, b"BIG", &big)]);
        check(&t("DosSetPathInfo level 2"), set_path(&file, &mut l).0 == 0);
        let (rc, names) = enum_path(&file);
        let want: [(&[u8], u16); 4] = [(b".TYPE", ty.len() as u16), (b".LONGNAME", long.len() as u16), (b"RS2.TEST", 8), (b"BIG", 40_000)];
        check(
            &t("DosEnumAttribute path"),
            rc == 0 && names.len() == 4 && want.iter().all(|w| names.iter().any(|n| n.0 == w.0 && n.1 == w.1)),
        );
        // Query a subset by name, case-insensitively; a missing name comes
        // back with an empty value.
        let (rc, got) = query_path(&file, &[b".longname", b"RS2.Test", b"NOSUCH"], 1024);
        check(
            &t("DosQueryPathInfo level 3"),
            rc == 0
                && got.len() == 3
                && got[0].1 == b".LONGNAME"
                && got[0].2 == long
                && got[1].0 == FEA_NEEDEA
                && got[1].2 == b"critical"
                && got[2].1 == b"NOSUCH"
                && got[2].2.is_empty(),
        );
        let (rc, got) = query_path(&file, &[b"BIG"], 40_100);
        check(&t("large EA value"), rc == 0 && got.len() == 1 && got[0].2 == big);
        check(&t("level 3 buffer too small"), query_path(&file, &[b"BIG"], 1000).0 == 111);
        // All EAs through the handle (level 4) and the handle forms.
        let mut out = vec![0u8; 50_000];
        out[0..4].copy_from_slice(&50_000u32.to_le_bytes());
        let mut op = EaOp2 { gea2: null_mut(), fea2: out.as_mut_ptr(), error: 0 };
        let rc = DosQueryFileInfo(hf, FIL_QUERYALLEAS, &mut op as *mut _ as *mut u8, 12);
        check(&t("DosQueryFileInfo level 4"), rc == 0 && parse_fea2(&out).len() == 4);
        let mut b = [0u8; 28];
        let rc = DosQueryFileInfo(hf, FIL_QUERYEASIZE, b.as_mut_ptr(), 28);
        check(&t("DosQueryFileInfo level 2"), rc == 0 && u32::from_le_bytes([b[24], b[25], b[26], b[27]]) == ea_size(&file));
        // Delete one EA (zero-length value) through the handle.
        let mut l = fea2(&[(0, b"big", b"")]);
        let op = EaOp2 { gea2: null_mut(), fea2: l.as_mut_ptr(), error: 0 };
        check(&t("DosSetFileInfo level 2 delete"), DosSetFileInfo(hf, FIL_QUERYEASIZE, &op as *const _ as *const u8, 12) == 0);
        let mut ehf = [0u8; 4];
        ehf.copy_from_slice(&hf.to_le_bytes());
        let mut buf = [0u8; 512];
        let mut count = u32::MAX;
        let rc = DosEnumAttribute(ENUMEA_REFTYPE_FHANDLE, ehf.as_ptr(), 1, buf.as_mut_ptr(), 512, &mut count, 1);
        check(&t("DosEnumAttribute handle"), rc == 0 && count == 3);
        let mut count = 1;
        let rc = DosEnumAttribute(ENUMEA_REFTYPE_FHANDLE, ehf.as_ptr(), 3, buf.as_mut_ptr(), 512, &mut count, 1);
        check(&t("DosEnumAttribute from entry 3"), rc == 0 && count == 1 && &buf[8..16] == b"RS2.TEST");
        DosClose(hf);
        // Bad requests.
        let mut l = fea2(&[(0, b"BAD*NAME", b"x")]);
        check(&t("ERROR_INVALID_EA_NAME"), set_path(&file, &mut l) == (254, 4));
        let mut l = fea2(&[(0, b"A", b"x"), (0, b"B", b"y")]);
        l[4..8].copy_from_slice(&200u32.to_le_bytes());
        check(&t("ERROR_EA_LIST_INCONSISTENT"), set_path(&file, &mut l).0 == 255);
        // DosFindFirst level 2 (cbList) and level 3 (EAs per entry).
        let spec = z(&format!("{d}\\EATEST.*"));
        let mut fb = [0u8; 600];
        let mut h = HDIR_CREATE;
        let mut cnt = 1;
        let rc = DosFindFirst(spec.as_ptr(), &mut h, 0, fb.as_mut_ptr(), 600, &mut cnt, FIL_QUERYEASIZE);
        DosFindClose(h);
        check(&t("DosFindFirst level 2"), rc == 0 && u32::from_le_bytes([fb[28], fb[29], fb[30], fb[31]]) == ea_size(&file));
        let mut g = gea2(&[b".TYPE"]);
        let mut fb = vec![0u8; 600];
        let op = EaOp2 { gea2: g.as_mut_ptr(), fea2: null_mut(), error: 0 };
        core::ptr::copy_nonoverlapping(&op as *const _ as *const u8, fb.as_mut_ptr(), 12);
        let mut h = HDIR_CREATE;
        let mut cnt = 1;
        let rc = DosFindFirst(spec.as_ptr(), &mut h, 0, fb.as_mut_ptr(), 600, &mut cnt, FIL_QUERYEASFROMLIST);
        DosFindClose(h);
        // EAOP2, then oNext, FILESTATUS3 (24 bytes), the FEA2LIST, the name.
        let list = &fb[12 + 28..];
        let got = parse_fea2(list);
        let cb = u32::from_le_bytes([list[0], list[1], list[2], list[3]]) as usize;
        check(
            &t("DosFindFirst level 3"),
            rc == 0 && cnt == 1 && got.len() == 1 && got[0].2 == ty && &list[cb + 1..cb + 11] == b"EATEST.TMP",
        );
        let mut h = HDIR_CREATE;
        let mut cnt = 1;
        let rc = DosFindFirst(spec.as_ptr(), &mut h, 0, fb.as_mut_ptr(), 60, &mut cnt, FIL_QUERYEASFROMLIST);
        DosFindClose(h);
        check(&t("ERROR_EAS_DIDNT_FIT"), rc == 275 && cnt == 1);
        // DosCopy copies EAs, DosMove keeps them.
        check(&t("DosCopy"), DosCopy(file.as_ptr(), copy.as_ptr(), 0) == 0);
        let (_, names) = enum_path(&copy);
        check(&t("DosCopy copies EAs"), names.len() == 3 && ea_size(&copy) == ea_size(&file));
        // Directories: EAs at creation, set, query, delete.
        let mut l = fea2(&[(0, b".SUBJECT", &ascii(b"folder"))]);
        let op = EaOp2 { gea2: null_mut(), fea2: l.as_mut_ptr(), error: 0 };
        check(&t("DosCreateDir with EAs"), DosCreateDir(dir.as_ptr(), &op as *const _ as *mut u8) == 0);
        let mut l = fea2(&[(0, b".COMMENTS", &ascii(b"a directory comment"))]);
        check(&t("directory DosSetPathInfo"), set_path(&dir, &mut l).0 == 0);
        let (rc, names) = enum_path(&dir);
        check(&t("directory EAs"), rc == 0 && names.len() == 2);
        check(&t("DosMove"), DosMove(copy.as_ptr(), moved.as_ptr()) == 0);
        let (rc, got) = query_path(&moved, &[b".LONGNAME"], 256);
        check(&t("DosMove keeps EAs"), rc == 0 && got.len() == 1 && got[0].2 == long);
        // Remove all EAs: cbList back to 4.
        let mut l = fea2(&[(0, b".TYPE", b""), (0, b".LONGNAME", b""), (0, b"RS2.TEST", b"")]);
        check(&t("remove all EAs"), set_path(&file, &mut l).0 == 0 && ea_size(&file) == 4 && enum_path(&file).1.is_empty());
        check(&t("delete files with EAs"), DosDelete(moved.as_ptr()) == 0 && DosDelete(file.as_ptr()) == 0);
        check(&t("delete directory with EAs"), DosDeleteDir(dir.as_ptr()) == 0);
    }
}
