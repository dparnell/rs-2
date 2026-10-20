//! Dump an OS/2 binary profile (INI file) as text: each application, its
//! keys and their values (text when printable, else hex), and whether the
//! file is laid out the way IBM's writer (and this crate) writes it.
//!
//! `cargo run -p inifile --example inidump -- FILE.INI [--summary]`

use std::process::ExitCode;

fn show(v: &[u8]) -> String {
    let text = v.strip_suffix(b"\0").unwrap_or(v);
    if text.iter().all(|&c| (0x20..0x7f).contains(&c) || c >= 0x80) {
        format!("\"{}\"{}", text.iter().map(|&c| c as char).collect::<String>(), if text.len() < v.len() { "" } else { " (no NUL)" })
    } else {
        v.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: inidump FILE.INI [--summary]");
        return ExitCode::FAILURE;
    };
    let summary = args.iter().any(|a| a == "--summary");
    let b = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let p = inifile::parse(&b);
    if p.foreign {
        println!("{path}: not an OS/2 profile");
        return ExitCode::FAILURE;
    }
    let keys: usize = p.profile.apps.iter().map(|a| a.keys.len()).sum();
    let canonical = p.profile.to_bytes() == b;
    println!(
        "{path}: {} bytes, {} applications, {keys} keys{}{}",
        b.len(),
        p.profile.apps.len(),
        if p.damaged { ", DAMAGED" } else { "" },
        if canonical { ", canonical layout" } else { ", not in canonical layout" }
    );
    if !summary {
        for a in &p.profile.apps {
            println!("[{}]", String::from_utf8_lossy(&a.name));
            for k in &a.keys {
                println!("  {} = {}", String::from_utf8_lossy(&k.name), show(&k.data));
            }
        }
    }
    if p.damaged { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
