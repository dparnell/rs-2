//! The Icon Editor's dialogs (OS/2's ids and controls): New Figure, the
//! standard file dialogs with Save Options (layout and forms), Edit
//! color, Device List, Add Device, Customize Device and Edit Predefined
//! Device Forms; palette files; the clipboard.

use alloc::vec::Vec;

use crate::devices::{self, Device};
use crate::edit::{self, Area};
use crate::format::{self, Format, Kind, INVERSE, SCREEN};
use crate::*;

/// The Help push button of every dialog.
pub const ID_HELP: u16 = 0x3f5;

fn help_button_at(x: i16, y: i16, w: i16) -> pmkit::Item {
    help_button((x, y, w), ID_HELP as u32)
}

fn num_entry(r: (i16, i16, i16, i16), id: u32) -> pmkit::Item {
    entry(r, id, 0)
}

fn set_num(h: u32, id: u32, v: i64) {
    set_item_text(h, id, alloc::format!("{v}").as_bytes());
}

fn get_num(h: u32, id: u32) -> Option<i64> {
    parse_num(&item_text(h, id))
}

/// WM_HELP in a dialog: the dialog's own help panel.
fn dialog_help(panel: u16) {
    if let Some(h) = app().help.as_deref() {
        h.show(panel);
    } else {
        help_request(HM_DISPLAY_HELP, panel as u32, HM_RESOURCEID);
    }
}

// ---- New Figure ----

static mut NEW_KIND: Kind = Kind::Icon;
static mut NEW_SIZE: (u32, u32) = (64, 64);

extern "C" fn new_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            let k = unsafe { NEW_KIND };
            set_check(h, match k { Kind::Icon => 103, Kind::Pointer => 104, Kind::Bitmap => 105 }, true);
            let (w, hh) = unsafe { NEW_SIZE };
            set_num(h, 106, w as i64);
            set_num(h, 107, hh as i64);
            enable(h, 106, k == Kind::Bitmap);
            enable(h, 107, k == Kind::Bitmap);
            unsafe { WinShowWindow(item(h, 108), 0) };
            0
        }
        WM_CONTROL if matches!(lo(mp1), 103..=105) => {
            let bm = checked(h, 105);
            enable(h, 106, bm);
            enable(h, 107, bm);
            0
        }
        WM_COMMAND if lo(mp1) == DID_OK => {
            let kind = if checked(h, 104) { Kind::Pointer } else if checked(h, 105) { Kind::Bitmap } else { Kind::Icon };
            if kind == Kind::Bitmap {
                let (w, hh) = (get_num(h, 106).unwrap_or(0), get_num(h, 107).unwrap_or(0));
                if !(1..=format::MAX_SIDE as i64).contains(&w) || !(1..=format::MAX_SIDE as i64).contains(&hh) {
                    unsafe { WinShowWindow(item(h, 108), 1) };
                    unsafe { WinAlarm(HWND_DESKTOP, 1) };
                    return 0;
                }
                unsafe { NEW_SIZE = (w as u32, hh as u32) };
            }
            unsafe { NEW_KIND = kind };
            unsafe { WinDismissDlg(h, DID_OK) };
            0
        }
        WM_HELP => {
            dialog_help(20058);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

pub fn new_dialog() {
    let a = app();
    unsafe { NEW_KIND = a.fig.kind };
    let tpl = template(
        dialog_frame(b"New Figure", DLG_NEW, 162, 84),
        alloc::vec![
            label(b"Specify the figure type. Bitmaps need a size.", (6, 54, 150, 24), DT_LEFT | DT_WORDBREAK),
            text_id(b"The bitmap size is beyond the limit.", (6, 54, 150, 24), 108, DT_LEFT | DT_WORDBREAK),
            radio(b"~Icon", (7, 40, 44, 10), 103).group(),
            radio(b"~Pointer", (52, 40, 50, 10), 104),
            radio(b"~Bitmap", (104, 40, 52, 10), 105),
            text_id(b"Bitmap size:", (6, 22, 66, 12), 110, DT_RIGHT | DT_VCENTER),
            num_entry((78, 24, 24, 8), 106).group(),
            label(b"x", (104, 22, 12, 12), DT_CENTER | DT_VCENTER),
            num_entry((120, 24, 24, 8), 107),
            button(b"OK", (6, 4, 44), DID_OK, BS_DEFAULT).group(),
            button(b"Cancel", (56, 4, 44), DID_CANCEL, 0),
            help_button_at(106, 4, 44),
        ],
    );
    if run_dialog(a.frame, new_proc, &tpl, 0) != DID_OK || !query_save() {
        return;
    }
    new_figure(unsafe { NEW_KIND }, unsafe { NEW_SIZE });
    after_change();
}

// ---- Files ----

const FILEDLG_SIZE: usize = 328;
const FDS_CENTER: u32 = 0x0001;
const FDS_HELPBUTTON: u32 = 0x0008;
const FDS_OPEN_DIALOG: u32 = 0x0100;
const FDS_SAVEAS_DIALOG: u32 = 0x0200;
const FDS_ENABLEFILELB: u32 = 0x0800;

extern "C" fn open_help_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m == WM_HELP {
        dialog_help(20008);
        return 0;
    }
    unsafe { WinDefFileDlgProc(h, m, mp1, mp2) }
}

extern "C" fn save_help_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m == WM_HELP {
        dialog_help(20011);
        return 0;
    }
    unsafe { WinDefFileDlgProc(h, m, mp1, mp2) }
}

