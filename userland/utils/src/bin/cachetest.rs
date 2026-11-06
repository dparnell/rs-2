//! CACHETEST: tests of the block cache (docs/ARCHITECTURE.md).
//!
//! CACHETEST MODEL d:\dir   random reads and writes of every size against
//!                          a copy in memory
//! CACHETEST MARK d:\dir    MARKA.DAT committed by DosResetBuffer, MARKB.DAT
//!                          only written, MARKC.DAT opened write-through:
//!                          a look at the disk image tells which arrived
//! CACHETEST NAMES d:\dir   random file and directory changes, each path
//!                          asked for twice in both cases against a model
//! CACHETEST SHUTDOWN       DosShutdown
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use os2::dos::*;
use os2::path::cstr;
use os2::APIRET;
use utils::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "DOSCALLS.254"]
    fn DosResetBuffer(hf: u32) -> APIRET;
    #[link_name = "DOSCALLS.415"]
    fn DosShutdown(reserved: u32) -> APIRET;
}

const REPLACE_OR_CREATE: u32 = 0x12;
const READWRITE_DENYNONE: u32 = 0x42;
const WRITE_THROUGH: u32 = 0x4000;
const SIZE: usize = 1 << 20;
const ROUNDS: u32 = 3000;

fn open(path: &str, flags: u32) -> u32 {
    let name = format!("{path}\0");
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe { DosOpen(name.as_ptr(), &mut hf, &mut act, 0, 0, REPLACE_OR_CREATE, READWRITE_DENYNONE | flags, core::ptr::null_mut()) };
    if rc != 0 {
        outln(&format!("open {path}: {rc}"));
    }
    hf
}

fn reopen(path: &str) -> u32 {
    let name = format!("{path}\0");
    let (mut hf, mut act) = (0, 0);
    unsafe { DosOpen(name.as_ptr(), &mut hf, &mut act, 0, 0, 1, READWRITE_DENYNONE, core::ptr::null_mut()) };
    hf
}

fn write_at(hf: u32, at: usize, b: &[u8]) -> bool {
    let (mut pos, mut n) = (0, 0);
    unsafe { DosSetFilePtr(hf, at as i32, 0, &mut pos) == 0 && DosWrite(hf, b.as_ptr(), b.len() as u32, &mut n) == 0 && n as usize == b.len() }
}

fn read_at(hf: u32, at: usize, b: &mut [u8]) -> bool {
    let (mut pos, mut n) = (0, 0);
    unsafe { DosSetFilePtr(hf, at as i32, 0, &mut pos) == 0 && DosRead(hf, b.as_mut_ptr(), b.len() as u32, &mut n) == 0 && n as usize == b.len() }
}

struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 >> 8
    }
}

fn model(dir: &str) -> u32 {
    let path = format!("{dir}\\CACHET.DAT");
    let mut r = Lcg(0x1234);
    let mut copy: Vec<u8> = (0..SIZE).map(|_| r.next() as u8).collect();
    let mut hf = open(&path, 0);
    if !write_at(hf, 0, &copy) {
        outln("first write failed");
        return 1;
    }
    // Under a sector, a sector, a block, either side of the largest
    // request the cache keeps, and well past it.
    const LENGTHS: [usize; 9] = [17, 512, 1000, 4096, 5000, 65536, 65537, 70000, 200000];
    let mut b = vec![0u8; 200000];
    let mut bad = 0;
    for round in 0..ROUNDS {
        let len = LENGTHS[r.next() as usize % LENGTHS.len()];
        let at = r.next() as usize % (SIZE - len);
        match r.next() % 16 {
            0 => {
                unsafe { DosResetBuffer(hf) };
            }
            1 => {
                unsafe { DosClose(hf) };
                hf = reopen(&path);
            }
            2..=8 => {
                for x in &mut b[..len] {
                    *x = r.next() as u8;
                }
                copy[at..at + len].copy_from_slice(&b[..len]);
                if !write_at(hf, at, &b[..len]) {
                    outln(&format!("round {round}: write of {len} at {at} failed"));
                    bad += 1;
                }
            }
            _ => {
                if !read_at(hf, at, &mut b[..len]) || b[..len] != copy[at..at + len] {
                    outln(&format!("round {round}: read of {len} at {at} differs"));
                    bad += 1;
                }
            }
        }
    }
    unsafe { DosClose(hf) };
    hf = reopen(&path);
    let mut all = vec![0u8; SIZE];
    if !read_at(hf, 0, &mut all) || all != copy {
        outln("the file differs after closing it");
        bad += 1;
    }
    unsafe { DosClose(hf) };
    outln(&format!("cachetest model {dir}: {ROUNDS} rounds, {bad} failure(s)"));
    bad
}

fn marked(c: char) -> Vec<u8> {
    format!("RS2CACHE-MARK-{c}-").repeat(512).into_bytes()
}

