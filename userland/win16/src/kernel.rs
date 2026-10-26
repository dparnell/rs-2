//! KERNEL: the task (InitTask), global and local memory, modules,
//! strings; and KEYBOARD's few conversions.
//!
//! Global memory blocks are tiled OS/2 memory objects: the handle is the
//! block's selector (GlobalLock gives selector:0), and blocks over 64 KB
//! are consecutive tiles (selectors __AHINCR = 8 apart). A block keeps
//! its whole last 64 KB tile, so GlobalReAlloc grows it in place up to
//! that. Local heaps are kept here, per segment: Windows' handle of a
//! local block is its offset (fixed and moveable alike).

use alloc::collections::BTreeMap;
use thunk16::{far, flat, Pascal};

use crate::pm::{self, trace};
use crate::{cstr, far_str, Global};

pub struct Task {
    /// The instance: the program's DGROUP selector.
    pub hinst: u16,
    /// A DOS program segment prefix (command tail at 0x80) for the C
    /// runtime.
    pub psp: u16,
    pub cmdshow: u16,
    pub hab: u32,
    pub hmq: u32,
    pub error_mode: u16,
}

pub static TASK: Global<Task> = Global::new(Task { hinst: 0, psp: 0, cmdshow: 1, hab: 0, hmq: 0, error_mode: 0 });

struct Block {
    base: *mut u8,
    size: u32,
    /// Bytes reserved (whole tiles).
    room: u32,
    flags: u16,
    locks: u16,
}

static GLOBALS: Global<BTreeMap<u16, Block>> = Global::new(BTreeMap::new());

struct Heap {
    start: u16,
    end: u16,
    /// Offset -> (size, flags, lock count).
    blocks: BTreeMap<u16, (u16, u16, u8)>,
}

static HEAPS: Global<BTreeMap<u16, Heap>> = Global::new(BTreeMap::new());

const GMEM_ZEROINIT: u16 = 0x0040;
const GMEM_MODIFY: u16 = 0x0080;
const GMEM_MOVEABLE: u16 = 0x0002;
pub const GMEM_DDESHARE: u16 = 0x2000;
const LMEM_MOVEABLE: u16 = 0x0002;
const LMEM_ZEROINIT: u16 = 0x0040;
const LMEM_MODIFY: u16 = 0x0080;