/// The standard file dialog: the file chosen.
pub fn file_dialog(save: bool, title: &[u8], initial: &[u8]) -> Option<Vec<u8>> {
    let a = app();
    let mut fd = alloc::vec![0u8; FILEDLG_SIZE];
    wr32(&mut fd, 0, FILEDLG_SIZE as u32);
    let fl = FDS_CENTER | FDS_HELPBUTTON | if save { FDS_SAVEAS_DIALOG | FDS_ENABLEFILELB } else { FDS_OPEN_DIALOG };
    wr32(&mut fd, 4, fl);
    let t = z(title);
    wr32(&mut fd, 20, t.as_ptr() as u32);
    let proc_ = if save { save_help_proc as *const () as u32 } else { open_help_proc as *const () as u32 };
    wr32(&mut fd, 28, proc_);
    let n = initial.len().min(259);
    fd[52..52 + n].copy_from_slice(&initial[..n]);
    fd[326..328].copy_from_slice(&(-1i16).to_le_bytes());
    let ok = unsafe { WinFileDlg(HWND_DESKTOP, a.frame, fd.as_mut_ptr()) } != 0;
    if !ok || rd32(&fd, 12) != DID_OK {
        return None;
    }
    let name = until_nul(&fd[52..312]).to_vec();
    (!name.is_empty()).then_some(name)
}

fn kind_ext(k: Kind) -> &'static [u8] {
    match k {
        Kind::Icon => b"ICO",
        Kind::Pointer => b"PTR",
        Kind::Bitmap => b"BMP",
    }
}

/// File > Open.
pub fn open() {
    let a = app();
    if !query_save() {
        return;
    }
    let pattern = [&b"*."[..], kind_ext(a.fig.kind)].concat();
    if let Some(p) = file_dialog(false, b"Open file", &pattern) {
        open_file(&p, false);
        after_change();
    }
}

static mut SAVE_FORMAT: Format = Format::V20;
static mut SAVE_FORMS: Vec<usize> = Vec::new();

