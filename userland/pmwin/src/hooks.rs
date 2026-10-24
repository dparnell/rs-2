//! Hooks (WinSetHook), called as IBM's PMWIN calls them (studied in
//! PMMERGE: WSetHook, GetHookToCall and the Call*Hook functions).
//!
//! Each kind has one chain: the hooks of message queues (newest first),
//! then the system hooks (hmq NULLHANDLE, newest first), which need the
//! module that holds the hook procedure. A hook is called on the thread
//! whose work it hooks (the sender of a message, the thread reading its
//! queue), so a system hook runs in every PM process: its module is loaded
//! into a process the first time one of its hooks is called there (DLL
//! code has the same address in every process). The chain is read again
//! after each hook, which may set or release hooks. Hooks run without the
//! PM lock. The PM server calls none (it never waits for an application)
//! but the journal playback hook, on a thread of its own.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::lock;
use crate::state::{self, st, QMsg};

pub const HK_SENDMSG: u32 = 0;
pub const HK_INPUT: u32 = 1;
pub const HK_MSGFILTER: u32 = 2;
pub const HK_JOURNALRECORD: u32 = 3;
pub const HK_JOURNALPLAYBACK: u32 = 4;
pub const HK_HELP: u32 = 5;
pub const HK_LOADER: u32 = 6;
pub const HK_REGISTERUSERMSG: u32 = 7;
pub const HK_FINDWORD: u32 = 11;
pub const HK_CODEPAGECHANGED: u32 = 12;
pub const HK_WINDOWDC: u32 = 15;
pub const HK_DESTROYWINDOW: u32 = 16;
pub const HK_CHECKMSGFILTER: u32 = 20;
pub const HK_MSGINPUT: u32 = 21;
/// The highest kind IBM's WinSetHook takes.
const HK_LAST: u32 = 0x19;

pub const MSGF_DIALOGBOX: u32 = 1;
pub const MSGF_MESSAGEBOX: u32 = 2;
pub const MSGF_TRACK: u32 = 8;

/// QMSG.reserved of a message that came from the keyboard or mouse (the
/// journal record hook gets those as they are taken off the queue); it is
/// cleared before an application sees the message.
pub const QMSG_INPUT: u32 = 0x8000_0000;

unsafe extern "C" {
    #[link_name = "DOSCALLS.320"]
    fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> u32;
}

/// Hook modules of other processes loaded into this one (module handles
/// are the same in every process).
static LOADED: [AtomicU32; 32] = [const { AtomicU32::new(0) }; 32];

fn loaded(hmod: u32) -> bool {
    LOADED.iter().any(|m| m.load(Ordering::Relaxed) == hmod)
}

/// Load the module of another process's system hook into this process.
fn load(hmod: u32, name: &[u8]) -> bool {
    if loaded(hmod) {
        return true;
    }
    let mut z = name.to_vec();
    z.push(0);
    let mut h = 0;
    let rc = lock::unlocked(|| unsafe { os2::DosLoadModule(core::ptr::null_mut(), 0, z.as_ptr(), &mut h) });
    if rc != 0 {
        return false;
    }
    if !loaded(h) {
        if let Some(slot) = LOADED.iter().find(|m| m.load(Ordering::Relaxed) == 0) {
            slot.store(h, Ordering::Relaxed);
        }
    }
    true
}

/// WinSetHook(hab, hmq, iHook, pfnHook, hmod): a hook for one queue
/// (HMQ_CURRENT: the caller's) or, with hmq NULLHANDLE, for the whole
/// system (then hmod must name the module that holds pfnHook). Setting a
/// hook that is already there moves it to the front.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetHook(hab: u32, hmq: u32, kind: u32, pfn: u32, hmod: u32) -> u32 {
    if pfn == 0 || kind > HK_LAST {
        return 0;
    }
    let s = st();
    let queue = if hmq == 1 { s.my_queue() } else { hmq };
    if queue != 0 && !s.queues.contains_key(&queue) {
        return 0;
    }
    let mut module = Vec::new();
    if hmod != 0 {
        let mut buf = [0u8; 260];
        if unsafe { DosQueryModuleName(hmod, buf.len() as u32, buf.as_mut_ptr()) } != 0 {
            return 0;
        }
        module.extend(buf.iter().take_while(|&&b| b != 0));
    } else if queue == 0 {
        return 0;
    }
    let me = state::pid();
    s.hooks.retain(|h| !(h.queue == queue && h.kind == kind && h.pfn == pfn && h.pid == me));
    s.hooks.insert(0, state::Hook { queue, kind, pfn, hmod, pid: me, hab, module });
    if kind == HK_JOURNALPLAYBACK {
        crate::server::journal_changed();
    }
    1
}

