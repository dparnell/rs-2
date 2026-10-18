//! Decompress every packed file of the installation media, if present
//! (`work/media/client/os2image`, not part of the repository).

use std::path::{Path, PathBuf};

use os2_pack::{Decompressor, Error};

/// Entries that do not decompress on the Warp 4.52 media: UNPACK2's own
/// decoder (run under emulation) produces the same inconsistent token
/// stream for them, so the data itself is bad. Identical copies of some of
/// them exist in other packs.
const KNOWN_BAD: &[&str] = &[
    "\\OS2\\DLL\\OPENGL.DLL",
    "\\OS2\\DLL\\PMVIOP.DLL",
    "\\OS2\\SYSTEM\\NAMEMOVE.EXE",
    "\\OS2\\BITMAP\\DTOPBGND.BMP",
    "\\MMOS2\\VIDVBC.SYS",
    "\\LASERJET\\pre32.dll",
];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn media() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/media/client/os2image");
    if !root.is_dir() {
        eprintln!("skipping: {} not present", root.display());
        return;
    }
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    let mut dec = Decompressor::new();
    let (mut packs, mut ok, mut bad) = (0, 0, Vec::new());
    for f in files {
        let data = std::fs::read(&f).unwrap();
        if !os2_pack::is_packed(&data) {
            continue;
        }
        packs += 1;
        for e in os2_pack::entries(&data).unwrap_or_else(|err| panic!("{}: {err:?}", f.display())) {
            let r: Result<(), Error> = e
                .decompress(&mut dec)
                .and_then(|_| e.decompress_ea(&mut dec).map(|_| ()));
            let name = String::from_utf8_lossy(e.path).into_owned();
            match r {
                Ok(()) => ok += 1,
                Err(_) if KNOWN_BAD.contains(&name.as_str()) => bad.push(name),
                Err(err) => panic!("{}: {name}: {err:?}", f.display()),
            }
        }
    }
    eprintln!(
        "{packs} packed files, {ok} entries ok, {} known bad",
        bad.len()
    );
    assert!(packs > 0 && ok > 0);
}
