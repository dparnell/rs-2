//! Calls real programs make that do not belong to a bigger part of
//! WIN16: hmemcpy, window placement, hooks, font enumeration, and small
//! GDI and USER queries.

use alloc::vec::Vec;
use thunk16::{Pascal, PascalCall};

use crate::Global;

/// Installed hooks: (handle, type, procedure). WH_GETMESSAGE,
/// WH_KEYBOARD, WH_CALLWNDPROC and WH_MSGFILTER hooks are called (user.rs);
/// the others are kept.
pub struct Hook {
    pub h: u32,
    pub kind: i16,
    pub proc_: u32,
}

pub static HOOKS: Global<Vec<Hook>> = Global::new(Vec::new());
static NEXT_HOOK: Global<u32> = Global::new(0x4b48_0001);

/// Call the newest hook of a type (it passes the call on with
/// CallNextHookEx or DefHookProc).
pub fn call_hooks(kind: i16, code: i16, wp: u16, lp: u32) -> u32 {
    let Some(p) = HOOKS.get().iter().rev().find(|h| h.kind == kind).map(|h| h.proc_) else { return 0 };
    PascalCall::new().w(code as u16).w(wp).l(lp).call(p)
}

/// The hook after `h` (older) in its chain.
fn next_hook(h: u32) -> Option<u32> {
    let hs = HOOKS.get();
    let i = hs.iter().position(|x| x.h == h)?;
    let kind = hs[i].kind;
    hs[..i].iter().rev().find(|x| x.kind == kind).map(|x| x.proc_)
}

pub fn has_hooks(kind: i16) -> bool {
    HOOKS.get().iter().any(|h| h.kind == kind)
}

/// The faces EnumFonts and EnumFontFamilies report: (face, pitch and
/// family, TrueType).
const FACES: &[(&[u8], u8, bool)] = &[
    (b"System", 0x22, false),
    (b"Fixedsys", 0x31, false),
    (b"Terminal", 0x31, false),
    (b"MS Sans Serif", 0x22, false),
    (b"MS Serif", 0x12, false),
    (b"Courier", 0x31, false),
    (b"Arial", 0x22, true),
    (b"Courier New", 0x31, true),
    (b"Times New Roman", 0x12, true),
];

