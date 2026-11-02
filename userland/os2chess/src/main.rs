//! OS2CHESS.EXE: rs2's OS/2 Chess, a re-implementation of OS/2's.
//!
//! As OS/2's (studied in its binary and resources): the board in a 3-D
//! view (turned with View > Rotate 3-D board, flipped, labelled) or a
//! flat 2-D view; OS/2 Chess's menus (Game: New, Load, Save, Delete,
//! Print; Moves: Take back, Move status, Game record, Valid moves,
//! Captured pieces; View; Options: Timer, Keyboard input, Set players,
//! Set position, Settings, Save settings; Network; Help) with its command
//! ids, accelerators and help panel numbers; the board's pop-up menu;
//! Set Players (human or computer for each side, names, the computer's
//! playing level and book openings), Pawn Promotion, Player to Move,
//! the modeless Game Record, Move Status, Time Elapsed and Keyboard
//! windows; OS/2 Chess's messages ("Check!", "Checkmate!!!  White
//! wins!", the draw rules, why a move is invalid). Games are saved one
//! per `*.PMC` file in the current directory (rs2's own text format:
//! the players, the date and the moves); settings in OS2.INI
//! (application OS2CHESS). Network play is not there (rs2 has no
//! NetBIOS/APPC partner to play with): the Network menu stays disabled.
//!
//! The rules and the computer player are in engine.rs (its own code);
//! the board and the pieces (rs2's own shapes) in draw.rs.
//! `OS2CHESS /SELFTEST` checks the engine (perft counts, mates).

#![no_std]
#![no_main]

extern crate alloc;

mod dialogs;
mod draw;
mod engine;
mod game;
mod selftest;

use alloc::vec::Vec;

use pmkit::gpi;
use pmkit::help::{Help, HelpWindow};
use pmkit::*;

use draw::{Colors, Layout, Marks, View};
use engine::*;
use game::Game;

os2::entry!(main);

// Menu ids (OS/2 Chess's).
pub const IDM_GAME: u16 = 8100;
pub const IDM_NEW: u16 = 8101;
pub const IDM_LOAD: u16 = 8102;
pub const IDM_SAVE: u16 = 8103;
pub const IDM_DELETE: u16 = 8104;
pub const IDM_PRINT: u16 = 8105;
pub const IDM_MOVES: u16 = 8106;
pub const IDM_OPTIONS: u16 = 8107;
pub const IDM_PLAYERS: u16 = 8108;
pub const IDM_SETPOS: u16 = 8109;
pub const IDM_LABEL: u16 = 8113;
pub const IDM_KEYBOARD: u16 = 8114;
pub const IDM_TIMER: u16 = 8115;
pub const IDM_TAKEBACK: u16 = 8116;
pub const IDM_VIEW: u16 = 8117;
pub const IDM_STATUS: u16 = 8118;
pub const IDM_RECORD: u16 = 8119;
pub const IDM_VALID: u16 = 8120;
pub const IDM_CAPTURED: u16 = 8122;
pub const IDM_ROTATE: u16 = 8123;
pub const IDM_FLIP: u16 = 8124;
pub const IDM_NETWORK: u16 = 8125;
pub const IDM_SENDMSG: u16 = 8126;
pub const IDM_DISCONNECT: u16 = 8127;
pub const IDM_USERS: u16 = 8128;
pub const IDM_SETTINGS: u16 = 8130;
pub const IDM_SHOWMOVES: u16 = 8148;
pub const IDM_SAVESETTINGS: u16 = 1238;
pub const IDM_HELP: u16 = 150;
pub const IDM_HELPINDEX: u16 = 151;
pub const IDM_HELPCONTENTS: u16 = 152;
pub const IDM_KEYSHELP: u16 = 153;
pub const IDM_USINGHELP: u16 = 154;
pub const IDM_GENERALHELP: u16 = 8200;
pub const IDM_PRODUCT: u16 = 8224;
/// rs2's: the pieces of the Set position pop-up menu (9000 + piece + 6).
pub const IDM_PUT: u16 = 9000;

pub const FRAME_ID: u32 = 1000;
pub const HELP_FILE: &[u8] = b"OS2CHESS.HLP";
pub const TITLE: &[u8] = b"OS/2 Chess";
pub const INI_APP: &[u8] = b"OS2CHESS";

