//! XCOPY: copy files and directory trees. XCOPY source [dest] [/S] [/E]
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::path::cstr;
use os2::*;
use utils::*;
os2::entry!(main);

fn copy_file(from: &str, to: &str) -> u32 {
    let Ok(src) = open_read(from) else { return 2 };
    let c = cstr(to);
    let (mut dst, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(c.as_ptr(), &mut dst, &mut act, 0, 0, OPEN_ACTION_REPLACE_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut())
    };
    if rc != 0 {
        unsafe { DosClose(src) };
        return rc;
    }
    let mut buf = alloc::vec![0u8; 32768];
    let mut result = 0;
    loop {
        let mut n = 0;
        if unsafe { DosRead(src, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        let mut w = 0;
        let r = unsafe { DosWrite(dst, buf.as_ptr(), n, &mut w) };
        if r != 0 || w != n {
            result = if r != 0 { r } else { 112 };
            break;
        }
    }
    unsafe {
        DosClose(src);
        DosClose(dst);
    }
    result
}

fn copy_tree(src_dir: &str, pattern: &str, dest: &str, sub: bool, empty: bool, count: &mut u32) -> u32 {
    let mut rc = 0;
    let files = find(&alloc::format!("{src_dir}{pattern}"), 0);
    if !files.is_empty() || empty {
        let c = cstr(dest.trim_end_matches('\\'));
        unsafe { DosCreateDir(c.as_ptr(), core::ptr::null_mut()) };
    }
    for f in files {
        let from = alloc::format!("{src_dir}{}", f.name);
        let to = alloc::format!("{}\\{}", dest.trim_end_matches('\\'), f.name);
        outln(&from);
        let r = copy_file(&from, &to);
        if r != 0 {
            eprint(&alloc::format!("Cannot copy {from} (error {r})\r\n"));
            rc = r;
        } else {
            *count += 1;
        }
    }
    if sub {
        let dirs: Vec<Entry> = find(&alloc::format!("{src_dir}*"), FILE_DIRECTORY).into_iter().filter(|e| e.attr & FILE_DIRECTORY != 0).collect();
        for d in dirs {
            let r = copy_tree(
                &alloc::format!("{src_dir}{}\\", d.name),
                pattern,
                &alloc::format!("{}\\{}", dest.trim_end_matches('\\'), d.name),
                sub,
                empty,
                count,
            );
            if r != 0 {
                rc = r;
            }
        }
    }
    rc
}

fn main(s: &os2::Startup) -> u32 {
    let (mut sub, mut empty) = (false, false);
    let mut paths: Vec<String> = Vec::new();
    for a in args(s) {
        match a.to_ascii_uppercase().as_str() {
            "/S" => sub = true,
            "/E" => empty = true,
            _ => paths.push(a),
        }
    }
    let Some(src) = paths.first().cloned() else {
        eprint("XCOPY: no source specified\r\n");
        return 4;
    };
    let dest = paths.get(1).cloned().unwrap_or_else(|| String::from("."));
    let (dir, pat) = if is_dir(&src) {
        (alloc::format!("{}\\", src.trim_end_matches('\\')), String::from("*"))
    } else {
        let d = dir_of(&src);
        let p = String::from(&src[d.len()..]);
        (d, p)
    };
    let mut count = 0;
    let rc = copy_tree(&dir, &pat, &dest, sub, empty, &mut count);
    outln(&alloc::format!("{count:>9} file(s) copied."));
    rc
}
