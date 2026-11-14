//! Accelerator tables. The message loop translates WM_CHAR messages as it
//! takes them off the queue: the tables of the frames containing the
//! focus window, then the queue's table, then the system keys (F10 menu,
//! Alt+F4 close, Alt+Space / Shift+Esc system menu, Alt+F5/F9/F10/F11
//! restore, minimize, maximize, hide).

#![allow(non_snake_case)]

use alloc::vec::Vec;

use crate::ctl::{self, mp, Key, CMDSRC_ACCELERATOR, WM_COMMAND, WM_HELP};
use crate::frame::{self, FID_MENU, SC_APPMENU, SC_CLOSE, SC_SYSMENU};
use crate::res::{self, Accel};
use crate::state::{st, QMsg};
use crate::win::{self, WM_CHAR, WM_SYSCOMMAND};

const AF_CHAR: u16 = 0x0001;
const AF_VIRTUALKEY: u16 = 0x0002;
const AF_SCANCODE: u16 = 0x0004;
const AF_SHIFT: u16 = 0x0008;
const AF_CONTROL: u16 = 0x0010;
const AF_ALT: u16 = 0x0020;
const AF_SYSCOMMAND: u16 = 0x0100;
const AF_HELP: u16 = 0x0200;
const VK_F1: u32 = 0x20;

const SYSTEM: [Accel; 9] = [
    Accel { fs: AF_VIRTUALKEY | AF_HELP, key: VK_F1 as u16, cmd: 0 },
    Accel { fs: AF_VIRTUALKEY | AF_SYSCOMMAND, key: ctl::VK_F10 as u16, cmd: SC_APPMENU as u16 },
    Accel { fs: AF_VIRTUALKEY | AF_ALT | AF_SYSCOMMAND, key: 0x23, cmd: SC_CLOSE as u16 },
    Accel { fs: AF_VIRTUALKEY | AF_ALT | AF_SYSCOMMAND, key: ctl::VK_SPACE as u16, cmd: SC_SYSMENU as u16 },
    Accel { fs: AF_VIRTUALKEY | AF_SHIFT | AF_SYSCOMMAND, key: ctl::VK_ESC as u16, cmd: SC_SYSMENU as u16 },
    // Alt+F5 restore, Alt+F9 minimize, Alt+F10 maximize, Alt+F11 hide.
    Accel { fs: AF_VIRTUALKEY | AF_ALT | AF_SYSCOMMAND, key: 0x24, cmd: frame::SC_RESTORE as u16 },
    Accel { fs: AF_VIRTUALKEY | AF_ALT | AF_SYSCOMMAND, key: 0x28, cmd: frame::SC_MINIMIZE as u16 },
    Accel { fs: AF_VIRTUALKEY | AF_ALT | AF_SYSCOMMAND, key: 0x29, cmd: frame::SC_MAXIMIZE as u16 },
    Accel { fs: AF_VIRTUALKEY | AF_ALT | AF_SYSCOMMAND, key: 0x2a, cmd: frame::SC_HIDE as u16 },
];

fn table(h: u32) -> Option<&'static Vec<Accel>> {
    st().accels.get((h as usize).checked_sub(1)?)?.as_ref().map(|t| &t.keys)
}

fn add(keys: Vec<Accel>) -> u32 {
    let t = crate::state::AccelTable { pid: crate::state::pid(), keys };
    let s = st();
    match s.accels.iter().position(|a| a.is_none()) {
        Some(i) => {
            s.accels[i] = Some(t);
            i as u32 + 1
        }
        None => {
            s.accels.push(Some(t));
            s.accels.len() as u32
        }
    }
}

/// Does an accelerator match the key of a WM_CHAR?
fn matches(a: &Accel, mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.ctrl() != (a.fs & AF_CONTROL != 0) || k.alt() != (a.fs & AF_ALT != 0) {
        return false;
    }
    let shift_ok = k.shift() == (a.fs & AF_SHIFT != 0);
    if a.fs & AF_VIRTUALKEY != 0 {
        return shift_ok && k.vk == a.key as u32;
    }
    if a.fs & AF_SCANCODE != 0 {
        return shift_ok && (mp1 >> 24) == a.key as u32;
    }
    if a.fs & AF_CHAR != 0 || a.fs & (AF_VIRTUALKEY | AF_SCANCODE) == 0 {
        // The character carries Shift itself, unless AF_SHIFT asks for it.
        if a.fs & AF_SHIFT != 0 && !k.shift() {
            return false;
        }
        let key = a.key as u8;
        if k.ch != 0 && (k.ch == key || (k.ctrl() || k.alt()) && k.ch.eq_ignore_ascii_case(&key)) {
            return true;
        }
        return k.ch == 0 && key.is_ascii_alphanumeric() && k.letter(mp1) == key.to_ascii_lowercase();
    }
    false
}