/// The computer's answer (mp1: the move packed, mp2: the request).
pub const WM_COMPUTERMOVE: u32 = WM_USER + 1;
const TIMER_CLOCK: u32 = 1;

/// A side's player.
#[derive(Clone)]
pub struct Player {
    pub computer: bool,
    pub name: Vec<u8>,
    /// 0 Beginner .. 4 Advanced.
    pub level: u32,
}

pub struct App {
    pub hab: u32,
    pub frame: u32,
    pub client: u32,
    pub menu: u32,
    pub help: Option<&'static mut Help>,
    pub game: Game,
    pub players: [Player; 2],
    pub book: bool,
    pub view: View,
    pub scheme: usize,
    pub warnings: bool,
    pub show_moves: bool,
    /// The square picked up by the user.
    pub selected: Option<u8>,
    /// Set position mode.
    pub setting: bool,
    pub status: Vec<u8>,
    pub thinking: bool,
    /// Which computer request is current (an older answer is dropped).
    pub request: u32,
    pub think_start: u32,
    /// Milliseconds each side has used, and when the clock last ticked.
    pub elapsed: [u32; 2],
    pub tick: u32,
    pub record_dlg: u32,
    pub status_dlg: u32,
    pub timer_dlg: u32,
    pub keyboard_dlg: u32,
    pub buffer: Option<gpi::MemPs>,
    pub layout: Option<Layout>,
}

static mut APP: Option<App> = None;

pub fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

pub fn trace(s: &[u8]) {
    pmkit::trace("os2chess", s);
}

/// The millisecond clock.
pub fn ms() -> u32 {
    let mut v = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(14, 14, v.as_mut_ptr(), 4) };
    v[0]
}

pub fn side_index(side: i8) -> usize {
    if side == WHITE { 0 } else { 1 }
}

