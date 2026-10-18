//! Batch files (.CMD): GOTO, CALL, IF, FOR, SHIFT.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

use crate::parse::Simple;
use crate::shell::Shell;
use crate::util::*;

pub struct Batch {
    pub lines: Vec<String>,
    pub pc: usize,
    /// %0 is the batch file name.
    pub args: Vec<String>,
    pub shift: usize,
    /// Echo state when the batch started (restored at the end).
    pub saved_echo: bool,
}

impl Batch {
    pub fn arg(&self, n: usize) -> String {
        let i = if n == 0 { 0 } else { n + self.shift };
        self.args.get(i).cloned().unwrap_or_default()
    }
}

fn load(path: &str) -> Option<Vec<String>> {
    let c = cstr(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut())
    };
    if rc != 0 {
        return None;
    }
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(hf) };
    let text: String = data.iter().take_while(|&&b| b != 0x1a).map(|&b| b as char).collect();
    Some(text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect())
}

/// Start running a batch file (replaces the current one unless CALLed).
pub fn start(sh: &mut Shell, path: &str, args: &str) {
    let Some(lines) = load(path) else {
        sys_error(2);
        return;
    };
    let mut a = alloc::vec![path.to_string()];
    a.extend(crate::parse::split_words(args));
    let saved_echo = sh.echo;
    // Running a batch file from a batch file (without CALL) ends the
    // first one.
    sh.batch.pop();
    sh.batch.push(Batch { lines, pc: 0, args: a, shift: 0, saved_echo });
}

/// Next line of the innermost batch file, echoed if ECHO is on.
pub fn next_line(sh: &mut Shell) -> Option<String> {
    loop {
        let b = sh.batch.last_mut()?;
        if b.pc >= b.lines.len() {
            let done = sh.batch.pop().unwrap();
            if sh.batch.is_empty() {
                sh.echo = done.saved_echo;
            }
            continue;
        }
        let line = b.lines[b.pc].clone();
        b.pc += 1;
        let t = line.trim();
        if t.is_empty() || t.starts_with(':') {
            continue;
        }
        let (quiet, cmd) = match t.strip_prefix('@') {
            Some(r) => (true, r.to_string()),
            None => (false, t.to_string()),
        };
        if sh.echo && !quiet {
            outln("");
            sh.prompt();
            outln(&cmd);
        }
        return Some(cmd);
    }
}

pub fn goto(sh: &mut Shell, label: &str) -> u32 {
    let label = label.trim_start_matches(':').to_ascii_uppercase();
    let Some(b) = sh.batch.last_mut() else { return 0 };
    let found = b.lines.iter().position(|l| {
        let t = l.trim();
        t.starts_with(':') && t[1..].split_whitespace().next().is_some_and(|n| n.eq_ignore_ascii_case(&label))
    });
    match found {
        Some(i) => {
            b.pc = i + 1;
            0
        }
        None => {
            err_out(&alloc::format!("SYS1035: The label {label} was not found.\r\n"));
            sh.batch.pop();
            1
        }
    }
}

pub fn call(sh: &mut Shell, s: &Simple) -> u32 {
    let words = s.words();
    let Some(name) = words.first() else { return 0 };
    let rest = s.args.trim_start().strip_prefix(name.as_str()).unwrap_or("").trim().to_string();
    match crate::exec::find_program(sh, name) {
        Some(p) if p.to_ascii_uppercase().ends_with(".CMD") => {
            let Some(lines) = load(&p) else { return 2 };
            let mut a = alloc::vec![p];
            a.extend(crate::parse::split_words(&rest));
            let saved_echo = sh.echo;
            sh.batch.push(Batch { lines, pc: 0, args: a, shift: 0, saved_echo });
            0
        }
        _ => {
            sh.run_line(&s.args);
            sh.rc
        }
    }
}

pub fn shift(sh: &mut Shell) -> u32 {
    if let Some(b) = sh.batch.last_mut() {
        b.shift += 1;
    }
    0
}

/// IF [NOT] ERRORLEVEL n | EXIST file | a==b  command
pub fn if_cmd(sh: &mut Shell, s: &Simple) -> u32 {
    let mut rest = s.args.trim();
    let mut not = false;
    if rest.len() > 4 && rest[..4].eq_ignore_ascii_case("NOT ") {
        not = true;
        rest = rest[4..].trim_start();
    }
    let up = rest.to_ascii_uppercase();
    let (cond, cmd) = if up.starts_with("ERRORLEVEL ") {
        let r = rest[11..].trim_start();
        let (n, c) = r.split_once(' ').unwrap_or((r, ""));
        (sh.rc >= n.parse::<u32>().unwrap_or(0), c)
    } else if up.starts_with("EXIST ") {
        let r = rest[6..].trim_start();
        let (f, c) = r.split_once(' ').unwrap_or((r, ""));
        let f = f.trim_matches('"');
        let ok = if has_wildcards(f) { find(f, 0).is_ok() } else { exists(f) };
        (ok, c)
    } else if let Some(i) = rest.find("==") {
        let a = rest[..i].trim();
        let r = rest[i + 2..].trim_start();
        let (b, c) = split_operand(r);
        (a.trim_matches('"') == b.trim_matches('"'), c)
    } else {
        err_out("SYS1003: The syntax of the command is incorrect.\r\n");
        return 1;
    };
    if cond != not {
        sh.run_line(cmd.trim());
        sh.rc
    } else {
        0
    }
}

/// First operand (respecting quotes) and the rest.
fn split_operand(s: &str) -> (&str, &str) {
    if let Some(r) = s.strip_prefix('"') {
        if let Some(e) = r.find('"') {
            return (&s[..e + 2], &s[e + 2..]);
        }
    }
    s.split_once(' ').unwrap_or((s, ""))
}

/// FOR %%v IN (set) DO command
pub fn for_cmd(sh: &mut Shell, s: &Simple) -> u32 {
    let a = s.args.trim();
    let mut it = a.splitn(2, ' ');
    let var = it.next().unwrap_or("").trim_start_matches('%').to_string();
    let rest = it.next().unwrap_or("").trim_start();
    if !rest.to_ascii_uppercase().starts_with("IN") {
        err_out("SYS1003: The syntax of the command is incorrect.\r\n");
        return 1;
    }
    let rest = rest[2..].trim_start();
    let (Some(open), Some(close)) = (rest.find('('), rest.find(')')) else { return 1 };
    let set = &rest[open + 1..close];
    let after = rest[close + 1..].trim_start();
    if !after.to_ascii_uppercase().starts_with("DO") {
        err_out("SYS1003: The syntax of the command is incorrect.\r\n");
        return 1;
    }
    let cmd = after[2..].trim_start();
    let mut items = Vec::new();
    for w in crate::parse::split_words(set) {
        if has_wildcards(&w) {
            let (dir, _) = split_path(&w);
            if let Ok(f) = find(&w, 0) {
                items.extend(f.into_iter().map(|x| alloc::format!("{dir}{}", x.name)));
            }
        } else {
            items.push(w);
        }
    }
    let pat1 = alloc::format!("%%{var}");
    let pat2 = alloc::format!("%{var}");
    for item in items {
        let line = cmd.replace(&pat1, &item).replace(&pat2, &item);
        sh.run_line(&line);
        if sh.exit.is_some() {
            break;
        }
    }
    sh.rc
}