/// A menu item for the command exists in the frame's menu and is
/// disabled.
fn disabled_in_menu(frame: u32, cmd: u32) -> bool {
    let m = win::WinWindowFromID(frame, FID_MENU);
    m != 0 && crate::menu::item_disabled(m, cmd)
}

/// Translate with one table; the command goes to `target`.
fn apply(t: &[Accel], target: u32, m: &mut QMsg) -> bool {
    let Some(a) = t.iter().find(|a| matches(a, m.mp1, m.mp2)) else { return false };
    if a.fs & (AF_SYSCOMMAND | AF_HELP) == 0 && disabled_in_menu(target, a.cmd as u32) {
        return false;
    }
    m.hwnd = target;
    // As IBM's TranslateAccel: WM_HELP goes to the focus window (and up
    // through WinDefWindowProc to the frame), so a control on the way (a
    // notebook: BKN_HELP) or the program's own window sees it first.
    if a.fs & AF_HELP != 0 {
        let f = st().focus;
        if f != 0 && st().desk.exists(f) {
            m.hwnd = f;
        }
    }
    m.msg = if a.fs & AF_SYSCOMMAND != 0 {
        WM_SYSCOMMAND
    } else if a.fs & AF_HELP != 0 {
        WM_HELP
    } else {
        WM_COMMAND
    };
    m.mp1 = a.cmd as u32;
    m.mp2 = mp(CMDSRC_ACCELERATOR, 0);
    true
}

/// Is the key Alt+letter for an item of the frame's menu?
fn mnemonic_of(frame: u32, m: &QMsg) -> bool {
    let k = Key::from(m.mp1, m.mp2);
    if !k.alt() || k.ctrl() || k.up() {
        return false;
    }
    let c = k.letter(m.mp1);
    let bar = win::WinWindowFromID(frame, FID_MENU);
    c != 0 && bar != 0 && crate::menu::has_mnemonic(bar, c)
}

/// WM_TRANSLATEACCEL (mp1 the QMSG to translate in place).
pub const WM_TRANSLATEACCEL: u32 = 0x004b;

/// Translate a WM_CHAR taken off the queue into a command, if it is an
/// accelerator. As IBM's XAccelTranslate (in XScanSysQueue): the window
/// the key is for gets WM_TRANSLATEACCEL with a copy of the QMSG, which
/// replaces the message when it answers TRUE. WinDefWindowProc passes it
/// up to the parent; each frame on the way tries its table, the
/// mnemonics of its action bar and the queue's table (frame_translate),
/// and above the top-level window come the system keys (def_translate).
/// A program's own procedure can keep a key from being translated (the
/// Open Watcom GUI library answers FALSE for F10, which its programs
/// use themselves).
pub fn translate(m: &mut QMsg) -> bool {
    if m.msg != WM_CHAR || m.mp1 & ctl::KC_KEYUP != 0 || crate::state::menu_mode().is_some() {
        return false;
    }
    if m.hwnd == 0 || !st().desk.exists(m.hwnd) {
        return false;
    }
    let mut copy = *m;
    if crate::msg::send(m.hwnd, WM_TRANSLATEACCEL, &mut copy as *mut QMsg as u32, 0) == 0 {
        return false;
    }
    *m = copy;
    true
}

fn qmsg_at(pq: u32) -> Option<QMsg> {
    (pq != 0).then(|| unsafe { (pq as *const QMsg).read_unaligned() })
}

/// A frame's WM_TRANSLATEACCEL, as IBM's FrameTranslateAccel: the
/// frame's accelerator table, then an Alt+letter mnemonic of its action
/// bar (IBM's frame passes it to FID_MENU, whose MenuTranslateMnemonic
/// opens the item and empties the QMSG), then the queue's table; each
/// command goes to this frame.
pub fn frame_translate(frame: u32, pq: u32) -> u32 {
    let Some(mut m) = qmsg_at(pq) else { return 0 };
    if m.msg != WM_CHAR {
        return 0;
    }
    let done = |m: &QMsg| {
        unsafe { (pq as *mut QMsg).write_unaligned(*m) };
        1
    };
    let a = st().frames.get(&frame).map_or(0, |f| f.accel);
    if let Some(t) = table(a) {
        if apply(&t.clone(), frame, &mut m) {
            return done(&m);
        }
    }
    if mnemonic_of(frame, &m) {
        let k = Key::from(m.mp1, m.mp2);
        crate::menu::open_mnemonic(win::WinWindowFromID(frame, FID_MENU), k.letter(m.mp1));
        let t = (m.time, m.x, m.y, m.reserved);
        m = QMsg { hwnd: 0, msg: 0, mp1: 0, mp2: 0, time: t.0, x: t.1, y: t.2, reserved: t.3 };
        return done(&m);
    }
    let qa = st().my_queue_mut().map_or(0, |q| q.accel);
    if let Some(t) = table(qa) {
        if apply(&t.clone(), frame, &mut m) {
            return done(&m);
        }
    }
    0
}

