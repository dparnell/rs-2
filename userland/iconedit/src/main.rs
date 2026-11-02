//! ICONEDIT.EXE: rs2's Icon Editor, a re-implementation of OS/2's.
//!
//! As OS/2's (studied in its binary and by running it): one window titled
//! "Icon Editor - FILE" with a status area along the top (a mouse showing
//! the colours of its two buttons, the figure at its real size, the form
//! size, pen location, pen size and hot spot, the figure type and the
//! form's device), the magnified edit area and a palette down the right
//! (Screen and Inverse for icons and pointers, then the form's colours:
//! button 1 picks the colour of mouse button 1, button 2 of button 2).
//! The menus, accelerators, dialogs and help panel numbers are OS/2's;
//! settings live in OS2.INI (application ICONEDIT).
//!
//! A figure (icon, pointer or bitmap) holds forms for several devices
//! (sizes and colour depths): Device > List shows them, adds forms
//! rendered from the current one and deletes and orders them. The file
//! formats are in format.rs (1.x and 2.0 headers, bitmap arrays,
//! compressed 2.0), the tools in edit.rs.
//!
//! `ICONEDIT file` opens a file; `ICONEDIT /SELFTEST` runs the format and
//! tool checks and prints "iconedit selftest done: N failure(s)".

#![no_std]
#![no_main]

extern crate alloc;

mod devices;
mod dialogs;
mod edit;
mod format;

use alloc::vec::Vec;

use edit::Area;
use format::{Figure, Form, Format, INVERSE, Kind, SCREEN};
use pmkit::gpi;
use pmkit::help::{Help, HelpWindow};
use pmkit::*;

os2::entry!(main);

// Menu commands (OS/2's ids).
pub const IDM_FILE: u16 = 3200;
pub const IDM_OPEN: u16 = 3201;
pub const IDM_NEXT: u16 = 3202;
pub const IDM_NEW: u16 = 3203;
pub const IDM_SAVE: u16 = 3204;
pub const IDM_SAVEAS: u16 = 3205;
pub const IDM_EDIT: u16 = 3232;
pub const IDM_UNDO: u16 = 3233;
pub const IDM_SELECT: u16 = 3234;
pub const IDM_SELECTALL: u16 = 3235;
pub const IDM_CUT: u16 = 3236;
pub const IDM_COPY: u16 = 3237;
pub const IDM_PASTE: u16 = 3238;
pub const IDM_CLEAR: u16 = 3239;
pub const IDM_STRETCHPASTE: u16 = 3240;
pub const IDM_FILL: u16 = 3241;
pub const IDM_FLIPH: u16 = 3242;
pub const IDM_FLIPV: u16 = 3243;
pub const IDM_CIRCLE: u16 = 3244;
pub const IDM_PALETTE: u16 = 3264;
pub const IDM_PALDEFAULT: u16 = 3265;
pub const IDM_PALOPEN: u16 = 3266;
pub const IDM_PALSAVE: u16 = 3267;
pub const IDM_PALSAVEAS: u16 = 3268;
pub const IDM_EDITCOLOR: u16 = 3269;
pub const IDM_SETDEFPAL: u16 = 3270;
pub const IDM_SWAP: u16 = 3392;
pub const IDM_SWAPKEEP: u16 = 3393;
pub const IDM_SWAPCHANGE: u16 = 3394;
pub const IDM_OPTIONS: u16 = 3296;
pub const IDM_TEST: u16 = 3297;
pub const IDM_GRID: u16 = 3298;
pub const IDM_XBG: u16 = 3299;
pub const IDM_STRAIGHT: u16 = 3300;
pub const IDM_PENSHAPE: u16 = 3301;
pub const IDM_HOTSPOT: u16 = 3302;
pub const IDM_PENSIZE: u16 = 3328;
pub const IDM_PEN1: u16 = 3329;
pub const IDM_PREFS: u16 = 3360;
pub const IDM_SAFE: u16 = 3361;
pub const IDM_NOWARN: u16 = 3362;
pub const IDM_SAVESTATE: u16 = 3363;
pub const IDM_STATUS: u16 = 3364;
pub const IDM_RESET: u16 = 3365;
pub const IDM_SAVEPROFILE: u16 = 3366;
pub const IDM_DEVICE: u16 = 3424;
pub const IDM_DEVLIST: u16 = 3425;
pub const IDM_DEVEDIT: u16 = 3427;
pub const IDM_TOOLS: u16 = 3456;
pub const IDM_COLORFILL: u16 = 3457;
pub const IDM_FINDCOLOR: u16 = 3458;
pub const IDM_HELP: u16 = 3488;
pub const IDM_HELPINDEX: u16 = 3489;
pub const IDM_GENERALHELP: u16 = 3490;
pub const IDM_USINGHELP: u16 = 3491;
pub const IDM_KEYSHELP: u16 = 3492;
pub const IDM_PRODUCT: u16 = 3493;
/// Esc: leave the current mode.
pub const IDM_ESCAPE: u16 = 73;

/// Dialog ids (their help subtables).
pub const DLG_NEW: u16 = 101;
pub const DLG_CUSTOM: u16 = 201;
pub const DLG_DEVLIST: u16 = 301;
pub const DLG_COLOR: u16 = 401;
pub const DLG_PREDEF: u16 = 501;
pub const DLG_SAVEOPTS: u16 = 600;
pub const DLG_ADDDEV: u16 = 700;
pub const DLG_PRODUCT: u16 = 900;

/// Help panels (res= numbers of ICONEDIT.HLP).
pub const PANEL_GENERAL: u16 = 20086;
pub const PANEL_KEYS: u16 = 64025;

const FRAME_ID: u32 = 1;
pub const HELP_FILE: &[u8] = b"ICONEDIT.HLP";
pub const TITLE: &[u8] = b"Icon Editor";
pub const INI_APP: &[u8] = b"ICONEDIT";
const CLASS: &[u8] = b"Iconedit.IconEdit";

/// How Screen and Inverse pels look in the edit area.
pub const SCREEN_RGB: u32 = 0x80ff80;
pub const INVERSE_RGB: u32 = 0x800080;

/// What a mouse button paints with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Screen,
    Inverse,
    Pal(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Draw,
    /// Edit > Select: dragging out a rectangle.
    Select,
    Hotspot,
    ColorFill,
    FindColor,
    /// Options > Set pen shape: the arrow keys size the pen.
    PenShape,
}

/// The options kept in OS2.INI (OPTIONS) when "Save state on exit" is on
/// or with Save profile.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Opts {
    pub grid: u32,
    pub xbg: u32,
    pub straight: u32,
    pub pen_w: u32,
    pub pen_h: u32,
    pub safe: u32,
    pub nowarn: u32,
    pub save_state: u32,
    pub status: u32,
}

pub const DEFAULT_OPTS: Opts = Opts { grid: 1, xbg: 0, straight: 0, pen_w: 1, pen_h: 1, safe: 1, nowarn: 0, save_state: 1, status: 1 };

struct Drag {
    button: u8,
    start: (i32, i32),
    last: (i32, i32),
    /// Draw straight: the first move decides (true: horizontal).
    axis: Option<bool>,
}

pub struct App {
    pub hab: u32,
    pub frame: u32,
    pub client: u32,
    pub menu: u32,
    pub help: Option<&'static mut Help>,
    pub fig: Figure,
    /// The form being edited.
    pub cur: usize,
    pub path: Option<Vec<u8>>,
    pub changed: bool,
    pub left: Pick,
    pub right: Pick,
    /// The colours offered for true colour forms.
    pub draw_pal: Vec<u32>,
    pub mode: Mode,
    pub opts: Opts,
    pub undo: Option<(usize, Form)>,
    pub sel: Option<Area>,
    drag: Option<Drag>,
    /// The pen location (keyboard and mouse).
    pub pen_at: Option<(i32, i32)>,
    pub test_ptr: u32,
    pub test: bool,
    pub devices: Vec<Device>,
    pub pal_path: Option<Vec<u8>>,
    pub pal_changed: bool,
    pub clip_fmt: u32,
}

pub use devices::Device;

