//! Run rs2's linkers on the host:
//! `cargo run -p omflink --example link -- link|link386 COMMAND LINE`.
//! File names are looked up case-insensitively in their directory, as on
//! OS/2; `\` works as a path separator.
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

struct H;

fn real(p: &str) -> PathBuf {
    let p = p.replace('\\', "/");
    let path = Path::new(&p);
    if path.exists() {
        return path.to_path_buf();
    }
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().eq_ignore_ascii_case(&name) {
                return e.path();
            }
        }
    }
    path.to_path_buf()
}

impl omflink::Host for H {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let p = real(path);
        if p.is_dir() {
            return None;
        }
        std::fs::read(p).ok()
    }
    fn write(&mut self, path: &str, data: &[u8]) -> bool {
        std::fs::write(real(path), data).is_ok()
    }
    fn delete(&mut self, path: &str) {
        let _ = std::fs::remove_file(real(path));
    }
    fn env(&mut self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn out(&mut self, s: &str) {
        print!("{s}");
        let _ = std::io::stdout().flush();
    }
    fn prompt(&mut self) -> Option<String> {
        let mut l = String::new();
        match std::io::stdin().lock().read_line(&mut l) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(l.trim_end().to_string()),
        }
    }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let prog = match a.next().as_deref() {
        Some("link386") => omflink::Prog::Link386,
        _ => omflink::Prog::Link,
    };
    let rest: Vec<String> = a.collect();
    let code = omflink::run(&mut H, prog, &rest.join(" "));
    std::process::exit(code);
}
