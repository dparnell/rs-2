//! INSTALL.EXE: rs2's installer on the command line (PMINSTAL.EXE is the
//! same installer under Presentation Manager; both use `engine`).
//!
//! ```text
//! INSTALL [/LIST] [/DISK:n] [/NEW[:FAT32|HPFS|JFS] | /UPDATE] [/YES] [/NORESTART]
//! ```
//!
//! Without options it asks: which disk, and for a disk that has rs2
//! whether to update it or erase it, and for a new installation which
//! layout. /YES skips the confirmation (a new installation erases the
//! disk).

#![no_std]
#![no_main]

extern crate alloc;

mod engine;

use alloc::string::{String, ToString};

use engine::{DiskInfo, Fs};
use os2::*;

os2::entry!(main);

fn out(s: &str) {
    os2::print(s);
}

fn outln(s: &str) {
    os2::print(s);
    os2::print("\r\n");
}

fn ask(prompt: &str) -> String {
    out(prompt);
    let mut buf = [0u8; 128];
    let mut n = 0;
    unsafe { DosRead(0, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    buf[..n as usize].iter().map(|&b| b as char).collect::<String>().trim().to_string()
}

struct Text {
    dots: bool,
}

impl engine::Progress for Text {
    fn step(&mut self, what: &str) {
        if self.dots {
            outln("");
        }
        out(&alloc::format!("{what}..."));
        self.dots = true;
    }
    fn file(&mut self, count: u32, _name: &str) {
        if count % 10 == 0 {
            out(".");
        }
    }
    fn note(&mut self, what: &str) {
        outln("");
        out(&alloc::format!("  {what}"));
    }
}

#[derive(Default)]
struct Opts {
    list: bool,
    disk: Option<u32>,
    /// Some(None): /NEW without a layout.
    new: Option<Option<Fs>>,
    update: bool,
    yes: bool,
    no_restart: bool,
}

fn parse(args: &str) -> Result<Opts, String> {
    let mut o = Opts::default();
    for w in args.split_whitespace() {
        let u = w.to_ascii_uppercase();
        let (k, v) = match u.split_once(':') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (u.clone(), None),
        };
        match k.trim_start_matches(['/', '-']) {
            "LIST" => o.list = true,
            "DISK" => o.disk = Some(v.as_deref().and_then(|n| n.parse().ok()).ok_or("/DISK needs a disk number (/DISK:1).")?),
            "NEW" | "ERASE" => {
                o.new = Some(match v.as_deref() {
                    None => None,
                    Some("FAT32" | "FAT") => Some(Fs::Fat),
                    Some("HPFS") => Some(Fs::Hpfs),
                    Some("JFS") => Some(Fs::Jfs),
                    Some(x) => return Err(alloc::format!("Unknown layout {x}: FAT32, HPFS or JFS.")),
                })
            }
            "UPDATE" => o.update = true,
            "YES" | "Y" => o.yes = true,
            "NORESTART" => o.no_restart = true,
            "?" | "HELP" => return Err(String::new()),
            _ => return Err(alloc::format!("Unknown option {w}.")),
        }
    }
    if o.update && o.new.is_some() {
        return Err(String::from("/UPDATE and /NEW cannot be used together."));
    }
    Ok(o)
}

fn usage() {
    outln("Installs rs2 from this disc onto a hard disk, or updates it there.");
    outln("");
    outln("INSTALL [/LIST] [/DISK:n] [/NEW[:FAT32|HPFS|JFS] | /UPDATE] [/YES] [/NORESTART]");
    outln("");
    outln("  /LIST       list the hard disks and what is on them");
    outln("  /DISK:n     the disk to install on");
    outln("  /NEW:FAT32  erase the disk: one FAT32 partition for everything");
    outln("  /NEW:HPFS   erase the disk: a FAT32 boot partition and an HPFS");
    outln("              system partition");
    outln("  /NEW:JFS    erase the disk: a FAT32 boot partition and a JFS");
    outln("              system partition");
    outln("  /UPDATE     update the rs2 on the disk, keeping settings and files");
    outln("  /YES        do not ask for confirmation");
    outln("  /NORESTART  do not restart the computer afterwards");
}

fn describe(d: &DiskInfo) -> String {
    let what = match &d.found {
        Some(f) => alloc::format!(" - {} installed ({})", f.version, layout_text(f.layout())),
        None => String::new(),
    };
    alloc::format!("  {}. {} MB{what}", d.number, d.disk.mb())
}

fn layout_text(fs: Fs) -> &'static str {
    match fs {
        Fs::Fat => "FAT32",
        Fs::Hpfs => "FAT32 boot partition, HPFS system",
        Fs::Jfs => "FAT32 boot partition, JFS system",
    }
}

