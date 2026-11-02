//! UTILTEST: checks of rs2's base utilities, run as child programs with
//! their output captured. `UTILTEST [drive:]` uses the drive for the
//! backup set and scratch files (default: the drive labelled RS2DISK, the
//! test disk `cargo xtask disk` makes); the system drive holds the test
//! tree \UTILTEST. Prints PASS/FAIL per check and a summary line.
//!
//! Checks look at results (files, EAs, return codes, numbers in the
//! output), not at message texts, which come from OSO001.MSG.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use os2::path::cstr;
use os2::*;
use utils::files::*;
use utils::*;
os2::entry!(main);

static PASSED: AtomicU32 = AtomicU32::new(0);
static FAILED: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        PASSED.fetch_add(1, Ordering::Relaxed);
        outln(&alloc::format!("PASS {name}"));
    } else {
        FAILED.fetch_add(1, Ordering::Relaxed);
        outln(&alloc::format!("FAIL {name}"));
    }
}

/// Run `prog args` with `input` on its standard input; returns its exit
/// code and everything it wrote to standard output and error.
fn run(prog: &str, args: &str, input: &str) -> (u32, String) {
    let (mut ir, mut iw, mut or, mut ow) = (0u32, 0u32, 0u32, 0u32);
    unsafe {
        DosCreatePipe(&mut ir, &mut iw, 4096);
        DosCreatePipe(&mut or, &mut ow, 65536);
    }
    let data = os2::text_bytes(input);
    let _ = write_all(iw, &data);
    unsafe { DosClose(iw) };
    // Save our standard handles, put the pipes in their place.
    let mut saved = [u32::MAX; 3];
    for (i, s) in saved.iter_mut().enumerate() {
        unsafe { DosDupHandle(i as u32, s) };
    }
    let mut h;
    unsafe {
        h = 0;
        DosDupHandle(ir, &mut h);
        h = 1;
        DosDupHandle(ow, &mut h);
        h = 2;
        DosDupHandle(ow, &mut h);
        DosClose(ir);
        DosClose(ow);
    }
    let mut argv: Vec<u8> = Vec::new();
    argv.extend(os2::text_bytes(prog).iter());
    argv.push(0);
    argv.extend(os2::text_bytes(args).iter());
    argv.push(0);
    argv.push(0);
    let name = cstr(prog);
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    // EXEC_ASYNCRESULT: read the output while the child runs.
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 2, argv.as_ptr(), core::ptr::null(), &mut res, name.as_ptr()) };
    let child = res.code_terminate;
    for (i, s) in saved.iter().enumerate() {
        let mut t = i as u32;
        unsafe {
            DosDupHandle(*s, &mut t);
            DosClose(*s);
        }
    }
    if rc != 0 {
        unsafe { DosClose(or) };
        return (rc | 0x10000, String::new());
    }
    let out = read_all(or);
    unsafe { DosClose(or) };
    let mut pid = 0;
    let mut r = ResultCodes { code_terminate: 0, code_result: 0 };
    unsafe { DosWaitChild(0, 0, &mut r, &mut pid, child) };
    (r.code_result, out.iter().map(|&b| b as char).collect())
}

fn util(name: &str) -> String {
    alloc::format!("{}:\\OS2\\{name}.EXE", boot_drive())
}

fn boot_drive() -> char {
    let mut d = 3u32;
    unsafe { DosQuerySysInfo(5, 5, &mut d, 4) };
    (b'A' + d as u8 - 1) as char
}

fn put(path: &str, text: &str) {
    let _ = write_file(path, &os2::text_bytes(text));
}

fn ea(name: &str, text: &str) -> Ea {
    let mut v = alloc::vec![0xfd, 0xff];
    v.extend_from_slice(&(text.len() as u16).to_le_bytes());
    v.extend_from_slice(text.as_bytes());
    Ea { flags: 0, name: Vec::from(name.as_bytes()), value: v }
}

fn has_ea(path: &str, name: &str, text: &str) -> bool {
    read_eas(path).is_ok_and(|l| l.iter().any(|e| e.name == name.as_bytes() && e.value.ends_with(text.as_bytes())))
}

