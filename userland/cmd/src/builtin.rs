//! Internal commands.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

use crate::parse::Simple;
use crate::shell::Shell;
use crate::util::*;

const NAMES: &[&str] = &[
    "CD", "CHDIR", "CLS", "COPY", "DATE", "DEL", "ERASE", "DIR", "ECHO", "EXIT", "MD", "MKDIR", "PATH",
    "DPATH", "PROMPT", "RD", "RMDIR", "REN", "RENAME", "SET", "TIME", "TYPE", "VER", "VOL", "REM", "PAUSE",
    "GOTO", "CALL", "IF", "FOR", "SHIFT", "START", "DETACH", "SETLOCAL", "ENDLOCAL", "HELP", "VERIFY",
    "MOVE", "CHCP", "ANSI", "EXTPROC", "KEYS",
];

pub fn is_builtin(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    NAMES.contains(&up.as_str()) || (up.len() == 2 && up.ends_with(':'))
}

/// Run an internal command; None if `s` is not one.
pub fn run(sh: &mut Shell, s: &Simple) -> Option<u32> {
    let up = s.name.to_ascii_uppercase();
    if up.len() == 2 && up.ends_with(':') {
        let d = up.as_bytes()[0];
        if !d.is_ascii_uppercase() {
            return Some(15);
        }
        let rc = unsafe { DosSetDefaultDisk((d - b'A' + 1) as u32) };
        if rc != 0 {
            sys_error(15);
        }
        return Some(rc);
    }
    let rc = match up.as_str() {
        "CD" | "CHDIR" => cd(s),
        "CLS" => {
            out("\x1b[2J");
            0
        }
        "COPY" => copy(s),
        "DATE" => {
            outln(&alloc::format!("The current date is: {}", date_string()));
            0
        }
        "TIME" => {
            outln(&alloc::format!("The current time is: {}", time_string()));
            0
        }
        "DEL" | "ERASE" => del(s),
        "DIR" => crate::dir::dir(s),
        "ECHO" => echo(sh, s),
        "EXIT" => {
            let code = s.words().first().and_then(|w| w.parse().ok()).unwrap_or(sh.rc);
            if sh.batch.is_empty() {
                sh.exit = Some(code);
            } else {
                // EXIT in a batch file ends the batch and the interpreter.
                sh.batch.clear();
                sh.exit = Some(code);
            }
            code
        }
        "MD" | "MKDIR" => dir_op(s, true),
        "RD" | "RMDIR" => dir_op(s, false),
        "PATH" | "DPATH" => path_cmd(sh, &up, s),
        "PROMPT" => {
            sh.set("PROMPT", s.args.trim());
            0
        }
        "REN" | "RENAME" | "MOVE" => rename(s),
        "SET" => set(sh, s),
        "TYPE" => type_cmd(s),
        "VER" => {
            outln("");
            outln("The rs2 Version is 4.52");
            0
        }
        "VOL" => vol(s),
        "REM" => 0,
        "PAUSE" => {
            out("Press Enter to continue . . . ");
            let _ = read_line();
            0
        }
        "VERIFY" => {
            outln("VERIFY is off.");
            0
        }
        "GOTO" => crate::batch::goto(sh, s.args.trim()),
        "CALL" => crate::batch::call(sh, s),
        "IF" => crate::batch::if_cmd(sh, s),
        "FOR" => crate::batch::for_cmd(sh, s),
        "SHIFT" => crate::batch::shift(sh),
        "START" => start_session(sh, s),
        "DETACH" => start(sh, s),
        "SETLOCAL" => {
            sh.locals.push((sh.env.clone(), current_dir(0)));
            0
        }
        "ENDLOCAL" => {
            if let Some((env, dir)) = sh.locals.pop() {
                sh.env = env;
                let c = cstr(&dir);
                unsafe {
                    DosSetDefaultDisk((dir.as_bytes()[0] - b'A' + 1) as u32);
                    DosSetCurrentDir(c.as_ptr());
                }
            }
            0
        }
        "HELP" => {
            help();
            0
        }
        "CHCP" => {
            outln("Active code page: 850");
            0
        }
        "ANSI" => {
            outln("ANSI extended screen and keyboard control is on.");
            0
        }
        "EXTPROC" | "KEYS" => 0,
        _ => return None,
    };
    Some(rc)
}

