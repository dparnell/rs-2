//! DIR.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use os2::*;

use crate::parse::Simple;
use crate::util::*;

struct Opts {
    wide: bool,
    bare: bool,
    sub: bool,
    pause: bool,
    attr: u32,
}

pub fn dir(s: &Simple) -> u32 {
    let mut o = Opts { wide: false, bare: false, sub: false, pause: false, attr: FILE_DIRECTORY };
    let mut specs = Vec::new();
    // Options may be glued to the command ("dir/w") or to each other.
    let args = s.args.replace('/', " /");
    for w in crate::parse::split_words(&args) {
        if let Some(opt) = w.strip_prefix('/') {
            for ch in opt.chars() {
                match ch.to_ascii_uppercase() {
                    'W' => o.wide = true,
                    'B' => o.bare = true,
                    'S' => o.sub = true,
                    'P' => o.pause = true,
                    'A' => o.attr = FILE_DIRECTORY | FILE_HIDDEN | FILE_SYSTEM,
                    _ => {}
                }
            }
        } else {
            specs.push(w);
        }
    }
    if specs.is_empty() {
        specs.push(String::new());
    }
    let mut rc = 0;
    for spec in specs {
        let r = list(&spec, &o);
        if r != 0 {
            rc = r;
        }
    }
    rc
}

/// Turn a DIR argument into a search pattern.
fn pattern(spec: &str) -> String {
    if spec.is_empty() {
        let mut d = current_dir(0);
        if !d.ends_with('\\') {
            d.push('\\');
        }
        d.push('*');
        return d;
    }
    let s = if spec.len() == 2 && spec.ends_with(':') {
        let d = (spec.as_bytes()[0].to_ascii_uppercase() - b'A' + 1) as u32;
        current_dir(d)
    } else {
        String::from(spec)
    };
    if !has_wildcards(&s) && is_dir(&s) {
        alloc::format!("{}\\*", s.trim_end_matches('\\'))
    } else {
        s
    }
}

fn list(spec: &str, o: &Opts) -> u32 {
    let pat = pattern(spec);
    let full = full_path(&pat);
    let (dir, _) = split_path(&full);
    let drive = (full.as_bytes()[0].to_ascii_uppercase() - b'A' + 1) as u32;
    if !o.bare {
        header(drive);
        outln(&alloc::format!("Directory of {}", dir.trim_end_matches('\\').to_string() + if dir.len() <= 3 { "\\" } else { "" }));
        outln("");
    }
    let files = match find(&full, o.attr) {
        Ok(f) => f,
        Err(e) => {
            if !o.bare {
                outln("File Not Found");
            }
            return e;
        }
    };
    let mut count = 0;
    let mut bytes: u64 = 0;
    let mut col = 0;
    let mut lines = 0;
    for f in &files {
        count += 1;
        bytes += f.size as u64;
        let is_dir = f.attr & FILE_DIRECTORY != 0;
        if o.bare {
            if f.name != "." && f.name != ".." {
                if o.sub {
                    outln(&alloc::format!("{dir}{}", f.name));
                } else {
                    outln(&f.name);
                }
            }
        } else if o.wide {
            let n = if is_dir { alloc::format!("[{}]", f.name) } else { f.name.clone() };
            out(&alloc::format!("{n:<16}"));
            col += 1;
            if col == 5 {
                outln("");
                col = 0;
            }
        } else {
            let size = if is_dir { String::from("<DIR>") } else { alloc::format!("{}", f.size) };
            outln(&alloc::format!("{}  {}  {:>10}  {:>10}  {}", fmt_date(f.date), fmt_time(f.time), size, 0, f.name));
            lines += 1;
            if o.pause && lines % 20 == 0 {
                out("Press Enter to continue . . . ");
                let _ = read_line();
            }
        }
    }
    if o.wide && col != 0 {
        outln("");
    }
    if !o.bare {
        outln(&alloc::format!("{count:>9} file(s) {bytes:>10} bytes used"));
        outln(&alloc::format!("{:>28} bytes free", free_space(drive)));
    }
    if o.sub {
        // Recurse into subdirectories.
        let subs = find(&alloc::format!("{dir}*"), FILE_DIRECTORY).unwrap_or_default();
        let (_, name) = split_path(&full);
        for d in subs {
            if d.attr & FILE_DIRECTORY != 0 && d.name != "." && d.name != ".." {
                if !o.bare {
                    outln("");
                }
                list(&alloc::format!("{dir}{}\\{name}", d.name), o);
            }
        }
    }
    0
}

fn header(drive: u32) {
    let mut fi = FsInfo { serial: 0, cch_label: 0, label: [0; 12] };
    let rc = unsafe { DosQueryFSInfo(drive, FSIL_VOLSER, &mut fi as *mut _ as *mut u8, core::mem::size_of::<FsInfo>() as u32) };
    let letter = (b'A' + drive as u8 - 1) as char;
    outln("");
    if rc == 0 {
        let label = os2::path::from_bytes(&fi.label[..(fi.cch_label as usize).min(fi.label.len())]);
        if label.is_empty() {
            outln(&alloc::format!("The volume in drive {letter} has no label."));
        } else {
            outln(&alloc::format!("The volume label in drive {letter} is {label}."));
        }
        outln(&alloc::format!("The Volume Serial Number is {:04X}:{:04X}.", fi.serial >> 16, fi.serial & 0xffff));
    }
}

fn free_space(drive: u32) -> u64 {
    let mut a = FsAllocate::default();
    let rc = unsafe { DosQueryFSInfo(drive, FSIL_ALLOC, &mut a as *mut _ as *mut u8, core::mem::size_of::<FsAllocate>() as u32) };
    if rc != 0 {
        return 0;
    }
    a.units_avail as u64 * a.sectors_per_unit as u64 * a.bytes_per_sector as u64
}
