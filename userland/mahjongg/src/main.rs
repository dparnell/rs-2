//! MAHJONGG.EXE: rs2's Mahjongg Solitaire, a re-implementation of the
//! Mahjongg that OS/2 Warp 4.52 installs in its Games folder.
//!
//! As OS/2's (studied in its binary and resources): 144 tiles stacked
//! in a layout; two free tiles with the same face (any two flowers, any
//! two seasons) are taken off together; a tile is free when nothing lies
//! on it and its left or right side is open. The menus are OS/2's, with
//! its command ids and accelerators: Game (New deal: Random, Same,
//! Select...; Default, Open..., Save, Save as...; Auto play), Moves
//! (Take back, Replay, Show replay, Cheat, Show), View (Play, Edit),
//! Options (Timer, Matches, Settings..., Save settings) and Help; the
//! pop-up menu on mouse button 2. Layouts are OS/2's game files
//! (`.MAH`, see layout.rs), read and written; the built-in one is the
//! traditional turtle. Deals are numbered (Select... picks one) and, as
//! an option, made solvable. Edit mode builds layouts level by level.
//! Settings are kept in MAHJONGG.INI as OS/2's (application MAHJONGG,
//! values as text); the tile set is rs2's own, drawn with GPI
//! (tiles.rs). The help panels are OS/2's numbers (MAHJONGG.HLP).
//!
//! `MAHJONGG [file.MAH]` opens a game file; `MAHJONGG /SELFTEST` runs
//! the self-checks (selftest.rs).

#![no_std]
#![no_main]

extern crate alloc;

mod dialogs;
mod game;
mod layout;
mod selftest;
mod tiles;

use alloc::vec::Vec;

use game::Game;
use layout::{Layout, Pos};
use pmkit::gpi;
use pmkit::help::{Help, HelpWindow};
use pmkit::*;

os2::entry!(main);

// Menu commands (OS/2's ids).
pub const IDM_GAME: u16 = 1210;
pub const IDM_NEWDEAL: u16 = 1211;
pub const IDM_RANDOM: u16 = 1212;
pub const IDM_SAME: u16 = 1213;
pub const IDM_SELECT: u16 = 1214;
pub const IDM_DEFAULT: u16 = 1215;
pub const IDM_OPEN: u16 = 1216;
pub const IDM_SAVE: u16 = 1217;
pub const IDM_SAVEAS: u16 = 1218;
pub const IDM_AUTOPLAY: u16 = 1219;
pub const IDM_MOVES: u16 = 1220;
pub const IDM_TAKEBACK: u16 = 1221;
pub const IDM_REPLAY: u16 = 1222;
pub const IDM_SHOWREPLAY: u16 = 1223;
pub const IDM_CHEAT: u16 = 1224;
pub const IDM_SHOW: u16 = 1225;
pub const IDM_SHOWSAME: u16 = 1226;
pub const IDM_VIEW: u16 = 1230;
pub const IDM_PLAY: u16 = 1231;
pub const IDM_EDIT: u16 = 1232;
pub const IDM_OPTIONS: u16 = 1233;
pub const IDM_TIMER: u16 = 1234;
pub const IDM_MATCHES: u16 = 1235;
pub const IDM_SETTINGS: u16 = 1236;
pub const IDM_SAVESETTINGS: u16 = 1238;
pub const IDM_HELP: u16 = 150;
pub const IDM_HELPCONTENTS: u16 = 152;
pub const IDM_KEYSHELP: u16 = 153;
pub const IDM_USINGHELP: u16 = 154;
pub const IDM_GENERALHELP: u16 = 1292;
pub const IDM_GAMEINFO: u16 = 1298;
pub const IDM_PRODUCT: u16 = 1999;
pub const IDM_EXIT: u16 = 1200;

/// Windows with help (OS/2's ids, which its help table uses).
pub const FRAME_ID: u32 = 1000;
pub const DLG_EDIT: u16 = 1230;
pub const DLG_PRODUCT: u16 = 3500;
pub const DLG_NOMOVES: u16 = 3540;
pub const DLG_SETTINGS: u16 = 3600;
pub const DLG_GAMEINFO: u16 = 3720;
pub const DLG_LEVEL: u16 = 3900;
pub const DLG_SELECT: u16 = 3910;
/// The panel of the keys.
pub const PANEL_KEYS: u16 = 1001;

pub const HELP_FILE: &[u8] = b"MAHJONGG.HLP";
pub const TITLE: &[u8] = b"Mahjongg Solitaire";
const INI_FILE: &[u8] = b"MAHJONGG.INI";
const INI_APP: &[u8] = b"MAHJONGG";

/// Edit mode ends (mp1: keep the layout).
pub const WM_LEAVE_EDIT: u32 = WM_USER + 1;

const TIMER_CLOCK: u32 = 1;
const TIMER_AUTO: u32 = 2;
const TIMER_HINT: u32 = 3;

#[derive(Clone, Copy, PartialEq)]
pub struct Settings {
    pub beep: bool,
    pub solvable: bool,
    /// 0 free hints, 1 subtle hints (one tile of a pair), 2 none.
    pub hint_mode: u32,
    pub auto_hints: bool,
    /// 0: the tiles' sides at the lower right, 1: at the lower left.
    pub view3d: u32,
    /// Auto play and replay speed: 0 slow, 1 medium, 2 fast.
    pub speed: u32,
    pub use_back: bool,
    pub back: u32,
    pub timer: bool,
    pub matches: bool,
}

impl Settings {
    pub const DEFAULT: Settings = Settings { beep: true, solvable: true, hint_mode: 0, auto_hints: false, view3d: 0, speed: 1, use_back: false, back: 0x2a7a50, timer: true, matches: true };
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Play,
    Edit,
}

