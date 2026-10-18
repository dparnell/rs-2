//! List and extract OS/2 installer packed files (FTCOMP, as read by
//! UNPACK/UNPACK2).
//!
//! ```text
//! unpack list FILE...             one line per entry: size, date, attributes, path
//! unpack extract [--ea] FILE... OUTDIR
//!                                 recreate the stored paths under OUTDIR; with
//!                                 --ea also write each entry's extended
//!                                 attributes (FEA2LIST) to PATH.ea
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use os2_pack::{Decompressor, Entry};

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn date_string(date: u16, time: u16) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        1980 + (date >> 9),
        (date >> 5) & 0xf,
        date & 0x1f,
        time >> 11,
        (time >> 5) & 0x3f,
        (time & 0x1f) * 2
    )
}

/// DOS date/time (taken as UTC) to a system time.
fn system_time(date: u16, time: u16) -> Option<SystemTime> {
    let (y, m, d) = (
        1980 + (date >> 9) as i64,
        ((date >> 5) & 0xf) as i64,
        (date & 0x1f) as i64,
    );
    if !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    // Days from the civil date (proleptic Gregorian).
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400
        + (time >> 11) as i64 * 3600
        + ((time >> 5) & 0x3f) as i64 * 60
        + (time & 0x1f) as i64 * 2;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs.try_into().ok()?))
}

/// Where an entry goes under `out`, refusing paths that would escape it.
fn target(out: &Path, e: &Entry) -> Option<PathBuf> {
    let mut p = out.to_path_buf();
    let mut any = false;
    for c in e.components() {
        let c = s(c);
        if c == "." || c == ".." {
            return None;
        }
        p.push(c);
        any = true;
    }
    any.then_some(p)
}

fn attr_string(a: u32) -> String {
    let flag = |bit: u32, c: char| if a & bit != 0 { c } else { '-' };
    [
        flag(0x20, 'A'),
        flag(0x10, 'D'),
        flag(0x04, 'S'),
        flag(0x02, 'H'),
        flag(0x01, 'R'),
    ]
    .iter()
    .collect()
}

fn read(path: &str) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("{path}: {e}");
            None
        }
    }
}

fn list(files: &[String]) -> bool {
    let mut ok = true;
    for f in files {
        let Some(data) = read(f) else {
            ok = false;
            continue;
        };
        match os2_pack::entries(&data) {
            Ok(entries) => {
                for e in entries {
                    let ea = if e.ea.is_some() {
                        format!(" ea={}", e.ea_size)
                    } else {
                        String::new()
                    };
                    println!(
                        "{:>9} {} {} {}{}",
                        e.size,
                        date_string(e.date, e.time),
                        attr_string(e.attributes),
                        s(e.path),
                        ea
                    );
                }
            }
            Err(err) => {
                eprintln!("{f}: {err:?}");
                ok = false;
            }
        }
    }
    ok
}

fn extract(files: &[String], out: &Path, with_ea: bool) -> bool {
    let mut dec = Decompressor::new();
    let (mut good, mut bad) = (0usize, 0usize);
    for f in files {
        let Some(data) = read(f) else {
            bad += 1;
            continue;
        };
        let entries = match os2_pack::entries(&data) {
            Ok(e) => e,
            Err(err) => {
                eprintln!("{f}: {err:?}");
                bad += 1;
                continue;
            }
        };
        for e in entries {
            let name = s(e.path);
            let Some(dst) = target(out, &e) else {
                eprintln!("{f}: {name}: unsafe path, skipped");
                bad += 1;
                continue;
            };
            let result = e
                .decompress(&mut dec)
                .and_then(|d| Ok((d, e.decompress_ea(&mut dec)?)));
            let (body, ea) = match result {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("{f}: {name}: {err:?}");
                    bad += 1;
                    continue;
                }
            };
            let write = || -> std::io::Result<()> {
                if let Some(dir) = dst.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&dst, &body)?;
                if let Some(t) = system_time(e.date, e.time) {
                    std::fs::File::options()
                        .write(true)
                        .open(&dst)?
                        .set_modified(t)?;
                }
                if let (true, Some(ea)) = (with_ea, ea) {
                    let mut p = dst.clone().into_os_string();
                    p.push(".ea");
                    std::fs::write(p, ea)?;
                }
                Ok(())
            };
            match write() {
                Ok(()) => good += 1,
                Err(err) => {
                    eprintln!("{}: {err}", dst.display());
                    bad += 1;
                }
            }
        }
    }
    eprintln!("{good} extracted, {bad} failed");
    bad == 0
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        eprintln!("usage: unpack list FILE...\n       unpack extract [--ea] FILE... OUTDIR");
        ExitCode::FAILURE
    };
    if args.is_empty() {
        return usage();
    }
    let cmd = args.remove(0);
    let ok = match cmd.as_str() {
        "list" if !args.is_empty() => list(&args),
        "extract" => {
            let with_ea = args.first().is_some_and(|a| a == "--ea");
            if with_ea {
                args.remove(0);
            }
            if args.len() < 2 {
                return usage();
            }
            let out = PathBuf::from(args.pop().unwrap());
            extract(&args, &out, with_ea)
        }
        _ => return usage(),
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