fn rmtree(path: &str) {
    for f in find_all(&join(path, "*"), FILE_HIDDEN | FILE_SYSTEM | FILE_READONLY | FILE_DIRECTORY) {
        let p = join(path, &f.name);
        if f.attr & FILE_DIRECTORY != 0 {
            rmtree(&p);
        } else {
            set_info(&p, 0, 0, 0);
            let c = cstr(&p);
            unsafe { DosForceDelete(c.as_ptr()) };
        }
    }
    let c = cstr(path);
    unsafe { DosDeleteDir(c.as_ptr()) };
}

/// The drive with the given volume label.
fn drive_labelled(label: &str) -> Option<char> {
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut cur, &mut map) };
    (2..26u8).filter(|d| map & (1 << d) != 0).find(|&d| volume_label(d).is_ok_and(|(l, _)| l.eq_ignore_ascii_case(label))).map(|d| (b'A' + d) as char)
}

fn comp(root: &str) {
    put(&join(root, "A.TXT"), "hello world one\r\n");
    put(&join(root, "B.TXT"), "hello world two\r\n");
    put(&join(root, "C.TXT"), "hello world one\r\n");
    let (rc, o) = run(&util("COMP"), &alloc::format!("{root}\\A.TXT {root}\\B.TXT"), "n\r\n");
    check("COMP lists the differing bytes", rc == 0 && o.contains("= 6F") && o.contains("= 74") && o.contains("OFFSET C"));
    let (_, o) = run(&util("COMP"), &alloc::format!("{root}\\A.TXT {root}\\C.TXT"), "n\r\n");
    check("COMP equal files", !o.contains("OFFSET"));
}

fn backup_restore(root: &str, target: char) {
    rmtree(&alloc::format!("{target}:\\BACKUP"));
    let sub = join(root, "SUB");
    make_dirs(&sub);
    put(&join(root, "A.TXT"), "hello world one\r\n");
    put(&join(&sub, "S.TXT"), "sub file\r\n");
    write_eas(&join(root, "A.TXT"), &[ea(".SUBJECT", "hello")]);
    write_eas(&sub, &[ea(".COMMENTS", "dir")]);
    let (rc, _) = run(&util("BACKUP"), &alloc::format!("{root}\\*.* {target}: /S"), "\r\n");
    check("BACKUP returns 0", rc == 0);
    let ctl = read_file(&alloc::format!("{target}:\\BACKUP\\CONTROL.001")).ok().and_then(|d| os2_backup::Control::parse(&d).ok());
    check("BACKUP writes IBM's control file", ctl.as_ref().is_some_and(|c| c.dirs.iter().any(|(d, f)| d.path.ends_with("UTILTEST\\SUB") && f.iter().any(|f| f.name == "S.TXT") && d.ea_len > 0)));
    // Restore after deleting.
    rmtree(&sub);
    let a = join(root, "A.TXT");
    let c = cstr(&a);
    unsafe { DosDelete(c.as_ptr()) };
    let (rc, o) = run(&util("RESTORE"), &alloc::format!("{target}: {root}\\*.* /S"), "");
    check("RESTORE returns 0", rc == 0 && o.contains("\\S.TXT"));
    check("RESTORE brings back the data", read_file(&a).ok().as_deref() == Some(&b"hello world one\r\n"[..]) && read_file(&join(&sub, "S.TXT")).ok().as_deref() == Some(&b"sub file\r\n"[..]));
    check("RESTORE brings back file and directory EAs", has_ea(&a, ".SUBJECT", "hello") && has_ea(&sub, ".COMMENTS", "dir"));
    let (rc, _) = run(&util("RESTORE"), &alloc::format!("{target}: {root}\\*.* /S /N"), "");
    check("RESTORE /N finds nothing missing", rc == 1);
}

fn pack_unpack(root: &str, target: char) {
    let src = join(root, "P.TXT");
    let text = "packed text packed text packed text\r\n".repeat(40);
    put(&src, &text);
    write_eas(&src, &[ea(".TYPE", "Plain Text")]);
    let pk = alloc::format!("{target}:\\P.PK");
    let (rc, _) = run(&util("PACK"), &alloc::format!("{src} {pk} /H:\\PK\\"), "");
    let size = status(&pk).map_or(0, |s| s.cb_file);
    check("PACK compresses", rc == 0 && size > 0 && (size as usize) < text.len() / 2);
    let (_, o) = run(&util("UNPACK"), &alloc::format!("{pk} /SHOW"), "");
    check("UNPACK /SHOW shows the stored path", o.contains("->\\PK\\P.TXT"));
    let (rc, _) = run(&util("UNPACK"), &alloc::format!("{pk} {target}:"), "");
    let out = alloc::format!("{target}:\\PK\\P.TXT");
    check("UNPACK restores data and EAs", rc == 0 && read_file(&out).ok() == Some(os2::text_bytes(&text).into_owned()) && has_ea(&out, ".TYPE", "Plain Text"));
}