pub struct App {
    pub hab: u32,
    pub frame: u32,
    pub client: u32,
    pub menu: u32,
    pub help: Option<&'static mut Help>,
    pub layout: Layout,
    /// The game file it came from (None: the built-in layout).
    pub path: Option<Vec<u8>>,
    pub layout_changed: bool,
    pub game: Game,
    pub selected: Option<usize>,
    /// Tiles shown as a hint.
    pub hint: Vec<usize>,
    pub hint_next: usize,
    pub cheat: bool,
    pub over: bool,
    pub settings: Settings,
    pub saved_settings: Settings,
    pub seconds: u32,
    /// Auto play or a replay running: the moves still to show.
    pub auto: bool,
    pub replay: Option<(Vec<game::Move>, usize)>,
    pub mode: Mode,
    pub edit_level: i32,
    pub edit_backup: Option<Layout>,
    pub edit_dlg: u32,
    // Geometry of the last paint.
    pub w: i32,
    pub h: i32,
    pub d: i32,
    pub ox: i32,
    pub oy: i32,
    pub back: Option<gpi::MemPs>,
}

static mut APP: Option<App> = None;

pub fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

pub fn trace(s: &[u8]) {
    pmkit::trace("mahjongg", s);
}

unsafe extern "C" {
    #[link_name = "PMSHAPI.102"]
    fn PrfOpenProfile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMSHAPI.103"]
    fn PrfCloseProfile(hini: u32) -> u32;
}

fn main(s: &os2::Startup) -> u32 {
    let arg = pmkit::arg(s);
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if hmq == 0 {
        return 1;
    }
    let layout = Layout::default_layout();
    let game = Game::deal(&layout, 1, true);
    unsafe {
        APP = Some(App {
            hab,
            frame: 0,
            client: 0,
            menu: 0,
            help: None,
            layout,
            path: None,
            layout_changed: false,
            game,
            selected: None,
            hint: Vec::new(),
            hint_next: 0,
            cheat: false,
            over: false,
            settings: Settings::DEFAULT,
            saved_settings: Settings::DEFAULT,
            seconds: 0,
            auto: false,
            replay: None,
            mode: Mode::Play,
            edit_level: 0,
            edit_backup: None,
            edit_dlg: 0,
            w: 0,
            h: 0,
            d: 0,
            ox: 0,
            oy: 0,
            back: None,
        })
    };
    if arg.eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return fails;
    }
    let a = app();
    let first = !load_settings();
    a.saved_settings = a.settings;
    if !create_window() {
        return 1;
    }
    if !arg.is_empty() && !arg.starts_with(b"/") {
        open_file(arg);
    }
    new_deal(random_number());
    show_at_saved(hab, a.frame, INI_APP, b"WINDOWPOS", Some((720, 560)));
    if first {
        // The first time: how much help the player wants.
        dialogs::initial_level();
        a.saved_settings.hint_mode = a.settings.hint_mode;
        save_settings();
    }
    unsafe { WinStartTimer(hab, a.client, TIMER_CLOCK, 1000) };
    loop {
        message_loop(hab);
        if ask_save_on_exit() {
            break;
        }
        unsafe { WinCancelShutdown(hmq, 0) };
    }
    save_position(a.frame, INI_APP, b"WINDOWPOS");
    unsafe {
        if let Some(h) = a.help.as_mut() {
            h.destroy(a.frame);
        }
        a.back = None;
        WinDestroyWindow(a.frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

/// Before ending: a changed game file and changed settings may be
/// saved. False when the user cancelled.
fn ask_save_on_exit() -> bool {
    let a = app();
    if a.layout_changed {
        match message(a.frame, b"This game layout has been changed. Do you want to save it?", TITLE, MB_YESNOCANCEL | MB_ICONQUESTION) {
            MBID_YES => {
                if !save_layout(false) {
                    return false;
                }
            }
            MBID_CANCEL => return false,
            _ => {}
        }
    }
    if a.settings != a.saved_settings {
        match message(a.frame, b"You have changed the settings. Do you want to keep them for the next game?", TITLE, MB_YESNOCANCEL | MB_ICONQUESTION) {
            MBID_YES => save_settings(),
            MBID_CANCEL => return false,
            _ => {}
        }
    }
    true
}

// ---- Settings (MAHJONGG.INI) ----

fn ini_get(h: u32, key: &[u8]) -> Option<Vec<u8>> {
    let k = z(key);
    let mut buf = [0u8; 260];
    let n = unsafe { PrfQueryProfileString(h, z(INI_APP).as_ptr(), k.as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), 260) } as usize;
    let v = until_nul(&buf[..n.min(260)]).to_vec();
    (!v.is_empty()).then_some(v)
}

fn ini_num(h: u32, key: &[u8], d: u32) -> u32 {
    ini_get(h, key).and_then(|v| parse_num(&v)).map_or(d, |n| n as u32)
}

/// Read the settings; false if there were none (the first start).
fn load_settings() -> bool {
    let a = app();
    let f = z(INI_FILE);
    let h = unsafe { PrfOpenProfile(a.hab, f.as_ptr()) };
    if h == 0 {
        return false;
    }
    let found = ini_get(h, b"ErrorBeep").is_some();
    if found {
        let s = &mut a.settings;
        let d = Settings::DEFAULT;
        s.beep = ini_num(h, b"ErrorBeep", d.beep as u32) != 0;
        s.solvable = ini_num(h, b"Solvable", d.solvable as u32) != 0;
        s.hint_mode = ini_num(h, b"HintMode", d.hint_mode).min(2);
        s.auto_hints = ini_num(h, b"AutoHints", d.auto_hints as u32) != 0;
        s.view3d = ini_num(h, b"3DView", d.view3d).min(1);
        s.speed = ini_num(h, b"Speed", d.speed).min(2);
        s.use_back = ini_num(h, b"UseBackClr", d.use_back as u32) != 0;
        s.timer = ini_num(h, b"Timer", d.timer as u32) != 0;
        s.matches = ini_num(h, b"Status", d.matches as u32) != 0;
        if let Some(c) = ini_get(h, b"BackClr").and_then(|v| parse_rgb_text(&v)) {
            s.back = c;
        }
    }
    unsafe { PrfCloseProfile(h) };
    found
}

/// "RGB(r,g,b)"
pub fn parse_rgb_text(v: &[u8]) -> Option<u32> {
    let s = v.to_ascii_uppercase();
    let at = s.windows(4).position(|w| w == b"RGB(")? + 4;
    let mut c = 0u32;
    let mut rest = &s[at..];
    for _ in 0..3 {
        let n = parse_num(rest)?.clamp(0, 255) as u32;
        c = c << 8 | n;
        let i = rest.iter().position(|&ch| ch == b',' || ch == b')').unwrap_or(rest.len());
        rest = &rest[(i + 1).min(rest.len())..];
    }
    Some(c)
}

pub fn save_settings() {
    let a = app();
    let f = z(INI_FILE);
    let h = unsafe { PrfOpenProfile(a.hab, f.as_ptr()) };
    if h == 0 {
        return;
    }
    let s = a.settings;
    let put = |k: &[u8], v: &[u8]| {
        let (ka, kk, vv) = (z(INI_APP), z(k), z(v));
        unsafe { PrfWriteProfileString(h, ka.as_ptr(), kk.as_ptr(), vv.as_ptr()) };
    };
    let b = |v: bool| if v { &b"1"[..] } else { &b"0"[..] };
    put(b"ErrorBeep", b(s.beep));
    put(b"Solvable", b(s.solvable));
    put(b"HintMode", &num(s.hint_mode as i64));
    put(b"AutoHints", b(s.auto_hints));
    put(b"3DView", &num(s.view3d as i64));
    put(b"Speed", &num(s.speed as i64));
    put(b"UseBackClr", b(s.use_back));
    put(b"Timer", b(s.timer));
    put(b"Status", b(s.matches));
    put(b"BackClr", alloc::format!("RGB({},{},{})", s.back >> 16, (s.back >> 8) & 0xff, s.back & 0xff).as_bytes());
    unsafe { PrfCloseProfile(h) };
    a.saved_settings = s;
    trace(b"settings saved");
}

// ---- The window ----

fn menu_items() -> Vec<Menu<'static>> {
    alloc::vec![
        Menu::Sub(IDM_GAME, b"~Game", alloc::vec![
            Menu::Sub(IDM_NEWDEAL, b"~New deal\tCtrl+N", alloc::vec![
                Menu::Item(IDM_RANDOM, b"~Random"),
                Menu::Item(IDM_SAME, b"~Same"),
                Menu::Item(IDM_SELECT, b"Se~lect..."),
            ]),
            Menu::Sep,
            Menu::Item(IDM_DEFAULT, b"~Default"),
            Menu::Item(IDM_OPEN, b"~Open..."),
            Menu::Item(IDM_SAVE, b"~Save"),
            Menu::Item(IDM_SAVEAS, b"S~ave as..."),
            Menu::Sep,
            Menu::Item(IDM_AUTOPLAY, b"Auto ~play\tCtrl+P"),
        ]),
        Menu::Sub(IDM_MOVES, b"~Moves", alloc::vec![
            Menu::Item(IDM_TAKEBACK, b"~Take back\tCtrl+T"),
            Menu::Item(IDM_REPLAY, b"~Replay\tCtrl+R"),
            Menu::Item(IDM_SHOWREPLAY, b"Show repla~y\tCtrl+Y"),
            Menu::Sep,
            Menu::Item(IDM_CHEAT, b"~Cheat\tCtrl+C"),
            Menu::Sep,
            Menu::Item(IDM_SHOW, b"~Show\tCtrl+S"),
        ]),
        Menu::Sub(IDM_VIEW, b"~View", alloc::vec![
            Menu::Check(IDM_PLAY, b"~Play", true),
            Menu::Item(IDM_EDIT, b"~Edit"),
        ]),
        Menu::Sub(IDM_OPTIONS, b"~Options", alloc::vec![
            Menu::Check(IDM_TIMER, b"~Timer", app().settings.timer),
            Menu::Check(IDM_MATCHES, b"~Matches", app().settings.matches),
            Menu::Sep,
            Menu::Item(IDM_SETTINGS, b"~Settings..."),
            Menu::Item(IDM_SAVESETTINGS, b"S~ave settings"),
        ]),
        Menu::Sub(IDM_HELP, b"~Help", alloc::vec![
            Menu::Item(IDM_HELPCONTENTS, b"~Contents"),
            Menu::Item(IDM_GENERALHELP, b"~General help"),
            Menu::Item(IDM_KEYSHELP, b"~Keys help"),
            Menu::Item(IDM_USINGHELP, b"~Using help"),
            Menu::Sep,
            Menu::Item(IDM_GAMEINFO, b"Game ~information"),
            Menu::Sep,
            Menu::Item(IDM_PRODUCT, b"~Product information"),
        ]),
    ]
}

