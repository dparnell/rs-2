//! rs2's COMMAND.COM.
//!
//! The file on disk ([`command_com`]) is a few bytes of code: a callback
//! into this module and an `INT 21h` in a loop. The shell itself is Rust:
//! each callback it decides what comes next and, when that needs the
//! guest (reading a line, running a program, waiting for a key), leaves
//! the registers for one DOS call that the stub then makes; the next
//! callback sees its result. So programs are run through INT 21h 4B00h
//! as with any shell, the line is read through 0Ah (TSRs hooking INT 21h
//! see it), and redirection works through the shell's own handles, which
//! children inherit.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use x86emu::{Seg, CS, DS, ES, ESP, SS};

use crate::dos::{fcb_form, matches, Name};
use crate::host::Create;
use crate::{Cb, Host, Machine};

pub const CB_SHELL: u16 = 0x102;

/// Where the shell keeps things in its PSP segment.
const LINE_BUF: u16 = 0x180;
const EXEC_BLOCK: u16 = 0x210;
const EXEC_TAIL: u16 = 0x220;
const EXEC_FCB1: u16 = 0x2a0;
const EXEC_FCB2: u16 = 0x2c0;
const EXEC_PATH: u16 = 0x2e0;
/// The shell's memory block (paragraphs) and stack.
const SHELL_PARAS: u16 = 0x80;
const SHELL_SP: u16 = 0x7fe;
const ENV_DEFAULT: usize = 1024;

/// The bytes of COMMAND.COM.
pub fn command_com() -> Vec<u8> {
    let mut v = Vec::new();
    // jmp short over the signature
    v.extend_from_slice(&[0xeb, 0x1e]);
    let sig = b"rs2 COMMAND.COM 1.0\r\n\x1a";
    v.extend_from_slice(sig);
    while v.len() < 0x20 {
        v.push(0);
    }
    // loop: callback; INT 21h (or INT 2Fh when BP = 1); jmp loop
    v.extend_from_slice(&[0xfe, 0x38, CB_SHELL as u8, (CB_SHELL >> 8) as u8, 0x83, 0xfd, 0x01, 0x74, 0x04, 0xcd, 0x21, 0xeb, 0xf3, 0xcd, 0x2f, 0xeb, 0xef]);
    v
}

#[derive(Clone, Debug)]
enum Pending {
    Init,
    /// INT 2Fh AX=4800h: is DOSKEY there (it then reads the line)?
    CheckDoskey,
    ReadLine,
    Exec,
    /// A key for PAUSE.
    Key,
    Idle,
}

struct Batch {
    lines: Vec<Vec<u8>>,
    pos: usize,
    args: Vec<String>,
    shift: usize,
}

/// One command of a pipeline, with its redirections.
#[derive(Clone, Debug, Default)]
struct Stage {
    cmd: String,
    input: Option<String>,
    output: Option<(String, bool)>,
}

pub struct Shell {
    pub psp: u16,
    pending: Pending,
    echo: bool,
    permanent: bool,
    /// /C (exit after) or /K (stay) command.
    single: Option<(String, bool)>,
    batches: Vec<Batch>,
    stages: VecDeque<Stage>,
    temps: Vec<String>,
    /// Lines queued by FOR.
    queued: VecDeque<String>,
    /// Handles 0 and 1 before redirection (SFT indices).
    saved: Option<[u8; 2]>,
    errorlevel: u8,
    /// Ctrl+C: drop the batch and the line.
    broken: bool,
    /// After EXIT or /C.
    leaving: bool,
    /// Loading high (LH) for the program now running.
    high: Option<(u16, bool)>,
    /// A command from the keyboard ran: a blank line before the prompt.
    ran: bool,
}

impl Shell {
    fn new(psp: u16) -> Shell {
        Shell {
            psp,
            pending: Pending::Init,
            echo: true,
            permanent: false,
            single: None,
            batches: Vec::new(),
            stages: VecDeque::new(),
            temps: Vec::new(),
            queued: VecDeque::new(),
            saved: None,
            errorlevel: 0,
            broken: false,
            leaving: false,
            high: None,
            ran: false,
        }
    }
}

fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