/// WinReleaseHook(hab, hmq, iHook, pfnHook, hmod).
#[unsafe(no_mangle)]
pub extern "C" fn WinReleaseHook(_hab: u32, hmq: u32, kind: u32, pfn: u32, _hmod: u32) -> u32 {
    let s = st();
    let queue = if hmq == 1 { s.my_queue() } else { hmq };
    let me = state::pid();
    let n = s.hooks.len();
    if let Some(i) = s.hooks.iter().position(|h| h.queue == queue && h.kind == kind && h.pfn == pfn && h.pid == me) {
        s.hooks.remove(i);
    }
    if kind == HK_JOURNALPLAYBACK {
        crate::server::journal_changed();
    }
    (s.hooks.len() != n) as u32
}

/// Some hook of this kind is set (the cheap test before building a
/// hook's arguments).
pub fn any(kind: u32) -> bool {
    let s = st();
    !s.hooks.is_empty() && s.hooks.iter().any(|h| h.kind == kind)
}

/// The next hook of `kind` for the calling thread's queue that has not
/// been called yet: (pfn, installer's hab, queue). Queue hooks come
/// before system hooks; another process's system hook has its module
/// loaded.
fn next(kind: u32, called: &[(u32, u32)], journal: bool) -> Option<(u32, u32, u32)> {
    loop {
        let s = st();
        if s.is_server() && !journal {
            return None;
        }
        let q = s.my_queue();
        let me = state::pid();
        let pick = |system: bool| {
            s.hooks.iter().find(|h| {
                h.kind == kind
                    && if system { h.queue == 0 } else { h.queue == q && q != 0 }
                    && !called.contains(&(h.pfn, h.queue))
                    && (h.pid == me || h.hmod != 0)
            })
        };
        let h = pick(false).or_else(|| pick(true))?;
        let (pfn, hab, queue, hmod) = (h.pfn, h.hab, h.queue, h.hmod);
        if h.pid == me || loaded(hmod) {
            return Some((pfn, hab, queue));
        }
        let name = h.module.clone();
        if !load(hmod, &name) {
            // The module does not load here: no hooks of it in this
            // process.
            return None;
        }
    }
}

/// Call the hooks of `kind` one after the other, `f(pfn, hab)` each
/// (without the PM lock), until one returns non-zero when `until_true`;
/// returns the last result.
pub fn call(kind: u32, until_true: bool, mut f: impl FnMut(u32, u32) -> u32) -> u32 {
    call_ex(kind, until_true, false, &mut f)
}

fn call_ex(kind: u32, until_true: bool, journal: bool, f: &mut dyn FnMut(u32, u32) -> u32) -> u32 {
    if !any(kind) {
        return 0;
    }
    let mut called: Vec<(u32, u32)> = Vec::new();
    let mut r = 0;
    while let Some((pfn, hab, queue)) = next(kind, &called, journal) {
        called.push((pfn, queue));
        r = lock::unlocked(|| f(pfn, hab));
        if until_true && r != 0 {
            break;
        }
    }
    r
}

/// The first hook of `kind` only (journal playback, message input: one
/// source of input at a time).
fn first(kind: u32, journal: bool) -> Option<(u32, u32)> {
    if !any(kind) {
        return None;
    }
    next(kind, &[], journal).map(|(pfn, hab, _)| (pfn, hab))
}

type Fn2 = extern "C" fn(u32, u32) -> u32;
type Fn3 = extern "C" fn(u32, u32, u32) -> u32;
type Fn4 = extern "C" fn(u32, u32, u32, u32) -> u32;
type Fn5 = extern "C" fn(u32, u32, u32, u32, u32) -> u32;
type Fn7 = extern "C" fn(u32, u32, u32, u32, u32, u32, u32) -> u32;

