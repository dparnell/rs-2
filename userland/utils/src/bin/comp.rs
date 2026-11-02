//! COMP: compare files byte by byte. COMP [file1] [file2]
//!
//! As OS/2's: wildcards in the first name compare each match with the
//! file of the same name (DosEditName) in the second, which may also be a
//! directory; up to ten differences are listed per pair (offset and the
//! two bytes, in hex); files of different sizes are compared up to the
//! shorter length if the user agrees. Afterwards it asks whether to
//! compare more files.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::*;
use utils::files::*;
use utils::msgs::{say, text, Class};
use utils::*;
os2::entry!(main);

fn read_line(n: u32, fallback: &str) -> String {
    out(&text(n, &[], Class::Info, fallback));
    ask("")
}

/// Compare one pair.
fn compare(a: &str, b: &str) {
    say(1170, &[&full_path(a), &full_path(b)], "\r\nComparing %1 with %2.\r\n");
    let (da, db) = match (read_file(a), read_file(b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(rc), _) | (_, Err(rc)) => {
            error(rc);
            return;
        }
    };
    out("\r\n");
    if da.len() != db.len() {
        let go = yes_no(1169, &[], "The two files have different sizes.  Compare them\r\nanyway (Y/N)? ");
        out("\r\n");
        if !go {
            return;
        }
    }
    let n = da.len().min(db.len());
    let mut bad = 0;
    for i in 0..n {
        if da[i] != db[i] {
            if bad == 10 {
                say(1165, &[], "Ten or more differences were found; the comparison\r\nof these files has stopped.\r\n");
                out("\r\n");
                return;
            }
            bad += 1;
            out(&text(1168, &[], Class::Info, "Difference found at OFFSET "));
            outln(&alloc::format!("{i:X}"));
            out(&text(1167, &[], Class::Info, "Byte in file 1 = "));
            outln(&alloc::format!("{:X}", da[i]));
            out("\r\n");
            out(&text(1166, &[], Class::Info, "Byte in file 2 = "));
            outln(&alloc::format!("{:X}", db[i]));
            out("\r\n");
        }
    }
    if bad == 0 {
        if da.len() == db.len() {
            say(1164, &[], "The files are the same.\r\n");
        } else {
            say(1179, &[], "The files are the same up to the shorter length.\r\n");
        }
        out("\r\n");
    }
}

/// Compare `first` (may have wildcards) with `second`.
fn run(first: &str, second: &str) -> u32 {
    let wild = first.contains(['*', '?']);
    let matches: Vec<String> = if wild {
        let dir = dir_of(first);
        find(first, FILE_READONLY | FILE_ARCHIVED).into_iter().filter(|e| e.attr & FILE_DIRECTORY == 0).map(|e| alloc::format!("{dir}{}", e.name)).collect()
    } else if exists(first) {
        alloc::vec![String::from(first)]
    } else {
        Vec::new()
    };
    if matches.is_empty() {
        msgs::err(2, &[], "The file cannot be found.\r\n");
        return 2;
    }
    for a in matches {
        let name = base_name(&a);
        let b = if second.ends_with(['\\', ':']) || is_dir(second) {
            join(second, name)
        } else if second.contains(['*', '?']) {
            let dir = dir_of(second);
            alloc::format!("{dir}{}", edit_name(name, &second[dir.len()..]))
        } else {
            String::from(second)
        };
        compare(&a, &b);
    }
    0
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.iter().any(|x| x == "/?") {
        say(3094, &[], "COMP compares the contents of two files or sets of files.\r\nSyntax:\r\n  COMP [drive:][path][filename] [drive:][path][filename]\r\n\r\nWildcards in the names compare sets of files.\r\n");
        return 0;
    }
    let mut first = a.first().cloned();
    let mut second = a.get(1).cloned();
    let mut rc;
    loop {
        let f = match first.take() {
            Some(f) => f,
            None => read_line(1162, "Name of the first file:\r\n"),
        };
        let s2 = match second.take() {
            Some(s) => s,
            None => read_line(1163, "Name of the second file:\r\n"),
        };
        if f.is_empty() || s2.is_empty() {
            return 0;
        }
        rc = run(&f, &s2);
        if !yes_no(1161, &[], "Compare more files (Y/N)? ") {
            out("\r\n");
            break;
        }
        out("\r\n");
    }
    rc
}
