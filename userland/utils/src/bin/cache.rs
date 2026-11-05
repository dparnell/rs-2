//! CACHE: show or set the HPFS cache parameters.
//! CACHE [/DISKIDLE:n] [/MAXAGE:n] [/BUFFERIDLE:n] [/LAZY:ON|OFF|n] [/READAHEAD:ON|OFF|n]
//!
//! Through DosFSCtl to the HPFS file system: 0x9003 reads the parameter
//! block (DiskIdle, MaxAge, BufferIdle in milliseconds, the size of write
//! that goes past the cache in bytes, then words: DirtyMax, the cache size
//! in 2 KB buffers, the lazy-write and read-ahead worker counts),
//! 0x9002 sets it and 0x9001 stops lazy writing. Options may start with
//! `-` or `/`.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec::Vec;
use os2::APIRET;
use utils::msgs::say;
use utils::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "DOSCALLS.285"]
    fn DosFSCtl(data: *mut u8, cbdata: u32, pcbdata: *mut u32, parm: *mut u8, cbparm: u32, pcbparm: *mut u32, func: u32, route: *const u8, hfile: u32, method: u32) -> APIRET;
}

const FSCTL_FSDNAME: u32 = 3;

fn fsctl(func: u32, data: &mut [u8], parm: &mut [u8]) -> u32 {
    let (mut cd, mut cp) = (data.len() as u32, parm.len() as u32);
    let pd = if data.is_empty() { core::ptr::null_mut() } else { data.as_mut_ptr() };
    let pp = if parm.is_empty() { core::ptr::null_mut() } else { parm.as_mut_ptr() };
    unsafe { DosFSCtl(pd, data.len() as u32, &mut cd, pp, parm.len() as u32, &mut cp, func, b"HPFS\0".as_ptr(), u32::MAX, FSCTL_FSDNAME) }
}

fn u32at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn usage(prog: &str) {
    say(
        622,
        &[prog],
        "Syntax: %1 [OPTIONS]\r\nwhere OPTIONS are any of:\r\n -DISKIDLE:n         Wait n milliseconds of disk idle time before writing.\r\n -MAXAGE:n           Write cache buffers older than n milliseconds.\r\n -BUFFERIDLE:n       Write buffers idle for n milliseconds.\r\n -LAZY:ON|OFF|n      Lazy writing on, off, or with n lazy writers.\r\n -READAHEAD:ON|OFF|n Read-ahead on, off, or with n read-ahead workers.\r\n",
    );
}

fn main(s: &os2::Startup) -> u32 {
    let prog = "CACHE";
    let mut block = [0u8; 26];
    let rc = fsctl(0x9003, &mut block, &mut []);
    if rc != 0 {
        say(615, &[&alloc::format!("{rc}")], "Reading the cache settings returned %1\r\n");
        return rc;
    }
    let mut set = false;
    let mut stop_lazy = false;
    let opts: Vec<alloc::string::String> = args(s);
    for o in &opts {
        let u = o.to_ascii_uppercase();
        let Some(p) = u.strip_prefix('-').or_else(|| u.strip_prefix('/')) else {
            say(625, &[o], "The option %1 is not correct.\r\n");
            usage(prog);
            return 1;
        };
        let Some((k, v)) = p.split_once(':') else {
            say(625, &[o], "The option %1 is not correct.\r\n");
            usage(prog);
            return 1;
        };
        // rs2's own: a report on the log of where a workload's time went.
        if k == "STATS" {
            return fsctl(if v == "RESET" { 0x9f00 } else { 0x9f01 }, &mut [], &mut []);
        }
        let num = |v: &str| v.parse::<u32>().ok();
        let slot = match k {
            "DISKIDLE" => Some(0),
            "MAXAGE" => Some(4),
            "BUFFERIDLE" => Some(8),
            _ => None,
        };
        if let Some(at) = slot {
            let Some(n) = num(v) else {
                say(623, &[k, v], "The option %1:%2 is not correct; a positive number\r\nwas expected.\r\n");
                return 1;
            };
            block[at..at + 4].copy_from_slice(&n.to_le_bytes());
            set = true;
            continue;
        }
        let at = match k {
            "LAZY" => 20,
            "READAHEAD" => 22,
            _ => {
                say(625, &[o], "The option %1 is not correct.\r\n");
                usage(prog);
                return 1;
            }
        };
        let n = match v {
            "ON" => 1,
            "OFF" => 0,
            _ => match num(v) {
                Some(n) => n as u16,
                None => {
                    say(623, &[k, v], "The option %1:%2 is not correct; a positive number\r\nwas expected.\r\n");
                    return 1;
                }
            },
        };
        if k == "LAZY" && n == 0 {
            stop_lazy = true;
        }
        block[at..at + 2].copy_from_slice(&n.to_le_bytes());
        set = true;
    }
    if stop_lazy {
        let rc = fsctl(0x9001, &mut [], &mut []);
        if rc != 0 {
            say(618, &[&alloc::format!("{rc}")], "Stopping lazy writing returned %1\r\n");
            return rc;
        }
    }
    if set {
        let mut parm = block;
        let rc = fsctl(0x9002, &mut [], &mut parm);
        if rc != 0 {
            say(616, &[&alloc::format!("{rc}")], "Setting the cache parameters returned %1\r\n");
            return rc;
        }
        fsctl(0x9003, &mut block, &mut []);
    }
    if opts.is_empty() || set {
        let f = |v: u32| alloc::format!("{v:>8}");
        say(
            621,
            &[&f(u32at(&block, 0)), &f(u32at(&block, 4)), &f(u32at(&block, 8)), &f(u16::from_le_bytes([block[18], block[19]]) as u32 * 2)],
            "\r\n          DiskIdle: %1 milliseconds\r\n            MaxAge: %2 milliseconds\r\n        BufferIdle: %3 milliseconds\r\n        Cache size: %4 kbytes\r\n",
        );
        let lazy = u16::from_le_bytes([block[20], block[21]]);
        let ahead = u16::from_le_bytes([block[22], block[23]]);
        if lazy != 0 {
            say(689, &[&alloc::format!("{lazy}")], "%1 lazy writer(s) are on.\r\n");
        } else {
            say(690, &[], "Lazy writing is off.\r\n");
        }
        if ahead != 0 {
            say(695, &[&alloc::format!("{ahead}")], "%1 read-ahead worker(s) are on.\r\n");
        } else {
            say(696, &[], "Read-ahead is off.\r\n");
        }
    }
    0
}