fn f2(p: u32) -> Fn2 {
    unsafe { core::mem::transmute(p as usize) }
}
fn f3(p: u32) -> Fn3 {
    unsafe { core::mem::transmute(p as usize) }
}
fn f4(p: u32) -> Fn4 {
    unsafe { core::mem::transmute(p as usize) }
}
fn f5(p: u32) -> Fn5 {
    unsafe { core::mem::transmute(p as usize) }
}
fn f7(p: u32) -> Fn7 {
    unsafe { core::mem::transmute(p as usize) }
}

/// Hooks return BOOL; 16-bit ones leave the high word undefined.
fn bool16(r: u32) -> u32 {
    (r & 0xffff != 0) as u32
}

/// SMHSTRUCT: mp2, mp1, msg, hwnd, model (PM_MODEL_2X).
#[repr(C)]
struct Smh {
    mp2: u32,
    mp1: u32,
    msg: u32,
    hwnd: u32,
    model: u32,
}

/// HK_SENDMSG, in the sender before the message goes anywhere
/// (SendMsgHook(hab, psmh, fInterTask)); the hooks may change the
/// message.
pub fn send_msg(h: &mut u32, msg: &mut u32, mp1: &mut u32, mp2: &mut u32) {
    if !any(HK_SENDMSG) {
        return;
    }
    let s = st();
    let inter = (s.queue_of(*h) != s.my_queue()) as u32;
    let mut smh = Smh { mp2: *mp2, mp1: *mp1, msg: *msg, hwnd: *h, model: 1 };
    let p = &mut smh as *mut Smh as u32;
    call(HK_SENDMSG, false, |pfn, hab| {
        f3(pfn)(hab, p, inter);
        0
    });
    (*h, *msg, *mp1, *mp2) = (smh.hwnd, smh.msg, smh.mp1, smh.mp2);
}

/// HK_INPUT, as a message is retrieved (InputHook(hab, pqmsg, fs),
/// fs PM_REMOVE or PM_NOREMOVE): TRUE means the hook took the message.
pub fn input(m: &mut QMsg, remove: bool) -> bool {
    if !any(HK_INPUT) {
        return false;
    }
    let p = m as *mut QMsg as u32;
    bool16(call(HK_INPUT, true, |pfn, hab| bool16(f3(pfn)(hab, p, remove as u32)))) != 0
}

/// HK_MSGFILTER (MsgFilterHook(hab, pqmsg, msgf)): TRUE means the hook
/// processed the message.
pub fn msg_filter(m: *mut QMsg, msgf: u32) -> bool {
    if !any(HK_MSGFILTER) {
        return false;
    }
    let p = m as u32;
    bool16(call(HK_MSGFILTER, true, |pfn, hab| bool16(f3(pfn)(hab, p, msgf)))) != 0
}

/// WinCallMsgFilter(hab, pqmsg, msgf): the message filter hooks of the
/// caller's queue and the system's; TRUE if one processed the message.
#[unsafe(no_mangle)]
pub extern "C" fn WinCallMsgFilter(_hab: u32, pq: *mut QMsg, msgf: u32) -> u32 {
    if pq.is_null() {
        return 0;
    }
    msg_filter(pq, msgf) as u32
}

/// HK_JOURNALRECORD, as a keyboard or mouse message is taken off the
/// queue (JournalRecordHook(hab, pqmsg)).
pub fn journal_record(m: &QMsg) {
    if !any(HK_JOURNALRECORD) {
        return;
    }
    let mut copy = *m;
    let p = &mut copy as *mut QMsg as u32;
    call(HK_JOURNALRECORD, false, |pfn, hab| {
        f2(pfn)(hab, p);
        0
    });
}

/// HK_JOURNALPLAYBACK (JournalPlaybackHook(hab, fSkip, pqmsg)): the
/// playback hook in effect for the system, if any (PM server only).
pub fn playback_hook() -> Option<(u32, u32)> {
    first(HK_JOURNALPLAYBACK, true)
}

pub fn playback(pfn: u32, hab: u32, skip: bool, m: *mut QMsg) -> u32 {
    f3(pfn)(hab, skip as u32, m as u32)
}

