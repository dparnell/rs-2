//! Host front end for testing: `rc compile SCRIPT OUT.RES [-i DIR]...`,
//! `rc attach IN.RES IN.EXE OUT.EXE [-x1|-x2] [-p]`, and `rc table64 SCRIPT
//! OUT.BIN`: the resource table of a 64-bit module (docs/API64.md; C
//! programs link it with userland64/c/cc64.sh).
//!
//! `cargo run -p os2-rc --features host --example rc -- ...`

use os2_rc::bind::BindOptions;
use os2_rc::host::HostFiles;
use os2_rc::{res, Options};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        Some("compile") => {
            let script = std::path::Path::new(&a[1]);
            let dir = script.parent().unwrap().to_path_buf();
            let mut include = Vec::new();
            let mut i = 3;
            while i + 1 < a.len() {
                if a[i] == "-i" {
                    include.push(a[i + 1].clone());
                }
                i += 2;
            }
            let mut files = HostFiles { dir };
            let out = os2_rc::compile(&mut files, &script.file_name().unwrap().to_string_lossy(), &Options { include, codepage: 850, ..Default::default() });
            for d in &out.diags {
                eprintln!("{}", d.format());
            }
            if out.failed() {
                std::process::exit(1);
            }
            std::fs::write(&a[2], res::write(&out.resources)).unwrap();
        }
        Some("table64") => match os2_rc::host::compile_table64(std::path::Path::new(&a[1])) {
            Ok(t) => std::fs::write(&a[2], t).unwrap(),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        Some("attach") => {
            let r = res::read(&std::fs::read(&a[1]).unwrap()).expect("not a .RES file");
            let exe = std::fs::read(&a[2]).unwrap();
            let mut o = BindOptions::default();
            for x in &a[4..] {
                match x.as_str() {
                    "-x" | "-x1" => o.compress = 1,
                    "-x2" => o.compress = 2,
                    "-p" => o.pack = true,
                    _ => {}
                }
            }
            match os2_rc::bind::bind(&exe, &r, o) {
                Ok(d) => std::fs::write(&a[3], d).unwrap(),
                Err(e) => {
                    eprintln!("bind: {e:?}");
                    std::process::exit(1);
                }
            }
        }
        _ => eprintln!("usage: rc compile SCRIPT OUT.RES | rc attach IN.RES IN.EXE OUT.EXE"),
    }
}