extern "C" fn saveopts_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            // The layout the file was read in is marked <...>.
            let read = if a.path.is_some() { Some(a.fig.format) } else { None };
            for (id, f, t) in [(601u32, Format::V11, &b"1.1"[..]), (602, Format::V12, b"1.2"), (603, Format::V20, b"2.0"), (604, Format::V20Packed, b"packed 2.0")] {
                let mut s = Vec::new();
                if Some(f) == read {
                    s.push(b'<');
                }
                s.push(b'~');
                s.extend_from_slice(t);
                if Some(f) == read {
                    s.push(b'>');
                }
                set_item_text(h, id, &s);
                set_check(h, id, f == unsafe { SAVE_FORMAT });
            }
            for (i, f) in a.fig.forms.iter().enumerate() {
                let mut t = format::describe(f);
                t.extend_from_slice(b"  ");
                t.extend_from_slice(&form_device_name(i));
                lb_add(h, 606, &t);
                dlg_msg(h, 606, LM_SELECTITEM, i as u32, 1);
            }
            unsafe { WinSetFocus(HWND_DESKTOP, item(h, DID_OK)) };
            1
        }
        WM_CONTROL if lo(mp1) == 606 && hi(mp1) == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND if lo(mp1) == DID_OK => {
            let fmt = if checked(h, 601) { Format::V11 } else if checked(h, 602) { Format::V12 } else if checked(h, 604) { Format::V20Packed } else { Format::V20 };
            let mut forms = Vec::new();
            let mut i = LIT_FIRST;
            loop {
                let r = dlg_msg(h, 606, LM_QUERYSELECTION, i, 0) as i16;
                if r < 0 || forms.len() > a.fig.forms.len() {
                    break;
                }
                forms.push(r as usize);
                i = r as u32;
            }
            if forms.is_empty() {
                message(h, b"You must choose at least one form to save or cancel the save operation.", TITLE, MB_OK | MB_WARNING);
                return 0;
            }
            unsafe {
                SAVE_FORMAT = fmt;
                SAVE_FORMS = forms;
                WinDismissDlg(h, DID_OK);
            }
            0
        }
        WM_HELP => {
            dialog_help(20122);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Save Options: the layout and the forms to write; false on Cancel.
fn save_options_dialog() -> bool {
    let a = app();
    unsafe { SAVE_FORMAT = a.fig.format };
    let tpl = template(
        dialog_frame(b"Save Options", DLG_SAVEOPTS, 248, 124),
        alloc::vec![
            group_box(b"Choose format - <>s mark the format read", (6, 94, 237, 26)),
            radio(b"~1.1", (14, 100, 50, 10), 601).group(),
            radio(b"1.~2", (66, 100, 46, 10), 602),
            radio(b"2.~0", (114, 100, 50, 10), 603),
            radio(b"~packed 2.0", (166, 100, 72, 10), 604),
            group_box(b"Select forms to save", (6, 26, 237, 66)),
            list_box((15, 32, 219, 48), 606, LS_MULTIPLESEL).group(),
            button(b"~Save", (6, 6, 44), DID_OK, BS_DEFAULT).group(),
            button(b"Cancel", (56, 6, 44), DID_CANCEL, 0),
            help_button_at(199, 6, 44),
        ],
    );
    run_dialog(a.frame, saveopts_proc, &tpl, 0) == DID_OK
}

/// File > Save (as): true when saved.
pub fn save(ask_name: bool) -> bool {
    let a = app();
    let ext = kind_ext(a.fig.kind);
    let path = match (&a.path, ask_name) {
        (Some(p), false) => p.clone(),
        _ => {
            let initial = a.path.clone().unwrap_or_else(|| [&b"*."[..], ext].concat());
            let Some(p) = file_dialog(true, b"Save file", &initial) else { return false };
            // A name without an extension gets the figure's.
            let p = if file_name(&p).contains(&b'.') { p } else { [&p[..], b".", ext].concat() };
            if extension(&p) != ext && a.opts.nowarn == 0 {
                let t = [&b"The extension "[..], &extension(&p), b" is not the usual one for this kind of figure. Save anyway?"].concat();
                if message(a.frame, &t, TITLE, MB_YESNO | MB_WARNING) != MBID_YES {
                    return false;
                }
            }
            if a.opts.safe != 0 && read_file(&p).is_some() && Some(&p) != a.path.as_ref() {
                let t = [&b"Replace existing "[..], &p, b"?"].concat();
                if message(a.frame, &t, TITLE, MB_YESNO | MB_ICONQUESTION) != MBID_YES {
                    return false;
                }
            }
            if !save_options_dialog() {
                return false;
            }
            return save_file(&p, &unsafe { (*core::ptr::addr_of!(SAVE_FORMS)).clone() }, unsafe { SAVE_FORMAT });
        }
    };
    let all: Vec<usize> = (0..a.fig.forms.len()).collect();
    let fmt = if a.fig.format == Format::V11 && a.fig.forms.len() > 1 { Format::V12 } else { a.fig.format };
    save_file(&path, &all, fmt)
}

// ---- Palette files ----

const PAL_MAGIC: &[u8; 8] = b"RS2PAL\x1a\0";

/// Palette > Open: a palette file (rs2's: magic, count, 0xRRGGBB
/// entries) for the current form's colour count.
pub fn palette_open() {
    let a = app();
    let Some(p) = file_dialog(false, b"Load a palette from disk", b"*.PAL") else { return };
    let Some(d) = read_file(&p) else {
        message(a.frame, &[&b"Cannot open file "[..], &p, b"."].concat(), TITLE, MB_OK | MB_ERROR);
        return;
    };
    let n = rd32(&d, 8) as usize;
    if d.len() < 12 + n * 4 || &d[0..8] != PAL_MAGIC || n == 0 || n > 256 {
        message(a.frame, &[&b"The file "[..], &p, b" is not a palette file."].concat(), TITLE, MB_OK | MB_ERROR);
        return;
    }
    let pal: Vec<u32> = (0..n).map(|i| rd32(&d, 12 + i * 4) & 0xffffff).collect();
    let f = form();
    if f.bpp > 8 {
        a.draw_pal = pal.into_iter().take(16).collect();
    } else {
        checkpoint();
        for (i, c) in pal.into_iter().enumerate().take(form().palette.len()) {
            edit::set_palette(form(), i, c);
        }
    }
    a.pal_path = Some(p);
    a.pal_changed = false;
    after_change();
}

pub fn palette_save(ask: bool) {
    let a = app();
    let path = match (&a.pal_path, ask) {
        (Some(p), false) => p.clone(),
        _ => match file_dialog(true, b"Save current palette to disk", b"*.PAL") {
            Some(p) => p,
            None => return,
        },
    };
    let pal = palette();
    let mut d = PAL_MAGIC.to_vec();
    d.extend_from_slice(&(pal.len() as u32).to_le_bytes());
    for c in pal {
        d.extend_from_slice(&c.to_le_bytes());
    }
    match write_file(&path, &d) {
        Ok(()) => {
            a.pal_path = Some(path);
            a.pal_changed = false;
        }
        Err(rc) => {
            message(a.frame, alloc::format!("Error creating the palette file (error {rc}).").as_bytes(), TITLE, MB_OK | MB_ERROR);
        }
    }
}

// ---- Edit color ----

static mut EDIT_RGB: u32 = 0;
static mut EDIT_HSV: bool = false;
static mut SAMPLE: u32 = 0;

extern "C" fn sample_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m == WM_PAINT {
        let hps = unsafe { WinBeginPaint(h, 0, core::ptr::null_mut()) };
        gpi::rgb_mode(hps);
        let r = window_rect(h);
        gpi::fill(hps, r, unsafe { EDIT_RGB } as i32);
        gpi::frame(hps, r, 0);
        unsafe { WinEndPaint(hps) };
        return 0;
    }
    unsafe { WinDefWindowProc(h, m, mp1, mp2) }
}

fn rgb_to_hsv(c: u32) -> (i64, i64, i64) {
    let (r, g, b) = ((c >> 16 & 255) as i64, (c >> 8 & 255) as i64, (c & 255) as i64);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max * 100 / 255;
    let s = if max == 0 { 0 } else { d * 100 / max };
    let h = if d == 0 {
        0
    } else if max == r {
        (60 * (g - b) / d + 360) % 360
    } else if max == g {
        60 * (b - r) / d + 120
    } else {
        60 * (r - g) / d + 240
    };
    (h, s, v)
}

fn hsv_to_rgb(h: i64, s: i64, v: i64) -> u32 {
    let v255 = v * 255 / 100;
    if s == 0 {
        let c = v255 as u32;
        return c << 16 | c << 8 | c;
    }
    let h = h.rem_euclid(360);
    let sector = h / 60;
    let f = h % 60;
    let p = v255 * (100 - s) / 100;
    let q = v255 * (100 - s * f / 60) / 100;
    let t = v255 * (100 - s * (60 - f) / 60) / 100;
    let (r, g, b) = match sector {
        0 => (v255, t, p),
        1 => (q, v255, p),
        2 => (p, v255, t),
        3 => (p, q, v255),
        4 => (t, p, v255),
        _ => (v255, p, q),
    };
    (r.clamp(0, 255) as u32) << 16 | (g.clamp(0, 255) as u32) << 8 | b.clamp(0, 255) as u32
}