fn cd(s: &Simple) -> u32 {
    let arg = s.args.trim().trim_matches('"');
    if arg.is_empty() {
        outln(&current_dir(0));
        return 0;
    }
    // "CD X:" shows that drive's directory.
    if arg.len() == 2 && arg.ends_with(':') {
        let d = arg.as_bytes()[0].to_ascii_uppercase();
        outln(&current_dir((d - b'A' + 1) as u32));
        return 0;
    }
    let c = cstr(arg);
    let rc = unsafe { DosSetCurrentDir(c.as_ptr()) };
    if rc != 0 {
        sys_error(if rc == 2 { 3 } else { rc });
    }
    rc
}

fn echo(sh: &mut Shell, s: &Simple) -> u32 {
    let a = s.args.as_str();
    let t = a.trim();
    match t.to_ascii_uppercase().as_str() {
        "" if !a.starts_with('.') => outln(if sh.echo { "ECHO is on." } else { "ECHO is off." }),
        "ON" => sh.echo = true,
        "OFF" => sh.echo = false,
        _ => outln(a.strip_prefix('.').unwrap_or(a)),
    }
    0
}

fn dir_op(s: &Simple, make: bool) -> u32 {
    let mut rc = 0;
    let words = s.words();
    if words.is_empty() {
        sys_error(87);
        return 87;
    }
    for w in words {
        let c = cstr(&w);
        let r = unsafe { if make { DosCreateDir(c.as_ptr(), core::ptr::null_mut()) } else { DosDeleteDir(c.as_ptr()) } };
        if r != 0 {
            sys_error(match (make, r) {
                (true, 5) => 82,
                (false, 5) => 16,
                _ => r,
            });
            rc = r;
        }
    }
    rc
}

fn path_cmd(sh: &mut Shell, var: &str, s: &Simple) -> u32 {
    let a = s.args.trim();
    if a.is_empty() {
        outln(&alloc::format!("{}={}", var, sh.get(var).unwrap_or("")));
    } else if a == ";" {
        sh.set(var, "");
    } else {
        sh.set(var, a);
    }
    0
}

fn set(sh: &mut Shell, s: &Simple) -> u32 {
    let a = s.args.trim();
    if a.is_empty() {
        for (k, v) in &sh.env {
            outln(&alloc::format!("{k}={v}"));
        }
        return 0;
    }
    match a.split_once('=') {
        Some((k, v)) => {
            sh.set(k.trim(), v);
            0
        }
        None => {
            // "SET NAME" shows variables starting with NAME.
            let up = a.to_ascii_uppercase();
            for (k, v) in &sh.env {
                if k.starts_with(&up) {
                    outln(&alloc::format!("{k}={v}"));
                }
            }
            0
        }
    }
}

fn type_cmd(s: &Simple) -> u32 {
    let mut rc = 0;
    for w in s.words() {
        let files: Vec<String> = if has_wildcards(&w) {
            let (dir, _) = split_path(&w);
            match find(&w, 0) {
                Ok(v) => v.into_iter().map(|f| alloc::format!("{dir}{}", f.name)).collect(),
                Err(e) => {
                    sys_error(if e == 18 { 2 } else { e });
                    rc = e;
                    continue;
                }
            }
        } else {
            alloc::vec![w.clone()]
        };
        for f in files {
            if let Err(e) = type_file(&f) {
                sys_error(e);
                rc = e;
            }
        }
    }
    rc
}