fn create_window() -> bool {
    let a = app();
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(a.hab, b"MahjonggClient", client_proc, CS_SIZEREDRAW, 0, fcf, TITLE, FRAME_ID);
    if frame == 0 {
        return false;
    }
    a.frame = frame;
    a.client = client;
    a.menu = set_menu(frame, &menu_items());
    let c = AF_CHAR | AF_CONTROL;
    set_accels(
        a.hab,
        frame,
        &[
            (c, b'c' as u16, IDM_CHEAT),
            (c, b'C' as u16, IDM_CHEAT),
            (c, b'n' as u16, IDM_RANDOM),
            (c, b'N' as u16, IDM_RANDOM),
            (c, b'p' as u16, IDM_AUTOPLAY),
            (c, b'P' as u16, IDM_AUTOPLAY),
            (c, b'r' as u16, IDM_REPLAY),
            (c, b'R' as u16, IDM_REPLAY),
            (c, b'y' as u16, IDM_SHOWREPLAY),
            (c, b'Y' as u16, IDM_SHOWREPLAY),
            (c, b's' as u16, IDM_SHOW),
            (c, b'S' as u16, IDM_SHOW),
            (c, b't' as u16, IDM_TAKEBACK),
            (c, b'T' as u16, IDM_TAKEBACK),
            (AF_VIRTUALKEY | AF_ALT, VK_BACKSPACE, IDM_TAKEBACK),
        ],
    );
    // Each menu item's panel is its own number, but New deal's items and
    // Save settings share their menu's.
    let mut items: Vec<(u16, u16)> = Vec::new();
    for id in [IDM_GAME, IDM_NEWDEAL, IDM_DEFAULT, IDM_OPEN, IDM_SAVE, IDM_SAVEAS, IDM_AUTOPLAY, IDM_MOVES, IDM_TAKEBACK, IDM_REPLAY, IDM_SHOWREPLAY, IDM_CHEAT, IDM_SHOW, IDM_SHOWSAME, IDM_VIEW, IDM_OPTIONS, IDM_TIMER, IDM_MATCHES, IDM_SETTINGS, IDM_HELP, IDM_USINGHELP, IDM_GENERALHELP, IDM_KEYSHELP, IDM_PRODUCT] {
        items.push((id, id));
    }
    items.extend_from_slice(&[(IDM_RANDOM, IDM_NEWDEAL), (IDM_SAME, IDM_NEWDEAL), (IDM_SELECT, IDM_NEWDEAL), (IDM_PLAY, IDM_VIEW), (IDM_EDIT, IDM_VIEW), (IDM_SAVESETTINGS, IDM_OPTIONS), (IDM_HELPCONTENTS, IDM_HELP), (IDM_GAMEINFO, IDM_HELP)]);
    let items: &'static [(u16, u16)] = alloc::boxed::Box::leak(items.into_boxed_slice());
    let edit_items: &'static [(u16, u16)] = &[(1, DLG_EDIT), (2, DLG_EDIT), (3701, DLG_EDIT), (3702, DLG_EDIT), (3703, DLG_EDIT), (3705, DLG_EDIT), (3706, DLG_EDIT)];
    a.help = Help::create(
        a.hab,
        frame,
        HELP_FILE,
        b"Mahjongg Solitaire - Help",
        &[
            HelpWindow { id: FRAME_ID as u16, ext: 1000, items },
            HelpWindow { id: DLG_EDIT, ext: DLG_EDIT, items: edit_items },
            HelpWindow { id: DLG_SETTINGS, ext: DLG_SETTINGS, items: &[(1, DLG_SETTINGS), (2, DLG_SETTINGS)] },
            HelpWindow { id: DLG_NOMOVES, ext: DLG_NOMOVES, items: &[(3541, DLG_NOMOVES), (3542, DLG_NOMOVES), (IDM_EXIT, DLG_NOMOVES), (2, DLG_NOMOVES)] },
            HelpWindow { id: DLG_LEVEL, ext: DLG_LEVEL, items: &[(3901, DLG_LEVEL), (3902, DLG_LEVEL), (3903, DLG_LEVEL), (1, DLG_LEVEL), (3904, DLG_LEVEL)] },
            HelpWindow { id: DLG_SELECT, ext: DLG_SELECT, items: &[(3911, DLG_SELECT), (1, DLG_SELECT), (2, DLG_SELECT), (3912, DLG_SELECT)] },
            HelpWindow { id: DLG_GAMEINFO, ext: DLG_GAMEINFO, items: &[(1, DLG_GAMEINFO), (2, DLG_GAMEINFO)] },
            HelpWindow { id: DLG_PRODUCT, ext: IDM_PRODUCT, items: &[(1, IDM_PRODUCT)] },
        ],
    );
    if a.help.is_none() {
        trace(b"no help file");
    }
    true
}