/// Fake module handles of the system modules (GetModuleHandle).
const SYSTEM_MODULES: &[(&[u8], u16, u32)] = &[(b"KERNEL", 0x0f07, 1000), (b"USER", 0x0f0f, 2000), (b"GDI", 0x0f17, 3000), (b"KEYBOARD", 0x0f1f, 4000)];

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    match ord {
        // FatalExit(code)
        1 => {
            let code = a.w();
            trace(format_args!("WIN16: FatalExit({code})"));
            unsafe { os2::DosExit(1, code as u32) };
            0
        }
        // GetVersion: Windows 3.10 in AX, DOS 5.0 in DX.
        3 => 0x0500_0a03,
        // LocalInit(seg, start, end)
        4 => {
            let (seg, start, end) = (a.w(), a.w(), a.w());
            local_init(if seg == 0 { crate::caller_ds() } else { seg }, start, end) as u32
        }
        // LocalAlloc(flags, bytes)
        5 => {
            let (fl, n) = (a.w(), a.w());
            local_alloc(crate::caller_ds(), fl, n) as u32
        }
        // LocalReAlloc(h, bytes, flags)
        6 => {
            let (h, n, fl) = (a.w(), a.w(), a.w());
            local_realloc(crate::caller_ds(), h, n, fl) as u32
        }
        // LocalFree(h): 0 when freed.
        7 => {
            let h = a.w();
            let ds = crate::caller_ds();
            let freed = HEAPS.get().get_mut(&ds).is_some_and(|hp| hp.blocks.remove(&h).is_some());
            if freed { 0 } else { h as u32 }
        }
        // LocalLock(h): the block's offset.
        8 => {
            let h = a.w();
            local_block(crate::caller_ds(), h, |b| b.2 = b.2.saturating_add(1)).map_or(0, |_| h as u32)
        }
        // LocalUnlock(h): still locked?
        9 => {
            let h = a.w();
            local_block(crate::caller_ds(), h, |b| b.2 = b.2.saturating_sub(1)).map_or(0, |b| (b.2 > 0) as u32)
        }
        // LocalSize(h)
        10 => {
            let h = a.w();
            local_block(crate::caller_ds(), h, |_| ()).map_or(0, |b| b.0 as u32)
        }
        // LocalHandle(ptr)
        11 => a.w() as u32,
        // LocalFlags(h): lock count.
        12 => {
            let h = a.w();
            local_block(crate::caller_ds(), h, |_| ()).map_or(0, |b| b.2 as u32)
        }
        // LocalCompact(min): the largest free block.
        13 => largest_free(crate::caller_ds()) as u32,
        // GlobalAlloc(flags, bytes)
        15 => {
            let fl = a.w();
            let n = a.l();
            global_alloc(fl, n) as u32
        }
        // GlobalReAlloc(h, bytes, flags)
        16 => {
            let h = a.w();
            let n = a.l();
            let fl = a.w();
            global_realloc(h, n, fl) as u32
        }
        // GlobalFree(h): 0 when freed.
        17 => {
            let h = a.w();
            if global_free(h) { 0 } else { h as u32 }
        }
        // GlobalLock(h): selector:0.
        18 => {
            let h = a.w() | 7;
            match GLOBALS.get().get_mut(&h) {
                Some(b) => {
                    b.locks = b.locks.saturating_add(1);
                    (h as u32) << 16
                }
                // Selectors not allocated here (DGROUP, code) lock too.
                None if h != 7 => (h as u32) << 16,
                None => 0,
            }
        }
        // GlobalUnlock(h): still locked?
        19 => {
            let h = a.w() | 7;
            GLOBALS.get().get_mut(&h).map_or(0, |b| {
                b.locks = b.locks.saturating_sub(1);
                (b.locks > 0) as u32
            })
        }
        // GlobalSize(h)
        20 => {
            let h = a.w() | 7;
            match GLOBALS.get().get(&h) {
                Some(b) => b.size,
                None => segment_size(h),
            }
        }
        // GlobalHandle(sel): handle in AX, selector in DX.
        21 => {
            let s = a.w() | 7;
            (s as u32) << 16 | s as u32
        }
        // GlobalFlags(h): lock count.
        22 => {
            let h = a.w() | 7;
            GLOBALS.get().get(&h).map_or(0, |b| b.locks as u32 & 0xff)
        }
        // LockSegment(seg) / UnlockSegment(seg): -1 is DS.
        23 | 24 => {
            let s = a.w();
            if s == 0xffff { crate::caller_ds() as u32 } else { s as u32 }
        }
        // GlobalCompact(min): the free memory.
        25 => 0x00ff_0000,
        // Yield
        29 => 0,
        // WaitEvent(task): no events for a task here.
        30 => 0,
        // GetCurrentTask (and the first task, in DX)
        36 => {
            let t = TASK.get().hinst as u32;
            t << 16 | t
        }
        // GetCurrentPDB
        37 => TASK.get().psp as u32,
        // GetModuleHandle(name)
        47 => {
            let p = a.l();
            module_handle(p) as u32
        }
        // GetModuleUsage(h)
        48 => {
            a.w();
            1
        }
        // GetModuleFileName(h, buf, cb)
        49 => {
            let h = a.w();
            let buf = a.p();
            let cb = a.i();
            module_file_name(h, buf, cb) as u32
        }
        // GetProcAddress(h, name)
        50 => {
            let h = a.w();
            let name = a.l();
            proc_address(h, name)
        }
        // MakeProcInstance(proc, inst): procedures get the caller's DS
        // already (each task is a process of its own).
        51 => {
            let p = a.l();
            a.w();
            p
        }
        // FreeProcInstance(proc)
        52 => 0,
        // GetInstanceData(inst, ptr, cb): no other instances.
        54 => 0,
        // GetProfileInt(app, key, default), GetPrivateProfileInt(+file):
        // no WIN.INI yet, the default.
        57 | 127 => {
            let (_app, _key, def) = (a.l(), a.l(), a.w());
            def as u32
        }
        // GetProfileString(app, key, default, buf, max),
        // GetPrivateProfileString(+file): the default.
        58 | 128 => {
            let (app, key, def, buf, max) = (a.l(), a.l(), a.l(), a.p(), a.i());
            if ord == 58 {
                // WIN.INI's printers (print.rs).
                if let Some(v) = crate::print::profile(far_str(app).unwrap_or(b""), far_str(key)) {
                    return profile_out(&v, buf, max);
                }
            }
            crate::user::copy_out(far_str(def).unwrap_or(b""), buf, max) as u32
        }
        // WriteProfileString(app, key, value), WritePrivateProfileString
        59 | 129 => 1,
        // FindResource .. SizeofResource
        60..=65 => crate::res::kernel(ord, a),
        // OpenFile(path, lpOfStruct, style)
        74 => {
            let (path, of, style) = (a.l(), a.p(), a.w());
            open_file(far_str(path).unwrap_or(b""), of, style)
        }
        // GetTempDrive(drive): C:
        92 => {
            a.w();
            b'C' as u32
        }
        // LoadLibrary(name): no Windows DLLs of the program's own yet
        // (file not found).
        95 => {
            let n = a.l();
            trace(format_args!("WIN16: LoadLibrary({}) not supported", far_str(n).unwrap_or(b"").iter().map(|&c| c as char).collect::<alloc::string::String>()));
            2
        }
        // FreeLibrary(h)
        96 => {
            a.w();
            0
        }
        // GetWindowsDirectory(buf, max), GetSystemDirectory(buf, max)
        134 | 135 => {
            let (buf, max) = (a.p(), a.i());
            crate::user::copy_out(if ord == 134 { b"C:\\OS2\\MDOS\\WINOS2" } else { b"C:\\OS2\\MDOS\\WINOS2\\SYSTEM" }, buf, max) as u32
        }
        // WinExec(cmdline, show): the program started (asynchronously), a
        // value above 32; else 2 (file not found).
        166 => {
            let (cmd, _show) = (a.l(), a.w());
            win_exec(far_str(cmd).unwrap_or(b""))
        }
        // _lclose(hf)
        81 => {
            let h = a.w();
            if unsafe { os2::DosClose(h as u32) } == 0 { 0 } else { HFILE_ERROR }
        }
        // _lread(hf, buf, cb), _lwrite(hf, buf, cb)
        82 | 86 => {
            let (h, buf, cb) = (a.w(), a.l(), a.w());
            let p = huge(buf);
            let mut n = 0u32;
            let e = unsafe {
                if ord == 82 { os2::DosRead(h as u32, p, cb as u32, &mut n) } else { os2::DosWrite(h as u32, p, cb as u32, &mut n) }
            };
            if e == 0 { n } else { HFILE_ERROR }
        }
        // _lcreat(path, attr), _lopen(path, mode)
        83 | 85 => {
            let (path, v) = (a.l(), a.w());
            let mut p = far_str(path).unwrap_or(b"").to_vec();
            p.push(0);
            let (flags, mode, attr) = if ord == 83 {
                (0x12, 0x42, v as u32 & 0x27)
            } else {
                // OF_READ/WRITE/READWRITE and the OF_SHARE_* modes
                // (compatibility mode: deny none).
                let share = match v as u32 & 0x70 {
                    0 => 0x40,
                    s => s,
                };
                (0x01, (v as u32 & 3) | share, 0)
            };
            let (mut hf, mut action) = (0u32, 0u32);
            match unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut action, 0, attr, flags, mode, core::ptr::null_mut()) } {
                0 => hf,
                _ => HFILE_ERROR,
            }
        }
        // _llseek(hf, offset, origin): the new position.
        84 => {
            let h = a.w();
            let off = a.l() as i32;
            let origin = a.w();
            let mut pos = 0u32;
            match unsafe { os2::DosSetFilePtr(h as u32, off, origin as u32, &mut pos) } {
                0 => pos,
                _ => u32::MAX,
            }
        }
        // lstrcpy(dst, src)
        88 => {
            let (d, s) = (a.l(), a.l());
            let src = unsafe { cstr(flat(s)) };
            let dst = flat(d);
            if !dst.is_null() {
                unsafe {
                    core::ptr::copy(src.as_ptr(), dst, src.len());
                    *dst.add(src.len()) = 0;
                }
            }
            d
        }
        // lstrcat(dst, src)
        89 => {
            let (d, s) = (a.l(), a.l());
            let src = unsafe { cstr(flat(s)) };
            let dst = flat(d);
            if !dst.is_null() {
                let n = unsafe { cstr(dst) }.len();
                unsafe {
                    core::ptr::copy(src.as_ptr(), dst.add(n), src.len());
                    *dst.add(n + src.len()) = 0;
                }
            }
            d
        }
        // lstrlen(s)
        90 => {
            let s = a.l();
            unsafe { cstr(flat(s)) }.len() as u32
        }
        // SetErrorMode(mode): the previous one.
        107 => {
            let m = a.w();
            core::mem::replace(&mut TASK.get().error_mode, m) as u32
        }
        // OutputDebugString(s)
        115 => {
            let s = a.l();
            let t = far_str(s).unwrap_or(b"");
            trace(format_args!("W16 debug: {}", t.iter().map(|&c| c as char).collect::<alloc::string::String>()));
            0
        }
        // GetDOSEnvironment
        131 => {
            let mut tib = core::ptr::null_mut();
            let mut pib: *mut os2::Pib = core::ptr::null_mut();
            unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
            if pib.is_null() { 0 } else { far(unsafe { (*pib).pchenv }) }
        }
        // GetWinFlags: protected mode, 386, enhanced mode, coprocessor.
        132 => WINFLAGS,
        // FatalAppExit(action, msg)
        137 => {
            a.w();
            let m = a.l();
            let text = far_str(m).unwrap_or(b"");
            let mut t = text.to_vec();
            t.push(0);
            unsafe { pm::WinMessageBox(pm::HWND_DESKTOP, 0, t.as_ptr(), b"Application Error\0".as_ptr(), 0, 0x4000 | 0x40) };
            unsafe { os2::DosExit(1, 0xff) };
            0
        }
        // SetHandleCount(n)
        199 => a.w().min(255) as u32,
        // IsDBCSLeadByte(c): single-byte code pages.
        207 => 0,
        _ => 0,
    }
}

