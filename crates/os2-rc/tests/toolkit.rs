//! The Toolkit samples' scripts compiled and compared with the resources
//! IBM's RC bound into the samples' modules (skipped without the Toolkit
//! in work/media). `examples/rccheck.rs` shows the differences.

use std::path::{Path, PathBuf};

use os2_rc::host::HostFiles;
use os2_rc::{Name, Options};

const TK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/boot/toolkit/os2tk45");

fn rcs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for p in rd.flatten().map(|e| e.path()) {
        if p.is_dir() {
            rcs(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rc")) {
            out.push(p);
        }
    }
}

#[test]
fn toolkit_samples_match_ibm() {
    let mut list = Vec::new();
    rcs(&Path::new(TK).join("samples"), &mut list);
    if list.is_empty() {
        return;
    }
    let (mut same, mut compiled) = (0, 0);
    let mut bad = Vec::new();
    for rc in list {
        let dir = rc.parent().unwrap().to_path_buf();
        let mut files = HostFiles { dir: dir.clone() };
        let opts = Options { include: vec![format!("{TK}/h")], codepage: 850, ..Default::default() };
        let out = os2_rc::compile(&mut files, &rc.file_name().unwrap().to_string_lossy(), &opts);
        if out.failed() {
            // Scripts IBM's RC rejects as well (missing PRODINFO.BMP, SOM
            // headers, Windows headers, files meant for RCINCLUDE).
            continue;
        }
        compiled += 1;
        let mods: Vec<Vec<u8>> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.ends_with(".exe") || n.ends_with(".dll")
            })
            .map(|e| std::fs::read(e.path()).unwrap())
            .collect();
        for r in &out.resources {
            let Name::Id(id) = r.name else { continue };
            let have: Vec<Vec<u8>> = mods.iter().filter_map(|d| os2_exe::res::get(d, r.ty, id)).collect();
            if have.is_empty() {
                continue;
            }
            if have.iter().any(|d| *d == r.data) {
                same += 1;
            } else {
                bad.push(format!("{}: {}.{id}", rc.display(), r.ty));
            }
        }
    }
    assert!(bad.is_empty(), "resources that differ from IBM's: {bad:#?}");
    assert!(compiled >= 40 && same >= 300, "{compiled} scripts, {same} resources compared");
}
