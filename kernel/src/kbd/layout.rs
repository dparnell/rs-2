//! Keyboard layouts: national key assignments, AltGr and dead keys, and
//! the keyboard code page the characters are delivered in.
//!
//! The layouts are generated into `layouts.rs` by tools/scripts/gen_nls.py.
//! The layout is chosen by CONFIG.SYS `DEVINFO=KBD,xx,...` and by KEYB
//! (category 4 IOCtl, see api/ioctl.rs); the code page follows KbdSetCp /
//! DosSetCp and the primary CODEPAGE.

use super::layouts::{Layout, COMPOSE, LAYOUTS, SPACING};
use super::shift::*;
use crate::codepage::CodePage;
use crate::sync::SpinLock;

const DEAD: u32 = 0x8000_0000;

struct State {
    layout: usize,
    codepage: u16,
    /// Combining accent of a dead key waiting for the next key.
    pending: Option<u16>,
}

static STATE: SpinLock<State> = SpinLock::new(State { layout: 0, codepage: 850, pending: None });

fn find(id: &str) -> Option<usize> {
    LAYOUTS.iter().position(|l| l.id.eq_ignore_ascii_case(id))
}

/// Select a layout by its two-letter id (US, UK, GR, FR, ...).
pub fn set_layout(id: &str) -> bool {
    match find(id.trim()) {
        Some(i) => {
            let mut s = STATE.lock();
            s.layout = i;
            s.pending = None;
            true
        }
        None => false,
    }
}

pub fn layout_id() -> &'static str {
    LAYOUTS[STATE.lock().layout].id
}

/// The code page a layout was designed for.
pub fn layout_codepage(id: &str) -> Option<u16> {
    find(id).map(|i| LAYOUTS[i].codepage)
}

pub fn layouts() -> impl Iterator<Item = &'static Layout> {
    LAYOUTS.iter()
}

pub fn set_codepage(cp: u16) {
    if CodePage::from_number(cp).is_some() {
        STATE.lock().codepage = cp;
    }
}

pub fn codepage() -> u16 {
    STATE.lock().codepage
}

fn spacing(comb: u16) -> char {
    SPACING.iter().find(|e| e.0 == comb).and_then(|e| char::from_u32(e.1 as u32)).unwrap_or('?')
}

fn compose(comb: u16, base: char) -> Option<char> {
    let b = base as u32;
    if b > 0xffff {
        return None;
    }
    COMPOSE
        .binary_search_by(|e| (e.0, e.1).cmp(&(comb, b as u16)))
        .ok()
        .and_then(|i| char::from_u32(COMPOSE[i].2 as u32))
}

/// Result of a key on a layout key position.
pub enum Key {
    /// Not a layout key (Esc, Enter, function keys, ...).
    Other,
    /// A dead key was stored, or nothing is assigned: no character.
    Nothing,
    /// The characters to deliver (a failed dead key composition gives the
    /// accent and the character), as code page bytes (0 = no byte for
    /// the character in the keyboard code page).
    Chars(u8, Option<u8>),
}

/// Translate a key press (scan code set 1, no 0xE0 prefix) under the
/// current shift state.
pub fn key(code: u8, shift: u16) -> Key {
    let mut s = STATE.lock();
    let layout = &LAYOUTS[s.layout];
    let Some(&(_, normal, shifted, altgr, altgr_shift)) = layout.keys.iter().find(|k| k.0 == code) else {
        return Key::Other;
    };
    let shifted_on = shift & (LEFTSHIFT | RIGHTSHIFT) != 0;
    let ctrl = shift & CONTROL != 0;
    let alt = shift & ALT != 0;
    // AltGr: the right Alt key, or Ctrl+Alt, on layouts that have it.
    let altgr_on = layout.altgr && (shift & RIGHTALT != 0 || (ctrl && alt));
    let page = CodePage::from_number(s.codepage).unwrap_or(CodePage::Cp850);
    let enc = |ch: char| page.from_char(ch).unwrap_or(0);
    if altgr_on {
        let v = if shifted_on && altgr_shift != 0 { altgr_shift } else { altgr };
        if v == 0 {
            return Key::Chars(0, None);
        }
        return emit(&mut s, v, &enc);
    }
    if alt {
        // Alt+key: no character, the scan code identifies the key.
        s.pending = None;
        return Key::Chars(0, None);
    }
    let letter = char::from_u32(normal).is_some_and(|c| c.is_alphabetic());
    let mut use_shift = shifted_on;
    if shift & CAPSLOCK_ON != 0 && letter {
        use_shift = !use_shift;
    }
    let v = if use_shift { shifted } else { normal };
    if ctrl {
        s.pending = None;
        let c = char::from_u32(normal & !DEAD).unwrap_or('\0');
        let b = match c {
            'a'..='z' => c as u8 & 0x1f,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            _ => 0,
        };
        return Key::Chars(b, None);
    }
    if v == 0 {
        return Key::Nothing;
    }
    emit(&mut s, v, &enc)
}

fn emit(s: &mut State, v: u32, enc: &dyn Fn(char) -> u8) -> Key {
    if v & DEAD != 0 {
        let comb = (v & 0xffff) as u16;
        match s.pending.take() {
            // The same dead key twice types the accent.
            Some(p) if p == comb => return Key::Chars(enc(spacing(p)), None),
            Some(p) => {
                s.pending = Some(comb);
                return Key::Chars(enc(spacing(p)), None);
            }
            None => {
                s.pending = Some(comb);
                return Key::Nothing;
            }
        }
    }
    let ch = char::from_u32(v).unwrap_or('?');
    match s.pending.take() {
        None => Key::Chars(enc(ch), None),
        Some(p) => match compose(p, ch) {
            Some(c) if enc(c) != 0 => Key::Chars(enc(c), None),
            _ if ch == ' ' => Key::Chars(enc(spacing(p)), None),
            _ => Key::Chars(enc(spacing(p)), Some(enc(ch))),
        },
    }
}

/// The keypad decimal key's character with NumLock on.
pub fn keypad_decimal() -> u8 {
    LAYOUTS[STATE.lock().layout].decimal
}

/// Forget a pending dead key (focus or session change).
pub fn reset() {
    STATE.lock().pending = None;
}
