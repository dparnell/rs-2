//! The Workplace Shell, the switch list and shutdown: PMWIN, PMSHAPI and
//! PMWP entry points found at run time (as IBM's REXXUTIL does, so that
//! programs not using them never load PM).

use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicUsize, Ordering};

use os2::*;

use crate::api::*;
use crate::rx::*;

/// The address of `module`'s entry `ord`, loading the module if needed.
fn proc(module: &[u8], ord: u32, slot: &AtomicUsize) -> Option<usize> {
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return Some(p);
    }
    let mut h = 0u32;
    if unsafe { DosQueryModuleHandle(module.as_ptr(), &mut h) } != 0 {
        let mut fail = [0u8; 16];
        if unsafe { DosLoadModule(fail.as_mut_ptr(), 16, module.as_ptr(), &mut h) } != 0 {
            return None;
        }
    }
    let mut p = 0usize;
    if unsafe { DosQueryProcAddr(h, ord, null(), &mut p) } != 0 || p == 0 {
        return None;
    }
    slot.store(p, Ordering::Relaxed);
    Some(p)
}

macro_rules! pm_api {
    ($($name:ident($($t:ty),*) -> $r:ty = $m:literal, $ord:literal;)*) => {$(
        pub fn $name() -> Option<unsafe extern "C" fn($($t),*) -> $r> {
            static SLOT: AtomicUsize = AtomicUsize::new(0);
            let p = proc($m, $ord, &SLOT)?;
            Some(unsafe { core::mem::transmute::<usize, unsafe extern "C" fn($($t),*) -> $r>(p) })
        }
    )*};
}

pm_api! {
    WinInitialize(u32) -> u32 = b"PMWIN\0", 763;
    WinQueryAnchorBlock(u32) -> u32 = b"PMWIN\0", 800;
    WinTerminate(u32) -> u32 = b"PMWIN\0", 888;
    WinQuerySwitchList(u32, *mut u8, u32) -> u32 = b"PMSHAPI\0", 126;
    WinSwitchToProgram(u32) -> u32 = b"PMSHAPI\0", 131;
    PrfOpenProfile(u32, *const u8) -> u32 = b"PMSHAPI\0", 102;
    PrfCloseProfile(u32) -> u32 = b"PMSHAPI\0", 103;
    PrfQueryProfile(u32, *mut u32) -> u32 = b"PMSHAPI\0", 107;
    PrfQueryProfileData(u32, *const u8, *const u8, *mut u8, *mut u32) -> u32 = b"PMSHAPI\0", 117;
    PrfWriteProfileData(u32, *const u8, *const u8, *const u8, u32) -> u32 = b"PMSHAPI\0", 118;
    WinShutdownSystem(u32, u32) -> u32 = b"PMWP\0", 149;
    WinRegisterObjectClass(*const u8, *const u8) -> u32 = b"PMWP\0", 200;
    WinDeregisterObjectClass(*const u8) -> u32 = b"PMWP\0", 201;
    WinEnumObjectClasses(*mut u8, *mut u32) -> u32 = b"PMWP\0", 205;
    WinSetFileIcon(*const u8, *const u32) -> u32 = b"PMWP\0", 210;
    WinSetObjectData(u32, *const u8) -> u32 = b"PMWP\0", 250;
    WinDestroyObject(u32) -> u32 = b"PMWP\0", 251;
    WinQueryObject(*const u8) -> u32 = b"PMWP\0", 252;
    WinCreateObject(*const u8, *const u8, *const u8, *const u8, u32) -> u32 = b"PMWP\0", 281;
    WinSaveObject(u32, u32) -> u32 = b"PMWP\0", 285;
    WinOpenObject(u32, u32, u32) -> u32 = b"PMWP\0", 286;
    WinMoveObject(u32, u32, u32) -> u32 = b"PMWP\0", 287;
    WinCopyObject(u32, u32, u32) -> u32 = b"PMWP\0", 288;
    WinCreateShadow(u32, u32, u32) -> u32 = b"PMWP\0", 289;
    WinWaitForShell(u32) -> u32 = b"PMWP\0", 512;
}

/// An anchor block: the program's own (terminated when dropped) or, when
/// it already has one, the desktop's.
pub struct Hab {
    pub hab: u32,
    term: Option<unsafe extern "C" fn(u32) -> u32>,
}

impl Hab {
    /// None if PMWIN cannot be used.
    pub fn new() -> Option<Hab> {
        let (init, query, term) = (WinInitialize()?, WinQueryAnchorBlock()?, WinTerminate()?);
        let hab = unsafe { init(0) };
        if hab != 0 {
            return Some(Hab { hab, term: Some(term) });
        }
        Some(Hab { hab: unsafe { query(1) }, term: None })
    }
}

