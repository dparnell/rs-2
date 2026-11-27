//! Calls real programs make that do not belong to a bigger part of
//! WIN16: hmemcpy, programs started, hooks (W32USER calls them; the
//! system-wide ones reach other tasks here), and small KERNEL and SHELL
//! queries.

use alloc::vec::Vec;
use thunk16::{Pascal, PascalCall};

use crate::Global;

/// Installed hooks: (handle, type, procedure). Each is W32USER's too
/// (`w32`), which calls it for the task's own messages (WH_GETMESSAGE,
/// WH_KEYBOARD, WH_CALLWNDPROC, WH_MSGFILTER); the chain is passed on
/// here (CallNextHookEx), and system-wide hooks see other tasks'
/// messages through `remote`.
pub struct Hook {
    pub h: u32,
    pub kind: i16,
    pub proc_: u32,
    /// W32USER's hook.
    pub w32: u32,
    /// System-wide (SetWindowsHookEx with no task): other tasks' messages
    /// come here too (`forward`).
    pub global: bool,
}

pub static HOOKS: Global<Vec<Hook>> = Global::new(Vec::new());
static NEXT_HOOK: Global<u32> = Global::new(0x4b48_0001);

/// The hook after `h` (older) in its chain.
fn next_hook(h: u32) -> Option<u32> {
    let hs = HOOKS.get();
    let i = hs.iter().position(|x| x.h == h)?;
    let kind = hs[i].kind;
    hs[..i].iter().rev().find(|x| x.kind == kind).map(|x| x.proc_)
}


/// WinExec(cmdline, show), ShellExecute's programs: start the program
/// (a Windows or OS/2 program, as a process of its own). Windows finds a
/// program without a path in the current directory, the Windows
/// directories, the caller's directory and the PATH. 33 for a started
/// program, else Windows' error (2: file not found).
pub fn exec(cmd: &[u8]) -> u16 {
    let cmd = trim(cmd);
    let (prog, args) = match cmd.iter().position(|&c| c == b' ') {
        Some(i) => (&cmd[..i], trim(&cmd[i + 1..])),
        None => (cmd, &b""[..]),
    };
    if prog.is_empty() {
        return 2;
    }
    let with_ext = if prog.iter().rev().take_while(|&&c| c != b'\\').any(|&c| c == b'.') { prog.to_vec() } else { [prog, b".EXE"].concat() };
    let Some(path) = find_program(&with_ext) else {
        // REGEDIT FILE.REG (/s: silently) merges a file into the
        // registration database: done here when there is no REGEDIT.EXE.
        if crate::module::base_name(prog) == b"REGEDIT" {
            let file = args.split(|&c| c == b' ').filter(|a| !a.is_empty() && a[0] != b'/' && a[0] != b'-').last().unwrap_or(b"");
            return if crate::reg::merge(file) { 33 } else { 2 };
        }
        return 2;
    };
    // DosExecPgm's arguments: the program name, NUL, the arguments, NUL NUL.
    let mut a = path.clone();
    a.push(0);
    a.extend_from_slice(args);
    a.extend_from_slice(&[0, 0]);
    let mut z = path;
    z.push(0);
    let mut fail = [0u8; 64];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let mut ty = 0u32;
    let windows = unsafe { os2::DosQueryAppType(z.as_ptr(), &mut ty) } == 0 && ty & 0x0200 != 0;
    // EXEC_ASYNC: it runs on its own. Windows returns once the new task
    // has started (and debuggers have heard of it): its instance handle.
    let rc = unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 1, a.as_ptr(), core::ptr::null(), &mut res, z.as_ptr()) };
    let path = &z[..z.len() - 1];
    if rc != 0 {
        let what = unsafe { crate::cstr(fail.as_ptr()) };
        let what = if what.is_empty() || what.eq_ignore_ascii_case(path) { alloc::string::String::new() } else { alloc::format!(" ({})", alloc::string::String::from_utf8_lossy(what)) };
        report(path, args, &alloc::format!("cannot start it: {}{what}", sys_message(rc, path)));
    }
    match rc {
        0 if windows => match crate::toolhelp::wait_started(res.code_terminate) {
            Some(h) if h > 32 => h,
            _ => {
                if let Some((term, code)) = *crate::toolhelp::ENDED_AT_START.get() {
                    report(path, args, &alloc::format!("it ended before it started (termination {term}, result {code})"));
                }
                33
            }
        },
        0 => 33,
        2 | 3 | 110 => 2,
        8 => 8,
        191 | 193 => 11,
        _ => 31,
    }
}