fn main(s: &os2::Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if hmq == 0 {
        return 1;
    }
    if arg(s).eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return fails;
    }
    unsafe {
        APP = Some(App {
            hab,
            frame: 0,
            client: 0,
            menu: 0,
            help: None,
            game: Game::new(),
            players: [Player { computer: false, name: b"White".to_vec(), level: 2 }, Player { computer: true, name: b"Computer".to_vec(), level: 2 }],
            book: true,
            view: View { three_d: true, flipped: false, angle: 0, labels: true },
            scheme: 0,
            warnings: true,
            show_moves: false,
            selected: None,
            setting: false,
            status: Vec::new(),
            thinking: false,
            request: 0,
            think_start: 0,
            elapsed: [0; 2],
            tick: 0,
            record_dlg: 0,
            status_dlg: 0,
            timer_dlg: 0,
            keyboard_dlg: 0,
            buffer: None,
            layout: None,
        })
    };
    load_settings();
    let a = app();
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(hab, b"OS2Chess", client_proc, CS_SIZEREDRAW, 0, fcf, TITLE, FRAME_ID);
    if frame == 0 {
        return 1;
    }
    a.frame = frame;
    a.client = client;
    a.menu = set_menu(frame, &menu_items());
    set_accels(
        hab,
        frame,
        &[
            (AF_CHAR | AF_CONTROL, b'n' as u16, IDM_NEW),
            (AF_CHAR | AF_CONTROL, b'N' as u16, IDM_NEW),
            (AF_CHAR | AF_CONTROL, b'p' as u16, IDM_PRINT),
            (AF_CHAR | AF_CONTROL, b'P' as u16, IDM_PRINT),
            (AF_CHAR | AF_CONTROL, b'l' as u16, IDM_LABEL),
            (AF_CHAR | AF_CONTROL, b'L' as u16, IDM_LABEL),
            (AF_CHAR | AF_CONTROL, b't' as u16, IDM_TAKEBACK),
            (AF_CHAR | AF_CONTROL, b'T' as u16, IDM_TAKEBACK),
            (AF_CHAR | AF_CONTROL, b's' as u16, IDM_STATUS),
            (AF_CHAR | AF_CONTROL, b'S' as u16, IDM_STATUS),
            (AF_CHAR | AF_CONTROL, b'r' as u16, IDM_RECORD),
            (AF_CHAR | AF_CONTROL, b'R' as u16, IDM_RECORD),
            (AF_CHAR | AF_CONTROL, b'v' as u16, IDM_VALID),
            (AF_CHAR | AF_CONTROL, b'V' as u16, IDM_VALID),
            (AF_CHAR | AF_CONTROL, b'c' as u16, IDM_CAPTURED),
            (AF_CHAR | AF_CONTROL, b'C' as u16, IDM_CAPTURED),
        ],
    );
    a.help = Help::create(hab, frame, HELP_FILE, b"Help for OS/2 Chess", &help_windows());
    update_menu();
    show_at_saved(hab, frame, INI_APP, b"WINDOWPOS", Some((620, 580)));
    unsafe { WinStartTimer(hab, client, TIMER_CLOCK, 1000) };
    a.tick = ms();
    new_status();
    // OS/2 Chess asks who plays when it starts.
    dialogs::players();
    if !app().thinking {
        next_turn();
    }
    message_loop(hab);
    let a = app();
    a.request = a.request.wrapping_add(1);
    game::abort_thinking();
    save_position(frame, INI_APP, b"WINDOWPOS");
    unsafe {
        if let Some(h) = a.help.as_mut() {
            h.destroy(frame);
        }
        a.buffer = None;
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

fn menu_items() -> Vec<Menu<'static>> {
    let a = app();
    alloc::vec![
        Menu::Sub(IDM_GAME, b"~Game", alloc::vec![
            Menu::Item(IDM_NEW, b"~New\tCtrl+N"),
            Menu::Sep,
            Menu::Item(IDM_LOAD, b"~Load..."),
            Menu::Item(IDM_SAVE, b"~Save"),
            Menu::Item(IDM_DELETE, b"~Delete..."),
            Menu::Sep,
            Menu::Item(IDM_PRINT, b"~Print\tCtrl+P"),
        ]),
        Menu::Sub(IDM_MOVES, b"~Moves", alloc::vec![
            Menu::Item(IDM_TAKEBACK, b"Take ~back\tCtrl+T"),
            Menu::Sep,
            Menu::Item(IDM_STATUS, b"~Move status...\tCtrl+S"),
            Menu::Item(IDM_RECORD, b"~Game record...\tCtrl+R"),
            Menu::Item(IDM_VALID, b"~Valid moves...\tCtrl+V"),
            Menu::Item(IDM_CAPTURED, b"~Captured pieces...\tCtrl+C"),
        ]),
        Menu::Sub(IDM_VIEW, b"~View", alloc::vec![
            Menu::Item(IDM_ROTATE, b"~Rotate 3-D board..."),
            Menu::Check(IDM_FLIP, b"~Flip board", a.view.flipped),
            Menu::Check(IDM_LABEL, b"~Label board\tCtrl+L", a.view.labels),
        ]),
        Menu::Sub(IDM_OPTIONS, b"~Options", alloc::vec![
            Menu::Item(IDM_TIMER, b"~Timer..."),
            Menu::Item(IDM_KEYBOARD, b"~Keyboard input..."),
            Menu::Sep,
            Menu::Item(IDM_PLAYERS, b"Set ~players..."),
            Menu::Item(IDM_SETPOS, b"Set p~osition"),
            Menu::Sep,
            Menu::Item(IDM_SETTINGS, b"~Settings..."),
            Menu::Item(IDM_SAVESETTINGS, b"S~ave settings"),
        ]),
        Menu::Sub(IDM_NETWORK, b"~Network", alloc::vec![
            Menu::Disabled(IDM_SENDMSG, b"~Send message..."),
            Menu::Disabled(IDM_DISCONNECT, b"~Disconnect"),
            Menu::Disabled(IDM_USERS, b"~List users..."),
        ]),
        Menu::Sub(IDM_HELP, b"~Help", alloc::vec![
            Menu::Item(IDM_HELPCONTENTS, b"~Contents"),
            Menu::Item(IDM_HELPINDEX, b"~Index"),
            Menu::Item(IDM_GENERALHELP, b"~General help"),
            Menu::Item(IDM_KEYSHELP, b"~Keys help"),
            Menu::Item(IDM_USINGHELP, b"~Using help"),
            Menu::Sep,
            Menu::Item(IDM_PRODUCT, b"~Product information"),
        ]),
    ]
}

