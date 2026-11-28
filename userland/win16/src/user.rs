//! USER: a facade over W32USER (docs/WIN16.md, "One window manager").
//!
//! W32USER is rs2's window manager for both kinds of Windows program:
//! classes, windows on PM, messages, painting, dialogs, the controls,
//! menus, MDI, the caret, hooks. WIN16 is its host (kern.rs there): it
//! calls the program's 16-bit procedures for W32USER, converting each
//! message to its Win16 form and back (msg16.rs), runs the task's side of
//! the message loop (TOOLHELP, the Win16Lock, DDE with other tasks), and
//! gives W32USER the program's resources in their Win32 forms
//! (tmpl16.rs). The entries here take the Win16 arguments (16-bit
//! handles, 16:16 pointers, Win16 structures) and call W32USER's Win32
//! calls with them. Handles are the same numbers in both.

use alloc::vec::Vec;
use thunk16::{flat, Pascal, PascalCall};

use crate::kernel::TASK;
use crate::msg16::{self, sx, K_BUTTON, K_COMBO, K_DIALOG, K_EDIT, K_LISTBOX, K_MDICLIENT, K_NONE, K_SCROLLBAR, K_STATIC};
use crate::pm::QMsg;
use crate::w32::*;
use crate::{far_str, read_rect, trace, write_rect, Global};

pub const W_SETTEXT: u16 = 0x000c;
pub const W_GETTEXT: u16 = 0x000d;
pub const W_QUIT: u16 = 0x0012;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_CHILD: u32 = 0x4000_0000;

/// PM message number of a Windows message posted with PostMessage
/// (W32USER's).
const POSTED: u32 = 0x0001_0000;

// ---- The host ----

/// W32USER's host interface (kern.rs `Host` there: the same layout).
#[repr(C)]
struct Host {
    call16: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) -> u32,
    poll: unsafe extern "C" fn(u32),
    wait: unsafe extern "C" fn(unsafe extern "C" fn(*mut u8), *mut u8),
    foreign: unsafe extern "C" fn(*const QMsg) -> u32,
    send_foreign: unsafe extern "C" fn(u32, u32, u32, u32, *mut u32) -> u32,
    observe: unsafe extern "C" fn(i32, *const u32),
    global: unsafe extern "C" fn(u32, u32, u32, u32) -> u32,
    resource: unsafe extern "C" fn(u32, u32, u32, *mut u32) -> u32,
    translate: unsafe extern "C" fn(u32, *const QMsg, *mut u32) -> u32,
    posted_base: u32,
    flags: u32,
    hinst: u32,
}

static HOST: Global<Host> = Global::new(Host {
    call16: msg16::host_call16,
    poll: host_poll,
    wait: host_wait,
    foreign: host_foreign,
    send_foreign: host_send_foreign,
    observe: host_observe,
    global: host_global,
    resource: crate::tmpl16::host_resource,
    translate: host_translate,
    posted_base: crate::dde::W16X,
    flags: 1,
    hinst: 0,
});

/// Each pass of GetMessage: TOOLHELP's notifications; with nothing
/// queued, the other tasks run (Yield).
unsafe extern "C" fn host_poll(waiting: u32) {
    crate::toolhelp::poll(waiting != 0);
    if waiting != 0 {
        crate::toolhelp::task_yields();
        crate::w16lock::yield_now();
    }
}

/// A wait for messages: the task's other thread (a debugger's stop) may
/// run meanwhile.
unsafe extern "C" fn host_wait(f: unsafe extern "C" fn(*mut u8), a: *mut u8) {
    crate::w16lock::waiting(|| {
        crate::toolhelp::stop_thread_waits(true);
        unsafe { f(a) };
        crate::toolhelp::stop_thread_waits(false);
    });
}

/// The task's own queue messages (TOOLHELP's).
unsafe extern "C" fn host_foreign(q: *const QMsg) -> u32 {
    let q = unsafe { &*q };
    if q.msg == crate::toolhelp::NOTIFY_MSG && q.hwnd == 0 {
        crate::toolhelp::deliver(q.mp1, q.mp2);
        return 1;
    }
    if q.msg == crate::toolhelp::SWITCH_MSG && q.hwnd == 0 {
        crate::toolhelp::switched_to();
        return 1;
    }
    0
}

/// PM messages W32USER does not translate: other tasks' Windows messages
/// (W16X), PM programs' DDE, the clipboard's (dde.rs, clip.rs).
unsafe extern "C" fn host_translate(h: u32, q: *const QMsg, out: *mut u32) -> u32 {
    let q = unsafe { &*q };
    let m = q.msg;
    let r16 = if crate::dde::is_w16x(m) {
        crate::dde::incoming(m, q.mp1, q.mp2)
    } else if crate::dde::is_pm_dde(m) {
        crate::dde::from_pm(m, q.mp1, q.mp2, q.time)
    } else if crate::clip::is_clip_message(m) {
        crate::clip::translate(m, q.mp1, q.mp2)
    } else {
        return 0;
    };
    let (m32, wp32, lp32) = msg16::posted16to32(h as u16, r16.0, r16.1, r16.2);
    unsafe {
        *out = m32;
        *out.add(1) = wp32;
        *out.add(2) = lp32;
    }
    1
}

/// W32USER sends to a window that is another task's (or to all).
unsafe extern "C" fn host_send_foreign(h: u32, m: u32, wp: u32, lp: u32, r: *mut u32) -> u32 {
    let (m16, wp16, lp16) = msg16::posted32to16(h as u16, m, wp, lp);
    let v = if h as u16 == 0xffff {
        Some(crate::dde::broadcast(m16, wp16, lp16))
    } else {
        crate::dde::send_foreign(h as u16, m16, wp16, lp16)
    };
    match v {
        Some(x) => {
            unsafe { *r = x };
            1
        }
        None => 0,
    }
}

/// What W32USER's hooks see goes to other tasks' system-wide hooks.
unsafe extern "C" fn host_observe(kind: i32, m: *const u32) {
    let m = unsafe { core::slice::from_raw_parts(m, 7) };
    let (m16, wp, lp) = msg16::posted32to16(m[0] as u16, m[1], m[2], m[3]);
    crate::compat::forward(kind as i16, m[0] as u16, m16, wp, lp, m[4]);
}

/// Global memory for W32USER: the task's (16-bit handles).
unsafe extern "C" fn host_global(op: u32, h: u32, n: u32, flags: u32) -> u32 {
    match op {
        0 => crate::kernel::global_alloc(flags as u16, n) as u32,
        1 => flat(((h as u16 | 7) as u32) << 16) as u32,
        2 => crate::kernel::global_size(h as u16),
        3 => crate::kernel::global_realloc(h as u16, n, flags as u16) as u32,
        _ => 1,
    }
}

/// PM for a USER or GDI call before InitApp (DLLs' LibMain run before
/// the program's InitTask: BWCC's registers its messages there).
pub fn ensure_pm() {
    if TASK.get().hmq == 0 {
        init_app();
    }
}

/// InitApp: W32USER's host is WIN16 from now on; the task's PM queue.
fn init_app() -> u32 {
    let t = TASK.get();
    if t.hmq != 0 {
        return 1;
    }
    let h = HOST.get();
    h.hinst = crate::module::find(0).map_or(t.hinst, |m| m.hinst) as u32;
    unsafe { Rs2W32UserHost(h as *const Host as u32) };
    let (mut hab, mut hmq) = (0u32, 0u32);
    unsafe { Rs2W32Queue(&mut hab as *mut u32 as u32, &mut hmq as *mut u32 as u32) };
    t.hab = hab;
    t.hmq = hmq;
    if hmq == 0 {
        trace(format_args!("WIN16: no PM message queue"));
        return 0;
    }
    crate::toolhelp::queue_created(hmq);
    1
}

// ---- The built-in procedures ----

unsafe extern "C" {
    fn p_button();
    fn p_static();
    fn p_edit();
    fn p_listbox();
    fn p_combo();
    fn p_scrollbar();
    fn p_combolbox();
    fn p_mdiclient();
    fn p_commdlg();
    fn p_ddeml();
    fn p_progman();
    fn u_defwindowproc();
    fn u_defdlgproc();
    fn u_defmdichildproc();
}

/// A built-in procedure: its WIN16 entry number, its 16:16 address (the
/// entry's), W32USER's procedure (0 for WIN16's own) and the built-in
/// kind its messages are converted for.
struct Native {
    idx: u32,
    far: u32,
    p32: u32,
    kind: u32,
}

fn natives() -> &'static [Native] {
    static N: Global<Vec<Native>> = Global::new(Vec::new());
    let n = N.get();
    if n.is_empty() {
        let f = |p: unsafe extern "C" fn()| thunk16::far(p as *const u8);
        let a = |p: unsafe extern "stdcall" fn(u32, u32, u32, u32) -> u32| p as usize as u32;
        *n = alloc::vec![
            Native { idx: 2, far: f(p_button), p32: a(Rs2ButtonProc), kind: K_BUTTON },
            Native { idx: 3, far: f(p_static), p32: a(Rs2StaticProc), kind: K_STATIC },
            Native { idx: 4, far: f(p_edit), p32: a(Rs2EditProc), kind: K_EDIT },
            Native { idx: 5, far: f(p_listbox), p32: a(Rs2ListBoxProc), kind: K_LISTBOX },
            Native { idx: 6, far: f(p_combo), p32: a(Rs2ComboBoxProc), kind: K_COMBO },
            Native { idx: 7, far: f(p_scrollbar), p32: a(Rs2ScrollBarProc), kind: K_SCROLLBAR },
            Native { idx: 8, far: f(p_combolbox), p32: a(Rs2ListBoxProc), kind: K_LISTBOX },
            Native { idx: 9, far: f(p_mdiclient), p32: a(Rs2MDIClientProc), kind: K_MDICLIENT },
            Native { idx: 11, far: f(p_commdlg), p32: a(Rs2CommDlgProc), kind: K_DIALOG },
            Native { idx: 2107, far: f(u_defwindowproc), p32: a(DefWindowProcA), kind: K_NONE },
            Native { idx: 2308, far: f(u_defdlgproc), p32: a(DefDlgProcA), kind: K_DIALOG },
            Native { idx: 2447, far: f(u_defmdichildproc), p32: a(DefMDIChildProcA), kind: K_NONE },
            Native { idx: 10, far: f(p_progman), p32: 0, kind: K_NONE },
            Native { idx: 12, far: f(p_ddeml), p32: 0, kind: K_NONE },
        ];
    }
    n
}

