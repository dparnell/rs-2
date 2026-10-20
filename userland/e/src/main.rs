//! E.EXE: rs2's System Editor, a re-implementation of OS/2's E.
//!
//! It follows OS/2's E (studied in its binary): a standard frame titled
//! with the file's full name (or "Untitled") whose client holds a WC_MLE
//! (id 0x200), E's menus and command ids (File 0x400.., Edit 0x410..,
//! Options 0x428.., Help 0x42d..), its accelerators (Ctrl+F Find, F3
//! close), its dialogs, its help table and its OS2.INI settings
//! (application "EHXMAIN": IAUTOSAVE, BAUTOSAVEON, ULFGNDCOLOR,
//! ULBGNDCOLOR, BWRAPSTYLE, BCASESENSEARCH, BWRAPSEARCH, USDEFAULTTYPEEA,
//! FONTATTRS, FXPTSIZE, WINDOWPOSITION).
//!
//! Files: `E [file]`, File > Open (the standard file dialog), New; the
//! text is CR LF with a Ctrl+Z end-of-file mark (dropped on reading,
//! written on saving). NUL characters in a file become blanks after
//! asking. Save looks at the file's .TYPE first: none asks for one ("Save
//! notification": Type..., Save as...), types the editor does not know
//! ask whether to add "Plain Text", Save as gets the default type (the
//! one last chosen in the Type dialog). Closing the window with changes
//! beeps and asks (Save, Save as, Discard, Cancel); so does WM_QUIT from
//! outside (shutdown), with WinCancelShutdown when the answer is Cancel.
//! Autosave saves after N changes.
//!
//! `E /SELFTEST` runs self-checks of the file and editing logic and
//! prints PASS/FAIL lines and "e selftest done: N failure(s)".

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

// Menu commands (OS/2's E's ids).
pub const IDM_FILE: u16 = 0x400;
pub const IDM_OPEN: u16 = 0x401;
pub const IDM_NEW: u16 = 0x402;
pub const IDM_SAVE: u16 = 0x403;
pub const IDM_SAVEAS: u16 = 0x404;
pub const IDM_AUTOSAVE: u16 = 0x406;
pub const IDM_PRODUCTINFO: u16 = 0x408;
pub const IDM_CLOSE: u16 = 0x409;
pub const IDM_EDIT: u16 = 0x410;
pub const IDM_UNDO: u16 = 0x411;
pub const IDM_CUT: u16 = 0x422;
pub const IDM_COPY: u16 = 0x423;
pub const IDM_PASTE: u16 = 0x424;
pub const IDM_CLEAR: u16 = 0x425;
pub const IDM_FIND: u16 = 0x426;
pub const IDM_SELECTALL: u16 = 0x427;
pub const IDM_OPTIONS: u16 = 0x428;
pub const IDM_FONT: u16 = 0x42a;
pub const IDM_COLORS: u16 = 0x42b;
pub const IDM_WRAP: u16 = 0x42c;
pub const IDM_HELP: u16 = 0x42d;
pub const IDM_USINGHELP: u16 = 0x42e;
pub const IDM_HELPINDEX: u16 = 0x42f;
pub const IDM_KEYSHELP: u16 = 0x430;
pub const IDM_GENERALHELP: u16 = 0x431;
pub const IDM_WRAPON: u16 = 0x433;
pub const IDM_WRAPOFF: u16 = 0x434;

/// Dialog ids (their help subtables).
pub const DLG_CHANGED: u16 = 0x3000;
pub const DLG_COLORS: u16 = 0x4000;
pub const DLG_AUTOSAVE: u16 = 0x5000;
pub const DLG_FIND: u16 = 0x6000;
pub const DLG_PRODUCT: u16 = 0x7500;
pub const DLG_TYPE: u16 = 0x41a;
pub const DLG_NOTYPE: u16 = 0x424;
pub const DLG_BADTYPE: u16 = 0x42e;
pub const DLG_NULLS: u16 = 0x438;
pub const DLG_OPEN: u16 = 0x3e8;
pub const DLG_SAVEAS: u16 = 0x3e9;
pub const DLG_FONT: u16 = 0x104;
/// The Help push button of every dialog.
pub const ID_HELP: u32 = 0x3f5;

/// Help panels (res= numbers of the help file).
pub const PANEL_EXTENDED: u16 = 0x1030;
pub const PANEL_KEYS: u16 = 0x1031;
pub const PANEL_OPEN: u16 = 0x1043;
pub const PANEL_SAVEAS: u16 = 0x1072;
pub const PANEL_AUTOSAVE: u16 = 0x1082;
pub const PANEL_FIND: u16 = 0x1161;
pub const PANEL_FONT: u16 = 0x1181;
pub const PANEL_COLORS: u16 = 0x1191;
pub const PANEL_CHANGED: u16 = 0x1360;
pub const PANEL_NOTYPE: u16 = 0x1392;
pub const PANEL_BADTYPE: u16 = 0x139c;
pub const PANEL_TYPE: u16 = 0x13a6;
pub const PANEL_NULLS: u16 = 0x13ba;
pub const PANEL_PRODUCT: u16 = 0xfa60;

