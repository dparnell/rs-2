//! Menu and dialog templates made in memory (the RT_MENU and RT_DIALOG
//! layouts the resource compiler writes), and running dialogs.

use alloc::vec::Vec;

use crate::consts::*;
use crate::sys::*;

/// One item of a menu built in memory.
pub enum Menu<'a> {
    Item(u16, &'a [u8]),
    /// An item that starts checked.
    Check(u16, &'a [u8], bool),
    /// An item that starts disabled.
    Disabled(u16, &'a [u8]),
    /// A system command (MIS_SYSCOMMAND, e.g. SC_CLOSE).
    Sys(u16, &'a [u8]),
    /// A help item (MIS_HELP: sends WM_HELP).
    Help(u16, &'a [u8]),
    Sep,
    Sub(u16, &'a [u8], Vec<Menu<'a>>),
}

/// A menu template: cb, version, codepage, reserved, count; each item
/// style, attribute, id, text and, for a submenu, its own template.
pub fn menu_template(items: &[Menu]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&850u16.to_le_bytes());
    out.extend_from_slice(&10u16.to_le_bytes());
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    for it in items {
        let (style, attr, id, text, sub): (u16, u16, u16, &[u8], Option<&Vec<Menu>>) = match it {
            Menu::Item(id, t) => (MIS_TEXT, 0, *id, t, None),
            Menu::Check(id, t, on) => (MIS_TEXT, if *on { MIA_CHECKED } else { 0 }, *id, t, None),
            Menu::Disabled(id, t) => (MIS_TEXT, MIA_DISABLED, *id, t, None),
            Menu::Sys(id, t) => (MIS_TEXT | MIS_SYSCOMMAND, 0, *id, t, None),
            Menu::Help(id, t) => (MIS_TEXT | MIS_HELP, 0, *id, t, None),
            Menu::Sep => (MIS_SEPARATOR, 0, 0xffff, b"", None),
            Menu::Sub(id, t, s) => (MIS_TEXT | MIS_SUBMENU, 0, *id, t, Some(s)),
        };
        out.extend_from_slice(&style.to_le_bytes());
        out.extend_from_slice(&attr.to_le_bytes());
        out.extend_from_slice(&id.to_le_bytes());
        if style & MIS_TEXT != 0 {
            out.extend_from_slice(text);
            out.push(0);
        }
        if let Some(s) = sub {
            out.extend_from_slice(&menu_template(s));
        }
    }
    let n = out.len() as u16;
    out[0..2].copy_from_slice(&n.to_le_bytes());
    out
}

/// A pop-up menu (owned by `owner`, not shown) made from items.
pub fn popup_menu(owner: u32, items: &[Menu]) -> u32 {
    let tpl = menu_template(items);
    let _ = owner;
    unsafe { WinCreateMenu(HWND_DESKTOP, tpl.as_ptr()) }
}

/// One control of a dialog template.
pub struct Item {
    pub class: u16,
    /// A class by name (overrides `class`).
    pub class_name: Option<Vec<u8>>,
    pub text: Vec<u8>,
    pub style: u32,
    pub x: i16,
    pub y: i16,
    pub cx: i16,
    pub cy: i16,
    pub id: u16,
    pub ctl: Vec<u8>,
    /// Presentation parameters (a PRESPARAMS block).
    pub pres: Vec<u8>,
}

pub fn item(class: u16, text: &[u8], style: u32, (x, y, cx, cy): (i16, i16, i16, i16), id: u16) -> Item {
    Item { class, class_name: None, text: text.to_vec(), style, x, y, cx, cy, id, ctl: Vec::new(), pres: Vec::new() }
}

impl Item {
    /// Control data (CTLDATA) for the control.
    pub fn ctl(mut self, c: &[u8]) -> Item {
        self.ctl = c.to_vec();
        self
    }
    /// A font for the control (`"8.Helv"`).
    pub fn font(mut self, f: &[u8]) -> Item {
        self.pres = crate::font_pres(f);
        self
    }
    /// Give the control a group start (WS_GROUP).
    pub fn group(mut self) -> Item {
        self.style |= WS_GROUP;
        self
    }
}

/// A push button (WS_TABSTOP; `extra` adds BS_DEFAULT, BS_HELP...).
pub fn button(text: &[u8], (x, y, cx): (i16, i16, i16), id: u32, extra: u32) -> Item {
    item(WC_BUTTON, text, WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON | extra, (x, y, cx, 14), id as u16)
}

/// A Help button (BS_HELP | BS_NOPOINTERFOCUS).
pub fn help_button((x, y, cx): (i16, i16, i16), id: u32) -> Item {
    item(WC_BUTTON, b"Help", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON | BS_HELP | BS_NOPOINTERFOCUS, (x, y, cx, 14), id as u16)
}

/// A check box.
pub fn check_box(text: &[u8], r: (i16, i16, i16, i16), id: u32) -> Item {
    item(WC_BUTTON, text, WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX, r, id as u16)
}

/// A radio button (the first of a group should get `.group()`).
pub fn radio(text: &[u8], r: (i16, i16, i16, i16), id: u32) -> Item {
    item(WC_BUTTON, text, WS_VISIBLE | BS_AUTORADIOBUTTON, r, id as u16)
}

/// Static text.
pub fn label(text: &[u8], r: (i16, i16, i16, i16), extra: u32) -> Item {
    item(WC_STATIC, text, WS_VISIBLE | SS_TEXT | extra, r, 0xffff)
}

/// Static text with an id (to be changed later).
pub fn text_id(text: &[u8], r: (i16, i16, i16, i16), id: u32, extra: u32) -> Item {
    item(WC_STATIC, text, WS_VISIBLE | SS_TEXT | extra, r, id as u16)
}

/// A group box.
pub fn group_box(text: &[u8], r: (i16, i16, i16, i16)) -> Item {
    item(WC_STATIC, text, WS_VISIBLE | SS_GROUPBOX, r, 0xffff)
}

/// An entry field.
pub fn entry(r: (i16, i16, i16, i16), id: u32, extra: u32) -> Item {
    item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | ES_MARGIN | ES_AUTOSCROLL | extra, r, id as u16)
}

/// A list box.
pub fn list_box(r: (i16, i16, i16, i16), id: u32, extra: u32) -> Item {
    item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | extra, r, id as u16)
}

/// A drop-down list (combination box).
pub fn drop_list(r: (i16, i16, i16, i16), id: u32) -> Item {
    item(WC_COMBOBOX, b"", WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST, r, id as u16)
}

/// A numeric spin button.
pub fn spin(r: (i16, i16, i16, i16), id: u32) -> Item {
    item(WC_SPINBUTTON, b"", WS_VISIBLE | WS_TABSTOP | SPBS_NUMERICONLY | SPBS_MASTER | SPBS_JUSTRIGHT, r, id as u16)
}

/// The system icon of a message (SPTR_ICON*).
pub fn sys_icon(sptr: i32, (x, y): (i16, i16)) -> Item {
    // SS_SYSICON's text is "#n", the icon's id.
    let t = alloc::format!("#{sptr}").into_bytes();
    item(WC_STATIC, &t, WS_VISIBLE | SS_SYSICON, (x, y, 21, 21), 0xffff)
}

/// A window of a class registered by the program.
pub fn custom(class: &[u8], r: (i16, i16, i16, i16), id: u32, style: u32) -> Item {
    let mut it = item(0, b"", WS_VISIBLE | style, r, id as u16);
    it.class_name = Some(class.to_vec());
    it
}

/// A dialog frame with a title bar and system menu; `id` is the dialog's
/// id (its help subtable).
pub fn dialog_frame(title: &[u8], id: u16, cx: i16, cy: i16) -> Item {
    // WS_CLIPSIBLINGS | WS_SAVEBITS | FS_DLGBORDER | FS_NOBYTEALIGN.
    let mut f = item(WC_FRAME, title, 0x1400_0000 | FS_DLGBORDER | FS_NOBYTEALIGN, (20, 20, cx, cy), id);
    f.ctl = (FCF_TITLEBAR | FCF_SYSMENU).to_le_bytes().to_vec();
    f
}

/// Build a template: header (cbTemplate, type, codepage, offadlgti,
/// fsTemplateStatus, iItemFocus, coffPresParams), 30-byte items, then
/// the strings, control data and presentation parameters.
pub fn template(frame: Item, children: Vec<Item>) -> Vec<u8> {
    let items: Vec<Item> = core::iter::once(frame).chain(children).collect();
    let n = items.len();
    let head = 14;
    let mut out = alloc::vec![0u8; head + 30 * n];
    let mut extra: Vec<u8> = Vec::new();
    let base = head + 30 * n;
    let put16 = |o: &mut Vec<u8>, at: usize, v: u16| o[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put16(&mut out, 2, 0);
    put16(&mut out, 4, 850);
    put16(&mut out, 6, head as u16);
    put16(&mut out, 8, 1);
    put16(&mut out, 10, 0xffff);
    put16(&mut out, 12, 0);
    for (i, it) in items.iter().enumerate() {
        let at = head + 30 * i;
        let children = if i == 0 { (n - 1) as u16 } else { 0 };
        let toff = base + extra.len();
        extra.extend_from_slice(&it.text);
        extra.push(0);
        // cchClassName: 0 for a predefined class (offClassName is then
        // its WC_ number).
        let (cch, class) = match &it.class_name {
            Some(name) => {
                let off = base + extra.len();
                extra.extend_from_slice(name);
                extra.push(0);
                (name.len() as u16, off as u16)
            }
            None => (0, it.class),
        };
        let coff = if it.ctl.is_empty() {
            0xffff
        } else {
            while (base + extra.len()) % 4 != 0 {
                extra.push(0);
            }
            let c = base + extra.len();
            extra.extend_from_slice(&it.ctl);
            c as u16
        };
        let poff = if it.pres.is_empty() {
            0xffff
        } else {
            while (base + extra.len()) % 4 != 0 {
                extra.push(0);
            }
            let c = base + extra.len();
            extra.extend_from_slice(&it.pres);
            c as u16
        };
        put16(&mut out, at, 0);
        put16(&mut out, at + 2, children);
        put16(&mut out, at + 4, cch);
        put16(&mut out, at + 6, class);
        put16(&mut out, at + 8, it.text.len() as u16 + 1);
        put16(&mut out, at + 10, toff as u16);
        out[at + 12..at + 16].copy_from_slice(&it.style.to_le_bytes());
        put16(&mut out, at + 16, it.x as u16);
        put16(&mut out, at + 18, it.y as u16);
        put16(&mut out, at + 20, it.cx as u16);
        put16(&mut out, at + 22, it.cy as u16);
        put16(&mut out, at + 24, it.id);
        put16(&mut out, at + 26, poff as u16);
        put16(&mut out, at + 28, coff as u16);
    }
    out.extend_from_slice(&extra);
    let len = out.len() as u16;
    put16(&mut out, 0, len);
    out
}

/// Run a modal dialog made from a template; returns its result.
pub fn run_dialog(owner: u32, proc_: WndProc, tpl: &[u8], param: u32) -> u32 {
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, proc_ as *const () as u32, tpl.as_ptr(), param) };
    if dlg == 0 {
        return DID_CANCEL;
    }
    if owner != 0 {
        crate::place_over(dlg, owner);
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    r
}

/// Make a modeless dialog (shown); returns its window.
pub fn modeless_dialog(owner: u32, proc_: WndProc, tpl: &[u8], param: u32) -> u32 {
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, proc_ as *const () as u32, tpl.as_ptr(), param) };
    if dlg != 0 {
        if owner != 0 {
            crate::place_over(dlg, owner);
        }
        unsafe { WinShowWindow(dlg, 1) };
    }
    dlg
}

extern "C" fn about_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinDefDlgProc(h, m, a, b) }
}

/// The Product Information dialog: the program's icon, its name and
/// lines of text, OK and Help; `id` is the dialog's id (help subtable).
pub fn product_info(owner: u32, id: u16, title: &[u8], name: &[u8], lines: &[&[u8]]) {
    let cy = 70 + 10 * lines.len() as i16;
    let mut v = Vec::new();
    v.push(sys_icon(SPTR_APPICON, (10, cy - 34)));
    v.push(label(name, (40, cy - 30, 180, 10), DT_LEFT));
    for (i, l) in lines.iter().enumerate() {
        v.push(label(l, (40, cy - 44 - 10 * i as i16, 180, 9), DT_LEFT));
    }
    v.push(button(b"OK", (60, 6, 46), DID_OK, BS_DEFAULT));
    v.push(help_button((114, 6, 46), 0x3f5));
    let tpl = template(dialog_frame(title, id, 230, cy), v);
    run_dialog(owner, about_proc, &tpl, 0);
}