/// OSO001.MSG's text for an error ("SYS0002: The system cannot find the
/// file specified."; %1 the program), or SYSnnnn without the file.
fn sys_message(rc: u32, program: &[u8]) -> alloc::string::String {
    let mut buf = [0u8; 300];
    let mut n = 0;
    let file = b"OSO001.MSG\0";
    let mut p = program.to_vec();
    p.push(0);
    let table = [p.as_ptr()];
    if unsafe { os2::DosGetMessage(table.as_ptr(), 1, buf.as_mut_ptr(), buf.len() as u32, rc, file.as_ptr(), &mut n) } == 0 {
        let t = buf[..n as usize].trim_ascii_end();
        return alloc::string::String::from_utf8_lossy(t).replace("\r\n", " ");
    }
    alloc::format!("SYS{rc:04}")
}

/// A program WinExec started that did not run, on the caller's standard
/// error (the command prompt it was started from) and the serial port:
/// Windows says nothing, and setup programs that only start the next
/// one (Borland's SETUP and INSTALL) would end without a word.
fn report(path: &[u8], args: &[u8], why: &str) {
    let me = crate::module::find(0).map(|m| m.name.clone()).unwrap_or_default();
    let line = alloc::format!(
        "WIN16: {} WinExec {}{}{}: {why}",
        alloc::string::String::from_utf8_lossy(&me),
        alloc::string::String::from_utf8_lossy(path),
        if args.is_empty() { "" } else { " " },
        alloc::string::String::from_utf8_lossy(args)
    );
    crate::pm::trace(format_args!("{line}"));
    crate::stderr_line(&line);
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&c| c != b' ').unwrap_or(s.len());
    let b = s.iter().rposition(|&c| c != b' ').map_or(a, |i| i + 1);
    &s[a..b]
}

fn exists(p: &[u8]) -> bool {
    let mut z = p.to_vec();
    z.push(0);
    let mut fs = [0u8; 24];
    unsafe { os2::DosQueryPathInfo(z.as_ptr(), 1, fs.as_mut_ptr(), 24) == 0 && fs[20] & 0x10 == 0 }
}

/// Where Windows finds a program (or another file OpenFile looks for).
pub fn find_program(name: &[u8]) -> Option<Vec<u8>> {
    if name.iter().any(|&c| c == b'\\' || c == b':') {
        return exists(name).then(|| name.to_vec());
    }
    let mut dirs: Vec<Vec<u8>> = alloc::vec![b".".to_vec(), crate::profile::WINDOWS_DIR.to_vec(), crate::profile::SYSTEM_DIR.to_vec()];
    if let Some(p) = crate::module::find(0).and_then(|m| crate::module::path_of(m.hmod)) {
        dirs.push(p[..p.iter().rposition(|&c| c == b'\\').unwrap_or(0)].to_vec());
    }
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"PATH\0".as_ptr(), &mut v) } == 0 {
        let p = unsafe { crate::cstr(v) };
        dirs.extend(p.split(|&c| c == b';').filter(|d| !d.is_empty()).map(|d| d.to_vec()));
    }
    for d in dirs {
        let full = [&d[..], b"\\", name].concat();
        if exists(&full) {
            return Some(full);
        }
    }
    None
}

fn temp_file_name(pre: &[u8], unique: u16, buf: *mut u8) -> u16 {
    let mut v: *const u8 = core::ptr::null();
    let dir = if unsafe { os2::DosScanEnv(b"TEMP\0".as_ptr(), &mut v) } == 0 && !v.is_null() {
        unsafe { crate::cstr(v) }.to_vec()
    } else {
        crate::profile::make_dirs();
        crate::profile::WINDOWS_DIR.to_vec()
    };
    let dir = dir.strip_suffix(b"\\").unwrap_or(&dir).to_vec();
    let pre = &pre[..pre.len().min(3)];
    let mk = |n: u16| [&dir[..], b"\\~", pre, alloc::format!("{:04X}.TMP", n).as_bytes()].concat();
    let mut n = unique;
    if unique == 0 {
        n = (unsafe { crate::pm::WinGetCurrentTime(crate::kernel::TASK.get().hab) } & 0xffff) as u16;
        loop {
            n = n.wrapping_add(1).max(1);
            let p = mk(n);
            if !exists(&p) {
                let mut z = p.clone();
                z.push(0);
                let (mut hf, mut action) = (0u32, 0u32);
                if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x10, 0x42, core::ptr::null_mut()) } == 0 {
                    unsafe { os2::DosClose(hf) };
                    break;
                }
            }
        }
    }
    crate::user::copy_out(&mk(n), buf, 144);
    n
}