static mut APP: App = App {
    hab: 0,
    frame: 0,
    client: 0,
    menu: 0,
    help: None,
    fig: Figure { kind: Kind::Icon, forms: Vec::new(), format: Format::V20 },
    cur: 0,
    path: None,
    changed: false,
    left: Pick::Pal(1),
    right: Pick::Pal(15),
    draw_pal: Vec::new(),
    mode: Mode::Draw,
    opts: DEFAULT_OPTS,
    undo: None,
    sel: None,
    drag: None,
    pen_at: None,
    test_ptr: 0,
    test: false,
    devices: Vec::new(),
    pal_path: None,
    pal_changed: false,
    clip_fmt: 0,
};

pub fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

pub fn trace(s: &[u8]) {
    pmkit::trace("iconedit", s);
}

fn main(s: &os2::Startup) -> u32 {
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        return 1;
    }
    let arg = pmkit::arg(s);
    if arg.eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    load_settings();
    new_figure(Kind::Icon, (32, 32));
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(a.hab, CLASS, client_proc, CS_SIZEREDRAW, 0, fcf, TITLE, FRAME_ID);
    if frame == 0 {
        return 1;
    }
    a.frame = frame;
    a.client = client;
    a.menu = set_menu(frame, &menu_items());
    set_accels(a.hab, frame, &accels());
    a.clip_fmt = unsafe { WinAddAtom(WinQuerySystemAtomTable(), b"IconEdit Color Bits Format\0".as_ptr()) };
    create_help();
    if !arg.is_empty() {
        open_file(arg, true);
    }
    update_menus();
    set_title();
    show_at_saved(a.hab, frame, INI_APP, b"WINDOWPOS", Some((612, 458)));
    loop {
        message_loop(a.hab);
        if query_save() {
            break;
        }
        unsafe { WinCancelShutdown(hmq, 0) };
    }
    save_position(frame, INI_APP, b"WINDOWPOS");
    if a.opts.save_state != 0 {
        save_options();
    }
    unsafe {
        if a.test_ptr != 0 {
            WinDestroyPointer(a.test_ptr);
        }
        if let Some(h) = a.help.as_mut() {
            h.destroy(frame);
        }
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

fn selftest() -> u32 {
    let report = |name: &str, ok: bool| {
        let line = alloc::format!("{} {name}", if ok { "PASS" } else { "FAIL" });
        trace(line.as_bytes());
        os2::println!("{}", line);
    };
    let fails = format::selftest(report) + edit::selftest(report);
    let d = devices::builtin();
    let ok = devices::from_text(&devices::to_text(&d)) == d;
    report("device list in OS2.INI form", ok);
    let fails = fails + !ok as u32;
    let line = alloc::format!("iconedit selftest done: {fails} failure(s)");
    trace(line.as_bytes());
    os2::println!("{}", line);
    fails
}

// ---- Settings ----

fn load_settings() {
    let a = app();
    let mut o = DEFAULT_OPTS;
    if ini::read_struct(INI_APP, b"OPTIONS", &mut o) {
        o.pen_w = o.pen_w.clamp(1, 9);
        o.pen_h = o.pen_h.clamp(1, 9);
        a.opts = o;
    }
    a.devices = ini::read(INI_APP, b"DEVICES").map(|t| devices::from_text(&t)).filter(|l| !l.is_empty()).unwrap_or_else(devices::builtin);
    a.draw_pal = format::default_palette(4);
}

pub fn save_options() {
    ini::write_struct(INI_APP, b"OPTIONS", &app().opts);
}

/// The default palette for a colour depth: the one Set default palette
/// kept, else OS/2's.
pub fn default_palette(bpp: u16) -> Vec<u32> {
    let key = alloc::format!("PALETTE{bpp}");
    if let Some(v) = ini::read(INI_APP, key.as_bytes()) {
        let n = 1usize << bpp.min(8);
        if bpp <= 8 && v.len() == n * 4 {
            return v.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        }
    }
    format::default_palette(bpp)
}

// ---- The figure ----

pub fn form() -> &'static mut Form {
    let a = app();
    &mut a.fig.forms[a.cur]
}

/// The colours the palette shows for the current form.
pub fn palette() -> &'static [u32] {
    let a = app();
    let f = &a.fig.forms[a.cur];
    if f.bpp > 8 { &a.draw_pal } else { &f.palette }
}

/// A new figure: icons and pointers get the recommended device forms,
/// bitmaps one form of the size given.
pub fn new_figure(kind: Kind, size: (u32, u32)) {
    let a = app();
    let mut forms = Vec::new();
    match kind {
        Kind::Bitmap => {
            let mut f = Form::new(size.0, size.1, 4, 0xffffff);
            f.palette = default_palette(4);
            f.hot = (0, 0);
            forms.push(f);
        }
        _ => {
            for d in a.devices.iter().filter(|d| d.recommended && (kind == Kind::Icon || d.icon == d.pointer)) {
                let (cx, cy) = devices::size_for(d, kind, (32, 32));
                let mut f = Form::new(cx, cy, d.bpp, SCREEN);
                if d.bpp <= 8 {
                    f.palette = default_palette(d.bpp);
                }
                f.display = d.display;
                if kind == Kind::Pointer {
                    f.hot = (0, cy as i32 - 1);
                }
                forms.push(f);
            }
            if forms.is_empty() {
                forms.push(Form::new(32, 32, 4, SCREEN));
            }
        }
    }
    a.fig = Figure { kind, forms, format: Format::V20 };
    a.cur = 0;
    a.path = None;
    a.changed = false;
    a.undo = None;
    a.sel = None;
    a.mode = Mode::Draw;
    fix_picks();
}

/// Keep the buttons' colours valid for the current form.
pub fn fix_picks() {
    let a = app();
    let n = palette().len();
    let icon = a.fig.kind != Kind::Bitmap;
    for p in [&mut a.left, &mut a.right] {
        match *p {
            Pick::Pal(i) if i >= n => *p = Pick::Pal(n.saturating_sub(1)),
            Pick::Screen | Pick::Inverse if !icon => *p = Pick::Pal(0),
            _ => {}
        }
    }
}

pub fn pick_value(p: Pick) -> u32 {
    match p {
        Pick::Screen => SCREEN,
        Pick::Inverse => INVERSE,
        Pick::Pal(i) => *palette().get(i).unwrap_or(&0),
    }
}

/// Keep the form for Undo (before a change).
pub fn checkpoint() {
    let a = app();
    a.undo = Some((a.cur, a.fig.forms[a.cur].clone()));
    a.changed = true;
}

pub fn set_title() {
    let a = app();
    let mut t = TITLE.to_vec();
    t.extend_from_slice(b" - ");
    match &a.path {
        Some(p) => t.extend_from_slice(file_name(p)),
        None => t.extend_from_slice(b"(Untitled)"),
    }
    set_text(a.frame, &t);
}

pub fn kind_name(k: Kind) -> &'static [u8] {
    match k {
        Kind::Icon => b"Icon",
        Kind::Pointer => b"Pointer",
        Kind::Bitmap => b"Bitmap",
    }
}

/// The name of the current form's device ("Custom" when none matches).
pub fn form_device_name(i: usize) -> Vec<u8> {
    let a = app();
    let base = (a.fig.forms[0].cx, a.fig.forms[0].cy);
    match devices::find(&a.devices, a.fig.kind, &a.fig.forms[i], base) {
        Some(d) => d.name.clone(),
        None => b"Custom".to_vec(),
    }
}

/// Read a file into the editor; false (after saying why) when it cannot.
pub fn open_file(path: &[u8], quiet_missing: bool) -> bool {
    let a = app();
    let Some(data) = read_file(path) else {
        if !quiet_missing {
            let t = [&b"Cannot open file "[..], path, b"."].concat();
            message(a.frame, &t, TITLE, MB_OK | MB_ERROR);
        } else {
            // A name that does not exist yet: a new figure of the kind its
            // extension says, saved under that name.
            let kind = match extension(path).as_slice() {
                b"PTR" => Kind::Pointer,
                b"BMP" => Kind::Bitmap,
                _ => Kind::Icon,
            };
            new_figure(kind, (64, 64));
            a.path = Some(path.to_vec());
        }
        return false;
    };
    match format::read(&data) {
        Ok(fig) => {
            a.fig = fig;
            a.cur = 0;
            a.path = Some(path.to_vec());
            a.changed = false;
            a.undo = None;
            a.sel = None;
            a.mode = Mode::Draw;
            fix_picks();
            trace(alloc::format!("opened {} ({} forms)", core::str::from_utf8(path).unwrap_or("?"), a.fig.forms.len()).as_bytes());
            true
        }
        Err(e) => {
            let t = [&b"The file "[..], path, &b" cannot be read or is not a proper image file ("[..], e.as_bytes(), b")."].concat();
            message(a.frame, &t, TITLE, MB_OK | MB_ERROR);
            trace(e.as_bytes());
            false
        }
    }
}

