//! Lazy drag (pick up and drop), as IBM's PMCTLS does it (DRGLAZY.C:
//! `DrgLazyDrag`, `DrgInputHook`, `DrgDoLazyMessage`, `DoLazyChar`,
//! `SendDragOver`, `TermLazyDrag`).
//!
//! DrgLazyDrag returns at once: the objects are picked up and the user
//! goes on working. A system input hook (in every PM process) passes what
//! matters to an object window in the source's process ("Drag Object
//! Window"): the drag button (SV_BEGINDRAG's) going down and up, and,
//! while it is down, mouse moves and keys. Held down with Ctrl (DO_COPY),
//! Shift (DO_MOVE) or both (DO_LINK) it drops: the window under the
//! pointer gets DM_DRAGOVER as the pointer moves (the source
//! DM_DRAGOVERNOTIFY) and, when the button goes up over a window that
//! takes the drop, DM_DROP. Without those keys the click is the window's
//! own: a target offers "Drop" in its pop-up menu and calls DrgLazyDrop.
//! DrgCancelLazyDrag puts the objects down. Either way the source gets
//! DM_DROPNOTIFY (the target, or 0) at the end.
//!
//! What all processes must see (the hook runs in each) is in named
//! shared memory; the object window and the drag images belong to the
//! source's process, and DrgLazyDrop and DrgCancelLazyDrag from another
//! process are done there (sent to the object window).

use alloc::vec::Vec;

use crate::*;

unsafe extern "C" {
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: extern "C" fn(u32, u32, u32, u32) -> u32, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: *const u8, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.928"]
    fn WinSetHook(hab: u32, hmq: u32, kind: u32, pfn: u32, hmod: u32) -> u32;
    #[link_name = "PMWIN.927"]
    fn WinReleaseHook(hab: u32, hmq: u32, kind: u32, pfn: u32, hmod: u32) -> u32;
    #[link_name = "PMWIN.829"]
    fn WinQuerySysValue(desk: u32, i: i32) -> i32;
}

const HWND_OBJECT: u32 = 2;
const HWND_BOTTOM: u32 = 4;
const HK_INPUT: u32 = 1;
const SV_BEGINDRAG: i32 = 0x50;
const WM_BUTTON1DOWN: u32 = 0x0071;
const WM_BUTTON2DOWN: u32 = 0x0074;
const WM_BUTTON3DOWN: u32 = 0x0077;
const DM_DROPNOTIFY: u32 = 0x031e;
const KC_CTRL: u32 = 0x0010;
const KC_SHIFT: u32 = 0x0008;

/// The object window's: input the hook passed on (IBM's 0x100C, 0x100D).
const WM_LAZY_INPUT: u32 = 0x100c;
const WM_LAZY_CHAR: u32 = 0x100d;
/// rs2's: DrgLazyDrop and DrgCancelLazyDrag from another process.
const WM_LAZY_END: u32 = 0x1020;

const PMERR_NO_LAZY_DRAG: u32 = 0x1f00;

const CLASS: &[u8] = b"Drag Object Window\0";
const SHARED_NAME: &[u8] = b"\\SHAREMEM\\PMDRAG\\LAZY.DAT\0";

// Lazy.flags (IBM's): the target takes a drop; it never will (no more
// DM_DRAGOVER until the pointer leaves it); not with this operation.
const F_DROP: u16 = 1;
const F_NEVER: u16 = 2;
const F_NOOP: u16 = 4;

/// The lazy drag, seen by every process.
#[repr(C)]
struct Lazy {
    active: u32,
    pid: u32,
    pdinfo: u32,
    obj: u32,
    target: u32,
    flags: u16,
    reply_op: u16,
    /// The operation before the keys changed it.
    saved_op: u16,
    /// Ctrl and Shift held down: 0 (the click is the window's), 1 or 2.
    keys: u16,
    /// The drag button (1-3) and whether it is down.
    button: u32,
    pressed: u32,
    last_char: u32,
    /// DrgLazyDrop's target and operation (WM_LAZY_END).
    end_target: u32,
    end_op: u16,
    end_drop: u16,
}