/// IsBadReadPtr and friends: can `n` bytes at `far` be read (written,
/// executed)?
fn bad_ptr(far: u32, n: u32, write: bool, code: bool) -> bool {
    let sel = far >> 16;
    let off = far & 0xffff;
    if sel == 0 {
        return true;
    }
    let (ok, limit): (u8, u32);
    unsafe { core::arch::asm!("lsl {0:e}, {1:e}", "setz {2}", out(reg) limit, in(reg) sel, out(reg_byte) ok, options(nostack)) };
    if ok == 0 {
        return true;
    }
    let (r, w): (u8, u8);
    unsafe { core::arch::asm!("verr {2:x}", "setz {0}", "verw {2:x}", "setz {1}", out(reg_byte) r, out(reg_byte) w, in(reg) sel, options(nostack)) };
    if code {
        let ar: u32;
        unsafe { core::arch::asm!("lar {0:e}, {1:e}", out(reg) ar, in(reg) sel, options(nostack)) };
        return ar & (1 << 11) == 0;
    }
    if (write && w == 0) || (!write && r == 0) {
        return true;
    }
    n > 0 && off as u64 + n as u64 - 1 > limit as u64
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // WinExec(cmdline, show)
        1166 => {
            let (c, _show) = (a.l(), a.w());
            exec(crate::far_str(c).unwrap_or(b"")) as u32
        }
        // GetTempFileName(drive, prefix, unique, buf): "dir\~PRE1234.TMP"
        // in TEMP (or the Windows directory); with no number given, a new
        // file is made.
        1097 => {
            let (_drive, pre, unique, buf) = (a.w(), a.l(), a.w(), a.p());
            temp_file_name(crate::far_str(pre).unwrap_or(b""), unique, buf) as u32
        }
        // LoadModule(name, lpParameterBlock: environment, command tail (a
        // length byte, then the text), show, reserved): a program started
        // as WinExec starts it (DLLs: LoadLibrary).
        1045 => {
            let (n, pb) = (a.l(), a.p());
            let name = crate::far_str(n).unwrap_or(b"").to_vec();
            let mut cmd = name.clone();
            if !pb.is_null() {
                let tail = thunk16::flat(unsafe { (pb.add(2) as *const u32).read_unaligned() });
                if !tail.is_null() {
                    let len = unsafe { *tail } as usize;
                    let t = unsafe { core::slice::from_raw_parts(tail.add(1), len) };
                    let t = t.split(|&c| c == b'\r').next().unwrap_or(t);
                    cmd.push(b' ');
                    cmd.extend_from_slice(t);
                }
            }
            let up = name.to_ascii_uppercase();
            if up.ends_with(b".DLL") || up.ends_with(b".DRV") {
                return Some(crate::module::load_library(n) as u32);
            }
            exec(&cmd) as u32
        }
        // GetFreeSpace(flags): bytes of global memory free.
        1169 => {
            a.w();
            crate::stress::free_space()
        }
        // GetHeapSpaces(hmodule): the free bytes and the size of a module's
        // local heap (USER's and GDI's, for resource meters: what STRESS
        // left of them; other modules' 90% free).
        1138 => {
            let h = a.w();
            let (free, size) = crate::stress::heap_spaces(h);
            (size as u32) << 16 | free as u32
        }
        // GetNumTasks, IsTask(htask), GetExePtr(h)
        1152 => 1,
        1320 => crate::toolhelp::is_task(a.w()) as u32,
        1133 => {
            let h = a.w();
            crate::module::find(h).map_or(0, |m| m.hinst as u32)
        }
        // IsBadReadPtr(p, n), IsBadWritePtr, IsBadCodePtr(p),
        // IsBadStringPtr(p, n), IsBadHugeReadPtr(p, n), IsBadHugeWritePtr
        1334 | 1335 => {
            let (p, n) = (a.l(), a.w());
            bad_ptr(p, n as u32, ord == 1335, false) as u32
        }
        1336 => bad_ptr(a.l(), 1, false, true) as u32,
        1337 => {
            let (p, n) = (a.l(), a.w());
            if bad_ptr(p, 1, false, false) {
                return Some(1);
            }
            let s = crate::far_str(p).unwrap_or(b"");
            bad_ptr(p, (s.len() + 1).min(n as usize) as u32, false, false) as u32
        }
        1346 | 1347 => {
            let (p, n) = (a.l(), a.l());
            bad_ptr(p, n.min(0x10000), ord == 1347, false) as u32
        }
        // lstrcpyn(dst, src, n)
        1353 => {
            let (d, s, n) = (a.l(), a.l(), a.i());
            let src = crate::far_str(s).unwrap_or(b"");
            crate::user::copy_out(src, thunk16::flat(d), n);
            d
        }
        // GlobalDosAlloc(bytes): selector (low) and segment (high, the
        // same: no real mode here); GlobalDosFree(sel)
        1184 => {
            let n = a.l();
            let s = crate::kernel::global_alloc(0x40, n) as u32;
            s << 16 | s
        }
        1185 => {
            let s = a.w();
            if crate::kernel::global_free(s) { 0 } else { s as u32 }
        }
        // EnumTaskWindows(htask, proc, lParam): the task's top-level
        // windows. Another task's (a process of its own, as a task a
        // debugger stopped) are not this task's to enumerate.
        2225 => {
            let (t, p, lp) = (a.w(), a.l(), a.l());
            if crate::toolhelp::task_pid(t).is_some_and(|p| p != crate::toolhelp::own_pid()) {
                return Some(1);
            }
            for w in crate::user::top_levels() {
                if crate::user::exists(w) && PascalCall::new().w(w).l(lp).call(p) as u16 == 0 {
                    break;
                }
            }
            1
        }
        // DragAcceptFiles(hwnd, accept): no files are dropped on Windows
        // programs yet.
        6509 => {
            a.w();
            a.w();
            0
        }
        // ShellExecute(hwnd, op, file, params, dir, show): programs.
        6520 => {
            let (_h, _op, f, p, _d, _show) = (a.w(), a.l(), a.l(), a.l(), a.l(), a.w());
            let mut c = crate::far_str(f).unwrap_or(b"").to_vec();
            if let Some(p) = crate::far_str(p) {
                c.push(b' ');
                c.extend_from_slice(p);
            }
            exec(&c) as u32
        }
        // TimerCount(lpTimerInfo: dwSize, dwmsSinceStart, dwmsThisVM)
        6280 => {
            let p = a.p();
            if p.is_null() {
                return Some(0);
            }
            let t = unsafe { crate::pm::WinGetCurrentTime(crate::kernel::TASK.get().hab) };
            unsafe {
                (p.add(4) as *mut u32).write_unaligned(t);
                (p.add(8) as *mut u32).write_unaligned(t);
            }
            1
        }
        // hmemcpy(dst, src, n): huge pointers (contiguous here).
        1348 => {
            let (d, s, n) = (a.l(), a.l(), a.l());
            let (d, s) = (crate::kernel::huge(d), crate::kernel::huge(s));
            if !d.is_null() && !s.is_null() {
                unsafe { core::ptr::copy(s, d, n as usize) };
            }
            0
        }
        // SetWindowsHook(type, proc): the previous hook (none);
        // SetWindowsHookEx(type, proc, hinst, task): a hook handle.
        2121 | 2291 => {
            let (kind, p) = (a.w() as i16, a.l());
            let task = if ord == 2291 {
                a.w();
                a.w()
            } else {
                0
            };
            let n = NEXT_HOOK.get();
            let h = *n;
            *n += 1;
            // A hook on another task (a process of its own) is not called
            // here: its handle is given, nothing is hooked.
            if task == 0 || crate::toolhelp::task_pid(task).is_none_or(|p| p == crate::toolhelp::own_pid()) {
                let w32 = unsafe { crate::w32::SetWindowsHookExA(kind as i32 as u32, crate::msg16::proc16to32(p), 0, 0) };
                HOOKS.get().push(Hook { h, kind, proc_: p, w32, global: ord == 2291 && task == 0 });
                publish_hooks();
            }
            if ord == 2291 { h } else { 0 }
        }
        // UnhookWindowsHook(type, proc), UnhookWindowsHookEx(hhook)
        2234 => {
            let (kind, p) = (a.w() as i16, a.l());
            HOOKS.get().retain(|x| {
                let gone = x.kind == kind && x.proc_ == p;
                if gone {
                    unsafe { crate::w32::UnhookWindowsHookEx(x.w32) };
                }
                !gone
            });
            publish_hooks();
            1
        }
        2292 => {
            let h = a.l();
            HOOKS.get().retain(|x| {
                if x.h == h {
                    unsafe { crate::w32::UnhookWindowsHookEx(x.w32) };
                }
                x.h != h
            });
            publish_hooks();
            1
        }
        // DefHookProc(code, wp, lp, lplpfnNext): the next procedure is
        // the one SetWindowsHook returned (none here).
        2235 => {
            let _ = (a.w(), a.w(), a.l(), a.l());
            0
        }
        // CallNextHookEx(hhook, code, wp, lp)
        2293 => {
            let (h, code, wp, lp) = (a.l(), a.w(), a.w(), a.l());
            next_hook(h).map_or(0, |p| PascalCall::new().w(code).w(wp).l(lp).call(p))
        }
        // The network calls (USER 512-530, WNet*): no Windows network.
        // WNetGetCaps(index): no capabilities.
        2513 => {
            a.w();
            0
        }
        // WNetGetConnection(local, remote, lpcb), WNetGetUser(buf, lpcb),
        // WNetAddConnection(net, password, local), WNetCancelConnection(
        // name, force), WNetGetError(lpError), WNetGetErrorText(error,
        // buf, lpcb), WNetGetDirectoryType(dir, lpType): WN_NOT_SUPPORTED
        // (a directory is a local one).
        2512 | 2516 | 2517 | 2518 | 2519 | 2520 | 2530 => {
            match ord {
                2512 | 2520 => {
                    let _ = (a.l(), a.l(), a.l());
                }
                2516 => {
                    let _ = (a.l(), a.l());
                }
                2517 => {
                    let _ = (a.l(), a.l(), a.l());
                }
                2518 => {
                    let _ = (a.l(), a.w());
                }
                2519 => {
                    a.l();
                }
                _ => {
                    let (_d, t) = (a.l(), a.p());
                    if !t.is_null() {
                        unsafe { (t as *mut i16).write_unaligned(0) };
                    }
                }
            }
            1
        }
        // SetMessageQueue(size)
        2266 => {
            a.w();
            1
        }
        // GetFreeSystemResources(type): percent free.
        2284 => {
            a.w();
            90
        }
        _ => return None,
    })
}