/// Write the figure (the forms chosen, in a layout).
pub fn save_file(path: &[u8], which: &[usize], fmt: Format) -> bool {
    let a = app();
    let data = format::write(&a.fig, which, fmt);
    if let Err(rc) = write_file(path, &data) {
        let t = alloc::format!("Error writing to {} (error {rc}).", core::str::from_utf8(path).unwrap_or("?"));
        message(a.frame, t.as_bytes(), TITLE, MB_OK | MB_ERROR);
        return false;
    }
    if data.len() > 65536 && a.opts.nowarn == 0 {
        let t = [&b"Warning! The file "[..], path, b" is larger than 64K bytes. This may restrict system use of the file."].concat();
        message(a.frame, &t, TITLE, MB_OK | MB_WARNING);
    }
    a.path = Some(path.to_vec());
    a.fig.format = fmt;
    a.changed = false;
    set_title();
    trace(alloc::format!("saved {} bytes, {} forms", data.len(), which.len()).as_bytes());
    true
}

/// Before the figure is replaced or the window closes: ask about changes.
/// False when the user cancels.
pub fn query_save() -> bool {
    let a = app();
    if !a.changed || a.opts.safe == 0 {
        return true;
    }
    let name: Vec<u8> = a.path.as_deref().map(|p| file_name(p).to_vec()).unwrap_or(b"(Untitled)".to_vec());
    let t = [&name[..], b" has changed. Save current changes?"].concat();
    match message(a.frame, &t, TITLE, MB_YESNOCANCEL | MB_ICONQUESTION) {
        MBID_YES => dialogs::save(false),
        MBID_NO => true,
        _ => false,
    }
}