const HFILE_ERROR: u32 = 0xffff;

/// WinExec: the program (".EXE" added; as given, else in the program's
/// own directory) started with DosExecPgm, EXEC_ASYNC.
fn win_exec(cmd: &[u8]) -> u32 {
    let cmd = cmd.trim_ascii();
    let n = cmd.iter().position(|&c| c == b' ').unwrap_or(cmd.len());
    let (prog, args) = (&cmd[..n], cmd[n..].trim_ascii());
    if prog.is_empty() {
        return 2;
    }
    let mut prog = prog.to_vec();
    if !prog.rsplit(|&c| c == b'\\').next().unwrap_or(&prog).contains(&b'.') {
        prog.extend_from_slice(b".EXE");
    }
    let mut paths = alloc::vec![prog.clone()];
    if !prog.contains(&b'\\') && !prog.contains(&b':') {
        let mut own = [0u8; 260];
        let k = module_file_name(TASK.get().hinst, own.as_mut_ptr(), 260) as usize;
        if let Some(i) = own[..k].iter().rposition(|&c| c == b'\\') {
            let mut p = own[..=i].to_vec();
            p.extend_from_slice(&prog);
            paths.push(p);
        }
    }
    for p in paths {
        let mut z = p.clone();
        z.push(0);
        // "program\0arguments\0\0"
        let mut argv = p.clone();
        argv.push(0);
        argv.extend_from_slice(args);
        argv.extend_from_slice(&[0, 0]);
        let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
        let mut fail = [0u8; 64];
        // EXEC_ASYNC
        if unsafe { os2::DosExecPgm(fail.as_mut_ptr(), 64, 1, argv.as_ptr(), core::ptr::null(), &mut res, z.as_ptr()) } == 0 {
            return 33;
        }
    }
    2
}