/// The 16:16 address of a built-in procedure's entry.
pub fn native_far(idx: u32) -> u32 {
    natives().iter().find(|n| n.idx == idx).map_or(0, |n| n.far)
}

/// W32USER's procedure behind one of WIN16's entries.
pub fn native32_of(far_: u32) -> Option<u32> {
    natives().iter().find(|n| n.far == far_ && n.p32 != 0).map(|n| n.p32)
}

/// WIN16's entry for one of W32USER's procedures.
pub fn native16_of(p: u32) -> Option<u32> {
    natives().iter().find(|n| n.p32 == p).map(|n| n.far)
}

/// A built-in procedure called with a Win16 message (a program calling
/// its entry, CallWindowProc, DefWindowProc): W32USER's, the message
/// converted; WIN16's own in Rust.
pub fn call_native(idx: u32, h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    match idx {
        10 => crate::progman::wnd_proc(h, m, wp, lp),
        12 => crate::ddeml::wnd_proc(h, m, wp, lp),
        _ => {
            let Some(n) = natives().iter().find(|n| n.idx == idx) else { return 0 };
            let (p32, k) = (n.p32, n.kind);
            msg16::to32(h, m, wp, lp, k, &mut |m, wp, lp| unsafe { CallWindowProcA(p32, h as u32, m, wp, lp) })
        }
    }
}

/// WIN16's own procedures, for raw16: None for others.
pub fn win16_native(far_: u32, h: u16, m: u16, wp: u16, lp: u32) -> Option<u32> {
    let idx = natives().iter().find(|n| n.far == far_).map(|n| n.idx)?;
    Some(call_native(idx, h, m, wp, lp))
}

/// Call a window procedure with a Win16 message: a 16-bit one directly,
/// a built-in one through W32USER.
pub fn call_proc(p: u32, h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    if let Some(idx) = natives().iter().find(|n| n.far == p).map(|n| n.idx) {
        return call_native(idx, h, m, wp, lp);
    }
    // A W32USER procedure handed over as it is (a 32-bit value a program
    // got from somewhere other than GetWindowLong).
    if p >> 16 == 0xffff || unsafe { Rs2W32Far16(p) } != 0 {
        let f = unsafe { Rs2W32Far16(p) };
        return msg16::raw16(f, h, m, wp, lp);
    }
    msg16::raw16(p, h, m, wp, lp)
}

// ---- Windows ----

/// Is `h` one of the task's windows?
pub fn exists(h: u16) -> bool {
    h != 0 && unsafe { IsWindow(h as u32) } != 0
}

/// A window's PM client window (0 for none).
pub fn pm_client(h: u16) -> u32 {
    unsafe { Rs2W32PmWindow(h as u32, 0) }
}

/// A window's PM frame.
pub fn pm_frame(h: u16) -> u32 {
    unsafe { Rs2W32PmWindow(h as u32, 1) }
}

/// The window of a PM window.
pub fn hwnd_of(pm: u32) -> u16 {
    unsafe { Rs2W32HwndOfPm(pm, 0) as u16 }
}

fn windows(top: bool) -> Vec<u16> {
    let n = unsafe { Rs2W32Windows(0, 0, top as u32) } as usize;
    let mut v = alloc::vec![0u32; n];
    let n = unsafe { Rs2W32Windows(v.as_mut_ptr() as u32, n as u32, top as u32) } as usize;
    v.iter().take(n).map(|&h| h as u16).collect()
}

/// The task's top-level windows, oldest first.
pub fn top_levels() -> Vec<u16> {
    windows(true)
}


/// The top-level window a window is in.
pub fn top_level(h: u16) -> u16 {
    let mut h = h;
    for _ in 0..64 {
        let style = unsafe { GetWindowLongA(h as u32, (-16i32) as u32) };
        if style & WS_CHILD == 0 {
            return h;
        }
        let p = unsafe { GetParent(h as u32) } as u16;
        if p == 0 {
            return h;
        }
        h = p;
    }
    h
}

/// The classes: (name, module), for TOOLHELP.
pub fn class_list() -> Vec<(Vec<u8>, u16)> {
    let mut v = Vec::new();
    let mut i = 0;
    loop {
        let mut name = [0u8; 64];
        let mut hinst = 0u32;
        if unsafe { Rs2W32ClassInfo(i, name.as_mut_ptr() as u32, 64, &mut hinst as *mut u32 as u32) } == 0 {
            break;
        }
        let n = name.iter().position(|&c| c == 0).unwrap_or(0);
        if n > 0 {
            v.push((name[..n].to_vec(), hinst as u16));
        }
        i += 1;
    }
    v
}

/// A window's descendants, depth first.
fn descendants(h: u16) -> Vec<u16> {
    let mut v = Vec::new();
    let mut c = unsafe { GetWindow(h as u32, 5) } as u16;
    while c != 0 {
        v.push(c);
        v.extend(descendants(c));
        c = unsafe { GetWindow(c as u32, 2) } as u16;
    }
    v
}

/// An instance handle as W32USER is given it: the module's own (a
/// program's instance handles all name one module).
pub fn hinst32(h: u16) -> u32 {
    if h == 0 {
        return 0;
    }
    crate::module::find(h).map_or(h as u32, |m| m.hinst as u32)
}

/// An instance handle for finding a class: the module's (0 for a handle
/// of no module, such as a block a DLL allocated: then W32USER takes the
/// class of that name registered last, as WIN16 did).
pub fn class_hinst32(h: u16) -> u32 {
    if h == 0 {
        return 0;
    }
    crate::module::find(h).map_or(0, |m| m.hinst as u32)
}

/// A 16:16 string or ID argument as W32USER takes it (flat, or the ID).
pub fn str32(far_: u32) -> u32 {
    if far_ >> 16 == 0 { far_ & 0xffff } else { flat(far_) as u32 }
}

/// CW_USEDEFAULT (0x8000) as 32 bits.
fn cw(v: i32) -> u32 {
    if v == -0x8000 { 0x8000_0000 } else { v as u32 }
}

/// CreateWindowEx with Win16 arguments.
pub fn create_window(ex: u32, class: u32, title: u32, style: u32, pos: [i32; 4], parent: u16, menu: u16, hinst: u16, param: u32) -> u16 {
    // MDICLIENT's CLIENTCREATESTRUCT is 32-bit for W32USER's MDICLIENT.
    let mut ccs = [0u32; 2];
    let mut param32 = param;
    let is_mdiclient = match far_str(class) {
        Some(n) => n.eq_ignore_ascii_case(b"MDICLIENT"),
        None => false,
    };
    if is_mdiclient && param != 0 {
        let p = flat(param);
        ccs = [unsafe { (p as *const u16).read_unaligned() } as u32, unsafe { (p.add(2) as *const u16).read_unaligned() } as u32];
        param32 = ccs.as_ptr() as u32;
    }
    let h = unsafe { CreateWindowExA(ex, str32(class), str32(title), style, cw(pos[0]), cw(pos[1]), cw(pos[2]), cw(pos[3]), parent as u32, menu as u32, class_hinst32(hinst), param32) };
    let _ = ccs;
    if crate::tracing() {
        trace(format_args!("W16 CreateWindow -> {h:#x}"));
    }
    h as u16
}

/// Register a class of WIN16's own (DDEML's, the Program Manager
/// stand-in's) and make a hidden pop-up of it.
pub fn own_popup(class: &[u8], proc_far: u32, title: &[u8], hinst: u16) -> u16 {
    let name = z(class);
    let mut wc = [0u32; 10];
    wc[1] = msg16::proc16to32(proc_far);
    wc[4] = hinst32(hinst.max(1));
    wc[9] = name.as_ptr() as u32;
    unsafe { RegisterClassA(wc.as_ptr() as u32) };
    let t = z(title);
    unsafe { CreateWindowExA(0, name.as_ptr() as u32, t.as_ptr() as u32, WS_POPUP, 0, 0, 0, 0, 0, 0, hinst32(hinst.max(1)), 0) as u16 }
}

pub fn destroy_window(h: u16) -> bool {
    unsafe { DestroyWindow(h as u32) != 0 }
}


// ---- Messages ----

/// SendMessage with a Win16 message: through W32USER, converted (to
/// another task's window: DDE's way).
pub fn send(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    if h == 0xffff {
        return crate::dde::broadcast(m, wp, lp);
    }
    if !exists(h) {
        return crate::dde::send_foreign(h, m, wp, lp).unwrap_or(0);
    }
    let k = msg16::kind(h);
    msg16::to32(h, m, wp, lp, k, &mut |m, wp, lp| unsafe { SendMessageA(h as u32, m, wp, lp) })
}

/// PostMessage with a Win16 message.
pub fn post(h: u16, m: u16, wp: u16, lp: u32) -> bool {
    if (0x03e0..=0x03e8).contains(&m) {
        crate::ddeml::monitor_msg(true, h, m, wp, lp);
    }
    let (m32, wp32, lp32) = msg16::posted16to32(h, m, wp, lp);
    if h == 0 || h == 0xffff {
        let q = crate::toolhelp::thread_queue().unwrap_or(TASK.get().hmq);
        return unsafe { crate::pm::WinPostQueueMsg(q, POSTED | m32, wp32, lp32) } != 0;
    }
    if !exists(h) {
        return crate::dde::post_foreign(h, m, wp, lp).unwrap_or(false);
    }
    unsafe { PostMessageA(h as u32, m32, wp32, lp32) != 0 }
}