/// File > Next: the next file of the same kind in the directory.
fn next_file() {
    let a = app();
    let Some(p) = a.path.clone() else {
        message(a.frame, b"Open a file first: Next opens the next file in its directory.", TITLE, MB_OK | MB_INFORMATION);
        return;
    };
    let cut = p.iter().rposition(|&c| c == b'\\' || c == b':').map_or(0, |i| i + 1);
    let dir = &p[..cut];
    let pattern = [dir, b"*.", &extension(&p)[..]].concat();
    let mut names = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    let mut h: u32 = 0xffff_ffff;
    let mut n: u32 = 64;
    let pz = z(&pattern);
    let mut rc = unsafe { os2::DosFindFirst(pz.as_ptr(), &mut h, 0, buf.as_mut_ptr(), buf.len() as u32, &mut n, 1) };
    while rc == 0 && n > 0 {
        // FILEFINDBUF3: oNextEntryOffset, dates, sizes, attributes, cchName at 26, achName at 27.
        let mut o = 0usize;
        for _ in 0..n {
            let next = rd32(&buf, o) as usize;
            let len = buf[o + 26] as usize;
            names.push(buf[o + 27..o + 27 + len].to_vec());
            if next == 0 {
                break;
            }
            o += next;
        }
        n = 64;
        rc = unsafe { os2::DosFindNext(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    }
    unsafe { os2::DosFindClose(h) };
    names.sort_by_key(|n| n.to_ascii_uppercase());
    let me = file_name(&p).to_ascii_uppercase();
    let next = names.iter().find(|n| n.to_ascii_uppercase() > me).or(names.first());
    match next {
        Some(n) if n.to_ascii_uppercase() != me => {
            if query_save() {
                let path = [dir, &n[..]].concat();
                open_file(&path, false);
                after_change();
            }
        }
        _ => {
            message(a.frame, b"There is no other file of this kind in the directory.", TITLE, MB_OK | MB_INFORMATION);
        }
    }
}

/// After the figure or form was replaced: menus, title, test pointer,
/// redraw.
pub fn after_change() {
    fix_picks();
    update_menus();
    set_title();
    refresh_test_pointer();
    invalidate(app().client);
}

// ---- Menus and keys ----

fn menu_items() -> Vec<Menu<'static>> {
    let pens = [b"~1 x 1\tCtrl+1", b"~2 x 2\tCtrl+2", b"~3 x 3\tCtrl+3", b"~4 x 4\tCtrl+4", b"~5 x 5\tCtrl+5", b"~6 x 6\tCtrl+6", b"~7 x 7\tCtrl+7", b"~8 x 8\tCtrl+8", b"~9 x 9\tCtrl+9"];
    alloc::vec![
        Menu::Sub(IDM_FILE, b"~File", alloc::vec![
            Menu::Item(IDM_NEW, b"~New...\tCtrl+N"),
            Menu::Item(IDM_OPEN, b"~Open...\tCtrl+O"),
            Menu::Sep,
            Menu::Item(IDM_SAVE, b"~Save\tCtrl+S"),
            Menu::Item(IDM_SAVEAS, b"Save ~as..."),
            Menu::Sep,
            Menu::Item(IDM_NEXT, b"N~ext\tCtrl+E"),
        ]),
        Menu::Sub(IDM_EDIT, b"~Edit", alloc::vec![
            Menu::Item(IDM_UNDO, b"U~ndo\tAlt+Backspace"),
            Menu::Sep,
            Menu::Item(IDM_CUT, b"C~ut\tShift+Delete"),
            Menu::Item(IDM_COPY, b"Cop~y\tCtrl+Insert"),
            Menu::Item(IDM_PASTE, b"~Paste\tShift+Insert"),
            Menu::Sep,
            Menu::Item(IDM_CLEAR, b"~Clear\tDelete"),
            Menu::Sep,
            Menu::Item(IDM_SELECT, b"~Select\tCtrl+A"),
            Menu::Item(IDM_SELECTALL, b"Select ~all\tAlt+A"),
            Menu::Sep,
            Menu::Item(IDM_STRETCHPASTE, b"S~tretch paste\tAlt+I"),
            Menu::Item(IDM_FILL, b"~Fill\tCtrl+Delete"),
            Menu::Item(IDM_FLIPH, b"Flip hori~zontal\tCtrl+H"),
            Menu::Item(IDM_FLIPV, b"Flip ~vertical\tCtrl+V"),
            Menu::Item(IDM_CIRCLE, b"Ci~rcle\tCtrl+C"),
        ]),
        Menu::Sub(IDM_PALETTE, b"~Palette", alloc::vec![
            Menu::Item(IDM_PALDEFAULT, b"~Load default palette\tAlt+N"),
            Menu::Item(IDM_PALOPEN, b"~Open...\tAlt+M"),
            Menu::Item(IDM_PALSAVE, b"~Save\tAlt+S"),
            Menu::Item(IDM_PALSAVEAS, b"S~ave as..."),
            Menu::Sep,
            Menu::Item(IDM_EDITCOLOR, b"~Edit color...\tAlt+C"),
            Menu::Sub(IDM_SWAP, b"S~wap colors", alloc::vec![
                Menu::Item(IDM_SWAPKEEP, b"~Preserve figure\tAlt+U"),
                Menu::Item(IDM_SWAPCHANGE, b"~Don't preserve figure\tAlt+V"),
            ]),
            Menu::Item(IDM_SETDEFPAL, b"Set ~default palette\tAlt+G"),
        ]),
        Menu::Sub(IDM_OPTIONS, b"~Options", alloc::vec![
            Menu::Item(IDM_TEST, b"~Test\tCtrl+T"),
            Menu::Item(IDM_GRID, b"~Grid\tCtrl+G"),
            Menu::Item(IDM_XBG, b"~X background\tCtrl+X"),
            Menu::Item(IDM_STRAIGHT, b"~Draw straight\tCtrl+-"),
            Menu::Sub(IDM_PENSIZE, b"~Pen size", pens.iter().enumerate().map(|(i, t)| Menu::Item(IDM_PEN1 + i as u16, &t[..])).collect()),
            Menu::Sub(IDM_PREFS, b"P~references", alloc::vec![
                Menu::Item(IDM_SAFE, b"~Safe prompting\tAlt+X"),
                Menu::Item(IDM_NOWARN, b"Suppress ~warnings\tAlt+W"),
                Menu::Item(IDM_SAVESTATE, b"S~ave state on exit\tAlt+Y"),
                Menu::Item(IDM_STATUS, b"~Display status area\tAlt+Z"),
                Menu::Item(IDM_RESET, b"~Reset options and modes\tAlt+R"),
                Menu::Item(IDM_SAVEPROFILE, b"Save ~profile\tAlt+B"),
            ]),
            Menu::Sep,
            Menu::Item(IDM_PENSHAPE, b"S~et pen shape\tAlt+J"),
            Menu::Item(IDM_HOTSPOT, b"~Hotspot\tAlt+K"),
        ]),
        Menu::Sub(IDM_DEVICE, b"~Device", alloc::vec![
            Menu::Item(IDM_DEVLIST, b"~List...\tCtrl+L"),
            Menu::Item(IDM_DEVEDIT, b"~Edit predefined...\tCtrl+W"),
        ]),
        Menu::Sub(IDM_TOOLS, b"~Tools", alloc::vec![
            Menu::Item(IDM_COLORFILL, b"Color ~fill\tCtrl+F"),
            Menu::Item(IDM_FINDCOLOR, b"Find ~color\tAlt+Q"),
        ]),
        Menu::Sub(IDM_HELP, b"~Help", alloc::vec![
            Menu::Item(IDM_HELPINDEX, b"Help ~index"),
            Menu::Item(IDM_GENERALHELP, b"~General help"),
            Menu::Item(IDM_USINGHELP, b"~Using help"),
            Menu::Item(IDM_KEYSHELP, b"~Keys help"),
            Menu::Sep,
            Menu::Item(IDM_PRODUCT, b"~Product information"),
        ]),
    ]
}

/// OS/2's accelerator table (Ctrl and Alt letters in both cases).
fn accels() -> Vec<(u16, u16, u16)> {
    let ctrl = AF_CHAR | AF_CONTROL;
    let alt = AF_CHAR | AF_ALT;
    let chars: &[(u16, u8, u16)] = &[
        (ctrl, b'n', IDM_NEW),
        (ctrl, b'o', IDM_OPEN),
        (ctrl, b's', IDM_SAVE),
        (ctrl, b'e', IDM_NEXT),
        (ctrl, b'u', IDM_UNDO),
        (ctrl, b'a', IDM_SELECT),
        (alt, b'a', IDM_SELECTALL),
        (alt, b'i', IDM_STRETCHPASTE),
        (ctrl, b'h', IDM_FLIPH),
        (ctrl, b'v', IDM_FLIPV),
        (ctrl, b'c', IDM_CIRCLE),
        (alt, b'n', IDM_PALDEFAULT),
        (alt, b'm', IDM_PALOPEN),
        (alt, b's', IDM_PALSAVE),
        (alt, b'u', IDM_SWAPKEEP),
        (alt, b'v', IDM_SWAPCHANGE),
        (alt, b'c', IDM_EDITCOLOR),
        (alt, b'g', IDM_SETDEFPAL),
        (ctrl, b'l', IDM_DEVLIST),
        (ctrl, b'w', IDM_DEVEDIT),
        (ctrl, b't', IDM_TEST),
        (ctrl, b'g', IDM_GRID),
        (ctrl, b'x', IDM_XBG),
        (ctrl, b'-', IDM_STRAIGHT),
        (alt, b'x', IDM_SAFE),
        (alt, b'w', IDM_NOWARN),
        (alt, b'y', IDM_SAVESTATE),
        (alt, b'z', IDM_STATUS),
        (alt, b'r', IDM_RESET),
        (alt, b'b', IDM_SAVEPROFILE),
        (alt, b'j', IDM_PENSHAPE),
        (alt, b'k', IDM_HOTSPOT),
        (ctrl, b'f', IDM_COLORFILL),
        (alt, b'q', IDM_FINDCOLOR),
    ];
    let mut v = Vec::new();
    for &(fs, c, cmd) in chars {
        v.push((fs, c as u16, cmd));
        if c.is_ascii_lowercase() {
            v.push((fs, c.to_ascii_uppercase() as u16, cmd));
        }
    }
    for i in 0..9u16 {
        v.push((ctrl, b'1' as u16 + i, IDM_PEN1 + i));
    }
    v.extend_from_slice(&[
        (AF_VIRTUALKEY, VK_ESC, IDM_ESCAPE),
        (AF_VIRTUALKEY | AF_ALT, VK_BACKSPACE, IDM_UNDO),
        (AF_VIRTUALKEY | AF_SHIFT, VK_INSERT, IDM_PASTE),
        (AF_VIRTUALKEY | AF_CONTROL, VK_INSERT, IDM_COPY),
        (AF_VIRTUALKEY | AF_SHIFT, VK_DELETE, IDM_CUT),
        (AF_VIRTUALKEY, VK_DELETE, IDM_CLEAR),
        (AF_VIRTUALKEY | AF_CONTROL, VK_DELETE, IDM_FILL),
    ]);
    v
}

/// Menu items' checks and what is possible now.
pub fn update_menus() {
    let a = app();
    let m = a.menu;
    let o = a.opts;
    menu_check(m, IDM_GRID, o.grid != 0);
    menu_check(m, IDM_XBG, o.xbg != 0);
    menu_check(m, IDM_STRAIGHT, o.straight != 0);
    menu_check(m, IDM_TEST, a.test);
    menu_check(m, IDM_SAFE, o.safe != 0);
    menu_check(m, IDM_NOWARN, o.nowarn != 0);
    menu_check(m, IDM_SAVESTATE, o.save_state != 0);
    menu_check(m, IDM_STATUS, o.status != 0);
    menu_check(m, IDM_HOTSPOT, a.mode == Mode::Hotspot);
    menu_check(m, IDM_PENSHAPE, a.mode == Mode::PenShape);
    menu_check(m, IDM_SELECT, a.mode == Mode::Select);
    menu_check(m, IDM_COLORFILL, a.mode == Mode::ColorFill);
    menu_check(m, IDM_FINDCOLOR, a.mode == Mode::FindColor);
    for i in 0..9u32 {
        menu_check(m, IDM_PEN1 + i as u16, o.pen_w == o.pen_h && o.pen_w == i + 1);
    }
    let icon = a.fig.kind != Kind::Bitmap;
    menu_enable(m, IDM_HOTSPOT, icon);
    menu_enable(m, IDM_TEST, icon);
    menu_enable(m, IDM_UNDO, a.undo.is_some());
    menu_enable(m, IDM_SAVE, true);
}

// ---- Help ----

fn create_help() {
    let a = app();
    let frame_items: &[(u16, u16)] = &[
        (IDM_FILE, 20001), (IDM_NEW, 20007), (IDM_OPEN, 20008), (IDM_NEXT, 20009), (IDM_SAVE, 20010), (IDM_SAVEAS, 20011),
        (IDM_EDIT, 20002), (IDM_UNDO, 20013), (IDM_SELECT, 20014), (IDM_SELECTALL, 20015), (IDM_CUT, 20016), (IDM_COPY, 20017),
        (IDM_PASTE, 20018), (IDM_CLEAR, 20019), (IDM_STRETCHPASTE, 20020), (IDM_FILL, 20021), (IDM_FLIPH, 20022), (IDM_FLIPV, 20023),
        (IDM_CIRCLE, 20024), (IDM_PENSHAPE, 20025), (IDM_HOTSPOT, 20026), (IDM_PALETTE, 20003), (IDM_PALDEFAULT, 20027),
        (IDM_PALOPEN, 20028), (IDM_PALSAVE, 20029), (IDM_PALSAVEAS, 20078), (IDM_EDITCOLOR, 20031), (IDM_SWAP, 20033),
        (IDM_SWAPKEEP, 20032), (IDM_SWAPCHANGE, 20042), (IDM_SETDEFPAL, 20034), (IDM_OPTIONS, 20004), (IDM_TEST, 20035),
        (IDM_GRID, 20036), (IDM_XBG, 20037), (IDM_STRAIGHT, 20038), (IDM_PENSIZE, 20039), (IDM_PEN1, 20085), (IDM_PEN1 + 1, 20085),
        (IDM_PEN1 + 2, 20085), (IDM_PEN1 + 3, 20085), (IDM_PEN1 + 4, 20085), (IDM_PEN1 + 5, 20085), (IDM_PEN1 + 6, 20085),
        (IDM_PEN1 + 7, 20085), (IDM_PEN1 + 8, 20085), (IDM_PREFS, 20040), (IDM_SAFE, 20047), (IDM_NOWARN, 20048),
        (IDM_SAVESTATE, 20049), (IDM_STATUS, 20090), (IDM_RESET, 20050), (IDM_SAVEPROFILE, 20040), (IDM_DEVICE, 20005),
        (IDM_DEVLIST, 20041), (IDM_DEVEDIT, 20044), (IDM_TOOLS, 20006), (IDM_COLORFILL, 20045), (IDM_FINDCOLOR, 20046),
        (IDM_HELP, 64022), (IDM_HELPINDEX, 64026), (IDM_GENERALHELP, 64024), (IDM_USINGHELP, 64023), (IDM_KEYSHELP, 64025),
        (IDM_PRODUCT, 64096),
    ];
    let help = dialogs::ID_HELP;
    a.help = Help::create(
        a.hab,
        a.frame,
        HELP_FILE,
        b"Icon Editor Help",
        &[
            HelpWindow { id: FRAME_ID as u16, ext: PANEL_GENERAL, items: frame_items },
            HelpWindow { id: DLG_NEW, ext: 20058, items: &[(help, 20058), (103, 20058), (104, 20058), (105, 20058), (106, 20058), (107, 20058)] },
            HelpWindow { id: DLG_CUSTOM, ext: 20108, items: &[(help, 20108)] },
            HelpWindow { id: DLG_DEVLIST, ext: 20098, items: &[(help, 20098), (303, 20098), (304, 20098), (305, 20098)] },
            HelpWindow { id: DLG_COLOR, ext: 20084, items: &[(help, 20084)] },
            HelpWindow { id: DLG_PREDEF, ext: 20119, items: &[(help, 20119)] },
            HelpWindow { id: DLG_ADDDEV, ext: 20101, items: &[(help, 20101), (701, 20101), (703, 20101)] },
            HelpWindow { id: DLG_SAVEOPTS, ext: 20122, items: &[(help, 20122)] },
            HelpWindow { id: DLG_PRODUCT, ext: 64096, items: &[(1, 64096)] },
        ],
    );
    if a.help.is_none() {
        trace(b"no help file");
    }
}

pub fn help_request(msg: u32, mp1: u32, mp2: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, mp1, mp2);
}