impl Drop for Hab {
    fn drop(&mut self) {
        if let Some(t) = self.term {
            unsafe { t(self.hab) };
        }
    }
}

/// An object's handle from its name (or 0).
fn object(name: &[u8]) -> u32 {
    let n = cz(name);
    WinQueryObject().map(|f| unsafe { f(n.as_ptr()) }).unwrap_or(0)
}

/// SysCopyObject / SysMoveObject / SysCreateShadow(object, dest).
fn object_to(a: &Args, r: &mut Ret, f: Option<unsafe extern "C" fn(u32, u32, u32) -> u32>) -> u32 {
    if a.n() < 2 || a.n() > 3 {
        return INVALID;
    }
    let (Some(f), Some(_)) = (f, WinQueryObject()) else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let (obj, dest) = (object(a.cstr(0)), object(a.cstr(1)));
    let ok = obj != 0 && dest != 0 && unsafe { f(obj, dest, 0) } != 0;
    r.bool(ok);
    0
}

pub fn sys_copy_object(a: &Args, r: &mut Ret) -> u32 {
    object_to(a, r, WinCopyObject())
}

pub fn sys_move_object(a: &Args, r: &mut Ret) -> u32 {
    object_to(a, r, WinMoveObject())
}

pub fn sys_create_shadow(a: &Args, r: &mut Ret) -> u32 {
    object_to(a, r, WinCreateShadow())
}

/// SysOpenObject(object, view, flag).
pub fn sys_open_object(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 3 {
        return INVALID;
    }
    let (Some(open), Some(_)) = (WinOpenObject(), WinQueryObject()) else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let obj = object(a.cstr(0));
    let mut ok = false;
    if obj != 0 {
        let v = a.cstr(1);
        let view = if eqi(v, b"SETTINGS") {
            2
        } else if eqi(v, b"DEFAULT") {
            0
        } else if eqi(v, b"ICON") {
            1
        } else if eqi(v, b"HELP") {
            3
        } else if eqi(v, b"RUNNING") {
            4
        } else if eqi(v, b"PROMPTDLG") {
            5
        } else if eqi(v, b"PALETTE") {
            0x79
        } else if v.first().is_some_and(u8::is_ascii_digit) {
            match string2ulong(v) {
                Some(n) => n,
                None => return INVALID,
            }
        } else {
            return INVALID;
        };
        let flag = if eqi(a.cstr(2), b"FALSE") { 0 } else { 1 };
        ok = unsafe { open(obj, view, flag) } != 0;
    }
    r.bool(ok);
    0
}

/// SysSaveObject(object, async).
pub fn sys_save_object(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 {
        return INVALID;
    }
    let (Some(save), Some(_)) = (WinSaveObject(), WinQueryObject()) else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let obj = object(a.cstr(0));
    let mut ok = false;
    if obj != 0 {
        let s = a.cstr(1);
        let sync = eqi(s, b"FALSE") || eqi(s, b"0") || eqi(s, b"S");
        ok = unsafe { save(obj, if sync { 0 } else { 1 }) } != 0;
    }
    r.bool(ok);
    0
}

/// SysDestroyObject(object).
pub fn sys_destroy_object(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let (Some(destroy), Some(_)) = (WinDestroyObject(), WinQueryObject()) else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let obj = object(a.cstr(0));
    r.bool(obj != 0 && unsafe { destroy(obj) } != 0);
    0
}

/// SysSetObjectData(object, setup).
pub fn sys_set_object_data(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(0) {
        return INVALID;
    }
    let (Some(set), Some(_)) = (WinSetObjectData(), WinQueryObject()) else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let obj = object(a.cstr(0));
    let setup = cz(a.cstr(1));
    r.bool(obj != 0 && unsafe { set(obj, setup.as_ptr()) } != 0);
    0
}

/// SysCreateObject(class, title, location [, setup [, 'F'|'R'|'U']]).
pub fn sys_create_object(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 3 || a.n() > 5 || !a.valid(0) || !a.valid(1) || !a.valid(2) {
        return INVALID;
    }
    let setup = if a.n() >= 4 && a.valid(3) { cz(a.cstr(3)) } else { cz(b"") };
    let mut flags = 0;
    if a.n() == 5 {
        if a.exists(4) && a.len(4) == 0 {
            return INVALID;
        }
        flags = match upper(a.first(4)) {
            b'F' => 0,
            b'R' => 1,
            b'U' => 2,
            _ => return INVALID,
        };
    }
    let Some(create) = WinCreateObject() else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let (class, title, loc) = (cz(a.cstr(0)), cz(a.cstr(1)), cz(a.cstr(2)));
    let h = unsafe { create(class.as_ptr(), title.as_ptr(), setup.as_ptr(), loc.as_ptr(), flags) };
    r.bool(h != 0);
    0
}