// ---- System-wide hooks across tasks ----
//
// On Windows a system-wide WH_CALLWNDPROC or WH_GETMESSAGE hook (in a
// DLL) runs in every task. Here each task is a process, and a hook DLL
// usually only passes the message on to its program (Open Watcom's Spy:
// WSPYHK calls a procedure of WSPY's), which cannot run in another
// process. So the hook runs in the task that installed it: a named shared
// block lists the tasks with system-wide hooks (a window of theirs, the
// hook types, the process), and every other task posts each message its
// hooks would see to that window (message 0x3ED of DDEML's windows, the
// message in a giveable block); the hooking task calls its hooks with it
// (`remote`), the windows being its proxies of the other task's.

const HOOK_REG: &[u8] = b"\\SHAREMEM\\WIN16\\HOOKS\0";
const HOOK_SLOTS: usize = 32;

/// Each slot: the hooking task's window (PM), the hook types (bit n:
/// WH type n), its process.
struct HookReg {
    slots: [[u32; 3]; HOOK_SLOTS],
}

fn hook_reg() -> Option<&'static mut HookReg> {
    static P: Global<usize> = Global::new(0);
    if *P.get() == 0 {
        let mut p: *mut u8 = core::ptr::null_mut();
        let rw = os2::PAG_READ | os2::PAG_WRITE;
        let size = core::mem::size_of::<HookReg>() as u32;
        unsafe {
            if os2::DosGetNamedSharedMem(&mut p, HOOK_REG.as_ptr(), rw) != 0
                && os2::DosAllocSharedMem(&mut p, HOOK_REG.as_ptr(), size, rw | os2::PAG_COMMIT) != 0
                && os2::DosGetNamedSharedMem(&mut p, HOOK_REG.as_ptr(), rw) != 0
            {
                return None;
            }
        }
        *P.get() = p as usize;
    }
    unsafe { (*P.get() as *mut HookReg).as_mut() }
}