/// A profile string (or, for `list`, NUL-separated keys and a final NUL)
/// into a buffer of `max` bytes: the bytes copied, without the last NUL.
fn profile_out(v: &[u8], buf: *mut u8, max: i32) -> u32 {
    if buf.is_null() || max <= 0 {
        return 0;
    }
    let max = max as usize;
    let n = v.len().min(max - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(v.as_ptr(), buf, n);
        *buf.add(n) = 0;
        if n + 1 < max {
            *buf.add(n + 1) = 0;
        }
    }
    n as u32
}

/// A huge (16:16) pointer as flat memory: tiled, the tiles of a block
/// are contiguous.
fn huge(far: u32) -> *mut u8 {
    flat(far)
}

/// WF_PMODE | WF_CPU386 | WF_ENHANCED | WF_80x87.
pub const WINFLAGS: u32 = 0x0001 | 0x0004 | 0x0020 | 0x0400;

pub fn keyboard(ord: u32, a: &mut Pascal) -> u32 {
    match ord {
        // AnsiToOem(src, dst), OemToAnsi(src, dst): text is passed as it
        // is (both are the process code page here).
        5 | 6 => {
            let (s, d) = (a.l(), a.l());
            let src = unsafe { cstr(flat(s)) };
            let dst = flat(d);
            if !dst.is_null() && dst as *const u8 != src.as_ptr() {
                unsafe {
                    core::ptr::copy(src.as_ptr(), dst, src.len());
                    *dst.add(src.len()) = 0;
                }
            }
            1
        }
        // GetKBCodePage
        132 => 437,
        // GetKeyboardType(which): an enhanced 101/102-key keyboard.
        130 => match a.w() {
            0 => 4,
            1 => 0,
            _ => 12,
        },
        _ => 0,
    }
}

