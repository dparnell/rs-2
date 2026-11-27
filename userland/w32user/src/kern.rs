//! What W32USER needs from outside it: W32KERN's calls, and the host.
//!
//! W32USER serves two kinds of process. A Win32 program's has W32KERN
//! (KERNEL32), whose calls are found when first used. A Windows 3.x
//! program's has WIN16 instead, which is a facade over W32USER: it
//! registers itself as the host (`Rs2W32UserHost`), which calls the
//! program's 16-bit procedures, runs the task's side of the message
//! loop, and finds resources in NE modules. With neither, the calls
//! here do without (no last error, no resources).

use crate::Global;
use crate::pm::QMsg;

/// WIN16's side of W32USER, filled in by WIN16.
#[repr(C)]
pub struct Host {
    /// Call a 16-bit procedure with a message in its 32-bit form:
    /// `kind` 0 for a window, dialog or timer procedure (hwnd, msg,
    /// wParam, lParam), 1 for a hook procedure (hook type, code,
    /// wParam, lParam).
    pub call16: unsafe extern "C" fn(kind: u32, far: u32, a: u32, b: u32, c: u32, d: u32) -> u32,
    /// Each pass of GetMessage and PeekMessage (`waiting`: the queue is
    /// empty and the caller will wait or yield).
    pub poll: unsafe extern "C" fn(waiting: u32),
    /// Run `f(arg)`, a wait for messages, letting the task's other
    /// threads run meanwhile.
    pub wait: unsafe extern "C" fn(f: unsafe extern "C" fn(*mut u8), arg: *mut u8),
    /// A PM queue message for no window of W32USER's: 1 if the host
    /// took it.
    pub foreign: unsafe extern "C" fn(q: *const QMsg) -> u32,
    /// A message sent to a window that is not W32USER's (another task's,
    /// or 0xFFFF for all top-level windows): 1 and `*r` set if the host
    /// delivered it.
    pub send_foreign: unsafe extern "C" fn(h: u32, m: u32, wp: u32, lp: u32, r: *mut u32) -> u32,
    /// A message a hook of `kind` sees (WH_GETMESSAGE as GetMessage gives
    /// it, WH_CALLWNDPROC as it is sent), for the host's system-wide
    /// hooks in other processes.
    pub observe: unsafe extern "C" fn(kind: i32, m: *const crate::wnd::Msg),
    /// Flags: 1, every thread uses the first thread's queue (a Windows
    /// task's threads take turns).
    pub flags: u32,
    /// The task's module (GetModuleHandle(NULL)).
    pub hinst: u32,
}

static HOST: Global<Option<&'static Host>> = Global::new(None);

pub fn host() -> Option<&'static Host> {
    *HOST.get()
}

/// Rs2W32UserHost(host): WIN16 makes itself the host (once, at InitTask).
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn Rs2W32UserHost(h: *const Host) -> u32 {
    *HOST.get() = unsafe { h.as_ref() };
    1
}

/// One queue for the process (a Windows task).
pub fn one_queue() -> bool {
    host().is_some_and(|h| h.flags & 1 != 0)
}

// ---- W32KERN, when there is one ----

const NAMES: [&[u8]; 9] = [
    b"SetLastError\0",
    b"GetModuleHandleA\0",
    b"FindResourceW\0",
    b"LoadResource\0",
    b"SizeofResource\0",
    b"GlobalAlloc\0",
    b"GlobalSize\0",
    b"WaitForMultipleObjects\0",
    b"GetModuleFileNameA\0",
];

/// The addresses found: 0 not looked for yet, 1 absent.
static PROCS: Global<[usize; 9]> = Global::new([0; 9]);

fn proc_(i: usize) -> Option<usize> {
    let t = PROCS.get();
    if t[i] == 0 {
        let mut hmod = 0;
        let mut a = 0usize;
        let ok = unsafe { os2::DosQueryModuleHandle(b"W32KERN\0".as_ptr(), &mut hmod) } == 0 && unsafe { os2::DosQueryProcAddr(hmod, 0, NAMES[i].as_ptr(), &mut a) } == 0;
        t[i] = if ok && a > 1 { a } else { 1 };
    }
    (t[i] > 1).then_some(t[i])
}

macro_rules! kern {
    ($i:expr, fn($($t:ty),*) -> $r:ty) => {
        proc_($i).map(|a| unsafe { core::mem::transmute::<usize, unsafe extern "stdcall" fn($($t),*) -> $r>(a) })
    };
}

pub fn set_last_error(e: u32) {
    if let Some(f) = kern!(0, fn(u32) -> ()) {
        unsafe { f(e) };
    }
}

/// GetModuleHandle(NULL): the program's module.
pub fn exe() -> u32 {
    if let Some(h) = host() {
        return h.hinst;
    }
    kern!(1, fn(*const u8) -> u32).map_or(0, |f| unsafe { f(core::ptr::null()) })
}

