//! USER32's entry points, on wnd.rs (WIN16's window manager in 32-bit
//! form) and the modules around it. HWNDs, HMENUs and HICONs are small
//! numbers; a handle above 0xFFFF is no window.

use alloc::vec::Vec;

use crate::gdi;
use crate::pm::*;
use crate::res::Name;
use crate::wnd::{self, hw, wnd, ClassName, Msg, NewClass};
use crate::{far_str, read_rect, rd32, wide_str, wr32, write_point, write_rect};


/// A string argument (A or W) in the process code page; None for an ID.
fn str_arg(p: u32, wide: bool) -> Option<Vec<u8>> {
    if wide { wide_str(p) } else { far_str(p).map(|s| s.to_vec()) }
}

unsafe extern "C" {
    fn __w32t_DefWindowProcA();
    fn __w32t_DefWindowProcW();
    fn __w32t_DefDlgProcA();
    fn __w32t_Rs2ScrollBarProc();
    fn __w32t_Rs2ButtonProc();
    fn __w32t_Rs2StaticProc();
    fn __w32t_Rs2EditProc();
    fn __w32t_Rs2ListBoxProc();
    fn __w32t_Rs2ComboBoxProc();
}

/// The address programs see of an exported procedure (its entry thunk).
pub fn thunk(f: unsafe extern "C" fn()) -> u32 {
    f as *const () as usize as u32
}

/// DefDlgProcA's entry.
pub fn def_dlg_a() -> unsafe extern "C" fn() {
    __w32t_DefDlgProcA
}

/// The built-in classes and procedures: (name, atom, procedure's entry,
/// the procedure, class style, extra bytes, cursor, brush).
pub fn register_builtin() {
    wnd::add_native(thunk(__w32t_DefWindowProcA), wnd::def_window_proc);
    wnd::add_native(thunk(__w32t_DefWindowProcW), def_window_proc_w);
    let dbl = wnd::CS_DBLCLKS | wnd::CS_PARENTDC;
    let classes: [(&[u8], u16, unsafe extern "C" fn(), wnd::Native, u32, u32, u32); 8] = [
        (b"BUTTON", 0x8080, __w32t_Rs2ButtonProc, crate::ctl::button, dbl, 0, 0),
        (b"STATIC", 0x8081, __w32t_Rs2StaticProc, crate::ctl::static_, wnd::CS_PARENTDC, 0, 0),
        (b"EDIT", 0x8082, __w32t_Rs2EditProc, crate::edit::proc_, dbl, 0, 0),
        (b"LISTBOX", 0x8083, __w32t_Rs2ListBoxProc, crate::listbox::proc_, dbl, 0, 0),
        (b"COMBOBOX", 0x8084, __w32t_Rs2ComboBoxProc, crate::listbox::combo_proc, dbl, 0, 0),
        (b"COMBOLBOX", 0x8085, __w32t_Rs2ListBoxProc, crate::listbox::proc_, dbl, 0, 0),
        (b"SCROLLBAR", 0x8086, __w32t_Rs2ScrollBarProc, crate::scroll::control_proc, dbl, 0, 0),
        (b"#32770", 0x8002, __w32t_DefDlgProcA, crate::dialog::def_dlg_proc, wnd::CS_DBLCLKS | 0x0800, crate::dialog::DLGWINDOWEXTRA, 0),
    ];
    wnd::add_native(crate::comdlg::proc_addr(), crate::comdlg::dlg_proc);
    for (name, atom, entry, f, style, extra, brush) in classes {
        wnd::add_native(thunk(entry), f);
        let cursor = if name == b"EDIT" { 32513 } else { 32512 };
        wnd::register_system(name, atom, style, thunk(entry), extra, cursor, brush);
    }
}

