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

/// Call the hooks of one type (newest first, as Windows chains them)
/// until one returns non-zero.
pub fn call_hooks(kind: i16, code: i16, wp: u16, lp: u32) -> u32 {
    let procs: Vec<u32> = HOOKS.get().iter().rev().filter(|h| h.kind == kind).map(|h| h.proc_).collect();
    for p in procs {
        let r = PascalCall::new().w(code as u16).w(wp).l(lp).call(p);
        if r != 0 {
            return r;
        }
    }
    0
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

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
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
            if ord == 2291 {
                a.w();
                a.w();
            }
            let n = NEXT_HOOK.get();
            let h = *n;
            *n += 1;
            HOOKS.get().push(Hook { h, kind, proc_: p });
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
        // DefHookProc(code, wp, lp, lpfnNext), CallNextHookEx(hhook, code,
        // wp, lp): the chain is called by WIN16 itself.
        2235 => {
            let _ = (a.w(), a.w(), a.l(), a.l());
            0
        }
        2293 => {
            let _ = (a.l(), a.w(), a.w(), a.l());
            0
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