// ---- Layout ----

pub const STATUS_H: i32 = 74;
pub const PANEL_W: i32 = 148;

pub struct Layout {
    pub status: RectL,
    pub edit: RectL,
    pub panel: RectL,
    /// The grid's bottom left and the size of a pel.
    pub gx: i32,
    pub gy: i32,
    pub z: i32,
}

pub fn layout() -> Layout {
    let a = app();
    let r = window_rect(a.client);
    let sh = if a.opts.status != 0 { STATUS_H } else { 0 };
    let status = RectL::new(r.l, r.t - sh, r.r, r.t);
    let panel = RectL::new(r.r - PANEL_W, r.b, r.r, r.t - sh);
    let edit = RectL::new(r.l, r.b, r.r - PANEL_W, r.t - sh);
    let f = &a.fig.forms[a.cur];
    let z = ((edit.width() - 16) / f.cx as i32).min((edit.height() - 16) / f.cy as i32).max(1);
    let gx = edit.l + (edit.width() - z * f.cx as i32) / 2;
    let gy = edit.b + (edit.height() - z * f.cy as i32) / 2;
    Layout { status, edit, panel, gx, gy, z }
}

/// The pel under a window point (may be outside the form).
pub fn pel_at(l: &Layout, x: i32, y: i32) -> (i32, i32) {
    let f = form();
    let px = (x - l.gx).div_euclid(l.z);
    let py = f.cy as i32 - 1 - (y - l.gy).div_euclid(l.z);
    (px, py)
}

/// The palette's buttons: what each picks and where it is.
pub fn palette_cells(l: &Layout) -> Vec<(Pick, RectL)> {
    let a = app();
    let mut v = Vec::new();
    let (x0, mut top) = (l.panel.l + 10, l.panel.t - 10);
    let w = l.panel.r - 10 - x0;
    if a.fig.kind != Kind::Bitmap {
        v.push((Pick::Screen, RectL::new(x0, top - 28, x0 + w, top)));
        v.push((Pick::Inverse, RectL::new(x0, top - 60, x0 + w, top - 32)));
        top -= 66;
    }
    let n = palette().len();
    if n <= 16 {
        let cw = (w - 4) / 2;
        let ch = ((top - l.panel.b - 6) / 8 - 4).clamp(8, 28);
        // Two columns: the dark colours on the left, their bright partners
        // (entry + 8) on the right, as OS/2's editor shows them.
        let half = n.div_ceil(2);
        for i in 0..n {
            let (col, row) = ((i / half) as i32, (i % half) as i32);
            let x = x0 + col * (cw + 4);
            let t = top - row * (ch + 4);
            v.push((Pick::Pal(i), RectL::new(x, t - ch, x + cw, t)));
        }
    } else {
        let c = (w / 16).min((top - l.panel.b - 6) / 16).max(3);
        for i in 0..n {
            let (col, row) = ((i % 16) as i32, (i / 16) as i32);
            let x = x0 + col * c;
            let t = top - row * c;
            v.push((Pick::Pal(i), RectL::new(x, t - c + 1, x + c - 1, t)));
        }
    }
    v
}

// ---- Painting ----

/// The form as colours to show: Screen and Inverse pels in their
/// display colours.
fn display_rgb(f: &Form) -> Vec<u32> {
    f.pels.iter().map(|&p| match p {
        SCREEN => SCREEN_RGB,
        INVERSE => INVERSE_RGB,
        c => c & 0xffffff,
    }).collect()
}

fn paint(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, 0, &mut upd) };
    gpi::rgb_mode(hps);
    let l = layout();
    let r = window_rect(hwnd);
    let gray = 0xcccccc;
    gpi::fill(hps, r, gray);
    let f = &a.fig.forms[a.cur];
    let rgb = display_rgb(f);
    let packed = gpi::pack_rgb(&rgb, f.cx as usize, f.cy as usize);
    // The status area.
    if a.opts.status != 0 {
        paint_status(hps, &l, &packed);
        gpi::color(hps, 0x000000);
        gpi::line(hps, r.l, l.status.b, r.r, l.status.b);
    }
    // The edit area.
    let (cw, ch) = (f.cx as i32 * l.z, f.cy as i32 * l.z);
    gpi::draw_rgb(hps, &packed, f.cx as i32, f.cy as i32, l.gx, l.gy, cw, ch);
    if a.opts.xbg != 0 && l.z >= 5 {
        gpi::color(hps, 0x40a040);
        for y in 0..f.cy as i32 {
            for x in 0..f.cx as i32 {
                if f.get(x, y) == SCREEN {
                    let (x0, y0) = (l.gx + x * l.z, l.gy + (f.cy as i32 - 1 - y) * l.z);
                    gpi::line(hps, x0 + 1, y0 + 1, x0 + l.z - 2, y0 + l.z - 2);
                    gpi::line(hps, x0 + 1, y0 + l.z - 2, x0 + l.z - 2, y0 + 1);
                }
            }
        }
    }
    if a.opts.grid != 0 && l.z >= 4 {
        gpi::color(hps, 0x808080);
        for i in 0..=f.cx as i32 {
            gpi::line(hps, l.gx + i * l.z, l.gy, l.gx + i * l.z, l.gy + ch);
        }
        for i in 0..=f.cy as i32 {
            gpi::line(hps, l.gx, l.gy + i * l.z, l.gx + cw, l.gy + i * l.z);
        }
    }
    gpi::frame(hps, RectL::new(l.gx - 1, l.gy - 1, l.gx + cw + 1, l.gy + ch + 1), 0x000000);
    // The hot spot of icons and pointers.
    if a.fig.kind == Kind::Pointer || a.mode == Mode::Hotspot {
        let (hx, hy) = (l.gx + f.hot.0 * l.z + l.z / 2, l.gy + f.hot.1 * l.z + l.z / 2);
        let s = (l.z / 2 + 2).max(4);
        gpi::color(hps, 0xff0000);
        gpi::line(hps, hx - s, hy, hx + s, hy);
        gpi::line(hps, hx, hy - s, hx, hy + s);
    }
    // The selection.
    if let Some(s) = a.sel {
        let rr = RectL::new(l.gx + s.x0 * l.z, l.gy + (f.cy as i32 - 1 - s.y1) * l.z, l.gx + (s.x1 + 1) * l.z + 1, l.gy + (f.cy as i32 - s.y0) * l.z + 1);
        unsafe { GpiSetLineType(hps, LINETYPE_ALTERNATE) };
        gpi::frame(hps, rr, 0x000000);
        unsafe { GpiSetLineType(hps, LINETYPE_SOLID) };
    }
    // The keyboard's pen location.
    if let Some((px, py)) = a.pen_at {
        if a.drag.is_none() && px >= 0 && py >= 0 && px < f.cx as i32 && py < f.cy as i32 && l.z >= 4 {
            let x0 = l.gx + px * l.z;
            let y0 = l.gy + (f.cy as i32 - 1 - py) * l.z;
            gpi::frame(hps, RectL::new(x0, y0, x0 + l.z + 1, y0 + l.z + 1), 0xff0000);
        }
    }
    // The palette.
    gpi::color(hps, 0x000000);
    gpi::line(hps, l.panel.l, l.panel.b, l.panel.l, l.panel.t);
    for (p, rr) in palette_cells(&l) {
        let c = match p {
            Pick::Screen => SCREEN_RGB,
            Pick::Inverse => INVERSE_RGB,
            Pick::Pal(i) => palette()[i],
        };
        gpi::fill(hps, rr, c as i32);
        gpi::frame(hps, rr, 0x606060);
        match p {
            Pick::Screen => gpi::draw_text(hps, RectL::new(rr.l + 3, rr.b, rr.r, rr.t), b"Screen", 0x000000, 0, DT_LEFT | DT_VCENTER),
            Pick::Inverse => gpi::draw_text(hps, RectL::new(rr.l + 3, rr.b, rr.r, rr.t), b"Inverse", 0x000000, 0, DT_LEFT | DT_VCENTER),
            _ => {}
        }
        // Button 1's colour framed in red, button 2's in green.
        if p == a.left {
            gpi::frame(hps, RectL::new(rr.l - 2, rr.b - 2, rr.r + 2, rr.t + 2), 0xff0000);
            gpi::frame(hps, RectL::new(rr.l - 1, rr.b - 1, rr.r + 1, rr.t + 1), 0xff0000);
        }
        if p == a.right {
            gpi::frame(hps, RectL::new(rr.l + 2, rr.b + 2, rr.r - 2, rr.t - 2), 0x00c000);
            gpi::frame(hps, RectL::new(rr.l + 3, rr.b + 3, rr.r - 3, rr.t - 3), 0x00c000);
        }
    }
    unsafe { WinEndPaint(hps) };
}