/// DefWindowProcW as a native procedure (it takes the W form: natives
/// are given the A form, so this is DefWindowProcA).
fn def_window_proc_w(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    wnd::def_window_proc(h, m, wp, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2ButtonProc(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    crate::ctl::button(hw(h), m as u16, wp, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2StaticProc(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    crate::ctl::static_(hw(h), m as u16, wp, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2EditProc(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    crate::edit::proc_(hw(h), m as u16, wp, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2ListBoxProc(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    crate::listbox::proc_(hw(h), m as u16, wp, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2ComboBoxProc(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    crate::listbox::combo_proc(hw(h), m as u16, wp, lp)
}

/// The SCROLLBAR class's procedure (programs see its address).
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2ScrollBarProc(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    crate::scroll::control_proc(hw(h), m as u16, wp, lp)
}

// ---- Classes ----

/// WNDCLASS(A/W) (Ex: cbSize first, hIconSm last).
fn register(p: u32, ex: bool, wide: bool) -> u32 {
    if p == 0 {
        return 0;
    }
    let o = if ex { 4 } else { 0 };
    let r = |i: u32| rd32(p, o + 4 * i);
    let Some(name) = (if r(9) >> 16 == 0 { None } else { str_arg(r(9), wide) }) else { return crate::fail(87, 0) };
    wnd::register_class(NewClass {
        name,
        style: r(0),
        proc_: r(1),
        unicode: wide,
        cls_extra: r(2),
        wnd_extra: r(3),
        hinst: r(4),
        icon: r(5),
        icon_sm: if ex { r(10) } else { 0 },
        cursor: r(6),
        brush: r(7),
        menu: unsafe { Name::of(r(8), wide) },
    }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterClassA(p: u32) -> u32 {
    register(p, false, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterClassW(p: u32) -> u32 {
    register(p, false, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterClassExA(p: u32) -> u32 {
    register(p, true, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterClassExW(p: u32) -> u32 {
    register(p, true, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnregisterClassA(n: u32, hi: u32) -> i32 {
    wnd::unregister_class(&ClassName::of(n, false), hi) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnregisterClassW(n: u32, hi: u32) -> i32 {
    wnd::unregister_class(&ClassName::of(n, true), hi) as i32
}

/// GetClassInfo(Ex): the class's WNDCLASS(EX); the atom.
fn class_info(hi: u32, n: u32, p: u32, ex: bool, wide: bool) -> u32 {
    let Some(ci) = wnd::find_class(&ClassName::of(n, wide), hi) else { return crate::fail(1411, 0) };
    let Some(c) = wnd::class(ci) else { return 0 };
    if p == 0 {
        return 0;
    }
    let o = if ex { 4 } else { 0 };
    let v = [c.style, c.proc_, c.cls_extra.len() as u32, c.wnd_extra, c.hinst, c.icon, c.cursor, c.brush, 0, n];
    for (i, x) in v.iter().enumerate() {
        wr32(p, o + 4 * i as u32, *x);
    }
    if ex {
        wr32(p, 44, c.icon_sm);
    }
    c.atom as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassInfoA(hi: u32, n: u32, p: u32) -> i32 {
    class_info(hi, n, p, false, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassInfoW(hi: u32, n: u32, p: u32) -> i32 {
    class_info(hi, n, p, false, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassInfoExA(hi: u32, n: u32, p: u32) -> u32 {
    class_info(hi, n, p, true, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassInfoExW(hi: u32, n: u32, p: u32) -> u32 {
    class_info(hi, n, p, true, true)
}

fn class_name(h: u32, buf: u32, max: i32, wide: bool) -> i32 {
    let Some(ci) = wnd(hw(h)).map(|w| w.class) else { return crate::fail(1400, 0) };
    let name = wnd::class(ci).map_or(Vec::new(), |c| c.name.clone());
    out_text(&name, buf, max, wide)
}

/// Text into an A or W buffer of `max` characters: the characters.
fn out_text(t: &[u8], buf: u32, max: i32, wide: bool) -> i32 {
    if wide { crate::copy_out_w(&crate::to_wide(t), buf as *mut u16, max) as i32 } else { crate::copy_out(t, buf as *mut u8, max) as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassNameA(h: u32, b: u32, m: i32) -> i32 {
    class_name(h, b, m, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassNameW(h: u32, b: u32, m: i32) -> i32 {
    class_name(h, b, m, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RealGetWindowClassA(h: u32, b: u32, m: u32) -> u32 {
    class_name(h, b, m as i32, false) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RealGetWindowClassW(h: u32, b: u32, m: u32) -> u32 {
    class_name(h, b, m as i32, true) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassLongA(h: u32, i: i32) -> u32 {
    wnd::class_value(hw(h), i, None, 4)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassLongW(h: u32, i: i32) -> u32 {
    wnd::class_value(hw(h), i, None, 4)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetClassLongA(h: u32, i: i32, v: u32) -> u32 {
    wnd::class_value(hw(h), i, Some(v), 4)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetClassLongW(h: u32, i: i32, v: u32) -> u32 {
    wnd::class_value(hw(h), i, Some(v), 4)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClassWord(h: u32, i: i32) -> u32 {
    wnd::class_value(hw(h), i, None, 2) & 0xffff
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetClassWord(h: u32, i: i32, v: u32) -> u32 {
    wnd::class_value(hw(h), i, Some(v & 0xffff), 2) & 0xffff
}

// ---- Windows ----

fn create_window(ex: u32, class: u32, title: u32, style: u32, x: i32, y: i32, cx: i32, cy: i32, parent: u32, menu: u32, hi: u32, param: u32, wide: bool) -> u32 {
    let cname = ClassName::of(class, wide);
    let Some(ci) = wnd::find_class(&cname, hi) else {
        if crate::tracing() {
            let n = match &cname {
                ClassName::Name(n) => alloc::string::String::from_utf8_lossy(n).into_owned(),
                ClassName::Atom(a) => alloc::format!("#{a:#x}"),
            };
            crate::trace(format_args!("W32 CreateWindow: no class {n}"));
        }
        return crate::fail(1407, 0); // ERROR_CANNOT_FIND_WND_CLASS
    };
    let text = str_arg(title, wide).unwrap_or_default();
    // HWND_MESSAGE (-3): a message-only window, a hidden top-level one.
    let parent = if parent == 0xffff_fffd { 0 } else { hw(parent) };
    wnd::create(&wnd::Create {
        exstyle: ex,
        class: ci,
        title: text,
        title_arg: title,
        class_arg: class,
        wide,
        style,
        pos: [x, y, cx, cy],
        parent,
        menu,
        hinst: hi,
        param,
    }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateWindowExA(ex: u32, c: u32, t: u32, s: u32, x: i32, y: i32, cx: i32, cy: i32, p: u32, m: u32, hi: u32, lp: u32) -> u32 {
    create_window(ex, c, t, s, x, y, cx, cy, p, m, hi, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateWindowExW(ex: u32, c: u32, t: u32, s: u32, x: i32, y: i32, cx: i32, cy: i32, p: u32, m: u32, hi: u32, lp: u32) -> u32 {
    create_window(ex, c, t, s, x, y, cx, cy, p, m, hi, lp, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DestroyWindow(h: u32) -> i32 {
    if wnd(hw(h)).is_none() {
        return crate::fail(1400, 0);
    }
    wnd::destroy_window(hw(h)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ShowWindow(h: u32, cmd: i32) -> i32 {
    wnd::show_window(hw(h), cmd as u32) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ShowWindowAsync(h: u32, cmd: i32) -> i32 {
    wnd::show_window(hw(h), cmd as u32) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UpdateWindow(h: u32) -> i32 {
    match wnd(hw(h)) {
        Some(w) => {
            let c = w.client;
            unsafe { WinUpdateWindow(c) };
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MoveWindow(h: u32, x: i32, y: i32, cx: i32, cy: i32, _rp: i32) -> i32 {
    wnd::move_window(hw(h), x, y, cx, cy, SWP_MOVE | SWP_SIZE) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowPos(h: u32, after: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> i32 {
    wnd::set_window_pos(hw(h), after, x, y, cx, cy, fl) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn BeginDeferWindowPos(_n: i32) -> u32 {
    0x00ee_0001
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DeferWindowPos(d: u32, h: u32, after: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32 {
    wnd::set_window_pos(hw(h), after, x, y, cx, cy, fl);
    d
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EndDeferWindowPos(_d: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowRect(h: u32, r: u32) -> i32 {
    if h == 0 || h == DESKTOP {
        let (cx, cy) = wnd::screen_size();
        write_rect(r, [0, 0, cx, cy]);
        return 1;
    }
    if wnd(hw(h)).is_none() {
        return crate::fail(1400, 0);
    }
    write_rect(r, wnd::window_screen_rect(hw(h)));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClientRect(h: u32, r: u32) -> i32 {
    if h == 0 || h == DESKTOP {
        let (cx, cy) = wnd::screen_size();
        write_rect(r, [0, 0, cx, cy]);
        return 1;
    }
    if wnd(hw(h)).is_none() {
        return crate::fail(1400, 0);
    }
    let (cx, cy) = wnd::client_size(hw(h));
    write_rect(r, [0, 0, cx, cy]);
    1
}

/// GetWindowInfo: WINDOWINFO (cbSize, rcWindow, rcClient, dwStyle,
/// dwExStyle, dwWindowStatus, cxWindowBorders, cyWindowBorders,
/// atomWindowType, wCreatorVersion).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowInfo(h: u32, p: u32) -> i32 {
    let Some((style, ex, ci)) = wnd(hw(h)).map(|w| (w.style, w.exstyle, w.class)) else { return 0 };
    write_rect(p + 4, wnd::window_screen_rect(hw(h)));
    let (cx, cy) = wnd::client_size(hw(h));
    let (x, y) = wnd::to_screen(hw(h), 0, 0);
    write_rect(p + 20, [x, y, x + cx, y + cy]);
    wr32(p, 36, style);
    wr32(p, 40, ex);
    wr32(p, 44, (wnd::focus() == hw(h)) as u32);
    wr32(p, 48, 1);
    wr32(p, 52, 1);
    wr32(p, 56, wnd::class(ci).map_or(0, |c| c.atom as u32) | 0x0500 << 16);
    1
}

const DESKTOP: u32 = 0x0001_0010;

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDesktopWindow() -> u32 {
    DESKTOP
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetShellWindow() -> u32 {
    0
}

fn map_points(from: u32, to: u32, p: u32, n: u32) -> i32 {
    let (f, t) = (if from == DESKTOP { 0 } else { hw(from) }, if to == DESKTOP { 0 } else { hw(to) });
    let (fx, fy) = if f == 0 { (0, 0) } else { wnd::to_screen(f, 0, 0) };
    let (tx, ty) = if t == 0 { (0, 0) } else { wnd::to_screen(t, 0, 0) };
    let (dx, dy) = (fx - tx, fy - ty);
    for i in 0..n {
        if let Some((x, y)) = crate::read_point(p + 8 * i) {
            write_point(p + 8 * i, (x + dx, y + dy));
        }
    }
    ((dy as u32 & 0xffff) << 16 | dx as u32 & 0xffff) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MapWindowPoints(from: u32, to: u32, p: u32, n: u32) -> i32 {
    map_points(from, to, p, n)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ClientToScreen(h: u32, p: u32) -> i32 {
    if wnd(hw(h)).is_none() {
        return 0;
    }
    map_points(h, 0, p, 1);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ScreenToClient(h: u32, p: u32) -> i32 {
    if wnd(hw(h)).is_none() {
        return 0;
    }
    map_points(0, h, p, 1);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WindowFromPoint(x: i32, y: i32) -> u32 {
    let sy = wnd::screen_size().1;
    let pm = unsafe { WinWindowFromPoint(HWND_DESKTOP, &PointL { x, y: sy - 1 - y }, 1) };
    wnd::hwnd_above(pm) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChildWindowFromPoint(h: u32, x: i32, y: i32) -> u32 {
    wnd::child_from_point(hw(h), x, y, 0) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChildWindowFromPointEx(h: u32, x: i32, y: i32, fl: u32) -> u32 {
    wnd::child_from_point(hw(h), x, y, fl) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RealChildWindowFromPoint(h: u32, x: i32, y: i32) -> u32 {
    wnd::child_from_point(hw(h), x, y, 1) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetParent(h: u32) -> u32 {
    // A child's parent, a pop-up's owner.
    wnd(hw(h)).map_or(0, |w| if w.style & (wnd::WS_CHILD | wnd::WS_POPUP) != 0 { w.parent as u32 } else { 0 })
}

/// GetAncestor(hwnd, GA_PARENT 1, GA_ROOT 2, GA_ROOTOWNER 3).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetAncestor(h: u32, how: u32) -> u32 {
    let h = hw(h);
    match how {
        1 => wnd(h).map_or(0, |w| if w.style & wnd::WS_CHILD != 0 { w.parent as u32 } else { DESKTOP }),
        2 => wnd::top_level(h) as u32,
        _ => {
            let mut t = wnd::top_level(h);
            for _ in 0..32 {
                match wnd(t).map(|w| w.parent).filter(|&p| p != 0) {
                    Some(p) => t = wnd::top_level(p),
                    None => break,
                }
            }
            t as u32
        }
    }
}

/// SetParent: a child window moves to another parent (its PM window
/// too); the old parent.
#[unsafe(no_mangle)]
pub extern "stdcall" fn SetParent(h: u32, p: u32) -> u32 {
    let (h, p) = (hw(h), hw(p));
    let Some((old, f, child)) = wnd(h).map(|w| (w.parent, w.frame, w.style & wnd::WS_CHILD != 0)) else { return 0 };
    let pm = if p == 0 { HWND_DESKTOP } else { wnd(p).map_or(HWND_DESKTOP, |w| w.client) };
    if child || p != 0 {
        unsafe { WinSetParent(f, pm, 1) };
    }
    if let Some(w) = wnd(h) {
        w.parent = p;
    }
    old as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindow(h: u32, cmd: u32) -> u32 {
    wnd::get_window(hw(h), cmd) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTopWindow(h: u32) -> u32 {
    if h == 0 || h == DESKTOP {
        return wnd::top_levels().first().copied().unwrap_or(0) as u32;
    }
    wnd::children(hw(h)).first().copied().unwrap_or(0) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetNextWindow(h: u32, cmd: u32) -> u32 {
    wnd::get_window(hw(h), cmd) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetLastActivePopup(h: u32) -> u32 {
    h
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsWindow(h: u32) -> i32 {
    wnd(hw(h)).is_some() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsWindowVisible(h: u32) -> i32 {
    wnd(hw(h)).is_some_and(|w| unsafe { WinIsWindowVisible(w.frame) } != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsWindowEnabled(h: u32) -> i32 {
    wnd(hw(h)).is_some_and(|w| w.style & wnd::WS_DISABLED == 0) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnableWindow(h: u32, on: i32) -> i32 {
    wnd::enable_window(hw(h), on != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsChild(p: u32, h: u32) -> i32 {
    wnd::is_child(hw(p), hw(h)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsIconic(h: u32) -> i32 {
    wnd::is_iconic(hw(h)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsZoomed(h: u32) -> i32 {
    wnd::is_zoomed(hw(h)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsWindowUnicode(h: u32) -> i32 {
    wnd(hw(h)).is_some_and(|w| w.unicode) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CloseWindow(h: u32) -> i32 {
    wnd::show_window(hw(h), 6);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenIcon(h: u32) -> i32 {
    wnd::show_window(hw(h), 9);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AnyPopup() -> i32 {
    wnd::top_levels().iter().any(|&w| wnd(w).is_some_and(|x| x.style & 0x9000_0000 == 0x9000_0000)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ShowOwnedPopups(h: u32, show: i32) -> i32 {
    for w in wnd::owned_popups(hw(h)) {
        wnd::show_window(w, if show != 0 { 5 } else { 0 });
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LockWindowUpdate(_h: u32) -> i32 {
    1
}

fn find(class: u32, title: u32, parent: Option<u16>, after: u16, wide: bool) -> u32 {
    let ci = if class != 0 { wnd::find_class(&ClassName::of(class, wide), 0) } else { None };
    let t = if title != 0 { str_arg(title, wide) } else { None };
    let r = wnd::find_window(ci, class != 0, t.as_deref(), parent, after) as u32;
    if r == 0 {
        crate::fail(2, 0)
    } else {
        r
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FindWindowA(c: u32, t: u32) -> u32 {
    find(c, t, None, 0, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FindWindowW(c: u32, t: u32) -> u32 {
    find(c, t, None, 0, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FindWindowExA(p: u32, after: u32, c: u32, t: u32) -> u32 {
    find(c, t, (p != 0 && p != DESKTOP).then_some(hw(p)), hw(after), false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FindWindowExW(p: u32, after: u32, c: u32, t: u32) -> u32 {
    find(c, t, (p != 0 && p != DESKTOP).then_some(hw(p)), hw(after), true)
}

fn enum_list(list: Vec<u16>, f: u32, lp: u32) -> i32 {
    for w in list {
        if wnd(w).is_some() && crate::call4_2(f, w as u32, lp) == 0 {
            return 0;
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumWindows(f: u32, lp: u32) -> i32 {
    enum_list(wnd::top_levels(), f, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumChildWindows(h: u32, f: u32, lp: u32) -> i32 {
    let list = if h == 0 || h == DESKTOP { wnd::top_levels() } else { wnd::descendants(hw(h)) };
    enum_list(list, f, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumThreadWindows(t: u32, f: u32, lp: u32) -> i32 {
    let list = wnd::top_levels().into_iter().filter(|&w| wnd(w).is_some_and(|x| x.tid == t)).collect();
    enum_list(list, f, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDesktopWindows(_d: u32, f: u32, lp: u32) -> i32 {
    enum_list(wnd::top_levels(), f, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowTextA(h: u32, buf: u32, max: i32) -> i32 {
    if buf == 0 || max <= 0 {
        return 0;
    }
    wnd::deliver(hw(h), wnd::W_GETTEXT, max as u32, buf, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowTextW(h: u32, buf: u32, max: i32) -> i32 {
    if buf == 0 || max <= 0 {
        return 0;
    }
    wnd::deliver(hw(h), wnd::W_GETTEXT, max as u32, buf, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InternalGetWindowText(h: u32, buf: u32, max: i32) -> i32 {
    out_text(&wnd::window_text(hw(h)), buf, max, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowTextA(h: u32, t: u32) -> i32 {
    wnd::deliver(hw(h), wnd::W_SETTEXT, 0, t, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowTextW(h: u32, t: u32) -> i32 {
    wnd::deliver(hw(h), wnd::W_SETTEXT, 0, t, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowTextLengthA(h: u32) -> i32 {
    wnd::deliver(hw(h), wnd::W_GETTEXTLENGTH, 0, 0, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowTextLengthW(h: u32) -> i32 {
    wnd::deliver(hw(h), wnd::W_GETTEXTLENGTH, 0, 0, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowLongA(h: u32, i: i32) -> u32 {
    wnd::window_value(hw(h), i, None, 4, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowLongW(h: u32, i: i32) -> u32 {
    wnd::window_value(hw(h), i, None, 4, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowLongA(h: u32, i: i32, v: u32) -> u32 {
    wnd::window_value(hw(h), i, Some(v), 4, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowLongW(h: u32, i: i32, v: u32) -> u32 {
    wnd::window_value(hw(h), i, Some(v), 4, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowWord(h: u32, i: i32) -> u32 {
    wnd::window_value(hw(h), i, None, 2, false) & 0xffff
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowWord(h: u32, i: i32, v: u32) -> u32 {
    wnd::window_value(hw(h), i, Some(v & 0xffff), 2, false) & 0xffff
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowThreadProcessId(h: u32, pid: u32) -> u32 {
    let Some(t) = wnd(hw(h)).map(|w| w.tid) else { return 0 };
    if pid != 0 {
        let p: u32;
        unsafe { core::arch::asm!("mov {0}, dword ptr fs:[0x20]", out(reg) p, options(nostack, readonly)) };
        wr32(pid, 0, p);
    }
    t
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetForegroundWindow(h: u32) -> i32 {
    match wnd(wnd::top_level(hw(h))) {
        Some(w) => {
            let f = w.frame;
            unsafe { WinSetActiveWindow(HWND_DESKTOP, f) };
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetForegroundWindow() -> u32 {
    wnd::hwnd_of(unsafe { WinQueryActiveWindow(HWND_DESKTOP) }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetActiveWindow(h: u32) -> u32 {
    let old = wnd::hwnd_of(unsafe { WinQueryActiveWindow(HWND_DESKTOP) });
    if let Some(f) = wnd(wnd::top_level(hw(h))).map(|w| w.frame) {
        unsafe { WinSetActiveWindow(HWND_DESKTOP, f) };
    }
    old as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetActiveWindow() -> u32 {
    wnd::hwnd_of(unsafe { WinQueryActiveWindow(HWND_DESKTOP) }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn BringWindowToTop(h: u32) -> i32 {
    if let Some(f) = wnd(hw(h)).map(|w| w.frame) {
        unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER) };
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetFocus(h: u32) -> u32 {
    wnd::set_focus(hw(h)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetFocus() -> u32 {
    wnd::focus() as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetCapture(h: u32) -> u32 {
    wnd::set_capture(hw(h)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ReleaseCapture() -> i32 {
    unsafe { WinSetCapture(HWND_DESKTOP, 0) };
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCapture() -> u32 {
    wnd::hwnd_above(unsafe { WinQueryCapture(HWND_DESKTOP) }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDlgCtrlID(h: u32) -> i32 {
    wnd(hw(h)).map_or(0, |w| w.id as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDlgItem(h: u32, id: i32) -> u32 {
    let h = hw(h);
    wnd::children(h).into_iter().find(|&c| wnd(c).is_some_and(|w| w.id == id as u32)).map_or_else(|| crate::fail(1421, 0), |c| c as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowRgn(_h: u32, r: u32, _redraw: i32) -> i32 {
    if r != 0 {
        crate::gdi32::DeleteObject(r);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowRgn(_h: u32, _r: u32) -> i32 {
    0
}

/// WINDOWPLACEMENT: length, flags, showCmd, ptMinPosition,
/// ptMaxPosition, rcNormalPosition.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowPlacement(h: u32, p: u32) -> i32 {
    let h = hw(h);
    if wnd(h).is_none() || p == 0 {
        return 0;
    }
    let show = if wnd::is_iconic(h) { 2 } else if wnd::is_zoomed(h) { 3 } else { 1 };
    wr32(p, 4, 0);
    wr32(p, 8, show);
    write_point(p + 12, (-1, -1));
    write_point(p + 20, (-1, -1));
    write_rect(p + 28, wnd::window_rect(h));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowPlacement(h: u32, p: u32) -> i32 {
    let h = hw(h);
    if wnd(h).is_none() || p == 0 {
        return 0;
    }
    if let Some(r) = read_rect(p + 28) {
        wnd::move_window(h, r[0], r[1], r[2] - r[0], r[3] - r[1], SWP_MOVE | SWP_SIZE);
    }
    wnd::show_window(h, rd32(p, 8));
    1
}

fn adjust(r: u32, style: u32, menu: i32, ex: u32) -> i32 {
    let Some(rc) = read_rect(r) else { return 0 };
    let style = if style & (wnd::WS_CHILD | wnd::WS_POPUP) == 0 { style | wnd::WS_CAPTION } else { style };
    let (l, t, rr, b) = wnd::frame_insets(style & !(wnd::WS_VSCROLL | wnd::WS_HSCROLL), ex, menu != 0);
    write_rect(r, [rc[0] - l, rc[1] - t, rc[2] + rr, rc[3] + b]);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AdjustWindowRect(r: u32, style: u32, menu: i32) -> i32 {
    adjust(r, style, menu, 0)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AdjustWindowRectEx(r: u32, style: u32, menu: i32, ex: u32) -> i32 {
    adjust(r, style, menu, ex)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FlashWindow(h: u32, on: i32) -> i32 {
    if let Some(f) = wnd(hw(h)).map(|w| w.frame) {
        unsafe { WinFlashWindow(f, on as u32) };
    }
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FlashWindowEx(p: u32) -> i32 {
    FlashWindow(rd32(p, 4), (rd32(p, 8) != 0) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowModuleFileNameA(_h: u32, b: u32, n: u32) -> u32 {
    crate::kern::module_file_name(0, b, n)
}

// ---- Properties ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetPropA(h: u32, n: u32, v: u32) -> i32 {
    wnd::set_prop(hw(h), wnd::prop_name(n, false), v) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetPropW(h: u32, n: u32, v: u32) -> i32 {
    wnd::set_prop(hw(h), wnd::prop_name(n, true), v) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPropA(h: u32, n: u32) -> u32 {
    wnd::get_prop(hw(h), &wnd::prop_name(n, false))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPropW(h: u32, n: u32) -> u32 {
    wnd::get_prop(hw(h), &wnd::prop_name(n, true))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RemovePropA(h: u32, n: u32) -> u32 {
    wnd::remove_prop(hw(h), &wnd::prop_name(n, false))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RemovePropW(h: u32, n: u32) -> u32 {
    wnd::remove_prop(hw(h), &wnd::prop_name(n, true))
}

// ---- Messages ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendMessageA(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    send(h, m, wp, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendMessageW(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    send(h, m, wp, lp, true)
}

fn send(h: u32, m: u32, wp: u32, lp: u32, wide: bool) -> u32 {
    if h == 0xffff || h == 0xffff_ffff {
        for t in wnd::top_levels() {
            wnd::deliver(t, m as u16, wp, lp, wide);
        }
        return 1;
    }
    if wnd(hw(h)).is_none() {
        return crate::fail(1400, 0);
    }
    wnd::deliver(hw(h), m as u16, wp, lp, wide)
}

/// SendMessageTimeout: sent (and waited for), the result stored.
fn send_timeout(h: u32, m: u32, wp: u32, lp: u32, res: u32, wide: bool) -> u32 {
    if wnd(hw(h)).is_none() && h != 0xffff {
        return 0;
    }
    let r = send(h, m, wp, lp, wide);
    if res != 0 {
        wr32(res, 0, r);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendMessageTimeoutA(h: u32, m: u32, wp: u32, lp: u32, _f: u32, _t: u32, res: u32) -> u32 {
    send_timeout(h, m, wp, lp, res, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendMessageTimeoutW(h: u32, m: u32, wp: u32, lp: u32, _f: u32, _t: u32, res: u32) -> u32 {
    send_timeout(h, m, wp, lp, res, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendNotifyMessageA(h: u32, m: u32, wp: u32, lp: u32) -> i32 {
    send(h, m, wp, lp, false);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendNotifyMessageW(h: u32, m: u32, wp: u32, lp: u32) -> i32 {
    send(h, m, wp, lp, true);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PostMessageA(h: u32, m: u32, wp: u32, lp: u32) -> i32 {
    wnd::post(if h == 0xffff_ffff { 0xffff } else { hw(h) }, m as u16, wp, lp) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PostMessageW(h: u32, m: u32, wp: u32, lp: u32) -> i32 {
    PostMessageA(h, m, wp, lp)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PostThreadMessageA(t: u32, m: u32, wp: u32, lp: u32) -> i32 {
    wnd::post_thread(t, m as u16, wp, lp) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PostThreadMessageW(t: u32, m: u32, wp: u32, lp: u32) -> i32 {
    wnd::post_thread(t, m as u16, wp, lp) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PostQuitMessage(code: i32) {
    unsafe { WinPostQueueMsg(crate::queue().hmq, WM_QUIT, code as u32, 0) };
}

/// A MSG for a W caller: a character message's character in UTF-16.
fn widen(m: &mut Msg) {
    if matches!(m.msg as u16, wnd::W_CHAR | wnd::W_SYSCHAR | wnd::W_DEADCHAR) && m.wp < 0x100 {
        m.wp = crate::to_wide(&[m.wp as u8]).first().copied().unwrap_or(0x3f) as u32;
    }
}

fn get(p: u32, h: u32, lo: u32, hi: u32, wide: bool) -> i32 {
    let Some(mut m) = wnd::get_message(hw(h), lo, hi, None) else { return -1 };
    if wide {
        widen(&mut m);
    }
    m.write(p);
    (m.msg != wnd::W_QUIT as u32) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMessageA(p: u32, h: u32, lo: u32, hi: u32) -> i32 {
    get(p, h, lo, hi, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMessageW(p: u32, h: u32, lo: u32, hi: u32) -> i32 {
    get(p, h, lo, hi, true)
}

fn peek(p: u32, h: u32, lo: u32, hi: u32, fl: u32, wide: bool) -> i32 {
    let h = if h == 0xffff_ffff { 0 } else { hw(h) };
    match wnd::get_message(h, lo, hi, Some(fl)) {
        Some(mut m) => {
            if wide {
                widen(&mut m);
            }
            m.write(p);
            1
        }
        None => {
            // A program polling in a loop lets the others run.
            unsafe { os2::DosSleep(0) };
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PeekMessageA(p: u32, h: u32, lo: u32, hi: u32, fl: u32) -> i32 {
    peek(p, h, lo, hi, fl, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PeekMessageW(p: u32, h: u32, lo: u32, hi: u32, fl: u32) -> i32 {
    peek(p, h, lo, hi, fl, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TranslateMessage(p: u32) -> i32 {
    Msg::read(p).is_some_and(|m| wnd::translate(&m)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DispatchMessageA(p: u32) -> u32 {
    Msg::read(p).map_or(0, |m| wnd::dispatch(&m, false))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DispatchMessageW(p: u32) -> u32 {
    Msg::read(p).map_or(0, |m| wnd::dispatch(&m, true))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WaitMessage() -> i32 {
    wnd::wait_message();
    1
}

/// MsgWaitForMultipleObjects(Ex): until a handle is signalled (the
/// handle's index) or input comes (WAIT_OBJECT_0 + n), polling both.
fn msg_wait(n: u32, hs: *const u32, all: bool, timeout: u32) -> u32 {
    let start = crate::kern::tick_count();
    loop {
        if n > 0 {
            let r = crate::kern::wait_objects(n, hs, all as i32, 0);
            if r != 0x102 {
                return r;
            }
        }
        if wnd::input_waiting() {
            return n;
        }
        if timeout != 0xffff_ffff && crate::kern::tick_count().wrapping_sub(start) >= timeout {
            return 0x102;
        }
        unsafe { os2::DosSleep(10) };
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MsgWaitForMultipleObjects(n: u32, hs: *const u32, all: i32, timeout: u32, _mask: u32) -> u32 {
    msg_wait(n, hs, all != 0, timeout)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MsgWaitForMultipleObjectsEx(n: u32, hs: *const u32, timeout: u32, _mask: u32, fl: u32) -> u32 {
    msg_wait(n, hs, fl & 1 != 0, timeout)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMessagePos() -> u32 {
    wnd::message_pos()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMessageTime() -> u32 {
    wnd::message_time()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMessageExtraInfo() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMessageExtraInfo(_v: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetQueueStatus(_f: u32) -> u32 {
    if wnd::input_waiting() { 0x00ff_00ff } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetInputState() -> i32 {
    wnd::input_waiting() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InSendMessage() -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InSendMessageEx(_r: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ReplyMessage(_r: u32) -> i32 {
    0
}

/// CallWindowProc(A/W): a procedure in its own form (a known one's, or
/// the caller's).
fn call_window_proc(p: u32, h: u32, m: u32, wp: u32, lp: u32, wide: bool) -> u32 {
    let form = wnd::proc_form(p).unwrap_or(wide);
    wnd::call_conv(p, form, hw(h), m as u16, wp, lp, wide)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CallWindowProcA(p: u32, h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    call_window_proc(p, h, m, wp, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CallWindowProcW(p: u32, h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    call_window_proc(p, h, m, wp, lp, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DefWindowProcA(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    wnd::def_window_proc(hw(h), m as u16, wp, lp)
}

/// DefWindowProcW: the text messages converted to the A form.
#[unsafe(no_mangle)]
pub extern "stdcall" fn DefWindowProcW(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    wnd::call_conv(thunk(__w32t_DefWindowProcA), false, hw(h), m as u16, wp, lp, true)
}

unsafe extern "C" {
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.700"]
    fn WinAddAtom(table: u32, name: *const u8) -> u32;
}

/// RegisterWindowMessage: PM's atom table (0xC000 and up), the same
/// in every process.
pub fn register_message(name: &[u8]) -> u32 {
    if name.is_empty() {
        return 0;
    }
    let z = crate::z(name);
    unsafe {
        let t = WinQuerySystemAtomTable();
        WinAddAtom(t, z.as_ptr())
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterWindowMessageA(n: u32) -> u32 {
    register_message(far_str(n).unwrap_or(b""))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterWindowMessageW(n: u32) -> u32 {
    register_message(&wide_str(n).unwrap_or_default())
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn BroadcastSystemMessageA(_f: u32, _r: u32, m: u32, wp: u32, lp: u32) -> i32 {
    send(0xffff, m, wp, lp, false);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn BroadcastSystemMessageW(_f: u32, _r: u32, m: u32, wp: u32, lp: u32) -> i32 {
    send(0xffff, m, wp, lp, true);
    1
}

/// Hooks: kept, not called (the input reaches the windows as before).
#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowsHookExA(_id: i32, _f: u32, _hi: u32, _t: u32) -> u32 {
    0x00ec_0001
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowsHookExW(_id: i32, _f: u32, _hi: u32, _t: u32) -> u32 {
    0x00ec_0001
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowsHookA(_id: i32, _f: u32) -> u32 {
    0x00ec_0001
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnhookWindowsHookEx(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnhookWindowsHook(_id: i32, _f: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CallNextHookEx(_h: u32, _c: i32, _wp: u32, _lp: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWinEventHook(_a: u32, _b: u32, _m: u32, _f: u32, _p: u32, _t: u32, _fl: u32) -> u32 {
    0x00ec_0002
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnhookWinEvent(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn NotifyWinEvent(_e: u32, _h: u32, _o: i32, _c: i32) {}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AttachThreadInput(_a: u32, _b: u32, _on: i32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WaitForInputIdle(_p: u32, _t: u32) -> u32 {
    0
}

// ---- Painting ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn BeginPaint(h: u32, ps: u32) -> u32 {
    wnd::begin_paint(hw(h), ps) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EndPaint(_h: u32, ps: u32) -> i32 {
    if ps != 0 {
        gdi::release_dc(rd32(ps, 0) as u16);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDC(h: u32) -> u32 {
    wnd::get_dc(if h == DESKTOP { 0 } else { hw(h) }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDCEx(h: u32, _r: u32, _fl: u32) -> u32 {
    GetDC(h)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowDC(h: u32) -> u32 {
    GetDC(h)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ReleaseDC(_h: u32, hdc: u32) -> i32 {
    if hdc > 0xffff {
        return 0;
    }
    gdi::release_dc(hdc as u16) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WindowFromDC(hdc: u32) -> u32 {
    gdi::dc(hdc as u16).map_or(0, |d| wnd::hwnd_of(d.pmwnd) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InvalidateRect(h: u32, r: u32, _erase: i32) -> i32 {
    if h == 0 {
        for t in wnd::top_levels() {
            wnd::invalidate(t, None);
        }
        return 1;
    }
    wnd::invalidate(hw(h), read_rect(r));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ValidateRect(h: u32, r: u32) -> i32 {
    wnd::validate(hw(h), read_rect(r));
    1
}

fn region_rects(r: u32) -> Option<Vec<[i32; 4]>> {
    if r == 0 { None } else { crate::region::rects(r as u16) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InvalidateRgn(h: u32, r: u32, _erase: i32) -> i32 {
    match region_rects(r) {
        Some(rs) => rs.into_iter().for_each(|q| wnd::invalidate(hw(h), Some(q))),
        None => wnd::invalidate(hw(h), None),
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ValidateRgn(h: u32, r: u32) -> i32 {
    match region_rects(r) {
        Some(rs) => rs.into_iter().for_each(|q| wnd::validate(hw(h), Some(q))),
        None => wnd::validate(hw(h), None),
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetUpdateRect(h: u32, r: u32, _erase: i32) -> i32 {
    match wnd::update_rect(hw(h)) {
        Some(u) => {
            write_rect(r, u);
            1
        }
        None => {
            write_rect(r, [0; 4]);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetUpdateRgn(h: u32, r: u32, _erase: i32) -> i32 {
    let u = wnd::update_rect(hw(h));
    crate::region::set(r as u16, u.map_or(Vec::new(), |q| alloc::vec![q])) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExcludeUpdateRgn(hdc: u32, h: u32) -> i32 {
    if let Some(r) = wnd::update_rect(hw(h)) {
        if let Some(d) = gdi::dc(hdc as u16) {
            let rc = gdi::pm_rect_dev(d, r);
            unsafe { GpiExcludeClipRectangle(d.hps, &rc) };
            d.clipped = true;
        }
    }
    2
}

/// RedrawWindow(hwnd, rect, rgn, flags): RDW_INVALIDATE 1, RDW_VALIDATE
/// 8, RDW_UPDATENOW 0x100, RDW_ERASENOW 0x200, RDW_ALLCHILDREN 0x80.
#[unsafe(no_mangle)]
pub extern "stdcall" fn RedrawWindow(h: u32, r: u32, rgn: u32, fl: u32) -> i32 {
    let hh = if h == 0 { 0 } else { hw(h) };
    let targets: Vec<u16> = if hh == 0 { wnd::top_levels() } else { alloc::vec![hh] };
    for t in targets {
        if fl & 1 != 0 {
            match (read_rect(r), region_rects(rgn)) {
                (Some(q), _) => wnd::invalidate(t, Some(q)),
                (None, Some(rs)) => rs.into_iter().for_each(|q| wnd::invalidate(t, Some(q))),
                _ => wnd::invalidate(t, None),
            }
            if fl & 0x80 != 0 {
                for c in wnd::descendants(t) {
                    wnd::invalidate(c, None);
                }
            }
        }
        if fl & 8 != 0 {
            wnd::validate(t, read_rect(r));
        }
        if fl & 0x300 != 0 {
            UpdateWindow(t as u32);
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ScrollWindow(h: u32, dx: i32, dy: i32, r: u32, c: u32) -> i32 {
    wnd::scroll_window(hw(h), dx, dy, read_rect(r), read_rect(c), false);
    1
}

/// ScrollWindowEx(hwnd, dx, dy, scroll, clip, hrgnUpdate, lprcUpdate,
/// flags): SW_SCROLLCHILDREN 1, SW_INVALIDATE 2.
#[unsafe(no_mangle)]
pub extern "stdcall" fn ScrollWindowEx(h: u32, dx: i32, dy: i32, r: u32, c: u32, _rgn: u32, up: u32, fl: u32) -> i32 {
    wnd::scroll_window(hw(h), dx, dy, read_rect(r), read_rect(c), fl & 1 != 0);
    if up != 0 {
        let (cx, cy) = wnd::client_size(hw(h));
        write_rect(up, [0, 0, cx, cy]);
    }
    2
}

/// ScrollDC(hdc, dx, dy, scroll, clip, hrgnUpdate, lprcUpdate).
#[unsafe(no_mangle)]
pub extern "stdcall" fn ScrollDC(hdc: u32, dx: i32, dy: i32, sr: u32, cr: u32, rgn: u32, up: u32) -> i32 {
    let h = hdc as u16;
    let r = read_rect(sr).unwrap_or_else(|| {
        let (w, hh) = gdi::dc_size(h);
        [0, 0, w, hh]
    });
    let clip = read_rect(cr).unwrap_or(r);
    let src = [r[0].max(clip[0] - dx), r[1].max(clip[1] - dy), r[2].min(clip[2] - dx), r[3].min(clip[3] - dy)];
    if src[2] > src[0] && src[3] > src[1] {
        crate::bitmap::blit(h, src[0] + dx, src[1] + dy, src[2] - src[0], src[3] - src[1], h, src[0], src[1], 0x00cc_0020);
    }
    let moved = [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy];
    let mut upd = crate::region::combine(&alloc::vec![r], &alloc::vec![moved], 4);
    upd = crate::region::combine(&upd, &alloc::vec![clip], 1);
    if rgn != 0 {
        crate::region::set(rgn as u16, upd.clone());
    }
    write_rect(up, crate::region::bounds(&upd));
    1
}

fn draw_text(hdc: u32, s: u32, n: i32, r: u32, fmt: u32, wide: bool) -> i32 {
    crate::gdi32::drawing(hdc as u16, hdc as u16, || draw_text_(hdc, s, n, r, fmt, wide))
}

fn draw_text_(hdc: u32, s: u32, n: i32, r: u32, fmt: u32, wide: bool) -> i32 {
    let Some(rect) = read_rect(r) else { return 0 };
    let text = crate::gdi32::text_arg(s, n, wide);
    // DT_END_ELLIPSIS, DT_PATH_ELLIPSIS, DT_MODIFYSTRING and the others
    // above the 3.1 flags draw as plain text.
    let (h, out) = gdi::draw_text_bytes(hdc as u16, &text, rect, (fmt & 0xffff) as u16);
    if fmt & gdi::DT_CALCRECT as u32 != 0 {
        write_rect(r, out);
    }
    h
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawTextA(hdc: u32, s: u32, n: i32, r: u32, fmt: u32) -> i32 {
    draw_text(hdc, s, n, r, fmt, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawTextW(hdc: u32, s: u32, n: i32, r: u32, fmt: u32) -> i32 {
    draw_text(hdc, s, n, r, fmt, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawTextExA(hdc: u32, s: u32, n: i32, r: u32, fmt: u32, _p: u32) -> i32 {
    draw_text(hdc, s, n, r, fmt, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawTextExW(hdc: u32, s: u32, n: i32, r: u32, fmt: u32, _p: u32) -> i32 {
    draw_text(hdc, s, n, r, fmt, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FillRect(hdc: u32, r: u32, br: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        if let Some(r) = read_rect(r) {
            gdi::fill_rect(hdc as u16, r, br as u16);
        }
        1
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FrameRect(hdc: u32, r: u32, br: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        if let Some([l, t, rr, b]) = read_rect(r) {
            for e in [[l, t, rr, t + 1], [l, b - 1, rr, b], [l, t, l + 1, b], [rr - 1, t, rr, b]] {
                gdi::fill_rect(hdc as u16, e, br as u16);
            }
        }
        1
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InvertRect(hdc: u32, r: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        if let Some(r) = read_rect(r) {
            gdi::invert(hdc as u16, r);
        }
        1
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawFocusRect(hdc: u32, r: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        if let Some(r) = read_rect(r) {
            gdi::focus_rect(hdc as u16, r);
        }
        1
    })
}

/// DrawEdge(hdc, rect, edge, flags): BDR_RAISEDOUTER 1, BDR_SUNKENOUTER
/// 2, BDR_RAISEDINNER 4, BDR_SUNKENINNER 8; BF_LEFT 1, BF_TOP 2,
/// BF_RIGHT 4, BF_BOTTOM 8, BF_MIDDLE 0x800, BF_ADJUST 0x2000, BF_FLAT
/// 0x4000, BF_MONO 0x8000.
#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawEdge(hdc: u32, r: u32, edge: u32, fl: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        let Some(mut rc) = read_rect(r) else { return 0 };
        let h = hdc as u16;
        let c = |i| gdi::sys_color(i);
        let (light, face, shadow, dark) = (c(gdi::COLOR_BTNHIGHLIGHT), c(gdi::COLOR_BTNFACE), c(gdi::COLOR_BTNSHADOW), 0x000000);
        let ring = |outer: bool, rc: &mut [i32; 4]| {
            let (raised, sunken) = if outer { (edge & 1 != 0, edge & 2 != 0) } else { (edge & 4 != 0, edge & 8 != 0) };
            if !raised && !sunken {
                return;
            }
            let (tl, br) = if fl & 0x4000 != 0 {
                (shadow, shadow)
            } else if raised {
                if outer { (face, dark) } else { (light, shadow) }
            } else if outer {
                (shadow, light)
            } else {
                (dark, face)
            };
            let [l, t, rr, b] = *rc;
            if fl & 2 != 0 {
                gdi::fill(h, [l, t, rr, t + 1], tl);
            }
            if fl & 1 != 0 {
                gdi::fill(h, [l, t, l + 1, b], tl);
            }
            if fl & 8 != 0 {
                gdi::fill(h, [l, b - 1, rr, b], br);
            }
            if fl & 4 != 0 {
                gdi::fill(h, [rr - 1, t, rr, b], br);
            }
            *rc = [l + (fl & 1 != 0) as i32, t + (fl & 2 != 0) as i32, rr - (fl & 4 != 0) as i32, b - (fl & 8 != 0) as i32];
        };
        ring(true, &mut rc);
        ring(false, &mut rc);
        if fl & 0x800 != 0 {
            gdi::fill(h, rc, face);
        }
        if fl & 0x2000 != 0 {
            write_rect(r, rc);
        }
        1
    })
}

/// DrawFrameControl(hdc, rect, type, state): buttons (DFC_BUTTON 4:
/// push, check, radio), scroll arrows (DFC_SCROLL 3), caption buttons
/// (DFC_CAPTION 1), menu marks (DFC_MENU 2), drawn in the 3.1 style.
#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawFrameControl(hdc: u32, r: u32, ty: u32, st: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        let Some(rc) = read_rect(r) else { return 0 };
        let h = hdc as u16;
        let pushed = st & 0x0200 != 0;
        let checked = st & 0x0400 != 0;
        match ty {
            4 if st & 0xff == 0 || st & 0x10 != 0 => crate::scroll::raised(h, rc, pushed),
            4 => {
                gdi::fill(h, rc, 0xffffff);
                gdi::frame(h, rc, 0);
                if checked {
                    let [l, t, rr, b] = rc;
                    gdi::line(h, l + 2, t + 2, rr - 2, b - 2, 0);
                    gdi::line(h, rr - 3, t + 2, l + 1, b - 2, 0);
                }
            }
            3 => {
                crate::scroll::raised(h, rc, pushed);
                let dir = match st & 0xff {
                    0 => b'u',
                    1 => b'd',
                    2 => b'l',
                    _ => b'r',
                };
                crate::scroll::triangle(h, rc, dir, 0);
            }
            1 => {
                crate::scroll::raised(h, rc, pushed);
                let dir = if st & 0xff == 2 { b'u' } else { b'd' };
                crate::scroll::triangle(h, rc, dir, 0);
            }
            _ => {
                if checked || st & 0xff == 1 {
                    crate::scroll::triangle(h, rc, b'r', 0);
                }
            }
        }
        1
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawIcon(hdc: u32, x: i32, y: i32, h: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        crate::icon::draw(hdc as u16, x, y, h) as i32
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawIconEx(hdc: u32, x: i32, y: i32, h: u32, _cx: i32, _cy: i32, _step: u32, _br: u32, _fl: u32) -> i32 {
    let hdc_ = hdc as u16;
    crate::gdi32::drawing(hdc_, hdc_, || {
        crate::icon::draw(hdc as u16, x, y, h) as i32
    })
}

/// GrayString(hdc, hbr, proc, data, n, x, y, cx, cy): the text (or what
/// the procedure draws) in the gray text colour.
fn gray_string(hdc: u32, p: u32, data: u32, n: i32, x: i32, y: i32, wide: bool) -> i32 {
    crate::gdi32::drawing(hdc as u16, hdc as u16, || gray_string_(hdc, p, data, n, x, y, wide))
}

fn gray_string_(hdc: u32, p: u32, data: u32, n: i32, x: i32, y: i32, wide: bool) -> i32 {
    let h = hdc as u16;
    let old = gdi::set_text_color(h, gdi::sys_color(gdi::COLOR_GRAYTEXT));
    let r = if p != 0 {
        crate::call4_3(p, hdc, data, n as u32) != 0
    } else {
        let s = crate::gdi32::text_arg(data, if n > 0 { n } else { -1 }, wide);
        gdi::text_at(h, x, y, &s);
        true
    };
    gdi::set_text_color(h, old);
    r as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GrayStringA(hdc: u32, _br: u32, p: u32, d: u32, n: i32, x: i32, y: i32, _cx: i32, _cy: i32) -> i32 {
    gray_string(hdc, p, d, n, x, y, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GrayStringW(hdc: u32, _br: u32, p: u32, d: u32, n: i32, x: i32, y: i32, _cx: i32, _cy: i32) -> i32 {
    gray_string(hdc, p, d, n, x, y, true)
}

/// DrawState(hdc, hbr, proc, data, wData, x, y, cx, cy, flags):
/// DST_TEXT 1, DST_PREFIXTEXT 2, DST_ICON 3, DST_BITMAP 4; DSS_DISABLED
/// 0x20 grays it.
fn draw_state(hdc: u32, p: u32, data: u32, wd: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32, wide: bool) -> i32 {
    let h = hdc as u16;
    let old = if fl & 0x20 != 0 { Some(gdi::set_text_color(h, gdi::sys_color(gdi::COLOR_GRAYTEXT))) } else { None };
    match fl & 0xf {
        1 | 2 => {
            let s = crate::gdi32::text_arg(data, if wd > 0 { wd as i32 } else { -1 }, wide);
            let f = if fl & 0xf == 1 { gdi::DT_NOPREFIX } else { 0 };
            gdi::draw_text_bytes(h, &s, [x, y, x + cx.max(1000), y + cy.max(100)], f | gdi::DT_SINGLELINE);
        }
        3 => {
            crate::icon::draw(h, x, y, data);
        }
        4 => {
            let mem = crate::bitmap::create_memory_dc();
            let b = data as u16;
            let (bw, bh) = crate::bitmap::bitmap_of(b).map_or((0, 0), |v| (v.1, v.2));
            crate::bitmap::select(mem, b);
            crate::bitmap::blit(h, x, y, if cx > 0 { cx } else { bw }, if cy > 0 { cy } else { bh }, mem, 0, 0, 0x00cc_0020);
            crate::bitmap::deselect(mem);
            gdi::release_dc(mem);
        }
        _ => {
            if p != 0 {
                crate::call4_5(p, hdc, data, wd, cx as u32, cy as u32);
            }
        }
    }
    if let Some(c) = old {
        gdi::set_text_color(h, c);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawStateA(hdc: u32, _br: u32, p: u32, d: u32, wd: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> i32 {
    draw_state(hdc, p, d, wd, x, y, cx, cy, fl, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawStateW(hdc: u32, _br: u32, p: u32, d: u32, wd: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> i32 {
    draw_state(hdc, p, d, wd, x, y, cx, cy, fl, true)
}

fn tabbed(hdc: u32, x: i32, y: i32, s: u32, n: i32, nt: i32, tp: u32, origin: i32, draw: bool, wide: bool) -> u32 {
    let text = crate::gdi32::text_arg(s, n.max(0), wide);
    if !draw {
        let tabs: Vec<i32> = if tp == 0 { Vec::new() } else { (0..nt.max(0) as u32).map(|i| rd32(tp, 4 * i) as i32).collect() };
        return crate::draw::tabbed(hdc as u16, x, y, &text, &tabs, origin, false);
    }
    let tabs: Vec<i32> = if tp == 0 { Vec::new() } else { (0..nt.max(0) as u32).map(|i| rd32(tp, 4 * i) as i32).collect() };
    crate::gdi32::drawing(hdc as u16, hdc as u16, || crate::draw::tabbed(hdc as u16, x, y, &text, &tabs, origin, draw))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TabbedTextOutA(hdc: u32, x: i32, y: i32, s: u32, n: i32, nt: i32, tp: u32, o: i32) -> u32 {
    tabbed(hdc, x, y, s, n, nt, tp, o, true, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TabbedTextOutW(hdc: u32, x: i32, y: i32, s: u32, n: i32, nt: i32, tp: u32, o: i32) -> u32 {
    tabbed(hdc, x, y, s, n, nt, tp, o, true, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTabbedTextExtentA(hdc: u32, s: u32, n: i32, nt: i32, tp: u32) -> u32 {
    tabbed(hdc, 0, 0, s, n, nt, tp, 0, false, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTabbedTextExtentW(hdc: u32, s: u32, n: i32, nt: i32, tp: u32) -> u32 {
    tabbed(hdc, 0, 0, s, n, nt, tp, 0, false, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PaintDesktop(_hdc: u32) -> i32 {
    1
}

// ---- Rectangles ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetRect(r: u32, l: i32, t: i32, rr: i32, b: i32) -> i32 {
    write_rect(r, [l, t, rr, b]);
    (r != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetRectEmpty(r: u32) -> i32 {
    write_rect(r, [0; 4]);
    (r != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CopyRect(d: u32, s: u32) -> i32 {
    match read_rect(s) {
        Some(r) if d != 0 => {
            write_rect(d, r);
            1
        }
        _ => 0,
    }
}

fn empty(r: [i32; 4]) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsRectEmpty(r: u32) -> i32 {
    read_rect(r).is_none_or(empty) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PtInRect(r: u32, x: i32, y: i32) -> i32 {
    read_rect(r).is_some_and(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OffsetRect(p: u32, dx: i32, dy: i32) -> i32 {
    let Some(r) = read_rect(p) else { return 0 };
    write_rect(p, [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InflateRect(p: u32, dx: i32, dy: i32) -> i32 {
    let Some(r) = read_rect(p) else { return 0 };
    write_rect(p, [r[0] - dx, r[1] - dy, r[2] + dx, r[3] + dy]);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IntersectRect(d: u32, a: u32, b: u32) -> i32 {
    let (Some(x), Some(y)) = (read_rect(a), read_rect(b)) else { return 0 };
    let r = [x[0].max(y[0]), x[1].max(y[1]), x[2].min(y[2]), x[3].min(y[3])];
    let r = if empty(r) { [0; 4] } else { r };
    write_rect(d, r);
    !empty(r) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnionRect(d: u32, a: u32, b: u32) -> i32 {
    let (Some(x), Some(y)) = (read_rect(a), read_rect(b)) else { return 0 };
    let r = if empty(x) {
        y
    } else if empty(y) {
        x
    } else {
        [x[0].min(y[0]), x[1].min(y[1]), x[2].max(y[2]), x[3].max(y[3])]
    };
    let r = if empty(r) { [0; 4] } else { r };
    write_rect(d, r);
    !empty(r) as i32
}

/// SubtractRect: `a` less `b` when that is a rectangle (b covers a whole
/// side of a), else `a`.
#[unsafe(no_mangle)]
pub extern "stdcall" fn SubtractRect(d: u32, a: u32, b: u32) -> i32 {
    let (Some(mut x), Some(y)) = (read_rect(a), read_rect(b)) else { return 0 };
    if y[1] <= x[1] && y[3] >= x[3] {
        if y[0] <= x[0] && y[2] > x[0] {
            x[0] = y[2];
        } else if y[2] >= x[2] && y[0] < x[2] {
            x[2] = y[0];
        }
    }
    if y[0] <= x[0] && y[2] >= x[2] {
        if y[1] <= x[1] && y[3] > x[1] {
            x[1] = y[3];
        } else if y[3] >= x[3] && y[1] < x[3] {
            x[3] = y[1];
        }
    }
    let r = if empty(x) { [0; 4] } else { x };
    write_rect(d, r);
    !empty(r) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EqualRect(a: u32, b: u32) -> i32 {
    match (read_rect(a), read_rect(b)) {
        (Some(x), Some(y)) => (x == y) as i32,
        _ => 0,
    }
}

// ---- Timers ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetTimer(h: u32, id: u32, ms: u32, p: u32) -> u32 {
    wnd::set_timer(hw(h), id, ms, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn KillTimer(h: u32, id: u32) -> i32 {
    wnd::kill_timer(hw(h), id) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetCoalescableTimer(h: u32, id: u32, ms: u32, p: u32, _t: u32) -> u32 {
    wnd::set_timer(hw(h), id, ms, p)
}

// ---- Keyboard and mouse ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyState(vk: i32) -> i16 {
    wnd::key_state(vk as u16) as i16
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetAsyncKeyState(vk: i32) -> i16 {
    wnd::key_state(vk as u16) as i16
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyboardState(p: *mut u8) -> i32 {
    if p.is_null() {
        return 0;
    }
    for vk in 0..256u16 {
        let s = wnd::key_state(vk);
        unsafe { *p.add(vk as usize) = (if s & 0x8000 != 0 { 0x80 } else { 0 }) | (s & 1) as u8 };
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetKeyboardState(_p: u32) -> i32 {
    1
}

/// MapVirtualKey(code, type): MAPVK_VK_TO_VSC 0, MAPVK_VSC_TO_VK 1,
/// MAPVK_VK_TO_CHAR 2 (letters and digits).
#[unsafe(no_mangle)]
pub extern "stdcall" fn MapVirtualKeyA(code: u32, ty: u32) -> u32 {
    const ROWS: [(u8, &[u8]); 4] = [(0x02, b"1234567890"), (0x10, b"QWERTYUIOP"), (0x1e, b"ASDFGHJKL"), (0x2c, b"ZXCVBNM")];
    match ty {
        0 => {
            for (base, keys) in ROWS {
                if let Some(i) = keys.iter().position(|&k| k as u32 == code) {
                    return base as u32 + i as u32;
                }
            }
            match code {
                0x0d => 0x1c,
                0x20 => 0x39,
                0x1b => 0x01,
                0x08 => 0x0e,
                0x09 => 0x0f,
                0x10 => 0x2a,
                0x11 => 0x1d,
                0x12 => 0x38,
                0x70..=0x79 => 0x3b + code - 0x70,
                _ => 0,
            }
        }
        1 | 3 => {
            for (base, keys) in ROWS {
                if code >= base as u32 && ((code - base as u32) as usize) < keys.len() {
                    return keys[(code - base as u32) as usize] as u32;
                }
            }
            0
        }
        2 => {
            if (b'0' as u32..=b'9' as u32).contains(&code) || (b'A' as u32..=b'Z' as u32).contains(&code) || code == 0x20 {
                code
            } else {
                0
            }
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MapVirtualKeyW(code: u32, ty: u32) -> u32 {
    MapVirtualKeyA(code, ty)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MapVirtualKeyExA(code: u32, ty: u32, _l: u32) -> u32 {
    MapVirtualKeyA(code, ty)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MapVirtualKeyExW(code: u32, ty: u32, _l: u32) -> u32 {
    MapVirtualKeyA(code, ty)
}

/// VkKeyScan: the virtual key of a character, shift (1) in the high
/// byte.
#[unsafe(no_mangle)]
pub extern "stdcall" fn VkKeyScanA(c: u32) -> i16 {
    let c = c as u8;
    match c {
        b'a'..=b'z' => (c - 32) as i16,
        b'A'..=b'Z' => 0x100 | c as i16,
        b'0'..=b'9' | b' ' => c as i16,
        b'\r' => 0x0d,
        b'\t' => 0x09,
        8 => 0x08,
        27 => 0x1b,
        _ => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn VkKeyScanW(c: u32) -> i16 {
    if c < 0x80 { VkKeyScanA(c) } else { -1 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn VkKeyScanExA(c: u32, _l: u32) -> i16 {
    VkKeyScanA(c)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn VkKeyScanExW(c: u32, _l: u32) -> i16 {
    VkKeyScanW(c)
}

fn key_name(lp: u32, buf: u32, max: i32, wide: bool) -> i32 {
    let scan = (lp >> 16) & 0xff;
    let vk = MapVirtualKeyA(scan, 1);
    let name: Vec<u8> = match scan {
        0x01 => b"Esc".to_vec(),
        0x0e => b"Backspace".to_vec(),
        0x0f => b"Tab".to_vec(),
        0x1c => b"Enter".to_vec(),
        0x1d => b"Ctrl".to_vec(),
        0x2a => b"Shift".to_vec(),
        0x38 => b"Alt".to_vec(),
        0x39 => b"Space".to_vec(),
        0x3b..=0x44 => alloc::format!("F{}", scan - 0x3a).into_bytes(),
        _ if vk != 0 => alloc::vec![vk as u8],
        _ => Vec::new(),
    };
    out_text(&name, buf, max, wide)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyNameTextA(lp: u32, b: u32, m: i32) -> i32 {
    key_name(lp, b, m, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyNameTextW(lp: u32, b: u32, m: i32) -> i32 {
    key_name(lp, b, m, true)
}

/// GetKeyboardLayout: US English (0x04090409).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyboardLayout(_t: u32) -> u32 {
    0x0409_0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyboardLayoutList(n: i32, p: u32) -> i32 {
    if n > 0 && p != 0 {
        wr32(p, 0, 0x0409_0409);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyboardLayoutNameA(p: u32) -> i32 {
    crate::copy_out(b"00000409", p as *mut u8, 9);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyboardLayoutNameW(p: u32) -> i32 {
    crate::copy_out_w(&crate::to_wide(b"00000409"), p as *mut u16, 9);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ActivateKeyboardLayout(_h: u32, _f: u32) -> u32 {
    0x0409_0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadKeyboardLayoutA(_n: u32, _f: u32) -> u32 {
    0x0409_0409
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadKeyboardLayoutW(_n: u32, _f: u32) -> u32 {
    0x0409_0409
}

/// GetKeyboardType: an enhanced 101/102-key keyboard, 12 function keys.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKeyboardType(i: i32) -> i32 {
    match i {
        0 => 4,
        2 => 12,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCursorPos(p: u32) -> i32 {
    let mut pt = PointL::default();
    unsafe { WinQueryPointerPos(HWND_DESKTOP, &mut pt) };
    let sy = wnd::screen_size().1;
    write_point(p, (pt.x, sy - 1 - pt.y));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPhysicalCursorPos(p: u32) -> i32 {
    GetCursorPos(p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetCursorPos(x: i32, y: i32) -> i32 {
    let sy = wnd::screen_size().1;
    unsafe { WinSetPointerPos(HWND_DESKTOP, x, sy - 1 - y) };
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetCursor(c: u32) -> u32 {
    wnd::set_cursor(c)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCursor() -> u32 {
    wnd::cursor()
}

/// ShowCursor: the display count (PM's pointer stays).
#[unsafe(no_mangle)]
pub extern "stdcall" fn ShowCursor(show: i32) -> i32 {
    static COUNT: crate::Global<i32> = crate::Global::new(0);
    let c = COUNT.get();
    *c += if show != 0 { 1 } else { -1 };
    *c
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ClipCursor(_r: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipCursor(r: u32) -> i32 {
    let (cx, cy) = wnd::screen_size();
    write_rect(r, [0, 0, cx, cy]);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDoubleClickTime() -> u32 {
    500
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDoubleClickTime(_t: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SwapMouseButton(_s: i32) -> i32 {
    0
}

/// TrackMouseEvent: accepted (WM_MOUSELEAVE is not sent).
#[unsafe(no_mangle)]
pub extern "stdcall" fn TrackMouseEvent(_p: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn _TrackMouseEvent(_p: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DragDetect(_h: u32, _x: i32, _y: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn keybd_event(_vk: u32, _scan: u32, _f: u32, _x: u32) {}

#[unsafe(no_mangle)]
pub extern "stdcall" fn mouse_event(_f: u32, _x: u32, _y: u32, _d: u32, _e: u32) {}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SendInput(_n: u32, _p: u32, _cb: i32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn BlockInput(_b: i32) -> i32 {
    0
}

// ---- System metrics, colours, parameters ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSystemMetrics(i: i32) -> i32 {
    wnd::system_metric(i)
}

/// GetSysColor: Windows 3.1's colours for 0-20; the later indexes
/// (COLOR_3DDKSHADOW 21, COLOR_3DLIGHT 22, COLOR_INFOTEXT 23,
/// COLOR_INFOBK 24, gradients 27, 28, hot light 26, menu 29/30).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSysColor(i: i32) -> u32 {
    match i {
        0..=20 => gdi::sys_color(i as u16),
        21 => 0x404040,
        22 => 0xe0e0e0,
        23 => 0x000000,
        24 => 0xe1ffff,
        26 => 0xff0000,
        27 => 0x800000,
        28 => 0x808080,
        29 => 0x800000,
        30 => 0xc0c0c0,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSysColorBrush(i: i32) -> u32 {
    if (0..=20).contains(&i) {
        return gdi::sys_brush(i as u16) as u32;
    }
    crate::gdi32::CreateSolidBrush(GetSysColor(i))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetSysColors(_n: i32, _i: u32, _c: u32) -> i32 {
    1
}

/// NONCLIENTMETRICS's LOGFONTs: 8-point MS Sans Serif (A: 60 bytes, W:
/// 92).
fn put_logfont(p: u32, wide: bool, bold: bool) {
    unsafe { core::ptr::write_bytes(p as *mut u8, 0, if wide { 92 } else { 60 }) };
    wr32(p, 0, (-11i32) as u32);
    wr32(p, 16, if bold { 700 } else { 400 });
    let name = b"MS Sans Serif";
    if wide {
        crate::copy_out_w(&crate::to_wide(name), (p + 28) as *mut u16, 32);
    } else {
        crate::copy_out(name, (p + 28) as *mut u8, 32);
    }
}

fn spi(act: u32, param: u32, pv: u32, wide: bool) -> i32 {
    let put = |v: u32| {
        if pv != 0 {
            wr32(pv, 0, v);
        }
    };
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    match act {
        // SPI_GETBEEP, SPI_GETMOUSE (3 ints), SPI_GETBORDER
        1 => put(1),
        3 => {
            if pv != 0 {
                wr32(pv, 0, 6);
                wr32(pv, 4, 10);
                wr32(pv, 8, 1);
            }
        }
        5 => put(1),
        10 => put(31),
        13 | 24 => put(75),
        14 | 16 | 18 | 27 => put(0),
        22 => put(1),
        25 => put(1),
        // SPI_GETICONTITLELOGFONT
        31 => {
            if pv != 0 {
                put_logfont(pv, wide, false);
            }
        }
        // SPI_GETNONCLIENTMETRICS: cbSize, iBorderWidth, iScrollWidth,
        // iScrollHeight, iCaptionWidth, iCaptionHeight, lfCaptionFont,
        // iSmCaptionWidth, iSmCaptionHeight, lfSmCaptionFont,
        // iMenuWidth, iMenuHeight, lfMenuFont, lfStatusFont,
        // lfMessageFont.
        41 => {
            if pv == 0 {
                return 0;
            }
            let lf = if wide { 92 } else { 60 };
            let mut o = 4;
            for v in [1, q(SV_CXVSCROLL), q(SV_CYHSCROLL), q(SV_CYTITLEBAR), q(SV_CYTITLEBAR)] {
                wr32(pv, o, v as u32);
                o += 4;
            }
            put_logfont(pv + o, wide, true);
            o += lf;
            for v in [q(SV_CYTITLEBAR) - 4, q(SV_CYTITLEBAR) - 4] {
                wr32(pv, o, v as u32);
                o += 4;
            }
            put_logfont(pv + o, wide, true);
            o += lf;
            for v in [q(SV_CYMENU), q(SV_CYMENU)] {
                wr32(pv, o, v as u32);
                o += 4;
            }
            for _ in 0..3 {
                put_logfont(pv + o, wide, false);
                o += lf;
            }
        }
        // SPI_GETWORKAREA
        48 => {
            let (cx, cy) = wnd::screen_size();
            write_rect(pv, [0, 0, cx, cy]);
        }
        // SPI_GETHIGHCONTRAST, SPI_GETANIMATION...: none.
        66 | 72 => {
            if pv != 0 && rd32(pv, 0) >= 8 {
                wr32(pv, 4, 0);
            }
        }
        // SPI_GETKEYBOARDPREF, SPI_GETSCREENREADER, SPI_GETFONTSMOOTHING,
        // SPI_GETDRAGFULLWINDOWS, SPI_GETSHOWSOUNDS
        68 | 70 | 74 | 38 | 56 => put(0),
        // SPI_GETWHEELSCROLLLINES
        104 => put(3),
        // SPI_GETMENUSHOWDELAY
        106 => put(400),
        // The UI effects (SPI_GETMENUANIMATION, ...): off.
        0x1002..=0x101f | 0x103e => put(0),
        // SPI_GETFOCUSBORDERWIDTH/HEIGHT
        0x200e | 0x2010 => put(1),
        // SPI_GETCARETWIDTH
        0x2006 => put(1),
        _ => {
            let _ = param;
            put(0);
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SystemParametersInfoA(a: u32, p: u32, pv: u32, _ini: u32) -> i32 {
    spi(a, p, pv, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SystemParametersInfoW(a: u32, p: u32, pv: u32, _ini: u32) -> i32 {
    spi(a, p, pv, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBeep(t: u32) -> i32 {
    unsafe { WinAlarm(HWND_DESKTOP, if t == 0x10 { 1 } else if t == 0x30 { 0 } else { 2 }) };
    1
}

fn message_box(h: u32, text: u32, cap: u32, ty: u32, wide: bool) -> i32 {
    let t = str_arg(text, wide).unwrap_or_default();
    let c = str_arg(cap, wide).unwrap_or_else(|| b"Error".to_vec());
    wnd::message_box(hw(h), &t, &c, ty) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxA(h: u32, t: u32, c: u32, ty: u32) -> i32 {
    message_box(h, t, c, ty, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxW(h: u32, t: u32, c: u32, ty: u32) -> i32 {
    message_box(h, t, c, ty, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxExA(h: u32, t: u32, c: u32, ty: u32, _l: u32) -> i32 {
    message_box(h, t, c, ty, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxExW(h: u32, t: u32, c: u32, ty: u32, _l: u32) -> i32 {
    message_box(h, t, c, ty, true)
}

/// MessageBoxIndirect: MSGBOXPARAMS (cbSize, hwndOwner, hInstance,
/// lpszText, lpszCaption, dwStyle, ...).
#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxIndirectA(p: u32) -> i32 {
    message_box(rd32(p, 4), rd32(p, 12), rd32(p, 16), rd32(p, 20), false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxIndirectW(p: u32) -> i32 {
    message_box(rd32(p, 4), rd32(p, 12), rd32(p, 16), rd32(p, 20), true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxTimeoutA(h: u32, t: u32, c: u32, ty: u32, _l: u32, _ms: u32) -> i32 {
    message_box(h, t, c, ty, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MessageBoxTimeoutW(h: u32, t: u32, c: u32, ty: u32, _l: u32, _ms: u32) -> i32 {
    message_box(h, t, c, ty, true)
}

// ---- Resources: strings, icons, cursors, bitmaps, images ----

fn load_string(hi: u32, id: u32, buf: u32, max: i32, wide: bool) -> i32 {
    let s = crate::res::string(hi, id);
    if wide && max == 0 {
        // A pointer to the string itself (read-only), its length.
        let Some(s) = s else { return 0 };
        wr32(buf, 0, s.as_ptr() as u32);
        return s.len() as i32;
    }
    let Some(s) = s else {
        if buf != 0 && max > 0 {
            if wide { crate::copy_out_w(&[], buf as *mut u16, max) } else { crate::copy_out(&[], buf as *mut u8, max) };
        }
        return 0;
    };
    if wide { crate::copy_out_w(s, buf as *mut u16, max) as i32 } else { crate::copy_out(&crate::narrow(s), buf as *mut u8, max) as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadStringA(hi: u32, id: u32, b: u32, m: i32) -> i32 {
    load_string(hi, id, b, m, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadStringW(hi: u32, id: u32, b: u32, m: i32) -> i32 {
    load_string(hi, id, b, m, true)
}

/// LoadIcon / LoadCursor: the program's resources, or the built-in
/// ones (IDI_*, IDC_*: their IDs are their handles).
fn load_icon(hi: u32, n: u32, cursor: bool, wide: bool) -> u32 {
    if n >> 16 == 0 && (hi == 0 || (32512..=32651).contains(&n)) {
        return n & 0xffff;
    }
    let Some(name) = (unsafe { Name::of(n, wide) }) else { return 0 };
    match crate::icon::load(hi, &name, cursor) {
        0 if cursor => crate::fail(1814, 0),
        0 => crate::fail(1814, 0),
        h => h as u32,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadIconA(hi: u32, n: u32) -> u32 {
    load_icon(hi, n, false, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadIconW(hi: u32, n: u32) -> u32 {
    load_icon(hi, n, false, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadCursorA(hi: u32, n: u32) -> u32 {
    load_icon(hi, n, true, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadCursorW(hi: u32, n: u32) -> u32 {
    load_icon(hi, n, true, true)
}

fn load_bitmap(hi: u32, n: u32, wide: bool) -> u32 {
    let Some(name) = (unsafe { Name::of(n, wide) }) else { return 0 };
    if hi == 0 {
        return match name {
            Name::Id(id) if id >= 32734 => crate::bitmap::system_bitmap(id) as u32,
            _ => 0,
        };
    }
    crate::res::find(hi, crate::res::RT_BITMAP, &name).map_or(0, |d| crate::bitmap::from_dib(d) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadBitmapA(hi: u32, n: u32) -> u32 {
    load_bitmap(hi, n, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadBitmapW(hi: u32, n: u32) -> u32 {
    load_bitmap(hi, n, true)
}

/// LoadImage(hinst, name, type, cx, cy, flags): IMAGE_BITMAP 0,
/// IMAGE_ICON 1, IMAGE_CURSOR 2, from the resources (LR_LOADFROMFILE
/// 0x10: a .bmp file).
fn load_image(hi: u32, n: u32, ty: u32, fl: u32, wide: bool) -> u32 {
    if fl & 0x10 != 0 && ty == 0 {
        let Some(path) = str_arg(n, wide) else { return 0 };
        return bmp_file(&path);
    }
    match ty {
        0 => load_bitmap(hi, n, wide),
        1 => load_icon(hi, n, false, wide),
        2 => load_icon(hi, n, true, wide),
        _ => 0,
    }
}

/// A .bmp file (BITMAPFILEHEADER, then a DIB) as a bitmap.
fn bmp_file(path: &[u8]) -> u32 {
    let z = crate::z(path);
    let mut f = 0u32;
    let mut action = 0;
    if unsafe { os2::DosOpen(z.as_ptr(), &mut f, &mut action, 0, 0, 0x0001, 0x0040, core::ptr::null_mut()) } != 0 {
        return 0;
    }
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { os2::DosRead(f, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(f) };
    if data.len() < 14 || &data[..2] != b"BM" {
        return 0;
    }
    crate::bitmap::from_dib(&data[14..]) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadImageA(hi: u32, n: u32, ty: u32, _cx: i32, _cy: i32, fl: u32) -> u32 {
    load_image(hi, n, ty, fl, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadImageW(hi: u32, n: u32, ty: u32, _cx: i32, _cy: i32, fl: u32) -> u32 {
    load_image(hi, n, ty, fl, true)
}

/// CopyImage: a bitmap is copied; icons and cursors are shared.
#[unsafe(no_mangle)]
pub extern "stdcall" fn CopyImage(h: u32, ty: u32, _cx: i32, _cy: i32, _fl: u32) -> u32 {
    if ty == 0 { crate::bitmap::copy_bitmap(h as u16) as u32 } else { h }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DestroyIcon(h: u32) -> i32 {
    crate::icon::destroy(h);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DestroyCursor(h: u32) -> i32 {
    crate::icon::destroy(h);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CopyIcon(h: u32) -> u32 {
    h
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateIcon(_hi: u32, w: i32, h: i32, planes: u32, bpp: u32, and: *const u8, xor: *const u8) -> u32 {
    crate::icon::create(w, h, planes as u16, bpp as u16, and, xor) as u32
}

/// CreateIconIndirect(ICONINFO: fIcon, xHotspot, yHotspot, hbmMask,
/// hbmColor).
#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateIconIndirect(p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let (mask, color) = (rd32(p, 12) as u16, rd32(p, 16) as u16);
    let Some((_, w, h, _)) = crate::bitmap::bitmap_of(mask) else { return 0 };
    // A monochrome icon has the mask over the image in one bitmap.
    let (and, xor, hh) = if color == 0 {
        let a = crate::bitmap::new_bitmap(w, h / 2, 1);
        let x = crate::bitmap::new_bitmap(w, h / 2, 1);
        copy_part(mask, a, 0, w, h / 2);
        copy_part(mask, x, h / 2, w, h / 2);
        (a, x, h / 2)
    } else {
        (crate::bitmap::copy_bitmap(mask), crate::bitmap::copy_bitmap(color), h)
    };
    let cursor = rd32(p, 0) == 0;
    let hot = if cursor { (rd32(p, 4) as i32, rd32(p, 8) as i32) } else { (w / 2, hh / 2) };
    crate::icon::add(crate::icon::Icon { w, h: hh, hot, and, xor, cursor, pm: 0 }) as u32
}

fn copy_part(from: u16, to: u16, y: i32, w: i32, h: i32) {
    let (a, b) = (crate::bitmap::create_memory_dc(), crate::bitmap::create_memory_dc());
    crate::bitmap::select(a, from);
    crate::bitmap::select(b, to);
    crate::bitmap::blit(b, 0, 0, w, h, a, 0, y, 0x00cc_0020);
    crate::bitmap::deselect(a);
    crate::bitmap::deselect(b);
    gdi::release_dc(a);
    gdi::release_dc(b);
}

/// GetIconInfo: ICONINFO with copies of the mask and image.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetIconInfo(h: u32, p: u32) -> i32 {
    let Some(i) = crate::icon::get(h) else { return 0 };
    let (w, ih, hot, cursor, and, xor) = (i.w, i.h, i.hot, i.cursor, i.and, i.xor);
    let _ = (w, ih);
    wr32(p, 0, !cursor as u32);
    wr32(p, 4, hot.0 as u32);
    wr32(p, 8, hot.1 as u32);
    wr32(p, 12, crate::bitmap::copy_bitmap(and) as u32);
    wr32(p, 16, crate::bitmap::copy_bitmap(xor) as u32);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LookupIconIdFromDirectoryEx(_p: u32, _f: i32, _cx: i32, _cy: i32, _fl: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateIconFromResourceEx(p: *const u8, n: u32, icon: i32, _v: u32, _cx: i32, _cy: i32, _fl: u32) -> u32 {
    if p.is_null() {
        return 0;
    }
    let d = unsafe { core::slice::from_raw_parts(p, n as usize) };
    if icon != 0 {
        crate::icon::from_image(d, (0, 0), false) as u32
    } else {
        let hot = (crate::res::w16(d, 0) as i32, crate::res::w16(d, 2) as i32);
        d.get(4..).map_or(0, |x| crate::icon::from_image(x, hot, true) as u32)
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateIconFromResource(p: *const u8, n: u32, icon: i32, v: u32) -> u32 {
    CreateIconFromResourceEx(p, n, icon, v, 0, 0, 0)
}

// ---- Menus ----

fn menu_name(hi: u32, n: u32, wide: bool) -> u32 {
    unsafe { Name::of(n, wide) }.map_or(0, |n| crate::menu::load(hi, &n) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadMenuA(hi: u32, n: u32) -> u32 {
    menu_name(hi, n, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadMenuW(hi: u32, n: u32) -> u32 {
    menu_name(hi, n, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadMenuIndirectA(p: *const u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    crate::menu::from_template(unsafe { core::slice::from_raw_parts(p, 0x10000) }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadMenuIndirectW(p: *const u8) -> u32 {
    LoadMenuIndirectA(p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateMenu() -> u32 {
    crate::menu::create(true) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreatePopupMenu() -> u32 {
    crate::menu::create(false) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DestroyMenu(h: u32) -> i32 {
    let pm = crate::menu::pm_of(h as u16);
    if pm == 0 {
        return 0;
    }
    crate::menu::forget_tree(pm);
    unsafe { WinDestroyWindow(pm) };
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenu(h: u32) -> u32 {
    wnd(hw(h)).map_or(0, |w| w.menu)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMenu(h: u32, m: u32) -> i32 {
    crate::menu::set_menu(hw(h), m) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSubMenu(h: u32, pos: i32) -> u32 {
    let pm = crate::menu::pm_of(h as u16);
    let id = crate::menu::send(pm, crate::menu::MM_ITEMIDFROMPOSITION, pos as u32, 0);
    if pm == 0 || id == crate::menu::MIT_NONE {
        return 0;
    }
    crate::menu::query(pm, id as u16).map_or(0, |mi| if mi.sub != 0 { crate::menu::handle_of(mi.sub) as u32 } else { 0 })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSystemMenu(h: u32, revert: i32) -> u32 {
    if revert != 0 {
        return 0;
    }
    crate::menu::system_menu(hw(h)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DrawMenuBar(h: u32) -> i32 {
    crate::menu::draw_bar(hw(h));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AppendMenuA(h: u32, fl: u32, id: u32, t: u32) -> i32 {
    crate::menu::insert(h as u16, -1, fl as u16, id as u16, &crate::menu::item_text(fl, t, false)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AppendMenuW(h: u32, fl: u32, id: u32, t: u32) -> i32 {
    crate::menu::insert(h as u16, -1, fl as u16, id as u16, &crate::menu::item_text(fl, t, true)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InsertMenuA(h: u32, pos: u32, fl: u32, id: u32, t: u32) -> i32 {
    crate::menu::insert_menu(h as u16, pos, fl, id, &crate::menu::item_text(fl, t, false)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InsertMenuW(h: u32, pos: u32, fl: u32, id: u32, t: u32) -> i32 {
    crate::menu::insert_menu(h as u16, pos, fl, id, &crate::menu::item_text(fl, t, true)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ModifyMenuA(h: u32, pos: u32, fl: u32, id: u32, t: u32) -> i32 {
    crate::menu::modify_menu(h as u16, pos, fl, id, &crate::menu::item_text(fl, t, false)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ModifyMenuW(h: u32, pos: u32, fl: u32, id: u32, t: u32) -> i32 {
    crate::menu::modify_menu(h as u16, pos, fl, id, &crate::menu::item_text(fl, t, true)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RemoveMenu(h: u32, pos: u32, fl: u32) -> i32 {
    crate::menu::remove(h as u16, pos as u16, fl as u16, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DeleteMenu(h: u32, pos: u32, fl: u32) -> i32 {
    crate::menu::remove(h as u16, pos as u16, fl as u16, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CheckMenuItem(h: u32, id: u32, fl: u32) -> u32 {
    crate::menu::set_attr(h as u16, id as u16, fl as u16, crate::menu::MIA_CHECKED, fl & crate::menu::MF_CHECKED as u32 != 0)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnableMenuItem(h: u32, id: u32, fl: u32) -> u32 {
    crate::menu::set_attr(h as u16, id as u16, fl as u16, crate::menu::MIA_DISABLED, fl & (crate::menu::MF_GRAYED | crate::menu::MF_DISABLED) as u32 != 0)
}

/// CheckMenuRadioItem(hmenu, first, last, check, flags).
#[unsafe(no_mangle)]
pub extern "stdcall" fn CheckMenuRadioItem(h: u32, first: u32, last: u32, check: u32, fl: u32) -> i32 {
    for i in first..=last {
        crate::menu::set_attr(h as u16, i as u16, fl as u16, crate::menu::MIA_CHECKED, i == check);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuItemCount(h: u32) -> i32 {
    let pm = crate::menu::pm_of(h as u16);
    if pm == 0 { -1 } else { crate::menu::send(pm, crate::menu::MM_QUERYITEMCOUNT, 0, 0) as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuItemID(h: u32, pos: i32) -> u32 {
    let pm = crate::menu::pm_of(h as u16);
    let id = crate::menu::send(pm, crate::menu::MM_ITEMIDFROMPOSITION, pos as u32, 0);
    if pm == 0 || id == crate::menu::MIT_NONE || id >= 0xf800 { 0xffff_ffff } else { id }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuState(h: u32, id: u32, fl: u32) -> u32 {
    let pm = crate::menu::pm_of(h as u16);
    match crate::menu::item_id(pm, id as u16, fl as u16) {
        Some(i) => crate::menu::state(crate::menu::holder(pm, i).unwrap_or(pm), i),
        None => 0xffff_ffff,
    }
}

fn menu_string(h: u32, which: u32, buf: u32, max: i32, fl: u32, wide: bool) -> i32 {
    let pm = crate::menu::pm_of(h as u16);
    let Some(id) = crate::menu::item_id(pm, which as u16, fl as u16) else { return 0 };
    let owner = crate::menu::holder(pm, id).unwrap_or(pm);
    let mut b = [0u8; 256];
    crate::menu::send(owner, crate::menu::MM_QUERYITEMTEXT, crate::menu::mp(id as u32, 255), b.as_mut_ptr() as u32);
    let t = crate::menu::win_text(unsafe { crate::cstr(b.as_ptr()) });
    if buf == 0 {
        return t.len() as i32;
    }
    out_text(&t, buf, max, wide)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuStringA(h: u32, id: u32, b: u32, m: i32, fl: u32) -> i32 {
    menu_string(h, id, b, m, fl, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuStringW(h: u32, id: u32, b: u32, m: i32, fl: u32) -> i32 {
    menu_string(h, id, b, m, fl, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuItemInfoA(h: u32, i: u32, pos: i32, p: u32) -> i32 {
    crate::menu::get_item_info(h as u16, i, pos != 0, p, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuItemInfoW(h: u32, i: u32, pos: i32, p: u32) -> i32 {
    crate::menu::get_item_info(h as u16, i, pos != 0, p, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMenuItemInfoA(h: u32, i: u32, pos: i32, p: u32) -> i32 {
    crate::menu::set_item_info(h as u16, i, pos != 0, p, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMenuItemInfoW(h: u32, i: u32, pos: i32, p: u32) -> i32 {
    crate::menu::set_item_info(h as u16, i, pos != 0, p, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InsertMenuItemA(h: u32, i: u32, pos: i32, p: u32) -> i32 {
    crate::menu::insert_item_info(h as u16, i, pos != 0, p, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InsertMenuItemW(h: u32, i: u32, pos: i32, p: u32) -> i32 {
    crate::menu::insert_item_info(h as u16, i, pos != 0, p, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsMenu(h: u32) -> i32 {
    (crate::menu::pm_of(h as u16) != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HiliteMenuItem(_h: u32, _m: u32, _i: u32, _f: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMenuDefaultItem(_h: u32, _i: u32, _p: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuDefaultItem(_h: u32, _p: u32, _f: u32) -> u32 {
    0xffff_ffff
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuCheckMarkDimensions() -> u32 {
    0x000e_000e
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMenuItemBitmaps(_h: u32, _p: u32, _f: u32, _a: u32, _b: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMenuInfo(_h: u32, _p: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuInfo(_h: u32, _p: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMenuItemRect(_h: u32, _m: u32, _i: u32, r: u32) -> i32 {
    write_rect(r, [0; 4]);
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MenuItemFromPoint(_h: u32, _m: u32, _x: i32, _y: i32) -> i32 {
    -1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EndMenu() -> i32 {
    crate::menu::end_menu();
    1
}

/// TrackPopupMenu(hmenu, flags, x, y, reserved, hwnd, rect):
/// TPM_RETURNCMD gives the command back.
#[unsafe(no_mangle)]
pub extern "stdcall" fn TrackPopupMenu(h: u32, fl: u32, x: i32, y: i32, _r: i32, hwnd: u32, _rc: u32) -> i32 {
    crate::menu::track_popup(h as u16, x, y, hw(hwnd), fl) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TrackPopupMenuEx(h: u32, fl: u32, x: i32, y: i32, hwnd: u32, _p: u32) -> i32 {
    crate::menu::track_popup(h as u16, x, y, hw(hwnd), fl) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadAcceleratorsA(hi: u32, n: u32) -> u32 {
    unsafe { Name::a(n as *const u8) }.map_or(0, |n| crate::menu::load_accel(hi, &n))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadAcceleratorsW(hi: u32, n: u32) -> u32 {
    unsafe { Name::w(n as *const u16) }.map_or(0, |n| crate::menu::load_accel(hi, &n))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateAcceleratorTableA(p: u32, n: i32) -> u32 {
    crate::menu::create_accel(p, n)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateAcceleratorTableW(p: u32, n: i32) -> u32 {
    crate::menu::create_accel(p, n)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CopyAcceleratorTableA(h: u32, p: u32, n: i32) -> i32 {
    crate::menu::copy_accel(h, p, n)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CopyAcceleratorTableW(h: u32, p: u32, n: i32) -> i32 {
    crate::menu::copy_accel(h, p, n)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DestroyAcceleratorTable(h: u32) -> i32 {
    crate::menu::destroy_accel(h) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TranslateAcceleratorA(h: u32, a: u32, p: u32) -> i32 {
    Msg::read(p).is_some_and(|m| crate::menu::translate_accel(hw(h), a, &m)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TranslateAcceleratorW(h: u32, a: u32, p: u32) -> i32 {
    TranslateAcceleratorA(h, a, p)
}

// ---- The caret ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateCaret(h: u32, bm: u32, cx: i32, cy: i32) -> i32 {
    crate::caret::create(hw(h), bm == 1, cx, cy);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DestroyCaret() -> i32 {
    crate::caret::destroy();
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetCaretPos(x: i32, y: i32) -> i32 {
    crate::caret::set_pos(x, y);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCaretPos(p: u32) -> i32 {
    match crate::caret::CARET.get().as_ref() {
        Some(c) => {
            write_point(p, (c.x, c.y));
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ShowCaret(h: u32) -> i32 {
    crate::caret::show(hw(h), true);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HideCaret(h: u32) -> i32 {
    crate::caret::show(hw(h), false);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetCaretBlinkTime(ms: u32) -> i32 {
    *crate::caret::BLINK.get() = ms;
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCaretBlinkTime() -> u32 {
    *crate::caret::BLINK.get()
}

// ---- The clipboard: text, on PM's ----

static CLIP_OWNER: crate::Global<u32> = crate::Global::new(0);

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenClipboard(h: u32) -> i32 {
    *CLIP_OWNER.get() = h;
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CloseClipboard() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EmptyClipboard() -> i32 {
    crate::caret::set_clip_text(b"") as i32
}

/// GetClipboardData: CF_TEXT 1, CF_OEMTEXT 7 and CF_UNICODETEXT 13, in a
/// global block.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipboardData(f: u32) -> u32 {
    if !matches!(f, 1 | 7 | 13) {
        return 0;
    }
    let Some(t) = crate::caret::clip_text() else { return 0 };
    if f == 13 {
        let w = crate::zw(&crate::to_wide(&t));
        let g = crate::kern::global_alloc(0x40, 2 * w.len() as u32);
        if g != 0 {
            unsafe { core::ptr::copy_nonoverlapping(w.as_ptr(), g as *mut u16, w.len()) };
        }
        return g;
    }
    let z = crate::z(&t);
    let g = crate::kern::global_alloc(0x40, z.len() as u32);
    if g != 0 {
        unsafe { core::ptr::copy_nonoverlapping(z.as_ptr(), g as *mut u8, z.len()) };
    }
    g
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetClipboardData(f: u32, g: u32) -> u32 {
    if g == 0 {
        return 0;
    }
    let n = crate::kern::global_size(g) as usize;
    let t = match f {
        1 | 7 => {
            let b = unsafe { core::slice::from_raw_parts(g as *const u8, n) };
            b[..b.iter().position(|&c| c == 0).unwrap_or(n)].to_vec()
        }
        13 => {
            let w = unsafe { core::slice::from_raw_parts(g as *const u16, n / 2) };
            crate::narrow(&w[..w.iter().position(|&c| c == 0).unwrap_or(n / 2)])
        }
        _ => return g,
    };
    crate::caret::set_clip_text(&t);
    g
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsClipboardFormatAvailable(f: u32) -> i32 {
    (matches!(f, 1 | 7 | 13) && crate::caret::clip_text().is_some_and(|t| !t.is_empty())) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CountClipboardFormats() -> i32 {
    if IsClipboardFormatAvailable(1) != 0 { 3 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumClipboardFormats(f: u32) -> u32 {
    if IsClipboardFormatAvailable(1) == 0 {
        return 0;
    }
    match f {
        0 => 1,
        1 => 13,
        13 => 7,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPriorityClipboardFormat(p: u32, n: i32) -> i32 {
    for i in 0..n.max(0) as u32 {
        let f = rd32(p, 4 * i);
        if IsClipboardFormatAvailable(f) != 0 {
            return f as i32;
        }
    }
    -1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterClipboardFormatA(n: u32) -> u32 {
    register_message(far_str(n).unwrap_or(b""))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterClipboardFormatW(n: u32) -> u32 {
    register_message(&wide_str(n).unwrap_or_default())
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipboardFormatNameA(_f: u32, _b: u32, _m: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipboardFormatNameW(_f: u32, _b: u32, _m: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipboardOwner() -> u32 {
    *CLIP_OWNER.get()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetOpenClipboardWindow() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetClipboardViewer(_h: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChangeClipboardChain(_a: u32, _b: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AddClipboardFormatListener(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RemoveClipboardFormatListener(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipboardSequenceNumber() -> u32 {
    1
}

// ---- Scroll bars ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetScrollPos(h: u32, bar: i32, pos: i32, _rd: i32) -> i32 {
    crate::scroll::set_pos(hw(h), bar as u16, pos)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetScrollPos(h: u32, bar: i32) -> i32 {
    crate::scroll::get(hw(h), bar as u16).map_or(0, |b| b.2)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetScrollRange(h: u32, bar: i32, lo: i32, hi: i32, _rd: i32) -> i32 {
    crate::scroll::set_range(hw(h), bar as u16, lo, hi);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetScrollRange(h: u32, bar: i32, lo: u32, hi: u32) -> i32 {
    let (a, b) = crate::scroll::get(hw(h), bar as u16).map_or((0, 0), |b| (b.0, b.1));
    if lo != 0 {
        wr32(lo, 0, a as u32);
    }
    if hi != 0 {
        wr32(hi, 0, b as u32);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetScrollInfo(h: u32, bar: i32, si: u32, _rd: i32) -> i32 {
    crate::scroll::set_info(hw(h), bar as u16, si)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetScrollInfo(h: u32, bar: i32, si: u32) -> i32 {
    crate::scroll::get_info(hw(h), bar as u16, si) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ShowScrollBar(h: u32, bar: i32, show: i32) -> i32 {
    crate::scroll::show(hw(h), bar as u16, show != 0);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnableScrollBar(h: u32, bar: u32, fl: u32) -> i32 {
    crate::scroll::enable(hw(h), bar as u16, fl as u16) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetScrollBarInfo(_h: u32, _o: i32, _p: u32) -> i32 {
    0
}

// ---- Window stations, desktops and the rest ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetProcessWindowStation() -> u32 {
    0x00ed_0001
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetThreadDesktop(_t: u32) -> u32 {
    0x00ed_0002
}

/// GetUserObjectInformation(h, UOI_FLAGS 1 / UOI_NAME 2): WSF_VISIBLE
/// for the window station ("WinSta0"), "Default" for the desktop.
fn user_object(h: u32, i: u32, p: u32, n: u32, need: u32, wide: bool) -> i32 {
    match i {
        1 => {
            if n < 12 {
                return 0;
            }
            wr32(p, 0, 0);
            wr32(p, 4, 0);
            wr32(p, 8, 1);
            if need != 0 {
                wr32(need, 0, 12);
            }
            1
        }
        2 => {
            let name: &[u8] = if h == 0x00ed_0001 { b"WinSta0" } else { b"Default" };
            let len = (name.len() as u32 + 1) * if wide { 2 } else { 1 };
            if need != 0 {
                wr32(need, 0, len);
            }
            if n < len || p == 0 {
                return crate::fail(122, 0);
            }
            out_text(name, p, name.len() as i32 + 1, wide);
            1
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetUserObjectInformationA(h: u32, i: u32, p: u32, n: u32, need: u32) -> i32 {
    user_object(h, i, p, n, need, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetUserObjectInformationW(h: u32, i: u32, p: u32, n: u32, need: u32) -> i32 {
    user_object(h, i, p, n, need, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenDesktopA(_n: u32, _f: u32, _i: i32, _a: u32) -> u32 {
    0x00ed_0002
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenDesktopW(_n: u32, _f: u32, _i: i32, _a: u32) -> u32 {
    0x00ed_0002
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OpenInputDesktop(_f: u32, _i: i32, _a: u32) -> u32 {
    0x00ed_0002
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CloseDesktop(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CloseWindowStation(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetThreadDesktop(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SwitchDesktop(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExitWindowsEx(_f: u32, _r: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsGUIThread(_c: i32) -> i32 {
    (crate::queue_of(crate::tid()).is_some_and(|q| q.hmq != 0)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetGUIThreadInfo(_t: u32, p: u32) -> i32 {
    if p == 0 {
        return 0;
    }
    wr32(p, 8, GetActiveWindow());
    wr32(p, 12, GetFocus());
    wr32(p, 16, GetCapture());
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetProcessDPIAware() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsProcessDPIAware() -> i32 {
    1
}

/// GetMonitorInfo(hmonitor, MONITORINFO(EX)): the screen.
#[unsafe(no_mangle)]
pub extern "stdcall" fn MonitorFromWindow(_h: u32, _f: u32) -> u32 {
    0x00ee_0010
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MonitorFromPoint(_x: i32, _y: i32, _f: u32) -> u32 {
    0x00ee_0010
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn MonitorFromRect(_r: u32, _f: u32) -> u32 {
    0x00ee_0010
}

fn monitor_info(p: u32, wide: bool) -> i32 {
    if p == 0 {
        return 0;
    }
    let (cx, cy) = wnd::screen_size();
    write_rect(p + 4, [0, 0, cx, cy]);
    write_rect(p + 20, [0, 0, cx, cy]);
    wr32(p, 36, 1);
    if rd32(p, 0) >= 72 {
        out_text(b"\\\\.\\DISPLAY1", p + 40, 32, wide);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMonitorInfoA(_m: u32, p: u32) -> i32 {
    monitor_info(p, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMonitorInfoW(_m: u32, p: u32) -> i32 {
    monitor_info(p, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDisplayMonitors(hdc: u32, _r: u32, f: u32, lp: u32) -> i32 {
    let (cx, cy) = wnd::screen_size();
    let r = [0i32, 0, cx, cy];
    crate::call4(f, 0x00ee_0010, hdc, r.as_ptr() as u32, lp);
    1
}

/// EnumDisplaySettings(device, mode, DEVMODE): the screen's mode
/// (dmBitsPerPel at 104, dmPelsWidth 108, dmPelsHeight 112,
/// dmDisplayFrequency 120 in DEVMODEA; 32 more for DEVMODEW's names).
fn display_settings(n: u32, p: u32, wide: bool) -> i32 {
    if n != 0xffff_ffff && n != 0xffff_fffe && n != 0 || p == 0 {
        return 0;
    }
    let o = if wide { 32 * 2 - 32 } else { 0 };
    let (cx, cy) = wnd::screen_size();
    wr32(p, o + 40, 0x0004_0000 | 0x0008_0000 | 0x0010_0000 | 0x0040_0000);
    wr32(p, o + 104, if crate::bitmap::display_bits() == 8 { 8 } else { 32 });
    wr32(p, o + 108, cx as u32);
    wr32(p, o + 112, cy as u32);
    wr32(p, o + 120, 60);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDisplaySettingsA(_d: u32, n: u32, p: u32) -> i32 {
    display_settings(n, p, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDisplaySettingsW(_d: u32, n: u32, p: u32) -> i32 {
    display_settings(n, p, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDisplaySettingsExA(_d: u32, n: u32, p: u32, _f: u32) -> i32 {
    display_settings(n, p, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDisplaySettingsExW(_d: u32, n: u32, p: u32, _f: u32) -> i32 {
    display_settings(n, p, true)
}

/// ChangeDisplaySettings: DISP_CHANGE_FAILED (-1) for a new mode.
#[unsafe(no_mangle)]
pub extern "stdcall" fn ChangeDisplaySettingsA(p: u32, _f: u32) -> i32 {
    if p == 0 { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChangeDisplaySettingsW(p: u32, _f: u32) -> i32 {
    if p == 0 { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDisplayDevicesA(_d: u32, n: u32, p: u32, _f: u32) -> i32 {
    if n != 0 || p == 0 {
        return 0;
    }
    crate::copy_out(b"\\\\.\\DISPLAY1", (p + 4) as *mut u8, 32);
    crate::copy_out(b"rs2 display", (p + 36) as *mut u8, 128);
    wr32(p, 164, 0x5);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumDisplayDevicesW(_d: u32, n: u32, p: u32, _f: u32) -> i32 {
    if n != 0 || p == 0 {
        return 0;
    }
    crate::copy_out_w(&crate::to_wide(b"\\\\.\\DISPLAY1"), (p + 4) as *mut u16, 32);
    crate::copy_out_w(&crate::to_wide(b"rs2 display"), (p + 68) as *mut u16, 128);
    wr32(p, 324, 0x5);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowContextHelpId(_h: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowContextHelpId(_h: u32, _i: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WinHelpA(_h: u32, _f: u32, _c: u32, _d: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WinHelpW(_h: u32, _f: u32, _c: u32, _d: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterHotKey(_h: u32, _i: i32, _m: u32, _v: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnregisterHotKey(_h: u32, _i: i32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetLayeredWindowAttributes(_h: u32, _k: u32, _a: u8, _f: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UpdateLayeredWindow(_h: u32, _d: u32, _p: u32, _s: u32, _sd: u32, _sp: u32, _k: u32, _b: u32, _f: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterDeviceNotificationA(_h: u32, _f: u32, _fl: u32) -> u32 {
    0x00ee_0020
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterDeviceNotificationW(_h: u32, _f: u32, _fl: u32) -> u32 {
    0x00ee_0020
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnregisterDeviceNotification(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RegisterRawInputDevices(_p: u32, _n: u32, _s: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSystemMenuDummy() -> u32 {
    0
}