fn mark(dir: &str) -> u32 {
    let a = open(&format!("{dir}\\MARKA.DAT"), 0);
    write_at(a, 0, &marked('A'));
    let rc = unsafe { DosResetBuffer(a) };
    let c = open(&format!("{dir}\\MARKC.DAT"), WRITE_THROUGH);
    write_at(c, 0, &marked('C'));
    // Last: a commit writes out the whole volume.
    let b = open(&format!("{dir}\\MARKB.DAT"), 0);
    write_at(b, 0, &marked('B'));
    for h in [a, b, c] {
        unsafe { DosClose(h) };
    }
    outln(&format!("cachetest mark {dir}: reset buffer {rc}"));
    0
}

/// What is in the tree: upper-case paths below `dir`; directories end in `\\`.
fn names(dir: &str) -> u32 {
    use alloc::collections::BTreeSet;
    use utils::files::{make_dirs, status};
    make_dirs(dir);
    let mut seed = 2026u32;
    let mut rnd = move |n: u32| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) % n
    };
    let comps = ["AA", "BB", "CC.TXT", "DIR1", "DIR2"];
    let mut made: BTreeSet<String> = BTreeSet::new();
    let (mut bad, mut steps) = (0, 0);
    let exists = |m: &BTreeSet<String>, p: &str| m.contains(p) || m.contains(&format!("{p}\\"));
    for _ in 0..ROUNDS {
        let mut rel = String::from(comps[rnd(5) as usize]);
        for _ in 0..rnd(3) {
            rel = format!("{rel}\\{}", comps[rnd(5) as usize]);
        }
        let (parent, _) = rel.rsplit_once('\\').unwrap_or(("", &rel));
        let parent_ok = parent.is_empty() || made.contains(&format!("{parent}\\"));
        let full = cstr(&format!("{dir}\\{rel}"));
        match rnd(5) {
            0 if parent_ok && !exists(&made, &rel) => {
                if unsafe { DosCreateDir(full.as_ptr(), core::ptr::null_mut()) } == 0 {
                    made.insert(format!("{rel}\\"));
                }
            }
            1 if parent_ok && !exists(&made, &rel) => {
                if let Ok(h) = utils::files::create(&format!("{dir}\\{rel}"), 0) {
                    unsafe { DosClose(h) };
                    made.insert(rel.clone());
                }
            }
            2 if made.contains(&rel) => {
                if unsafe { DosDelete(full.as_ptr()) } == 0 {
                    made.remove(&rel);
                }
            }
            3 if made.contains(&format!("{rel}\\")) => {
                let prefix = format!("{rel}\\");
                if !made.iter().any(|x| x.len() > prefix.len() && x.starts_with(&prefix)) && unsafe { DosDeleteDir(full.as_ptr()) } == 0 {
                    made.remove(&prefix);
                }
            }
            4 if exists(&made, &rel) => {
                // Somewhere free in the same directory, with what is below it.
                let last = comps[rnd(5) as usize];
                let to = if parent.is_empty() { String::from(last) } else { format!("{parent}\\{last}") };
                let target = cstr(&format!("{dir}\\{to}"));
                if !exists(&made, &to) && unsafe { DosMove(full.as_ptr(), target.as_ptr()) } == 0 {
                    let was = format!("{rel}\\");
                    let moved: Vec<String> = made.iter().filter(|x| **x == rel || x.starts_with(&was)).cloned().collect();
                    for x in moved {
                        made.remove(&x);
                        made.insert(format!("{to}{}", &x[rel.len()..]));
                    }
                }
            }
            _ => {}
        }
        for _ in 0..4 {
            let mut p = String::from(comps[rnd(5) as usize]);
            for _ in 0..rnd(3) {
                p = format!("{p}\\{}", comps[rnd(5) as usize]);
            }
            let want = exists(&made, &p);
            for q in [format!("{dir}\\{p}"), format!("{dir}\\{p}").to_ascii_lowercase(), format!("{dir}\\{p}")] {
                steps += 1;
                if status(&q).is_some() != want {
                    bad += 1;
                    outln(&format!("cachetest names: {q}: {}", if want { "gone" } else { "still there" }));
                }
            }
        }
    }
    outln(&format!("cachetest names {dir}: {steps} lookups, {bad} failure(s)"));
    bad
}

fn main(s: &os2::Startup) -> u32 {
    let a: Vec<String> = args(s);
    match (a.first().map(|x| x.to_ascii_uppercase()).as_deref(), a.get(1)) {
        (Some("MODEL"), Some(d)) => model(d),
        (Some("MARK"), Some(d)) => mark(d),
        (Some("NAMES"), Some(d)) => names(d),
        (Some("SHUTDOWN"), _) => {
            let rc = unsafe { DosShutdown(0) };
            outln(&format!("cachetest shutdown: {rc}"));
            rc
        }
        _ => {
            outln("CACHETEST MODEL dir | MARK dir | NAMES dir | SHUTDOWN");
            1
        }
    }
}
