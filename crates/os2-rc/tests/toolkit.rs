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

/// Rebinding a module's own resources (plain and compressed) keeps them
/// readable and leaves the rest of the module alone.
#[test]
fn rebind_toolkit_modules() {
    let mut mods = Vec::new();
    for d in ["samples/pm/stdwnd/hello.exe", "samples/pm/dialog/dialog.exe", "samples/mm/clock/clock.exe", "samples/rexx/api/pmrexx/pmrexxio.dll"] {
        if let Ok(b) = std::fs::read(format!("{TK}/{d}")) {
            mods.push(b);
        }
    }
    let ne = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/unpacked/disk_1/OS2/DLL/PMFORMAT.DLL");
    if let Ok(b) = std::fs::read(ne) {
        mods.push(b);
    }
    for m in mods {
        let res: Vec<os2_rc::Resource> = os2_exe::res::list(&m)
            .into_iter()
            .map(|(ty, id)| os2_rc::Resource { ty, name: Name::Id(id), flags: 0x1030, data: os2_exe::res::get(&m, ty, id).unwrap() })
            .collect();
        for compress in [0, 1] {
            let o = os2_rc::bind::BindOptions { compress, pack: false };
            let out = os2_rc::bind::bind(&m, &res, o).unwrap();
            for r in &res {
                let Name::Id(id) = r.name else { continue };
                assert_eq!(os2_exe::res::get(&out, r.ty, id).as_deref(), Some(&r.data[..]), "{}.{id}", r.ty);
            }
            // Bound twice gives the same module.
            assert_eq!(os2_rc::bind::bind(&out, &res, o).unwrap(), out);
        }
    }
}

/// rs2's own test scripts (tests/rc) give the .RES files Warp's RC.EXE
/// wrote for them (tests/rc/ibm, made under rs2). They include the
/// Toolkit's os2.h, so they are skipped without it.
#[test]
fn test_scripts_match_ibm() {
    if !Path::new(&format!("{TK}/h/os2.h")).exists() {
        return;
    }
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/rc"));
    for t in ["DIALOGS", "WINDOWS", "MENUS", "INCL", "NAMES"] {
        let mut files = HostFiles { dir: dir.clone() };
        let opts = Options { include: vec![format!("{TK}/h")], codepage: 850, ..Default::default() };
        let out = os2_rc::compile(&mut files, &format!("{t}.RC"), &opts);
        assert!(!out.failed(), "{t}: {:?}", out.diags);
        let ibm = std::fs::read(dir.join("ibm").join(format!("{t}.RES"))).unwrap();
        assert!(os2_rc::res::write(&out.resources) == ibm, "{t}.RC differs from IBM's .RES");
    }
    // Named resources numbered as IBM's RC numbers them.
    assert_eq!(os2_rc::bind::name_id(b"NAMED"), 0xf44d);
    assert_eq!(os2_rc::bind::name_id(b"LONGERNAME1"), 0x1956);
    assert_eq!(os2_rc::bind::name_id(b"ZZZZZZZZ"), 0x8000);
}