/// InitTask (INT E0h from w16_inittask): the program's start registers
/// in, Windows' InitTask results out (AX 1, ES:BX the command line, CX
/// the stack limit, DX nCmdShow, SI the previous instance, DI the
/// instance).
pub fn init_task(r: &mut crate::dos::Regs) {
    use crate::dos::reg::*;
    let (stack, heap, inst) = (r[BX], r[CX], r[DI]);
    let ds = r[DS];
    let t = TASK.get();
    t.hinst = if inst != 0 { inst } else { ds };
    // The stack: SS:SP is where the program started (a far call and the
    // INT frame below); its bottom (highest address) is the initial SP.
    let bottom = r[SP].wrapping_add(4);
    let top = bottom.wrapping_sub(stack);
    let dgroup = flat((ds as u32) << 16) as *mut u16;
    unsafe {
        // The instance data header: pStackTop, pStackMin, pStackBot.
        dgroup.add(5).write_unaligned(top);
        dgroup.add(6).write_unaligned(bottom);
        dgroup.add(7).write_unaligned(bottom);
    }
    if heap != 0 {
        // The local heap: from the stack's bottom to the end of DGROUP.
        local_init(ds, bottom, 0xfff0);
    }
    t.psp = make_psp();
    r[AX] = 1;
    // The command line: a NUL-terminated copy of the PSP's tail (startup
    // code takes it as a C string).
    r[BX] = 0x100;
    r[ES] = t.psp;
    r[CX] = top;
    r[DX] = t.cmdshow;
    r[SI] = 0;
    r[DI] = t.hinst;
    if crate::tracing() {
        trace(format_args!("W16 InitTask: inst {:#x} stack {:#x}-{:#x} heap {:#x} psp {:#x}", t.hinst, top, bottom, heap, t.psp));
    }
}

/// A program segment prefix with the command tail at 0x80 (length
/// byte, text, CR, NUL) and the environment selector at 0x2C.
fn make_psp() -> u16 {
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocMem(&mut p, 0x1000, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | os2::OBJ_TILE) } != 0 {
        return 0;
    }
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    let mut args: &[u8] = b"";
    let mut env = 0u16;
    if !pib.is_null() {
        // "program\0arguments\0"
        let prog = unsafe { (*pib).pchcmd };
        if !prog.is_null() {
            let n = unsafe { cstr(prog) }.len();
            args = unsafe { cstr(prog.add(n + 1)) };
        }
        env = (far(unsafe { (*pib).pchenv }) >> 16) as u16;
    }
    let args = args.strip_prefix(b" ").unwrap_or(args);
    let n = args.len().min(125);
    unsafe {
        *p = 0xcd;
        *p.add(1) = 0x20;
        (p.add(0x2c) as *mut u16).write_unaligned(env);
        *p.add(0x80) = (n + if n > 0 { 1 } else { 0 }) as u8;
        let mut o = 0x81;
        if n > 0 {
            *p.add(o) = b' ';
            o += 1;
            core::ptr::copy_nonoverlapping(args.as_ptr(), p.add(o), n);
            o += n;
        }
        *p.add(o) = 0x0d;
        *p.add(o + 1) = 0;
        core::ptr::copy_nonoverlapping(args.as_ptr(), p.add(0x100), n);
        *p.add(0x100 + n) = 0;
    }
    (far(p) >> 16) as u16
}

/// The size of a segment not allocated here (its OS/2 memory object).
fn segment_size(sel: u16) -> u32 {
    unsafe extern "C" {
        #[link_name = "DOSCALLS.306"]
        fn DosQueryMem(p: *const u8, cb: *mut u32, fl: *mut u32) -> u32;
    }
    let p = flat((sel as u32) << 16);
    let (mut cb, mut fl) = (0x10000u32, 0u32);
    if unsafe { DosQueryMem(p, &mut cb, &mut fl) } != 0 {
        return 0;
    }
    cb.min(0x10000)
}