fn type_file(path: &str) -> Result<(), u32> {
    let c = cstr(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, core::ptr::null_mut())
    };
    if rc != 0 {
        return Err(rc);
    }
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if n == 0 {
            break;
        }
        // Stop at Ctrl+Z, like the system's TYPE.
        let end = buf[..n as usize].iter().position(|&b| b == 0x1a).unwrap_or(n as usize);
        os2::write(&buf[..end]);
        if end < n as usize {
            break;
        }
    }
    unsafe { DosClose(hf) };
    Ok(())
}

/// Expand a file argument into (source path, name) pairs.
fn expand_files(spec: &str) -> Result<Vec<(String, String)>, u32> {
    if is_dir(spec) && !has_wildcards(spec) {
        let base = alloc::format!("{}\\*", spec.trim_end_matches('\\'));
        let dir = alloc::format!("{}\\", spec.trim_end_matches('\\'));
        return find(&base, 0).map(|v| v.into_iter().map(|f| (alloc::format!("{dir}{}", f.name), f.name)).collect());
    }
    let (dir, _) = split_path(spec);
    find(spec, 0).map(|v| v.into_iter().map(|f| (alloc::format!("{dir}{}", f.name), f.name)).collect())
}

fn copy_file(from: &str, to: &str, append: bool) -> Result<(), u32> {
    let (mut src, mut dst, mut act) = (0, 0, 0);
    let cf = cstr(from);
    let ct = cstr(to);
    unsafe {
        let rc = DosOpen(cf.as_ptr(), &mut src, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut());
        if rc != 0 {
            return Err(rc);
        }
        let flags = if append { OPEN_ACTION_OPEN_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW } else { OPEN_ACTION_REPLACE_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW };
        let rc = DosOpen(ct.as_ptr(), &mut dst, &mut act, 0, 0, flags, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut());
        if rc != 0 {
            DosClose(src);
            return Err(rc);
        }
        if append {
            let mut p = 0;
            DosSetFilePtr(dst, 0, FILE_END, &mut p);
        }
        let mut buf = alloc::vec![0u8; 32768];
        let mut result = Ok(());
        loop {
            let mut n = 0;
            if DosRead(src, buf.as_mut_ptr(), buf.len() as u32, &mut n) != 0 || n == 0 {
                break;
            }
            let mut w = 0;
            let rc = DosWrite(dst, buf.as_ptr(), n, &mut w);
            if rc != 0 || w != n {
                result = Err(if rc != 0 { rc } else { 112 });
                break;
            }
        }
        // Keep the source's time stamps.
        let mut fs = FileStatus3::default();
        if DosQueryFileInfo(src, FIL_STANDARD, &mut fs as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) == 0 {
            let _ = fs;
        }
        DosClose(src);
        DosClose(dst);
        result
    }
}

fn copy(s: &Simple) -> u32 {
    let words: Vec<String> = s.words().into_iter().filter(|w| !w.starts_with('/')).collect();
    if words.is_empty() {
        sys_error(87);
        return 87;
    }
    // "COPY a+b dest" concatenates.
    let src_spec = &words[0];
    let dest = words.get(1).cloned().unwrap_or_else(|| String::from("."));
    let sources: Vec<String> = src_spec.split('+').map(|x| x.to_string()).collect();
    let mut copied = 0;
    if sources.len() > 1 {
        for (i, f) in sources.iter().enumerate() {
            if let Err(e) = copy_file(f, &dest, i > 0) {
                sys_error(e);
                return e;
            }
        }
        outln("        1 file(s) copied.");
        return 0;
    }
    let files = match expand_files(src_spec) {
        Ok(f) => f,
        Err(e) => {
            sys_error(if e == 18 { 2 } else { e });
            return e;
        }
    };
    let dest_is_dir = is_dir(&dest);
    for (path, name) in &files {
        let target = if dest_is_dir { alloc::format!("{}\\{}", dest.trim_end_matches('\\'), name) } else { dest.clone() };
        if full_path(path).eq_ignore_ascii_case(&full_path(&target)) {
            err_out("The file cannot be copied onto itself.\r\n");
            continue;
        }
        if files.len() > 1 {
            outln(path);
        }
        match copy_file(path, &target, false) {
            Ok(()) => copied += 1,
            Err(e) => {
                sys_error(e);
                return e;
            }
        }
    }
    outln(&alloc::format!("{copied:>9} file(s) copied."));
    0
}