pub fn set_title() {
    let a = app();
    let mut t = if a.layout.name.is_empty() { b"Mahjongg Solitaire - (Untitled)".to_vec() } else { a.layout.name.clone() };
    if a.mode == Mode::Edit {
        t.extend_from_slice(b" - Edit");
    }
    set_text(a.frame, &t);
}

/// Menu items that depend on the state.
fn update_menu() {
    let a = app();
    let play = a.mode == Mode::Play;
    let busy = a.auto || a.replay.is_some();
    menu_check(a.menu, IDM_PLAY, play);
    menu_check(a.menu, IDM_EDIT, !play);
    menu_check(a.menu, IDM_CHEAT, a.cheat);
    menu_check(a.menu, IDM_TIMER, a.settings.timer);
    menu_check(a.menu, IDM_MATCHES, a.settings.matches);
    menu_check(a.menu, IDM_AUTOPLAY, a.auto);
    for id in [IDM_NEWDEAL, IDM_DEFAULT, IDM_OPEN, IDM_TAKEBACK, IDM_REPLAY, IDM_SHOWREPLAY, IDM_CHEAT, IDM_SHOW] {
        menu_enable(a.menu, id, play && !busy);
    }
    menu_enable(a.menu, IDM_AUTOPLAY, play && a.replay.is_none());
    menu_enable(a.menu, IDM_TAKEBACK, play && !busy && !a.game.moves.is_empty());
    menu_enable(a.menu, IDM_SHOWREPLAY, play && !busy && !a.game.moves.is_empty());
    menu_enable(a.menu, IDM_SHOW, play && !busy && a.settings.hint_mode != 2);
    menu_enable(a.menu, IDM_SAVE, play);
    menu_enable(a.menu, IDM_SAVEAS, play);
}

pub fn beep() {
    if app().settings.beep {
        unsafe { WinAlarm(HWND_DESKTOP, 0) };
    }
}

/// A game number for Random.
fn random_number() -> u32 {
    let mut r = Rng::new();
    1 + r.below(30000)
}

/// Deal game `number` on the current layout.
pub fn new_deal(number: u32) {
    let a = app();
    stop_auto();
    a.game = Game::deal(&a.layout, number, a.settings.solvable);
    reset_play();
    trace(alloc::format!("deal {} on {} ({} tiles), {} matches", number, core::str::from_utf8(&a.layout.name).unwrap_or("?"), a.game.left(), a.game.match_count()).as_bytes());
}

fn reset_play() {
    let a = app();
    a.selected = None;
    a.hint.clear();
    a.hint_next = 0;
    a.cheat = false;
    a.over = false;
    a.seconds = 0;
    set_title();
    update_menu();
    invalidate(a.client);
}

