//! E.EXE: rs2's System Editor, a re-implementation of OS/2's E.
//!
//! A standard frame (title bar with the file's full name or "Untitled",
//! system menu, sizing border, menu bar) whose client holds a WC_MLE.
//! The menus are those of OS/2's System Editor: File (New, Open, Save,
//! Save as, Autosave), Edit (Undo, Cut, Copy, Paste, Clear, Find,
//! Select all), Options (Set font, Set colors, Word wrap On/Off) and Help
//! (Help index, General help, Using help, Keys help, Product
//! information). Files open from the command line (`E [file]`) or with
//! the standard file dialog; Save asks for a file type (.TYPE EA) when
//! the file has none, as E does; closing the window, or
//! WM_SAVEAPPLICATION at shutdown, asks about unsaved changes. Options
//! are kept in OS2.INI ("SystemEditor"). The help instance uses E.HLP.
//!
//! `E /SELFTEST` runs self-checks of the file and editing logic (load,
//! edit, save, .TYPE, the close question) and prints PASS/FAIL lines and
//! "e selftest done: N failure(s)" on the serial log.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

mod dialogs;
mod file;
mod pm;
mod selftest;

use alloc::vec::Vec;

use pm::*;

os2::entry!(main);

// Menu commands. The help panels of the menu items have the same numbers.
pub const IDM_FILE: u16 = 100;
pub const IDM_NEW: u16 = 101;
pub const IDM_OPEN: u16 = 102;
pub const IDM_SAVE: u16 = 103;
pub const IDM_SAVEAS: u16 = 104;
pub const IDM_AUTOSAVE: u16 = 105;
pub const IDM_EDIT: u16 = 200;
pub const IDM_UNDO: u16 = 201;
pub const IDM_CUT: u16 = 202;
pub const IDM_COPY: u16 = 203;
pub const IDM_PASTE: u16 = 204;
pub const IDM_CLEAR: u16 = 205;
pub const IDM_FIND: u16 = 206;
pub const IDM_SELECTALL: u16 = 207;
pub const IDM_OPTIONS: u16 = 300;
pub const IDM_FONT: u16 = 301;
pub const IDM_COLORS: u16 = 302;
pub const IDM_WRAP: u16 = 303;
pub const IDM_WRAPON: u16 = 304;
pub const IDM_WRAPOFF: u16 = 305;
pub const IDM_HELP: u16 = 400;
pub const IDM_HELPINDEX: u16 = 401;
pub const IDM_GENERALHELP: u16 = 402;
pub const IDM_USINGHELP: u16 = 403;
pub const IDM_KEYSHELP: u16 = 404;
pub const IDM_PRODUCTINFO: u16 = 405;

/// Help panels (E.HLP) of the editor and its dialogs.
pub const PANEL_GENERAL: u16 = 1;
pub const PANEL_KEYS: u16 = 2;
pub const PANEL_CHANGED: u16 = 10;
pub const PANEL_NOTYPE: u16 = 11;
pub const PANEL_TYPE: u16 = 12;
pub const PANEL_AUTOSAVE: u16 = 13;
pub const PANEL_FIND: u16 = 14;
pub const PANEL_COLORS: u16 = 15;

const FRAME_ID: u32 = 1;
const MLE_ID: u32 = 10;
const CLASS: &[u8] = b"RS2SystemEditor\0";
const INI_APP: &[u8] = b"SystemEditor\0";
const INI_KEY: &[u8] = b"Settings\0";
pub const UNTITLED: &[u8] = b"Untitled";

/// The file types offered (Type dialog, Save as).
pub const TYPES: [&[u8]; 3] = [b"Plain Text", b"OS/2 Command File", b"DOS Command File"];

/// Default colours (CLR_*).
pub const CLR_WHITE: i32 = -2;
pub const CLR_BLACK: i32 = -1;