fn del(s: &Simple) -> u32 {
    let mut rc = 0;
    for w in s.words().into_iter().filter(|w| !w.starts_with('/')) {
        let spec = if is_dir(&w) { alloc::format!("{}\\*", w.trim_end_matches('\\')) } else { w.clone() };
        if spec.ends_with("\\*") || spec == "*" || spec == "*.*" {
            out("Are you sure (Y/N)? ");
            let ans = read_line().unwrap_or_default();
            if !ans.trim().eq_ignore_ascii_case("y") {
                continue;
            }
        }
        let (dir, _) = split_path(&spec);
        match find(&spec, 0) {
            Ok(files) => {
                for f in files {
                    let c = cstr(&alloc::format!("{dir}{}", f.name));
                    let r = unsafe { DosDelete(c.as_ptr()) };
                    if r != 0 {
                        sys_error(r);
                        rc = r;
                    }
                }
            }
            Err(e) => {
                sys_error(if e == 18 { 2 } else { e });
                rc = e;
            }
        }
    }
    rc
}

fn rename(s: &Simple) -> u32 {
    let w = s.words();
    if w.len() < 2 {
        sys_error(87);
        return 87;
    }
    let (dir, _) = split_path(&w[0]);
    // The new name is relative to the source directory unless it has a
    // path of its own.
    let to = if w[1].contains(['\\', ':']) { w[1].clone() } else { alloc::format!("{dir}{}", w[1]) };
    let (cf, ct) = (cstr(&w[0]), cstr(&to));
    let rc = unsafe { DosMove(cf.as_ptr(), ct.as_ptr()) };
    if rc != 0 {
        sys_error(rc);
    }
    rc
}

fn vol(s: &Simple) -> u32 {
    let w = s.words();
    let drive = w.first().map(|d| (d.as_bytes()[0].to_ascii_uppercase() - b'A' + 1) as u32).unwrap_or(0);
    let d = if drive == 0 { current_drive() } else { drive };
    let mut fi = FsInfo { serial: 0, cch_label: 0, label: [0; 12] };
    let rc = unsafe { DosQueryFSInfo(d, FSIL_VOLSER, &mut fi as *mut _ as *mut u8, core::mem::size_of::<FsInfo>() as u32) };
    if rc != 0 {
        sys_error(rc);
        return rc;
    }
    let letter = (b'A' + d as u8 - 1) as char;
    outln("");
    let label = os2::path::from_bytes(&fi.label[..fi.cch_label as usize]);
    if label.is_empty() {
        outln(&alloc::format!(" The volume in drive {letter} has no label."));
    } else {
        outln(&alloc::format!(" The volume label in drive {letter} is {label}."));
    }
    outln(&alloc::format!(" The Volume Serial Number is {:04X}:{:04X}.", fi.serial >> 16, fi.serial & 0xffff));
    0
}

fn start(sh: &mut Shell, s: &Simple) -> u32 {
    // DETACH: run the program in the background, without a session.
    let words = s.words();
    let Some(prog) = words.iter().find(|w| !w.starts_with('/')) else { return 0 };
    let rest = s.args.split_once(prog.as_str()).map(|(_, r)| r.trim()).unwrap_or("");
    let Some(path) = crate::exec::find_program(sh, prog) else {
        sys_error(2);
        return 2;
    };
    let mut block = Vec::new();
    block.extend_from_slice(prog.as_bytes());
    block.push(0);
    block.extend_from_slice(rest.as_bytes());
    block.extend_from_slice(&[0, 0]);
    let env = sh.env_block();
    let c = cstr(&path);
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { DosExecPgm(core::ptr::null_mut(), 0, EXEC_BACKGROUND, block.as_ptr(), env.as_ptr(), &mut res, c.as_ptr()) };
    if rc != 0 {
        sys_error(rc);
    } else {
        outln(&alloc::format!("The process identification number is {}.", res.code_terminate));
    }
    rc
}