/// The three values shown (RGB or HSV) and their ranges.
fn color_values() -> ([i64; 3], [i64; 3]) {
    let c = unsafe { EDIT_RGB };
    if unsafe { EDIT_HSV } {
        let (h, s, v) = rgb_to_hsv(c);
        ([h, s, v], [359, 100, 100])
    } else {
        ([(c >> 16 & 255) as i64, (c >> 8 & 255) as i64, (c & 255) as i64], [255, 255, 255])
    }
}

fn show_color(h: u32, skip_entry: Option<u32>) {
    let (v, max) = color_values();
    let hsv = unsafe { EDIT_HSV };
    for i in 0..3u32 {
        if skip_entry != Some(403 + i) {
            set_num(h, 403 + i, v[i as usize]);
        }
        dlg_msg(h, 406 + i, SBM_SETSCROLLBAR, v[i as usize] as u32, mp(0, max[i as usize] as u32));
        set_item_text(h, 409 + i, if hsv { [&b"H"[..], b"S", b"V"][i as usize] } else { [&b"R"[..], b"G", b"B"][i as usize] });
    }
    unsafe { WinInvalidateRect(item(h, 416), core::ptr::null(), 0) };
}

fn set_from_values(v: [i64; 3]) {
    let c = if unsafe { EDIT_HSV } {
        hsv_to_rgb(v[0], v[1], v[2])
    } else {
        (v[0].clamp(0, 255) as u32) << 16 | (v[1].clamp(0, 255) as u32) << 8 | v[2].clamp(0, 255) as u32
    };
    unsafe { EDIT_RGB = c };
}