pub struct App {
    pub hab: u32,
    pub frame: u32,
    pub client: u32,
    pub mle: u32,
    pub menu: u32,
    pub help: u32,
    /// The file's full name (None: Untitled).
    pub path: Option<Vec<u8>>,
    pub wrap: bool,
    pub autosave: bool,
    pub autosave_every: u32,
    /// Changes since the last save (for autosave).
    pub changes: u32,
    pub fg: i32,
    pub bg: i32,
    pub fattrs: [u8; 56],
    pub points: u32,
    /// The modeless Find dialog.
    pub find: u32,
    /// The answer to the "file changed" question asked for
    /// WM_SAVEAPPLICATION, used by the WM_CLOSE that follows it.
    pub pending_close: Option<bool>,
    /// Loading a file: MLN_CHANGE does not count.
    pub loading: bool,
}

static mut APP: App = App {
    hab: 0,
    frame: 0,
    client: 0,
    mle: 0,
    menu: 0,
    help: 0,
    path: None,
    wrap: true,
    autosave: false,
    autosave_every: 100,
    changes: 0,
    fg: CLR_BLACK,
    bg: CLR_WHITE,
    fattrs: [0; 56],
    points: 12,
    find: 0,
    pending_close: None,
    loading: false,
};

pub fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

/// FATTRS of a bitmap font: System VIO 8 x 16 (12 points).
pub fn default_fattrs() -> [u8; 56] {
    let mut f = [0u8; 56];
    f[0..2].copy_from_slice(&56u16.to_le_bytes());
    f[8..18].copy_from_slice(b"System VIO");
    f[44..48].copy_from_slice(&16i32.to_le_bytes());
    f[48..52].copy_from_slice(&8i32.to_le_bytes());
    f
}

fn main(s: &os2::Startup) -> u32 {
    let args = s.args();
    let arg = args.iter().position(|&c| c != b' ').map_or(&b""[..], |i| &args[i..]);
    let arg = {
        let a = arg.strip_suffix(b" ").unwrap_or(arg);
        let end = a.iter().rposition(|&c| c != b' ').map_or(0, |i| i + 1);
        let a = &a[..end];
        a.strip_prefix(b"\"").and_then(|a| a.strip_suffix(b"\"")).unwrap_or(a)
    };
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    a.fattrs = default_fattrs();
    load_settings();
    if arg.eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    if !create_window() {
        return 1;
    }
    if !arg.is_empty() {
        open_file(arg);
    }
    unsafe {
        WinSetWindowPos(a.frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, a.mle);
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(a.hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(a.hab, &q) };
    }
    trace(b"closed");
    unsafe {
        if a.help != 0 {
            WinAssociateHelpInstance(0, a.frame);
            WinDestroyHelpInstance(a.help);
        }
        WinDestroyWindow(a.frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

/// The frame, its menu bar, accelerators, the MLE and the help instance.
pub fn create_window() -> bool {
    let a = app();
    unsafe { WinRegisterClass(a.hab, CLASS.as_ptr(), client_proc, 0x0004 /* CS_SIZEREDRAW */, 0) };
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_SHELLPOSITION | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let mut client = 0;
    let title = z(UNTITLED);
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), title.as_ptr(), WS_CLIPCHILDREN, 0, FRAME_ID, &mut client) };
    if frame == 0 {
        return false;
    }
    a.frame = frame;
    a.client = client;
    let tpl = menu_template(&menu_items());
    a.menu = unsafe { WinCreateMenu(frame, tpl.as_ptr()) };
    send(frame, WM_UPDATEFRAME, FCF_MENU, 0);
    // Ctrl+F: Find.
    let mut acc = Vec::new();
    acc.extend_from_slice(&2u16.to_le_bytes());
    acc.extend_from_slice(&850u16.to_le_bytes());
    for key in [b'f', b'F'] {
        acc.extend_from_slice(&(AF_CHAR | AF_CONTROL).to_le_bytes());
        acc.extend_from_slice(&(key as u16).to_le_bytes());
        acc.extend_from_slice(&IDM_FIND.to_le_bytes());
    }
    unsafe {
        let haccel = WinCreateAccelTable(a.hab, acc.as_ptr());
        WinSetAccelTable(a.hab, haccel, frame);
    }
    // The frame passes on what only it gets.
    let old = unsafe { WinSubclassWindow(frame, frame_proc) };
    unsafe { FRAME_PROC = old };
    create_help();
    true
}