fn stop_auto() {
    let a = app();
    a.auto = false;
    a.replay = None;
    unsafe { WinStopTimer(a.hab, a.client, TIMER_AUTO) };
}

fn auto_interval() -> u32 {
    [900, 450, 150][app().settings.speed.min(2) as usize]
}

// ---- Files ----

/// Open a game file; false (after saying why) if it cannot be used.
pub fn open_file(path: &[u8]) -> bool {
    let a = app();
    let Some(text) = read_file(path) else {
        let m = [&b"The file "[..], path, b" cannot be read."].concat();
        message(a.frame, &m, TITLE, MB_OK | MB_ERROR);
        return false;
    };
    let l = match Layout::parse(&text) {
        Ok(l) => l,
        // OS/2's DEFAULT.MAH stands for the built-in game.
        Err(_) if file_name(path).eq_ignore_ascii_case(b"DEFAULT.MAH") => Layout::default_layout(),
        Err(e) => {
            message(a.frame, &[path, b": ", &e].concat(), TITLE, MB_OK | MB_ERROR);
            return false;
        }
    };
    if let Err(e) = l.check() {
        message(a.frame, &[path, b": ", &e].concat(), TITLE, MB_OK | MB_ERROR);
        return false;
    }
    a.layout = l;
    a.path = Some(path.to_vec());
    a.layout_changed = false;
    a.back = None;
    trace(&[&b"opened "[..], path].concat());
    true
}

/// Save the layout (Save, or Save as with `ask`); false if not saved.
pub fn save_layout(ask: bool) -> bool {
    let a = app();
    let path = match (&a.path, ask) {
        (Some(p), false) => p.clone(),
        _ => match dialogs::file_dialog(true) {
            Some(p) => p,
            None => return false,
        },
    };
    let mut path = path;
    if extension(&path).is_empty() {
        path.extend_from_slice(b".MAH");
    }
    let new = a.path.as_deref() != Some(&path[..]);
    if let Err(rc) = write_file(&path, &a.layout.write()) {
        message(a.frame, alloc::format!("The game could not be saved (error {rc}).").as_bytes(), TITLE, MB_OK | MB_ERROR);
        return false;
    }
    a.path = Some(path.clone());
    a.layout_changed = false;
    trace(&[&b"saved "[..], &path].concat());
    if new && message(a.frame, b"Do you want a shadow of this new game in the Mahjongg folder?", TITLE, MB_YESNO | MB_ICONQUESTION) == MBID_YES {
        let title = if a.layout.name.is_empty() { file_name(&path).to_vec() } else { a.layout.name.clone() };
        let setup = [&b"SHADOWID="[..], &path].concat();
        if !create_object(b"WPShadow", &title, &setup, b"<MAH_FOLDER>", 0) {
            trace(b"no shadow made");
        }
    }
    true
}

// ---- Geometry and painting ----

/// Work out the tiles' size and the board's place in the window.
fn geometry() {
    let a = app();
    let r = window_rect(a.client);
    let (cw, ch) = (r.width().max(40), r.height().max(40));
    let l = &a.layout;
    let lv = l.levels().max(1);
    // Tiles three wide by four high; each level is a seventh of a tile
    // up and to the side.
    let wx = (cw - 16) * 7 / (7 * l.dx + lv + 1);
    let wy = (ch - 16) * 21 / (28 * l.dy + 3 * (lv + 1));
    let w = wx.min(wy).max(12);
    let h = w * 4 / 3;
    let d = (w / 7).max(2);
    a.w = w;
    a.h = h;
    a.d = d;
    let bw = l.dx * w + lv * d;
    let bh = l.dy * h + lv * d;
    a.ox = (cw - bw) / 2 + if a.settings.view3d == 0 { (lv - 1) * d } else { d };
    a.oy = (ch - bh) / 2 + d;
}

/// The lower left corner of a tile's face.
pub fn tile_xy(p: Pos) -> (i32, i32) {
    let a = app();
    let s = if a.settings.view3d == 0 { -1 } else { 1 };
    (a.ox + p.x * a.w / 2 + s * p.z * a.d, a.oy + p.y * a.h / 2 + p.z * a.d)
}

/// The tiles in drawing order (back to front).
fn draw_order(pos: &[Pos]) -> Vec<usize> {
    let left = app().settings.view3d != 0;
    let mut v: Vec<usize> = (0..pos.len()).collect();
    v.sort_by(|&i, &j| {
        let (p, q) = (pos[i], pos[j]);
        p.z.cmp(&q.z).then(q.y.cmp(&p.y)).then(if left { q.x.cmp(&p.x) } else { p.x.cmp(&q.x) })
    });
    v
}

const FACE_BG: i32 = 0xf4f0e0;
const SIDE: i32 = 0xc9b78f;
const SIDE_DARK: i32 = 0x8d7a55;
const EDGE: i32 = 0x50402a;

/// One tile: its body (thickness on the side away from the light), then
/// its face.
fn draw_tile(hps: u32, p: Pos, face: Option<u8>, bg: i32) {
    let a = app();
    let (x, y) = tile_xy(p);
    let (w, h, d) = (a.w, a.h, a.d);
    let dx = if a.settings.view3d == 0 { d } else { -d };
    // The body: the face's outline moved down and to the side.
    if dx > 0 {
        gpi::polygon(hps, &[pt(x + w - 1, y + h - 1), pt(x + w - 1 + dx, y + h - 1 - d), pt(x + w - 1 + dx, y - d), pt(x + w - 1, y)], SIDE);
    } else {
        gpi::polygon(hps, &[pt(x, y + h - 1), pt(x + dx, y + h - 1 - d), pt(x + dx, y - d), pt(x, y)], SIDE);
    }
    gpi::polygon(hps, &[pt(x, y), pt(x + dx, y - d), pt(x + w - 1 + dx, y - d), pt(x + w - 1, y)], SIDE_DARK);
    gpi::fill(hps, RectL::new(x, y, x + w, y + h), bg);
    if let Some(f) = face {
        tiles::draw_face(hps, f, x + 2, y + 2, w - 4, h - 4, bg);
    }
    gpi::frame(hps, RectL::new(x, y, x + w, y + h), EDGE);
}