fn eautil(root: &str) {
    let f = join(root, "E.TXT");
    put(&f, "ea file\r\n");
    write_eas(&f, &[ea(".SUBJECT", "split me")]);
    let hold = join(root, "E.EA");
    let (rc, _) = run(&util("EAUTIL"), &alloc::format!("{f} {hold} /S"), "");
    let held = read_file(&hold).unwrap_or_default();
    check("EAUTIL /S moves the EAs to a FEALIST hold file", rc == 0 && !has_ea(&f, ".SUBJECT", "split me") && os2_backup::parse_fealist(&held).is_ok_and(|l| l.len() == 1));
    let (rc, _) = run(&util("EAUTIL"), &alloc::format!("{f} {hold} /J"), "");
    check("EAUTIL /J joins them back", rc == 0 && has_ea(&f, ".SUBJECT", "split me") && !exists(&hold));
}

fn replace(root: &str) {
    let to = join(root, "R");
    make_dirs(&to);
    put(&join(root, "N1.TXT"), "new one\r\n");
    put(&join(&to, "N1.TXT"), "old one\r\n");
    put(&join(root, "N2.TXT"), "new two\r\n");
    let (rc, _) = run(&util("REPLACE"), &alloc::format!("{root}\\N?.TXT {to} /A"), "");
    check("REPLACE /A adds only missing files", rc == 0 && exists(&join(&to, "N2.TXT")) && read_file(&join(&to, "N1.TXT")).ok().as_deref() == Some(&b"old one\r\n"[..]));
    let (rc, _) = run(&util("REPLACE"), &alloc::format!("{root}\\N1.TXT {to}"), "");
    check("REPLACE replaces", rc == 0 && read_file(&join(&to, "N1.TXT")).ok().as_deref() == Some(&b"new one\r\n"[..]));
}

fn patch(root: &str) {
    let f = join(root, "PATCHME.BIN");
    let _ = write_file(&f, &[0x10, 0x20, 0x30, 0x40]);
    let ctl = join(root, "FIX.PAT");
    put(&ctl, &alloc::format!("; test\r\nFILE {f}\r\nVER 1 20 30\r\nCHA 2 AA\r\n"));
    let (rc, _) = run(&util("PATCH"), &alloc::format!("{ctl} /A"), "");
    check("PATCH /A applies a verified change", rc == 0 && read_file(&f).ok().as_deref() == Some(&[0x10, 0x20, 0xaa, 0x40][..]));
    put(&ctl, &alloc::format!("FILE {f}\r\nVER 1 99\r\nCHA 0 00\r\n"));
    let (rc, _) = run(&util("PATCH"), &alloc::format!("{ctl} /A"), "");
    check("PATCH /A refuses when VER does not match", rc != 0 && read_file(&f).ok().as_deref() == Some(&[0x10, 0x20, 0xaa, 0x40][..]));
}

fn levels(root: &str) {
    let f = join(root, "SIG.DAT");
    put(&f, "junk\0@#rs2:1.02#@Test component\0more");
    let (rc, o) = run(&util("BLDLEVEL"), &f, "");
    check("BLDLEVEL finds the signature", rc == 0 && o.contains("rs2") && o.contains("1.2") && o.contains("Test component"));
    let d = join(root, "LVL");
    make_dirs(&d);
    let mut s = alloc::vec![0u8; 0xa5];
    s[0] = 0xff;
    s[1] = 0xff;
    s[2..10].copy_from_slice(b"SYSLEVEL");
    s[0x0f] = 1;
    s[0x21] = 0x25;
    s[0x28] = 0x45;
    s[0x29] = 2;
    s[0x2c..0x34].copy_from_slice(b"XR04502_");
    s[0x34..0x3c].copy_from_slice(b"XR04502_");
    s[0x3c..0x3c + 13].copy_from_slice(b"Test product\0");
    s[0x8c..0x95].copy_from_slice(b"5639A6101");
    let _ = write_file(&join(&d, "SYSLEVEL.TST"), &s);
    let (rc, o) = run(&util("SYSLEVEL"), &d, "");
    check("SYSLEVEL reads a SYSLEVEL file", rc == 0 && o.contains("Test product") && o.contains("5639A6101") && o.contains("XR04502") && o.contains("4.52"));
}