/// This task's system-wide hook types into the list (with a window that
/// gets the other tasks' messages).
fn publish_hooks() {
    static WIN: Global<u32> = Global::new(0);
    let kinds = HOOKS.get().iter().filter(|h| h.global && (h.kind == 3 || h.kind == 4)).fold(0u32, |m, h| m | 1 << h.kind);
    let me = crate::dde::pid();
    if kinds != 0 && *WIN.get() == 0 {
        let w = crate::ddeml::messenger_window();
        *WIN.get() = crate::user::pm_client(w);
    }
    let pm = *WIN.get();
    let Some(r) = hook_reg() else { return };
    let mine = r.slots.iter().position(|s| s[2] == me && s[0] != 0);
    match (mine, kinds) {
        (Some(i), 0) => r.slots[i] = [0, 0, 0],
        (Some(i), k) => r.slots[i] = [pm, k, me],
        (None, 0) => {}
        (None, k) => {
            let hab = crate::kernel::TASK.get().hab;
            if let Some(i) = r.slots.iter().position(|s| s[0] == 0 || unsafe { crate::pm::WinIsWindow(hab, s[0]) } == 0) {
                r.slots[i] = [pm, k, me];
            }
        }
    }
}

/// A message this task's hooks of `kind` (3 WH_GETMESSAGE, 4
/// WH_CALLWNDPROC) see: to the other tasks with system-wide hooks of
/// that kind.
pub fn forward(kind: i16, hwnd: u16, msg: u16, wp: u16, lp: u32, time: u32) {
    let Some(r) = hook_reg() else { return };
    let bit = 1u32 << kind;
    if !r.slots.iter().any(|s| s[0] != 0 && s[1] & bit != 0) {
        return;
    }
    let me = crate::dde::pid();
    let pm_win = |h: u16| Some(crate::user::pm_client(h)).filter(|&c| c != 0).or_else(|| crate::dde::foreign(h)).unwrap_or(0);
    let mut b = Vec::with_capacity(16);
    b.extend_from_slice(&pm_win(hwnd).to_le_bytes());
    b.extend_from_slice(&msg.to_le_bytes());
    b.extend_from_slice(&wp.to_le_bytes());
    b.extend_from_slice(&lp.to_le_bytes());
    b.extend_from_slice(&time.to_le_bytes());
    for i in 0..HOOK_SLOTS {
        let [pm, kinds, pid] = r.slots[i];
        if pm == 0 || kinds & bit == 0 || pid == me {
            continue;
        }
        if !crate::ddeml::give_post(pm, 0x03ed, kind as u16, &b) {
            // Gone (its task ended).
            r.slots[i] = [0, 0, 0];
        }
    }
}