fn paint_status(hps: u32, l: &Layout, packed: &[u8]) {
    let a = app();
    let s = l.status;
    let f = &a.fig.forms[a.cur];
    // A mouse whose buttons show their colours.
    let (mx, my) = (s.l + 12, s.t - 62);
    gpi::round_box(hps, RectL::new(mx, my, mx + 30, my + 44), 8, 0xffffe0, 0x000000);
    let lc = pick_rgb(a.left);
    let rc = pick_rgb(a.right);
    gpi::fill(hps, RectL::new(mx + 4, my + 26, mx + 14, my + 40), lc as i32);
    gpi::frame(hps, RectL::new(mx + 4, my + 26, mx + 14, my + 40), 0x000000);
    gpi::fill(hps, RectL::new(mx + 16, my + 26, mx + 26, my + 40), rc as i32);
    gpi::frame(hps, RectL::new(mx + 16, my + 26, mx + 26, my + 40), 0x000000);
    gpi::line(hps, mx + 15, my + 44, mx + 15, my + 52);
    // The figure at its real size (smaller if it is large).
    let box_ = 64;
    let (bw, bh) = (f.cx.min(box_) as i32, f.cy.min(box_) as i32);
    let (w, h) = if f.cx as i32 > bw || f.cy as i32 > bh {
        let k = (f.cx.max(f.cy)) as i32;
        ((f.cx as i32 * box_ as i32 / k).max(1), (f.cy as i32 * box_ as i32 / k).max(1))
    } else {
        (bw, bh)
    };
    let (px, py) = (s.l + 60, s.b + (s.height() - h) / 2);
    gpi::frame(hps, RectL::new(px - 1, py - 1, px + w + 1, py + h + 1), 0x000000);
    gpi::draw_rgb(hps, packed, f.cx as i32, f.cy as i32, px, py, w, h);
    // The figures.
    let line_h = 16;
    let lx = s.l + 150;
    let vx = lx + 86;
    let row = |i: i32| RectL::new(0, s.t - 6 - (i + 1) * line_h, 0, s.t - 6 - i * line_h);
    let label = |i: i32, t: &[u8]| {
        let r = row(i);
        gpi::draw_text(hps, RectL::new(lx, r.b, vx - 4, r.t), t, 0x000000, 0, DT_RIGHT | DT_VCENTER);
    };
    let value = |i: i32, x: i32, t: &[u8]| {
        let r = row(i);
        gpi::draw_text(hps, RectL::new(x, r.b, s.r - 4, r.t), t, 0x000000, 0, DT_LEFT | DT_VCENTER);
    };
    label(0, b"Form size :");
    value(0, vx, alloc::format!("{}x{}", f.cx, f.cy).as_bytes());
    label(1, b"Pen location :");
    if let Some((x, y)) = a.pen_at {
        if x >= 0 && y >= 0 && x < f.cx as i32 && y < f.cy as i32 {
            value(1, vx, alloc::format!("{},{}", x, f.cy as i32 - 1 - y).as_bytes());
        }
    }
    label(2, b"Pen size :");
    value(2, vx, alloc::format!("{}x{}", a.opts.pen_w, a.opts.pen_h).as_bytes());
    if let Some(sel) = a.sel {
        label(3, b"Selection size :");
        value(3, vx, alloc::format!("{}x{}", sel.w(), sel.h()).as_bytes());
    } else if a.fig.kind != Kind::Bitmap {
        label(3, b"Hotspot :");
        value(3, vx, alloc::format!("{}x{}", f.hot.0, f.hot.1).as_bytes());
    }
    let cx = vx + 86;
    value(0, cx, &[&b"Figure type : "[..], kind_name(a.fig.kind)].concat());
    value(1, cx, &[&b"Form "[..], alloc::format!("{} of {} : ", a.cur + 1, a.fig.forms.len()).as_bytes(), &format::describe(f)].concat());
    value(2, cx, &form_device_name(a.cur));
    value(3, cx, mode_text());
}

fn mode_text() -> &'static [u8] {
    let a = app();
    match a.mode {
        Mode::Draw if a.sel.is_some() => b"Select mode - selected.",
        Mode::Draw => b"",
        Mode::Select if a.drag.is_some() => b"Select mode - selecting...",
        Mode::Select => b"Select mode - make selection.",
        Mode::Hotspot => b"Hotspot mode.",
        Mode::ColorFill => b"Click on spot to flood with the mouse button color.",
        Mode::FindColor => b"Click on a spot to locate its color on the palette.",
        Mode::PenShape => b"Arrow keys change pen shape.",
    }
}

fn pick_rgb(p: Pick) -> u32 {
    match p {
        Pick::Screen => SCREEN_RGB,
        Pick::Inverse => INVERSE_RGB,
        Pick::Pal(i) => *palette().get(i).unwrap_or(&0),
    }
}

// ---- Mouse and keys ----