const FRAME_ID: u32 = 1;
pub const MLE_ID: u32 = 0x200;
const CLASS: &[u8] = b"EHXMAIN\0";
const INI_APP: &[u8] = b"EHXMAIN\0";
pub const UNTITLED: &[u8] = b"Untitled";

/// The file types the editor offers (.TYPE values).
pub const TYPES: [&[u8]; 3] = [b"Plain Text", b"OS/2 Command File", b"DOS Command File"];

/// Colours (CLR_*).
pub const CLR_WHITE: i32 = -2;
pub const CLR_BLACK: i32 = -1;
pub const CLR_BACKGROUND: i32 = 0;
pub const CLR_NEUTRAL: i32 = 7;

pub struct App {
    pub hab: u32,
    pub hmq: u32,
    pub frame: u32,
    pub client: u32,
    pub mle: u32,
    pub menu: u32,
    pub help: u32,
    /// The file's full name (None: Untitled).
    pub path: Option<Vec<u8>>,
    /// The frame's own title (the program's title, if it was started with
    /// one): the title bar shows "PREFIX - FILE".
    pub title_prefix: Vec<u8>,
    pub wrap: bool,
    pub autosave: bool,
    pub autosave_every: u32,
    /// Changes since the last save (for autosave).
    pub changes: u32,
    /// Saving for autosave (the type questions say so; Cancel there turns
    /// autosave off).
    pub autosaving: bool,
    pub fg: i32,
    pub bg: i32,
    /// The font chosen (FATTRS), if any.
    pub fattrs: Option<[u8; 56]>,
    /// Its size (FIXED points).
    pub point_size: u32,
    /// TYPES index given by Save as and the Type dialog.
    pub default_type: usize,
    /// The file's type was dealt with by a save in this session.
    pub type_checked: bool,
    pub find: u32,
    pub find_text: Vec<u8>,
    pub change_text: Vec<u8>,
    pub find_case: bool,
    pub find_wrap: bool,
    /// Asking about changes (no second question meanwhile).
    pub in_save_quit: bool,
    /// Loading a file: MLN_CHANGE does not count.
    pub loading: bool,
}

static mut APP: App = App {
    hab: 0,
    hmq: 0,
    frame: 0,
    client: 0,
    mle: 0,
    menu: 0,
    help: 0,
    path: None,
    title_prefix: Vec::new(),
    wrap: true,
    autosave: false,
    autosave_every: 100,
    changes: 0,
    autosaving: false,
    fg: CLR_NEUTRAL,
    bg: CLR_BACKGROUND,
    fattrs: None,
    point_size: 0,
    default_type: 0,
    type_checked: false,
    find: 0,
    find_text: Vec::new(),
    change_text: Vec::new(),
    find_case: true,
    find_wrap: false,
    in_save_quit: false,
    loading: false,
};

pub fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

fn main(s: &os2::Startup) -> u32 {
    let args = s.args();
    let arg = {
        let start = args.iter().position(|&c| c != b' ').unwrap_or(args.len());
        let end = args.iter().rposition(|&c| c != b' ').map_or(start, |i| i + 1);
        let a = &args[start..end.max(start)];
        a.strip_prefix(b"\"").and_then(|a| a.strip_suffix(b"\"")).unwrap_or(a)
    };
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    a.hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if a.hmq == 0 {
        return 1;
    }
    if arg.eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(a.hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    load_settings();
    if !create_window() {
        return 1;
    }
    if !arg.is_empty() {
        open_file(arg);
    }
    show_window();
    // Closing asks about changes; so does a WM_QUIT from outside (a
    // shutdown), which Cancel cancels.
    let mut q = QMsg::default();
    loop {
        while unsafe { WinGetMsg(a.hab, &mut q, 0, 0, 0) } != 0 {
            unsafe { WinDispatchMsg(a.hab, &q) };
        }
        if process_save_quit() {
            break;
        }
        unsafe { WinCancelShutdown(a.hmq, 0) };
    }
    trace(b"closed");
    save_window_position();
    unsafe {
        if a.help != 0 {
            WinAssociateHelpInstance(0, a.frame);
            WinDestroyHelpInstance(a.help);
        }
        WinDestroyWindow(a.frame);
        WinDestroyMsgQueue(a.hmq);
        WinTerminate(a.hab);
    }
    0
}

/// The frame, its menu bar, accelerators, the MLE and the help instance.
pub fn create_window() -> bool {
    let a = app();
    unsafe { WinRegisterClass(a.hab, CLASS.as_ptr(), client_proc, 0x0004 /* CS_SIZEREDRAW */, 4) };
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), core::ptr::null(), WS_CLIPCHILDREN, 0, FRAME_ID, &mut client) };
    if frame == 0 {
        return false;
    }
    a.frame = frame;
    a.client = client;
    a.title_prefix = window_text(frame);
    let tpl = menu_template(&menu_items());
    a.menu = unsafe { WinCreateMenu(frame, tpl.as_ptr()) };
    send(frame, WM_UPDATEFRAME, FCF_MENU, 0);
    // Ctrl+F (with or without Shift): Find; F3: close.
    let accels: [(u16, u16, u16); 5] = [
        (AF_CHAR | AF_CONTROL, b'f' as u16, IDM_FIND),
        (AF_CHAR | AF_CONTROL, b'F' as u16, IDM_FIND),
        (AF_CHAR | AF_CONTROL | AF_SHIFT, b'f' as u16, IDM_FIND),
        (AF_CHAR | AF_CONTROL | AF_SHIFT, b'F' as u16, IDM_FIND),
        (AF_VIRTUALKEY, VK_F3, IDM_CLOSE),
    ];
    let mut acc = Vec::new();
    acc.extend_from_slice(&(accels.len() as u16).to_le_bytes());
    acc.extend_from_slice(&850u16.to_le_bytes());
    for (fs, key, cmd) in accels {
        acc.extend_from_slice(&fs.to_le_bytes());
        acc.extend_from_slice(&key.to_le_bytes());
        acc.extend_from_slice(&cmd.to_le_bytes());
    }
    unsafe {
        let haccel = WinCreateAccelTable(a.hab, acc.as_ptr());
        WinSetAccelTable(a.hab, haccel, frame);
    }
    // The help manager's notifications to the frame go to the client.
    let old = unsafe { WinSubclassWindow(frame, frame_proc) };
    unsafe { FRAME_PROC = old };
    create_help();
    set_title();
    true
}

