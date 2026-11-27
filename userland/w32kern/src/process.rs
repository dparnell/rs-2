//! The command line, the environment, processes and modules.
//!
//! Module handles: a PE module's is its image base, as on Windows; an rs2
//! DLL's (W32KERN for KERNEL32...) is its OS/2 module handle, a small
//! number GetProcAddress also takes.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::handles::{self, Obj, CURRENT_PROCESS};
use crate::sync::{Kind, Waitable, STILL_ACTIVE};
use crate::text::{from_wide, to_wide};
use crate::{cstr, err, fail, put_str, put_wstr, set_error, win_error, wstr, z, Global, FALSE, TRUE};

struct Proc {
    /// GetCommandLineA's string (NUL-terminated) and its UTF-16 copy.
    cmdline: Vec<u8>,
    cmdline_w: Vec<u16>,
    /// VAR=VALUE strings.
    env: Vec<Vec<u8>>,
    /// The ANSI and UTF-16 environment blocks handed out last.
    env_block: Vec<u8>,
    env_block_w: Vec<u16>,
    pid: u32,
}

static P: Global<Proc> = Global::new(Proc { cmdline: Vec::new(), cmdline_w: Vec::new(), env: Vec::new(), env_block: Vec::new(), env_block_w: Vec::new(), pid: 0 });

/// Quote a program name for a command line if it needs it.
fn quoted(p: &[u8]) -> Vec<u8> {
    if p.contains(&b' ') && !p.starts_with(b"\"") {
        let mut v = alloc::vec![b'"'];
        v.extend_from_slice(p);
        v.push(b'"');
        v
    } else {
        p.to_vec()
    }
}