extern "C" fn color_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            set_check(h, 412, true);
            for i in 0..3 {
                dlg_msg(h, 403 + i, EM_SETTEXTLIMIT, 3, 0);
            }
            show_color(h, None);
            0
        }
        WM_HSCROLL => {
            let id = lo(mp1);
            if !(406..=408).contains(&id) {
                return 0;
            }
            let (mut v, max) = color_values();
            let k = (id - 406) as usize;
            let cmd = hi(mp2);
            v[k] = match cmd {
                SB_LINELEFT => v[k] - 1,
                SB_LINERIGHT => v[k] + 1,
                SB_PAGELEFT => v[k] - 16,
                SB_PAGERIGHT => v[k] + 16,
                SB_SLIDERTRACK | SB_SLIDERPOSITION => lo(mp2) as i16 as i64,
                _ => return 0,
            }
            .clamp(0, max[k]);
            set_from_values(v);
            show_color(h, None);
            0
        }
        WM_CONTROL => {
            let id = lo(mp1);
            if (403..=405).contains(&id) && hi(mp1) == EN_CHANGE {
                let (mut v, max) = color_values();
                if let Some(n) = get_num(h, id) {
                    v[(id - 403) as usize] = n.clamp(0, max[(id - 403) as usize]);
                    set_from_values(v);
                    show_color(h, Some(id));
                }
            } else if id == 412 || id == 413 {
                unsafe { EDIT_HSV = checked(h, 413) };
                show_color(h, None);
            }
            0
        }
        WM_HELP => {
            dialog_help(20084);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Palette > Edit color (or a double click on a colour): entry `i`.
pub fn edit_color(i: usize) {
    let a = app();
    let Some(&old) = palette().get(i) else { return };
    unsafe {
        EDIT_RGB = old;
        EDIT_HSV = false;
        if SAMPLE == 0 {
            WinRegisterClass(a.hab, b"IEColorSample\0".as_ptr(), sample_proc, 0, 0);
            SAMPLE = 1;
        }
    }
    let sb = |r: (i16, i16, i16, i16), id: u32| pmkit::tpl::item(WC_SCROLLBAR, b"", WS_VISIBLE | WS_TABSTOP | SBS_HORZ, r, id as u16);
    let tpl = template(
        dialog_frame(b"Edit color", DLG_COLOR, 198, 132),
        alloc::vec![
            text_id(b"R", (6, 110, 16, 12), 409, DT_CENTER | DT_VCENTER),
            sb((24, 111, 130, 10), 406),
            num_entry((164, 112, 22, 8), 403),
            text_id(b"G", (6, 92, 16, 12), 410, DT_CENTER | DT_VCENTER),
            sb((24, 93, 130, 10), 407),
            num_entry((164, 94, 22, 8), 404),
            text_id(b"B", (6, 74, 16, 12), 411, DT_CENTER | DT_VCENTER),
            sb((24, 75, 130, 10), 408),
            num_entry((164, 76, 22, 8), 405),
            custom(b"IEColorSample", (30, 24, 70, 44), 416, 0),
            radio(b"~RGB", (126, 58, 40, 10), 412).group(),
            radio(b"~HSV", (126, 48, 40, 10), 413),
            check_box(b"~Important", (126, 34, 66, 10), 415).group(),
            check_box(b"~Dynamic", (126, 22, 66, 10), 414),
            button(b"~OK", (10, 6, 44), DID_OK, BS_DEFAULT).group(),
            button(b"Cancel", (62, 6, 44), DID_CANCEL, 0),
            help_button_at(114, 6, 44),
        ],
    );
    if run_dialog(a.frame, color_proc, &tpl, 0) != DID_OK {
        return;
    }
    let new = unsafe { EDIT_RGB };
    if new == old {
        return;
    }
    if form().bpp > 8 {
        a.draw_pal[i] = new;
    } else {
        checkpoint();
        edit::set_palette(form(), i, new);
        // At least one entry must stay black: screen pels are written with
        // the black entry.
        if a.fig.kind != Kind::Bitmap && !form().palette.contains(&0) && a.opts.nowarn == 0 {
            message(a.frame, b"At least one palette color must be black for the screen and inverse areas to show properly. The changed color is kept; make another entry black before saving.", TITLE, MB_OK | MB_WARNING);
        }
    }
    a.pal_changed = true;
    after_change();
}

// ---- Device List ----

fn fill_device_list(h: u32) {
    let a = app();
    dlg_msg(h, 303, LM_DELETEALL, 0, 0);
    for (i, f) in a.fig.forms.iter().enumerate() {
        let mut t = format::describe(f);
        t.extend_from_slice(b"  ");
        t.extend_from_slice(&form_device_name(i));
        lb_add(h, 303, &t);
    }
    dlg_msg(h, 303, LM_SELECTITEM, a.cur as u32, 1);
}

fn selected(h: u32, id: u32) -> Option<usize> {
    let r = dlg_msg(h, id, LM_QUERYSELECTION, LIT_FIRST, 0) as i16;
    (r >= 0).then_some(r as usize)
}

extern "C" fn devlist_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            fill_device_list(h);
            0
        }
        WM_CONTROL if lo(mp1) == 303 && hi(mp1) == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            let sel = selected(h, 303);
            match lo(mp1) {
                DID_OK | 307 => {
                    if let Some(i) = sel {
                        a.cur = i.min(a.fig.forms.len() - 1);
                        a.sel = None;
                        after_change();
                    }
                    if lo(mp1) == DID_OK {
                        unsafe { WinDismissDlg(h, DID_OK) };
                    }
                }
                DID_CANCEL => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
                305 => {
                    add_device(h);
                    fill_device_list(h);
                }
                304 => {
                    if let Some(i) = sel {
                        if a.fig.forms.len() <= 1 {
                            message(h, b"A figure needs at least one form: the last cannot be deleted.", TITLE, MB_OK | MB_WARNING);
                        } else {
                            a.fig.forms.remove(i);
                            if a.cur > i {
                                a.cur -= 1;
                            }
                            a.cur = a.cur.min(a.fig.forms.len() - 1);
                            a.undo = None;
                            a.changed = true;
                            after_change();
                            fill_device_list(h);
                        }
                    }
                }
                309 | 308 => {
                    if let Some(i) = sel {
                        let j = if lo(mp1) == 309 { i.checked_sub(1) } else { (i + 1 < a.fig.forms.len()).then_some(i + 1) };
                        if let Some(j) = j {
                            a.fig.forms.swap(i, j);
                            if a.cur == i {
                                a.cur = j;
                            } else if a.cur == j {
                                a.cur = i;
                            }
                            a.undo = None;
                            a.changed = true;
                            fill_device_list(h);
                            dlg_msg(h, 303, LM_SELECTITEM, j as u32, 1);
                            after_change();
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_HELP => {
            dialog_help(20098);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Device > List.
pub fn device_list() {
    let a = app();
    let tpl = template(
        dialog_frame(b"Device List", DLG_DEVLIST, 228, 114),
        alloc::vec![
            list_box((8, 44, 213, 64), 303, 0),
            button(b"~Add...", (8, 25, 38), 305, 0).group(),
            button(b"~Delete", (52, 25, 38), 304, 0),
            button(b"~View", (96, 25, 38), 307, 0),
            button(b"~Up", (140, 25, 38), 309, 0),
            button(b"Do~wn", (184, 25, 38), 308, 0),
            button(b"~OK", (8, 6, 38), DID_OK, BS_DEFAULT),
            button(b"Cancel", (52, 6, 38), DID_CANCEL, 0),
            help_button_at(184, 6, 38),
        ],
    );
    run_dialog(a.frame, devlist_proc, &tpl, 0);
}

// ---- Add Device and Customize Device ----

static mut ADD_CHOICE: Option<usize> = None;

extern "C" fn adddev_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            for d in &a.devices {
                let (cx, cy) = devices::size_for(d, a.fig.kind, (a.fig.forms[0].cx, a.fig.forms[0].cy));
                let mut t = d.name.clone();
                t.extend_from_slice(alloc::format!("  {}x{}, {}", cx, cy, colors_name(d.bpp)).as_bytes());
                lb_add(h, 701, &t);
            }
            dlg_msg(h, 701, LM_SELECTITEM, 0, 1);
            0
        }
        WM_CONTROL if lo(mp1) == 701 && hi(mp1) == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                DID_OK => unsafe {
                    ADD_CHOICE = selected(h, 701);
                    WinDismissDlg(h, DID_OK);
                },
                703 => unsafe {
                    ADD_CHOICE = None;
                    WinDismissDlg(h, 703);
                },
                id => unsafe {
                    WinDismissDlg(h, id);
                },
            }
            0
        }
        WM_HELP => {
            dialog_help(20101);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

pub fn colors_name(bpp: u16) -> &'static str {
    match bpp {
        1 => "2 colors",
        4 => "16 colors",
        8 => "256 colors",
        _ => "RGB",
    }
}

/// A form for a device, rendered from the current form.
fn add_form(cx: u32, cy: u32, bpp: u16, display: (u16, u16), inherit: bool) {
    let a = app();
    let from = &a.fig.forms[a.cur];
    let mut f = edit::render(from, cx, cy, bpp, display);
    if bpp <= 8 && !(inherit && from.bpp == bpp) {
        let pal = default_palette(bpp);
        for (i, c) in pal.into_iter().enumerate() {
            f.palette[i] = c;
        }
        for i in 0..f.pels.len() {
            f.pels[i] = edit::fit(&f, f.pels[i]);
        }
    } else if bpp <= 8 {
        f.palette = from.palette.clone();
    }
    if a.fig.kind == Kind::Bitmap && a.fig.forms.is_empty() {
        f.hot = (0, 0);
    }
    a.fig.forms.push(f);
    a.cur = a.fig.forms.len() - 1;
    a.changed = true;
    a.undo = None;
    after_change();
}

fn add_device(owner: u32) {
    let a = app();
    let tpl = template(
        dialog_frame(b"Add Device", DLG_ADDDEV, 257, 110),
        alloc::vec![
            group_box(b"Select choice to add to device list.", (11, 25, 237, 83)),
            list_box((19, 32, 222, 64), 701, 0).group(),
            button(b"Cancel", (8, 6, 44), DID_CANCEL, 0).group(),
            button(b"~OK", (56, 6, 44), DID_OK, BS_DEFAULT),
            button(b"C~ustomize...", (104, 6, 60), 703, 0),
            help_button_at(206, 6, 44),
        ],
    );
    let r = run_dialog(owner, adddev_proc, &tpl, 0);
    if r == DID_OK {
        if let Some(i) = unsafe { ADD_CHOICE } {
            let d = a.devices[i].clone();
            let (cx, cy) = devices::size_for(&d, a.fig.kind, (a.fig.forms[0].cx, a.fig.forms[0].cy));
            if a.fig.forms.iter().any(|f| (f.cx, f.cy, f.bpp, f.display) == (cx, cy, d.bpp, d.display)) {
                message(owner, b"The device specified has already been defined.", TITLE, MB_OK | MB_INFORMATION);
                return;
            }
            add_form(cx, cy, d.bpp, d.display, d.inherit);
        }
    } else if r == 703 {
        customize(owner);
    }
}

static mut CUSTOM: (u16, u16, u32, u32, u16) = (0, 0, 32, 32, 4);

extern "C" fn custom_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            let (dx, dy, cx, cy, bpp) = unsafe { CUSTOM };
            set_num(h, 203, dx as i64);
            set_num(h, 204, dy as i64);
            set_num(h, 205, 96);
            set_num(h, 206, 96);
            set_num(h, 207, cx as i64);
            set_num(h, 208, cy as i64);
            set_check(h, match bpp { 1 => 209, 4 => 210, 8 => 211, _ => 212 }, true);
            0
        }
        WM_COMMAND if lo(mp1) == DID_OK => {
            let n = |id| get_num(h, id).unwrap_or(-1);
            let (cx, cy) = (n(207), n(208));
            if !(1..=format::MAX_SIDE as i64).contains(&cx) || !(1..=format::MAX_SIDE as i64).contains(&cy) || n(203) < 0 || n(204) < 0 {
                message(h, b"These bitmap attributes are not acceptable.", TITLE, MB_OK | MB_WARNING);
                return 0;
            }
            let bpp = if checked(h, 209) { 1 } else if checked(h, 211) { 8 } else if checked(h, 212) { 24 } else { 4 };
            unsafe {
                CUSTOM = (n(203) as u16, n(204) as u16, cx as u32, cy as u32, bpp);
                WinDismissDlg(h, DID_OK);
            }
            0
        }
        WM_HELP => {
            dialog_help(20108);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn customize(owner: u32) {
    let f = form();
    unsafe { CUSTOM = (f.display.0, f.display.1, f.cx, f.cy, f.bpp) };
    let tpl = template(
        dialog_frame(b"Customize Device", DLG_CUSTOM, 186, 98),
        alloc::vec![
            label(b"Device size:", (6, 77, 96, 12), DT_RIGHT | DT_VCENTER),
            num_entry((108, 79, 22, 8), 203).group(),
            label(b"x", (132, 77, 12, 12), DT_CENTER | DT_VCENTER),
            num_entry((148, 79, 22, 8), 204),
            label(b"Device resolution:", (6, 63, 96, 12), DT_RIGHT | DT_VCENTER),
            num_entry((108, 65, 22, 8), 205),
            label(b"x", (132, 63, 12, 12), DT_CENTER | DT_VCENTER),
            num_entry((148, 65, 22, 8), 206),
            label(b"Image size:", (6, 49, 96, 12), DT_RIGHT | DT_VCENTER),
            num_entry((108, 51, 22, 8), 207),
            label(b"x", (132, 49, 12, 12), DT_CENTER | DT_VCENTER),
            num_entry((148, 51, 22, 8), 208),
            label(b"Colors supported:", (6, 35, 96, 12), DT_RIGHT | DT_VCENTER),
            radio(b"~2", (106, 35, 30, 10), 209).group(),
            radio(b"~16", (142, 35, 34, 10), 210),
            radio(b"2~56", (106, 25, 34, 10), 211),
            radio(b"~RGB", (142, 25, 34, 10), 212),
            button(b"~OK", (10, 6, 44), DID_OK, BS_DEFAULT).group(),
            button(b"Cancel", (60, 6, 44), DID_CANCEL, 0),
            help_button_at(110, 6, 44),
        ],
    );
    if run_dialog(owner, custom_proc, &tpl, 0) == DID_OK {
        let (dx, dy, cx, cy, bpp) = unsafe { CUSTOM };
        add_form(cx, cy, bpp, (dx, dy), true);
    }
}

// ---- Edit Predefined Device Forms ----

static mut PREDEF: Vec<Device> = Vec::new();
static mut PREDEF_CUR: usize = 0;

fn predef() -> &'static mut Vec<Device> {
    unsafe { &mut *core::ptr::addr_of_mut!(PREDEF) }
}

fn show_device(h: u32) {
    let list = predef();
    let Some(d) = list.get(unsafe { PREDEF_CUR }) else { return };
    set_check(h, if d.inherit { 506 } else { 507 }, true);
    set_check(h, 522, d.recommended);
    set_check(h, match d.bpp { 1 => 508, 4 => 509, 8 => 510, _ => 511 }, true);
    for (id, v) in [(512, d.icon.0), (513, d.icon.1), (514, d.pointer.0), (515, d.pointer.1), (516, d.bitmap.0), (517, d.bitmap.1), (518, d.display.0), (519, d.display.1), (521, d.res.0), (520, d.res.1)] {
        set_num(h, id, v as i64);
    }
}

/// Take the fields into the device shown; false if a number is bad.
fn take_device(h: u32) -> bool {
    let list = predef();
    let Some(d) = list.get_mut(unsafe { PREDEF_CUR }) else { return true };
    let n = |id| get_num(h, id).filter(|&v| (0..=4096).contains(&v)).map(|v| v as u16);
    let (Some(ix), Some(iy), Some(px), Some(py), Some(bx), Some(by), Some(dx), Some(dy), Some(rx), Some(ry)) = (n(512), n(513), n(514), n(515), n(516), n(517), n(518), n(519), n(521), n(520)) else {
        return false;
    };
    if ix == 0 || iy == 0 || px == 0 || py == 0 || ix as u32 > format::MAX_SIDE || iy as u32 > format::MAX_SIDE {
        return false;
    }
    d.icon = (ix, iy);
    d.pointer = (px, py);
    d.bitmap = (bx.max(1), by.max(1));
    d.display = (dx, dy);
    d.res = (rx, ry);
    d.inherit = checked(h, 506);
    d.recommended = checked(h, 522);
    d.bpp = if checked(h, 508) { 1 } else if checked(h, 510) { 8 } else if checked(h, 511) { 24 } else { 4 };
    true
}

fn fill_predef(h: u32) {
    dlg_msg(h, 505, LM_DELETEALL, 0, 0);
    for d in predef().iter() {
        lb_add(h, 505, &d.name);
    }
    dlg_msg(h, 505, LM_SELECTITEM, unsafe { PREDEF_CUR } as u32, 1);
    show_device(h);
}

extern "C" fn predef_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            fill_predef(h);
            0
        }
        WM_CONTROL if lo(mp1) == 505 && hi(mp1) == 4 => {
            // CBN_LBSELECT: show the device chosen (keeping the one shown).
            let i = dlg_msg(h, 505, LM_QUERYSELECTION, LIT_FIRST, 0) as i16;
            if i >= 0 && i as usize != unsafe { PREDEF_CUR } {
                take_device(h);
                unsafe { PREDEF_CUR = i as usize };
                show_device(h);
            }
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                DID_OK => {
                    if !take_device(h) {
                        message(h, b"These bitmap attributes are not acceptable.", TITLE, MB_OK | MB_WARNING);
                        return 0;
                    }
                    unsafe { WinDismissDlg(h, DID_OK) };
                }
                503 => {
                    take_device(h);
                    let list = predef();
                    let mut d = list.get(unsafe { PREDEF_CUR }).cloned().unwrap_or_else(|| devices::builtin()[0].clone());
                    let n = list.iter().filter(|d| d.name.starts_with(b"Custom")).count() + 1;
                    d.name = alloc::format!("Custom {n}").into_bytes();
                    d.recommended = false;
                    list.push(d);
                    unsafe { PREDEF_CUR = list.len() - 1 };
                    fill_predef(h);
                }
                504 => {
                    let list = predef();
                    if list.len() <= 1 {
                        message(h, b"The list needs at least one device form.", TITLE, MB_OK | MB_WARNING);
                    } else {
                        list.remove(unsafe { PREDEF_CUR });
                        unsafe { PREDEF_CUR = PREDEF_CUR.min(list.len() - 1) };
                        fill_predef(h);
                    }
                }
                id => unsafe {
                    WinDismissDlg(h, id);
                },
            }
            0
        }
        WM_HELP => {
            dialog_help(20119);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Device > Edit predefined.
pub fn edit_predefined() {
    let a = app();
    unsafe {
        PREDEF = a.devices.clone();
        PREDEF_CUR = 0;
    }
    let mut items = alloc::vec![
        label(b"Default forms list", (8, 121, 120, 10), DT_LEFT),
        pmkit::tpl::item(WC_COMBOBOX, b"", WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST, (8, 21, 237, 99), 505),
        group_box(b"Palette", (8, 76, 80, 30)),
        radio(b"~Inherited", (12, 88, 70, 10), 506).group(),
        radio(b"~Predefined", (12, 78, 70, 10), 507),
        check_box(b"~Recommended", (10, 58, 80, 11), 522).group(),
        group_box(b"Colors", (8, 23, 80, 30)),
        radio(b"~2", (12, 35, 32, 10), 508).group(),
        radio(b"~16", (50, 35, 32, 10), 509),
        radio(b"2~56", (12, 25, 32, 10), 510),
        radio(b"R~GB", (50, 25, 32, 10), 511),
        group_box(b"Default sizes", (93, 23, 152, 82)),
    ];
    let rows: [(&[u8], u32, u32, i16); 5] = [(b"Icon:", 512, 513, 84), (b"Pointer:", 514, 515, 70), (b"Bitmap (%):", 516, 517, 56), (b"Device size:", 518, 519, 42), (b"Resolution:", 521, 520, 28)];
    for (t, a_id, b_id, y) in rows {
        items.push(label(t, (98, y, 70, 12), DT_RIGHT | DT_VCENTER));
        items.push(num_entry((174, y + 2, 24, 8), a_id).group());
        items.push(label(b"x", (200, y, 10, 12), DT_CENTER | DT_VCENTER));
        items.push(num_entry((214, y + 2, 24, 8), b_id));
    }
    items.push(button(b"~OK", (6, 4, 44), DID_OK, BS_DEFAULT).group());
    items.push(button(b"Cancel", (55, 4, 44), DID_CANCEL, 0));
    items.push(button(b"~New", (104, 4, 44), 503, 0));
    items.push(button(b"D~elete", (153, 4, 44), 504, 0));
    items.push(help_button_at(202, 4, 44));
    let tpl = template(dialog_frame(b"Edit Predefined Device Forms", DLG_PREDEF, 253, 136), items);
    if run_dialog(a.frame, predef_proc, &tpl, 0) == DID_OK {
        a.devices = predef().clone();
        ini::write(INI_APP, b"DEVICES", &devices::to_text(&a.devices));
        after_change();
    }
}

// ---- The clipboard ----

const PAG_READ: u32 = 0x1;
const PAG_WRITE: u32 = 0x2;
const PAG_COMMIT: u32 = 0x10;
const OBJ_GIVEABLE: u32 = 0x200;

/// Edit > Copy: the area as the editor's own format (with the screen and
/// inverse pels) and as a bitmap for other programs.
pub fn copy(area: Area) {
    let a = app();
    let (w, h, pels) = edit::extract(form(), area.clip(form()));
    if w == 0 || h == 0 {
        return;
    }
    if unsafe { WinOpenClipbrd(a.hab) } == 0 {
        message(a.frame, b"The clipboard cannot be opened or emptied.", TITLE, MB_OK | MB_ERROR);
        return;
    }
    unsafe { WinEmptyClipbrd(a.hab) };
    let size = 8 + pels.len() * 4;
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), size as u32, PAG_COMMIT | PAG_READ | PAG_WRITE | OBJ_GIVEABLE) } == 0 {
        let mut d = Vec::with_capacity(size);
        d.extend_from_slice(&w.to_le_bytes());
        d.extend_from_slice(&h.to_le_bytes());
        for v in &pels {
            d.extend_from_slice(&v.to_le_bytes());
        }
        unsafe {
            core::ptr::copy_nonoverlapping(d.as_ptr(), p, d.len());
            WinSetClipbrdData(a.hab, p as u32, a.clip_fmt, CFI_POINTER);
        }
    }
    // The bitmap: screen pels white, inverse black.
    let rgb: Vec<u32> = pels.iter().map(|&v| match v {
        SCREEN => 0xffffff,
        INVERSE => 0,
        c => c,
    }).collect();
    let bits = gpi::pack_rgb(&rgb, w as usize, h as usize);
    if let Some(m) = gpi::MemPs::new(a.hab, w as i32, h as i32) {
        let mut bih = BitmapInfoHeader2::new(w, h, 24);
        bih.cb = 16;
        let hbm = unsafe { GpiCreateBitmap(m.hps, &bih, CBM_INIT, bits.as_ptr(), &bih as *const BitmapInfoHeader2 as *const u8) };
        if hbm != 0 {
            unsafe { WinSetClipbrdData(a.hab, hbm, CF_BITMAP, CFI_HANDLE) };
        }
    }
    unsafe { WinCloseClipbrd(a.hab) };
    trace(alloc::format!("copied {w}x{h}").as_bytes());
}