pub fn global_alloc(fl: u16, n: u32) -> u16 {
    let size = n.max(1);
    let room = size.next_multiple_of(0x10000);
    let mut p: *mut u8 = core::ptr::null_mut();
    let rw = os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | os2::OBJ_TILE;
    // GMEM_DDESHARE blocks are giveable shared memory: the same address
    // and selector in every process they are given to (DDE, dde.rs).
    let rc = if fl & GMEM_DDESHARE != 0 {
        unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), room, rw | os2::OBJ_GIVEABLE) }
    } else {
        unsafe { os2::DosAllocMem(&mut p, room, rw) }
    };
    if rc != 0 {
        return 0;
    }
    // Committed memory is zero already (GMEM_ZEROINIT).
    let _ = GMEM_ZEROINIT;
    let h = (far(p) >> 16) as u16;
    GLOBALS.get().insert(h, Block { base: p, size, room, flags: fl, locks: 0 });
    h
}

/// A GMEM_DDESHARE block another process gave this one (dde.rs): a
/// global block here from now on.
pub fn adopt(h: u16, size: u32) {
    let h = h | 7;
    if GLOBALS.get().contains_key(&h) {
        return;
    }
    let base = flat((h as u32) << 16);
    let room = size.next_multiple_of(0x10000).max(0x10000);
    GLOBALS.get().insert(h, Block { base, size: size.max(1), room, flags: GMEM_DDESHARE, locks: 0 });
}

/// Is `h` a GMEM_DDESHARE block of this process?
pub fn is_shared(h: u16) -> bool {
    GLOBALS.get().get(&(h | 7)).is_some_and(|b| b.flags & GMEM_DDESHARE != 0)
}

/// GlobalSize of a block of this process (0 if none).
pub fn global_size(h: u16) -> u32 {
    GLOBALS.get().get(&(h | 7)).map_or(0, |b| b.size)
}

pub fn global_free(h: u16) -> bool {
    match GLOBALS.get().remove(&(h | 7)) {
        Some(b) => {
            unsafe { os2::DosFreeMem(b.base) };
            forget_selector(h | 7);
            true
        }
        None => false,
    }
}

/// A selector is gone: the 16-bit caller's DS and ES (saved by the entry
/// stub, below which `sp16` points 32 bytes down) no longer hold it, as
/// Windows' GlobalFree does, so that the return does not fault.
fn forget_selector(sel: u16) {
    let v = thunk16::sp16();
    if v == 0 {
        return;
    }
    // The stub's frame: fs, bx, di, si, es, ds, bp.
    let frame = flat(v.wrapping_add(32)) as *mut u16;
    for i in [4, 5] {
        let p = unsafe { frame.add(i) };
        if unsafe { p.read_unaligned() } | 7 == sel {
            unsafe { p.write_unaligned(0) };
        }
    }
}

/// OpenFile: OF_READ/WRITE/READWRITE, OF_CREATE, OF_EXIST, OF_DELETE,
/// OF_PARSE; the OFSTRUCT (cBytes, fFixedDisk, nErrCode, reserved,
/// szPathName) gets the full path.
fn open_file(path: &[u8], of: *mut u8, style: u16) -> u32 {
    const OF_PARSE: u16 = 0x0100;
    const OF_DELETE: u16 = 0x0200;
    const OF_CREATE: u16 = 0x1000;
    const OF_EXIST: u16 = 0x4000;
    let mut full = [0u8; 260];
    let mut z = path.to_vec();
    z.push(0);
    unsafe extern "C" {
        #[link_name = "DOSCALLS.223"]
        fn DosQueryPathInfo(p: *const u8, level: u32, buf: *mut u8, cb: u32) -> u32;
    }
    // FIL_QUERYFULLNAME
    if unsafe { DosQueryPathInfo(z.as_ptr(), 5, full.as_mut_ptr(), 260) } != 0 {
        full[..path.len().min(259)].copy_from_slice(&path[..path.len().min(259)]);
    }
    let name = unsafe { cstr(full.as_ptr()) }.to_ascii_uppercase();
    let set = |err: u16| {
        if of.is_null() {
            return;
        }
        let mut b = [0u8; 136];
        b[0] = 136;
        b[1] = 1;
        b[2..4].copy_from_slice(&err.to_le_bytes());
        let n = name.len().min(127);
        b[8..8 + n].copy_from_slice(&name[..n]);
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), of, 136) };
    };
    if style & OF_PARSE != 0 {
        set(0);
        return 0;
    }
    if style & OF_DELETE != 0 {
        let ok = unsafe { os2::DosDelete(z.as_ptr()) } == 0;
        set(if ok { 0 } else { 2 });
        return if ok { 1 } else { HFILE_ERROR };
    }
    let (flags, mode) = if style & OF_CREATE != 0 { (0x12, 0x42 | (style as u32 & 3)) } else { (0x01, (style as u32 & 3) | 0x40) };
    let (mut hf, mut action) = (0u32, 0u32);
    let r = unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, flags, mode, core::ptr::null_mut()) };
    if r != 0 {
        set(if r == 110 { 2 } else { r as u16 });
        return HFILE_ERROR;
    }
    set(0);
    if style & OF_EXIST != 0 {
        unsafe { os2::DosClose(hf) };
        return 1;
    }
    hf
}