/// The help tables: OS/2 Chess's windows and panel numbers.
fn help_windows() -> Vec<HelpWindow<'static>> {
    let main: &'static [(u16, u16)] = &[
        (IDM_GAME, 8100),
        (IDM_NEW, 8101),
        (IDM_LOAD, 8102),
        (IDM_SAVE, 8103),
        (IDM_DELETE, 8104),
        (IDM_PRINT, 8105),
        (IDM_MOVES, 8106),
        (IDM_OPTIONS, 8107),
        (IDM_PLAYERS, 8108),
        (IDM_SETPOS, 8109),
        (IDM_LABEL, 8113),
        (IDM_SHOWMOVES, 8148),
        (IDM_KEYBOARD, 8114),
        (IDM_TIMER, 8115),
        (IDM_TAKEBACK, 8116),
        (IDM_VIEW, 8117),
        (IDM_STATUS, 8118),
        (IDM_RECORD, 8119),
        (IDM_VALID, 8120),
        (IDM_CAPTURED, 8122),
        (IDM_ROTATE, 8123),
        (IDM_FLIP, 8124),
        (IDM_NETWORK, 8125),
        (IDM_SENDMSG, 8126),
        (IDM_DISCONNECT, 8127),
        (IDM_USERS, 8128),
        (IDM_HELPINDEX, 64026),
        (IDM_GENERALHELP, 8200),
        (IDM_KEYSHELP, 8228),
        (IDM_USINGHELP, 64023),
        (IDM_HELPCONTENTS, 64027),
        (IDM_PRODUCT, 8224),
        (IDM_SETTINGS, 8130),
        (IDM_SAVESETTINGS, 8107),
    ];
    alloc::vec![
        HelpWindow { id: FRAME_ID as u16, ext: 8200, items: main },
        HelpWindow { id: dialogs::DLG_LOAD, ext: 8102, items: &[(1, 8220), (2, 8221)] },
        HelpWindow { id: dialogs::DLG_DELETE, ext: 8104, items: &[(1, 8223), (2, 8221)] },
        HelpWindow { id: dialogs::DLG_PLAYERS, ext: 8108, items: &[(1, 8220), (2, 8221)] },
        HelpWindow { id: dialogs::DLG_LEVEL, ext: 8108, items: &[(1, 8220), (2, 8221)] },
        HelpWindow { id: dialogs::DLG_SETTINGS, ext: 8130, items: &[(1, 8220), (2, 8221)] },
        HelpWindow { id: dialogs::DLG_TIMER, ext: 8115, items: &[] },
        HelpWindow { id: dialogs::DLG_RECORD, ext: 8119, items: &[] },
        HelpWindow { id: dialogs::DLG_STATUS, ext: 8118, items: &[] },
        HelpWindow { id: dialogs::DLG_KEYBOARD, ext: 8114, items: &[(2, 8221)] },
        HelpWindow { id: dialogs::DLG_VALID, ext: 8120, items: &[] },
        HelpWindow { id: dialogs::DLG_CAPTURED, ext: 8122, items: &[] },
        HelpWindow { id: dialogs::DLG_ROTATE, ext: 8123, items: &[] },
        HelpWindow { id: dialogs::DLG_PROMOTE, ext: 8204, items: &[(1, 8220)] },
        HelpWindow { id: dialogs::DLG_TOMOVE, ext: 8160, items: &[(1, 8220)] },
        HelpWindow { id: dialogs::DLG_PRODUCT, ext: 8224, items: &[(1, 8224)] },
    ]
}

// ---- Settings ----

fn load_settings() {
    let a = app();
    let get = |k: &[u8], d: i32| ini::read_int(INI_APP, k, d);
    for (i, side) in [b"WHITE", b"BLACK"].iter().enumerate() {
        let p = &mut a.players[i];
        p.computer = get(&[&side[..], b"PLAYER"].concat(), p.computer as i32) != 0;
        p.level = get(&[&side[..], b"LEVEL"].concat(), p.level as i32).clamp(0, 4) as u32;
        if let Some(n) = ini::read_str(INI_APP, &[&side[..], b"NAME"].concat())
            && !n.is_empty()
        {
            p.name = n;
        }
    }
    a.book = get(b"BOOK", 1) != 0;
    a.view.three_d = get(b"VIEW", 3) != 2;
    a.view.flipped = get(b"FLIPPED", 0) != 0;
    a.view.angle = get(b"ANGLE", 0).rem_euclid(360);
    a.view.labels = get(b"LABELS", 1) != 0;
    a.scheme = (get(b"COLORS", 0).max(0) as usize).min(draw::SCHEMES.len() - 1);
    a.warnings = get(b"WARNINGS", 1) != 0;
}

