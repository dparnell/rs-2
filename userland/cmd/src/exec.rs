//! Running commands: redirection, pipes, internal commands and programs.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

use crate::parse::{Pipeline, Redir, Simple, Target};
use crate::shell::Shell;
use crate::util::*;

/// Saved standard handles while a command runs with redirection.
struct Saved(Vec<(u32, u32)>);

impl Saved {
    fn restore(self) {
        for (h, copy) in self.0.into_iter().rev() {
            let mut target = h;
            unsafe {
                DosDupHandle(copy, &mut target);
                DosClose(copy);
            }
        }
    }
}

/// Point `h` at `new` (a handle), remembering the old one.
fn redirect_handle(saved: &mut Saved, h: u32, new: u32) {
    let mut copy = 0xffff_ffff;
    unsafe {
        DosDupHandle(h, &mut copy);
        DosSetFHState(copy, OPEN_FLAGS_NOINHERIT);
    }
    saved.0.push((h, copy));
    let mut target = h;
    unsafe { DosDupHandle(new, &mut target) };
}

fn apply(redirs: &[Redir]) -> Result<Saved, u32> {
    let mut saved = Saved(Vec::new());
    for r in redirs {
        match &r.target {
            Target::Handle(src) => redirect_handle(&mut saved, r.handle, *src),
            Target::File { path, append } => {
                let mut hf = 0;
                let mut act = 0;
                let c = cstr(path);
                let rc = if r.handle == 0 {
                    unsafe {
                        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, core::ptr::null_mut())
                    }
                } else {
                    let flags = if *append {
                        OPEN_ACTION_OPEN_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW
                    } else {
                        OPEN_ACTION_REPLACE_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW
                    };
                    unsafe {
                        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, flags, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut())
                    }
                };
                if rc != 0 {
                    saved.restore();
                    return Err(rc);
                }
                if *append {
                    let mut pos = 0;
                    unsafe { DosSetFilePtr(hf, 0, FILE_END, &mut pos) };
                }
                redirect_handle(&mut saved, r.handle, hf);
                unsafe { DosClose(hf) };
                let _ = OPEN_FLAGS_NOINHERIT;
            }
        }
    }
    Ok(saved)
}

pub fn run_pipeline(sh: &mut Shell, p: &Pipeline) {
    if p.stages.len() == 1 {
        run_simple(sh, &p.stages[0], false);
        return;
    }
    // Connect the stages with pipes; programs run concurrently, internal
    // commands run in order after all programs started.
    let n = p.stages.len();
    let mut pipes: Vec<(u32, u32)> = Vec::new();
    for _ in 0..n - 1 {
        let (mut r, mut w) = (0, 0);
        unsafe {
            DosCreatePipe(&mut r, &mut w, 4096);
            // Only the redirected standard handles may reach the children.
            DosSetFHState(r, OPEN_FLAGS_NOINHERIT);
            DosSetFHState(w, OPEN_FLAGS_NOINHERIT);
        }
        pipes.push((r, w));
    }
    let mut children = Vec::new();
    let mut internal = Vec::new();
    for (i, st) in p.stages.iter().enumerate() {
        let mut st = st.clone();
        if i > 0 {
            st.redirs.insert(0, Redir { handle: 0, target: Target::Handle(pipes[i - 1].0) });
        }
        if i < n - 1 {
            st.redirs.insert(0, Redir { handle: 1, target: Target::Handle(pipes[i].1) });
        }
        if crate::builtin::is_builtin(&st.name) {
            internal.push(st);
        } else if let Some(pid) = run_simple(sh, &st, true) {
            children.push(pid);
        }
    }
    for st in internal {
        run_simple(sh, &st, false);
    }
    for (r, w) in pipes {
        unsafe {
            DosClose(r);
            DosClose(w);
        }
    }
    for pid in children {
        let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
        let mut got = 0;
        unsafe { DosWaitChild(DCWA_PROCESS, DCWW_WAIT, &mut res, &mut got, pid) };
        sh.rc = res.code_result;
    }
}