fn system() {
    let (rc, o) = run(&util("PSTAT"), "/C", "");
    check("PSTAT /C lists this program", rc == 0 && o.contains("UTILTEST.EXE"));
    let (rc, o) = run(&util("PSTAT"), "/L", "");
    check("PSTAT /L lists libraries", rc == 0 && o.contains("DOSCALLS"));
    let (rc, o) = run(&util("CACHE"), "", "");
    check("CACHE shows the HPFS parameters", rc == 0 && o.contains("1000") && o.contains("5000"));
    let (rc, _) = run(&util("SETBOOT"), "/Q", "");
    check("SETBOOT /Q", rc == 0);
    let (rc, o) = run(&util("SETBOOT"), "/X:1", "");
    check("SETBOOT Boot Manager options", rc == 1 && !o.is_empty());
    let (rc, _) = run(&util("ANSI"), "ON", "");
    check("ANSI ON", rc == 0);
    let (rc, o) = run(&util("LVM"), "/QUERY", "");
    check("LVM /QUERY lists the disks", rc == 0 && o.contains("Drive 1"));
    let (rc, _) = run(&util("UNDELETE"), "", "");
    check("UNDELETE without DELDIR", rc == 1);
}

fn diskettes(root: &str) {
    // A 1.44 MB diskette image with a boot sector BPB.
    let mut img = alloc::vec![0u8; 1440 * 1024];
    img[11..13].copy_from_slice(&512u16.to_le_bytes());
    img[19..21].copy_from_slice(&2880u16.to_le_bytes());
    img[24..26].copy_from_slice(&18u16.to_le_bytes());
    img[26..28].copy_from_slice(&2u16.to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xaa;
    for (i, b) in img.iter_mut().enumerate().skip(512) {
        *b = (i % 251) as u8;
    }
    let (a, b) = (join(root, "A.IMG"), join(root, "B.IMG"));
    let _ = write_file(&a, &img);
    let (rc, o) = run(&util("DISKCOPY"), &alloc::format!("{a} {b}"), "");
    check("DISKCOPY copies an image", rc == 0 && o.contains("80") && o.contains("18") && read_file(&b).ok().as_deref() == Some(&img[..]));
    let (rc, _) = run(&util("DISKCOMP"), &alloc::format!("{a} {b}"), "");
    check("DISKCOMP finds them equal", rc == 0);
    img[100_000] ^= 1;
    let _ = write_file(&b, &img);
    let (rc, o) = run(&util("DISKCOMP"), &alloc::format!("{a} {b}"), "");
    check("DISKCOMP reports the differing track", rc == 1 && o.contains(" 2"));
    let f = join(root, "RECOVER.TXT");
    put(&f, "recover me\r\n");
    let (rc, o) = run(&util("RECOVER"), &f, "");
    check("RECOVER keeps a readable file", rc == 0 && o.contains("12") && read_file(&f).ok().as_deref() == Some(&b"recover me\r\n"[..]));
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let target = match a.first() {
        Some(d) if d.len() == 2 && d.ends_with(':') => d.as_bytes()[0].to_ascii_uppercase() as char,
        _ => match drive_labelled("RS2DISK").or_else(|| drive_labelled("RS2HPFS")) {
            Some(d) => d,
            None => {
                outln("UTILTEST: no test drive (label RS2DISK); give one as UTILTEST d:");
                return 1;
            }
        },
    };
    let root = alloc::format!("{}:\\UTILTEST", boot_drive());
    rmtree(&root);
    make_dirs(&root);
    outln(&alloc::format!("UTILTEST: tree {root}, backup drive {target}:"));
    comp(&root);
    backup_restore(&root, target);
    pack_unpack(&root, target);
    eautil(&root);
    replace(&root);
    patch(&root);
    levels(&root);
    system();
    diskettes(&root);
    let (p, f) = (PASSED.load(Ordering::Relaxed), FAILED.load(Ordering::Relaxed));
    outln(&alloc::format!("UTILTEST: {p} passed, {f} failed"));
    rmtree(&root);
    if f == 0 { 0 } else { 1 }
}