fn menu_items() -> Vec<Menu<'static>> {
    let wrap = app().wrap;
    alloc::vec![
        Menu::Sub(IDM_FILE, b"~File", alloc::vec![
            Menu::Item(IDM_NEW, b"~New"),
            Menu::Item(IDM_OPEN, b"~Open..."),
            Menu::Sep,
            Menu::Item(IDM_SAVE, b"~Save"),
            Menu::Item(IDM_SAVEAS, b"Save ~as..."),
            Menu::Item(IDM_AUTOSAVE, b"A~utosave..."),
        ]),
        Menu::Sub(IDM_EDIT, b"~Edit", alloc::vec![
            Menu::Item(IDM_UNDO, b"~Undo\tAlt+Backspace"),
            Menu::Sep,
            Menu::Item(IDM_CUT, b"Cu~t\tShift+Delete"),
            Menu::Item(IDM_COPY, b"~Copy\tCtrl+Insert"),
            Menu::Item(IDM_PASTE, b"~Paste\tShift+Insert"),
            Menu::Sep,
            Menu::Item(IDM_CLEAR, b"Cl~ear\tDelete"),
            Menu::Sep,
            Menu::Item(IDM_FIND, b"~Find...\tCtrl+F"),
            Menu::Item(IDM_SELECTALL, b"Select ~all"),
        ]),
        Menu::Sub(IDM_OPTIONS, b"~Options", alloc::vec![
            Menu::Item(IDM_FONT, b"Set ~font..."),
            Menu::Item(IDM_COLORS, b"Set ~colors..."),
            Menu::Sep,
            Menu::Sub(IDM_WRAP, b"~Word wrap", alloc::vec![Menu::Check(IDM_WRAPON, b"~On", wrap), Menu::Check(IDM_WRAPOFF, b"O~ff", !wrap)]),
        ]),
        Menu::Sub(IDM_HELP, b"~Help", alloc::vec![
            Menu::Item(IDM_HELPINDEX, b"Help ~index"),
            Menu::Item(IDM_GENERALHELP, b"~General help"),
            Menu::Item(IDM_USINGHELP, b"~Using help"),
            Menu::Item(IDM_KEYSHELP, b"~Keys help"),
            Menu::Sep,
            Menu::Item(IDM_PRODUCTINFO, b"~Product information"),
        ]),
    ]
}

// ---- Help ----

/// The help table: the frame's subtable maps menu items and dialogs to
/// panels (kept for the life of the instance).
static mut HELP_TABLE: Vec<u8> = Vec::new();
static mut HELP_SUBTABLE: Vec<u16> = Vec::new();

fn create_help() {
    let a = app();
    let ids = [
        IDM_FILE, IDM_NEW, IDM_OPEN, IDM_SAVE, IDM_SAVEAS, IDM_AUTOSAVE, IDM_EDIT, IDM_UNDO, IDM_CUT, IDM_COPY, IDM_PASTE, IDM_CLEAR, IDM_FIND, IDM_SELECTALL, IDM_OPTIONS, IDM_FONT, IDM_COLORS, IDM_WRAP,
        IDM_WRAPON, IDM_WRAPOFF, IDM_HELP, IDM_HELPINDEX, IDM_GENERALHELP, IDM_USINGHELP, IDM_KEYSHELP, IDM_PRODUCTINFO,
    ];
    unsafe {
        let sub = &mut *core::ptr::addr_of_mut!(HELP_SUBTABLE);
        // HELPSUBTABLE: words per entry, then (item, panel) pairs, 0.
        sub.push(2);
        for id in ids {
            sub.push(id);
            sub.push(id);
        }
        sub.push(0);
        // HELPTABLE (packed): idAppWindow, phstHelpSubTable, idExtPanel;
        // a zero entry ends it.
        let t = &mut *core::ptr::addr_of_mut!(HELP_TABLE);
        t.extend_from_slice(&(FRAME_ID as u16).to_le_bytes());
        t.extend_from_slice(&(sub.as_ptr() as u32).to_le_bytes());
        t.extend_from_slice(&PANEL_GENERAL.to_le_bytes());
        t.extend_from_slice(&[0u8; 8]);
    }
    let mut init = HelpInit {
        cb: core::mem::size_of::<HelpInit>() as u32,
        ret: 0,
        tutorial: core::ptr::null(),
        table: unsafe { (*core::ptr::addr_of!(HELP_TABLE)).as_ptr() },
        table_module: 0,
        accel_module: 0,
        accel_id: 0,
        action_bar_id: 0,
        title: b"System Editor Help\0".as_ptr(),
        show_panel_id: 0,
        library: b"E.HLP\0".as_ptr(),
    };
    let h = unsafe { WinCreateHelpInstance(a.hab, &mut init) };
    if h != 0 && init.ret != 0 {
        unsafe { WinDestroyHelpInstance(h) };
        trace(alloc::format!("no help (error {:#x})", init.ret).as_bytes());
        return;
    }
    if h != 0 {
        unsafe { WinAssociateHelpInstance(h, a.frame) };
        a.help = h;
    }
}