fn background() -> u32 {
    let a = app();
    if a.settings.use_back { a.settings.back } else { a.layout.color }
}

fn paint(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, 0, &mut upd) };
    let r = window_rect(hwnd);
    let (cw, ch) = (r.width().max(1), r.height().max(1));
    if a.back.as_ref().is_none_or(|b| b.cx != cw || b.cy != ch) {
        a.back = gpi::MemPs::new(a.hab, cw, ch);
    }
    geometry();
    if let Some(b) = a.back.take() {
        draw_board(b.hps, cw, ch);
        b.blit_to(hps, 0, 0);
        a.back = Some(b);
    }
    unsafe { WinEndPaint(hps) };
}

fn draw_board(hps: u32, cw: i32, ch: i32) {
    let a = app();
    gpi::fill(hps, RectL::new(0, 0, cw, ch), background() as i32);
    tiles::begin(hps);
    match a.mode {
        Mode::Play => {
            let order = draw_order(&a.game.pos);
            for i in order {
                if !a.game.present[i] {
                    continue;
                }
                let bg = if a.selected == Some(i) {
                    0xf7d75a
                } else if a.hint.contains(&i) {
                    0xa6e3a1
                } else {
                    FACE_BG
                };
                draw_tile(hps, a.game.pos[i], Some(a.game.face[i]), bg);
            }
        }
        Mode::Edit => {
            let order = draw_order(&a.layout.pos);
            for i in order {
                let p = a.layout.pos[i];
                if p.z > a.edit_level {
                    continue;
                }
                let bg = if p.z == a.edit_level { FACE_BG } else { 0xa0a0a0 };
                draw_tile(hps, p, None, bg);
            }
        }
    }
    draw_status(hps);
}

/// The status box (OS/2's: at the layout's x_stat, y_stat): the matches,
/// the time; in edit mode the level and the count of tiles.
fn draw_status(hps: u32) {
    let a = app();
    let mut lines: Vec<Vec<u8>> = Vec::new();
    match a.mode {
        Mode::Play => {
            if a.settings.matches {
                let n = a.game.match_count();
                lines.push(alloc::format!("{} match{}", n, if n == 1 { "" } else { "es" }).into_bytes());
            }
            if a.settings.timer {
                let s = a.seconds;
                lines.push(alloc::format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60).into_bytes());
            }
            if a.cheat {
                lines.push(b"Cheat".to_vec());
            }
        }
        Mode::Edit => {
            lines.push(alloc::format!("Level {}", a.edit_level).into_bytes());
            lines.push(alloc::format!("{} of 144", a.layout.pos.len()).into_bytes());
        }
    }
    if lines.is_empty() {
        return;
    }
    let fh = (a.h / 4).clamp(9, 20);
    gpi::select_font(hps, 2, b"Helvetica", fh);
    let lh = fh + 3;
    let bw = lines.iter().map(|l| gpi::text_width(hps, l)).max().unwrap_or(0) + 12;
    let bh = lh * lines.len() as i32 + 6;
    let x = a.ox + a.layout.x_stat * a.w / 10;
    let y = a.oy + a.layout.y_stat * a.h / 10;
    let r = RectL::new(x, y, x + bw, y + bh);
    gpi::fill(hps, RectL::new(x + 3, y - 3, x + bw + 3, y + bh - 3), 0x303030);
    gpi::fill(hps, r, 0xd8d8d0);
    gpi::bevel(hps, r, 0xffffff, 0x808080, 2);
    for (i, l) in lines.iter().enumerate() {
        let c = if i == 0 && a.mode == Mode::Play && a.settings.matches { 0xc4231b } else { 0x202020 };
        gpi::text_at(hps, x + 6, y + bh - 4 - (i as i32 + 1) * lh + 4, l, c);
    }
    unsafe { GpiSetCharSet(hps, 0) };
}

/// The tile under a point (the frontmost), in play or edit mode.
fn hit(x: i32, y: i32) -> Option<usize> {
    let a = app();
    let (pos, present): (&[Pos], Option<&[bool]>) = match a.mode {
        Mode::Play => (&a.game.pos, Some(&a.game.present)),
        Mode::Edit => (&a.layout.pos, None),
    };
    let order = draw_order(pos);
    for &i in order.iter().rev() {
        if present.is_some_and(|p| !p[i]) {
            continue;
        }
        if a.mode == Mode::Edit && pos[i].z > a.edit_level {
            continue;
        }
        let (tx, ty) = tile_xy(pos[i]);
        if x >= tx && x < tx + a.w && y >= ty && y < ty + a.h {
            return Some(i);
        }
    }
    None
}

// ---- Playing ----

fn click(x: i32, y: i32) {
    let a = app();
    if a.mode == Mode::Edit {
        edit_click(x, y);
        return;
    }
    if a.over || a.auto || a.replay.is_some() {
        return;
    }
    let Some(i) = hit(x, y) else {
        if a.selected.take().is_some() {
            a.hint.clear();
            invalidate(a.client);
        }
        return;
    };
    if !a.game.free(i) && !a.cheat {
        beep();
        return;
    }
    match a.selected {
        Some(s) if s == i => {
            a.selected = None;
            a.hint.clear();
        }
        Some(s) if game::matches(a.game.face[s], a.game.face[i]) => {
            let cheat = !a.game.free(s) || !a.game.free(i);
            a.game.remove(s, i, cheat);
            a.selected = None;
            a.hint.clear();
            a.cheat = false;
            trace(alloc::format!("took {} and {}{}, {} left, {} matches", s, i, if cheat { " (cheat)" } else { "" }, a.game.left(), a.game.match_count()).as_bytes());
            after_move();
        }
        _ => {
            if a.selected.is_some() {
                beep();
            }
            a.selected = Some(i);
            a.hint.clear();
            trace(alloc::format!("selected {} ({})", i, tiles::face_name(a.game.face[i])).as_bytes());
            if a.settings.auto_hints && a.settings.hint_mode != 2 {
                let f = a.game.face[i];
                a.hint = (0..a.game.pos.len()).filter(|&j| j != i && a.game.free(j) && game::matches(f, a.game.face[j])).collect();
            }
        }
    }
    update_menu();
    invalidate(a.client);
}