/// HK_MSGINPUT (MsgInputHook(hab, pqmsg, fSkip, pfNoRecord)): a source
/// of input messages before the queue's own (pen input). Returns the
/// message it gives.
pub fn msg_input(skip: bool) -> Option<QMsg> {
    let (pfn, hab) = first(HK_MSGINPUT, false)?;
    let mut m = QMsg::default();
    let mut no_record = 0u32;
    let pm = if skip { 0 } else { &mut m as *mut QMsg as u32 };
    let r = lock::unlocked(|| f4(pfn)(hab, pm, skip as u32, &mut no_record as *mut u32 as u32));
    (bool16(r) != 0 && !skip).then_some(m)
}

/// HK_CHECKMSGFILTER (CheckMsgFilterHook(hab, pqmsg, first, last, fl)):
/// whether a message the filter range would take is skipped instead.
pub fn check_filter(m: &QMsg, first: u32, last: u32, fl: u32) -> Option<bool> {
    if !any(HK_CHECKMSGFILTER) {
        return None;
    }
    let mut copy = *m;
    let p = &mut copy as *mut QMsg as u32;
    Some(bool16(call(HK_CHECKMSGFILTER, true, |pfn, hab| bool16(f5(pfn)(hab, p, first, last, fl)))) != 0)
}

/// HK_WINDOWDC (WindowDCHook(hab, hdc, hwnd, fAssociate)): a cache
/// presentation space of a window is handed out or taken back.
pub fn window_dc(hdc: u32, h: u32, assoc: bool) {
    if !any(HK_WINDOWDC) {
        return;
    }
    call(HK_WINDOWDC, true, |pfn, hab| bool16(f4(pfn)(hab, hdc, h, assoc as u32)));
}

/// HK_DESTROYWINDOW (DestroyWindowHook(hab, hwnd, 0)), before the window
/// gets WM_DESTROY.
pub fn destroy_window(h: u32) {
    if !any(HK_DESTROYWINDOW) {
        return;
    }
    call(HK_DESTROYWINDOW, false, |pfn, hab| {
        f3(pfn)(hab, h, 0);
        0
    });
}

/// HK_CODEPAGECHANGED (CodePageChangedHook(hmq, cpOld, cpNew)).
pub fn codepage_changed(hmq: u32, old: u32, new: u32) {
    if !any(HK_CODEPAGECHANGED) {
        return;
    }
    call(HK_CODEPAGECHANGED, false, |pfn, _| {
        f3(pfn)(hmq, old, new);
        0
    });
}

/// HK_FINDWORD (FindWordHook(cp, pszText, cb, ich, pichStart, pichEnd,
/// pichNext)): where a hook breaks a line near `ich`: (start, end, next)
/// of the word there.
pub fn find_word(cp: u32, text: &[u8], ich: u32) -> Option<(u32, u32, u32)> {
    if !any(HK_FINDWORD) {
        return None;
    }
    let mut z = text.to_vec();
    z.push(0);
    let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
    let pz = z.as_ptr() as u32;
    let (pa, pb, pc) = (&mut a as *mut u32 as u32, &mut b as *mut u32 as u32, &mut c as *mut u32 as u32);
    let r = call(HK_FINDWORD, true, |pfn, _| bool16(f7(pfn)(cp, pz, text.len() as u32, ich, pa, pb, pc)));
    (r != 0).then_some((a, b, c))
}

/// HK_LOADER (LoaderHook(hab, idContext, pszLibname, phlib, pszProcname,
/// wndProc, pfSuccess)): TRUE if a hook did the work (its *pfSuccess and
/// *phlib are the result).
pub fn loader(ctx: u32, lib: *const u8, phlib: *mut u32, proc_: *const u8, wndproc: u32, ok: *mut u32) -> bool {
    if !any(HK_LOADER) {
        return false;
    }
    let (a, b, c, d) = (lib as u32, phlib as u32, proc_ as u32, ok as u32);
    bool16(call(HK_LOADER, true, |pfn, hab| bool16(f7(pfn)(hab, ctx, a, b, c, wndproc, d)))) != 0
}

/// The help hooks (HelpHook(hab, usMode, idTopic, idSubTopic,
/// prcPosition)) until one returns TRUE.
pub fn help(mode: u32, topic: u32, sub: u32, rect: *const crate::gpiapi::RectL) -> bool {
    let p = rect as u32;
    bool16(call(HK_HELP, true, |pfn, hab| bool16(f5(pfn)(hab, mode, topic, sub, p)))) != 0
}

