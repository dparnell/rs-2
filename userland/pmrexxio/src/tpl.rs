//! Menu, dialog and help table templates built in memory (rs2's LX
//! modules carry no resources): the I/O window's action bar, the search
//! dialog and the help table.

use alloc::vec::Vec;

use crate::api::*;
use crate::ids::*;

pub enum Menu {
    /// id, text, attributes (MIA_*)
    Item(u16, &'static [u8], u16),
    Sep,
    Sub(u16, &'static [u8], Vec<Menu>),
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// An RT_MENU layout: cb, version, codepage, offset of the items, count;
/// each item style, attribute, id, text and, for a submenu, its own menu.
pub fn menu(items: &[Menu]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [0, 0, 437, 10, items.len() as u16] {
        put16(&mut out, v);
    }
    for it in items {
        match it {
            Menu::Item(id, text, attr) => {
                for v in [MIS_TEXT, *attr, *id] {
                    put16(&mut out, v);
                }
                out.extend_from_slice(text);
                out.push(0);
            }
            Menu::Sep => {
                for v in [MIS_SEPARATOR, 0, 0xffff] {
                    put16(&mut out, v);
                }
            }
            Menu::Sub(id, text, sub) => {
                for v in [MIS_TEXT | MIS_SUBMENU, 0, *id] {
                    put16(&mut out, v);
                }
                out.extend_from_slice(text);
                out.push(0);
                out.extend_from_slice(&menu(sub));
            }
        }
    }
    let n = out.len() as u16;
    out[0..2].copy_from_slice(&n.to_le_bytes());
    out
}

const DIS: u16 = MIA_DISABLED as u16;

/// The action bar: the I/O window's own items and those a program using
/// the window (PMREXX) acts on, with IBM's ids.
pub fn action_bar() -> Vec<u8> {
    use Menu::*;
    menu(&[
        Sub(RXIOM_FILE, b"~File", alloc::vec![
            Item(RXIOA_SAVE, b"~Save\tCtrl+S", 0),
            Item(RXIOA_SAVEAS, b"Save ~as...", 0),
            Sep,
            Sub(RXIOA_PROPERTIES, b"~Properties", alloc::vec![
                Item(RXIOA_FONTS, b"~Font...", 0),
                Item(RXIOA_RESULTS, b"Append ~results", MIA_CHECKED as u16),
            ]),
            Sep,
            Item(RXIOA_EXIT, b"E~xit\tF3", 0),
        ]),
        Sub(RXIOM_EDIT, b"~Edit", alloc::vec![
            Item(RXIOA_CUT, b"Cu~t", 0),
            Item(RXIOA_COPY, b"~Copy\tCtrl+Insert", 0),
            Item(RXIOA_PASTE, b"~Paste\tShift+Insert", 0),
            Sep,
            Item(RXIOA_DELETE, b"~Delete\tDelete", 0),
            Sep,
            Item(RXIOA_ALL, b"Select ~all", 0),
            Item(RXIOA_DALL, b"Dese~lect all", 0),
            Sep,
            Item(RXIOA_SEARCH, b"~Find...", 0),
        ]),
        Sub(RXIOM_ACTIONS, b"~Trace", alloc::vec![
            Item(RXIOA_TRACE, b"~Interactive trace", 0),
            Sep,
            Item(RXIOA_TRCOFF, b"Trace ~off", DIS),
            Sep,
            Item(RXIOA_TRCSTEP, b"~Next clause", DIS),
            Item(RXIOA_TRCLAST, b"Re~peat last clause", DIS),
            Sep,
            Item(RXIOA_RESTART, b"~Restart", DIS),
            Item(RXIOA_HALT, b"~Halt procedure", DIS),
        ]),
        Sub(RXIOM_HELP, b"~Help", alloc::vec![
            Item(RXIOM_INDEXHELP, b"Help ~index", 0),
            Item(RXIOM_EXTHELP, b"~General help", 0),
            Item(RXIOM_HELPHELP, b"~Using help", 0),
            Item(RXIOM_KEYSHELP, b"~Keys help", 0),
            Sep,
            Item(RXIOA_ABOUT, b"~Product information", 0),
        ]),
    ])
}

pub struct Ctl {
    pub class: u16,
    pub text: &'static [u8],
    pub style: u32,
    pub pos: (i16, i16, i16, i16),
    pub id: u16,
}

/// A dialog template: a frame (FCF_TITLEBAR | FCF_SYSMENU, FS_DLGBORDER)
/// and its controls (WC_* class numbers, dialog units).
pub fn dialog(title: &[u8], id: u16, cx: i16, cy: i16, ctls: &[Ctl]) -> Vec<u8> {
    let n = ctls.len() + 1;
    let head = 14;
    let base = head + 30 * n;
    let mut items = Vec::new();
    let mut extra: Vec<u8> = Vec::new();
    let add = |items: &mut Vec<u8>, extra: &mut Vec<u8>, class: u16, children: u16, text: &[u8], style: u32, pos: (i16, i16, i16, i16), id: u16, ctl: &[u8]| {
        let toff = base + extra.len();
        extra.extend_from_slice(text);
        extra.push(0);
        let coff = if ctl.is_empty() {
            0xffff
        } else {
            while extra.len() % 4 != 0 {
                extra.push(0);
            }
            let c = base + extra.len();
            extra.extend_from_slice(ctl);
            c as u16
        };
        for v in [0, children, 0, class, text.len() as u16 + 1, toff as u16] {
            put16(items, v);
        }
        items.extend_from_slice(&style.to_le_bytes());
        for v in [pos.0 as u16, pos.1 as u16, pos.2 as u16, pos.3 as u16, id, 0xffff, coff] {
            put16(items, v);
        }
    };
    // WS_CLIPSIBLINGS | WS_SAVEBITS | FS_DLGBORDER
    let fcf = (FCF_TITLEBAR | FCF_SYSMENU).to_le_bytes();
    add(&mut items, &mut extra, 1, ctls.len() as u16, title, 0x1400_0080, (16, 28, cx, cy), id, &fcf);
    for c in ctls {
        add(&mut items, &mut extra, c.class, 0, c.text, c.style, c.pos, c.id, &[]);
    }
    let mut out = Vec::new();
    for v in [0, 0, 437, head as u16, 1, 0xffff, 0] {
        put16(&mut out, v);
    }
    out.extend_from_slice(&items);
    out.extend_from_slice(&extra);
    let len = out.len() as u16;
    out[0..2].copy_from_slice(&len.to_le_bytes());
    out
}

/// The Find dialog (IBM's is dialog 130 with these control ids).
pub fn search_dialog() -> Vec<u8> {
    const V: u32 = WS_VISIBLE;
    dialog(b"Find", PMREXX_SEARCH, 184, 85, &[
        Ctl { class: 5, text: b"Text to find:", style: V | WS_GROUP | SS_TEXT, pos: (5, 68, 150, 8), id: RX_SEARCH_TEXT },
        Ctl { class: 6, text: b"", style: V | WS_TABSTOP | WS_GROUP | ES_MARGIN | ES_AUTOSCROLL, pos: (7, 53, 170, 8), id: RX_SEARCH_FIELD },
        Ctl { class: 3, text: b"~Match case", style: V | WS_TABSTOP | BS_AUTOCHECKBOX, pos: (5, 33, 100, 10), id: RX_CASEBOX },
        Ctl { class: 3, text: b"~OK", style: V | WS_TABSTOP | WS_GROUP | BS_DEFAULT, pos: (5, 9, 40, 14), id: PB_OK },
        Ctl { class: 3, text: b"~Cancel", style: V | WS_TABSTOP, pos: (60, 9, 40, 14), id: PB_CANCEL },
        Ctl { class: 3, text: b"Help", style: V | WS_TABSTOP | BS_HELP | BS_NOPOINTERFOCUS, pos: (115, 9, 40, 14), id: PB_HELP },
    ])
}

/// The help table: the I/O window's subtable (menu items to panels),
/// the standard file and font dialogs' and the Find dialog's.
pub fn help_table() -> Vec<u8> {
    fn sub(pairs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        put16(&mut out, 2);
        for &(id, panel) in pairs {
            put16(&mut out, id);
            put16(&mut out, panel);
        }
        put16(&mut out, 0);
        out
    }
    let main = sub(&[
        (RXIOM_EDIT, 10), (RXIOA_CUT, 32), (RXIOA_COPY, 11), (RXIOA_PASTE, 12), (RXIOA_DELETE, 13),
        (RXIOA_ALL, 14), (RXIOA_DALL, 34), (RXIOA_TRACE, 17), (RXIOA_FONTS, 18), (RXIOA_PROPERTIES, 35),
        (RXIOA_RESULTS, 36), (RXIOA_SEARCH, 37), (RXIOM_ACTIONS, 24), (RXIOA_HALT, 25), (RXIOA_TRCSTEP, 26),
        (RXIOA_TRCLAST, 27), (RXIOA_TRCOFF, 28), (RXIOM_HELP, 45), (RXIOM_HELPHELP, 29), (RXIOM_EXTHELP, 1),
        (RXIOM_KEYSHELP, 31), (RXIOM_INDEXHELP, 30), (RXIOM_FILE, 2), (RXIOA_SAVE, 3), (RXIOA_SAVEAS, 4),
        (RXIOA_EXIT, 9), (RXIOA_RESTART, 16), (RXIOA_ABOUT, 42),
    ]);
    let search = sub(&[(RX_SEARCH_FIELD, 601), (RX_CASEBOX, 601), (PB_OK, 601), (PB_CANCEL, 601)]);
    let empty = sub(&[]);
    // Entries: window id, subtable, extended help panel (8 bytes each),
    // then the subtables; pointers are filled in once the table is in
    // its final place (see `HelpTable`).
    let entries: [(u16, &Vec<u8>, u16); 4] = [(RXHOST_WIN, &main, 1), (0x100, &empty, 600), (0x12c, &empty, 400), (PMREXX_SEARCH, &search, 601)];
    let mut out = Vec::new();
    let mut subs = Vec::new();
    let head = 8 * (entries.len() + 1);
    for (id, s, ext) in entries {
        let off = head + subs.len();
        put16(&mut out, id);
        out.extend_from_slice(&(off as u32).to_le_bytes());
        put16(&mut out, ext);
        subs.extend_from_slice(s);
    }
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&subs);
    out
}

/// Turn `help_table`'s subtable offsets into addresses (the table lives
/// as long as the window).
pub fn fix_help_table(t: &mut [u8]) {
    let base = t.as_ptr() as u32;
    let mut o = 0;
    while u16::from_le_bytes([t[o], t[o + 1]]) != 0 {
        let off = u32::from_le_bytes([t[o + 2], t[o + 3], t[o + 4], t[o + 5]]);
        t[o + 2..o + 6].copy_from_slice(&(base + off).to_le_bytes());
        o += 8;
    }
}