/// START ["title"] [/K|/C|/N] [/F|/B] [/FS|/WIN|/PM|/DOS] [/I] [/MAX|/MIN]
/// [command [arguments]]: run a command in a new session.
fn start_session(sh: &mut Shell, s: &Simple) -> u32 {
    let mut rest = s.args.trim();
    let mut title = String::new();
    if let Some(r) = rest.strip_prefix('"') {
        let (t, after) = r.split_once('"').unwrap_or((r, ""));
        title = t.into();
        rest = after.trim_start();
    }
    let (mut keep, mut direct, mut background) = (false, false, false);
    while let Some(r) = rest.strip_prefix('/') {
        let (sw, after) = r.split_once(' ').unwrap_or((r, ""));
        match sw.to_ascii_uppercase().as_str() {
            "K" => keep = true,
            "C" => keep = false,
            "N" => direct = true,
            "B" => background = true,
            "F" => background = false,
            "FS" | "WIN" | "PM" | "DOS" | "I" | "MAX" | "MIN" | "PGM" => {}
            _ => {
                sys_error(87);
                return 87;
            }
        }
        rest = after.trim_start();
    }
    let (prog, args) = rest.split_once(' ').map(|(p, a)| (p, a.trim())).unwrap_or((rest, ""));
    // A program started directly (/N, or an .EXE without /K); otherwise a
    // command processor runs the command.
    let path = if prog.is_empty() { None } else { crate::exec::find_program(sh, prog) };
    let is_exe = path.as_deref().is_some_and(|p| p.to_ascii_uppercase().ends_with(".EXE"));
    let (name, inputs) = if prog.is_empty() {
        (None, String::new())
    } else if direct || (is_exe && !keep) {
        let Some(p) = path else {
            sys_error(2);
            return 2;
        };
        (Some(p), String::from(args))
    } else {
        (None, alloc::format!("{} {}", if keep { "/K" } else { "/C" }, rest))
    };
    if title.is_empty() {
        title = if prog.is_empty() { String::from("OS/2 Window") } else { String::from(prog) };
    }
    let ctitle = cstr(&title);
    let cname = name.as_deref().map(cstr);
    let cinputs = cstr(&inputs);
    let env = sh.env_block();
    let sd = StartData {
        length: core::mem::size_of::<StartData>() as u16,
        related: 0,
        fgbg: background as u16,
        trace: 0,
        title: ctitle.as_ptr(),
        name: cname.as_ref().map_or(core::ptr::null(), |n| n.as_ptr()),
        inputs: cinputs.as_ptr(),
        termq: core::ptr::null(),
        env: env.as_ptr(),
        inherit: 0,
        session_type: 0,
    };
    let (mut sid, mut pid) = (0, 0);
    let rc = unsafe { DosStartSession(&sd, &mut sid, &mut pid) };
    if rc != 0 && rc != 457 {
        sys_error(rc);
        return rc;
    }
    0
}

fn help() {
    outln("rs2 command interpreter. Internal commands:");
    outln("  CD CHDIR CLS COPY DATE DEL ERASE DIR ECHO EXIT MD MKDIR PATH DPATH");
    outln("  PROMPT RD RMDIR REN RENAME MOVE SET TIME TYPE VER VOL PAUSE START");
    outln("  Batch files (.CMD): REM GOTO CALL IF FOR SHIFT SETLOCAL ENDLOCAL");
    outln("  Operators: < > >> 2> 2>&1 | & && ||");
}

pub fn date_string() -> String {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    alloc::format!("{} {:02}-{:02}-{:04}", DAYS[(dt.weekday % 7) as usize], dt.month, dt.day, dt.year)
}

pub fn time_string() -> String {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    alloc::format!("{:2}:{:02}:{:02}.{:02}", dt.hours, dt.minutes, dt.seconds, dt.hundredths)
}
