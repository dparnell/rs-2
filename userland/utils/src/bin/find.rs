//! FIND: search for a string in files or standard input.
//! FIND [/V] [/C] [/N] [/I] "string" [files]
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use utils::*;
os2::entry!(main);
fn main(s: &os2::Startup) -> u32 {
    let raw: String = s.args().iter().map(|&b| b as char).collect();
    let (mut inv, mut count, mut num, mut nocase) = (false, false, false, false);
    let mut pat = None;
    let mut files = Vec::new();
    // The pattern is the first quoted argument.
    let mut rest = raw.as_str();
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix('/') {
            let opt = r.chars().next().map(|c| c.to_ascii_uppercase());
            match opt {
                Some('V') => inv = true,
                Some('C') => count = true,
                Some('N') => num = true,
                Some('I') => nocase = true,
                _ => {}
            }
            rest = &r[1.min(r.len())..];
            continue;
        }
        if pat.is_none() {
            if let Some(r) = rest.strip_prefix('"') {
                if let Some(e) = r.find('"') {
                    pat = Some(String::from(&r[..e]));
                    rest = &r[e + 1..];
                    continue;
                }
            }
        }
        break;
    }
    files.extend(split(rest));
    let Some(pat) = pat else {
        eprint("FIND: parameter format not correct\r\n");
        return 2;
    };
    let needle = if nocase { pat.to_ascii_uppercase() } else { pat.clone() };
    let mut found = false;
    let mut search = |name: Option<&str>, data: &[u8]| {
        if let Some(n) = name {
            outln("");
            outln(&alloc::format!("---------- {n}"));
        }
        let mut c = 0;
        for (i, l) in lines(data).iter().enumerate() {
            let hay = if nocase { l.to_ascii_uppercase() } else { l.clone() };
            if hay.contains(needle.as_str()) != inv {
                c += 1;
                found = true;
                if !count {
                    if num {
                        out(&alloc::format!("[{}]", i + 1));
                    }
                    outln(l);
                }
            }
        }
        if count {
            outln(&alloc::format!("{c}"));
        }
    };
    if files.is_empty() {
        let d = read_all(0);
        search(None, &d);
    } else {
        for f in &files {
            match open_read(f) {
                Ok(h) => {
                    let d = read_all(h);
                    unsafe { os2::DosClose(h) };
                    search(Some(f), &d);
                }
                Err(_) => eprint(&alloc::format!("File not found - {f}\r\n")),
            }
        }
    }
    if found { 0 } else { 1 }
}
