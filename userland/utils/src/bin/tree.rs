//! TREE: show the directory structure. TREE [drive:][path] [/F]
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use os2::*;
use utils::*;
os2::entry!(main);
fn walk(dir: &str, files: bool, depth: usize) {
    let base = alloc::format!("{}\\*", dir.trim_end_matches('\\'));
    for e in find(&base, FILE_DIRECTORY) {
        let indent: String = core::iter::repeat_n("    ", depth).collect();
        if e.attr & FILE_DIRECTORY != 0 {
            outln(&alloc::format!("{indent}{}", e.name));
            walk(&alloc::format!("{}\\{}", dir.trim_end_matches('\\'), e.name), files, depth + 1);
        } else if files {
            outln(&alloc::format!("{indent}  {}", e.name));
        }
    }
}
fn main(s: &os2::Startup) -> u32 {
    let mut files = false;
    let mut root = String::from(".");
    for a in args(s) {
        if a.eq_ignore_ascii_case("/F") {
            files = true;
        } else {
            root = a;
        }
    }
    outln(&alloc::format!("Directory tree of {root}"));
    walk(&root, files, 0);
    0
}