/// SysRegisterObjectClass(class, module).
pub fn sys_register_object_class(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let Some(reg) = WinRegisterObjectClass() else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let (c, m) = (cz(a.cstr(0)), cz(a.cstr(1)));
    r.num(unsafe { reg(c.as_ptr(), m.as_ptr()) } as i32);
    0
}

/// SysDeregisterObjectClass(class).
pub fn sys_deregister_object_class(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 || !a.valid(0) {
        return INVALID;
    }
    let Some(dereg) = WinDeregisterObjectClass() else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let c = cz(a.cstr(0));
    r.num(unsafe { dereg(c.as_ptr()) } as i32);
    0
}

/// SysSetIcon(file, iconfile).
pub fn sys_set_icon(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let Some(set) = WinSetFileIcon() else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let (f, i) = (cz(a.cstr(0)), cz(a.cstr(1)));
    // ICONINFO: cb, fFormat (ICON_FILE), pszFileName, hmod, resid,
    // cbIconData, pIconData.
    let info: [u32; 7] = [0x1c, 1, i.as_ptr() as u32, 0, 0, 0, 0];
    r.num(unsafe { set(f.as_ptr(), info.as_ptr()) } as i32);
    0
}

/// SysQueryClassList(stem): "class module" for each registered class.
pub fn sys_query_class_list(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 || !a.valid(0) {
        return INVALID;
    }
    let stem = stem_name(a.cstr(0));
    let Some(enumc) = WinEnumObjectClasses() else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    let mut size = 0u32;
    if unsafe { enumc(null_mut(), &mut size) } == 0 {
        r.set(b"0");
        return 0;
    }
    let mut buf: *mut u8 = null_mut();
    if unsafe { DosAllocMem(&mut buf, size.max(1), PAG_COMMIT | PAG_WRITE) } != 0 {
        r.set(b"0");
        return 0;
    }
    if unsafe { enumc(buf, &mut size) } == 0 {
        unsafe { DosFreeMem(buf) };
        r.set(b"0");
        return 0;
    }
    // OBJCLASS: pNext, pszClassName, pszModName.
    let mut count = 0u32;
    let mut p = buf as *const u32;
    while !p.is_null() {
        count += 1;
        let (class, module) = unsafe { (*p.add(1) as *const u8, *p.add(2) as *const u8) };
        let mut v = Vec::new();
        v.extend_from_slice(unsafe { core::slice::from_raw_parts(class, c_len(class)) });
        v.push(b' ');
        v.extend_from_slice(unsafe { core::slice::from_raw_parts(module, c_len(module)) });
        if set_var(SHV_SET, &tail(&stem, &dec(count as i64)), &v) == SHV_BADN {
            unsafe { DosFreeMem(buf) };
            return INVALID;
        }
        p = unsafe { *p as *const u32 };
    }
    set_var(SHV_SET, &tail(&stem, b"0"), &dec(count as i64));
    unsafe { DosFreeMem(buf) };
    r.num(0);
    0
}

/// SysWaitForShell(event [, 'QUERY']): 1 when the shell has got there.
pub fn sys_wait_for_shell(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 2 || !a.valid(0) {
        return INVALID;
    }
    let e = a.cstr(0);
    let mut flags: u32 = if eqi(e, b"DESKTOPOPENED") {
        2
    } else if eqi(e, b"DESKTOPCREATED") {
        1
    } else if eqi(e, b"DESKTOPPOPULATED") {
        3
    } else {
        return INVALID;
    };
    if a.n() == 2 {
        if !eqi(a.cstr(1), b"QUERY") {
            return INVALID;
        }
        flags |= 0x8000_0000;
    }
    let Some(wait) = WinWaitForShell() else {
        return INVALID;
    };
    let Some(_hab) = Hab::new() else {
        return INVALID;
    };
    r.bool(unsafe { wait(flags) } != 0);
    0
}

/// SWENTRY: hswitch, then SWCNTRL (hwnd, hwndIcon, hprog, idProcess,
/// idSession, uchVisibility, fbJump, szSwtitle[64], bProgType).
const SWENTRY: usize = 100;