/// Where the window was last time (OS2.INI), else where the shell puts
/// programs.
fn show_window() {
    let a = app();
    let mut swp = Swp::default();
    let size = core::mem::size_of::<Swp>() as u32;
    let mut n = size;
    let saved = unsafe { PrfQueryProfileData(HINI_USERPROFILE, INI_APP.as_ptr(), b"WINDOWPOSITION\0".as_ptr(), &mut swp as *mut Swp as *mut u8, &mut n) } != 0
        && n == size
        && swp.cx > 0
        && swp.cy > 0;
    if !saved {
        unsafe { WinQueryTaskSizePos(a.hab, 0, &mut swp) };
    }
    unsafe {
        WinSetWindowPos(a.frame, HWND_TOP, swp.x, swp.y, swp.cx, swp.cy, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, a.mle);
    }
}

fn save_window_position() {
    let a = app();
    let swp = window_pos(a.frame);
    // Minimized or maximized: keep the last normal position.
    if swp.fl & 0x0c00 != 0 || swp.cx <= 0 || swp.cy <= 0 {
        return;
    }
    save_data(b"WINDOWPOSITION\0", unsafe { core::slice::from_raw_parts(&swp as *const Swp as *const u8, core::mem::size_of::<Swp>()) });
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
            Menu::Item(IDM_HELPINDEX, b"~Help index"),
            Menu::Item(IDM_GENERALHELP, b"~General help"),
            Menu::Item(IDM_USINGHELP, b"~Using help"),
            Menu::Item(IDM_KEYSHELP, b"~Keys help"),
            Menu::Sep,
            Menu::Item(IDM_PRODUCTINFO, b"~Product information"),
        ]),
    ]
}

// ---- Help ----

/// The help table (kept for the life of the instance): the frame's
/// subtable (menu items) and one per dialog, numbered as OS/2's E does.
static mut HELP_TABLE: Vec<u8> = Vec::new();
static mut HELP_SUBTABLES: Vec<Vec<u16>> = Vec::new();

