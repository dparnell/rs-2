//! RS2-LD: what rustc on rs2 runs to link (docs/SELF-HOSTING.md). Does what
//! lld.sh does for cross builds, with the parts from `lib/rs2` next to its
//! own directory: link64.ld, the start-up objects, and `libc.a`, which is
//! rs2libc, so that programs need not name it. Runs LLD.EXE from its own
//! directory with a response file.

use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use rs2libc as _;

fn die(why: impl std::fmt::Display) -> ! {
    eprintln!("rs2-ld: {why}");
    exit(1)
}

/// The arguments, with `@file`s read (GNU quoting).
fn expand(args: impl Iterator<Item = String>, out: &mut Vec<String>) {
    for a in args {
        match a.strip_prefix('@') {
            Some(f) => {
                let t = std::fs::read_to_string(f).unwrap_or_else(|e| die(format_args!("{f}: {e}")));
                expand(split(&t).into_iter(), out);
            }
            None => out.push(a),
        }
    }
}

fn split(t: &str) -> Vec<String> {
    let (mut v, mut cur, mut quote, mut some) = (Vec::new(), String::new(), None, false);
    let mut it = t.chars();
    while let Some(c) = it.next() {
        match (c, quote) {
            ('\\', _) => {
                cur.extend(it.next());
                some = true;
            }
            ('"' | '\'', None) => {
                quote = Some(c);
                some = true;
            }
            (c, Some(q)) if c == q => quote = None,
            (c, None) if c.is_whitespace() => {
                if some {
                    v.push(std::mem::take(&mut cur));
                    some = false;
                }
            }
            (c, _) => {
                cur.push(c);
                some = true;
            }
        }
    }
    if some {
        v.push(cur);
    }
    v
}

fn quoted(a: &str) -> String {
    let mut s = String::from("\"");
    for c in a.chars() {
        if c == '"' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s + "\"\n"
}

/// Paths as cargo writes them (rs2libc's rooted_paths): /C:/dir is C:/dir.
fn unrooted(a: &str) -> String {
    let b = a.as_bytes();
    let at = |i: usize| b.get(i) == Some(&b'/') && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) && b.get(i + 2) == Some(&b':');
    // At the start, after -L or after =; and the last drive counts.
    match (0..a.len()).rev().find(|&i| at(i)) {
        Some(i) => {
            let keep = match a.find(['=']) {
                Some(e) if e < i => e + 1,
                _ if a.starts_with("-L") => 2,
                _ => 0,
            };
            format!("{}{}", &a[..keep], &a[i + 1..])
        }
        None => a.to_string(),
    }
}

/// A DLL's module name: libfoo.so or libfoo-HASH.so is FOO.
fn module(out: &str) -> String {
    let n = Path::new(out).file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let n = n.strip_prefix("lib").unwrap_or(&n);
    let n = match n.rsplit_once('-') {
        Some((a, h)) if h.len() >= 8 && h.bytes().all(|c| c.is_ascii_hexdigit()) => a,
        _ => n,
    };
    n.to_ascii_uppercase()
}

fn main() {
    let exe = std::env::current_exe().unwrap_or_else(|e| die(e));
    let bin = exe.parent().unwrap_or_else(|| die("no directory"));
    let lib = bin.parent().unwrap_or(Path::new(".")).join("lib/rs2");
    let part = |n: &str| lib.join(n).to_string_lossy().into_owned();
    let mut args = Vec::new();
    expand(std::env::args().skip(1), &mut args);
    for a in &mut args {
        *a = unrooted(a);
    }
    if args.first().is_some_and(|a| a == "-flavor") {
        args.drain(..2.min(args.len()));
    }
    let shared = args.iter().any(|a| a == "-shared");
    let out = args.iter().position(|a| a == "-o").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| "a.exe".into());
    let name = module(&out);
    let (rsp, exports) = (format!("{out}.rsp"), format!("{out}.exp"));

    let mut l: Vec<String> = ["-flavor", "gnu"].map(String::from).to_vec();
    l.extend(["-L".into(), lib.to_string_lossy().into_owned(), "-T".into(), part("link64.ld")]);
    l.extend(
        [
            "-zdynamic-undefined-weak",
            "--unresolved-symbols=ignore-all",
            "--rs2-imports",
            "-nostdlib",
            "--no-rosegment",
            "-znow",
            "-znorelro",
            "--hash-style=sysv",
            "--eh-frame-hdr",
        ]
        .map(String::from),
    );
    l.extend(
        ["RS2_THREAD_START", "RS2_HOST", "RS2_MODULE", "RS2_APPTYPE", "RS2_RESOURCES"]
            .map(|s| format!("--export-dynamic-symbol={s}")),
    );
    if shared {
        l.extend([format!("-soname={name}"), "-Bsymbolic".into(), "-u".into(), "_DLL_InitTerm".into(), part("crt64d.o")]);
    } else {
        l.push(part("crt64.o"));
    }
    let mut script = false;
    for a in args {
        if a.starts_with("-soname=") || a == "--no-undefined-version" || a == "--rs2-64" {
            // Ours, or not wanted.
        } else if let (true, Some(f)) = (shared, a.strip_prefix("--version-script=")) {
            // What rustc exports, and what the loader looks for.
            let t = std::fs::read_to_string(f).unwrap_or_else(|e| die(format_args!("{f}: {e}")));
            let t = t.replacen("global:", "global: _DLL_InitTerm; RS2_MODULE; RS2_RESOURCES;", 1);
            std::fs::write(&exports, t).unwrap_or_else(|e| die(format_args!("{exports}: {e}")));
            script = true;
            l.extend([format!("--version-script={exports}"), "--undefined-version".into()]);
        } else if a == "-zrelro" || a == "-zlazy" {
        } else {
            l.push(a);
        }
    }
    l.push("-lc".into());
    // "-z relro" in two arguments.
    while let Some(i) = l.windows(2).position(|w| w[0] == "-z" && (w[1] == "relro" || w[1] == "lazy")) {
        l.drain(i..i + 2);
    }
    if std::env::var_os("RS2_LD_VERBOSE").is_some() {
        eprintln!("lld {}", l.join(" "));
    }
    let text: String = l[2..].iter().map(|a| quoted(a)).collect();
    std::fs::write(&rsp, text).unwrap_or_else(|e| die(format_args!("{rsp}: {e}")));
    let lld: PathBuf = bin.join("LLD.EXE");
    let r = Command::new(&lld).args(["-flavor", "gnu"]).arg(format!("@{rsp}")).status();
    let _ = std::fs::remove_file(&rsp);
    if script {
        let _ = std::fs::remove_file(&exports);
    }
    match r {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => die(format_args!("{}: {e}", lld.display())),
    }
}