/// After tiles were taken off: the end of the game, or no moves left.
fn after_move() {
    let a = app();
    invalidate(a.client);
    unsafe { WinUpdateWindow(a.client) };
    if a.game.won() {
        a.over = true;
        stop_auto();
        let t = a.seconds;
        let cheats = a.game.cheats();
        let mut m = alloc::format!("All 144 tiles are off the board in {}:{:02}:{:02}.", t / 3600, t / 60 % 60, t % 60);
        match cheats {
            0 => m.push_str(" Well played!"),
            1 => m.push_str(" (One move was a cheat.)"),
            n => m.push_str(&alloc::format!(" ({n} moves were cheats.)")),
        }
        trace(b"game won");
        message(a.frame, m.as_bytes(), b"Congratulations", MB_OK | MB_INFORMATION);
        update_menu();
        return;
    }
    if a.game.match_count() == 0 && a.replay.is_none() {
        stop_auto();
        trace(b"no more moves");
        match dialogs::no_more_moves() {
            dialogs::NoMoves::New => new_deal(random_number()),
            dialogs::NoMoves::Retry => {
                a.game.restart();
                reset_play();
            }
            dialogs::NoMoves::Exit => {
                post(a.frame, WM_CLOSE, 0, 0);
            }
            dialogs::NoMoves::Cancel => {}
        }
    }
}

/// Show: the next pair that can be taken (one tile of it for subtle
/// hints).
fn show_hint() {
    let a = app();
    let pairs = a.game.free_pairs();
    if pairs.is_empty() {
        beep();
        return;
    }
    let (p, q) = pairs[a.hint_next % pairs.len()];
    a.hint_next += 1;
    a.hint = if a.settings.hint_mode == 1 { alloc::vec![p] } else { alloc::vec![p, q] };
    unsafe { WinStartTimer(a.hab, a.client, TIMER_HINT, 2500) };
    invalidate(a.client);
}

/// Show same: every tile like the one selected (or under the pointer).
fn show_same(x: i32, y: i32) {
    let a = app();
    let Some(i) = a.selected.or_else(|| hit(x, y)) else {
        beep();
        return;
    };
    let f = a.game.face[i];
    a.hint = (0..a.game.pos.len()).filter(|&j| a.game.present[j] && game::kind(a.game.face[j]) == game::kind(f)).collect();
    unsafe { WinStartTimer(a.hab, a.client, TIMER_HINT, 2500) };
    invalidate(a.client);
}

fn auto_step() {
    let a = app();
    if let Some((moves, at)) = a.replay.as_mut() {
        if *at >= moves.len() {
            stop_auto();
            update_menu();
            return;
        }
        let m = moves[*at];
        *at += 1;
        a.game.remove(m.a, m.b, m.cheat);
        invalidate(a.client);
        return;
    }
    if !a.auto {
        return;
    }
    let pairs = a.game.free_pairs();
    match pairs.first() {
        Some(&(p, q)) => {
            a.game.remove(p, q, false);
            after_move();
        }
        None => {
            stop_auto();
            after_move();
        }
    }
    update_menu();
}

// ---- Edit mode ----

fn enter_edit() {
    let a = app();
    if a.mode == Mode::Edit {
        return;
    }
    stop_auto();
    a.mode = Mode::Edit;
    a.edit_backup = Some(a.layout.clone());
    a.edit_level = 0;
    a.edit_dlg = dialogs::edit_dialog();
    set_title();
    update_menu();
    invalidate(a.client);
}

/// End edit mode: keep the layout (`ok`, if it has 144 tiles) or go back.
pub fn leave_edit(ok: bool) -> bool {
    let a = app();
    if ok {
        if let Err(e) = a.layout.check() {
            message(a.edit_dlg, &e, TITLE, MB_OK | MB_WARNING);
            return false;
        }
        a.layout_changed = true;
        a.path = None;
    } else if let Some(l) = a.edit_backup.take() {
        a.layout = l;
    }
    a.edit_backup = None;
    a.mode = Mode::Play;
    if a.edit_dlg != 0 {
        unsafe { WinDestroyWindow(a.edit_dlg) };
        a.edit_dlg = 0;
    }
    new_deal(random_number());
    true
}

fn edit_click(x: i32, y: i32) {
    let a = app();
    let z = a.edit_level;
    if let Some(i) = hit(x, y).filter(|&i| a.layout.pos[i].z == z) {
        a.layout.pos.remove(i);
        invalidate(a.client);
        return;
    }
    if a.layout.pos.len() >= layout::TILES {
        message(a.frame, b"The layout has all 144 tiles.", TITLE, MB_OK | MB_INFORMATION);
        return;
    }
    // The half-tile place whose tile is centred nearest the pointer.
    let s = if a.settings.view3d == 0 { -1 } else { 1 };
    let fx = x - a.ox - s * z * a.d - a.w / 2;
    let fy = y - a.oy - z * a.d - a.h / 2;
    let px = (fx * 2 + a.w / 2).div_euclid(a.w);
    let py = (fy * 2 + a.h / 2).div_euclid(a.h);
    let p = Pos { x: px, y: py, z };
    if px < 0 || py < 0 || px > (a.layout.dx - 1) * 2 || py > (a.layout.dy - 1) * 2 {
        beep();
        return;
    }
    if a.layout.pos.iter().any(|&q| Layout::overlap(p, q)) {
        beep();
        return;
    }
    a.layout.pos.push(p);
    invalidate(a.client);
}

// ---- Commands ----