pub fn def_window_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    call_native(2107, h, m, wp, lp)
}

/// A MSG16 at `p`: (hwnd, message, wParam, lParam).
pub fn read_msg(p: *const u8) -> Option<(u16, u16, u16, u32)> {
    if p.is_null() {
        return None;
    }
    let w = |o: usize| unsafe { (p.add(o) as *const u16).read_unaligned() };
    Some((w(0), w(2), w(4), unsafe { (p.add(6) as *const u32).read_unaligned() }))
}

/// GetMessage / PeekMessage (flags given) into a MSG16.
pub fn get_message(p: *mut u8, h: u16, lo: u16, hi: u16, peek: Option<u16>) -> u32 {
    // A debugger's code that stopped another task waits for messages as
    // this task (toolhelp.rs).
    let saved = crate::toolhelp::set_current(0);
    let mut m = [0u32; 7];
    let mp = m.as_mut_ptr() as u32;
    let r = match peek {
        None => unsafe { GetMessageA(mp, h as u32, lo as u32, hi as u32) },
        Some(fl) => unsafe { PeekMessageA(mp, h as u32, lo as u32, hi as u32, fl as u32) },
    };
    crate::toolhelp::set_current(saved);
    if peek.is_none() || r != 0 {
        msg16::msg32to16(m.as_ptr() as *const u8, p);
        if crate::tracing() && (0x100..=0x109).contains(&m[1]) {
            trace(format_args!("W16 key {:#x} {:#x} {:#x} {:#x}", m[0], m[1], m[2], m[3]));
        }
    }
    if peek.is_none() { (r as i32 > 0) as u32 } else { r }
}

/// A MSG16 as W32USER's MSG.
fn msg32(p: *const u8) -> [u32; 7] {
    let mut m = [0u32; 7];
    msg16::msg16to32(p, &mut m);
    m
}

pub fn translate(p: *const u16) -> bool {
    if p.is_null() {
        return false;
    }
    let m = msg32(p as *const u8);
    unsafe { TranslateMessage(m.as_ptr() as u32) != 0 }
}

pub fn dispatch(p: *mut u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let m = msg32(p);
    // To another task's window: DDE's way.
    let h = m[0] as u16;
    if h != 0 && !exists(h) {
        if let Some((h, msg, wp, lp)) = read_msg(p) {
            return crate::dde::send_foreign(h, msg, wp, lp).unwrap_or(0);
        }
    }
    unsafe { DispatchMessageA(m.as_ptr() as u32) }
}



pub fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// Copy text to a buffer of `max` characters (NUL-terminated, truncated
/// to fit): the characters copied.
pub fn copy_out(t: &[u8], buf: *mut u8, max: i32) -> usize {
    if buf.is_null() || max <= 0 {
        return 0;
    }
    let n = t.len().min(max as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n
}

pub fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(0) as i32, GetSystemMetrics(1) as i32) }
}

/// A point (16-bit) from LPARAM-style packing.
fn pt(v: u32) -> (u32, u32) {
    (sx(v as u16), sx((v >> 16) as u16))
}

/// A Win16 POINT at a far pointer as a 32-bit one and back.
fn with_point(far_: u32, f: impl FnOnce(u32) -> u32) -> u32 {
    let p = flat(far_) as *mut i16;
    if p.is_null() {
        return f(0);
    }
    let mut q = [unsafe { p.read_unaligned() } as i32, unsafe { p.add(1).read_unaligned() } as i32];
    let r = f(q.as_mut_ptr() as u32);
    unsafe {
        p.write_unaligned(q[0] as i16);
        p.add(1).write_unaligned(q[1] as i16);
    }
    r
}

/// A Win16 RECT at a far pointer as a 32-bit one (None for null).
fn rect32(far_: u32) -> Option<[i32; 4]> {
    read_rect(far_)
}

/// Run `f` with a 32-bit copy of a Win16 RECT (0 for none), writing it
/// back.
fn with_rect(far_: u32, f: impl FnOnce(u32) -> u32) -> u32 {
    match rect32(far_) {
        None => f(0),
        Some(mut r) => {
            let v = f(r.as_mut_ptr() as u32);
            write_rect(far_, r);
            v
        }
    }
}

/// A RECT W32USER fills, written as a Win16 one.
fn out_rect(far_: u32, f: impl FnOnce(u32) -> u32) -> u32 {
    let mut r = [0i32; 4];
    let v = f(r.as_mut_ptr() as u32);
    write_rect(far_, r);
    v
}

/// Get/SetWindowWord/Long: procedures converted (GWL_WNDPROC, a
/// dialog's DWL_DLGPROC).
fn window_value(h: u16, i: i32, set: Option<u32>, long: bool) -> u32 {
    let is_proc = i == -4 || (i == 4 && long && msg16::kind(h) == K_DIALOG);
    if is_proc {
        let r = match set {
            Some(v) => unsafe { SetWindowLongA(h as u32, i as u32, msg16::proc16to32(v)) },
            None => unsafe { GetWindowLongA(h as u32, i as u32) },
        };
        return msg16::proc32to16(r);
    }
    unsafe {
        match (set, long) {
            (Some(v), true) => SetWindowLongA(h as u32, i as u32, v),
            (Some(v), false) => SetWindowWord(h as u32, i as u32, v) & 0xffff,
            (None, true) => GetWindowLongA(h as u32, i as u32),
            (None, false) => GetWindowWord(h as u32, i as u32) & 0xffff,
        }
    }
}

/// Get/SetClassWord/Long: GCL_WNDPROC converted.
fn class_value(h: u16, i: i32, set: Option<u32>, long: bool) -> u32 {
    if i == -24 {
        let r = match set {
            Some(v) => unsafe { SetClassLongA(h as u32, i as u32, msg16::proc16to32(v)) },
            None => unsafe { GetClassLongA(h as u32, i as u32) },
        };
        return msg16::proc32to16(r);
    }
    unsafe {
        match (set, long) {
            (Some(v), true) => SetClassLongA(h as u32, i as u32, v),
            (Some(v), false) => SetClassWord(h as u32, i as u32, v) & 0xffff,
            (None, true) => GetClassLongA(h as u32, i as u32),
            (None, false) => GetClassWord(h as u32, i as u32) & 0xffff,
        }
    }
}

/// WNDCLASS16 (style, lpfnWndProc, cbClsExtra, cbWndExtra, hInstance,
/// hIcon, hCursor, hbrBackground, lpszMenuName, lpszClassName) as
/// W32USER's WNDCLASSA.
fn register_class(p: *const u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let w = |o: usize| unsafe { (p.add(o) as *const u16).read_unaligned() } as u32;
    let l = |o: usize| unsafe { (p.add(o) as *const u32).read_unaligned() };
    let wc = [w(0), msg16::proc16to32(l(2)), w(6), w(8), hinst32(w(10) as u16).max(1), w(12), w(14), w(16), str32(l(18)), str32(l(22))];
    if crate::tracing() {
        trace(format_args!("W16 RegisterClass {:?}", alloc::string::String::from_utf8_lossy(far_str(l(22)).unwrap_or(b"#"))));
    }
    unsafe { RegisterClassA(wc.as_ptr() as u32) & 0xffff }
}

/// GetClassInfo into a WNDCLASS16.
fn class_info(hinst: u16, name: u32, p: *mut u8) -> u32 {
    let mut wc = [0u32; 10];
    if unsafe { GetClassInfoA(class_hinst32(hinst), str32(name), wc.as_mut_ptr() as u32) } == 0 {
        return 0;
    }
    if !p.is_null() {
        let mut b = [0u8; 26];
        b[0..2].copy_from_slice(&(wc[0] as u16).to_le_bytes());
        b[2..6].copy_from_slice(&msg16::proc32to16(wc[1]).to_le_bytes());
        b[6..8].copy_from_slice(&(wc[2] as u16).to_le_bytes());
        b[8..10].copy_from_slice(&(wc[3] as u16).to_le_bytes());
        b[10..12].copy_from_slice(&(wc[4] as u16).to_le_bytes());
        b[12..14].copy_from_slice(&(wc[5] as u16).to_le_bytes());
        b[14..16].copy_from_slice(&(wc[6] as u16).to_le_bytes());
        b[16..18].copy_from_slice(&(wc[7] as u16).to_le_bytes());
        b[22..26].copy_from_slice(&name.to_le_bytes());
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 26) };
    }
    1
}

/// PAINTSTRUCT32 (64 bytes) as PAINTSTRUCT16 (32: hdc, fErase, rcPaint,
/// fRestore, fIncUpdate, rgbReserved).
fn paint16(ps32: &[u32; 16], p: *mut u8) {
    if p.is_null() {
        return;
    }
    let w = |o: usize, v: u32| unsafe { (p.add(o) as *mut u16).write_unaligned(v as u16) };
    w(0, ps32[0]);
    w(2, ps32[1]);
    for i in 0..4 {
        w(4 + 2 * i, ps32[2 + i]);
    }
    w(12, ps32[6]);
    w(14, ps32[7]);
}

/// The PAINTSTRUCT32 a BeginPaint gave, for its EndPaint.
static PAINTS: Global<Vec<(u16, [u32; 16])>> = Global::new(Vec::new());

/// WinHelp's owner, for winhelp.rs.
pub fn frame_of_top(h: u16) -> u32 {
    pm_frame(top_level(h))
}

/// A menu item's "new item" argument: a string's flat address (a bitmap
/// handle or owner-drawn data as it is).
fn item32(fl: u16, v: u32) -> u32 {
    if fl & (0x0004 | 0x0100) != 0 { v } else { str32(v) }
}

/// Accelerator tables: the 16-bit handles WIN16 gives out for W32USER's
/// (32-bit) ones.
static ACCELS: Global<Vec<u32>> = Global::new(Vec::new());