/// Run one command. With `async_` a program is started without waiting
/// and its PID returned.
pub fn run_simple(sh: &mut Shell, s: &Simple, async_: bool) -> Option<u32> {
    if s.name.is_empty() {
        return None;
    }
    let saved = match apply(&s.redirs) {
        Ok(v) => v,
        Err(rc) => {
            sys_error(rc);
            sh.rc = rc;
            return None;
        }
    };
    let mut pid = None;
    if let Some(rc) = crate::builtin::run(sh, s) {
        sh.rc = rc;
    } else {
        match run_program(sh, &s.name, &s.args, async_) {
            Ok(Some(p)) => pid = Some(p),
            Ok(None) => {}
            Err(rc) => {
                sh.rc = rc;
            }
        }
    }
    saved.restore();
    pid
}

/// Locate a program: an explicit path, else the current directory and
/// the PATH, trying .COM, .EXE and .CMD.
pub fn find_program(sh: &Shell, name: &str) -> Option<String> {
    let exts: &[&str] = if name.contains('.') && !name.ends_with('.') { &[""] } else { &[".COM", ".EXE", ".CMD"] };
    let try_dir = |dir: &str| -> Option<String> {
        for e in exts {
            let cand = if dir.is_empty() { alloc::format!("{name}{e}") } else { alloc::format!("{}\\{name}{e}", dir.trim_end_matches('\\')) };
            if let Some(f) = path_info(&cand) {
                if f.attr & FILE_DIRECTORY == 0 {
                    return Some(full_path(&cand));
                }
            }
        }
        None
    };
    if name.contains(['\\', ':', '/']) {
        return try_dir("");
    }
    if let Some(p) = try_dir("") {
        return Some(p);
    }
    let path = sh.get("PATH").unwrap_or("").to_string();
    for dir in path.split(';').filter(|d| !d.is_empty()) {
        if let Some(p) = try_dir(dir) {
            return Some(p);
        }
    }
    None
}

fn run_program(sh: &mut Shell, name: &str, args: &str, async_: bool) -> Result<Option<u32>, u32> {
    let Some(path) = find_program(sh, name) else {
        err_out(&alloc::format!(
            "SYS1041: The name {} is not recognized as an\r\ninternal or external command, operable program or batch file.\r\n",
            name
        ));
        return Err(1041);
    };
    if path.to_ascii_uppercase().ends_with(".CMD") {
        if !crate::rexx::run(sh, &path, args) {
            crate::batch::start(sh, &path, args);
        }
        return Ok(None);
    }
    // Arguments: "program\0 arguments\0\0". The arguments keep the blank
    // that separated them from the name, as with the OS/2 CMD.EXE; some
    // programs (IBM LABEL) skip the first word otherwise.
    let mut block = Vec::new();
    block.extend_from_slice(name.as_bytes());
    block.push(0);
    // IBM's CMD.EXE starts the argument string with the blank that
    // separated it from the program name; some IBM utilities (FIND, LABEL)
    // rely on that.
    if !args.is_empty() && !args.starts_with([' ', '\t']) {
        block.push(b' ');
    }
    block.extend_from_slice(args.as_bytes());
    block.push(0);
    block.push(0);
    let env = sh.env_block();
    let cpath = cstr(&path);
    let mut fail = [0u8; 260];
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let flags = if async_ { EXEC_ASYNCRESULT } else { EXEC_SYNC };
    let rc = unsafe {
        DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, flags, block.as_ptr(), env.as_ptr(), &mut res, cpath.as_ptr())
    };
    if rc != 0 {
        sys_error(rc);
        return Err(rc);
    }
    if async_ {
        return Ok(Some(res.code_terminate));
    }
    sh.rc = res.code_result;
    if res.code_terminate != 0 {
        outln("");
        outln("The program ended abnormally.");
    }
    Ok(None)
}