fn text(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

const INTERNAL: &[&str] = &[
    "BREAK", "CALL", "CD", "CHDIR", "CLS", "COPY", "CTTY", "DATE", "DEL", "DIR", "ECHO", "ERASE", "EXIT", "FOR", "GOTO", "IF", "LH", "LOADHIGH", "MD", "MKDIR", "PATH",
    "PAUSE", "PROMPT", "RD", "REM", "REN", "RENAME", "RMDIR", "SET", "SHIFT", "TIME", "TRUENAME", "TYPE", "VER", "VERIFY", "VOL",
];

/// Split a command line into the command word and the rest: the word
/// ends at a blank or a switch, and an internal command also at the
/// characters DOS allows right after one ("CD..", "ECHO.", "CD\").
fn split_cmd(line: &str) -> (String, String) {
    let t = line.trim_start_matches([' ', '\t', '@']);
    let end = t.find([' ', '\t', '/']).unwrap_or(t.len());
    let word = &t[..end];
    let u = upper(word);
    for name in INTERNAL {
        if u.len() > name.len() && u.starts_with(name) && u[name.len()..].starts_with(['.', '\\', ':', '+', '=', ',', ';', '[', ']']) {
            return (t[..name.len()].to_string(), t[name.len()..].to_string());
        }
    }
    (word.to_string(), t[end..].to_string())
}

impl Machine {
    fn shell_index(&self, psp: u16) -> Option<usize> {
        self.shells.iter().position(|s| s.psp == psp)
    }

    /// The shell whose PSP is current (creating its state on the first
    /// callback).
    pub(crate) fn shell_step<H: Host>(&mut self, host: &mut H) -> Cb {
        let psp = self.dos.psp;
        let i = match self.shell_index(psp) {
            Some(i) => i,
            None => {
                self.shells.push(Shell::new(psp));
                self.shells.len() - 1
            }
        };
        let pending = core::mem::replace(&mut self.shells[i].pending, Pending::Idle);
        self.cpu.regs[x86emu::EBP] = 0;
        match pending {
            Pending::Init => self.shell_init(host, i),
            Pending::CheckDoskey => {
                // DOSKEY answers with AL non-zero; it then edits the line (4810h).
                let psp = self.shells[i].psp;
                let a = Machine::lin(psp, LINE_BUF);
                self.hw.wr(a, 128);
                self.hw.wr(a + 1, 0);
                self.cpu.seg[DS] = Seg::real(psp);
                self.cpu.set_dx(LINE_BUF);
                if self.cpu.al() != 0 {
                    self.cpu.set_ax(0x4810);
                    self.cpu.regs[x86emu::EBP] = 1;
                } else {
                    self.cpu.set_ah(0x0a);
                }
                self.shells[i].pending = Pending::ReadLine;
                return Cb::Done;
            }
            Pending::ReadLine => {
                let a = Machine::lin(psp, LINE_BUF);
                let n = self.hw.rd(a + 1) as usize;
                let mut line = alloc::vec![0u8; n];
                self.hw.read_bytes(a + 2, &mut line);
                self.out(host, b"\r\n");
                if self.shells[i].broken {
                    self.shells[i].broken = false;
                } else {
                    let l = text(&line);
                    self.shells[i].ran = !l.trim().is_empty();
                    self.shell_line(host, i, &l, false);
                }
            }
            Pending::Exec => {
                let failed = self.ret_flags_live() & 1 != 0;
                let code = if failed { self.cpu.ax() } else { 0 };
                if failed {
                    let msg = match code {
                        2 | 3 => "Bad command or file name\r\n",
                        8 => "Insufficient memory\r\n",
                        11 => "Bad format\r\n",
                        _ => "Cannot execute the program\r\n",
                    };
                    self.out(host, msg.as_bytes());
                    self.shells[i].errorlevel = 1;
                } else {
                    self.shells[i].errorlevel = self.dos.return_code as u8;
                    if self.dos.return_code >> 8 == 1 {
                        // Ended with Ctrl+C: so is a batch.
                        self.shells[i].batches.clear();
                        self.shells[i].queued.clear();
                    }
                }
                self.dos.return_code = 0;
                if let Some((strat, link)) = self.shells[i].high.take() {
                    self.dos.strategy = strat;
                    self.set_umb_link(link);
                }
                self.restore_handles(host, i);
            }
            Pending::Key => {}
            Pending::Idle => {}
        }
        self.shell_next(host, i)
    }

    /// The live CF (the stub's INT 21h has returned).
    fn ret_flags_live(&self) -> u32 {
        self.cpu.flags
    }

    fn shell_init<H: Host>(&mut self, host: &mut H, i: usize) {
        let psp = self.shells[i].psp;
        // Keep a small block; the stack inside it.
        let _ = self.resize_mem(psp, SHELL_PARAS);
        self.hw.wr16(Machine::lin(psp, 2), psp + SHELL_PARAS);
        self.cpu.set16(ESP, SHELL_SP);
        // Command tail: /C, /K, /P, /E:n.
        let a = Machine::lin(psp, 0x80);
        let n = self.hw.rd(a) as usize;
        let mut tail = alloc::vec![0u8; n];
        self.hw.read_bytes(a + 1, &mut tail);
        let tail = text(&tail);
        let mut env_size = ENV_DEFAULT;
        let mut rest = tail.trim().to_string();
        loop {
            let u = upper(&rest);
            if u.starts_with("/C") || u.starts_with("/K") {
                let stay = u.starts_with("/K");
                self.shells[i].single = Some((rest[2..].trim().to_string(), stay));
                break;
            } else if u.starts_with("/P") {
                self.shells[i].permanent = true;
                rest = rest[2..].trim_start().to_string();
            } else if u.starts_with("/E:") {
                let v: String = rest[3..].chars().take_while(|c| c.is_ascii_digit()).collect();
                env_size = v.parse::<usize>().unwrap_or(ENV_DEFAULT).clamp(160, 32768);
                rest = rest[3 + v.len()..].trim_start().to_string();
            } else if u.starts_with('/') {
                // Unknown switch (/D, /MSG...): skip it.
                let k = rest.find(' ').unwrap_or(rest.len());
                rest = rest[k..].trim_start().to_string();
            } else {
                break;
            }
        }
        // An environment of its own with room to grow.
        let env = self.hw.rd16(Machine::lin(psp, 0x2c));
        let mut vars = self.env_strings(env);
        let comspec = self.shell_path(host, psp);
        if !vars.iter().any(|v| v.len() > 8 && v[..8].eq_ignore_ascii_case(b"COMSPEC=")) {
            vars.push(format!("COMSPEC={comspec}").into_bytes());
        }
        if !vars.iter().any(|v| v.len() > 5 && v[..5].eq_ignore_ascii_case(b"PATH=")) {
            vars.push(b"PATH=".to_vec());
        }
        if !vars.iter().any(|v| v.len() > 7 && v[..7].eq_ignore_ascii_case(b"PROMPT=")) {
            vars.push(b"PROMPT=$P$G".to_vec());
        }
        if let Ok(seg) = self.alloc_strategy((env_size as u32).div_ceil(16) as u16, psp, 0) {
            self.hw.wr16(Machine::lin(psp, 0x2c), seg);
            self.write_env(seg, &vars);
            if env != 0 && self.shell_index_parent_owns(env, psp) {
                let _ = self.free_mem(env);
            }
        }
        if self.shells[i].single.is_none() && psp == self.dos.root_psp {
            self.shells[i].permanent = false;
        }
        if self.shells[i].single.is_none() {
            // An interactive shell shows its version.
            let (maj, min) = self.cfg.version;
            let v = format!("\r\nrs2 DOS session (DOS {}.{:02})\r\n", maj, min);
            self.out(host, v.as_bytes());
        }
        let _ = host;
    }

    /// True if `env` was the shell's (copied) environment block.
    fn shell_index_parent_owns(&mut self, env: u16, psp: u16) -> bool {
        self.mcb(env - 1).1 == psp
    }

    /// The path of the running COMMAND.COM (from its environment).
    fn shell_path<H: Host>(&mut self, host: &mut H, psp: u16) -> String {
        let _ = host;
        let env = self.hw.rd16(Machine::lin(psp, 0x2c));
        if env != 0 {
            // After the variables: a count word and the program path.
            let mut a = Machine::lin(env, 0);
            for _ in 0..1024 {
                let s = self.hw.cstr(a, 32768);
                a += s.len() as u32 + 1;
                if s.is_empty() {
                    break;
                }
            }
            if self.hw.rd16(a) >= 1 {
                let p = self.hw.cstr(a + 2, 128);
                if !p.is_empty() {
                    return text(&p);
                }
            }
        }
        String::from("C:\\COMMAND.COM")
    }

    fn write_env(&mut self, seg: u16, vars: &[Vec<u8>]) {
        let size = self.mcb(seg - 1).2 as usize * 16;
        let mut data = Vec::new();
        for v in vars {
            if data.len() + v.len() + 4 >= size {
                break;
            }
            data.extend_from_slice(v);
            data.push(0);
        }
        data.push(0);
        data.extend_from_slice(&0u16.to_le_bytes());
        self.hw.write_bytes(Machine::lin(seg, 0), &data);
    }

    /// The current process's environment as strings.
    fn shell_env(&mut self) -> (u16, Vec<Vec<u8>>) {
        let psp = self.dos.psp;
        let env = self.hw.rd16(Machine::lin(psp, 0x2c));
        let v = self.env_strings(env);
        (env, v)
    }

    fn set_var(&mut self, host: &mut impl Host, name: &str, value: &str) {
        let (env, mut vars) = self.shell_env();
        let key = upper(name);
        vars.retain(|v| !(v.len() > key.len() && v[..key.len()].eq_ignore_ascii_case(key.as_bytes()) && v[key.len()] == b'='));
        if !value.is_empty() {
            vars.push(format!("{key}={value}").into_bytes());
        }
        let size = self.mcb(env - 1).2 as usize * 16;
        let need: usize = vars.iter().map(|v| v.len() + 1).sum::<usize>() + 3;
        if need > size {
            self.out(host, b"Out of environment space\r\n");
            return;
        }
        self.write_env(env, &vars);
    }

    /// Write to the shell's standard output.
    fn out<H: Host>(&mut self, host: &mut H, s: &[u8]) {
        self.con_write_std(host, s);
    }

    fn outs<H: Host>(&mut self, host: &mut H, s: &str) {
        self.out(host, s.as_bytes());
    }

    /// What to do next: a pipeline stage, a queued line, the next batch
    /// line, the /C command, or the prompt.
    fn shell_next<H: Host>(&mut self, host: &mut H, i: usize) -> Cb {
        loop {
            if self.shells[i].broken {
                self.shells[i].broken = false;
                self.shells[i].batches.clear();
                self.shells[i].queued.clear();
                self.shells[i].stages.clear();
                self.restore_handles(host, i);
            }
            if self.exited.is_some() || self.dos.psp != self.shells[i].psp {
                return Cb::Done;
            }
            if let Some(st) = self.shells[i].stages.pop_front() {
                if self.run_stage(host, i, st) {
                    return Cb::Done;
                }
                continue;
            }
            self.cleanup_temps(host, i);
            if self.shells[i].leaving {
                let code = self.shells[i].errorlevel;
                self.shells.remove(i);
                self.dos_exit(host, code, 0);
                return Cb::Done;
            }
            if let Some(l) = self.shells[i].queued.pop_front() {
                self.shell_line(host, i, &l, true);
                continue;
            }
            if let Some(b) = self.shells[i].batches.last_mut() {
                if b.pos < b.lines.len() {
                    let l = text(&b.lines[b.pos]);
                    b.pos += 1;
                    self.shell_line(host, i, &l, true);
                    continue;
                }
                self.shells[i].batches.pop();
                if self.shells[i].batches.is_empty() && matches!(self.shells[i].single, Some((_, false))) && self.shells[i].single.as_ref().is_some_and(|s| s.0.is_empty()) {
                    self.shells[i].leaving = true;
                }
                continue;
            }
            if let Some((cmd, stay)) = self.shells[i].single.take() {
                if !cmd.is_empty() {
                    if !stay {
                        // Leave once the command (and any batch) is done.
                        self.shells[i].single = Some((String::new(), false));
                    }
                    self.shell_line(host, i, &cmd, false);
                    continue;
                }
                if !stay {
                    self.shells[i].leaving = true;
                    continue;
                }
            }
            // Interactive: the prompt, then read a line.
            if core::mem::take(&mut self.shells[i].ran) && self.shells[i].echo {
                self.out(host, b"\r\n");
            }
            self.prompt(host);
            self.cpu.set_ax(0x4800);
            self.cpu.regs[x86emu::EBP] = 1;
            self.shells[i].pending = Pending::CheckDoskey;
            return Cb::Done;
        }
    }

    fn prompt<H: Host>(&mut self, host: &mut H) {
        let p = self.getenv("PROMPT").unwrap_or_else(|| String::from("$P$G"));
        let mut out = String::new();
        let mut it = p.chars();
        while let Some(c) = it.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            match it.next().map(|c| c.to_ascii_uppercase()) {
                Some('P') => {
                    let d = self.dos.drive;
                    out.push((b'A' + d) as char);
                    out.push_str(":\\");
                    out.push_str(&self.dos.cwd[d as usize]);
                }
                Some('N') => out.push((b'A' + self.dos.drive) as char),
                Some('G') => out.push('>'),
                Some('L') => out.push('<'),
                Some('B') => out.push('|'),
                Some('Q') => out.push('='),
                Some('$') => out.push('$'),
                Some('_') => out.push_str("\r\n"),
                Some('E') => out.push('\x1b'),
                Some('H') => out.push('\x08'),
                Some('V') => {
                    let (a, b) = self.cfg.version;
                    out.push_str(&format!("rs2 DOS Version {}.{:02}", a, b));
                }
                Some('D') => {
                    let n = host.now();
                    out.push_str(&format!("{} {:02}-{:02}-{:04}", ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][n.weekday as usize % 7], n.month, n.day, n.year));
                }
                Some('T') => {
                    let n = host.now();
                    out.push_str(&format!("{:2}:{:02}:{:02}.{:02}", n.hour, n.minute, n.second, n.hundredths));
                }
                _ => {}
            }
        }
        self.outs(host, &out);
    }

    /// Expand %VAR% (and in batches %0-%9, %%).
    fn expand(&mut self, i: usize, line: &str) -> String {
        let mut out = String::new();
        let b: Vec<char> = line.chars().collect();
        let mut k = 0;
        let in_batch = !self.shells[i].batches.is_empty();
        while k < b.len() {
            let c = b[k];
            if c != '%' {
                out.push(c);
                k += 1;
                continue;
            }
            if k + 1 < b.len() && b[k + 1] == '%' {
                out.push('%');
                if in_batch {
                    k += 2;
                } else {
                    k += 1;
                }
                continue;
            }
            if in_batch && k + 1 < b.len() && b[k + 1].is_ascii_digit() {
                let n = b[k + 1] as usize - '0' as usize;
                let bt = self.shells[i].batches.last().unwrap();
                if let Some(a) = bt.args.get(n + bt.shift) {
                    out.push_str(a);
                }
                k += 2;
                continue;
            }
            if let Some(end) = b[k + 1..].iter().position(|&c| c == '%') {
                let name: String = b[k + 1..k + 1 + end].iter().collect();
                if !name.is_empty() && !name.contains(' ') {
                    if let Some(v) = self.getenv(&name) {
                        out.push_str(&v);
                    }
                    k += end + 2;
                    continue;
                }
            }
            if !in_batch {
                out.push('%');
            }
            k += 1;
        }
        out
    }

    /// Run one command line (from the keyboard, a batch, /C).
    fn shell_line<H: Host>(&mut self, host: &mut H, i: usize, raw: &str, from_batch: bool) {
        let raw = raw.trim_end_matches(['\r', '\n', '\x1a']);
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.starts_with(':') {
            return;
        }
        let quiet = trimmed.starts_with('@');
        let line = self.expand(i, trimmed.trim_start_matches('@'));
        if from_batch && self.shells[i].echo && !quiet && !self.shells[i].batches.is_empty() {
            self.out(host, b"\r\n");
            self.prompt(host);
            self.outs(host, &line);
            self.out(host, b"\r\n");
        }
        // Split the pipeline and the redirections.
        let mut stages = Vec::new();
        for part in line.split('|') {
            let mut st = Stage::default();
            let mut cmd = String::new();
            let mut chars = part.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '<' | '>' => {
                        let append = c == '>' && chars.peek() == Some(&'>');
                        if append {
                            chars.next();
                        }
                        while chars.peek() == Some(&' ') {
                            chars.next();
                        }
                        let mut f = String::new();
                        while let Some(&c) = chars.peek() {
                            if c == ' ' || c == '<' || c == '>' {
                                break;
                            }
                            f.push(c);
                            chars.next();
                        }
                        if c == '<' {
                            st.input = Some(f);
                        } else {
                            st.output = Some((f, append));
                        }
                    }
                    _ => cmd.push(c),
                }
            }
            st.cmd = cmd.trim().to_string();
            stages.push(st);
        }
        // Pipes through temporary files.
        let n = stages.len();
        if n > 1 {
            let tmp = self.getenv("TEMP").or_else(|| self.getenv("TMP")).unwrap_or_else(|| format!("{}:\\", (b'A' + self.dos.drive) as char));
            let tmp = if tmp.ends_with('\\') { tmp } else { format!("{tmp}\\") };
            for k in 0..n - 1 {
                let f = format!("{tmp}PIPE{:04X}.$$$", (self.hw.us_now as u16).wrapping_add(k as u16 * 7));
                if stages[k].output.is_none() {
                    stages[k].output = Some((f.clone(), false));
                }
                if stages[k + 1].input.is_none() {
                    stages[k + 1].input = Some(f.clone());
                }
                self.shells[i].temps.push(f);
            }
        }
        self.shells[i].stages.extend(stages);
    }

    /// Redirect handles 0 and 1 for a stage (keeping the old ones).
    fn redirect<H: Host>(&mut self, host: &mut H, i: usize, st: &Stage) -> bool {
        if st.input.is_none() && st.output.is_none() {
            return true;
        }
        let psp = self.shells[i].psp;
        let jft = Machine::lin(psp, 0x18);
        let old = [self.hw.rd(jft), self.hw.rd(jft + 1)];
        if let Some(f) = &st.input {
            let n = match self.resolve(f.as_bytes()) {
                Ok(n) => n,
                Err(_) => {
                    self.outs(host, "File not found\r\n");
                    return false;
                }
            };
            match self.open_name(host, &n, 0x40, None) {
                Ok(s) => self.hw.wr(jft, s as u8),
                Err(_) => {
                    self.outs(host, "File not found\r\n");
                    return false;
                }
            }
        }
        if let Some((f, append)) = &st.output {
            let n = match self.resolve(f.as_bytes()) {
                Ok(n) => n,
                Err(_) => {
                    self.outs(host, "File creation error\r\n");
                    return false;
                }
            };
            let r = if *append {
                self.open_name(host, &n, 0x41, None).inspect(|&s| {
                    if let Some(Some(e)) = self.dos.sft.get(s) {
                        if e.dev.is_none() {
                            let _ = host.seek(e.host, 0, 2);
                        }
                    }
                })
            } else {
                Err(0)
            };
            let r = r.or_else(|_| self.open_name(host, &n, 0x41, Some((0, Create::Truncate))));
            match r {
                Ok(s) => self.hw.wr(jft + 1, s as u8),
                Err(_) => {
                    self.outs(host, "File creation error\r\n");
                    return false;
                }
            }
        }
        self.shells[i].saved = Some(old);
        true
    }

    fn restore_handles<H: Host>(&mut self, host: &mut H, i: usize) {
        let Some(old) = self.shells[i].saved.take() else { return };
        let psp = self.shells[i].psp;
        let jft = Machine::lin(psp, 0x18);
        for (h, &o) in old.iter().enumerate() {
            let cur = self.hw.rd(jft + h as u32);
            if cur != o {
                self.release_sft(host, cur as usize);
                self.hw.wr(jft + h as u32, o);
            }
        }
    }

    fn cleanup_temps<H: Host>(&mut self, host: &mut H, i: usize) {
        for f in core::mem::take(&mut self.shells[i].temps) {
            if let Ok(Name::File(p)) = self.resolve(f.as_bytes()) {
                let _ = host.delete(&p);
            }
        }
    }

    /// Run a pipeline stage: true if the stub has a DOS call to make.
    fn run_stage<H: Host>(&mut self, host: &mut H, i: usize, st: Stage) -> bool {
        if !self.redirect(host, i, &st) {
            self.restore_handles(host, i);
            self.shells[i].stages.clear();
            return false;
        }
        let r = self.command(host, i, &st.cmd);
        if !r {
            self.restore_handles(host, i);
        }
        r
    }

    /// Execute a command; true if the stub has a DOS call to make (the
    /// handles stay redirected until it returns).
    fn command<H: Host>(&mut self, host: &mut H, i: usize, line: &str) -> bool {
        let (word, rest) = split_cmd(line);
        if word.is_empty() {
            return false;
        }
        let w = upper(&word);
        let args = rest.trim().to_string();
        // A drive change.
        if w.len() == 2 && w.ends_with(':') && args.is_empty() {
            let d = w.as_bytes()[0].wrapping_sub(b'A');
            if d < 26 && self.dos.present[d as usize] {
                self.dos.drive = d;
            } else {
                self.outs(host, "Invalid drive specification\r\n");
            }
            return false;
        }
        match w.as_str() {
            "REM" => {}
            "ECHO" => {
                let r = rest.strip_prefix([' ', '.', '/', '\t']).unwrap_or(&rest);
                let u = upper(r.trim());
                if rest.is_empty() {
                    let on = if self.shells[i].echo { "on" } else { "off" };
                    self.outs(host, &format!("ECHO is {on}\r\n"));
                } else if u == "OFF" && !rest.starts_with('.') {
                    self.shells[i].echo = false;
                } else if u == "ON" && !rest.starts_with('.') {
                    self.shells[i].echo = true;
                } else {
                    self.outs(host, r);
                    self.out(host, b"\r\n");
                }
            }
            "CLS" => {
                let m = self.bda_mode();
                self.set_mode(m, false);
            }
            "VER" => {
                let (a, b) = self.cfg.version;
                self.outs(host, &format!("\r\nrs2 DOS session, version {}.{:02}\r\n", a, b));
            }
            "EXIT" => {
                if !self.shells[i].permanent {
                    self.shells[i].leaving = true;
                    self.shells[i].batches.clear();
                }
            }
            "SET" => {
                if args.is_empty() {
                    let (_, vars) = self.shell_env();
                    for v in vars {
                        self.out(host, &v);
                        self.out(host, b"\r\n");
                    }
                } else if let Some((k, v)) = args.split_once('=') {
                    self.set_var(host, k.trim(), v);
                } else if let Some(v) = self.getenv(args.trim()) {
                    self.outs(host, &format!("{}={}\r\n", upper(args.trim()), v));
                }
            }
            "PATH" => {
                let a = rest.trim_start_matches(['=', ' ']).trim();
                if a.is_empty() && !rest.contains('=') && !rest.contains(';') {
                    let p = self.getenv("PATH").unwrap_or_default();
                    if p.is_empty() {
                        self.outs(host, "No Path\r\n");
                    } else {
                        self.outs(host, &format!("PATH={p}\r\n"));
                    }
                } else {
                    let v = if a == ";" { "" } else { a };
                    self.set_var(host, "PATH", &upper(v));
                }
            }
            "PROMPT" => {
                let a = rest.trim_start_matches(['=', ' ']);
                self.set_var(host, "PROMPT", a);
            }
            "CD" | "CHDIR" => {
                if args.is_empty() || (args.len() == 2 && args.ends_with(':')) {
                    let d = if args.is_empty() { self.dos.drive } else { upper(&args).as_bytes()[0] - b'A' };
                    let s = format!("{}:\\{}\r\n", (b'A' + d) as char, self.dos.cwd.get(d as usize).cloned().unwrap_or_default());
                    self.outs(host, &s);
                } else if self.chdir(host, &args).is_err() {
                    self.outs(host, "Invalid directory\r\n");
                }
            }
            "MD" | "MKDIR" => match self.resolve(args.as_bytes()) {
                Ok(Name::File(f)) if host.mkdir(&f).is_ok() => {}
                _ => self.outs(host, "Unable to create directory\r\n"),
            },
            "RD" | "RMDIR" => match self.resolve(args.as_bytes()) {
                Ok(Name::File(f)) if host.rmdir(&f).is_ok() => {}
                _ => self.outs(host, "Invalid path, not directory,\r\nor directory not empty\r\n"),
            },
            "DIR" => self.dir(host, &args),
            "TYPE" => self.type_file(host, &args),
            "DEL" | "ERASE" => self.del(host, &args),
            "REN" | "RENAME" => self.ren(host, &args),
            "COPY" => self.copy(host, &args),
            "VOL" => {
                let d = if args.len() >= 2 && args.as_bytes()[1] == b':' { upper(&args).as_bytes()[0] - b'A' } else { self.dos.drive };
                self.vol(host, d);
            }
            "DATE" => {
                let n = host.now();
                let day = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][n.weekday as usize % 7];
                self.outs(host, &format!("Current date is {day} {:02}-{:02}-{:04}\r\n", n.month, n.day, n.year));
            }
            "TIME" => {
                let n = host.now();
                self.outs(host, &format!("Current time is {:2}:{:02}:{:02}.{:02}\r\n", n.hour, n.minute, n.second, n.hundredths));
            }
            "TRUENAME" => match self.resolve(if args.is_empty() { b"." } else { args.as_bytes() }) {
                Ok(Name::File(f)) => self.outs(host, &format!("{f}\r\n")),
                Ok(Name::Dev(d)) => self.outs(host, &format!("{}:/{}\r\n", (b'A' + self.dos.drive) as char, upper(&format!("{d:?}")))),
                Err(_) => self.outs(host, "Invalid path\r\n"),
            },
            "BREAK" => {
                let u = upper(&args);
                if u == "ON" {
                    self.dos.break_on = true;
                } else if u == "OFF" {
                    self.dos.break_on = false;
                } else {
                    let s = if self.dos.break_on { "on" } else { "off" };
                    self.outs(host, &format!("BREAK is {s}\r\n"));
                }
            }
            "VERIFY" => {
                let u = upper(&args);
                if u == "ON" || u == "OFF" {
                    self.dos.verify = u == "ON";
                } else {
                    let s = if self.dos.verify { "on" } else { "off" };
                    self.outs(host, &format!("VERIFY is {s}\r\n"));
                }
            }
            "PAUSE" => {
                self.outs(host, "Press any key to continue . . .");
                self.shells[i].queued.push_front(String::from("ECHO."));
                self.cpu.set_ah(0x08);
                self.shells[i].pending = Pending::Key;
                return true;
            }
            "GOTO" => {
                let label = upper(args.trim_start_matches(':').trim());
                let label: String = label.chars().take(8).collect();
                if let Some(b) = self.shells[i].batches.last_mut() {
                    let at = b.lines.iter().position(|l| {
                        let t = text(l);
                        let t = t.trim_start();
                        t.starts_with(':') && upper(t[1..].trim()).chars().take(8).collect::<String>() == label
                    });
                    match at {
                        Some(p) => b.pos = p + 1,
                        None => {
                            b.pos = b.lines.len();
                            self.outs(host, "Label not found\r\n");
                        }
                    }
                }
            }
            "SHIFT" => {
                if let Some(b) = self.shells[i].batches.last_mut() {
                    b.shift += 1;
                }
            }
            "IF" => return self.if_cmd(host, i, &args),
            "FOR" => self.for_cmd(host, i, &args),
            "CALL" => {
                let (w2, r2) = split_cmd(&args);
                if let Some(p) = self.find_program(host, &w2) {
                    if p.ends_with(".BAT") {
                        self.start_batch(host, i, &p, &r2, true);
                        return false;
                    }
                }
                return self.command(host, i, &args);
            }
            "LH" | "LOADHIGH" => {
                let link = self.dos.umb_link;
                self.shells[i].high = Some((self.dos.strategy, link));
                self.set_umb_link(true);
                self.dos.strategy = 0x80;
                let r = self.command(host, i, &args);
                if !r {
                    if let Some((s, l)) = self.shells[i].high.take() {
                        self.dos.strategy = s;
                        self.set_umb_link(l);
                    }
                }
                return r;
            }
            "CTTY" | "LOCK" | "UNLOCK" => {}
            _ => return self.external(host, i, &word, &rest),
        }
        false
    }

    fn chdir<H: Host>(&mut self, host: &mut H, path: &str) -> Result<(), u16> {
        let full = match self.resolve(path.trim().as_bytes())? {
            Name::File(f) => f,
            Name::Dev(_) => return Err(3),
        };
        if full.len() > 3 {
            let a = host.get_attr(&full)?;
            if a & 0x10 == 0 {
                return Err(3);
            }
        }
        let d = (full.as_bytes()[0] - b'A') as usize;
        self.dos.cwd[d] = full[3..].to_string();
        self.write_cds(host, d as u8);
        Ok(())
    }

    /// Find a program: as given, or with .COM, .EXE, .BAT, in the
    /// current directory and then along PATH.
    fn find_program<H: Host>(&mut self, host: &mut H, word: &str) -> Option<String> {
        let w = upper(word);
        let has_ext = w.rsplit(['\\', ':']).next().is_some_and(|b| b.contains('.'));
        let exts: &[&str] = if has_ext { &[""] } else { &[".COM", ".EXE", ".BAT"] };
        let try_dir = |m: &mut Machine, host: &mut H, base: &str| -> Option<String> {
            for e in exts {
                let cand = format!("{base}{e}");
                if let Ok(Name::File(f)) = m.resolve(cand.as_bytes()) {
                    if host.get_attr(&f).is_ok_and(|a| a & 0x18 == 0) {
                        return Some(f);
                    }
                }
            }
            None
        };
        if let Some(f) = try_dir(self, host, &w) {
            return Some(f);
        }
        if w.contains(['\\', ':']) {
            return None;
        }
        let path = self.getenv("PATH").unwrap_or_default();
        for dir in path.split(';').filter(|d| !d.is_empty()) {
            let base = format!("{}\\{}", dir.trim_end_matches('\\'), w);
            if let Some(f) = try_dir(self, host, &base) {
                return Some(f);
            }
        }
        None
    }

    fn external<H: Host>(&mut self, host: &mut H, i: usize, word: &str, rest: &str) -> bool {
        let Some(path) = self.find_program(host, word) else {
            self.outs(host, "Bad command or file name\r\n");
            self.shells[i].errorlevel = 1;
            return false;
        };
        if path.ends_with(".BAT") {
            // A batch file replaces the running one (CALL nests).
            self.shells[i].batches.pop();
            self.start_batch(host, i, &path, rest, false);
            return false;
        }
        // The EXEC parameter block and the command tail.
        let psp = self.shells[i].psp;
        let tail = rest.trim_end();
        let tail = if tail.is_empty() || tail.starts_with([' ', '/']) { tail.to_string() } else { format!(" {tail}") };
        let tb: Vec<u8> = tail.bytes().take(126).collect();
        let ta = Machine::lin(psp, EXEC_TAIL);
        self.hw.wr(ta, tb.len() as u8);
        self.hw.write_bytes(ta + 1, &tb);
        self.hw.wr(ta + 1 + tb.len() as u32, 0x0d);
        let (f1, f2) = self.parse_fcbs(&tb);
        self.hw.write_bytes(Machine::lin(psp, EXEC_FCB1), &f1);
        self.hw.write_bytes(Machine::lin(psp, EXEC_FCB2), &f2);
        let pa = Machine::lin(psp, EXEC_PATH);
        self.hw.write_bytes(pa, path.as_bytes());
        self.hw.wr(pa + path.len() as u32, 0);
        let b = Machine::lin(psp, EXEC_BLOCK);
        self.hw.wr16(b, 0);
        for (k, (off, seg)) in [(EXEC_TAIL, psp), (EXEC_FCB1, psp), (EXEC_FCB2, psp)].iter().enumerate() {
            self.hw.wr16(b + 2 + k as u32 * 4, *off);
            self.hw.wr16(b + 4 + k as u32 * 4, *seg);
        }
        self.cpu.set_ax(0x4b00);
        self.cpu.seg[DS] = Seg::real(psp);
        self.cpu.set_dx(EXEC_PATH);
        self.cpu.seg[ES] = Seg::real(psp);
        self.cpu.set_bx(EXEC_BLOCK);
        self.shells[i].pending = Pending::Exec;
        true
    }

    fn start_batch<H: Host>(&mut self, host: &mut H, i: usize, path: &str, rest: &str, nested: bool) {
        let data = match self.read_file(host, path) {
            Ok(d) => d,
            Err(_) => {
                self.outs(host, "Batch file missing\r\n");
                return;
            }
        };
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for l in data.split(|&c| c == b'\n') {
            let mut l = l.to_vec();
            while l.last() == Some(&b'\r') {
                l.pop();
            }
            if let Some(z) = l.iter().position(|&c| c == 0x1a) {
                l.truncate(z);
                lines.push(l);
                break;
            }
            lines.push(l);
        }
        let mut args = alloc::vec![path.rsplit('\\').next().unwrap_or(path).to_string()];
        args.extend(rest.split([' ', '\t', ',', ';']).filter(|a| !a.is_empty()).map(String::from));
        let _ = nested;
        self.shells[i].batches.push(Batch { lines, pos: 0, args, shift: 0 });
    }

    fn if_cmd<H: Host>(&mut self, host: &mut H, i: usize, args: &str) -> bool {
        let mut a = args.trim_start();
        let mut not = false;
        if upper(a).starts_with("NOT ") {
            not = true;
            a = a[4..].trim_start();
        }
        let u = upper(a);
        let (cond, rest) = if u.starts_with("ERRORLEVEL ") {
            let r = a[11..].trim_start();
            let n: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
            let lvl: u32 = n.parse().unwrap_or(0);
            (self.shells[i].errorlevel as u32 >= lvl, r[n.len()..].to_string())
        } else if u.starts_with("EXIST ") {
            let r = a[6..].trim_start();
            let end = r.find(' ').unwrap_or(r.len());
            let f = &r[..end];
            (self.exists(host, f), r[end..].to_string())
        } else if let Some(p) = a.find("==") {
            let left = a[..p].trim();
            let r = a[p + 2..].trim_start();
            let end = r.find(' ').unwrap_or(r.len());
            (left == &r[..end], r[end..].to_string())
        } else {
            self.outs(host, "Syntax error\r\n");
            return false;
        };
        if cond != not {
            return self.command(host, i, rest.trim());
        }
        false
    }

    fn exists<H: Host>(&mut self, host: &mut H, pat: &str) -> bool {
        match self.resolve(pat.as_bytes()) {
            Ok(Name::Dev(_)) => true,
            Ok(Name::File(f)) => {
                if !f.contains(['*', '?']) {
                    return host.get_attr(&f).is_ok();
                }
                !self.glob(host, &f, 0x06).is_empty()
            }
            Err(_) => false,
        }
    }

    /// Entries matching a full path with wildcards: (directory, entries).
    fn glob<H: Host>(&mut self, host: &mut H, full: &str, attr: u8) -> Vec<(String, crate::DirEntry)> {
        let (dir, pat) = match full.rfind('\\') {
            Some(k) => (&full[..k], &full[k + 1..]),
            None => return Vec::new(),
        };
        let d = if dir.len() == 2 { format!("{dir}\\") } else { dir.to_string() };
        let pattern = fcb_form(pat.as_bytes());
        let Ok(list) = host.list_dir(&d) else { return Vec::new() };
        let mut v: Vec<(String, crate::DirEntry)> = list
            .into_iter()
            .filter(|e| crate::dos::file_is_short(&e.name) && e.name != "." && e.name != "..")
            .map(|e| crate::DirEntry { name: upper(&e.name), ..e })
            .filter(|e| matches(&pattern, e.name.as_bytes()) && e.attr & 0x16 & !attr == 0)
            .map(|e| (dir.to_string(), e))
            .collect();
        v.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        v
    }

    fn for_cmd<H: Host>(&mut self, host: &mut H, i: usize, args: &str) {
        // FOR %%V IN (set) DO command
        let a = args.trim();
        let Some(var_end) = a.find(' ') else {
            self.outs(host, "Syntax error\r\n");
            return;
        };
        let var = a[..var_end].trim_start_matches('%').to_string();
        let rest = a[var_end..].trim_start();
        let (Some(open), Some(close)) = (rest.find('('), rest.find(')')) else {
            self.outs(host, "Syntax error\r\n");
            return;
        };
        let set: Vec<String> = rest[open + 1..close].split([' ', ',', ';', '\t']).filter(|s| !s.is_empty()).map(String::from).collect();
        let after = rest[close + 1..].trim_start();
        let Some(cmd) = after.strip_prefix("DO ").or_else(|| after.strip_prefix("do ")).or_else(|| upper(after).starts_with("DO ").then(|| &after[3..])) else {
            self.outs(host, "Syntax error\r\n");
            return;
        };
        let mut items = Vec::new();
        for s in set {
            if s.contains(['*', '?']) {
                if let Ok(Name::File(f)) = self.resolve(s.as_bytes()) {
                    let prefix = match s.rfind(['\\', ':']) {
                        Some(k) => s[..=k].to_string(),
                        None => String::new(),
                    };
                    for (_, e) in self.glob(host, &f, 0) {
                        items.push(format!("{prefix}{}", e.name));
                    }
                }
            } else {
                items.push(s);
            }
        }
        let pat1 = format!("%%{var}");
        let pat2 = format!("%{var}");
        for (k, it) in items.iter().enumerate() {
            let c = cmd.replace(&pat1, it).replace(&pat2, it);
            self.shells[i].queued.insert(k, format!("@{c}"));
        }
    }

    fn vol<H: Host>(&mut self, host: &mut H, d: u8) {
        let l = host.volume_label(d);
        match l {
            Some(l) => self.outs(host, &format!(" Volume in drive {} is {}\r\n", (b'A' + d) as char, l)),
            None => self.outs(host, &format!(" Volume in drive {} has no label\r\n", (b'A' + d) as char)),
        }
    }

    /// The files an argument names: a directory means all of it.
    fn arg_files<H: Host>(&mut self, host: &mut H, arg: &str, attr: u8) -> Option<(String, Vec<(String, crate::DirEntry)>)> {
        let a = if arg.is_empty() { "*.*" } else { arg };
        let mut full = match self.resolve(a.as_bytes()) {
            Ok(Name::File(f)) => f,
            _ => return None,
        };
        if !full.contains(['*', '?']) && (full.len() == 3 || host.get_attr(&full).is_ok_and(|x| x & 0x10 != 0)) {
            full = if full.ends_with('\\') { format!("{full}*.*") } else { format!("{full}\\*.*") };
        }
        let list = self.glob(host, &full, attr);
        let dir = full[..full.rfind('\\').unwrap()].to_string();
        Some((dir, list))
    }

    fn dir<H: Host>(&mut self, host: &mut H, args: &str) {
        let mut wide = false;
        let mut bare = false;
        let mut attr = 0x10u8;
        let mut target = String::new();
        let dircmd = self.getenv("DIRCMD").unwrap_or_default();
        for a in dircmd.split(' ').chain(args.split(' ')).filter(|a| !a.is_empty()) {
            let u = upper(a);
            if let Some(sw) = u.strip_prefix('/') {
                match sw.chars().next() {
                    Some('W') => wide = true,
                    Some('B') => bare = true,
                    Some('A') => attr = 0x16,
                    _ => {}
                }
            } else {
                target = a.to_string();
            }
        }
        let a = if target.is_empty() { "*.*".to_string() } else { target };
        let mut full = match self.resolve(a.as_bytes()) {
            Ok(Name::File(f)) => f,
            _ => {
                self.outs(host, "Invalid drive specification\r\n");
                return;
            }
        };
        if !full.contains(['*', '?']) && (full.len() == 3 || host.get_attr(&full).is_ok_and(|x| x & 0x10 != 0)) {
            full = if full.ends_with('\\') { format!("{full}*.*") } else { format!("{full}\\*.*") };
        } else if !full.contains('.') || full.rsplit('\\').next().is_some_and(|n| !n.contains('.')) {
            full.push_str(".*");
        }
        let k = full.rfind('\\').unwrap();
        let dir = full[..k].to_string();
        let d = full.as_bytes()[0] - b'A';
        let pattern = fcb_form(full[k + 1..].as_bytes());
        let dpath = if dir.len() == 2 { format!("{dir}\\") } else { dir.clone() };
        let list = host.list_dir(&dpath);
        if !bare {
            self.out(host, b"\r\n");
            self.vol(host, d);
            self.outs(host, " Volume Serial Number is 1993-0452\r\n");
            self.outs(host, &format!(" Directory of {dpath}\r\n\r\n"));
        }
        let Ok(list) = list else {
            self.outs(host, "File not found\r\n");
            return;
        };
        let mut v: Vec<crate::DirEntry> = list
            .into_iter()
            .filter(|e| crate::dos::file_is_short(&e.name))
            .filter(|e| !(dpath.len() == 3 && (e.name == "." || e.name == "..")))
            .map(|e| crate::DirEntry { name: upper(&e.name), ..e })
            .filter(|e| matches(&pattern, e.name.as_bytes()) && e.attr & 0x06 & !attr == 0)
            .collect();
        v.sort_by(|a, b| (a.name != "." && a.name != "..", a.name.clone()).cmp(&(b.name != "." && b.name != "..", b.name.clone())));
        let (mut files, mut bytes) = (0u32, 0u64);
        let mut col = 0;
        for e in &v {
            let isdir = e.attr & 0x10 != 0;
            if !isdir {
                files += 1;
                bytes += e.size as u64;
            } else {
                files += 1;
            }
            let (base, ext) = match e.name.find('.') {
                Some(p) if e.name != "." && e.name != ".." => (&e.name[..p], &e.name[p + 1..]),
                _ => (e.name.as_str(), ""),
            };
            if bare {
                self.outs(host, &format!("{}\r\n", e.name));
            } else if wide {
                let n = if isdir { format!("[{}]", e.name) } else { e.name.clone() };
                self.outs(host, &format!("{n:<16}"));
                col += 1;
                if col == 5 {
                    col = 0;
                    self.out(host, b"\r\n");
                }
            } else {
                let size = if isdir { String::from("<DIR>    ") } else { format!("{:>9}", group(e.size as u64)) };
                let (mo, dy, yr) = ((e.date >> 5) & 15, e.date & 31, 1980 + (e.date >> 9) as u32);
                let (h, mi) = ((e.time >> 11) as u32, ((e.time >> 5) & 63) as u32);
                let (h12, ap) = (if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { 'a' } else { 'p' });
                self.outs(host, &format!("{:<8} {:<3} {} {:02}-{:02}-{:02}  {:2}:{:02}{}\r\n", base, ext, size, mo, dy, yr % 100, h12, mi, ap));
            }
        }
        if bare {
            return;
        }
        if wide && col != 0 {
            self.out(host, b"\r\n");
        }
        if v.is_empty() {
            self.outs(host, "File not found\r\n");
            return;
        }
        let free = host.disk_space(d).map(|(spc, bps, free, _)| spc as u64 * bps as u64 * free as u64).unwrap_or(0);
        self.outs(host, &format!("{:>9} file(s) {:>14} bytes\r\n", files, group(bytes)));
        self.outs(host, &format!("{:>39} bytes free\r\n", group(free)));
    }

    fn type_file<H: Host>(&mut self, host: &mut H, args: &str) {
        let name = args.trim();
        if name.is_empty() {
            self.outs(host, "Required parameter missing\r\n");
            return;
        }
        match self.resolve(name.as_bytes()) {
            Ok(Name::File(f)) => match self.read_file(host, &f) {
                Ok(d) => {
                    let end = d.iter().position(|&c| c == 0x1a).unwrap_or(d.len());
                    for chunk in d[..end].chunks(4096) {
                        self.out(host, chunk);
                    }
                }
                Err(_) => self.outs(host, "File not found\r\n"),
            },
            _ => self.outs(host, "File not found\r\n"),
        }
    }

    fn del<H: Host>(&mut self, host: &mut H, args: &str) {
        let a = args.split(' ').find(|a| !a.is_empty() && !a.starts_with('/')).unwrap_or("");
        if a.is_empty() {
            self.outs(host, "Required parameter missing\r\n");
            return;
        }
        let Some((dir, list)) = self.arg_files(host, a, 0) else {
            self.outs(host, "File not found\r\n");
            return;
        };
        if list.is_empty() {
            self.outs(host, "File not found\r\n");
            return;
        }
        for (_, e) in list {
            if e.attr & 0x10 != 0 {
                continue;
            }
            let p = format!("{}\\{}", dir, e.name);
            if host.delete(&p).is_err() {
                self.outs(host, "Access denied\r\n");
            }
        }
    }

    fn ren<H: Host>(&mut self, host: &mut H, args: &str) {
        let parts: Vec<&str> = args.split([' ', ',']).filter(|a| !a.is_empty()).collect();
        if parts.len() != 2 {
            self.outs(host, "Required parameter missing\r\n");
            return;
        }
        let Some((dir, list)) = self.arg_files(host, parts[0], 0) else {
            self.outs(host, "File not found\r\n");
            return;
        };
        if list.is_empty() {
            self.outs(host, "File not found\r\n");
            return;
        }
        let new = fcb_form(upper(parts[1]).as_bytes());
        for (_, e) in list {
            let old = fcb_form(e.name.as_bytes());
            let mut n = [b' '; 11];
            for k in 0..11 {
                n[k] = if new[k] == b'?' { old[k] } else { new[k] };
            }
            let base: String = n[..8].iter().take_while(|&&c| c != b' ').map(|&c| c as char).collect();
            let ext: String = n[8..].iter().take_while(|&&c| c != b' ').map(|&c| c as char).collect();
            let to = if ext.is_empty() { format!("{dir}\\{base}") } else { format!("{dir}\\{base}.{ext}") };
            if host.rename(&format!("{dir}\\{}", e.name), &to).is_err() {
                self.outs(host, "Duplicate file name or file not found\r\n");
            }
        }
    }

    fn copy<H: Host>(&mut self, host: &mut H, args: &str) {
        let parts: Vec<String> = args.split(' ').filter(|a| !a.is_empty() && !a.starts_with('/')).map(String::from).collect();
        if parts.is_empty() {
            self.outs(host, "Required parameter missing\r\n");
            return;
        }
        let srcs: Vec<String> = parts[0].split('+').map(String::from).collect();
        let dest = parts.get(1).cloned().unwrap_or_else(|| String::from("."));
        let concat = srcs.len() > 1;
        let mut files: Vec<(String, String)> = Vec::new();
        for s in &srcs {
            match self.arg_files(host, s, 0) {
                Some((dir, list)) => {
                    for (_, e) in list {
                        if e.attr & 0x10 == 0 {
                            files.push((format!("{dir}\\{}", e.name), e.name));
                        }
                    }
                }
                None => {}
            }
        }
        if files.is_empty() {
            self.outs(host, "File not found\r\n        0 file(s) copied\r\n");
            return;
        }
        let dest_full = match self.resolve(dest.as_bytes()) {
            Ok(Name::File(f)) => f,
            Ok(Name::Dev(_)) => {
                // COPY to a device (CON, NUL): through the handle calls.
                for (p, _) in &files {
                    if let Ok(d) = self.read_file(host, p) {
                        if upper(&dest).starts_with("CON") {
                            self.out(host, &d);
                        }
                    }
                }
                self.outs(host, &format!("{:>9} file(s) copied\r\n", files.len()));
                return;
            }
            Err(_) => {
                self.outs(host, "Invalid path\r\n");
                return;
            }
        };
        let dest_is_dir = dest_full.len() == 3 || host.get_attr(&dest_full).is_ok_and(|a| a & 0x10 != 0);
        let mut n = 0;
        let mut all = Vec::new();
        for (p, name) in &files {
            let Ok(d) = self.read_file(host, p) else { continue };
            if concat {
                let end = d.iter().position(|&c| c == 0x1a).unwrap_or(d.len());
                all.extend_from_slice(&d[..end]);
                continue;
            }
            let target = if dest_is_dir {
                if dest_full.ends_with('\\') { format!("{dest_full}{name}") } else { format!("{dest_full}\\{name}") }
            } else if dest_full.contains(['*', '?']) {
                let pat = fcb_form(dest_full.rsplit('\\').next().unwrap().as_bytes());
                let old = fcb_form(name.as_bytes());
                let mut nn = [b' '; 11];
                for k in 0..11 {
                    nn[k] = if pat[k] == b'?' { old[k] } else { pat[k] };
                }
                let base: String = nn[..8].iter().take_while(|&&c| c != b' ').map(|&c| c as char).collect();
                let ext: String = nn[8..].iter().take_while(|&&c| c != b' ').map(|&c| c as char).collect();
                let dir = &dest_full[..dest_full.rfind('\\').unwrap()];
                if ext.is_empty() { format!("{dir}\\{base}") } else { format!("{dir}\\{base}.{ext}") }
            } else {
                dest_full.clone()
            };
            if &target == p {
                self.outs(host, "File cannot be copied onto itself\r\n");
                continue;
            }
            if files.len() > 1 {
                self.outs(host, &format!("{}\r\n", name));
            }
            if self.write_whole(host, &target, &d).is_ok() {
                n += 1;
            }
        }
        if concat {
            let target = if dest_is_dir { format!("{}\\{}", dest_full.trim_end_matches('\\'), files[0].1) } else { dest_full.clone() };
            all.push(0x1a);
            if self.write_whole(host, &target, &all).is_ok() {
                n = 1;
            }
        }
        self.outs(host, &format!("{:>9} file(s) copied\r\n", n));
    }

    fn write_whole<H: Host>(&mut self, host: &mut H, path: &str, data: &[u8]) -> Result<(), u16> {
        let h = host.create(path, 0x20, Create::Truncate)?;
        let r = host.write(h, data);
        host.close(h);
        r.map(|_| ())
    }

    /// A child of a shell ended (its errorlevel is read from 4Dh).
    pub(crate) fn shell_child_ended<H: Host>(&mut self, host: &mut H, psp: u16) {
        let _ = host;
        if let Some(i) = self.shell_index(psp) {
            // The shell itself ended.
            self.shells.remove(i);
        }
    }

    /// Ctrl+C while a shell reads its line: drop the line and any batch.
    pub(crate) fn shell_break(&mut self) -> bool {
        let psp = self.dos.psp;
        let Some(i) = self.shell_index(psp) else { return false };
        self.shells[i].broken = true;
        let a = Machine::lin(psp, LINE_BUF);
        self.hw.wr(a + 1, 0);
        true
    }

    /// A COMMAND.COM to run when the session has no program: put the
    /// stub in place of the program file's contents (the host supplies
    /// the file; this is for hosts that have none).
    pub fn shell_image() -> Vec<u8> {
        command_com()
    }

    #[allow(dead_code)]
    fn shell_regs_reset(&mut self) {
        let _ = (CS, SS);
    }
}

/// 1234567 -> "1,234,567".
fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