pub fn global_realloc(h: u16, n: u32, fl: u16) -> u16 {
    let h = h | 7;
    let g = GLOBALS.get();
    let Some(b) = g.get_mut(&h) else { return 0 };
    if fl & GMEM_MODIFY != 0 {
        b.flags = (b.flags & !GMEM_MOVEABLE) | (fl & GMEM_MOVEABLE);
        return h;
    }
    let size = n.max(1);
    if size <= b.room {
        if size > b.size && fl & GMEM_ZEROINIT != 0 {
            unsafe { core::ptr::write_bytes(b.base.add(b.size as usize), 0, (size - b.size) as usize) };
        }
        b.size = size;
        return h;
    }
    // Beyond the reserved tiles: a new block (a new handle).
    let (old, old_size, flags) = (b.base, b.size, b.flags);
    let nh = global_alloc(flags, size);
    if nh == 0 {
        return 0;
    }
    let nb = GLOBALS.get().get(&nh).map(|b| b.base).unwrap();
    unsafe { core::ptr::copy_nonoverlapping(old, nb, old_size as usize) };
    GLOBALS.get().remove(&h);
    unsafe { os2::DosFreeMem(old) };
    forget_selector(h);
    nh
}

pub fn local_init(seg: u16, start: u16, end: u16) -> bool {
    let start = start.max(0x10).next_multiple_of(4);
    if end <= start {
        return false;
    }
    HEAPS.get().insert(seg, Heap { start, end, blocks: BTreeMap::new() });
    // pLocalHeap in the instance data header.
    let p = flat((seg as u32) << 16) as *mut u16;
    unsafe { p.add(3).write_unaligned(start) };
    true
}

fn local_block(seg: u16, h: u16, f: impl FnOnce(&mut (u16, u16, u8))) -> Option<(u16, u16, u8)> {
    let b = HEAPS.get().get_mut(&seg)?.blocks.get_mut(&h)?;
    f(b);
    Some(*b)
}

/// Offset of a free run of `n` bytes in a heap.
fn find_free(hp: &Heap, n: u16) -> Option<u16> {
    let mut at = hp.start as u32;
    for (&off, &(size, _, _)) in &hp.blocks {
        if off as u32 >= at + n as u32 {
            return Some(at as u16);
        }
        at = (off as u32 + size as u32).next_multiple_of(4);
    }
    (at + n as u32 <= hp.end as u32).then_some(at as u16)
}

fn largest_free(seg: u16) -> u16 {
    let Some(hp) = HEAPS.get().get(&seg) else { return 0 };
    let mut at = hp.start as u32;
    let mut best = 0;
    for (&off, &(size, _, _)) in &hp.blocks {
        best = best.max((off as u32).saturating_sub(at));
        at = (off as u32 + size as u32).next_multiple_of(4);
    }
    best.max((hp.end as u32).saturating_sub(at)) as u16
}

fn local_alloc(seg: u16, fl: u16, n: u16) -> u16 {
    let n = n.max(1).next_multiple_of(4);
    let Some(hp) = HEAPS.get().get_mut(&seg) else { return 0 };
    let Some(off) = find_free(hp, n) else { return 0 };
    hp.blocks.insert(off, (n, fl, 0));
    // Blocks start zeroed (LMEM_ZEROINIT or not).
    let p = flat((seg as u32) << 16 | off as u32);
    unsafe { core::ptr::write_bytes(p, 0, n as usize) };
    off
}