fn accel16(h32: u32) -> u16 {
    if h32 == 0 {
        return 0;
    }
    let v = ACCELS.get();
    let i = v.iter().position(|&x| x == h32).unwrap_or_else(|| {
        v.push(h32);
        v.len() - 1
    });
    0xac01 + i as u16
}

fn accel32(h: u16) -> u32 {
    ACCELS.get().get(h.wrapping_sub(0xac01) as usize).copied().unwrap_or(0)
}

/// The PAINTSTRUCT32s of BeginPaints not yet ended.
fn paint_end(hdc: u16) -> Option<[u32; 16]> {
    let v = PAINTS.get();
    let i = v.iter().position(|e| e.0 == hdc)?;
    Some(v.remove(i).1)
}

/// An array of Win16 POINTs as 32-bit ones, written back after `f`.
fn with_points(far_: u32, n: usize, f: impl FnOnce(u32) -> u32) -> u32 {
    let p = flat(far_) as *mut i16;
    if p.is_null() {
        return f(0);
    }
    let mut v: Vec<i32> = (0..2 * n).map(|i| unsafe { p.add(i).read_unaligned() } as i32).collect();
    let r = f(v.as_mut_ptr() as u32);
    for (i, x) in v.iter().enumerate() {
        unsafe { p.add(i).write_unaligned(*x as i16) };
    }
    r
}

/// An array of Win16 ints as 32-bit ones.
fn ints(far_: u32, n: usize) -> Vec<i32> {
    let p = flat(far_) as *const i16;
    if p.is_null() {
        return Vec::new();
    }
    (0..n).map(|i| unsafe { p.add(i).read_unaligned() } as i32).collect()
}

/// A Win16 dialog template in memory (a global block or a far pointer)
/// as Win32's.
fn template32(far_: u32, size: u32) -> Vec<u8> {
    let p = flat(far_);
    if p.is_null() {
        return Vec::new();
    }
    crate::tmpl16::dialog(unsafe { core::slice::from_raw_parts(p, size.clamp(16, 0xffff) as usize) })
}

/// The far address and size of a global block.
fn global_block(h: u16) -> (u32, u32) {
    (((h | 7) as u32) << 16, crate::kernel::global_size(h))
}