fn main(s: &Startup) -> u32 {
    let args: String = s.args().iter().map(|&b| b as char).collect();
    let rc = match parse(&args) {
        Ok(o) => run(o),
        Err(e) => {
            if !e.is_empty() {
                outln(&e);
                outln("");
            }
            usage();
            if e.is_empty() { 0 } else { 1 }
        }
    };
    // Started from the desktop the installer has a session of its own,
    // which closes when it ends: keep any message on the screen.
    if rc != 0 && args.trim().is_empty() {
        outln("");
        let _ = ask("Press Enter to close. ");
    }
    rc
}

fn run(o: Opts) -> u32 {
    let disks = engine::disks();
    if o.list {
        outln(&alloc::format!("This disc: {}", engine::this_version()));
        for d in &disks {
            outln(&describe(d));
        }
        return 0;
    }
    let interactive = o.disk.is_none() || (o.new.is_none() && !o.update);
    if interactive {
        outln("");
        outln("rs2 Installation");
        outln("================");
        outln("");
        outln(&alloc::format!("This disc: {}", engine::this_version()));
        outln("");
    }
    if disks.is_empty() {
        outln("No hard disks were found. Installation cannot continue.");
        return 1;
    }
    let number = match o.disk {
        Some(n) => n,
        None => {
            outln("Hard disks:");
            for d in &disks {
                outln(&describe(d));
            }
            match ask("\r\nInstall on which disk (number, or Enter to cancel)? ").parse() {
                Ok(n) => n,
                Err(_) => {
                    outln("Installation cancelled.");
                    return 1;
                }
            }
        }
    };
    let Some(d) = disks.iter().find(|d| d.number == number) else {
        outln("There is no such disk.");
        return 1;
    };
    // Update, or a new installation (and its layout).
    let mut update = o.update;
    let mut new = o.new;
    if !update && new.is_none() {
        if let Some(f) = &d.found {
            outln("");
            outln(&alloc::format!("Disk {number} has {} ({}).", f.version, layout_text(f.layout())));
            outln("  U  Update it: the system files are replaced; your settings, desktop,");
            outln("     programs and files are kept.");
            outln("  E  Erase the disk and install anew.");
            match ask("Choose U or E (or Enter to cancel): ").to_ascii_uppercase().as_str() {
                "U" => update = true,
                "E" => new = Some(None),
                _ => {
                    outln("Installation cancelled.");
                    return 1;
                }
            }
        } else {
            new = Some(None);
        }
    }
    let mut p = Text { dots: false };
    let res = if update {
        let Some(f) = &d.found else {
            outln(&alloc::format!("Disk {number} has no rs2 to update."));
            return 1;
        };
        engine::update(f, &mut p)
    } else {
        let fs = match new.flatten() {
            Some(fs) => fs,
            None => {
                outln("");
                outln("How should the disk be set up?");
                outln("  1  One FAT32 partition for everything (the simplest).");
                outln(&alloc::format!("  2  A FAT32 boot partition ({} MB) and the system on HPFS", engine::ESP_MB));
                outln("     (long file names, extended attributes; at most 64 GB).");
                outln(&alloc::format!("  3  A FAT32 boot partition ({} MB) and the system on JFS", engine::ESP_MB));
                outln("     (journaled: quick to check after a crash; any size).");
                match ask("Choose 1, 2 or 3 (or Enter to cancel): ").as_str() {
                    "1" => Fs::Fat,
                    "2" => Fs::Hpfs,
                    "3" => Fs::Jfs,
                    _ => {
                        outln("Installation cancelled.");
                        return 1;
                    }
                }
            }
        };
        if !engine::layouts().contains(&fs) {
            outln(&alloc::format!("{} is not available yet.", fs.name()));
            return 1;
        }
        if !o.yes {
            outln("");
            outln(&alloc::format!("ALL DATA ON DISK {number} WILL BE ERASED."));
            if !ask("Type YES to continue: ").eq_ignore_ascii_case("YES") {
                outln("Installation cancelled.");
                return 1;
            }
        }
        engine::install_fresh(&d.disk, fs, &mut p)
    };
    outln("");
    if let Err(e) = res {
        outln(&e);
        return 1;
    }
    outln("");
    if o.no_restart {
        outln("Installation is complete.");
        return 0;
    }
    outln("Installation is complete. Remove the installation disc and press Enter");
    out("to restart the computer.");
    if !o.yes {
        let _ = ask("");
    }
    engine::disk::restart();
    0
}