fn help_table() -> *const u8 {
    let frame: &[(u16, u16)] = &[
        (IDM_FILE, 0x1320),
        (IDM_OPEN, 0x1040),
        (IDM_NEW, 0x1050),
        (IDM_SAVE, 0x1060),
        (IDM_SAVEAS, 0x1070),
        (IDM_AUTOSAVE, 0x1080),
        (IDM_PRODUCTINFO, PANEL_PRODUCT),
        (IDM_CLOSE, 0x1100),
        (IDM_EDIT, 0x1330),
        (IDM_UNDO, 0x1110),
        (IDM_CUT, 0x1120),
        (IDM_COPY, 0x1130),
        (IDM_PASTE, 0x1140),
        (IDM_CLEAR, 0x1150),
        (IDM_FIND, 0x1160),
        (IDM_SELECTALL, 0x1170),
        (IDM_OPTIONS, 0x1340),
        (IDM_FONT, 0x1180),
        (IDM_COLORS, 0x1190),
        (IDM_WRAP, 0x1310),
        (IDM_WRAPON, 0x1310),
        (IDM_WRAPOFF, 0x1310),
        (IDM_HELP, 0xfa16),
        (IDM_USINGHELP, 0xfa17),
        (IDM_GENERALHELP, 0xfa18),
        (IDM_HELPINDEX, 0xfa1a),
        (IDM_KEYSHELP, 0xfa19),
    ];
    // (window, extended help panel, its items); every item of a dialog
    // shows the dialog's panel.
    let help = ID_HELP as u16;
    let dialogs: &[(u16, u16, &[u16])] = &[
        (DLG_OPEN, PANEL_OPEN, &[0x3eb, 0x3f1, 0x3ef, 1, 2, help]),
        (DLG_SAVEAS, PANEL_SAVEAS, &[0x3eb, 0x3ef, 1, 0x3f3, 2, help]),
        (DLG_CHANGED, PANEL_CHANGED, &[0x3003, 0x3004, 0x3005, 2, help]),
        (DLG_COLORS, PANEL_COLORS, &[0x4001, 0x4002, 1, 0x4006, 2, help]),
        (DLG_FONT, PANEL_FONT, &[0x106, 0x107, 0x109, 0x10a, 0x10b, 0x10c, 0x10d, 0x10e, 0x10f, help]),
        (DLG_AUTOSAVE, PANEL_AUTOSAVE, &[0x5004, 0x5002, 0x5001, 0x5005, help]),
        (DLG_FIND, PANEL_FIND, &[0x6005, 0x6006, 0x6050, 0x6051, 1, 0x6003, 0x6001, 0x6004, 2, help]),
        (DLG_PRODUCT, PANEL_PRODUCT, &[1, help]),
        (DLG_TYPE, PANEL_TYPE, &[0x41b, 1, 2, help]),
        (DLG_NOTYPE, PANEL_NOTYPE, &[0x425, 0x426, 2, help]),
        (DLG_BADTYPE, PANEL_BADTYPE, &[0x42f, 0x430, 0x426, 2, help]),
        (DLG_NULLS, PANEL_NULLS, &[1, 2, help]),
    ];
    unsafe {
        let subs = &mut *core::ptr::addr_of_mut!(HELP_SUBTABLES);
        let table = &mut *core::ptr::addr_of_mut!(HELP_TABLE);
        if !table.is_empty() {
            return table.as_ptr();
        }
        // HELPSUBTABLE: words per entry, (item, panel) pairs, 0.
        let mut sub = alloc::vec![2u16];
        for &(id, panel) in frame {
            sub.extend_from_slice(&[id, panel]);
        }
        sub.push(0);
        subs.push(sub);
        for &(_, panel, items) in dialogs {
            let mut sub = alloc::vec![2u16];
            for &id in items {
                sub.extend_from_slice(&[id, panel]);
            }
            sub.push(0);
            subs.push(sub);
        }
        // HELPTABLE (packed): idAppWindow, phstHelpSubTable, idExtPanel;
        // a zero entry ends it.
        let windows = core::iter::once((FRAME_ID as u16, PANEL_EXTENDED)).chain(dialogs.iter().map(|&(w, p, _)| (w, p)));
        for ((w, p), sub) in windows.zip(subs.iter()) {
            table.extend_from_slice(&w.to_le_bytes());
            table.extend_from_slice(&(sub.as_ptr() as u32).to_le_bytes());
            table.extend_from_slice(&p.to_le_bytes());
        }
        table.extend_from_slice(&[0u8; 8]);
        table.as_ptr()
    }
}