/// WinDefWindowProc's WM_TRANSLATEACCEL: to the parent, as IBM's; a
/// top-level window has the queue's table (for one that is not a
/// frame) and then the system keys.
pub fn def_translate(h: u32, pq: u32) -> u32 {
    let parent = ctl::parent(h);
    if parent != 0 && parent != pmwin::HWND_DESKTOP && parent != pmwin::HWND_OBJECT {
        return crate::msg::send(parent, WM_TRANSLATEACCEL, pq, 0);
    }
    let Some(mut m) = qmsg_at(pq) else { return 0 };
    if m.msg != WM_CHAR {
        return 0;
    }
    let is_frame = frame::is_frame(h);
    let qa = st().my_queue_mut().map_or(0, |q| q.accel);
    let hit = match table(qa) {
        Some(t) if !is_frame => apply(&t.clone(), h, &mut m),
        _ => false,
    } || is_frame && apply(&SYSTEM, h, &mut m);
    if hit {
        unsafe { (pq as *mut QMsg).write_unaligned(m) };
    }
    hit as u32
}

/// WinLoadAccelTable(hab, hmod, idAccelTable)
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadAccelTable(_hab: u32, hmod: u32, id: u32) -> u32 {
    match res::load(hmod, res::RT_ACCELTABLE, id) {
        Some(b) => add(res::parse_accel(b)),
        None => 0,
    }
}

/// WinCreateAccelTable(hab, pAccelTable): ACCELTABLE has the resource
/// layout (count, code page, entries).
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateAccelTable(_hab: u32, p: *const u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let n = unsafe { (p as *const u16).read_unaligned() } as usize;
    let b = unsafe { core::slice::from_raw_parts(p, 4 + n * 6) };
    add(res::parse_accel(b))
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyAccelTable(h: u32) -> u32 {
    match st().accels.get_mut((h as usize).wrapping_sub(1)) {
        Some(slot @ Some(_)) => {
            *slot = None;
            1
        }
        _ => 0,
    }
}

/// WinSetAccelTable(hab, haccel, hwndFrame): hwndFrame 0 sets the
/// queue's table.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetAccelTable(_hab: u32, h: u32, frame: u32) -> u32 {
    let s = st();
    if frame == 0 {
        return match s.my_queue_mut() {
            Some(q) => {
                q.accel = h;
                1
            }
            None => 0,
        };
    }
    match s.frames.get_mut(&frame) {
        Some(f) => {
            f.accel = h;
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryAccelTable(_hab: u32, frame: u32) -> u32 {
    let s = st();
    if frame == 0 { s.my_queue_mut().map_or(0, |q| q.accel) } else { s.frames.get(&frame).map_or(0, |f| f.accel) }
}

/// WinTranslateAccel(hab, hwnd, haccel, pqmsg)
#[unsafe(no_mangle)]
pub extern "C" fn WinTranslateAccel(_hab: u32, h: u32, haccel: u32, pq: *mut QMsg) -> u32 {
    if pq.is_null() {
        return 0;
    }
    let mut m = unsafe { pq.read_unaligned() };
    if m.msg != WM_CHAR || m.mp1 & ctl::KC_KEYUP != 0 {
        return 0;
    }
    let Some(t) = table(haccel) else { return 0 };
    if !apply(&t.clone(), h, &mut m) {
        return 0;
    }
    unsafe { pq.write_unaligned(m) };
    1
}

/// WinCopyAccelTable(haccel, pAccelTable, cbCopyMax): the size of the
/// table; copies it when the buffer is given.
#[unsafe(no_mangle)]
pub extern "C" fn WinCopyAccelTable(h: u32, p: *mut u8, cb: u32) -> u32 {
    let Some(t) = table(h) else { return 0 };
    let mut b = Vec::with_capacity(4 + t.len() * 6);
    b.extend_from_slice(&(t.len() as u16).to_le_bytes());
    b.extend_from_slice(&850u16.to_le_bytes());
    for a in t {
        for v in [a.fs, a.key, a.cmd] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    if !p.is_null() {
        let n = b.len().min(cb as usize);
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, n) };
        return n as u32;
    }
    b.len() as u32
}