/// Send a request to the help instance (a message if there is none).
pub fn help_request(msg: u32, mp1: u32, mp2: u32) {
    let a = app();
    if a.help == 0 {
        message(a.frame, b"Help for the System Editor is not available.", b"Help", MB_OK | MB_INFORMATION);
        return;
    }
    send(a.help, msg, mp1, mp2);
}

/// Show a help panel.
pub fn help_panel(panel: u16) {
    help_request(HM_DISPLAY_HELP, panel as u32, HM_RESOURCEID);
}

// ---- Window procedures ----

static mut FRAME_PROC: u32 = 0;

extern "C" fn frame_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    // WM_SAVEAPPLICATION and the help manager's notifications go to the
    // client.
    if m == WM_SAVEAPPLICATION || (HM_MSG_BASE..HM_MSG_BASE + 0x30).contains(&m) {
        return send(app().client, m, a, b);
    }
    let old: WndProc = unsafe { core::mem::transmute(FRAME_PROC as usize) };
    old(h, m, a, b)
}

extern "C" fn client_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_CREATE => {
            a.client = h;
            let style = WS_VISIBLE | MLS_VSCROLL | MLS_HSCROLL | if a.wrap { MLS_WORDWRAP } else { 0 };
            a.mle = unsafe { WinCreateWindow(h, WC_MLE, b"\0".as_ptr(), style, 0, 0, 0, 0, h, HWND_TOP, MLE_ID, core::ptr::null(), core::ptr::null()) };
            send(a.mle, MLM_SETTEXTLIMIT, u32::MAX, 0);
            send(a.mle, MLM_FORMAT, MLFIE_CFTEXT, 0);
            apply_look();
            0
        }
        WM_SIZE => {
            unsafe { WinSetWindowPos(a.mle, 0, 0, 0, (mp2 & 0xffff) as i16 as i32, (mp2 >> 16) as i16 as i32, SWP_MOVE | SWP_SIZE) };
            0
        }
        WM_SETFOCUS => {
            if mp2 & 0xffff != 0 {
                post(h, WM_USER, 0, 0);
            }
            0
        }
        WM_USER => {
            unsafe { WinSetFocus(HWND_DESKTOP, a.mle) };
            0
        }
        WM_PAINT => {
            let mut r = RectL::default();
            unsafe {
                let hps = WinBeginPaint(h, 0, &mut r);
                WinFillRect(hps, &r, a.bg);
                WinEndPaint(hps);
            }
            0
        }
        WM_INITMENU => {
            init_menu(mp1 & 0xffff);
            0
        }
        WM_COMMAND => {
            command((mp1 & 0xffff) as u16);
            0
        }
        WM_CONTROL => {
            if mp1 & 0xffff == MLE_ID && mp1 >> 16 == MLN_CHANGE {
                text_changed();
            }
            0
        }
        WM_CLOSE => {
            let ok = match a.pending_close.take() {
                Some(ok) => ok,
                None => confirm_discard(),
            };
            if ok {
                post(h, WM_QUIT, 0, 0);
            }
            0
        }
        WM_SAVEAPPLICATION => {
            save_settings();
            if send(a.mle, MLM_QUERYCHANGED, 0, 0) != 0 {
                a.pending_close = Some(confirm_discard());
            }
            0
        }
        HM_QUERY_KEYS_HELP => PANEL_KEYS as u32,
        HM_ERROR => {
            trace(alloc::format!("help error {mp1:#x}").as_bytes());
            0
        }
        _ => unsafe { WinDefWindowProc(h, m, mp1, mp2) },
    }
}