pub fn save_settings() {
    let a = app();
    for (i, side) in [b"WHITE", b"BLACK"].iter().enumerate() {
        let p = &a.players[i];
        ini::write_int(INI_APP, &[&side[..], b"PLAYER"].concat(), p.computer as i32);
        ini::write_int(INI_APP, &[&side[..], b"LEVEL"].concat(), p.level as i32);
        ini::write_str(INI_APP, &[&side[..], b"NAME"].concat(), &p.name);
    }
    ini::write_int(INI_APP, b"BOOK", a.book as i32);
    ini::write_int(INI_APP, b"VIEW", if a.view.three_d { 3 } else { 2 });
    ini::write_int(INI_APP, b"FLIPPED", a.view.flipped as i32);
    ini::write_int(INI_APP, b"ANGLE", a.view.angle);
    ini::write_int(INI_APP, b"LABELS", a.view.labels as i32);
    ini::write_int(INI_APP, b"COLORS", a.scheme as i32);
    ini::write_int(INI_APP, b"WARNINGS", a.warnings as i32);
    trace(b"settings saved");
}

pub fn colors() -> Colors {
    draw::SCHEMES[app().scheme].1
}

// ---- The game flow ----

/// The status line for the position.
pub fn new_status() {
    let a = app();
    let side = a.game.pos.side;
    let who = &a.players[side_index(side)];
    let mut t = Vec::new();
    if a.setting {
        t.extend_from_slice(b"Set board: click a square to put a piece on it; choose Set position again when done.");
    } else if let Some(over) = a.game.over {
        t.extend_from_slice(&game::result_text(over, side));
    } else {
        t.extend_from_slice(if side == WHITE { b"White" } else { b"Black" });
        t.extend_from_slice(b" (");
        t.extend_from_slice(&who.name);
        t.extend_from_slice(b") to move");
        if a.thinking {
            t.extend_from_slice(b" - thinking...");
        } else if a.game.pos.in_check(side) {
            t.extend_from_slice(b" - Check!");
        }
    }
    a.status = t;
    invalidate(a.client);
}

/// After a move or a change: end the game if it is over, else let the
/// computer play if it is its turn.
pub fn next_turn() {
    let a = app();
    a.selected = None;
    update_menu();
    dialogs::refresh_record();
    if a.setting {
        new_status();
        return;
    }
    if a.game.over.is_none() {
        let st = a.game.pos.status();
        match st {
            Status::Normal => {}
            Status::Check => {
                new_status();
                if a.warnings && !a.players[side_index(a.game.pos.side)].computer {
                    // OS/2 Chess says so in the status; a beep too.
                    unsafe { WinAlarm(HWND_DESKTOP, 0) };
                }
            }
            over => {
                a.game.over = Some(over);
                new_status();
                trace(&game::result_text(over, a.game.pos.side));
                message(a.frame, &game::result_text(over, a.game.pos.side), TITLE, MB_OK | MB_INFORMATION);
                return;
            }
        }
    }
    new_status();
    if a.game.over.is_none() && a.players[side_index(a.game.pos.side)].computer {
        game::start_thinking();
        new_status();
    }
}

/// Play a move (from the user or the computer).
pub fn play(m: Move) {
    let a = app();
    let text = a.game.play(m);
    let mut t = b"move ".to_vec();
    t.extend_from_slice(&text);
    trace(&t);
    next_turn();
}

pub fn update_menu() {
    let a = app();
    let m = a.menu;
    let idle = !a.thinking;
    menu_enable(m, IDM_TAKEBACK, !a.game.moves.is_empty() && !a.setting);
    menu_enable(m, IDM_VALID, idle && a.game.over.is_none() && !a.setting);
    menu_enable(m, IDM_SAVE, !a.setting);
    menu_check(m, IDM_SETPOS, a.setting);
    menu_check(m, IDM_FLIP, a.view.flipped);
    menu_check(m, IDM_LABEL, a.view.labels);
    menu_enable(m, IDM_ROTATE, a.view.three_d);
}