/// EnumFonts(hdc, face, proc, data), EnumFontFamilies: the callback gets
/// a LOGFONT (ENUMLOGFONT), a TEXTMETRIC (NEWTEXTMETRIC), the font type
/// and the data, for each face (or the one asked for).
fn enum_fonts(face: u32, p: u32, data: u32) -> u32 {
    let want = crate::far_str(face).map(|s| s.to_ascii_lowercase());
    let mut r = 1;
    for &(name, pitch, tt) in FACES {
        if want.as_ref().is_some_and(|w| !w.is_empty() && *w != name.to_ascii_lowercase()) {
            continue;
        }
        // ENUMLOGFONT: LOGFONT (50) + full name (64) + style (32);
        // NEWTEXTMETRIC: TEXTMETRIC (31) + flags, sizes (12).
        r = crate::mem16::with(146 + 44, |far, buf| {
            if buf.is_null() {
                return 0;
            }
            let lf = buf;
            let tm = unsafe { buf.add(146) };
            let height: i16 = 16;
            unsafe {
                (lf as *mut i16).write_unaligned(height);
                (lf.add(2) as *mut i16).write_unaligned(if pitch & 3 == 1 { 8 } else { 7 });
                (lf.add(8) as *mut i16).write_unaligned(400);
                *lf.add(17) = pitch;
                *lf.add(13) = 0; // ANSI_CHARSET
                *lf.add(16) = if tt { 2 } else { 0 };
                core::ptr::copy_nonoverlapping(name.as_ptr(), lf.add(18), name.len());
                core::ptr::copy_nonoverlapping(name.as_ptr(), lf.add(50), name.len());
                core::ptr::copy_nonoverlapping(b"Regular".as_ptr(), lf.add(114), 7);
                for (i, v) in [height, 13, 3, 0, 0, if pitch & 3 == 1 { 8 } else { 7 }, 16, 400].iter().enumerate() {
                    (tm.add(2 * i) as *mut i16).write_unaligned(*v);
                }
                *tm.add(19) = 0x20;
                *tm.add(20) = 0xff;
                *tm.add(21) = b'?';
                *tm.add(22) = b' ';
                *tm.add(23) = if pitch & 3 == 1 { 0x30 } else { pitch & 0xf0 | 1 } | if tt { 4 } else { 0 };
                (tm.add(25) as *mut i16).write_unaligned(96);
                (tm.add(27) as *mut i16).write_unaligned(96);
            }
            // RASTER_FONTTYPE 1, TRUETYPE_FONTTYPE 4.
            let ty = if tt { 4 } else { 1 };
            PascalCall::new().l(far).l(far + 146).w(ty).l(data).call(p) as u16 as u32
        });
        if r == 0 {
            break;
        }
    }
    r
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
    match unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 1, a.as_ptr(), core::ptr::null(), &mut res, z.as_ptr()) } {
        0 if windows => crate::toolhelp::wait_started(res.code_terminate).filter(|&h| h > 32).unwrap_or(33),
        0 => 33,
        2 | 3 | 110 => 2,
        8 => 8,
        191 | 193 => 11,
        _ => 31,
    }
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
            16 << 20
        }
        // GetHeapSpaces(hmodule): the free bytes and the size of a module's
        // local heap (USER's and GDI's, for resource meters: 90% free).
        1138 => {
            a.w();
            0xffff_0000 | 0xe666
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
                if crate::user::wnd(w).is_some() && PascalCall::new().w(w).l(lp).call(p) as u16 == 0 {
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
        // GetWindowPlacement(hwnd, lpwndpl), SetWindowPlacement:
        // length, flags, showCmd, ptMinPosition, ptMaxPosition,
        // rcNormalPosition.
        2370 | 2371 => {
            let (h, p) = (a.w(), a.p());
            if p.is_null() || crate::user::wnd(h).is_none() {
                return Some(0);
            }
            if ord == 2370 {
                let r = crate::user::window_rect(h);
                let cmd = if crate::user::is_iconic(h) { 2 } else if crate::user::is_zoomed(h) { 3 } else { 1 };
                let v: [i16; 11] = [22, 0, cmd, -1, -1, -1, -1, r[0] as i16, r[1] as i16, r[2] as i16, r[3] as i16];
                for (i, x) in v.iter().enumerate() {
                    unsafe { (p.add(2 * i) as *mut i16).write_unaligned(*x) };
                }
            } else {
                let rd = |i: usize| unsafe { (p.add(2 * i) as *const i16).read_unaligned() } as i32;
                let cmd = rd(2) as u16;
                crate::user::move_to(h, [rd(7), rd(8), rd(9), rd(10)]);
                crate::user::show_window(h, cmd);
            }
            1
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
                HOOKS.get().push(Hook { h, kind, proc_: p });
            }
            if ord == 2291 { h } else { 0 }
        }
        // UnhookWindowsHook(type, proc), UnhookWindowsHookEx(hhook)
        2234 => {
            let (kind, p) = (a.w() as i16, a.l());
            HOOKS.get().retain(|x| !(x.kind == kind && x.proc_ == p));
            1
        }
        2292 => {
            let h = a.l();
            HOOKS.get().retain(|x| x.h != h);
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
        // RectVisible(hdc, lpRect), PtVisible(hdc, x, y)
        3104 => {
            let (_h, _r) = (a.w(), a.l());
            1
        }
        3103 => {
            let (_h, _x, _y) = (a.w(), a.i(), a.i());
            1
        }
        // UnrealizeObject(obj)
        3150 => {
            a.w();
            1
        }
        // EnumFonts(hdc, face, proc, data), EnumFontFamilies(hdc, family,
        // proc, data)
        3070 | 3330 => {
            let (_h, face, p, data) = (a.w(), a.l(), a.l(), a.l());
            enum_fonts(face, p, data)
        }
        _ => return None,
    })
}