/// Font, colours and word wrap on the MLE.
pub fn apply_look() {
    let a = app();
    send(a.mle, MLM_SETFONT, a.fattrs.as_ptr() as u32, 0);
    send(a.mle, MLM_SETTEXTCOLOR, a.fg as u32, 0);
    send(a.mle, MLM_SETBACKCOLOR, a.bg as u32, 0);
    send(a.mle, MLM_SETWRAP, a.wrap as u32, 0);
}

fn text_changed() {
    let a = app();
    if a.loading {
        return;
    }
    a.pending_close = None;
    a.changes += 1;
    if a.autosave && a.changes >= a.autosave_every.max(1) {
        if let Some(p) = a.path.clone() {
            trace(alloc::format!("autosave after {} changes", a.changes).as_bytes());
            save_to(&p, None);
        }
        a.changes = 0;
    }
}

/// Edit items enabled as the selection, undo and clipboard allow.
fn init_menu(id: u32) {
    let a = app();
    if id != IDM_EDIT as u32 {
        return;
    }
    let has_sel = send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0) != send(a.mle, MLM_QUERYSEL, MLFQS_MAXSEL, 0);
    let undo = send(a.mle, MLM_QUERYUNDO, 0, 0) != 0;
    let mut info = 0u32;
    let clip = unsafe { WinQueryClipbrdFmtInfo(a.hab, CF_TEXT, &mut info) } != 0;
    menu_attr(a.menu, IDM_UNDO, MIA_DISABLED, !undo);
    for id in [IDM_CUT, IDM_COPY, IDM_CLEAR] {
        menu_attr(a.menu, id, MIA_DISABLED, !has_sel);
    }
    menu_attr(a.menu, IDM_PASTE, MIA_DISABLED, !clip);
}

fn command(id: u16) {
    let a = app();
    match id {
        IDM_NEW => {
            if confirm_discard() {
                new_file();
            }
        }
        IDM_OPEN => {
            if confirm_discard() {
                if let Some(p) = dialogs::file_dialog() {
                    open_file(&p);
                }
            }
        }
        IDM_SAVE => {
            save();
        }
        IDM_SAVEAS => {
            save_as();
        }
        IDM_AUTOSAVE => dialogs::autosave(),
        IDM_UNDO => {
            send(a.mle, MLM_UNDO, 0, 0);
        }
        IDM_CUT => {
            send(a.mle, MLM_CUT, 0, 0);
        }
        IDM_COPY => {
            send(a.mle, MLM_COPY, 0, 0);
        }
        IDM_PASTE => {
            send(a.mle, MLM_PASTE, 0, 0);
        }
        IDM_CLEAR => {
            send(a.mle, MLM_CLEAR, 0, 0);
        }
        IDM_SELECTALL => {
            let n = send(a.mle, MLM_QUERYTEXTLENGTH, 0, 0);
            send(a.mle, MLM_SETSEL, 0, n);
        }
        IDM_FIND => dialogs::find(),
        IDM_FONT => dialogs::font(),
        IDM_COLORS => dialogs::colors(),
        IDM_WRAPON | IDM_WRAPOFF => {
            a.wrap = id == IDM_WRAPON;
            send(a.mle, MLM_SETWRAP, a.wrap as u32, 0);
            menu_attr(a.menu, IDM_WRAPON, MIA_CHECKED, a.wrap);
            menu_attr(a.menu, IDM_WRAPOFF, MIA_CHECKED, !a.wrap);
            save_settings();
        }
        IDM_HELPINDEX => help_request(HM_HELP_INDEX, 0, 0),
        IDM_GENERALHELP => help_request(HM_EXT_HELP, 0, 0),
        IDM_USINGHELP => help_request(HM_DISPLAY_HELP, 0, 0),
        IDM_KEYSHELP => help_request(HM_KEYS_HELP, 0, 0),
        IDM_PRODUCTINFO => dialogs::product_info(),
        _ => {}
    }
    if id != IDM_FIND {
        unsafe { WinSetFocus(HWND_DESKTOP, a.mle) };
    }
}