/// What Paste puts in: the editor's own format, else a bitmap.
pub fn paste_data() -> Option<(u32, u32, Vec<u32>)> {
    let a = app();
    if unsafe { WinOpenClipbrd(a.hab) } == 0 {
        return None;
    }
    let mut out = None;
    let own = unsafe { WinQueryClipbrdData(a.hab, a.clip_fmt) };
    if own != 0 {
        let p = own as *const u8;
        let hdr = unsafe { core::slice::from_raw_parts(p, 8) };
        let (w, h) = (rd32(hdr, 0), rd32(hdr, 4));
        if w > 0 && h > 0 && w <= format::MAX_SIDE && h <= format::MAX_SIDE {
            let d = unsafe { core::slice::from_raw_parts(p.add(8), (w * h * 4) as usize) };
            out = Some((w, h, d.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()));
        }
    } else {
        let hbm = unsafe { WinQueryClipbrdData(a.hab, CF_BITMAP) };
        if hbm != 0 {
            let mut bih = BitmapInfoHeader2::new(0, 0, 0);
            bih.cb = 16;
            unsafe { GpiQueryBitmapInfoHeader(hbm, &mut bih) };
            let (w, h) = (bih.cx.min(format::MAX_SIDE), bih.cy.min(format::MAX_SIDE));
            if w > 0 && h > 0 {
                if let Some(m) = gpi::MemPs::new(a.hab, w as i32, h as i32) {
                    unsafe { WinDrawBitmap(m.hps, hbm, core::ptr::null(), &pt(0, 0), CLR_BLACK, CLR_WHITE, DBM_NORMAL) };
                    let mut q = BitmapInfoHeader2::new(w, h, 24);
                    q.cb = 16;
                    let stride = ((w * 3 + 3) & !3) as usize;
                    let mut bits = alloc::vec![0u8; stride * h as usize];
                    unsafe { GpiQueryBitmapBits(m.hps, 0, h as i32, bits.as_mut_ptr(), &mut q as *mut BitmapInfoHeader2 as *mut u8) };
                    let mut pels = alloc::vec![0u32; (w * h) as usize];
                    for y in 0..h as usize {
                        let row = &bits[(h as usize - 1 - y) * stride..];
                        for x in 0..w as usize {
                            pels[y * w as usize + x] = (row[x * 3 + 2] as u32) << 16 | (row[x * 3 + 1] as u32) << 8 | row[x * 3] as u32;
                        }
                    }
                    out = Some((w, h, pels));
                }
            }
        }
    }
    unsafe { WinCloseClipbrd(a.hab) };
    if out.is_none() {
        message(a.frame, b"The clipboard holds nothing the editor can paste.", TITLE, MB_OK | MB_INFORMATION);
    }
    out
}