/// Another task's message for this task's system-wide hooks of `kind`.
pub fn remote(kind: u16, p: *mut u8) {
    if p.is_null() {
        return;
    }
    let rd16 = |o: usize| unsafe { (p.add(o) as *const u16).read_unaligned() };
    let rd32 = |o: usize| unsafe { (p.add(o) as *const u32).read_unaligned() };
    let (pm, msg, wp, lp, time) = (rd32(0), rd16(4), rd16(6), rd32(8), rd32(12));
    unsafe { os2::DosFreeMem(p) };
    let kind = kind as i16;
    let Some(hook) = HOOKS.get().iter().rev().find(|h| h.global && h.kind == kind).map(|h| h.proc_) else { return };
    let hwnd = crate::dde::proxy(pm);
    let saved = thunk16::sp16();
    let far = thunk16::alloc16(18);
    let q = thunk16::flat(far);
    let mut b = Vec::with_capacity(18);
    if kind == 4 {
        // CWPSTRUCT: lParam, wParam, message, hwnd.
        b.extend_from_slice(&lp.to_le_bytes());
        b.extend_from_slice(&wp.to_le_bytes());
        b.extend_from_slice(&msg.to_le_bytes());
        b.extend_from_slice(&hwnd.to_le_bytes());
    } else {
        // MSG: hwnd, message, wParam, lParam, time, pt.
        b.extend_from_slice(&hwnd.to_le_bytes());
        b.extend_from_slice(&msg.to_le_bytes());
        b.extend_from_slice(&wp.to_le_bytes());
        b.extend_from_slice(&lp.to_le_bytes());
        b.extend_from_slice(&time.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
    }
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), q, b.len()) };
    PascalCall::new().w(0).w(if kind == 4 { 1 } else { 0 }).l(far).call(hook);
    thunk16::set_sp16(saved);
}