static EVENT_HOOK: Global<u32> = Global::new(0);

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    let h32 = |h: u16| h as u32;
    match ord {
        // MessageBox(hwnd, text, caption, type)
        1 => {
            let (h, text, cap, ty) = (a.w(), a.l(), a.l(), a.w());
            if crate::tracing() {
                trace(format_args!("W16 MessageBox: {:?}", alloc::string::String::from_utf8_lossy(far_str(text).unwrap_or(b""))));
            }
            unsafe { MessageBoxA(h32(h), str32(text), if cap == 0 { b"Error\0".as_ptr() as u32 } else { str32(cap) }, ty as u32) }
        }
        // InitApp(hInstance)
        5 => {
            a.w();
            init_app()
        }
        // PostQuitMessage(code)
        6 => {
            let code = a.w();
            unsafe { PostQuitMessage(sx(code)) };
            0
        }
        // SetTimer(hwnd, id, ms, proc), KillTimer(hwnd, id)
        10 => {
            let (h, id, ms, p) = (a.w(), a.w(), a.w(), a.l());
            unsafe { SetTimer(h32(h), id as u32, ms as u32, msg16::proc16to32(p)) & 0xffff }
        }
        12 => {
            let (h, id) = (a.w(), a.w());
            unsafe { KillTimer(h32(h), id as u32) }
        }
        // GetTickCount, GetCurrentTime
        13 | 15 => unsafe { crate::pm::WinGetCurrentTime(TASK.get().hab) },
        // GetCursorPos(lpPoint)
        17 => {
            let p = a.l();
            with_point(p, |q| unsafe { GetCursorPos(q) });
            0
        }
        18 => unsafe { SetCapture(h32(a.w())) & 0xffff },
        19 => {
            unsafe { ReleaseCapture() };
            0
        }
        20 => {
            let ms = a.w();
            unsafe { SetDoubleClickTime(ms as u32) };
            0
        }
        21 => unsafe { GetDoubleClickTime() },
        22 => unsafe { SetFocus(h32(a.w())) & 0xffff },
        23 => unsafe { GetFocus() & 0xffff },
        // ClientToScreen(hwnd, lpPoint), ScreenToClient(hwnd, lpPoint)
        28 | 29 => {
            let (h, p) = (a.w(), a.l());
            with_point(p, |q| unsafe { if ord == 28 { ClientToScreen(h32(h), q) } else { ScreenToClient(h32(h), q) } });
            0
        }
        // WindowFromPoint(POINT)
        30 => {
            let (x, y) = pt(a.l());
            unsafe { WindowFromPoint(x, y) & 0xffff }
        }
        31 => unsafe { IsIconic(h32(a.w())) },
        272 => unsafe { IsZoomed(h32(a.w())) },
        // GetWindowRect(hwnd, lpRect), GetClientRect: the desktop window
        // (GetDesktopWindow's 0 here) is the screen.
        32 | 33 => {
            let (h, r) = (a.w(), a.l());
            if h == 0 {
                let (cx, cy) = screen_size();
                write_rect(r, [0, 0, cx, cy]);
                return 0;
            }
            out_rect(r, |q| unsafe { if ord == 32 { GetWindowRect(h32(h), q) } else { GetClientRect(h32(h), q) } });
            0
        }
        34 => {
            let (h, on) = (a.w(), a.w());
            unsafe { EnableWindow(h32(h), on as u32) }
        }
        35 => unsafe { IsWindowEnabled(h32(a.w())) },
        // GetWindowText(hwnd, buf, max), SetWindowText(hwnd, text),
        // GetWindowTextLength(hwnd): the messages (a subclass sees them).
        36 => {
            let (h, buf, max) = (a.w(), a.l(), a.i());
            if buf == 0 || max <= 0 {
                return 0;
            }
            send(h, W_GETTEXT, max as u16, buf)
        }
        37 => {
            let (h, t) = (a.w(), a.l());
            send(h, W_SETTEXT, 0, t);
            0
        }
        38 => send(a.w(), 0x000e, 0, 0),
        // BeginPaint(hwnd, lpPaint), EndPaint(hwnd, lpPaint)
        39 => {
            let (h, ps) = (a.w(), a.p());
            let mut ps32 = [0u32; 16];
            let hdc = unsafe { BeginPaint(h32(h), ps32.as_mut_ptr() as u32) } as u16;
            paint16(&ps32, ps);
            if hdc != 0 {
                PAINTS.get().push((hdc, ps32));
            }
            hdc as u32
        }
        40 => {
            let (h, ps) = (a.w(), a.p());
            if ps.is_null() {
                return 0;
            }
            let hdc = unsafe { (ps as *const u16).read_unaligned() };
            if let Some(ps32) = paint_end(hdc) {
                unsafe { EndPaint(h32(h), ps32.as_ptr() as u32) };
            } else {
                unsafe { ReleaseDC(h32(h), hdc as u32) };
            }
            0
        }
        // CreateWindow(class, title, style, x, y, cx, cy, parent, menu,
        // instance, param)
        41 => {
            let (class, title, style) = (a.l(), a.l(), a.l());
            let (x, y, cx, cy) = (a.i(), a.i(), a.i(), a.i());
            let (parent, menu, hinst, param) = (a.w(), a.w(), a.w(), a.l());
            create_window(0, class, title, style, [x, y, cx, cy], parent, menu, hinst, param) as u32
        }
        42 => {
            let (h, cmd) = (a.w(), a.w());
            unsafe { ShowWindow(h32(h), cmd as u32) }
        }
        43 => unsafe { CloseWindow(h32(a.w())) },
        44 => unsafe { OpenIcon(h32(a.w())) },
        45 => unsafe { BringWindowToTop(h32(a.w())) },
        46 => unsafe { GetParent(h32(a.w())) & 0xffff },
        47 => {
            let h = a.w();
            (exists(h) || crate::dde::is_foreign_window(h)) as u32
        }
        48 => {
            let (p, h) = (a.w(), a.w());
            unsafe { IsChild(h32(p), h32(h)) }
        }
        49 => unsafe { IsWindowVisible(h32(a.w())) },
        // FindWindow(class, title)
        50 => {
            let (c, t) = (a.l(), a.l());
            unsafe { FindWindowA(str32(c), str32(t)) & 0xffff }
        }
        52 => unsafe { AnyPopup() },
        53 => destroy_window(a.w()) as u32,
        // EnumWindows(proc, lParam), EnumChildWindows(hwnd, proc, lParam)
        54 | 55 => {
            let h = if ord == 55 { a.w() } else { 0 };
            let (p, lp) = (a.l(), a.l());
            let list = if ord == 54 { top_levels() } else { descendants(h) };
            for w in list {
                if exists(w) && PascalCall::new().w(w).l(lp).call(p) as u16 == 0 {
                    break;
                }
            }
            1
        }
        // MoveWindow(hwnd, x, y, cx, cy, repaint)
        56 => {
            let (h, x, y, cx, cy, rp) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.w());
            unsafe { MoveWindow(h32(h), x as u32, y as u32, cx as u32, cy as u32, rp as u32) }
        }
        57 => register_class(a.p()),
        // GetClassName(hwnd, buf, max)
        58 => {
            let (h, buf, max) = (a.w(), a.l(), a.i());
            unsafe { GetClassNameA(h32(h), str32(buf), max as u32) }
        }
        59 => unsafe { SetActiveWindow(h32(a.w())) & 0xffff },
        60 => unsafe { GetActiveWindow() & 0xffff },
        // ScrollWindow(hwnd, dx, dy, lpRect, lpClip)
        61 => {
            let (h, dx, dy, r, c) = (a.w(), a.i(), a.i(), a.l(), a.l());
            let (r, c) = (rect32(r), rect32(c));
            let rp = r.as_ref().map_or(0, |x| x.as_ptr() as u32);
            let cp = c.as_ref().map_or(0, |x| x.as_ptr() as u32);
            unsafe { ScrollWindow(h32(h), dx as u32, dy as u32, rp, cp) };
            0
        }
        // SetScrollPos(hwnd, bar, pos, redraw), GetScrollPos(hwnd, bar)
        62 => {
            let (h, bar, pos, rd) = (a.w(), a.w(), a.i(), a.w());
            unsafe { SetScrollPos(h32(h), bar as u32, pos as u32, rd as u32) & 0xffff }
        }
        63 => {
            let (h, bar) = (a.w(), a.w());
            unsafe { GetScrollPos(h32(h), bar as u32) & 0xffff }
        }
        // SetScrollRange(hwnd, bar, min, max, redraw)
        64 => {
            let (h, bar, lo, hi, rd) = (a.w(), a.w(), a.i(), a.i(), a.w());
            unsafe { SetScrollRange(h32(h), bar as u32, lo as u32, hi as u32, rd as u32) };
            0
        }
        // GetScrollRange(hwnd, bar, lpMin, lpMax)
        65 => {
            let (h, bar, pmin, pmax) = (a.w(), a.w(), a.p() as *mut i16, a.p() as *mut i16);
            let (mut lo, mut hi) = (0i32, 0i32);
            unsafe { GetScrollRange(h32(h), bar as u32, &mut lo as *mut i32 as u32, &mut hi as *mut i32 as u32) };
            unsafe {
                if !pmin.is_null() {
                    pmin.write_unaligned(lo as i16);
                }
                if !pmax.is_null() {
                    pmax.write_unaligned(hi as i16);
                }
            }
            0
        }
        66 => unsafe { GetDC(h32(a.w())) & 0xffff },
        67 => unsafe { GetWindowDC(h32(a.w())) & 0xffff },
        68 => {
            let (h, hdc) = (a.w(), a.w());
            unsafe { ReleaseDC(h32(h), hdc as u32) }
        }
        69 => unsafe { SetCursor(a.w() as u32) & 0xffff },
        70 => {
            let (x, y) = (a.i(), a.i());
            unsafe { SetCursorPos(x as u32, y as u32) };
            0
        }
        71 => unsafe { ShowCursor(a.w() as u32) & 0xffff },
        // The rectangle calls.
        72 => {
            let r = a.l();
            let v = [a.i(), a.i(), a.i(), a.i()];
            write_rect(r, v);
            0
        }
        73 => {
            write_rect(a.l(), [0; 4]);
            0
        }
        74 => {
            let (d, s) = (a.l(), a.l());
            if let Some(r) = read_rect(s) {
                write_rect(d, r);
            }
            1
        }
        75 => read_rect(a.l()).is_none_or(|r| r[2] <= r[0] || r[3] <= r[1]) as u32,
        76 => {
            let r = a.l();
            let (x, y) = pt(a.l());
            let (x, y) = (x as i32, y as i32);
            read_rect(r).is_some_and(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]) as u32
        }
        77 | 78 => {
            let (p, dx, dy) = (a.l(), a.i(), a.i());
            if let Some(r) = read_rect(p) {
                write_rect(p, if ord == 77 { [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy] } else { [r[0] - dx, r[1] - dy, r[2] + dx, r[3] + dy] });
            }
            0
        }
        79 | 80 => {
            let (d, s1, s2) = (a.l(), a.l(), a.l());
            let (Some(x), Some(y)) = (read_rect(s1), read_rect(s2)) else { return 0 };
            let empty = |r: [i32; 4]| r[2] <= r[0] || r[3] <= r[1];
            let r = if ord == 79 {
                [x[0].max(y[0]), x[1].max(y[1]), x[2].min(y[2]), x[3].min(y[3])]
            } else if empty(x) {
                y
            } else if empty(y) {
                x
            } else {
                [x[0].min(y[0]), x[1].min(y[1]), x[2].max(y[2]), x[3].max(y[3])]
            };
            let r = if empty(r) { [0; 4] } else { r };
            write_rect(d, r);
            !empty(r) as u32
        }
        // FillRect(hdc, lpRect, hbrush), InvertRect(hdc, lpRect),
        // FrameRect(hdc, lpRect, hbrush)
        81 | 83 => {
            let (hdc, r, br) = (a.w(), a.l(), a.w());
            let Some(r) = rect32(r) else { return 0 };
            unsafe { if ord == 81 { FillRect(hdc as u32, r.as_ptr() as u32, br as u32) } else { FrameRect(hdc as u32, r.as_ptr() as u32, br as u32) } }
        }
        82 => {
            let (hdc, r) = (a.w(), a.l());
            let Some(r) = rect32(r) else { return 0 };
            unsafe { InvertRect(hdc as u32, r.as_ptr() as u32) };
            0
        }
        // DrawIcon(hdc, x, y, hicon)
        84 => {
            let (hdc, x, y, i) = (a.w(), a.i(), a.i(), a.w());
            unsafe { DrawIcon(hdc as u32, x as u32, y as u32, i as u32) }
        }
        // DrawText(hdc, text, count, lpRect, format)
        85 => {
            let (hdc, s, n, r, fmt) = (a.w(), a.l(), a.i(), a.l(), a.w());
            with_rect(r, |q| unsafe { DrawTextA(hdc as u32, str32(s), n as u32, q, fmt as u32) }) & 0xffff
        }
        // DialogBox(hinst, template, owner, proc), DialogBoxParam(+param)
        87 | 239 => {
            let (hi, t, owner, p) = (a.w(), a.l(), a.w(), a.l());
            let param = if ord == 239 { a.l() } else { 0 };
            unsafe { DialogBoxParamA(hinst32(hi), str32(t), h32(owner), msg16::proc16to32(p), param) }
        }
        // CreateDialog(hinst, template, owner, proc), +Param
        89 | 241 => {
            let (hi, t, owner, p) = (a.w(), a.l(), a.w(), a.l());
            let param = if ord == 241 { a.l() } else { 0 };
            unsafe { CreateDialogParamA(hinst32(hi), str32(t), h32(owner), msg16::proc16to32(p), param) & 0xffff }
        }
        // DialogBoxIndirect(hinst, hglobal, owner, proc), +Param
        218 | 240 => {
            let (hi, hg, owner, p) = (a.w(), a.w(), a.w(), a.l());
            let param = if ord == 240 { a.l() } else { 0 };
            let (far_, n) = global_block(hg);
            let t = template32(far_, n);
            unsafe { DialogBoxIndirectParamA(hinst32(hi), t.as_ptr() as u32, h32(owner), msg16::proc16to32(p), param) }
        }
        // CreateDialogIndirect(hinst, lpTemplate, owner, proc), +Param
        219 | 242 => {
            let (hi, t, owner, p) = (a.w(), a.l(), a.w(), a.l());
            let param = if ord == 242 { a.l() } else { 0 };
            let t = template32(t, 0xffff - (t & 0xffff));
            unsafe { CreateDialogIndirectParamA(hinst32(hi), t.as_ptr() as u32, h32(owner), msg16::proc16to32(p), param) & 0xffff }
        }
        88 => {
            let (h, r) = (a.w(), a.w());
            unsafe { EndDialog(h32(h), sx(r)) };
            1
        }
        90 => {
            let (h, p) = (a.w(), a.p());
            if p.is_null() {
                return 0;
            }
            let m = msg32(p);
            unsafe { IsDialogMessageA(h32(h), m.as_ptr() as u32) }
        }
        91 => {
            let (h, id) = (a.w(), a.w());
            unsafe { GetDlgItem(h32(h), id as u32) & 0xffff }
        }
        92 => {
            let (h, id, t) = (a.w(), a.w(), a.l());
            unsafe { SetDlgItemTextA(h32(h), id as u32, str32(t)) };
            0
        }
        93 => {
            let (h, id, b, n) = (a.w(), a.w(), a.l(), a.i());
            unsafe { GetDlgItemTextA(h32(h), id as u32, str32(b), n as u32) }
        }
        94 => {
            let (h, id, v, s) = (a.w(), a.w(), a.w(), a.w());
            unsafe { SetDlgItemInt(h32(h), id as u32, if s != 0 { sx(v) } else { v as u32 }, s as u32) };
            0
        }
        95 => {
            let (h, id, ok, s) = (a.w(), a.w(), a.p() as *mut u16, a.w());
            let mut t = 0u32;
            let r = unsafe { GetDlgItemInt(h32(h), id as u32, &mut t as *mut u32 as u32, s as u32) };
            if !ok.is_null() {
                unsafe { ok.write_unaligned(t as u16) };
            }
            r & 0xffff
        }
        96 => {
            let (h, x, y, c) = (a.w(), a.w(), a.w(), a.w());
            unsafe { CheckRadioButton(h32(h), x as u32, y as u32, c as u32) };
            0
        }
        97 => {
            let (h, id, c) = (a.w(), a.w(), a.w());
            unsafe { CheckDlgButton(h32(h), id as u32, c as u32) };
            0
        }
        98 => {
            let (h, id) = (a.w(), a.w());
            unsafe { IsDlgButtonChecked(h32(h), id as u32) }
        }
        // DlgDirSelect(hwnd, buf, id), DlgDirList(hwnd, spec, lb, st, attr)
        99 => {
            let (h, b, id) = (a.w(), a.l(), a.w());
            unsafe { DlgDirSelectExA(h32(h), str32(b), 128, id as u32) }
        }
        100 => {
            let (h, s, lb, st, at) = (a.w(), a.l(), a.w(), a.w(), a.w());
            unsafe { DlgDirListA(h32(h), str32(s), lb as u32, st as u32, at as u32) }
        }
        // SendDlgItemMessage(hwnd, id, msg, wParam, lParam)
        101 => {
            let (h, id, m, wp, lp) = (a.w(), a.w(), a.w(), a.w(), a.l());
            let c = unsafe { GetDlgItem(h32(h), id as u32) } as u16;
            if c == 0 {
                return 0;
            }
            send(c, m, wp, lp)
        }
        // AdjustWindowRect(lpRect, style, menu), AdjustWindowRectEx(+ex)
        102 | 454 => {
            let (r, style, menu) = (a.l(), a.l(), a.w());
            let ex = if ord == 454 { a.l() } else { 0 };
            with_rect(r, |q| unsafe { AdjustWindowRectEx(q, style, menu as u32, ex) });
            1
        }
        103 => {
            let (h, r) = (a.w(), a.l());
            with_rect(r, |q| unsafe { MapDialogRect(h32(h), q) });
            0
        }
        104 => {
            let t = a.w();
            unsafe { MessageBeep(t as u32) };
            0
        }
        105 => {
            let (h, on) = (a.w(), a.w());
            unsafe { FlashWindow(h32(h), on as u32) }
        }
        106 => sx(unsafe { GetKeyState(a.w() as u32) } as u16),
        249 => sx(unsafe { GetAsyncKeyState(a.w() as u32) } as u16),
        // DefWindowProc(hwnd, msg, wParam, lParam)
        107 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            def_window_proc(h, m, wp, lp)
        }
        // GetMessage(lpMsg, hwnd, min, max)
        108 => {
            let (p, h, lo, hi) = (a.p(), a.w(), a.w(), a.w());
            if crate::tracing() {
                let (_, ip, cs) = thunk16::caller16();
                trace(format_args!("W16 GetMessage from {cs:04x}:{ip:04x}"));
            }
            get_message(p, h, lo, hi, None)
        }
        // PeekMessage(lpMsg, hwnd, min, max, flags)
        109 => {
            let (p, h, lo, hi, fl) = (a.p(), a.w(), a.w(), a.w(), a.w());
            get_message(p, h, lo, hi, Some(fl))
        }
        110 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            post(h, m, wp, lp) as u32
        }
        111 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            // (DDEML monitors see what a program sends.)
            crate::ddeml::monitor_msg(false, h, m, wp, lp);
            send(h, m, wp, lp)
        }
        112 => {
            unsafe { WaitMessage() };
            0
        }
        113 => translate(a.p() as *const u16) as u32,
        114 => dispatch(a.p()),
        115 => {
            let r = a.l();
            unsafe { ReplyMessage(r) };
            0
        }
        // PostAppMessage(task, msg, wParam, lParam)
        116 => {
            let (t, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            let (m32, wp32, lp32) = msg16::posted16to32(0, m, wp, lp);
            // To a task being stopped: its thread (toolhelp.rs).
            if let Some(q) = crate::toolhelp::task_queue(t) {
                return unsafe { crate::pm::WinPostQueueMsg(q, POSTED | m32, wp32, lp32) };
            }
            // To this task (from a stop's thread too): its own queue.
            if t == 0 || t | 7 == crate::toolhelp::own_tdb() | 7 {
                let q = if t == 0 { crate::toolhelp::thread_queue().unwrap_or(TASK.get().hmq) } else { TASK.get().hmq };
                return unsafe { crate::pm::WinPostQueueMsg(q, POSTED | m32, wp32, lp32) };
            }
            post(0, m, wp, lp) as u32
        }
        119 => unsafe { GetMessagePos() },
        120 => unsafe { GetMessageTime() },
        // CallWindowProc(proc, hwnd, msg, wParam, lParam)
        122 => {
            let (p, h, m, wp, lp) = (a.l(), a.w(), a.w(), a.w(), a.l());
            call_proc(p, h, m, wp, lp)
        }
        124 => {
            unsafe { UpdateWindow(h32(a.w())) };
            0
        }
        // InvalidateRect(hwnd, lpRect, erase), ValidateRect(hwnd, lpRect)
        125 | 127 => {
            let (h, r) = (a.w(), a.l());
            let e = if ord == 125 { a.w() } else { 0 };
            if h == 0 {
                return 0;
            }
            let r = rect32(r);
            let rp = r.as_ref().map_or(0, |x| x.as_ptr() as u32);
            unsafe { if ord == 125 { InvalidateRect(h32(h), rp, e as u32) } else { ValidateRect(h32(h), rp) } };
            0
        }
        // InvalidateRgn(hwnd, hrgn, erase), ValidateRgn(hwnd, hrgn)
        126 | 128 => {
            let (h, r) = (a.w(), a.w());
            let e = if ord == 126 { a.w() } else { 0 };
            unsafe { if ord == 126 { InvalidateRgn(h32(h), r as u32, e as u32) } else { ValidateRgn(h32(h), r as u32) } };
            0
        }
        129 | 131 => {
            let (h, i) = (a.w(), a.i());
            class_value(h, i, None, ord == 131)
        }
        130 => {
            let (h, i, v) = (a.w(), a.i(), a.w());
            class_value(h, i, Some(v as u32), false)
        }
        132 => {
            let (h, i, v) = (a.w(), a.i(), a.l());
            class_value(h, i, Some(v), true)
        }
        133 | 135 => {
            let (h, i) = (a.w(), a.i());
            window_value(h, i, None, ord == 135)
        }
        134 => {
            let (h, i, v) = (a.w(), a.i(), a.w());
            window_value(h, i, Some(v as u32), false)
        }
        136 => {
            let (h, i, v) = (a.w(), a.i(), a.l());
            window_value(h, i, Some(v), true)
        }
        // ---- Menus ----
        // LoadMenu(hinst, name)
        150 => {
            let (hi, n) = (a.w(), a.l());
            unsafe { LoadMenuA(hinst32(hi), str32(n)) & 0xffff }
        }
        151 => unsafe { CreateMenu() & 0xffff },
        415 => unsafe { CreatePopupMenu() & 0xffff },
        152 => unsafe { DestroyMenu(a.w() as u32) },
        // ChangeMenu(hmenu, cmd, text, id, flags): Windows 2.x's one call:
        // MF_APPEND (0x100), MF_DELETE (0x200), MF_REMOVE (0x1000),
        // MF_CHANGE (0x80), else MF_INSERT.
        153 => {
            let (m, cmd, text, id, fl) = (a.w(), a.w(), a.l(), a.w(), a.w());
            let f = fl & !(0x80 | 0x100 | 0x200 | 0x1000);
            unsafe {
                if fl & 0x100 != 0 {
                    AppendMenuA(m as u32, f as u32, id as u32, item32(f, text))
                } else if fl & 0x200 != 0 {
                    DeleteMenu(m as u32, cmd as u32, f as u32)
                } else if fl & 0x1000 != 0 {
                    RemoveMenu(m as u32, cmd as u32, f as u32)
                } else if fl & 0x80 != 0 {
                    ModifyMenuA(m as u32, cmd as u32, f as u32, id as u32, item32(f, text))
                } else {
                    InsertMenuA(m as u32, cmd as u32, f as u32, id as u32, item32(f, text))
                }
            }
        }
        154 => {
            let (m, id, fl) = (a.w(), a.w(), a.w());
            unsafe { CheckMenuItem(m as u32, id as u32, fl as u32) & 0xffff }
        }
        155 => {
            let (m, id, fl) = (a.w(), a.w(), a.w());
            unsafe { EnableMenuItem(m as u32, id as u32, fl as u32) & 0xffff }
        }
        156 => {
            let (h, rv) = (a.w(), a.w());
            unsafe { GetSystemMenu(h32(h), rv as u32) & 0xffff }
        }
        157 => unsafe { GetMenu(h32(a.w())) & 0xffff },
        158 => {
            let (h, m) = (a.w(), a.w());
            unsafe { SetMenu(h32(h), m as u32) }
        }
        159 => {
            let (m, pos) = (a.w(), a.w());
            unsafe { GetSubMenu(m as u32, pos as u32) & 0xffff }
        }
        160 => {
            unsafe { DrawMenuBar(h32(a.w())) };
            0
        }
        161 => {
            let (m, id, b, n, fl) = (a.w(), a.w(), a.l(), a.i(), a.w());
            unsafe { GetMenuStringA(m as u32, id as u32, str32(b), n as u32, fl as u32) }
        }
        162 => {
            let (h, m, id, fl) = (a.w(), a.w(), a.w(), a.w());
            unsafe { HiliteMenuItem(h32(h), m as u32, id as u32, fl as u32) }
        }
        // LoadMenuIndirect(lpTemplate)
        220 => {
            let t = a.l();
            let p = flat(t);
            if p.is_null() {
                return 0;
            }
            let m = crate::tmpl16::menu(unsafe { core::slice::from_raw_parts(p, (0x10000 - (t & 0xffff)) as usize) });
            unsafe { LoadMenuIndirectA(m.as_ptr() as u32) & 0xffff }
        }
        250 => {
            let (m, id, fl) = (a.w(), a.w(), a.w());
            unsafe { GetMenuState(m as u32, id as u32, fl as u32) & 0xffff }
        }
        263 => unsafe { GetMenuItemCount(a.w() as u32) & 0xffff },
        264 => {
            let (m, pos) = (a.w(), a.w());
            unsafe { GetMenuItemID(m as u32, pos as u32) & 0xffff }
        }
        358 => unsafe { IsMenu(a.w() as u32) },
        // InsertMenu(hmenu, pos, flags, id, text), AppendMenu(hmenu,
        // flags, id, text), ModifyMenu(hmenu, pos, flags, id, text)
        410 | 414 => {
            let (m, pos, fl, id, t) = (a.w(), a.w(), a.w(), a.w(), a.l());
            unsafe { if ord == 410 { InsertMenuA(m as u32, pos as u32, fl as u32, id as u32, item32(fl, t)) } else { ModifyMenuA(m as u32, pos as u32, fl as u32, id as u32, item32(fl, t)) } }
        }
        411 => {
            let (m, fl, id, t) = (a.w(), a.w(), a.w(), a.l());
            unsafe { AppendMenuA(m as u32, fl as u32, id as u32, item32(fl, t)) }
        }
        412 | 413 => {
            let (m, pos, fl) = (a.w(), a.w(), a.w());
            unsafe { if ord == 412 { RemoveMenu(m as u32, pos as u32, fl as u32) } else { DeleteMenu(m as u32, pos as u32, fl as u32) } }
        }
        // TrackPopupMenu(hmenu, flags, x, y, reserved, hwnd, lpRect)
        416 => {
            let (m, fl, x, y, r, h, rc) = (a.w(), a.w(), a.i(), a.i(), a.w(), a.w(), a.l());
            let rc = rect32(rc);
            let rp = rc.as_ref().map_or(0, |x| x.as_ptr() as u32);
            unsafe { TrackPopupMenu(m as u32, fl as u32, x as u32, y as u32, r as u32, h32(h), rp) }
        }
        417 => unsafe { GetMenuCheckMarkDimensions() },
        418 => {
            let (m, pos, fl, u, c) = (a.w(), a.w(), a.w(), a.w(), a.w());
            unsafe { SetMenuItemBitmaps(m as u32, pos as u32, fl as u32, u as u32, c as u32) }
        }
        187 => {
            unsafe { EndMenu() };
            0
        }
        // ---- The caret ----
        163 => {
            let (h, bm, cx, cy) = (a.w(), a.w(), a.i(), a.i());
            unsafe { CreateCaret(h32(h), bm as u32, cx as u32, cy as u32) };
            0
        }
        164 => {
            unsafe { DestroyCaret() };
            0
        }
        165 => {
            let (x, y) = (a.i(), a.i());
            unsafe { SetCaretPos(x as u32, y as u32) };
            0
        }
        166 => {
            unsafe { HideCaret(h32(a.w())) };
            0
        }
        167 => {
            unsafe { ShowCaret(h32(a.w())) };
            0
        }
        168 => {
            unsafe { SetCaretBlinkTime(a.w() as u32) };
            0
        }
        169 => unsafe { GetCaretBlinkTime() },
        183 => {
            let p = a.l();
            with_point(p, |q| unsafe { GetCaretPos(q) });
            0
        }
        // ---- Resources ----
        // LoadCursor(hinst, name), LoadIcon(hinst, name), LoadBitmap
        173 | 174 | 175 => {
            let (hi, n) = (a.w(), a.l());
            let (hi, n) = (hinst32(hi), str32(n));
            unsafe {
                (match ord {
                    173 => LoadCursorA(hi, n),
                    174 => LoadIconA(hi, n),
                    _ => LoadBitmapA(hi, n),
                }) & 0xffff
            }
        }
        // LoadAccelerators(hinst, name), TranslateAccelerator(hwnd, haccel,
        // lpMsg)
        177 => {
            let (hi, n) = (a.w(), a.l());
            accel16(unsafe { LoadAcceleratorsA(hinst32(hi), str32(n)) }) as u32
        }
        178 => {
            let (h, ac, p) = (a.w(), a.w(), a.p());
            if p.is_null() {
                return 0;
            }
            let m = msg32(p);
            unsafe { TranslateAcceleratorA(h32(h), accel32(ac), m.as_ptr() as u32) }
        }
        179 => sx(unsafe { GetSystemMetrics(a.i() as u32) } as u16),
        180 => unsafe { GetSysColor(a.w() as u32) },
        // SetSysColors(n, lpIndexes, lpColors)
        181 => {
            let (n, ix, cs) = (a.i(), a.l(), a.l());
            let v = ints(ix, n.max(0) as usize);
            unsafe { SetSysColors(n as u32, v.as_ptr() as u32, flat(cs) as u32) };
            0
        }
        184 | 188 | 189 => {
            0
        }
        // GrayString(hdc, hbr, proc, data, n, x, y, cx, cy): the text (or
        // what the procedure draws).
        185 => {
            let (hdc, br, p, data, n, x, y, cx, cy) = (a.w(), a.w(), a.l(), a.l(), a.i(), a.i(), a.i(), a.i(), a.i());
            if p != 0 {
                return PascalCall::new().w(hdc).l(data).w(n as u16).call(p) & 0xffff;
            }
            unsafe { GrayStringA(hdc as u32, br as u32, 0, str32(data), n as u32, x as u32, y as u32, cx as u32, cy as u32) }
        }
        186 => unsafe { SwapMouseButton(a.w() as u32) },
        // GetUpdateRect(hwnd, lpRect, erase)
        190 => {
            let (h, r, e) = (a.w(), a.l(), a.w());
            if r == 0 {
                return unsafe { GetUpdateRect(h32(h), 0, e as u32) };
            }
            out_rect(r, |q| unsafe { GetUpdateRect(h32(h), q, e as u32) })
        }
        191 => {
            let (h, p) = (a.w(), a.l());
            let (x, y) = pt(p);
            unsafe { ChildWindowFromPoint(h32(h), x, y) & 0xffff }
        }
        192 => unsafe { InSendMessage() },
        // TabbedTextOut(hdc, x, y, text, n, ntabs, tabs, origin),
        // GetTabbedTextExtent(hdc, text, n, ntabs, tabs)
        196 => {
            let (hdc, x, y, s, n, nt, t, org) = (a.w(), a.i(), a.i(), a.l(), a.i(), a.i(), a.l(), a.i());
            let tabs = ints(t, nt.max(0) as usize);
            unsafe { TabbedTextOutA(hdc as u32, x as u32, y as u32, str32(s), n as u32, nt as u32, if tabs.is_empty() { 0 } else { tabs.as_ptr() as u32 }, org as u32) }
        }
        197 => {
            let (hdc, s, n, nt, t) = (a.w(), a.l(), a.i(), a.i(), a.l());
            let tabs = ints(t, nt.max(0) as usize);
            unsafe { GetTabbedTextExtentA(hdc as u32, str32(s), n as u32, nt as u32, if tabs.is_empty() { 0 } else { tabs.as_ptr() as u32 }) }
        }
        // ScrollDC(hdc, dx, dy, lprcScroll, lprcClip, hrgnUpdate,
        // lprcUpdate)
        221 => {
            let (hdc, dx, dy, s, c, rgn, u) = (a.w(), a.i(), a.i(), a.l(), a.l(), a.w(), a.l());
            let (s, c) = (rect32(s), rect32(c));
            let sp = s.as_ref().map_or(0, |x| x.as_ptr() as u32);
            let cp = c.as_ref().map_or(0, |x| x.as_ptr() as u32);
            if u == 0 {
                return unsafe { ScrollDC(hdc as u32, dx as u32, dy as u32, sp, cp, rgn as u32, 0) };
            }
            out_rect(u, |q| unsafe { ScrollDC(hdc as u32, dx as u32, dy as u32, sp, cp, rgn as u32, q) })
        }
        222 => {
            let p = a.l();
            unsafe { GetKeyboardState(str32(p)) };
            0
        }
        223 => {
            let p = a.l();
            unsafe { SetKeyboardState(str32(p)) };
            0
        }
        // SwitchToThisWindow(hwnd, fRestore) (undocumented)
        172 => {
            let (h, restore) = (a.w(), a.w());
            let iconic = unsafe { IsIconic(h32(h)) } != 0;
            unsafe { ShowWindow(h32(h), if restore != 0 && iconic { 9 } else { 5 }) };
            0
        }
        // RepaintScreen (undocumented): every window paints again.
        275 => {
            unsafe { crate::pm::WinInvalidateRect(crate::pm::HWND_DESKTOP, core::ptr::null(), 1) };
            0
        }
        // SetEventHook(lpfn) (undocumented): kept, not called.
        321 => {
            let f = a.l();
            core::mem::replace(EVENT_HOOK.get(), f)
        }
        // GetWindowTask(hwnd) (another task's window, a proxy: that
        // task's handle).
        224 => {
            let h = a.w();
            match crate::dde::foreign(h) {
                Some(pm) if !exists(h) => crate::toolhelp::task_of_pid(crate::dde::window_pid(pm)) as u32,
                _ => crate::toolhelp::current_task() as u32,
            }
        }
        227 | 228 => {
            let (h, c, prev) = (a.w(), a.w(), a.w());
            unsafe { (if ord == 227 { GetNextDlgGroupItem(h32(h), c as u32, prev as u32) } else { GetNextDlgTabItem(h32(h), c as u32, prev as u32) }) & 0xffff }
        }
        229 => unsafe { GetTopWindow(h32(a.w())) & 0xffff },
        230 | 262 => {
            let (h, c) = (a.w(), a.w());
            unsafe { GetWindow(h32(h), c as u32) & 0xffff }
        }
        231 => 0,
        // SetWindowPos(hwnd, after, x, y, cx, cy, flags)
        232 => {
            let (h, after, x, y, cx, cy, fl) = (a.w(), a.w(), a.i(), a.i(), a.i(), a.i(), a.w());
            let after = if after >= 0xfffe { sx(after) } else { after as u32 };
            unsafe { SetWindowPos(h32(h), after, x as u32, y as u32, cx as u32, cy as u32, fl as u32) }
        }
        233 => {
            let (h, p) = (a.w(), a.w());
            unsafe { SetParent(h32(h), h32(p)) & 0xffff }
        }
        236 => unsafe { GetCapture() & 0xffff },
        237 => {
            let (h, rgn, e) = (a.w(), a.w(), a.w());
            unsafe { GetUpdateRgn(h32(h), rgn as u32, e as u32) }
        }
        238 => {
            let (hdc, h) = (a.w(), a.w());
            unsafe { ExcludeUpdateRgn(hdc as u32, h32(h)) }
        }
        243 => unsafe { GetDialogBaseUnits() },
        244 => {
            let (x, y) = (a.l(), a.l());
            (read_rect(x) == read_rect(y)) as u32
        }
        247 => unsafe { GetCursor() & 0xffff },
        // MapWindowPoints(from, to, points, n)
        258 => {
            let (from, to, p, n) = (a.w(), a.w(), a.l(), a.w());
            with_points(p, n as usize, |q| unsafe { MapWindowPoints(h32(from), h32(to), q, n as u32) })
        }
        259 => unsafe { BeginDeferWindowPos(a.i() as u32) & 0xffff },
        260 => {
            let (d, h, after, x, y, cx, cy, fl) = (a.w(), a.w(), a.w(), a.i(), a.i(), a.i(), a.i(), a.w());
            let after = if after >= 0xfffe { sx(after) } else { after as u32 };
            unsafe { DeferWindowPos(d as u32, h32(h), after, x as u32, y as u32, cx as u32, cy as u32, fl as u32) & 0xffff }
        }
        261 => unsafe { EndDeferWindowPos(a.w() as u32) },
        265 => {
            let (h, on) = (a.w(), a.w());
            unsafe { ShowOwnedPopups(h32(h), on as u32) };
            0
        }
        266 => {
            a.w();
            1
        }
        267 => {
            let (h, bar, on) = (a.w(), a.w(), a.w());
            unsafe { ShowScrollBar(h32(h), bar as u32, on as u32) };
            0
        }
        482 => {
            let (h, bar, fl) = (a.w(), a.w(), a.w());
            unsafe { EnableScrollBar(h32(h), bar as u32, fl as u32) }
        }
        277 => unsafe { GetDlgCtrlID(h32(a.w())) & 0xffff },
        278 | 286 => 0,
        282 => {
            let (hdc, pal, bg) = (a.w(), a.w(), a.w());
            unsafe { SelectPalette(hdc as u32, pal as u32, bg as u32) & 0xffff }
        }
        283 => unsafe { RealizePalette(a.w() as u32) },
        287 => unsafe { GetLastActivePopup(h32(a.w())) & 0xffff },
        288 => unsafe { GetMessageExtraInfo() },
        // RedrawWindow(hwnd, lpRect, hrgn, flags)
        290 => {
            let (h, r, rgn, fl) = (a.w(), a.l(), a.w(), a.w());
            let r = rect32(r);
            let rp = r.as_ref().map_or(0, |x| x.as_ptr() as u32);
            unsafe { RedrawWindow(h32(h), rp, rgn as u32, fl as u32) }
        }
        294 => unsafe { LockWindowUpdate(h32(a.w())) },
        // DefDlgProc(hwnd, msg, wParam, lParam)
        308 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            call_native(2308, h, m, wp, lp)
        }
        334 => unsafe { GetQueueStatus(a.w() as u32) },
        335 => unsafe { GetInputState() },
        359 => {
            let (h, rgn, fl) = (a.w(), a.w(), a.l());
            unsafe { GetDCEx(h32(h), rgn as u32, fl) & 0xffff }
        }
        // CopyIcon(hinst, hicon), CopyCursor(hinst, hcursor)
        368 | 369 => {
            let (_hi, i) = (a.w(), a.w());
            unsafe { CopyIcon(i as u32) & 0xffff }
        }
        // GetWindowPlacement(hwnd, lpwndpl), SetWindowPlacement:
        // WINDOWPLACEMENT16 (length, flags, showCmd, ptMinPosition,
        // ptMaxPosition, rcNormalPosition: 22 bytes) as 32-bit (44).
        370 | 371 => {
            let (h, p) = (a.w(), a.p());
            if p.is_null() {
                return 0;
            }
            let rd = |o: usize| unsafe { (p.add(o) as *const u16).read_unaligned() };
            let mut w = [0u32; 11];
            w[0] = 44;
            if ord == 371 {
                w[1] = rd(2) as u32;
                w[2] = rd(4) as u32;
                for i in 0..8 {
                    w[3 + i] = sx(rd(6 + 2 * i));
                }
            }
            let r = unsafe { if ord == 370 { GetWindowPlacement(h32(h), w.as_mut_ptr() as u32) } else { SetWindowPlacement(h32(h), w.as_ptr() as u32) } };
            if ord == 370 {
                let wr = |o: usize, v: u32| unsafe { (p.add(o) as *mut u16).write_unaligned(v as u16) };
                wr(0, 22);
                wr(2, w[1]);
                wr(4, w[2]);
                for i in 0..8 {
                    wr(6 + 2 * i, w[3 + i]);
                }
            }
            r
        }
        // SubtractRect(dst, a, b)
        373 => {
            let (d, s1, s2) = (a.l(), a.l(), a.l());
            let (Some(x), Some(y)) = (read_rect(s1), read_rect(s2)) else { return 0 };
            let mut r = x;
            if y[1] <= x[1] && y[3] >= x[3] {
                if y[0] <= x[0] && y[2] > x[0] {
                    r[0] = y[2];
                }
                if y[2] >= x[2] && y[0] < x[2] {
                    r[2] = y[0];
                }
            }
            if y[0] <= x[0] && y[2] >= x[2] {
                if y[1] <= x[1] && y[3] > x[1] {
                    r[1] = y[3];
                }
                if y[3] >= x[3] && y[1] < x[3] {
                    r[3] = y[1];
                }
            }
            let empty = r[2] <= r[0] || r[3] <= r[1];
            write_rect(d, if empty { [0; 4] } else { r });
            !empty as u32
        }
        403 => {
            let (n, hi) = (a.l(), a.w());
            unsafe { UnregisterClassA(str32(n), class_hinst32(hi)) }
        }
        404 => {
            let (hi, n, p) = (a.w(), a.l(), a.p());
            class_info(hi, n, p)
        }
        // CreateCursor(hinst, x, y, w, h, and, xor): a monochrome cursor
        // (the AND mask over the XOR mask, double height).
        406 => {
            let (_hi, x, y, w, h, and, xor) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.l(), a.l());
            let row = ((w + 15) / 16 * 2) as usize;
            let n = row * h.max(0) as usize;
            let mut bits = alloc::vec![0u8; 2 * n];
            unsafe {
                if !flat(and).is_null() {
                    core::ptr::copy_nonoverlapping(flat(and), bits.as_mut_ptr(), n);
                }
                if !flat(xor).is_null() {
                    core::ptr::copy_nonoverlapping(flat(xor), bits.as_mut_ptr().add(n), n);
                }
            }
            let mask = unsafe { CreateBitmap(w as u32, 2 * h as u32, 1, 1, bits.as_ptr() as u32) };
            let ii = [0u32, x as u32, y as u32, mask, 0];
            let c = unsafe { CreateIconIndirect(ii.as_ptr() as u32) };
            unsafe { DeleteObject(mask) };
            c & 0xffff
        }
        407 => {
            let (hi, w, h, planes, bpp, and, xor) = (a.w(), a.i(), a.i(), a.w(), a.w(), a.l(), a.l());
            unsafe { CreateIcon(hinst32(hi), w as u32, h as u32, planes as u32, bpp as u32, str32(and), str32(xor)) & 0xffff }
        }
        457 => unsafe { DestroyIcon(a.w() as u32) },
        458 => unsafe { DestroyCursor(a.w() as u32) },
        // DefFrameProc(hwnd, hwndMDIClient, msg, wParam, lParam)
        445 => {
            let (h, mc, m, wp, lp) = (a.w(), a.w(), a.w(), a.w(), a.l());
            msg16::to32(h, m, wp, lp, K_NONE, &mut |m, wp, lp| unsafe { DefFrameProcA(h32(h), h32(mc), m, wp, lp) })
        }
        // DefMDIChildProc(hwnd, msg, wParam, lParam)
        447 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            call_native(2447, h, m, wp, lp)
        }
        // TranslateMDISysAccel(hwndClient, lpMsg)
        451 => {
            let (mc, p) = (a.w(), a.p());
            if p.is_null() {
                return 0;
            }
            let m = msg32(p);
            unsafe { TranslateMDISysAccel(h32(mc), m.as_ptr() as u32) }
        }
        // ArrangeIconicWindows(hwnd)
        170 => unsafe { ArrangeIconicWindows(h32(a.w())) },
        // CreateWindowEx(exstyle, class, title, style, x, y, cx, cy,
        // parent, menu, instance, param)
        452 => {
            let (ex, class, title, style) = (a.l(), a.l(), a.l(), a.l());
            let (x, y, cx, cy) = (a.i(), a.i(), a.i(), a.i());
            let (parent, menu, hinst, param) = (a.w(), a.w(), a.w(), a.l());
            create_window(ex, class, title, style, [x, y, cx, cy], parent, menu, hinst, param) as u32
        }
        // DrawFocusRect(hdc, lpRect)
        466 => {
            let (hdc, r) = (a.w(), a.l());
            let Some(r) = rect32(r) else { return 0 };
            unsafe { DrawFocusRect(hdc as u32, r.as_ptr() as u32) };
            0
        }
        _ => crate::misc::call(ord, a),
    }
}

/// ExtractIcon(hinst, file, index): the index-th icon of a program or DLL
/// (-1: how many it has).
pub fn extract_icon(path: &[u8], index: u16) -> u16 {
    let Some(file) = crate::module::read_file(path, usize::MAX) else { return 0 };
    let groups = crate::res::list_type(&file, 14);
    if index == 0xffff {
        return groups.len() as u16;
    }
    let Some(&(off, len)) = groups.get(index as usize) else { return 0 };
    let id = unsafe { LookupIconIdFromDirectoryEx(file[off..off + len].as_ptr() as u32, 1, 0, 0, 0) } as u16;
    let Some((o, l)) = crate::res::find_in_file(&file, 3, &crate::res::Name::Id(id)) else { return 0 };
    unsafe { CreateIconFromResourceEx(file[o..o + l].as_ptr() as u32, l as u32, 1, 0x30000, 0, 0, 0) as u16 }
}