static mut SHARED: *mut Lazy = core::ptr::null_mut();
static mut HMOD: u32 = 0;

/// In the source's process: the images and the pointer shown.
static mut IMAGES: Vec<u8> = Vec::new();
static mut SHAPE: Shape = Shape { hptr: 0, made_of: (0, false, 0), generation: 0 };

fn images() -> &'static mut Vec<u8> {
    unsafe { &mut *core::ptr::addr_of_mut!(IMAGES) }
}

fn shape() -> &'static mut Shape {
    unsafe { &mut *core::ptr::addr_of_mut!(SHAPE) }
}

fn lz() -> Option<&'static mut Lazy> {
    unsafe { SHARED.as_mut() }
}

/// The lazy drag running (in any process).
fn running() -> Option<&'static mut Lazy> {
    lz().filter(|l| l.active != 0)
}

pub fn active() -> bool {
    running().is_some()
}

pub fn draginfo() -> u32 {
    running().map_or(0, |l| l.pdinfo)
}

/// _DLL_InitTerm: the shared block (made by the first process); a source
/// that ends drops its lazy drag.
pub fn init_term(hmod: u32, flag: u32) {
    if flag == 0 {
        unsafe { HMOD = hmod };
        let mut p: *mut u8 = core::ptr::null_mut();
        // PAG_READ | PAG_WRITE (| PAG_COMMIT to allocate).
        if unsafe { os2::DosGetNamedSharedMem(&mut p, SHARED_NAME.as_ptr(), 3) } != 0 {
            if unsafe { os2::DosAllocSharedMem(&mut p, SHARED_NAME.as_ptr(), 0x1000, 0x13) } == 0 {
                unsafe { core::ptr::write_bytes(p, 0, core::mem::size_of::<Lazy>()) };
            } else if unsafe { os2::DosGetNamedSharedMem(&mut p, SHARED_NAME.as_ptr(), 3) } != 0 {
                p = core::ptr::null_mut();
            }
        }
        unsafe { SHARED = p as *mut Lazy };
    } else if let Some(l) = running().filter(|l| l.pid == my_pid()) {
        l.active = 0;
    }
}

// ---- The calls ----