/// E.HLP (or OS/2's EHXHP.HLP, whose panels have the same numbers) on
/// the HELP path or in \OS2\HELP of the boot drive.
fn find_help_file() -> Option<Vec<u8>> {
    let mut boot = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(5, 5, boot.as_mut_ptr(), 4) };
    let mut dir = alloc::vec![b'@' + boot[0] as u8];
    dir.extend_from_slice(b":\\OS2\\HELP\0");
    for name in [&b"E.HLP\0"[..], b"EHXHP.HLP\0"] {
        let mut buf = [0u8; 260];
        // SEARCH_ENVIRONMENT: the directories in HELP.
        if unsafe { DosSearchPath(2, b"HELP\0".as_ptr(), name.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0
            || unsafe { DosSearchPath(0, dir.as_ptr(), name.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0
        {
            return Some(until_nul(&buf).to_vec());
        }
    }
    None
}

fn create_help() {
    let a = app();
    let Some(lib) = find_help_file() else {
        trace(b"no help file");
        return;
    };
    let lib = z(&lib);
    let mut init = HelpInit {
        cb: core::mem::size_of::<HelpInit>() as u32,
        ret: 0,
        tutorial: core::ptr::null(),
        table: help_table(),
        table_module: 0,
        accel_module: 0,
        accel_id: 0,
        action_bar_id: 0,
        title: b"System Editor Help\0".as_ptr(),
        show_panel_id: 0,
        library: lib.as_ptr(),
    };
    let h = unsafe { WinCreateHelpInstance(a.hab, &mut init) };
    if h == 0 || init.ret != 0 {
        if h != 0 {
            unsafe { WinDestroyHelpInstance(h) };
        }
        trace(alloc::format!("no help (error {:#x})", init.ret).as_bytes());
        return;
    }
    if unsafe { WinAssociateHelpInstance(h, a.frame) } == 0 {
        unsafe { WinDestroyHelpInstance(h) };
        return;
    }
    a.help = h;
    trace(alloc::format!("help from {}", txt(until_nul(&lib))).as_bytes());
}

/// Send a request to the help instance (a message if there is none).
pub fn help_request(msg: u32, mp1: u32, mp2: u32) {
    let a = app();
    if a.help == 0 {
        message(a.frame, b"Help is not available: the help file E.HLP was not found.", b"System Editor", MB_OK | MB_INFORMATION);
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
    if (HM_MSG_BASE..HM_MSG_BASE + 0x30).contains(&m) {
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
            create_mle(h);
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
                WinFillRect(hps, &r, 15 /* CLR_PALEGRAY */);
                WinEndPaint(hps);
            }
            0
        }
        WM_INITMENU => {
            init_menu((mp1 & 0xffff) as u16);
            0
        }
        WM_COMMAND => command((mp1 & 0xffff) as u16) as u32,
        WM_CONTROL => {
            if mp1 & 0xffff == MLE_ID {
                match mp1 >> 16 {
                    MLN_CHANGE => text_changed(),
                    MLN_OVERFLOW => {
                        message(a.frame, b"The text is too long for the editor.", b"Edit Application Error", MB_OK | MB_ERROR);
                    }
                    _ => {}
                }
            }
            0
        }
        WM_CLOSE => {
            if process_save_quit() {
                post(h, WM_QUIT, 0, 0);
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

/// The MLE (no border, both scroll bars), with the colours, word wrap
/// and font of the settings.
fn create_mle(client: u32) {
    let a = app();
    let style = WS_VISIBLE | MLS_VSCROLL | MLS_HSCROLL;
    a.mle = unsafe { WinCreateWindow(client, WC_MLE, b"\0".as_ptr(), style, 0, 0, 0, 0, client, 3 /* HWND_TOP */, MLE_ID, core::ptr::null(), core::ptr::null()) };
    send(a.mle, MLM_SETTEXTLIMIT, u32::MAX, 0);
    send(a.mle, MLM_FORMAT, MLFIE_CFTEXT, 0);
    send(a.mle, MLM_SETBACKCOLOR, a.bg as u32, 0);
    send(a.mle, MLM_SETTEXTCOLOR, a.fg as u32, 0);
    send(a.mle, MLM_SETWRAP, a.wrap as u32, 0);
    if let Some(f) = a.fattrs {
        send(a.mle, MLM_SETFONT, f.as_ptr() as u32, 0);
    }
    set_tabs();
    send(a.mle, MLM_RESETUNDO, 0, 0);
}

/// Tab stops every 8 average character widths of the font.
pub fn set_tabs() {
    let a = app();
    let mut f = [0u8; 56];
    send(a.mle, MLM_QUERYFONT, f.as_mut_ptr() as u32, 0);
    let avg = u32::from_le_bytes([f[48], f[49], f[50], f[51]]) & 0x1fff;
    if avg != 0 {
        send(a.mle, MLM_SETTABSTOP, avg << 3, 0);
    }
}

fn text_changed() {
    let a = app();
    if a.loading {
        return;
    }
    if !a.autosave {
        a.changes = 0;
        return;
    }
    a.changes += 1;
    if a.path.is_some() && a.autosave_every != 0 && a.changes >= a.autosave_every {
        trace(alloc::format!("autosave after {} changes", a.changes).as_bytes());
        a.autosaving = true;
        let ok = command(IDM_SAVE);
        a.autosaving = false;
        if ok {
            a.changes = 0;
        }
    }
}

pub fn changed() -> bool {
    send(app().mle, MLM_QUERYCHANGED, 0, 0) != 0
}

pub fn text_length() -> u32 {
    send(app().mle, MLM_QUERYTEXTLENGTH, 0, 0)
}

/// Menu items enabled and checked as the state allows (WM_INITMENU),
/// the Undo item naming what it undoes or redoes.
fn init_menu(id: u16) {
    let a = app();
    match id {
        IDM_FILE => menu_attr(a.menu, IDM_AUTOSAVE, MIA_DISABLED, a.path.is_none()),
        IDM_EDIT => {
            let undo = send(a.mle, MLM_QUERYUNDO, 0, 0);
            let text = z(&undo_text(undo));
            send(a.menu, MM_SETITEMTEXT, IDM_UNDO as u32, text.as_ptr() as u32);
            menu_attr(a.menu, IDM_UNDO, MIA_DISABLED, undo == 0);
            let has_sel = send(a.mle, MLM_QUERYSEL, MLFQS_MINSEL, 0) != send(a.mle, MLM_QUERYSEL, MLFQS_MAXSEL, 0);
            for id in [IDM_CUT, IDM_COPY, IDM_CLEAR] {
                menu_attr(a.menu, id, MIA_DISABLED, !has_sel);
            }
            let mut info = 0u32;
            let clip = unsafe { WinQueryClipbrdFmtInfo(a.hab, CF_TEXT, &mut info) } != 0;
            menu_attr(a.menu, IDM_PASTE, MIA_DISABLED, !clip);
            let empty = text_length() == 0;
            menu_attr(a.menu, IDM_FIND, MIA_DISABLED, empty);
            menu_attr(a.menu, IDM_SELECTALL, MIA_DISABLED, empty);
        }
        IDM_OPTIONS => {
            menu_attr(a.menu, IDM_WRAPON, MIA_CHECKED, a.wrap);
            menu_attr(a.menu, IDM_WRAPOFF, MIA_CHECKED, !a.wrap);
        }
        _ => {}
    }
}

/// The Undo item's text for MLM_QUERYUNDO's answer (low word: undo or
/// redo, high word: the operation): "~Undo typing", "~Redo paste", ...
fn undo_text(undo: u32) -> Vec<u8> {
    let what: &[u8] = match undo >> 16 {
        _ if undo == 0 => b"",
        0x7a => b" typing", // WM_CHAR
        MLM_INSERT => b" change",
        MLM_SETFONT => b" font",
        MLM_SETTEXTCOLOR => b" color",
        MLM_CUT => b" cut",
        MLM_PASTE => b" paste",
        MLM_CLEAR => b" clear",
        _ => b"",
    };
    let mut t = if undo != 0 && undo & 0xffff == 0 { b"~Redo".to_vec() } else { b"~Undo".to_vec() };
    t.extend_from_slice(what);
    t.extend_from_slice(b"\tAlt+Backspace");
    t
}

/// An MLE edit command; a message when it fails.
fn edit(msg: u32, what: &[u8]) -> bool {
    let a = app();
    if send(a.mle, msg, 0, 0) != 0 {
        return true;
    }
    let mut t = what.to_vec();
    t.extend_from_slice(b" could not be completed.");
    message(a.frame, &t, b"Clipboard Error", MB_OK | MB_ERROR);
    false
}

/// A menu command (also sent as WM_COMMAND); for Save and Save as the
/// result says whether the file was written.
pub fn command(id: u16) -> bool {
    let a = app();
    let r = match id {
        IDM_OPEN => {
            if !process_save_quit() {
                return false;
            }
            match dialogs::open_dialog() {
                Some(p) => open_file(&p),
                None => false,
            }
        }
        IDM_NEW => {
            if !process_save_quit() {
                return false;
            }
            new_file();
            true
        }
        IDM_SAVE => save(),
        IDM_SAVEAS => save_as(),
        IDM_AUTOSAVE => {
            dialogs::autosave();
            true
        }
        IDM_PRODUCTINFO => {
            dialogs::product_info();
            true
        }
        IDM_CLOSE => {
            post(a.client, WM_CLOSE, 0, 0);
            true
        }
        IDM_UNDO => return edit(MLM_UNDO, b"Undo"),
        IDM_CUT => return edit(MLM_CUT, b"Cut"),
        IDM_COPY => return edit(MLM_COPY, b"Copy"),
        IDM_PASTE => return edit(MLM_PASTE, b"Paste"),
        IDM_CLEAR => return edit(MLM_CLEAR, b"Clear"),
        IDM_FIND => {
            dialogs::find();
            true
        }
        IDM_SELECTALL => {
            send(a.mle, MLM_SETSEL, 0, text_length());
            true
        }
        IDM_FONT => {
            dialogs::font();
            true
        }
        IDM_COLORS => {
            dialogs::colors();
            true
        }
        IDM_WRAP | IDM_WRAPON | IDM_WRAPOFF => {
            a.wrap = match id {
                IDM_WRAPON => true,
                IDM_WRAPOFF => false,
                _ => !a.wrap,
            };
            send(a.mle, MLM_SETWRAP, a.wrap as u32, 0);
            menu_attr(a.menu, IDM_WRAPON, MIA_CHECKED, a.wrap);
            menu_attr(a.menu, IDM_WRAPOFF, MIA_CHECKED, !a.wrap);
            save_int(b"BWRAPSTYLE\0", a.wrap as i32);
            true
        }
        IDM_USINGHELP => {
            help_request(HM_DISPLAY_HELP, 0, 0);
            true
        }
        IDM_HELPINDEX => {
            help_request(HM_HELP_INDEX, 0, 0);
            true
        }
        IDM_KEYSHELP => {
            help_request(HM_KEYS_HELP, 0, 0);
            true
        }
        IDM_GENERALHELP => {
            help_request(HM_EXT_HELP, 0, 0);
            true
        }
        _ => return false,
    };
    // As E does: the undo history ends at a menu command.
    send(a.mle, MLM_RESETUNDO, 0, 0);
    if id != IDM_FIND {
        unsafe { WinSetFocus(HWND_DESKTOP, a.mle) };
    }
    r
}

// ---- Files ----

/// The title bar: the file's name (or Untitled), after the program's own
/// title if it has one.
pub fn set_title() {
    let a = app();
    let name = a.path.as_deref().unwrap_or(UNTITLED);
    let mut t = Vec::new();
    if !a.title_prefix.is_empty() {
        t.extend_from_slice(&a.title_prefix);
        t.extend_from_slice(b" - ");
    }
    t.extend_from_slice(name);
    let t = z(&t);
    unsafe { WinSetWindowText(a.frame, t.as_ptr()) };
}

pub fn new_file() {
    let a = app();
    set_text(b"");
    a.path = None;
    a.type_checked = false;
    set_title();
}

/// Replace the MLE's text (CR LF text) as a fresh, unchanged document.
pub fn set_text(data: &[u8]) {
    let a = app();
    a.loading = true;
    send(a.mle, MLM_DISABLEREFRESH, 0, 0);
    let len = text_length();
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
    a.loading = false;
}

/// The MLE's text as CR LF text.
pub fn get_text() -> Vec<u8> {
    let a = app();
    send(a.mle, MLM_FORMAT, MLFIE_CFTEXT, 0);
    let chars = text_length();
    let bytes = send(a.mle, MLM_QUERYFORMATTEXTLENGTH, 0, chars) as usize;
    let mut buf = alloc::vec![0u8; bytes + 16];
    send(a.mle, MLM_SETIMPORTEXPORT, buf.as_mut_ptr() as u32, buf.len() as u32);
    let mut ipt: i32 = 0;
    let mut count = chars;
    let n = send(a.mle, MLM_EXPORT, &mut ipt as *mut i32 as u32, &mut count as *mut u32 as u32) as usize;
    buf.truncate(n.min(bytes + 16));
    buf
}

/// What a file error means.
fn error_text(rc: u32) -> &'static str {
    match rc {
        2 | 110 => "The file was not found.",
        3 => "The path was not found.",
        4 => "Too many files are open.",
        5 => "Access was denied.",
        15 => "The drive does not exist.",
        19 => "The disk is write protected.",
        21 => "The drive is not ready.",
        32 | 33 => "The file is in use by another program.",
        112 => "The disk is full.",
        _ => "An error occurred.",
    }
}

/// A file error: Retry or Cancel; true for Retry.
fn retry(rc: u32, doing: &str, path: &[u8]) -> bool {
    let t = alloc::format!("Error {} {}.\n{} (error {rc})", doing, txt(path), error_text(rc));
    message(app().frame, t.as_bytes(), b"System Editor - Error", MB_RETRYCANCEL | MB_ERROR) == MBID_RETRY
}

/// Open a file (a new, empty document under its name if it does not
/// exist).
pub fn open_file(name: &[u8]) -> bool {
    let a = app();
    let full = file::full_name(name);
    let data = loop {
        match file::read(&full) {
            Ok(d) => break Some(file::text_of(d)),
            Err(2 | 3 | 110) => break None,
            Err(rc) => {
                if !retry(rc, "reading", &full) {
                    return false;
                }
            }
        }
    };
    match data {
        Some(mut text) => {
            // NUL characters become blanks, or the file is not loaded.
            if text.contains(&0) {
                if !dialogs::nulls() {
                    new_file();
                    return false;
                }
                for c in text.iter_mut().filter(|c| **c == 0) {
                    *c = b' ';
                }
            }
            set_text(&text);
            trace(alloc::format!("opened {} ({} bytes)", txt(&full), text.len()).as_bytes());
        }
        None => {
            set_text(b"");
            trace(alloc::format!("new file {}", txt(&full)).as_bytes());
        }
    }
    a.path = Some(full);
    a.type_checked = false;
    set_title();
    true
}

/// The types a save gives the file.
pub enum NewTypes {
    Keep,
    Set(Vec<Vec<u8>>),
}

/// The types for writing `path`, looking at its .TYPE first as E does:
/// Save as and a malformed type get the default type; Save of a file
/// without a type asks for one (the Type dialog changes the default), of
/// a file whose types the editor does not know asks whether to add
/// "Plain Text". None: not to be written.
pub fn types_for(path: &[u8], save_as: bool) -> Option<NewTypes> {
    let a = app();
    let ea = loop {
        match file::classify(path, &TYPES) {
            Ok(e) => break e,
            Err(rc) => {
                if !retry(rc, "reading the extended attributes of", path) {
                    return None;
                }
            }
        }
    };
    let default = || NewTypes::Set(alloc::vec![TYPES[app().default_type].to_vec()]);
    Some(match ea {
        file::TypeEa::Unsupported => NewTypes::Keep,
        ref e if a.type_checked && *e != file::TypeEa::NoFile => NewTypes::Keep,
        _ if save_as => default(),
        file::TypeEa::Malformed => default(),
        file::TypeEa::Known => NewTypes::Keep,
        file::TypeEa::NoFile | file::TypeEa::None => match dialogs::no_type(a.autosaving) {
            dialogs::Answer::Type => default(),
            dialogs::Answer::SaveAs => {
                post(a.client, WM_COMMAND, IDM_SAVEAS as u32, 0);
                return None;
            }
            _ => return None,
        },
        file::TypeEa::Unknown(list) => match dialogs::bad_type(a.autosaving) {
            dialogs::Answer::Yes => {
                let mut l = list;
                l.push(TYPES[0].to_vec());
                NewTypes::Set(l)
            }
            dialogs::Answer::No => NewTypes::Keep,
            dialogs::Answer::SaveAs => {
                post(a.client, WM_COMMAND, IDM_SAVEAS as u32, 0);
                return None;
            }
            _ => return None,
        },
    })
}

/// Write the text (and its end-of-file mark), then set the types; the
/// document is then unchanged.
pub fn write_with(path: &[u8], types: NewTypes) -> bool {
    let a = app();
    let mut data = get_text();
    data.push(file::EOF_MARK);
    // Rewriting a file keeps its types.
    let old = file::type_value(path).ok().filter(|v| !v.is_empty());
    loop {
        match file::write(path, &data) {
            Ok(()) => break,
            Err(rc) => {
                if !retry(rc, "writing", path) {
                    return false;
                }
            }
        }
    }
    let set = |t: &[Vec<u8>]| {
        let refs: Vec<&[u8]> = t.iter().map(|t| &t[..]).collect();
        file::set_types(path, &refs);
    };
    match &types {
        NewTypes::Set(t) => set(t),
        NewTypes::Keep => {
            if let Some(t) = old.as_deref().and_then(file::parse_types) {
                if file::types(path).is_empty() {
                    set(&t);
                }
            }
        }
    }
    send(a.mle, MLM_SETCHANGED, 0, 0);
    a.type_checked = true;
    let what = match &types {
        NewTypes::Set(t) => alloc::format!(", type {}", t.iter().map(|t| txt(t)).collect::<Vec<_>>().join(", ")),
        NewTypes::Keep => alloc::string::String::new(),
    };
    trace(alloc::format!("saved {} ({} bytes{what})", txt(path), data.len()).as_bytes());
    true
}

/// File > Save: Save as for Untitled; a notice when there is nothing to
/// save.
pub fn save() -> bool {
    let a = app();
    let Some(path) = a.path.clone() else { return save_as() };
    if text_length() > 0 && !changed() && !a.autosaving {
        let t = alloc::format!("{} has not changed since it was last saved,\nso it does not need to be saved now.", txt(&path));
        message(a.frame, t.as_bytes(), b"System Editor - Notification", MB_OK | MB_INFORMATION);
        return true;
    }
    match types_for(&path, false) {
        Some(t) => write_with(&path, t),
        None => false,
    }
}

/// File > Save as: the file dialog (with its Type button), asking before
/// replacing a file; the file gets the default type.
pub fn save_as() -> bool {
    let a = app();
    let Some(path) = dialogs::save_as_dialog() else { return false };
    let path = file::full_name(&path);
    if file::exists(&path) {
        let t = alloc::format!("{} already exists. Do you want to replace it?", txt(&path));
        if message(a.frame, t.as_bytes(), b"System Editor - Warning", MB_YESNO | MB_WARNING) != MBID_YES {
            return false;
        }
    }
    let Some(types) = types_for(&path, true) else { return false };
    if !write_with(&path, types) {
        return false;
    }
    a.path = Some(path);
    set_title();
    true
}

/// Unsaved changes: beep and ask (Save, Save as, Discard, Cancel). True
/// when the text may go (Discard empties the window).
pub fn process_save_quit() -> bool {
    let a = app();
    if a.in_save_quit || !changed() {
        return true;
    }
    unsafe { DosBeep(875, 40) };
    a.in_save_quit = true;
    let answer = dialogs::file_changed();
    if answer == dialogs::Answer::Discard {
        new_file();
    }
    a.in_save_quit = false;
    answer != dialogs::Answer::Cancel
}

/// OS/2 text (one char per byte) for format!.
pub fn txt(b: &[u8]) -> alloc::string::String {
    b.iter().map(|&c| c as char).collect()
}

// ---- Settings (OS2.INI, application EHXMAIN) ----

fn query_int(key: &[u8], default: i32) -> i32 {
    unsafe { PrfQueryProfileInt(HINI_USERPROFILE, INI_APP.as_ptr(), key.as_ptr(), default) }
}

pub fn save_int(key: &[u8], v: i32) {
    let s = z(alloc::format!("{v}").as_bytes());
    unsafe { PrfWriteProfileString(HINI_USERPROFILE, INI_APP.as_ptr(), key.as_ptr(), s.as_ptr()) };
}

pub fn save_data(key: &[u8], data: &[u8]) {
    unsafe { PrfWriteProfileData(HINI_USERPROFILE, INI_APP.as_ptr(), key.as_ptr(), data.as_ptr(), data.len() as u32) };
}

pub fn load_settings() {
    let a = app();
    a.autosave_every = query_int(b"IAUTOSAVE\0", 100).clamp(0, 9999) as u32;
    a.autosave = query_int(b"BAUTOSAVEON\0", 0) != 0;
    a.fg = query_int(b"ULFGNDCOLOR\0", CLR_NEUTRAL);
    a.bg = query_int(b"ULBGNDCOLOR\0", CLR_BACKGROUND);
    a.wrap = query_int(b"BWRAPSTYLE\0", 1) != 0;
    a.find_case = query_int(b"BCASESENSEARCH\0", 1) != 0;
    a.find_wrap = query_int(b"BWRAPSEARCH\0", 0) != 0;
    a.default_type = (query_int(b"USDEFAULTTYPEEA\0", 0).max(0) as usize).min(TYPES.len() - 1);
    let mut f = [0u8; 56];
    let mut n = 56u32;
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, INI_APP.as_ptr(), b"FONTATTRS\0".as_ptr(), f.as_mut_ptr(), &mut n) } != 0 && n == 56 {
        a.fattrs = Some(f);
    }
    let mut p = [0u8; 4];
    let mut n = 4u32;
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, INI_APP.as_ptr(), b"FXPTSIZE\0".as_ptr(), p.as_mut_ptr(), &mut n) } != 0 && n == 4 {
        a.point_size = u32::from_le_bytes(p);
    }
}