fn local_realloc(seg: u16, h: u16, n: u16, fl: u16) -> u16 {
    let Some(hp) = HEAPS.get().get_mut(&seg) else { return 0 };
    let Some(&(size, flags, locks)) = hp.blocks.get(&h) else { return 0 };
    if fl & LMEM_MODIFY != 0 {
        hp.blocks.insert(h, (size, fl & !LMEM_MODIFY, locks));
        return h;
    }
    let n = n.max(1).next_multiple_of(4);
    // In place when the space after the block is free.
    let next = hp.blocks.range(h + 1..).next().map_or(hp.end as u32, |(&o, _)| o as u32);
    if h as u32 + n as u32 <= next {
        hp.blocks.insert(h, (n, flags, locks));
        if n > size && fl & LMEM_ZEROINIT != 0 {
            let p = flat((seg as u32) << 16 | (h + size) as u32);
            unsafe { core::ptr::write_bytes(p, 0, (n - size) as usize) };
        }
        return h;
    }
    if (fl | flags) & LMEM_MOVEABLE == 0 {
        return 0;
    }
    hp.blocks.remove(&h);
    let Some(off) = find_free(hp, n) else {
        hp.blocks.insert(h, (size, flags, locks));
        return 0;
    };
    hp.blocks.insert(off, (n, flags, locks));
    let base = flat((seg as u32) << 16);
    unsafe {
        core::ptr::copy(base.add(h as usize), base.add(off as usize), size.min(n) as usize);
        if n > size {
            core::ptr::write_bytes(base.add((off + size) as usize), 0, (n - size) as usize);
        }
    }
    off
}

fn module_handle(name: u32) -> u16 {
    let t = TASK.get();
    let Some(n) = far_str(name) else {
        // An instance handle: its module.
        return if name as u16 == t.hinst || name == 0 { t.hinst } else { 0 };
    };
    let base = n.rsplit(|&c| c == b'\\' || c == b':').next().unwrap_or(n);
    let base = base.split(|&c| c == b'.').next().unwrap_or(base);
    if let Some(&(_, h, _)) = SYSTEM_MODULES.iter().find(|(m, _, _)| m.eq_ignore_ascii_case(base)) {
        return h;
    }
    // The program itself.
    let mut path = [0u8; 260];
    if module_file_name(t.hinst, path.as_mut_ptr(), 260) > 0 {
        let p = unsafe { cstr(path.as_ptr()) };
        let pb = p.rsplit(|&c| c == b'\\').next().unwrap_or(p);
        let pb = pb.split(|&c| c == b'.').next().unwrap_or(pb);
        if pb.eq_ignore_ascii_case(base) {
            return t.hinst;
        }
    }
    0
}

fn module_file_name(h: u16, buf: *mut u8, cb: i32) -> i32 {
    if buf.is_null() || cb <= 0 {
        return 0;
    }
    let mut path = [0u8; 260];
    let ok = if SYSTEM_MODULES.iter().any(|m| m.1 == h) {
        let mut hmod = 0;
        unsafe { os2::DosQueryModuleHandle(b"WIN16\0".as_ptr(), &mut hmod) == 0 && pm::DosQueryModuleName(hmod, 260, path.as_mut_ptr()) == 0 }
    } else {
        let mut tib = core::ptr::null_mut();
        let mut pib: *mut os2::Pib = core::ptr::null_mut();
        unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
        !pib.is_null() && unsafe { pm::DosQueryModuleName((*pib).hmte, 260, path.as_mut_ptr()) } == 0
    };
    if !ok {
        return 0;
    }
    let p = unsafe { cstr(path.as_ptr()) };
    let n = p.len().min(cb as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(p.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as i32
}

/// GetProcAddress: the system modules' entries (by name or ordinal) are
/// WIN16's.
fn proc_address(h: u16, name: u32) -> u32 {
    let Some(&(_, _, base)) = SYSTEM_MODULES.iter().find(|m| m.1 == h) else { return 0 };
    let mut hmod = 0;
    if unsafe { os2::DosQueryModuleHandle(b"WIN16\0".as_ptr(), &mut hmod) } != 0 {
        return 0;
    }
    let mut f = 0usize;
    let r = match far_str(name) {
        Some(n) => {
            let mut z = n.to_ascii_uppercase();
            z.push(0);
            unsafe { os2::DosQueryProcAddr(hmod, 0, z.as_ptr(), &mut f) }
        }
        None => unsafe { os2::DosQueryProcAddr(hmod, base + (name & 0xffff), core::ptr::null(), &mut f) },
    };
    if r != 0 || f == 0 { 0 } else { far(f as *const u8) }
}