// ---- Files ----

pub fn set_title() {
    let a = app();
    let t = z(a.path.as_deref().unwrap_or(UNTITLED));
    unsafe { WinSetWindowText(a.frame, t.as_ptr()) };
}

fn new_file() {
    let a = app();
    set_text(b"");
    a.path = None;
    set_title();
}

/// Replace the MLE's text (CR LF text) as a fresh, unchanged document.
pub fn set_text(data: &[u8]) {
    let a = app();
    a.loading = true;
    send(a.mle, MLM_DISABLEREFRESH, 0, 0);
    let len = send(a.mle, MLM_QUERYTEXTLENGTH, 0, 0);
    send(a.mle, MLM_DELETE, 0, len);
    send(a.mle, MLM_FORMAT, MLFIE_CFTEXT, 0);
    let mut off = 0usize;
    let mut ipt: i32 = 0;
    while off < data.len() {
        let chunk = &data[off..(off + 0xf000).min(data.len())];
        send(a.mle, MLM_SETIMPORTEXPORT, chunk.as_ptr() as u32, chunk.len() as u32);
        let n = send(a.mle, MLM_IMPORT, &mut ipt as *mut i32 as u32, chunk.len() as u32);
        if n == 0 {
            break;
        }
        off += n as usize;
    }
    send(a.mle, MLM_SETSEL, 0, 0);
    send(a.mle, MLM_SETCHANGED, 0, 0);
    send(a.mle, MLM_RESETUNDO, 0, 0);
    send(a.mle, MLM_ENABLEREFRESH, 0, 0);
    a.changes = 0;
    a.pending_close = None;
    a.loading = false;
}

/// The MLE's text as CR LF text.
pub fn get_text() -> Vec<u8> {
    let a = app();
    send(a.mle, MLM_FORMAT, MLFIE_CFTEXT, 0);
    let chars = send(a.mle, MLM_QUERYTEXTLENGTH, 0, 0);
    let bytes = send(a.mle, MLM_QUERYFORMATTEXTLENGTH, 0, chars) as usize;
    let mut buf = alloc::vec![0u8; bytes + 16];
    send(a.mle, MLM_SETIMPORTEXPORT, buf.as_mut_ptr() as u32, buf.len() as u32);
    let mut ipt: i32 = 0;
    let mut count = chars;
    let n = send(a.mle, MLM_EXPORT, &mut ipt as *mut i32 as u32, &mut count as *mut u32 as u32) as usize;
    buf.truncate(n.min(bytes + 16));
    buf
}

/// Open a file (a new, empty one if it does not exist).
pub fn open_file(name: &[u8]) -> bool {
    let a = app();
    let full = file::full_name(name);
    match file::read(&full) {
        Ok(data) => {
            set_text(&data);
            trace(alloc::format!("opened {} ({} bytes)", txt(&full), data.len()).as_bytes());
        }
        // Not there (DosOpen with OPEN_ACTION_FAIL_IF_NEW: ERROR_OPEN_FAILED).
        Err(2 | 110) => {
            set_text(b"");
            trace(alloc::format!("new file {}", txt(&full)).as_bytes());
        }
        Err(rc) => {
            let t = alloc::format!("Cannot open {} (error {rc}).", txt(&full));
            message(a.frame, t.as_bytes(), b"System Editor", MB_OK | MB_ERROR);
            return false;
        }
    }
    a.path = Some(full);
    set_title();
    true
}

