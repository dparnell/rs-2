//! SORT: sort standard input. SORT [/R] [/+n]
#![no_std]
#![no_main]
extern crate alloc;
use utils::*;
os2::entry!(main);
fn main(s: &os2::Startup) -> u32 {
    let (mut rev, mut col) = (false, 0usize);
    for a in args(s) {
        let up = a.to_ascii_uppercase();
        if up == "/R" {
            rev = true;
        } else if let Some(n) = up.strip_prefix("/+") {
            col = n.parse::<usize>().unwrap_or(1).saturating_sub(1);
        }
    }
    let mut v = lines(&read_all(0));
    v.sort_by(|a, b| {
        let ka = a.get(col..).unwrap_or("").to_ascii_uppercase();
        let kb = b.get(col..).unwrap_or("").to_ascii_uppercase();
        ka.cmp(&kb)
    });
    if rev {
        v.reverse();
    }
    for l in v {
        outln(&l);
    }
    0
}
