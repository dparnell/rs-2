//! RC64: compiles an OS/2 resource script into assembler source that clang
//! links into a 64-bit program (the module's RS2_RESOURCES, docs/API64.md):
//!
//! ```text
//! rc64 PROG.RC [-o PROG_RC.s]
//! clang -mwindows prog.c PROG_RC.s -o PROG.EXE
//! ```
//!
//! `-b` writes the table itself (what os2::resources!() of Rust programs
//! embeds).

use std::fmt::Write;
use std::path::PathBuf;
use std::process::exit;

use rs2libc as _;

fn main() {
    let (mut rc, mut out, mut binary) = (None, None, false);
    let mut a = std::env::args().skip(1);
    while let Some(x) = a.next() {
        match x.as_str() {
            "-o" => out = a.next().map(PathBuf::from),
            "-b" => binary = true,
            _ if rc.is_none() && !x.starts_with('-') => rc = Some(PathBuf::from(x)),
            _ => rc = None,
        }
    }
    let Some(rc) = rc else {
        eprintln!("usage: rc64 FILE.RC [-b] [-o OUT]");
        exit(2)
    };
    let die = |why: &dyn std::fmt::Display| -> ! {
        eprintln!("rc64: {why}");
        exit(1)
    };
    let out = out.unwrap_or_else(|| {
        let stem = rc.file_stem().unwrap_or_default().to_string_lossy();
        rc.with_file_name(if binary { format!("{stem}.bin") } else { format!("{stem}_rc.s") })
    });
    // (The compiler looks for files below the script's directory.)
    let full = std::env::current_dir().map_or_else(|_| rc.clone(), |d| d.join(&rc));
    let table = os2_rc::host::compile_table64(&full).unwrap_or_else(|e| die(&e));
    let data = if binary {
        table
    } else {
        let mut t = String::from(".section .rodata\n.globl RS2_RESOURCES\n.type RS2_RESOURCES,@object\n.p2align 2\nRS2_RESOURCES:\n");
        for line in table.chunks(16) {
            let bytes: Vec<String> = line.iter().map(|b| b.to_string()).collect();
            let _ = writeln!(t, "    .byte {}", bytes.join(","));
        }
        t.into_bytes()
    };
    std::fs::write(&out, data).unwrap_or_else(|e| die(&format_args!("{}: {e}", out.display())));
}