/// SysQuerySwitchList(stem [, options]): the switch list's titles ('D':
/// pid, session, visibility, jump flags, program type and title).
pub fn sys_query_switch_list(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 2 || !a.valid(0) {
        return INVALID;
    }
    let stem = stem_name(a.cstr(0));
    let (mut detail, mut vis, mut jump) = (false, 4u32, 2u32);
    if a.n() == 2 && a.exists(1) {
        if a.len(1) == 0 {
            return INVALID;
        }
        for &c in a.cstr(1) {
            match upper(c) {
                b'D' => detail = true,
                b'G' => vis |= 2,
                b'I' => vis |= 1,
                b'N' => jump |= 1,
                _ => return INVALID,
            }
        }
    }
    let Some(query) = WinQuerySwitchList() else {
        return INVALID;
    };
    let Some(hab) = Hab::new() else {
        return INVALID;
    };
    let mut buf: *mut u8 = null_mut();
    let rc = unsafe { DosAllocMem(&mut buf, 0x4000, PAG_COMMIT | PAG_WRITE) };
    if rc == 0 {
        let n = unsafe { query(hab.hab, buf, 0x4000) } as i32;
        let b = unsafe { core::slice::from_raw_parts(buf, 0x4000) };
        let w = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let mut count = 0u32;
        for i in 0..n.max(0) as usize {
            let e = 4 + i * SWENTRY;
            if e + SWENTRY > b.len() {
                break;
            }
            let (v, j) = (w(e + 4 + 20), w(e + 4 + 24));
            if v & vis == 0 || j & jump == 0 {
                continue;
            }
            count += 1;
            let title = &b[e + 4 + 28..e + 4 + 92];
            let title = &title[..title.iter().position(|&c| c == 0).unwrap_or(64)];
            let mut line = Vec::new();
            if detail {
                line.extend(dec_w(w(e + 4 + 12) as i32 as i64, 8, false));
                line.push(b' ');
                line.extend(dec_w(w(e + 4 + 16) as i32 as i64, 8, false));
                line.push(b' ');
                line.extend(dec((v & 7) as i64));
                line.push(b' ');
                line.extend(dec((j & 3) as i64));
                line.push(b' ');
                line.extend(dec_w(w(e + 4 + 92) as i32 as i64, 3, false));
                line.push(b' ');
            }
            line.extend_from_slice(title);
            if set_var(SHV_SET, &tail(&stem, &dec(count as i64)), &line) == SHV_BADN {
                unsafe { DosFreeMem(buf) };
                return INVALID;
            }
        }
        unsafe { DosFreeMem(buf) };
        set_var(SHV_SET, &tail(&stem, b"0"), &dec(count as i64));
    }
    r.num(rc as i32);
    0
}

/// SysSwitchSession(title): switch to the switch list entry with that
/// title; 0, or 4357 if there is none.
pub fn sys_switch_session(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let (Some(query), Some(switch)) = (WinQuerySwitchList(), WinSwitchToProgram()) else {
        return INVALID;
    };
    let Some(hab) = Hab::new() else {
        return INVALID;
    };
    let mut buf: *mut u8 = null_mut();
    let mut rc = unsafe { DosAllocMem(&mut buf, 0x2000, PAG_COMMIT | PAG_WRITE) };
    if rc == 0 {
        rc = 0x1105;
        let n = unsafe { query(hab.hab, buf, 0x1000) } as i32;
        let b = unsafe { core::slice::from_raw_parts(buf, 0x2000) };
        for i in 0..n.max(0) as usize {
            let e = 4 + i * SWENTRY;
            if e + SWENTRY > 0x2000 {
                break;
            }
            let title = &b[e + 32..e + 96];
            if stricmp(title, a.cstr(0)) == 0 {
                let hsw = u32::from_le_bytes([b[e], b[e + 1], b[e + 2], b[e + 3]]);
                unsafe { switch(hsw) };
                rc = 0;
                break;
            }
        }
        unsafe { DosFreeMem(buf) };
    }
    r.num(rc as i32);
    0
}

/// SysShutDownSystem(): 1 if the shutdown started.
pub fn sys_shutdown_system(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    let ok = match (WinInitialize(), WinShutdownSystem()) {
        (Some(_), Some(shut)) => {
            let Some(hab) = Hab::new() else {
                return INVALID;
            };
            unsafe { shut(hab.hab, 0) != 0 }
        }
        (Some(_), None) => return INVALID,
        _ => unsafe { DosShutdown(0) == 0 },
    };
    r.bool(ok);
    0
}
