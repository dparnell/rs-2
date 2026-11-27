//! Hooks (SetWindowsHookEx): WH_MSGFILTER in the dialog loop,
//! WH_KEYBOARD and WH_GETMESSAGE as GetMessage takes a message,
//! WH_CALLWNDPROC on SendMessage (WIN16's, kept for both kinds of
//! program; a 16-bit hook procedure is called through the host). The
//! other kinds are kept and not called. Hooks are the thread's (or the
//! process's, with no thread given); system-wide hooks across processes
//! are WIN16's own (it forwards its tasks' messages).

use alloc::vec::Vec;

use crate::wnd::{far16, Msg};
use crate::{tid, Global};

pub const WH_MSGFILTER: i32 = -1;
pub const WH_KEYBOARD: i32 = 2;
pub const WH_GETMESSAGE: i32 = 3;
pub const WH_CALLWNDPROC: i32 = 4;

struct Hook {
    h: u32,
    kind: i32,
    proc_: u32,
    /// The thread it watches (0: every thread of the process).
    tid: u32,
}

static HOOKS: Global<Vec<Hook>> = Global::new(Vec::new());
static NEXT: Global<u32> = Global::new(0x0e00_0001);

/// SetWindowsHookEx: a hook of `kind` (the newest first in its chain).
pub fn set(kind: i32, proc_: u32, t: u32) -> u32 {
    if proc_ == 0 || !(-1..=14).contains(&kind) {
        return crate::fail(1426, 0); // ERROR_INVALID_HOOK_FILTER
    }
    let h = *NEXT.get();
    *NEXT.get() += 1;
    HOOKS.get().push(Hook { h, kind, proc_, tid: t });
    h
}

pub fn unhook(h: u32) -> bool {
    let v = HOOKS.get();
    let Some(i) = v.iter().position(|x| x.h == h) else { return crate::fail(1404, false) };
    v.remove(i);
    true
}

/// UnhookWindowsHook(kind, proc): the old form.
pub fn unhook_proc(kind: i32, proc_: u32) -> bool {
    let h = HOOKS.get().iter().rev().find(|x| x.kind == kind && x.proc_ == proc_).map(|x| x.h);
    h.is_some_and(unhook)
}

fn applies(x: &Hook, kind: i32) -> bool {
    x.kind == kind && (x.tid == 0 || x.tid == tid())
}

pub fn has(kind: i32) -> bool {
    HOOKS.get().iter().any(|x| applies(x, kind))
}

fn call_proc(kind: i32, p: u32, code: i32, wp: u32, lp: u32) -> u32 {
    if let Some(f) = far16(p) {
        return crate::kern::host().map_or(0, |h| unsafe { (h.call16)(1, f, kind as u32, code as u32, wp, lp) });
    }
    crate::call4_3(p, code as u32, wp, lp)
}

/// Call the newest hook of a kind; it passes the call on with
/// CallNextHookEx.
pub fn call(kind: i32, code: i32, wp: u32, lp: u32) -> u32 {
    let Some(p) = HOOKS.get().iter().rev().find(|x| applies(x, kind)).map(|x| x.proc_) else { return 0 };
    call_proc(kind, p, code, wp, lp)
}

/// CallNextHookEx(h, code, wParam, lParam): the hook older than `h` in
/// its chain (0 at the end).
pub fn call_next(h: u32, code: i32, wp: u32, lp: u32) -> u32 {
    let next = {
        let v = HOOKS.get();
        let Some(i) = v.iter().position(|x| x.h == h) else { return 0 };
        let kind = v[i].kind;
        v[..i].iter().rev().find(|x| applies(x, kind)).map(|x| (kind, x.proc_))
    };
    next.map_or(0, |(kind, p)| call_proc(kind, p, code, wp, lp))
}

/// WH_CALLWNDPROC: the hooks see a message being sent (CWPSTRUCT:
/// lParam, wParam, message, hwnd).
pub fn sending(hwnd: u16, msg: u16, wp: u32, lp: u32) {
    if !has(WH_CALLWNDPROC) {
        return;
    }
    let cwp = [lp, wp, msg as u32, hwnd as u32];
    call(WH_CALLWNDPROC, 0, 1, cwp.as_ptr() as u32);
}

/// WH_KEYBOARD: a key message GetMessage is about to give: true if a hook
/// took it (it is dropped).
pub fn key(wp: u32, lp: u32) -> bool {
    has(WH_KEYBOARD) && call(WH_KEYBOARD, 0, wp, lp) != 0
}

/// WH_GETMESSAGE: the hooks see (and may change) the message GetMessage
/// gives (`removed`: PM_REMOVE).
pub fn got(m: &mut Msg, removed: bool) {
    if !has(WH_GETMESSAGE) {
        return;
    }
    let buf = [0u32; 7];
    let p = buf.as_ptr() as u32;
    m.write(p);
    call(WH_GETMESSAGE, 0, removed as u32, p);
    if let Some(n) = Msg::read(p) {
        *m = n;
    }
}

/// WH_MSGFILTER (CallMsgFilter): a message of a dialog's or menu's loop
/// (`code`: MSGF_DIALOGBOX 0, MSGF_MENU 2): true if a hook took it.
pub fn filter(m: &Msg, code: i32) -> bool {
    if !has(WH_MSGFILTER) {
        return false;
    }
    let buf = [0u32; 7];
    m.write(buf.as_ptr() as u32);
    call(WH_MSGFILTER, code, 0, buf.as_ptr() as u32) != 0
}