pub fn init() {
    let (mut tib, mut pib) = (core::ptr::null_mut(), core::ptr::null_mut());
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    let pib = unsafe { &*pib };
    // OS/2's command line: the program as started, NUL, the arguments.
    let prog = unsafe { cstr(pib.pchcmd) };
    let args = unsafe { cstr(pib.pchcmd.add(prog.len() + 1)) };
    let mut cl = quoted(prog);
    if !args.is_empty() {
        if !args.starts_with(b" ") && !args.starts_with(b"\t") {
            cl.push(b' ');
        }
        cl.extend_from_slice(args);
    }
    let mut env = Vec::new();
    let mut p = pib.pchenv;
    loop {
        let s = unsafe { cstr(p) };
        if s.is_empty() {
            break;
        }
        env.push(s.to_vec());
        p = unsafe { p.add(s.len() + 1) };
    }
    P.with(|x| {
        x.cmdline_w = to_wide(&cl);
        x.cmdline_w.push(0);
        cl.push(0);
        x.cmdline = cl;
        x.env = env;
        x.pid = pib.pid;
    });
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCommandLineA() -> *const u8 {
    P.with(|x| x.cmdline.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCommandLineW() -> *const u16 {
    P.with(|x| x.cmdline_w.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCurrentProcessId() -> u32 {
    P.with(|x| x.pid)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCurrentProcess() -> u32 {
    CURRENT_PROCESS
}

// ---- The environment ----

fn key_of(s: &[u8]) -> &[u8] {
    // "=C:=C:\DIR" variables keep their leading '='.
    let start = if s.starts_with(b"=") { 1 } else { 0 };
    match s[start..].iter().position(|&c| c == b'=') {
        Some(i) => &s[..start + i],
        None => s,
    }
}

pub fn env_get(name: &[u8]) -> Option<Vec<u8>> {
    P.with(|x| x.env.iter().find(|e| key_of(e).eq_ignore_ascii_case(name)).map(|e| e[key_of(e).len() + 1..].to_vec()))
}

fn env_set(name: &[u8], value: Option<&[u8]>) {
    P.with(|x| {
        x.env.retain(|e| !key_of(e).eq_ignore_ascii_case(name));
        if let Some(v) = value {
            let mut s = name.to_vec();
            s.push(b'=');
            s.extend_from_slice(v);
            x.env.push(s);
        }
    });
}

/// The environment as an OS/2 or Windows block: strings, then a NUL.
pub fn env_block() -> Vec<u8> {
    P.with(|x| {
        let mut b = Vec::new();
        for e in &x.env {
            b.extend_from_slice(e);
            b.push(0);
        }
        if x.env.is_empty() {
            b.push(0);
        }
        b.push(0);
        b
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetEnvironmentVariableA(name: *const u8, buf: *mut u8, size: u32) -> u32 {
    match env_get(unsafe { cstr(name) }) {
        Some(v) => {
            set_error(0);
            unsafe { put_str(&v, buf, size) }
        }
        None => fail(err::ENVVAR_NOT_FOUND, 0),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetEnvironmentVariableW(name: *const u16, buf: *mut u16, size: u32) -> u32 {
    match env_get(&from_wide(unsafe { wstr(name) })) {
        Some(v) => unsafe { put_wstr(&to_wide(&v), buf, size) },
        None => fail(err::ENVVAR_NOT_FOUND, 0),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetEnvironmentVariableA(name: *const u8, value: *const u8) -> i32 {
    let n = unsafe { cstr(name) };
    if n.is_empty() {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    env_set(n, if value.is_null() { None } else { Some(unsafe { cstr(value) }) });
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetEnvironmentVariableW(name: *const u16, value: *const u16) -> i32 {
    let n = from_wide(unsafe { wstr(name) });
    let v = if value.is_null() { None } else { Some(from_wide(unsafe { wstr(value) })) };
    env_set(&n, v.as_deref());
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetEnvironmentStringsA() -> *const u8 {
    let b = env_block();
    P.with(|x| {
        x.env_block = b;
        x.env_block.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetEnvironmentStrings() -> *const u8 {
    GetEnvironmentStringsA()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetEnvironmentStringsW() -> *const u16 {
    let b = to_wide(&env_block());
    P.with(|x| {
        x.env_block_w = b;
        x.env_block_w.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FreeEnvironmentStringsA(_p: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FreeEnvironmentStringsW(_p: u32) -> i32 {
    TRUE
}

fn expand(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            if let Some(j) = s[i + 1..].iter().position(|&c| c == b'%') {
                let name = &s[i + 1..i + 1 + j];
                if let Some(v) = (!name.is_empty()).then(|| env_get(name)).flatten() {
                    out.extend_from_slice(&v);
                    i += j + 2;
                    continue;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ExpandEnvironmentStringsA(src: *const u8, dst: *mut u8, size: u32) -> u32 {
    let e = expand(unsafe { cstr(src) });
    if dst.is_null() || (size as usize) < e.len() + 1 {
        return e.len() as u32 + 1;
    }
    unsafe { put_str(&e, dst, size) + 1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ExpandEnvironmentStringsW(src: *const u16, dst: *mut u16, size: u32) -> u32 {
    let e = to_wide(&expand(&from_wide(unsafe { wstr(src) })));
    if dst.is_null() || (size as usize) < e.len() + 1 {
        return e.len() as u32 + 1;
    }
    unsafe { put_wstr(&e, dst, size) + 1 }
}

// ---- Processes ----

static SELF: Global<Option<Arc<Waitable>>> = Global::new(None);

/// GetCurrentProcess()'s object (never signalled while we run).
pub fn current_process_object() -> Arc<Waitable> {
    SELF.with(|s| s.get_or_insert_with(|| Waitable::new(Kind::Process { pid: 0, done: false, code: STILL_ACTIVE })).clone())
}

/// The exit code of child `pid` if it has ended (collected once: the
/// object keeps it).
pub fn child_ended(pid: u32) -> Option<u32> {
    if pid == 0 {
        return None;
    }
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let mut got = 0u32;
    let rc = unsafe { os2::DosWaitChild(os2::DCWA_PROCESS, os2::DCWW_NOWAIT, &mut res, &mut got, pid) };
    match rc {
        0 => Some(if res.code_terminate == 0 { res.code_result } else { 0xC000_0005 }),
        // ERROR_CHILD_NOT_COMPLETE
        129 => None,
        // Collected already, or not ours: ended.
        _ => Some(0),
    }
}

/// Directories programs and DLLs are looked for in: the program's, the
/// current one, the Windows directories, the PATH.
pub fn search_dirs() -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut exe = [0u8; 260];
    let n = unsafe { GetModuleFileNameA(0, exe.as_mut_ptr(), 260) } as usize;
    if let Some(i) = exe[..n].iter().rposition(|&c| c == b'\\') {
        v.push(exe[..i].to_vec());
    }
    v.push(crate::file::current_dir(crate::file::current_drive()));
    v.push(crate::misc::windows_dir());
    if let Some(p) = env_get(b"PATH") {
        for d in p.split(|&c| c == b';').filter(|d| !d.is_empty()) {
            v.push(d.to_vec());
        }
    }
    v
}

fn exists(p: &[u8]) -> bool {
    (unsafe { crate::file::GetFileAttributesA(z(p).as_ptr()) }) != crate::file::INVALID_FILE_ATTRIBUTES
}

/// Find a program: with ".EXE" if it has no extension, in the search
/// directories unless it has a path.
fn find_program(name: &[u8]) -> Option<Vec<u8>> {
    let base = name.rsplit(|&c| c == b'\\' || c == b'/').next().unwrap_or(name);
    let mut cands = alloc::vec![name.to_vec()];
    if !base.contains(&b'.') {
        let mut n = name.to_vec();
        n.extend_from_slice(b".EXE");
        cands.insert(0, n);
    }
    if name.contains(&b'\\') || name.contains(&b'/') || name.contains(&b':') {
        return cands.into_iter().find(|c| exists(c)).map(|c| crate::file::full_path(&c));
    }
    for d in search_dirs() {
        for c in &cands {
            let mut p = d.clone();
            if !p.ends_with(b"\\") {
                p.push(b'\\');
            }
            p.extend_from_slice(c);
            if exists(&p) {
                return Some(crate::file::full_path(&p));
            }
        }
    }
    None
}

/// Split a command line into its program and the rest.
fn split_cmdline(cl: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let s = cl.iter().position(|&c| c != b' ' && c != b'\t').unwrap_or(cl.len());
    let cl = &cl[s..];
    if cl.starts_with(b"\"") {
        let end = cl[1..].iter().position(|&c| c == b'"').map_or(cl.len(), |i| i + 1);
        let prog = cl[1..end].to_vec();
        let rest = cl.get(end + 1..).unwrap_or(&[]).to_vec();
        (prog, rest)
    } else {
        let end = cl.iter().position(|&c| c == b' ' || c == b'\t').unwrap_or(cl.len());
        (cl[..end].to_vec(), cl[end..].to_vec())
    }
}

/// PROCESS_INFORMATION.
#[repr(C)]
pub struct ProcessInformation {
    pub process: u32,
    pub thread: u32,
    pub pid: u32,
    pub tid: u32,
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateProcessA(app: *const u8, cmdline: *const u8, _psa: u32, _tsa: u32, _inherit: i32, _flags: u32, env: *const u8, cwd: *const u8, si: *const u32, pi: *mut ProcessInformation) -> i32 {
    let cl = unsafe { cstr(cmdline) }.to_vec();
    let (prog, rest) = split_cmdline(&cl);
    let (path, rest) = if !app.is_null() {
        let a = unsafe { cstr(app) }.to_vec();
        // With an application name, the whole command line is the
        // arguments after argv[0].
        (find_program(&a), if cmdline.is_null() { Vec::new() } else { rest })
    } else {
        (find_program(&prog), rest)
    };
    let Some(path) = path else { return fail(err::FILE_NOT_FOUND, FALSE) };
    // OS/2's arguments: the program as typed, NUL, the rest, NUL NUL.
    let shown = if app.is_null() { prog.clone() } else { unsafe { cstr(app) }.to_vec() };
    let mut args = quoted(&shown);
    args.push(0);
    args.extend_from_slice(&rest);
    args.push(0);
    args.push(0);
    let env_b = if env.is_null() {
        env_block()
    } else {
        // A Windows block is an OS/2 block.
        let mut n = 0;
        while unsafe { *env.add(n) } != 0 || unsafe { *env.add(n + 1) } != 0 {
            n += 1;
        }
        unsafe { core::slice::from_raw_parts(env, n + 2) }.to_vec()
    };
    // STARTF_USESTDHANDLES: the child's standard handles, through the
    // parent's 0, 1 and 2 for the moment of the start.
    let mut saved: [Option<u32>; 3] = [None; 3];
    if !si.is_null() && unsafe { *si.add(11) } & 0x100 != 0 {
        for i in 0..3 {
            let h = unsafe { *si.add(14 + i) };
            if let Some(hf) = handles::file(h) {
                if hf != i as u32 {
                    let mut keep = 0xffff_ffffu32;
                    unsafe { os2::DosDupHandle(i as u32, &mut keep) };
                    let mut target = i as u32;
                    unsafe { os2::DosDupHandle(hf, &mut target) };
                    saved[i] = Some(keep);
                }
            }
        }
    }
    // The child starts in the directory asked for (OS/2 children inherit
    // the parent's): the parent goes there for the moment.
    let old_dir = (!cwd.is_null()).then(|| crate::file::current_dir(crate::file::current_drive()));
    if !cwd.is_null() {
        unsafe { crate::file::SetCurrentDirectoryA(cwd) };
    }
    let p = z(&path);
    let mut fail_name = [0u8; 260];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { os2::DosExecPgm(fail_name.as_mut_ptr(), 260, os2::EXEC_ASYNCRESULT, args.as_ptr(), env_b.as_ptr(), &mut res, p.as_ptr()) };
    if let Some(d) = old_dir {
        unsafe { crate::file::SetCurrentDirectoryA(z(&d).as_ptr()) };
    }
    for (i, s) in saved.iter().enumerate() {
        if let Some(keep) = *s {
            let mut target = i as u32;
            unsafe {
                os2::DosDupHandle(keep, &mut target);
                os2::DosClose(keep);
            }
        }
    }
    if rc != 0 {
        return fail(win_error(rc), FALSE);
    }
    let pid = res.code_terminate;
    let obj = Waitable::new(Kind::Process { pid, done: false, code: STILL_ACTIVE });
    if !pi.is_null() {
        unsafe {
            (*pi).process = handles::add(Obj::Wait(obj.clone()));
            (*pi).thread = handles::add(Obj::Wait(obj));
            (*pi).pid = pid;
            (*pi).tid = 1;
        }
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateProcessW(app: *const u16, cmdline: *const u16, psa: u32, tsa: u32, inherit: i32, flags: u32, env: *const u8, cwd: *const u16, si: *const u32, pi: *mut ProcessInformation) -> i32 {
    let a = (!app.is_null()).then(|| z(&from_wide(unsafe { wstr(app) })));
    let c = (!cmdline.is_null()).then(|| z(&from_wide(unsafe { wstr(cmdline) })));
    let d = (!cwd.is_null()).then(|| z(&from_wide(unsafe { wstr(cwd) })));
    // CREATE_UNICODE_ENVIRONMENT (0x400): convert the block.
    let e = (!env.is_null() && flags & 0x400 != 0).then(|| {
        let w = env as *const u16;
        let mut n = 0;
        while unsafe { *w.add(n) } != 0 || unsafe { *w.add(n + 1) } != 0 {
            n += 1;
        }
        from_wide(unsafe { core::slice::from_raw_parts(w, n + 2) })
    });
    let p = |o: &Option<Vec<u8>>| o.as_ref().map_or(core::ptr::null(), |v| v.as_ptr());
    let env_p = match &e {
        Some(v) => v.as_ptr(),
        None => env,
    };
    unsafe { CreateProcessA(p(&a), p(&c), psa, tsa, inherit, flags, env_p, p(&d), si, pi) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetExitCodeProcess(h: u32, code: *mut u32) -> i32 {
    if h == CURRENT_PROCESS {
        unsafe { *code = STILL_ACTIVE };
        return TRUE;
    }
    let Some(o) = handles::waitable(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let st = o.read(|k| match k {
        Kind::Process { pid, done, code } => Some((*pid, *done, *code)),
        _ => None,
    });
    let Some((pid, done, c)) = st else { return fail(err::INVALID_HANDLE, FALSE) };
    let c = if done {
        c
    } else if let Some(x) = child_ended(pid) {
        o.update(|k| {
            if let Kind::Process { done, code, .. } = k {
                *done = true;
                *code = x;
            }
        });
        x
    } else {
        STILL_ACTIVE
    };
    unsafe { *code = c };
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExitProcess(code: u32) -> ! {
    unsafe {
        crate::Rs2W32ProcessDetach();
        os2::DosExit(1, code);
    }
    loop {}
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TerminateProcess(h: u32, code: u32) -> i32 {
    if h == CURRENT_PROCESS {
        unsafe { os2::DosExit(1, code) };
    }
    let Some(o) = handles::waitable(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let pid = o.read(|k| if let Kind::Process { pid, .. } = k { *pid } else { 0 });
    if pid == 0 {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let rc = unsafe { os2::DosKillProcess(1, pid) };
    if rc != 0 { fail(win_error(rc), FALSE) } else { TRUE }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenProcess(_access: u32, _inherit: i32, pid: u32) -> u32 {
    if pid == GetCurrentProcessId() {
        return handles::add(Obj::Wait(current_process_object()));
    }
    handles::add(Obj::Wait(Waitable::new(Kind::Process { pid, done: false, code: STILL_ACTIVE })))
}

/// STARTUPINFOA: nothing special, the standard handles.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetStartupInfoA(si: *mut u32) {
    unsafe {
        core::ptr::write_bytes(si, 0, 17);
        *si = 68;
        *si.add(14) = handles::GetStdHandle(handles::STD_INPUT_HANDLE);
        *si.add(15) = handles::GetStdHandle(handles::STD_OUTPUT_HANDLE);
        *si.add(16) = handles::GetStdHandle(handles::STD_ERROR_HANDLE);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetStartupInfoW(si: *mut u32) {
    unsafe { GetStartupInfoA(si) };
}

// ---- Modules ----

/// The image base of the program.
pub fn exe_base() -> u32 {
    let peb = crate::teb_get(crate::TEB_PEB);
    unsafe { *((peb + 8) as *const u32) }
}

/// The OS/2 name ("X:\DIR\FILE.DLL") of a module handle.
fn module_path(hmte: u32) -> Option<Vec<u8>> {
    let mut b = [0u8; 260];
    (unsafe { os2::DosQueryModuleName(hmte, 260, b.as_mut_ptr()) } == 0).then(|| unsafe { cstr(b.as_ptr()) }.to_vec())
}

/// "FOO" of "C:\X\FOO.DLL" or "foo.dll", upper case.
fn base_name(p: &[u8]) -> Vec<u8> {
    let b = p.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(p);
    let b = b.split(|&c| c == b'.').next().unwrap_or(b);
    b.to_ascii_uppercase()
}

/// The rs2 DLL a Windows system DLL is (as the kernel's PE loader maps
/// imports).
fn win32_module(name: &[u8]) -> Option<&'static [u8]> {
    match &base_name(name)[..] {
        b"KERNEL32" => Some(b"W32KERN"),
        b"NTDLL" => Some(b"W32NTDLL"),
        b"USER32" => Some(b"W32USER"),
        _ => None,
    }
}

/// The PE modules of the process: (image base, OS/2 module handle).
fn pe_modules() -> Vec<(u32, u32)> {
    let mut buf = alloc::vec![0u32; 256];
    let n = unsafe { crate::Rs2W32Modules(buf.as_mut_ptr(), 128) } as usize;
    buf[..n.min(128) * 2].chunks(2).map(|c| (c[0], c[1])).collect()
}

fn hmte_of(h: u32) -> u32 {
    if h == 0 {
        unsafe { crate::Rs2W32HmteOfBase(exe_base()) }
    } else if h < 0x10000 {
        h
    } else {
        unsafe { crate::Rs2W32HmteOfBase(h) }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetModuleHandleA(name: *const u8) -> u32 {
    if name.is_null() {
        return exe_base();
    }
    let n = unsafe { cstr(name) };
    if let Some(m) = win32_module(n) {
        let mut h = 0;
        let mz = z(m);
        if unsafe { os2::DosQueryModuleHandle(mz.as_ptr(), &mut h) } == 0 {
            return h;
        }
        return fail(err::MOD_NOT_FOUND, 0);
    }
    let want = base_name(n);
    for (base, hmte) in pe_modules() {
        if module_path(hmte).is_some_and(|p| base_name(&p) == want) {
            return base;
        }
    }
    let mut h = 0;
    let wz = z(&want);
    if unsafe { os2::DosQueryModuleHandle(wz.as_ptr(), &mut h) } == 0 {
        return unsafe { crate::Rs2W32BaseOfHmte(h) }.max(h);
    }
    fail(err::MOD_NOT_FOUND, 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetModuleHandleW(name: *const u16) -> u32 {
    if name.is_null() {
        return exe_base();
    }
    let n = z(&from_wide(unsafe { wstr(name) }));
    unsafe { GetModuleHandleA(n.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetModuleFileNameA(h: u32, buf: *mut u8, size: u32) -> u32 {
    let Some(p) = module_path(hmte_of(h)) else { return fail(err::MOD_NOT_FOUND, 0) };
    if size == 0 || buf.is_null() {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    // Truncated to fit, as Windows does.
    let n = p.len().min(size as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(p.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    if n < p.len() {
        set_error(err::INSUFFICIENT_BUFFER);
    }
    n as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetModuleFileNameW(h: u32, buf: *mut u16, size: u32) -> u32 {
    let Some(p) = module_path(hmte_of(h)) else { return fail(err::MOD_NOT_FOUND, 0) };
    if size == 0 || buf.is_null() {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    let w = to_wide(&p);
    let n = w.len().min(size as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(w.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as u32
}

/// Load a DLL: an rs2 DLL for the system DLLs, else the file found in
/// the search directories (with ".DLL" if the name has no extension).
pub fn load_library(name: &[u8]) -> u32 {
    if let Some(m) = win32_module(name) {
        let mut h = 0;
        let mz = z(m);
        let mut f = [0u8; 64];
        if unsafe { os2::DosLoadModule(f.as_mut_ptr(), 64, mz.as_ptr(), &mut h) } != 0 {
            return fail(err::MOD_NOT_FOUND, 0);
        }
        return h;
    }
    let base = name.rsplit(|&c| c == b'\\' || c == b'/').next().unwrap_or(name);
    let mut file = name.to_vec();
    if !base.contains(&b'.') {
        file.extend_from_slice(b".DLL");
    }
    // A trailing '.' means "no extension".
    if file.ends_with(b".") {
        file.pop();
    }
    let found = if file.contains(&b'\\') || file.contains(&b'/') || file.contains(&b':') {
        exists(&file).then(|| crate::file::full_path(&file))
    } else {
        search_dirs().into_iter().find_map(|d| {
            let mut p = d;
            if !p.ends_with(b"\\") {
                p.push(b'\\');
            }
            p.extend_from_slice(&file);
            exists(&p).then(|| crate::file::full_path(&p))
        })
    };
    // Not found as a file: an OS/2 DLL along the LIBPATH by its name.
    let what = found.unwrap_or_else(|| base_name(&file));
    let mut h = 0;
    let wz = z(&what);
    let mut f = [0u8; 260];
    let rc = unsafe { os2::DosLoadModule(f.as_mut_ptr(), 260, wz.as_ptr(), &mut h) };
    if rc != 0 {
        return fail(if rc == 2 || rc == 3 || rc == 126 { err::MOD_NOT_FOUND } else { win_error(rc) }, 0);
    }
    let pe = unsafe { crate::Rs2W32BaseOfHmte(h) };
    if pe == 0 {
        return h;
    }
    if unsafe { crate::Rs2W32InitModules() } != 0 {
        // A DllMain refused: ERROR_DLL_INIT_FAILED.
        return fail(1114, 0);
    }
    pe
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LoadLibraryA(name: *const u8) -> u32 {
    load_library(unsafe { cstr(name) })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LoadLibraryW(name: *const u16) -> u32 {
    load_library(&from_wide(unsafe { wstr(name) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LoadLibraryExA(name: *const u8, _file: u32, _flags: u32) -> u32 {
    load_library(unsafe { cstr(name) })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LoadLibraryExW(name: *const u16, _file: u32, _flags: u32) -> u32 {
    load_library(&from_wide(unsafe { wstr(name) }))
}

/// FreeLibrary: modules stay loaded (their DLL_PROCESS_DETACH comes at
/// the process's end).
#[unsafe(no_mangle)]
pub extern "stdcall" fn FreeLibrary(h: u32) -> i32 {
    if hmte_of(h) == 0 {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FreeLibraryAndExitThread(_h: u32, code: u32) -> ! {
    crate::sync::ExitThread(code)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetProcAddress(h: u32, name: *const u8) -> u32 {
    let hmte = hmte_of(h);
    if hmte == 0 {
        return fail(err::INVALID_HANDLE, 0);
    }
    let mut pfn = 0usize;
    let rc = if (name as u32) < 0x10000 {
        unsafe { os2::DosQueryProcAddr(hmte, name as u32, core::ptr::null(), &mut pfn) }
    } else {
        unsafe { os2::DosQueryProcAddr(hmte, 0, name, &mut pfn) }
    };
    if rc != 0 || pfn == 0 {
        return fail(err::PROC_NOT_FOUND, 0);
    }
    pfn as u32
}