fn button_down(button: u8, x: i32, y: i32) {
    let a = app();
    let l = layout();
    // The palette.
    for (p, r) in palette_cells(&l) {
        if r.contains(x, y) || RectL::new(r.l - 1, r.b - 1, r.r + 1, r.t + 1).contains(x, y) {
            if button == 1 {
                a.left = p;
            } else {
                a.right = p;
            }
            invalidate(a.client);
            return;
        }
    }
    let (px, py) = pel_at(&l, x, y);
    let f = form();
    if px < 0 || py < 0 || px >= f.cx as i32 || py >= f.cy as i32 {
        return;
    }
    a.pen_at = Some((px, py));
    let value = pick_value(if button == 1 { a.left } else { a.right });
    match a.mode {
        Mode::Hotspot => {
            checkpoint();
            form().hot = (px, f.cy as i32 - 1 - py);
            refresh_test_pointer();
        }
        Mode::ColorFill => {
            checkpoint();
            let n = edit::flood(form(), px, py, value);
            trace(alloc::format!("flood {n} pels").as_bytes());
            refresh_test_pointer();
        }
        Mode::FindColor => {
            let v = f.get(px, py);
            let p = match v {
                SCREEN => Pick::Screen,
                INVERSE => Pick::Inverse,
                c => match palette().iter().position(|&q| q == c) {
                    Some(i) => Pick::Pal(i),
                    None => {
                        message(a.frame, b"This color is not in the palette.", TITLE, MB_OK | MB_INFORMATION);
                        return;
                    }
                },
            };
            if button == 1 {
                a.left = p;
            } else {
                a.right = p;
            }
            a.mode = Mode::Draw;
            update_menus();
        }
        Mode::Select => {
            a.sel = Some(Area::span((px, py), (px, py)));
            a.drag = Some(Drag { button, start: (px, py), last: (px, py), axis: None });
            unsafe { WinSetCapture(HWND_DESKTOP, a.client) };
        }
        Mode::Draw | Mode::PenShape => {
            checkpoint();
            a.sel = None;
            edit::pen(form(), px, py, a.opts.pen_w as i32, a.opts.pen_h as i32, value);
            a.drag = Some(Drag { button, start: (px, py), last: (px, py), axis: None });
            unsafe { WinSetCapture(HWND_DESKTOP, a.client) };
        }
    }
    invalidate(a.client);
}

fn mouse_move(x: i32, y: i32) {
    let a = app();
    let l = layout();
    let (mut px, mut py) = pel_at(&l, x, y);
    let f = form();
    let inside = px >= 0 && py >= 0 && px < f.cx as i32 && py < f.cy as i32;
    let Some(d) = a.drag.as_mut() else {
        let at = inside.then_some((px, py));
        if at != a.pen_at && a.opts.status != 0 {
            a.pen_at = at;
            unsafe { WinInvalidateRect(a.client, &l.status, 0) };
        }
        return;
    };
    px = px.clamp(0, f.cx as i32 - 1);
    py = py.clamp(0, f.cy as i32 - 1);
    if (px, py) == d.last {
        return;
    }
    if a.mode == Mode::Select {
        d.last = (px, py);
        a.sel = Some(Area::span(d.start, (px, py)));
    } else {
        // Draw straight: the first move decides the direction.
        if a.opts.straight != 0 {
            let axis = *d.axis.get_or_insert((px - d.start.0).abs() >= (py - d.start.1).abs());
            if axis {
                py = d.start.1;
            } else {
                px = d.start.0;
            }
        }
        let value = pick_value(if d.button == 1 { a.left } else { a.right });
        edit::line(form(), d.last, (px, py), a.opts.pen_w as i32, a.opts.pen_h as i32, value);
        d.last = (px, py);
    }
    a.pen_at = Some((px, py));
    invalidate(a.client);
}

fn button_up() {
    let a = app();
    if a.drag.take().is_some() {
        unsafe { WinSetCapture(HWND_DESKTOP, 0) };
        if a.mode == Mode::Select {
            a.mode = Mode::Draw;
        }
        refresh_test_pointer();
        update_menus();
        invalidate(a.client);
    }
}

/// Keys the accelerators leave: the arrows move the pen location (or
/// size the pen), the space bar paints with button 1's colour, Enter
/// with button 2's.
fn key(vk: u16) -> bool {
    let a = app();
    let f = form();
    if a.mode == Mode::PenShape {
        let o = &mut a.opts;
        match vk {
            VK_LEFT => o.pen_w = (o.pen_w - 1).max(1),
            VK_RIGHT => o.pen_w = (o.pen_w + 1).min(9),
            VK_DOWN => o.pen_h = (o.pen_h - 1).max(1),
            VK_UP => o.pen_h = (o.pen_h + 1).min(9),
            VK_ENTER | VK_NEWLINE => a.mode = Mode::Draw,
            _ => return false,
        }
        update_menus();
        invalidate(a.client);
        return true;
    }
    let (mut x, mut y) = a.pen_at.unwrap_or((f.cx as i32 / 2, f.cy as i32 / 2));
    match vk {
        VK_LEFT => x -= 1,
        VK_RIGHT => x += 1,
        VK_UP => y -= 1,
        VK_DOWN => y += 1,
        VK_SPACE | VK_ENTER | VK_NEWLINE => {
            checkpoint();
            let p = if vk == VK_SPACE { a.left } else { a.right };
            edit::pen(form(), x, y, a.opts.pen_w as i32, a.opts.pen_h as i32, pick_value(p));
            refresh_test_pointer();
        }
        _ => return false,
    }
    a.pen_at = Some((x.clamp(0, f.cx as i32 - 1), y.clamp(0, f.cy as i32 - 1)));
    invalidate(a.client);
    true
}

// ---- Test mode: the figure as the mouse pointer ----

pub fn refresh_test_pointer() {
    let a = app();
    if a.test_ptr != 0 {
        unsafe { WinDestroyPointer(a.test_ptr) };
        a.test_ptr = 0;
    }
    if !a.test || a.fig.kind == Kind::Bitmap {
        return;
    }
    a.test_ptr = make_pointer(form(), a.fig.kind == Kind::Pointer);
}

/// A PM pointer (or icon) from a form.
pub fn make_pointer(f: &Form, pointer: bool) -> u32 {
    let a = app();
    let (mask, color) = format::pointer_bits(f);
    let Some(m) = gpi::MemPs::new(a.hab, f.cx as i32, f.cy as i32) else { return 0 };
    // BITMAPINFO2 with 16 bytes of header, then the colour table.
    let mut mbmi = Vec::new();
    mbmi.extend_from_slice(&16u32.to_le_bytes());
    mbmi.extend_from_slice(&f.cx.to_le_bytes());
    mbmi.extend_from_slice(&(f.cy * 2).to_le_bytes());
    mbmi.extend_from_slice(&1u16.to_le_bytes());
    mbmi.extend_from_slice(&1u16.to_le_bytes());
    mbmi.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 0]);
    let mut mh = BitmapInfoHeader2::new(f.cx, f.cy * 2, 1);
    mh.cb = 16;
    let hbm_mask = unsafe { GpiCreateBitmap(m.hps, &mh, CBM_INIT, mask.as_ptr(), mbmi.as_ptr()) };
    let mut cbmi = Vec::new();
    cbmi.extend_from_slice(&16u32.to_le_bytes());
    cbmi.extend_from_slice(&f.cx.to_le_bytes());
    cbmi.extend_from_slice(&f.cy.to_le_bytes());
    cbmi.extend_from_slice(&1u16.to_le_bytes());
    cbmi.extend_from_slice(&24u16.to_le_bytes());
    let mut ch = BitmapInfoHeader2::new(f.cx, f.cy, 24);
    ch.cb = 16;
    let hbm_color = unsafe { GpiCreateBitmap(m.hps, &ch, CBM_INIT, color.as_ptr(), cbmi.as_ptr()) };
    let info = PointerInfo { pointer: pointer as u32, x_hotspot: f.hot.0, y_hotspot: f.hot.1, hbm_pointer: hbm_mask, hbm_color, hbm_mini_pointer: 0, hbm_mini_color: 0 };
    let h = unsafe { WinCreatePointerIndirect(HWND_DESKTOP, &info) };
    unsafe {
        GpiDeleteBitmap(hbm_mask);
        GpiDeleteBitmap(hbm_color);
    }
    h
}

// ---- Commands ----

fn selection_or_all() -> Area {
    app().sel.unwrap_or_else(|| Area::all(form()))
}