/// Write the text to a file, setting its type if given; the document
/// is then unchanged.
pub fn save_to(path: &[u8], ty: Option<&[u8]>) -> bool {
    let a = app();
    let data = get_text();
    // Rewriting a file keeps its type.
    let old = file::types(path);
    if let Err(rc) = file::write(path, &data) {
        let t = alloc::format!("Cannot save {} (error {rc}).", txt(path));
        message(a.frame, t.as_bytes(), b"System Editor", MB_OK | MB_ERROR);
        return false;
    }
    match ty {
        Some(t) => {
            file::set_type(path, t);
        }
        None => {
            if !old.is_empty() && file::types(path).is_empty() {
                file::set_type(path, &old[0]);
            }
        }
    }
    send(a.mle, MLM_SETCHANGED, 0, 0);
    a.changes = 0;
    trace(alloc::format!("saved {} ({} bytes{})", txt(path), data.len(), ty.map_or(alloc::string::String::new(), |t| alloc::format!(", type {}", txt(t)))).as_bytes());
    true
}

/// File > Save: Save as for Untitled; a file without a type first asks
/// for one (as E does).
pub fn save() -> bool {
    let a = app();
    let Some(path) = a.path.clone() else { return save_as() };
    if !file::types(&path).is_empty() {
        return save_to(&path, None);
    }
    loop {
        match dialogs::no_type() {
            dialogs::NoType::Type => {
                if let Some(t) = dialogs::choose_type() {
                    return save_to(&path, Some(TYPES[t]));
                }
            }
            dialogs::NoType::SaveAs => return save_as(),
            dialogs::NoType::Cancel => return false,
        }
    }
}

/// File > Save as: the file dialog, then the file (with the type chosen
/// in the dialog, if any).
pub fn save_as() -> bool {
    let a = app();
    loop {
        let Some((path, ty)) = dialogs::save_as_dialog() else { return false };
        let path = file::full_name(&path);
        if a.path.as_deref() != Some(&path[..]) && file::exists(&path) {
            let t = alloc::format!("{} already exists. Do you want to replace it?", txt(&path));
            if message(a.frame, t.as_bytes(), b"Save as", MB_YESNO | MB_WARNING) != MBID_YES {
                continue;
            }
        }
        if !save_to(&path, ty.map(|t| TYPES[t])) {
            return false;
        }
        a.path = Some(path);
        set_title();
        return true;
    }
}

/// Unsaved changes: ask (Save, Save as, Discard, Cancel). True when the
/// text may go.
pub fn confirm_discard() -> bool {
    let a = app();
    if send(a.mle, MLM_QUERYCHANGED, 0, 0) == 0 {
        return true;
    }
    match dialogs::file_changed() {
        dialogs::Changed::Save => save(),
        dialogs::Changed::SaveAs => save_as(),
        dialogs::Changed::Discard => true,
        dialogs::Changed::Cancel => false,
    }
}

/// OS/2 text (one char per byte) for format!.
pub fn txt(b: &[u8]) -> alloc::string::String {
    b.iter().map(|&c| c as char).collect()
}

// ---- Settings (OS2.INI) ----

/// version, wrap, autosave, 0, every, fg, bg, points, FATTRS.
const SETTINGS_LEN: usize = 4 + 16 + 56;

fn load_settings() {
    let a = app();
    let mut b = [0u8; SETTINGS_LEN];
    let mut n = b.len() as u32;
    let ok = unsafe { PrfQueryProfileData(HINI_USERPROFILE, INI_APP.as_ptr(), INI_KEY.as_ptr(), b.as_mut_ptr(), &mut n) } != 0;
    if !ok || n as usize != SETTINGS_LEN || b[0] != 1 {
        return;
    }
    let u = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    a.wrap = b[1] != 0;
    a.autosave = b[2] != 0;
    a.autosave_every = u(4).max(1);
    a.fg = u(8) as i32;
    a.bg = u(12) as i32;
    a.points = u(16);
    a.fattrs.copy_from_slice(&b[20..76]);
}

pub fn save_settings() {
    let a = app();
    let mut b = Vec::with_capacity(SETTINGS_LEN);
    b.extend_from_slice(&[1, a.wrap as u8, a.autosave as u8, 0]);
    for v in [a.autosave_every, a.fg as u32, a.bg as u32, a.points] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&a.fattrs);
    unsafe { PrfWriteProfileData(HINI_USERPROFILE, INI_APP.as_ptr(), INI_KEY.as_ptr(), b.as_ptr(), b.len() as u32) };
}
