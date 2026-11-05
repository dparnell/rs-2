//! CACHETEST: tests of the block cache (docs/ARCHITECTURE.md).
//!
//! CACHETEST MODEL d:\dir   random reads and writes of every size against
//!                          a copy in memory
//! CACHETEST MARK d:\dir    MARKA.DAT committed by DosResetBuffer, MARKB.DAT
//!                          only written, MARKC.DAT opened write-through:
//!                          a look at the disk image tells which arrived
//! CACHETEST SHUTDOWN       DosShutdown
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use os2::dos::*;
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

fn main(s: &os2::Startup) -> u32 {
    let a: Vec<String> = args(s);
    match (a.first().map(|x| x.to_ascii_uppercase()).as_deref(), a.get(1)) {
        (Some("MODEL"), Some(d)) => model(d),
        (Some("MARK"), Some(d)) => mark(d),
        (Some("SHUTDOWN"), _) => {
            let rc = unsafe { DosShutdown(0) };
            outln(&format!("cachetest shutdown: {rc}"));
            rc
        }
        _ => {
            outln("CACHETEST MODEL dir | MARK dir | SHUTDOWN");
            1
        }
    }
}
