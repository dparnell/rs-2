//! Helpers shared by the built-in window classes: per-window data,
//! message parameter packing, notifications to the owner and the
//! registration of the classes.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::msg::send;
use crate::state::{st, Class};

pub const WM_ENABLE: u32 = 0x0004;
pub const WM_COMMAND: u32 = 0x0020;
pub const WM_HELP: u32 = 0x0022;
pub const WM_CONTROL: u32 = 0x0030;
pub const WM_VSCROLL: u32 = 0x0031;
pub const WM_HSCROLL: u32 = 0x0032;
pub const WM_INITMENU: u32 = 0x0033;
pub const WM_MENUSELECT: u32 = 0x0034;
pub const WM_MENUEND: u32 = 0x0035;
pub const WM_DRAWITEM: u32 = 0x0036;
pub const WM_MEASUREITEM: u32 = 0x0037;
pub const WM_QUERYDLGCODE: u32 = 0x003a;
pub const WM_INITDLG: u32 = 0x003b;
pub const WM_MATCHMNEMONIC: u32 = 0x003d;
pub const WM_UPDATEFRAME: u32 = 0x0042;
pub const WM_BUTTON1DBLCLK: u32 = 0x0073;

pub const CMDSRC_PUSHBUTTON: u32 = 1;
pub const CMDSRC_MENU: u32 = 2;
pub const CMDSRC_ACCELERATOR: u32 = 3;
pub const CMDSRC_OTHER: u32 = 0;

pub const DLGC_ENTRYFIELD: u32 = 0x0001;
pub const DLGC_BUTTON: u32 = 0x0002;
pub const DLGC_RADIOBUTTON: u32 = 0x0004;
pub const DLGC_STATIC: u32 = 0x0008;
pub const DLGC_DEFAULT: u32 = 0x0010;
pub const DLGC_PUSHBUTTON: u32 = 0x0020;
pub const DLGC_CHECKBOX: u32 = 0x0040;
pub const DLGC_SCROLLBAR: u32 = 0x0080;
pub const DLGC_MENU: u32 = 0x0100;

pub const WS_GROUP: u32 = 0x0001_0000;
pub const WS_TABSTOP: u32 = 0x0002_0000;

/// WM_CHAR flags and virtual keys.
pub const KC_CHAR: u32 = 0x0001;
pub const KC_VIRTUALKEY: u32 = 0x0002;
pub const KC_SHIFT: u32 = 0x0008;
pub const KC_CTRL: u32 = 0x0010;
pub const KC_ALT: u32 = 0x0020;
pub const KC_KEYUP: u32 = 0x0040;
pub const VK_BACKSPACE: u32 = 0x05;
pub const VK_TAB: u32 = 0x06;
pub const VK_BACKTAB: u32 = 0x07;
pub const VK_NEWLINE: u32 = 0x08;
pub const VK_ESC: u32 = 0x0f;
pub const VK_SPACE: u32 = 0x10;
pub const VK_PAGEUP: u32 = 0x11;
pub const VK_PAGEDOWN: u32 = 0x12;
pub const VK_END: u32 = 0x13;
pub const VK_HOME: u32 = 0x14;
pub const VK_LEFT: u32 = 0x15;
pub const VK_UP: u32 = 0x16;
pub const VK_RIGHT: u32 = 0x17;
pub const VK_DOWN: u32 = 0x18;
pub const VK_INSERT: u32 = 0x1a;
pub const VK_DELETE: u32 = 0x1b;
pub const VK_ENTER: u32 = 0x1e;
pub const VK_F10: u32 = 0x29;

/// A decoded WM_CHAR.
pub struct Key {
    pub fs: u32,
    pub ch: u8,
    pub vk: u32,
}

impl Key {
    pub fn from(mp1: u32, mp2: u32) -> Key {
        let fs = mp1 & 0xffff;
        let vk = if fs & KC_VIRTUALKEY != 0 { mp2 >> 16 } else { 0 };
        let ch = if fs & KC_CHAR != 0 { mp2 as u8 } else { 0 };
        Key { fs, ch, vk }
    }

    pub fn up(&self) -> bool {
        self.fs & KC_KEYUP != 0
    }

    pub fn shift(&self) -> bool {
        self.fs & KC_SHIFT != 0
    }

    pub fn ctrl(&self) -> bool {
        self.fs & KC_CTRL != 0
    }

    pub fn alt(&self) -> bool {
        self.fs & KC_ALT != 0
    }

    /// The letter or digit of the key (also for Alt/Ctrl combinations,
    /// which carry no character), lower case.
    pub fn letter(&self, mp1: u32) -> u8 {
        if self.ch.is_ascii_graphic() {
            return self.ch.to_ascii_lowercase();
        }
        scan_letter((mp1 >> 24) as u8)
    }
}

