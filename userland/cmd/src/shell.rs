//! The interpreter: environment, prompt, variable expansion and the main
//! loop.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::batch::Batch;
use crate::parse::{self, Sep};
use crate::util::*;

pub struct Shell {
    /// Environment variables (name upper case, value as given).
    pub env: Vec<(String, String)>,
    /// Return code of the last command (%ERRORLEVEL%).
    pub rc: u32,
    pub echo: bool,
    /// Batch files being run (innermost last).
    pub batch: Vec<Batch>,
    pub exit: Option<u32>,
    /// SETLOCAL saves: environment, drive, directory.
    pub locals: Vec<(Vec<(String, String)>, String)>,
}

impl Shell {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, name: &str, value: &str) {
        let name = name.to_ascii_uppercase();
        self.env.retain(|(k, _)| *k != name);
        if !value.is_empty() {
            self.env.push((name, value.to_string()));
        }
    }

    /// The environment block for DosExecPgm.
    pub fn env_block(&self) -> Vec<u8> {
        let mut b = Vec::new();
        for (k, v) in &self.env {
            b.extend_from_slice(k.as_bytes());
            b.push(b'=');
            b.extend_from_slice(v.as_bytes());
            b.push(0);
        }
        b.push(0);
        b
    }

    /// Replace %NAME% with environment values and %0-%9 with batch
    /// arguments; "%%" becomes "%".
    pub fn expand(&self, line: &str) -> String {
        let mut out = String::new();
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] != b'%' {
                out.push(b[i] as char);
                i += 1;
                continue;
            }
            if i + 1 < b.len() && b[i + 1] == b'%' {
                out.push('%');
                i += 2;
                continue;
            }
            if i + 1 < b.len() && b[i + 1].is_ascii_digit() {
                if let Some(bt) = self.batch.last() {
                    out.push_str(&bt.arg((b[i + 1] - b'0') as usize));
                    i += 2;
                    continue;
                }
            }
            if let Some(end) = line[i + 1..].find('%') {
                let name = &line[i + 1..i + 1 + end];
                if !name.is_empty() && !name.contains(' ') {
                    if let Some(v) = self.get(name) {
                        out.push_str(v);
                    }
                    i += end + 2;
                    continue;
                }
            }
            out.push('%');
            i += 1;
        }
        out
    }

    pub fn prompt(&self) {
        let p = self.get("PROMPT").unwrap_or("$i[$p]").to_string();
        let mut s = String::new();
        let mut chars = p.chars();
        while let Some(c) = chars.next() {
            if c != '$' {
                s.push(c);
                continue;
            }
            match chars.next().map(|c| c.to_ascii_uppercase()) {
                Some('P') => s.push_str(&current_dir(0)),
                Some('N') => s.push((b'A' + current_drive() as u8 - 1) as char),
                Some('G') => s.push('>'),
                Some('L') => s.push('<'),
                Some('B') => s.push('|'),
                Some('Q') => s.push('='),
                Some('$') => s.push('$'),
                Some('_') => s.push_str("\r\n"),
                Some('E') => s.push('\x1b'),
                Some('H') => s.push('\x08'),
                Some('S') => s.push(' '),
                Some('A') => s.push('&'),
                Some('C') => s.push('('),
                Some('F') => s.push(')'),
                Some('R') => s.push_str(&self.rc.to_string()),
                Some('V') => s.push_str("rs2 Version 4.52"),
                Some('D') => s.push_str(&crate::builtin::date_string()),
                Some('T') => s.push_str(&crate::builtin::time_string()),
                Some('I') => {
                    // The help line at the top of the screen.
                    s.push_str("\x1b[s\x1b[1;1H\x1b[7m");
                    s.push_str("rs2      Ctrl+Esc = Window List        Type HELP = help                  ");
                    s.push_str("\x1b[0m\x1b[u");
                }
                _ => {}
            }
        }
        out(&s);
    }

    /// Run one input line.
    pub fn run_line(&mut self, line: &str) {
        let line = self.expand(line);
        let mut skip = false;
        let mut last_sep = Sep::Always;
        for (pipeline, sep) in parse::parse(&line) {
            match last_sep {
                Sep::And if self.rc != 0 => skip = true,
                Sep::Or if self.rc == 0 => skip = true,
                _ => {}
            }
            if !skip {
                crate::exec::run_pipeline(self, &pipeline);
            }
            skip = false;
            last_sep = sep;
            if self.exit.is_some() {
                break;
            }
        }
    }
}

pub fn run(s: &os2::Startup) -> u32 {
    let mut sh = Shell { env: Vec::new(), rc: 0, echo: true, batch: Vec::new(), exit: None, locals: Vec::new() };
    // Inherit the environment.
    unsafe {
        let mut p = s.env;
        loop {
            let e = os2::path::from_cstr(p);
            if e.is_empty() {
                break;
            }
            p = p.add(e.len() + 1);
            if let Some((k, v)) = e.split_once('=') {
                sh.env.push((k.to_ascii_uppercase(), v.to_string()));
            }
        }
    }
    let args: String = s.args().iter().map(|&b| b as char).collect();
    let args = args.trim();
    let up = args.to_ascii_uppercase();
    if let Some(rest) = up.strip_prefix("/C").map(|_| &args[2..]) {
        sh.run_line(rest.trim());
        return sh.exit.unwrap_or(sh.rc);
    }
    let mut first = None;
    if up.starts_with("/K") {
        first = Some(args[2..].trim().to_string());
    } else if !args.is_empty() && !up.starts_with('/') {
        first = Some(args.to_string());
    }
    outln("");
    outln("rs2 Command Interpreter version 4.52");
    outln("");
    if let Some(f) = first {
        sh.run_line(&f);
    }
    while sh.exit.is_none() {
        if let Some(line) = crate::batch::next_line(&mut sh) {
            sh.run_line(&line);
            continue;
        }
        outln("");
        sh.prompt();
        let Some(line) = read_line() else { break };
        if !line.trim().is_empty() {
            sh.run_line(&line);
        }
    }
    sh.exit.unwrap_or(sh.rc)
}