pub fn command(id: u16) {
    let a = app();
    match id {
        IDM_RANDOM => new_deal(random_number()),
        IDM_SAME => {
            let n = a.game.number;
            new_deal(n);
        }
        IDM_SELECT => {
            if let Some(n) = dialogs::select_game(a.game.number) {
                new_deal(n);
            }
        }
        IDM_DEFAULT => {
            a.layout = Layout::default_layout();
            a.path = None;
            a.layout_changed = false;
            new_deal(random_number());
        }
        IDM_OPEN => {
            if let Some(p) = dialogs::file_dialog(false) {
                if open_file(&p) {
                    new_deal(random_number());
                }
            }
        }
        IDM_SAVE => {
            save_layout(a.path.is_none());
        }
        IDM_SAVEAS => {
            save_layout(true);
        }
        IDM_AUTOPLAY => {
            if a.auto {
                stop_auto();
            } else if !a.over {
                a.auto = true;
                a.selected = None;
                unsafe { WinStartTimer(a.hab, a.client, TIMER_AUTO, auto_interval()) };
            }
            update_menu();
        }
        IDM_TAKEBACK => {
            if a.game.take_back().is_some() {
                a.over = false;
                a.selected = None;
                a.hint.clear();
                invalidate(a.client);
            } else {
                beep();
            }
            update_menu();
        }
        IDM_REPLAY => {
            a.game.restart();
            reset_play();
        }
        IDM_SHOWREPLAY => {
            let moves = a.game.moves.clone();
            a.game.restart();
            a.selected = None;
            a.hint.clear();
            a.replay = Some((moves, 0));
            unsafe { WinStartTimer(a.hab, a.client, TIMER_AUTO, auto_interval()) };
            update_menu();
            invalidate(a.client);
        }
        IDM_CHEAT => {
            a.cheat = !a.cheat;
            update_menu();
            invalidate(a.client);
        }
        IDM_SHOW => show_hint(),
        IDM_SHOWSAME => {
            let mut p = PointL::default();
            unsafe {
                WinQueryPointerPos(HWND_DESKTOP, &mut p);
                WinMapWindowPoints(HWND_DESKTOP, a.client, &mut p, 1);
            }
            show_same(p.x, p.y);
        }
        IDM_PLAY => {
            if a.mode == Mode::Edit {
                leave_edit(false);
            }
        }
        IDM_EDIT => enter_edit(),
        IDM_TIMER => {
            a.settings.timer = !a.settings.timer;
            update_menu();
            invalidate(a.client);
        }
        IDM_MATCHES => {
            a.settings.matches = !a.settings.matches;
            update_menu();
            invalidate(a.client);
        }
        IDM_SETTINGS => {
            if dialogs::settings() {
                a.back = None;
                update_menu();
                invalidate(a.client);
            }
        }
        IDM_SAVESETTINGS => save_settings(),
        IDM_HELPCONTENTS => help(HM_HELP_CONTENTS, 0, 0),
        IDM_GENERALHELP => help(HM_EXT_HELP, 0, 0),
        IDM_KEYSHELP => help(HM_KEYS_HELP, 0, 0),
        IDM_USINGHELP => help(HM_DISPLAY_HELP, 0, 0),
        IDM_GAMEINFO => dialogs::game_info(),
        IDM_PRODUCT => dialogs::product_info(),
        _ => {}
    }
}

pub fn help(msg: u32, mp1: u32, mp2: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, mp1, mp2);
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_SIZE => {
            invalidate(hwnd);
            0
        }
        WM_BUTTON1DOWN => {
            unsafe { WinSetFocus(HWND_DESKTOP, hwnd) };
            let (x, y) = mouse_xy(mp1);
            click(x, y);
            1
        }
        WM_CONTEXTMENU => {
            if a.mode == Mode::Play {
                popup(hwnd, mp1, hi(mp2) != 0);
            }
            1
        }
        WM_TIMER => {
            match lo(mp1) {
                TIMER_CLOCK => {
                    if a.mode == Mode::Play && !a.over && a.replay.is_none() && !a.game.moves.is_empty() {
                        a.seconds += 1;
                        if a.settings.timer {
                            invalidate(hwnd);
                        }
                    }
                }
                TIMER_AUTO => auto_step(),
                TIMER_HINT => {
                    unsafe { WinStopTimer(a.hab, hwnd, TIMER_HINT) };
                    if !a.hint.is_empty() && a.selected.is_none() {
                        a.hint.clear();
                        invalidate(hwnd);
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            command(lo(mp1) as u16);
            0
        }
        WM_LEAVE_EDIT => {
            if a.mode == Mode::Edit {
                leave_edit(mp1 != 0);
            }
            0
        }
        WM_HELP => 0,
        HM_QUERY_KEYS_HELP => PANEL_KEYS as u32,
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

/// The pop-up menu (OS/2's: New deal, Auto play, Moves, Show, Show
/// same).
fn popup(hwnd: u32, mp1: u32, keyboard: bool) {
    let items = alloc::vec![
        Menu::Sub(IDM_GAME, b"~Game", alloc::vec![
            Menu::Sub(IDM_NEWDEAL, b"~New deal", alloc::vec![
                Menu::Item(IDM_RANDOM, b"~Random"),
                Menu::Item(IDM_SAME, b"~Same"),
                Menu::Item(IDM_SELECT, b"Se~lect..."),
            ]),
            Menu::Item(IDM_AUTOPLAY, b"~Auto play"),
        ]),
        Menu::Sub(IDM_MOVES, b"~Moves", alloc::vec![
            Menu::Item(IDM_TAKEBACK, b"~Take back"),
            Menu::Item(IDM_REPLAY, b"~Replay"),
            Menu::Item(IDM_SHOWREPLAY, b"Show repla~y"),
            Menu::Sep,
            Menu::Item(IDM_CHEAT, b"~Cheat"),
        ]),
        Menu::Sep,
        Menu::Item(IDM_SHOW, b"~Show"),
        Menu::Item(IDM_SHOWSAME, b"Show s~ame"),
    ];
    let m = popup_menu(hwnd, &items);
    if m == 0 {
        return;
    }
    let (x, y) = if keyboard { (20, 20) } else { mouse_xy(mp1) };
    unsafe { WinPopupMenu(hwnd, hwnd, m, x, y, 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2) };
}