/// Letter or digit of a set-1 scan code.
pub fn scan_letter(scan: u8) -> u8 {
    const ROWS: [(u8, &[u8]); 4] = [(0x02, b"1234567890"), (0x10, b"qwertyuiop"), (0x1e, b"asdfghjkl"), (0x2c, b"zxcvbnm")];
    for (base, keys) in ROWS {
        if scan >= base && ((scan - base) as usize) < keys.len() {
            return keys[(scan - base) as usize];
        }
    }
    0
}

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

pub fn lo(v: u32) -> u32 {
    v & 0xffff
}

pub fn hi(v: u32) -> u32 {
    v >> 16
}

/// The data of a built-in control window.
pub fn data<T: 'static>(h: u32) -> Option<&'static mut T> {
    st().ctl.get_mut(&h)?.downcast_mut::<T>()
}

pub fn set_data<T: 'static>(h: u32, v: T) {
    st().ctl.insert(h, Box::new(v));
}

pub fn style(h: u32) -> u32 {
    st().desk.get(h).map_or(0, |w| w.style)
}

pub fn id(h: u32) -> u32 {
    st().desk.get(h).map_or(0, |w| w.id)
}

pub fn owner(h: u32) -> u32 {
    st().desk.get(h).map_or(0, |w| w.owner)
}

pub fn parent(h: u32) -> u32 {
    st().desk.get(h).map_or(0, |w| w.parent)
}

pub fn text(h: u32) -> Vec<u8> {
    st().desk.get(h).map_or(Vec::new(), |w| w.text.clone())
}

pub fn size(h: u32) -> (i32, i32) {
    st().desk.get(h).map_or((0, 0), |w| (w.cx, w.cy))
}

pub fn enabled(h: u32) -> bool {
    style(h) & pmwin::ws::DISABLED == 0
}

pub fn has_focus(h: u32) -> bool {
    st().focus == h
}

pub fn invalidate(h: u32) {
    st().desk.invalidate(h, None, false);
}

/// WM_CONTROL to the owner: mp1 = (id, code), mp2 = the control.
pub fn notify(h: u32, code: u32) -> u32 {
    send(owner(h), WM_CONTROL, mp(id(h), code), h)
}

/// Mouse position of a mouse message.
pub fn mouse_pos(mp1: u32) -> gpi::Point {
    gpi::Point::new(mp1 as i16 as i32, (mp1 >> 16) as i16 as i32)
}

/// Copy text into a caller's buffer of `cch` bytes, NUL-terminated;
/// returns the length copied.
pub fn copy_out(t: &[u8], cch: usize, buf: *mut u8) -> u32 {
    if buf.is_null() || cch == 0 {
        return 0;
    }
    let n = t.len().min(cch - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as u32
}

pub fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// The text of a control whose window text lives elsewhere (MLE, combo
/// box, spin button).
pub fn text_of(h: u32) -> Option<Vec<u8>> {
    crate::mle::text(h).or_else(|| crate::combo::text(h)).or_else(|| crate::spin::text(h))
}

/// Set such a control's text; false if `h` is not one.
pub fn set_text_of(h: u32, t: &[u8]) -> bool {
    if crate::mle::is_mle(h) {
        crate::mle::set_text(h, t);
    } else if crate::combo::is_combo(h) {
        crate::combo::set_text(h, t);
    } else if crate::spin::is_spin(h) {
        crate::spin::set_text(h, t);
    } else {
        return false;
    }
    true
}

/// Register the built-in classes (WC_FRAME etc., named "#n").
pub fn register_builtin() {
    let classes: [(u32, u32, u32); 14] = [
        (1, crate::frame::frame_entry as *const () as u32, 0x20),
        (3, crate::button::button_entry as *const () as u32, 0),
        (4, crate::menu::menu_entry as *const () as u32, 0),
        (5, crate::statics::static_entry as *const () as u32, 0),
        (6, crate::entry::entry_entry as *const () as u32, 0),
        (7, crate::listbox::listbox_entry as *const () as u32, 0x2000_0000),
        (8, crate::scrollbar::scrollbar_entry as *const () as u32, 0),
        (9, crate::frame::titlebar_entry as *const () as u32, 0),
        (10, crate::mle::mle_entry as *const () as u32, 0x2000_0000),
        (2, crate::combo::combo_entry as *const () as u32, 0x2000_0000),
        (32, crate::spin::spin_entry as *const () as u32, 0x2000_0000),
        (38, crate::slider::slider_entry as *const () as u32, 0),
        (39, crate::valueset::valueset_entry as *const () as u32, 0),
        (40, crate::notebook::notebook_entry as *const () as u32, 0x2000_0000),
    ];
    let s = st();
    for (n, proc_, style) in classes {
        s.classes.push(Class { name: alloc::format!("#{n}").into_bytes(), proc_, style, words: 0, pid: 0 });
    }
}