/// A resource's bytes (FindResourceW, LoadResource, SizeofResource).
pub fn resource(hinst: u32, name: *const u16, ty: *const u16) -> Option<&'static [u8]> {
    let find = kern!(2, fn(u32, *const u16, *const u16) -> u32)?;
    let load = kern!(3, fn(u32, u32) -> u32)?;
    let size = kern!(4, fn(u32, u32) -> u32)?;
    unsafe {
        let r = find(hinst, name, ty);
        if r == 0 {
            return None;
        }
        let p = load(hinst, r);
        let n = size(hinst, r);
        (p != 0).then(|| core::slice::from_raw_parts(p as *const u8, n as usize))
    }
}

/// GlobalAlloc (the clipboard's handles): 0 without W32KERN.
pub fn global_alloc(flags: u32, n: u32) -> u32 {
    kern!(5, fn(u32, u32) -> u32).map_or(0, |f| unsafe { f(flags, n) })
}

pub fn global_size(h: u32) -> u32 {
    kern!(6, fn(u32) -> u32).map_or(0, |f| unsafe { f(h) })
}

/// WaitForMultipleObjects; WAIT_TIMEOUT without W32KERN.
pub fn wait_objects(n: u32, hs: *const u32, all: i32, timeout: u32) -> u32 {
    kern!(7, fn(u32, *const u32, i32, u32) -> u32).map_or(0x102, |f| unsafe { f(n, hs, all, timeout) })
}

pub fn module_file_name(h: u32, b: u32, n: u32) -> u32 {
    kern!(8, fn(u32, u32, u32) -> u32).map_or(0, |f| unsafe { f(h, b, n) })
}

/// Milliseconds since the system started.
pub fn tick_count() -> u32 {
    let mut v = 0u32;
    unsafe { os2::DosQuerySysInfo(14, 14, &mut v, 4) };
    v
}

// ---- The process code page ----

use crate::cptab::{CP437, CP850};

static PROCESS_CP: Global<u32> = Global::new(0);

unsafe extern "C" {
    #[link_name = "DOSCALLS.291"]
    fn DosQueryCp(cb: u32, cps: *mut u32, pcb: *mut u32) -> u32;
}

/// The process's code page table (437 or 850; others are treated as
/// 850), as W32KERN's MultiByteToWideChar uses for CP_ACP.
fn table() -> &'static [u16; 128] {
    let c = PROCESS_CP.get();
    if *c == 0 {
        let mut cps = [0u32; 4];
        let mut n = 0;
        *c = if unsafe { DosQueryCp(16, cps.as_mut_ptr(), &mut n) } == 0 && cps[0] == 437 { 437 } else { 850 };
    }
    if *c == 437 { &CP437 } else { &CP850 }
}

pub fn byte_to_wide(b: u8) -> u16 {
    if b < 0x80 { b as u16 } else { table()[(b - 0x80) as usize] }
}

pub fn wide_to_byte(c: u16) -> u8 {
    if c < 0x80 {
        return c as u8;
    }
    table().iter().position(|&x| x == c).map_or(b'?', |i| i as u8 + 0x80)
}

// ---- The host's side of the message loop ----

/// Each pass of GetMessage and PeekMessage (`waiting`: nothing queued).
pub fn poll(waiting: bool) {
    if let Some(h) = host() {
        unsafe { (h.poll)(waiting as u32) };
    }
}

/// Run `f`, a wait for messages, as the host lets its other threads run.
pub fn wait<T, F: FnOnce() -> T>(f: F) -> T {
    let Some(h) = host() else { return f() };
    struct Ctx<F, T> {
        f: Option<F>,
        r: Option<T>,
    }
    unsafe extern "C" fn tramp<F: FnOnce() -> T, T>(a: *mut u8) {
        let c = unsafe { &mut *(a as *mut Ctx<F, T>) };
        if let Some(f) = c.f.take() {
            c.r = Some(f());
        }
    }
    let mut c = Ctx { f: Some(f), r: None };
    unsafe { (h.wait)(tramp::<F, T>, &mut c as *mut Ctx<F, T> as *mut u8) };
    c.r.take().expect("host wait ran its function")
}

/// A PM queue message for no window of ours: whether the host took it.
pub fn foreign(q: &QMsg) -> bool {
    host().is_some_and(|h| unsafe { (h.foreign)(q) } != 0)
}

/// A message for a window not ours: the host's answer, if it took it.
pub fn send_foreign(hwnd: u32, m: u32, wp: u32, lp: u32) -> Option<u32> {
    let h = host()?;
    let mut r = 0;
    (unsafe { (h.send_foreign)(hwnd, m, wp, lp, &mut r) } != 0).then_some(r)
}

/// Tell the host of a message a hook of `kind` sees.
pub fn observe(kind: i32, m: &crate::wnd::Msg) {
    if let Some(h) = host() {
        unsafe { (h.observe)(kind, m) };
    }
}
