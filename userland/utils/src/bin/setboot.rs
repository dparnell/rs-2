//! SETBOOT: show and change how the system starts, and restart it.
//! SETBOOT [/Q] [/T:x | /T:NO] [/B] [/IBD:d] ...
//!
//! rs2 starts from its own UEFI loader, not from IBM's Boot Manager. Its
//! settings are the words of `\RS2\CMDLINE` on the EFI system partition
//! (the drive holding `\RS2\KERNEL`): `bootwait=MS` is how long the loader
//! waits for T (a text-mode start), which SETBOOT shows and sets as the
//! timeout (/T:x seconds; /T:NO for no wait). /B and /IBD:d (d being the
//! system's drive) restart the computer. The Boot Manager settings (/M,
//! /X, /N, /IBA) are answered as IBM's SETBOOT does without Boot Manager.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::*;
use utils::files::*;
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

/// The drive (letter) whose root holds \RS2\KERNEL and \RS2\CMDLINE.
fn loader_drive() -> Option<char> {
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut cur, &mut map) };
    (2..26u8).filter(|d| map & (1 << d) != 0).map(|d| (b'A' + d) as char).find(|l| exists(&alloc::format!("{l}:\\RS2\\KERNEL")))
}

fn restart() -> u32 {
    let c = os2::path::cstr("DOS$");
    let (mut h, mut act) = (0, 0);
    let rc = unsafe { DosOpen(c.as_ptr(), &mut h, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, core::ptr::null_mut()) };
    if rc == 0 {
        let (mut pl, mut dl) = (0, 0);
        unsafe { DosDevIOCtl(h, 0xd5, 0xab, core::ptr::null_mut(), 0, &mut pl, core::ptr::null_mut(), 0, &mut dl) };
    }
    err(1731, &[], "The system could not be restarted.\r\n");
    1731
}

fn usage() {
    say(
        867,
        &[],
        "SETBOOT sets up how the system starts.\r\nSyntax:\r\n  SETBOOT [/Q]\r\n  SETBOOT [/T:x or /T:NO][/B][/IBD:d]\r\nwhere:\r\n  Q        Shows the current start-up settings.\r\n  T:x      Waits x seconds at start-up for a text-mode choice.\r\n  T:NO     Does not wait.\r\n  B        Shuts down and restarts the system.\r\n  IBD:d    Shuts down and restarts the system from drive d.\r\n",
    );
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.is_empty() {
        usage();
        return 0;
    }
    let mut query = false;
    let mut timeout: Option<Option<u32>> = None;
    let mut reboot = false;
    for arg in &a {
        let u = arg.to_ascii_uppercase();
        let Some(p) = u.strip_prefix('/').or_else(|| u.strip_prefix('-')) else {
            say(872, &[arg], "%1 is not a correct parameter.\r\n");
            usage();
            return 1;
        };
        let (key, val) = match p.split_once(':') {
            Some((k, v)) => (k, Some(v)),
            None => (p, None),
        };
        match (key, val) {
            ("Q", None) => query = true,
            ("B", None) => reboot = true,
            ("Q" | "B", Some(_)) => {
                say(870, &[&alloc::format!("/{key}")], "/%1 does not take a value.\r\n");
                return 1;
            }
            ("T" | "M" | "X" | "N" | "IBD" | "IBA", None) => {
                say(871, &[&alloc::format!("/{key}")], "%1 needs a value.\r\n");
                return 1;
            }
            ("T", Some("NO")) => timeout = Some(None),
            ("T", Some(v)) => match v.parse::<u32>() {
                Ok(n) => timeout = Some(Some(n)),
                Err(_) => {
                    say(873, &[v, "/T"], "%1 is not a correct value for %2.\r\n");
                    return 1;
                }
            },
            ("IBD", Some(d)) => {
                let want = d.trim_end_matches(':');
                // Only the drive rs2 runs from can be started.
                let sys = boot_drive();
                if want.len() != 1 || !want.starts_with(sys) {
                    say(869, &[], "The system is not set up for starting from that drive.\r\n");
                    return 1;
                }
                reboot = true;
            }
            ("M" | "X" | "N" | "IBA", Some(_)) => {
                say(874, &[], "Boot Manager is not installed.\r\n");
                return 1;
            }
            _ => {
                say(872, &[arg], "%1 is not a correct parameter.\r\n");
                usage();
                return 1;
            }
        }
    }
    let Some(l) = loader_drive() else {
        say(874, &[], "Boot Manager is not installed.\r\n");
        return 1;
    };
    let path = alloc::format!("{l}:\\RS2\\CMDLINE");
    let text: String = read_file(&path).map(|d| d.iter().map(|&b| b as char).collect()).unwrap_or_default();
    let mut words: Vec<String> = text.split_whitespace().map(String::from).collect();
    if let Some(t) = timeout {
        words.retain(|w| !w.starts_with("bootwait="));
        words.push(alloc::format!("bootwait={}", t.map_or(0, |s| s * 1000)));
        let line = words.join(" ") + "\r\n";
        let bytes: Vec<u8> = line.chars().map(|c| c as u32 as u8).collect();
        if let Err(rc) = write_file(&path, &bytes) {
            say(868, &[], "The start-up settings could not be written.\r\n");
            return rc;
        }
    }
    if query {
        let ms = words.iter().find_map(|w| w.strip_prefix("bootwait=")).and_then(|v| v.parse::<u32>().ok()).unwrap_or(1000);
        if ms == 0 {
            say(860, &[], "Timeout         No\r\n");
        } else {
            say(861, &[&alloc::format!("{}", ms.div_ceil(1000))], "Timeout         %1\r\n");
        }
        say(864, &[], "Mode            Normal\r\n");
        let sys = alloc::format!("rs2 ({}:)", boot_drive());
        say(865, &["0", &sys], "System %1       %2\r\n");
    }
    if reboot {
        return restart();
    }
    0
}

/// The drive the system runs from (QSV_BOOT_DRIVE).
fn boot_drive() -> char {
    let mut d = 3u32;
    unsafe { DosQuerySysInfo(5, 5, &mut d, 4) };
    (b'A' + d as u8 - 1) as char
}