/// Start again from the beginning (asking first about a game going on).
pub fn new_game(ask: bool) {
    let a = app();
    if ask && !a.game.moves.is_empty() && a.game.over.is_none() && message(a.frame, b"Save current game?", TITLE, MB_YESNO | MB_ICONQUESTION) == MBID_YES {
        game::save_game();
    }
    a.request = a.request.wrapping_add(1);
    game::abort_thinking();
    a.game = Game::new();
    a.setting = false;
    a.elapsed = [0; 2];
    next_turn();
}

// ---- Drawing ----

fn status_height() -> i32 {
    let hps = unsafe { WinGetPS(app().client) };
    let (h, _) = gpi::font_height(hps);
    unsafe { WinReleasePS(hps) };
    h + 8
}

fn paint(hwnd: u32) {
    let a = app();
    let r = window_rect(hwnd);
    let hps = unsafe { WinBeginPaint(hwnd, 0, core::ptr::null_mut()) };
    let (w, h) = (r.width().max(1), r.height().max(1));
    if a.buffer.as_ref().is_none_or(|b| b.cx != w || b.cy != h) {
        a.buffer = gpi::MemPs::new(a.hab, w, h);
    }
    let sh = status_height();
    let board_area = RectL::new(0, sh, w, h);
    let l = draw::layout(board_area, &a.view);
    let c = colors();
    let targets: Vec<u8> = match a.selected {
        Some(s) if a.show_moves => a.game.pos.legal_moves().iter().filter(|m| m.from == s).map(|m| m.to).collect(),
        _ => Vec::new(),
    };
    let last = a.game.moves.last().map(|m| (m.from, m.to));
    let target = a.buffer.as_ref().map_or(hps, |b| b.hps);
    gpi::rgb_mode(target);
    draw::board(target, &l, &a.game.pos, &a.view, &c, &Marks { selected: a.selected, targets: &targets, last });
    // The status line.
    gpi::fill(target, RectL::new(0, 0, w, sh), 0xd0d0d0);
    gpi::color(target, 0x808080);
    gpi::line(target, 0, sh - 1, w, sh - 1);
    let (_, desc) = gpi::font_height(target);
    gpi::text_at(target, 6, 4 + desc, &a.status, 0x000000);
    if let Some(b) = a.buffer.as_ref() {
        b.blit_to(hps, 0, 0);
    }
    a.layout = Some(l);
    unsafe { WinEndPaint(hps) };
}

// ---- The user's moves ----

fn click(x: i32, y: i32) {
    let a = app();
    let Some(l) = a.layout.as_ref() else { return };
    // A piece of the side to move is picked up by its shape, other
    // clicks go to the square under the pointer.
    let own = if a.setting { None } else { draw::piece_at(l, &a.game.pos, a.game.pos.side, x, y) };
    let Some(sq) = own.or_else(|| draw::square_at(l, x, y)) else { return };
    if a.setting {
        dialogs::piece_menu(sq, x, y);
        return;
    }
    if a.game.over.is_some() || a.thinking || a.players[side_index(a.game.pos.side)].computer {
        unsafe { WinAlarm(HWND_DESKTOP, 0) };
        return;
    }
    let pc = a.game.pos.sq[sq as usize];
    match a.selected {
        None => {
            if pc == 0 {
                return;
            }
            if (pc > 0) != (a.game.pos.side > 0) {
                invalid(b"Wrong piece color!");
                return;
            }
            a.selected = Some(sq);
            invalidate(a.client);
        }
        Some(from) if from == sq => {
            a.selected = None;
            invalidate(a.client);
        }
        Some(from) => {
            if pc != 0 && (pc > 0) == (a.game.pos.side > 0) {
                // Another of one's own pieces: pick it up instead.
                a.selected = Some(sq);
                invalidate(a.client);
                return;
            }
            try_move(from, sq);
        }
    }
}

/// The user's move from `from` to `to` (asking for the promotion piece).
pub fn try_move(from: u8, to: u8) {
    let a = app();
    let moves = a.game.pos.legal_moves();
    let cands: Vec<Move> = moves.iter().copied().filter(|m| m.from == from && m.to == to).collect();
    if cands.is_empty() {
        let why = game::why_invalid(&mut a.game.pos, from, to);
        a.selected = None;
        invalid(&why);
        invalidate(a.client);
        return;
    }
    let m = if cands.len() > 1 {
        let kind = dialogs::promotion();
        cands.iter().copied().find(|m| m.promo.abs() == kind).unwrap_or(cands[0])
    } else {
        cands[0]
    };
    play(m);
}