/// The calling process ended: its hooks go (the PM server, for a process
/// that died).
pub fn process_gone(p: u32) {
    st().hooks.retain(|h| h.pid != p);
}

// ---- Calls that exist for their hooks ----

const LHK_DELETEPROC: u32 = 1;
const LHK_DELETELIB: u32 = 2;
const LHK_LOADPROC: u32 = 3;
const LHK_LOADLIB: u32 = 4;

/// WinLoadLibrary(hab, pszLibname): a loader hook may load it instead of
/// DosLoadModule.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadLibrary(_hab: u32, name: *const u8) -> u32 {
    if name.is_null() {
        return 0;
    }
    let (mut hlib, mut ok) = (0u32, 0u32);
    if loader(LHK_LOADLIB, name, &mut hlib, core::ptr::null(), 0, &mut ok) {
        return hlib;
    }
    let rc = lock::unlocked(|| unsafe { os2::DosLoadModule(core::ptr::null_mut(), 0, name, &mut hlib) });
    if rc == 0 { hlib } else { 0 }
}

/// WinDeleteLibrary(hab, hlib).
#[unsafe(no_mangle)]
pub extern "C" fn WinDeleteLibrary(_hab: u32, hlib: u32) -> u32 {
    let (mut h, mut ok) = (hlib, 0u32);
    if loader(LHK_DELETELIB, core::ptr::null(), &mut h, core::ptr::null(), 0, &mut ok) {
        return ok;
    }
    (lock::unlocked(|| unsafe { os2::DosFreeModule(hlib) }) == 0) as u32
}

/// WinLoadProcedure(hab, hlib, pszProcname): a window procedure of a
/// library.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadProcedure(_hab: u32, hlib: u32, name: *const u8) -> u32 {
    if name.is_null() {
        return 0;
    }
    let (mut h, mut ok) = (hlib, 0u32);
    if loader(LHK_LOADPROC, core::ptr::null(), &mut h, name, 0, &mut ok) {
        return if ok != 0 { h } else { 0 };
    }
    let mut pfn = 0usize;
    if unsafe { os2::DosQueryProcAddr(hlib, 0, name, &mut pfn) } == 0 { pfn as u32 } else { 0 }
}

/// WinDeleteProcedure(hab, pfnwp): nothing to free but for a hook.
#[unsafe(no_mangle)]
pub extern "C" fn WinDeleteProcedure(_hab: u32, pfn: u32) -> u32 {
    let (mut h, mut ok) = (0u32, 0u32);
    if loader(LHK_DELETEPROC, core::ptr::null(), &mut h, core::ptr::null(), pfn, &mut ok) {
        return ok;
    }
    (pfn != 0) as u32
}

const RUMHK_DATATYPE: u32 = 1;
const RUMHK_MSG: u32 = 2;

/// The register user message hook (RegisterUserHook(hab, idContext, msg,
/// type1, dir1, type2, dir2, typer, cUshort, arRMP, pfRegistered)): if one
/// answers, its *pfRegistered is the result.
fn reg_user(a: [u32; 10]) -> u32 {
    let Some((pfn, hab)) = first(HK_REGISTERUSERMSG, false) else { return 1 };
    let mut reg = 0u32;
    let pr = &mut reg as *mut u32 as u32;
    type F = extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32;
    let f: F = unsafe { core::mem::transmute(pfn as usize) };
    let r = lock::unlocked(|| f(hab, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], pr));
    if bool16(r) != 0 { reg } else { 1 }
}

/// WinRegisterUserDatatype(hab, lDatatype, lCount, plTypes): message
/// parameter types matter only to a hook (rs2 passes messages between
/// processes as they are).
#[unsafe(no_mangle)]
pub extern "C" fn WinRegisterUserDatatype(_hab: u32, ty: u32, count: u32, types: u32) -> u32 {
    reg_user([RUMHK_DATATYPE, 0, ty, 0, 0, 0, 0, count, types, 0])
}

/// WinRegisterUserMsg(hab, msgid, lType1, lDir1, lType2, lDir2, lTyper).
#[unsafe(no_mangle)]
pub extern "C" fn WinRegisterUserMsg(_hab: u32, msg: u32, t1: u32, d1: u32, t2: u32, d2: u32, tr: u32) -> u32 {
    reg_user([RUMHK_MSG, msg, t1, d1, t2, d2, tr, 0, 0, 0])
}
