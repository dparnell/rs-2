//! Compiles the Toolkit samples' scripts and compares the resources with
//! the ones IBM's RC bound into the samples' EXEs and DLLs, and (given a
//! directory of IBM RC's .RES files, from a run under rs2) with those.
//!
//! `cargo run -p os2-rc --features host --example rccheck [-v] [FILTER] [--res DIR]`

use std::path::{Path, PathBuf};

use os2_rc::host::HostFiles;
use os2_rc::{res, Name, Options};

const TK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/boot/toolkit/os2tk45");

fn hex(d: &[u8], from: usize, n: usize) -> String {
    d.iter().skip(from).take(n).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn rcs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            rcs(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rc")) {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "-v");
    let resdir = args.iter().position(|a| a == "--res").map(|i| PathBuf::from(&args[i + 1]));
    let filter = args.iter().find(|a| !a.starts_with('-') && Some(PathBuf::from(a.as_str())) != resdir).cloned();
    let mut list = Vec::new();
    rcs(&Path::new(TK).join("samples"), &mut list);
    let (mut ok, mut bad, mut absent, mut errs) = (0, 0, 0, 0);
    let (mut rok, mut rbad, mut rnone) = (0, 0, 0);
    for rc in list {
        let rel = rc.strip_prefix(TK).unwrap().to_string_lossy().to_string();
        if let Some(f) = &filter {
            if !rel.contains(f.as_str()) {
                continue;
            }
        }
        let dir = rc.parent().unwrap().to_path_buf();
        let mut files = HostFiles { dir: dir.clone() };
        let opts = Options { include: vec![format!("{TK}/h")], codepage: 850, ..Default::default() };
        let name = rc.file_name().unwrap().to_string_lossy().to_string();
        let out = os2_rc::compile(&mut files, &name, &opts);
        for d in &out.diags {
            println!("{rel}: {}", d.format());
        }
        if out.failed() {
            errs += 1;
            println!("{rel}: FAILED");
            continue;
        }
        // The modules of the directory.
        let mut mods: Vec<(String, Vec<u8>)> = Vec::new();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.ends_with(".exe") || n.ends_with(".dll") {
                mods.push((n, std::fs::read(e.path()).unwrap()));
            }
        }
        let (mut o, mut b, mut a) = (0, 0, 0);
        for r in &out.resources {
            let Name::Id(id) = r.name else { continue };
            let have: Vec<(&String, Vec<u8>)> =
                mods.iter().filter_map(|(n, d)| os2_exe::res::get(d, r.ty, id).map(|x| (n, x))).collect();
            if have.is_empty() {
                a += 1;
                if verbose {
                    println!("{rel}: {}.{id} not in the modules", r.ty);
                }
            } else if have.iter().any(|(_, d)| *d == r.data) {
                o += 1;
            } else {
                b += 1;
                let (m, d) = &have[0];
                let at = d.iter().zip(&r.data).position(|(x, y)| x != y).unwrap_or(d.len().min(r.data.len()));
                println!(
                    "{rel}: {}.{id} differs from {m} at {at:#x} (sizes {} ibm, {} ours)\n   ibm:  {}\n   ours: {}",
                    r.ty,
                    d.len(),
                    r.data.len(),
                    hex(d, at.saturating_sub(4), 24),
                    hex(&r.data, at.saturating_sub(4), 24)
                );
            }
        }
        ok += o;
        bad += b;
        absent += a;
        if verbose || b > 0 {
            println!("{rel}: {o} same, {b} different, {a} not bound");
        }
        // Against IBM RC's .RES files.
        if let Some(rd) = &resdir {
            let rel_dir = dir.strip_prefix(Path::new(TK).join("samples")).unwrap();
            let base = rc.file_stem().unwrap().to_string_lossy().to_uppercase();
            let ours = res::write(&out.resources);
            let mut any = false;
            for ext in ["R32", "R16", "RW"] {
                let p = os2_rc::host::resolve(&rd.join("S"), &format!("{}/{base}.{ext}", rel_dir.to_string_lossy()));
                let Some(p) = p.filter(|p| p.is_file()) else { continue };
                any = true;
                let theirs = std::fs::read(&p).unwrap();
                if theirs == ours {
                    rok += 1;
                } else {
                    rbad += 1;
                    let at = theirs.iter().zip(&ours).position(|(x, y)| x != y).unwrap_or(theirs.len().min(ours.len()));
                    println!(
                        "{rel}: .{ext} differs at {at:#x} (sizes {} ibm, {} ours)\n   ibm:  {}\n   ours: {}",
                        theirs.len(),
                        ours.len(),
                        hex(&theirs, at.saturating_sub(8), 32),
                        hex(&ours, at.saturating_sub(8), 32)
                    );
                }
            }
            if !any {
                rnone += 1;
            }
        }
    }
    println!("resources: {ok} same, {bad} different, {absent} not bound; {errs} scripts failed");
    if resdir.is_some() {
        println!(".RES files: {rok} same, {rbad} different, {rnone} scripts without IBM output");
    }
}