/// DrgLazyDrag(hwndSource, pdinfo, pdimg, cdimg, pRsvd)
#[unsafe(no_mangle)]
pub extern "C" fn DrgLazyDrag(src: u32, pdinfo: u32, pdimg: u32, cdimg: u32, _rsvd: u32) -> u32 {
    let Some(l) = lz() else { return 0 };
    if drag().is_some() || (l.active != 0 && l.pid != my_pid()) {
        error(PMERR_DRAG_IN_PROGRESS);
        return 0;
    }
    if pdinfo == 0 || !valid(pdinfo) {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    let cur = rd(pdinfo, DI_SOURCE);
    if cur == 0 || unsafe { WinIsWindow(0, cur) } == 0 {
        if unsafe { WinIsWindow(0, src) } == 0 {
            error(PMERR_INVALID_HWND);
            return 0;
        }
        wr(pdinfo, DI_SOURCE, src);
    }
    let source = rd(pdinfo, DI_SOURCE);
    if l.active == 0 {
        let hab = unsafe { WinQueryAnchorBlock(source) };
        unsafe { WinRegisterClass(hab, CLASS.as_ptr(), obj_proc, 0, 0) };
        let obj = unsafe { WinCreateWindow(HWND_OBJECT, CLASS.as_ptr(), core::ptr::null(), 0, 0, 0, 0, 0, source, HWND_BOTTOM, 0, core::ptr::null(), core::ptr::null()) };
        if obj == 0 {
            return 0;
        }
        l.obj = obj;
    }
    l.pid = my_pid();
    l.pdinfo = pdinfo;
    l.button = match (unsafe { WinQuerySysValue(HWND_DESKTOP, SV_BEGINDRAG) } as u32) & 0xffff {
        WM_BUTTON1DOWN => 1,
        WM_BUTTON3DOWN => 3,
        _ => 2,
    };
    l.pressed = 0;
    l.last_char = 0;
    let p = pointer_pos();
    wr16(pdinfo, DI_X, p[0] as u16);
    wr16(pdinfo, DI_Y, p[1] as u16);
    *images() = copy_images(pdimg, cdimg);
    shape().changed();
    init_operation(l);
    l.target = unsafe { WinWindowFromPoint(HWND_DESKTOP, &p, 1) };
    l.flags = 0;
    l.reply_op = 0;
    l.active = 1;
    let ok = unsafe { WinSetHook(0, 0, HK_INPUT, input_hook as *const () as u32, HMOD) } != 0;
    if !ok {
        l.active = 0;
        unsafe { WinDestroyWindow(l.obj) };
        return 0;
    }
    show_pointer(l);
    1
}

/// The operation from Ctrl and Shift as the drag starts.
fn init_operation(l: &mut Lazy) {
    let pdinfo = l.pdinfo;
    l.saved_op = rd16(pdinfo, DI_OP);
    let (ctrl, shift) = (key_down(VK_CTRL), key_down(VK_SHIFT));
    l.keys = ctrl as u16 + shift as u16;
    let op = match (ctrl, shift) {
        (true, true) => rmf::DO_LINK,
        (false, true) => rmf::DO_MOVE,
        (true, false) => rmf::DO_COPY,
        _ => rmf::DO_DEFAULT,
    };
    wr16(pdinfo, DI_OP, op);
}

/// DrgCancelLazyDrag(): the objects are put down.
#[unsafe(no_mangle)]
pub extern "C" fn DrgCancelLazyDrag() -> u32 {
    let Some(l) = running() else {
        error(PMERR_NO_LAZY_DRAG);
        return 0;
    };
    l.end_drop = 0;
    end(l);
    1
}

/// DrgLazyDrop(hwndTarget, usOperation, pptlDrop): a target drops the
/// objects on itself (its pop-up menu's Drop).
#[unsafe(no_mangle)]
pub extern "C" fn DrgLazyDrop(target: u32, op: u32, pptl: u32) -> u32 {
    let Some(l) = running() else {
        error(PMERR_NO_LAZY_DRAG);
        return 0;
    };
    let pdinfo = l.pdinfo;
    wr16(pdinfo, DI_OP, op as u16);
    if pptl != 0 {
        wr16(pdinfo, DI_X, rd(pptl, 0) as u16);
        wr16(pdinfo, DI_Y, rd(pptl, 4) as u16);
    }
    l.end_target = target;
    l.end_op = op as u16;
    l.end_drop = 1;
    end(l);
    1
}

/// End the drag in the source's process.
fn end(l: &mut Lazy) {
    if l.pid == my_pid() {
        finish(l);
    } else {
        unsafe { WinSendMsg(l.obj, WM_LAZY_END, 0, 0) };
    }
}

fn finish(l: &mut Lazy) {
    if l.end_drop != 0 {
        l.flags |= F_DROP;
        l.target = l.end_target;
    } else {
        l.flags &= !F_DROP;
        l.target = 0;
    }
    term(l, l.end_drop != 0);
}

/// DrgSetDragImage during a lazy drag (its source).
pub fn set_images(pdinfo: u32, pdimg: u32, cdimg: u32) -> bool {
    let Some(l) = running().filter(|l| l.pdinfo == pdinfo && l.pid == my_pid()) else { return false };
    if pdimg == 0 || cdimg == 0 {
        return false;
    }
    *images() = copy_images(pdimg, cdimg);
    shape().changed();
    show_pointer(l);
    true
}

// ---- The hook (in every process) ----

extern "C" fn input_hook(_hab: u32, pq: u32, _fs: u32) -> u32 {
    let Some(l) = running() else { return 0 };
    let (msg, mp1, mp2) = (rd(pq, 4), rd(pq, 8), rd(pq, 12));
    match msg {
        WM_BUTTON1DOWN | WM_BUTTON2DOWN | WM_BUTTON3DOWN | WM_BUTTON1UP | WM_BUTTON2UP | WM_BUTTON3UP => {
            unsafe { WinSendMsg(l.obj, WM_LAZY_INPUT, l.pdinfo, msg) };
        }
        WM_MOUSEMOVE if l.pressed != 0 => {
            unsafe { WinSendMsg(l.obj, WM_LAZY_INPUT, l.pdinfo, msg) };
        }
        WM_CHAR if l.pressed != 0 && mp1 != l.last_char => {
            l.last_char = mp1;
            unsafe { WinSendMsg(l.obj, WM_LAZY_CHAR, mp1, mp2) };
        }
        _ => {}
    }
    0
}

// ---- The object window (in the source's process) ----

extern "C" fn obj_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(l) = running().filter(|l| l.obj == h) else {
        return unsafe { WinDefWindowProc(h, msg, mp1, mp2) };
    };
    match msg {
        WM_LAZY_INPUT => match mp2 {
            WM_MOUSEMOVE => mouse_move(l),
            WM_BUTTON1UP | WM_BUTTON2UP | WM_BUTTON3UP | WM_ENDDRAG => button_up(l, mp2),
            WM_BUTTON1DOWN | WM_BUTTON2DOWN | WM_BUTTON3DOWN => button_down(l, mp2),
            _ => {}
        },
        WM_LAZY_CHAR => lazy_char(l, mp1),
        WM_LAZY_END => finish(l),
        _ => return unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
    0
}

fn mouse_move(l: &mut Lazy) {
    let p = pointer_pos();
    let under = unsafe { WinWindowFromPoint(HWND_DESKTOP, &p, 1) };
    wr16(l.pdinfo, DI_X, p[0] as u16);
    wr16(l.pdinfo, DI_Y, p[1] as u16);
    if l.target != under {
        let old = l.target;
        l.target = under;
        l.flags &= !(F_NEVER | F_NOOP | 8);
        if l.keys != 0 && old != 0 {
            unsafe { WinSendMsg(old, DM_DRAGLEAVE, l.pdinfo, 0) };
        }
    }
    if l.keys != 0 {
        if l.flags & (F_NEVER | F_NOOP) == 0 {
            send_dragover(l);
        }
        show_pointer(l);
    }
}

fn button_of(msg: u32) -> u32 {
    match msg {
        WM_BUTTON1DOWN | WM_BUTTON1UP => 1,
        WM_BUTTON2DOWN | WM_BUTTON2UP => 2,
        WM_BUTTON3DOWN | WM_BUTTON3UP => 3,
        WM_ENDDRAG => VK_ENDDRAG,
        _ => 0,
    }
}

fn button_down(l: &mut Lazy, msg: u32) {
    if button_of(msg) != l.button || key_down(VK_ALT) {
        return;
    }
    let (ctrl, shift) = (key_down(VK_CTRL), key_down(VK_SHIFT));
    let (keys, op) = match (ctrl, shift) {
        (true, true) => (2, rmf::DO_LINK),
        (true, false) => (1, rmf::DO_COPY),
        (false, true) => (1, rmf::DO_MOVE),
        _ => (0, rd16(l.pdinfo, DI_OP)),
    };
    l.keys = keys;
    wr16(l.pdinfo, DI_OP, op);
    if l.keys != 0 {
        mouse_move(l);
    }
    l.pressed = 1;
}

fn button_up(l: &mut Lazy, msg: u32) {
    if button_of(msg) != l.button {
        return;
    }
    l.pressed = 0;
    if l.keys != 0 {
        term(l, true);
    }
}

/// Ctrl and Shift going down and up while the drag button is down.
fn lazy_char(l: &mut Lazy, fs: u32) {
    if fs & KC_PREVDOWN != 0 {
        return;
    }
    if fs & KC_KEYUP == 0 {
        l.keys += 1;
    } else if l.keys != 0 {
        l.keys -= 1;
    }
    let op = match l.keys {
        0 => None,
        1 if fs & KC_CTRL != 0 => Some(rmf::DO_COPY),
        1 if fs & KC_SHIFT != 0 => Some(rmf::DO_MOVE),
        2 if fs & (KC_CTRL | KC_SHIFT) == KC_CTRL | KC_SHIFT => Some(rmf::DO_LINK),
        _ => Some(rmf::DO_UNKNOWN),
    };
    match op {
        None => {
            unsafe { WinSendMsg(l.target, DM_DRAGLEAVE, l.pdinfo, 0) };
        }
        Some(op) => {
            wr16(l.pdinfo, DI_OP, op);
            if l.flags & F_NEVER == 0 {
                send_dragover(l);
            }
        }
    }
    show_pointer(l);
}

/// DM_DRAGOVER to the target, DM_DRAGOVERNOTIFY to the source, and what
/// the answer says (IBM's SendDragOver).
fn send_dragover(l: &mut Lazy) {
    let di = l.pdinfo;
    if rd16(di, DI_OP) == rmf::DO_DEFAULT {
        wr16(di, DI_OP, l.saved_op);
    }
    let reply = unsafe { WinSendMsg(l.target, DM_DRAGOVER, di, rd(di, DI_X)) };
    unsafe { WinSendMsg(rd(di, DI_SOURCE), DM_DRAGOVERNOTIFY, di, reply) };
    let Some(l) = running() else { return };
    let (code, op) = (reply as u16, (reply >> 16) as u16);
    match code {
        DOR_DROP => {
            if rd16(di, DI_OP) == rmf::DO_DEFAULT && !matches!(op, rmf::DO_COPY | rmf::DO_MOVE | rmf::DO_LINK | 0x80) && op < rmf::DO_UNKNOWN {
                l.flags = (l.flags | F_NEVER) & !F_DROP;
                l.reply_op = 0;
            } else {
                l.flags |= F_DROP;
                l.reply_op = op;
            }
        }
        DOR_NODROP => l.flags &= !F_DROP,
        DOR_NODROPOP => l.flags = (l.flags | F_NOOP) & !F_DROP,
        _ => l.flags = (l.flags | F_NEVER) & !F_DROP,
    }
}

fn show_pointer(l: &mut Lazy) {
    let bad = l.keys != 0 && l.flags & F_DROP == 0;
    let base = unsafe { WinQuerySysPointer(HWND_DESKTOP, if bad { SPTR_ILLEGAL } else { SPTR_ARROW }, 0) };
    shape().show(base, images(), halftone(rd16(l.pdinfo, DI_OP), l.reply_op));
}

/// The end: DM_DROP (or DM_DRAGLEAVE) to the target when dropping, then
/// DM_DROPNOTIFY to the source (IBM's TermLazyDrag).
fn term(l: &mut Lazy, dropping: bool) {
    unsafe { WinReleaseHook(0, 0, HK_INPUT, input_hook as *const () as u32, HMOD) };
    let di = l.pdinfo;
    let mut target = l.target;
    if dropping && target != 0 {
        if l.flags & F_DROP == 0 {
            unsafe { WinSendMsg(target, DM_DRAGLEAVE, di, 0) };
            target = 0;
        } else {
            let op = rd16(di, DI_OP);
            if op == rmf::DO_DEFAULT || (op == rmf::DO_UNKNOWN && l.reply_op > rmf::DO_UNKNOWN) {
                wr16(di, DI_OP, l.reply_op);
            }
            unsafe { WinSendMsg(target, DM_DROP, di, 0) };
        }
    } else {
        target = 0;
    }
    let (obj, source) = (l.obj, rd(di, DI_SOURCE));
    l.active = 0;
    l.target = 0;
    l.pressed = 0;
    unsafe { WinSendMsg(source, DM_DROPNOTIFY, di, target) };
    shape().end();
    images().clear();
    unsafe { WinDestroyWindow(obj) };
}