fn command(id: u16) {
    let a = app();
    match id {
        IDM_NEW => dialogs::new_dialog(),
        IDM_OPEN => dialogs::open(),
        IDM_SAVE => {
            dialogs::save(false);
        }
        IDM_SAVEAS => {
            dialogs::save(true);
        }
        IDM_NEXT => next_file(),
        IDM_UNDO => {
            if let Some((i, f)) = a.undo.take() {
                if i < a.fig.forms.len() {
                    let now = core::mem::replace(&mut a.fig.forms[i], f);
                    a.undo = Some((i, now));
                    a.cur = i;
                    a.changed = true;
                }
            }
        }
        IDM_SELECT => {
            a.mode = if a.mode == Mode::Select { Mode::Draw } else { Mode::Select };
            a.sel = None;
        }
        IDM_SELECTALL => {
            a.sel = Some(Area::all(form()));
            a.mode = Mode::Draw;
        }
        IDM_CUT | IDM_COPY => {
            dialogs::copy(selection_or_all());
            if id == IDM_CUT {
                checkpoint();
                edit::fill(form(), selection_or_all(), clear_value());
            }
        }
        IDM_PASTE | IDM_STRETCHPASTE => {
            if let Some((w, h, pels)) = dialogs::paste_data() {
                checkpoint();
                if id == IDM_STRETCHPASTE {
                    edit::stretch_paste(form(), selection_or_all(), w, h, &pels);
                } else {
                    let s = selection_or_all();
                    edit::paste(form(), s.x0, s.y0, w, h, &pels);
                    let f = form();
                    a.sel = Some(Area { x0: s.x0, y0: s.y0, x1: (s.x0 + w as i32 - 1).min(f.cx as i32 - 1), y1: (s.y0 + h as i32 - 1).min(f.cy as i32 - 1) });
                }
            }
        }
        IDM_CLEAR => {
            checkpoint();
            edit::fill(form(), selection_or_all(), clear_value());
        }
        IDM_FILL => {
            checkpoint();
            edit::fill(form(), selection_or_all(), pick_value(a.left));
        }
        IDM_FLIPH => {
            checkpoint();
            edit::flip_h(form(), selection_or_all());
        }
        IDM_FLIPV => {
            checkpoint();
            edit::flip_v(form(), selection_or_all());
        }
        IDM_CIRCLE => {
            checkpoint();
            edit::circle(form(), selection_or_all(), a.opts.pen_w as i32, a.opts.pen_h as i32, pick_value(a.left));
        }
        IDM_PALDEFAULT => {
            let f = form();
            if f.bpp <= 8 {
                checkpoint();
                let d = default_palette(f.bpp);
                for (i, &c) in d.iter().enumerate() {
                    edit::set_palette(form(), i, c);
                }
            } else {
                a.draw_pal = format::default_palette(4);
            }
        }
        IDM_PALOPEN => dialogs::palette_open(),
        IDM_PALSAVE => dialogs::palette_save(false),
        IDM_PALSAVEAS => dialogs::palette_save(true),
        IDM_EDITCOLOR => {
            if let Pick::Pal(i) = a.left {
                dialogs::edit_color(i);
            } else {
                message(a.frame, b"Choose a palette color with mouse button 1 first: Screen and Inverse have no color of their own.", TITLE, MB_OK | MB_INFORMATION);
            }
        }
        IDM_SWAPKEEP | IDM_SWAPCHANGE => {
            if let (Pick::Pal(l), Pick::Pal(r)) = (a.left, a.right) {
                if form().bpp <= 8 {
                    checkpoint();
                    edit::swap_colors(form(), l, r, id == IDM_SWAPKEEP);
                } else {
                    a.draw_pal.swap(l, r);
                }
                if id == IDM_SWAPKEEP {
                    a.left = Pick::Pal(r);
                    a.right = Pick::Pal(l);
                }
            }
        }
        IDM_SETDEFPAL => {
            let f = form();
            if f.bpp <= 8 {
                let key = alloc::format!("PALETTE{}", f.bpp);
                let bytes: Vec<u8> = f.palette.iter().flat_map(|c| c.to_le_bytes()).collect();
                ini::write(INI_APP, key.as_bytes(), &bytes);
                message(a.frame, b"The default palette is set.", TITLE, MB_OK | MB_INFORMATION);
            }
        }
        IDM_TEST => {
            a.test = !a.test;
            refresh_test_pointer();
        }
        IDM_GRID => a.opts.grid ^= 1,
        IDM_XBG => a.opts.xbg ^= 1,
        IDM_STRAIGHT => a.opts.straight ^= 1,
        IDM_SAFE => a.opts.safe ^= 1,
        IDM_NOWARN => a.opts.nowarn ^= 1,
        IDM_SAVESTATE => a.opts.save_state ^= 1,
        IDM_STATUS => a.opts.status ^= 1,
        IDM_RESET => {
            a.opts = DEFAULT_OPTS;
            a.mode = Mode::Draw;
            a.sel = None;
            a.test = false;
            refresh_test_pointer();
        }
        IDM_SAVEPROFILE => {
            save_options();
            message(a.frame, b"The options are saved.", TITLE, MB_OK | MB_INFORMATION);
        }
        IDM_PENSHAPE => a.mode = if a.mode == Mode::PenShape { Mode::Draw } else { Mode::PenShape },
        IDM_HOTSPOT => a.mode = if a.mode == Mode::Hotspot { Mode::Draw } else { Mode::Hotspot },
        IDM_COLORFILL => a.mode = if a.mode == Mode::ColorFill { Mode::Draw } else { Mode::ColorFill },
        IDM_FINDCOLOR => a.mode = if a.mode == Mode::FindColor { Mode::Draw } else { Mode::FindColor },
        IDM_ESCAPE => {
            a.mode = Mode::Draw;
            a.sel = None;
        }
        IDM_DEVLIST => dialogs::device_list(),
        IDM_DEVEDIT => dialogs::edit_predefined(),
        IDM_HELPINDEX => help_request(HM_HELP_INDEX, 0, 0),
        IDM_GENERALHELP => help_request(HM_EXT_HELP, 0, 0),
        IDM_USINGHELP => help_request(HM_DISPLAY_HELP, 0, 0),
        IDM_KEYSHELP => help_request(HM_KEYS_HELP, 0, 0),
        IDM_PRODUCT => product_info(a.frame, DLG_PRODUCT, b"Product information", b"Icon Editor", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Draws icons, pointers and bitmaps in OS/2's formats."]),
        _ if (IDM_PEN1..IDM_PEN1 + 9).contains(&id) => {
            a.opts.pen_w = (id - IDM_PEN1 + 1) as u32;
            a.opts.pen_h = a.opts.pen_w;
        }
        _ => return,
    }
    if matches!(id, IDM_UNDO | IDM_CUT | IDM_PASTE | IDM_STRETCHPASTE | IDM_CLEAR | IDM_FILL | IDM_FLIPH | IDM_FLIPV | IDM_CIRCLE | IDM_PALDEFAULT | IDM_SWAPKEEP | IDM_SWAPCHANGE) {
        refresh_test_pointer();
    }
    fix_picks();
    update_menus();
    invalidate(a.client);
}

/// What Clear and Cut leave: the screen for icons and pointers, white
/// (or the palette's last colour) for bitmaps.
fn clear_value() -> u32 {
    let a = app();
    if a.fig.kind != Kind::Bitmap {
        return SCREEN;
    }
    let p = palette();
    if p.contains(&0xffffff) || form().bpp > 8 { 0xffffff } else { *p.last().unwrap_or(&0) }
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON2DOWN => {
            unsafe { WinSetFocus(HWND_DESKTOP, hwnd) };
            let (x, y) = mouse_xy(mp1);
            button_down(if msg == WM_BUTTON1DOWN { 1 } else { 2 }, x, y);
            1
        }
        WM_BUTTON1UP | WM_BUTTON2UP => {
            button_up();
            1
        }
        WM_BUTTON1DBLCLK | WM_BUTTON2DBLCLK => {
            // A double click on a palette colour edits it.
            let (x, y) = mouse_xy(mp1);
            let l = layout();
            for (p, r) in palette_cells(&l) {
                if let Pick::Pal(i) = p {
                    if r.contains(x, y) {
                        dialogs::edit_color(i);
                        return 1;
                    }
                }
            }
            button_down(if msg == WM_BUTTON1DBLCLK { 1 } else { 2 }, x, y);
            1
        }
        WM_MOUSEMOVE => {
            let (x, y) = mouse_xy(mp1);
            mouse_move(x, y);
            let a = app();
            let l = layout();
            if a.test && a.test_ptr != 0 && l.edit.contains(x, y) {
                unsafe { WinSetPointer(HWND_DESKTOP, a.test_ptr) };
                return 1;
            }
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        WM_CHAR => {
            let fl = lo(mp1);
            if fl & KC_KEYUP == 0 && fl & KC_VIRTUALKEY != 0 && fl & (KC_CTRL | KC_ALT) == 0 && key(hi(mp2) as u16) {
                return 1;
            }
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        WM_COMMAND => {
            command(lo(mp1) as u16);
            0
        }
        WM_CLOSE => {
            unsafe { WinPostMsg(hwnd, WM_QUIT, 0, 0) };
            0
        }
        WM_HELP => 0,
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        HM_QUERY_KEYS_HELP => PANEL_KEYS as u32,
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}