pub fn invalid(why: &[u8]) {
    let a = app();
    let mut t = b"Invalid move: ".to_vec();
    t.extend_from_slice(why);
    trace(&t);
    if a.warnings {
        message(a.frame, &t, TITLE, MB_OK | MB_WARNING);
    } else {
        unsafe { WinAlarm(HWND_DESKTOP, 0) };
    }
}

// ---- Commands ----

fn command(id: u16) {
    let a = app();
    match id {
        IDM_NEW => new_game(true),
        IDM_LOAD => dialogs::load_dialog(false),
        IDM_DELETE => dialogs::load_dialog(true),
        IDM_SAVE => game::save_game(),
        IDM_PRINT => game::print(),
        IDM_TAKEBACK => game::take_back(),
        IDM_STATUS => dialogs::status_window(),
        IDM_RECORD => dialogs::record_window(),
        IDM_VALID => dialogs::valid_moves(),
        IDM_CAPTURED => dialogs::captured(),
        IDM_ROTATE => dialogs::rotate(),
        IDM_FLIP => {
            a.view.flipped = !a.view.flipped;
            update_menu();
            invalidate(a.client);
        }
        IDM_LABEL => {
            a.view.labels = !a.view.labels;
            update_menu();
            invalidate(a.client);
        }
        IDM_SHOWMOVES => {
            a.show_moves = !a.show_moves;
            invalidate(a.client);
        }
        IDM_TIMER => dialogs::timer_window(),
        IDM_KEYBOARD => dialogs::keyboard_window(),
        IDM_PLAYERS => dialogs::players(),
        IDM_SETPOS => dialogs::set_position(),
        IDM_SETTINGS => dialogs::settings(),
        IDM_SAVESETTINGS => save_settings(),
        IDM_HELPINDEX => help_request(HM_HELP_INDEX),
        IDM_HELPCONTENTS => help_request(HM_HELP_CONTENTS),
        IDM_GENERALHELP => help_request(HM_EXT_HELP),
        IDM_KEYSHELP => help_request(HM_KEYS_HELP),
        IDM_USINGHELP => help_request(HM_DISPLAY_HELP),
        IDM_PRODUCT => product_info(a.frame, dialogs::DLG_PRODUCT, b"OS/2 Chess - Product Information", b"OS/2 Chess", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Chess against the computer or another person."]),
        _ if (IDM_PUT..IDM_PUT + 13).contains(&id) => dialogs::put_piece((id - IDM_PUT) as i8 - 6),
        _ => {}
    }
}

fn help_request(msg: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, 0, 0);
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBACKGROUND => 0,
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
        WM_BUTTON2DOWN | WM_CONTEXTMENU => {
            let (x, y) = if msg == WM_BUTTON2DOWN { mouse_xy(mp1) } else { (60, 60) };
            dialogs::board_menu(x, y);
            1
        }
        WM_COMMAND => {
            command(lo(mp1) as u16);
            0
        }
        WM_COMPUTERMOVE => {
            if mp2 == a.request && a.thinking {
                a.thinking = false;
                dialogs::refresh_status(true);
                let m = game::unpack(mp1);
                if !m.is_none() {
                    play(m);
                } else {
                    next_turn();
                }
            }
            0
        }
        WM_TIMER if lo(mp1) == TIMER_CLOCK => {
            let now = ms();
            if a.game.over.is_none() && !a.setting && !a.game.moves.is_empty() {
                a.elapsed[side_index(a.game.pos.side)] += now.wrapping_sub(a.tick);
            }
            a.tick = now;
            dialogs::refresh_timer();
            if a.thinking {
                dialogs::refresh_status(false);
            }
            0
        }
        WM_CLOSE => {
            if !a.game.moves.is_empty() && a.game.over.is_none() {
                match message(a.frame, b"Save current game?", TITLE, MB_YESNOCANCEL | MB_ICONQUESTION) {
                    MBID_YES => game::save_game(),
                    MBID_CANCEL => return 0,
                    _ => {}
                }
            }
            post(hwnd, WM_QUIT, 0, 0);
            0
        }
        HM_QUERY_KEYS_HELP => 8228,
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}
